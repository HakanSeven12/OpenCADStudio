// Mesh GPU buffers — TriangleList rendering for solid objects.
//
// Vertex layout (40 bytes):
//   position   [f32; 3]   offset  0   12 B
//   normal     [f32; 3]   offset 12   12 B
//   color      [f32; 4]   offset 24   16 B
//                                ------
//                                 40 B / vertex

use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;

// ── Vertex layout ─────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub position_low: [f32; 3],
    /// gloss, reflectivity, self-illumination, luminance
    pub material: [f32; 4],
    /// specular RGB and refraction index
    pub specular: [f32; 4],
    pub uv_diffuse: [f32; 2],
    /// ambient RGB and translucence
    pub ambient: [f32; 4],
    /// normal strength, bump scale, reflectance scale, transmittance scale
    pub advanced: [f32; 4],
    /// illumination model, channel flags, material mode, luminance mode
    pub flags: [u32; 4],
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
                offset: std::mem::offset_of!(MeshVertex, color) as u64,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, position_low) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, material) as u64,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, specular) as u64,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_diffuse) as u64,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, ambient) as u64,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, advanced) as u64,
                shader_location: 8,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, flags) as u64,
                shader_location: 9,
                format: wgpu::VertexFormat::Uint32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_specular) as u64,
                shader_location: 10,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_reflection) as u64,
                shader_location: 11,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_opacity) as u64,
                shader_location: 12,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_bump) as u64,
                shader_location: 13,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_refraction) as u64,
                shader_location: 14,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, uv_normal) as u64,
                shader_location: 15,
                format: wgpu::VertexFormat::Float32x2,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRS,
        }
    }

    /// Minimal layout shared by native and WebGL mesh edge pipelines.
    ///
    /// Edge fragments only need position and entity color. Advertising the
    /// material/normal/UV attributes here would keep the full surface-shader
    /// interface alive on WebGL even though the edge entry point never reads
    /// those values.
    pub fn edge_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, position) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, color) as u64,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(MeshVertex, position_low) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x3,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
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

pub struct MeshBatchChunk {
    pub vertex_buffer: wgpu::Buffer,
    /// Opaque triangle indices (mesh colour alpha ≈ 1). Drawn with depth write.
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    /// Transparent triangle indices (mesh colour alpha < 1). Drawn after the
    /// opaque fills with depth write disabled so they blend over — rather than
    /// erase — the geometry behind them.
    pub transp_index_buffer: wgpu::Buffer,
    pub transp_index_count: u32,
    /// Triangle-edge line list (into `vertex_buffer`) for plain meshes that
    /// carry no B-rep edges — the tessellation wireframe.
    pub wire_index_buffer: wgpu::Buffer,
    pub wire_index_count: u32,
    /// B-rep feature edges of ACIS solids, as a standalone LineList vertex
    /// buffer (pairs of endpoints), drawn non-indexed. Empty for plain meshes.
    pub edge_vertex_buffer: wgpu::Buffer,
    pub edge_vertex_count: u32,
    pub instance_buffer: wgpu::Buffer,
    pub instance_count: u32,
    pub highlight_ranges: Vec<MeshBatchRange>,
    pub handles: rustc_hash::FxHashSet<acadrust::Handle>,
    pub world_aabb: [f32; 6],
    pub visible: bool,
    pub material: Option<crate::scene::model::material_model::MeshMaterial>,
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

fn make_chunk(
    device: &wgpu::Device,
    verts: &[MeshVertex],
    indices: &[u32],
    transp_indices: &[u32],
    wire_indices: &[u32],
    edge_verts: &[MeshVertex],
    highlight_ranges: &[MeshBatchRange],
    instances: &[MeshInstanceGpu],
    handles: &rustc_hash::FxHashSet<acadrust::Handle>,
    bounds_override: Option<[f32; 6]>,
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
) -> MeshBatchChunk {
    // `create_buffer_init` with an empty slice yields a zero-sized buffer that
    // some backends reject for INDEX usage; a chunk can legitimately hold only
    // opaque or only transparent tris, so fall back to a 1-index stub (count
    // stays 0, so the draw loop skips it).
    let mk_index = |data: &[u32], label: &'static str| {
        let stub = [0u32];
        let src = if data.is_empty() { &stub[..] } else { data };
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(src),
            usage: wgpu::BufferUsages::INDEX,
        })
    };
    let mk_vertex = |data: &[MeshVertex], label: &'static str| {
        let stub = [MeshVertex {
            position: [0.0; 3],
            normal: [0.0, 1.0, 0.0],
            color: [0.0; 4],
            position_low: [0.0; 3],
            material: [0.0; 4],
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
        }];
        let src = if data.is_empty() { &stub[..] } else { data };
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(src),
            usage: wgpu::BufferUsages::VERTEX,
        })
    };
    let identity = [MeshInstanceGpu::identity()];
    let instance_data = if instances.is_empty() {
        &identity[..]
    } else {
        instances
    };
    let instance_usage =
        if device.limits().max_storage_buffers_per_shader_stage > 0 {
            wgpu::BufferUsages::STORAGE
        } else {
            wgpu::BufferUsages::UNIFORM
        };
    let mut computed_aabb = [
        f32::INFINITY,
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for vertex in verts.iter().chain(edge_verts) {
        let point = [
            vertex.position[0] + vertex.position_low[0],
            vertex.position[1] + vertex.position_low[1],
            vertex.position[2] + vertex.position_low[2],
        ];
        for axis in 0..3 {
            computed_aabb[axis] = computed_aabb[axis].min(point[axis]);
            computed_aabb[axis + 3] = computed_aabb[axis + 3].max(point[axis]);
        }
    }
    let world_aabb = bounds_override.unwrap_or(computed_aabb);
    MeshBatchChunk {
        vertex_buffer: mk_vertex(verts, "mesh.batch.vbuf"),
        index_buffer: mk_index(indices, "mesh.batch.ibuf"),
        index_count: indices.len() as u32,
        transp_index_buffer: mk_index(transp_indices, "mesh.batch.transp_ibuf"),
        transp_index_count: transp_indices.len() as u32,
        wire_index_buffer: mk_index(wire_indices, "mesh.batch.wire_ibuf"),
        wire_index_count: wire_indices.len() as u32,
        edge_vertex_buffer: mk_vertex(edge_verts, "mesh.batch.edge_vbuf"),
        edge_vertex_count: edge_verts.len() as u32,
        instance_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh.batch.instances"),
            contents: bytemuck::cast_slice(instance_data),
            usage: instance_usage,
        }),
        instance_count: instance_data.len() as u32,
        highlight_ranges: highlight_ranges.to_vec(),
        handles: handles.clone(),
        world_aabb,
        visible: true,
        material: material.cloned(),
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

