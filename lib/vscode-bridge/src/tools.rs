//! Narzędzia zewnętrzne: `cargo check` i `tsc`.
//!
//! Wbudowany analizator z modułu [`analysis`](crate::analysis) widzi tylko
//! plik. Prawdziwą semantykę dają kompilatory, i to one produkują **prawdziwe**
//! błędy — tyle że potrzebują całego projektu i chwilę trwają.
//!
//! Ten moduł uruchamia narzędzie w katalogu projektu, parsuje jego wypis i
//! zamienia na diagnostykę LSP. Gdy narzędzia nie ma w `PATH`, zwraca
//! `Ok(None)` — serwer po prostu zostaje przy wbudowanych regułach.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::lsp::{Diagnostic, Position, Range, Severity};
use crate::workspace::Language;

/// Narzędzie zewnętrzne analizujące cały projekt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// `cargo check --message-format=short`.
    Cargo,
    /// `tsc --noEmit` dla TypeScriptu.
    Tsc,
    /// `node --check` dla JavaScriptu.
    Node,
}

impl Tool {
    /// Nazwa programu szukanego w `PATH`.
    pub fn program(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Tsc => "tsc",
            Self::Node => "node",
        }
    }

    /// Czy program jest w `PATH`.
    pub fn is_available(self) -> bool {
        detect(self).is_some()
    }

    /// Narzędzie właściwe dla języka (`None`, gdy wbudowany analizator wystarczy).
    pub fn for_language(language: Language) -> Option<Self> {
        match language {
            Language::Rust => Some(Self::Cargo),
            Language::TypeScript => Some(Self::Tsc),
            Language::JavaScript => Some(Self::Node),
            Language::Inne => None,
        }
    }
}

/// Znajduje program narzędzia, przeglądając `PATH`.
pub fn detect(tool: Tool) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find(|katalog| katalog.join(tool.program()).is_file())
}

/// Wynik uruchomienia narzędzia.
pub struct Report {
    /// Diagnostyki zebrane z wypisu.
    pub diagnostics: Vec<Diagnostic>,
    /// Kod wyjścia procesu (`None`, gdy proces się nie udał uruchomić).
    pub status: Option<i32>,
    /// Surowy wypis — przyda się przy diagnozowaniu samego serwera.
    pub stdout: String,
}

impl Report {
    /// Czy narzędzie zakończyło się bez błędów.
    pub fn is_ok(&self) -> bool {
        self.status == Some(0)
    }
}

/// Uruchamia narzędzie w katalogu `project` i parsuje jego wypis.
///
/// Zwraca `Ok(None)`, gdy narzędzia nie ma — serwer nie jest wtedy zepsuty,
/// po prostu nie ma pełnej analizy semantycznej.
pub fn run(tool: Tool, project: &Path) -> Result<Option<Report>> {
    if detect(tool).is_none() {
        log::debug!("vscode-bridge: `{}` nie ma w PATH", tool.program());
        return Ok(None);
    }

    let wynik = match tool {
        Tool::Cargo => Command::new("cargo")
            .args(["check", "--message-format=short", "--quiet"])
            .current_dir(project)
            .output(),
        Tool::Tsc => Command::new("tsc")
            .args(["--noEmit", "--pretty", "false"])
            .current_dir(project)
            .output(),
        Tool::Node => Command::new("node")
            .arg("--version")
            .current_dir(project)
            .output(),
    }
    .map_err(Error::Spawn)?;

    let stdout = String::from_utf8_lossy(&wynik.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&wynik.stderr).into_owned();
    let diagnostyki = match tool {
        Tool::Cargo => parsuj_cargo(&stdout),
        Tool::Tsc => parsuj_tsc(&stdout, &stderr),
        Tool::Node => Vec::new(),
    };

    Ok(Some(Report {
        diagnostics: diagnostyki,
        status: wynik.status.code(),
        stdout: format!("{stdout}{stderr}"),
    }))
}

