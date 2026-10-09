//! Macierze [`Mat2`], [`Mat3`] i [`Mat4`].
//!
//! Wszystkie są zapisane **wierszowo**: `m[rows * cols + col]`, czyli
//! `m[row()][col()]` w zapisie matematycznym. To nie jest układ
//! kolumnowo-wierszowy, którego oczekują niektóre shadery — jeśli wysyłasz
//! macierz do `mat4x4<f32>` w WGSL, użyj [`Mat4::to_cols_array`], bo WGSL
//! liczy macierze kolumnowo.
//!
//! ```
//! use aurum::math::{Mat3, Vec3};
//!
//! let m = Mat3::from_cols(Vec3::X, Vec3::Y, Vec3::Z);
//! assert_eq!(m.mul_vec3(Vec3::new(1.0, 2.0, 3.0)), Vec3::new(1.0, 2.0, 3.0));
//! ```

use crate::math::{Vec2, Vec3, Vec4};
use bytemuck::{Pod, Zeroable};
use std::ops::{Add, Mul, Sub};

macro_rules! macierz {
    ($ty:ident, $dim:literal) => {
        impl $ty {
            /// Liczba wierszy (i kolumn) macierzy.
            pub const DIM: usize = $dim;

            /// Liczba składowych.
            pub const LEN: usize = $dim * $dim;

            /// Macierz jednostkowa.
            pub const IDENTITY: Self = Self {
                m: {
                    let mut m = [0.0; $dim * $dim];
                    let mut i = 0;
                    while i < $dim {
                        m[i * $dim + i] = 1.0;
                        i += 1;
                    }
                    m
                },
            };

            /// Macierz zerowa.
            pub const ZERO: Self = Self { m: [0.0; $dim * $dim] };

            /// Buduje macierz z tablicy wierszowo (wiersz po wierszu).
            pub const fn from_rows(m: [f32; $dim * $dim]) -> Self {
                Self { m }
            }

            /// Zawartość macierzy jako tablica wierszowo.
            pub const fn to_rows(self) -> [f32; $dim * $dim] {
                self.m
            }

            /// Element `[wiersz, kolumn]`.
            pub fn get(&self, row: usize, col: usize) -> f32 {
                self.m[row * $dim + col]
            }

            /// Ustawia element `[wiersz, kolumn]`.
            pub fn set(&mut self, row: usize, col: usize, value: f32) {
                self.m[row * $dim + col] = value;
            }

            /// Wiersz jako tablica.
            pub fn row(&self, row: usize) -> [f32; $dim] {
                let mut out = [0.0; $dim];
                out.copy_from_slice(&self.m[row * $dim..row * $dim + $dim]);
                out
            }

            /// Kolumna jako tablica.
            pub fn col(&self, col: usize) -> [f32; $dim] {
                let mut out = [0.0; $dim];
                for (row, slot) in out.iter_mut().enumerate() {
                    *slot = self.m[row * $dim + col];
                }
                out
            }

            /// Transpozycja.
            pub fn transpose(self) -> Self {
                let mut out = Self::ZERO;
                for row in 0..$dim {
                    for col in 0..$dim {
                        out.m[row * $dim + col] = self.m[col * $dim + row];
                    }
                }
                out
            }

            /// Ślad macierzy.
            pub fn trace(self) -> f32 {
                let mut sum = 0.0;
                let mut i = 0;
                while i < $dim {
                    sum += self.m[i * $dim + i];
                    i += 1;
                }
                sum
            }

            /// Iloczyn macierzy.
            pub fn mul_mat(self, rhs: Self) -> Self {
                let mut out = Self::ZERO;
                for row in 0..$dim {
                    for col in 0..$dim {
                        let mut sum = 0.0;
                        for k in 0..$dim {
                            sum += self.m[row * $dim + k] * rhs.m[k * $dim + col];
                        }
                        out.m[row * $dim + col] = sum;
                    }
                }
                out
            }

            /// Iloczyn macierzy i wektora kolumnowego (tablica wierszowo).
            pub fn mul_slice(self, rhs: &[f32]) -> Vec<f32> {
                let mut out = vec![0.0; $dim];
                for row in 0..$dim {
                    let mut sum = 0.0;
                    for k in 0..$dim {
                        sum += self.m[row * $dim + k] * rhs[k];
                    }
                    out[row] = sum;
                }
                out
            }
        }

        impl Mul for $ty {
            type Output = Self;

            fn mul(self, rhs: Self) -> Self {
                self.mul_mat(rhs)
            }
        }

        impl Add for $ty {
            type Output = Self;

            fn add(self, rhs: Self) -> Self {
                let mut m = self.m;
                for (i, value) in rhs.m.iter().enumerate() {
                    m[i] += value;
                }
                Self { m }
            }
        }

        impl Sub for $ty {
            type Output = Self;

            fn sub(self, rhs: Self) -> Self {
                let mut m = self.m;
                for (i, value) in rhs.m.iter().enumerate() {
                    m[i] -= value;
                }
                Self { m }
            }
        }

        impl Mul<f32> for $ty {
            type Output = Self;

            fn mul(self, rhs: f32) -> Self {
                let mut m = self.m;
                for value in m.iter_mut() {
                    *value *= rhs;
                }
                Self { m }
            }
        }

        impl From<[f32; $dim * $dim]> for $ty {
            fn from(m: [f32; $dim * $dim]) -> Self {
                Self { m }
            }
        }
    };
}

