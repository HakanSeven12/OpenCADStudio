//! Polylines whose segments may be arcs.
//!
//! CAD polylines carry a *bulge* per vertex rather than an explicit arc:
//! `bulge = tan(θ/4)`, where θ is the included angle of the arc leaving that
//! vertex. Zero is a straight segment, `±1` a half turn, and the sign gives
//! the direction — positive counter-clockwise.
//!
//! It is a compact encoding and a fiddly one, because recovering the centre
//! and sweep from it has several sign and half-turn cases that are easy to get
//! subtly wrong in each place that needs them. [`BulgeArc`] does it once.

use super::{frame::Frame, vec::Vec2, Tolerance};

/// The circular arc a bulge encodes, in the form most callers actually want.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BulgeArc {
    /// Centre of the circle the arc lies on.
    pub center: [f64; 2],
    /// Its radius.
    pub radius: f64,
    /// Angle from the centre to the first endpoint, in `-π..=π`.
    pub start_angle: f64,
    /// Angle from the centre to the second endpoint, in `-π..=π`.
    pub end_angle: f64,
    /// Signed sweep from first to second endpoint: positive counter-clockwise
    /// for a positive bulge, negative for a negative one. An exact half turn
    /// takes its direction from the bulge's sign, since the endpoint angles
    /// alone cannot say which way round it goes.
    pub sweep: f64,
}

impl BulgeArc {
    /// Recovers the arc between `p0` and `p1` for the given bulge.
    ///
    /// `None` when there is no arc to recover: a chord of no length, or a
    /// bulge of zero, which is a straight segment.
    pub fn from_bulge(p0: [f64; 2], p1: [f64; 2], bulge: f64) -> Option<Self> {
        let chord_x = p1[0] - p0[0];
        let chord_y = p1[1] - p0[1];
        let chord_len = (chord_x * chord_x + chord_y * chord_y).sqrt();
        if chord_len < 1e-12 || bulge.abs() < 1e-12 {
            return None;
        }
        let squared = bulge * bulge;
        // r = chord · (1 + b²) / (4·|b|)
        let radius = chord_len * (1.0 + squared) / (4.0 * bulge.abs());
        // Signed distance from the chord's midpoint to the centre,
        // r · (1 - b²) / (1 + b²), which is r · cos(θ/2).
        let to_centre = radius * (1.0 - squared) / (1.0 + squared);
        let mid_x = (p0[0] + p1[0]) * 0.5;
        let mid_y = (p0[1] + p1[1]) * 0.5;
        // Left perpendicular to the chord, a quarter turn counter-clockwise.
        let perp_x = -chord_y / chord_len;
        let perp_y = chord_x / chord_len;
        let sign = bulge.signum();
        let center = [
            mid_x + sign * to_centre * perp_x,
            mid_y + sign * to_centre * perp_y,
        ];

        let start_angle = (p0[1] - center[1]).atan2(p0[0] - center[0]);
        let end_angle = (p1[1] - center[1]).atan2(p1[0] - center[0]);

        // Wrap the sweep so its sign matches the bulge's.
        let mut sweep = end_angle - start_angle;
        if bulge > 0.0 {
            if sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
        } else if sweep >= 0.0 {
            sweep -= std::f64::consts::TAU;
        }
        if sweep.abs() < 1e-9 {
            // The endpoints coincide in angle: a full turn, pointed by bulge.
            sweep = if bulge > 0.0 {
                std::f64::consts::TAU
            } else {
                -std::f64::consts::TAU
            };
        }

        Some(Self {
            center,
            radius,
            start_angle,
            end_angle,
            sweep,
        })
    }

    /// The point a fraction `t` along the arc, walking the signed sweep.
    ///
    /// `t = 0` is the first endpoint, `t = 1` the second.
    pub fn sample(&self, t: f64) -> [f64; 2] {
        let angle = self.start_angle + self.sweep * t;
        [
            self.center[0] + self.radius * angle.cos(),
            self.center[1] + self.radius * angle.sin(),
        ]
    }

