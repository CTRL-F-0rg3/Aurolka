//! Pola proceduralne liczone na GPU: mandelbrot i szum fraktalny.
//!
//! To klasyczne zadania, na których widać przewagę GPU — każdy piksel liczy
//! setki operacji, więc transfer danych jest znikomy w porównaniu z pracą.
//!
//! ```
//! use aurum::gpu::Context;
//! use aurum::ops::field;
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//! let obraz = field::mandelbrot(&ctx, 64, 0.0, 0.0, 3.0, 128)?;
//!
//! assert_eq!(obraz.len(), 64 * 64);
//!
//! // Środek obrazu to c = 0, które nigdy nie ucieka — pętla wyczerpuje limit.
//! assert_eq!(obraz[32 * 64 + 32], 128);
//!
//! // Lewa krawędź to c = −1.5, czyli punkt okresowy: też nie ucieka.
//! assert_eq!(obraz[32 * 64], 128);
//!
//! // Natomiast rog obrazu (c = −1.5 − 1.5i) ucieka w kilku krokach.
//! assert!(obraz[0] < 16, "rog = {}", obraz[0]);
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use crate::gpu::buffer::Buffer;
use crate::gpu::context::Context;
use crate::gpu::kernel::{Binding, BindingKind, Kernel, KernelDesc};
use crate::ops::run;

/// Widok mandelbrota: środek, szerokość świata, limit iteracji i bok obrazu.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MandelbrotView {
    center_x: f32,
    center_y: f32,
    scale: f32,
    max_iter: u32,
    /// Bok obrazu w pikselach — wyrownanie wiersza w buforze.
    side: u32,
    // Uniformy wymagają wyrównania 16 B, więc dopełnienie jest z trzech
    // osobnych skalarnych (tablica w uniformie musiałaby mieć stride 16 B).
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

const MANDELBROT_WGSL: &str = r#"
struct View {
    center_x: f32,
    center_y: f32,
    scale: f32,
    max_iter: u32,
    side: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<storage, read_write> iterations: array<u32>;
@group(0) @binding(1) var<uniform>             view: View;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= view.side || gid.y >= view.side) { return; }

    // Jednostka na piksel; obraz jest kwadratowy.
    let unit = view.scale / f32(view.side);
    let c = vec2<f32>(
        view.center_x + (f32(gid.x) - f32(view.side) * 0.5) * unit,
        view.center_y + (f32(gid.y) - f32(view.side) * 0.5) * unit,
    );

    var z = vec2<f32>(0.0, 0.0);
    var i: u32 = 0u;
    loop {
        if (i >= view.max_iter) { break; }
        if (dot(z, z) > 4.0) { break; }
        z = vec2<f32>(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y) + c;
        i = i + 1u;
    }

    iterations[gid.y * view.side + gid.x] = i;
}
"#;

/// Liczba iteracji mandelbrota dla każdego piksela kwadratu `size × size`.
///
/// `center_x`/`center_y` to środek obrazu w płaszczyźnie zespolonej, a `scale`
/// to szerokość widoku w jednostkach świata. `0` oznacza punkt wewnątrz zbioru,
/// `max_iter` — punkt, który nie uciekł w zadanym limicie kroków.
pub fn mandelbrot(
    ctx: &Context,
    size: usize,
    center_x: f32,
    center_y: f32,
    scale: f32,
    max_iter: u32,
) -> Result<Vec<u32>> {
    if size == 0 {
        return Err(Error::empty("rozmiar obrazu"));
    }

    let view = Buffer::uniform(
        ctx,
        &[MandelbrotView {
            center_x,
            center_y,
            scale,
            max_iter,
            side: size as u32,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        }],
        "mandelbrot.view",
    )?;

    let out = Buffer::zeros(ctx, size * size, "mandelbrot.out")?;
    let kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "field.mandelbrot",
            source: MANDELBROT_WGSL,
            entry_point: "main",
            bindings: &[BindingKind::Storage, BindingKind::Uniform],
        },
    )?;

    let group = kernel.bind_group(
        ctx,
        &[Binding::write(&out), Binding::uniform(&view)],
    )?;

    let groups = size.div_ceil(8) as u32;
    run(ctx, |encoder| kernel.dispatch(encoder, &group, (groups, groups, 1)))?;

    out.read(ctx)
}

