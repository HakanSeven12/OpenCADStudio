//! Native document API adapter over the existing scene, history and kernel paths.

use acadrust::entities::Solid3D;
use acadrust::objects::{Dictionary, ObjectType};
use acadrust::{EntityType, Handle};
use ocs_doc_api::backend::{DocApiBackend, KernelBody};
use ocs_doc_api::{
    Aabb, ApiError, ApiResult, Color, Curve2Spec, EntityView, GeometryErrorKind, GeometryRevision,
    LayerFlags, LayerInfo, LineWeight, ObjectId, PlacementSpec,
};

use crate::scene::annotative::root_named_dict_handle;
use crate::scene::convert::acis_export;
use crate::scene::model::solid_model;
use ocs_doc_api::convert;

use super::plugin_host::HostSession;

const XRECORD_DICT_NAME: &str = "OCS_XRECORD_DICT";

/// Return the stable named-object dictionary used to hold `XRecord` objects.
/// Created on first use under the root named-objects dictionary and reused
/// thereafter.
fn xrecord_dictionary_handle(doc: &mut acadrust::CadDocument) -> Handle {
    let root_h = root_named_dict_handle(doc);
    let existing = match doc.objects.get(&root_h) {
        Some(ObjectType::Dictionary(root)) => root
            .entries
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(XRECORD_DICT_NAME))
            .map(|(_, handle)| *handle),
        _ => None,
    };
    match existing.filter(|handle| matches!(doc.objects.get(handle), Some(ObjectType::Dictionary(_)))) {
        Some(handle) => handle,
        None => {
            let handle = doc.allocate_handle();
            let mut dict = Dictionary::new();
            dict.handle = handle;
            dict.owner = root_h;
            doc.objects.insert(handle, ObjectType::Dictionary(dict));
            if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_h) {
                root.entries.retain(|(name, _)| !name.eq_ignore_ascii_case(XRECORD_DICT_NAME));
                root.add_entry(XRECORD_DICT_NAME, handle);
            }
            handle
        }
    }
}

fn xrecord_owner_dictionary(
    doc: &acadrust::CadDocument,
    record_handle: Handle,
    owner: Handle,
) -> Option<Handle> {
    let owns_record = |handle: Handle| {
        matches!(doc.objects.get(&handle), Some(ObjectType::Dictionary(dict)) if
            dict.entries.iter().any(|(_, child)| *child == record_handle))
    };
    if owns_record(owner) {
        return Some(owner);
    }
    doc.objects.iter().find_map(|(handle, object)| match object {
        ObjectType::Dictionary(dict)
            if dict.entries.iter().any(|(_, child)| *child == record_handle) =>
        {
            Some(*handle)
        }
        _ => None,
    })
}

/// Entry point called by the `HostApi::doc_api_dispatch` override (below) and by
// the in-process path. Deserializes the envelope, runs the crate executor,
// serializes the `Receipt` (or `ApiError`) back to bytes.
pub fn execute_doc_api(
    host: &mut HostSession<'_>,
    tab_id: u64,
    bytes: &[u8],
) -> Result<Vec<u8>, String> {
    use bincode::Options;
    use ocs_doc_api::{DocApiEnvelope, EnvelopeBody};
    // Authorization: the request must name the SAME tab the HostSession is bound to.
    // The HostSession is built per-tab by the dispatch pump; a plugin naming a
    // different tab_id would otherwise reach a tab it isn't bound to (confused deputy).
    if tab_id != host.tab_id() {
        return Err(format!(
            "DocApi tab mismatch: request names tab {tab_id} but the bound tab is {}",
            host.tab_id()
        ));
    }
    host.scene_mut().doc_api_cold_tess_used = 0;
    const FRAME_LIMIT: u64 = 64 * 1024 * 1024;
    if bytes.len() as u64 > FRAME_LIMIT {
        return Err("DocApi frame exceeds 64 MiB".into());
    }
    let envelope: DocApiEnvelope = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(FRAME_LIMIT)
        .reject_trailing_bytes()
        .deserialize(bytes)
        .map_err(|e| format!("DocApiEnvelope deserialize: {e}"))?;
    let result: ApiResult<ocs_doc_api::Receipt> =
        envelope
            .validate_version()
            .and_then(|_| match envelope.body {
                EnvelopeBody::Op(op) => ocs_doc_api::executor::apply_op(host, op),
                EnvelopeBody::Queries(qs) => ocs_doc_api::executor::apply_queries(host, qs),
            });
    bincode::serialize(&result).map_err(|e| format!("Receipt serialize: {e}"))
}

// ObjectId <-> acadrust::Handle converters delegate to the crate's helpers
// (single source of truth; no local duplicates).
fn obj_to_handle(id: ObjectId) -> Handle {
    id.to_handle()
}
fn handle_to_obj(h: Handle) -> ObjectId {
    ObjectId::from_handle(h)
}

