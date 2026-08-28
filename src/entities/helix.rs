use acadrust::entities::{Helix, HelixConstraint, Spline};
use acadrust::types::Vector3;
use cadkernel::space::{HelixCurve, HelixDirection, NurbsCurve3, Vec3};
use glam::DVec3;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_prop as edit, format_angle, format_length, parse_f64, ro_prop as ro,
    square_grip,
};
use crate::entities::traits::{Grippable, RenderConvertible, Transformable};
use crate::scene::convert::acad_to_render::RenderEntity;
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::t;

const EPSILON: f64 = 1.0e-9;

fn point(vector: Vector3) -> Vec3 {
    Vec3::new(vector.x, vector.y, vector.z)
}

fn vector(value: Vec3) -> Vector3 {
    Vector3::new(value.x, value.y, value.z)
}

fn display(value: Vec3) -> DVec3 {
    DVec3::new(value.x, value.y, value.z)
}

fn kernel(value: DVec3) -> Vec3 {
    Vec3::new(value.x, value.y, value.z)
}

fn finite(value: Vec3) -> bool {
    value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

fn spline_curve(spline: &Spline) -> Option<NurbsCurve3> {
    let controls: Vec<[f64; 3]> = spline
        .control_points
        .iter()
        .map(|value| [value.x, value.y, value.z])
        .collect();
    let degree = usize::try_from(spline.degree).ok()?;
    let weights = if spline.weights.is_empty() {
        vec![1.0; controls.len()]
    } else {
        spline.weights.clone()
    };
    NurbsCurve3::new_strict(degree, controls, spline.knots.clone(), weights)
}

fn axis_frame(helix: &Helix) -> Option<(Vec3, Vec3, Vec3)> {
    let base = point(helix.axis_base_point);
    let axis = point(helix.axis_vector).normalize()?;
    let radial = point(helix.start_point) - base;
    let start_direction = (radial - axis * radial.dot(axis)).normalize()?;
    Some((base, axis, start_direction))
}

fn radius_from_axis(helix: &Helix, value: Vec3) -> Option<f64> {
    let (base, axis, _) = axis_frame(helix)?;
    let delta = value - base;
    Some((delta - axis * delta.dot(axis)).length())
}

fn top_radius(helix: &Helix) -> f64 {
    spline_curve(&helix.spline)
        .map(|curve| Vec3::from(curve.point_at(1.0)))
        .and_then(|endpoint| radius_from_axis(helix, endpoint))
        .unwrap_or(helix.radius)
}

fn kernel_curve(helix: &Helix, top_radius: f64) -> Option<HelixCurve> {
    let (base, axis, start_direction) = axis_frame(helix)?;
    Some(HelixCurve {
        base_center: base.to_array(),
        axis_direction: axis.to_array(),
        start_direction: start_direction.to_array(),
        base_radius: helix.radius,
        top_radius,
        height: helix.turns * helix.turn_height,
        turns: helix.turns,
        direction: if helix.handedness {
            HelixDirection::CounterClockwise
        } else {
            HelixDirection::Clockwise
        },
    })
}

fn projected_direction(direction: Vec3, axis: Vec3) -> Option<Vec3> {
    (direction - axis * direction.dot(axis)).normalize()
}

fn perpendicular(axis: Vec3) -> Option<Vec3> {
    let basis = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vec3::X
    } else if axis.y.abs() <= axis.z.abs() {
        Vec3::Y
    } else {
        Vec3::Z
    };
    projected_direction(basis, axis)
}

fn rebuild(helix: &mut Helix, top_radius: f64) -> bool {
    let Some(curve) = kernel_curve(helix, top_radius) else {
        return false;
    };
    let Some(nurbs) = curve.nurbs() else {
        return false;
    };
    helix.spline.degree = nurbs.degree() as i32;
    helix.spline.knots = nurbs.knots().to_vec();
    helix.spline.control_points = nurbs
        .control_points()
        .iter()
        .map(|value| Vector3::new(value[0], value[1], value[2]))
        .collect();
    helix.spline.weights = if nurbs.is_rational() {
        nurbs.weights().to_vec()
    } else {
        Vec::new()
    };
    helix.spline.fit_points.clear();
    helix.spline.flags.closed = false;
    helix.spline.flags.periodic = false;
    helix.spline.flags.rational = nurbs.is_rational();
    helix.spline.flags.planar = false;
    helix.spline.flags.linear = false;
    true
}

fn choice(label: &str, field: &'static str, selected: &str, options: &[String]) -> Property {
    Property {
        label: label.to_string(),
        field,
        value: PropValue::Choice {
            selected: selected.to_string(),
            options: options.to_vec(),
        },
    }
}

