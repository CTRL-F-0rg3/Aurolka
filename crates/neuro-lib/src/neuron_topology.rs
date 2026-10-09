//! Topologia sieci: neuron, konfiguracja uruchomienia i mapa sektorów hex.
//!
//! To warstwa CPU — typy stąd trafiają wprost na GPU przez `bytemuck`.
//! Układ [`Neuron`] musi zgadzać się ze strukturą `Neuron` w pliku
//! `neuro-bin/src/neuron_pipeline.wgsl` (kolejność pól i rozmiar).

use bytemuck::{Pod, Zeroable};

use crate::error::Error;

/// Decyzja neuronu po jednym kroku symulacji.
///
/// Odpowiada kodom `decision_state` w WGSL: `0` — bezczynny, `1` — wystrzał,
/// `2` — migracja do innego sektora.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Decision {
    /// Neuron czeka — nic się nie dzieje.
    Idle = 0,
    /// Neuron wystrzeliwuje (`FIRE`).
    Fire = 1,
    /// Neuron żąda zmiany sektora (`MIGRATE`).
    Migrate = 2,
}

impl Decision {
    /// Zamienia kod z shadera na decyzję. Nieznany kod traktowany jest jako
    /// [`Decision::Idle`].
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Fire,
            2 => Self::Migrate,
            _ => Self::Idle,
        }
    }

    /// Kod używany w WGSL.
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// Pojedynczy neuron — układ zgodny ze strukturą w `neuron_pipeline.wgsl`.
///
/// Rozmiar to 48 bajtów (12 pól po 4 bajty), czyli wielokrotność 16 bajtów
/// wymagana przez `array<Neuron>` w buforze storage.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Neuron {
    /// Sygnał wejściowy (zerowany po każdym kroku przez shader).
    pub input_signal: f32,
    /// Waga wejścia.
    pub input_weight: f32,
    /// Stan rdzenia (pamięć stanu z aktywacją `tanh`).
    pub core_state: f32,
    /// Projekt decyzyjny wyliczony z rdzenia.
    pub decision_projection: f32,
    /// Sektor docelowy połączenia (numer sektora, nie globalny indeks).
    pub connection_target_sector: u32,
    /// Indeks neuronu w sektorze docelowym.
    pub connection_target_index: u32,
    /// Priorytet decyzyjny — co bardziej liczy się: rdzeń czy informacja
    /// zwrotna od sąsiada (mechanizm anty-halucynacyjny).
    pub decision_prior: f32,
    /// Stan decyzji w kodzie WGSL — patrz [`Decision`].
    pub decision_state: u32,
    /// Intencja działania przekazana dalej (`action_intent` w WGSL).
    pub action_intent: u32,
    /// Sygnał wyjściowy czytany przez sąsiadów.
    pub output_signal: f32,
    /// Sektor hex, w którym neuron się znajduje.
    pub current_hex_sector: u32,
    /// Wyrównanie struktury — musi pozostać zerowe.
    pub _padding: u32,
}

impl Neuron {
    /// Neuron w sektorze `sector`, celujący w `target_sector`/`target_index`.
    ///
    /// ```rust
    /// # use neuro_lib::{Decision, Neuron};
    /// let n = Neuron::new(0, 1, 7);
    /// assert_eq!(n.current_hex_sector, 0);
    /// assert_eq!(n.connection_target_sector, 1);
    /// assert_eq!(n.connection_target_index, 7);
    /// assert_eq!(n.decision(), Decision::Idle);
    /// ```
    pub fn new(sector: u32, target_sector: u32, target_index: u32) -> Self {
        Self {
            input_signal: 0.0,
            input_weight: 1.0,
            core_state: 0.0,
            decision_projection: 0.0,
            connection_target_sector: target_sector,
            connection_target_index: target_index,
            decision_prior: 0.5,
            decision_state: Decision::Idle.code(),
            action_intent: Decision::Idle.code(),
            output_signal: 0.0,
            current_hex_sector: sector,
            _padding: 0,
        }
    }

    /// Podpina sygnał wejściowy — wygodne przy przygotowywaniu danych.
    pub fn with_input(mut self, signal: f32, weight: f32) -> Self {
        self.input_signal = signal;
        self.input_weight = weight;
        self
    }

    /// Aktualna decyzja (odczyt z [`Neuron::decision_state`]).
    pub fn decision(&self) -> Decision {
        Decision::from_code(self.decision_state)
    }
}

impl Default for Neuron {
    fn default() -> Self {
        Self::new(0, 0, 0)
    }
}

/// Konfiguracja uruchomienia — bufor uniform shadera `Config`.
///
/// Rozmiar to 16 bajtów (4 pola po 4 bajty), czyli dokładnie tyle, ile
/// deklaruje `struct Config` w WGSL.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct RunConfig {
    /// Liczba neuronów widocznych dla shadera (strażnik wyjścia poza tablicę).
    pub total_neurons: u32,
    /// Krok czasowy symulacji.
    pub time_step: f32,
    /// Wyrównanie — musi pozostać zerowe.
    pub _pad1: u32,
    /// Wyrównanie — musi pozostać zerowe.
    pub _pad2: u32,
}

impl RunConfig {
    /// Konfiguracja dla zadanej liczby neuronów i kroku czasowego.
    pub fn new(total_neurons: u32, time_step: f32) -> Self {
        Self {
            total_neurons,
            time_step,
            _pad1: 0,
            _pad2: 0,
        }
    }
}

impl Default for RunConfig {
    fn default() -> Self {
        Self::new(0, 1.0)
    }
}

