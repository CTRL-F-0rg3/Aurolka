//! Typy LSP (Language Server Protocol) używane przez ten most.
//!
//! Zbieramy tylko to, czego naprawdę potrzebujemy: pozycje, zasięgi,
//! diagnostykę, dokumenty i parametry `initialize`. Pełny LSP ma kilkadziesiąt
//! typów, ale każdy z nich kosztowałby konwersję do i z JSON — więc trzymamy
//! minimalny podzbiór, a nieznane pola przechodzą jako `serde_json::Value`.
//!
//! ```
//! use vscode_bridge::lsp::{Diagnostic, Position, Range, Severity};
//!
//! let d = Diagnostic::error(
//!     Range::new(Position::new(3, 10), Position::new(3, 14)),
//!     "nieoczekiwany koniec pliku",
//! );
//! assert_eq!(d.severity, Some(Severity::Error));
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Pozycja w dokumencie: znak jest liczony od zera, linia od zera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    /// Numer linii, od zera.
    pub line: u32,
    /// Numer znaku w linii, od zera.
    pub character: u32,
}

impl Position {
    /// Nowa pozycja.
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// Zasięg tekstu — od początku do końca, zawsze uszeregowany.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Range {
    /// Początek.
    pub start: Position,
    /// Koniec.
    pub end: Position,
}

impl Range {
    /// Nowy zasięg.
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    /// Zasięg obejmujący jedną pozycję (używany przez analizator).
    pub const fn point(at: Position) -> Self {
        Self { start: at, end: at }
    }

    /// Czy zasięg zawiera pozycję.
    pub fn contains(&self, position: Position) -> bool {
        position >= self.start && position <= self.end
    }

    /// Czy zasięgi się przecinają.
    pub fn overlaps(&self, other: &Range) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// Poziom powagi diagnostyki.
///
/// W JSONie LSP to **liczby** 1–4 (takiego formatu oczekuje VS Code w
/// `DiagnosticSeverity`), mimo że w kodzie czytamy je jako nazwany wariant.
/// Serializacja jest więc ręczna: `#[serde(rename_all = "lowercase")` dałoby
/// `"error"`, a edytor takiego komunikatu nie zrozumie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Błąd — blokuje kompilację (`DiagnosticSeverity.Error` = 1).
    Error,
    /// Ostrzeżenie (`DiagnosticSeverity.Warning` = 2).
    Warning,
    /// Informacja (`DiagnosticSeverity.Information` = 3).
    Information,
    /// Podpowiedź (`DiagnosticSeverity.Hint` = 4).
    Hint,
}

impl Severity {
    /// Numer używany w JSONie LSP.
    pub const fn as_code(self) -> u8 {
        match self {
            Self::Error => 1,
            Self::Warning => 2,
            Self::Information => 3,
            Self::Hint => 4,
        }
    }

    /// Odwrotność [`Severity::as_code`] — `None` dla liczb spoza 1–4.
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Error),
            2 => Some(Self::Warning),
            3 => Some(Self::Information),
            4 => Some(Self::Hint),
            _ => None,
        }
    }
}

impl Serialize for Severity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.as_code())
    }
}

impl<'de> Deserialize<'de> for Severity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = u8::deserialize(deserializer)?;
        Self::from_code(code).ok_or_else(|| {
            serde::de::Error::custom(format!("`severity` poza zakresem 1–4: {code}"))
        })
    }
}

/// Jedna diagnostyka (podkreślenie w edytorze).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Zakres podkreślany w edytorze.
    pub range: Range,
    /// Komunikat dla człowieka.
    pub message: String,
    /// Powaga.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Kod diagnostyki (np. `E0308`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Źródło (np. `vscode-bridge` albo `cargo`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Dodatkowe pola specyficzne dla serwera.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Diagnostic {
    /// Błąd z komunikatem i zakresem.
    pub fn error(range: Range, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
            severity: Some(Severity::Error),
            ..Self::default()
        }
    }

    /// Ostrzeżenie z komunikatem i zakresem.
    pub fn warning(range: Range, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
            severity: Some(Severity::Warning),
            ..Self::default()
        }
    }
}
/// Parametr `textDocument/didOpen`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenParams {
    /// Otwarty dokument.
    pub text_document: TextDocumentItem,
}

