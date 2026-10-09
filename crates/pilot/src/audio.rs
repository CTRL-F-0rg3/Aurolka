//! Odtwarzanie audio w tle.
//!
//! UI nie może blokować na dekodowaniu plików, więc cały odtwarzacz żyje
//! w osobnym wątku, a stan (co gra, ile upłynęło) wystawiony przez
//! `Arc<Mutex<Playback>>`.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, Player};

/// Formaty odtwarzane przez pilota.
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "ogg", "m4a", "aac", "opus"];

/// Jak często odświeżamy pozycję utworu (200 ms ≈ 5 FPS wystarczy).
const REFRESH_INTERVAL: Duration = Duration::from_millis(200);

/// Polecenie dla wątku odtwarzacza.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Wczytaj nową bibliotekę z folderu.
    Rescan(PathBuf),
    /// Odtwórz (jeśli zatrzymane) lub wstrzymaj.
    Toggle,
    /// Następny utwór.
    Next,
    /// Poprzedni utwór.
    Previous,
}

/// Stan odtwarzacza widoczny dla UI.
#[derive(Debug, Default, Clone)]
pub struct Playback {
    /// Utwory znalezione w folderze.
    pub tracks: Vec<PathBuf>,
    /// Indeks aktualnie odtwarzanego utworu.
    pub index: Option<usize>,
    /// Czy trwa odtwarzanie.
    pub playing: bool,
    /// Ile czasu upłynęło na bieżącym utworze.
    pub elapsed: Duration,
    /// Długość bieżącego utworu (`None`, gdy nie znamy).
    pub duration: Option<Duration>,
    /// Ostatni błąd (np. brak plików audio).
    pub error: Option<String>,
}

/// Uchwyt na stan odtwarzacza.
pub type Shared = Arc<Mutex<Playback>>;

/// Czy dany plik wygląda na utwór audio.
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            AUDIO_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

/// Zbiera pliki audio z folderu (bez zagłębiania w podfoldery).
pub fn collect_tracks(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };

    let mut tracks: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_audio_file(path))
        .collect();

    // Sortowanie po nazwie pliku daje przewidywalną kolejność.
    tracks.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    tracks
}

/// Nazwa utworu bez rozszerzenia.
pub fn track_title(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "?".to_owned())
}

/// Wątek odtwarzacza audio.
/// Wątek odtwarzacza audio.
///
/// Odtwarzanie działa tak: `Player` trzyma kolejkę źródeł, a my po prostu
/// dokładamy kolejne utwory. Zmiana utworu to `clear()` + `append()`.
pub struct Engine {
    shared: Shared,
    commands: Receiver<Command>,
    /// Odtwarzacz dołączony do miksu. `None`, gdy brak urządzenia audio.
    player: Option<Player>,
}

impl Engine {
    /// Uruchamia wątek odtwarzania i zwraca uchwyt + nadawcę poleceń.
    ///
    /// Brak urządzenia audio nie jest błędem krytycznym — UI nadal działa,
    /// a w [`Playback::error`] pojawia się komunikat.
    pub fn spawn(folder: PathBuf) -> (Shared, Sender<Command>) {
        let shared: Shared = Arc::new(Mutex::new(Playback {
            tracks: collect_tracks(&folder),
            ..Playback::default()
        }));
        let (tx, rx) = mpsc::channel();

        let handle = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("pilot-audio".to_owned())
            .spawn(move || {
                let engine = Engine {
                    shared: handle,
                    commands: rx,
                    player: None,
                };
                engine.run();
            })
            .expect("nie udało się uruchomić wątku audio");

        (shared, tx)
    }

