//! Warstwa GPU: kontekst, bufory i kernele WGSL.
//!
//! To najniższy poziom biblioteki — cienka, przewidywalna nakładka na `wgpu`,
//! która dodaje trzy rzeczy: automatyczny wybór urządzenia, cache pipeline’ów
//! i błędy WGSL zwracane synchronicznie zamiast `panic` w tle.
//!
//! | Typ | Rola |
//! |---|---|
//! | [`Context`] | `Instance` → `Adapter` → `Device` + `Queue`, cache zasobów |
//! | [`Buffer`] | typowany bufor GPU (`STORAGE` + `COPY_SRC` + `MAP_READ`) |
//! | [`Kernel`] | pipeline compute’owy z layoutem bindingów i dispatchem |
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::Result;
//!
//! fn main() -> Result<()> {
//!     let ctx = Context::new()?;
//!     let bufor = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0], "liczby")?;
//!     assert_eq!(bufor.len(), 3);
//!     assert_eq!(bufor.read(&ctx)?, vec![1.0, 2.0, 3.0]);
//!     Ok(())
//! }
//! ```

pub mod buffer;
pub mod context;
pub mod kernel;

pub use buffer::Buffer;
pub use context::{Context, ContextBuilder};
pub use kernel::{Binding, BindingKind, Kernel, KernelDesc, groups_for};