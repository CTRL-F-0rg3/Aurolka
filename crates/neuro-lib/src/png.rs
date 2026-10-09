//! Minimalny dekoder PNG (bez zewnętrznych zależności) do celów wizualnych.
//!
//! Obsługuje dokładnie tyle, ile potrzebne do wczytania obrazu referencyjnego
//! zadania: 8-bitowe kolory (szarość, RGB, indeksowane, szarość+alfa, RGBA),
//! brak przeplotu, kompresję `zlib` (inflate) i pięć filtrów wierszowych PNG.
//!
//! Wynik to zawsze bufor RGBA 8-bit — jeden, wspólny format dla shadera.

/// Obraz wczytany z pliku PNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngImage {
    /// Szerokość w pikselach.
    pub width: u32,
    /// Wysokość w pikselach.
    pub height: u32,
    /// Piksele w formacie RGBA, `width * height * 4` bajtów.
    pub pixels: Vec<u8>,
}

/// Rodzaj koloru z nagłówka IHDR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorType {
    /// 0 — szarość, 1 kanał.
    Gray = 0,
    /// 2 — RGB, 3 kanały.
    Rgb = 2,
    /// 3 — paleta, 1 kanał (indeks).
    Palette = 3,
    /// 4 — szarość + alfa, 2 kanały.
    GrayAlpha = 4,
    /// 6 — RGBA, 4 kanały.
    Rgba = 6,
}

impl ColorType {
    /// Rozpoznaje typ koloru z bajtu IHDR.
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Gray),
            2 => Some(Self::Rgb),
            3 => Some(Self::Palette),
            4 => Some(Self::GrayAlpha),
            6 => Some(Self::Rgba),
            _ => None,
        }
    }

    /// Liczba kanałów w pikselu.
    fn channels(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
            Self::Palette => 1,
            Self::GrayAlpha => 2,
            Self::Rgba => 4,
        }
    }
}

/// Wczytuje obraz PNG z pliku.
pub fn load_png(path: impl AsRef<std::path::Path>) -> Result<PngImage, String> {
    let bytes = std::fs::read(path.as_ref())
        .map_err(|error| format!("nie udało się wczytać {}: {error}", path.as_ref().display()))?;
    decode_png(&bytes)
}

/// Dekoduje obraz PNG z bajtów w pamięci.
pub fn decode_png(bytes: &[u8]) -> Result<PngImage, String> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err("to nie jest plik PNG (zła sygnatura)".into());
    }

    // --- Zbieranie chunków: IHDR, sklejone IDAT, PLTE ---
    let mut position = 8;
    let mut header: Option<(u32, u32, u8, ColorType)> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut palette: Vec<[u8; 3]> = Vec::new();

    while position + 8 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[position],
            bytes[position + 1],
            bytes[position + 2],
            bytes[position + 3],
        ]) as usize;
        let kind = &bytes[position + 4..position + 8];
        let start = position + 8;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "uszkodzony chunk PNG (długość poza plikiem)".to_string())?;
        let data = &bytes[start..end];

        match (kind[0], kind[1], kind[2], kind[3]) {
            (b'I', b'H', b'D', b'R') => {
                if data.len() < 13 {
                    return Err("chunk IHDR za krótki".into());
                }
                let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                let bit_depth = data[8];
                let color_type = ColorType::from_byte(data[9])
                    .ok_or_else(|| format!("nieobsługiwany typ koloru {}", data[9]))?;
                if data[12] != 0 {
                    return Err("przeplot (interlace) nie jest obsługiwany".into());
                }
                if bit_depth != 8 {
                    return Err(format!("obsługiwana jest tylko głębia 8 bitów, nie {bit_depth}"));
                }
                header = Some((width, height, bit_depth, color_type));
            }
            (b'I', b'D', b'A', b'T') => idat.extend_from_slice(data),
            (b'P', b'L', b'T', b'E') => {
                for chunk in data.chunks_exact(3) {
                    palette.push([chunk[0], chunk[1], chunk[2]]);
                }
            }
            (b'I', b'E', b'N', b'D') => break,
            _ => {} // tEXT, pHYs, sRGB… — ignorujemy
        }
        position = end + 4; // + CRC
    }

    let (width, height, _depth, color_type) =
        header.ok_or_else(|| "brak nagłówka IHDR w pliku PNG".to_string())?;
    if width == 0 || height == 0 {
        return Err("obraz PNG ma zerowy wymiar".into());
    }

    let raw = inflate_zlib(&idat)?;
    unfilter(&raw, width, height, color_type, &palette)
}

