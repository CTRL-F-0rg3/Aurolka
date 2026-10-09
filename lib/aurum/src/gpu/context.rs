//! Kontekst obliczeniowy: wybór adaptera, urządzenia i kolejki.
//!
//! [`Context`] trzyma `wgpu::Device`, `wgpu::Queue` i cache zasobów
//! (layoutów bindingów i pipeline’ów compute’owych). Dzięki cache’u wywołanie
//! z tym samym WGSL nie tworzy drugiego pipeline’u — a `wgpu` potrafi to
//! potrwać kilkaset milisekund.
//!
//! ```
//! use aurum::gpu::{Context, ContextBuilder};
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::builder()
//!     .label("liczenia")
//!     .power_preference(wgpu::PowerPreference::HighPerformance)
//!     .build()?;
//!
//! println!("{} ({})", ctx.info().name, ctx.info().backend);
//! # Ok(())
//! # }
//! ```
//!
//! # Interoperacyjność
//!
//! Aplikacja, która już ma urządzenie (np. renderer `glaz`), może zamiast
//! [`Context::new`] użyć [`Context::wrap`] i liczyć na tym samym urządzeniu —
//! bez drugiego adaptera i bez ręcznego `copy_buffer_to_buffer`.
//!
//! # Wątek
//!
//! `Context` nie jest `Sync` (cache używa `RefCell`), więc jeden `Context`
//! obsługuje jeden wątek. Do pracy równoległej twórz osobne konteksty.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Wake, Waker};

use crate::error::{Error, Result};
use crate::gpu::kernel::BindingKind;

/// Waker „niebudzący” — zapytania `wgpu` na natywnym backendzie rozwiązują się
/// w trakcie `poll`, więc budzenie z innego wątku nie jest potrzebne.
struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
    fn wake_by_ref(self: &Arc<Self>) {}
}

/// Oczekuje na przyszłość wewnątrz pętli zdarzeń (bez zależności od `futures`).
fn block_on<F: Future>(future: F) -> F::Output {
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

/// Klucz cache'a pipeline'ów: skrót WGSL + punkt wejścia + layout bindingów.
type PipelineKey = (u64, String, Vec<BindingKind>);

/// Kontekst obliczeniowy — urządzenie GPU wraz z cache’em zasobów.
pub struct Context {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    pipelines: RefCell<HashMap<PipelineKey, Arc<wgpu::ComputePipeline>>>,
    layouts: RefCell<HashMap<Vec<BindingKind>, Arc<wgpu::BindGroupLayout>>>,
    pipeline_layouts: RefCell<HashMap<Vec<BindingKind>, Arc<wgpu::PipelineLayout>>>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("adapter", &self.info.name)
            .field("backend", &self.info.backend)
            .field("pipelines", &self.cached_pipelines())
            .finish()
    }
}

impl Context {
    /// Tworzy kontekst: wybiera adapter zgodnie z [`ContextBuilder`] i prosi
    /// o urządzenie.
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Buduje [`Context`] krok po kroku (domyślnie najwydajniejszy adapter).
    pub fn builder() -> ContextBuilder {
        ContextBuilder::new()
    }

    /// Zawija już posiadane urządzenie w [`Context`].
    ///
    /// Przydatne, gdy aplikacja renderuje na `wgpu` i chce liczyć na tym samym
    /// urządzeniu.
    pub fn wrap(device: wgpu::Device, queue: wgpu::Queue, info: wgpu::AdapterInfo) -> Self {
        Self {
            device,
            queue,
            info,
            pipelines: RefCell::new(HashMap::new()),
            layouts: RefCell::new(HashMap::new()),
            pipeline_layouts: RefCell::new(HashMap::new()),
        }
    }

    /// Urządzenie GPU — do zasobów spoza biblioteki.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Kolejka GPU — do przesyłania danych poza biblioteką.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Informacje o adapterze (nazwa, backend, typ).
    pub fn info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    /// Limity urządzenia.
    pub fn limits(&self) -> wgpu::Limits {
        self.device.limits()
    }

    /// Cechy urządzenia.
    pub fn features(&self) -> wgpu::Features {
        self.device.features()
    }

    /// Ile pipeline’ów jest obecnie w cache’u.
    pub fn cached_pipelines(&self) -> usize {
        self.pipelines.borrow().len()
    }

    /// Ile layoutów bindingów jest obecnie w cache’u.
    pub fn cached_layouts(&self) -> usize {
        self.layouts.borrow().len()
    }

    /// Sprawdza, czy WGSL się kompiluje i spełnia reguły WebGPU, bez tworzenia
    /// pipeline’u.
    ///
    /// Robimy to przez `naga`: błąd wraca jako [`Error::Shader`] z numerem linii
    /// i kolumny, więc literówka w shaderze ani zły layout uniformu nie kończą
    /// się asynchronicznym `panic`em `wgpu`.
    pub fn validate(&self, source: &str) -> Result<()> {
        let module = wgpu::naga::front::wgsl::parse_str(source)
            .map_err(|e| Error::shader("<walidacja>", e.emit_to_string(source)))?;

        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|e| Error::shader("<walidacja>", e.emit_to_string(source)))?;

        Ok(())
    }

    /// Wysyła komendy w jednym `submit` i zwraca wynik z domknięcia.
