//! Przykład: okno z własnymi dekoracjami i przezroczystym tłem.
//!
//! Uruchomienie:
//! ```bash
//! cargo run --example custom_chrome
//! ```

use glaz::prelude::*;
use glaz::renderer::paint::BackdropPaint;
use glaz::widget;
use glaz::{column, row};

struct Demo {
    clicks: u32,
    light: bool,
}

/// Wiadomość domenowa — tak przekazuje się dane do warstwy aplikacji
/// (np. z modelu AI).
struct Clicked;

impl Application for Demo {
    fn theme(&self) -> Theme {
        if self.light {
            Theme::light().with_transparency(window_transparency())
        } else {
            Theme::dark().with_transparency(window_transparency())
        }
    }

    fn update(&mut self, _ctx: &mut Context<'_>, message: Message) -> Task {
        if message.is::<Clicked>() {
            self.clicks += 1;
        }
        if matches!(message, Message::ToggleTheme) {
            self.light = !self.light;
        }
        Task::none()
    }

    fn view(&mut self) -> Element {
        column![
            row![
                widget::draggable_area(widget::text("  Moje okno").build())
                    .height(36.0)
                    .build(),
                widget::window_button(WindowCommand::Close, "  ✕  ")
                    .size(Size::new(52.0, 36.0))
                    .hover(Color::from_rgba8(200, 40, 40, 220))
                    .radius(Radius::uniform(9.0))
                    .build(),
            ]
            .spacing(0.0)
            .width(Length::Fill)
            .build(),
            self.content(),
        ]
        .spacing(0.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .build()
    }
}

impl Demo {
    fn content(&self) -> Element {
        widget::container()
            .padding(Padding::all(20.0))
            .backdrop(BackdropPaint {
                radius: 24.0,
                tint: Color::from_rgba8(255, 255, 255, 28),
                saturation: 1.2,
            })
            .child(
                column![
                    widget::text(format!("Kliknięcia: {}", self.clicks))
                        .size(20.0)
                        .build(),
                    widget::spacer(0.0, 12.0).build(),
                    widget::button("Kliknij mnie")
                        .on_press(|| Message::Custom(Box::new(Clicked)))
                        .accent(true)
                        .build(),
                    widget::spacer(0.0, 12.0).build(),
                    widget::button("Zmień motyw")
                        .on_press(|| Message::ToggleTheme)
                        .build(),
                ]
                .spacing(0.0)
                .build(),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .build()
    }
}

fn window_transparency() -> Transparency {
    Transparency::Blur(
        glaz::Backdrop::acrylic(Color::from_rgba8(30, 30, 40, 90))
            .shape(WindowShape::rounded(10.0)),
    )
}

fn main() -> Result<()> {
    let settings = WindowSettings::new()
        .title("Glaz — własne dekoracje")
        .size(Size::new(720.0, 460.0))
        .with_custom_decorations()
        .with_shape(WindowShape::rounded(10.0));

    glaz::run(
        Demo {
            clicks: 0,
            light: false,
        },
        settings,
    )
}
