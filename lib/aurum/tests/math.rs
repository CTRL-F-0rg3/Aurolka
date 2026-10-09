//! Testy matematyki CPU — bez GPU, więc działają wszędzie.
//!
//! Wartości porównywane są z literalami i z tożsamościami algebraicznymi
//! (`(A·B)ᵀ = Bᵀ·Aᵀ`, `q⁻¹·q = 1`, …), a nie z „wynikiem z przeszłości”.

use aurum::math::noise::{fbm_2, perlin_2, value_noise_2};
use aurum::math::scalar::{clamp, lerp, mix_angle, remap, smoothstep, wrap, TAU};
use aurum::math::spline::{catmull_rom_path, cubic_bezier, easing};
use aurum::math::stats::{self, cumulative_sum, linspace};
use aurum::math::{Complex, Mat2, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

fn close(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

/// Porównuje dwa wektory z tolerancją.
fn close3(a: Vec3, b: Vec3, eps: f32) -> bool {
    a.distance(b) <= eps
}

#[test]
fn wektory_podstawowa_algebra() {
    let a = Vec3::new(1.0, 2.0, 3.0);
    let b = Vec3::splat(4.0);

    assert_eq!(a + b, Vec3::new(5.0, 6.0, 7.0));
    assert_eq!(b - a, Vec3::new(3.0, 2.0, 1.0));
    assert_eq!(a * 2.0, Vec3::new(2.0, 4.0, 6.0));
    assert_eq!(a.dot(b), 24.0);

    // Iloczyn wektorowy jest antykomutatywny i prostopadły do obu wektorów.
    let c = a.cross(b);
    assert!(close(c.dot(a), 0.0, 1e-5) && close(c.dot(b), 0.0, 1e-5));
    assert!(close3(b.cross(a), -c, 1e-5));

    // Wektor jednostkowy × sam siebie daje zerową długość.
    let n = a.normalize();
    assert!(close(n.length(), 1.0, 1e-6));
    assert!(close(n.cross(a).length(), 0.0, 1e-5));
}

#[test]
fn wektor_odbija_sie_wzgl_normalnej() {
    let v = Vec2::new(1.0, -1.0);
    let normal = Vec2::new(0.0, 1.0);
    let odbity = v.reflect(normal);
    assert!(close(odbity.y, 1.0, 1e-6));
    // Kąt padający = kąt odbicia.
    assert!(close(v.dot(normal), -odbity.dot(normal), 1e-6));
}

#[test]
fn macierze_maja_jednostkowy_wyznacznik() {
    // Tożsamość ma wyznacznik 1 i neutralność mnożenia — dla każdego rozmiaru.
    assert_eq!((Mat2::IDENTITY * Mat2::IDENTITY).determinant(), 1.0);
    assert_eq!((Mat3::IDENTITY * Mat3::IDENTITY).determinant(), 1.0);
    assert_eq!((Mat4::IDENTITY * Mat4::IDENTITY).determinant(), 1.0);
    assert_eq!(Mat3::IDENTITY.trace(), 3.0);
    assert_eq!(Mat4::IDENTITY.trace(), 4.0);
}

/// Mnożenie macierzy wierszowo — odpowiednik kernela z `ops::linalg`.
fn mat_mul_cpu(a: &[f32], b: &[f32], rows: usize, inner: usize, cols: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; rows * cols];
    for row in 0..rows {
        for col in 0..cols {
            let mut sum = 0.0;
            for k in 0..inner {
                sum += a[row * inner + k] * b[k * cols + col];
            }
            out[row * cols + col] = sum;
        }
    }
    out
}

#[test]
fn mnozenie_macierzy_odpowiada_petli_cpu() {
    let a: Vec<f32> = linspace(-2.0, 2.0, 9).into_iter().map(|v| v * 0.3).collect();
    let b: Vec<f32> = linspace(1.0, 3.0, 9).into_iter().map(|v| 1.0 - v * 0.2).collect();

    let a_tab: [f32; 9] = a.clone().try_into().expect("9 elementów");
    let b_tab: [f32; 9] = b.clone().try_into().expect("9 elementów");

    // Ten sam wynik daje typ biblioteki i jawna pętla — na tych samych danych.
    let przez_typy = Mat3::from_rows(a_tab).mul_mat(Mat3::from_rows(b_tab)).to_rows();
    let przez_petle = mat_mul_cpu(&a, &b, 3, 3, 3);
    for (index, value) in przez_typy.iter().enumerate() {
        assert!(
            close(*value, przez_petle[index], 1e-5),
            "element {index}: {value} != {}",
            przez_petle[index]
        );
    }

    // Tożsamość: A · I = A, a transpozycja odwraca mnożenie.
    let x = Mat3::from_rows(a_tab);
    let y = Mat3::from_rows(b_tab);
    assert_eq!(x.mul_mat(Mat3::IDENTITY), x);
    assert_eq!((x * y).transpose(), y.transpose() * x.transpose());
}

#[test]
fn macierz4x4_odwrotnica_odtwarza_punkt() {
    let m = Mat4::translation(Vec3::new(2.0, -3.0, 0.5))
        * Mat4::rotation_z(0.7)
        * Mat4::scale(Vec3::new(2.0, 0.5, 1.5));

    let inv = m.inverse().expect("macierz odwracalna");
    let p = Vec3::new(0.3, -1.2, 4.0);
    let q = inv.mul_point(m.mul_point(p));
    assert!(close3(q, p, 1e-3));

    // Wyznacznik uwzględnia skalę i obrót (obrót nie zmienia objętości).
    assert!(close(m.determinant(), 2.0 * 0.5 * 1.5, 1e-3));
}

#[test]
fn kwaternion_tozsamosc_z_kompozycja() {
    let a = Quat::from_euler(0.2, -1.1, 0.9);
    let b = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 0.4);

    // Iloczyn kwaternionów to złożenie obrotów.
    let v = Vec3::new(0.2, -1.0, 0.5);
    assert!(close3(
        (a * b).rotate_vec3(v),
        a.rotate_vec3(b.rotate_vec3(v)),
        1e-5
    ));

    // Tożsamość nie zmienia niczego.
    assert!(close3(Quat::IDENTITY.rotate_vec3(v), v, 1e-6));

    // Slerp daje stałą prędkość kątową.
    let start = Quat::IDENTITY;
    let end = Quat::from_axis_angle(Vec3::Y, 1.0);
    assert!(close(start.slerp(end, 0.25).angle(), 0.25, 1e-4));
    assert!(close(start.slerp(end, 1.0).angle(), 1.0, 1e-4));
}

