use super::*;
use codec::entities::{CenterMarkAssociation, CenterMarkSource, CenterMarkSourceKind};
use codec::types::Vector3;
use glam::DVec3;
use kernel::geom2d::{closest_point, Arc as KernelArc, BulgeArc, Curve};

fn vector(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

pub(crate) fn dvec(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
}

fn ocs_point(point: (f64, f64, f64), normal: Vector3) -> DVec3 {
    let point =
        crate::scene::view::transform::ocs_point_to_wcs(point, (normal.x, normal.y, normal.z));
    DVec3::new(point.0, point.1, point.2)
}

fn axes(normal: Vector3) -> (DVec3, DVec3) {
    let (x, y) = crate::scene::view::transform::ocs_axes((normal.x, normal.y, normal.z));
    (DVec3::new(x.0, x.1, x.2), DVec3::new(y.0, y.1, y.2))
}

fn arc_distance(arc: &BulgeArc, pick: [f64; 2]) -> f64 {
    let (start_angle, end_angle) = if arc.sweep >= 0.0 {
        (arc.start_angle, arc.end_angle)
    } else {
        (arc.end_angle, arc.start_angle)
    };
    closest_point(
        &Curve::Arc(KernelArc {
            centre: arc.center,
            radius: arc.radius,
            start_angle,
            end_angle,
        }),
        pick,
    )
    .distance
}

fn segment_count(vertices: usize, closed: bool) -> usize {
    if closed && vertices > 1 {
        vertices
    } else {
        vertices.saturating_sub(1)
    }
}

/// Resolve a pick to circular source geometry, including bulged polyline arcs.
pub(crate) fn picked_mark_source(
    entity: &EntityType,
    handle: Handle,
    pick: DVec3,
) -> Option<(CenterMarkSource, DVec3, f64, DVec3, DVec3)> {
    match entity {
        EntityType::Circle(circle) if circle.radius > 1.0e-10 => {
            let center = ocs_point(
                (circle.center.x, circle.center.y, circle.center.z),
                circle.normal,
            );
            let (x, y) = axes(circle.normal);
            Some((
                CenterMarkSource {
                    handle,
                    kind: CenterMarkSourceKind::Circle,
                    segment_index: -1,
                    pick_point: vector(pick),
                },
                center,
                circle.radius,
                x,
                y,
            ))
        }
        EntityType::Arc(arc) if arc.radius > 1.0e-10 => {
            let center = ocs_point((arc.center.x, arc.center.y, arc.center.z), arc.normal);
            let (x, y) = axes(arc.normal);
            Some((
                CenterMarkSource {
                    handle,
                    kind: CenterMarkSourceKind::Arc,
                    segment_index: -1,
                    pick_point: vector(pick),
                },
                center,
                arc.radius,
                x,
                y,
            ))
        }
        EntityType::LwPolyline(polyline) => {
            let local_pick = {
                let origin = ocs_point((0.0, 0.0, polyline.elevation), polyline.normal);
                let (x, y) = axes(polyline.normal);
                let delta = pick - origin;
                [delta.dot(x), delta.dot(y)]
            };
            let count = segment_count(polyline.vertices.len(), polyline.is_closed);
            let (index, arc) = (0..count)
                .filter_map(|index| {
                    let next = (index + 1) % polyline.vertices.len();
                    let a = polyline.vertices[index].location;
                    let b = polyline.vertices[next].location;
                    BulgeArc::from_bulge([a.x, a.y], [b.x, b.y], polyline.vertices[index].bulge)
                        .map(|arc| (index, arc))
                })
                .min_by(|(_, a), (_, b)| {
                    arc_distance(a, local_pick).total_cmp(&arc_distance(b, local_pick))
                })?;
            let center = ocs_point(
                (arc.center[0], arc.center[1], polyline.elevation),
                polyline.normal,
            );
            let (x, y) = axes(polyline.normal);
            Some((
                CenterMarkSource {
                    handle,
                    kind: CenterMarkSourceKind::LwPolylineArcSegment,
                    segment_index: index as i32,
                    pick_point: vector(pick),
                },
                center,
                arc.radius,
                x,
                y,
            ))
        }
        EntityType::Polyline2D(polyline) => {
            let local_pick = {
                let origin = ocs_point((0.0, 0.0, polyline.elevation), polyline.normal);
                let (x, y) = axes(polyline.normal);
                let delta = pick - origin;
                [delta.dot(x), delta.dot(y)]
            };
            let count = segment_count(polyline.vertices.len(), polyline.is_closed());
            let (index, arc) = (0..count)
                .filter_map(|index| {
                    let next = (index + 1) % polyline.vertices.len();
                    let a = polyline.vertices[index].location;
                    let b = polyline.vertices[next].location;
                    BulgeArc::from_bulge([a.x, a.y], [b.x, b.y], polyline.vertices[index].bulge)
                        .map(|arc| (index, arc))
                })
                .min_by(|(_, a), (_, b)| {
                    arc_distance(a, local_pick).total_cmp(&arc_distance(b, local_pick))
                })?;
            let center = ocs_point(
                (arc.center[0], arc.center[1], polyline.elevation),
                polyline.normal,
            );
            let (x, y) = axes(polyline.normal);
            Some((
                CenterMarkSource {
                    handle,
                    kind: CenterMarkSourceKind::Polyline2DArcSegment,
                    segment_index: index as i32,
                    pick_point: vector(pick),
                },
                center,
                arc.radius,
                x,
                y,
            ))
        }
        _ => None,
    }
}

fn resolve_source(
    document: &codec::Document,
    source: &CenterMarkSource,
) -> Option<(DVec3, f64, DVec3, DVec3)> {
    let entity = document.get_entity(source.handle)?;
    match (source.kind, entity) {
        (CenterMarkSourceKind::Circle, EntityType::Circle(circle)) if circle.radius > 1.0e-10 => {
            let center = ocs_point(
                (circle.center.x, circle.center.y, circle.center.z),
                circle.normal,
            );
            let (x, y) = axes(circle.normal);
            Some((center, circle.radius, x, y))
        }
        (CenterMarkSourceKind::Arc, EntityType::Arc(arc)) if arc.radius > 1.0e-10 => {
            let center = ocs_point((arc.center.x, arc.center.y, arc.center.z), arc.normal);
            let (x, y) = axes(arc.normal);
            Some((center, arc.radius, x, y))
        }
        (CenterMarkSourceKind::LwPolylineArcSegment, EntityType::LwPolyline(polyline)) => {
            let index = usize::try_from(source.segment_index).ok()?;
            let count = segment_count(polyline.vertices.len(), polyline.is_closed);
            if index >= count {
                return None;
            }
            let next = (index + 1) % polyline.vertices.len();
            let a = polyline.vertices[index].location;
            let b = polyline.vertices[next].location;
            let arc = BulgeArc::from_bulge([a.x, a.y], [b.x, b.y], polyline.vertices[index].bulge)?;
            let center = ocs_point(
                (arc.center[0], arc.center[1], polyline.elevation),
                polyline.normal,
            );
            let (x, y) = axes(polyline.normal);
            Some((center, arc.radius, x, y))
        }
        (CenterMarkSourceKind::Polyline2DArcSegment, EntityType::Polyline2D(polyline)) => {
            let index = usize::try_from(source.segment_index).ok()?;
            let count = segment_count(polyline.vertices.len(), polyline.is_closed());
            if index >= count {
                return None;
            }
            let next = (index + 1) % polyline.vertices.len();
            let a = polyline.vertices[index].location;
            let b = polyline.vertices[next].location;
            let arc = BulgeArc::from_bulge([a.x, a.y], [b.x, b.y], polyline.vertices[index].bulge)?;
            let center = ocs_point(
                (arc.center[0], arc.center[1], polyline.elevation),
                polyline.normal,
            );
            let (x, y) = axes(polyline.normal);
            Some((center, arc.radius, x, y))
        }
        _ => None,
    }
}

pub(crate) fn mark_directions(association: &CenterMarkAssociation) -> [DVec3; 4] {
    let x = dvec(association.plane_x).normalize_or(DVec3::X);
    let y = dvec(association.plane_y).normalize_or(DVec3::Y);
    [x, -x, y, -y]
}

pub(crate) fn mark_segments(association: &CenterMarkAssociation) -> Vec<[DVec3; 2]> {
    let center = dvec(association.center);
    let directions = mark_directions(association);
    let half = (association.cross_size * 0.5).max(0.0);
    let mut segments = vec![
        [center - directions[0] * half, center + directions[0] * half],
        [center - directions[2] * half, center + directions[2] * half],
    ];
    if association.show_extensions && association.radius > half + association.cross_gap {
        for (index, direction) in directions.into_iter().enumerate() {
            let start = center + direction * (half + association.cross_gap);
            let end_distance = (association.radius
                + association.extension_length
                + association.length_adjustments[index]
                + association.overshoots[index])
                .max(half + association.cross_gap);
            segments.push([start, center + direction * end_distance]);
        }
    }
    segments
}

pub(crate) fn render_segments(association: &CenterMarkAssociation) -> Vec<[DVec3; 2]> {
    let mut segments = mark_segments(association);
    if association.associated {
        return segments;
    }
    let center = dvec(association.center);
    let directions = mark_directions(association);
    let size = association
        .cross_size
        .max(association.radius * 0.08)
        .max(1.0e-6)
        * 0.35;
    let badge_center = center
        + directions[0] * (association.radius + association.extension_length + size * 2.0)
        + directions[2] * size * 2.0;
    let corners = [
        badge_center + directions[2] * size,
        badge_center + directions[0] * size,
        badge_center - directions[2] * size,
        badge_center - directions[0] * size,
    ];
    for index in 0..4 {
        segments.push([corners[index], corners[(index + 1) % 4]]);
    }
    segments.push([
        badge_center + directions[2] * size * 0.45,
        badge_center - directions[2] * size * 0.2,
    ]);
    segments.push([
        badge_center - directions[2] * size * 0.55,
        badge_center - directions[2] * size * 0.65,
    ]);
    segments
}

pub(crate) fn mark_bounds(association: &CenterMarkAssociation) -> ([f64; 3], [f64; 3]) {
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for point in render_segments(association).into_iter().flatten() {
        let point = point.to_array();
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(point[axis]);
            maximum[axis] = maximum[axis].max(point[axis]);
        }
    }
    (minimum, maximum)
}

