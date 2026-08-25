use acadrust::entities::{
    Dimension, DimensionAligned, DimensionAngular2Ln, DimensionAngular3Pt, DimensionArc,
    DimensionBase, DimensionDiameter, DimensionLargeRadial, DimensionLinear, DimensionOrdinate,
    DimensionRadius,
};
use acadrust::Entity;
use glam::{DVec3, Vec3};

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_angle_prop as edit_angle, edit_prop as edit, lineweight_label,
    lineweight_options, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::t;

pub(crate) fn dimension_text_override(base: &DimensionBase) -> Option<&str> {
    base.user_text
        .as_deref()
        .or_else(|| (!base.text.is_empty()).then_some(base.text.as_str()))
}

pub(crate) fn set_dimension_text_override(base: &mut DimensionBase, text: Option<String>) {
    base.text = text.clone().unwrap_or_default();
    base.user_text = text;
}

fn dimension_definition_point(dim: &Dimension) -> acadrust::types::Vector3 {
    match dim {
        Dimension::Aligned(d) => d.definition_point,
        Dimension::Linear(d) => d.definition_point,
        Dimension::Radius(d) => d.definition_point,
        Dimension::Diameter(d) => d.definition_point,
        Dimension::Angular2Ln(d) => d.definition_point,
        Dimension::Angular3Pt(d) => d.definition_point,
        Dimension::Ordinate(d) => d.definition_point,
        Dimension::Arc(d) => d.definition_point,
        Dimension::LargeRadial(d) => d.definition_point,
    }
}

fn base_props(base: &DimensionBase) -> Vec<crate::scene::model::object::Property> {
    vec![
        crate::scene::model::object::Property {
            label: t!("Text").into_owned(),
            field: "text",
            value: crate::scene::model::object::PropValue::PlainText(base.text.clone()),
        },
        crate::scene::model::object::Property {
            label: t!("User Text").into_owned(),
            field: "user_text",
            value: crate::scene::model::object::PropValue::PlainText(
                base.user_text.clone().unwrap_or_default(),
            ),
        },
        crate::scene::model::object::Property {
            label: t!("Style").into_owned(),
            field: "style_name",
            value: crate::scene::model::object::PropValue::PlainText(base.style_name.clone()),
        },
        edit(t!("Text X").as_ref(), "text_x", base.text_middle_point.x),
        edit(t!("Text Y").as_ref(), "text_y", base.text_middle_point.y),
        edit(t!("Text Z").as_ref(), "text_z", base.text_middle_point.z),
        edit(
            t!("Text Rotation (deg)").as_ref(),
            "text_rotation",
            base.text_rotation.to_degrees(),
        ),
        edit(
            t!("Horizontal Dir (deg)").as_ref(),
            "horizontal_direction",
            base.horizontal_direction.to_degrees(),
        ),
        edit(
            t!("Line Spacing").as_ref(),
            "line_spacing_factor",
            base.line_spacing_factor,
        ),
        ro(
            t!("Measurement").as_ref(),
            "measurement",
            format!("{:.4}", base.actual_measurement),
        ),
    ]
}

fn properties(dim: &Dimension) -> Vec<PropSection> {
    if let Dimension::Radius(radius) = dim {
        return vec![PropSection {
            title: t!("Misc").into_owned(),
            props: vec![
                Property {
                    label: t!("Dimension style").into_owned(),
                    field: "style_name",
                    value: PropValue::PlainText(radius.base.style_name.clone()),
                },
                edit(
                    t!("Leader Length").as_ref(),
                    "leader_length",
                    radius.leader_length,
                ),
            ],
        }];
    }
    let compact_linear = match dim {
        Dimension::Linear(d) => Some((
            &d.base,
            d.rotation,
            d.ext_line_rotation,
        )),
        Dimension::Aligned(d) => Some((
            &d.base,
            (d.second_point.y - d.first_point.y)
                .atan2(d.second_point.x - d.first_point.x),
            d.ext_line_rotation,
        )),
        _ => None,
    };
    if let Some((base, rotation, ext_line_rotation)) = compact_linear {
        return vec![PropSection {
            title: t!("Misc").into_owned(),
            props: vec![
                crate::scene::model::object::Property {
                    label: t!("Dimension style").into_owned(),
                    field: "style_name",
                    value: crate::scene::model::object::PropValue::PlainText(
                        base.style_name.clone(),
                    ),
                },
                edit_angle(
                    t!("Dim line angle").as_ref(),
                    "rotation",
                    rotation.to_degrees(),
                ),
                edit_angle(
                    t!("Extension line angle").as_ref(),
                    "ext_line_rotation",
                    ext_line_rotation.to_degrees(),
                ),
            ],
        }];
    }
    if matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)) {
        return vec![PropSection {
            title: t!("Misc").into_owned(),
            props: vec![crate::scene::model::object::Property {
                label: t!("Dimension style").into_owned(),
                field: "style_name",
                value: crate::scene::model::object::PropValue::PlainText(
                    dim.base().style_name.clone(),
                ),
            }],
        }];
    }
    let mut props = base_props(dim.base());
    match dim {
        Dimension::Aligned(d) => {
            props.extend(linear_like_props(
                d.first_point,
                d.second_point,
                d.definition_point,
            ));
            props.push(edit(
                t!("Ext Rotation (deg)").as_ref(),
                "ext_line_rotation",
                d.ext_line_rotation.to_degrees(),
            ));
        }
        Dimension::Linear(d) => {
            props.extend(linear_like_props(
                d.first_point,
                d.second_point,
                d.definition_point,
            ));
            props.push(edit_angle(t!("Rotation").as_ref(), "rotation", d.rotation.to_degrees()));
            props.push(edit(
                t!("Ext Rotation (deg)").as_ref(),
                "ext_line_rotation",
                d.ext_line_rotation.to_degrees(),
            ));
        }
        Dimension::Radius(d) => {
            props.extend(radius_like_props(d.angle_vertex, d.definition_point));
            props.push(edit(t!("Leader Length").as_ref(), "leader_length", d.leader_length));
        }
        Dimension::Diameter(d) => {
            props.extend(radius_like_props(d.angle_vertex, d.definition_point));
            props.push(edit(t!("Leader Length").as_ref(), "leader_length", d.leader_length));
        }
        Dimension::Angular2Ln(d) => {
            props.extend(angular_props(
                d.angle_vertex,
                d.first_point,
                d.second_point,
                d.definition_point,
            ));
            props.push(edit(t!("Arc X").as_ref(), "dimension_arc_x", d.dimension_arc.x));
            props.push(edit(t!("Arc Y").as_ref(), "dimension_arc_y", d.dimension_arc.y));
            props.push(edit(t!("Arc Z").as_ref(), "dimension_arc_z", d.dimension_arc.z));
        }
        Dimension::Angular3Pt(d) => {
            props.extend(angular_props(
                d.angle_vertex,
                d.first_point,
                d.second_point,
                d.definition_point,
            ));
        }
        Dimension::Ordinate(d) => {
            props.push(edit(t!("Origin X").as_ref(), "definition_x", d.definition_point.x));
            props.push(edit(t!("Origin Y").as_ref(), "definition_y", d.definition_point.y));
            props.push(edit(t!("Origin Z").as_ref(), "definition_z", d.definition_point.z));
            props.push(edit(t!("Feature X").as_ref(), "feature_x", d.feature_location.x));
            props.push(edit(t!("Feature Y").as_ref(), "feature_y", d.feature_location.y));
            props.push(edit(t!("Feature Z").as_ref(), "feature_z", d.feature_location.z));
            props.push(edit(t!("Leader X").as_ref(), "leader_x", d.leader_endpoint.x));
            props.push(edit(t!("Leader Y").as_ref(), "leader_y", d.leader_endpoint.y));
            props.push(edit(t!("Leader Z").as_ref(), "leader_z", d.leader_endpoint.z));
            props.push(ro(
                t!("Ordinate Type").as_ref(),
                "ordinate_type",
                if d.is_ordinate_type_x { "X" } else { "Y" },
            ));
        }
        Dimension::Arc(d) => {
            props.extend(angular_props(
                d.center_point,
                d.first_extension_point,
                d.second_extension_point,
                d.definition_point,
            ));
            props.push(edit(
                t!("Arc Start (deg)").as_ref(),
                "arc_start_parameter",
                d.arc_start_parameter.to_degrees(),
            ));
            props.push(edit(
                t!("Arc End (deg)").as_ref(),
                "arc_end_parameter",
                d.arc_end_parameter.to_degrees(),
            ));
            props.push(ro(t!("Partial").as_ref(), "is_partial", d.is_partial.to_string()));
            props.push(ro(t!("Has Leader").as_ref(), "has_leader", d.has_leader.to_string()));
            props.push(edit(t!("Leader 1 X").as_ref(), "leader1_x", d.first_leader_point.x));
            props.push(edit(t!("Leader 1 Y").as_ref(), "leader1_y", d.first_leader_point.y));
            props.push(edit(t!("Leader 1 Z").as_ref(), "leader1_z", d.first_leader_point.z));
            props.push(edit(t!("Leader 2 X").as_ref(), "leader2_x", d.second_leader_point.x));
            props.push(edit(t!("Leader 2 Y").as_ref(), "leader2_y", d.second_leader_point.y));
            props.push(edit(t!("Leader 2 Z").as_ref(), "leader2_z", d.second_leader_point.z));
        }
        Dimension::LargeRadial(d) => {
            props.push(edit(t!("Center X").as_ref(), "definition_x", d.definition_point.x));
            props.push(edit(t!("Center Y").as_ref(), "definition_y", d.definition_point.y));
            props.push(edit(t!("Center Z").as_ref(), "definition_z", d.definition_point.z));
            props.push(edit(t!("Chord X").as_ref(), "chord_x", d.chord_point.x));
            props.push(edit(t!("Chord Y").as_ref(), "chord_y", d.chord_point.y));
            props.push(edit(t!("Chord Z").as_ref(), "chord_z", d.chord_point.z));
            props.push(edit(t!("Override Center X").as_ref(), "override_x", d.override_center.x));
            props.push(edit(t!("Override Center Y").as_ref(), "override_y", d.override_center.y));
            props.push(edit(t!("Override Center Z").as_ref(), "override_z", d.override_center.z));
            props.push(edit(t!("Jog X").as_ref(), "jog_x", d.jog_point.x));
            props.push(edit(t!("Jog Y").as_ref(), "jog_y", d.jog_point.y));
            props.push(edit(t!("Jog Z").as_ref(), "jog_z", d.jog_point.z));
            props.push(edit_angle(t!("Jog Angle").as_ref(), "jog_angle", d.jog_angle.to_degrees()));
        }
    }
    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props,
    }]
}

fn linear_like_props(
    first: acadrust::types::Vector3,
    second: acadrust::types::Vector3,
    definition: acadrust::types::Vector3,
) -> Vec<crate::scene::model::object::Property> {
    vec![
        edit(t!("First X").as_ref(), "first_x", first.x),
        edit(t!("First Y").as_ref(), "first_y", first.y),
        edit(t!("First Z").as_ref(), "first_z", first.z),
        edit(t!("Second X").as_ref(), "second_x", second.x),
        edit(t!("Second Y").as_ref(), "second_y", second.y),
        edit(t!("Second Z").as_ref(), "second_z", second.z),
        edit(t!("Definition X").as_ref(), "definition_x", definition.x),
        edit(t!("Definition Y").as_ref(), "definition_y", definition.y),
        edit(t!("Definition Z").as_ref(), "definition_z", definition.z),
    ]
}

fn radius_like_props(
    center: acadrust::types::Vector3,
    point: acadrust::types::Vector3,
) -> Vec<crate::scene::model::object::Property> {
    vec![
        edit(t!("Center X").as_ref(), "center_x", center.x),
        edit(t!("Center Y").as_ref(), "center_y", center.y),
        edit(t!("Center Z").as_ref(), "center_z", center.z),
        edit(t!("Point X").as_ref(), "point_x", point.x),
        edit(t!("Point Y").as_ref(), "point_y", point.y),
        edit(t!("Point Z").as_ref(), "point_z", point.z),
    ]
}

fn angular_props(
    vertex: acadrust::types::Vector3,
    first: acadrust::types::Vector3,
    second: acadrust::types::Vector3,
    definition: acadrust::types::Vector3,
) -> Vec<crate::scene::model::object::Property> {
    vec![
        edit(t!("Vertex X").as_ref(), "vertex_x", vertex.x),
        edit(t!("Vertex Y").as_ref(), "vertex_y", vertex.y),
        edit(t!("Vertex Z").as_ref(), "vertex_z", vertex.z),
        edit(t!("First X").as_ref(), "first_x", first.x),
        edit(t!("First Y").as_ref(), "first_y", first.y),
        edit(t!("First Z").as_ref(), "first_z", first.z),
        edit(t!("Second X").as_ref(), "second_x", second.x),
        edit(t!("Second Y").as_ref(), "second_y", second.y),
        edit(t!("Second Z").as_ref(), "second_z", second.z),
        edit(t!("Definition X").as_ref(), "definition_x", definition.x),
        edit(t!("Definition Y").as_ref(), "definition_y", definition.y),
        edit(t!("Definition Z").as_ref(), "definition_z", definition.z),
    ]
}

fn apply_base_prop(base: &mut DimensionBase, field: &str, value: &str) -> bool {
    match field {
        "text" | "user_text" | "text_override" => {
            set_dimension_text_override(base, if value.trim().is_empty() {
                None
            } else {
                Some(value.to_string())
            });
            true
        }
        "style_name" => {
            base.style_name = value.to_string();
            true
        }
        // Editing the text position in the properties panel pins it to a
        // user-defined location (stops following DIMTAD). See #94.
        "text_x" => {
            base.text_user_positioned = true;
            assign_f64(value, &mut base.text_middle_point.x)
        }
        "text_y" => {
            base.text_user_positioned = true;
            assign_f64(value, &mut base.text_middle_point.y)
        }
        "text_z" => {
            base.text_user_positioned = true;
            assign_f64(value, &mut base.text_middle_point.z)
        }
        "text_rotation" => assign_deg(value, &mut base.text_rotation),
        "horizontal_direction" => assign_deg(value, &mut base.horizontal_direction),
        "line_spacing_factor" => assign_f64(value, &mut base.line_spacing_factor),
        _ => false,
    }
}

fn assign_f64(value: &str, target: &mut f64) -> bool {
    let Some(v) = parse_f64(value) else {
        return false;
    };
    *target = v;
    true
}

/// Parse a value entered in DEGREES and store it as radians. Dimension angle
/// fields are kept in radians internally but shown/edited in degrees, matching
/// arc.rs / text.rs so the properties panel reads consistently.
fn assign_deg(value: &str, target: &mut f64) -> bool {
    let Some(v) = parse_f64(value) else {
        return false;
    };
    *target = v.to_radians();
    true
}

fn apply_geom_prop(dim: &mut Dimension, field: &str, value: &str) {
    if apply_base_prop(dim.base_mut(), field, value) {
        return;
    }
    match dim {
        Dimension::Aligned(d) => apply_linear_fields_aligned(d, field, value),
        Dimension::Linear(d) => apply_linear_fields_linear(d, field, value),
        Dimension::Radius(d) => apply_radius_fields(d, field, value),
        Dimension::Diameter(d) => apply_diameter_fields(d, field, value),
        Dimension::Angular2Ln(d) => apply_angular2_fields(d, field, value),
        Dimension::Angular3Pt(d) => apply_angular3_fields(d, field, value),
        Dimension::Ordinate(d) => apply_ordinate_fields(d, field, value),
        Dimension::Arc(d) => apply_arc_fields(d, field, value),
        Dimension::LargeRadial(d) => apply_large_radial_fields(d, field, value),
    }
    let definition_point = dimension_definition_point(dim);
    dim.base_mut().definition_point = definition_point;
    dim.base_mut().actual_measurement = dim.measurement();
}

fn apply_linear_fields_aligned(d: &mut DimensionAligned, field: &str, value: &str) {
    if field == "rotation" {
        let Some(angle) = parse_f64(value).map(f64::to_radians) else {
            return;
        };
        let old_angle = (d.second_point.y - d.first_point.y)
            .atan2(d.second_point.x - d.first_point.x);
        let delta = angle - old_angle;
        let origin_x = d.first_point.x;
        let origin_y = d.first_point.y;
        let rotate = |point: &mut acadrust::types::Vector3| {
            let x = point.x - origin_x;
            let y = point.y - origin_y;
            let (sin, cos) = delta.sin_cos();
            point.x = origin_x + x * cos - y * sin;
            point.y = origin_y + x * sin + y * cos;
        };
        rotate(&mut d.second_point);
        rotate(&mut d.definition_point);
        rotate(&mut d.base.definition_point);
        rotate(&mut d.base.text_middle_point);
        rotate(&mut d.base.insertion_point);
        return;
    }
    apply_linear_common(
        &mut d.first_point,
        &mut d.second_point,
        &mut d.definition_point,
        field,
        value,
    );
    // Only the oblique field touches ext_line_rotation — otherwise editing a
    // coordinate (e.g. First X) would corrupt the dimension's obliquing. #181.
    if field == "ext_line_rotation" {
        let _ = assign_deg(value, &mut d.ext_line_rotation);
    }
}

fn apply_linear_fields_linear(d: &mut DimensionLinear, field: &str, value: &str) {
    apply_linear_common(
        &mut d.first_point,
        &mut d.second_point,
        &mut d.definition_point,
        field,
        value,
    );
    match field {
        "rotation" => {
            let _ = assign_deg(value, &mut d.rotation);
        }
        "ext_line_rotation" => {
            let _ = assign_deg(value, &mut d.ext_line_rotation);
        }
        _ => {}
    }
}

fn apply_linear_common(
    first: &mut acadrust::types::Vector3,
    second: &mut acadrust::types::Vector3,
    definition: &mut acadrust::types::Vector3,
    field: &str,
    value: &str,
) {
    match field {
        "first_x" => {
            let _ = assign_f64(value, &mut first.x);
        }
        "first_y" => {
            let _ = assign_f64(value, &mut first.y);
        }
        "first_z" => {
            let _ = assign_f64(value, &mut first.z);
        }
        "second_x" => {
            let _ = assign_f64(value, &mut second.x);
        }
        "second_y" => {
            let _ = assign_f64(value, &mut second.y);
        }
        "second_z" => {
            let _ = assign_f64(value, &mut second.z);
        }
        "definition_x" => {
            let _ = assign_f64(value, &mut definition.x);
        }
        "definition_y" => {
            let _ = assign_f64(value, &mut definition.y);
        }
        "definition_z" => {
            let _ = assign_f64(value, &mut definition.z);
        }
        _ => {}
    }
}

fn apply_radius_fields(d: &mut DimensionRadius, field: &str, value: &str) {
    apply_radius_common(&mut d.angle_vertex, &mut d.definition_point, field, value);
    if field == "leader_length" {
        let _ = assign_f64(value, &mut d.leader_length);
    }
}

fn apply_diameter_fields(d: &mut DimensionDiameter, field: &str, value: &str) {
    apply_radius_common(&mut d.angle_vertex, &mut d.definition_point, field, value);
    if field == "leader_length" {
        let _ = assign_f64(value, &mut d.leader_length);
    }
}

fn apply_radius_common(
    center: &mut acadrust::types::Vector3,
    point: &mut acadrust::types::Vector3,
    field: &str,
    value: &str,
) {
    match field {
        "center_x" => {
            let _ = assign_f64(value, &mut center.x);
        }
        "center_y" => {
            let _ = assign_f64(value, &mut center.y);
        }
        "center_z" => {
            let _ = assign_f64(value, &mut center.z);
        }
        "point_x" => {
            let _ = assign_f64(value, &mut point.x);
        }
        "point_y" => {
            let _ = assign_f64(value, &mut point.y);
        }
        "point_z" => {
            let _ = assign_f64(value, &mut point.z);
        }
        _ => {}
    }
}

fn apply_angular2_fields(d: &mut DimensionAngular2Ln, field: &str, value: &str) {
    apply_angular_common(
        &mut d.angle_vertex,
        &mut d.first_point,
        &mut d.second_point,
        &mut d.definition_point,
        field,
        value,
    );
    match field {
        "dimension_arc_x" => {
            let _ = assign_f64(value, &mut d.dimension_arc.x);
        }
        "dimension_arc_y" => {
            let _ = assign_f64(value, &mut d.dimension_arc.y);
        }
        "dimension_arc_z" => {
            let _ = assign_f64(value, &mut d.dimension_arc.z);
        }
        _ => {}
    }
}

fn apply_angular3_fields(d: &mut DimensionAngular3Pt, field: &str, value: &str) {
    apply_angular_common(
        &mut d.angle_vertex,
        &mut d.first_point,
        &mut d.second_point,
        &mut d.definition_point,
        field,
        value,
    );
}

fn apply_angular_common(
    vertex: &mut acadrust::types::Vector3,
    first: &mut acadrust::types::Vector3,
    second: &mut acadrust::types::Vector3,
    definition: &mut acadrust::types::Vector3,
    field: &str,
    value: &str,
) {
    match field {
        "vertex_x" => {
            let _ = assign_f64(value, &mut vertex.x);
        }
        "vertex_y" => {
            let _ = assign_f64(value, &mut vertex.y);
        }
        "vertex_z" => {
            let _ = assign_f64(value, &mut vertex.z);
        }
        "first_x" => {
            let _ = assign_f64(value, &mut first.x);
        }
        "first_y" => {
            let _ = assign_f64(value, &mut first.y);
        }
        "first_z" => {
            let _ = assign_f64(value, &mut first.z);
        }
        "second_x" => {
            let _ = assign_f64(value, &mut second.x);
        }
        "second_y" => {
            let _ = assign_f64(value, &mut second.y);
        }
        "second_z" => {
            let _ = assign_f64(value, &mut second.z);
        }
        "definition_x" => {
            let _ = assign_f64(value, &mut definition.x);
        }
        "definition_y" => {
            let _ = assign_f64(value, &mut definition.y);
        }
        "definition_z" => {
            let _ = assign_f64(value, &mut definition.z);
        }
        _ => {}
    }
}

fn apply_ordinate_fields(d: &mut DimensionOrdinate, field: &str, value: &str) {
    match field {
        "definition_x" => {
            let _ = assign_f64(value, &mut d.definition_point.x);
        }
        "definition_y" => {
            let _ = assign_f64(value, &mut d.definition_point.y);
        }
        "definition_z" => {
            let _ = assign_f64(value, &mut d.definition_point.z);
        }
        "feature_x" => {
            let _ = assign_f64(value, &mut d.feature_location.x);
        }
        "feature_y" => {
            let _ = assign_f64(value, &mut d.feature_location.y);
        }
        "feature_z" => {
            let _ = assign_f64(value, &mut d.feature_location.z);
        }
        "leader_x" => {
            let _ = assign_f64(value, &mut d.leader_endpoint.x);
        }
        "leader_y" => {
            let _ = assign_f64(value, &mut d.leader_endpoint.y);
        }
        "leader_z" => {
            let _ = assign_f64(value, &mut d.leader_endpoint.z);
        }
        _ => {}
    }
}

fn apply_arc_fields(d: &mut DimensionArc, field: &str, value: &str) {
    apply_angular_common(
        &mut d.center_point,
        &mut d.first_extension_point,
        &mut d.second_extension_point,
        &mut d.definition_point,
        field,
        value,
    );
    match field {
        "arc_start_parameter" => {
            let _ = assign_deg(value, &mut d.arc_start_parameter);
        }
        "arc_end_parameter" => {
            let _ = assign_deg(value, &mut d.arc_end_parameter);
        }
        "leader1_x" => {
            let _ = assign_f64(value, &mut d.first_leader_point.x);
        }
        "leader1_y" => {
            let _ = assign_f64(value, &mut d.first_leader_point.y);
        }
        "leader1_z" => {
            let _ = assign_f64(value, &mut d.first_leader_point.z);
        }
        "leader2_x" => {
            let _ = assign_f64(value, &mut d.second_leader_point.x);
        }
        "leader2_y" => {
            let _ = assign_f64(value, &mut d.second_leader_point.y);
        }
        "leader2_z" => {
            let _ = assign_f64(value, &mut d.second_leader_point.z);
        }
        _ => {}
    }
}