#[test]
fn liczby_zespolone_mnozenie_dzielenie_pierwiastek() {
    let z = Complex::new(3.0f64, 4.0);
    assert!(close(z.abs() as f32, 5.0, 1e-6));
    assert!(close(z.norm_squared() as f32, 25.0, 1e-6));

    let w = Complex::new(1.0f64, -2.0);
    let back = z * w / w;
    assert!(close(back.re as f32, 3.0, 1e-9) && close(back.im as f32, 4.0, 1e-9));

    let sqrt = z.sqrt();
    assert!(close((sqrt * sqrt - z).re as f32, 0.0, 1e-9));
    assert!(close((sqrt * sqrt - z).im as f32, 0.0, 1e-9));
}

#[test]
fn szum_jest_w_zakresie_i_deterministyczny() {
    for i in 0..500 {
        let (x, y) = (i as f32 * 0.031, i as f32 * -0.017);
        let v = value_noise_2(x, y);
        assert!((-1.0..=1.0).contains(&v), "value noise {v} poza zakresem");
        assert!((-1.0..=1.0).contains(&perlin_2(x, y)));
        assert!((-1.0..=1.0).contains(&fbm_2(x, y, 5, 2.0, 0.5)));
        assert_eq!(v, value_noise_2(x, y));
    }
}

