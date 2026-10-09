//! Błędy biblioteki — jeden typ, żeby aplikacja nie musiała znać szczegółów
//! protokołu ani biblioteki LSP.

use std::fmt;

/// Wynik operacji.
pub type Result<T> = std::result::Result<T, Error>;

/// Błąd mostu do Visual Studio Code.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Błąd protokołu JSON-RPC: zła ramka, uszkodzony JSON, nieznana metoda.
    Protocol(String),

    /// Brak transportu (np. host zamknął kanał).
    Disconnected,

    /// Nie znaleziono dokumentu o podanym identyfikatorze URI.
    UnknownDocument(String),

    /// Nieobsługiwany język.
    UnsupportedLanguage(String),

    /// Nie udało się uruchomić procesu pomocniczego (np. `cargo`, `node`).
    Spawn(std::io::Error),

    /// Błąd wejścia/wyjścia.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(message) => write!(f, "błąd protokołu: {message}"),
            Self::Disconnected => write!(f, "połączenie z Visual Studio Code uległo przerwaniu"),
            Self::UnknownDocument(uri) => write!(f, "nieznany dokument: `{uri}`"),
            Self::UnsupportedLanguage(id) => write!(f, "nieobsługiwany język: `{id}`"),
            Self::Spawn(error) => write!(f, "nie udało się uruchomić procesu: {error}"),
            Self::Io(error) => write!(f, "błąd wejścia/wyjścia: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) | Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