fn apply_large_radial_fields(d: &mut DimensionLargeRadial, field: &str, value: &str) {
    match field {
        "definition_x" => {
            let _ = assign_f64(value, &mut d.definition_point.x);
        }
        "definition_y" => {
            let _ = assign_f64(value, &mut d.definition_point.y);
        }
        "definition_z" => {
            let _ = assign_f64(value, &mut d.definition_point.z);
        }
        "chord_x" => {
            let _ = assign_f64(value, &mut d.chord_point.x);
        }
        "chord_y" => {
            let _ = assign_f64(value, &mut d.chord_point.y);
        }
        "chord_z" => {
            let _ = assign_f64(value, &mut d.chord_point.z);
        }
        "override_x" => {
            let _ = assign_f64(value, &mut d.override_center.x);
        }
        "override_y" => {
            let _ = assign_f64(value, &mut d.override_center.y);
        }
        "override_z" => {
            let _ = assign_f64(value, &mut d.override_center.z);
        }
        "jog_x" => {
            let _ = assign_f64(value, &mut d.jog_point.x);
        }
        "jog_y" => {
            let _ = assign_f64(value, &mut d.jog_point.y);
        }
        "jog_z" => {
            let _ = assign_f64(value, &mut d.jog_point.z);
        }
        "jog_angle" => {
            let _ = assign_deg(value, &mut d.jog_angle);
        }
        _ => {}
    }
}

fn apply_transform(dim: &mut Dimension, t: &EntityTransform) {
    match t {
        EntityTransform::Translate(d) => dim.translate(acadrust::types::Vector3::new(
            d.x as f64, d.y as f64, d.z as f64,
        )),
        EntityTransform::Rotate { center, axis, angle_rad } => {
            if axis.normalize_or(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-10) {
                transform_dimension_points(dim, |pt| rotate_point(pt, *center, *angle_rad))
            } else {
                crate::scene::view::transform::apply_standard_transform(
                    dim,
                    *center,
                    *axis,
                    *angle_rad,
                );
            }
        }
        EntityTransform::Scale { center, factor } => {
            transform_dimension_points(dim, |pt| scale_point(pt, *center, *factor))
        }
        EntityTransform::Mirror { p1, p2, working_normal } => {
            if working_normal.normalize_or(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-10) {
                transform_dimension_points(dim, |pt| mirror_point(pt, *p1, *p2))
            } else {
                acadrust::Entity::apply_transform(
                    dim,
                    &crate::scene::view::transform::reflection_about_working_line(
                        *p1,
                        *p2,
                        *working_normal,
                    ),
                );
            }
        }
        EntityTransform::Affine(transform) => {
            let old_normal = dim.base().normal;
            let text_rotation =
                transformed_dimension_angle(old_normal, dim.base().text_rotation, transform);
            let horizontal_direction = transformed_dimension_angle(
                old_normal,
                dim.base().horizontal_direction,
                transform,
            );
            let insertion_rotation = transformed_dimension_angle(
                old_normal,
                dim.base().insertion_rotation,
                transform,
            );
            let linear_angles = match dim {
                Dimension::Linear(value) => Some((
                    transformed_dimension_angle(old_normal, value.rotation, transform),
                    transformed_dimension_angle(old_normal, value.ext_line_rotation, transform),
                )),
                _ => None,
            };
            transform_dimension_points(dim, |point| {
                *point = transform.apply(*point);
            });
            let transformed_normal = transform.apply_rotation(old_normal);
            let base = dim.base_mut();
            base.normal = if transformed_normal.length() > 1e-12 {
                transformed_normal.normalize()
            } else {
                old_normal
            };
            base.text_rotation = text_rotation;
            base.horizontal_direction = horizontal_direction;
            base.insertion_rotation = insertion_rotation;
            if let Some((rotation, ext_line_rotation)) = linear_angles {
                if let Dimension::Linear(value) = dim {
                    value.rotation = rotation;
                    value.ext_line_rotation = ext_line_rotation;
                }
            }
        }
    }
    dim.base_mut().actual_measurement = dim.measurement();
}

fn transform_dimension_points<F>(dim: &mut Dimension, mut f: F)
where
    F: FnMut(&mut acadrust::types::Vector3),
{
    f(&mut dim.base_mut().definition_point);
    f(&mut dim.base_mut().text_middle_point);
    f(&mut dim.base_mut().insertion_point);
    match dim {
        Dimension::Aligned(d) => {
            f(&mut d.first_point);
            f(&mut d.second_point);
            f(&mut d.definition_point);
        }
        Dimension::Linear(d) => {
            f(&mut d.first_point);
            f(&mut d.second_point);
            f(&mut d.definition_point);
        }
        Dimension::Radius(d) => {
            f(&mut d.angle_vertex);
            f(&mut d.definition_point);
        }
        Dimension::Diameter(d) => {
            f(&mut d.angle_vertex);
            f(&mut d.definition_point);
        }
        Dimension::Angular2Ln(d) => {
            f(&mut d.dimension_arc);
            f(&mut d.first_point);
            f(&mut d.second_point);
            f(&mut d.angle_vertex);
            f(&mut d.definition_point);
        }
        Dimension::Angular3Pt(d) => {
            f(&mut d.first_point);
            f(&mut d.second_point);
            f(&mut d.angle_vertex);
            f(&mut d.definition_point);
        }
        Dimension::Ordinate(d) => {
            f(&mut d.definition_point);
            f(&mut d.feature_location);
            f(&mut d.leader_endpoint);
        }
        Dimension::Arc(d) => {
            f(&mut d.definition_point);
            f(&mut d.first_extension_point);
            f(&mut d.second_extension_point);
            f(&mut d.center_point);
            if d.has_leader {
                f(&mut d.first_leader_point);
                f(&mut d.second_leader_point);
            }
        }
        Dimension::LargeRadial(d) => {
            f(&mut d.definition_point);
            f(&mut d.chord_point);
            f(&mut d.override_center);
            f(&mut d.jog_point);
        }
    }
}

fn transformed_dimension_angle(
    normal: acadrust::types::Vector3,
    angle: f64,
    transform: &acadrust::types::Transform,
) -> f64 {
    let ((xx, xy, xz), (yx, yy, yz)) =
        crate::scene::view::transform::ocs_axes((normal.x, normal.y, normal.z));
    let direction = acadrust::types::Vector3::new(
        xx * angle.cos() + yx * angle.sin(),
        xy * angle.cos() + yy * angle.sin(),
        xz * angle.cos() + yz * angle.sin(),
    );
    let transformed_normal = transform.apply_rotation(normal).normalize();
    let transformed_direction = transform.apply_rotation(direction).normalize();
    let ((nxx, nxy, nxz), (nyx, nyy, nyz)) = crate::scene::view::transform::ocs_axes((
        transformed_normal.x,
        transformed_normal.y,
        transformed_normal.z,
    ));
    let new_x = acadrust::types::Vector3::new(nxx, nxy, nxz);
    let new_y = acadrust::types::Vector3::new(nyx, nyy, nyz);
    transformed_direction
        .dot(&new_y)
        .atan2(transformed_direction.dot(&new_x))
}

fn rotate_point(p: &mut acadrust::types::Vector3, center: DVec3, angle_rad: f64) {
    let dx = p.x - center.x;
    let dy = p.y - center.y;
    let (s, c) = angle_rad.sin_cos();
    p.x = center.x + dx * c - dy * s;
    p.y = center.y + dx * s + dy * c;
}

fn scale_point(p: &mut acadrust::types::Vector3, center: DVec3, factor: f64) {
    let f = factor;
    p.x = center.x + (p.x - center.x) * f;
    p.y = center.y + (p.y - center.y) * f;
    p.z = center.z + (p.z - center.z) * f;
}

fn mirror_point(p: &mut acadrust::types::Vector3, p1: DVec3, p2: DVec3) {
    crate::scene::view::transform::reflect_xy_point(&mut p.x, &mut p.y, p1, p2);
}

impl PropertyEditable for Dimension {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Dimension {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

// ── Grippable ─────────────────────────────────────────────────────────────────

/// f64 variant for grip positions — grips must not round UTM-scale
/// coordinates through f32 before the world-offset subtraction.
fn dv3(v: &acadrust::types::Vector3) -> glam::DVec3 {
    glam::DVec3::new(v.x, v.y, v.z)
}

fn set_v3(target: &mut acadrust::types::Vector3, p: DVec3) {
    target.x = p.x;
    target.y = p.y;
    target.z = p.z;
}

fn translate_v3(target: &mut acadrust::types::Vector3, d: DVec3) {
    target.x += d.x;
    target.y += d.y;
    target.z += d.z;
}

fn apply_to_v3(target: &mut acadrust::types::Vector3, apply: &GripApply) {
    match apply {
        GripApply::Absolute(p) => set_v3(target, *p),
        GripApply::Translate(d) => translate_v3(target, *d),
    }
}



#[derive(Clone, Copy)]
struct DimTextRelativePosition {
    along_fraction: f64,
    perpendicular_offset: f64,
    z_offset: f64,
}

fn capture_dim_text_relative_position(dim: &Dimension) -> Option<DimTextRelativePosition> {
    let base = dim.base();

    // Auto-positioned text is recomputed by the renderer from DIMSTYLE.
    // Only an explicitly moved text point needs to follow a grip deformation.
    if !base.text_user_positioned {
        return None;
    }

    let text = base.text_middle_point;
    if text.x * text.x + text.y * text.y + text.z * text.z <= 1e-16 {
        return None;
    }

    let (first, second, defpt, ax, ay) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            d.rotation.cos(),
            d.rotation.sin(),
        ),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len <= 1e-12 {
                return None;
            }

            (
                d.first_point,
                d.second_point,
                d.definition_point,
                dx / len,
                dy / len,
            )
        }
        _ => return None,
    };

    let px = -ay;
    let py = ax;

    let t1 = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
    let t2 = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;
    let text_t = (text.x - defpt.x) * ax + (text.y - defpt.y) * ay;

    let span = t2 - t1;
    let along_fraction = if span.abs() > 1e-12 {
        (text_t - t1) / span
    } else {
        0.5
    };

    let along = t1 + span * along_fraction;
    let line_x = defpt.x + ax * along;
    let line_y = defpt.y + ay * along;

    let perpendicular_offset =
        (text.x - line_x) * px + (text.y - line_y) * py;

    Some(DimTextRelativePosition {
        along_fraction,
        perpendicular_offset,
        z_offset: text.z - defpt.z,
    })
}

fn restore_dim_text_relative_position(
    dim: &mut Dimension,
    saved: DimTextRelativePosition,
) {
    let (first, second, defpt, ax, ay) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            d.rotation.cos(),
            d.rotation.sin(),
        ),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len <= 1e-12 {
                return;
            }

            (
                d.first_point,
                d.second_point,
                d.definition_point,
                dx / len,
                dy / len,
            )
        }
        _ => return,
    };

    let px = -ay;
    let py = ax;

    let t1 = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
    let t2 = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;

    let along = t1 + (t2 - t1) * saved.along_fraction;

    let base = dim.base_mut();
    base.text_middle_point = Vector3::new(
        defpt.x + ax * along + px * saved.perpendicular_offset,
        defpt.y + ay * along + py * saved.perpendicular_offset,
        defpt.z + saved.z_offset,
    );
}

fn dimension_line_grip_position(dim: &Dimension) -> Option<DVec3> {
    match dim {
        Dimension::Angular2Ln(d) => return Some(dv3(&d.dimension_arc)),
        Dimension::Angular3Pt(d) => return Some(dv3(&d.definition_point)),
        Dimension::Arc(d) => return Some(dv3(&d.definition_point)),
        _ => {}
    }
    let (first, second, defpt, ax, ay) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            d.rotation.cos(),
            d.rotation.sin(),
        ),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt();

            if len <= 1e-12 {
                return None;
            }

            (
                d.first_point,
                d.second_point,
                d.definition_point,
                dx / len,
                dy / len,
            )
        }
        _ => return None,
    };

    let px = -ay;
    let py = ax;

    // Project both extension origins onto the current dimension line.
    let off1 =
        (defpt.x - first.x) * px + (defpt.y - first.y) * py;
    let off2 =
        (defpt.x - second.x) * px + (defpt.y - second.y) * py;

    let p1 = DVec3::new(
        first.x + px * off1,
        first.y + py * off1,
        defpt.z,
    );

    let p2 = DVec3::new(
        second.x + px * off2,
        second.y + py * off2,
        defpt.z,
    );

    Some((p1 + p2) * 0.5)
}

fn above_dimension_text_position(dim: &Dimension) -> Option<DVec3> {
    if let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) {
        let angle = (start + end) * 0.5;
        let direction = DVec3::new(angle.cos() as f64, angle.sin() as f64, 0.0);
        return Some(
            DVec3::new(vertex.x as f64, vertex.y as f64, vertex.z as f64)
                + direction * (radius as f64 + 1.0),
        );
    }
    let center = dimension_line_grip_position(dim)?;
    let (ax, ay) = match dim {
        Dimension::Linear(d) => (d.rotation.cos(), d.rotation.sin()),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let length = (dx * dx + dy * dy).sqrt();
            if length <= 1e-12 {
                return None;
            }
            (dx / length, dy / length)
        }
        _ => return Some(center + DVec3::Y),
    };
    Some(center + DVec3::new(-ay, ax, 0.0))
}

impl Grippable for Dimension {
    fn grips(&self) -> Vec<GripDef> {
        // Auto-placed dimensions carry a zero text_middle_point sentinel; put
        // the text grip at the style-default placement (default metrics) instead
        // of the world origin, so it stays on the visible text and grabbable.
        let text = {
            let p = self.base().text_middle_point;
            if p.x * p.x + p.y * p.y + p.z * p.z > 1e-16 {
                dv3(&p)
            } else {
                dv3(&dimension_text_pos_f64(self, None, 2.5, 1.0))
            }
        };
        match self {
            Dimension::Linear(d) => vec![
                square_grip(0, dv3(&d.first_point)),
                center_grip(1, dv3(&d.second_point)),
                center_grip(
                    2,
                    dimension_line_grip_position(self)
                        .unwrap_or_else(|| dv3(&d.definition_point)),
                ),
                center_grip(3, text),
            ],
            Dimension::Aligned(d) => vec![
                square_grip(0, dv3(&d.first_point)),
                center_grip(1, dv3(&d.second_point)),
                center_grip(
                    2,
                    dimension_line_grip_position(self)
                        .unwrap_or_else(|| dv3(&d.definition_point)),
                ),
                center_grip(3, text),
            ],
            Dimension::Radius(d) => vec![
                square_grip(0, dv3(&d.angle_vertex)),
                center_grip(1, dv3(&d.definition_point)),
                center_grip(2, text),
            ],
            Dimension::Diameter(d) => vec![
                square_grip(0, dv3(&d.angle_vertex)),
                center_grip(1, dv3(&d.definition_point)),
                center_grip(2, text),
            ],
            Dimension::Angular2Ln(d) => vec![
                square_grip(0, dv3(&d.first_point)),
                center_grip(1, dv3(&d.second_point)),
                center_grip(2, dv3(&d.angle_vertex)),
                center_grip(3, dv3(&d.definition_point)),
                center_grip(4, dv3(&d.dimension_arc)),
                center_grip(5, text),
            ],
            Dimension::Angular3Pt(d) => vec![
                square_grip(0, dv3(&d.angle_vertex)),
                center_grip(1, dv3(&d.first_point)),
                center_grip(2, dv3(&d.second_point)),
                center_grip(3, dv3(&d.definition_point)),
                center_grip(4, text),
            ],
            Dimension::Ordinate(d) => vec![
                square_grip(0, dv3(&d.definition_point)),
                center_grip(1, dv3(&d.feature_location)),
                center_grip(2, dv3(&d.leader_endpoint)),
                center_grip(3, text),
            ],
            Dimension::Arc(d) => {
                let mut grips = vec![
                    square_grip(0, dv3(&d.center_point)),
                    center_grip(1, dv3(&d.first_extension_point)),
                    center_grip(2, dv3(&d.second_extension_point)),
                    center_grip(3, dv3(&d.definition_point)),
                ];
                if d.has_leader {
                    grips.push(center_grip(4, dv3(&d.first_leader_point)));
                    grips.push(center_grip(5, dv3(&d.second_leader_point)));
                    grips.push(center_grip(6, text));
                } else {
                    grips.push(center_grip(4, text));
                }
                grips
            }
            Dimension::LargeRadial(d) => vec![
                square_grip(0, dv3(&d.definition_point)),
                center_grip(1, dv3(&d.chord_point)),
                center_grip(2, dv3(&d.override_center)),
                center_grip(3, dv3(&d.jog_point)),
                center_grip(4, text),
            ],
        }
    }



    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        // Last grip always moves the text.
        let text_grip = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => 3,
            Dimension::Radius(_) | Dimension::Diameter(_) => 2,
            Dimension::Angular2Ln(_) => 5,
            Dimension::Angular3Pt(_) => 4,
            Dimension::Ordinate(_) => 3,
            Dimension::Arc(d) => if d.has_leader { 6 } else { 4 },
            Dimension::LargeRadial(_) => 4,
        };
        if grip_id == text_grip {
            apply_to_v3(&mut self.base_mut().text_middle_point, &apply);
            // Dragging the text grip pins it to a user-defined location, so it
            // no longer follows the style (DIMTAD). See #94.
            self.base_mut().text_user_positioned = true;
            return;
        }

        // A manually positioned dimension text point is stored in the DWG as an
        // absolute coordinate. Capture its relation to the old dimension geometry
        // before moving a definition grip so that it can be reconstructed against
        // the new geometry afterwards.
        let relative_text = capture_dim_text_relative_position(self);

        match self {
            Dimension::Linear(d) => match grip_id {
                0 => apply_to_v3(&mut d.first_point, &apply),
                1 => apply_to_v3(&mut d.second_point, &apply),
                2 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Aligned(d) => match grip_id {
                0 => apply_to_v3(&mut d.first_point, &apply),
                1 => apply_to_v3(&mut d.second_point, &apply),
                2 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Radius(d) => match grip_id {
                0 => apply_to_v3(&mut d.angle_vertex, &apply),
                1 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Diameter(d) => match grip_id {
                0 => apply_to_v3(&mut d.angle_vertex, &apply),
                1 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Angular2Ln(d) => match grip_id {
                0 => apply_to_v3(&mut d.first_point, &apply),
                1 => apply_to_v3(&mut d.second_point, &apply),
                2 => apply_to_v3(&mut d.angle_vertex, &apply),
                3 => apply_to_v3(&mut d.definition_point, &apply),
                4 => apply_to_v3(&mut d.dimension_arc, &apply),
                _ => {}
            },
            Dimension::Angular3Pt(d) => match grip_id {
                0 => apply_to_v3(&mut d.angle_vertex, &apply),
                1 => apply_to_v3(&mut d.first_point, &apply),
                2 => apply_to_v3(&mut d.second_point, &apply),
                3 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Ordinate(d) => match grip_id {
                0 => apply_to_v3(&mut d.definition_point, &apply),
                1 => apply_to_v3(&mut d.feature_location, &apply),
                2 => apply_to_v3(&mut d.leader_endpoint, &apply),
                _ => {}
            },
            Dimension::Arc(d) => match grip_id {
                0 => apply_to_v3(&mut d.center_point, &apply),
                1 => apply_to_v3(&mut d.first_extension_point, &apply),
                2 => apply_to_v3(&mut d.second_extension_point, &apply),
                3 => apply_to_v3(&mut d.definition_point, &apply),
                4 => apply_to_v3(&mut d.first_leader_point, &apply),
                5 => apply_to_v3(&mut d.second_leader_point, &apply),
                _ => {}
            },
            Dimension::LargeRadial(d) => match grip_id {
                0 => apply_to_v3(&mut d.definition_point, &apply),
                1 => apply_to_v3(&mut d.chord_point, &apply),
                2 => apply_to_v3(&mut d.override_center, &apply),
                3 => apply_to_v3(&mut d.jog_point, &apply),
                _ => {}
            },
        }

        if let Some(saved) = relative_text {
            restore_dim_text_relative_position(self, saved);
        }

        let definition_point = dimension_definition_point(self);
        self.base_mut().definition_point = definition_point;
        self.base_mut().actual_measurement = self.measurement();
    }

    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        let (dim_line_grip, text_grip) = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => (2, 3),
            Dimension::Radius(_) | Dimension::Diameter(_) => (1, 2),
            Dimension::Angular2Ln(_) => (4, 5),
            Dimension::Angular3Pt(_) => (3, 4),
            Dimension::Ordinate(_) => (0, 3),
            Dimension::Arc(d) => (3, if d.has_leader { 6 } else { 4 }),
            Dimension::LargeRadial(_) => (3, 4),
        };
        if grip_id == text_grip {
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Move with Dim Line",
                    action: GripMenuAction::MoveWithDimLine,
                },
                GripMenuItem {
                    label: "Move with Leader",
                    action: GripMenuAction::MoveWithLeader,
                },
                GripMenuItem {
                    label: "Move Independent",
                    action: GripMenuAction::MoveIndependent,
                },
                GripMenuItem {
                    label: "Reset Text",
                    action: GripMenuAction::ResetText,
                },
                GripMenuItem {
                    label: "Rotate Text",
                    action: GripMenuAction::RotateText,
                },
                GripMenuItem {
                    label: "Above Dim Line",
                    action: GripMenuAction::AboveDimLine,
                },
                GripMenuItem {
                    label: "Center",
                    action: GripMenuAction::Center,
                },
            ]
        } else if grip_id == dim_line_grip {
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Reverse Arrows",
                    action: GripMenuAction::ReverseArrows,
                },
            ]
        } else {
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
        }
    }

    fn apply_grip_menu(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        let (_dim_line_grip, text_grip) = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => (2, 3),
            Dimension::Radius(_) | Dimension::Diameter(_) => (1, 2),
            Dimension::Angular2Ln(_) => (4, 5),
            Dimension::Angular3Pt(_) => (3, 4),
            Dimension::Ordinate(_) => (0, 3),
            Dimension::Arc(d) => (3, if d.has_leader { 6 } else { 4 }),
            Dimension::LargeRadial(_) => (3, 4),
        };
        match action {
            A::ResetText if grip_id == text_grip => {
                // Drop any text-position override — leave it to the
                // renderer to recompute from the dim style.
                let b = self.base_mut();
                b.text_middle_point.x = 0.0;
                b.text_middle_point.y = 0.0;
                b.text_middle_point.z = 0.0;
                b.text_user_positioned = false;
            }
            A::Center if grip_id == text_grip => {
                if let Some(point) = dimension_line_grip_position(self) {
                    let base = self.base_mut();
                    base.text_middle_point = Vector3::new(point.x, point.y, point.z);
                    base.text_user_positioned = true;
                }
            }
            A::ReverseArrows => {
                let base = self.base_mut();
                base.flip_arrow1 = !base.flip_arrow1;
                base.flip_arrow2 = !base.flip_arrow2;
            }
            A::AboveDimLine if grip_id == text_grip => {
                if let Some(point) = above_dimension_text_position(self) {
                    let base = self.base_mut();
                    base.text_middle_point = Vector3::new(point.x, point.y + 1.0, point.z);
                    base.text_user_positioned = true;
                }
            }
            _ => {}
        }
    }

    fn grip_menu_value_prompt(
        &self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::model::object::GripMenuAction as A;
        let text_grip = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => 3,
            Dimension::Radius(_) | Dimension::Diameter(_) => 2,
            Dimension::Angular2Ln(_) => 5,
            Dimension::Angular3Pt(_) => 4,
            Dimension::Ordinate(_) => 3,
            Dimension::Arc(d) => if d.has_leader { 6 } else { 4 },
            Dimension::LargeRadial(_) => 4,
        };
        (grip_id == text_grip && matches!(action, A::RotateText))
            .then_some("Specify text rotation")
    }

    fn apply_grip_menu_value(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        if self.grip_menu_value_prompt(grip_id, action).is_some()
            && matches!(action, A::RotateText)
        {
            self.base_mut().text_rotation = value.to_radians();
        }
    }
}