/// Parametr `textDocument/didChange` (wariant pełnego tekstu).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeParams {
    /// Zmieniony dokument.
    pub text_document: VersionedTextDocumentIdentifier,
    /// Lista zmian; obsługujemy wariant z pełnym tekstem.
    #[serde(default)]
    pub content_changes: Vec<TextDocumentContentChangeEvent>,
}

/// Wersjonowany identyfikator dokumentu.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionedTextDocumentIdentifier {
    /// Identyfikator URI.
    pub uri: String,
    /// Wersja tekstu.
    pub version: i64,
}

/// Jedna zmiana treści.
#[derive(Debug, Clone, Deserialize)]
pub struct TextDocumentContentChangeEvent {
    /// Nowa treść dokumentu (wariant pełnego tekstu).
    #[serde(default)]
    pub text: Option<String>,
}

/// Element `textDocument` w `didOpen`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentItem {
    /// Identyfikator URI, np. `file:///home/x/agh.rs`.
    pub uri: String,
    /// Identyfikator języka, np. `rust`.
    pub language_id: String,
    /// Wersja dokumentu.
    pub version: i64,
    /// Pełna treść.
    pub text: String,
}

/// Parametr `textDocument/didClose` i `didSave`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidCloseParams {
    /// Dokument.
    pub text_document: TextDocumentIdentifier,
}

/// Identyfikator dokumentu.
#[derive(Debug, Clone, Deserialize)]
pub struct TextDocumentIdentifier {
    /// Identyfikator URI.
    pub uri: String,
}

/// Parametr `initialize` — tylko to, co nas interesuje.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Informacje o procesie klienta.
    #[serde(default)]
    pub process_id: Option<i64>,
    /// Gdzie leży katalog roboczy.
    #[serde(default)]
    pub root_uri: Option<String>,
    /// Katalogi należące do projektu.
    #[serde(default)]
    pub workspace_folders: Option<Vec<WorkspaceFolder>>,
}

/// Katalog projektu.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceFolder {
    /// Identyfikator URI.
    pub uri: String,
    /// Nazwa wyświetlana.
    pub name: String,
}

/// Opis wystawianych komend.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExecuteCommandOptions {
    /// Lista komend, które klient ma zarejestrować.
    pub commands: Vec<String>,
}

/// Deklaracja możliwości serwera (wynik `initialize`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    /// Tryb synchronizacji tekstu: 1 = pełna treść po każdej zmianie.
    pub text_document_sync: i64,
    /// Czy serwer wystawia komendy.
    pub execute_command_provider: Option<ExecuteCommandOptions>,
    /// Czy serwer wystawia symbole dokumentu.
    pub document_symbol_provider: bool,
}

impl ServerCapabilities {
    /// Zestaw możliwości naszego serwera.
    pub fn for_bridge() -> Self {
        Self {
            text_document_sync: 1,
            execute_command_provider: Some(ExecuteCommandOptions {
                commands: crate::server::COMMANDS
                    .iter()
                    .map(|c| (*c).to_owned())
                    .collect(),
            }),
            document_symbol_provider: true,
        }
    }
}

/// Wynik `initialize`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Możliwości serwera.
    pub capabilities: ServerCapabilities,
    /// Informacja o serwerze (pokazywana w `about`).
    pub server_info: ServerInfo,
}

