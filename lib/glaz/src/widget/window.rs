//! **Własne dekoracje okna.**
//!
//! Widgety z tego modułu nie rysują niczego „magicznie” — podczas układania
//! dopisują do [`Chrome`](crate::window::Chrome) obszary, które program
//! wykorzystuje do hit-testu kursora i wykonywania poleceń okna.
//!
//! ```
//! use aurola::prelude::*;
//! use aurola::widget;
//!
//! fn titlebar_view() -> Element {
//!     widget::row![
//!         widget::draggable_area(widget::text("Tytuł okna")),
//!         widget::window_button(WindowCommand::Close, widget::text("×")),
//!     ]
//!     .height(32.0)
//!     .spacing(0.0)
//!     .build()
//! }
//! ```
//!
//! Kolejność obszarów ma znaczenie — dziecko rejestruje się **po** rodzicu,
//! więc przycisk w pasku tytułowym ma pierwszeństwo w hit-teście.
//! Dlatego [`draggable_area`] dopisuje obszar w fazie `layout`, a dzieci
//! dopisują swoje w fazie `draw` (wykonywanej po nadrzędnych).

use crate::context::Context;
use crate::element::{Element, Widget};
use crate::event::{Event, MouseButton, MouseEvent};
use crate::geometry::{Length, Point, Rect, Size};
use crate::layout::{Cursor, EventStatus, Interaction, Layout, Limits};
use crate::message::Message;
use crate::renderer::paint::{Paint, TextAlign, TextStyle};
use crate::tree::Tree;
use crate::window::chrome::{ChromeAction, WindowCommand};

/// Obszar dekoracji: opakowuje dziecko i rejestruje akcję chrome.
///
/// Rejestracja odbywa się w fazie `layout`, więc dziecko (rejestrujące się
/// później, w `draw`) ma pierwszeństwo w hit-teście.
#[derive(Debug)]
pub struct ChromeArea {
    action: ChromeAction,
    background: Option<crate::geometry::Color>,
    radius: crate::geometry::Radius,
    hover_tint: Option<crate::geometry::Color>,
    width: Length,
    height: Length,
    command: Option<WindowCommand>,
    child: Element,
}

impl ChromeArea {
    /// Nowy obszar dekoracji.
    pub fn new(action: ChromeAction, child: Element) -> Self {
        Self {
            action,
            background: None,
            radius: crate::geometry::Radius::zero(),
            hover_tint: None,
            width: Length::Shrink,
            height: Length::Shrink,
            command: None,
            child,
        }
    }

    /// Kolor tła obszaru.
    pub fn background(mut self, color: crate::geometry::Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Promienie narożników.
    pub fn radius(mut self, radius: crate::geometry::Radius) -> Self {
        self.radius = radius;
        self
    }

    /// Buduje element.
    pub fn build(self) -> Element {
        Element::new(self)
    }

    /// Kolor narzucany po najechaniu myszy.
    pub fn on_hover(mut self, color: crate::geometry::Color) -> Self {
        self.hover_tint = Some(color);
        self
    }

    /// Szerokość obszaru.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Wysokość obszaru.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Ustawia polecenie wysyłane po kliknięciu.
    pub fn command(mut self, command: WindowCommand) -> Self {
        self.command = Some(command);
        self
    }
}

impl Widget for ChromeArea {
    fn children(&self, tree: &mut Tree) -> Vec<Tree> {
        tree.children_from_slice(std::slice::from_ref(&self.child))
    }

    fn layout(&self, tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout) {
        let node = tree.children.first_mut().expect("dziecko obszaru");
        let (content, child_layout) = self.child.as_widget().layout(node, limits, ctx);

        let size = Size::new(
            self.width
                .resolve(limits.max.width, content.width)
                .min(limits.max.width),
            self.height
                .resolve(limits.max.height, content.height)
                .min(limits.max.height),
        );

        // Obszar chrome obejmuje cały przydzielony prostokąt.
        let bounds = Rect::new(Point::ZERO, size);
        let _ = child_layout;
        ctx.chrome.push_area(bounds, self.action);

        (size, Layout::new(bounds))
    }