fn apply_source_geometry(
    association: &mut CenterMarkAssociation,
    center: DVec3,
    radius: f64,
    x: DVec3,
    y: DVec3,
) {
    let old_diameter = association.radius * 2.0;
    let size_factor = (old_diameter > 1.0e-12)
        .then_some(association.cross_size / old_diameter)
        .unwrap_or(0.1);
    let gap_factor = (old_diameter > 1.0e-12)
        .then_some(association.cross_gap / old_diameter)
        .unwrap_or(0.05);
    association.center = vector(center);
    association.radius = radius;
    association.plane_origin = vector(center);
    // The mark keeps its own rotation while its source stays in the same
    // plane; only a source turned out of that plane re-aligns it.
    let old_normal = dvec(association.plane_x)
        .cross(dvec(association.plane_y))
        .normalize_or_zero();
    if old_normal.dot(x.cross(y).normalize_or_zero()) < 1.0 - 1.0e-9 {
        association.plane_x = vector(x);
        association.plane_y = vector(y);
    }
    if association.cross_size_relative {
        association.cross_size = radius * 2.0 * size_factor;
    }
    if association.cross_gap_relative {
        association.cross_gap = radius * 2.0 * gap_factor;
    }
}

pub(crate) fn update_carrier(line: &mut codec::Line, association: &CenterMarkAssociation) {
    let segments = mark_segments(association);
    let horizontal = segments
        .first()
        .copied()
        .unwrap_or([dvec(association.center), dvec(association.center)]);
    line.start = vector(horizontal[0]);
    line.end = vector(horizontal[1]);
    association.write(&mut line.common.extended_data);
}

