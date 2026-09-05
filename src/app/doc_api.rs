// DocApi v2 backend: implement `ocs_doc_api::backend::DocApiBackend` for
// `HostSession` (plan Â§6/Â§8, decision #11 â€” thin host hook, executor logic in
// the versioned crate). Maps each backend method to already-present scene/app
// primitives; no new geometry code.

use acadrust::entities::Solid3D;
use acadrust::{EntityType, Handle};
use ocs_doc_api::backend::{DocApiBackend, KernelBody};
use ocs_doc_api::{
    Aabb, ApiError, ApiResult, Curve2Spec, EntityView, GeometryErrorKind, GeometryRevision, ObjectId,
    PlacementSpec,
};

use crate::scene::convert::acis_export;
use crate::scene::model::solid_model;

use super::plugin_host::HostSession;

/// Entry point called by the `HostApi::doc_api_dispatch` override (below) and by
// the in-process path. Deserializes the envelope, runs the crate executor,
// serializes the `Receipt` (or `ApiError`) back to bytes.
pub fn execute_doc_api(host: &mut HostSession<'_>, _tab_id: u64, bytes: &[u8]) -> Result<Vec<u8>, String> {
    use ocs_doc_api::{DocApiEnvelope, EnvelopeBody};
    let envelope: DocApiEnvelope = bincode::deserialize(bytes)
        .map_err(|e| format!("DocApiEnvelope deserialize: {e}"))?;
    let result: ApiResult<ocs_doc_api::Receipt> = match envelope.body {
        EnvelopeBody::Op(op) => ocs_doc_api::executor::apply_op(host, op),
        EnvelopeBody::Queries(qs) => ocs_doc_api::executor::apply_queries(host, qs),
    };
    bincode::serialize(&result).map_err(|e| format!("Receipt serialize: {e}"))
}

fn obj_to_handle(id: ObjectId) -> Handle {
    Handle::new(id.as_u64())
}
fn handle_to_obj(h: Handle) -> ObjectId {
    ObjectId::from_u64(h.value())
}

