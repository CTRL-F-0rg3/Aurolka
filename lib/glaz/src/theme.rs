//! Motyw wizualny: paleta, typografia i parametry powierzchni.
//!
//! Motyw jest zwykłą wartością przekazywaną przez kontekst rysowania, więc nie
//! ma globalnego stanu i testy są deterministyczne.

use crate::geometry::{Color, Radius};
use crate::transparency::{Backdrop, Transparency};

/// Paleta kolorów motywu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Kolor tła powierzchni aplikacji.
    pub background: Color,
    /// Tło elementów wstępnie podkreślonych (panele, paski).
    pub surface: Color,
    /// Tło elementów w stanie hover.
    pub surface_hover: Color,
    /// Tło elementów aktywnych (przycisk wciśnięty / focus).
    pub surface_active: Color,
    /// Kolor obramowań.
    pub border: Color,
    /// Kolor tekstu głównego.
    pub text: Color,
    /// Kolor tekstu pomocniczego.
    pub text_muted: Color,
    /// Kolor akcentu (focus, aktywne elementy).
    pub accent: Color,
    /// Kolor tekstu na akcencie.
    pub on_accent: Color,
    /// Kolor tła titlebara własnych dekoracji.
    pub titlebar: Color,
    /// Kolor ikony zamknięcia w stanie hover.
    pub danger: Color,
}

impl Palette {
    /// Ciemna paleta (domyślna).
    pub const fn dark() -> Self {
        Self {
            background: Color::rgba(0.06, 0.06, 0.08, 1.0),
            surface: Color::rgba(1.0, 1.0, 1.0, 0.06),
            surface_hover: Color::rgba(1.0, 1.0, 1.0, 0.10),
            surface_active: Color::rgba(1.0, 1.0, 1.0, 0.16),
            border: Color::rgba(1.0, 1.0, 1.0, 0.10),
            text: Color::rgba(0.96, 0.96, 0.98, 1.0),
            text_muted: Color::rgba(0.70, 0.70, 0.76, 1.0),
            accent: Color::rgba(0.40, 0.64, 1.0, 1.0),
            on_accent: Color::rgba(0.02, 0.02, 0.04, 1.0),
            titlebar: Color::rgba(1.0, 1.0, 1.0, 0.04),
            danger: Color::rgba(0.86, 0.22, 0.22, 1.0),
        }
    }

    /// Jasna paleta.
    pub const fn light() -> Self {
        Self {
            background: Color::rgba(0.97, 0.97, 0.98, 1.0),
            surface: Color::rgba(0.0, 0.0, 0.0, 0.05),
            surface_hover: Color::rgba(0.0, 0.0, 0.0, 0.09),
            surface_active: Color::rgba(0.0, 0.0, 0.0, 0.14),
            border: Color::rgba(0.0, 0.0, 0.0, 0.12),
            text: Color::rgba(0.08, 0.08, 0.10, 1.0),
            text_muted: Color::rgba(0.38, 0.38, 0.44, 1.0),
            accent: Color::rgba(0.18, 0.42, 0.90, 1.0),
            on_accent: Color::WHITE,
            titlebar: Color::rgba(0.0, 0.0, 0.0, 0.03),
            danger: Color::rgba(0.80, 0.16, 0.16, 1.0),
        }
    }

    /// Paleta dopasowana do przezroczystego tła okna.
    ///
    /// Przy przezroczystości kolory powierzchni muszą być półprzezroczyste,
    /// inaczej okno wygląda jak szary kafel zamiast jak szyba.
    pub fn for_transparency(&self, transparency: Transparency) -> Self {
        match transparency {
            Transparency::Opaque => *self,
            Transparency::Alpha => Self {
                surface: self.surface.multiply_alpha(0.55),
                titlebar: self.titlebar.multiply_alpha(0.55),
                ..*self
            },
            Transparency::Blur(backdrop) => Self {
                surface: tint_for_backdrop(self.surface, &backdrop),
                titlebar: tint_for_backdrop(self.titlebar, &backdrop),
                ..*self
            },
        }
    }
}