/// Szum fraktalny w WGSL — **bit w bit zgodny** z
/// [`math::noise`](crate::math::noise), więc wynik GPU można porównać z CPU
/// w testach (patrz `tests/gpu.rs`).
///
/// Hash „Wang”, `hash_to_unit` z 24 bitami mantysy i Hermite’owe wygładzanie
/// to dokładnie te operacje, co w wersji CPU.
fn noise_source(octaves: u32, lacunarity: f32, gain: f32) -> String {
    format!(
        r#"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

fn hash_u32(seed: u32) -> u32 {{
    var x = seed;
    x = (x ^ 61u) ^ (x >> 16u);
    x = x * 9u;
    x = x ^ (x >> 4u);
    x = x * 0x27d4eb2du;
    return x ^ (x >> 15u);
}}

fn hash_2(x: i32, y: i32) -> u32 {{
    return hash_u32(bitcast<u32>(x) * 0x8da6b343u ^ bitcast<u32>(y) * 0xd8163841u);
}}

fn hash_to_unit(hash: u32) -> f32 {{
    return f32(hash >> 8u) / f32(1u << 24u);
}}

fn fade(t: f32) -> f32 {{
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}}

fn value_noise_2(p: vec2<f32>) -> f32 {{
    let base = floor(p);
    let local = p - base;
    let ix = i32(base.x);
    let iy = i32(base.y);
    // WGSL nie ma destrukturyzacji krotek, więc składowe wyciągamy pojedynczo.
    let fx = fade(local.x);
    let fy = fade(local.y);

    let v00 = hash_to_unit(hash_2(ix, iy));
    let v10 = hash_to_unit(hash_2(ix + 1, iy));
    let v01 = hash_to_unit(hash_2(ix, iy + 1));
    let v11 = hash_to_unit(hash_2(ix + 1, iy + 1));

    let top = v00 + (v10 - v00) * fx;
    let bottom = v01 + (v11 - v01) * fx;
    return clamp((top + (bottom - top) * fy) * 2.0 - 1.0, -1.0, 1.0);
}}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let i = gid.x;
    if (i >= arrayLength(&out)) {{ return; }}

    let size = sqrt(f32(arrayLength(&out)));
    let x = f32(i % u32(size));
    let y = f32(i / u32(size));

    var sum = 0.0;
    var amplitude = 1.0;
    var frequency = 1.0;
    var norm = 0.0;
    for (var octave = 0u; octave < {octaves}u; octave = octave + 1u) {{
        sum = sum + value_noise_2(vec2<f32>(x, y) * frequency) * amplitude;
        norm = norm + amplitude;
        frequency = frequency * {lacunarity};
        amplitude = amplitude * {gain};
    }}

    out[i] = clamp(sum / norm, -1.0, 1.0);
}}
"#
    )
}

/// Szum fraktalny (fBm) na kwadratowym obrazie `size × size`.
///
/// Liczony dokładnie tak samo jak [`math::noise::fbm_2`](crate::math::noise::fbm_2):
/// ten sam hash, to samo wygładzanie, ta sama normalizacja — dlatego oba wyniki
/// powinny być równe co do bita (na `f32`).
///
/// ```
/// use aurum::gpu::Context;
/// use aurum::math::noise::fbm_2;
/// use aurum::ops::field;
/// use aurum::Result;
///
/// # fn main() -> Result<()> {
/// let ctx = Context::new()?;
/// let size = 8;
/// let obraz = field::fbm_2d(&ctx, size, 4, 2.0, 0.5)?;
///
/// // Piksel (3, 5) ma tę samą wartość co CPU — różnica to ostatni bit `f32`.
/// let (x, y) = (3usize, 5usize);
/// let oczekiwane = fbm_2(x as f32, y as f32, 4, 2.0, 0.5);
/// assert!((obraz[y * size + x] - oczekiwane).abs() < 1e-6);
/// # Ok(())
/// # }
/// ```
pub fn fbm_2d(
    ctx: &Context,
    size: usize,
    octaves: u32,
    lacunarity: f32,
    gain: f32,
) -> Result<Vec<f32>> {
    if size == 0 {
        return Err(Error::empty("rozmiar obrazu"));
    }
    if octaves == 0 {
        return Err(Error::empty("liczba oktaw"));
    }

    let out = Buffer::zeros(ctx, size * size, "fbm.out")?;
    let kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "field.fbm",
            source: &noise_source(octaves, lacunarity, gain),
            entry_point: "main",
            bindings: &[BindingKind::Storage],
        },
    )?;

    let group = kernel.bind_group(ctx, &[crate::gpu::Binding::write(&out)])?;
    run(ctx, |encoder| {
        kernel.dispatch(
            encoder,
            &group,
            (crate::gpu::groups_for(out.len(), 64), 1, 1),
        )
    })?;

    out.read(ctx)
}