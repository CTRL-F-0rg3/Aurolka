//! Wektory [`Vec2`], [`Vec3`] i [`Vec4`].
//!
//! Wszystkie trzy są `#[repr(C)]` i implementują `bytemuck::Pod`, więc można
//! je wypychać do [`Buffer`](crate::gpu::Buffer) i czytać w nich z shaderów
//! jako `vec2<f32>`, `vec3<f32>`, `vec4<f32>`. Kolejność składowych odpowiada
//! `wgpu::VertexFormat::Float32x4` i `vec4<f32>` w WGSL.
//!
//! ```
//! use aurum::math::Vec3;
//!
//! let a = Vec3::new(1.0, 0.0, 0.0);
//! let b = Vec3::new(0.0, 1.0, 0.0);
//! assert_eq!(a.cross(b), Vec3::new(0.0, 0.0, 1.0));
//! assert_eq!(a.dot(b), 0.0);
//! ```

use bytemuck::{Pod, Zeroable};
use std::ops::{Add, AddAssign, Div, Index, IndexMut, Mul, Neg, Sub, SubAssign};

/// Wektor dwuwymiarowy.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, Zeroable, Pod)]
pub struct Vec2 {
    /// Składowa X.
    pub x: f32,
    /// Składowa Y.
    pub y: f32,
}

/// Wektor trójwymiarowy.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, Zeroable, Pod)]
pub struct Vec3 {
    /// Składowa X.
    pub x: f32,
    /// Składowa Y.
    pub y: f32,
    /// Składowa Z.
    pub z: f32,
}

/// Wektor czterowymiarowy.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, Zeroable, Pod)]
pub struct Vec4 {
    /// Składowa X.
    pub x: f32,
    /// Składowa Y.
    pub y: f32,
    /// Składowa Z.
    pub z: f32,
    /// Składowa W.
    pub w: f32,
}

/// Wspólna część API wektorów — jedna definicja, trzy typy.
macro_rules! wektor {
    ($ty:ident { $($f:ident : $i:literal),+ $(,)? }) => {
        impl $ty {
            /// Zerowy wektor.
            pub const ZERO: Self = Self { $($f: 0.0),+ };

            /// Wektor o wszystkich składowych równych `v`.
            pub const fn splat(v: f32) -> Self {
                Self { $($f: v),+ }
            }

            /// Buduje wektor ze składowych.
            pub const fn new($($f: f32),+) -> Self {
                Self { $($f),+ }
            }

            /// Wektor z tablicy — kolejność składowych jak w pamięci.
            pub fn from_array(v: [f32; Self::DIM]) -> Self {
                Self { $($f: v[$i]),+ }
            }

            /// Wektor jako tablica — kolejność składowych jak w pamięci.
            pub fn to_array(self) -> [f32; Self::DIM] {
                [$(self.$f),+]
            }

            /// Iloczyn skalarny (dot product).
            pub fn dot(self, other: Self) -> f32 {
                0.0 $(+ self.$f * other.$f)+
            }

            /// Kwadrat długości — bez pierwiastka, więc taniej.
            pub fn length_squared(self) -> f32 {
                self.dot(self)
            }

            /// Długość wektora.
            pub fn length(self) -> f32 {
                self.length_squared().sqrt()
            }

            /// Odległość dwóch punktów.
            pub fn distance(self, other: Self) -> f32 {
                (other - self).length()
            }

            /// Wektor jednostkowy w tym samym kierunku.
            ///
            /// Wektor zerowy zostaje zerowy — dzielenie przez zero byłoby błędem,
            /// a w shaderach `normalize(vec3(0.0))` i tak daje `NaN`.
            pub fn normalize(self) -> Self {
                let len = self.length();
                if len == 0.0 {
                    Self::ZERO
                } else {
                    self / len
                }
            }
        }
    };
}

