//! Zarządzanie oknem: ustawienia, builder i własne dekoracje.
//!
//! Okno jest opisane przez [`WindowSettings`] i tworzone przez [`WindowBuilder`].
//! Pola `#[non_exhaustive]` oznaczają, że przyszłe dodatki nie złamią kompatybilności.

pub mod chrome;

pub use chrome::{
    resize_direction_at, Chrome, ChromeAction, ChromeArea, CursorIcon, ResizeDirection,
    WindowCommand,
};

use crate::geometry::{Size, Size as LogicalSize};
use crate::transparency::{Backdrop, Transparency, WindowShape};

/// Identyfikator okna (unikalny w ramach procesu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u64);

/// Ustawienia okna aplikacji.
///
/// Ustawienia są **niemutowalne** — zmiany przechodzą przez
/// [`WindowBuilder`](crate::window::WindowBuilder) albo przez wywołanie metod
/// na [`crate::program::Program`], co zapobiega rozjazdu stanu konfiguracji.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WindowSettings {
    /// Tytuł okna.
    pub title: String,
    /// Rozmiar początkowy (piksele logiczne).
    pub size: LogicalSize,
    /// Minimalny rozmiar (piksele logiczne).
    pub min_size: Option<LogicalSize>,
    /// Maksymalny rozmiar (piksele logiczne).
    pub max_size: Option<LogicalSize>,
    /// Czy okno można zmieniać rozmiar (wpływa na strefy chrome).
    pub resizable: bool,
    /// Czy system ma rysować własny pasek tytułowy.
    pub decorations: bool,
    /// Czy okno jest przezroczyste (wymusza tryb alpha na surface).
    pub transparent: bool,
    /// Efekt przezroczystości i tła.
    pub transparency: Transparency,
    /// Kształt narożników okna.
    pub shape: WindowShape,
    /// Grubość aktywnej strefy zmiany rozmiaru przy krawędziach (piksele logiczne).
    pub resize_border: f32,
    /// Pozycja startowa.
    pub position: Option<(f64, f64)>,
    /// Czy okno ma być widoczne od razu po utworzeniu.
    pub visible: bool,
    /// Tło okna rysowane przed drzewem widgetów.
    pub background: crate::geometry::Color,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            title: "Okno".to_owned(),
            size: Size::new(960.0, 640.0),
            min_size: Some(Size::new(320.0, 240.0)),
            max_size: None,
            resizable: true,
            decorations: true,
            transparent: false,
            transparency: Transparency::Opaque,
            shape: WindowShape::Rectangle,
            resize_border: 8.0,
            position: None,
            visible: true,
            background: crate::geometry::Color::TRANSPARENT,
        }
    }
}

impl WindowSettings {
    /// Nowe ustawienia z domyślnymi wartościami.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ustawia tytuł.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Ustawia rozmiar.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Wyłącza własne dekoracje (rysowanie paska tytułowego po stronie aplikacji).
    pub fn with_custom_decorations(mut self) -> Self {
        self.decorations = false;
        self.transparent = true;
        self
    }

    /// Włącza przezroczystość z rozmyciem tła.
    pub fn with_blur(mut self, backdrop: Backdrop) -> Self {
        self.transparency = Transparency::Blur(backdrop);
        self.transparent = true;
        self.shape = backdrop.shape;
        self
    }

    /// Ustawia kształt okna i włącza przezroczystość.
    pub fn with_shape(mut self, shape: WindowShape) -> Self {
        self.shape = shape;
        if shape != WindowShape::Rectangle {
            self.transparent = true;
        }
        self
    }

    /// Ustawia kolor tła okna (rysowany przed drzewem widgetów).
    ///
    /// Przy przezroczystym oknie warto dać tu lekko nieprzezroczysty odcień —
    /// domyślne `Color::TRANSPARENT` daje czarne okno i nie ma czego
    /// próbkować przy rozmyciu tła.
    pub fn background(mut self, color: crate::geometry::Color) -> Self {
        self.background = color;
        self
    }
}
