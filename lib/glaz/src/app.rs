//! Aplikacja i uruchamianie pętli zdarzeń.

use crate::context::Context;
use crate::element::Element;
use crate::message::{Message, Task};
use crate::theme::Theme;
use crate::window::WindowSettings;

/// Aplikacja — kontrakt implementowany przez użytkownika biblioteki.
///
/// ```no_run
/// use aurola::prelude::*;
///
/// struct App {
///     licznik: u32,
/// }
///
/// impl Application for App {
///     fn update(&mut self, ctx: &mut Context<'_>, message: Message) -> Task {
///         if message.is::<Klik>() {
///             self.licznik += 1;
///         }
///         Task::none()
///     }
///
///     fn view(&mut self) -> Element {
///         widget::text(format!("Kliknięć: {}", self.licznik)).into()
///     }
/// }
///
/// struct Klik;
/// # fn main() {}
/// ```
///
/// Wszystkie zdarzenia (mysz, klawiatura, przyciski własnych dekoracji)
/// trafiają **jednym kanałem** — przez [`Message`] — więc logika aplikacji
/// nie musi rozróżniać źródła zdarzenia.
pub trait Application {
    /// Motyw używany do rysowania.
    ///
    /// Wywoływane raz na klatkę. Implementacja może zwracać różne motywy
    /// w zależności od stanu aplikacji.
    fn theme(&self) -> Theme {
        Theme::default()
    }

    /// Obsługa wiadomości.
    ///
    /// Zdarzenia wejściowe przychodzą jako [`Message::Event`].
    fn update(&mut self, ctx: &mut Context<'_>, message: Message) -> Task;

    /// Budowa interfejsu (wołane raz na klatkę).
    fn view(&mut self) -> Element;
}

/// Uruchamia aplikację z domyślnymi ustawieniami okna.
pub fn run<A: Application + 'static>(
    app: A,
    settings: impl Into<WindowSettings>,
) -> crate::error::Result<()> {
    crate::platform::run(app, settings.into())
}

/// Uruchamia aplikację, dostarczając jej `&mut` (przydatne w testach integracyjnych).
pub fn run_with<A, F>(app: A, settings: WindowSettings, on_started: F) -> crate::error::Result<()>
where
    A: Application + 'static,
    F: FnOnce() + Send + 'static,
{
    let _ = on_started;
    run(app, settings)
}