pub fn create_material_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    material: Option<&crate::scene::model::material_model::MeshMaterial>,
    instance_buffer: Option<&wgpu::Buffer>,
) -> wgpu::BindGroup {
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
    let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mesh.material.params"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let fallback_instances;
    let instance_buffer = match instance_buffer {
        Some(buffer) => buffer,
        None => {
            let usage =
                if device.limits().max_storage_buffers_per_shader_stage > 0 {
                    wgpu::BufferUsages::STORAGE
                } else {
                    wgpu::BufferUsages::UNIFORM
                };
            fallback_instances =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh.instances.identity"),
                    contents: bytemuck::bytes_of(&MeshInstanceGpu::identity()),
                    usage,
                });
            &fallback_instances
        }
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh.material.bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&diffuse_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&specular_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&reflection_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&opacity_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&bump_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&refraction_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&normal_view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&diffuse_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::Sampler(&specular_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::Sampler(&reflection_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::Sampler(&opacity_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::Sampler(&bump_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: wgpu::BindingResource::Sampler(&refraction_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: wgpu::BindingResource::Sampler(&normal_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 14,
                resource: params_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 15,
                resource: instance_buffer.as_entire_binding(),
            },
        ],
    })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    display_mesh: &'a MeshModel,
    uv_mesh: &'a MeshModel,
    material: Option<&'a crate::scene::model::material_model::MeshMaterial>,
    color: [f32; 4],
    indices: Vec<u32>,
    include_faces: bool,
    include_edges: bool,
    part_slot: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

fn build_instanced_chunk(
    device: &wgpu::Device,
    parts: &[MeshBatchPart<'_>],
) -> Option<(MeshBatchChunk, u64)> {
    let first = parts.first()?;
    let source = first.set.instance_source.as_ref()?;
    let mesh = first.mesh;
    let material = first.material;
    let color = first.color;
    let has_normals = mesh.normals.len() == mesh.verts.len();
    let bounds = mesh_bounds(mesh);
    let (material_params, specular, ambient, advanced, flags) =
        material_vertex_params(material);
    let vertex = |index: usize, edge: bool| {
        let normal = if edge {
            [0.0, 1.0, 0.0]
        } else if has_normals {
            mesh.normals[index]
        } else {
            [0.0, 1.0, 0.0]
        };
        let position = if edge {
            source.edge_verts[index]
        } else {
            mesh.verts[index]
        };
        let position_low = if edge {
            source
                .edge_verts_low
                .get(index)
                .copied()
                .unwrap_or([0.0; 3])
        } else {
            mesh.verts_low.get(index).copied().unwrap_or([0.0; 3])
        };
        let uvs = if edge {
            [[0.0; 2]; 7]
        } else {
            material_uvs(
                material,
                position,
                normal,
                bounds,
                position,
                normal,
                bounds,
            )
        };
        MeshVertex {
            position,
            normal,
            color,
            position_low,
            material: material_params,
            specular,
            uv_diffuse: uvs[0],
            ambient,
            advanced,
            flags,
            uv_specular: uvs[1],
            uv_reflection: uvs[2],
            uv_opacity: uvs[3],
            uv_bump: uvs[4],
            uv_refraction: uvs[5],
            uv_normal: uvs[6],
        }
    };
    let verts: Vec<_> = (0..mesh.verts.len())
        .map(|index| vertex(index, false))
        .collect();
    let edge_color = first
        .set
        .visual_style
        .as_ref()
        .map_or(first.display_mesh.color, |style| {
            style.edge_color(first.display_mesh.color)
        });
    let edge_verts: Vec<_> = if first.include_edges {
        (0..source.edge_verts.len())
            .map(|index| {
                let mut out = vertex(index, true);
                out.color = edge_color;
                out
            })
            .collect()
    } else {
        Vec::new()
    };
    let has_feature_edges = !edge_verts.is_empty();
    let mut wire_indices = Vec::new();
    if first.include_edges && !has_feature_edges {
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
    let transparent = material_is_transparent(material, color);
    let face_indices = first.include_faces.then_some(first.indices.as_slice());
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
        if let Some(handle) = part
            .display_mesh
            .name
            .parse::<u64>()
            .ok()
            .map(acadrust::Handle::new)
        {
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
    let triangles = if first.include_faces {
        (first.indices.len() / 3) as u64 * instances.len() as u64
    } else {
        0
    };
    Some((
        make_chunk(
            device,
            &verts,
            opaque_indices,
            transparent_indices,
            &wire_indices,
            &edge_verts,
            &highlights,
            &instances,
            &handles,
            Some(bounds),
            material,
        ),
        triangles,
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

fn material_uses_model_mapper(
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
        .any(|map| map.image.is_some() && map.auto_transform & 4 != 0)
    })
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
    [
        material_map_uv(&material.diffuse_map, local_position, local_normal, local_bounds, model_position, model_normal, model_bounds),
        material_map_uv(&material.specular_map, local_position, local_normal, local_bounds, model_position, model_normal, model_bounds),
        material_map_uv(&material.reflection_map, local_position, local_normal, local_bounds, model_position, model_normal, model_bounds),
        material_map_uv(&material.opacity_map, local_position, local_normal, local_bounds, model_position, model_normal, model_bounds),
        material_map_uv(&material.bump_map, local_position, local_normal, local_bounds, model_position, model_normal, model_bounds),
        material_map_uv(&material.refraction_map, local_position, local_normal, local_bounds, model_position, model_normal, model_bounds),
        material_map_uv(&material.normal_map, local_position, local_normal, local_bounds, model_position, model_normal, model_bounds),
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
pub fn build_mesh_batch(device: &wgpu::Device, sets: &[MeshLodSet]) -> (Vec<MeshBatchChunk>, u64) {
    build_mesh_batch_filtered(device, sets, None)
}

pub fn build_mesh_batch_filtered(
    device: &wgpu::Device,
    sets: &[MeshLodSet],
    handles: Option<&rustc_hash::FxHashSet<acadrust::Handle>>,
) -> (Vec<MeshBatchChunk>, u64) {
    // Derive the caps from the real device limit and vertex size. The previous
    // fixed 6 M-vertex cap assumed 40 B/vertex, but `position_low` (RTE) grew
    // MeshVertex to 52 B, so 6 M × 52 B = 312 MB blew past the 256 MB cap.
    let hard_budget = (device.limits().max_buffer_size as usize / 10) * 9;
    let budget = hard_budget.min(32 * 1024 * 1024);
    let vsize = std::mem::size_of::<MeshVertex>();
    let max_verts = (budget / vsize).max(3);
    let max_tris = (budget / (6 * 4)).max(1); // wire-index buffer: 6 u32 per tri

    let mut chunks = Vec::new();
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut transp_indices: Vec<u32> = Vec::new();
    let mut wire_indices: Vec<u32> = Vec::new();
    let mut edge_verts: Vec<MeshVertex> = Vec::new();
    let mut highlight_ranges: Vec<MeshBatchRange> = Vec::new();
    let mut chunk_handles: rustc_hash::FxHashSet<acadrust::Handle> =
        rustc_hash::FxHashSet::default();
    let mut total_tris = 0u64;
    let mut ordered: Vec<MeshBatchPart<'_>> = Vec::new();
    for set in sets {
        let Some(display_mesh) = set.lods.iter().find(|mesh| !mesh.indices.is_empty()) else {
            continue;
        };
        let display_handle = display_mesh
            .name
            .parse::<u64>()
            .ok()
            .map(acadrust::Handle::new);
        if handles.is_some_and(|wanted| {
            display_handle.is_none_or(|handle| !wanted.contains(&handle))
        }) {
            continue;
        }
        let source_mesh = set
            .instance_source
            .as_ref()
            .and_then(|source| source.lods.iter().find(|mesh| !mesh.indices.is_empty()));
        let mesh = source_mesh.unwrap_or(display_mesh);
        let triangle_count = display_mesh.indices.len() / 3;
        let has_face_materials =
            display_mesh.triangle_material_handles.len() == triangle_count
            && !set.face_materials.is_empty();
        let has_face_colors = display_mesh.triangle_colors.len() == triangle_count
            && display_mesh.triangle_colors.iter().any(Option::is_some);
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
                set.material.as_ref().map_or(display_mesh.color, |material| material.diffuse);
            let color = set
                .visual_style
                .as_ref()
                .map_or(base_color, |style| style.face_color(base_color));
            ordered.push(MeshBatchPart {
                set,
                mesh,
                display_mesh,
                uv_mesh: mesh,
                material: set.material.as_ref(),
                color,
                indices: display_mesh.indices.clone(),
                include_faces,
                include_edges,
                part_slot: 0,
            });
            continue;
        }
        let mut groups: std::collections::BTreeMap<
            MaterialBatchKey,
            (
                Option<&crate::scene::model::material_model::MeshMaterial>,
                [f32; 4],
                Vec<u32>,
            ),
        > = std::collections::BTreeMap::new();
        for (triangle, indices) in display_mesh.indices.chunks_exact(3).enumerate() {
            let material = if has_face_materials {
                display_mesh.triangle_material_handles[triangle]
                    .and_then(|handle| set.face_materials.get(&handle))
                    .or(set.material.as_ref())
            } else {
                set.material.as_ref()
            };
            let base_color =
                material.map_or(display_mesh.color, |material| material.diffuse);
            let base_color = if has_face_colors {
                display_mesh.triangle_colors[triangle].unwrap_or(base_color)
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
        for (part_index, (_, (material, color, indices))) in groups.into_iter().enumerate() {
            ordered.push(MeshBatchPart {
                set,
                mesh,
                display_mesh,
                uv_mesh: mesh,
                material,
                color,
                indices,
                include_faces,
                include_edges: include_edges && part_index == 0,
                part_slot: part_index as u32,
            });
        }
    }
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
    let storage_instancing =
        device.limits().max_storage_buffers_per_shader_stage > 0;
    let mut instance_groups: std::collections::BTreeMap<
        InstanceGroupKey,
        Vec<MeshBatchPart<'_>>,
    > = std::collections::BTreeMap::new();
    let mut direct_parts = Vec::with_capacity(ordered.len());
    for mut part in ordered {
        let eligible = storage_instancing
            && part.set.instance_transform.is_some()
            && part.set.instance_source.is_some()
            && !material_uses_model_mapper(part.material)
            && part.mesh.verts.len() <= max_verts
            && part.indices.len() / 3 <= max_tris
            && part
                .set
                .instance_source
                .as_ref()
                .is_some_and(|source| source.edge_verts.len() <= max_verts)
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
                    index_hash: index_hash(&part.indices),
                    include_faces: part.include_faces,
                    include_edges: part.include_edges,
                })
                .or_default()
                .push(part);
        } else {
            // Compatibility and non-instanced meshes keep their already
            // transformed display vertices in the ordinary static batch.
            part.mesh = part.display_mesh;
            direct_parts.push(part);
        }
    }
    let sparse_groups: Vec<_> = instance_groups
        .iter()
        .filter_map(|(key, parts)| {
            let first = parts.first()?;
            let geometry_bytes = first
                .mesh
                .verts
                .len()
                .saturating_mul(std::mem::size_of::<MeshVertex>())
                .saturating_add(first.indices.len().saturating_mul(12))
                .saturating_add(
                    first
                        .set
                        .instance_source
                        .as_ref()
                        .map_or(0, |source| {
                            source
                                .edge_verts
                                .len()
                                .saturating_mul(std::mem::size_of::<MeshVertex>())
                        }),
                );
            let saved_bytes = geometry_bytes.saturating_mul(parts.len().saturating_sub(1));
            (parts.len() < 4 && saved_bytes < 512 * 1024).then_some(*key)
        })
        .collect();
    for key in sparse_groups {
        if let Some(parts) = instance_groups.remove(&key) {
            for mut part in parts {
                part.mesh = part.display_mesh;
                direct_parts.push(part);
            }
        }
    }
    direct_parts.sort_by_key(|part| {
        (
            material_key(part.material, part.color),
            mesh_spatial_key(part.set, spatial_bounds),
        )
    });
    let ordered = direct_parts;
    let mut active_key: Option<MaterialBatchKey> = None;
    let mut active_material: Option<&crate::scene::model::material_model::MeshMaterial> = None;
    for part in ordered {
        let set = part.set;
        let mesh = part.mesh;
        let material = part.material;
        let part_color = part.color;
        let entity_handle = part
            .display_mesh
            .name
            .parse::<u64>()
            .ok()
            .map(acadrust::Handle::new);
        let key = material_key(material, part_color);
        if active_key.is_some_and(|active| active != key)
            && (!verts.is_empty() || !edge_verts.is_empty())
        {
            chunks.push(make_chunk(
                device,
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
        if let Some(handle) = entity_handle {
            chunk_handles.insert(handle);
        }
        let has_normals = mesh.normals.len() == mesh.verts.len();
        let (material_params, specular, ambient, advanced, flags) =
            material_vertex_params(material);
        let uv_mesh = part.uv_mesh;
        let local_bounds = mesh_bounds(uv_mesh);
        let model_bounds = mesh_bounds(mesh);
        let edge_color = set
            .visual_style
            .as_ref()
            .map_or(mesh.color, |style| style.edge_color(mesh.color));
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
                color: part_color,
                position_low: mesh.verts_low.get(vi).copied().unwrap_or([0.0; 3]),
                material: material_params,
                specular,
                uv_diffuse: uv[0],
                ambient,
                advanced,
                flags,
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
                    edge_verts.push(MeshVertex {
                        position: set.edge_verts[i],
                        normal: [0.0, 1.0, 0.0],
                        color: edge_color,
                        position_low: set.edge_verts_low.get(i).copied().unwrap_or([0.0; 3]),
                        material: material_params,
                        specular,
                        uv_diffuse: [0.0; 2],
                        ambient,
                        advanced,
                        flags,
                        uv_specular: [0.0; 2],
                        uv_reflection: [0.0; 2],
                        uv_opacity: [0.0; 2],
                        uv_bump: [0.0; 2],
                        uv_refraction: [0.0; 2],
                        uv_normal: [0.0; 2],
                    });
                }
                edge_start += take;
                if edge_start < edge_end {
                    chunks.push(make_chunk(
                        device,
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
                    ));
                } else {
                    let sub_handles: rustc_hash::FxHashSet<_> =
                        entity_handle.into_iter().collect();
                    chunks.push(make_chunk(
                        device,
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
                    ));
                }
                t = end;
            }
            chunk_handles.clear();
            continue;
        }

        // Flush when adding this mesh would overflow either the vertex buffer
        // or the wire-index buffer.
        if !verts.is_empty()
            && (verts.len() + mesh.verts.len() > max_verts
                || wire_indices.len() / 6 + mesh_tris > max_tris)
        {
            chunks.push(make_chunk(
                device,
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
            for &idx in &part.indices {
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
        ));
    }
    for parts in instance_groups.values() {
        let Some(first) = parts.first() else {
            continue;
        };
        let source_bytes = first
            .mesh
            .verts
            .len()
            .saturating_mul(std::mem::size_of::<MeshVertex>())
            .saturating_add(first.indices.len().saturating_mul(12))
            .saturating_add(
                first
                    .set
                    .instance_source
                    .as_ref()
                    .map_or(0, |source| {
                        source
                            .edge_verts
                            .len()
                            .saturating_mul(std::mem::size_of::<MeshVertex>())
                    }),
            )
            .max(1);
        // Keep repeated geometry bounded while retaining useful culling
        // granularity. Small block definitions get spatial clusters of roughly
        // 64–256 INSERTs; a huge source is duplicated only a few times.
        let max_clusters = ((64 * 1024 * 1024) / source_bytes).clamp(1, 64);
        let cluster_len = parts
            .len()
            .div_ceil(max_clusters)
            .max(64);
        for cluster in parts.chunks(cluster_len) {
            if let Some((chunk, triangles)) = build_instanced_chunk(device, cluster) {
                total_tris += triangles;
                chunks.push(chunk);
            }
        }
    }
    (chunks, total_tris)
}