    fn fills_main_axis(&self) -> bool {
        self.width == Length::Fill || self.height == Length::Fill
    }

    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        let hovered = ctx.cursor_over(layout.bounds);
        let color = match (hovered, self.hover_tint) {
            (true, Some(tint)) => Some(tint),
            _ => self.background,
        };
        if let Some(color) = color {
            let paint = Paint::rounded(layout.bounds, self.radius, color).with_clip(layout.clip);
            ctx.renderer.draw_quad(&paint);
        }

        let Some(node) = tree.children.first() else {
            return;
        };
        let child_layout = Layout {
            bounds: Rect::new(layout.bounds.position, node.layout.bounds.size),
            clip: layout.clip,
            z: layout.z,
            interaction: Interaction::Idle,
        };

        // Dziecko rejestruje się tutaj — czyli po rodzicu, więc ma pierwszeństwo.
        self.child.as_widget().draw(node, ctx, &child_layout);
    }

    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        let Some(node) = tree.children.first() else {
            return EventStatus::Ignored;
        };
        let child_layout = Layout {
            bounds: Rect::new(layout.bounds.position, node.layout.bounds.size),
            clip: layout.clip,
            z: layout.z,
            interaction: Interaction::Idle,
        };

        // Obszary `Interactive` odsłaniają zawartość dla UI.
        if matches!(self.action, ChromeAction::Interactive) {
            let over = ctx.cursor_over(layout.bounds);
            if let (true, Event::Mouse(MouseEvent::ButtonPressed { button, position })) =
                (over, event)
            {
                if *button == MouseButton::Left && layout.bounds.contains(*position) {
                    if let Some(command) = self.command {
                        ctx.send(Message::Window(command));
                        return EventStatus::Handled;
                    }
                }
            }
        }

        self.child
            .as_widget()
            .event(node, event, ctx, &child_layout)
    }

    fn cursor(&self, tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        let Some(node) = tree.children.first() else {
            return match self.action {
                ChromeAction::Interactive => Interaction::Clickable,
                _ => Interaction::None,
            };
        };
        let child_layout = Layout::new(Rect::new(layout.bounds.position, node.layout.bounds.size))
            .with_clip(layout.clip);

        match self.child.as_widget().cursor(node, &child_layout, cursor) {
            Interaction::None if matches!(self.action, ChromeAction::Interactive) => {
                Interaction::Clickable
            }
            other => other,
        }
    }
}

/// Obszar przeciągający okno.
pub fn draggable_area(child: Element) -> ChromeArea {
    ChromeArea::new(ChromeAction::Drag, child)
}

/// Obszar „interaktywny” — kliknięcie nie jest traktowane jako drag.
pub fn interactive_area(child: Element) -> ChromeArea {
    ChromeArea::new(ChromeAction::Interactive, child)
}

/// Obszar zmieniający rozmiar okna w zadanym kierunku.
pub fn resize_area(
    direction: crate::window::chrome::ResizeDirection,
    child: Element,
) -> ChromeArea {
    ChromeArea::new(ChromeAction::Resize(direction), child)
}

/// Przycisk okna (minimalizacja / maksymalizacja / zamknięcie).
///
/// Sam wysyła [`Message::Window`] po kliknięciu, więc aplikacja nie musi
/// nic obsługiwać ręcznie.
#[derive(Debug, Clone)]
pub struct WindowButton {
    command: WindowCommand,
    label: String,
    size: Size,
    hover: Option<crate::geometry::Color>,
    radius: crate::geometry::Radius,
}

impl WindowButton {
    /// Nowy przycisk okna.
    pub fn new(command: WindowCommand, label: impl Into<String>) -> Self {
        Self {
            command,
            label: label.into(),
            size: Size::new(46.0, 32.0),
            hover: None,
            radius: crate::geometry::Radius::zero(),
        }
    }

    /// Rozmiar przycisku.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Kolor podświetlenia (np. czerwony dla „zamknij”).
    pub fn hover(mut self, color: crate::geometry::Color) -> Self {
        self.hover = Some(color);
        self
    }

    /// Promienie narożników.
    pub fn radius(mut self, radius: crate::geometry::Radius) -> Self {
        self.radius = radius;
        self
    }

    /// Buduje element.
    pub fn build(self) -> Element {
        Element::new(self)
    }
}

