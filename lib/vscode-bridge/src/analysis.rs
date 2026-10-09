//! Wbudowany analizator: diagnostyka bez zewnętrznych serwerów.
//!
//! VS Code ma wbudowanego dostawcę diagnostyki dla TypeScriptu (`tsserver`)
//! i jest słaby dla Rusta, dopóki nie ma `rust-analyzer`. Ten moduł daje
//! **coś zawsze**:
//!
//! * błędy składniowe wykrywane skanerem nawiasów (pominającym napisy),
//! * ostrzeżenia specyficzne dla języka: `debugger`, `var`, `== null`,
//!   `use` wewnątrz funkcji, `#[derive(Debug)]` bez `use std::…`,
//! * pozycje w formacie LSP, czyli gotowe do podkreślenia w edytorze.
//!
//! Reguły celowo **nie** udają pełnej analizy semantycznej — to pierwsza warstwa,
//! która działa natychmiast i offline. Dokładną semantykę dokładają narzędzia
//! z modułu [`tools`](crate::tools) (`cargo check`, `tsc`).
//!
//! ```
//! use vscode_bridge::analysis::analyze;
//! use vscode_bridge::lsp::{Position, Severity};
//! use vscode_bridge::workspace::{Document, Language};
//!
//! let mut d = Document::new("file:///a.rs", Language::Rust, 1);
//! d.set_text("fn main() {\n".to_owned());
//!
//! let bledy = analyze(&d);
//! assert_eq!(bledy[0].severity, Some(Severity::Error));
//! // Klamra w `fn main() {` stoi na 11. znaku.
//! assert_eq!(bledy[0].range.start, Position::new(0, 10));
//! ```

use crate::lsp::{Diagnostic, Range, Severity};
use crate::workspace::{Document, Language};

/// Podpis diagnostyk w polu `source`.
const SOURCE: &str = "vscode-bridge";

/// Analizuje dokument i zwraca listę problemów.
///
/// Pusta lista oznacza „nie znamy problemów”, a nie „plik jest poprawny” —
/// pełną ocenę daje `cargo check` albo `tsc`.
pub fn analyze(document: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    match document.language() {
        Language::Rust => rust::analyze(document, &mut out),
        Language::TypeScript | Language::JavaScript => script::analyze(document, &mut out),
        Language::Inne => {}
    }

    for diagnostic in &mut out {
        diagnostic.source = Some(SOURCE.to_owned());
    }
    out
}

/// Zasięg jednego znaku na podstawie indeksu w tekście.
fn znak_na_zakres(document: &Document, offset: usize) -> Range {
    let dlugosc = document.text()[offset..]
        .chars()
        .next()
        .map_or(1, char::len_utf8);
    Range::new(
        document.position_of(offset),
        document.position_of(offset + dlugosc),
    )
}

/// Zasięg fragmentu wiersza: `linia`, `od` i `dlugosc` to liczby **bajtów**
/// od początku wiersza.
///
/// Kolumny przeliczamy na jednostki UTF-16 przez [`Document::position_of`],
/// bo tego oczekuje VS Code — inaczej podkreślenie po emoji przesuwa się
/// o jeden znak w lewo.
fn wiersz_na_zakres(document: &Document, linia: usize, od: usize, dlugosc: usize) -> Range {
    let start = document.line_start_offset(linia) + od;
    Range::new(
        document.position_of(start),
        document.position_of(start + dlugosc),
    )
}

/// Nawias zamykający i jego odpowiednik.
fn dopasowanie(znak: char) -> char {
    match znak {
        ')' => '(',
        '}' => '{',
        _ => '[',
    }
}

/// Zlicza diagnostyki: `(błędy, ostrzeżenia i informacje)`.
pub fn zlicz(diagnostyki: &[Diagnostic]) -> (usize, usize) {
    let bledy = diagnostyki
        .iter()
        .filter(|d| d.severity == Some(Severity::Error))
        .count();
    (bledy, diagnostyki.len() - bledy)
}
/// Reguły dla Rusta.
mod rust {
    use super::*;

