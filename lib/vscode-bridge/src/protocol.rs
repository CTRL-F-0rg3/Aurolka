//! Transport JSON-RPC 2.0 używany przez LSP i rozszerzenia VS Code.
//!
//! VS Code gada z serwerami ramkowanymi: przed każdą wiadomością JSON leci
//! nagłówek `Content-Length: N`, potem dokładnie `N` bajtów payloadu.
//! Ten moduł czyta i zapisuje takie ramki na dowolnym `Read`/`Write`, więc
//! da się go testować bez procesu VS Code.
//!
//! ```
//! use vscode_bridge::protocol::{Message, Transport};
//! use std::sync::{Arc, Mutex};
//!
//! // Wspólny bufor: transport pisze, a test czyta.
//! let zapis = Arc::new(Mutex::new(Vec::new()));
//! let cel = Arc::clone(&zapis);
//!
//! let mut transport = Transport::new(std::io::Cursor::new(Vec::new()), Buf(cel));
//! transport
//!     .send(&Message::request(1, "initialize", serde_json::json!({})))
//!     .unwrap();
//!
//! let bajty = zapis.lock().unwrap().clone();
//! let wypis = String::from_utf8_lossy(&bajty);
//! assert!(wypis.starts_with("Content-Length: "));
//! assert!(wypis.contains("\"method\":\"initialize\""));
//!
//! // Bufor zgodny z `std::io::Write`.
//! struct Buf(Arc<Mutex<Vec<u8>>>);
//! impl std::io::Write for Buf {
//!     fn write(&mut self, d: &[u8]) -> std::io::Result<usize> {
//!         self.0.lock().unwrap().extend_from_slice(d);
//!         Ok(d.len())
//!     }
//!     fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
//! }
//! ```

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
/// Maksymalny rozmiar jednej ramki (16 MiB) — wartość zgodna z `vscode-jsonrpc`,
/// która odrzuca ramki większe niż 100 MB, ale nasza jest mniejsza i bezpieczniejsza.
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Wiadomość JSON-RPC: żądanie, odpowiedź albo powiadomienie.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    /// Powiadomienie (bez `id`) albo żądanie (z `id`).
    ///
    /// Ważna kolejność wariantów: `Request` ma **wymagane** pole `method`, więc
    /// odpowiedź (która go nie ma) nie zostanie w niego dopasowana.
    Request(Request),
    /// Odpowiedź na żądanie (albo błąd protokołu).
    Response(Response),
}

/// Wersja protokołu — wymagana w każdej wiadomości przez JSON-RPC 2.0 i przez
/// ścisłą walidację `vscode-jsonrpc`, którą stosuje VS Code.
const JSONRPC_VERSION: &str = "2.0";

/// Wersja protokołu dla odczytu — brak pola tolerujemy (bywa w starszych
/// klientach), ale nigdy go nie wysyłamy.
fn wersja_jsonrpc() -> String {
    JSONRPC_VERSION.to_owned()
}

/// Żądanie lub powiadomienie.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Zawsze `"2.0"` — pole wymagane przez specyfikację.
    #[serde(default = "wersja_jsonrpc")]
    pub jsonrpc: String,
    /// Metoda, np. `textDocument/didOpen`.
    pub method: String,
    /// Parametry — dowolny JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Identyfikator żądania; `None` oznacza powiadomienie.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

/// Odpowiedź na żądanie.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Zawsze `"2.0"` — pole wymagane przez specyfikację.
    #[serde(default = "wersja_jsonrpc")]
    pub jsonrpc: String,
    /// Identyfikator, do którego odpowiedź należy — **wymagany**, żeby
    /// wiadomość bez `method` nie została wzięta za odpowiedź.
    pub id: Value,
    /// Wynik (obowiązkowy, gdy `error` jest `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Błąd protokołu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

