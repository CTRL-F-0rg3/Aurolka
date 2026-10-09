//! Test integracyjny: klient ↔ serwer przez prawdziwy transport.
//!
//! Żadnego VS Code — po jednej stronie jest klient symulowany w pamięci, po
//! drugiej [`Server`]. Ramki `Content-Length` są te same, które widziałby
//! edytor, więc test sprawdza całą ścieżkę: transport → serwer → diagnostyka →
//! transport → klient.
//!
//! ```text
//! cargo test -p vscode-bridge --test protocol
//! ```

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use vscode_bridge::host::Host;
use vscode_bridge::protocol::{Message, Transport};
use vscode_bridge::server::Server;

/// Bufor współdzielony: to, co serwer wypisuje, widzi klient.
#[derive(Clone, Default)]
struct SpyWriter(Arc<Mutex<Vec<u8>>>);

impl SpyWriter {
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl std::io::Write for SpyWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Przepuszcza wiadomość przez serwer i zwraca to, co serwer wypisał.
///
/// Nowy serwer przy każdym wywołaniu — wygodne dla testów jednostkowych
/// pojedynczych metod. Gdy potrzebny jest stan (np. `didOpen`, a potem
/// `documentSymbol`), użyj [`Sesja`].
fn przez_serwer(message: &Message) -> Vec<u8> {
    let mut sesja = Sesja::new();
    sesja.wyslij(message)
}

/// Sesja: jeden serwer, jeden bufor, wiele wiadomości po kolei.
struct Sesja {
    serwer: Server,
    nadawca: SpyWriter,
}

impl Sesja {
    fn new() -> Self {
        Self {
            serwer: Server::new(),
            nadawca: SpyWriter::default(),
        }
    }

    /// Podaje serwerowi wiadomość i zwraca **nowe** wiadomości klienta.
    fn wyslij(&mut self, message: &Message) -> Vec<u8> {
        // Pusty czytnik: `handle` nie czyta z transportu, tylko pisze.
        let mut transport = Transport::new(Cursor::new(Vec::new()), self.nadawca.clone());
        let start = self.nadawca.bytes().len();

        {
            let mut host = Host::new(&mut transport, 1);
            let odpowiedz = self
                .serwer
                .handle(message, &mut host)
                .expect("serwer nie wywalił się");
            if let Some(odpowiedz) = odpowiedz {
                transport.send(&odpowiedz).expect("wysyłka odpowiedzi");
            }
        }

        self.nadawca.bytes()[start..].to_vec()
    }
}

/// Wyciąga wszystkie wiadomości z bajtów wypisanych przez serwer.
fn odczytaj(bajty: &[u8]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut pozycja = 0;

    while let Some(koniec_naglowka) = bajty[pozycja..].windows(4).position(|o| o == b"\r\n\r\n") {
        let koniec_naglowka = pozycja + koniec_naglowka;
        let naglowek = String::from_utf8_lossy(&bajty[pozycja..koniec_naglowka]).to_string();
        let dlugosc: usize = naglowek
            .rsplit("Content-Length:")
            .next()
            .expect("brak Content-Length")
            .trim()
            .parse()
            .expect("zła długość");

        let start = koniec_naglowka + 4;
        out.push(serde_json::from_slice(&bajty[start..start + dlugosc]).expect("zły JSON w ramce"));
        pozycja = start + dlugosc;
    }

    out
}

/// `didOpen` dla pliku o podanym języku.
fn did_open(uri: &str, jezyk: &str, tekst: &str) -> Message {
    Message::notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": jezyk,
                "version": 1,
                "text": tekst,
            }
        }),
    )
}

/// Pierwsza diagnostyka z publikacji, albo `Null` gdy jej nie było.
fn pierwsza_diagnostyka(wiadomosci: &[Value]) -> Value {
    wiadomosci
        .iter()
        .find(|w| w["method"] == "textDocument/publishDiagnostics")
        .and_then(|w| w["params"]["diagnostics"].as_array())
        .and_then(|d| d.first().cloned())
        .unwrap_or(Value::Null)
}