#[test]
fn splajny_sa_ciagle_w_punktach_kontrolnych() {
    let punkty = [
        Vec2::ZERO,
        Vec2::new(1.0, 2.0),
        Vec2::new(2.0, 0.0),
        Vec2::new(3.0, 1.0),
    ];
    for (i, p) in punkty.iter().enumerate() {
        let v = catmull_rom_path(&punkty, i as f32).unwrap();
        assert!(v.distance(*p) < 1e-5, "punkt {i}: {v:?} != {p:?}");
    }

    // Bézier z definicji: środek łuku jest bliżej punktów sterujących niż końców.
    let (p0, p1, p2, p3) = (
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 4.0),
        Vec2::new(2.0, 4.0),
        Vec2::new(3.0, 0.0),
    );
    let srodek = cubic_bezier(p0, p1, p2, p3, 0.5);
    assert!(srodek.distance(p1) < srodek.distance(p0));

    // Easingu są niemalejące i mieszczą się w [0, 1].
    let mut poprzednia = -1.0;
    for k in 0..=20 {
        let t = k as f32 / 20.0;
        let v = easing::in_out_cubic(t);
        assert!(v >= poprzednia - 1e-6, "easingu nie rośnie w {t}");
        assert!((0.0..=1.0).contains(&v));
        poprzednia = v;
    }
}

#[test]
fn statystyki_zgadzaja_sie_z_zapisem_ręcznym() {
    let dane: Vec<f32> = (0..100).map(|i| (i * 7 % 23) as f32 - 11.0).collect();

    let reczna_suma: f32 = dane.iter().sum();
    assert!(close(stats::sum(&dane), reczna_suma, 1e-2));
    assert!(close(stats::mean(&dane), reczna_suma / 100.0, 1e-3));
    assert!(close(stats::min(&dane), -11.0, 1e-6));
    assert!(close(stats::max(&dane), 11.0, 1e-6));
    assert_eq!(dane[stats::argmax(&dane).unwrap()], stats::max(&dane));
    assert_eq!(dane[stats::argmin(&dane).unwrap()], stats::min(&dane));

    // Ostatnia suma prefiksowa to suma całego wektora.
    let mut skumulowane = cumulative_sum(&dane);
    assert!(close(skumulowane.pop().unwrap(), reczna_suma, 1e-2));
}

#[test]
fn skalary_zgadzaja_sie_z_definicja() {
    assert_eq!(clamp(11.0, 0.0, 10.0), 10.0);
    assert_eq!(lerp(-2.0f32, 2.0, 0.25), -1.0);
    assert_eq!(remap(0.25f32, 0.0, 1.0, 10.0, 20.0), 12.5);
    assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
    assert_eq!(wrap(-0.5, 1.0), 0.5);
    // Krótka droga z 0 do 350° to 0.2 rad wstecz, więc w połowie −0.1.
    assert!(close(mix_angle(0.0, TAU - 0.2, 0.5), -0.1, 1e-5));
}

#[test]
fn typy_wektorow_maja_uklad_zgodny_z_gpu() {
    // Rozmiar i wyrównanie muszą się zgadzać z `vec2/3/4<f32>` w WGSL.
    assert_eq!(std::mem::size_of::<Vec2>(), 2 * 4);
    assert_eq!(std::mem::size_of::<Vec3>(), 3 * 4);
    assert_eq!(std::mem::size_of::<Vec4>(), 4 * 4);
    assert_eq!(std::mem::align_of::<Vec3>(), 4);

    // `bytemuck::Pod` wymaga zerowego wypełnienia — inaczej GPU zobaczy śmieci.
    let v = Vec4::new(1.0, 2.0, 3.0, 4.0);
    let bajty: &[u8] = bytemuck::bytes_of(&v);
    assert_eq!(bajty.len(), 16);
    assert_eq!(f32::from_le_bytes([bajty[0], bajty[1], bajty[2], bajty[3]]), 1.0);
}