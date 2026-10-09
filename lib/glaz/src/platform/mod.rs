//! Warstwa platformy: most do `winit` (okna, pętla zdarzeń, mapowanie zdarzeń).
//!
//! Ten moduł jest jedynym miejscem, które zna `winit`. Dzięki temu reszta
//! biblioteki (i API użytkownika) nie jest sprzężona z biblioteką okien —
//! w razie potrzeby wystarczy podmienić implementację.

mod backend;

pub use backend::run;

/// Kursor systemowy mapowany na `winit`.
pub fn to_winit_cursor(icon: crate::window::CursorIcon) -> winit::window::CursorIcon {
    use crate::window::CursorIcon as Ours;
    match icon {
        Ours::Default => winit::window::CursorIcon::Default,
        Ours::Pointer => winit::window::CursorIcon::Pointer,
        Ours::Text => winit::window::CursorIcon::Text,
        Ours::Grab => winit::window::CursorIcon::Grab,
        Ours::Grabbing => winit::window::CursorIcon::Grabbing,
        Ours::ResizeNorth => winit::window::CursorIcon::NResize,
        Ours::ResizeSouth => winit::window::CursorIcon::SResize,
        Ours::ResizeWest => winit::window::CursorIcon::WResize,
        Ours::ResizeEast => winit::window::CursorIcon::EResize,
        Ours::ResizeNorthEast => winit::window::CursorIcon::NeResize,
        Ours::ResizeNorthWest => winit::window::CursorIcon::NwResize,
        Ours::ResizeSouthEast => winit::window::CursorIcon::SeResize,
        Ours::ResizeSouthWest => winit::window::CursorIcon::SwResize,
        Ours::Move => winit::window::CursorIcon::Move,
        Ours::NotAllowed => winit::window::CursorIcon::NotAllowed,
        Ours::Wait => winit::window::CursorIcon::Wait,
        Ours::Help => winit::window::CursorIcon::Help,
        Ours::Crosshair => winit::window::CursorIcon::Crosshair,
    }
}
