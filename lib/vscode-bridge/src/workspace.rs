//! Obsługa URI, ścieżek i magazyn otwartych dokumentów.
//!
//! VS Code mówi o plikach przez `file:///…` (URI), a reszta świata oczekuje
//! ścieżek. Ten moduł jest granicą między jednym a drugim — i jednocześnie
//! magazynem dokumentów, na których pracuje analizator.
//!
//! ```
//! use vscode_bridge::lsp::Position;
//! use vscode_bridge::workspace::{Document, Language, path_to_uri, uri_to_path};
//!
//! let mut d = Document::new("file:///home/x/agh.rs", Language::Rust, 1);
//! d.set_text("fn main() {}\n".to_owned());
//!
//! assert_eq!(d.line_count(), 2);
//! assert_eq!(d.offset_of(Position::new(0, 3)), 3);
//! assert_eq!(uri_to_path(&path_to_uri(std::path::Path::new("/a.rs"))).unwrap(),
//!            std::path::Path::new("/a.rs"));
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::lsp::{Position, Range};

/// Zamienia ścieżkę w URI `file://…` (spacje, `#` i `?` escapowane).
///
/// ```
/// use std::path::Path;
/// use vscode_bridge::workspace::path_to_uri;
///
/// assert_eq!(path_to_uri(Path::new("/home/x/agh.rs")), "file:///home/x/agh.rs");
/// assert_eq!(path_to_uri(Path::new("/tmp/a b.rs")), "file:///tmp/a%20b.rs");
/// ```
pub fn path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let escapowana: String = text
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_owned(),
            '#' => "%23".to_owned(),
            '?' => "%3F".to_owned(),
            c if (c as u32) < 0x20 => format!("%{:02X}", c as u32),
            c => c.to_string(),
        })
        .collect();

    format!("file://{escapowana}")
}

/// Zamienia URI `file://…` na ścieżkę; inne schematy to błąd.
pub fn uri_to_path(uri: &str) -> Result<PathBuf> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| Error::Protocol(format!("nieobsługiwany schemat URI: `{uri}`")))?;

    // Dekodujemy tylko `%XX` — pełne dekodowanie URL-i wymagałoby zależności.
    let mut out = Vec::with_capacity(rest.len());
    let bajty = rest.as_bytes();
    let mut i = 0;
    while i < bajty.len() {
        if bajty[i] == b'%' && i + 2 < bajty.len() {
            let hex = std::str::from_utf8(&bajty[i + 1..i + 3]).unwrap_or("");
            match u8::from_str_radix(hex, 16) {
                Ok(b) => {
                    out.push(b);
                    i += 3;
                    continue;
                }
                Err(_) => return Err(Error::Protocol(format!("zły escape w URI: `{uri}`"))),
            }
        }
        out.push(bajty[i]);
        i += 1;
    }

    let tekst =
        String::from_utf8(out).map_err(|e| Error::Protocol(format!("URI nie jest UTF-8: {e}")))?;
    Ok(PathBuf::from(tekst))
}

/// Język dokumentu — to, co VS Code nazywa `languageId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Language {
    /// Rust (`.rs`).
    Rust,
    /// TypeScript (`.ts`, `.tsx`).
    TypeScript,
    /// JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`).
    JavaScript,
    /// Wszystko inne — analizator milczy, ale serwer działa.
    Inne,
}

impl Language {
    /// Rozpoznaje język z identyfikatora VS Code.
    pub fn from_id(id: &str) -> Self {
        match id {
            "rust" => Self::Rust,
            "typescript" | "typescriptreact" => Self::TypeScript,
            "javascript" | "javascriptreact" => Self::JavaScript,
            _ => Self::Inne,
        }
    }

    /// Rozpoznaje język z rozszerzenia pliku.
    pub fn from_path(path: &Path) -> Self {
        let rozszerzenie = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match rozszerzenie.as_str() {
            "rs" => Self::Rust,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            _ => Self::Inne,
        }
    }

