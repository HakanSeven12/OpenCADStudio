// Triangle mesh model — produced by the kernel Shell/Solid tessellation.
//
// Stored alongside WireModels in the scene; rendered by the mesh pipeline
// (wgpu TriangleList with depth test, flat normals).

use super::wire_model::WireModel;

/// Hard ceiling on the point count of a generated volume snap wire, matching
/// the per-wire cap the block cache applies (`block_cache.rs`) and the spirit
/// of the mesh converters' `SNAP_TABLE_MAX_VERTICES`: a mesh dense enough to
/// exceed this has edges far too close together to pick apart at snap
/// aperture size, and duplicating them into the wire set would cost more
/// memory than the whole rest of the interaction set. The builder budgets
/// SEGMENTS against this (each costs two endpoints plus a NaN separator) and
/// decimates rather than dropping when it is exceeded.
pub const SNAP_EDGE_WIRE_MAX_POINTS: usize = 100_000;

/// Segment budget implied by [`SNAP_EDGE_WIRE_MAX_POINTS`]: S segments cost
/// `3·S − 1` wire points (two endpoints each plus a NaN separator between
/// neighbours), so the point cap admits at most `(cap + 1) / 3` segments.
const MAX_EDGE_SEGMENTS: usize = (SNAP_EDGE_WIRE_MAX_POINTS + 1) / 3;

/// Halve `kept` in place, retaining every second segment (indices 0, 2, …).
fn halve_kept_segments(kept: &mut Vec<([f64; 3], [f64; 3])>) {
    let mut write = 0;
    for read in (0..kept.len()).step_by(2) {
        kept[write] = kept[read];
        write += 1;
    }
    kept.truncate(write);
}

