//! Menedżer sektorów GPU — device/queue, pipeline compute’owy, bufory
//! i prawa dostępu Płatków.
//!
//! Sektor (bufor) to podstawowa jednostkę pamięci na GPU:
//!
//! | Sektor | Binding | Zawartość |
//! |---|---|---|
//! | roboczy | `@binding(0)` | `array<Neuron>` |
//! | systemowy | `@binding(1)` | `array<u32>` — offsety sektorów hex |
//! | konfiguracja | `@binding(2)` | `Config` — liczba neuronów, krok czasowy |
//!
//! [`GpuSectorManager::create_petal_bind_group`] to mechanizm uprawnień:
//! Płatek dostaje bind grupę złożoną wyłącznie z sektorów, do których ma prawo
//! ([`SectorAccessRights`]). Gdy prawa brakuje, w miejsce bufora wstawiany jest
//! mały bufor zastępczy — odczyty poza jego zakresem zwracają zera, a zapisy
//! są odrzucane przez odporność `wgpu` (robust buffer access). Dzięki temu
//! izolacja działa po stronie GPU, bez przepisywania danych na CPU.

use std::cell::Cell;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Wake, Waker};

use crate::error::{Error, Result};
use crate::neuron_topology::{Neuron, RunConfig, Topology};

/// Rozmiar grupy roboczej — musi zgadzać się z `@workgroup_size(64)`
/// w `neuron_pipeline.wgsl`.
const WORKGROUP_SIZE: u32 = 64;

/// Wspólna etykieta zasobów (widoczna w narzędziach debugowania GPU).
const LABEL: &str = "neuro";

/// Prawo Płatka do jednego z sektorów GPU.
///
/// Odpowiada numerowi bindingu w bind grupie — patrz
/// [`SectorAccessRights::binding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectorAccessRights {
    /// Sektor roboczy neuronów — `@binding(0)`.
    NeuronWorkspace,
    /// Sektor systemowy (mapa sektorów hex) — `@binding(1)`.
    SystemMap,
    /// Sektor konfiguracji — `@binding(2)`.
    Config,
}

impl SectorAccessRights {
    /// Numer bindingu w bind grupie.
    pub fn binding(self) -> u32 {
        match self {
            Self::NeuronWorkspace => 0,
            Self::SystemMap => 1,
            Self::Config => 2,
        }
    }
}

/// Menedżer sektorów: trzyma device, queue, pipeline i bufory trzech sektorów.
///
/// Tworzony raz na początku symulacji ([`GpuSectorManager::new`]); kolejne
/// kroki to [`GpuSectorManager::execute_neuron_pipeline`] i — gdy potrzeba
/// danych na CPU — [`GpuSectorManager::read_neurons`].
pub struct GpuSectorManager {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,

    // Bufory reprezentujące sektory hex w VRAM.
    neurons_buffer: wgpu::Buffer,    // Sektor roboczy neuronów
    sector_map_buffer: wgpu::Buffer, // Sektor systemowy (mapa)
    config_buffer: wgpu::Buffer,     // Sektor konfiguracji

    // Bufory zastępcze dla Płatków bez prawa dostępu do sektora.
    denied_neurons_buffer: wgpu::Buffer,
    denied_sector_map_buffer: wgpu::Buffer,
    denied_config_buffer: wgpu::Buffer,

    // Bufor odczytu (MAP_READ) — kopia sektora roboczego na CPU.
    staging_buffer: wgpu::Buffer,

    total_neurons: u32,
    config: Cell<RunConfig>,
}

