//! Silnik tekstu: pomiar, kształtowanie (`cosmic-text`) i rasteryzacja glifów.
//!
//! Warstwa cache’uje `cosmic_text::Buffer` per (tekst, styl, szerokość), więc
//! powtarzalne klatki nie kształtują tekstu od nowa, a rasteryzacja glifów
//! trafia do atlasu tylko raz dla danej kombinacji (font, rozmiar, pozycja).

use cosmic_text::{Attrs, Buffer, FontSystem, SwashCache, SwashContent};
use std::collections::HashMap;

use crate::geometry::Size;

use super::atlas::{AtlasSlot, ShelfAllocator};
use super::paint::{TextLine, TextMetrics, TextStyle};

/// Błąd przygotowania silnika tekstu.
#[derive(Debug)]
pub enum TextError {
    /// `cosmic-text` zwrócił błąd podczas inicjalizacji bazy czcionek.
    FontSystem(String),
}

/// Klucz cache'u układu tekstu.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    size_bits: u32,
    line_height_bits: u32,
    weight: u16,
    max_width_bits: u32,
    family: Option<String>,
}

impl LayoutKey {
    fn new(text: &str, style: &TextStyle, max_width: Option<f32>) -> Self {
        Self {
            text: text.to_owned(),
            size_bits: style.size.to_bits(),
            line_height_bits: style.line_height.to_bits(),
            weight: style.weight,
            max_width_bits: max_width.map(f32::to_bits).unwrap_or(0),
            family: style.family.clone(),
        }
    }
}

/// Wpisy gotowego glifu do atlasu.
#[derive(Debug, Clone, Copy)]
pub struct GlyphPlacement {
    /// Prostokąt glifu w pikselach fizycznych (x, y, w, h).
    pub rect: [f32; 4],
    /// Zakres tekstury atlasu (u0, v0, u1, v1).
    pub uv: [f32; 4],
}

/// Gotowy blok tekstu do narysowania.
#[derive(Debug, Default)]
pub struct ShapedText {
    /// Glify w kolejności rysowania.
    pub glyphs: Vec<GlyphPlacement>,
    /// Szerokość najszerszej linii (piksele logiczne).
    pub width: f32,
    /// Wysokość całego bloku (piksele logiczne).
    pub height: f32,
}

/// Silnik tekstu — właściciel bazy czcionek, cache i atlasu.
pub struct TextEngine {
    font_system: FontSystem,
    swash_cache: SwashCache,
    layouts: HashMap<LayoutKey, (Buffer, TextMetrics)>,
    order: Vec<LayoutKey>,
    atlas: wgpu::Texture,
    atlas_view: wgpu::TextureView,
    atlas_allocator: ShelfAllocator,
    atlas_dirty: bool,
    scale_factor: f32,
    cache_capacity: usize,
}

impl std::fmt::Debug for TextEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextEngine")
            .field("cached_layouts", &self.layouts.len())
            .field("atlas_usage", &self.atlas_allocator.usage())
            .field("scale_factor", &self.scale_factor)
            .finish()
    }
}

impl TextEngine {
    /// Rozmiar tekstury atlasu w pikselach.
    pub const ATLAS_SIZE: u32 = 2048;

