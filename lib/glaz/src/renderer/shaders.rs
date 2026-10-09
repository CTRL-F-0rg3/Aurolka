//! Shadery WGSL używane przez renderer.
//!
//! Trzy osobne moduły:
//! * [`QUAD_WGSL`] — prostokąty SDF z zaokrągleniami, obramowaniem, cieniem
//!   i próbkowaniem rozmytego tła (backdrop),
//! * [`TEXT_WGSL`] — glify z atlasu kanału alpha,
//! * [`POST_WGSL`] — rozmycie separowalne i kompozycja z maską kształtu okna.
//!
//! Kolory są **premultiplied alpha** — dzięki temu mieszanie w shaderach jest
//! zwykłym dodawaniem, a przezroczystość okna działa poprawnie.

/// Pełny trójkąt pokrywający ekran — dokumentacja do `POST_WGSL` / `COMPOSITE_WGSL`.
///
/// Moduły post-processingu wklejają ten kod do siebie, bo `concat!`
/// przyjmuje wyłącznie literały, więc nie da się użyć tu stałej.
#[allow(dead_code)]
const FULLSCREEN_VS_WGSL: &str = r#"
struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex_index: u32) -> FullscreenOut {
    var out: FullscreenOut;
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    // v = 0 odpowiada górze tekstury (renderujemy z odwróconą osią Y).
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}
"#;

/// Moduł shaderów dla prostokątów.
pub const QUAD_WGSL: &str = concat!(
    r#"
struct Frame {
    // Rozmiar render targetu w pikselach fizycznych.
    resolution: vec2<f32>,
    // Rozmiar pojedynczego texela bufora rozmycia.
    blur_texel: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var backdrop_texture: texture_2d<f32>;

struct QuadIn {
    @location(0) rect: vec4<f32>,
    @location(1) background: vec4<f32>,
    @location(2) foreground: vec4<f32>,
    @location(3) radius: vec4<f32>,
    @location(4) border_width: f32,
    @location(5) shadow_offset: vec2<f32>,
    @location(6) shadow_blur: f32,
    @location(7) shadow_spread: f32,
    @location(8) shadow_color: vec4<f32>,
    @location(9) backdrop_blur: f32,
    @location(10) backdrop_color: vec4<f32>,
    @location(11) backdrop_saturation: f32,
    // Obszar przycięcia w pikselach fizycznych: x0, y0, x1, y1.
    @location(12) clip: vec4<f32>,
};

struct QuadOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) background: vec4<f32>,
    @location(3) foreground: vec4<f32>,
    @location(4) radius: vec4<f32>,
    @location(5) border_width: f32,
    @location(6) shadow_offset: vec2<f32>,
    @location(7) shadow_blur: f32,
    @location(8) shadow_spread: f32,
    @location(9) shadow_color: vec4<f32>,
    @location(10) screen: vec2<f32>,
    @location(11) backdrop_blur: f32,
    @location(12) backdrop_color: vec4<f32>,
    @location(13) backdrop_saturation: f32,
    @location(14) clip: vec4<f32>,
};

/// Odległość od prostokąta ze ściętymi narożnikami (promień `r`).
fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

/// Odległość od prostokąta z niezależnym promieniem każdego narożnika.
/// Kolejność `radius`: top-left, top-right, bottom-right, bottom-left.
fn sd_rounded_box(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    let limit = min(b.x, b.y) * 0.5;
    let rr = clamp(r, vec4<f32>(0.0, 0.0, 0.0, 0.0), vec4<f32>(limit, limit, limit, limit));
    var radius = rr.x;
    if (p.x > 0.0) {
        radius = select(rr.y, rr.z, p.y > 0.0);
    } else {
        radius = select(rr.x, rr.w, p.y > 0.0);
    }
    return sd_round_box(p, b, radius);
}

