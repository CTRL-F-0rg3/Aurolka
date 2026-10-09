#!/bin/bash
# Narzędzie do integracji nowego modułu z głównym projektem Aurole

MODULE_NAME=$1
MAIN_CARGO_TOML="./Cargo.toml"
MAIN_RS="./src/main.rs"

if [ -z "$MODULE_NAME" ]; then
    echo "ERROR: Missing module name"; exit 1
fi

echo "[DEVTOOL] Integruję moduł $MODULE_NAME z rdzeniem Aurole..."

# 1. Dodaj moduł do workspace w głównym Cargo.toml
if ! grep -q "\"crates/$MODULE_NAME\"" "$MAIN_CARGO_TOML"; then
    # Znajdź linię z members i dodaj nowy wpis
    sed -i "/members = \[/a \    \"crates/$MODULE_NAME\"," "$MAIN_CARGO_TOML"
    echo "[DEVTOOL] Dodano $MODULE_NAME do workspace"
fi

# 2. Dodaj zależność do głównego binarium
if ! grep -q "$MODULE_NAME =" "$MAIN_CARGO_TOML"; then
    echo "$MODULE_NAME = { path = \"crates/$MODULE_NAME\" }" >> "$MAIN_CARGO_TOML"
    echo "[DEVTOOL] Dodano zależność Cargo"
fi

# 3. Zarejestruj Płatek w main.rs (dodaj mod do listy płatków)
if ! grep -q "mod $MODULE_NAME;" "$MAIN_RS"; then
    echo "mod $MODULE_NAME;" >> "$MAIN_RS"
    echo "[DEVTOOL] Zarejestrowano moduł w main.rs"
fi

echo "[DEVTOOL] Integracja zakończona. Wymagany restart Aurole."
exit 0