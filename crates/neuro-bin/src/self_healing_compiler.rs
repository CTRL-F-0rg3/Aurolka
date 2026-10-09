//! Samonaprawiający się kompilator: kod Beef → walidacja → korekta → kod.
//!
//! Kompilatora Beef (`bfc`) nie ma w systemie, więc walidacja jest
//! **strukturalna** i deterministyczna: liczymy nawiasy klamrowe, sprawdzamy
//! wymagane konstrukcje z XML (klasy, funkcje) oraz to, czy każda instrukcja
//! kończy się średnikiem. Błąd zamieniany jest na impuls korygujący, który
//! wstrzykujemy w macierz GPU, a neurony w kolejnym przebiegu dostarczają
//! wzorzec dla brakującej konstrukcji.

use crate::beef_decoder::BeefDecoder;
use crate::report::Report;
use neuro_lib::{Machine, MachineState, ScenarioEngine};

/// Shader macierzy — ten sam, którym liczy reszta systemu.
const SCENARIO_SHADER: &str = include_str!("../shaiders/scenario_matrix.wgsl");

/// Rozmiar macierzy korekcyjnej (komórki).
const FIX_MATRIX_SIZE: u32 = 64;

/// Liczba form neuronów używanych do korekty.
const FIX_NEURONS: u32 = 16;

/// Cykli GPU na jedną próbę korekty.
const CYCLES_PER_FIX: u32 = 2;

/// Wynik samonaprawy.
pub struct HealingOutcome {
    /// Finalny kod Beef.
    pub code: String,
    /// Liczba wykonanych przebiegów (1 = kod był poprawny od razu).
    pub attempts: u32,
    /// Czy kod przeszedł walidację strukturalną.
    pub valid: bool,
    /// Lista opisów błędów z ostatniej walidacji (pusta = bez błędów).
    pub errors: Vec<String>,
}

/// Wymagana konstrukcja z XML: `("class", "CalculatorWindow")` lub `("fn", "RenderUI")`.
#[derive(Debug, Clone)]
pub struct Requirement {
    /// Rodzaj: `class` albo `fn`.
    pub kind: &'static str,
    /// Nazwa konstrukcji.
    pub name: String,
}

/// Kompilator z pętlą samonaprawy.
pub struct HealingCompiler;

impl HealingCompiler {
    /// Waliduje kod Beef strukturalnie (bez `bfc`) i zwraca listę błędów.
    pub fn validate(code: &str, requirements: &[Requirement]) -> Vec<String> {
        let mut errors = Vec::new();

        // 1. Równowaga nawiasów klamrowych.
        let balance = code.matches('{').count() as i32 - code.matches('}').count() as i32;
        if balance != 0 {
            errors.push(format!("nierównowaga klamrowa: {balance}"));
        }
        // Równowaga nawiasów okrągłych.
        let parens = code.matches('(').count() as i32 - code.matches(')').count() as i32;
        if parens != 0 {
            errors.push(format!("nierównowaga nawiasów okrągłych: {parens}"));
        }

        // 2. Wymagane konstrukcje z XML.
        for requirement in requirements {
            let found = match requirement.kind {
                "class" => code.contains(&format!("class {}", requirement.name)),
                "fn" => {
                    code.contains(&format!("{}(", requirement.name))
                        || code.contains(&format!("void {}", requirement.name))
                }
                _ => code.contains(&requirement.name),
            };
            if !found {
                errors.push(format!(
                    "brak wymaganej konstrukcji {} `{}`",
                    requirement.kind, requirement.name
                ));
            }
        }

        // 3. Średniki: każda linia z instrukcją kończy się `;`, `{` albo `}`.
        for (number, line) in code.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            let closes = trimmed.ends_with(';')
                || trimmed.ends_with('{')
                || trimmed.ends_with('}')
                || trimmed.ends_with(',');
            if !closes && !trimmed.starts_with("class") && !trimmed.starts_with("namespace") {
                errors.push(format!("linia {}: brak średnika na końcu", number + 1));
            }
        }