/// Macierz 2×2.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Zeroable, Pod)]
pub struct Mat2 {
    /// Składowe wierszowo.
    pub m: [f32; 4],
}

/// Macierz 3×3.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Zeroable, Pod)]
pub struct Mat3 {
    /// Składowe wierszowo.
    pub m: [f32; 9],
}

/// Macierz 4×4.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Zeroable, Pod)]
pub struct Mat4 {
    /// Składowe wierszowo.
    pub m: [f32; 16],
}

macierz!(Mat2, 2);
macierz!(Mat3, 3);
macierz!(Mat4, 4);

impl Mat2 {
    /// Macierz z dwóch kolumn.
    pub const fn from_cols(c0: Vec2, c1: Vec2) -> Self {
        Self {
            m: [c0.x, c1.x, c0.y, c1.y],
        }
    }

    /// Zwycięstwo wyznacznika.
    pub fn determinant(self) -> f32 {
        self.m[0] * self.m[3] - self.m[1] * self.m[2]
    }

    /// Macierz odwrotna albo `None`, gdy wyznacznik jest zerowy.
    pub fn inverse(self) -> Option<Self> {
        let det = self.determinant();
        if det.abs() < f32::EPSILON {
            return None;
        }
        let inv = 1.0 / det;
        Some(Self {
            m: [
                self.m[3] * inv,
                -self.m[1] * inv,
                -self.m[2] * inv,
                self.m[0] * inv,
            ],
        })
    }

    /// Obrót o kąt (w radianach).
    pub fn rotation(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self {
            m: [cos, -sin, sin, cos],
        }
    }

    /// Skalowanie każdej osi.
    pub fn scale(sx: f32, sy: f32) -> Self {
        Self {
            m: [sx, 0.0, 0.0, sy],
        }
    }

    /// Iloczyn z wektorem.
    pub fn mul_vec2(self, v: Vec2) -> Vec2 {
        Vec2::new(
            self.m[0] * v.x + self.m[1] * v.y,
            self.m[2] * v.x + self.m[3] * v.y,
        )
    }
}

impl Mat3 {
    /// Macierz z trzech kolumn.
    pub const fn from_cols(c0: Vec3, c1: Vec3, c2: Vec3) -> Self {
        Self {
            m: [
                c0.x, c1.x, c2.x, c0.y, c1.y, c2.y, c0.z, c1.z, c2.z,
            ],
        }
    }

    /// Trzy pierwsze kolumny jako wektory.
    pub fn cols(self) -> [Vec3; 3] {
        [
            Vec3::new(self.m[0], self.m[3], self.m[6]),
            Vec3::new(self.m[1], self.m[4], self.m[7]),
            Vec3::new(self.m[2], self.m[5], self.m[8]),
        ]
    }