impl GpuSectorManager {
    /// Tworzy menedżera: waliduje shader, wybiera adapter GPU, buduje bufory
    /// sektorów i pipeline compute’owy.
    ///
    /// * `shader_source` — kod WGSL z punktem wejścia `main`,
    /// * `neurons` — stan początkowy (dokładnie `topology.total_neurons()` sztuk),
    /// * `topology` — mapa sektorów hex przesyłana do shadera jako `sector_map`,
    /// * `time_step` — krok czasowy zapisywany do buforu konfiguracji.
    pub fn new(
        shader_source: &str,
        neurons: &[Neuron],
        topology: &Topology,
        time_step: f32,
    ) -> Result<Self> {
        if neurons.is_empty() {
            return Err(Error::Topology("liczba neuronów nie może być zerowa".into()));
        }
        if neurons.len() != topology.total_neurons() as usize {
            return Err(Error::Topology(format!(
                "topologia opisuje {} neuronów, a przekazano {}",
                topology.total_neurons(),
                neurons.len()
            )));
        }

        // Błąd WGSL ma wrócić teraz, z numerem linii, a nie jako panic w tle.
        validate_shader(shader_source)?;
        let (info, device, queue) = request_device()?;

        let total_neurons = neurons.len() as u32;
        let config = RunConfig::new(total_neurons, time_step);

        let storage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let uniform = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;

        let neurons_buffer = create_buffer(
            &device,
            &queue,
            "neuro.neurons",
            bytemuck::cast_slice(neurons),
            storage,
        );
        let sector_map_buffer = create_buffer(
            &device,
            &queue,
            "neuro.sector_map",
            bytemuck::cast_slice(topology.offsets()),
            storage,
        );
        let config_buffer =
            create_buffer(&device, &queue, "neuro.config", bytemuck::cast_slice(&[config]), uniform);

        // Bufory zastępcze wypełnione zerami. Muszą być co najmniej tak duże,
        // jak minimalny rozmiar bindingu w shaderze, inaczej `wgpu` odrzuci
        // dispatch: `array<Neuron>` wymaga 48 B, `Config` i `array<u32>` — 16 B.
        let denied_neurons_buffer = empty_buffer(
            &device,
            &queue,
            "neuro.denied.neurons",
            std::mem::size_of::<Neuron>() as u64,
            storage,
        );
        let denied_sector_map_buffer =
            empty_buffer(&device, &queue, "neuro.denied.sector_map", 16, storage);
        let denied_config_buffer = empty_buffer(
            &device,
            &queue,
            "neuro.denied.config",
            std::mem::size_of::<RunConfig>() as u64,
            uniform,
        );

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("neuro.staging"),
            size: std::mem::size_of::<Neuron>() as u64 * u64::from(total_neurons),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Layout musi zgadzać się z deklaracjami w WGSL:
        // 0 — storage read_write, 1 — storage read, 2 — uniform.
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(LABEL),
            entries: &[storage_entry(0, false), storage_entry(1, true), uniform_entry(2)],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(LABEL),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(LABEL),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(LABEL),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        log::info!(
            "neuro-lib: adapter `{}` ({}) — {} neuronów w {} sektorach",
            info.name,
            info.backend,
            total_neurons,
            topology.sector_count()
        );

        Ok(Self {
            device,
            queue,
            info,
            bind_group_layout,
            pipeline,
            neurons_buffer,
            sector_map_buffer,
            config_buffer,
            denied_neurons_buffer,
            denied_sector_map_buffer,
            denied_config_buffer,
            staging_buffer,
            total_neurons,
            config: Cell::new(config),
        })
    }

    /// Informacje o adapterze (nazwa, backend, typ urządzenia).
    pub fn info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    /// Device GPU — do zasobów spoza menedżera.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Queue GPU — do przesyłania danych spoza menedżera.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Layout bindingów używany przez pipeline (`0`/`1`/`2`).
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// Liczba neuronów w sektorze roboczym.
    pub fn total_neurons(&self) -> u32 {
        self.total_neurons
    }

    /// Bieżąca konfiguracja uruchomienia (kopia bufora uniform).
    pub fn config(&self) -> RunConfig {
        self.config.get()
    }

    /// Nadpisuje cały sektor roboczy danymi z hosta.
    pub fn write_neurons(&self, neurons: &[Neuron]) -> Result<()> {
        if neurons.len() != self.total_neurons as usize {
            return Err(Error::Topology(format!(
                "bufor neuronów ma {} elementów, a sektor ma {}",
                neurons.len(),
                self.total_neurons
            )));
        }
        self.queue
            .write_buffer(&self.neurons_buffer, 0, bytemuck::cast_slice(neurons));
        Ok(())
    }