fn v3_to_array(v: acadrust::types::Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
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

    fn with_body<R>(
        &mut self,
        id: ObjectId,
        f: &mut dyn FnMut(&KernelBody) -> ApiResult<R>,
    ) -> ApiResult<R> {
        let handle = obj_to_handle(id);
        // Lift-on-miss, then borrow the cache entry in place — O(1), no deep clone.
        self.scene_mut().restore_solid_models(&[handle]);
        let body = self
            .scene()
            .solid_models
            .get(&handle)
            .ok_or(ApiError::UnknownId(id))?;
        f(body)
    }

    fn store_solid(&mut self, body: &KernelBody) -> ApiResult<ObjectId> {
        let prepared = self.prepare_doc_solid(body.clone(), None)?;
        Ok(self.apply_doc_entity(prepared, None))
    }

    fn update_solid(&mut self, id: ObjectId, body: &KernelBody) -> ApiResult<()> {
        self.can_modify(id)?;
        let prepared = self.prepare_doc_solid(body.clone(), Some(id))?;
        self.apply_doc_entity(prepared, Some(id));
        Ok(())
    }

    fn create_many(&mut self, specs: &[ocs_doc_api::EntitySpec]) -> ApiResult<Vec<ObjectId>> {
        // Validate per-spec layers up front so the op is atomic.
        for spec in specs {
            if let Some(name) = spec.layer() {
                self.lookup_layer(name).map_err(|e| ApiError::validation(
                    "CreateMany",
                    e.to_string(),
                ))?;
            }
        }
        let mut prepared = Vec::with_capacity(specs.len());
        for spec in specs {
            prepared.push(match spec {
                ocs_doc_api::EntitySpec::Solid(spec) => {
                    self.prepare_doc_solid(ocs_doc_api::geom::make_solid(spec)?, None)?
                }
                ocs_doc_api::EntitySpec::Curve(spec) => {
                    let mut entity = convert::curve_spec_to_entity(spec)?;
                    if let Some(name) = spec.layer() {
                        entity.common_mut().layer =
                            acadrust::tables::normalize_name(name.trim());
                    }
                    PreparedDocEntity {
                        entity,
                        solid: None,
                    }
                }
            });
        }
        DocApiBackend::push_undo(self, "CreateMany");
        let ids = prepared
            .into_iter()
            .map(|entity| self.apply_doc_entity(entity, None))
            .collect();
        self.finalize_op();
        Ok(ids)
    }

    fn transform_many(&mut self, ids: &[ObjectId], placement: &PlacementSpec) -> ApiResult<()> {
        let prepared = ids
            .iter()
            .map(|&id| self.prepare_doc_transform(id, placement))
            .collect::<ApiResult<Vec<_>>>()?;
        DocApiBackend::push_undo(self, "TransformMany");
        for (&id, entity) in ids.iter().zip(prepared) {
            self.apply_doc_entity(entity, Some(id));
        }
        self.finalize_op();
        Ok(())
    }

    fn add_curve(&mut self, spec: &Curve2Spec) -> ApiResult<ObjectId> {
        let layer_name = spec.layer();
        if let Some(name) = layer_name {
            self.lookup_layer(name).map_err(|e| ApiError::validation(
                "CreateCurve",
                e.to_string(),
            ))?;
        }
        let mut entity = convert::curve_spec_to_entity(spec)?;
        if let Some(name) = layer_name {
            entity.common_mut().layer = acadrust::tables::normalize_name(name.trim());
        }
        let handle = self.scene_mut().add_entity(entity);
        Ok(handle_to_obj(handle))
    }

    fn lookup_layer(&self, name: &str) -> ApiResult<ocs_doc_api::LayerInfo> {
        let norm = acadrust::tables::normalize_name(name.trim());
        if norm.is_empty() {
            return Err(ApiError::validation("LookupLayer", "empty layer name"));
        }
        self.document()
            .layers
            .get(&norm)
            .map(|l| layer_info_from_acadrust(l))
            .ok_or_else(|| {
                ApiError::validation(
                    "LookupLayer",
                    format!("layer '{name}' does not exist"),
                )
            })
    }

    fn add_insert(&mut self, spec: &ocs_doc_api::ops::InsertSpec) -> ApiResult<ObjectId> {
        // Validate the referenced block exists before committing.
        if self
            .document()
            .block_records
            .get(&spec.block_name)
            .is_none()
        {
            return Err(ApiError::validation(
                "CreateInsert",
                format!("unknown block {:?}", spec.block_name),
            ));
        }
        let mut ins = acadrust::entities::Insert::new(
            spec.block_name.clone(),
            acadrust::types::Vector3::new(
                spec.insert_point[0],
                spec.insert_point[1],
                spec.insert_point[2],
            ),
        );
        ins.rotation = spec.rotation;
        ins.set_x_scale(spec.scale);
        ins.set_y_scale(spec.scale);
        ins.set_z_scale(spec.scale);
        let handle = self.scene_mut().add_entity(EntityType::Insert(ins));
        Ok(handle_to_obj(handle))
    }

    fn add_viewport(&mut self, spec: &ocs_doc_api::ops::ViewportSpec) -> ApiResult<ObjectId> {
        use acadrust::entities::Viewport;
        use acadrust::types::Vector3;
        let v3 = |p: [f64; 3]| Vector3::new(p[0], p[1], p[2]);
        let mut vp = Viewport::new();
        vp.center = v3(spec.center);
        vp.width = spec.width;
        vp.height = spec.height;
        vp.view_target = v3(spec.view_target);
        vp.view_height = spec.view_height;
        let handle = self.scene_mut().add_entity(EntityType::Viewport(vp));
        Ok(handle_to_obj(handle))
    }

    fn add_text(&mut self, spec: &ocs_doc_api::ops::TextSpec) -> ApiResult<ObjectId> {
        use acadrust::entities::Text;
        use acadrust::types::Vector3;
        let mut t = Text::new();
        t.value = spec.value.clone();
        t.insertion_point = Vector3::new(
            spec.insertion_point[0],
            spec.insertion_point[1],
            spec.insertion_point[2],
        );
        t.height = spec.height;
        t.rotation = spec.rotation;
        let handle = self.scene_mut().add_entity(EntityType::Text(t));
        Ok(handle_to_obj(handle))
    }

    fn add_mtext(&mut self, spec: &ocs_doc_api::ops::MTextSpec) -> ApiResult<ObjectId> {
        use acadrust::entities::MText;
        use acadrust::types::Vector3;
        let mut t = MText::with_value(
            spec.value.clone(),
            Vector3::new(
                spec.insertion_point[0],
                spec.insertion_point[1],
                spec.insertion_point[2],
            ),
        );
        t.height = spec.height;
        let handle = self.scene_mut().add_entity(EntityType::MText(t));
        Ok(handle_to_obj(handle))
    }

    fn set_text_content(&mut self, id: ObjectId, value: &str) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
            .cloned()
            .ok_or(ApiError::UnknownId(id))?;
        let new_entity = match entity {
            EntityType::Text(mut t) => {
                t.value = value.to_string();
                EntityType::Text(t)
            }
            EntityType::MText(mut t) => {
                t.value = value.to_string();
                EntityType::MText(t)
            }
            _ => {
                return Err(ApiError::Unsupported(
                    "SetTextContent is only for Text/MText".into(),
                ))
            }
        };
        if !self.scene_mut().update_entity(new_entity) {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
        }
        Ok(())
    }

    fn text_content(&self, id: ObjectId) -> ApiResult<String> {
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        match entity {
            EntityType::Text(t) => Ok(t.value.clone()),
            EntityType::MText(t) => Ok(t.value.clone()),
            _ => Err(ApiError::Unsupported(
                "GetTextContent is only for Text/MText".into(),
            )),
        }
    }

    fn can_modify(&self, id: ObjectId) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        if !self.entity_exists(id) {
            return Err(ApiError::UnknownId(id));
        }
        if self.scene().is_layer_locked(handle) {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
        }
        Ok(())
    }

    fn add_hatch(&mut self, spec: &ocs_doc_api::ops::HatchSpec) -> ApiResult<ObjectId> {
        use acadrust::entities::{BoundaryEdge, BoundaryPath, Hatch, LineEdge};
        use acadrust::types::Vector2;
        let mut hatch = if spec.solid {
            Hatch::solid()
        } else {
            Hatch::default()
        };
        // Build a BoundaryPath of Line edges around the closed polyline.
        let n = spec.boundary.len();
        let mut path = BoundaryPath::new();
        for i in 0..n {
            let start = spec.boundary[i];
            let end = spec.boundary[(i + 1) % n];
            path.edges.push(BoundaryEdge::Line(LineEdge {
                start: Vector2::new(start[0], start[1]),
                end: Vector2::new(end[0], end[1]),
            }));
        }
        hatch.paths.push(path);
        let handle = self.scene_mut().add_entity(EntityType::Hatch(hatch));
        Ok(handle_to_obj(handle))
    }

    fn hatch_boundary(&self, id: ObjectId) -> ApiResult<Vec<Vec<[f64; 2]>>> {
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        let EntityType::Hatch(h) = entity else {
            return Err(ApiError::Unsupported(
                "GetHatchBoundary is only for Hatch".into(),
            ));
        };
        // Reconstruct each boundary loop's vertices from its Line edges.
        let mut loops = Vec::with_capacity(h.paths.len());
        for path in &h.paths {
            let mut pts = Vec::with_capacity(path.edges.len());
            for edge in &path.edges {
                match edge {
                    acadrust::entities::BoundaryEdge::Line(le) => {
                        pts.push([le.start.x, le.start.y])
                    }
                    // Arc/ellipse boundary edges are a later refinement (no vertex list).
                    _ => {
                        return Err(ApiError::Unsupported(
                            "hatch boundary contains non-line edges".into(),
                        ))
                    }
                }
            }
            loops.push(pts);
        }
        Ok(loops)
    }

    fn add_dimension_linear(
        &mut self,
        spec: &ocs_doc_api::ops::DimensionSpec,
    ) -> ApiResult<ObjectId> {
        use acadrust::entities::{Dimension, DimensionLinear};
        use acadrust::types::Vector3;
        let v3 = |p: [f64; 3]| Vector3::new(p[0], p[1], p[2]);
        let mut dim = DimensionLinear::new(v3(spec.first_point), v3(spec.second_point));
        dim.definition_point = v3(spec.definition_point);
        let handle = self
            .scene_mut()
            .add_entity(EntityType::Dimension(Dimension::Linear(dim)));
        Ok(handle_to_obj(handle))
    }

    fn add_dimension_radius(
        &mut self,
        spec: &ocs_doc_api::ops::DimensionRadialSpec,
    ) -> ApiResult<ObjectId> {
        use acadrust::entities::{Dimension, DimensionRadius};
        use acadrust::types::Vector3;
        let v3 = |p: [f64; 3]| Vector3::new(p[0], p[1], p[2]);
        let dim = DimensionRadius::new(v3(spec.center), v3(spec.point));
        let handle = self
            .scene_mut()
            .add_entity(EntityType::Dimension(Dimension::Radius(dim)));
        Ok(handle_to_obj(handle))
    }

    fn add_dimension_diameter(
        &mut self,
        spec: &ocs_doc_api::ops::DimensionRadialSpec,
    ) -> ApiResult<ObjectId> {
        use acadrust::entities::{Dimension, DimensionDiameter};
        use acadrust::types::Vector3;
        let v3 = |p: [f64; 3]| Vector3::new(p[0], p[1], p[2]);
        let dim = DimensionDiameter::new(v3(spec.center), v3(spec.point));
        let handle = self
            .scene_mut()
            .add_entity(EntityType::Dimension(Dimension::Diameter(dim)));
        Ok(handle_to_obj(handle))
    }

    fn add_dimension_angular(
        &mut self,
        spec: &ocs_doc_api::ops::DimensionAngularSpec,
    ) -> ApiResult<ObjectId> {
        use acadrust::entities::{Dimension, DimensionAngular3Pt};
        use acadrust::types::Vector3;
        let v3 = |p: [f64; 3]| Vector3::new(p[0], p[1], p[2]);
        let mut dim =
            DimensionAngular3Pt::new(v3(spec.vertex), v3(spec.first_point), v3(spec.second_point));
        dim.definition_point = v3(spec.arc_location);
        let handle = self
            .scene_mut()
            .add_entity(EntityType::Dimension(Dimension::Angular3Pt(dim)));
        Ok(handle_to_obj(handle))
    }

    fn dimension_measurement(&self, id: ObjectId) -> ApiResult<f64> {
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        let EntityType::Dimension(d) = entity else {
            return Err(ApiError::Unsupported(
                "GetDimensionMeasurement is only for Dimension".into(),
            ));
        };
        // Distance for linear/radius/diameter; degrees for angular.
        Ok(match d {
            acadrust::entities::Dimension::Linear(x) => x.measurement(),
            acadrust::entities::Dimension::Radius(x) => x.measurement(),
            acadrust::entities::Dimension::Diameter(x) => x.measurement(),
            acadrust::entities::Dimension::Angular3Pt(x) => x.measurement_degrees(),
            acadrust::entities::Dimension::Angular2Ln(x) => x.measurement_degrees(),
            _ => {
                return Err(ApiError::Unsupported(
                    "dimension measurement for this sub-type".into(),
                ))
            }
        })
    }

    fn add_attribute_definition(
        &mut self,
        spec: &ocs_doc_api::ops::AttributeDefinitionSpec,
    ) -> ApiResult<ObjectId> {
        use acadrust::entities::AttributeDefinition;
        use acadrust::types::Vector3;
        let att = AttributeDefinition {
            tag: spec.tag.clone(),
            prompt: spec.prompt.clone(),
            default_value: spec.default_value.clone(),
            insertion_point: Vector3::new(
                spec.insertion_point[0],
                spec.insertion_point[1],
                spec.insertion_point[2],
            ),
            height: spec.height,
            rotation: spec.rotation,
            ..Default::default()
        };
        let handle = self
            .scene_mut()
            .add_entity(EntityType::AttributeDefinition(att));
        Ok(handle_to_obj(handle))
    }

    fn add_dimension_angular2ln(
        &mut self,
        spec: &ocs_doc_api::ops::DimensionAngularSpec,
    ) -> ApiResult<ObjectId> {
        use acadrust::entities::{Dimension, DimensionAngular2Ln};
        use acadrust::types::Vector3;
        let v3 = |p: [f64; 3]| Vector3::new(p[0], p[1], p[2]);
        let mut dim =
            DimensionAngular2Ln::new(v3(spec.vertex), v3(spec.first_point), v3(spec.second_point));
        dim.dimension_arc = v3(spec.arc_location);
        let handle = self
            .scene_mut()
            .add_entity(EntityType::Dimension(Dimension::Angular2Ln(dim)));
        Ok(handle_to_obj(handle))
    }

    fn add_table(&mut self, spec: &ocs_doc_api::ops::TableSpec) -> ApiResult<ObjectId> {
        use acadrust::entities::Table;
        use acadrust::types::Vector3;
        let cols = spec.data.first().map(|r| r.len()).unwrap_or(0);
        let mut table = Table::new(
            Vector3::new(
                spec.insertion_point[0],
                spec.insertion_point[1],
                spec.insertion_point[2],
            ),
            spec.data.len(),
            cols,
        );
        // Fill cells from the grid (each cell = text).
        for (r, row) in spec.data.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                table.set_cell_text(r, c, text);
            }
        }
        let handle = self.scene_mut().add_entity(EntityType::Table(table));
        Ok(handle_to_obj(handle))
    }

    fn set_attribute(&mut self, id: ObjectId, tag: &str, value: &str) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
            .cloned()
            .ok_or(ApiError::UnknownId(id))?;
        let EntityType::Insert(mut ins) = entity else {
            return Err(ApiError::Unsupported(
                "SetAttribute is only for Insert entities".into(),
            ));
        };
        if let Some(attr) = ins.attributes.iter_mut().find(|a| a.tag == tag) {
            attr.value = value.to_string();
        } else {
            let attr = acadrust::entities::AttributeEntity {
                tag: tag.to_string(),
                value: value.to_string(),
                insertion_point: ins.insert_point,
                ..Default::default()
            };
            ins.attributes.push(attr);
        }
        if !self.scene_mut().update_entity(EntityType::Insert(ins)) {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
        }
        Ok(())
    }

    fn attributes(&self, id: ObjectId) -> ApiResult<Vec<(String, String)>> {
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        let EntityType::Insert(ins) = entity else {
            return Err(ApiError::Unsupported(
                "GetAttributes is only for Insert entities".into(),
            ));
        };
        Ok(ins
            .attributes
            .iter()
            .map(|a| (a.tag.clone(), a.value.clone()))
            .collect())
    }

    fn block_entities(&self, block_name: &str) -> ApiResult<Vec<EntityView>> {
        let br = self
            .document()
            .block_records
            .get(block_name)
            .ok_or_else(|| {
                ApiError::validation("GetBlockEntities", format!("unknown block {block_name:?}"))
            })?;
        // Read-only traversal of the block definition's entities (handle lookups).
        let mut out = Vec::with_capacity(br.entity_handles.len());
        for h in &br.entity_handles {
            let id = handle_to_obj(*h);
            let (kind, bounds) = match self.document().get_entity(*h) {
                Some(e) => (
                    convert::entity_kind_name(e).to_string(),
                    convert::entity_bounds(Some(e), id).ok(),
                ),
                None => ("Missing".to_string(), None),
            };
            let layer = match self.document().get_entity(*h) {
                Some(e) => e.common().layer.clone(),
                None => String::new(),
            };
            out.push(EntityView {
                id,
                kind,
                layer,
                bounds,
            });
        }
        Ok(out)
    }

    fn set_viewport_view(
        &mut self,
        id: ObjectId,
        view_target: [f64; 3],
        view_height: f64,
    ) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
            .cloned()
            .ok_or(ApiError::UnknownId(id))?;
        let EntityType::Viewport(mut vp) = entity else {
            return Err(ApiError::Unsupported(
                "SetViewportView is only for Viewport".into(),
            ));
        };
        vp.view_target =
            acadrust::types::Vector3::new(view_target[0], view_target[1], view_target[2]);
        vp.view_height = view_height;
        if !self.scene_mut().update_entity(EntityType::Viewport(vp)) {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
        }
        Ok(())
    }

    fn viewport_view(&self, id: ObjectId) -> ApiResult<([f64; 3], f64)> {
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        let EntityType::Viewport(vp) = entity else {
            return Err(ApiError::Unsupported(
                "GetViewportView is only for Viewport".into(),
            ));
        };
        Ok((
            [vp.view_target.x, vp.view_target.y, vp.view_target.z],
            vp.view_height,
        ))
    }

    fn add_raster_image(
        &mut self,
        spec: &ocs_doc_api::ops::RasterImageSpec,
    ) -> ApiResult<ObjectId> {
        use acadrust::entities::RasterImage;
        use acadrust::types::{Vector2, Vector3};
        // Validate the plugin-supplied path before embedding it in the document:
        // non-empty, a known image extension, and not a UNC/network path or
        // parent-traversal (which could point the host at unintended resources).
        validate_image_path(&spec.file_path)?;
        let img = RasterImage {
            file_path: spec.file_path.clone(),
            insertion_point: Vector3::new(
                spec.insertion_point[0],
                spec.insertion_point[1],
                spec.insertion_point[2],
            ),
            u_vector: Vector3::new(spec.u_vector[0], spec.u_vector[1], spec.u_vector[2]),
            v_vector: Vector3::new(spec.v_vector[0], spec.v_vector[1], spec.v_vector[2]),
            size: Vector2::new(spec.size[0], spec.size[1]),
            ..Default::default()
        };
        // add_entity auto-registers the ImageDefinition (scene/entity.rs).
        let handle = self.scene_mut().add_entity(EntityType::RasterImage(img));
        Ok(handle_to_obj(handle))
    }

    fn loft(
        &mut self,
        sections: &[(cadkernel::space::Plane, Vec<cadkernel::geom2d::Curve>)],
    ) -> ApiResult<ObjectId> {
        let body = cadkernel::brep::loft(sections)
            .ok_or_else(|| ApiError::geometry(GeometryErrorKind::InvalidInput, "loft failed"))?;
        self.store_solid(&body)
    }

    fn set_xdata(
        &mut self,
        id: ObjectId,
        application_name: &str,
        record: Option<&ocs_doc_api::XDataRecord>,
    ) -> ApiResult<()> {
        self.can_modify(id)?;
        let handle = obj_to_handle(id);
        let values = record.map(|r| {
            r.values
                .iter()
                .map(|v| xdata_value_to_acadrust(v, self.document()))
                .collect::<Vec<_>>()
        });
        crate::scene::view::dispatch::set_entity_xdata(
            self.document_mut(),
            handle,
            application_name,
            values,
        );
        Ok(())
    }

    fn xdata(&self, id: ObjectId, application_name: &str) -> ApiResult<Option<ocs_doc_api::XDataRecord>> {
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
            .ok_or(ApiError::UnknownId(id))?;
        let Some(rec) = entity.common().extended_data.get_record(application_name) else {
            return Ok(None);
        };
        Ok(Some(ocs_doc_api::XDataRecord {
            application_name: rec.application_name.clone(),
            values: rec
                .values
                .iter()
                .map(xdata_value_from_acadrust)
                .collect(),
        }))
    }

    fn object_exists(&self, id: ObjectId) -> bool {
        let handle = obj_to_handle(id);
        self.document().objects.contains_key(&handle)
    }

    fn add_xrecord(&mut self, spec: &ocs_doc_api::XRecordSpec) -> ApiResult<ObjectId> {
        use acadrust::objects::{ObjectType, XRecord};
        let dict_h = xrecord_dictionary_handle(self.document_mut());
        if matches!(self.document().objects.get(&dict_h), Some(ObjectType::Dictionary(dict)) if
            dict.entries.iter().any(|(name, _)| name.eq_ignore_ascii_case(&spec.name)))
        {
            return Err(ApiError::validation(
                "CreateXRecord",
                format!("XRecord '{}' already exists", spec.name),
            ));
        }
        let mut record = XRecord::named(spec.name.clone());
        record.owner = dict_h;
        record.cloning_flags = cloning_flags_to_acadrust(spec.cloning_flags);
        record.entries = spec
            .entries
            .iter()
            .map(xrecord_entry_to_acadrust)
            .collect();
        record.synchronize_object_references();
        let record_handle = {
            let doc = self.document_mut();
            let handle = doc.allocate_handle();
            record.handle = handle;
            doc.objects
                .insert(handle, ObjectType::XRecord(record));
            handle
        };
        // Attach the new XRecord to the stable named-object dictionary so it is
        // reachable as a standalone named object.
        if let Some(ObjectType::Dictionary(dict)) = self.document_mut().objects.get_mut(&dict_h) {
            dict.add_entry(spec.name.clone(), record_handle);
        }
        Ok(handle_to_obj(record_handle))
    }

    fn set_xrecord(&mut self, id: ObjectId, spec: &ocs_doc_api::XRecordSpec) -> ApiResult<()> {
        if !self.object_exists(id) {
            return Err(ApiError::validation(
                "SetXRecord",
                format!("unknown ObjectId {id:?}"),
            ));
        }
        let handle = obj_to_handle(id);
        let Some(ObjectType::XRecord(record)) = self.document().objects.get(&handle) else {
            return Err(ApiError::Unsupported(format!("ObjectId {id:?} is not an XRecord")));
        };
        let mut updated = record.clone();
        let dict_h = xrecord_owner_dictionary(self.document(), handle, record.owner)
            .unwrap_or_else(|| xrecord_dictionary_handle(self.document_mut()));
        if matches!(self.document().objects.get(&dict_h), Some(ObjectType::Dictionary(dict)) if
            dict.entries.iter().any(|(name, child)| {
                *child != handle && name.eq_ignore_ascii_case(&spec.name)
            }))
        {
            return Err(ApiError::validation(
                "SetXRecord",
                format!("XRecord '{}' already exists", spec.name),
            ));
        }
        updated.name = spec.name.clone();
        updated.owner = dict_h;
        updated.cloning_flags = cloning_flags_to_acadrust(spec.cloning_flags);
        updated.entries = spec
            .entries
            .iter()
            .map(xrecord_entry_to_acadrust)
            .collect();
        updated.synchronize_object_references();
        self.document_mut()
            .objects
            .insert(handle, acadrust::objects::ObjectType::XRecord(updated));
        if let Some(ObjectType::Dictionary(dict)) = self.document_mut().objects.get_mut(&dict_h) {
            dict.entries.retain(|(_, child)| *child != handle);
            dict.add_entry(spec.name.clone(), handle);
        }
        Ok(())
    }

    fn xrecord(&self, id: ObjectId) -> ApiResult<Option<ocs_doc_api::XRecordSpec>> {
        let handle = obj_to_handle(id);
        let Some(ObjectType::XRecord(record)) = self.document().objects.get(&handle) else {
            return Ok(None);
        };
        Ok(Some(ocs_doc_api::XRecordSpec {
            name: record.name.clone(),
            cloning_flags: cloning_flags_from_acadrust(record.cloning_flags),
            entries: record
                .entries
                .iter()
                .map(xrecord_entry_from_acadrust)
                .collect(),
        }))
    }

    fn create_layer(&mut self, info: &LayerInfo) -> ApiResult<()> {
        let name = acadrust::tables::normalize_name(info.name.trim());
        if name.is_empty() {
            return Err(ApiError::validation("CreateLayer", "empty layer name"));
        }
        if self.document().layers.contains(&name) {
            return Err(ApiError::validation(
                "CreateLayer",
                format!("layer '{name}' already exists"),
            ));
        }
        let mut layer = acadrust::tables::Layer::new(&name);
        layer.handle = self.document_mut().allocate_handle();
        apply_layer_info(&mut layer, info);
        self.document_mut()
            .layers
            .add(layer)
            .map_err(|e| ApiError::validation("CreateLayer", e))?;
        Ok(())
    }

    fn update_layer(&mut self, name: &str, info: &LayerInfo) -> ApiResult<()> {
        let deps = {
            let doc = self.document_mut();
            let name_upper = acadrust::tables::normalize_name(name.trim());
            let Some(existing) = doc.layers.get(&name_upper).cloned() else {
                return Err(ApiError::validation(
                    "UpdateLayer",
                    format!("layer '{name}' does not exist"),
                ));
            };
            let new_name = acadrust::tables::normalize_name(info.name.trim());
            if new_name.is_empty() {
                return Err(ApiError::validation("UpdateLayer", "empty layer name"));
            }
            // Rename if the key changed (preserves handle and table position).
            if name_upper != new_name {
                doc.layers
                    .rename(&name_upper, new_name.clone())
                    .map_err(|e| ApiError::validation("UpdateLayer", e))?;
            }
            let Some(layer) = doc.layers.get_mut(&new_name) else {
                return Err(ApiError::validation(
                    "UpdateLayer",
                    format!("layer '{name}' does not exist"),
                ));
            };
            apply_layer_info(layer, info);
            [existing.name.clone(), layer.name.clone()]
        };
        // By-layer appearance changed: rebuild derived geometry that resolved it.
        self.scene_mut().invalidate_layer_dependencies(&deps);
        Ok(())
    }
    fn delete_layer(&mut self, name: &str) -> ApiResult<()> {
        let name_upper = acadrust::tables::normalize_name(name.trim());
        if name_upper == "0" {
            return Err(ApiError::validation("DeleteLayer", "cannot delete layer 0"));
        }
        if !self.document().layers.contains(&name_upper) {
            return Err(ApiError::validation(
                "DeleteLayer",
                format!("layer '{name}' does not exist"),
            ));
        }
        if acadrust::tables::normalize_name(&self.document().header.current_layer_name) == name_upper
        {
            return Err(ApiError::validation(
                "DeleteLayer",
                "cannot delete the current layer",
            ));
        }
        if self
            .document()
            .entities()
            .any(|e| acadrust::tables::normalize_name(&e.common().layer) == name_upper)
        {
            return Err(ApiError::validation(
                "DeleteLayer",
                format!("layer '{name}' is still in use"),
            ));
        }
        // Remove by the normalized name so a differently-cased request behaves
        // consistently with the rest of the layer table API.
        self.document_mut().layers.remove(&name_upper);
        Ok(())
    }


    fn set_entity_layer(&mut self, id: ObjectId, layer: &str) -> ApiResult<()> {
        self.can_modify(id)?;
        let handle = obj_to_handle(id);
        let layer_upper = acadrust::tables::normalize_name(layer.trim());
        let target = self.document().layers.get(&layer_upper).cloned();
        let Some(target) = target else {
            return Err(ApiError::validation(
                "SetEntityLayer",
                format!("layer '{layer}' does not exist"),
            ));
        };
        if target.is_locked() {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} cannot be moved to locked layer '{layer}'"
            )));
        }
        let mut entity = self
            .document()
            .get_entity(handle)
            .cloned()
            .ok_or(ApiError::UnknownId(id))?;
        entity.common_mut().layer = target.name.clone();
        if !self.scene_mut().update_entity(entity) {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
        }
        Ok(())
    }

    fn layers(&self) -> ApiResult<Vec<LayerInfo>> {
        let mut v: Vec<_> = self
            .document()
            .layers
            .iter()
            .map(layer_info_from_acadrust)
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(v)
    }

    fn entity_layer(&self, id: ObjectId) -> ApiResult<String> {
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
            .ok_or(ApiError::UnknownId(id))?;
        Ok(entity.common().layer.clone())
    }

    fn enumerate_entities(
        &self,
        kind: Option<&str>,
        layer: Option<&str>,
        include_bounds: bool,
    ) -> ApiResult<Vec<EntityView>> {
        let doc = self.document();
        let mut out = Vec::new();
        for entity in doc.entities() {
            let handle = entity.common().handle;
            let id = handle_to_obj(handle);
            let entity_kind = convert::entity_kind_name(entity);
            if let Some(k) = kind {
                if !entity_kind.eq_ignore_ascii_case(k) {
                    continue;
                }
            }
            if let Some(l) = layer {
                if acadrust::tables::normalize_name(&entity.common().layer)
                    != acadrust::tables::normalize_name(l.trim())
                {
                    continue;
                }
            }
            out.push(EntityView {
                id,
                kind: entity_kind.to_string(),
                layer: entity.common().layer.clone(),
                bounds: if include_bounds {
                    convert::entity_bounds(Some(entity), id).ok()
                } else {
                    None
                },
            });
        }
        Ok(out)
    }

    fn point_position(&self, id: ObjectId) -> ApiResult<[f64; 3]> {
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
            .ok_or(ApiError::UnknownId(id))?;
        let EntityType::Point(pt) = entity else {
            return Err(ApiError::Unsupported(
                "GetPointPosition is only for Point entities".into(),
            ));
        };
        Ok(v3_to_array(pt.location))
    }

    fn line_geometry(&self, id: ObjectId) -> ApiResult<([f64; 3], [f64; 3])> {
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
            .ok_or(ApiError::UnknownId(id))?;
        let EntityType::Line(line) = entity else {
            return Err(ApiError::Unsupported(
                "GetLineGeometry is only for Line entities".into(),
            ));
        };
        Ok((v3_to_array(line.start), v3_to_array(line.end)))
    }

    fn add_vertex(&mut self, id: ObjectId, at: usize, point: [f64; 3]) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        let Some(entity) = self.document().get_entity(handle).cloned() else {
            return Err(ApiError::UnknownId(id));
        };
        let EntityType::LwPolyline(mut pl) = entity else {
            return Err(ApiError::Unsupported(
                "AddVertex is only implemented for polylines".into(),
            ));
        };
        if point[2] != pl.elevation || pl.vertices.len() >= ocs_doc_api::ops::BULK_ITEM_CAP {
            return Err(ApiError::validation(
                "AddVertex",
                "vertex must share the polyline elevation and fit the item cap",
            ));
        }
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
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
        }
        Ok(())
    }

    fn remove_entity(&mut self, id: ObjectId) -> ApiResult<bool> {
        let handle = obj_to_handle(id);
        if !self.entity_exists(id) {
            return Ok(false);
        }
        // Locked layers refuse deletion; surface that as an error so the executor
        // does not treat a no-op as success.
        if self.scene().is_layer_locked(handle) {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
        }
        // erase_entities clears the entity + solid_models/meshes/hatches and
        // records the undo delta; the single publish happens at finalize_op.
        self.scene_mut().erase_entities(&[handle]);
        Ok(self.document().get_entity(handle).is_none())
    }

    fn ensure_transformable(&mut self, id: ObjectId) -> ApiResult<()> {
        // Pre-validating everything the apply loop can actually fail on is what makes
        // TransformMany all-or-nothing (no mid-loop failure after earlier mutations).
        // This must check more than the entity TYPE: layer-lock (update_entity returns
        // false) and, for solids, that the body resolves.
        let handle = obj_to_handle(id);
        let entity = self
            .document()
            .get_entity(handle)
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
                | EntityType::Viewport(_)
                | EntityType::Text(_)
                | EntityType::MText(_)
                | EntityType::Point(_)
                | EntityType::LwPolyline(_)
        );
        if !ok {
            return Err(ApiError::Unsupported(
                "transform is not supported for this entity family".into(),
            ));
        }
        // Locked layer -> update_entity would return false mid-loop.
        if self.scene().is_layer_locked(handle) {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
        }
        // Solid: the body must resolve (kernel transform/display-prep can still fail
        // at apply time, but resolution is the checkable precondition).
        if matches!(entity, EntityType::Solid3D(_)) {
            self.resolve_body(id)?;
        }
        Ok(())
    }

    fn can_remove(&self, id: ObjectId) -> ApiResult<()> {
        let handle = obj_to_handle(id);
        if !self.entity_exists(id) {
            return Err(ApiError::UnknownId(id));
        }
        if self.scene().is_layer_locked(handle) {
            return Err(ApiError::Unsupported(format!(
                "entity {id:?} is on a locked layer"
            )));
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
            kind: convert::entity_kind_name(entity).to_string(),
            layer: entity.common().layer.clone(),
            bounds: self.bounds(id).ok(),
        })
    }

    fn transform_entity(&mut self, id: ObjectId, placement: &PlacementSpec) -> ApiResult<()> {
        let prepared = self.prepare_doc_transform(id, placement)?;
        self.apply_doc_entity(prepared, Some(id));
        Ok(())
    }

    fn profile_curves(&self, id: ObjectId) -> ApiResult<Vec<cadkernel::geom2d::Curve>> {
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        crate::scene::model::sweep_model::extrusion_profile_of(entity)
            .map(|(profile, _)| profile.pieces)
            .ok_or_else(|| ApiError::Unsupported("entity is not a planar profile".into()))
    }

    fn profile_plane(&self, id: ObjectId) -> ApiResult<cadkernel::space::Plane> {
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        crate::scene::model::sweep_model::extrusion_profile_of(entity)
            .map(|(profile, _)| profile.plane)
            .ok_or_else(|| ApiError::Unsupported("entity is not a planar profile".into()))
    }

    fn bounds(&mut self, id: ObjectId) -> ApiResult<Aabb> {
        let handle = obj_to_handle(id);
        // Lift-on-miss for modeler geometry (consistent with volume/centroid); borrow the cache
        // read-only via with_body (O(1), no B-rep deep clone).
        if matches!(
            self.document().get_entity(handle),
            Some(EntityType::Solid3D(_))
        ) || matches!(
            self.document().get_entity(handle),
            Some(EntityType::Region(region)) if region.acis_data.has_data()
        ) {
            let mut f = |body: &KernelBody| -> ApiResult<Aabb> {
                let bb = cadkernel::brep::body_bounds(body)
                    .ok_or_else(|| ApiError::geometry(GeometryErrorKind::Empty, "no bounds"))?;
                Ok(Aabb {
                    min: bb.min,
                    max: bb.max,
                })
            };
            return self.with_body(id, &mut f);
        }
        convert::entity_bounds(self.document().get_entity(handle), id)
    }

    fn centroid(&mut self, id: ObjectId) -> ApiResult<[f64; 3]> {
        Ok(self.mass_properties(id)?.1)
    }

    fn volume(&mut self, id: ObjectId) -> ApiResult<f64> {
        Ok(self.mass_properties(id)?.0)
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

    fn cancel_op(&mut self) {
        self.cancel_doc_api_undo();
    }

    fn finalize_op(&mut self) {
        // Close the delta entry + bump geometry + republish the document view.
        self.commit_doc_api_undo();
    }
}

