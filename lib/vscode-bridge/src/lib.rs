//! **vscode-bridge** — most między interfejsem Rust a Visual Studio Code.
//!
//! Biblioteka realizuje dwie rzeczy, których w ekosystemie LSP brakuje:
//!
//! * **serwer językowy** dla Rust, TypeScript i JavaScript, który działa
//!   natychmiast i offline — bez `rust-analyzer` i bez `tsserver`, a gdy one są,
//!   można podpiąć je przez [`tools`];
//! * **kanał zwrotny** ([`host`]), dzięki któremu aplikacja napisana w Rustcie
//!   (np. zbudowana na [`aurum`](https://docs.rs/aurum)) prosi edytor o komunikaty,
//!   otwieranie plików, wstawianie tekstu czy uruchamianie komend.
//!
//! # Jak to działa
//!
//! ```text
//! Visual Studio Code ⇄ JSON-RPC 2.0 (Content-Length) ⇄ vscode-bridge ⇄ pliki
//! ```
//!
//! Serwer odbiera `didOpen` / `didChange`, analizuje dokument i wysyła
//! `publishDiagnostics`. Odwrotnie — przez [`Host`] — potrafi poprosić edytor
//! o akcję (`vscode/*`).
//!
//! # Uruchomienie serwera
//!
//! ```bash
//! cargo run -p vscode-ls            # serwer LSP po stdio
//! ```
//!
//! # Podłączenie do projektu
//!
//! ```json
//! { "name": "Rust", "language": "rust", "command": "vscode-ls" }
//! ```
//!
//! # Przykład: serwer w testach (bez VS Code)
//!
//! ```
//! use vscode_bridge::host::Host;
//! use vscode_bridge::protocol::{Message, Transport};
//! use vscode_bridge::server::Server;
//! use std::io::Cursor;
//!
//! let mut transport = Transport::new(Cursor::new(Vec::new()), Vec::new());
//! let mut server = Server::new();
//! let mut host = Host::new(&mut transport, 1);
//!
//! let wiadomosc = Message::notification(
//!     "textDocument/didOpen",
//!     serde_json::json!({ "textDocument": {
//!         "uri": "file:///a.rs",
//!         "languageId": "rust",
//!         "version": 1,
//!         "text": "fn main() {\n",
//!     }}),
//! );
//!
//! server.handle(&wiadomosc, &mut host).unwrap();
//! assert_eq!(server.workspace().len(), 1);
//! ```
//!
//! # Warstwy
//!
//! | Moduł | Rola |
//! |---|---|
//! | [`protocol`] | ramkowanie JSON-RPC 2.0 (`Content-Length`) |
//! | [`lsp`] | typy protokołu: pozycje, zasięgi, diagnostyka |
//! | [`workspace`] | URI ↔ ścieżka, magazyn otwartych dokumentów |
//! | [`analysis`] | wbudowany analizator Rust / TypeScript / JavaScript |
//! | [`symbols`] | symbole dokumentu dla panelu „Outline” |
//! | [`tools`] | delegowanie do `cargo` i `tsc` |
//! | [`host`] | kanał zwrotny: komunikaty, edycje, komendy |
//! | [`server`] | pętla wiadomości i obsługa metod LSP |
//! | [`panel`] | layout paneli liczony `aurum::math` |

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod analysis;
pub mod error;
pub mod host;
pub mod lsp;
pub mod panel;
pub mod prelude;
pub mod protocol;
pub mod server;
pub mod symbols;
pub mod tools;
pub mod workspace;

pub use error::{Error, Result};

/// Wersja biblioteki — trafia do `initialize` i do `window/logMessage`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
