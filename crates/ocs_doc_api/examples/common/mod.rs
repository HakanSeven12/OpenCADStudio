//! Shared in-memory `DocApiBackend` for the runnable examples.
//!
//! Lets the examples run without the host binary: an in-process mock document
//! (HashMap<ObjectId, Body> + a kind table) behind the `InProcess` transport.
//! Not for production use — the real backend lives in the host (`src/app/doc_api.rs`).

use std::collections::HashMap;

use ocs_doc_api::backend::{DocApiBackend, KernelBody};
use ocs_doc_api::{
    Aabb, ApiError, ApiResult, Curve2Spec, EntityView, GeometryErrorKind, GeometryRevision, ObjectId,
    PlacementSpec,
};

#[derive(Default)]
pub struct MockBackend {
    next_id: u64,
    revision: u64,
    undo_steps: u32,
    bodies: HashMap<ObjectId, KernelBody>,
    kinds: HashMap<ObjectId, String>,
    /// Stored 2D curve specs so `bounds()` works on non-solid entities too.
    curves: HashMap<ObjectId, Curve2Spec>,
    /// Stored text content for Text/MText annotations.
    text_values: HashMap<ObjectId, String>,
    /// Stored hatch boundary loops.
    hatch_boundaries: HashMap<ObjectId, Vec<Vec<[f64; 2]>>>,
    /// Stored dimension measurements.
    dimension_measurements: HashMap<ObjectId, f64>,
    /// Stored insert attributes.
    attributes_store: HashMap<ObjectId, Vec<(String, String)>>,
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
    #[allow(dead_code)]
    pub fn undo_steps(&self) -> u32 {
        self.undo_steps
    }
}

/// Coarse bounds for a stored 2D curve spec (mirrors the host's entity_bounds).
fn curve_bounds(spec: &Curve2Spec) -> Aabb {
    match spec {
        Curve2Spec::Line { start, end } => Aabb {
            min: [
                start[0].min(end[0]),
                start[1].min(end[1]),
                start[2].min(end[2]),
            ],
            max: [
                start[0].max(end[0]),
                start[1].max(end[1]),
                start[2].max(end[2]),
            ],
        },
        Curve2Spec::Circle { centre, radius } => Aabb {
            min: [centre[0] - radius, centre[1] - radius, centre[2]],
            max: [centre[0] + radius, centre[1] + radius, centre[2]],
        },
        Curve2Spec::Point { position } => Aabb { min: *position, max: *position },
        Curve2Spec::Polyline { points, .. } => {
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
        Curve2Spec::Arc { centre, radius, .. } => Aabb {
            min: [centre[0] - radius, centre[1] - radius, centre[2]],
            max: [centre[0] + radius, centre[1] + radius, centre[2]],
        },
        Curve2Spec::Ellipse { centre, major_axis, .. } => {
            let r = (major_axis[0] * major_axis[0] + major_axis[1] * major_axis[1] + major_axis[2] * major_axis[2]).sqrt();
            Aabb { min: [centre[0] - r, centre[1] - r, centre[2] - r], max: [centre[0] + r, centre[1] + r, centre[2] + r] }
        }
        Curve2Spec::Spline { control_points, .. } => {
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for p in control_points {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
            Aabb { min, max }
        }
        Curve2Spec::Ray { origin, .. } | Curve2Spec::XLine { origin, .. } => Aabb { min: *origin, max: *origin },
    }
}

impl DocApiBackend for MockBackend {
    fn resolve_body(&mut self, id: ObjectId) -> ApiResult<KernelBody> {
        Ok(self.body(id)?.clone())
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
    fn add_viewport(&mut self, _spec: &ocs_doc_api::ops::ViewportSpec) -> ApiResult<ObjectId> {
        Ok(self.alloc("Viewport"))
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
    fn dimension_measurement(&self, id: ObjectId) -> ApiResult<f64> {
        self.dimension_measurements.get(&id).copied().ok_or(ApiError::UnknownId(id))
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
        // Solids via the kernel body; 2D curves from their stored spec.
        if let Some(body) = self.bodies.get(&id) {
            let bb = cadkernel::brep::body_bounds(body)
                .ok_or_else(|| ApiError::geometry(GeometryErrorKind::Empty, "no bounds"))?;
            return Ok(Aabb { min: bb.min, max: bb.max });
        }
        if let Some(spec) = self.curves.get(&id) {
            return Ok(curve_bounds(spec));
        }
        Err(ApiError::UnknownId(id))
    }
    fn centroid(&mut self, id: ObjectId) -> ApiResult<[f64; 3]> {
        let body = self.body(id)?;
        Ok(mesh_vc(&cadkernel::brep::mesh_body(body, 0.5, 1e-3)).1)
    }
    fn volume(&mut self, id: ObjectId) -> ApiResult<f64> {
        let body = self.body(id)?;
        Ok(mesh_vc(&cadkernel::brep::mesh_body(body, 0.5, 1e-3)).0)
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

/// Signed volume + centroid of a closed mesh — delegates to the crate's single
/// source of truth (divergence theorem) so the example mock can't drift.
fn mesh_vc(mesh: &cadkernel::brep::Mesh) -> (f64, [f64; 3]) {
    ocs_doc_api::geom::mesh_volume_centroid(mesh)
}
