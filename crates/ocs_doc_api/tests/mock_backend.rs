//! Unit tests over an in-memory mock `DocApiBackend` (plan Â§11.1): no host.
//! Covers the Â§9 examples: per-op atomicity, boolean erase_sources, OpGroup
//! compensation, bulk all-or-nothing, revision bumps, read-only query batching,
//! and stale-handle `UnknownId` semantics.

use std::collections::HashMap;
use std::sync::Arc;

use ocs_doc_api::backend::{DocApiBackend, KernelBody};
use ocs_doc_api::{
    Aabb, ApiError, ApiResult, Curve2Spec, DocApi, EntityView, GeometryErrorKind, GeometryRevision,
    HasId, ObjectId, PlacementSpec,
};

/// Coarse bounds for a stored 2D curve spec (mirrors the host's entity_bounds and
/// the example mock — all three must answer curve bounds identically).
fn curve_bounds(spec: &Curve2Spec) -> Aabb {
    match spec {
        Curve2Spec::Line { start, end } => Aabb {
            min: [start[0].min(end[0]), start[1].min(end[1]), start[2].min(end[2])],
            max: [start[0].max(end[0]), start[1].max(end[1]), start[2].max(end[2])],
        },
        Curve2Spec::Circle { centre, radius } | Curve2Spec::Arc { centre, radius, .. } => Aabb {
            min: [centre[0] - radius, centre[1] - radius, centre[2]],
            max: [centre[0] + radius, centre[1] + radius, centre[2]],
        },
        Curve2Spec::Point { position } => Aabb { min: *position, max: *position },
        Curve2Spec::Polyline { points, .. } | Curve2Spec::Spline { control_points: points, .. } => {
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for p in points {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
            Aabb { min, max }
        }
        Curve2Spec::Ellipse { centre, major_axis, .. } => {
            let r = (major_axis[0] * major_axis[0] + major_axis[1] * major_axis[1] + major_axis[2] * major_axis[2]).sqrt();
            Aabb { min: [centre[0] - r, centre[1] - r, centre[2] - r], max: [centre[0] + r, centre[1] + r, centre[2] + r] }
        }
        Curve2Spec::Ray { origin, .. } | Curve2Spec::XLine { origin, .. } => Aabb { min: *origin, max: *origin },
    }
}

/// In-memory mock backend: HashMap<ObjectId, Body> + a tiny entity table.
/// Tracks undo steps + revision bumps to assert per-op semantics.
#[derive(Default)]
struct MockBackend {
    next_id: u64,
    revision: u64,
    undo_steps: u32,
    bodies: HashMap<ObjectId, KernelBody>,
    kinds: HashMap<ObjectId, String>,
    /// Stored text content for Text/MText annotations.
    text_values: HashMap<ObjectId, String>,
    /// Stored hatch boundary loops.
    hatch_boundaries: HashMap<ObjectId, Vec<Vec<[f64; 2]>>>,
    /// Stored dimension measurements.
    dimension_measurements: HashMap<ObjectId, f64>,
    /// Stored insert attributes.
    attributes_store: HashMap<ObjectId, Vec<(String, String)>>,
    /// Stored 2D curve specs so `bounds()` works on curve entities (aligned with
    /// the example mock — both mocks must answer curve bounds the same way).
    curves: HashMap<ObjectId, Curve2Spec>,
    /// Stored viewport views (target, height).
    viewport_views: HashMap<ObjectId, ([f64; 3], f64)>,
}

impl MockBackend {
    fn alloc(&mut self, kind: &str) -> ObjectId {
        self.next_id += 1;
        let id = ObjectId::from_u64(self.next_id);
        self.kinds.insert(id, kind.to_string());
        id
    }
    fn body(&self, id: ObjectId) -> ApiResult<&KernelBody> {
        self.bodies.get(&id).ok_or(ApiError::UnknownId(id))
    }
}

impl DocApiBackend for MockBackend {
    fn resolve_body(&mut self, id: ObjectId) -> ApiResult<KernelBody> {
        self.bodies.get(&id).cloned().ok_or(ApiError::UnknownId(id))
    }
    fn store_solid(&mut self, body: &KernelBody) -> ApiResult<ObjectId> {
        let id = self.alloc("Solid3D");
        self.bodies.insert(id, body.clone());
        Ok(id)
    }
    fn update_solid(&mut self, id: ObjectId, body: &KernelBody) -> ApiResult<()> {
        if self.bodies.insert(id, body.clone()).is_none() {
            return Err(ApiError::UnknownId(id));
        }
        Ok(())
    }
    fn add_curve(&mut self, spec: &Curve2Spec) -> ApiResult<ObjectId> {
        let kind = match spec {
            Curve2Spec::Line { .. } => "Line",
            Curve2Spec::Circle { .. } => "Circle",
            Curve2Spec::Polyline { .. } => "LwPolyline",
            Curve2Spec::Point { .. } => "Point",
            Curve2Spec::Arc { .. } => "Arc",
            Curve2Spec::Ellipse { .. } => "Ellipse",
            Curve2Spec::Spline { .. } => "Spline",
            Curve2Spec::Ray { .. } => "Ray",
            Curve2Spec::XLine { .. } => "XLine",
        };
        let id = self.alloc(kind);
        self.curves.insert(id, spec.clone());
        Ok(id)
    }
    fn add_insert(&mut self, spec: &ocs_doc_api::ops::InsertSpec) -> ApiResult<ObjectId> {
        if spec.block_name.is_empty() {
            return Err(ApiError::validation("CreateInsert", "empty block name"));
        }
        Ok(self.alloc("Insert"))
    }
    fn add_viewport(&mut self, spec: &ocs_doc_api::ops::ViewportSpec) -> ApiResult<ObjectId> {
        let id = self.alloc("Viewport");
        self.viewport_views.insert(id, (spec.view_target, spec.view_height));
        Ok(id)
    }
    fn add_text(&mut self, spec: &ocs_doc_api::ops::TextSpec) -> ApiResult<ObjectId> {
        let id = self.alloc("Text");
        self.text_values.insert(id, spec.value.clone());
        Ok(id)
    }
    fn add_mtext(&mut self, spec: &ocs_doc_api::ops::MTextSpec) -> ApiResult<ObjectId> {
        let id = self.alloc("MText");
        self.text_values.insert(id, spec.value.clone());
        Ok(id)
    }
    fn set_text_content(&mut self, id: ObjectId, value: &str) -> ApiResult<()> {
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.text_values.entry(id) {
            e.insert(value.to_string());
            Ok(())
        } else {
            Err(ApiError::UnknownId(id))
        }
    }
    fn text_content(&self, id: ObjectId) -> ApiResult<String> {
        self.text_values.get(&id).cloned().ok_or(ApiError::UnknownId(id))
    }
    fn add_hatch(&mut self, spec: &ocs_doc_api::ops::HatchSpec) -> ApiResult<ObjectId> {
        let id = self.alloc("Hatch");
        self.hatch_boundaries.insert(id, vec![spec.boundary.clone()]);
        Ok(id)
    }
    fn hatch_boundary(&self, id: ObjectId) -> ApiResult<Vec<Vec<[f64; 2]>>> {
        self.hatch_boundaries.get(&id).cloned().ok_or(ApiError::UnknownId(id))
    }
    fn add_dimension_linear(&mut self, spec: &ocs_doc_api::ops::DimensionSpec) -> ApiResult<ObjectId> {
        let id = self.alloc("Dimension");
        let d = ((spec.first_point[0] - spec.second_point[0]).powi(2)
            + (spec.first_point[1] - spec.second_point[1]).powi(2)
            + (spec.first_point[2] - spec.second_point[2]).powi(2))
        .sqrt();
        self.dimension_measurements.insert(id, d);
        Ok(id)
    }
    fn add_dimension_radius(&mut self, spec: &ocs_doc_api::ops::DimensionRadialSpec) -> ApiResult<ObjectId> {
        let id = self.alloc("Dimension");
        let d = ((spec.center[0] - spec.point[0]).powi(2) + (spec.center[1] - spec.point[1]).powi(2) + (spec.center[2] - spec.point[2]).powi(2)).sqrt();
        self.dimension_measurements.insert(id, d);
        Ok(id)
    }
    fn add_dimension_diameter(&mut self, spec: &ocs_doc_api::ops::DimensionRadialSpec) -> ApiResult<ObjectId> {
        self.add_dimension_radius(spec)
    }
    fn add_dimension_angular(&mut self, _spec: &ocs_doc_api::ops::DimensionAngularSpec) -> ApiResult<ObjectId> {
        let id = self.alloc("Dimension");
        self.dimension_measurements.insert(id, 90.0); // mock: 90 degrees
        Ok(id)
    }
    fn dimension_measurement(&self, id: ObjectId) -> ApiResult<f64> {
        self.dimension_measurements.get(&id).copied().ok_or(ApiError::UnknownId(id))
    }
    fn add_attribute_definition(&mut self, spec: &ocs_doc_api::ops::AttributeDefinitionSpec) -> ApiResult<ObjectId> {
        if spec.tag.is_empty() {
            return Err(ApiError::validation("CreateAttributeDefinition", "empty tag"));
        }
        Ok(self.alloc("AttributeDefinition"))
    }
    fn add_dimension_angular2ln(&mut self, _spec: &ocs_doc_api::ops::DimensionAngularSpec) -> ApiResult<ObjectId> {
        let id = self.alloc("Dimension");
        self.dimension_measurements.insert(id, 90.0); // mock: 90 degrees
        Ok(id)
    }
    fn add_table(&mut self, spec: &ocs_doc_api::ops::TableSpec) -> ApiResult<ObjectId> {
        let cols = spec.data.first().map(|r| r.len()).unwrap_or(0);
        if spec.data.is_empty() || cols == 0 || spec.data.iter().any(|r| r.len() != cols) {
            return Err(ApiError::validation("CreateTable", "table needs a non-empty rectangular grid"));
        }
        Ok(self.alloc("Table"))
    }
    fn set_attribute(&mut self, id: ObjectId, tag: &str, value: &str) -> ApiResult<()> {
        if !self.entity_exists(id) {
            return Err(ApiError::UnknownId(id));
        }
        let attrs = self.attributes_store.entry(id).or_default();
        if let Some(a) = attrs.iter_mut().find(|(t, _)| t == tag) {
            a.1 = value.to_string();
        } else {
            attrs.push((tag.to_string(), value.to_string()));
        }
        Ok(())
    }
    fn attributes(&self, id: ObjectId) -> ApiResult<Vec<(String, String)>> {
        self.attributes_store.get(&id).cloned().ok_or(ApiError::UnknownId(id))
    }
    fn block_entities(&self, _block_name: &str) -> ApiResult<Vec<EntityView>> {
        Ok(Vec::new())
    }
    fn add_raster_image(&mut self, _spec: &ocs_doc_api::ops::RasterImageSpec) -> ApiResult<ObjectId> {
        Ok(self.alloc("RasterImage"))
    }
    fn loft(&mut self, sections: &[Vec<cadkernel::geom2d::Curve>]) -> ApiResult<ObjectId> {
        if sections.len() < 2 {
            return Err(ApiError::validation("Loft", "loft needs >= 2 profiles"));
        }
        // Mock: store a unit cuboid as the loft result.
        let body = cadkernel::brep::make::cuboid([0.0, 0.0, 0.0], [1.0; 3])
            .ok_or_else(|| ApiError::geometry(GeometryErrorKind::InvalidInput, "make cuboid"))?;
        self.store_solid(&body)
    }
    fn set_viewport_view(&mut self, id: ObjectId, view_target: [f64; 3], view_height: f64) -> ApiResult<()> {
        if !self.entity_exists(id) {
            return Err(ApiError::UnknownId(id));
        }
        self.viewport_views.insert(id, (view_target, view_height));
        Ok(())
    }
    fn viewport_view(&self, id: ObjectId) -> ApiResult<([f64; 3], f64)> {
        self.viewport_views.get(&id).copied().ok_or(ApiError::UnknownId(id))
    }
    fn add_vertex(&mut self, id: ObjectId, _at: usize, _point: [f64; 3]) -> ApiResult<()> {
        if self.entity_exists(id) {
            Ok(())
        } else {
            Err(ApiError::UnknownId(id))
        }
    }
    fn remove_entity(&mut self, id: ObjectId) -> ApiResult<bool> {
        self.bodies.remove(&id);
        Ok(self.kinds.remove(&id).is_some())
    }
    fn get_entity(&mut self, id: ObjectId) -> ApiResult<EntityView> {
        let kind = self.kinds.get(&id).ok_or(ApiError::UnknownId(id))?.clone();
        let bounds = self.bodies.get(&id).and_then(|b| {
            cadkernel::brep::body_bounds(b).map(|bb| Aabb { min: bb.min, max: bb.max })
        });
        Ok(EntityView { id, kind, bounds })
    }
    fn transform_entity(&mut self, id: ObjectId, placement: &PlacementSpec) -> ApiResult<()> {
        if !self.entity_exists(id) {
            return Err(ApiError::UnknownId(id));
        }
        if let Some(body) = self.bodies.get(&id).cloned() {
            let place = cadkernel::brep::Placement {
                x_axis: placement.x_axis,
                y_axis: placement.y_axis,
                z_axis: placement.z_axis,
                origin: placement.origin,
            };
            let moved = cadkernel::brep::transform(&body, &place)
                .ok_or_else(|| ApiError::geometry(GeometryErrorKind::InvalidInput, "transform failed"))?;
            self.bodies.insert(id, moved);
        }
        Ok(())
    }
    fn profile_curves(&self, _id: ObjectId) -> ApiResult<Vec<cadkernel::geom2d::Curve>> {
        Err(ApiError::Unsupported("profiles not mocked".into()))
    }
    fn bounds(&mut self, id: ObjectId) -> ApiResult<Aabb> {
        // Solids via the kernel body; 2D curves from their stored spec (aligned
        // with the example mock — both mocks answer curve bounds identically).
        if let Some(spec) = self.curves.get(&id) {
            return Ok(curve_bounds(spec));
        }
        let body = self.body(id)?;
        let bb = cadkernel::brep::body_bounds(body)
            .ok_or_else(|| ApiError::geometry(GeometryErrorKind::Empty, "no bounds"))?;
        Ok(Aabb { min: bb.min, max: bb.max })
    }
    fn centroid(&mut self, id: ObjectId) -> ApiResult<[f64; 3]> {
        let body = self.body(id)?;
        Ok(ocs_doc_api::geom::mesh_volume_centroid(&cadkernel::brep::mesh_body(body, 0.5, 1e-3)).1)
    }
    fn volume(&mut self, id: ObjectId) -> ApiResult<f64> {
        let body = self.body(id)?;
        Ok(ocs_doc_api::geom::mesh_volume_centroid(&cadkernel::brep::mesh_body(body, 0.5, 1e-3)).0)
    }
    fn entity_exists(&self, id: ObjectId) -> bool {
        self.kinds.contains_key(&id)
    }
    fn revision(&self) -> GeometryRevision {
        GeometryRevision(self.revision)
    }
    fn push_undo(&mut self, _label: &str) {}
    fn finalize_op(&mut self) {
        self.undo_steps += 1;
        self.revision += 1;
    }
}


fn api() -> (DocApi, Arc<ocs_doc_api::transport::InProcess<MockBackend>>) {
    let tp = Arc::new(ocs_doc_api::transport::InProcess::new(MockBackend::default()));
    (DocApi::connect(tp.clone(), 0), tp)
}

#[test]
fn cuboid_intersect_cuboid_atomic_ops_and_revision() {
    let (api, _tp) = api();
    let doc = api.document(api.active_tab());
    let mut grp = ocs_doc_api::OpGroup::new();

    let rev0 = doc.revision().unwrap();
    // Two overlapping boxes â€” the kernel's own boolean test case (boolean.rs pair()).
    let block = grp.track(doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0])).unwrap();
    let other = grp.track(doc.solids().create_cuboid([5.0, 5.0, 5.0], [10.0, 10.0, 10.0])).unwrap();
    assert_eq!(doc.revision().unwrap().as_u64(), rev0.as_u64() + 2);

    let lens = match block.intersect(&other) {
        Ok(l) => { grp.commit(); l }
        Err(e) => panic!("intersect failed: {e}"),
    };
    assert_eq!(doc.revision().unwrap().as_u64(), rev0.as_u64() + 3);

    let res = doc
        .query_batch(|q| {
            q.bounds(&lens);
            q.volume(&lens);
        })
        .unwrap();
    let bb = res.bounds(0).unwrap();
    let vol = res.volume(1).unwrap();
    // Intersection of [0,10]Â³ âˆ© [5,15]Â³ = [5,10]Â³ â†’ bounds + volume 125.
    assert!((bb.min[0] - 5.0).abs() < 1e-6 && (bb.max[0] - 10.0).abs() < 1e-6, "bounds {bb:?}");
    assert!(vol > 0.0, "intersection volume must be positive");
}

