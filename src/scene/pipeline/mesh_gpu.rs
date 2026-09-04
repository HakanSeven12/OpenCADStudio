// GPU buffers for triangle meshes.

use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};
use iced::wgpu;

// ── Vertex layout ─────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub position_low: [f32; 3],
    pub uv_diffuse: [f32; 2],
    pub uv_specular: [f32; 2],
    pub uv_reflection: [f32; 2],
    pub uv_opacity: [f32; 2],
    pub uv_bump: [f32; 2],
    pub uv_refraction: [f32; 2],
    pub uv_normal: [f32; 2],
}

impl MeshVertex {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, position) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, normal) as u64,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, position_low) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_diffuse) as u64,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_specular) as u64,
                shader_location: 10,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_opacity) as u64,
                shader_location: 11,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_refraction) as u64,
                shader_location: 12,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRS,
        }
    }

    /// Surface-independent layout for mesh edge pipelines.
    pub fn edge_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshEdgeVertex, position) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshEdgeVertex, color) as u64,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshEdgeVertex, position_low) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x3,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshEdgeVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshPlainVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub position_low: [f32; 3],
}

impl MeshPlainVertex {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshPlainVertex, position) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshPlainVertex, normal) as u64,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshPlainVertex, position_low) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x3,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshEdgeVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub position_low: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SilhouetteVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub position_low: [f32; 3],
}

impl SilhouetteVertex {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(SilhouetteVertex, position) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(SilhouetteVertex, color) as u64,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(SilhouetteVertex, position_low) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x3,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SilhouetteInstance {
    pub translation: [f32; 3],
    pub translation_low: [f32; 3],
}

impl SilhouetteInstance {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(SilhouetteInstance, translation) as u64,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(SilhouetteInstance, translation_low) as u64,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x3,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshInstanceGpu {
    pub model_row_0: [f32; 4],
    pub model_row_1: [f32; 4],
    pub model_row_2: [f32; 4],
    pub translation_low: [f32; 4],
    pub normal_row_0: [f32; 4],
    pub normal_row_1: [f32; 4],
    pub normal_row_2: [f32; 4],
}

impl MeshInstanceGpu {
    fn identity() -> Self {
        Self {
            model_row_0: [1.0, 0.0, 0.0, 0.0],
            model_row_1: [0.0, 1.0, 0.0, 0.0],
            model_row_2: [0.0, 0.0, 1.0, 0.0],
            translation_low: [0.0; 4],
            normal_row_0: [1.0, 0.0, 0.0, 0.0],
            normal_row_1: [0.0, 1.0, 0.0, 0.0],
            normal_row_2: [0.0, 0.0, 1.0, 0.0],
        }
    }

    fn from_transform(transform: acadrust::types::Transform) -> Self {
        let m = transform.matrix.m;
        let translation = [m[0][3], m[1][3], m[2][3]];
        let translation_high = [
            translation[0] as f32,
            translation[1] as f32,
            translation[2] as f32,
        ];
        let linear = glam::DMat3::from_cols_array(&[
            m[0][0], m[1][0], m[2][0],
            m[0][1], m[1][1], m[2][1],
            m[0][2], m[1][2], m[2][2],
        ]);
        let normal = if linear.determinant().abs() > 1e-18 {
            linear.inverse().transpose()
        } else {
            glam::DMat3::IDENTITY
        };
        let n = normal.to_cols_array();
        Self {
            model_row_0: [
                m[0][0] as f32,
                m[0][1] as f32,
                m[0][2] as f32,
                translation_high[0],
            ],
            model_row_1: [
                m[1][0] as f32,
                m[1][1] as f32,
                m[1][2] as f32,
                translation_high[1],
            ],
            model_row_2: [
                m[2][0] as f32,
                m[2][1] as f32,
                m[2][2] as f32,
                translation_high[2],
            ],
            translation_low: [
                (translation[0] - translation_high[0] as f64) as f32,
                (translation[1] - translation_high[1] as f64) as f32,
                (translation[2] - translation_high[2] as f64) as f32,
                0.0,
            ],
            normal_row_0: [n[0] as f32, n[3] as f32, n[6] as f32, 0.0],
            normal_row_1: [n[1] as f32, n[4] as f32, n[7] as f32, 0.0],
            normal_row_2: [n[2] as f32, n[5] as f32, n[8] as f32, 0.0],
        }
    }

    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshInstanceGpu, model_row_0) as u64,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshInstanceGpu, model_row_1) as u64,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshInstanceGpu, model_row_2) as u64,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshInstanceGpu, translation_low) as u64,
                shader_location: 8,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshInstanceGpu, normal_row_0) as u64,
                shader_location: 9,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshInstanceGpu, normal_row_1) as u64,
                shader_location: 13,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshInstanceGpu, normal_row_2) as u64,
                shader_location: 14,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

// ── Batched mesh buffers ──────────────────────────────────────────────────
//
// One GPU allocation per solid means one vertex/index bind + draw call per solid —
// ~10k draw calls a frame on a heavy 3D model, which strangles the GPU front
// end. The batch concatenates every solid's LOD0 geometry into a handful of
// large buffers (split only to stay under the 256 MB per-buffer cap), so the
// whole mesh set draws in a few calls. Vertices already carry their own colour,
// so no per-mesh state is needed between draws. Built once per geometry epoch —
// selection/hover no longer rebuild it (that tint is dropped in the batch path).

#[derive(Clone)]
pub struct MeshBatchChunk {
    pub vertex_buffer: wgpu::Buffer,
    pub compact_vertices: bool,
    /// Opaque triangle indices (mesh colour alpha ≈ 1). Drawn with depth write.
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    /// Transparent triangle indices (mesh colour alpha < 1). Drawn after the
    /// opaque fills with depth write disabled so they blend over — rather than
    /// erase — the geometry behind them.
    pub transp_index_buffer: wgpu::Buffer,
    pub transp_index_count: u32,
    /// Expanded triangle-edge line list for plain meshes that carry no B-rep
    /// edges. Keeping the edge endpoints together avoids coupling this buffer
    /// to the surface mesh's indexing and vertex representation.
    pub wire_vertex_buffer: wgpu::Buffer,
    pub wire_vertex_count: u32,
    /// B-rep feature edges of ACIS solids, as a standalone LineList vertex
    /// buffer (pairs of endpoints), drawn non-indexed. Empty for plain meshes.
    pub edge_vertex_buffer: wgpu::Buffer,
    pub edge_vertex_count: u32,
    pub instance_buffer: wgpu::Buffer,
    pub instance_count: u32,
    pub highlight_ranges: Vec<MeshBatchRange>,
    pub handles: rustc_hash::FxHashSet<acadrust::Handle>,
    pub material: Option<crate::scene::model::material_model::MeshMaterial>,
    pub face_color: [f32; 4],
    pub material_bind_group: Option<wgpu::BindGroup>,
}

#[derive(Clone, Copy)]
pub struct MeshBatchRange {
    pub handle: acadrust::Handle,
    pub index_start: u32,
    pub index_count: u32,
    pub transparent: bool,
    pub instance_start: u32,
    pub instance_count: u32,
}

struct MeshBatchStubs {
    index: wgpu::Buffer,
    vertex: wgpu::Buffer,
}

impl MeshBatchStubs {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            index: super::gpu_upload::upload_buffer(
                device,
                queue,
                "mesh.batch.empty_ibuf",
                &[0u32],
                wgpu::BufferUsages::INDEX,
            ),
            vertex: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh.batch.empty_vbuf"),
                size: std::mem::size_of::<MeshVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            }),
        }
    }
}

