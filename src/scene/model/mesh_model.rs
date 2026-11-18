// Triangle mesh model — produced by the kernel Shell/Solid tessellation.
//
// Stored alongside WireModels in the scene; rendered by the mesh pipeline
// (wgpu TriangleList with depth test, flat normals).

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
    pub triangle_material_handles: Vec<Option<document::Handle>>,
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
/// `lods[0]` is the full mesh every pick, snap, edit and measurement uses.
/// Levels 1 and 2 are display-only index buffers over its vertices
/// ([`MeshLodSet::display_lods`]); a missing level draws level 0.
/// Kernel-owned source for a view-dependent silhouette.
#[derive(Clone, Debug)]
pub struct CurvedGen {
    pub source: ocs_brep::mesh::SilhouetteSource,
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
    fn apply_mass_properties(&mut self, properties: ocs_brep::MassProperties) {
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
                ocs_brep::MassProperties {
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
    /// Fully resolved display/export layer for this placement. Geometry and
    /// instance sources remain shared and placement-independent.
    pub effective_layer: Option<std::sync::Arc<str>>,
    /// Effective AcDbMaterial resolved from entity/layer/INSERT inheritance.
    /// Geometry remains usable when it is absent; the renderer then keeps the
    /// per-mesh entity colour.
    pub material: Option<super::material_model::MeshMaterial>,
    /// Face-level ACIS material overrides, resolved from each LOD's
    /// `triangle_material_handles`. Only handles actually referenced by the
    /// tessellation are retained.
    pub face_materials:
        rustc_hash::FxHashMap<document::Handle, super::material_model::MeshMaterial>,
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
    /// A colour of its own for each of `edge_verts`, where the B-rep edge
    /// carries one; shorter or empty when none do.
    pub edge_vert_colors: Vec<Option<[f32; 4]>>,
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
    pub instance_transform: Option<document::types::Transform>,
    /// Parent INSERT selected for this rendered block instance.
    pub instance_handle: Option<document::Handle>,
    /// Effective colour after INSERT inheritance, without copying the mesh.
    pub instance_color: Option<[f32; 4]>,
    /// Colour of the edge and silhouette lines when it differs from the
    /// faces': colour 7 drawn against the background it sits on.
    pub edge_color: Option<[f32; 4]>,
    /// Precise world bounds used by the interaction index.
    pub instance_aabb: Option<[f64; 6]>,
    /// Display LODs 1 and 2 over `lods[0]`'s vertices ([`simplified_lods`]).
    /// Shared, so cloning a set does not copy them.
    pub display_lods: std::sync::Arc<[DisplayLod]>,
}

/// A coarser triangle list over a mesh's own vertex buffer: drawn in place
/// of the full triangles when the mesh is small on screen, never picked.
#[derive(Clone, Debug, Default)]
pub struct DisplayLod {
    pub indices: Vec<u32>,
    /// Aligned with `indices` triplets when the source mesh has them.
    pub triangle_material_handles: Vec<Option<document::Handle>>,
    pub triangle_colors: Vec<Option<[f32; 4]>>,
    /// Vertex and triangle counts of the mesh it was made from: a level whose
    /// source has since been re-tessellated no longer applies.
    pub source: (usize, usize),
}

#[derive(Clone, Debug)]
pub struct MeshInstanceSource {
    pub handle: document::Handle,
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
    let kernel_mesh = ocs_brep::Mesh {
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
    pub fn apply_mass_properties(&mut self, properties: ocs_brep::MassProperties) {
        self.metrics.apply_mass_properties(properties);
    }

    /// Build a set from its LODs, computing the 3D AABB.
    pub fn from_lods(lods: Vec<MeshModel>) -> Self {
        let (world_aabb, z_aabb) = compute_mesh_aabb(&lods);
        let metrics = compute_mesh_metrics(&lods);
        Self {
            lods,
            effective_layer: None,
            material: None,
            face_materials: rustc_hash::FxHashMap::default(),
            visual_style: None,
            complete: true,
            edge_verts: Vec::new(),
            edge_verts_low: Vec::new(),
            edge_vert_colors: Vec::new(),
            curved_gens: Vec::new(),
            metrics,
            world_aabb,
            z_aabb,
            instance_source: None,
            instance_transform: None,
            instance_handle: None,
            instance_color: None,
            edge_color: None,
            instance_aabb: None,
            display_lods: Default::default(),
        }
    }

    /// Builds display LODs 1 and 2 from `lods[0]` (desktop only; the web
    /// build draws every mesh at full detail).
    pub fn build_display_lods(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(mesh) = self.lods.first() {
            self.display_lods = simplified_lods(mesh).into();
        }
    }

    /// Display LOD `level` (1 or 2) when it still matches the mesh it was
    /// built from.
    pub fn display_lod(&self, level: usize) -> Option<&DisplayLod> {
        let lod = self.display_lods.get(level.checked_sub(1)?)?;
        let mesh = self.lods.first()?;
        (lod.source == (mesh.verts.len(), mesh.indices.len() / 3)).then_some(lod)
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

    pub fn prepare_instance_source(&mut self, handle: document::Handle) {
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
            |source| {
                (
                    source.edge_verts.as_slice(),
                    source.edge_verts_low.as_slice(),
                )
            },
        )
    }

    pub fn entity_handle(&self) -> Option<document::Handle> {
        self.instance_handle.or_else(|| {
            self.lods
                .first()
                .and_then(|mesh| mesh.name.parse::<u64>().ok())
                .map(document::Handle::new)
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
}

/// Meshes below this many triangles keep a single LOD: simplifying them
/// saves less than the extra index buffers cost.
pub const MIN_LOD_TRIANGLES: usize = 2048;

/// Display LODs 1 and 2 of `mesh`: about ½ and ¼ of its triangles over the
/// same vertex buffer (same vertices and normals, fewer triangles).
///
/// Solid tessellations split vertices per triangle, so corners are first
/// welded by position for the simplifier to see one surface; each kept
/// corner then goes back to the copy whose normal best fits its triangle,
/// so creases keep their own shading. Triangles are simplified per
/// (material, colour) group with each group's border locked, so faces of
/// different appearance keep their shared boundary and the per-triangle
/// attributes stay aligned. A level that would not drop at least a tenth of
/// the triangles is left out.
#[cfg(not(target_arch = "wasm32"))]
pub fn simplified_lods(mesh: &MeshModel) -> Vec<DisplayLod> {
    let triangles = mesh.indices.len() / 3;
    if triangles < MIN_LOD_TRIANGLES
        || mesh.verts.is_empty()
        || mesh.indices.iter().any(|&i| i as usize >= mesh.verts.len())
    {
        return Vec::new();
    }
    // `weld[i]` is the first vertex at vertex i's position; `copies` lists
    // every vertex grouped by that welded one. The simplifier gets only the
    // welded positions (`dense`): copies left in its buffer at the same
    // place would read as seams it may not collapse.
    let mut first_at = rustc_hash::FxHashMap::default();
    let weld: Vec<u32> = mesh
        .verts
        .iter()
        .enumerate()
        .map(|(i, v)| *first_at.entry(v.map(f32::to_bits)).or_insert(i as u32))
        .collect();
    drop(first_at);
    let mut dense_of = vec![u32::MAX; mesh.verts.len()];
    let mut dense: Vec<u32> = Vec::new();
    for (i, &w) in weld.iter().enumerate() {
        if w as usize == i {
            dense_of[i] = dense.len() as u32;
            dense.push(w);
        }
    }
    let positions: Vec<[f32; 3]> = dense.iter().map(|&i| mesh.verts[i as usize]).collect();
    let mut copies: Vec<u32> = (0..mesh.verts.len() as u32).collect();
    copies.sort_unstable_by_key(|&i| weld[i as usize]);
    let copies_of = |welded: u32| {
        let low = copies.partition_point(|&i| weld[i as usize] < welded);
        let high = copies.partition_point(|&i| weld[i as usize] <= welded);
        &copies[low..high]
    };
    let has_normals = mesh.normals.len() == mesh.verts.len();
    let unweld = |triangle: &[u32]| -> [u32; 3] {
        let p = |i: u32| glam::Vec3::from(mesh.verts[i as usize]);
        let facing = (p(triangle[1]) - p(triangle[0])).cross(p(triangle[2]) - p(triangle[0]));
        [0, 1, 2].map(|corner| {
            let welded = triangle[corner];
            if !has_normals {
                return welded;
            }
            copies_of(welded)
                .iter()
                .copied()
                .max_by(|a, b| {
                    let fit = |i: u32| glam::Vec3::from(mesh.normals[i as usize]).dot(facing);
                    fit(*a).total_cmp(&fit(*b))
                })
                .unwrap_or(welded)
        })
    };
    let materials = mesh.triangle_material_handles.len() == triangles;
    let colors = mesh.triangle_colors.len() == triangles;
    let key = |t: usize| {
        (
            materials.then(|| mesh.triangle_material_handles[t]).flatten(),
            colors
                .then(|| mesh.triangle_colors[t].map(|c| c.map(f32::to_bits)))
                .flatten(),
        )
    };
    let mut groups: Vec<(usize, Vec<u32>)> = Vec::new();
    let mut slot = rustc_hash::FxHashMap::default();
    for (t, triangle) in mesh.indices.chunks_exact(3).enumerate() {
        let at = *slot.entry(key(t)).or_insert_with(|| {
            groups.push((t, Vec::new()));
            groups.len() - 1
        });
        groups[at]
            .1
            .extend(triangle.iter().map(|&i| dense_of[weld[i as usize] as usize]));
    }
    let mut lods = Vec::new();
    let mut previous = triangles;
    for (fraction, target_error) in [(0.5_f64, 0.005_f32), (0.25, 0.02)] {
        let mut lod = DisplayLod {
            source: (mesh.verts.len(), triangles),
            ..DisplayLod::default()
        };
        for (first, indices) in &groups {
            let target = ((indices.len() as f64 * fraction) as usize / 3 * 3).max(3);
            let kept = meshopt::simplify::simplify_decoder(
                indices,
                &positions,
                target,
                target_error,
                meshopt::SimplifyOptions::LockBorder,
                None,
            );
            let count = kept.len() / 3;
            lod.indices
                .extend(kept.chunks_exact(3).flat_map(|triangle| {
                    unweld(&[triangle[0], triangle[1], triangle[2]].map(|d| dense[d as usize]))
                }));
            if materials {
                lod.triangle_material_handles
                    .extend(std::iter::repeat_n(mesh.triangle_material_handles[*first], count));
            }
            if colors {
                lod.triangle_colors
                    .extend(std::iter::repeat_n(mesh.triangle_colors[*first], count));
            }
        }
        let count = lod.indices.len() / 3;
        if count * 10 > previous * 9 {
            break;
        }
        previous = count;
        lods.push(lod);
    }
    lods
}

/// Max B-rep edge pairs read from one solid for snapping. Curved-edge
/// tessellations fan out into thousands of joints; the cap keeps the
/// interaction index and the per-move snap scan bounded.
pub const MAX_SOLID_SNAP_EDGES: usize = 2048;

/// Distinct B-rep edge segments from a high/low double-single line list
/// (mesh `edge_verts` pairs + `edge_verts_low`), feeding solid vertex
/// (endpoints) and edge-midpoint (centres) snaps.
///
/// Degenerate (zero-length) and non-finite pairs are dropped, and identical
/// pairs collapse regardless of endpoint order. Past the cap the output is
/// stride-sampled so coverage degrades gracefully instead of dropping every
/// edge past some cutoff.
pub fn solid_edge_segments(
    edge_verts: &[[f32; 3]],
    edge_verts_low: &[[f32; 3]],
) -> Vec<(glam::DVec3, glam::DVec3)> {
    fn point(high: [f32; 3], low: [f32; 3]) -> Option<glam::DVec3> {
        let p = glam::DVec3::new(
            high[0] as f64 + low[0] as f64,
            high[1] as f64 + low[1] as f64,
            high[2] as f64 + low[2] as f64,
        );
        p.is_finite().then_some(p)
    }
    fn key(p: &glam::DVec3) -> [u64; 3] {
        [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for index in 0..edge_verts.len() / 2 {
        let high = [edge_verts[2 * index], edge_verts[2 * index + 1]];
        let low = [
            edge_verts_low.get(2 * index).copied().unwrap_or([0.0; 3]),
            edge_verts_low
                .get(2 * index + 1)
                .copied()
                .unwrap_or([0.0; 3]),
        ];
        let (Some(a), Some(b)) = (point(high[0], low[0]), point(high[1], low[1])) else {
            continue;
        };
        if a.distance_squared(b) <= 1e-12 {
            continue;
        }
        let (ka, kb) = (key(&a), key(&b));
        let ordered = if ka <= kb { (ka, kb) } else { (kb, ka) };
        if !seen.insert(ordered) {
            continue;
        }
        out.push((a, b));
    }
    if out.len() > MAX_SOLID_SNAP_EDGES {
        let step = out.len().div_ceil(MAX_SOLID_SNAP_EDGES);
        out = out.into_iter().step_by(step).collect();
    }
    out
}

#[cfg(test)]
mod solid_snap_tests {
    use super::solid_edge_segments;

    fn pt(x: f32, y: f32, z: f32) -> [f32; 3] {
        [x, y, z]
    }

    #[test]
    fn shared_corners_keep_both_segments() {
        // Two edges sharing the corner (1,0,0): both pairs survive, the
        // corner itself dedups later at attach time, not here.
        let edges = vec![
            pt(0.0, 0.0, 0.0),
            pt(1.0, 0.0, 0.0),
            pt(1.0, 0.0, 0.0),
            pt(1.0, 1.0, 0.0),
        ];
        let low = vec![[0.0f32; 3]; 4];
        let segs = solid_edge_segments(&edges, &low);
        assert_eq!(segs.len(), 2, "shared corner collapsed segments: {segs:?}");
    }

    #[test]
    fn degenerate_and_non_finite_pairs_are_dropped() {
        let edges = vec![
            pt(0.0, 0.0, 0.0),
            pt(f32::NAN, 0.0, 0.0),
            pt(1.0, 0.0, 0.0),
            pt(1.0, 0.0, 0.0), // degenerate: zero length
            pt(2.0, 0.0, 0.0),
            pt(3.0, 0.0, f32::INFINITY),
            pt(4.0, 0.0, 0.0),
            pt(5.0, 0.0, 0.0),
        ];
        let low = vec![[0.0f32; 3]; 8];
        let segs = solid_edge_segments(&edges, &low);
        assert_eq!(segs.len(), 1, "expected only the valid pair: {segs:?}");
        assert!(segs[0].0.is_finite() && segs[0].1.is_finite());
    }

    #[test]
    fn output_is_capped_with_coverage() {
        let mut edges = Vec::new();
        for k in 0..2200 {
            let x = 100.0 + k as f32;
            edges.push(pt(x, 0.0, 0.0));
            edges.push(pt(x, 1.0, 0.0));
        }
        let low = vec![[0.0f32; 3]; 4400];
        let segs = solid_edge_segments(&edges, &low);
        assert!(
            segs.len() <= super::MAX_SOLID_SNAP_EDGES,
            "cap exceeded: {}",
            segs.len()
        );
        assert!(
            segs.len() >= super::MAX_SOLID_SNAP_EDGES / 2,
            "stride sampling dropped too much coverage: {}",
            segs.len()
        );
        // The first pair survives sampling (index 0 is kept).
        assert_eq!(segs[0].0, glam::DVec3::new(100.0, 0.0, 0.0));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod lod_tests {
    use super::{simplified_lods, MeshModel};

    fn mesh_of(body: &ocs_brep::Body) -> MeshModel {
        let tessellation = ocs_brep::mesh::tessellate(
            body,
            ocs_brep::mesh::TessellationTolerance::new(0.05, 1e-3),
        );
        let mesh = tessellation.mesh;
        MeshModel {
            name: "1".into(),
            verts: mesh
                .positions
                .iter()
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
                .collect(),
            verts_low: Vec::new(),
            normals: mesh
                .normals
                .iter()
                .map(|n| [n[0] as f32, n[1] as f32, n[2] as f32])
                .collect(),
            indices: mesh
                .triangles
                .iter()
                .flat_map(|t| t.map(|i| i as u32))
                .collect(),
            triangle_material_handles: Vec::new(),
            triangle_colors: Vec::new(),
            color: [1.0; 4],
            selected: false,
        }
    }

    /// Worst distance of a triangle's centroid off the exact surface, where
    /// `off` measures a point's distance from it.
    fn worst(mesh: &MeshModel, indices: &[u32], off: impl Fn(glam::DVec3) -> f64) -> f64 {
        indices
            .chunks_exact(3)
            .map(|t| {
                let p = |i: u32| glam::DVec3::from(mesh.verts[i as usize].map(f64::from));
                off((p(t[0]) + p(t[1]) + p(t[2])) / 3.0)
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_sphere_simplifies_to_half_and_quarter_within_a_small_error() {
        let body = ocs_brep::make::sphere([0.0; 3], 10.0).unwrap();
        let mesh = mesh_of(&body);
        let lods = simplified_lods(&mesh);
        assert_eq!(lods.len(), 2);
        let full = mesh.indices.len() / 3;
        let off = |p: glam::DVec3| (p.length() - 10.0).abs();
        let base = worst(&mesh, &mesh.indices, off);
        for (lod, fraction) in lods.iter().zip([0.5, 0.25]) {
            let count = lod.indices.len() / 3;
            assert!(
                (count as f64) < full as f64 * (fraction + 0.15),
                "{count} of {full} triangles"
            );
            assert_eq!(lod.source, (mesh.verts.len(), full));
            let error = worst(&mesh, &lod.indices, off);
            assert!(error < base + 0.05, "{error} vs {base}");
        }
    }

    #[test]
    fn a_torus_keeps_each_colour_group_and_its_attributes_aligned() {
        let body = ocs_brep::make::torus([0.0; 3], 20.0, 5.0).unwrap();
        let mut mesh = mesh_of(&body);
        // Colour by side of the YZ plane: two groups sharing a boundary.
        let red = Some([1.0, 0.0, 0.0, 1.0]);
        mesh.triangle_colors = mesh
            .indices
            .chunks_exact(3)
            .map(|t| (mesh.verts[t[0] as usize][0] > 0.0).then_some(red).flatten())
            .collect();
        let lods = simplified_lods(&mesh);
        assert!(!lods.is_empty());
        let full = mesh.indices.len() / 3;
        let off = |p: glam::DVec3| {
            let ring = glam::DVec2::new(p.x, p.y).length() - 20.0;
            (glam::DVec2::new(ring, p.z).length() - 5.0).abs()
        };
        let base = worst(&mesh, &mesh.indices, off);
        for lod in &lods {
            let count = lod.indices.len() / 3;
            assert!(count * 10 <= full * 9, "{count} of {full}");
            assert_eq!(lod.triangle_colors.len(), count);
            assert!(lod.triangle_material_handles.is_empty());
            let error = worst(&mesh, &lod.indices, off);
            assert!(error < base + 0.06, "{error} vs {base}");
            // Every triangle keeps its group's colour: no red triangle
            // wholly on the uncoloured side, nor the other way round.
            for (t, colour) in lod.indices.chunks_exact(3).zip(&lod.triangle_colors) {
                let xs = [t[0], t[1], t[2]].map(|i| mesh.verts[i as usize][0]);
                if colour.is_some() {
                    assert!(xs.iter().any(|x| *x > 0.0), "{xs:?}");
                } else {
                    assert!(xs.iter().any(|x| *x <= 0.0), "{xs:?}");
                }
            }
        }
    }
}
