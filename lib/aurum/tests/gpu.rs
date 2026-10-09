//! Testy GPU — każdą operację porównujemy z tą samą liczbą na CPU.
//!
//! Jeśli w systemie nie ma adaptera GPU (CI na maszynie bez karty, kontener
//! bez `lavapipe`), testy są **pomijane**, a nie „zielone na siłę” — brak
//! urządzenia to nie sukces obliczeń.
//!
//! ```text
//! cargo test -p aurum --test gpu
//! ```

use aurum::error::Error;
use aurum::gpu::{Binding, BindingKind, Buffer, Context, Kernel, KernelDesc};
use aurum::math::{stats, Vec3};
use aurum::ops;

/// Kontekst albo `None`, gdy na maszynie nie ma GPU.
fn context() -> Option<Context> {
    match Context::new() {
        Ok(ctx) => Some(ctx),
        Err(Error::NoAdapter) | Err(Error::Adapter(_)) => {
            eprintln!("pomijam: brak adaptera GPU");
            None
        }
        Err(e) => panic!("nie udało się utworzyć kontekstu: {e}"),
    }
}

/// Porównuje wektory z tolerancją względną.
fn assert_close(gpu: &[f32], cpu: &[f32], eps: f32, what: &str) {
    assert_eq!(gpu.len(), cpu.len(), "{what}: różna długość");
    for (index, (g, c)) in gpu.iter().zip(cpu).enumerate() {
        assert!(
            (g - c).abs() <= eps * c.abs().max(1.0),
            "{what}: element {index} — GPU {g}, CPU {c}"
        );
    }
}

/// Kontekst albo wczesny powrót z testu.
macro_rules! gpu_or_skip {
    () => {
        match context() {
            Some(ctx) => ctx,
            None => return,
        }
    };
}

#[test]
fn bufor_zapis_odczyt_i_rozmiar() {
    let ctx = gpu_or_skip!();

    let bufor = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0, 4.0], "x").unwrap();
    assert_eq!(bufor.len(), 4);
    assert_eq!(bufor.bytes(), 16);
    assert_eq!(bufor.read(&ctx).unwrap(), vec![1.0, 2.0, 3.0, 4.0]);

    // Zapis krótszy niż bufor zostawia resztę bez zmian.
    bufor.write(&ctx, &[9.0]).unwrap();
    assert_eq!(bufor.read(&ctx).unwrap(), vec![9.0, 2.0, 3.0, 4.0]);

    // Zapis dłuższy niż bufor to błąd, nie ciche obcięcie danych.
    assert!(bufor.write(&ctx, &[0.0; 9]).is_err());

    // Bufor zerowy musi mieć dodatnią długość.
    assert!(Buffer::<f32>::zeros(&ctx, 0, "pusty").is_err());

    // Wektory przechodzą bez rzutowania — są `Pod`.
    let punkty = Buffer::from_slice(&ctx, &[Vec3::X, Vec3::Y, Vec3::Z], "wektory").unwrap();
    assert_eq!(punkty.bytes(), 36);
    assert_eq!(punkty.read(&ctx).unwrap()[2], Vec3::Z);
}

#[test]
fn wlasny_kernel_liczy_kwadraty() {
    let ctx = gpu_or_skip!();

    const WGSL: &str = r#"
@group(0) @binding(0) var<storage, read>       src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&src)) { return; }
    dst[i] = src[i] * src[i] + 1.0;
}
"#;

    let dane: Vec<f32> = (0..1000).map(|i| i as f32 * 0.5 - 100.0).collect();
    let src = Buffer::from_slice(&ctx, &dane, "src").unwrap();
    let dst = Buffer::zeros(&ctx, dane.len(), "dst").unwrap();

    let kernel = Kernel::new(
        &ctx,
        KernelDesc {
            label: "test.kwadraty",
            source: WGSL,
            entry_point: "main",
            bindings: &[BindingKind::ReadOnlyStorage, BindingKind::Storage],
        },
    )
    .unwrap();

    let group = kernel
        .bind_group(&ctx, &[Binding::read(&src), Binding::write(&dst)])
        .unwrap();

    // 1000 elementów przy 64 wątkach na grupę = 16 grup.
    ctx.submit(|encoder| kernel.dispatch(encoder, &group, (16, 1, 1)))
        .unwrap();

    let oczekiwane: Vec<f32> = dane.iter().map(|x| x * x + 1.0).collect();
    assert_close(&dst.read(&ctx).unwrap(), &oczekiwane, 1e-6, "kwadraty");
}