/// Mapa sektorów hex: przesunięcie każdego sektora w tablicy neuronów.
///
/// Shader czyta ją jako `sector_map: array<u32>` — wpis `i` to globalny
/// indeks pierwszego neuronu sektora `i`. Dzięki temu neuron potrafi znaleźć
/// cel połączenia nie znając całej topologii.
///
/// ```rust
/// # use neuro_lib::Topology;
/// let topology = Topology::new(&[10, 20, 30])?;
/// assert_eq!(topology.offsets(), &[0, 10, 30]);
/// assert_eq!(topology.total_neurons(), 60);
/// assert_eq!(topology.sector_of(42), Some(2));
/// # Ok::<(), neuro_lib::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topology {
    offsets: Vec<u32>,
    total_neurons: u32,
}

impl Topology {
    /// Buduje mapę z liczności sektorów (kolejność = numer sektora).
    ///
    /// Sektorów musi być co najmniej jeden, każdy niepusty, a ich suma
    /// nie może przekroczyć `u32`.
    pub fn new(sector_sizes: &[u32]) -> Result<Self, Error> {
        if sector_sizes.is_empty() {
            return Err(Error::Topology(
                "topologia musi mieć co najmniej jeden sektor".into(),
            ));
        }

        let mut offsets = Vec::with_capacity(sector_sizes.len());
        let mut offset = 0u64;
        for (index, size) in sector_sizes.iter().enumerate() {
            if *size == 0 {
                return Err(Error::Topology(format!("sektor {index} jest pusty")));
            }
            offsets.push(u32::try_from(offset).map_err(|_| {
                Error::Topology(format!("liczba neuronów przekracza u32 (sektor {index})"))
            })?);
            offset += u64::from(*size);
        }

        let total_neurons = u32::try_from(offset)
            .map_err(|_| Error::Topology("liczba neuronów przekracza u32".into()))?;

        Ok(Self {
            offsets,
            total_neurons,
        })
    }

    /// Przesunięcia sektorów w tablicy neuronów — dokładna zawartość
    /// bufora `sector_map` na GPU.
    pub fn offsets(&self) -> &[u32] {
        &self.offsets
    }

    /// Liczba sektorów hex.
    pub fn sector_count(&self) -> usize {
        self.offsets.len()
    }

    /// Łączna liczba neuronów we wszystkich sektorach.
    pub fn total_neurons(&self) -> u32 {
        self.total_neurons
    }

    /// Sektor, w którym leży neuron o globalnym indeksie `index`.
    ///
    /// ```rust
    /// # use neuro_lib::Topology;
    /// let topology = Topology::new(&[4, 4])?;
    /// assert_eq!(topology.sector_of(0), Some(0));
    /// assert_eq!(topology.sector_of(5), Some(1));
    /// assert_eq!(topology.sector_of(8), None);
    /// # Ok::<(), neuro_lib::Error>(())
    /// ```
    pub fn sector_of(&self, index: u32) -> Option<u32> {
        if index >= self.total_neurons {
            return None;
        }

        // Sektorów jest kilka — liniowe szukanie wystarcza i pozostaje proste.
        let mut found = 0u32;
        for (position, offset) in self.offsets.iter().enumerate() {
            if index >= *offset {
                found = position as u32;
            }
        }
        Some(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neuron_miesci_sie_w_wymaganiach_wgsl() {
        // array<Neuron> w buforze storage ma krok wielokrotny 16 bajtów.
        assert_eq!(std::mem::size_of::<Neuron>(), 48);
        assert_eq!(std::mem::size_of::<Neuron>() % 16, 0);
        // Uniform Config ma dokładnie 16 bajtów.
        assert_eq!(std::mem::size_of::<RunConfig>(), 16);
        // Brak dziur w strukturach — inaczej `Pod` nie przeszedłby derive.
        assert_eq!(std::mem::size_of::<Neuron>(), 12 * 4);
    }

    #[test]
    fn domyslny_neuron_spoczywa() {
        let neuron = Neuron::new(2, 3, 5);
        assert_eq!(neuron.decision(), Decision::Idle);
        assert_eq!(neuron.decision_state, Decision::Idle.code());
        assert_eq!(neuron.current_hex_sector, 2);
        assert_eq!(neuron.connection_target_sector, 3);
        assert_eq!(neuron.connection_target_index, 5);
        assert_eq!(neuron._padding, 0);
    }

    #[test]
    fn decyzje_maja_kody_zgodne_z_wgsl() {
        assert_eq!(Decision::Idle.code(), 0);
        assert_eq!(Decision::Fire.code(), 1);
        assert_eq!(Decision::Migrate.code(), 2);
        assert_eq!(Decision::from_code(1), Decision::Fire);
        assert_eq!(Decision::from_code(2), Decision::Migrate);
        assert_eq!(Decision::from_code(99), Decision::Idle);
    }

    #[test]
    fn offsety_to_sumy_prefiksowe() {
        let topology = Topology::new(&[10, 20, 30]).unwrap();
        assert_eq!(topology.offsets(), &[0, 10, 30]);
        assert_eq!(topology.sector_count(), 3);
        assert_eq!(topology.total_neurons(), 60);
    }

    #[test]
    fn sector_of_wskazuje_wlasciwy_sektor() {
        let topology = Topology::new(&[4, 4]).unwrap();
        assert_eq!(topology.sector_of(0), Some(0));
        assert_eq!(topology.sector_of(3), Some(0));
        assert_eq!(topology.sector_of(4), Some(1));
        assert_eq!(topology.sector_of(7), Some(1));
        assert_eq!(topology.sector_of(8), None);
    }

    #[test]
    fn topologia_odrzuca_blędne_dane() {
        assert!(Topology::new(&[]).is_err());
        assert!(Topology::new(&[4, 0, 4]).is_err());
    }
}

