//! Moving geometry from one frame to another.
//!
//! A block's contents are drawn in the block's own coordinates and placed by
//! an insertion that may move, turn, scale and mirror them. Anything that
//! wants those contents in world coordinates — snapping, picking, measuring —
//! has to carry them across, and carrying a *curve* across is not the same as
//! carrying its points.
//!
//! # What a scale does to a circle
//!
//! Under a uniform scale a circle is a circle. Under one that is not — a
//! block placed at 2× in X and 1× in Y — it is an ellipse, and pretending
//! otherwise puts geometry visibly in the wrong place. So the transform
//! promotes: circles and arcs come back as ellipses and elliptical arcs when
//! the scale is uneven, and stay as they were when it is not.
//!
//! A polyline is the one shape that cannot always follow. Its arcs are stored
//! as bulges, which describe a circular arc and nothing else, so a polyline
//! carrying one cannot be squashed. Straight ones transform freely.

use super::curve::{Arc, Circle, Curve, EllipseArc, Line, Ray, XLine};
use super::vec::Vec2;
use super::Ellipse;
use std::f64::consts::{FRAC_PI_2, TAU};

/// An affine map of the plane: a linear part and a translation.
///
/// Held as the images of the two axes and of the origin, which is the form
/// that makes what it does legible — `x_axis` is where `(1, 0)` lands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Where the X axis lands.
    pub x_axis: Vec2,
    /// Where the Y axis lands.
    pub y_axis: Vec2,
    /// Where the origin lands.
    pub origin: Vec2,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// The map that changes nothing.
    pub const IDENTITY: Self = Self {
        x_axis: Vec2::new(1.0, 0.0),
        y_axis: Vec2::new(0.0, 1.0),
        origin: Vec2::ZERO,
    };

    /// A shift with no turn or scale.
    pub fn translation(by: [f64; 2]) -> Self {
        Self {
            origin: Vec2::from(by),
            ..Self::IDENTITY
        }
    }

    /// A turn about the origin, counter-clockwise.
    pub fn rotation(angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self {
            x_axis: Vec2::new(cos, sin),
            y_axis: Vec2::new(-sin, cos),
            origin: Vec2::ZERO,
        }
    }

    /// A scale about the origin, per axis. Equal factors keep circles round;
    /// a negative one mirrors.
    pub fn scale(x: f64, y: f64) -> Self {
        Self {
            x_axis: Vec2::new(x, 0.0),
            y_axis: Vec2::new(0.0, y),
            origin: Vec2::ZERO,
        }
    }

    /// The block-insertion map: scale, then turn, then move.
    ///
    /// In that order because it is the order an insertion means — the scale is
    /// about the block's own origin, not about where it ends up.
    pub fn insertion(at: [f64; 2], scale: [f64; 2], angle: f64) -> Self {
        Self::translation(at).then_of(Self::rotation(angle).then_of(Self::scale(scale[0], scale[1])))
    }

    /// This map applied after `first`.
    pub fn then_of(self, first: Self) -> Self {
        Self {
            x_axis: self.apply_vector(first.x_axis.to_array()).into(),
            y_axis: self.apply_vector(first.y_axis.to_array()).into(),
            origin: self.apply_point(first.origin.to_array()).into(),
        }
    }

    /// Where a point lands.
    pub fn apply_point(&self, point: [f64; 2]) -> [f64; 2] {
        let p = Vec2::from(point);
        (self.origin + self.x_axis * p.x + self.y_axis * p.y).to_array()
    }

    /// Where a direction lands. Unlike a point, it ignores the translation.
    pub fn apply_vector(&self, vector: [f64; 2]) -> [f64; 2] {
        let v = Vec2::from(vector);
        (self.x_axis * v.x + self.y_axis * v.y).to_array()
    }

    /// Signed area scaling. Negative means the map mirrors.
    pub fn determinant(&self) -> f64 {
        self.x_axis.cross(self.y_axis)
    }

    /// Whether the map keeps circles round: the axes stay the same length and
    /// stay square to one another.
    pub fn is_similarity(&self) -> bool {
        let x = self.x_axis.length();
        let y = self.y_axis.length();
        if x < 1e-12 || y < 1e-12 {
            return false;
        }
        (x - y).abs() <= 1e-9 * x.max(y)
            && self.x_axis.dot(self.y_axis).abs() <= 1e-9 * x * y
    }

    /// How much lengths are multiplied by, for a map that keeps circles round.
    fn similarity_scale(&self) -> f64 {
        self.x_axis.length()
    }
}