#[test]
fn boolean_erase_sources_removes_second_solid() {
    let (api, tp) = api();
    let doc = api.document(api.active_tab());
    let a = doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3]).unwrap();
    let b = doc.solids().create_cuboid([5.0, 5.0, 5.0], [10.0; 3]).unwrap();
    let result = a.union(&b).unwrap();
    let backend = tp.backend();
    assert!(backend.entity_exists(a.id()));
    assert!(!backend.entity_exists(b.id()));
    assert_eq!(result.id(), a.id());
}

#[test]
fn stale_handle_get_returns_error() {
    let (api, _tp) = api();
    let doc = api.document(api.active_tab());
    let ghost = ObjectId::from_u64(999_999);
    assert!(doc.entities().get(ghost).is_err());
}

#[test]
fn opgroup_compensate_deletes_created() {
    let (api, tp) = api();
    let doc = api.document(api.active_tab());
    let mut grp = ocs_doc_api::OpGroup::new();
    let a = grp.track(doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3])).unwrap();
    let b = grp.track(doc.solids().create_sphere([5.0, 5.0, 5.0], 4.0)).unwrap();
    grp.compensate(&doc).unwrap();
    let backend = tp.backend();
    assert!(!backend.entity_exists(a.id()));
    assert!(!backend.entity_exists(b.id()));
}