/// Rozpakowuje strumień `zlib` (nagłówek 2 bajty + bloki DEFLATE).
fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 2 {
        return Err("strumień zlib za krótki".into());
    }
    // Pierwszy bajt: CM=8 (deflate), CMF/FLG z sumą kontrolną wielokrotnością 31.
    if data[0] & 0x0F != 8 {
        return Err(format!("nieobsługiwana metoda kompresji {}", data[0] & 0x0F));
    }
    let mut inflater = Inflater::new(&data[2..]);
    inflater.run()
}

/// Krótki, iteratywny inflate (RFC 1951): stored + Huffman ustalony i dynamiczny.
struct Inflater<'a> {
    input: &'a [u8],
    /// Bieżąca pozycja bitowa w strumieniu wejściowym.
    bit_pos: usize,
    output: Vec<u8>,
}

/// Węzeł drzewa Huffmana (budowane dynamicznie z listy długości kodów).
#[derive(Clone, Copy)]
enum Huffman {
    /// Liść: symbol.
    Leaf(u16),
    /// Gałązka: indeksy lewy/prawy w tablicy węzłów (`0` = brak).
    Branch(usize, usize),
}

/// Drzewo Huffmana: korzeń plus pula węzłów, do których odwołują się gałązki.
struct HuffmanTree {
    /// Węzeł startowy dekodowania.
    root: Huffman,
    /// Pula węzłów (indeksy w [`Huffman::Branch`]).
    nodes: Vec<Huffman>,
}