/// Wszystkie diagnostyki z ostatniej publikacji.
fn wszystkie_diagnostyki(wiadomosci: &[Value]) -> Vec<Value> {
    wiadomosci
        .iter()
        .rfind(|w| w["method"] == "textDocument/publishDiagnostics")
        .and_then(|w| w["params"]["diagnostics"].as_array().cloned())
        .unwrap_or_default()
}
#[test]
fn initialize_zwraca_mozliwosci_serwera() {
    let wiadomosc = Message::request(
        1,
        "initialize",
        json!({
            "processId": 4242,
            "rootUri": "file:///projekt",
            "workspaceFolders": [{ "uri": "file:///projekt", "name": "projekt" }],
        }),
    );

    let odpowiedzi = odczytaj(&przez_serwer(&wiadomosc));
    let odpowiedz = odpowiedzi.first().expect("serwer musi odpowiedzieć");

    assert_eq!(odpowiedz["id"], 1);
    assert_eq!(
        odpowiedz["jsonrpc"], "2.0",
        "pole wymagane przez JSON-RPC 2.0"
    );
    assert_eq!(
        odpowiedz["result"]["capabilities"]["textDocumentSync"], 1,
        "serwer deklaruje pełną synchronizację tekstu"
    );
    assert_eq!(
        odpowiedz["result"]["capabilities"]["documentSymbolProvider"],
        true
    );
    assert_eq!(odpowiedz["result"]["serverInfo"]["name"], "vscode-bridge");
}

#[test]
fn initialize_echa_id_zadania() {
    // Identyfikator bywa dowolny — serwer nie może go zakładać.
    let wiadomosc = Message::request(4711, "initialize", json!({}));

    let odpowiedzi = odczytaj(&przez_serwer(&wiadomosc));
    assert_eq!(odpowiedzi[0]["id"], 4711);
}

#[test]
fn did_open_rust_wysyla_diagnostyke() {
    let wiadomosc = did_open("file:///a.rs", "rust", "fn main() {\n    let x = 1;\n");

    let wiadomosci = odczytaj(&przez_serwer(&wiadomosc));
    let diagnostyka = pierwsza_diagnostyka(&wiadomosci);

    // `severity` to liczba 1–4, nie słowo — takiego formatu wymaga VS Code.
    assert_eq!(diagnostyka["severity"], 1);
    assert_eq!(diagnostyka["source"], "vscode-bridge");
    assert!(
        diagnostyka["message"]
            .as_str()
            .unwrap()
            .contains("niezamknięty"),
        "komunikat: {}",
        diagnostyka["message"]
    );
}

#[test]
fn typescript_i_javascript_dziela_reguly() {
    for (jezyk, tekst, oczekiwany) in [
        (
            "typescript",
            "var x = 1;\n",
            "`var` ma zasięg funkcji — użyj `let` albo `const`",
        ),
        (
            "javascript",
            "if (a == null) {}\n",
            "`== null` porównuje też `undefined` — użyj `===`",
        ),
    ] {
        let wiadomosci = odczytaj(&przez_serwer(&did_open("file:///a.ts", jezyk, tekst)));
        let diagnostyka = pierwsza_diagnostyka(&wiadomosci);

        assert_eq!(diagnostyka["message"], oczekiwany, "język {jezyk}");
        assert_eq!(diagnostyka["severity"], 2, "ostrzeżenie");
    }
}

#[test]
fn czysty_plik_nie_daje_diagnostyk() {
    let wiadomosci = odczytaj(&przez_serwer(&did_open(
        "file:///a.rs",
        "rust",
        "fn main() {\n    println!(\"cześć\");\n}\n",
    )));

    assert!(
        wszystkie_diagnostyki(&wiadomosci).is_empty(),
        "poprawny plik nie powinien mieć podkreśleń"
    );
}

