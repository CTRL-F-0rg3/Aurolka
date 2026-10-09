//! Kernele compute’owe: WGSL + layout bindingów + dispatch.
//!
//! [`Kernel`] to gotowy do użycia pipeline’owy shader z minimalnym API: tworzysz
//! go raz, tworzysz bind grupę i odpalaasz dispatch. Pipeline i layout są
//! trzymane w cache’u [`Context`](crate::gpu::Context), więc wywołanie
//! [`Kernel::new`] z tym samym WGSL nie kosztuje nic.
//!
//! # Konwencja bindingów
//!
//! Bindingi są **gęste i numerowane od zera**: pozycja w `bindings` odpowiada
//! `@binding(n)` w WGSL. Nie trzeba podawać żadnych dodatkowych nazw — shader
//! sam deklaruje bufor pod swoim numerem, a [`Kernel::bind_group`] pilnuje tylko
//! zgodności rodzajów i kolejności.
//!
//! # Rozmiar grupy roboczej
//!
//! `dispatch` przyjmuje liczbę **grup roboczych**, a nie wątków. Dla długości `n`
//! i rozmiaru grupy `w` użyj [`groups_for`], który zaokrągla w górę.

use std::sync::Arc;

use bytemuck::Pod;

use crate::error::{Error, Result};
use crate::gpu::buffer::Buffer;
use crate::gpu::context::Context;

/// Rodzaj zasobu pod bindingiem — musi się zgadzać z deklaracją w WGSL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// `var<storage, read_write>` — kernel zapisuje wynik.
    Storage,
    /// `var<storage, read>` — kernel tylko czyta.
    ReadOnlyStorage,
    /// `var<uniform>` — małe stałe (wymiary, maski, parametry).
    Uniform,
}

impl BindingKind {
    /// Czy kernel może zapisywać do tego bufora.
    pub fn is_writable(self) -> bool {
        matches!(self, Self::Storage)
    }
}

/// Opis kernela — wszystko, czego potrzeba do stworzenia pipeline’u.
#[derive(Debug, Clone, Copy)]
pub struct KernelDesc<'a> {
    /// Etykieta (widoczna w debuggerach GPU i w logach biblioteki).
    pub label: &'a str,
    /// Kod WGSL.
    pub source: &'a str,
    /// Nazwa punktu wejścia compute (`fn main` najczęściej).
    pub entry_point: &'a str,
    /// Rodzaje bindingów w kolejności `@binding(0)`, `@binding(1)`, ...
    pub bindings: &'a [BindingKind],
}

/// Bufor przekazany do bindingu, z informacją o tym, jak ma być użyty.
///
/// Typ elementu nie gra tu roli — binding trzyma surowy `wgpu::Buffer`, więc
/// jedna bind grupa może mieszać `f32`, `u32` i struktury uniformowe.
#[derive(Debug, Clone, Copy)]
pub enum Binding<'a> {
    /// Shader tylko czyta (`var<storage, read>`).
    Read(&'a wgpu::Buffer),
    /// Shader zapisuje do bufora (`var<storage, read_write>`).
    Write(&'a wgpu::Buffer),
    /// Bufor uniformowy (`var<uniform>`).
    Uniform(&'a wgpu::Buffer),
}

impl<'a> Binding<'a> {
    /// Bufor tylko do odczytu.
    pub fn read<T: Pod>(buffer: &'a Buffer<T>) -> Self {
        Self::Read(buffer.raw())
    }

    /// Bufor do zapisu.
    pub fn write<T: Pod>(buffer: &'a Buffer<T>) -> Self {
        Self::Write(buffer.raw())
    }

    /// Bufor uniformowy.
    pub fn uniform<T: Pod>(buffer: &'a Buffer<T>) -> Self {
        Self::Uniform(buffer.raw())
    }

    /// Surowy bufor `wgpu`.
    pub fn buffer(&self) -> &'a wgpu::Buffer {
        match self {
            Self::Read(buffer) | Self::Write(buffer) | Self::Uniform(buffer) => buffer,
        }
    }

    /// Rodzaj zasobu, który chce zadeklarować layout.
    fn kind(&self) -> BindingKind {
        match self {
            Self::Read(_) => BindingKind::ReadOnlyStorage,
            Self::Write(_) => BindingKind::Storage,
            Self::Uniform(_) => BindingKind::Uniform,
        }
    }
}

/// Gotowy pipeline compute’owy.
#[derive(Clone)]
pub struct Kernel {
    label: String,
    entry_point: String,
    bindings: Arc<[BindingKind]>,
    pipeline: Arc<wgpu::ComputePipeline>,
    layout: Arc<wgpu::BindGroupLayout>,
}

impl std::fmt::Debug for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kernel")
            .field("label", &self.label)
            .field("entry_point", &self.entry_point)
            .field("bindings", &self.bindings.len())
            .finish()
    }
}

