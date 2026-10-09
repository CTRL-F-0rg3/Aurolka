//! Symulacja cząsteczek na GPU: ruch + odbicia od ścian + historia ruchu.
//!
//! ```text
//! cargo run -p aurum --example particles
//! ```
//!
//! Pokazuje, jak złożyć własny kernel z gotowymi operacjami z `ops`:
//! pozycje i prędkości żyją w buforach GPU, a klatka animacji to kilka
//! dispatchów bez synchronizacji z procesorem.

use aurum::bytemuck::{Pod, Zeroable};
use aurum::gpu::{Binding, BindingKind, Buffer, Context, Kernel, KernelDesc};
use aurum::math::Vec2;
use aurum::Result;

/// Pozycja i prędkość cząsteczki — `vec2<f32>` × 2 w jednym buforze.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Particle {
    position: Vec2,
    velocity: Vec2,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    dt: f32,
    width: u32,
    height: u32,
    _pad0: f32,
    _pad1: f32,
}

/// Jeden krok symulacji: pozycja += prędkość · dt, odbicie od ścian.
const STEP_WGSL: &str = r#"
struct Particle {
    position: vec2<f32>,
    velocity: vec2<f32>,
};

struct Params {
    dt: f32,
    width: u32,
    height: u32,
    _pad0: f32,
    _pad1: f32,
};



@group(0) @binding(0) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(1) var<uniform>             params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&particles)) { return; }

    var p = particles[i];

    p.position = p.position + p.velocity * params.dt;

    // Odbicie od ścian z utratą 0.1% energii.
    let limit = vec2<f32>(f32(params.width), f32(params.height));
    if (p.position.x < 0.0) { p.position.x = -p.position.x; p.velocity.x = -p.velocity.x * 0.999; }
    if (p.position.x > limit.x) { p.position.x = 2.0 * limit.x - p.position.x; p.velocity.x = -p.velocity.x * 0.999; }
    if (p.position.y < 0.0) { p.position.y = -p.position.y; p.velocity.y = -p.velocity.y * 0.999; }
    if (p.position.y > limit.y) { p.position.y = 2.0 * limit.y - p.position.y; p.velocity.y = -p.velocity.y * 0.999; }

    particles[i] = p;
}
"#;

fn main() -> Result<()> {
    let ctx = Context::new()?;
    println!("adapter: {} ({:?})", ctx.info().name, ctx.info().backend);

    let (width, height) = (200u32, 60u32);
    let liczba = 20_000usize;
    let kroki = 200u32;

    // Start: pozycje na siatce z małym szumem, prędkości losowe.
    let mut seed = 0x5eed_1234u32;
    let mut nastepny = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };

    let cząsteczki: Vec<Particle> = (0..liczba)
        .map(|i| {
            let gx = (i % 200) as f32;
            let gy = (i / 200) as f32;
            Particle {
                position: Vec2::new(gx + nastepny(), gy + nastepny()),
                velocity: Vec2::new(nastepny() * 2.0 - 1.0, nastepny() * 2.0 - 1.0)
                    .normalize()
                    * (0.5 + nastepny()),
            }
        })
        .collect();

    let bufor = Buffer::from_slice(&ctx, &cząsteczki, "czasteczki")?;
    let params = Buffer::uniform(
        &ctx,
        &[Params {
            dt: 1.0 / 60.0,
            width,
            height,
            _pad0: 0.0,
            _pad1: 0.0,
        }],
        "params",
    )?;

    let kernel = Kernel::new(
        &ctx,
        KernelDesc {
            label: "particles.step",
            source: STEP_WGSL,
            entry_point: "main",
            bindings: &[BindingKind::Storage, BindingKind::Uniform],
        },
    )?;
    let group = kernel.bind_group(&ctx, &[Binding::write(&bufor), Binding::uniform(&params)])?;
    let grupy = aurum::gpu::groups_for(liczba, 64);

    // Cała symulacja w jednym `submit` — procesor nie dotyka danych.
    let start = std::time::Instant::now();
    ctx.submit(|encoder| {
        for _ in 0..kroki {
            kernel.dispatch(encoder, &group, (grupy, 1, 1))?;
        }
        Ok(())
    })?;
    ctx.poll()?;
    let czas = start.elapsed();

    let wynik = bufor.read(&ctx)?;
    let w_sumie: f32 = wynik.iter().map(|p| p.position.x + p.position.y).sum();
    let srednia = w_sumie / wynik.len() as f32;

    println!(
        "{liczba} cząsteczek × {kroki} kroków w {czas:.2?} ({:.1} mln kroków/s)",
        (liczba * kroki as usize) as f64 / 1.0 / 1e6 / (czas.as_secs_f64().max(1e-9))
    );
    println!("średnia pozycja (x + y): {srednia:.2}");

    // Fizyczna sanity-check: wszystkie cząsteczki mają być w boku.
    let poza = wynik
        .iter()
        .filter(|p| {
            p.position.x < -1.0
                || p.position.x > width as f32 + 1.0
                || p.position.y < -1.0
                || p.position.y > height as f32 + 1.0
        })
        .count();
    assert_eq!(poza, 0, "{poza} cząsteczek wyszło poza planszę");

    Ok(())
}