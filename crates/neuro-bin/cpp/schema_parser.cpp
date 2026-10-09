// cpp/schema_parser.cpp

#include "schema_parser.h"
#include <filesystem>
#include <fstream>
#include <vector>
#include <string>
#include <unordered_map>
#include <algorithm>
#include <cstring>

namespace fs = std::filesystem;

// ============================================================
// WEWNĘTRZNA STRUKTURA PARSERA
// ============================================================

struct SchemaParser {
    LangId lang;
    std::vector<AstNode> nodes;
    std::vector<KeywordEntry> keywords;
    std::unordered_map<std::string, uint32_t> keyword_freq;

    // Słowa kluczowe per język
    static const std::vector<std::pair<std::string, uint8_t>> RUST_KEYWORDS;
    static const std::vector<std::pair<std::string, uint8_t>> BEEF_KEYWORDS;
    static const std::vector<std::pair<std::string, uint8_t>> CPP_KEYWORDS;
};

// Definicje słów kluczowych z kategoriami:
// 0=control (if, while, for, match)
// 1=type (struct, enum, class, fn, int, f32)
// 2=memory (let, mut, ref, new, delete, Box, Vec)
// 3=io (println, print, read, write, use, import)

const std::vector<std::pair<std::string, uint8_t>> SchemaParser::RUST_KEYWORDS = {
    {"fn", 1}, {"let", 2}, {"mut", 2}, {"struct", 1}, {"enum", 1},
    {"impl", 1}, {"trait", 1}, {"pub", 0}, {"use", 3}, {"mod", 0},
    {"if", 0}, {"else", 0}, {"match", 0}, {"for", 0}, {"while", 0},
    {"loop", 0}, {"return", 0}, {"break", 0}, {"continue", 0},
    {"where", 0}, {"async", 0}, {"await", 0}, {"move", 2},
    {"ref", 2}, {"self", 2}, {"Self", 1}, {"super", 0}, {"crate", 0},
    {"const", 2}, {"static", 2}, {"type", 1}, {"unsafe", 0},
    {"extern", 0}, {"dyn", 1}, {"as", 0}, {"in", 0},
    {"i8", 1}, {"i16", 1}, {"i32", 1}, {"i64", 1}, {"i128", 1},
    {"u8", 1}, {"u16", 1}, {"u32", 1}, {"u64", 1}, {"u128", 1},
    {"f32", 1}, {"f64", 1}, {"bool", 1}, {"char", 1}, {"str", 1},
    {"String", 1}, {"Vec", 1}, {"Box", 2}, {"Option", 1}, {"Result", 1},
    {"Some", 1}, {"None", 1}, {"Ok", 1}, {"Err", 1},
    {"println!", 3}, {"print!", 3}, {"eprintln!", 3},
    {"macro_rules!", 1}, {"derive", 1},
};

const std::vector<std::pair<std::string, uint8_t>> SchemaParser::BEEF_KEYWORDS = {
    {"class", 1}, {"struct", 1}, {"enum", 1}, {"interface", 1},
    {"fn", 1}, {"function", 1}, {"let", 2}, {"var", 2},
    {"if", 0}, {"else", 0}, {"for", 0}, {"while", 0}, {"switch", 0},
    {"case", 0}, {"default", 0}, {"return", 0}, {"break", 0},
    {"continue", 0}, {"using", 3}, {"namespace", 0},
    {"public", 0}, {"private", 0}, {"protected", 0}, {"static", 2},
    {"virtual", 0}, {"override", 0}, {"abstract", 0},
    {"int", 1}, {"int8", 1}, {"int16", 1}, {"int32", 1}, {"int64", 1},
    {"uint", 1}, {"float", 1}, {"double", 1}, {"bool", 1}, {"char8", 1},
    {"String", 1}, {"List", 1}, {"Dictionary", 1},
    {"new", 2}, {"delete", 2}, {"this", 2}, {"base", 2},
    {"true", 1}, {"false", 1}, {"null", 1},
    {"Console", 3}, {"WriteLine", 3},
};

