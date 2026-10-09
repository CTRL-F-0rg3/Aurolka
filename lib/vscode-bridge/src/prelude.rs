//! Wygodne importy — `use vscode_bridge::prelude::*;` zamiast listy modułów.
//!
//! ```
//! use vscode_bridge::prelude::*;
//!
//! let mut document = Document::new("file:///a.rs", Language::TypeScript, 1);
//! document.set_text("var x = 1;\n".to_owned());
//!
//! let bledy = analyze(&document);
//! assert_eq!(bledy.len(), 1);
//!
//! let symbole = document_symbols(&document);
//! assert_eq!(symbole[0].name, "x");
//! ```

pub use crate::analysis::{analyze, zlicz};
pub use crate::error::{Error, Result};
pub use crate::host::{Host, LogLevel};
pub use crate::lsp::{Diagnostic, Position, Range, Severity};
pub use crate::protocol::{Message, Request, Response, Transport};
pub use crate::server::{Server, COMMANDS};
pub use crate::symbols::{document_symbols, SymbolInformation};
pub use crate::tools::{detect, Report, Tool};
pub use crate::workspace::{path_to_uri, uri_to_path, Document, Language, Workspace};
