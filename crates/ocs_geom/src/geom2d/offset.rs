//! Parallel offset of a polyline.
//!
//! Offsetting is the one operation here that leans on an outside
//! implementation: `cavalier_contours` already solves the hard part, which is
//! not moving the segments but deciding which of the resulting pieces survive
//! once the offset self-intersects. This module owns the conditioning around
//! it, the join style, and the conversion at the boundary.
//!
//! Behind the `offset` feature, so a caller that only wants intersection or
//! tessellation does not pull the dependency in.

use cavalier_contours::core::math::Vector2 as CavVector2;
use cavalier_contours::polyline::internal::pline_offset::{
    create_raw_offset_polyline, slices_from_dual_raw_offsets, stitch_slices_together,
};
use cavalier_contours::polyline::{
    seg_tangent_vector, PlineOffsetOptions, PlineSource, PlineSourceMut, Polyline as CavPolyline,
};

use super::intersect::line_line;
use super::polyline::{BulgeArc, Polyline, PolylineVertex};
use super::vec::Vec2;

/// Positions closer than this are the same point.
///
/// Applied in normalised space, where the offset distance is exactly 1, so it
/// reads as a fraction of the offset rather than a length in the drawing.
const POSITION_EPSILON: f64 = 1e-5;

/// Slack for joining slices and for recognising a generated join arc.
const JOIN_EPSILON: f64 = 1e-4;

/// Offsets `source` by `distance`, toward whichever side `side` lies on.
///
/// Returns every surviving loop: an offset that folds through itself comes
/// back as several pieces, and a source that collapses entirely comes back
/// empty. `distance` is taken as a magnitude; the side decides the direction.
///
/// # Conditioning
///
/// The work happens with the source translated to its first vertex and scaled
/// so the offset distance is exactly 1. Both matter. The translation is the
/// usual survey-coordinate problem — see [`Frame`](super::Frame). The scale
/// matters because the tolerances the offset algorithm needs are relative to
/// the offset, not to the drawing: a 0.1 mm offset and a 10 km one otherwise
/// need different epsilons to behave the same way.
pub fn offset_polyline(source: &Polyline, distance: f64, side: [f64; 2]) -> Vec<Polyline> {
    offset_polyline_with_gap(source, distance, side, OffsetGap::Extend)
}

/// How an offset closes the gap that opens at a convex corner between two
/// straight segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OffsetGap {
    /// Extend both segments to the point where they meet.
    #[default]
    Extend,
    /// Join them with an arc of the offset radius, centred on the corner.
    Fillet,
    /// Join them with the straight line tangent to that arc at its midpoint.
    Chamfer,
}

/// [`offset_polyline`] with a chosen way of closing the corner gaps.
pub fn offset_polyline_with_gap(
    source: &Polyline,
    distance: f64,
    side: [f64; 2],
    gap: OffsetGap,
) -> Vec<Polyline> {
    let distance = distance.abs();
    if distance < 1e-12 {
        return Vec::new();
    }
    let Some((normalised, origin)) = normalise(source, distance) else {
        return Vec::new();
    };

    let side = CavVector2::new(
        (side[0] - origin[0]) / distance,
        (side[1] - origin[1]) / distance,
    );
    let Some(closest) = normalised.closest_point(side, POSITION_EPSILON) else {
        return Vec::new();
    };

    // Which side the pick sits on: the sign of the cross product between the
    // polyline's direction there and the direction out to the pick.
    let start_index = closest.seg_start_index;
    let tangent = seg_tangent_vector(
        normalised.at(start_index),
        normalised.at(normalised.next_wrapping_index(start_index)),
        closest.seg_point,
    );
    let toward_pick = side - closest.seg_point;
    let cross = tangent.x * toward_pick.y - tangent.y * toward_pick.x;
    let signed_offset = if cross >= 0.0 { 1.0 } else { -1.0 };

    parallel_offset(&normalised, signed_offset, gap)
        .into_iter()
        .filter(|result| result.vertex_count() >= 2)
        .map(|result| Polyline {
            closed: result.is_closed(),
            vertices: result
                .iter_vertexes()
                .map(|vertex| PolylineVertex {
                    position: [
                        origin[0] + vertex.x * distance,
                        origin[1] + vertex.y * distance,
                    ],
                    bulge: vertex.bulge,
                })
                .collect(),
        })
        .collect()
}