    /// Typy z biblioteki standardowej — użyte w `derive` bez `use`.
    const STD_TRAITS: &[&str] = &["Debug", "Clone", "Copy", "Default", "PartialEq", "Iterator"];

    /// Analiza pliku Rust.
    pub fn analyze(document: &Document, out: &mut Vec<Diagnostic>) {
        nawiasy(document, out);
        debugger(document, out);
        use_w_funkcji(document, out);
        typy_bez_use(document, out);
    }

    /// Niezamknięty lub nadmiarowy nawias.
    ///
    /// Skaner pomija zawartość napisów, bo `"}"` w stringu nie zamyka żadnego
    /// nawiasu — bez tego każdy `println!("}")` dawałby fałszywy błąd.
    fn nawiasy(document: &Document, out: &mut Vec<Diagnostic>) {
        let mut stos: Vec<(char, usize)> = Vec::new();
        let mut w_stryngu = false;
        let mut escapowany = false;

        for (offset, znak) in document.text().char_indices() {
            if w_stryngu {
                if escapowany {
                    escapowany = false;
                } else if znak == '\\' {
                    escapowany = true;
                } else if znak == '"' {
                    w_stryngu = false;
                }
                continue;
            }

            match znak {
                '"' => w_stryngu = true,
                '(' | '{' | '[' => stos.push((znak, offset)),
                ')' | '}' | ']' => {
                    let oczekiwany = dopasowanie(znak);
                    match stos.last() {
                        Some(&(otwarty, _)) if otwarty == oczekiwany => {
                            stos.pop();
                        }
                        _ => out.push(Diagnostic::error(
                            znak_na_zakres(document, offset),
                            format!("nadmiarowy `{znak}`"),
                        )),
                    }
                }
                _ => {}
            }
        }

        for (znak, offset) in stos {
            out.push(Diagnostic::error(
                znak_na_zakres(document, offset),
                format!("niezamknięty `{znak}`"),
            ));
        }
    }

    /// `debugger;` w kodzie produkcyjnym.
    fn debugger(document: &Document, out: &mut Vec<Diagnostic>) {
        for linia in 0..document.line_count() {
            let tekst = document.line_text(linia).unwrap_or_default();
            if let Some(kolumna) = tekst.find("debugger;") {
                out.push(Diagnostic::warning(
                    wiersz_na_zakres(document, linia, kolumna, 9),
                    "`debugger;` zatrzymuje program w przeglądarce — usuń przed commitem",
                ));
            }
        }
    }

    /// `use` wewnątrz funkcji — import musi być na poziomie modułu.
    fn use_w_funkcji(document: &Document, out: &mut Vec<Diagnostic>) {
        for linia in 0..document.line_count() {
            let tekst = document.line_text(linia).unwrap_or_default();
            let przycięty = tekst.trim_start();
            let wcięcie = tekst.len() - przycięty.len();
            if wcięcie == 0 {
                continue;
            }
            if przycięty.starts_with("use ") || przycięty.starts_with("pub use ") {
                out.push(Diagnostic::warning(
                    wiersz_na_zakres(document, linia, wcięcie, przycięty.len()),
                    "`use` wewnątrz funkcji — importy należą do modułu",
                ));
            }
        }
    }

    /// Wyliczone typy z `std` bez `use` — klasyka pierwszego dnia z Rustem.
    fn typy_bez_use(document: &Document, out: &mut Vec<Diagnostic>) {
        let tekst = document.text();
        if !tekst.contains("#[derive(") {
            return;
        }
        for nazwa in STD_TRAITS {
            // `Debug` w `#[derive(Debug)]` nie jest zakończone przecinkiem,
            // więc szukamy nazwy z `,` albo zamykającą klamrą.
            let znalezione = tekst
                .find(&format!("{nazwa},"))
                .or_else(|| tekst.find(&format!("#[derive({nazwa})")));
            let Some(start) = znalezione else {
                continue;
            };
            if tekst[..start].contains("use std::") {
                continue;
            }
            let linia = tekst[..start].matches('\n').count();
            let kolumna = texto_start(tekst, start);
            out.push(Diagnostic::warning(
                wiersz_na_zakres(document, linia, kolumna, nazwa.len()),
                format!("`{nazwa}` wymaga `use std::{nazwa}` albo ścieżki `std::`"),
            ));
        }
    }