impl Widget for WindowButton {
    fn layout(&self, _tree: &mut Tree, limits: &Limits, _ctx: &mut Context<'_>) -> (Size, Layout) {
        let size = Size::new(
            self.size.width.min(limits.max.width),
            self.size.height.min(limits.max.height),
        );
        (size, Layout::new(Rect::new(Point::ZERO, size)))
    }

    fn draw(&self, _tree: &Tree, ctx: &mut Context<'_>, layout: &Layout) {
        // Rejestrujemy się w `draw`, aby przyciski w pasku miały pierwszeństwo
        // przed obszarem drag zarejestrowanym w `layout` rodzica.
        ctx.chrome
            .push_area(layout.bounds, ChromeAction::Interactive);

        let hovered = ctx.cursor_over(layout.bounds);
        if let Some(color) = self.hover.filter(|_| hovered) {
            let paint = Paint::rounded(layout.bounds, self.radius, color).with_clip(layout.clip);
            ctx.renderer.draw_quad(&paint);
        }

        let style = TextStyle {
            size: ctx.theme.typography.size,
            line_height: ctx.theme.typography.line_height,
            weight: 400,
            color: ctx.theme.palette.text,
            family: None,
            align: TextAlign::Center,
        };
        ctx.renderer.draw_text(&self.label, layout.bounds, &style);
    }

    fn event(
        &self,
        _tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        let Event::Mouse(MouseEvent::ButtonPressed { button, position }) = event else {
            return EventStatus::Ignored;
        };
        if *button != MouseButton::Left || !layout.bounds.contains(*position) {
            return EventStatus::Ignored;
        }
        ctx.send(Message::Window(self.command));
        EventStatus::Handled
    }

    fn cursor(&self, _tree: &Tree, _layout: &Layout, _cursor: Cursor) -> Interaction {
        Interaction::Clickable
    }
}

/// Skrót tworzący przycisk okna z etykietą tekstową.
pub fn window_button(command: WindowCommand, label: impl Into<String>) -> WindowButton {
    WindowButton::new(command, label)
}

/// Skrót tworzący pasek tytułowy (cała zawartość przeciąga okno).
pub fn titlebar(child: Element) -> ChromeArea {
    ChromeArea::new(ChromeAction::Drag, child)
}

// ---------------------------------------------------------------- ikony ----

/// Zestaw ikon paska tytułowego (Lucide).
///
/// Ikony to glify z fontu osadzonego w bibliotece, więc rysują się
/// przez istniejący [`crate::widget::text`] — bez osobnego pipeline'u.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleBarIcons {
    /// Minimalizacja.
    pub minimize: char,
    /// Przełączenie maksymalizacji.
    pub maximize: char,
    /// Przywrócenie okna.
    pub restore: char,
    /// Zamknięcie.
    pub close: char,
}

impl Default for TitleBarIcons {
    fn default() -> Self {
        use lucide_icons::Icon;
        Self {
            minimize: Icon::Minus.unicode(),
            maximize: Icon::Square.unicode(),
            restore: Icon::Copy.unicode(),
            close: Icon::X.unicode(),
        }
    }
}

/// Przycisk okna rysowany ikoną z fontu Lucide.
#[derive(Debug, Clone, Copy)]
pub struct TitleBarButton {
    command: WindowCommand,
    icon: char,
    hover: crate::geometry::Color,
}

impl TitleBarButton {
    /// Nowy przycisk z ikoną.
    pub fn new(command: WindowCommand, icon: char) -> Self {
        Self {
            command,
            icon,
            hover: crate::geometry::Color::from_rgba8(255, 255, 255, 26),
        }
    }

    /// Kolor podświetlenia.
    pub fn hover(mut self, color: crate::geometry::Color) -> Self {
        self.hover = color;
        self
    }

    /// Buduje element przycisku z ikoną.
    pub fn element(self, size: Size, radius: crate::geometry::Radius) -> Element {
        // Ikony są budowane przez `WindowButton` z etykietą jednokodową —
        // dzięki temu korzystają z istniejącego hit-testu i rejestracji chromu.
        window_button(self.command, self.icon.to_string())
            .size(size)
            .hover(self.hover)
            .radius(radius)
            .build()
    }
}

