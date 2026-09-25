//! Entity-level automation operations — create, erase, transform, XData and
//! block-definition operations on the drawing database. All operations are atomic (one undo step), validate
//! every input before touching the document, and address entities by the
//! hexadecimal handles that `query`/`records` return.

use super::*;
use crate::command::EntityTransform;

type Parsed<T> = Result<T, Value>;

/// `[x,y]` or `[x,y,z]` (missing z = 0) from `req[key]`.
fn point_field(req: &Value, key: &str) -> Parsed<codec::types::Vector3> {
    let values = req[key]
        .as_array()
        .filter(|v| (2..=3).contains(&v.len()))
        .ok_or_else(|| failure("invalid_point", format!("{key} must be [x,y] or [x,y,z]")))?;
    let coord = |i: usize| {
        values[i]
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| failure("invalid_point", format!("{key} needs finite coordinates")))
    };
    Ok(codec::types::Vector3::new(coord(0)?, coord(1)?, values.get(2).and_then(Value::as_f64).unwrap_or(0.0)))
}

fn deg_field(req: &Value, key: &str) -> f64 {
    req[key].as_f64().unwrap_or(0.0).to_radians()
}

pub(super) fn hex_handles(req: &Value, key: &str) -> Parsed<Vec<codec::Handle>> {
    let list = req[key]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| failure("handles_required", format!("Supply a non-empty {key}:[…] list")))?;
    list.iter()
        .map(|v| {
            u64::from_str_radix(
                v.as_str()
                    .unwrap_or("")
                    .trim_start_matches("0x")
                    .trim_start_matches("0X"),
                16,
            )
            .map(codec::Handle::new)
            .map_err(|_| {
                failure(
                    "invalid_handle",
                    format!("Expected a hexadecimal handle, got {v}"),
                )
            })
        })
        .collect()
}

/// Every handle must exist in the document; otherwise `entity_absent` naming
/// the missing ones so the caller can refresh instead of guessing.
pub(super) fn require_existing(document: &codec::CadDocument, handles: &[codec::Handle]) -> Parsed<()> {
    let missing: Vec<String> = handles
        .iter()
        .filter(|h| document.get_entity(**h).is_none())
        .map(|h| format!("{:X}", h.value()))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(failure(
            "entity_absent",
            format!("Handles not found: {}", missing.join(", ")),
        ))
    }
}

fn apply_common_properties(spec: &Value, entity: &mut codec::EntityType) -> Parsed<()> {
    if let Some(layer) = spec["layer"].as_str() {
        if !layer.is_empty()
            && !matches!(
                entity,
                codec::EntityType::Block(_) | codec::EntityType::BlockEnd(_)
            )
        {
            entity.common_mut().layer = layer.to_owned();
        }
    }
    if let Some(index) = spec["color"].as_i64().and_then(|v| i16::try_from(v).ok()) {
        entity.common_mut().color = codec::types::Color::from_index(index);
    }
    Ok(())
}

