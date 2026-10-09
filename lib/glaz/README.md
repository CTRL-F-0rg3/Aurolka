# Glaz

Biblioteka UI oparta na [`wgpu`](https://github.com/gfx-rs/wgpu), skoncentrowana
na **własnych dekoracjach okien**, **przezroczystości** i **efektach backdrop**.

> Status: API (v0.1) zamrożone od strony semantycznej — enumy oznaczone
> `#[non_exhaustive]`, typy kolorów i wymiarów należą do biblioteki (nie do `wgpu`).

## Dlaczego

Tradycyjne biblioteki GUI albo nie pozwalają rysować własnego paska tytułowego,
albo robią to kosztem stabilności API. Glaz idzie w drugą stronę:

| Obszar | Rozwiązanie |
|---|---|
| Przezroczystość | okno z `CompositeAlphaMode::PreMultiplied`, renderowanie do `rgba16float`, maska kształtu w shaderze kompozycji |
| Zaokrąglone narożniki | `WindowShape` maskowany na GPU — działa tak samo na X11, Waylandzie, Windows i macOS |
| Blur / acrylic / mica | separowalny gaussian blur liczony na GPU; elementy „szklane” rozmywają to, co jest pod nimi |
| Własne dekoracje | widgety rejestrują `ChromeArea` podczas układania; hit-test rozstrzyga drag / resize / przyciski |
| Stabilność API | własne typy `Color`, `Point`, `Rect`; `#[non_exhaustive]`; brak `unsafe` w kodzie aplikacji |

## Szybki start

```rust
use glaz::prelude::*;
use glaz::widget;

struct App { clicks: u32 }
struct Clicked;

impl Application for App {
    fn update(&mut self, ctx: &mut Context<'_>, message: Message) -> Task {
        if message.is::<Clicked>() {
            self.clicks += 1;
        }
        Task::none()
    }

    fn view(&mut self) -> Element {
        widget::column![
            widget::text(format!("Kliknięcia: {}", self.clicks)),
            widget::button("Kliknij").on_press(|| Message::Custom(Box::new(Clicked))),
        ]
        .spacing(8.0)
        .padding(16.0)
        .build()
    }
}

fn main() -> Result<()> {
    glaz::run(App { clicks: 0 }, WindowSettings::new().title("Glaz"))
}
```

## Przezroczystość i własne dekoracje

```rust
let settings = WindowSettings::new()
    .with_custom_decorations()                        // bez systemowego paska
    .with_shape(WindowShape::rounded(12.0));           // zaokrąglone narożniki

fn view(&mut self) -> Element {
    widget::column![
        widget::draggable_area(widget::text("  Tytuł")),      // obszar przeciągania
        widget::window_button(WindowCommand::Close, "  ✕  "), // przycisk okna
        widget::container()
            .backdrop(BackdropPaint {                        // „szkło”
                radius: 24.0,
                tint: Color::from_rgba8(255, 255, 255, 30),
                saturation: 1.2,
            })
            .child(widget::text("Zawartość")),
    ]
    .spacing(0.0)
    .build()
}
```

Strefy zmiany rozmiaru przy krawędziach dodaje program automatycznie
(`resize_border`, domyślnie 8 px), więc okno jest zmienialne rozmiarem nawet
bez systemowych dekoracji.

## Jak to działa

```text
winit::WindowEvent
   → dekodowanie zdarzeń
   → chrome hit-test (drag / resize / przyciski)
   → layout drzewa (stan przenoszony po Id)
   → rysowanie: tło → [blur] → elementy szkliste → kompozycja z maską okna
```

* `geometry`, `layout`, `element`, `tree` — czysta logika, testowalna bez GPU.
* `renderer` — jedyne miejsce znające `wgpu`.
* `platform` — jedyne miejsce znające `winit`.

## Uruchomienie przykładów

```bash
cargo run -p aurola --example custom_chrome
cargo run -p aurola --example transparent
cargo test -p aurola
```

## Uwagi

* Kod biblioteki nie używa `unsafe` (`#![forbid(unsafe_code)]`).
* Blur działa na zawartości narysowanej w bieżącej klatce. Efekty **systemowe**
  (natywny acrylic Windows 11) są punktem rozszerzeń w `window::PlatformEffects`.
* Warstwa tekstu używa `cosmic-text` (kształtowanie) + `swash` (rasteryzacja)
  z atlasem glifów 2048² i cache per (tekst, styl, szerokość).

## Licencja

MIT lub Apache-2.0.
