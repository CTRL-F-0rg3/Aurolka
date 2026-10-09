//! Kanał zwrotny: jak serwer prosi VS Code o coś.
//!
//! Serwer nie tylko przyjmuje wiadomości — potrafi też **zażądać** od edytora
//! pokazania komunikatu, otwarcia pliku, wstawienia tekstu, uruchomienia komendy
//! albo zapisu linii w kanale wyjściowym. To jest ta część, dzięki której
//! aplikacja oparta na [`aurum`](https://docs.rs/aurum) może gadać z edytorem.
//!
//! ```
//! use std::io::Cursor;
//! use std::sync::{Arc, Mutex};
//! use vscode_bridge::host::{Host, LogLevel};
//! use vscode_bridge::protocol::{Message, Transport};
//!
//! #[derive(Clone, Default)]
//! struct Buf(Arc<Mutex<Vec<u8>>>);
//!
//! impl std::io::Write for Buf {
//!     fn write(&mut self, d: &[u8]) -> std::io::Result<usize> {
//!         self.0.lock().unwrap().extend_from_slice(d);
//!         Ok(d.len())
//!     }
//!     fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
//! }
//!
//! let bufor = Buf::default();
//! let mut transport = Transport::new(Cursor::new(Vec::new()), bufor.clone());
//! let mut host = Host::new(&mut transport, 1);
//!
//! host.show_information("Witaj w VS Code").unwrap();
//! assert_eq!(host.last_id(), 1, "powiadomienia nie zużywają identyfikatorów");
//!
//! let bajty = bufor.0.lock().unwrap().clone();
//! let wypis = String::from_utf8_lossy(&bajty);
//! assert!(wypis.contains("window/showInformationMessage"));
//! assert!(wypis.contains("Content-Length:"));
//! # let _ = (Message::notification("exit", serde_json::json!({})), LogLevel::Info);
//! ```

use serde_json::json;

use crate::error::Result;
use crate::protocol::{Message, Transport};

/// Poziom wpisu w kanale wyjściowym rozszerzenia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Informacja.
    Info,
    /// Ostrzeżenie.
    Warning,
    /// Błąd.
    Error,
}

impl LogLevel {
    /// Tekst przekazywany do klienta (`window/logMessage`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// Numer typu komunikatu LSP (1 = error, 2 = warning, 3 = info).
    pub fn as_message_type(self) -> i64 {
        match self {
            Self::Error => 1,
            Self::Warning => 2,
            Self::Info => 3,
        }
    }
}

/// Kanał zwrotny do Visual Studio Code.
pub struct Host<'a, T: std::io::BufRead> {
    transport: &'a mut Transport<T>,
    /// Ostatni nadany identyfikator; następny to `+1`.
    ostatni: i64,
}

impl<'a, T: std::io::BufRead> Host<'a, T> {
    /// Podłącza kanał do istniejącego transportu.
    pub fn new(transport: &'a mut Transport<T>, ostatni_id: i64) -> Self {
        Self {
            transport,
            ostatni: ostatni_id,
        }
    }

    /// Nadaje następny identyfikator żądania.
    pub fn next_id(&mut self) -> i64 {
        self.ostatni += 1;
        self.ostatni
    }

    /// Ostatni nadany identyfikator — przydatny przy przebudowie hosta.
    pub fn last_id(&self) -> i64 {
        self.ostatni
    }

