//! Biblioteki `neuro-lib` wszystko, co może pójść nie tak, sprowadza do
//! jednego typu [`Error`] — tak jak `aurum`. Błędy `wgpu` są spłaszczane
//! do tekstu, żeby aplikacja nie musiała zależeć od konkretnej wersji `wgpu`.

use std::fmt;

/// Wynik operacji `neuro-lib`.
pub type Result<T> = std::result::Result<T, Error>;

/// Błąd `neuro-lib`.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// W systemie nie ma żadnego adaptera GPU.
    NoAdapter,

    /// Nie udało się wybrać adaptera GPU.
    Adapter(String),

    /// Nie udało się utworzyć urządzenia (`Device`) na wybranym adapterze.
    Device(String),

    /// Shader WGSL nie przeszedł walidacji.
    Shader {
        /// Etykieta shadera.
        label: String,
        /// Opis błędu wraz z numerem linii.
        message: String,
    },

    /// Dane topologii są niespójne (pusty sektor, zła liczba neuronów…).
    Topology(String),

    /// Parametry macierzy scenariuszy są niepoprawne (wymiar, rozmiar bufora…).
    Matrix(String),

    /// Zadanie (plik XML) jest niepoprawne albo nie da się go wykonać.
    Task(String),

    /// Urządzenie nie odpowiedziało na `poll`.
    Poll(String),

    /// Nie udało się odczytać bufora z GPU.
    Map(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter => write!(f, "nie znaleziono adaptera GPU"),
            Self::Adapter(message) => write!(f, "nie udało się wybrać adaptera GPU: {message}"),
            Self::Device(message) => write!(f, "nie udało się utworzyć urządzenia GPU: {message}"),
            Self::Shader { label, message } => {
                write!(f, "błąd shadera `{label}`: {message}")
            }
            Self::Topology(message) => write!(f, "błąd topologii: {message}"),
            Self::Matrix(message) => write!(f, "błąd macierzy scenariuszy: {message}"),
            Self::Task(message) => write!(f, "błąd zadania: {message}"),
            Self::Poll(message) => write!(f, "błąd odpytywania urządzenia GPU: {message}"),
            Self::Map(message) => write!(f, "nie udało się odczytać bufora GPU: {message}"),
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