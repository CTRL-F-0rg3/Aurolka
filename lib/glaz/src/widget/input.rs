//! Kontrolki wejściowe: przycisk, checkbox, suwak.

use crate::context::Context;
use crate::element::{Element, Widget};
use crate::event::{Event, MouseButton, MouseEvent, ScrollDelta};
use crate::geometry::{Color, Padding, Point, Radius, Rect, Size};
use crate::layout::{Cursor, EventStatus, Interaction, Layout, Limits};
use crate::message::Message;
use crate::renderer::paint::{Paint, TextAlign, TextStyle};
use crate::theme::Theme;
use crate::tree::Tree;

/// Fabryka wiadomości wysyłanej po kliknięciu.
type ClickFactory = Box<dyn Fn() -> Message + Send + Sync>;

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label)
            .field("accent", &self.accent)
            .field("tooltip", &self.tooltip)
            .finish_non_exhaustive()
    }
}

/// Przycisk z etykietą.
pub struct Button {
    label: String,
    on_click: Option<ClickFactory>,
    accent: bool,
    min_width: Option<f32>,
    padding: crate::geometry::Padding,
    radius: Option<crate::geometry::Radius>,
    tooltip: Option<String>,
}

impl Button {
    /// Nowy przycisk z etykietą.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            on_click: None,
            accent: false,
            min_width: None,
            padding: crate::geometry::Padding::symmetric(14.0, 8.0),
            radius: None,
            tooltip: None,
        }
    }

    /// Reakcja na kliknięcie — fabryka wiadomości.
    ///
    /// ```ignore
    /// widget::button("Wyślij").on_press(|| Message::Custom(Box::new(SendRequested)))
    /// ```
    ///
    /// Wiadomość jest **tworzona przy każdym kliknięciu**, dzięki czemu
    /// `Message::Custom` (z danymi, np. z modelu AI) działa bez klonowania.
    pub fn on_press<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> Message + Send + Sync + 'static,
    {
        self.on_click = Some(Box::new(factory));
        self
    }

    /// Styl akcentowany (kolor motywu).
    pub fn accent(mut self, accent: bool) -> Self {
        self.accent = accent;
        self
    }

    /// Minimalna szerokość.
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = Some(width);
        self
    }

    /// Buduje element.
    pub fn build(self) -> Element {
        Element::new(self)
    }

    /// Margines wewnętrzny.
    pub fn padding(mut self, padding: crate::geometry::Padding) -> Self {
        self.padding = padding;
        self
    }

    /// Promienie narożników (nadpisują `theme.button_radius`).
    pub fn radius(mut self, radius: crate::geometry::Radius) -> Self {
        self.radius = Some(radius);
        self
    }

    /// Podpowiedź (opis funkcji) — dostępna czytnikom ekranu.
    ///
    /// Pole nie jest rysowane: biblioteka celowo nie ma warstwy tooltipów,
    /// ale atrybut jest semantycznie potrzebny i używany przez `Debug`.
    pub fn tooltip(mut self, hint: impl Into<String>) -> Self {
        self.tooltip = Some(hint.into());
        self
    }

    fn style(&self, theme: &Theme, hovered: bool, pressed: bool) -> (Color, Color) {
        let palette = &theme.palette;
        if self.accent {
            let base = palette.accent;
            let text = palette.on_accent;
            if pressed {
                (base.multiply_alpha(0.75), text)
            } else if hovered {
                (base.multiply_alpha(0.88), text)
            } else {
                (base, text)
            }
        } else {
            let background = if pressed {
                palette.surface_active
            } else if hovered {
                palette.surface_hover
            } else {
                palette.surface
            };
            (background, palette.text)
        }
    }
}

