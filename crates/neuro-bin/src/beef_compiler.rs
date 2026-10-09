//! Próba prawdziwej kompilacji kodu Beef.
//!
//! Kolejno szukamy w systemie kompilatora Beef (`beef`, `BeefBuild`, `bfc`).
//! Gdy żadnego nie ma (typowa sytuacja — Beef wymaga IDE), wracamy do
//! walidacji strukturalnej [`HealingCompiler`]. Wynik jest ujednolicony,
//! żeby pętla uczenia widziała tylko `ok` + listę błędów + nazwę narzędzia.

use std::path::Path;
use std::process::Command;

use crate::self_healing_compiler::{HealingCompiler, Requirement};

/// Wynik próby kompilacji kodu Beef.
pub struct CompileReport {
    /// Czy kompilacja (albo walidacja zastępcza) zakończyła się sukcesem.
    pub ok: bool,
    /// Czy użyto prawdziwego kompilatora Beef (`false` = walidacja strukturalna).
    pub real_compiler: bool,
    /// Nazwa użytego narzędzia.
    pub tool: String,
    /// Lista błędów (pusta = sukces).
    pub errors: Vec<String>,
    /// Pełne wyjście narzędzia (do raportu).
    pub output: String,
}

/// Próbuje skompilować `code` (zapisany w `source_path`) prawdziwym
/// kompilatorem Beef; gdy go brak — waliduje strukturalnie.
pub fn compile(code: &str, source_path: &Path, requirements: &[Requirement]) -> CompileReport {
    if let Err(error) = std::fs::write(source_path, code) {
        return CompileReport {
            ok: false,
            real_compiler: false,
            tool: "zapis pliku".into(),
            errors: vec![format!("nie udało się zapisać {}: {error}", source_path.display())],
            output: String::new(),
        };
    }

    // Kolejność: `beef` (skrót), `BeefBuild` (oficjalny), `bfc`.
    let attempts: [(&str, &[&str]); 3] = [
        ("beef", &["build"]),
        ("BeefBuild", &["-workspace=.", "-run"]),
        ("bfc", &[]),
    ];

    for (tool, extra_args) in attempts {
        let mut command = Command::new(tool);
        command.args(extra_args).arg(source_path);
        match command.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                let ok = output.status.success();
                let mut errors = Vec::new();
                if !ok {
                    for line in stderr.lines().chain(stdout.lines()) {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        errors.push(trimmed.to_string());
                    }
                    if errors.is_empty() {
                        errors.push("kompilator zakończył się błędem bez komunikatu".into());
                    }
                }
                return CompileReport {
                    ok,
                    real_compiler: true,
                    tool: tool.into(),
                    errors,
                    output: format!("{stdout}\n{stderr}"),
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // tego narzędzia nie ma w PATH — próbujemy następne
            }
            Err(error) => {
                return CompileReport {
                    ok: false,
                    real_compiler: true,
                    tool: tool.into(),
                    errors: vec![format!("nie udało się uruchomić `{tool}`: {error}")],
                    output: String::new(),
                };
            }
        }
    }

    // Żadnego kompilatora Beef — walidacja strukturalna.
    let errors = HealingCompiler::validate(code, requirements);
    CompileReport {
        ok: errors.is_empty(),
        real_compiler: false,
        tool: "walidacja strukturalna".into(),
        errors,
        output: String::new(),
    }
}