fn make_chunk(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    stubs: &MeshBatchStubs,
    verts: &[MeshVertex],
    indices: &[u32],
    transp_indices: &[u32],
    wire_indices: &[u32],
    edge_verts: &[MeshEdgeVertex],
    highlight_ranges: &[MeshBatchRange],
    instances: &[MeshInstanceGpu],
    handles: &rustc_hash::FxHashSet<acadrust::Handle>,
    _bounds_override: Option<[f32; 6]>,
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
    face_color: [f32; 4],
    vertex_buffer_override: Option<wgpu::Buffer>,
    edge_buffer_override: Option<wgpu::Buffer>,
) -> MeshBatchChunk {
    let mk_index = |data: &[u32], label: &'static str| {
        if data.is_empty() {
            return stubs.index.clone();
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of_val(data) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(data));
        buffer
    };
    let mk_vertex = |data: &[MeshVertex], label: &'static str| {
        if data.is_empty() {
            return stubs.vertex.clone();
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of_val(data) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(data));
        buffer
    };
    let compact_vertices = !material_has_textures(material);
    let mk_plain_vertex = |data: &[MeshVertex]| {
        if data.is_empty() {
            return stubs.vertex.clone();
        }
        let compact: Vec<MeshPlainVertex> = data
            .iter()
            .map(|vertex| MeshPlainVertex {
                position: vertex.position,
                normal: vertex.normal,
                position_low: vertex.position_low,
            })
            .collect();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mesh.batch.plain_vbuf"),
            size: std::mem::size_of_val(compact.as_slice()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&compact));
        buffer
    };
    let mk_edge_vertex = |data: &[MeshEdgeVertex], label: &'static str| {
        if data.is_empty() {
            return stubs.vertex.clone();
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of_val(data) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(data));
        buffer
    };
    let identity = [MeshInstanceGpu::identity()];
    let instance_data = if instances.is_empty() {
        &identity[..]
    } else {
        instances
    };
    let mut wire_vertices = Vec::with_capacity(wire_indices.len());
    for line in wire_indices.chunks_exact(2) {
        let (Some(start), Some(end)) = (
            verts.get(line[0] as usize),
            verts.get(line[1] as usize),
        ) else {
            continue;
        };
        for vertex in [start, end] {
            wire_vertices.push(MeshEdgeVertex {
                position: vertex.position,
                color: face_color,
                position_low: vertex.position_low,
            });
        }
    }
    MeshBatchChunk {
        vertex_buffer: vertex_buffer_override.unwrap_or_else(|| {
            if compact_vertices {
                mk_plain_vertex(verts)
            } else {
                mk_vertex(verts, "mesh.batch.vbuf")
            }
        }),
        compact_vertices,
        index_buffer: mk_index(indices, "mesh.batch.ibuf"),
        index_count: indices.len() as u32,
        transp_index_buffer: mk_index(transp_indices, "mesh.batch.transp_ibuf"),
        transp_index_count: transp_indices.len() as u32,
        wire_vertex_buffer: mk_edge_vertex(&wire_vertices, "mesh.batch.wire_vbuf"),
        wire_vertex_count: wire_vertices.len() as u32,
        edge_vertex_buffer: edge_buffer_override
            .unwrap_or_else(|| mk_edge_vertex(edge_verts, "mesh.batch.edge_vbuf")),
        edge_vertex_count: edge_verts.len() as u32,
        instance_buffer: {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh.batch.instances"),
                size: std::mem::size_of_val(instance_data) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buffer, 0, bytemuck::cast_slice(instance_data));
            buffer
        },
        instance_count: instance_data.len() as u32,
        highlight_ranges: highlight_ranges.to_vec(),
        handles: handles.clone(),
        material: material.cloned(),
        face_color,
        material_bind_group: None,
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialMapParams {
    /// diffuse, specular, reflection and opacity blend factors.
    blends0: [f32; 4],
    /// Presence bits for the same four maps.
    present0: [u32; 4],
    /// bump, refraction, normal and reserved blend factors.
    blends1: [f32; 4],
    /// Presence bits for the same four maps.
    present1: [u32; 4],
    /// Tiling modes for diffuse, specular, reflection and opacity.
    tiling0: [u32; 4],
    /// Tiling modes for bump, refraction, normal and reserved.
    tiling1: [u32; 4],
    /// Two-sided, normal-map method, global-illumination and final-gather modes.
    render_modes: [u32; 4],
    /// Advanced-data-present, anonymous and two reserved source-state bits.
    source_state: [u32; 4],
    /// Color-bleed scale and reserved render values.
    indirect: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshSurfaceParams {
    face_color: [f32; 4],
    material: [f32; 4],
    specular: [f32; 4],
    ambient: [f32; 4],
    advanced: [f32; 4],
    flags: [u32; 4],
}

fn upload_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    image: Option<&crate::scene::model::material_model::MaterialImage>,
    fallback: [u8; 4],
    srgb: bool,
) -> wgpu::TextureView {
    let (width, height, pixels) = image.map_or((1, 1, fallback.as_slice()), |image| {
        (image.width, image.height, image.rgba.as_slice())
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
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
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

pub struct MeshMaterialResources {
    diffuse_view: wgpu::TextureView,
    specular_view: wgpu::TextureView,
    reflection_view: wgpu::TextureView,
    opacity_view: wgpu::TextureView,
    bump_view: wgpu::TextureView,
    refraction_view: wgpu::TextureView,
    normal_view: wgpu::TextureView,
    diffuse_sampler: wgpu::Sampler,
    specular_sampler: wgpu::Sampler,
    reflection_sampler: wgpu::Sampler,
    opacity_sampler: wgpu::Sampler,
    bump_sampler: wgpu::Sampler,
    refraction_sampler: wgpu::Sampler,
    normal_sampler: wgpu::Sampler,
    params_buffer: wgpu::Buffer,
}

pub fn create_material_resources(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
) -> MeshMaterialResources {
    let diffuse = material.and_then(|material| material.diffuse_map.image.as_deref());
    let specular = material.and_then(|material| material.specular_map.image.as_deref());
    let reflection = material.and_then(|material| material.reflection_map.image.as_deref());
    let opacity = material.and_then(|material| material.opacity_map.image.as_deref());
    let bump = material.and_then(|material| material.bump_map.image.as_deref());
    let refraction = material.and_then(|material| material.refraction_map.image.as_deref());
    let normal = material.and_then(|material| material.normal_map.image.as_deref());
    let diffuse_view =
        upload_rgba_texture(device, queue, "mesh.material.diffuse", diffuse, [255; 4], true);
    let specular_view =
        upload_rgba_texture(device, queue, "mesh.material.specular", specular, [255; 4], true);
    let reflection_view =
        upload_rgba_texture(device, queue, "mesh.material.reflection", reflection, [0, 0, 0, 255], true);
    let opacity_view =
        upload_rgba_texture(device, queue, "mesh.material.opacity", opacity, [255; 4], false);
    let bump_view =
        upload_rgba_texture(device, queue, "mesh.material.bump", bump, [128, 128, 128, 255], false);
    let refraction_view =
        upload_rgba_texture(device, queue, "mesh.material.refraction", refraction, [255; 4], true);
    let normal_view = upload_rgba_texture(
        device,
        queue,
        "mesh.material.normal",
        normal,
        [128, 128, 255, 255],
        false,
    );
    let sampler = |label: &'static str, tiling: u8| {
        let address = match tiling {
            0 | 1 => wgpu::AddressMode::Repeat,
            4 => wgpu::AddressMode::MirrorRepeat,
            _ => wgpu::AddressMode::ClampToEdge,
        };
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: address,
            address_mode_v: address,
            address_mode_w: address,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        })
    };
    let tiling = |channel: usize| {
        material.map_or(1, |material| {
            [
                material.diffuse_map.tiling,
                material.specular_map.tiling,
                material.reflection_map.tiling,
                material.opacity_map.tiling,
                material.bump_map.tiling,
                material.refraction_map.tiling,
                material.normal_map.tiling,
            ][channel]
        })
    };
    let diffuse_sampler = sampler("mesh.material.diffuse_sampler", tiling(0));
    let specular_sampler = sampler("mesh.material.specular_sampler", tiling(1));
    let reflection_sampler = sampler("mesh.material.reflection_sampler", tiling(2));
    let opacity_sampler = sampler("mesh.material.opacity_sampler", tiling(3));
    let bump_sampler = sampler("mesh.material.bump_sampler", tiling(4));
    let refraction_sampler = sampler("mesh.material.refraction_sampler", tiling(5));
    let normal_sampler = sampler("mesh.material.normal_sampler", tiling(6));
    let channel_flags = material.map_or(0x7f, |material| material.channel_flags as u32);
    let channel_present = |channel: usize, bit: u32| {
        material.is_some_and(|material| {
            let maps = [
                &material.diffuse_map,
                &material.specular_map,
                &material.reflection_map,
                &material.opacity_map,
                &material.bump_map,
                &material.refraction_map,
                &material.normal_map,
            ];
            channel_flags & bit != 0 && maps[channel].image.is_some()
        }) as u32
    };
    let params = MaterialMapParams {
        blends0: material.map_or([0.0; 4], |material| {
            [
                material.diffuse_map.blend_factor,
                material.specular_map.blend_factor,
                material.reflection_map.blend_factor,
                material.opacity_map.blend_factor,
            ]
        }),
        present0: material.map_or([0; 4], |_material| {
            [
                channel_present(0, 0x01),
                channel_present(1, 0x02),
                channel_present(2, 0x04),
                channel_present(3, 0x08),
            ]
        }),
        blends1: material.map_or([0.0; 4], |material| {
            [
                material.bump_map.blend_factor,
                material.refraction_map.blend_factor,
                material.normal_map.blend_factor,
                0.0,
            ]
        }),
        present1: material.map_or([0; 4], |material| {
            [
                channel_present(4, 0x10),
                channel_present(5, 0x20),
                material.normal_map.image.is_some() as u32
                    * (material.normal_map_method == 0) as u32,
                0,
            ]
        }),
        tiling0: material.map_or([1; 4], |material| {
            [
                material.diffuse_map.tiling as u32,
                material.specular_map.tiling as u32,
                material.reflection_map.tiling as u32,
                material.opacity_map.tiling as u32,
            ]
        }),
        tiling1: material.map_or([1; 4], |material| {
            [
                material.bump_map.tiling as u32,
                material.refraction_map.tiling as u32,
                material.normal_map.tiling as u32,
                1,
            ]
        }),
        render_modes: material.map_or([1, 0, 0, 0], |material| {
            [
                material.two_sided as u32,
                material.normal_map_method as u32,
                material.global_illumination as u32,
                material.final_gather as u32,
            ]
        }),
        source_state: material.map_or([0; 4], |material| {
            [
                material.advanced_data_present as u32,
                material.is_anonymous as u32,
                material.handle.is_some() as u32,
                0,
            ]
        }),
        indirect: material.map_or([1.0, 0.0, 0.0, 0.0], |material| {
            [material.color_bleed_scale, 0.0, 0.0, 0.0]
        }),
    };
    let params_buffer = super::gpu_upload::upload_buffer(
        device,
        queue,
        "mesh.material.params",
        std::slice::from_ref(&params),
        wgpu::BufferUsages::UNIFORM,
    );
    MeshMaterialResources {
        diffuse_view,
        specular_view,
        reflection_view,
        opacity_view,
        bump_view,
        refraction_view,
        normal_view,
        diffuse_sampler,
        specular_sampler,
        reflection_sampler,
        opacity_sampler,
        bump_sampler,
        refraction_sampler,
        normal_sampler,
        params_buffer,
    }
}

