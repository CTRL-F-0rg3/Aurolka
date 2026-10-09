//! Sploty i filtry na tablicach liczb.
//!
//! Rozmycie Gaussa liczone separowalnie (najpierw po wierszach, potem po
//! kolumnach) kosztuje `O(n · r)` zamiast `O(n · r²)` pełnego splotu 2D —
//! dlatego ten sam wynik jest cztery razy (a przy dużych promieniach znacznie
//! więcej) tańszy.
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::ops::filter;
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//!
//! // Obraz 16×12 z jednym jasnym pikselem w środku.
//! let (width, height) = (16usize, 12usize);
//! let mut obraz = vec![0.0f32; width * height];
//! obraz[width * height / 2 + width / 2] = 100.0;
//! let obraz = Buffer::from_slice(&ctx, &obraz, "obraz")?;
//!
//! let rozmyty = filter::gaussian_2d(&ctx, &obraz, width, height, 1.5)?;
//!
//! // Splot separowalny z jądrem o sumie 1 nie traci energii.
//! let suma: f32 = rozmyty.iter().sum();
//! assert!((suma - 100.0).abs() < 1e-2, "suma = {suma}");
//!
//! // Środek traci jasność, sąsiedzi ją zyskują.
//! let srodek = width * height / 2 + width / 2;
//! assert!(rozmyty[srodek] < 100.0);
//! assert!(rozmyty.iter().filter(|&&v| v > 1.0).count() > 1);
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use crate::gpu::buffer::Buffer;
use crate::gpu::context::Context;
use crate::gpu::kernel::{Binding, BindingKind, Kernel, KernelDesc};
use crate::math::scalar::gaussian;
use crate::ops::run;

/// Parametry splotu przekazywane jako uniform (16 bajtów, wyrównanie WGSL).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    /// Szerokość (liczba wątków na wiersz).
    width: u32,
    /// Wysokość.
    height: u32,
    /// Promień jądra (bez +1).
    radius: u32,
    /// `1` = wierszowo, `0` = kolumnowo.
    horizontal: u32,
}

const CONVOLVE_WGSL: &str = r#"
struct Params { width: u32, height: u32, radius: u32, horizontal: u32 };

@group(0) @binding(0) var<storage, read>       src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read>       kernel: array<f32>;
@group(0) @binding(3) var<uniform>             params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let total = params.width * params.height;
    if (i >= total) { return; }

    let x = i32(i % params.width);
    let y = i32(i / params.width);

    var acc: f32 = 0.0;
    let radius = i32(params.radius);
    for (var k: i32 = -radius; k <= radius; k = k + 1) {
        var sx: i32 = x;
        var sy: i32 = y;
        if (params.horizontal == 1u) {
            sx = x + k;
        } else {
            sy = y + k;
        }

        // Brzeg obrazu: klamrowanie zamiast zawijania.
        sx = clamp(sx, 0, i32(params.width) - 1);
        sy = clamp(sy, 0, i32(params.height) - 1);

        let idx = u32(sy) * params.width + u32(sx);
        acc = acc + src[idx] * kernel[u32(k + radius)];
    }

    dst[i] = acc;
}
"#;

/// Jądro Gaussa znormalizowane do sumy `1`.
///
/// Promień dobierany tak, aby jądro sięgało `3σ` — dalej wkład Gaussa jest
/// pomijalny, a szersze jądro tylko marnuje czas.
pub fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let sigma = sigma.max(1e-3);
    let radius = (3.0 * sigma).ceil().max(1.0) as usize;
    let mut kernel: Vec<f32> = (0..=radius * 2)
        .map(|i| gaussian(i as f32 - radius as f32, sigma))
        .collect();
    let total: f32 = kernel.iter().sum();
    if total.abs() > f32::EPSILON {
        for value in kernel.iter_mut() {
            *value /= total;
        }
    }
    kernel
}

/// Splot 1D z własnym jądrem (klamrowanie na brzegach).
///
/// `kernel` musi mieć **nieparzystą** długość; jej środek trafia na piksel.
pub fn convolve_1d(ctx: &Context, src: &Buffer<f32>, kernel: &[f32]) -> Result<Vec<f32>> {
    if src.is_empty() {
        return Err(Error::empty("wejście"));
    }
    if kernel.is_empty() || kernel.len() % 2 == 0 {
        return Err(Error::size("jądro (nieparzysta długość)", 1, kernel.len()));
    }

    let pass_out = pass(
        ctx,
        src,
        src.len(),
        1,
        kernel,
        (kernel.len() / 2) as u32,
        true,
    )?;
    pass_out.read(ctx)
}

/// Rozmycie Gaussa 1D — `sigma` w pikselach.
pub fn gaussian_1d(ctx: &Context, src: &Buffer<f32>, sigma: f32) -> Result<Vec<f32>> {
    convolve_1d(ctx, src, &gaussian_kernel(sigma))
}

/// Rozmycie Gaussa na obrazie zapisanym wierszowo (`width × height`).
///
/// Dwa przebiegi: po wierszach i po kolumnach. Krawędzie są klamrowane, więc
/// wynik zawsze ma te same wymiary.
pub fn gaussian_2d(
    ctx: &Context,
    src: &Buffer<f32>,
    width: usize,
    height: usize,
    sigma: f32,
) -> Result<Vec<f32>> {
    if width == 0 || height == 0 {
        return Err(Error::empty("wymiary obrazu"));
    }
    if src.len() != width * height {
        return Err(Error::size("obraz", width * height, src.len()));
    }

    let kernel = gaussian_kernel(sigma);
    let radius = (kernel.len() / 2) as u32;

    // Przebieg poziomy: src → tmp, potem pionowy: tmp → out.
    let horizontal = pass(ctx, src, width, height, &kernel, radius, true)?;
    pass(ctx, &horizontal, width, height, &kernel, radius, false)?.read(ctx)
}

/// Jeden przebieg splotu po wierszach albo kolumnach.
fn pass(
    ctx: &Context,
    src: &Buffer<f32>,
    width: usize,
    height: usize,
    kernel: &[f32],
    radius: u32,
    horizontal: bool,
) -> Result<Buffer<f32>> {
    let params = Buffer::uniform(
        ctx,
        &[Params {
            width: width as u32,
            height: height as u32,
            radius,
            horizontal: u32::from(horizontal),
        }],
        "filter.params",
    )?;
    let kernel_buffer = Buffer::from_slice(ctx, kernel, "filter.kernel")?;
    let dst = Buffer::zeros(ctx, src.len(), "filter.out")?;

    let gpu_kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: if horizontal {
                "filter.gaussian_h"
            } else {
                "filter.gaussian_v"
            },
            source: CONVOLVE_WGSL,
            entry_point: "main",
            bindings: &[
                BindingKind::ReadOnlyStorage,
                BindingKind::Storage,
                BindingKind::ReadOnlyStorage,
                BindingKind::Uniform,
            ],
        },
    )?;

    let group = gpu_kernel.bind_group(
        ctx,
        &[
            Binding::read(src),
            Binding::write(&dst),
            Binding::read(&kernel_buffer),
            Binding::uniform(&params),
        ],
    )?;

    run(ctx, |encoder| {
        gpu_kernel.dispatch(encoder, &group, (crate::gpu::groups_for(src.len(), 64), 1, 1))
    })?;

    Ok(dst)
}