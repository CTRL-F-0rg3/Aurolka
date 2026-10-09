//! Demonstracja `neuro-lib` — neurony i globalna macierz scenariuszy na GPU.
//!
//! ```bash
//! cargo run -p neuro-bin                       # raport trafia też do ./matrix.txt
//! NEURO_MATRIX_FILE=out.txt cargo run -p neuro-bin
//! ```
//!
//! Program robi trzy rzeczy:
//!
//! 1. liczy sieć neuronów przez `neuron_pipeline.wgsl` (kroki FIRE/MIGRATE/IDLE)
//!    i sprawdza uprawnienia Płatków,
//! 2. odpala `scenario_matrix.wgsl` — impuls w środku macierzy, adaptacja form
//!    neuronów, diffusion,
//! 3. **wszystko, co wypisuje na ekran, zapisuje też do `matrix.txt`**
//!    (tam samo trafia pełna siatka macierzy i formy neuronów).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use neuro_lib::{
    Decision, GpuSectorManager, Neuron, NeuronForm, PngImage, ScenarioMatrixManager,
    SectorAccessRights, Topology, VisualMemoryManager,
};

mod asm_decoder;
mod beef_compiler;
mod beef_decoder;
mod report;
mod schema_engine;
mod schema_learning;
mod self_healing_compiler;
mod task_runner;
mod window_preview;

use report::Report;

/// Pipeline sieci neuronów.
const NEURON_SHADER: &str = include_str!("neuron_pipeline.wgsl");

/// Pipeline globalnej macierzy scenariuszy.
const SCENARIO_SHADER: &str = include_str!("../shaiders/scenario_matrix.wgsl");

/// Pipeline pamięci wizualnej — uczy model odwzorować obraz referencyjny.
const VISUAL_MEMORY_SHADER: &str = include_str!("../shaiders/visual_memory.wgsl");

/// Cztery sektory hex po 256 neuronów.
const SECTOR_SIZES: [u32; 4] = [256, 256, 256, 256];

/// Liczba kroków symulacji neuronów.
const STEPS: u32 = 8;

/// Ile neuronów dostaje impuls wejściowy na starcie.
const PROBES: u32 = 8;

/// Bok macierzy scenariuszy w komórkach — musi być wielokrotnością 4.
const MATRIX_SIZE: u32 = 64;

/// Liczba form neuronów trzymanych przez macierz.
const MATRIX_NEURONS: u32 = 16;

/// Kroków diffusion po impulsie.
const MATRIX_STEPS: u32 = 24;

/// Liczba form neuronów w pamięci wizualnej (siatka 16×16 bloków obrazu).
const VISUAL_NEURONS: u32 = 16;

/// Ile przebiegów nauki wizualnej wykonać (każdy przybliża „wyobraźnię”
/// do obrazu referencyjnego).
const VISUAL_STEPS: u32 = 48;

/// Ile przebiegów pętli „napisz → skompiluj → zobacz podgląd → popraw”.
const GUI_ITERATIONS: u32 = 8;

/// Próg podobieństwa podglądu do obrazu referencyjnego (znormalizowany MSE).
const GUI_LOSS_THRESHOLD: f64 = 0.05;

/// Ścieżka podglądu okna zapisywanego do pliku (do wglądu człowieka).
fn gui_preview_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tasks/gui_calc_preview.ppm")
}

/// Plik raportu — nadpisać zmienną `NEURO_MATRIX_FILE`.
fn matrix_file() -> PathBuf {
    std::env::var("NEURO_MATRIX_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("matrix.txt"))
}

/// Ścieżka zadania kalkulatora (obok źródeł crate'u — działa z każdego CWD).
fn task_xml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tasks/calc_x86_64.xml")
}

/// Ścieżka obrazu referencyjnego kalkulatora GUI (`calc.png`).
fn calc_png_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tasks/calc.png")
}

/// Ścieżka wyjściowego kodu Beef.
fn gui_beef_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tasks/gui_calc.beef")
}

/// Wymagane konstrukcje kodu Beef z `tasks/gui_calc_beef.xml`.
fn beef_requirements() -> Vec<self_healing_compiler::Requirement> {
    vec![
        self_healing_compiler::Requirement {
            kind: "class",
            name: "CalculatorWindow".into(),
        },
        self_healing_compiler::Requirement {
            kind: "class",
            name: "Button".into(),
        },
        self_healing_compiler::Requirement {
            kind: "fn",
            name: "OnButtonClick".into(),
        },
        self_healing_compiler::Requirement {
            kind: "fn",
            name: "RenderUI".into(),
        },
    ]
}

