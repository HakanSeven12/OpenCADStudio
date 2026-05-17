pub mod face3d_gpu;
pub mod hatch_gpu;
pub mod image_gpu;
pub mod mesh_gpu;
pub mod uniforms;
pub mod viewcube;
pub mod wire_gpu;

use iced::wgpu;
use iced::{Rectangle, Size};

pub use face3d_gpu::Face3DGpu;
pub use hatch_gpu::HatchGpu;
pub use image_gpu::ImageGpu;
pub use mesh_gpu::MeshLodGpu;
pub use uniforms::Uniforms;
pub use viewcube::ViewCubePipeline;
pub use wire_gpu::WireGpu;

use crate::scene::hatch_model::HatchModel;
use crate::scene::image_model::ImageModel;
use crate::scene::mesh_model::MeshLodSet;
use crate::scene::wire_model::WireModel;

/// MSAA sample count for the main drawing pipelines.
const MSAA_SAMPLES: u32 = 4;

pub struct Pipeline {
    wire_pipeline: wgpu::RenderPipeline,
    /// Same shader as wire_pipeline but depth_compare=Greater, depth_write_enabled=false.
    /// Used to draw ghost copies of selected wires through occluding geometry.
    wire_xray_pipeline: wgpu::RenderPipeline,
    hatch_pipeline: wgpu::RenderPipeline,
    image_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    face3d_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    hatch_bgl1: wgpu::BindGroupLayout,
    image_bgl1: wgpu::BindGroupLayout,
    depth_texture_size: Size<u32>,
    depth_view: wgpu::TextureView,
    /// 4× MSAA color buffer for the main drawing passes.
    msaa_view: wgpu::TextureView,
    /// Single-sample texture that receives the MSAA resolve result.
    resolve_view: wgpu::TextureView,
    /// Pipeline + resources for blitting the resolve texture to the surface target.
    blit_pipeline: wgpu::RenderPipeline,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_sampler: wgpu::Sampler,
    blit_bind_group: wgpu::BindGroup,
    /// Cached texture format (needed to recreate MSAA / depth textures on resize).
    surface_format: wgpu::TextureFormat,
    gpu_wires: Vec<WireGpu>,
    /// Pixel scissor rects [x, y, w, h] for viewport-clipped wires. Recomputed each frame.
    wire_pixel_scissors: Vec<Option<[u32; 4]>>,
    /// Ghost copies (25% alpha) of selected wires for the X-ray depth pass.
    gpu_selected_wires: Vec<WireGpu>,
    gpu_hatches: Vec<HatchGpu>,
    /// Pixel scissor rects [x, y, w, h] for viewport-clipped hatches. Recomputed each frame.
    hatch_pixel_scissors: Vec<Option<[u32; 4]>>,
    /// `true` for hatches whose AABB projects to less than ~2 px at the
    /// current zoom. Render pass skips the draw call. Recomputed each
    /// frame alongside the scissor pass (Phase 3.3 hatch LOD).
    hatch_skip_flags: Vec<bool>,
    /// Wipeout fills — rendered after wires in a separate pass.
    gpu_wipeouts: Vec<HatchGpu>,
    /// Pixel scissor rects [x, y, w, h] for viewport-clipped wipeouts. Recomputed each frame.
    wipeout_pixel_scissors: Vec<Option<[u32; 4]>>,
    gpu_images: Vec<ImageGpu>,
    /// Pixel scissor rects [x, y, w, h] for viewport-clipped images. Recomputed each frame.
    image_pixel_scissors: Vec<Option<[u32; 4]>>,
    gpu_meshes: Vec<MeshLodGpu>,
    /// Per-mesh LOD level (0=high, 1=mid, 2=low) picked each frame from
    /// the projected pixel diagonal. Mirrors `hatch_pixel_scissors` —
    /// recomputed in `compute_mesh_lod`.
    mesh_lod_levels: Vec<usize>,
    /// Per-mesh frustum-visible flag (Phase 2.2). `false` when the
    /// mesh's projected AABB falls entirely outside the viewport rect
    /// — the draw loop skips it. Mirrors `mesh_lod_levels` length /
    /// index space; recomputed alongside it.
    mesh_visible: Vec<bool>,
    /// Batched 3DFACE fill (all faces in one buffer) and edges (merged wire).
    gpu_face3d_fill: Option<Face3DGpu>,
    gpu_face3d_edges: Vec<WireGpu>,
    pub viewcube: ViewCubePipeline,
    /// Last `(geometry_epoch, camera_generation)` value for which GPU buffers
    /// were uploaded. We re-upload when either side changes — pan/zoom bumps
    /// camera_generation, which triggers re-culling and a fresh upload.
    pub cached_epoch: (u64, u64),
}

