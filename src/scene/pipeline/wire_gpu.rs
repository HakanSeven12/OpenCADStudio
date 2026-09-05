// Wire GPU buffers — instanced quad rendering for thick lines.
//
// Each segment [A→B] is one INSTANCE; the vertex shader expands a 6-vertex
// unit quad whose corners are derived from `@builtin(vertex_index)`. This
// cuts upload bandwidth by ~6.5× versus the old layout (which duplicated
// the segment payload across six vertex records).
//
// NaN sentinel: text glyphs pack multiple disconnected strokes into one
// WireModel, separated by [NaN, NaN, NaN] points. Segments where either
// endpoint contains NaN are silently skipped during emission.
//
// Instance layout (step_mode = Instance):
//   pos_a          [f32; 3]   — segment start (high half, world / offset-relative)
//   pos_a_low      [f32; 3]   — segment start low residual (double-single pair)
//   pos_b          [f32; 3]   — segment end (high)
//   pos_b_low      [f32; 3]   — segment end low residual
//   color          [u8;  4]   — RGBA, Unorm8x4 → vec4<f32> in shader
//   distance_a     f32        — arc-length at endpoint A
//   distance_b     f32        — arc-length at endpoint B
//   half_width     f32        — half line width in pixels
//   pattern_length f32        — dash pattern total length
//   pat0           [f32; 4]   — pattern elements 0-3
//   pat1           [f32; 4]   — pattern elements 4-7
//   draw_depth     f32        — normalized draw-order depth bias
// The high+low pair encodes the f64 source so the relative-to-eye shader
// stays precise at UTM-scale coordinates and after a cross-drawing paste.

use crate::scene::model::wire_model::WireModel;
use iced::wgpu;

/// Allocate a VERTEX buffer and fill it through the queue.
///
/// This used to map at creation to skip the staging copy. Mapping panics on
/// the invalid buffer an out-of-memory device returns, and that panic aborts
/// the process from inside iced's redraw, so the staging copy is the price of
/// a session that survives a failed allocation. Chunking keeps the transient
/// staging cost bounded. See `super::gpu_upload`.
fn instance_buffer<T: bytemuck::Pod>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    data: &[T],
) -> wgpu::Buffer {
    super::gpu_upload::upload_buffer(device, queue, label, data, wgpu::BufferUsages::VERTEX)
}

// ── Instance layout ───────────────────────────────────────────────────────

// ── Storage path: slim per-segment instance + shared constants ──────────────
//
// Every segment of a wire used to carry the wire's color / line-weight / dash
// pattern / draw-depth (~44 B) on each instance — re-fetched once per segment
// even though it's constant along the wire. On native we hoist those into a
// per-wire `WireConst` storage buffer indexed by `wire_id`, so the instance
// keeps only the per-segment data (endpoints + arc-length distances). Cuts the
// instance from 104 B to one 64-byte cache line and removes the redundant
// per-segment re-fetch of the shared constants. WebGL2 has no vertex-stage
// storage buffers, so the compatibility path keeps the self-contained fat
// instance.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WireInstance {
    pub pos_a: [f32; 3],
    pub pos_a_low: [f32; 3],
    pub pos_b: [f32; 3],
    pub pos_b_low: [f32; 3],
    pub distance_a: f32,
    pub distance_b: f32,
    /// Index into the per-wire `WireConst` storage buffer (group 1).
    pub wire_id: u32,
    /// Endpoint width / the per-wire maximum width, normalized by the vertex
    /// fetch unit. `[0, 0]` means use the constant width. Ratios retain the
    /// full f32 world-width scale in `WireConst` while making every instance
    /// exactly one 64-byte cache line.
    pub taper_ratio: [u16; 2],
}

impl WireInstance {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        // Must match `InstanceIn` in wire_indexed.wgsl.
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute { offset: std::mem::offset_of!(WireInstance, pos_a) as u64,      shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(WireInstance, pos_b) as u64,      shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(WireInstance, pos_a_low) as u64,  shader_location: 2, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(WireInstance, pos_b_low) as u64,  shader_location: 3, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(WireInstance, distance_a) as u64, shader_location: 4, format: wgpu::VertexFormat::Float32   },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(WireInstance, distance_b) as u64, shader_location: 5, format: wgpu::VertexFormat::Float32   },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(WireInstance, wire_id) as u64,    shader_location: 6, format: wgpu::VertexFormat::Uint32    },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(WireInstance, taper_ratio) as u64, shader_location: 7, format: wgpu::VertexFormat::Unorm16x2 },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<WireInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

/// Per-wire constants shared by every segment of a wire (storage path). std430
/// layout: three vec4, eight scalars, then three vec4 = 128 B, matching `WireConst` in
/// wire_indexed.wgsl. `align_end` / `align_total` carry the "A"-type endpoint
/// alignment (see `wire_distances`); 0.0 total = no alignment.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WireConst {
    pub color: [f32; 4],
    pub pat0: [f32; 4],
    pub pat1: [f32; 4],
    pub half_width: f32,
    pub pattern_length: f32,
    pub draw_depth: f32,
    pub align_end: f32,
    pub align_total: f32,
    /// World-space half-width for a wide-polyline band. `0.0` = a normal wire
    /// (uses `half_width`, screen pixels). Non-zero = the vertex shader expands
    /// the quad by `world_half_width / world_per_pixel` so the band tracks zoom
    /// in drawing units.
    pub world_half_width: f32,
    pub _pad1: f32,
    pub _pad2: f32,
    /// Point-marker origin as a double-single pair. `marker_normal_scale.w`
    /// stores the viewport-height percentage; zero disables marker scaling.
    pub marker_origin_high: [f32; 4],
    pub marker_origin_low: [f32; 4],
    pub marker_normal_scale: [f32; 4],
}

impl WireConst {
    /// Bind-group layout for the per-wire storage buffer (group 1 of the wire /
    /// xray pipelines). Read-only storage, visible to the vertex stage.
    pub fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wire_const.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        })
    }
}

