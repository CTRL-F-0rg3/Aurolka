//! Dekoder Beef: wzorzec liczbowy z GPU → linia kodu Beef.
//!
//! Beef to język z rodziny C#: bloki w nawiasach klamrowych, `let mut`
//! dla zmiennych, wywołania metod po kropce. Dekoder działa tak samo jak
//! [`AsmDecoder`](crate::asm_decoder::AsmDecoder): twardy słownik wzorców,
//! dopasowanie niezmiennicze na skalę, próg pewności.
//!
//! Zwracany jest kanoniczny mnemonik ze słownika (np. `"BUTTON"`), żeby
//! maszyna mogła go porównać z planem i ograniczeniami z XML.

/// Jeden wpis słownika: wzorzec kierunku, mnemonik i szablon Beef.
#[derive(Debug, Clone, Copy)]
pub struct BeefEntry {
    /// Wzorzec kierunku w przestrzeni cech.
    pub pattern: [f32; 4],
    /// Kanoniczny mnemonik (porównywany z planem i XML).
    pub mnemonic: &'static str,
    /// Szablon linii Beef (`{dst}` / `{src}` — nazwy ze słownika GUI).
    pub template: &'static str,
}

const fn entry(pattern: [f32; 4], mnemonic: &'static str, template: &'static str) -> BeefEntry {
    BeefEntry {
        pattern,
        mnemonic,
        template,
    }
}

/// Słownik konstrukcji Beef dla kalkulatora graficznego.
///
/// Wektory rozsunięte tak, by po normalizacji każda para różniła się
/// o ponad 0.5 (próg odrzucenia).
pub const BEEF_DICTIONARY: [BeefEntry; 12] = [
    entry([0.9, 0.1, 0.0, 0.0], "USING", "using System;\nusing BeefGE;"),
    entry([0.1, 0.9, 0.0, 0.0], "CLASS", "class {dst} : Window"),
    entry([0.0, 0.1, 0.9, 0.0], "FIELD", "    {dst} m{dst};"),
    entry([0.0, 0.0, 0.1, 0.9], "METHOD", "    void {dst}()"),
    entry(
        [0.7, 0.7, 0.0, 0.0],
        "WINDOW",
        "        this.Title = \"Kalkulator\";\n        this.Size = {dst};",
    ),
    entry(
        [0.0, 0.7, 0.7, 0.0],
        "BUTTON",
        "        m{dst} = new Button();\n        m{dst}.Label = \"{src}\";\n        m{dst}.Position = {src};",
    ),
    entry(
        [0.7, 0.0, 0.7, 0.0],
        "EVENT",
        "        m{dst}.Click += new (obj) => OnButtonClick(\"{src}\");",
    ),
    entry([0.0, 0.7, 0.0, 0.7], "HANDLER", "    void OnButtonClick({dst} label)"),
    entry(
        [0.5, 0.5, 0.5, 0.5],
        "RENDER",
        "    void RenderUI()\n    {\n        AddChild(mDisplay);\n    }",
    ),
    entry(
        [0.6, 0.4, 0.6, 0.4],
        "LOOP",
        "    public override void Update()\n    {\n        base.Update();\n    }",
    ),
    entry([0.4, 0.6, 0.4, 0.6], "CALL", "        {dst}.{src}();"),
    entry([0.8, 0.2, 0.8, 0.2], "END", "}"),
];

/// Zdekodowana linia Beef.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeefInstruction {
    /// Kanoniczny mnemonik wpisu słownika (np. `"BUTTON"`).
    pub mnemonic: &'static str,
    /// Gotowa linia (lub blok) kodu Beef.
    pub text: String,
}

/// Nazwy własne dostępne dekoderowi (`{dst}` / `{src}`) — kontury kalkulatora.
pub const GUI_NAMES: [&str; 16] = [
    "CalculatorWindow",
    "Button",
    "Display",
    "mButton",
    "RenderUI",
    "OnButtonClick",
    "Update",
    "Main",
    "7",
    "8",
    "9",
    "0",
    "(10, 60)",
    "(100, 60)",
    "250, 350",
    "e",
];

