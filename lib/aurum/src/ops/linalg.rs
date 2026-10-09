//! Algebra liniowa: mnożenie macierzy, transpozycja i iloczyn skalarny.
//!
//! Macierze są jednowymiarowymi buforami `f32` w zapisie **wierszowo**
//! (`a[row * k + col]`), tak samo jak [`math::matrix`](crate::math::matrix) na
//! CPU. To najwygodniejszy układ dla buforów, a kernel sam liczy indeksy.
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::ops::linalg;
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//!
//! // 2×2 · 2×2
//! let a = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0, 4.0], "a")?;
//! let b = Buffer::from_slice(&ctx, &[5.0f32, 6.0, 7.0, 8.0], "b")?;
//!
//! assert_eq!(linalg::mat_mul(&ctx, &a, &b, 2, 2, 2)?, vec![19.0, 22.0, 43.0, 50.0]);
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use crate::gpu::buffer::Buffer;
use crate::gpu::context::Context;
use crate::gpu::kernel::{Binding, BindingKind, Kernel, KernelDesc};
use crate::ops::run;

/// Wymiary przekazywane do shadera jako uniform (16 bajtów, wyrównanie WGSL).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Dims {
    /// Liczba wierszy (w `mat_mul` — wierszy wyniku).
    pub rows: u32,
    /// Liczba kolumn.
    pub cols: u32,
    /// Wymiar wspólny (`inner` w `mat_mul`).
    pub inner: u32,
    /// dopełnienie do 16 bajtów.
    pub _pad: u32,
}

const MAT_MUL_WGSL: &str = r#"
struct Dims { rows: u32, cols: u32, inner: u32, _pad: u32 };

@group(0) @binding(0) var<storage, read>       a: array<f32>;
@group(0) @binding(1) var<storage, read>       b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@group(0) @binding(3) var<uniform>             dims: Dims;

// Kafelki 16×16 = 256 elementów; indeks `lid.y * 16 + lid.x` sięga dokładnie
// tej długości.
var<workgroup> tile_a: array<f32, 256>;
var<workgroup> tile_b: array<f32, 256>;

@compute @workgroup_size(16, 16)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let row = gid.y;
    let col = gid.x;

    var acc: f32 = 0.0;
    let tiles = (dims.inner + 15u) / 16u;

    for (var t: u32 = 0u; t < tiles; t = t + 1u) {
        // Kafelki 16×16 w pamięci współdzielonej — mniej odczytów z RAM.
        let a_col = t * 16u + lid.x;
        let b_row = t * 16u + lid.y;

        var av: f32 = 0.0;
        if (row < dims.rows && a_col < dims.inner) {
            av = a[row * dims.inner + a_col];
        }
        var bv: f32 = 0.0;
        if (b_row < dims.inner && col < dims.cols) {
            bv = b[b_row * dims.cols + col];
        }

        tile_a[lid.y * 16u + lid.x] = av;
        tile_b[lid.y * 16u + lid.x] = bv;
        workgroupBarrier();

        for (var k: u32 = 0u; k < 16u; k = k + 1u) {
            acc = acc + tile_a[lid.y * 16u + k] * tile_b[k * 16u + lid.x];
        }
        workgroupBarrier();
    }

    if (row < dims.rows && col < dims.cols) {
        c[row * dims.cols + col] = acc;
    }
}
"#;

const TRANSPOSE_WGSL: &str = r#"
struct Dims { rows: u32, cols: u32, inner: u32, _pad: u32 };

@group(0) @binding(0) var<storage, read>       src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<uniform>             dims: Dims;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.y;
    let col = gid.x;
    if (row >= dims.rows || col >= dims.cols) { return; }
    dst[col * dims.rows + row] = src[row * dims.cols + col];
}
"#;