    /// Kolumna znaku na podstawie przesunięcia w tekście.
    fn texto_start(tekst: &str, offset: usize) -> usize {
        offset - tekst[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }
}
/// Reguły wspólne dla TypeScriptu i JavaScriptu.
mod script {
    use super::*;

    /// Analiza pliku TypeScript / JavaScript.
    pub fn analyze(document: &Document, out: &mut Vec<Diagnostic>) {
        nawiasy(document, out);

        for linia in 0..document.line_count() {
            let tekst = document.line_text(linia).unwrap_or_default();
            let przycięty = tekst.trim_start();
            let wcięcie = tekst.len() - przycięty.len();

            if przycięty.starts_with("var ") {
                out.push(Diagnostic::warning(
                    wiersz_na_zakres(document, linia, wcięcie, przycięty.len()),
                    "`var` ma zasięg funkcji — użyj `let` albo `const`",
                ));
            }

            if przycięty.starts_with("debugger") {
                out.push(Diagnostic::warning(
                    wiersz_na_zakres(document, linia, wcięcie, przycięty.len()),
                    "`debugger` zatrzymuje program — usuń przed commitem",
                ));
            }

            if let Some(kolumna) = tekst.find("== null") {
                out.push(Diagnostic::warning(
                    wiersz_na_zakres(document, linia, kolumna, 8),
                    "`== null` porównuje też `undefined` — użyj `===`",
                ));
            }
        }
    }

