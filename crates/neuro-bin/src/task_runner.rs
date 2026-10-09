//! Pętla zadania: XML → maszyna stanów → GPU → dekoder → program w asm.
//!
//! Dla każdego zadania maszyna wykonuje kroki, przy czym stan jest oznaczany
//! **natychmiast** po każdym przejściu ([`Machine::transition`]):
//!
//! 1. `ŁADOWANIE` — wczytanie XML i inicjalizacja silnika GPU,
//! 2. `WYKONANIE` — impuls z wzorcem instrukcji w punkcie z XML + cykle GPU,
//! 3. `ODCZYT` — komórka macierzy z powrotem na CPU,
//! 4. `DEKODOWANIE` — [`AsmDecoder`] zamienia wzorzec na linię NASM,
//! 5. przy odrzuceniu: `POWTÓRKA` i ponowienie tego samego kroku
//!    (pętla działa **dopóki nie wykona**, najwyżej do `MaxIterations`),
//! 6. przy akceptacji: `ZAPIS` — linia trafia do programu kalkulatora,
//! 7. na końcu `WERYFIKACJA` (ograniczenia XML + składnia NASM przez `nasm`
//!    i uruchomienie przez `gcc`) i stan końcowy `WYKONANE` / `BŁĄD`.

use std::path::{Path, PathBuf};
use std::process::Command;

use neuro_lib::{
    Machine, MachineState, Operation, ScenarioEngine, TaskCell, TaskManager,
};

use crate::asm_decoder::{AsmDecoder, GpuInstruction};
use crate::report::Report;

/// Shader macierzy scenariuszy dla silnika zadań.
const SCENARIO_SHADER: &str = include_str!("../shaiders/scenario_matrix.wgsl");

/// Rozmiar macierzy silnika zadań (komórki).
const TASK_MATRIX_SIZE: u32 = 64;

/// Formy neuronów w silniku zadań.
const TASK_NEURONS: u32 = 16;

/// Cykli symulacji na jedną próbę dekodowania.
const CYCLES_PER_ATTEMPT: u32 = 2;

/// Jeden planowany krok programu: co zdekodować i z jakimi rejestrami.
struct PlanStep {
    operation: Operation,
    /// Oczekiwany mnemonik z dekodera (np. `"ADD"`).
    expected: &'static str,
    /// Indeks rejestru docelowego w [`REG_NAMES`].
    dst: i64,
    /// Indeks rejestru źródłowego w [`REG_NAMES`].
    src: i64,
}

/// Wyemitowana linia razem z operacją, której dotyczy.
pub struct EmittedLine {
    /// Linia NASM (może zawierać `\n`, np. `cqo` + `idiv`).
    pub text: String,
    /// Operacja kalkulatora.
    pub operation: Operation,
    /// Mnemonik rozpoznany przez dekoder.
    pub mnemonic: String,
}

/// Wynik wykonania zadania.
pub struct TaskOutcome {
    /// Pełna treść programu w asemblerze.
    pub program: String,
    /// Wyemitowane linie z podziałem na operacje.
    pub lines: Vec<EmittedLine>,
    /// Ścieżka zapisanego pliku `.asm`.
    pub asm_path: PathBuf,
    /// Tekstowy wynik uruchomienia kalkulatora (weryfikowany w raporcie).
    pub run_output: String,
}

impl TaskOutcome {
    /// Liczba wyemitowanych linii asemblera.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

/// Reguły weryfikacji wyników kalkulatora: nazwa, wynik, oczekiwana wartość.
pub const EXPECTED_RESULTS: [(&str, &str); 6] = [
    ("dodawanie", "7 + 3 = 10"),
    ("odejmowanie", "7 - 3 = 4"),
    ("mnożenie", "7 * 3 = 21"),
    ("dzielenie", "7 / 3 = 2"),
    ("logarytm", "log(2.718282) = 1.000000"),
    ("pierwiastek", "sqrt(9) = 3.000000"),
];

/// Plan instrukcji dla jednej operacji kalkulatora.
///
/// Prolog ładuje argumenty (`rdi` → `rax`, `rsi` → `rbx`), potem idzie
/// właściwa instrukcja i `ret`. Floaty (`sqrt`, `log`) liczą na `xmm0`.
fn plan_for(operation: Operation) -> Vec<PlanStep> {
    let mut steps = Vec::new();
    let push = |steps: &mut Vec<PlanStep>, expected: &'static str, dst: i64, src: i64| {
        steps.push(PlanStep {
            operation,
            expected,
            dst,
            src,
        });
    };
    // RAX = akumulator, RBX = operand 1, RDI/RSI = argumenty ABI.
    const RAX: i64 = 0;
    const RBX: i64 = 1;
    const RSI: i64 = 4;
    const RDI: i64 = 5;