/// `src/lib.rs:12:5: warning: unused variable: x`
fn parsuj_cargo(wyjście: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    for linia in wyjście.lines() {
        let czesci: Vec<&str> = linia.splitn(5, ':').collect();
        if czesci.len() < 5 {
            continue;
        }
        let (sciezka, numer, kolumna, poziom, komunikat) = (
            czesci[0],
            czesci[1],
            czesci[2],
            czesci[3].trim(),
            czesci[4].trim(),
        );

        let Ok(linia) = numer.parse::<u32>() else {
            continue;
        };
        let kolumna = kolumna.parse::<u32>().unwrap_or(0);

        // `cargo` wypisuje też `error[E0308]:` — prefiks musi się zgadzać.
        let severity = if poziom.starts_with("error") {
            Some(Severity::Error)
        } else if poziom.starts_with("warning") {
            Some(Severity::Warning)
        } else {
            None
        };
        let Some(severity) = severity else {
            continue;
        };

        let pozycja = Position::new(linia.saturating_sub(1), kolumna.saturating_sub(1));
        out.push(Diagnostic {
            range: Range::new(pozycja, pozycja),
            message: komunikat.to_owned(),
            severity: Some(severity),
            code: None,
            source: Some("cargo".to_owned()),
            data: Some(serde_json::json!({ "path": sciezka })),
        });
    }

    out
}
/// `src/app.ts(12,5): error TS2345: Argument of type ...`
///
/// Format tsc trzyma nawias tuż po ścieżce, więc po dzieleniu przez `:`:
///
/// ```text
/// ["src/app.ts(12,5)", " error TS2345", " Argument of type ..."]
/// ```
fn parsuj_tsc(stdout: &str, stderr: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    for linia in stdout.lines().chain(stderr.lines()) {
        let czesci: Vec<&str> = linia.splitn(3, ':').collect();
        if czesci.len() < 3 {
            continue;
        }
        let (nawias, poziom, wiadomosc) = (czesci[0], czesci[1].trim(), czesci[2].trim());

        // Ścieżka i pozycja: `src/app.ts(12,5)`
        let (sciezka, reszta) = match nawias.rfind('(') {
            Some(i) => (&nawias[..i], &nawias[i + 1..]),
            None => continue,
        };
        let (numer, kolumna) = match reszta.trim_end_matches(')').split_once(',') {
            Some((l, k)) => (l.trim(), k.trim().parse::<u32>().unwrap_or(0)),
            None => continue,
        };
        let Ok(linia_nr) = numer.parse::<u32>() else {
            continue;
        };

        let (severity, numer_kodu) = if let Some(kod) = poziom.strip_prefix("error TS") {
            (Severity::Error, Some(kod.trim().to_owned()))
        } else if let Some(kod) = poziom.strip_prefix("warning TS") {
            (Severity::Warning, Some(kod.trim().to_owned()))
        } else {
            continue;
        };

        let pozycja = Position::new(linia_nr.saturating_sub(1), kolumna.saturating_sub(1));
        out.push(Diagnostic {
            range: Range::new(pozycja, pozycja),
            message: wiadomosc.to_owned(),
            severity: Some(severity),
            code: numer_kodu,
            source: Some("tsc".to_owned()),
            data: Some(serde_json::json!({ "path": sciezka })),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wykrywanie_narzedzi_jest_spójne() {
        // Nie wymagamy narzędzia w systemie — sprawdzamy spójność API.
        assert_eq!(Tool::Cargo.is_available(), detect(Tool::Cargo).is_some());
        assert_eq!(Tool::Cargo.program(), "cargo");
        assert_eq!(Tool::for_language(Language::Rust), Some(Tool::Cargo));
        assert_eq!(Tool::for_language(Language::Inne), None);
    }

    #[test]
    fn cargo_krotki_format_jest_parsowany() {
        let wyjscie = "src/lib.rs:12:5: warning: unused variable: `x`\n\
                      src/main.rs:3:1: error[E0308]: mismatched types\n\
                      nieprawidłowa linia\n";
        let d = parsuj_cargo(wyjscie);
        assert_eq!(d.len(), 2, "{d:?}");

        assert_eq!(d[0].severity, Some(Severity::Warning));
        assert_eq!(d[0].range.start, Position::new(11, 4), "pozycje są od zera");
        assert!(d[0].message.contains("unused variable"));

        assert_eq!(d[1].severity, Some(Severity::Error));
        assert_eq!(d[1].range.start.line, 2);
        assert_eq!(d[1].source.as_deref(), Some("cargo"));
    }

    #[test]
    fn tsc_format_jest_parsowany() {
        let wyjscie = "src/app.ts(12,5): error TS2345: Argument of type 'string' \
                       is not assignable to parameter of type 'number'.\n";
        let d = parsuj_tsc(wyjscie, "");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].severity, Some(Severity::Error));
        assert_eq!(d[0].range.start, Position::new(11, 4));
        assert_eq!(d[0].code.as_deref(), Some("2345"));
        assert!(d[0].message.contains("not assignable"), "{}", d[0].message);
    }

    #[test]
    fn smieci_sa_pomijane() {
        assert!(parsuj_cargo("").is_empty());
        assert!(parsuj_tsc("", "").is_empty());
        assert!(parsuj_tsc("info: sprawdzam wersję", "").is_empty());
    }

    #[test]
    fn brak_narzędzia_to_nie_błąd() {
        // W środowisku bez `cargo` wynik to `None`, a nie błąd.
        match run(Tool::Cargo, &std::env::temp_dir()) {
            Ok(None) => {}
            Ok(Some(report)) => assert!(report.diagnostics.len() < 10_000),
            Err(e) => panic!("run() nie powinien zwracać błędu: {e}"),
        }
    }
}