impl<'a> Inflater<'a> {
    /// Nowy inflater na danych bez nagłówka zlib.
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            bit_pos: 0,
            output: Vec::new(),
        }
    }

    /// Czyta jeden bit.
    fn bit(&mut self) -> Result<u32, String> {
        let byte_index = self.bit_pos / 8;
        if byte_index >= self.input.len() {
            return Err("koniec strumienia DEFLATE w środku danych".into());
        }
        let bit = (self.input[byte_index] >> (self.bit_pos % 8)) & 1;
        self.bit_pos += 1;
        Ok(u32::from(bit))
    }

    /// Czyta `count` bitów, LSB-first.
    fn bits(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0u32;
        for index in 0..count {
            value |= self.bit()? << index;
        }
        Ok(value)
    }

    /// Wyrównuje pozycję do pełnego bajtu.
    fn align_byte(&mut self) {
        self.bit_pos = self.bit_pos.div_ceil(8) * 8;
    }

    /// Wykonuje dekompresję i zwraca surowe bajty.
    fn run(&mut self) -> Result<Vec<u8>, String> {
        loop {
            let last = self.bit()?;
            let kind = self.bits(2)?;

            match kind {
                0 => self.stored_block()?,
                1 => self.huffman_block(&fixed_lit_tree(), &fixed_dist_tree())?,
                2 => {
                    let (lit, dist) = self.dynamic_trees()?;
                    self.huffman_block(&lit, &dist)?;
                }
                other => return Err(format!("nieznany typ bloku DEFLATE {other}")),
            }

            if last == 1 {
                return Ok(std::mem::take(&mut self.output));
            }
        }
    }

    /// Blok nieskompresowany (`BTYPE = 00`).
    fn stored_block(&mut self) -> Result<(), String> {
        self.align_byte();
        let start = self.bit_pos / 8;
        if start + 4 > self.input.len() {
            return Err("nagłówek bloku stored poza strumieniem".into());
        }
        let length = u16::from_le_bytes([self.input[start], self.input[start + 1]]) as usize;
        let data_start = start + 4;
        let data_end = data_start
            .checked_add(length)
            .filter(|end| *end <= self.input.len())
            .ok_or_else(|| "blok stored dłuższy niż strumień".to_string())?;
        self.output.extend_from_slice(&self.input[data_start..data_end]);
        self.bit_pos = data_end * 8;
        Ok(())
    }

    /// Blok z kodowaniem Huffmana (`BTYPE = 01` lub `10`).
    fn huffman_block(&mut self, lit: &HuffmanTree, dist: &HuffmanTree) -> Result<(), String> {
        loop {
            let symbol = self.decode_symbol(lit)?;

            if symbol < 256 {
                self.output.push(symbol as u8);
            } else if symbol == 256 {
                return Ok(());
            } else {
                // Długość kopii z tablicy DEFLATE (symbole 257..285).
                let (base, extra_bits) = LENGTH_TABLE[(symbol - 257) as usize];
                let length = base as usize + self.bits(extra_bits)? as usize;

                let distance_symbol = self.decode_symbol(dist)?;
                let (dist_base, dist_bits) = DISTANCE_TABLE[distance_symbol as usize];
                let distance = dist_base as usize + self.bits(dist_bits)? as usize;

                if distance == 0 || distance > self.output.len() {
                    return Err("odległość kopii większa niż dotychczasowy wynik".into());
                }
                let start = self.output.len() - distance;
                for offset in 0..length {
                    let byte = self.output[start + offset];
                    self.output.push(byte);
                }
            }
        }
    }

    /// Dekoduje jeden symbol drzewem Huffmana.
    fn decode_symbol(&mut self, tree: &HuffmanTree) -> Result<u16, String> {
        let mut node = tree.root;
        loop {
            match node {
                Huffman::Leaf(symbol) => return Ok(symbol),
                Huffman::Branch(zero, one) => {
                    node = tree.nodes[if self.bit()? == 0 { zero } else { one }];
                }
            }
        }
    }

    /// Czyta drzewa dynamiczne z nagłówka bloku (`BTYPE = 10`).
    fn dynamic_trees(&mut self) -> Result<(HuffmanTree, HuffmanTree), String> {
        let lit_count = self.bits(5)? as usize + 257;
        let dist_count = self.bits(5)? as usize + 1;
        let code_count = self.bits(4)? as usize + 4;

        // Długości kodów alfabetu długości (z stałej kolejności).
        const ORDER: [usize; 19] = [
            16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
        ];
        let mut code_lengths = [0u8; 19];
        for slot in ORDER.iter().take(code_count) {
            code_lengths[*slot] = self.bits(3)? as u8;
        }
        let length_tree = build_tree(&code_lengths)?;

        // Rozpakowanie długości kodów literalnych i odległości.
        let total = lit_count + dist_count;
        let mut lengths: Vec<u8> = Vec::with_capacity(total);
        while lengths.len() < total {
            let symbol = self.decode_symbol(&length_tree)?;
            match symbol {
                0..=15 => lengths.push(symbol as u8),
                16 => {
                    let previous = *lengths.last().ok_or_else(|| "powtórka bez poprzednika".to_string())?;
                    let repeat = 3 + self.bits(2)? as usize;
                    lengths.extend(std::iter::repeat(previous).take(repeat));
                }
                17 => {
                    let repeat = 3 + self.bits(3)? as usize;
                    lengths.extend(std::iter::repeat(0u8).take(repeat));
                }
                18 => {
                    let repeat = 11 + self.bits(7)? as usize;
                    lengths.extend(std::iter::repeat(0u8).take(repeat));
                }
                other => return Err(format!("zły symbol długości kodu {other}")),
            }
            if lengths.len() > total {
                return Err("za dużo długości kodów w bloku dynamicznym".into());
            }
        }

        let lit = build_tree(&lengths[..lit_count])?;
        let dist = build_tree(&lengths[lit_count..])?;
        Ok((lit, dist))
    }
}