// ── Packed compatibility instance (no vertex-stage storage) ────────────────
//
// Selected at runtime for devices whose storage-buffer limits are insufficient,
// or when --compat-renderer is set.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PackedWireInstance {
    pub pos_a: [f32; 3],
    pub pos_a_low: [f32; 3],
    pub pos_b: [f32; 3],
    pub pos_b_low: [f32; 3],
    /// RGBA packed as `Unorm8x4` — the vertex shader receives a `vec4<f32>`
    /// in [0, 1] after the GPU does the conversion. 8 bits per channel is
    /// indistinguishable from f32 at 8-bit display output.
    pub color: [u8; 4],
    pub distance_a: f32,
    pub distance_b: f32,
    pub half_width: f32,
    pub pattern_length: f32,
    pub pat0: [f32; 4],
    pub pat1: [f32; 4],
    /// Normalized draw-order depth in (0,1); applied as a small clip-z bias
    /// in the shader so this wire orders against other entity types.
    pub draw_depth: f32,
    /// "A"-type endpoint alignment (see `wire_distances`): the end-dash length
    /// and the total wire length. `align_total == 0.0` = not aligned.
    pub align_end: f32,
    pub align_total: f32,
    /// World-space half-width for a wide-polyline band (see `WireConst`). `0.0`
    /// = a normal wire (uses `half_width`, screen pixels).
    pub world_half_width: f32,
    /// Per-endpoint world half-width for a tapered band (0 = use the constant
    /// `world_half_width`). The shader interpolates across the segment.
    pub world_hw_a: f32,
    pub world_hw_b: f32,
    pub marker_origin_high: [f32; 4],
    pub marker_origin_low: [f32; 4],
    pub marker_normal_scale: [f32; 4],
}

impl PackedWireInstance {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        // Offsets come from the struct layout (must match the shader location
        // indices in wire.wgsl). Scalars ride in PACKED vec4/vec2 attributes —
        // WebGL2 / WebGPU cap vertex attributes at 16 and the one-scalar-per-
        // location layout had grown to 17, so the pipeline failed to build and
        // the web viewport drew no lines at all (#414). The struct fields are
        // laid out so each packed group is contiguous.
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, pos_a) as u64,          shader_location: 0,  format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, pos_b) as u64,          shader_location: 1,  format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, color) as u64,          shader_location: 2,  format: wgpu::VertexFormat::Unorm8x4  },
            // dists = (distance_a, distance_b, half_width, pattern_length)
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, distance_a) as u64,     shader_location: 3,  format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, pat0) as u64,           shader_location: 4,  format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, pat1) as u64,           shader_location: 5,  format: wgpu::VertexFormat::Float32x4 },
            // misc = (draw_depth, align_end, align_total, world_half_width)
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, draw_depth) as u64,     shader_location: 6,  format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, pos_a_low) as u64,      shader_location: 7,  format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, pos_b_low) as u64,      shader_location: 8,  format: wgpu::VertexFormat::Float32x3 },
            // taper = (world_hw_a, world_hw_b)
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, world_hw_a) as u64,     shader_location: 9,  format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, marker_origin_high) as u64, shader_location: 10, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, marker_origin_low) as u64, shader_location: 11, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(PackedWireInstance, marker_normal_scale) as u64, shader_location: 12, format: wgpu::VertexFormat::Float32x4 },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<PackedWireInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

/// Wire and hatch pipelines switch together: the fast path uses storage
/// buffers; the compatibility path carries wire constants in packed vertex
/// attributes and hatch data in a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WirePipelineMode {
    IndexedStorage,
    Packed,
}

fn select_pipeline(
    capabilities: super::device_capabilities::DeviceCapabilities,
    forced: bool,
) -> WirePipelineMode {
    if forced || !capabilities.supports_wire_storage() {
        WirePipelineMode::Packed
    } else {
        WirePipelineMode::IndexedStorage
    }
}

impl WirePipelineMode {
    pub fn select(
        capabilities: super::device_capabilities::DeviceCapabilities,
        forced: bool,
    ) -> Self {
        select_pipeline(capabilities, forced)
    }

    pub fn uses_storage(self) -> bool {
        match self {
            Self::IndexedStorage => true,
            Self::Packed => false,
        }
    }

    pub fn layout<'a>(self) -> wgpu::VertexBufferLayout<'a> {
        match self {
            Self::IndexedStorage => WireInstance::layout(),
            Self::Packed => PackedWireInstance::layout(),
        }
    }
}

// ── GPU handle ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct WireGpu {
    pub instance_buffer: wgpu::Buffer,
    /// First instance in a shared arena buffer. Standalone buffers start at 0.
    pub first_instance: u32,
    pub instance_count: u32,
    /// `true` when the source `WireModel` also carries `fill_tris`
    /// (i.e. it is a 3D mesh face — PolyfaceMesh / PolygonMesh — whose
    /// outline lives in `points`). The wire pass skips these instances
    /// in shaded modes so the surface reads as a clean solid; pure
    /// wireframe / HiddenLine / *WithEdges modes draw them.
    pub is_3d_mesh_edge: bool,
    /// Per-wire constants for this chunk (group 1); packed instances carry them inline.
    pub const_bind_group: Option<std::sync::Arc<wgpu::BindGroup>>,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BlockWireVertex {
    pub pos_a: [f32; 3],
    pub pos_a_low: [f32; 3],
    pub pos_b: [f32; 3],
    pub pos_b_low: [f32; 3],
    pub distances: [f32; 2],
    pub taper_ratio: [u16; 2],
}

impl BlockWireVertex {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireVertex, pos_a) as u64,       shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireVertex, pos_b) as u64,       shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireVertex, pos_a_low) as u64,   shader_location: 2, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireVertex, pos_b_low) as u64,   shader_location: 3, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireVertex, distances) as u64,   shader_location: 4, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireVertex, taper_ratio) as u64, shader_location: 5, format: wgpu::VertexFormat::Unorm16x2 },
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
pub struct BlockWireInstance {
    pub translation: [f32; 3],
    pub translation_low: [f32; 3],
    pub depth: [f32; 2],
}

impl BlockWireInstance {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireInstance, translation) as u64,     shader_location: 6, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireInstance, translation_low) as u64, shader_location: 7, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockWireInstance, depth) as u64,           shader_location: 8, format: wgpu::VertexFormat::Float32x2 },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