impl Scene {
    pub(crate) fn reassociate_center_mark(
        &mut self,
        target: Handle,
        source_handle: Handle,
        pick: DVec3,
    ) -> bool {
        let Some(source_entity) = self.document.get_entity(source_handle) else {
            return false;
        };
        let Some((source, center, radius, x, y)) =
            picked_mark_source(source_entity, source_handle, pick)
        else {
            return false;
        };
        let Some(EntityType::Line(target_line)) = self.document.get_entity(target) else {
            return false;
        };
        let Some(mut association) = CenterMarkAssociation::read(&target_line.common.extended_data)
        else {
            return false;
        };
        association.source = source;
        apply_source_geometry(&mut association, center, radius, x, y);
        association.associated = true;
        if self.is_recording_undo() {
            let before = self.document.get_entity_arc(target);
            self.record_undo_before(target, before);
        }
        let Some(EntityType::Line(line)) = self.document.get_entity_mut(target) else {
            return false;
        };
        update_carrier(line, &association);
        // The mark now points at `source_handle`; drop the indexed edge.
        self.associative_centermark_source_cache
            .borrow_mut()
            .take();
        self.bump_entities(&[(target, ChangeKind::Modified)]);
        true
    }

    /// Centre-mark carriers attached to the changed source handle, read from
    /// the reverse index (built on first use, dropped whenever an entity or an
    /// association may have changed). Candidates are re-validated against
    /// their current xdata, so a stale entry can only cost a lookup.
    fn associative_centermark_dependents(
        &self,
        changed: &rustc_hash::FxHashSet<Handle>,
    ) -> Vec<Handle> {
        if self
            .associative_centermark_source_cache
            .borrow()
            .is_none()
        {
            let mut index: HashMap<Handle, Vec<Handle>> = HashMap::default();
            for entity in self.document.entities() {
                let EntityType::Line(line) = entity else {
                    continue;
                };
                let Some(association) = CenterMarkAssociation::read(&line.common.extended_data)
                else {
                    continue;
                };
                if !association.associated {
                    continue;
                }
                let carriers = index.entry(association.source.handle).or_default();
                if !carriers.contains(&line.common.handle) {
                    carriers.push(line.common.handle);
                }
            }
            *self.associative_centermark_source_cache.borrow_mut() = Some(index);
        }
        let cache = self.associative_centermark_source_cache.borrow();
        let index = cache.as_ref().expect("associative centre mark index");
        let mut handles = rustc_hash::FxHashSet::default();
        for source in changed {
            if let Some(carriers) = index.get(source) {
                handles.extend(carriers.iter().copied());
            }
        }
        handles.into_iter().collect()
    }