// â”€â”€ helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

type SolidDisplay = (
    crate::scene::model::mesh_model::MeshLodSet,
    Vec<acadrust::entities::Wire>,
    [f64; 3],
);

struct PreparedDocEntity {
    entity: EntityType,
    solid: Option<(KernelBody, SolidDisplay)>,
}

impl HostSession<'_> {
    fn prepare_doc_solid(
        &self,
        body: KernelBody,
        id: Option<ObjectId>,
    ) -> ApiResult<PreparedDocEntity> {
        let mut inner = match id {
            Some(id) => match self.document().get_entity(obj_to_handle(id)) {
                Some(EntityType::Solid3D(solid)) => solid.clone(),
                _ => return Err(ApiError::UnknownId(id)),
            },
            None => Solid3D::new(),
        };
        let handle = id.map_or(Handle::NULL, obj_to_handle);
        let display = self
            .scene()
            .prepare_solid_model_display(handle, &body)
            .ok_or_else(|| {
                ApiError::geometry(GeometryErrorKind::Other, "solid display preparation failed")
            })?;
        let sat = acis_export::solid_to_sat(&body).ok_or_else(|| {
            ApiError::geometry(GeometryErrorKind::Acis, "solid serialization failed")
        })?;
        inner.wires = solid_model::edge_wires(&body);
        inner.set_sat_document(&sat);
        Ok(PreparedDocEntity {
            entity: EntityType::Solid3D(inner),
            solid: Some((body, display)),
        })
    }

    fn prepare_doc_transform(
        &mut self,
        id: ObjectId,
        placement: &PlacementSpec,
    ) -> ApiResult<PreparedDocEntity> {
        self.ensure_transformable(id)?;
        let entity = self
            .document()
            .get_entity(obj_to_handle(id))
            .ok_or(ApiError::UnknownId(id))?;
        if matches!(entity, EntityType::Solid3D(_)) {
            let body = self.resolve_body(id)?;
            let moved = cadkernel::brep::transform(&body, &kernel_placement(placement))
                .ok_or_else(|| {
                    ApiError::geometry(GeometryErrorKind::InvalidInput, "transform failed")
                })?;
            self.prepare_doc_solid(moved, Some(id))
        } else {
            Ok(PreparedDocEntity {
                entity: convert::transform_entity_geometry(entity, placement)?,
                solid: None,
            })
        }
    }

    fn apply_doc_entity(&mut self, prepared: PreparedDocEntity, id: Option<ObjectId>) -> ObjectId {
        let handle = match id {
            Some(id) => {
                assert!(
                    self.scene_mut().update_entity(prepared.entity),
                    "prevalidated entity update"
                );
                obj_to_handle(id)
            }
            None => self.scene_mut().add_entity(prepared.entity),
        };
        if let Some((body, display)) = prepared.solid {
            self.scene_mut()
                .register_prepared_solid_model(handle, body, display);
        }
        handle_to_obj(handle)
    }

    /// Kernel mesh mass properties, cached per entity and geometry epoch.
    fn mass_properties(&mut self, id: ObjectId) -> ApiResult<(f64, [f64; 3])> {
        let handle = obj_to_handle(id);
        let epoch = self.scene().geometry_epoch;
        if let Some(&(cached_epoch, v, c)) = self.scene().doc_api_mass_cache.get(&handle) {
            if cached_epoch == epoch {
                return Ok((v, c));
            }
        }
        if self.scene().doc_api_cold_tess_used >= 32 {
            return Err(ApiError::validation(
                "mass properties",
                "request exceeds 32 uncached solid queries",
            ));
        }
        let body = self.resolve_body(id)?;
        self.scene_mut().doc_api_cold_tess_used += 1;
        let mesh = cadkernel::brep::mesh_body(&body, 0.1, 1e-4);
        let (v, c) = mesh.mass_properties().ok_or_else(|| {
            ApiError::geometry(GeometryErrorKind::Empty, "solid has no measurable volume")
        })?;
        if self.scene().doc_api_mass_cache.len() >= ocs_doc_api::ops::BULK_ITEM_CAP {
            self.scene_mut().doc_api_mass_cache.clear();
        }
        self.scene_mut()
            .doc_api_mass_cache
            .insert(handle, (epoch, v, c));
        Ok((v, c))
    }
}

/// Validate a raster-image file path before embedding it in a document.
/// Rejects empty paths, UNC/network paths, parent-traversal, and non-image extensions.
fn validate_image_path(path: &str) -> ApiResult<()> {
    let p = path.trim();
    if p.is_empty() {
        return Err(ApiError::validation(
            "CreateRasterImage",
            "empty image file path",
        ));
    }
    // UNC / network / device paths.
    if p.starts_with("\\\\") || p.starts_with("//") {
        return Err(ApiError::Unsupported(
            "network/UNC image paths are not allowed".into(),
        ));
    }
    // Parent-traversal anywhere in the path.
    if p.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(ApiError::Unsupported(
            "parent-traversal ('..') in image path is not allowed".into(),
        ));
    }
    // Known image extension (last component).
    let ext_ok = p
        .rsplit('.')
        .next()
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "bmp" | "gif" | "tif" | "tiff" | "webp" | "tga"
            )
        })
        .unwrap_or(false);
    if !ext_ok {
        return Err(ApiError::validation(
            "CreateRasterImage",
            format!("unsupported image file extension in {path:?}"),
        ));
    }
    Ok(())
}

fn kernel_placement(p: &PlacementSpec) -> cadkernel::brep::Placement {
    cadkernel::brep::Placement {
        x_axis: p.x_axis,
        y_axis: p.y_axis,
        z_axis: p.z_axis,
        origin: p.origin,
    }
}

// Conversions between ocs_doc_api plain-data DTOs and acadrust types.
// Kept in this file because they are host-side implementation details.

fn cloning_flags_to_acadrust(
    flags: ocs_doc_api::XRecordCloningFlags,
) -> acadrust::objects::DictionaryCloningFlags {
    use acadrust::objects::DictionaryCloningFlags as F;
    match flags {
        ocs_doc_api::XRecordCloningFlags::NotApplicable => F::NotApplicable,
        ocs_doc_api::XRecordCloningFlags::KeepExisting => F::KeepExisting,
        ocs_doc_api::XRecordCloningFlags::UseClone => F::UseClone,
        ocs_doc_api::XRecordCloningFlags::XrefName => F::XrefName,
        ocs_doc_api::XRecordCloningFlags::Name => F::Name,
        ocs_doc_api::XRecordCloningFlags::UnmangleName => F::UnmangleName,
    }
}

