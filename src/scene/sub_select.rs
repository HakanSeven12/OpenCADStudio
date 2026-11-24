//! Picking part of an object: Ctrl+click on a polyline selects the segment
//! under the cursor. The polyline joins the selection, only its picked
//! segments are highlighted, and MOVE / ROTATE / SCALE / MIRROR then change
//! just the vertices of those segments, stretching the neighbours.

use std::collections::BTreeSet;

use document::{EntityType, Handle};
use glam::DVec3;
use ocs_geom::geom2d::{Curve, Polyline};
use ocs_geom::space::PlanarCurve;

use super::Scene;
use crate::entities::transform::EntityTransform;

/// The polyline whose segments a pick indexes, with the entity vertex each
/// of its vertices is: an LWPOLYLINE, or a heavy 2D POLYLINE along the curve
/// it draws (a fit polyline's generated vertices, never its frame).
fn plain_polyline(entity: &EntityType) -> Option<(PlanarCurve, Vec<usize>)> {
    match entity {
        EntityType::LwPolyline(polyline) => crate::entities::curve::lwpolyline_curve(polyline)
            .map(|curve| (curve, (0..polyline.vertices.len()).collect())),
        EntityType::Polyline2D(polyline) => {
            let drawn = (0..polyline.vertices.len())
                .filter(|&i| polyline.vertices[i].flags.bits() & 16 == 0)
                .collect();
            crate::entities::curve::polyline2d_curve(polyline).map(|curve| (curve, drawn))
        }
        _ => None,
    }
}

fn vertex_count(curve: &PlanarCurve) -> usize {
    match &curve.curve {
        Curve::Polyline(polyline) => polyline.vertices.len(),
        _ => 0,
    }
}

/// Points of segment `index` of `curve` on its own: its two vertices, with
/// the bulge of the first.
fn segment_points(curve: &PlanarCurve, index: usize) -> Vec<[f64; 3]> {
    let Curve::Polyline(polyline) = &curve.curve else {
        return Vec::new();
    };
    let count = polyline.vertices.len();
    let next = if index + 1 < count {
        index + 1
    } else if polyline.closed && count > 2 {
        0
    } else {
        return Vec::new();
    };
    let mut second = polyline.vertices[next];
    second.bulge = 0.0;
    let part = PlanarCurve::new(
        curve.plane,
        Curve::Polyline(Polyline {
            vertices: vec![polyline.vertices[index], second],
            closed: false,
        }),
    );
    crate::entities::curve::curve_points(&part)
}

/// Whether segments `a`-`b` and `c`-`d` cross or touch.
fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let side = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let (d1, d2) = (side(c, d, a), side(c, d, b));
    let (d3, d4) = (side(a, b, c), side(a, b, d));
    d1 * d2 <= 0.0 && d3 * d4 <= 0.0
}