// ── Tessellation ─────────────────────────────────────────────────────────
//
// Per-entity tessellation entry for `Dimension`. The trait + impl live in
// this file so all dimension tess code stays alongside the entity
// definition. Shared dim machinery (`ArrowKind`, `DimGeom`, `append_arrow`,
// arrow blocks, colour resolution, `add_segment` / `add_polyline`,
// `normalized_or`, `entity_z`, `offset_snap_pts`) lives in
// `scene::convert::tessellate` and is reused by Leader / MultiLeader too.

use acadrust::entities::{MText, Text};
use acadrust::tables::DimStyle;
use acadrust::types::{Color as AcadColor, Vector3};

/// Build the linear-dimension property groups from the assigned style plus
/// any entity-level DSTYLE overrides. Conditional rows remain visible but are
/// read-only while their controlling option is disabled.
pub fn style_sections(
    style: &DimStyle,
    dimension: &Dimension,
    document: &CadDocument,
) -> Vec<PropSection> {
    use crate::entities::dim_override as ov;

    let data = &dimension.base().common.extended_data;
    let real = |code, inherited| ov::real(data, code).unwrap_or(inherited);
    let int = |code, inherited| ov::int(data, code).unwrap_or(inherited);
    let string = |code, inherited: &str| {
        ov::string(data, code).unwrap_or_else(|| inherited.to_string())
    };
    let on = |value: bool| if value { "On" } else { "Off" };
    let yes = |value: bool| if value { "Yes" } else { "No" };
    let number = |label: &str, field: &'static str, value: f64, editable: bool| {
        property(
            label,
            field,
            if editable {
                PropValue::EditText(format!("{value:.4}"))
            } else {
                PropValue::ReadOnly(format!("{value:.4}"))
            },
        )
    };
    let text = |label: &str, field: &'static str, value: String, editable: bool| {
        property(
            label,
            field,
            if editable {
                PropValue::PlainText(value)
            } else {
                PropValue::ReadOnly(value)
            },
        )
    };
    let choice = |label: &str,
                  field: &'static str,
                  selected: &str,
                  options: &[&str],
                  editable: bool| {
        property(
            label,
            field,
            if editable {
                choice_value(selected, options)
            } else {
                PropValue::ReadOnly(selected.to_string())
            },
        )
    };

    let s = style;
    let dimfxlon = int(ov::DIMFXLON, s.dimfxlon as i16) != 0;
    let dimtix = int(ov::DIMTIX, s.dimtix as i16) != 0;
    let dimlunit = int(ov::DIMLUNIT, s.dimlunit);
    let dimfrac = int(ov::DIMFRAC, s.dimfrac);
    let dimalt = int(ov::DIMALT, s.dimalt as i16) != 0;
    let dimtol = int(ov::DIMTOL, s.dimtol as i16) != 0;
    let dimlim = int(ov::DIMLIM, s.dimlim as i16) != 0;
    let dimtp = real(ov::DIMTP, s.dimtp);
    let dimtm = real(ov::DIMTM, s.dimtm);
    let dimgap = real(ov::DIMGAP, s.dimgap);
    let tolerance_display = if dimgap < 0.0 {
        "Basic"
    } else if dimlim {
        "Limits"
    } else if dimtol && (dimtp - dimtm).abs() <= 1e-12 {
        "Symmetrical"
    } else if dimtol {
        "Deviation"
    } else {
        "None"
    };
    let tolerance_enabled = matches!(
        tolerance_display,
        "Symmetrical" | "Deviation" | "Limits"
    );
    let alternate_tolerance_enabled = tolerance_enabled && dimalt;
    let dimzin = int(ov::DIMZIN, s.dimzin);
    let dimaltz = int(ov::DIMALTZ, s.dimaltz);
    let dimtzin = int(ov::DIMTZIN, s.dimtzin);
    let dimalttz = int(ov::DIMALTTZ, s.dimalttz);
    let annotative = s.annotative
        || !crate::scene::annotative::object_scale_memberships(
            document,
            dimension.base().common.handle,
        )
        .is_empty();

    let overridden_text_style = ov::handle(data, ov::DIMTXSTY);
    let text_style_name = overridden_text_style
        .and_then(|handle| {
            document
                .text_styles
                .iter()
                .find(|record| record.handle == handle)
                .map(|record| record.name.clone())
        })
        .unwrap_or_else(|| s.dimtxsty.clone());
    let text_height_editable = document
        .text_styles
        .iter()
        .find(|record| {
            overridden_text_style
                .is_some_and(|handle| record.handle == handle)
                || (overridden_text_style.is_none()
                    && record.name.eq_ignore_ascii_case(&text_style_name))
        })
        .is_none_or(|record| !record.has_fixed_height());

    let (dim_prefix, dim_suffix) =
        split_measurement_template(&string(ov::DIMPOST, &s.dimpost));
    let (alt_prefix, alt_suffix) =
        split_measurement_template(&string(ov::DIMAPOST, &s.dimapost));
    let decimal_separator = {
        let value = int(ov::DIMDSEP, s.dimdsep);
        let character = value as u8 as char;
        if value > 0 && !character.is_control() {
            character.to_string()
        } else {
            value.to_string()
        }
    };

    let mut arrow_options: Vec<String> = std::iter::once("Closed filled".to_string())
        .chain(
            document
                .block_records
                .iter()
                .map(|record| record.name.clone())
                .filter(|name| !name.is_empty()),
        )
        .collect();
    let mut linetype_options: Vec<String> = document
        .line_types
        .iter()
        .map(|line_type| line_type.name.clone())
        .filter(|name| !name.is_empty())
        .collect();
    let text_style_options: Vec<String> = document
        .text_styles
        .iter()
        .map(|record| record.name.clone())
        .filter(|name| !name.is_empty())
        .collect();

    let arrow_name = |code, inherited_handle, inherited_name: &str| {
        ov::handle(data, code)
            .map(|handle| block_name(document, handle, inherited_name))
            .unwrap_or_else(|| block_name(document, inherited_handle, inherited_name))
    };
    let linetype = |code, inherited| {
        linetype_name(document, ov::handle(data, code).unwrap_or(inherited))
    };
    let arrow_1 = if matches!(dimension, Dimension::Radius(_)) {
        let inherited = if s.dimblk1.is_null() { s.dimblk } else { s.dimblk1 };
        let inherited_name = if s.dimblk1.is_null() {
            &s.dimblk_name
        } else {
            &s.dimblk1_name
        };
        block_name(
            document,
            ov::handle(data, ov::DIMBLK1)
                .or_else(|| ov::handle(data, ov::DIMBLK))
                .unwrap_or(inherited),
            inherited_name,
        )
    } else {
        arrow_name(ov::DIMBLK1, s.dimblk1, &s.dimblk1_name)
    };
    let arrow_2 = arrow_name(ov::DIMBLK2, s.dimblk2, &s.dimblk2_name);
    for current in [&arrow_1, &arrow_2] {
        if !arrow_options.contains(current) {
            arrow_options.push(current.clone());
        }
    }
    for current in [
        linetype(ov::DIMLTYPE, s.dimltex_handle),
        linetype(ov::DIMLTEX1, s.dimltex1_handle),
        linetype(ov::DIMLTEX2, s.dimltex2_handle),
    ] {
        if !linetype_options.contains(&current) {
            linetype_options.push(current);
        }
    }

    let precision_options: Vec<String> = (0..=8).map(|value| value.to_string()).collect();
    let linear_units = [
        "Scientific",
        "Decimal",
        "Engineering",
        "Architectural",
        "Fractional",
        "Desktop",
    ];
    let alternate_unit_options = [
        "Scientific",
        "Decimal",
        "Engineering",
        "Architectural stacked",
        "Fractional stacked",
        "Architectural",
        "Fractional",
        "Desktop",
    ];

    let fill_mode = int(ov::DIMTFILL, s.dimtfill);
    let fill_color = ov::color(data, ov::DIMTFILLCLR)
        .unwrap_or_else(|| AcadColor::from_index(s.dimtfillclr));
    let fill_value = match fill_mode {
        1 => choice_value("Background", &["None", "Background", "Color"]),
        2 => PropValue::ColorChoice(fill_color),
        _ => choice_value("None", &["None", "Background", "Color"]),
    };

    let mut sections = vec![
        PropSection {
            title: t!("Lines & Arrows").into_owned(),
            props: vec![
                property(
                    t!("Arrow 1").as_ref(),
                    "dim_arrowhead_1",
                    PropValue::Choice {
                        selected: arrow_1,
                        options: arrow_options.clone(),
                    },
                ),
                property(
                    t!("Arrow 2").as_ref(),
                    "dim_arrowhead_2",
                    PropValue::Choice {
                        selected: arrow_2,
                        options: arrow_options,
                    },
                ),
                number(
                    t!("Arrow size").as_ref(),
                    "dim_arrow_size",
                    real(ov::DIMASZ, s.dimasz),
                    true,
                ),
                property(
                    t!("Dim line lineweight").as_ref(),
                    "dim_line_lineweight",
                    PropValue::Choice {
                        selected: lineweight_label(int(ov::DIMLWD, s.dimlwd)),
                        options: lineweight_options(),
                    },
                ),
                property(
                    t!("Ext line lineweight").as_ref(),
                    "dim_ext_line_lineweight",
                    PropValue::Choice {
                        selected: lineweight_label(int(ov::DIMLWE, s.dimlwe)),
                        options: lineweight_options(),
                    },
                ),
                choice(
                    t!("Dim line 1").as_ref(),
                    "dim_line_1",
                    on(int(ov::DIMSD1, s.dimsd1 as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Dim line 2").as_ref(),
                    "dim_line_2",
                    on(int(ov::DIMSD2, s.dimsd2 as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                property(
                    t!("Dim line color").as_ref(),
                    "dim_line_color",
                    PropValue::ColorChoice(
                        ov::color(data, ov::DIMCLRD)
                            .unwrap_or_else(|| AcadColor::from_index(s.dimclrd)),
                    ),
                ),
                property(
                    t!("Dim line linetype").as_ref(),
                    "dim_linetype",
                    PropValue::Choice {
                        selected: linetype(ov::DIMLTYPE, s.dimltex_handle),
                        options: linetype_options.clone(),
                    },
                ),
                number(
                    t!("Dim line ext").as_ref(),
                    "dim_line_ext",
                    real(ov::DIMDLE, s.dimdle),
                    true,
                ),
                property(
                    t!("Ext line 1 linetype").as_ref(),
                    "dim_ext_linetype_1",
                    PropValue::Choice {
                        selected: linetype(ov::DIMLTEX1, s.dimltex1_handle),
                        options: linetype_options.clone(),
                    },
                ),
                property(
                    t!("Ext line 2 linetype").as_ref(),
                    "dim_ext_linetype_2",
                    PropValue::Choice {
                        selected: linetype(ov::DIMLTEX2, s.dimltex2_handle),
                        options: linetype_options,
                    },
                ),
                choice(
                    t!("Ext line 1").as_ref(),
                    "dim_ext_line_1",
                    on(int(ov::DIMSE1, s.dimse1 as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Ext line 2").as_ref(),
                    "dim_ext_line_2",
                    on(int(ov::DIMSE2, s.dimse2 as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Ext line fixed").as_ref(),
                    "dim_ext_line_fixed",
                    on(dimfxlon),
                    &["On", "Off"],
                    true,
                ),
                number(
                    t!("Ext line fixed length").as_ref(),
                    "dim_ext_line_fixed_length",
                    real(ov::DIMFXL, s.dimfxl),
                    dimfxlon,
                ),
                property(
                    t!("Ext line color").as_ref(),
                    "dim_ext_line_color",
                    PropValue::ColorChoice(
                        ov::color(data, ov::DIMCLRE)
                            .unwrap_or_else(|| AcadColor::from_index(s.dimclre)),
                    ),
                ),
                number(
                    t!("Ext line ext").as_ref(),
                    "dim_ext_line_ext",
                    real(ov::DIMEXE, s.dimexe),
                    true,
                ),
                number(
                    t!("Ext line offset").as_ref(),
                    "dim_ext_line_offset",
                    real(ov::DIMEXO, s.dimexo),
                    true,
                ),
            ],
        },
        PropSection {
            title: t!("Text").into_owned(),
            props: vec![
                property(
                    t!("Fill color").as_ref(),
                    "dim_text_fill_color",
                    fill_value,
                ),
                choice(
                    t!("Fractional type").as_ref(),
                    "dim_fractional_type",
                    fraction_type_label(dimfrac),
                    &["Horizontal", "Diagonal", "Not stacked"],
                    matches!(dimlunit, 4 | 5),
                ),
                property(
                    t!("Text color").as_ref(),
                    "dim_text_color",
                    PropValue::ColorChoice(
                        ov::color(data, ov::DIMCLRT)
                            .unwrap_or_else(|| AcadColor::from_index(s.dimclrt)),
                    ),
                ),
                number(
                    t!("Text height").as_ref(),
                    "dim_text_height",
                    real(ov::DIMTXT, s.dimtxt),
                    text_height_editable,
                ),
                number(
                    t!("Text offset").as_ref(),
                    "dim_text_offset",
                    dimgap.abs(),
                    true,
                ),
                choice(
                    t!("Text outside align").as_ref(),
                    "dim_text_outside_align",
                    on(int(ov::DIMTOH, s.dimtoh as i16) != 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Text pos hor").as_ref(),
                    "dim_text_pos_hor",
                    text_horizontal_label(int(ov::DIMJUST, s.dimjust)),
                    &[
                        "Centered",
                        "At extension line 1",
                        "At extension line 2",
                        "Over extension line 1",
                        "Over extension line 2",
                    ],
                    true,
                ),
                choice(
                    t!("Text pos vert").as_ref(),
                    "dim_text_pos_vert",
                    text_vertical_label(int(ov::DIMTAD, s.dimtad)),
                    &["Centered", "Above", "Outside", "JIS", "Below"],
                    true,
                ),
                property(
                    t!("Text style").as_ref(),
                    "dim_text_style",
                    PropValue::Choice {
                        selected: text_style_name,
                        options: text_style_options,
                    },
                ),
                choice(
                    t!("Text inside align").as_ref(),
                    "dim_text_inside_align",
                    on(int(ov::DIMTIH, s.dimtih as i16) != 0),
                    &["On", "Off"],
                    dimtix,
                ),
                property(
                    t!("Text position X").as_ref(),
                    "text_x",
                    PropValue::EditText(format!(
                        "{:.4}",
                        dimension.base().text_middle_point.x
                    )),
                ),
                property(
                    t!("Text position Y").as_ref(),
                    "text_y",
                    PropValue::EditText(format!(
                        "{:.4}",
                        dimension.base().text_middle_point.y
                    )),
                ),
                property(
                    t!("Text rotation").as_ref(),
                    "text_rotation",
                    PropValue::EditText(format!(
                        "{:.4}",
                        dimension.base().text_rotation.to_degrees()
                    )),
                ),
                choice(
                    t!("Text view direction").as_ref(),
                    "dim_text_view_direction",
                    if int(ov::DIMTXTDIRECTION, s.dimtxtdirection as i16) != 0 {
                        "Right-to-left"
                    } else {
                        "Left-to-right"
                    },
                    &["Left-to-right", "Right-to-left"],
                    true,
                ),
                property(
                    t!("Measurement").as_ref(),
                    "measurement",
                    PropValue::ReadOnly(format!("{:.4}", dimension.measurement())),
                ),
                property(
                    t!("Text override").as_ref(),
                    "text_override",
                    PropValue::PlainText(
                        dimension_text_override(dimension.base())
                            .unwrap_or("")
                            .to_string(),
                    ),
                ),
            ],
        },
        PropSection {
            title: t!("Fit").into_owned(),
            props: vec![
                choice(
                    t!("Fit").as_ref(),
                    "dim_fit",
                    fit_label(int(ov::DIMATFIT, s.dimatfit)),
                    &["Both text and arrows", "Arrows", "Text", "Best fit"],
                    true,
                ),
                choice(
                    t!("Text inside").as_ref(),
                    "dim_text_inside",
                    on(dimtix),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Text movement").as_ref(),
                    "dim_text_movement",
                    text_movement_label(int(ov::DIMTMOVE, s.dimtmove)),
                    &[
                        "Keep dim line with text",
                        "Move text, add leader",
                        "Move text, no leader",
                    ],
                    true,
                ),
                number(
                    t!("Dim scale overall").as_ref(),
                    "dim_scale_overall",
                    real(ov::DIMSCALE, s.dimscale),
                    !annotative,
                ),
                choice(
                    t!("Dim line forced").as_ref(),
                    "dim_line_forced",
                    on(int(ov::DIMTOFL, s.dimtofl as i16) != 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Dim line inside").as_ref(),
                    "dim_line_inside",
                    on(int(ov::DIMSOXD, s.dimsoxd as i16) != 0),
                    &["On", "Off"],
                    true,
                ),
            ],
        },
        PropSection {
            title: t!("Primary Units").into_owned(),
            props: vec![
                text(
                    t!("Decimal separator").as_ref(),
                    "dim_decimal_separator",
                    decimal_separator.clone(),
                    true,
                ),
                text(
                    t!("Prefix").as_ref(),
                    "dim_prefix",
                    dim_prefix.clone(),
                    true,
                ),
                text(
                    t!("Suffix").as_ref(),
                    "dim_suffix",
                    dim_suffix.clone(),
                    true,
                ),
                text(
                    t!("Dim sub-units suffix").as_ref(),
                    "dim_sub_units_suffix",
                    string(ov::DIMMZS, &s.dimmzs),
                    dimzin & 4 != 0,
                ),
                number(
                    t!("Dim roundoff").as_ref(),
                    "dim_roundoff",
                    real(ov::DIMRND, s.dimrnd),
                    true,
                ),
                number(
                    t!("Dim scale linear").as_ref(),
                    "dim_scale_linear",
                    real(ov::DIMLFAC, s.dimlfac),
                    true,
                ),
                number(
                    t!("Dim sub-units scale").as_ref(),
                    "dim_sub_units_scale",
                    real(ov::DIMMZF, s.dimmzf),
                    dimzin & 4 != 0,
                ),
                choice(
                    t!("Dim units").as_ref(),
                    "dim_units",
                    linear_unit_label(dimlunit),
                    &linear_units,
                    true,
                ),
                choice(
                    t!("Suppress leading zeros").as_ref(),
                    "dim_suppress_leading_zeros",
                    yes(dimzin & 4 != 0),
                    &["Yes", "No"],
                    true,
                ),
                choice(
                    t!("Suppress trailing zeros").as_ref(),
                    "dim_suppress_trailing_zeros",
                    yes(dimzin & 8 != 0),
                    &["Yes", "No"],
                    true,
                ),
                choice(
                    t!("Suppress zero feet").as_ref(),
                    "dim_suppress_zero_feet",
                    yes(suppresses_zero_feet(dimzin)),
                    &["Yes", "No"],
                    true,
                ),
                choice(
                    t!("Suppress zero inches").as_ref(),
                    "dim_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimzin)),
                    &["Yes", "No"],
                    true,
                ),
                property(
                    t!("Precision").as_ref(),
                    "dim_precision",
                    PropValue::Choice {
                        selected: int(ov::DIMDEC, s.dimdec).to_string(),
                        options: precision_options.clone(),
                    },
                ),
            ],
        },
        PropSection {
            title: t!("Alternate Units").into_owned(),
            props: vec![
                choice(
                    t!("Alt enabled").as_ref(),
                    "dim_alt_enabled",
                    on(dimalt),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Alt format").as_ref(),
                    "dim_alt_format",
                    alternate_unit_label(int(ov::DIMALTU, s.dimaltu)),
                    &alternate_unit_options,
                    dimalt,
                ),
                property(
                    t!("Alt precision").as_ref(),
                    "dim_alt_precision",
                    if dimalt {
                        PropValue::Choice {
                            selected: int(ov::DIMALTD, s.dimaltd).to_string(),
                            options: precision_options.clone(),
                        }
                    } else {
                        PropValue::ReadOnly(int(ov::DIMALTD, s.dimaltd).to_string())
                    },
                ),
                number(
                    t!("Alt scale factor").as_ref(),
                    "dim_alt_scale_factor",
                    real(ov::DIMALTF, s.dimaltf),
                    dimalt,
                ),
                number(
                    t!("Alt sub-units scale").as_ref(),
                    "dim_alt_sub_units_scale",
                    real(ov::DIMALTMZF, s.dimaltmzf),
                    dimalt && dimaltz & 4 != 0,
                ),
                number(
                    t!("Alt roundoff").as_ref(),
                    "dim_alt_roundoff",
                    real(ov::DIMALTRND, s.dimaltrnd),
                    dimalt,
                ),
                text(
                    t!("Alt prefix").as_ref(),
                    "dim_alt_prefix",
                    alt_prefix,
                    dimalt,
                ),
                text(
                    t!("Alt suffix").as_ref(),
                    "dim_alt_suffix",
                    alt_suffix,
                    dimalt,
                ),
                text(
                    t!("Alt sub-units suffix").as_ref(),
                    "dim_alt_sub_units_suffix",
                    string(ov::DIMALTMZS, &s.dimaltmzs),
                    dimalt && dimaltz & 4 != 0,
                ),
                choice(
                    t!("Alt suppress leading zeros").as_ref(),
                    "dim_alt_suppress_leading_zeros",
                    yes(dimaltz & 4 != 0),
                    &["Yes", "No"],
                    dimalt,
                ),
                choice(
                    t!("Alt suppress trailing zeros").as_ref(),
                    "dim_alt_suppress_trailing_zeros",
                    yes(dimaltz & 8 != 0),
                    &["Yes", "No"],
                    dimalt,
                ),
                choice(
                    t!("Alt suppress zero feet").as_ref(),
                    "dim_alt_suppress_zero_feet",
                    yes(suppresses_zero_feet(dimaltz)),
                    &["Yes", "No"],
                    dimalt,
                ),
                choice(
                    t!("Alt suppress zero inches").as_ref(),
                    "dim_alt_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimaltz)),
                    &["Yes", "No"],
                    dimalt,
                ),
            ],
        },
        PropSection {
            title: t!("Tolerances").into_owned(),
            props: vec![
                choice(
                    t!("Tolerance display").as_ref(),
                    "dim_tolerance_display",
                    tolerance_display,
                    &["None", "Symmetrical", "Deviation", "Limits", "Basic"],
                    true,
                ),
                property(
                    t!("Tolerance precision").as_ref(),
                    "dim_tolerance_precision",
                    if tolerance_enabled {
                        PropValue::Choice {
                            selected: int(ov::DIMTDEC, s.dimtdec).to_string(),
                            options: precision_options.clone(),
                        }
                    } else {
                        PropValue::ReadOnly(int(ov::DIMTDEC, s.dimtdec).to_string())
                    },
                ),
                number(
                    t!("Tolerance limit lower").as_ref(),
                    "dim_tolerance_limit_lower",
                    dimtm,
                    matches!(tolerance_display, "Deviation" | "Limits"),
                ),
                number(
                    t!("Tolerance limit upper").as_ref(),
                    "dim_tolerance_limit_upper",
                    dimtp,
                    tolerance_enabled,
                ),
                number(
                    t!("Tolerance text height").as_ref(),
                    "dim_tolerance_text_height",
                    real(ov::DIMTFAC, s.dimtfac),
                    tolerance_enabled,
                ),
                choice(
                    t!("Tolerance pos vert").as_ref(),
                    "dim_tolerance_pos_vert",
                    tolerance_vertical_label(int(ov::DIMTOLJ, s.dimtolj)),
                    &["Bottom", "Middle", "Top"],
                    true,
                ),
                choice(
                    t!("Tolerance alignment").as_ref(),
                    "dim_tolerance_alignment",
                    tolerance_alignment_label(int(ov::DIMTALN, 0)),
                    &["Align decimal separators", "Align operational symbols"],
                    true,
                ),
                choice(
                    t!("Tolerance suppress leading zeros").as_ref(),
                    "dim_tolerance_suppress_leading_zeros",
                    yes(dimtzin & 4 != 0),
                    &["Yes", "No"],
                    tolerance_enabled,
                ),
                choice(
                    t!("Tolerance suppress trailing zeros").as_ref(),
                    "dim_tolerance_suppress_trailing_zeros",
                    yes(dimtzin & 8 != 0),
                    &["Yes", "No"],
                    tolerance_enabled,
                ),
                choice(
                    t!("Tolerance suppress zero feet").as_ref(),
                    "dim_tolerance_suppress_zero_feet",
                    yes(suppresses_zero_feet(dimtzin)),
                    &["Yes", "No"],
                    tolerance_enabled,
                ),
                choice(
                    t!("Tolerance suppress zero inches").as_ref(),
                    "dim_tolerance_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimtzin)),
                    &["Yes", "No"],
                    tolerance_enabled,
                ),
                property(
                    t!("Alt tolerance precision").as_ref(),
                    "dim_alt_tolerance_precision",
                    if alternate_tolerance_enabled {
                        PropValue::Choice {
                            selected: int(ov::DIMALTTD, s.dimalttd).to_string(),
                            options: precision_options,
                        }
                    } else {
                        PropValue::ReadOnly(int(ov::DIMALTTD, s.dimalttd).to_string())
                    },
                ),
                choice(
                    t!("Alt tolerance suppress leading zeros").as_ref(),
                    "dim_alt_tolerance_suppress_leading_zeros",
                    yes(dimalttz & 4 != 0),
                    &["Yes", "No"],
                    alternate_tolerance_enabled,
                ),
                choice(
                    t!("Alt tolerance suppress trailing zeros").as_ref(),
                    "dim_alt_tolerance_suppress_trailing_zeros",
                    yes(dimalttz & 8 != 0),
                    &["Yes", "No"],
                    alternate_tolerance_enabled,
                ),
                choice(
                    t!("Alt tolerance suppress zero feet").as_ref(),
                    "dim_alt_tolerance_suppress_zero_feet",
                    yes(suppresses_zero_feet(dimalttz)),
                    &["Yes", "No"],
                    alternate_tolerance_enabled,
                ),
                choice(
                    t!("Alt tolerance suppress zero inches").as_ref(),
                    "dim_alt_tolerance_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimalttz)),
                    &["Yes", "No"],
                    alternate_tolerance_enabled,
                ),
            ],
        },
    ];

    if matches!(dimension, Dimension::Radius(_)) {
        let dimcen = real(ov::DIMCEN, s.dimcen);
        let center_type = if dimcen > 1e-12 {
            "Center marks"
        } else if dimcen < -1e-12 {
            "Centerlines"
        } else {
            "None"
        };
        if let Some(lines) = sections
            .iter_mut()
            .find(|section| section.title == t!("Lines & Arrows").as_ref())
        {
            const RADIUS_LINE_FIELDS: &[&str] = &[
                "dim_arrowhead_1",
                "dim_arrow_size",
                "dim_line_lineweight",
                "dim_ext_line_lineweight",
                "dim_line_1",
                "dim_line_color",
                "dim_linetype",
                "dim_ext_linetype_1",
                "dim_ext_line_1",
                "dim_ext_line_color",
                "dim_ext_line_ext",
                "dim_ext_line_offset",
            ];
            lines
                .props
                .retain(|property| RADIUS_LINE_FIELDS.contains(&property.field));
            lines.props.push(choice(
                t!("Center mark").as_ref(),
                "dim_center_type",
                center_type,
                &["None", "Center marks", "Centerlines"],
                true,
            ));
            lines.props.push(number(
                t!("Center mark size").as_ref(),
                "dim_center_size",
                dimcen.abs(),
                center_type != "None",
            ));
        }
        if let Some(primary_units) = sections
            .iter_mut()
            .find(|section| section.title == t!("Primary Units").as_ref())
        {
            primary_units.props.retain(|property| {
                !matches!(
                    property.field,
                    "dim_sub_units_suffix" | "dim_sub_units_scale"
                )
            });
        }
    }

    if matches!(dimension, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)) {
        if let Some(text_section) = sections
            .iter_mut()
            .find(|section| section.title == t!("Text").as_ref())
        {
            text_section
                .props
                .retain(|property| property.field != "dim_fractional_type");
        }

        let angle_zero_suppression = int(ov::DIMAZIN, s.dimazin);
        let angle_unit = int(ov::DIMAUNIT, s.dimaunit);
        if let Some(primary_units) = sections
            .iter_mut()
            .find(|section| section.title == t!("Primary Units").as_ref())
        {
            primary_units.props = vec![
                text(
                    t!("Decimal separator").as_ref(),
                    "dim_decimal_separator",
                    decimal_separator,
                    true,
                ),
                text(t!("Prefix").as_ref(), "dim_prefix", dim_prefix, true),
                text(t!("Suffix").as_ref(), "dim_suffix", dim_suffix, true),
                choice(
                    t!("Angle format").as_ref(),
                    "dim_angle_units",
                    angular_unit_label(angle_unit),
                    &[
                        "Decimal degrees",
                        "Degrees/minutes/seconds",
                        "Gradians",
                        "Radians",
                    ],
                    true,
                ),
                choice(
                    t!("Suppress leading zeros").as_ref(),
                    "dim_angle_suppress_leading_zeros",
                    yes(angle_zero_suppression & 1 != 0),
                    &["Yes", "No"],
                    true,
                ),
                choice(
                    t!("Suppress trailing zeros").as_ref(),
                    "dim_angle_suppress_trailing_zeros",
                    yes(angle_zero_suppression & 2 != 0),
                    &["Yes", "No"],
                    true,
                ),
                property(
                    t!("Precision").as_ref(),
                    "dim_angle_precision",
                    PropValue::Choice {
                        selected: int(ov::DIMADEC, s.dimadec).to_string(),
                        options: (-1..=8).map(|value| value.to_string()).collect(),
                    },
                ),
            ];
        }
        sections.retain(|section| section.title != t!("Alternate Units").as_ref());
        if let Some(tolerances) = sections
            .iter_mut()
            .find(|section| section.title == t!("Tolerances").as_ref())
        {
            tolerances
                .props
                .retain(|property| !property.field.starts_with("dim_alt_tolerance_"));
        }
    }

    sections
}

fn property(label: &str, field: &'static str, value: PropValue) -> Property {
    Property {
        label: label.to_string(),
        field,
        value,
    }
}

fn split_measurement_template(value: &str) -> (String, String) {
    value
        .split_once("<>")
        .map(|(prefix, suffix)| (prefix.to_string(), suffix.to_string()))
        .unwrap_or_else(|| (String::new(), value.to_string()))
}

fn fraction_type_label(value: i16) -> &'static str {
    match value {
        1 => "Diagonal",
        2 => "Not stacked",
        _ => "Horizontal",
    }
}

fn suppresses_zero_feet(value: i16) -> bool {
    matches!(value & 3, 0 | 3)
}

fn suppresses_zero_inches(value: i16) -> bool {
    matches!(value & 3, 0 | 2)
}

fn text_horizontal_label(value: i16) -> &'static str {
    match value {
        1 => "At extension line 1",
        2 => "At extension line 2",
        3 => "Over extension line 1",
        4 => "Over extension line 2",
        _ => "Centered",
    }
}

fn text_vertical_label(value: i16) -> &'static str {
    match value {
        1 => "Above",
        2 => "Outside",
        3 => "JIS",
        4 => "Below",
        _ => "Centered",
    }
}

fn fit_label(value: i16) -> &'static str {
    match value {
        1 => "Arrows",
        2 => "Text",
        3 => "Best fit",
        _ => "Both text and arrows",
    }
}

fn text_movement_label(value: i16) -> &'static str {
    match value {
        1 => "Move text, add leader",
        2 => "Move text, no leader",
        _ => "Keep dim line with text",
    }
}

fn alternate_unit_label(value: i16) -> &'static str {
    match value {
        1 => "Scientific",
        3 => "Engineering",
        4 => "Architectural stacked",
        5 => "Fractional stacked",
        6 => "Architectural",
        7 => "Fractional",
        8 => "Desktop",
        _ => "Decimal",
    }
}

fn angular_unit_label(value: i16) -> &'static str {
    match value {
        1 => "Degrees/minutes/seconds",
        2 => "Gradians",
        3 => "Radians",
        _ => "Decimal degrees",
    }
}

fn tolerance_vertical_label(value: i16) -> &'static str {
    match value {
        0 => "Bottom",
        2 => "Top",
        _ => "Middle",
    }
}

fn tolerance_alignment_label(value: i16) -> &'static str {
    if value == 0 {
        "Align decimal separators"
    } else {
        "Align operational symbols"
    }
}