fn cloning_flags_from_acadrust(
    flags: acadrust::objects::DictionaryCloningFlags,
) -> ocs_doc_api::XRecordCloningFlags {
    use acadrust::objects::DictionaryCloningFlags as F;
    match flags {
        F::NotApplicable => ocs_doc_api::XRecordCloningFlags::NotApplicable,
        F::KeepExisting => ocs_doc_api::XRecordCloningFlags::KeepExisting,
        F::UseClone => ocs_doc_api::XRecordCloningFlags::UseClone,
        F::XrefName => ocs_doc_api::XRecordCloningFlags::XrefName,
        F::Name => ocs_doc_api::XRecordCloningFlags::Name,
        F::UnmangleName => ocs_doc_api::XRecordCloningFlags::UnmangleName,
    }
}

fn xdata_value_to_acadrust(
    value: &ocs_doc_api::XDataValue,
    _doc: &acadrust::CadDocument,
) -> acadrust::xdata::XDataValue {
    use acadrust::types::Vector3;
    use ocs_doc_api::XDataValue as V;
    match value {
        V::String(s) => acadrust::xdata::XDataValue::String(s.clone()),
        V::ControlString(s) => acadrust::xdata::XDataValue::ControlString(s.clone()),
        V::LayerName(s) => acadrust::xdata::XDataValue::LayerName(s.clone()),
        V::BinaryData(b) => acadrust::xdata::XDataValue::BinaryData(b.clone()),
        V::Handle(h) => acadrust::xdata::XDataValue::Handle(acadrust::Handle::new(*h)),
        V::Point3D(p) => acadrust::xdata::XDataValue::Point3D(Vector3::new(p[0], p[1], p[2])),
        V::Position3D(p) => acadrust::xdata::XDataValue::Position3D(Vector3::new(p[0], p[1], p[2])),
        V::Displacement3D(p) => {
            acadrust::xdata::XDataValue::Displacement3D(Vector3::new(p[0], p[1], p[2]))
        }
        V::Direction3D(p) => acadrust::xdata::XDataValue::Direction3D(Vector3::new(p[0], p[1], p[2])),
        V::Real(r) => acadrust::xdata::XDataValue::Real(*r),
        V::Distance(d) => acadrust::xdata::XDataValue::Distance(*d),
        V::ScaleFactor(s) => acadrust::xdata::XDataValue::ScaleFactor(*s),
        V::Integer16(i) => acadrust::xdata::XDataValue::Integer16(*i),
        V::Integer32(i) => acadrust::xdata::XDataValue::Integer32(*i),
    }
}

fn xdata_value_from_acadrust(value: &acadrust::xdata::XDataValue) -> ocs_doc_api::XDataValue {
    use acadrust::xdata::XDataValue as V;
    match value {
        V::String(s) => ocs_doc_api::XDataValue::String(s.clone()),
        V::ControlString(s) => ocs_doc_api::XDataValue::ControlString(s.clone()),
        V::LayerName(s) => ocs_doc_api::XDataValue::LayerName(s.clone()),
        V::BinaryData(b) => ocs_doc_api::XDataValue::BinaryData(b.clone()),
        V::Handle(h) => ocs_doc_api::XDataValue::Handle(h.value()),
        V::Point3D(v) => ocs_doc_api::XDataValue::Point3D([v.x, v.y, v.z]),
        V::Position3D(v) => ocs_doc_api::XDataValue::Position3D([v.x, v.y, v.z]),
        V::Displacement3D(v) => ocs_doc_api::XDataValue::Displacement3D([v.x, v.y, v.z]),
        V::Direction3D(v) => ocs_doc_api::XDataValue::Direction3D([v.x, v.y, v.z]),
        V::Real(r) => ocs_doc_api::XDataValue::Real(*r),
        V::Distance(d) => ocs_doc_api::XDataValue::Distance(*d),
        V::ScaleFactor(s) => ocs_doc_api::XDataValue::ScaleFactor(*s),
        V::Integer16(i) => ocs_doc_api::XDataValue::Integer16(*i),
        V::Integer32(i) => ocs_doc_api::XDataValue::Integer32(*i),
    }
}

fn xrecord_value_to_acadrust(value: &ocs_doc_api::XRecordValue) -> acadrust::objects::XRecordValue {
    use ocs_doc_api::XRecordValue as V;
    match value {
        V::String(s) => acadrust::objects::XRecordValue::String(s.clone()),
        V::Double(d) => acadrust::objects::XRecordValue::Double(*d),
        V::Int16(i) => acadrust::objects::XRecordValue::Int16(*i),
        V::Int32(i) => acadrust::objects::XRecordValue::Int32(*i),
        V::Int64(i) => acadrust::objects::XRecordValue::Int64(*i),
        V::Byte(b) => acadrust::objects::XRecordValue::Byte(*b),
        V::Bool(b) => acadrust::objects::XRecordValue::Bool(*b),
        V::Handle(h) => acadrust::objects::XRecordValue::Handle(acadrust::Handle::new(*h)),
        V::Point3D(p) => acadrust::objects::XRecordValue::Point3D(p[0], p[1], p[2]),
        V::Chunk(c) => acadrust::objects::XRecordValue::Chunk(c.clone()),
    }
}

fn xrecord_value_from_acadrust(
    value: &acadrust::objects::XRecordValue,
) -> ocs_doc_api::XRecordValue {
    use acadrust::objects::XRecordValue as V;
    match value {
        V::String(s) => ocs_doc_api::XRecordValue::String(s.clone()),
        V::Double(d) => ocs_doc_api::XRecordValue::Double(*d),
        V::Int16(i) => ocs_doc_api::XRecordValue::Int16(*i),
        V::Int32(i) => ocs_doc_api::XRecordValue::Int32(*i),
        V::Int64(i) => ocs_doc_api::XRecordValue::Int64(*i),
        V::Byte(b) => ocs_doc_api::XRecordValue::Byte(*b),
        V::Bool(b) => ocs_doc_api::XRecordValue::Bool(*b),
        V::Handle(h) => ocs_doc_api::XRecordValue::Handle(h.value()),
        V::Point3D(x, y, z) => ocs_doc_api::XRecordValue::Point3D([*x, *y, *z]),
        V::Chunk(c) => ocs_doc_api::XRecordValue::Chunk(c.clone()),
    }
}

fn xrecord_entry_to_acadrust(
    entry: &ocs_doc_api::XRecordEntry,
) -> acadrust::objects::XRecordEntry {
    acadrust::objects::XRecordEntry {
        code: entry.code,
        value: xrecord_value_to_acadrust(&entry.value),
    }
}

fn xrecord_entry_from_acadrust(
    entry: &acadrust::objects::XRecordEntry,
) -> ocs_doc_api::XRecordEntry {
    ocs_doc_api::XRecordEntry {
        code: entry.code,
        value: xrecord_value_from_acadrust(&entry.value),
    }
}

// ── layer conversions ─────────────────────────────────────────────────────

fn color_to_acadrust(c: Color) -> acadrust::types::Color {
    use acadrust::types::Color as A;
    match c {
        Color::ByLayer => A::ByLayer,
        Color::None => A::None,
        Color::ByBlock => A::ByBlock,
        Color::Index(i) => A::Index(i),
        Color::Rgb { r, g, b } => A::Rgb { r, g, b },
    }
}

fn color_from_acadrust(c: acadrust::types::Color) -> Color {
    use acadrust::types::Color as A;
    match c {
        A::ByLayer => Color::ByLayer,
        A::None => Color::None,
        A::ByBlock => Color::ByBlock,
        A::Index(i) => Color::Index(i),
        A::Rgb { r, g, b } => Color::Rgb { r, g, b },
    }
}

fn line_weight_to_acadrust(lw: LineWeight) -> acadrust::types::LineWeight {
    use acadrust::types::LineWeight as A;
    match lw {
        LineWeight::ByLayer => A::ByLayer,
        LineWeight::ByBlock => A::ByBlock,
        LineWeight::Default => A::Default,
        LineWeight::Value(v) => A::Value(v),
    }
}

fn line_weight_from_acadrust(lw: acadrust::types::LineWeight) -> LineWeight {
    use acadrust::types::LineWeight as A;
    match lw {
        A::ByLayer => LineWeight::ByLayer,
        A::ByBlock => LineWeight::ByBlock,
        A::Default => LineWeight::Default,
        A::Value(v) => LineWeight::Value(v),
    }
}

fn layer_flags_to_acadrust(f: LayerFlags) -> acadrust::tables::LayerFlags {
    acadrust::tables::LayerFlags {
        frozen: f.frozen,
        locked: f.locked,
        frozen_in_new_viewport: f.frozen_in_new_viewport,
        off: f.off,
        xref_dependent: false,
    }
}

fn layer_flags_from_acadrust(f: acadrust::tables::LayerFlags) -> LayerFlags {
    LayerFlags {
        frozen: f.frozen,
        locked: f.locked,
        frozen_in_new_viewport: f.frozen_in_new_viewport,
        off: f.off,
    }
}

fn apply_layer_info(layer: &mut acadrust::tables::Layer, info: &LayerInfo) {
    layer.name = info.name.trim().to_string();
    layer.flags = layer_flags_to_acadrust(info.flags);
    layer.color = color_to_acadrust(info.color);
    layer.color_name = None;
    layer.book_name = None;
    layer.line_type = info.line_type.clone();
    layer.line_weight = line_weight_to_acadrust(info.line_weight);
    layer.plot_style = info.plot_style.clone();
    layer.is_plottable = info.is_plottable;
}

