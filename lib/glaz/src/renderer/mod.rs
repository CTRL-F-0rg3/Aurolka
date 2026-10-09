//! Renderer oparty na `wgpu`.
//!
//! # Pipeline
//!
//! ```text
//!  widgety → kolejki wierzchołków → [scena offscreen] → kompozycja → surface okna
//! ```
//!
//! * Elementy z `Paint::backdrop` trafiają do osobnej kolejki i są rysowane
//!   **po** rozmyciu tła — dzięki temu „szkło” rozmywa to, co jest pod nim,
//!   a nie to, co jest nad nim.
//! * Przezroczystość wymusza renderowanie do tekstury `rgba16float` i osobny
//!   pass kompozycji, który nakłada maskę kształtu okna (zaokrąglone narożniki).
//! * Okno w pełni nieprzezroczyste i bez backdropu pomija cały pipeline
//!   pośredni — to najczęstszy i najszybszy przypadek.

use bytemuck::{Pod, Zeroable};

use crate::error::{Error, Result};
use crate::geometry::{Color, Point, Radius, Rect, Size};
use crate::transparency::{Transparency, WindowShape};

pub mod atlas;
pub mod paint;
pub mod shaders;
pub mod text;

use paint::{Paint, TextAlign, TextMetrics, TextStyle};
use shaders::{COMPOSITE_WGSL, POST_WGSL, QUAD_WGSL, TEXT_WGSL};
use text::TextEngine;

/// Format tekstury sceny (rozmycie wymaga zmiennoprzecinkowego).
const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Wierzchołek prostokąta (SDF).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    rect: [f32; 4],
    background: [f32; 4],
    foreground: [f32; 4],
    radius: [f32; 4],
    border_width: f32,
    shadow_offset: [f32; 2],
    shadow_blur: f32,
    shadow_spread: f32,
    shadow_color: [f32; 4],
    backdrop_blur: f32,
    backdrop_color: [f32; 4],
    backdrop_saturation: f32,
    clip: [f32; 4],
}

/// Wierzchołek glifu tekstowego.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct GlyphVertex {
    rect: [f32; 4],
    uv_rect: [f32; 4],
    color: [f32; 4],
    clip: [f32; 4],
}

/// Uniformy klatki.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    resolution: [f32; 2],
    blur_texel: [f32; 2],
}

/// Uniformy rozmycia.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BlurUniform {
    texel: [f32; 2],
    radius: [f32; 2],
}

/// Uniformy kompozycji (maska kształtu okna).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CompositeUniform {
    shape: [f32; 4],
    radius: [f32; 4],
    aa: f32,
    pad0: f32,
    pad1: [f32; 2],
}

/// Tekstury pośrednie wymagane przez przezroczystość i backdrop.
struct RenderTarget {
    size: [u32; 2],
    scene: wgpu::Texture,
    scene_view: wgpu::TextureView,
    blur_a: wgpu::Texture,
    blur_a_view: wgpu::TextureView,
    blur_b: wgpu::Texture,
    blur_b_view: wgpu::TextureView,
}

impl RenderTarget {
    fn destroy(&self) {
        self.scene.destroy();
        self.blur_a.destroy();
        self.blur_b.destroy();
    }
}

/// Konfiguracja renderera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RendererConfig {
    /// Format powierzchni okna.
    pub surface_format: wgpu::TextureFormat,
    /// Tryb przezroczystości.
    pub transparency: Transparency,
    /// Współczynnik skalowania ekranu (DPI).
    pub scale_factor: f32,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            surface_format: wgpu::TextureFormat::Bgra8Unorm,
            transparency: Transparency::Opaque,
            scale_factor: 1.0,
        }
    }
}