    /// Samples by maximum change of direction, in radians.
    pub fn tessellate_angle(&self, max_angle: f64) -> Vec<[f64; 2]> {
        let max_angle = crate::tessellation::angle(max_angle);
        let segments = (self.sweep.abs() / max_angle).ceil().max(1.0) as usize;
        (0..=segments)
            .map(|index| self.sample(index as f64 / segments as f64))
            .collect()
    }
}

/// One vertex, and the shape of the segment leaving it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolylineVertex {
    /// Where the vertex sits.
    pub position: [f64; 2],
    /// Bulge of the segment from here to the next vertex; zero is straight.
    pub bulge: f64,
}

impl PolylineVertex {
    /// A vertex whose outgoing segment is straight.
    pub fn straight(position: [f64; 2]) -> Self {
        Self {
            position,
            bulge: 0.0,
        }
    }

    /// A vertex whose outgoing segment arcs by `bulge`.
    pub fn curved(position: [f64; 2], bulge: f64) -> Self {
        Self { position, bulge }
    }
}

/// A chain of vertices, open or closed.
///
/// On a closed polyline the segment leaving the last vertex returns to the
/// first, and that segment takes its bulge from the last vertex — there is no
/// repeated closing vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polyline {
    /// Vertices in order.
    pub vertices: Vec<PolylineVertex>,
    /// Whether the last vertex joins back to the first.
    pub closed: bool,
}

/// The orthogonal frame and dimensions of a rectangular polyline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangleFrame {
    /// First vertex of the polyline.
    pub origin: [f64; 2],
    /// Unit direction from the first vertex to the second.
    pub width_axis: [f64; 2],
    /// Unit direction from the second vertex to the third.
    pub height_axis: [f64; 2],
    /// Distance from the first vertex to the second.
    pub width: f64,
    /// Perpendicular distance from the second vertex to the third.
    pub height: f64,
}

impl Polyline {
    /// An empty open polyline.
    pub fn new() -> Self {
        Self::default()
    }

    /// The arc leaving vertex `index`, or `None` where that segment is
    /// straight or does not exist.
    pub fn segment_arc(&self, index: usize) -> Option<BulgeArc> {
        let count = self.vertices.len();
        if index >= count {
            return None;
        }
        let next = if index + 1 < count {
            index + 1
        } else if self.closed {
            0
        } else {
            return None;
        };
        BulgeArc::from_bulge(
            self.vertices[index].position,
            self.vertices[next].position,
            self.vertices[index].bulge,
        )
    }

    /// Returns the rectangle described by four ordered straight vertices.
    pub fn rectangle_frame(&self, tolerance: Tolerance) -> Option<RectangleFrame> {
        if !self.closed
            || self.vertices.len() != 4
            || self.vertices.iter().any(|vertex| {
                vertex.bulge != 0.0 || !vertex.position.iter().all(|value| value.is_finite())
            })
        {
            return None;
        }

        let world = [0, 1, 2, 3].map(|index| {
            let [x, y] = self.vertices[index].position;
            [x, y, 0.0]
        });
        let frame = Frame::around(world.iter());
        let coordinates = world.map(|point| Vec2::from(frame.lift_2d([point[0], point[1]])));
        let width_vector = coordinates[1] - coordinates[0];
        let second_edge = coordinates[2] - coordinates[1];
        let width = width_vector.length();
        if width <= tolerance.linear() {
            return None;
        }

        let width_axis = width_vector / width;
        let perpendicular = width_axis.perpendicular();
        let signed_height = second_edge.dot(perpendicular);
        let height = signed_height.abs();
        if height <= tolerance.linear() {
            return None;
        }
        let height_axis = perpendicular * signed_height.signum();
        let expected_second = coordinates[1] + height_axis * height;
        let expected_third = coordinates[0] + height_axis * height;
        if coordinates[2].distance(expected_second) > tolerance.linear()
            || coordinates[3].distance(expected_third) > tolerance.linear()
        {
            return None;
        }

        Some(RectangleFrame {
            origin: self.vertices[0].position,
            width_axis: width_axis.to_array(),
            height_axis: height_axis.to_array(),
            width,
            height,
        })
    }
}