/// Builds the offset input: translated to the first vertex, scaled so the
/// offset distance is 1, and with over-half-turn arcs split.
fn normalise(source: &Polyline, distance: f64) -> Option<(CavPolyline<f64>, [f64; 2])> {
    let first = source.vertices.first()?;
    let origin = first.position;
    let to_local = |point: [f64; 2]| {
        [
            (point[0] - origin[0]) / distance,
            (point[1] - origin[1]) / distance,
        ]
    };

    let count = source.vertices.len();
    if count < 2 {
        return None;
    }
    let segment_count = if source.closed { count } else { count - 1 };
    let mut out = if source.closed {
        CavPolyline::new_closed()
    } else {
        CavPolyline::new()
    };

    for index in 0..segment_count {
        let start = source.vertices[index];
        let end = source.vertices[(index + 1) % count];
        let bulge = if start.bulge.is_finite() {
            start.bulge
        } else {
            0.0
        };
        let local = to_local(start.position);

        // CavalierContours carries at most a half turn per segment. Split a
        // major arc at its own midpoint; both halves keep the original circle
        // and direction of travel.
        if bulge.abs() > 1.0 {
            if let Some(arc) = BulgeArc::from_bulge(start.position, end.position, bulge) {
                let half_bulge = (arc.sweep / 8.0).tan();
                let midpoint = to_local(arc.sample(0.5));
                out.add(local[0], local[1], half_bulge);
                out.add(midpoint[0], midpoint[1], half_bulge);
                continue;
            }
        }

        out.add(local[0], local[1], bulge);
    }

    if !source.closed {
        let last = to_local(source.vertices[count - 1].position);
        out.add(last[0], last[1], 0.0);
    }

    let out = out.remove_repeat_pos(POSITION_EPSILON).unwrap_or(out);
    (out.vertex_count() >= 2).then_some((out, origin))
}

/// The offset proper: raw offsets both ways, sharpened joins, then the slice
/// pass that discards the parts the offset folded over.
fn parallel_offset(
    source: &CavPolyline<f64>,
    signed_offset: f64,
    gap: OffsetGap,
) -> Vec<CavPolyline<f64>> {
    let options = PlineOffsetOptions {
        handle_self_intersects: true,
        pos_equal_eps: POSITION_EPSILON,
        slice_join_eps: JOIN_EPSILON,
        offset_dist_eps: JOIN_EPSILON,
        ..Default::default()
    };
    let index = source.create_approx_aabb_index();
    let mut raw: CavPolyline<f64> = create_raw_offset_polyline(source, signed_offset, POSITION_EPSILON);
    if raw.is_empty() {
        return Vec::new();
    }
    let mut dual: CavPolyline<f64> =
        create_raw_offset_polyline(source, -signed_offset, POSITION_EPSILON);
    match gap {
        OffsetGap::Extend => {
            sharpen_joins(&mut raw, source);
            sharpen_joins(&mut dual, source);
        }
        // The raw offset already joins with arcs of the offset radius.
        OffsetGap::Fillet => {}
        OffsetGap::Chamfer => {
            chamfer_joins(&mut raw, source);
            chamfer_joins(&mut dual, source);
        }
    }

    let slices = slices_from_dual_raw_offsets(source, &raw, &dual, &index, signed_offset, &options);
    stitch_slices_together::<_, f64, CavPolyline<f64>>(
        &raw,
        &slices,
        source.is_closed(),
        raw.vertex_count(),
        &options,
    )
}

