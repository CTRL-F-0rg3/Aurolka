// crates/neuro-bin/src/compiler_learner.rs
//!
//! Kompilator wielojęzykowy — próbuje skompilować wygenerowany kod i zwraca
//! wynik: sukces, błąd (do nauki) albo brak kompilatora.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::schema_generator::SchemaGenerator;

/// Wynik próby kompilacji.
#[derive(Debug)]
pub enum CompileOutcome {
    /// Kod się skompilował — wzorzec korzystny.
    Success,
    /// Kompilacja nie powiodła się (stderr do analizy) — wzorzec niekorzystny.
    Failure(String),
    /// Brak kompilatora w systemie — nie oceniamy kodu.
    Unavailable(String),
}

pub struct CompilerLearner;

impl CompilerLearner {
    /// Kompiluje `code` (język `lang`) w katalogu sandbox i zwraca wynik.
    pub fn compile(
        &self,
        lang: &str,
        module_name: &str,
        code: &str,
        sandbox_dir: &Path,
    ) -> CompileOutcome {
        let dir = sandbox_dir.join(module_name);
        fs::create_dir_all(&dir).ok();

        if lang == "rust" {
            return self.compile_rust(&dir, module_name, code);
        }

        let ext = SchemaGenerator::extension(lang);
        let file = dir.join(format!("{module_name}.{ext}"));
        fs::write(&file, code).ok();

        let (tool, args): (&str, Vec<&str>) = match lang {
            "cpp" => ("g++", vec!["-std=c++20", "-fsyntax-only"]),
            "c" => ("gcc", vec!["-std=c11", "-fsyntax-only"]),
            "zig" => ("zig", vec!["build-exe", "-femit-bin=/dev/null"]),
            "odin" => ("odin", vec!["build", "-file", "-no-entry-point"]),
            other => return CompileOutcome::Unavailable(format!("nieznany język: {other}")),
        };

        match Command::new(tool).args(args).arg(&file).output() {
            Ok(out) if out.status.success() => CompileOutcome::Success,
            Ok(out) => CompileOutcome::Failure(String::from_utf8_lossy(&out.stderr).to_string()),
            Err(error) => CompileOutcome::Unavailable(format!("{tool}: {error}")),
        }
    }

    fn compile_rust(&self, dir: &Path, module_name: &str, code: &str) -> CompileOutcome {
        fs::create_dir_all(dir.join("src")).ok();
        fs::write(dir.join("src/lib.rs"), code).ok();

        // Samodzielna skrzynka (własny workspace) — bez zależności.
        let manifest = format!(
            "[package]\nname = \"{module_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n"
        );
        fs::write(dir.join("Cargo.toml"), manifest).ok();

        let manifest_path = dir.join("Cargo.toml");
        match Command::new("cargo")
            .args(["check", "--offline", "--manifest-path"])
            .arg(&manifest_path)
            .output()
        {
            Ok(out) if out.status.success() => CompileOutcome::Success,
            Ok(out) => CompileOutcome::Failure(String::from_utf8_lossy(&out.stderr).to_string()),
            Err(error) => CompileOutcome::Unavailable(format!("cargo: {error}")),
        }
    }
}
