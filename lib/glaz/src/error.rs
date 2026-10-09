//! Typy błędów biblioteki.
//!
//! Błędy są z definicji „niewidoczne” — nie zwracamy `Result` z krytycznych
//! miejsc (np. `update`), żeby nie zaśmiecać API. `Result` pojawia się tam,
//! gdzie użytkownik może chcieć obsłużyć sytuację (start aplikacji, render).

use std::fmt;

/// Wynik operacji zwracającej błąd biblioteki.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Błąd biblioteki.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Nie znaleziono adaptera GPU spełniającego wymagania.
    Adapter(wgpu::RequestAdapterError),
    /// Nie udało się utworzyć urządzenia graficznego.
    Device(wgpu::RequestDeviceError),
    /// Nie udało się utworzyć powierzchni renderowania dla okna.
    Surface(wgpu::CreateSurfaceError),
    /// Błąd przy nabywaniu klatki ze surface.
    AcquireFrame(wgpu::CurrentSurfaceTexture),
    /// Nie udało się utworzyć okna systemowego.
    Window(String),
    /// Nie udało się uruchomić pętli zdarzeń.
    EventLoop(String),
    /// Tekst jest za długi, aby zmieścić się w zadanych ograniczeniach.
    TextOverflow {
        /// Maksymalna dozwolona wysokość.
        max_height: f32,
        /// Wymagana wysokość tekstu.
        required_height: f32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Adapter(e) => write!(f, "brak kompatybilnego adaptera GPU: {e}"),
            Self::Device(e) => write!(f, "nie udało się utworzyć urządzenia GPU: {e}"),
            Self::Surface(e) => write!(f, "nie udało się utworzyć powierzchni renderowania: {e}"),
            Self::AcquireFrame(e) => write!(f, "nie udało się pobrać klatki: {e:?}"),
            Self::Window(e) => write!(f, "błąd okna: {e}"),
            Self::EventLoop(e) => write!(f, "błąd pętli zdarzeń: {e}"),
            Self::TextOverflow {
                max_height,
                required_height,
            } => write!(
                f,
                "tekst nie mieści się w {max_height:.1} px (wymaga {required_height:.1} px)"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Adapter(e) => Some(e),
            Self::Device(e) => Some(e),
            Self::Surface(e) => Some(e),
            _ => None,
        }
    }
}

impl From<wgpu::RequestAdapterError> for Error {
    fn from(e: wgpu::RequestAdapterError) -> Self {
        Self::Adapter(e)
    }
}

impl From<wgpu::RequestDeviceError> for Error {
    fn from(e: wgpu::RequestDeviceError) -> Self {
        Self::Device(e)
    }
}

impl From<wgpu::CreateSurfaceError> for Error {
    fn from(e: wgpu::CreateSurfaceError) -> Self {
        Self::Surface(e)
    }
}