    /// Identyfikator używany przez VS Code.
    pub fn id(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Inne => "plaintext",
        }
    }

    /// Nazwa języka dla człowieka.
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::TypeScript => "TypeScript",
            Self::JavaScript => "JavaScript",
            Self::Inne => "tekst",
        }
    }
}
/// Otwarty dokument w edytorze: treść, wersja, język.
#[derive(Debug, Clone)]
pub struct Document {
    uri: String,
    language: Language,
    version: i64,
    text: String,
    /// Indeks początku każdej linii — przyspiesza `position_of`.
    line_starts: Vec<usize>,
}

impl Document {
    /// Nowy dokument (początkowo pusty).
    pub fn new(uri: impl Into<String>, language: Language, version: i64) -> Self {
        let mut document = Self {
            uri: uri.into(),
            language,
            version,
            text: String::new(),
            line_starts: vec![0],
        };
        document.set_text(String::new());
        document
    }

    /// Podmienia całą treść i przelicza indeks linii.
    pub fn set_text(&mut self, text: String) {
        self.line_starts = vec![0];
        for (i, c) in text.char_indices() {
            if c == '\n' {
                self.line_starts.push(i + 1);
            }
        }
        self.text = text;
    }

    /// Identyfikator URI.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Ścieżka pliku (dla `file://`) albo URI, gdy to inny schemat.
    pub fn uri_path(&self) -> String {
        uri_to_path(&self.uri)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| self.uri.clone())
    }

    /// Język dokumentu.
    pub fn language(&self) -> Language {
        self.language
    }

    /// Wersja dokumentu.
    pub fn version(&self) -> i64 {
        self.version
    }

    /// Ustawia wersję (np. po `didChange`).
    pub fn set_version(&mut self, version: i64) {
        self.version = version;
    }

    /// Pełna treść.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Liczba linii (plik kończący się `\n` ma dodatkową, pustą linię).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Koniec **zawartości** linii — bez znaków końca linii.
    ///
    /// Pozycja za końcem zawartości wskazuje w LSP na znak `0` następnej
    /// linii, a nie na `koniec` bieżącej — dlatego `offset_of` przycina właśnie
    /// do tego miejsca.
    fn line_end(&self, linia: usize) -> usize {
        let start = self.line_starts[linia];
        let mut end = self
            .line_starts
            .get(linia + 1)
            .copied()
            .unwrap_or(self.text.len());

        let bajty = self.text.as_bytes();
        if end > start && bajty[end - 1] == b'\n' {
            end -= 1;
        }
        if end > start && bajty[end - 1] == b'\r' {
            end -= 1;
        }
        end
    }

    /// Przekłada pozycję LSP na przesunięcie w **bajtach** tekstu.
    ///
    /// `position.character` liczy **jednostki UTF-16**, a nie znaki i nie bajty —
    /// tak właśnie liczy kolumny VS Code. Dla polskich znaków (1 znak = 1
    /// jednostka) zgadza się to ze znakami, ale dla emoji (😀 = 2 jednostki)
    /// różnica jest już widoczna, więc przechodzimy po `char` i liczymy
    /// `len_utf16`.
    ///
    /// Pozycje wskazujące w środek pary zastępczej (czyli na połowę emoji)
    /// przycinamy do jej początku — to jedyna liczba całkowita, która nie
    /// rozcina znaku. Pozycje spoza dokumentu przycinamy do końca linii:
    /// VS Code potrafi wysłać znak za końcem pliku i serwer nie może na tym paść.
    pub fn offset_of(&self, position: Position) -> usize {
        let linia = (position.line as usize).min(self.line_starts.len().saturating_sub(1));
        let start = self.line_starts[linia];
        let koniec = self.line_end(linia);

        let mut jednostek = 0usize;
        for (i, znak) in self.text[start..koniec].char_indices() {
            if jednostek >= position.character as usize {
                return start + i;
            }
            jednostek += znak.len_utf16();
        }
        koniec
    }

    /// Przekłada przesunięcie w bajtach na pozycję LSP (liczoną w UTF-16).
    pub fn position_of(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let linia = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let start = self.line_starts[linia];
        let jednostki: usize = self.text[start..offset].chars().map(char::len_utf16).sum();
        Position::new(linia as u32, jednostki as u32)
    }

    /// Tekst linii (bez `\n`).
    pub fn line_text(&self, line: usize) -> Option<&str> {
        let start = *self.line_starts.get(line)?;
        let end = self.line_end(line);
        Some(self.text[start..end].trim_end_matches(['\n', '\r']))
    }

    /// Zasięg obejmujący całą linię (używany przez analizator).
    pub fn line_range(&self, line: usize) -> Option<Range> {
        let start = *self.line_starts.get(line)?;
        let end = self.line_end(line);
        Some(Range::new(self.position_of(start), self.position_of(end)))
    }

    /// Przesunięcie w bajtach, od którego zaczyna się wiersz.
    ///
    /// Analizador zna kolumnę jako przesunięcie w wierszu, a nie jako
    /// pozycję LSP — ta funkcja jest pomostem między oboma liczbami.
    pub fn line_start_offset(&self, line: usize) -> usize {
        self.line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len())
    }
}