/// Bazy długości i liczba bitów dodatkowych (symbole 257..285).
const LENGTH_TABLE: [(u16, u32); 29] = [
    (3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0), (11, 1), (13, 1), (15, 1),
    (17, 1), (19, 2), (23, 2), (27, 2), (31, 2), (35, 3), (43, 3), (51, 3), (59, 3), (67, 4),
    (83, 4), (99, 4), (115, 4), (131, 5), (163, 5), (195, 5), (227, 5), (258, 0),
];

/// Bazy odległości i liczba bitów dodatkowych (symbole 0..29).
const DISTANCE_TABLE: [(u16, u32); 30] = [
    (1, 0), (2, 0), (3, 0), (4, 0), (5, 1), (7, 1), (9, 2), (13, 2), (17, 3), (25, 3), (33, 4),
    (49, 4), (65, 5), (97, 5), (129, 6), (193, 6), (257, 7), (385, 7), (513, 8), (769, 8),
    (1025, 9), (1537, 9), (2049, 10), (3073, 10), (4097, 11), (6145, 11), (8193, 12), (12289, 12),
    (16385, 13), (24577, 13),
];

/// Buduje drzewo Huffmana z długości kodów (kod kanoniczny, RFC 1951).
///
/// Drzewo budujemy od korzenia: każdy kod wpisujemy bit po bicie,
/// tworząc po drodze brakujące gałązki.
fn build_tree(lengths: &[u8]) -> Result<HuffmanTree, String> {
    // Liczba kodów na każdą długość (0..=15).
    let mut counts = [0u16; 16];
    for &length in lengths {
        if length as usize >= counts.len() {
            return Err("długość kodu Huffmana większa niż 15".into());
        }
        counts[length as usize] += 1;
    }
    counts[0] = 0; // symbole bez kodu nie wchodzą do drzewa

    // Pierwszy kod dla każdej długości (kod kanoniczny).
    let mut next_code = [0u32; 16];
    let mut code = 0u32;
    for bits in 1..16 {
        code = (code + u32::from(counts[bits - 1])) << 1;
        next_code[bits] = code;
    }

    let mut nodes: Vec<Huffman> = vec![Huffman::Leaf(0)]; // węzeł 0 = korzeń

    for (symbol, &length) in lengths.iter().enumerate() {
        if length == 0 {
            continue;
        }
        let bits = length as u32;
        let symbol_code = next_code[length as usize];
        next_code[length as usize] += 1;

        // Spacer od korzenia; każdy krok to jeden bit kodu (MSB-first).
        let mut current = 0usize;
        for depth in (0..bits).rev() {
            let bit = (symbol_code >> depth) & 1;
            match nodes[current] {
                Huffman::Leaf(_) => {
                    // Zamieniamy placeholder na gałązkę z dwoma dziećmi.
                    let zero = nodes.len();
                    let one = nodes.len() + 1;
                    nodes.push(Huffman::Leaf(0));
                    nodes.push(Huffman::Leaf(0));
                    nodes[current] = Huffman::Branch(zero, one);
                }
                Huffman::Branch(_, _) => {}
            }

            let (zero, one) = match nodes[current] {
                Huffman::Branch(zero, one) => (zero, one),
                Huffman::Leaf(_) => unreachable!("gałązka utworzona powyżej"),
            };
            current = if bit == 0 { zero } else { one };
        }

        if let Huffman::Branch(_, _) = nodes[current] {
            return Err("kod Huffmana jest prefiksem innego kodu".into());
        }
        nodes[current] = Huffman::Leaf(symbol as u16);
    }

    Ok(HuffmanTree {
        root: nodes[0],
        nodes,
    })
}

/// Drzewo Huffmana ustalone (`BTYPE = 01`).
fn fixed_lit_tree() -> HuffmanTree {
    let mut lengths = [0u8; 288];
    for (symbol, length) in lengths.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    build_tree(&lengths).expect("stałe drzewo Huffmana jest poprawne")
}