/// Replaces the round joins the offset generates between diverging straight
/// segments with the sharp corner where those segments would actually meet.
///
/// `cavalier_contours` connects them with an arc of the offset radius. CAD
/// convention extends the two lines to their projected intersection instead,
/// so this rewrites exactly those arcs — identified by having the offset
/// radius and being centred on a vertex of the source — and leaves every other
/// arc alone. Runs before the self-intersection pass, so the sharpened corners
/// take part in it.
fn sharpen_joins(raw: &mut CavPolyline<f64>, source: &CavPolyline<f64>) {
    loop {
        let count = raw.vertex_data.len();
        if count < 4 {
            return;
        }

        let mut changed = false;
        for index in 0..count {
            if !raw.is_closed && index == 0 {
                continue;
            }
            let Some(next) = step(index, count, raw.is_closed) else {
                continue;
            };
            let Some(after) = step(next, count, raw.is_closed) else {
                continue;
            };
            let previous = if index > 0 {
                index - 1
            } else if raw.is_closed {
                count - 1
            } else {
                continue;
            };

            let arc_start = raw.vertex_data[index];
            let arc_end = raw.vertex_data[next];
            // Only a lone arc between two straight segments can be a join.
            if arc_start.bulge.abs() < POSITION_EPSILON
                || raw.vertex_data[previous].bulge.abs() >= POSITION_EPSILON
                || arc_end.bulge.abs() >= POSITION_EPSILON
            {
                continue;
            }

            let Some(connection) = BulgeArc::from_bulge(
                [arc_start.x, arc_start.y],
                [arc_end.x, arc_end.y],
                arc_start.bulge,
            ) else {
                continue;
            };
            // In normalised space the offset radius is exactly 1.
            if (connection.radius - 1.0).abs() > JOIN_EPSILON {
                continue;
            }
            let centre = Vec2::from(connection.center);
            let sits_on_a_source_vertex = source.iter_vertexes().any(|vertex| {
                Vec2::new(vertex.x, vertex.y).distance_squared(centre)
                    <= JOIN_EPSILON * JOIN_EPSILON
            });
            if !sits_on_a_source_vertex {
                continue;
            }

            let before = raw.vertex_data[previous];
            let after_vertex = raw.vertex_data[after];
            let Some(corner) = segment_crossing(
                [before.x, before.y],
                [arc_start.x, arc_start.y],
                [arc_end.x, arc_end.y],
                [after_vertex.x, after_vertex.y],
            ) else {
                continue;
            };

            raw.vertex_data[index].x = corner[0];
            raw.vertex_data[index].y = corner[1];
            raw.vertex_data[index].bulge = 0.0;
            raw.vertex_data.remove(next);
            changed = true;
            break;
        }

        if !changed {
            return;
        }
    }
}