    /// Niezamknięty lub nadmiarowy nawias.
    ///
    /// Tu świadomie nie śledzimy napisów: reguła ma być szybka i nie mylić się
    /// o nawias w stringu. Dokładną analizę składni daje `tsc`.
    fn nawiasy(document: &Document, out: &mut Vec<Diagnostic>) {
        let mut stos: Vec<(char, usize)> = Vec::new();

        for (offset, znak) in document.text().char_indices() {
            match znak {
                '(' | '{' | '[' => stos.push((znak, offset)),
                ')' | '}' | ']' => {
                    let oczekiwany = dopasowanie(znak);
                    match stos.last() {
                        Some(&(otwarty, _)) if otwarty == oczekiwany => {
                            stos.pop();
                        }
                        _ => out.push(Diagnostic::error(
                            znak_na_zakres(document, offset),
                            format!("nadmiarowy `{znak}`"),
                        )),
                    }
                }
                _ => {}
            }
        }

        for (znak, offset) in stos {
            out.push(Diagnostic::error(
                znak_na_zakres(document, offset),
                format!("niezamknięty `{znak}`"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::Position;

    /// Analizuje tekst w danym języku — skrót używany w testach.
    fn analizuj(tekst: &str, jezyk: Language) -> Vec<Diagnostic> {
        let mut d = Document::new("file:///x", jezyk, 1);
        d.set_text(tekst.to_owned());
        analyze(&d)
    }

    /// Czy wśród diagnostyk jest komunikat zawierający `fragment`.
    fn jest(d: &[Diagnostic], fragment: &str) -> bool {
        d.iter().any(|x| x.message.contains(fragment))
    }

    #[test]
    fn poprawny_rust_jest_czysty() {
        let d = analizuj(
            "use std::fmt;\n\nfn main() {\n    let x = vec![1, 2];\n    println!(\"{:?}\", x);\n}\n",
            Language::Rust,
        );
        assert!(d.is_empty(), "niespodziewane: {d:?}");
    }

    #[test]
    fn niezamkniety_nawias_w_ruscie() {
        let d = analizuj("fn main() {\n    let x = 1;\n", Language::Rust);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].severity, Some(Severity::Error));
        assert_eq!(d[0].range.start, Position::new(0, 10), "klamra z linii 0");
    }

    #[test]
    fn nadmiarowy_nawias_w_ruscie() {
        let d = analizuj("fn main() { } }\n", Language::Rust);
        assert!(jest(&d, "nadmiarowy"), "{d:?}");
    }

    #[test]
    fn nawiasy_w_napisach_sa_pominiete() {
        // Gdyby skaner patrzył na `"}"`, zgłosiłby fałszywy błąd.
        let d = analizuj(r#"fn main() { println!("}}"); }"#, Language::Rust);
        assert!(d.is_empty(), "niespodziewane: {d:?}");
    }

    #[test]
    fn debugger_jest_ostrzezeniem() {
        let d = analizuj("fn main() {\n    debugger;\n}\n", Language::Rust);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].severity, Some(Severity::Warning));
        assert_eq!(d[0].range.start, Position::new(1, 4));
    }
    #[test]
    fn use_w_funkcji_jest_ostrzezeniem() {
        let d = analizuj("fn main() {\n    use std::fmt;\n}\n", Language::Rust);
        assert!(jest(&d, "`use` wewnątrz funkcji"), "{d:?}");
    }

    #[test]
    fn use_na_poziomie_modulu_jest_ok() {
        let d = analizuj("use std::fmt::Debug;\nfn main() {}\n", Language::Rust);
        assert!(d.is_empty(), "niespodziewane: {d:?}");
    }

    #[test]
    fn derive_bez_use_std() {
        let d = analizuj("#[derive(Debug)]\nstruct P;\n", Language::Rust);
        assert!(jest(&d, "wymaga `use std::"), "{d:?}");
    }

    #[test]
    fn derive_z_uzytym_use_std_jest_czyste() {
        let d = analizuj(
            "use std::fmt::Debug;\n#[derive(Debug)]\nstruct P;\n",
            Language::Rust,
        );
        assert!(d.is_empty(), "niespodziewane: {d:?}");
    }

    #[test]
    fn javascript_var_i_debugger() {
        let d = analizuj(
            "var x = 1;\nfunction f() {\n  debugger;\n}\n",
            Language::JavaScript,
        );
        assert_eq!(d.len(), 2, "{d:?}");
        assert!(d.iter().all(|x| x.severity == Some(Severity::Warning)));
        assert!(jest(&d, "`var` ma zasięg funkcji"));
    }

    #[test]
    fn typescript_dziedziczy_reguly_javascriptu() {
        let d = analizuj(
            "var a = 1;\nif (a == null) {}\ndebugger;\n",
            Language::TypeScript,
        );
        assert_eq!(d.len(), 3, "TS dostaje reguły JS: {d:?}");
        assert!(jest(&d, "`var` ma zasięg funkcji"));
        assert!(jest(&d, "`== null`"));
        assert!(jest(&d, "`debugger`"));
    }

    #[test]
    fn rownosc_luzna_w_js() {
        let d = analizuj("if (a == null) {}\n", Language::JavaScript);
        assert!(jest(&d, "`== null`"), "{d:?}");
    }

    #[test]
    fn nieznany_jezyk_nie_daje_diagnostyk() {
        assert!(analizuj("cokolwiek {", Language::Inne).is_empty());
    }

    #[test]
    fn wszystkie_diagnostyki_maja_zrodlo() {
        let d = analizuj("fn main() {\n    debugger;\n", Language::Rust);
        assert!(d.iter().all(|x| x.source.as_deref() == Some(SOURCE)));
    }

    #[test]
    fn zlicz_dzieli_bledy_od_ostrzezen() {
        let d = analizuj("fn main() {\n    debugger;\n", Language::Rust);
        assert_eq!(zlicz(&d), (1, 1), "jeden błąd i jedno ostrzeżenie");
    }

    #[test]
    fn pozycje_licza_znaki_a_nie_bajty() {
        // `ł` i `ó` to po 2 bajty — zasięg ma wskazywać na znak, nie na bajt.
        let d = analizuj("fn główna() {\n", Language::Rust);
        let ostatni = d.last().unwrap();
        assert_eq!(ostatni.range.start.line, 0);
        assert_eq!(
            ostatni.range.start.character, 12,
            "`{{` jest 13. znakiem wiersza, choć 14. bajtem"
        );
    }
}