///
/// Domknięcie dostaje `&mut wgpu::CommandEncoder`; typowo są to kolejne
/// wywołania [`Kernel::dispatch`](crate::gpu::Kernel::dispatch). Błąd w
/// domknięciu oznacza, że komendy **nie** trafią do kolejki wcale.
///
/// ```
/// # use aurum::gpu::Context;
/// # use aurum::Result;
/// # fn main() -> Result<()> {
/// # let ctx = Context::new()?;
/// ctx.submit(|encoder| {
///     let _ = encoder; // tu np. `kernel.dispatch(encoder, &group, (8, 1, 1))`
///     Ok(())
/// })?;
/// # Ok(())
/// # }
/// ```
pub fn submit(
        &self,
        record: impl FnOnce(&mut wgpu::CommandEncoder) -> Result<()>,
    ) -> Result<()> {
        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("aurum.encoder"),
                });

        // Błąd w domknięciu nie może zostawić half-napisanej komendy w kolejce.
        record(&mut encoder)?;
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    /// Czeka, aż GPU skończy wcześniej wysłane komendy.
    ///
    /// Wywołuj przed [`Buffer::read`](crate::gpu::Buffer::read) — mapowanie
    /// buforów jest asynchroniczne.
    pub fn poll(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| Error::Poll(e.to_string()))
    }

    /// Layout bindingów z cache’a (tworzy go przy pierwszym użyciu).
    pub(crate) fn bind_group_layout(
        &self,
        label: &str,
        bindings: &[BindingKind],
    ) -> Arc<wgpu::BindGroupLayout> {
        if let Some(layout) = self.layouts.borrow().get(bindings) {
            return Arc::clone(layout);
        }

        let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
            .iter()
            .enumerate()
            .map(|(index, kind)| wgpu::BindGroupLayoutEntry {
                binding: index as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: match kind {
                    BindingKind::Storage => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    BindingKind::ReadOnlyStorage => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    BindingKind::Uniform => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                },
                count: None,
            })
            .collect();

        let layout = Arc::new(
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some(label),
                    entries: &entries,
                }),
        );
        self.layouts
            .borrow_mut()
            .insert(bindings.to_vec(), Arc::clone(&layout));
        layout
    }

    /// Layout pipeline’u (bind group + brak push constantów) z cache’a.
    pub(crate) fn pipeline_layout(
        &self,
        label: &str,
        bindings: &[BindingKind],
    ) -> Arc<wgpu::PipelineLayout> {
        if let Some(layout) = self.pipeline_layouts.borrow().get(bindings) {
            return Arc::clone(layout);
        }

        let bind_group_layout = self.bind_group_layout(label, bindings);
        let layout = Arc::new(self.device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(&*bind_group_layout)],
                // Biblioteka nie używa push constantów — parametry idą
                // przez bufory uniformowe.
                immediate_size: 0,
            },
        ));

        self.pipeline_layouts
            .borrow_mut()
            .insert(bindings.to_vec(), Arc::clone(&layout));
        layout
    }

    /// Pipeline compute’owy z cache’a (tworzy go przy pierwszym użyciu).
    pub(crate) fn compute_pipeline(
        &self,
        label: &str,
        source: &str,
        entry_point: &str,
        bindings: &[BindingKind],
        layout: &wgpu::PipelineLayout,
    ) -> Result<Arc<wgpu::ComputePipeline>> {
        let key = (crate::gpu::kernel::hash(source), entry_point.to_owned(), bindings.to_vec());
        if let Some(pipeline) = self.pipelines.borrow().get(&key) {
            return Ok(Arc::clone(pipeline));
        }

        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let pipeline = Arc::new(self.device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            },
        ));

        log::debug!("aurum: utworzono pipeline `{label}`");
        self.pipelines
            .borrow_mut()
            .insert(key, Arc::clone(&pipeline));
        Ok(pipeline)
    }
}

/// Budowniczek [`Context`] — wybór adaptera i limitów.
#[derive(Debug, Clone)]
pub struct ContextBuilder {
    label: Option<String>,
    power_preference: wgpu::PowerPreference,
    force_fallback_adapter: bool,
    required_features: wgpu::Features,
    limits: Option<wgpu::Limits>,
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextBuilder {
    /// Builder z ustawieniami domyślnymi: najwydajniejszy adapter, pełne limity.
    pub fn new() -> Self {
        Self {
            label: None,
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            required_features: wgpu::Features::empty(),
            limits: None,
        }
    }

    /// Etykieta urządzenia — widać ją w narzędziach do debugowania GPU.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Preferencja zasilania adaptera.
    pub fn power_preference(mut self, preference: wgpu::PowerPreference) -> Self {
        self.power_preference = preference;
        self
    }

    /// Wymuszenie adaptera zastępczego (software, np. `lavapipe`).
    pub fn force_fallback_adapter(mut self, force: bool) -> Self {
        self.force_fallback_adapter = force;
        self
    }

    /// Wymagane cechy urządzenia (np. `wgpu::Features::TIMESTAMP_QUERY`).
    pub fn required_features(mut self, features: wgpu::Features) -> Self {
        self.required_features = features;
        self
    }

    /// Wymagane limity — domyślnie brane z adaptera.
    pub fn required_limits(mut self, limits: wgpu::Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Tworzy kontekst.
    pub fn build(self) -> Result<Context> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: self.power_preference,
            force_fallback_adapter: self.force_fallback_adapter,
            compatible_surface: None,
            ..Default::default()
        }))
        .map_err(Error::from)?;

        let info = adapter.get_info();
        let limits = self.limits.unwrap_or_else(|| adapter.limits());

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: self.label.as_deref(),
            required_features: self.required_features,
            required_limits: limits,
            ..Default::default()
        }))
        .map_err(Error::from)?;

        log::info!("aurum: adapter `{}` ({})", info.name, info.backend);

        Ok(Context {
            device,
            queue,
            info,
            pipelines: RefCell::new(HashMap::new()),
            layouts: RefCell::new(HashMap::new()),
            pipeline_layouts: RefCell::new(HashMap::new()),
        })
    }
}