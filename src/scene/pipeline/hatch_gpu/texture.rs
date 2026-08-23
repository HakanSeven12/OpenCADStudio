// Texture-backed hatch renderer for devices without storage buffers. Boundary
// geometry comes from the same kernel mesh as the storage backend.

use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;

/// Width (in texels) of the RGBA32F data texture. Height grows to fit; a hatch
/// with N total texels uses ceil(N / WIDTH) rows.
const DATA_TEX_WIDTH: u32 = 1024;

// ── Vertex ────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct HatchVertex {
    pos: [f32; 3],
    _pad: f32,
}

pub(super) fn vertex_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<HatchVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        }],
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct TextureHatchPlacement {
    translation: [f32; 2],
    translation_low: [f32; 2],
    draw_depth: f32,
    _pad: [f32; 3],
}

impl TextureHatchPlacement {
    pub(super) fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 16, shader_location: 3, format: wgpu::VertexFormat::Float32 },
            ],
        }
    }
}

// ── Per-hatch uniform (binding 0) — 96 bytes, matches HatchUniforms in
//    hatch_texture.wgsl. ──────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TextureHatchUniform {
    color: [f32; 4],      //  0
    color2: [f32; 4],     // 16
    mode: u32,            // 32
    _reserved: u32,       // 36
    angle_offset: f32,    // 40
    scale: f32,           // 44
    grad_cos: f32,        // 48
    grad_sin: f32,        // 52
    grad_min: f32,        // 56
    grad_range: f32,      // 60
    origin: [f32; 2],     // 64
    origin_low: [f32; 2], // 72
    n_families: u32,      // 80
    fam_off: u32,         // 84: texel offset of the family section
    dash_off: u32,        // 88: texel offset of the dash section
    tex_width: u32,       // 92
}

// ── Per-hatch GPU handle ────────────────────────────────────────────────────

pub(super) struct TextureHatch {
    pub(super) vertex_buffer: wgpu::Buffer,
    pub(super) index_buffer: wgpu::Buffer,
    pub(super) placement_buffer: wgpu::Buffer,
    pub(super) index_count: u32,
    pub(super) instance_count: u32,
    pub(super) bind_group: wgpu::BindGroup,
    /// Reserved for per-frame AABB LOD (mirrors `WipeoutGpu`); not yet wired
    /// into the web hatch draw loop — the native batched path doesn't do
    /// per-hatch LOD either.
    #[allow(dead_code)]
    pub world_aabb: [f32; 4],
    _uniform_buf: wgpu::Buffer,
    _data_tex: wgpu::Texture,
}

impl TextureHatch {
    pub(super) fn from_models(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        models: &[HatchModel],
        bgl1: &wgpu::BindGroupLayout,
    ) -> Vec<Self> {
        let mut slots = rustc_hash::FxHashMap::default();
        let mut groups: Vec<Vec<&HatchModel>> = Vec::new();
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
        groups
            .into_iter()
            .filter_map(|group| Self::new(device, queue, &group, bgl1))
            .collect()
    }

