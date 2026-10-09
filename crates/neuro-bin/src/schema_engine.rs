// crates/neuro-bin/src/schema_engine.rs

use std::ffi::CString;
use std::path::Path;

use bytemuck::{Pod, Zeroable};

// ============================================================
// STRUKTURY BINARNE (muszą być identyczne z C++ — repr(C))
// ============================================================

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SchemaHeader {
    pub magic: u32,
    pub lang_id: u16,
    pub version: u16,
    pub node_count: u32,
    pub keyword_count: u32,
    pub total_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AstNode {
    pub node_type: u8,
    pub machine_effect: u8,
    pub depth: u16,
    pub line_number: u32,
    pub col_number: u32,
    pub token_hash: u32,
    pub complexity: f32,
    pub machine_cost: f32,
    pub parent_idx: u32,
    pub child_count: u32,
    pub reg_mask: u32,
    pub flag_mask: u32,
    pub sector_hint: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct KeywordEntry {
    pub text: [u8; 24],
    pub category: u8,
    pub machine_effect: u8,
    pub frequency: u16,
    pub token_hash: u32,
    pub reserved: u32,
}

// ============================================================
// FFI DO C++
// ============================================================

extern "C" {
    fn schema_parser_init(lang_id: u16) -> *mut std::ffi::c_void;
    fn schema_parser_parse_file(parser: *mut std::ffi::c_void, path: *const std::ffi::c_char) -> u32;
    fn schema_parser_parse_dir(parser: *mut std::ffi::c_void, path: *const std::ffi::c_char) -> u32;
    fn schema_parser_get_header(parser: *mut std::ffi::c_void) -> SchemaHeader;
    fn schema_parser_get_nodes(parser: *mut std::ffi::c_void, count: *mut u32) -> *const AstNode;
    fn schema_parser_get_keywords(parser: *mut std::ffi::c_void, count: *mut u32) -> *const KeywordEntry;
    fn schema_parser_write_schema(parser: *mut std::ffi::c_void, path: *const std::ffi::c_char) -> i32;
    fn schema_parser_free(parser: *mut std::ffi::c_void);
}

// ============================================================
// BEZPIECZNY RUST WRAPPER
// ============================================================

pub struct SchemaEngine {
    parser: *mut std::ffi::c_void,
}

impl SchemaEngine {
    pub fn new(lang_id: u16) -> Self {
        let parser = unsafe { schema_parser_init(lang_id) };
        assert!(!parser.is_null(), "Failed to init C++ schema parser");
        Self { parser }
    }

    /// Parsuje pojedynczy plik źródłowy
    pub fn parse_file(&self, path: &str) -> u32 {
        let c_path = CString::new(path).unwrap();
        unsafe { schema_parser_parse_file(self.parser, c_path.as_ptr()) }
    }

    /// Rekurencyjnie skanuje folder i parsuje wszystkie pliki danego języka
    /// Przechodzi przez dowolnie głębokie drzewo katalogów!
    pub fn parse_directory(&self, dir: &str) -> u32 {
        let c_dir = CString::new(dir).unwrap();
        unsafe { schema_parser_parse_dir(self.parser, c_dir.as_ptr()) }
    }

    /// Pobiera nagłówek schematyku (hex metadane)
    pub fn get_header(&self) -> SchemaHeader {
        unsafe { schema_parser_get_header(self.parser) }
    }

    /// Kopiuje węzły AST z C++ do Rust (bezpieczne)
    pub fn get_nodes(&self) -> Vec<AstNode> {
        let mut count: u32 = 0;
        let ptr = unsafe { schema_parser_get_nodes(self.parser, &mut count) };
        if ptr.is_null() || count == 0 { return vec![]; }
        let slice = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
        slice.to_vec()
    }

    /// Kopiuje słowa kluczowe z C++ do Rust
    pub fn get_keywords(&self) -> Vec<KeywordEntry> {
        let mut count: u32 = 0;
        let ptr = unsafe { schema_parser_get_keywords(self.parser, &mut count) };
        if ptr.is_null() || count == 0 { return vec![]; }
        let slice = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
        slice.to_vec()
    }

    /// Zapisuje binarny schematyk do pliku .sch
    pub fn write_schema(&self, output_path: &str) -> Result<(), String> {
        let c_path = CString::new(output_path).unwrap();
        let result = unsafe { schema_parser_write_schema(self.parser, c_path.as_ptr()) };
        if result == 0 { Ok(()) } else { Err("Failed to write schema".into()) }
    }
}

impl Drop for SchemaEngine {
    fn drop(&mut self) {
        unsafe { schema_parser_free(self.parser); }
    }
}

// ============================================================
// WCZYTYWANIE SCHEMATYKÓW Z DYSKU (bez FFI — czysty Rust)
// ============================================================

/// Magiczna liczba pliku `.sch` („SCHM”).
pub const SCHEMA_MAGIC: u32 = 0x5343_484D;

/// Wczytany z dysku schematyk maszynowy.
#[derive(Debug, Clone)]
pub struct Schema {
    /// Nagłówek (metadane hex).
    pub header: SchemaHeader,
    /// Węzły AST z mapowaniem na efekty maszynowe.
    pub nodes: Vec<AstNode>,
    /// Słowa kluczowe z kategoriami i częstotliwościami.
    pub keywords: Vec<KeywordEntry>,
}

/// Odczytuje binarny schematyk z pliku `.sch`.
pub fn read_schema(path: &Path) -> Result<Schema, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("nie udało się wczytać {}: {error}", path.display()))?;

    let header_size = std::mem::size_of::<SchemaHeader>();
    if bytes.len() < header_size {
        return Err(format!("{}: plik za krótki na nagłówek", path.display()));
    }

    let header: &SchemaHeader = bytemuck::try_from_bytes(&bytes[..header_size])
        .map_err(|error| format!("{}: zły nagłówek ({error})", path.display()))?;
    let header = *header;

    if header.magic != SCHEMA_MAGIC {
        return Err(format!(
            "{}: zły magic 0x{:08X} (oczekiwano 0x{SCHEMA_MAGIC:08X})",
            path.display(),
            header.magic
        ));
    }

    let node_size = std::mem::size_of::<AstNode>();
    let keyword_size = std::mem::size_of::<KeywordEntry>();
    let nodes_start = header_size;
    let nodes_end = nodes_start + header.node_count as usize * node_size;
    let keywords_end = nodes_end + header.keyword_count as usize * keyword_size;

    if bytes.len() < keywords_end {
        return Err(format!("{}: plik obcięty", path.display()));
    }

    let nodes: Vec<AstNode> = bytemuck::try_cast_slice(&bytes[nodes_start..nodes_end])
        .map_err(|error| format!("{}: złe węzły AST ({error})", path.display()))?
        .to_vec();
    let keywords: Vec<KeywordEntry> = bytemuck::try_cast_slice(&bytes[nodes_end..keywords_end])
        .map_err(|error| format!("{}: złe słowa kluczowe ({error})", path.display()))?
        .to_vec();

    Ok(Schema {
        header,
        nodes,
        keywords,
    })
}