/// Dekoder instrukcji Beef.
pub struct BeefDecoder;

impl BeefDecoder {
    /// Znajduje wzorzec słownika dla danego mnemonika.
    pub fn pattern_for(mnemonic: &str) -> Option<[f32; 4]> {
        BEEF_DICTIONARY
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
        [vector[0] / len, vector[1] / len, vector[2] / len, vector[3] / len]
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

    /// Zamienia surowy wzorzec z GPU na linię kodu Beef.
    ///
    /// Odrzuca wzorzec, gdy pewność sieci < 0.85 albo odległość do
    /// najbliższego wpisu słownika > 0.5.
    pub fn decode_to_beef(
        gpu_pattern: &[f32; 4],
        op1: f32,
        op2: f32,
        confidence: f32,
    ) -> Option<BeefInstruction> {
        // 1. Szukamy najbliższego wzorca w słowniku.
        let mut best: Option<&BeefEntry> = None;
        let mut min_dist = f32::MAX;

        for entry in BEEF_DICTIONARY.iter() {
            let dist = Self::pattern_distance(*gpu_pattern, entry.pattern);
            if dist < min_dist {
                min_dist = dist;
                best = Some(entry);
            }
        }
        let entry = best?;

        // 2. Odrzucamy niepewne wzorce.
        if confidence < 0.85 || min_dist > 0.5 {
            return None;
        }

        // 3. Dekodowanie nazw własnych.
        let dst_idx = op1.round() as i64;
        let src_idx = op2.round() as i64;
        if !(0..GUI_NAMES.len() as i64).contains(&dst_idx)
            || !(0..GUI_NAMES.len() as i64).contains(&src_idx)
        {
            return None;
        }
        let (dst, src) = (GUI_NAMES[dst_idx as usize], GUI_NAMES[src_idx as usize]);

        // 4. Złożenie finalnej linii.
        Some(BeefInstruction {
            mnemonic: entry.mnemonic,
            text: entry.template.replace("{dst}", dst).replace("{src}", src),
        })
    }

    /// Pierwsza linia tekstu — dla raportu.
    pub fn first_line(text: &str) -> &str {
        text.lines().next().unwrap_or(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instr(pattern: [f32; 4], op1: f32, op2: f32) -> Option<BeefInstruction> {
        BeefDecoder::decode_to_beef(&pattern, op1, op2, 0.99)
    }

    #[test]
    fn dekoduje_klase_kalkulatora() {
        let decoded = instr([0.1, 0.9, 0.0, 0.0], 0.0, 1.0).unwrap();
        assert_eq!(decoded.mnemonic, "CLASS");
        assert_eq!(decoded.text, "class CalculatorWindow : Window");
    }

    #[test]
    fn dekoduje_przycisk_z_etykieta_i_pozycja() {
        let decoded = instr([0.0, 0.7, 0.7, 0.0], 3.0, 12.0).unwrap();
        assert_eq!(decoded.mnemonic, "BUTTON");
        assert!(decoded.text.contains("new Button()"));
        assert!(decoded.text.contains("(10, 60)"));
    }

    #[test]
    fn dopasowanie_jest_niezmiennicze_na_skale() {
        let decoded = instr([0.005, 0.045, 0.0, 0.0], 0.0, 1.0).unwrap();
        assert_eq!(decoded.mnemonic, "CLASS");
    }

    #[test]
    fn niska_pewnosc_odrzuca_wzorzec() {
        let low = BeefDecoder::decode_to_beef(&[0.1, 0.9, 0.0, 0.0], 0.0, 1.0, 0.5);
        assert!(low.is_none());
    }

    #[test]
    fn obcy_wzorzec_jest_odrzucany() {
        let foreign = BeefDecoder::decode_to_beef(&[-1.0, -1.0, -1.0, -1.0], 0.0, 1.0, 0.99);
        assert!(foreign.is_none());
    }
}

