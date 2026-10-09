//! Skalary: operacje, które działają na pojedynczej liczbie.
//!
//! Funkcje, które da się napisać czysto na `+`, `-`, `*`, `/` (jak [`clamp`],
//! [`lerp`] czy [`remap`]), są generyczne i działają dla `f32`, `f64` i typów
//! całkowitych. Pozostałe (`smoothstep`, `deg_to_rad`, ...) są liczone na
//! `f32`, bo ich odpowiedniki w WGSL też pracują na `f32`.

use std::ops::{Add, Div, Mul, Sub};

/// Liczba π.
pub const PI: f32 = std::f32::consts::PI;

/// Liczba 2π.
pub const TAU: f32 = PI * 2.0;

/// Podstawa logarytmu naturalnego.
pub const E: f32 = std::f32::consts::E;

/// 1 / √2.
pub const FRAC_1_SQRT_2: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Ogranicza `value` do przedziału `[min, max]`.
///
/// ```
/// use aurum::math::scalar::clamp;
/// assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
/// assert_eq!(clamp(-5.0, 0.0, 1.0), 0.0);
/// ```
pub fn clamp<T: PartialOrd>(value: T, min: T, max: T) -> T {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Interpolacja liniowa: `a` przy `t = 0`, `b` przy `t = 1`.
///
/// ```
/// use aurum::math::scalar::lerp;
/// assert_eq!(lerp(0.0f32, 10.0, 0.25), 2.5);
/// ```
pub fn lerp<T>(a: T, b: T, t: T) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<Output = T>,
{
    a + (b - a) * t
}

/// Przenosi wartość z jednego przedziału na drugi.
///
/// ```
/// use aurum::math::scalar::remap;
/// assert_eq!(remap(5.0f32, 0.0, 10.0, 0.0, 100.0), 50.0);
/// // pusty przedział wejściowy nie dzieli przez zero
/// assert_eq!(remap(5.0f32, 2.0, 2.0, 7.0, 9.0), 7.0);
/// ```
pub fn remap<T>(value: T, in_min: T, in_max: T, out_min: T, out_max: T) -> T
where
    T: Copy
        + PartialEq
        + Add<Output = T>
        + Sub<Output = T>
        + Mul<Output = T>
        + Div<Output = T>,
{
    if in_max == in_min {
        return out_min;
    }
    let t = (value - in_min) / (in_max - in_min);
    out_min + (out_max - out_min) * t
}

/// Wygładzone przejście 0 → 1 między `edge0` a `edge1`.
///
/// Wynik jest zaciągnięty do `[0, 1]` i ma zerowe pochodne na obu końcach,
/// więc nadaje się na współczynnik mieszania.
///
/// ```
/// use aurum::math::scalar::smoothstep;
/// assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
/// assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
/// assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
/// ```
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Jak [`smoothstep`], ale z zerowymi drugimi pochodnymi na końcach.
pub fn smootherstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Stopnie na radiany.
pub fn deg_to_rad(deg: f32) -> f32 {
    deg * (PI / 180.0)
}

/// Radiany na stopnie.
pub fn rad_to_deg(rad: f32) -> f32 {
    rad * (180.0 / PI)
}

/// Znak liczby: `-1.0`, `0.0` albo `1.0`.
pub fn signum(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Czy dwie liczby różnią się najwyżej o `epsilon`.
pub fn is_close(a: f32, b: f32, epsilon: f32) -> bool {
    (a - b).abs() <= epsilon
}

/// Część ułamkowa liczby (`x - floor(x)`, zawsze w `[0, 1)`).
pub fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// Zawija wartość do przedziału `[0, period)` — przydatne dla kątów i indeksów.
pub fn wrap(value: f32, period: f32) -> f32 {
    if period == 0.0 {
        return 0.0;
    }
    let wrapped = value % period;
    if wrapped < 0.0 {
        wrapped + period
    } else {
        wrapped
    }
}

/// Najbliższy kąt po skróceniu do `(-π, π]`.
///
/// ```
/// use aurum::math::scalar::{mix_angle, wrap, TAU};
///
/// // Z 350° do 10° idziemy o 20°, a nie o 340°, więc w połowie jesteśmy
/// // przy 360°, czyli co do 2π przy 0°.
/// let a = 350f32.to_radians();
/// let b = 10f32.to_radians();
/// let srodek = mix_angle(a, b, 0.5);
///
/// let do_zera = wrap(srodek, TAU);
/// let blisko_zera = do_zera < 1e-4 || TAU - do_zera < 1e-4;
/// assert!(blisko_zera, "środek = {srodek} rad");
/// ```
pub fn mix_angle(a: f32, b: f32, t: f32) -> f32 {
    let delta = wrap(b - a + PI, TAU) - PI;
    a + delta * t
}

/// Krzywa Gaussa `exp(-x² / (2σ²))` — jądro splotu Gaussa.
pub fn gaussian(x: f32, sigma: f32) -> f32 {
    if sigma <= 0.0 {
        return if x == 0.0 { 1.0 } else { 0.0 };
    }
    (-(x * x) / (2.0 * sigma * sigma)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_ogranicza_z_dwoch_stron() {
        assert_eq!(clamp(5, 0, 10), 5);
        assert_eq!(clamp(-1, 0, 10), 0);
        assert_eq!(clamp(11, 0, 10), 10);
    }

    #[test]
    fn lerp_dziala_dla_cala_kowego_typu() {
        assert_eq!(lerp(0i32, 10, 1), 10);
        assert_eq!(lerp(2.0f64, 4.0, 0.25), 2.5);
    }

    #[test]
    fn remap_zwraca_out_min_gdy_przedzial_pusty() {
        assert_eq!(remap(0.5f32, 0.0, 1.0, -1.0, 1.0), 0.0);
        assert_eq!(remap(3.0f32, 2.0, 2.0, 42.0, 43.0), 42.0);
    }

    #[test]
    fn smoothstep_zaciaga_do_jedynki() {
        assert_eq!(smoothstep(0.0, 2.0, 1.0), 0.5);
        assert_eq!(smootherstep(0.0, 1.0, 0.5), 0.5);
    }

    #[test]
    fn wrap_dziala_dla_ujemnych() {
        assert_eq!(wrap(-1.0, 4.0), 3.0);
        assert_eq!(wrap(5.0, 4.0), 1.0);
        assert_eq!(wrap(1.0, 0.0), 0.0);
    }

    #[test]
    fn mix_angle_idzie_krotsza_droga() {
        let wynik = mix_angle(0.0, TAU - 0.1, 0.5);
        assert!(wynik < 0.0, "kąt {wynik} powinien przejść przez zero");
    }

    #[test]
    fn gaussian_maksymalnie_w_zero() {
        assert_eq!(gaussian(0.0, 1.0), 1.0);
        // W 3σ wkład wynosi e^{-4.5} ≈ 0.011, więc jądro 3σ obejmuje ~99%.
        assert!(gaussian(3.0, 1.0) < 0.02);
        assert!(gaussian(1.0, 1.0) > gaussian(2.0, 1.0));
    }
}