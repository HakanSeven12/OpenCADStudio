// Wipeout GPU buffers — the legacy per-primitive fill renderer, now used ONLY
// for wipeout masks (solid fills drawn after wires to hide them). Real hatch
// fills go through the capability-selected `hatch_gpu` renderer.
//
// It retains a general pattern/gradient capability (mode 0/2 below) and the
// `MAX_FAMILIES = 16` cap, but wipeouts are always solid (mode 1, zero pattern
// families), so the cap never truncates anything in practice. The family code
// is kept only as a latent non-batched fallback; do NOT read `MAX_FAMILIES` as
// a limit on visible hatch fills.
//
// Group 1 bindings per fill:
//   binding 0 — HatchUniformData  fill and plane parameters
//   binding 1 — BoundaryData      (16384 bytes) boundary polygon vertices
//   binding 2 — FamilyBatchData   (1296 bytes)  up to 16 line families + 128 dash values (unused for wipeouts)

use crate::scene::model::hatch_model::{
    FillPlane, HatchModel, HatchPattern, PatFamily, MAX_HATCH_BOUNDARY_VERTS,
};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;

// ── Constants ──────────────────────────────────────────────────────────────

pub const MAX_FAMILIES: usize = 16;
pub const MAX_DASHES: usize = 128;

// ── Vertex ────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HatchVertex {
    pub pos: [f32; 3],
    pub _pad: f32,
}

impl HatchVertex {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
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
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WipeoutPlacement {
    pub translation: [f32; 3],
    pub translation_low: [f32; 3],
    pub draw_depth: f32,
    pub _pad: f32,
}

impl WipeoutPlacement {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 2, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 24, shader_location: 3, format: wgpu::VertexFormat::Float32 },
            ],
        }
    }
}

// ── Uniform structs ───────────────────────────────────────────────────────

/// Per-fill parameters (binding 0).
///
/// `mode` encoding:
///   0 → Pattern  (families in FamilyBatchData)
///   1 → Solid
///   2 → Gradient (grad_cos/sin/min/range used)
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HatchUniformData {
    pub color: [f32; 4],     //  0: primary RGBA / gradient start
    pub color2: [f32; 4],    // 16: gradient end color
    pub mode: u32,           // 32: 0=pattern, 1=solid, 2=gradient
    pub vertex_count: u32,   // 36: boundary vertex count
    pub angle_offset: f32,   // 40: pattern rotation (radians)
    pub scale: f32,          // 44: pattern scale multiplier
    pub grad_cos: f32,       // 48: gradient direction cos
    pub grad_sin: f32,       // 52: gradient direction sin
    pub grad_min: f32,       // 56: gradient proj_min
    pub grad_range: f32,     // 60: gradient proj_range
    pub origin: [f32; 2],     // 64: reserved pattern origin
    pub origin_low: [f32; 2], // 72: reserved low residual
    pub draw_depth: f32,      // 80: signed (-1,1) draw-order bias
    pub _pad: [f32; 3],       // 84..96
    pub plane_origin_high: [f32; 4],
    pub plane_origin_low: [f32; 4],
    pub plane_x: [f32; 4],
    pub plane_y: [f32; 4],
}

/// Boundary polygon (binding 1).  Matches WGSL `Boundary`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BoundaryData {
    pub verts: [[f32; 4]; MAX_HATCH_BOUNDARY_VERTS], // world XY in .xy, .zw unused
}

/// One line family packed for the shader (48 bytes).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineFamilyGpu {
    pub cos_a: f32,      // cos(angle_rad)
    pub sin_a: f32,      // sin(angle_rad)
    pub x0: f32,         // family origin x
    pub y0: f32,         // family origin y
    pub dx: f32,         // step vector x
    pub dy: f32,         // step vector y
    pub perp_step: f32,  // -dx*sin_a + dy*cos_a  (perpendicular spacing)
    pub along_step: f32, //  dx*cos_a + dy*sin_a  (along-line phase shift per step)
    pub line_width: f32, // |perp_step| * 0.08
    pub period: f32,     // sum of |dash values|  (0 = solid)
    pub n_dashes: u32,   // number of dash entries (0 = solid)
    pub dash_off: u32,   // index into FamilyBatchData::dash_values
}

