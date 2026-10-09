//! Mnożenie macierzy: kafelkowane 16×16 na GPU vs. potrójna pętla na CPU.
//!
//! ```text
//! cargo run -p aurum --example matmul            # 512×512
//! cargo run -p aurum --example matmul -- 2048    # większe, by zobaczyć przewagę
//! ```

use std::time::Instant;

use aurum::gpu::{Buffer, Context};
use aurum::ops;

fn main() -> aurum::Result<()> {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(512);

    let ctx = Context::new()?;
    println!("adapter: {}", ctx.info().name);
    println!("mnożenie {n}×{n} (to jest {m} mnożeń skalarnych)\n", m = n * n * n);

    // Macierze o wartościach z zakresu [-1, 1] — bez tego `f32` szybko
    // traciłoby precyzję w sumach.
    let a: Vec<f32> = (0..n * n).map(|i| ((i * 7919) % 101) as f32 / 50.0 - 1.0).collect();
    let b: Vec<f32> = (0..n * n).map(|i| ((i * 6271) % 103) as f32 / 50.0 - 1.0).collect();

    let ba = Buffer::from_slice(&ctx, &a, "a")?;
    let bb = Buffer::from_slice(&ctx, &b, "b")?;

    // Kompilacja WGSL jest jednorazowa — osobno ją mierzymy.
    let start = Instant::now();
    let gpu = ops::linalg::mat_mul(&ctx, &ba, &bb, n, n, n)?;
    let z_kompilacji = start.elapsed();

    let start = Instant::now();
    let cpu = mat_mul_cpu(&a, &b, n);
    let czas_cpu = start.elapsed();

    let max_roznica = gpu
        .iter()
        .zip(&cpu)
        .map(|(g, c)| (g - c).abs())
        .fold(0.0f32, f32::max);
    let suma_gpu: f64 = gpu.iter().map(|&v| v as f64).sum();
    let suma_cpu: f64 = cpu.iter().map(|&v| v as f64).sum();

    println!("GPU (z kompilacją shadera): {z_kompilacji:.2?}");
    println!("CPU (1 rdzeń):              {czas_cpu:.2?}");
    println!(
        "przyspieszenie: {:.1}×",
        czas_cpu.as_secs_f64() / z_kompilacji.as_secs_f64().max(1e-9)
    );
    println!("maksymalna różnica: {max_roznica:.3e} (błąd zaokrągleń `f32`)");
    println!("suma GPU {suma_gpu:.4} vs CPU {suma_cpu:.4}");

    assert!(
        (suma_gpu - suma_cpu).abs() < suma_cpu.abs() * 1e-3 + 1e-2,
        "wyniki różnią się zbytnio"
    );

    // Drugie wywołanie korzysta z cache'u pipeline’ów — czas powinien spaść
    // do samego dispatchu i odczytu.
    let start = Instant::now();
    let _ = ops::linalg::mat_mul(&ctx, &ba, &bb, n, n, n)?;
    println!("\npowtórka z cache'em pipeline’ów: {:?}", start.elapsed());
    println!("pipeline’ów w cache: {}", ctx.cached_pipelines());

    Ok(())
}

/// Potrójna pętla — to samo, co robi shader, ale bez kafelków.
fn mat_mul_cpu(a: &[f32], b: &[f32], n: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; n * n];
    for row in 0..n {
        let a_slice = &a[row * n..row * n + n];
        let out_slice = &mut out[row * n..row * n + n];
        for (k, av) in a_slice.iter().enumerate() {
            let b_slice = &b[k * n..k * n + n];
            for (o, bv) in out_slice.iter_mut().zip(b_slice) {
                *o += av * bv;
            }
        }
    }
    out
}