# Aurola — most do Visual Studio Code

Rozszerzenie uruchamia `vscode-ls` — serwer językowy napisany w Ruście —
i pokazuje jego diagnostykę w edytorze. Bez zależności: czysty Node
i `child_process`.

Obsługuje **Rust**, **TypeScript** i **JavaScript**.

## Instalacja

### 1. Zbuduj serwer

```bash
cargo build -p vscode-ls
```

Powstanie `target/debug/vscode-ls`.

### 2. Zainstaluj rozszerzenie

```bash
cd crates/vscode-ls/extension
code --install-extension .
```

Albo bez instalowania: **F5** w VS Code (tryb debug uruchamia Extension Host
i ładuje rozszerzenie z tego katalogu).

### 3. Wskaż serwer, jeśli nie jest w PATH

Rozszerzenie szuka `vscode-ls` w kolejności:

1. ustawienie `aurola.serverPath`,
2. `target/debug/vscode-ls` w katalogu projektu,
3. `vscode-ls` w `PATH`.

Jeśli serwer leży w innym miejscu:

```json
{ "aurola.serverPath": "/pełna/ścieżka/do/vscode-ls" }
```

## Komendy

| Komenda | Skrót | Działanie |
|---|---|---|
| `Aurola: stan serwera` | — | pokazuje liczbę śledzonych plików |
| `Aurola: sprawdź ponownie wszystkie pliki` | `Ctrl+Alt+R` | wymusza ponowną analizę |
| `Aurola: uruchom serwer ponownie` | — | restart procesu |
| `Aurola: pokaż kanał wyjściowy` | — | otwiera log |

## Co serwer sprawdza

Wbudowany analizator działa natychmiast i offline:

* niezamknięte i nadmiarowe nawiasy (skaner pomija zawartość napisów),
* `debugger;` w kodzie produkcyjnym,
* `var` w TypeScripcie i JavaScripcie,
* `== null` zamiast `=== null`,
* `use` wewnątrz funkcji,
* `#[derive(Debug)]` bez `use std::…`.

Panel **Outline** pokazuje symbole dokumentu: funkcje, struktury, enumy,
traity, implementacje, moduły, klasy i typy.

## Diagnostyka serwera

```bash
# wyświetl kanał wyjściowy
code --command workbench.action.output.toggleOutput
```

albo komenda **Aurola: pokaż kanał wyjściowy**. Serwer sam pisze tam start,
błędy odczytu ramki i nieznane metody.

## Testy

```bash
cd crates/vscode-ls/extension
npm test        # node --test test/  → 10 testów parsera ramkującego
npm run lint    # node --check client.js
```

Testy działają w czystym Node — atrapa modułu `vscode` podmienia Extension
Host, więc sprawdzamy logikę ramkowania, a nie integrację z edytorem.

## Ograniczenia

* To warstwa **pierwsza**: wyłapie błędy składniowe i typowe pułapki, ale nie
  zrobi pełnej analizy typów. Do tego służą `rust-analyzer` i `tsserver`.
* Symbole dokumentu są **płaską listą**, bez zagnieżdżania.
* `didChange` obsługuje wariant pełnego tekstu, nie przyrostkowy.