fn choice_value(
    selected: &str,
    options: &[&str],
) -> crate::scene::model::object::PropValue {
    crate::scene::model::object::PropValue::Choice {
        selected: selected.to_string(),
        options: options.iter().map(|value| (*value).to_string()).collect(),
    }
}

fn block_name(document: &CadDocument, handle: acadrust::Handle, fallback: &str) -> String {
    if handle.is_null() {
        return "Closed filled".to_string();
    }
    document
        .block_records
        .iter()
        .find(|record| record.handle == handle)
        .map(|record| record.name.clone())
        .unwrap_or_else(|| fallback.to_string())
}

fn linetype_name(document: &CadDocument, handle: acadrust::Handle) -> String {
    document
        .line_types
        .iter()
        .find(|line_type| line_type.handle == handle)
        .map(|line_type| line_type.name.clone())
        .unwrap_or_else(|| "ByBlock".to_string())
}

fn linear_unit_label(value: i16) -> &'static str {
    match value {
        1 => "Scientific",
        3 => "Engineering",
        4 => "Architectural",
        5 => "Fractional",
        6 => "Desktop",
        _ => "Decimal",
    }
}

use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::convert::tess_util::aci_to_rgba;
use crate::scene::convert::tessellate::{
    add_polyline, add_segment, append_arrow, arrow_from_block_with_deferred_hatch,
    normalized_or, ArrowKind, DimGeom,
};
use crate::scene::model::wire_model::{SnapHint, WireModel};

fn apply_dimension_breaks(
    document: &CadDocument,
    dimension: Handle,
    lines: &mut Vec<[f32; 3]>,
) {
    let references: Vec<_> = document
        .objects
        .values()
        .filter_map(|object| {
            let acadrust::objects::ObjectType::DataObject(object) = object else {
                return None;
            };
            let acadrust::objects::DataObjectData::BreakData(data) = &object.data
            else {
                return None;
            };
            (data.dimension_reference == dimension).then_some(&data.point_references)
        })
        .flatten()
        .filter(|reference| {
            let points = [reference.first_point, reference.second_point];
            points.iter().all(|point| {
                point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
            })
        })
        .collect();
    if references.is_empty() || lines.len() < 2 {
        return;
    }

    let mut output = Vec::with_capacity(lines.len() + references.len() * 3);
    for run in lines.split(|point| point[0].is_nan()) {
        for segment in run.windows(2) {
            let a = Vec3::from_array(segment[0]);
            let b = Vec3::from_array(segment[1]);
            let direction = b - a;
            let length_squared = direction.length_squared();
            if length_squared <= 1e-12 {
                continue;
            }
            let mut intervals = vec![(0.0f32, 1.0f32)];
            for reference in &references {
                let first = vec3_local(reference.first_point);
                let second = vec3_local(reference.second_point);
                let t1 = (first - a).dot(direction) / length_squared;
                let t2 = (second - a).dot(direction) / length_squared;
                let closest1 = a + direction * t1.clamp(0.0, 1.0);
                let closest2 = a + direction * t2.clamp(0.0, 1.0);
                let requested_gap = (second - first).length();
                let tolerance = (requested_gap * 0.25)
                    .max(direction.length() * 1e-5)
                    .max(1e-4);
                if (first - closest1).length() > tolerance
                    || (second - closest2).length() > tolerance
                {
                    continue;
                }
                let half_point_gap = if requested_gap <= 1e-6 {
                    (tolerance / direction.length()).min(0.1)
                } else {
                    0.0
                };
                let cut_start = (t1.min(t2) - half_point_gap).clamp(0.0, 1.0);
                let cut_end = (t1.max(t2) + half_point_gap).clamp(0.0, 1.0);
                if cut_end <= cut_start {
                    continue;
                }
                let mut remaining = Vec::new();
                for (start, end) in intervals {
                    if cut_end <= start || cut_start >= end {
                        remaining.push((start, end));
                        continue;
                    }
                    if cut_start > start {
                        remaining.push((start, cut_start));
                    }
                    if cut_end < end {
                        remaining.push((cut_end, end));
                    }
                }
                intervals = remaining;
            }
            for (start, end) in intervals {
                if end - start > 1e-6 {
                    add_segment(
                        &mut output,
                        a + direction * start,
                        a + direction * end,
                    );
                }
            }
        }
    }
    *lines = output;
}

pub trait DimensionTess {
    fn tessellate(
        &self,
        document: &CadDocument,
        handle: Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        selected_set: &rustc_hash::FxHashSet<acadrust::Handle>,
        active_viewport: Option<acadrust::Handle>,
        bg_color: [f32; 4],
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
    ) -> Vec<WireModel>;
}

impl DimensionTess for Dimension {
    fn tessellate(
        &self,
        document: &CadDocument,
        handle: Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        selected_set: &rustc_hash::FxHashSet<acadrust::Handle>,
        active_viewport: Option<acadrust::Handle>,
        bg_color: [f32; 4],
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
    ) -> Vec<WireModel> {
        tessellate_dimension_inner(
            document,
            handle,
            self,
            selected,
            entity_color,
            line_weight_px,
            anno_scale,
            selected_set,
            active_viewport,
            bg_color,
            view_aabb,
            world_per_pixel,
        )
    }
}