/// Replaces each join arc (see [`sharpen_joins`]) with the straight line
/// tangent to it at its midpoint: both neighbouring segments are extended by
/// `tan(sweep / 4)` of the offset radius and meet that line there.
fn chamfer_joins(raw: &mut CavPolyline<f64>, source: &CavPolyline<f64>) {
    let count = raw.vertex_data.len();
    if count < 3 {
        return;
    }
    for index in 0..count {
        let previous = if index > 0 {
            index - 1
        } else if raw.is_closed {
            count - 1
        } else {
            continue;
        };
        let Some(next) = step(index, count, raw.is_closed) else {
            continue;
        };
        let Some(after) = step(next, count, raw.is_closed) else {
            continue;
        };
        let arc_start = raw.vertex_data[index];
        let arc_end = raw.vertex_data[next];
        if arc_start.bulge.abs() < POSITION_EPSILON
            || raw.vertex_data[previous].bulge.abs() >= POSITION_EPSILON
            || arc_end.bulge.abs() >= POSITION_EPSILON
        {
            continue;
        }
        let Some(connection) = BulgeArc::from_bulge(
            [arc_start.x, arc_start.y],
            [arc_end.x, arc_end.y],
            arc_start.bulge,
        ) else {
            continue;
        };
        if (connection.radius - 1.0).abs() > JOIN_EPSILON {
            continue;
        }
        let centre = Vec2::from(connection.center);
        if !source.iter_vertexes().any(|vertex| {
            Vec2::new(vertex.x, vertex.y).distance_squared(centre) <= JOIN_EPSILON * JOIN_EPSILON
        }) {
            continue;
        }
        let before = raw.vertex_data[previous];
        let after_vertex = raw.vertex_data[after];
        let incoming = Vec2::new(arc_start.x - before.x, arc_start.y - before.y).normalize();
        let outgoing = Vec2::new(after_vertex.x - arc_end.x, after_vertex.y - arc_end.y).normalize();
        let (Some(incoming), Some(outgoing)) = (incoming, outgoing) else {
            continue;
        };
        let reach = (connection.sweep.abs() / 4.0).tan();
        raw.vertex_data[index].x = arc_start.x + incoming.x * reach;
        raw.vertex_data[index].y = arc_start.y + incoming.y * reach;
        raw.vertex_data[index].bulge = 0.0;
        raw.vertex_data[next].x = arc_end.x - outgoing.x * reach;
        raw.vertex_data[next].y = arc_end.y - outgoing.y * reach;
    }
}

/// The next index, wrapping only on a closed polyline.
fn step(index: usize, count: usize, closed: bool) -> Option<usize> {
    if index + 1 < count {
        Some(index + 1)
    } else if closed {
        Some(0)
    } else {
        None
    }
}

