//! Gotowe obliczenia na GPU — cienka warstwa nad [`gpu`](crate::gpu).
//!
//! Każda funkcja sama tworzy (lub bierze z cache’a) kernel, wysyła dispatch
//! i czeka na wynik. Do własnych shaderów służy niższa warstwa.
//!
//! | Moduł | Operacje |
//! |---|---|
//! | [`elementwise`] | `map`, `zip`, `axpy`, `scale` — obliczenia na `f32` |
//! | [`reduce`] | `sum`, `min`, `max`, `mean`, `argmax`, `argmin`, iloczyn skalarny |
//! | [`scan`] | sumy prefiksowe |
//! | [`linalg`] | mnożenie macierzy, transpozycja, iloczyn wektorów |
//! | [`filter`] | splot Gaussa 1D i 2D, konwolucja z własnym jądrem |
//! | [`histogram`] | histogram wartości `f32` (na atomikach GPU) |
//! | [`field`] | pola proceduralne — mandelbrot, szum fBm na GPU |
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::{ops, Result};
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//! let x = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0, 4.0], "x")?;
//!
//! assert_eq!(ops::reduce::sum(&ctx, &x)?, 10.0);
//! assert_eq!(ops::reduce::mean(&ctx, &x)?, 2.5);
//! assert_eq!(ops::reduce::max(&ctx, &x)?, 4.0);
//! # Ok(())
//! # }
//! ```
//!
//! # Kiedy GPU się opłaca
//!
//! Powyżej około 10 tys. elementów przewaga GPU rośnie szybko. Poniżej —
//! transfer danych i synchronizacja kosztują więcej niż obliczenie. Testy
//! porównują obie strony (`tests/gpu.rs`), żeby widać było, gdzie jest granica.

pub mod elementwise;
pub mod field;
pub mod filter;
pub mod histogram;
pub mod linalg;
pub mod reduce;
pub mod scan;

use crate::error::Result;
use crate::gpu::context::Context;

/// Wysyła komendy i czeka na ich wykonanie.
///
/// Wszystkie operacje z tego modułu kończą się wynikiem na CPU, więc
/// synchronizacja jest częścią ich kontraktu.
pub(crate) fn run(ctx: &Context, record: impl FnOnce(&mut wgpu::CommandEncoder) -> Result<()>) -> Result<()> {
    ctx.submit(record)?;
    ctx.poll()
}

/// Rozmiar grupy roboczej używany przez większość operacji elementwise.
pub(crate) const WORKGROUP: u32 = 64;