impl Scene {
    /// Ctrl+click: toggle the segment of polyline `handle` nearest `point`
    /// and keep the polyline selected while any of its segments is. `false`
    /// when `handle` is not a polyline.
    pub fn toggle_sub_segment(&mut self, handle: Handle, point: DVec3) -> bool {
        let Some((curve, _)) = self.document.get_entity(handle).and_then(plain_polyline) else {
            return false;
        };
        let nearest = (0..vertex_count(&curve))
            .filter_map(|index| {
                let distance = segment_points(&curve, index)
                    .windows(2)
                    .map(|pair| {
                        let a = DVec3::from_array(pair[0]).truncate();
                        let b = DVec3::from_array(pair[1]).truncate();
                        let p = point.truncate();
                        let t = ((p - a).dot(b - a) / (b - a).length_squared().max(1e-300))
                            .clamp(0.0, 1.0);
                        p.distance(a + (b - a) * t)
                    })
                    .fold(f64::INFINITY, f64::min);
                distance.is_finite().then_some((index, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((index, _)) = nearest else {
            return false;
        };
        if !self.selected.contains(&handle) {
            self.sub_segments.remove(&handle);
        }
        let segments = self.sub_segments.entry(handle).or_default();
        if !segments.remove(&index) {
            segments.insert(index);
        }
        if segments.is_empty() {
            self.sub_segments.remove(&handle);
            self.deselect_entity(handle);
        } else {
            self.select_entity(handle, false);
        }
        self.bump_selection_set();
        true
    }

    /// Ctrl+window: the segments of polyline `handle` a selection box
    /// catches join its picked segments, as a Ctrl+click picks one. A
    /// segment is caught when it lies wholly inside the box (`min`..`max`,
    /// in the space `to_screen` maps to) or, for a crossing box, when it
    /// also just touches it. `false` when none was caught.
    pub fn add_box_sub_segments(
        &mut self,
        handle: Handle,
        to_screen: impl Fn(DVec3) -> [f64; 2],
        min: [f64; 2],
        max: [f64; 2],
        crossing: bool,
    ) -> bool {
        let Some((curve, _)) = self.document.get_entity(handle).and_then(plain_polyline) else {
            return false;
        };
        let inside =
            |p: [f64; 2]| p[0] >= min[0] && p[0] <= max[0] && p[1] >= min[1] && p[1] <= max[1];
        let corners = [min, [max[0], min[1]], max, [min[0], max[1]]];
        let crosses_box = |a: [f64; 2], b: [f64; 2]| {
            (0..4).any(|k| segments_cross(a, b, corners[k], corners[(k + 1) % 4]))
        };
        let caught: Vec<usize> = (0..vertex_count(&curve))
            .filter(|&index| {
                let points: Vec<[f64; 2]> = segment_points(&curve, index)
                    .into_iter()
                    .map(|p| to_screen(DVec3::from_array(p)))
                    .collect();
                if points.is_empty() {
                    return false;
                }
                if points.iter().all(|p| inside(*p)) {
                    return true;
                }
                crossing
                    && (points.iter().any(|p| inside(*p))
                        || points.windows(2).any(|pair| crosses_box(pair[0], pair[1])))
            })
            .collect();
        if caught.is_empty() {
            return false;
        }
        if !self.selected.contains(&handle) {
            self.sub_segments.remove(&handle);
        }
        self.sub_segments.entry(handle).or_default().extend(caught);
        self.select_entity(handle, false);
        self.bump_selection_set();
        true
    }

    /// The picked segments of a selected polyline, if only part of it is.
    pub(crate) fn picked_segments(&self, handle: Handle) -> Option<&BTreeSet<usize>> {
        self.selected
            .contains(&handle)
            .then(|| self.sub_segments.get(&handle))
            .flatten()
    }

    /// What is left of LWPOLYLINE `handle` once its picked segments are
    /// erased (#3552): one open polyline per run of unpicked segments, none
    /// when every segment was picked. `None` when `handle` is not a
    /// lightweight polyline with picked segments, so it erases whole.
    // ponytail: heavy 2D polylines still erase whole; map `plain_polyline`'s
    // drawn vertices back to theirs if that is asked for.
    pub(crate) fn polyline_without_picked_segments(
        &self,
        handle: Handle,
    ) -> Option<Vec<EntityType>> {
        let picked = self.picked_segments(handle)?;
        let Some(EntityType::LwPolyline(polyline)) = self.document.get_entity(handle) else {
            return None;
        };
        let n = polyline.vertices.len();
        let segments = if polyline.is_closed { n } else { n.saturating_sub(1) };
        // A closed polyline is walked from just after a picked segment, so
        // no run wraps past the start.
        let start = if polyline.is_closed {
            picked.iter().next().map_or(0, |&first| first + 1)
        } else {
            0
        };
        let mut pieces = Vec::new();
        let mut run: Vec<usize> = Vec::new();
        let mut flush = |run: &mut Vec<usize>| {
            let Some(&last) = run.last() else {
                return;
            };
            let mut piece = polyline.clone();
            piece.is_closed = false;
            piece.vertices = run.iter().map(|&k| polyline.vertices[k % n].clone()).collect();
            let mut end = polyline.vertices[(last + 1) % n].clone();
            end.bulge = 0.0;
            piece.vertices.push(end);
            pieces.push(EntityType::LwPolyline(piece));
            run.clear();
        };
        for step in 0..segments {
            let k = (start + step) % segments.max(1);
            if picked.contains(&k) {
                flush(&mut run);
            } else {
                run.push(k);
            }
        }
        flush(&mut run);
        Some(pieces)
    }

    /// World points of the picked segments of `handle`, NaN-separated, for
    /// the selection highlight.
    pub(crate) fn picked_segment_points(&self, handle: Handle) -> Option<Vec<[f64; 3]>> {
        let segments = self.picked_segments(handle)?;
        let (curve, _) = self.document.get_entity(handle).and_then(plain_polyline)?;
        let mut points = Vec::new();
        for &index in segments {
            points.extend(segment_points(&curve, index));
            points.push([f64::NAN; 3]);
        }
        Some(points)
    }

    /// Apply `transform` to just the vertices of the picked segments of each
    /// partly selected polyline in `handles`. Returns the handles left to
    /// transform whole.
    pub fn transform_picked_segments(
        &mut self,
        handles: &[Handle],
        transform: &EntityTransform,
    ) -> Vec<Handle> {
        let mut whole = Vec::with_capacity(handles.len());
        for &handle in handles {
            let (Some(segments), Some((curve, entity_index))) = (
                self.picked_segments(handle),
                self.document.get_entity(handle).and_then(plain_polyline),
            ) else {
                whole.push(handle);
                continue;
            };
            let Curve::Polyline(polyline) = &curve.curve else {
                whole.push(handle);
                continue;
            };
            let count = polyline.vertices.len();
            let vertices: BTreeSet<usize> = segments
                .iter()
                .flat_map(|&index| [index, (index + 1) % count])
                .collect();
            let mut moved_to = Vec::with_capacity(vertices.len());
            for index in vertices {
                let world = curve.plane.point_at(polyline.vertices[index].position);
                let mut marker = EntityType::Point(document::entities::Point::at(
                    document::types::Vector3::new(world[0], world[1], world[2]),
                ));
                super::view::dispatch::apply_transform(&mut marker, transform);
                let EntityType::Point(marker) = marker else {
                    continue;
                };
                let location = marker.location;
                if let Some(at) = curve.plane.project([location.x, location.y, location.z]) {
                    moved_to.push((entity_index[index], at));
                }
            }
            let Some(mut moved) = self.document.get_entity(handle).cloned() else {
                continue;
            };
            for (index, [x, y]) in moved_to {
                match &mut moved {
                    EntityType::LwPolyline(polyline) => {
                        let location = &mut polyline.vertices[index].location;
                        (location.x, location.y) = (x, y);
                    }
                    EntityType::Polyline2D(polyline) => {
                        let location = &mut polyline.vertices[index].location;
                        (location.x, location.y) = (x, y);
                    }
                    _ => {}
                }
            }
            self.update_entity(moved);
        }
        whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::{LwPolyline, LwVertex};

    /// Ctrl+window (#748): a window takes the segments wholly inside it, a
    /// crossing box also those it touches; the polyline is selected with
    /// only them picked.
    #[test]
    fn a_box_picks_the_segments_it_catches() {
        let polyline = || {
            let mut p = LwPolyline::new();
            p.vertices = vec![
                LwVertex::from_coords(0.0, 0.0),
                LwVertex::from_coords(10.0, 0.0),
                LwVertex::from_coords(20.0, 0.0),
                LwVertex::from_coords(30.0, 0.0),
            ];
            EntityType::LwPolyline(p)
        };
        let to_screen = |p: DVec3| [p.x, p.y];

        let mut scene = Scene::new();
        let handle = scene.add_entity(polyline());
        assert!(scene.add_box_sub_segments(handle, to_screen, [-1.0, -1.0], [21.0, 1.0], false));
        let picked: Vec<usize> = scene.picked_segments(handle).unwrap().iter().copied().collect();
        assert_eq!(picked, [0, 1], "a window takes the segments wholly inside");

        let mut scene = Scene::new();
        let handle = scene.add_entity(polyline());
        assert!(scene.add_box_sub_segments(handle, to_screen, [15.0, -1.0], [25.0, 1.0], true));
        let picked: Vec<usize> = scene.picked_segments(handle).unwrap().iter().copied().collect();
        assert_eq!(picked, [1, 2], "a crossing box also takes the segments it touches");

        let mut scene = Scene::new();
        let handle = scene.add_entity(polyline());
        assert!(!scene.add_box_sub_segments(handle, to_screen, [15.0, -1.0], [25.0, 1.0], false));
        assert!(!scene.selected.contains(&handle));
    }
}
