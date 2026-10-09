//! **Pilot** — małe okienko do zarządzania muzyką.
//!
//! Cztery funkcje, dokładnie tyle:
//!
//! | Przycisk | Działanie |
//! |---|---|
//! | ⏮ | poprzedni utwór |
//! | ⏯ | odtwarzanie / pauza |
//! | ⏭ | następny utwór |
//! | ⟳ | ponowne skanowanie folderu `music/` |
//!
//! Okno ma własne dekoracje (przeciąganie za pasek, przycisk zamknięcia)
//! i przezroczyste tło z rozmyciem — dzięki temu wygląda jak mały pilot,
//! a nie jak kolejne okno menedżera plików.

mod audio;

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use audio::{format_time, Command, Playback, Shared};
use glaz::prelude::*;
use glaz::renderer::paint::BackdropPaint;
use glaz::widget;
use glaz::{column, row};
use lucide_icons::Icon;

/// Folder z muzyką (można nadpisać zmienną `PILOT_MUSIC_DIR`).
fn music_dir() -> PathBuf {
    std::env::var("PILOT_MUSIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("music"))
}

/// Wiadomości aplikacji (domena).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pilot {
    Previous,
    Toggle,
    Next,
    Rescan,
}

/// Aplikacja pilota.
struct App {
    playback: Shared,
    commands: Sender<Command>,
    folder: PathBuf,
}

impl App {
    fn new() -> Self {
        let folder = music_dir();
        let (playback, commands) = audio::Engine::spawn(folder.clone());
        Self {
            playback,
            commands,
            folder,
        }
    }

    /// Krótki stan odtwarzacza do rysowania.
    fn snapshot(&self) -> Playback {
        match self.playback.lock() {
            Ok(state) => state.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn send(&self, command: Command) {
        audio::send(&self.commands, command);
    }

    /// Tytuł utworu albo komunikat zastępczy.
    fn headline(state: &Playback) -> String {
        match state.index.and_then(|index| state.tracks.get(index)) {
            Some(path) => audio::track_title(path),
            None => "Brak muzyki".to_owned(),
        }
    }

    /// Linia pomocnicza: pozycja w kolejce albo komunikat błędu.
    fn subtitle(state: &Playback) -> String {
        if let Some(error) = &state.error {
            return error.clone();
        }
        let position = format_time(state.elapsed);
        match state.duration {
            Some(total) => {
                let counter = format!("{} / {}", position, format_time(total));
                match state.index {
                    Some(index) => format!("{counter}  ·  {} z {}", index + 1, state.tracks.len()),
                    None => counter,
                }
            }
            None => {
                let status = if state.playing {
                    "odtwarzanie"
                } else {
                    "pauza"
                };
                match state.index {
                    Some(index) => format!(
                        "{status}  ·  {}:{} z {}",
                        position,
                        index + 1,
                        state.tracks.len()
                    ),
                    None => format!("{status}  ·  {} plików", state.tracks.len()),
                }
            }
        }
    }
}

impl Application for App {
    fn update(&mut self, _ctx: &mut Context<'_>, message: Message) -> Task {
        let action = message.downcast_ref::<Pilot>().copied();

        match action {
            Some(Pilot::Previous) => self.send(Command::Previous),
            Some(Pilot::Toggle) => self.send(Command::Toggle),
            Some(Pilot::Next) => self.send(Command::Next),
            Some(Pilot::Rescan) => self.send(Command::Rescan(self.folder.clone())),
            None => {}
        }

        Task::none()
    }

    fn view(&mut self) -> Element {
        let state = self.snapshot();

        // Gotowy pasek tytulowy: obszar przeciagania + minimalizacja,
        // maksymalizacja i zamkniecie (ikony Lucide).
        let titlebar = widget::titlebar_with_icons("PILOT")
            .height(34.0)
            .button_size(Size::new(42.0, 34.0))
            .radius(Radius::uniform(9.0))
            .padding(Padding::symmetric(6.0, 0.0))
            .build();

        // Play/pause wymaga ikony zaleznej od stanu, wiec tu liczymy ja raz,
        // a nie trzymamy statyczna.
        let toggle_icon = if state.playing {
            Icon::Pause
        } else {
            Icon::Play
        };

        // Cztery przyciski — jedyne funkcje pilota.
        let controls = row![
            pilot_button(Icon::SkipBack, Pilot::Previous, "Poprzedni utwor"),
            pilot_button(toggle_icon, Pilot::Toggle, "Odtwarzaj / pauza"),
            pilot_button(Icon::SkipForward, Pilot::Next, "Nastepny utwor"),
            pilot_button(Icon::RefreshCw, Pilot::Rescan, "Skanuj ponownie"),
        ]
        .spacing(8.0)
        .width(Length::Fill)
        .build();

        // Panel „szklany". Kolor musi byc zadany wprost (`Container::draw`
        // rysuje, gdy jest kolor LUB backdrop), a promien dopasowany do
        // zaokraglenia okna, inaczej widac prostokat wewnatrz zaokraglenia.
        let panel = widget::container()
            .style(Color::rgba(1.0, 1.0, 1.0, 0.05))
            .backdrop(BackdropPaint {
                radius: 24.0,
                tint: Color::from_rgba8(255, 255, 255, 30),
                saturation: 1.3,
            })
            .radius(Radius::uniform(12.0))
            .border(1.0, Color::from_rgba8(255, 255, 255, 18))
            .padding(Padding::symmetric(16.0, 18.0))
            .child(
                column![
                    widget::text(Self::headline(&state))
                        .size(16.0)
                        .weight(600)
                        .color(Color::from_rgba8(255, 255, 255, 238))
                        .build(),
                    widget::spacer(0.0, 3.0).build(),
                    widget::text(Self::subtitle(&state))
                        .size(11.0)
                        .color(Color::from_rgba8(255, 255, 255, 140))
                        .build(),
                    widget::spacer(0.0, 16.0).build(),
                    controls,
                ]
                .spacing(0.0)
                .width(Length::Fill)
                .build(),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .build();

        column![
            titlebar,
            widget::container()
                .padding(Padding::symmetric(10.0, 6.0))
                .child(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .build(),
        ]
        .spacing(0.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .build()
    }
}

/// Jeden przycisk pilota — ikona Lucide z podpowiedzia dostepna
/// czytnikom ekranu.
fn pilot_button(icon: Icon, action: Pilot, hint: &str) -> Element {
    widget::button(icon.unicode().to_string())
        .on_press(move || Message::Custom(Box::new(action)))
        .tooltip(hint)
        .min_width(58.0)
        .padding(Padding::symmetric(12.0, 14.0))
        .radius(Radius::uniform(10.0))
        .build()
}

fn main() -> Result<()> {
    let folder = music_dir();
    // Pusty folder → generujemy trzy krótkie tony, żeby pilot od razu coś grał.
    audio::ensure_demo_tracks(&folder);

    let settings = WindowSettings::new()
        .title("Pilot")
        .size(Size::new(320.0, 208.0))
        // Ciemna, lekko przezroczysta baza. Bez tego `clear_color` to
        // `Color::TRANSPARENT`, okno wychodzi czarne, a rozmyte tło pod
        // „szklanym" panelem nie ma czego próbkowac.
        .background(Color::rgba(0.07, 0.07, 0.09, 0.82))
        .with_custom_decorations()
        .with_shape(WindowShape::rounded(16.0));

    glaz::run(App::new(), settings)
}