/// Renderer — serce biblioteki.
///
/// Widgety nigdy nie dotykają `wgpu` bezpośrednio; wszystkie rysowania
/// przechodzą przez uproszczone API (prostokąty i tekst), a batching,
/// przezroczystość i kompozycja są sprawą renderera.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: RendererConfig,

    quad_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    blur_h_pipeline: wgpu::RenderPipeline,
    blur_v_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,

    frame_layout: wgpu::BindGroupLayout,
    post_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    frame_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    text_bind_group: wgpu::BindGroup,
    blur_uniform: wgpu::Buffer,
    blur_h_bind_group: wgpu::BindGroup,
    blur_v_bind_group: wgpu::BindGroup,
    composite_uniform: wgpu::Buffer,
    composite_bind_group: wgpu::BindGroup,

    quad_buffer: wgpu::Buffer,
    quad_capacity: u64,
    glass_quad_buffer: wgpu::Buffer,
    glass_glyph_buffer: wgpu::Buffer,
    glyph_buffer: wgpu::Buffer,
    glyph_capacity: u64,
    sampler: wgpu::Sampler,

    // Kolejki rysowania: najpierw tło, potem elementy z backdropem.
    plain_quads: Vec<QuadVertex>,
    glass_quads: Vec<QuadVertex>,
    plain_glyphs: Vec<GlyphVertex>,
    glass_glyphs: Vec<GlyphVertex>,

    target: Option<RenderTarget>,
    text: TextEngine,

    // Tekstura 1×1 używana, zanim powstaną tekstury sceny.
    // Tekstura 1×1 używana, zanim powstaną tekstury sceny — trzymana, żeby
    // użytkownik nie musiał tworzyć zasobów przed pierwszą klatką.
    #[allow(dead_code)]
    dummy: wgpu::Texture,
    #[allow(dead_code)]
    dummy_view: wgpu::TextureView,

    size: Size,
    clip_stack: Vec<Rect>,
    clear_color: Color,
    shape: WindowShape,
    blur_radius: f32,
    blur_passes: u8,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("size", &self.size)
            .field("scale_factor", &self.config.scale_factor)
            .field("quads", &(self.plain_quads.len() + self.glass_quads.len()))
            .field(
                "glyphs",
                &(self.plain_glyphs.len() + self.glass_glyphs.len()),
            )
            .field("offscreen", &self.target.is_some())
            .finish()
    }
}

impl Renderer {
    /// Maksymalna liczba prostokątów w buforze wierzchołków.
    const QUAD_CAPACITY: usize = 16 * 1024;
    /// Maksymalna liczba glifów w buforze wierzchołków.
    const GLYPH_CAPACITY: usize = 64 * 1024;

