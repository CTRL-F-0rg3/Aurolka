//! # Błędy biblioteki.
//!
//! Wszystko, co może pójść nie tak przy liczeniu na GPU, sprowadza się do
//! jednego typu [`Error`]. Błędy `wgpu` (adapter, urządzenie, odpytywanie)
//! są w nim spłaszczane do tekstu — dzięki temu biblioteka nie zmusza
//! aplikacji do zależności od konkretnej wersji `wgpu` w obsłudze błędów.

use std::fmt;

/// Wynik operacji biblioteki.
pub type Result<T> = std::result::Result<T, Error>;

/// Błąd biblioteki.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// W systemie nie ma żadnego adaptera GPU.
    NoAdapter,

    /// Nie udało się wybrać adaptera GPU.
    Adapter(String),

    /// Nie udało się utworzyć urządzenia (`Device`) na wybranym adapterze.
    Device(String),

    /// Shader WGSL nie przeszedł kompilacji.
    Shader {
        /// Etykieta shadera (najczęściej nazwa kernela).
        label: String,
        /// Opis błędu wraz z miejscem w kodzie.
        message: String,
    },

    /// Bufor ma niepoprawny rozmiar (np. pusty albo za duży dla urządzenia).
    Buffer {
        /// Etykieta bufora.
        label: String,
        /// Opis problemu.
        message: String,
    },

    /// Operacja wymaga niepustego wejścia.
    Empty {
        /// Nazwa operandu, który okazał się pusty.
        what: &'static str,
    },

    /// Niezgodne rozmiary buforów lub tablic.
    Size {
        /// Co porównujemy.
        what: &'static str,
        /// Oczekiwwana liczba elementów.
        expected: usize,
        /// Faktyczna liczba elementów.
        got: usize,
    },

    /// Do bindingu podano bufor innego typu elementów, niż deklaruje shader.
    ElementType {
        /// Indeks bindingu (`@binding(n)`).
        binding: usize,
        /// Typ elementu oczekiwany przez layout.
        expected: &'static str,
        /// Typ elementu przekazanego przez aplikację.
        got: &'static str,
    },

    /// Liczba bindingów nie zgadza się z deklaracją kernela.
    Bindings {
        /// Ile bindingów deklaruje kernel.
        expected: usize,
        /// Ile bindingów przekazano.
        got: usize,
    },

    /// Dispatch miałby zero grup roboczych — nie ma czego liczyć.
    EmptyDispatch {
        /// Etykieta kernela.
        label: String,
    },

    /// Nie udało się odczytać danych z bufora GPU.
    Map(String),

    /// Urządzenie nie odpowiedziało na `poll`.
    Poll(String),
}

impl Error {
    /// Buduje błąd kompilacji shadera.
    pub fn shader(label: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Shader {
            label: label.into(),
            message: message.into(),
        }
    }

    /// Buduje błąd bufora.
    pub fn buffer(label: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Buffer {
            label: label.into(),
            message: message.into(),
        }
    }

    /// Buduje błąd niezgodnych rozmiarów.
    pub fn size(what: &'static str, expected: usize, got: usize) -> Self {
        Self::Size {
            what,
            expected,
            got,
        }
    }

    /// Buduje błąd „operacja na pustych danych”.
    pub fn empty(what: &'static str) -> Self {
        Self::Empty { what }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter => write!(f, "nie znaleziono adaptera GPU"),
            Self::Adapter(message) => write!(f, "nie udało się wybrać adaptera GPU: {message}"),
            Self::Device(message) => write!(f, "nie udało się utworzyć urządzenia GPU: {message}"),
            Self::Shader { label, message } => write!(f, "błąd shadera `{label}`: {message}"),
            Self::Buffer { label, message } => write!(f, "błąd bufora `{label}`: {message}"),
            Self::Empty { what } => write!(f, "operacja wymaga niepustego `{what}`"),
            Self::Size {
                what,
                expected,
                got,
            } => write!(f, "niezgodny rozmiar `{what}`: oczekiwano {expected}, a jest {got}"),
            Self::ElementType {
                binding,
                expected,
                got,
            } => write!(
                f,
                "binding {binding} przyjmuje `{expected}`, a podano `{got}`"
            ),
            Self::Bindings { expected, got } => {
                write!(f, "kernel deklaruje {expected} bindingów, a przekazano {got}")
            }
            Self::EmptyDispatch { label } => {
                write!(f, "kernel `{label}` dostałby zero grup roboczych")
            }
            Self::Map(message) => write!(f, "nie udało się odczytać bufora GPU: {message}"),
            Self::Poll(message) => write!(f, "błąd odpytywania urządzenia GPU: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<wgpu::RequestAdapterError> for Error {
    fn from(error: wgpu::RequestAdapterError) -> Self {
        Self::Adapter(error.to_string())
    }
}

impl From<wgpu::RequestDeviceError> for Error {
    fn from(error: wgpu::RequestDeviceError) -> Self {
        Self::Device(error.to_string())
    }
}