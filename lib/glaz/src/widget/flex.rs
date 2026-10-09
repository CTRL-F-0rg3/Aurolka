//! Kolumna i wiersz — podstawowe kontenery układu.

use crate::context::Context;
use crate::element::{Element, Widget};
use crate::event::Event;
use crate::geometry::{Align, Length, Point, Rect, Size};
use crate::layout::{Cursor, EventStatus, Interaction, Layout, Limits};
use crate::tree::Tree;

/// Wspólna implementacja osiowego układania dzieci.
#[derive(Debug)]
struct Flex {
    direction: crate::geometry::Direction,
    spacing: f32,
    padding: crate::geometry::Padding,
    width: Length,
    height: Length,
    align: Align,
    children: Vec<Element>,
}

impl Flex {
    fn new(direction: crate::geometry::Direction) -> Self {
        Self {
            direction,
            spacing: 0.0,
            padding: crate::geometry::Padding::ZERO,
            width: Length::Fill,
            height: Length::Shrink,
            align: Align::Start,
            children: Vec::new(),
        }
    }

    fn axis(&self) -> crate::geometry::Axis {
        self.direction.axis()
    }

    /// Czy ten wiersz/kolumna rozciaga sie w swojej osi glownej.
    fn fills_main_axis_impl(&self) -> bool {
        self.main_length() == Length::Fill
    }

    fn main_length(&self) -> Length {
        match self.axis() {
            crate::geometry::Axis::Horizontal => self.width,
            crate::geometry::Axis::Vertical => self.height,
        }
    }

    fn cross_length(&self) -> Length {
        match self.axis() {
            crate::geometry::Axis::Horizontal => self.height,
            crate::geometry::Axis::Vertical => self.width,
        }
    }
}

impl Widget for Flex {
    fn children(&self, tree: &mut Tree) -> Vec<Tree> {
        tree.children_from_slice(&self.children)
    }