fn tessellate_dimension_inner(
    document: &CadDocument,
    handle: Handle,
    dim: &Dimension,
    selected: bool,
    entity_color: [f32; 4],
    line_weight_px: f32,
    anno_scale: f32,
    // LOD hints — when present, synthesised dim text routes through the
    // top-level LOD ladder (baseline / greek / full) instead of the render
    // path so far-out drawings collapse to a colored rect or baseline.
    selected_set: &rustc_hash::FxHashSet<acadrust::Handle>,
    active_viewport: Option<acadrust::Handle>,
    bg_color: [f32; 4],
    view_aabb: Option<[f32; 4]>,
    world_per_pixel: Option<f32>,
) -> Vec<WireModel> {
    let name = handle.value().to_string();
    // (Baked-block fast path moved up into scene::tessellate_entity so the
    // recursive call goes through the LOD ladder, not the kernel path.)

    let style_name = &dim.base().style_name;
    let source_style = document.dim_styles.iter().find(|s| {
        s.name.eq_ignore_ascii_case(style_name)
            || (style_name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
    });
    let mut effective_style = source_style.cloned();
    if let Some(style) = &mut effective_style {
        use crate::entities::dim_override as ov;
        let data = &dim.base().common.extended_data;
        macro_rules! real {
            ($field:ident, $code:ident) => {
                if let Some(value) = ov::real(data, ov::$code) {
                    style.$field = value;
                }
            };
        }
        macro_rules! int {
            ($field:ident, $code:ident) => {
                if let Some(value) = ov::int(data, ov::$code) {
                    style.$field = value;
                }
            };
        }
        macro_rules! flag {
            ($field:ident, $code:ident) => {
                if let Some(value) = ov::int(data, ov::$code) {
                    style.$field = value != 0;
                }
            };
        }
        macro_rules! handle {
            ($field:ident, $code:ident) => {
                if let Some(value) = ov::handle(data, ov::$code) {
                    style.$field = value;
                }
            };
        }
        real!(dimscale, DIMSCALE);
        real!(dimasz, DIMASZ);
        real!(dimexo, DIMEXO);
        real!(dimdli, DIMDLI);
        real!(dimexe, DIMEXE);
        real!(dimrnd, DIMRND);
        real!(dimdle, DIMDLE);
        real!(dimtp, DIMTP);
        real!(dimtm, DIMTM);
        real!(dimfxl, DIMFXL);
        real!(dimjogang, DIMJOGANG);
        real!(dimtxt, DIMTXT);
        real!(dimcen, DIMCEN);
        real!(dimtsz, DIMTSZ);
        real!(dimaltf, DIMALTF);
        real!(dimlfac, DIMLFAC);
        real!(dimtvp, DIMTVP);
        real!(dimtfac, DIMTFAC);
        real!(dimgap, DIMGAP);
        real!(dimaltrnd, DIMALTRND);
        real!(dimaltmzf, DIMALTMZF);
        real!(dimmzf, DIMMZF);
        flag!(dimtol, DIMTOL);
        flag!(dimlim, DIMLIM);
        flag!(dimtih, DIMTIH);
        flag!(dimtoh, DIMTOH);
        flag!(dimse1, DIMSE1);
        flag!(dimse2, DIMSE2);
        flag!(dimalt, DIMALT);
        flag!(dimtofl, DIMTOFL);
        flag!(dimsah, DIMSAH);
        flag!(dimtix, DIMTIX);
        flag!(dimsoxd, DIMSOXD);
        flag!(dimsd1, DIMSD1);
        flag!(dimsd2, DIMSD2);
        flag!(dimupt, DIMUPT);
        flag!(dimfxlon, DIMFXLON);
        flag!(dimtxtdirection, DIMTXTDIRECTION);
        int!(dimzin, DIMZIN);
        int!(dimtad, DIMTAD);
        int!(dimazin, DIMAZIN);
        int!(dimarcsym, DIMARCSYM);
        int!(dimclrd, DIMCLRD);
        int!(dimclre, DIMCLRE);
        int!(dimclrt, DIMCLRT);
        int!(dimadec, DIMADEC);
        int!(dimaltd, DIMALTD);
        int!(dimdec, DIMDEC);
        int!(dimtdec, DIMTDEC);
        int!(dimaltu, DIMALTU);
        int!(dimalttd, DIMALTTD);
        int!(dimaunit, DIMAUNIT);
        int!(dimfrac, DIMFRAC);
        int!(dimlunit, DIMLUNIT);
        int!(dimdsep, DIMDSEP);
        int!(dimtmove, DIMTMOVE);
        int!(dimjust, DIMJUST);
        int!(dimtolj, DIMTOLJ);
        int!(dimtzin, DIMTZIN);
        int!(dimaltz, DIMALTZ);
        int!(dimalttz, DIMALTTZ);
        int!(dimatfit, DIMATFIT);
        int!(dimtfill, DIMTFILL);
        int!(dimtfillclr, DIMTFILLCLR);
        int!(dimlwd, DIMLWD);
        int!(dimlwe, DIMLWE);
        handle!(dimldrblk, DIMLDRBLK);
        handle!(dimblk, DIMBLK);
        handle!(dimblk1, DIMBLK1);
        handle!(dimblk2, DIMBLK2);
        handle!(dimltex_handle, DIMLTYPE);
        handle!(dimltex1_handle, DIMLTEX1);
        handle!(dimltex2_handle, DIMLTEX2);
        if let Some(value) = ov::string(data, ov::DIMPOST) {
            style.dimpost = value;
        }
        if let Some(value) = ov::string(data, ov::DIMAPOST) {
            style.dimapost = value;
        }
        if let Some(value) = ov::string(data, ov::DIMALTMZS) {
            style.dimaltmzs = value;
        }
        if let Some(value) = ov::string(data, ov::DIMMZS) {
            style.dimmzs = value;
        }
        if let Some(value) = ov::handle(data, ov::DIMTXSTY) {
            style.dimtxsty_handle = value;
            if let Some(record) = document.text_styles.iter().find(|record| record.handle == value) {
                style.dimtxsty = record.name.clone();
            }
        }
    }
    let style = effective_style.as_ref();

    // A positive style scale is fixed. A zero style scale uses the multiplier
    // resolved by the scene for the current annotation or viewport context.
    let dim_scale = style
        .map(|s| {
            if s.dimscale > 1e-6 {
                s.dimscale
            } else {
                anno_scale as f64
            }
        })
        .unwrap_or(1.0);

    let (
        dimasz_raw,
        dimexo,
        dimexe,
        dim_txt,
        dimtsz_raw,
        dimsah,
        dimse1,
        dimse2,
        dimsd1,
        dimsd2,
        dimdle,
        dimfxl,
        dimfxlon,
        dimsoxd,
        dimcen,
    ) = style
        .map(|s| {
            (
                s.dimasz * dim_scale,
                (s.dimexo * dim_scale) as f32,
                (s.dimexe * dim_scale) as f32,
                s.dimtxt * dim_scale,
                s.dimtsz * dim_scale,
                s.dimsah,
                s.dimse1,
                s.dimse2,
                s.dimsd1,
                s.dimsd2,
                (s.dimdle * dim_scale) as f32,
                (s.dimfxl * dim_scale) as f32,
                s.dimfxlon,
                s.dimsoxd,
                (s.dimcen * dim_scale) as f32,
            )
        })
        .unwrap_or((
            0.18, 0.0, 0.0, 2.5, 0.0, false, false, false, false, false, 0.0, 1.0, false, false,
            0.09,
        ));

    // Arrow selection precedence:
    //   1. DIMTSZ>0 → oblique tick (overrides DIMBLK*).
    //   2. DIMSAH false → DIMBLK on both ends.
    //   3. DIMSAH true  → DIMBLK1 (first end), DIMBLK2 (second end).
    // Unknown / NULL block handles fall back to ClosedFilled.
    let dimasz = (dimasz_raw as f32).max(0.001);
    let defer_arrow_hatches = {
        let name = dim.base().block_name.trim();
        let mut memo = std::collections::HashMap::new();
        !name.is_empty()
            && crate::scene::render_graph::block_contains_hatch(
                document,
                name,
                &mut memo,
            )
    };
    let (arrow1, arrow2) = if dimtsz_raw > 1e-9 {
        let t = ArrowKind::Tick {
            size: (dimtsz_raw as f32).max(0.001),
        };
        (t.clone(), t)
    } else if let Some(s) = style {
        if matches!(dim, Dimension::Radius(_)) {
            let handle = if s.dimblk1.is_null() { s.dimblk } else { s.dimblk1 };
            let arrow = arrow_from_block_with_deferred_hatch(
                document,
                handle,
                dimasz,
                defer_arrow_hatches,
            );
            (arrow.clone(), arrow)
        } else if dimsah {
            (
                arrow_from_block_with_deferred_hatch(
                    document,
                    s.dimblk1,
                    dimasz,
                    defer_arrow_hatches,
                ),
                arrow_from_block_with_deferred_hatch(
                    document,
                    s.dimblk2,
                    dimasz,
                    defer_arrow_hatches,
                ),
            )
        } else {
            let a = arrow_from_block_with_deferred_hatch(
                document,
                s.dimblk,
                dimasz,
                defer_arrow_hatches,
            );
            (a.clone(), a)
        }
    } else {
        let a = ArrowKind::Triangle {
            size: dimasz,
            filled: true,
            size_mul: 1.0,
        };
        (a.clone(), a)
    };

    // Text box (local space) so the dim line can be broken where the text
    // crosses it — lets a DIMTFILL background sit over the line. The renderer
    // draws 2D fills under all wires, so the line is gapped rather than masked.
    let dimgap_local = style
        .map(|s| (s.dimgap.abs() * dim_scale) as f32)
        .unwrap_or(0.09);
    let text_width = dimension_text_value(dim, style)
        .map(|text| text.chars().count() as f32 * dim_txt as f32 * 0.6 + dimgap_local * 2.0)
        .unwrap_or(0.0);
    let text_break = {
        let tp = vec3_local(dimension_text_pos_f64(dim, style, dim_txt, dim_scale));
        let tw = (text_width - dimgap_local * 2.0).max(0.0);
        if tw > 0.0 {
            // Vertical threshold is the bare text half-height (no DIMGAP): the
            // line only breaks when it actually passes under the glyphs. Text
            // placed above/below the line (DIMTAD 1/4) sits exactly
            // `text_half + DIMGAP` away, so excluding the gap here keeps it
            // strictly outside and the line continuous — otherwise the two
            // terms cancel at the same scaled value and the gap flickers with
            // DIMGAP/DIMSCALE. The horizontal half-width keeps DIMGAP so a
            // genuine break still clears the text comfortably. (#94)
            Some((tp, tw * 0.5 + dimgap_local, dim_txt as f32 * 0.5))
        } else {
            None
        }
    };
    let text_position = vec3_local(dimension_text_pos_f64(dim, style, dim_txt, dim_scale));
    let text_is_outside = dimension_text_is_outside(dim, style);
    let horizontal_text = style.is_some_and(|style| {
        (text_is_outside && style.dimtoh) || (!text_is_outside && style.dimtih)
    });

    let mut geom = dimension_geometry(
        dim,
        &arrow1,
        &arrow2,
        DimLineParams {
            dimexo,
            dimexe,
            dimdle,
            dimfxl,
            dimfxlon,
            dimsoxd,
            dimcen,
            ticks: dimtsz_raw > 1e-9,
            arrow_len: dimasz,
            text_width,
            dimatfit: style.map(|s| s.dimatfit).unwrap_or(3),
            dimtofl: style.map(|s| s.dimtofl).unwrap_or(false),
            text_position,
            horizontal_text,
            text_break,
        },
        SuppressFlags {
            ext1: dimse1,
            ext2: dimse2,
            dim1: dimsd1,
            dim2: dimsd2,
        },
    );

    if !dimse1 {
        if let Some(points) = crate::scene::dimension_assoc::radial_extension_points(
            document,
            handle,
            dimexo as f64,
            dimexe as f64,
        ) {
            let points: Vec<Vec3> = points.into_iter().map(vec3_local).collect();
            add_polyline(&mut geom.ext_lines, &points);
        }
    }

    // DIMTMOVE = 1: when the saved text_middle_point sits far from the
    // dim-line anchor, draw a short leader connecting them. (=0 anchors text
    // to the dim line — no leader; =2 frees text without a leader.)
    if let Some(s) = style {
        if s.dimtmove == 1 {
            if let Some((anchor, txt)) = dimtmove_leader_endpoints(dim) {
                let gap = dim_txt as f32 * 0.5;
                if (txt - anchor).length() > gap * 2.0 {
                    add_segment(&mut geom.dim_lines, anchor, txt);
                }
            }
        }
        // DIMUPT governs interactive creation-time text placement; saved
        // geometry already carries the resulting position.
        let _ = s.dimupt;
        // DIMARCSYM only applies to arc-length dims; DIMJOGANG only to
        // jogged-radius dims. We don't ship those Dimension variants yet,
        // so the values are read for round-trip but not drawn.
        let _ = (s.dimarcsym, s.dimjogang);
        // DIMUNIT is the obsolete pre-R2000 linear unit format; DIMLUNIT
        // supersedes it. Read but not honoured.
        let _ = s.dimunit;
    }
    apply_dimension_breaks(document, handle, &mut geom.dim_lines);
    // Dimension entity fields that the render path doesn't yet use but are
    // preserved on save:
    //   - base.insertion_point: legacy anchor reference; render uses
    //     text_middle_point + dim-line geometry instead.
    //   - base.block_name: generated anonymous block name for
    //     the dim graphics — we re-tessellate so don't need it.
    //   - base.version: DXF format marker (metadata only).
    let _ = (
        dim.base().insertion_point,
        &dim.base().block_name,
        dim.base().version,
    );

    // Per-spec colours: DIMCLRD (dim/arrows), DIMCLRE (ext), DIMCLRT (text).
    // 0=ByBlock and 256=ByLayer fall through to entity_color. DIMCLRD also
    // honours a per-object ACAD_DSTYLE override (code 176) so an edited
    // dim-line colour renders even without touching the style.
    let dim_color = if selected {
        WireModel::SELECTED
    } else {
        let dim_clr = crate::entities::dim_override::int(
            &dim.base().common.extended_data,
            crate::entities::dim_override::DIMCLRD,
        )
        .unwrap_or_else(|| style.map(|s| s.dimclrd).unwrap_or(0));
        resolve_dim_color(dim_clr, entity_color)
    };
    let ext_color = if selected {
        WireModel::SELECTED
    } else {
        resolve_dim_color(style.map(|s| s.dimclre).unwrap_or(0), entity_color)
    };
    let text_color = if selected {
        entity_color // text wire color set by inner tessellate; keep entity tint
    } else {
        resolve_dim_color(style.map(|s| s.dimclrt).unwrap_or(0), entity_color)
    };

    let snap_pts = dimension_snap_pts(dim);
    let key_vertices: Vec<[f64; 3]> = geom
        .dim_lines
        .iter()
        .chain(geom.ext_lines.iter())
        .copied()
        .filter(|p| !(p[0].is_nan() || p[1].is_nan() || p[2].is_nan()))
        .map(|[x, y, z]| [x as f64, y as f64, z as f64])
        .collect();

    // DIMLWD (dim line + arrows) and DIMLWE (extension lines). Negative
    // codes fall through to the entity's own resolved weight.
    let lw_dim = resolve_dim_lineweight_px(style.map(|s| s.dimlwd).unwrap_or(-2), line_weight_px);
    let lw_ext = resolve_dim_lineweight_px(style.map(|s| s.dimlwe).unwrap_or(-2), line_weight_px);

    // DIMLTEX (dim line) / DIMLTEX1 (ext1) / DIMLTEX2 (ext2) — linetype
    // handles → pattern. Looked up in document.line_types by handle.
    let lt_scale = document.header.linetype_scale as f32 * dim.base().common.linetype_scale as f32;
    let (dim_pat_len, dim_pat) = style
        .map(|s| resolve_pattern_by_handle(document, s.dimltex_handle, lt_scale))
        .unwrap_or((0.0, [0.0; 8]));
    let (ext1_pat_len, ext1_pat) = style
        .map(|s| resolve_pattern_by_handle(document, s.dimltex1_handle, lt_scale))
        .unwrap_or((0.0, [0.0; 8]));
    let (ext2_pat_len, ext2_pat) = style
        .map(|s| resolve_pattern_by_handle(document, s.dimltex2_handle, lt_scale))
        .unwrap_or((0.0, [0.0; 8]));

    let mut wires = Vec::new();

    if !geom.ext_lines.is_empty() {
        // If ext1 and ext2 have different linetypes, split into two wires so
        // each can carry its own pattern. Otherwise emit as a single wire.
        let split = ext1_pat_len != ext2_pat_len || ext1_pat != ext2_pat;
        if split {
            let (ext1, ext2) = split_ext_lines(&geom.ext_lines);
            if !ext1.is_empty() {
                wires.push(WireModel {
                    point_marker: None,
                    taper_widths: Vec::new(),
                    pattern_stations: Vec::new(),
                    world_width: 0.0,
                    depth_override: None,
                    display_visible: true,
                    plot_visible: true,
                    fill_is_3d: false,
                    fill_is_2d_solid: false,
                    render_instance: None,
                    pick_tris: Vec::new(),
                    pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
                    name: name.clone(),
                    points: ext1,
                    points_low: Vec::new(),
                    color: ext_color,
                    selected,
                    aci: 0,
                    pattern_length: ext1_pat_len,
                    pattern: ext1_pat,
                    line_weight_px: lw_ext,
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    fill_tris: vec![],
                    fill_tris_low: Vec::new(),
                });
            }
            if !ext2.is_empty() {
                wires.push(WireModel {
                    point_marker: None,
                    taper_widths: Vec::new(),
                    pattern_stations: Vec::new(),
                    world_width: 0.0,
                    depth_override: None,
                    display_visible: true,
                    plot_visible: true,
                    fill_is_3d: false,
                    fill_is_2d_solid: false,
                    render_instance: None,
                    pick_tris: Vec::new(),
                    pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
                    name: name.clone(),
                    points: ext2,
                    points_low: Vec::new(),
                    color: ext_color,
                    selected,
                    aci: 0,
                    pattern_length: ext2_pat_len,
                    pattern: ext2_pat,
                    line_weight_px: lw_ext,
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    fill_tris: vec![],
                    fill_tris_low: Vec::new(),
                });
            }
        } else {
            wires.push(WireModel {
                point_marker: None,
                taper_widths: Vec::new(),
                pattern_stations: Vec::new(),
                world_width: 0.0,
                depth_override: None,
                display_visible: true,
                plot_visible: true,
                fill_is_3d: false,
                fill_is_2d_solid: false,
                render_instance: None,
                pick_tris: Vec::new(),
                pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
                name: name.clone(),
                points: geom.ext_lines,
                points_low: Vec::new(),
                color: ext_color,
                selected,
                aci: 0,
                pattern_length: ext1_pat_len,
                pattern: ext1_pat,
                line_weight_px: lw_ext,
                snap_pts: vec![],
                tangent_geoms: vec![],
                key_vertices: vec![],
                aabb: WireModel::UNBOUNDED_AABB,
                plinegen: true,
                fill_tris: vec![],
                fill_tris_low: Vec::new(),
            });
        }
    }

    wires.push(WireModel {
        point_marker: None,
        taper_widths: Vec::new(),
        pattern_stations: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        plot_visible: true,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        render_instance: None,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
        name: name.clone(),
        points: geom.dim_lines,
        points_low: Vec::new(),
        color: dim_color,
        selected,
        aci: 0,
        pattern_length: dim_pat_len,
        pattern: dim_pat,
        line_weight_px: lw_dim,
        snap_pts,
        tangent_geoms: vec![],
        key_vertices,
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: geom.arrow_fill,
        // fill_tris_low intentionally empty: this fill renders on the top-level
        // path, where consumers (face3d_gpu, xclip) treat a short low half as
        // all-zero, so it draws at f32 precision (sub-metre error at UTM scale)
        // — not a crash. Follow-up: double-single-split via points_to_ds to
        // match emit_wire's paired fill path.
        fill_tris_low: Vec::new(),
    });

    // DIMTFILL: 0=none, 1=drawing background (mask), 2=DIMTFILLCLR.
    if let Some(s) = style {
        if s.dimtfill == 1 || s.dimtfill == 2 {
            if let Some(rect) = text_fill_rect(dim, style, dim_txt, dim_scale) {
                let fill_color = if selected {
                    WireModel::SELECTED
                } else if s.dimtfill == 1 {
                    // Drawing-background fill: mask out geometry behind the text.
                    bg_color
                } else {
                    let c = AcadColor::from_index(s.dimtfillclr);
                    aci_to_rgba(&c)
                };
                wires.push(WireModel {
                    point_marker: None,
                    taper_widths: Vec::new(),
                    pattern_stations: Vec::new(),
                    world_width: 0.0,
                    depth_override: None,
                    display_visible: true,
                    plot_visible: true,
                    fill_is_3d: false,
                    fill_is_2d_solid: false,
                    render_instance: None,
                    pick_tris: Vec::new(),
                    pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
                    name: name.clone(),
                    points: vec![],
                    points_low: Vec::new(),
                    color: fill_color,
                    selected,
                    aci: 0,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px: 1.0,
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    fill_tris: rect,
                    // fill_tris_low intentionally empty: this fill renders on the
                    // top-level path, where consumers (face3d_gpu, xclip) treat a
                    // short low half as all-zero, so it draws at f32 precision
                    // (sub-metre error at UTM scale) — not a crash. Follow-up:
                    // double-single-split via points_to_ds to match emit_wire.
                    fill_tris_low: Vec::new(),
                });
            }
        }
    }

    // A negative DIMGAP denotes the Basic tolerance display: frame the
    // dimension text while keeping the absolute gap as the frame margin.
    if style.is_some_and(|s| s.dimgap < 0.0) {
        if let Some(rect) = text_fill_rect(dim, style, dim_txt, dim_scale) {
            let p1 = rect[0];
            let p2 = rect[1];
            let p3 = rect[2];
            let p4 = rect[5];
            wires.push(WireModel {
                point_marker: None,
                taper_widths: Vec::new(),
                pattern_stations: Vec::new(),
                world_width: 0.0,
                depth_override: None,
                display_visible: true,
                plot_visible: true,
                fill_is_3d: false,
                fill_is_2d_solid: false,
                render_instance: None,
                pick_tris: Vec::new(),
                pick_tris_low: Vec::new(),
                dash_from_start: false,
                dash_align_end: None,
                text_verts: Vec::new(),
                name: name.clone(),
                points: vec![p1, p2, p2, p3, p3, p4, p4, p1],
                points_low: Vec::new(),
                color: if selected { WireModel::SELECTED } else { text_color },
                selected,
                aci: 0,
                pattern_length: 0.0,
                pattern: [0.0; 8],
                line_weight_px: 1.0,
                snap_pts: vec![],
                tangent_geoms: vec![],
                key_vertices: vec![],
                aabb: WireModel::UNBOUNDED_AABB,
                plinegen: true,
                fill_tris: vec![],
                fill_tris_low: Vec::new(),
            });
        }
    }

    if let Some(synth_text_entity) = dimension_text_entity(dim, dim_txt, style, document, dim_scale)
    {
        // Tolerance Text rendered separately so DIMTFAC scales its height
        // and DIMTOLJ aligns it vertically against the primary text.
        let tol_entity = dimension_tolerance_entity(dim, style, &synth_text_entity, dim_txt);
        // Route synthesised dim text through tessellate_entity so the
        // baseline/greek/full LOD ladder applies (zoom-out behaviour
        // matches top-level Text / MText). The text already has dim_scale
        // baked into its height, so anno_scale stays 1.0.
        let text_wires = crate::scene::tessellate_entity_dim_text(
            document,
            selected_set,
            active_viewport,
            bg_color,
            1.0,
            &synth_text_entity,
            view_aabb,
            world_per_pixel,
            text_color,
        );
        for mut w in text_wires {
            w.name = name.clone();
            wires.push(w);
        }

        if let Some(tol_entity_e) = tol_entity {
            let tol_wires = crate::scene::tessellate_entity_dim_text(
                document,
                selected_set,
                active_viewport,
                bg_color,
                1.0,
                &tol_entity_e,
                view_aabb,
                world_per_pixel,
                text_color,
            );
            for mut w in tol_wires {
                w.name = name.clone();
                wires.push(w);
            }
        }
    }

    wires
}
fn resolve_dim_color(idx: i16, fallback: [f32; 4]) -> [f32; 4] {
    // DIMCLR* convention: 0 = BYBLOCK, 256 = BYLAYER → entity colour wins.
    if idx == 0 || idx == 256 {
        return fallback;
    }
    aci_to_rgba(&AcadColor::from_index(idx))
}

/// Resolve a DIMLWD / DIMLWE table value (the i16 lineweight code) into a
/// pixel width. -1 (ByLayer) / -2 (ByBlock) / -3 (Default) fall through to
/// the entity's already-resolved width.
fn resolve_dim_lineweight_px(code: i16, fallback_px: f32) -> f32 {
    const MM_TO_PX: f32 = 96.0 / 25.4;
    if code < 0 {
        return fallback_px;
    }
    // i16 value 0..=211 represents 1/100 mm.
    let mm = code as f32 / 100.0;
    (mm * MM_TO_PX).max(1.0)
}

/// Look up a linetype in the document's line_types table by handle and
/// resolve it to a (pattern_length, pattern) pair compatible with WireModel.
fn resolve_pattern_by_handle(
    doc: &CadDocument,
    handle: acadrust::types::Handle,
    scale: f32,
) -> (f32, [f32; 8]) {
    if handle.is_null() {
        return (0.0, [0.0; 8]);
    }
    let name = doc
        .line_types
        .iter()
        .find(|lt| lt.handle == handle)
        .map(|lt| lt.name.clone());
    match name {
        Some(n) => crate::scene::view::render::resolve_pattern(&doc.line_types, &n, scale),
        None => (0.0, [0.0; 8]),
    }
}

/// Split the combined ext-lines point list (NaN-separated segment pairs)
/// into "first" / "second" halves. `append_linear_dimension` writes ext1
/// before ext2, so the first segment is ext1 and the second is ext2.
fn split_ext_lines(points: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut groups: Vec<Vec<[f32; 3]>> = Vec::new();
    let mut current: Vec<[f32; 3]> = Vec::new();
    for &p in points {
        if p[0].is_nan() {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else {
            current.push(p);
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    let mut iter = groups.into_iter();
    let first = iter.next().unwrap_or_default();
    let rest: Vec<[f32; 3]> = iter.flatten().collect();
    (first, rest)
}

/// Endpoints for the DIMTMOVE=1 leader: (anchor on the dim line, saved
/// text_middle_point). Returns None when the dim has no saved text position
/// or has no well-defined dim-line midpoint (radius/diameter handled by
/// their own leg).
fn dimtmove_leader_endpoints(dim: &Dimension) -> Option<(Vec3, Vec3)> {
    let base = dim.base();
    let txt = base.text_middle_point;
    if txt.x * txt.x + txt.y * txt.y + txt.z * txt.z <= 1e-16 {
        return None;
    }
    let lv = |v| vec3_local(v);
    let anchor = match dim {
        Dimension::Linear(d) => {
            let perp = Vec3::new(-(d.rotation.sin() as f32), d.rotation.cos() as f32, 0.0);
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let def = lv(d.definition_point);
            let off1 = def.dot(perp) - first.dot(perp);
            let off2 = def.dot(perp) - second.dot(perp);
            (first + perp * off1 + second + perp * off2) * 0.5
        }
        Dimension::Aligned(d) => {
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let axis = normalized_or(second - first, Vec3::X);
            let perp = Vec3::new(-axis.y, axis.x, 0.0);
            let def = lv(d.definition_point);
            let off1 = def.dot(perp) - first.dot(perp);
            let off2 = def.dot(perp) - second.dot(perp);
            (first + perp * off1 + second + perp * off2) * 0.5
        }
        Dimension::Radius(_) => return None,
        Dimension::Diameter(d) => (lv(d.angle_vertex) + lv(d.definition_point)) * 0.5,
        Dimension::Angular2Ln(d) => lv(d.dimension_arc),
        Dimension::Angular3Pt(d) => lv(d.definition_point),
        _ => return None,
    };
    Some((anchor, lv(txt)))
}

/// Build a rectangle of filled triangles sitting under the dim text, used
/// when DIMTFILL = 2 (explicit fill colour). The rect width is estimated
/// from the formatted text length × character-cell width; an absolutely
/// correct box would need full text metrics from the font cache.
fn text_fill_rect(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
) -> Option<Vec<[f32; 3]>> {
    let value = dimension_text_value(dim, style)?;
    if value.is_empty() {
        return None;
    }
    let pos = dimension_text_pos_f64(dim, style, text_height, dim_scale);
    let dimgap = style.map(|s| s.dimgap.abs()).unwrap_or(0.0) * dim_scale;
    // ~0.6 × text_height per character; matches average glyph aspect for
    // the bundled stick fonts. Inflate by 1 DIMGAP on each side.
    let approx_w = value.chars().count() as f64 * text_height * 0.6 + dimgap * 2.0;
    let approx_h = text_height + dimgap * 2.0;
    let rot = if dim.base().text_rotation.abs() > 1e-9 {
        dim.base().text_rotation
    } else {
        dimension_text_natural_rotation(dim)
    };
    let (sr, cr) = rot.sin_cos();
    let hx = approx_w * 0.5;
    let hy = approx_h * 0.5;
    let cx = (pos.x) as f32;
    let cy = (pos.y) as f32;
    let cz = (pos.z) as f32;
    let corner = |dx: f64, dy: f64| -> [f32; 3] {
        let lx = dx * cr - dy * sr;
        let ly = dx * sr + dy * cr;
        [cx + lx as f32, cy + ly as f32, cz]
    };
    let p1 = corner(-hx, -hy);
    let p2 = corner(hx, -hy);
    let p3 = corner(hx, hy);
    let p4 = corner(-hx, hy);
    Some(vec![p1, p2, p3, p1, p3, p4])
}
struct SuppressFlags {
    ext1: bool,
    ext2: bool,
    dim1: bool,
    dim2: bool,
}

#[derive(Clone, Copy)]
struct DimLineParams {
    dimexo: f32,
    dimexe: f32,
    dimdle: f32,
    dimfxl: f32,
    dimfxlon: bool,
    dimsoxd: bool,
    dimcen: f32,
    ticks: bool,
    /// Arrowhead length (DIMASZ, scaled) — used to decide arrow-outside fit.
    arrow_len: f32,
    text_width: f32,
    dimatfit: i16,
    dimtofl: bool,
    text_position: Vec3,
    horizontal_text: bool,
    /// Text box (local centre, half-width, half-height) used to break the
    /// dimension line where the text sits on it, so a DIMTFILL background reads
    /// over the line. None when the text doesn't overlap the line.
    text_break: Option<(Vec3, f32, f32)>,
}
fn dimension_geometry(
    dim: &Dimension,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
    params: DimLineParams,
    suppress: SuppressFlags,
) -> DimGeom {
    let lv = |v| vec3_local(v);
    let mut g = DimGeom::new();
    match dim {
        Dimension::Aligned(d) => {
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let def = lv(d.definition_point);
            let axis = normalized_or(second - first, Vec3::X);
            append_linear_dimension(
                &mut g,
                first,
                second,
                def,
                axis,
                arrow1,
                arrow2,
                params,
                suppress,
                d.ext_line_rotation as f32,
            );
        }
        Dimension::Linear(d) => {
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let def = lv(d.definition_point);
            let axis = Vec3::new(d.rotation.cos() as f32, d.rotation.sin() as f32, 0.0);
            append_linear_dimension(
                &mut g,
                first,
                second,
                def,
                normalized_or(axis, Vec3::X),
                arrow1,
                arrow2,
                params,
                suppress,
                d.ext_line_rotation as f32,
            );
        }
        Dimension::Radius(d) => {
            let center = lv(d.angle_vertex);
            let point = lv(d.definition_point);
            let text = params.text_position;
            // Jogged radius dimensions use a shortened zig-zag leader.
            let jogged = dim
                .base()
                .common
                .extended_data
                .get_record("OCS_JOGGED")
                .is_some();
            if jogged {
                let delta = point - center;
                let dist = delta.length();
                let u = normalized_or(delta, Vec3::X);
                let perp = Vec3::new(-u.y, u.x, 0.0);
                let half = (dist * 0.06).max(1e-3);
                let mid = center + u * (dist * 0.5);
                let a = mid - u * half + perp * half;
                let b = mid + u * half - perp * half;
                if !suppress.dim1 {
                    add_segment(&mut g.dim_lines, center, a);
                    add_segment(&mut g.dim_lines, a, b);
                    add_segment(&mut g.dim_lines, b, point);
                }
            } else if !suppress.dim1 {
                add_segment(&mut g.dim_lines, center, point);
            }
            let radius = (point - center).length();
            let text_is_outside = text.distance(center) > radius + 1e-5;
            if text_is_outside && !suppress.dim1 {
                let radial = normalized_or(point - center, Vec3::X);
                let angle_from_horizontal = radial.y.abs().atan2(radial.x.abs());
                if params.horizontal_text
                    && angle_from_horizontal > 15.0_f32.to_radians()
                    && radial.y.abs() > 1e-6
                {
                    let travel = (text.y - point.y) / radial.y;
                    if travel > 0.0 {
                        let elbow = point + radial * travel;
                        let landing_gap = params.text_width * 0.5 + params.arrow_len;
                        let landing = Vec3::new(
                            text.x - radial.x.signum() * landing_gap,
                            text.y,
                            text.z,
                        );
                        add_segment(&mut g.dim_lines, point, elbow);
                        add_segment(&mut g.dim_lines, elbow, landing);
                    } else {
                        add_segment(&mut g.dim_lines, point, text);
                    }
                } else {
                    add_segment(&mut g.dim_lines, point, text);
                }
            }
            append_arrow(
                &mut g,
                point,
                normalized_or(center - point, Vec3::X),
                arrow1,
            );
            if text_is_outside {
                append_center_mark(&mut g, center, params.dimcen, radius);
            }
        }
        Dimension::Diameter(d) => {
            // angle_vertex is the circle centre and definition_point a point on
            // the circle. The diameter line runs edge-to-edge THROUGH the centre
            // (far edge → near edge), with arrows pointing inward at each edge.
            let center = lv(d.angle_vertex);
            let edge = lv(d.definition_point);
            let far = center * 2.0 - edge;
            add_segment(&mut g.dim_lines, far, edge);
            append_arrow(&mut g, edge, normalized_or(far - edge, Vec3::X), arrow1);
            append_arrow(&mut g, far, normalized_or(edge - far, Vec3::X), arrow2);
            // Diameter leader: continue past the near edge toward the text.
            if d.leader_length.abs() > 1e-9 {
                let text = dimension_text_position(dim);
                let leader_dir = normalized_or(text - edge, edge - far);
                add_segment(
                    &mut g.dim_lines,
                    edge,
                    edge + leader_dir * (d.leader_length as f32),
                );
            }
            let radius = (edge - center).length();
            append_center_mark(&mut g, center, params.dimcen, radius);
        }
        Dimension::Angular2Ln(d) => {
            // A two-line angular dimension stores two LINES, not two rays:
            // `first_point`→`second_point` is one, `angle_vertex`→
            // `definition_point` the other, and the angle is between them at
            // their intersection. Reading `angle_vertex` as the centre and the
            // other two as rays measures something else entirely — an angle of
            // ten degrees came out as two hundred and seventy.
            let (p1, p2) = (lv(d.first_point), lv(d.second_point));
            let (p3, p4) = (lv(d.angle_vertex), lv(d.definition_point));
            let arc_point = lv(d.dimension_arc);
            match two_line_angle_frame(p1, p2, p3, p4, arc_point) {
                Some((vertex, start, end)) => append_angular_dimension(
                    &mut g,
                    vertex,
                    vertex,
                    vertex,
                    arc_point,
                    arrow1,
                    arrow2,
                    Some((start, end)),
                    params,
                    suppress,
                ),
                // Parallel lines have no vertex and so no angle to draw; the
                // extension lines alone say where the dimension was.
                None => {
                    add_segment(&mut g.ext_lines, p1, p2);
                    add_segment(&mut g.ext_lines, p3, p4);
                }
            }
        }
        Dimension::Angular3Pt(d) => {
            let vertex = lv(d.angle_vertex);
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let arc_point = lv(d.definition_point);
            let explicit_sweep = two_line_angle_frame(
                vertex,
                first,
                vertex,
                second,
                arc_point,
            )
            .map(|(_, start, end)| (start, end));
            append_angular_dimension(
                &mut g,
                vertex,
                first,
                second,
                arc_point,
                arrow1,
                arrow2,
                explicit_sweep,
                params,
                suppress,
            );
        }
        Dimension::Ordinate(d) => {
            add_segment(
                &mut g.dim_lines,
                lv(d.feature_location),
                lv(d.definition_point),
            );
            add_segment(
                &mut g.dim_lines,
                lv(d.definition_point),
                lv(d.leader_endpoint),
            );
        }
        Dimension::Arc(d) => {
            append_angular_dimension(
                &mut g,
                lv(d.center_point),
                lv(d.first_extension_point),
                lv(d.second_extension_point),
                lv(d.definition_point),
                arrow1,
                arrow2,
                d.is_partial.then_some((
                    d.arc_start_parameter as f32,
                    d.arc_end_parameter as f32,
                )),
                params,
                suppress,
            );
            if d.has_leader {
                add_segment(
                    &mut g.dim_lines,
                    lv(d.first_leader_point),
                    lv(d.second_leader_point),
                );
            }
        }
        Dimension::LargeRadial(d) => {
            let chord = lv(d.chord_point);
            let jog = lv(d.jog_point);
            let override_center = lv(d.override_center);
            let (near, far) =
                jogged_radial_break(chord, jog, override_center, d.jog_angle as f32);
            add_segment(&mut g.dim_lines, chord, near);
            add_segment(&mut g.dim_lines, near, far);
            add_segment(&mut g.dim_lines, far, override_center);
            append_arrow(
                &mut g,
                chord,
                normalized_or(near - chord, Vec3::X),
                arrow1,
            );
        }
    }
    g
}

fn jogged_radial_break(
    chord: Vec3,
    jog: Vec3,
    override_center: Vec3,
    jog_angle: f32,
) -> (Vec3, Vec3) {
    let radial = normalized_or(chord - override_center, Vec3::X);
    let (sin, cos) = jog_angle.sin_cos();
    let transverse = normalized_or(
        Vec3::new(
            radial.x * cos - radial.y * sin,
            radial.x * sin + radial.y * cos,
            0.0,
        ),
        Vec3::Y,
    );
    let half = ((chord - override_center).length() * 0.04).max(1e-3);
    let first = jog - transverse * half;
    let second = jog + transverse * half;
    if chord.distance_squared(first) <= chord.distance_squared(second) {
        (first, second)
    } else {
        (second, first)
    }
}

fn append_linear_dimension(
    g: &mut DimGeom,
    first: Vec3,
    second: Vec3,
    def: Vec3,
    axis: Vec3,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
    params: DimLineParams,
    suppress: SuppressFlags,
    ext_line_rotation: f32,
) {
    let perp = Vec3::new(-axis.y, axis.x, 0.0);
    let dim_line_pos = def.dot(perp);
    let offset1 = dim_line_pos - first.dot(perp);
    let offset2 = dim_line_pos - second.dot(perp);
    let d1 = first + perp * offset1;
    let d2 = second + perp * offset2;
    let sign1 = if offset1 >= 0.0 { 1.0_f32 } else { -1.0 };
    let sign2 = if offset2 >= 0.0 { 1.0_f32 } else { -1.0 };

    // ext_line_rotation (DIMEDIT "Oblique"): rotate the extension lines by
    // this angle relative to perpendicular. The ext line still starts at
    // the def point; only the direction differs.
    let ext_dir = if ext_line_rotation.abs() > 1e-6 {
        let c = ext_line_rotation.cos();
        let s = ext_line_rotation.sin();
        // Rotate `perp` by ext_line_rotation around Z.
        Vec3::new(perp.x * c - perp.y * s, perp.x * s + perp.y * c, 0.0)
    } else {
        perp
    };

    // DIMFXLON / DIMFXL: fixed extension-line length from the dim line back
    // toward (but not past) the definition point. Otherwise grow from the
    // def point with DIMEXO gap, extending DIMEXE past the dim line.
    // When oblique, lengths are measured along ext_dir instead of perp.
    let (ext1_start, ext1_end, ext2_start, ext2_end) = if params.dimfxlon {
        let fxl = params.dimfxl.max(0.0);
        let s1 = d1 - ext_dir * (sign1 * fxl);
        let e1 = d1 + ext_dir * (sign1 * params.dimexe);
        let s2 = d2 - ext_dir * (sign2 * fxl);
        let e2 = d2 + ext_dir * (sign2 * params.dimexe);
        (s1, e1, s2, e2)
    } else {
        (
            first + ext_dir * (sign1 * params.dimexo),
            d1 + ext_dir * (sign1 * params.dimexe),
            second + ext_dir * (sign2 * params.dimexo),
            d2 + ext_dir * (sign2 * params.dimexe),
        )
    };
    if !suppress.ext1 {
        add_segment(&mut g.ext_lines, ext1_start, ext1_end);
    }
    if !suppress.ext2 {
        add_segment(&mut g.ext_lines, ext2_start, ext2_end);
    }

    // DIMDLE: dim line overshoots the ext line by `dimdle` at each end,
    // but only when ticks are in use (DIMTSZ > 0). With arrowheads this
    // is ignored.
    let dle = if params.ticks { params.dimdle } else { 0.0 };
    let dir_d1_to_d2 = normalized_or(d2 - d1, axis);
    let d1_out = d1 - dir_d1_to_d2 * dle;
    let d2_out = d2 + dir_d1_to_d2 * dle;

    // When text plus arrows do not fit, DIMATFIT decides which component moves
    // first: 0=both, 1=arrows, 2=text, 3=best fit.
    let gap = (d2 - d1).length();
    let arrows_outside = if params.ticks || params.arrow_len <= 1e-6 {
        false
    } else if gap < 2.0 * params.arrow_len {
        true
    } else if gap < params.text_width + 2.0 * params.arrow_len {
        match params.dimatfit {
            0 | 1 => true,
            2 => false,
            _ => params.text_width <= gap,
        }
    } else {
        false
    };

    // The two suppression flags address the portions at the first and second
    // measured points independently. The text gap is also the natural split;
    // when there is no gap, split at the projected text position or midpoint.
    let line_dir = normalized_or(d2_out - d1_out, axis);
    let line_len = (d2_out - d1_out).length();
    let mut split = line_len * 0.5;
    let mut left_end = split;
    let mut right_start = split;
    if let Some((text_center, half_width, half_height)) = params.text_break {
        let along = (text_center - d1_out).dot(line_dir);
        if along > 0.0 && along < line_len {
            split = along;
            left_end = split;
            right_start = split;
            let perpendicular = (text_center - (d1_out + line_dir * along)).length();
            if perpendicular < half_height
                && along - half_width > 0.0
                && along + half_width < line_len
            {
                left_end = along - half_width;
                right_start = along + half_width;
            }
        }
    }
    let draw_inside_line = !arrows_outside || params.dimtofl;
    if draw_inside_line && !suppress.dim1 && left_end > 1e-6 {
        add_segment(
            &mut g.dim_lines,
            d1_out,
            d1_out + line_dir * left_end,
        );
    }
    if draw_inside_line && !suppress.dim2 && line_len - right_start > 1e-6 {
        add_segment(
            &mut g.dim_lines,
            d1_out + line_dir * right_start,
            d2_out,
        );
    }
    if arrows_outside && !params.dimsoxd {
        let stub = params.arrow_len * 2.0;
        if !suppress.dim1 {
            add_segment(&mut g.dim_lines, d1 - dir_d1_to_d2 * stub, d1);
        }
        if !suppress.dim2 {
            add_segment(&mut g.dim_lines, d2, d2 + dir_d1_to_d2 * stub);
        }
    }

    if arrows_outside {
        // Tip on the ext line, body pointing outward.
        append_arrow(g, d1, normalized_or(d1 - d2, -axis), arrow1);
        append_arrow(g, d2, normalized_or(d2 - d1, axis), arrow2);
    } else {
        append_arrow(g, d1, normalized_or(d2 - d1, axis), arrow1);
        append_arrow(g, d2, normalized_or(d1 - d2, -axis), arrow2);
    }
}

/// Draw a center mark for radius/diameter dimensions.
///   DIMCEN > 0 → small "+" of half-length |DIMCEN| at the centre.
///   DIMCEN < 0 → small "+" *plus* four line segments extending from the
///                circle (radius - |DIMCEN|) outward to (radius + |DIMCEN|).
///   DIMCEN = 0 → no mark.
fn append_center_mark(g: &mut DimGeom, center: Vec3, dimcen: f32, radius: f32) {
    let mag = dimcen.abs();
    if mag <= 1e-6 {
        return;
    }
    // Small "+" at the centre.
    let h = mag;
    add_segment(
        &mut g.dim_lines,
        Vec3::new(center.x - h, center.y, center.z),
        Vec3::new(center.x + h, center.y, center.z),
    );
    add_segment(
        &mut g.dim_lines,
        Vec3::new(center.x, center.y - h, center.z),
        Vec3::new(center.x, center.y + h, center.z),
    );
    if dimcen < 0.0 && radius > mag + 1e-6 {
        let inner = (radius - mag).max(0.0);
        let outer = radius + mag;
        // Four short radial strokes spanning the circle edge.
        add_segment(
            &mut g.dim_lines,
            Vec3::new(center.x + inner, center.y, center.z),
            Vec3::new(center.x + outer, center.y, center.z),
        );
        add_segment(
            &mut g.dim_lines,
            Vec3::new(center.x - inner, center.y, center.z),
            Vec3::new(center.x - outer, center.y, center.z),
        );
        add_segment(
            &mut g.dim_lines,
            Vec3::new(center.x, center.y + inner, center.z),
            Vec3::new(center.x, center.y + outer, center.z),
        );
        add_segment(
            &mut g.dim_lines,
            Vec3::new(center.x, center.y - inner, center.z),
            Vec3::new(center.x, center.y - outer, center.z),
        );
    }
}

/// Vertex and sweep of the angle between two lines, as a two-line angular
/// dimension stores them: `a1`→`a2` and `b1`→`b2`, with `arc_point` sitting on
/// the arc that shows which of the four angles at the crossing is meant.
///
/// Returns `None` for parallel lines, which cross nowhere and enclose nothing.
fn two_line_angle_frame(
    a1: Vec3,
    a2: Vec3,
    b1: Vec3,
    b2: Vec3,
    arc_point: Vec3,
) -> Option<(Vec3, f32, f32)> {
    let (u, v) = (a2 - a1, b2 - b1);
    let (t, _) = cadkernel::geom2d::line_line(
        [a1.x as f64, a1.y as f64],
        [u.x as f64, u.y as f64],
        [b1.x as f64, b1.y as f64],
        [v.x as f64, v.y as f64],
    )?;
    let vertex = a1 + u * t as f32;

    // Two lines cross at four angles; the arc point picks one. Each line
    // contributes its direction and its reverse, so try the four pairs and keep
    // the one whose sweep both contains the arc point and is the shorter way
    // round — the dimension marks an angle, never its reflex twin.
    let angle_of = |d: Vec3| d.y.atan2(d.x);
    let target = angle_of(arc_point - vertex);
    let mut best: Option<(f32, f32, f32)> = None;
    for su in [1.0f32, -1.0] {
        for sv in [1.0f32, -1.0] {
            let (from_u, from_v) = (angle_of(u * su), angle_of(v * sv));
            // Either line can be the one the sweep starts from; taking only
            // u→v leaves out half the angles at the crossing, and the half left
            // out is the one wanted whenever the lines are nearly parallel.
            for (start, end) in [(from_u, from_v), (from_v, from_u)] {
                let sweep = (end - start).rem_euclid(std::f32::consts::TAU);
                if sweep <= 1e-6 || sweep > std::f32::consts::PI {
                    continue;
                }
                let into_target = (target - start).rem_euclid(std::f32::consts::TAU);
                if into_target > sweep {
                    continue;
                }
                if best.is_none_or(|(_, _, known)| sweep < known) {
                    best = Some((start, start + sweep, sweep));
                }
            }
        }
    }
    let (start, end, _) = best?;
    Some((vertex, start, end))
}

fn angular_dimension_frame(dim: &Dimension) -> Option<(Vec3, f32, f32, f32)> {
    let (vertex, start, end, arc_point) = match dim {
        Dimension::Angular2Ln(value) => {
            let first_start = vec3_local(value.first_point);
            let first_end = vec3_local(value.second_point);
            let second_start = vec3_local(value.angle_vertex);
            let second_end = vec3_local(value.definition_point);
            let arc_point = vec3_local(value.dimension_arc);
            let (vertex, start, end) = two_line_angle_frame(
                first_start,
                first_end,
                second_start,
                second_end,
                arc_point,
            )?;
            (vertex, start, end, arc_point)
        }
        Dimension::Angular3Pt(value) => {
            let vertex = vec3_local(value.angle_vertex);
            let first = vec3_local(value.first_point);
            let second = vec3_local(value.second_point);
            let arc_point = vec3_local(value.definition_point);
            let (vertex, start, end) =
                two_line_angle_frame(vertex, first, vertex, second, arc_point)?;
            (vertex, start, end, arc_point)
        }
        _ => return None,
    };
    let radius = vertex.distance(arc_point);
    (radius > 1.0e-6).then_some((vertex, start, end, radius))
}

fn append_angular_dimension(
    g: &mut DimGeom,
    vertex: Vec3,
    first: Vec3,
    second: Vec3,
    arc_point: Vec3,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
    explicit_sweep: Option<(f32, f32)>,
    params: DimLineParams,
    suppress: SuppressFlags,
) {
    let radius = vertex.distance(arc_point);
    if radius <= 1e-6 {
        add_segment(&mut g.ext_lines, vertex, first);
        add_segment(&mut g.ext_lines, vertex, second);
        return;
    }
    // Extension lines run from each measured point out to the dimension arc so
    // there is no gap between a ray and its arc endpoint. (#181 / DIM-027)
    let measured_start = (first.y - vertex.y).atan2(first.x - vertex.x);
    let measured_end = (second.y - vertex.y).atan2(second.x - vertex.x);
    let (start, mut end) = explicit_sweep.unwrap_or((measured_start, measured_end));
    let dir1 = Vec3::new(start.cos(), start.sin(), 0.0);
    let dir2 = Vec3::new(end.cos(), end.sin(), 0.0);
    let arc_start = vertex + dir1 * radius;
    let arc_end = vertex + dir2 * radius;
    let extension = |origin: Vec3, endpoint: Vec3, direction: Vec3| {
        if params.dimfxlon {
            (
                endpoint - direction * params.dimfxl.max(0.0),
                endpoint + direction * params.dimexe,
            )
        } else {
            let toward_arc = normalized_or(endpoint - origin, direction);
            (
                origin + toward_arc * params.dimexo,
                endpoint + toward_arc * params.dimexe,
            )
        }
    };
    let (ext1_start, ext1_end) = extension(first, arc_start, dir1);
    let (ext2_start, ext2_end) = extension(second, arc_end, dir2);
    if !suppress.ext1 {
        add_segment(&mut g.ext_lines, ext1_start, ext1_end);
    }
    if !suppress.ext2 {
        add_segment(&mut g.ext_lines, ext2_start, ext2_end);
    }

    let mut delta = end - start;
    // Wrap a negative sweep forwards, but leave a zero one alone: two rays that
    // point the same way enclose no angle, and turning that into a full turn
    // drew a whole circle where there was nothing to draw.
    while delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    if delta.abs() <= 1e-6 {
        return;
    }
    if explicit_sweep.is_none() && delta > std::f32::consts::PI {
        end -= std::f32::consts::TAU;
        delta = end - start;
    }

    let arc_length = radius * delta.abs();
    let arrows_outside = if params.ticks || params.arrow_len <= 1.0e-6 {
        false
    } else if arc_length < params.arrow_len * 2.0 {
        true
    } else if arc_length < params.text_width + params.arrow_len * 2.0 {
        match params.dimatfit {
            0 | 1 => true,
            2 => false,
            _ => params.text_width <= arc_length,
        }
    } else {
        false
    };
    let draw_inside_line = !arrows_outside || params.dimtofl;
    let arc_extension = if params.ticks { params.dimdle / radius } else { 0.0 };
    let direction = delta.signum();
    let draw_start = start - direction * arc_extension;
    let draw_delta = delta + direction * arc_extension * 2.0;
    let arc_pts = sample_angular_arc(vertex, radius, draw_start, draw_delta);
    let steps = arc_pts.len().saturating_sub(1);
    if draw_inside_line {
        for index in 0..steps {
            let t = (index as f32 + 0.5) / steps as f32;
            if (t < 0.5 && suppress.dim1) || (t >= 0.5 && suppress.dim2) {
                continue;
            }
            let a = arc_pts[index];
            let b = arc_pts[index + 1];
            let hidden_by_text = params.text_break.is_some_and(|(center, half_width, half_height)| {
                let angle = draw_start + draw_delta * t;
                let radial = Vec3::new(angle.cos(), angle.sin(), 0.0);
                let tangent = Vec3::new(-angle.sin(), angle.cos(), 0.0);
                let offset = center - (a + b) * 0.5;
                offset.dot(tangent).abs() <= half_width
                    && offset.dot(radial).abs() <= half_height
            });
            if !hidden_by_text {
                add_segment(&mut g.dim_lines, a, b);
            }
        }
    }

    if arrows_outside && !params.dimsoxd {
        let stub_angle = (params.arrow_len * 2.0 / radius).min(std::f32::consts::FRAC_PI_2);
        if !suppress.dim1 {
            append_sampled_arc(
                &mut g.dim_lines,
                vertex,
                radius,
                start - direction * stub_angle,
                start,
            );
        }
        if !suppress.dim2 {
            append_sampled_arc(
                &mut g.dim_lines,
                vertex,
                radius,
                end,
                end + direction * stub_angle,
            );
        }
    }

    if let Some((text_center, half_width, _)) = params.text_break {
        let text_angle = (text_center.y - vertex.y).atan2(text_center.x - vertex.x);
        let into = (text_angle - start).rem_euclid(std::f32::consts::TAU);
        if into > delta.abs() + 1.0e-6 {
            let back_to_start = (start - text_angle).rem_euclid(std::f32::consts::TAU);
            let forward_from_end = (text_angle - end).rem_euclid(std::f32::consts::TAU);
            let text_gap = (half_width / radius).min(std::f32::consts::FRAC_PI_2);
            if back_to_start <= forward_from_end && !suppress.dim1 {
                append_sampled_arc(
                    &mut g.dim_lines,
                    vertex,
                    radius,
                    text_angle + text_gap,
                    start,
                );
            } else if !suppress.dim2 {
                append_sampled_arc(
                    &mut g.dim_lines,
                    vertex,
                    radius,
                    end,
                    text_angle - text_gap,
                );
            }
        }
    }

    let start_tangent = Vec3::new(-start.sin(), start.cos(), 0.0) * direction;
    let end_tangent = Vec3::new(-end.sin(), end.cos(), 0.0) * direction;
    let draw_arrows = !arrows_outside || !params.dimsoxd;
    if draw_arrows && !suppress.dim1 {
        append_arrow(
            g,
            arc_start,
            if arrows_outside { -start_tangent } else { start_tangent },
            arrow1,
        );
    }
    if draw_arrows && !suppress.dim2 {
        append_arrow(
            g,
            arc_end,
            if arrows_outside { end_tangent } else { -end_tangent },
            arrow2,
        );
    }
}

fn append_sampled_arc(
    lines: &mut Vec<[f32; 3]>,
    vertex: Vec3,
    radius: f32,
    start: f32,
    end: f32,
) {
    for pair in sample_angular_arc(vertex, radius, start, end - start).windows(2) {
        add_segment(lines, pair[0], pair[1]);
    }
}

fn sample_angular_arc(vertex: Vec3, radius: f32, start: f32, sweep: f32) -> Vec<Vec3> {
    if sweep.abs() <= 1.0e-6 {
        return vec![vertex + Vec3::new(start.cos() * radius, start.sin() * radius, 0.0)];
    }
    let end = start + sweep;
    let (from, to, reverse) = if sweep >= 0.0 {
        (start, end, false)
    } else {
        (end, start, true)
    };
    let mut points: Vec<_> = cadkernel::geom2d::tessellate::arc(
        [vertex.x as f64, vertex.y as f64],
        radius as f64,
        from as f64,
        to as f64,
        vertex.z as f64,
        cadkernel::geom2d::tessellate::DEFAULT_SEGMENTS_PER_RADIAN,
    )
    .into_iter()
    .map(|point| Vec3::new(point[0] as f32, point[1] as f32, point[2] as f32))
    .collect();
    if reverse {
        points.reverse();
    }
    points
}

fn dimension_snap_pts(dim: &Dimension) -> Vec<(glam::DVec3, SnapHint)> {
    let lv = |v: acadrust::types::Vector3| glam::DVec3::new(v.x, v.y, v.z);
    let node = |v: acadrust::types::Vector3| (lv(v), SnapHint::Node);
    match dim {
        Dimension::Linear(d) => vec![
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Aligned(d) => vec![
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Radius(d) => vec![node(d.angle_vertex), node(d.definition_point)],
        Dimension::Diameter(d) => vec![node(d.angle_vertex), node(d.definition_point)],
        Dimension::Angular2Ln(d) => vec![
            node(d.first_point),
            node(d.second_point),
            node(d.angle_vertex),
            node(d.definition_point),
            node(d.dimension_arc),
        ],
        Dimension::Angular3Pt(d) => vec![
            node(d.angle_vertex),
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Ordinate(d) => vec![
            node(d.definition_point),
            node(d.feature_location),
            node(d.leader_endpoint),
        ],
        Dimension::Arc(d) => {
            let mut points = vec![
                node(d.center_point),
                node(d.first_extension_point),
                node(d.second_extension_point),
                node(d.definition_point),
            ];
            if d.has_leader {
                points.push(node(d.first_leader_point));
                points.push(node(d.second_leader_point));
            }
            points
        }
        Dimension::LargeRadial(d) => vec![
            node(d.definition_point),
            node(d.chord_point),
            node(d.override_center),
            node(d.jog_point),
        ],
    }
}

/// Cheap heuristic: does this string contain anything the MText parser would
/// interpret? Used by `dimension_text_entity` to pick between a synthetic
/// `Text` (plain DXF special chars only) and a synthetic `MText` (full inline
/// format-code pipeline) for the dim text override.
fn value_has_mtext_codes(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' || c == '}' {
            return true;
        }
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                // Any backslash followed by a known MText escape letter.
                if matches!(
                    next,
                    'H' | 'W'
                        | 'Q'
                        | 'T'
                        | 'A'
                        | 'C'
                        | 'c'
                        | 'f'
                        | 'F'
                        | 'p'
                        | 'L'
                        | 'l'
                        | 'O'
                        | 'o'
                        | 'K'
                        | 'k'
                        | 'S'
                        | 's'
                        | 'P'
                        | 'n'
                        | 'N'
                        | 't'
                        | 'U'
                        | 'u'
                        | 'M'
                        | 'X'
                        | '~'
                        | '{'
                        | '}'
                ) {
                    return true;
                }
            }
        }
    }
    false
}

fn dimension_text_entity(
    dim: &Dimension,
    text_height: f64,
    style: Option<&DimStyle>,
    document: &CadDocument,
    dim_scale: f64,
) -> Option<EntityType> {
    // Tolerances are emitted by `dimension_tolerance_entity` at their own
    // height and alignment; keep the primary entity free of duplicate text.
    let (value, _) = dimension_text_parts(dim, style)?;
    // Use f64 position directly to avoid f32 round-trip precision loss at large
    // coordinates (e.g. Turkish UTM ~4,000,000 m). tessellate() will apply
    // world_offset when rendering this synthetic entity.
    let pos_f64 = dimension_text_pos_f64(dim, style, text_height, dim_scale);
    let base = dim.base();

    let rotation = dimension_text_rotation(dim, style);

    // Text style resolution priority:
    //   1. DIMTXSTY by handle (most reliable; survives rename)
    //   2. DIMTXSTY by name
    //   3. dim's own style_name (rare fallback)
    let style_name = style
        .and_then(|s| {
            if !s.dimtxsty_handle.is_null() {
                document
                    .text_styles
                    .iter()
                    .find(|ts| ts.handle == s.dimtxsty_handle)
                    .map(|ts| ts.name.clone())
            } else {
                None
            }
        })
        .or_else(|| {
            style
                .map(|s| s.dimtxsty.clone())
                .filter(|n| !n.trim().is_empty())
        })
        .unwrap_or_else(|| base.style_name.clone());

    // Route through MText whenever the value carries inline format codes
    // (`\f`, `\C`, `\H`, `\S`, brace scopes, …). Otherwise stay on the Text
    // path — single-line dim text doesn't need the full MText pipeline.
    if value_has_mtext_codes(&value) {
        use acadrust::entities::dimension::AttachmentPointType as DA;
        use acadrust::entities::AttachmentPoint as MA;
        let attachment_point = match base.attachment_point {
            DA::TopLeft => MA::TopLeft,
            DA::TopCenter => MA::TopCenter,
            DA::TopRight => MA::TopRight,
            DA::MiddleLeft => MA::MiddleLeft,
            DA::MiddleCenter => MA::MiddleCenter,
            DA::MiddleRight => MA::MiddleRight,
            DA::BottomLeft => MA::BottomLeft,
            DA::BottomCenter => MA::BottomCenter,
            DA::BottomRight => MA::BottomRight,
        };
        let mut mtext = MText::with_value(value, pos_f64);
        mtext.height = text_height;
        mtext.rotation = rotation;
        mtext.style = style_name;
        mtext.attachment_point = attachment_point;
        if base.line_spacing_factor.abs() > 1e-9 {
            mtext.line_spacing_factor = base.line_spacing_factor;
        }
        mtext.normal = base.normal;
        mtext.common = base.common.clone();
        return Some(EntityType::MText(mtext));
    }

    let value = if style.is_some_and(|style| style.dimtxtdirection) {
        value.chars().rev().collect()
    } else {
        value
    };
    let mut text = Text::with_value(value, pos_f64)
        .with_height(text_height)
        .with_rotation(rotation);
    text.style = style_name;

    // Map AttachmentPointType (1..9 grid) to Text horizontal + vertical
    // alignments. 1=TopLeft … 9=BottomRight (column-major).
    let (ha, va) = attachment_to_text_align(base.attachment_point);
    text.horizontal_alignment = ha;
    text.vertical_alignment = va;
    // line_spacing_factor controls multi-line text spacing in MText. Our
    // synthetic Text is single-line so this is a no-op, but pass through
    // for completeness.
    let _ = base.line_spacing_factor;
    // normal would rotate the dim plane out of XY. The local 2D pipeline
    // assumes XY, so non-XY normals are read but not applied here.
    let _ = base.normal;

    text.common = base.common.clone();
    Some(EntityType::Text(text))
}

fn attachment_to_text_align(
    attach: acadrust::entities::dimension::AttachmentPointType,
) -> (
    acadrust::entities::text::TextHorizontalAlignment,
    acadrust::entities::text::TextVerticalAlignment,
) {
    use acadrust::entities::dimension::AttachmentPointType as A;
    use acadrust::entities::text::{TextHorizontalAlignment as H, TextVerticalAlignment as V};
    match attach {
        A::TopLeft => (H::Left, V::Top),
        A::TopCenter => (H::Center, V::Top),
        A::TopRight => (H::Right, V::Top),
        A::MiddleLeft => (H::Left, V::Middle),
        A::MiddleCenter => (H::Center, V::Middle),
        A::MiddleRight => (H::Right, V::Middle),
        A::BottomLeft => (H::Left, V::Bottom),
        A::BottomCenter => (H::Center, V::Bottom),
        A::BottomRight => (H::Right, V::Bottom),
    }
}

/// Reading rotation for a dimension's measurement text, resolving the style
/// flags the same way the live renderer does: explicit text_rotation, then
/// horizontal_direction, then DIMTIH/DIMTOH force-horizontal, else the natural
/// dim-line angle (+90° for DIMJUST 3/4). Shared by the live text entity and
/// the bake so a reloaded dimension's text reads at the same angle. (#181)
fn dimension_text_rotation(dim: &Dimension, style: Option<&DimStyle>) -> f64 {
    let base = dim.base();
    let dimtih = style.map(|s| s.dimtih).unwrap_or(false);
    let dimtoh = style.map(|s| s.dimtoh).unwrap_or(false);
    let dimjust = style.map(|s| s.dimjust).unwrap_or(0);
    let outside = dimension_text_is_outside(dim, style);
    if base.text_rotation.abs() > 1e-9 {
        base.text_rotation
    } else if base.horizontal_direction.abs() > 1e-9 {
        base.horizontal_direction
    } else if (outside && dimtoh) || (!outside && dimtih) {
        0.0
    } else {
        let mut r = dimension_text_natural_rotation(dim);
        if dimjust == 3 || dimjust == 4 {
            r += std::f64::consts::FRAC_PI_2;
        }
        r
    }
}

fn dimension_text_is_outside(dim: &Dimension, style: Option<&DimStyle>) -> bool {
    let Some(style) = style else {
        return false;
    };
    if let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) {
        if dim.base().text_user_positioned {
            let text = vec3_local(dim.base().text_middle_point);
            let angle = (text.y - vertex.y).atan2(text.x - vertex.x);
            return (angle - start).rem_euclid(std::f32::consts::TAU)
                > end - start + 1.0e-6;
        }
        if style.dimtix {
            return false;
        }
        let scale = if style.dimscale > 1e-9 { style.dimscale } else { 1.0 };
        let height = style.dimtxt * scale;
        let gap = style.dimgap.abs() * scale;
        let text_width = dimension_text_value(dim, Some(style))
            .map(|value| value.chars().count() as f64 * height * 0.6 + gap * 2.0)
            .unwrap_or(0.0);
        let arrow = style.dimasz * scale;
        let span = radius as f64 * (end - start).abs() as f64;
        let insufficient = text_width + arrow * 2.0 > span;
        return insufficient
            && match style.dimatfit {
                0 | 2 => true,
                1 | 3 => text_width > span,
                _ => text_width > span,
            };
    }
    if let Dimension::Radius(radius) = dim {
        let dx = radius.definition_point.x - radius.angle_vertex.x;
        let dy = radius.definition_point.y - radius.angle_vertex.y;
        let available = dx.hypot(dy);
        if dim.base().text_user_positioned {
            let text = dim.base().text_middle_point;
            return (text.x - radius.angle_vertex.x)
                .hypot(text.y - radius.angle_vertex.y)
                > available + 1e-9;
        }
        if style.dimtix {
            return false;
        }
        let scale = if style.dimscale > 1e-9 { style.dimscale } else { 1.0 };
        let height = style.dimtxt * scale;
        let gap = style.dimgap.abs() * scale;
        let text_width = dimension_text_value(dim, Some(style))
            .map(|value| value.chars().count() as f64 * height * 0.6 + gap * 2.0)
            .unwrap_or(0.0);
        let arrow = style.dimasz * scale;
        let insufficient = text_width + arrow > available;
        return insufficient
            && match style.dimatfit {
                0 | 2 => true,
                1 | 3 => text_width > available,
                _ => text_width > available,
            };
    }
    let (first, second, axis) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            Vector3::new(d.rotation.cos(), d.rotation.sin(), 0.0),
        ),
        Dimension::Aligned(d) => {
            let delta = d.second_point - d.first_point;
            let length = (delta.x * delta.x + delta.y * delta.y).sqrt().max(1e-12);
            (d.first_point, d.second_point, delta / length)
        }
        _ => return false,
    };
    let first_axis = first.x * axis.x + first.y * axis.y;
    let second_axis = second.x * axis.x + second.y * axis.y;
    let lo = first_axis.min(second_axis);
    let hi = first_axis.max(second_axis);
    if dim.base().text_user_positioned {
        let point = dim.base().text_middle_point;
        let position = point.x * axis.x + point.y * axis.y;
        return position < lo || position > hi;
    }
    if style.dimtix {
        return false;
    }
    let scale = if style.dimscale > 1e-9 { style.dimscale } else { 1.0 };
    let height = style.dimtxt * scale;
    let gap = style.dimgap.abs() * scale;
    let text_width = dimension_text_value(dim, Some(style))
        .map(|value| value.chars().count() as f64 * height * 0.6 + gap * 2.0)
        .unwrap_or(0.0);
    let arrow = style.dimasz * scale;
    let span = hi - lo;
    let insufficient = text_width + arrow * 2.0 > span;
    insufficient
        && match style.dimatfit {
            0 | 2 => true,
            1 | 3 => text_width > span,
            _ => text_width > span,
        }
}