/// All line families + dash values for one hatch (binding 2).
/// Total size: 16×48 + 32×16 + 4×4 = 768 + 512 + 16 = 1296 bytes.
/// dash_values is [f32; 4]×32 so each element is vec4<f32> (16-byte stride, valid in uniform).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FamilyBatchData {
    pub families: [LineFamilyGpu; MAX_FAMILIES], // 768 bytes
    pub dash_values: [[f32; 4]; 32],             // 512 bytes (128 f32s as 32×vec4)
    pub n_families: u32,                         //   4 bytes
    pub _pad: [u32; 3],                          //  12 bytes
} // 1296 bytes total

// ── Per-hatch GPU handle ───────────────────────────────────────────────────

pub struct WipeoutGpu {
    pub vertex_buffer: wgpu::Buffer,
    pub instance_buffer: wgpu::Buffer,
    pub instance_count: u32,
    pub bind_group: wgpu::BindGroup,
    /// World-space XY bounding rect [min_x, min_y, max_x, max_y] of the
    /// boundary polygon. Used by the per-frame LOD pass to skip hatches
    /// whose entire footprint projects to less than ~2 px (Phase 3.3).
    pub world_aabb: [f32; 4],
    _uniform_buf: wgpu::Buffer,
    _boundary_buf: wgpu::Buffer,
    _family_buf: wgpu::Buffer,
}

impl WipeoutGpu {
    pub fn from_models(
        device: &wgpu::Device,
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
            .map(|group| Self::new(device, &group, bgl1))
            .collect()
    }

