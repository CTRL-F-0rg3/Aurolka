//! Ograniczenia i wyniki układania elementów.
//!
//! Model jest celowo prosty i przewidywalny (styl *flexbox*, ale bez zagnieżdżonych
//! reguł): każdy widget dostaje [`Limits`] i zwraca `(Size, Layout)`. Dzięki temu
//! pisanie własnych widgetów jest przewidywalne i stabilne.

use crate::geometry::{Padding, Point, Rect, Size};

/// Ograniczenia rozmiaru przekazywane widgetowi podczas układania.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Minimalny rozmiar.
    pub min: Size,
    /// Maksymalny rozmiar.
    pub max: Size,
}

impl Limits {
    /// Nowe ograniczenia z podanym maksimum.
    pub fn none(max: Size) -> Self {
        Self {
            min: Size::ZERO,
            max,
        }
    }

    /// Ograniczenia z zadanym zakresem.
    pub fn loose(min: Size, max: Size) -> Self {
        Self { min, max }
    }

    /// Ograniczenia do dokładnego rozmiaru.
    pub fn tight(size: Size) -> Self {
        Self {
            min: size,
            max: size,
        }
    }

    /// Zawęża limity o zadany rozmiar.
    pub fn shrink(&self, amount: Size) -> Self {
        Self {
            min: Size::new(
                (self.min.width - amount.width).max(0.0),
                (self.min.height - amount.height).max(0.0),
            ),
            max: Size::new(
                (self.max.width - amount.width).max(0.0),
                (self.max.height - amount.height).max(0.0),
            ),
        }
    }

    /// Rozszerza limity o zadany rozmiar.
    pub fn expand(&self, amount: Size) -> Self {
        Self {
            min: self.min + amount,
            max: self.max + amount,
        }
    }

    /// Przycina limity do maksymalnej szerokości.
    pub fn max_width(mut self, width: f32) -> Self {
        self.max.width = self.max.width.min(width);
        self.min.width = self.min.width.min(self.max.width);
        self
    }

    /// Przycina limity do maksymalnej wysokości.
    pub fn max_height(mut self, height: f32) -> Self {
        self.max.height = self.max.height.min(height);
        self.min.height = self.min.height.min(self.max.height);
        self
    }

    /// Ogranicza limity o padding (mniejsze maksymalne i minimalne wymiary).
    pub fn pad(&self, padding: Padding) -> Self {
        Self {
            min: self.min.shrink(Size::new(
                padding.horizontal_total(),
                padding.vertical_total(),
            )),
            max: self.max.shrink(Size::new(
                padding.horizontal_total(),
                padding.vertical_total(),
            )),
        }
    }
}

/// Interakcja zgłaszana przez widget pod kursorem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interaction {
    /// Brak interakcji — element nie reaguje na kursor.
    #[default]
    None,
    /// Element nieaktywny (kursor przechodzi, ale nie reaguje).
    Idle,
    /// Kursor nad elementem klikalnym.
    Clickable,
    /// Kursor nad elementem przeciąganym.
    Grabbed,
}

/// Wynik obsługi zdarzenia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventStatus {
    /// Zdarzenie nie zostało obsłużone — propaguj dalej.
    #[default]
    Ignored,
    /// Zdarzenie zostało obsłużone (blokuje propagację).
    Handled,
}

impl EventStatus {
    /// Czy zdarzenie zostało obsłużone.
    pub fn is_handled(&self) -> bool {
        matches!(self, Self::Handled)
    }

    /// Scala statusy — pierwszy obsłużony wygrywa.
    pub fn merge(self, other: Self) -> Self {
        if self.is_handled() {
            self
        } else {
            other
        }
    }
}

/// Rezultat układania: pozycja, obszar przycięcia i kolejność renderowania.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// Obszar zajmowany przez widget (bez marginesów).
    pub bounds: Rect,
    /// Obszar, w którym widget może być rysowany (obszar przycięcia).
    pub clip: Rect,
    /// Kolejność renderowania — większa wartość rysowana później.
    pub z: f32,
    /// Interakcja zgłaszana dla tego widgetu.
    pub interaction: Interaction,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            bounds: Rect::ZERO,
            clip: Rect::ZERO,
            z: 0.0,
            interaction: Interaction::None,
        }
    }
}

impl Layout {
    /// Nowy layout na podstawie prostokąta.
    pub fn new(bounds: Rect) -> Self {
        Self {
            clip: bounds,
            bounds,
            ..Self::default()
        }
    }

    /// Ustawia obszar przycięcia.
    pub fn with_clip(mut self, clip: Rect) -> Self {
        self.clip = clip;
        self
    }

    /// Ustawia kolejność renderowania.
    pub fn with_z(mut self, z: f32) -> Self {
        self.z = z;
        self
    }

    /// Ustawia interakcję.
    pub fn with_interaction(mut self, interaction: Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    /// Przesuwa layout o wektor.
    pub fn translate(&self, offset: Point) -> Self {
        let mut next = *self;
        next.bounds = next.bounds.translate(offset);
        next
    }
}

/// Stan kursora w danej chwili.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cursor {
    /// Aktualna pozycja w przestrzeni okna.
    pub position: Point,
    /// Czy kursor znajduje się nad oknem.
    pub is_over: bool,
}

impl Cursor {
    /// Nowy stan kursora.
    pub fn new(position: Point, is_over: bool) -> Self {
        Self { position, is_over }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_shrink_never_negative() {
        let l = Limits::tight(Size::new(10.0, 10.0)).shrink(Size::new(20.0, 20.0));
        assert_eq!(l.min, Size::ZERO);
        assert_eq!(l.max, Size::ZERO);
    }

    #[test]
    fn limits_pad_symmetrically() {
        let l = Limits::none(Size::new(100.0, 100.0)).pad(Padding::all(10.0));
        assert_eq!(l.max, Size::new(80.0, 80.0));
    }

    #[test]
    fn event_status_merge_prefers_handled() {
        assert_eq!(
            EventStatus::Ignored.merge(EventStatus::Handled),
            EventStatus::Handled
        );
        assert_eq!(
            EventStatus::Handled.merge(EventStatus::Ignored),
            EventStatus::Handled
        );
    }

    #[test]
    fn layout_clip_defaults_to_bounds() {
        let l = Layout::new(Rect::new(Point::new(1.0, 2.0), Size::new(3.0, 4.0)));
        assert_eq!(l.clip, l.bounds);
    }
}
