//! Własne dekoracje okna: strefy przeciągania, zmiany rozmiaru i przyciski.
//!
//! Zamiast udawać, że system zna nasz titlebar, biblioteka utrzymuje listę
//! [`ChromeArea`] przebudowywaną przy każdym układaniu drzewa widgetów.
//! Widgety takie jak `crate::widget::titlebar` lub `crate::widget::window_button`
//! dopisują swoje obszary, a [`Chrome::hit_test`] rozstrzyga, co jest pod kursorem.
//!
//! Kolejność ma znaczenie: obszary sprawdzane są **od końca do początku**, więc
//! dziecko (np. przycisk zamknięcia) ma pierwszeństwo przed rodzicem (pasek
//! tytułowy). Dzięki temu nie trzeba „wykluczać” przycisków z obszaru drag.

use crate::geometry::{Point, Rect};

/// Polecenie zarządzane oknem, wywoływane po kliknięciu w obszar chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowCommand {
    /// Zminimalizuj okno.
    Minimize,
    /// Przełącz między maksymalizacją a przywracaniem.
    ToggleMaximize,
    /// Przywróć okno do poprzedniego rozmiaru.
    Restore,
    /// Zamknij okno.
    Close,
    /// Pokaż menu systemowe (menu okna).
    ShowSystemMenu,
}

/// Kierunek zmiany rozmiaru okna.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResizeDirection {
    /// Góra.
    North,
    /// Dół.
    South,
    /// Lewo.
    West,
    /// Prawo.
    East,
    /// Góra-prawo.
    NorthEast,
    /// Góra-lewo.
    NorthWest,
    /// Dół-prawo.
    SouthEast,
    /// Dół-lewo.
    SouthWest,
}

impl ResizeDirection {
    /// Wariant kursora odpowiadający temu kierunkowi.
    pub fn cursor(self) -> CursorIcon {
        match self {
            Self::North => CursorIcon::ResizeNorth,
            Self::South => CursorIcon::ResizeSouth,
            Self::West => CursorIcon::ResizeWest,
            Self::East => CursorIcon::ResizeEast,
            Self::NorthEast => CursorIcon::ResizeNorthEast,
            Self::NorthWest => CursorIcon::ResizeNorthWest,
            Self::SouthEast => CursorIcon::ResizeSouthEast,
            Self::SouthWest => CursorIcon::ResizeSouthWest,
        }
    }
}

/// Kursor systemowy (własny typ, aby API nie zależało od `winit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CursorIcon {
    /// Domyślny strzałek.
    #[default]
    Default,
    /// Wskazanie (klikalne).
    Pointer,
    /// I-beam (edytowanie tekstu).
    Text,
    /// Chwytanie.
    Grab,
    /// Aktywne chwytanie.
    Grabbing,
    /// Zmiana rozmiaru — góra.
    ResizeNorth,
    /// Zmiana rozmiaru — dół.
    ResizeSouth,
    /// Zmiana rozmiaru — lewo.
    ResizeWest,
    /// Zmiana rozmiaru — prawo.
    ResizeEast,
    /// Zmiana rozmiaru — góra-prawo.
    ResizeNorthEast,
    /// Zmiana rozmiaru — góra-lewo.
    ResizeNorthWest,
    /// Zmiana rozmiaru — dół-prawo.
    ResizeSouthEast,
    /// Zmiana rozmiaru — dół-lewo.
    ResizeSouthWest,
    /// Przeciąganie.
    Move,
    /// Niedozwolone.
    NotAllowed,
    /// Oczekiwanie.
    Wait,
    /// Pomoc.
    Help,
    /// Krzyżyk (precyzyjne wskazanie).
    Crosshair,
}

/// Co należy zrobić po kliknięciu w obszar dekoracji.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChromeAction {
    /// Obszar przeciąga okno (drag).
    Drag,
    /// Obszar zmienia rozmiar okna.
    Resize(ResizeDirection),
    /// Obszar wywołuje polecenie okna.
    Command(WindowCommand),
    /// Obszar jest „dziurą” — kliknięcie przechodzi do widgetów poniżej,
    /// a nie jest traktowane jako drag.
    Interactive,
}

/// Pojedynczy obszar dekoracji.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromeArea {
    /// Obszar w przestrzeni okna (piksele logiczne).
    pub rect: Rect,
    /// Działanie po kliknięciu.
    pub action: ChromeAction,
    /// Kursor wyświetlany nad obszarem.
    pub cursor: Option<CursorIcon>,
}

impl ChromeArea {
    /// Nowy obszar z domyślnym kursorem dobranym do działania.
    pub fn new(rect: Rect, action: ChromeAction) -> Self {
        let cursor = match action {
            ChromeAction::Drag => Some(CursorIcon::Move),
            ChromeAction::Resize(dir) => Some(dir.cursor()),
            ChromeAction::Command(_) | ChromeAction::Interactive => Some(CursorIcon::Pointer),
        };
        Self {
            rect,
            action,
            cursor,
        }
    }

    /// Ustawia kursor jawnie.
    pub fn with_cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Czy punkt leży w tym obszarze.
    pub fn hit(&self, point: Point) -> bool {
        self.rect.contains(point)
    }
}

/// Kolektor obszarów dekoracji dla jednego okna.
///
/// Powstaje od zera w każdej klatce podczas układania drzewa widgetów.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chrome {
    areas: Vec<ChromeArea>,
}

