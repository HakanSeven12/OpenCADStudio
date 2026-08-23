use super::*;
use acadrust::entities::{
    CenterLineAssociation, CenterLineSource, CenterLineSourceKind,
};
use acadrust::objects::{XRecordEntry, XRecordValue};
use acadrust::types::Vector3;
use cadkernel::geom2d::{centerline_between, closest_point, Curve, Line as KernelLine};
use glam::DVec3;
use crate::command::WorkingPlane;

const SETTINGS_RECORD: &str = "OPEN_CAD_CENTERLINE_SETTINGS";

#[derive(Clone, Debug)]
pub(crate) struct CenterLineSettings {
    pub extension: f64,
    pub layer: String,
    pub linetype: String,
    pub linetype_scale: f64,
    pub linetype_file: String,
    pub cross_size: String,
    pub cross_gap: String,
    pub mark_extensions: bool,
}

impl Default for CenterLineSettings {
    fn default() -> Self {
        Self {
            extension: 0.12,
            layer: "Current".to_owned(),
            linetype: "CENTER2".to_owned(),
            linetype_scale: 1.0,
            linetype_file: String::new(),
            cross_size: "0.1x".to_owned(),
            cross_gap: "0.05x".to_owned(),
            mark_extensions: true,
        }
    }
}

fn vector(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

fn dvec(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
}

fn ocs_point(point: (f64, f64, f64), normal: Vector3) -> DVec3 {
    let point = crate::scene::view::transform::ocs_point_to_wcs(
        point,
        (normal.x, normal.y, normal.z),
    );
    DVec3::new(point.0, point.1, point.2)
}

fn segment_distance(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    closest_point(&Curve::Line(KernelLine { start, end }), point).distance
}

fn segment_indices(vertex_count: usize, closed: bool) -> impl Iterator<Item = (usize, usize)> {
    let count = if closed && vertex_count > 2 {
        vertex_count
    } else {
        vertex_count.saturating_sub(1)
    };
    (0..count).map(move |start| (start, (start + 1) % vertex_count.max(1)))
}

/// Resolve a user pick to a line or the closest linear polyline segment.
pub(crate) fn picked_source(
    entity: &EntityType,
    handle: Handle,
    pick: DVec3,
) -> Option<(CenterLineSource, DVec3, DVec3)> {
    match entity {
        EntityType::Line(line) if line.length() > 1.0e-10 => Some((
            CenterLineSource {
                handle,
                kind: CenterLineSourceKind::Line,
                segment_index: -1,
                pick_point: vector(pick),
            },
            dvec(line.start),
            dvec(line.end),
        )),
        EntityType::LwPolyline(polyline) => {
            let local_pick = crate::scene::view::transform::wcs_point_to_ocs(
                (pick.x, pick.y, pick.z),
                (polyline.normal.x, polyline.normal.y, polyline.normal.z),
            );
            let pick_2d = [local_pick.0, local_pick.1];
            segment_indices(polyline.vertices.len(), polyline.is_closed)
                .filter(|(start, _)| polyline.vertices[*start].bulge.abs() <= 1.0e-12)
                .filter_map(|(start, end)| {
                    let a = polyline.vertices[start].location;
                    let b = polyline.vertices[end].location;
                    ((a.x - b.x).hypot(a.y - b.y) > 1.0e-10).then_some((start, a, b))
                })
                .min_by(|(_, a, b), (_, c, d)| {
                    segment_distance(pick_2d, [a.x, a.y], [b.x, b.y])
                        .total_cmp(&segment_distance(pick_2d, [c.x, c.y], [d.x, d.y]))
                })
                .map(|(index, a, b)| {
                    let start = ocs_point((a.x, a.y, polyline.elevation), polyline.normal);
                    let end = ocs_point((b.x, b.y, polyline.elevation), polyline.normal);
                    (
                        CenterLineSource {
                            handle,
                            kind: CenterLineSourceKind::LwPolylineSegment,
                            segment_index: index as i32,
                            pick_point: vector(pick),
                        },
                        start,
                        end,
                    )
                })
        }
        EntityType::Polyline2D(polyline) => {
            let local_pick = crate::scene::view::transform::wcs_point_to_ocs(
                (pick.x, pick.y, pick.z),
                (polyline.normal.x, polyline.normal.y, polyline.normal.z),
            );
            let pick_2d = [local_pick.0, local_pick.1];
            segment_indices(polyline.vertices.len(), polyline.is_closed())
                .filter(|(start, _)| polyline.vertices[*start].bulge.abs() <= 1.0e-12)
                .filter_map(|(start, end)| {
                    let a = polyline.vertices[start].location;
                    let b = polyline.vertices[end].location;
                    ((a.x - b.x).hypot(a.y - b.y) > 1.0e-10).then_some((start, a, b))
                })
                .min_by(|(_, a, b), (_, c, d)| {
                    segment_distance(pick_2d, [a.x, a.y], [b.x, b.y])
                        .total_cmp(&segment_distance(pick_2d, [c.x, c.y], [d.x, d.y]))
                })
                .map(|(index, a, b)| {
                    let start = ocs_point((a.x, a.y, polyline.elevation), polyline.normal);
                    let end = ocs_point((b.x, b.y, polyline.elevation), polyline.normal);
                    (
                        CenterLineSource {
                            handle,
                            kind: CenterLineSourceKind::Polyline2DSegment,
                            segment_index: index as i32,
                            pick_point: vector(pick),
                        },
                        start,
                        end,
                    )
                })
        }
        _ => None,
    }
}

fn resolve_source(document: &acadrust::CadDocument, source: &CenterLineSource) -> Option<(DVec3, DVec3)> {
    let entity = document.get_entity(source.handle)?;
    let (_, start, end) = picked_source(entity, source.handle, dvec(source.pick_point))?;
    match source.kind {
        CenterLineSourceKind::Line => matches!(entity, EntityType::Line(_)).then_some((start, end)),
        CenterLineSourceKind::LwPolylineSegment => {
            let EntityType::LwPolyline(polyline) = entity else { return None; };
            let index = usize::try_from(source.segment_index).ok()?;
            let end_index = if index + 1 < polyline.vertices.len() { index + 1 } else if polyline.is_closed { 0 } else { return None; };
            if polyline.vertices.get(index)?.bulge.abs() > 1.0e-12 { return None; }
            let a = polyline.vertices[index].location;
            let b = polyline.vertices[end_index].location;
            Some((
                ocs_point((a.x, a.y, polyline.elevation), polyline.normal),
                ocs_point((b.x, b.y, polyline.elevation), polyline.normal),
            ))
        }
        CenterLineSourceKind::Polyline2DSegment => {
            let EntityType::Polyline2D(polyline) = entity else { return None; };
            let index = usize::try_from(source.segment_index).ok()?;
            let end_index = if index + 1 < polyline.vertices.len() { index + 1 } else if polyline.is_closed() { 0 } else { return None; };
            if polyline.vertices.get(index)?.bulge.abs() > 1.0e-12 { return None; }
            let a = polyline.vertices[index].location;
            let b = polyline.vertices[end_index].location;
            Some((
                ocs_point((a.x, a.y, polyline.elevation), polyline.normal),
                ocs_point((b.x, b.y, polyline.elevation), polyline.normal),
            ))
        }
    }
}

fn rebuild_line(
    document: &acadrust::CadDocument,
    association: &CenterLineAssociation,
) -> Option<acadrust::Line> {
    let first = resolve_source(document, &association.first)?;
    let second = resolve_source(document, &association.second)?;
    construct_line(first, second, association)
}

pub(crate) fn construct_line(
    first: (DVec3, DVec3),
    second: (DVec3, DVec3),
    association: &CenterLineAssociation,
) -> Option<acadrust::Line> {
    let plane = WorkingPlane::new(
        dvec(association.plane_origin),
        dvec(association.plane_x),
        dvec(association.plane_y),
    );
    let first_start = plane.to_local(first.0);
    let first_end = plane.to_local(first.1);
    let second_start = plane.to_local(second.0);
    let second_end = plane.to_local(second.1);
    let first_pick = plane.to_local(dvec(association.first.pick_point));
    let second_pick = plane.to_local(dvec(association.second.pick_point));
    let geometry = centerline_between(
        KernelLine { start: [first_start.x, first_start.y], end: [first_end.x, first_end.y] },
        KernelLine { start: [second_start.x, second_start.y], end: [second_end.x, second_end.y] },
        [first_pick.x, first_pick.y],
        [second_pick.x, second_pick.y],
        association.start_extension + association.start_length_adjustment,
        association.end_extension + association.end_length_adjustment,
    )?;
    let elevation = (first_start.z + first_end.z + second_start.z + second_end.z) * 0.25;
    let start = plane.to_world(DVec3::new(geometry.start[0], geometry.start[1], elevation));
    let end = plane.to_world(DVec3::new(geometry.end[0], geometry.end[1], elevation));
    Some(acadrust::Line::from_points(vector(start), vector(end)))
}

impl Scene {
    fn centerline_settings_owner(&self) -> Option<Handle> {
        self.document
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                ObjectType::Layout(layout) if layout.name.eq_ignore_ascii_case("Model") => {
                    Some(*handle)
                }
                _ => None,
            })
            .or_else(|| self.current_layout_object_handle())
    }

    pub(crate) fn centerline_settings(&self) -> CenterLineSettings {
        let mut defaults = CenterLineSettings::default();
        if matches!(self.document.header.insertion_units, 4..=7 | 11..=17) {
            defaults.extension = 3.5;
        }
        let Some(owner) = self.centerline_settings_owner() else { return defaults; };
        let Some(record) = self.document.xrecord(owner, SETTINGS_RECORD) else { return defaults; };
        let value = |code| record.entries.iter().find(|entry| entry.code == code).map(|entry| &entry.value);
        CenterLineSettings {
            extension: value(40).and_then(XRecordValue::as_double).unwrap_or(defaults.extension),
            layer: value(1).and_then(XRecordValue::as_string).unwrap_or(&defaults.layer).to_owned(),
            linetype: value(2).and_then(XRecordValue::as_string).unwrap_or(&defaults.linetype).to_owned(),
            linetype_scale: value(41).and_then(XRecordValue::as_double).unwrap_or(defaults.linetype_scale),
            linetype_file: value(3).and_then(XRecordValue::as_string).unwrap_or(&defaults.linetype_file).to_owned(),
            cross_size: value(4).and_then(XRecordValue::as_string).unwrap_or(&defaults.cross_size).to_owned(),
            cross_gap: value(5).and_then(XRecordValue::as_string).unwrap_or(&defaults.cross_gap).to_owned(),
            mark_extensions: value(290).and_then(XRecordValue::as_bool).unwrap_or(defaults.mark_extensions),
        }
    }

    pub(crate) fn set_centerline_setting(&mut self, name: &str, value: &str) -> Result<String, String> {
        let Some(owner) = self.centerline_settings_owner() else { return Err("No drawing settings owner.".to_owned()); };
        if name == "CENTERLTYPE"
            && !value.eq_ignore_ascii_case("Current")
            && !self.document.line_types.contains(value)
        {
            return Err(format!("CENTERLTYPE: linetype \"{value}\" is not loaded."));
        }
        if name == "CENTERLTYPEFILE" && !value.trim().is_empty() {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let source = std::fs::read_to_string(value)
                    .map_err(|error| format!("CENTERLTYPEFILE: {error}"))?;
                crate::io::linetypes::populate_document_from_source(
                    &mut self.document,
                    &source,
                );
            }
            #[cfg(target_arch = "wasm32")]
            return Err("CENTERLTYPEFILE is unavailable in the web build.".to_owned());
        }
        self.document.ensure_xrecord(owner, SETTINGS_RECORD);
        let record = self.document.xrecord_mut(owner, SETTINGS_RECORD).ok_or_else(|| "Cannot store centerline settings.".to_owned())?;
        let (code, replacement, display) = match name {
            "CENTEREXE" => {
                let number = value.parse::<f64>().map_err(|_| "CENTEREXE requires a non-negative number.".to_owned())?;
                if !number.is_finite() || number < 0.0 { return Err("CENTEREXE requires a non-negative number.".to_owned()); }
                (40, XRecordValue::Double(number), number.to_string())
            }
            "CENTERLAYER" => (1, XRecordValue::String(value.to_owned()), value.to_owned()),
            "CENTERLTYPE" => (2, XRecordValue::String(value.to_owned()), value.to_owned()),
            "CENTERLTSCALE" => {
                let number = value.parse::<f64>().map_err(|_| "CENTERLTSCALE requires a non-zero number.".to_owned())?;
                if !number.is_finite() || number == 0.0 { return Err("CENTERLTSCALE requires a non-zero number.".to_owned()); }
                (41, XRecordValue::Double(number), number.to_string())
            }
            "CENTERLTYPEFILE" => (3, XRecordValue::String(value.to_owned()), value.to_owned()),
            "CENTERCROSSSIZE" | "CENTERCROSSGAP" => {
                let trimmed = value.trim();
                let valid = trimmed.eq_ignore_ascii_case("ByLineType")
                    || trimmed.strip_suffix(['x', 'X']).is_some_and(|number| {
                        number.parse::<f64>().is_ok_and(|value| value.is_finite() && value > 0.0)
                    })
                    || trimmed.parse::<f64>().is_ok_and(|value| value.is_finite() && value > 0.0);
                if !valid {
                    return Err(format!("{name} requires a positive length, a positive x factor, or ByLineType."));
                }
                let code = if name == "CENTERCROSSSIZE" { 4 } else { 5 };
                (code, XRecordValue::String(trimmed.to_owned()), trimmed.to_owned())
            }
            "CENTERMARKEXE" => {
                let enabled = match value.trim().to_ascii_lowercase().as_str() {
                    "1" | "on" | "yes" | "true" => true,
                    "0" | "off" | "no" | "false" => false,
                    _ => return Err("CENTERMARKEXE requires On/Off or 1/0.".to_owned()),
                };
                (290, XRecordValue::Bool(enabled), if enabled { "1" } else { "0" }.to_owned())
            }
            _ => return Err(format!("Unknown centerline setting: {name}")),
        };
        if let Some(entry) = record.entries.iter_mut().find(|entry| entry.code == code) {
            entry.value = replacement;
        } else {
            record.entries.push(XRecordEntry { code, value: replacement });
        }
        Ok(format!("{name} = {display}"))
    }

    pub(crate) fn refresh_associative_centerlines(&mut self, changes: &[(Handle, ChangeKind)]) -> Vec<(Handle, ChangeKind)> {
        let changed: rustc_hash::FxHashSet<_> = changes.iter().map(|(handle, _)| *handle).collect();
        if changed.is_empty() { return Vec::new(); }
        let candidates: Vec<_> = self.document.entities().filter_map(|entity| {
            let EntityType::Line(line) = entity else { return None; };
            let association = CenterLineAssociation::read(&line.common.extended_data)?;
            (association.associated && (changed.contains(&association.first.handle) || changed.contains(&association.second.handle)))
                .then_some((line.common.handle, association))
        }).collect();
        let mut result = Vec::new();
        for (handle, mut association) in candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(handle);
                self.record_undo_before(handle, before);
            }
            let refreshed = resolve_source(&self.document, &association.first)
                .zip(resolve_source(&self.document, &association.second))
                .and_then(|(first, second)| construct_line(first, second, &association));
            let Some(mut new_line) = refreshed else {
                association.associated = false;
                if let Some(EntityType::Line(line)) = self.document.get_entity_mut(handle) {
                    association.write(&mut line.common.extended_data);
                    result.push((handle, ChangeKind::Modified));
                }
                continue;
            };
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(handle) {
                new_line.common = line.common.clone();
                *line = new_line;
                result.push((handle, ChangeKind::Modified));
            }
        }
        result
    }

    pub(crate) fn reset_centerlines(&mut self, handles: &[Handle]) -> usize {
        let extension = self.centerline_settings().extension;
        let candidates: Vec<_> = handles.iter().filter_map(|handle| {
            let EntityType::Line(line) = self.document.get_entity(*handle)? else { return None; };
            let mut association = CenterLineAssociation::read(&line.common.extended_data)?;
            association.start_extension = extension;
            association.end_extension = extension;
            association.start_length_adjustment = 0.0;
            association.end_length_adjustment = 0.0;
            rebuild_line(&self.document, &association).map(|line| (*handle, association, line))
        }).collect();
        for (handle, association, rebuilt) in &candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(*handle);
                self.record_undo_before(*handle, before);
            }
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(*handle) {
                let mut rebuilt = rebuilt.clone();
                rebuilt.common = line.common.clone();
                association.write(&mut rebuilt.common.extended_data);
                *line = rebuilt;
            }
        }
        if !candidates.is_empty() {
            let changes: Vec<_> = candidates.iter().map(|(handle, _, _)| (*handle, ChangeKind::Modified)).collect();
            self.bump_entities(&changes);
        }
        candidates.len()
    }

    pub(crate) fn set_centerline_association(&mut self, handles: &[Handle], associated: bool) -> usize {
        let candidates: Vec<_> = handles.iter().filter_map(|handle| {
            let EntityType::Line(line) = self.document.get_entity(*handle)? else { return None; };
            let mut association = CenterLineAssociation::read(&line.common.extended_data)?;
            association.associated = associated;
            let rebuilt = associated.then(|| rebuild_line(&self.document, &association)).flatten();
            (!associated || rebuilt.is_some()).then_some((*handle, association, rebuilt))
        }).collect();
        for (handle, association, rebuilt) in &candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(*handle);
                self.record_undo_before(*handle, before);
            }
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(*handle) {
                if let Some(mut rebuilt) = rebuilt.clone() {
                    rebuilt.common = line.common.clone();
                    association.write(&mut rebuilt.common.extended_data);
                    *line = rebuilt;
                } else {
                    association.write(&mut line.common.extended_data);
                }
            }
        }
        if !candidates.is_empty() {
            let changes: Vec<_> = candidates.iter().map(|(handle, _, _)| (*handle, ChangeKind::Modified)).collect();
            self.bump_entities(&changes);
        }
        candidates.len()
    }
}

pub(crate) fn resolve_center_measure(specification: &str, diameter: f64, fallback_factor: f64) -> f64 {
    let text = specification.trim();
    if text.eq_ignore_ascii_case("ByLineType") {
        return diameter * fallback_factor;
    }
    if let Some(factor) = text.strip_suffix(['x', 'X']).and_then(|value| value.parse::<f64>().ok()) {
        return diameter * factor.max(0.0);
    }
    text.parse::<f64>().unwrap_or(diameter * fallback_factor).max(0.0)
}

pub(crate) fn center_measure_is_relative(specification: &str) -> bool {
    let text = specification.trim();
    text.eq_ignore_ascii_case("ByLineType") || text.ends_with(['x', 'X'])
}