/// Wynik zbudowania jednego schematyku (do raportu).
#[derive(Debug, Clone)]
pub struct SchemaSummary {
    /// Nazwa języka (np. `rust`).
    pub lang: String,
    /// Liczba węzłów AST.
    pub node_count: u32,
    /// Liczba słów kluczowych.
    pub keyword_count: u32,
    /// Całkowity rozmiar pliku.
    pub total_size: u32,
    /// Ścieżka zapisanego pliku `.sch`.
    pub output_path: String,
}

// ============================================================
// GŁÓWNA FUNKCJA: Budowanie Mapy Schematyków
// ============================================================

/// Buduje mapę schematyków maszynowych: parsuje źródła danego języka,
/// zapisuje binarne `.sch` na dysk i zwraca podsumowania do raportu.
///
/// `project_root` to katalog skrzynki (`CARGO_MANIFEST_DIR`) — skanujemy
/// jej własne źródła, żeby demo było szybkie i samowystarczalne.
pub fn build_schema_map(project_root: &str) -> Vec<SchemaSummary> {
    let schemas_dir = format!("{}/schemas", project_root);
    std::fs::create_dir_all(&schemas_dir).unwrap();

    // Lista języków do analizy (hex kody z LangId).
    let languages: Vec<(u16, &str, &str)> = vec![
        (0x5253, "rust", "src"),   // Rust
        (0x4246, "beef", "tasks"), // Beef (gui_calc.beef)
        (0x4350, "cpp", "cpp"),    // C++ (schema_parser.cpp)
    ];

    let mut summaries = Vec::new();

    for (lang_id, lang_name, sub_dir) in languages {
        let full_dir = format!("{}/{sub_dir}", project_root);
        if !Path::new(&full_dir).exists() {
            continue;
        }

        let engine = SchemaEngine::new(lang_id);
        engine.parse_directory(&full_dir);
        let header = engine.get_header();

        // Zapisz pełny schematyk.
        let output = format!("{schemas_dir}/{lang_name}_full.sch");
        if engine.write_schema(&output).is_err() {
            continue;
        }

        summaries.push(SchemaSummary {
            lang: lang_name.to_string(),
            node_count: header.node_count,
            keyword_count: header.keyword_count,
            total_size: header.total_size,
            output_path: output,
        });
    }

    summaries
}