#[test]
fn bledy_wgsl_wracaja_jako_bled_biblioteki() {
    let ctx = gpu_or_skip!();

    let zly = Kernel::new(
        &ctx,
        KernelDesc {
            label: "test.zly",
            source: "@compute fn main( { to nie jest WGSL",
            entry_point: "main",
            bindings: &[],
        },
    );
    assert!(zly.is_err(), "zły WGSL nie może przejść");

    // Ten sam shader drugi raz — błąd jest błędem, nie `panic`em.
    assert!(ctx.validate("@compute fn main( {").is_err());
}

#[test]
fn niezgodne_bindingi_sa_wykrywane() {
    let ctx = gpu_or_skip!();

    const WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> src: array<f32>;
@compute @workgroup_size(1)
fn main() { let x = arrayLength(&src); }
"#;

    let kernel = Kernel::new(
        &ctx,
        KernelDesc {
            label: "test.bindingi",
            source: WGSL,
            entry_point: "main",
            bindings: &[BindingKind::ReadOnlyStorage],
        },
    )
    .unwrap();

    let a = Buffer::from_slice(&ctx, &[1.0f32], "a").unwrap();

    // Za mało bindingów...
    assert!(kernel.bind_group(&ctx, &[]).is_err());
    // ...i zły rodzaj: shader czyta, a binding chce pisać.
    assert!(kernel.bind_group(&ctx, &[Binding::write(&a)]).is_err());
    // Właściwy przechodzi.
    assert!(kernel.bind_group(&ctx, &[Binding::read(&a)]).is_ok());

    // Dispatch bez grup roboczych jest błędem, a nie cichym wykonaniem.
    let group = kernel.bind_group(&ctx, &[Binding::read(&a)]).unwrap();
    assert!(ctx
        .submit(|encoder| kernel.dispatch(encoder, &group, (0, 1, 1)))
        .is_err());
}

#[test]
fn cache_pipelineow_dziala() {
    let ctx = gpu_or_skip!();

    let a = Buffer::from_slice(&ctx, &[2.0f32, 3.0], "a").unwrap();

    // Ten sam shader liczony trzykrotnie: `map` za każdym razem buduje
    // `Kernel`, więc cache pipeline’ów powinien zadziałać.
    let pierwsze = ops::elementwise::map(&ctx, &a, "x * x").unwrap();
    let pipeliney_po_pierwszym = ctx.cached_pipelines();
    let drugie = ops::elementwise::map(&ctx, &a, "x * x").unwrap();

    assert_eq!(pierwsze, vec![4.0, 9.0]);
    assert_eq!(pierwsze, drugie);
    assert_eq!(
        ctx.cached_pipelines(),
        pipeliney_po_pierwszym,
        "powtórzony shader nie powinien tworzyć nowego pipeline’u"
    );
}

#[test]
fn elementowe_operacje_zgadzaja_sie_z_cpu() {
    let ctx = gpu_or_skip!();

    let dane: Vec<f32> = (0..4096).map(|i| (i % 100) as f32 - 50.0).collect();
    let odwrotne: Vec<f32> = dane.iter().rev().copied().collect();
    let x = Buffer::from_slice(&ctx, &dane, "x").unwrap();
    let y = Buffer::from_slice(&ctx, &odwrotne, "y").unwrap();

    assert_close(
        &ops::elementwise::map(&ctx, &x, "x * x + 1.0").unwrap(),
        &dane.iter().map(|v| v * v + 1.0).collect::<Vec<_>>(),
        1e-6,
        "map",
    );
    assert_close(
        &ops::elementwise::zip(&ctx, &x, &y, "x + y").unwrap(),
        &dane
            .iter()
            .zip(&odwrotne)
            .map(|(a, b)| a + b)
            .collect::<Vec<_>>(),
        1e-6,
        "zip",
    );
    assert_close(
        &ops::elementwise::axpy(&ctx, &x, &y, 0.25).unwrap(),
        &dane
            .iter()
            .zip(&odwrotne)
            .map(|(a, b)| 0.25 * a + b)
            .collect::<Vec<_>>(),
        1e-6,
        "axpy",
    );
    assert_close(
        &ops::elementwise::clamp(&ctx, &x, -10.0, 10.0).unwrap(),
        &dane.iter().map(|v| v.clamp(-10.0, 10.0)).collect::<Vec<_>>(),
        1e-6,
        "clamp",
    );

    // Różne długości to błąd, a nie ciche przetworzenie mniejszego zakresu.
    let krotszy = Buffer::from_slice(&ctx, &dane[..10], "krotszy").unwrap();
    assert!(ops::elementwise::zip(&ctx, &x, &krotszy, "x + y").is_err());
}