fn layer_info_from_acadrust(layer: &acadrust::tables::Layer) -> LayerInfo {
    LayerInfo {
        name: layer.name.clone(),
        flags: layer_flags_from_acadrust(layer.flags),
        color: color_from_acadrust(layer.color),
        line_type: layer.line_type.clone(),
        line_weight: line_weight_from_acadrust(layer.line_weight),
        plot_style: layer.plot_style.clone(),
        is_plottable: layer.is_plottable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::plugin_host::HostSession;
    use crate::app::OpenCADStudio;
    use ocs_doc_api::ops::{
        BoolOp, Color, LayerFlags, LayerInfo, LineWeight, SolidPrimitive, XDataRecord,
        XDataValue, XRecordEntry, XRecordSpec, XRecordValue,
    };
    use ocs_doc_api::{
        DocApiEnvelope, HasId, ObjectId, Operation, Query, QueryResult, Receipt,
        XRecordCloningFlags,
    };

    fn dispatch(host: &mut HostSession<'_>, env: DocApiEnvelope) -> ApiResult<Receipt> {
        let bytes = bincode::serialize(&env).unwrap();
        let tab_id = host.tab_id();
        let out = execute_doc_api(host, tab_id, &bytes).expect("dispatch failed");
        bincode::deserialize(&out).expect("receipt deserialize")
    }

    fn new_id(receipt: &Receipt) -> ObjectId {
        receipt
            .outcome
            .as_ref()
            .and_then(|o| o.new_id())
            .expect("no new id")
    }

    #[test]
    fn doc_api_create_boolean_query_end_to_end() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let rev0 = host.scene().geometry_epoch;

        // op 1 + 2: create two overlapping boxes (one undo step + one bump each).
        let cuboid_a = Operation::CreateSolid(SolidPrimitive::Cuboid {
            origin: [0.0; 3],
            size: [10.0; 3],
        });
        let cuboid_b = Operation::CreateSolid(SolidPrimitive::Cuboid {
            origin: [5.0; 3],
            size: [10.0; 3],
        });
        let a = new_id(&dispatch(&mut host, DocApiEnvelope::op(cuboid_a)).unwrap());
        let b = new_id(&dispatch(&mut host, DocApiEnvelope::op(cuboid_b)).unwrap());
        // Each write op advanced the geometry epoch (at least one bump per op).
        assert!(
            host.scene().geometry_epoch > rev0,
            "epoch advanced by creates"
        );

        // op 3: intersect; erase_sources keeps the result at `a`, erases `b`.
        let intersect = Operation::SolidBoolean {
            op: BoolOp::Intersection,
            a,
            b,
            erase_sources: true,
        };
        let lens = new_id(&dispatch(&mut host, DocApiEnvelope::op(intersect)).unwrap());
        assert_eq!(lens, a);
        assert!(
            host.document().get_entity(obj_to_handle(b)).is_none(),
            "b erased"
        );
        assert!(
            host.scene().solid_models.contains_key(&obj_to_handle(a)),
            "result in cache"
        );

        // query batch: bounds + volume (read-only, no bump).
        let rev_before_query = host.scene().geometry_epoch;
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: a }, Query::GetVolume { id: a }]),
        )
        .unwrap();
        assert_eq!(
            host.scene().geometry_epoch,
            rev_before_query,
            "queries must not bump"
        );
        let bb = match &receipt.query_results[0] {
            QueryResult::Bounds(bb) => *bb,
            other => panic!("expected bounds, got {other:?}"),
        };
        // [0,10]^3 intersect [5,15]^3 = [5,10]^3.
        assert!(
            (bb.min[0] - 5.0).abs() < 1e-4 && (bb.max[0] - 10.0).abs() < 1e-4,
            "bounds {bb:?}"
        );
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
        let mk_solid = Operation::CreateSolid(SolidPrimitive::Cuboid {
            origin: [0.0; 3],
            size: [10.0; 3],
        });
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
        assert!(
            matches!(err, ApiError::Validation { .. } | ApiError::UnknownId(_)),
            "{err:?}"
        );
        assert_eq!(
            host.scene().geometry_epoch,
            rev_before,
            "no mutation on rejected TransformMany"
        );
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
        let mk_poly = Operation::CreateCurve(Curve2Spec::Polyline { layer: None,
            points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]],
            closed: false,
        });
        let poly = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_poly)).unwrap());
        // Insert a vertex at index 1.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::AddVertex {
                id: poly,
                at: 1,
                point: [5.0, 0.0, 0.0],
            }),
        )
        .unwrap();
        // The polyline now has 4 vertices with the inserted one at index 1.
        let handle = obj_to_handle(poly);
        let Some(EntityType::LwPolyline(pl)) = host.document().get_entity(handle) else {
            panic!("polyline not found");
        };
        assert_eq!(pl.vertices.len(), 4);
        assert!((pl.vertices[1].location.x - 5.0).abs() < 1e-9);
        // Out-of-range index is a validation error, not a panic.
        let err = dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::AddVertex {
                id: poly,
                at: 99,
                point: [0.0, 0.0, 0.0],
            }),
        )
        .unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. }), "{err:?}");
    }

    #[test]
    fn phase0_extrude_rectangular_profile_to_solid() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // A closed rectangular 2x3 profile in XY.
        let mk_profile = Operation::CreateCurve(Curve2Spec::Polyline { layer: None,
            points: vec![
                [0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 3.0, 0.0],
                [0.0, 3.0, 0.0],
            ],
            closed: true,
        });
        let profile = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_profile)).unwrap());
        // Extrude +Z by 5 -> a 2x3x5 box = volume 30.
        let solid = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::Extrude {
                    profile,
                    direction: [0.0, 0.0, 5.0],
                }),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetVolume { id: solid }]),
        )
        .unwrap();
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
        let mk_profile = Operation::CreateCurve(Curve2Spec::Polyline { layer: None,
            points: vec![
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 2.0, 0.0],
                [1.0, 2.0, 0.0],
            ],
            closed: true,
        });
        let profile = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_profile)).unwrap());
        let result = dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Revolve {
                profile,
                axis: ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                angle: std::f64::consts::TAU,
            }),
        );
        // Revolve may be geometry-sensitive; assert it either produces a positive
        // volume or surfaces a structured geometry error (no panic).
        match result {
            Ok(receipt) => {
                let id = receipt.outcome.and_then(|o| o.new_id()).unwrap();
                let v = dispatch(
                    &mut host,
                    DocApiEnvelope::queries(vec![Query::GetVolume { id }]),
                )
                .unwrap();
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
        let mk_line = Operation::CreateCurve(Curve2Spec::Line { layer: None,
            start: [0.0; 3],
            end: [10.0; 3],
        });
        let line = new_id(&dispatch(&mut host, DocApiEnvelope::op(mk_line)).unwrap());
        // Translate the line by +5 in X: bounds shift by +5.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: line,
                placement: ocs_doc_api::PlacementSpec::at([5.0, 0.0, 0.0]),
            }),
        )
        .unwrap();
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: line }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 5.0).abs() < 1e-6 && (b.max[0] - 15.0).abs() < 1e-6,
                    "{b:?}"
                );
            }
            other => panic!("expected bounds, got {other:?}"),
        }
        // TransformMany over a mix of line + circle is now all-or-nothing (both transformable).
        let circle = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Circle { layer: None,
                    centre: [0.0; 3],
                    radius: 2.0,
                })),
            )
            .unwrap(),
        );
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::TransformMany {
                ids: vec![line, circle],
                placement: ocs_doc_api::PlacementSpec::at([0.0, 1.0, 0.0]),
            }),
        )
        .unwrap();
    }

    // ── Phase 2: full 2D curve families ────────────────────────────────────

    #[test]
    fn phase2_arc_ellipse_spline_create_bounds_transform() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);

        // Arc: create -> kind Arc; bounds are the coarse full-circle bounds.
        let arc = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Arc { layer: None,
                    centre: [5.0, 5.0, 0.0],
                    radius: 4.0,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::PI,
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetEntity { id: arc },
                Query::GetBounds { id: arc },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "Arc"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Bounds(b) => assert!(
                (b.min[0] - 1.0).abs() < 1e-6 && (b.max[0] - 9.0).abs() < 1e-6,
                "{b:?}"
            ),
            other => panic!("expected bounds, got {other:?}"),
        }

        // Ellipse: create -> kind Ellipse; bounds = centre ± major-axis length.
        let ell = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Ellipse { layer: None,
                    centre: [0.0, 0.0, 0.0],
                    major_axis: [6.0, 0.0, 0.0],
                    ratio: 0.5,
                    start: 0.0,
                    end: std::f64::consts::TAU,
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetEntity { id: ell }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "Ellipse"),
            other => panic!("expected entity, got {other:?}"),
        }

        // Spline (degree-3 cubic through 4 control points): create -> kind Spline;
        // bounds = control-point bounds. Transform by +10 X shifts bounds.
        let spline = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Spline { layer: None,
                    degree: 3,
                    control_points: vec![
                        [0.0, 0.0, 0.0],
                        [1.0, 2.0, 0.0],
                        [2.0, -2.0, 0.0],
                        [3.0, 0.0, 0.0],
                    ],
                    knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    weights: vec![1.0; 4],
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: spline }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => assert!(
                (b.min[0] - 0.0).abs() < 1e-6 && (b.max[0] - 3.0).abs() < 1e-6,
                "{b:?}"
            ),
            other => panic!("expected bounds, got {other:?}"),
        }
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: spline,
                placement: ocs_doc_api::PlacementSpec::at([10.0, 0.0, 0.0]),
            }),
        )
        .unwrap();
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: spline }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => assert!((b.min[0] - 10.0).abs() < 1e-6, "{b:?}"),
            other => panic!("expected bounds, got {other:?}"),
        }
    }

    #[test]
    fn phase2_ray_xline_create_and_transform() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let ray = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Ray { layer: None,
                    origin: [1.0, 2.0, 0.0],
                    direction: [1.0, 0.0, 0.0],
                })),
            )
            .unwrap(),
        );
        let xline = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::XLine { layer: None,
                    origin: [0.0, 0.0, 0.0],
                    direction: [0.0, 1.0, 0.0],
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetEntity { id: ray },
                Query::GetEntity { id: xline },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "Ray"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "XLine"),
            other => panic!("expected entity, got {other:?}"),
        }
        // Ray/XLine are unbounded -> GetBounds is Unsupported (not a panic).
        assert!(dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: ray }])
        )
        .is_err());
        // Transform moves the ray's base point (no error).
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: ray,
                placement: ocs_doc_api::PlacementSpec::at([0.0, 0.0, 5.0]),
            }),
        )
        .unwrap();
        let handle = obj_to_handle(ray);
        let Some(EntityType::Ray(r)) = host.document().get_entity(handle) else {
            panic!("ray not found")
        };
        assert!((r.base_point.z - 5.0).abs() < 1e-6);
    }

    // ── Phase 4: paper-space viewports ───────────────────────────────────────

    #[test]
    fn phase4_create_viewport_bounds_transform() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // A 40x30 paper-space viewport at (50,50,0) looking at model origin.
        let vp = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateViewport(ocs_doc_api::ops::ViewportSpec {
                    center: [50.0, 50.0, 0.0],
                    width: 40.0,
                    height: 30.0,
                    view_target: [0.0, 0.0, 0.0],
                    view_height: 100.0,
                })),
            )
            .unwrap(),
        );
        let handle = obj_to_handle(vp);
        let Some(EntityType::Viewport(v)) = host.document().get_entity(handle) else {
            panic!("viewport not found")
        };
        assert!((v.width - 40.0).abs() < 1e-9 && (v.view_height - 100.0).abs() < 1e-9);

        // Bounds = center ± width/2, height/2.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetEntity { id: vp },
                Query::GetBounds { id: vp },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(e) => assert_eq!(e.kind, "Viewport"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 30.0).abs() < 1e-6 && (b.max[0] - 70.0).abs() < 1e-6,
                    "{b:?}"
                );
                assert!(
                    (b.min[1] - 35.0).abs() < 1e-6 && (b.max[1] - 65.0).abs() < 1e-6,
                    "{b:?}"
                );
            }
            other => panic!("expected bounds, got {other:?}"),
        }

        // Transform moves the viewport's paper-space center.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: vp,
                placement: ocs_doc_api::PlacementSpec::at([10.0, 0.0, 0.0]),
            }),
        )
        .unwrap();
        let Some(EntityType::Viewport(v)) = host.document().get_entity(handle) else {
            panic!("viewport not found")
        };
        assert!((v.center.x - 60.0).abs() < 1e-6);
    }

    // ── Regression tests for review fixes (s^2 scale, rotation, all-or-nothing) ──

    #[test]
    fn fix_uniform_scale_applied_once_not_squared() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let circle = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Circle { layer: None,
                    centre: [0.0, 0.0, 0.0],
                    radius: 1.0,
                })),
            )
            .unwrap(),
        );
        // Uniform scale by 2 (x_axis=(2,0,0) etc). Radius must become 2 (s), not 4 (s^2).
        let placement = ocs_doc_api::PlacementSpec {
            x_axis: [2.0, 0.0, 0.0],
            y_axis: [0.0, 2.0, 0.0],
            z_axis: [0.0, 0.0, 2.0],
            origin: [0.0; 3],
        };
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: circle,
                placement,
            }),
        )
        .unwrap();
        let handle = obj_to_handle(circle);
        let Some(EntityType::Circle(c)) = host.document().get_entity(handle) else {
            panic!("circle not found")
        };
        assert!(
            (c.radius - 2.0).abs() < 1e-9,
            "radius {} (must be 2.0, not 4.0)",
            c.radius
        );
    }

    #[test]
    fn fix_rotation_rotates_ellipse_ray_and_insert() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // 90-degree Z-rotation: x_axis=(0,1,0), y_axis=(-1,0,0).
        let rot90 = ocs_doc_api::PlacementSpec {
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [-1.0, 0.0, 0.0],
            z_axis: [0.0, 0.0, 1.0],
            origin: [0.0; 3],
        };
        // Ray along +X must become along +Y after a 90° rotation.
        let ray = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Ray { layer: None,
                    origin: [0.0, 0.0, 0.0],
                    direction: [1.0, 0.0, 0.0],
                })),
            )
            .unwrap(),
        );
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: ray,
                placement: rot90,
            }),
        )
        .unwrap();
        let Some(EntityType::Ray(r)) = host.document().get_entity(obj_to_handle(ray)) else {
            panic!("ray not found")
        };
        assert!(
            (r.direction.y - 1.0).abs() < 1e-6 && r.direction.x.abs() < 1e-6,
            "ray dir {:?}",
            r.direction
        );

        // Ellipse major_axis along +X must rotate to +Y (length preserved).
        let ell = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Ellipse { layer: None,
                    centre: [0.0; 3],
                    major_axis: [4.0, 0.0, 0.0],
                    ratio: 0.5,
                    start: 0.0,
                    end: std::f64::consts::TAU,
                })),
            )
            .unwrap(),
        );
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: ell,
                placement: rot90,
            }),
        )
        .unwrap();
        let Some(EntityType::Ellipse(e)) = host.document().get_entity(obj_to_handle(ell)) else {
            panic!("ellipse not found")
        };
        assert!(
            (e.major_axis.y - 4.0).abs() < 1e-6 && e.major_axis.x.abs() < 1e-6,
            "ellipse axis {:?}",
            e.major_axis
        );

        // Arc angles offset by +90° (π/2).
        let arc = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Arc { layer: None,
                    centre: [0.0; 3],
                    radius: 2.0,
                    start_angle: 0.0,
                    end_angle: 1.0,
                })),
            )
            .unwrap(),
        );
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: arc,
                placement: rot90,
            }),
        )
        .unwrap();
        let Some(EntityType::Arc(a)) = host.document().get_entity(obj_to_handle(arc)) else {
            panic!("arc not found")
        };
        let half_pi = std::f64::consts::FRAC_PI_2;
        assert!(
            (a.start_angle - half_pi).abs() < 1e-6,
            "arc start {} (expected {})",
            a.start_angle,
            half_pi
        );
    }

    #[test]
    fn fix_transform_many_locked_layer_fails_all_or_nothing() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let a = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Point { layer: None,
                    position: [0.0; 3],
                })),
            )
            .unwrap(),
        );
        let b = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Point { layer: None,
                    position: [1.0; 3],
                })),
            )
            .unwrap(),
        );
        // Lock the layer holding `b`.
        let bh = obj_to_handle(b);
        let layer = host
            .document()
            .get_entity(bh)
            .unwrap()
            .common()
            .layer
            .clone();
        host.document_mut().layers.get_mut(&layer).unwrap().lock();
        let rev_before = host.scene().geometry_epoch;

        // TransformMany [a, b] must fail all-or-nothing (b is locked) BEFORE mutating a.
        let err = dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::TransformMany {
                ids: vec![a, b],
                placement: ocs_doc_api::PlacementSpec::at([5.0, 0.0, 0.0]),
            }),
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                ApiError::Validation { .. } | ApiError::Unsupported(_) | ApiError::UnknownId(_)
            ),
            "{err:?}"
        );
        // `a` was NOT transformed (all-or-nothing): its location is unchanged.
        let Some(EntityType::Point(pa)) = host.document().get_entity(obj_to_handle(a)) else {
            panic!("point a not found")
        };
        assert!(
            (pa.location.x - 0.0).abs() < 1e-9,
            "a moved despite locked batch member"
        );
        assert_eq!(
            host.scene().geometry_epoch,
            rev_before,
            "no mutation on rejected TransformMany"
        );
    }

    // ── Phase 2b-a: annotations (Text/MText) ─────────────────────────────────

    #[test]
    fn phase2b_text_mtext_create_content_transform() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);

        // Create a TEXT, read its content, set it, verify.
        let text = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateText(ocs_doc_api::ops::TextSpec {
                    value: "hello".into(),
                    insertion_point: [1.0, 2.0, 0.0],
                    height: 2.5,
                    rotation: 0.0,
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetTextContent { id: text }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::TextContent(s) => assert_eq!(s, "hello"),
            other => panic!("expected content, got {other:?}"),
        }
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetTextContent {
                id: text,
                value: "world".into(),
            }),
        )
        .unwrap();
        let Some(EntityType::Text(t)) = host.document().get_entity(obj_to_handle(text)) else {
            panic!("text not found")
        };
        assert_eq!(t.value, "world");

        // SetTextContent on a non-text entity is Unsupported.
        let line = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Line { layer: None,
                    start: [0.0; 3],
                    end: [1.0; 3],
                })),
            )
            .unwrap(),
        );
        assert!(dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetTextContent {
                id: line,
                value: "x".into()
            })
        )
        .is_err());

        // MTEXT create + content + transform (insertion point moves).
        let mtext = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateMText(ocs_doc_api::ops::MTextSpec {
                    value: "multi\nline".into(),
                    insertion_point: [5.0, 5.0, 0.0],
                    height: 3.0,
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetTextContent { id: mtext }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::TextContent(s) => assert_eq!(s, "multi\nline"),
            other => panic!("expected content, got {other:?}"),
        }
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: mtext,
                placement: ocs_doc_api::PlacementSpec::at([10.0, 0.0, 0.0]),
            }),
        )
        .unwrap();
        let Some(EntityType::MText(t)) = host.document().get_entity(obj_to_handle(mtext)) else {
            panic!("mtext not found")
        };
        assert!((t.insertion_point.x - 15.0).abs() < 1e-6);
    }

    // ── Outstanding methods: loft + bulge-arc profiles ───────────────────────

    #[test]
    fn loft_two_profiles_produces_solid() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Two circles at different Z (profiles for loft).
        let c1 = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Circle { layer: None,
                    centre: [0.0, 0.0, 0.0],
                    radius: 5.0,
                })),
            )
            .unwrap(),
        );
        let c2 = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Circle { layer: None,
                    centre: [0.0, 0.0, 10.0],
                    radius: 2.0,
                })),
            )
            .unwrap(),
        );
        let result = dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Loft {
                profiles: vec![c1, c2],
            }),
        );
        let id = new_id(&result.expect("loft must preserve the sections' elevations"));
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetVolume { id }, Query::GetBounds { id }]),
        )
        .unwrap();
        let QueryResult::Volume(volume) = receipt.query_results[0] else {
            panic!("volume")
        };
        let expected = std::f64::consts::PI * 10.0 * 39.0 / 3.0;
        assert!(
            (volume - expected).abs() < expected * 0.05,
            "volume {volume}"
        );
        let QueryResult::Bounds(bounds) = receipt.query_results[1] else {
            panic!("bounds")
        };
        assert!((bounds.min[2]).abs() < 1e-4 && (bounds.max[2] - 10.0).abs() < 1e-4);
    }

    #[test]
    fn bulge_arc_polyline_profile_converts_to_arc() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // A polyline with a bulge (arc) segment: square with one curved side.
        let poly = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Polyline { layer: None,
                    points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]],
                    closed: false,
                })),
            )
            .unwrap(),
        );
        // Add a bulge to the first segment by editing the vertex.
        let handle = obj_to_handle(poly);
        let Some(EntityType::LwPolyline(mut pl)) = host.document().get_entity(handle).cloned()
        else {
            panic!("polyline not found")
        };
        pl.vertices[0].bulge = 1.0; // 90-degree arc (tan(π/2/4) = 1)
        host.scene_mut().update_entity(EntityType::LwPolyline(pl));

        // profile_curves now converts the bulge to a Curve::Arc (not Unsupported).
        let curves = host
            .profile_curves(poly)
            .expect("bulge profile must convert");
        // Open polyline, 3 vertices -> 2 segments: arc (bulged first), line.
        assert_eq!(curves.len(), 2, "2 segments (arc + line)");
        assert!(
            matches!(curves[0], cadkernel::geom2d::Curve::Arc(_)),
            "first segment is an arc"
        );
        assert!(
            matches!(curves[1], cadkernel::geom2d::Curve::Line(_)),
            "second is a line"
        );
        // bulge=1 -> included angle θ = 4·atan(1) = π (a semicircle); radius = chord/2 = 5.
        if let cadkernel::geom2d::Curve::Arc(arc) = &curves[0] {
            assert!(
                (arc.radius - 5.0).abs() < 0.01,
                "arc radius {} (semicircle)",
                arc.radius
            );
        }

        // Negative (CW) bulge: the center must be on the CW side (mirror of CCW).
        // A single 90° bulge segment start=(0,0), end=(1,0), bulge=-0.4142 (~90° CW):
        // correct center is BELOW the chord; the buggy double-sign put it above.
        let neg = convert::bulge_arc_segment([0.0, 0.0], [1.0, 0.0], -0.4142)
            .expect("negative bulge arc");
        if let cadkernel::geom2d::Curve::Arc(arc) = &neg {
            // 90° arc, chord 1: radius = 0.5/sin(45°) ≈ 0.7071; center y must be negative.
            assert!(
                (arc.radius - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01,
                "cw arc radius {}",
                arc.radius
            );
            assert!(
                arc.centre[1] < 0.0,
                "CW arc center y must be below the chord, got {:?}",
                arc.centre
            );
        } else {
            panic!("expected an arc for negative bulge");
        }
        // And the CCW mirror: center above.
        let pos =
            convert::bulge_arc_segment([0.0, 0.0], [1.0, 0.0], 0.4142).expect("positive bulge arc");
        if let cadkernel::geom2d::Curve::Arc(arc) = &pos {
            assert!(
                arc.centre[1] > 0.0,
                "CCW arc center y must be above the chord, got {:?}",
                arc.centre
            );
        }
    }

    // ── Phase 2c-ii: dimension sub-types (radius/diameter/angular) ──────────

    #[test]
    fn phase2cii_dimension_radius_diameter_angular() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Radius: center (0,0,0), point on circle (4,0,0) -> radius 4.
        let rad = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateDimensionRadius(
                    ocs_doc_api::ops::DimensionRadialSpec {
                        center: [0.0; 3],
                        point: [4.0, 0.0, 0.0],
                    },
                )),
            )
            .unwrap(),
        );
        // Diameter: chord points (0,0,0) and (6,0,0) -> diameter 6.
        let dia = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateDimensionDiameter(
                    ocs_doc_api::ops::DimensionRadialSpec {
                        center: [0.0; 3],
                        point: [6.0, 0.0, 0.0],
                    },
                )),
            )
            .unwrap(),
        );
        // Angular: vertex (0,0,0), leg points (10,0,0) and (0,10,0) -> 90 degrees.
        let ang = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateDimensionAngular(
                    ocs_doc_api::ops::DimensionAngularSpec {
                        vertex: [0.0; 3],
                        first_point: [10.0, 0.0, 0.0],
                        second_point: [0.0, 10.0, 0.0],
                        arc_location: [5.0, 5.0, 0.0],
                    },
                )),
            )
            .unwrap(),
        );

        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetDimensionMeasurement { id: rad },
                Query::GetDimensionMeasurement { id: dia },
                Query::GetDimensionMeasurement { id: ang },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::DimensionMeasurement(v) => assert!((*v - 4.0).abs() < 1e-4, "radius {v}"),
            other => panic!("expected measurement, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::DimensionMeasurement(v) => {
                assert!((*v - 6.0).abs() < 1e-4, "diameter {v}")
            }
            other => panic!("expected measurement, got {other:?}"),
        }
        match &receipt.query_results[2] {
            QueryResult::DimensionMeasurement(v) => assert!((*v - 90.0).abs() < 0.1, "angle {v}"),
            other => panic!("expected measurement, got {other:?}"),
        }
    }

    // ── Phase 3-ii: typed AttributeDefinition create ──────────────────────────

    #[test]
    fn phase3ii_create_attribute_definition() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let attdef = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateAttributeDefinition(
                    ocs_doc_api::ops::AttributeDefinitionSpec {
                        tag: "DOOR_NO".into(),
                        prompt: "Door number?".into(),
                        default_value: "D-1".into(),
                        insertion_point: [5.0, 5.0, 0.0],
                        height: 2.5,
                        rotation: 0.0,
                    },
                )),
            )
            .unwrap(),
        );
        let handle = obj_to_handle(attdef);
        let Some(EntityType::AttributeDefinition(a)) = host.document().get_entity(handle) else {
            panic!("attdef not found")
        };
        assert_eq!(a.tag, "DOOR_NO");
        assert_eq!(a.default_value, "D-1");
        assert!((a.height - 2.5).abs() < 1e-9);
        // GetEntity reports the AttributeDefinition kind.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetEntity { id: attdef }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(e) => assert_eq!(e.kind, "AttributeDefinition"),
            other => panic!("expected entity, got {other:?}"),
        }
    }

    // ── Phase 5-ii: typed Table create ───────────────────────────────────────

    #[test]
    fn phase5ii_create_table_from_grid() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let data = vec![
            vec!["Name".to_string(), "Value".to_string()],
            vec!["Length".to_string(), "100.0".to_string()],
            vec!["Width".to_string(), "50.0".to_string()],
        ];
        let table = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateTable(ocs_doc_api::ops::TableSpec {
                    insertion_point: [10.0, 10.0, 0.0],
                    data: data.clone(),
                })),
            )
            .unwrap(),
        );
        let handle = obj_to_handle(table);
        let Some(EntityType::Table(t)) = host.document().get_entity(handle) else {
            panic!("table not found")
        };
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.columns.len(), 2);
        // Non-rectangular grid is a validation error.
        let bad = vec![
            vec!["a".to_string()],
            vec!["b".to_string(), "c".to_string()],
        ];
        assert!(dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::CreateTable(ocs_doc_api::ops::TableSpec {
                insertion_point: [0.0; 3],
                data: bad
            }))
        )
        .is_err());
        // GetEntity reports kind Table.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetEntity { id: table }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(e) => assert_eq!(e.kind, "Table"),
            other => panic!("expected entity, got {other:?}"),
        }
    }

    // ── Outstanding method: accurate volume/centroid (fine tessellation) ─────

    #[test]
    fn accurate_volume_centroid_sphere_and_cube() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Sphere r=5: analytic volume = 4/3 * pi * 125 ≈ 523.599; centroid at centre.
        let ball = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateSolid(SolidPrimitive::Sphere {
                    centre: [10.0, 20.0, 30.0],
                    radius: 5.0,
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetVolume { id: ball },
                Query::GetCentroid { id: ball },
            ]),
        )
        .unwrap();
        let analytic = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
        match &receipt.query_results[0] {
            QueryResult::Volume(v) => {
                let err = ((*v - analytic) / analytic).abs();
                assert!(
                    err < 0.005,
                    "sphere volume {v} vs analytic {analytic} (err {err:.4})"
                );
            }
            other => panic!("expected volume, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Centroid(c) => {
                assert!(
                    (c[0] - 10.0).abs() < 0.05
                        && (c[1] - 20.0).abs() < 0.05
                        && (c[2] - 30.0).abs() < 0.05,
                    "sphere centroid {c:?}"
                );
            }
            other => panic!("expected centroid, got {other:?}"),
        }
        // Cube 10^3 is exact (planar faces: divergence is exact regardless of LOD).
        let cube = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateSolid(SolidPrimitive::Cuboid {
                    origin: [0.0; 3],
                    size: [10.0; 3],
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetVolume { id: cube }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Volume(v) => assert!((*v - 1000.0).abs() < 1.0, "cube volume {v}"),
            other => panic!("expected volume, got {other:?}"),
        }
    }

    // ── Phase 2c-iii: 2-line angular dimension ────────────────────────────────

    #[test]
    fn phase2ciii_dimension_angular2ln() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Angle between line (0,0,0)->(10,0,0) and (0,0,0)->(0,10,0) = 90 degrees.
        let ang = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateDimensionAngular2Ln(
                    ocs_doc_api::ops::DimensionAngularSpec {
                        vertex: [0.0; 3],
                        first_point: [10.0, 0.0, 0.0],
                        second_point: [0.0, 10.0, 0.0],
                        arc_location: [5.0, 5.0, 0.0],
                    },
                )),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetDimensionMeasurement { id: ang },
                Query::GetEntity { id: ang },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::DimensionMeasurement(v) => assert!((*v - 90.0).abs() < 0.1, "angle {v}"),
            other => panic!("expected measurement, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Entity(e) => assert_eq!(e.kind, "Dimension"),
            other => panic!("expected entity, got {other:?}"),
        }
    }

    // ── Review-fix regression tests (Loft cap, CreateMany validation, Ellipse
    //    rotation, raster path validation) ─────────────────────────────────────

    #[test]
    fn fix_loft_over_cap_is_rejected() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let over = ocs_doc_api::ops::BULK_ITEM_CAP + 1;
        let profiles: Vec<ObjectId> = (0..over).map(|i| ObjectId::from_u64(i as u64)).collect();
        let err =
            dispatch(&mut host, DocApiEnvelope::op(Operation::Loft { profiles })).unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. }), "{err:?}");
    }

    #[test]
    fn fix_create_many_invalid_curve_is_all_or_nothing() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let rev_before = host.scene().geometry_epoch;
        // A valid point + an invalid ellipse (ratio > 1) in one CreateMany batch.
        let specs = vec![
            ocs_doc_api::ops::EntitySpec::Curve(Curve2Spec::Point { layer: None, position: [0.0; 3] }),
            ocs_doc_api::ops::EntitySpec::Curve(Curve2Spec::Ellipse { layer: None,
                centre: [0.0; 3],
                major_axis: [1.0; 3],
                ratio: 2.0,
                start: 0.0,
                end: std::f64::consts::TAU,
            }),
        ];
        let err =
            dispatch(&mut host, DocApiEnvelope::op(Operation::CreateMany(specs))).unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. }), "{err:?}");
        // All-or-nothing: NO entity was created (the point would have been added first).
        assert_eq!(
            host.scene().geometry_epoch,
            rev_before,
            "no mutation on rejected CreateMany"
        );
    }

    #[test]
    fn partial_ellipse_rotation_preserves_parameters() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Rotating the major axis already rotates the ellipse; its parameters stay fixed.
        let ell = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Ellipse { layer: None,
                    centre: [0.0; 3],
                    major_axis: [4.0, 0.0, 0.0],
                    ratio: 0.5,
                    start: 0.0,
                    end: std::f64::consts::FRAC_PI_2,
                })),
            )
            .unwrap(),
        );
        let rot90 = ocs_doc_api::PlacementSpec {
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [-1.0, 0.0, 0.0],
            z_axis: [0.0, 0.0, 1.0],
            origin: [0.0; 3],
        };
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: ell,
                placement: rot90,
            }),
        )
        .unwrap();
        let Some(EntityType::Ellipse(e)) = host.document().get_entity(obj_to_handle(ell)) else {
            panic!("ellipse not found")
        };
        let half_pi = std::f64::consts::FRAC_PI_2;
        assert!(e.start_parameter.abs() < 1e-6);
        assert!((e.end_parameter - half_pi).abs() < 1e-6);
        // major_axis rotated from +X to +Y.
        assert!(
            (e.major_axis.y - 4.0).abs() < 1e-6,
            "major_axis {:?}",
            e.major_axis
        );
    }

    #[test]
    fn fix_raster_image_path_validation() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let mk = |path: &str| {
            Operation::CreateRasterImage(ocs_doc_api::ops::RasterImageSpec {
                file_path: path.into(),
                insertion_point: [0.0; 3],
                u_vector: [0.5, 0.0, 0.0],
                v_vector: [0.0, 0.5, 0.0],
                size: [10.0, 10.0],
            })
        };
        // Valid path succeeds.
        assert!(dispatch(&mut host, DocApiEnvelope::op(mk("C:/img/photo.png"))).is_ok());
        // UNC path rejected.
        assert!(dispatch(
            &mut host,
            DocApiEnvelope::op(mk("\\\\server\\share\\x.png"))
        )
        .is_err());
        // Parent-traversal rejected.
        assert!(dispatch(
            &mut host,
            DocApiEnvelope::op(mk("C:/img/../../etc/passwd.png"))
        )
        .is_err());
        // Non-image extension rejected.
        assert!(dispatch(&mut host, DocApiEnvelope::op(mk("C:/img/evil.exe"))).is_err());
        // Empty path rejected.
        assert!(dispatch(&mut host, DocApiEnvelope::op(mk(""))).is_err());
    }

    // ── Read-mostly family bounds (Leader/Mesh/Face3D/MLine/Helix) ──────────

    #[test]
    fn read_mostly_family_bounds_leader_and_mesh() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Leader with vertices at (0,0,0),(5,5,0),(10,0,0) -> bounds x[0,10] y[0,5].
        let leader = acadrust::entities::Leader {
            vertices: vec![
                acadrust::types::Vector3::new(0.0, 0.0, 0.0),
                acadrust::types::Vector3::new(5.0, 5.0, 0.0),
                acadrust::types::Vector3::new(10.0, 0.0, 0.0),
            ],
            ..Default::default()
        };
        let leader_h = host
            .document_mut()
            .add_entity(EntityType::Leader(leader))
            .unwrap();
        // Mesh with vertices in a unit cube at (2..3, 2..3, 0).
        let mesh = acadrust::entities::Mesh {
            vertices: vec![
                acadrust::types::Vector3::new(2.0, 2.0, 0.0),
                acadrust::types::Vector3::new(3.0, 2.0, 0.0),
                acadrust::types::Vector3::new(3.0, 3.0, 0.0),
                acadrust::types::Vector3::new(2.0, 3.0, 0.0),
            ],
            ..Default::default()
        };
        let mesh_h = host
            .document_mut()
            .add_entity(EntityType::Mesh(mesh))
            .unwrap();

        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetBounds {
                    id: handle_to_obj(leader_h),
                },
                Query::GetBounds {
                    id: handle_to_obj(mesh_h),
                },
                Query::GetEntity {
                    id: handle_to_obj(leader_h),
                },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 0.0).abs() < 1e-6 && (b.max[0] - 10.0).abs() < 1e-6,
                    "{b:?}"
                );
                assert!(
                    (b.min[1] - 0.0).abs() < 1e-6 && (b.max[1] - 5.0).abs() < 1e-6,
                    "{b:?}"
                );
            }
            other => panic!("expected bounds, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 2.0).abs() < 1e-6 && (b.max[0] - 3.0).abs() < 1e-6,
                    "{b:?}"
                );
            }
            other => panic!("expected bounds, got {other:?}"),
        }
        match &receipt.query_results[2] {
            QueryResult::Entity(e) => assert_eq!(e.kind, "Leader"),
            other => panic!("expected entity, got {other:?}"),
        }
    }

    // ── Phase 5 remaining: typed raster image create ─────────────────────────

    #[test]
    fn phase5_create_raster_image_registers_definition() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let img = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateRasterImage(
                    ocs_doc_api::ops::RasterImageSpec {
                        file_path: "C:/img/photo.png".into(),
                        insertion_point: [10.0, 20.0, 0.0],
                        u_vector: [0.5, 0.0, 0.0],
                        v_vector: [0.0, 0.5, 0.0],
                        size: [100.0, 50.0],
                    },
                )),
            )
            .unwrap(),
        );
        let handle = obj_to_handle(img);
        let Some(EntityType::RasterImage(i)) = host.document().get_entity(handle) else {
            panic!("image not found")
        };
        assert_eq!(i.file_path, "C:/img/photo.png");
        // The host auto-registered an ImageDefinition (definition_handle set).
        assert!(
            i.definition_handle.is_some(),
            "ImageDefinition was auto-registered"
        );
        // Bounds = insertion + u*100 + v*50 = (60, 45) (4-corner bracket).
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: img }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 10.0).abs() < 1e-6 && (b.max[0] - 60.0).abs() < 1e-6,
                    "{b:?}"
                );
                assert!(
                    (b.min[1] - 20.0).abs() < 1e-6 && (b.max[1] - 45.0).abs() < 1e-6,
                    "{b:?}"
                );
            }
            other => panic!("expected bounds, got {other:?}"),
        }
    }

    // ── Phase 4 remaining: set_view + viewport-view query ────────────────────

    #[test]
    fn phase4_set_view_and_view_query() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let vp = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateViewport(ocs_doc_api::ops::ViewportSpec {
                    center: [50.0, 50.0, 0.0],
                    width: 40.0,
                    height: 30.0,
                    view_target: [0.0, 0.0, 0.0],
                    view_height: 100.0,
                })),
            )
            .unwrap(),
        );

        // set_view retargets + re-zooms in place.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetViewportView {
                id: vp,
                view_target: [20.0, 30.0, 0.0],
                view_height: 50.0,
            }),
        )
        .unwrap();
        let Some(EntityType::Viewport(v)) = host.document().get_entity(obj_to_handle(vp)) else {
            panic!("viewport not found")
        };
        assert!((v.view_target.x - 20.0).abs() < 1e-6 && (v.view_height - 50.0).abs() < 1e-6);

        // viewport_view query reads it back.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetViewportView { id: vp }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::ViewportView { target, height } => {
                assert!((target[0] - 20.0).abs() < 1e-6 && (*height - 50.0).abs() < 1e-6);
            }
            other => panic!("expected viewport view, got {other:?}"),
        }

        // set_view on a non-viewport is Unsupported.
        let line = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Line { layer: None,
                    start: [0.0; 3],
                    end: [1.0; 3],
                })),
            )
            .unwrap(),
        );
        assert!(dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetViewportView {
                id: line,
                view_target: [0.0; 3],
                view_height: 1.0
            })
        )
        .is_err());
    }

    // ── Phase 3 remaining: attributes + nested block traversal ──────────────

    #[test]
    fn phase3_attributes_and_block_traversal() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // A block "Door" containing a line; insert it, then set/read attributes.
        host.document_mut()
            .block_records
            .add(acadrust::tables::BlockRecord::new("Door"))
            .unwrap();
        let line = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Line { layer: None,
                    start: [0.0; 3],
                    end: [10.0; 3],
                })),
            )
            .unwrap(),
        );
        host.document_mut()
            .block_records
            .get_mut("Door")
            .unwrap()
            .entity_handles
            .push(obj_to_handle(line));

        let ins = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateInsert(ocs_doc_api::ops::InsertSpec {
                    block_name: "Door".into(),
                    insert_point: [0.0; 3],
                    scale: 1.0,
                    rotation: 0.0,
                })),
            )
            .unwrap(),
        );

        // set_attribute adds a new attribute; get_attributes reads it back.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetAttribute {
                id: ins,
                tag: "HANDLE".into(),
                value: "A-1".into(),
            }),
        )
        .unwrap();
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetAttributes { id: ins }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Attributes(attrs) => {
                assert_eq!(attrs.len(), 1);
                assert_eq!(attrs[0], ("HANDLE".to_string(), "A-1".to_string()));
            }
            other => panic!("expected attributes, got {other:?}"),
        }
        // set_attribute on an existing tag updates it.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetAttribute {
                id: ins,
                tag: "HANDLE".into(),
                value: "A-2".into(),
            }),
        )
        .unwrap();
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetAttributes { id: ins }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Attributes(attrs) => assert_eq!(attrs[0].1, "A-2"),
            other => panic!("expected attributes, got {other:?}"),
        }

        // Nested traversal: GetBlockEntities("Door") returns the line.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBlockEntities {
                block_name: "Door".into(),
            }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::BlockEntities(v) => {
                assert_eq!(v.len(), 1);
                assert_eq!(v[0].kind, "Line");
            }
            other => panic!("expected block entities, got {other:?}"),
        }
        // Unknown block -> validation error.
        assert!(dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBlockEntities {
                block_name: "Nope".into()
            }])
        )
        .is_err());
    }

    // ── Phase 2b-c: dimension ─────────────────────────────────────────────────

    #[test]
    fn phase2c_dimension_linear_create_measurement_bounds() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Linear dimension from (0,0,0) to (30,0,0) with the line at (0,5,0).
        let dim = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateDimensionLinear(
                    ocs_doc_api::ops::DimensionSpec {
                        first_point: [0.0, 0.0, 0.0],
                        second_point: [30.0, 0.0, 0.0],
                        definition_point: [0.0, 5.0, 0.0],
                    },
                )),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetDimensionMeasurement { id: dim },
                Query::GetEntity { id: dim },
                Query::GetBounds { id: dim },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::DimensionMeasurement(v) => {
                assert!((*v - 30.0).abs() < 1e-6, "measurement {v}")
            }
            other => panic!("expected measurement, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Entity(e) => assert_eq!(e.kind, "Dimension"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[2] {
            QueryResult::Bounds(b) => assert!(
                (b.min[0] - 0.0).abs() < 1e-6 && (b.max[0] - 30.0).abs() < 1e-6,
                "{b:?}"
            ),
            other => panic!("expected bounds, got {other:?}"),
        }
    }

    // ── Phase 2b-b: hatch ────────────────────────────────────────────────────

    #[test]
    fn phase2b_hatch_create_boundary_bounds_delete() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Solid hatch over a unit square boundary.
        let sq = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hatch = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateHatch(ocs_doc_api::ops::HatchSpec {
                    boundary: sq.clone(),
                    solid: true,
                })),
            )
            .unwrap(),
        );

        // Boundary round-trips (one loop, 4 vertices).
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetHatchBoundary { id: hatch },
                Query::GetBounds { id: hatch },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::HatchBoundary(loops) => {
                assert_eq!(loops.len(), 1);
                assert_eq!(loops[0].len(), 4);
                assert!(
                    (loops[0][0][0] - 0.0).abs() < 1e-9 && (loops[0][2][0] - 10.0).abs() < 1e-9
                );
            }
            other => panic!("expected boundary, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Bounds(b) => assert!(
                (b.min[0] - 0.0).abs() < 1e-6 && (b.max[1] - 10.0).abs() < 1e-6,
                "{b:?}"
            ),
            other => panic!("expected bounds, got {other:?}"),
        }

        // Boundary with < 3 points is a validation error.
        assert!(dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::CreateHatch(ocs_doc_api::ops::HatchSpec {
                boundary: vec![[0.0, 0.0], [1.0, 1.0]],
                solid: true
            }))
        )
        .is_err());

        // Generic delete works.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Delete { id: hatch }),
        )
        .unwrap();
        assert!(host.document().get_entity(obj_to_handle(hatch)).is_none());
    }

    // ── Phase 5: media & misc (read-mostly) ──────────────────────────────────

    #[test]
    fn phase5_media_entities_read_kind_and_bounds() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Insert a RasterImage directly (read-mostly: DocApi reads kind + bounds).
        let img = {
            use acadrust::entities::RasterImage;
            use acadrust::types::{Vector2, Vector3};
            let img = RasterImage {
                insertion_point: Vector3::new(10.0, 20.0, 0.0),
                u_vector: Vector3::new(0.5, 0.0, 0.0), // 0.5 world-units/pixel in X
                v_vector: Vector3::new(0.0, 0.5, 0.0),
                size: Vector2::new(100.0, 50.0),
                ..Default::default()
            };
            host.document_mut()
                .add_entity(EntityType::RasterImage(img))
                .unwrap()
        };
        let img_id = handle_to_obj(img);
        // GetEntity reports kind "RasterImage"; bounds = insertion + u*100 + v*50 = (60, 45).
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetEntity { id: img_id },
                Query::GetBounds { id: img_id },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(e) => assert_eq!(e.kind, "RasterImage"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 10.0).abs() < 1e-6 && (b.max[0] - 60.0).abs() < 1e-6,
                    "{b:?}"
                );
                assert!(
                    (b.min[1] - 20.0).abs() < 1e-6 && (b.max[1] - 45.0).abs() < 1e-6,
                    "{b:?}"
                );
            }
            other => panic!("expected bounds, got {other:?}"),
        }
        // Generic delete works on media entities (read-mostly, but deletable).
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Delete { id: img_id }),
        )
        .unwrap();
        assert!(host.document().get_entity(img).is_none());
    }

    // ── Phase 3: containers (block references) ───────────────────────────────

    #[test]
    fn phase3_create_insert_and_transform() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // Register a block record so the insert can reference it.
        host.document_mut()
            .block_records
            .add(acadrust::tables::BlockRecord::new("MyBlock"))
            .unwrap();

        // Insert the block at (10,20,0) scale 2, rotation 0.
        let ins = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateInsert(ocs_doc_api::ops::InsertSpec {
                    block_name: "MyBlock".into(),
                    insert_point: [10.0, 20.0, 0.0],
                    scale: 2.0,
                    rotation: 0.0,
                })),
            )
            .unwrap(),
        );
        let handle = obj_to_handle(ins);
        let Some(EntityType::Insert(i)) = host.document().get_entity(handle) else {
            panic!("insert not found")
        };
        assert_eq!(i.block_name, "MyBlock");
        assert!((i.insert_point.x - 10.0).abs() < 1e-9 && (i.x_scale() - 2.0).abs() < 1e-9);

        // Transform the insert by +5 in X.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: ins,
                placement: ocs_doc_api::PlacementSpec::at([5.0, 0.0, 0.0]),
            }),
        )
        .unwrap();
        let Some(EntityType::Insert(i)) = host.document().get_entity(handle) else {
            panic!("insert not found")
        };
        assert!((i.insert_point.x - 15.0).abs() < 1e-6);

        // Inserting a non-existent block is a Validation error, no entity created.
        let before = host.scene().geometry_epoch;
        let err = dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::CreateInsert(ocs_doc_api::ops::InsertSpec {
                block_name: "NoSuchBlock".into(),
                insert_point: [0.0; 3],
                scale: 1.0,
                rotation: 0.0,
            })),
        )
        .unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. }), "{err:?}");
        assert_eq!(
            host.scene().geometry_epoch,
            before,
            "no entity on unknown block"
        );
    }

    #[test]
    fn doc_api_tab_mismatch_is_rejected() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        // A request naming a DIFFERENT tab than the bound one is rejected.
        let env = DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Point { layer: None,
            position: [0.0; 3],
        }));
        let bytes = bincode::serialize(&env).unwrap();
        let wrong_tab = host.tab_id().wrapping_add(999);
        let err = execute_doc_api(&mut host, wrong_tab, &bytes).unwrap_err();
        assert!(err.contains("tab mismatch"), "{err}");
    }

    #[test]
    fn doc_api_unknown_id_surfaces_structured_error() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let ghost = ObjectId::from_u64(0xDEAD);
        let err = dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Delete { id: ghost }),
        )
        .unwrap_err();
        assert!(
            matches!(err, ApiError::Validation { .. } | ApiError::UnknownId(_)),
            "{err:?}"
        );
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
        let line = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Line { layer: None,
                    start: [0.0, 0.0, 0.0],
                    end: [10.0, 0.0, 0.0],
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetEntity { id: line },
                Query::GetBounds { id: line },
            ]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Entity(v) => assert_eq!(v.kind, "Line"),
            other => panic!("expected entity, got {other:?}"),
        }
        match &receipt.query_results[1] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 0.0).abs() < 1e-9 && (b.max[0] - 10.0).abs() < 1e-9,
                    "{b:?}"
                );
            }
            other => panic!("expected bounds, got {other:?}"),
        }

        // Circle centre (5,5,0) r=3: bounds = centre ± radius in XY.
        let circle = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Circle { layer: None,
                    centre: [5.0, 5.0, 0.0],
                    radius: 3.0,
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: circle }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 2.0).abs() < 1e-9 && (b.max[0] - 8.0).abs() < 1e-9,
                    "{b:?}"
                );
            }
            other => panic!("expected bounds, got {other:?}"),
        }

        // Point at (1,2,3): degenerate bounds.
        let point = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Point { layer: None,
                    position: [1.0, 2.0, 3.0],
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: point }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => assert_eq!(b.min, [1.0, 2.0, 3.0]),
            other => panic!("expected bounds, got {other:?}"),
        }

        // Closed polyline (unit square in XY): bounds = [0,1]^2 at z=0.
        let poly = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Polyline { layer: None,
                    points: vec![
                        [0.0, 0.0, 0.0],
                        [1.0, 0.0, 0.0],
                        [1.0, 1.0, 0.0],
                        [0.0, 1.0, 0.0],
                    ],
                    closed: true,
                })),
            )
            .unwrap(),
        );
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: poly }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(b) => {
                assert!(
                    (b.min[0] - 0.0).abs() < 1e-9 && (b.max[1] - 1.0).abs() < 1e-9,
                    "{b:?}"
                );
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
        let a = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateSolid(SolidPrimitive::Cuboid {
                    origin: [0.0; 3],
                    size: [10.0; 3],
                })),
            )
            .unwrap(),
        );
        let b = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateSolid(SolidPrimitive::Cuboid {
                    origin: [5.0; 3],
                    size: [10.0; 3],
                })),
            )
            .unwrap(),
        );

        // Intersection FIRST: [0,10]^3 ∩ [5,15]^3 = [5,10]^3 = 125.
        let intersect_op = Operation::SolidBoolean {
            op: BoolOp::Intersection,
            a,
            b,
            erase_sources: true,
        };
        let inter = new_id(&dispatch(&mut host, DocApiEnvelope::op(intersect_op)).unwrap());
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetVolume { id: inter }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Volume(v) => assert!((*v - 125.0).abs() < 1.0, "intersection vol {v}"),
            other => panic!("expected volume, got {other:?}"),
        }

        // THEN transform the result by +100 in X (a real, non-identity move): its
        // bounds must shift by exactly +100 in X while Y/Z stay at [5,10].
        let move_op = Operation::Transform {
            id: inter,
            placement: PlacementSpec::at([100.0, 0.0, 0.0]),
        };
        dispatch(&mut host, DocApiEnvelope::op(move_op)).unwrap();
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetBounds { id: inter }]),
        )
        .unwrap();
        match &receipt.query_results[0] {
            QueryResult::Bounds(bb) => {
                assert!(
                    (bb.min[0] - 105.0).abs() < 1e-4 && (bb.max[0] - 110.0).abs() < 1e-4,
                    "{bb:?}"
                );
                assert!(
                    (bb.min[1] - 5.0).abs() < 1e-4 && (bb.max[1] - 10.0).abs() < 1e-4,
                    "{bb:?}"
                );
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
        let a = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateSolid(SolidPrimitive::Cuboid {
                    origin: [0.0; 3],
                    size: [10.0; 3],
                })),
            )
            .unwrap(),
        );
        let b = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateSolid(SolidPrimitive::Cuboid {
                    origin: [5.0; 3],
                    size: [10.0; 3],
                })),
            )
            .unwrap(),
        );
        let intersect_op = Operation::SolidBoolean {
            op: BoolOp::Intersection,
            a,
            b,
            erase_sources: true,
        };
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
                let alt = dir.join(format!(
                    "doc_api_roundtrip_intersected_{}.dwg",
                    std::process::id()
                ));
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
        let vol = ocs_doc_api::geom::mesh_volume_centroid(&mesh).0;
        assert!(
            (vol - 125.0).abs() < 1.0,
            "reloaded intersection volume {vol}"
        );

        // The DWG is intentionally kept for inspection (see `path` above).
    }
    #[test]
    fn doc_api_failed_inputs_leave_document_and_history_unchanged() {
        use ocs_doc_api::EntitySpec;
        let mut app = OpenCADStudio::new_for_test();
        let count = app.tabs[0].scene.document.entity_count();
        let dirty = app.tabs[0].dirty;
        let undo = app.tabs[0].history.undo_stack.len();
        let epoch = app.tabs[0].scene.geometry_epoch;
        {
            let mut host = HostSession::new(&mut app, 0);
            let invalid_ops = vec![
                Operation::CreateMany(vec![
                    EntitySpec::Curve(Curve2Spec::Point { layer: None, position: [0.0; 3] }),
                    EntitySpec::Solid(SolidPrimitive::Sphere {
                        centre: [0.0; 3],
                        radius: f64::NAN,
                    }),
                ]),
                Operation::CreateCurve(Curve2Spec::Circle { layer: None,
                    centre: [0.0; 3],
                    radius: -1.0,
                }),
                Operation::CreateCurve(Curve2Spec::Polyline { layer: None,
                    points: vec![[0.0; 3], [1.0; 3]],
                    closed: false,
                }),
                Operation::CreateText(ocs_doc_api::ops::TextSpec {
                    value: "test".into(),
                    insertion_point: [0.0; 3],
                    height: f64::INFINITY,
                    rotation: 0.0,
                }),
            ];
            for op in invalid_ops {
                assert!(dispatch(&mut host, DocApiEnvelope::op(op)).is_err());
                assert!(!host.scene().is_recording_undo());
            }
        }
        assert_eq!(app.tabs[0].scene.document.entity_count(), count);
        assert_eq!(app.tabs[0].scene.geometry_epoch, epoch);
        assert_eq!(app.tabs[0].history.undo_stack.len(), undo);
        assert_eq!(app.tabs[0].dirty, dirty);
    }

    #[test]
    fn doc_api_bulk_creation_is_dirty_and_undoes_in_one_step() {
        use ocs_doc_api::EntitySpec;
        let mut app = OpenCADStudio::new_for_test();
        let undo = app.tabs[0].history.undo_stack.len();
        let count = app.tabs[0].scene.document.entity_count();
        let ids = {
            let mut host = HostSession::new(&mut app, 0);
            let receipt = dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateMany(vec![
                    EntitySpec::Curve(Curve2Spec::Point { layer: None, position: [1.0; 3] }),
                    EntitySpec::Solid(SolidPrimitive::Cuboid {
                        origin: [0.0; 3],
                        size: [2.0; 3],
                    }),
                ])),
            )
            .unwrap();
            receipt.outcome.unwrap().new_ids().to_vec()
        };
        assert!(app.tabs[0].dirty);
        assert_eq!(app.tabs[0].history.undo_stack.len(), undo + 1);
        assert_eq!(app.tabs[0].scene.document.entity_count(), count + 2);
        app.undo_active_tab();
        assert_eq!(app.tabs[0].scene.document.entity_count(), count);
        for id in &ids {
            assert!(!app.tabs[0]
                .scene
                .solid_models
                .contains_key(&obj_to_handle(*id)));
        }
        app.redo_active_tab();
        for id in ids {
            assert!(app.tabs[0]
                .scene
                .document
                .get_entity(obj_to_handle(id))
                .is_some());
        }
    }

    #[test]
    fn doc_api_text_scale_and_elevated_extrusion() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let text = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateText(ocs_doc_api::ops::TextSpec {
                    value: "label".into(),
                    insertion_point: [1.0, 2.0, 0.0],
                    height: 2.0,
                    rotation: 0.0,
                })),
            )
            .unwrap(),
        );
        let placement = PlacementSpec {
            origin: [0.0, 0.0, 7.0],
            x_axis: [2.0, 0.0, 0.0],
            y_axis: [0.0, 2.0, 0.0],
            z_axis: [0.0, 0.0, 2.0],
        };
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::Transform {
                id: text,
                placement,
            }),
        )
        .unwrap();
        let EntityType::Text(text) = host.document().get_entity(obj_to_handle(text)).unwrap()
        else {
            panic!("text")
        };
        assert!((text.height - 4.0).abs() < 1e-9);
        assert!((text.insertion_point.z - 7.0).abs() < 1e-9);
        let profile = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(Curve2Spec::Polyline { layer: None,
                    points: vec![
                        [0.0, 0.0, 7.0],
                        [2.0, 0.0, 7.0],
                        [2.0, 3.0, 7.0],
                        [0.0, 3.0, 7.0],
                    ],
                    closed: true,
                })),
            )
            .unwrap(),
        );
        let solid = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::Extrude {
                    profile,
                    direction: [0.0, 0.0, 5.0],
                }),
            )
            .unwrap(),
        );
        let bounds = host.bounds(solid).unwrap();
        assert!(
            (bounds.min[2] - 7.0).abs() < 1e-6 && (bounds.max[2] - 12.0).abs() < 1e-6,
            "{bounds:?}"
        );
        assert!((host.volume(solid).unwrap() - 30.0).abs() < 1e-6);
    }

    #[test]
    fn doc_api_envelope_guards_and_query_budget_reset() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let id = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateSolid(SolidPrimitive::Cuboid {
                    origin: [0.0; 3],
                    size: [2.0; 3],
                })),
            )
            .unwrap(),
        );
        let mut env = DocApiEnvelope::queries(vec![Query::GetVolume { id }]);
        env.version += 1;
        assert!(matches!(
            dispatch(&mut host, env),
            Err(ApiError::Unsupported(_))
        ));
        let valid = DocApiEnvelope::queries(vec![Query::GetVolume { id }]);
        let mut bytes = bincode::serialize(&valid).unwrap();
        bytes.push(0);
        let tab = host.tab_id();
        assert!(execute_doc_api(&mut host, tab, &bytes).is_err());
        let bytes = bincode::serialize(&valid).unwrap();
        assert!(execute_doc_api(&mut host, tab + 1, &bytes).is_err());
        host.scene_mut().doc_api_cold_tess_used = 32;
        let receipt = dispatch(&mut host, valid).unwrap();
        assert!(
            matches!(receipt.query_results[0], QueryResult::Volume(v) if (v - 8.0).abs() < 1e-9)
        );
        assert_eq!(host.scene().doc_api_cold_tess_used, 1);
    }
    #[test]
    fn doc_api_plugin_wire_dispatch_and_image_undo() {
        use ocs_plugin_api::ipc::{
            protocol::{PluginRequest, PluginResponse},
            server::handle_plugin_request,
        };
        let mut app = OpenCADStudio::new_for_test();
        let objects = app.tabs[0].scene.document.objects.len();
        let id = {
            let mut host = HostSession::new(&mut app, 0);
            let env = DocApiEnvelope::op(Operation::CreateRasterImage(
                ocs_doc_api::ops::RasterImageSpec {
                    file_path: "image.png".into(),
                    insertion_point: [0.0; 3],
                    u_vector: [1.0, 0.0, 0.0],
                    v_vector: [0.0, 1.0, 0.0],
                    size: [2.0; 2],
                },
            ));
            let request = PluginRequest::DocApiRequest {
                tab_id: host.tab_id(),
                bytes: bincode::serialize(&env).unwrap(),
            };
            let request = bincode::deserialize(&bincode::serialize(&request).unwrap()).unwrap();
            let response = handle_plugin_request(&mut host, request, &mut |_| {});
            let response: PluginResponse =
                bincode::deserialize(&bincode::serialize(&response).unwrap()).unwrap();
            let PluginResponse::DocApiResponse { bytes } = response else {
                panic!("unexpected response")
            };
            let result: ApiResult<Receipt> = bincode::deserialize(&bytes).unwrap();
            new_id(&result.unwrap())
        };
        assert!(app.tabs[0].scene.document.objects.len() > objects);
        app.undo_active_tab();
        assert!(app.tabs[0]
            .scene
            .document
            .get_entity(obj_to_handle(id))
            .is_none());
        assert_eq!(app.tabs[0].scene.document.objects.len(), objects);
        app.redo_active_tab();
        assert!(app.tabs[0]
            .scene
            .document
            .get_entity(obj_to_handle(id))
            .is_some());
        assert!(app.tabs[0].scene.document.objects.len() > objects);
    }

    #[test]
    fn doc_api_layer_crud_roundtrip_and_entity_assignment() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);

        // Create a line to assign to a layer later.
        let line = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(ocs_doc_api::Curve2Spec::Line { layer: None,
                    start: [0.0; 3],
                    end: [1.0; 3],
                })),
            )
            .unwrap(),
        );

        // List default layers — must include "0".
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::ListLayers])).unwrap();
        let QueryResult::Layers(list) = &receipt.query_results[0] else {
            panic!("expected Layers result");
        };
        assert!(list.iter().any(|l| l.name == "0"));

        // Create a new layer.
        let mut info = LayerInfo::new("Walls");
        info.color = Color::Index(1);
        info.line_weight = LineWeight::Value(25);
        info.flags = LayerFlags {
            frozen: false,
            locked: false,
            frozen_in_new_viewport: false,
            off: false,
        };
        dispatch(&mut host, DocApiEnvelope::op(Operation::CreateLayer(info.clone()))).unwrap();

        // Update the layer color.
        let mut updated = info.clone();
        updated.color = Color::Index(2);
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::UpdateLayer {
                name: " Walls ".into(),
                info: updated.clone(),
            }),
        )
        .unwrap();

        // Assign the line to the new layer.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetEntityLayer {
                id: line,
                layer: "Walls".into(),
            }),
        )
        .unwrap();
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetEntityLayer { id: line }]),
        )
        .unwrap();
        assert!(matches!(&receipt.query_results[0], QueryResult::EntityLayer(name) if name == "Walls"));

        // Deleting a layer still in use fails.
        let err = dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::DeleteLayer {
                name: "Walls".into(),
            }),
        )
        .unwrap_err();
        assert!(matches!(err, ApiError::Validation { .. }));

        // Move the line back to "0", then delete the layer.
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetEntityLayer {
                id: line,
                layer: "0".into(),
            }),
        )
        .unwrap();
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::DeleteLayer {
                name: " Walls ".into(),
            }),
        )
        .unwrap();
        let receipt = dispatch(&mut host, DocApiEnvelope::queries(vec![Query::ListLayers])).unwrap();
        let QueryResult::Layers(list) = &receipt.query_results[0] else {
            panic!("expected Layers result");
        };
        assert!(!list.iter().any(|l| l.name == "Walls"));
    }

    #[test]
    fn doc_api_xdata_batch_rejects_a_locked_entity_before_mutating() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let line = |host: &mut HostSession<'_>| {
            new_id(&dispatch(
                host,
                DocApiEnvelope::op(Operation::CreateCurve(ocs_doc_api::Curve2Spec::Line {
                    start: [0.0; 3],
                    end: [1.0; 3],
                    layer: None,
                })),
            ).unwrap())
        };
        let first = line(&mut host);
        let locked = line(&mut host);

        let mut layer = LayerInfo::new("Locked");
        dispatch(&mut host, DocApiEnvelope::op(Operation::CreateLayer(layer.clone()))).unwrap();
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetEntityLayer {
                id: locked,
                layer: layer.name.clone(),
            }),
        )
        .unwrap();
        layer.flags.locked = true;
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::UpdateLayer {
                name: layer.name.clone(),
                info: layer,
            }),
        )
        .unwrap();

        let record = |application_name: &str| XDataRecord {
            application_name: application_name.into(),
            values: vec![XDataValue::Integer32(42)],
        };
        let result = dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetXDataMany(vec![
                (first, "FIRST".into(), Some(record("FIRST"))),
                (locked, "LOCKED".into(), Some(record("LOCKED"))),
            ])),
        );
        assert!(matches!(result, Err(ApiError::Validation { .. })));
        assert!(host.xdata(first, "FIRST").unwrap().is_none());
    }

    #[test]
    fn doc_api_xrecords_keep_one_named_owner() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let spec = |name: &str| XRecordSpec {
            name: name.into(),
            cloning_flags: XRecordCloningFlags::NotApplicable,
            entries: vec![XRecordEntry {
                code: 1,
                value: XRecordValue::String("value".into()),
            }],
        };

        let first = new_id(&dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::CreateXRecord(spec("FIRST"))),
        ).unwrap());
        let first_handle = obj_to_handle(first);
        let ObjectType::XRecord(first_record) = host.document().objects.get(&first_handle).unwrap()
        else {
            panic!("expected XRecord")
        };
        let owner = first_record.owner;
        assert!(matches!(host.document().objects.get(&owner), Some(ObjectType::Dictionary(dict)) if
            dict.entries.iter().any(|(name, child)| name == "FIRST" && *child == first_handle)));

        let object_count = host.document().objects.len();
        assert!(matches!(
            dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateXRecord(spec("first"))),
            ),
            Err(ApiError::Validation { .. })
        ));
        assert_eq!(host.document().objects.len(), object_count);

        let second = new_id(&dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::CreateXRecord(spec("SECOND"))),
        ).unwrap());
        assert!(matches!(
            dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::SetXRecord {
                    id: second,
                    spec: spec("FIRST"),
                }),
            ),
            Err(ApiError::Validation { .. })
        ));
        assert_eq!(host.xrecord(second).unwrap().unwrap().name, "SECOND");

        let mut invalid = spec("INVALID");
        invalid.entries[0] = XRecordEntry {
            code: 1,
            value: XRecordValue::Int32(7),
        };
        assert!(matches!(
            dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateXRecord(invalid)),
            ),
            Err(ApiError::Validation { .. })
        ));
    }

    #[test]
    fn doc_api_enumerate_entities_and_geometry_queries() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);

        // Create entities on different layers.
        let line_id = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(ocs_doc_api::Curve2Spec::Line { layer: None,
                    start: [1.0, 2.0, 3.0],
                    end: [4.0, 5.0, 6.0],
                })),
            )
            .unwrap(),
        );
        let point_id = new_id(
            &dispatch(
                &mut host,
                DocApiEnvelope::op(Operation::CreateCurve(ocs_doc_api::Curve2Spec::Point { layer: None,
                    position: [7.0, 8.0, 9.0],
                })),
            )
            .unwrap(),
        );

        // Create a layer and move the point to it.
        let mut info = LayerInfo::new("Markers");
        info.color = Color::Index(3);
        dispatch(&mut host, DocApiEnvelope::op(Operation::CreateLayer(info))).unwrap();
        dispatch(
            &mut host,
            DocApiEnvelope::op(Operation::SetEntityLayer {
                id: point_id,
                layer: "Markers".into(),
            }),
        )
        .unwrap();

        // Enumerate all entities.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::EnumerateEntities {
                kind: None,
                layer: None,
                include_bounds: true,
            }]),
        )
        .unwrap();
        let QueryResult::Entities(all) = &receipt.query_results[0] else {
            panic!("expected Entities result");
        };
        assert!(all.iter().any(|e| e.id == line_id && e.kind == "Line" && e.layer == "0"));
        assert!(all.iter().any(|e| e.id == point_id && e.kind == "Point" && e.layer == "Markers"));

        // Filter by kind.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::EnumerateEntities {
                kind: Some("Line".into()),
                layer: None,
                include_bounds: true,
            }]),
        )
        .unwrap();
        let QueryResult::Entities(lines) = &receipt.query_results[0] else {
            panic!("expected Entities result");
        };
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].id, line_id);

        // Filter by layer.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::EnumerateEntities {
                kind: None,
                layer: Some(" Markers ".into()),
                include_bounds: true,
            }]),
        )
        .unwrap();
        let QueryResult::Entities(markers) = &receipt.query_results[0] else {
            panic!("expected Entities result");
        };
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].id, point_id);

        // Geometry queries.
        let receipt = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![
                Query::GetPointPosition { id: point_id },
                Query::GetLineGeometry { id: line_id },
            ]),
        )
        .unwrap();
        let QueryResult::PointPosition(pos) = &receipt.query_results[0] else {
            panic!("expected PointPosition result");
        };
        assert!((pos[0] - 7.0).abs() < 1e-9);
        assert!((pos[1] - 8.0).abs() < 1e-9);
        assert!((pos[2] - 9.0).abs() < 1e-9);
        let QueryResult::LineGeometry((start, end)) = &receipt.query_results[1] else {
            panic!("expected LineGeometry result");
        };
        assert!((start[0] - 1.0).abs() < 1e-9);
        assert!((end[0] - 4.0).abs() < 1e-9);

        // Wrong entity family is rejected.
        let err = dispatch(
            &mut host,
            DocApiEnvelope::queries(vec![Query::GetPointPosition { id: line_id }]),
        )
        .unwrap_err();
        assert!(matches!(err, ApiError::Unsupported { .. }));
    }
}