    /// Tworzy silnik tekstu dla zadanego urządzenia GPU.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scale_factor: f32,
    ) -> Result<Self, TextError> {
        let mut font_system = FontSystem::new();
        // Font ikon Lucide. Bez niego glify z Unicode Private Use Area
        // nie mają czego renderować i ikony wychodzą puste.
        font_system
            .db_mut()
            .load_font_data(lucide_icons::LUCIDE_FONT_BYTES.to_vec());
        let size = Self::ATLAS_SIZE;
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aurola.glyph-atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Zerowanie tekstury — glify trafiają na przezroczyste tło.
        let zeros = vec![0u8; (size * size * 4) as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &zeros,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );

        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self {
            font_system,
            swash_cache: SwashCache::new(),
            layouts: HashMap::new(),
            order: Vec::new(),
            atlas,
            atlas_view,
            atlas_allocator: ShelfAllocator::new(size, 1),
            atlas_dirty: false,
            scale_factor,
            cache_capacity: 512,
        })
    }

    /// Ustawia współczynnik skalowania (do rasteryzacji w skali ekranu).
    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        if (scale_factor - self.scale_factor).abs() > f32::EPSILON {
            self.scale_factor = scale_factor;
            self.clear_cache();
        }
    }

    /// Współczynnik skalowania.
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Czy atlas wymaga ponownego załadowania na GPU.
    pub fn atlas_dirty(&self) -> bool {
        self.atlas_dirty
    }

    /// Zaznacza atlas jako zaktualizowany.
    pub fn clear_dirty(&mut self) {
        self.atlas_dirty = false;
    }

    /// Widok tekstury atlasu (do bind grupy).
    pub fn atlas_view(&self) -> &wgpu::TextureView {
        &self.atlas_view
    }

    /// Czyści cache układów (np. po zmianie motywu lub DPI).
    pub fn clear_cache(&mut self) {
        self.layouts.clear();
        self.order.clear();
    }

    /// Zwalnia wszystkie zasoby GPU.
    pub fn destroy(&mut self) {
        self.atlas.destroy();
    }

    /// Buduje (lub pobiera z cache) `cosmic_text::Buffer` dla zadanego tekstu.
    /// Zwraca `true`, gdy bufor był już w cache.
    fn ensure_buffer(
        &mut self,
        key: &LayoutKey,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> bool {
        if self.layouts.contains_key(key) {
            return true;
        }

        let metrics = cosmic_text::Metrics::new(style.size, style.line_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_wrap(&mut self.font_system, cosmic_text::Wrap::WordOrGlyph);
        buffer.set_size(&mut self.font_system, max_width, None);

        let attrs = Attrs {
            family: style
                .family
                .as_deref()
                .map(cosmic_text::Family::Name)
                .unwrap_or(cosmic_text::Family::SansSerif),
            weight: cosmic_text::Weight(style.weight),
            ..Attrs::new()
        };
        buffer.set_text(
            &mut self.font_system,
            &key.text,
            &attrs,
            cosmic_text::Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let measured = self.measure_buffer(&buffer, style);
        self.insert_layout(key.clone(), buffer, measured);
        false
    }

    fn insert_layout(&mut self, key: LayoutKey, buffer: Buffer, metrics: TextMetrics) {
        if self.layouts.len() >= self.cache_capacity && !self.layouts.contains_key(&key) {
            // Proste wyrównanie LRU: zdejmij najstarszy wpis.
            if let Some(oldest) = self.order.first().cloned() {
                self.layouts.remove(&oldest);
                self.order.remove(0);
            }
        }
        self.order.push(key.clone());
        self.layouts.insert(key, (buffer, metrics));
    }

    /// Mierzy blok tekstu.
    ///
    /// Wynik jest cache’owany — kolejne klatki z tym samym tekstem i stylem
    /// nie powodują ponownego kształtowania.
    pub fn measure(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextMetrics {
        let key = LayoutKey::new(text, style, max_width);
        if let Some((_, metrics)) = self.layouts.get(&key) {
            return metrics.clone();
        }
        self.ensure_buffer(&key, style, max_width);
        self.layouts
            .get(&key)
            .map(|(_, metrics)| metrics.clone())
            .unwrap_or_else(TextMetrics::empty)
    }

    /// Wyciąga wymiary i zakresy linii z gotowego bufora.
    fn measure_buffer(&mut self, buffer: &Buffer, style: &TextStyle) -> TextMetrics {
        let total_len = text_len(buffer);
        let mut lines: Vec<TextLine> = Vec::new();
        let mut width: f32 = 0.0;
        let mut height: f32 = 0.0;

        for run in buffer.layout_runs() {
            let line_width = run.glyphs.iter().map(|g| g.x + g.w).fold(0.0_f32, f32::max);
            width = width.max(line_width);
            height = (run.line_top + style.line_height).max(height);
            lines.push(TextLine {
                range: 0..total_len,
                width: line_width,
                height: style.line_height,
            });
        }

        if lines.is_empty() {
            lines.push(TextLine {
                range: 0..total_len,
                width: 0.0,
                height: style.line_height,
            });
            height = style.line_height;
        }

        TextMetrics {
            size: Size::new(width, height),
            lines,
            overflowed: false,
        }
    }

    /// Kształtuje tekst i zwraca listę glifów gotowych do narysowania.
    ///
    /// `origin` to pozycja lewego górnego rogu bloku w pikselach logicznych.
    pub fn shape(
        &mut self,
        queue: &wgpu::Queue,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        origin: crate::geometry::Point,
    ) -> ShapedText {
        let key = LayoutKey::new(text, style, max_width);
        self.ensure_buffer(&key, style, max_width);

        let Self {
            font_system,
            swash_cache,
            layouts,
            atlas_allocator,
            atlas_dirty,
            scale_factor,
            atlas,
            ..
        } = self;

        let Some((buffer, _)) = layouts.get_mut(&key) else {
            return ShapedText::default();
        };

        let mut shaped = ShapedText::default();
        let scale = *scale_factor;
        let origin_px = (origin.x * scale, origin.y * scale);
        let texture_size = TextEngine::ATLAS_SIZE as f32;

        for run in buffer.layout_runs() {
            shaped.width = shaped.width.max(run.line_w);
            shaped.height = shaped.height.max(run.line_y + style.line_height);

            for glyph in run.glyphs {
                if glyph.font_size <= 0.0 {
                    continue;
                }
                let physical = glyph.physical(origin_px, scale);
                let Some(image) = swash_cache.get_image(font_system, physical.cache_key) else {
                    continue;
                };
                let placement = image.placement;
                if placement.width == 0 || placement.height == 0 {
                    continue;
                }

                // Mask coverage trafia do ALFY. `fs_glyph` próbkuje właśnie
                // kanał `.a` atlasu (`.r` byłoby zerowe dla maski), a dla
                // SwashContent::Color kolor leży w `.rgb`.
                let pixels = match image.content {
                    SwashContent::Mask => {
                        let mut rgba =
                            Vec::with_capacity((placement.width * placement.height * 4) as usize);
                        for &coverage in image.data.iter() {
                            rgba.extend_from_slice(&[255, 255, 255, coverage]);
                        }
                        rgba
                    }
                    SwashContent::Color => image.data.to_vec(),
                    _ => continue,
                };

                let Some(slot) = atlas_allocator.allocate(placement.width, placement.height) else {
                    // Atlas pełny — kończymy zamiast panikować.
                    break;
                };
                *atlas_dirty = true;
                write_glyph(
                    queue,
                    atlas,
                    slot,
                    &pixels,
                    placement.width,
                    placement.height,
                );

                shaped.glyphs.push(GlyphPlacement {
                    rect: [
                        physical.x as f32 + placement.left as f32,
                        physical.y as f32 - placement.top as f32,
                        placement.width as f32,
                        placement.height as f32,
                    ],
                    uv: [
                        slot.x as f32 / texture_size,
                        slot.y as f32 / texture_size,
                        (slot.x + slot.width) as f32 / texture_size,
                        (slot.y + slot.height) as f32 / texture_size,
                    ],
                });
            }
        }

        shaped
    }
}

/// Wgrywa prostokąt maski glifu do atlasu.
fn write_glyph(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    slot: AtlasSlot,
    pixels: &[u8],
    width: u32,
    height: u32,
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: slot.x,
                y: slot.y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// Łączna długość tekstu w bajtach.
fn text_len(buffer: &Buffer) -> usize {
    buffer.lines.iter().map(|line| line.text().len()).sum()
}
