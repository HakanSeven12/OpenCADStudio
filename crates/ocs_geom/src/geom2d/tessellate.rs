//! Turning curves into polylines.
//!
//! Everything downstream — rendering, picking, offsetting, hatch boundary
//! resolution — eventually wants points. These produce them in world
//! coordinates as `[x, y, z]`, `f64`.
//!
//! # Why `f64` and not the render vertex type
//!
//! Emitting `f32` here would put the narrowing *before* the caller has a
//! chance to condition the values. A drawing at survey coordinates has
//! roughly seven significant digits eaten by the origin, and `f32` carries
//! about seven in total, so the cast lands squarely on the geometry. The
//! renderer's own vertex path already splits `f64` into a high and low `f32`
//! pair for exactly this reason; handing it `f32` would defeat that.
//!
//! So: `f64` out, and narrowing stays at the GPU boundary where the residual
//! is kept.

use super::angle::{arc_span, normalize_angle};
use super::vec::Vec2;
use super::Ellipse;

/// Polyline density the editor has used for preview geometry: twenty segments
/// per radian, so roughly 125 around a full turn.
///
/// Exposed because it is a look-and-feel number rather than a correctness
/// one, and a caller tessellating for analysis rather than display wants to
/// pick its own.
pub const DEFAULT_SEGMENTS_PER_RADIAN: f64 = 20.0;

/// No arc gets fewer than this many segments, however short its sweep.
///
/// Without a floor, a very short arc collapses to a single chord and stops
/// being distinguishable from the line that trimmed it.
const MINIMUM_SEGMENTS: usize = 4;

fn segment_count(sweep: f64, segments_per_radian: f64) -> usize {
    let wanted = (sweep.abs() * segments_per_radian).ceil();
    if wanted.is_finite() && wanted > MINIMUM_SEGMENTS as f64 {
        wanted as usize
    } else {
        MINIMUM_SEGMENTS
    }
}

/// Samples a circular arc counter-clockwise from `start_angle` to
/// `end_angle`.
///
/// Both endpoints are included, so an arc of *n* segments yields *n + 1*
/// points. A closed arc — equal angles — sweeps a full turn rather than
/// collapsing, matching [`arc_span`].
pub fn arc(
    centre: [f64; 2],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    z: f64,
    segments_per_radian: f64,
) -> Vec<[f64; 3]> {
    let sweep = arc_span(start_angle, end_angle);
    let segments = segment_count(sweep, segments_per_radian);
    let base = normalize_angle(start_angle);
    (0..=segments)
        .map(|i| {
            let angle = base + sweep * (i as f64 / segments as f64);
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
                z,
            ]
        })
        .collect()
}

/// Samples an elliptical arc between two of the ellipse's own parameters.
///
/// Unlike [`arc`], the parameters are taken as given rather than normalised:
/// an ellipse's stored parameters are not angles on a circle, and the caller
/// is the one that knows whether a wrap needs a turn added. Passing
/// `end_parameter` below `start_parameter` sweeps backwards, which is what a
/// clockwise arc wants.
pub fn ellipse_arc(
    ellipse: &Ellipse,
    start_parameter: f64,
    end_parameter: f64,
    z: f64,
    segments_per_radian: f64,
) -> Vec<[f64; 3]> {
    let sweep = end_parameter - start_parameter;
    let segments = segment_count(sweep, segments_per_radian);
    (0..=segments)
        .map(|i| {
            let t = start_parameter + sweep * (i as f64 / segments as f64);
            let point = ellipse.point_at(t);
            [point[0], point[1], z]
        })
        .collect()
}

