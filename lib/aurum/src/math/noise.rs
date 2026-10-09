//! Szum proceduralny: `value noise`, `perlin`, `fbm` i `curl`.
//!
//! Wszystkie funkcje są deterministyczne — ten sam punkt zawsze daje ten sam
//! wynik, więc można je liczyć na CPU i na GPU i porównywać wyniki w testach.
//! Wartości wychodzą w przedziale `[-1, 1]` (poza `hash_to_unit`, które daje
//! `[0, 1)`).
//!
//! ```
//! use aurum::math::noise::{fbm_2, value_noise_2};
//!
//! let n = value_noise_2(1.5, -2.5);
//! assert!((-1.0..=1.0).contains(&n));
//! assert!((-1.0..=1.0).contains(&fbm_2(1.5, -2.5, 4, 2.0, 0.5)));
//! ```

use crate::math::Vec2;

/// Miesza dwie liczby 32-bitowe w jedną — „Wang hash”, bez maskowania na wejściu.
pub fn hash_u32(mut x: u32) -> u32 {
    x = (x ^ 61) ^ (x >> 16);
    x = x.wrapping_mul(9);
    x ^= x >> 4;
    x = x.wrapping_mul(0x27d4_eb2d);
    x ^ (x >> 15)
}

/// Hash dwuwymiarowy (indeksy całkowite).
pub fn hash_2(x: i32, y: i32) -> u32 {
    hash_u32((x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841))
}

/// Hash trójwymiarowy (indeksy całkowite).
pub fn hash_3(x: i32, y: i32, z: i32) -> u32 {
    hash_2(x, y) ^ hash_u32(z as u32)
}

/// Zamienia hash na liczbę w przedziale `[0, 1)`.
pub fn hash_to_unit(hash: u32) -> f32 {
    // 24 bity mantysy — zakres [0, 1) bez utraty równomierności.
    (hash >> 8) as f32 / (1u32 << 24) as f32
}

/// Wygładzenie Hermite’a — zerowe pochodne na końcach przedziału.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Szum wartości w 2D — interpolacja losowych wartości z siatki całkowitej.
pub fn value_noise_2(x: f32, y: f32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = fade(x - x0);
    let fy = fade(y - y0);
    let (ix, iy) = (x0 as i32, y0 as i32);

    let v00 = hash_to_unit(hash_2(ix, iy));
    let v10 = hash_to_unit(hash_2(ix + 1, iy));
    let v01 = hash_to_unit(hash_2(ix, iy + 1));
    let v11 = hash_to_unit(hash_2(ix + 1, iy + 1));

    let top = v00 + (v10 - v00) * fx;
    let bottom = v01 + (v11 - v01) * fx;
    ((top + (bottom - top) * fy) * 2.0 - 1.0).clamp(-1.0, 1.0)
}

/// Szum wartości w 3D.
pub fn value_noise_3(x: f32, y: f32, z: f32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let z0 = z.floor();
    let fx = fade(x - x0);
    let fy = fade(y - y0);
    let fz = fade(z - z0);
    let (ix, iy, iz) = (x0 as i32, y0 as i32, z0 as i32);

    let sample = |dx: i32, dy: i32, dz: i32| hash_to_unit(hash_3(ix + dx, iy + dy, iz + dz));

    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;

    let c00 = lerp(sample(0, 0, 0), sample(1, 0, 0), fx);
    let c10 = lerp(sample(0, 1, 0), sample(1, 1, 0), fx);
    let c01 = lerp(sample(0, 0, 1), sample(1, 0, 1), fx);
    let c11 = lerp(sample(0, 1, 1), sample(1, 1, 1), fx);

    let c0 = lerp(c00, c10, fy);
    let c1 = lerp(c01, c11, fy);
    (lerp(c0, c1, fz) * 2.0 - 1.0).clamp(-1.0, 1.0)
}

/// Szum gradientowy (Perlina) w 2D — ciągłe nachylenie, więc daje ładne linie.
pub fn perlin_2(x: f32, y: f32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let (ix, iy) = (x0 as i32, y0 as i32);
    let (fx, fy) = (x - x0, y - y0);

    // Gradienty z ośmiu kierunków na siatki, liczone dla wejścia (gx, gy).
    let gradient = |dx: i32, dy: i32, gx: f32, gy: f32| -> f32 {
        let h = hash_2(ix + dx, iy + dy) & 7;
        let angle = h as f32 * (std::f32::consts::TAU / 8.0);
        angle.cos() * gx + angle.sin() * gy
    };

    let n00 = gradient(0, 0, fx, fy);
    let n10 = gradient(1, 0, fx - 1.0, fy);
    let n01 = gradient(0, 1, fx, fy - 1.0);
    let n11 = gradient(1, 1, fx - 1.0, fy - 1.0);

    // Interpolacja Hermite’a po obu osiach — daje ciągłe drugie pochodne.
    let u = fade(fx);
    let v = fade(fy);
    let top = n00 + u * (n10 - n00);
    let bottom = n01 + u * (n11 - n01);
    (top + v * (bottom - top)).clamp(-1.0, 1.0)
}

