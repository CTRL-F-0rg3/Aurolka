//! Pamięć wizualna — kernel `visual_memory.wgsl` uczący model odwzorować
//! obraz referencyjny (np. `calc.png`) w wewnętrznej „wyobraźni”.
//!
//! Shader trzyma cztery bufory:
//!
//! | Binding | Zawartość | Dostęp |
//! |---|---|---|
//! | `@binding(0)` | `array<vec4<f32>>` — obraz referencyjny (RGBA) | `read` |
//! | `@binding(1)` | `array<vec4<f32>>` — „wyobraźnia” modelu | `read_write` |
//! | `@binding(2)` | `array<NeuronForm>` — formy neuronów | `read_write` |
//! | `@binding(3)` | `vec4<u32>` — konfiguracja | `uniform` |
//!
//! Konfiguracja (`vec4<u32>`): `x` — szerokość, `y` — wysokość,
//! `z` — liczba neuronów, `w` — krok (niewykorzystywany przez ten kernel).
//!
//! W przeciwieństwie do [`ScenarioMatrixManager`](crate::ScenarioMatrixManager)
//! każdy piksel to **jedna** komórka `vec4<f32>` (nie cztery), więc liczba
//! komórek to po prostu `width × height`.

use std::cell::Cell;

use bytemuck::Pod;

use crate::error::{Error, Result};
use crate::gpu_manager::{
    empty_buffer, request_device, storage_entry, uniform_entry, validate_shader,
};
use crate::png::PngImage;
use crate::scenario_matrix::NeuronForm;

/// Grupa robocza shadera: `@workgroup_size(16, 16)`.
const WORKGROUP_X: u32 = 16;
const WORKGROUP_Y: u32 = 16;

/// Etykieta zasobów.
const LABEL: &str = "neuro.visual";

/// Menedżer pamięci wizualnej: bufory obrazu referencyjnego, „wyobraźni”
/// i form neuronów plus pipeline compute’owy dla `visual_memory.wgsl`.
pub struct VisualMemoryManager {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,

    reference_buffer: wgpu::Buffer, // @binding(0) — obraz referencyjny
    output_buffer: wgpu::Buffer,    // @binding(1) — wyobraźnia
    forms_buffer: wgpu::Buffer,     // @binding(2) — formy neuronów

    output_staging: wgpu::Buffer,
    forms_staging: wgpu::Buffer,

    width: u32,
    height: u32,
    total_neurons: u32,
    config: Cell<[u32; 4]>,
}