/// Operacje składowe i implementacje operatorów — też jedna definicja.
macro_rules! wektor_operacje {
    ($ty:ident { $($f:ident : $i:literal),+ $(,)? }) => {
        impl $ty {
            /// Minimum składowo.
            pub fn min(self, other: Self) -> Self {
                Self { $($f: self.$f.min(other.$f)),+ }
            }

            /// Maksimum składowo.
            pub fn max(self, other: Self) -> Self {
                Self { $($f: self.$f.max(other.$f)),+ }
            }

            /// Wartość bezwzględna każdej składowej.
            pub fn abs(self) -> Self {
                Self { $($f: self.$f.abs()),+ }
            }

            /// Znak każdej składowej.
            pub fn signum(self) -> Self {
                Self { $($f: self.$f.signum()),+ }
            }

            /// Ograniczenie każdej składowej do `[min, max]`.
            pub fn clamp(self, min: Self, max: Self) -> Self {
                self.max(min).min(max)
            }

            /// Interpolacja składowa.
            pub fn lerp(self, other: Self, t: f32) -> Self {
                Self { $($f: self.$f + (other.$f - self.$f) * t),+ }
            }

            /// Przekształcenie każdej składowej funkcją.
            pub fn map(self, mut f: impl FnMut(f32) -> f32) -> Self {
                Self { $($f: f(self.$f)),+ }
            }

            /// Połączenie składowo z drugim wektorem.
            pub fn zip(self, other: Self, mut f: impl FnMut(f32, f32) -> f32) -> Self {
                Self { $($f: f(self.$f, other.$f)),+ }
            }

            /// Mnożenie składowe (iloczyn Hadamarda).
            pub fn hadamard(self, other: Self) -> Self {
                self.zip(other, |a, b| a * b)
            }

            /// Czy wszystkie składowe są skończone.
            pub fn is_finite(self) -> bool {
                true $(&& self.$f.is_finite())+
            }
        }

        impl Index<usize> for $ty {
            type Output = f32;

            fn index(&self, index: usize) -> &f32 {
                match index {
                    $($i => &self.$f,)+
                    _ => panic!("indeks {index} poza zakresem {}", Self::DIM),
                }
            }
        }

        impl IndexMut<usize> for $ty {
            fn index_mut(&mut self, index: usize) -> &mut f32 {
                match index {
                    $($i => &mut self.$f,)+
                    _ => panic!("indeks {index} poza zakresem {}", Self::DIM),
                }
            }
        }

        impl From<[f32; $ty::DIM]> for $ty {
            fn from(value: [f32; $ty::DIM]) -> Self {
                Self::from_array(value)
            }
        }

        impl From<$ty> for [f32; $ty::DIM] {
            fn from(value: $ty) -> Self {
                value.to_array()
            }
        }

        impl Add for $ty {
            type Output = Self;

            fn add(self, other: Self) -> Self {
                Self { $($f: self.$f + other.$f),+ }
            }
        }

        impl AddAssign for $ty {
            fn add_assign(&mut self, other: Self) {
                *self = *self + other;
            }
        }

        impl Sub for $ty {
            type Output = Self;

            fn sub(self, other: Self) -> Self {
                Self { $($f: self.$f - other.$f),+ }
            }
        }

        impl SubAssign for $ty {
            fn sub_assign(&mut self, other: Self) {
                *self = *self - other;
            }
        }

        impl Mul<f32> for $ty {
            type Output = Self;

            fn mul(self, rhs: f32) -> Self {
                Self { $($f: self.$f * rhs),+ }
            }
        }

        impl Mul<$ty> for f32 {
            type Output = $ty;

            fn mul(self, rhs: $ty) -> $ty {
                rhs * self
            }
        }

        impl Mul for $ty {
            type Output = Self;

            fn mul(self, rhs: Self) -> Self {
                self.hadamard(rhs)
            }
        }

        impl Div<f32> for $ty {
            type Output = Self;

            fn div(self, rhs: f32) -> Self {
                Self { $($f: self.$f / rhs),+ }
            }
        }

        impl Neg for $ty {
            type Output = Self;

            fn neg(self) -> Self {
                Self { $($f: -self.$f),+ }
            }
        }

        impl std::iter::Sum for $ty {
            fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
                iter.fold(Self::ZERO, |acc, v| acc + v)
            }
        }
    };
}

wektor!(Vec2 { x: 0, y: 1 });
wektor!(Vec3 { x: 0, y: 1, z: 2 });
wektor!(Vec4 { x: 0, y: 1, z: 2, w: 3 });

wektor_operacje!(Vec2 { x: 0, y: 1 });
wektor_operacje!(Vec3 { x: 0, y: 1, z: 2 });
wektor_operacje!(Vec4 { x: 0, y: 1, z: 2, w: 3 });
impl Vec2 {
    /// Liczba składowych.
    pub const DIM: usize = 2;

    /// Wektor osi X (`1, 0`).
    pub const X: Self = Self::new(1.0, 0.0);

    /// Wektor osi Y (`0, 1`).
    pub const Y: Self = Self::new(0.0, 1.0);

    /// Wektor kierunku o zadanym kącie (w radianach, od osi X).
    pub fn from_angle(radians: f32) -> Self {
        Self::new(radians.cos(), radians.sin())
    }

    /// Kąt wektora względem osi X.
    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    /// Obrót o zadany kąt (w radianach).
    pub fn rotate(self, radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }

