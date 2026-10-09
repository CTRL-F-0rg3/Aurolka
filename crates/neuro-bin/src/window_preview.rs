//! Software-owy podgląd okna kalkulatora (symulacja zrzutu ekranu).
//!
//! Bez kompilatora Beef ani systemu okien nie da się zrobić prawdziwego
//! zrzutu. Dlatego renderujemy podgląd **programowo**: parsujemy kod Beef
//! (wywołania `AddButton("etykieta", x, y, w, h)`), rysujemy okno,
//! wyświetlacz i siatkę przycisków do bufora RGBA, który można porównać
//! z obrazem referencyjnym `calc.png`.

/// Pojedynczy przycisk wyciągnięty z kodu Beef.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// Etykieta przycisku (np. `"7"`, `"+"`).
    pub label: String,
    /// Lewy górny róg (w pikselach okna).
    pub x: i32,
    /// Lewy górny róg (w pikselach okna).
    pub y: i32,
    /// Szerokość przycisku.
    pub width: i32,
    /// Wysokość przycisku.
    pub height: i32,
}

/// Podgląd okna: piksele RGBA.
pub struct Preview {
    /// Piksele RGBA (`width * height * 4` bajtów).
    pub pixels: Vec<u8>,
}

/// Parsuje kod Beef i wyciąga przyciski `AddButton("label", x, y, w, h)`.
pub fn parse_buttons(code: &str) -> Vec<Button> {
    let mut buttons = Vec::new();
    for line in code.lines() {
        let line = line.trim();
        if !line.starts_with("AddButton(") {
            continue;
        }
        let Some(inner) = line
            .strip_prefix("AddButton(")
            .and_then(|rest| rest.strip_suffix(");"))
        else {
            continue;
        };
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() < 5 {
            continue;
        }
        let label = parts[0].trim_matches('"').to_string();
        let parse = |s: &str| s.parse::<i32>().ok();
        let (Some(x), Some(y), Some(width), Some(height)) = (
            parse(parts[1]),
            parse(parts[2]),
            parse(parts[3]),
            parse(parts[4]),
        ) else {
            continue;
        };
        buttons.push(Button {
            label,
            x,
            y,
            width,
            height,
        });
    }
    buttons
}

/// Rysuje kalkulator z kodu Beef do bufora RGBA.
pub fn render(code: &str, width: u32, height: u32) -> Preview {
    let buttons = parse_buttons(code);
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];

    fill_rect(&mut pixels, width, height, 0, 0, width as i32, height as i32, [40, 40, 44, 255]);
    fill_rect(&mut pixels, width, height, 0, 0, width as i32, 36, [52, 52, 58, 255]);
    fill_rect(&mut pixels, width, height, 20, 52, width as i32 - 40, 84, [225, 225, 228, 255]);
    draw_text(&mut pixels, width, height, "0", 34, 70, 5, [40, 40, 44, 255]);

    for button in &buttons {
        let color = if is_operator(&button.label) {
            [250, 150, 40, 255]
        } else {
            [96, 96, 104, 255]
        };
        fill_rect(
            &mut pixels,
            width,
            height,
            button.x,
            button.y,
            button.x + button.width,
            button.y + button.height,
            color,
        );
        let scale = 6;
        let glyph_w = 3 * scale;
        let label_px = button.label.chars().count() as i32 * glyph_w;
        let text_x = button.x + (button.width - label_px) / 2;
        let text_y = button.y + (button.height - 5 * scale) / 2;
        draw_text(
            &mut pixels,
            width,
            height,
            &button.label,
            text_x,
            text_y,
            scale,
            [245, 245, 248, 255],
        );
    }

    Preview { pixels }
}


/// Czy etykieta to operator (wtedy przycisk jest pomarańczowy).
fn is_operator(label: &str) -> bool {
    matches!(label, "+" | "-" | "*" | "/" | "=" | "%" | "C" | "(" | ")")
}

/// Wypełnia prostokąt kolorem (z przycięciem do bufora).
fn fill_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: [u8; 4],
) {
    let x0 = x0.max(0) as u32;
    let y0 = y0.max(0) as u32;
    let x1 = (x1 as u32).min(width);
    let y1 = (y1 as u32).min(height);
    for y in y0..y1 {
        for x in x0..x1 {
            let index = ((y * width + x) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&color);
        }
    }
}

/// Rysuje tekst bitmapową czcionką 3×5 (skalowaną).
fn draw_text(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    text: &str,
    start_x: i32,
    start_y: i32,
    scale: i32,
    color: [u8; 4],
) {
    let mut cursor = start_x;
    for character in text.chars() {
        let glyph = glyph(character);
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << column) != 0 {
                    let x0 = cursor + column * scale;
                    let y0 = start_y + row as i32 * scale;
                    fill_rect(pixels, width, height, x0, y0, x0 + scale, y0 + scale, color);
                }
            }
        }
        cursor += 4 * scale;
    }
}

/// Glyph 3×5 dla znaku (każdy wiersz to 3 bity, bit 0 = lewa kolumna).
fn glyph(character: char) -> [u8; 5] {
    match character {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '*' => [0b101, 0b010, 0b101, 0b000, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '=' => [0b000, 0b111, 0b000, 0b111, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b010, 0b000],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        '(' => [0b001, 0b010, 0b010, 0b010, 0b001],
        ')' => [0b100, 0b010, 0b010, 0b010, 0b100],
        _ => [0b111, 0b111, 0b111, 0b111, 0b111],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsuje_przyciski_z_kodu() {
        let code = "AddButton(\"7\", 20, 100, 135, 60);\nAddButton(\"+\", 485, 400, 135, 60);";
        let buttons = parse_buttons(code);
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0].label, "7");
        assert_eq!(buttons[0].x, 20);
        assert_eq!(buttons[1].label, "+");
        assert_eq!(buttons[1].height, 60);
    }

    #[test]
    fn ignoruje_nieprzyciskowe_linie() {
        let code = "void RenderUI() {\nAddButton(\"C\", 1, 2, 3, 4);\n}";
        assert_eq!(parse_buttons(code).len(), 1);
    }

    #[test]
    fn render_daje_bufor_o_dlugosci_wxhx4() {
        let code = "AddButton(\"7\", 0, 0, 10, 10);";
        let preview = render(code, 64, 64);
        assert_eq!(preview.pixels.len(), 64 * 64 * 4);
    }
}