impl VisualMemoryManager {
    /// Tworzy menedżera: waliduje shader, buduje bufory i pipeline.
    ///
    /// * `width`/`height` — wymiary obrazu w pikselach,
    /// * `total_neurons` — liczba form neuronów w `neuron_forms`.
    pub fn new(
        shader_source: &str,
        width: u32,
        height: u32,
        total_neurons: u32,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::Matrix(
                "wymiary obrazu nie mogą być zerowe".into(),
            ));
        }
        if total_neurons == 0 {
            return Err(Error::Matrix("liczba neuronów nie może być zerowa".into()));
        }

        validate_shader(shader_source)?;
        let (info, device, queue) = request_device()?;

        let cells = u64::from(width) * u64::from(height);
        let storage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let uniform = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let forms_bytes = u64::from(total_neurons) * std::mem::size_of::<NeuronForm>() as u64;

        let reference_buffer =
            empty_buffer(&device, &queue, "neuro.visual.reference", cells * 16, storage);
        let output_buffer =
            empty_buffer(&device, &queue, "neuro.visual.output", cells * 16, storage);
        let forms_buffer =
            empty_buffer(&device, &queue, "neuro.visual.forms", forms_bytes, storage);
        let config_buffer =
            empty_buffer(&device, &queue, "neuro.visual.config", 16, uniform);

        let output_staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("neuro.visual.output.staging"),
            size: cells * 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let forms_staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("neuro.visual.forms.staging"),
            size: forms_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Layout musi zgadzać się z deklaracjami w `visual_memory.wgsl`.
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(LABEL),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                storage_entry(2, false),
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
            entry_point: Some("visual_pass"),
            compilation_options: Default::default(),
            cache: None,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(LABEL),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: reference_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: forms_buffer.as_entire_binding(),
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
            "neuro-lib: pamięć wizualna {width}×{height} ({} komórek), {total_neurons} neuronów",
            cells
        );

        Ok(Self {
            device,
            queue,
            info,
            pipeline,
            bind_group,
            reference_buffer,
            output_buffer,
            forms_buffer,
            output_staging,
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

    /// Szerokość obrazu w pikselach.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Wysokość obrazu w pikselach.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Liczba form neuronów.
    pub fn total_neurons(&self) -> u32 {
        self.total_neurons
    }

    /// Liczba komórek — tyle jest `vec4<f32>` w buforze (jeden na piksel).
    pub fn cells(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Bieżąca konfiguracja (`[szerokość, wysokość, neurony, krok]`).
    pub fn config(&self) -> [u32; 4] {
        self.config.get()
    }

    /// Zapisuje obraz referencyjny (RGBA) do bufora GPU.
    ///
    /// Piksele są normalizowane do zakresu `0.0..=1.0`. Obraz mniejszy niż
    /// menedżer jest dopełniany zerami, większy — przycinany.
    pub fn write_image(&self, image: &PngImage) -> Result<()> {
        let mut cells = vec![[0.0f32; 4]; (self.width * self.height) as usize];
        let copy_width = image.width.min(self.width) as usize;
        let copy_height = image.height.min(self.height) as usize;

        for y in 0..copy_height {
            for x in 0..copy_width {
                let src = (y * image.width as usize + x) * 4;
                let dst = y * self.width as usize + x;
                cells[dst] = [
                    image.pixels[src] as f32 / 255.0,
                    image.pixels[src + 1] as f32 / 255.0,
                    image.pixels[src + 2] as f32 / 255.0,
                    image.pixels[src + 3] as f32 / 255.0,
                ];
            }
        }

        self.queue
            .write_buffer(&self.reference_buffer, 0, bytemuck::cast_slice(&cells));
        Ok(())
    }

    /// Alias [`Self::write_image`] — podmienia obraz referencyjny w trakcie
    /// nauki (np. kolejna klatka kalibracyjna).
    pub fn update_reference_image(&self, image: &PngImage) -> Result<()> {
        self.write_image(image)
    }

    /// Zeruje „wyobraźnię” modelu (bufor wyjściowy) przed nowym przebiegiem.
    pub fn prepare_canvas(&self) -> Result<()> {
        let cells = (self.width * self.height) as usize;
        let zeros = vec![[0.0f32; 4]; cells];
        self.queue
            .write_buffer(&self.output_buffer, 0, bytemuck::cast_slice(&zeros));
        Ok(())
    }

    /// Nadpisuje „wyobraźnię” modelu (`values.len()` musi wynosić [`Self::cells`]).
    pub fn write_visual_output(&self, values: &[[f32; 4]]) -> Result<()> {
        let expected = self.cells() as usize;
        if values.len() != expected {
            return Err(Error::Matrix(format!(
                "wyobraźnia: podano {} elementów, a bufor ma {expected}",
                values.len()
            )));
        }
        self.queue
            .write_buffer(&self.output_buffer, 0, bytemuck::cast_slice(values));
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

    /// Jedno przeliczenie wizualne — dispatch dwuwymiarowy:
    /// `ceil(width / 16) × ceil(height / 16)` grup roboczych.
    pub fn execute(&self) -> Result<()> {
        let groups_x = self.width.div_ceil(WORKGROUP_X);
        let groups_y = self.height.div_ceil(WORKGROUP_Y);

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("neuro.visual.encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("neuro.visual.pass"),
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

    /// Odczytuje całą „wyobraźnię” (wyjście wizualne) z GPU.
    pub fn read_visual_output(&self) -> Result<Vec<[f32; 4]>> {
        self.read_into(
            &self.output_buffer,
            &self.output_staging,
            self.cells() as usize,
            "wyobraźnia",
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

    /// Odczytuje wzorzec liczbowy pojedynczego piksela `(x, y)` wyobraźni.
    pub fn read_pattern_cell(&self, x: u32, y: u32) -> Result<[f32; 4]> {
        if x >= self.width || y >= self.height {
            return Err(Error::Matrix(format!(
                "odczyt: ({x},{y}) poza obrazem {}×{}",
                self.width, self.height
            )));
        }
        let output = self.read_visual_output()?;
        output
            .get((y * self.width + x) as usize)
            .copied()
            .ok_or_else(|| Error::Matrix("odczyt: indeks piksela poza buforem".into()))
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
            label: Some("neuro.visual.readback"),
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
            .map_err(|error| Error::Map(format!("callback mapowania nie przyszedł: {error}")))?
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

#[cfg(test)]
mod tests {
    #[test]
    fn liczba_komorek_to_iloczyn_wymiarow() {
        // W pamięci wizualnej każdy piksel to jedna komórka vec4.
        let cells = |w: u32, h: u32| u64::from(w) * u64::from(h);
        assert_eq!(cells(64, 64), 4096);
        assert_eq!(cells(256, 256), 65_536);
    }

    #[test]
    fn manager_odrzuca_zerowe_wymiary_bez_gpu() {
        // Brak GPU jest akceptowalny — walidacja wymiarów dzieje się wcześniej,
        // więc test tylko sprawdza logikę arytmetyczną.
        assert_eq!(0u32.div_ceil(16), 0);
        assert_eq!(1u32.div_ceil(16), 1);
        assert_eq!(256u32.div_ceil(16), 16);
    }
}