/// One retained segment's correspondence to its source segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolylineRangeSegment {
    pub source_index: usize,
    pub from: f64,
    pub to: f64,
}

impl PolylineRangeSegment {
    /// Restrict a quantity varying linearly along the source segment, such as width.
    pub fn interpolate(&self, start: f64, end: f64) -> [f64; 2] {
        [(1.0 - self.from) * start + self.from * end,
         (1.0 - self.to) * start + self.to * end]
    }
}

/// An open portion of a polyline and its outgoing segment correspondences.
#[derive(Clone, Debug, PartialEq)]
pub struct PolylineRange {
    pub polyline: Polyline,
    pub segments: Vec<PolylineRangeSegment>,
}

impl Polyline {
    /// Extract an increasing interval of the uniform segment parameter `0..=1`.
    /// Closed inputs may wrap once, with `to` above one. Circular segments are
    /// restricted analytically, retaining their signed bulges. No tessellation
    /// or snapping to nearby vertices is performed. Invalid/empty intervals,
    /// nonfinite geometry and collapsed curved segments return `None`.
    pub fn ranged(&self, from: f64, to: f64) -> Option<PolylineRange> {
        let n = self.vertices.len();
        if n < 2 || !from.is_finite() || !to.is_finite() || from < 0.0
            || from > 1.0 || to <= from || to > from + 1.0
            || (!self.closed && to > 1.0)
            || self.vertices.iter().any(|v| !v.bulge.is_finite()
                || v.position.iter().any(|x| !x.is_finite())) { return None; }
        let count = if self.closed { n } else { n - 1 };
        let start = from * count as f64;
        let end = to * count as f64;
        let first = start.floor() as usize;
        let last = end.ceil() as usize;
        let mut vertices = Vec::with_capacity(last - first + 1);
        let mut segments = Vec::with_capacity(last - first);
        let mut endpoint = None;
        for index in first..last {
            let source_index = index % count;
            let a = (start - index as f64).max(0.0);
            let b = (end - index as f64).min(1.0);
            if b <= a { continue; }
            let v = self.vertices[source_index];
            let next = self.vertices[(source_index + 1) % n];
            let arc = if v.bulge.abs() >= 1e-12 {
                Some(BulgeArc::from_bulge(v.position, next.position, v.bulge)?)
            } else { None };
            let sample = |t: f64| {
                if t == 0.0 { v.position } else if t == 1.0 { next.position }
                else if let Some(arc) = arc { arc.sample(t) }
                else { [(1.0-t)*v.position[0]+t*next.position[0],
                        (1.0-t)*v.position[1]+t*next.position[1]] }
            };
            let bulge = if a == 0.0 && b == 1.0 { v.bulge }
                else if arc.is_some() { (v.bulge.atan() * (b-a)).tan() }
                else { 0.0 };
            let position = sample(a);
            let finish = sample(b);
            if !bulge.is_finite() || position.iter().chain(finish.iter()).any(|x| !x.is_finite()) { return None; }
            vertices.push(PolylineVertex { position, bulge });
            endpoint = Some(finish);
            segments.push(PolylineRangeSegment { source_index, from: a, to: b });
        }
        vertices.push(PolylineVertex::straight(endpoint?));
        Some(PolylineRange { polyline: Polyline { vertices, closed: false }, segments })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    #[test]
    fn a_half_turn_bulge_gives_the_chord_as_diameter() {
        let arc = BulgeArc::from_bulge([0.0, 0.0], [2.0, 0.0], 1.0).unwrap();
        assert!((arc.radius - 1.0).abs() < 1e-12);
        assert!((arc.center[0] - 1.0).abs() < 1e-12 && arc.center[1].abs() < 1e-12);
        assert!((arc.sweep - PI).abs() < 1e-12);
    }

    #[test]
    fn the_bulge_sign_picks_the_side() {
        let ccw = BulgeArc::from_bulge([0.0, 0.0], [2.0, 0.0], 1.0).unwrap();
        let cw = BulgeArc::from_bulge([0.0, 0.0], [2.0, 0.0], -1.0).unwrap();
        assert!(ccw.sweep > 0.0 && cw.sweep < 0.0);
        // Same circle either way; only the direction of travel differs.
        assert!((ccw.radius - cw.radius).abs() < 1e-12);

        // Travelling counter-clockwise from (0,0) to (2,0) about (1,0) means
        // going the long way under the circle, so a positive bulge dips below
        // this left-to-right chord rather than above it.
        let over_ccw = ccw.sample(0.5);
        let over_cw = cw.sample(0.5);
        assert!(over_ccw[1] < 0.0, "positive bulge should dip below, got {over_ccw:?}");
        assert!(over_cw[1] > 0.0, "negative bulge should rise above, got {over_cw:?}");
        // Mirror images about the chord.
        assert!((over_ccw[1] + over_cw[1]).abs() < 1e-12);
    }

    #[test]
    fn sampling_hits_the_endpoints() {
        let arc = BulgeArc::from_bulge([1.0, 2.0], [4.0, 6.0], 0.4).unwrap();
        let start = arc.sample(0.0);
        let end = arc.sample(1.0);
        assert!((start[0] - 1.0).abs() < 1e-9 && (start[1] - 2.0).abs() < 1e-9);
        assert!((end[0] - 4.0).abs() < 1e-9 && (end[1] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn a_quarter_turn_bulge_sweeps_a_quarter_turn() {
        // tan(90° / 4) for a quarter-turn arc.
        let bulge = (FRAC_PI_2 / 4.0).tan();
        let arc = BulgeArc::from_bulge([1.0, 0.0], [0.0, 1.0], bulge).unwrap();
        assert!((arc.sweep - FRAC_PI_2).abs() < 1e-9, "swept {}", arc.sweep);
        assert!(arc.center[0].abs() < 1e-9 && arc.center[1].abs() < 1e-9);
        assert!((arc.radius - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_bulge_over_one_sweeps_past_a_half_turn() {
        let arc = BulgeArc::from_bulge([0.0, 0.0], [2.0, 0.0], 2.0).unwrap();
        assert!(arc.sweep > PI, "expected a major arc, swept {}", arc.sweep);
        assert!(arc.sweep < TAU);
    }

    #[test]
    fn degenerate_input_has_no_arc() {
        assert!(BulgeArc::from_bulge([1.0, 1.0], [1.0, 1.0], 0.5).is_none());
        assert!(BulgeArc::from_bulge([0.0, 0.0], [1.0, 0.0], 0.0).is_none());
    }

    #[test]
    fn a_closed_polyline_arcs_back_from_its_last_vertex() {
        let poly = Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::curved([2.0, 0.0], 1.0),
            ],
            closed: true,
        };
        // The segment leaving vertex 1 returns to vertex 0 and is an arc.
        let arc = poly.segment_arc(1).expect("closing segment should arc");
        assert!((arc.radius - 1.0).abs() < 1e-12);
        // Vertex 0's own segment is straight, so it has no arc.
        assert!(poly.segment_arc(0).is_none());
    }

    #[test]
    fn an_open_polyline_has_no_closing_segment() {
        let poly = Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::curved([2.0, 0.0], 1.0),
            ],
            closed: false,
        };
        assert!(poly.segment_arc(1).is_none());
        assert!(poly.segment_arc(5).is_none());
    }

    #[test]
    fn rectangle_frame_recognizes_rotation_and_rejects_non_rectangles() {
        let origin = Vec2::new(500_000.0, 4_500_000.0);
        let width_axis = Vec2::new(0.6, 0.8);
        let height_axis = Vec2::new(-0.8, 0.6);
        let points = [
            origin,
            origin + width_axis * 10.0,
            origin + width_axis * 10.0 + height_axis * 4.0,
            origin + height_axis * 4.0,
        ];
        let mut polyline = Polyline {
            vertices: points
                .map(|point| PolylineVertex::straight(point.to_array()))
                .to_vec(),
            closed: true,
        };

        let rectangle = polyline.rectangle_frame(Tolerance::default()).unwrap();
        assert!((rectangle.width - 10.0).abs() < 1.0e-9);
        assert!((rectangle.height - 4.0).abs() < 1.0e-9);
        assert!(Vec2::from(rectangle.width_axis).distance(width_axis) < 1.0e-9);
        assert!(Vec2::from(rectangle.height_axis).distance(height_axis) < 1.0e-9);

        polyline.vertices[3].position[0] += 0.01;
        assert!(polyline.rectangle_frame(Tolerance::default()).is_none());
        polyline.vertices[3].position = points[3].to_array();
        polyline.vertices[0].bulge = 1.0e-12;
        assert!(polyline.rectangle_frame(Tolerance::default()).is_none());
    }

    #[test]
    fn a_range_keeps_segment_provenance_and_interpolation_parameters() {
        let polyline = Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([1.0, 0.0]),
                PolylineVertex::straight([1.0, 1.0]),
            ],
            closed: false,
        };
        let range = polyline.ranged(0.25, 0.75).unwrap();
        assert_eq!(
            range.polyline.vertices,
            vec![
                PolylineVertex::straight([0.5, 0.0]),
                PolylineVertex::straight([1.0, 0.0]),
                PolylineVertex::straight([1.0, 0.5]),
            ]
        );
        assert_eq!(
            range.segments,
            vec![
                PolylineRangeSegment {
                    source_index: 0,
                    from: 0.5,
                    to: 1.0,
                },
                PolylineRangeSegment {
                    source_index: 1,
                    from: 0.0,
                    to: 0.5,
                },
            ]
        );
        assert_eq!(range.segments[0].interpolate(2.0, 6.0), [4.0, 6.0]);
        assert_eq!(range.segments[1].interpolate(6.0, 10.0), [6.0, 8.0]);
    }

    #[test]
    fn a_partial_signed_arc_remains_the_same_exact_arc() {
        let polyline = Polyline {
            vertices: vec![
                PolylineVertex::curved([-1.0, 0.0], -1.0),
                PolylineVertex::straight([1.0, 0.0]),
            ],
            closed: false,
        };
        let original = polyline.segment_arc(0).unwrap();
        let range = polyline.ranged(0.25, 0.75).unwrap();
        let restricted = range.polyline.segment_arc(0).unwrap();
        assert!(
            crate::geom2d::Vec2::from(restricted.sample(0.5))
                .distance(crate::geom2d::Vec2::from(original.sample(0.5)))
                < 1e-12
        );
        assert!((range.polyline.vertices[0].bulge - (-std::f64::consts::FRAC_PI_8).tan()).abs() < 1e-12);
    }

    #[test]
    fn a_closed_range_can_wrap_once_across_the_original_seam() {
        let polyline = Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([1.0, 0.0]),
                PolylineVertex::straight([1.0, 1.0]),
                PolylineVertex::straight([0.0, 1.0]),
            ],
            closed: true,
        };
        let range = polyline.ranged(0.75, 1.25).unwrap();
        assert!(!range.polyline.closed);
        assert_eq!(
            range.polyline.vertices,
            vec![
                PolylineVertex::straight([0.0, 1.0]),
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([1.0, 0.0]),
            ]
        );
        assert_eq!(
            range
                .segments
                .iter()
                .map(|segment| segment.source_index)
                .collect::<Vec<_>>(),
            vec![3, 0]
        );
    }

    #[test]
    fn invalid_ranges_and_geometry_are_rejected() {
        let mut polyline = Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([1.0, 0.0]),
            ],
            closed: false,
        };
        assert!(polyline.ranged(0.5, 0.5).is_none());
        assert!(polyline.ranged(f64::NAN, 0.5).is_none());
        assert!(polyline.ranged(0.5, 1.5).is_none());
        polyline.vertices[0] = PolylineVertex::curved([0.0, 0.0], 1.0);
        polyline.vertices[1].position = [0.0, 0.0];
        assert!(polyline.ranged(0.0, 1.0).is_none());
    }
}