#[test]
fn bulk_create_many_one_undo_step() {
    let (api, tp) = api();
    let doc = api.document(api.active_tab());
    let rev0 = doc.revision().unwrap().as_u64();
    let coords: Vec<[f64; 3]> = (0..1000).map(|i| [i as f64, 0.0, 0.0]).collect();
    let pts = doc.curves().create_points(&coords).unwrap();
    assert_eq!(pts.len(), 1000);
    let backend = tp.backend();
    assert_eq!(backend.revision, rev0 + 1);
    assert_eq!(backend.undo_steps, 1);
}

#[test]
fn transform_many_with_stale_id_fails_and_applies_nothing() {
    let (api, tp) = api();
    let doc = api.document(api.active_tab());
    let a = doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3]).unwrap();
    let rev_before = doc.revision().unwrap().as_u64();
    let stale = ObjectId::from_u64(424_242);
    let err = doc
        .entities()
        .transform_many(&[a.id(), stale], PlacementSpec::at([1.0, 0.0, 0.0]))
        .unwrap_err();
    assert!(matches!(err, ApiError::Validation { .. }));
    let backend = tp.backend();
    assert_eq!(backend.revision, rev_before);
}

#[test]
fn assert_revision_guard() {
    let (api, _tp) = api();
    let doc = api.document(api.active_tab());
    let rev = doc.revision().unwrap();
    doc.assert_revision(rev).unwrap();
    doc.solids().create_cuboid([0.0, 0.0, 0.0], [1.0; 3]).unwrap();
    assert!(doc.assert_revision(rev).is_err());
}

#[test]
fn failed_boolean_leaves_inputs_live_and_no_undo() {
    let (api, tp) = api();
    let doc = api.document(api.active_tab());
    let a = doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3]).unwrap();
    // `b` is stale: boolean fails before push_undo -> no mutation, no undo step.
    let stale_solid = {
        // Build a Solid handle to a non-existent id via Entity downcast path.
        // Here we just call intersect against a deleted id.
        let tmp = doc.solids().create_sphere([5.0, 5.0, 5.0], 4.0).unwrap();
        tmp.delete().unwrap();
        tmp
    };
    let rev_before = doc.revision().unwrap().as_u64();
    assert!(a.intersect(&stale_solid).is_err());
    let backend = tp.backend();
    // `a` is still live; no undo recorded for the failed boolean.
    assert!(backend.entity_exists(a.id()));
    assert_eq!(backend.revision, rev_before);
}
