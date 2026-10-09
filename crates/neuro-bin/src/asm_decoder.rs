//! Dekoder: surowy wzorzec liczbowy z GPU → twarda linia asemblera x86-64.
//!
//! Słownik operacji jest twardy i deterministyczny — żadnych halucynacji.
//! Klucz to wektor liczbowy (wzorzec kierunku), wartość to mnemonik
//! z gotowym szablonem NASM i kodem operacji.
//!
//! Format wyjściowy to poprawna składnia NASM (tryb 64-bit, Intel):
//! `add RAX, RBX`, `cqo` + `idiv RBX`, `sqrtsd xmm0, xmm0`, `call log`, `ret`.
//!
//! Dopasowanie jest niezmiennicze na skalę: wektor odczytany z GPU
//! (wygaszony dyfuzją) i wzorzec słownika są normalizowane przed
//! policzeniem odległości euklidesowej.

/// Jeden wpis słownika: wzorzec kierunku, mnemonik, szablon NASM,
/// liczba operandów rejestrowych i kod operacji.
///
/// `reg_operands` mówi, ile indeksów rejestrów dekoder odczytuje z wektorów
/// (2 → `add {dst}, {src}`, 1 → `idiv {src}`, 0 → szablon bez rejestrów),
/// a `opcode` dokumentuje kod maszynowy x86-64.
#[derive(Debug, Clone, Copy)]
pub struct OpcodeEntry {
    /// Wzorzec kierunku w przestrzeni cech.
    pub pattern: [f32; 4],
    /// Mnemonik zgodny z `RequiredOpcodes` w XML.
    pub mnemonic: &'static str,
    /// Szablon NASM (`{dst}` / `{src}` — rejestry z mapy zadania).
    pub template: &'static str,
    /// Ile operandów rejestrowych wstawia dekoder (0, 1 albo 2).
    pub reg_operands: u8,
    /// Kod maszynowy (dla dokumentacji, nie do emisji).
    pub opcode: u8,
}

const fn entry(
    pattern: [f32; 4],
    mnemonic: &'static str,
    template: &'static str,
    reg_operands: u8,
    opcode: u8,
) -> OpcodeEntry {
    OpcodeEntry {
        pattern,
        mnemonic,
        template,
        reg_operands,
        opcode,
    }
}

/// Słownik operacji kalkulatora. Wektory dobrane tak, by po normalizacji
/// każda para różniła się o ponad 0.5 (próg odrzucenia).
pub const OPCODE_DICTIONARY: [OpcodeEntry; 9] = [
    entry([0.9, 0.1, 0.0, 0.0], "MOV", "mov {dst}, {src}", 2, 0x89),
    entry([0.1, 0.9, 0.0, 0.0], "ADD", "add {dst}, {src}", 2, 0x01),
    entry([0.0, 0.1, 0.9, 0.0], "SUB", "sub {dst}, {src}", 2, 0x29),
    entry([0.0, 0.0, 0.1, 0.9], "CMP", "cmp {dst}, {src}", 2, 0x39),
    entry([0.7, 0.7, 0.0, 0.0], "MUL", "imul {dst}, {src}", 2, 0xAF),
    entry([0.0, 0.7, 0.7, 0.0], "DIV", "cqo\n    idiv {src}", 1, 0xF7),
    entry(
        [0.7, 0.0, 0.7, 0.0],
        "LOG",
        "sub rsp, 8\n    call log\n    add rsp, 8",
        0,
        0xE8,
    ),
    entry([0.0, 0.7, 0.0, 0.7], "SQRT", "sqrtsd xmm0, xmm0", 0, 0xF2),
    entry([0.5, 0.5, 0.5, 0.5], "RET", "ret", 0, 0xC3),
];

/// Surowa instrukcja odczytana z GPU: trzy wektory i pewność sieci.
#[repr(C)]
pub struct GpuInstruction {
    /// Wzorzec operacji (kierunek w przestrzeni cech).
    pub opcode_vec: [f32; 4],
    /// Pierwszy operand (rejestr w `vec[0]`).
    pub op1_vec: [f32; 4],
    /// Drugi operand (rejestr w `vec[0]`).
    pub op2_vec: [f32; 4],
    /// Pewność sieci 0..=1 (jakość zachowania wzorca przez GPU).
    pub confidence: f32,
}

/// Nazwy rejestrów x86-64 — indeks `vec[0]` to pozycja na liście.
pub const REG_NAMES: [&str; 16] = [
    "RAX", "RBX", "RCX", "RDX", "RSI", "RDI", "RSP", "RBP", "R8", "R9", "R10", "R11", "R12",
    "R13", "R14", "R15",
];

/// Dekoder instrukcji.
pub struct AsmDecoder;

/// Zdekodowana instrukcja: kanoniczny mnemonik ze słownika + linia NASM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedInstruction {
    /// Mnemonik wpisu słownika (np. `"MUL"` — do porównania z planem/XML).
    pub mnemonic: &'static str,
    /// Gotowa linia NASM (może być wieloliniowa).
    pub text: String,
}

impl AsmDecoder {
    /// Znajduje wzorzec słownika dla danego mnemonika.
    pub fn pattern_for(mnemonic: &str) -> Option<[f32; 4]> {
        OPCODE_DICTIONARY
            .iter()
            .find(|entry| entry.mnemonic.eq_ignore_ascii_case(mnemonic))
            .map(|entry| entry.pattern)
    }