impl Chrome {
    /// Pusty kolektor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Dopisuje obszar na końcu (najwyższy priorytet przy hit-testowaniu).
    pub fn push(&mut self, area: ChromeArea) {
        self.areas.push(area);
    }

    /// Dopisuje obszar z akcją, dobierając kursor automatycznie.
    pub fn push_area(&mut self, rect: Rect, action: ChromeAction) {
        self.push(ChromeArea::new(rect, action));
    }

    /// Lista obszarów w kolejności rejestracji.
    pub fn areas(&self) -> &[ChromeArea] {
        &self.areas
    }

    /// Czy kolektor nie zawiera żadnych obszarów.
    pub fn is_empty(&self) -> bool {
        self.areas.is_empty()
    }

    /// Czyści kolektor (wywoływane przed układaniem drzewa).
    pub fn clear(&mut self) {
        self.areas.clear();
    }

    /// Zwraca działanie dla punktu — ostatni dopasowany obszar wygrywa.
    pub fn hit_test(&self, point: Point) -> Option<ChromeAction> {
        self.areas
            .iter()
            .rev()
            .find(|area| area.hit(point))
            .map(|area| area.action)
    }

    /// Zwraca kursor dla punktu.
    pub fn cursor_at(&self, point: Point) -> Option<CursorIcon> {
        self.areas
            .iter()
            .rev()
            .find(|area| area.hit(point))
            .and_then(|area| area.cursor)
    }

    /// Czy punkt leży w obszarze przeciągania okna.
    pub fn is_drag_region(&self, point: Point) -> bool {
        matches!(self.hit_test(point), Some(ChromeAction::Drag))
    }
}

/// Wyznacza kierunek zmiany rozmiaru na podstawie pozycji kursora względem
/// krawędzi okna.
///
/// Zwraca `None`, gdy kursor jest daleko od krawędzi albo okno nie jest
/// zmienialne. `border_width` to grubość aktywnej strefy w pikselach logicznych.
pub fn resize_direction_at(
    point: Point,
    bounds: Rect,
    border_width: f32,
    resizable: bool,
) -> Option<ResizeDirection> {
    if !resizable || border_width <= 0.0 {
        return None;
    }

    let near_left = point.x <= bounds.left() + border_width;
    let near_right = point.x >= bounds.right() - border_width;

    let near_top = point.y <= bounds.top() + border_width;
    let near_bottom = point.y >= bounds.bottom() - border_width;

    // Środek okna to decyzja UI, nie okna.
    if !near_left && !near_right && !near_top && !near_bottom {
        return None;
    }
    // Przy bardzo małych oknach narożniki „zjadłyby" cały obszar.
    if bounds.size.width <= border_width * 2.0 || bounds.size.height <= border_width * 2.0 {
        return None;
    }

    match (near_left, near_right, near_top, near_bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Size;

    /// Okno testowe 400×300.
    fn rect() -> Rect {
        Rect::new(Point::ZERO, Size::new(400.0, 300.0))
    }

    #[test]
    fn later_area_wins_over_earlier() {
        let mut chrome = Chrome::new();
        chrome.push_area(rect(), ChromeAction::Drag);
        chrome.push_area(
            Rect::new(Point::new(350.0, 0.0), Size::new(50.0, 30.0)),
            ChromeAction::Command(WindowCommand::Close),
        );

        assert_eq!(
            chrome.hit_test(Point::new(375.0, 15.0)),
            Some(ChromeAction::Command(WindowCommand::Close))
        );
        assert_eq!(
            chrome.hit_test(Point::new(100.0, 15.0)),
            Some(ChromeAction::Drag)
        );
    }

    #[test]
    fn interactive_punches_hole_in_drag_region() {
        let mut chrome = Chrome::new();
        chrome.push_area(rect(), ChromeAction::Drag);
        chrome.push_area(
            Rect::new(Point::new(10.0, 10.0), Size::new(20.0, 20.0)),
            ChromeAction::Interactive,
        );

        assert_eq!(
            chrome.hit_test(Point::new(15.0, 15.0)),
            Some(ChromeAction::Interactive)
        );
        assert!(!chrome.is_drag_region(Point::new(15.0, 15.0)));
    }

    #[test]
    fn edges_and_corners_resolve() {
        assert_eq!(
            resize_direction_at(Point::new(2.0, 150.0), rect(), 8.0, true),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            resize_direction_at(Point::new(2.0, 2.0), rect(), 8.0, true),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction_at(Point::new(200.0, 150.0), rect(), 8.0, true),
            None
        );
    }

    #[test]
    fn non_resizable_window_has_no_resize_zones() {
        assert_eq!(
            resize_direction_at(Point::new(1.0, 1.0), rect(), 8.0, false),
            None
        );
    }

    #[test]
    fn tiny_window_has_no_resize_zones() {
        let tiny = Rect::new(Point::ZERO, Size::new(10.0, 10.0));
        assert_eq!(
            resize_direction_at(Point::new(1.0, 1.0), tiny, 8.0, true),
            None
        );
    }

    #[test]
    fn cursor_defaults_follow_action() {
        let area = ChromeArea::new(rect(), ChromeAction::Resize(ResizeDirection::SouthWest));
        assert_eq!(area.cursor, Some(CursorIcon::ResizeSouthWest));
    }
}