impl Widget for Button {
    fn layout(&self, tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        let style = TextStyle {
            size: ctx.theme.typography.size,
            line_height: ctx.theme.typography.line_height,
            weight: ctx.theme.typography.weight,
            color: Color::WHITE,
            family: None,
            align: TextAlign::Start,
        };
        let metrics = ctx.renderer.measure(&self.label, &style, None);
        let min = self.min_width.unwrap_or(0.0);
        let width = (metrics.size.width + self.padding.horizontal_total() + self.border_extra())
            .max(min)
            .min(limits.max.width);
        let height = (metrics.size.height + self.padding.vertical_total()).min(limits.max.height);
        let size = Size::new(width, height);
        let _ = tree;
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        let state = button_state(tree);
        let (background, foreground) = self.style(
            ctx.theme,
            state == ButtonState::Hovered,
            state == ButtonState::Pressed,
        );

        let radius = self.radius.unwrap_or(ctx.theme.button_radius);
        let paint = Paint::rounded(layout.bounds, radius, background).with_clip(layout.clip);
        ctx.renderer.draw_quad(&paint);

        let style = TextStyle {
            size: ctx.theme.typography.size,
            line_height: ctx.theme.typography.line_height,
            weight: ctx.theme.typography.weight,
            color: foreground,
            family: None,
            align: TextAlign::Center,
        };
        ctx.renderer.draw_text(&self.label, layout.bounds, &style);
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        let over = ctx.cursor_over(layout.bounds);
        match event {
            Event::Mouse(MouseEvent::ButtonPressed { button, position }) => {
                if *button == MouseButton::Left && layout.bounds.contains(*position) {
                    tree.state
                        .update::<ButtonState>(|state| *state = ButtonState::Pressed);
                    return EventStatus::Handled;
                }
            }
            Event::Mouse(MouseEvent::ButtonReleased { button, position }) => {
                if *button != MouseButton::Left {
                    return EventStatus::Ignored;
                }
                let was_pressed = button_state(tree) == ButtonState::Pressed;
                tree.state
                    .update::<ButtonState>(|state| *state = ButtonState::None);
                if was_pressed && layout.bounds.contains(*position) {
                    if let Some(factory) = &self.on_click {
                        ctx.send(factory());
                    }
                    return EventStatus::Handled;
                }
                return EventStatus::Handled;
            }
            Event::Mouse(MouseEvent::CursorLeft) => {
                tree.state
                    .update::<ButtonState>(|state| *state = ButtonState::None);
            }
            _ => {}
        }

        if !over {
            tree.state
                .update::<ButtonState>(|state| *state = ButtonState::None);
        }
        EventStatus::Ignored
    }

    fn cursor(&self, tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        if !cursor.is_over || !layout.bounds.contains(cursor.position) {
            return Interaction::None;
        }
        if button_state(tree) == ButtonState::None {
            tree.state
                .update::<ButtonState>(|stored| *stored = ButtonState::Hovered);
        }
        Interaction::Clickable
    }
}

