//! Picking part of an object: Ctrl+click on a polyline selects the segment
//! under the cursor. The polyline joins the selection, only its picked
//! segments are highlighted, and MOVE / ROTATE / SCALE / MIRROR then change
//! just the vertices of those segments, stretching the neighbours.

use std::collections::BTreeSet;

use codec::entities::LwPolyline;
use codec::{EntityType, Handle};
use glam::DVec3;

use super::Scene;
use crate::command::EntityTransform;

/// Segment `index` of `polyline` on its own: its two vertices, with the
/// bulge of the first.
fn segment(polyline: &LwPolyline, index: usize) -> Option<LwPolyline> {
    let count = polyline.vertices.len();
    let next = if index + 1 < count {
        index + 1
    } else if polyline.is_closed && count > 2 {
        0
    } else {
        return None;
    };
    let mut part = polyline.clone();
    part.vertices = vec![
        polyline.vertices[index].clone(),
        polyline.vertices[next].clone(),
    ];
    part.vertices[1].bulge = 0.0;
    part.is_closed = false;
    Some(part)
}

fn segment_points(polyline: &LwPolyline, index: usize) -> Vec<[f64; 3]> {
    segment(polyline, index)
        .and_then(|part| crate::entities::curve::lwpolyline_curve(&part))
        .map(|curve| crate::entities::curve::curve_points(&curve))
        .unwrap_or_default()
}

impl Scene {
    /// Ctrl+click: toggle the segment of polyline `handle` nearest `point`
    /// and keep the polyline selected while any of its segments is. `false`
    /// when `handle` is not a polyline.
    pub fn toggle_sub_segment(&mut self, handle: Handle, point: DVec3) -> bool {
        let Some(EntityType::LwPolyline(polyline)) = self.document.get_entity(handle) else {
            return false;
        };
        let nearest = (0..polyline.vertices.len())
            .filter_map(|index| {
                let distance = segment_points(polyline, index)
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

    /// The picked segments of a selected polyline, if only part of it is.
    pub(crate) fn picked_segments(&self, handle: Handle) -> Option<&BTreeSet<usize>> {
        self.selected
            .contains(&handle)
            .then(|| self.sub_segments.get(&handle))
            .flatten()
    }

    /// World points of the picked segments of `handle`, NaN-separated, for
    /// the selection highlight.
    pub(crate) fn picked_segment_points(&self, handle: Handle) -> Option<Vec<[f64; 3]>> {
        let segments = self.picked_segments(handle)?;
        let Some(EntityType::LwPolyline(polyline)) = self.document.get_entity(handle) else {
            return None;
        };
        let mut points = Vec::new();
        for &index in segments {
            points.extend(segment_points(polyline, index));
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
            let (Some(segments), Some(EntityType::LwPolyline(polyline))) = (
                self.picked_segments(handle),
                self.document.get_entity(handle),
            ) else {
                whole.push(handle);
                continue;
            };
            let count = polyline.vertices.len();
            let vertices: BTreeSet<usize> = segments
                .iter()
                .flat_map(|&index| [index, (index + 1) % count])
                .collect();
            let Some(plane) =
                crate::entities::curve::lwpolyline_curve(polyline).map(|curve| curve.plane)
            else {
                whole.push(handle);
                continue;
            };
            let mut moved = polyline.clone();
            for index in vertices {
                let vertex = &mut moved.vertices[index];
                let world = plane.point_at([vertex.location.x, vertex.location.y]);
                let mut marker = EntityType::Point(codec::entities::Point::at(
                    codec::types::Vector3::new(world[0], world[1], world[2]),
                ));
                super::view::dispatch::apply_transform(&mut marker, transform);
                let EntityType::Point(marker) = marker else {
                    continue;
                };
                let location = marker.location;
                if let Some([x, y]) = plane.project([location.x, location.y, location.z]) {
                    vertex.location.x = x;
                    vertex.location.y = y;
                }
            }
            self.update_entity(EntityType::LwPolyline(moved));
        }
        whole
    }
}