/// Build the interaction-only edge wire shared by every volume entity
/// (3D solids, regions, bodies, surfaces, plain meshes).
///
/// `pairs` yields one `(a, b)` world-space segment per edge. The wire carries
/// the segments as NaN-separated point pairs (so `points` never chains two
/// unrelated edges together) plus the same NaN-separated pairs in
/// `key_vertices` (so Endpoint/Midpoint snap sees every true edge end / edge
/// middle, not the ends of an artificial chain between unrelated edges —
/// Midpoint consumers treat consecutive `key_vertices` as one segment). It is
/// `display_visible: false` / `plot_visible: false`: the mesh pipeline
/// already draws these edges, the wire exists so the SNAPPER and the
/// proximity pick see them exactly like any 2-D entity's geometry (#1030).
///
/// Past [`MAX_EDGE_SEGMENTS`] the wire is not dropped but DECIMATED: the
/// segment pairs are uniformly subsampled (every Nth, N a power of two, with
/// the first and last segments always retained) down to the budget. Dropping
/// the wire outright used to cost dense meshes ALL endpoint snap; decimation
/// keeps partial coverage at the same bounded memory. The tradeoff: mid-wire
/// edges thin out (their Midpoint/Nearest coverage becomes sparser) while
/// both ends of the mesh stay snap-reachable.
///
/// Returns `None` only when `pairs` yields no segments at all.
pub fn edge_snap_wire(
    name: String,
    color: [f32; 4],
    selected: bool,
    pairs: impl Iterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<WireModel> {
    // ── Segment selection with bounded memory ─────────────────────────────
    // Stream the pairs and keep at most MAX_EDGE_SEGMENTS of them: whenever
    // the kept buffer fills, halve it and double the stride. That is uniform
    // "every Nth" subsampling (power-of-two N) without ever buffering more
    // than the budget — a runaway mesh costs O(cap) here, not O(edges), and
    // its (already resident) tessellation is never duplicated.
    let mut kept: Vec<([f64; 3], [f64; 3])> = Vec::new();
    let mut stride = 1usize;
    let mut total = 0usize;
    let mut last_pair: Option<([f64; 3], [f64; 3])> = None;
    for pair in pairs {
        if total % stride == 0 {
            if kept.len() >= MAX_EDGE_SEGMENTS {
                halve_kept_segments(&mut kept);
                stride *= 2;
            }
            kept.push(pair);
        }
        last_pair = Some(pair);
        total += 1;
    }
    // Force-retain the final segment so decimation never removes the far end
    // of the mesh: `(total - 1) % stride != 0` proves it is not already kept
    // (every kept entry sits at an index that is a multiple of `stride`).
    if let Some(last) = last_pair {
        if (total - 1) % stride != 0 {
            if kept.len() >= MAX_EDGE_SEGMENTS {
                halve_kept_segments(&mut kept);
            }
            kept.push(last);
        }
    }

    let mut points: Vec<[f32; 3]> = Vec::with_capacity(kept.len().saturating_mul(3));
    let mut points_low: Vec<[f32; 3]> = Vec::with_capacity(kept.len().saturating_mul(3));
    let mut key_vertices: Vec<[f64; 3]> = Vec::with_capacity(kept.len().saturating_mul(3));
    let mut aabb = WireModel::UNBOUNDED_AABB;
    let extend = |p: [f64; 3], aabb: &mut [f32; 4]| {
        let (x, y) = (p[0] as f32, p[1] as f32);
        if x.is_finite() && y.is_finite() {
            if *aabb == WireModel::UNBOUNDED_AABB {
                *aabb = [x, y, x, y];
            } else {
                aabb[0] = aabb[0].min(x);
                aabb[1] = aabb[1].min(y);
                aabb[2] = aabb[2].max(x);
                aabb[3] = aabb[3].max(y);
            }
        }
    };
    for (index, (a, b)) in kept.into_iter().enumerate() {
        if index > 0 {
            points.push([f32::NAN; 3]);
            points_low.push([0.0; 3]);
            // Mirrors `points`: without this separator Midpoint would pair
            // the last vertex of one edge with the first vertex of the next
            // and fabricate a floating midpoint between unrelated edges.
            key_vertices.push([f64::NAN; 3]);
        }
        for p in [a, b] {
            let (hx, lx) = WireModel::split_ds(p[0]);
            let (hy, ly) = WireModel::split_ds(p[1]);
            let (hz, lz) = WireModel::split_ds(p[2]);
            points.push([hx, hy, hz]);
            points_low.push([lx, ly, lz]);
            key_vertices.push(p);
            extend(p, &mut aabb);
        }
    }
    if points.is_empty() {
        return None;
    }
    Some(WireModel {
        bg_adapt: None,
        point_marker: None,
        taper_widths: Vec::new(),
        pattern_stations: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: false,
        plot_visible: false,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        render_instance: None,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
        text_verts: Vec::new(),
        name,
        points,
        points_low,
        color,
        selected,
        aci: 0,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        key_vertices,
        aabb,
        plinegen: true,
        dash_from_start: false,
        dash_align_end: None,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    })
}

/// A tessellated triangle mesh ready to upload to the GPU.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct MeshModel {
    /// Unique identifier (entity handle value as decimal string).
    pub name: String,
    /// World-space vertex positions (high half of the double-single pair).
    pub verts: Vec<[f32; 3]>,
    /// Low residual paired with `verts` so meshes stay precise at UTM scale.
    /// Empty = all-zero (legacy / interactive meshes near the origin).
    pub verts_low: Vec<[f32; 3]>,
    /// Per-vertex normals (may be empty if not available).
    pub normals: Vec<[f32; 3]>,
    /// Triangle indices into `verts` (every 3 values = one triangle).
    pub indices: Vec<u32>,
    /// Optional AcDbMaterial handle per triangle. Empty means the whole mesh
    /// uses `MeshLodSet::material`; otherwise each entry aligns with one
    /// `indices` triplet and overrides the entity material for that face.
    pub triangle_material_handles: Vec<Option<acadrust::Handle>>,
    /// Optional ACIS face colour per triangle, aligned with `indices` triplets.
    pub triangle_colors: Vec<Option<[f32; 4]>>,
    /// RGBA colour in [0, 1].
    pub color: [f32; 4],
    /// Whether this mesh is currently selected.
    pub selected: bool,
}