/// Błąd zwracany w odpowiedzi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    /// Kod zgodny z JSON-RPC (-32768…-32000).
    pub code: i64,
    /// Czytelny komunikat.
    pub message: String,
    /// Dodatkowe dane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Message {
    /// Buduje żądanie z identyfikatorem.
    pub fn request(id: i64, method: &str, params: Value) -> Self {
        Self::Request(Request {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            method: method.to_owned(),
            params: Some(params),
            id: Some(Value::from(id)),
        })
    }

    /// Buduje powiadomienie (bez odpowiedzi).
    pub fn notification(method: &str, params: Value) -> Self {
        Self::Request(Request {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            method: method.to_owned(),
            params: Some(params),
            id: None,
        })
    }

    /// Buduje odpowiedź powodzenia.
    pub fn result(id: Value, result: Value) -> Self {
        Self::Response(Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: Some(result),
            error: None,
        })
    }

    /// Buduje odpowiedź z błędem.
    pub fn failure(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self::Response(Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(ResponseError {
                code,
                message: message.into(),
                data: None,
            }),
        })
    }

    /// Nazwa metody (powiadomienia/żądania) albo `None` dla odpowiedzi.
    pub fn method(&self) -> Option<&str> {
        match self {
            Self::Request(request) => Some(&request.method),
            Self::Response(_) => None,
        }
    }

    /// Parametry żądania/powiadomienia albo wynik odpowiedzi.
    pub fn params(&self) -> Option<&Value> {
        match self {
            Self::Request(request) => request.params.as_ref(),
            Self::Response(response) => response.result.as_ref(),
        }
    }

    /// Identyfikator żądania albo odpowiedzi.
    pub fn id(&self) -> Option<&Value> {
        match self {
            Self::Request(request) => request.id.as_ref(),
            Self::Response(response) => Some(&response.id),
        }
    }

    /// Czy to powiadomienie (żądanie bez `id`).
    pub fn is_notification(&self) -> bool {
        matches!(self, Self::Request(request) if request.id.is_none())
    }
}

/// Ramkowanie wiadomości: nagłówek `Content-Length` + payload.
pub struct Transport<T: BufRead> {
    reader: T,
    writer: Box<dyn Write + Send>,
}

impl<T: BufRead> Transport<T> {
    /// Tworzy transport czytający z `reader` i piszący do `writer`.
    ///
    /// Rozdzielenie obu strumieni ma sens dla testów: można czytać z bufora
    /// i jednocześnie zbierać to, co transport wypisuje. Zapis idzie wprost do
    /// `writer` (bez bufora), żeby nic nie przepadło przy zamknięciu transportu.
    pub fn new(reader: T, writer: impl Write + Send + 'static) -> Self {
        Self {
            reader,
            writer: Box::new(writer),
        }
    }

    /// Czyta kolejną ramkę; `Ok(None)` oznacza koniec strumienia.
    pub fn read(&mut self) -> Result<Option<Message>> {
        let mut content_length: Option<usize> = None;

        loop {
            let mut line = String::new();
            let read = read_line(&mut self.reader, &mut line)?;
            if read == 0 {
                // Koniec wejścia — jeśli nie było nic w ramce, to nie błąd.
                return match content_length {
                    None => Ok(None),
                    Some(_) => Err(Error::Protocol("strumień urwany w trakcie nagłówka".into())),
                };
            }

            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break; // pusta linia kończy nagłówek
            }

            let Some((name, value)) = line.split_once(':') else {
                return Err(Error::Protocol(format!("zły nagłówek: `{line}`")));
            };
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().parse().map_err(|_| {
                    Error::Protocol(format!("zły `Content-Length`: `{}`", value.trim()))
                })?);
            }
            // `Content-Type` i inne nagłówki ignorujemy świadomie.
        }

        let length = content_length
            .ok_or_else(|| Error::Protocol("brak nagłówka `Content-Length`".into()))?;
        if length > MAX_FRAME_BYTES {
            return Err(Error::Protocol(format!(
                "ramka {length} B przekracza limit {MAX_FRAME_BYTES} B"
            )));
        }

        let mut buffer = vec![0u8; length];
        self.reader.read_exact(&mut buffer)?;

        let message = serde_json::from_slice(&buffer)
            .map_err(|e| Error::Protocol(format!("zły JSON w ramce: {e}")))?;
        Ok(Some(message))
    }

    /// Zwraca wypisany strumień — przydatne w testach i przy przekierowaniu
    /// wyjścia do pliku.
    pub fn into_writer(self) -> Box<dyn Write + Send> {
        self.writer
    }

    /// Wysyła wiadomość w ramce.
    pub fn send(&mut self, message: &Message) -> Result<()> {
        let payload = serde_json::to_vec(message)
            .map_err(|e| Error::Protocol(format!("nie da się serializować: {e}")))?;
        write!(self.writer, "Content-Length: {}\r\n\r\n", payload.len())?;
        self.writer.write_all(&payload)?;
        self.writer.flush()?;
        Ok(())
    }
}