fn grips(helix: &Helix) -> Vec<GripDef> {
    let Some((base, axis, _)) = axis_frame(helix) else {
        return Vec::new();
    };
    let height = helix.turns * helix.turn_height;
    let top_center = base + axis * height;
    let top_endpoint = spline_curve(&helix.spline)
        .map(|curve| Vec3::from(curve.point_at(1.0)))
        .unwrap_or(top_center);
    vec![
        center_grip(0, display(base)),
        square_grip(1, display(point(helix.start_point))),
        square_grip(2, display(top_center)),
        square_grip(3, display(top_endpoint)),
    ]
}

fn properties(helix: &Helix) -> Vec<PropSection> {
    let base = point(helix.axis_base_point);
    let height = helix.turns * helix.turn_height;
    let top_radius = top_radius(helix);
    let curve = kernel_curve(helix, top_radius);
    let turn_slope = curve.as_ref().and_then(HelixCurve::turn_slope).unwrap_or(0.0);
    let total_length = curve.as_ref().and_then(HelixCurve::length).unwrap_or(0.0);
    let constrain = match helix.constraint {
        HelixConstraint::TurnHeight => t!("Turn Height").into_owned(),
        HelixConstraint::Turns => t!("Turns").into_owned(),
        HelixConstraint::Height => t!("Height").into_owned(),
    };
    let twist = if helix.handedness {
        t!("CCW").into_owned()
    } else {
        t!("CW").into_owned()
    };
    let constraint_options = vec![
        t!("Turn Height").into_owned(),
        t!("Turns").into_owned(),
        t!("Height").into_owned(),
    ];
    let twist_options = vec![t!("CW").into_owned(), t!("CCW").into_owned()];

    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            edit(t!("Position X").as_ref(), "position_x", base.x),
            edit(t!("Position Y").as_ref(), "position_y", base.y),
            edit(t!("Position Z").as_ref(), "position_z", base.z),
            choice(t!("Constrain").as_ref(), "constrain", &constrain, &constraint_options),
            edit(t!("Height").as_ref(), "height", height),
            edit(t!("Turns").as_ref(), "turns", helix.turns),
            edit(t!("Turn height").as_ref(), "turn_height", helix.turn_height),
            edit(t!("Base radius").as_ref(), "base_radius", helix.radius),
            edit(t!("Top radius").as_ref(), "top_radius", top_radius),
            choice(t!("Twist").as_ref(), "twist", &twist, &twist_options),
            ro(t!("Turn slope").as_ref(), "turn_slope", format_angle(turn_slope)),
            ro(t!("Total length").as_ref(), "total_length", format_length(total_length)),
        ],
    }]
}

fn apply_geom_prop(helix: &mut Helix, field: &str, value: &str) {
    if field == "constrain" {
        let constraint = if value == t!("Height").as_ref() {
            HelixConstraint::Height
        } else if value == t!("Turns").as_ref() {
            HelixConstraint::Turns
        } else if value == t!("Turn Height").as_ref() {
            HelixConstraint::TurnHeight
        } else {
            return;
        };
        helix.constraint = constraint;
        return;
    }
    let original = helix.clone();
    let top = top_radius(helix);
    if field == "twist" {
        if value == t!("CCW").as_ref() {
            helix.handedness = true;
        } else if value == t!("CW").as_ref() {
            helix.handedness = false;
        } else {
            return;
        }
        if !rebuild(helix, top) {
            *helix = original;
        }
        return;
    }
    let Some(number) = parse_f64(value).filter(|number| number.is_finite()) else {
        return;
    };
    let old_base = point(helix.axis_base_point);
    let old_height = helix.turns * helix.turn_height;
    match field {
        "position_x" | "position_y" | "position_z" => {
            let mut base = old_base;
            match field {
                "position_x" => base.x = number,
                "position_y" => base.y = number,
                _ => base.z = number,
            }
            let delta = base - old_base;
            helix.axis_base_point = vector(base);
            helix.start_point = vector(point(helix.start_point) + delta);
        }
        "height" if number.abs() > EPSILON => {
            if helix.constraint == HelixConstraint::TurnHeight && helix.turn_height.abs() > EPSILON {
                let turns = number / helix.turn_height;
                if turns <= EPSILON {
                    return;
                }
                helix.turns = turns;
            } else {
                helix.turn_height = number / helix.turns.max(EPSILON);
            }
        }
        "turns" if number > EPSILON => {
            if helix.constraint == HelixConstraint::TurnHeight {
                helix.turns = number;
            } else {
                helix.turns = number;
                helix.turn_height = old_height / number;
            }
        }
        "turn_height" if number.abs() > EPSILON => {
            if helix.constraint == HelixConstraint::Turns {
                helix.turn_height = number;
            } else {
                let turns = old_height / number;
                if turns <= EPSILON {
                    return;
                }
                helix.turn_height = number;
                helix.turns = turns;
            }
        }
        "base_radius" if number > EPSILON => {
            let Some((base, _, start_direction)) = axis_frame(helix) else {
                return;
            };
            helix.radius = number;
            helix.start_point = vector(base + start_direction * number);
        }
        "top_radius" if number >= 0.0 => {
            if !rebuild(helix, number) {
                *helix = original;
            }
            return;
        }
        _ => return,
    }
    if !rebuild(helix, top) {
        *helix = original;
    }
}