fn dimension_text_natural_rotation(dim: &Dimension) -> f64 {
    let angle = match dim {
        Dimension::Linear(d) => d.rotation,
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            dy.atan2(dx)
        }
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => angular_dimension_frame(dim)
            .map(|(_, start, end, _)| {
                ((start + end) * 0.5 + std::f32::consts::FRAC_PI_2) as f64
            })
            .unwrap_or(0.0),
        Dimension::Radius(d) => (d.definition_point.y - d.angle_vertex.y)
            .atan2(d.definition_point.x - d.angle_vertex.x),
        _ => 0.0,
    };
    // Clamp to (-π/2, π/2] so text never appears upside-down.
    let pi = std::f64::consts::PI;
    if angle > pi / 2.0 {
        angle - pi
    } else if angle <= -pi / 2.0 {
        angle + pi
    } else {
        angle
    }
}

pub(crate) fn dimension_text_value(dim: &Dimension, style: Option<&DimStyle>) -> Option<String> {
    let (main, tol) = dimension_text_parts(dim, style)?;
    // Tolerance is appended inline for callers (e.g. fill rect width) that
    // don't render a separate tolerance entity. The visual pipeline that
    // does emit a separate tolerance text re-derives the parts itself.
    match tol {
        Some(t) => Some(format!("{} {}", main, t)),
        None => Some(main),
    }
}