/// Bundle of mesh tessellations at different sampling densities, picked
/// per frame by the render pipeline based on the projected pixel size of
/// `world_aabb`. Phase 3.4 LOD ladder:
///
/// | LOD | Source     | Use when projected diagonal |
/// |-----|------------|------------------------------|
/// | 0   | HIGH       | > 200 px                     |
/// | 1   | MID (½)    | 50–200 px                    |
/// | 2   | LOW (¼)    | < 50 px                      |
///
/// `lods` holds up to one MeshModel per LOD level (high → low). Empty
/// slots fall back to the nearest available LOD at render time.
/// Kernel-owned source for a view-dependent silhouette.
#[derive(Clone, Debug)]
pub struct CurvedGen {
    pub source: cadkernel::brep::mesh::SilhouetteSource,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MeshMetrics {
    pub vertices: usize,
    pub triangles: usize,
    pub surface_area: f64,
    pub volume: f64,
    pub centroid: [f64; 3],
    pub moment_of_inertia: [f64; 3],
    pub principal_directions: [f64; 9],
    pub principal_moments: [f64; 3],
    pub product_of_inertia: [f64; 3],
    pub radii_of_gyration: [f64; 3],
}

impl MeshMetrics {
    fn apply_mass_properties(&mut self, properties: cadkernel::brep::MassProperties) {
        self.volume = properties.volume;
        self.centroid = properties.centroid;
        self.moment_of_inertia = properties.moment_of_inertia;
        self.principal_directions = properties.principal_directions;
        self.principal_moments = properties.principal_moments;
        self.product_of_inertia = properties.product_of_inertia;
        self.radii_of_gyration = properties.radii_of_gyration;
    }