    /// Tworzy renderer dla zadanego urządzenia.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, config: RendererConfig) -> Result<Self> {
        let quad_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurola.quad"),
            source: wgpu::ShaderSource::Wgsl(QUAD_WGSL.into()),
        });
        let text_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurola.text"),
            source: wgpu::ShaderSource::Wgsl(TEXT_WGSL.into()),
        });
        let post_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurola.post"),
            source: wgpu::ShaderSource::Wgsl(POST_WGSL.into()),
        });
        let composite_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurola.composite"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_WGSL.into()),
        });

        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let sampler_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let texture_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };

        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aurola.frame-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                sampler_entry(1, wgpu::ShaderStages::FRAGMENT),
                texture_entry(2, wgpu::ShaderStages::FRAGMENT),
            ],
        });

        let post_entries = [
            sampler_entry(0, wgpu::ShaderStages::FRAGMENT),
            texture_entry(1, wgpu::ShaderStages::FRAGMENT),
            uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
        ];

        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aurola.post-layout"),
            entries: &post_entries,
        });

        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aurola.composite-layout"),
            entries: &post_entries,
        });

        // 1×1 tekstura zastępcza — bind grupy muszą istnieć od razu, zanim
        // powstaną tekstury sceny (bind grupy odświeżamy w `ensure_target`).
        let dummy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aurola.dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let dummy_view = dummy.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aurola.linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aurola.frame-uniform"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blur_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aurola.blur-uniform"),
            size: std::mem::size_of::<BlurUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aurola.composite-uniform"),
            size: std::mem::size_of::<CompositeUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let frame_bind_group = Self::frame_bind_group(
            &device,
            &frame_layout,
            &sampler,
            frame_uniform.as_entire_binding(),
            &dummy_view,
        );

        let text = TextEngine::new(&device, &queue, config.scale_factor).map_err(|e| {
            Error::Window(format!("nie udało się przygotować silnika tekstu: {e:?}"))
        })?;

        let text_bind_group = Self::frame_bind_group(
            &device,
            &frame_layout,
            &sampler,
            frame_uniform.as_entire_binding(),
            text.atlas_view(),
        );

        let post_bind = |layout: &wgpu::BindGroupLayout, uniform: wgpu::BindingResource<'_>| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("aurola.post-bind-group"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&dummy_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform,
                    },
                ],
            })
        };

        let blur_h_bind_group = post_bind(&post_layout, blur_uniform.as_entire_binding());
        let blur_v_bind_group = post_bind(&post_layout, blur_uniform.as_entire_binding());
        let composite_bind_group =
            post_bind(&composite_layout, composite_uniform.as_entire_binding());

        // Osobne bufory dla passu zwikłego i szklanego. Obie kolejki trafiają
        // do jednego `submit`, a `Queue::write_buffer` jest kolejkowany — przy
        // wspólnym buforze drugi zapis nadpisywałby pierwszy jeszcze przed
        // wykonaniem komend, więc zwykłe elementy rysowałyby się jako szkło.
        let make_vertex_buffer = |label: &'static str, count: usize, stride: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: count as u64 * stride,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let quad_stride = std::mem::size_of::<QuadVertex>() as u64;
        let glyph_stride = std::mem::size_of::<GlyphVertex>() as u64;
        let quad_buffer =
            make_vertex_buffer("aurola.quad-buffer", Self::QUAD_CAPACITY, quad_stride);
        let glass_quad_buffer =
            make_vertex_buffer("aurola.glass-quad-buffer", Self::QUAD_CAPACITY, quad_stride);
        let glyph_buffer =
            make_vertex_buffer("aurola.glyph-buffer", Self::GLYPH_CAPACITY, glyph_stride);
        let glass_glyph_buffer = make_vertex_buffer(
            "aurola.glass-glyph-buffer",
            Self::GLYPH_CAPACITY,
            glyph_stride,
        );

        let quad_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aurola.quad-pipeline-layout"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aurola.post-pipeline-layout"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });
        // Pipeline kompozycji używa własnego bind grupy (scena), więc pipeline layout
        // tworzymy, ale nie trzymamy — bind grupa jest w `Renderer::composite_bind_group`.
        let _composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aurola.composite-pipeline-layout"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });

        // Premultiplied alpha: `src * 1 + dst * (1 - src.a)`.
        let blend = Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        });

        let quad_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32, 5 => Float32x2, 6 => Float32, 7 => Float32, 8 => Float32x4, 9 => Float32, 10 => Float32x4, 11 => Float32, 12 => Float32x4],
        };
        let glyph_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4],
        };

        let quad_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aurola.quad-pipeline"),
            layout: Some(&quad_layout),
            vertex: wgpu::VertexState {
                module: &quad_module,
                entry_point: Some("vs_quad"),
                compilation_options: Default::default(),
                buffers: &[Some(quad_vertex_layout.clone())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &quad_module,
                entry_point: Some("fs_quad"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SCENE_FORMAT,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aurola.text-pipeline"),
            layout: Some(&quad_layout),
            vertex: wgpu::VertexState {
                module: &text_module,
                entry_point: Some("vs_glyph"),
                compilation_options: Default::default(),
                buffers: &[Some(glyph_vertex_layout)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &text_module,
                entry_point: Some("fs_glyph"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SCENE_FORMAT,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let make_post =
            |label: &'static str, module: &wgpu::ShaderModule, entry: &'static str, format| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&post_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("vs_fullscreen"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };

        let blur_h_pipeline = make_post(
            "aurola.blur-h",
            &post_module,
            "fs_blur_horizontal",
            SCENE_FORMAT,
        );
        let blur_v_pipeline = make_post(
            "aurola.blur-v",
            &post_module,
            "fs_blur_vertical",
            SCENE_FORMAT,
        );
        let composite_pipeline = make_post(
            "aurola.composite",
            &composite_module,
            "fs_composite",
            config.surface_format,
        );

        let (blur_radius, blur_passes) = match config.transparency {
            Transparency::Blur(backdrop) => (backdrop.radius, backdrop.passes),
            _ => (0.0, 0),
        };

        Ok(Self {
            device,
            queue,
            quad_pipeline,
            text_pipeline,
            blur_h_pipeline,
            blur_v_pipeline,
            composite_pipeline,
            frame_layout,
            post_layout,
            composite_layout,
            frame_uniform,
            frame_bind_group,
            text_bind_group,
            blur_uniform,
            blur_h_bind_group,
            blur_v_bind_group,
            composite_uniform,
            composite_bind_group,
            quad_buffer,
            quad_capacity: Self::QUAD_CAPACITY as u64,
            glass_quad_buffer,
            glass_glyph_buffer,
            glyph_buffer,
            glyph_capacity: Self::GLYPH_CAPACITY as u64,
            sampler,
            plain_quads: Vec::with_capacity(512),
            glass_quads: Vec::new(),
            plain_glyphs: Vec::with_capacity(1024),
            glass_glyphs: Vec::new(),
            target: None,
            text,
            size: Size::ZERO,
            clip_stack: Vec::new(),
            clear_color: Color::TRANSPARENT,
            shape: WindowShape::Rectangle,
            blur_radius,
            blur_passes,
            config,
            dummy,
            dummy_view,
        })
    }

    /// Bind grupy współdzielącej układ: uniform + sampler + tekstura.
    fn frame_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        uniform: wgpu::BindingResource<'_>,
        texture: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aurola.frame-bind-group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform,
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(texture),
                },
            ],
        })
    }

    /// Bind grupy post-processingu: sampler + tekstura + uniform.
    fn post_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        uniform: wgpu::BindingResource<'_>,
        texture: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aurola.post-bind-group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform,
                },
            ],
        })
    }

    /// Konfiguracja powierzchni renderowania dla danego formatu i przezroczystości.
    pub fn surface_config(
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        transparency: Transparency,
    ) -> wgpu::SurfaceConfiguration {
        wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            color_space: wgpu::SurfaceColorSpace::Auto,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: if transparency.is_transparent() {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else {
                wgpu::CompositeAlphaMode::Auto
            },
            view_formats: vec![],
        }
    }

    /// Rozmiar renderu w pikselach logicznych.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Współczynnik skalowania ekranu.
    pub fn scale_factor(&self) -> f32 {
        self.config.scale_factor
    }

    /// Ustawia współczynnik skalowania (po zmianie DPI).
    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        if (scale_factor - self.config.scale_factor).abs() > f32::EPSILON {
            self.config.scale_factor = scale_factor;
            self.text.set_scale_factor(scale_factor);
        }
    }

    /// Ustawia tryb przezroczystości okna.
    pub fn set_transparency(&mut self, transparency: Transparency) {
        self.config.transparency = transparency;
        self.shape = match transparency {
            Transparency::Blur(backdrop) => backdrop.shape,
            _ => WindowShape::Rectangle,
        };
        if let Transparency::Blur(backdrop) = transparency {
            self.blur_radius = backdrop.radius;
            self.blur_passes = backdrop.passes;
        } else {
            self.blur_radius = 0.0;
            self.blur_passes = 0;
        }
    }

    /// Ustawia kształt okna (zaokrąglone narożniki).
    pub fn set_shape(&mut self, shape: WindowShape) {
        self.shape = shape;
    }

    /// Ustawia kolor czyszczenia klatki.
    pub fn set_clear_color(&mut self, color: Color) {
        self.clear_color = color;
    }

    /// Rozpoczyna nową klatkę: czyści kolejki i resetuje stos przycięć.
    pub fn begin_frame(&mut self, size: Size) {
        self.size = size;
        self.plain_quads.clear();
        self.glass_quads.clear();
        self.plain_glyphs.clear();
        self.glass_glyphs.clear();
        self.clip_stack.clear();
    }

    /// Aktualny obszar przycięcia (top stosu).
    pub fn clip(&self) -> Rect {
        self.clip_stack
            .last()
            .copied()
            .unwrap_or(Rect::new(Point::ZERO, self.size))
    }

    /// Dokłada obszar przycięcia (przecięcie z aktualnym).
    pub fn push_clip(&mut self, rect: Rect) {
        let next = self.clip().intersection(&rect);
        self.clip_stack.push(next);
    }

    /// Zdejmuje ostatni obszar przycięcia.
    pub fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    /// Mierzy tekst (z cache).
    pub fn measure(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextMetrics {
        self.text.measure(text, style, max_width)
    }

    /// Dodaje prostokąt do kolejki rysowania.
    pub fn draw_quad(&mut self, paint: &Paint) {
        if !paint.is_drawable() {
            return;
        }

        let scale = self.config.scale_factor;
        let clip = if paint.clip.size.is_valid() && paint.clip.size.width > 0.0 {
            paint.clip
        } else {
            Rect::new(Point::ZERO, self.size)
        };

        let shadow = paint.shadow.filter(|s| s.is_visible());
        let (shadow_offset, shadow_blur, shadow_spread, shadow_color) = shadow
            .map(|s| (s.offset, s.blur, s.spread, s.color))
            .unwrap_or((Point::ZERO, 0.0, 0.0, Color::TRANSPARENT));

        let backdrop = &paint.backdrop;
        let vertex = QuadVertex {
            rect: [
                paint.bounds.left() * scale,
                paint.bounds.top() * scale,
                paint.bounds.size.width * scale,
                paint.bounds.size.height * scale,
            ],
            background: paint.fill.unwrap_or(Color::TRANSPARENT).to_array(),
            foreground: paint.border.unwrap_or(Color::TRANSPARENT).to_array(),
            radius: [
                paint.radius.top_left * scale,
                paint.radius.top_right * scale,
                paint.radius.bottom_right * scale,
                paint.radius.bottom_left * scale,
            ],
            border_width: paint.border_width * scale,
            shadow_offset: [shadow_offset.x * scale, shadow_offset.y * scale],
            shadow_blur: shadow_blur * scale,
            shadow_spread: shadow_spread * scale,
            shadow_color: shadow_color.to_array(),
            // Efekt „szkla" musi dzialac takze wtedy, gdy okno samo nie ma
            // skonfigurowanego `Transparency::Blur` — inaczej `blur_radius` wynosi
            // 0.0, a `BackdropPaint` ustawiony przez aplikacje byl cicho ignorowany.
            // Promien bierzemy z paintu, a z konfiguratora jako dolna granice.
            backdrop_blur: if backdrop.is_enabled() {
                let requested = backdrop.radius * scale;
                requested.max(self.blur_radius * scale)
            } else {
                0.0
            },
            backdrop_color: backdrop.tint.to_array(),
            backdrop_saturation: backdrop.saturation,
            clip: [
                clip.left() * scale,
                clip.top() * scale,
                clip.right() * scale,
                clip.bottom() * scale,
            ],
        };

        // Kolejność renderowania jest determinowana przez `z`.
        let bucket = if vertex.backdrop_blur > 0.0 {
            &mut self.glass_quads
        } else {
            &mut self.plain_quads
        };
        if bucket.len() < self.quad_capacity as usize {
            bucket.push(vertex);
        }
    }

    /// Wypełnia prostokąt kolorem (używany przez większość widgetów).
    pub fn fill(&mut self, bounds: Rect, radius: Radius, color: Color) {
        let clip = self.clip();
        let paint = Paint::rounded(bounds, radius, color).with_clip(clip);
        self.draw_quad(&paint);
    }

    /// Rysuje tekst w zadanym prostokącie (wyrównanie w pionie: środek).
    ///
    /// Zwraca faktycznie zajmowany rozmiar.
    pub fn draw_text(&mut self, content: &str, bounds: Rect, style: &TextStyle) -> Size {
        if content.is_empty() {
            return Size::ZERO;
        }

        let max_width = Some(bounds.size.width);
        let metrics = self.text.measure(content, style, max_width);
        let x = match style.align {
            TextAlign::Start => bounds.left(),
            TextAlign::Center => bounds.left() + (bounds.size.width - metrics.size.width) / 2.0,
            TextAlign::End => bounds.right() - metrics.size.width,
        };
        let y = bounds.top() + (bounds.size.height - metrics.size.height) / 2.0;

        let shaped = self
            .text
            .shape(&self.queue, content, style, max_width, Point::new(x, y));
        let clip = self.clip();
        let clip_px = [
            clip.left() * self.config.scale_factor,
            clip.top() * self.config.scale_factor,
            clip.right() * self.config.scale_factor,
            clip.bottom() * self.config.scale_factor,
        ];

        let bucket = &mut self.plain_glyphs;
        for glyph in shaped.glyphs {
            if bucket.len() >= self.glyph_capacity as usize {
                break;
            }
            bucket.push(GlyphVertex {
                rect: glyph.rect,
                uv_rect: glyph.uv,
                color: style.color.to_array(),
                clip: clip_px,
            });
        }

        metrics.size
    }

    /// Rysuje tekst na elemencie z rozmyciem tła (warstwa „szkła”).
    pub fn draw_text_on_backdrop(
        &mut self,
        content: &str,
        bounds: Rect,
        style: &TextStyle,
    ) -> Size {
        if content.is_empty() {
            return Size::ZERO;
        }

        let max_width = Some(bounds.size.width);
        let metrics = self.text.measure(content, style, max_width);
        let x = match style.align {
            TextAlign::Start => bounds.left(),
            TextAlign::Center => bounds.left() + (bounds.size.width - metrics.size.width) / 2.0,
            TextAlign::End => bounds.right() - metrics.size.width,
        };
        let y = bounds.top() + (bounds.size.height - metrics.size.height) / 2.0;

        let shaped = self
            .text
            .shape(&self.queue, content, style, max_width, Point::new(x, y));
        let clip = self.clip();
        let clip_px = [
            clip.left() * self.config.scale_factor,
            clip.top() * self.config.scale_factor,
            clip.right() * self.config.scale_factor,
            clip.bottom() * self.config.scale_factor,
        ];

        let bucket = &mut self.glass_glyphs;
        for glyph in shaped.glyphs {
            if bucket.len() >= self.glyph_capacity as usize {
                break;
            }
            bucket.push(GlyphVertex {
                rect: glyph.rect,
                uv_rect: glyph.uv,
                color: style.color.to_array(),
                clip: clip_px,
            });
        }

        metrics.size
    }

    /// Zapewnia, że tekstury pośrednie istnieją i mają właściwy rozmiar.
    fn ensure_target(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if let Some(target) = &self.target {
            if target.size == [width, height] {
                return;
            }
        }
        if let Some(target) = self.target.take() {
            target.destroy();
        }

        let make = |label: &'static str| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SCENE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };

        let scene = make("aurola.scene");
        let blur_a = make("aurola.blur-a");
        let blur_b = make("aurola.blur-b");
        let scene_view = scene.create_view(&wgpu::TextureViewDescriptor::default());
        let blur_a_view = blur_a.create_view(&wgpu::TextureViewDescriptor::default());
        let blur_b_view = blur_b.create_view(&wgpu::TextureViewDescriptor::default());

        // Bind grupy pipeline'u prostokątów próbuje rozmyte tło z `blur_b` —
        // tam ląduje wynik po obu przebiegach (H: scene → blur_a, V: → blur_b).
        self.frame_bind_group = Self::frame_bind_group(
            &self.device,
            &self.frame_layout,
            &self.sampler,
            self.frame_uniform.as_entire_binding(),
            // Po obu przebiegach rozmycie leży w `blur_b`:
            // H: scene → blur_a, potem V: blur_a → blur_b.
            &blur_b_view,
        );
        let blur_uniform = self.blur_uniform.as_entire_binding();
        self.blur_h_bind_group = Self::post_bind_group(
            &self.device,
            &self.post_layout,
            &self.sampler,
            blur_uniform,
            &scene_view,
        );
        let blur_uniform = self.blur_uniform.as_entire_binding();
        self.blur_v_bind_group = Self::post_bind_group(
            &self.device,
            &self.post_layout,
            &self.sampler,
            blur_uniform,
            &blur_a_view,
        );
        let composite_uniform = self.composite_uniform.as_entire_binding();
        self.composite_bind_group = Self::post_bind_group(
            &self.device,
            &self.composite_layout,
            &self.sampler,
            composite_uniform,
            &scene_view,
        );

        self.target = Some(RenderTarget {
            size: [width, height],
            scene,
            scene_view,
            blur_a,
            blur_a_view,
            blur_b,
            blur_b_view,
        });
    }

    /// Renderuje zebraną klatkę na podaną powierzchnię.
    ///
    /// Kolejność:
    /// 1. tło i zwykłe elementy → scena,
    /// 2. rozmycie sceny (jeśli są elementy „szklane”),
    /// 3. elementy szklane na wierzchu,
    /// 4. kompozycja sceny na powierzchnię okna z maską kształtu.
    pub fn render(&mut self, surface: &wgpu::Surface) -> Result<()> {
        // `wgpu` 30 zwraca wariant zamiast `Result` — `Lost`/`Outdated`
        // to sytuacje odtwarzalne, więc po prostu pomijamy klatkę.
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            other => {
                log::debug!("klatka pominięta: {other:?}");
                return Ok(());
            }
        };

        let width = frame.texture.width();
        let height = frame.texture.height();
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Zawsze renderujemy do tekstury sceny, a potem komponujemy na powierzchnie
        // okna. Powod: pipeline'y prostokatow i tekstu celuja w `SCENE_FORMAT`
        // (rgba16float), a format powierzchni zalezy od platformy
        // (`Bgra8UnormSrgb`, `Rgba8Unorm`, ...). Rysowanie prosto na surface
        // dawalo blad walidacji "incompatible color attachments".
        // Jeden dodatkowy pelnoscreenowy blit kosztuje znikomo, a dzieki niemu
        // istnieje tylko jedna sciezka renderowania.
        self.ensure_target(width, height);

        self.queue.write_buffer(
            &self.frame_uniform,
            0,
            bytemuck::bytes_of(&FrameUniform {
                resolution: [width as f32, height as f32],
                blur_texel: [1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32],
            }),
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aurola.frame"),
            });

        let (scene_view, blur_a_view, blur_b_view) = {
            let t = self.target.as_ref().expect("tekstury sceny utworzone");
            (
                t.scene_view.clone(),
                t.blur_a_view.clone(),
                t.blur_b_view.clone(),
            )
        };
        let radius = self.blur_radius * self.config.scale_factor;

        // --- 1. Tlo + zwykle elementy ---
        self.record_pass(&mut encoder, &scene_view, true, false);

        let has_glass = !self.glass_quads.is_empty() || !self.glass_glyphs.is_empty();

        if has_glass {
            // --- 2. Rozmycie tla (H: scene -> blur_a, V: blur_a -> blur_b) ---
            self.queue.write_buffer(
                &self.blur_uniform,
                0,
                bytemuck::bytes_of(&BlurUniform {
                    texel: [1.0 / width as f32, 1.0 / height as f32],
                    radius: [radius, radius],
                }),
            );
            self.record_post(
                &mut encoder,
                &self.blur_h_pipeline,
                &self.blur_h_bind_group,
                &blur_a_view,
            );
            self.record_post(
                &mut encoder,
                &self.blur_v_pipeline,
                &self.blur_v_bind_group,
                &blur_b_view,
            );

            // --- 3. Elementy "szklane" (probkuja rozmycie z blur_b) ---
            self.record_pass(&mut encoder, &scene_view, false, true);
        }

        // --- 4. Kompozycja z maska ksztaltu okna ---
        self.record_composite(&mut encoder, &scene_view, &surface_view, width, height);

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        self.text.clear_dirty();
        Ok(())
    }

    /// Zapisuje pass rysujący prostokąty i glify (zwykłe albo szklane).
    ///
    /// `clear` decyduje, czy zawartość bufora ma zostać wyczyszczona.
    fn record_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        clear: bool,
        glass: bool,
    ) {
        let (quad_len, glyph_len) = if glass {
            (self.glass_quads.len(), self.glass_glyphs.len())
        } else {
            (self.plain_quads.len(), self.plain_glyphs.len())
        };
        if quad_len == 0 && glyph_len == 0 {
            return;
        }

        // Wgrywamy wierzchołki przed rozpoczęciem pasa (kolejka nie jest w trakcie).
        // Kazdy pass dostaje wlasny bufor: oba trafiaja do jednego `submit`,
        // a `write_buffer` jest kolejkowany, wiec wspolny bufor bylby nadpisany
        // zanim komendy w ogole sie wykonaja.
        if quad_len > 0 {
            let (buffer, data): (&wgpu::Buffer, &[QuadVertex]) = if glass {
                (&self.glass_quad_buffer, &self.glass_quads)
            } else {
                (&self.quad_buffer, &self.plain_quads)
            };
            self.queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(data));
        }
        if glyph_len > 0 {
            let (buffer, data): (&wgpu::Buffer, &[GlyphVertex]) = if glass {
                (&self.glass_glyph_buffer, &self.glass_glyphs)
            } else {
                (&self.glyph_buffer, &self.plain_glyphs)
            };
            self.queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(data));
        }

        let quad_bind_group = self.frame_bind_group.clone();
        let text_bind_group = self.text_bind_group.clone();
        let (quad_buffer, glyph_buffer) = if glass {
            (
                self.glass_quad_buffer.slice(..),
                self.glass_glyph_buffer.slice(..),
            )
        } else {
            (self.quad_buffer.slice(..), self.glyph_buffer.slice(..))
        };
        let quad_pipeline = &self.quad_pipeline;
        let text_pipeline = &self.text_pipeline;

        let clear_value = wgpu::Color {
            r: self.clear_color.r as f64,
            g: self.clear_color.g as f64,
            b: self.clear_color.b as f64,
            a: self.clear_color.a as f64,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(if glass {
                "aurola.glass"
            } else {
                "aurola.solid"
            }),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(clear_value)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        if quad_len > 0 {
            pass.set_pipeline(quad_pipeline);
            pass.set_bind_group(0, &quad_bind_group, &[]);
            pass.set_vertex_buffer(0, quad_buffer);
            pass.draw(0..(quad_len * 4) as u32, 0..quad_len as u32);
        }
        if glyph_len > 0 {
            pass.set_pipeline(text_pipeline);
            pass.set_bind_group(0, &text_bind_group, &[]);
            pass.set_vertex_buffer(0, glyph_buffer);
            pass.draw(0..(glyph_len * 4) as u32, 0..glyph_len as u32);
        }
    }

    /// Zapisuje pełnoekranowy pass post-processingu.
    fn record_post(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        view: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aurola.post"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Zapisuje pass kompozycji sceny na powierzchnię okna (maska kształtu).
    fn record_composite(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        _scene_view: &wgpu::TextureView,
        surface_view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        let scale = self.config.scale_factor;
        let radius = self.shape.radius().clamp(self.size);
        self.queue.write_buffer(
            &self.composite_uniform,
            0,
            bytemuck::bytes_of(&CompositeUniform {
                shape: [0.0, 0.0, width as f32, height as f32],
                radius: [
                    radius.top_left * scale,
                    radius.top_right * scale,
                    radius.bottom_right * scale,
                    radius.bottom_left * scale,
                ],
                // Szerokość pasma antyaliasingu maski (jeden texel).
                aa: 1.0,
                pad0: 0.0,
                pad1: [0.0, 0.0],
            }),
        );

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aurola.composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &self.composite_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Zwalnia zasoby GPU renderera.
    pub fn destroy(&mut self) {
        if let Some(target) = self.target.take() {
            target.destroy();
        }
        self.text.destroy();
    }
}

pub use atlas::{AtlasSlot, ShelfAllocator};
// Publiczne nazwy pochodzą z `paint` — moduł re-eksportuje je „w płaskiej”
// przestrzeni, żeby `glaz::renderer::Paint` działało tak, jak w `iced`.
pub use paint::{
    BackdropPaint, Paint as PaintPublic, Shadow, TextAlign as TextAlignPublic, TextLine,
    TextMetrics as TextMetricsPublic, TextStyle as TextStylePublic,
};
pub use text::{GlyphPlacement, ShapedText};
