//! Kontenery: tło z marginesem, odstępy i nakładanie elementów.

use crate::context::Context;
use crate::element::{Element, Widget};
use crate::event::Event;
use crate::geometry::{Align, Color, Length, Padding, Point, Radius, Rect, Size};
use crate::layout::{Cursor, EventStatus, Interaction, Layout, Limits};
use crate::renderer::paint::{BackdropPaint, Paint, Shadow};
use crate::tree::Tree;

/// Kontener: kolor tła, margines wewnętrzny, obramowanie, rozmycie, dziecko.
#[derive(Debug)]
pub struct Container {
    color: Option<Color>,
    radius: Radius,
    padding: Padding,
    border_width: f32,
    border_color: Option<Color>,
    backdrop: Option<BackdropPaint>,
    shadow: f32,
    width: Length,
    height: Length,
    align_x: Align,
    align_y: Align,
    child: Option<Element>,
}

impl Default for Container {
    fn default() -> Self {
        Self {
            color: None,
            radius: Radius::zero(),
            padding: Padding::ZERO,
            border_width: 0.0,
            border_color: None,
            backdrop: None,
            shadow: 0.0,
            width: Length::Fill,
            height: Length::Shrink,
            align_x: Align::Start,
            align_y: Align::Start,
            child: None,
        }
    }
}

impl Container {
    /// Nowy kontener (przezroczysty, bez marginesu).
    pub fn new() -> Self {
        Self::default()
    }

    /// Kolor tła.
    pub fn style(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Promienie narożników.
    pub fn radius(mut self, radius: Radius) -> Self {
        self.radius = radius;
        self
    }

    /// Margines wewnętrzny.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Obramowanie.
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.border_width = width;
        self.border_color = Some(color);
        self
    }

    /// Efekt rozmycia tła pod kontenerem („szkło” / acrylic).
    pub fn backdrop(mut self, backdrop: BackdropPaint) -> Self {
        self.backdrop = Some(backdrop);
        self
    }

    /// Cień (`0.0` = brak).
    pub fn shadow(mut self, shadow: f32) -> Self {
        self.shadow = shadow;
        self
    }

    /// Szerokość kontenera.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Wysokość kontenera.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Wyrównanie zawartości do środka.
    pub fn center_content(mut self) -> Self {
        self.align_x = Align::Center;
        self.align_y = Align::Center;
        self
    }

    /// Wyrównanie zawartości.
    pub fn align_content(mut self, x: Align, y: Align) -> Self {
        self.align_x = x;
        self.align_y = y;
        self
    }

    /// Dodaje dziecko.
    pub fn child(mut self, child: Element) -> Self {
        self.child = Some(child);
        self
    }

    /// Buduje element.
    pub fn build(self) -> Element {
        Element::new(self)
    }

    fn child_layout(&self, tree: &Tree, layout: &Layout) -> Option<Layout> {
        let node = tree.children.first()?;
        let inner = layout.bounds.shrink(self.padding);
        Some(Layout {
            bounds: Rect::new(
                Point::new(
                    inner.position.x
                        + self.align_x.position(
                            0.0,
                            inner.size.width,
                            node.layout.bounds.size.width,
                        ),
                    inner.position.y
                        + self.align_y.position(
                            0.0,
                            inner.size.height,
                            node.layout.bounds.size.height,
                        ),
                ),
                node.layout.bounds.size,
            ),
            clip: layout.clip,
            z: layout.z,
            interaction: Interaction::Idle,
        })
    }
}

impl Widget for Container {
    fn children(&self, tree: &mut Tree) -> Vec<Tree> {
        match &self.child {
            Some(child) => tree.children_from_slice(std::slice::from_ref(child)),
            None => Vec::new(),
        }
    }

    fn layout(&self, tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        let inner = limits.pad(self.padding);
        let content = match (self.child.as_ref(), tree.children.first_mut()) {
            (Some(child), Some(node)) => child.as_widget().layout(node, &inner, ctx).0,
            _ => Size::ZERO,
        };
        let shrink = content
            + Size::new(
                self.padding.horizontal_total() + self.border_width * 2.0,
                self.padding.vertical_total() + self.border_width * 2.0,
            );
        let size = Size::new(
            self.width
                .resolve(limits.max.width, shrink.width)
                .min(limits.max.width),
            self.height
                .resolve(limits.max.height, shrink.height)
                .min(limits.max.height),
        );
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn fills_main_axis(&self) -> bool {
        // Kontener nie ma osi glownej — traktujemy go jako rozciagajacy sie
        // tylko wtedy, gdy wprost podano `Length::Fill`.
        self.width == Length::Fill || self.height == Length::Fill
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        // Kontener rysujemy, gdy ma KTÓRĄKOLWIEK warstwę wizualną: kolor,
        // obramowanie, cień albo backdrop. Wcześniej warunek brzmiał
        // `if let Some(color) = self.color`, przez co `.backdrop(..)` bez
        // `.style(..)` był po cichu ignorowany — kontener zostawał niewidoczny.
        let backdrop = self.backdrop.unwrap_or_default();
        let has_shadow = self.shadow > 0.0;
        let has_border = self.border_color.is_some() && self.border_width > 0.0;

        if self.color.is_some() || has_border || has_shadow || backdrop.is_enabled() {
            let mut paint = Paint::rounded(
                layout.bounds,
                self.radius,
                self.color.unwrap_or(Color::TRANSPARENT),
            )
            .with_clip(layout.clip);
            paint.border = self.border_color;
            paint.border_width = self.border_width;
            paint.backdrop = backdrop;
            if has_shadow {
                paint.shadow = Some(Shadow::new(
                    Point::new(0.0, self.shadow * 0.25),
                    self.shadow,
                    0.0,
                    ctx.theme.palette.background.with_alpha(0.45),
                ));
            }
            ctx.renderer.draw_quad(&paint);
        }

        let (Some(child), Some(child_layout)) =
            (self.child.as_ref(), self.child_layout(tree, layout))
        else {
            return;
        };
        let node = tree.children.first().expect("węzeł potomka");

        ctx.renderer.push_clip(layout.bounds);
        child.as_widget().draw(node, ctx, &child_layout);
        ctx.renderer.pop_clip();
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        let (Some(child), Some(child_layout)) =
            (self.child.as_ref(), self.child_layout(tree, layout))
        else {
            return EventStatus::Ignored;
        };
        let node = tree.children.first().expect("węzeł potomka");
        child.as_widget().event(node, event, ctx, &child_layout)
    }

    fn cursor(&self, tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        let (Some(child), Some(child_layout)) =
            (self.child.as_ref(), self.child_layout(tree, layout))
        else {
            return Interaction::None;
        };
        let node = tree.children.first().expect("węzeł potomka");
        child.as_widget().cursor(node, &child_layout, cursor)
    }
}

/// Kontener z tłem — skrót dla [`Container::new`].
pub fn container() -> Container {
    Container::new()
}

/// Odstęp o zadanym rozmiarze.
#[derive(Debug, Clone, Copy)]
pub struct Spacer {
    size: Size,
}

impl Spacer {
    /// Nowy odstęp.
    pub fn new(size: Size) -> Self {
        Self { size }
    }

