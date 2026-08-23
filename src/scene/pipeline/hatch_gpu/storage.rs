// Batched hatch rendering. The kernel triangulates each boundary once; the
// fragment shader only evaluates the fill pattern inside that mesh.

use crate::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;

// ── GPU structs ────────────────────────────────────────────────────────────
//
// Layout matches the WGSL `HatchInstance` exactly. `repr(C)` + manual
// padding keeps WGSL's 16-byte alignment rules satisfied for arrays of
// this struct.

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HatchInstance {
    pub color: [f32; 4],            //   0
    pub color2: [f32; 4],           //  16  (gradient end)
    pub aabb: [f32; 4],             //  32  (local-space xmin,ymin,xmax,ymax)
    pub world_origin: [f32; 2],     //  48  (anchor high half, added back in VS)
    pub world_origin_low: [f32; 2], //  56  (anchor low residual — double-single)
    pub angle_offset: f32,          //  64
    pub scale: f32,                 //  68
    pub grad_cos: f32,              //  72
    pub grad_sin: f32,              //  76
    pub grad_min: f32,              //  80
    pub grad_range: f32,            //  84
    pub mode: u32,                  //  88  (0=pattern, 1=solid, 2=gradient)
    pub visible: u32,               //  92  (CPU sets to 0 to skip)
    pub family_offset: u32,         //  96
    pub family_count: u32,          // 100
    /// Signed draw-order depth (-1,1); 0.0 = neutral. Applied as a clip-z
    /// bias in the vertex shader so this fill orders against other types.
    pub draw_depth: f32,            // 104
    /// Gradient shape (`GradientKind::shader_kind`), bit 4 = inverted stops.
    pub grad_kind: u32,             // 108
}

const _: () = assert!(std::mem::size_of::<HatchInstance>() == 112);

/// Split a hatch's f64 world-origin anchor into double-single (high, low) f32
/// pairs so the GPU keeps sub-unit precision at UTM-scale coordinates.
#[inline]
fn split_origin_ds(o: [f64; 2]) -> ([f32; 2], [f32; 2]) {
    let hx = o[0] as f32;
    let hy = o[1] as f32;
    ([hx, hy], [(o[0] - hx as f64) as f32, (o[1] - hy as f64) as f32])
}

/// Mirrors the per-family struct used by the existing per-hatch shader,
/// but the dash slice lives in a separate concatenated DashBuffer (the
/// old shader had it embedded). `dash_offset` / `dash_count` index into
/// that flat array.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineFamilyGpu {
    pub cos_a: f32,        //  0
    pub sin_a: f32,        //  4
    pub x0: f32,           //  8
    pub y0: f32,           // 12
    pub dx: f32,           // 16
    pub dy: f32,           // 20
    pub perp_step: f32,    // 24
    pub along_step: f32,   // 28
    pub line_width: f32,   // 32
    pub period: f32,       // 36
    pub n_dashes: u32,     // 40
    pub dash_offset: u32,  // 44
}

const _: () = assert!(std::mem::size_of::<LineFamilyGpu>() == 48);

/// Kernel mesh vertex in local space with its source instance.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct HatchVertex {
    pub local_xy: [f32; 2],  // 0  — local-space position
    pub instance_index: u32, // 8  — index into InstanceBuffer
}

impl HatchVertex {
    pub(super) fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<HatchVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct HatchPlacement {
    pub translation: [f32; 2],
    pub translation_low: [f32; 2],
    pub draw_depth: f32,
    pub visible: u32,
}

impl HatchPlacement {
    pub(super) fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 16, shader_location: 4, format: wgpu::VertexFormat::Float32 },
                wgpu::VertexAttribute { offset: 20, shader_location: 5, format: wgpu::VertexFormat::Uint32 },
            ],
        }
    }
}

// ── Batch builder ──────────────────────────────────────────────────────────

