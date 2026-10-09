//! Widget tekstowy.

use crate::context::Context;
use crate::element::{Element, Widget};
use crate::event::Event;
use crate::geometry::{Length, Point, Rect, Size};
use crate::layout::{Cursor, EventStatus, Interaction, Layout, Limits};
use crate::renderer::paint::{TextAlign, TextStyle};
use crate::theme::Theme;
use crate::tree::Tree;

/// Tekst z ustawieniami motywu.
#[derive(Debug, Clone)]
pub struct Text {
    content: String,
    style: Option<TextStyle>,
    width: Length,
    size: f32,
    line_height: f32,
    weight: Option<u16>,
    align: TextAlign,
    color: Option<crate::geometry::Color>,
}

impl Text {
    /// Nowy widget tekstu.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: None,
            width: Length::Shrink,
            size: f32::NAN,
            line_height: f32::NAN,
            weight: None,
            align: TextAlign::Start,
            color: None,
        }
    }

    /// Ustawia rozmiar czcionki (nadpisuje motyw).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Ustawia wysokość linii (nadpisuje motyw).
    pub fn line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    /// Ustawia grubość czcionki (nadpisuje motyw).
    pub fn weight(mut self, weight: u16) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Ustawia kolor tekstu (nadpisuje motyw).
    pub fn color(mut self, color: crate::geometry::Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Ustawia wyrównanie poziome.
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Ustawia szerokość bloku tekstu.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Ustawia pełny styl (nadpisuje wszystkie ustawienia motywu).
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Buduje element.
    pub fn build(self) -> Element {
        Element::new(self)
    }

    /// Rozwiązuje styl względem motywu.
    fn resolve_style(&self, theme: &Theme) -> TextStyle {
        let mut style = self
            .style
            .clone()
            .unwrap_or_else(|| TextStyle::sized(theme.typography.size));
        style.line_height = if self.line_height.is_finite() {
            self.line_height
        } else if self.size.is_finite() {
            self.size * 1.4
        } else {
            theme.typography.line_height
        };
        if self.size.is_finite() {
            style.size = self.size;
        }
        style.weight = self.weight.unwrap_or(theme.typography.weight);
        if let Some(color) = self.color {
            style.color = color;
        }
        style.align = self.align;
        style
    }
}

impl From<&str> for Text {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Text {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl Widget for Text {
    fn layout(&self, _tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        let style = self.resolve_style(ctx.theme);
        let max_width = match self.width {
            Length::Fill | Length::FillPortion(_) => Some(limits.max.width),
            _ => None,
        };
        let metrics = ctx.renderer.measure(&self.content, &style, max_width);
        let width = self
            .width
            .resolve(limits.max.width, metrics.size.width)
            .min(limits.max.width)
            .max(metrics.size.width);
        let size = Size::new(width, metrics.size.height);
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn draw(&self, _tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        let style = self.resolve_style(ctx.theme);
        let aligned = match self.align {
            TextAlign::Start => layout.bounds,
            TextAlign::Center => Rect::new(layout.bounds.position, layout.bounds.size),
            TextAlign::End => layout.bounds,
        };
        ctx.renderer.draw_text(&self.content, aligned, &style);
    }

    fn event(
        &self,
        _tree: &Tree,
        _event: &Event,
        _ctx: &mut Context<'_>,
        _layout: &Layout,
    ) -> EventStatus {
        EventStatus::Ignored
    }

    fn cursor(&self, _tree: &Tree, _layout: &Layout, _cursor: Cursor) -> Interaction {
        Interaction::Idle
    }
}

/// Skrót tworzący widget tekstowy.
pub fn text(content: impl Into<String>) -> Text {
    Text::new(content)
}