impl Kernel {
    /// Tworzy (albo bierze z cache’a) pipeline compute’owy.
    ///
    /// WGSL jest najpierw walidowany przez `naga`, więc błąd składni wraca
    /// jako [`Error::Shader`] zamiast późniejszego błędu walidacji `wgpu`.
    pub fn new(ctx: &Context, desc: KernelDesc<'_>) -> Result<Self> {
        ctx.validate(desc.source)?;

        let layout = ctx.bind_group_layout(desc.label, desc.bindings);
        let pipeline_layout = ctx.pipeline_layout(desc.label, desc.bindings);
        let pipeline = ctx.compute_pipeline(
            desc.label,
            desc.source,
            desc.entry_point,
            desc.bindings,
            &pipeline_layout,
        )?;

        Ok(Self {
            label: desc.label.to_owned(),
            entry_point: desc.entry_point.to_owned(),
            bindings: Arc::from(desc.bindings),
            pipeline,
            layout,
        })
    }

    /// Etykieta kernela.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Punkt wejścia WGSL.
    pub fn entry_point(&self) -> &str {
        &self.entry_point
    }

    /// Deklarowane bindingi.
    pub fn bindings(&self) -> &[BindingKind] {
        &self.bindings
    }

    /// Tworzy bind grupę, sprawdzając liczbę i rodzaje bindingów.
    ///
    /// Kolejność na liście musi odpowiadać `@binding(0..n)` w shaderze.
    pub fn bind_group(&self, ctx: &Context, bindings: &[Binding<'_>]) -> Result<wgpu::BindGroup> {
        if bindings.len() != self.bindings.len() {
            return Err(Error::Bindings {
                expected: self.bindings.len(),
                got: bindings.len(),
            });
        }

        let entries: Vec<wgpu::BindGroupEntry> = bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| {
                let declared = self.bindings[index];
                let given = binding.kind();
                if declared != given {
                    return Err(Error::ElementType {
                        binding: index,
                        expected: declared_name(declared),
                        got: declared_name(given),
                    });
                }
                Ok(wgpu::BindGroupEntry {
                    binding: index as u32,
                    resource: binding.buffer().as_entire_binding(),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(ctx.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&self.label),
            layout: &self.layout,
            entries: &entries,
        }))
    }

    /// Wysyła dispatch do kodera komend.
    ///
    /// Liczba to **grupy robocze**, nie wątki — patrz [`groups_for`]. Wywołanie
    /// niczego nie wysyła samo w sobie: komendy trafią do kolejki dopiero w
    /// [`Context::submit`](crate::gpu::Context::submit).
    pub fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind_group: &wgpu::BindGroup,
        workgroups: (u32, u32, u32),
    ) -> Result<()> {
        if workgroups.0 == 0 || workgroups.1 == 0 || workgroups.2 == 0 {
            return Err(Error::EmptyDispatch {
                label: self.label.clone(),
            });
        }

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(&self.label),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(workgroups.0, workgroups.1, workgroups.2);
        drop(pass);

        Ok(())
    }
}

/// Nazwa rodzaju bindingu dla komunikatów o błędach.
fn declared_name(kind: BindingKind) -> &'static str {
    match kind {
        BindingKind::Storage => "var<storage, read_write>",
        BindingKind::ReadOnlyStorage => "var<storage, read>",
        BindingKind::Uniform => "var<uniform>",
    }
}

/// Liczba grup roboczych potrzebna do obsłużenia `len` elementów.
///
/// ```
/// use aurum::gpu::groups_for;
///
/// assert_eq!(groups_for(64, 64), 1);
/// assert_eq!(groups_for(65, 64), 2);
/// assert_eq!(groups_for(0, 64), 0);
/// ```
pub fn groups_for(len: usize, workgroup_size: u32) -> u32 {
    let per_group = workgroup_size.max(1) as usize;
    len.div_ceil(per_group) as u32
}

/// Skrót FNV-1a na potrzeby klucza w cache’u pipeline’ów.
pub(crate) fn hash(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}