/// Magazyn dokumentów całego okna edytora.
#[derive(Debug, Default)]
pub struct Workspace {
    documents: BTreeMap<String, Document>,
    roots: Vec<PathBuf>,
}

impl Workspace {
    /// Pusty obszar roboczy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ustawia katalogi projektu (z `initialize`).
    pub fn set_roots(&mut self, roots: Vec<PathBuf>) {
        self.roots = roots;
    }

    /// Katalogi projektu.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Dodaje lub podmienia dokument.
    pub fn upsert(&mut self, document: Document) {
        self.documents.insert(document.uri().to_owned(), document);
    }

    /// Usuwa dokument (po `didClose`).
    pub fn remove(&mut self, uri: &str) -> Option<Document> {
        self.documents.remove(uri)
    }

    /// Dokument po URI.
    pub fn document(&self, uri: &str) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Mutowalny dokument po URI.
    pub fn document_mut(&mut self, uri: &str) -> Option<&mut Document> {
        self.documents.get_mut(uri)
    }

    /// Liczba otwartych dokumentów.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Czy nie ma żadnych dokumentów.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Wszystkie URI posortowane (determinizm przy testach).
    pub fn uris(&self) -> impl Iterator<Item = &str> {
        self.documents.keys().map(String::as_str)
    }

    /// Dokument po ścieżce pliku.
    pub fn by_path(&self, path: &Path) -> Option<&Document> {
        self.documents.get(&path_to_uri(path))
    }