    /// Wektor prostopadły (obrót o 90°).
    pub fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// Iloczyn 2D (skalar `z` z krzyżówki rozszerzonej o zero).
    pub fn cross(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    /// Odbicie kierunku względem normalnej (normalna musi być jednostkowa).
    pub fn reflect(self, normal: Self) -> Self {
        self - normal * (2.0 * self.dot(normal))
    }
}

impl Vec3 {
    /// Liczba składowych.
    pub const DIM: usize = 3;

    /// Wektor osi X (`1, 0, 0`).
    pub const X: Self = Self::new(1.0, 0.0, 0.0);

    /// Wektor osi Y (`0, 1, 0`).
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);

    /// Wektor osi Z (`0, 0, 1`).
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// Wektor w górę (`+Y`).
    pub const UP: Self = Self::Y;

    /// Wektor w prawo (`+X`).
    pub const RIGHT: Self = Self::X;

    /// Wektor do przodu (`-Z`, konwencja kamery).
    pub const FORWARD: Self = Self::new(0.0, 0.0, -1.0);

    /// Iloczyn wektorowy (cross product).
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Odbicie kierunku względem normalnej (normalna musi być jednostkowa).
    pub fn reflect(self, normal: Self) -> Self {
        self - normal * (2.0 * self.dot(normal))
    }

    /// Kąt między wektorami w radianach.
    pub fn angle_to(self, other: Self) -> f32 {
        let denominator = self.length() * other.length();
        if denominator == 0.0 {
            return 0.0;
        }
        (self.dot(other) / denominator).clamp(-1.0, 1.0).acos()
    }

    /// Składowe `x`, `y` bez `z`.
    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

impl Vec4 {
    /// Liczba składowych.
    pub const DIM: usize = 4;

    /// Trzy pierwsze składowe.
    pub fn truncate(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    /// Podmienia składową `w`.
    pub fn with_w(self, w: f32) -> Self {
        Self::new(self.x, self.y, self.z, w)
    }
}

impl From<Vec3> for Vec4 {
    fn from(value: Vec3) -> Self {
        Self::new(value.x, value.y, value.z, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iloczyny_skalarne() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);
        assert_eq!(a.dot(b), 32.0);
        assert_eq!(a.cross(b), Vec3::new(-3.0, 6.0, -3.0));
        assert_eq!(a.length(), 14f32.sqrt());
    }

    #[test]
    fn normalizacja_zera_nie_wybucha() {
        assert_eq!(Vec2::ZERO.normalize(), Vec2::ZERO);
        assert!((Vec2::new(3.0, 4.0).normalize().length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn min_max_i_mapa() {
        let a = Vec2::new(1.0, 5.0);
        let b = Vec2::new(4.0, 2.0);
        assert_eq!(a.min(b), Vec2::new(1.0, 2.0));
        assert_eq!(a.max(b), Vec2::new(4.0, 5.0));
        assert_eq!(a.map(f32::sqrt), Vec2::new(1.0, 5f32.sqrt()));
        assert!(a.is_finite());
    }

    #[test]
    fn obrot_w_wektorze_2d() {
        let v = Vec2::from_angle(0.0).rotate(std::f32::consts::FRAC_PI_2);
        assert!((v.y - 1.0).abs() < 1e-6);
        assert!(v.x.abs() < 1e-6);
    }

    #[test]
    fn indeksowanie_i_operatory() {
        let mut v = Vec3::new(1.0, 2.0, 3.0);
        v[1] = 7.0;
        assert_eq!(v[1], 7.0);
        assert_eq!(v.to_array(), [1.0, 7.0, 3.0]);
        assert_eq!(v * 2.0, Vec3::new(2.0, 14.0, 6.0));
        assert_eq!(2.0 * v, Vec3::new(2.0, 14.0, 6.0));
        assert_eq!(-v, Vec3::new(-1.0, -7.0, -3.0));
        assert_eq!(v / 2.0, Vec3::new(0.5, 3.5, 1.5));
        assert_eq!(v + v, Vec3::new(2.0, 14.0, 6.0));
        assert_eq!(v.hadamard(v), v * v);
    }

    #[test]
    fn suma_z_iteratora() {
        let suma: Vec3 = [Vec3::splat(1.0), Vec3::splat(2.0)].into_iter().sum();
        assert_eq!(suma, Vec3::splat(3.0));
    }

    #[test]
    fn uklad_pamieci_zgodny_z_gpu() {
        assert_eq!(std::mem::size_of::<Vec2>(), 8);
        assert_eq!(std::mem::size_of::<Vec3>(), 12);
        assert_eq!(std::mem::size_of::<Vec4>(), 16);
        assert_eq!(
            Vec4::from([1.0, 2.0, 3.0, 4.0]).to_array(),
            [1.0, 2.0, 3.0, 4.0]
        );
    }
}