        errors
    }

    /// Próbuje naprawić kod przez pętlę: walidacja → impuls korygujący → GPU.
    ///
    /// `max_attempts` to liczba *korekt* po pierwszym przebiegu.
    pub fn compile_and_fix(
        report: &mut Report,
        code: &str,
        requirements: &[Requirement],
        max_attempts: u32,
    ) -> HealingOutcome {
        let mut machine = Machine::new();
        let mut current = code.to_string();

        let start = machine.transition(MachineState::Verifying, "walidacja wygenerowanego kodu Beef");
        report.line(start.to_string());

        for attempt in 1..=max_attempts {
            let errors = Self::validate(&current, requirements);
            if errors.is_empty() {
                let ok = machine.transition(
                    MachineState::Completed,
                    format!("kod Beef poprawny po {attempt} przebiegach"),
                );
                report.line(ok.to_string());
                return HealingOutcome {
                    code: current,
                    attempts: attempt,
                    valid: true,
                    errors: Vec::new(),
                };
            }

            for error in &errors {
                report.line(format!("  błąd: {error}"));
            }

            if attempt == max_attempts {
                break;
            }

            // Błąd → impuls korygujący w macierzy GPU → nowy wzorzec.
            let retry = machine.transition(
                MachineState::Retrying,
                format!("przebieg {attempt}: wstrzykuję impuls korygujący"),
            );
            report.line(retry.to_string());

            let impulse = Self::parse_errors_to_impulse(&errors.join("\n"));
            match ScenarioEngine::new(SCENARIO_SHADER, FIX_MATRIX_SIZE, FIX_NEURONS) {
                Ok(engine) => {
                    if engine.inject_stimulus(32, 32, impulse).is_err() {
                        report.line(String::from("  uwaga: nie udało się wstrzyknąć impulsu"));
                        continue;
                    }
                    let _ = engine.run_simulation_cycle(CYCLES_PER_FIX);
                    let cell = engine.read_pattern_cell(32, 32).unwrap_or(impulse);

                    // Neurony dostarczają wzorzec dla brakującej konstrukcji.
                    if let Some(decoded) = BeefDecoder::decode_to_beef(&cell, 0.0, 8.0, 0.99) {
                        report.line(format!(
                            "  korekta z GPU: `{}` → {}",
                            decoded.mnemonic,
                            BeefDecoder::first_line(&decoded.text)
                        ));
                        current = Self::apply_correction(&current, &decoded.text);
                    }
                }
                Err(error) => {
                    report.line(format!("  uwaga: GPU niedostępne ({error}) — korekta lokalna"));
                    current = Self::apply_local_correction(&current);
                }
            }
        }

        let failed = machine.transition(
            MachineState::Failed,
            format!("kod Beef nadal niepoprawny po {max_attempts} przebiegach"),
        );
        report.line(failed.to_string());

        let errors = Self::validate(&current, requirements);
        HealingOutcome {
            code: current,
            attempts: max_attempts,
            valid: false,
            errors,
        }
    }

    /// Zamienia komunikat błędu na wzorzec liczbowy (impuls korygujący).
    fn parse_errors_to_impulse(stderr: &str) -> [f32; 4] {
        if stderr.contains("klamrowa") || stderr.contains("Unexpected token") {
            [1.5, 0.5, 0.0, 0.0] // wymusza strukturę bloku
        } else if stderr.contains("brak wymaganej") {
            [0.5, 1.5, 0.0, 0.0] // wymusza deklarację klasy/funkcji
        } else if stderr.contains("nawias") {
            [0.0, 1.5, 0.5, 0.0] // wymusza domknięcie nawiasów
        } else {
            [0.1, 0.1, 0.1, 0.1] // ogólny szum korygujący
        }
    }

    /// Dokleja zdekodowaną linię do kodu (przed ostatnią klamrą).
    fn apply_correction(code: &str, line: &str) -> String {
        let mut out = code.to_string();
        match out.rfind('}') {
            Some(position) => out.insert_str(position, &format!("{line}\n")),
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    /// Lokalna korekta: domyka klamry i nawiasy, dodaje brakujące średniki.
    fn apply_local_correction(code: &str) -> String {
        let mut out = String::new();
        for line in code.lines() {
            let trimmed = line.trim_end();
            let needs_semicolon = !trimmed.is_empty()
                && !trimmed.ends_with(';')
                && !trimmed.ends_with('{')
                && !trimmed.ends_with('}')
                && !trimmed.ends_with(',')
                && !trimmed.starts_with("//")
                && !trimmed.starts_with("class")
                && !trimmed.starts_with("namespace");
            if needs_semicolon {
                out.push_str(trimmed);
                out.push(';');
            } else {
                out.push_str(trimmed);
            }
            out.push('\n');
        }

        let balance = out.matches('{').count() as i32 - out.matches('}').count() as i32;
        for _ in 0..balance.abs() {
            if balance > 0 {
                out.push_str("}\n");
            } else {
                out.insert_str(0, "{\n");
            }
        }

        let parens = out.matches('(').count() as i32 - out.matches(')').count() as i32;
        if parens > 0 {
            out.push_str(&")".repeat(parens as usize));
        }

        out
    }
}
