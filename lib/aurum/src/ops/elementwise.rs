//! Obliczenia na elementach wektorów `f32`.
//!
//! Ekspresja jest **tekstem WGSL**, nie domknięciem — dlatego te operacje
//! działają dla dowolnej arytmetyki obsługiwanej przez `wgpu`, a wygenerowany
//! dla danego wyrażenia pipeline jest trzymany w cache’u. W wyrażeniu widoczne
//! są zmienne `x` (pierwszy bufor) oraz `x`, `y` (dwa bufory).
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::ops::elementwise;
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//! let x = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0], "x")?;
//! let out = elementwise::map(&ctx, &x, "sin(x)")?;
//! assert!((out[0] - 1.0f32.sin()).abs() < 1e-6);
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use crate::gpu::buffer::Buffer;
use crate::gpu::context::Context;
use crate::gpu::kernel::{groups_for, Binding, BindingKind, Kernel, KernelDesc};
use crate::ops::{run, WORKGROUP};

/// Szablon shadera dla operacji jednoargumentowej.
fn map_source(expr: &str) -> String {
    format!(
        r#"
@group(0) @binding(0) var<storage, read>       src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;

@compute @workgroup_size({WORKGROUP})
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let i = gid.x;
    if (i >= arrayLength(&src)) {{ return; }}
    let x = src[i];
    dst[i] = {expr};
}}
"#
    )
}

/// Szablon shadera dla operacji dwuargumentowej.
fn zip_source(expr: &str) -> String {
    format!(
        r#"
@group(0) @binding(0) var<storage, read>       a: array<f32>;
@group(0) @binding(1) var<storage, read>       b: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;

@compute @workgroup_size({WORKGROUP})
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let i = gid.x;
    if (i >= min(arrayLength(&a), arrayLength(&b))) {{ return; }}
    let x = a[i];
    let y = b[i];
    dst[i] = {expr};
}}
"#
    )
}

/// Liczy wyrażenie WGSL dla każdego elementu i zwraca **wynik na CPU**.
///
/// `expr` widzi zmienną `x` — np. `"x * x + 1.0"`, `"sin(x)"`, `"exp(-x)"`.
///
/// ```
/// use aurum::gpu::{Buffer, Context};
/// use aurum::ops::elementwise;
/// use aurum::Result;
///
/// # fn main() -> Result<()> {
/// let ctx = Context::new()?;
/// let x = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0], "x")?;
///
/// let kwadraty = elementwise::map(&ctx, &x, "x * x")?;
/// assert_eq!(kwadraty, vec![1.0, 4.0, 9.0]);
/// # Ok(())
/// # }
/// ```
pub fn map(ctx: &Context, src: &Buffer<f32>, expr: &str) -> Result<Vec<f32>> {
    let dst = map_buffer(ctx, src, expr)?;
    dst.read(ctx)
}

/// Liczy wyrażenie dla dwóch wektorów i zwraca wynik na CPU.
///
/// `expr` widzi zmienne `x` i `y`.
///
/// ```
/// use aurum::gpu::{Buffer, Context};
/// use aurum::ops::elementwise;
/// use aurum::Result;
///
/// # fn main() -> Result<()> {
/// let ctx = Context::new()?;
/// let a = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0], "a")?;
/// let b = Buffer::from_slice(&ctx, &[10.0f32, 20.0, 30.0], "b")?;
///
/// let suma = elementwise::zip(&ctx, &a, &b, "x + y")?;
/// assert_eq!(suma, vec![11.0, 22.0, 33.0]);
/// # Ok(())
/// # }
/// ```
pub fn zip(ctx: &Context, a: &Buffer<f32>, b: &Buffer<f32>, expr: &str) -> Result<Vec<f32>> {
    let dst = zip_buffer(ctx, a, b, expr)?;
    dst.read(ctx)
}

/// Buduje nowy bufor z wynikiem wyrażenia liczonego elementowo.
pub(crate) fn map_buffer(ctx: &Context, src: &Buffer<f32>, expr: &str) -> Result<Buffer<f32>> {
    if src.is_empty() {
        return Err(Error::empty("źródło"));
    }

    let dst = Buffer::zeros(ctx, src.len(), "elementwise.out")?;
    let kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "elementwise.map",
            source: &map_source(expr),
            entry_point: "main",
            bindings: &[BindingKind::ReadOnlyStorage, BindingKind::Storage],
        },
    )?;

    let group = kernel.bind_group(ctx, &[Binding::read(src), Binding::write(&dst)])?;
    run(ctx, |encoder| {
        kernel.dispatch(encoder, &group, (groups_for(src.len(), WORKGROUP), 1, 1))
    })?;

    Ok(dst)
}

