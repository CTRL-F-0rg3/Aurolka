//! Histogram wartości `f32` liczony na atomikach GPU.
//!
//! Każdy wątek sam zwiększa licznik swojej półki, więc nie ma wyścigu — GPU
//! rozwiązuje to sprzętowo. Wynik to `bins` liczb `u32`, czyli liczność każdej
//! półki.
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::ops::histogram;
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//! let dane = Buffer::from_slice(&ctx, &[0.0f32, 0.4, 0.6, 0.9, 2.0], "dane")?;
//!
//! // Półka [0,1): 0.0 i 0.4 w pierwszej, 0.6 i 0.9 w drugiej; 2.0 poza zakresem.
//! assert_eq!(histogram::histogram(&ctx, &dane, 2, 0.0, 1.0)?, vec![2, 2]);
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use crate::gpu::buffer::Buffer;
use crate::gpu::context::Context;
use crate::gpu::kernel::{Binding, BindingKind, Kernel, KernelDesc};
use crate::ops::run;

/// Zakres histogramu przekazywany jako uniform (16 bajtów).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Range {
    /// Liczba elementów wejścia.
    count: u32,
    /// Liczba półek.
    bins: u32,
    /// Dolna granica (włączona).
    lo: f32,
    /// Górna granica (wyłączona).
    hi: f32,
}

const HISTOGRAM_WGSL: &str = r#"
struct Range { count: u32, bins: u32, lo: f32, hi: f32 };

@group(0) @binding(0) var<storage, read>       src: array<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>>;
@group(0) @binding(2) var<uniform>             range: Range;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= range.count) { return; }

    let value = src[i];
    // Wartości poza zakresem są pomijane — tak samo jak w math::stats::histogram.
    if (value < range.lo || value >= range.hi) { return; }

    let t = (value - range.lo) / (range.hi - range.lo);
    let bin = min(u32(t * f32(range.bins)), range.bins - 1u);
    atomicAdd(&bins[bin], 1u);
}
"#;

/// Histogram wartości z przedziału `[lo, hi)`.
///
/// Wymaga `bins > 0` i `hi > lo`; wartości poza zakresem są pomijane.
pub fn histogram(ctx: &Context, src: &Buffer<f32>, bins: usize, lo: f32, hi: f32) -> Result<Vec<u32>> {
    if src.is_empty() {
        return Err(Error::empty("wejście"));
    }
    if bins == 0 {
        return Err(Error::empty("liczba półek"));
    }
    if hi <= lo {
        return Err(Error::size("zakres (hi > lo)", 1, 0));
    }

    let range = Buffer::uniform(
        ctx,
        &[Range {
            count: src.len() as u32,
            bins: bins as u32,
            lo,
            hi,
        }],
        "histogram.range",
    )?;

    // Bufory `u32` nie są zerowane automatycznie — trzeba je wyzerować.
    let counts = Buffer::zeros(ctx, bins, "histogram.bins")?;
    let kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "histogram.atomic",
            source: HISTOGRAM_WGSL,
            entry_point: "main",
            bindings: &[
                BindingKind::ReadOnlyStorage,
                BindingKind::Storage,
                BindingKind::Uniform,
            ],
        },
    )?;

    let group = kernel.bind_group(
        ctx,
        &[
            Binding::read(src),
            Binding::write(&counts),
            Binding::uniform(&range),
        ],
    )?;

    run(ctx, |encoder| {
        kernel.dispatch(
            encoder,
            &group,
            (crate::gpu::groups_for(src.len(), 64), 1, 1),
        )
    })?;

    counts.read(ctx)
}