#[test]
fn redukcje_zgadzaja_sie_z_cpu() {
    let ctx = gpu_or_skip!();

    // Więcej niż jedna grupa robocza (256 wątków) i więcej niż jeden etap.
    let dane: Vec<f32> = (0..10_000).map(|i| ((i * 37) % 211) as f32 - 105.0).collect();
    let x = Buffer::from_slice(&ctx, &dane, "x").unwrap();

    let suma = ops::reduce::sum(&ctx, &x).unwrap();
    assert!(
        (suma - stats::sum(&dane)).abs() < 1e-1,
        "suma: GPU {suma}, CPU {}",
        stats::sum(&dane)
    );
    assert_eq!(ops::reduce::min(&ctx, &x).unwrap(), stats::min(&dane));
    assert_eq!(ops::reduce::max(&ctx, &x).unwrap(), stats::max(&dane));

    let (index, value) = ops::reduce::argmax(&ctx, &x).unwrap();
    assert_eq!(index, stats::argmax(&dane).unwrap());
    assert_eq!(value, stats::max(&dane));

    let (index, value) = ops::reduce::argmin(&ctx, &x).unwrap();
    assert_eq!(index, stats::argmin(&dane).unwrap());
    assert_eq!(value, stats::min(&dane));

    // Rozmiar nie będący wielokrotnością grupy roboczej też musi działać.
    let ogon = Buffer::from_slice(&ctx, &dane[..333], "ogon").unwrap();
    let suma_ogona = ops::reduce::sum(&ctx, &ogon).unwrap();
    assert!((suma_ogona - stats::sum(&dane[..333])).abs() < 1e-2);
}

#[test]
fn iloczyn_skalarny_zgadza_sie_z_cpu() {
    let ctx = gpu_or_skip!();

    let a: Vec<f32> = (0..5000).map(|i| i as f32 * 0.001).collect();
    let b: Vec<f32> = (0..5000).map(|i| (5000 - i) as f32 * 0.002).collect();
    let ba = Buffer::from_slice(&ctx, &a, "a").unwrap();
    let bb = Buffer::from_slice(&ctx, &b, "b").unwrap();

    let recznie: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    let na_gpu = ops::linalg::dot(&ctx, &ba, &bb).unwrap();

    assert!(
        (na_gpu - recznie).abs() < recznie.abs() * 1e-3 + 1e-3,
        "GPU {na_gpu} vs CPU {recznie}"
    );
}

#[test]
fn sumy_prefiksowe_zgadzaja_sie_z_cpu() {
    let ctx = gpu_or_skip!();

    // Długość przekracza kilka bloków 256 elementów i nie jest ich wielokrotnością.
    let dane: Vec<f32> = (0..1000).map(|i| (i % 7) as f32 - 3.0).collect();
    let x = Buffer::from_slice(&ctx, &dane, "x").unwrap();

    assert_close(
        &ops::scan::prefix_sum(&ctx, &x).unwrap(),
        &stats::cumulative_sum(&dane),
        1e-4,
        "prefix_sum",
    );
}

#[test]
fn mnozenie_macierzy_zgadza_sie_z_cpu() {
    let ctx = gpu_or_skip!();

    let (rows, inner, cols) = (37usize, 23usize, 41usize);
    let a: Vec<f32> = (0..rows * inner).map(|i| (i % 13) as f32 - 6.0).collect();
    let b: Vec<f32> = (0..inner * cols).map(|i| (i % 11) as f32 - 5.0).collect();

    let ba = Buffer::from_slice(&ctx, &a, "a").unwrap();
    let bb = Buffer::from_slice(&ctx, &b, "b").unwrap();
    let wynik = ops::linalg::mat_mul(&ctx, &ba, &bb, rows, inner, cols).unwrap();

    let mut oczekiwane = vec![0.0f32; rows * cols];
    for (row, chunk) in oczekiwane.chunks_mut(cols).enumerate() {
        for (col, value) in chunk.iter_mut().enumerate() {
            *value = (0..inner).map(|k| a[row * inner + k] * b[k * cols + col]).sum();
        }
    }
    assert_close(&wynik, &oczekiwane, 1e-4, "mat_mul");

    // Podwójna transpozycja jest tożsamością.
    let t = ops::linalg::transpose(&ctx, &ba, rows, inner).unwrap();
    let bt = Buffer::from_slice(&ctx, &t, "t").unwrap();
    let tt = ops::linalg::transpose(&ctx, &bt, inner, rows).unwrap();
    assert_close(&tt, &a, 1e-6, "transpozycja dwa razy");

    // Złe wymiary są błędem, nie cichym wykonaniem.
    assert!(ops::linalg::mat_mul(&ctx, &ba, &bb, rows, inner + 1, cols).is_err());
    assert!(ops::linalg::mat_mul(&ctx, &ba, &bb, 0, inner, cols).is_err());
}