/// Returns (primary_text, tolerance_suffix). The tolerance is emitted as a
/// separate Text entity so DIMTFAC can scale its height and DIMTOLJ can
/// align it vertically against the primary value.
fn dimension_text_parts(
    dim: &Dimension,
    style: Option<&DimStyle>,
) -> Option<(String, Option<String>)> {
    let base = dim.base();
    let is_angular = matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_));

    // Auto-generated body used when the user did not override it. Built first
    // so user_text "<>" substitution can re-use it.
    let primary_raw = if is_angular {
        format_angular_value(dim.measurement(), style)
    } else {
        let v = format_linear_value(dim.measurement(), style);
        match dim {
            Dimension::Radius(_) | Dimension::LargeRadial(_) => format!("R{}", v),
            Dimension::Diameter(_) => format!("Ø{}", v),
            _ => v,
        }
    };

    // Build tolerance / limits suffix separately so the caller can render
    // it as its own Text entity at DIMTFAC × DIMTXT height.
    let tolerance_suffix = build_tolerance_suffix(dim, style, is_angular);
    let primary = apply_dimpost(&primary_raw, style);

    // Alternate units appended in brackets when DIMALT is on (linear only).
    let primary = if !is_angular {
        match alternate_units_text(dim.measurement(), style) {
            Some(alt) => format!("{} [{}]", primary, alt),
            None => primary,
        }
    } else {
        primary
    };

    // Explicit user override (mtext-style "user_text") wins, but "<>" inside
    // it substitutes the measured value. " " (single space) suppresses text.
    if let Some(user_text) = &base.user_text {
        if user_text.is_empty() || user_text.trim().is_empty() {
            return None;
        }
        return Some((user_text.replace("<>", &primary), tolerance_suffix));
    }
    if !base.text.trim().is_empty() {
        return Some((base.text.replace("<>", &primary), tolerance_suffix));
    }
    Some((primary, tolerance_suffix))
}

fn build_tolerance_suffix(
    dim: &Dimension,
    style: Option<&DimStyle>,
    is_angular: bool,
) -> Option<String> {
    let s = style?;
    let measurement = dim.measurement();
    let dimtdec = s.dimtdec.max(0) as usize;
    let dimtzin = s.dimtzin;
    let fmt = |v: f64| -> String {
        let raw = format!("{:.*}", dimtdec, v);
        swap_decimal_sep(&apply_linear_zero_suppression(&raw, dimtzin), s.dimdsep)
    };
    if s.dimlim {
        let high = measurement + s.dimtp;
        let low = measurement - s.dimtm;
        return Some(format!("\\S{}^{};", fmt(high), fmt(low)));
    }
    if s.dimtol {
        let unit = if is_angular { "°" } else { "" };
        if (s.dimtp - s.dimtm).abs() < 1e-12 && s.dimtp.abs() > 1e-12 {
            return Some(format!("±{}{}", fmt(s.dimtp), unit));
        }
        if s.dimtp.abs() > 1e-12 || s.dimtm.abs() > 1e-12 {
            let mut upper = fmt(s.dimtp);
            let mut lower = fmt(s.dimtm);
            let alignment = crate::entities::dim_override::int(
                &dim.base().common.extended_data,
                crate::entities::dim_override::DIMTALN,
            )
            .unwrap_or(0);
            if alignment == 0 {
                align_tolerance_decimals(&mut upper, &mut lower, s.dimdsep);
            }
            return Some(format!(
                "\\S+{}{}^-{}{};",
                upper, unit, lower, unit
            ));
        }
    }
    None
}

fn align_tolerance_decimals(upper: &mut String, lower: &mut String, separator: i16) {
    let separator = separator as u8 as char;
    let integer_width = |value: &str| {
        value
            .find(separator)
            .or_else(|| value.find('.'))
            .unwrap_or(value.len())
    };
    let upper_width = integer_width(upper);
    let lower_width = integer_width(lower);
    let target = upper_width.max(lower_width);
    if upper_width < target {
        upper.insert_str(0, &" ".repeat(target - upper_width));
    }
    if lower_width < target {
        lower.insert_str(0, &" ".repeat(target - lower_width));
    }
}

/// Build the bracketed alternate-units suffix when DIMALT is enabled.
/// When DIMTOL is also on, the bracketed text includes the tolerance
/// component formatted with DIMALTTD / DIMALTTZ.
fn alternate_units_text(measurement: f64, style: Option<&DimStyle>) -> Option<String> {
    let s = style?;
    if !s.dimalt {
        return None;
    }
    let scaled = measurement * s.dimaltf;
    let use_sub_units = s.dimaltz & 4 != 0
        && scaled.abs() < 1.0
        && s.dimaltmzf.abs() > 1e-12;
    // DIMALTMZF replaces the ordinary alternate-unit factor for sub-unit
    // values; it is not an additional multiplier.
    let mut v = if use_sub_units {
        measurement * s.dimaltmzf
    } else {
        scaled
    };
    if s.dimaltrnd > 1e-12 {
        v = (v / s.dimaltrnd).round() * s.dimaltrnd;
    }
    let dec = s.dimaltd.max(0) as usize;
    let raw = format_with_unit(v, s.dimaltu, dec, s.dimfrac, s.dimaltz, true);
    let suppressed = apply_linear_zero_suppression(&raw, s.dimaltz);
    let mut sep_swapped = swap_decimal_sep(&suppressed, s.dimdsep);
    if use_sub_units {
        sep_swapped.push_str(&s.dimaltmzs);
    }
    let tolerance_factor = if use_sub_units {
        s.dimaltmzf
    } else {
        s.dimaltf
    };
    // Alt-unit tolerance suffix using DIMALTTD / DIMALTTZ.
    let alt_value = if s.dimtol {
        let alttdec = s.dimalttd.max(0) as usize;
        let alttzin = s.dimalttz;
        let fmt = |x: f64| -> String {
            let raw = format!("{:.*}", alttdec, x * tolerance_factor);
            swap_decimal_sep(&apply_linear_zero_suppression(&raw, alttzin), s.dimdsep)
        };
        if (s.dimtp - s.dimtm).abs() < 1e-12 && s.dimtp.abs() > 1e-12 {
            format!("{}±{}", sep_swapped, fmt(s.dimtp))
        } else if s.dimtp.abs() > 1e-12 || s.dimtm.abs() > 1e-12 {
            format!("{} +{} / -{}", sep_swapped, fmt(s.dimtp), fmt(s.dimtm))
        } else {
            sep_swapped
        }
    } else if s.dimlim {
        let alttdec = s.dimalttd.max(0) as usize;
        let alttzin = s.dimalttz;
        let fmt = |x: f64| -> String {
            let raw = format!("{:.*}", alttdec, x * tolerance_factor);
            swap_decimal_sep(&apply_linear_zero_suppression(&raw, alttzin), s.dimdsep)
        };
        format!(
            "{}/{}",
            fmt(measurement + s.dimtp),
            fmt(measurement - s.dimtm)
        )
    } else {
        sep_swapped
    };
    // DIMAPOST wraps the alt value (same "<>" convention as DIMPOST).
    let wrapped = if s.dimapost.is_empty() {
        alt_value
    } else if s.dimapost.contains("<>") {
        s.dimapost.replace("<>", &alt_value)
    } else {
        format!("{}{}", alt_value, s.dimapost)
    };
    Some(wrapped)
}

/// Build the secondary tolerance Text entity at `DIMTXT × DIMTFAC` height,
/// positioned to the right of the primary text and vertically aligned per
/// `DIMTOLJ` (0=bottom, 1=middle, 2=top). Returns None when DIMTOL/DIMLIM
/// produce no tolerance string (e.g. both DIMTP and DIMTM are zero).
fn dimension_tolerance_entity(
    dim: &Dimension,
    style: Option<&DimStyle>,
    primary: &EntityType,
    primary_height: f64,
) -> Option<EntityType> {
    let s = style?;
    let is_angular = matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_));
    let tol = build_tolerance_suffix(dim, style, is_angular)?;
    let dimtfac = if s.dimtfac.abs() < 1e-12 {
        1.0
    } else {
        s.dimtfac
    };
    let tol_height = primary_height * dimtfac;

    // Pull the geometry we need from the synthetic primary entity (Text or
    // MText — `dimension_text_entity` routes to MText when the dim value
    // carries inline format codes).
    let (primary_value_len, primary_insertion, primary_rotation, primary_style, primary_common) =
        match primary {
            EntityType::Text(t) => (
                t.value.chars().count(),
                t.insertion_point,
                t.rotation,
                t.style.clone(),
                t.common.clone(),
            ),
            EntityType::MText(m) => (
                m.value.chars().count(),
                m.insertion_point,
                m.rotation,
                m.style.clone(),
                m.common.clone(),
            ),
            _ => return None,
        };

    // Approximate widths from glyph counts (~0.6 × cell size per char).
    let primary_w = primary_value_len as f64 * primary_height * 0.6;
    let tol_visible_chars = tol
        .strip_prefix("\\S")
        .and_then(|value| value.strip_suffix(';'))
        .map(|value| value.split('^').map(str::len).max().unwrap_or(0))
        .unwrap_or_else(|| tol.chars().count());
    let tol_w = tol_visible_chars as f64 * tol_height * 0.6;
    let gap = primary_height * 0.2;
    let dx_local = primary_w * 0.5 + tol_w * 0.5 + gap;
    let dy_local = match s.dimtolj {
        0 => -primary_height * 0.5 + tol_height * 0.5, // bottom-aligned with primary baseline
        2 => primary_height * 0.5 - tol_height * 0.5,  // top-aligned with primary top
        _ => 0.0,                                      // centred (default for ±)
    };
    let rot = primary_rotation;
    let (sr, cr) = rot.sin_cos();
    let pos = Vector3::new(
        primary_insertion.x + dx_local * cr - dy_local * sr,
        primary_insertion.y + dx_local * sr + dy_local * cr,
        primary_insertion.z,
    );
    if value_has_mtext_codes(&tol) {
        let mut mtext = MText::with_value(tol, pos);
        mtext.height = tol_height;
        mtext.rotation = rot;
        mtext.style = primary_style;
        mtext.attachment_point = acadrust::entities::AttachmentPoint::MiddleCenter;
        mtext.common = primary_common;
        return Some(EntityType::MText(mtext));
    }

    let mut t = Text::with_value(tol, pos)
        .with_height(tol_height)
        .with_rotation(rot);
    t.style = primary_style;
    t.common = primary_common;
    t.horizontal_alignment = acadrust::entities::text::TextHorizontalAlignment::Center;
    t.vertical_alignment = acadrust::entities::text::TextVerticalAlignment::Middle;
    Some(EntityType::Text(t))
}

/// Wrap a measured value with the style's DIMPOST prefix/suffix template.
/// "<>" inside DIMPOST is replaced by the value; absent "<>" appends.
fn apply_dimpost(value: &str, style: Option<&DimStyle>) -> String {
    let post = style.map(|s| s.dimpost.as_str()).unwrap_or("");
    if post.is_empty() {
        return value.to_string();
    }
    if post.contains("<>") {
        post.replace("<>", value)
    } else {
        format!("{}{}", value, post)
    }
}

/// Format a linear measurement honouring DIMLFAC, DIMRND, DIMDEC, DIMZIN, DIMDSEP, DIMLUNIT.
fn format_linear_value(measurement: f64, style: Option<&DimStyle>) -> String {
    let (dec, zin, lfac, rnd, dsep, lunit, frac, sub_factor, sub_suffix) = style
        .map(|s| {
            (
                s.dimdec,
                s.dimzin,
                s.dimlfac,
                s.dimrnd,
                s.dimdsep,
                s.dimlunit,
                s.dimfrac,
                s.dimmzf,
                s.dimmzs.as_str(),
            )
        })
        .unwrap_or((4, 8, 1.0, 0.0, 46, 2, 0, 1.0, ""));

    let lfac = if lfac.abs() < 1e-12 { 1.0 } else { lfac };
    let scaled = measurement * lfac;
    let use_sub_units = zin & 4 != 0 && scaled.abs() < 1.0 && sub_factor.abs() > 1e-12;
    // For values below one unit, DIMMZF replaces DIMLFAC; applying it after
    // DIMLFAC multiplies both factors and reports the wrong sub-unit value.
    let mut v = if use_sub_units {
        measurement * sub_factor
    } else {
        scaled
    };
    if rnd > 1e-12 {
        v = (v / rnd).round() * rnd;
    }
    let dec = dec.max(0) as usize;
    let raw = format_with_unit(v, lunit, dec, frac, zin, false);
    let suppressed = apply_linear_zero_suppression(&raw, zin);
    let mut formatted = swap_decimal_sep(&suppressed, dsep);
    if use_sub_units {
        formatted.push_str(sub_suffix);
    }
    formatted
}

/// Dispatch on DIMLUNIT / DIMALTU.
///   1 = Scientific
///   2 = Decimal (default)
///   3 = Engineering   (feet + decimal inches; 1 unit = 1 inch)
///   4 = Architectural (feet + fractional inches)
///   5 = Fractional    (integer + fractional inches)
/// For alternate units, 4/5 are stacked and 6/7 are unstacked architectural
/// and fractional forms. For primary units DIMFRAC selects the stack form.
fn format_with_unit(
    value: f64,
    unit: i16,
    dec: usize,
    dimfrac: i16,
    zin: i16,
    alternate: bool,
) -> String {
    match unit {
        1 => format!("{:.*e}", dec, value),
        3 => format_engineering(value, dec),
        4 => format_architectural(value, dec, if alternate { 0 } else { dimfrac }, zin),
        5 => format_fractional(value, dec, if alternate { 0 } else { dimfrac }),
        6 if alternate => format_architectural(value, dec, 2, zin),
        7 if alternate => format_fractional(value, dec, 2),
        _ => format!("{:.*}", dec, value),
    }
}