/// Konfiguracja gotowego paska tytułowego.
#[derive(Debug, Clone)]
pub struct TitleBar {
    title: String,
    icons: TitleBarIcons,
    button_size: Size,
    height: f32,
    radius: crate::geometry::Radius,
    padding: crate::geometry::Padding,
    hover_foreground: Option<crate::geometry::Color>,
    show_minimize: bool,
    show_maximize: bool,
    show_close: bool,
}

impl Default for TitleBar {
    fn default() -> Self {
        Self {
            title: String::new(),
            icons: TitleBarIcons::default(),
            button_size: Size::new(46.0, 32.0),
            height: 36.0,
            radius: crate::geometry::Radius::zero(),
            padding: crate::geometry::Padding::symmetric(8.0, 0.0),
            hover_foreground: None,
            show_minimize: true,
            show_maximize: true,
            show_close: true,
        }
    }
}

impl TitleBar {
    /// Nowy pasek tytułowy.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Ikony przycisków.
    pub fn icons(mut self, icons: TitleBarIcons) -> Self {
        self.icons = icons;
        self
    }

    /// Rozmiar przycisków okna.
    pub fn button_size(mut self, size: Size) -> Self {
        self.button_size = size;
        self
    }

    /// Wysokość paska.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Promienie narożników (dla okien o zaokrąglonych rogach).
    pub fn radius(mut self, radius: crate::geometry::Radius) -> Self {
        self.radius = radius;
        self
    }

    /// Margines wewnętrzny.
    pub fn padding(mut self, padding: crate::geometry::Padding) -> Self {
        self.padding = padding;
        self
    }

    /// Kolor ikony po najechaniu na przycisk.
    pub fn hover_foreground(mut self, color: crate::geometry::Color) -> Self {
        self.hover_foreground = Some(color);
        self
    }

    /// Ukrywa przycisk minimalizacji.
    pub fn without_minimize(mut self) -> Self {
        self.show_minimize = false;
        self
    }

    /// Ukrywa przycisk maksymalizacji.
    pub fn without_maximize(mut self) -> Self {
        self.show_maximize = false;
        self
    }

    /// Ukrywa przycisk zamknięcia.
    pub fn without_close(mut self) -> Self {
        self.show_close = false;
        self
    }

    /// Buduje pasek: obszar przeciągania + przyciski okna.
    ///
    /// Przyciski rejestrują się w [`Chrome`] później niż pasek, więc mają
    /// pierwszeństwo w hit-teście i kliknięcie nie uruchamia przeciągania.
    pub fn build(self) -> Element {
        let TitleBar {
            title,
            icons,
            button_size,
            height,
            radius,
            padding,
            hover_foreground,
            show_minimize,
            show_maximize,
            show_close,
        } = self;

        let label = crate::widget::Text::new(title)
            .size(12.0)
            .color(
                hover_foreground
                    .map(|c| c.with_alpha(0.9))
                    .unwrap_or(crate::geometry::Color::from_rgba8(255, 255, 255, 190)),
            )
            .build();

        // Cała dostępna szerokość należy do obszaru drag — przyciski
        // odpychają go dopiero na prawej krawędzi.
        let drag = titlebar(
            crate::widget::container()
                .child(label)
                .width(Length::Fill)
                .build(),
        )
        .width(Length::Fill)
        .height(height)
        .build();

        let mut controls: Vec<Element> = Vec::new();
        if show_minimize {
            controls.push(
                TitleBarButton::new(WindowCommand::Minimize, icons.minimize)
                    .element(button_size, radius),
            );
        }
        if show_maximize {
            controls.push(
                TitleBarButton::new(WindowCommand::ToggleMaximize, icons.maximize)
                    .element(button_size, radius),
            );
        }
        if show_close {
            controls.push(
                TitleBarButton::new(WindowCommand::Close, icons.close)
                    .hover(crate::geometry::Color::from_rgba8(220, 60, 70, 255))
                    .element(button_size, radius),
            );
        }

        crate::widget::row(vec![drag, crate::widget::row(controls).build()])
            .spacing(0.0)
            .padding(padding)
            .width(Length::Fill)
            .height(height)
            .build()
    }
}

/// Skrót tworzący gotowy pasek tytułowy z ikonami.
pub fn titlebar_with_icons(title: impl Into<String>) -> TitleBar {
    TitleBar::new(title)
}