impl Pipeline {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // ── Shared frame uniform buffer (view_proj etc.) ───────────────────
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewer.uniform_buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind group layout 0 — shared by wire and hatch pipelines.
        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer.frame_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer.bind_group"),
            layout: &frame_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // ── Wire pipeline ──────────────────────────────────────────────────
        let wire_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wire.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl],
            push_constant_ranges: &[],
        });

        let depth_tex = create_depth_texture(device, Size::new(1, 1));
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let wire_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wire.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/wire.wgsl"
            ))),
        });

        let wire_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire.pipeline"),
            layout: Some(&wire_layout),
            vertex: wgpu::VertexState {
                module: &wire_shader,
                entry_point: Some("vs_main"),
                buffers: &[wire_gpu::WireVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &wire_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        // Selection overlay variant: renders selected wires on top of everything
        // (depth_compare=Always), without writing depth. Ensures selected entities
        // are always fully visible regardless of occluding geometry.
        let wire_xray_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire_xray.pipeline"),
            layout: Some(&wire_layout),
            vertex: wgpu::VertexState {
                module: &wire_shader,
                entry_point: Some("vs_main"),
                buffers: &[wire_gpu::WireVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &wire_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        // ── Hatch pipeline ─────────────────────────────────────────────────
        // binding 0 (HatchUniforms) is read by the vertex shader too — it
        // pulls `origin` to undo the CPU-side hatch-local pre-shift when
        // computing clip position. bindings 1 (Boundary) and 2
        // (FamilyBatch) stay fragment-only.
        let hatch_entry = |binding: u32, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let frag = wgpu::ShaderStages::FRAGMENT;
        let vert_frag = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let hatch_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hatch.bgl1"),
            entries: &[
                hatch_entry(0, vert_frag),
                hatch_entry(1, frag),
                hatch_entry(2, frag),
            ],
        });

        let hatch_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hatch.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &hatch_bgl1],
            push_constant_ranges: &[],
        });

        let hatch_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hatch.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/hatch.wgsl"
            ))),
        });

        let hatch_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hatch.pipeline"),
            layout: Some(&hatch_layout),
            vertex: wgpu::VertexState {
                module: &hatch_shader,
                entry_point: Some("vs_main"),
                buffers: &[hatch_gpu::HatchVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &hatch_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        // ── Mesh pipeline ──────────────────────────────────────────────────
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/mesh.wgsl"
            ))),
        });

        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl],
            push_constant_ranges: &[],
        });

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh.pipeline"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                buffers: &[mesh_gpu::MeshVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        // ── Face3D pipeline ────────────────────────────────────────────────
        let face3d_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("face3d.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/face3d.wgsl"
            ))),
        });

        let face3d_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("face3d.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl],
            push_constant_ranges: &[],
        });

        let face3d_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("face3d.pipeline"),
            layout: Some(&face3d_layout),
            vertex: wgpu::VertexState {
                module: &face3d_shader,
                entry_point: Some("vs_main"),
                buffers: &[face3d_gpu::Face3DVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &face3d_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        // ── Image pipeline ─────────────────────────────────────────────────
        let image_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image.bgl1"),
            entries: &[
                // binding 0: texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 1: sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // binding 2: ImageParams uniform
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &image_bgl1],
            push_constant_ranges: &[],
        });

        let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/image.wgsl"
            ))),
        });

        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image.pipeline"),
            layout: Some(&image_layout),
            vertex: wgpu::VertexState {
                module: &image_shader,
                entry_point: Some("vs_main"),
                buffers: &[image_gpu::ImageVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &image_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        let viewcube = ViewCubePipeline::new(device, queue, format);

        let init_size = Size::new(1, 1);
        let msaa_view = create_msaa_texture(device, init_size, format)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let resolve_tex = create_resolve_texture(device, init_size, format);
        let resolve_view = resolve_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // ── Blit pipeline (resolve texture → surface target) ──────────────
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/blit.wgsl"
            ))),
        });

        let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit.pipeline_layout"),
            bind_group_layouts: &[&blit_bgl],
            push_constant_ranges: &[],
        });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit.pipeline"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit.sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit.bind_group"),
            layout: &blit_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolve_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&blit_sampler),
                },
            ],
        });

        Self {
            wire_pipeline,
            wire_xray_pipeline,
            hatch_pipeline,
            image_pipeline,
            mesh_pipeline,
            face3d_pipeline,
            uniform_buffer,
            uniform_bind_group,
            hatch_bgl1,
            image_bgl1,
            depth_texture_size: Size::new(1, 1),
            depth_view,
            msaa_view,
            resolve_view,
            blit_pipeline,
            blit_bind_group_layout: blit_bgl,
            blit_sampler,
            blit_bind_group,
            surface_format: format,
            gpu_wires: vec![],
            wire_pixel_scissors: vec![],
            gpu_selected_wires: vec![],
            gpu_hatches: vec![],
            hatch_pixel_scissors: vec![],
            hatch_skip_flags: vec![],
            gpu_wipeouts: vec![],
            wipeout_pixel_scissors: vec![],
            gpu_images: vec![],
            image_pixel_scissors: vec![],
            gpu_meshes: vec![],
            mesh_lod_levels: vec![],
            mesh_visible: vec![],
            gpu_face3d_fill: None,
            gpu_face3d_edges: vec![],
            viewcube,
            cached_epoch: (u64::MAX, u64::MAX),
        }
    }

    pub fn upload_wires(&mut self, device: &wgpu::Device, wires: &[WireModel]) {
        self.gpu_wires = wires.iter().map(|w| WireGpu::new(device, w)).collect();

        // Full-brightness copies of selected wires, drawn on top of everything
        // in the selection overlay pass so they're always visible.
        self.gpu_selected_wires = wires
            .iter()
            .filter(|w| w.selected)
            .map(|w| WireGpu::new(device, w))
            .collect();
    }

    /// Recompute pixel scissor rects for viewport-clipped wires from the current view_proj.
    /// Called every frame from prepare() because scissor pixels shift with pan/zoom.
    pub fn compute_wire_scissors(&mut self, view_proj: glam::Mat4, clip_w: u32, clip_h: u32) {
        self.wire_pixel_scissors = self
            .gpu_wires
            .iter()
            .map(|w| project_scissor(w.vp_scissor, view_proj, clip_w, clip_h))
            .collect();
    }

    /// Recompute pixel scissor rects for viewport-clipped hatches.
    pub fn compute_hatch_scissors(&mut self, view_proj: glam::Mat4, clip_w: u32, clip_h: u32) {
        self.hatch_pixel_scissors = self
            .gpu_hatches
            .iter()
            .map(|h| project_scissor(h.vp_scissor, view_proj, clip_w, clip_h))
            .collect();
    }

    /// Recompute per-hatch LOD skip flags. A hatch whose entire AABB
    /// projects to less than ~2 px is invisible at this zoom — skip the
    /// draw call entirely instead of running the polygon test for every
    /// fragment in the bounding quad. Pairs with the shader-side
    /// solid-fill substitution for hatches between 2 px and the dense
    /// pattern threshold.
    pub fn compute_hatch_lod(&mut self, view_proj: glam::Mat4, clip_w: u32, clip_h: u32) {
        self.hatch_skip_flags = self
            .gpu_hatches
            .iter()
            .map(|h| aabb_below_pixel(h.world_aabb, view_proj, clip_w, clip_h, 2.0))
            .collect();
    }

    /// Recompute pixel scissor rects for viewport-clipped wipeouts.
    pub fn compute_wipeout_scissors(&mut self, view_proj: glam::Mat4, clip_w: u32, clip_h: u32) {
        self.wipeout_pixel_scissors = self
            .gpu_wipeouts
            .iter()
            .map(|h| project_scissor(h.vp_scissor, view_proj, clip_w, clip_h))
            .collect();
    }

    /// Recompute pixel scissor rects for viewport-clipped raster images.
    pub fn compute_image_scissors(&mut self, view_proj: glam::Mat4, clip_w: u32, clip_h: u32) {
        self.image_pixel_scissors = self
            .gpu_images
            .iter()
            .map(|i| project_scissor(i.vp_scissor, view_proj, clip_w, clip_h))
            .collect();
    }

    /// Upload all 3DFACE entities as two batched GPU objects:
    /// - `gpu_face3d_fill`: filled triangles (1 buffer, 1 draw call)
    /// - `gpu_face3d_edges`: merged edge wires (1 buffer, 1 draw call)
    pub fn upload_face3d(
        &mut self,
        device: &wgpu::Device,
        face3d_wires: &[WireModel],
        all_wires: &[WireModel],
    ) {
        let has_fills =
            !face3d_wires.is_empty() || all_wires.iter().any(|w| !w.fill_tris.is_empty());
        if !has_fills {
            self.gpu_face3d_fill = None;
            self.gpu_face3d_edges = vec![];
            return;
        }
        self.gpu_face3d_fill = Some(Face3DGpu::from_wires(device, face3d_wires, all_wires));
        self.gpu_face3d_edges = WireGpu::from_batch(device, face3d_wires);
    }

    pub fn upload_meshes(&mut self, device: &wgpu::Device, meshes: &[MeshLodSet]) {
        self.gpu_meshes = meshes
            .iter()
            .filter(|s| s.lods.iter().any(|m| !m.indices.is_empty()))
            .map(|s| MeshLodGpu::new(device, s))
            .collect();
    }

    /// Per-frame mesh LOD selector. Picks slot 0/1/2 based on the
    /// projected pixel diagonal of each mesh's `world_aabb` (Phase 3.4
    /// ladder: >200 px → 0, 50–200 → 1, <50 → 2). Falls back to the
    /// nearest available lower slot when a level wasn't generated.
    pub fn compute_mesh_lod(&mut self, view_proj: glam::Mat4, clip_w: u32, clip_h: u32) {
        self.mesh_lod_levels = self
            .gpu_meshes
            .iter()
            .map(|m| pick_mesh_lod(m, view_proj, clip_w, clip_h))
            .collect();
        // Phase 2.2 — frustum-visibility flag per mesh. Cheap: same
        // 4-corner projection used for LOD selection, just answering a
        // different question (any corner inside the viewport rect?).
        self.mesh_visible = self
            .gpu_meshes
            .iter()
            .map(|m| !aabb_offscreen(m.world_aabb, view_proj, clip_w, clip_h))
            .collect();
    }

    pub fn upload_hatches(&mut self, device: &wgpu::Device, hatches: &[HatchModel]) {
        self.gpu_hatches = hatches
            .iter()
            .filter(|h| h.boundary.len() >= 3)
            .map(|h| HatchGpu::new(device, h, &self.hatch_bgl1))
            .collect();
    }

    pub fn upload_wipeouts(&mut self, device: &wgpu::Device, wipeouts: &[HatchModel]) {
        self.gpu_wipeouts = wipeouts
            .iter()
            .filter(|h| h.boundary.len() >= 3)
            .map(|h| HatchGpu::new(device, h, &self.hatch_bgl1))
            .collect();
    }

    pub fn upload_images(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        images: &[ImageModel],
    ) {
        self.gpu_images = images
            .iter()
            .filter_map(|m| ImageGpu::new(device, queue, m, &self.image_bgl1))
            .collect();
    }

    pub fn upload_uniforms(&self, queue: &wgpu::Queue, uniforms: &Uniforms) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));
    }

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: Rectangle<u32>,
        bg_color: [f32; 4],
    ) {
        let vp = clip_bounds;
        let msaa = &self.msaa_view;
        let [r, g, b, a] = bg_color;
        let clear_color = wgpu::Color {
            r: r as f64,
            g: g as f64,
            b: b as f64,
            a: a as f64,
        };

        // ── Pass 1: hatch fills ────────────────────────────────────────────
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hatch.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Clear MSAA to background color on the first pass.
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            // MSAA texture is clip-bounds-sized, so viewport starts at (0, 0).
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            if !self.gpu_hatches.is_empty() {
                pass.set_pipeline(&self.hatch_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                let mut scissor_active = false;
                for (i, hatch) in self.gpu_hatches.iter().enumerate() {
                    // Phase 3.3 LOD: skip hatches projecting to sub-pixel size.
                    if self.hatch_skip_flags.get(i).copied().unwrap_or(false) {
                        continue;
                    }
                    match self.hatch_pixel_scissors.get(i) {
                        Some(Some([x, y, w, h])) => {
                            pass.set_scissor_rect(*x, *y, *w, *h);
                            scissor_active = true;
                        }
                        _ if scissor_active => {
                            pass.set_scissor_rect(0, 0, vp.width, vp.height);
                            scissor_active = false;
                        }
                        _ => {}
                    }
                    pass.set_bind_group(1, &hatch.bind_group, &[]);
                    pass.set_vertex_buffer(0, hatch.vertex_buffer.slice(..));
                    pass.draw(0..6, 0..1);
                }
                if scissor_active {
                    pass.set_scissor_rect(0, 0, vp.width, vp.height);
                }
            }
        }

        // ── Pass 2: raster images ─────────────────────────────────────────
        if !self.gpu_images.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("image.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.image_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            let mut scissor_active = false;
            for (i, img) in self.gpu_images.iter().enumerate() {
                match self.image_pixel_scissors.get(i) {
                    Some(Some([x, y, w, h])) => {
                        pass.set_scissor_rect(*x, *y, *w, *h);
                        scissor_active = true;
                    }
                    _ if scissor_active => {
                        pass.set_scissor_rect(0, 0, vp.width, vp.height);
                        scissor_active = false;
                    }
                    _ => {}
                }
                pass.set_bind_group(1, &img.bind_group, &[]);
                pass.set_vertex_buffer(0, img.vertex_buffer.slice(..));
                pass.draw(0..6, 0..1);
            }
            if scissor_active {
                pass.set_scissor_rect(0, 0, vp.width, vp.height);
            }
        }

        // ── Pass 4: solid meshes ──────────────────────────────────────────
        if !self.gpu_meshes.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mesh.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.mesh_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            for (i, set) in self.gpu_meshes.iter().enumerate() {
                if !self.mesh_visible.get(i).copied().unwrap_or(true) {
                    continue;
                }
                let level = self
                    .mesh_lod_levels
                    .get(i)
                    .copied()
                    .unwrap_or(0)
                    .min(set.lods.len().saturating_sub(1));
                let Some(mesh) = set.lods.get(level) else {
                    continue;
                };
                if mesh.index_count == 0 {
                    continue;
                }
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        // ── Pass 5a: 3DFACE fills ─────────────────────────────────────────
        if let Some(ref fill) = self.gpu_face3d_fill {
            if fill.vertex_count > 0 {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("face3d.render_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: msaa,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
                pass.set_pipeline(&self.face3d_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, fill.vertex_buffer.slice(..));
                pass.draw(0..fill.vertex_count, 0..1);
            }
        }

        // ── Pass 5b: 3DFACE edges (batched, possibly multiple chunks) ────
        if !self.gpu_face3d_edges.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("face3d_edges.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.wire_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            for edges in &self.gpu_face3d_edges {
                if edges.vertex_count >= 6 {
                    pass.set_vertex_buffer(0, edges.vertex_buffer.slice(..));
                    pass.draw(0..edges.vertex_count, 0..1);
                }
            }
        }

        // ── Pass 5: wires ─────────────────────────────────────────────────
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wire.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.wire_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            let mut scissor_active = false;
            for (i, wire) in self.gpu_wires.iter().enumerate() {
                if wire.vertex_count < 6 {
                    continue;
                }
                match self.wire_pixel_scissors.get(i) {
                    Some(Some([x, y, w, h])) => {
                        pass.set_scissor_rect(*x, *y, *w, *h);
                        scissor_active = true;
                    }
                    _ if scissor_active => {
                        pass.set_scissor_rect(0, 0, vp.width, vp.height);
                        scissor_active = false;
                    }
                    _ => {}
                }
                pass.set_vertex_buffer(0, wire.vertex_buffer.slice(..));
                pass.draw(0..wire.vertex_count, 0..1);
            }
            if scissor_active {
                pass.set_scissor_rect(0, 0, vp.width, vp.height);
            }
        }

        // ── Pass 6: wipeout fills (drawn after wires to mask them) ────────
        if !self.gpu_wipeouts.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wipeout.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.hatch_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            let mut scissor_active = false;
            for (i, wipeout) in self.gpu_wipeouts.iter().enumerate() {
                match self.wipeout_pixel_scissors.get(i) {
                    Some(Some([x, y, w, h])) => {
                        pass.set_scissor_rect(*x, *y, *w, *h);
                        scissor_active = true;
                    }
                    _ if scissor_active => {
                        pass.set_scissor_rect(0, 0, vp.width, vp.height);
                        scissor_active = false;
                    }
                    _ => {}
                }
                pass.set_bind_group(1, &wipeout.bind_group, &[]);
                pass.set_vertex_buffer(0, wipeout.vertex_buffer.slice(..));
                pass.draw(0..6, 0..1);
            }
            if scissor_active {
                pass.set_scissor_rect(0, 0, vp.width, vp.height);
            }
        }

        // ── Pass 7: selected wire overlay pass ───────────────────────────
        // Redraws selected wires with depth_compare=Always so they appear on
        // top of all other geometry at full brightness.
        if !self.gpu_selected_wires.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wire_xray.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.wire_xray_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            for wire in &self.gpu_selected_wires {
                if wire.vertex_count >= 6 {
                    pass.set_vertex_buffer(0, wire.vertex_buffer.slice(..));
                    pass.draw(0..wire.vertex_count, 0..1);
                }
            }
        }

        // ── Resolve MSAA → clip-sized resolve texture ─────────────────────
        // Both msaa_view and resolve_view are sized to clip_bounds, so the
        // resolve does NOT touch any pixels outside the shader widget's area.
        {
            let _resolve = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("msaa.resolve_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: Some(&self.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            // No draw calls — the pass itself triggers the MSAA resolve.
        }

        // ── Blit resolve texture → surface target at clip_bounds position ──
        // The viewport maps the full-screen NDC quad to exactly clip_bounds
        // in the surface, leaving all other widgets untouched.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(
                vp.x as f32,
                vp.y as f32,
                vp.width as f32,
                vp.height as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.blit_bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    pub fn ensure_depth_texture(&mut self, device: &wgpu::Device, size: Size<u32>) {
        if self.depth_texture_size != size {
            let depth_tex = create_depth_texture(device, size);
            self.depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let msaa_tex = create_msaa_texture(device, size, self.surface_format);
            self.msaa_view = msaa_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let resolve_tex = create_resolve_texture(device, size, self.surface_format);
            let resolve_view = resolve_tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blit.bind_group"),
                layout: &self.blit_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&resolve_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                    },
                ],
            });
            self.resolve_view = resolve_view;
            self.depth_texture_size = size;
        }
    }
}