    pub(crate) fn refresh_associative_center_marks(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) -> Vec<(Handle, ChangeKind)> {
        let changed: rustc_hash::FxHashSet<_> = changes.iter().map(|(handle, _)| *handle).collect();
        if changed.is_empty() {
            return Vec::new();
        }
        let candidates: Vec<_> = self
            .associative_centermark_dependents(&changed)
            .into_iter()
            .filter_map(|handle| {
                let EntityType::Line(line) = self.document.get_entity(handle)? else {
                    return None;
                };
                let association = CenterMarkAssociation::read(&line.common.extended_data)?;
                (association.associated && changed.contains(&association.source.handle))
                    .then_some((handle, association))
            })
            .collect();
        let mut result = Vec::new();
        for (handle, mut association) in candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(handle);
                self.record_undo_before(handle, before);
            }
            if let Some((center, radius, x, y)) =
                resolve_source(&self.document, &association.source)
            {
                apply_source_geometry(&mut association, center, radius, x, y);
            } else {
                association.associated = false;
            }
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(handle) {
                update_carrier(line, &association);
                result.push((handle, ChangeKind::Modified));
            }
        }
        result
    }

    pub(crate) fn reset_center_marks(&mut self, handles: &[Handle]) -> usize {
        let settings = self.centerline_settings();
        let candidates: Vec<_> = handles
            .iter()
            .filter_map(|handle| {
                let EntityType::Line(line) = self.document.get_entity(*handle)? else {
                    return None;
                };
                let mut association = CenterMarkAssociation::read(&line.common.extended_data)?;
                let diameter = association.radius * 2.0;
                association.cross_size =
                    super::centerline::resolve_center_measure(&settings.cross_size, diameter, 0.1);
                association.cross_gap =
                    super::centerline::resolve_center_measure(&settings.cross_gap, diameter, 0.05);
                association.cross_size_relative =
                    super::centerline::center_measure_is_relative(&settings.cross_size);
                association.cross_gap_relative =
                    super::centerline::center_measure_is_relative(&settings.cross_gap);
                association.extension_length = settings.extension;
                association.length_adjustments = [0.0; 4];
                association.overshoots = [0.0; 4];
                association.show_extensions = settings.mark_extensions;
                Some((*handle, association))
            })
            .collect();
        for (handle, association) in &candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(*handle);
                self.record_undo_before(*handle, before);
            }
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(*handle) {
                update_carrier(line, association);
            }
        }
        if !candidates.is_empty() {
            let changes: Vec<_> = candidates
                .iter()
                .map(|(handle, _)| (*handle, ChangeKind::Modified))
                .collect();
            self.bump_entities(&changes);
        }
        candidates.len()
    }

    pub(crate) fn set_center_mark_association(
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
                let mut association = CenterMarkAssociation::read(&line.common.extended_data)?;
                if associated {
                    let (center, radius, x, y) =
                        resolve_source(&self.document, &association.source)?;
                    apply_source_geometry(&mut association, center, radius, x, y);
                }
                association.associated = associated;
                Some((*handle, association))
            })
            .collect();
        for (handle, association) in &candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(*handle);
                self.record_undo_before(*handle, before);
            }
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(*handle) {
                update_carrier(line, association);
            }
        }
        if !candidates.is_empty() {
            // The association flags just changed. Drop the index; the bump
            // below re-indexes from the new xdata (an all-disassociated bump
            // rebuilds an empty index and refreshes nothing).
            self.associative_centermark_source_cache
                .borrow_mut()
                .take();
            let changes: Vec<_> = candidates
                .iter()
                .map(|(handle, _)| (*handle, ChangeKind::Modified))
                .collect();
            self.bump_entities(&changes);
        }
        candidates.len()
    }
}