/// Where the lines through `p0→p1` and `q0→q1` cross, as a point.
fn segment_crossing(
    p0: [f64; 2],
    p1: [f64; 2],
    q0: [f64; 2],
    q1: [f64; 2],
) -> Option<[f64; 2]> {
    let d = [p1[0] - p0[0], p1[1] - p0[1]];
    let e = [q1[0] - q0[0], q1[1] - q0[1]];
    let (t, _) = line_line(p0, d, q0, e)?;
    Some([p0[0] + t * d[0], p0[1] + t * d[1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(width: f64, height: f64) -> Polyline {
        Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([width, 0.0]),
                PolylineVertex::straight([width, height]),
                PolylineVertex::straight([0.0, height]),
            ],
            closed: true,
        }
    }

    #[test]
    fn gap_types_close_a_square_corner_as_measured() {
        let square = rectangle(100.0, 100.0);
        let fillet = offset_polyline_with_gap(&square, 10.0, [50.0, -50.0], OffsetGap::Fillet);
        assert_eq!(fillet[0].vertices.len(), 8);
        assert!(fillet[0].vertices.iter().any(|v| (v.bulge.abs() - 0.414214).abs() < 1e-5));
        let chamfer = offset_polyline_with_gap(&square, 10.0, [50.0, -50.0], OffsetGap::Chamfer);
        assert_eq!(chamfer[0].vertices.len(), 8);
        assert!(chamfer[0].vertices.iter().all(|v| v.bulge == 0.0));
        assert!(chamfer[0].vertices.iter().any(|v| {
            (v.position[0] - 104.142136).abs() < 1e-5 && (v.position[1] + 10.0).abs() < 1e-9
        }));
    }

    #[test]
    fn offsetting_outward_grows_the_loop() {
        let out = offset_polyline(&rectangle(10.0, 6.0), 1.0, [-5.0, 3.0]);
        assert_eq!(out.len(), 1);
        let xs: Vec<f64> = out[0].vertices.iter().map(|v| v.position[0]).collect();
        let min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(min < -0.5, "outward offset should pass x = 0, got {min}");
    }

    #[test]
    fn offsetting_inward_shrinks_the_loop() {
        let out = offset_polyline(&rectangle(10.0, 6.0), 1.0, [5.0, 3.0]);
        assert_eq!(out.len(), 1);
        let xs: Vec<f64> = out[0].vertices.iter().map(|v| v.position[0]).collect();
        let min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(min > 0.5, "inward offset should stay past x = 0, got {min}");
    }

    #[test]
    fn a_closed_source_offsets_to_a_closed_result() {
        let out = offset_polyline(&rectangle(10.0, 6.0), 1.0, [5.0, 3.0]);
        assert!(out[0].closed);
    }

    #[test]
    fn corners_stay_sharp_rather_than_rounding() {
        // A rounded join would leave a bulge on the corner vertices.
        let out = offset_polyline(&rectangle(10.0, 6.0), 1.0, [-5.0, 3.0]);
        assert!(
            out[0].vertices.iter().all(|v| v.bulge.abs() < 1e-6),
            "expected sharp corners, found a bulge: {:?}",
            out[0].vertices
        );
    }

    #[test]
    fn an_inward_offset_wider_than_the_shape_collapses() {
        // Offsetting a 10x6 rectangle 5 inward leaves nothing coherent.
        let out = offset_polyline(&rectangle(10.0, 6.0), 5.0, [5.0, 3.0]);
        assert!(out.is_empty(), "expected collapse, got {out:?}");
    }

    #[test]
    fn an_open_polyline_offsets_without_closing() {
        let source = Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([10.0, 0.0]),
                PolylineVertex::straight([10.0, 6.0]),
            ],
            closed: false,
        };
        let out = offset_polyline(&source, 1.0, [5.0, 1.0]);
        assert_eq!(out.len(), 1);
        assert!(!out[0].closed);
    }

    #[test]
    fn a_zero_distance_offset_does_nothing() {
        assert!(offset_polyline(&rectangle(10.0, 6.0), 0.0, [5.0, 3.0]).is_empty());
    }

    #[test]
    fn too_few_vertices_offset_to_nothing() {
        let lone = Polyline {
            vertices: vec![PolylineVertex::straight([0.0, 0.0])],
            closed: false,
        };
        assert!(offset_polyline(&lone, 1.0, [1.0, 1.0]).is_empty());
    }

    #[test]
    fn survey_coordinates_offset_by_the_distance_asked_for() {
        // The conditioning claim, exercised: same rectangle at a UTM origin.
        let origin = [512_345.678, 4_512_345.678];
        let source = Polyline {
            vertices: vec![
                PolylineVertex::straight(origin),
                PolylineVertex::straight([origin[0] + 10.0, origin[1]]),
                PolylineVertex::straight([origin[0] + 10.0, origin[1] + 6.0]),
                PolylineVertex::straight([origin[0], origin[1] + 6.0]),
            ],
            closed: true,
        };
        let out = offset_polyline(&source, 1.0, [origin[0] + 5.0, origin[1] + 3.0]);
        assert_eq!(out.len(), 1);
        let xs: Vec<f64> = out[0].vertices.iter().map(|v| v.position[0]).collect();
        let min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(
            (min - (origin[0] + 1.0)).abs() < 1e-6,
            "inward edge should sit 1 from the source, got {}",
            min - origin[0]
        );
    }

    #[test]
    fn an_arc_segment_survives_the_round_trip() {
        let source = Polyline {
            vertices: vec![
                PolylineVertex::curved([0.0, 0.0], 1.0),
                PolylineVertex::straight([4.0, 0.0]),
            ],
            closed: true,
        };
        let out = offset_polyline(&source, 0.5, [2.0, 0.1]);
        assert!(!out.is_empty());
        assert!(
            out.iter().any(|p| p.vertices.iter().any(|v| v.bulge.abs() > 1e-6)),
            "the arc should still be an arc after offsetting"
        );
    }
}
