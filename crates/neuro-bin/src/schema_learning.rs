//! Uczenie schematyczne dla schematyków maszynowych.
//!
//! Sieć **uczy się** binarnych schematyków `.sch` zapisanych na dysku
//! (produkt `schema_engine::build_schema_map`), sprowadzając każdy z nich
//! do zwartego wektora cech — *sygnatury schematyku*. Sygnatury są trwałe:
//! zapisujemy je na dysk, by móc ich użyć do **wykonania innego zadania**.
//!
//! Wykonanie nowego zadania to dopasowanie profilu efektów maszynowych
//! zadania do wyuczonych sygnatur (podobieństwo kosinusowe), a następnie
//! przełożenie zwycięskiego schematyku na twarde instrukcje asemblera
//! (przez słownik `asm_decoder`). Dzięki temu sieć, która rozumie zapisane
//! schematy, potrafi wykonać zadanie, którego wprost nigdy nie trenowała.

use std::path::{Path, PathBuf};

use crate::schema_engine::{read_schema, Schema};

// ============================================================
// STAŁE STRUKTURY CECH
// ============================================================

/// Liczba efektów maszynowych (`MachineEffect`: NONE..=SIMD_OP).
pub const EFFECT_COUNT: usize = 11;

/// Liczba typów węzłów AST (`AstNodeType`: KEYWORD..=ATTRIBUTE).
pub const NODE_TYPE_COUNT: usize = 10;

/// Długość wektora cech wyuczonej sygnatury.
///
/// `[0..11]` histogram efektów maszynowych, `[11..21]` histogram typów
/// węzłów, `[21]` średni koszt maszynowy, `[22]` średnia złożoność,
/// `[23]` bogactwo słownika (słowa kluczowe / węzły).
pub const FEATURE_SIZE: usize = 24;

/// Czytelne nazwy efektów maszynowych (indeks = kod efektu).
pub const EFFECT_NAMES: [&str; EFFECT_COUNT] = [
    "NONE", "STACK_PUSH", "STACK_POP", "REG_WRITE", "REG_READ",
    "ALU_OP", "BRANCH", "SYSCALL", "HEAP_ALLOC", "HEAP_FREE", "SIMD_OP",
];

/// Czytelne nazwy typów węzłów AST (indeks = kod - 1).
pub const NODE_TYPE_NAMES: [&str; NODE_TYPE_COUNT] = [
    "KEYWORD", "TYPE_DEF", "FUNCTION", "EXPRESSION", "STATEMENT",
    "LITERAL", "OPERATOR", "BLOCK", "IMPORT", "ATTRIBUTE",
];

// ============================================================
// SYGNATURA SCHEMATYKU (wyuczona reprezentacja)
// ============================================================

/// Wyuczona, trwała reprezentacja pojedynczego schematyku.
#[derive(Debug, Clone)]
pub struct SchemaSignature {
    /// Nazwa schematyku (np. `rust`, `cpp`, `beef`).
    pub name: String,
    /// Kod języka (`LangId`, np. 0x5253 dla Rust).
    pub lang_id: u16,
    /// Liczba węzłów AST użyta do nauki.
    pub node_count: u32,
    /// Liczba słów kluczowych użyta do nauki.
    pub keyword_count: u32,
    /// Wektor cech (znormalizowany, długość `FEATURE_SIZE`).
    pub features: [f32; FEATURE_SIZE],
}

impl SchemaSignature {
    /// Profil efektów maszynowych (pierwsze `EFFECT_COUNT` cech).
    pub fn effect_profile(&self) -> &[f32] {
        &self.features[..EFFECT_COUNT]
    }

    /// Profil typów węzłów AST (cechy `EFFECT_COUNT..EFFECT_COUNT + NODE_TYPE_COUNT`).
    pub fn node_type_profile(&self) -> &[f32] {
        &self.features[EFFECT_COUNT..EFFECT_COUNT + NODE_TYPE_COUNT]
    }

    /// Najczęstsze typy węzłów (indeks, udział) w kolejności malejącej.
    pub fn dominant_node_types(&self) -> Vec<(usize, f32)> {
        let mut ranked: Vec<(usize, f32)> =
            self.node_type_profile().iter().copied().enumerate().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
            .into_iter()
            .filter(|(_, value)| *value > 0.0)
            .take(4)
            .collect()
    }