/// One `entities` array entry → a real entity. Geometry validation happens
/// here so a bad definition aborts the whole batch before anything commits.
fn build_entity(spec: &Value) -> Parsed<codec::EntityType> {
    use codec::entities::*;
    use codec::types::Vector3;
    let kind = spec["type"].as_str().unwrap_or("").to_ascii_uppercase();
    let entity = match kind.as_str() {
        "LINE" => {
            let mut line = Line::from_points(point_field(spec, "start")?, point_field(spec, "end")?);
            if let Some(t) = spec["thickness"].as_f64() {
                line.thickness = t;
            }
            EntityType::Line(line)
        }
        "CIRCLE" => {
            let radius = spec["radius"]
                .as_f64()
                .filter(|r| r.is_finite() && *r >= 0.0)
                .ok_or_else(|| failure("invalid_radius", "circle needs a finite radius >= 0"))?;
            EntityType::Circle(Circle::from_center_radius(point_field(spec, "center")?, radius))
        }
        "ARC" => {
            let radius = spec["radius"]
                .as_f64()
                .filter(|r| r.is_finite() && *r > 0.0)
                .ok_or_else(|| failure("invalid_radius", "arc needs a finite radius > 0"))?;
            EntityType::Arc(Arc::from_center_radius_angles(
                point_field(spec, "center")?,
                radius,
                deg_field(spec, "start_angle_deg"),
                deg_field(spec, "end_angle_deg"),
            ))
        }
        "LWPOLYLINE" | "POLYLINE" => {
            let vertices = spec["vertices"]
                .as_array()
                .filter(|v| v.len() >= 2)
                .ok_or_else(|| failure("invalid_vertices", "polyline needs vertices:[[x,y],…]"))?;
            let mut pline = LwPolyline::new();
            for point in vertices {
                let x = point[0].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                    failure("invalid_vertices", "polyline vertices must be finite [x,y]")
                })?;
                let y = point[1].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                    failure("invalid_vertices", "polyline vertices must be finite [x,y]")
                })?;
                pline.add_point(codec::types::Vector2::new(x, y));
            }
            if spec["closed"].as_bool().unwrap_or(false) {
                pline.close();
            }
            if let Some(width) = spec["constant_width"].as_f64() {
                pline.constant_width = width;
            }
            EntityType::LwPolyline(pline)
        }
        "POINT" => EntityType::Point(Point::at(point_field(spec, "location")?)),
        "TEXT" => {
            let value = spec["value"].as_str().unwrap_or("");
            if value.is_empty() {
                return Err(failure("invalid_text", "text needs a non-empty value"));
            }
            let mut text = Text::with_value(value, point_field(spec, "position")?)
                .with_height(spec["height"].as_f64().unwrap_or(2.5).abs().max(1e-6))
                .with_rotation(deg_field(spec, "rotation_deg"));
            if let Some(style) = spec["style"].as_str().filter(|s| !s.is_empty()) {
                text.style = style.to_owned();
            }
            EntityType::Text(text)
        }
        "MTEXT" => {
            let value = spec["value"].as_str().unwrap_or("");
            if value.is_empty() {
                return Err(failure("invalid_text", "mtext needs a non-empty value"));
            }
            let mut mtext = MText::with_value(value, point_field(spec, "position")?)
                .with_height(spec["height"].as_f64().unwrap_or(2.5).abs().max(1e-6));
            if let Some(width) = spec["width"].as_f64().filter(|w| *w > 0.0) {
                mtext.rectangle_width = width;
            }
            mtext.rotation = deg_field(spec, "rotation_deg");
            EntityType::MText(mtext)
        }
        "INSERT" => {
            let block = spec["block"].as_str().unwrap_or("");
            if block.is_empty() {
                return Err(failure("block_required", "insert needs a block name"));
            }
            let mut insert = Insert::new(block, point_field(spec, "position")?)
                .with_rotation(deg_field(spec, "rotation_deg"));
            match &spec["scale"] {
                Value::Number(factor) => {
                    let factor = factor.as_f64().filter(|f| *f != 0.0).ok_or_else(|| {
                        failure("invalid_scale", "insert scale must be nonzero")
                    })?;
                    insert = insert.with_uniform_scale(factor);
                }
                Value::Array(factors) if factors.len() == 3 => {
                    let axis = |i: usize| {
                        factors[i].as_f64().filter(|f| *f != 0.0).ok_or_else(|| {
                            failure("invalid_scale", "insert scale components must be nonzero")
                        })
                    };
                    insert = insert.with_scale(axis(0)?, axis(1)?, axis(2)?);
                }
                _ => {}
            }
            EntityType::Insert(insert)
        }
        "SOLID" => {
            let corners = spec["corners"]
                .as_array()
                .filter(|v| (3..=4).contains(&v.len()))
                .ok_or_else(|| failure("invalid_corners", "solid needs 3 or 4 corners"))?;
            let corner = |i: usize| -> Parsed<Vector3> {
                let raw = &corners[i];
                let coord = |k: usize| {
                    raw.get(k)
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.0)
                };
                Ok(Vector3::new(coord(0), coord(1), coord(2)))
            };
            let fourth = corners
                .get(3)
                .map(|_| corner(3))
                .unwrap_or_else(|| Ok(Vector3::ZERO));
            EntityType::Solid(Solid::new(corner(0)?, corner(1)?, corner(2)?, fourth?))
        }
        "HATCH" => {
            let boundary = spec["boundary"]
                .as_array()
                .filter(|v| v.len() >= 3)
                .ok_or_else(|| failure("invalid_boundary", "hatch needs boundary:[[x,y],…] (>= 3 points)"))?;
            let mut vertices = Vec::with_capacity(boundary.len());
            for point in boundary {
                let x = point[0].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                    failure("invalid_boundary", "hatch boundary must be finite [x,y]")
                })?;
                let y = point[1].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                    failure("invalid_boundary", "hatch boundary must be finite [x,y]")
                })?;
                vertices.push(codec::types::Vector2::new(x, y));
            }
            let mut hatch = Hatch::new();
            if spec["solid"].as_bool().unwrap_or(false) {
                hatch.is_solid = true;
            } else {
                hatch.is_solid = false;
                hatch.pattern = HatchPattern::new(
                    spec["pattern"].as_str().unwrap_or("ANSI31"),
                );
                hatch.pattern_scale = spec["pattern_scale"].as_f64().unwrap_or(1.0).abs().max(1e-6);
                hatch.pattern_angle = deg_field(spec, "pattern_angle_deg");
            }
            let mut path = BoundaryPath::external();
            path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(vertices, true)));
            hatch.paths.push(path);
            EntityType::Hatch(hatch)
        }
        other => {
            return Err(failure(
                "unknown_entity_type",
                format!(
                    "Unknown entity type '{other}'. Supported: Line, Circle, Arc, LwPolyline, Point, Text, MText, Insert, Solid, Hatch"
                ),
            ))
        }
    };
    let mut entity = entity;
    apply_common_properties(spec, &mut entity)?;
    Ok(entity)
}