/// Opis serwera.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    /// Nazwa.
    pub name: String,
    /// Wersja.
    pub version: String,
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pozycje_porownuja_sie_po_kolejnosci() {
        assert!(Position::new(1, 0) > Position::new(0, 99));
        assert_eq!(Position::default(), Position::new(0, 0));
    }

    #[test]
    fn zasiag_zawiera_i_przecina() {
        let z = Range::new(Position::new(1, 0), Position::new(3, 0));
        assert!(z.contains(Position::new(2, 5)));
        assert!(!z.contains(Position::new(4, 0)));
        assert!(z.overlaps(&Range::new(Position::new(3, 0), Position::new(5, 0))));
        assert!(!z.overlaps(&Range::new(Position::new(4, 0), Position::new(5, 0))));
        assert_eq!(Range::point(Position::new(1, 1)).start, Position::new(1, 1));
    }

    #[test]
    fn diagnostyka_serializuje_sie_po_lsp() {
        let d = Diagnostic::error(
            Range::new(Position::new(1, 2), Position::new(1, 5)),
            "brak nawiasu",
        );
        let v = serde_json::to_value(&d).unwrap();
        // VS Code oczekuje liczby, nie słowa — inaczej podkreślenie znika.
        assert_eq!(v["severity"], 1);
        assert_eq!(v["message"], "brak nawiasu");
        assert_eq!(v["range"]["start"]["line"], 1);
        assert_eq!(v["range"]["start"]["character"], 2);
        // Pola `null`-owe pomijamy — VS Code ich nie oczekuje.
        assert!(v.get("code").is_none());
    }

    #[test]
    fn powaga_serializuje_sie_jako_liczba() {
        assert_eq!(serde_json::to_value(Severity::Error).unwrap(), json!(1));
        assert_eq!(serde_json::to_value(Severity::Warning).unwrap(), json!(2));
        assert_eq!(
            serde_json::to_value(Severity::Information).unwrap(),
            json!(3)
        );
        assert_eq!(serde_json::to_value(Severity::Hint).unwrap(), json!(4));
    }

    #[test]
    fn powaga_wczytuje_sie_z_liczby() {
        let z: Severity = serde_json::from_value(json!(3)).unwrap();
        assert_eq!(z, Severity::Information);
        assert!(serde_json::from_value::<Severity>(json!(9)).is_err());
        assert!(serde_json::from_value::<Severity>(json!("error")).is_err());
    }

    #[test]
    fn powaga_zachowuje_kody_1_do_4() {
        for (kod, wariant) in [
            (1u8, Severity::Error),
            (2, Severity::Warning),
            (3, Severity::Information),
            (4, Severity::Hint),
        ] {
            assert_eq!(Severity::from_code(kod), Some(wariant));
            assert_eq!(wariant.as_code(), kod);
        }
        assert_eq!(Severity::from_code(0), None);
        assert_eq!(Severity::from_code(5), None);
    }

    #[test]
    fn did_open_odczytuje_json_vs_code() {
        let params: DidOpenParams = serde_json::from_value(serde_json::json!({
            "textDocument": {
                "uri": "file:///a.rs",
                "languageId": "rust",
                "version": 1,
                "text": "fn main() {}"
            }
        }))
        .unwrap();
        assert_eq!(params.text_document.language_id, "rust");
        assert_eq!(params.text_document.text, "fn main() {}");
    }

    #[test]
    fn initialize_odczytuje_katalogi() {
        let params: InitializeParams = serde_json::from_value(serde_json::json!({
            "processId": 42,
            "rootUri": "file:///projekt",
            "workspaceFolders": [
                { "uri": "file:///projekt", "name": "projekt" }
            ]
        }))
        .unwrap();
        assert_eq!(params.process_id, Some(42));
        assert_eq!(params.workspace_folders.unwrap()[0].name, "projekt");
    }

    #[test]
    fn mozliwosci_serializuja_sie_jak_w_lsp() {
        let v = serde_json::to_value(ServerCapabilities::for_bridge()).unwrap();
        assert_eq!(v["textDocumentSync"], 1);
        assert_eq!(v["documentSymbolProvider"], true);
        assert!(v["executeCommandProvider"]["commands"].is_array());
    }
}