impl Curve {
    /// This curve in another frame.
    ///
    /// Shapes are promoted rather than misrepresented: under a scale that is
    /// not uniform a circle becomes an ellipse and an arc an elliptical arc,
    /// because that is what they are.
    ///
    /// `None` only for a polyline carrying bulges under such a scale. A bulge
    /// describes a circular arc and cannot describe a squashed one, so there
    /// is nothing truthful to return.
    pub fn transformed(&self, by: &Transform) -> Option<Curve> {
        let point = |p: [f64; 2]| by.apply_point(p);

        Some(match self {
            Curve::Line(line) => Curve::Line(Line {
                start: point(line.start),
                end: point(line.end),
            }),
            Curve::Ray(ray) => Curve::Ray(Ray {
                origin: point(ray.origin),
                direction: by.apply_vector(ray.direction),
            }),
            Curve::XLine(line) => Curve::XLine(XLine {
                base: point(line.base),
                direction: by.apply_vector(line.direction),
            }),

            Curve::Circle(circle) if by.is_similarity() => Curve::Circle(Circle {
                centre: point(circle.centre),
                radius: circle.radius * by.similarity_scale(),
            }),
            Curve::Arc(arc) if by.is_similarity() => {
                let turn = Vec2::from(by.apply_vector([1.0, 0.0])).angle();
                let (start, end) = if by.determinant() < 0.0 {
                    // A reflection sends the angle t to `turn - t`, which
                    // reverses the direction of travel — so the ends swap to
                    // keep the arc sweeping counter-clockwise.
                    (turn - arc.end_angle, turn - arc.start_angle)
                } else {
                    (arc.start_angle + turn, arc.end_angle + turn)
                };
                Curve::Arc(Arc {
                    centre: point(arc.centre),
                    radius: arc.radius * by.similarity_scale(),
                    start_angle: start,
                    end_angle: end,
                })
            }

            // Uneven scale: what comes out is elliptical.
            Curve::Circle(circle) => Curve::Ellipse(EllipseArc::full(transformed_ellipse(
                &Ellipse {
                    centre: circle.centre,
                    major_radius: circle.radius,
                    minor_radius: circle.radius,
                    major_axis: [1.0, 0.0],
                },
                by,
            ))),
            Curve::Arc(arc) => elliptical_arc_from(
                &Ellipse {
                    centre: arc.centre,
                    major_radius: arc.radius,
                    minor_radius: arc.radius,
                    major_axis: [1.0, 0.0],
                },
                arc.start_angle,
                arc.start_angle + arc.sweep(),
                by,
            ),
            Curve::Ellipse(arc) => elliptical_arc_from(
                &arc.ellipse,
                arc.start_parameter,
                arc.start_parameter + arc.sweep(),
                by,
            ),

            Curve::Nurbs(curve) => {
                // A NURBS curve is a weighted average of its control points,
                // and an affine map commutes with that — so moving the control
                // points moves the curve, exactly, under any map at all.
                Curve::Nurbs(super::nurbs::NurbsCurve::new(
                    curve.degree(),
                    curve.control_points().iter().map(|p| point(*p)).collect(),
                    curve.knots().to_vec(),
                    Some(curve.weights().to_vec()),
                )?)
            }

            Curve::Polyline(polyline) => {
                let has_arcs = polyline.vertices.iter().any(|v| v.bulge.abs() > 1e-12);
                if has_arcs && !by.is_similarity() {
                    return None;
                }
                // A bulge is the tangent of a quarter of the included angle,
                // which a similarity leaves alone — but a mirror reverses which
                // side the arc bows to.
                let flip = if by.determinant() < 0.0 { -1.0 } else { 1.0 };
                Curve::Polyline(super::polyline::Polyline {
                    closed: polyline.closed,
                    vertices: polyline
                        .vertices
                        .iter()
                        .map(|v| super::polyline::PolylineVertex {
                            position: point(v.position),
                            bulge: v.bulge * flip,
                        })
                        .collect(),
                })
            }
        })
    }
}

