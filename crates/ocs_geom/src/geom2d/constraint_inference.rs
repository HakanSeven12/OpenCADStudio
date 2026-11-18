//! Geometric relations that are already present in a set of drawing curves.

use super::{Arc, Circle, Line, Tolerance, Vec2};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParametricPrimitive {
    Line(Line),
    Circle(Circle),
    Arc(Arc),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InferenceKind {
    Coincident,
    Collinear,
    Parallel,
    Perpendicular,
    Tangent,
    Concentric,
    Horizontal,
    Vertical,
    Equal,
}

impl InferenceKind {
    pub const ALL: [Self; 9] = [
        Self::Coincident,
        Self::Collinear,
        Self::Parallel,
        Self::Perpendicular,
        Self::Tangent,
        Self::Concentric,
        Self::Horizontal,
        Self::Vertical,
        Self::Equal,
    ];
}

/// Controls which geometric relations inference returns and how close input
/// geometry must already be to those relations.
#[derive(Debug, Clone, PartialEq)]
pub struct InferenceSettings {
    /// Enabled relationship kinds in application-priority order.
    pub priority: Vec<InferenceKind>,
    /// World-space tolerance used by point and curve-distance comparisons.
    pub distance_tolerance: f64,
    /// Angular tolerance in radians.
    pub angle_tolerance_radians: f64,
    /// Require a line/circle tangency point to lie on both bounded curves.
    pub tangent_must_share_point: bool,
    /// Require perpendicular line segments to meet within distance tolerance.
    pub perpendicular_must_intersect: bool,
}

impl Default for InferenceSettings {
    fn default() -> Self {
        Self {
            priority: InferenceKind::ALL.to_vec(),
            distance_tolerance: 0.05,
            angle_tolerance_radians: 1.0_f64.to_radians(),
            tangent_must_share_point: true,
            perpendicular_must_intersect: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InferredConstraint {
    Coincident {
        first: usize,
        first_endpoint: Endpoint,
        second: usize,
        second_endpoint: Endpoint,
    },
    Collinear { first: usize, second: usize },
    Concentric { first: usize, second: usize },
    Parallel { first: usize, second: usize },
    Perpendicular { first: usize, second: usize },
    Horizontal { entity: usize },
    Vertical { entity: usize },
    Tangent { first: usize, second: usize },
    Equal { first: usize, second: usize },
}

impl InferredConstraint {
    pub const fn kind(self) -> InferenceKind {
        match self {
            Self::Coincident { .. } => InferenceKind::Coincident,
            Self::Collinear { .. } => InferenceKind::Collinear,
            Self::Concentric { .. } => InferenceKind::Concentric,
            Self::Parallel { .. } => InferenceKind::Parallel,
            Self::Perpendicular { .. } => InferenceKind::Perpendicular,
            Self::Horizontal { .. } => InferenceKind::Horizontal,
            Self::Vertical { .. } => InferenceKind::Vertical,
            Self::Tangent { .. } => InferenceKind::Tangent,
            Self::Equal { .. } => InferenceKind::Equal,
        }
    }
}

fn endpoints(primitive: ParametricPrimitive) -> Option<[[f64; 2]; 2]> {
    match primitive {
        ParametricPrimitive::Line(line) => Some([line.start, line.end]),
        ParametricPrimitive::Arc(arc) => Some([
            [
                arc.centre[0] + arc.radius * arc.start_angle.cos(),
                arc.centre[1] + arc.radius * arc.start_angle.sin(),
            ],
            [
                arc.centre[0] + arc.radius * arc.end_angle.cos(),
                arc.centre[1] + arc.radius * arc.end_angle.sin(),
            ],
        ]),
        ParametricPrimitive::Circle(_) => None,
    }
}

fn circle(primitive: ParametricPrimitive) -> Option<Circle> {
    match primitive {
        ParametricPrimitive::Circle(circle) => Some(circle),
        ParametricPrimitive::Arc(arc) => Some(Circle {
            centre: arc.centre,
            radius: arc.radius,
        }),
        ParametricPrimitive::Line(_) => None,
    }
}

fn unit_line(line: Line) -> Option<Vec2> {
    Vec2::from(line.direction()).normalize()
}

fn line_distance(line: Line, point: [f64; 2]) -> Option<f64> {
    let direction = unit_line(line)?;
    Some(
        (Vec2::from(point) - Vec2::from(line.start))
            .cross(direction)
            .abs(),
    )
}

fn line_length(line: Line) -> f64 {
    Vec2::from(line.start).distance(Vec2::from(line.end))
}

fn same_measure(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn parameter_on_line(line: Line, point: [f64; 2]) -> Option<f64> {
    let d = Vec2::from(line.end) - Vec2::from(line.start);
    let length_squared = d.dot(d);
    (length_squared > 1e-24)
        .then(|| (Vec2::from(point) - Vec2::from(line.start)).dot(d) / length_squared)
}

fn line_intersection_parameters(a: Line, b: Line) -> Option<(f64, f64)> {
    let p = Vec2::from(a.start);
    let q = Vec2::from(b.start);
    let r = Vec2::from(a.end) - p;
    let s = Vec2::from(b.end) - q;
    let denominator = r.cross(s);
    if denominator.abs() <= 1e-20 {
        return None;
    }
    let qp = q - p;
    Some((qp.cross(s) / denominator, qp.cross(r) / denominator))
}

fn parameter_slack(line: Line, distance: f64) -> f64 {
    let length = line_length(line);
    if length > 1e-20 {
        distance / length
    } else {
        0.0
    }
}

fn segments_intersect_within(a: Line, b: Line, distance: f64) -> bool {
    line_intersection_parameters(a, b).is_some_and(|(ta, tb)| {
        let sa = parameter_slack(a, distance);
        let sb = parameter_slack(b, distance);
        ta >= -sa && ta <= 1.0 + sa && tb >= -sb && tb <= 1.0 + sb
    })
}

fn line_circle_contact(line: Line, circle: Circle) -> Option<([f64; 2], f64)> {
    let t = parameter_on_line(line, circle.centre)?;
    let start = Vec2::from(line.start);
    let direction = Vec2::from(line.end) - start;
    Some(((start + direction * t).to_array(), t))
}

fn line_circle_contact_on_segment(line: Line, circle: Circle, distance: f64) -> bool {
    let Some((_, t)) = line_circle_contact(line, circle) else {
        return false;
    };
    let slack = parameter_slack(line, distance);
    t >= -slack && t <= 1.0 + slack
}

fn primitive_contains_circle_point(
    primitive: ParametricPrimitive,
    point: [f64; 2],
    distance: f64,
) -> bool {
    let ParametricPrimitive::Arc(arc) = primitive else {
        return true;
    };
    let delta = Vec2::from(point) - Vec2::from(arc.centre);
    if super::angle::angle_within_arc(delta.y.atan2(delta.x), arc.start_angle, arc.end_angle) {
        return true;
    }
    endpoints(primitive).is_some_and(|ends| {
        ends.into_iter()
            .any(|end| Vec2::from(end).distance(Vec2::from(point)) <= distance)
    })
}

fn circle_circle_contact(a: Circle, b: Circle, distance: f64) -> Option<[f64; 2]> {
    let ca = Vec2::from(a.centre);
    let cb = Vec2::from(b.centre);
    let delta = cb - ca;
    let centres = delta.length();
    if centres <= 1e-20 {
        return None;
    }
    let direction = delta / centres;
    if (centres - (a.radius + b.radius)).abs() <= distance {
        return Some((ca + direction * a.radius).to_array());
    }
    if (centres - (a.radius - b.radius).abs()).abs() <= distance {
        let sign = if a.radius >= b.radius { 1.0 } else { -1.0 };
        return Some((ca + direction * (sign * a.radius)).to_array());
    }
    None
}

fn pair(item: InferredConstraint) -> Option<(usize, usize)> {
    match item {
        InferredConstraint::Collinear { first, second }
        | InferredConstraint::Concentric { first, second }
        | InferredConstraint::Parallel { first, second }
        | InferredConstraint::Perpendicular { first, second }
        | InferredConstraint::Tangent { first, second }
        | InferredConstraint::Equal { first, second } => Some((first, second)),
        _ => None,
    }
}

fn axis_state(candidates: &[InferredConstraint], entity: usize) -> (bool, bool) {
    candidates
        .iter()
        .fold((false, false), |(horizontal, vertical), item| match *item {
            InferredConstraint::Horizontal { entity: found } if found == entity => {
                (true, vertical)
            }
            InferredConstraint::Vertical { entity: found } if found == entity => {
                (horizontal, true)
            }
            _ => (horizontal, vertical),
        })
}

/// Returns constraints already implied by the input, respecting the enabled
/// kinds and priority. Redundant pair relations are omitted when individual
/// axis relations already imply them.
pub fn infer_constraints_with_settings(
    primitives: &[ParametricPrimitive],
    settings: &InferenceSettings,
) -> Vec<InferredConstraint> {
    let distance = settings.distance_tolerance;
    let angle = settings.angle_tolerance_radians;
    if !distance.is_finite() || distance < 0.0 || !angle.is_finite() || angle < 0.0 {
        return Vec::new();
    }
    let sin_angle = angle.sin().abs();
    let mut candidates = Vec::new();

    for (index, primitive) in primitives.iter().copied().enumerate() {
        if let ParametricPrimitive::Line(line) = primitive {
            if let Some(direction) = unit_line(line) {
                if direction.y.abs() <= sin_angle {
                    candidates.push(InferredConstraint::Horizontal { entity: index });
                }
                if direction.x.abs() <= sin_angle {
                    candidates.push(InferredConstraint::Vertical { entity: index });
                }
            }
        }
    }

    for first in 0..primitives.len() {
        for second in first + 1..primitives.len() {
            if let (Some(a), Some(b)) =
                (endpoints(primitives[first]), endpoints(primitives[second]))
            {
                for (ai, ap) in a.into_iter().enumerate() {
                    for (bi, bp) in b.into_iter().enumerate() {
                        if Vec2::from(ap).distance(Vec2::from(bp)) <= distance {
                            candidates.push(InferredConstraint::Coincident {
                                first,
                                first_endpoint: if ai == 0 {
                                    Endpoint::Start
                                } else {
                                    Endpoint::End
                                },
                                second,
                                second_endpoint: if bi == 0 {
                                    Endpoint::Start
                                } else {
                                    Endpoint::End
                                },
                            });
                        }
                    }
                }
            }

            if let (ParametricPrimitive::Line(a), ParametricPrimitive::Line(b)) =
                (primitives[first], primitives[second])
            {
                if let (Some(ua), Some(ub)) = (unit_line(a), unit_line(b)) {
                    let cross = ua.cross(ub).abs();
                    if cross <= sin_angle {
                        if line_distance(a, b.start).is_some_and(|value| value <= distance) {
                            candidates.push(InferredConstraint::Collinear { first, second });
                        } else {
                            candidates.push(InferredConstraint::Parallel { first, second });
                        }
                    } else if ua.dot(ub).abs() <= sin_angle
                        && (!settings.perpendicular_must_intersect
                            || segments_intersect_within(a, b, distance))
                    {
                        candidates.push(InferredConstraint::Perpendicular { first, second });
                    }
                    if same_measure(line_length(a), line_length(b)) {
                        candidates.push(InferredConstraint::Equal { first, second });
                    }
                }
            }

            match (circle(primitives[first]), circle(primitives[second])) {
                (Some(a), Some(b)) => {
                    let centres = Vec2::from(a.centre).distance(Vec2::from(b.centre));
                    if centres <= distance {
                        candidates.push(InferredConstraint::Concentric { first, second });
                    } else if circle_circle_contact(a, b, distance).is_some_and(|point| {
                        !settings.tangent_must_share_point
                            || (primitive_contains_circle_point(
                                primitives[first],
                                point,
                                distance,
                            ) && primitive_contains_circle_point(
                                primitives[second],
                                point,
                                distance,
                            ))
                    }) {
                        candidates.push(InferredConstraint::Tangent { first, second });
                    }
                    if same_measure(a.radius, b.radius) {
                        candidates.push(InferredConstraint::Equal { first, second });
                    }
                }
                (Some(circle), None)
                    if matches!(primitives[second], ParametricPrimitive::Line(_)) =>
                {
                    let ParametricPrimitive::Line(line) = primitives[second] else {
                        unreachable!()
                    };
                    if line_distance(line, circle.centre)
                        .is_some_and(|value| (value - circle.radius).abs() <= distance)
                        && (!settings.tangent_must_share_point
                            || line_circle_contact(line, circle).is_some_and(|(point, _)| {
                                line_circle_contact_on_segment(line, circle, distance)
                                    && primitive_contains_circle_point(
                                        primitives[first],
                                        point,
                                        distance,
                                    )
                            }))
                    {
                        candidates.push(InferredConstraint::Tangent { first, second });
                    }
                }
                (None, Some(circle))
                    if matches!(primitives[first], ParametricPrimitive::Line(_)) =>
                {
                    let ParametricPrimitive::Line(line) = primitives[first] else {
                        unreachable!()
                    };
                    if line_distance(line, circle.centre)
                        .is_some_and(|value| (value - circle.radius).abs() <= distance)
                        && (!settings.tangent_must_share_point
                            || line_circle_contact(line, circle).is_some_and(|(point, _)| {
                                line_circle_contact_on_segment(line, circle, distance)
                                    && primitive_contains_circle_point(
                                        primitives[second],
                                        point,
                                        distance,
                                    )
                            }))
                    {
                        candidates.push(InferredConstraint::Tangent { first, second });
                    }
                }
                _ => {}
            }
        }
    }

    // Disabled axis constraints cannot make an enabled pair relation
    // redundant: they will not be returned to the caller.
    candidates.retain(|candidate| settings.priority.contains(&candidate.kind()));
    let axis_candidates = candidates.clone();
    candidates.retain(|candidate| {
        let Some((first, second)) = pair(*candidate) else {
            return true;
        };
        let (first_h, first_v) = axis_state(&axis_candidates, first);
        let (second_h, second_v) = axis_state(&axis_candidates, second);
        match candidate {
            InferredConstraint::Collinear { .. } | InferredConstraint::Parallel { .. } => {
                !((first_h && second_h) || (first_v && second_v))
            }
            InferredConstraint::Perpendicular { .. } => {
                !((first_h && second_v) || (first_v && second_h))
            }
            _ => true,
        }
    });

    let mut result = Vec::new();
    for kind in &settings.priority {
        result.extend(
            candidates
                .iter()
                .copied()
                .filter(|item| item.kind() == *kind),
        );
    }
    result
}

/// Compatibility entry point using all supported relationship kinds and the
/// supplied tolerances.
pub fn infer_constraints(
    primitives: &[ParametricPrimitive],
    distance_tolerance: Tolerance,
    angle_tolerance_radians: f64,
) -> Vec<InferredConstraint> {
    infer_constraints_with_settings(
        primitives,
        &InferenceSettings {
            distance_tolerance: distance_tolerance.linear(),
            angle_tolerance_radians,
            ..InferenceSettings::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relations_are_in_priority_order_without_redundant_collinearity() {
        let curves = [
            ParametricPrimitive::Line(Line {
                start: [0.0, 0.0],
                end: [2.0, 0.0],
            }),
            ParametricPrimitive::Line(Line {
                start: [2.0, 0.0],
                end: [4.0, 0.0],
            }),
        ];
        let found = infer_constraints(&curves, Tolerance::new(1e-6), 1e-6);
        assert!(matches!(
            found[0],
            InferredConstraint::Coincident { .. }
        ));
        assert!(!found
            .iter()
            .any(|item| matches!(item, InferredConstraint::Collinear { .. })));
        assert_eq!(
            found
                .iter()
                .filter(|item| matches!(item, InferredConstraint::Horizontal { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn bounded_tangency_respects_shared_point_setting() {
        let curves = [
            ParametricPrimitive::Circle(Circle {
                centre: [0.0, 0.0],
                radius: 2.0,
            }),
            ParametricPrimitive::Line(Line {
                start: [10.0, 2.0],
                end: [12.0, 2.0],
            }),
        ];
        let strict = infer_constraints_with_settings(&curves, &InferenceSettings::default());
        assert!(!strict
            .iter()
            .any(|item| matches!(item, InferredConstraint::Tangent { .. })));
        let loose = infer_constraints_with_settings(
            &curves,
            &InferenceSettings {
                tangent_must_share_point: false,
                ..InferenceSettings::default()
            },
        );
        assert!(loose
            .iter()
            .any(|item| matches!(item, InferredConstraint::Tangent { .. })));
    }

    #[test]
    fn disabled_axes_do_not_suppress_enabled_pair_relations() {
        let curves = [
            ParametricPrimitive::Line(Line {
                start: [0.0, 0.0],
                end: [2.0, 0.0],
            }),
            ParametricPrimitive::Line(Line {
                start: [3.0, 0.0],
                end: [5.0, 0.0],
            }),
        ];
        let found = infer_constraints_with_settings(
            &curves,
            &InferenceSettings {
                priority: vec![InferenceKind::Collinear],
                ..InferenceSettings::default()
            },
        );
        assert_eq!(
            found,
            [InferredConstraint::Collinear {
                first: 0,
                second: 1,
            }]
        );
    }
}
