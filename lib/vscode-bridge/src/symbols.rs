//! Symbole dokumentu dla panelu „Outline”.
//!
//! `textDocument/documentSymbol` zasługuje na osobny moduł, bo to jedyna
//! metoda LSP, która **cofa** informacje: klient pyta o strukturę pliku, a my
//! musimy ją odtworzyć z tekstu. Nie budujemy drzewa semantycznego — skanujemy
//! wiersze i rozpoznajemy deklaracje, co wystarcza do nawigacji i zwijania
//! bloków w edytorze.
//!
//! Rodzaje symboli są numerami z `SymbolKind` LSP; te same liczby VS Code
//! mapuje na swoje ikony.
//!
//! ```
//! use vscode_bridge::symbols::document_symbols;
//! use vscode_bridge::workspace::{Document, Language};
//!
//! let mut d = Document::new("file:///a.rs", Language::Rust, 1);
//! d.set_text("fn main() {}\n".to_owned());
//!
//! let symbole = document_symbols(&d);
//! assert_eq!(symbole[0].name, "main");
//! assert_eq!(symbole[0].kind, 12, "12 = Function");
//! ```

use serde::{Deserialize, Serialize};

use crate::lsp::{Position, Range};
use crate::workspace::{Document, Language};

/// Symbol deklaracji (`SymbolKind` LSP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInformation {
    /// Nazwa deklaracji.
    pub name: String,
    /// Rodzaj — numer `SymbolKind` LSP.
    pub kind: u8,
    /// Cały blok (tu: linia deklaracji razem z `{`).
    pub range: Range,
    /// Sama nazwa — to ją podkreśla VS Code po najechaniu.
    pub selection_range: Range,
}

/// Kody `SymbolKind`, których używamy (zgodne ze specyfikacją LSP).
mod kind {
    /// `SymbolKind.Module`.
    pub const MODULE: u8 = 2;
    /// `SymbolKind.Namespace`.
    pub const NAMESPACE: u8 = 3;
    /// `SymbolKind.Class`.
    pub const CLASS: u8 = 5;
    /// `SymbolKind.Enum`.
    pub const ENUM: u8 = 10;
    /// `SymbolKind.Interface`.
    pub const INTERFACE: u8 = 11;
    /// `SymbolKind.Function`.
    pub const FUNCTION: u8 = 12;
    /// `SymbolKind.Variable`.
    pub const VARIABLE: u8 = 13;
    /// `SymbolKind.Constant`.
    pub const CONSTANT: u8 = 14;
    /// `SymbolKind.TypeParameter`.
    pub const TYPE_PARAMETER: u8 = 26;
}

/// Deklaracja rozpoznana w jednym wierszu.
struct Deklaracja {
    nazwa: String,
    rodzaj: u8,
    /// Kolumna nazwy liczona w **bajtach** od początku wiersza.
    kolumna_nazwy: usize,
}