/// Project a mesh's `world_aabb` and pick a LOD slot:
///   diagonal > 200 px → 0 (HIGH)
///   diagonal 50–200 px → 1 (MID)
///   diagonal < 50 px   → 2 (LOW)
/// When the picked slot is missing from `gpu_meshes[i].lods`, walks down
/// to the next available coarser/finer level so the mesh always renders.
fn pick_mesh_lod(
    mesh: &MeshLodGpu,
    view_proj: glam::Mat4,
    clip_w: u32,
    clip_h: u32,
) -> usize {
    let diag_px = aabb_diagonal_pixels(mesh.world_aabb, view_proj, clip_w, clip_h);
    let target = if diag_px > 200.0 {
        0
    } else if diag_px > 50.0 {
        1
    } else {
        2
    };
    // Walk down to nearest available LOD (some entities won't have all 3).
    for level in (0..=target).rev() {
        if mesh.lods.get(level).is_some() {
            return level;
        }
    }
    0
}

fn aabb_diagonal_pixels(
    aabb: [f32; 4],
    view_proj: glam::Mat4,
    clip_w: u32,
    clip_h: u32,
) -> f32 {
    let [x0, y0, x1, y1] = aabb;
    if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
        return f32::INFINITY;
    }
    let w = clip_w as f32;
    let h = clip_h as f32;
    let corners = [
        view_proj.project_point3(glam::Vec3::new(x0, y0, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x1, y0, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x0, y1, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x1, y1, 0.0)),
    ];
    let mut min_px = f32::INFINITY;
    let mut max_px = f32::NEG_INFINITY;
    let mut min_py = f32::INFINITY;
    let mut max_py = f32::NEG_INFINITY;
    for c in &corners {
        let px = (c.x + 1.0) * 0.5 * w;
        let py = (1.0 - c.y) * 0.5 * h;
        if px < min_px { min_px = px; }
        if px > max_px { max_px = px; }
        if py < min_py { min_py = py; }
        if py > max_py { max_py = py; }
    }
    let dx = max_px - min_px;
    let dy = max_py - min_py;
    (dx * dx + dy * dy).sqrt()
}