    /// Krótki opis najsilniejszych efektów maszynowych schematyku.
    pub fn dominant_effects(&self) -> Vec<(usize, f32)> {
        let mut ranked: Vec<(usize, f32)> =
            self.effect_profile().iter().copied().enumerate().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
            .into_iter()
            .filter(|(_, value)| *value > 0.0)
            .take(4)
            .collect()
    }
}

/// Uczy sieć jednego schematyku: sprowadza go do wektora cech.
pub fn learn_schema(schema: &Schema) -> SchemaSignature {
    let node_count = schema.nodes.len().max(1);

    let mut effects = [0.0f32; EFFECT_COUNT];
    let mut node_types = [0.0f32; NODE_TYPE_COUNT];
    let mut cost_sum = 0.0f32;
    let mut complexity_sum = 0.0f32;

    for node in &schema.nodes {
        let effect = (node.machine_effect as usize).min(EFFECT_COUNT - 1);
        effects[effect] += 1.0;

        if node.node_type > 0 {
            let type_idx = (node.node_type as usize - 1).min(NODE_TYPE_COUNT - 1);
            node_types[type_idx] += 1.0;
        }

        cost_sum += node.machine_cost;
        complexity_sum += node.complexity;
    }

    // Normalizacja histogramów.
    let count = node_count as f32;
    for value in effects.iter_mut().chain(node_types.iter_mut()) {
        *value /= count;
    }

    let mut features = [0.0f32; FEATURE_SIZE];
    features[..EFFECT_COUNT].copy_from_slice(&effects);
    features[EFFECT_COUNT..EFFECT_COUNT + NODE_TYPE_COUNT].copy_from_slice(&node_types);
    features[EFFECT_COUNT + NODE_TYPE_COUNT] = cost_sum / count;
    features[EFFECT_COUNT + NODE_TYPE_COUNT + 1] = complexity_sum / count;
    features[EFFECT_COUNT + NODE_TYPE_COUNT + 2] = schema.keywords.len() as f32 / count;

    SchemaSignature {
        name: schema_name_from_header(schema),
        lang_id: schema.header.lang_id,
        node_count: schema.header.node_count,
        keyword_count: schema.header.keyword_count,
        features,
    }
}

/// Nazwa schematyku wyprowadzona z kodu języka w nagłówku.
fn schema_name_from_header(schema: &Schema) -> String {
    match schema.header.lang_id {
        0x5253 => "rust".to_string(),
        0x4246 => "beef".to_string(),
        0x4350 => "cpp".to_string(),
        0x4300 => "c".to_string(),
        0x4158 => "asm_x86".to_string(),
        0x4152 => "asm_rv".to_string(),
        other => format!("lang_{other:04X}"),
    }
}

// ============================================================
// UCZENIE CAŁEJ BIBLIOTEKI SCHEMATYKÓW Z DYSKU
// ============================================================

/// Uczy sieć wszystkich schematyków `.sch` w katalogu i zwraca sygnatury.
pub fn learn_all(schemas_dir: &Path) -> Result<Vec<SchemaSignature>, String> {
    let mut signatures = Vec::new();

    let entries = std::fs::read_dir(schemas_dir)
        .map_err(|error| format!("brak katalogu schematyków {}: {error}", schemas_dir.display()))?;

    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "sch"))
        .collect();
    paths.sort();

    for path in paths {
        let schema = read_schema(&path)?;
        let signature = learn_schema(&schema);
        signatures.push(signature);
    }

    Ok(signatures)
}

// ============================================================
// TRWAŁE SYGNATURY (zapis / odczyt z dysku)
// ============================================================

/// Binarne zapisanie wyuczonych sygnatur (trwała pamięć schematyczna).
pub fn save_signatures(path: &Path, signatures: &[SchemaSignature]) -> Result<(), String> {
    let mut out = Vec::new();

    // Nagłówek: magic + liczba sygnatur.
    out.extend_from_slice(&0x5349474Eu32.to_le_bytes()); // "SIGN"
    out.extend_from_slice(&(signatures.len() as u32).to_le_bytes());

    for signature in signatures {
        // Nazwa: u16 długość + bajty UTF-8.
        let name = signature.name.as_bytes();
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);

        out.extend_from_slice(&signature.lang_id.to_le_bytes());
        out.extend_from_slice(&signature.node_count.to_le_bytes());
        out.extend_from_slice(&signature.keyword_count.to_le_bytes());
        for feature in signature.features {
            out.extend_from_slice(&feature.to_le_bytes());
        }
    }

    std::fs::write(path, out)
        .map_err(|error| format!("nie udało się zapisać {}: {error}", path.display()))
}

