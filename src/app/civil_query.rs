//! Read-only civil query ops: station-offset points on alignments and TIN
//! surface statistics.
//!
//! Both ops answer from the already-decoded civil records (`ArchEntityData`)
//! without touching the document: a station-offset read solves the alignment's
//! stored design elements into a point the way Civil 3D's `GetPointAtStation`
//! does, and a surface read tallies the triangulation the record keeps. No op
//! here writes, selects, or rebuilds anything.

use document::classes::arch::ArchEntityData;
use document::classes::civil::{AlignmentPiece, StationEquation, TinSurface};
use document::entities::{EntityType, ExtendedEntityData};
use document::types::Vector2;
use document::Handle;
use serde_json::{json, Value};

/// Chords shorter than this carry no geometry (the solver's dedup epsilon).
const EPS: f64 = 1e-9;

/// The station length one chord of a solved piece spans: a curve's chord is
/// measured along the arc it cuts off its piece's solved curvature.
fn span_length(kind: &AlignmentPiece, curvature: &f64, a: Vector2, b: Vector2) -> f64 {
    let c = a.distance(&b);
    if *kind == AlignmentPiece::Curve && curvature.abs() > EPS {
        let radius = 1.0 / curvature.abs();
        2.0 * radius * (c / (2.0 * radius)).min(1.0).asin()
    } else {
        c
    }
}

/// The station length of a whole solved piece.
fn piece_length(piece: &(AlignmentPiece, f64, Vec<Vector2>)) -> f64 {
    let (kind, curvature, points) = piece;
    points.windows(2).map(|w| span_length(kind, curvature, w[0], w[1])).sum()
}

fn err(msg: impl std::fmt::Display) -> Value {
    json!({ "ok": false, "error": msg.to_string() })
}

/// The civil record of `handle`, when it is an Extended entity of `dxf_name`.
fn arch_named<'a>(
    document: &'a document::Document,
    handle: Handle,
    dxf_name: &str,
) -> Option<&'a ArchEntityData> {
    let EntityType::Extended(entity) = document.get_entity(handle)? else {
        return None;
    };
    let ExtendedEntityData::Arch(data) = &entity.data else {
        return None;
    };
    (data.dxf_name == dxf_name).then_some(data)
}

/// The raw (geometry) distance a displayed station names. `display_station`
/// runs stations past an equation on from its station ahead; this inverts it:
/// the equation whose ahead station the shown station has passed converts
/// back to its raw anchor, the same way the drawing's station labels do.
fn raw_station(equations: &[StationEquation], shown: f64) -> f64 {
    let mut raw = shown;
    for equation in equations {
        let passed = if equation.increasing {
            shown >= equation.ahead
        } else {
            shown <= equation.ahead
        };
        if passed {
            raw = if equation.increasing {
                equation.raw + (shown - equation.ahead)
            } else {
                equation.raw + (equation.ahead - shown)
            };
        }
    }
    raw
}

