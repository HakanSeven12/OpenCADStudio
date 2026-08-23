// Image GPU buffers — renders raster images as textured quads.
//
// Group 1 bindings per image:
//   binding 0 — texture_2d<f32>   (RGBA image texture)
//   binding 1 — sampler           (bilinear filtering)
//   binding 2 — ImageParams       (opacity uniform, 16 bytes)

use crate::scene::model::image_model::ImageModel;
use iced::wgpu;
use iced::wgpu::util::DeviceExt;
use std::sync::Arc;

// ── Vertex ────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ImageVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub pos_low: [f32; 3],
}

impl ImageVertex {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(ImageVertex, pos) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(ImageVertex, uv) as u64,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(ImageVertex, pos_low) as u64,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x3,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ImageInstance {
    pub translation: [f32; 3],
    pub translation_low: [f32; 3],
    pub draw_depth: f32,
    pub _pad: [f32; 3],
}

impl ImageInstance {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute { offset: std::mem::offset_of!(ImageInstance, translation) as u64, shader_location: 3, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(ImageInstance, translation_low) as u64, shader_location: 4, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(ImageInstance, draw_depth) as u64, shader_location: 5, format: wgpu::VertexFormat::Float32 },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

// ── Uniform ───────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ImageParams {
    opacity: f32,
    /// Signed draw-order depth (-1,1); applied as a clip-z bias in the shader
    /// so the raster orders against other entity types. 0.0 = neutral.
    draw_depth: f32,
    _pad: [f32; 2],
} // 16 bytes

// ── Per-image GPU handle ──────────────────────────────────────────────────

pub struct ImageGpu {
    pub vertex_buffer: wgpu::Buffer,
    pub instance_buffer: Arc<wgpu::Buffer>,
    /// Number of triangle vertices in `vertex_buffer` — 6 for a plain quad, or
    /// more when the raster is clipped to a triangulated polygon.
    pub vertex_count: u32,
    pub instance_count: u32,
    pub bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
    _sampler: Arc<wgpu::Sampler>,
    _params_buf: Arc<wgpu::Buffer>,
}

impl ImageGpu {
    pub fn from_models(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        models: &[ImageModel],
        bgl1: &wgpu::BindGroupLayout,
    ) -> Vec<Self> {
        let mut slots = rustc_hash::FxHashMap::default();
        let mut groups: Vec<Vec<&ImageModel>> = Vec::new();
        for (index, model) in models.iter().enumerate() {
            let key = model
                .render_instance
                .map(|instance| (true, instance.source_id))
                .unwrap_or((false, index as u64));
            let slot = *slots.entry(key).or_insert_with(|| {
                let slot = groups.len();
                groups.push(Vec::new());
                slot
            });
            groups[slot].push(model);
        }
        let sampler = Arc::new(device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        }));
        groups
            .into_iter()
            .flat_map(|group| Self::new(device, queue, &group, bgl1, &sampler))
            .collect()
    }

    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        models: &[&ImageModel],
        bgl1: &wgpu::BindGroupLayout,
        sampler: &Arc<wgpu::Sampler>,
    ) -> Vec<Self> {
        let Some(&model) = models.first() else {
            return Vec::new();
        };
        if model.pixels.is_empty() || model.width == 0 || model.height == 0 {
            return Vec::new();
        }

        let limit = device.limits().max_texture_dimension_2d.max(1);
        let x_tiles = tile_ranges(model.width, limit);
        let y_tiles = tile_ranges(model.height, limit);
        let base = model
            .render_instance
            .map_or([0.0; 3], |instance| instance.translation);
        let instances: Vec<ImageInstance> = models
            .iter()
            .map(|model| {
                let translation = model
                    .render_instance
                    .map_or([0.0; 3], |instance| instance.translation);
                let delta = [
                    translation[0] - base[0],
                    translation[1] - base[1],
                    translation[2] - base[2],
                ];
                let high = delta.map(|value| value as f32);
                ImageInstance {
                    translation: high,
                    translation_low: [
                        (delta[0] - high[0] as f64) as f32,
                        (delta[1] - high[1] as f64) as f32,
                        (delta[2] - high[2] as f64) as f32,
                    ],
                    draw_depth: model.draw_depth,
                    _pad: [0.0; 3],
                }
            })
            .collect();
        let instance_buffer = Arc::new(device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("image.instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            },
        ));
        let params = ImageParams {
            opacity: model.opacity.clamp(0.0, 1.0),
            draw_depth: 0.0,
            _pad: [0.0; 2],
        };
        let params_buf = Arc::new(device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("image.params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        ));

        let mut output = Vec::with_capacity(x_tiles.len() * y_tiles.len());
        for y in &y_tiles {
            for x in &x_tiles {
                let verts = tile_vertices(model, *x, *y);
                if verts.is_empty() {
                    continue;
                }

                let width = x.data_end - x.data_start;
                let height = y.data_end - y.data_start;
                let tex_label = format!(
                    "image.texture:{}:{}:{}",
                    model.file_path, x.content_start, y.content_start
                );
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(&tex_label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    texture.as_image_copy(),
                    &model.pixels[((y.data_start as usize * model.width as usize
                        + x.data_start as usize)
                        * 4)..],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * model.width),
                        rows_per_image: Some(model.height),
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
                let tex_view =
                    texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("image.bind_group1"),
                    layout: bgl1,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&tex_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: params_buf.as_entire_binding(),
                        },
                    ],
                });
                let vertex_buffer =
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("image.vbuf"),
                        contents: bytemuck::cast_slice(&verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                output.push(Self {
                    vertex_buffer,
                    instance_buffer: Arc::clone(&instance_buffer),
                    vertex_count: verts.len() as u32,
                    instance_count: instances.len() as u32,
                    bind_group,
                    _texture: texture,
                    _sampler: Arc::clone(sampler),
                    _params_buf: Arc::clone(&params_buf),
                });
            }
        }
        output
    }
}