    /// Wyznacznik.
    pub fn determinant(self) -> f32 {
        let m = &self.m;
        m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6])
    }

    /// Macierz odwrotna albo `None`, gdy macierz jest osobliwa.
    pub fn inverse(self) -> Option<Self> {
        let det = self.determinant();
        if det.abs() < f32::EPSILON {
            return None;
        }
        let m = self.m;
        let inv = 1.0 / det;
        let mut out = [0.0; 9];
        out[0] = (m[4] * m[8] - m[5] * m[7]) * inv;
        out[1] = (m[2] * m[7] - m[1] * m[8]) * inv;
        out[2] = (m[1] * m[5] - m[2] * m[4]) * inv;
        out[3] = (m[5] * m[6] - m[3] * m[8]) * inv;
        out[4] = (m[0] * m[8] - m[2] * m[6]) * inv;
        out[5] = (m[2] * m[3] - m[0] * m[5]) * inv;
        out[6] = (m[3] * m[7] - m[4] * m[6]) * inv;
        out[7] = (m[1] * m[6] - m[0] * m[7]) * inv;
        out[8] = (m[0] * m[4] - m[1] * m[3]) * inv;
        Some(Self { m: out })
    }

    /// Iloczyn z wektorem.
    pub fn mul_vec3(self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.m[0] * v.x + self.m[1] * v.y + self.m[2] * v.z,
            self.m[3] * v.x + self.m[4] * v.y + self.m[5] * v.z,
            self.m[6] * v.x + self.m[7] * v.y + self.m[8] * v.z,
        )
    }

    /// Skalowanie każdej osi.
    pub fn scale(s: Vec3) -> Self {
        Self {
            m: [s.x, 0.0, 0.0, 0.0, s.y, 0.0, 0.0, 0.0, s.z],
        }
    }
}

impl Mat4 {
    /// Macierz z czterech kolumn.
    pub const fn from_cols(c0: Vec4, c1: Vec4, c2: Vec4, c3: Vec4) -> Self {
        Self {
            m: [
                c0.x, c1.x, c2.x, c3.x, c0.y, c1.y, c2.y, c3.y, c0.z, c1.z, c2.z, c3.z, c0.w,
                c1.w, c2.w, c3.w,
            ],
        }
    }

    /// Cztery kolumny jako wektory.
    pub fn cols(self) -> [Vec4; 4] {
        [
            Vec4::new(self.m[0], self.m[4], self.m[8], self.m[12]),
            Vec4::new(self.m[1], self.m[5], self.m[9], self.m[13]),
            Vec4::new(self.m[2], self.m[6], self.m[10], self.m[14]),
            Vec4::new(self.m[3], self.m[7], self.m[11], self.m[15]),
        ]
    }

    /// Układ kolumnowo-wierszowy, jakiego oczekuje WGSL (`mat4x4<f32>`).
    pub fn to_cols_array(self) -> [[f32; 4]; 4] {
        [self.col(0), self.col(1), self.col(2), self.col(3)]
    }

    /// Buduje macierz z układu kolumnowo-wierszowego (jak po `transpose` w WGSL).
    pub fn from_cols_array(cols: [[f32; 4]; 4]) -> Self {
        Self::from_cols(
            Vec4::from(cols[0]),
            Vec4::from(cols[1]),
            Vec4::from(cols[2]),
            Vec4::from(cols[3]),
        )
    }

    /// Iloczyn z wektorem homogenicznym (`w` zachowane).
    pub fn mul_vec4(self, v: Vec4) -> Vec4 {
        Vec4::new(
            self.m[0] * v.x + self.m[1] * v.y + self.m[2] * v.z + self.m[3] * v.w,
            self.m[4] * v.x + self.m[5] * v.y + self.m[6] * v.z + self.m[7] * v.w,
            self.m[8] * v.x + self.m[9] * v.y + self.m[10] * v.z + self.m[11] * v.w,
            self.m[12] * v.x + self.m[13] * v.y + self.m[14] * v.z + self.m[15] * v.w,
        )
    }

    /// Iloczyn z punktem (`w = 1`) — tłumaczenie, obrót, skala.
    pub fn mul_point(self, v: Vec3) -> Vec3 {
        self.mul_vec4(Vec4::new(v.x, v.y, v.z, 1.0)).truncate()
    }

    /// Iloczyn z kierunkiem (`w = 0`) — bez tłumaczenia.
    pub fn mul_direction(self, v: Vec3) -> Vec3 {
        self.mul_vec4(Vec4::new(v.x, v.y, v.z, 0.0)).truncate()
    }

    /// Wyznacznik (permutacje Laplace’a).
    pub fn determinant(self) -> f32 {
        let m = &self.m;
        let s0 = m[0] * m[5] - m[1] * m[4];
        let s1 = m[0] * m[6] - m[2] * m[4];
        let s2 = m[0] * m[7] - m[3] * m[4];
        let s3 = m[1] * m[6] - m[2] * m[5];
        let s4 = m[1] * m[7] - m[3] * m[5];
        let s5 = m[2] * m[7] - m[3] * m[6];

        let c5 = m[10] * m[15] - m[11] * m[14];
        let c4 = m[9] * m[15] - m[11] * m[13];
        let c3 = m[9] * m[14] - m[10] * m[13];
        let c2 = m[8] * m[15] - m[11] * m[12];
        let c1 = m[8] * m[14] - m[10] * m[12];
        let c0 = m[8] * m[13] - m[9] * m[12];

        s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0
    }

