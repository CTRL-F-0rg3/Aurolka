//! Zdarzenia wejściowe (mysz, klawiatura) oraz ich mapowanie z backendu.

use crate::geometry::Point;

/// Klawisz (bez modyfikatorów).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(String);

impl Key {
    /// Nowy klawisz z tekstu.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Klawisz znakowy.
    pub fn character(c: char) -> Self {
        Self(c.to_string())
    }

    /// Tekstowa reprezentacja klawisza.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Key {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for Key {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Klawisz funkcyjny / specjalny.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NamedKey {
    /// Brak klawisza specjalnego.
    #[default]
    Unidentified,
    /// Enter / Return.
    Enter,
    /// Escape.
    Escape,
    /// Spacja.
    Space,
    /// Tabulator.
    Tab,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Strzałka w górę.
    ArrowUp,
    /// Strzałka w dół.
    ArrowDown,
    /// Strzałka w lewo.
    ArrowLeft,
    /// Strzałka w prawo.
    ArrowRight,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
}

/// Kombinacja klawiszy i modyfikatorów.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// Klawisz.
    pub key: Key,
    /// Klawisz specjalny, jeśli dotyczy.
    pub named: NamedKey,
    /// Stan: `true` = wciśnięty.
    pub pressed: bool,
    /// Modyfikatory (Ctrl / Alt / Shift / Meta).
    pub modifiers: Modifiers,
    /// Powtórzenia systemowe (trzymanie klawisza).
    pub repeat: bool,
}

impl KeyEvent {
    /// Czy wciśnięto klawisz z zadanymi modyfikatorami.
    pub fn matches(&self, key: &str, modifiers: Modifiers) -> bool {
        self.pressed && self.key.0.eq_ignore_ascii_case(key) && self.modifiers.contains(modifiers)
    }
}

/// Zestaw aktywnych modyfikatorów.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Ctrl (Control / Command na macOS).
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// Alt (Option na macOS).
    pub alt: bool,
    /// Meta (Super / Windows / Command).
    pub meta: bool,
}

impl Modifiers {
    /// Brak modyfikatorów.
    pub const NONE: Self = Self {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };

    /// Zwraca `true`, jeśli wszystkie wymagane bity są ustawione.
    pub fn contains(self, other: Self) -> bool {
        (!other.ctrl || self.ctrl)
            && (!other.shift || self.shift)
            && (!other.alt || self.alt)
            && (!other.meta || self.meta)
    }
}

/// Przycisk myszy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MouseButton {
    /// Lewy.
    Left,
    /// Środkowy.
    Middle,
    /// Prawy.
    Right,
    /// Nieznany przycisk (np. dodatkowy).
    #[default]
    Other,
}

/// Kierunek przewijania.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// Przewinięcie w liniach tekstu.
    Lines {
        /// Przesunięcie poziome.
        x: f32,
        /// Przesunięcie pionowe.
        y: f32,
    },
    /// Przewinięcie w pikselach.
    Pixels {
        /// Przesunięcie poziome.
        x: f32,
        /// Przesunięcie pionowe.
        y: f32,
    },
}

/// Zdarzenie myszy w przestrzeni logicznej okna.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseEvent {
    /// Kursor się poruszył.
    CursorMoved {
        /// Nowa pozycja kursora.
        position: Point,
    },
    /// Kursor wszedł w okno.
    CursorEntered,
    /// Kursor opuścił okno.
    CursorLeft,
    /// Przycisk wciśnięty.
    ButtonPressed {
        /// Który przycisk.
        button: MouseButton,
        /// Pozycja w chwili wciśnięcia.
        position: Point,
    },
    /// Przycisk zwolniony.
    ButtonReleased {
        /// Który przycisk.
        button: MouseButton,
        /// Pozycja w chwili zwolnienia.
        position: Point,
    },
    /// Koło przewinięte.
    Wheel {
        /// Wektor przewinięcia.
        delta: ScrollDelta,
    },
}

/// Zdarzenie niezależne od okna (sprzętowe).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum UserEvent {
    /// Brak zdarzenia.
    #[default]
    None,
    /// Zamknięcie okna.
    CloseRequested,
    /// Okno utraciło fokus.
    FocusLost,
    /// Okno odzyskało fokus.
    FocusGained,
    /// Zmiana motywu systemu (light/dark).
    SystemThemeChanged {
        /// `true`, jeśli system przełączył się na jasny motyw.
        light: bool,
    },
    /// Ponowne żądanie przerysowania.
    RedrawRequested,
    /// Zdarzenie użytkownika z innego wątku.
    Custom,
}

/// Zdarzenie przekazywane do `Application::update`.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Zdarzenie klawiatury.
    Keyboard(KeyEvent),
    /// Zdarzenie myszy.
    Mouse(MouseEvent),
    /// Zdaranie systemowe okna.
    Window(UserEvent),
    /// Tekst wklejony ze schowka.
    ClipboardPaste(String),
}

impl From<MouseEvent> for Event {
    fn from(e: MouseEvent) -> Self {
        Self::Mouse(e)
    }
}

impl From<KeyEvent> for Event {
    fn from(e: KeyEvent) -> Self {
        Self::Keyboard(e)
    }
}