    fn new(device: &wgpu::Device, models: &[&HatchModel], bgl1: &wgpu::BindGroupLayout) -> Self {
        let model = models[0];
        // ── Decode pattern mode ──────────────────────────────────────────
        let (mode, color2, grad_cos, grad_sin) = match &model.pattern {
            HatchPattern::Solid => (1u32, [0.0f32; 4], 0.0f32, 0.0f32),
            HatchPattern::Pattern(_) => (0u32, [0.0f32; 4], 0.0f32, 0.0f32),
            HatchPattern::Gradient { angle_deg, color2, .. } => {
                let r = angle_deg.to_radians();
                (2u32, *color2, r.cos(), r.sin())
            }
        };

        // ── Bounding box ─────────────────────────────────────────────────
        let plane = model.fill_plane.unwrap_or(FillPlane {
            origin: [model.world_origin[0], model.world_origin[1], 0.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        });
        let gpu_boundary = model
            .fill_plane_boundary
            .as_deref()
            .unwrap_or(model.boundary.as_ref());
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        );
        for &[x, y] in gpu_boundary {
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }

        let max_spacing = match &model.pattern {
            HatchPattern::Pattern(families) => families
                .iter()
                .map(|f| perp_spacing(f).abs())
                .fold(0.0f32, f32::max),
            _ => 5.0,
        };
        let diag = ((max_x - min_x).powi(2) + (max_y - min_y).powi(2)).sqrt();
        let pad = if mode == 1 {
            0.0
        } else {
            (diag * 0.8 + max_spacing * 2.0 * model.scale).max(1.0)
        };
        let (x0, x1, y0, y1) = (
            min_x - pad,
            max_x + pad,
            min_y - pad,
            max_y + pad,
        );

        let quad = [
            HatchVertex {
                pos: [x0, y0, 0.0],
                _pad: 0.0,
            },
            HatchVertex {
                pos: [x1, y0, 0.0],
                _pad: 0.0,
            },
            HatchVertex {
                pos: [x1, y1, 0.0],
                _pad: 0.0,
            },
            HatchVertex {
                pos: [x0, y0, 0.0],
                _pad: 0.0,
            },
            HatchVertex {
                pos: [x1, y1, 0.0],
                _pad: 0.0,
            },
            HatchVertex {
                pos: [x0, y1, 0.0],
                _pad: 0.0,
            },
        ];
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.vbuf"),
            contents: bytemuck::cast_slice(&quad),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // ── Gradient: projection range (in snapped-local space) ──────────
        let (grad_min, grad_range) = if mode == 2 {
            let projs: Vec<f32> = gpu_boundary.iter()
                .filter(|v| v[0].is_finite() && v[1].is_finite())
                .map(|&[x, y]| x * grad_cos + y * grad_sin)
                .collect();
            if projs.is_empty() {
                (0.0, 1.0)
            } else {
                let proj_min = projs.iter().cloned().fold(f32::INFINITY, f32::min);
                let proj_max = projs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                (proj_min, (proj_max - proj_min).max(1e-4))
            }
        } else {
            (0.0, 1.0)
        };

        // ── HatchUniformData ─────────────────────────────────────────────
        let n = gpu_boundary.len().min(MAX_HATCH_BOUNDARY_VERTS);
        let plane_origin_high = plane.origin.map(|value| value as f32);
        let plane_origin_low: [f32; 3] = std::array::from_fn(|index| {
            (plane.origin[index] - plane_origin_high[index] as f64) as f32
        });
        let uniform_data = HatchUniformData {
            color: model.color,
            color2,
            mode,
            vertex_count: n as u32,
            angle_offset: model.angle_offset,
            scale: model.scale,
            grad_cos,
            grad_sin,
            grad_min,
            grad_range,
            origin: [0.0; 2],
            origin_low: [0.0; 2],
            draw_depth: 0.0,
            _pad: [0.0; 3],
            plane_origin_high: [
                plane_origin_high[0],
                plane_origin_high[1],
                plane_origin_high[2],
                0.0,
            ],
            plane_origin_low: [
                plane_origin_low[0],
                plane_origin_low[1],
                plane_origin_low[2],
                0.0,
            ],
            plane_x: [
                plane.x_axis[0] as f32,
                plane.x_axis[1] as f32,
                plane.x_axis[2] as f32,
                0.0,
            ],
            plane_y: [
                plane.y_axis[0] as f32,
                plane.y_axis[1] as f32,
                plane.y_axis[2] as f32,
                0.0,
            ],
        };

        // ── BoundaryData (in snapped-local space) ────────────────────────
        let mut boundary_data = BoundaryData {
            verts: [[0.0; 4]; MAX_HATCH_BOUNDARY_VERTS],
        };
        for (i, &[x, y]) in gpu_boundary.iter()
            .take(MAX_HATCH_BOUNDARY_VERTS)
            .enumerate()
        {
            if x.is_finite() && y.is_finite() {
                boundary_data.verts[i] = [x, y, 0.0, 0.0];
            } else {
                let s = crate::scene::model::hatch_model::GPU_BOUNDARY_SEP;
                boundary_data.verts[i] = [s, s, 0.0, 0.0];
            }
        }

        // ── FamilyBatchData ───────────────────────────────────────────────
        let family_batch = build_family_batch(&model.pattern);

        // ── GPU buffers ───────────────────────────────────────────────────
        let _uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.uniforms"),
            contents: bytemuck::bytes_of(&uniform_data),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let _boundary_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.boundary"),
            contents: bytemuck::bytes_of(&boundary_data),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let _family_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.families"),
            contents: bytemuck::bytes_of(&family_batch),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hatch.bind_group1"),
            layout: bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: _uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: _boundary_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: _family_buf.as_entire_binding(),
                },
            ],
        });

        // Boundary vertices are stored as f32 offsets from
        // `model.world_origin` (so f32 keeps precision at UTM-scale
        // WCS). The pipeline-level frustum cull (Phase 2.3
        // `aabb_offscreen`) projects through `view_proj`, which expects
        // local-space (world_offset-subtracted) coordinates — the same
        // space as `world_origin`. Add the origin back so `world_aabb`
        // is in that absolute local space rather than relative to the
        // anchor. (Sub-pixel LOD via `aabb_below_pixel` is unaffected:
        // it measures a projected diagonal, invariant under translation.)
        let base = model
            .render_instance
            .map_or([0.0; 3], |instance| instance.translation);
        let placements: Vec<WipeoutPlacement> = models
            .iter()
            .map(|model| {
                let translation = model
                    .render_instance
                    .map_or([0.0; 3], |instance| instance.translation);
                let delta = std::array::from_fn(|index| translation[index] - base[index]);
                let high = delta.map(|value| value as f32);
                WipeoutPlacement {
                    translation: high,
                    translation_low: std::array::from_fn(|index| {
                        (delta[index] - high[index] as f64) as f32
                    }),
                    draw_depth: model.draw_depth,
                    _pad: 0.0,
                }
            })
            .collect();
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wipeout.instances"),
            contents: bytemuck::cast_slice(&placements),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut world_bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for &[x, y] in model.boundary.iter() {
            if x.is_finite() && y.is_finite() {
                world_bounds[0] = world_bounds[0].min(x);
                world_bounds[1] = world_bounds[1].min(y);
                world_bounds[2] = world_bounds[2].max(x);
                world_bounds[3] = world_bounds[3].max(y);
            }
        }
        let tilted = plane.origin[2].abs() > f64::EPSILON
            || plane.x_axis[2].abs() > f64::EPSILON
            || plane.y_axis[2].abs() > f64::EPSILON;
        let world_aabb = if tilted {
            [f32::NAN; 4]
        } else if world_bounds[0].is_finite() && world_bounds[1].is_finite() {
            let ox = model.world_origin[0] as f32;
            let oy = model.world_origin[1] as f32;
            let mut aabb = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
            for placement in &placements {
                aabb[0] = aabb[0].min(world_bounds[0] + ox + placement.translation[0]);
                aabb[1] = aabb[1].min(world_bounds[1] + oy + placement.translation[1]);
                aabb[2] = aabb[2].max(world_bounds[2] + ox + placement.translation[0]);
                aabb[3] = aabb[3].max(world_bounds[3] + oy + placement.translation[1]);
            }
            aabb
        } else {
            world_bounds
        };

        Self {
            vertex_buffer,
            instance_buffer,
            instance_count: placements.len() as u32,
            bind_group,
            world_aabb,
            _uniform_buf,
            _boundary_buf,
            _family_buf,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Perpendicular spacing between adjacent parallel lines.
/// Pattern spacing perpendicular to adjacent parallel lines.
fn perp_spacing(f: &PatFamily) -> f32 {
    f.dy
}

/// Pack pattern families into the GPU batch struct.
fn build_family_batch(pattern: &HatchPattern) -> FamilyBatchData {
    let mut batch = FamilyBatchData {
        families: [LineFamilyGpu {
            cos_a: 0.0,
            sin_a: 0.0,
            x0: 0.0,
            y0: 0.0,
            dx: 0.0,
            dy: 0.0,
            perp_step: 0.0,
            along_step: 0.0,
            line_width: 0.0,
            period: 0.0,
            n_dashes: 0,
            dash_off: 0,
        }; MAX_FAMILIES],
        dash_values: [[0.0; 4]; 32],
        n_families: 0,
        _pad: [0; 3],
    };

    let HatchPattern::Pattern(families) = pattern else {
        return batch;
    };

    let mut dash_cursor: usize = 0;

    for (fi, family) in families.iter().take(MAX_FAMILIES).enumerate() {
        let angle_r = family.angle_deg.to_radians();
        let cos_a = angle_r.cos();
        let sin_a = angle_r.sin();
        // Pattern offsets are stored in the line-local frame.
        let perp_step = family.dy;
        let along_step = family.dx;
        let line_width = 0.0_f32; // unused: shader uses screen-space derivative for 1px lines

        // Dash pattern: collect up to available space.
        let n_avail = MAX_DASHES.saturating_sub(dash_cursor);
        let n_dashes = family.dashes.len().min(n_avail);
        let period: f32 = family.dashes[..n_dashes].iter().map(|d| d.abs()).sum();

        let dash_off = dash_cursor as u32;
        for &d in &family.dashes[..n_dashes] {
            batch.dash_values[dash_cursor / 4][dash_cursor % 4] = d;
            dash_cursor += 1;
        }

        batch.families[fi] = LineFamilyGpu {
            cos_a,
            sin_a,
            x0: family.x0,
            y0: family.y0,
            dx: family.dx,
            dy: family.dy,
            perp_step,
            along_step,
            line_width,
            period: if n_dashes > 0 { period } else { 0.0 },
            n_dashes: n_dashes as u32,
            dash_off,
        };
        batch.n_families += 1;

        if dash_cursor >= MAX_DASHES {
            break;
        }
    }

    batch
}
