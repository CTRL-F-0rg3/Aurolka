# Folder z muzyką

Pilot skanuje **ten folder** (poz. `crates/pilot/music`) i odtwarza znalezione
pliki. Rozpoznawane rozszerzenia:

| Format | Obsługa |
|---|---|
| `mp3` | pełna |
| `flac` | pełna |
| `wav` | pełna |
| `ogg`, `m4a`, `aac`, `opus` | pełna |

## Jak dodać własną muzykę

```bash
cp ~/moja-piosenka.mp3 crates/pilot/music/
```

Potem w pilocie kliknij **⟳** (ponowne skanowanie). Kolejność odtwarzania
jest alfabetyczna według nazwy pliku.

## Inny folder

Ustaw zmienną środowiskową:

```bash
PILOT_MUSIC_DIR=~/Muzyka cargo run -p pilot
```

## Brak plików

Aplikacja **nie crashuje** — pokazuje komunikat „brak plików w …” i wszystkie
cztery przyciski pozostają aktywne, więc po dodaniu plików wystarczy kliknąć ⟳.

## Utwór demonstracyjny

Przy pierwszym uruchomieniu (gdy folder jest pusty) pilot generuje trzy krótkie
pliki WAV — dzięki temu od razu widać, czy wszystko działa, bez szukania
utworów. To pliki z tonami sinusoidalnymi, nie muzyka; usuń je, kiedy dodasz
własne.