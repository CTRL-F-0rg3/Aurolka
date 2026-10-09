//! Serwer LSP: pętla wiadomości i obsługa metod.
//!
//! Serwer to stan + tabela metod. Stan trzyma [`Workspace`], a każda obsłużona
//! metoda aktualizuje diagnostykę i wysyła ją przez [`Host`]. Dzięki temu cały
//! serwer da się przetestować **bez VS Code** — wystarczy podstawić transport
//! czytający bajty z testu (patrz `tests/protocol.rs`).

use std::path::PathBuf;

use serde_json::json;

use crate::analysis;
use crate::error::Result;
use crate::host::Host;
use crate::lsp::{
    Diagnostic, DidChangeParams, DidCloseParams, DidOpenParams, InitializeParams, InitializeResult,
    ServerInfo,
};
use crate::protocol::Message;
use crate::symbols;
use crate::workspace::{uri_to_path, Document, Language, Workspace};

/// Komendy wystawiane przez serwer (klient musi je zarejestrować).
pub const COMMANDS: &[&str] = &[
    "vscodeBridge.status",
    "vscodeBridge.reanalyzeAll",
    "vscodeBridge.reanalyzeFile",
];

/// Stan serwera: obszar roboczy, dokumenty i flagi cyklu życia.
pub struct Server {
    workspace: Workspace,
    initialized: bool,
    shutting_down: bool,
    exit_code: Option<i32>,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("initialized", &self.initialized)
            .field("documents", &self.workspace.len())
            .field("shutting_down", &self.shutting_down)
            .finish()
    }
}

impl Server {
    /// Pusty serwer, gotowy do `initialize`.
    pub fn new() -> Self {
        Self {
            workspace: Workspace::new(),
            initialized: false,
            shutting_down: false,
            exit_code: None,
        }
    }

    /// Stan obszaru roboczego.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Czy serwer przeszedł `initialize`.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Kod wyjścia procesu, jeśli klient wysłał `exit`.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Obsługuje jedną wiadomość; zwraca odpowiedź, o ile jest potrzebna.
    ///
    /// Powiadomienia nie generują odpowiedzi — zgodnie ze specyfikacją.
    /// Diagnostyka leci do klienta przez `host`.
    pub fn handle<T: std::io::BufRead>(
        &mut self,
        message: &Message,
        host: &mut Host<'_, T>,
    ) -> Result<Option<Message>> {
        let Some(method) = message.method() else {
            return Ok(None); // odpowiedź na nasze żądanie — ignorujemy
        };

        log::debug!("vscode-bridge: `{method}`");

        let params = message.params().cloned().unwrap_or(json!({}));

        match method {
            "initialize" => Ok(Some(
                self.initialize(message.id().cloned().unwrap_or(json!(null)), params),
            )),
            "initialized" => {
                self.initialized = true;
                Ok(None)
            }
            "shutdown" => {
                self.shutting_down = true;
                Ok(Some(Message::result(
                    message.id().cloned().unwrap_or(json!(null)),
                    json!(null),
                )))
            }
            "exit" => {
                // Kod 0 tylko wtedy, gdy wcześniej przyszło `shutdown`.
                self.exit_code = Some(if self.shutting_down { 0 } else { 1 });
                Ok(None)
            }
            "textDocument/didOpen" => self.did_open(params, host),
            "textDocument/didChange" => self.did_change(params, host),
            "textDocument/didClose" => self.did_close(params, host),
            "textDocument/documentSymbol" => Ok(self.document_symbol(message, params)),
            "workspace/executeCommand" => self.execute_command(message, params, host),
            "textDocument/didSave" => self.publish_for(&uri_of(&params), host).map(|_| None),
            "workspace/didChangeConfiguration" => {
                // Nie mamy ustawień poza włączonym analizatorem, ale odświeżenie
                // diagnostyki kosztuje nic, a poprawia wynik.
                self.publish_all(host);
                Ok(None)
            }
            "$/cancelRequest" | "$/setTrace" => Ok(None),
            other => Ok(nieznana_metoda(message, other)),
        }
    }

