//! Gotowe kontrolki i kontenery.
//!
//! Zestaw jest celowo mały i spójny:
//!
//! | Funkcja | Do czego |
//! |---|---|
//! | [`column`] / [`row`] | układanie dzieci w pionie / poziomie |
//! | [`container`] | tło, margines, obramowanie, rozmycie pod spodem |
//! | [`stack`] | nakładanie elementów na siebie |
//! | [`text`] | tekst z motywu |
//! | [`button`] / [`checkbox`] / [`slider`] | kontrolki wejściowe |
//! | [`titlebar`] / [`window_button`] / [`draggable_area`] | **własne dekoracje okna** |
//! | [`spacer`] | odstęp o zadanym rozmiarze |

mod flex;
mod input;
mod surface;
mod text;
mod window;

pub use flex::{column, row, Column, Row};
pub use input::{button, checkbox, slider, Button, Checkbox, Slider};
pub use surface::{container, spacer, stack, Container, Spacer, Stack};
pub use text::{text, Text};
pub use window::{
    draggable_area, interactive_area, resize_area, titlebar, titlebar_with_icons, window_button,
    ChromeArea, TitleBar, TitleBarButton, TitleBarIcons, WindowButton,
};

/// Element rozciągający się na całą dostępną przestrzeń.
pub fn fill() -> crate::element::Element {
    container()
        .width(crate::geometry::Length::Fill)
        .height(crate::geometry::Length::Fill)
        .build()
}

/// Wyrównanie zawartości w kontenerze.
pub use crate::geometry::Align;

/// Buduje kolumnę z listy elementów.
///
/// ```ignore
/// use glaz::column;
/// column![text("a").build(), button("b").build()]
/// ```
///
/// Makro jest eksportowane w katalogu głównym crate'a (`glaz::column!`),
/// żeby nie kolidowało z funkcją [`column`] w module `widget`.
#[macro_export]
macro_rules! column {
    ($($child:expr),* $(,)?) => {
        $crate::widget::column(::std::vec![$($child),*])
    };
}

/// Buduje wiersz z listy elementów.
///
/// ```ignore
/// use glaz::row;
/// row![text("a").build(), button("b").build()]
/// ```
#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => {
        $crate::widget::row(::std::vec![$($child),*])
    };
}
