#!/bin/bash
# Narzędzie do tworzenia nowego modułu Rust w architekturze Aurole
# Wywoływane przez EvolutionEngine

MODULE_NAME=$1
MODULE_TYPE=$2 # np. "nlp", "vision", "math"
TARGET_DIR="./crates/$MODULE_NAME"

if [ -z "$MODULE_NAME" ]; then
    echo "ERROR: Missing module name"
    exit 1
fi

echo "[DEVTOOL] Tworzę nowy moduł: $MODULE_NAME (typ: $MODULE_TYPE)"

# 1. Utwórz strukturę katalogów
mkdir -p "$TARGET_DIR/src"

# 2. Wygeneruj Cargo.toml (samodzielna skrzynka, bez zależności)
cat > "$TARGET_DIR/Cargo.toml" <<EOF
[package]
name = "$MODULE_NAME"
version = "0.1.0"
edition = "2021"

[workspace]
EOF

# 3. Wygeneruj szablon lib.rs (samowystarczalny, poprawny Rust)
cat > "$TARGET_DIR/src/lib.rs" <<EOF
//! Moduł wygenerowany automatycznie przez Aurole Evolution Engine
//! Cel: $MODULE_TYPE

#[derive(Debug, Clone)]
pub struct ${MODULE_NAME^}Petal {
    pub id: u64,
}

impl ${MODULE_NAME^}Petal {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    // Tu model będzie wstrzykiwał swoją logikę w kolejnych iteracjach
    pub fn process(&self, input: &[u8]) -> Vec<u8> {
        input.iter().map(|byte| byte ^ 0xA5).collect()
    }
}
EOF

echo "[DEVTOOL] Moduł $MODULE_NAME utworzony w $TARGET_DIR"
exit 0