//! Leader geometry for dimension annotations.

use super::{Frame, Line, Vec2};

/// Connect an interior text position to the dimension line, or extend the near
/// end under exterior text. The boolean identifies an exterior extension.
pub fn linear_dimension_leader(
    measured: Line,
    definition: [f64; 2],
    axis: [f64; 2],
    text: [f64; 2],
    text_half_extents: [f64; 2],
    text_rotation: f64,
) -> Option<(Line, bool)> {
    if [
        measured.start,
        measured.end,
        definition,
        axis,
        text,
        text_half_extents,
    ]
    .into_iter()
    .flatten()
    .any(|value| !value.is_finite())
        || !text_rotation.is_finite()
        || text_half_extents.into_iter().any(|value| value < 0.0)
    {
        return None;
    }
    let axis = Vec2::from(axis).normalize()?;
    let frame = Frame::at([definition[0], definition[1], 0.0]);
    let first = Vec2::from(frame.lift_2d(measured.start)).dot(axis);
    let second = Vec2::from(frame.lift_2d(measured.end)).dot(axis);
    let text = Vec2::from(frame.lift_2d(text));
    let along = text.dot(axis);
    let (min, max) = (first.min(second), first.max(second));
    let text_axis = Vec2::new(text_rotation.cos(), text_rotation.sin());
    let half = text_half_extents[0] * axis.dot(text_axis).abs()
        + text_half_extents[1] * axis.dot(text_axis.perpendicular()).abs();
    let (start, end, under_text) = if along > max {
        (axis * max, axis * (along + half), true)
    } else if along < min {
        (axis * min, axis * (along - half), true)
    } else {
        (axis * along, text, false)
    };
    Some((
        Line {
            start: frame.lower_2d(start.to_array()),
            end: frame.lower_2d(end.to_array()),
        },
        under_text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leader_tracks_text_placement_rotation_and_large_origins() {
        for origin in [[0.0, 0.0], [500_000.0, 4_500_000.0]] {
            for angle in [0.0_f64, 0.6, std::f64::consts::FRAC_PI_2] {
                let axis = Vec2::new(angle.cos(), angle.sin());
                let normal = axis.perpendicular();
                let at = |x, y| (Vec2::from(origin) + axis * x + normal * y).to_array();
                for reverse in [false, true] {
                    let measured = if reverse {
                        Line {
                            start: at(20.0, 0.0),
                            end: at(0.0, 0.0),
                        }
                    } else {
                        Line {
                            start: at(0.0, 0.0),
                            end: at(20.0, 0.0),
                        }
                    };
                    for (x, rotation, expected_x, under) in [
                        (30.0, angle, 33.0, true),
                        (-12.0, angle, -15.0, true),
                        (10.0, angle, 10.0, false),
                        (30.0, angle + std::f64::consts::FRAC_PI_2, 31.0, true),
                    ] {
                        let (leader, under_text) = linear_dimension_leader(
                            measured,
                            at(0.0, -5.0),
                            axis.to_array(),
                            at(x, -3.0),
                            [3.0, 1.0],
                            rotation,
                        )
                        .unwrap();
                        assert_eq!(under_text, under);
                        let expected_start = at(x.clamp(0.0, 20.0), -5.0);
                        let expected_end = at(expected_x, if under { -5.0 } else { -3.0 });
                        assert!(
                            Vec2::from(leader.start).distance(Vec2::from(expected_start)) < 1.0e-8
                        );
                        assert!(Vec2::from(leader.end).distance(Vec2::from(expected_end)) < 1.0e-8);
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_leader_inputs_are_rejected() {
        let line = Line {
            start: [0.0, 0.0],
            end: [20.0, 0.0],
        };
        assert!(linear_dimension_leader(
            line,
            [0.0, -5.0],
            [0.0, 0.0],
            [30.0, -3.0],
            [3.0, 1.0],
            0.0
        )
        .is_none());
        assert!(linear_dimension_leader(
            line,
            [0.0, -5.0],
            [1.0, 0.0],
            [f64::NAN, -3.0],
            [3.0, 1.0],
            0.0
        )
        .is_none());
        assert!(linear_dimension_leader(
            line,
            [0.0, -5.0],
            [1.0, 0.0],
            [30.0, -3.0],
            [-3.0, 1.0],
            0.0
        )
        .is_none());
    }
}
