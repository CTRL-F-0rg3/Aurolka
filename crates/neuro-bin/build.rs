// crates/neuro-bin/build.rs

fn main() {
    // Kompilujemy silnik C++ analizy składni
    // Używamy C++20 dla std::filesystem (rekurencyjne skanowanie folderów)
    cc::Build::new()
        .cpp(true)
        .std("c++20")
        .flag("-O3")
        .flag("-march=native")      // Optymalizacja pod Twój CPU
        .flag("-Wall")
        .flag("-Wno-unused-parameter")
        // Pliki źródłowe C++
        .file("cpp/schema_parser.cpp")
        // Nagłówki
        .include("cpp")
        .compile("schema_engine_cpp");

    // Informujemy Cargo, żeby przebudował, gdy zmienią się pliki C++
    println!("cargo:rerun-if-changed=cpp/schema_parser.h");
    println!("cargo:rerun-if-changed=cpp/schema_parser.cpp");
    
    // Linkujemy stdc++fs dla std::filesystem na starszych GCC
    println!("cargo:rustc-link-lib=stdc++fs");
}