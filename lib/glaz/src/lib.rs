//! **Glaz** — biblioteka UI oparta na `wgpu`, skoncentrowana na własnych
//! dekoracjach okien, przezroczystości i efektach „backdrop”.
//!
//! # Dlaczego ta biblioteka
//!
//! Większość bibliotek GUI albo nie pozwala rysować własnego paska tytułowego,
//! albo robi to kosztem stabilności API. Glaz idzie w drugą stronę:
//!
//! * **przezroczystość jest pierwszorzędowa** — okno może mieć przezroczyste
//!   tło, zaokrąglone narożniki i rozmycie (acrylic/mica) bez natywnych API
//!   systemu okien, więc działa tak samo na X11, Waylandzie, Windows i macOS;
//! * **dekoracje są zwykłymi widgetami** — zamiast udawać, że istnieje specjalny
//!   „titlebar”, biblioteka utrzymuje listę [`ChromeArea`](window::ChromeArea)
//!   przebudowywaną przy każdym układaniu drzewa, a hit-test rozstrzyga,
//!   co jest pod kursorem;
//! * **API jest zamrożone** — typy kolorów i wymiarów należą do biblioteki
//!   (nie do `wgpu`), a enumy oznaczone `#[non_exhaustive]` mogą rosnąć.
//!
//! # Przykład
//!
//! ```no_run
//! use glaz::prelude::*;
//!
//! struct MojaAplikacja {
//!     kliknieta: u32,
//! }
//!
//! impl Application for MojaAplikacja {
//!     fn update(&mut self, ctx: &mut Context<'_>, event: Event) -> Task {
//!         Task::none()
//!     }
//!
//!     fn view(&mut self) -> Element {
//!         widget::column![
//!             widget::titlebar(widget::text("Moje okno")).height(40.0),
//!             widget::button("Kliknij mnie").on_press(Message::Custom(Box::new(Click()))),
//!         ]
//!         .into()
//!     }
//! }
//!
//! struct Click;
//! # fn main() {}
//! ```
//!
//! # Warstwy
//!
//! | Moduł | Odpowiada za |
//! |---|---|
//! | [`geometry`] | punkty, rozmiary, kolory, promienie |
//! | [`layout`] | ograniczenia i wyniki układania |
//! | [`element`] | `Element` i trait `Widget` |
//! | [`renderer`] | rysowanie w `wgpu`, przezroczystość, blur |
//! | [`transparency`] | tryby przezroczystości i kształt okna |
//! | [`window`] | ustawienia okna i własne dekoracje |
//! | [`platform`] | most do `winit` (pętla zdarzeń, mapowanie zdarzeń) |
//! | [`widget`] | gotowe kontrolki |

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod context;
pub mod element;
pub mod error;
pub mod event;
pub mod geometry;
pub mod layout;
pub mod message;
pub mod platform;
pub mod renderer;
pub mod theme;
pub mod transparency;
pub mod tree;
pub mod widget;
pub mod window;

pub use app::{run, Application};
pub use context::Context;
pub use element::{Element, Widget};
pub use error::{Error, Result};
pub use event::{Event, KeyEvent, Modifiers, MouseButton, MouseEvent, NamedKey, UserEvent};
pub use geometry::{Color, Length, Padding, Point, Radius, Rect, Size};
pub use layout::{Cursor, EventStatus, Interaction, Layout, Limits};
pub use message::{Message, Task};
pub use theme::{Palette, Theme, Typography};
pub use transparency::{Backdrop, Transparency, WindowShape};
pub use tree::Id;
pub use window::{Chrome, ChromeAction, WindowCommand, WindowSettings};

/// Najczęściej używane typy — wystarczy `use glaz::prelude::*;`.
pub mod prelude {
    pub use crate::app::{run, Application};
    pub use crate::context::Context;
    pub use crate::element::{Element, Widget};
    pub use crate::event::{Event, KeyEvent, Modifiers, MouseButton, MouseEvent, NamedKey};
    pub use crate::geometry::{Align, Color, Length, Padding, Point, Radius, Rect, Size};
    pub use crate::layout::{Cursor, EventStatus, Interaction, Layout, Limits};
    pub use crate::message::{Message, Task};
    pub use crate::theme::{Palette, Theme, Typography};
    pub use crate::transparency::{Backdrop, Transparency, WindowShape};
    pub use crate::tree::Id;
    pub use crate::widget;
    pub use crate::window::{
        ChromeAction, CursorIcon, ResizeDirection, WindowCommand, WindowSettings,
    };
    pub use crate::Error;
    pub use crate::Result;
}