const std::vector<std::pair<std::string, uint8_t>> SchemaParser::CPP_KEYWORDS = {
    {"class", 1}, {"struct", 1}, {"enum", 1}, {"union", 1},
    {"template", 1}, {"typename", 1}, {"namespace", 0},
    {"void", 1}, {"int", 1}, {"char", 1}, {"float", 1}, {"double", 1},
    {"bool", 1}, {"long", 1}, {"short", 1}, {"unsigned", 1}, {"signed", 1},
    {"auto", 1}, {"const", 2}, {"constexpr", 2}, {"static", 2},
    {"volatile", 2}, {"mutable", 2},
    {"if", 0}, {"else", 0}, {"for", 0}, {"while", 0}, {"do", 0},
    {"switch", 0}, {"case", 0}, {"default", 0},
    {"return", 0}, {"break", 0}, {"continue", 0}, {"goto", 0},
    {"new", 2}, {"delete", 2}, {"this", 2},
    {"virtual", 0}, {"override", 0}, {"final", 0},
    {"public", 0}, {"private", 0}, {"protected", 0},
    {"try", 0}, {"catch", 0}, {"throw", 0},
    {"#include", 3}, {"#define", 3}, {"#ifdef", 3},
    {"std", 3}, {"cout", 3}, {"cin", 3},
    {"vector", 1}, {"string", 1}, {"map", 1}, {"unique_ptr", 2},
    {"shared_ptr", 2},
};

// ============================================================
// FUNKCJE POMOCNICZE
// ============================================================

// Prosty hash FNV-1a dla tokenów
static uint32_t hash_token(const std::string& s) {
    uint32_t hash = 0x811C9DC5;
    for (char c : s) {
        hash ^= static_cast<uint8_t>(c);
        hash *= 0x01000193;
    }
    return hash;
}

// Wykrywa język na podstawie rozszerzenia pliku
static LangId detect_lang(const fs::path& p) {
    auto ext = p.extension().string();
    if (ext == ".rs") return LangId::RUST;
    if (ext == ".bf" || ext == ".beef") return LangId::BEEF;
    if (ext == ".cpp" || ext == ".cc" || ext == ".cxx") return LangId::CPP;
    if (ext == ".c") return LangId::C;
    if (ext == ".asm" || ext == ".s") return LangId::ASM_X86;
    return LangId::UNKNOWN;
}

// Mapuje kategorię słowa kluczowego na MachineEffect
static MachineEffect category_to_effect(uint8_t category) {
    switch (category) {
        case 0: return MachineEffect::BRANCH;      // control flow
        case 1: return MachineEffect::REG_WRITE;    // type definition
        case 2: return MachineEffect::STACK_PUSH;   // memory operation
        case 3: return MachineEffect::SYSCALL;      // I/O
        default: return MachineEffect::NONE;
    }
}

// Oblicza "koszt maszynowy" konstrukcji (ile cykli CPU ~)
static float estimate_machine_cost(AstNodeType type, MachineEffect effect) {
    float base = 0.0f;
    switch (type) {
        case AstNodeType::KEYWORD:    base = 0.1f; break;
        case AstNodeType::TYPE_DEF:   base = 0.3f; break;
        case AstNodeType::FUNCTION:   base = 0.5f; break;
        case AstNodeType::EXPRESSION: base = 0.2f; break;
        case AstNodeType::STATEMENT:  base = 0.15f; break;
        case AstNodeType::LITERAL:    base = 0.05f; break;
        case AstNodeType::OPERATOR:   base = 0.1f; break;
        case AstNodeType::BLOCK:      base = 0.2f; break;
        case AstNodeType::IMPORT:     base = 0.4f; break;
        case AstNodeType::ATTRIBUTE:  base = 0.15f; break;
    }
    switch (effect) {
        case MachineEffect::HEAP_ALLOC: base += 0.8f; break;
        case MachineEffect::SYSCALL:    base += 0.9f; break;
        case MachineEffect::SIMD_OP:    base += 0.3f; break;
        case MachineEffect::BRANCH:     base += 0.2f; break;
        default: break;
    }
    return std::min(base, 1.0f);
}

