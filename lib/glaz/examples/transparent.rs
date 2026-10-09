//! Przykład: okno w pełni przezroczyste z zaokrąglonymi narożnikami.
//!
//! Uruchomienie:
//! ```bash
//! cargo run --example transparent
//! ```
//!
//! Okno nie ma tła — widać przez nie pulpit. Zaokrąglone narożniki są
//! maskowane na GPU, więbo efekt działa identycznie na X11, Waylandzie,
//! Windows i macOS.

use glaz::column;
use glaz::prelude::*;
use glaz::widget;

struct Ball {
    /// Pozycja w czasie (animacja).
    phase: f32,
}

struct Bounce;

impl Application for Ball {
    fn theme(&self) -> Theme {
        Theme::dark()
    }

    fn update(&mut self, _ctx: &mut Context<'_>, message: Message) -> Task {
        if message.is::<Bounce>() {
            self.phase += 0.6;
        }
        Task::none()
    }

    fn view(&mut self) -> Element {
        // Okno jest przezroczyste, więc tło aplikacji musi być przezroczyste.
        widget::container()
            .style(Color::TRANSPARENT)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_content(Align::Center, Align::Center)
            .child(
                column![
                    widget::text("Przeciągnij okno za ten napis")
                        .size(18.0)
                        .build(),
                    widget::spacer(0.0, 16.0).build(),
                    widget::button("Odbij")
                        .on_press(|| Message::Custom(Box::new(Bounce)))
                        .build(),
                ]
                .spacing(0.0)
                .width(Length::Fill)
                .build(),
            )
            .build()
    }
}

fn main() -> Result<()> {
    let settings = WindowSettings::new()
        .title("Glaz — przezroczystość")
        .size(Size::new(640.0, 400.0))
        .with_custom_decorations()
        .with_shape(WindowShape::rounded(16.0));

    glaz::run(Ball { phase: 0.0 }, settings)
}
