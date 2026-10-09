# vscode-bridge

Most między aplikacją napisaną w Ruście a Visual Studio Code.

Biblioteka robi dwie rzeczy, których nie ma w gotowych rozwiązaniach:

* **serwer językowy** dla Rust, TypeScript i JavaScript, który startuje natychmiast
  i działa offline — bez `rust-analyzer` i bez `tsserver`;
* **kanał zwrotny** (`Host`), przez który serwer **prosi** edytor o komunikat,
  otwarcie pliku, wstawienie tekstu albo uruchomienie komendy.

Zero zależności czasu działania poza `serde`, `serde_json`, `log` i `aurum`.
Brak tokio, brak `async`, brak npm.

## Jak to działa

```text
Visual Studio Code ⇄ JSON-RPC 2.0 (Content-Length) ⇄ vscode-bridge ⇄ pliki
```

Serwer odbiera `didOpen` / `didChange`, analizuje dokument i wysyła
`publishDiagnostics`. Pozycje liczone są w **jednostkach UTF-16** — tak, jak
liczy je VS Code, więc podkreślenia nie zjeżdżają o znak przy emoji.

## Szybki start

```bash
cargo run -p vscode-ls            # serwer LSP po stdio
```

Podłączenie do istniejącego projektu:

```json
{ "name": "Aurola", "language": "rust", "command": "vscode-ls" }
```

## Moduły

| Moduł | Rola |
|---|---|
| [`protocol`] | ramkowanie JSON-RPC 2.0 (`Content-Length`) |
| [`lsp`] | typy protokołu: pozycje, zasięgi, diagnostyka |
| [`workspace`] | URI ↔ ścieżka, magazyn otwartych dokumentów |
| [`analysis`] | wbudowany analizator Rust / TypeScript / JavaScript |
| [`symbols`] | symbole dokumentu dla panelu „Outline” |
| [`tools`] | delegowanie do `cargo` i `tsc` |
| [`host`] | kanał zwrotny: komunikaty, edycje, komendy |
| [`server`] | pętla wiadomości i obsługa metod LSP |
| [`panel`] | layout paneli liczony `aurum::math` |

## Obsługiwane metody

Żądania:

| Metoda | Działanie |
|---|---|
| `initialize` | zapamiętuje katalogi, deklaruje możliwości, **echo**uje ID żądania |
| `shutdown` / `exit` | standardowy cykl życia; kod wyjścia 0 tylko po `shutdown` |
| `textDocument/documentSymbol` | symbole do panelu „Outline” |
| `workspace/executeCommand` | `vscodeBridge.status`, `.reanalyzeAll`, `.reanalyzeFile` |

Powiadomienia:

| Metoda | Działanie |
|---|---|
| `textDocument/didOpen` / `didChange` / `didClose` | synchronizacja tekstu i diagnostyka |
| `textDocument/didSave` | ponowna analiza jednego pliku |
| `workspace/didChangeConfiguration` | ponowna analiza wszystkiego |
| `initialized`, `$/cancelRequest`, `$/setTrace` | pomijane |

Każda wiadomość wychodząca ma `"jsonrpc": "2.0"` — pole wymagane przez
specyfikację i przez ścisłą walidację klienta VS Code.

## Kanał zwrotny

Serwer nie tylko przyjmuje — potrafi też działać na edytorze:

```rust,ignore
host.show_information("Gotowe")?;
host.open_document("file:///tmp/agh.rs")?;
host.insert_text("file:///tmp/agh.rs", Position::new(0, 0), "// wygenerowane\n")?;
host.execute_command("workbench.action.terminal.sendSequence", vec![json!("cargo test")])?;
host.set_status("80%")?;
```

Rozszerzenie rozpoznaje metody `vscode/open`, `vscode/insertText`,
`vscode/setStatus`, `window/*` oraz `workspace/executeCommand`.

## Testowanie

Cała biblioteka testuje się **bez VS Code** — transport podstawiamy na
bufor w pamięci:

```bash
cargo test -p vscode-bridge            # 74 jednostkowe + 17 integracyjnych + 10 doctestów
```

`tests/protocol.rs` sprawdza pełną ścieżkę: klient → ramka → serwer →
diagnostyka → ramka → klient.

## Granice

* Wbudowany analizator **nie** jest pełną analizą semantyczną — szuka błędów
  składniowych i typowych pułapek (`debugger`, `var`, `== null`, `use`
  wewnątrz funkcji, `#[derive(Debug)]` bez `use std::…`). Dokładną semantykę
  dają `cargo check` i `tsc` z modułu [`tools`].
* `documentSymbol` zwraca **płaską** listę (`SymbolInformation`), bez drzewa
  zagnieżdżonego — wystarcza do nawigacji i zwijania bloków.
* Serwer obsługuje wariant **pełnego tekstu** w `didChange`; przyrostkowy
  (`range`) jest ignorowany.