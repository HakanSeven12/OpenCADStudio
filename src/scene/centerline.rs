use super::*;
use crate::command::WorkingPlane;
use codec::entities::{CenterLineAssociation, CenterLineSource, CenterLineSourceKind};
use codec::objects::XRecordValue;
use codec::types::Vector3;
use glam::DVec3;
use kernel::geom2d::{centerline_between, closest_point, Curve, Line as KernelLine};

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
            layer: ".".to_owned(),
            linetype: String::new(),
            linetype_scale: 1.0,
            linetype_file: "acad.lin".to_owned(),
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
    let point =
        crate::scene::view::transform::ocs_point_to_wcs(point, (normal.x, normal.y, normal.z));
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
                    segment_distance(pick_2d, [a.x, a.y], [b.x, b.y]).total_cmp(&segment_distance(
                        pick_2d,
                        [c.x, c.y],
                        [d.x, d.y],
                    ))
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
                    segment_distance(pick_2d, [a.x, a.y], [b.x, b.y]).total_cmp(&segment_distance(
                        pick_2d,
                        [c.x, c.y],
                        [d.x, d.y],
                    ))
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

/// The current endpoints of a centre-line source edge.
pub(crate) fn source_segment(
    document: &codec::Document,
    source: &CenterLineSource,
) -> Option<(DVec3, DVec3)> {
    resolve_source(document, source)
}