    fn layout(&self, tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        let inner = limits.pad(self.padding);
        let axis = self.axis();

        let count = self.children.len();

        // Dzieci z `Length::Fill` dziela POMOCZĄ przestrzeń w osi glownej.
        // Bez tego kazde z nich dostalo pełna szerokosc (albo wysokosc)
        // rodzica i wiersz `pasek + przyciski` mial 2x za szeroka zawartosc,
        // przez co przyciski i panel wypadaly poza okno.
        let fill_children = self
            .children
            .iter()
            .filter(|child| child.as_widget().fills_main_axis())
            .count();
        let spacing_total_for_fills = self.spacing * count.saturating_sub(1) as f32;

        let main_available_for_children = match axis {
            crate::geometry::Axis::Horizontal => inner.max.width,
            crate::geometry::Axis::Vertical => inner.max.height,
        };
        let fill_share = if fill_children > 0 {
            ((main_available_for_children - spacing_total_for_fills).max(0.0)
                / fill_children as f32)
                .max(0.0)
        } else {
            0.0
        };

        let mut main_used = 0.0_f32;
        let mut cross_used = 0.0_f32;

        for (index, child) in self.children.iter().enumerate() {
            let Some(node) = tree.children.get_mut(index) else {
                continue;
            };
            let fills = child.as_widget().fills_main_axis();
            let child_limits = if fills {
                let mut narrowed = inner;
                match axis {
                    crate::geometry::Axis::Horizontal => {
                        narrowed.max.width = fill_share.min(narrowed.max.width);
                    }
                    crate::geometry::Axis::Vertical => {
                        narrowed.max.height = fill_share.min(narrowed.max.height);
                    }
                }
                narrowed
            } else {
                inner
            };
            let (size, _) = child.as_widget().layout(node, &child_limits, ctx);
            match axis {
                crate::geometry::Axis::Horizontal => {
                    main_used += size.width;
                    cross_used = cross_used.max(size.height);
                }
                crate::geometry::Axis::Vertical => {
                    main_used += size.height;
                    cross_used = cross_used.max(size.width);
                }
            }
        }

        let content_main = main_used + self.spacing * count.saturating_sub(1) as f32;
        let main_available = match axis {
            crate::geometry::Axis::Horizontal => inner.max.width,
            crate::geometry::Axis::Vertical => inner.max.height,
        };
        let main = self
            .main_length()
            .resolve(main_available, content_main)
            .min(main_available);

        let cross_available = match axis {
            crate::geometry::Axis::Horizontal => inner.max.height,
            crate::geometry::Axis::Vertical => inner.max.width,
        };
        let cross = self
            .cross_length()
            .resolve(cross_available, cross_used)
            .min(cross_available);

        let size = match axis {
            crate::geometry::Axis::Horizontal => Size::new(main, cross),
            crate::geometry::Axis::Vertical => Size::new(cross, main),
        };
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn fills_main_axis(&self) -> bool {
        self.fills_main_axis_impl()
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        let inner = layout.bounds.shrink(self.padding);
        let axis = self.axis();
        let mut cursor = match axis {
            crate::geometry::Axis::Horizontal => inner.position.x,
            crate::geometry::Axis::Vertical => inner.position.y,
        };

        for (index, child) in self.children.iter().enumerate() {
            let Some(node) = tree.children.get(index) else {
                continue;
            };
            let size = node.layout.bounds.size;
            let cross_used = match axis {
                crate::geometry::Axis::Horizontal => inner.size.height,
                crate::geometry::Axis::Vertical => inner.size.width,
            };
            let cross_size = match axis {
                crate::geometry::Axis::Horizontal => size.height,
                crate::geometry::Axis::Vertical => size.width,
            };
            let cross_offset = self.align.position(0.0, cross_used, cross_size);
            let offset = match axis {
                crate::geometry::Axis::Horizontal => {
                    Point::new(cursor, inner.position.y + cross_offset)
                }
                crate::geometry::Axis::Vertical => {
                    Point::new(inner.position.x + cross_offset, cursor)
                }
            };

            let child_layout = Layout {
                bounds: Rect::new(offset, size),
                clip: layout.clip,
                z: layout.z + index as f32 * 0.001,
                interaction: Interaction::Idle,
            };
            child.as_widget().draw(node, ctx, &child_layout);

            cursor += match axis {
                crate::geometry::Axis::Horizontal => size.width + self.spacing,
                crate::geometry::Axis::Vertical => size.height + self.spacing,
            };
        }
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        let inner = layout.bounds.shrink(self.padding);
        let axis = self.axis();
        let mut cursor = match axis {
            crate::geometry::Axis::Horizontal => inner.position.x,
            crate::geometry::Axis::Vertical => inner.position.y,
        };

        for (index, child) in self.children.iter().enumerate() {
            let Some(node) = tree.children.get(index) else {
                continue;
            };
            let size = node.layout.bounds.size;
            let cross_used = match axis {
                crate::geometry::Axis::Horizontal => inner.size.height,
                crate::geometry::Axis::Vertical => inner.size.width,
            };
            let cross_size = match axis {
                crate::geometry::Axis::Horizontal => size.height,
                crate::geometry::Axis::Vertical => size.width,
            };
            let cross_offset = self.align.position(0.0, cross_used, cross_size);
            let offset = match axis {
                crate::geometry::Axis::Horizontal => {
                    Point::new(cursor, inner.position.y + cross_offset)
                }
                crate::geometry::Axis::Vertical => {
                    Point::new(inner.position.x + cross_offset, cursor)
                }
            };

            let child_layout = Layout {
                bounds: Rect::new(offset, size),
                clip: layout.clip,
                z: layout.z + index as f32 * 0.001,
                interaction: Interaction::Idle,
            };
            let status = child.as_widget().event(node, event, ctx, &child_layout);
            if status.is_handled() {
                return status;
            }

            cursor += match axis {
                crate::geometry::Axis::Horizontal => size.width + self.spacing,
                crate::geometry::Axis::Vertical => size.height + self.spacing,
            };
        }
        EventStatus::Ignored
    }

    fn cursor(&self, tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        let inner = layout.bounds.shrink(self.padding);
        let axis = self.axis();
        let mut offset = match axis {
            crate::geometry::Axis::Horizontal => inner.position.x,
            crate::geometry::Axis::Vertical => inner.position.y,
        };

        for (index, child) in self.children.iter().enumerate() {
            let Some(node) = tree.children.get(index) else {
                continue;
            };
            let size = node.layout.bounds.size;
            let main = match axis {
                crate::geometry::Axis::Horizontal => size.width,
                crate::geometry::Axis::Vertical => size.height,
            };
            let bounds = match axis {
                crate::geometry::Axis::Horizontal => {
                    Rect::new(Point::new(offset, inner.position.y), size)
                }
                crate::geometry::Axis::Vertical => {
                    Rect::new(Point::new(inner.position.x, offset), size)
                }
            };
            if bounds.contains(cursor.position) {
                let child_layout = Layout::new(bounds).with_clip(layout.clip);
                let interaction = child.as_widget().cursor(node, &child_layout, cursor);
                if interaction != Interaction::None {
                    return interaction;
                }
            }
            let _ = index;
            offset += main + self.spacing;
        }
        Interaction::None
    }
}

/// Kolumna — dzieci ułożone od góry do dołu.
#[derive(Debug)]
pub struct Column {
    inner: Flex,
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

impl Column {
    /// Nowa kolumna.
    pub fn new() -> Self {
        Self {
            inner: Flex::new(crate::geometry::Direction::Vertical),
        }
    }

