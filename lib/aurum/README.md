# Aurum

Biblioteka matematyczna i obliczeniowa na GPU oparta na [`wgpu`](https://github.com/gfx-rs/wgpu).
Dwa katalogi: **matematyka na CPU** (wektory, macierze, kwaterniony, szum, splajny,
statystyka) i **obliczenia na GPU** (bufory, kernele WGSL, gotowe operacje).

> Status: API (v0.1) zamrożone od strony semantycznej — enumy oznaczone
> `#[non_exhaustive]`, typy liczb należą do biblioteki (nie do `wgpu`).

## Dlaczego

| Problem | Rozwiązanie |
|---|---|
| Wybór urządzenia i kolejki za każdym razem | `Context::new()` — raz, z cache pipeline’ów i layoutów |
| Błędy WGSL jako asynchroniczny `panic` | walidacja `naga` przed pipeline’em → `Error::Shader` z linią i kolumną |
| Ręczne `copy_buffer_to_buffer` i mapowanie | `Buffer::read` robi to samo (bufor stagingowy, jeden `poll`) |
| Shader „na wiarę” | każdy element kodu ma odpowiednik CPU, a testy porównują obie strony |

## Szybki start

```rust
use aurum::gpu::{Binding, BindingKind, Buffer, Context, Kernel, KernelDesc};
use aurum::Result;

fn main() -> Result<()> {
    let ctx = Context::new()?;

    let x = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0, 4.0], "x")?;
    let y = Buffer::from_slice(&ctx, &[0.5f32, 0.5, 0.5, 0.5], "y")?;

    // Kernel liczy kwadrat każdego elementu.
    let kernel = Kernel::new(&ctx, KernelDesc {
        label: "kwadrat",
        source: r#"
            @group(0) @binding(0) var<storage, read>       src: array<f32>;
            @group(0) @binding(1) var<storage, read_write> dst: array<f32>;

            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                let i = gid.x;
                if (i >= arrayLength(&src)) { return; }
                dst[i] = src[i] * src[i];
            }
        "#,
        entry_point: "main",
        bindings: &[BindingKind::ReadOnlyStorage, BindingKind::Storage],
    })?;

    let group = kernel.bind_group(&ctx, &[Binding::read(&x), Binding::write(&y)])?;
    ctx.submit(|encoder| kernel.dispatch(encoder, &group, (1, 1, 1)))?;

    assert_eq!(y.read(&ctx)?, vec![1.0, 4.0, 9.0, 16.0]);
    Ok(())
}
```

## Gotowe operacje

```rust
use aurum::gpu::{Buffer, Context};
use aurum::ops;
use aurum::Result;

fn main() -> Result<()> {
    let ctx = Context::new()?;
    let x = Buffer::from_slice(&ctx, &vec![3.0f32; 100_000], "x")?;

    ops::reduce::sum(&ctx, &x)?;             // suma, min, max, mean, argmax
    ops::scan::prefix_sum(&ctx, &x)?;        // sumy prefiksowe
    ops::linalg::mat_mul(&ctx, &a, &b, m, k, n)?;  // macierze kafelkowane 16×16
    ops::filter::gaussian_2d(&ctx, &img, w, h, 1.5)?;  // splot separowalny
    ops::histogram::histogram(&ctx, &x, 32, -1.0, 1.0)?;  // na atomikach
    ops::field::mandelbrot(&ctx, 512, -0.6, 0.0, 3.0, 500)?;  // pola proceduralne
    Ok(())
}
```

Wyrażenia elementowe (`ops::elementwise`) podaje się **tekstem WGSL**, nie
domknięciem — dzięki temu działają dla każdej arytmetyki, a wygenerowany
pipeline trafia do cache’a:

```rust
ops::elementwise::map(&ctx, &x, "sin(x)")?;
ops::elementwise::zip(&ctx, &x, &y, "x * y")?;
ops::elementwise::axpy(&ctx, &x, &y, 0.5)?;   // 0.5·x + y
```

## Matematyka na CPU

```rust
use aurum::math::{Mat4, Quat, Vec3};
use aurum::math::noise::fbm_2;

// Wektory i macierze są `Pod`, więc trafiają do buforów bez rzutowania.
let m = Mat4::translation(Vec3::new(1.0, 2.0, 3.0)) * Mat4::rotation_z(0.5);
let q = Quat::from_axis_angle(Vec3::Z, 0.5);

// Ten sam hash co w WGSL — wynik GPU i CPU zgadza się co do bita.
let szum = fbm_2(1.5, 2.5, 4, 2.0, 0.5);
```

## Uruchomienie przykładów

```bash
cargo run -p aurum --example mandelbrot     # zbiór mandelbrota + porównanie z CPU
cargo run -p aurum --example matmul         # mnożenie macierzy 512×512
cargo run -p aurum --example particles      # 20 tys. cząsteczek, 200 kroków
cargo test -p aurum                          # 53 jednostkowe + 26 integracyjnych
```

## Uwagi

* Kod biblioteki nie używa `unsafe` (`#![forbid(unsafe_code)]`).
* `Context` nie jest `Sync` — jeden kontekst na wątek; do pracy równoległej twórz
  osobne.
* Buforów uniformowych nie da się odczytać na CPU (to decyzja WebGPU, nie
  biblioteki).
* Testy GPU **pomijają się**, gdy w systemie nie ma adaptera — brak urządzenia to
  nie sukces obliczeń.

## Licencja

MIT lub Apache-2.0.