    fn run(mut self) {
        // Wyjście audio tworzymy raz — `Player` reaguje na `pause`/`play`,
        // więc przełączanie nie wymaga ponownej inicjalizacji urządzenia.
        self.player = DeviceSinkBuilder::from_default_device()
            .ok()
            .and_then(|builder| builder.open_stream().ok())
            .map(|sink| Player::connect_new(sink.mixer()));

        if self.player.is_none() {
            log::warn!("brak urządzenia audio — pilot będzie tylko pokazywał stan");
        }

        // `recv_timeout` zamiast `recv`: dzięki temu regularnie odświeżamy
        // pozycję w UI, zamiast czekać na kolejne kliknięcie.
        loop {
            match self.commands.recv_timeout(REFRESH_INTERVAL) {
                Ok(command) => self.handle(command),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if let Some(player) = &self.player {
                poll_position(player, &self.shared);
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Rescan(folder) => self.rescan(folder),
            Command::Toggle => self.toggle(),
            Command::Next => self.step(1),
            Command::Previous => self.step(-1),
        }
    }

    fn rescan(&mut self, folder: PathBuf) {
        let mut state = self.lock();
        state.tracks = collect_tracks(&folder);
        state.error = None;
        if state.tracks.is_empty() {
            state.error = Some(format!("brak plików w {}", folder.display()));
            state.index = None;
            state.playing = false;
        } else if state.index.is_none() || state.index.unwrap_or(0) >= state.tracks.len() {
            state.index = Some(0);
        }
    }

    fn toggle(&mut self) {
        let Some(player) = &self.player else {
            self.set_error("brak urządzenia audio");
            return;
        };

        let was_playing = {
            let mut state = self.lock();
            if state.tracks.is_empty() {
                state.error = Some("pusty folder z muzyką".to_owned());
                return;
            }
            if state.index.is_none() {
                state.index = Some(0);
            }
            let was_playing = state.playing;
            state.playing = !was_playing;
            was_playing
        };

        if was_playing {
            player.play();
        } else {
            player.pause();
        }
    }

    fn step(&mut self, delta: isize) {
        let path = {
            let mut state = self.lock();
            if state.tracks.is_empty() {
                state.error = Some("pusty folder z muzyką".to_owned());
                return;
            }
            let len = state.tracks.len() as isize;
            let current = state.index.unwrap_or(0) as isize;
            let next = ((current + delta) % len + len) % len;
            state.index = Some(next as usize);
            state.error = None;
            state.tracks[next as usize].clone()
        };

        let Some(player) = &self.player else {
            self.set_error("brak urządzenia audio");
            return;
        };

        if let Err(error) = play(player, &path, &self.shared) {
            let mut state = self.lock();
            state.error = Some(format!("{}: {error}", track_title(&path)));
            state.playing = false;
        }
    }

    /// Blokada na stan — nie panikuje przy zatrutym mutexie.
    fn lock(&self) -> std::sync::MutexGuard<'_, Playback> {
        match self.shared.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn set_error(&mut self, message: &str) {
        let mut state = self.lock();
        state.error = Some(message.to_owned());
        state.playing = false;
    }
}

/// Wczytuje plik do odtwarzacza i ustawia stan odtwarzanego utworu.
fn play(player: &Player, path: &Path, shared: &Shared) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let source = Decoder::new(BufReader::new(file))?;

    player.clear();
    player.append(source);
    player.play();

    // Wyzerujemy licznik — długości nie znamy, bo `Decoder` nie jej udostępnia.
    if let Ok(mut state) = shared.lock() {
        state.elapsed = Duration::ZERO;
        state.duration = None;
    }
    Ok(())
}

/// Uzupełnia licznik pozycji na podstawie `Player::get_pos()`.
///
/// Wołane cyklicznie, bo `Player` sam nie aktualizuje naszego stanu.
pub fn poll_position(player: &Player, shared: &Shared) {
    let position = player.get_pos();
    if let Ok(mut state) = shared.lock() {
        state.elapsed = position;
    }
}

/// Wysyła polecenie, ignorując zamknięty kanał.
pub fn send(sender: &Sender<Command>, command: Command) {
    let _ = sender.send(command);
}

/// Formatuje czas w postaci `m:ss`.
pub fn format_time(duration: Duration) -> String {
    let total = duration.as_secs();
    format!("{}:{:02}", total / 60, total % 60)
}

// ---------------------------------------------------------------- demo ----

/// Generuje krótkie pliki WAV, gdy folder z muzyką jest pusty.
///
/// Dzięki temu pilot da się sprawdzić od razu po `cargo run`, bez szukania
/// utworów. To czyste tony sinusoidalne — nie muzyka.
pub fn ensure_demo_tracks(folder: &Path) {
    if !collect_tracks(folder).is_empty() {
        return;
    }
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }

