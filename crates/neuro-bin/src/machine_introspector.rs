// crates/neuro-bin/src/machine_introspector.rs
//!
//! Introspekcja maszynowa — analiza tego, czego brakuje w architekturze,
//! oraz cykliczny katalog możliwości do zdobycia (niezależnie od zadania).

/// Pojedyncza potrzeba maszynowa (luka do wypełnienia).
#[derive(Debug, Clone)]
pub struct MachineNeed {
    pub description: String,
    pub target_module: String,
    /// Efekty maszynowe, które moduł musi realizować (np. "SIMD_OP", "HEAP_ALLOC").
    pub required_machine_effects: Vec<String>,
    /// Sugerowany język (`.rs`, `.cpp`, `.c`, `.zig`, `.odin`).
    pub language: String,
}

/// Pamięć wzorców korzystnych / niekorzystnych + cykliczny indeks potrzeb.
pub struct MachineIntrospector {
    pub bad_patterns: Vec<String>,
    pub good_patterns: Vec<String>,
    index: usize,
}

impl MachineIntrospector {
    pub fn new() -> Self {
        Self {
            bad_patterns: Vec::new(),
            good_patterns: Vec::new(),
            index: 0,
        }
    }

    /// Katalog możliwości, które Aurole chce zdobywać — rozszerza własne
    /// możliwości o nowe moduły w wielu językach.
    fn catalog() -> Vec<MachineNeed> {
        vec![
            MachineNeed {
                description: "Akceleracja SIMD operacji macierzowych".into(),
                target_module: "simd_math".into(),
                required_machine_effects: vec!["SIMD_OP".into(), "REG_WRITE".into()],
                language: "rust".into(),
            },
            MachineNeed {
                description: "Komunikacja sieciowa (HTTP/TCP)".into(),
                target_module: "net_client".into(),
                required_machine_effects: vec!["SYSCALL".into(), "HEAP_ALLOC".into()],
                language: "cpp".into(),
            },
            MachineNeed {
                description: "Ultraszybki parser AST".into(),
                target_module: "fast_parser".into(),
                required_machine_effects: vec!["REG_READ".into(), "BRANCH".into()],
                language: "zig".into(),
            },
            MachineNeed {
                description: "Transformacja bufora bajtów".into(),
                target_module: "byte_transform".into(),
                required_machine_effects: vec!["ALU_OP".into(), "REG_WRITE".into()],
                language: "c".into(),
            },
            MachineNeed {
                description: "Struktury danych i alokacje sterty".into(),
                target_module: "data_structs".into(),
                required_machine_effects: vec!["HEAP_ALLOC".into(), "REG_WRITE".into()],
                language: "odin".into(),
            },
        ]
    }

    /// Zwraca kolejną potrzebę (cyklicznie, po wszystkich językach).
    pub fn next_need(&mut self) -> MachineNeed {
        let catalog = Self::catalog();
        let need = catalog[self.index % catalog.len()].clone();
        self.index += 1;
        need
    }

    /// Rejestruje wzorzec korzystny (moduł, który się skompilował).
    pub fn record_success(&mut self, module: &str) {
        let key = module.to_string();
        if !self.good_patterns.contains(&key) {
            self.good_patterns.push(key);
        }
    }

    /// Rejestruje wzorzec niekorzystny (błąd, który wystąpił).
    pub fn record_failure(&mut self, pattern: &str) {
        let key = pattern.to_string();
        if !self.bad_patterns.contains(&key) {
            self.bad_patterns.push(key);
        }
    }
}
