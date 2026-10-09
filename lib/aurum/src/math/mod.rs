//! Matematyka na CPU — bez GPU, bez zależności.
//!
//! Wszystko, co jest potrzebne do policzenia wyniku na procesorze, zanim warto
//! przenieść dane na GPU (albo żeby porównać wynik GPU z CPU w testach).
//!
//! | Moduł | Zawartość |
//! |---|---|
//! | [`scalar`] | `clamp`, `lerp`, `remap`, `smoothstep`, funkcje wygładzające |
//! | [`vector`] | [`Vec2`], [`Vec3`], [`Vec4`] — typy `Pod`, gotowe do buforów |
//! | [`matrix`] | [`Mat2`], [`Mat3`], [`Mat4`] wierszowo, z odwrotnością i transpozycją |
//! | [`quat`] | kwaterniony: oś–kąt, euler, mnożenie, `slerp`, obrót wektorów |
//! | [`complex`] | liczby zespolone nad `f32` i `f64` |
//! | [`noise`] | `value noise`, `perlin`, `fbm`, `ridged`, `curl` |
//! | [`spline`] | splajny Catmull-Rom, Béziery i zestaw easings |
//! | [`stats`] | suma, średnia, wariancja, percentyle, histogram (odpowiedniki GPU) |

pub mod complex;
pub mod matrix;
pub mod noise;
pub mod quat;
pub mod scalar;
pub mod spline;
pub mod stats;
pub mod vector;

pub use complex::{Complex, ComplexScalar};
pub use matrix::{Mat2, Mat3, Mat4};
pub use quat::Quat;
pub use vector::{Vec2, Vec3, Vec4};