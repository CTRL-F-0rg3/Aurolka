//! Redukcje: sumowanie i wyszukiwanie skrajnych wartości w jednym przebiegu.
//!
//! Redukcja na GPU idzie dwuetapowo: najpierw każda grupa robocza (256 wątków)
//! liczy wynik dla swojego kawałka danych, potem **jedna** grupa scala te
//! wyniki. Dzięki temu równoległość jest równa liczbie jednostek SM, a nie
//! liczbie wątków.
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::ops::reduce;
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//! let x = Buffer::from_slice(&ctx, &[3.0f32, -1.0, 7.0, 2.0], "x")?;
//!
//! assert_eq!(reduce::sum(&ctx, &x)?, 11.0);
//! assert_eq!(reduce::min(&ctx, &x)?, -1.0);
//! assert_eq!(reduce::max(&ctx, &x)?, 7.0);
//! assert_eq!(reduce::argmax(&ctx, &x)?.0, 2); // indeks 2 ma wartość 7.0
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use crate::gpu::buffer::Buffer;
use crate::gpu::context::Context;
use crate::gpu::kernel::{groups_for, Binding, BindingKind, Kernel, KernelDesc};
use crate::math::Vec2;
use crate::ops::run;

/// Liczba wątków w grupie roboczej redukcji.
const WG: u32 = 256;

/// Redukcja wartości `f32` wykonywana w pamięci współdzielonej grupy roboczej.
fn reduce_source(init: &str, op: &str, combine: &str) -> String {
    format!(
        r#"
@group(0) @binding(0) var<storage, read>       src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;

var<workgroup> scratch: array<f32, {WG}>;

@compute @workgroup_size({WG})
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {{
    var value: f32 = {init};
    if (gid.x < arrayLength(&src)) {{
        value = {op};
    }}

    scratch[lid.x] = value;
    workgroupBarrier();

    // Redukcja drzewa w pamięci współdzielonej — z log2(256) = 8 krokami.
    var stride: u32 = {WG} / 2u;
    loop {{
        if (stride == 0u) {{ break; }}
        if (lid.x < stride) {{
            scratch[lid.x] = {combine};
        }}
        workgroupBarrier();
        stride = stride / 2u;
    }}

    if (lid.x == 0u) {{
        dst[wid.x] = scratch[0];
    }}
}}
"#
    )
}

/// Redukcja pary `(wartość, indeks)` — dla `argmax` i `argmin`.
///
/// `partially` steruje tym, czym jest wejście: `false` to bufor `f32`
/// (pierwszy przebieg), `true` to `vec2<f32>` z wynikami cząstkowymi.
fn arg_reduce_source(is_max: bool, partially: bool) -> String {
    let better = if is_max { ">" } else { "<" };
    let init = if is_max {
        "-3.402823466e+38"
    } else {
        "3.402823466e+38"
    };
    // W drugim przebiegu wejściem są już pary (wartość, indeks).
    let (src_decl, load, dst_decl) = if partially {
        (
            "var<storage, read> src: array<vec2<f32>>;",
            "value = src[gid.x].x;\n        index = u32(src[gid.x].y);",
            "var<storage, read_write> dst: array<vec2<f32>>;",
        )
    } else {
        (
            "var<storage, read> src: array<f32>;",
            "value = src[gid.x];\n        index = gid.x;",
            "var<storage, read_write> dst: array<vec2<f32>>;",
        )
    };

    format!(
        r#"
@group(0) @binding(0) {src_decl}
@group(0) @binding(1) {dst_decl}

var<workgroup> shared_value: array<f32, {WG}>;
var<workgroup> shared_index: array<u32, {WG}>;

@compute @workgroup_size({WG})
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {{
    var value: f32 = {init};
    var index: u32 = 0xffffffffu;
    if (gid.x < arrayLength(&src)) {{
        {load}
    }}

    shared_value[lid.x] = value;
    shared_index[lid.x] = index;
    workgroupBarrier();

    var stride: u32 = {WG} / 2u;
    loop {{
        if (stride == 0u) {{ break; }}
        if (lid.x < stride) {{
            let other = shared_value[lid.x + stride];
            if (other {better} shared_value[lid.x]) {{
                shared_value[lid.x] = other;
                shared_index[lid.x] = shared_index[lid.x + stride];
            }}
        }}
        workgroupBarrier();
        stride = stride / 2u;
    }}

    if (lid.x == 0u) {{
        dst[wid.x] = vec2<f32>(shared_value[0], f32(shared_index[0]));
    }}
}}
"#
    )
}

