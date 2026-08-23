// Dispatch entry points for entity editing.

use acadrust::types::{Color as AcadColor, LineWeight, Transparency};
use acadrust::{EntityType, Handle};

use crate::command::EntityTransform;
use crate::entities::traits::EntityTypeOps;
use crate::scene::model::object::{GripDef, PropSection};
use crate::scene::cache::properties;

thread_local! {
    /// Which vertex a multi-vertex entity's Properties panel is focused on
    /// (Current Vertex stepper). Set by the app from its `prop_vertex` state
    /// before building or editing a polyline's properties; read by the
    /// polyline `properties` / `apply_geom_prop` so the X/Y and per-vertex
    /// width rows target that vertex. A thread-local keeps the per-entity trait
    /// signatures unchanged (mirrors the curve-tolerance override).
    static PROP_CURRENT_VERTEX: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Focus the Properties panel on vertex `i` for the next properties build / edit.
pub fn set_prop_current_vertex(i: usize) {
    PROP_CURRENT_VERTEX.with(|c| c.set(i));
}

/// The vertex the Properties panel is focused on.
pub fn prop_current_vertex() -> usize {
    PROP_CURRENT_VERTEX.with(|c| c.get())
}

/// Extrusion normal of a planar (OCS-stored) entity when it isn't the
/// default +Z. `None` = the entity stores WCS coordinates (LINE, POINT,
/// ELLIPSE, MTEXT, ...) or maps its own OCS internally (INSERT) — grips pass
/// through untouched. Per-entity grip code works in raw field space, so the
/// dispatch layer converts: display grips OCS→WCS, incoming edits WCS→OCS.
fn planar_ocs_normal(entity: &EntityType) -> Option<(f64, f64, f64)> {
    let n = match entity {
        EntityType::Arc(e) => e.normal,
        EntityType::Circle(e) => e.normal,
        EntityType::LwPolyline(e) => e.normal,
        EntityType::Polyline2D(e) => e.normal,
        EntityType::Solid(e) => e.normal,
        EntityType::Text(e) => e.normal,
        EntityType::AttributeDefinition(e) => e.normal,
        EntityType::Hatch(e) => e.normal,
        EntityType::Shape(e) => e.normal,
        _ => return None,
    };
    if n.x == 0.0 && n.y == 0.0 && n.z == 1.0 {
        None
    } else {
        Some((n.x, n.y, n.z))
    }
}

pub fn grips(entity: &EntityType) -> Vec<GripDef> {
    let mut grips = EntityTypeOps::grips(entity);
    if let Some(n) = planar_ocs_normal(entity) {
        for g in &mut grips {
            let (x, y, z) =
                super::transform::ocs_point_to_wcs((g.world.x, g.world.y, g.world.z), n);
            g.world = glam::DVec3::new(x, y, z);
        }
    }
    grips
}

pub fn properties_sectioned(
    handle: Handle,
    entity: &EntityType,
    text_style_names: &[String],
) -> Vec<PropSection> {
    let mut sections = vec![properties::general_section(entity)];
    if let Some(viz) = properties::visualization_section(entity) {
        sections.push(viz);
    }
    let groups = entity.geometry_properties(text_style_names);
    if groups.is_empty() {
        sections.push(properties::fallback_properties(handle, entity));
    } else {
        sections.extend(groups);
    }
    sections
}

pub fn entity_in_working_plane(
    entity: &EntityType,
    plane: crate::command::WorkingPlane,
) -> EntityType {
    let mut local = entity.clone();
    if !plane.is_identity() {
        apply_transform(
            &mut local,
            &EntityTransform::Affine(plane.to_local_transform()),
        );
    }
    local
}

pub fn apply_geom_prop_in_working_plane(
    entity: &mut EntityType,
    field: &str,
    value: &str,
    plane: crate::command::WorkingPlane,
) {
    if plane.is_identity() {
        apply_geom_prop(entity, field, value);
        return;
    }
    let mut local = entity_in_working_plane(entity, plane);
    apply_geom_prop(&mut local, field, value);
    apply_transform(
        &mut local,
        &EntityTransform::Affine(plane.to_world_transform()),
    );
    *entity = local;
}

pub fn apply_common_prop(entity: &mut EntityType, field: &str, value: &str) {
    let e = entity.as_entity_mut();
    match field {
        "layer" => e.set_layer(value.to_string()),
        "linetype" => {
            entity.common_mut().linetype = if value == "ByLayer" {
                String::new()
            } else {
                value.to_string()
            };
        }
        "linetype_scale" => {
            if let Ok(v) = value.trim().parse::<f64>() {
                if v > 0.0 {
                    entity.common_mut().linetype_scale = v;
                }
            }
        }
        "transparency" => {
            let s = value.trim();
            if s.eq_ignore_ascii_case("ByLayer") {
                entity.as_entity_mut().set_transparency(Transparency::new(0));
            } else if s.eq_ignore_ascii_case("ByBlock") {
                entity.as_entity_mut().set_transparency(Transparency::new(1));
            } else {
                let clean = s.trim_end_matches('%').trim();
                if let Ok(pct) = clean.parse::<f64>() {
                    let alpha = (pct.clamp(0.0, 90.0) / 100.0 * 255.0).round() as u8;
                    entity
                        .as_entity_mut()
                        .set_transparency(Transparency::new(alpha));
                }
            }
        }
        "material" => {
            let common = entity.common_mut();
            match value.trim() {
                "ByLayer" => {
                    common.material_flags = 0;
                    common.material_handle = None;
                }
                "ByBlock" => {
                    common.material_flags = 1;
                    common.material_handle = None;
                }
                "Global" => {
                    common.material_flags = 2;
                    common.material_handle = None;
                }
                _ => {}
            }
        }
        "plot_style" => {
            let common = entity.common_mut();
            match value.trim() {
                "ByLayer" => {
                    common.plotstyle_flags = 0;
                    common.plotstyle_handle = None;
                }
                "ByBlock" => {
                    common.plotstyle_flags = 1;
                    common.plotstyle_handle = None;
                }
                "Normal" => {
                    common.plotstyle_flags = 2;
                    common.plotstyle_handle = None;
                }
                _ => {}
            }
        }
        "thickness" => {
            if let Ok(v) = value.trim().parse::<f64>() {
                set_entity_thickness(entity, v);
            }
        }
        _ => {}
    }
}

/// Replace — or, when `values` is `None`, remove — the single XDATA record for
/// application `app` on the entity `handle`, leaving every other record intact.
///
/// This is document-level rather than entity-level for two DWG-correctness
/// reasons the entity alone can't satisfy:
///   * the writer skips a record whose application is not registered in the
///     APPID table, so a brand-new hyperlink/override would silently vanish —
///     register the app (with a real handle) first;
///   * on a DWG read each application's XDATA is captured verbatim in
///     `raw_dwg_eed`, and that verbatim blob *wins* over a structured record on
///     save. Editing an existing record therefore also has to drop that app's
///     stale verbatim block (matched by APPID handle) so the new value wins.
/// Other applications' verbatim blocks are preserved for round-trip fidelity.
pub fn set_entity_xdata(
    doc: &mut acadrust::CadDocument,
    handle: acadrust::Handle,
    app: &str,
    values: Option<Vec<acadrust::xdata::XDataValue>>,
) {
    if values.is_some() && !doc.app_ids.contains(app) {
        let mut a = acadrust::tables::AppId::new(app);
        a.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(a);
    }
    let app_handle = doc.app_ids.get(app).map(|a| a.handle.value());
    let Some(entity) = doc.get_entity_mut(handle) else {
        return;
    };
    let common = entity.common_mut();
    let mut rebuilt = acadrust::xdata::ExtendedData::new();
    for r in common.extended_data.records() {
        if r.application_name != app {
            rebuilt.add_record(r.clone());
        }
    }
    if let Some(vals) = values {
        let mut rec = acadrust::xdata::ExtendedDataRecord::new(app);
        for v in vals {
            rec.add_value(v);
        }
        rebuilt.add_record(rec);
    }
    rebuilt.raw_dwg_eed = common
        .extended_data
        .raw_dwg_eed
        .iter()
        .filter(|(h, _)| Some(*h) != app_handle)
        .cloned()
        .collect();
    common.extended_data = rebuilt;
}

/// The extrusion thickness (DXF 39) of the entities that carry one, or `None`
/// for entity types that have none. Thickness is a per-entity field but is
/// surfaced in the General group (as in a standard properties palette), so
/// this bridges the two.
pub fn entity_thickness(entity: &EntityType) -> Option<f64> {
    Some(match entity {
        EntityType::Arc(e) => e.thickness,
        EntityType::Circle(e) => e.thickness,
        EntityType::Line(e) => e.thickness,
        EntityType::LwPolyline(e) => e.thickness,
        EntityType::Point(e) => e.thickness,
        EntityType::PolyfaceMesh(e) => e.thickness,
        EntityType::Polyline2D(e) => e.thickness,
        EntityType::Shape(e) => e.thickness,
        EntityType::Solid(e) => e.thickness,
        EntityType::Text(e) => e.thickness,
        _ => return None,
    })
}

/// Set the extrusion thickness on the entity types that carry one; no-op for
/// the rest.
pub fn set_entity_thickness(entity: &mut EntityType, v: f64) {
    match entity {
        EntityType::Arc(e) => e.thickness = v,
        EntityType::Circle(e) => e.thickness = v,
        EntityType::Line(e) => e.thickness = v,
        EntityType::LwPolyline(e) => e.thickness = v,
        EntityType::Point(e) => e.thickness = v,
        EntityType::PolyfaceMesh(e) => e.thickness = v,
        EntityType::Polyline2D(e) => e.thickness = v,
        EntityType::Shape(e) => e.thickness = v,
        EntityType::Solid(e) => e.thickness = v,
        EntityType::Text(e) => e.thickness = v,
        _ => {}
    }
}

pub fn toggle_invisible(entity: &mut EntityType) {
    let cur = entity.as_entity_mut().is_invisible();
    entity.as_entity_mut().set_invisible(!cur);
}

pub fn apply_color(entity: &mut EntityType, color: AcadColor) {
    entity.as_entity_mut().set_color(color);
    entity.common_mut().color_book_handle = None;
}

pub fn apply_line_weight(entity: &mut EntityType, lw: LineWeight) {
    entity.as_entity_mut().set_line_weight(lw);
}

pub fn apply_geom_prop(entity: &mut EntityType, field: &str, value: &str) {
    EntityTypeOps::apply_geom_prop(entity, field, value);
}

pub fn apply_grip(entity: &mut EntityType, grip_id: usize, apply: crate::scene::model::object::GripApply) {
    use crate::scene::model::object::GripApply;
    // Grip drags arrive in world space; per-entity apply code writes raw
    // fields, which for planar entities live in OCS — convert first (the
    // pure-rotation map handles points and deltas alike).
    let apply = match (planar_ocs_normal(entity), apply) {
        (Some(n), GripApply::Absolute(p)) => {
            let (x, y, z) = super::transform::wcs_point_to_ocs((p.x, p.y, p.z), n);
            GripApply::Absolute(glam::DVec3::new(x, y, z))
        }
        (Some(n), GripApply::Translate(d)) => {
            let (x, y, z) = super::transform::wcs_point_to_ocs((d.x, d.y, d.z), n);
            GripApply::Translate(glam::DVec3::new(x, y, z))
        }
        (None, a) => a,
    };
    EntityTypeOps::apply_grip(entity, grip_id, apply);
}

/// Rebuild an arc from its drag-start control points and the point grips moved
/// in the current frame. Returns `None` for non-arc entities and `Some(false)`
/// when the requested points are temporarily degenerate.
pub fn refit_arc_grips(
    entity: &mut EntityType,
    original: &EntityType,
    edits: &[(usize, glam::DVec3)],
) -> Option<bool> {
    let EntityType::Arc(arc) = entity else {
        return None;
    };
    let EntityType::Arc(original_arc) = original else {
        return None;
    };
    let edits = match planar_ocs_normal(original) {
        Some(normal) => edits
            .iter()
            .map(|&(grip_id, point)| {
                let (x, y, z) =
                    super::transform::wcs_point_to_ocs((point.x, point.y, point.z), normal);
                (grip_id, glam::DVec3::new(x, y, z))
            })
            .collect::<Vec<_>>(),
        None => edits.to_vec(),
    };
    Some(crate::entities::arc::refit_grips(
        arc,
        original_arc,
        &edits,
    ))
}

/// Convert a world-space cursor point into the value expected by an
/// interactive grip-menu action.
pub fn grip_menu_point_value(
    entity: &EntityType,
    grip_id: usize,
    action: crate::scene::model::object::GripMenuAction,
    point: glam::DVec3,
) -> Option<f64> {
    let point = match planar_ocs_normal(entity) {
        Some(normal) => {
            let (x, y, z) =
                super::transform::wcs_point_to_ocs((point.x, point.y, point.z), normal);
            glam::DVec3::new(x, y, z)
        }
        None => point,
    };
    EntityTypeOps::grip_menu_point_value(entity, grip_id, action, point)
}

pub fn apply_transform(entity: &mut EntityType, t: &EntityTransform) {
    EntityTypeOps::apply_transform(entity, t);
}