impl DocApiBackend for HostSession<'_> {
    fn resolve_body(&mut self, id: ObjectId) -> ApiResult<KernelBody> {
        let handle = obj_to_handle(id);
        // Lift-on-miss: populate the solid_models cache from AcisData if absent.
        self.scene_mut().restore_solid_models(&[handle]);
        self.scene()
            .solid_models
            .get(&handle)
            .cloned()
            .ok_or(ApiError::UnknownId(id))
    }

    fn store_solid(&mut self, body: &KernelBody) -> ApiResult<ObjectId> {
        // Pre-flight the display prep BEFORE committing the entity: if the mesh
        // cannot be produced, fail now — no entity, no rollback, no revision bump
        // (plan review: store_solid rollback moved the epoch on failure).
        let scene = self.scene_mut();
        let Some(display) = scene.prepare_solid_model_display(Handle::NULL, body) else {
            return Err(ApiError::geometry(GeometryErrorKind::Other, "prepare_solid_model_display failed"));
        };
        // Mirror `add_solid_model` (history-free v1): edge wires + SAT + entity.
        let mut inner = Solid3D::new();
        inner.common.plotstyle_flags = 2;
        inner.wires = solid_model::edge_wires(body);
        let document = acis_export::solid_to_sat(body)
            .ok_or_else(|| ApiError::geometry(GeometryErrorKind::Acis, "solid_to_sat failed"))?;
        inner.set_sat_document(&document);
        let handle = self
            .commit_entity_handle(EntityType::Solid3D(inner))
            .ok_or_else(|| ApiError::geometry(GeometryErrorKind::Other, "commit_entity_handle failed"))?;
        // register_prepared_solid_model cannot fail once the display prep succeeded,
        // so there is no post-commit rollback path. Register the live B-rep so
        // booleans/transforms see it (decision #7).
        self.scene_mut().register_prepared_solid_model(handle, body.clone(), display);
        Ok(handle_to_obj(handle))
    }

    fn update_solid(&mut self, id: ObjectId, body: &KernelBody) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        // Rebuild the Solid3D entity with fresh SAT + wires, preserving the handle.
        let Some(existing) = self.document().get_entity(handle).cloned() else {
            return Err(ApiError::UnknownId(id));
        };
        let EntityType::Solid3D(mut inner) = existing else {
            return Err(ApiError::Unsupported("update_solid on non-solid entity".into()));
        };
        inner.wires = solid_model::edge_wires(body);
        let document = acis_export::solid_to_sat(body)
            .ok_or_else(|| ApiError::geometry(GeometryErrorKind::Acis, "solid_to_sat failed"))?;
        inner.set_sat_document(&document);
        // Pre-flight the display prep before mutating (fail without touching state).
        let Some(display) = self.scene_mut().prepare_solid_model_display(handle, body) else {
            return Err(ApiError::geometry(GeometryErrorKind::Other, "prepare_solid_model_display failed"));
        };
        if !self.scene_mut().update_entity(EntityType::Solid3D(inner)) {
            return Err(ApiError::UnknownId(id));
        }
        // Refresh the live cache (cannot fail after a successful pre-flight).
        self.scene_mut().register_prepared_solid_model(handle, body.clone(), display);
        Ok(())
    }

    fn add_curve(&mut self, spec: &Curve2Spec) -> ApiResult<ObjectId> {
        let entity = crate::app::doc_api_convert::curve_spec_to_entity(spec)?;
        let handle = self.scene_mut().add_entity(entity);
        Ok(handle_to_obj(handle))
    }

    fn add_insert(&mut self, spec: &ocs_doc_api::ops::InsertSpec) -> ApiResult<ObjectId> {
        // Validate the referenced block exists before committing.
        if self.document().block_records.get(&spec.block_name).is_none() {
            return Err(ApiError::validation(
                "CreateInsert",
                format!("unknown block {:?}", spec.block_name),
            ));
        }
        let mut ins = acadrust::entities::Insert::new(
            spec.block_name.clone(),
            acadrust::types::Vector3::new(spec.insert_point[0], spec.insert_point[1], spec.insert_point[2]),
        );
        ins.rotation = spec.rotation;
        ins.set_x_scale(spec.scale);
        ins.set_y_scale(spec.scale);
        ins.set_z_scale(spec.scale);
        let handle = self.scene_mut().add_entity(EntityType::Insert(ins));
        Ok(handle_to_obj(handle))
    }

    fn add_vertex(&mut self, id: ObjectId, at: usize, point: [f64; 3]) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        let Some(entity) = self.document().get_entity(handle).cloned() else {
            return Err(ApiError::UnknownId(id));
        };
        let EntityType::LwPolyline(mut pl) = entity else {
            return Err(ApiError::Unsupported("AddVertex is only implemented for polylines".into()));
        };
        if at > pl.vertices.len() {
            return Err(ApiError::validation(
                "AddVertex",
                format!("index {at} out of range ({} vertices)", pl.vertices.len()),
            ));
        }
        pl.vertices.insert(
            at,
            acadrust::entities::LwVertex::from_coords(point[0], point[1]),
        );
        if !self.scene_mut().update_entity(EntityType::LwPolyline(pl)) {
            return Err(ApiError::Unsupported(format!("entity {id:?} is on a locked layer")));
        }
        Ok(())
    }

    fn remove_entity(&mut self, id: ObjectId) -> ApiResult<bool> {
        let handle = obj_to_handle(id);
        if !self.entity_exists(id) {
            return Ok(false);
        }
        // Locked layers refuse deletion; surface that as an error so the executor
        // does not treat a no-op as success (plan review: erase path).
        if self.scene().is_layer_locked(handle) {
            return Err(ApiError::Unsupported(format!("entity {id:?} is on a locked layer")));
        }
        // erase_entities clears the entity + solid_models/meshes/hatches and
        // records the undo delta; the single publish happens at finalize_op.
        self.scene_mut().erase_entities(&[handle]);
        Ok(self.document().get_entity(handle).is_none())
    }

    fn ensure_transformable(&mut self, id: ObjectId) -> ApiResult<()> {
        // Pre-validating this lets TransformMany be all-or-nothing. Solids and the
        // supported 2D families are transformable; anything else errors up front.
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        let ok = matches!(
            entity,
            EntityType::Solid3D(_)
                | EntityType::Line(_)
                | EntityType::Circle(_)
                | EntityType::Arc(_)
                | EntityType::Ellipse(_)
                | EntityType::Spline(_)
                | EntityType::Ray(_)
                | EntityType::XLine(_)
                | EntityType::Insert(_)
                | EntityType::Point(_)
                | EntityType::LwPolyline(_)
        );
        if ok {
            Ok(())
        } else {
            Err(ApiError::Unsupported("transform is not supported for this entity family".into()))
        }
    }

    fn can_remove(&self, id: ObjectId) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        if !self.entity_exists(id) {
            return Err(ApiError::UnknownId(id));
        }
        if self.scene().is_layer_locked(handle) {
            return Err(ApiError::Unsupported(format!("entity {id:?} is on a locked layer")));
        }
        Ok(())
    }

    fn get_entity(&mut self, id: ObjectId) -> ApiResult<EntityView> {
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
            .ok_or(ApiError::UnknownId(id))?;
        Ok(EntityView {
            id,
            kind: crate::app::doc_api_convert::entity_kind_name(entity).to_string(),
            bounds: self.bounds(id).ok(),
        })
    }

    fn transform_entity(&mut self, id: ObjectId, placement: &PlacementSpec) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        if !self.entity_exists(id) {
            return Err(ApiError::UnknownId(id));
        }
        // Solids go through the kernel transform (re-embed SAT). 2D entities are
        // transformed via acadrust geometry and updated in place.
        if matches!(self.document().get_entity(handle), Some(EntityType::Solid3D(_))) {
            let body = self.resolve_body(id)?;
            let place = kernel_placement(placement);
            let moved = cadkernel::brep::transform(&body, &place)
                .ok_or_else(|| ApiError::geometry(GeometryErrorKind::InvalidInput, "transform failed"))?;
            self.update_solid(id, &moved)
        } else {
            let entity = self
                .document()
                .get_entity(handle)
                .cloned()
                .ok_or(ApiError::UnknownId(id))?;
            let moved = crate::app::doc_api_convert::transform_entity_geometry(&entity, placement)?;
            if !self.scene_mut().update_entity(moved) {
                return Err(ApiError::Unsupported(format!("entity {id:?} is on a locked layer")));
            }
            Ok(())
        }
    }

    fn profile_curves(&self, id: ObjectId) -> ApiResult<Vec<cadkernel::geom2d::Curve>> {
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        crate::app::doc_api_convert::entity_to_profile_curves(entity)
    }

    fn bounds(&mut self, id: ObjectId) -> ApiResult<Aabb> {
        let handle = obj_to_handle(id);
        // Lift-on-miss for solids (consistent with volume/centroid): resolve_body
        // repopulates a cold solid_models cache from SAT. A solid whose cache entry
        // was dropped (e.g. by a host-side update_entity) no longer misreports
        // `Unsupported` (plan review).
        if matches!(self.document().get_entity(handle), Some(EntityType::Solid3D(_))) {
            let body = self.resolve_body(id)?;
            let bb = cadkernel::brep::body_bounds(&body)
                .ok_or_else(|| ApiError::geometry(GeometryErrorKind::Empty, "no bounds"))?;
            return Ok(Aabb { min: bb.min, max: bb.max });
        }
        crate::app::doc_api_convert::entity_bounds(self.document().get_entity(handle), id)
    }

    fn centroid(&mut self, id: ObjectId) -> ApiResult<[f64; 3]> {
        let handle = obj_to_handle(id);
        // Serve from the scene's render-mesh metrics cache when present (avoids a
        // full body tessellation + B-rep clone per query, plan review); fall back
        // to the kernel mesh when the cache is cold.
        if let Some(metrics) = self.scene().meshes.get(&handle).map(|m| m.metrics) {
            if metrics.volume.abs() > 0.0 {
                return Ok(metrics.centroid);
            }
        }
        let body = self.resolve_body(id)?;
        let mesh = cadkernel::brep::mesh_body(&body, 0.5, 1e-3);
        Ok(mesh_volume_centroid(&mesh).1)
    }

    fn volume(&mut self, id: ObjectId) -> ApiResult<f64> {
        let handle = obj_to_handle(id);
        if let Some(metrics) = self.scene().meshes.get(&handle).map(|m| m.metrics) {
            if metrics.volume.abs() > 0.0 {
                return Ok(metrics.volume);
            }
        }
        let body = self.resolve_body(id)?;
        let mesh = cadkernel::brep::mesh_body(&body, 0.5, 1e-3);
        Ok(mesh_volume_centroid(&mesh).0)
    }

    fn entity_exists(&self, id: ObjectId) -> bool {
        self.document().get_entity(obj_to_handle(id)).is_some()
    }

    fn revision(&self) -> GeometryRevision {
        GeometryRevision(self.scene().geometry_epoch)
    }

    fn push_undo(&mut self, label: &str) {
        // Begin undo capture for an entity-only delta (delta_safe = true: solids
        // and curves are entity-store changes).
        self.begin_doc_api_undo(label);
    }

    fn finalize_op(&mut self) {
        // Close the delta entry + bump geometry + republish the document view.
        self.commit_doc_api_undo();
    }
}

