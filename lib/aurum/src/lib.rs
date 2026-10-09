//! **Aurum** — biblioteka matematyczna i obliczeniowa na GPU oparta na [`wgpu`].
//!
//! Dwa katalogi, jeden typ liczb:
//!
//! * [`math`] — czysta matematyka na CPU: wektory, macierze, kwaterniony,
//!   liczby zespolone, szum, splajny i statystyka. Bez GPU, testowalna w zwykłym
//!   `cargo test`.
//! * [`gpu`] i [`ops`] — te same liczby, ale liczone na GPU: kontekst
//!   (`Instance` → `Adapter` → `Device`), bufory, kernele WGSL oraz gotowe
//!   operacje — suma, `min`/`max`, `prefix sum`, mnożenie macierzy, splot
//!   Gaussa, histogram.
//!
//! # Dlaczego taki podział
//!
//! Koszt przesłania danych na GPU rośnie szybciej niż koszt samego obliczenia.
//! Dlatego biblioteka nie zasłania tego faktu: [`gpu`] daje pełną kontrolę nad
//! buforami i dispatchami, a [`ops`] to cienka warstwa gotowych recept, którą
//! można zignorować.
//!
//! * **zero `unsafe`** — `#![forbid(unsafe_code)]`, cała komunikacja z GPU
//!   idzie przez bezpieczne API `wgpu`;
//! * **cache pipeline’ów** — ten sam WGSL i ten sam layout bindingów nie tworzą
//!   drugiego pipeline’u;
//! * **walidacja WGSL na wejściu** — błąd w shaderze wraca jako [`Error::Shader`],
//!   a nie jako asynchroniczny `panic` `wgpu`;
//! * **własne typy** — wektory i macierze należą do biblioteki i są `Pod`,
//!   więc trafiają do buforów bez rzutowania.
//!
//! # Przykład — własny kernel
//!
//! ```no_run
//! use aurum::gpu::{Binding, BindingKind, Buffer, Context, Kernel, KernelDesc};
//! use aurum::math::Vec3;
//! use aurum::Result;
//!
//! const WGSL: &str = r#"
//! @group(0) @binding(0) var<storage, read>       src: array<vec3<f32>>;
//! @group(0) @binding(1) var<storage, read_write> dst: array<vec3<f32>>;
//!
//! @compute @workgroup_size(64)
//! fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
//!     let i = gid.x;
//!     if (i >= arrayLength(&src)) { return; }
//!     let v = src[i];
//!     dst[i] = v * 2.0 + vec3<f32>(0.0, 1.0, 0.0);
//! }
//! "#;
//!
//! fn main() -> Result<()> {
//!     let ctx = Context::new()?;
//!
//!     let points: Vec<Vec3> = (0..1000).map(|i| Vec3::new(i as f32, 0.0, 0.0)).collect();
//!     let src = Buffer::from_slice(&ctx, &points, "punkty")?;
//!     let dst: Buffer<Vec3> = Buffer::zeros(&ctx, points.len(), "przesunięte")?;
//!
//!     let kernel = Kernel::new(
//!         &ctx,
//!         KernelDesc {
//!             label: "podniesienie",
//!             source: WGSL,
//!             entry_point: "main",
//!             bindings: &[BindingKind::ReadOnlyStorage, BindingKind::Storage],
//!         },
//!     )?;
//!
//!     let group = kernel.bind_group(&ctx, &[Binding::read(&src), Binding::write(&dst)])?;
//!     ctx.submit(|encoder| kernel.dispatch(encoder, &group, (16, 1, 1)))?;
//!     ctx.poll()?;
//!
//!     let moved = dst.read(&ctx)?;
//!     assert_eq!(moved[0].y, 1.0);
//!     Ok(())
//! }
//! ```
//!
//! # Przykład — gotowa operacja
//!
//! ```no_run
//! use aurum::gpu::{Buffer, Context};
//! use aurum::{ops, Result};
//!
//! fn main() -> Result<()> {
//!     let ctx = Context::new()?;
//!
//!     let data: Vec<f32> = (0..100_000).map(|i| (i % 97) as f32).collect();
//!     let x = Buffer::from_slice(&ctx, &data, "x")?;
//!
//!     let sum = ops::reduce::sum(&ctx, &x)?;
//!     let prefix = ops::scan::prefix_sum(&ctx, &x)?;
//!
//!     println!("suma = {sum}, ostatnia suma prefiksowa = {}", prefix.last().copied().unwrap_or(0.0));
//!     Ok(())
//! }
//! ```
//!
//! # Warstwy
//!
//! | Moduł | Odpowiada za |
//! |---|---|
//! | [`math`] | wektory, macierze, kwaterniony, liczby zespolone, szum, splajny, statystyka |
//! | [`gpu`] | kontekst GPU, bufory, kernele WGSL i cache pipeline’ów |
//! | [`ops`] | gotowe obliczenia na GPU (map, reduce, scan, matmul, splot, histogram) |
//! | [`error`] | jeden typ błędu dla całej biblioteki |

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod gpu;
pub mod math;
pub mod ops;
pub mod prelude;

pub use error::{Error, Result};

/// Reeksport `bytemuck` — potrzebny do własnych typów w buforach i uniformach.
///
/// ```
/// use aurum::bytemuck::{Pod, Zeroable};
///
/// #[repr(C)]
/// #[derive(Clone, Copy, Pod, Zeroable)]
/// struct Czastka {
///     pozycja: [f32; 2],
///     predkosc: [f32; 2],
/// }
/// ```
pub use bytemuck;