#[test]
fn splot_gaussa_rozmywa_i_zachowuje_sume() {
    let ctx = gpu_or_skip!();

    let (width, height) = (16usize, 12usize);
    let mut obraz = vec![0.0f32; width * height];
    obraz[width * height / 2 + width / 2] = 100.0; // jeden jasny piksel

    let src = Buffer::from_slice(&ctx, &obraz, "obraz").unwrap();
    let rozmyty = ops::filter::gaussian_2d(&ctx, &src, width, height, 1.5).unwrap();

    // Jądro znormalizowane do sumy 1, więc suma energii się nie zmienia.
    let suma: f32 = rozmyty.iter().sum();
    assert!((suma - 100.0).abs() < 1e-2, "suma po rozmyciu: {suma}");

    // Piksel poza środkiem dostał część energii, sam środek stracił część.
    let srodek = width * height / 2 + width / 2;
    assert!(rozmyty[srodek] < 100.0 && rozmyty[srodek] > 0.0);
    assert!(rozmyty.iter().filter(|&&v| v > 0.5).count() > 1);

    let suma_jadra: f32 = ops::filter::gaussian_kernel(1.5).iter().sum();
    assert!((suma_jadra - 1.0).abs() < 1e-5);

    // Nieparzysta długość jądra jest wymagana.
    assert!(ops::filter::convolve_1d(&ctx, &src, &[0.5, 0.5]).is_err());
}

#[test]
fn histogram_zgadza_sie_z_cpu() {
    let ctx = gpu_or_skip!();

    let dane: Vec<f32> = (0..10_000).map(|i| (i % 1000) as f32 / 999.0).collect();
    let x = Buffer::from_slice(&ctx, &dane, "x").unwrap();

    assert_eq!(
        ops::histogram::histogram(&ctx, &x, 10, 0.0, 1.0).unwrap(),
        stats::histogram(&dane, 10, 0.0, 1.0)
    );

    // Wartości poza zakresem są pomijane — tak samo na CPU i GPU.
    let z_mixem: Vec<f32> = dane
        .iter()
        .copied()
        .chain([-5.0f32, 5.0f32])
        .collect();
    let mieszany = Buffer::from_slice(&ctx, &z_mixem, "mieszany").unwrap();
    assert_eq!(
        ops::histogram::histogram(&ctx, &mieszany, 10, 0.0, 1.0).unwrap(),
        stats::histogram(&z_mixem, 10, 0.0, 1.0)
    );

    // Niepoprawny zakres to błąd.
    assert!(ops::histogram::histogram(&ctx, &x, 0, 0.0, 1.0).is_err());
    assert!(ops::histogram::histogram(&ctx, &x, 10, 1.0, 0.0).is_err());
}

#[test]
fn szum_gpu_zgadza_sie_z_cpu() {
    let ctx = gpu_or_skip!();

    // Ten sam algorytm co `math::noise::fbm_2` — różnica może być tylko
    // w ostatnich bitach (kolejność operacji na GPU jest inna).
    let size = 12usize;
    let gpu = ops::field::fbm_2d(&ctx, size, 4, 2.0, 0.5).unwrap();

    for (index, value) in gpu.iter().enumerate() {
        let (x, y) = ((index % size) as f32, (index / size) as f32);
        let cpu = aurum::math::noise::fbm_2(x, y, 4, 2.0, 0.5);
        assert!(
            (value - cpu).abs() < 1e-5,
            "piksel ({x}, {y}): GPU {value}, CPU {cpu}"
        );
    }
}

#[test]
fn mandelbrot_zgadza_sie_z_liczeniem_na_cpu() {
    let ctx = gpu_or_skip!();

    let size = 32usize;
    let max_iter = 64u32;
    let scale = 3.0f32;
    let obraz = ops::field::mandelbrot(&ctx, size, 0.0, 0.0, scale, max_iter).unwrap();

    // Ta sama pętla co w shaderze, na CPU — porównujemy wszystkie piksele.
    let unit = scale / size as f32;
    let mut niezgodne = 0;
    for y in 0..size {
        for x in 0..size {
            let cx = (x as f32 - size as f32 * 0.5) * unit;
            let cy = (y as f32 - size as f32 * 0.5) * unit;
            let (mut zx, mut zy) = (0.0f32, 0.0f32);
            let mut i = 0u32;
            while i < max_iter && zx * zx + zy * zy <= 4.0 {
                let next = zx * zx - zy * zy + cx;
                zy = 2.0 * zx * zy + cy;
                zx = next;
                i += 1;
            }
            let oczekiwane = obraz[y * size + x];
            if oczekiwane != i {
                niezgodne += 1;
                assert!(niezgodne < 5, "piksel ({x}, {y}): GPU {oczekiwane}, CPU {i}");
            }
        }
    }
    assert_eq!(niezgodne, 0, "GPU i CPU różnią się w {niezgodne} pikselach");
}