pub fn create_material_bind_group_from_resources(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    resources: &MeshMaterialResources,
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
    face_color: [f32; 4],
) -> wgpu::BindGroup {
    let (material_params, specular, ambient, advanced, flags) =
        material_vertex_params(material);
    let surface = MeshSurfaceParams {
        face_color,
        material: material_params,
        specular,
        ambient,
        advanced,
        flags,
    };
    let surface_buffer = super::gpu_upload::upload_buffer(
        device,
        queue,
        "mesh.material.surface",
        std::slice::from_ref(&surface),
        wgpu::BufferUsages::UNIFORM,
    );
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh.material.bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&resources.diffuse_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&resources.specular_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&resources.reflection_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&resources.opacity_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&resources.bump_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&resources.refraction_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&resources.normal_view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&resources.diffuse_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::Sampler(&resources.specular_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::Sampler(&resources.reflection_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::Sampler(&resources.opacity_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::Sampler(&resources.bump_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: wgpu::BindingResource::Sampler(&resources.refraction_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: wgpu::BindingResource::Sampler(&resources.normal_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 14,
                resource: resources.params_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 16,
                resource: surface_buffer.as_entire_binding(),
            },
        ],
    })
}

pub fn create_material_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
) -> wgpu::BindGroup {
    let resources = create_material_resources(device, queue, material);
    let color = material.map_or([0.8, 0.8, 0.8, 1.0], |material| material.diffuse);
    create_material_bind_group_from_resources(
        device,
        queue,
        layout,
        &resources,
        material,
        color,
    )
}

pub fn upload_chunk_material_bind_groups(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    chunks: &mut [MeshBatchChunk],
) {
    let mut fallback = None;
    let mut materials = rustc_hash::FxHashMap::default();
    for chunk in chunks {
        let resources = match chunk.material.as_ref() {
            None => fallback
                .get_or_insert_with(|| create_material_resources(device, queue, None)),
            Some(material) => {
                let Some(handle) = material.handle else {
                    let resources = create_material_resources(device, queue, Some(material));
                    chunk.material_bind_group = Some(create_material_bind_group_from_resources(
                        device,
                        queue,
                        layout,
                        &resources,
                        chunk.material.as_ref(),
                        chunk.face_color,
                    ));
                    continue;
                };
                materials.entry(handle.value()).or_insert_with(|| {
                    create_material_resources(device, queue, Some(material))
                })
            }
        };
        chunk.material_bind_group = Some(create_material_bind_group_from_resources(
            device,
            queue,
            layout,
            resources,
            chunk.material.as_ref(),
            chunk.face_color,
        ));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MaterialBatchKey([u32; 40]);

fn material_key(
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
    color: [f32; 4],
) -> MaterialBatchKey {
    let mut key = [0u32; 40];
    key[2..6].copy_from_slice(&color.map(f32::to_bits));
    let Some(material) = material else {
        return MaterialBatchKey(key);
    };
    let handle = material.handle.map_or(0, |handle| handle.value());
    key[0] = handle as u32;
    key[1] = (handle >> 32) as u32;
    key[6..10].copy_from_slice(&material.diffuse.map(f32::to_bits));
    key[10..13].copy_from_slice(&material.ambient.map(f32::to_bits));
    key[13..16].copy_from_slice(&material.specular.map(f32::to_bits));
    key[16] = material.gloss.to_bits();
    key[17] = material.reflectivity.to_bits();
    key[18] = material.self_illumination.to_bits();
    key[19] = material.translucence.to_bits();
    key[20] = material.refraction_index.to_bits();
    key[21] = material.luminance.to_bits();
    key[22] = material.two_sided as u32;
    key[23] = material.illumination_model as u32;
    key[24] = material.channel_flags as u32;
    key[25] = material.mode as u32;
    key[26] = material.indirect_bump_scale.to_bits();
    key[27] = material.reflectance_scale.to_bits();
    key[28] = material.transmittance_scale.to_bits();
    key[29] = material.luminance_mode as u32;
    key[30] = material.normal_map_method as u32;
    key[31] = material.normal_map_strength.to_bits();
    key[32] = material.is_anonymous as u32;
    key[33] = material.global_illumination as u32;
    key[34] = material.final_gather as u32;
    key[35] = material.color_bleed_scale.to_bits();
    key[36] = material.advanced_data_present as u32;
    MaterialBatchKey(key)
}

fn morton_axis(mut value: u32) -> u64 {
    value &= 0x3ff;
    let mut result = 0u64;
    for bit in 0..10 {
        result |= ((value >> bit) as u64 & 1) << (bit * 3);
    }
    result
}

fn mesh_spatial_key(set: &MeshLodSet, bounds: [f32; 6]) -> u64 {
    let center = [
        (set.world_aabb[0] + set.world_aabb[2]) * 0.5,
        (set.world_aabb[1] + set.world_aabb[3]) * 0.5,
        (set.z_aabb[0] + set.z_aabb[1]) * 0.5,
    ];
    let quantize = |value: f32, min: f32, max: f32| {
        let span = max - min;
        if !value.is_finite() || !span.is_finite() || span <= f32::EPSILON {
            0
        } else {
            (((value - min) / span).clamp(0.0, 1.0) * 1023.0).round() as u32
        }
    };
    let x = quantize(center[0], bounds[0], bounds[3]);
    let y = quantize(center[1], bounds[1], bounds[4]);
    let z = quantize(center[2], bounds[2], bounds[5]);
    morton_axis(x) | (morton_axis(y) << 1) | (morton_axis(z) << 2)
}

struct MeshBatchPart<'a> {
    set: &'a MeshLodSet,
    mesh: &'a MeshModel,
    uv_mesh: &'a MeshModel,
    entity_handle: Option<acadrust::Handle>,
    display_color: [f32; 4],
    material: Option<&'a crate::scene::model::material_model::MeshMaterial>,
    color: [f32; 4],
    indices: std::sync::Arc<[u32]>,
    index_hash: u64,
    include_faces: bool,
    include_edges: bool,
    part_slot: u32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct FacePartitionKey {
    instanced: bool,
    source: u64,
    base_material: MaterialBatchKey,
    face_materials: Vec<(u64, MaterialBatchKey)>,
    visual_style: [u64; 12],
    display_color: [u32; 4],
    include_faces: bool,
    include_edges: bool,
}

#[derive(Clone)]
struct CachedFacePart<'a> {
    material: Option<&'a crate::scene::model::material_model::MeshMaterial>,
    color: [f32; 4],
    indices: std::sync::Arc<[u32]>,
    index_hash: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct InstanceGroupKey {
    material: MaterialBatchKey,
    source: u64,
    part_slot: u32,
    index_count: usize,
    index_hash: u64,
    include_faces: bool,
    include_edges: bool,
}

fn index_hash(indices: &[u32]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    indices.hash(&mut hasher);
    hasher.finish()
}

fn optimize_triangle_indices(indices: &[u32], vertex_count: usize) -> std::sync::Arc<[u32]> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        return meshopt::optimize::optimize_vertex_cache(indices, vertex_count).into();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = vertex_count;
        indices.into()
    }
}

fn visual_style_partition_key(
    style: Option<&crate::scene::model::visual_style_model::MeshVisualStyle>,
) -> [u64; 12] {
    let mut key = [0; 12];
    let Some(style) = style else {
        return key;
    };
    key[0] = style.face_lighting_model as u64;
    key[1] = style.face_color_mode as u64;
    key[2] = style.face_modifier as u64;
    key[3] = style.face_opacity.to_bits() as u64;
    if let Some(color) = style.mono_color {
        key[4] = 1;
        for (slot, value) in key[5..9].iter_mut().zip(color) {
            *slot = value.to_bits() as u64;
        }
    }
    key
}

#[derive(Default)]
struct InstancedBuildProfile {
    vertices: std::time::Duration,
    edges: std::time::Duration,
    instances: std::time::Duration,
    upload: std::time::Duration,
    vertex_count: usize,
    edge_count: usize,
    index_count: usize,
    chunk_count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct InstancedVertexKey {
    source: u64,
    material: u64,
    compact: bool,
}

#[derive(Default)]
struct InstancedBufferCache {
    vertices: rustc_hash::FxHashMap<InstancedVertexKey, wgpu::Buffer>,
    edges: rustc_hash::FxHashMap<(u64, [u32; 4]), wgpu::Buffer>,
}

fn material_has_textures(
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
) -> bool {
    material.is_some_and(|material| {
        [
            &material.diffuse_map,
            &material.specular_map,
            &material.reflection_map,
            &material.opacity_map,
            &material.bump_map,
            &material.refraction_map,
            &material.normal_map,
        ]
        .into_iter()
        .any(|map| map.image.is_some())
    })
}

fn build_instanced_chunks(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    stubs: &MeshBatchStubs,
    buffers: &mut InstancedBufferCache,
    parts: &[MeshBatchPart<'_>],
    profile_enabled: bool,
) -> Option<(Vec<MeshBatchChunk>, u64, InstancedBuildProfile)> {
    let started = profile_enabled.then(iced::time::Instant::now);
    let first = parts.first()?;
    let source = first.set.instance_source.as_ref()?;
    let mesh = first.mesh;
    let material = first.material;
    let color = first.color;
    let source_handle = source.handle.value();
    let compact_vertices = !material_has_textures(material);
    let material_identity = if material_has_textures(material) {
        material.map_or(0, |material| {
            material
                .handle
                .map_or(material as *const _ as usize as u64, |handle| handle.value())
        })
    } else {
        0
    };
    let vertex_key = InstancedVertexKey {
        source: source_handle,
        material: material_identity,
        compact: compact_vertices,
    };
    let budget = super::gpu_budget::buffer_budget(device);
    let needs_wire_vertices = first.include_edges && source.edge_verts.is_empty();
    let split_geometry = mesh
        .verts
        .len()
        .saturating_mul(std::mem::size_of::<MeshVertex>())
        > budget
        || first
            .indices
            .len()
            .saturating_mul(std::mem::size_of::<u32>())
            > budget
        || (needs_wire_vertices
            && first
                .indices
                .len()
                .saturating_mul(2 * std::mem::size_of::<MeshEdgeVertex>())
                > budget)
        || (first.include_edges
            && source
                .edge_verts
                .len()
                .saturating_mul(std::mem::size_of::<MeshEdgeVertex>())
                > budget);
    let shared_vertex_buffer = (!split_geometry)
        .then(|| buffers.vertices.get(&vertex_key).cloned())
        .flatten();

    let has_normals = mesh.normals.len() == mesh.verts.len();
    let bounds = mesh_bounds(mesh);
    let vertex = |index: usize| {
        let normal = if has_normals {
            mesh.normals[index]
        } else {
            [0.0, 1.0, 0.0]
        };
        let position = mesh.verts[index];
        let position_low = mesh.verts_low.get(index).copied().unwrap_or([0.0; 3]);
        let uvs = material_uvs(
            material,
            position,
            normal,
            bounds,
            position,
            normal,
            bounds,
        );
        MeshVertex {
            position,
            normal,
            position_low,
            uv_diffuse: uvs[0],
            uv_specular: uvs[1],
            uv_reflection: uvs[2],
            uv_opacity: uvs[3],
            uv_bump: uvs[4],
            uv_refraction: uvs[5],
            uv_normal: uvs[6],
        }
    };
    let verts: Vec<_> =
        if !split_geometry && (shared_vertex_buffer.is_none() || needs_wire_vertices) {
            (0..mesh.verts.len()).map(vertex).collect()
        } else {
            Vec::new()
        };
    let vertices_at = profile_enabled.then(iced::time::Instant::now);
    let edge_color = first
        .set
        .visual_style
        .as_ref()
        .map_or(first.display_color, |style| {
            style.edge_color(first.display_color)
        });
    let edge_key = (source_handle, edge_color.map(f32::to_bits));
    let shared_edge_buffer = (first.include_edges && !split_geometry)
        .then(|| buffers.edges.get(&edge_key).cloned())
        .flatten();
    let edge_verts: Vec<_> = if first.include_edges && shared_edge_buffer.is_none() {
        (0..source.edge_verts.len())
            .map(|index| MeshEdgeVertex {
                position: source.edge_verts[index],
                color: edge_color,
                position_low: source
                    .edge_verts_low
                    .get(index)
                    .copied()
                    .unwrap_or([0.0; 3]),
            })
            .collect()
    } else {
        Vec::new()
    };
    let has_feature_edges = first.include_edges && !source.edge_verts.is_empty();
    let mut wire_indices = Vec::new();
    if first.include_edges && !has_feature_edges && !split_geometry {
        wire_indices.reserve(first.indices.len() * 2);
        for triangle in first.indices.chunks_exact(3) {
            wire_indices.extend_from_slice(&[
                triangle[0],
                triangle[1],
                triangle[1],
                triangle[2],
                triangle[2],
                triangle[0],
            ]);
        }
    }
    let edges_at = profile_enabled.then(iced::time::Instant::now);
    let transparent = material_is_transparent(material, color);
    let face_indices = first.include_faces.then_some(first.indices.as_ref());
    let opaque_indices = if transparent {
        &[][..]
    } else {
        face_indices.unwrap_or(&[])
    };
    let transparent_indices = if transparent {
        face_indices.unwrap_or(&[])
    } else {
        &[][..]
    };
    let mut instances = Vec::with_capacity(parts.len());
    let mut highlights = Vec::with_capacity(parts.len());
    let mut handles = rustc_hash::FxHashSet::default();
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for part in parts {
        let transform = part.set.instance_transform?;
        let instance_start = instances.len() as u32;
        instances.push(MeshInstanceGpu::from_transform(transform));
        bounds[0] = bounds[0].min(part.set.world_aabb[0]);
        bounds[1] = bounds[1].min(part.set.world_aabb[1]);
        bounds[2] = bounds[2].min(part.set.z_aabb[0]);
        bounds[3] = bounds[3].max(part.set.world_aabb[2]);
        bounds[4] = bounds[4].max(part.set.world_aabb[3]);
        bounds[5] = bounds[5].max(part.set.z_aabb[1]);
        if let Some(handle) = part.entity_handle {
            handles.insert(handle);
            if first.include_faces {
                highlights.push(MeshBatchRange {
                    handle,
                    index_start: 0,
                    index_count: first.indices.len() as u32,
                    transparent,
                    instance_start,
                    instance_count: 1,
                });
            }
        }
    }
    let instances_at = profile_enabled.then(iced::time::Instant::now);
    let triangles = if first.include_faces {
        (first.indices.len() / 3) as u64 * instances.len() as u64
    } else {
        0
    };
    // Upload each source chunk once, then share it across all instance chunks.
    let mut geometry = Vec::new();
    if split_geometry {
        let max_triangles = (super::gpu_budget::max_elements::<MeshVertex>(device) / 3)
            .min(super::gpu_budget::max_elements::<u32>(device) / 3)
            .min(super::gpu_budget::max_elements::<MeshEdgeVertex>(device) / 6)
            .max(1);
        if first.include_faces || needs_wire_vertices {
            for triangles in first.indices.chunks(max_triangles * 3) {
                let vertices: Vec<_> = triangles
                    .iter()
                    .map(|index| vertex(*index as usize))
                    .collect();
                let indices: Vec<_> = (0..vertices.len() as u32).collect();
                let wire_indices: Vec<_> = if needs_wire_vertices {
                    indices
                        .chunks_exact(3)
                        .flat_map(|t| [t[0], t[1], t[1], t[2], t[2], t[0]])
                        .collect()
                } else {
                    Vec::new()
                };
                geometry.push(make_chunk(
                    device,
                    queue,
                    stubs,
                    &vertices,
                    if first.include_faces && !transparent {
                        &indices
                    } else {
                        &[]
                    },
                    if first.include_faces && transparent {
                        &indices
                    } else {
                        &[]
                    },
                    &wire_indices,
                    &[],
                    &[],
                    &[],
                    &handles,
                    Some(bounds),
                    material,
                    color,
                    None,
                    None,
                ));
            }
        }
        for edges in edge_verts.chunks(super::gpu_budget::max_elements_grouped::<MeshEdgeVertex>(
            device, 2,
        )) {
            geometry.push(make_chunk(
                device,
                queue,
                stubs,
                &[],
                &[],
                &[],
                &[],
                edges,
                &[],
                &[],
                &handles,
                Some(bounds),
                material,
                color,
                None,
                None,
            ));
        }
    } else {
        let mut chunk = make_chunk(
            device,
            queue,
            stubs,
            &verts,
            opaque_indices,
            transparent_indices,
            &wire_indices,
            &edge_verts,
            &[],
            &[],
            &handles,
            Some(bounds),
            material,
            color,
            shared_vertex_buffer,
            shared_edge_buffer,
        );
        if first.include_edges {
            // Cached buffers still carry the complete source edge count.
            chunk.edge_vertex_count = source.edge_verts.len() as u32;
            buffers
                .edges
                .entry(edge_key)
                .or_insert_with(|| chunk.edge_vertex_buffer.clone());
        }
        buffers
            .vertices
            .entry(vertex_key)
            .or_insert_with(|| chunk.vertex_buffer.clone());
        geometry.push(chunk);
    }
    let mut chunks = Vec::new();
    let max_instances = super::gpu_budget::max_elements::<MeshInstanceGpu>(device);
    for (group, instance_data) in instances.chunks(max_instances).enumerate() {
        let start = group * max_instances;
        let end = start + instance_data.len();
        let instance_buffer = super::gpu_upload::upload_buffer(
            device,
            queue,
            "mesh.batch.instances",
            instance_data,
            wgpu::BufferUsages::VERTEX,
        );
        let group_handles: rustc_hash::FxHashSet<_> = parts[start..end]
            .iter()
            .filter_map(|part| part.entity_handle)
            .collect();
        for source_chunk in &geometry {
            let mut chunk = source_chunk.clone();
            chunk.instance_buffer = instance_buffer.clone();
            chunk.instance_count = instance_data.len() as u32;
            chunk.handles = group_handles.clone();
            let index_count = chunk.index_count + chunk.transp_index_count;
            chunk.highlight_ranges = highlights
                .iter()
                .filter(|range| {
                    (start..end).contains(&(range.instance_start as usize)) && index_count > 0
                })
                .map(|range| MeshBatchRange {
                    instance_start: range.instance_start - start as u32,
                    index_count,
                    ..*range
                })
                .collect();
            chunks.push(chunk);
        }
    }
    let uploaded_at = profile_enabled.then(iced::time::Instant::now);
    let chunk_count = chunks.len();
    Some((
        chunks,
        triangles,
        InstancedBuildProfile {
            vertices: started
                .zip(vertices_at)
                .map(|(start, end)| end.duration_since(start))
                .unwrap_or_default(),
            edges: vertices_at
                .zip(edges_at)
                .map(|(start, end)| end.duration_since(start))
                .unwrap_or_default(),
            instances: edges_at
                .zip(instances_at)
                .map(|(start, end)| end.duration_since(start))
                .unwrap_or_default(),
            upload: instances_at
                .zip(uploaded_at)
                .map(|(start, end)| end.duration_since(start))
                .unwrap_or_default(),
            vertex_count: if split_geometry && (first.include_faces || needs_wire_vertices) {
                first.indices.len()
            } else {
                verts.len()
            },
            edge_count: edge_verts.len(),
            index_count: first.indices.len(),
            chunk_count,
        },
    ))
}

fn mesh_bounds(mesh: &MeshModel) -> [f32; 6] {
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for (index, high) in mesh.verts.iter().copied().enumerate() {
        let low = mesh.verts_low.get(index).copied().unwrap_or([0.0; 3]);
        for axis in 0..3 {
            let value = high[axis] + low[axis];
            bounds[axis] = bounds[axis].min(value);
            bounds[axis + 3] = bounds[axis + 3].max(value);
        }
    }
    bounds
}

fn material_is_transparent(
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
    color: [f32; 4],
) -> bool {
    color[3] < 0.999
        || material.is_some_and(|material| {
            material.translucence > 0.0
                || (material.channel_flags as u32 & 0x08 != 0
                    && material.opacity_map.image.is_some())
        })
}

fn material_map_uv(
    map: &crate::scene::model::material_model::MeshTextureMap,
    local_position: [f32; 3],
    local_normal: [f32; 3],
    local_bounds: [f32; 6],
    model_position: [f32; 3],
    model_normal: [f32; 3],
    model_bounds: [f32; 6],
) -> [f32; 2] {
    let (mut position, normal, bounds) = if map.auto_transform & 4 != 0 {
        (model_position, model_normal, model_bounds)
    } else {
        (local_position, local_normal, local_bounds)
    };
    if map.auto_transform & 2 != 0 {
        for axis in 0..3 {
            let extent = bounds[axis + 3] - bounds[axis];
            position[axis] = if extent.is_finite() && extent.abs() > f32::EPSILON {
                (position[axis] - bounds[axis]) / extent
            } else {
                0.0
            };
        }
    }
    let m = &map.transform;
    let p = [
        position[0] * m[0] + position[1] * m[1] + position[2] * m[2] + m[3],
        position[0] * m[4] + position[1] * m[5] + position[2] * m[6] + m[7],
        position[0] * m[8] + position[1] * m[9] + position[2] * m[10] + m[11],
    ];
    match map.projection {
        3 => [
            p[1].atan2(p[0]) / std::f32::consts::TAU + 0.5,
            p[2],
        ],
        4 => {
            let radius = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            if radius <= f32::EPSILON {
                [0.0; 2]
            } else {
                [
                    p[1].atan2(p[0]) / std::f32::consts::TAU + 0.5,
                    (p[2] / radius).clamp(-1.0, 1.0).acos() / std::f32::consts::PI,
                ]
            }
        }
        2 => {
            let n = [normal[0].abs(), normal[1].abs(), normal[2].abs()];
            if n[0] >= n[1] && n[0] >= n[2] {
                [p[1], p[2]]
            } else if n[1] >= n[2] {
                [p[0], p[2]]
            } else {
                [p[0], p[1]]
            }
        }
        _ => [p[0], p[1]],
    }
}

fn material_uvs(
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
    local_position: [f32; 3],
    local_normal: [f32; 3],
    local_bounds: [f32; 6],
    model_position: [f32; 3],
    model_normal: [f32; 3],
    model_bounds: [f32; 6],
) -> [[f32; 2]; 7] {
    let Some(material) = material else {
        return [[0.0; 2]; 7];
    };
    let uv = |map: &crate::scene::model::material_model::MeshTextureMap| {
        if map.image.is_none() {
            [0.0; 2]
        } else {
            material_map_uv(
                map,
                local_position,
                local_normal,
                local_bounds,
                model_position,
                model_normal,
                model_bounds,
            )
        }
    };
    [
        uv(&material.diffuse_map),
        uv(&material.specular_map),
        uv(&material.reflection_map),
        uv(&material.opacity_map),
        uv(&material.bump_map),
        uv(&material.refraction_map),
        uv(&material.normal_map),
    ]
}

fn material_vertex_params(
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
) -> ([f32; 4], [f32; 4], [f32; 4], [f32; 4], [u32; 4]) {
    let Some(material) = material else {
        return (
            [0.5, 0.0, 0.0, 0.0],
            [0.08, 0.08, 0.08, 1.0],
            [0.3, 0.3, 0.3, 0.0],
            [1.0; 4],
            [0, 127, 0, 0],
        );
    };
    (
        [
            material.gloss,
            material.reflectivity,
            material.self_illumination,
            material.luminance,
        ],
        [
            material.specular[0],
            material.specular[1],
            material.specular[2],
            material.refraction_index,
        ],
        [
            material.ambient[0],
            material.ambient[1],
            material.ambient[2],
            material.translucence,
        ],
        [
            material.normal_map_strength,
            material.indirect_bump_scale,
            material.reflectance_scale,
            material.transmittance_scale,
        ],
        [
            material.illumination_model as u32,
            material.channel_flags as u32,
            material.mode as u32,
            material.luminance_mode as u32,
        ],
    )
}

/// Concatenate every set's first non-empty LOD into a few large GPU buffers.
/// Returns the chunks plus the total triangle count drawn (for diagnostics).
///
/// Every emitted buffer stays under the device's `max_buffer_size` (default
/// 256 MB). Both the vertex buffer (`size_of::<MeshVertex>()` B/vert) and the
/// wire-index buffer (6 u32 = 24 B/triangle — the fattest index buffer) are
/// bounded; a single mesh too large for one chunk is split into triangle-soup
/// sub-chunks so an XREF-heavy model can never overflow a single buffer (#203).
pub fn build_mesh_batch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sets: &[MeshLodSet],
) -> (Vec<MeshBatchChunk>, u64) {
    build_mesh_batch_filtered(device, queue, sets, None)
}

pub fn build_mesh_batch_filtered(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sets: &[MeshLodSet],
    handles: Option<&rustc_hash::FxHashSet<acadrust::Handle>>,
) -> (Vec<MeshBatchChunk>, u64) {
    let perf_started = crate::perf::enabled().then(iced::time::Instant::now);
    // Derive the caps from the real device limit and vertex size. The previous
    // fixed 6 M-vertex cap assumed 40 B/vertex, but `position_low` (RTE) grew
    // MeshVertex to 52 B, so 6 M × 52 B = 312 MB blew past the 256 MB cap.
    let budget = super::gpu_budget::buffer_budget(device);
    let max_verts = super::gpu_budget::max_elements::<MeshVertex>(device).max(3);
    // A triangle contributes three line segments (six standalone edge
    // vertices). Bound chunks by the largest buffer produced for wire meshes.
    let max_tris =
        (budget / (6 * std::mem::size_of::<MeshEdgeVertex>())).max(1);
    let stubs = MeshBatchStubs::new(device, queue);

    let mut chunks = Vec::new();
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut transp_indices: Vec<u32> = Vec::new();
    let mut wire_indices: Vec<u32> = Vec::new();
    let mut edge_verts: Vec<MeshEdgeVertex> = Vec::new();
    let mut highlight_ranges: Vec<MeshBatchRange> = Vec::new();
    let mut chunk_handles: rustc_hash::FxHashSet<acadrust::Handle> =
        rustc_hash::FxHashSet::default();
    let mut total_tris = 0u64;
    let mut ordered: Vec<MeshBatchPart<'_>> = Vec::new();
    let mut source_indices: rustc_hash::FxHashMap<
        (bool, u64),
        (std::sync::Arc<[u32]>, u64),
    > = rustc_hash::FxHashMap::default();
    let mut face_partitions: rustc_hash::FxHashMap<FacePartitionKey, Vec<CachedFacePart<'_>>> =
        rustc_hash::FxHashMap::default();
    for set in sets {
        let Some(mesh) = set
            .geometry_lods()
            .iter()
            .find(|mesh| !mesh.indices.is_empty())
        else {
            continue;
        };
        let display_handle = set.entity_handle();
        if handles.is_some_and(|wanted| {
            display_handle.is_none_or(|handle| !wanted.contains(&handle))
        }) {
            continue;
        }
        let display_color = set.display_color().unwrap_or(mesh.color);
        let source_identity = set.instance_source.as_ref().map_or_else(
            || (false, mesh as *const MeshModel as usize as u64),
            |source| (true, source.handle.value()),
        );
        let triangle_count = mesh.indices.len() / 3;
        let has_face_materials =
            mesh.triangle_material_handles.len() == triangle_count
            && !set.face_materials.is_empty();
        let has_face_colors = mesh.triangle_colors.len() == triangle_count
            && mesh.triangle_colors.iter().any(Option::is_some);
        let include_faces = set
            .visual_style
            .as_ref()
            .map_or(true, |style| style.face_visible());
        let include_edges = set
            .visual_style
            .as_ref()
            .map_or(true, |style| style.edges_visible());
        if !has_face_materials && !has_face_colors {
            let base_color =
                set.material.as_ref().map_or(display_color, |material| material.diffuse);
            let color = set
                .visual_style
                .as_ref()
                .map_or(base_color, |style| style.face_color(base_color));
            let (shared_indices, shared_hash) = source_indices
                .entry(source_identity)
                .or_insert_with(|| {
                    let indices = optimize_triangle_indices(&mesh.indices, mesh.verts.len());
                    let hash = index_hash(indices.as_ref());
                    (indices, hash)
                })
                .clone();
            ordered.push(MeshBatchPart {
                set,
                mesh,
                uv_mesh: mesh,
                entity_handle: display_handle,
                display_color,
                material: set.material.as_ref(),
                color,
                indices: shared_indices,
                index_hash: shared_hash,
                include_faces,
                include_edges,
                part_slot: 0,
            });
            continue;
        }
        let mut face_materials: Vec<_> = set
            .face_materials
            .iter()
            .map(|(handle, material)| {
                (
                    handle.value(),
                    material_key(Some(material), material.diffuse),
                )
            })
            .collect();
        face_materials.sort_by_key(|(handle, _)| *handle);
        let partition_key = FacePartitionKey {
            instanced: source_identity.0,
            source: source_identity.1,
            base_material: material_key(set.material.as_ref(), display_color),
            face_materials,
            visual_style: visual_style_partition_key(set.visual_style.as_ref()),
            display_color: display_color.map(f32::to_bits),
            include_faces,
            include_edges,
        };
        let cached_parts = if let Some(parts) = face_partitions.get(&partition_key) {
            parts.clone()
        } else {
            let mut groups: std::collections::BTreeMap<
                MaterialBatchKey,
                (
                    Option<&crate::scene::model::material_model::MeshMaterial>,
                    [f32; 4],
                    Vec<u32>,
                ),
            > = std::collections::BTreeMap::new();
            for (triangle, indices) in mesh.indices.chunks_exact(3).enumerate() {
                let material = if has_face_materials {
                    mesh.triangle_material_handles[triangle]
                        .and_then(|handle| set.face_materials.get(&handle))
                        .or(set.material.as_ref())
                } else {
                    set.material.as_ref()
                };
                let base_color =
                    material.map_or(display_color, |material| material.diffuse);
                let base_color = if has_face_colors {
                    mesh.triangle_colors[triangle].unwrap_or(base_color)
                } else {
                    base_color
                };
                let color = set
                    .visual_style
                    .as_ref()
                    .map_or(base_color, |style| style.face_color(base_color));
                groups
                    .entry(material_key(material, color))
                    .or_insert_with(|| (material, color, Vec::new()))
                    .2
                    .extend_from_slice(indices);
            }
            let parts: Vec<_> = groups
                .into_values()
                .map(|(material, color, indices)| {
                    let indices = optimize_triangle_indices(&indices, mesh.verts.len());
                    let index_hash = index_hash(indices.as_ref());
                    CachedFacePart {
                        material,
                        color,
                        indices,
                        index_hash,
                    }
                })
                .collect();
            face_partitions.insert(partition_key, parts.clone());
            parts
        };
        for (part_index, part) in cached_parts.into_iter().enumerate() {
            ordered.push(MeshBatchPart {
                set,
                mesh,
                uv_mesh: mesh,
                entity_handle: display_handle,
                display_color,
                material: part.material,
                color: part.color,
                indices: part.indices,
                index_hash: part.index_hash,
                include_faces,
                include_edges: include_edges && part_index == 0,
                part_slot: part_index as u32,
            });
        }
    }
    let prepared_at = iced::time::Instant::now();
    let spatial_bounds = sets.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |mut bounds, set| {
            bounds[0] = bounds[0].min(set.world_aabb[0]);
            bounds[1] = bounds[1].min(set.world_aabb[1]);
            bounds[2] = bounds[2].min(set.z_aabb[0]);
            bounds[3] = bounds[3].max(set.world_aabb[2]);
            bounds[4] = bounds[4].max(set.world_aabb[3]);
            bounds[5] = bounds[5].max(set.z_aabb[1]);
            bounds
        },
    );
    ordered.sort_by_key(|part| {
        (
            material_key(part.material, part.color),
            mesh_spatial_key(part.set, spatial_bounds),
        )
    });
    let mut instance_groups: std::collections::BTreeMap<
        InstanceGroupKey,
        Vec<MeshBatchPart<'_>>,
    > = std::collections::BTreeMap::new();
    let mut direct_parts = Vec::with_capacity(ordered.len());
    for part in ordered {
        let eligible = part.set.instance_transform.is_some()
            && part.set.instance_source.is_some()
            && part
                .indices
                .iter()
                .all(|index| (*index as usize) < part.mesh.verts.len());
        if eligible {
            let source = part
                .set
                .instance_source
                .as_ref()
                .expect("checked above")
                .handle
                .value();
            instance_groups
                .entry(InstanceGroupKey {
                    material: material_key(part.material, part.color),
                    source,
                    part_slot: part.part_slot,
                    index_count: part.indices.len(),
                    index_hash: part.index_hash,
                    include_faces: part.include_faces,
                    include_edges: part.include_edges,
                })
                .or_default()
                .push(part);
        } else {
            direct_parts.push(part);
        }
    }
    direct_parts.sort_by_key(|part| {
        (
            material_key(part.material, part.color),
            mesh_spatial_key(part.set, spatial_bounds),
        )
    });
    let grouped_at = iced::time::Instant::now();
    let ordered = direct_parts;
    let mut active_key: Option<MaterialBatchKey> = None;
    let mut active_material: Option<&crate::scene::model::material_model::MeshMaterial> = None;
    let mut active_color = [1.0; 4];
    for part in ordered {
        let set = part.set;
        let mesh = part.mesh;
        let material = part.material;
        let part_color = part.color;
        let entity_handle = part.entity_handle;
        let key = material_key(material, part_color);
        if active_key.is_some_and(|active| active != key)
            && (!verts.is_empty() || !edge_verts.is_empty())
        {
            chunks.push(make_chunk(
                device,
                queue,
                &stubs,
                &verts,
                &indices,
                &transp_indices,
                &wire_indices,
                &edge_verts,
                &highlight_ranges,
                &[],
                &chunk_handles,
                None,
                active_material,
                active_color,
                None,
                None,
            ));
            verts.clear();
            indices.clear();
            transp_indices.clear();
            wire_indices.clear();
            edge_verts.clear();
            highlight_ranges.clear();
            chunk_handles.clear();
        }
        active_key = Some(key);
        active_material = material;
        active_color = part_color;
        if let Some(handle) = entity_handle {
            chunk_handles.insert(handle);
        }
        let has_normals = mesh.normals.len() == mesh.verts.len();
        let uv_mesh = part.uv_mesh;
        let local_bounds = mesh_bounds(uv_mesh);
        let model_bounds = mesh_bounds(mesh);
        let edge_color = set
            .visual_style
            .as_ref()
            .map_or(part.display_color, |style| style.edge_color(part.display_color));
        let vtx = |vi: usize| {
            let normal = if has_normals {
                mesh.normals[vi]
            } else {
                [0.0, 1.0, 0.0]
            };
            let local_position = uv_mesh
                .verts
                .get(vi)
                .copied()
                .unwrap_or(mesh.verts[vi]);
            let local_normal = uv_mesh
                .normals
                .get(vi)
                .copied()
                .unwrap_or(normal);
            let uv = material_uvs(
                material,
                local_position,
                local_normal,
                local_bounds,
                mesh.verts[vi],
                normal,
                model_bounds,
            );
            MeshVertex {
                position: mesh.verts[vi],
                normal,
                position_low: mesh.verts_low.get(vi).copied().unwrap_or([0.0; 3]),
                uv_diffuse: uv[0],
                uv_specular: uv[1],
                uv_reflection: uv[2],
                uv_opacity: uv[3],
                uv_bump: uv[4],
                uv_refraction: uv[5],
                uv_normal: uv[6],
            }
        };
        // A solid whose baked colour is not fully opaque routes into the
        // transparent index stream so it is drawn last, without depth writes.
        let is_transp = material_is_transparent(material, part_color);
        let mesh_tris = part.indices.len() / 3;
        if part.include_faces {
            total_tris += mesh_tris as u64;
        }

        // Feature edges present (ACIS solid) → emit the B-rep edges as a line
        // list and skip the triangulation wireframe. Absent (plain mesh) → keep
        // the triangle edges so the mesh still shows a wireframe.
        let has_feat = !set.edge_verts.is_empty();
        if has_feat && part.include_edges {
            // Feature edges use their own vertex buffer, so they need their
            // own cap. Large ACIS models can have relatively few faces but
            // millions of B-rep edge vertices; only checking `mesh.verts`
            // allowed `edge_vbuf` to exceed wgpu's max_buffer_size.
            let mut edge_start = 0;
            let edge_end = set.edge_verts.len() & !1usize;
            while edge_start < edge_end {
                let available = max_verts.saturating_sub(edge_verts.len());
                // LineList consumes pairs. Never split a segment between
                // chunks even when the vertex budget is odd.
                let take = available
                    .min(edge_end - edge_start)
                    & !1usize;
                if take == 0 {
                    chunks.push(make_chunk(
                        device,
                        queue,
                        &stubs,
                        &verts,
                        &indices,
                        &transp_indices,
                        &wire_indices,
                        &edge_verts,
                        &highlight_ranges,
                        &[],
                        &chunk_handles,
                        None,
                        active_material,
                        active_color,
                        None,
                        None,
                    ));
                    verts.clear();
                    indices.clear();
                    transp_indices.clear();
                    wire_indices.clear();
                    edge_verts.clear();
                    highlight_ranges.clear();
                    chunk_handles.clear();
                    if let Some(handle) = entity_handle {
                        chunk_handles.insert(handle);
                    }
                    continue;
                }
                for i in edge_start..edge_start + take {
                    edge_verts.push(MeshEdgeVertex {
                        position: set.edge_verts[i],
                        color: edge_color,
                        position_low: set.edge_verts_low.get(i).copied().unwrap_or([0.0; 3]),
                    });
                }
                edge_start += take;
                if edge_start < edge_end {
                    chunks.push(make_chunk(
                        device,
                        queue,
                        &stubs,
                        &verts,
                        &indices,
                        &transp_indices,
                        &wire_indices,
                        &edge_verts,
                        &highlight_ranges,
                        &[],
                        &chunk_handles,
                        None,
                        active_material,
                        active_color,
                        None,
                        None,
                    ));
                    verts.clear();
                    indices.clear();
                    transp_indices.clear();
                    wire_indices.clear();
                    edge_verts.clear();
                    highlight_ranges.clear();
                    chunk_handles.clear();
                    if let Some(handle) = entity_handle {
                        chunk_handles.insert(handle);
                    }
                }
            }
        }

        // A single mesh larger than a whole chunk: emit as triangle-soup
        // sub-chunks (corners expanded, no vertex sharing) so each buffer fits.
        if mesh.verts.len() > max_verts || mesh_tris > max_tris {
            if !verts.is_empty() || !edge_verts.is_empty() {
                chunks.push(make_chunk(
                    device,
                    queue,
                    &stubs,
                    &verts,
                    &indices,
                    &transp_indices,
                    &wire_indices,
                    &edge_verts,
                    &highlight_ranges,
                    &[],
                    &chunk_handles,
                    None,
                    active_material,
                    active_color,
                    None,
                    None,
                ));
                verts.clear();
                indices.clear();
                transp_indices.clear();
                wire_indices.clear();
                edge_verts.clear();
                highlight_ranges.clear();
                chunk_handles.clear();
                if let Some(handle) = entity_handle {
                    chunk_handles.insert(handle);
                }
            }
            let tris_per = (max_verts / 3).min(max_tris).max(1);
            let mut t = 0;
            while t < mesh_tris {
                let end = (t + tris_per).min(mesh_tris);
                let (mut sv, mut si, mut swi) = (Vec::new(), Vec::new(), Vec::new());
                for tri in t..end {
                    let ix = &part.indices[tri * 3..tri * 3 + 3];
                    let b = sv.len() as u32;
                    sv.push(vtx(ix[0] as usize));
                    sv.push(vtx(ix[1] as usize));
                    sv.push(vtx(ix[2] as usize));
                    if part.include_faces {
                        si.extend_from_slice(&[b, b + 1, b + 2]);
                    }
                    if part.include_edges && !has_feat {
                        swi.extend_from_slice(&[b, b + 1, b + 1, b + 2, b + 2, b]);
                    }
                }
                // The whole mesh shares one colour, so a sub-chunk is entirely
                // opaque or entirely transparent.
                let sub_ranges: Vec<MeshBatchRange> = if part.include_faces {
                    entity_handle
                        .map(|handle| {
                            vec![MeshBatchRange {
                                handle,
                                index_start: 0,
                                index_count: si.len() as u32,
                                transparent: is_transp,
                                instance_start: 0,
                                instance_count: 1,
                            }]
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                if is_transp {
                    let sub_handles: rustc_hash::FxHashSet<_> =
                        entity_handle.into_iter().collect();
                    chunks.push(make_chunk(
                        device,
                        queue,
                        &stubs,
                        &sv,
                        &[],
                        &si,
                        &swi,
                        &[],
                        &sub_ranges,
                        &[],
                        &sub_handles,
                        None,
                        active_material,
                        active_color,
                        None,
                        None,
                    ));
                } else {
                    let sub_handles: rustc_hash::FxHashSet<_> =
                        entity_handle.into_iter().collect();
                    chunks.push(make_chunk(
                        device,
                        queue,
                        &stubs,
                        &sv,
                        &si,
                        &[],
                        &swi,
                        &[],
                        &sub_ranges,
                        &[],
                        &sub_handles,
                        None,
                        active_material,
                        active_color,
                        None,
                        None,
                    ));
                }
                t = end;
            }
            chunk_handles.clear();
            continue;
        }

        // Flush when adding this mesh would overflow either the surface vertex
        // buffer or the expanded wire-vertex buffer.
        if !verts.is_empty()
            && (verts.len() + mesh.verts.len() > max_verts
                || wire_indices.len() / 6 + mesh_tris > max_tris)
        {
            chunks.push(make_chunk(
                device,
                queue,
                &stubs,
                &verts,
                &indices,
                &transp_indices,
                &wire_indices,
                &edge_verts,
                &highlight_ranges,
                &[],
                &chunk_handles,
                None,
                active_material,
                active_color,
                None,
                None,
            ));
            verts.clear();
            indices.clear();
            transp_indices.clear();
            wire_indices.clear();
            edge_verts.clear();
            highlight_ranges.clear();
            chunk_handles.clear();
            if let Some(handle) = entity_handle {
                chunk_handles.insert(handle);
            }
        }
        let base = verts.len() as u32;
        for i in 0..mesh.verts.len() {
            verts.push(vtx(i));
        }
        if part.include_faces {
            let fill = if is_transp { &mut transp_indices } else { &mut indices };
            let index_start = fill.len() as u32;
            for &idx in part.indices.iter() {
                fill.push(base + idx);
            }
            if let Some(handle) = entity_handle {
                highlight_ranges.push(MeshBatchRange {
                    handle,
                    index_start,
                    index_count: part.indices.len() as u32,
                    transparent: is_transp,
                    instance_start: 0,
                    instance_count: 1,
                });
            }
        }
        if part.include_edges && !has_feat {
            for tri in part.indices.chunks_exact(3) {
                let (a, b, c) = (base + tri[0], base + tri[1], base + tri[2]);
                wire_indices.extend_from_slice(&[a, b, b, c, c, a]);
            }
        }
    }
    if !indices.is_empty()
        || !transp_indices.is_empty()
        || !wire_indices.is_empty()
        || !edge_verts.is_empty()
    {
        chunks.push(make_chunk(
            device,
            queue,
            &stubs,
            &verts,
            &indices,
            &transp_indices,
            &wire_indices,
            &edge_verts,
            &highlight_ranges,
            &[],
            &chunk_handles,
            None,
            active_material,
            active_color,
            None,
            None,
        ));
    }
    let direct_at = iced::time::Instant::now();
    let mut instanced_profile = InstancedBuildProfile::default();
    let mut instanced_buffers = InstancedBufferCache::default();
    for parts in instance_groups.values() {
        if let Some((instanced, triangles, profile)) = build_instanced_chunks(
            device,
            queue,
            &stubs,
            &mut instanced_buffers,
            parts,
            perf_started.is_some(),
        ) {
            total_tris += triangles;
            chunks.extend(instanced);
            instanced_profile.vertices += profile.vertices;
            instanced_profile.edges += profile.edges;
            instanced_profile.instances += profile.instances;
            instanced_profile.upload += profile.upload;
            instanced_profile.vertex_count += profile.vertex_count;
            instanced_profile.edge_count += profile.edge_count;
            instanced_profile.index_count += profile.index_count;
            instanced_profile.chunk_count += profile.chunk_count;
        }
    }
    if let Some(started) = perf_started {
        let done = iced::time::Instant::now();
        crate::perf_record!(
            "[perf] mesh-build-detail prepare={:.1} group={:.1} direct={:.1} instanced={:.1}",
            prepared_at.duration_since(started).as_secs_f64() * 1000.0,
            grouped_at.duration_since(prepared_at).as_secs_f64() * 1000.0,
            direct_at.duration_since(grouped_at).as_secs_f64() * 1000.0,
            done.duration_since(direct_at).as_secs_f64() * 1000.0,
        );
        crate::perf_record!(
            "[perf] mesh-instanced vertices={:.1} edges={:.1} instances={:.1} upload={:.1} chunks={} verts={} edge_verts={} indices={}",
            instanced_profile.vertices.as_secs_f64() * 1000.0,
            instanced_profile.edges.as_secs_f64() * 1000.0,
            instanced_profile.instances.as_secs_f64() * 1000.0,
            instanced_profile.upload.as_secs_f64() * 1000.0,
            instanced_profile.chunk_count,
            instanced_profile.vertex_count,
            instanced_profile.edge_count,
            instanced_profile.index_count,
        );
    }
    (chunks, total_tris)
}
