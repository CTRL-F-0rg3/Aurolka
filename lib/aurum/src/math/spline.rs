//! Splajny i krzywe ułatwiające animacje.
//!
//! Wszystkie funkcje operują na wektorach 2D — to format, w którym animuje się
//! ruch kamery i UI. Krzywe easingu przyjmują `t` z przedziału `[0, 1]` i
//! zwracają `0` na początku oraz `1` na końcu.
//!
//! ```
//! use aurum::math::spline::{catmull_rom, easing};
//! use aurum::math::Vec2;
//!
//! let p0 = Vec2::new(0.0, 0.0);
//! let p1 = Vec2::new(1.0, 2.0);
//! let p2 = Vec2::new(2.0, 0.0);
//! let p3 = Vec2::new(3.0, 2.0);
//! assert_eq!(catmull_rom(p0, p1, p2, p3, 0.0), p1);
//! assert_eq!(easing::smooth_step(0.5), 0.5);
//! ```

use crate::math::Vec2;

/// Krzywa Catmull-Rom pomiędzy `p1` a `p2` (`p0` i `p3` to sąsiedzi).
///
/// Dla `t = 0` zwraca `p1`, dla `t = 1` zwraca `p2`; łuk jest ciągły w całej
/// serii punktów, więc można podstawić dowolny ciąg i uzyskać gładką ścieżkę.
pub fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let t2 = t * t;
    let t3 = t2 * t;
    (((p1 * 2.0) + (p2 - p0) * t)
        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
        + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3)
        * 0.5
}

/// Krzywa Béziera stopnia drugiego.
pub fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    p0 * (u * u) + p1 * (2.0 * u * t) + p2 * (t * t)
}

/// Krzywa Béziera stopnia trzeciego.
pub fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

/// Punkt na ścieżce Catmull-Rom zbudowanej z listy punktów.
///
/// `t` liczy segmenty: `0.0` to pierwszy punkt, `len - 1` to ostatni.
pub fn catmull_rom_path(points: &[Vec2], t: f32) -> Option<Vec2> {
    if points.len() < 2 {
        return None;
    }
    let max = (points.len() - 1) as f32;
    let t = t.clamp(0.0, max);
    let segment = (t.floor() as usize).min(points.len() - 2);
    let local = t - segment as f32;

    let p = |i: isize| -> Vec2 {
        let index = (segment as isize + i).clamp(0, points.len() as isize - 1) as usize;
        points[index]
    };
    Some(catmull_rom(p(-1), p(0), p(1), p(2), local))
}

/// Zestaw krzywych easingu — każda zwraca `0` dla `t = 0` i `1` dla `t = 1`.
pub mod easing {
    /// Interpolacja liniowa.
    pub fn linear(t: f32) -> f32 {
        t.clamp(0.0, 1.0)
    }

    /// Przybliżenie kwadratowe (start).
    pub fn in_quad(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t
    }

    /// Przybliżenie kwadratowe (koniec).
    pub fn out_quad(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * (2.0 - t)
    }

    /// Przybliżenie kwadratowe (oba końce).
    pub fn in_out_quad(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t < 0.5 {
            2.0 * t * t
        } else {
            -1.0 + (4.0 - 2.0 * t) * t
        }
    }

    /// Przybliżenie sześcienne (koniec).
    pub fn out_cubic(t: f32) -> f32 {
        let u = t.clamp(0.0, 1.0) - 1.0;
        u * u * u + 1.0
    }

    /// Przybliżenie sześcienne (oba końce) — „ease in out”.
    pub fn in_out_cubic(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            let u = -2.0 * t + 2.0;
            1.0 - u * u * u / 2.0
        }
    }

    /// Wykładnicze wygasanie (koniec) — bardzo szybki start.
    pub fn out_expo(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t >= 1.0 {
            1.0
        } else {
            1.0 - 2f32.powf(-10.0 * t)
        }
    }

    /// Wygładzone przejście Hermite’a.
    pub fn smooth_step(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Jeszcze gładsze przejście (zerowe drugie pochodne na końcach).
    pub fn smoother_step(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    /// Sprężynowy nawrót — lekki „przeskok” ponad cel.
    pub fn out_back(t: f32) -> f32 {
        const C: f32 = 1.70158;
        let t = t.clamp(0.0, 1.0);
        1.0 + (C + 1.0) * (t - 1.0).powi(3) + C * (t - 1.0).powi(2)
    }

    /// Odbicie sprężynowe.
    pub fn out_bounce(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        const N: f32 = 7.5625;
        const D: f32 = 2.75;
        if t < 1.0 / D {
            N * t * t
        } else if t < 2.0 / D {
            let u = t - 1.5 / D;
            N * u * u + 0.75
        } else if t < 2.5 / D {
            let u = t - 2.25 / D;
            N * u * u + 0.9375
        } else {
            let u = t - 2.625 / D;
            N * u * u + 0.984375
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::scalar::is_close;

    #[test]
    fn catmull_rom_przechodzi_przez_punkty() {
        let p0 = Vec2::new(0.0, 0.0);
        let p1 = Vec2::new(1.0, 2.0);
        let p2 = Vec2::new(2.0, 0.0);
        let p3 = Vec2::new(3.0, 2.0);
        assert_eq!(catmull_rom(p0, p1, p2, p3, 0.0), p1);
        assert_eq!(catmull_rom(p0, p1, p2, p3, 1.0), p2);
    }

    #[test]
    fn sciezka_zaczyna_i_konczy_w_punktach() {
        let points = [Vec2::ZERO, Vec2::new(1.0, 1.0), Vec2::new(2.0, 0.0)];
        assert_eq!(catmull_rom_path(&points, 0.0).unwrap(), points[0]);
        let last = catmull_rom_path(&points, 2.0).unwrap();
        assert!(last.distance(points[2]) < 1e-5);
        assert!(catmull_rom_path(&[Vec2::ZERO], 0.0).is_none());
    }

    #[test]
    fn beziery_maja_dobre_koncowki() {
        let p0 = Vec2::new(0.0, 0.0);
        let p1 = Vec2::new(1.0, 3.0);
        let p2 = Vec2::new(2.0, 3.0);
        let p3 = Vec2::new(3.0, 0.0);
        assert_eq!(quadratic_bezier(p0, p1, p2, 0.0), p0);
        assert_eq!(cubic_bezier(p0, p1, p2, p3, 1.0), p3);
    }

    #[test]
    fn easingu_zawsze_zaczyna_i_konczy_w_0_i_1() {
        let krzywe: [fn(f32) -> f32; 11] = [
            easing::linear,
            easing::in_quad,
            easing::out_quad,
            easing::in_out_quad,
            easing::out_cubic,
            easing::in_out_cubic,
            easing::out_expo,
            easing::smooth_step,
            easing::smoother_step,
            easing::out_back,
            easing::out_bounce,
        ];
        for krzywa in krzywe {
            assert!(is_close(krzywa(0.0), 0.0, 1e-4), "początek {}", krzywa(0.0));
            assert!(is_close(krzywa(1.0), 1.0, 1e-4), "koniec {}", krzywa(1.0));
        }
    }

    #[test]
    fn easingu_zostaje_w_zakresie() {
        for t in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
            assert!((0.0..=1.0).contains(&easing::in_out_cubic(t)));
            assert!((0.0..=1.0).contains(&easing::smoother_step(t)));
        }
    }
}