    /// Normalizuje wektor do długości 1 (zerowy zostaje zerowy).
    pub fn normalize(vector: [f32; 4]) -> [f32; 4] {
        let len = (vector[0].powi(2) + vector[1].powi(2) + vector[2].powi(2) + vector[3].powi(2))
            .sqrt();
        if len < f32::EPSILON {
            return [0.0; 4];
        }
        [
            vector[0] / len,
            vector[1] / len,
            vector[2] / len,
            vector[3] / len,
        ]
    }

    /// Odległość euklidesowa między znormalizowanymi wektorami.
    pub fn pattern_distance(a: [f32; 4], b: [f32; 4]) -> f32 {
        let a = Self::normalize(a);
        let b = Self::normalize(b);
        ((a[0] - b[0]).powi(2)
            + (a[1] - b[1]).powi(2)
            + (a[2] - b[2]).powi(2)
            + (a[3] - b[3]).powi(2))
        .sqrt()
    }

    /// Zamienia surowy wzorzec liczbowy z GPU na twardą instrukcję asemblera.
    ///
    /// Szuka najbliższego wzorca w słowniku (odległość euklidesowa
    /// po normalizacji). Jeśli pewność sieci jest zbyt niska albo wzorzec
    /// za daleko od słownika — odrzuca (zabezpieczenie przed błędnym kodem).
    pub fn decode_instruction(gpu_instr: &GpuInstruction) -> Option<DecodedInstruction> {
        // 1. Znajdź najbliższy wzorzec w słowniku.
        let mut best: Option<&OpcodeEntry> = None;
        let mut min_dist = f32::MAX;

        for entry in OPCODE_DICTIONARY.iter() {
            let dist = Self::pattern_distance(gpu_instr.opcode_vec, entry.pattern);
            if dist < min_dist {
                min_dist = dist;
                best = Some(entry);
            }
        }
        let entry = best?;

        // 2. Jeśli pewność sieci jest zbyt niska, odrzucamy.
        if gpu_instr.confidence < 0.85 || min_dist > 0.5 {
            return None; // Sieć nie jest pewna, nie generuj kodu.
        }

        // 3. Dekodowanie operandów (rejestrów).
        let reg1_idx = gpu_instr.op1_vec[0].round() as i64;
        let reg2_idx = gpu_instr.op2_vec[0].round() as i64;
        if !(0..REG_NAMES.len() as i64).contains(&reg1_idx)
            || !(0..REG_NAMES.len() as i64).contains(&reg2_idx)
        {
            return None;
        }
        let (dst, src) = (REG_NAMES[reg1_idx as usize], REG_NAMES[reg2_idx as usize]);

        // 4. Złożenie finalnej, poprawnej składniowo linii asemblera.
        Some(DecodedInstruction {
            mnemonic: entry.mnemonic,
            text: entry.template.replace("{dst}", dst).replace("{src}", src),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instr(pattern: [f32; 4], op1: f32, op2: f32) -> GpuInstruction {
        GpuInstruction {
            opcode_vec: pattern,
            op1_vec: [op1, 0.0, 0.0, 0.0],
            op2_vec: [op2, 0.0, 0.0, 0.0],
            confidence: 0.99,
        }
    }

    #[test]
    fn dekoduje_dodawanie_z_rejestrami() {
        let decoded =
            AsmDecoder::decode_instruction(&instr([0.1, 0.9, 0.0, 0.0], 0.0, 1.0)).unwrap();
        assert_eq!(decoded.mnemonic, "ADD");
        assert_eq!(decoded.text, "add RAX, RBX");
    }

    #[test]
    fn dopasowanie_jest_niezmiennicze_na_skale() {
        // Wzorzec wygaszony dyfuzją (×0.05) nadal dekoduje się tak samo.
        let decoded =
            AsmDecoder::decode_instruction(&instr([0.005, 0.045, 0.0, 0.0], 0.0, 1.0)).unwrap();
        assert_eq!(decoded.mnemonic, "ADD");
        assert_eq!(decoded.text, "add RAX, RBX");
    }

    #[test]
    fn slownik_rozpoznaje_szesciu_operatorow_kalkulatora() {
        for (mnemonic, pattern, dst, src, expected) in [
            ("ADD", [0.1, 0.9, 0.0, 0.0], 0.0, 1.0, "add RAX, RBX"),
            ("SUB", [0.0, 0.1, 0.9, 0.0], 0.0, 2.0, "sub RAX, RCX"),
            ("MUL", [0.7, 0.7, 0.0, 0.0], 0.0, 1.0, "imul RAX, RBX"),
            ("DIV", [0.0, 0.7, 0.7, 0.0], 0.0, 1.0, "cqo\n    idiv RBX"),
            (
                "LOG",
                [0.7, 0.0, 0.7, 0.0],
                0.0,
                0.0,
                "sub rsp, 8\n    call log\n    add rsp, 8",
            ),
            ("SQRT", [0.0, 0.7, 0.0, 0.7], 0.0, 0.0, "sqrtsd xmm0, xmm0"),
        ] {
            let decoded = AsmDecoder::decode_instruction(&instr(pattern, dst, src)).unwrap();
            assert_eq!(decoded.mnemonic, mnemonic);
            assert_eq!(decoded.text, expected, "mnemonik {mnemonic}");
        }
    }

    #[test]
    fn niska_pewnosc_odrzuca_instrukcje() {
        let mut bad = instr([0.1, 0.9, 0.0, 0.0], 0.0, 1.0);
        bad.confidence = 0.5;
        assert!(AsmDecoder::decode_instruction(&bad).is_none());
    }

    #[test]
    fn obcy_wzorzec_jest_odrzucany() {
        let foreign = instr([-1.0, -1.0, -1.0, -1.0], 0.0, 1.0);
        assert!(AsmDecoder::decode_instruction(&foreign).is_none());
    }
}
