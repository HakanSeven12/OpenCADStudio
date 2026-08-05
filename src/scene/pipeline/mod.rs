mod device_capabilities;
pub mod face3d_gpu;
pub mod hatch_gpu;
pub mod wipeout_gpu;
pub mod image_gpu;
pub mod mesh_gpu;
pub mod text_gpu;
pub mod uniforms;
pub mod viewcube;
/// Persistent per-entity wire instance arena. Its indexed-storage and packed
/// adapters share the same patch/cull lifecycle across native, WebGPU, and
/// WebGL2.
pub mod wire_arena;
pub mod wire_gpu;

use iced::wgpu;
use iced::wgpu::util::DeviceExt;
use iced::{Rectangle, Size};

pub use face3d_gpu::Face3DGpu;
pub use wipeout_gpu::WipeoutGpu;
pub use image_gpu::ImageGpu;
pub use uniforms::Uniforms;
pub use viewcube::ViewCubePipeline;
pub use wire_gpu::WireGpu;

use crate::scene::model::hatch_model::HatchModel;
use crate::scene::model::image_model::ImageModel;
use crate::scene::model::mesh_model::MeshLodSet;
use crate::scene::model::wire_model::WireModel;
use device_capabilities::DeviceCapabilities;

/// MSAA sample count for the main drawing pipelines.
const MSAA_SAMPLES: u32 = 4;
const SHADOW_MAP_SIZE: u32 = 2048;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MeshHighlightKind {
    Selected,
    Hover,
}

#[derive(Clone, Copy)]
struct MeshResidentRange {
    chunk: usize,
    dynamic: bool,
    index_start: u32,
    index_count: u32,
    transparent: bool,
    instance_start: u32,
    instance_count: u32,
}