/// Buduje nowy bufor z wynikiem wyrażenia liczonego dla dwóch wektorów.
pub(crate) fn zip_buffer(ctx: &Context, a: &Buffer<f32>, b: &Buffer<f32>, expr: &str) -> Result<Buffer<f32>> {
    if a.is_empty() {
        return Err(Error::empty("źródło"));
    }
    if a.len() != b.len() {
        return Err(Error::size("zip", a.len(), b.len()));
    }

    let dst = Buffer::zeros(ctx, a.len(), "elementwise.zip")?;
    let kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "elementwise.zip",
            source: &zip_source(expr),
            entry_point: "main",
            bindings: &[
                BindingKind::ReadOnlyStorage,
                BindingKind::ReadOnlyStorage,
                BindingKind::Storage,
            ],
        },
    )?;

    let group = kernel.bind_group(ctx, &[Binding::read(a), Binding::read(b), Binding::write(&dst)])?;
    run(ctx, |encoder| {
        kernel.dispatch(encoder, &group, (groups_for(a.len(), WORKGROUP), 1, 1))
    })?;

    Ok(dst)
}

/// Iloczyn skalarny: `out[i] = alpha * x[i] + y[i]`.
///
/// Najczęstsza operacja w iteracyjnych symulacjach (relaksacja cząsteczek,
/// gradienty, mnożenie przez stałą w iteracji).
///
/// ```
/// use aurum::gpu::{Buffer, Context};
/// use aurum::ops::elementwise;
/// use aurum::Result;
///
/// # fn main() -> Result<()> {
/// let ctx = Context::new()?;
/// let x = Buffer::from_slice(&ctx, &[1.0f32, 2.0], "x")?;
/// let y = Buffer::from_slice(&ctx, &[10.0f32, 20.0], "y")?;
///
/// let out = elementwise::axpy(&ctx, &x, &y, 0.5)?;
/// assert_eq!(out, vec![10.5, 21.0]);
/// # Ok(())
/// # }
/// ```
pub fn axpy(ctx: &Context, x: &Buffer<f32>, y: &Buffer<f32>, alpha: f32) -> Result<Vec<f32>> {
    let expr = format!("{alpha} * x + y");
    zip(ctx, x, y, &expr)
}

/// Mnożenie przez skalę: `out[i] = alpha * x[i]`.
pub fn scale(ctx: &Context, x: &Buffer<f32>, alpha: f32) -> Result<Vec<f32>> {
    let expr = format!("{alpha} * x");
    map(ctx, x, &expr)
}

/// Dodawanie dwóch wektorów.
pub fn add(ctx: &Context, a: &Buffer<f32>, b: &Buffer<f32>) -> Result<Vec<f32>> {
    zip(ctx, a, b, "x + y")
}

/// Odejmowanie dwóch wektorów.
pub fn sub(ctx: &Context, a: &Buffer<f32>, b: &Buffer<f32>) -> Result<Vec<f32>> {
    zip(ctx, a, b, "x - y")
}

/// Mnożenie składowe.
pub fn mul(ctx: &Context, a: &Buffer<f32>, b: &Buffer<f32>) -> Result<Vec<f32>> {
    zip(ctx, a, b, "x * y")
}

/// Mnożenie składowe dwóch buforów, z **wynikiem w buforze GPU**.
///
/// Wewnętrzna wersja [`mul`] — nie robi odczytu, więc można ją używać jako
/// kroku w dłuższym potoku na GPU.
pub(crate) fn mul_buffer(ctx: &Context, a: &Buffer<f32>, b: &Buffer<f32>) -> Result<Buffer<f32>> {
    zip_buffer(ctx, a, b, "x * y")
}

/// Ograniczenie do `[min, max]`.
pub fn clamp(ctx: &Context, x: &Buffer<f32>, min: f32, max: f32) -> Result<Vec<f32>> {
    let expr = format!("clamp(x, {min}, {max})");
    map(ctx, x, &expr)
}