#[derive(Clone)]
pub struct BlockWireGpu {
    /// Packed mode only; `None` in storage mode, where the segments reach the
    /// shader through `const_bind_group` instead.
    pub vertex_buffer: Option<wgpu::Buffer>,
    pub instance_buffer: wgpu::Buffer,
    pub vertex_count: u32,
    pub instance_count: u32,
    pub is_3d_mesh_edge: bool,
    pub const_bind_group: std::sync::Arc<wgpu::BindGroup>,
}

/// Binding 0 is the definition's constants. In storage mode binding 1 carries
/// that chunk's segments, which is why the bind group is per-chunk there and
/// shared across chunks in packed mode.
pub fn block_const_bind_group_layout(
    device: &wgpu::Device,
    uses_storage: bool,
) -> wgpu::BindGroupLayout {
    let mut entries = vec![wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }];
    if uses_storage {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("block_wire_const.bgl"),
        entries: &entries,
    })
}

/// Expand one `WireModel` into its per-segment instance stream (1 instance per
/// finite segment). Pulled out so both the single-wire and batched paths share
/// the same emission logic, and so the batched path can `par_iter` across
/// wires on cold open.
fn pack_color(color: [f32; 4]) -> [u8; 4] {
    [
        (color[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (color[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (color[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (color[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
    ]
}

/// Cumulative arc-length per point (NaN-break aware) plus the `"A"`-type
/// alignment pair `(align_end, align_total)`. Shared by the wasm and native
/// emission paths.
///
/// AutoCAD-style linetypes are implicitly `A` (aligned): a dashed line must
/// begin AND end on a solid dash, keeping the interior dashes at their nominal
/// length and stretching/shrinking only the two end dashes symmetrically to
/// absorb the leftover (so parallel lines share an identical interior). We
/// express that on the GPU by handing the shader the total wire length
/// (`align_total`) and the end-dash length (`align_end`); the pattern walk then
/// forces the two end regions lit and phases the interior to resume right after
/// the first dash. `align_total == 0.0` disables it and the shader falls back to
/// the legacy centred repeating pattern.
///
/// Alignment applies only to a single continuous run (`!has_break`) whose
/// pattern begins with a dash. NaN-separated (plinegen=false) polylines and
/// non-dash-first patterns keep the legacy centred phase.
fn wire_distances(wire: &WireModel) -> (Vec<f32>, f32, f32) {
    let n = wire.points.len();
    let explicit = wire.pattern_stations.len() >= n + 1;
    let (mut dists, has_break, total) = if explicit {
        (
            wire.pattern_stations[..n].to_vec(),
            !wire.plinegen,
            wire.pattern_stations[n],
        )
    } else {
        let mut dists = vec![0.0_f32; n];
        let mut has_break = false;
        // Accumulate arc-length in f64 from double-single deltas (high + low).
        let mut acc = 0.0_f64;
        for i in 1..n {
            let p = wire.points[i - 1];
            let q = wire.points[i];
            if !p[0].is_finite() || !q[0].is_finite() {
                has_break = true;
                if !wire.plinegen && !p[0].is_finite() && q[0].is_finite() {
                    acc = 0.0;
                }
                dists[i] = acc as f32;
            } else {
                let pl = wire.points_low.get(i - 1).copied().unwrap_or([0.0; 3]);
                let ql = wire.points_low.get(i).copied().unwrap_or([0.0; 3]);
                let dx = (q[0] as f64 - p[0] as f64) + (ql[0] as f64 - pl[0] as f64);
                let dy = (q[1] as f64 - p[1] as f64) + (ql[1] as f64 - pl[1] as f64);
                let dz = (q[2] as f64 - p[2] as f64) + (ql[2] as f64 - pl[2] as f64);
                acc += (dx * dx + dy * dy + dz * dz).sqrt();
                dists[i] = acc as f32;
            }
        }
        let total = dists.last().copied().unwrap_or(0.0);
        (dists, has_break, total)
    };

    let pat_len = wire.pattern_length;
    if pat_len <= 1e-6 || has_break || n < 2 {
        return (dists, 0.0, 0.0);
    }
    if total <= 1e-6 {
        return (dists, 0.0, 0.0);
    }

    // DGN line styles draw the pattern from the START vertex with continuous
    // phase and are NOT end-aligned. The raw arc-length distances already put
    // dist 0 at the first vertex, so a dash-first pattern begins a dash exactly
    // there. Return before the A-type / centring logic that standard linetypes
    // use (see `WireModel::dash_from_start`).
    if wire.dash_from_start {
        return (dists, 0.0, 0.0);
    }

    // Shared "A"-type for MLINE elements: the caller fixes the begin/end
    // solid-dash length (`dash_align_end`, derived once from the multiline's
    // centre-line length) so every parallel element runs the SAME interior phase
    // — the shader's interior walk depends on `align_end`, not on the wire's own
    // length — while `align_total` stays this wire's own length, so each element
    // still ends on a dash at its own endpoint.
    if let Some(ae) = wire.dash_align_end {
        if total <= pat_len {
            // Shorter than one full period → solid (matches the per-wire path).
            return (dists, total, total);
        }
        return (dists, ae.clamp(1e-4, total * 0.5), total);
    }

    // Align only a proper alternating pattern that BEGINS with a dash followed
    // by a gap — every standard linetype does (DASHED/DASHDOT/CENTER/HIDDEN/…).
    // Gap-first, dot-first, single-element, or consecutive-dash patterns keep
    // the legacy centred phase: the A-type interior-resume assumes the element
    // after the leading dash is a gap, and force-lighting an end dash on a
    // non-dash-start would paint over a leading blank.
    if wire.pattern[0] > 0.0 && wire.pattern[1] < 0.0 {
        let a = wire.pattern[0];
        let p = pat_len;
        if total <= p {
            // Shorter than one full pattern period → drawn as a single solid
            // dash spanning the whole line (aligned linetypes can't fit a
            // dash-gap-dash in less than one period).
            return (dists, total, total);
        }
        // "A" alignment for a dash-first pattern of period P laid out as
        //   [D] [gap] ([dash] [gap])*(k-1) [D]
        // gives  L = 2D + (k-1)a + k(P-a)  =>  D = (L - k*P + a) / 2.
        // Pick the interior period count k so the end dash D stays near nominal a.
        let mut k = ((total - a) / p).round().max(1.0);
        let mut d_end = (total - k * p + a) * 0.5;
        if d_end <= 1e-4 {
            // End dash underflowed (period ≫ first dash); drop one period so the
            // ends stay visible.
            k = (k - 1.0).max(0.0);
            d_end = (total - k * p + a) * 0.5;
        }
        let d_end = d_end.clamp(1e-4, total * 0.5);
        return (dists, d_end, total);
    }

    // Legacy centred phase for non-aligned patterns (behaviour unchanged from
    // before A-type). The shader reads phase as `dist % pattern_length`, so a
    // constant offset shifts it; place the wire midpoint at the first dash's
    // centre so the two ends stay symmetric.
    let first_dash = wire
        .pattern
        .iter()
        .copied()
        .find(|&v| v > 0.0)
        .unwrap_or_else(|| wire.pattern[0].abs());
    let offset = first_dash * 0.5 + total * 0.5;
    for d in dists.iter_mut() {
        *d += offset;
    }
    (dists, 0.0, 0.0)
}

#[inline]
fn finite3(p: [f32; 3]) -> bool {
    p[0].is_finite() && p[1].is_finite() && p[2].is_finite()
}

fn marker_metadata(wire: &WireModel) -> ([f32; 4], [f32; 4], [f32; 4]) {
    let Some(marker) = wire.point_marker else {
        return ([0.0; 4], [0.0; 4], [0.0; 4]);
    };
    let origin = marker.origin;
    let (hx, lx) = WireModel::split_ds(origin.x);
    let (hy, ly) = WireModel::split_ds(origin.y);
    let (hz, lz) = WireModel::split_ds(origin.z);
    let normal = marker.normal.as_vec3().normalize_or(glam::Vec3::Z);
    (
        [hx, hy, hz, 0.0],
        [lx, ly, lz, 0.0],
        [normal.x, normal.y, normal.z, marker.viewport_percent],
    )
}

/// Emit packed per-segment instances (each carries the wire's constants).
pub(crate) fn emit_wire_packed(
    wire: &WireModel,
    color: [f32; 4],
    draw_depth: f32,
) -> Vec<PackedWireInstance> {
    let color_u8 = pack_color(color);
    let pat0 = [wire.pattern[0], wire.pattern[1], wire.pattern[2], wire.pattern[3]];
    let pat1 = [wire.pattern[4], wire.pattern[5], wire.pattern[6], wire.pattern[7]];
    let half_width = wire.line_weight_px * 0.5;
    let n = wire.points.len();
    let seg_count = n.saturating_sub(1);
    if seg_count == 0 {
        return Vec::new();
    }
    let (dists, align_end, align_total) = wire_distances(wire);
    let (marker_origin_high, marker_origin_low, marker_normal_scale) = marker_metadata(wire);
    let low = |i: usize| -> [f32; 3] { wire.points_low.get(i).copied().unwrap_or([0.0; 3]) };
    let tw = |i: usize| -> f32 { wire.taper_widths.get(i).copied().unwrap_or(0.0) * 0.5 };
    let mut instances: Vec<PackedWireInstance> = Vec::with_capacity(seg_count);
    for i in 0..seg_count {
        let a = wire.points[i];
        let b = wire.points[i + 1];
        if !finite3(a) || !finite3(b) {
            continue;
        }
        instances.push(PackedWireInstance {
            pos_a: a,
            pos_a_low: low(i),
            pos_b: b,
            pos_b_low: low(i + 1),
            color: color_u8,
            distance_a: dists[i],
            distance_b: dists[i + 1],
            half_width,
            pattern_length: wire.pattern_length,
            pat0,
            pat1,
            draw_depth,
            align_end,
            align_total,
            world_half_width: wire.world_width * 0.5,
            world_hw_a: tw(i),
            world_hw_b: tw(i + 1),
            marker_origin_high,
            marker_origin_low,
            marker_normal_scale,
        });
    }
    instances
}

/// Storage path: emit slim instances (positions + distances + `wire_id`)
/// plus the one `WireConst` record every segment of this wire shares.
pub(crate) fn emit_wire_native(
    wire: &WireModel,
    wire_id: u32,
    color: [f32; 4],
    draw_depth: f32,
) -> (Vec<WireInstance>, WireConst) {
    let (dists, align_end, align_total) = wire_distances(wire);
    let (marker_origin_high, marker_origin_low, marker_normal_scale) = marker_metadata(wire);
    let cst = WireConst {
        color,
        pat0: [wire.pattern[0], wire.pattern[1], wire.pattern[2], wire.pattern[3]],
        pat1: [wire.pattern[4], wire.pattern[5], wire.pattern[6], wire.pattern[7]],
        half_width: wire.line_weight_px * 0.5,
        pattern_length: wire.pattern_length,
        draw_depth,
        align_end,
        align_total,
        world_half_width: wire.world_width * 0.5,
        _pad1: 0.0,
        _pad2: 0.0,
        marker_origin_high,
        marker_origin_low,
        marker_normal_scale,
    };
    let n = wire.points.len();
    let seg_count = n.saturating_sub(1);
    if seg_count == 0 {
        return (Vec::new(), cst);
    }
    let low = |i: usize| -> [f32; 3] { wire.points_low.get(i).copied().unwrap_or([0.0; 3]) };
    // Store an endpoint/max-width ratio. The shared f32 maximum keeps drawing
    // units and range out of the packed field; UNORM16 contributes only a
    // relative error below 1/65535. Preserve zero as the existing constant
    // width fallback sentinel.
    let taper_ratio = |i: usize| -> u16 {
        let width = wire.taper_widths.get(i).copied().unwrap_or(0.0);
        if width <= 0.0 || wire.world_width <= 0.0 {
            0
        } else {
            ((width / wire.world_width).clamp(0.0, 1.0) * u16::MAX as f32)
                .round()
                .max(1.0) as u16
        }
    };
    let mut instances: Vec<WireInstance> = Vec::with_capacity(seg_count);
    for i in 0..seg_count {
        let a = wire.points[i];
        let b = wire.points[i + 1];
        if !finite3(a) || !finite3(b) {
            continue;
        }
        instances.push(WireInstance {
            pos_a: a,
            pos_a_low: low(i),
            pos_b: b,
            pos_b_low: low(i + 1),
            distance_a: dists[i],
            distance_b: dists[i + 1],
            wire_id,
            taper_ratio: [taper_ratio(i), taper_ratio(i + 1)],
        });
    }
    (instances, cst)
}

/// Looks up a wire's draw-order depth from the per-entity map using the
/// handle encoded in its `name`. Falls back to 0.0 (transient / preview
/// wires that carry no document handle). A wire carrying a block-local
/// `depth_override` (a wide-polyline band inside a block) composes it into
/// the insert's own sub-range so the band orders against its block siblings.
pub(crate) fn wire_draw_depth(
    wire: &WireModel,
    depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> f32 {
    let base = wire
        .name
        .parse::<u64>()
        .ok()
        .and_then(|h| depth_map.get(&h).copied());
    match (base, wire.depth_override) {
        (Some([d, half]), Some(local)) => d + local * half,
        (Some([d, _]), None) => d,
        (None, _) => 0.0,
    }
}

/// Cached block geometry keyed by source, edge mode, colour and base translation.
/// Vertices use the first instance's world coordinates, so changing that base
/// must invalidate the geometry as well as rebuild the relative placements.
pub type BlockGeometryCache = rustc_hash::FxHashMap<
    BlockGeometryKey,
    std::sync::Arc<Vec<BlockGeometryChunk>>,
>;

/// How this device draws block wires. The layout and the mode are decided
/// together when the pipeline is built and are never chosen independently, so
/// they travel as one argument.
#[derive(Clone, Copy)]
pub struct BlockWireTarget<'a> {
    pub const_bgl: &'a wgpu::BindGroupLayout,
    pub mode: WirePipelineMode,
}

/// `(source_id, mesh_edge, colour bits, base translation bits)`.
pub type BlockGeometryKey = (u64, bool, [u32; 4], [u64; 3]);

/// One drawable slice of a block definition's geometry.
///
/// The two pipeline modes put the segments in different places, so the chunk
/// carries whichever the mode produced:
///
/// * packed — `vertex_buffer` holds six copies of each segment, one per corner
///   of its quad, and `bind_group` is the definition's constants alone;
/// * storage — the segments are in a read-only storage buffer inside
///   `bind_group`, one copy each, and `vertex_buffer` is `None`.
///
/// `vertex_count` is six per segment either way: in storage mode the shader
/// divides `vertex_index` by six to find its segment.
pub struct BlockGeometryChunk {
    pub vertex_buffer: Option<wgpu::Buffer>,
    pub vertex_count: u32,
    pub bind_group: std::sync::Arc<wgpu::BindGroup>,
}

impl BlockWireGpu {
    /// Identity of the geometry this batch draws, comparable across pipeline
    /// modes.
    ///
    /// Packed mode keeps the segments in `vertex_buffer`; storage mode keeps
    /// them inside `const_bind_group`, which is per-chunk there, and leaves
    /// `vertex_buffer` `None`. Both are built on the same cache miss and
    /// reused together on a hit, so the pair answers "is this the same
    /// geometry as before?" in either mode — which `vertex_buffer` alone
    /// cannot once it is always `None`.
    pub fn geometry_id(&self) -> (Option<&wgpu::Buffer>, &wgpu::BindGroup) {
        (self.vertex_buffer.as_ref(), self.const_bind_group.as_ref())
    }

    pub fn from_wires(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[&WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
        color_override: Option<[f32; 4]>,
        target: BlockWireTarget<'_>,
        mut cache: Option<&mut BlockGeometryCache>,
    ) -> Vec<Self> {
        let BlockWireTarget { const_bgl, mode } = target;
        let mut slots: rustc_hash::FxHashMap<(u64, bool), usize> =
            rustc_hash::FxHashMap::default();
        let mut groups: Vec<(bool, Vec<&WireModel>)> = Vec::new();
        for &wire in wires {
            if color_override.is_none() && !wire.display_visible {
                continue;
            }
            let Some(instance) = wire.render_instance else {
                continue;
            };
            if wire.points.len() < 2 {
                continue;
            }
            let mesh_edge = color_override.is_none() && wire.fill_is_3d;
            let key = (instance.source_id, mesh_edge);
            let slot = *slots.entry(key).or_insert_with(|| {
                let slot = groups.len();
                groups.push((mesh_edge, Vec::new()));
                slot
            });
            groups[slot].1.push(wire);
        }

        let mut out = Vec::new();
        let mut live_keys: rustc_hash::FxHashSet<BlockGeometryKey> =
            rustc_hash::FxHashSet::default();
        // Geometry bytes actually sent to the device this call, so the storage
        // path's saving is visible rather than assumed.
        let mut uploaded_bytes = 0usize;
        let mut uploaded_segments = 0usize;
        for (mesh_edge, group) in groups {
            let Some(&source) = group.first() else {
                continue;
            };
            let Some(base) = source.render_instance else {
                continue;
            };
            let color = color_override.unwrap_or(source.color);
            let cache_key = (
                base.source_id,
                mesh_edge,
                color.map(f32::to_bits),
                base.translation.map(f64::to_bits),
            );
            live_keys.insert(cache_key);
            // Reuse this definition's geometry when it is already on the GPU.
            if let Some(chunks) = cache
                .as_deref()
                .and_then(|c| c.get(&cache_key))
                .map(std::sync::Arc::clone)
            {
                let instances = block_instances(&group, base, mesh_edge, depth_map);
                push_block_batches(&mut out, device, queue, &chunks, &instances, mesh_edge);
                continue;
            }
            let (segments, mut constant) = emit_wire_native(source, 0, color, 0.0);
            if segments.is_empty() {
                continue;
            }
            constant.draw_depth = 0.0;
            let records: Vec<BlockWireVertex> = segments
                .iter()
                .map(|segment| BlockWireVertex {
                    pos_a: segment.pos_a,
                    pos_a_low: segment.pos_a_low,
                    pos_b: segment.pos_b,
                    pos_b_low: segment.pos_b_low,
                    distances: [segment.distance_a, segment.distance_b],
                    taper_ratio: segment.taper_ratio,
                })
                .collect();
            // Queue uploads avoid mapping a failed allocation.
            let const_buffer = super::gpu_upload::upload_buffer(
                device,
                queue,
                "block_wire.const",
                std::slice::from_ref(&constant),
                wgpu::BufferUsages::UNIFORM,
            );
            // Bound uploads so a block-heavy drawing cannot ask a low-VRAM
            // device for one multi-hundred-MB buffer.
            let chunks: Vec<BlockGeometryChunk> = if mode.uses_storage() {
                // One record per segment. A storage chunk is bounded by the
                // binding size as well as the buffer budget, and needs no
                // grouping: the shader indexes segments directly.
                let max_records =
                    super::gpu_budget::max_storage_elements::<BlockWireVertex>(device);
                records
                    .chunks(max_records)
                    .map(|chunk| {
                        let segments = super::gpu_upload::upload_buffer(
                            device,
                            queue,
                            "block_wire.segments",
                            chunk,
                            wgpu::BufferUsages::STORAGE,
                        );
                        BlockGeometryChunk {
                            vertex_buffer: None,
                            vertex_count: chunk.len() as u32 * 6,
                            bind_group: std::sync::Arc::new(device.create_bind_group(
                                &wgpu::BindGroupDescriptor {
                                    label: Some("block_wire.const.bg"),
                                    layout: const_bgl,
                                    entries: &[
                                        wgpu::BindGroupEntry {
                                            binding: 0,
                                            resource: const_buffer.as_entire_binding(),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 1,
                                            resource: segments.as_entire_binding(),
                                        },
                                    ],
                                },
                            )),
                        }
                    })
                    .collect()
            } else {
                // Six copies per segment, one per corner of its quad, without
                // splitting a segment's six vertices across two buffers.
                let mut vertices = Vec::with_capacity(records.len() * 6);
                for record in &records {
                    vertices.extend_from_slice(&[*record; 6]);
                }
                let max_verts =
                    super::gpu_budget::max_elements_grouped::<BlockWireVertex>(device, 6);
                let shared = std::sync::Arc::new(device.create_bind_group(
                    &wgpu::BindGroupDescriptor {
                        label: Some("block_wire.const.bg"),
                        layout: const_bgl,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: const_buffer.as_entire_binding(),
                        }],
                    },
                ));
                vertices
                    .chunks(max_verts)
                    .map(|chunk| BlockGeometryChunk {
                        vertex_buffer: Some(super::gpu_upload::upload_buffer(
                            device,
                            queue,
                            "block_wire.vertices",
                            chunk,
                            wgpu::BufferUsages::VERTEX,
                        )),
                        vertex_count: chunk.len() as u32,
                        bind_group: std::sync::Arc::clone(&shared),
                    })
                    .collect()
            };
            uploaded_segments += records.len();
            uploaded_bytes += records.len()
                * std::mem::size_of::<BlockWireVertex>()
                * if mode.uses_storage() { 1 } else { 6 };
            let chunks = std::sync::Arc::new(chunks);
            if let Some(cache) = cache.as_deref_mut() {
                cache.insert(cache_key, std::sync::Arc::clone(&chunks));
            }
            let instances = block_instances(&group, base, mesh_edge, depth_map);
            push_block_batches(&mut out, device, queue, &chunks, &instances, mesh_edge);
        }
        // Release geometry that this build no longer uses.
        if let Some(cache) = cache {
            cache.retain(|key, _| live_keys.contains(key));
        }
        if crate::perf::enabled() && uploaded_segments > 0 {
            crate::perf_record!(
                "[perf] block-wire-geometry mode={} definitions={} segments={} bytes={} \
bytes_if_packed={}",
                if mode.uses_storage() { "storage" } else { "packed" },
                live_keys.len(),
                uploaded_segments,
                uploaded_bytes,
                uploaded_segments * std::mem::size_of::<BlockWireVertex>() * 6,
            );
        }
        out
    }
}

/// Placements and draw depths relative to the group's first instance.
fn block_instances(
    group: &[&WireModel],
    base: crate::scene::model::instance_model::RenderInstance,
    mesh_edge: bool,
    depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Vec<BlockWireInstance> {
    group
        .iter()
        .filter_map(|wire| {
            let instance = wire.render_instance?;
            let delta = [
                instance.translation[0] - base.translation[0],
                instance.translation[1] - base.translation[1],
                instance.translation[2] - base.translation[2],
            ];
            let high = delta.map(|value| value as f32);
            Some(BlockWireInstance {
                translation: high,
                translation_low: [
                    (delta[0] - high[0] as f64) as f32,
                    (delta[1] - high[1] as f64) as f32,
                    (delta[2] - high[2] as f64) as f32,
                ],
                depth: [
                    if mesh_edge {
                        0.0
                    } else {
                        wire_draw_depth(wire, depth_map)
                    },
                    0.0,
                ],
            })
        })
        .collect()
}

/// One batch per (geometry chunk × instance chunk). Instance buffers are built
/// once and shared across geometry chunks rather than re-uploaded per chunk.
fn push_block_batches(
    out: &mut Vec<BlockWireGpu>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    chunks: &[BlockGeometryChunk],
    instances: &[BlockWireInstance],
    mesh_edge: bool,
) {
    let max_instances = super::gpu_budget::max_elements::<BlockWireInstance>(device);
    let instance_chunks: Vec<(wgpu::Buffer, u32)> = instances
        .chunks(max_instances)
        .map(|chunk| {
            (
                instance_buffer(device, queue, "block_wire.instances", chunk),
                chunk.len() as u32,
            )
        })
        .collect();
    for chunk in chunks {
        for (instance_buffer, instance_count) in &instance_chunks {
            out.push(BlockWireGpu {
                vertex_buffer: chunk.vertex_buffer.clone(),
                instance_buffer: instance_buffer.clone(),
                vertex_count: chunk.vertex_count,
                instance_count: *instance_count,
                is_3d_mesh_edge: mesh_edge,
                const_bind_group: std::sync::Arc::clone(&chunk.bind_group),
            });
        }
    }
}

/// Upload one constant chunk; its instances use chunk-local wire IDs.
fn build_const_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bgl: &wgpu::BindGroupLayout,
    consts: &[WireConst],
) -> std::sync::Arc<wgpu::BindGroup> {
    // wgpu rejects zero-sized buffers; pad with one zeroed record when empty.
    let one = [<WireConst as bytemuck::Zeroable>::zeroed()];
    let data: &[WireConst] = if consts.is_empty() { &one } else { consts };
    let buf = super::gpu_upload::upload_buffer(
        device,
        queue,
        "wire_const.buf",
        data,
        wgpu::BufferUsages::STORAGE,
    );
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("wire_const.bg"),
        layout: bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buf.as_entire_binding(),
        }],
    });
    std::sync::Arc::new(bg)
}

impl WireGpu {
    fn upload_native(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        const_bgl: &wgpu::BindGroupLayout,
        mut instances: Vec<WireInstance>,
        consts: &[WireConst],
        mesh_edge: bool,
        label: &str,
    ) -> Vec<Self> {
        let max_consts = super::gpu_budget::max_storage_elements::<WireConst>(device);
        let max_instances = super::gpu_budget::max_elements::<WireInstance>(device);
        let mut out = Vec::new();
        let mut remaining = instances.as_mut_slice();
        for (group, constants) in consts.chunks(max_consts).enumerate() {
            let base = group * max_consts;
            let count =
                remaining.partition_point(|item| (item.wire_id as usize) < base + constants.len());
            let (items, rest) = remaining.split_at_mut(count);
            remaining = rest;
            if items.is_empty() {
                continue;
            }
            for item in items.iter_mut() {
                item.wire_id -= base as u32;
            }
            debug_assert!(items
                .iter()
                .all(|item| (item.wire_id as usize) < constants.len()));
            let bind_group = build_const_bind_group(device, queue, const_bgl, constants);
            for chunk in items.chunks(max_instances) {
                out.push(Self {
                    instance_buffer: instance_buffer(device, queue, label, chunk),
                    first_instance: 0,
                    instance_count: chunk.len() as u32,
                    is_3d_mesh_edge: mesh_edge,
                    const_bind_group: Some(bind_group.clone()),
                });
            }
        }
        out
    }

    /// Build a small selection/hover overlay from borrowed resident wires while
    /// overriding their colour. Avoids deep-cloning every point/text/fill array
    /// of a large selected polyline or block before packing the overlay.
    pub fn from_highlight_refs(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[&WireModel],
        color: [f32; 4],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
        const_bgl: Option<&wgpu::BindGroupLayout>,
    ) -> Vec<Self> {
        if let Some(const_bgl) = const_bgl {
            use crate::par::prelude::*;
            let per: Vec<(Vec<WireInstance>, WireConst)> = wires
                .par_iter()
                .enumerate()
                .map(|(idx, &wire)| {
                    emit_wire_native(wire, idx as u32, color, wire_draw_depth(wire, depth_map))
                })
                .collect();
            let mut instances =
                Vec::with_capacity(per.iter().map(|(items, _)| items.len()).sum());
            let mut consts = Vec::with_capacity(per.len());
            for (mut items, constant) in per {
                instances.append(&mut items);
                consts.push(constant);
            }
            if instances.is_empty() {
                return Vec::new();
            }
            return Self::upload_native(
                device,
                queue,
                const_bgl,
                instances,
                &consts,
                false,
                "wire.highlight.ibuf",
            );
        }

        let _ = const_bgl;
        let max_packed_instances = super::gpu_budget::max_elements::<PackedWireInstance>(device);
        let per: Vec<Vec<PackedWireInstance>> = wires
            .iter()
            .map(|wire| {
                emit_wire_packed(wire, color, wire_draw_depth(wire, depth_map))
            })
            .collect();
        let mut instances = Vec::with_capacity(per.iter().map(Vec::len).sum());
        for mut items in per {
            instances.append(&mut items);
        }
        instances
            .chunks(max_packed_instances)
            .map(|chunk| Self {
                instance_buffer: instance_buffer(
                    device,
                    queue,
                    "wire.highlight.compat.ibuf",
                    chunk,
                ),
                first_instance: 0,
                instance_count: chunk.len() as u32,
                is_3d_mesh_edge: false,
                const_bind_group: None,
            })
            .collect()
    }

    /// Upload borrowed wires when a partition exceeds the arena budget.
    pub fn from_run_refs(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[&WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
        mesh_edge: bool,
        const_bgl: Option<&wgpu::BindGroupLayout>,
    ) -> Vec<Self> {
        let Some(const_bgl) = const_bgl else {
            let max_instances = super::gpu_budget::max_elements::<PackedWireInstance>(device);
            use crate::par::prelude::*;
            let per: Vec<Vec<PackedWireInstance>> = wires
                .par_iter()
                .map(|&wire| {
                    let depth = if mesh_edge {
                        0.0
                    } else {
                        wire_draw_depth(wire, depth_map)
                    };
                    emit_wire_packed(wire, wire.color, depth)
                })
                .collect();
            let mut instances =
                Vec::with_capacity(per.iter().map(Vec::len).sum());
            for mut items in per {
                instances.append(&mut items);
            }
            return instances
                .chunks(max_instances)
                .map(|chunk| Self {
                    instance_buffer: instance_buffer(
                        device,
                        queue,
                        "wire.run.hybrid.compat.ibuf",
                        chunk,
                    ),
                    first_instance: 0,
                    instance_count: chunk.len() as u32,
                    is_3d_mesh_edge: mesh_edge,
                    const_bind_group: None,
                })
                .collect();
        };
        use crate::par::prelude::*;
        let per: Vec<(Vec<WireInstance>, WireConst)> = wires
            .par_iter()
            .enumerate()
            .map(|(idx, &wire)| {
                let depth = if mesh_edge {
                    0.0
                } else {
                    wire_draw_depth(wire, depth_map)
                };
                emit_wire_native(wire, idx as u32, wire.color, depth)
            })
            .collect();
        let mut instances: Vec<WireInstance> =
            Vec::with_capacity(per.iter().map(|(items, _)| items.len()).sum());
        let mut consts = Vec::with_capacity(per.len());
        for (mut items, constant) in per {
            instances.append(&mut items);
            consts.push(constant);
        }
        if instances.is_empty() {
            return Vec::new();
        }
        Self::upload_native(
            device,
            queue,
            const_bgl,
            instances,
            &consts,
            mesh_edge,
            "wire.run.hybrid.ibuf",
        )
    }

    /// Upload a run of wires in submission order, within the buffer budget.
    ///
    /// Unlike [`from_batch`], instance order is **guaranteed** to follow wire
    /// order (parallel `collect` is index-ordered; the flatten is sequential).
    /// The main wire pass depends on that — depth-biased overlap *and* alpha
    /// blending both resolve in submission order, so a reorder would change the
    /// image for transparent / coincident wires.
    pub fn from_run(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
        mesh_edge: bool,
        const_bgl: Option<&wgpu::BindGroupLayout>,
    ) -> Vec<Self> {
        if let Some(const_bgl) = const_bgl {
            use crate::par::prelude::*;
            // Indexed collection preserves transparent and coincident wire order.
            let per: Vec<(Vec<WireInstance>, WireConst)> = wires
                .par_iter()
                .enumerate()
                .map(|(idx, w)| {
                    // 3D mesh outline edges are real geometry occluded by true
                    // depth — they must NOT take the draw-order z-bias (which
                    // pulls 2D wires toward the camera), or the hidden edges of a
                    // small / distant mesh peek through its own shaded fill.
                    let dd = if mesh_edge { 0.0 } else { wire_draw_depth(w, depth_map) };
                    emit_wire_native(w, idx as u32, w.color, dd)
                })
                .collect();
            let mut instances: Vec<WireInstance> =
                Vec::with_capacity(per.iter().map(|(v, _)| v.len()).sum());
            let mut consts: Vec<WireConst> = Vec::with_capacity(per.len());
            for (mut v, c) in per {
                instances.append(&mut v);
                consts.push(c);
            }
            if instances.is_empty() {
                return vec![];
            }
            return Self::upload_native(
                device,
                queue,
                const_bgl,
                instances,
                &consts,
                mesh_edge,
                "wire.run.ibuf",
            );
        }

        let _ = const_bgl;
        let max_packed_instances = super::gpu_budget::max_elements::<PackedWireInstance>(device);
        let per: Vec<Vec<PackedWireInstance>> = wires
            .iter()
            .map(|w| {
                let dd = if mesh_edge { 0.0 } else { wire_draw_depth(w, depth_map) };
                emit_wire_packed(w, w.color, dd)
            })
            .collect();
        let mut instances: Vec<PackedWireInstance> =
            Vec::with_capacity(per.iter().map(Vec::len).sum());
        for mut v in per {
            instances.append(&mut v);
        }
        if instances.is_empty() {
            return vec![];
        }
        instances
            .chunks(max_packed_instances)
            .map(|chunk| {
                let buf = instance_buffer(device, queue, "wire.run.compat.ibuf", chunk);
                Self {
                    instance_buffer: buf,
                    first_instance: 0,
                    instance_count: chunk.len() as u32,
                    is_3d_mesh_edge: mesh_edge,
                    const_bind_group: None,
                }
            })
            .collect()
    }

    /// Upload wires in chunks that retain each wire’s color and pattern.
    pub fn from_batch(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
        const_bgl: Option<&wgpu::BindGroupLayout>,
    ) -> Vec<Self> {
        let total_segs: usize = wires.iter().map(|w| w.points.len().saturating_sub(1)).sum();
        if total_segs == 0 {
            return vec![];
        }
        if let Some(const_bgl) = const_bgl {
            use crate::par::prelude::*;
            // `block_cache` groups wires by style upstream; order within a batch
            // doesn't affect correctness, but indexed `collect` gives each wire a
            // stable `wire_id` into the shared WireConst buffer.
            let per: Vec<(Vec<WireInstance>, WireConst)> = wires
                .par_iter()
                .enumerate()
                .map(|(idx, w)| {
                    emit_wire_native(w, idx as u32, w.color, wire_draw_depth(w, depth_map))
                })
                .collect();
            let mut instances: Vec<WireInstance> =
                Vec::with_capacity(per.iter().map(|(v, _)| v.len()).sum());
            let mut consts: Vec<WireConst> = Vec::with_capacity(per.len());
            for (mut v, c) in per {
                instances.append(&mut v);
                consts.push(c);
            }
            if instances.is_empty() {
                return vec![];
            }
            return Self::upload_native(
                device,
                queue,
                const_bgl,
                instances,
                &consts,
                false,
                "wire.batch.ibuf",
            );
        }

        let _ = const_bgl;
        let max_packed_instances = super::gpu_budget::max_elements::<PackedWireInstance>(device);
        let instances: Vec<PackedWireInstance> = wires
            .iter()
            .flat_map(|w| emit_wire_packed(w, w.color, wire_draw_depth(w, depth_map)))
            .collect();
        if instances.is_empty() {
            return vec![];
        }
        instances
            .chunks(max_packed_instances)
            .enumerate()
            .map(|(i, chunk)| {
                let label = format!("wire.batch.compat.ibuf.{i}");
                let instance_buffer = instance_buffer(device, queue, &label, chunk);
                Self {
                    instance_buffer,
                    first_instance: 0,
                    instance_count: chunk.len() as u32,
                    is_3d_mesh_edge: false,
                    const_bind_group: None,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod block_wire_storage_tests {
    use super::BlockWireVertex;

    fn module(source: &str) -> naga::Module {
        naga::front::wgsl::parse_str(source).expect("shader parses")
    }

    fn validate(module: &naga::Module) -> naga::valid::ModuleInfo {
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(module)
        .expect("shader validates")
    }

    /// A layout mismatch here is invisible until a block draws as garbage on a
    /// real device, so the stride is checked against the Rust record instead.
    #[test]
    fn segment_struct_matches_the_rust_record() {
        let module = module(include_str!("../../shaders/block_wire_storage.wgsl"));
        validate(&module);
        let mut layouter = naga::proc::Layouter::default();
        layouter
            .update(module.to_ctx())
            .expect("layouts are computable");
        let (handle, _) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Segment"))
            .expect("Segment is declared");
        assert_eq!(
            layouter[handle].size as usize,
            std::mem::size_of::<BlockWireVertex>(),
            "WGSL Segment and Rust BlockWireVertex must agree byte for byte; \
             a vec3 member would pad to 16 and break this",
        );
        assert_eq!(layouter[handle].alignment * 1u32, 4);
    }

    /// The two shaders must stay interchangeable: same entry points, same
    /// fragment behaviour. Only the geometry source differs.
    #[test]
    fn both_block_wire_shaders_expose_the_same_entry_points() {
        let packed = module(include_str!("../../shaders/block_wire.wgsl"));
        let storage = module(include_str!("../../shaders/block_wire_storage.wgsl"));
        validate(&packed);
        let names = |m: &naga::Module| {
            let mut n: Vec<String> = m.entry_points.iter().map(|e| e.name.clone()).collect();
            n.sort();
            n
        };
        assert_eq!(names(&packed), names(&storage));
    }
}