/// Where `distance` from the first solved run's start sits: the point and its
/// unit direction of travel.
///
/// A curve's chords are measured along its arc at the curvature the codec
/// solved the element with (the way the codec's own station runs do), so a
/// station picks the same point whether it names a line, curve or spiral run.
pub(crate) fn point_at_distance(
    pieces: &[(AlignmentPiece, f64, Vec<Vector2>)],
    distance: f64,
) -> Option<(Vector2, [f64; 2])> {
    let mut start = 0.0;
    for (kind, curvature, points) in pieces {
        let lengths: Vec<f64> = points
            .windows(2)
            .map(|w| span_length(kind, curvature, w[0], w[1]))
            .collect();
        let total: f64 = lengths.iter().sum();
        // A hair past the end (rounding) still reads as the piece's end.
        if distance > start + total + EPS {
            start += total;
            continue;
        }
        let local = (distance - start).clamp(0.0, total);
        let mut walked = 0.0;
        for (index, length) in lengths.iter().enumerate() {
            if local > walked + *length + EPS {
                walked += *length;
                continue;
            }
            let (a, b) = (points[index], points[index + 1]);
            let c = a.distance(&b);
            if c < EPS || *length < EPS {
                continue;
            }
            let f = ((local - walked) / *length).clamp(0.0, 1.0);
            let ex = [(b.x - a.x) / c, (b.y - a.y) / c];
            if *kind == AlignmentPiece::Curve && curvature.abs() > EPS {
                // The arc through the chord at the piece's solved curvature:
                // its centre hangs off the chord's midpoint (left for a
                // counter-clockwise piece, right for a clockwise one), and
                // the chord's central angle is 2·asin(chord / 2r) — the
                // same accounting that measured the piece's station length.
                let radius = 1.0 / curvature.abs();
                let turn = f64::from(curvature.signum() > 0.0) * 2.0 - 1.0;
                let theta = 2.0 * (c / (2.0 * radius)).min(1.0).asin();
                let mid = [(a.x + b.x) / 2.0, (a.y + b.y) / 2.0];
                let sagitta = (radius * radius - c * c / 4.0).max(0.0).sqrt();
                let center = [
                    mid[0] - ex[1] * turn * sagitta,
                    mid[1] + ex[0] * turn * sagitta,
                ];
                let (sin, cos) = (turn * f * theta).sin_cos();
                let from = [a.x - center[0], a.y - center[1]];
                let point = [
                    center[0] + from[0] * cos - from[1] * sin,
                    center[1] + from[0] * sin + from[1] * cos,
                ];
                // The chord aims at the arc's middle: the travel direction
                // at fraction f turns (f - ½)·θ from it.
                let (sin, cos) = (turn * (f - 0.5) * theta).sin_cos();
                let direction = [ex[0] * cos - ex[1] * sin, ex[0] * sin + ex[1] * cos];
                return Some((Vector2::new(point[0], point[1]), direction));
            }
            // A line runs straight; a spiral reads from its solved points
            // (the integration's sampling is the codec's spiral truth).
            let point = [a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f];
            return Some((Vector2::new(point[0], point[1]), ex));
        }
        return None;
    }
    None
}

/// `{"op":"civil_station_offset","handle":"…","station":…,"offset_distance":…}` — the
/// point at a station of an alignment, offset perpendicular to it.
///
/// The station is the one Civil 3D shows (past a station equation it runs on
/// from the station ahead); the `offset_distance` is signed across the
/// alignment, positive to the right of the direction of travel. Pure read:
/// nothing in the document changes.
pub(crate) fn civil_station_offset(document: &document::Document, req: &Value) -> Value {
    let Some(handle) = super::automation::request_handle(&req["handle"]) else {
        return err("civil_station_offset: missing or invalid \"handle\"");
    };
    let Some(data) = arch_named(document, handle, "AECC_ALIGNMENT") else {
        return err(format!(
            "civil_station_offset: handle {:?} is not an AECC_ALIGNMENT",
            req["handle"].as_str().unwrap_or_default()
        ));
    };
    let Some(station) = req["station"].as_f64() else {
        return err("civil_station_offset: missing \"station\"");
    };
    let offset = req["offset_distance"].as_f64().unwrap_or(0.0);
    let start = data.alignment_start_station().unwrap_or(0.0);
    let pieces = data.alignment_pieces_with_curvature();
    let length: f64 = pieces.iter().map(piece_length).sum();
    let distance = raw_station(&data.alignment_station_equations(), station) - start;
    // The solved run's length can fall a hair short of the stationing the
    // record reports (integrated spirals); a station that names the end
    // still reads as the end.
    const END_TOLERANCE: f64 = 1e-3;
    if !(-END_TOLERANCE..=length + END_TOLERANCE).contains(&distance) {
        return err(format!(
            "civil_station_offset: station {station:.4} is outside the alignment's stations {:.4} to {:.4}",
            start,
            start + length
        ));
    }
    let distance = distance.clamp(0.0, length);
    let Some((point, direction)) = point_at_distance(&pieces, distance) else {
        return err("civil_station_offset: the alignment's geometry is not solved that far");
    };
    // Positive offset to the right of the direction of travel.
    let at = [
        point.x + offset * direction[1],
        point.y - offset * direction[0],
    ];
    json!({
        "ok": true,
        "handle": format!("{:X}", handle.value()),
        "station": station,
        "offset": offset,
        "raw_station": start + distance,
        "point": [at[0], at[1]],
    })
}