impl OpenCADStudio {
    /// `entities_create` — build a batch of typed entities, auto-create the
    /// layers they name, and commit everything as one undoable step. All definitions are validated before the first
    /// entity is added, so the batch is all-or-nothing.
    pub(super) fn control_entities_create(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let list = req["entities"]
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| failure("entities_required", "Supply entities:[{type:…},…]"))?;
        let built: Vec<codec::EntityType> = list.iter().map(build_entity).collect::<Parsed<_>>()?;

        let i = self.active_tab;
        self.push_undo_snapshot(i, "ENTITIESCREATE");
        // Auto-create missing layers (Catalog §1.4: "create layer if missing")
        // after the undo snapshot, so undo removes them together with the
        // entities.
        let mut layers_created = Vec::new();
        {
            let document = &mut self.tabs[i].scene.document;
            for entity in &built {
                let layer = entity.common().layer.clone();
                if layer.is_empty() || document.layers.contains(&layer) {
                    continue;
                }
                let mut layer_record = codec::tables::Layer::new(layer.clone());
                layer_record.handle = document.allocate_handle();
                document
                    .layers
                    .add(layer_record)
                    .map_err(|e| failure("layer_failed", e))?;
                layers_created.push(layer);
            }
        }
        let handles: Vec<String> = {
            let scene = &mut self.tabs[i].scene;
            built
                .into_iter()
                .map(|entity| format!("{:X}", scene.add_entity(entity).value()))
                .collect()
        };
        self.post_ref_op(i);
        let created = handles.len();
        self.set_control_result(json!({
            "handles": handles,
            "created": created,
            "layers_created": layers_created,
        }));
        Ok(Task::none())
    }

    /// `entities_delete` — erase by handles. Every handle is
    /// checked up front; nothing is erased when one is missing.
    pub(super) fn control_entities_delete(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let handles = hex_handles(req, "handles")?;
        let i = self.active_tab;
        require_existing(&self.tabs[i].scene.document, &handles)?;
        self.push_undo_snapshot(i, "ENTITIESDELETE");
        self.tabs[i].scene.erase_entities(&handles);
        self.post_ref_op(i);
        let erased = handles.len();
        self.set_control_result(json!({ "erased": erased }));
        Ok(Task::none())
    }

    /// `entities_transform` — move/copy/rotate/scale/mirror/array by handles
    /// Angles are degrees. `copy`, mirror with
    /// `"copy":true` and `array` return the newly created handles.
    pub(super) fn control_entities_transform(
        &mut self,
        req: &Value,
    ) -> Result<Task<Message>, Value> {
        let handles = hex_handles(req, "handles")?;
        let action = req["action"].as_str().unwrap_or("move").to_ascii_lowercase();
        let i = self.active_tab;
        require_existing(&self.tabs[i].scene.document, &handles)?;

        let transform = |req: &Value| -> Parsed<EntityTransform> {
            Ok(match action.as_str() {
                "move" | "copy" => EntityTransform::Translate(glam::DVec3::new(
                    req["vector"][0].as_f64().unwrap_or(0.0),
                    req["vector"][1].as_f64().unwrap_or(0.0),
                    req["vector"][2].as_f64().unwrap_or(0.0),
                )),
                "rotate" => EntityTransform::Rotate {
                    center: glam::DVec3::new(
                        req["center"][0].as_f64().unwrap_or(0.0),
                        req["center"][1].as_f64().unwrap_or(0.0),
                        req["center"][2].as_f64().unwrap_or(0.0),
                    ),
                    axis: glam::DVec3::Z,
                    angle_rad: deg_field(req, "angle_deg"),
                },
                "scale" => {
                    let factor = req["factor"]
                        .as_f64()
                        .filter(|f| f.is_finite() && *f != 0.0)
                        .ok_or_else(|| {
                            failure("invalid_factor", "scale needs a nonzero finite factor")
                        })?;
                    EntityTransform::Scale {
                        center: glam::DVec3::new(
                            req["center"][0].as_f64().unwrap_or(0.0),
                            req["center"][1].as_f64().unwrap_or(0.0),
                            req["center"][2].as_f64().unwrap_or(0.0),
                        ),
                        factor,
                    }
                }
                "mirror" => {
                    let axis = req["axis"]
                        .as_array()
                        .filter(|v| v.len() == 2)
                        .ok_or_else(|| {
                            failure("invalid_axis", "mirror needs axis:[[x1,y1],[x2,y2]]")
                        })?;
                    let end = |k: usize| {
                        axis[k]
                            .as_array()
                            .filter(|v| v.len() >= 2)
                            .ok_or_else(|| {
                                failure("invalid_axis", "mirror needs axis:[[x1,y1],[x2,y2]]")
                            })
                    };
                    let (a, b) = (end(0)?, end(1)?);
                    let coord = |v: &[Value], k: usize| {
                        v[k].as_f64()
                            .filter(|f| f.is_finite())
                            .ok_or_else(|| failure("invalid_axis", "mirror axis points must be finite"))
                    };
                    let p1 = glam::DVec3::new(coord(&a, 0)?, coord(&a, 1)?, 0.0);
                    let p2 = glam::DVec3::new(coord(&b, 0)?, coord(&b, 1)?, 0.0);
                    if p1 == p2 {
                        return Err(failure(
                            "invalid_axis",
                            "mirror axis points must differ",
                        ));
                    }
                    EntityTransform::Mirror {
                        p1,
                        p2,
                        working_normal: glam::DVec3::Z,
                    }
                }
                other => {
                    return Err(failure(
                        "invalid_action",
                        format!(
                            "Unknown transform action '{other}'. Use move, copy, rotate, scale, mirror or array"
                        ),
                    ))
                }
            })
        };

        self.push_undo_snapshot(i, "ENTITIESTRANSFORM");
        let scene = &mut self.tabs[i].scene;
        let mut created: Vec<String> = Vec::new();
        let affected;
        match action.as_str() {
            "move" | "rotate" | "scale" => {
                let t = transform(req)?;
                scene.transform_entities(&handles, &t);
                affected = handles.len();
            }
            "mirror" => {
                let t = transform(req)?;
                if req["copy"].as_bool().unwrap_or(false) {
                    created = scene
                        .copy_entities(&handles, &t)
                        .into_iter()
                        .map(|h| format!("{:X}", h.value()))
                        .collect();
                } else {
                    scene.transform_entities(&handles, &t);
                }
                affected = handles.len();
            }
            "copy" => {
                let t = transform(req)?;
                created = scene
                    .copy_entities(&handles, &t)
                    .into_iter()
                    .map(|h| format!("{:X}", h.value()))
                    .collect();
                affected = handles.len();
            }
            "array" => {
                let rows = req["rows"].as_u64().unwrap_or(1).clamp(1, 100) as i32;
                let columns = req["columns"].as_u64().unwrap_or(1).clamp(1, 100) as i32;
                let row_spacing = req["row_spacing"].as_f64().unwrap_or(0.0);
                let column_spacing = req["column_spacing"].as_f64().unwrap_or(0.0);
                for row in 0..rows {
                    for column in 0..columns {
                        if row == 0 && column == 0 {
                            continue;
                        }
                        let t = EntityTransform::Translate(glam::DVec3::new(
                            column as f64 * column_spacing,
                            row as f64 * row_spacing,
                            0.0,
                        ));
                        created.extend(
                            scene
                                .copy_entities(&handles, &t)
                                .into_iter()
                                .map(|h| format!("{:X}", h.value())),
                        );
                    }
                }
                affected = handles.len();
            }
            _ => unreachable!("validated by transform()"),
        }
        self.post_ref_op(i);
        self.set_control_result(json!({
            "affected": affected,
            "created": created,
        }));
        Ok(Task::none())
    }

    /// `xdata_set` — replace one application's extended-data record on every
    /// listed handle. The APPID table entry is
    /// registered implicitly; an empty (or absent) `data` list removes the
    /// application's record.
    pub(super) fn control_xdata_set(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let handles = hex_handles(req, "handles")?;
        let app = string(req, "app")?.to_owned();
        if app.is_empty() {
            return Err(failure("app_required", "Supply an application name"));
        }
        let values = parse_xdata_values(req)?;
        let i = self.active_tab;
        require_existing(&self.tabs[i].scene.document, &handles)?;
        self.push_undo_snapshot(i, "XDATASET");
        {
            let document = &mut self.tabs[i].scene.document;
            for handle in &handles {
                crate::scene::view::dispatch::set_entity_xdata(
                    document,
                    *handle,
                    &app,
                    values.clone(),
                );
            }
        }
        let updated = handles.len();
        self.set_control_result(json!({ "updated": updated, "app": app }));
        Ok(Task::none())
    }

    /// `xdata_get` — read extended data back, optionally for one application.
    /// A read: dispatched through the query path, no request_id needed.
    pub(super) fn xdata_read(&self, req: &Value) -> Value {
        let handles = match hex_handles(req, "handles") {
            Ok(handles) => handles,
            Err(error) => return error,
        };
        let app = req["app"].as_str().unwrap_or("");
        let i = self.active_tab;
        if let Err(error) = require_existing(&self.tabs[i].scene.document, &handles) {
            return error;
        }
        let document = &self.tabs[i].scene.document;
        let items: Vec<Value> = handles
            .iter()
            .filter_map(|handle| {
                let entity = document.get_entity(*handle)?;
                let extended = &entity.common().extended_data;
                let records: serde_json::Map<String, Value> = extended
                    .records()
                    .iter()
                    .filter(|record| app.is_empty() || record.application_name == app)
                    .map(|record| {
                        (
                            record.application_name.clone(),
                            json!(record.values.iter().map(xdata_value_json).collect::<Vec<_>>()),
                        )
                    })
                    .collect();
                Some(json!({
                    "handle": format!("{:X}", handle.value()),
                    "xdata": records,
                }))
            })
            .collect();
        json!({ "ok": true, "items": items })
    }

    /// `block_define` — turn the listed entities into a new block definition
    /// (block-definition semantics, as the BLOCK command: the sources become
    /// the definition's content and an Insert is placed at `insert_at`,
    /// defaulting to `base`, so the drawing looks unchanged).
    pub(super) fn control_block_define(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let name = string(req, "name")?.to_owned();
        if name.is_empty() {
            return Err(failure("name_required", "Supply a block name"));
        }
        let handles = hex_handles(req, "handles")?;
        let base = point_field(req, "base")?;
        let insert_at = if req.get("insert_at").is_some() {
            point_field(req, "insert_at")?
        } else {
            base
        };
        let i = self.active_tab;
        require_existing(&self.tabs[i].scene.document, &handles)?;

        // World → block space translates `base` onto the block origin; the
        // inverse places the Insert so entities keep their world position.
        let world_to_block = codec::types::Transform::from_translation(
            codec::types::Vector3::new(-base.x, -base.y, -base.z),
        );
        let block_to_world =
            codec::types::Transform::from_translation(insert_at);
        self.push_undo_snapshot(i, "BLOCKDEFINE");
        // CF-01.3 (barcode re-import): `replace:true` drops the previous
        // definition — its children, markers and inserts — inside the same
        // undo step, so the new definition can take the name.
        if req["replace"].as_bool().unwrap_or(false) {
            erase_block_definition(&mut self.tabs[i].scene, &name);
        }
        let insert = self.tabs[i]
            .scene
            .create_block_from_entities(&handles, &name, &world_to_block, &block_to_world)
            .map_err(|e| failure("block_failed", e))?;
        self.post_ref_op(i);
        self.set_control_result(json!({
            "block": name,
            "insert": format!("{:X}", insert.value()),
        }));
        Ok(Task::none())
    }

    /// `block_delete` — remove a block definition together with its child
    /// entities, the Block/BlockEnd markers and every Insert referencing it.
    pub(super) fn control_block_delete(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let name = string(req, "name")?.to_owned();
        if name.starts_with('*') {
            return Err(failure(
                "block_protected",
                "Layout block records cannot be deleted",
            ));
        }
        let i = self.active_tab;
        if self.tabs[i].scene.document.block_records.get(&name).is_none() {
            return Err(failure(
                "block_missing",
                format!("Block '{name}' does not exist"),
            ));
        }
        self.push_undo_snapshot(i, "BLOCKDELETE");
        let erased = erase_block_definition(&mut self.tabs[i].scene, &name);
        self.post_ref_op(i);
        self.set_control_result(json!({ "block": name, "erased": erased }));
        Ok(Task::none())
    }

    /// `view_focus` — fit the listed entities into the view and highlight
    /// them (Catalog §1.3). GUI sessions only; headless servers answer
    /// `gui_required`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn control_view_focus(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        if self.main_window.is_none() {
            return Err(failure(
                "gui_required",
                "view_focus needs the editor window",
            ));
        }
        let handles = hex_handles(req, "handles")?;
        let i = self.active_tab;
        require_existing(&self.tabs[i].scene.document, &handles)?;
        if req["highlight"].as_bool().unwrap_or(true) {
            let scene = &mut self.tabs[i].scene;
            scene.deselect_all();
            for handle in &handles {
                scene.select_entity(*handle, false);
            }
        }
        let zoomed = self.tabs[i].scene.zoom_to_entities(&handles);
        let selected = self.tabs[i].scene.selected_entities().len();
        self.set_control_result(json!({ "zoomed": zoomed, "selected": selected }));
        Ok(Task::none())
    }
}