    pub fn translate(&mut self, delta: [f64; 3]) {
        if self.volume > 1e-18 {
            self.apply_mass_properties(
                cadkernel::brep::MassProperties {
                    volume: self.volume,
                    centroid: self.centroid,
                    moment_of_inertia: self.moment_of_inertia,
                    principal_directions: self.principal_directions,
                    principal_moments: self.principal_moments,
                    product_of_inertia: self.product_of_inertia,
                    radii_of_gyration: self.radii_of_gyration,
                }
                .translated(delta),
            );
        } else {
            for axis in 0..3 {
                self.centroid[axis] += delta[axis];
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MeshLodSet {
    pub lods: Vec<MeshModel>,
    /// Effective AcDbMaterial resolved from entity/layer/INSERT inheritance.
    /// Geometry remains usable when it is absent; the renderer then keeps the
    /// per-mesh entity colour.
    pub material: Option<super::material_model::MeshMaterial>,
    /// Face-level ACIS material overrides, resolved from each LOD's
    /// `triangle_material_handles`. Only handles actually referenced by the
    /// tessellation are retained.
    pub face_materials:
        rustc_hash::FxHashMap<acadrust::Handle, super::material_model::MeshMaterial>,
    /// Effective AcDbVisualStyle override resolved from the entity's full,
    /// face and edge style handles.
    pub visual_style: Option<super::visual_style_model::MeshVisualStyle>,
    /// True only when every source face produced triangles. False keeps
    /// downstream solid-edit code from treating a display-only partial shell
    /// as a closed, valid solid.
    pub complete: bool,
    /// Feature-edge line list (LOD-independent): pairs of endpoints, high half
    /// of the double-single. Populated for ACIS solids (the B-rep face-boundary
    /// edges) so their wireframe shows real edges rather than the triangulation.
    /// Empty for plain meshes — those fall back to triangle edges at batch time.
    pub edge_verts: Vec<[f32; 3]>,
    /// Low residual paired with `edge_verts`.
    pub edge_verts_low: Vec<[f32; 3]>,
    /// Kernel sources for per-frame silhouettes.
    pub curved_gens: Vec<CurvedGen>,
    /// Geometry measurements calculated once from the highest available LOD.
    /// Properties can read these without re-parsing or re-tessellating ACIS on
    /// the UI thread.
    pub metrics: MeshMetrics,
    /// World XY AABB `[min_x, min_y, max_x, max_y]` of the mesh — used
    /// by the per-frame LOD selector to compute the projected pixel
    /// diagonal.
    pub world_aabb: [f32; 4],
    /// World Z extent `[min_z, max_z]`. With `world_aabb` this is the full 3D
    /// box, which the pick path projects to a screen rect to skip solids whose
    /// footprint isn't under the cursor (O(solids) instead of ray-testing every
    /// triangle). `verts` carry only the high half of the double-single
    /// position, so the bound is f32-precise — fine for a conservative cull.
    pub z_aabb: [f32; 2],
    /// Immutable block-local geometry shared by every INSERT instance of this
    /// block entity. Top-level meshes leave this empty.
    pub instance_source: Option<std::sync::Arc<MeshInstanceSource>>,
    /// Accumulated block-local → world transform for this rendered instance.
    pub instance_transform: Option<acadrust::types::Transform>,
    /// Parent INSERT selected for this rendered block instance.
    pub instance_handle: Option<acadrust::Handle>,
    /// Effective colour after INSERT inheritance, without copying the mesh.
    pub instance_color: Option<[f32; 4]>,
    /// Precise world bounds used by the interaction index.
    pub instance_aabb: Option<[f64; 6]>,
}

#[derive(Clone, Debug)]
pub struct MeshInstanceSource {
    pub handle: acadrust::Handle,
    pub lods: Vec<MeshModel>,
    pub edge_verts: Vec<[f32; 3]>,
    pub edge_verts_low: Vec<[f32; 3]>,
    pub curved_gens: Vec<CurvedGen>,
}

/// 3D bounds of every LOD's vertices: `([min_x, min_y, max_x, max_y], [min_z, max_z])`.
pub fn compute_mesh_aabb(lods: &[MeshModel]) -> ([f32; 4], [f32; 2]) {
    let (mut min_x, mut min_y, mut min_z) = (f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y, mut max_z) =
        (f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for lod in lods {
        for &[x, y, z] in &lod.verts {
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                continue;
            }
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            min_z = min_z.min(z);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            max_z = max_z.max(z);
        }
    }
    ([min_x, min_y, max_x, max_y], [min_z, max_z])
}

fn compute_mesh_metrics(lods: &[MeshModel]) -> MeshMetrics {
    let Some(mesh) = lods.iter().find(|mesh| !mesh.indices.is_empty()) else {
        return MeshMetrics::default();
    };
    let kernel_mesh = cadkernel::brep::Mesh {
        positions: mesh
            .verts
            .iter()
            .enumerate()
            .map(|(index, high)| {
                let low = mesh.verts_low.get(index).copied().unwrap_or([0.0; 3]);
                [
                    high[0] as f64 + low[0] as f64,
                    high[1] as f64 + low[1] as f64,
                    high[2] as f64 + low[2] as f64,
                ]
            })
            .collect(),
        normals: Vec::new(),
        triangles: mesh
            .indices
            .chunks_exact(3)
            .map(|triangle| {
                [
                    triangle[0] as usize,
                    triangle[1] as usize,
                    triangle[2] as usize,
                ]
            })
            .collect(),
    };
    let surface = kernel_mesh.surface_properties();
    let mut metrics = MeshMetrics {
        vertices: mesh.verts.len(),
        triangles: mesh.indices.len() / 3,
        surface_area: surface.map_or(0.0, |value| value.0),
        centroid: surface.map_or([0.0; 3], |value| value.1),
        ..MeshMetrics::default()
    };
    if let Some(properties) = kernel_mesh.inertial_properties() {
        metrics.apply_mass_properties(properties);
    }
    metrics
}

impl MeshLodSet {
    pub fn apply_mass_properties(&mut self, properties: cadkernel::brep::MassProperties) {
        self.metrics.apply_mass_properties(properties);
    }

    /// Build a set from its LODs, computing the 3D AABB.
    pub fn from_lods(lods: Vec<MeshModel>) -> Self {
        let (world_aabb, z_aabb) = compute_mesh_aabb(&lods);
        let metrics = compute_mesh_metrics(&lods);
        Self {
            lods,
            material: None,
            face_materials: rustc_hash::FxHashMap::default(),
            visual_style: None,
            complete: true,
            edge_verts: Vec::new(),
            edge_verts_low: Vec::new(),
            curved_gens: Vec::new(),
            metrics,
            world_aabb,
            z_aabb,
            instance_source: None,
            instance_transform: None,
            instance_handle: None,
            instance_color: None,
            instance_aabb: None,
        }
    }

    /// Wrap a single MeshModel as a one-LOD set. Used by interactive
    /// commands that only produce one tessellation (e.g. the kernel-based
    /// BOX/CYLINDER creation). The LOD selector will pick slot 0 for
    /// every zoom level.
    pub fn from_single(mesh: MeshModel) -> Self {
        Self::from_lods(vec![mesh])
    }

    /// Recompute `world_aabb` / `z_aabb` after the LODs' vertices were rewritten
    /// (relative-to-eye re-split, INSERT transform).
    pub fn recompute_aabb(&mut self) {
        let (xy, z) = compute_mesh_aabb(&self.lods);
        self.world_aabb = xy;
        self.z_aabb = z;
    }

    pub fn prepare_instance_source(&mut self, handle: acadrust::Handle) {
        self.instance_source = Some(std::sync::Arc::new(MeshInstanceSource {
            handle,
            lods: self.lods.clone(),
            edge_verts: self.edge_verts.clone(),
            edge_verts_low: self.edge_verts_low.clone(),
            curved_gens: self.curved_gens.clone(),
        }));
        self.instance_transform = None;
        self.instance_handle = None;
        self.instance_color = None;
        self.instance_aabb = None;
    }

    pub fn geometry_lods(&self) -> &[MeshModel] {
        self.instance_source
            .as_ref()
            .map_or(self.lods.as_slice(), |source| source.lods.as_slice())
    }

    pub fn geometry_edges(&self) -> (&[[f32; 3]], &[[f32; 3]]) {
        self.instance_source.as_ref().map_or(
            (self.edge_verts.as_slice(), self.edge_verts_low.as_slice()),
            |source| (
                source.edge_verts.as_slice(),
                source.edge_verts_low.as_slice(),
            ),
        )
    }

    pub fn entity_handle(&self) -> Option<acadrust::Handle> {
        self.instance_handle.or_else(|| {
            self.lods
                .first()
                .and_then(|mesh| mesh.name.parse::<u64>().ok())
                .map(acadrust::Handle::new)
        })
    }

    pub fn display_color(&self) -> Option<[f32; 4]> {
        self.instance_color.or_else(|| {
            self.geometry_lods()
                .iter()
                .find(|mesh| !mesh.indices.is_empty())
                .map(|mesh| mesh.color)
        })
    }

    /// Interaction-only edge wire for this volume entity (#1030).
    ///
    /// ACIS solids carry their B-rep feature edges in `edge_verts` (a flat
    /// line list of endpoint pairs). Plain meshes (OBJ imports, SubD meshes,
    /// polyface meshes) leave it empty — their visible wireframe is the
    /// triangle edges of the highest-detail LOD, exactly what the mesh GPU
    /// pipeline draws when `edge_verts` is empty — so derive the pairs from
    /// LOD 0's index buffer the same way. Instanced (block-local) sources are
    /// resolved through `geometry_lods` / `geometry_edges`; the caller applies
    /// the INSERT transform when it expands the block, like any other wire.
    pub fn snap_edge_wire(&self, name: String) -> Option<WireModel> {
        let color = self.display_color().unwrap_or([0.8, 0.8, 0.85, 1.0]);
        let (edges, edges_low) = self.geometry_edges();
        if !edges.is_empty() {
            let at = |index: usize| -> [f64; 3] {
                let high = edges[index];
                let low = edges_low.get(index).copied().unwrap_or([0.0; 3]);
                [
                    high[0] as f64 + low[0] as f64,
                    high[1] as f64 + low[1] as f64,
                    high[2] as f64 + low[2] as f64,
                ]
            };
            return edge_snap_wire(
                name,
                color,
                false,
                (0..edges.len() / 2).map(|pair| {
                    let base = pair * 2;
                    (at(base), at(base + 1))
                }),
            );
        }
        let mesh = self
            .geometry_lods()
            .iter()
            .find(|lod| !lod.indices.is_empty())?;
        let verts = &mesh.verts;
        let at = |index: usize| -> Option<[f64; 3]> {
            let high = verts.get(index)?;
            let low = mesh.verts_low.get(index).copied().unwrap_or([0.0; 3]);
            Some([
                high[0] as f64 + low[0] as f64,
                high[1] as f64 + low[1] as f64,
                high[2] as f64 + low[2] as f64,
            ])
        };
        let pairs = mesh
            .indices
            .chunks_exact(3)
            .filter_map(|triangle| {
                Some((
                    at(triangle[0] as usize)?,
                    at(triangle[1] as usize)?,
                    at(triangle[2] as usize)?,
                ))
            })
            .flat_map(|(a, b, c)| [(a, b), (b, c), (c, a)]);
        edge_snap_wire(name, color, false, pairs)
    }
}