#[derive(Clone, Copy)]
struct MeshHighlightDraw {
    range: MeshResidentRange,
    kind: MeshHighlightKind,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshCullItem {
    min: [f32; 4],
    max: [f32; 4],
    counts: [u32; 4],
    meta: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshCullUniform {
    view_rot: [f32; 16],
    eye: [f32; 4],
    count: [u32; 4],
}

pub struct Pipeline {
    background_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    wire_pipeline: wgpu::RenderPipeline,
    /// Stamps clip-boundary polygons into the stencil buffer (viewports + XCLIP).
    clip_mask_pipeline: wgpu::RenderPipeline,
    /// Black-fragment variant of `wire_pipeline` for 3D mesh outline edges in
    /// filled render modes.
    wire_black_pipeline: wgpu::RenderPipeline,
    /// Same shader as wire_pipeline but depth_compare=Greater, depth_write_enabled=false.
    /// Used to draw ghost copies of selected wires through occluding geometry.
    wire_xray_pipeline: wgpu::RenderPipeline,
    /// Layout for the per-wire `WireConst` storage buffer (group 1 of the wire /
    /// xray pipelines). `Some` on any storage-capable device; `None` in packed
    /// compatibility mode. Passed to `WireGpu::from_run` / `from_batch`.
    pub(crate) wire_const_bgl: Option<wgpu::BindGroupLayout>,
    wipeout_pipeline: wgpu::RenderPipeline,
    /// Capability-selected hatch renderer. Storage and texture transports are
    /// private backends behind one upload/LOD/draw lifecycle.
    hatch_gpu: hatch_gpu::HatchGpu,
    image_pipeline: wgpu::RenderPipeline,
    /// SDF text-quad pipeline (Phase 2b): draws per-glyph quads sampling the
    /// shared glyph atlas. Fed only when `OCS_TEXT_SDF` is set (else no verts).
    text_pipeline: wgpu::RenderPipeline,
    /// Depth-independent variant used by selection / rollover highlighting.
    text_highlight_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    /// Depth-write-disabled variant of `mesh_pipeline` for non-opaque solids.
    mesh_transparent_pipeline: wgpu::RenderPipeline,
    mesh_selected_pipeline: wgpu::RenderPipeline,
    mesh_hover_pipeline: wgpu::RenderPipeline,
    /// Wireframe variant of the mesh pipeline (LineList topology, same
    /// vertex layout / shader). Used when the active render mode is
    /// Wireframe 2D or Wireframe 3D so 3D solids draw as their
    /// triangle edges instead of filled faces.
    mesh_wireframe_pipeline: wgpu::RenderPipeline,
    /// Edge pipeline that forces black, for the edge overlay in filled modes.
    mesh_edge_black_pipeline: wgpu::RenderPipeline,
    /// Depth-only variant of the mesh pipeline (TriangleList, no color
    /// writes, writes depth). Used in HiddenLine mode so 3D solids
    /// occlude wires behind them without painting visible pixels.
    mesh_depth_pipeline: wgpu::RenderPipeline,
    mesh_material_bgl: wgpu::BindGroupLayout,
    mesh_default_material_bind_group: wgpu::BindGroup,
    mesh_cull_pipeline: Option<wgpu::ComputePipeline>,
    mesh_cull_bgl: Option<wgpu::BindGroupLayout>,
    mesh_cull_uniform: Option<wgpu::Buffer>,
    mesh_cull_items: Option<wgpu::Buffer>,
    mesh_cull_bind_group: Option<wgpu::BindGroup>,
    mesh_opaque_indirect: Option<wgpu::Buffer>,
    mesh_transparent_indirect: Option<wgpu::Buffer>,
    mesh_wire_indirect: Option<wgpu::Buffer>,
    mesh_edge_indirect: Option<wgpu::Buffer>,
    mesh_cull_count: u32,
    face3d_pipeline: wgpu::RenderPipeline,
    /// Depth-only variant of the face3d pipeline (no color writes,
    /// writes depth). Paired with `mesh_depth_pipeline` for HiddenLine.
    face3d_depth_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    frame_bgl: wgpu::BindGroupLayout,
    uniform_bind_group: wgpu::BindGroup,
    shadow_uniform_bind_group: wgpu::BindGroup,
    background_texture: wgpu::Texture,
    environment_texture: wgpu::Texture,
    background_sampler: wgpu::Sampler,
    background_source_id: usize,
    environment_source_id: usize,
    _shadow_texture: wgpu::Texture,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    shadow_enabled: bool,
    wipeout_bgl1: wgpu::BindGroupLayout,
    image_bgl1: wgpu::BindGroupLayout,
    /// Group-1 layout for the text pipeline (atlas texture + sampler).
    text_atlas_bgl: wgpu::BindGroupLayout,
    /// GPU glyph atlas (texture + sampler + bind group). Rebuilt when the shared
    /// CPU atlas grows (new glyphs baked). `None` until the first text upload.
    text_atlas_gpu: Option<text_gpu::TextAtlasGpu>,
    /// All glyph-quad vertices for the frame, one buffer, `None` when empty.
    text_vbuf: Option<wgpu::Buffer>,
    text_vcount: u32,
    /// Tinted glyph quads of just the selected / hovered text, drawn over the
    /// base text pass so a selection / rollover recolours the glyphs (the text
    /// analogue of the selected-wire xray overlay). Rebuilt on selection change.
    text_highlight_vbuf: Option<wgpu::Buffer>,
    text_highlight_vcount: u32,
    /// Live grip-drag / command-preview SDF glyph quads. The base text buffer
    /// only re-uploads on a geometry-epoch change, but a grip drag hides the
    /// dragged entity from the base wire set and shows it as a per-frame
    /// overlay — so its glyphs ride here, re-uploaded every frame like
    /// `gpu_preview_wires`, or the dragged text vanishes until release re-tesses
    /// it back into the base set (issue #316).
    text_preview_vbuf: Option<wgpu::Buffer>,
    text_preview_vcount: u32,
    /// Per-frame DISPSILH silhouette line list — rebuilt every prepare() from
    /// the mesh sets' curved-face generators and the current view direction, so
    /// the outline tracks the camera. Reuses the mesh vertex format / pipeline.
    silhouette_vbuf: Option<wgpu::Buffer>,
    silhouette_vcount: u32,
    /// Last requested render size (the full viewport rect, in pixels). The
    /// geometry passes render at this size; the blit UV is scaled by
    /// `depth_texture_size / alloc_size` so it samples only the filled region.
    depth_texture_size: Size<u32>,
    /// Actual allocated size of the depth / MSAA / resolve textures. Rounded
    /// up from the requested size to a coarse grid so a divider drag (which
    /// changes the pane size a few pixels every frame) doesn't recreate these
    /// textures every frame. Recreation is what hangs the ANGLE → D3D11 path
    /// on Windows-Firefox (issue #191); grow-only bucketing makes it rare.
    alloc_size: Size<u32>,
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
    /// UV transform (offset + scale) consumed by the blit shader so a
    /// partially off-canvas viewport still composites the right portion of
    /// its resolve texture to the visible portion of the surface.
    blit_uniform_buffer: wgpu::Buffer,
    /// Cached texture format (needed to recreate MSAA / depth textures on resize).
    surface_format: wgpu::TextureFormat,
    /// The resident wire batches, shared by `std::sync::Arc` with every other
    /// pipeline slot rendering the *same* `wire_content_id` (see
    /// `MultiPipeline::wire_buffer_cache`). A `wgpu::Buffer` is internally
    /// ref-counted, so N paper viewports (or Model tiles) drawing one identical
    /// resident set hold one copy of the GPU vertex buffers between them —
    /// their camera uniforms + scissor stay per-slot, only the geometry is
    /// deduplicated. Never mutated in place; an arena-backed slot may also
    /// replace this thin draw-range list on camera changes without touching the
    /// shared resident buffer.
    pub(crate) gpu_wires: std::sync::Arc<Vec<WireGpu>>,
    /// Persistent per-entity wire instance arena (capability-selected format).
    /// When active, `gpu_wires` is a thin wrapper over this arena's buffers and an
    /// edit patches one entity's slab in place instead of rebuilding every wire.
    pub(crate) wire_arena: Option<wire_arena::PersistentWireArena>,
    /// Second arena for the mesh/solid EDGE wires (drawn with the mesh-edge skip /
    /// black treatment); the resident set is split into this + `wire_arena` so
    /// both patch incrementally. Shares `wire_arena_id`.
    pub(crate) wire_arena_mesh: Option<wire_arena::PersistentWireArena>,
    /// Chunked resident buffers for whichever arena partition exceeded one
    /// GPU buffer. `Some(false)` = regular wires, `Some(true)` = mesh edges.
    pub(crate) wire_arena_fallback: std::sync::Arc<Vec<WireGpu>>,
    pub(crate) wire_arena_fallback_kind: Option<bool>,
    pub(crate) wire_arena_fallback_handles: rustc_hash::FxHashSet<acadrust::Handle>,
    /// The Model content id both arenas currently mirror (`u64::MAX` = none).
    pub(crate) wire_arena_id: u64,
    /// Last content/camera/viewport tuple used to derive visible instance
    /// ranges from the resident arena.
    pub(crate) wire_cull_key: (u64, u64, u32, u32),
    /// View/source keys for CPU visibility passes. A plain entity edit changes
    /// the scene render signature, but it must not rescan every unchanged hatch,
    /// wipeout, or mesh AABB when the camera and corresponding source stayed
    /// identical.
    pub(crate) hatch_lod_key: (usize, u64, u32, u32, bool),
    pub(crate) wipeout_lod_key: (usize, u64, u32, u32, bool),
    pub(crate) mesh_lod_key: (usize, u64, u32, u32),
    /// This content viewport's non-rectangular clip boundary as a triangle-fan
    /// vertex buffer in the render target's normalized device coords (`None` =
    /// rectangular / unclipped, where the viewport's own render rectangle does
    /// the clipping). Stamped into the stencil once per frame; every content
    /// pass then draws with stencil reference 1 so only the interior survives.
    clip_boundary: Option<(wgpu::Buffer, u32)>,
    /// Ghost copies (25% alpha) of selected wires for the X-ray depth pass.
    gpu_selected_wires: Vec<WireGpu>,
    /// Command-preview / interim / grip-drag overlay wires. Re-uploaded every
    /// frame they are present (small), drawn on top of the base wire pass — so
    /// a live drag never re-uploads the resident base buffer.
    gpu_preview_wires: Vec<WireGpu>,
    /// Wipeout masks — solid fills rendered after wires in a separate pass via
    /// the legacy per-primitive `WipeoutGpu` renderer.
    gpu_wipeouts: Vec<WipeoutGpu>,
    /// Per-wipeout draw-time skip flag (Phase 2.3 frustum cull). `true`
    /// when the wipeout's projected AABB sits entirely outside the
    /// viewport rect. Recomputed by `compute_wipeout_lod`.
    wipeout_skip_flags: Vec<bool>,
    gpu_images: Vec<ImageGpu>,
    /// Batched mesh geometry — every solid's LOD0 concatenated into a few large
    /// buffers so the whole set draws in a handful of calls instead of one per
    /// solid. Hover / selection never re-pack it.
    gpu_mesh_batch: Vec<mesh_gpu::MeshBatchChunk>,
    /// Small mutable working set rebuilt after entity-only edits. Static chunks
    /// touched by an edit are disabled wholesale and their current handles are
    /// repacked here, keeping the rest of a multi-million-triangle scene resident.
    gpu_mesh_dynamic: Vec<mesh_gpu::MeshBatchChunk>,
    mesh_disabled_chunks: rustc_hash::FxHashSet<usize>,
    mesh_dynamic_handles: rustc_hash::FxHashSet<acadrust::Handle>,
    /// Wire content generation whose geometry the static+dynamic mesh state
    /// mirrors. It gates replay of the same per-entity journal handoff.
    pub cached_mesh_content_id: u64,
    /// Draw ranges for each entity inside the resident mesh chunks. Highlight
    /// overlays reuse these buffers instead of uploading duplicate geometry.
    mesh_ranges_by_handle:
        rustc_hash::FxHashMap<acadrust::Handle, Vec<MeshResidentRange>>,
    mesh_highlight_draws: Vec<MeshHighlightDraw>,
    /// `(geometry_epoch, selection_generation)` the highlight overlay was built for.
    pub cached_highlight_key: (u64, u64),
    /// Batched 3DFACE fill (all faces in one buffer) and edges (merged wire).
    gpu_face3d_fill: Option<Face3DGpu>,
    gpu_face3d_edges: Vec<WireGpu>,
    pub viewcube: ViewCubePipeline,
    /// Strong source guards for category-specific GPU uploads. Holding the old
    /// Arc makes pointer identity ABA-safe: an unchanged category reuses the
    /// same Arc even when an unrelated entity advances `geometry_epoch`.
    pub cached_hatch_source: Option<std::sync::Arc<Vec<HatchModel>>>,
    pub cached_preview_hatch_source: Option<std::sync::Arc<Vec<HatchModel>>>,
    pub cached_wipeout_source: Option<std::sync::Arc<Vec<HatchModel>>>,
    pub cached_image_source: Option<std::sync::Arc<Vec<ImageModel>>>,
    pub cached_text_source: Option<std::sync::Arc<Vec<text_gpu::TextVertex>>>,
    pub cached_annotation_highlight_source: Option<std::sync::Arc<Vec<WireModel>>>,
    pub cached_mesh_source: Option<std::sync::Arc<Vec<MeshLodSet>>>,
    pub cached_face3d_source: Option<std::sync::Arc<Vec<WireModel>>>,
    pub cached_face3d_depth_source:
        Option<std::sync::Weak<rustc_hash::FxHashMap<u64, [f32; 2]>>>,
    pub cached_fill_mode: bool,
    /// Last `(geometry_epoch, camera_generation)` value for which GPU buffers
    /// were uploaded. We re-upload when either side changes — pan/zoom bumps
    /// camera_generation, which triggers re-culling and a fresh upload.
    /// `(geometry_epoch, camera_generation, selection_generation)` of the
    /// last static-buffer upload. Selection is tracked so the selected-hatch
    /// tint refreshes on select / deselect (issue #71).
    pub cached_epoch: (u64, u64, u64),
    /// Content id of the wire buffer currently resident on the GPU (Phase
    /// 3.2). When the incoming `ViewportData.wire_content_id` matches, the
    /// world-space wire vertices are unchanged (e.g. a pure pan reused the
    /// Model-tile tessellation) and `upload_wires` is skipped. `u64::MAX` =
    /// nothing uploaded yet.
    pub cached_wire_id: u64,
    /// `(wire_content_id, selection_generation)` the selection xray overlay
    /// (`gpu_selected_wires`) was last built for. Rebuilt when either changes —
    /// a pick bumps only `selection_generation`, refreshing the overlay without
    /// touching the main wire buffers.
    pub cached_selection: (u64, u64),
    /// `(wire_content_id, face3d_fill_active, show_2d_solid_fills)` the Face3D
    /// edge/fill buffers were uploaded for. A stable content id avoids retaining
    /// the resident wire Arc:
    /// that Arc must stay uniquely owned by Scene so a small edit can splice it
    /// in place instead of rebuilding the whole drawing.
    pub cached_face3d_key: (u64, bool, bool),
    /// Handle → indices into the resident wire set, built once per wire upload
    /// (when `cached_wire_id` changes). Lets the selection/hover xray overlay
    /// gather just the highlighted entity's wires (`O(highlighted)`) instead of
    /// scanning and string-parsing every wire on each hover change. Shared by
    /// `std::sync::Arc` alongside `gpu_wires` — built once per `wire_content_id`
    /// and cloned into every slot that renders it.
    pub(crate) wire_handle_index: std::sync::Arc<rustc_hash::FxHashMap<u64, Vec<u32>>>,
    /// Signature of the last frame's rendered scene (camera uniforms, geometry
    /// / selection generations, wire content id, render-mode flags, clip size,
    /// live preview). When the next frame's signature matches, the image is
    /// pixel-identical, so `render` skips every geometry pass + the MSAA
    /// resolve and only re-blits the resolve texture (which still holds it).
    /// This turns a pure cursor move over a large drawing from an O(N)
    /// re-rasterization into a single fullscreen blit. `u64::MAX` = nothing
    /// rendered yet (forces the first frame to render fully).
    pub render_sig: u64,
    /// Set by `prepare` each frame: `true` when this frame's signature matched
    /// `render_sig`, so `render` may skip the geometry passes. Read (never
    /// written) by `render`, which always runs after `prepare` for the frame.
    pub skip_geometry: bool,
    /// Interaction LOD: set by `prepare` when the view is actively navigating, so
    /// `render` skips the (per-pixel, GPU-dominating) hatch pass this frame. The
    /// scene-render cache holds the full-quality frame once the view settles.
    pub skip_hatch_frame: bool,
    /// Stable identity of the viewport that last used this (index-addressed)
    /// pipeline slot. The renderer's viewport list drops off-canvas viewports,
    /// so a slot can be reused by a *different* viewport across frames; when the
    /// occupant changes, all cache keys above belong to the previous one and are
    /// reset so every buffer re-uploads. `u64::MAX` = never used.
    pub slot_id: u64,
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
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_frame_bgl = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("shadow.frame_bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            },
        );

        let placeholder_texture = |label: &'static str, rgba: [u8; 4]| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            texture
        };
        let background_texture = placeholder_texture("background.placeholder", [0, 0, 0, 255]);
        let environment_texture =
            placeholder_texture("environment.placeholder", [128, 128, 128, 255]);
        let background_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("background.sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow.texture"),
            size: wgpu::Extent3d {
                width: SHADOW_MAP_SIZE,
                height: SHADOW_MAP_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let background_view = background_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let environment_view = environment_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer.bind_group"),
            layout: &frame_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&background_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&background_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&environment_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&background_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });
        let shadow_uniform_bind_group = device.create_bind_group(
            &wgpu::BindGroupDescriptor {
                label: Some("shadow.frame_bind_group"),
                layout: &shadow_frame_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                }],
            },
        );

        // ── Wire pipeline ──────────────────────────────────────────────────
        // Select once from actual device limits. Any device whose compositor
        // exposes the required storage limits uses the storage renderer; all
        // other devices use the packed/texture compatibility renderer.
        let device_caps = DeviceCapabilities::detect(device);
        #[cfg(not(target_arch = "wasm32"))]
        let force_compat_renderer = crate::cli::gui_config().compat_renderer;
        #[cfg(target_arch = "wasm32")]
        let force_compat_renderer = false;
        let wire_mode =
            wire_gpu::WirePipelineMode::select(device_caps, force_compat_renderer);
        let wire_const_bgl = wire_mode
            .uses_storage()
            .then(|| wire_gpu::WireConst::bind_group_layout(device));
        let mut wire_bgls: Vec<Option<&wgpu::BindGroupLayout>> = vec![Some(&frame_bgl)];
        if let Some(bgl) = &wire_const_bgl {
            wire_bgls.push(Some(bgl));
        }
        let wire_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wire.pipeline_layout"),
            bind_group_layouts: &wire_bgls,
            immediate_size: 0,
        });

        let depth_tex = create_depth_texture(device, Size::new(1, 1));
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let wire_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wire.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(match wire_mode {
                wire_gpu::WirePipelineMode::IndexedStorage => {
                    include_str!("../../shaders/wire_indexed.wgsl")
                }
                wire_gpu::WirePipelineMode::Packed => include_str!("../../shaders/wire.wgsl"),
            })),
        });

        // Stencil test shared by every paper content pipeline: draw only where
        // the stencil equals the bound reference. Non-clipped content binds
        // reference 0 (matching the frame's stencil, cleared to 0); a clip
        // region's content binds reference 1 after its boundary has been stamped
        // into the stencil by the clip-mask pipeline. In Model space nothing
        // masks the stencil, so it stays 0 and reference 0 always passes.
        let content_stencil_face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Keep,
        };
        let content_stencil = wgpu::StencilState {
            front: content_stencil_face,
            back: content_stencil_face,
            read_mask: 0xff,
            write_mask: 0x00,
        };

        let background_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("background.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/background.wgsl"
            ))),
        });
        let background_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("background.pipeline_layout"),
            bind_group_layouts: &[Some(&frame_bgl)],
            immediate_size: 0,
        });
        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("background.pipeline"),
            layout: Some(&background_layout),
            vertex: wgpu::VertexState {
                module: &background_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &background_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let wire_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire.pipeline"),
            layout: Some(&wire_layout),
            vertex: wgpu::VertexState {
                module: &wire_shader,
                entry_point: Some("vs_main"),
                buffers: &[wire_mode.layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
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
            multiview_mask: None,
            cache: None,
        });

        // ── Clip-mask pipeline ─────────────────────────────────────────────
        // Stamps a viewport / XCLIP boundary polygon into the stencil buffer:
        // no colour, no depth, stencil `Invert` (even-odd fill → interior marked
        // for any polygon, convex or not). The boundary is drawn as a triangle
        // fan in paper coordinates and transformed exactly like wires.
        let clip_mask_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clip_mask.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/clip_mask.wgsl"
            ))),
        });
        let clip_mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("clip_mask.pipeline_layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let clip_mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("clip_mask.pipeline"),
            layout: Some(&clip_mask_layout),
            vertex: wgpu::VertexState {
                module: &clip_mask_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    }],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState {
                    front: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Always,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::Invert,
                    },
                    back: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Always,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::Invert,
                    },
                    read_mask: 0xff,
                    write_mask: 0xff,
                },
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &clip_mask_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        // Black variant of `wire_pipeline` — same geometry/depth, black fragment
        // — for 3D mesh outline edges in filled modes (see the wire draw loop).
        let wire_black_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire.black.pipeline"),
            layout: Some(&wire_layout),
            vertex: wgpu::VertexState {
                module: &wire_shader,
                entry_point: Some("vs_main"),
                buffers: &[wire_mode.layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &wire_shader,
                entry_point: Some("fs_black"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
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
                buffers: &[wire_mode.layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: content_stencil.clone(),
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
            multiview_mask: None,
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
        let wipeout_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wipeout.bgl1"),
            entries: &[
                hatch_entry(0, vert_frag),
                hatch_entry(1, frag),
                hatch_entry(2, frag),
            ],
        });

        let wipeout_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wipeout.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &wipeout_bgl1].map(Some),
            immediate_size: 0,
        });

        let wipeout_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wipeout.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/wipeout.wgsl"
            ))),
        });

        let wipeout_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wipeout.pipeline"),
            layout: Some(&wipeout_layout),
            vertex: wgpu::VertexState {
                module: &wipeout_shader,
                entry_point: Some("vs_main"),
                buffers: &[wipeout_gpu::HatchVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
                // Bias TOWARD the camera: a wipeout must win against geometry at
                // its own depth (a block's wipeout + shapes are coincident at
                // Z=0). A positive bias pushed the mask behind, so LessEqual
                // rejected it and the geometry showed through. Tiny enough that
                // meaningfully-nearer geometry still occludes the mask.
                bias: wgpu::DepthBiasState {
                    constant: -8,
                    slope_scale: -1.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &wipeout_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        // ── Hatch renderer ─────────────────────────────────────────────────
        // The façade owns capability selection plus both backend lifecycles.
        let hatch_gpu = hatch_gpu::HatchGpu::new(
            device,
            format,
            &frame_bgl,
            &content_stencil,
            device_caps,
            force_compat_renderer,
        );
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::var_os("RUST_LOG").is_some() {
            eprintln!(
                "renderer pipelines: wire={} hatch={} mesh={} compute-cull={} (storage buffers/stage: {})",
                if wire_mode.uses_storage() { "storage" } else { "packed" },
                hatch_gpu.backend_name(),
                if device_caps.supports_mesh_storage_instancing() { "storage" } else { "uniform" },
                if device_caps.supports_mesh_compute_culling() { "gpu" } else { "cpu" },
                device.limits().max_storage_buffers_per_shader_stage
            );
        }
        #[cfg(target_arch = "wasm32")]
        log::info!(
            "renderer pipelines: wire={} hatch={} mesh={} compute-cull={} (storage buffers/stage: {})",
            if wire_mode.uses_storage() { "storage" } else { "packed" },
            hatch_gpu.backend_name(),
            if device_caps.supports_mesh_storage_instancing() { "storage" } else { "uniform" },
            if device_caps.supports_mesh_compute_culling() { "gpu" } else { "cpu" },
            device.limits().max_storage_buffers_per_shader_stage
        );

        // ── Mesh pipeline ──────────────────────────────────────────────────
        let mesh_storage_instancing = device_caps.supports_mesh_storage_instancing();
        let mesh_source = include_str!("../../shaders/mesh.wgsl");
        let mesh_source = if mesh_storage_instancing {
            std::borrow::Cow::Borrowed(mesh_source)
        } else {
            std::borrow::Cow::Owned(mesh_source.replace(
                "var<storage, read> mesh_instances: array<MeshInstance>;",
                "var<uniform> mesh_instances: array<MeshInstance, 1>;",
            ))
        };
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh.shader"),
            source: wgpu::ShaderSource::Wgsl(mesh_source),
        });
        let (mesh_cull_bgl, mesh_cull_pipeline, mesh_cull_uniform) =
            if device_caps.supports_mesh_compute_culling() {
                let bgl =
                    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("mesh.cull.bgl"),
                        entries: &[
                            wgpu::BindGroupLayoutEntry {
                                binding: 0,
                                visibility: wgpu::ShaderStages::COMPUTE,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Uniform,
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 1,
                                visibility: wgpu::ShaderStages::COMPUTE,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Storage {
                                        read_only: true,
                                    },
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 2,
                                visibility: wgpu::ShaderStages::COMPUTE,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Storage {
                                        read_only: false,
                                    },
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 3,
                                visibility: wgpu::ShaderStages::COMPUTE,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Storage {
                                        read_only: false,
                                    },
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 4,
                                visibility: wgpu::ShaderStages::COMPUTE,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Storage {
                                        read_only: false,
                                    },
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 5,
                                visibility: wgpu::ShaderStages::COMPUTE,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Storage {
                                        read_only: false,
                                    },
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                        ],
                    });
                let shader =
                    device.create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("mesh.cull.shader"),
                        source: wgpu::ShaderSource::Wgsl(
                            std::borrow::Cow::Borrowed(include_str!(
                                "../../shaders/mesh_cull.wgsl"
                            )),
                        ),
                    });
                let layout =
                    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("mesh.cull.layout"),
                        bind_group_layouts: &[&bgl].map(Some),
                        immediate_size: 0,
                    });
                let pipeline =
                    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some("mesh.cull.pipeline"),
                        layout: Some(&layout),
                        module: &shader,
                        entry_point: Some("main"),
                        compilation_options:
                            wgpu::PipelineCompilationOptions::default(),
                        cache: None,
                    });
                let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("mesh.cull.uniform"),
                    size: std::mem::size_of::<MeshCullUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                (Some(bgl), Some(pipeline), Some(uniform))
            } else {
                (None, None, None)
            };

        let mesh_material_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("mesh.material.bgl"),
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 7,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 8,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 9,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 10,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 11,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 12,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 13,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 14,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 15,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: if mesh_storage_instancing {
                                wgpu::BufferBindingType::Storage { read_only: true }
                            } else {
                                wgpu::BufferBindingType::Uniform
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let mesh_default_material_bind_group = mesh_gpu::create_material_bind_group(
            device,
            queue,
            &mesh_material_bgl,
            None,
            None,
        );

        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &mesh_material_bgl].map(Some),
            immediate_size: 0,
        });

        let shadow_source = include_str!("../../shaders/shadow.wgsl");
        let shadow_source = if mesh_storage_instancing {
            std::borrow::Cow::Borrowed(shadow_source)
        } else {
            std::borrow::Cow::Owned(shadow_source.replace(
                "var<storage, read> mesh_instances: array<MeshInstance>;",
                "var<uniform> mesh_instances: array<MeshInstance, 1>;",
            ))
        };
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow.shader"),
            source: wgpu::ShaderSource::Wgsl(shadow_source),
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow.pipeline_layout"),
            bind_group_layouts: &[&shadow_frame_bgl, &mesh_material_bgl].map(Some),
            immediate_size: 0,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow.pipeline"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_main"),
                buffers: &[mesh_gpu::MeshVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
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
                cull_mode: None, // two-sided: ACIS import winding is unreliable
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
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
            multiview_mask: None,
            cache: None,
        });

        // Transparent variant — identical to `mesh_pipeline` but with depth
        // writes disabled. Non-opaque solids are drawn after the opaque fills
        // with this pipeline so they blend over the geometry behind them (which
        // has already written depth) instead of erasing it.
        let mesh_transparent_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh.transparent.pipeline"),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[mesh_gpu::MeshVertex::layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24PlusStencil8,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: content_stencil.clone(),
                    bias: wgpu::DepthBiasState {
                        constant: 1,
                        slope_scale: 1.0,
                        clamp: 0.0,
                    },
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
                multiview_mask: None,
                cache: None,
            });

        // Highlight variants reuse the resident mesh buffers and differ only in
        // their fixed fragment tint. No selected mesh geometry is re-uploaded.
        let make_mesh_highlight_pipeline =
            |label: &'static str, fragment_entry: &'static str| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&mesh_layout),
                    vertex: wgpu::VertexState {
                        module: &mesh_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[mesh_gpu::MeshVertex::layout()],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth24PlusStencil8,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: content_stencil.clone(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: MSAA_SAMPLES,
                        mask: !0,
                        alpha_to_coverage_enabled: false,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &mesh_shader,
                        entry_point: Some(fragment_entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let mesh_selected_pipeline = make_mesh_highlight_pipeline(
            "mesh.highlight.selected.pipeline",
            "fs_highlight_selected",
        );
        let mesh_hover_pipeline =
            make_mesh_highlight_pipeline("mesh.highlight.hover.pipeline", "fs_highlight_hover");

        // Wireframe variant — same shader / vertex layout / depth state,
        // only the input topology changes (LineList) and back-face
        // culling drops out (each triangle edge is shared between two
        // faces, one of which would otherwise hide the edge).
        // Edge/wireframe pipeline (LineList). `fs_edge` outputs the flat entity
        // colour — no lighting — for the lines-only modes. A `fs_edge_black`
        // twin (below) forces black for the edge overlay in filled modes.
        let make_edge_pipeline = |label: &'static str, fs: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_shader,
                    entry_point: Some("vs_edge"),
                    buffers: &[mesh_gpu::MeshVertex::edge_layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24PlusStencil8,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: content_stencil.clone(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: MSAA_SAMPLES,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mesh_shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_wireframe_pipeline = make_edge_pipeline("mesh.wireframe.pipeline", "fs_edge");
        let mesh_edge_black_pipeline = make_edge_pipeline("mesh.edge_black.pipeline", "fs_edge_black");

        // Depth-only variant — TriangleList, back-face culling stays on
        // (we only want front-facing fragments to write depth so wires
        // on the far side of the mesh stay hidden), `write_mask` zero
        // so no fragment ever reaches the colour buffer.
        let mesh_depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh.depth.pipeline"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                buffers: &[mesh_gpu::MeshVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None, // two-sided: ACIS import winding is unreliable
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
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
            bind_group_layouts: &[&frame_bgl].map(Some),
            immediate_size: 0,
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
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
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
            multiview_mask: None,
            cache: None,
        });

        // Depth-only variant — write_mask zero, no blend. The face3d
        // shader still runs but its colour output is discarded, so we
        // get a pure depth prepass for HiddenLine.
        let face3d_depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("face3d.depth.pipeline"),
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
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
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
                // binding 2: ImageParams uniform (read in vertex for the
                // draw-order z bias and in fragment for opacity).
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
            bind_group_layouts: &[&frame_bgl, &image_bgl1].map(Some),
            immediate_size: 0,
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
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
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
            multiview_mask: None,
            cache: None,
        });

        // ── Text (SDF glyph quads) ─────────────────────────────────────────
        let text_atlas_bgl = text_gpu::TextAtlasGpu::bind_group_layout(device);
        let (text_pipeline, text_highlight_pipeline) =
            text_gpu::create_pipelines(
                device,
                &frame_bgl,
                &text_atlas_bgl,
                format,
                MSAA_SAMPLES,
                &content_stencil,
            );

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

        // UV crop uniform: [uv_offset_x, uv_offset_y, uv_scale_x, uv_scale_y]
        // padded to 16 bytes (std140 vec2 alignment). Defaulted to the
        // identity crop (offset 0, scale 1) for the common on-canvas case.
        let blit_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("blit.uniform_buffer"),
            contents: bytemuck::cast_slice(&[0.0f32, 0.0, 1.0, 1.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit.pipeline_layout"),
            bind_group_layouts: &[&blit_bgl].map(Some),
            immediate_size: 0,
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
                    // Premultiplied-alpha blend: the geometry passes already
                    // wrote into a transparent MSAA target with standard
                    // `SrcAlpha / 1-SrcAlpha` blending, so AA-edge fragments
                    // sit as `(rgb * a, a)` in the resolve texture. Treating
                    // that as straight alpha during the surface blit would
                    // multiply by alpha a second time and darken thin lines
                    // / curves. `PREMULTIPLIED_ALPHA_BLENDING` uses `One` as
                    // the source colour factor and leaves the dst weighted
                    // by `1-SrcAlpha`, which is the correct compositing
                    // operator for already-premultiplied content.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: blit_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        Self {
            background_pipeline,
            shadow_pipeline,
            wire_pipeline,
            clip_mask_pipeline,
            wire_black_pipeline,
            wire_xray_pipeline,
            wire_const_bgl,
            wipeout_pipeline,
            hatch_gpu,
            image_pipeline,
            text_pipeline,
            text_highlight_pipeline,
            text_atlas_bgl,
            text_atlas_gpu: None,
            text_vbuf: None,
            text_vcount: 0,
            text_highlight_vbuf: None,
            text_highlight_vcount: 0,
            text_preview_vbuf: None,
            text_preview_vcount: 0,
            silhouette_vbuf: None,
            silhouette_vcount: 0,
            mesh_pipeline,
            mesh_transparent_pipeline,
            mesh_selected_pipeline,
            mesh_hover_pipeline,
            mesh_wireframe_pipeline,
            mesh_edge_black_pipeline,
            mesh_depth_pipeline,
            mesh_material_bgl,
            mesh_default_material_bind_group,
            mesh_cull_pipeline,
            mesh_cull_bgl,
            mesh_cull_uniform,
            mesh_cull_items: None,
            mesh_cull_bind_group: None,
            mesh_opaque_indirect: None,
            mesh_transparent_indirect: None,
            mesh_wire_indirect: None,
            mesh_edge_indirect: None,
            mesh_cull_count: 0,
            face3d_pipeline,
            face3d_depth_pipeline,
            uniform_buffer,
            frame_bgl,
            uniform_bind_group,
            shadow_uniform_bind_group,
            background_texture,
            environment_texture,
            background_sampler,
            background_source_id: 0,
            environment_source_id: 0,
            _shadow_texture: shadow_texture,
            shadow_view,
            shadow_sampler,
            shadow_enabled: false,
            wipeout_bgl1,
            image_bgl1,
            depth_texture_size: Size::new(1, 1),
            // (0, 0) forces the first `ensure_depth_texture` to allocate at the
            // real rounded size — the constructor textures above are placeholders.
            alloc_size: Size::new(0, 0),
            depth_view,
            msaa_view,
            resolve_view,
            blit_pipeline,
            blit_bind_group_layout: blit_bgl,
            blit_sampler,
            blit_bind_group,
            blit_uniform_buffer,
            surface_format: format,
            gpu_wires: std::sync::Arc::new(vec![]),
            wire_arena: None,
            wire_arena_mesh: None,
            wire_arena_fallback: std::sync::Arc::new(Vec::new()),
            wire_arena_fallback_kind: None,
            wire_arena_fallback_handles: rustc_hash::FxHashSet::default(),
            wire_arena_id: u64::MAX,
            wire_cull_key: (u64::MAX, u64::MAX, 0, 0),
            hatch_lod_key: (usize::MAX, u64::MAX, 0, 0, false),
            wipeout_lod_key: (usize::MAX, u64::MAX, 0, 0, false),
            mesh_lod_key: (usize::MAX, u64::MAX, 0, 0),
            clip_boundary: None,
            gpu_selected_wires: vec![],
            gpu_preview_wires: vec![],
            gpu_wipeouts: vec![],
            wipeout_skip_flags: vec![],
            gpu_images: vec![],
            gpu_mesh_batch: vec![],
            gpu_mesh_dynamic: vec![],
            mesh_disabled_chunks: rustc_hash::FxHashSet::default(),
            mesh_dynamic_handles: rustc_hash::FxHashSet::default(),
            cached_mesh_content_id: u64::MAX,
            mesh_ranges_by_handle: rustc_hash::FxHashMap::default(),
            mesh_highlight_draws: vec![],
            cached_highlight_key: (u64::MAX, u64::MAX),
            gpu_face3d_fill: None,
            gpu_face3d_edges: vec![],
            viewcube,
            cached_hatch_source: None,
            cached_preview_hatch_source: None,
            cached_wipeout_source: None,
            cached_image_source: None,
            cached_text_source: None,
            cached_annotation_highlight_source: None,
            cached_mesh_source: None,
            cached_face3d_source: None,
            cached_face3d_depth_source: None,
            cached_fill_mode: false,
            cached_epoch: (u64::MAX, u64::MAX, u64::MAX),
            cached_wire_id: u64::MAX,
            cached_selection: (u64::MAX, u64::MAX),
            cached_face3d_key: (u64::MAX, false, false),
            wire_handle_index: std::sync::Arc::new(rustc_hash::FxHashMap::default()),
            render_sig: u64::MAX,
            skip_geometry: false,
            skip_hatch_frame: false,
            slot_id: u64::MAX,
        }
    }

    /// Build the resident wire batches + handle index for `wires`, wrapped in
    /// `Arc` so the caller can cache them by `wire_content_id` and share one
    /// copy across every slot that renders that content (see
    /// `MultiPipeline::wire_buffer_cache`). Takes `&self` (reads only the const
    /// bind-group layout) so the shared cache — not the slot — owns the result.
    pub fn build_wire_buffers(
        &self,
        device: &wgpu::Device,
        wires: &[WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    ) -> (
        std::sync::Arc<Vec<WireGpu>>,
        std::sync::Arc<rustc_hash::FxHashMap<u64, Vec<u32>>>,
    ) {
        // Batch the wire pass: instead of one GPU buffer + one draw call per
        // WireModel (tens of thousands on a large drawing), merge maximal runs
        // of *consecutive* wires that share scissor + mesh-edge state into one
        // concatenated instance buffer each. The draw loop then issues one draw
        // per run. Runs must be consecutive (not regrouped) so the original
        // wire order — already sorted by draw order — is preserved; depth bias
        // and alpha blending both depend on it. Scissor and mesh-edge stay
        // grouping keys because the draw loop sets one scissor per batch and
        // skips whole mesh-edge batches in shaded modes.
        // A 3D mesh entity (PolyfaceMesh / PolygonMesh) emits its face fill and
        // its outline edges as *separate* WireModels sharing the entity handle
        // (`name`): the fill carries `fill_tris` + a non-empty `fill_tris_low`
        // (real 3D depth, same test face3d uses); the edge carries `points`.
        // Flag the edge wire as `is_3d_mesh_edge` so the draw loop can hide it in
        // clean-shaded modes and draw it black in filled-with-edges modes.
        let mesh_names: rustc_hash::FxHashSet<&str> = wires
            .iter()
            .filter(|w| !w.fill_tris.is_empty() && !w.fill_tris_low.is_empty())
            .map(|w| w.name.as_str())
            .collect();
        let is_mesh_edge =
            |w: &WireModel| !w.points.is_empty() && mesh_names.contains(w.name.as_str());
        let mut batches: Vec<WireGpu> = Vec::new();
        let mut i = 0;
        while i < wires.len() {
            let mesh_edge = is_mesh_edge(&wires[i]);
            let mut j = i + 1;
            while j < wires.len() && is_mesh_edge(&wires[j]) == mesh_edge {
                j += 1;
            }
            batches.extend(WireGpu::from_run(device, &wires[i..j], depth_map, mesh_edge, self.wire_const_bgl.as_ref()));
            i = j;
        }

        // Index handle → wire slots once, here, so the per-hover selection
        // overlay can gather just the highlighted wires instead of scanning +
        // string-parsing the whole set every time the hovered entity changes.
        let mut index: rustc_hash::FxHashMap<u64, Vec<u32>> = rustc_hash::FxHashMap::default();
        index.reserve(wires.len());
        for (idx, w) in wires.iter().enumerate() {
            if let Ok(h) = w.name.parse::<u64>() {
                index.entry(h).or_default().push(idx as u32);
            }
        }
        (std::sync::Arc::new(batches), std::sync::Arc::new(index))
    }

    /// Build the selection xray overlay: full-brightness copies of the wires
    /// whose entity handle is in `highlight`, drawn on top so the selection is
    /// always visible. Selection is no longer baked into the wire tessellation,
    /// so this is driven by the live highlight set and rebuilt only when the
    /// selection (or the underlying wire content) changes — picking an entity
    /// refreshes just this overlay instead of re-tessellating the model. The
    /// xray pass applies neither scissor nor mesh-edge skip, so everything
    /// merges into one order-preserving run.
    pub fn upload_selected_wires(
        &mut self,
        device: &wgpu::Device,
        wires: &[WireModel],
        selected: &rustc_hash::FxHashSet<acadrust::Handle>,
        hovered: &rustc_hash::FxHashSet<acadrust::Handle>,
        annotation_context_wires: &[WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    ) {
        let perf_started = crate::perf::enabled().then(iced::time::Instant::now);
        if selected.is_empty() && hovered.is_empty() && annotation_context_wires.is_empty() {
            self.gpu_selected_wires = vec![];
            return;
        }
        // Gather borrowed highlighted wires via the prebuilt index —
        // O(highlighted), no per-wire string parse or deep geometry clone.
        let mut selected_wires: Vec<&WireModel> = Vec::new();
        let mut hover_wires: Vec<&WireModel> = Vec::new();
        for h in selected {
            if let Some(idxs) = self.wire_handle_index.get(&h.value()) {
                let mut slots = idxs.clone();
                slots.sort_unstable();
                for &i in &slots {
                    if let Some(w) = wires.get(i as usize) {
                        selected_wires.push(w);
                    }
                }
            }
        }
        for h in hovered.iter().filter(|handle| !selected.contains(handle)) {
            if let Some(idxs) = self.wire_handle_index.get(&h.value()) {
                let mut slots = idxs.clone();
                slots.sort_unstable();
                for &i in &slots {
                    if let Some(w) = wires.get(i as usize) {
                        hover_wires.push(w);
                    }
                }
            }
        }
        for wire in annotation_context_wires {
            if wire.selected {
                selected_wires.push(wire);
            } else {
                hover_wires.push(wire);
            }
        }
        let mut gpu = WireGpu::from_highlight_refs(
            device,
            &selected_wires,
            WireModel::SELECTED,
            depth_map,
            self.wire_const_bgl.as_ref(),
        );
        gpu.extend(WireGpu::from_highlight_refs(
            device,
            &hover_wires,
            WireModel::HOVER,
            depth_map,
            self.wire_const_bgl.as_ref(),
        ));
        self.gpu_selected_wires = gpu;
        if let Some(started) = perf_started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] wire-highlight {:>7.1}ms handles={} wires={}",
                    elapsed_ms,
                    selected.len()
                        + hovered
                            .iter()
                            .filter(|handle| !selected.contains(handle))
                            .count(),
                    selected_wires.len() + hover_wires.len(),
                );
            }
        }
    }

    /// Build the text-highlight overlay: the glyph quads of just the selected /
    /// hovered text, recoloured (selection blue, hover orange) and drawn over
    /// the base text pass. Uses the same handle→wire index as the selected-wire
    /// overlay, so it is O(highlighted). Empty when nothing is highlighted.
    pub fn upload_text_highlight(
        &mut self,
        device: &wgpu::Device,
        wires: &[WireModel],
        selected: &rustc_hash::FxHashSet<acadrust::Handle>,
        hovered: &rustc_hash::FxHashSet<acadrust::Handle>,
        annotation_context_wires: &[WireModel],
    ) {
        let perf_started = crate::perf::enabled().then(iced::time::Instant::now);
        if selected.is_empty() && hovered.is_empty() && annotation_context_wires.is_empty() {
            self.text_highlight_vbuf = None;
            self.text_highlight_vcount = 0;
            return;
        }
        let mut out: Vec<text_gpu::TextVertex> = Vec::new();
        let push =
            |handle_val: u64, tint: [f32; 4], wires: &[WireModel], out: &mut Vec<text_gpu::TextVertex>| {
                if let Some(idxs) = self.wire_handle_index.get(&handle_val) {
                    for &i in idxs {
                        if let Some(w) = wires.get(i as usize) {
                            for v in &w.text_verts {
                                out.push(text_gpu::TextVertex {
                                    color: [tint[0], tint[1], tint[2], v.color[3]],
                                    ..*v
                                });
                            }
                        }
                    }
                }
            };
        for h in selected {
            push(h.value(), WireModel::SELECTED, wires, &mut out);
        }
        for h in hovered.iter().filter(|handle| !selected.contains(handle)) {
            push(h.value(), WireModel::HOVER, wires, &mut out);
        }
        for wire in annotation_context_wires {
            let tint = if wire.selected {
                WireModel::SELECTED
            } else {
                WireModel::HOVER
            };
            for vertex in &wire.text_verts {
                out.push(text_gpu::TextVertex {
                    color: [tint[0], tint[1], tint[2], vertex.color[3]],
                    ..*vertex
                });
            }
        }
        self.text_highlight_vcount = out.len() as u32;
        self.text_highlight_vbuf = text_gpu::upload_vertices(device, &out);
        if let Some(started) = perf_started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] text-highlight {:>7.1}ms vertices={}",
                    elapsed_ms,
                    out.len(),
                );
            }
        }
    }

    /// Upload the live overlay (command preview / interim / grip-drag) wires.
    /// Small and refreshed each frame they're present; kept separate from the
    /// base wire buffer so a drag never re-uploads the resident base set.
    pub fn upload_preview_wires(
        &mut self,
        device: &wgpu::Device,
        wires: &[WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    ) {
        self.gpu_preview_wires = if wires.is_empty() {
            vec![]
        } else {
            WireGpu::from_run(device, wires, depth_map, false, self.wire_const_bgl.as_ref())
        };
    }

    /// Upload the live grip-drag / command-preview SDF glyph quads. Re-uploaded
    /// every frame they're present, kept separate from the epoch-cached base
    /// text buffer so a drag never re-uploads the (potentially huge) resident
    /// text set — and so the dragged entity's glyphs (hidden from the base set
    /// while the drag is live) still reach the GPU each frame (issue #316).
    pub fn upload_preview_text(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        verts: &[text_gpu::TextVertex],
    ) {
        if verts.is_empty() {
            self.text_preview_vbuf = None;
            self.text_preview_vcount = 0;
            return;
        }
        // Ensure the glyph atlas exists / is current even when the base text
        // pass didn't run this frame (e.g. the dragged text is the only text).
        if let Ok(mut atlas) = crate::scene::text::sdf_atlas::text_atlas().lock() {
            if self.text_atlas_gpu.is_none() || atlas.is_dirty() {
                self.text_atlas_gpu = Some(text_gpu::TextAtlasGpu::upload(
                    device,
                    queue,
                    &atlas,
                    &self.text_atlas_bgl,
                ));
                atlas.clear_dirty();
            }
        }
        self.text_preview_vbuf = text_gpu::upload_vertices(device, verts);
        self.text_preview_vcount = verts.len() as u32;
    }

    // The math lives outside the GPU method so it can be unit-tested; see the
    // module test below.

    /// Rebuild the per-frame DISPSILH silhouette line list from the mesh sets'
    /// curved-face generators and the current eye. For each cone/cylinder face
    /// the silhouette runs at the two angles where the surface turns edge-on to
    /// the view — `θ = φ ± acos(-tanα·(view·axis) / |view⊥|)`, which reduces to
    /// `φ ± π/2` for a cylinder. Segments are uploaded in the mesh vertex format
    /// so they draw through the existing wireframe pipeline.
    pub fn upload_silhouettes(
        &mut self,
        device: &wgpu::Device,
        sets: &[crate::scene::model::mesh_model::MeshLodSet],
        view_dir: glam::Vec3,
    ) {
        // Silhouettes follow the view *angle* only — a single parallel direction
        // for the whole scene, not the eye-to-surface vector — so the outline
        // stays put under pan and doesn't foreshorten. This is the orthographic
        // silhouette a CAD wireframe expects.
        let view = glam::DVec3::new(view_dir.x as f64, view_dir.y as f64, view_dir.z as f64)
            .normalize_or(glam::DVec3::NEG_Z);
        use crate::scene::model::mesh_model::CurvedGen;
        use crate::scene::pipeline::mesh_gpu::MeshVertex;
        let mut verts: Vec<MeshVertex> = Vec::new();
        let d3 = |a: [f32; 3]| glam::DVec3::new(a[0] as f64, a[1] as f64, a[2] as f64);
        let lo = |c: [f32; 3], l: [f32; 3]| {
            glam::DVec3::new(
                c[0] as f64 + l[0] as f64,
                c[1] as f64 + l[1] as f64,
                c[2] as f64 + l[2] as f64,
            )
        };
        for set in sets {
            let color = set.lods.first().map(|m| m.color).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let mk = |w: glam::DVec3| -> MeshVertex {
                let (hx, hy, hz) = (w.x as f32, w.y as f32, w.z as f32);
                MeshVertex {
                    position: [hx, hy, hz],
                    normal: [0.0, 1.0, 0.0],
                    color,
                    position_low: [
                        (w.x - hx as f64) as f32,
                        (w.y - hy as f64) as f32,
                        (w.z - hz as f64) as f32,
                    ],
                    material: [0.5, 0.0, 0.0, 0.0],
                    specular: [1.0, 1.0, 1.0, 1.0],
                    uv_diffuse: [0.0; 2],
                    ambient: [0.3, 0.3, 0.3, 0.0],
                    advanced: [1.0; 4],
                    flags: [0, 127, 0, 0],
                    uv_specular: [0.0; 2],
                    uv_reflection: [0.0; 2],
                    uv_opacity: [0.0; 2],
                    uv_bump: [0.0; 2],
                    uv_refraction: [0.0; 2],
                    uv_normal: [0.0; 2],
                }
            };
            for g in &set.curved_gens {
                match g {
                    CurvedGen::Cone {
                        base, base_low, axis, u_dir, v_dir, radius, tan_a,
                        h_max, theta_min, theta_span, full,
                    } => {
                        let base = lo(*base, *base_low);
                        let (axis, u, v) = (d3(*axis), d3(*u_dir), d3(*v_dir));
                        let Some((t0, t1)) =
                            silhouette_thetas(view.dot(u), view.dot(v), view.dot(axis), *tan_a as f64)
                        else {
                            continue;
                        };
                        let r0 = *radius as f64;
                        let r1 = *radius as f64 + *h_max as f64 * *tan_a as f64;
                        for theta in [t0, t1] {
                            if !full {
                                let off = (theta - *theta_min as f64).rem_euclid(std::f64::consts::TAU);
                                if off > *theta_span as f64 {
                                    continue;
                                }
                            }
                            let (c, s) = (theta.cos(), theta.sin());
                            let radial = u * c + v * s;
                            verts.push(mk(base + radial * r0));
                            verts.push(mk(base + radial * r1 + axis * *h_max as f64));
                        }
                    }
                    CurvedGen::Sphere {
                        center, center_low, pole, u_dir, v_dir, radius,
                        theta_min, theta_span, full, phi_min, phi_max,
                    } => {
                        let c = lo(*center, *center_low);
                        let (pole, u, v) = (d3(*pole), d3(*u_dir), d3(*v_dir));
                        let r = *radius as f64;
                        // Great circle in the plane perpendicular to the view.
                        let mut e1 = view.cross(pole);
                        if e1.length_squared() < 1e-12 {
                            e1 = view.cross(u);
                        }
                        let e1 = e1.normalize();
                        let e2 = view.cross(e1).normalize();
                        const N: usize = 64;
                        let mut prev: Option<glam::DVec3> = None;
                        for i in 0..=N {
                            let a = std::f64::consts::TAU * (i as f64 / N as f64);
                            let dir = e1 * a.cos() + e2 * a.sin();
                            // Keep only the arc that lies on the actual face.
                            let on_face = *full || {
                                let phi = dir.dot(pole).clamp(-1.0, 1.0).acos();
                                let th = dir.dot(v).atan2(dir.dot(u));
                                let toff = (th - *theta_min as f64).rem_euclid(std::f64::consts::TAU);
                                phi >= *phi_min as f64
                                    && phi <= *phi_max as f64
                                    && (toff <= *theta_span as f64)
                            };
                            let p = if on_face { Some(c + dir * r) } else { None };
                            if let (Some(a), Some(b)) = (prev, p) {
                                verts.push(mk(a));
                                verts.push(mk(b));
                            }
                            prev = p;
                        }
                    }
                    CurvedGen::Torus {
                        center, center_low, axis, u_dir, v_dir, major, minor,
                        phi_min, phi_span, full, theta_min, theta_span, theta_full,
                    } => {
                        let ctr = lo(*center, *center_low);
                        let (axis, u, v) = (d3(*axis), d3(*u_dir), d3(*v_dir));
                        let (major, minor) = (*major as f64, *minor as f64);
                        // True silhouette: at each revolution angle the tube is a
                        // circle; the two points where its normal turns edge-on
                        // trace two curves around the ring. Sample the revolution
                        // and connect consecutive edge-on points.
                        const N: usize = 72;
                        let span = if *full { std::f64::consts::TAU } else { *phi_span as f64 };
                        let mut prev: [Option<glam::DVec3>; 2] = [None, None];
                        for i in 0..=N {
                            let phi = *phi_min as f64 + span * (i as f64 / N as f64);
                            let radial = u * phi.cos() + v * phi.sin();
                            let ring = ctr + radial * major;
                            let (rv, av) = (radial.dot(view), axis.dot(view));
                            if rv.abs() < 1e-9 && av.abs() < 1e-9 {
                                prev = [None, None];
                                continue;
                            }
                            // tube normal(θ) = radial·cosθ + axis·sinθ; ⟂ view at
                            // θ = atan2(-rv, av) and +π.
                            let th = (-rv).atan2(av);
                            let cur = [th, th + std::f64::consts::PI];
                            for k in 0..2 {
                                let t = cur[k];
                                let theta_offset =
                                    (t - *theta_min as f64).rem_euclid(std::f64::consts::TAU);
                                let on_face = *theta_full || theta_offset <= *theta_span as f64;
                                let p = on_face.then(|| {
                                    ring + (radial * t.cos() + axis * t.sin()) * minor
                                });
                                if let (Some(pp), Some(p)) = (prev[k], p) {
                                    verts.push(mk(pp));
                                    verts.push(mk(p));
                                }
                                prev[k] = p;
                            }
                        }
                    }
                }
            }
            if set.curved_gens.is_empty() || !set.complete {
                let best = set.stored_silhouettes.iter().max_by(|left, right| {
                    let score = |silhouette: &crate::scene::model::mesh_model::StoredSilhouette| {
                        let direction = d3(silhouette.view_direction)
                            .normalize_or(glam::DVec3::NEG_Z);
                        direction.dot(view).abs()
                    };
                    score(left)
                        .partial_cmp(&score(right))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                if let Some(silhouette) = best {
                    for (index, high) in silhouette.edge_verts.iter().copied().enumerate() {
                        let low = silhouette
                            .edge_verts_low
                            .get(index)
                            .copied()
                            .unwrap_or([0.0; 3]);
                        verts.push(mk(lo(high, low)));
                    }
                }
            }
        }
        if verts.is_empty() {
            self.silhouette_vbuf = None;
            self.silhouette_vcount = 0;
            return;
        }
        use wgpu::util::DeviceExt;
        self.silhouette_vbuf = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh.silhouette.vbuf"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        }));
        self.silhouette_vcount = verts.len() as u32;
    }

    /// Upload this content viewport's non-rectangular clip boundary as a
    /// triangle-fan vertex buffer in render-target NDC. The boundary is already
    /// projected (paper shape → the same visible-sub-rect crop as the content),
    /// so the mask pipeline stamps it straight into the stencil with `Invert`
    /// (even-odd fill → interior marked, any convexity). Empty input clears the
    /// boundary so the viewport renders unclipped (its render rectangle clips).
    pub fn upload_clip_boundary(&mut self, device: &wgpu::Device, boundary_ndc: &[[f32; 2]]) {
        use wgpu::util::DeviceExt;
        if boundary_ndc.len() < 3 {
            self.clip_boundary = None;
            return;
        }
        // Triangle fan (p0, pi, pi+1).
        let mut verts: Vec<f32> = Vec::with_capacity((boundary_ndc.len() - 2) * 6);
        for i in 1..boundary_ndc.len() - 1 {
            for &p in &[boundary_ndc[0], boundary_ndc[i], boundary_ndc[i + 1]] {
                verts.extend_from_slice(&[p[0], p[1]]);
            }
        }
        if verts.is_empty() {
            self.clip_boundary = None;
            return;
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("clip_boundary.vbuf"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.clip_boundary = Some((vbuf, (verts.len() / 2) as u32));
    }

    /// Per-frame visibility refresh for the batched hatch path.
    /// Combines Phase 3.3 sub-pixel LOD skip with Phase 2.3 frustum
    /// cull and pushes the resulting 0/1 mask through to the GPU
    /// `visibility_buffer`. Vertex shader maps 0 → out-of-NDC clip,
    /// so the rasterizer culls the primitive before any fragment
    /// runs.
    pub fn compute_hatch_lod(
        &mut self,
        queue: &wgpu::Queue,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        clip_w: u32,
        clip_h: u32,
    ) {
        let perf_started = crate::perf::enabled().then(iced::time::Instant::now);
        let instance_count = self.hatch_gpu.update_visibility(queue, |aabb| {
            !aabb_below_pixel(aabb, view_rot, eye, clip_w, clip_h, 2.0)
                && !aabb_offscreen(aabb, view_rot, eye, clip_w, clip_h)
        });
        if instance_count == 0 {
            return;
        }
        if let Some(started) = perf_started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] hatch-lod {:>7.1}ms instances={}",
                    elapsed_ms,
                    instance_count,
                );
            }
        }
    }

    /// Per-frame wipeout frustum-skip flag (Phase 2.3). Mirrors
    /// `compute_hatch_lod`'s frustum branch. No sub-pixel skip:
    /// wipeouts mask, so dropping a sub-pixel one wouldn't be wrong
    /// but also wouldn't pay off — they're usually few.
    pub fn compute_wipeout_lod(&mut self, view_rot: glam::Mat4, eye: glam::DVec3, clip_w: u32, clip_h: u32) {
        self.wipeout_skip_flags = self
            .gpu_wipeouts
            .iter()
            .map(|h| aabb_offscreen(h.world_aabb, view_rot, eye, clip_w, clip_h))
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
        wireframe_only: bool,
        show_2d_solid_fills: bool,
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    ) {
        let perf_started = crate::perf::enabled().then(iced::time::Instant::now);
        // Edge buffer is always built from `face3d_wires`, so 3DFACE
        // outlines stay on the screen regardless of mode.
        self.gpu_face3d_edges =
            WireGpu::from_batch(device, face3d_wires, depth_map, self.wire_const_bgl.as_ref());
        // Fill buffer split: 3D quads + PolyfaceMesh / PolygonMesh face
        // tris go to `chunks_3d` (gated by `keep_3d_mesh_fills`);
        // 2D fills (text-LOD greek, MultiLeader background, dimension arrows) go to
        // `chunks_2d`. The 3-D wireframe additionally removes only legacy
        // planar SOLID interiors; HATCH is handled by a separate pass.
        let keep_3d_mesh_fills = !wireframe_only;
        let solid_fill_hidden = |wire: &WireModel| {
            !show_2d_solid_fills && wire.fill_is_2d_solid
        };
        let has_any_2d_fill = all_wires
            .iter()
            .any(|w| !w.fill_tris.is_empty() && !w.fill_is_3d && !solid_fill_hidden(w));
        let has_any_3d_fill = !face3d_wires.is_empty()
            || all_wires
                .iter()
                .any(|w| !w.fill_tris.is_empty() && w.fill_is_3d);
        let has_fills = has_any_2d_fill || (keep_3d_mesh_fills && has_any_3d_fill);
        if !has_fills {
            self.gpu_face3d_fill = None;
        } else {
            self.gpu_face3d_fill = Some(Face3DGpu::from_wires(
                device,
                face3d_wires,
                all_wires,
                keep_3d_mesh_fills,
                show_2d_solid_fills,
                depth_map,
            ));
        }
        if let Some(started) = perf_started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] face3d-upload {:>7.1}ms face-wires={} all-wires={}",
                    elapsed_ms,
                    face3d_wires.len(),
                    all_wires.len(),
                );
            }
        }
    }

    /// Build the batched mesh buffers (a few large buffers for the whole solid
    /// set, drawn in a handful of calls). Selection/hover tint is intentionally
    /// not applied here — the batch is geometry-only and stays resident across
    /// camera moves and pick changes.
    pub fn upload_mesh_batch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        meshes: &[MeshLodSet],
    ) {
        let started = iced::time::Instant::now();
        let (mut chunks, triangles) = mesh_gpu::build_mesh_batch(device, meshes);
        for chunk in &mut chunks {
            chunk.material_bind_group = Some(mesh_gpu::create_material_bind_group(
                device,
                queue,
                &self.mesh_material_bgl,
                chunk.material.as_ref(),
                Some(&chunk.instance_buffer),
            ));
        }
        self.mesh_ranges_by_handle.clear();
        for (chunk_index, chunk) in chunks.iter().enumerate() {
            for range in &chunk.highlight_ranges {
                self.mesh_ranges_by_handle
                    .entry(range.handle)
                    .or_default()
                    .push(MeshResidentRange {
                        chunk: chunk_index,
                        dynamic: false,
                        index_start: range.index_start,
                        index_count: range.index_count,
                        transparent: range.transparent,
                        instance_start: range.instance_start,
                        instance_count: range.instance_count,
                    });
            }
        }
        self.mesh_highlight_draws.clear();
        self.cached_highlight_key = (u64::MAX, u64::MAX);
        self.gpu_mesh_dynamic.clear();
        self.mesh_disabled_chunks.clear();
        self.mesh_dynamic_handles.clear();
        self.gpu_mesh_batch = chunks;
        self.rebuild_mesh_cull_resources(device);
        if crate::perf::enabled() {
            let instances: u64 = self
                .gpu_mesh_batch
                .iter()
                .map(|chunk| chunk.instance_count as u64)
                .sum();
            crate::perf_record!(
                "[perf] mesh-batch {:>7.1}ms sets={} chunks={} instances={} triangles={}",
                started.elapsed().as_secs_f64() * 1000.0,
                meshes.len(),
                self.gpu_mesh_batch.len(),
                instances,
                triangles,
            );
        }
    }

    fn rebuild_mesh_cull_resources(&mut self, device: &wgpu::Device) {
        let count = self.gpu_mesh_batch.len() + self.gpu_mesh_dynamic.len();
        // Below this point CPU projection is cheaper than a compute pass plus
        // indirect command reads. The large-scene path turns on automatically.
        if count < 128
            || self.mesh_cull_pipeline.is_none()
            || self.mesh_cull_bgl.is_none()
            || self.mesh_cull_uniform.is_none()
        {
            self.mesh_cull_bind_group = None;
            self.mesh_cull_items = None;
            self.mesh_opaque_indirect = None;
            self.mesh_transparent_indirect = None;
            self.mesh_wire_indirect = None;
            self.mesh_edge_indirect = None;
            self.mesh_cull_count = 0;
            return;
        }
        let mut items = Vec::with_capacity(count);
        for (index, chunk) in self.gpu_mesh_batch.iter().enumerate() {
            items.push(MeshCullItem {
                min: [
                    chunk.world_aabb[0],
                    chunk.world_aabb[1],
                    chunk.world_aabb[2],
                    0.0,
                ],
                max: [
                    chunk.world_aabb[3],
                    chunk.world_aabb[4],
                    chunk.world_aabb[5],
                    0.0,
                ],
                counts: [
                    chunk.index_count,
                    chunk.transp_index_count,
                    chunk.wire_index_count,
                    chunk.edge_vertex_count,
                ],
                meta: [
                    chunk.instance_count,
                    (!self.mesh_disabled_chunks.contains(&index)) as u32,
                    0,
                    0,
                ],
            });
        }
        for chunk in &self.gpu_mesh_dynamic {
            items.push(MeshCullItem {
                min: [
                    chunk.world_aabb[0],
                    chunk.world_aabb[1],
                    chunk.world_aabb[2],
                    0.0,
                ],
                max: [
                    chunk.world_aabb[3],
                    chunk.world_aabb[4],
                    chunk.world_aabb[5],
                    0.0,
                ],
                counts: [
                    chunk.index_count,
                    chunk.transp_index_count,
                    chunk.wire_index_count,
                    chunk.edge_vertex_count,
                ],
                meta: [chunk.instance_count, 1, 0, 0],
            });
        }
        let items_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh.cull.items"),
                contents: bytemuck::cast_slice(&items),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let indexed_size = (count * std::mem::size_of::<wgpu::util::DrawIndexedIndirectArgs>())
            as u64;
        let draw_size =
            (count * std::mem::size_of::<wgpu::util::DrawIndirectArgs>()) as u64;
        let make_output = |label: &'static str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT,
                mapped_at_creation: false,
            })
        };
        let opaque = make_output("mesh.cull.opaque", indexed_size);
        let transparent = make_output("mesh.cull.transparent", indexed_size);
        let wire = make_output("mesh.cull.wire", indexed_size);
        let edge = make_output("mesh.cull.edge", draw_size);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh.cull.bind_group"),
            layout: self.mesh_cull_bgl.as_ref().expect("checked above"),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self
                        .mesh_cull_uniform
                        .as_ref()
                        .expect("checked above")
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: items_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: opaque.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: transparent.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wire.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: edge.as_entire_binding(),
                },
            ],
        });
        self.mesh_cull_bind_group = Some(bind_group);
        self.mesh_cull_items = Some(items_buffer);
        self.mesh_opaque_indirect = Some(opaque);
        self.mesh_transparent_indirect = Some(transparent);
        self.mesh_wire_indirect = Some(wire);
        self.mesh_edge_indirect = Some(edge);
        self.mesh_cull_count = count as u32;
    }

    fn rebuild_mesh_range_map(&mut self) {
        self.mesh_ranges_by_handle.clear();
        for (chunk_index, chunk) in self.gpu_mesh_batch.iter().enumerate() {
            if self.mesh_disabled_chunks.contains(&chunk_index) {
                continue;
            }
            for range in &chunk.highlight_ranges {
                self.mesh_ranges_by_handle
                    .entry(range.handle)
                    .or_default()
                    .push(MeshResidentRange {
                        chunk: chunk_index,
                        dynamic: false,
                        index_start: range.index_start,
                        index_count: range.index_count,
                        transparent: range.transparent,
                        instance_start: range.instance_start,
                        instance_count: range.instance_count,
                    });
            }
        }
        for (chunk_index, chunk) in self.gpu_mesh_dynamic.iter().enumerate() {
            for range in &chunk.highlight_ranges {
                self.mesh_ranges_by_handle
                    .entry(range.handle)
                    .or_default()
                    .push(MeshResidentRange {
                        chunk: chunk_index,
                        dynamic: true,
                        index_start: range.index_start,
                        index_count: range.index_count,
                        transparent: range.transparent,
                        instance_start: range.instance_start,
                        instance_count: range.instance_count,
                    });
            }
        }
        self.mesh_highlight_draws.clear();
        self.cached_highlight_key = (u64::MAX, u64::MAX);
    }

    /// Patch an entity-only geometry change without rebuilding the resident
    /// mesh set. Any 32 MiB static chunk containing a changed handle is retired;
    /// the current versions of all its handles are rebuilt into a small dynamic
    /// working set. A bounded threshold falls back to a full rebuild before the
    /// working set can grow into a second copy of the drawing.
    pub fn patch_mesh_batch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        meshes: &[MeshLodSet],
        changes: &[(acadrust::Handle, crate::scene::ChangeKind)],
    ) -> bool {
        let started = iced::time::Instant::now();
        if self.gpu_mesh_batch.is_empty() || changes.is_empty() {
            return false;
        }
        let current_handles: rustc_hash::FxHashSet<_> = meshes
            .iter()
            .filter_map(|set| {
                set.lods
                    .first()
                    .and_then(|mesh| mesh.name.parse::<u64>().ok())
                    .map(acadrust::Handle::new)
            })
            .collect();
        let changed: rustc_hash::FxHashSet<_> = changes
            .iter()
            .map(|(handle, _)| *handle)
            .filter(|handle| {
                current_handles.contains(handle)
                    || self.mesh_dynamic_handles.contains(handle)
                    || self
                        .gpu_mesh_batch
                        .iter()
                        .any(|chunk| chunk.handles.contains(handle))
            })
            .collect();
        if changed.is_empty() {
            return true;
        }
        for (chunk_index, chunk) in self.gpu_mesh_batch.iter().enumerate() {
            if chunk.handles.iter().any(|handle| changed.contains(handle)) {
                self.mesh_disabled_chunks.insert(chunk_index);
                self.mesh_dynamic_handles
                    .extend(chunk.handles.iter().copied());
            }
        }
        self.mesh_dynamic_handles.extend(changed.iter().copied());
        let disabled_limit = self.gpu_mesh_batch.len().div_ceil(2).max(8);
        if self.mesh_disabled_chunks.len() > disabled_limit
            || self.mesh_dynamic_handles.len() > 4096
        {
            return false;
        }
        let (mut chunks, _) = mesh_gpu::build_mesh_batch_filtered(
            device,
            meshes,
            Some(&self.mesh_dynamic_handles),
        );
        for chunk in &mut chunks {
            chunk.material_bind_group = Some(mesh_gpu::create_material_bind_group(
                device,
                queue,
                &self.mesh_material_bgl,
                chunk.material.as_ref(),
                Some(&chunk.instance_buffer),
            ));
        }
        self.gpu_mesh_dynamic = chunks;
        self.rebuild_mesh_cull_resources(device);
        self.rebuild_mesh_range_map();
        if crate::perf::enabled() {
            crate::perf_record!(
                "[perf] mesh-patch {:>7.1}ms changed={} retired={} dynamic_handles={} dynamic_chunks={}",
                started.elapsed().as_secs_f64() * 1000.0,
                changed.len(),
                self.mesh_disabled_chunks.len(),
                self.mesh_dynamic_handles.len(),
                self.gpu_mesh_dynamic.len(),
            );
        }
        true
    }

    /// Refresh selection/hover draw ranges. The overlay references resident
    /// chunk buffers; changing hover never allocates or uploads mesh geometry.
    pub fn update_mesh_highlight(
        &mut self,
        selected: &rustc_hash::FxHashSet<acadrust::Handle>,
        hovered: &rustc_hash::FxHashSet<acadrust::Handle>,
    ) {
        let mut out = Vec::new();
        for handle in selected {
            if let Some(ranges) = self.mesh_ranges_by_handle.get(handle) {
                out.extend(ranges.iter().copied().map(|range| MeshHighlightDraw {
                    range,
                    kind: MeshHighlightKind::Selected,
                }));
            }
        }
        for handle in hovered.iter().filter(|handle| !selected.contains(handle)) {
            if let Some(ranges) = self.mesh_ranges_by_handle.get(&handle) {
                out.extend(ranges.iter().copied().map(|range| MeshHighlightDraw {
                    range,
                    kind: MeshHighlightKind::Hover,
                }));
            }
        }
        self.mesh_highlight_draws = out;
    }

    fn active_mesh_chunks_indexed(
        &self,
    ) -> impl Iterator<Item = (usize, &mesh_gpu::MeshBatchChunk)> {
        let dynamic_offset = self.gpu_mesh_batch.len();
        self.gpu_mesh_batch
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.mesh_disabled_chunks.contains(index))
            .chain(
                self.gpu_mesh_dynamic
                    .iter()
                    .enumerate()
                    .map(move |(index, chunk)| (dynamic_offset + index, chunk)),
            )
    }

    /// Per-frame coarse frustum culling for the spatially sorted resident
    /// chunks. It changes only draw eligibility and never rebuilds GPU data.
    pub fn compute_mesh_lod(
        &mut self,
        queue: &wgpu::Queue,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        clip_w: u32,
        clip_h: u32,
    ) {
        if self.mesh_cull_count != 0 {
            if let Some(uniform) = &self.mesh_cull_uniform {
                queue.write_buffer(
                    uniform,
                    0,
                    bytemuck::bytes_of(&MeshCullUniform {
                        view_rot: view_rot.to_cols_array(),
                        eye: [eye.x as f32, eye.y as f32, eye.z as f32, 0.0],
                        count: [self.mesh_cull_count, 0, 0, 0],
                    }),
                );
            }
            for (index, chunk) in self.gpu_mesh_batch.iter_mut().enumerate() {
                chunk.visible = !self.mesh_disabled_chunks.contains(&index);
            }
            for chunk in &mut self.gpu_mesh_dynamic {
                chunk.visible = true;
            }
            return;
        }
        for (index, chunk) in self.gpu_mesh_batch.iter_mut().enumerate() {
            chunk.visible =
                !self.mesh_disabled_chunks.contains(&index)
                    && !aabb3_offscreen(chunk.world_aabb, view_rot, eye, clip_w, clip_h);
        }
        for chunk in &mut self.gpu_mesh_dynamic {
            chunk.visible =
                !aabb3_offscreen(chunk.world_aabb, view_rot, eye, clip_w, clip_h);
        }
    }

    pub fn upload_hatches(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hatches: &[HatchModel],
    ) {
        let perf_started = crate::perf::enabled().then(iced::time::Instant::now);
        let renderable_count = hatches
            .iter()
            .filter(|hatch| hatch.boundary.len() >= 3)
            .count();
        self.hatch_gpu.upload(device, queue, hatches);
        if let Some(started) = perf_started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] hatch-upload {:>7.1}ms models={}",
                    elapsed_ms,
                    renderable_count,
                );
            }
        }
    }

    pub fn upload_preview_hatches(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hatches: &[HatchModel],
    ) {
        self.hatch_gpu.upload_preview(device, queue, hatches);
    }

    pub fn upload_wipeouts(&mut self, device: &wgpu::Device, wipeouts: &[HatchModel]) {
        self.gpu_wipeouts = wipeouts
            .iter()
            .filter(|h| h.boundary.len() >= 3)
            .map(|h| WipeoutGpu::new(device, h, &self.wipeout_bgl1))
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

    /// Upload the frame's SDF text-quad vertices, and (re)build the GPU glyph
    /// atlas from the shared CPU atlas when it grew (new glyphs baked by the
    /// text collector). `verts` empty (flag off) leaves nothing to draw.
    pub fn upload_text(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        verts: &[text_gpu::TextVertex],
    ) {
        let perf_started = crate::perf::enabled().then(iced::time::Instant::now);
        if let Ok(mut atlas) = crate::scene::text::sdf_atlas::text_atlas().lock() {
            if self.text_atlas_gpu.is_none() || atlas.is_dirty() {
                self.text_atlas_gpu = Some(text_gpu::TextAtlasGpu::upload(
                    device,
                    queue,
                    &atlas,
                    &self.text_atlas_bgl,
                ));
                atlas.clear_dirty();
            }
        }
        self.text_vbuf = text_gpu::upload_vertices(device, verts);
        self.text_vcount = verts.len() as u32;
        if let Some(started) = perf_started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] text-upload {:>7.1}ms vertices={}",
                    elapsed_ms,
                    verts.len(),
                );
            }
        }
    }

    pub fn upload_uniforms(&mut self, queue: &wgpu::Queue, uniforms: &Uniforms) {
        self.shadow_enabled = uniforms.shadow_params[0] > 0.5;
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));
    }

    pub fn upload_background_images(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        background: Option<&crate::scene::model::image_model::DecodedImage>,
        environment: Option<&crate::scene::model::image_model::DecodedImage>,
    ) {
        let source_id = |image: Option<&crate::scene::model::image_model::DecodedImage>| {
            image
                .map(|image| std::sync::Arc::as_ptr(&image.pixels) as usize)
                .unwrap_or(0)
        };
        let background_id = source_id(background);
        let environment_id = source_id(environment);
        if background_id == self.background_source_id
            && environment_id == self.environment_source_id
        {
            return;
        }
        let upload = |
            label: &'static str,
            image: Option<&crate::scene::model::image_model::DecodedImage>,
            fallback: [u8; 4],
        | {
            let (width, height, pixels) = image
                .map(|image| (image.width, image.height, image.pixels.as_slice()))
                .unwrap_or((1, 1, fallback.as_slice()));
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * width.max(1)),
                    rows_per_image: Some(height.max(1)),
                },
                wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
            );
            texture
        };
        self.background_texture = upload("background.texture", background, [0, 0, 0, 255]);
        self.environment_texture = upload(
            "environment.texture",
            environment,
            [128, 128, 128, 255],
        );
        let background_view = self
            .background_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let environment_view = self
            .environment_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer.bind_group"),
            layout: &self.frame_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&background_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.background_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&environment_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.background_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
            ],
        });
        self.background_source_id = background_id;
        self.environment_source_id = environment_id;
    }

    /// Write the blit shader's UV crop uniform. Call in `prepare` (the only
    /// place with a `&Queue`) — `render` then just submits the draw call.
    pub fn upload_blit_uv(&self, queue: &wgpu::Queue, uv_offset: [f32; 2], uv_scale: [f32; 2]) {
        // The geometry passes render at `depth_texture_size` into the top-left
        // corner of the (possibly larger) `alloc_size` resolve texture, so the
        // rendered image occupies UV [0, render/alloc]. Scale the incoming crop
        // — computed in full-image-normalized units — down into that region.
        let fx = self.depth_texture_size.width as f32 / self.alloc_size.width.max(1) as f32;
        let fy = self.depth_texture_size.height as f32 / self.alloc_size.height.max(1) as f32;
        queue.write_buffer(
            &self.blit_uniform_buffer,
            0,
            bytemuck::cast_slice(&[
                uv_offset[0] * fx,
                uv_offset[1] * fy,
                uv_scale[0] * fx,
                uv_scale[1] * fy,
            ]),
        );
    }

    /// Render the geometry passes at `vp_size` (the full viewport size — the
    /// MSAA / resolve textures are this size) and blit the resulting resolve
    /// to `surface_dest` on the swap-chain. The UV crop is read from the
    /// blit uniform buffer (written by `upload_blit_uv` during `prepare`)
    /// so a viewport that hangs off the canvas still composites the correct
    /// sub-rectangle to the visible portion of the surface.
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        vp_size: Size<u32>,
        surface_dest: Rectangle<u32>,
        bg_color: [f32; 4],
        mesh_wireframe: bool,
        hidden_line: bool,
        show_3d_edges: bool,
    ) {
        let vp = Rectangle::<u32> {
            x: 0,
            y: 0,
            width: vp_size.width,
            height: vp_size.height,
        };
        let msaa = &self.msaa_view;
        let [r, g, b, a] = bg_color;
        let clear_color = if self.clip_boundary.is_some() {
            wgpu::Color::TRANSPARENT
        } else {
            wgpu::Color {
                r: r as f64,
                g: g as f64,
                b: b as f64,
                a: a as f64,
            }
        };
        // Non-rectangular viewport clip: the boundary is stamped into the
        // just-cleared (0x00) stencil with `Invert`, so an odd (interior)
        // coverage becomes 0xFF. Every content pass then draws with reference
        // 0xFF so only the interior survives. Rectangular / unclipped viewports
        // leave the stencil at 0 and draw with reference 0 (the viewport's own
        // render rectangle does the clipping).
        let stencil_ref: u32 = if self.clip_boundary.is_some() { 0xFF } else { 0 };

        if self.shadow_enabled && !self.skip_geometry && !mesh_wireframe {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow.render_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_uniform_bind_group, &[]);
            for (_, chunk) in self.active_mesh_chunks_indexed() {
                if !chunk.visible {
                    continue;
                }
                pass.set_bind_group(
                    1,
                    chunk
                        .material_bind_group
                        .as_ref()
                        .unwrap_or(&self.mesh_default_material_bind_group),
                    &[],
                );
                pass.set_vertex_buffer(0, chunk.vertex_buffer.slice(..));
                if chunk.index_count != 0 {
                    pass.set_index_buffer(chunk.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..chunk.index_count, 0, 0..chunk.instance_count);
                }
                if chunk.transp_index_count != 0 {
                    pass.set_index_buffer(
                        chunk.transp_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(0..chunk.transp_index_count, 0, 0..chunk.instance_count);
                }
            }
        }

        // Scene-render cache: when `prepare` found this frame pixel-identical
        // to the last (unchanged view / geometry / selection / preview), the
        // resolve texture already holds the image. Skip every geometry pass +
        // the MSAA resolve and fall straight through to the blit below. This
        // is what makes a pure cursor move cost one fullscreen blit instead of
        // re-rasterizing the whole drawing every frame.
        if !self.skip_geometry {
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
                    // Clip stencil starts at 0 (= "unclipped", passes content
                    // bound to reference 0); clip masks stamp 1 into interiors.
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // MSAA texture is clip-bounds-sized, so viewport starts at (0, 0).
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            // Stamp the viewport clip boundary into the just-cleared stencil
            // (interior → 1) before any content draws, so every pass below can
            // clip to the shape with reference 1.
            if let Some((vbuf, vcount)) = &self.clip_boundary {
                pass.set_pipeline(&self.clip_mask_pipeline);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.draw(0..*vcount, 0..1);
            }
            pass.set_pipeline(&self.background_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_stencil_reference(stencil_ref);
            pass.draw(0..3, 0..1);
            // The capability-selected façade dispatches storage or texture
            // draws before wires so outlines remain on top in either backend.
            // Skipped while navigating because per-pixel hatch work dominates
            // hatch-heavy drawings.
            if !self.skip_hatch_frame {
                self.hatch_gpu
                    .draw(&mut pass, &self.uniform_bind_group, stencil_ref);
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.image_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_stencil_reference(stencil_ref);
            for img in self.gpu_images.iter() {
                pass.set_bind_group(1, &img.bind_group, &[]);
                pass.set_vertex_buffer(0, img.vertex_buffer.slice(..));
                pass.draw(0..img.vertex_count, 0..1);
            }
        }

        if self.mesh_cull_count != 0 {
            if let (Some(pipeline), Some(bind_group)) =
                (&self.mesh_cull_pipeline, &self.mesh_cull_bind_group)
            {
                let mut pass =
                    encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("mesh.cull.compute_pass"),
                        timestamp_writes: None,
                    });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.dispatch_workgroups(self.mesh_cull_count.div_ceil(64), 1, 1);
            }
        }

        // ── Pass 4: solid meshes (batched) ────────────────────────────────
        if !self.gpu_mesh_batch.is_empty() || !self.gpu_mesh_dynamic.is_empty() {
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_stencil_reference(stencil_ref);
            // Four draw paths share this pass:
            //  - Solid:           `mesh_pipeline` + triangle index buf.
            //  - Wireframe:       `mesh_wireframe_pipeline` + the
            //                     pre-built `wire_index_buffer`.
            //  - HiddenLine:      depth prepass (`mesh_depth_pipeline`,
            //                     writes Z, no colour) → wire overlay.
            //  - Solid+Edges:     `mesh_pipeline` shaded fill → wire
            //                     overlay; LessEqual depth test on the
            //                     wire pass keeps the edges crisp on
            //                     top of the shaded surface.
            let want_solid_with_edges = !hidden_line && !mesh_wireframe && show_3d_edges;
            // Each path now binds a chunk's buffers and draws the whole chunk in
            // one call — a handful of draws total instead of one per solid. No
            // per-solid LOD / frustum cull in the batched path (the batch is
            // resident in full); that is reintroduced separately if needed.
            if hidden_line {
                // Depth-only prepass: every solid surface occludes hidden edges,
                // so both the opaque and the transparent tris write depth here.
                pass.set_pipeline(&self.mesh_depth_pipeline);
                for (mesh_command, c) in self.active_mesh_chunks_indexed() {
                    if !c.visible {
                        continue;
                    }
                    pass.set_bind_group(
                        1,
                        c.material_bind_group
                            .as_ref()
                            .unwrap_or(&self.mesh_default_material_bind_group),
                        &[],
                    );
                    pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                    if c.index_count != 0 {
                        pass.set_index_buffer(
                            c.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        if let Some(indirect) = &self.mesh_opaque_indirect {
                            pass.draw_indexed_indirect(
                                indirect,
                                mesh_command as u64
                                    * std::mem::size_of::<
                                        wgpu::util::DrawIndexedIndirectArgs,
                                    >() as u64,
                            );
                        } else {
                            pass.draw_indexed(0..c.index_count, 0, 0..c.instance_count);
                        }
                    }
                    if c.transp_index_count != 0 {
                        pass.set_index_buffer(
                            c.transp_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        if let Some(indirect) = &self.mesh_transparent_indirect {
                            pass.draw_indexed_indirect(
                                indirect,
                                mesh_command as u64
                                    * std::mem::size_of::<
                                        wgpu::util::DrawIndexedIndirectArgs,
                                    >() as u64,
                            );
                        } else {
                            pass.draw_indexed(
                                0..c.transp_index_count,
                                0,
                                0..c.instance_count,
                            );
                        }
                    }
                }
                pass.set_pipeline(&self.mesh_wireframe_pipeline);
                pass.set_bind_group(1, &self.mesh_default_material_bind_group, &[]);
                for (mesh_command, c) in self.active_mesh_chunks_indexed() {
                    if !c.visible {
                        continue;
                    }
                    pass.set_bind_group(
                        1,
                        c.material_bind_group
                            .as_ref()
                            .unwrap_or(&self.mesh_default_material_bind_group),
                        &[],
                    );
                    // Plain-mesh triangulation edges.
                    if c.wire_index_count != 0 {
                        pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            c.wire_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        if let Some(indirect) = &self.mesh_wire_indirect {
                            pass.draw_indexed_indirect(
                                indirect,
                                mesh_command as u64
                                    * std::mem::size_of::<
                                        wgpu::util::DrawIndexedIndirectArgs,
                                    >() as u64,
                            );
                        } else {
                            pass.draw_indexed(
                                0..c.wire_index_count,
                                0,
                                0..c.instance_count,
                            );
                        }
                    }
                    // ACIS solid B-rep feature edges (LineList, non-indexed).
                    if c.edge_vertex_count != 0 {
                        pass.set_vertex_buffer(0, c.edge_vertex_buffer.slice(..));
                        if let Some(indirect) = &self.mesh_edge_indirect {
                            pass.draw_indirect(
                                indirect,
                                mesh_command as u64
                                    * std::mem::size_of::<
                                        wgpu::util::DrawIndirectArgs,
                                    >() as u64,
                            );
                        } else {
                            pass.draw(0..c.edge_vertex_count, 0..c.instance_count);
                        }
                    }
                }
                // DISPSILH silhouettes (whole batch, one buffer).
                if let Some(ref vb) = self.silhouette_vbuf {
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.draw(0..self.silhouette_vcount, 0..1);
                }
            } else {
                if mesh_wireframe {
                    pass.set_pipeline(&self.mesh_wireframe_pipeline);
                    pass.set_bind_group(1, &self.mesh_default_material_bind_group, &[]);
                    for (mesh_command, c) in self.active_mesh_chunks_indexed() {
                        if !c.visible {
                            continue;
                        }
                        pass.set_bind_group(
                            1,
                            c.material_bind_group
                                .as_ref()
                                .unwrap_or(&self.mesh_default_material_bind_group),
                            &[],
                        );
                        if c.wire_index_count != 0 {
                            pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                            pass.set_index_buffer(
                                c.wire_index_buffer.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            if let Some(indirect) = &self.mesh_wire_indirect {
                                pass.draw_indexed_indirect(
                                    indirect,
                                    mesh_command as u64
                                        * std::mem::size_of::<
                                            wgpu::util::DrawIndexedIndirectArgs,
                                        >() as u64,
                                );
                            } else {
                                pass.draw_indexed(
                                    0..c.wire_index_count,
                                    0,
                                    0..c.instance_count,
                                );
                            }
                        }
                        if c.edge_vertex_count != 0 {
                            pass.set_vertex_buffer(0, c.edge_vertex_buffer.slice(..));
                            if let Some(indirect) = &self.mesh_edge_indirect {
                                pass.draw_indirect(
                                    indirect,
                                    mesh_command as u64
                                        * std::mem::size_of::<
                                            wgpu::util::DrawIndirectArgs,
                                        >() as u64,
                                );
                            } else {
                                pass.draw(0..c.edge_vertex_count, 0..c.instance_count);
                            }
                        }
                    }
                    // DISPSILH silhouettes.
                    if let Some(ref vb) = self.silhouette_vbuf {
                        pass.set_vertex_buffer(0, vb.slice(..));
                        pass.draw(0..self.silhouette_vcount, 0..1);
                    }
                } else {
                    // Opaque fills first (they write depth).
                    pass.set_pipeline(&self.mesh_pipeline);
                    for (mesh_command, c) in self.active_mesh_chunks_indexed() {
                        if !c.visible {
                            continue;
                        }
                        pass.set_bind_group(
                            1,
                            c.material_bind_group
                                .as_ref()
                                .unwrap_or(&self.mesh_default_material_bind_group),
                            &[],
                        );
                        if c.index_count == 0 {
                            continue;
                        }
                        pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                        pass.set_index_buffer(c.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                        if let Some(indirect) = &self.mesh_opaque_indirect {
                            pass.draw_indexed_indirect(
                                indirect,
                                mesh_command as u64
                                    * std::mem::size_of::<
                                        wgpu::util::DrawIndexedIndirectArgs,
                                    >() as u64,
                            );
                        } else {
                            pass.draw_indexed(0..c.index_count, 0, 0..c.instance_count);
                        }
                    }
                    // Transparent fills last, with depth writes disabled, so they
                    // blend over the opaque geometry behind them instead of
                    // culling it via the depth buffer.
                    pass.set_pipeline(&self.mesh_transparent_pipeline);
                    for (mesh_command, c) in self.active_mesh_chunks_indexed() {
                        if !c.visible {
                            continue;
                        }
                        pass.set_bind_group(
                            1,
                            c.material_bind_group
                                .as_ref()
                                .unwrap_or(&self.mesh_default_material_bind_group),
                            &[],
                        );
                        if c.transp_index_count == 0 {
                            continue;
                        }
                        pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            c.transp_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        if let Some(indirect) = &self.mesh_transparent_indirect {
                            pass.draw_indexed_indirect(
                                indirect,
                                mesh_command as u64
                                    * std::mem::size_of::<
                                        wgpu::util::DrawIndexedIndirectArgs,
                                    >() as u64,
                            );
                        } else {
                            pass.draw_indexed(
                                0..c.transp_index_count,
                                0,
                                0..c.instance_count,
                            );
                        }
                    }
                }
                // Selection / hover highlight reuses index ranges already
                // resident in the chunk buffers; hover never uploads geometry.
                for kind in [MeshHighlightKind::Selected, MeshHighlightKind::Hover] {
                    if !self
                        .mesh_highlight_draws
                        .iter()
                        .any(|draw| draw.kind == kind)
                    {
                        continue;
                    }
                    pass.set_pipeline(match kind {
                        MeshHighlightKind::Selected => &self.mesh_selected_pipeline,
                        MeshHighlightKind::Hover => &self.mesh_hover_pipeline,
                    });
                    for draw in self
                        .mesh_highlight_draws
                        .iter()
                        .filter(|draw| draw.kind == kind)
                    {
                        let chunk = if draw.range.dynamic {
                            self.gpu_mesh_dynamic.get(draw.range.chunk)
                        } else {
                            self.gpu_mesh_batch.get(draw.range.chunk)
                        };
                        let Some(chunk) = chunk else {
                            continue;
                        };
                        if !chunk.visible || draw.range.index_count == 0 {
                            continue;
                        }
                        pass.set_bind_group(
                            1,
                            chunk
                                .material_bind_group
                                .as_ref()
                                .unwrap_or(&self.mesh_default_material_bind_group),
                            &[],
                        );
                        pass.set_vertex_buffer(0, chunk.vertex_buffer.slice(..));
                        if draw.range.transparent {
                            pass.set_index_buffer(
                                chunk.transp_index_buffer.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                        } else {
                            pass.set_index_buffer(
                                chunk.index_buffer.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                        }
                        pass.draw_indexed(
                            draw.range.index_start
                                ..draw.range.index_start + draw.range.index_count,
                            0,
                            draw.range.instance_start
                                ..draw.range.instance_start + draw.range.instance_count,
                        );
                    }
                }
                // *WithEdges variants: overlay edge segments on top of the shaded
                // fill in black (mesh_edge_black_pipeline). The LessEqual depth
                // test keeps the edges visible over the fragments the fill wrote.
                if want_solid_with_edges {
                    pass.set_pipeline(&self.mesh_edge_black_pipeline);
                    for (mesh_command, c) in self.active_mesh_chunks_indexed() {
                        if !c.visible {
                            continue;
                        }
                        pass.set_bind_group(
                            1,
                            c.material_bind_group
                                .as_ref()
                                .unwrap_or(&self.mesh_default_material_bind_group),
                            &[],
                        );
                        if c.wire_index_count != 0 {
                            pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                            pass.set_index_buffer(
                                c.wire_index_buffer.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            if let Some(indirect) = &self.mesh_wire_indirect {
                                pass.draw_indexed_indirect(
                                    indirect,
                                    mesh_command as u64
                                        * std::mem::size_of::<
                                            wgpu::util::DrawIndexedIndirectArgs,
                                        >() as u64,
                                );
                            } else {
                                pass.draw_indexed(
                                    0..c.wire_index_count,
                                    0,
                                    0..c.instance_count,
                                );
                            }
                        }
                        if c.edge_vertex_count != 0 {
                            pass.set_vertex_buffer(0, c.edge_vertex_buffer.slice(..));
                            if let Some(indirect) = &self.mesh_edge_indirect {
                                pass.draw_indirect(
                                    indirect,
                                    mesh_command as u64
                                        * std::mem::size_of::<
                                            wgpu::util::DrawIndirectArgs,
                                        >() as u64,
                                );
                            } else {
                                pass.draw(0..c.edge_vertex_count, 0..c.instance_count);
                            }
                        }
                    }
                    // DISPSILH silhouettes over the shaded fill.
                    if let Some(ref vb) = self.silhouette_vbuf {
                        pass.set_vertex_buffer(0, vb.slice(..));
                        pass.draw(0..self.silhouette_vcount, 0..1);
                    }
                }
            }
        }

        // ── Pass 5a: 3DFACE fills (3D + 2D split) ─────────────────────────
        // 3D quads + PolyfaceMesh face tris go through the depth-only
        // pipeline in HiddenLine so wires hidden behind them disappear.
        // 2D fills (text greek, MultiLeader bg) always draw with colour.
        if let Some(ref fill) = self.gpu_face3d_fill {
            if !fill.chunks_3d.is_empty() || !fill.chunks_2d.is_empty() {
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
                        stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_stencil_reference(stencil_ref);
                if !fill.chunks_3d.is_empty() {
                    if hidden_line {
                        pass.set_pipeline(&self.face3d_depth_pipeline);
                    } else {
                        pass.set_pipeline(&self.face3d_pipeline);
                    }
                    for c in &fill.chunks_3d {
                        pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                        pass.draw(0..c.vertex_count, 0..1);
                    }
                }
                if !fill.chunks_2d.is_empty() {
                    pass.set_pipeline(&self.face3d_pipeline);
                    for c in &fill.chunks_2d {
                        pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                        pass.draw(0..c.vertex_count, 0..1);
                    }
                }
            }
        }

        // ── Pass 5b: 3DFACE edges (batched, possibly multiple chunks) ────
        // FlatShaded / GouraudShaded hide the 3DFACE outline (the user
        // chose a clean shaded look); every other mode keeps it.
        if show_3d_edges && !self.gpu_face3d_edges.is_empty() {
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.wire_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_stencil_reference(stencil_ref);
            for edges in &self.gpu_face3d_edges {
                if edges.instance_count > 0 {
                    if let Some(bg) = &edges.const_bind_group {
                        pass.set_bind_group(1, bg.as_ref(), &[]);
                    }
                    pass.set_vertex_buffer(0, edges.instance_buffer.slice(..));
                    pass.draw(
                        0..6,
                        edges.first_instance..edges.first_instance + edges.instance_count,
                    );
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.wire_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            // In a filled-with-edges mode the mesh outline edges frame the shaded
            // fill and should read black; wireframe / hidden-line keep the entity
            // colour. `wire_black_pipeline` shares the wire layout, so the switch
            // needs no bind-group rebind.
            let want_solid_with_edges = !hidden_line && !mesh_wireframe && show_3d_edges;
            let mut black_active = false;
            pass.set_stencil_reference(stencil_ref);
            for wire in self.gpu_wires.iter() {
                if wire.instance_count == 0 {
                    continue;
                }
                // PolyfaceMesh / PolygonMesh outline edges live in
                // `gpu_wires` (their `WireModel` has both `points` and
                // `fill_tris`). In FlatShaded / GouraudShaded the user
                // wants a clean shaded surface, so the wire pass skips
                // these instances; the *WithEdges and pure wireframe
                // modes leave the flag at true and draw them.
                if !show_3d_edges && wire.is_3d_mesh_edge {
                    continue;
                }
                let use_black = want_solid_with_edges && wire.is_3d_mesh_edge;

                if use_black != black_active {
                    pass.set_pipeline(if use_black {
                        &self.wire_black_pipeline
                    } else {
                        &self.wire_pipeline
                    });
                    black_active = use_black;
                }
                if let Some(bg) = &wire.const_bind_group {
                    pass.set_bind_group(1, bg.as_ref(), &[]);
                }
                pass.set_vertex_buffer(0, wire.instance_buffer.slice(..));
                pass.draw(
                    0..6,
                    wire.first_instance..wire.first_instance + wire.instance_count,
                );
            }
            // Live overlay wires (command preview / interim / grip drag) always
            // on top: the xray pipeline (depth_compare=Always, no depth write)
            // keeps them visible through any occluding geometry — a 3D solid, or
            // 2D geometry drawn in front — so a command preview is never hidden.
            // No scissor.
            if self.gpu_preview_wires.iter().any(|pw| pw.instance_count > 0) {
                pass.set_pipeline(&self.wire_xray_pipeline);
                for pw in &self.gpu_preview_wires {
                    if pw.instance_count > 0 {
                        if let Some(bg) = &pw.const_bind_group {
                            pass.set_bind_group(1, bg.as_ref(), &[]);
                        }
                        pass.set_vertex_buffer(0, pw.instance_buffer.slice(..));
                        pass.draw(
                            0..6,
                            pw.first_instance..pw.first_instance + pw.instance_count,
                        );
                    }
                }
            }
        }

        // ── Pass 5c: SDF text quads (drawn over wires) ────────────────────
        // Selection / rollover text is drawn later with the selected-wire xray
        // overlay, after wipeouts, so normal text cannot hide its own tint.
        if let Some(atlas) = &self.text_atlas_gpu {
            let have_base = self.text_vbuf.is_some() && self.text_vcount > 0;
            let have_preview =
                self.text_preview_vbuf.is_some() && self.text_preview_vcount > 0;
            if have_base || have_preview {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("text.render_pass"),
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
                        stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_stencil_reference(stencil_ref);
                pass.set_bind_group(1, &atlas.bind_group, &[]);
                if let Some(vbuf) = &self.text_vbuf {
                    if self.text_vcount > 0 {
                        pass.set_vertex_buffer(0, vbuf.slice(..));
                        pass.draw(0..self.text_vcount, 0..1);
                    }
                }
                // Grip-drag / command-preview glyphs, drawn over the base text.
                if let Some(pbuf) = &self.text_preview_vbuf {
                    if self.text_preview_vcount > 0 {
                        pass.set_vertex_buffer(0, pbuf.slice(..));
                        pass.draw(0..self.text_preview_vcount, 0..1);
                    }
                }
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_pipeline(&self.wipeout_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_stencil_reference(stencil_ref);
            for (i, wipeout) in self.gpu_wipeouts.iter().enumerate() {
                if self.wipeout_skip_flags.get(i).copied().unwrap_or(false) {
                    continue;
                }
                pass.set_bind_group(1, &wipeout.bind_group, &[]);
                pass.set_vertex_buffer(0, wipeout.vertex_buffer.slice(..));
                pass.draw(0..6, 0..1);
            }
        }

        // ── Pass 7: selection overlay pass ───────────────────────────────
        // Redraws selected wires and text with depth_compare=Always so both
        // appear on top of all other geometry at full brightness.
        let have_text_highlight =
            self.text_highlight_vbuf.is_some() && self.text_highlight_vcount > 0;
        if !self.gpu_selected_wires.is_empty() || have_text_highlight {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("selection_xray.render_pass"),
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, vp.width as f32, vp.height as f32, 0.0, 1.0);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_stencil_reference(stencil_ref);
            if !self.gpu_selected_wires.is_empty() {
                pass.set_pipeline(&self.wire_xray_pipeline);
                for wire in &self.gpu_selected_wires {
                    if wire.instance_count > 0 {
                        if let Some(bg) = &wire.const_bind_group {
                            pass.set_bind_group(1, bg.as_ref(), &[]);
                        }
                        pass.set_vertex_buffer(0, wire.instance_buffer.slice(..));
                        pass.draw(
                            0..6,
                            wire.first_instance..wire.first_instance + wire.instance_count,
                        );
                    }
                }
            }
            if let (Some(atlas), Some(hlbuf)) =
                (&self.text_atlas_gpu, &self.text_highlight_vbuf)
            {
                if self.text_highlight_vcount > 0 {
                    pass.set_pipeline(&self.text_highlight_pipeline);
                    pass.set_bind_group(1, &atlas.bind_group, &[]);
                    pass.set_vertex_buffer(0, hlbuf.slice(..));
                    pass.draw(0..self.text_highlight_vcount, 0..1);
                }
            }
        }

        // ── Resolve MSAA → resolve texture ────────────────────────────────
        // Both are the same (rounded `alloc_size`) offscreen texture, so the
        // resolve never touches the surface. Only the drawn [0, render_size]
        // corner holds content; the rounded border stays the cleared bg color
        // and is never sampled (the blit UV is scaled to render/alloc).
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
                multiview_mask: None,
            });
            // No draw calls — the pass itself triggers the MSAA resolve.
        }
        } // end `if !self.skip_geometry`

        // ── Blit resolve texture → surface target at surface_dest position ──
        // The viewport maps the NDC quad to exactly `surface_dest` in the
        // swap-chain; `uv_offset` + `uv_scale` (passed through the blit
        // uniform) crop the resolve so we sample only the visible portion
        // of the full viewport's MSAA texture.
        if surface_dest.width > 0 && surface_dest.height > 0 {
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
                multiview_mask: None,
            });
            pass.set_viewport(
                surface_dest.x as f32,
                surface_dest.y as f32,
                surface_dest.width as f32,
                surface_dest.height as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.blit_bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    pub fn ensure_depth_texture(&mut self, device: &wgpu::Device, size: Size<u32>) {
        // Record the requested render size every frame (the blit UV scale reads
        // it); only reallocate the textures when the *rounded* size changes.
        self.depth_texture_size = size;
        let alloc = Size::new(round_up_tex(size.width), round_up_tex(size.height));
        if self.alloc_size != alloc {
            let size = alloc;
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
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.blit_uniform_buffer.as_entire_binding(),
                    },
                ],
            });
            self.resolve_view = resolve_view;
            self.alloc_size = alloc;
        }
    }
}

/// Round a texture dimension up to a coarse grid so a live divider drag
/// doesn't recreate the depth / MSAA / resolve textures on every frame. See
/// `alloc_size` — this is the mitigation for the Windows-Firefox freeze (#191).
fn round_up_tex(n: u32) -> u32 {
    const GRID: u32 = 128;
    ((n.max(1) + GRID - 1) / GRID) * GRID
}

fn aabb3_offscreen(
    aabb: [f32; 6],
    view_rot: glam::Mat4,
    eye: glam::DVec3,
    clip_w: u32,
    clip_h: u32,
) -> bool {
    if aabb.iter().any(|value| !value.is_finite()) {
        return false;
    }
    let w = clip_w as f32;
    let h = clip_h as f32;
    let mut min_px = f32::INFINITY;
    let mut max_px = f32::NEG_INFINITY;
    let mut min_py = f32::INFINITY;
    let mut max_py = f32::NEG_INFINITY;
    for x in [aabb[0], aabb[3]] {
        for y in [aabb[1], aabb[4]] {
            for z in [aabb[2], aabb[5]] {
                let relative =
                    (glam::DVec3::new(x as f64, y as f64, z as f64) - eye).as_vec3();
                let clip = view_rot * relative.extend(1.0);
                if !clip.is_finite() || clip.w <= f32::EPSILON {
                    return false;
                }
                let ndc = clip.truncate() / clip.w;
                let px = (ndc.x + 1.0) * 0.5 * w;
                let py = (1.0 - ndc.y) * 0.5 * h;
                min_px = min_px.min(px);
                max_px = max_px.max(px);
                min_py = min_py.min(py);
                max_py = max_py.max(py);
            }
        }
    }
    const MARGIN_FRAC: f32 = 0.25;
    let mx = w * MARGIN_FRAC;
    let my = h * MARGIN_FRAC;
    max_px < -mx || min_px > w + mx || max_py < -my || min_py > h + my
}

/// `true` when the world-XY AABB projects entirely outside the
/// viewport rect (extended by `MARGIN_FRAC` to absorb pan inertia and
/// avoid edge pop-in). Phase 2.2 mesh-frustum / Phase 2.3 hatch +
/// wipeout cull. Equivalent to a 2D bounding-box rejection test in
/// NDC; uses the same 4-corner projection that LOD picking already
/// does, so the extra cost is negligible.
///
/// IMPORTANT: the AABB must be in the same local space (world_offset
/// subtracted) that `view_proj` expects. `WipeoutGpu.world_aabb` rebuilds
/// the absolute local-space rect from `model.world_origin + boundary
/// extents` for this reason; meshes already store an absolute rect.
fn aabb_offscreen(
    aabb: [f32; 4],
    view_rot: glam::Mat4,
    eye: glam::DVec3,
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
        view_rot.project_point3((glam::DVec3::new(x0 as f64, y0 as f64, 0.0) - eye).as_vec3()),
        view_rot.project_point3((glam::DVec3::new(x1 as f64, y0 as f64, 0.0) - eye).as_vec3()),
        view_rot.project_point3((glam::DVec3::new(x0 as f64, y1 as f64, 0.0) - eye).as_vec3()),
        view_rot.project_point3((glam::DVec3::new(x1 as f64, y1 as f64, 0.0) - eye).as_vec3()),
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
    // 25% pad on each side — matches `view_world_aabb` (wire path),
    // keeps edge geometry rendered while panning before the next
    // upload reaches the GPU.
    const MARGIN_FRAC: f32 = 0.25;
    let mx = w * MARGIN_FRAC;
    let my = h * MARGIN_FRAC;
    max_px < -mx || min_px > w + mx || max_py < -my || min_py > h + my
}

/// Return `true` when the world-XY AABB's screen-space size is below the
/// given pixel threshold. Used by LOD passes (hatch skip, etc.) to drop
/// draw calls that wouldn't contribute a visible pixel.
fn aabb_below_pixel(
    aabb: [f32; 4],
    view_rot: glam::Mat4,
    eye: glam::DVec3,
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
        view_rot.project_point3((glam::DVec3::new(x0 as f64, y0 as f64, 0.0) - eye).as_vec3()),
        view_rot.project_point3((glam::DVec3::new(x1 as f64, y0 as f64, 0.0) - eye).as_vec3()),
        view_rot.project_point3((glam::DVec3::new(x0 as f64, y1 as f64, 0.0) - eye).as_vec3()),
        view_rot.project_point3((glam::DVec3::new(x1 as f64, y1 as f64, 0.0) - eye).as_vec3()),
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
        format: wgpu::TextureFormat::Depth24PlusStencil8,
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

/// Holds one inner `Pipeline` per viewport drawn this frame. A single
/// shader widget owns one `MultiPipeline`; the unified renderer grows the
/// `inners` vector to match the viewport count and draws each into its own
/// screen rectangle. Inner `Pipeline` code (upload / LOD / render / blit)
/// is unchanged — it just runs once per viewport.
pub struct MultiPipeline {
    pub(crate) inners: Vec<Pipeline>,
    format: wgpu::TextureFormat,
    /// Stable viewport identity → pipeline slot map. Paper viewports used to
    /// occupy slots by their current list position, so switching layouts or
    /// scrolling a sheet could assign an existing viewport to a different
    /// slot and throw away all of its GPU caches. Keep the association across
    /// tab switches and only recycle genuinely cold slots.
    pub(crate) slot_by_instance: rustc_hash::FxHashMap<u64, usize>,
    slot_last_used: Vec<u64>,
    slot_clock: u64,
    /// The resident wire batches, keyed by `wire_content_id` and shared across
    /// every slot (and every pane — one `MultiPipeline` backs all of them) that
    /// renders the same content. `prepare` builds an entry once on a cache miss
    /// then hands `Arc` clones to each slot, so N paper viewports / Model tiles
    /// showing one identical resident set upload the wire vertex buffers exactly
    /// once between them instead of once per slot. Kept trim by dropping entries
    /// no slot still references (`Arc::strong_count == 1`) once it grows past a
    /// small bound.
    pub(crate) wire_buffer_cache: rustc_hash::FxHashMap<
        u64,
        (
            std::sync::Arc<Vec<WireGpu>>,
            std::sync::Arc<rustc_hash::FxHashMap<u64, Vec<u32>>>,
        ),
    >,
}

impl MultiPipeline {
    /// Ensure at least `n` (≥1) inner pipelines exist, creating any missing
    /// ones. Grow-only: extra pipelines are NOT dropped, because per-pane Model
    /// shader widgets share this storage and each only touches its own slot —
    /// truncating mid-frame would destroy another pane's slot. Stale inners (a
    /// closed pane, or fewer paper viewports) are harmless idle GPU resources.
    pub(crate) fn ensure_len(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        n: usize,
    ) {
        let n = n.max(1);
        while self.inners.len() < n {
            self.inners.push(Pipeline::new(device, queue, self.format));
            self.slot_last_used.push(0);
        }
    }

    /// Resolve stable slots for the viewport identities in one primitive.
    /// Thirty-two hot slots cover ordinary tiled/paper drawings. A cold slot
    /// is recycled only after several other prepare calls, which prevents
    /// sibling Model panes prepared in the same frame from evicting each
    /// other. If every slot is still hot, growing is safer than a visible
    /// rebuild hitch.
    pub(crate) fn resolve_slots(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instance_ids: &[u64],
    ) -> Vec<usize> {
        const SOFT_LIMIT: usize = 32;
        const HOT_WINDOW: u64 = 8;

        self.slot_clock = self.slot_clock.wrapping_add(1).max(1);
        let now = self.slot_clock;
        let reserved: rustc_hash::FxHashSet<u64> = instance_ids.iter().copied().collect();
        // `inner.slot_id` is updated later in `prepare`, after this whole
        // method returns. Without a per-call claim set, two new viewports in
        // the same primitive both see the freshly-grown slot as `MAX` and get
        // assigned to it. Paper then blits the final occupant's resolve texture
        // once as the full sheet and again as the floating viewport.
        let mut claimed: rustc_hash::FxHashSet<usize> =
            rustc_hash::FxHashSet::default();
        let mut slots = Vec::with_capacity(instance_ids.len());

        for &instance_id in instance_ids {
            let existing = self
                .slot_by_instance
                .get(&instance_id)
                .copied()
                .filter(|slot| !claimed.contains(slot));
            let slot = if let Some(slot) = existing {
                slot
            } else {
                self.slot_by_instance.remove(&instance_id);
                let vacant = self
                    .inners
                    .iter()
                    .enumerate()
                    .find(|(slot, inner)| {
                        !claimed.contains(slot) && inner.slot_id == u64::MAX
                    })
                    .map(|(slot, _)| slot);
                let recyclable = vacant.or_else(|| {
                    (self.inners.len() >= SOFT_LIMIT)
                        .then(|| {
                            self.inners
                                .iter()
                                .enumerate()
                                .filter(|(slot, _)| !claimed.contains(slot))
                                .filter(|(_, inner)| !reserved.contains(&inner.slot_id))
                                .filter(|(slot, _)| {
                                    now.saturating_sub(self.slot_last_used[*slot]) > HOT_WINDOW
                                })
                                .min_by_key(|(slot, _)| self.slot_last_used[*slot])
                                .map(|(slot, _)| slot)
                        })
                        .flatten()
                });
                let slot = recyclable.unwrap_or_else(|| {
                    let slot = self.inners.len();
                    self.ensure_len(device, queue, slot + 1);
                    slot
                });
                let old_id = self.inners[slot].slot_id;
                if old_id != u64::MAX {
                    self.slot_by_instance.remove(&old_id);
                }
                self.slot_by_instance.insert(instance_id, slot);
                slot
            };
            claimed.insert(slot);
            self.slot_last_used[slot] = now;
            slots.push(slot);
        }
        slots
    }
}

/// Send wgpu's uncaptured validation errors to stderr instead of the default
/// handler, which panics — and inside iced's main-thread redraw that panic
/// aborts the process, taking the user's unsaved work with it (#358). A bad
/// frame must degrade, not end the session.
///
/// The handler slot is per-device and single: this also catches iced's own draw
/// errors, and nothing else can report them, so it must stay loud enough to be
/// noticed. A validation error normally repeats on every frame, though, so a
/// plain print would flood stderr at refresh rate and bury the first (most
/// useful) message. Log the 1st, 2nd, 4th, 8th … occurrence instead: the error
/// is never hidden, the count shows it is ongoing, and the output stays finite.
///
/// Installed from `MultiPipeline::new`, which runs once per device — NOT from
/// `Pipeline::new`, which runs again for every pane `ensure_len` adds. If iced
/// ever rebuilds the device it builds a new `MultiPipeline` too, so the fresh
/// device is covered (and the throttle restarts with it).
fn install_gpu_error_handler(device: &wgpu::Device) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEEN: AtomicUsize = AtomicUsize::new(0);
    SEEN.store(0, Ordering::Relaxed);
    device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
        let n = SEEN.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_power_of_two() {
            eprintln!("[gpu] uncaptured wgpu error #{n} (frame degraded, session kept alive): {e}");
        }
    }));
}