/// The ellipse a map turns another ellipse into.
///
/// The images of the two semi-axes are *conjugate* semi-diameters of the
/// result — a pair that describes the ellipse but need not be its axes. The
/// angle below is the rotation that turns them into the pair that is.
fn transformed_ellipse(ellipse: &Ellipse, by: &Transform) -> Ellipse {
    let minor_axis = ellipse.minor_axis();
    let p = Vec2::from(by.apply_vector(
        (Vec2::from(ellipse.major_axis) * ellipse.major_radius).to_array(),
    ));
    let q = Vec2::from(by.apply_vector(
        (Vec2::from(minor_axis) * ellipse.minor_radius).to_array(),
    ));

    let shift = principal_shift(p, q);
    let (first, second) = principal_axes(p, q, shift);

    let (major, minor) = if first.length() >= second.length() {
        (first, second)
    } else {
        (second, first)
    };
    Ellipse {
        centre: by.apply_point(ellipse.centre),
        major_radius: major.length(),
        minor_radius: minor.length(),
        major_axis: major.normalize().unwrap_or(Vec2::new(1.0, 0.0)).to_array(),
    }
}

/// The parameter shift that turns a pair of conjugate semi-diameters into the
/// ellipse's actual axes.
fn principal_shift(p: Vec2, q: Vec2) -> f64 {
    let numerator = 2.0 * p.dot(q);
    let denominator = p.length_squared() - q.length_squared();
    if numerator.abs() < 1e-15 && denominator.abs() < 1e-15 {
        0.0
    } else {
        0.5 * numerator.atan2(denominator)
    }
}

fn principal_axes(p: Vec2, q: Vec2, shift: f64) -> (Vec2, Vec2) {
    let (sin, cos) = shift.sin_cos();
    let first = p * cos + q * sin;
    let (sin2, cos2) = (shift + FRAC_PI_2).sin_cos();
    let second = p * cos2 + q * sin2;
    (first, second)
}