/// Czyta jedną linię, obsługując brak finalnego `\n`.
fn read_line<R: BufRead>(reader: &mut R, out: &mut String) -> Result<usize> {
    let mut bytes = Vec::new();
    let read = reader.read_until(b'\n', &mut bytes)?;
    out.push_str(&String::from_utf8_lossy(&bytes));
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    /// Writer, z którego da się odczytać to, co transport wypisał.
    ///
    /// Dzięki wspólnemu `Arc` testy widzą ramki dokładnie tak, jak zobaczyłby
    /// je VS Code — razem z nagłówkiem i długością.
    #[derive(Clone, Default)]
    struct SpyWriter(Arc<Mutex<Vec<u8>>>);

    impl SpyWriter {
        fn bytes(&self) -> Vec<u8> {
            self.0.lock().unwrap().clone()
        }
    }

    impl Write for SpyWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Buduje bajty ramki dokładnie tak, jak robi to transport.
    fn frame(message: &Message) -> Vec<u8> {
        let payload = serde_json::to_vec(message).unwrap();
        let mut out = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
        out.extend_from_slice(&payload);
        out
    }

    /// Rozdziela bajty na (nagłówek, payload).
    fn split_frame(bytes: &[u8]) -> (String, String) {
        let text = String::from_utf8_lossy(bytes);
        let koniec = text.find("\r\n\r\n").expect("brak końca nagłówka");
        (text[..koniec].to_owned(), text[koniec + 4..].to_owned())
    }

    #[test]
    fn send_zapisuje_naglowek_i_payload() {
        let spy = SpyWriter::default();
        let mut t = Transport::new(Cursor::new(Vec::new()), spy.clone());

        t.send(&Message::notification("exit", json!({}))).unwrap();

        let (naglowek, payload) = split_frame(&spy.bytes());
        let dlugosc: usize = naglowek
            .strip_prefix("Content-Length: ")
            .expect("nagłówek bez Content-Length")
            .trim()
            .parse()
            .expect("długość ramki nie jest liczbą");

        assert_eq!(
            payload.len(),
            dlugosc,
            "długość w nagłówku musi zgadzać się z bajtami"
        );
        assert!(payload.contains("\"method\":\"exit\""), "{payload}");
    }

    #[test]
    fn kazda_wiadomosc_nosi_pole_jsonrpc() {
        // VS Code odrzuca ramkę bez `jsonrpc` — sprawdzamy więc własne wyjście,
        // a nie tylko wejście (które odczytujemy z obcego klienta).
        for wiadomosc in [
            Message::notification("exit", json!({})),
            Message::request(1, "initialize", json!({})),
            Message::result(json!(1), json!({"ok": true})),
            Message::failure(json!(1), -32601, "Method not found"),
        ] {
            let bajty = frame(&wiadomosc);
            let tekst = String::from_utf8_lossy(&bajty);
            assert!(
                tekst.contains("\"jsonrpc\":\"2.0\""),
                "brak `jsonrpc` w ramce: {tekst}"
            );
        }
    }

    #[test]
    fn round_trip_zadania_i_odpowiedzi() {
        let nadawca = SpyWriter::default();
        let mut t = Transport::new(Cursor::new(Vec::new()), nadawca.clone());
        t.send(&Message::request(7, "shutdown", json!(null)))
            .unwrap();
        t.send(&Message::result(json!(7), json!({"ok": true})))
            .unwrap();

        let mut odbiorca = Transport::new(Cursor::new(nadawca.bytes()), Vec::new());

        let pierwsza = odbiorca.read().unwrap().unwrap();
        assert_eq!(pierwsza.method(), Some("shutdown"));
        assert_eq!(pierwsza.id(), Some(&json!(7)));

        let druga = odbiorca.read().unwrap().unwrap();
        assert_eq!(druga.id(), Some(&json!(7)));
        assert_eq!(druga.params(), Some(&json!({"ok": true})));

        assert!(odbiorca.read().unwrap().is_none(), "koniec strumienia");
    }

    #[test]
    fn powiadomienie_nie_ma_id() {
        let wiadomosc = Message::notification("textDocument/didSave", json!({"uri": "x.rs"}));
        assert!(wiadomosc.is_notification());
        assert_eq!(wiadomosc.id(), None);
        assert_eq!(wiadomosc.method(), Some("textDocument/didSave"));
    }

    #[test]
    fn odpowiedz_z_bledem_nosi_kod() {
        let w = Message::failure(json!(3), -32601, "Method not found");
        let json = serde_json::to_value(&w).unwrap();
        assert_eq!(json["error"]["code"], -32601);
        assert_eq!(json["error"]["message"], "Method not found");
        assert!(json["result"].is_null(), "błędna odpowiedź nie ma `result`");
    }

    #[test]
    fn zly_naglowek_to_blad() {
        let mut t = Transport::new(
            Cursor::new(b"Content-Length: abc\r\n\r\n".to_vec()),
            Vec::new(),
        );
        assert!(matches!(t.read(), Err(Error::Protocol(_))));
    }

    #[test]
    fn urwana_ramka_to_blad() {
        let mut t = Transport::new(
            Cursor::new(b"Content-Length: 100\r\n\r\n{\"jsonrpc\"".to_vec()),
            Vec::new(),
        );
        assert!(t.read().is_err());
    }

    #[test]
    fn pusty_strumien_to_nie_blad() {
        let mut t = Transport::new(Cursor::new(Vec::new()), Vec::new());
        assert!(t.read().unwrap().is_none());
    }

    #[test]
    fn za_dluga_ramka_jest_odrzucana() {
        let naglowek = format!("Content-Length: {}\r\n\r\n", MAX_FRAME_BYTES + 1);
        let mut t = Transport::new(Cursor::new(naglowek.into_bytes()), Vec::new());
        assert!(t.read().is_err());
    }

    #[test]
    fn brak_naglowka_content_length_to_blad() {
        let mut t = Transport::new(Cursor::new(b"X-Nic: 1\r\n\r\n{}".to_vec()), Vec::new());
        assert!(t.read().is_err());
    }

    #[test]
    fn naglowek_content_type_jest_pomijany() {
        // Tak wygląda prawdziwa ramka z VS Code: dwa nagłówki.
        let wiadomosc = Message::request(1, "initialize", json!({}));
        let payload = serde_json::to_vec(&wiadomosc).unwrap();
        let bajty = format!(
            "Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n",
            payload.len()
        )
        .into_bytes()
        .into_iter()
        .chain(payload)
        .collect::<Vec<u8>>();

        let mut t = Transport::new(Cursor::new(bajty), Vec::new());
        assert_eq!(t.read().unwrap().unwrap().method(), Some("initialize"));
    }

    #[test]
    fn ramka_ze_zlym_jsonem_to_blad() {
        let mut t = Transport::new(
            Cursor::new(b"Content-Length: 2\r\n\r\n{}".to_vec()),
            Vec::new(),
        );
        assert!(t.read().is_err(), "puste `{{}}` to nie wiadomość JSON-RPC");
    }

    #[test]
    fn ramka_bez_id_i_metody_to_blad() {
        let mut t = Transport::new(
            Cursor::new(b"Content-Length: 17\r\n\r\n{\"jsonrpc\":\"2.0\"}\n".to_vec()),
            Vec::new(),
        );
        assert!(t.read().is_err());
    }
}