/// Remove one block definition from the scene: every entity owned by the
/// block record, every Insert referencing the name, the Block/BlockEnd
/// markers (which the entity iterator skips) and the table record itself.
/// Silent no-op for an unknown name (callers validate first);
/// `*`-prefixed layout records are rejected by the callers. Returns the
/// number of entities erased.
fn erase_block_definition(scene: &mut crate::scene::Scene, name: &str) -> usize {
    let Some(record) = scene.document.block_records.get(name) else {
        return 0;
    };
    let owner = record.handle;
    let block_markers = [record.block_entity_handle, record.block_end_handle];
    let mut victims: Vec<codec::Handle> = scene
        .document
        .entities()
        .filter(|entity| {
            entity.common().owner_handle == owner
                || matches!(
                    entity,
                    codec::EntityType::Insert(insert)
                        if insert.block_name.eq_ignore_ascii_case(name)
                )
        })
        .map(|entity| entity.common().handle)
        .collect();
    victims.extend_from_slice(&block_markers);
    scene.erase_entities(&victims);
    scene.document.block_records.remove(name);
    victims.len()
}

/// `data:[{code,value},…]` → typed XData values; an absent or empty list
/// means "remove this application's record".
fn parse_xdata_values(req: &Value) -> Parsed<Option<Vec<codec::xdata::XDataValue>>> {
    use codec::xdata::XDataValue;
    let Some(list) = req["data"].as_array() else {
        return Ok(None);
    };
    if list.is_empty() {
        return Ok(None);
    }
    let mut values = Vec::with_capacity(list.len());
    for item in list {
        let code = item["code"]
            .as_i64()
            .ok_or_else(|| failure("invalid_xdata", "data entries need an integer code"))?;
        let value = &item["value"];
        let parsed = match code {
            1000 => XDataValue::String(
                value.as_str().ok_or_else(|| {
                    failure("invalid_xdata", "code 1000 needs a string value")
                })?
                    .to_string(),
            ),
            1003 => XDataValue::LayerName(
                value.as_str().ok_or_else(|| {
                    failure("invalid_xdata", "code 1003 needs a layer name")
                })?
                    .to_string(),
            ),
            1004 => {
                let hex = value.as_str().ok_or_else(|| {
                    failure("invalid_xdata", "code 1004 needs a hex byte string")
                })?;
                ((0..hex.len()).step_by(2))
                    .map(|k| {
                        u8::from_str_radix(&hex[k..(k + 2).min(hex.len())], 16).map_err(|_| {
                            failure("invalid_xdata", "code 1004 needs a valid hex byte string")
                        })
                    })
                    .collect::<Result<Vec<u8>, Value>>()
                    .map(XDataValue::BinaryData)?
            }
            1005 => {
                let raw = value.as_str().ok_or_else(|| {
                    failure("invalid_xdata", "code 1005 needs a hex handle")
                })?;
                let raw = raw.trim_start_matches("0x").trim_start_matches("0X");
                let handle = u64::from_str_radix(raw, 16)
                    .map_err(|_| failure("invalid_xdata", "code 1005 needs a valid hex handle"))?;
                XDataValue::Handle(codec::Handle::new(handle))
            }
            1010..=1013 => {
                let point = value.as_array().filter(|v| !v.is_empty()).ok_or_else(|| {
                    failure("invalid_xdata", "codes 1010-1013 need [x,y,z]")
                })?;
                let coord = |k: usize| point.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                let point = codec::types::Vector3::new(coord(0), coord(1), coord(2));
                match code {
                    1010 => XDataValue::Point3D(point),
                    1011 => XDataValue::Position3D(point),
                    1012 => XDataValue::Displacement3D(point),
                    _ => XDataValue::Direction3D(point),
                }
            }
            1040 => XDataValue::Real(value.as_f64().unwrap_or(0.0)),
            1041 => XDataValue::Distance(value.as_f64().unwrap_or(0.0)),
            1042 => XDataValue::ScaleFactor(value.as_f64().unwrap_or(0.0)),
            1070 => XDataValue::Integer16(
                value
                    .as_i64()
                    .and_then(|v| i16::try_from(v).ok())
                    .ok_or_else(|| failure("invalid_xdata", "code 1070 needs a 16-bit integer"))?,
            ),
            1071 => XDataValue::Integer32(
                value
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| failure("invalid_xdata", "code 1071 needs a 32-bit integer"))?,
            ),
            other => {
                return Err(failure(
                    "invalid_xdata",
                    format!(
                        "Unsupported xdata code {other}. Use 1000, 1003, 1004, 1005, 1010-1013, 1040, 1041, 1042, 1070 or 1071"
                    ),
                ))
            }
        };
        values.push(parsed);
    }
    Ok(Some(values))
}

/// JSON shape for one stored XData value.
fn xdata_value_json(value: &codec::xdata::XDataValue) -> Value {
    use codec::xdata::XDataValue;
    match value {
        XDataValue::String(text)
        | XDataValue::ControlString(text)
        | XDataValue::LayerName(text) => json!(text),
        XDataValue::BinaryData(bytes) => json!({
            "binary": bytes.iter().map(|b| format!("{b:02X}")).collect::<String>()
        }),
        XDataValue::Handle(handle) => json!(format!("{:X}", handle.value())),
        XDataValue::Point3D(point)
        | XDataValue::Position3D(point)
        | XDataValue::Displacement3D(point)
        | XDataValue::Direction3D(point) => json!([point.x, point.y, point.z]),
        XDataValue::Real(number)
        | XDataValue::Distance(number)
        | XDataValue::ScaleFactor(number) => json!(number),
        XDataValue::Integer16(number) => json!(number),
        XDataValue::Integer32(number) => json!(number),
    }
}