/// Pack a list of `HatchModel`s into the four concatenated storage
/// buffers + the per-vertex buffer needed by `hatch.wgsl`.
/// Returns `None` when the input slice is empty (caller skips the
/// hatch render pass entirely).
pub(super) struct StorageHatchBatch {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub placement_buffer: wgpu::Buffer,
    pub draws: Vec<(std::ops::Range<u32>, std::ops::Range<u32>)>,
    // The four storage buffers below are referenced via `bind_group` —
    // dropping them would invalidate it, but the bind group is the
    // only direct consumer. Keep them as fields to keep ownership in
    // one place; `#[allow(dead_code)]` silences the read-never warning.
    #[allow(dead_code)] pub instance_buffer: wgpu::Buffer,
    #[allow(dead_code)] pub family_buffer:   wgpu::Buffer,
    #[allow(dead_code)] pub dash_buffer:     wgpu::Buffer,
    /// Per-instance visibility flag (1=draw, 0=skip). Stored in its
    /// own small storage buffer so per-frame updates don't have to
    /// touch the large `instance_buffer`. Vertex shader reads
    /// `visibility[instance_index]` — when 0 it emits an out-of-NDC
    /// clip position so the GPU clips the primitive before the
    /// fragment stage runs.
    #[allow(dead_code)] pub visibility_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    #[allow(dead_code)] pub instance_count: u32,
    /// CPU mirror — `update_visibility` re-uploads this whole slice
    /// when any flag changes. ~4 B per hatch, so 40 KB / 10 k hatches
    /// per pan tick. Far cheaper than touching the instance data.
    pub placements: Vec<HatchPlacement>,
    pub source_visibility: Vec<u32>,
    pub source_aabbs: Vec<[f32; 4]>,
    pub unique_source_count: usize,
    /// CPU-side mirror of each instance's local-space AABB (world-
    /// offset-subtracted, world_origin already added back). Used by
    /// `compute_hatch_lod` to evaluate the sub-pixel + frustum cull
    /// without reading back from the GPU.
    pub instance_aabbs: Vec<[f32; 4]>,
}

fn hatch_buffer_cost(hatch: &HatchModel) -> [u64; 7] {
    let boundary = hatch.boundary.len() as u64;
    let (families, dashes) = match &hatch.pattern {
        HatchPattern::Pattern(families) => (
            families.len() as u64,
            families.iter().map(|family| family.dashes.len() as u64).sum(),
        ),
        _ => (0, 0),
    };
    [
        boundary.saturating_mul(std::mem::size_of::<HatchVertex>() as u64),
        boundary.saturating_mul(24),
        std::mem::size_of::<HatchInstance>() as u64,
        families.saturating_mul(std::mem::size_of::<LineFamilyGpu>() as u64),
        dashes.saturating_mul(std::mem::size_of::<f32>() as u64),
        std::mem::size_of::<u32>() as u64,
        std::mem::size_of::<HatchPlacement>() as u64,
    ]
}

impl StorageHatchBatch {
    /// Builds whole-hatch chunks within the device's buffer limits.
    pub(super) fn build(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        hatches: &[HatchModel],
    ) -> Vec<Self> {
        if hatches.is_empty() {
            return Vec::new();
        }

        let limits = device.limits();
        let chunk_limit = limits
            .max_buffer_size
            .min(limits.max_storage_buffer_binding_size as u64);
        let mut ranges = Vec::new();
        let mut start = 0;
        let mut used = [0u64; 7];
        for (index, hatch) in hatches.iter().enumerate() {
            let cost = hatch_buffer_cost(hatch);
            let overflow = used
                .iter()
                .zip(cost)
                .any(|(used, cost)| used.saturating_add(cost) > chunk_limit);
            if overflow && index > start {
                ranges.push(start..index);
                start = index;
                used = [0; 7];
            }
            for (used, cost) in used.iter_mut().zip(cost) {
                *used = used.saturating_add(cost);
            }
        }
        ranges.push(start..hatches.len());
        ranges
            .into_iter()
            .filter_map(|range| Self::build_chunk(device, bgl, &hatches[range]))
            .collect()
    }