// ============================================================
// PARSOWANIE PLIKU ŹRÓDŁOWEGO (Tokenizer + AST Builder)
// ============================================================

static void tokenize_and_build(
    SchemaParser* parser,
    const std::string& source,
    uint32_t& line_num
) {
    // Pobierz listę słów kluczowych dla tego języka
    const std::vector<std::pair<std::string, uint8_t>>* kw_list = nullptr;
    switch (parser->lang) {
        case LangId::RUST: kw_list = &SchemaParser::RUST_KEYWORDS; break;
        case LangId::BEEF: kw_list = &SchemaParser::BEEF_KEYWORDS; break;
        case LangId::CPP:  kw_list = &SchemaParser::CPP_KEYWORDS;  break;
        default: return;
    }

    // Budujemy mapę hash -> (kategoria, tekst) dla szybkiego wyszukiwania
    std::unordered_map<uint32_t, std::pair<uint8_t, std::string>> kw_map;
    for (auto& [text, cat] : *kw_list) {
        kw_map[hash_token(text)] = {cat, text};
    }

    // Prosty tokenizer: dzielimy po białych znakach i separatorach
    // (Pełny parser byłby znacznie bardziej złożony, ale to wystarczy
    //  do wygenerowania schematyku składni i słów kluczowych)
    std::string token;
    uint32_t col = 1;
    uint32_t depth = 0;
    uint32_t parent_idx = 0;

    for (size_t i = 0; i < source.size(); ++i) {
        char c = source[i];

        if (c == '\n') {
            line_num++;
            col = 1;
            if (!token.empty()) {
                // Przetwórz token
                uint32_t h = hash_token(token);
                auto it = kw_map.find(h);

                AstNode node{};
                node.line_number = line_num;
                node.col_number = col;
                node.token_hash = h;
                node.depth = static_cast<uint16_t>(depth);
                node.parent_idx = parent_idx;

                if (it != kw_map.end()) {
                    node.node_type = static_cast<uint8_t>(AstNodeType::KEYWORD);
                    node.machine_effect = static_cast<uint8_t>(
                        category_to_effect(it->second.first)
                    );
                    parser->keyword_freq[it->second.second]++;
                } else if (token[0] >= '0' && token[0] <= '9') {
                    node.node_type = static_cast<uint8_t>(AstNodeType::LITERAL);
                    node.machine_effect = static_cast<uint8_t>(MachineEffect::REG_WRITE);
                } else if (token == "{" || token == "}") {
                    node.node_type = static_cast<uint8_t>(AstNodeType::BLOCK);
                    if (token == "{") { depth++; node.machine_effect = static_cast<uint8_t>(MachineEffect::STACK_PUSH); }
                    else { depth--; node.machine_effect = static_cast<uint8_t>(MachineEffect::STACK_POP); }
                } else if (token == "+" || token == "-" || token == "*" || 
                           token == "/" || token == "==" || token == "!=") {
                    node.node_type = static_cast<uint8_t>(AstNodeType::OPERATOR);
                    node.machine_effect = static_cast<uint8_t>(MachineEffect::ALU_OP);
                } else {
                    node.node_type = static_cast<uint8_t>(AstNodeType::EXPRESSION);
                    node.machine_effect = static_cast<uint8_t>(MachineEffect::REG_READ);
                }

                node.complexity = static_cast<float>(depth) * 0.1f;
                node.machine_cost = estimate_machine_cost(
                    static_cast<AstNodeType>(node.node_type),
                    static_cast<MachineEffect>(node.machine_effect)
                );

                // Maska rejestrów (heurystyka: głębsze = więcej rejestrów)
                node.reg_mask = (1u << std::min(depth, 15u)) - 1;
                node.flag_mask = (node.machine_effect == static_cast<uint8_t>(MachineEffect::ALU_OP)) ? 0x0F : 0x00;
                
                // Sugerowany sektor hex (oparty na typie węzła i głębokości)
                node.sector_hint = (static_cast<uint32_t>(node.node_type) << 16) | 
                                   (static_cast<uint32_t>(depth) << 8) |
                                   (static_cast<uint32_t>(parser->lang));

                parser->nodes.push_back(node);
                token.clear();
            }
            continue;
        }

        if (std::isspace(c)) {
            col++;
            if (!token.empty()) {
                // (token processing same as above — simplified for brevity)
                uint32_t h = hash_token(token);
                auto it = kw_map.find(h);
                if (it != kw_map.end()) {
                    parser->keyword_freq[it->second.second]++;
                    AstNode node{};
                    node.node_type = static_cast<uint8_t>(AstNodeType::KEYWORD);
                    node.machine_effect = static_cast<uint8_t>(category_to_effect(it->second.first));
                    node.token_hash = h;
                    node.line_number = line_num;
                    node.col_number = col;
                    node.depth = static_cast<uint16_t>(depth);
                    node.complexity = static_cast<float>(depth) * 0.1f;
                    node.machine_cost = estimate_machine_cost(
                        static_cast<AstNodeType>(node.node_type),
                        static_cast<MachineEffect>(node.machine_effect)
                    );
                    node.reg_mask = (1u << std::min(depth, 15u)) - 1;
                    node.sector_hint = (static_cast<uint32_t>(node.node_type) << 16) | 
                                       (static_cast<uint32_t>(parser->lang));
                    parser->nodes.push_back(node);
                }
                token.clear();
            }
            continue;
        }

        token += c;
        col++;
    }
}

