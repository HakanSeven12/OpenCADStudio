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
/// A curved face's generator, kept so a view-dependent silhouette (DISPSILH)
/// can be computed per frame — the silhouette is where the surface turns away
/// from the eye, which no baked edge can capture. World-space, post body
/// transform; base/centre points carry a double-single low half so they stay
/// precise at UTM scale like the mesh verts. Each variant also carries the
/// face's parametric extent so the silhouette is clipped to the actual face
/// rather than drawn across the whole (possibly partial) surface.
#[derive(Clone, Copy, Debug)]
pub enum CurvedGen {
    /// Cone / cylinder: two edge-on lines up the surface.
    Cone {
        base: [f32; 3],
        base_low: [f32; 3],
        axis: [f32; 3],
        /// Radial frame: `u` is the θ=0 direction, `v = axis × u`.
        u_dir: [f32; 3],
        v_dir: [f32; 3],
        /// Radius at the base (`h = 0`).
        radius: f32,
        /// `tan(half-angle)`: radius at height `h` is `radius + h * tan_a`.
        tan_a: f32,
        /// Height span along the axis the face covers (base is `h = 0`).
        h_max: f32,
        theta_min: f32,
        theta_span: f32,
        full: bool,
    },
    /// Sphere: the great circle perpendicular to the view, clipped to the
    /// face's longitude/colatitude window.
    Sphere {
        center: [f32; 3],
        center_low: [f32; 3],
        pole: [f32; 3],
        u_dir: [f32; 3],
        v_dir: [f32; 3],
        radius: f32,
        theta_min: f32,
        theta_span: f32,
        full: bool,
        phi_min: f32,
        phi_max: f32,
    },
    /// Torus: view-dependent tube silhouette, clipped to both parametric
    /// windows the face covers.
    Torus {
        center: [f32; 3],
        center_low: [f32; 3],
        axis: [f32; 3],
        u_dir: [f32; 3],
        v_dir: [f32; 3],
        major: f32,
        minor: f32,
        phi_min: f32,
        phi_span: f32,
        full: bool,
        theta_min: f32,
        theta_span: f32,
        theta_full: bool,
    },
}

#[derive(Clone, Debug)]
pub struct StoredSilhouette {
    pub viewport_id: i64,
    pub view_direction: [f32; 3],
    pub up_vector: [f32; 3],
    pub target: [f32; 3],
    pub is_perspective: bool,
    pub edge_verts: Vec<[f32; 3]>,
    pub edge_verts_low: Vec<[f32; 3]>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MeshMetrics {
    pub vertices: usize,
    pub triangles: usize,
    pub surface_area: f64,
    pub volume: f64,
    pub centroid: [f64; 3],
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
    /// Curved-face generators for per-frame silhouette (DISPSILH). Empty for a
    /// solid with no curved faces, or when silhouettes aren't wanted.
    pub curved_gens: Vec<CurvedGen>,
    /// View-specific silhouette caches stored in COMMON_3DSOLID. They are used
    /// when the decoded surface family cannot provide a live analytic
    /// silhouette for the current view.
    pub stored_silhouettes: Vec<StoredSilhouette>,
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
}

#[derive(Clone, Debug)]
pub struct MeshInstanceSource {
    pub handle: acadrust::Handle,
    pub lods: Vec<MeshModel>,
    pub edge_verts: Vec<[f32; 3]>,
    pub edge_verts_low: Vec<[f32; 3]>,
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
    let point = |index: u32| {
        let index = index as usize;
        let high = mesh.verts.get(index).copied().unwrap_or([0.0; 3]);
        let low = mesh.verts_low.get(index).copied().unwrap_or([0.0; 3]);
        [
            high[0] as f64 + low[0] as f64,
            high[1] as f64 + low[1] as f64,
            high[2] as f64 + low[2] as f64,
        ]
    };
    let mut area = 0.0;
    let mut area_centroid_numerator = [0.0; 3];
    let mut signed_volume = 0.0;
    let mut centroid_numerator = [0.0; 3];
    for triangle in mesh.indices.chunks_exact(3) {
        let a = point(triangle[0]);
        let b = point(triangle[1]);
        let c = point(triangle[2]);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let triangle_area = 0.5
            * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2])
                .sqrt();
        area += triangle_area;
        for axis in 0..3 {
            area_centroid_numerator[axis] +=
                triangle_area * (a[axis] + b[axis] + c[axis]) / 3.0;
        }
        let tetra = (
            a[0] * (b[1] * c[2] - b[2] * c[1])
                - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0])
        ) / 6.0;
        signed_volume += tetra;
        for axis in 0..3 {
            centroid_numerator[axis] += tetra * (a[axis] + b[axis] + c[axis]) / 4.0;
        }
    }
    let centroid = if signed_volume.abs() > 1e-18 {
        [
            centroid_numerator[0] / signed_volume,
            centroid_numerator[1] / signed_volume,
            centroid_numerator[2] / signed_volume,
        ]
    } else if area > 1e-18 {
        [
            area_centroid_numerator[0] / area,
            area_centroid_numerator[1] / area,
            area_centroid_numerator[2] / area,
        ]
    } else {
        [0.0; 3]
    };
    MeshMetrics {
        vertices: mesh.verts.len(),
        triangles: mesh.indices.len() / 3,
        surface_area: area,
        volume: signed_volume.abs(),
        centroid,
    }
}

impl MeshLodSet {
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
            stored_silhouettes: Vec::new(),
            metrics,
            world_aabb,
            z_aabb,
            instance_source: None,
            instance_transform: None,
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
        }));
        self.instance_transform = None;
    }
}
