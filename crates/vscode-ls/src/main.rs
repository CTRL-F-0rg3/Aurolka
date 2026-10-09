//! Serwer językowy `vscode-ls` — punkt wejścia procesu.
//!
//! Czyta wiadomości JSON-RPC ze standardowego wejścia, obsługuje je przez
//! [`vscode_bridge::Server`] i odpowiada na standardowym wyjściu. Klient
//! (VS Code albo nasza wtyczka) uruchamia proces i gada z nim przez stdio.
//!
//! ```text
//! cargo run -p vscode-ls
//! ```

use std::io::{self, BufReader};

use vscode_bridge::error::Result;
use vscode_bridge::host::{Host, LogLevel};
use vscode_bridge::protocol::Transport;
use vscode_bridge::server::Server;

fn main() {
    if let Err(error) = uruchom() {
        // Serwer nie ma się do czego zgłosić poza stderr — VS Code go pokaże.
        eprintln!("vscode-ls: {error}");
        std::process::exit(1);
    }
}

/// Pętla serwera: czytaj, obsłuż, odpowiedz.
fn uruchom() -> Result<()> {
    let wejscie = BufReader::new(io::stdin());
    let wyjscie = io::stdout();

    let mut transport = Transport::new(wejscie, wyjscie);
    let mut server = Server::new();
    let mut ostatni_id = 1;

    {
        let mut host = Host::new(&mut transport, ostatni_id);
        host.log(LogLevel::Info, "vscode-ls wystartował")?;
        ostatni_id = host.last_id();
    }

    loop {
        let wiadomosc = match transport.read() {
            Ok(Some(message)) => message,
            Ok(None) => break, // klient zamknął kanał
            Err(error) => {
                // Uszkodzona ramka to nie koniec serwera — zgłaszamy i jedziemy dalej.
                eprintln!("vscode-ls: błąd odczytu: {error}");
                continue;
            }
        };

        let metoda = wiadomosc.method().unwrap_or("<odpowiedź>").to_owned();

        // Host żyje tylko tyle, ile trwa obsługa jednej wiadomości — inaczej
        // `&mut transport` byłby pożyczony na całą pętlę.
        let odpowiedz = {
            let mut host = Host::new(&mut transport, ostatni_id);
            let wynik = server.handle(&wiadomosc, &mut host);
            ostatni_id = host.last_id();
            wynik?
        };

        if let Some(odpowiedz) = odpowiedz {
            transport.send(&odpowiedz)?;
        }

        // `exit` kończy proces — tak przewiduje LSP.
        if metoda == "exit" {
            return Ok(());
        }
    }

    Ok(())
}
