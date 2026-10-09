# Aurolka

Projekt **Aurolka** zawiera dwie rzeczy:

| Katalog | Co to jest |
|---|---|
| `lib/glaz` | biblioteka UI na `wgpu` + `winit` |
| `lib/aurum` | biblioteka matematyczna i obliczeniowa na GPU (`wgpu`) |
| `crates/pilot` | aplikacja „Pilot” do zarządzania muzyką |

Nazwy bibliotek to **`glaz`** (od *szkła*) i **`aurum`** (od *złota*) — nie mylić
z nazwą projektu.

---

## Pilot

Małe okienko z **czterema** przyciskami i folderem z muzyką.

```
┌──────────────────────────────────┐
│  PILOT                        ✕  │  ← własne dekoracje
│                                  │
│   Nocny_Ktokolwiek               │  ← nazwa utworu
│   odtwarzanie · 0:03 z 7         │  ← stan
│                                  │
│   ( ⏮ ) ( ⏯ ) ( ⏭ ) ( ⟳ )       │  ← 4 funkcje
└──────────────────────────────────┘
```

### Cztery funkcje

| Przycisk | Działanie |
|---|---|
| `⏮` | poprzedni utwór |
| `⏯` | odtwarzanie / pauza |
| `⏭` | następny utwór |
| `⟳` | ponowne skanowanie folderu `music/` |

### Uruchomienie

```bash
cargo run -p pilot
```

### Muzyka

Pilot czyta z `crates/pilot/music/` (nadpisz: `PILOT_MUSIC_DIR=...`).
Obsługiwane formaty: `mp3`, `flac`, `wav`, `ogg`, `m4a`, `aac`, `opus`.
Gdy folder jest pusty, pilot generuje trzy krótkie tony WAV, żeby dało się
to od razu sprawdzić. Szczegóły: [`crates/pilot/music/README.md`](crates/pilot/music/README.md).

---

## aurum

Biblioteka matematyczna liczona na GPU: wektory, macierze, kwaterniony, szum
i splajny na CPU oraz gotowe operacje compute (`suma`, `min`/`max`, sumy
prefiksowe, mnożenie macierzy, splot Gaussa, histogram, pola proceduralne) na
`wgpu`.

```rust
let ctx = aurum::gpu::Context::new()?;
let x = aurum::gpu::Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0], "x")?;
assert_eq!(aurum::ops::reduce::sum(&ctx, &x)?, 6.0);
```

```bash
cargo run -p aurum --example mandelbrot
cargo test -p aurum
```

Szczegóły: [`lib/aurum/README.md`](lib/aurum/README.md).

---

## glaz

Biblioteka UI w stylu `iced`, ale nastawiona na rzeczy, których `iced`
nie robi dobrze: **własne dekoracje okien** i **przezroczystość**.

```rust
glaz::run(MyApp::default(), WindowSettings::new()
    .size(glaz::geometry::Size::new(300.0, 190.0))
    .with_custom_decorations()
    .with_shape(glaz::WindowShape::rounded(14.0)));
```

### Co jest w środku

* `wgpu` 30, `winit` 0.30, `cosmic-text` 0.16
* premultiplied alpha, SDF-owe zaokrąglone prostokąty, cienie, obramowania
* offscreen `rgba16float` + separowalny blur → efekty „szkła”
* maska kształtu okna w ostatnim passie (zaokrąglone przezroczyste narożniki)
* hit-testing chrome w odwrotnej kolejności, żeby kontrolki wygrywały z przeciąganiem
* `#![forbid(unsafe_code)]`

### Ograniczenia

* „Acrylic” i „mica” to **presety wewnątrz aplikacji** — rozmywają zawartość
  sceny, nie pulpit za oknem. Efekt pulpitu wymagałby natywnego compositinga.
* Widgety: `container`, `column`, `row`, `stack`, `spacer`, `text`, `button`,
  `checkbox`, `slider`, obszary chrome (`draggable_area`, `resize_area`, `titlebar`).

### Testy

```bash
cargo test --workspace
```

