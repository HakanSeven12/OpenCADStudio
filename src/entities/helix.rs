use acadrust::entities::{Helix, HelixConstraint, Spline};
use acadrust::types::Vector3;
use cadkernel::space::{HelixCurve, HelixDirection, NurbsCurve3};
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

fn point(vector: Vector3) -> DVec3 {
    DVec3::new(vector.x, vector.y, vector.z)
}

fn vector(value: DVec3) -> Vector3 {
    Vector3::new(value.x, value.y, value.z)
}

fn spline_curve(spline: &Spline) -> Option<NurbsCurve3> {
    let controls = spline
        .control_points
        .iter()
        .map(|value| [value.x, value.y, value.z])
        .collect();
    let weights = (spline.weights.len() == spline.control_points.len())
        .then(|| spline.weights.clone());
    NurbsCurve3::new(spline.degree.max(1) as usize, controls, spline.knots.clone(), weights)
}

fn axis_frame(helix: &Helix) -> Option<(DVec3, DVec3, DVec3)> {
    let base = point(helix.axis_base_point);
    let axis = point(helix.axis_vector).normalize_or_zero();
    if axis.length_squared() <= EPSILON * EPSILON {
        return None;
    }
    let radial = point(helix.start_point) - base;
    let start_direction = (radial - axis * radial.dot(axis)).normalize_or_zero();
    if start_direction.length_squared() <= EPSILON * EPSILON {
        return None;
    }
    Some((base, axis, start_direction))
}

fn radius_from_axis(helix: &Helix, value: DVec3) -> Option<f64> {
    let (base, axis, _) = axis_frame(helix)?;
    let delta = value - base;
    Some((delta - axis * delta.dot(axis)).length())
}

fn top_radius(helix: &Helix) -> f64 {
    spline_curve(&helix.spline)
        .map(|curve| DVec3::from_array(curve.point_at(1.0)))
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
        .map(|curve| DVec3::from_array(curve.point_at(1.0)))
        .unwrap_or(top_center);
    vec![
        center_grip(0, base),
        square_grip(1, point(helix.start_point)),
        square_grip(2, top_center),
        square_grip(3, top_endpoint),
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
        helix.constraint = if value == t!("Height").as_ref() {
            HelixConstraint::Height
        } else if value == t!("Turns").as_ref() {
            HelixConstraint::Turns
        } else {
            HelixConstraint::TurnHeight
        };
        return;
    }
    let top = top_radius(helix);
    if field == "twist" {
        helix.handedness = value == t!("CCW").as_ref();
        let _ = rebuild(helix, top);
        return;
    }
    let Some(number) = parse_f64(value) else {
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
                helix.turns = (number / helix.turn_height).abs().max(EPSILON);
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
                helix.turn_height = number;
                helix.turns = (old_height / number).abs().max(EPSILON);
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
            let _ = rebuild(helix, number);
            return;
        }
        _ => return,
    }
    let _ = rebuild(helix, top);
}

fn apply_grip(helix: &mut Helix, grip_id: usize, apply: GripApply) {
    let top = top_radius(helix);
    let Some((base, axis, _)) = axis_frame(helix) else {
        return;
    };
    match (grip_id, apply) {
        (0, GripApply::Translate(delta)) => {
            helix.axis_base_point = vector(base + delta);
            helix.start_point = vector(point(helix.start_point) + delta);
        }
        (0, GripApply::Absolute(position)) => {
            let delta = position - base;
            helix.axis_base_point = vector(position);
            helix.start_point = vector(point(helix.start_point) + delta);
        }
        (1, GripApply::Absolute(position)) => {
            let radial = position - base - axis * (position - base).dot(axis);
            let radius = radial.length();
            if radius <= EPSILON {
                return;
            }
            helix.radius = radius;
            helix.start_point = vector(base + radial);
        }
        (2, GripApply::Absolute(position)) => {
            let axis_vector = position - base;
            let height = axis_vector.length();
            if height <= EPSILON {
                return;
            }
            helix.axis_vector = vector(axis_vector / height);
            if helix.constraint == HelixConstraint::TurnHeight && helix.turn_height.abs() > EPSILON {
                helix.turns = (height / helix.turn_height).abs().max(EPSILON);
            } else {
                helix.turn_height = height / helix.turns.max(EPSILON);
            }
        }
        (3, GripApply::Absolute(position)) => {
            let delta = position - base;
            let height = delta.dot(axis);
            let radius = (delta - axis * height).length();
            if height.abs() <= EPSILON {
                return;
            }
            if helix.constraint == HelixConstraint::TurnHeight && helix.turn_height.abs() > EPSILON {
                helix.turns = (height / helix.turn_height).abs().max(EPSILON);
            } else {
                helix.turn_height = height / helix.turns.max(EPSILON);
            }
            let _ = rebuild(helix, radius);
            return;
        }
        _ => return,
    }
    let _ = rebuild(helix, top);
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