    /// Wysyła gotowe powiadomienie do klienta.
    pub fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        self.transport.send(&Message::notification(method, params))
    }

    /// Wysyła żądanie do klienta (bez czekania na odpowiedź).
    pub fn request(&mut self, method: &str, params: serde_json::Value) -> Result<i64> {
        let id = self.next_id();
        self.transport.send(&Message::request(id, method, params))?;
        Ok(id)
    }

    /// Komunikat informacyjny w rogu ekranu.
    pub fn show_information(&mut self, message: &str) -> Result<()> {
        self.notify(
            "window/showInformationMessage",
            json!({ "message": message, "type": "info" }),
        )
    }

    /// Ostrzeżenie w rogu ekranu.
    pub fn show_warning(&mut self, message: &str) -> Result<()> {
        self.notify(
            "window/showWarningMessage",
            json!({ "message": message, "type": "warning" }),
        )
    }

    /// Błąd w rogu ekranu.
    pub fn show_error(&mut self, message: &str) -> Result<()> {
        self.notify(
            "window/showErrorMessage",
            json!({ "message": message, "type": "error" }),
        )
    }

    /// Otwiera plik w edytorze.
    pub fn open_document(&mut self, uri: &str) -> Result<()> {
        self.notify("vscode/open", json!({ "uri": uri }))
    }

    /// Wstawia tekst do otwartego dokumentu.
    pub fn insert_text(
        &mut self,
        uri: &str,
        position: crate::lsp::Position,
        text: &str,
    ) -> Result<()> {
        self.notify(
            "vscode/insertText",
            json!({ "uri": uri, "position": position, "text": text }),
        )
    }

    /// Uruchamia komendę klienta (np. `workbench.action.terminal.sendSequence`).
    pub fn execute_command(&mut self, command: &str, args: Vec<serde_json::Value>) -> Result<()> {
        self.notify(
            "workspace/executeCommand",
            json!({ "command": command, "arguments": args }),
        )
    }

    /// Wpis w kanale wyjściowym rozszerzenia.
    pub fn log(&mut self, level: LogLevel, message: &str) -> Result<()> {
        self.notify(
            "window/logMessage",
            json!({ "type": level.as_message_type(), "message": message }),
        )
    }

    /// Zmienia stan paska postępu w oknie.
    pub fn set_status(&mut self, text: &str) -> Result<()> {
        self.notify("vscode/setStatus", json!({ "text": text }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::Position;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    /// Bufor współdzielony — pozwala sprawdzić, co host wypisał.
    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl Buf {
        fn tekst(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    impl std::io::Write for Buf {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Transport z podpiętym buforem — host trzeba podpiąć osobno.
    fn setup() -> (Buf, Transport<Cursor<Vec<u8>>>) {
        let bufor = Buf::default();
        let transport = Transport::new(Cursor::new(Vec::new()), bufor.clone());
        (bufor, transport)
    }

    #[test]
    fn identyfikatory_idą_od_wczysto() {
        let (bufor, mut transport) = setup();
        let mut host = Host::new(&mut transport, 1);
        assert_eq!(host.next_id(), 2);
        host.notify("test", json!({})).unwrap();
        assert_eq!(host.next_id(), 3);
        assert!(bufor.tekst().contains("\"method\":\"test\""));
    }

    #[test]
    fn poziomy_logowania_tlumaczone_sa_na_kody() {
        assert_eq!(LogLevel::Info.as_str(), "info");
        assert_eq!(LogLevel::Error.as_message_type(), 1);
        assert_eq!(LogLevel::Info.as_message_type(), 3);
    }

    #[test]
    fn komunikaty_uzywaja_oczekiwanych_metod() {
        let (bufor, mut transport) = setup();
        let mut host = Host::new(&mut transport, 1);

        host.show_information("cześć").unwrap();
        host.show_warning("uwaga").unwrap();
        host.show_error("błąd").unwrap();
        host.open_document("file:///a.rs").unwrap();
        host.insert_text("file:///a.rs", Position::new(1, 2), "tekst")
            .unwrap();
        host.execute_command("terminal", vec![json!("cargo test")])
            .unwrap();
        host.log(LogLevel::Warning, "zapis").unwrap();
        host.set_status("80%").unwrap();

        let wypis = bufor.tekst();
        for metoda in [
            "window/showInformationMessage",
            "window/showWarningMessage",
            "window/showErrorMessage",
            "vscode/open",
            "vscode/insertText",
            "workspace/executeCommand",
            "window/logMessage",
            "vscode/setStatus",
        ] {
            assert!(wypis.contains(metoda), "brakuje metody `{metoda}`");
        }
        assert!(wypis.contains("\"type\":2"), "logWarning ma typ 2");
    }

    #[test]
    fn zadanie_dostaje_id() {
        let (bufor, mut transport) = setup();
        let mut host = Host::new(&mut transport, 7);
        let id = host.request("custom/ask", json!({"x": 1})).unwrap();
        assert_eq!(id, 8);
        assert!(bufor.tekst().contains("\"id\":8"));
        assert!(bufor.tekst().contains("\"method\":\"custom/ask\""));
    }
}