/// Nazwa jest identyfikatorem, jeśli zaczyna się literą albo `_`.
///
/// Sprawdzamy `is_alphanumeric`, a nie `is_alphabetic`, bo w Ruście i
/// TypeScripcie nazwą może być też znak spoza ASCII — wtedy `jest` byłoby
/// dobrym wyjątkiem, a `😀` już nie.
fn jest_identyfikator(tekst: &str) -> bool {
    match tekst.chars().next() {
        Some(c) if c.is_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    tekst.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Odcina wiersz od komentarza (`//`) — żeby `// fn a` nie był deklaracją.
fn bez_komentarza(tekst: &str) -> &str {
    match tekst.find("//") {
        Some(i) => &tekst[..i],
        None => tekst,
    }
}

/// Rozdziela tekst na słowo kluczowe i resztę za nim.
fn podziel_slowo(tekst: &str) -> Option<(&str, &str)> {
    let slowo = tekst.split_whitespace().next()?;
    if slowo.is_empty() {
        return None;
    }
    Some((slowo, &tekst[slowo.len()..]))
}

/// Wycina nazwę z nagłówka: `Foo<T>` → `Foo`, `suma(a` → `suma`,
/// `MAX: u8 = 1` → `MAX`.
///
/// `!` z `macro_rules!` zostaje usunięte, a sama nazwa zachowuje czytelność.
fn nazwa_z_naglowka(tekst: &str) -> &str {
    let wyraz = tekst
        .split(['<', '(', ' ', '{', ';', ',', '=', ':'])
        .next()
        .unwrap_or("");
    wyraz.trim_end_matches('!')
}

/// Przesunięcie pierwszego znaku poza wcięciem (w bajtach).
fn od_nazwy(tekst: &str) -> usize {
    tekst.len() - tekst.trim_start().len()
}

/// Zwraca symbole dokumentu w kolejności występowania.
///
/// Język nieznany (albo pusty plik) daje pustą listę, a nie błąd — klient
/// pyta o symbole dla każdego otwartego pliku, także takiego, którego nie
/// umiemy analizować.
pub fn document_symbols(document: &Document) -> Vec<SymbolInformation> {
    let mut out = Vec::new();

    for linia in 0..document.line_count() {
        let tekst = document.line_text(linia).unwrap_or_default();
        let deklaracja = match document.language() {
            Language::Rust => w_rust(tekst),
            Language::TypeScript | Language::JavaScript => w_skrypcie(tekst),
            Language::Inne => None,
        };
        let Some(deklaracja) = deklaracja else {
            continue;
        };

        let zakres = document.line_range(linia).unwrap_or_else(|| {
            let poczatek = Position::new(linia as u32, 0);
            Range::new(poczatek, poczatek)
        });

        let start_nazwy = document.line_start_offset(linia) + deklaracja.kolumna_nazwy;
        let selection_range = Range::new(
            document.position_of(start_nazwy),
            document.position_of(start_nazwy + deklaracja.nazwa.len()),
        );

        out.push(SymbolInformation {
            name: deklaracja.nazwa,
            kind: deklaracja.rodzaj,
            range: zakres,
            selection_range,
        });
    }

    out
}

/// Deklaracje Rusta: `fn`, `struct`, `enum`, `trait`, `impl`, `mod`, `const`,
/// `static`, `type`, `macro_rules!`.
fn w_rust(linia: &str) -> Option<Deklaracja> {
    let kod = bez_komentarza(linia);
    let wciecie = kod.len() - kod.trim_start().len();
    let mut tresc = kod.trim_start();
    let mut kolumna = wciecie;

    // `pub` jest opcjonalne i nie zmienia rodzaju symbolu.
    if let Some(po) = tresc.strip_prefix("pub ") {
        kolumna += 4;
        tresc = po.trim_start();
    }
    // `unsafe fn`, `async fn` — modyfikatory przed słowem kluczowym.
    for prefiks in ["unsafe ", "async "] {
        if let Some(po) = tresc.strip_prefix(prefiks) {
            kolumna += prefiks.len();
            tresc = po.trim_start();
        }
    }

    // `impl Trait for Type` nazywamy po **typie**, nie po cechach — tak robi
    // sam `rust-analyzer`, bo w Outline chcemy widzieć typ, a nie listę cech.
    //
    // Rozpatrujemy `impl` przed `podziel_slowo`, bo `impl<T>` to dla
    // `split_whitespace` jedno słowo — nie trafilibyśmy na prefiks `impl`.
    if let Some(po_impl) = tresc.strip_prefix("impl") {
        let mut naglowek = po_impl.trim_start();
        let mut przesuniecie = kolumna + 4 + (po_impl.len() - naglowek.len());

        // `impl<T>` i `impl<'a, T>` — lista generyków przed cechami typu.
        if naglowek.starts_with('<') {
            if let Some(i) = naglowek.find('>') {
                przesuniecie += i + 1;
                naglowek = naglowek[i + 1..].trim_start();
            }
        }

        let (typ, przesuniecie_typu) = match naglowek.split_once(" for ") {
            Some((_, po)) => {
                let przesuw = naglowek.len() - po.len();
                (po, przesuniecie + przesuw)
            }
            None => (naglowek, przesuniecie),
        };

        let kropka = typ.find('<').unwrap_or(typ.len());
        let nazwa = nazwa_z_naglowka(&typ[..kropka]);
        if !jest_identyfikator(nazwa) {
            return None;
        }
        let start = przesuniecie_typu + od_nazwy(&typ[..kropka]);
        return Some(Deklaracja {
            nazwa: nazwa.to_owned(),
            rodzaj: kind::CLASS,
            kolumna_nazwy: start,
        });
    }

    let (slowo, reszta) = podziel_slowo(tresc)?;
    let po_slowie = tresc.len() - reszta.len();

    let rodzaj = match slowo {
        "fn" => kind::FUNCTION,
        "struct" | "union" => kind::CLASS,
        "enum" => kind::ENUM,
        "trait" => kind::INTERFACE,
        "mod" => kind::MODULE,
        "const" | "static" => kind::CONSTANT,
        "type" => kind::TYPE_PARAMETER,
        "macro_rules!" => kind::FUNCTION,
        _ => return None,
    };

    let naglowek = reszta.trim_start();
    let przesuniecie = reszta.len() - naglowek.len();
    let nazwa = nazwa_z_naglowka(naglowek);
    if !jest_identyfikator(nazwa) {
        return None;
    }

    Some(Deklaracja {
        nazwa: nazwa.to_owned(),
        rodzaj,
        kolumna_nazwy: kolumna + po_slowie + przesuniecie + od_nazwy(naglowek),
    })
}

/// Deklaracje TypeScriptu i JavaScriptu: `function`, `class`, `interface`,
/// `enum`, `namespace`, `type` oraz `const` / `let` / `var`.
///
/// Świadomie rozpoznajemy **tylko deklaracje z najwyższego poziomu** —
/// metody klasy (`foo() {}`) i funkcje zagnieżdżone zostawiamy poza
/// zakresem. Bez pełnego parsera nie odróżniamy ich od zwykłych wywołań,
/// a fałszywy symbol w panelu „Outline” jest gorszy niż jego brak.
///
/// Modyfikatory (`export`, `static`, `async`, …) są pomijane, bo nie
/// zmieniają rodzaju symbolu.
fn w_skrypcie(linia: &str) -> Option<Deklaracja> {
    let kod = bez_komentarza(linia);
    let wciecie = kod.len() - kod.trim_start().len();
    let mut tresc = kod.trim_start();
    let mut kolumna = wciecie;

    // Modyfikatory, które nie zmieniają rodzaju symbolu.
    for prefiks in [
        "export default ",
        "export ",
        "declare ",
        "abstract ",
        "public ",
        "private ",
        "protected ",
        "async ",
        "static ",
        "readonly ",
        "get ",
        "set ",
        "default ",
    ] {
        if let Some(po) = tresc.strip_prefix(prefiks) {
            kolumna += prefiks.len();
            tresc = po;
        }
    }

    let (slowo, reszta) = podziel_slowo(tresc.trim_start())?;
    let po_slowie = tresc.trim_start().len() - reszta.len();

    let rodzaj = match slowo {
        "function" => kind::FUNCTION,
        "class" => kind::CLASS,
        "interface" => kind::INTERFACE,
        "enum" => kind::ENUM,
        "namespace" | "module" => kind::NAMESPACE,
        "type" => kind::TYPE_PARAMETER,
        "const" | "let" | "var" => {
            // `const f = () => {}` to funkcja, `const x = 1` to zmienna.
            if reszta.contains("=>") || reszta.contains("function") {
                kind::FUNCTION
            } else {
                kind::VARIABLE
            }
        }
        _ => return None,
    };

    let naglowek = reszta.trim_start();
    let przesuniecie = reszta.len() - naglowek.len();
    // Deklaracja bez nazwy (`function` w nowej linii) to nie symbol.
    let nazwa = nazwa_z_naglowka(naglowek);
    if !jest_identyfikator(nazwa) {
        return None;
    }

    Some(Deklaracja {
        nazwa: nazwa.to_owned(),
        rodzaj,
        kolumna_nazwy: kolumna + po_slowie + przesuniecie + od_nazwy(naglowek),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbole(tekst: &str, jezyk: Language) -> Vec<SymbolInformation> {
        let mut d = Document::new("file:///x", jezyk, 1);
        d.set_text(tekst.to_owned());
        document_symbols(&d)
    }

    fn pary(tekst: &str, jezyk: Language) -> Vec<(String, u8)> {
        symbole(tekst, jezyk)
            .iter()
            .map(|s| (s.name.clone(), s.kind))
            .collect()
    }

    fn nazwy(tekst: &str, jezyk: Language) -> Vec<String> {
        symbole(tekst, jezyk)
            .iter()
            .map(|s| s.name.clone())
            .collect()
    }

    #[test]
    fn rust_znajmuje_deklaracje() {
        let s = pary(
            "pub fn main() {}\nstruct Punkt;\nenum Kolor {}\ntrait Rysuj {}\nmod utils;\nconst MAX: u8 = 1;\n",
            Language::Rust,
        );
        assert_eq!(
            s,
            vec![
                ("main".to_owned(), kind::FUNCTION),
                ("Punkt".to_owned(), kind::CLASS),
                ("Kolor".to_owned(), kind::ENUM),
                ("Rysuj".to_owned(), kind::INTERFACE),
                ("utils".to_owned(), kind::MODULE),
                ("MAX".to_owned(), kind::CONSTANT),
            ]
        );
    }

    #[test]
    fn impl_nazywamy_po_typie() {
        assert_eq!(
            nazwy("impl Rysuj for Punkt {}\n", Language::Rust),
            ["Punkt"]
        );
        assert_eq!(nazwy("impl Punkt {}\n", Language::Rust), ["Punkt"]);
        assert_eq!(
            nazwy("impl<T> Rysuj<T> for Punkt<T> {}\n", Language::Rust),
            ["Punkt"]
        );
    }

    #[test]
    fn generyki_i_modyfikatory_sie_kroja() {
        assert_eq!(nazwy("struct Wektor<T> {}\n", Language::Rust), ["Wektor"]);
        assert_eq!(nazwy("fn suma<T>(a: T) {}\n", Language::Rust), ["suma"]);
        assert_eq!(nazwy("pub async fn idz() {}\n", Language::Rust), ["idz"]);
        assert_eq!(
            nazwy("unsafe fn ryzykowna() {}\n", Language::Rust),
            ["ryzykowna"]
        );
    }

    #[test]
    fn komentarze_nie_sa_deklaracjami() {
        assert!(nazwy("// fn nieistniejaca() {}\n", Language::Rust).is_empty());
        assert!(nazwy("  // struct TezNie;\n", Language::Rust).is_empty());
        assert!(nazwy("// function f() {}\n", Language::JavaScript).is_empty());
    }

    #[test]
    fn zwykly_kod_rust_nie_jest_symbolami() {
        let s = nazwy(
            "let x = 1;\nprintln!();\nuse std::fmt;\nreturn;\n",
            Language::Rust,
        );
        assert!(s.is_empty(), "{s:?}");
    }

    #[test]
    fn skrypt_rozpoznaje_funkcje_i_klasy() {
        let s = pary(
            "export function f() {}\nexport class K {}\nconst g = () => {};\nlet x = 1;\n",
            Language::JavaScript,
        );
        assert_eq!(
            s,
            vec![
                ("f".to_owned(), kind::FUNCTION),
                ("K".to_owned(), kind::CLASS),
                ("g".to_owned(), kind::FUNCTION),
                ("x".to_owned(), kind::VARIABLE),
            ]
        );
    }

    #[test]
    fn typescript_dodaje_interface_i_type() {
        assert_eq!(
            nazwy(
                "interface U {}\ntype Alias = string;\n",
                Language::TypeScript
            ),
            ["U", "Alias"]
        );
    }

    #[test]
    fn metody_klasy_nie_sa_symbolami() {
        // Bez parsera nie odróżniamy `foo() {}` od wywołania — lepiej pominąć.
        let s = "class K {\n  static metoda() {}\n  inna() {}\n}\n";
        assert_eq!(nazwy(s, Language::JavaScript), ["K"]);
    }

    #[test]
    fn zasiag_nazwy_obejmuje_tylko_nia() {
        let s = symbole("fn main() {}\n", Language::Rust);
        assert_eq!(s[0].selection_range.start, Position::new(0, 3));
        assert_eq!(s[0].selection_range.end, Position::new(0, 7));
        assert_eq!(s[0].range.end, Position::new(0, 12), "cała linia");
    }

    #[test]
    fn pusty_i_nieznany_plik_daja_pusta_liste() {
        assert!(symbole("", Language::Rust).is_empty());
        assert!(symbole("fn main() {}", Language::Inne).is_empty());
    }

    #[test]
    fn pozycje_licza_utf16() {
        // Cztery znaki BMP to cztery jednostki UTF-16, więc kolumny wychodzą
        // tak samo jak dla ASCII — ale `pub` i wcięcie muszą się zsumować.
        let s = symbole("pub fn żółw() {}\n", Language::Rust);
        assert_eq!(s[0].name, "żółw");
        assert_eq!(s[0].selection_range.start.character, 7);
        assert_eq!(s[0].selection_range.end.character, 11);

        let s = symbole("    fn suma() {}\n", Language::Rust);
        assert_eq!(s[0].selection_range.start.character, 7, "wcięcie się liczy");
    }

    #[test]
    fn emoji_nie_jest_identyfikatorem() {
        // `fn 😀()` nie skompilowałby się w Ruście, więc nie zgłaszamy symbolu.
        assert!(symbole("fn 😀() {}\n", Language::Rust).is_empty());
    }

    #[test]
    fn symbole_serializuja_sie_jak_lsp() {
        let v = serde_json::to_value(&symbole("fn main() {}\n", Language::Rust)[0]).unwrap();
        assert_eq!(v["name"], "main");
        assert_eq!(v["kind"], 12);
        assert_eq!(v["selectionRange"]["start"]["character"], 3);
        assert_eq!(v["range"]["end"]["character"], 12);
    }
}