/// Mnożenie macierzy: `c = a · b` o wymiarach `rows × inner · inner × cols`.
///
/// Kafelkowane po 16×16 z pamięcią współdzieloną; radzi sobie z macierzami
/// o dowolnych rozmiarach (nie tylko wielokrotnościach 16).
pub fn mat_mul(
    ctx: &Context,
    a: &Buffer<f32>,
    b: &Buffer<f32>,
    rows: usize,
    inner: usize,
    cols: usize,
) -> Result<Vec<f32>> {
    check_dims(rows, inner, cols)?;
    if a.len() != rows * inner {
        return Err(Error::size("a", rows * inner, a.len()));
    }
    if b.len() != inner * cols {
        return Err(Error::size("b", inner * cols, b.len()));
    }

    let dims = dims_buffer(ctx, rows, cols, inner, "matmul.dims")?;
    let c = Buffer::zeros(ctx, rows * cols, "matmul.out")?;

    let kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "linalg.mat_mul",
            source: MAT_MUL_WGSL,
            entry_point: "main",
            bindings: &[
                BindingKind::ReadOnlyStorage,
                BindingKind::ReadOnlyStorage,
                BindingKind::Storage,
                BindingKind::Uniform,
            ],
        },
    )?;

    // Uniform przekazujemy jako `Binding::Uniform` — inaczej biblioteka
    // odrzuciłaby go jako niezgodny rodzaj bindingu.
    let group = kernel.bind_group(
        ctx,
        &[
            Binding::read(a),
            Binding::read(b),
            Binding::write(&c),
            Binding::uniform(&dims),
        ],
    )?;

    run(ctx, |encoder| {
        kernel.dispatch(
            encoder,
            &group,
            (cols.div_ceil(16) as u32, rows.div_ceil(16) as u32, 1),
        )
    })?;

    c.read(ctx)
}

/// Transpozycja macierzy `rows × cols` zapisanej wierszowo.
pub fn transpose(ctx: &Context, src: &Buffer<f32>, rows: usize, cols: usize) -> Result<Vec<f32>> {
    if rows == 0 || cols == 0 {
        return Err(Error::empty("macierz"));
    }
    if src.len() != rows * cols {
        return Err(Error::size("macierz", rows * cols, src.len()));
    }

    let dims = dims_buffer(ctx, rows, cols, 0, "transpose.dims")?;
    let dst = Buffer::zeros(ctx, rows * cols, "transpose.out")?;

    let kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "linalg.transpose",
            source: TRANSPOSE_WGSL,
            entry_point: "main",
            bindings: &[
                BindingKind::ReadOnlyStorage,
                BindingKind::Storage,
                BindingKind::Uniform,
            ],
        },
    )?;

    let group = kernel.bind_group(ctx, &[Binding::read(src), Binding::write(&dst), Binding::uniform(&dims)])?;
    run(ctx, |encoder| {
        kernel.dispatch(
            encoder,
            &group,
            (cols.div_ceil(16) as u32, rows.div_ceil(16) as u32, 1),
        )
    })?;

    dst.read(ctx)
}

/// Mnożenie macierzy przez wektor: `out = a · v` (o `cols` składowych).
///
/// Liczone elementowo i sumowane po kolumnach — dla rozsądnych rozmiarów
/// szybciej niż osobny kernel z atomikami.
pub fn mat_vec(
    ctx: &Context,
    a: &Buffer<f32>,
    v: &[f32],
    rows: usize,
    cols: usize,
) -> Result<Vec<f32>> {
    if rows == 0 || cols == 0 {
        return Err(Error::empty("wymiary macierzy"));
    }
    if a.len() != rows * cols {
        return Err(Error::size("macierz", rows * cols, a.len()));
    }
    if v.len() != cols {
        return Err(Error::size("wektor", cols, v.len()));
    }

    let column = Buffer::from_slice(ctx, v, "matvec.v")?;
    let products = crate::ops::elementwise::mul_buffer(ctx, a, &column)?.read(ctx)?;

    let mut out = vec![0.0f32; rows];
    for (row, chunk) in products.chunks(cols).enumerate() {
        out[row] = chunk.iter().sum();
    }
    Ok(out)
}

/// Iloczyn skalarny dwóch wektorów — `sum(a[i] · b[i])`.
pub fn dot(ctx: &Context, a: &Buffer<f32>, b: &Buffer<f32>) -> Result<f32> {
    let products = crate::ops::elementwise::mul_buffer(ctx, a, b)?;
    crate::ops::reduce::sum(ctx, &products)
}

/// Bufor uniformowy z wymiarami dla shadera.
fn dims_buffer(ctx: &Context, rows: usize, cols: usize, inner: usize, label: &str) -> Result<Buffer<Dims>> {
    Buffer::uniform(
        ctx,
        &[Dims {
            rows: rows as u32,
            cols: cols as u32,
            inner: inner as u32,
            _pad: 0,
        }],
        label,
    )
}

/// Sprawdza, czy wymiary są dodatnie.
fn check_dims(rows: usize, inner: usize, cols: usize) -> Result<()> {
    if rows == 0 || inner == 0 || cols == 0 {
        return Err(Error::empty("wymiary macierzy"));
    }
    Ok(())
}