#[test]
fn did_change_aktualizuje_dokument() {
    let mut sesja = Sesja::new();

    // Najpierw otwieramy plik z błędem…
    sesja.wyslij(&did_open("file:///a.rs", "rust", "fn main() {\n"));

    // …a potem go poprawiamy przez `didChange`.
    let zmiana = Message::notification(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": "file:///a.rs", "version": 2 },
            "contentChanges": [{ "text": "fn main() {}\n" }],
        }),
    );

    let wiadomosci = odczytaj(&sesja.wyslij(&zmiana));
    assert!(
        wszystkie_diagnostyki(&wiadomosci).is_empty(),
        "po poprawieniu pliku nie ma już błędów"
    );
}

#[test]
fn did_close_czysci_diagnostyke() {
    let mut sesja = Sesja::new();
    sesja.wyslij(&did_open("file:///a.rs", "rust", "fn main() {\n"));

    let zamkniecie = Message::notification(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": "file:///a.rs" } }),
    );

    let wiadomosci = odczytaj(&sesja.wyslij(&zamkniecie));

    // Pusta lista = „zetrzyj podkreślenia”, nie „nie ma błędów”.
    assert!(
        wszystkie_diagnostyki(&wiadomosci).is_empty(),
        "zamknięty plik nie ma diagnostyki"
    );
    assert_eq!(
        sesja.serwer.workspace().len(),
        0,
        "dokument wypadł z magazynu"
    );
}

#[test]
fn nieznana_metoda_dostaje_kod_bledu() {
    let wiadomosc = Message::request(9, "textDocument/hover", json!({}));
    let odpowiedz = odczytaj(&przez_serwer(&wiadomosc)).remove(0);

    assert_eq!(odpowiedz["id"], 9);
    assert_eq!(
        odpowiedz["error"]["code"], -32601,
        "JSON-RPC: Method not found"
    );
}

#[test]
fn powiadomienie_nie_dostaje_odpowiedzi() {
    let wiadomosci = odczytaj(&przez_serwer(&did_open(
        "file:///a.rs",
        "rust",
        "fn main() {}\n",
    )));

    // Jedyna wiadomość to publikacja diagnostyki — bez żadnej odpowiedzi.
    assert!(
        wiadomosci.iter().all(|w| w.get("id").is_none()),
        "{wiadomosci:?}"
    );
}

#[test]
fn shutdown_i_exit_daja_kod_wyjścia_zero() {
    let mut server = Server::new();
    let nadawca = SpyWriter::default();
    let pusty = Cursor::new(Vec::new());
    let mut transport = Transport::new(pusty, nadawca);

    {
        let mut host = Host::new(&mut transport, 1);
        server
            .handle(&Message::request(10, "shutdown", Value::Null), &mut host)
            .unwrap();
        assert!(
            server.exit_code().is_none(),
            "sam shutdown nie kończy procesu"
        );

        server
            .handle(&Message::notification("exit", Value::Null), &mut host)
            .unwrap();
    }

    assert_eq!(server.exit_code(), Some(0), "exit po shutdown to czyste 0");
}

#[test]
fn exit_bez_shutdown_daje_kod_jedynki() {
    let mut server = Server::new();
    let nadawca = SpyWriter::default();
    let mut transport = Transport::new(Cursor::new(Vec::new()), nadawca);

    {
        let mut host = Host::new(&mut transport, 1);
        server
            .handle(&Message::notification("exit", Value::Null), &mut host)
            .unwrap();
    }

    assert_eq!(server.exit_code(), Some(1));
}