impl Button {
    fn border_extra(&self) -> f32 {
        0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ButtonState {
    #[default]
    None,
    Hovered,
    Pressed,
}

fn button_state(tree: &Tree) -> ButtonState {
    tree.state.get::<ButtonState>().unwrap_or(ButtonState::None)
}

/// Skrót tworzący przycisk.
pub fn button(label: impl Into<String>) -> Button {
    Button::new(label)
}

/// Pole wyboru (checkbox).
pub struct Checkbox {
    label: String,
    value: bool,
    on_toggle: Option<Box<dyn Fn(bool) + Send + Sync>>,
}

impl Checkbox {
    /// Nowy checkbox.
    pub fn new(label: impl Into<String>, value: bool) -> Self {
        Self {
            label: label.into(),
            value,
            on_toggle: None,
        }
    }

    /// Reakcja na zmianę wartości.
    pub fn on_toggle<F>(mut self, handler: F) -> Self
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl Widget for Checkbox {
    fn layout(&self, _tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        let box_size = 18.0_f32.min(limits.max.height);
        let style = checkbox_label_style(ctx);
        let metrics = ctx.renderer.measure(&self.label, &style, None);
        let size = Size::new(
            (box_size + 10.0 + metrics.size.width).min(limits.max.width),
            box_size.max(metrics.size.height).min(limits.max.height),
        );
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        let checked = tree.state.get::<bool>().unwrap_or(self.value);
        let box_size = 18.0_f32.min(layout.bounds.size.height);
        let box_rect = Rect::new(layout.bounds.position, Size::new(box_size, box_size));

        let hovered = ctx.cursor_over(layout.bounds);
        let fill = if checked {
            ctx.theme.palette.accent
        } else if hovered {
            ctx.theme.palette.surface_hover
        } else {
            ctx.theme.palette.surface
        };

        let paint = Paint::rounded(box_rect, Radius::uniform(5.0), fill)
            .with_border(1.0, ctx.theme.palette.border)
            .with_clip(layout.clip);
        ctx.renderer.draw_quad(&paint);

        if checked {
            let inner = box_rect.shrink(Padding::all(5.0));
            let mark = Paint::rounded(inner, Radius::uniform(3.0), ctx.theme.palette.on_accent)
                .with_clip(layout.clip);
            ctx.renderer.draw_quad(&mark);
        }

        let label_rect = Rect::new(
            Point::new(box_rect.right() + 10.0, layout.bounds.position.y),
            Size::new(
                (layout.bounds.size.width - box_size - 10.0).max(0.0),
                layout.bounds.size.height,
            ),
        );
        ctx.renderer
            .draw_text(&self.label, label_rect, &checkbox_label_style(ctx));
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        let Event::Mouse(MouseEvent::ButtonReleased { button, position }) = event else {
            return EventStatus::Ignored;
        };
        if *button != MouseButton::Left || !layout.bounds.contains(*position) {
            return EventStatus::Ignored;
        }
        let current = tree.state.get::<bool>().unwrap_or(self.value);
        let next = !current;
        tree.state.set(next);
        if let Some(handler) = &self.on_toggle {
            handler(next);
        }
        let _ = ctx;
        EventStatus::Handled
    }

    fn cursor(&self, _tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        if cursor.is_over && layout.bounds.contains(cursor.position) {
            Interaction::Clickable
        } else {
            Interaction::None
        }
    }
}

/// Skrót tworzący checkbox.
pub fn checkbox(label: impl Into<String>, value: bool) -> Checkbox {
    Checkbox::new(label, value)
}

fn checkbox_label_style(ctx: &Context<'_>) -> TextStyle {
    TextStyle {
        size: ctx.theme.typography.size,
        line_height: ctx.theme.typography.line_height,
        weight: ctx.theme.typography.weight,
        color: ctx.theme.palette.text,
        family: None,
        align: TextAlign::Start,
    }
}

impl std::fmt::Debug for Checkbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkbox")
            .field("label", &self.label)
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

/// Suwak (slider).
pub struct Slider {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    on_change: Option<Box<dyn Fn(f32) + Send + Sync>>,
    dragging: bool,
}

impl Slider {
    /// Nowy suwak w zakresie `0.0..=1.0`.
    pub fn new(value: f32) -> Self {
        Self {
            value,
            min: 0.0,
            max: 1.0,
            step: 0.01,
            on_change: None,
            dragging: false,
        }
    }

    /// Zakres wartości.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// Krok zmiany wartości.
    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    /// Reakcja na zmianę wartości.
    pub fn on_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(f32) + Send + Sync + 'static,
    {
        self.on_change = Some(Box::new(handler));
        self
    }

    fn normalized(&self, value: f32) -> f32 {
        let span = self.max - self.min;
        if span.abs() < f32::EPSILON {
            0.0
        } else {
            ((value - self.min) / span).clamp(0.0, 1.0)
        }
    }

    fn quantize(&self, raw: f32) -> f32 {
        if self.step > 0.0 {
            let steps = ((raw - self.min) / self.step).round();
            (self.min + steps * self.step).clamp(self.min, self.max)
        } else {
            raw.clamp(self.min, self.max)
        }
    }

    fn apply(&self, tree: &Tree, layout: &Layout, position: Point) -> Option<f32> {
        let width = layout.bounds.size.width;
        if width <= 0.0 {
            return None;
        }
        let t = ((position.x - layout.bounds.left()) / width).clamp(0.0, 1.0);
        let next = self.quantize(self.min + t * (self.max - self.min));
        tree.state.set(next);
        Some(next)
    }

    fn emit(&self, next: f32) {
        if let Some(handler) = &self.on_change {
            handler(next);
        }
    }
}

impl Widget for Slider {
    fn layout(&self, _tree: &mut Tree, limits: &Limits, _ctx: &mut Context<'_>) -> (Size, Layout) {
        let height = 20.0_f32.min(limits.max.height);
        let size = Size::new(limits.max.width, height);
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        let value = tree.state.get::<f32>().unwrap_or(self.value);
        let t = self.normalized(value);
        let bounds = layout.bounds;
        let thickness = 4.0;

        let track = Rect::new(
            Point::new(bounds.left(), bounds.center().y - thickness / 2.0),
            Size::new(bounds.size.width, thickness),
        );
        ctx.renderer.draw_quad(
            &Paint::rounded(
                track,
                Radius::uniform(thickness / 2.0),
                ctx.theme.palette.surface_active,
            )
            .with_clip(layout.clip),
        );

        let filled = Rect::new(
            track.position,
            Size::new(track.size.width * t, track.size.height),
        );
        ctx.renderer.draw_quad(
            &Paint::rounded(
                filled,
                Radius::uniform(thickness / 2.0),
                ctx.theme.palette.accent,
            )
            .with_clip(layout.clip),
        );

        let knob_radius = 8.0;
        let knob = Rect::new(
            Point::new(
                bounds.left() + bounds.size.width * t - knob_radius,
                bounds.center().y - knob_radius,
            ),
            Size::new(knob_radius * 2.0, knob_radius * 2.0),
        );
        ctx.renderer.draw_quad(
            &Paint::rounded(knob, Radius::uniform(knob_radius), ctx.theme.palette.accent)
                .with_border(2.0, ctx.theme.palette.background)
                .with_clip(layout.clip),
        );
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        _ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        match event {
            Event::Mouse(MouseEvent::ButtonPressed { button, position }) => {
                if *button != MouseButton::Left || !layout.bounds.contains(*position) {
                    return EventStatus::Ignored;
                }
                if let Some(next) = self.apply(tree, layout, *position) {
                    self.emit(next);
                }
                EventStatus::Handled
            }
            Event::Mouse(MouseEvent::CursorMoved { position }) => {
                if !self.dragging {
                    return EventStatus::Ignored;
                }
                if let Some(next) = self.apply(tree, layout, *position) {
                    self.emit(next);
                }
                EventStatus::Handled
            }
            Event::Mouse(MouseEvent::ButtonReleased { button, .. }) => {
                if *button == MouseButton::Left {
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            Event::Mouse(MouseEvent::Wheel { delta }) => {
                let scroll = match delta {
                    ScrollDelta::Lines { y, .. } => *y,
                    ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                let current = tree.state.get::<f32>().unwrap_or(self.value);
                let next = self.quantize(current + scroll * self.step.max(0.01));
                tree.state.set(next);
                self.emit(next);
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn cursor(&self, _tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        if cursor.is_over && layout.bounds.contains(cursor.position) {
            Interaction::Clickable
        } else {
            Interaction::None
        }
    }
}

impl std::fmt::Debug for Slider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slider")
            .field("value", &self.value)
            .field("range", &(self.min, self.max))
            .finish_non_exhaustive()
    }
}

/// Skrót tworzący suwak.
pub fn slider(value: f32) -> Slider {
    Slider::new(value)
}