fn format_engineering(inches: f64, dec: usize) -> String {
    let sign = if inches < 0.0 { "-" } else { "" };
    let abs = inches.abs();
    let feet = (abs / 12.0).trunc();
    let rem_in = abs - feet * 12.0;
    format!("{}{:.0}'-{:.*}\"", sign, feet, dec, rem_in)
}

fn format_architectural(inches: f64, precision: usize, dimfrac: i16, zin: i16) -> String {
    let denom = fractional_denominator(precision);
    // Round to the nearest 1/denom inch *first*, in integer ticks, so a fraction
    // that rounds up to a whole inch carries into the inches — and on into the
    // feet — instead of printing an un-reduced "11 1/1". A 3ft object that
    // measures 35.9999" (float) now reads 3'-0", not 2'-11 1".
    let ticks = (inches.abs() * denom as f64).round() as u64;
    let sign = if inches < 0.0 && ticks != 0 { "-" } else { "" };
    let per_foot = 12 * denom;
    let feet = ticks / per_foot;
    let rem = ticks % per_foot;
    let whole = rem / denom;
    let frac_str = format_fraction_component(&reduce_fraction(rem % denom, denom), dimfrac);

    // DIMZIN feet/inch suppression:
    //   0 suppress zero feet & zero inches, 1 include both,
    //   2 include zero feet / suppress zero inches,
    //   3 suppress zero feet / include zero inches.
    let suppress_zero_feet = zin == 0 || zin == 3;
    let suppress_zero_inches = zin == 0 || zin == 2;
    let feet_zero = feet == 0;
    let inches_zero = whole == 0 && frac_str.is_empty();
    let show_feet = !feet_zero || !suppress_zero_feet;
    let show_inches = !inches_zero || !suppress_zero_inches;

    let feet_part = if show_feet {
        format!("{}'", feet)
    } else {
        String::new()
    };
    let inch_part = if show_inches {
        if frac_str.is_empty() {
            format!("{}\"", whole)
        } else {
            format!("{} {}\"", whole, frac_str)
        }
    } else {
        String::new()
    };
    let body = match (feet_part.is_empty(), inch_part.is_empty()) {
        (false, false) => format!("{}-{}", feet_part, inch_part),
        (false, true) => feet_part,
        (true, false) => inch_part,
        (true, true) => "0\"".to_string(),
    };
    format!("{}{}", sign, body)
}

fn format_fractional(value: f64, precision: usize, dimfrac: i16) -> String {
    let denom = fractional_denominator(precision);
    // Round on the fraction grid first (same carry reasoning as architectural).
    let ticks = (value.abs() * denom as f64).round() as u64;
    let sign = if value < 0.0 && ticks != 0 { "-" } else { "" };
    let whole = ticks / denom;
    let frac_str = format_fraction_component(&reduce_fraction(ticks % denom, denom), dimfrac);
    if frac_str.is_empty() {
        format!("{}{}", sign, whole)
    } else if whole == 0 {
        format!("{}{}", sign, frac_str)
    } else {
        format!("{}{} {}", sign, whole, frac_str)
    }
}

/// Fraction precision maps 0..8 to whole units through 1/256. DIMFRAC controls
/// only the visual stack form; it must not change the numeric denominator.
fn fractional_denominator(precision: usize) -> u64 {
    1u64 << precision.min(8)
}

fn format_fraction_component(value: &str, dimfrac: i16) -> String {
    if value.is_empty() || dimfrac == 2 {
        return value.to_string();
    }
    let separator = if dimfrac == 1 { '#' } else { '/' };
    format!("\\S{};", value.replacen('/', &separator.to_string(), 1))
}

/// Reduce a power-of-two fraction numer/denom to display form. Empty for a zero
/// numerator. Callers strip the whole units first, so `numer < denom` and the
/// reduced denominator is always ≥ 2 — the carry that used to leak out as a bare
/// "1/1" is handled upstream now.
fn reduce_fraction(mut n: u64, mut d: u64) -> String {
    if n == 0 {
        return String::new();
    }
    while n % 2 == 0 && d % 2 == 0 {
        n /= 2;
        d /= 2;
    }
    if d == 1 {
        format!("{}", n)
    } else {
        format!("{}/{}", n, d)
    }
}

/// Format an angular measurement in degrees using the angular style settings.
fn format_angular_value(measurement_deg: f64, style: Option<&DimStyle>) -> String {
    let (aunit, adec, azin, decimal_separator) = style
        .map(|s| {
            let precision = if s.dimadec < 0 { s.dimdec } else { s.dimadec };
            (s.dimaunit, precision, s.dimazin, s.dimdsep)
        })
        .unwrap_or((0, 2, 0, b'.' as i16));
    let adec = adec.clamp(0, 8) as usize;

    match aunit {
        // 1 = Degrees / Minutes / Seconds
        1 => format_dms(measurement_deg, adec, azin, decimal_separator),
        // 2 = Gradians
        2 => {
            let g = measurement_deg / 0.9;
            let raw = format!("{:.*}", adec, g);
            format!(
                "{}g",
                swap_decimal_sep(&apply_angular_zero_suppression(&raw, azin), decimal_separator)
            )
        }
        // 3 = Radians
        3 => {
            let r = measurement_deg.to_radians();
            let raw = format!("{:.*}", adec, r);
            format!(
                "{}r",
                swap_decimal_sep(&apply_angular_zero_suppression(&raw, azin), decimal_separator)
            )
        }
        // 0 or unknown = Decimal Degrees
        _ => {
            let raw = format!("{:.*}", adec, measurement_deg);
            format!(
                "{}°",
                swap_decimal_sep(&apply_angular_zero_suppression(&raw, azin), decimal_separator)
            )
        }
    }
}

fn format_dms(deg: f64, sec_dec: usize, azin: i16, decimal_separator: i16) -> String {
    let sign = if deg < 0.0 { "-" } else { "" };
    let scale = 10_u64.pow(sec_dec.min(8) as u32) as f64;
    let total_ticks = (deg.abs() * 3600.0 * scale).round();
    let d = (total_ticks / (3600.0 * scale)).floor();
    let remaining = total_ticks - d * 3600.0 * scale;
    let m = (remaining / (60.0 * scale)).floor();
    let s = (remaining - m * 60.0 * scale) / scale;
    let s_str = format!("{:.*}", sec_dec, s);
    let s_str = swap_decimal_sep(
        &apply_angular_zero_suppression(&s_str, azin),
        decimal_separator,
    );
    format!("{}{:.0}°{:.0}'{}\"", sign, d, m, s_str)
}

/// Apply DIMZIN bit flags to a formatted linear value.
///  bit 0 (1)  suppress 0' (imperial feet)        — not applicable for decimal
///  bit 1 (2)  suppress 0" (imperial inches)      — not applicable for decimal
///  bit 2 (4)  suppress leading zeros             (e.g. ".5" not "0.5")
///  bit 3 (8)  suppress trailing zeros            (e.g. "1.5" not "1.50")
/// Default = 8 (trailing-zero suppression on).
fn apply_linear_zero_suppression(s: &str, zin: i16) -> String {
    let mut out = s.to_string();
    if zin & 8 != 0 {
        out = strip_trailing_zeros(&out);
    }
    if zin & 4 != 0 {
        out = strip_leading_zero(&out);
    }
    out
}

fn apply_angular_zero_suppression(s: &str, azin: i16) -> String {
    // DIMAZIN: 0=neither, 1=leading, 2=trailing, 3=both.
    let mut out = s.to_string();
    if azin & 2 != 0 {
        out = strip_trailing_zeros(&out);
    }
    if azin & 1 != 0 {
        out = strip_leading_zero(&out);
    }
    out
}

fn strip_trailing_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn strip_leading_zero(s: &str) -> String {
    // "0.5" → ".5",  "-0.5" → "-.5",  "0" stays.
    if let Some(rest) = s.strip_prefix("-0.") {
        return format!("-.{rest}");
    }
    if let Some(rest) = s.strip_prefix("0.") {
        return format!(".{rest}");
    }
    s.to_string()
}

fn swap_decimal_sep(s: &str, dsep_code: i16) -> String {
    // DIMDSEP holds an ASCII code (0 means default '.'). 46='.', 44=',', etc.
    if dsep_code <= 0 || dsep_code == 46 {
        return s.to_string();
    }
    let ch = char::from_u32(dsep_code as u32).unwrap_or('.');
    s.replace('.', &ch.to_string())
}

fn dimension_text_position(dim: &Dimension) -> Vec3 {
    let lv = |v| vec3_local(v);
    let base = dim.base();
    let pos = lv(base.text_middle_point);
    if pos.length_squared() > 1e-8 {
        return pos;
    }
    match dim {
        Dimension::Aligned(d) => (lv(d.first_point) + lv(d.second_point)) * 0.5,
        Dimension::Linear(d) => (lv(d.first_point) + lv(d.second_point)) * 0.5,
        Dimension::Radius(d) => (lv(d.angle_vertex) + lv(d.definition_point)) * 0.5,
        Dimension::Diameter(d) => (lv(d.angle_vertex) + lv(d.definition_point)) * 0.5,
        Dimension::Angular2Ln(d) => lv(d.dimension_arc),
        Dimension::Angular3Pt(d) => lv(d.definition_point),
        Dimension::Ordinate(d) => lv(d.leader_endpoint),
        Dimension::Arc(d) => lv(d.definition_point),
        Dimension::LargeRadial(d) => lv(d.jog_point),
    }
}

fn vec3_local(v: Vector3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

/// Style-driven text anchor on the dimension line: a point on the line through
/// `defpt` parallel to (`ax`,`ay`), slid along it per DIMJUST and lifted by
/// `perp_off` toward the side the dimension line sits on.
#[allow(clippy::too_many_arguments)]
fn text_on_dim_line(
    first: Vector3,
    second: Vector3,
    defpt: Vector3,
    ax: f64,
    ay: f64,
    dimjust: i16,
    perp_off: f64,
    text_w: f64,
    arrow: f64,
    dimtix: bool,
    dimatfit: i16,
    tad: i16,
) -> Vector3 {
    // The perpendicular "up" side: the perpendicular of the dimension line
    // normalised so the text reads left-to-right (or bottom-to-top when
    // vertical). DIMTAD "above" (1) and JIS (3) sit on this side.
    let (mut nx, mut ny) = (ax, ay);
    if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
        nx = -nx;
        ny = -ny;
    }
    let px = -ny;
    let py = nx;
    // DIMTAD side:
    //   1 (above) / 3 (JIS) / 0 (centred) → the text-up side, independent of the
    //     object — keying it off the object flipped "above" to "below" whenever
    //     the dimension ran the opposite way (#144);
    //   4 (below)   → the opposite side;
    //   2 (outside) → the side farthest from the defining points (the object).
    let perp_sign = match tad {
        4 => -1.0,
        2 => {
            let off = (defpt.x - first.x) * px + (defpt.y - first.y) * py;
            if off >= 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        _ => 1.0,
    };
    // Along-axis positions of the extension points relative to the dim line.
    let t1 = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
    let t2 = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;
    // DIMJUST: 0=centred, 1/3=near first ext, 2/4=near second ext.
    let mut along = match dimjust {
        1 | 3 => t1,
        2 | 4 => t2,
        _ => (t1 + t2) * 0.5,
    };
    // DIMATFIT / DIMTIX fit: move text according to the selected priority when
    // the combined text-and-arrow envelope cannot fit.
    if dimjust == 0 && !dimtix && text_w > 0.0 {
        let lo = t1.min(t2);
        let hi = t1.max(t2);
        let span = hi - lo;
        let insufficient = text_w + 2.0 * arrow > span;
        let text_outside = insufficient
            && match dimatfit {
                0 | 2 => true,
                1 | 3 => text_w > span,
                _ => text_w > span,
            };
        if text_outside {
            along = hi + arrow + text_w * 0.5;
        }
    }
    let bx = defpt.x + ax * along;
    let by = defpt.y + ay * along;
    Vector3::new(
        bx + px * perp_off * perp_sign,
        by + py * perp_off * perp_sign,
        defpt.z,
    )
}

fn dimension_text_pos_f64(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
) -> Vector3 {
    let base = dim.base();

    // DIMTAD: 0=centred (on the line), 1=above, 4=below. 2 (outside, i.e. the
    // side farthest from the defining points) and 3 (JIS) both resolve to the
    // away-from-object side, which is the same as "above" for 2-D linear dims.
    let dimtad = style.map(|s| s.dimtad).unwrap_or(1);
    // DIMGAP scales with DIMSCALE just like DIMTXT, so the text-to-line gap
    // stays consistent when DIMSCALE != 1.
    let dimgap = style.map(|s| s.dimgap.abs()).unwrap_or(0.0) * dim_scale;
    let dimjust = style.map(|s| s.dimjust).unwrap_or(0);
    // DIMTIX forces the text to stay between the extension lines.
    let dimtix = style.map(|s| s.dimtix).unwrap_or(false);
    let dimatfit = style.map(|s| s.dimatfit).unwrap_or(3);
    // DIMTVP vertical-position multiplier (units of dimtxt). Only honoured when
    // DIMTAD == 0; offsets text perpendicular to the dim line.
    let dimtvp = style.map(|s| s.dimtvp).unwrap_or(0.0);
    let perp_off = if dimtad == 0 {
        dimtvp * text_height
    } else {
        text_height * 0.5 + dimgap
    };
    // Rough text width + arrow allowance, used to decide text-outside fit.
    let text_w = dimension_text_value(dim, style)
        .map(|t| t.chars().count() as f64 * text_height * 0.6 + 2.0 * dimgap)
        .unwrap_or(0.0);
    let arrow = text_height; // arrows are roughly text-height sized

    // Explicit per-entity override (text dragged to a custom location): the
    // saved point wins. Otherwise the dimension style governs placement.
    let use_saved = base.text_user_positioned && {
        let p = base.text_middle_point;
        p.x * p.x + p.y * p.y + p.z * p.z > 1e-16
    };
    if use_saved {
        return base.text_middle_point;
    }

    match dim {
        Dimension::Linear(d) => {
            let ax = d.rotation.cos();
            let ay = d.rotation.sin();
            text_on_dim_line(
                d.first_point,
                d.second_point,
                d.definition_point,
                ax,
                ay,
                dimjust,
                perp_off,
                text_w,
                arrow,
                dimtix,
                dimatfit,
                dimtad,
            )
        }
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt().max(1e-12);
            text_on_dim_line(
                d.first_point,
                d.second_point,
                d.definition_point,
                dx / len,
                dy / len,
                dimjust,
                perp_off,
                text_w,
                arrow,
                dimtix,
                dimatfit,
                dimtad,
            )
        }
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => {
            let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) else {
                return base.text_middle_point;
            };
            let span = radius as f64 * (end - start).abs() as f64;
            let insufficient = text_w + arrow * 2.0 > span;
            let move_outside = !dimtix
                && insufficient
                && match dimatfit {
                    0 | 2 => true,
                    1 | 3 => text_w > span,
                    _ => text_w > span,
                };
            let angle = if move_outside {
                end as f64 + (text_w * 0.5 + dimgap + arrow) / (radius as f64).max(1.0e-12)
            } else {
                ((start + end) * 0.5) as f64
            };
            let radial_offset = if dimtad == 4 { -perp_off } else { perp_off };
            let text_radius = (radius as f64 + radial_offset).max(0.0);
            Vector3::new(
                vertex.x as f64 + angle.cos() * text_radius,
                vertex.y as f64 + angle.sin() * text_radius,
                vertex.z as f64,
            )
        }
        Dimension::Radius(d) => {
            let dx = d.definition_point.x - d.angle_vertex.x;
            let dy = d.definition_point.y - d.angle_vertex.y;
            let radius = dx.hypot(dy).max(1e-12);
            let ux = dx / radius;
            let uy = dy / radius;
            let outside = dimension_text_is_outside(dim, style);
            let (mut x, mut y) = if outside {
                let distance = arrow + text_w * 0.5 + dimgap;
                (
                    d.definition_point.x + ux * distance,
                    d.definition_point.y + uy * distance,
                )
            } else {
                (
                    d.angle_vertex.x + ux * radius * 0.5,
                    d.angle_vertex.y + uy * radius * 0.5,
                )
            };
            if dimtad != 0 {
                let sign = if dimtad == 4 { -1.0 } else { 1.0 };
                x += -uy * perp_off * sign;
                y += ux * perp_off * sign;
            }
            Vector3::new(x, y, d.definition_point.z)
        }
        _ => {
            // Non-linear (radius / diameter / angular / ordinate): lift the
            // natural mid point straight up by the style offset. A user-dragged
            // text point is already returned by the `use_saved` gate above, so
            // we must NOT short-circuit on a merely-nonzero text_middle_point
            // here — that would ignore the style placement for auto-placed dims
            // and make a re-style a no-op. (#181)
            let mid = match dim {
                Dimension::Diameter(d) => Vector3::new(
                    (d.angle_vertex.x + d.definition_point.x) * 0.5,
                    (d.angle_vertex.y + d.definition_point.y) * 0.5,
                    (d.angle_vertex.z + d.definition_point.z) * 0.5,
                ),
                Dimension::Angular2Ln(d) => d.dimension_arc,
                Dimension::Angular3Pt(d) => d.definition_point,
                Dimension::Ordinate(d) => d.leader_endpoint,
                Dimension::Arc(d) => d.definition_point,
                Dimension::LargeRadial(d) => d.jog_point,
                _ => base.text_middle_point,
            };
            Vector3::new(mid.x, mid.y + perp_off * perp_sign_default(), mid.z)
        }
    }
}

fn perp_sign_default() -> f64 {
    1.0
}

/// The measurement-text entity (Text or MText) for a baked `*D` block, built
/// through the SAME `dimension_text_entity` the live renderer uses — so the
/// baked text matches the on-screen value, position, height, rotation,
/// alignment, text style and MText handling, and nothing shifts when the file
/// is saved and reopened (the reload renders from the block). Returns `None`
/// when the text is suppressed (`user_text` is a single space). `anno_scale` is
/// the annotative scale (1.0 for a plain model-space bake).
pub(crate) fn baked_dimension_text_entity(
    dim: &Dimension,
    document: &CadDocument,
    anno_scale: f64,
) -> Option<EntityType> {
    let style_name = dim.base().style_name.as_str();
    let style = document.dim_styles.iter().find(|s| {
        s.name.eq_ignore_ascii_case(style_name)
            || (style_name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
    });
    let dim_scale = style
        .map(|s| {
            if s.dimscale > 1e-6 {
                s.dimscale
            } else {
                anno_scale
            }
        })
        .unwrap_or(1.0);
    let dim_txt = style
        .map(|s| s.dimtxt * dim_scale)
        .unwrap_or(2.5 * dim_scale);
    let mut ent = dimension_text_entity(dim, dim_txt, style, document, dim_scale)?;
    // For a non-default-aligned Text, pin the DXF alignment point (group 11) to
    // the insertion point so other CAD programs anchor the centred text where
    // OCS does, not at the world origin.
    if let EntityType::Text(t) = &mut ent {
        t.alignment_point = Some(t.insertion_point);
    }
    Some(ent)
}

pub(crate) fn dimension_text_grip_position(
    dim: &Dimension,
    document: &CadDocument,
    anno_scale: f64,
) -> Option<Vector3> {
    // Once the user has moved the text, its saved point is authoritative.
    let base = dim.base();
    if base.text_user_positioned {
        let p = base.text_middle_point;
        if p.x * p.x + p.y * p.y + p.z * p.z > 1e-16 {
            return Some(p);
        }
    }

    // For automatic text, use the same text-building path used by the
    // dimension picture so the grip follows the actual DIMSTYLE placement,
    // including annotation scaling and fit behaviour.
    match baked_dimension_text_entity(dim, document, anno_scale)? {
        EntityType::Text(text) => Some(text.insertion_point),
        EntityType::MText(text) => Some(text.insertion_point),
        _ => None,
    }
}

#[cfg(test)]
mod dimtad_tests {
    use super::text_on_dim_line;
    use acadrust::types::Vector3;

    fn v(x: f64, y: f64) -> Vector3 {
        Vector3::new(x, y, 0.0)
    }

    // DIMTAD=Above must place text on the geometric "up" side of a horizontal
    // dimension whichever way the dimension runs; DIMTAD=Below flips it. (#144)
    #[test]
    fn above_is_up_regardless_of_direction() {
        let (first, second, defpt) = (v(0.0, 10.0), v(20.0, 10.0), v(0.0, 0.0));
        let perp = 5.0;
        let fwd = text_on_dim_line(first, second, defpt, 1.0, 0.0, 0, perp, 0.0, 1.0, false, 3, 1);
        let rev = text_on_dim_line(first, second, defpt, -1.0, 0.0, 0, perp, 0.0, 1.0, false, 3, 1);
        assert!(fwd.y > 0.0, "above must be +Y, got {}", fwd.y);
        assert!(rev.y > 0.0, "above must be +Y even reversed, got {}", rev.y);
        let below = text_on_dim_line(first, second, defpt, 1.0, 0.0, 0, perp, 0.0, 1.0, false, 3, 4);
        assert!(below.y < 0.0, "below must be -Y, got {}", below.y);
    }

    // DIMTAD=Outside (2) stays on the side farthest from the measured points,
    // whichever side of the geometry the dimension line is on.
    #[test]
    fn outside_is_away_from_object() {
        let perp = 5.0;
        // Object at y=0, dim line above it (y=10): away → further above.
        let above = text_on_dim_line(
            v(0.0, 0.0),
            v(20.0, 0.0),
            v(0.0, 10.0),
            1.0,
            0.0,
            0,
            perp,
            0.0,
            1.0,
            false,
            3,
            2,
        );
        assert!(
            above.y > 10.0,
            "outside must clear the object side, got {}",
            above.y
        );
        // Object at y=0, dim line below it (y=-10): away → further below.
        let below = text_on_dim_line(
            v(0.0, 0.0),
            v(20.0, 0.0),
            v(0.0, -10.0),
            1.0,
            0.0,
            0,
            perp,
            0.0,
            1.0,
            false,
            3,
            2,
        );
        assert!(
            below.y < -10.0,
            "outside must clear the object side, got {}",
            below.y
        );
    }
}

#[cfg(test)]
mod arch_format_tests {
    use super::{format_architectural, format_fractional};

    // A 3ft object that measures 35.99" (float noise) must carry the rounded
    // fraction up through inches into feet — not print "2'-11 1"". (Regression
    // for the fraction that reduced to 1/1 and leaked out as a bare "1".)
    #[test]
    fn arch_carries_fraction_up_to_feet() {
        assert_eq!(format_architectural(35.99, 4, 2, 1), "3'-0\"");
        assert_eq!(format_architectural(36.0, 4, 2, 1), "3'-0\"");
        // An exact 15/16 must still render as a fraction, no spurious carry.
        assert_eq!(format_architectural(35.9375, 4, 2, 1), "2'-11 15/16\"");
    }

    // Carry that stops at inches (11.999" → 12" → 1'-0", not "0'-11 1"").
    #[test]
    fn arch_carries_fraction_up_to_inches() {
        assert_eq!(format_architectural(11.999, 4, 2, 1), "1'-0\"");
    }

    #[test]
    fn arch_normal_values_unchanged() {
        assert_eq!(format_architectural(30.5, 4, 2, 1), "2'-6 1/2\"");
        assert_eq!(format_architectural(0.0, 4, 2, 1), "0'-0\"");
        assert_eq!(format_architectural(-30.25, 4, 2, 1), "-2'-6 1/4\"");
    }

    // Same carry bug lived in the plain fractional formatter.
    #[test]
    fn fractional_carries_up() {
        assert_eq!(format_fractional(35.9999, 4, 2), "36");
        assert_eq!(format_fractional(11.999, 4, 2), "12");
        assert_eq!(format_fractional(6.5, 4, 2), "6 1/2");
    }
}
