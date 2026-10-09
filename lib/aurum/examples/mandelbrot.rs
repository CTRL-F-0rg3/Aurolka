//! Zbiór mandelbrota liczony na GPU, z porównaniem do CPU.
//!
//! ```text
//! cargo run -p aurum --example mandelbrot
//! ```
//!
//! Zadanie jest idealne na GPU: każdy piksel liczy setki iteracji, więc
//! transfer danych (kilka kiB rozmiaru i wyniku) jest znikomy wobec pracy.

use std::time::Instant;

use aurum::gpu::Context;
use aurum::ops;

fn main() -> aurum::Result<()> {
    let rozmiar: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(256);
    let max_iter: u32 = 500;

    let ctx = Context::new()?;
    println!(
        "adapter: {} ({:?}), typ {:?}",
        ctx.info().name,
        ctx.info().backend,
        ctx.info().device_type
    );
    println!("mandelbrot {rozmiar}×{rozmiar}, {max_iter} iteracji\n");

    // GPU: twardy reset, żeby zmierzyć sam kernel, bez tworzenia zasobów.
    let start = Instant::now();
    let gpu = ops::field::mandelbrot(&ctx, rozmiar, -0.6, 0.0, 3.0, max_iter)?;
    let czas_gpu = start.elapsed();

    // CPU: ta sama pętla, liczona szeregowo.
    let start = Instant::now();
    let cpu = mandelbrot_cpu(rozmiar, -0.6, 0.0, 3.0, max_iter);
    let czas_cpu = start.elapsed();

    // Różnice mogą wynikać z innej kolejności operacji zmiennoprzecinkowych
    // w shaderze, więc porównujemy z tolerancją.
    let roznic: Vec<(usize, u32, u32)> = (0..gpu.len())
        .filter(|&i| (gpu[i] as i64 - cpu[i] as i64).abs() > 1)
        .map(|i| (i, gpu[i], cpu[i]))
        .collect();
    let zgodne = gpu.len() - roznic.len();
    let srednia_gpu: f64 = gpu.iter().map(|&v| v as f64).sum::<f64>() / gpu.len() as f64;
    let srednia_cpu: f64 = cpu.iter().map(|&v| v as f64).sum::<f64>() / cpu.len() as f64;

    println!("GPU: {:>8.2?}  ({} pikseli)", czas_gpu, gpu.len());
    println!("CPU: {:>8.2?}", czas_cpu);
    println!(
        "przyspieszenie: {:.1}× (na 1 rdzeniu CPU)",
        czas_cpu.as_secs_f64() / czas_gpu.as_secs_f64().max(1e-9)
    );
    println!("średnia iteracji: GPU {srednia_gpu:.2}, CPU {srednia_cpu:.2}");
    println!("pikseli zgodnych: {zgodne}/{}", gpu.len());
    if let Some((index, g, c)) = roznic.first() {
        println!("pierwsza różnica: indeks {index} — GPU {g}, CPU {c}");
    }

    // Podgląd w terminalu — kilka wierszy ASCII z dokładnością 1/16 obrazu.
    println!();
    for y in (0..rozmiar).step_by((rozmiar / 24).max(1)) {
        let mut wiersz = String::with_capacity(rozmiar / 8);
        for x in (0..rozmiar).step_by((rozmiar / 80).max(1)) {
            let i = y * rozmiar + x;
            let znak = match gpu[i] {
                v if v >= max_iter => '#',
                v if v > max_iter / 2 => '*',
                v if v > max_iter / 8 => '+',
                v if v > max_iter / 32 => '.',
                v if v > 0 => ',',
                _ => ' ',
            };
            wiersz.push(znak);
        }
        println!("{wiersz}");
    }

    Ok(())
}

/// Ta sama pętla co w shaderze, ale na jednym rdzeniu.
fn mandelbrot_cpu(size: usize, cx: f32, cy: f32, scale: f32, max_iter: u32) -> Vec<u32> {
    let unit = scale / size as f32;
    let mut out = vec![0u32; size * size];

    for y in 0..size {
        for x in 0..size {
            let c = (
                cx + (x as f32 - size as f32 * 0.5) * unit,
                cy + (y as f32 - size as f32 * 0.5) * unit,
            );
            let (mut zx, mut zy) = (0.0f32, 0.0f32);
            let mut i = 0u32;
            while i < max_iter && zx * zx + zy * zy <= 4.0 {
                let next = zx * zx - zy * zy + c.0;
                zy = 2.0 * zx * zy + c.1;
                zx = next;
                i += 1;
            }
            out[y * size + x] = i;
        }
    }

    out
}