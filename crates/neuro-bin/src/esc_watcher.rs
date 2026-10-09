// crates/neuro-bin/src/esc_watcher.rs
//!
//! Nieblokujące nasłuchiwanie klawisza ESC (tryb raw terminala na Linuksie).

use std::io::Read;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

/// Nasłuchuje klawisza ESC w tle i ustawia flagę stopu.
pub struct EscWatcher {
    stop: Arc<AtomicBool>,
}

impl EscWatcher {
    /// Uruchamia wątek nasłuchujący. Gdy wejście nie jest terminalem
    /// (np. potok), wątek kończy się natychmiast i pętla nie zostanie
    /// zatrzymana przez ESC — to bezpieczne dla testów/CI.
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);

        thread::spawn(move || {
            let stdin = std::io::stdin();
            let fd = stdin.as_raw_fd();

            // Pobierz oryginalne ustawienia terminala.
            let original = unsafe {
                let mut term = std::mem::MaybeUninit::<libc::termios>::uninit();
                if libc::tcgetattr(fd, term.as_mut_ptr()) != 0 {
                    return; // wejście nie jest terminalem
                }
                term.assume_init()
            };

            // Przełącz w tryb raw, by ESC docierał bez wciskania Enter.
            let mut raw = original;
            unsafe { libc::cfmakeraw(&mut raw) };
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) };

            let mut buffer = [0u8; 16];
            let mut handle = stdin.lock();
            while !stop_flag.load(Ordering::Relaxed) {
                match handle.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) if buffer[..n].contains(&0x1b) => {
                        stop_flag.store(true, Ordering::Relaxed);
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }

            // Przywróć oryginalny tryb terminala.
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };
        });

        Self { stop }
    }

    /// Czy użytkownik nacisnął ESC?
    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}