/// A point a fraction `t` of the way from `from` to `to`.
///
/// The array-flavoured form of [`Vec2::lerp`], since this crate's signatures
/// speak in arrays.
pub fn lerp(from: [f64; 2], to: [f64; 2], t: f64) -> [f64; 2] {
    Vec2::from(from).lerp(Vec2::from(to), t).to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    #[test]
    fn an_arc_includes_both_endpoints() {
        let pts = arc(
            [0.0, 0.0],
            1.0,
            0.0,
            FRAC_PI_2,
            0.0,
            DEFAULT_SEGMENTS_PER_RADIAN,
        );
        let first = pts.first().unwrap();
        let last = pts.last().unwrap();
        assert!((first[0] - 1.0).abs() < 1e-12 && first[1].abs() < 1e-12);
        assert!(last[0].abs() < 1e-12 && (last[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn every_sampled_point_sits_on_the_circle() {
        let centre = [3.0, -4.0];
        let radius = 7.5;
        for p in arc(centre, radius, 0.3, 4.0, 2.0, DEFAULT_SEGMENTS_PER_RADIAN) {
            let dx = p[0] - centre[0];
            let dy = p[1] - centre[1];
            assert!(((dx * dx + dy * dy).sqrt() - radius).abs() < 1e-9);
            assert_eq!(p[2], 2.0, "z should be carried through untouched");
        }
    }

    #[test]
    fn a_closed_arc_sweeps_a_full_turn() {
        let pts = arc([0.0, 0.0], 1.0, 0.0, 0.0, 0.0, DEFAULT_SEGMENTS_PER_RADIAN);
        assert!(
            pts.len() > 100,
            "expected a full circle, got {} pts",
            pts.len()
        );
        let first = pts.first().unwrap();
        let last = pts.last().unwrap();
        assert!((first[0] - last[0]).abs() < 1e-9 && (first[1] - last[1]).abs() < 1e-9);
    }

    #[test]
    fn a_vanishingly_short_arc_still_gets_a_few_segments() {
        let pts = arc(
            [0.0, 0.0],
            1.0,
            1.0,
            1.000_001,
            0.0,
            DEFAULT_SEGMENTS_PER_RADIAN,
        );
        assert_eq!(pts.len(), MINIMUM_SEGMENTS + 1);
    }

    #[test]
    fn density_scales_with_the_requested_rate() {
        let sparse = arc([0.0, 0.0], 1.0, 0.0, TAU, 0.0, 1.0).len();
        let dense = arc([0.0, 0.0], 1.0, 0.0, TAU, 0.0, 50.0).len();
        assert!(dense > sparse * 10, "{dense} should dwarf {sparse}");
    }

    #[test]
    fn components_land_in_x_y_z_order() {
        // A quarter turn from the +X axis ends on +Y. If y and z were swapped
        // the second component would stay at the elevation instead.
        let circle = Ellipse {
            centre: [0.0, 0.0],
            major_radius: 1.0,
            minor_radius: 1.0,
            major_axis: [1.0, 0.0],
        };
        let pts = ellipse_arc(&circle, 0.0, FRAC_PI_2, 9.0, DEFAULT_SEGMENTS_PER_RADIAN);
        let last = pts.last().unwrap();
        assert!(
            last[0].abs() < 1e-12,
            "x should reach zero, got {}",
            last[0]
        );
        assert!(
            (last[1] - 1.0).abs() < 1e-12,
            "y should reach one, got {}",
            last[1]
        );
        assert_eq!(last[2], 9.0, "z should be the elevation");
    }

    #[test]
    fn a_unit_ratio_ellipse_matches_the_circle_of_the_same_radius() {
        let circle = Ellipse {
            centre: [1.0, 2.0],
            major_radius: 3.0,
            minor_radius: 3.0,
            major_axis: [1.0, 0.0],
        };
        let as_ellipse = ellipse_arc(&circle, 0.0, PI, 0.0, DEFAULT_SEGMENTS_PER_RADIAN);
        let as_arc = arc([1.0, 2.0], 3.0, 0.0, PI, 0.0, DEFAULT_SEGMENTS_PER_RADIAN);
        assert_eq!(as_ellipse.len(), as_arc.len());
        for (a, b) in as_ellipse.iter().zip(as_arc.iter()) {
            assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
        }
    }

    #[test]
    fn every_sampled_point_satisfies_the_ellipse_equation() {
        let ellipse = Ellipse {
            centre: [-6.0, 11.0],
            major_radius: 8.0,
            minor_radius: 3.0,
            major_axis: [0.6, 0.8], // unit
        };
        let Ellipse {
            centre,
            major_radius,
            minor_radius,
            major_axis,
        } = ellipse;
        for p in ellipse_arc(&ellipse, 0.2, 5.0, 0.0, DEFAULT_SEGMENTS_PER_RADIAN) {
            let rx = p[0] - centre[0];
            let ry = p[1] - centre[1];
            let along = rx * major_axis[0] + ry * major_axis[1];
            let across = -rx * major_axis[1] + ry * major_axis[0];
            let value = (along / major_radius).powi(2) + (across / minor_radius).powi(2);
            assert!((value - 1.0).abs() < 1e-9, "off the ellipse: {value}");
        }
    }

    #[test]
    fn a_backwards_parameter_range_sweeps_the_other_way() {
        let ellipse = Ellipse {
            centre: [0.0, 0.0],
            major_radius: 2.0,
            minor_radius: 1.0,
            major_axis: [1.0, 0.0],
        };
        let forward = ellipse_arc(&ellipse, 0.0, FRAC_PI_2, 0.0, DEFAULT_SEGMENTS_PER_RADIAN);
        let backward = ellipse_arc(&ellipse, FRAC_PI_2, 0.0, 0.0, DEFAULT_SEGMENTS_PER_RADIAN);
        assert_eq!(forward.len(), backward.len());
        let forward_end = forward.last().unwrap();
        let backward_start = backward.first().unwrap();
        assert!((forward_end[1] - backward_start[1]).abs() < 1e-12);
    }

    #[test]
    fn survey_coordinates_keep_their_precision() {
        // The same arc at a UTM-scale centre. In f64 the radius is still
        // resolvable to well under a micrometre; this is the value the render
        // path splits rather than truncates.
        let centre = [512_345.678, 4_512_345.678];
        let radius = 0.5;
        for p in arc(centre, radius, 0.0, TAU, 0.0, DEFAULT_SEGMENTS_PER_RADIAN) {
            let dx = p[0] - centre[0];
            let dy = p[1] - centre[1];
            assert!(
                ((dx * dx + dy * dy).sqrt() - radius).abs() < 1e-9,
                "radius drifted at survey scale"
            );
        }
    }

    #[test]
    fn lerp_hits_both_ends_and_the_middle() {
        assert_eq!(lerp([0.0, 0.0], [10.0, 20.0], 0.0), [0.0, 0.0]);
        assert_eq!(lerp([0.0, 0.0], [10.0, 20.0], 1.0), [10.0, 20.0]);
        assert_eq!(lerp([0.0, 0.0], [10.0, 20.0], 0.5), [5.0, 10.0]);
    }
}
