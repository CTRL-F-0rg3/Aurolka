//! Wiadomości wysyłane przez widgety do aplikacji.
//!
//! Widgety nie mutują stanu aplikacji bezpośrednio — zwracają [`Message`].
//! Dzięki temu logika UI jest czysta i testowalna, a typ payloadu jest dowolny.
//!
//! Kanał `Message::Custom` jest szczególnie przydatny do synchronizacji UI
//! z usługą/modelem AI: widget może wysłać strukturę z danymi, a aplikacja
//! odczytuje ją przez [`Message::downcast`].

use std::any::Any;

use crate::event::Event;
use crate::window::chrome::WindowCommand;

/// Wiadomość z widgetu (albo z systemu) do `Application::update`.
#[derive(Debug)]
#[non_exhaustive]
pub enum Message {
    /// Zdarzenie wejściowe przekazane dalej (np. z testów lub z autosave).
    Event(Event),
    /// Polecenie zarządzania oknem (z przycisku własnych dekoracji).
    Window(WindowCommand),
    /// Zamknij bieżące okno.
    CloseWindow,
    /// Zakończ całą aplikację.
    Exit,
    /// Skopiuj tekst do schowka systemowego.
    Copy(String),
    /// Zmiana motywu jasnego/ciemnego.
    ToggleTheme,
    /// Dowolna wiadomość domenowa typu `T`.
    Custom(Box<dyn Any + Send>),
}

impl Message {
    /// Czy wiadomość jest typu `T`.
    pub fn is<T: 'static>(&self) -> bool {
        matches!(self, Self::Custom(v) if v.is::<T>())
    }

    /// Próbuje odczytać payload typu `T`.
    pub fn downcast<T: 'static>(self) -> Option<T> {
        match self {
            Self::Custom(v) => v.downcast::<T>().ok().map(|v| *v),
            _ => None,
        }
    }

    /// Referencja do payloadu typu `T`.
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        match self {
            Self::Custom(v) => v.downcast_ref::<T>(),
            _ => None,
        }
    }

    /// Czy to wiadomość powodująca zakończenie aplikacji.
    pub fn should_exit(&self) -> bool {
        matches!(self, Self::Exit)
    }
}

impl From<Event> for Message {
    fn from(event: Event) -> Self {
        Self::Event(event)
    }
}

impl From<WindowCommand> for Message {
    fn from(command: WindowCommand) -> Self {
        Self::Window(command)
    }
}

/// Zbiór zadań asynchronicznych zwracanych przez `Application::update`.
///
/// Biblioteka dostarcza minimalistyczny executor (bez zależności od
/// `futures`): zadania są odpytywane raz na klatkę. Do przełączania aplikacji
/// w tryb „renderuj tylko gdy coś się dzieje” służy [`Task::none`].
#[derive(Default)]
pub struct Task {
    inner: Option<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>,
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("pending", &self.inner.is_some())
            .finish()
    }
}

impl Task {
    /// Zadanie nic nie robiące.
    pub fn none() -> Self {
        Self { inner: None }
    }

    /// Zadanie, które nic nie robi (alias semantyczny).
    pub fn done() -> Self {
        Self::none()
    }

    /// Opakowanie przyszłości jako zadanie.
    pub fn future(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self {
            inner: Some(Box::pin(future)),
        }
    }

    /// Czy zadanie jest zakończone (brak oczekiwania).
    pub fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    /// Scala dwa zadania — pierwsze niepuste wygrywa.
    pub fn or(self, other: Self) -> Self {
        if self.inner.is_some() {
            self
        } else {
            other
        }
    }
}

use std::future::Future;
use std::pin::Pin;
