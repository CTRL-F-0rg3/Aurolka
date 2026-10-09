// cpp/schema_parser.h
#pragma once

#include <cstdint>
#include <cstddef>

// ============================================================
// BINARNY FORMAT SCHEMATYKU (Hex Nagłówek + Dane)
// ============================================================

// Magic number dla pliku schematyku: "SCHM" w hex
constexpr uint32_t SCHEMA_MAGIC = 0x5343484D;

// Typy języków (hex kody)
enum class LangId : uint16_t {
    RUST    = 0x5253, // "RS"
    BEEF    = 0x4246, // "BF"
    CPP     = 0x4350, // "CP"
    C       = 0x4300, // "C"
    ASM_X86 = 0x4158, // "AX"
    ASM_RV  = 0x4152, // "AR" (RISC-V)
    UNKNOWN = 0x0000
};

// Typy węzłów AST (jak konstrukcja języka mapuje się na maszynę)
enum class AstNodeType : uint8_t {
    KEYWORD     = 0x01, // Słowo kluczowe (fn, let, if, class...)
    TYPE_DEF    = 0x02, // Definicja typu (struct, enum, class)
    FUNCTION    = 0x03, // Funkcja / metoda
    EXPRESSION  = 0x04, // Wyrażenie (a + b, x.y)
    STATEMENT   = 0x05, // Instrukcja (return, break, for)
    LITERAL     = 0x06, // Literał (42, "hello", true)
    OPERATOR    = 0x07, // Operator (+, -, *, ==, &&)
    BLOCK       = 0x08, // Blok { }
    IMPORT      = 0x09, // Import / use / #include
    ATTRIBUTE   = 0x0A, // Atrybut / dekorator (#[derive], @override)
};

// Mapowanie na stany maszyny (jak konstrukcja wpływa na rejestry/flagi)
enum class MachineEffect : uint8_t {
    NONE        = 0x00,
    STACK_PUSH  = 0x01, // Wymaga push na stos (np. call, local var)
    STACK_POP   = 0x02, // Wymaga pop (np. ret, end scope)
    REG_WRITE   = 0x03, // Zapis do rejestru (np. mov, let x =)
    REG_READ    = 0x04, // Odczyt z rejestru (np. add rax, rbx)
    ALU_OP      = 0x05, // Operacja ALU (np. +, -, *, /)
    BRANCH      = 0x06, // Skok warunkowy (if, match, while)
    SYSCALL     = 0x07, // Wywołanie systemowe (println!, File::open)
    HEAP_ALLOC  = 0x08, // Alokacja na stercie (Box::new, new)
    HEAP_FREE   = 0x09, // Dealokacja (drop, delete)
    SIMD_OP     = 0x0A, // Operacja wektorowa (SIMD)
};

// ============================================================
// STRUKTURY BINARNE (identyczne w C++ i Rust — #[repr(C)])
// ============================================================

// Nagłówek pliku schematyku (32 bajty)
struct SchemaHeader {
    uint32_t magic;          // 0x5343484D ("SCHM")
    uint16_t lang_id;        // LangId (hex kod języka)
    uint16_t version;        // Wersja formatu (0x0001)
    uint32_t node_count;     // Liczba węzłów AST
    uint32_t keyword_count;  // Liczba słów kluczowych
    uint32_t total_size;     // Całkowity rozmiar pliku w bajtach
    uint32_t crc32;          // Suma kontrolna danych
    uint32_t reserved;       // Rezerwa na przyszłość
};

// Węzeł AST w formie binarnej (48 bajtów)
struct AstNode {
    uint8_t  node_type;      // AstNodeType
    uint8_t  machine_effect; // MachineEffect
    uint16_t depth;          // Głębokość w drzewie AST
    uint32_t line_number;    // Linia w pliku źródłowym
    uint32_t col_number;     // Kolumna
    uint32_t token_hash;     // Hash tokenu (np. "fn" -> 0xA3F2)
    float    complexity;     // Złożoność obliczeniowa (0.0 - 1.0)
    float    machine_cost;   // "Koszt" maszynowy (ile cykli CPU)
    uint32_t parent_idx;     // Indeks węzła nadrzędnego
    uint32_t child_count;    // Liczba dzieci
    uint32_t reg_mask;       // Maska rejestrów (które rejestry używa)
    uint32_t flag_mask;      // Maska flag (które flagi modyfikuje)
    uint32_t sector_hint;    // Sugerowany sektor hex w GPU
    uint32_t padding;        // Wyrównanie do 48 bajtów
};

// Słowo kluczowe języka (32 bajty)
struct KeywordEntry {
    char     text[24];       // Tekst słowa (np. "fn", "let", "mut")
    uint8_t  category;       // 0=control, 1=type, 2=memory, 3=io
    uint8_t  machine_effect; // MachineEffect
    uint16_t frequency;      // Jak często występuje w analizowanych plikach
    uint32_t token_hash;     // Hash
    uint32_t reserved;
};

// ============================================================
// INTERFEJS FFI (C++ -> Rust)
// ============================================================

extern "C" {
    // Inicjalizuje parser dla danego języka
    void* schema_parser_init(uint16_t lang_id);

    // Parsuje plik źródłowy i zwraca liczbę węzłów AST
    uint32_t schema_parser_parse_file(void* parser, const char* file_path);

    // Parsuje cały folder rekurencyjnie
    uint32_t schema_parser_parse_dir(void* parser, const char* dir_path);

    // Pobiera nagłówek wygenerowanego schematyku
    SchemaHeader schema_parser_get_header(void* parser);

    // Pobiera węzły AST (zwraca wskaźnik do tablicy, Rust kopiuje)
    const AstNode* schema_parser_get_nodes(void* parser, uint32_t* out_count);

    // Pobiera słowa kluczowe
    const KeywordEntry* schema_parser_get_keywords(void* parser, uint32_t* out_count);

    // Zapisuje schematyk do pliku binarnego
    int schema_parser_write_schema(void* parser, const char* output_path);

    // Zwalnia pamięć
    void schema_parser_free(void* parser);

    // Mapuje węzeł AST na stan maszyny (dla GPU)
    MachineEffect schema_map_to_machine(uint8_t node_type, uint16_t lang_id);
}