    /// Odstęp między dziećmi.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.inner.spacing = spacing;
        self
    }

    /// Margines wewnętrzny.
    pub fn padding(mut self, padding: impl Into<crate::geometry::Padding>) -> Self {
        self.inner.padding = padding.into();
        self
    }

    /// Szerokość kolumny.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.inner.width = width.into();
        self
    }

    /// Wysokość kolumny.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.inner.height = height.into();
        self
    }

    /// Wyrównanie dzieci w osi poprzecznej.
    pub fn align(mut self, align: Align) -> Self {
        self.inner.align = align;
        self
    }

    /// Dodaje dziecko.
    pub fn push(mut self, element: Element) -> Self {
        self.inner.children.push(element);
        self
    }

    /// Buduje element.
    pub fn build(self) -> Element {
        Element::new(self)
    }
}

impl Widget for Column {
    fn children(&self, tree: &mut Tree) -> Vec<Tree> {
        self.inner.children(tree)
    }

    fn layout(&self, tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        self.inner.layout(tree, limits, ctx)
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        self.inner.draw(tree, ctx, layout);
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        self.inner.event(tree, event, ctx, layout)
    }

    fn cursor(&self, tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        self.inner.cursor(tree, layout, cursor)
    }
}

/// Wiersz — dzieci ułożone od lewej do prawej.
#[derive(Debug)]
pub struct Row {
    inner: Flex,
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

impl Row {
    /// Nowy wiersz.
    pub fn new() -> Self {
        Self {
            inner: Flex::new(crate::geometry::Direction::Horizontal),
        }
    }

    /// Odstęp między dziećmi.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.inner.spacing = spacing;
        self
    }

    /// Margines wewnętrzny.
    pub fn padding(mut self, padding: impl Into<crate::geometry::Padding>) -> Self {
        self.inner.padding = padding.into();
        self
    }

    /// Szerokość wiersza.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.inner.width = width.into();
        self
    }

    /// Wysokość wiersza.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.inner.height = height.into();
        self
    }

    /// Wyrównanie dzieci w osi poprzecznej.
    pub fn align(mut self, align: Align) -> Self {
        self.inner.align = align;
        self
    }

    /// Dodaje dziecko.
    pub fn push(mut self, element: Element) -> Self {
        self.inner.children.push(element);
        self
    }

    /// Buduje element.
    pub fn build(self) -> Element {
        Element::new(self)
    }
}

impl Widget for Row {
    fn children(&self, tree: &mut Tree) -> Vec<Tree> {
        self.inner.children(tree)
    }

    fn layout(&self, tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        self.inner.layout(tree, limits, ctx)
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        self.inner.draw(tree, ctx, layout);
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        self.inner.event(tree, event, ctx, layout)
    }

    fn cursor(&self, tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        self.inner.cursor(tree, layout, cursor)
    }
}

/// Skrót: kolumna z dziećmi.
pub fn column(children: impl IntoIterator<Item = Element>) -> Column {
    let mut c = Column::new();
    for child in children {
        c = c.push(child);
    }
    c
}

/// Skrót: wiersz z dziećmi.
pub fn row(children: impl IntoIterator<Item = Element>) -> Row {
    let mut r = Row::new();
    for child in children {
        r = r.push(child);
    }
    r
}