/// `true` when the world-XY AABB projects entirely outside the
/// viewport rect. Phase 2.2 mesh-frustum cull. Equivalent to a 2D
/// bounding-box rejection test in NDC; same 4-corner projection used
/// for LOD picking, so the extra cost is negligible.
fn aabb_offscreen(
    aabb: [f32; 4],
    view_proj: glam::Mat4,
    clip_w: u32,
    clip_h: u32,
) -> bool {
    let [x0, y0, x1, y1] = aabb;
    if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
        return false;
    }
    let w = clip_w as f32;
    let h = clip_h as f32;
    let corners = [
        view_proj.project_point3(glam::Vec3::new(x0, y0, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x1, y0, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x0, y1, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x1, y1, 0.0)),
    ];
    let mut min_px = f32::INFINITY;
    let mut max_px = f32::NEG_INFINITY;
    let mut min_py = f32::INFINITY;
    let mut max_py = f32::NEG_INFINITY;
    for c in &corners {
        let px = (c.x + 1.0) * 0.5 * w;
        let py = (1.0 - c.y) * 0.5 * h;
        if px < min_px { min_px = px; }
        if px > max_px { max_px = px; }
        if py < min_py { min_py = py; }
        if py > max_py { max_py = py; }
    }
    // Reject if projected AABB sits fully to one side of the viewport.
    max_px < 0.0 || min_px > w || max_py < 0.0 || min_py > h
}

