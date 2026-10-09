//! Globalna macierz scenariuszy — drugi kernel (`scenario_matrix.wgsl`).
//!
//! Shader trzyma trzy bufory „sektorów” i jeden uniform:
//!
//! | Binding | Zawartość | Dostęp |
//! |---|---|---|
//! | `@binding(0)` | `array<vec4<f32>>` — macierz scenariuszy | `read_write` |
//! | `@binding(1)` | `array<NeuronForm>` — wagi neuronów | `read_write` |
//! | `@binding(2)` | `array<vec4<f32>>` — wektor impulsu | `read` |
//! | `@binding(3)` | `vec4<u32>` — konfiguracja | `uniform` |
//!
//! Konfiguracja (`vec4<u32>`): `x` — szerokość, `y` — wysokość,
//! `z` — liczba neuronów, `w` — 1, gdy obowiązuje impuls („szok”).
//!
//! Wątek przetwarza **4 komórki** naraz (`vec4<f32>`), a grupa robocza to
//! `16 × 16` wątków, więc dispatch jest dwuwymiarowy:
//! `ceil(w / 4 / 16) × ceil(h / 16)`.

use std::cell::Cell;

use bytemuck::{Pod, Zeroable};

use crate::error::{Error, Result};
use crate::gpu_manager::{create_buffer, request_device, storage_entry, uniform_entry, validate_shader};

/// Grupa robocza shadera: `@workgroup_size(16, 16)`.
const WORKGROUP_X: u32 = 16;
const WORKGROUP_Y: u32 = 16;

/// Każdy wątek obsługuje 4 komórki macierzy (jeden `vec4<f32>`).
const CELLS_PER_THREAD: u32 = 4;

/// Etykiety zasobów.
const LABEL: &str = "neuro.matrix";

/// Forma (wagi) pojedynczego neuronu — układ zgodny ze strukturą
/// `NeuronForm` w `scenario_matrix.wgsl`: 32 bajty, wielokrotność 16.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct NeuronForm {
    /// Wagi lokalne modyfikowane podczas adaptacji.
    pub weights: [f32; 4],
    /// Próg pobudzenia — powyżej neuron zmienia formę.
    pub adaptation_threshold: f32,
    /// Współczynnik synchronizacji z otoczeniem.
    pub sync_rate: f32,
    /// Aktualny poziom stresu (rośnie, gdy adaptacji nie ma).
    pub stress_level: f32,
    /// Wyrównanie — musi pozostać zerowe.
    pub _pad: f32,
}

impl NeuronForm {
    /// Forma z podanym progiem i tempem adaptacji, wagi zerowe.
    ///
    /// ```rust
    /// # use neuro_lib::NeuronForm;
    /// let form = NeuronForm::new(2.0, 0.25);
    /// assert_eq!(form.weights, [0.0; 4]);
    /// assert_eq!(form.adaptation_threshold, 2.0);
    /// ```
    pub fn new(adaptation_threshold: f32, sync_rate: f32) -> Self {
        Self {
            weights: [0.0; 4],
            adaptation_threshold,
            sync_rate,
            stress_level: 0.0,
            _pad: 0.0,
        }
    }

    /// Długość wektora wag.
    pub fn weights_len(&self) -> f32 {
        self.weights.iter().map(|w| w * w).sum::<f32>().sqrt()
    }
}

impl Default for NeuronForm {
    fn default() -> Self {
        Self::new(1.0, 0.1)
    }
}

/// Menedżer macierzy scenariuszy: bufory macierzy, form neuronów i impulsu
/// plus pipeline compute’owy dla `scenario_matrix.wgsl`.
///
/// ```no_run
/// # use neuro_lib::ScenarioMatrixManager;
/// # const WGSL: &str = r#"
/// # @group(0) @binding(0) var<storage, read_write> m: array<vec4<f32>>;
/// # @group(0) @binding(1) var<storage, read_write> f: array<vec4<f32>>;
/// # @group(0) @binding(2) var<storage, read> s: array<vec4<f32>>;
/// # @group(0) @binding(3) var<uniform> c: vec4<u32>;
/// # @compute @workgroup_size(16, 16)
/// # fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
/// #     let i = gid.y * (c.x / 4u) + gid.x;
/// #     if (i >= arrayLength(&m)) { return; }
/// #     m[i] = m[i] + s[i] * f32(c.w);
/// # }
/// # "#;
/// let manager = ScenarioMatrixManager::new(WGSL, 64, 64, 4)?;
/// manager.write_stimulus(&vec![[1.0, 0.0, 0.0, 0.0]; manager.cells() as usize])?;
/// manager.set_stimulus_active(true)?;
/// manager.execute()?;
/// let stan = manager.read_matrix()?;
/// assert_eq!(stan.len(), manager.cells() as usize);
/// # Ok::<(), neuro_lib::Error>(())
/// ```
pub struct ScenarioMatrixManager {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,

    matrix_buffer: wgpu::Buffer,
    forms_buffer: wgpu::Buffer,
    stimulus_buffer: wgpu::Buffer,
    config_buffer: wgpu::Buffer,

    matrix_staging: wgpu::Buffer,
    forms_staging: wgpu::Buffer,