    fn build_chunk(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        hatches: &[HatchModel],
    ) -> Option<Self> {

        let mut instances: Vec<HatchInstance> = Vec::with_capacity(hatches.len());
        let mut families: Vec<LineFamilyGpu> = Vec::new();
        let mut dashes: Vec<f32> = Vec::new();
        let mut meshes = Vec::with_capacity(hatches.len());
        let mut slots = rustc_hash::FxHashMap::default();
        let mut groups: Vec<Vec<&HatchModel>> = Vec::new();
        for (index, hatch) in hatches.iter().enumerate() {
            let key = hatch
                .render_instance
                .map(|instance| (true, instance.source_id))
                .unwrap_or((false, index as u64));
            let slot = *slots.entry(key).or_insert_with(|| {
                let slot = groups.len();
                groups.push(Vec::new());
                slot
            });
            groups[slot].push(hatch);
        }
        groups.sort_by_key(|group| std::cmp::Reverse(group.len() == 1));
        let unique_source_count = groups.iter().take_while(|group| group.len() == 1).count();

        for group in &groups {
            let h = group[0];
            let mesh = h.fill_mesh();
            let has_mesh = !mesh.0.is_empty() && !mesh.1.is_empty();
            meshes.push(mesh);
            let family_offset = families.len() as u32;
            let mut family_count = 0u32;

            let mut grad_kind = 0u32;
            let (mode, color2, grad_cos, grad_sin, grad_min, grad_range) = match &h.pattern {
                HatchPattern::Solid => (1u32, [0.0; 4], 0.0, 0.0, 0.0, 1.0),
                HatchPattern::Gradient {
                    angle_deg,
                    color2,
                    kind,
                    invert,
                    shift,
                } => {
                    grad_kind = kind.shader_kind() | if *invert { 16 } else { 0 };
                    let frame = h.gradient_frame(*angle_deg, *shift);
                    if kind.radial() {
                        let center = frame.map_or([0.0, 0.0], |frame| {
                            [frame.center[0] as f32, frame.center[1] as f32]
                        });
                        let radius = frame.map_or(1.0, |frame| frame.radius as f32);
                        (3u32, *color2, center[0], center[1], 0.0, radius)
                    } else {
                        let r = angle_deg.to_radians();
                        let minimum = frame.map_or(0.0, |frame| frame.projection_min as f32);
                        let span = frame.map_or(1.0, |frame| frame.projection_span as f32);
                        (2u32, *color2, r.cos(), r.sin(), minimum, span)
                    }
                }
                HatchPattern::Pattern(fams) => {
                    for fam in fams {
                        let dash_offset = dashes.len() as u32;
                        for &d in &fam.dashes {
                            dashes.push(d);
                        }
                        let n_dashes = (dashes.len() as u32 - dash_offset).min(u32::MAX);
                        // PAT local frame: perpendicular spacing and
                        // along-line phase.
                        let perp_step = fam.dy;
                        let along_step = fam.dx;
                        // Screen-space derivative drives 1-px line width
                        // in the shader; this stored field is unused.
                        let line_width = 0.0_f32;
                        let period: f32 = fam.dashes.iter().map(|d| d.abs()).sum();
                        families.push(LineFamilyGpu {
                            cos_a: fam.angle_deg.to_radians().cos(),
                            sin_a: fam.angle_deg.to_radians().sin(),
                            x0: fam.x0,
                            y0: fam.y0,
                            dx: fam.dx,
                            dy: fam.dy,
                            perp_step,
                            along_step,
                            line_width,
                            period: if n_dashes > 0 { period } else { 0.0 },
                            n_dashes,
                            dash_offset,
                        });
                        family_count += 1;
                    }
                    (0u32, [0.0; 4], 0.0, 0.0, 0.0, 1.0)
                }
            };

            // Boundary AABB in local space (matches the corner quad
            // emitted by the vertex shader). The verts are already in
            // `world_origin`-relative coords (see scene/mod.rs hatch
            // packing), so this AABB lives in that frame.
            let mut min_x = f32::INFINITY;
            let mut min_y = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            for &[x, y] in h.boundary.iter() {
                if x.is_finite() && y.is_finite() {
                    if x < min_x { min_x = x; }
                    if y < min_y { min_y = y; }
                    if x > max_x { max_x = x; }
                    if y > max_y { max_y = y; }
                }
            }
            if !min_x.is_finite() {
                // Empty / all-NaN — skip but keep the slot so indices
                // stay in lockstep with the input list (visibility=0).
                let (wo_hi, wo_lo) = split_origin_ds(h.world_origin);
                instances.push(HatchInstance {
                    color: h.color,
                    color2,
                    aabb: [0.0, 0.0, 0.0, 0.0],
                    world_origin: wo_hi,
                    world_origin_low: wo_lo,
                    angle_offset: h.angle_offset,
                    scale: h.scale.max(1e-6),
                    grad_cos,
                    grad_sin,
                    grad_min,
                    grad_range,
                    mode,
                    visible: 0,
                    family_offset,
                    family_count,
                    draw_depth: if group.len() == 1 { h.draw_depth } else { 0.0 },
                    grad_kind,
                });
                continue;
            }

            let (wo_hi, wo_lo) = split_origin_ds(h.world_origin);
            instances.push(HatchInstance {
                color: h.color,
                color2,
                aabb: [min_x, min_y, max_x, max_y],
                world_origin: wo_hi,
                world_origin_low: wo_lo,
                angle_offset: h.angle_offset,
                scale: h.scale.max(1e-6),
                grad_cos,
                grad_sin,
                grad_min,
                grad_range,
                mode,
                visible: u32::from(has_mesh),
                family_offset,
                family_count,
                draw_depth: if group.len() == 1 { h.draw_depth } else { 0.0 },
                grad_kind,
            });
        }
        // Empty fallbacks — storage buffers can't be zero-sized.
        if families.is_empty() {
            families.push(LineFamilyGpu::default_filler());
        }
        if dashes.is_empty() {
            dashes.push(0.0);
        }

        let mut verts = Vec::new();
        let mut indices = Vec::new();
        let mut mesh_ranges = Vec::with_capacity(meshes.len());
        for (instance_index, (points, mesh_indices)) in meshes.into_iter().enumerate() {
            let Ok(base) = u32::try_from(verts.len()) else {
                return None;
            };
            let Ok(start) = u32::try_from(indices.len()) else {
                return None;
            };
            verts.extend(points.into_iter().map(|local_xy| HatchVertex {
                local_xy,
                instance_index: instance_index as u32,
            }));
            for index in mesh_indices {
                let Some(index) = base.checked_add(index) else {
                    return None;
                };
                indices.push(index);
            }
            let Ok(end) = u32::try_from(indices.len()) else {
                return None;
            };
            mesh_ranges.push(start..end);
        }
        if indices.is_empty() {
            return None;
        }
        let mut placements = Vec::new();
        let mut draws = Vec::with_capacity(groups.len());
        let mut instance_aabbs = Vec::new();
        if unique_source_count > 0 {
            placements.push(HatchPlacement {
                translation: [0.0; 2],
                translation_low: [0.0; 2],
                draw_depth: 0.0,
                visible: 1,
            });
            let end = mesh_ranges[unique_source_count - 1].end;
            if end > 0 {
                draws.push((0..end, 0..1));
            }
            let mut union = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
            for inst in instances.iter().take(unique_source_count) {
                union[0] = union[0].min(inst.aabb[0] + inst.world_origin[0]);
                union[1] = union[1].min(inst.aabb[1] + inst.world_origin[1]);
                union[2] = union[2].max(inst.aabb[2] + inst.world_origin[0]);
                union[3] = union[3].max(inst.aabb[3] + inst.world_origin[1]);
            }
            instance_aabbs.push(union);
        }
        for (source_index, group) in groups.iter().enumerate().skip(unique_source_count) {
            let source = group[0];
            let base = source
                .render_instance
                .map_or([0.0; 3], |instance| instance.translation);
            let placement_start = placements.len() as u32;
            for hatch in group {
                let translation = hatch
                    .render_instance
                    .map_or([0.0; 3], |instance| instance.translation);
                let delta = [translation[0] - base[0], translation[1] - base[1]];
                let high = [delta[0] as f32, delta[1] as f32];
                placements.push(HatchPlacement {
                    translation: high,
                    translation_low: [
                        (delta[0] - high[0] as f64) as f32,
                        (delta[1] - high[1] as f64) as f32,
                    ],
                    draw_depth: hatch.draw_depth,
                    visible: 1,
                });
                let inst = &instances[source_index];
                instance_aabbs.push([
                    inst.aabb[0] + inst.world_origin[0] + high[0],
                    inst.aabb[1] + inst.world_origin[1] + high[1],
                    inst.aabb[2] + inst.world_origin[0] + high[0],
                    inst.aabb[3] + inst.world_origin[1] + high[1],
                ]);
            }
            if !mesh_ranges[source_index].is_empty() {
                draws.push((
                    mesh_ranges[source_index].clone(),
                    placement_start..placements.len() as u32,
                ));
            }
        }

        let visibility: Vec<u32> = instances.iter().map(|instance| instance.visible).collect();
        let limits = device.limits();
        let buffer_fits = |count: usize, stride: usize| {
            (count as u64).saturating_mul(stride as u64) <= limits.max_buffer_size
        };
        let storage_fits = |count: usize, stride: usize| {
            (count as u64).saturating_mul(stride as u64)
                <= limits.max_storage_buffer_binding_size as u64
        };
        if !buffer_fits(verts.len(), std::mem::size_of::<HatchVertex>())
            || !buffer_fits(indices.len(), std::mem::size_of::<u32>())
            || !buffer_fits(placements.len(), std::mem::size_of::<HatchPlacement>())
            || !storage_fits(instances.len(), std::mem::size_of::<HatchInstance>())
            || !storage_fits(families.len(), std::mem::size_of::<LineFamilyGpu>())
            || !storage_fits(dashes.len(), std::mem::size_of::<f32>())
            || !storage_fits(visibility.len(), std::mem::size_of::<u32>())
        {
            return None;
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.vertex"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.index"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let placement_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.placements"),
            contents: bytemuck::cast_slice(&placements),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.instances"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        let family_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.families"),
            contents: bytemuck::cast_slice(&families),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let dash_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.dashes"),
            contents: bytemuck::cast_slice(&dashes),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let visibility_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hatch.visibility"),
            contents: bytemuck::cast_slice(&visibility),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hatch.bg"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: instance_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: family_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: dash_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: visibility_buffer.as_entire_binding(),
                },
            ],
        });
        let source_aabbs = instances
            .iter()
            .map(|inst| [
                inst.aabb[0] + inst.world_origin[0],
                inst.aabb[1] + inst.world_origin[1],
                inst.aabb[2] + inst.world_origin[0],
                inst.aabb[3] + inst.world_origin[1],
            ])
            .collect();

        Some(Self {
            vertex_buffer,
            index_buffer,
            placement_buffer,
            draws,
            instance_buffer,
            family_buffer,
            dash_buffer,
            visibility_buffer,
            bind_group,
            instance_count: instances.len() as u32,
            placements,
            source_visibility: visibility,
            source_aabbs,
            unique_source_count,
            instance_aabbs,
        })
    }

    /// Push the CPU `visibility` slice to GPU. Call when any
    /// element changes (typically per-frame from compute_hatch_lod).
    pub(super) fn upload_visibility(&self, queue: &wgpu::Queue) {
        queue.write_buffer(
            &self.placement_buffer,
            0,
            bytemuck::cast_slice(&self.placements),
        );
        queue.write_buffer(
            &self.visibility_buffer,
            0,
            bytemuck::cast_slice(&self.source_visibility),
        );
    }

    /// Shared layout for instance, family, dash, and visibility storage.
    #[allow(dead_code)]
    pub(super) fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        let entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hatch.bgl"),
            entries: &[entry(0), entry(1), entry(2), entry(3)],
        })
    }
}

impl LineFamilyGpu {
    fn default_filler() -> Self {
        Self {
            cos_a: 1.0,
            sin_a: 0.0,
            x0: 0.0,
            y0: 0.0,
            dx: 1.0,
            dy: 0.0,
            perp_step: 1.0,
            along_step: 1.0,
            line_width: 0.0,
            period: 0.0,
            n_dashes: 0,
            dash_offset: 0,
        }
    }
}

// PatFamily is re-exported by hatch_model so we don't need to import
// it explicitly anywhere else — but rust needs the type referenced to
// confirm the layout assumption above.
#[allow(dead_code)]
fn _assert_patfamily_fields(f: &PatFamily) -> (f32, f32, f32) {
    (f.angle_deg, f.x0, f.y0)
}