    const SAMPLE_RATE: u32 = 44_100;
    let demo = [
        ("01-ton-a.wav", 440.0_f32, 2.0_f32),
        ("02-ton-b.wav", 523.25, 2.0),
        ("03-ton-c.wav", 659.25, 2.0),
    ];

    for (name, frequency, seconds) in demo {
        let path = folder.join(name);
        if path.exists() {
            continue;
        }
        if let Some(bytes) = render_tone_wav(frequency, seconds, SAMPLE_RATE) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

/// Renderuje plik WAV (16-bit mono) z zanikającą sinusoidą.
fn render_tone_wav(frequency: f32, seconds: f32, sample_rate: u32) -> Option<Vec<u8>> {
    let total = (seconds * sample_rate as f32) as usize;
    if total == 0 {
        return None;
    }

    let mut samples: Vec<i16> = Vec::with_capacity(total);
    for index in 0..total {
        let t = index as f32 / sample_rate as f32;
        let progress = index as f32 / total as f32;

        // Obwiednia prostokątna łagodzi start i koniec tonu.
        let fade = (progress * 12.0).min(1.0) * ((1.0 - progress) * 12.0).min(1.0);
        let value = (t * frequency * std::f32::consts::TAU).sin() * 0.28 * fade;
        samples.push((value * i16::MAX as f32) as i16);
    }

    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);

    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // rozmiar bloku fmt
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // bajtów na sekundę
    bytes.extend_from_slice(&2u16.to_le_bytes()); // wyrównanie bloku
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bitów na próbkę
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }

    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rozpoznaje_formaty_audio() {
        assert!(is_audio_file(Path::new("utwór.mp3")));
        assert!(is_audio_file(Path::new("utwór.FLAC")));
        assert!(is_audio_file(Path::new("utwór.Wav")));
        assert!(!is_audio_file(Path::new("okładka.jpg")));
        assert!(!is_audio_file(Path::new("notatki.txt")));
        assert!(!is_audio_file(Path::new("bezrozszerzenia")));
    }

    #[test]
    fn pusty_folder_daje_pustą_listę() {
        let folder = std::env::temp_dir().join("pilot-test-empty");
        let _ = std::fs::remove_dir_all(&folder);
        assert!(collect_tracks(&folder).is_empty());
    }

    #[test]
    fn skanuje_i_sortuje_pliki() {
        let folder = std::env::temp_dir().join("pilot-test-scan");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("b.mp3"), []).unwrap();
        std::fs::write(folder.join("a.flac"), []).unwrap();
        std::fs::write(folder.join("c.txt"), []).unwrap();

        let tracks = collect_tracks(&folder);
        let names: Vec<String> = tracks
            .iter()
            .map(|path| track_title(path))
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["a", "b"]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn demo_tony_to_pliki_wav() {
        let bytes = render_tone_wav(440.0, 0.01, 44_100).expect("ton powinien się wyrenderować");

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");

        // Nagłówek 44 B + 176 400 B danych (0.01 s × 44100 Hz × 2 B).
        assert_eq!(bytes.len(), 44 + 882);
    }

    #[test]
    fn formatuje_czas() {
        assert_eq!(format_time(Duration::from_secs(0)), "0:00");
        assert_eq!(format_time(Duration::from_secs(65)), "1:05");
        assert_eq!(format_time(Duration::from_secs(600)), "10:00");
    }
}