/// Odczytuje wyuczone sygnatury z dysku.
pub fn load_signatures(path: &Path) -> Result<Vec<SchemaSignature>, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("nie udało się wczytać {}: {error}", path.display()))?;

    if bytes.len() < 8 {
        return Err("plik sygnatur za krótki".into());
    }

    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != 0x5349474E {
        return Err("zły magic pliku sygnatur".into());
    }
    let count = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;

    let mut cursor = 8usize;
    let mut signatures = Vec::with_capacity(count);

    for _ in 0..count {
        if cursor + 2 > bytes.len() {
            return Err("plik sygnatur obcięty (nazwa)".into());
        }
        let name_len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        cursor += 2;
        if cursor + name_len > bytes.len() {
            return Err("plik sygnatur obcięty (treść nazwy)".into());
        }
        let name = String::from_utf8_lossy(&bytes[cursor..cursor + name_len]).into_owned();
        cursor += name_len;

        if cursor + 10 > bytes.len() {
            return Err("plik sygnatur obcięty (metadane)".into());
        }
        let lang_id = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]);
        cursor += 2;
        let node_count = u32::from_le_bytes([
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
        ]);
        cursor += 4;
        let keyword_count = u32::from_le_bytes([
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
        ]);
        cursor += 4;

        let mut features = [0.0f32; FEATURE_SIZE];
        for feature in features.iter_mut() {
            if cursor + 4 > bytes.len() {
                return Err("plik sygnatur obcięty (cechy)".into());
            }
            *feature = f32::from_le_bytes([
                bytes[cursor],
                bytes[cursor + 1],
                bytes[cursor + 2],
                bytes[cursor + 3],
            ]);
            cursor += 4;
        }

        signatures.push(SchemaSignature {
            name,
            lang_id,
            node_count,
            keyword_count,
            features,
        });
    }

    Ok(signatures)
}

// ============================================================
// WYKONYWANIE INNEGO ZADANIA PRZY UŻYCIU ZAPISANYCH SCHEMATYKÓW
// ============================================================

/// Opis nowego zadania przez profil efektów maszynowych.
///
/// Zadanie nie musi być wprost wytrenowane — wystarczy opisać, czego
/// potrzebuje na poziomie maszyny (jakie efekty dominują), a sieć dopasuje
/// do niego najlepiej rozumiany schematyk.
#[derive(Debug, Clone)]
pub struct TaskSpec {
    /// Czytelna nazwa zadania (np. `kalkulator`).
    pub name: String,
    /// Profil efektów maszynowych (długość `EFFECT_COUNT`).
    pub effects: [f32; EFFECT_COUNT],
}

impl TaskSpec {
    /// Normalizuje profil efektów do wektora jednostkowego.
    fn normalized(&self) -> [f32; EFFECT_COUNT] {
        let norm = self.effects.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm <= f32::EPSILON {
            return self.effects;
        }
        let mut out = self.effects;
        for value in out.iter_mut() {
            *value /= norm;
        }
        out
    }
}