/// Elevation bounds and counts of a TIN surface's drawn triangulation: the
/// vertices a visible triangle uses (a surface point outside the boundary or
/// dropped by an edit draws no triangle and carries no elevation).
pub(crate) fn surface_stats(surface: &TinSurface) -> Option<(f64, f64, usize, usize, usize)> {
    let mut drawn = vec![false; surface.vertices.len()];
    let mut visible = 0usize;
    for triangle in &surface.triangles {
        if !triangle.is_visible() {
            continue;
        }
        visible += 1;
        for index in triangle.vertices {
            if let Some(slot) = drawn.get_mut(index as usize) {
                *slot = true;
            }
        }
    }
    let mut bounds: Option<(f64, f64)> = None;
    for (index, used) in drawn.iter().enumerate() {
        if !used {
            continue;
        }
        let z = surface.vertices[index].position.z;
        bounds = Some(match bounds {
            Some((min, max)) => (min.min(z), max.max(z)),
            None => (z, z),
        });
    }
    bounds.map(|(min, max)| (min, max, surface.vertices.len(), surface.triangles.len(), visible))
}

/// `{"op":"civil_surface_stats","handle":"…"}` — a TIN surface's elevation
/// bounds and triangulation counts. Pure read over the decoded record.
pub(crate) fn civil_surface_stats(document: &document::Document, req: &Value) -> Value {
    let Some(handle) = super::automation::request_handle(&req["handle"]) else {
        return err("civil_surface_stats: missing or invalid \"handle\"");
    };
    let Some(data) = arch_named(document, handle, "AECC_TIN_SURFACE") else {
        return err(format!(
            "civil_surface_stats: handle {:?} is not an AECC_TIN_SURFACE",
            req["handle"].as_str().unwrap_or_default()
        ));
    };
    // The built triangulation, falling back to the stored snapshot a surface
    // keeps from its last build.
    let surface = data.tin_surface().or_else(|| data.tin_snapshot());
    let Some(surface) = surface else {
        return err(format!(
            "civil_surface_stats: handle {:X} keeps no readable triangulation",
            handle.value()
        ));
    };
    let Some((min, max, vertices, triangles, visible)) = surface_stats(&surface) else {
        return err(format!(
            "civil_surface_stats: handle {:X} draws no triangles",
            handle.value()
        ));
    };
    json!({
        "ok": true,
        "handle": format!("{:X}", handle.value()),
        "min_elevation": min,
        "max_elevation": max,
        "vertices": vertices,
        "triangles": triangles,
        "visible_triangles": visible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_points(points: &[(f64, f64)]) -> Vec<Vector2> {
        points.iter().map(|(x, y)| Vector2::new(*x, *y)).collect()
    }

    #[test]
    fn a_station_names_its_point_on_a_line() {
        let pieces = vec![(AlignmentPiece::Line, 0.0, line_points(&[(0.0, 0.0), (100.0, 0.0)]))];
        let (point, direction) = point_at_distance(&pieces, 40.0).expect("point");
        assert!((point.x - 40.0).abs() < 1e-12 && point.y.abs() < 1e-12, "{point:?}");
        assert!((direction[0] - 1.0).abs() < 1e-12 && direction[1].abs() < 1e-12);
    }

    #[test]
    fn a_curve_station_measures_along_the_arc() {
        // A 90-degree left arc of radius 50 centred at (0, 50): from (0,0)
        // to (50, 50). Its arc length is 25π, far past the 50√2 chord.
        let arc = 25.0 * std::f64::consts::PI;
        let pieces = vec![(
            AlignmentPiece::Curve,
            1.0 / 50.0,
            line_points(&[(0.0, 0.0), (50.0, 50.0)]),
        )];
        let (point, _) = point_at_distance(&pieces, arc).expect("point at the arc's end");
        assert!((point.x - 50.0).abs() < 1e-9, "{point:?}");
        assert!((point.y - 50.0).abs() < 1e-9, "{point:?}");
        // Halfway along the arc the walk sits on its 45-degree mid point.
        let (point, direction) = point_at_distance(&pieces, arc / 2.0)
            .expect("point at the arc's mid");
        assert!((point.x - 50.0 / std::f64::consts::SQRT_2).abs() < 1e-9, "{point:?}");
        assert!((point.y - (50.0 - 50.0 / std::f64::consts::SQRT_2)).abs() < 1e-9, "{point:?}");
        // The tangent there points 45 degrees up.
        assert!((direction[0] - direction[1]).abs() < 1e-9, "{direction:?}");
    }

    #[test]
    fn a_clockwise_curve_bends_the_other_way() {
        // A 90-degree right arc of radius 50 centred at (0, -50): from
        // (0,0) heading +x it ends at (50, -50).
        let arc = 25.0 * std::f64::consts::PI;
        let pieces = vec![(
            AlignmentPiece::Curve,
            -1.0 / 50.0,
            line_points(&[(0.0, 0.0), (50.0, -50.0)]),
        )];
        let (point, _) = point_at_distance(&pieces, arc).expect("point at the arc's end");
        assert!((point.x - 50.0).abs() < 1e-9, "{point:?}");
        assert!((point.y + 50.0).abs() < 1e-9, "{point:?}");
    }

    #[test]
    fn pieces_hand_over_at_their_joins() {
        let pieces = vec![
            (AlignmentPiece::Line, 0.0, line_points(&[(0.0, 0.0), (10.0, 0.0)])),
            (AlignmentPiece::Line, 0.0, line_points(&[(10.0, 0.0), (10.0, 10.0)])),
        ];
        let (point, direction) = point_at_distance(&pieces, 15.0).expect("past the join");
        assert!((point.x - 10.0).abs() < 1e-12 && (point.y - 5.0).abs() < 1e-12, "{point:?}");
        assert!(direction[0].abs() < 1e-12 && (direction[1] - 1.0).abs() < 1e-12);
        assert!(point_at_distance(&pieces, 20.0 + 1.0).is_none(), "past the end");
    }

    #[test]
    fn a_station_equation_shifts_the_raw_distance() {
        let equation = StationEquation { raw: 100.0, back: 120.0, ahead: 120.0, increasing: true };
        assert!((raw_station(&[equation], 150.0) - 130.0).abs() < 1e-12);
        let decreasing = StationEquation { raw: 100.0, back: 120.0, ahead: 120.0, increasing: false };
        assert!((raw_station(&[decreasing], 90.0) - 130.0).abs() < 1e-12);
        // Without an equation the shown station is the raw one.
        assert!((raw_station(&[], 42.0) - 42.0).abs() < 1e-12);
        // The last equation the shown station has passed wins.
        let later = StationEquation { raw: 200.0, back: 210.0, ahead: 210.0, increasing: true };
        assert!((raw_station(&[equation, later], 215.0) - 205.0).abs() < 1e-12);
    }

    #[test]
    fn the_surface_bounds_come_from_the_drawn_triangles() {
        use document::classes::civil::{TinTriangle, TinVertex};
        use document::types::Vector3;
        let vertex = |x: f64, y: f64, z: f64| TinVertex {
            position: Vector3::new(x, y, z),
            flags: 0x80,
            triangle: 0,
        };
        let surface = TinSurface {
            // The drawn surface spans 10..20; the fourth vertex is a corner
            // whose triangles are hidden — it carries no drawn elevation.
            vertices: vec![vertex(0.0, 0.0, 10.0), vertex(1.0, 0.0, 20.0), vertex(0.0, 1.0, 15.0), vertex(5.0, 5.0, 99.0)],
            triangles: vec![
                TinTriangle { vertices: [0, 1, 2], neighbors: [-1, -1, -1], flags: 0 },
                TinTriangle { vertices: [0, 1, 3], neighbors: [-1, -1, -1], flags: 1 },
            ],
            corners: [0, 1, 2, 3],
        };
        let (min, max, vertices, triangles, visible) = surface_stats(&surface).expect("bounds");
        assert_eq!((min, max), (10.0, 20.0));
        assert_eq!((vertices, triangles, visible), (4, 2, 1));
    }
}