/// Whether `entity` carries a center mark association.
///
/// Used to keep the drawing-wide scan off the edit path: only an entity that
/// actually carries one can make the scan necessary.
pub(crate) fn entity_has_center_association(entity: &EntityType) -> bool {
    let EntityType::Line(line) = entity else {
        return false;
    };
    CenterMarkAssociation::read(&line.common.extended_data)
        .is_some_and(|association| association.associated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{Circle, Line};

    fn source_circle() -> EntityType {
        EntityType::Circle(Circle::from_center_radius(
            Vector3::new(10.0, 10.0, 0.0),
            5.0,
        ))
    }

    fn carrier(source: Handle) -> EntityType {
        let association = CenterMarkAssociation {
            source: CenterMarkSource {
                handle: source,
                kind: CenterMarkSourceKind::Circle,
                segment_index: -1,
                pick_point: Vector3::new(10.0, 10.0, 0.0),
            },
            plane_origin: Vector3::new(10.0, 10.0, 0.0),
            plane_x: Vector3::new(1.0, 0.0, 0.0),
            plane_y: Vector3::new(0.0, 1.0, 0.0),
            center: Vector3::new(10.0, 10.0, 0.0),
            radius: 5.0,
            cross_size: 1.0,
            cross_gap: 0.5,
            cross_size_relative: false,
            cross_gap_relative: false,
            extension_length: 0.0,
            length_adjustments: [0.0; 4],
            overshoots: [0.0; 4],
            show_extensions: false,
            associated: true,
        };
        let mut line = Line::from_points(
            Vector3::new(9.5, 10.0, 0.0),
            Vector3::new(10.5, 10.0, 0.0),
        );
        association.write(&mut line.common.extended_data);
        EntityType::Line(line)
    }

    fn carrier_y(scene: &Scene, handle: Handle) -> f64 {
        let Some(EntityType::Line(line)) = scene.document.get_entity(handle) else {
            panic!("carrier is not a line");
        };
        assert!((line.start.y - line.end.y).abs() < 1e-9, "carrier not horizontal");
        line.start.y
    }

    #[test]
    fn index_finds_marks_when_their_circle_moves() {
        let mut scene = Scene::new();
        let source = scene.add_entity(source_circle());
        let mark = scene.add_entity(carrier(source));
        assert!((carrier_y(&scene, mark) - 10.0).abs() < 1e-9);

        let Some(EntityType::Circle(circle)) = scene.document.get_entity_mut(source) else {
            panic!("source is not a circle");
        };
        circle.center.y = 20.0;
        scene.bump_entities(&[(source, ChangeKind::Modified)]);
        assert!((carrier_y(&scene, mark) - 20.0).abs() < 1e-9);
    }

    /// Paste inserts straight into the document and bumps `Added`; a mark
    /// arriving that way must reach a warm index too.
    #[test]
    fn index_picks_up_a_mark_added_after_it_was_built() {
        let mut scene = Scene::new();
        let source = scene.add_entity(source_circle());
        let early = scene.add_entity(carrier(source));

        let Some(EntityType::Circle(circle)) = scene.document.get_entity_mut(source) else {
            panic!("source is not a circle");
        };
        circle.center.y = 12.0;
        scene.bump_entities(&[(source, ChangeKind::Modified)]);
        assert!(
            (carrier_y(&scene, early) - 12.0).abs() < 1e-9,
            "index is warm"
        );

        let pasted = scene
            .document
            .add_entity(carrier(source))
            .unwrap_or(Handle::NULL);
        assert!(!pasted.is_null());
        scene.bump_entities(&[(pasted, ChangeKind::Added)]);

        let Some(EntityType::Circle(circle)) = scene.document.get_entity_mut(source) else {
            panic!("source is not a circle");
        };
        circle.center.y = 20.0;
        scene.bump_entities(&[(source, ChangeKind::Modified)]);
        assert!((carrier_y(&scene, pasted) - 20.0).abs() < 1e-9);
        assert!((carrier_y(&scene, early) - 20.0).abs() < 1e-9);
    }
}