/// Luminans dla operacji nasycenia.
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@vertex
fn vs_quad(in: QuadIn, @builtin(vertex_index) vertex_index: u32) -> QuadOut {
    var out: QuadOut;

    let rect_size = in.rect.zw;
    let center = in.rect.xy + rect_size * 0.5;

    // Quad musi pomieścić cień, który wychodzi poza rect.
    let padding = max(in.shadow_blur + max(in.shadow_spread, 0.0), 0.0);
    let origin = center - rect_size * 0.5 - vec2<f32>(padding, padding);
    let full_size = rect_size + vec2<f32>(padding, padding) * 2.0;

    let corner = vec2<f32>(
        select(0.0, 1.0, (vertex_index & 1u) == 1u),
        select(0.0, 1.0, (vertex_index & 2u) == 2u),
    );
    let vertex_px = origin + full_size * corner;
    let ndc = vec2<f32>(
        (vertex_px.x / frame.resolution.x) * 2.0 - 1.0,
        1.0 - (vertex_px.y / frame.resolution.y) * 2.0,
    );

    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.local = vertex_px - center;
    out.size = rect_size;
    out.background = in.background;
    out.foreground = in.foreground;
    out.radius = in.radius;
    out.border_width = in.border_width;
    out.shadow_offset = in.shadow_offset;
    out.shadow_blur = in.shadow_blur;
    out.shadow_spread = in.shadow_spread;
    out.shadow_color = in.shadow_color;
    out.screen = vertex_px;
    out.backdrop_blur = in.backdrop_blur;
    out.backdrop_color = in.backdrop_color;
    out.backdrop_saturation = in.backdrop_saturation;
    out.clip = in.clip;
    return out;
}

@fragment
fn fs_quad(in: QuadOut) -> @location(0) vec4<f32> {
    // Przycinanie do stosu clip — tańsze niż osobne draw call'e.
    if (in.screen.x < in.clip.x || in.screen.y < in.clip.y
        || in.screen.x > in.clip.z || in.screen.y > in.clip.w) {
        discard;
    }

    let half_size = in.size * 0.5;
    let distance = sd_rounded_box(in.local, half_size, in.radius);
    let aa = max(fwidth(distance), 0.0001);
    let shape = 1.0 - smoothstep(-aa, aa, distance);

    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);

    // --- Cień (najniższa warstwa) ---
    if (in.shadow_color.a > 0.0) {
        let spread = in.shadow_spread;
        let shadow_distance = sd_rounded_box(
            in.local - in.shadow_offset,
            half_size + vec2<f32>(spread, spread),
            in.radius + vec4<f32>(spread, spread, spread, spread),
        );
        let blur = max(in.shadow_blur, aa);
        let shadow_alpha = (1.0 - smoothstep(-blur, blur, shadow_distance)) * in.shadow_color.a;
        color = vec4<f32>(in.shadow_color.rgb * shadow_alpha, shadow_alpha);
    }

    // --- Rozmyte tło (backdrop / acrylic) ---
    if (in.backdrop_blur > 0.0) {
        let uv = in.screen / frame.resolution;
        let sampled = textureSampleLevel(backdrop_texture, linear_sampler, uv, 0.0);
        let sampled_alpha = max(sampled.a, 0.00001);
        var straight = clamp(sampled.rgb / sampled_alpha, vec3<f32>(0.0), vec3<f32>(1.0));
        if (in.backdrop_saturation != 1.0) {
            let l = luma(straight);
            straight = clamp(
                mix(vec3<f32>(l, l, l), straight, in.backdrop_saturation),
                vec3<f32>(0.0),
                vec3<f32>(1.0),
            );
        }
        straight = mix(straight, in.backdrop_color.rgb, clamp(in.backdrop_color.a, 0.0, 1.0));
        color += vec4<f32>(straight * shape, shape);
    }

    // --- Wypełnienie ---
    if (in.background.a > 0.0) {
        color += vec4<f32>(in.background.rgb * in.background.a, in.background.a) * shape;
    }

    // --- Obramowanie (pierścień przy krawędzi) ---
    if (in.foreground.a > 0.0 && in.border_width > 0.0) {
        let half_border = in.border_width * 0.5;
        let inner = max(half_size - vec2<f32>(half_border, half_border), vec2<f32>(0.0));
        let inner_radius = max(in.radius - vec4<f32>(half_border), vec4<f32>(0.0));
        let inner_distance = sd_rounded_box(in.local, inner, inner_radius);
        let ring = clamp(shape - (1.0 - smoothstep(-aa, aa, inner_distance)), 0.0, 1.0);
        color += vec4<f32>(in.foreground.rgb * in.foreground.a, in.foreground.a) * ring;
    }

    return color;
}
"#,
);

