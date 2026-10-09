//! Kwaterniony [`Quat`] — zapis obrotu bez gimbal locka.
//!
//! Kwaternion `(x, y, z, w)` reprezentuje obrót tak samo, jak macierz 3×3,
//! ale interpolacja (`slerp`) jest na nim tania i gładka — dlatego animacje
//! liczy się na kwaternionach, a do shadera i tak wysyła macierz.
//!
//! ```
//! use aurum::math::{Quat, Vec3};
//!
//! let obrot = Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2);
//! let v = obrot.rotate_vec3(Vec3::X);
//! assert!((v.y - 1.0).abs() < 1e-5);
//! ```

use crate::math::{Mat3, Vec3};
use bytemuck::{Pod, Zeroable};
use std::ops::{Add, Mul, Neg, Sub};

/// Kwaternion opisujący obrót.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Zeroable, Pod)]
pub struct Quat {
    /// Składowa X.
    pub x: f32,
    /// Składowa Y.
    pub y: f32,
    /// Składowa Z.
    pub z: f32,
    /// Składowa W.
    pub w: f32,
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Quat {
    /// Kwaternion tożsamości (brak obrotu).
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Kwaternion z czterech składowych.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Obrót wokół osi o kąt (w radianach).
    ///
    /// Oś jest normalizowana wewnętrtrz, więc można podać dowolny niezerowy
    /// wektor kierunku.
    pub fn from_axis_angle(axis: Vec3, radians: f32) -> Self {
        let axis = axis.normalize();
        let (sin, cos) = (radians * 0.5).sin_cos();
        Self {
            x: axis.x * sin,
            y: axis.y * sin,
            z: axis.z * sin,
            w: cos,
        }
    }

    /// Obrót z trzech kątów Eulera w radianach: pitch (oś X), yaw (oś Y), roll (oś Z).
    ///
    /// Kolejność złożenia to `Z * Y * X`, czyli najpierw pitch, potem yaw,
    /// na końcu roll.
    pub fn from_euler(pitch: f32, yaw: f32, roll: f32) -> Self {
        let (sp, cp) = (pitch * 0.5).sin_cos();
        let (sy, cy) = (yaw * 0.5).sin_cos();
        let (sr, cr) = (roll * 0.5).sin_cos();
        Self {
            x: cp * cy * sr + sp * sy * cr,
            y: cp * sy * cr + sp * cy * sr,
            z: sp * cy * cr - cp * sy * sr,
            w: cp * cy * cr - sp * sy * sr,
        }
    }

    /// Kwaternion ze składowej tablicy.
    pub fn from_array(v: [f32; 4]) -> Self {
        Self::new(v[0], v[1], v[2], v[3])
    }

    /// Kwaternion jako tablica składowych.
    pub fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }

    /// Kwadrat długości.
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// Długość.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Iloczyn skalarny.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Kwaternion jednostkowy (zerowy zostaje zerowy).
    pub fn normalize(self) -> Self {
        let len = self.length();
        if len == 0.0 {
            Self::IDENTITY
        } else {
            self * (1.0 / len)
        }
    }

    /// Sprzężenie (odwrotność kwaternionu jednostkowego).
    pub fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Odwrotność — sprzężenie podzielone przez kwadrat długości.
    pub fn inverse(self) -> Option<Self> {
        let n = self.length_squared();
        if n < f32::EPSILON {
            return None;
        }
        Some(Self::new(-self.x / n, -self.y / n, -self.z / n, self.w / n))
    }

    /// Obraca wektor (zakłada kwaternion jednostkowy).
    pub fn rotate_vec3(self, v: Vec3) -> Vec3 {
        // v + 2 * (q_xyz × (q_xyz × v + w * v)) — bez tworzenia macierzy.
        let u = Vec3::new(self.x, self.y, self.z);
        let t = u.cross(v) * 2.0;
        v + t * self.w + u.cross(t)
    }

    /// Macierz obrotu 3×3 (wierszowo).
    pub fn to_mat3(self) -> Mat3 {
        let q = self.normalize();
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        Mat3::from_rows([
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ])
    }