fn apply_grip(helix: &mut Helix, grip_id: usize, apply: GripApply) {
    let original = helix.clone();
    let top = top_radius(helix);
    let Some((base, axis, start_direction)) = axis_frame(helix) else {
        return;
    };
    match (grip_id, apply) {
        (0, GripApply::Translate(delta)) => {
            let delta = kernel(delta);
            if !finite(delta) {
                return;
            }
            helix.axis_base_point = vector(base + delta);
            helix.start_point = vector(point(helix.start_point) + delta);
        }
        (0, GripApply::Absolute(position)) => {
            let position = kernel(position);
            if !finite(position) {
                return;
            }
            let delta = position - base;
            helix.axis_base_point = vector(position);
            helix.start_point = vector(point(helix.start_point) + delta);
        }
        (1, GripApply::Absolute(position)) => {
            let position = kernel(position);
            if !finite(position) {
                return;
            }
            let radial = position - base - axis * (position - base).dot(axis);
            let radius = radial.length();
            if radius <= EPSILON {
                return;
            }
            helix.radius = radius;
            helix.start_point = vector(base + radial);
        }
        (2, GripApply::Absolute(position)) => {
            let position = kernel(position);
            if !finite(position) {
                return;
            }
            let axis_vector = position - base;
            let height = axis_vector.length();
            if height <= EPSILON {
                return;
            }
            let target_axis = axis_vector / height;
            if helix.constraint == HelixConstraint::TurnHeight && helix.turn_height.abs() > EPSILON {
                helix.axis_vector = vector(target_axis * helix.turn_height.signum());
                helix.turns = height / helix.turn_height.abs();
            } else {
                helix.axis_vector = vector(target_axis);
                helix.turn_height = height / helix.turns.max(EPSILON);
            }
            let new_axis = point(helix.axis_vector);
            let start_direction = projected_direction(start_direction, new_axis)
                .or_else(|| perpendicular(new_axis));
            let Some(start_direction) = start_direction else {
                *helix = original;
                return;
            };
            helix.start_point = vector(base + start_direction * helix.radius);
        }
        (3, GripApply::Absolute(position)) => {
            let position = kernel(position);
            if !finite(position) {
                return;
            }
            let delta = position - base;
            let height = delta.dot(axis);
            let radius = (delta - axis * height).length();
            if height.abs() <= EPSILON {
                return;
            }
            if helix.constraint == HelixConstraint::TurnHeight && helix.turn_height.abs() > EPSILON {
                let turns = height / helix.turn_height;
                if turns <= EPSILON {
                    return;
                }
                helix.turns = turns;
            } else {
                helix.turn_height = height / helix.turns.max(EPSILON);
            }
            if !rebuild(helix, radius) {
                *helix = original;
            }
            return;
        }
        _ => return,
    }
    if !rebuild(helix, top) {
        *helix = original;
    }
}

fn apply_transform(helix: &mut Helix, transform: &EntityTransform) {
    match transform {
        EntityTransform::Translate(delta) => {
            acadrust::Entity::translate(helix, Vector3::new(delta.x, delta.y, delta.z));
        }
        EntityTransform::Rotate { center, axis, angle_rad } => {
            crate::scene::view::transform::apply_standard_transform(
                helix,
                *center,
                *axis,
                *angle_rad,
            );
        }
        EntityTransform::Scale { center, factor } => {
            crate::scene::view::transform::apply_standard_scale(helix, *center, *factor);
        }
        EntityTransform::Mirror { p1, p2, working_normal } => {
            let reflection = crate::scene::view::transform::reflection_about_working_line(
                *p1,
                *p2,
                *working_normal,
            );
            acadrust::Entity::apply_transform(helix, &reflection);
        }
        EntityTransform::Affine(value) => acadrust::Entity::apply_transform(helix, value),
    }
}

impl RenderConvertible for Helix {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        self.spline.to_render(document)
    }
}

impl Grippable for Helix {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl crate::entities::traits::PropertyEditable for Helix {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Helix {
    fn apply_transform(&mut self, transform: &EntityTransform) {
        apply_transform(self, transform);
    }
}
