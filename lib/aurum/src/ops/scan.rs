//! Sumy prefiksowe (scan).
//!
//! `prefix_sum[i] = x[0] + … + x[i]`. To jedyna operacja, w której **kolejność**
//! ma znaczenie, więc nie da się jej zrównoleglić na GPU tak jak sumy.
//! Implementacja dzieli dane na bloki 256 elementów: sumy bloków liczą się
//! równolegle, ich sumy prefiksowe (ich jest n/256, czyli mało) powstają na
//! CPU, a właściwy scan dostaje przesunięcie jako bufor uniformowy.
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::ops::scan;
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//! let x = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0, 4.0], "x")?;
//!
//! assert_eq!(scan::prefix_sum(&ctx, &x)?, vec![1.0, 3.0, 6.0, 10.0]);
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use crate::gpu::buffer::Buffer;
use crate::gpu::context::Context;
use crate::gpu::kernel::{Binding, BindingKind, Kernel, KernelDesc};
use crate::ops::run;

/// Rozmiar bloku (i grupy roboczej) w skanie.
const WG: usize = 256;

/// Kernel sum prefiksowych: skan drzewowy (Hillis–Steele) wewnątrz bloku
/// plus przesunięcie o sumę poprzednich bloków.
const SCAN_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read>       src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read>       offsets: array<f32>;

var<workgroup> scratch: array<f32, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(local_invocation_index) lindex: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let n = arrayLength(&src);
    let base = wid.x * 256u + lindex;

    // Wczytanie kawałka; poza końcem danych wartość jest zerem.
    var value: f32 = 0.0;
    if (base < n) { value = src[base]; }
    scratch[lindex] = value;
    workgroupBarrier();

    // Drzewo wstępujące — log2(256) = 8 kroków. Kolejność „wszystkie odczyty,
    // potem wszystkie zapisy” jest tu obowiązkowa.
    var stride: u32 = 1u;
    loop {
        if (stride >= 256u) { break; }
        var addend: f32 = 0.0;
        if (lindex >= stride) { addend = scratch[lindex - stride]; }
        workgroupBarrier();
        if (lindex >= stride) { scratch[lindex] = scratch[lindex] + addend; }
        workgroupBarrier();
        stride = stride * 2u;
    }

    // Przesunięcie o sumę poprzednich bloków dodajemy **po** skanie — gdyby
    // dodać je wcześniej, każdy element uwzględniłby je (lindex + 1)-krotnie.
    let offset = offsets[wid.x];
    if (base < n) { dst[base] = scratch[lindex] + offset; }
}
"#;

/// Kernel liczący sumę każdego bloku — zwraca `blocks` liczb.
const BLOCK_SUM_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read>       src: array<f32>;
@group(0) @binding(1) var<storage, read_write> sums: array<f32>;

var<workgroup> scratch: array<f32, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let n = arrayLength(&src);
    var value: f32 = 0.0;
    if (gid.x < n) { value = src[gid.x]; }

    scratch[lid.x] = value;
    workgroupBarrier();

    var stride: u32 = 128u;
    loop {
        if (stride == 0u) { break; }
        if (lid.x < stride) { scratch[lid.x] = scratch[lid.x] + scratch[lid.x + stride]; }
        workgroupBarrier();
        stride = stride / 2u;
    }

    if (lid.x == 0u) { sums[wid.x] = scratch[0]; }
}
"#;

/// Sumy prefiksowe — `dst[i] = src[0] + … + src[i]`.
///
/// Wymaga niepustego wejścia. Wynik trafia na CPU, bo tak przewidują to
/// odpowiedniki z [`math::stats`](crate::math::stats); w pętlach, gdzie wynik
/// zostaje na GPU, wygodniej użyć kernelu ze źródłem.
pub fn prefix_sum(ctx: &Context, src: &Buffer<f32>) -> Result<Vec<f32>> {
    let n = src.len();
    if n == 0 {
        return Err(Error::empty("wejście"));
    }

    let blocks = n.div_ceil(WG);

    // 1. Suma każdego bloku 256 elementów — równolegle.
    let block_sums: Buffer<f32> = Buffer::zeros(ctx, blocks, "scan.block_sums")?;
    let block_kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "scan.block_sum",
            source: BLOCK_SUM_WGSL,
            entry_point: "main",
            bindings: &[BindingKind::ReadOnlyStorage, BindingKind::Storage],
        },
    )?;
    let block_group =
        block_kernel.bind_group(ctx, &[Binding::read(src), Binding::write(&block_sums)])?;
    run(ctx, |encoder| {
        block_kernel.dispatch(encoder, &block_group, (blocks as u32, 1, 1))
    })?;

    // 2. Suma poprzednich bloków dla każdego bloku. Bloków jest n/256, więc
    //    dla typowych rozmiarów danych to ułamek tysiąca liczb.
    let sums = block_sums.read(ctx)?;
    let mut offsets = Vec::with_capacity(blocks);
    let mut running = 0.0f32;
    for &s in &sums {
        offsets.push(running);
        running += s;
    }
    let offsets_buffer = Buffer::from_slice(ctx, &offsets, "scan.offsets")?;

    // 3. Właściwy scan z przesunięciami.
    let dst = Buffer::zeros(ctx, n, "scan.dst")?;
    let scan_kernel = Kernel::new(
        ctx,
        KernelDesc {
            label: "scan.prefix_sum",
            source: SCAN_WGSL,
            entry_point: "main",
            bindings: &[
                BindingKind::ReadOnlyStorage,
                BindingKind::Storage,
                BindingKind::ReadOnlyStorage,
            ],
        },
    )?;
    let scan_group = scan_kernel.bind_group(
        ctx,
        &[
            Binding::read(src),
            Binding::write(&dst),
            Binding::read(&offsets_buffer),
        ],
    )?;
    run(ctx, |encoder| {
        scan_kernel.dispatch(encoder, &scan_group, (blocks as u32, 1, 1))
    })?;

    dst.read(ctx)
}