/// Moduł shaderów dla tekstu (atlas glifów w kanale alpha).
pub const TEXT_WGSL: &str = concat!(
    r#"
struct Frame {
    // Rozmiar render targetu w pikselach fizycznych.
    resolution: vec2<f32>,
    // Rozmiar pojedynczego texela bufora rozmycia.
    blur_texel: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var atlas_texture: texture_2d<f32>;

struct GlyphIn {
    // Prostokąt glifu w pikselach fizycznych: x, y, w, h.
    @location(0) rect: vec4<f32>,
    // Zakres tekstury: u0, v0, u1, v1.
    @location(1) uv_rect: vec4<f32>,
    // Kolor w formacie straight alpha.
    @location(2) color: vec4<f32>,
    // Obszar przycięcia w pikselach fizycznych: x0, y0, x1, y1.
    @location(3) clip: vec4<f32>,
};

struct GlyphOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) screen: vec2<f32>,
    @location(3) clip: vec4<f32>,
};

@vertex
fn vs_glyph(in: GlyphIn, @builtin(vertex_index) vertex_index: u32) -> GlyphOut {
    var out: GlyphOut;

    let offset = vec2<f32>(
        select(0.0, in.rect.z, (vertex_index & 1u) == 1u),
        select(0.0, in.rect.w, (vertex_index & 2u) == 2u),
    );
    let uv_offset = vec2<f32>(
        select(0.0, in.uv_rect.z - in.uv_rect.x, (vertex_index & 1u) == 1u),
        select(0.0, in.uv_rect.w - in.uv_rect.y, (vertex_index & 2u) == 2u),
    );
    let vertex_px = in.rect.xy + offset;

    let ndc = vec2<f32>(
        (vertex_px.x / frame.resolution.x) * 2.0 - 1.0,
        1.0 - (vertex_px.y / frame.resolution.y) * 2.0,
    );

    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = in.uv_rect.xy + uv_offset;
    out.color = in.color;
    out.screen = vertex_px;
    out.clip = in.clip;
    return out;
}

@fragment
fn fs_glyph(in: GlyphOut) -> @location(0) vec4<f32> {
    if (in.screen.x < in.clip.x || in.screen.y < in.clip.y
        || in.screen.x > in.clip.z || in.screen.y > in.clip.w) {
        discard;
    }
    // Kanał ALFA: dla `SwashContent::Mask` pokrycie zapisujemy do `a`,
    // a `.rgb` wypełniamy białym. Próbkowanie `.r` dawałoby zero i cały
    // tekst byłby niewidoczny.
    let coverage = textureSampleLevel(atlas_texture, linear_sampler, in.uv, 0.0).a;
    let alpha = in.color.a * coverage;
    return vec4<f32>(in.color.rgb * alpha, alpha);
}
"#,
);