/// Drzewo odległości ustalone (`BTYPE = 01`) — 5 bitów dla 30 symboli.
fn fixed_dist_tree() -> HuffmanTree {
    build_tree(&[5u8; 30]).expect("stałe drzewo odległości jest poprawne")
}

/// Cofnięcie filtrów wierszowych PNG i zamiana na RGBA.
fn unfilter(
    raw: &[u8],
    width: u32,
    height: u32,
    color: ColorType,
    palette: &[[u8; 3]],
) -> Result<PngImage, String> {
    let channels = color.channels();
    let stride = width as usize * channels; // bajty pikseli w wierszu (bez filtra)
    let expected = (stride + 1) * height as usize; // + 1 bajt filtra na wiersz

    if raw.len() != expected {
        return Err(format!(
            "długość danych obrazu {} ≠ oczekiwana {expected}",
            raw.len()
        ));
    }

        let mut pixels = vec![0u8; stride * height as usize];

    for y in 0..height as usize {
        let filter = raw[y * (stride + 1)];
        let row_start = y * (stride + 1) + 1;
        let row = &raw[row_start..row_start + stride];

        // Dzielimy bufor na wiersze powyżej i wiersz bieżący,
        // żeby pożyczki nie kolidowały. Wiersz powyżej kopiujemy,
        // bo `split_at_mut` odciąga mutable borrow na `current`.
        let (done, current) = pixels.split_at_mut(y * stride);
        let current = &mut current[..stride];
        let previous_row: Vec<u8> = if y == 0 {
            Vec::new()
        } else {
            done[(y - 1) * stride..(y - 1) * stride + stride].to_vec()
        };

        match filter {
            0 => current.copy_from_slice(row), // None
            1 => {
                // Sub: z lewego sąsiada
                for index in 0..stride {
                    let left = if index >= channels { current[index - channels] } else { 0 };
                    current[index] = row[index].wrapping_add(left);
                }
            }
            2 => {
                // Up: z wiersza powyżej
                for index in 0..stride {
                    let up = if y == 0 { 0 } else { previous_row[index] };
                    current[index] = row[index].wrapping_add(up);
                }
            }
            3 => {
                // Average: średnia z lewego i górnego
                for index in 0..stride {
                    let left = if index >= channels { current[index - channels] as u32 } else { 0 };
                    let up = if y == 0 { 0 } else { previous_row[index] as u32 };
                    current[index] = row[index].wrapping_add(((left + up) / 2) as u8);
                }
            }
            4 => {
                // Paeth: predyktor Paeth
                for index in 0..stride {
                    let left = if index >= channels { current[index - channels] as i32 } else { 0 };
                    let up = if y == 0 { 0 } else { previous_row[index] as i32 };
                    let upper_left = if y > 0 && index >= channels {
                        previous_row[index - channels] as i32
                    } else {
                        0
                    };
                    current[index] = row[index].wrapping_add(paeth(left, up, upper_left) as u8);
                }
            }
            other => return Err(format!("nieznany filtr wiersza PNG {other}")),
        }
    }

    to_rgba(&pixels, width, height, color, palette)
}