fn tint_for_backdrop(color: Color, backdrop: &Backdrop) -> Color {
    // Elementy na rozmyciu dostają kroplę barwy podkładu, dzięki czemu
    // zachowują czytelność niezależnie od tapety.
    color.overlay(backdrop.tint, 0.25)
}

/// Skala typograficzna.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typography {
    /// Rozmiar czcionki w pikselach logicznych.
    pub size: f32,
    /// Wysokość linii.
    pub line_height: f32,
    /// Grubość czcionki (`400` = regular, `600` = semibold).
    pub weight: u16,
}

impl Default for Typography {
    fn default() -> Self {
        Self::body()
    }
}

impl Typography {
    /// Tekst podstawowy (14 px / 400).
    pub const fn body() -> Self {
        Self {
            size: 14.0,
            line_height: 20.0,
            weight: 400,
        }
    }

    /// Mały tekst pomocniczy (12 px).
    pub const fn caption() -> Self {
        Self {
            size: 12.0,
            line_height: 16.0,
            weight: 400,
        }
    }

    /// Nagłówek sekcji (18 px / 600).
    pub const fn title() -> Self {
        Self {
            size: 18.0,
            line_height: 24.0,
            weight: 600,
        }
    }

    /// Duży nagłówek (24 px / 600).
    pub const fn heading() -> Self {
        Self {
            size: 24.0,
            line_height: 32.0,
            weight: 600,
        }
    }

    /// Rozmiar w punktach typograficznych (przeliczony na `size * 4/3`).
    pub fn points(points: f32) -> Self {
        Self {
            size: points * 4.0 / 3.0,
            ..Self::body()
        }
    }
}

/// Kompletny motyw.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Paleta barw.
    pub palette: Palette,
    /// Typografia podstawowa.
    pub typography: Typography,
    /// Promień zaokrąglenia elementów.
    pub radius: Radius,
    /// Promień zaokrąglenia przycisków.
    pub button_radius: Radius,
    /// Odstęp bazowy między elementami.
    pub spacing: f32,
    /// Ciężar cienia w punktach (`0.0` = brak cienia).
    pub shadow: f32,
    /// Ustawienia przezroczystości okna.
    pub transparency: Transparency,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// Ciemny motyw.
    pub fn dark() -> Self {
        Self {
            palette: Palette::dark(),
            typography: Typography::body(),
            radius: Radius::uniform(10.0),
            button_radius: Radius::uniform(8.0),
            spacing: 8.0,
            shadow: 0.0,
            transparency: Transparency::Opaque,
        }
    }

    /// Jasny motyw.
    pub fn light() -> Self {
        Self {
            palette: Palette::light(),
            ..Self::dark()
        }
    }

    /// Zwraca kopię motywu dopasowaną do przezroczystości okna.
    pub fn with_transparency(&self, transparency: Transparency) -> Self {
        Self {
            palette: self.palette.for_transparency(transparency),
            transparency,
            ..self.clone()
        }
    }

    /// Wyłącza wszelkie cienie.
    pub fn without_shadows(mut self) -> Self {
        self.shadow = 0.0;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparency_dampens_surfaces() {
        let p = Palette::dark();
        let plain = p.for_transparency(Transparency::Opaque);
        let glass = p.for_transparency(Transparency::Alpha);
        assert!(glass.surface.a < plain.surface.a);
        assert_eq!(glass.text, plain.text);
    }

    #[test]
    fn blur_tints_surfaces() {
        let p = Palette::dark();
        let tinted = p.for_transparency(Transparency::Blur(Backdrop::acrylic(Color::WHITE)));
        assert!(tinted.surface.r >= p.surface.r);
    }

    #[test]
    fn theme_with_transparency_keeps_geometry() {
        let t = Theme::dark();
        let g = t.with_transparency(Transparency::Alpha);
        assert_eq!(g.radius, t.radius);
        assert_eq!(g.spacing, t.spacing);
    }

    #[test]
    fn typography_points_scaling() {
        assert!((Typography::points(12.0).size - 16.0).abs() < 0.001);
    }
}