#[test]
fn document_symbol_zwraca_symbole() {
    let mut sesja = Sesja::new();
    sesja.wyslij(&did_open(
        "file:///a.rs",
        "rust",
        "fn main() {}\nstruct Punkt;\n",
    ));

    let zapytanie = Message::request(
        5,
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": "file:///a.rs" } }),
    );

    let odpowiedzi = odczytaj(&sesja.wyslij(&zapytanie));
    let symbole = odpowiedzi[0]["result"].as_array().expect("lista symboli");

    assert_eq!(odpowiedzi[0]["id"], 5);
    assert_eq!(symbole.len(), 2);
    assert_eq!(symbole[0]["name"], "main");
    assert_eq!(symbole[0]["kind"], 12, "Function");
    assert_eq!(symbole[1]["name"], "Punkt");
    assert_eq!(symbole[1]["kind"], 5, "Class");
}

#[test]
fn document_symbol_nieznanego_pliku_to_pusta_lista() {
    // VS Code pyta o symbole pliku, którego jeszcze nie otworzyliśmy.
    let zapytanie = Message::request(
        6,
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": "file:///nieistniejacy.rs" } }),
    );

    let odpowiedzi = odczytaj(&przez_serwer(&zapytanie));
    assert_eq!(odpowiedzi[0]["result"], json!([]));
}

#[test]
fn execute_command_odpowiada_bez_bledu() {
    let mut sesja = Sesja::new();
    sesja.wyslij(&did_open("file:///a.rs", "rust", "fn main() { }\n"));

    for (id, komenda) in [(7, "vscodeBridge.status"), (8, "vscodeBridge.reanalyzeAll")] {
        let zadanie = Message::request(
            id,
            "workspace/executeCommand",
            json!({ "command": komenda, "arguments": [] }),
        );

        let odpowiedzi = odczytaj(&sesja.wyslij(&zadanie));
        let ostatnia = odpowiedzi.last().expect("serwer musi odpowiedzieć");
        assert_eq!(ostatnia["id"], id);
        assert!(ostatnia.get("error").is_none(), "{komenda}: {ostatnia}");
        assert_eq!(ostatnia["result"], json!(null));
    }
}

#[test]
fn nieznana_komenda_dostaje_kod_bledu() {
    let zadanie = Message::request(
        9,
        "workspace/executeCommand",
        json!({ "command": "vscodeBridge.nieMaTakiej" }),
    );

    let odpowiedzi = odczytaj(&przez_serwer(&zadanie));
    assert_eq!(odpowiedzi[0]["error"]["code"], -32601);
}

#[test]
fn pozycje_licza_jednostki_utf16() {
    // `fn 😀() {`: `f`, `n`, spacja to 3 jednostki, emoji to 2, potem `()`, spacja.
    // Klamra stoi więc na 8. jednostce UTF-16, choć jest 7. znakiem.
    let wiadomosci = odczytaj(&przez_serwer(&did_open(
        "file:///a.rs",
        "rust",
        "fn 😀() {",
    )));
    let diagnostyka = pierwsza_diagnostyka(&wiadomosci);

    assert_eq!(
        diagnostyka["range"]["start"]["character"], 8,
        "kolumna liczy jednostki UTF-16, nie znaki"
    );

    // Dla znaków BMP (`ó` = 1 jednostka) obie liczby są równe.
    let wiadomosci = odczytaj(&przez_serwer(&did_open("file:///b.rs", "rust", "fn ó() {")));
    let diagnostyka = pierwsza_diagnostyka(&wiadomosci);
    assert_eq!(diagnostyka["range"]["start"]["character"], 7);
}

#[test]
fn panel_uzywa_matematyki_aurum() {
    use aurum::math::Vec2;
    use vscode_bridge::panel::PanelLayout;

    // 1280 − 320 (panel) − 2×16 (marginesy) = 928 pikseli na edytor.
    let layout = PanelLayout::new(Vec2::new(1280.0, 720.0), 320.0, 16.0);
    assert_eq!(layout.editor().size.x, 928.0);
    assert_eq!(layout.editor().size.y, 688.0);
    assert_eq!(layout.panel().origin.x, 960.0);
}
