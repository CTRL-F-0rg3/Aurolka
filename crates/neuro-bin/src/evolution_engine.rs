// crates/neuro-bin/src/evolution_engine.rs
//!
//! Autonomiczny silnik samorozwoju — generuje kod wielojęzykowy (.rs .cpp .c
//! .zig .odin), kompiluje, uczy się na każdej zmianie (co działa / co nie) i
//! zapisuje rozwijaną schematykę. Pętla trwa do naciśnięcia ESC.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::compiler_learner::{CompileOutcome, CompilerLearner};
use crate::esc_watcher::EscWatcher;
use crate::machine_introspector::MachineIntrospector;
use crate::report::Report;
use crate::schema_generator::SchemaGenerator;

// ============================================================
// KONFIGURACJA
// ============================================================

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub core_identity: String,
    pub self_evolution_loop: bool,
    pub evolution_parameters: EvolutionParams,
    #[serde(default)]
    pub polyglot_capabilities: HashMap<String, PolyglotCapability>,
    pub tools_directory: String,
    pub manifests_directory: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvolutionParams {
    pub max_healing_attempts: u32,
    #[serde(default)]
    pub sandbox_enabled: bool,
    pub auto_assimilation: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolyglotCapability {
    pub role: String,
    pub compiler: String,
}

// ============================================================
// STAN NAUKI (trwały, rozwijana schematyka)
// ============================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LanguageScore {
    pub attempts: u32,
    pub successes: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearningState {
    pub generations: u32,
    pub languages: HashMap<String, LanguageScore>,
    pub good_patterns: Vec<String>,
    pub bad_patterns: Vec<String>,
    pub modules: Vec<String>,
}

// ============================================================
// SILNIK
// ============================================================

/// Języki rozwijane samodzielnie.
const LANGUAGES: [&str; 5] = ["rust", "cpp", "c", "zig", "odin"];

pub struct EvolutionEngine {
    config: Config,
    project_root: PathBuf,
    state: LearningState,
    introspector: MachineIntrospector,
    generator: SchemaGenerator,
    learner: CompilerLearner,
}

impl EvolutionEngine {
    pub fn new(project_root: &str) -> Result<Self, String> {
        let config_path = Path::new(project_root).join("config.json");
        let config_str = fs::read_to_string(&config_path)
            .map_err(|error| format!("brak config.json ({}): {error}", config_path.display()))?;
        let config: Config = serde_json::from_str(&config_str)
            .map_err(|error| format!("błąd parsowania config.json: {error}"))?;

        let root = PathBuf::from(project_root);
        let state = Self::load_state(&Self::state_path(&root));

        Ok(Self {
            config,
            project_root: root,
            state,
            introspector: MachineIntrospector::new(),
            generator: SchemaGenerator::new(),
            learner: CompilerLearner,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    fn sandbox_dir(&self) -> PathBuf {
        self.project_root.join("evolution_sandbox")
    }

    fn state_path(root: &Path) -> PathBuf {
        root.join("evolution_sandbox").join("learning_state.json")
    }

    fn load_state(path: &Path) -> LearningState {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// GŁÓWNA PĘTLA AUTONOMICZNA — do naciśnięcia ESC.
    pub fn run_autonomous(&mut self, report: &mut Report) {
        if !self.config.self_evolution_loop {
            report.line("[AUTO] Samorozwój wyłączony (self_evolution_loop=false).");
            return;
        }

        let esc = EscWatcher::start();
        report.line("[AUTO] Autonomiczny samorozwój — naciśnij ESC, aby zatrzymać.");
        report.line(format!(
            "[AUTO] Start: {} pokoleń, dobre wzorce: {}, złe: {}",
            self.state.generations,
            self.state.good_patterns.len(),
            self.state.bad_patterns.len()
        ));

        // Limit pokoleń (opcjonalny, np. do testów/CI). Domyślnie: do ESC.
        let max_generations: Option<u64> = std::env::var("AUTO_MAX_GENERATIONS")
            .ok()
            .and_then(|value| value.parse().ok());

        let mut iteration = 0u64;
        while !esc.is_stopped() {
            if let Some(max) = max_generations {
                if iteration >= max {
                    report.line(format!("[AUTO] Osiągnięto limit {max} pokoleń."));
                    break;
                }
            }
            iteration += 1;

            let lang = LANGUAGES[(iteration as usize) % LANGUAGES.len()];
            let need = self.introspector.next_need();
            let code = self.generator.generate(&need, lang, &self.state.bad_patterns);

            let module_name = format!("{}_{}", need.target_module, lang);
            let outcome = self.learner.compile(lang, &module_name, &code, &self.sandbox_dir());

            self.learn(lang, &module_name, &outcome, report);

            if iteration % 10 == 0 {
                self.save_state(report);
            }

            thread::sleep(Duration::from_millis(200));
        }

        self.save_state(report);
        report.line(format!(
            "[AUTO] Zatrzymano (ESC) po {} pokoleniach.",
            self.state.generations
        ));
    }

    /// Uczy się na podstawie wyniku kompilacji (co działa / co nie).
    fn learn(
        &mut self,
        lang: &str,
        module_name: &str,
        outcome: &CompileOutcome,
        report: &mut Report,
    ) {
        {
            let score = self.state.languages.entry(lang.to_string()).or_default();
            score.attempts += 1;
            if matches!(outcome, CompileOutcome::Success) {
                score.successes += 1;
            }
        }

        let gen = self.state.generations + 1;

        match outcome {
            CompileOutcome::Success => {
                self.introspector.record_success(module_name);
                if !self.state.modules.contains(&module_name.to_string()) {
                    self.state.modules.push(module_name.to_string());
                }
                let succ = self.state.languages.get(lang).map(|s| s.successes).unwrap_or(0);
                report.line(format!(
                    "[AUTO] gen #{gen}: {module_name} ({lang}) → SUKCES ({succ} łącznie)"
                ));
            }
            CompileOutcome::Failure(stderr) => {
                let pattern = classify_error(stderr);
                self.introspector.record_failure(&pattern);
                report.line(format!(
                    "[AUTO] gen #{gen}: {module_name} ({lang}) → BŁĄD: {pattern}"
                ));
            }
            CompileOutcome::Unavailable(reason) => {
                report.line(format!(
                    "[AUTO] gen #{gen}: {module_name} ({lang}) → POMINIĘTO ({reason})"
                ));
            }
        }

        self.state.generations = gen;
        self.state.good_patterns = self.introspector.good_patterns.clone();
        self.state.bad_patterns = self.introspector.bad_patterns.clone();
    }

    /// Zapisuje stan nauki (schematykę) na dysk.
    fn save_state(&self, report: &mut Report) {
        let dir = self.sandbox_dir();
        fs::create_dir_all(&dir).ok();

        let path = Self::state_path(&self.project_root);
        match serde_json::to_string_pretty(&self.state) {
            Ok(text) => match fs::write(&path, text) {
                Ok(()) => report.line(format!("[AUTO] Zapisano schematykę: {}", path.display())),
                Err(error) => report.line(format!("[AUTO] Błąd zapisu: {error}")),
            },
            Err(error) => report.line(format!("[AUTO] Błąd serializacji: {error}")),
        }
    }
}

/// Klasyfikuje błąd kompilacji na zwięzły wzorzec niekorzystny.
fn classify_error(stderr: &str) -> String {
    if stderr.contains("mismatched types") {
        "mismatched types: wymagane jawne rzutowanie".into()
    } else if stderr.contains("borrow") {
        "borrow checker: użyj .clone() lub zmień czas życia".into()
    } else if stderr.contains("cannot find")
        || stderr.contains("undeclared")
        || stderr.contains("not found")
    {
        "brakująca definicja/import".into()
    } else if stderr.contains("expected") || stderr.contains("error:") {
        "błąd składni".into()
    } else {
        format!("inny: {}", stderr.lines().next().unwrap_or("?").trim())
    }
}