    width: u32,
    height: u32,
    total_neurons: u32,
    config: Cell<[u32; 4]>,
}

impl ScenarioMatrixManager {
    /// Tworzy menedżera: waliduje shader, buduje bufory i pipeline.
    ///
    /// * `width`/`height` — wymiary macierzy (`width` musi być wielokrotnością 4),
    /// * `total_neurons` — liczba form neuronów w `neuron_forms`.
    pub fn new(
        shader_source: &str,
        width: u32,
        height: u32,
        total_neurons: u32,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::Matrix(
                "wymiary macierzy nie mogą być zerowe".into(),
            ));
        }
        if width % CELLS_PER_THREAD != 0 {
            return Err(Error::Matrix(format!(
                "szerokość {width} musi być wielokrotnością {CELLS_PER_THREAD} (wątek liczy vec4)"
            )));
        }
        if total_neurons == 0 {
            return Err(Error::Matrix("liczba neuronów nie może być zerowa".into()));
        }

        validate_shader(shader_source)?;
        let (info, device, queue) = request_device()?;

        let cells = cells_count(width, height);
        let storage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let uniform = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let forms_bytes = u64::from(total_neurons) * std::mem::size_of::<NeuronForm>() as u64;

        let matrix_buffer = create_zeros(&device, &queue, "neuro.matrix", cells * 16, storage);
        let forms_buffer = create_zeros(&device, &queue, "neuro.forms", forms_bytes, storage);
        let stimulus_buffer = create_zeros(&device, &queue, "neuro.stimulus", cells * 16, storage);
        let config_buffer = create_zeros(&device, &queue, "neuro.matrix.config", 16, uniform);

        let matrix_staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("neuro.matrix.staging"),
            size: cells * 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let forms_staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("neuro.forms.staging"),
            size: forms_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Layout musi zgadzać się z deklaracjami w `scenario_matrix.wgsl`.
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(LABEL),
            entries: &[
                storage_entry(0, false),
                storage_entry(1, false),
                storage_entry(2, true),
                uniform_entry(3),
            ],
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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(LABEL),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: matrix_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: forms_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: stimulus_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: config_buffer.as_entire_binding(),
                },
            ],
        });

        let config = [width, height, total_neurons, 0];
        queue.write_buffer(&config_buffer, 0, bytemuck::cast_slice(&config));

        log::info!(
            "neuro-lib: macierz scenariuszy {width}×{height} ({} komórek), {total_neurons} neuronów",
            cells_count(width, height)
        );

        Ok(Self {
            device,
            queue,
            info,
            pipeline,
            bind_group,
            matrix_buffer,
            forms_buffer,
            stimulus_buffer,
            config_buffer,
            matrix_staging,
            forms_staging,
            width,
            height,
            total_neurons,
            config: Cell::new(config),
        })
    }

    /// Informacje o adapterze.
    pub fn info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    /// Szerokość macierzy (w komórkach).
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Wysokość macierzy (w komórkach).
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Liczba form neuronów.
    pub fn total_neurons(&self) -> u32 {
        self.total_neurons
    }

    /// Liczba komórek macierzy — tyle jest `vec4<f32>` w buforze.
    pub fn cells(&self) -> u64 {
        cells_count(self.width, self.height)
    }

    /// Bieżąca konfiguracja (`[szerokość, wysokość, neurony, impuls]`).
    pub fn config(&self) -> [u32; 4] {
        self.config.get()
    }

    /// Nadpisuje macierz scenariuszy (`values.len()` musi wynosić [`Self::cells`]).
    pub fn write_matrix(&self, values: &[[f32; 4]]) -> Result<()> {
        self.check_cells(values.len(), "macierz")?;
        self.queue
            .write_buffer(&self.matrix_buffer, 0, bytemuck::cast_slice(values));
        Ok(())
    }

    /// Nadpisuje wektor impulsu — „szok” dodawany do macierzy, gdy
    /// [`Self::set_stimulus_active`] jest włączone.
    pub fn write_stimulus(&self, values: &[[f32; 4]]) -> Result<()> {
        self.check_cells(values.len(), "stimulus")?;
        self.queue
            .write_buffer(&self.stimulus_buffer, 0, bytemuck::cast_slice(values));
        Ok(())
    }

    /// Zapisuje pojedynczą komórkę impulsu o współrzędnych `(x, y)`.
    ///
    /// `x` jest wyrównywane w dół do granicy bloku `vec4` (wątek liczy
    /// czwórkami komórek), więc np. `x = 34` trafi w ten sam blok co `x = 32`.
    pub fn write_stimulus_cell(&self, x: u32, y: u32, data: [f32; 4]) -> Result<()> {
        if y >= self.height {
            return Err(Error::Matrix(format!(
                "stimulus: y={y} poza macierzą (wysokość {})",
                self.height
            )));
        }
        let per_row = self.width / CELLS_PER_THREAD;
        let block_x = (x / CELLS_PER_THREAD).min(per_row - 1);
        let offset = u64::from(y * per_row + block_x) * 16;
        self.queue
            .write_buffer(&self.stimulus_buffer, offset, bytemuck::cast_slice(&[data]));
        Ok(())
    }

    /// Nadpisuje formy neuronów.
    pub fn write_forms(&self, forms: &[NeuronForm]) -> Result<()> {
        if forms.len() != self.total_neurons as usize {
            return Err(Error::Matrix(format!(
                "podano {} form, a menedżer ma {}",
                forms.len(),
                self.total_neurons
            )));
        }
        self.queue
            .write_buffer(&self.forms_buffer, 0, bytemuck::cast_slice(forms));
        Ok(())
    }

    /// Włącza/wyłącza impuls w konfiguracji (`config.w`).
    pub fn set_stimulus_active(&self, active: bool) -> Result<()> {
        let mut config = self.config.get();
        config[3] = u32::from(active);
        self.queue
            .write_buffer(&self.config_buffer, 0, bytemuck::cast_slice(&config));
        self.config.set(config);
        Ok(())
    }

    fn check_cells(&self, len: usize, what: &str) -> Result<()> {
        let expected = self.cells() as usize;
        if len != expected {
            return Err(Error::Matrix(format!(
                "{what}: podano {len} elementów, a bufor ma {expected}"
            )));
        }
        Ok(())
    }

    /// Jedno przeliczenie macierzy — dispatch dwuwymiarowy:
    /// `ceil(w / 4 / 16) × ceil(h / 16)` grup roboczych.
    pub fn execute(&self) -> Result<()> {
        let groups_x = (self.width / CELLS_PER_THREAD).div_ceil(WORKGROUP_X);
        let groups_y = self.height.div_ceil(WORKGROUP_Y);

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("neuro.matrix.encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("neuro.matrix.pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    /// Wykonuje `steps` przeliczeń z rzędu.
    pub fn run_steps(&self, steps: u32) -> Result<()> {
        for _ in 0..steps {
            self.execute()?;
        }
        Ok(())
    }

    /// Czeka, aż GPU skończy wcześniej wysłane komendy.
    pub fn poll(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|error| Error::Poll(error.to_string()))
    }

    /// Odczytuje całą macierz scenariuszy z GPU.
    pub fn read_matrix(&self) -> Result<Vec<[f32; 4]>> {
        self.read_into(
            &self.matrix_buffer,
            &self.matrix_staging,
            self.cells() as usize,
            "macierz",
        )
    }

    /// Odczytuje formy wszystkich neuronów z GPU.
    pub fn read_forms(&self) -> Result<Vec<NeuronForm>> {
        self.read_into(
            &self.forms_buffer,
            &self.forms_staging,
            self.total_neurons as usize,
            "formy neuronów",
        )
    }

    /// Kopiuje bufor do stagingowego i mapuje go na CPU.
    fn read_into<T: Pod>(
        &self,
        source: &wgpu::Buffer,
        staging: &wgpu::Buffer,
        len: usize,
        what: &str,
    ) -> Result<Vec<T>> {
        let bytes = std::mem::size_of::<T>() as u64 * len as u64;

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("neuro.matrix.readback"),
        });
        encoder.copy_buffer_to_buffer(source, 0, staging, 0, bytes);
        self.queue.submit(std::iter::once(encoder.finish()));
        self.poll()?;

        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.poll()?;
        receiver
            .recv()
            .map_err(|error| Error::Map(format!("callback mapowania nie przysiedł: {error}")))?
            .map_err(|error| Error::Map(error.to_string()))?;

        let out = {
            let view = slice
                .get_mapped_range()
                .map_err(|error| Error::Map(error.to_string()))?;
            bytemuck::cast_slice::<u8, T>(&view).to_vec()
        };
        staging.unmap();

        log::debug!("neuro-lib: odczyt ({what}) — {} elementów", out.len());
        Ok(out)
    }
}