/// Dopasowuje zadanie do najlepiej rozumianego schematyku (podobieństwo
/// kosinusowe profili efektów). Zwraca sygnaturę i siłę dopasowania 0..=1.
pub fn match_task<'a>(
    task: &TaskSpec,
    signatures: &'a [SchemaSignature],
) -> Option<(&'a SchemaSignature, f32)> {
    let task_norm = task.normalized();

    signatures
        .iter()
        .filter_map(|signature| {
            let profile = signature.effect_profile();
            let profile_norm =
                profile.iter().map(|value| value * value).sum::<f32>().sqrt();
            if profile_norm <= f32::EPSILON {
                return None;
            }
            let dot: f32 = task_norm
                .iter()
                .zip(profile.iter())
                .map(|(a, b)| a * b)
                .sum();
            let similarity = (dot / profile_norm).clamp(0.0, 1.0);
            Some((signature, similarity))
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

/// Wynik wykonania zadania: który schematyk zrozumiał zadanie.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    /// Nazwa użytego schematyku.
    pub schema_name: String,
    /// Siła dopasowania zadania do schematyku (0..=1).
    pub match_score: f32,
    /// Dominujące efekty maszynowe schematyku (indeks, udział).
    pub effects: Vec<(usize, f32)>,
}

/// Wykonuje zadanie: dopasowuje schematyk i zwraca plan realizacji.
pub fn execute_task(
    task: &TaskSpec,
    signatures: &[SchemaSignature],
) -> Option<ExecutionPlan> {
    let (signature, match_score) = match_task(task, signatures)?;
    Some(ExecutionPlan {
        schema_name: signature.name.clone(),
        match_score,
        effects: signature.dominant_effects(),
    })
}

// ============================================================
// TESTY
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_engine::{AstNode, KeywordEntry, SchemaHeader, SCHEMA_MAGIC};

    fn ast_node(node_type: u8, machine_effect: u8) -> AstNode {
        AstNode {
            node_type,
            machine_effect,
            depth: 0,
            line_number: 0,
            col_number: 0,
            token_hash: 0,
            complexity: 0.1,
            machine_cost: 0.2,
            parent_idx: 0,
            child_count: 0,
            reg_mask: 0,
            flag_mask: 0,
            sector_hint: 0,
            padding: 0,
        }
    }

    fn keyword(text: &str) -> KeywordEntry {
        let mut buf = [0u8; 24];
        let bytes = text.as_bytes();
        let len = bytes.len().min(23);
        buf[..len].copy_from_slice(&bytes[..len]);
        KeywordEntry {
            text: buf,
            category: 1,
            machine_effect: 0x01,
            frequency: 1,
            token_hash: 0,
            reserved: 0,
        }
    }

    fn sample_schema() -> Schema {
        Schema {
            header: SchemaHeader {
                magic: SCHEMA_MAGIC,
                lang_id: 0x5253,
                version: 1,
                node_count: 4,
                keyword_count: 2,
                total_size: 0,
                crc32: 0,
                reserved: 0,
            },
            nodes: vec![
                ast_node(0x04, 0x05), // EXPRESSION / ALU_OP
                ast_node(0x04, 0x05),
                ast_node(0x07, 0x05), // OPERATOR / ALU_OP
                ast_node(0x03, 0x01), // FUNCTION / STACK_PUSH
            ],
            keywords: vec![keyword("fn"), keyword("let")],
        }
    }

    #[test]
    fn uczy_schematyk_i_normalizuje_histogram() {
        let sig = learn_schema(&sample_schema());
        assert_eq!(sig.name, "rust");
        assert!((sig.features[5] - 0.75).abs() < 1e-6, "ALU_OP powinno być 0.75");
        assert!((sig.features[1] - 0.25).abs() < 1e-6, "STACK_PUSH powinno być 0.25");
    }

    #[test]
    fn sygnatury_przetrwaja_zapis_i_odczyt() {
        let sig = learn_schema(&sample_schema());
        let path = std::env::temp_dir().join("neuro_schema_sig_test.bin");
        save_signatures(&path, &[sig.clone()]).unwrap();
        let loaded = load_signatures(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "rust");
        assert_eq!(loaded[0].features, sig.features);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn dopasowuje_zadanie_alu_do_schematyku_z_alu() {
        let sig = learn_schema(&sample_schema());
        let task = TaskSpec {
            name: "kalkulator".into(),
            effects: [
                0.0, 0.0, 0.0, 0.0, 0.0, 1.0, // ALU_OP
                0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        let sigs = [sig];
        let (matched, score) = match_task(&task, &sigs).unwrap();
        assert_eq!(matched.name, "rust");
        assert!(score > 0.9);
    }

    #[test]
    fn pusta_biblioteka_nie_dopasowuje_zadania() {
        let task = TaskSpec {
            name: "kalkulator".into(),
            effects: [0.0; EFFECT_COUNT],
        };
        assert!(match_task(&task, &[]).is_none());
    }
}