    /// Group-1 layout: uniform header (binding 0) + non-filterable float data
    /// texture (binding 1).
    pub(super) fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hatch.texture.bgl1"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        })
    }

    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        models: &[&HatchModel],
        bgl1: &wgpu::BindGroupLayout,
    ) -> Option<Self> {
        let model = models[0];
        // ── Decode pattern mode (mirrors WipeoutGpu::new) ─────────────────
        // The gradient shape (kind + invert bit) rides in the mode's high
        // bits — the 96-byte uniform has no spare slot.
        let (mode, color2, grad_cos, grad_sin) = match &model.pattern {
            HatchPattern::Solid => (1u32, [0.0f32; 4], 0.0f32, 0.0f32),
            HatchPattern::Pattern(_) => (0u32, [0.0f32; 4], 0.0f32, 0.0f32),
            HatchPattern::Gradient { angle_deg, color2, kind, invert, .. } => {
                let gk = (kind.shader_kind() | if *invert { 16 } else { 0 }) << 8;
                if kind.radial() {
                    // Radial: centre is the local origin; grad_cos/sin unused.
                    (3u32 | gk, *color2, 0.0, 0.0)
                } else {
                    let r = angle_deg.to_radians();
                    (2u32 | gk, *color2, r.cos(), r.sin())
                }
            }
        };

        // ── Bounding box ─────────────────────────────────────────────────
        let (mut min_x, mut max_x, mut min_y, mut max_y) =
            (f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
        for &[x, y] in model.boundary.iter() {
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }

        let origin = model.world_origin;
        let (mesh_points, indices) = model.fill_mesh();
        if mesh_points.is_empty() || indices.is_empty() {
            return None;
        }
        let vertices: Vec<HatchVertex> = mesh_points
            .into_iter()
            .map(|[x, y]| HatchVertex { pos: [x, y, 0.0], _pad: 0.0 })
            .collect();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.texture.vbuf"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.texture.ibuf"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let base = model
            .render_instance
            .map_or([0.0; 3], |instance| instance.translation);
        let placements: Vec<TextureHatchPlacement> = models
            .iter()
            .map(|model| {
                let translation = model
                    .render_instance
                    .map_or([0.0; 3], |instance| instance.translation);
                let delta = [translation[0] - base[0], translation[1] - base[1]];
                let high = [delta[0] as f32, delta[1] as f32];
                TextureHatchPlacement {
                    translation: high,
                    translation_low: [
                        (delta[0] - high[0] as f64) as f32,
                        (delta[1] - high[1] as f64) as f32,
                    ],
                    draw_depth: model.draw_depth,
                    _pad: [0.0; 3],
                }
            })
            .collect();
        let placement_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.texture.placements"),
            contents: bytemuck::cast_slice(&placements),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // ── Gradient projection range (snapped-local space) ───────────────
        let base_mode = mode & 0xFF;
        let frame = match &model.pattern {
            HatchPattern::Gradient {
                angle_deg, shift, ..
            } => model.gradient_frame(*angle_deg, *shift),
            _ => None,
        };
        let (grad_min, grad_range, radial_center) = if base_mode == 2 {
            frame.map_or((0.0, 1.0, [0.0, 0.0]), |frame| {
                (
                    frame.projection_min as f32,
                    frame.projection_span as f32,
                    [0.0, 0.0],
                )
            })
        } else if base_mode == 3 {
            frame.map_or((0.0, 1.0, [0.0, 0.0]), |frame| {
                (
                    0.0,
                    frame.radius as f32,
                    [frame.center[0] as f32, frame.center[1] as f32],
                )
            })
        } else {
            (0.0, 1.0, [0.0, 0.0])
        };

        // ── Pack the data texture: families | dashes ─────────────────────
        let mut texels: Vec<[f32; 4]> = Vec::new();
        // Family section (3 texels each) + a flat dash pool.
        let fam_off = texels.len() as u32;
        let mut n_families = 0u32;
        let mut dash_pool: Vec<f32> = Vec::new();
        if let HatchPattern::Pattern(families) = &model.pattern {
            for fam in families.iter() {
                let dash_rel = dash_pool.len() as u32;
                dash_pool.extend_from_slice(&fam.dashes);
                let n_dashes = fam.dashes.len() as u32;
                let period: f32 = if n_dashes > 0 {
                    fam.dashes.iter().map(|d| d.abs()).sum()
                } else {
                    0.0
                };
                let angle_r = fam.angle_deg.to_radians();
                // PAT local frame: perpendicular spacing and along-line phase.
                texels.push([angle_r.cos(), angle_r.sin(), fam.x0, fam.y0]);
                texels.push([fam.dx, fam.dy, fam.dy, fam.dx]);
                // Counts as exact f32 (small integers → no denormal/bitcast risk).
                texels.push([0.0, period, n_dashes as f32, dash_rel as f32]);
                n_families += 1;
            }
        }

        // Dash section (4 values per texel).
        let dash_off = texels.len() as u32;
        let mut i = 0usize;
        while i < dash_pool.len() {
            let mut t = [0.0f32; 4];
            for (c, slot) in t.iter_mut().enumerate() {
                if let Some(&d) = dash_pool.get(i + c) {
                    *slot = d;
                }
            }
            texels.push(t);
            i += 4;
        }

        // Upload the texture (min 1 texel; pad the last row).
        if texels.is_empty() {
            texels.push([0.0; 4]);
        }
        let width = DATA_TEX_WIDTH;
        let height = ((texels.len() as u32).div_ceil(width)).max(1);
        texels.resize((width * height) as usize, [0.0; 4]);
        let data_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hatch.texture.data_tex"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            data_tex.as_image_copy(),
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 16),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let tex_view = data_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // ── Uniform header ────────────────────────────────────────────────
        let uniform_data = TextureHatchUniform {
            color: model.color,
            color2,
            mode,
            _reserved: 0,
            angle_offset: model.angle_offset,
            // Clamp like the desktop renderer so scale==0 can't make perp_step 0
            // → round(perp/0)=NaN → an invisible hatch.
            scale: model.scale.max(1e-6),
            grad_cos: if base_mode == 3 { radial_center[0] } else { grad_cos },
            grad_sin: if base_mode == 3 { radial_center[1] } else { grad_sin },
            grad_min,
            grad_range,
            origin: [origin[0] as f32, origin[1] as f32],
            origin_low: [
                (origin[0] - origin[0] as f32 as f64) as f32,
                (origin[1] - origin[1] as f32 as f64) as f32,
            ],
            n_families,
            fam_off,
            dash_off,
            tex_width: width,
        };
        let _uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.texture.uniform"),
            contents: bytemuck::bytes_of(&uniform_data),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hatch.texture.bind_group1"),
            layout: bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: _uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
            ],
        });

        let ox = model.world_origin[0] as f32;
        let oy = model.world_origin[1] as f32;
        let world_aabb = if min_x.is_finite() && min_y.is_finite() {
            [min_x + ox, min_y + oy, max_x + ox, max_y + oy]
        } else {
            [min_x, min_y, max_x, max_y]
        };

        Some(Self {
            vertex_buffer,
            index_buffer,
            placement_buffer,
            index_count: indices.len() as u32,
            instance_count: placements.len() as u32,
            bind_group,
            world_aabb,
            _uniform_buf,
            _data_tex: data_tex,
        })
    }
}