/// Liczba `vec4<f32>` w macierzy `width × height` (wątek obsługuje 4 komórki).
fn cells_count(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height) / u64::from(CELLS_PER_THREAD)
}

/// Bufor wypełniony zerami.
fn create_zeros(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    create_buffer(device, queue, label, &vec![0u8; size as usize], usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_neuronu_miesci_sie_w_wymaganiach_wgsl() {
        // array<NeuronForm> w storage ma krok wielokrotny 16 bajtów.
        assert_eq!(std::mem::size_of::<NeuronForm>(), 32);
        assert_eq!(std::mem::size_of::<NeuronForm>() % 16, 0);
    }

    #[test]
    fn liczba_komorek_zgadza_z_wymiarami() {
        assert_eq!(cells_count(64, 64), 1024);
        assert_eq!(cells_count(2048, 2048), 1_048_576);
        // 64 × 64 komórek = 4096 f32 = 1024 vec4 → 16 384 bajty.
        assert_eq!(cells_count(64, 64) * 16, 64 * 64 * 4);
    }

    #[test]
    fn domyslna_forma_ma_zerowe_wagi() {
        let form = NeuronForm::default();
        assert_eq!(form.weights, [0.0; 4]);
        assert_eq!(form.stress_level, 0.0);
        assert_eq!(form._pad, 0.0);
        assert_eq!(form.weights_len(), 0.0);
    }
}