/// Return `true` when the world-XY AABB's screen-space size is below the
/// given pixel threshold. Used by LOD passes (hatch skip, etc.) to drop
/// draw calls that wouldn't contribute a visible pixel.
fn aabb_below_pixel(
    aabb: [f32; 4],
    view_proj: glam::Mat4,
    clip_w: u32,
    clip_h: u32,
    threshold_px: f32,
) -> bool {
    let [x0, y0, x1, y1] = aabb;
    if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
        return false;
    }
    let w = clip_w as f32;
    let h = clip_h as f32;
    let corners = [
        view_proj.project_point3(glam::Vec3::new(x0, y0, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x1, y0, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x0, y1, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x1, y1, 0.0)),
    ];
    let mut min_px = f32::INFINITY;
    let mut max_px = f32::NEG_INFINITY;
    let mut min_py = f32::INFINITY;
    let mut max_py = f32::NEG_INFINITY;
    for c in &corners {
        let px = (c.x + 1.0) * 0.5 * w;
        let py = (1.0 - c.y) * 0.5 * h;
        if px < min_px { min_px = px; }
        if px > max_px { max_px = px; }
        if py < min_py { min_py = py; }
        if py > max_py { max_py = py; }
    }
    (max_px - min_px).max(max_py - min_py) < threshold_px
}