// â”€â”€ helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn kernel_placement(p: &PlacementSpec) -> cadkernel::brep::Placement {
    cadkernel::brep::Placement {
        x_axis: p.x_axis,
        y_axis: p.y_axis,
        z_axis: p.z_axis,
        origin: p.origin,
    }
}

/// Signed volume and centroid of a closed triangle mesh via the divergence
/// theorem: V = Σ v0·(v1×v2)/6, C = Σ (v0+v1+v2)·tetra_vol / (4V). Single source
/// of truth for both (used by the cold-cache `volume`/`centroid` fallback).
fn mesh_volume_centroid(mesh: &cadkernel::brep::Mesh) -> (f64, [f64; 3]) {
    let mut vol = 0.0;
    let mut c = [0.0; 3];
    for t in &mesh.triangles {
        let (v0, v1, v2) = (mesh.positions[t[0]], mesh.positions[t[1]], mesh.positions[t[2]]);
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
        return (0.0, [0.0; 3]);
    }
    (vol, [c[0] / (4.0 * vol), c[1] / (4.0 * vol), c[2] / (4.0 * vol)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::plugin_host::HostSession;
    use crate::app::OpenCADStudio;
    use ocs_doc_api::ops::{BoolOp, SolidPrimitive};
    use ocs_doc_api::{
        DocApiEnvelope, HasId, ObjectId, Operation, Query, QueryResult, Receipt,
    };

    fn dispatch(host: &mut HostSession<'_>, env: DocApiEnvelope) -> ApiResult<Receipt> {
        let bytes = bincode::serialize(&env).unwrap();
        let out = execute_doc_api(host, 0, &bytes).expect("dispatch failed");
        bincode::deserialize(&out).expect("receipt deserialize")
    }

    fn new_id(receipt: &Receipt) -> ObjectId {
        receipt.outcome.as_ref().and_then(|o| o.new_id()).expect("no new id")
    }

    #[test]
    fn doc_api_create_boolean_query_end_to_end() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let rev0 = host.scene().geometry_epoch;

        // op 1 + 2: create two overlapping boxes (one undo step + one bump each).
        let cuboid_a = Operation::CreateSolid(SolidPrimitive::Cuboid { origin: [0.0; 3], size: [10.0; 3] });
        let cuboid_b = Operation::CreateSolid(SolidPrimitive::Cuboid { origin: [5.0; 3], size: [10.0; 3] });
        let a = new_id(&dispatch(&mut host, DocApiEnvelope::op(cuboid_a)).unwrap());
        let b = new_id(&dispatch(&mut host, DocApiEnvelope::op(cuboid_b)).unwrap());
        // Each write op advanced the geometry epoch (at least one bump per op).
        assert!(host.scene().geometry_epoch > rev0, "epoch advanced by creates");

        // op 3: intersect; erase_sources keeps the result at `a`, erases `b`.
        let intersect = Operation::SolidBoolean { op: BoolOp::Intersection, a, b, erase_sources: true };
        let lens = new_id(&dispatch(&mut host, DocApiEnvelope::op(intersect)).unwrap());
        assert_eq!(lens, a);
        assert!(host.document().get_entity(obj_to_handle(b)).is_none(), "b erased");
        assert!(host.scene().solid_models.contains_key(&obj_to_handle(a)), "result in cache");

        // query batch: bounds + volume (read-only, no bump).
        let rev_before_query = host.scene().geometry_epoch;
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![
            Query::GetBounds { id: a }, Query::GetVolume { id: a }])).unwrap();
        assert_eq!(host.scene().geometry_epoch, rev_before_query, "queries must not bump");
        let bb = match &receipt.query_results[0] {
            QueryResult::Bounds(bb) => *bb,
            other => panic!("expected bounds, got {other:?}"),
        };
        // [0,10]^3 intersect [5,15]^3 = [5,10]^3.
        assert!((bb.min[0] - 5.0).abs() < 1e-4 && (bb.max[0] - 10.0).abs() < 1e-4, "bounds {bb:?}");
        let vol = match &receipt.query_results[1] {
            QueryResult::Volume(v) => *v,
            other => panic!("expected volume, got {other:?}"),
        };
        assert!((vol - 125.0).abs() < 1.0, "volume {vol}");
        let _ = HasId::id(&lens);
    }

    #[test]
    fn doc_api_transform_many_with_stale_id_fails_all_or_nothing() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // A transformable solid, plus a stale id (always non-transformable -> UnknownId).
        let mk_solid = Operation::CreateSolid(SolidPrimitive::Cuboid { origin: [0.0; 3], size: [10.0; 3] });
        let solid = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_solid)).unwrap());
        let stale = ObjectId::from_u64(0xFFFF_FFFF);
        let rev_before = host.scene().geometry_epoch;

        // TransformMany over [solid, stale]: the stale id makes it fail BEFORE any
        // mutation (all-or-nothing), so the epoch must not move and no undo is recorded.
        let op = Operation::TransformMany {
            ids: vec![solid, stale],
            placement: ocs_doc_api::PlacementSpec::at([5.0, 0.0, 0.0]),
        };
        let err = dispatch(&mut host, DocApiEnvelope::op(op)).unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. } | ApiError::UnknownId(_)), "{err:?}");
        assert_eq!(host.scene().geometry_epoch, rev_before, "no mutation on rejected TransformMany");
    }

    #[test]
    fn doc_api_query_batch_over_cap_is_rejected() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let over = ocs_doc_api::ops::BULK_ITEM_CAP + 1;
        let queries: Vec<Query> = (0..over).map(|_| Query::GetGeometryRevision).collect();
        let err = dispatch(&mut host, DocApiEnvelope::queries(queries)).unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. }), "{err:?}");
    }

    // ── Phase 0: outstanding supported-family methods ──────────────────────

    #[test]
    fn phase0_add_vertex_to_polyline() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let mk_poly = Operation::CreateCurve(Curve2Spec::Polyline {
            points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]],
            closed: false,
        });
        let poly = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_poly)).unwrap());
        // Insert a vertex at index 1.
        dispatch(&mut host, DocApiEnvelope::op(Operation::AddVertex {
            id: poly, at: 1, point: [5.0, 0.0, 0.0] })).unwrap();
        // The polyline now has 4 vertices with the inserted one at index 1.
        let handle = obj_to_handle(poly);
        let Some(EntityType::LwPolyline(pl)) = host.document().get_entity(handle) else {
            panic!("polyline not found");
        };
        assert_eq!(pl.vertices.len(), 4);
        assert!((pl.vertices[1].location.x - 5.0).abs() < 1e-9);
        // Out-of-range index is a validation error, not a panic.
        let err = dispatch(&mut host, DocApiEnvelope::op(Operation::AddVertex {
            id: poly, at: 99, point: [0.0, 0.0, 0.0] })).unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. }), "{err:?}");
    }

    #[test]
    fn phase0_extrude_rectangular_profile_to_solid() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // A closed rectangular 2x3 profile in XY.
        let mk_profile = Operation::CreateCurve(Curve2Spec::Polyline {
            points: vec![[0.0,0.0,0.0],[2.0,0.0,0.0],[2.0,3.0,0.0],[0.0,3.0,0.0]],
            closed: true,
        });
        let profile = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_profile)).unwrap());
        // Extrude +Z by 5 -> a 2x3x5 box = volume 30.
        let solid = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::Extrude {
            profile, direction: [0.0, 0.0, 5.0] })).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetVolume { id: solid }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Volume(v) => assert!((*v - 30.0).abs() < 1.0, "extrude volume {v}"),
            other => panic!("expected volume, got {other:?}"),
        }
    }

    #[test]
    fn phase0_revolve_profile_to_solid() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // A 1-wide, 2-tall rectangle offset 1 from the Y axis; revolve about the Y
        // axis by 2*pi -> a cylinder-ish annulus (outer r=2, inner r=1, h=2): pi*(4-1)*2 = 6pi ≈ 18.85.
        let mk_profile = Operation::CreateCurve(Curve2Spec::Polyline {
            points: vec![[1.0,0.0,0.0],[2.0,0.0,0.0],[2.0,2.0,0.0],[1.0,2.0,0.0]],
            closed: true,
        });
        let profile = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_profile)).unwrap());
        let result = dispatch(&mut host, DocApiEnvelope::op(Operation::Revolve {
            profile,
            axis: ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            angle: std::f64::consts::TAU,
        }));
        // Revolve may be geometry-sensitive; assert it either produces a positive
        // volume or surfaces a structured geometry error (no panic).
        match result {
            Ok(receipt) => {
                let id = receipt.outcome.and_then(|o| o.new_id()).unwrap();
                let v = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetVolume { id }])).unwrap();
                match &v.query_results[0] {
                    QueryResult::Volume(vol) => assert!(*vol > 0.0, "revolve volume {vol}"),
                    other => panic!("expected volume, got {other:?}"),
                }
            }
            Err(ApiError::Geometry { .. } | ApiError::Validation { .. }) => {}
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn phase0_non_solid_transform_line_and_circle() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let mk_line = Operation::CreateCurve(Curve2Spec::Line { start: [0.0; 3], end: [10.0; 3] });
        let line = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_line)).unwrap());
        // Translate the line by +5 in X: bounds shift by +5.
        dispatch(&mut host, DocApiEnvelope::op(Operation::Transform {
            id: line, placement: ocs_doc_api::PlacementSpec::at([5.0, 0.0, 0.0]) })).unwrap();
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetBounds { id: line }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => {
                assert!((b.min[0] - 5.0).abs() < 1e-6 && (b.max[0] - 15.0).abs() < 1e-6, "{b:?}");
            }
            other => panic!("expected bounds, got {other:?}"),
        }
        // TransformMany over a mix of line + circle is now all-or-nothing (both transformable).
        let circle = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Circle { centre: [0.0; 3], radius: 2.0 }))).unwrap());
        dispatch(&mut host, DocApiEnvelope::op(Operation::TransformMany {
            ids: vec![line, circle],
            placement: ocs_doc_api::PlacementSpec::at([0.0, 1.0, 0.0]),
        })).unwrap();
    }

    // ── Phase 2: full 2D curve families ────────────────────────────────────

    #[test]
    fn phase2_arc_ellipse_spline_create_bounds_transform() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);

        // Arc: create -> kind Arc; bounds are the coarse full-circle bounds.
        let arc = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Arc { centre: [5.0, 5.0, 0.0], radius: 4.0, start_angle: 0.0, end_angle: std::f64::consts::PI }))).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![
            Query::GetEntity { id: arc }, Query::GetBounds { id: arc }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "Arc"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Bounds(b) => assert!((b.min[0] - 1.0).abs() < 1e-6 && (b.max[0] - 9.0).abs() < 1e-6, "{b:?}"),
            other => panic!("expected bounds, got {other:?}"),
        }

        // Ellipse: create -> kind Ellipse; bounds = centre ± major-axis length.
        let ell = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Ellipse { centre: [0.0, 0.0, 0.0], major_axis: [6.0, 0.0, 0.0], ratio: 0.5, start: 0.0, end: std::f64::consts::TAU }))).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetEntity { id: ell }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "Ellipse"),
            other => panic!("expected entity, got {other:?}"),
        }

        // Spline (degree-3 cubic through 4 control points): create -> kind Spline;
        // bounds = control-point bounds. Transform by +10 X shifts bounds.
        let spline = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Spline {
                degree: 3,
                control_points: vec![[0.0,0.0,0.0],[1.0,2.0,0.0],[2.0,-2.0,0.0],[3.0,0.0,0.0]],
                knots: vec![0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0],
                weights: vec![1.0; 4],
            }))).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetBounds { id: spline }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => assert!((b.min[0] - 0.0).abs() < 1e-6 && (b.max[0] - 3.0).abs() < 1e-6, "{b:?}"),
            other => panic!("expected bounds, got {other:?}"),
        }
        dispatch(&mut host, DocApiEnvelope::op(Operation::Transform {
            id: spline, placement: ocs_doc_api::PlacementSpec::at([10.0, 0.0, 0.0]) })).unwrap();
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetBounds { id: spline }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => assert!((b.min[0] - 10.0).abs() < 1e-6, "{b:?}"),
            other => panic!("expected bounds, got {other:?}"),
        }
    }

    #[test]
    fn phase2_ray_xline_create_and_transform() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let ray = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Ray { origin: [1.0, 2.0, 0.0], direction: [1.0, 0.0, 0.0] }))).unwrap());
        let xline = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::XLine { origin: [0.0, 0.0, 0.0], direction: [0.0, 1.0, 0.0] }))).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![
            Query::GetEntity { id: ray }, Query::GetEntity { id: xline }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "Ray"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "XLine"),
            other => panic!("expected entity, got {other:?}"),
        }
        // Ray/XLine are unbounded -> GetBounds is Unsupported (not a panic).
        assert!(dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetBounds { id: ray }])).is_err());
        // Transform moves the ray's base point (no error).
        dispatch(&mut host, DocApiEnvelope::op(Operation::Transform {
            id: ray, placement: ocs_doc_api::PlacementSpec::at([0.0, 0.0, 5.0]) })).unwrap();
        let handle = obj_to_handle(ray);
        let Some(EntityType::Ray(r)) = host.document().get_entity(handle) else { panic!("ray not found") };
        assert!((r.base_point.z - 5.0).abs() < 1e-6);
    }

    // ── Phase 3: containers (block references) ───────────────────────────────

    #[test]
    fn phase3_create_insert_and_transform() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Register a block record so the insert can reference it.
        host.document_mut().block_records.add(acadrust::tables::BlockRecord::new("MyBlock")).unwrap();

        // Insert the block at (10,20,0) scale 2, rotation 0.
        let ins = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateInsert(
            ocs_doc_api::ops::InsertSpec { block_name: "MyBlock".into(), insert_point: [10.0, 20.0, 0.0], scale: 2.0, rotation: 0.0 }))).unwrap());
        let handle = obj_to_handle(ins);
        let Some(EntityType::Insert(i)) = host.document().get_entity(handle) else { panic!("insert not found") };
        assert_eq!(i.block_name, "MyBlock");
        assert!((i.insert_point.x - 10.0).abs() < 1e-9 && (i.x_scale() - 2.0).abs() < 1e-9);

        // Transform the insert by +5 in X.
        dispatch(&mut host, DocApiEnvelope::op(Operation::Transform {
            id: ins, placement: ocs_doc_api::PlacementSpec::at([5.0, 0.0, 0.0]) })).unwrap();
        let Some(EntityType::Insert(i)) = host.document().get_entity(handle) else { panic!("insert not found") };
        assert!((i.insert_point.x - 15.0).abs() < 1e-6);

        // Inserting a non-existent block is a Validation error, no entity created.
        let before = host.scene().geometry_epoch;
        let err = dispatch(&mut host, DocApiEnvelope::op(Operation::CreateInsert(
            ocs_doc_api::ops::InsertSpec { block_name: "NoSuchBlock".into(), insert_point: [0.0; 3], scale: 1.0, rotation: 0.0 }))).unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. }), "{err:?}");
        assert_eq!(host.scene().geometry_epoch, before, "no entity on unknown block");
    }

    #[test]
    fn doc_api_unknown_id_surfaces_structured_error() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let ghost = ObjectId::from_u64(0xDEAD);
        let err = dispatch(&mut host, DocApiEnvelope::op(Operation::Delete { id: ghost })).unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. } | ApiError::UnknownId(_)), "{err:?}");
    }

    // ── Roundtrip tests: create -> read back via queries -> assert geometry ──

    use ocs_doc_api::ops::{Curve2Spec, PlacementSpec};

    /// Create an entity via DocApi, then read it back via GetEntity/GetBounds and
    /// assert the geometry round-trips with the expected kind + bounds.
    #[test]
    fn roundtrip_2d_entities_line_circle_point_polyline() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);

        // Line [0,0,0]-[10,0,0]: kind + bounds round-trip.
        let line = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Line { start: [0.0, 0.0, 0.0], end: [10.0, 0.0, 0.0] }))).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![
            Query::GetEntity { id: line }, Query::GetBounds { id: line }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "Line"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Bounds(b) => {
                assert!((b.min[0] - 0.0).abs() < 1e-9 && (b.max[0] - 10.0).abs() < 1e-9, "{b:?}");
            }
            other => panic!("expected bounds, got {other:?}"),
        }

        // Circle centre (5,5,0) r=3: bounds = centre ± radius in XY.
        let circle = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Circle { centre: [5.0, 5.0, 0.0], radius: 3.0 }))).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetBounds { id: circle }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => {
                assert!((b.min[0] - 2.0).abs() < 1e-9 && (b.max[0] - 8.0).abs() < 1e-9, "{b:?}");
            }
            other => panic!("expected bounds, got {other:?}"),
        }

        // Point at (1,2,3): degenerate bounds.
        let point = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Point { position: [1.0, 2.0, 3.0] }))).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetBounds { id: point }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => assert_eq!(b.min, [1.0, 2.0, 3.0]),
            other => panic!("expected bounds, got {other:?}"),
        }

        // Closed polyline (unit square in XY): bounds = [0,1]^2 at z=0.
        let poly = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateCurve(
            Curve2Spec::Polyline {
                points: vec![[0.0,0.0,0.0],[1.0,0.0,0.0],[1.0,1.0,0.0],[0.0,1.0,0.0]],
                closed: true,
            }))).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetBounds { id: poly }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => {
                assert!((b.min[0] - 0.0).abs() < 1e-9 && (b.max[1] - 1.0).abs() < 1e-9, "{b:?}");
            }
            other => panic!("expected bounds, got {other:?}"),
        }
    }

    /// Geometric methods round-trip: transform moves a solid (bounds shift);
    /// union/subtract produce the expected mass/volume relationships.
    #[test]
    fn roundtrip_geometric_methods_transform_and_booleans() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);

        // Two overlapping boxes [0,10]^3 and [5,15]^3.
        let a = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateSolid(
            SolidPrimitive::Cuboid { origin: [0.0; 3], size: [10.0; 3] }))).unwrap());
        let b = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateSolid(
            SolidPrimitive::Cuboid { origin: [5.0; 3], size: [10.0; 3] }))).unwrap());

        // Intersection FIRST: [0,10]^3 ∩ [5,15]^3 = [5,10]^3 = 125.
        let intersect_op = Operation::SolidBoolean { op: BoolOp::Intersection, a, b, erase_sources: true };
        let inter = new_id(&dispatch(&mut host, DocApiEnvelope::op(intersect_op)).unwrap());
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetVolume { id: inter }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Volume(v) => assert!((*v - 125.0).abs() < 1.0, "intersection vol {v}"),
            other => panic!("expected volume, got {other:?}"),
        }

        // THEN transform the result by +100 in X (a real, non-identity move): its
        // bounds must shift by exactly +100 in X while Y/Z stay at [5,10].
        let move_op = Operation::Transform { id: inter, placement: PlacementSpec::at([100.0, 0.0, 0.0]) };
        dispatch(&mut host, DocApiEnvelope::op(move_op)).unwrap();
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::GetBounds { id: inter }])).unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(bb) => {
                assert!((bb.min[0] - 105.0).abs() < 1e-4 && (bb.max[0] - 110.0).abs() < 1e-4, "{bb:?}");
                assert!((bb.min[1] - 5.0).abs() < 1e-4 && (bb.max[1] - 10.0).abs() < 1e-4, "{bb:?}");
            }
            other => panic!("expected bounds, got {other:?}"),
        }
    }

    /// DWG roundtrip: create two intersected solids, write the document to a DWG
    /// file, read it back, and assert the intersected solid survives with valid
    /// ACIS data (the exact path `restore_solid_models` re-lifts on load).
    #[test]
    fn roundtrip_intersected_solids_through_dwg_file() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);

        // Two overlapping boxes -> intersect -> the result solid (125 vol).
        let a = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateSolid(
            SolidPrimitive::Cuboid { origin: [0.0; 3], size: [10.0; 3] }))).unwrap());
        let b = new_id(&dispatch(&mut host, DocApiEnvelope::op(Operation::CreateSolid(
            SolidPrimitive::Cuboid { origin: [5.0; 3], size: [10.0; 3] }))).unwrap());
        let intersect_op = Operation::SolidBoolean { op: BoolOp::Intersection, a, b, erase_sources: true };
        let lens = new_id(&dispatch(&mut host, DocApiEnvelope::op(intersect_op)).unwrap());

        // Write the live document to a STABLE, inspectable DWG in the workspace
        // target dir so it can be opened in a CAD viewer after the test. The file
        // is kept (not deleted). If the stable path is momentarily locked (e.g. a
        // concurrent test run or a viewer holding it), fall back to a process-
        // unique path for the roundtrip so the test never flakes.
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&dir).expect("create target dir");
        let doc = host.document().clone();
        let stable = dir.join("doc_api_roundtrip_intersected.dwg");
        // Release any stale lock/handle on the stable file before rewriting.
        let _ = std::fs::remove_file(&stable);
        let path = match acadrust::io::dwg::DwgWriter::write_to_file(&stable, &doc) {
            Ok(()) => stable,
            Err(_) => {
                // Locked by another live process (e.g. a viewer) — write a unique
                // file for the roundtrip instead of failing.
                let alt = dir.join(format!("doc_api_roundtrip_intersected_{}.dwg", std::process::id()));
                acadrust::io::dwg::DwgWriter::write_to_file(&alt, &doc).expect("write DWG failed");
                alt
            }
        };
        assert!(path.exists(), "DWG was written");

        // Read it back.
        let mut reader = acadrust::io::dwg::DwgReader::from_file(&path).expect("open DWG");
        let reloaded = reader.read().expect("read DWG failed");

        // The intersected solid must be present as a Solid3D with valid ACIS data,
        // and re-lifting it must reproduce a body with the expected volume.
        let handle = obj_to_handle(lens);
        let Some(EntityType::Solid3D(solid)) = reloaded.get_entity(handle) else {
            panic!("intersected solid {handle:?} not found as Solid3D in reloaded DWG");
        };
        assert!(solid.has_acis_data(), "reloaded solid carries ACIS data");

        // Re-lift through the same SAT->kernel path used on load and check volume.
        let body = crate::scene::convert::solid3d_tess::kernel_body(solid)
            .expect("re-lift reloaded solid failed");
        let mesh = cadkernel::brep::mesh_body(&body, 0.5, 1e-3);
        let vol = mesh_volume_centroid(&mesh).0;
        assert!((vol - 125.0).abs() < 1.0, "reloaded intersection volume {vol}");

        // The DWG is intentionally kept for inspection (see `path` above).
    }
}