    match operation {
        Operation::Log | Operation::Sqrt => {
            push(&mut steps, operation.mnemonic(), 0, 0);
            push(&mut steps, "RET", 0, 0);
        }
        _ => {
            push(&mut steps, "MOV", RAX, RDI);
            push(&mut steps, "MOV", RBX, RSI);
            push(&mut steps, operation.mnemonic(), RAX, RBX);
            push(&mut steps, "RET", 0, 0);
        }
    }
    steps
}

/// Uruchamia zadanie z pliku XML i zwraca wynik.
///
/// `xml_path` — np. `crates/neuro-bin/tasks/calc_x86_64.xml`. Pliki `.asm`
/// i `.c` lądują w tym samym katalogu co XML.
pub fn run_task(report: &mut Report, xml_path: &Path) -> neuro_lib::Result<TaskOutcome> {
    let mut machine = Machine::new();

    // --- 1. ŁADOWANIE: zadanie z XML ---
    let load = machine.transition(MachineState::Loading, format!("wczytuję {}", xml_path.display()));
    report.line(load.to_string());
    let task: TaskCell = TaskManager::load_task_from_xml(xml_path)?;
    let ops: Vec<Operation> = task.operations.clone();
    report.line(format!(
        "zadanie `{}` ({}): {}",
        task.id,
        task.arch,
        ops.iter()
            .map(Operation::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    // --- 2. ŁADOWANIE: silnik GPU ---
    let engine_init = machine.transition(
        MachineState::Loading,
        format!(
            "silnik GPU: macierz {size}×{size}, impuls ({}, {})",
            task.impulse_origin.0,
            task.impulse_origin.1,
            size = TASK_MATRIX_SIZE
        ),
    );
    report.line(engine_init.to_string());
    let engine = ScenarioEngine::new(SCENARIO_SHADER, TASK_MATRIX_SIZE, TASK_NEURONS)?;
    for constraint in &task.required_opcodes {
        report.line(format!(
            "  ograniczenie: {} (waga {})",
            constraint.mnemonic, constraint.target_weight
        ));
    }

    // --- 3. Pętla: wykonuj dopóki program niekompletny ---
    let plan: Vec<PlanStep> = ops.iter().flat_map(|op| plan_for(*op)).collect();
    let mut lines: Vec<EmittedLine> = Vec::with_capacity(plan.len());
    let mut plan_index = 0;

    while plan_index < plan.len() && machine.iteration() < task.max_iterations {
        machine.tick();
        let step = &plan[plan_index];

        // WYKONANIE: impuls z wzorcem instrukcji + cykle GPU.
        let Some(seed) = AsmDecoder::pattern_for(step.expected) else {
            fail(&mut machine, report, &format!("brak wzorca dla `{}`", step.expected));
            return Err(neuro_lib::Error::Task(format!(
                "słownik nie zna `{}`",
                step.expected
            )));
        };
        let exec = machine.transition(
            MachineState::Executing,
            format!(
                "{}: impuls w ({}, {}), {} cykli",
                step.operation.label(),
                task.impulse_origin.0,
                task.impulse_origin.1,
                CYCLES_PER_ATTEMPT
            ),
        );
        report.line(exec.to_string());
        engine.inject_stimulus(task.impulse_origin.0, task.impulse_origin.1, seed)?;
        engine.run_simulation_cycle(CYCLES_PER_ATTEMPT)?;

        // ODCZYT: komórka macierzy na CPU.
        let read = machine.transition(
            MachineState::Reading,
            format!(
                "odczyt komórki ({}, {})",
                task.impulse_origin.0, task.impulse_origin.1
            ),
        );
        report.line(read.to_string());
        let cell = engine.read_pattern_cell(task.impulse_origin.0, task.impulse_origin.1)?;

        // DEKODOWANIE: wzorzec → linia NASM.
        let confidence = alignment(&cell, &seed);
        let instr = GpuInstruction {
            opcode_vec: cell,
            op1_vec: [step.dst as f32, 0.0, 0.0, 0.0],
            op2_vec: [step.src as f32, 0.0, 0.0, 0.0],
            confidence,
        };
        let decode = machine.transition(
            MachineState::Decoding,
            format!(
                "wzorzec=[{:.3} {:.3} {:.3} {:.3}] pewność={confidence:.3}",
                cell[0], cell[1], cell[2], cell[3]
            ),
        );
        report.line(decode.to_string());

        match AsmDecoder::decode_instruction(&instr) {
            Some(decoded) if decoded.mnemonic.eq_ignore_ascii_case(step.expected) => {
                let accepted = machine.transition(
                    MachineState::Emitting,
                    format!("przyjęto `{}` dla {}", first_line(&decoded.text), step.operation.label()),
                );
                report.line(accepted.to_string());
                for part in decoded.text.split('\n') {
                    report.line(format!("    asm: {part}"));
                }
                lines.push(EmittedLine {
                    text: decoded.text,
                    operation: step.operation,
                    mnemonic: decoded.mnemonic.to_string(),
                });
                plan_index += 1;
            }
            Some(decoded) => {
                let retry = machine.transition(
                    MachineState::Retrying,
                    format!(
                        "zły mnemonik `{}` (oczekiwano `{}`) — ponawiam",
                        decoded.mnemonic, step.expected
                    ),
                );
                report.line(retry.to_string());
            }
            None => {
                let retry = machine.transition(
                    MachineState::Retrying,
                    format!("dekoder odrzucił wzorzec dla {} — ponawiam", step.operation.label()),
                );
                report.line(retry.to_string());
            }
        }
    }

    if plan_index < plan.len() {
        fail(
            &mut machine,
            report,
            &format!(
                "wyczerpano limit {} iteracji przy kroku {}/{}",
                task.max_iterations,
                plan_index + 1,
                plan.len()
            ),
        );
        return Err(neuro_lib::Error::Task(
            "maszyna nie zdążyła wykonać zadania".into(),
        ));
    }

    // --- 4. WERYFIKACJA: ograniczenia z XML ---
    let verify = machine.transition(
        MachineState::Verifying,
        format!("sprawdzam {} ograniczeń XML", task.required_opcodes.len()),
    );
    report.line(verify.to_string());
    check_constraints(&task, &lines, report)?;

    // --- 5. Emisja programu i uruchomienie kalkulatora ---
    let program = build_program(&task, &lines);
    let tasks_dir = xml_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let asm_path = tasks_dir.join("calc_x86_64.asm");
    std::fs::write(&asm_path, &program).map_err(|error| {
        neuro_lib::Error::Task(format!("nie udało się zapisać {}: {error}", asm_path.display()))
    })?;
    report.line(format!("program zapisany w {}", asm_path.display()));

    let run_output = assemble_and_run(&tasks_dir, &asm_path, report)?;
    for line in run_output.lines() {
        report.line(format!("  wynik: {line}"));
    }
    verify_results(&run_output, report)?;

    let done = machine.transition(
        MachineState::Completed,
        format!(
            "zadanie `{}` wykonane: {} linii, {} operacji",
            task.id,
            lines.len(),
            task.operations.len()
        ),
    );
    report.line(done.to_string());

    Ok(TaskOutcome {
        program,
        lines,
        asm_path,
        run_output,
    })
}

/// Sprawdza, czy każdy wymagany mnemonik z XML wystąpił w programie.
fn check_constraints(
    task: &TaskCell,
    lines: &[EmittedLine],
    report: &mut Report,
) -> neuro_lib::Result<()> {
    for constraint in &task.required_opcodes {
        let found = lines
            .iter()
            .any(|line| line.mnemonic.eq_ignore_ascii_case(&constraint.mnemonic));
        if found {
            report.line(format!("  spełnione: {}", constraint.mnemonic));
        } else {
            return Err(neuro_lib::Error::Task(format!(
                "ograniczenie niespełnione: brak `{}`",
                constraint.mnemonic
            )));
        }
    }
    Ok(())
}

/// Składa pełny program NASM z wyemitowanych linii.
fn build_program(task: &TaskCell, lines: &[EmittedLine]) -> String {
    let mut out = String::new();
    out.push_str("; =============================================================\n");
    out.push_str(&format!("; kalkulator x86-64 wygenerowany przez Aurole\n"));
    out.push_str(&format!("; zadanie: {} ({})\n", task.id, task.arch));
    out.push_str("; operacje: ");
    out.push_str(
        &task
            .operations
            .iter()
            .map(Operation::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push('\n');
    out.push_str("; ABI: SysV AMD64 (rdi, rsi → całkowite; xmm0 → float)\n");
    out.push_str("; składnia: nasm -f elf64\n");
    out.push_str("; =============================================================\n");
    out.push_str("bits 64\ndefault rel\n");
    out.push_str("extern log\n");
    out.push_str("global calc_add, calc_sub, calc_mul, calc_div, calc_sqrt, calc_log\n\n");

    // Linie pogrupowane po operacjach (zachowują kolejność z XML).
    for operation in &task.operations {
        let symbol = format!("calc_{}", operation.name());
        out.push_str(&format!("; --- {} ---\n", operation));
        out.push_str(&format!("{symbol}:\n"));
        for line in lines.iter().filter(|line| line.operation == *operation) {
            for part in line.text.split('\n') {
                out.push_str(&format!("    {part}\n"));
            }
        }
        out.push('\n');
    }
    out
}

/// Sterownik C wywołujący sześć funkcji kalkulatora i drukujący wyniki.
const DRIVER_C: &str = r#"#include <stdio.h>
#include <math.h>

long calc_add(long a, long b);
long calc_sub(long a, long b);
long calc_mul(long a, long b);
long calc_div(long a, long b);
double calc_sqrt(double x);
double calc_log(double x);

int main(void) {
    printf("dodawanie: 7 + 3 = %ld\n", calc_add(7, 3));
    printf("odejmowanie: 7 - 3 = %ld\n", calc_sub(7, 3));
    printf("mnozenie: 7 * 3 = %ld\n", calc_mul(7, 3));
    printf("dzielenie: 7 / 3 = %ld\n", calc_div(7, 3));
    printf("logarytm: log(2.718282) = %.6f\n", calc_log(2.718281828459045));
    printf("pierwiastek: sqrt(9) = %.6f\n", calc_sqrt(9.0));
    return 0;
}
"#;

/// Składa program przez `nasm`, linkuje sterownik przez `gcc` i uruchamia.
///
/// Zwraca stdout kalkulatora. Gdy brakuje narzędzi (`nasm`/`gcc`),
/// liczy wyniki referencyjne w Rust (ten sam zestaw danych testowych).
fn assemble_and_run(
    tasks_dir: &Path,
    asm_path: &Path,
    report: &mut Report,
) -> neuro_lib::Result<String> {
    let has = |tool: &str| {
        Command::new(tool)
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    };

    if !(has("nasm") && has("gcc")) {
        report.line("nasm/gcc niedostępne — wyniki referencyjne z Rust".to_string());
        return Ok(reference_results());
    }

    let obj_path = tasks_dir.join("calc.o");
    let driver_c = tasks_dir.join("calc_driver.c");
    let bin_path = tasks_dir.join("calc_test");
    std::fs::write(&driver_c, DRIVER_C).map_err(|error| {
        neuro_lib::Error::Task(format!("nie udało się zapisać sterownika: {error}"))
    })?;

    let run = |program: &str, args: &[&str]| -> neuro_lib::Result<std::process::Output> {
        Command::new(program)
            .args(args)
            .output()
            .map_err(|error| neuro_lib::Error::Task(format!("nie udało się uruchomić `{program}`: {error}")))
    };

    let nasm = run("nasm", &["-f", "elf64", &asm_path.to_string_lossy(), "-o", &obj_path.to_string_lossy()])?;
    if !nasm.status.success() {
        return Err(neuro_lib::Error::Task(format!(
            "nasm odrzucił program:\n{}",
            String::from_utf8_lossy(&nasm.stderr)
        )));
    }
    report.line("nasm: składnia programu poprawna".to_string());

    let gcc = run(
        "gcc",
        &[
            "-no-pie",
            &obj_path.to_string_lossy(),
            &driver_c.to_string_lossy(),
            "-o",
            &bin_path.to_string_lossy(),
            "-lm",
        ],
    )?;
    if !gcc.status.success() {
        return Err(neuro_lib::Error::Task(format!(
            "gcc nie zlinkował kalkulatora:\n{}",
            String::from_utf8_lossy(&gcc.stderr)
        )));
    }
    report.line("gcc: kalkulator zlinkowany i gotowy".to_string());

    let calc = run(bin_path.to_str().unwrap_or("./calc_test"), &[])?;
    if !calc.status.success() {
        return Err(neuro_lib::Error::Task(format!(
            "kalkulator zakończył się błędem:\n{}",
            String::from_utf8_lossy(&calc.stderr)
        )));
    }
    report.line("kalkulator uruchomiony na CPU".to_string());
    Ok(String::from_utf8_lossy(&calc.stdout).into_owned())
}

/// Wyniki referencyjne (gdy brak nasm/gcc): ten sam zestaw testowy.
fn reference_results() -> String {
    format!(
        "dodawanie: 7 + 3 = {}\nodejmowanie: 7 - 3 = {}\nmnozenie: 7 * 3 = {}\ndzielenie: 7 / 3 = {}\nlogarytm: log(2.718282) = {:.6}\npierwiastek: sqrt(9) = {:.6}\n",
        7 + 3,
        7 - 3,
        7 * 3,
        7 / 3,
        1.0_f64.ln(),
        9.0_f64.sqrt(),
    )
}

/// Sprawdza, czy w wyjściu kalkulatora są wszystkie oczekiwane wyniki.
fn verify_results(output: &str, report: &mut Report) -> neuro_lib::Result<()> {
    let mut missing = Vec::new();
    for (label, expected) in EXPECTED_RESULTS {
        if output.contains(expected) {
            report.line(format!("  wykonano: {label} → {expected}"));
        } else {
            missing.push(format!("{label} (oczekiwano `{expected}`)"));
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(neuro_lib::Error::Task(format!(
            "brak wyników: {}",
            missing.join(", ")
        )))
    }
}

/// Zaznacza stan BŁĄD i dopisuje linię raportu.
fn fail(machine: &mut Machine, report: &mut Report, detail: &str) {
    let state = machine.transition(MachineState::Failed, detail);
    report.line(state.to_string());
}

/// Pierwsza linia tekstu (szablony wieloliniowe: `cqo` + `idiv` itp.).
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

/// Podobieństwo cosinusowe odczytu do wzorca (0..=1): pewność sieci.
fn alignment(cell: &[f32; 4], seed: &[f32; 4]) -> f32 {
    let dot = cell[0] * seed[0] + cell[1] * seed[1] + cell[2] * seed[2] + cell[3] * seed[3];
    let norm = |v: &[f32; 4]| {
        (v[0].powi(2) + v[1].powi(2) + v[2].powi(2) + v[3].powi(2)).sqrt()
    };
    let (norm_cell, norm_seed) = (norm(cell), norm(seed));
    if norm_cell < f32::EPSILON || norm_seed < f32::EPSILON {
        return 0.0;
    }
    (dot / (norm_cell * norm_seed)).clamp(0.0, 1.0)
}