    /// Macierz odwrotna albo `None`, gdy macierz jest osobliwa.
    ///
    /// Odwrócenie przez eliminację Gaussa–Jordana z wyborem elementu
    /// dominującego — działa też dla macierzy „złamanych”, np. po `T · S`.
    pub fn inverse(self) -> Option<Self> {
        const N: usize = 4;

        // Układ rozszerzony [A | I]: N wierszy po 2N elementów.
        let mut a = [[0.0f32; N * 2]; N];
        for (row, chunk) in a.iter_mut().enumerate() {
            chunk[..N].copy_from_slice(&self.m[row * N..row * N + N]);
            chunk[N + row] = 1.0;
        }

        for col in 0..N {
            // Element dominujący w kolumnie — bez tego wynik bywa bardzo niedokładny.
            let pivot = (col..N)
                .max_by(|&r1, &r2| a[r1][col].abs().total_cmp(&a[r2][col].abs()))
                .unwrap();
            if a[pivot][col].abs() < f32::EPSILON {
                return None;
            }
            a.swap(col, pivot);

            let div = a[col][col];
            for value in a[col].iter_mut() {
                *value /= div;
            }

            for row in 0..N {
                if row == col {
                    continue;
                }
                let factor = a[row][col];
                if factor == 0.0 {
                    continue;
                }
                // Kopiujemy wiersz pivotu: `a[row]` pożyczamy mutowalnie, a
                // wiersz pivotu nie może być jednocześnie pożyczony na
                // statycznie (oba mogą wskazywać ten sam indeks).
                let pivot_row = a[col];
                for (k, cel) in a[row].iter_mut().enumerate() {
                    *cel -= factor * pivot_row[k];
                }
            }
        }

        // Po eliminacji [A | I] zamienia się w [I | A⁻¹], więc odwrotność
        // siedzi w PRAWEJ połowie.
        let mut out = [0.0; 16];
        for (row, chunk) in a.iter().enumerate() {
            out[row * N..row * N + N].copy_from_slice(&chunk[N..]);
        }
        Some(Self { m: out })
    }

    /// Przesunięcie.
    ///
    /// Tłumaczenie siedzi w **ostatniej kolumnie** (indeksy 3, 7, 11), bo
    /// wektory traktujemy jako kolumnowe — dokładnie tak, jak `mat4x4<f32>`
    /// w WGSL. Dzięki temu `to_cols_array` nadaje się prosto do shadera.
    pub fn translation(offset: Vec3) -> Self {
        let mut m = Self::IDENTITY;
        m.m[3] = offset.x;
        m.m[7] = offset.y;
        m.m[11] = offset.z;
        m
    }

    /// Skalowanie każdej osi.
    pub fn scale(s: Vec3) -> Self {
        let mut m = Self::IDENTITY;
        m.m[0] = s.x;
        m.m[5] = s.y;
        m.m[10] = s.z;
        m
    }

    /// Obrót wokół osi X.
    pub fn rotation_x(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        let mut m = Self::IDENTITY;
        m.m[5] = cos;
        m.m[6] = sin;
        m.m[9] = -sin;
        m.m[10] = cos;
        m
    }

    /// Obrót wokół osi Y.
    pub fn rotation_y(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        let mut m = Self::IDENTITY;
        m.m[0] = cos;
        m.m[2] = -sin;
        m.m[8] = sin;
        m.m[10] = cos;
        m
    }

    /// Obrót wokół osi Z.
    pub fn rotation_z(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        let mut m = Self::IDENTITY;
        m.m[0] = cos;
        m.m[1] = sin;
        m.m[4] = -sin;
        m.m[5] = cos;
        m
    }

