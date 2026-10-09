//! Typy opisujące co i jak narysować.
//!
//! To jedyny „język” komunikacji między widgetami a GPU. Wszystko jest
//! wartościami (`Copy`), dzięki czemu można je swobodnie buforować i testować.

use std::ops::Range;

use crate::geometry::{Color, Point, Radius, Rect, Size};

/// Cień pod elementem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// Przesunięcie cienia względem elementu.
    pub offset: Point,
    /// Promień rozmycia cienia.
    pub blur: f32,
    /// Rozciągnięcie cienia (może być ujemne, by go „ściągnąć”).
    pub spread: f32,
    /// Kolor cienia.
    pub color: Color,
}

impl Shadow {
    /// Nowy cień.
    pub const fn new(offset: Point, blur: f32, spread: f32, color: Color) -> Self {
        Self {
            offset,
            blur,
            spread,
            color,
        }
    }

    /// Czy cień jest widoczny.
    pub fn is_visible(&self) -> bool {
        self.color.a > 0.0 && (self.blur > 0.0 || self.spread != 0.0 || self.offset != Point::ZERO)
    }
}

/// Efekt rozmycia tła pod elementem (backdrop).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackdropPaint {
    /// Promień rozmycia.
    pub radius: f32,
    /// Barwa nasycona na rozmycie, z kanałem alpha jako siłą.
    pub tint: Color,
    /// Mnożnik nasycenia (`1.0` = bez zmian).
    pub saturation: f32,
}

impl Default for BackdropPaint {
    /// Neutralny backdrop: bez rozmycia, nasycenie bez zmian.
    ///
    /// Uwaga na `#[derive(Default)]` — dałoby `saturation: 0.0`, a to
    /// włączałoby efekt, którego użytkownik nie prosił.
    fn default() -> Self {
        Self {
            radius: 0.0,
            tint: Color::TRANSPARENT,
            saturation: 1.0,
        }
    }
}

impl BackdropPaint {
    /// Czy w ogóle coś robić (renderer nie ma co próbkować).
    pub fn is_enabled(&self) -> bool {
        self.radius > 0.0 && (self.tint.a > 0.0 || self.saturation != 1.0)
    }
}

/// Opis pojedynczego prostokąta do narysowania (SDF rounded-rect).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint {
    /// Obszar elementu w pikselach logicznych.
    pub bounds: Rect,
    /// Promienie narożników.
    pub radius: Radius,
    /// Kolor wypełnienia.
    pub fill: Option<Color>,
    /// Kolor obramowania.
    pub border: Option<Color>,
    /// Grubość obramowania.
    pub border_width: f32,
    /// Cień pod elementem.
    pub shadow: Option<Shadow>,
    /// Efekt rozmycia tła pod elementem.
    pub backdrop: BackdropPaint,
    /// Obszar przycięcia (przekraczanie go jest przycinane w shaderze).
    pub clip: Rect,
    /// Kolejność renderowania.
    pub z: f32,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            bounds: Rect::ZERO,
            radius: Radius::zero(),
            fill: None,
            border: None,
            border_width: 0.0,
            shadow: None,
            backdrop: BackdropPaint::default(),
            clip: Rect::ZERO,
            z: 0.0,
        }
    }
}

impl Paint {
    /// Proste wypełnienie prostokątem.
    pub fn fill(bounds: Rect, color: Color) -> Self {
        Self {
            bounds,
            fill: Some(color),
            ..Self::default()
        }
    }

    /// Prostokąt z zaokrąglonymi narożnikami.
    pub fn rounded(bounds: Rect, radius: Radius, color: Color) -> Self {
        Self {
            radius: radius.clamp(bounds.size),
            ..Self::fill(bounds, color)
        }
    }

    /// Ustawia obramowanie o jednolitej grubości.
    pub fn with_border(mut self, width: f32, color: Color) -> Self {
        self.border = Some(color);
        self.border_width = width;
        self
    }

    /// Ustawia cień.
    pub fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = Some(shadow);
        self
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

    /// Ustawia efekt rozmycia tła.
    pub fn with_backdrop(mut self, backdrop: BackdropPaint) -> Self {
        self.backdrop = backdrop;
        self
    }

