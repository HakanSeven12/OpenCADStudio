use super::Tolerance;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientFrame {
    pub center: [f64; 2],
    pub projection_min: f64,
    pub projection_span: f64,
    pub radius: f64,
}

pub fn gradient_frame(
    boundary: &[[f64; 2]],
    angle: f64,
    shift: f64,
    tolerance: Tolerance,
) -> Option<GradientFrame> {
    if !angle.is_finite() || !shift.is_finite() {
        return None;
    }

    let direction = [angle.cos(), angle.sin()];
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    let mut projection_min = f64::INFINITY;
    let mut projection_max = f64::NEG_INFINITY;

    for &[x, y] in boundary {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        min[0] = min[0].min(x);
        min[1] = min[1].min(y);
        max[0] = max[0].max(x);
        max[1] = max[1].max(y);
        let projection = x * direction[0] + y * direction[1];
        projection_min = projection_min.min(projection);
        projection_max = projection_max.max(projection);
    }

    if !projection_min.is_finite() {
        return None;
    }

    let base_center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
    let shift = shift.clamp(0.0, 1.0);
    let offset = [
        -(max[0] - min[0]) * 0.25 * shift,
        (max[1] - min[1]) * 0.25 * shift,
    ];
    let center = [base_center[0] + offset[0], base_center[1] + offset[1]];
    let radius = boundary
        .iter()
        .filter(|point| point[0].is_finite() && point[1].is_finite())
        .map(|point| (point[0] - center[0]).hypot(point[1] - center[1]))
        .fold(0.0_f64, f64::max)
        .max(tolerance.linear());

    Some(GradientFrame {
        center,
        projection_min: projection_min
            + offset[0] * direction[0]
            + offset[1] * direction[1],
        projection_span: (projection_max - projection_min).max(tolerance.linear()),
        radius,
    })
}
