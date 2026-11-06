//! Picking part of an object: Ctrl+click on a polyline selects the segment
//! under the cursor. The polyline joins the selection, only its picked
//! segments are highlighted, and MOVE / ROTATE / SCALE / MIRROR then change
//! just the vertices of those segments, stretching the neighbours.

use std::collections::BTreeSet;

use document::{EntityType, Handle};
use glam::DVec3;
use kernel::geom2d::{Curve, Polyline};
use kernel::space::PlanarCurve;

use super::Scene;
use crate::command::EntityTransform;

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