/// Predyktor Paeth (filtr 4).
fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Zamienia wiersze w rodzimym formacie na jednolity RGBA.
fn to_rgba(
    pixels: &[u8],
    width: u32,
    height: u32,
    color: ColorType,
    palette: &[[u8; 3]],
) -> Result<PngImage, String> {
    let channels = color.channels();
    let count = width as usize * height as usize;
    let mut rgba = vec![0u8; count * 4];

    for index in 0..count {
        let pixel = &pixels[index * channels..index * channels + channels];
        let out = &mut rgba[index * 4..index * 4 + 4];
        match color {
            ColorType::Gray => {
                out[0] = pixel[0];
                out[1] = pixel[0];
                out[2] = pixel[0];
                out[3] = 255;
            }
            ColorType::GrayAlpha => {
                out[0] = pixel[0];
                out[1] = pixel[0];
                out[2] = pixel[0];
                out[3] = pixel[1];
            }
            ColorType::Rgb => {
                out[..3].copy_from_slice(pixel);
                out[3] = 255;
            }
            ColorType::Rgba => out.copy_from_slice(pixel),
            ColorType::Palette => {
                let entry = palette
                    .get(pixel[0] as usize)
                    .ok_or_else(|| format!("indeks palety {} poza zakresem", pixel[0]))?;
                out[..3].copy_from_slice(entry);
                out[3] = 255;
            }
        }
    }

    Ok(PngImage {
        width,
        height,
        pixels: rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Buduje minimalny, nieskompresowany PNG 1×1 RGBA (filtr 0).
    fn tiny_rgba_png(pixel: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&1u32.to_be_bytes()); // width
        ihdr.extend_from_slice(&1u32.to_be_bytes()); // height
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, brak filtra/entropii, brak przeplotu
        push_chunk(&mut out, b"IHDR", &ihdr);

        // Dane: filtr 0 + piksel; kompresja zlib "stored" (BTYPE = 00).
        let raw = [0u8, pixel[0], pixel[1], pixel[2], pixel[3]];
        let mut zlib = vec![0x78, 0x01]; // CMF/FLG — deflate, brak słownika
        zlib.push(0x01); // BFINAL = 1, BTYPE = 00 (stored)
        zlib.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw);
        push_chunk(&mut out, b"IDAT", &zlib);

        push_chunk(&mut out, b"IEND", &[]);
        out
    }

    fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc_input = Vec::with_capacity(4 + data.len());
        crc_input.extend_from_slice(kind);
        crc_input.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }

    /// Prosty CRC-32 (poly 0xEDB88320) — tylko do składania pliku testowego.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn dekoduje_jednopikselowy_rgba() {
        let image = decode_png(&tiny_rgba_png([10, 20, 30, 255])).unwrap();
        assert_eq!(image.width, 1);
        assert_eq!(image.height, 1);
        assert_eq!(image.pixels, vec![10, 20, 30, 255]);
    }

    #[test]
    fn odrzuca_nie_png() {
        assert!(decode_png(b"nie-obraz").is_err());
    }

    #[test]
    fn odrzuca_glebokosc_inna_niz_8() {
        let mut png = tiny_rgba_png([0, 0, 0, 255]);
        // IHDR zaczyna się po 8 bajtach sygnatury + 4 bajty długości + 4 bajty typu
        png[16] = 16; // bit depth = 16
        assert!(decode_png(&png).is_err());
    }

    #[test]
    fn inflate_rozpakowuje_stored() {
        // Nagłówek zlib + jeden blok stored z tekstem "hello".
        let payload = b"hello";
        let mut stream = vec![0x78, 0x01, 0x01];
        stream.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        stream.extend_from_slice(&(!(payload.len() as u16)).to_le_bytes());
        stream.extend_from_slice(payload);

        assert_eq!(inflate_zlib(&stream).unwrap(), payload);
    }

    #[test]
    fn filtry_wierszowe_dzialaja() {
        // Dwa wiersze 2 pikseli szarych; filtr Sub na drugim wierszu.
        let width = 2u32;
        let height = 2u32;
        let raw: Vec<u8> = vec![
            0, 10, 20, // wiersz 0, filtr None → [10, 20]
            1, 10, 5, // wiersz 1, filtr Sub → [10, 10+5=15]
        ];
        let image = unfilter(&raw, width, height, ColorType::Gray, &[]).unwrap();
        assert_eq!(image.width, 2);
        assert_eq!(image.height, 2);
        // Każdy piksel szary staje się R=G=B, alfa 255.
        assert_eq!(
            image.pixels,
            vec![
                10, 10, 10, 255, //
                20, 20, 20, 255, //
                10, 10, 10, 255, //
                15, 15, 15, 255,
            ]
        );
    }
}