/// Szum fraktalny (fBm) — suma oktaw o malejącej amplitudzie.
///
/// Każda oktawa ma `lacunarity` razy wyższą częstotliwość i `gain` razy
/// mniejszą amplitudę. Wynik jest znormalizowany do `[-1, 1]`.
pub fn fbm_2(x: f32, y: f32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
    if octaves == 0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut norm = 0.0;
    for _ in 0..octaves {
        sum += value_noise_2(x * frequency, y * frequency) * amplitude;
        norm += amplitude;
        frequency *= lacunarity;
        amplitude *= gain;
    }
    if norm == 0.0 {
        0.0
    } else {
        (sum / norm).clamp(-1.0, 1.0)
    }
}

/// Szum prążkowy — oktawy odwrócone i wyprostowane, daje ostre grzbienie.
pub fn ridged_fbm_2(x: f32, y: f32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
    if octaves == 0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut norm = 0.0;
    let mut weight = 1.0;
    for _ in 0..octaves {
        let signal = 1.0 - value_noise_2(x * frequency, y * frequency).abs();
        let shaped = signal * signal * weight;
        weight = shaped.clamp(0.0, 1.0);
        sum += shaped * amplitude;
        norm += amplitude;
        frequency *= lacunarity;
        amplitude *= gain;
    }
    if norm == 0.0 {
        0.0
    } else {
        (sum / norm).clamp(-1.0, 1.0)
    }
}

/// Równoleżnik pola wektorowego — do dymu, płynów i animacji sietki.
///
/// Liczy dwie niezależne pochodne szumu wartości różnicą centralną, więc wynik
/// nie znika tam, gdzie sam szum miałby zerowy gradient.
pub fn curl_2(x: f32, y: f32) -> Vec2 {
    const EPS: f32 = 0.1;
    let dx = (value_noise_2(x + EPS, y) - value_noise_2(x - EPS, y)) / (2.0 * EPS);
    let dy = (value_noise_2(x, y + EPS) - value_noise_2(x, y - EPS)) / (2.0 * EPS);
    Vec2::new(dx, -dy)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_daje_integers() {
        assert_eq!(hash_u32(1), hash_u32(1));
        assert_ne!(hash_u32(1), hash_u32(2));
        for i in 0..64 {
            let u = hash_to_unit(hash_2(i, i * 7));
            assert!((0.0..1.0).contains(&u), "{u} poza zakresem [0, 1)");
        }
    }

    #[test]
    fn szum_wartosci_jest_ciagly() {
        // Wierzchołki siatki są wygładzone, więc szum nie skacze.
        let eps = 1e-3;
        for i in 0..50 {
            let x = i as f32 * 0.37;
            let y = i as f32 * -0.21;
            let a = value_noise_2(x, y);
            let b = value_noise_2(x + eps, y);
            assert!((a - b).abs() < 0.1, "skok szumu: {a} vs {b}");
            assert!((-1.0..=1.0).contains(&a));
        }
    }

    #[test]
    fn szum_jest_deterministyczny() {
        assert_eq!(value_noise_2(3.7, -9.1), value_noise_2(3.7, -9.1));
        assert_eq!(fbm_2(1.0, 2.0, 5, 2.0, 0.5), fbm_2(1.0, 2.0, 5, 2.0, 0.5));
    }

    #[test]
    fn perlin_i_szum_3d_mieszcza_sie_w_zakresie() {
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.13, i as f32 * 0.29);
            assert!((-1.0..=1.0).contains(&perlin_2(x, y)));
            assert!((-1.0..=1.0).contains(&value_noise_3(x, y, 0.5)));
        }
    }

    #[test]
    fn zero_oktaw_daje_zero() {
        assert_eq!(fbm_2(1.0, 1.0, 0, 2.0, 0.5), 0.0);
        assert_eq!(ridged_fbm_2(1.0, 1.0, 0, 2.0, 0.5), 0.0);
    }

    #[test]
    fn curl_jest_skonczony() {
        let c = curl_2(1.3, -2.2);
        assert!(c.is_finite());
    }
}