    /// Buduje element.
    pub fn build(self) -> Element {
        Element::new(self)
    }
}

impl Widget for Spacer {
    fn layout(&self, _tree: &mut Tree, limits: &Limits, _ctx: &mut Context<'_>) -> (Size, Layout) {
        let size = Size::new(
            self.size.width.min(limits.max.width),
            self.size.height.min(limits.max.height),
        );
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn draw(&self, _tree: &Tree, _ctx: &mut Context<'_>, _layout: &Layout) {}
}

/// Skrót tworzący odstęp.
pub fn spacer(width: f32, height: f32) -> Spacer {
    Spacer::new(Size::new(width, height))
}

/// Stos — elementy nałożone na siebie (kolejność = kolejność w górę).
#[derive(Debug)]
pub struct Stack {
    children: Vec<Element>,
    width: Length,
    height: Length,
}

impl Default for Stack {
    fn default() -> Self {
        Self {
            children: Vec::new(),
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }
}

impl Stack {
    /// Nowy stos.
    pub fn new() -> Self {
        Self::default()
    }

    /// Dodaje element na wierzch stosu.
    pub fn push(mut self, element: Element) -> Self {
        self.children.push(element);
        self
    }

    /// Szerokość stosu.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Wysokość stosu.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    fn child_layout(&self, tree: &Tree, layout: &Layout, index: usize) -> Option<Layout> {
        let node = tree.children.get(index)?;
        Some(Layout {
            bounds: Rect::new(layout.bounds.position, node.layout.bounds.size),
            clip: layout.clip,
            z: layout.z + index as f32,
            interaction: Interaction::Idle,
        })
    }
}

impl Widget for Stack {
    fn children(&self, tree: &mut Tree) -> Vec<Tree> {
        tree.children_from_slice(&self.children)
    }

    fn layout(&self, tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        let inner = Limits::none(limits.max);
        let mut content = Size::ZERO;
        for (index, child) in self.children.iter().enumerate() {
            let Some(node) = tree.children.get_mut(index) else {
                continue;
            };
            let size = child.as_widget().layout(node, &inner, ctx).0;
            content.width = content.width.max(size.width);
            content.height = content.height.max(size.height);
        }
        let size = Size::new(
            self.width
                .resolve(limits.max.width, content.width)
                .min(limits.max.width),
            self.height
                .resolve(limits.max.height, content.height)
                .min(limits.max.height),
        );
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn fills_main_axis(&self) -> bool {
        self.width == Length::Fill || self.height == Length::Fill
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        for (index, child) in self.children.iter().enumerate() {
            let (Some(node), Some(child_layout)) = (
                tree.children.get(index),
                self.child_layout(tree, layout, index),
            ) else {
                continue;
            };
            child.as_widget().draw(node, ctx, &child_layout);
        }
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        // Odwrócona kolejność — elementy wyżej mają pierwszeństwo.
        for (index, child) in self.children.iter().enumerate().rev() {
            let (Some(node), Some(child_layout)) = (
                tree.children.get(index),
                self.child_layout(tree, layout, index),
            ) else {
                continue;
            };
            let status = child.as_widget().event(node, event, ctx, &child_layout);
            if status.is_handled() {
                return status;
            }
        }
        EventStatus::Ignored
    }

    fn cursor(&self, tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        for (index, child) in self.children.iter().enumerate().rev() {
            let (Some(node), Some(child_layout)) = (
                tree.children.get(index),
                self.child_layout(tree, layout, index),
            ) else {
                continue;
            };
            let interaction = child.as_widget().cursor(node, &child_layout, cursor);
            if interaction != Interaction::None {
                return interaction;
            }
        }
        Interaction::None
    }
}

/// Skrót tworzący stos z danymi elementami.
pub fn stack(children: impl IntoIterator<Item = Element>) -> Stack {
    let mut s = Stack::new();
    for child in children {
        s = s.push(child);
    }
    s
}
