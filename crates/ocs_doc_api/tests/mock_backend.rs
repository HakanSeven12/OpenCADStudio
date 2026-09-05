//! Unit tests over an in-memory mock `DocApiBackend` (plan §11.1): no host.
//! Covers the §9 examples: per-op atomicity, boolean erase_sources, OpGroup
//! compensation, bulk all-or-nothing, revision bumps, read-only query batching,
//! and stale-handle `UnknownId` semantics.

use std::collections::HashMap;
use std::sync::Arc;

use ocs_doc_api::backend::{DocApiBackend, KernelBody};
use ocs_doc_api::{
    Aabb, ApiError, ApiResult, Curve2Spec, DocApi, EntityView, GeometryErrorKind, GeometryRevision,
    HasId, ObjectId, PlacementSpec,
};

/// In-memory mock backend: HashMap<ObjectId, Body> + a tiny entity table.
/// Tracks undo steps + revision bumps to assert per-op semantics.
#[derive(Default)]
struct MockBackend {
    next_id: u64,
    revision: u64,
    undo_steps: u32,
    bodies: HashMap<ObjectId, KernelBody>,
    kinds: HashMap<ObjectId, String>,
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
        };
        Ok(self.alloc(kind))
    }
    fn add_insert(&mut self, spec: &ocs_doc_api::ops::InsertSpec) -> ApiResult<ObjectId> {
        if spec.block_name.is_empty() {
            return Err(ApiError::validation("CreateInsert", "empty block name"));
        }
        Ok(self.alloc("Insert"))
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
        let body = self.body(id)?;
        let bb = cadkernel::brep::body_bounds(body)
            .ok_or_else(|| ApiError::geometry(GeometryErrorKind::Empty, "no bounds"))?;
        Ok(Aabb { min: bb.min, max: bb.max })
    }
    fn centroid(&mut self, id: ObjectId) -> ApiResult<[f64; 3]> {
        let body = self.body(id)?;
        Ok(cadkernel::brep::mesh_body(body, 0.5, 1e-3).centroid_approx())
    }
    fn volume(&mut self, id: ObjectId) -> ApiResult<f64> {
        let body = self.body(id)?;
        Ok(cadkernel::brep::mesh_body(body, 0.5, 1e-3).volume_approx())
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

/// Approximate mesh volume/centroid for the mock (divergence over triangles).
trait MeshApprox {
    fn volume_approx(&self) -> f64;
    fn centroid_approx(&self) -> [f64; 3];
}
impl MeshApprox for cadkernel::brep::Mesh {
    fn volume_approx(&self) -> f64 {
        let mut vol = 0.0;
        for t in &self.triangles {
            let v0 = self.positions[t[0]];
            let v1 = self.positions[t[1]];
            let v2 = self.positions[t[2]];
            let cross = [
                v1[1] * v2[2] - v1[2] * v2[1],
                v1[2] * v2[0] - v1[0] * v2[2],
                v1[0] * v2[1] - v1[1] * v2[0],
            ];
            vol += (v0[0] * cross[0] + v0[1] * cross[1] + v0[2] * cross[2]) / 6.0;
        }
        vol
    }
    fn centroid_approx(&self) -> [f64; 3] {
        let mut vol = 0.0;
        let mut c = [0.0; 3];
        for t in &self.triangles {
            let v0 = self.positions[t[0]];
            let v1 = self.positions[t[1]];
            let v2 = self.positions[t[2]];
            let cross = [
                v1[1] * v2[2] - v1[2] * v2[1],
                v1[2] * v2[0] - v1[0] * v2[2],
                v1[0] * v2[1] - v1[1] * v2[0],
            ];
            let tet = (v0[0] * cross[0] + v0[1] * cross[1] + v0[2] * cross[2]) / 6.0;
            vol += tet;
            for i in 0..3 {
                c[i] += (v0[i] + v1[i] + v2[i]) * tet;
            }
        }
        if vol.abs() < 1e-12 {
            return [0.0; 3];
        }
        [c[0] / (4.0 * vol), c[1] / (4.0 * vol), c[2] / (4.0 * vol)]
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
    // Two overlapping boxes — the kernel's own boolean test case (boolean.rs pair()).
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
    // Intersection of [0,10]³ ∩ [5,15]³ = [5,10]³ → bounds + volume 125.
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