/// A transformed elliptical arc, with its parameter range carried across.
fn elliptical_arc_from(
    ellipse: &Ellipse,
    start: f64,
    end: f64,
    by: &Transform,
) -> Curve {
    let result = transformed_ellipse(ellipse, by);

    // Find where the old endpoints land, then read their parameters off the
    // new ellipse. Doing it by evaluation rather than by tracking the angle
    // shift keeps the axis swap and the mirror case from needing their own
    // arithmetic.
    let parameter_of = |t: f64| {
        let world = by.apply_point(ellipse.point_at(t));
        let relative = Vec2::from(world) - Vec2::from(result.centre);
        let major = Vec2::from(result.major_axis);
        let along = relative.dot(major) / result.major_radius;
        let across = relative.dot(major.perpendicular()) / result.minor_radius;
        across.atan2(along)
    };

    let new_start = parameter_of(start);
    let new_end = parameter_of(end);
    // A mirror reverses the direction of travel, so the ends swap to keep the
    // arc sweeping the way this crate stores them.
    let (new_start, new_end) = if by.determinant() < 0.0 {
        (new_end, new_start)
    } else {
        (new_start, new_end)
    };

    Curve::Ellipse(EllipseArc {
        ellipse: result,
        start_parameter: new_start,
        end_parameter: if (new_end - new_start).rem_euclid(TAU) < 1e-12 {
            new_start + TAU
        } else {
            new_end
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::polyline::{Polyline, PolylineVertex};

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        Vec2::from(a).distance(Vec2::from(b)) < 1e-9
    }

    fn segment(start: [f64; 2], end: [f64; 2]) -> Curve {
        Curve::Line(Line { start, end })
    }

    /// Every curve must transform so that evaluating then moving equals moving
    /// then evaluating. This is the property the whole module exists to keep.
    fn agrees_pointwise(curve: &Curve, by: &Transform) {
        let moved = curve.transformed(by).expect("should transform");
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            let direct = by.apply_point(curve.point_at(t));
            let after = moved.point_at(moved.parameter_at(direct));
            assert!(
                near(after, direct),
                "at t = {t}: moving gave {direct:?}, the moved curve has {after:?}"
            );
        }
    }

    #[test]
    fn the_identity_leaves_points_where_they_are() {
        assert!(near(Transform::IDENTITY.apply_point([3.0, -4.0]), [3.0, -4.0]));
    }

    #[test]
    fn a_translation_moves_points_but_not_directions() {
        let by = Transform::translation([10.0, 5.0]);
        assert!(near(by.apply_point([1.0, 1.0]), [11.0, 6.0]));
        assert!(near(by.apply_vector([1.0, 1.0]), [1.0, 1.0]), "a direction has no place");
    }

    #[test]
    fn a_rotation_turns_the_axes() {
        let by = Transform::rotation(FRAC_PI_2);
        assert!(near(by.apply_vector([1.0, 0.0]), [0.0, 1.0]));
        assert!(near(by.apply_vector([0.0, 1.0]), [-1.0, 0.0]));
    }

    #[test]
    fn an_insertion_scales_about_the_block_origin_then_places_it() {
        // 2x about the block's own origin, then moved to (100, 0).
        let by = Transform::insertion([100.0, 0.0], [2.0, 2.0], 0.0);
        assert!(near(by.apply_point([0.0, 0.0]), [100.0, 0.0]));
        assert!(near(by.apply_point([1.0, 0.0]), [102.0, 0.0]));
    }

    #[test]
    fn composition_applies_the_first_map_first() {
        let scale = Transform::scale(2.0, 2.0);
        let move_it = Transform::translation([10.0, 0.0]);
        // Scale, then move: (1,0) doubles to (2,0), then shifts to (12,0).
        assert!(near(move_it.then_of(scale).apply_point([1.0, 0.0]), [12.0, 0.0]));
        // Move, then scale: (1,0) shifts to (11,0), then doubles to (22,0).
        assert!(near(scale.then_of(move_it).apply_point([1.0, 0.0]), [22.0, 0.0]));
    }

    #[test]
    fn a_similarity_is_recognised_and_an_uneven_scale_is_not() {
        assert!(Transform::IDENTITY.is_similarity());
        assert!(Transform::rotation(0.7).is_similarity());
        assert!(Transform::scale(3.0, 3.0).is_similarity());
        assert!(Transform::scale(-2.0, 2.0).is_similarity(), "a mirror still is");
        assert!(!Transform::scale(2.0, 1.0).is_similarity());
    }

    #[test]
    fn a_line_follows_any_map() {
        let line = segment([1.0, 2.0], [7.0, -3.0]);
        agrees_pointwise(&line, &Transform::insertion([5.0, 5.0], [2.0, 3.0], 0.4));
    }

    #[test]
    fn a_circle_stays_a_circle_under_an_even_scale() {
        let circle = Curve::Circle(Circle {
            centre: [1.0, 2.0],
            radius: 3.0,
        });
        let moved = circle
            .transformed(&Transform::insertion([10.0, 0.0], [2.0, 2.0], 0.5))
            .unwrap();
        match moved {
            Curve::Circle(c) => assert!((c.radius - 6.0).abs() < 1e-9),
            other => panic!("expected a circle, got {other:?}"),
        }
    }

    #[test]
    fn a_circle_becomes_an_ellipse_under_an_uneven_one() {
        let circle = Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 1.0,
        });
        let moved = circle.transformed(&Transform::scale(3.0, 1.0)).unwrap();
        match &moved {
            Curve::Ellipse(arc) => {
                assert!((arc.ellipse.major_radius - 3.0).abs() < 1e-9);
                assert!((arc.ellipse.minor_radius - 1.0).abs() < 1e-9);
            }
            other => panic!("expected an ellipse, got {other:?}"),
        }
        agrees_pointwise(&circle, &Transform::scale(3.0, 1.0));
    }

    #[test]
    fn an_arc_follows_an_even_scale_and_a_turn() {
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 5.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        });
        agrees_pointwise(&arc, &Transform::insertion([3.0, -2.0], [2.0, 2.0], 0.9));
    }

    #[test]
    fn an_arc_becomes_an_elliptical_arc_under_an_uneven_scale() {
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 2.0,
            start_angle: 0.2,
            end_angle: 2.0,
        });
        let by = Transform::scale(4.0, 1.0);
        assert!(matches!(arc.transformed(&by), Some(Curve::Ellipse(_))));
        agrees_pointwise(&arc, &by);
    }

    #[test]
    fn an_ellipse_follows_a_general_map() {
        let ellipse = Curve::Ellipse(EllipseArc::full(Ellipse {
            centre: [1.0, 1.0],
            major_radius: 5.0,
            minor_radius: 2.0,
            major_axis: [0.6, 0.8],
        }));
        agrees_pointwise(&ellipse, &Transform::insertion([2.0, 3.0], [1.5, 0.5], 0.3));
    }

    #[test]
    fn a_spline_follows_any_map_exactly() {
        let spline = Curve::Nurbs(
            crate::geom2d::nurbs::NurbsCurve::interpolate(
                &[[0.0, 0.0], [2.0, 4.0], [6.0, -2.0], [9.0, 3.0]],
                None,
                None,
                crate::geom2d::nurbs::Parameterization::Chord,
            )
            .unwrap(),
        );
        agrees_pointwise(&spline, &Transform::insertion([1.0, 1.0], [2.0, 0.5], 0.7));
    }

    #[test]
    fn a_straight_polyline_follows_any_map() {
        let chain = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([2.0, 0.0]),
                PolylineVertex::straight([2.0, 2.0]),
            ],
            closed: false,
        });
        agrees_pointwise(&chain, &Transform::scale(3.0, 0.5));
    }

    #[test]
    fn a_bulged_polyline_cannot_be_squashed() {
        let chain = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::curved([0.0, 0.0], 1.0),
                PolylineVertex::straight([2.0, 0.0]),
            ],
            closed: false,
        });
        // A bulge says "circular arc" and nothing else, so there is no honest
        // answer under an uneven scale.
        assert!(chain.transformed(&Transform::scale(3.0, 1.0)).is_none());
        // An even one is fine.
        agrees_pointwise(&chain, &Transform::insertion([5.0, 5.0], [2.0, 2.0], 0.6));
    }

    #[test]
    fn a_mirror_flips_which_way_a_bulge_bows() {
        let chain = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::curved([0.0, 0.0], 1.0),
                PolylineVertex::straight([2.0, 0.0]),
            ],
            closed: false,
        });
        let mirrored = chain.transformed(&Transform::scale(1.0, -1.0)).unwrap();
        // The original dips below the chord, so the mirrored one rises above.
        assert!(chain.point_at(0.5)[1] < 0.0);
        assert!(mirrored.point_at(0.5)[1] > 0.0);
    }

    #[test]
    fn a_ray_keeps_its_direction_through_a_turn() {
        let ray = Curve::Ray(Ray {
            origin: [1.0, 0.0],
            direction: [1.0, 0.0],
        });
        let moved = ray.transformed(&Transform::rotation(FRAC_PI_2)).unwrap();
        match moved {
            Curve::Ray(r) => {
                assert!(near(r.origin, [0.0, 1.0]));
                assert!(near(r.direction, [0.0, 1.0]));
            }
            other => panic!("expected a ray, got {other:?}"),
        }
    }

    #[test]
    fn survey_coordinates_transform_without_drift() {
        let origin = [512_345.678, 4_512_345.678];
        let line = segment(origin, [origin[0] + 10.0, origin[1]]);
        let by = Transform::translation([1000.0, 0.0]);
        let moved = line.transformed(&by).unwrap();
        assert!(near(moved.point_at(0.0), [origin[0] + 1000.0, origin[1]]));
        assert!(near(
            moved.point_at(1.0),
            [origin[0] + 1010.0, origin[1]]
        ));
    }
}
