//! Reject invalid numeric inputs before the backend starts an undo operation.
use crate::{ops::*, ApiError, ApiResult};

fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}
fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}
fn direction(value: &[f64; 3]) -> bool {
    finite(value) && value.iter().any(|v| *v != 0.0)
}
fn check(valid: bool, op: &'static str) -> ApiResult<()> {
    if valid {
        Ok(())
    } else {
        Err(ApiError::validation(
            op,
            "invalid or nonfinite geometry input",
        ))
    }
}

pub(crate) fn curve(spec: &Curve2Spec) -> ApiResult<()> {
    use Curve2Spec::*;
    let valid = match spec {
        Line { start, end } => finite(start) && finite(end) && start != end,
        Circle { centre, radius } => finite(centre) && positive(*radius),
        Point { position } => finite(position),
        Arc {
            centre,
            radius,
            start_angle,
            end_angle,
        } => finite(centre) && positive(*radius) && finite(&[*start_angle, *end_angle]),
        Polyline { points, closed } => {
            points.len() >= if *closed { 3 } else { 2 }
                && points.len() <= BULK_ITEM_CAP
                && points.iter().all(|p| finite(p) && p[2] == points[0][2])
        }
        Ellipse {
            centre,
            major_axis,
            ratio,
            start,
            end,
        } => {
            finite(centre)
                && direction(major_axis)
                && positive(*ratio)
                && *ratio <= 1.0
                && finite(&[*start, *end])
        }
        Spline {
            degree,
            control_points,
            knots,
            weights,
        } => {
            *degree > 0
                && control_points.len() <= BULK_ITEM_CAP
                && knots.len() <= BULK_ITEM_CAP
                && weights.len() <= BULK_ITEM_CAP
                && cadkernel::space::NurbsCurve3::new_strict(
                    *degree as usize,
                    control_points.clone(),
                    knots.clone(),
                    if weights.is_empty() {
                        vec![1.0; control_points.len()]
                    } else {
                        weights.clone()
                    },
                )
                .is_some()
        }
        Ray {
            origin,
            direction: d,
        }
        | XLine {
            origin,
            direction: d,
        } => finite(origin) && direction(d),
    };
    check(valid, "CreateCurve")
}

pub(crate) fn operation(op: &Operation) -> ApiResult<()> {
    use Operation::*;
    let valid = match op {
        CreateCurve(spec) => return curve(spec),
        Extrude { direction: d, .. } => direction(d),
        Revolve {
            axis: (pivot, d),
            angle,
            ..
        } => finite(pivot) && direction(d) && angle.is_finite() && *angle != 0.0,
        AddVertex { point, .. } => finite(point),
        CreateInsert(s) => {
            finite(&s.insert_point)
                && s.scale.is_finite()
                && s.scale != 0.0
                && s.rotation.is_finite()
        }
        CreateViewport(s) => {
            finite(&s.center)
                && finite(&s.view_target)
                && positive(s.width)
                && positive(s.height)
                && positive(s.view_height)
        }
        CreateText(s) => finite(&s.insertion_point) && positive(s.height) && s.rotation.is_finite(),
        CreateMText(s) => finite(&s.insertion_point) && positive(s.height),
        CreateHatch(s) => {
            (3..=BULK_ITEM_CAP).contains(&s.boundary.len()) && s.boundary.iter().all(|p| finite(p))
        }
        CreateDimensionLinear(s) => {
            finite(&s.first_point) && finite(&s.second_point) && finite(&s.definition_point)
        }
        CreateDimensionRadius(s) | CreateDimensionDiameter(s) => {
            finite(&s.center) && finite(&s.point)
        }
        CreateDimensionAngular(s) | CreateDimensionAngular2Ln(s) => {
            finite(&s.vertex)
                && finite(&s.first_point)
                && finite(&s.second_point)
                && finite(&s.arc_location)
        }
        CreateAttributeDefinition(s) => {
            finite(&s.insertion_point) && positive(s.height) && s.rotation.is_finite()
        }
        SetViewportView {
            view_target,
            view_height,
            ..
        } => finite(view_target) && positive(*view_height),
        CreateRasterImage(s) => {
            finite(&s.insertion_point)
                && direction(&s.u_vector)
                && direction(&s.v_vector)
                && s.size.iter().all(|v| positive(*v))
        }
        CreateTable(s) => {
            finite(&s.insertion_point)
                && !s.data.is_empty()
                && !s.data[0].is_empty()
                && s.data
                    .len()
                    .checked_mul(s.data[0].len())
                    .is_some_and(|n| n <= BULK_ITEM_CAP)
                && s.data.iter().all(|r| r.len() == s.data[0].len())
        }
        _ => true,
    };
    check(valid, op.op_name())
}

pub(crate) fn solid(spec: &SolidPrimitive) -> ApiResult<()> {
    use SolidPrimitive::*;
    let valid = match spec {
        Cuboid { origin, size } | Wedge { origin, size } => {
            finite(origin) && size.iter().all(|v| positive(*v))
        }
        Sphere { centre, radius } => finite(centre) && positive(*radius),
        Cylinder {
            base,
            radius,
            height,
        }
        | Cone {
            base,
            radius,
            height,
        } => finite(base) && positive(*radius) && positive(*height),
        Torus {
            centre,
            major_radius,
            minor_radius,
        } => finite(centre) && positive(*major_radius) && positive(*minor_radius),
    };
    check(valid, "CreateSolid")
}