/// Wykonuje redukcję i zwraca wynik jako `f32`.
fn reduce_scalar(ctx: &Context, src: &Buffer<f32>, label: &str, source: String) -> Result<f32> {
    let len = src.len();
    if len == 0 {
        return Err(Error::empty("wejście"));
    }

    let partials = groups_for(len, WG) as usize;
    let partial: Buffer<f32> = Buffer::zeros(ctx, partials, "reduce.partial")?;
    let total: Buffer<f32> = Buffer::zeros(ctx, partials, "reduce.total")?;

    let kernel = Kernel::new(
        ctx,
        KernelDesc {
            label,
            source: &source,
            entry_point: "main",
            bindings: &[BindingKind::ReadOnlyStorage, BindingKind::Storage],
        },
    )?;

    let first = kernel.bind_group(
        ctx,
        &[Binding::read(src), Binding::write(&partial)],
    )?;
    let second = kernel.bind_group(
        ctx,
        &[Binding::read(&partial), Binding::write(&total)],
    )?;

    run(ctx, |encoder| {
        // Etap 1: każda grupa liczy swój kawałek.
        kernel.dispatch(encoder, &first, (partials as u32, 1, 1))?;
        // Etap 2: jedna grupa scala wyniki etapu 1.
        kernel.dispatch(encoder, &second, (1, 1, 1))
    })?;

    let result = total.read(ctx)?;
    Ok(result.first().copied().unwrap_or(f32::NAN))
}

/// Suma wszystkich elementów.
pub fn sum(ctx: &Context, src: &Buffer<f32>) -> Result<f32> {
    reduce_scalar(
        ctx,
        src,
        "reduce.sum",
        reduce_source("0.0", "src[gid.x]", "scratch[lid.x] + scratch[lid.x + stride]"),
    )
}

/// Najmniejszy element.
pub fn min(ctx: &Context, src: &Buffer<f32>) -> Result<f32> {
    reduce_scalar(
        ctx,
        src,
        "reduce.min",
        reduce_source(
            "3.402823466e+38",
            "src[gid.x]",
            "min(scratch[lid.x], scratch[lid.x + stride])",
        ),
    )
}

/// Największy element.
pub fn max(ctx: &Context, src: &Buffer<f32>) -> Result<f32> {
    reduce_scalar(
        ctx,
        src,
        "reduce.max",
        reduce_source(
            "-3.402823466e+38",
            "src[gid.x]",
            "max(scratch[lid.x], scratch[lid.x + stride])",
        ),
    )
}

/// Średnia arytmetyczna.
pub fn mean(ctx: &Context, src: &Buffer<f32>) -> Result<f32> {
    let len = src.len();
    if len == 0 {
        return Err(Error::empty("wejście"));
    }
    Ok(sum(ctx, src)? / len as f32)
}

/// Indeks i wartość największego elementu.
///
/// Przy remisie wygrywa **pierwsze** wystąpienie — tak samo jak
/// [`stats::argmax`](crate::math::stats::argmax).
pub fn argmax(ctx: &Context, src: &Buffer<f32>) -> Result<(usize, f32)> {
    arg_reduce(ctx, src, true)
}

/// Indeks i wartość najmniejszego elementu.
pub fn argmin(ctx: &Context, src: &Buffer<f32>) -> Result<(usize, f32)> {
    arg_reduce(ctx, src, false)
}

/// Wspólna część `argmax` i `argmin`.
///
/// Dwa różne kernele, bo drugi przebieg czyta **inny typ** wejścia: pary
/// `(wartość, indeks)` jako `vec2<f32>` zamiast gołych `f32`.
fn arg_reduce(ctx: &Context, src: &Buffer<f32>, is_max: bool) -> Result<(usize, f32)> {
    let len = src.len();
    if len == 0 {
        return Err(Error::empty("wejście"));
    }

    let partials = groups_for(len, WG) as usize;
    let partial: Buffer<Vec2> = Buffer::zeros(ctx, partials, "reduce.arg.partial")?;
    let total: Buffer<Vec2> = Buffer::zeros(ctx, partials, "reduce.arg.total")?;

    let label = if is_max { "reduce.argmax" } else { "reduce.argmin" };
    let bindings = [BindingKind::ReadOnlyStorage, BindingKind::Storage];

    // Przebieg 1: f32 → pary (wartość, indeks).
    let first_kernel = Kernel::new(
        ctx,
        KernelDesc {
            label,
            source: &arg_reduce_source(is_max, false),
            entry_point: "main",
            bindings: &bindings,
        },
    )?;
    let first_group = first_kernel.bind_group(ctx, &[Binding::read(src), Binding::write(&partial)])?;

    // Przebieg 2: pary → jedna para.
    let second_kernel = Kernel::new(
        ctx,
        KernelDesc {
            label,
            source: &arg_reduce_source(is_max, true),
            entry_point: "main",
            bindings: &bindings,
        },
    )?;
    let second_group =
        second_kernel.bind_group(ctx, &[Binding::read(&partial), Binding::write(&total)])?;

    run(ctx, |encoder| {
        first_kernel.dispatch(encoder, &first_group, (partials as u32, 1, 1))?;
        second_kernel.dispatch(encoder, &second_group, (1, 1, 1))
    })?;

    let result = total.read(ctx)?;
    let best = result.first().copied().unwrap_or(Vec2::ZERO);

    // Pozycje wracają przez `f32`, więc indeks jest dokładny do 2²⁴.
    if best.y < 0.0 {
        return Ok((0, best.x));
    }
    Ok((best.y as usize, best.x))
}