    /// Aktualizuje bufor konfiguracji (uniform) i kopię trzymaną w menedżerze.
    pub fn set_config(&self, config: &RunConfig) -> Result<()> {
        if config.total_neurons > self.total_neurons {
            return Err(Error::Topology(format!(
                "konfiguracja mówi o {} neuronach, a sektor ma {}",
                config.total_neurons, self.total_neurons
            )));
        }
        self.queue
            .write_buffer(&self.config_buffer, 0, bytemuck::cast_slice(&[*config]));
        self.config.set(*config);
        Ok(())
    }

    /// Tworzy Bind Group dla konkretnego Płatka (Komponentu Aurole).
    ///
    /// TO JEST NASZ MECHANIZM UPRAWNIEŃ NA GPU: każdy z trzech bindingów
    /// dostaje prawdziwy bufor sektora albo — gdy Płatek nie ma prawa —
    /// bufor zastępczy z zerami. Płatek bez `Config` dostaje `total_neurons = 0`,
    /// więc shader kończy pracę od razu.
    ///
    /// ```rust
    /// # use neuro_lib::{GpuSectorManager, SectorAccessRights};
    /// # fn demonstrate(manager: &GpuSectorManager) -> wgpu::BindGroup {
    /// // Płatek widzi tylko mapę sektorów — nie ma dostępu do neuronów.
    /// manager.create_petal_bind_group(&[SectorAccessRights::SystemMap])
    /// # }
    /// ```
    pub fn create_petal_bind_group(
        &self,
        allowed_sectors: &[SectorAccessRights],
    ) -> wgpu::BindGroup {
        let entries = [
            wgpu::BindGroupEntry {
                binding: SectorAccessRights::NeuronWorkspace.binding(),
                resource: pick_buffer(
                    allowed_sectors,
                    SectorAccessRights::NeuronWorkspace,
                    &self.neurons_buffer,
                    &self.denied_neurons_buffer,
                )
                .as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: SectorAccessRights::SystemMap.binding(),
                resource: pick_buffer(
                    allowed_sectors,
                    SectorAccessRights::SystemMap,
                    &self.sector_map_buffer,
                    &self.denied_sector_map_buffer,
                )
                .as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: SectorAccessRights::Config.binding(),
                resource: pick_buffer(
                    allowed_sectors,
                    SectorAccessRights::Config,
                    &self.config_buffer,
                    &self.denied_config_buffer,
                )
                .as_entire_binding(),
            },
        ];

        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Petal Compute Bind Group"),
            layout: &self.bind_group_layout,
            entries: &entries,
        })
    }
}

impl GpuSectorManager {
    /// Uruchamia kernel na GPU dla konkretnego Płatka.
    ///
    /// `total_neurons` to liczba neuronów do obsłużenia (zero wygasza dispatch).
    pub fn execute_neuron_pipeline(&self, bind_group: &wgpu::BindGroup, total_neurons: u32) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("neuro.encoder"),
        });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("neuro.compute_pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, bind_group, &[]);

            // Obliczenie liczby grup roboczych (workgroups).
            compute_pass.dispatch_workgroups(workgroups(total_neurons), 1, 1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Wykonuje `steps` kroków symulacji zadanym Płatkiem.
    pub fn run_steps(&self, bind_group: &wgpu::BindGroup, steps: u32) {
        for _ in 0..steps {
            self.execute_neuron_pipeline(bind_group, self.total_neurons);
        }
    }

    /// Czeka, aż GPU skończy wcześniej wysłane komendy.
    ///
    /// Wywołuj przed [`GpuSectorManager::read_neurons`] — mapowanie buforów
    /// jest asynchroniczne.
    pub fn poll(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|error| Error::Poll(error.to_string()))
    }

    /// Kopiuje sektor roboczy na CPU i zwraca aktualny stan neuronów.
    pub fn read_neurons(&self) -> Result<Vec<Neuron>> {
        let bytes = std::mem::size_of::<Neuron>() as u64 * u64::from(self.total_neurons);

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("neuro.readback"),
        });
        encoder.copy_buffer_to_buffer(&self.neurons_buffer, 0, &self.staging_buffer, 0, bytes);
        self.queue.submit(std::iter::once(encoder.finish()));
        self.poll()?;

        let slice = self.staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.poll()?;
        receiver
            .recv()
            .map_err(|error| Error::Map(format!("callback mapowania nie przysiedł: {error}")))?
            .map_err(|error| Error::Map(error.to_string()))?;

        let neurons = {
            let view = slice
                .get_mapped_range()
                .map_err(|error| Error::Map(error.to_string()))?;
            bytemuck::cast_slice::<u8, Neuron>(&view).to_vec()
        };
        self.staging_buffer.unmap();

        Ok(neurons)
    }
}

