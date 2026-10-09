//! Silnik scenariuszy — warstwa zadaniowa nad [`ScenarioMatrixManager`].
//!
//! [`ScenarioEngine`] trzyma własną macierz GPU i udostępnia trzy operacje
//! potrzebne maszynie zadań:
//!
//! * [`ScenarioEngine::inject_stimulus`] — wstrzykuje impuls w komórce `(x, y)`
//!   i od razu go aktywuje,
//! * [`ScenarioEngine::run_simulation_cycle`] — wykonuje cykle symulacji
//!   i dezaktywuje impuls,
//! * [`ScenarioEngine::read_pattern_cell`] — odczytuje wzorzec liczbowy
//!   z komórki `(x, y)` do decyzji dekodera.

use crate::error::{Error, Result};
use crate::scenario_matrix::{NeuronForm, ScenarioMatrixManager};

/// Silnik scenariuszy dla jednego zadania.
pub struct ScenarioEngine {
    manager: ScenarioMatrixManager,
}

impl ScenarioEngine {
    /// Tworzy silnik z macierzą `size × size` i `neurons` formami startowymi.
    ///
    /// Formy startują z niskim progiem adaptacji, żeby sieć reagowała
    /// na impuls od pierwszych cykli.
    pub fn new(shader_source: &str, size: u32, neurons: u32) -> Result<Self> {
        let manager = ScenarioMatrixManager::new(shader_source, size, size, neurons)?;
        let forms: Vec<NeuronForm> = (0..neurons)
            .map(|index| NeuronForm::new(0.1 + index as f32 * 0.05, 0.35))
            .collect();
        manager.write_forms(&forms)?;
        Ok(Self { manager })
    }

    /// Wstrzykuje impuls w komórce `(x, y)` i aktywuje go w konfiguracji.
    pub fn inject_stimulus(&self, x: u32, y: u32, data: [f32; 4]) -> Result<()> {
        self.manager.write_stimulus_cell(x, y, data)?;
        self.manager.set_stimulus_active(true)
    }

    /// Wykonuje `steps` cykli symulacji, potem dezaktywuje impuls.
    pub fn run_simulation_cycle(&self, steps: u32) -> Result<()> {
        self.manager.run_steps(steps)?;
        self.manager.set_stimulus_active(false)
    }

    /// Odczytuje wzorzec liczbowy z komórki `(x, y)` macierzy.
    pub fn read_pattern_cell(&self, x: u32, y: u32) -> Result<[f32; 4]> {
        let width = self.manager.width();
        let height = self.manager.height();
        if y >= height {
            return Err(Error::Matrix(format!(
                "odczyt: y={y} poza macierzą (wysokość {height})"
            )));
        }
        let per_row = width / 4;
        let block_x = (x / 4).min(per_row - 1);
        let matrix = self.manager.read_matrix()?;
        matrix
            .get((y * per_row + block_x) as usize)
            .copied()
            .ok_or_else(|| Error::Matrix("odczyt: indeks komórki poza buforem".into()))
    }

    /// Dostęp do menedżera (wymiary, formy, konfiguracja).
    pub fn manager(&self) -> &ScenarioMatrixManager {
        &self.manager
    }
}