// ============================================================
// IMPLEMENTACJA FFI
// ============================================================

extern "C" {

void* schema_parser_init(uint16_t lang_id) {
    auto* parser = new SchemaParser();
    parser->lang = static_cast<LangId>(lang_id);
    return parser;
}

uint32_t schema_parser_parse_file(void* handle, const char* file_path) {
    auto* parser = static_cast<SchemaParser*>(handle);
    
    std::ifstream file(file_path);
    if (!file.is_open()) return 0;
    
    std::string source((std::istreambuf_iterator<char>(file)),
                        std::istreambuf_iterator<char>());
    
    uint32_t line = 1;
    tokenize_and_build(parser, source, line);
    
    return static_cast<uint32_t>(parser->nodes.size());
}

uint32_t schema_parser_parse_dir(void* handle, const char* dir_path) {
    auto* parser = static_cast<SchemaParser*>(handle);
    uint32_t total_nodes = 0;

    // REKURENCYJNE SKANOWANIE FOLDERÓW (nawet bardzo głębokich)
    for (auto& entry : fs::recursive_directory_iterator(dir_path, 
            fs::directory_options::skip_permission_denied)) {
        if (!entry.is_regular_file()) continue;
        
        LangId file_lang = detect_lang(entry.path());
        if (file_lang == LangId::UNKNOWN) continue;
        if (parser->lang != file_lang && parser->lang != LangId::UNKNOWN) continue;

        std::ifstream file(entry.path());
        if (!file.is_open()) continue;

        std::string source((std::istreambuf_iterator<char>(file)),
                            std::istreambuf_iterator<char>());

        uint32_t line = 1;
        size_t before = parser->nodes.size();
        tokenize_and_build(parser, source, line);
        total_nodes += static_cast<uint32_t>(parser->nodes.size() - before);
    }

    return total_nodes;
}

SchemaHeader schema_parser_get_header(void* handle) {
    auto* parser = static_cast<SchemaParser*>(handle);
    
    // Budujemy listę słów kluczowych z częstotliwościami
    parser->keywords.clear();
    for (auto& [text, freq] : parser->keyword_freq) {
        KeywordEntry entry{};
        std::strncpy(entry.text, text.c_str(), 23);
        entry.text[23] = '\0';
        entry.frequency = static_cast<uint16_t>(std::min(freq, 65535u));
        entry.token_hash = hash_token(text);
        
        // Znajdź kategorię
        const std::vector<std::pair<std::string, uint8_t>>* kw_list = nullptr;
        switch (parser->lang) {
            case LangId::RUST: kw_list = &SchemaParser::RUST_KEYWORDS; break;
            case LangId::BEEF: kw_list = &SchemaParser::BEEF_KEYWORDS; break;
            case LangId::CPP:  kw_list = &SchemaParser::CPP_KEYWORDS;  break;
            default: break;
        }
        if (kw_list) {
            for (auto& [kw, cat] : *kw_list) {
                if (kw == text) {
                    entry.category = cat;
                    entry.machine_effect = static_cast<uint8_t>(category_to_effect(cat));
                    break;
                }
            }
        }
        parser->keywords.push_back(entry);
    }

    SchemaHeader header{};
    header.magic = SCHEMA_MAGIC;
    header.lang_id = static_cast<uint16_t>(parser->lang);
    header.version = 0x0001;
    header.node_count = static_cast<uint32_t>(parser->nodes.size());
    header.keyword_count = static_cast<uint32_t>(parser->keywords.size());
    header.total_size = sizeof(SchemaHeader) + 
                        header.node_count * sizeof(AstNode) + 
                        header.keyword_count * sizeof(KeywordEntry);
    header.crc32 = 0; // TODO: oblicz CRC32
    return header;
}

const AstNode* schema_parser_get_nodes(void* handle, uint32_t* out_count) {
    auto* parser = static_cast<SchemaParser*>(handle);
    *out_count = static_cast<uint32_t>(parser->nodes.size());
    return parser->nodes.data();
}

const KeywordEntry* schema_parser_get_keywords(void* handle, uint32_t* out_count) {
    auto* parser = static_cast<SchemaParser*>(handle);
    *out_count = static_cast<uint32_t>(parser->keywords.size());
    return parser->keywords.data();
}

int schema_parser_write_schema(void* handle, const char* output_path) {
    auto* parser = static_cast<SchemaParser*>(handle);
    SchemaHeader header = schema_parser_get_header(handle);

    std::ofstream out(output_path, std::ios::binary);
    if (!out.is_open()) return -1;

    out.write(reinterpret_cast<const char*>(&header), sizeof(header));
    out.write(reinterpret_cast<const char*>(parser->nodes.data()), 
              parser->nodes.size() * sizeof(AstNode));
    out.write(reinterpret_cast<const char*>(parser->keywords.data()), 
              parser->keywords.size() * sizeof(KeywordEntry));

    return 0;
}

void schema_parser_free(void* handle) {
    delete static_cast<SchemaParser*>(handle);
}

MachineEffect schema_map_to_machine(uint8_t node_type, uint16_t lang_id) {
    // Mapowanie zależne od języka (np. "let" w Rust = STACK_PUSH, w C++ "auto" = REG_WRITE)
    auto nt = static_cast<AstNodeType>(node_type);
    switch (nt) {
        case AstNodeType::KEYWORD:    return MachineEffect::BRANCH;
        case AstNodeType::FUNCTION:   return MachineEffect::STACK_PUSH;
        case AstNodeType::EXPRESSION: return MachineEffect::ALU_OP;
        case AstNodeType::STATEMENT:  return MachineEffect::REG_WRITE;
        case AstNodeType::IMPORT:     return MachineEffect::SYSCALL;
        default: return MachineEffect::NONE;
    }
}

} // extern "C"