fn resolve_source(
    document: &codec::Document,
    source: &CenterLineSource,
) -> Option<(DVec3, DVec3)> {
    let entity = document.get_entity(source.handle)?;
    let (_, start, end) = picked_source(entity, source.handle, dvec(source.pick_point))?;
    match source.kind {
        CenterLineSourceKind::Line => matches!(entity, EntityType::Line(_)).then_some((start, end)),
        CenterLineSourceKind::LwPolylineSegment => {
            let EntityType::LwPolyline(polyline) = entity else {
                return None;
            };
            let index = usize::try_from(source.segment_index).ok()?;
            let end_index = if index + 1 < polyline.vertices.len() {
                index + 1
            } else if polyline.is_closed {
                0
            } else {
                return None;
            };
            if polyline.vertices.get(index)?.bulge.abs() > 1.0e-12 {
                return None;
            }
            let a = polyline.vertices[index].location;
            let b = polyline.vertices[end_index].location;
            Some((
                ocs_point((a.x, a.y, polyline.elevation), polyline.normal),
                ocs_point((b.x, b.y, polyline.elevation), polyline.normal),
            ))
        }
        CenterLineSourceKind::Polyline2DSegment => {
            let EntityType::Polyline2D(polyline) = entity else {
                return None;
            };
            let index = usize::try_from(source.segment_index).ok()?;
            let end_index = if index + 1 < polyline.vertices.len() {
                index + 1
            } else if polyline.is_closed() {
                0
            } else {
                return None;
            };
            if polyline.vertices.get(index)?.bulge.abs() > 1.0e-12 {
                return None;
            }
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
    document: &codec::Document,
    association: &CenterLineAssociation,
) -> Option<codec::Line> {
    let first = resolve_source(document, &association.first)?;
    let second = resolve_source(document, &association.second)?;
    construct_line(first, second, association)
}

pub(crate) fn construct_line(
    first: (DVec3, DVec3),
    second: (DVec3, DVec3),
    association: &CenterLineAssociation,
) -> Option<codec::Line> {
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
        KernelLine {
            start: [first_start.x, first_start.y],
            end: [first_end.x, first_end.y],
        },
        KernelLine {
            start: [second_start.x, second_start.y],
            end: [second_end.x, second_end.y],
        },
        [first_pick.x, first_pick.y],
        [second_pick.x, second_pick.y],
        association.start_extension + association.start_length_adjustment,
        association.end_extension + association.end_length_adjustment,
    )?;
    let elevation = (first_start.z + first_end.z + second_start.z + second_end.z) * 0.25;
    let start = plane.to_world(DVec3::new(geometry.start[0], geometry.start[1], elevation));
    let end = plane.to_world(DVec3::new(geometry.end[0], geometry.end[1], elevation));
    Some(codec::Line::from_points(vector(start), vector(end)))
}

/// Reverse index from a centre-line source handle to the carrier lines
/// attached to it. `order` records each carrier's position in
/// `Document::entities`, so a refresh still visits candidates in document
/// order: a centre line may itself be another one's source, and the earlier
/// carrier must rebuild before the later one reads it.
pub(crate) struct CenterLineSourceIndex {
    by_source: HashMap<Handle, Vec<Handle>>,
    order: HashMap<Handle, usize>,
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

    /// CENTEREXE, CENTERLAYER, CENTERLTYPE, CENTERLTSCALE, CENTERLTYPEFILE,
    /// CENTERCROSSSIZE, CENTERCROSSGAP and CENTERMARKEXE of this drawing,
    /// kept in its variable dictionary (as the reference stores them); a
    /// drawing saved by an earlier release keeps its settings record.
    pub(crate) fn centerline_settings(&self) -> CenterLineSettings {
        let mut defaults = CenterLineSettings::default();
        if self.document.header.measurement == 1
            || matches!(self.document.header.insertion_units, 4..=7 | 11..=17)
        {
            defaults.extension = 3.5;
        }
        defaults.linetype_file = if self.document.header.measurement == 1 {
            "acadiso.lin"
        } else {
            "acad.lin"
        }
        .to_owned();
        let legacy = self
            .centerline_settings_owner()
            .and_then(|owner| self.document.xrecord(owner, SETTINGS_RECORD));
        let legacy_value = |code| {
            legacy.and_then(|record| {
                record
                    .entries
                    .iter()
                    .find(|entry| entry.code == code)
                    .map(|entry| &entry.value)
            })
        };
        let variable = |name: &str| crate::io::drawing_variable(&self.document, name);
        let number = |name: &str, code, fallback: f64| {
            variable(name)
                .and_then(|value| value.trim().parse::<f64>().ok())
                .or_else(|| legacy_value(code).and_then(XRecordValue::as_double))
                .unwrap_or(fallback)
        };
        let string = |name: &str, code, fallback: &str| {
            variable(name)
                .or_else(|| {
                    legacy_value(code)
                        .and_then(XRecordValue::as_string)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| fallback.to_owned())
        };
        // The settings record called the current layer / linetype "Current".
        let current = |value: String| {
            if value.eq_ignore_ascii_case("Current") {
                ".".to_owned()
            } else {
                value
            }
        };
        CenterLineSettings {
            extension: number("CENTEREXE", 40, defaults.extension),
            layer: current(string("CENTERLAYER", 1, &defaults.layer)),
            linetype: current(string("CENTERLTYPE", 2, &defaults.linetype)),
            linetype_scale: number("CENTERLTSCALE", 41, defaults.linetype_scale),
            linetype_file: string("CENTERLTYPEFILE", 3, &defaults.linetype_file),
            cross_size: string("CENTERCROSSSIZE", 4, &defaults.cross_size),
            cross_gap: string("CENTERCROSSGAP", 5, &defaults.cross_gap),
            mark_extensions: variable("CENTERMARKEXE")
                .map(|value| value.trim() != "0")
                .or_else(|| legacy_value(290).and_then(XRecordValue::as_bool))
                .unwrap_or(defaults.mark_extensions),
        }
    }

    /// Store one centre setting, already validated, in the drawing's
    /// variable dictionary as the reference writes it.
    pub(crate) fn set_centerline_setting(&mut self, name: &str, value: &str) {
        let stored = match name {
            "CENTEREXE" | "CENTERLTSCALE" => value
                .trim()
                .parse::<f64>()
                .map(|number| format!("{number:.6}"))
                .unwrap_or_else(|_| value.to_owned()),
            _ => value.to_owned(),
        };
        crate::io::set_drawing_variable(&mut self.document, name, &stored);
    }

    /// Centre-line carriers attached to the changed source handles, read from
    /// the reverse index (built on first use, dropped whenever an entity or an
    /// association may have changed). Replaces the per-edit walk over every
    /// line and parse of its xdata; candidates are re-validated against their
    /// current xdata, so a stale entry can only cost a lookup.
    fn associative_centerline_dependents(
        &self,
        changed: &rustc_hash::FxHashSet<Handle>,
    ) -> Vec<Handle> {
        if self
            .associative_centerline_source_cache
            .borrow()
            .is_none()
        {
            let mut by_source: HashMap<Handle, Vec<Handle>> = HashMap::default();
            let mut order: HashMap<Handle, usize> = HashMap::default();
            for (position, entity) in self.document.entities().enumerate() {
                let EntityType::Line(line) = entity else {
                    continue;
                };
                let Some(association) = CenterLineAssociation::read(&line.common.extended_data)
                else {
                    continue;
                };
                if !association.associated {
                    continue;
                }
                order.insert(line.common.handle, position);
                for source in [association.first.handle, association.second.handle] {
                    let carriers = by_source.entry(source).or_default();
                    if !carriers.contains(&line.common.handle) {
                        carriers.push(line.common.handle);
                    }
                }
            }
            *self.associative_centerline_source_cache.borrow_mut() =
                Some(CenterLineSourceIndex { by_source, order });
        }
        let cache = self.associative_centerline_source_cache.borrow();
        let index = cache.as_ref().expect("associative centre line index");
        let mut seen = rustc_hash::FxHashSet::default();
        let mut handles = Vec::new();
        for source in changed {
            if let Some(carriers) = index.by_source.get(source) {
                for &carrier in carriers {
                    if seen.insert(carrier) {
                        handles.push(carrier);
                    }
                }
            }
        }
        handles.sort_unstable_by_key(|handle| {
            index.order.get(handle).copied().unwrap_or(usize::MAX)
        });
        handles
    }

    pub(crate) fn refresh_associative_centerlines(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) -> Vec<(Handle, ChangeKind)> {
        let changed: rustc_hash::FxHashSet<_> = changes.iter().map(|(handle, _)| *handle).collect();
        if changed.is_empty() {
            return Vec::new();
        }
        let candidates: Vec<_> = self
            .associative_centerline_dependents(&changed)
            .into_iter()
            .filter_map(|handle| {
                let EntityType::Line(line) = self.document.get_entity(handle)? else {
                    return None;
                };
                let association = CenterLineAssociation::read(&line.common.extended_data)?;
                (association.associated
                    && (changed.contains(&association.first.handle)
                        || changed.contains(&association.second.handle)))
                .then_some((handle, association))
            })
            .collect();
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

    /// Attach centre line `target` to the lines picked at `first` and
    /// `second`, keeping its extensions, and rebuild it from them.
    pub(crate) fn reassociate_centerline(
        &mut self,
        target: Handle,
        first: (Handle, DVec3),
        second: (Handle, DVec3),
    ) -> bool {
        let pick = |document: &codec::Document, (handle, point): (Handle, DVec3)| {
            picked_source(document.get_entity(handle)?, handle, point)
        };
        let (Some(first), Some(second)) = (pick(&self.document, first), pick(&self.document, second))
        else {
            return false;
        };
        let Some(EntityType::Line(line)) = self.document.get_entity(target) else {
            return false;
        };
        let Some(mut association) = CenterLineAssociation::read(&line.common.extended_data) else {
            return false;
        };
        association.first = first.0;
        association.second = second.0;
        association.start_length_adjustment = 0.0;
        association.end_length_adjustment = 0.0;
        association.associated = true;
        let Some(mut rebuilt) =
            construct_line((first.1, first.2), (second.1, second.2), &association)
        else {
            return false;
        };
        if self.is_recording_undo() {
            let before = self.document.get_entity_arc(target);
            self.record_undo_before(target, before);
        }
        if let Some(EntityType::Line(line)) = self.document.get_entity_mut(target) {
            rebuilt.common = line.common.clone();
            association.write(&mut rebuilt.common.extended_data);
            *line = rebuilt;
        }
        self.associative_centerline_source_cache.borrow_mut().take();
        self.bump_entities(&[(target, ChangeKind::Modified)]);
        true
    }

    pub(crate) fn reset_centerlines(&mut self, handles: &[Handle]) -> usize {
        let extension = self.centerline_settings().extension;
        let candidates: Vec<_> = handles
            .iter()
            .filter_map(|handle| {
                let EntityType::Line(line) = self.document.get_entity(*handle)? else {
                    return None;
                };
                let mut association = CenterLineAssociation::read(&line.common.extended_data)?;
                association.start_extension = extension;
                association.end_extension = extension;
                association.start_length_adjustment = 0.0;
                association.end_length_adjustment = 0.0;
                rebuild_line(&self.document, &association).map(|line| (*handle, association, line))
            })
            .collect();
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
            let changes: Vec<_> = candidates
                .iter()
                .map(|(handle, _, _)| (*handle, ChangeKind::Modified))
                .collect();
            self.bump_entities(&changes);
        }
        candidates.len()
    }

    pub(crate) fn set_centerline_association(
        &mut self,
        handles: &[Handle],
        associated: bool,
    ) -> usize {
        let candidates: Vec<_> = handles
            .iter()
            .filter_map(|handle| {
                let EntityType::Line(line) = self.document.get_entity(*handle)? else {
                    return None;
                };
                let mut association = CenterLineAssociation::read(&line.common.extended_data)?;
                association.associated = associated;
                let rebuilt = associated
                    .then(|| rebuild_line(&self.document, &association))
                    .flatten();
                (!associated || rebuilt.is_some()).then_some((*handle, association, rebuilt))
            })
            .collect();
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
            // The association flags just changed. Drop the index; the bump
            // below re-indexes from the new xdata (an all-disassociated bump
            // rebuilds an empty index and refreshes nothing).
            self.associative_centerline_source_cache
                .borrow_mut()
                .take();
            let changes: Vec<_> = candidates
                .iter()
                .map(|(handle, _, _)| (*handle, ChangeKind::Modified))
                .collect();
            self.bump_entities(&changes);
        }
        candidates.len()
    }
}

pub(crate) fn resolve_center_measure(
    specification: &str,
    diameter: f64,
    fallback_factor: f64,
) -> f64 {
    let text = specification.trim();
    if text.eq_ignore_ascii_case("ByLineType") {
        return diameter * fallback_factor;
    }
    if let Some(factor) = text
        .strip_suffix(['x', 'X'])
        .and_then(|value| value.parse::<f64>().ok())
    {
        return diameter * factor.max(0.0);
    }
    text.parse::<f64>()
        .unwrap_or(diameter * fallback_factor)
        .max(0.0)
}

pub(crate) fn center_measure_is_relative(specification: &str) -> bool {
    let text = specification.trim();
    text.eq_ignore_ascii_case("ByLineType") || text.ends_with(['x', 'X'])
}

/// Whether `entity` is a centre mark or a centre line.
pub(crate) fn is_center_carrier(entity: &EntityType) -> bool {
    let EntityType::Line(line) = entity else {
        return false;
    };
    CenterLineAssociation::read(&line.common.extended_data).is_some()
        || codec::entities::CenterMarkAssociation::read(&line.common.extended_data).is_some()
}

impl Scene {
    /// Whether `handle` is a centre mark or a centre line.
    pub(crate) fn is_center_object(&self, handle: Handle) -> bool {
        self.document
            .get_entity(handle)
            .is_some_and(is_center_carrier)
    }

    /// (is a centre mark, is associated) of centre object `handle`.
    pub(crate) fn center_object_state(&self, handle: Handle) -> Option<(bool, bool)> {
        let EntityType::Line(line) = self.document.get_entity(handle)? else {
            return None;
        };
        if let Some(mark) = codec::entities::CenterMarkAssociation::read(&line.common.extended_data)
        {
            return Some((true, mark.associated));
        }
        CenterLineAssociation::read(&line.common.extended_data)
            .map(|association| (false, association.associated))
    }

    /// The drawing's layer names, for the centre commands' `?` listing.
    pub(crate) fn layer_names_for_center(&self) -> Vec<String> {
        self.document
            .layers
            .iter()
            .map(|layer| layer.name.clone())
            .collect()
    }

    /// Report each centre object `changes` move off their sources: one the
    /// open edit found associated is now detached (as MOVE, a grip or a
    /// property edit leave it).
    pub(crate) fn note_center_disassociations(&mut self, changes: &[(Handle, ChangeKind)]) {
        let Some(recording) = self.undo_recording.as_ref() else {
            return;
        };
        let mut notices = Vec::new();
        for (handle, kind) in changes {
            if !matches!(kind, ChangeKind::Modified) {
                continue;
            }
            let Some((mark, false)) = self.center_object_state(*handle) else {
                continue;
            };
            let before = recording.before.get(handle).and_then(|before| before.as_deref());
            let was_associated = match before {
                Some(EntityType::Line(line)) => {
                    codec::entities::CenterMarkAssociation::read(&line.common.extended_data)
                        .map(|association| association.associated)
                        .or_else(|| {
                            CenterLineAssociation::read(&line.common.extended_data)
                                .map(|association| association.associated)
                        })
                        .unwrap_or(false)
                }
                _ => false,
            };
            if was_associated {
                notices.push(if mark {
                    crate::tr!("center-cmd", "mark-disassociated")
                } else {
                    crate::tr!("center-cmd", "line-disassociated")
                });
            }
        }
        self.center_notices.extend(notices);
    }
}

/// Whether `entity` carries a centerline association.
///
/// Used to keep the drawing-wide scan off the edit path: only an entity that
/// actually carries one can make the scan necessary.
pub(crate) fn entity_has_center_association(entity: &EntityType) -> bool {
    let EntityType::Line(line) = entity else {
        return false;
    };
    CenterLineAssociation::read(&line.common.extended_data)
        .is_some_and(|association| association.associated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::Line;

    fn line(y: f64) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(0.0, y, 0.0),
            Vector3::new(100.0, y, 0.0),
        ))
    }

    fn association(first: Handle, second: Handle) -> CenterLineAssociation {
        CenterLineAssociation {
            first: CenterLineSource {
                handle: first,
                kind: CenterLineSourceKind::Line,
                segment_index: -1,
                pick_point: Vector3::new(50.0, 0.0, 0.0),
            },
            second: CenterLineSource {
                handle: second,
                kind: CenterLineSourceKind::Line,
                segment_index: -1,
                pick_point: Vector3::new(50.0, 10.0, 0.0),
            },
            plane_origin: Vector3::new(0.0, 0.0, 0.0),
            plane_x: Vector3::new(1.0, 0.0, 0.0),
            plane_y: Vector3::new(0.0, 1.0, 0.0),
            start_extension: 0.0,
            end_extension: 0.0,
            start_length_adjustment: 0.0,
            end_length_adjustment: 0.0,
            associated: true,
        }
    }

    fn carrier(first: Handle, second: Handle) -> EntityType {
        let EntityType::Line(mut line) = line(5.0) else {
            unreachable!()
        };
        association(first, second).write(&mut line.common.extended_data);
        EntityType::Line(line)
    }

    fn carrier_y(scene: &Scene, handle: Handle) -> f64 {
        let Some(EntityType::Line(line)) = scene.document.get_entity(handle) else {
            panic!("carrier is not a line");
        };
        assert!((line.start.y - line.end.y).abs() < 1e-9, "carrier not horizontal");
        line.start.y
    }

    fn move_line(scene: &mut Scene, handle: Handle, y: f64) {
        let Some(EntityType::Line(line)) = scene.document.get_entity_mut(handle) else {
            panic!("source is not a line");
        };
        line.start.y = y;
        line.end.y = y;
    }

    #[test]
    fn index_finds_carriers_and_follows_reassociation() {
        let mut scene = Scene::new();
        let first = scene.add_entity(line(0.0));
        let second = scene.add_entity(line(10.0));
        let carrier = scene.add_entity(carrier(first, second));
        assert!((carrier_y(&scene, carrier) - 5.0).abs() < 1e-9);

        // The index must find the carrier for an edit of one of its sources.
        move_line(&mut scene, first, 2.0);
        scene.bump_entities(&[(first, ChangeKind::Modified)]);
        assert!((carrier_y(&scene, carrier) - 6.0).abs() < 1e-9);

        // Disassociated: source edits must leave the carrier alone.
        assert_eq!(scene.set_centerline_association(&[carrier], false), 1);
        move_line(&mut scene, first, 4.0);
        scene.bump_entities(&[(first, ChangeKind::Modified)]);
        assert!((carrier_y(&scene, carrier) - 6.0).abs() < 1e-9);

        // Re-associated: the rebuilt index must pick the carrier up again.
        assert_eq!(scene.set_centerline_association(&[carrier], true), 1);
        move_line(&mut scene, first, 0.0);
        scene.bump_entities(&[(first, ChangeKind::Modified)]);
        assert!((carrier_y(&scene, carrier) - 5.0).abs() < 1e-9);
    }

    /// Paste inserts entities straight into the document and bumps them as
    /// `Added`; a carrier arriving that way must still reach a warm index.
    #[test]
    fn index_picks_up_a_carrier_added_after_it_was_built() {
        let mut scene = Scene::new();
        let first = scene.add_entity(line(0.0));
        let second = scene.add_entity(line(10.0));
        let early = scene.add_entity(carrier(first, second));
        move_line(&mut scene, first, 2.0);
        scene.bump_entities(&[(first, ChangeKind::Modified)]);
        assert!((carrier_y(&scene, early) - 6.0).abs() < 1e-9, "index is warm");

        let pasted = scene
            .document
            .add_entity(carrier(first, second))
            .unwrap_or(Handle::NULL);
        assert!(!pasted.is_null());
        scene.bump_entities(&[(pasted, ChangeKind::Added)]);

        move_line(&mut scene, first, 4.0);
        scene.bump_entities(&[(first, ChangeKind::Modified)]);
        assert!((carrier_y(&scene, pasted) - 7.0).abs() < 1e-9);
        assert!((carrier_y(&scene, early) - 7.0).abs() < 1e-9);
    }

    /// A centre line can be another one's source: candidates must come back
    /// in document order, so the inner carrier rebuilds before the outer one
    /// reads it.
    #[test]
    fn nested_carriers_refresh_in_document_order() {
        let mut scene = Scene::new();
        let a = scene.add_entity(line(0.0));
        let b = scene.add_entity(line(10.0));
        let c = scene.add_entity(line(20.0));
        let inner = scene.add_entity(carrier(a, b));
        let outer = scene.add_entity(carrier(inner, c));
        // The carrier helper seeds y = 5; put the outer one where CENTERLINE
        // would have built it (midway between 5 and 20).
        move_line(&mut scene, outer, 12.5);
        assert!((carrier_y(&scene, inner) - 5.0).abs() < 1e-9);
        assert!((carrier_y(&scene, outer) - 12.5).abs() < 1e-9);

        // One bump naming both the source and the inner carrier.
        move_line(&mut scene, a, 4.0);
        scene.bump_entities(&[
            (a, ChangeKind::Modified),
            (inner, ChangeKind::Modified),
        ]);
        assert!((carrier_y(&scene, inner) - 7.0).abs() < 1e-9);
        assert!((carrier_y(&scene, outer) - 13.5).abs() < 1e-9);
    }
}