#[derive(Clone, Copy)]
struct TileRange {
    content_start: u32,
    content_end: u32,
    data_start: u32,
    data_end: u32,
}

fn tile_ranges(size: u32, limit: u32) -> Vec<TileRange> {
    if size <= limit {
        return vec![TileRange {
            content_start: 0,
            content_end: size,
            data_start: 0,
            data_end: size,
        }];
    }
    let gutter = u32::from(limit > 2);
    let content_size = (limit - gutter * 2).max(1);
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < size {
        let end = start.saturating_add(content_size).min(size);
        ranges.push(TileRange {
            content_start: start,
            content_end: end,
            data_start: start.saturating_sub(gutter),
            data_end: end.saturating_add(gutter).min(size),
        });
        start = end;
    }
    ranges
}

fn tile_vertices(model: &ImageModel, x: TileRange, y: TileRange) -> Vec<ImageVertex> {
    let source: Vec<ImageVertex> = model
        .verts
        .iter()
        .map(|vertex| ImageVertex {
            pos: vertex.pos,
            uv: vertex.uv,
            pos_low: vertex.pos_low,
        })
        .collect();
    if x.content_start == 0
        && x.content_end == model.width
        && y.content_start == 0
        && y.content_end == model.height
    {
        return source;
    }

    let bounds = [
        (0, x.content_start as f32 / model.width as f32, true),
        (0, x.content_end as f32 / model.width as f32, false),
        (1, y.content_start as f32 / model.height as f32, true),
        (1, y.content_end as f32 / model.height as f32, false),
    ];
    let mut output = Vec::new();
    for triangle in source.chunks_exact(3) {
        let mut polygon = triangle.to_vec();
        for (axis, boundary, keep_greater) in bounds {
            polygon = clip_vertices(&polygon, axis, boundary, keep_greater);
        }
        for index in 1..polygon.len().saturating_sub(1) {
            output.push(polygon[0]);
            output.push(polygon[index]);
            output.push(polygon[index + 1]);
        }
    }

    let data_width = (x.data_end - x.data_start) as f32;
    let data_height = (y.data_end - y.data_start) as f32;
    for vertex in &mut output {
        vertex.uv = [
            ((vertex.uv[0] * model.width as f32 - x.data_start as f32) / data_width)
                .clamp(0.0, 1.0),
            ((vertex.uv[1] * model.height as f32 - y.data_start as f32) / data_height)
                .clamp(0.0, 1.0),
        ];
    }
    output
}

fn clip_vertices(
    input: &[ImageVertex],
    axis: usize,
    boundary: f32,
    keep_greater: bool,
) -> Vec<ImageVertex> {
    if input.is_empty() {
        return Vec::new();
    }
    let inside = |vertex: &ImageVertex| {
        if keep_greater {
            vertex.uv[axis] >= boundary
        } else {
            vertex.uv[axis] <= boundary
        }
    };
    let mut output = Vec::new();
    let mut previous = *input.last().unwrap();
    let mut previous_inside = inside(&previous);
    for &current in input {
        let current_inside = inside(&current);
        if current_inside != previous_inside {
            output.push(intersect_vertex(previous, current, axis, boundary));
        }
        if current_inside {
            output.push(current);
        }
        previous = current;
        previous_inside = current_inside;
    }
    output
}

fn intersect_vertex(
    start: ImageVertex,
    end: ImageVertex,
    axis: usize,
    boundary: f32,
) -> ImageVertex {
    let distance = end.uv[axis] - start.uv[axis];
    let t = if distance.abs() > f32::EPSILON {
        ((boundary - start.uv[axis]) / distance).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ImageVertex {
        pos: interpolate3(start.pos, end.pos, t),
        uv: [
            start.uv[0] + (end.uv[0] - start.uv[0]) * t,
            start.uv[1] + (end.uv[1] - start.uv[1]) * t,
        ],
        pos_low: interpolate3(start.pos_low, end.pos_low, t),
    }
}

fn interpolate3(start: [f32; 3], end: [f32; 3], t: f32) -> [f32; 3] {
    [
        start[0] + (end[0] - start[0]) * t,
        start[1] + (end[1] - start[1]) * t,
        start[2] + (end[2] - start[2]) * t,
    ]
}