    /// `initialize` — zapamiętujemy katalogi i deklarujemy możliwości.
    ///
    /// `id` musi pochodzić z żądania: klient paruje odpowiedź po nim, więc
    /// wpisanie na sztywno `1` gubi wszystkie kolejne żądania.
    fn initialize(&mut self, id: serde_json::Value, params: serde_json::Value) -> Message {
        let parsed: InitializeParams = serde_json::from_value(params).unwrap_or_default();

        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(folders) = &parsed.workspace_folders {
            roots.extend(folders.iter().filter_map(|f| uri_to_path(&f.uri).ok()));
        }
        if roots.is_empty() {
            if let Some(root) = &parsed.root_uri {
                if let Ok(path) = uri_to_path(root) {
                    roots.push(path);
                }
            }
        }
        self.workspace.set_roots(roots);
        self.initialized = true;

        let result = InitializeResult {
            capabilities: crate::lsp::ServerCapabilities::for_bridge(),
            server_info: ServerInfo {
                name: "vscode-bridge".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        };

        Message::result(id, serde_json::to_value(result).unwrap_or(json!({})))
    }

    /// `textDocument/documentSymbol` — lista symboli do panelu „Outline”.
    ///
    /// Pusta lista (a nie błąd) dla nieznanego dokumentu: VS Code pyta o
    /// symbole dla plików, których jeszcze nie otworzyliśmy.
    fn document_symbol(&self, message: &Message, params: serde_json::Value) -> Option<Message> {
        let uri = uri_of(&params);
        let Some(document) = self.workspace.document(&uri) else {
            return Some(Message::result(
                message.id().cloned().unwrap_or(json!(null)),
                json!([]),
            ));
        };

        let symbole = symbols::document_symbols(document);
        Some(Message::result(
            message.id().cloned().unwrap_or(json!(null)),
            serde_json::to_value(symbole).unwrap_or(json!([])),
        ))
    }

    /// `workspace/executeCommand` — komendy zadeklarowane w [`COMMANDS`].
    fn execute_command<T: std::io::BufRead>(
        &mut self,
        message: &Message,
        params: serde_json::Value,
        host: &mut Host<'_, T>,
    ) -> Result<Option<Message>> {
        let id = message.id().cloned().unwrap_or(json!(null));
        let command = params
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_owned();
        let argumenty = params
            .get("arguments")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default();

        match command.as_str() {
            "vscodeBridge.status" => {
                let liczba = self.workspace.len();
                host.show_information(&format!("vscode-bridge: śledzonych plików — {liczba}."))?;
            }
            "vscodeBridge.reanalyzeAll" => self.publish_all(host),
            "vscodeBridge.reanalyzeFile" => {
                let uri = argumenty
                    .first()
                    .and_then(|a| a.get("uri"))
                    .and_then(|u| u.as_str())
                    .unwrap_or_default()
                    .to_owned();
                self.publish_for(&uri, host)?;
            }
            other => {
                return Ok(Some(Message::failure(
                    id,
                    -32601,
                    format!("serwer nie zna komendy `{other}`"),
                )));
            }
        }

        Ok(Some(Message::result(id, json!(null))))
    }

    /// `textDocument/didOpen`.
    fn did_open<T: std::io::BufRead>(
        &mut self,
        params: serde_json::Value,
        host: &mut Host<'_, T>,
    ) -> Result<Option<Message>> {
        let Ok(parsed) = serde_json::from_value::<DidOpenParams>(params) else {
            return Ok(None);
        };

        let item = parsed.text_document;
        let language = Language::from_id(&item.language_id);
        let mut document = Document::new(&item.uri, language, item.version);
        document.set_text(item.text);
        let uri = item.uri;
        self.workspace.upsert(document);

        self.publish_for(&uri, host)?;
        Ok(None)
    }

    /// `textDocument/didChange` — obsługujemy wariant pełnego tekstu.
    fn did_change<T: std::io::BufRead>(
        &mut self,
        params: serde_json::Value,
        host: &mut Host<'_, T>,
    ) -> Result<Option<Message>> {
        let Ok(parsed) = serde_json::from_value::<DidChangeParams>(params) else {
            return Ok(None);
        };
        let uri = parsed.text_document.uri;
        let version = parsed.text_document.version;

        let Some(tekst) = parsed
            .content_changes
            .into_iter()
            .rev()
            .find_map(|change| change.text)
        else {
            return Ok(None); // wariant przyrostkowy — poza zakresem tego serwera
        };

        let Some(document) = self.workspace.document_mut(&uri) else {
            return Ok(None);
        };
        document.set_text(tekst);
        document.set_version(version);

        self.publish_for(&uri, host)?;
        Ok(None)
    }

    /// `textDocument/didClose` — usuwamy dokument i czyścimy jego diagnostykę.
    fn did_close<T: std::io::BufRead>(
        &mut self,
        params: serde_json::Value,
        host: &mut Host<'_, T>,
    ) -> Result<Option<Message>> {
        let Ok(parsed) = serde_json::from_value::<DidCloseParams>(params) else {
            return Ok(None);
        };
        let uri = parsed.text_document.uri;
        self.workspace.remove(&uri);
        self.publish(&uri, Vec::new(), host)?;
        Ok(None)
    }

    /// Analizuje dokument i wysyła jego diagnostykę.
    fn publish_for<T: std::io::BufRead>(
        &mut self,
        uri: &str,
        host: &mut Host<'_, T>,
    ) -> Result<()> {
        match self.workspace.document(uri) {
            Some(document) => {
                let diagnostyki = analysis::analyze(document);
                self.publish(uri, diagnostyki, host)
            }
            None => Ok(()),
        }
    }

    /// Wysyła `textDocument/publishDiagnostics`.
    fn publish<T: std::io::BufRead>(
        &mut self,
        uri: &str,
        diagnostyki: Vec<Diagnostic>,
        host: &mut Host<'_, T>,
    ) -> Result<()> {
        // Pusta lista nie znaczy „brak błędów”, tylko „zetrzyj podkreślenia” —
        // dlatego wysyłamy ją też po `didClose`.
        host.notify(
            "textDocument/publishDiagnostics",
            json!({ "uri": uri, "diagnostics": diagnostyki }),
        )
    }

    /// Ponowna analiza wszystkich otwartych dokumentów.
    fn publish_all<T: std::io::BufRead>(&mut self, host: &mut Host<'_, T>) {
        let uri: Vec<String> = self.workspace.uris().map(str::to_owned).collect();
        for uri in uri {
            let _ = self.publish_for(&uri, host);
        }
    }
}

/// Wyciąga URI z parametrów metod dokumentu.
fn uri_of(params: &serde_json::Value) -> String {
    params
        .get("textDocument")
        .and_then(|t| t.get("uri"))
        .and_then(|u| u.as_str())
        .unwrap_or_default()
        .to_owned()
}

/// Odpowiedź „nie znam takiej metody” — tylko dla żądań z `id`.
fn nieznana_metoda(message: &Message, method: &str) -> Option<Message> {
    message.id().map(|id| {
        Message::failure(
            id.clone(),
            -32601,
            format!("metoda `{method}` nie jest obsługiwana"),
        )
    })
}
