//! Raport: każda linia idzie na ekran i do pliku.
//!
//! Zapis po każdej linii, więc plik jest kompletny nawet gdy program
//! przerwie się po drodze.

use std::fmt;
use std::path::{Path, PathBuf};

/// Raport: ekran + plik w jednym.
pub struct Report {
    path: PathBuf,
    buffer: String,
}

impl Report {
    /// Nowy raport zapisywany do `path`.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            buffer: String::new(),
        }
    }

    /// Drukuje linię na ekran i dopisuje ją do pliku.
    pub fn line(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        println!("{text}");
        self.buffer.push_str(text);
        self.buffer.push('\n');
        if let Err(error) = std::fs::write(&self.path, &self.buffer) {
            eprintln!(
                "uwaga: nie udało się zapisać {}: {error}",
                self.path.display()
            );
        }
    }

    /// Drukuje sformatowaną linię.
    pub fn say(&mut self, args: fmt::Arguments<'_>) {
        let text = format!("{args}");
        self.line(text);
    }

    /// Ścieżka pliku raportu.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
