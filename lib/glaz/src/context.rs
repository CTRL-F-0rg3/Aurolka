//! Kontekst przekazywany widgetom w trakcie układania, rysowania i obsługi zdarzeń.
//!
//! Kontekst jest pożyczany — widgety nie mogą go przechowywać. Zawiera
//! renderer, motyw, stan kursora oraz bufory wyjściowe (wiadomości i obszary
//! dekoracji okna).

use std::time::Instant;

use crate::geometry::Size;
use crate::layout::Cursor;
use crate::message::Message;
use crate::renderer::Renderer;
use crate::theme::Theme;
use crate::window::chrome::Chrome;

/// Kontekst dostępny dla widgetów.
pub struct Context<'a> {
    /// Renderer (pomiar tekstu i kolejki rysowania).
    pub renderer: &'a mut Renderer,
    /// Aktywny motyw.
    pub theme: &'a Theme,
    /// Stan kursora.
    pub cursor: Cursor,
    /// Bufor wiadomości — widgety dopisują tu wyniki swojej pracy.
    pub output: &'a mut Vec<Message>,
    /// Kolektor obszarów własnych dekoracji okna (tylko w fazie układania).
    pub chrome: &'a mut Chrome,
    /// Czas od uruchomienia aplikacji — do animacji zależnych od czasu.
    pub time: Instant,
    /// Czy okno ma aktywny fokus.
    pub is_focused: bool,
    /// Rozmiar powierzchni okna w pikselach logicznych.
    pub bounds: Size,
}

impl<'a> Context<'a> {
    /// Nowy kontekst (używane wewnętrznie przez runtime).
    pub fn new(
        renderer: &'a mut Renderer,
        theme: &'a Theme,
        cursor: Cursor,
        output: &'a mut Vec<Message>,
        chrome: &'a mut Chrome,
        time: Instant,
        is_focused: bool,
        bounds: Size,
    ) -> Self {
        Self {
            renderer,
            theme,
            cursor,
            output,
            chrome,
            time,
            is_focused,
            bounds,
        }
    }

    /// Wysyła wiadomość do aplikacji.
    pub fn send(&mut self, message: Message) {
        self.output.push(message);
    }

    /// Wysyła wiadomość domenową dowolnego typu.
    pub fn send_custom<T: Send + 'static>(&mut self, payload: T) {
        self.output.push(Message::Custom(Box::new(payload)));
    }

    /// Czy kursor znajduje się nad zadanym prostokątem.
    pub fn cursor_over(&self, bounds: crate::geometry::Rect) -> bool {
        self.cursor.is_over && bounds.contains(self.cursor.position)
    }
}