/// Project a world-space XY scissor rect through `view_proj` into the four
/// pixel-space corners and return the smallest aligned-rect that bounds them,
/// clamped to the clip viewport. Returns `None` when the rect is missing or
/// the projection collapses (off-screen, behind camera).
fn project_scissor(
    rect: Option<[f32; 4]>,
    view_proj: glam::Mat4,
    clip_w: u32,
    clip_h: u32,
) -> Option<[u32; 4]> {
    let [x0, y0, x1, y1] = rect?;
    let w = clip_w as f32;
    let h = clip_h as f32;
    let corners = [
        view_proj.project_point3(glam::Vec3::new(x0, y0, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x1, y0, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x0, y1, 0.0)),
        view_proj.project_point3(glam::Vec3::new(x1, y1, 0.0)),
    ];
    let px: Vec<f32> = corners.iter().map(|c| (c.x + 1.0) * 0.5 * w).collect();
    let py: Vec<f32> = corners.iter().map(|c| (1.0 - c.y) * 0.5 * h).collect();
    let sx0 = px.iter().cloned().fold(f32::INFINITY, f32::min).max(0.0) as u32;
    let sy0 = py.iter().cloned().fold(f32::INFINITY, f32::min).max(0.0) as u32;
    let sx1 = (px.iter().cloned().fold(f32::NEG_INFINITY, f32::max) as u32).min(clip_w);
    let sy1 = (py.iter().cloned().fold(f32::NEG_INFINITY, f32::max) as u32).min(clip_h);
    if sx1 <= sx0 || sy1 <= sy0 {
        return None;
    }
    Some([sx0, sy0, sx1 - sx0, sy1 - sy0])
}

fn create_depth_texture(device: &wgpu::Device, size: Size<u32>) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewer.depth_texture"),
        size: wgpu::Extent3d {
            width: size.width.max(1),
            height: size.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: MSAA_SAMPLES,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn create_resolve_texture(
    device: &wgpu::Device,
    size: Size<u32>,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewer.resolve_texture"),
        size: wgpu::Extent3d {
            width: size.width.max(1),
            height: size.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_msaa_texture(
    device: &wgpu::Device,
    size: Size<u32>,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewer.msaa_texture"),
        size: wgpu::Extent3d {
            width: size.width.max(1),
            height: size.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: MSAA_SAMPLES,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

impl iced::widget::shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::new(device, queue, format)
    }
}