    /// Kwaternion z macierzy obrotu (część liniowa, bez skali).
    ///
    /// Zakłada macierz czystego obrotu; przy skali wynik bywa znormalizowany
    /// tylko częściowo.
    pub fn from_mat3(m: Mat3) -> Self {
        let trace = m.trace();
        let q = if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            Self::new(
                (m.get(2, 1) - m.get(1, 2)) / s,
                (m.get(0, 2) - m.get(2, 0)) / s,
                (m.get(1, 0) - m.get(0, 1)) / s,
                0.25 * s,
            )
        } else if m.get(0, 0) > m.get(1, 1) && m.get(0, 0) > m.get(2, 2) {
            let s = (1.0 + m.get(0, 0) - m.get(1, 1) - m.get(2, 2)).sqrt() * 2.0;
            Self::new(
                0.25 * s,
                (m.get(0, 1) + m.get(1, 0)) / s,
                (m.get(0, 2) + m.get(2, 0)) / s,
                (m.get(2, 1) - m.get(1, 2)) / s,
            )
        } else if m.get(1, 1) > m.get(2, 2) {
            let s = (1.0 + m.get(1, 1) - m.get(0, 0) - m.get(2, 2)).sqrt() * 2.0;
            Self::new(
                (m.get(0, 1) + m.get(1, 0)) / s,
                0.25 * s,
                (m.get(1, 2) + m.get(2, 1)) / s,
                (m.get(0, 2) - m.get(2, 0)) / s,
            )
        } else {
            let s = (1.0 + m.get(2, 2) - m.get(0, 0) - m.get(1, 1)).sqrt() * 2.0;
            Self::new(
                (m.get(0, 2) + m.get(2, 0)) / s,
                (m.get(1, 2) + m.get(2, 1)) / s,
                0.25 * s,
                (m.get(1, 0) - m.get(0, 1)) / s,
            )
        };
        q.normalize()
    }

    /// Kąt obrotu w radianach (niezależny od osi).
    pub fn angle(self) -> f32 {
        let q = self.normalize();
        2.0 * q.w.abs().clamp(0.0, 1.0).acos()
    }

    /// Liniowa interpolacja składowa (krótka droga — wynik trzeba normalizować).
    pub fn lerp(self, other: Self, t: f32) -> Self {
        (self * (1.0 - t) + other * t).normalize()
    }

    /// Interpolacja sferyczna — jedyna poprawna dla kwaternionów.
    pub fn slerp(self, other: Self, t: f32) -> Self {
        let a = self.normalize();
        let mut b = other.normalize();

        // Krótka droga: przeciwny kwaternion opisuje ten sam obrót.
        let mut cos = a.dot(b);
        if cos < 0.0 {
            b = -b;
            cos = -cos;
        }

        if cos > 0.9995 {
            // Kąt zbyt mały, żeby liczyć sin — wystarczy lerp.
            return a.lerp(b, t);
        }

        let theta = cos.clamp(-1.0, 1.0).acos();
        let sin_theta = theta.sin();
        let wa = ((1.0 - t) * theta).sin() / sin_theta;
        let wb = (t * theta).sin() / sin_theta;
        (a * wa + b * wb).normalize()
    }
}

impl Mul for Quat {
    type Output = Self;

    /// Iloczyn Hamiltona — złożenie obrotów (`a * b` to najpierw `b`, potem `a`).
    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
            self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
        )
    }
}

impl Mul<f32> for Quat {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs, self.w * rhs)
    }
}

impl Add for Quat {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self::new(
            self.x + rhs.x,
            self.y + rhs.y,
            self.z + rhs.z,
            self.w + rhs.w,
        )
    }
}

impl Sub for Quat {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self::new(
            self.x - rhs.x,
            self.y - rhs.y,
            self.z - rhs.z,
            self.w - rhs.w,
        )
    }
}

impl Neg for Quat {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, -self.w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::scalar::is_close;

    #[test]
    fn obrot_o_90_stopni_wokol_z() {
        let q = Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2);
        let v = q.rotate_vec3(Vec3::X);
        assert!(is_close(v.x, 0.0, 1e-6) && is_close(v.y, 1.0, 1e-6));
    }

    #[test]
    fn zlozenie_obrotow_jako_mnozenie() {
        let a = Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_4);
        let b = Quat::from_axis_angle(Vec3::X, std::f32::consts::FRAC_PI_4);
        let v = Vec3::new(1.0, 0.0, 0.0);
        let via_quat = (a * b).rotate_vec3(v);
        let via_mat = a.to_mat3().mul_vec3(b.to_mat3().mul_vec3(v));
        assert!(via_quat.distance(via_mat) < 1e-5);
    }

    #[test]
    fn macierz_i_kwaternion_zgadzaja_sie_w_dwie_strony() {
        let q = Quat::from_euler(0.3, -0.7, 1.1);
        let back = Quat::from_mat3(q.to_mat3());
        let v = Vec3::new(0.3, -0.9, 0.5);
        assert!(q.rotate_vec3(v).distance(back.rotate_vec3(v)) < 1e-5);
    }

    #[test]
    fn slerp_idzie_najkrotsza_droga() {
        let a = Quat::from_axis_angle(Vec3::Y, 0.0);
        let b = Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
        let mid = a.slerp(b, 0.5);
        assert!(is_close(mid.angle(), std::f32::consts::FRAC_PI_4, 1e-5));

        let c = -b;
        let mid2 = a.slerp(c, 0.5);
        assert!(is_close(mid2.angle(), std::f32::consts::FRAC_PI_4, 1e-5));
    }

    #[test]
    fn odwrotnosc_odwraca_obrot() {
        let q = Quat::from_euler(0.2, 0.4, 0.6);
        let inv = q.inverse().expect("niezerowy kwaternion");
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert!(q.rotate_vec3(inv.rotate_vec3(v)).distance(v) < 1e-5);
        assert!(Quat::new(0.0, 0.0, 0.0, 0.0).inverse().is_none());
    }

    #[test]
    fn normalizacja_zera_daje_tozsamosc() {
        assert_eq!(Quat::new(0.0, 0.0, 0.0, 0.0).normalize(), Quat::IDENTITY);
    }
}