    /// Czy cokolwiek zostanie narysowane.
    pub fn is_drawable(&self) -> bool {
        self.bounds.size.is_valid()
            && self.bounds.size.width > 0.0
            && self.bounds.size.height > 0.0
            && (self.fill.is_some_and(|c| c.a > 0.0)
                || self.border.is_some_and(|c| c.a > 0.0)
                || self.shadow.is_some_and(|s| s.is_visible())
                || self.backdrop.is_enabled())
    }
}

/// Wyrównanie tekstu w poziomie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    /// Do lewej.
    #[default]
    Start,
    /// Wyśrodkowany.
    Center,
    /// Do prawej.
    End,
}

/// Styl tekstu.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Rozmiar czcionki (piksele logiczne).
    pub size: f32,
    /// Wysokość linii.
    pub line_height: f32,
    /// Grubość czcionki.
    pub weight: u16,
    /// Kolor tekstu.
    pub color: Color,
    /// Opcjonalna nazwa rodziny czcionki.
    pub family: Option<String>,
    /// Wyrównanie poziome.
    pub align: TextAlign,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 14.0,
            line_height: 20.0,
            weight: 400,
            color: Color::WHITE,
            family: None,
            align: TextAlign::Start,
        }
    }
}

impl TextStyle {
    /// Styl o zadanym rozmiarze czcionki.
    pub fn sized(size: f32) -> Self {
        Self {
            size,
            line_height: size * 1.4,
            ..Self::default()
        }
    }

    /// Ustawia kolor.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Ustawia grubość.
    pub fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    /// Ustawia wyrównanie.
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Ustawia rodzinę czcionek.
    pub fn family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(family.into());
        self
    }
}

/// Zmierzona linia tekstu.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    /// Zakres znaków w oryginalnym napisie.
    pub range: Range<usize>,
    /// Szerokość linii.
    pub width: f32,
    /// Wysokość linii.
    pub height: f32,
}

/// Wynik pomiaru tekstu.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMetrics {
    /// Rozmiar potrzebny na cały blok tekstu.
    pub size: Size,
    /// Poszczególne linie.
    pub lines: Vec<TextLine>,
    /// Czy tekst nie zmieścił się w zadanej wysokości.
    pub overflowed: bool,
}

impl Default for TextMetrics {
    fn default() -> Self {
        Self::empty()
    }
}

impl TextMetrics {
    /// Pusty wynik pomiaru.
    pub fn empty() -> Self {
        Self {
            size: Size::ZERO,
            lines: Vec::new(),
            overflowed: false,
        }
    }

    /// Najszersza linia.
    pub fn max_line_width(&self) -> f32 {
        self.lines.iter().map(|l| l.width).fold(0.0, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_with_border_is_drawable() {
        let p = Paint::fill(
            Rect::new(Point::ZERO, Size::new(10.0, 10.0)),
            Color::TRANSPARENT,
        )
        .with_border(2.0, Color::WHITE);
        assert!(p.is_drawable());
    }

    #[test]
    fn empty_paint_is_not_drawable() {
        let p = Paint {
            bounds: Rect::new(Point::ZERO, Size::new(10.0, 10.0)),
            ..Default::default()
        };
        assert!(!p.is_drawable());
    }

    #[test]
    fn zero_size_is_not_drawable() {
        assert!(!Paint::fill(Rect::ZERO, Color::WHITE).is_drawable());
    }

    #[test]
    fn backdrop_disabled_without_tint() {
        assert!(!BackdropPaint {
            radius: 30.0,
            ..Default::default()
        }
        .is_enabled());
        assert!(BackdropPaint {
            radius: 30.0,
            saturation: 1.4,
            ..Default::default()
        }
        .is_enabled());
    }

    #[test]
    fn shadow_visibility() {
        assert!(!Shadow::new(Point::ZERO, 0.0, 0.0, Color::TRANSPARENT).is_visible());
        assert!(Shadow::new(Point::new(0.0, 2.0), 4.0, 0.0, Color::BLACK).is_visible());
    }

    #[test]
    fn text_style_line_height_scales() {
        assert!((TextStyle::sized(20.0).line_height - 28.0).abs() < 0.001);
    }
}
