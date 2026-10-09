//! Wygodne importy — `use aurum::prelude::*;` zamiast listy modułów.
//!
//! ```
//! use aurum::prelude::*;
//!
//! let v = Vec3::new(1.0, 2.0, 3.0).normalize();
//! assert!((v.length() - 1.0).abs() < 1e-6);
//! ```

pub use crate::error::{Error, Result};

pub use crate::gpu::{
    groups_for, Binding, BindingKind, Buffer, Context, ContextBuilder, Kernel, KernelDesc,
};

pub use crate::math::{Complex, Mat2, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

pub use crate::math::scalar::{clamp, is_close, lerp, remap, smoothstep};
pub use crate::math::stats::{argmax, mean, median, percentile, std_dev};

pub use crate::ops;