    /// Wymusza błąd, gdy dokumentu nie ma.
    pub fn require(&self, uri: &str) -> Result<&Document> {
        self.documents
            .get(uri)
            .ok_or_else(|| Error::UnknownDocument(uri.to_owned()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_i_sciezka_dokladnie_so_zgadzaja() {
        for sciezka in ["/home/x/agh.rs", "/tmp/a b.rs", "/z/o/że/ł.pl"] {
            let uri = path_to_uri(Path::new(sciezka));
            let z_powrotem = uri_to_path(&uri).unwrap();
            assert_eq!(
                z_powrotem,
                PathBuf::from(sciezka),
                "round-trip dla `{sciezka}`"
            );
        }
    }

    #[test]
    fn uri_ze_znakiem_hash_jest_escapowany() {
        let uri = path_to_uri(Path::new("/tmp/a#b.rs"));
        assert!(uri.ends_with("a%23b.rs"));
        assert_eq!(uri_to_path(&uri).unwrap(), PathBuf::from("/tmp/a#b.rs"));
    }

    #[test]
    fn obcy_schemat_to_blad() {
        assert!(uri_to_path("http://example.com/x").is_err());
        assert!(uri_to_path("file:///ok").is_ok());
        assert!(uri_to_path("file:///z%C0%F3").is_err(), "zły escape");
    }

    #[test]
    fn jezyk_po_rozszerzeniu_i_id() {
        assert_eq!(Language::from_path(Path::new("a/b.rs")), Language::Rust);
        assert_eq!(
            Language::from_path(Path::new("a/b.TSX")),
            Language::TypeScript
        );
        assert_eq!(
            Language::from_path(Path::new("a/b.mjs")),
            Language::JavaScript
        );
        assert_eq!(Language::from_path(Path::new("a/b.md")), Language::Inne);
        assert_eq!(Language::from_id("typescriptreact"), Language::TypeScript);
        assert_eq!(Language::Rust.id(), "rust");
        assert_eq!(Language::JavaScript.name(), "JavaScript");
    }

    #[test]
    fn pozycje_przechodza_w_obie_strony() {
        let mut d = Document::new("file:///a.rs", Language::Rust, 1);
        d.set_text("fn main() {\n    println!();\n}\n".to_owned());

        assert_eq!(d.line_count(), 4);
        assert_eq!(d.line_text(1), Some("    println!();"));
        assert_eq!(d.line_text(9), None);

        let pozycja = Position::new(1, 4);
        assert_eq!(d.position_of(d.offset_of(pozycja)), pozycja);
    }

    #[test]
    fn pozycje_spoza_pliku_sa_przycinane() {
        let mut d = Document::new("file:///a.rs", Language::Rust, 1);
        d.set_text("ab\ncd".to_owned());

        // Znak za końcem zawartości linii wskazuje na jej koniec.
        assert_eq!(
            d.offset_of(Position::new(0, 99)),
            2,
            "przycięcie do końca linii"
        );
        // Linia za końcem pliku to ostatnia linia.
        assert_eq!(
            d.offset_of(Position::new(99, 0)),
            3,
            "przycięcie do ostatniej linii"
        );
        // Przesunięcie za plikiem to koniec pliku.
        assert_eq!(d.position_of(9999), Position::new(1, 2));
    }

    #[test]
    fn zasiag_linii_obejmuje_cala_linia() {
        let mut d = Document::new("file:///a.rs", Language::Rust, 1);
        d.set_text("ab\ncdef\n".to_owned());
        let r = d.line_range(1).unwrap();
        assert_eq!(r.start, Position::new(1, 0));
        // Koniec linii to 4 znaki — znak `\n` nie należy do zasięgu.
        assert_eq!(r.end, Position::new(1, 4));
    }

    #[test]
    fn magazyn_dokumentow_dodaje_i_usuwa() {
        let mut w = Workspace::new();
        assert!(w.is_empty());

        let mut d = Document::new("file:///a.rs", Language::Rust, 1);
        d.set_text("fn a() {}".to_owned());
        w.upsert(d);

        assert_eq!(w.len(), 1);
        assert!(w.document("file:///a.rs").is_some());
        assert!(w.by_path(Path::new("/a.rs")).is_some());
        assert_eq!(w.uris().collect::<Vec<_>>(), vec!["file:///a.rs"]);

        assert!(w.require("file:///brak.rs").is_err());
        assert!(w.remove("file:///a.rs").is_some());
        assert!(w.is_empty());
    }

    #[test]
    fn katalogi_robocze_sa_zapamietywane() {
        let mut w = Workspace::new();
        w.set_roots(vec![PathBuf::from("/projekt")]);
        assert_eq!(w.roots(), [PathBuf::from("/projekt")]);
    }

    #[test]
    fn dokument_zna_swoja_sciezke() {
        let mut d = Document::new("file:///home/x/agh.rs", Language::Rust, 1);
        d.set_text(String::new());
        assert_eq!(d.uri_path(), "/home/x/agh.rs");
        assert_eq!(d.language().id(), "rust");
        assert_eq!(d.version(), 1);
    }
}