impl iced::widget::shader::Pipeline for MultiPipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        install_gpu_error_handler(device);
        Self {
            inners: vec![Pipeline::new(device, queue, format)],
            format,
            slot_by_instance: rustc_hash::FxHashMap::default(),
            slot_last_used: vec![0],
            slot_clock: 0,
            wire_buffer_cache: rustc_hash::FxHashMap::default(),
        }
    }
}

/// The two silhouette angles of a cone/cylinder face for a view direction,
/// expressed in the face's `(u, v, axis)` frame via the view's components on
/// each: `du = view·u`, `dv = view·v`, `da = view·axis`. `tan_a` is the cone
/// taper (0 for a cylinder).
///
/// The outward normal is edge-on to the view where `du·cosθ + dv·sinθ =
/// -tanα·da`, i.e. `θ = φ ± acos(-tanα·da / |view⊥|)` with `φ = atan2(dv, du)`.
/// `None` when the view runs down the axis (no outline) or the whole cone faces
/// toward/away (`|arg| > 1`).
fn silhouette_thetas(du: f64, dv: f64, da: f64, tan_a: f64) -> Option<(f64, f64)> {
    let r_perp = (du * du + dv * dv).sqrt();
    if r_perp < 1e-6 {
        return None;
    }
    let arg = -tan_a * da / r_perp;
    if arg.abs() > 1.0 {
        return None;
    }
    let phi = dv.atan2(du);
    let delta = arg.acos();
    Some((phi + delta, phi - delta))
}