/// Siatka przycisków kalkulatora: (etykieta, x, y) — 4 kolumny × 5 wierszy.
type ButtonLayout = [(&'static str, i32, i32); 20];

/// Początkowy układ przycisków (siatka 4×5 dopasowana do `calc.png` 640×490).
fn calculator_layout() -> ButtonLayout {
    [
        ("C", 20, 100),  ("(", 175, 100), (")", 330, 100), ("/", 485, 100),
        ("7", 20, 175),  ("8", 175, 175), ("9", 330, 175), ("*", 485, 175),
        ("4", 20, 250),  ("5", 175, 250), ("6", 330, 250), ("-", 485, 250),
        ("1", 20, 325),  ("2", 175, 325), ("3", 330, 325), ("+", 485, 325),
        ("0", 20, 400),  (".", 175, 400), ("=", 330, 400), ("%", 485, 400),
    ]
}

/// Buduje kod Beef kalkulatora GUI z układu przycisków.
///
/// Przyciski deklarujemy przez `AddButton("etykieta", x, y, w, h)` — to
/// jednocześnie specyfikacja podglądu okna (renderer parsuje te wywołania).
fn build_beef_code(layout: &ButtonLayout) -> String {
    let mut code = String::from(
        r#"using System;
using BeefGE;

class Button {
    string Label;
    int X;
    int Y;
    int Width;
    int Height;
}

class CalculatorWindow : Window {
    Display mDisplay;

    void OnButtonClick(string label) {
        mDisplay.Value = label;
    }

    void RenderUI() {
        AddChild(mDisplay);
    }

    void Setup() {
        this.Title = "Kalkulator";
        this.Size = 640, 490;
"#,
    );
    for (label, x, y) in layout {
        code.push_str(&format!("        AddButton(\"{label}\", {x}, {y}, 135, 60);\n"));
    }
    code.push_str(
        r#"    }

    public override void Update() {
        base.Update();
    }
}
"#,
    );
    code
}

/// Średni błąd kwadratowy między „wyobraźnią” a obrazem referencyjnym.
fn mean_squared_error(output: &[[f32; 4]], image: &PngImage) -> f64 {
    let cells = output.len().min(image.pixels.len() / 4);
    if cells == 0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    for (index, cell) in output.iter().take(cells).enumerate() {
        let src = index * 4;
        for channel in 0..3 {
            let target = image.pixels[src + channel] as f64 / 255.0;
            let predicted = cell[channel] as f64;
            sum += (target - predicted).powi(2);
        }
    }
    sum / (cells as f64 * 3.0)
}

/// Pełny przepływ zadania Beef: `calc.png` → pamięć wizualna (GPU) →
/// dekoder instrukcji → samonaprawa → kod Beef w pliku.
fn run_beef_gui_task(
    report: &mut Report,
) -> Result<(), Box<dyn std::error::Error>> {
    report.line(String::from(
        "--- zadanie: kalkulator GUI w Beef (tasks/gui_calc_beef.xml + calc.png) ---",
    ));

    // 1. Obraz referencyjny.
    let image = neuro_lib::png::load_png(calc_png_path())
        .map_err(|error| format!("nie udało się wczytać calc.png: {error}"))?;
    report.line(format!(
        "obraz referencyjny: {}×{} ({} bajtów RGBA)",
        image.width,
        image.height,
        image.pixels.len()
    ));

    // 2. Pamięć wizualna na GPU: model uczy się odwzorować obraz.
    let visual = VisualMemoryManager::new(
        VISUAL_MEMORY_SHADER,
        image.width,
        image.height,
        VISUAL_NEURONS,
    )?;
    report.line(format!(
        "adapter: {} ({})",
        visual.info().name, visual.info().backend
    ));

    visual.write_image(&image)?;
    visual.prepare_canvas()?;
    let forms: Vec<NeuronForm> = (0..VISUAL_NEURONS)
        .map(|index| NeuronForm::new(0.2 + index as f32 * 0.05, 0.3))
        .collect();
    visual.write_forms(&forms)?;
    visual.run_steps(VISUAL_STEPS)?;
    visual.poll()?;

    let output = visual.read_visual_output()?;
    let learned_mse = mean_squared_error(&output, &image);
    report.line(format!(
        "uczenie wizualne: {VISUAL_STEPS} kroków, MSE={learned_mse:.6}"
    ));

    // 3. Pętla: napisz → skompiluj → zobacz podgląd okna → porównaj → popraw.
    let requirements = beef_requirements();
    let mut layout = calculator_layout();
    let mut best_mse = f64::MAX;
    let mut best_code = build_beef_code(&layout);

    for iteration in 0..GUI_ITERATIONS {
        report.line(format!("--- przebieg {iteration} ---"));

        // 3a. Strukturalna samonaprawa (HealingCompiler → GPU przy błędach).
        let code = build_beef_code(&layout);
        let healed = self_healing_compiler::HealingCompiler::compile_and_fix(
            report,
            &code,
            &requirements,
            2,
        );
        let code = healed.code;
        report.line(format!(
            "  samonaprawa strukturalna: poprawna={} ({} przebiegów, {} błędów)",
            healed.valid,
            healed.attempts,
            healed.errors.len()
        ));

        // 3b. Kompilacja — prawdziwa próba, z zastępczą walidacją strukturalną.
        let compile = beef_compiler::compile(&code, &gui_beef_path(), &requirements);
        report.line(format!(
            "  kompilacja: {} [{}]",
            if compile.ok { "OK" } else { "BŁĄD" },
            compile.tool
        ));
        if !compile.real_compiler {
            report.line(String::from(
                "  (brak kompilatora Beef w systemie — walidacja strukturalna)",
            ));
        }
        for error in &compile.errors {
            report.line(format!("    błąd: {error}"));
        }
        if !compile.output.trim().is_empty() {
            report.line(format!("    wyjście: {}", compile.output.trim()));
        }

        // 3c. Podgląd okna + porównanie z obrazem referencyjnym.
        let preview = window_preview::render(&code, image.width, image.height);
        let mse = compare_preview(&preview, &image);
        report.line(format!(
            "  podgląd okna: MSE={mse:.6} (cel < {GUI_LOSS_THRESHOLD})"
        ));

        if mse < best_mse {
            best_mse = mse;
            best_code = code.clone();
        }

        // 3d. Warunek stopu.
        if compile.ok && mse <= GUI_LOSS_THRESHOLD {
            report.line(String::from(
                "  cel osiągnięty: kod poprawny i podgląd wystarczająco podobny",
            ));
            break;
        }

        // 3e. Korekta wizualna: przesuwamy przyciski i piszemy kod od nowa.
        shift_layout(&mut layout, iteration);
        report.line(format!("  zmiana układu przycisków (przebieg {iteration})"));
    }

    // 4. Zapis najlepszego kodu i podglądu okna.
    std::fs::write(gui_beef_path(), &best_code)
        .map_err(|error| format!("nie udało się zapisać kodu Beef: {error}"))?;
    save_preview(&best_code, image.width, image.height);
    report.line(format!("najlepszy MSE podglądu: {best_mse:.6}"));
    report.line(format!(
        "kod Beef zapisany w {}",
        gui_beef_path().display()
    ));
    report.line(format!(
        "podgląd okna zapisany w {}",
        gui_preview_path().display()
    ));
    for line in best_code.lines().take(12) {
        report.line(format!("  {line}"));
    }

    Ok(())
}

/// Porównuje podgląd okna z obrazem referencyjnym (znormalizowany MSE, 0..=1).
fn compare_preview(preview: &window_preview::Preview, image: &PngImage) -> f64 {
    let n = preview.pixels.len().min(image.pixels.len());
    let mut sum = 0.0f64;
    let mut count = 0u64;
    let mut index = 0;
    while index + 3 < n {
        for channel in 0..3 {
            let a = preview.pixels[index + channel] as f64;
            let b = image.pixels[index + channel] as f64;
            sum += (a - b).powi(2);
            count += 1;
        }
        index += 4;
    }
    if count == 0 {
        0.0
    } else {
        sum / (count as f64 * 255.0 * 255.0)
    }
}

/// Zapisuje podgląd okna jako PPM (do wglądu człowieka).
fn save_preview(code: &str, width: u32, height: u32) {
    let preview = window_preview::render(code, width, height);
    let mut out = Vec::with_capacity(preview.pixels.len());
    out.extend_from_slice(format!("P6\n{width} {height}\n255\n").as_bytes());
    for chunk in preview.pixels.chunks_exact(4) {
        out.push(chunk[0]);
        out.push(chunk[1]);
        out.push(chunk[2]);
    }
    if let Err(error) = std::fs::write(gui_preview_path(), out) {
        eprintln!("uwaga: nie udało się zapisać podglądu okna: {error}");
    }
}

/// Deterministycznie przesuwa przyciski — sieć „próbuje innego układu”
/// w poszukiwaniu lepszego dopasowania do obrazu referencyjnego.
fn shift_layout(layout: &mut ButtonLayout, iteration: u32) {
    let dx = ((iteration * 7) % 5) as i32 * 6;
    let dy = ((iteration * 3) % 4) as i32 * 8;
    for (_, x, y) in layout.iter_mut() {
        *x = (*x + dx).clamp(20, 480);
        *y = (*y + dy).clamp(100, 400);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut report = Report::new(matrix_file());

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    report.say(format_args!("=== neuro-bin · raport (ts {stamp}) ==="));
    report.line(format!("plik raportu: {}", report.path().display()));

    // ---------- 1. Sieć neuronów ----------
    let topology = Topology::new(&SECTOR_SIZES)?;
    let total = topology.total_neurons();

    let neurons: Vec<Neuron> = (0..total)
        .map(|index| {
            let sector = topology.sector_of(index).expect("indeks z zakresu");
            let local = index - topology.offsets()[sector as usize];
            let target_sector = (sector + 1) % topology.sector_count() as u32;
            let neuron = Neuron::new(sector, target_sector, local);
            if index < PROBES {
                neuron.with_input(1.0, 1.0)
            } else {
                neuron
            }
        })
        .collect();

    let manager = GpuSectorManager::new(NEURON_SHADER, &neurons, &topology, 1.0)?;
    report.line(format!(
        "adapter: {} ({})",
        manager.info().name,
        manager.info().backend
    ));
    report.line(format!(
        "topologia: {} sektorów po {} = {total} neuronów",
        topology.sector_count(),
        SECTOR_SIZES[0]
    ));

    let full_rights = [
        SectorAccessRights::NeuronWorkspace,
        SectorAccessRights::SystemMap,
        SectorAccessRights::Config,
    ];
    let group = manager.create_petal_bind_group(&full_rights);

    for step in 1..=STEPS {
        manager.execute_neuron_pipeline(&group, total);
        let state = manager.read_neurons()?;
        let fire = state
            .iter()
            .filter(|neuron| neuron.decision() == Decision::Fire)
            .count();
        let migrate = state
            .iter()
            .filter(|neuron| neuron.decision() == Decision::Migrate)
            .count();
        report.line(format!(
            "krok {step:>2}: FIRE={fire:>4}, MIGRATE={migrate:>4}, IDLE={:>4}",
            state.len() - fire - migrate
        ));
    }

    let after_full_rights = manager.read_neurons()?;
    let probe = after_full_rights[0];
    report.line(format!(
        "neuron 0: core_state={:.4}, output_signal={:.4}, decyzja={:?}",
        probe.core_state,
        probe.output_signal,
        probe.decision()
    ));

    let restricted = manager.create_petal_bind_group(&[
        SectorAccessRights::SystemMap,
        SectorAccessRights::Config,
    ]);
    manager.execute_neuron_pipeline(&restricted, total);
    let after_restricted = manager.read_neurons()?;
    if after_restricted != after_full_rights {
        return Err("naruszenie uprawnień: Płatek bez NeuronWorkspace zapisał do sektora".into());
    }
    report.line(String::from(
        "uprawnienia: Płatek bez NeuronWorkspace nie zmienił sektora — ok",
    ));

    // ---------- 2. Macierz scenariuszy ----------
    report.line(String::from("--- macierz scenariuszy ---"));

    let scenario =
        ScenarioMatrixManager::new(SCENARIO_SHADER, MATRIX_SIZE, MATRIX_SIZE, MATRIX_NEURONS)?;
    let cells = scenario.cells() as usize;
    let per_row = MATRIX_SIZE / 4; // bloki `vec4` w wierszu
    report.line(format!(
        "macierz: {}×{} ({} komórek), {MATRIX_NEURONS} form neuronów",
        scenario.width(),
        scenario.height(),
        scenario.cells()
    ));

    // Stan początkowy: spadek po przekątnej + kanał alfa = 1.
    let mut initial = vec![[0.0f32; 4]; cells];
    for y in 0..MATRIX_SIZE {
        for block in 0..per_row {
            let t = (y + block * 4) as f32 / (2.0 * MATRIX_SIZE as f32);
            initial[(y * per_row + block) as usize] = [t, t * 0.5, 1.0 - t, 1.0];
        }
    }
    scenario.write_matrix(&initial)?;

    // Formy neuronów: niski próg, żeby adaptacja faktycznie się odpalała.
    let forms: Vec<NeuronForm> = (0..MATRIX_NEURONS)
        .map(|index| NeuronForm::new(0.1 + index as f32 * 0.05, 0.35))
        .collect();
    scenario.write_forms(&forms)?;

    // Impuls („szok") w środku macierzy: jeden krok z nim, potem sama dyfuzja.
    let mut stimulus = vec![[0.0f32; 4]; cells];
    let center = (MATRIX_SIZE / 2) * per_row + (MATRIX_SIZE / 2) / 4;
    stimulus[center as usize] = [12.0, 6.0, 3.0, 1.0];
    scenario.write_stimulus(&stimulus)?;
    scenario.set_stimulus_active(true)?;
    scenario.execute()?;
    scenario.set_stimulus_active(false)?;
    scenario.run_steps(MATRIX_STEPS)?;

    // ---------- 3. Odczyt i dane do pliku ----------
    let matrix = scenario.read_matrix()?;
    let mut max_abs = 0.0f32;
    let mut sum_abs = 0.0f32;
    for cell in &matrix {
        for value in cell {
            max_abs = max_abs.max(value.abs());
            sum_abs += value.abs();
        }
    }
    report.line(format!(
        "impuls + {MATRIX_STEPS} kroków: max|wartość|={max_abs:.4}, średnia={:.5}",
        sum_abs / (cells as f32 * 4.0)
    ));

    report.line(format!(
        "--- macierz {}×{} (kanały: r g b a, jeden wiersz na linię) ---",
        MATRIX_SIZE, MATRIX_SIZE
    ));
    for y in 0..MATRIX_SIZE {
        let mut row = String::new();
        for block in 0..per_row {
            let cell = matrix[(y * per_row + block) as usize];
            row.push_str(&format!(
                "{:.3} {:.3} {:.3} {:.3} | ",
                cell[0], cell[1], cell[2], cell[3]
            ));
        }
        report.line(format!("wiersz {y:>2}: {row}"));
    }

    let forms_after = scenario.read_forms()?;
    let adapted = forms_after
        .iter()
        .filter(|form| form.weights.iter().any(|weight| *weight != 0.0))
        .count();
    report.line(format!(
        "--- formy neuronów (zaadaptowane: {adapted}/{MATRIX_NEURONS}) ---"
    ));
    for (index, form) in forms_after.iter().enumerate() {
        report.line(format!(
            "neuron {index:>2}: wagi=[{:.3} {:.3} {:.3} {:.3}], |w|={:.3}, próg={:.2}, stres={:.3}",
            form.weights[0],
            form.weights[1],
            form.weights[2],
            form.weights[3],
            form.weights_len(),
            form.adaptation_threshold,
            form.stress_level
        ));
    }

    // ---------- 4. Zadanie z XML: kalkulator w asemblerze ----------
    report.line(String::from(
        "--- zadanie: kalkulator x86-64 (tasks/calc_x86_64.xml) ---",
    ));
    let outcome = task_runner::run_task(&mut report, &task_xml_path())?;
    report.line(format!(
        "program: {} linii asm w {}",
        outcome.line_count(),
        outcome.asm_path.display()
    ));
    for line in outcome.program.lines().take(12) {
        report.line(format!("  {line}"));
    }
    report.line(String::from("  ... (pełny program w pliku .asm)"));

    // ---------- 5. Zadanie Beef: kalkulator GUI odwzorowujący calc.png ----------
    run_beef_gui_task(&mut report)?;

    // ---------- 6. Uczenie schematyczne: schematyki maszynowe z dysku ----------
    report.line(String::from(
        "--- uczenie schematyczne: schematyki maszynowe z dysku ---",
    ));

    // 6a. Zbuduj mapę schematyków (parsowanie źródeł → .sch na dysku).
    let project_root = env!("CARGO_MANIFEST_DIR");
    let summaries = schema_engine::build_schema_map(project_root);
    for summary in &summaries {
        report.line(format!(
            "[schematyk] {} → {} węzłów AST, {} słów kluczowych, {} B (zapisano {})",
            summary.lang,
            summary.node_count,
            summary.keyword_count,
            summary.total_size,
            summary.output_path
        ));
    }
    if summaries.is_empty() {
        report.line(String::from("[schematyk] brak źródeł do nauki — pomijam."));
    }

    // 6b. Ucz sieć schematyków z dysku.
    let schemas_dir = PathBuf::from(project_root).join("schemas");
    match schema_learning::learn_all(&schemas_dir) {
        Ok(signatures) => {
            for signature in &signatures {
                let dominant: Vec<String> = signature
                    .dominant_effects()
                    .iter()
                    .map(|(idx, share)| {
                        format!("{}({share:.2})", schema_learning::EFFECT_NAMES[*idx])
                    })
                    .collect();
                let node_types: Vec<String> = signature
                    .dominant_node_types()
                    .iter()
                    .map(|(idx, share)| {
                        format!("{}({share:.2})", schema_learning::NODE_TYPE_NAMES[*idx])
                    })
                    .collect();
                report.line(format!(
                    "[uczenie] {} (lang=0x{:04X}, {} węzłów): efekty → {}",
                    signature.name,
                    signature.lang_id,
                    signature.node_count,
                    dominant.join(", ")
                ));
                report.line(format!(
                    "[uczenie] {} → typy węzłów: {}",
                    signature.name,
                    node_types.join(", ")
                ));
            }

            // Zapisz wyuczone sygnatury (trwała pamięć schematyczna).
            let signatures_path = schemas_dir.join("learned_signatures.bin");
            match schema_learning::save_signatures(&signatures_path, &signatures) {
                Ok(()) => report.line(format!(
                    "[uczenie] zapisano {} sygnatur w {}",
                    signatures.len(),
                    signatures_path.display()
                )),
                Err(error) => report.line(format!("[uczenie] błąd zapisu sygnatur: {error}")),
            }

            // Odczytaj sygnatury z powrotem z dysku — trwałość nauki.
            match schema_learning::load_signatures(&signatures_path) {
                Ok(reloaded) => report.line(format!(
                    "[uczenie] odczytano z dysku {} sygnatur (round-trip)",
                    reloaded.len()
                )),
                Err(error) => report.line(format!("[uczenie] błąd odczytu sygnatur: {error}")),
            }

            // 6c. Wykonaj inne zadania używając zapisanych schematyków.
            let tasks = [
                schema_learning::TaskSpec {
                    name: "kalkulator".into(),
                    effects: [
                        0.0, 0.1, 0.1, 0.6, 0.6, 1.0, 0.2, 0.0, 0.0, 0.0, 0.0,
                    ],
                },
                schema_learning::TaskSpec {
                    name: "serwer-plików".into(),
                    effects: [
                        0.0, 0.3, 0.3, 0.3, 0.4, 0.1, 0.1, 1.0, 0.7, 0.1, 0.0,
                    ],
                },
            ];
            for task in &tasks {
                match schema_learning::execute_task(task, &signatures) {
                    Some(plan) => {
                        let effects: Vec<String> = plan
                            .effects
                            .iter()
                            .map(|(idx, share)| {
                                format!("{}({share:.2})", schema_learning::EFFECT_NAMES[*idx])
                            })
                            .collect();
                        report.line(format!(
                            "[zadanie] {} → schematyk `{}` (dopasowanie {:.3}); rozumie: {}",
                            task.name,
                            plan.schema_name,
                            plan.match_score,
                            effects.join(", ")
                        ));
                    }
                    None => report.line(format!(
                        "[zadanie] {} → brak dopasowania w zapisanych schematykach",
                        task.name
                    )),
                }
            }
        }
        Err(error) => report.line(format!("[uczenie] błąd: {error}")),
    }

    report.line(format!("raport zapisany w {}", report.path().display()));
    Ok(())
}