    /// Macierz rzutowania perspektywicznego (zasięg `[0, 1]`, jak w WebGPU).
    ///
    /// Wektory traktowane są jako kolumnowe, więc ostatni **wiersz** daje `w = −z`,
    /// a przedostatni wiersz mapuje głębię na `[0, 1]`.
    pub fn perspective(fov_y_radians: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1.0 / (fov_y_radians * 0.5).tan();
        let mut m = Self::ZERO;
        m.m[0] = f / aspect;
        m.m[5] = f;
        m.m[10] = far / (near - far);
        m.m[11] = (far * near) / (near - far);
        m.m[14] = -1.0;
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::scalar::is_close;

    #[test]
    fn tozsamosc_mnozy_sie_ze_soba() {
        // Tożsamość ma wyznacznik 1 i neutralność mnożenia.
        assert_eq!(Mat2::IDENTITY * Mat2::IDENTITY, Mat2::IDENTITY);
        assert_eq!(Mat3::IDENTITY * Mat3::IDENTITY, Mat3::IDENTITY);
        assert_eq!(Mat4::IDENTITY * Mat4::IDENTITY, Mat4::IDENTITY);
        assert_eq!(Mat2::IDENTITY.determinant(), 1.0);
        assert_eq!(Mat3::IDENTITY.determinant(), 1.0);
        assert_eq!(Mat4::IDENTITY.determinant(), 1.0);
        assert_eq!(Mat3::IDENTITY.trace(), 3.0);
        assert_eq!(Mat4::IDENTITY.trace(), 4.0);
    }

    #[test]
    fn transpozycja_odwraca_mnozenie() {
        let a = Mat3::from_rows([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0]);
        let b = Mat3::from_rows([2.0, 0.0, 1.0, 1.0, 3.0, 0.0, 4.0, 1.0, 5.0]);
        assert_eq!((a * b).transpose(), b.transpose() * a.transpose());
        assert_eq!(a.transpose().transpose(), a);
    }

    #[test]
    fn odwrotnica_mat3_dziala() {
        let m = Mat3::from_rows([2.0, 0.0, 1.0, 1.0, 3.0, 2.0, 0.0, 1.0, 4.0]);
        let inv = m.inverse().expect("macierz odwracalna");
        let iloczyn = m * inv;
        for i in 0..3 {
            for j in 0..3 {
                let expect = if i == j { 1.0 } else { 0.0 };
                assert!(is_close(iloczyn.get(i, j), expect, 1e-5), "{i},{j} = {}", iloczyn.get(i, j));
            }
        }
        assert!(Mat3::from_rows([1.0, 2.0, 3.0, 2.0, 4.0, 6.0, 1.0, 1.0, 1.0]).inverse().is_none());
    }

    #[test]
    fn odwrotnica_mat4_dziala_na_lamaniu() {
        let m = Mat4::translation(Vec3::new(3.0, -1.0, 2.0))
            * Mat4::rotation_y(std::f32::consts::FRAC_PI_3)
            * Mat4::scale(Vec3::splat(2.0));
        let inv = m.inverse().expect("macierz odwracalna");
        let v = Vec3::new(1.0, 2.0, 3.0);
        let w = inv.mul_point(m.mul_point(v));
        assert!(is_close(w.x, v.x, 1e-4) && is_close(w.y, v.y, 1e-4) && is_close(w.z, v.z, 1e-4));
        assert!(Mat4::from_rows([0.0; 16]).inverse().is_none());
    }

    #[test]
    fn kolumny_dla_wgsl() {
        let m = Mat4::translation(Vec3::new(1.0, 2.0, 3.0));
        let cols = m.to_cols_array();
        // WGSL liczy kolumnowo, więc ostatnia kolumna trzyma przesunięcie.
        assert_eq!(cols[3], [1.0, 2.0, 3.0, 1.0]);
        assert_eq!(Mat4::from_cols_array(cols), m);
    }

    #[test]
    fn perspektywa_daje_punkt_przed_kamera() {
        let near = 0.1;
        let far = 100.0;
        let p = Mat4::perspective(std::f32::consts::FRAC_PI_4, 1.0, near, far);

        // Punkt w odległości `near` daje `w = near`, czyli dzielenie przez `w`
        // mapuje go na zasięg [0, 1] (konwencja WebGPU).
        let clip = p.mul_vec4(Vec4::new(0.0, 0.0, -near, 1.0));
        assert!(is_close(clip.w, near, 1e-4), "w = {}", clip.w);
        let depth = clip.z / clip.w;
        assert!((0.0..=1.0).contains(&depth), "z = {depth}");

        // Punkt dalej niż `far` wychodzi poza zasięg.
        let daleko = p.mul_vec4(Vec4::new(0.0, 0.0, -far, 1.0));
        assert!(daleko.z / daleko.w >= 1.0);
    }
}