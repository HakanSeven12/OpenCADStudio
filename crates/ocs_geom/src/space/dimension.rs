//! Spatial constructions used to edit and render dimensions.

use super::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimensionSpacingFrame {
    pub perpendicular: [f64; 3],
    pub coordinate: f64,
}

/// Create the perpendicular coordinate frame used to space parallel dimensions.
pub fn dimension_spacing_frame(
    axis: [f64; 3],
    normal: [f64; 3],
    definition: [f64; 3],
) -> Option<DimensionSpacingFrame> {
    let axis = Vec3::from(axis).normalize()?;
    let normal = Vec3::from(normal).normalize().unwrap_or(Vec3::Z);
    let perpendicular = normal.cross(axis).normalize()?;
    Some(DimensionSpacingFrame {
        perpendicular: perpendicular.into(),
        coordinate: Vec3::from(definition).dot(perpendicular),
    })
}

/// Move a dimension definition point to a target perpendicular coordinate.
pub fn move_to_dimension_spacing(
    point: [f64; 3],
    frame: DimensionSpacingFrame,
    target: f64,
) -> [f64; 3] {
    let point = Vec3::from(point);
    let perpendicular = Vec3::from(frame.perpendicular);
    (point + perpendicular * (target - point.dot(perpendicular))).into()
}

/// Choose the default jog position between the first extension point and text.
pub fn default_dimension_jog_position(
    first: [f64; 3],
    second: [f64; 3],
    definition: [f64; 3],
    axis: [f64; 3],
    normal: [f64; 3],
    text: [f64; 3],
) -> Option<[f64; 3]> {
    let axis = Vec3::from(axis).normalize()?;
    let normal = Vec3::from(normal).normalize().unwrap_or(Vec3::Z);
    let perpendicular = normal.cross(axis).normalize()?;
    let definition = Vec3::from(definition);
    let project = |point: Vec3| point + perpendicular * ((definition - point).dot(perpendicular));
    let first = project(Vec3::from(first));
    let second = project(Vec3::from(second));
    let midpoint = (first + second) * 0.5;
    let text = Vec3::from(text);
    let line = second - first;
    let squared = line.length_squared();
    if squared <= 1.0e-18 {
        return Some(midpoint.into());
    }
    let parameter = (text - first).dot(line) / squared;
    Some(if text.is_finite() && (0.0..=1.0).contains(&parameter) {
        ((first + text) * 0.5).into()
    } else {
        midpoint.into()
    })
}

/// Find the segment and point nearest to a requested point.
pub fn nearest_segment_point(
    segments: &[[[f64; 3]; 2]],
    requested: [f64; 3],
) -> Option<(usize, [f64; 3])> {
    let requested = Vec3::from(requested);
    segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            let first = Vec3::from(segment[0]);
            let second = Vec3::from(segment[1]);
            let delta = second - first;
            let squared = delta.length_squared();
            if squared <= 1.0e-18 {
                return None;
            }
            let parameter = ((requested - first).dot(delta) / squared).clamp(0.0, 1.0);
            let point = first + delta * parameter;
            Some((index, point, point.distance_squared(requested)))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(index, point, _)| (index, point.into()))
}

/// Build the four points of a jog on a dimension segment.
pub fn dimension_jog_points(
    segment: [[f64; 3]; 2],
    center: [f64; 3],
    normal: [f64; 3],
    size: f64,
    angle: f64,
) -> Option<[[f64; 3]; 4]> {
    let first = Vec3::from(segment[0]);
    let second = Vec3::from(segment[1]);
    let direction = (second - first).normalize()?;
    let normal = Vec3::from(normal).normalize().unwrap_or(Vec3::Z);
    let perpendicular = normal.cross(direction).normalize()?;
    let center = Vec3::from(center);
    let half = size.max(1.0e-3).min(first.distance(second) * 0.2);
    let amplitude = (half * (angle * 0.5).tan().abs()).clamp(half * 0.35, half * 1.5);
    Some([
        (center - direction * half).into(),
        (center - direction * half * 0.25 + perpendicular * amplitude).into(),
        (center + direction * half * 0.25 - perpendicular * amplitude).into(),
        (center + direction * half).into(),
    ])
}

/// Compute a centered gap where two finite segments cross in the XY plane.
pub fn segment_break_gap_xy(
    first: [[f64; 3]; 2],
    second: [[f64; 3]; 2],
    maximum_half_gap: f64,
) -> Option<[[f64; 3]; 2]> {
    let start = Vec3::from(first[0]);
    let direction = Vec3::from(first[1]) - start;
    let crossing_start = Vec3::from(second[0]);
    let crossing_direction = Vec3::from(second[1]) - crossing_start;
    let cross_xy = |left: Vec3, right: Vec3| left.x * right.y - left.y * right.x;
    let denominator = cross_xy(direction, crossing_direction);
    if denominator.abs() <= 1.0e-12 {
        return None;
    }
    let between = crossing_start - start;
    let first_parameter = cross_xy(between, crossing_direction) / denominator;
    let second_parameter = cross_xy(between, direction) / denominator;
    if !(-1.0e-9..=1.0 + 1.0e-9).contains(&first_parameter)
        || !(-1.0e-9..=1.0 + 1.0e-9).contains(&second_parameter)
    {
        return None;
    }
    let unit = direction.normalize()?;
    let center = start + direction * first_parameter.clamp(0.0, 1.0);
    let half_gap = maximum_half_gap.min(direction.length() * 0.2);
    Some([
        (center - unit * half_gap).into(),
        (center + unit * half_gap).into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_gap_is_centered_on_the_crossing() {
        let gap = segment_break_gap_xy(
            [[0.0, 0.0, 2.0], [10.0, 0.0, 2.0]],
            [[5.0, -1.0, 0.0], [5.0, 1.0, 0.0]],
            0.25,
        )
        .unwrap();
        assert_eq!(gap, [[4.75, 0.0, 2.0], [5.25, 0.0, 2.0]]);
    }

    #[test]
    fn spacing_uses_the_dimension_plane() {
        let frame =
            dimension_spacing_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 4.0]).unwrap();
        assert_eq!(
            move_to_dimension_spacing([2.0, 3.0, 0.0], frame, 6.0),
            [2.0, 3.0, -6.0]
        );
    }

    #[test]
    fn an_unpositioned_text_marker_uses_the_dimension_midpoint() {
        let point = default_dimension_jog_position(
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [f64::NAN; 3],
        )
        .unwrap();
        assert_eq!(point, [5.0, 2.0, 0.0]);
    }
}