/// Moduł post-processingu: separowalny blur (H/V) oraz kompozycja sceny
/// z maską kształtu okna (zaokrąglone przezroczyste narożniki).
pub const POST_WGSL: &str = concat!(
    r#"
struct BlurParams {
    // Rozmiar texela tekstury źródłowej.
    texel: vec2<f32>,
    // Promień rozmycia w pikselach w osi X i Y.
    radius: vec2<f32>,
};

@group(0) @binding(0) var linear_sampler: sampler;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: BlurParams;

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex_index: u32) -> FullscreenOut {
    var out: FullscreenOut;
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}
/// 9-próbkowe przybliżenie gaussa (rozmycie separowalne), wagi rozwinięte
/// ręcznie — unika indeksowania tablicy zmienną w pętli.
fn blur(uv: vec2<f32>, stride: vec2<f32>) -> vec4<f32> {
    let c0 = 0.2270270270;
    let c1 = 0.1945945946;
    let c2 = 0.1216216216;
    let c3 = 0.0540540541;
    let c4 = 0.0162162162;

    var acc = textureSampleLevel(source_texture, linear_sampler, uv, 0.0) * c0;
    let s1 = stride * 1.0;
    let s2 = stride * 2.0;
    let s3 = stride * 3.0;
    let s4 = stride * 4.0;

    acc += textureSampleLevel(source_texture, linear_sampler, uv + s1, 0.0) * c1;
    acc += textureSampleLevel(source_texture, linear_sampler, uv - s1, 0.0) * c1;
    acc += textureSampleLevel(source_texture, linear_sampler, uv + s2, 0.0) * c2;
    acc += textureSampleLevel(source_texture, linear_sampler, uv - s2, 0.0) * c2;
    acc += textureSampleLevel(source_texture, linear_sampler, uv + s3, 0.0) * c3;
    acc += textureSampleLevel(source_texture, linear_sampler, uv - s3, 0.0) * c3;
    acc += textureSampleLevel(source_texture, linear_sampler, uv + s4, 0.0) * c4;
    acc += textureSampleLevel(source_texture, linear_sampler, uv - s4, 0.0) * c4;
    return clamp(acc, vec4<f32>(0.0), vec4<f32>(1.0));
}

@fragment
fn fs_blur_horizontal(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return blur(uv, params.texel * vec2<f32>(params.radius.x, 0.0));
}

@fragment
fn fs_blur_vertical(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return blur(uv, params.texel * vec2<f32>(0.0, params.radius.y));
}
"#,
);

/// Moduł kompozycji końcowej (scena → powierzchnia okna, z maską kształtu).
pub const COMPOSITE_WGSL: &str = concat!(
    r#"
struct CompositeParams {
    // Prostokąt kształtu okna w pikselach fizycznych: x, y, w, h.
    shape: vec4<f32>,
    // Promienie narożników: tl, tr, br, bl.
    radius: vec4<f32>,
    // Szerokość pasma antyaliasingu.
    aa: f32,
    // Wyrównanie rozmiaru struktury do 16 bajtów.
    _pad0: f32,
    _pad1: vec2<f32>,
};

@group(0) @binding(0) var linear_sampler: sampler;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: CompositeParams;

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex_index: u32) -> FullscreenOut {
    var out: FullscreenOut;
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}
fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn sd_rounded_box(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    let limit = max(min(b.x, b.y) * 0.5, 0.0);
    let rr = clamp(r, vec4<f32>(0.0, 0.0, 0.0, 0.0), vec4<f32>(limit, limit, limit, limit));
    var radius = rr.x;
    if (p.x > 0.0) {
        radius = select(rr.y, rr.z, p.y > 0.0);
    } else {
        radius = select(rr.x, rr.w, p.y > 0.0);
    }
    return sd_round_box(p, b, radius);
}

@fragment
fn fs_composite(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let scene = textureSampleLevel(source_texture, linear_sampler, uv, 0.0);

    let half_size = params.shape.zw * 0.5;
    let center = params.shape.xy + half_size;
    let pixel = uv * params.shape.zw;
    let distance = sd_rounded_box(pixel - center, half_size, params.radius);
    let mask = 1.0 - smoothstep(-params.aa, params.aa, distance);

    // Scena jest premultiplied, więc maskę nakładamy na oba kanały.
    return vec4<f32>(scene.rgb * mask, scene.a * mask);
}
"#,
);
