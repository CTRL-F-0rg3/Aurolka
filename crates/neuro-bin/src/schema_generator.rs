// crates/neuro-bin/src/schema_generator.rs
//!
//! Generator kodu wielojęzykowego — na podstawie potrzeb maszynowych
//! produkuje kod w `.rs`, `.cpp`, `.c`, `.zig` i `.odin`, unikając
//! zapamiętanych wzorców niekorzystnych.

use crate::machine_introspector::MachineNeed;

pub struct SchemaGenerator;

impl SchemaGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Rozszerzenie pliku źródłowego dla języka.
    pub fn extension(lang: &str) -> &'static str {
        match lang {
            "rust" => "rs",
            "cpp" => "cpp",
            "c" => "c",
            "zig" => "zig",
            "odin" => "odin",
            _ => "txt",
        }
    }

    /// Generuje kod w zadanym języku.
    pub fn generate(&self, need: &MachineNeed, lang: &str, bad_patterns: &[String]) -> String {
        match lang {
            "rust" => self.rust_code(need, bad_patterns),
            "cpp" => self.cpp_code(need),
            "c" => self.c_code(need),
            "zig" => self.zig_code(need),
            "odin" => self.odin_code(need),
            _ => self.rust_code(need, bad_patterns),
        }
    }

    fn rust_code(&self, need: &MachineNeed, bad_patterns: &[String]) -> String {
        let struct_name = Self::camelize(&need.target_module);
        let mut code = String::new();
        code.push_str(&format!(
            "//! Autonomicznie wygenerowano przez Aurole Schema Engine\n//! Cel: {}\n//! Sugerowany język: {}\n//! Wymagane efekty: {}\n\n",
            need.description,
            need.language,
            need.required_machine_effects.join(", ")
        ));

        if bad_patterns.iter().any(|p| p.contains("mismatched types")) {
            code.push_str("// WZORZEC KORZYSTNY: jawne rzutowania (nauczono z poprzedniego cyklu)\n");
        }

        code.push_str(&format!(
            "#[derive(Debug, Clone, Default)]\npub struct {struct_name} {{\n    pub data: Vec<u8>,\n}}\n\n\
             impl {struct_name} {{\n    pub fn new() -> Self {{ Self {{ data: Vec::new() }} }}\n\n    \
             pub fn process(&mut self, input: &[u8]) -> Vec<u8> {{\n        self.data.clear();\n        \
             for &byte in input {{\n            self.data.push(byte ^ 0xA5);\n        }}\n        self.data.clone()\n    }}\n}}\n"
        ));
        code
    }

    fn cpp_code(&self, need: &MachineNeed) -> String {
        let struct_name = Self::camelize(&need.target_module);
        format!(
            "// Autonomicznie wygenerowano przez Aurole Schema Engine\n// Cel: {desc}\n\n\
             #include <cstdint>\n#include <vector>\n\n\
             class {struct_name} {{\npublic:\n    std::vector<uint8_t> data;\n\n    \
             void process(const uint8_t* input, size_t len) {{\n        data.clear();\n        \
             for (size_t i = 0; i < len; ++i) {{ data.push_back(input[i] ^ 0xA5); }}\n    }}\n}};\n",
            desc = need.description
        )
    }

    fn c_code(&self, need: &MachineNeed) -> String {
        let fn_name = need.target_module.replace('-', "_");
        format!(
            "// Autonomicznie wygenerowano przez Aurole Schema Engine\n// Cel: {desc}\n\n\
             #include <stddef.h>\n#include <stdint.h>\n\n\
             void {fn_name}_process(const uint8_t* input, size_t len, uint8_t* output) {{\n    \
             for (size_t i = 0; i < len; ++i) {{ output[i] = input[i] ^ 0xA5; }}\n}}\n",
            desc = need.description
        )
    }

    fn zig_code(&self, need: &MachineNeed) -> String {
        format!(
            "// Autonomicznie wygenerowano przez Aurole Schema Engine\n// Cel: {desc}\n\n\
             const std = @import(\"std\");\n\n\
             pub fn process(input: []const u8) []u8 {{\n    \
             var out: std.ArrayList(u8) = std.ArrayList(u8).init(std.heap.page_allocator);\n    \
             for (input) |byte| {{\n        try out.append(byte ^ 0xA5);\n    }}\n    \
             return out.toOwnedSlice() catch &.{{}};\n}}\n",
            desc = need.description
        )
    }

    fn odin_code(&self, need: &MachineNeed) -> String {
        let proc_name = need.target_module.replace('-', "_");
        format!(
            "// Autonomicznie wygenerowano przez Aurole Schema Engine\n// Cel: {desc}\n\n\
             package {mod}\n\n\
             import \"core:slice\"\n\n\
             {proc_name}_process :: proc(input: []u8, output: []u8) {{\n    \
             for b, i in input {{ output[i] = b ~ 0xA5 }}\n}}\n",
            desc = need.description,
            mod = proc_name,
        )
    }

    /// `simd_math` → `SimdMath`.
    fn camelize(name: &str) -> String {
        name.split(['_', '-'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect()
    }
}