/// Liczba grup roboczych dla `total_neurons` neuronów (`workgroup_size` = 64).
fn workgroups(total_neurons: u32) -> u32 {
    total_neurons.div_ceil(WORKGROUP_SIZE)
}

/// Wybiera prawdziwy bufor sektora albo bufor zastępczy — sedno mechanizmu
/// uprawnień Płatków.
fn pick_buffer<'a>(
    allowed_sectors: &[SectorAccessRights],
    rights: SectorAccessRights,
    real: &'a wgpu::Buffer,
    denied: &'a wgpu::Buffer,
) -> &'a wgpu::Buffer {
    if allowed_sectors.contains(&rights) {
        real
    } else {
        denied
    }
}

/// Waliduje WGSL przez `naga` — błąd wraca z numerem linii, zanim `wgpu`
/// zdąży zapanikować w tle przy tworzeniu pipeline’u.
pub fn validate_shader(source: &str) -> Result<()> {
    let module = wgpu::naga::front::wgsl::parse_str(source).map_err(|error| Error::Shader {
        label: LABEL.to_owned(),
        message: error.emit_to_string(source),
    })?;

    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| Error::Shader {
        label: LABEL.to_owned(),
        message: error.emit_to_string(source),
    })?;

    Ok(())
}

/// Wybiera adapter i prosi o urządzenie — najpierw najlepszy adapter,
/// a gdy go brak (np. serwer bez GPU), adapter zastępczy (software).
pub fn request_device() -> Result<(wgpu::AdapterInfo, wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let options = |force_fallback_adapter| wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter,
        compatible_surface: None,
        ..Default::default()
    };

    let adapter = match block_on(instance.request_adapter(&options(false))) {
        Ok(adapter) => adapter,
        Err(error) => {
            log::warn!("neuro-lib: brak adaptera ({error}) — próbuję adaptera zastępczego");
            block_on(instance.request_adapter(&options(true))).map_err(|_| Error::NoAdapter)?
        }
    };

    let info = adapter.get_info();
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("neuro.device"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        ..Default::default()
    }))?;

    Ok((info, device, queue))
}

/// Tworzy bufor i od razu wypełnia go danymi z hosta.
pub fn create_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    data: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: data.len() as u64,
        usage,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, data);
    buffer
}

/// Tworzy mały bufor zastępczy wypełniony zerami — wstawiany w miejsce
/// sektora, do którego Płatek nie ma prawa.
pub fn empty_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    create_buffer(device, queue, label, &vec![0u8; size as usize], usage)
}

/// Wejście layoutu dla `var<storage>` (bindingi `0` i `1`).
pub fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Wejście layoutu dla `var<uniform>` (binding `2`).
pub fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Waker „niebudzący” — zapytania `wgpu` na natywnym backendzie rozwiązują się
/// w trakcie `poll`, więc budzenie z innego wątku nie jest potrzebne.
struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
    fn wake_by_ref(self: &Arc<Self>) {}
}

/// Oczekuje na przyszłość wewnątrz pętli (bez zależności od `futures`).
///
/// Tak samo jak w `aurum` i `glaz` — jednorazowe wywołanie przy starcie
/// nie uzasadnia ciągnięcia `pollster`/`futures`.
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = TaskContext::from_waker(&waker);
    let mut future = Box::pin(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grupy_robocze_sa_zaokraglane_w_gore() {
        assert_eq!(workgroups(0), 0);
        assert_eq!(workgroups(1), 1);
        assert_eq!(workgroups(64), 1);
        assert_eq!(workgroups(65), 2);
        assert_eq!(workgroups(1024), 16);
    }

    #[test]
    fn prawa_odpowiadaja_numerom_bindingow() {
        assert_eq!(SectorAccessRights::NeuronWorkspace.binding(), 0);
        assert_eq!(SectorAccessRights::SystemMap.binding(), 1);
        assert_eq!(SectorAccessRights::Config.binding(), 2);
    }
}
