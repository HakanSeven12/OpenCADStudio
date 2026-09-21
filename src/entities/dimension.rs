use acadrust::entities::{
    Dimension, DimensionAligned, DimensionAngular2Ln, DimensionAngular3Pt, DimensionArc,
    DimensionBase, DimensionDiameter, DimensionLargeRadial, DimensionLinear, DimensionOrdinate,
    DimensionRadius,
};
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
    let compact_radial = match dim {
        Dimension::Radius(radius) => Some((&radius.base, radius.leader_length)),
        Dimension::Diameter(diameter) => Some((&diameter.base, diameter.leader_length)),
        _ => None,
    };
    if let Some((base, leader_length)) = compact_radial {
        return vec![PropSection {
            title: t!("Misc").into_owned(),
            props: vec![
                Property {
                    label: t!("Dimension style").into_owned(),
                    field: "style_name",
                    value: PropValue::PlainText(base.style_name.clone()),
                },
                edit(
                    t!("Leader Length").as_ref(),
                    "leader_length",
                    leader_length,
                ),
            ],
        }];
    }
    if let Dimension::LargeRadial(d) = dim {
        return vec![PropSection {
            title: t!("Misc").into_owned(),
            props: vec![Property {
                label: t!("Dim style").into_owned(),
                field: "style_name",
                value: PropValue::PlainText(d.base.style_name.clone()),
            }],
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
    if let Dimension::Ordinate(d) = dim {
        return vec![PropSection {
            title: t!("Misc").into_owned(),
            props: vec![
                Property {
                    label: t!("Dimension style").into_owned(),
                    field: "style_name",
                    value: PropValue::PlainText(d.base.style_name.clone()),
                },
                edit_angle(
                    t!("Rotation").as_ref(),
                    "ordinate_rotation",
                    -d.base.horizontal_direction.to_degrees(),
                ),
            ],
        }];
    }
    if let Dimension::Arc(d) = dim {
        return vec![PropSection {
            title: t!("Misc").into_owned(),
            props: vec![Property {
                label: t!("Dimension style").into_owned(),
                field: "style_name",
                value: PropValue::PlainText(d.base.style_name.clone()),
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
        Dimension::LargeRadial(_) => unreachable!("handled by compact large-radial properties"),
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
            let changed = assign_f64(value, &mut base.text_middle_point.x);
            base.text_user_positioned |= changed;
            changed
        }
        "text_y" => {
            let changed = assign_f64(value, &mut base.text_middle_point.y);
            base.text_user_positioned |= changed;
            changed
        }
        "text_z" => {
            let changed = assign_f64(value, &mut base.text_middle_point.z);
            base.text_user_positioned |= changed;
            changed
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
    let ordinate_auto_text = match dim {
        Dimension::Ordinate(ordinate) if !ordinate.base.text_user_positioned => {
            Some(dimension_text_pos_f64(dim, None, 2.5, 1.0))
        }
        _ => None,
    };
    if let Dimension::Ordinate(ordinate) = dim {
        match field {
            "ordinate_rotation" => {
                if let Some(angle) = parse_f64(value) {
                    ordinate.base.horizontal_direction = -angle.to_radians();
                    ordinate.refresh_measurement();
                }
                return;
            }
            "text_x" | "text_y" => {
                let old_text = ordinate_auto_text.unwrap_or(ordinate.base.text_middle_point);
                let mut new_text = old_text;
                let changed = if field == "text_x" {
                    assign_f64(value, &mut new_text.x)
                } else {
                    assign_f64(value, &mut new_text.y)
                };
                if changed {
                    let delta = new_text - old_text;
                    ordinate.leader_endpoint = ordinate.leader_endpoint + delta;
                    ordinate.base.text_middle_point = new_text;
                    ordinate.base.text_user_positioned = true;
                }
                return;
            }
            _ => {}
        }
    }
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
    if field == "large_radial_rotation" {
        let Some(angle) = parse_f64(value).map(f64::to_radians) else {
            return;
        };
        let delta = angle - large_radial_rotation(d);
        let origin = d.definition_point;
        let rotate = |point: &mut acadrust::types::Vector3| {
            let x = point.x - origin.x;
            let y = point.y - origin.y;
            let (sin, cos) = delta.sin_cos();
            point.x = origin.x + x * cos - y * sin;
            point.y = origin.y + x * sin + y * cos;
        };
        rotate(&mut d.chord_point);
        rotate(&mut d.override_center);
        rotate(&mut d.jog_point);
        rotate(&mut d.base.text_middle_point);
        rotate(&mut d.base.insertion_point);
        return;
    }
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

fn large_radial_rotation(d: &DimensionLargeRadial) -> f64 {
    let delta = d.chord_point - d.definition_point;
    if delta.x.abs() <= 1.0e-12 && delta.y.abs() <= 1.0e-12 {
        let visible = d.chord_point - d.override_center;
        visible.y.atan2(visible.x)
    } else {
        delta.y.atan2(delta.x)
    }
}

fn apply_transform(dim: &mut Dimension, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(dim, t, |entity, p1, p2| {
        acadrust::Entity::apply_transform(
            entity,
            &crate::scene::view::transform::reflection_about_xy_line(p1, p2),
        );
    });
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
        Dimension::Diameter(d) => {
            let dx = d.definition_point.x - d.angle_vertex.x;
            let dy = d.definition_point.y - d.angle_vertex.y;
            let len = (dx * dx + dy * dy).sqrt();

            if len <= 1e-12 {
                return None;
            }

            (
                d.angle_vertex,
                d.definition_point,
                d.angle_vertex,
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
        Dimension::Diameter(d) => {
            let dx = d.definition_point.x - d.angle_vertex.x;
            let dy = d.definition_point.y - d.angle_vertex.y;
            let len = (dx * dx + dy * dy).sqrt();

            if len <= 1e-12 {
                return;
            }

            (
                d.angle_vertex,
                d.definition_point,
                d.angle_vertex,
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
                square_grip(0, dv3(&d.feature_location)),
                center_grip(1, dv3(&d.leader_endpoint)),
                center_grip(2, text),
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
        if matches!(self, Dimension::Ordinate(_)) && grip_id == 2 {
            let old_text = dimension_text_pos_f64(self, None, 2.5, 1.0);
            let mut new_text = old_text;
            apply_to_v3(&mut new_text, &apply);
            let delta = new_text - old_text;
            if let Dimension::Ordinate(d) = self {
                d.leader_endpoint = d.leader_endpoint + delta;
                d.base.text_middle_point = new_text;
                d.base.text_user_positioned = true;
                d.refresh_measurement();
            }
            return;
        }
        // Last grip always moves the text.
        let text_grip = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => 3,
            Dimension::Radius(_) | Dimension::Diameter(_) => 2,
            Dimension::Angular2Ln(_) => 5,
            Dimension::Angular3Pt(_) => 4,
            Dimension::Ordinate(_) => 2,
            Dimension::Arc(d) => if d.has_leader { 6 } else { 4 },
            Dimension::LargeRadial(_) => 4,
        };
        if grip_id == text_grip {
            apply_to_v3(&mut self.base_mut().text_middle_point, &apply);

            // A diametric dimension must remain collinear when its text/leader
            // grip is moved. Rotate both diameter endpoints around the fixed
            // center so the leader and diameter share the same radial axis.
            if let Dimension::Diameter(d) = self {
                let center = d.center();
                let radius = d.measurement() * 0.5;
                let offset = d.base.text_middle_point - center;

                if offset.length_squared() > 1e-24 && radius > 1e-12 {
                    let radial = offset.normalize() * radius;

                    // Keep the endpoint nearest the text on the text side.
                    d.angle_vertex = center + radial;
                    d.definition_point = center - radial;
                    d.base.definition_point = d.definition_point;
                    d.base.actual_measurement = d.measurement();
                }
            }

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
            Dimension::Diameter(d) => {
                let center = d.center();
                let radius = d.measurement() * 0.5;
                let mut target = match grip_id {
                    0 => d.angle_vertex,
                    1 => d.definition_point,
                    _ => center,
                };
                if grip_id <= 1 {
                    apply_to_v3(&mut target, &apply);
                    let offset = target - center;
                    if offset.length_squared() > 1e-24 && radius > 1e-12 {
                        let radial = offset.normalize() * radius;
                        if grip_id == 0 {
                            d.angle_vertex = center + radial;
                            d.definition_point = center - radial;
                        } else {
                            d.definition_point = center + radial;
                            d.angle_vertex = center - radial;
                        }
                    }
                }
            }
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
                0 => apply_to_v3(&mut d.feature_location, &apply),
                1 => {
                    let old = d.leader_endpoint;
                    apply_to_v3(&mut d.leader_endpoint, &apply);
                    if d.base.text_user_positioned {
                        d.base.text_middle_point =
                            d.base.text_middle_point + (d.leader_endpoint - old);
                    }
                }
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
            Dimension::Ordinate(_) => (1, 2),
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
        } else if matches!(self, Dimension::Ordinate(_)) {
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
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
            Dimension::Ordinate(_) => (1, 2),
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
            Dimension::Ordinate(_) => 2,
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

pub(crate) fn resolved_dimension_style(
    source: &DimStyle,
    dimension: &Dimension,
    document: &CadDocument,
) -> DimStyle {
    use crate::entities::dim_override as ov;

    let mut style = source.clone();
    let data = &dimension.base().common.extended_data;
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

    // Negative DIMLFAC applies only in paper space, and there only to a
    // dimension that measures model space through a viewport. The associative
    // record stores the applied factor; zero marks paper-space geometry whose
    // dimension reads its sheet distance.
    if style.dimlfac < 0.0 {
        style.dimlfac = match calculated_dimlfac(dimension) {
            Some(calculated) if calculated.abs() < 1e-12 => 1.0,
            Some(calculated) => calculated.abs(),
            None => crate::scene::viewport_ref::MeasurementScale::user_lfac_for_space(
                style.dimlfac,
                dimension_in_paper_space(dimension, document),
            ),
        };
    }
    style
}

/// The linear factor recorded when the dimension was last regenerated.
fn calculated_dimlfac(dimension: &Dimension) -> Option<f64> {
    dimension
        .base()
        .common
        .extended_data
        .get_record("ACAD_DIMASSOC_CALC_DIMLFAC")
        .and_then(|record| {
            record.values.iter().find_map(|value| match value {
                acadrust::xdata::XDataValue::Real(factor) if factor.is_finite() => Some(*factor),
                _ => None,
            })
        })
}

/// Whether the dimension's owner is paper space, even without a Layout object.
pub(crate) fn dimension_in_paper_space(dimension: &Dimension, document: &CadDocument) -> bool {
    let owner = dimension.base().common.owner_handle;
    if !owner.is_valid() {
        return false;
    }
    document.objects.values().any(|object| {
        matches!(object,
            acadrust::objects::ObjectType::Layout(layout)
                if layout.name != "Model" && layout.block_record == owner
        )
    }) || document.block_records.iter().any(|block| {
        block.handle == owner && block.name.to_ascii_uppercase().starts_with("*PAPER_SPACE")
    })
}

/// The value a dimension's text reports for its measurement: the raw
/// geometric measurement times the resolved linear factor. Angular dimensions
/// never scale.
///
/// `actual_measurement` (DXF group 42) stays **raw** — that is what the format
/// specifies and what export writes — so this is the one helper UI surfaces
/// use when they want the number the user reads.
pub(crate) fn displayed_measurement(dimension: &Dimension, style: Option<&DimStyle>) -> f64 {
    let raw = dimension.measurement();
    if matches!(
        dimension,
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)
    ) {
        return raw;
    }
    let lfac = style.map(|s| s.dimlfac).unwrap_or(1.0);
    if lfac.abs() < 1e-12 {
        raw
    } else {
        raw * lfac
    }
}

/// Build the linear-dimension property groups from the assigned style plus
/// any entity-level DSTYLE overrides. Conditional rows remain visible but are
/// read-only while their controlling option is disabled.
pub fn style_sections(
    style: &DimStyle,
    dimension: &Dimension,
    document: &CadDocument,
) -> Vec<PropSection> {
    use crate::entities::dim_override as ov;

    let effective_style = resolved_dimension_style(style, dimension, document);
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

    let s = &effective_style;
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
    let arrow_1 = if matches!(dimension, Dimension::Radius(_) | Dimension::LargeRadial(_)) {
        block_name(
            document,
            ov::handle(data, ov::DIMLDRBLK).unwrap_or(s.dimldrblk),
            "Closed filled",
        )
    } else {
        let inherited = if matches!(dimension, Dimension::Diameter(_)) && !s.dimsah {
            (s.dimblk, s.dimblk_name.as_str())
        } else {
            (s.dimblk1, s.dimblk1_name.as_str())
        };
        arrow_name(ov::DIMBLK1, inherited.0, inherited.1)
    };
    let arrow_2 = if matches!(dimension, Dimension::Diameter(_)) && !s.dimsah {
        arrow_name(ov::DIMBLK2, s.dimblk, &s.dimblk_name)
    } else {
        arrow_name(ov::DIMBLK2, s.dimblk2, &s.dimblk2_name)
    };
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

    let precision_options: Vec<String> = (0..=8).map(precision_label).collect();
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
                    dimtix || matches!(dimension, Dimension::LargeRadial(_)),
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
                        "Right-to-Left"
                    } else {
                        "Left-to-Right"
                    },
                    &["Left-to-Right", "Right-to-Left"],
                    true,
                ),
                property(
                    t!("Measurement").as_ref(),
                    "measurement",
                    // The value the dimension text reports (DIMLFAC applied,
                    // including viewport compensation) — not the raw paper
                    // distance, which would disagree with what is drawn.
                    PropValue::ReadOnly(format!(
                        "{:.4}",
                        displayed_measurement(dimension, Some(s))
                    )),
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
                    t!("Dim prefix").as_ref(),
                    "dim_prefix",
                    dim_prefix.clone(),
                    true,
                ),
                text(
                    t!("Dim suffix").as_ref(),
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
                    // The *raw* signed DIMLFAC: a negative value is the
                    // paper-space / viewport-compensation convention, and
                    // hiding its sign would make the row uneditable in
                    // practice. `s.dimlfac` is sign-resolved, so fall back to
                    // the unresolved source style instead.
                    real(ov::DIMLFAC, style.dimlfac),
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
                    matches!(dimlunit, 3 | 4),
                ),
                choice(
                    t!("Suppress zero inches").as_ref(),
                    "dim_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimzin)),
                    &["Yes", "No"],
                    matches!(dimlunit, 3 | 4),
                ),
                property(
                    t!("Precision").as_ref(),
                    "dim_precision",
                    PropValue::Choice {
                        selected: precision_label(int(ov::DIMDEC, s.dimdec)),
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
                            selected: precision_label(int(ov::DIMALTD, s.dimaltd)),
                            options: precision_options.clone(),
                        }
                    } else {
                        PropValue::ReadOnly(precision_label(int(ov::DIMALTD, s.dimaltd)))
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
                    t!("Alt round").as_ref(),
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
                            selected: precision_label(int(ov::DIMTDEC, s.dimtdec)),
                            options: precision_options.clone(),
                        }
                    } else {
                        PropValue::ReadOnly(precision_label(int(ov::DIMTDEC, s.dimtdec)))
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
                    &["Decimal Separator", "Operational Symbols"],
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
                            selected: precision_label(int(ov::DIMALTTD, s.dimalttd)),
                            options: precision_options,
                        }
                    } else {
                        PropValue::ReadOnly(precision_label(int(ov::DIMALTTD, s.dimalttd)))
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
    if matches!(
        dimension,
        Dimension::Radius(_) | Dimension::Diameter(_) | Dimension::LargeRadial(_)
    ) {
        let dimcen = real(ov::DIMCEN, s.dimcen);
        let center_type = if dimcen > 1e-12 {
            "Mark"
        } else if dimcen < -1e-12 {
            "Line"
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
                "dim_line_2",
                "dim_line_color",
                "dim_linetype",
                "dim_ext_linetype_1",
                "dim_ext_line_1",
                "dim_ext_line_color",
                "dim_ext_line_ext",
                "dim_ext_line_offset",
            ];
            const DIAMETER_LINE_FIELDS: &[&str] = &[
                "dim_arrowhead_1",
                "dim_arrowhead_2",
                "dim_arrow_size",
                "dim_line_lineweight",
                "dim_ext_line_lineweight",
                "dim_line_1",
                "dim_line_2",
                "dim_line_color",
                "dim_linetype",
                "dim_ext_linetype_1",
                "dim_ext_line_1",
                "dim_ext_line_color",
                "dim_ext_line_ext",
                "dim_ext_line_offset",
            ];
            const LARGE_RADIAL_LINE_FIELDS: &[&str] = &[
                "dim_arrowhead_1",
                "dim_arrow_size",
                "dim_line_lineweight",
                "dim_ext_line_lineweight",
                "dim_line_2",
                "dim_line_color",
                "dim_linetype",
                "dim_ext_linetype_1",
                "dim_ext_line_1",
                "dim_ext_line_color",
                "dim_ext_line_ext",
                "dim_ext_line_offset",
            ];
            let retained = if matches!(dimension, Dimension::Diameter(_)) {
                DIAMETER_LINE_FIELDS
            } else if matches!(dimension, Dimension::LargeRadial(_)) {
                LARGE_RADIAL_LINE_FIELDS
            } else {
                RADIUS_LINE_FIELDS
            };
            lines
                .props
                .retain(|property| retained.contains(&property.field));
            lines.props.push(choice(
                t!("Center mark").as_ref(),
                "dim_center_type",
                center_type,
                &["None", "Mark", "Line"],
                true,
            ));
            lines.props.push(number(
                t!("Center mark size").as_ref(),
                "dim_center_size",
                dimcen.abs(),
                center_type != "None",
            ));
            if let Dimension::LargeRadial(radial) = dimension {
                lines.props.push(edit(
                    t!("Center location override X").as_ref(),
                    "override_x",
                    radial.override_center.x,
                ));
                lines.props.push(edit(
                    t!("Center location override Y").as_ref(),
                    "override_y",
                    radial.override_center.y,
                ));
                lines.props.push(edit(
                    t!("Jog location X").as_ref(),
                    "jog_x",
                    radial.jog_point.x,
                ));
                lines.props.push(edit(
                    t!("Jog location Y").as_ref(),
                    "jog_y",
                    radial.jog_point.y,
                ));
                lines.props.push(edit_angle(
                    t!("Jog angle").as_ref(),
                    "jog_angle",
                    radial.jog_angle.to_degrees(),
                ));
            }
            if matches!(dimension, Dimension::Radius(_) | Dimension::LargeRadial(_)) {
                if let Some(arrow) = lines
                    .props
                    .iter_mut()
                    .find(|property| property.field == "dim_arrowhead_1")
                {
                    arrow.label = t!("Arrow").into_owned();
                    arrow.field = "dim_radial_arrow";
                }
                if let Some(dim_line) = lines
                    .props
                    .iter_mut()
                    .find(|property| property.field == "dim_line_2")
                {
                    dim_line.label = t!("Dim line").into_owned();
                }
            }
            if matches!(dimension, Dimension::LargeRadial(_)) {
                for property in &mut lines.props {
                    property.label = match property.field {
                        "dim_ext_line_1" => t!("Ext line").into_owned(),
                        "dim_ext_line_lineweight" => t!("Ext line weight").into_owned(),
                        "dim_ext_linetype_1" => t!("Ext line type").into_owned(),
                        _ => property.label.clone(),
                    };
                }
                const LARGE_RADIAL_ORDER: &[&str] = &[
                    "dim_radial_arrow",
                    "dim_arrow_size",
                    "dim_center_type",
                    "dim_center_size",
                    "dim_line_lineweight",
                    "dim_line_2",
                    "dim_linetype",
                    "dim_line_color",
                    "override_x",
                    "override_y",
                    "jog_x",
                    "jog_y",
                    "dim_ext_line_1",
                    "jog_angle",
                    "dim_ext_line_lineweight",
                    "dim_ext_linetype_1",
                    "dim_ext_line_ext",
                    "dim_ext_line_color",
                    "dim_ext_line_offset",
                ];
                lines.props.sort_by_key(|property| {
                    LARGE_RADIAL_ORDER
                        .iter()
                        .position(|field| *field == property.field)
                        .unwrap_or(LARGE_RADIAL_ORDER.len())
                });
            }
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
        if matches!(dimension, Dimension::LargeRadial(_)) {
            for (section_name, excluded) in [
                (t!("Text"), &["dim_text_pos_hor"][..]),
                (t!("Fit"), &["dim_line_inside"][..]),
                (
                    t!("Alternate Units"),
                    &["dim_alt_sub_units_scale", "dim_alt_sub_units_suffix"][..],
                ),
            ] {
                if let Some(section) = sections
                    .iter_mut()
                    .find(|section| section.title == section_name.as_ref())
                {
                    section
                        .props
                        .retain(|property| !excluded.contains(&property.field));
                }
            }
            for (section_name, fields) in [
                (
                    t!("Text"),
                    &["dim_text_outside_align", "dim_text_pos_vert"][..],
                ),
                (t!("Fit"), &["dim_fit", "dim_text_inside"][..]),
            ] {
                if let Some(section) = sections
                    .iter_mut()
                    .find(|section| section.title == section_name.as_ref())
                {
                    for property in &mut section.props {
                        if fields.contains(&property.field) {
                            if let PropValue::Choice { selected, .. } = &property.value {
                                property.value = PropValue::ReadOnly(selected.clone());
                            }
                        }
                    }
                }
            }
            if let Some(fit) = sections
                .iter_mut()
                .find(|section| section.title == t!("Fit").as_ref())
            {
                const LARGE_RADIAL_FIT_ORDER: &[&str] = &[
                    "dim_line_forced",
                    "dim_scale_overall",
                    "dim_fit",
                    "dim_text_inside",
                    "dim_text_movement",
                ];
                fit.props.sort_by_key(|property| {
                    LARGE_RADIAL_FIT_ORDER
                        .iter()
                        .position(|field| *field == property.field)
                        .unwrap_or(LARGE_RADIAL_FIT_ORDER.len())
                });
            }
            if let Some(alternate_units) = sections
                .iter_mut()
                .find(|section| section.title == t!("Alternate Units").as_ref())
            {
                const LARGE_RADIAL_ALT_UNITS_ORDER: &[&str] = &[
                    "dim_alt_enabled",
                    "dim_alt_format",
                    "dim_alt_precision",
                    "dim_alt_roundoff",
                    "dim_alt_scale_factor",
                    "dim_alt_suppress_leading_zeros",
                    "dim_alt_suppress_trailing_zeros",
                    "dim_alt_suppress_zero_feet",
                    "dim_alt_suppress_zero_inches",
                    "dim_alt_prefix",
                    "dim_alt_suffix",
                ];
                alternate_units.props.sort_by_key(|property| {
                    LARGE_RADIAL_ALT_UNITS_ORDER
                        .iter()
                        .position(|field| *field == property.field)
                        .unwrap_or(LARGE_RADIAL_ALT_UNITS_ORDER.len())
                });
            }
            if let Some(tolerances) = sections
                .iter_mut()
                .find(|section| section.title == t!("Tolerances").as_ref())
            {
                const LARGE_RADIAL_TOLERANCE_ORDER: &[&str] = &[
                    "dim_alt_tolerance_suppress_zero_inches",
                    "dim_tolerance_alignment",
                    "dim_tolerance_display",
                    "dim_tolerance_limit_lower",
                    "dim_tolerance_limit_upper",
                    "dim_tolerance_pos_vert",
                    "dim_tolerance_precision",
                    "dim_tolerance_suppress_leading_zeros",
                    "dim_tolerance_suppress_trailing_zeros",
                    "dim_tolerance_suppress_zero_feet",
                    "dim_tolerance_suppress_zero_inches",
                    "dim_tolerance_text_height",
                    "dim_alt_tolerance_precision",
                    "dim_alt_tolerance_suppress_leading_zeros",
                    "dim_alt_tolerance_suppress_trailing_zeros",
                    "dim_alt_tolerance_suppress_zero_feet",
                ];
                tolerances.props.sort_by_key(|property| {
                    LARGE_RADIAL_TOLERANCE_ORDER
                        .iter()
                        .position(|field| *field == property.field)
                        .unwrap_or(LARGE_RADIAL_TOLERANCE_ORDER.len())
                });
            }
            if !dimension.base().text_user_positioned {
                let text_position = styled_dimension_text_position(dimension, s, 1.0);
                for section in &mut sections {
                    for property in &mut section.props {
                        match property.field {
                            "text_x" => {
                                property.value =
                                    PropValue::EditText(format!("{:.4}", text_position.x));
                            }
                            "text_y" => {
                                property.value =
                                    PropValue::EditText(format!("{:.4}", text_position.y));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    if matches!(dimension, Dimension::Arc(_)) {
        if let Some(lines) = sections
            .iter_mut()
            .find(|section| section.title == t!("Lines & Arrows").as_ref())
        {
            let symbol = match int(ov::DIMARCSYM, s.dimarcsym) {
                1 => "Above dimension text",
                2 => "None",
                _ => "Preceding dimension text",
            };
            lines.props.insert(
                3,
                choice(
                    t!("Arc length symbol").as_ref(),
                    "dim_arc_symbol",
                    symbol,
                    &[
                        "Preceding dimension text",
                        "Above dimension text",
                        "None",
                    ],
                    true,
                ),
            );
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

    if matches!(dimension, Dimension::Ordinate(_)) {
        for section in &mut sections {
            section.props.retain(|property| match section.title.as_str() {
                title if title == t!("Lines & Arrows").as_ref() => matches!(
                    property.field,
                    "dim_arrow_size"
                        | "dim_ext_line_lineweight"
                        | "dim_ext_linetype_1"
                        | "dim_ext_line_1"
                        | "dim_ext_line_fixed"
                        | "dim_ext_line_fixed_length"
                        | "dim_ext_line_color"
                        | "dim_ext_line_offset"
                ),
                title if title == t!("Text").as_ref() => !matches!(
                    property.field,
                    "dim_text_outside_align" | "dim_text_pos_hor" | "dim_text_inside_align"
                ),
                title if title == t!("Fit").as_ref() => matches!(
                    property.field,
                    "dim_text_movement" | "dim_scale_overall"
                ),
                _ => true,
            });
        }
        let dim_scale = if s.dimscale > 1e-6 { s.dimscale } else { 1.0 };
        let text_position = dimension_text_pos_f64(
            dimension,
            Some(s),
            real(ov::DIMTXT, s.dimtxt) * dim_scale,
            dim_scale,
        );
        for section in &mut sections {
            for property in &mut section.props {
                match property.field {
                    "text_x" => {
                        property.value = PropValue::EditText(format!("{:.4}", text_position.x));
                    }
                    "text_y" => {
                        property.value = PropValue::EditText(format!("{:.4}", text_position.y));
                    }
                    _ => {}
                }
            }
        }
    }
    sections
}

fn dimension_style_scale(style: &DimStyle, fallback: f64) -> f64 {
    if style.dimscale > 1.0e-6 {
        style.dimscale
    } else {
        fallback
    }
}

fn styled_dimension_text_position(
    dimension: &Dimension,
    style: &DimStyle,
    fallback_scale: f64,
) -> Vector3 {
    let scale = dimension_style_scale(style, fallback_scale);
    dimension_text_pos_f64(dimension, Some(style), style.dimtxt * scale, scale)
}

pub(crate) fn materialize_large_radial_text_position(
    document: &mut CadDocument,
    handle: Handle,
    value: &str,
) {
    if parse_f64(value).is_none() {
        return;
    }
    let Some(EntityType::Dimension(dimension)) = document.get_entity(handle) else {
        return;
    };
    if !matches!(dimension, Dimension::LargeRadial(_))
        || dimension.base().text_user_positioned
    {
        return;
    }
    let source = document.dim_styles.iter().find(|style| {
        style.name.eq_ignore_ascii_case(&dimension.base().style_name)
            || (dimension.base().style_name.trim().is_empty()
                && style.name.eq_ignore_ascii_case("Standard"))
    });
    let position = source
        .map(|style| resolved_dimension_style(style, dimension, document))
        .map(|style| styled_dimension_text_position(dimension, &style, 1.0))
        .unwrap_or_else(|| dimension_text_pos_f64(dimension, None, 2.5, 1.0));
    if let Some(EntityType::Dimension(Dimension::LargeRadial(radial))) =
        document.get_entity_mut(handle)
    {
        radial.base.text_middle_point = position;
    }
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

fn precision_label(value: i16) -> String {
    let decimals = value.clamp(0, 8) as usize;
    if decimals == 0 {
        "0".to_string()
    } else {
        format!("0.{}", "0".repeat(decimals))
    }
}

fn tolerance_alignment_label(value: i16) -> &'static str {
    if value == 0 {
        "Decimal Separator"
    } else {
        "Operational Symbols"
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
    add_polyline, add_segment, append_arrow, arrow_from_block,
    arrow_from_block_with_deferred_hatch, normalized_or, ArrowKind, DimGeom,
};
use crate::scene::model::wire_model::{SnapHint, TangentGeom, WireModel};

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

fn dimension_jog_point(dimension: &Dimension) -> Option<Vec3> {
    if !matches!(dimension, Dimension::Linear(_) | Dimension::Aligned(_)) {
        return None;
    }
    dimension
        .base()
        .common
        .extended_data
        .get_record("ACAD_DSTYLE_DIMJAG_POSITION")
        .and_then(|record| {
            record.values.iter().rev().find_map(|value| match value {
                acadrust::xdata::XDataValue::Point3D(point) => Some(vec3_local(*point)),
                _ => None,
            })
        })
}

fn apply_dimension_jog(
    lines: &mut Vec<[f32; 3]>,
    requested: Vec3,
    normal: Vec3,
    size: f32,
    angle: f32,
) {
    if lines.len() < 2 {
        return;
    }
    let segments = lines
        .windows(2)
        .enumerate()
        .filter(|(_, segment)| segment[0][0].is_finite() && segment[1][0].is_finite())
        .map(|(index, segment)| {
            (
                index,
                [segment[0].map(f64::from), segment[1].map(f64::from)],
            )
        })
        .collect::<Vec<_>>();
    let coordinates = segments.iter().map(|(_, segment)| *segment).collect::<Vec<_>>();
    let Some((candidate, center)) = cadkernel::space::nearest_segment_point(
        &coordinates,
        requested.to_array().map(f64::from),
    ) else {
        return;
    };
    let target = segments[candidate].0;
    let Some(jog) = cadkernel::space::dimension_jog_points(
        coordinates[candidate],
        center,
        normal.to_array().map(f64::from),
        size as f64,
        angle as f64,
    ) else {
        return;
    };
    let jog = jog.map(|point| point.map(|value| value as f32));
    let first = lines[target];
    let second = lines[target + 1];

    let mut output = Vec::with_capacity(lines.len() + 4);
    output.extend_from_slice(&lines[..target]);
    if !output.is_empty() && !output.last().is_some_and(|point| point[0].is_nan()) {
        output.push([f32::NAN; 3]);
    }
    if first != jog[0] {
        output.push(first);
        output.push(jog[0]);
        output.push([f32::NAN; 3]);
    }
    output.extend(jog);
    if jog[3] != second {
        output.push([f32::NAN; 3]);
        output.push(jog[3]);
        output.push(second);
    }
    if target + 2 < lines.len() {
        if !output.last().is_some_and(|point| point[0].is_nan())
            && !lines[target + 2][0].is_nan()
        {
            output.push([f32::NAN; 3]);
        }
        output.extend_from_slice(&lines[target + 2..]);
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
    if let Dimension::Arc(arc) = dim {
        if let Some((local_arc, normal)) = arc_dimension_in_ocs(arc) {
            let mut wires = tessellate_dimension_inner(
                document,
                handle,
                &Dimension::Arc(local_arc),
                selected,
                entity_color,
                line_weight_px,
                anno_scale,
                selected_set,
                active_viewport,
                bg_color,
                None,
                world_per_pixel,
            );
            for wire in &mut wires {
                map_wire_ocs_to_wcs(wire, normal);
            }
            return wires;
        }
    }
    if let Dimension::LargeRadial(radial) = dim {
        if let Some((local_radial, normal)) = large_radial_dimension_in_ocs(radial) {
            let mut wires = tessellate_dimension_inner(
                document,
                handle,
                &Dimension::LargeRadial(local_radial),
                selected,
                entity_color,
                line_weight_px,
                anno_scale,
                selected_set,
                active_viewport,
                bg_color,
                None,
                world_per_pixel,
            );
            for wire in &mut wires {
                map_wire_ocs_to_wcs(wire, normal);
            }
            return wires;
        }
    }
    if let Some((local, normal)) = linear_dimension_in_ocs(dim) {
        let mut wires = tessellate_dimension_inner(
            document,
            handle,
            &local,
            selected,
            entity_color,
            line_weight_px,
            anno_scale,
            selected_set,
            active_viewport,
            bg_color,
            None,
            world_per_pixel,
        );
        for wire in &mut wires {
            map_wire_ocs_to_wcs(wire, normal);
        }
        return wires;
    }
    let name = handle.value().to_string();
    // (Baked-block fast path moved up into scene::tessellate_entity so the
    // recursive call goes through the LOD ladder, not the kernel path.)

    let style_name = &dim.base().style_name;
    let source_style = document.dim_styles.iter().find(|s| {
        s.name.eq_ignore_ascii_case(style_name)
            || (style_name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
    });
    let effective_style = source_style.map(|style| resolved_dimension_style(style, dim, document));
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
    //   1. DIMTSZ>0 → oblique tick (overrides arrow blocks).
    //   2. Radius and large-radius dimensions → DIMLDRBLK.
    //   3. DIMSAH false → DIMBLK on both ends.
    //   4. DIMSAH true  → DIMBLK1 (first end), DIMBLK2 (second end).
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
        if matches!(dim, Dimension::Radius(_) | Dimension::LargeRadial(_)) {
            let handle = crate::entities::dim_override::handle(
                &dim.base().common.extended_data,
                crate::entities::dim_override::DIMLDRBLK,
            )
            .unwrap_or(s.dimldrblk);
            let arrow = arrow_from_block_with_deferred_hatch(
                document,
                handle,
                dimasz,
                defer_arrow_hatches,
            );
            (arrow.clone(), arrow)
        } else if matches!(dim, Dimension::Diameter(_)) {
            let data = &dim.base().common.extended_data;
            let first = crate::entities::dim_override::handle(
                data,
                crate::entities::dim_override::DIMBLK1,
            )
            .unwrap_or(if dimsah { s.dimblk1 } else { s.dimblk });
            let second = crate::entities::dim_override::handle(
                data,
                crate::entities::dim_override::DIMBLK2,
            )
            .unwrap_or(if dimsah { s.dimblk2 } else { s.dimblk });
            (
                arrow_from_block_with_deferred_hatch(
                    document,
                    first,
                    dimasz,
                    defer_arrow_hatches,
                ),
                arrow_from_block_with_deferred_hatch(
                    document,
                    second,
                    dimasz,
                    defer_arrow_hatches,
                ),
            )
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

    let text_layout = dimension_text_layout(dim, style, dim_txt, dim_scale);

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
            text_width: text_layout.width,
            dimatfit: style.map(|s| s.dimatfit).unwrap_or(3),
            dimtix: style.is_some_and(|s| s.dimtix),
            dimtofl: style.map(|s| s.dimtofl).unwrap_or(false),
            text_position: text_layout.position,
            horizontal_text: text_layout.horizontal,
            text_movement: style.map(|s| s.dimtmove).unwrap_or(0),
            text_break: text_layout.break_box,
            leader_anchor: style
                .filter(|s| s.dimtmove == 1)
                .and_then(|_| stored_text_point(dim))
                .map(vec3_local),
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

    // DIMTMOVE=1 connects the dimension line to rendered text.
    if let Some(s) = style {
        if s.dimtmove == 1 {
            if let Some((anchor, txt, under_text)) = dimtmove_leader_endpoints(dim, text_layout) {
                let gap = dim_txt as f32 * 0.5;
                if under_text || (txt - anchor).length() > gap * 2.0 {
                    add_segment_with_text_break(
                        &mut geom.dim_lines,
                        anchor,
                        txt,
                        text_layout.break_box,
                    );
                }
            }
        }
        // DIMUPT governs interactive creation-time text placement; saved
        // geometry already carries the resulting position.
        let _ = s.dimupt;
        let _ = s.dimarcsym;
        // DIMUNIT is the obsolete pre-R2000 linear unit format; DIMLUNIT
        // supersedes it. Read but not honoured.
        let _ = s.dimunit;
    }
    if let Some(point) = dimension_jog_point(dim) {
        let jog_angle = style
            .map(|style| style.dimjogang as f32)
            .unwrap_or(std::f32::consts::FRAC_PI_4);
        apply_dimension_jog(
            &mut geom.dim_lines,
            point,
            vec3_local(dim.base().normal),
            dim_txt as f32 * CELL_WIDTH as f32,
            jog_angle,
        );
    }
    apply_dimension_breaks(document, handle, &mut geom.dim_lines);
    apply_dimension_breaks(document, handle, &mut geom.ext_lines);
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
    // 0=ByBlock and 256=ByLayer fall through to entity_color. Each honours the
    // per-object ACAD_DSTYLE override (codes 176/177/178) the Properties panel
    // reads and writes, so an edited colour renders without touching the style.
    let xd = &dim.base().common.extended_data;
    let dim_color = if selected {
        WireModel::SELECTED
    } else {
        resolve_dim_color(
            dim_color_index(
                xd,
                crate::entities::dim_override::DIMCLRD,
                style.map(|s| s.dimclrd).unwrap_or(0),
            ),
            entity_color,
        )
    };
    let ext_color = if selected {
        WireModel::SELECTED
    } else {
        resolve_dim_color(
            dim_color_index(
                xd,
                crate::entities::dim_override::DIMCLRE,
                style.map(|s| s.dimclre).unwrap_or(0),
            ),
            entity_color,
        )
    };
    let text_color = if selected {
        entity_color // text wire color set by inner tessellate; keep entity tint
    } else {
        resolve_dim_color(
            dim_color_index(
                xd,
                crate::entities::dim_override::DIMCLRT,
                style.map(|s| s.dimclrt).unwrap_or(0),
            ),
            entity_color,
        )
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
                    bg_adapt: None,
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
                    bg_adapt: None,
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
                bg_adapt: None,
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
        bg_adapt: None,
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

    if let Some(symbol) = style.and_then(|style| {
        arc_length_symbol_points(dim, Some(style), dim_txt, dim_scale, style.dimarcsym)
    }) {
        let mut points = Vec::new();
        add_polyline(&mut points, &symbol);
        wires.push(WireModel {
            bg_adapt: None,
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
            points,
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
                    bg_adapt: None,
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
                bg_adapt: None,
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
/// The ACI index a dimension colour renders with: the per-object ACAD_DSTYLE
/// override when the entity carries one, else the dimension style's value.
/// The Properties panel reads and writes these same overrides, so the renderer
/// has to consult them or an edited colour shows in the panel and nowhere else.
fn dim_color_index(
    xd: &acadrust::xdata::ExtendedData,
    code: i16,
    inherited: i16,
) -> i16 {
    crate::entities::dim_override::int(xd, code).unwrap_or(inherited)
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

/// Endpoints for the DIMTMOVE=1 leader.
fn dimtmove_leader_endpoints(
    dim: &Dimension,
    text: DimensionTextLayout,
) -> Option<(Vec3, Vec3, bool)> {
    let lv = |v| vec3_local(v);
    let txt = text.position;
    let linear = |first: Vec3, second: Vec3, definition: Vec3, axis: [f64; 2]| {
        let bounds = text.break_box?;
        let xy = |point: Vec3| [point.x as f64, point.y as f64];
        let (leader, under_text) = cadkernel::geom2d::linear_dimension_leader(
            cadkernel::geom2d::Line { start: xy(first), end: xy(second) },
            xy(definition),
            axis,
            xy(txt),
            [
                (bounds.half_width + bounds.padding) as f64,
                (bounds.half_height + bounds.padding) as f64,
            ],
            bounds.rotation,
        )?;
        let point = |p: [f64; 2]| Vec3::new(p[0] as f32, p[1] as f32, first.z);
        Some((point(leader.start), point(leader.end), under_text))
    };
    match dim {
        Dimension::Linear(d) => {
            return linear(
                lv(d.first_point),
                lv(d.second_point),
                lv(d.definition_point),
                [d.rotation.cos(), d.rotation.sin()],
            );
        }
        Dimension::Aligned(d) => {
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            return linear(
                first,
                second,
                lv(d.definition_point),
                [d.second_point.x - d.first_point.x, d.second_point.y - d.first_point.y],
            );
        }
        _ => {}
    }
    let anchor = match dim {
        Dimension::Radius(_) => return None,
        Dimension::Diameter(d) => {
            let chord = lv(d.angle_vertex);
            let far_chord = lv(d.definition_point);
            if chord.distance_squared(txt) <= far_chord.distance_squared(txt) {
                chord
            } else {
                far_chord
            }
        }
        Dimension::Angular2Ln(d) => lv(d.dimension_arc),
        Dimension::Angular3Pt(d) => lv(d.definition_point),
        Dimension::LargeRadial(d) => {
            let chord = lv(d.chord_point);
            let jog = lv(d.jog_point);
            let override_center = lv(d.override_center);
            let (near, far) =
                jogged_radial_break(chord, jog, override_center, d.jog_angle as f32);
            let curves = [(chord, near), (near, far), (far, override_center)].map(
                |(start, end)| {
                    cadkernel::geom2d::Curve::Line(cadkernel::geom2d::Line {
                        start: [start.x as f64, start.y as f64],
                        end: [end.x as f64, end.y as f64],
                    })
                },
            );
            let (_, closest) = cadkernel::geom2d::nearest_of(
                curves.iter(),
                [txt.x as f64, txt.y as f64],
            )?;
            Vec3::new(closest.point[0] as f32, closest.point[1] as f32, chord.z)
        }
        _ => return None,
    };
    Some((anchor, txt, false))
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
    // CELL_WIDTH × text_height per character approximates the average glyph
    // aspect of the bundled stick fonts. Inflate by 1 DIMGAP on each side.
    let stack_scale = style.map(dimtfac_or_one).unwrap_or(1.0);
    let approx_w = text_cells(&value, stack_scale) * text_height * CELL_WIDTH + dimgap * 2.0;
    let approx_h = dimension_text_extent_height(dim, style, text_height) + dimgap * 2.0;
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

fn arc_length_symbol_points(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
    symbol_position: i16,
) -> Option<Vec<Vec3>> {
    if !matches!(dim, Dimension::Arc(_)) || symbol_position == 2 {
        return None;
    }
    let value = dimension_text_value(dim, style)?;
    if value.is_empty() || text_height <= 1.0e-12 {
        return None;
    }

    let position = dimension_text_pos_f64(dim, style, text_height, dim_scale);
    let rotation = dimension_text_rotation(dim, style);
    let (sin_rotation, cos_rotation) = rotation.sin_cos();
    let stack_scale = style.map(dimtfac_or_one).unwrap_or(1.0);
    let text_width = text_cells(&value, stack_scale) * text_height * CELL_WIDTH;
    let symbol_width = text_height * 0.62;
    let symbol_height = text_height * 0.20;
    let (center_x, center_y) = if symbol_position == 1 {
        (0.0, text_height * 0.72)
    } else {
        (-(text_width * 0.5 + symbol_width * 0.70), text_height * 0.02)
    };
    let transform = |x: f64, y: f64| {
        let local_x = center_x + x;
        let local_y = center_y + y;
        Vec3::new(
            (position.x + local_x * cos_rotation - local_y * sin_rotation) as f32,
            (position.y + local_x * sin_rotation + local_y * cos_rotation) as f32,
            position.z as f32,
        )
    };

    let steps = 8usize;
    Some(
        (0..=steps)
            .map(|index| {
                let t = index as f64 / steps as f64;
                let x = (t - 0.5) * symbol_width;
                let normalized = x / (symbol_width * 0.5);
                let y = symbol_height * (1.0 - normalized * normalized);
                transform(x, y)
            })
            .collect(),
    )
}

struct SuppressFlags {
    ext1: bool,
    ext2: bool,
    dim1: bool,
    dim2: bool,
}

#[derive(Clone, Copy)]
struct TextBreak {
    center: Vec3,
    half_width: f32,
    half_height: f32,
    padding: f32,
    rotation: f64,
}

#[derive(Clone, Copy)]
struct DimensionTextLayout {
    position: Vec3,
    width: f32,
    break_box: Option<TextBreak>,
    horizontal: bool,
}

fn dimension_text_layout(
    dimension: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
) -> DimensionTextLayout {
    let gap = style
        .map(|style| (style.dimgap.abs() * dim_scale) as f32)
        .unwrap_or(0.09);
    let width = dimension_text_cells(dimension, style)
        .map(|cells| cells as f32 * text_height as f32 * CELL_WIDTH as f32 + gap * 2.0)
        .unwrap_or(0.0);
    let position = vec3_local(dimension_text_pos_f64(
        dimension,
        style,
        text_height,
        dim_scale,
    ));
    let bare_width = (width - gap * 2.0).max(0.0);
    let break_box = (bare_width > 0.0).then_some(TextBreak {
        center: position,
        half_width: bare_width * 0.5,
        // A limits stack is two lines tall; the break and the arrow fit both
        // have to clear the whole block.
        half_height: dimension_text_extent_height(dimension, style, text_height) as f32 * 0.5,
        padding: gap,
        rotation: dimension_text_rotation(dimension, style),
    });
    let outside = dimension_text_is_outside(dimension, style);
    let horizontal = style.is_some_and(|style| {
        (outside && style.dimtoh) || (!outside && style.dimtih)
    });
    DimensionTextLayout {
        position,
        width,
        break_box,
        horizontal,
    }
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
    dimtix: bool,
    dimtofl: bool,
    text_position: Vec3,
    horizontal_text: bool,
    text_movement: i16,
    text_break: Option<TextBreak>,
    /// DIMTMOVE 1: the point the file stores is where the leader's hook starts,
    /// so the leader is drawn to it exactly and only the text is estimated.
    leader_anchor: Option<Vec3>,
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
                if !suppress.dim2 {
                    add_segment(&mut g.dim_lines, center, a);
                    add_segment(&mut g.dim_lines, a, b);
                    add_segment(&mut g.dim_lines, b, point);
                }
            }
            let radius = (point - center).length();
            let text_is_outside = text.distance(center) > radius + 1e-5;
            // Text inside the arc: the dimension line runs from the arc point
            // to the centre. Text outside: only a leader from the arc point to
            // the text, unless DIMTOFL asks for the inside line as well.
            if !jogged && !suppress.dim2 && (!text_is_outside || params.dimtofl) {
                add_segment(&mut g.dim_lines, center, point);
            }
            if text_is_outside && !suppress.dim2 {
                append_radial_leader(&mut g, point, text, &params);
            }
            if !suppress.dim2 {
                // The arrowhead sits on the arc with its body toward the text.
                let body = if text_is_outside { point - center } else { center - point };
                append_arrow(&mut g, point, normalized_or(body, Vec3::X), arrow1);
            }
            if text_is_outside {
                append_center_mark(&mut g, center, params.dimcen, radius);
            }
        }
        Dimension::Diameter(d) => {
            let chord = lv(d.angle_vertex);
            let far_chord = lv(d.definition_point);
            append_diameter_dimension(
                &mut g,
                chord,
                far_chord,
                lv(d.base.text_middle_point),
                arrow1,
                arrow2,
                params,
                suppress,
            );
            let center = (chord + far_chord) * 0.5;
            let radius = chord.distance(far_chord) * 0.5;
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
            if !suppress.ext1 {
                let fixed_length = params
                    .dimfxlon
                    .then_some(params.dimfxl.max(0.0) as f64);
                let points = d.leader_polyline(
                    (params.arrow_len * 2.0) as f64,
                    params.dimexo as f64,
                    fixed_length,
                );
                for pair in points.windows(2) {
                    let start = lv(pair[0]);
                    let end = lv(pair[1]);
                    if (end - start).length_squared() > 1e-12 {
                        add_segment(&mut g.ext_lines, start, end);
                    }
                }
            }
        }
        Dimension::Arc(d) => {
            let explicit_sweep = arc_dimension_angles(d);
            append_angular_dimension(
                &mut g,
                lv(d.center_point),
                lv(d.first_extension_point),
                lv(d.second_extension_point),
                lv(d.definition_point),
                arrow1,
                arrow2,
                explicit_sweep,
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
            let axis = normalized_or(chord - override_center, Vec3::X);
            let available = chord.distance(override_center);
            let arrows_outside = if params.ticks || params.arrow_len <= 1.0e-6 {
                false
            } else if available < params.arrow_len {
                true
            } else if available < params.text_width + params.arrow_len {
                if params.dimtix {
                    true
                } else {
                    match params.dimatfit {
                        0 | 1 => true,
                        2 => false,
                        _ => params.text_width <= available,
                    }
                }
            } else {
                false
            };
            let true_center = lv(d.definition_point);
            let radius = chord.distance(true_center);
            let text_outside = params.text_position.distance(true_center) > radius + 1.0e-5;
            let draw_inside_line = !arrows_outside || params.dimtofl;
            if draw_inside_line && !suppress.dim2 {
                add_segment_with_text_break(&mut g.dim_lines, chord, near, params.text_break);
                add_segment_with_text_break(&mut g.dim_lines, near, far, params.text_break);
                add_segment_with_text_break(
                    &mut g.dim_lines,
                    far,
                    override_center,
                    params.text_break,
                );
            }
            if arrows_outside && !suppress.dim2 {
                add_segment(
                    &mut g.dim_lines,
                    chord,
                    chord + axis * (params.arrow_len * 2.0),
                );
            }
            if !suppress.dim2 {
                append_arrow(
                    &mut g,
                    chord,
                    if arrows_outside { axis } else { -axis },
                    arrow1,
                );
            }
            if text_outside && params.text_movement == 0 && !suppress.dim2 {
                add_segment_with_text_break(
                    &mut g.dim_lines,
                    chord,
                    params.text_position,
                    params.text_break,
                );
            }
            if arrows_outside || text_outside {
                append_center_mark(&mut g, true_center, params.dimcen, radius);
            }
        }
    }
    g
}

fn add_segment_with_text_break(
    points: &mut Vec<[f32; 3]>,
    start: Vec3,
    end: Vec3,
    text_break: Option<TextBreak>,
) {
    let segment = end - start;
    let length = segment.length();
    if length <= 1.0e-6 {
        return;
    }
    let Some(text_break) = text_break else {
        add_segment(points, start, end);
        return;
    };
    let segment_curve = cadkernel::geom2d::Curve::Line(cadkernel::geom2d::Line {
        start: [start.x as f64, start.y as f64],
        end: [end.x as f64, end.y as f64],
    });
    let tolerance = cadkernel::geom2d::Tolerance::new((length as f64 * 1.0e-9).max(1.0e-9));
    let inside = |padding: f32| {
        let (sin, cos) = text_break.rotation.sin_cos();
        let x_axis = Vec3::new(cos as f32, sin as f32, 0.0);
        let y_axis = Vec3::new(-sin as f32, cos as f32, 0.0);
        let half_width = text_break.half_width + padding;
        let half_height = text_break.half_height + padding;
        let corners = [
            text_break.center - x_axis * half_width - y_axis * half_height,
            text_break.center + x_axis * half_width - y_axis * half_height,
            text_break.center + x_axis * half_width + y_axis * half_height,
            text_break.center - x_axis * half_width + y_axis * half_height,
        ];
        let boundary: Vec<_> = (0..4)
            .map(|index| {
                let next = (index + 1) % 4;
                cadkernel::geom2d::Curve::Line(cadkernel::geom2d::Line {
                    start: [corners[index].x as f64, corners[index].y as f64],
                    end: [corners[next].x as f64, corners[next].y as f64],
                })
            })
            .collect();
        cadkernel::geom2d::inside_spans(&boundary, &segment_curve, tolerance)
    };
    if inside(0.0).is_empty() {
        add_segment(points, start, end);
        return;
    }
    let mut cursor = 0.0_f32;
    for [cut_start, cut_end] in inside(text_break.padding.max(0.0)) {
        let cut_start = (cut_start as f32).clamp(0.0, 1.0);
        let cut_end = (cut_end as f32).clamp(0.0, 1.0);
        if cut_start - cursor > 1.0e-6 {
            add_segment(points, start.lerp(end, cursor), start.lerp(end, cut_start));
        }
        cursor = cursor.max(cut_end);
    }
    if 1.0 - cursor > 1.0e-6 {
        add_segment(points, start.lerp(end, cursor), end);
    }
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
    // first: 0=both, 1=arrows, 2=text, 3=best fit. The text is measured along
    // the dimension line where it is drawn: horizontal text on a vertical
    // dimension takes up its height, and text dragged toward one end runs into
    // that arrow long before the sum of widths says the gap is full.
    let gap = (d2 - d1).length();
    let text_span = params.text_break.map(|text_break| {
        let axis_angle = dir_d1_to_d2.y.atan2(dir_d1_to_d2.x) as f64;
        let (sin, cos) = (text_break.rotation - axis_angle).sin_cos();
        let half = (text_break.half_width as f64 * cos).abs()
            + (text_break.half_height as f64 * sin).abs()
            + text_break.padding as f64;
        let along = (text_break.center - d1).dot(dir_d1_to_d2) as f64;
        (along - half, along + half)
    });
    let text_extent = text_span
        .map(|(lo, hi)| (hi - lo) as f32)
        .unwrap_or(params.text_width);
    // Horizontal text beside a non-horizontal dimension is reached by a leg
    // and a hook instead of the dimension line itself.
    let hooks_out_horizontally =
        params.horizontal_text && !params.ticks && dir_d1_to_d2.y.abs() > 1e-3;
    let text_hits_an_arrow = text_span.is_some_and(|(lo, hi)| {
        let center = (lo + hi) * 0.5;
        center > 0.0
            && center < gap as f64
            && (lo < params.arrow_len as f64 || hi > (gap - params.arrow_len) as f64)
    });
    let arrows_outside = if params.ticks || params.arrow_len <= 1e-6 {
        false
    } else if gap < 2.0 * params.arrow_len || text_hits_an_arrow {
        true
    } else if gap < text_extent + 2.0 * params.arrow_len {
        match params.dimatfit {
            0 | 1 => true,
            2 => false,
            _ => text_extent <= gap,
        }
    } else {
        false
    };

    let line_dir = normalized_or(d2_out - d1_out, axis);
    let line_len = (d2_out - d1_out).length();
    let split = params
        .text_break
        .map(|text_break| (text_break.center - d1_out).dot(line_dir))
        .filter(|along| *along > 0.0 && *along < line_len)
        .unwrap_or(line_len * 0.5);
    let split_point = d1_out + line_dir * split;
    let draw_inside_line = !arrows_outside || params.dimtofl;
    if draw_inside_line && !suppress.dim1 && split > 1e-6 {
        add_segment_with_text_break(&mut g.dim_lines, d1_out, split_point, params.text_break);
    }
    if draw_inside_line && !suppress.dim2 && line_len - split > 1e-6 {
        add_segment_with_text_break(&mut g.dim_lines, split_point, d2_out, params.text_break);
    }
    // Outside the extension lines the dimension line runs two arrow lengths,
    // or on to the text when the text sits further out along the line.
    let reach = |default: f32, toward_text: Option<f32>| toward_text.map_or(default, |t| t.max(default));
    let stub = params.arrow_len * 2.0;
    let text_beyond_d1 = text_span
        .filter(|(_, hi)| *hi < 0.0 && !hooks_out_horizontally)
        .map(|(_, hi)| -hi as f32);
    let text_beyond_d2 = text_span
        .filter(|(lo, _)| *lo > gap as f64 && !hooks_out_horizontally)
        .map(|(lo, _)| (lo - gap as f64) as f32);
    if (arrows_outside && !params.dimsoxd) || text_beyond_d1.is_some() || text_beyond_d2.is_some() {
        if !suppress.dim1 && (arrows_outside || text_beyond_d1.is_some()) {
            add_segment(&mut g.dim_lines, d1 - dir_d1_to_d2 * reach(stub, text_beyond_d1), d1);
        }
        if !suppress.dim2 && (arrows_outside || text_beyond_d2.is_some()) {
            add_segment(&mut g.dim_lines, d2, d2 + dir_d1_to_d2 * reach(stub, text_beyond_d2));
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

    // Horizontal text outside the extension lines of a dimension that is not
    // horizontal itself: the dimension line runs on to the text's height and
    // turns a hook one arrow long toward the text, which sits against it.
    if params.horizontal_text && !params.ticks && dir_d1_to_d2.y.abs() > 1e-3 {
        if let Some(text_break) = params.text_break {
            let along = (text_break.center - d1).dot(dir_d1_to_d2);
            let outside = if along < 0.0 {
                Some((d1, -dir_d1_to_d2, suppress.dim1))
            } else if along > gap {
                Some((d2, dir_d1_to_d2, suppress.dim2))
            } else {
                None
            };
            if let Some((end, outward, suppressed)) = outside {
                let travel = (text_break.center.y - end.y) / outward.y;
                if travel > 0.0 && !suppressed {
                    let elbow = end + outward * travel;
                    let side = if text_break.center.x >= elbow.x { 1.0 } else { -1.0 };
                    let hook_end = Vec3::new(elbow.x + side * params.arrow_len, elbow.y, elbow.z);
                    add_segment(&mut g.dim_lines, end, elbow);
                    add_segment(&mut g.dim_lines, elbow, hook_end);
                }
            }
        }
    }
}

/// Leader from a point on the circle to text outside it. Horizontal text gets a
/// hook one arrow long that the text sits against; aligned text is reached by a
/// straight leader that stops a gap short of it. `text_width` already carries
/// a gap on each side.
fn append_radial_leader(g: &mut DimGeom, tip: Vec3, text: Vec3, params: &DimLineParams) {
    if params.horizontal_text {
        let side = if text.x >= tip.x { 1.0 } else { -1.0 };
        let (hook_start, text_edge) = match params.leader_anchor {
            Some(anchor) => (anchor, anchor + Vec3::X * (side * params.arrow_len)),
            None => {
                let edge = Vec3::new(text.x - side * params.text_width * 0.5, text.y, text.z);
                (edge - Vec3::X * (side * params.arrow_len), edge)
            }
        };
        let slope = (hook_start - tip).y.abs().atan2((hook_start - tip).x.abs());
        // A leader already within 15° of horizontal runs straight to the text.
        if slope > 15.0_f32.to_radians() {
            add_segment(&mut g.dim_lines, tip, hook_start);
            add_segment(&mut g.dim_lines, hook_start, text_edge);
        } else {
            add_segment(&mut g.dim_lines, tip, text_edge);
        }
    } else {
        let toward = normalized_or(text - tip, Vec3::X);
        let reach = ((text - tip).length() - params.text_width * 0.5).max(0.0);
        add_segment(&mut g.dim_lines, tip, tip + toward * reach);
    }
}

fn append_diameter_dimension(
    g: &mut DimGeom,
    chord: Vec3,
    far_chord: Vec3,
    text_anchor: Vec3,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
    params: DimLineParams,
    suppress: SuppressFlags,
) {
    let axis = normalized_or(far_chord - chord, Vec3::X);
    let diameter = chord.distance(far_chord);
    if diameter <= 1e-6 {
        return;
    }
    let center = (chord + far_chord) * 0.5;
    let text_is_outside = params.text_position.distance(center) > diameter * 0.5 + 1e-5;
    if text_is_outside && !params.dimtofl {
        // Outside text without DIMTOFL: no line across the circle, just a
        // leader from the near side with the arrowhead on the circle and its
        // body toward the text. DIMTMOVE 2 keeps the arrowhead and drops the
        // leader.
        let (tip, suppressed) = if params.text_position.distance_squared(chord)
            <= params.text_position.distance_squared(far_chord)
        {
            (chord, suppress.dim1)
        } else {
            (far_chord, suppress.dim2)
        };
        if !suppressed {
            if params.text_movement != 2 {
                append_radial_leader(g, tip, params.text_position, &params);
            }
            append_arrow(g, tip, normalized_or(tip - center, axis), arrow1);
        }
        return;
    }
    // Text projected outside the diameter requires inward-pointing arrowheads.
    let text_along = (params.text_position - chord).dot(axis);
    let text_outside = text_along < 0.0 || text_along > diameter;

    let arrows_outside = if params.ticks || params.arrow_len <= 1e-6 {
        false
    } else if text_outside {
        true
    } else if diameter < 2.0 * params.arrow_len {
        true
    } else if diameter < params.text_width + 2.0 * params.arrow_len {
        match params.dimatfit {
            0 | 1 => true,
            2 => false,
            _ => params.text_width <= diameter,
        }
    } else {
        false
    };

    let extension = if params.ticks { params.dimdle } else { 0.0 };
    let first = chord - axis * extension;
    let second = far_chord + axis * extension;
    let line_length = first.distance(second);
    let split = params
        .text_break
        .map(|text_break| (text_break.center - first).dot(axis))
        .filter(|along| *along > 0.0 && *along < line_length)
        .unwrap_or(line_length * 0.5);
    let split_point = first + axis * split;

    let draw_inside_line = text_outside || !arrows_outside || params.dimtofl;
    if draw_inside_line && !suppress.dim1 && split > 1e-6 {
        add_segment_with_text_break(&mut g.dim_lines, first, split_point, params.text_break);
    }
    if draw_inside_line && !suppress.dim2 && line_length - split > 1e-6 {
        add_segment_with_text_break(&mut g.dim_lines, split_point, second, params.text_break);
    }
    if arrows_outside && !params.dimsoxd {
        let stub = params.arrow_len * 2.0;
        if !suppress.dim1 {
            add_segment(&mut g.dim_lines, chord - axis * stub, chord);
        }
        if !suppress.dim2 {
            add_segment(&mut g.dim_lines, far_chord, far_chord + axis * stub);
        }
    }

    if arrows_outside {
        append_arrow(g, chord, -axis, arrow1);
        append_arrow(g, far_chord, axis, arrow2);
    } else {
        append_arrow(g, chord, axis, arrow1);
        append_arrow(g, far_chord, -axis, arrow2);
    }

    if params.text_movement == 0 {
        let (tip, direction, suppressed) = if params.text_position.distance_squared(chord)
            <= params.text_position.distance_squared(far_chord)
        {
            (chord, -axis, suppress.dim1)
        } else {
            (far_chord, axis, suppress.dim2)
        };
        if text_outside && !suppressed {
            let leader_length = (text_anchor - tip).dot(direction);

            if leader_length > 1e-6 {
                let text_extension = params.text_width * 0.5;
                add_segment(
                    &mut g.dim_lines,
                    tip,
                    tip + direction * (leader_length + text_extension),
                );
            }
        } else if text_along < 0.0 && !suppress.dim1 {
            add_segment(&mut g.dim_lines, chord, params.text_position);
        } else if text_along > diameter && !suppress.dim2 {
            add_segment(&mut g.dim_lines, far_chord, params.text_position);
        }
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

pub(crate) fn arc_dimension_angles(dimension: &DimensionArc) -> Option<(f32, f32)> {
    let raw = dimension.arc_end_parameter - dimension.arc_start_parameter;
    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
    if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
        sweep = std::f64::consts::TAU;
    }
    if sweep > 1.0e-12 {
        let start = dimension.arc_start_parameter as f32;
        return Some((start, start + sweep as f32));
    }

    let start = (dimension.first_extension_point.y - dimension.center_point.y)
        .atan2(dimension.first_extension_point.x - dimension.center_point.x);
    let end = (dimension.second_extension_point.y - dimension.center_point.y)
        .atan2(dimension.second_extension_point.x - dimension.center_point.x);
    let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
    (sweep > 1.0e-12).then_some((start as f32, (start + sweep) as f32))
}

pub(crate) fn arc_dimension_in_ocs(
    dimension: &DimensionArc,
) -> Option<(DimensionArc, Vector3)> {
    let length = dimension.base.normal.length();
    if !length.is_finite() || length <= 1.0e-12 {
        return None;
    }
    let normal = dimension.base.normal / length;
    if (normal - Vector3::UNIT_Z).length() <= 1.0e-12 {
        return None;
    }
    let normal_tuple = (normal.x, normal.y, normal.z);
    let to_ocs = |point: Vector3| {
        let point = crate::scene::view::transform::wcs_point_to_ocs(
            (point.x, point.y, point.z),
            normal_tuple,
        );
        Vector3::new(point.0, point.1, point.2)
    };
    let mut local = dimension.clone();
    local.center_point = to_ocs(dimension.center_point);
    let elevation = local.center_point.z;
    let on_plane = |point: Vector3| {
        let mut point = to_ocs(point);
        point.z = elevation;
        point
    };
    local.definition_point = on_plane(dimension.definition_point);
    local.first_extension_point = on_plane(dimension.first_extension_point);
    local.second_extension_point = on_plane(dimension.second_extension_point);
    local.first_leader_point = on_plane(dimension.first_leader_point);
    local.second_leader_point = on_plane(dimension.second_leader_point);
    local.base.definition_point = local.definition_point;
    local.base.text_middle_point = on_plane(dimension.base.text_middle_point);
    local.base.insertion_point = on_plane(dimension.base.insertion_point);
    local.base.normal = Vector3::UNIT_Z;
    Some((local, normal))
}

pub(crate) fn large_radial_dimension_in_ocs(
    dimension: &DimensionLargeRadial,
) -> Option<(DimensionLargeRadial, Vector3)> {
    let length = dimension.base.normal.length();
    if !length.is_finite() || length <= 1.0e-12 {
        return None;
    }
    let normal = dimension.base.normal / length;
    if (normal - Vector3::UNIT_Z).length() <= 1.0e-12 {
        return None;
    }
    let normal_tuple = (normal.x, normal.y, normal.z);
    let to_ocs = |point: Vector3| {
        let point = crate::scene::view::transform::wcs_point_to_ocs(
            (point.x, point.y, point.z),
            normal_tuple,
        );
        Vector3::new(point.0, point.1, point.2)
    };
    let mut local = dimension.clone();
    local.definition_point = to_ocs(dimension.definition_point);
    let elevation = local.definition_point.z;
    let on_plane = |point: Vector3| {
        let mut point = to_ocs(point);
        point.z = elevation;
        point
    };
    local.chord_point = on_plane(dimension.chord_point);
    local.override_center = on_plane(dimension.override_center);
    local.jog_point = on_plane(dimension.jog_point);
    local.base.definition_point = local.definition_point;
    local.base.text_middle_point = on_plane(dimension.base.text_middle_point);
    local.base.insertion_point = on_plane(dimension.base.insertion_point);
    local.base.normal = Vector3::UNIT_Z;
    Some((local, normal))
}

/// A linear or aligned dimension in a plane off world Z, moved into its own
/// OCS. Their builders read the offset and extension directions in XY, so
/// they draw it there and the result is mapped back onto the plane. `None`
/// for a dimension in world XY or of another kind.
pub(crate) fn linear_dimension_in_ocs(dimension: &Dimension) -> Option<(Dimension, Vector3)> {
    let length = dimension.base().normal.length();
    if !length.is_finite() || length <= 1.0e-12 {
        return None;
    }
    let normal = dimension.base().normal / length;
    if (normal - Vector3::UNIT_Z).length() <= 1.0e-12 {
        return None;
    }
    let normal_tuple = (normal.x, normal.y, normal.z);
    let to_ocs = |point: Vector3| {
        let point = crate::scene::view::transform::wcs_point_to_ocs(
            (point.x, point.y, point.z),
            normal_tuple,
        );
        Vector3::new(point.0, point.1, point.2)
    };
    let mut local = dimension.clone();
    match &mut local {
        Dimension::Linear(d) => {
            d.first_point = to_ocs(d.first_point);
            d.second_point = to_ocs(d.second_point);
            d.definition_point = to_ocs(d.definition_point);
            d.base.definition_point = d.definition_point;
            d.base.text_middle_point = to_ocs(d.base.text_middle_point);
            d.base.normal = Vector3::UNIT_Z;
        }
        Dimension::Aligned(d) => {
            d.first_point = to_ocs(d.first_point);
            d.second_point = to_ocs(d.second_point);
            d.definition_point = to_ocs(d.definition_point);
            d.base.definition_point = d.definition_point;
            d.base.text_middle_point = to_ocs(d.base.text_middle_point);
            d.base.normal = Vector3::UNIT_Z;
        }
        _ => return None,
    }
    Some((local, normal))
}

fn map_wire_ocs_to_wcs(wire: &mut WireModel, normal: Vector3) {
    let normal_tuple = (normal.x, normal.y, normal.z);
    let map = |x: f64, y: f64, z: f64| {
        crate::scene::view::transform::ocs_point_to_wcs((x, y, z), normal_tuple)
    };
    let map_split = |high: &mut Vec<[f32; 3]>, low: &mut Vec<[f32; 3]>| {
        let mut mapped_high = Vec::with_capacity(high.len());
        let mut mapped_low = Vec::with_capacity(high.len());
        for (index, point) in high.iter().enumerate() {
            if point[0].is_nan() {
                mapped_high.push(*point);
                mapped_low.push([0.0; 3]);
                continue;
            }
            let residual = low.get(index).copied().unwrap_or([0.0; 3]);
            let point = map(
                point[0] as f64 + residual[0] as f64,
                point[1] as f64 + residual[1] as f64,
                point[2] as f64 + residual[2] as f64,
            );
            let (xh, xl) = WireModel::split_ds(point.0);
            let (yh, yl) = WireModel::split_ds(point.1);
            let (zh, zl) = WireModel::split_ds(point.2);
            mapped_high.push([xh, yh, zh]);
            mapped_low.push([xl, yl, zl]);
        }
        *high = mapped_high;
        *low = mapped_low;
    };

    map_split(&mut wire.points, &mut wire.points_low);
    map_split(&mut wire.fill_tris, &mut wire.fill_tris_low);
    map_split(&mut wire.pick_tris, &mut wire.pick_tris_low);
    wire.text_verts = crate::scene::model::wire_model::map_text_verts(
        &wire.text_verts,
        map,
    );
    for (point, _) in &mut wire.snap_pts {
        let mapped = map(point.x, point.y, point.z);
        *point = DVec3::new(mapped.0, mapped.1, mapped.2);
    }
    for point in &mut wire.key_vertices {
        let mapped = map(point[0], point[1], point[2]);
        *point = [mapped.0, mapped.1, mapped.2];
    }
    if let Some(marker) = &mut wire.point_marker {
        let origin = map(marker.origin.x, marker.origin.y, marker.origin.z);
        let vector = |value: DVec3| {
            let mapped = map(value.x, value.y, value.z);
            DVec3::new(mapped.0, mapped.1, mapped.2)
        };
        marker.origin = DVec3::new(origin.0, origin.1, origin.2);
        marker.normal = vector(marker.normal).normalize_or(DVec3::Z);
        marker.axis_x = vector(marker.axis_x).normalize_or(DVec3::X);
        marker.axis_y = vector(marker.axis_y).normalize_or(DVec3::Y);
    }
    for tangent in &mut wire.tangent_geoms {
        match tangent {
            TangentGeom::Line { p1, p2 } => {
                let first = map(p1[0] as f64, p1[1] as f64, p1[2] as f64);
                let second = map(p2[0] as f64, p2[1] as f64, p2[2] as f64);
                *p1 = [first.0 as f32, first.1 as f32, first.2 as f32];
                *p2 = [second.0 as f32, second.1 as f32, second.2 as f32];
            }
            TangentGeom::Circle { center, radius } => {
                let center = map(
                    center[0] as f64,
                    center[1] as f64,
                    center[2] as f64,
                );
                let (x_axis, y_axis) = crate::scene::view::transform::ocs_axes(normal_tuple);
                *tangent = TangentGeom::PlanarCircle {
                    center: [center.0, center.1, center.2],
                    axis_x: [x_axis.0, x_axis.1, x_axis.2],
                    axis_y: [y_axis.0, y_axis.1, y_axis.2],
                    radius: *radius as f64,
                };
            }
            TangentGeom::PlanarCircle { center, axis_x, axis_y, .. }
            | TangentGeom::Arc { center, axis_x, axis_y, .. } => {
                let mapped = map(center[0], center[1], center[2]);
                let map_vector = |axis: [f64; 3]| {
                    let mapped = map(axis[0], axis[1], axis[2]);
                    [mapped.0, mapped.1, mapped.2]
                };
                *center = [mapped.0, mapped.1, mapped.2];
                *axis_x = map_vector(*axis_x);
                *axis_y = map_vector(*axis_y);
            }
            TangentGeom::PlanarEllipse {
                center,
                major_axis,
                normal,
                ..
            } => {
                let mapped = map(center[0], center[1], center[2]);
                let map_vector = |axis: [f64; 3]| {
                    let mapped = map(axis[0], axis[1], axis[2]);
                    [mapped.0, mapped.1, mapped.2]
                };
                *center = [mapped.0, mapped.1, mapped.2];
                *major_axis = map_vector(*major_axis);
                *normal = map_vector(*normal);
            }
        }
    }
    wire.aabb = WireModel::UNBOUNDED_AABB;
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
        Dimension::Arc(value) => {
            let vertex = vec3_local(value.center_point);
            let arc_point = vec3_local(value.definition_point);
            let (start, end) = arc_dimension_angles(value)?;
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
            add_segment_with_text_break(&mut g.dim_lines, a, b, params.text_break);
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

    if let Some(text_break) = params.text_break {
        let text_angle = (text_break.center.y - vertex.y).atan2(text_break.center.x - vertex.x);
        let into = (text_angle - start).rem_euclid(std::f32::consts::TAU);
        if into > delta.abs() + 1.0e-6 {
            let back_to_start = (start - text_angle).rem_euclid(std::f32::consts::TAU);
            let forward_from_end = (text_angle - end).rem_euclid(std::f32::consts::TAU);
            if back_to_start <= forward_from_end && !suppress.dim1 {
                append_sampled_arc_with_text_break(
                    &mut g.dim_lines,
                    vertex,
                    radius,
                    text_angle,
                    start,
                    params.text_break,
                );
            } else if !suppress.dim2 {
                append_sampled_arc_with_text_break(
                    &mut g.dim_lines,
                    vertex,
                    radius,
                    end,
                    text_angle,
                    params.text_break,
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

fn append_sampled_arc_with_text_break(
    lines: &mut Vec<[f32; 3]>,
    vertex: Vec3,
    radius: f32,
    start: f32,
    end: f32,
    text_break: Option<TextBreak>,
) {
    for pair in sample_angular_arc(vertex, radius, start, end - start).windows(2) {
        add_segment_with_text_break(lines, pair[0], pair[1], text_break);
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
    // Deviations are emitted by `dimension_tolerance_entity` at their own
    // height and alignment; keep the primary entity free of duplicate text.
    let (value, _) = dimension_text_parts(dim, style)?;
    // Use f64 position directly to avoid f32 round-trip precision loss at large
    // coordinates (e.g. Turkish UTM ~4,000,000 m). tessellate() will apply
    // world_offset when rendering this synthetic entity.
    let pos_f64 = dimension_text_pos_f64(dim, style, text_height, dim_scale);
    // A limits stack draws each half at DIMTFAC × DIMTXT, like the source application.
    let text_height = match style {
        Some(s) if s.dimlim => text_height * dimtfac_or_one(s),
        _ => text_height,
    };
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
    } else if matches!(dim, Dimension::Ordinate(_)) {
        dimension_text_natural_rotation(dim)
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
        if let Some(point) = stored_text_point(dim) {
            let text = vec3_local(point);
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
        let text_width = dimension_text_cells(dim, Some(style))
            .map(|cells| cells * height * CELL_WIDTH + gap * 2.0)
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
    if let Dimension::LargeRadial(radial) = dim {
        let radius = radial.definition_point.distance(&radial.chord_point);
        if let Some(point) = stored_text_point(dim) {
            return radial.definition_point.distance(&point) > radius + 1.0e-9;
        }
        if style.dimtix {
            return false;
        }
        let available = radial.override_center.distance(&radial.chord_point);
        let scale = if style.dimscale > 1.0e-9 {
            style.dimscale
        } else {
            1.0
        };
        let height = style.dimtxt * scale;
        let gap = style.dimgap.abs() * scale;
        let text_width = dimension_text_cells(dim, Some(style))
            .map(|cells| cells * height * CELL_WIDTH + gap * 2.0)
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
    if let Dimension::Radius(radius) = dim {
        let dx = radius.definition_point.x - radius.angle_vertex.x;
        let dy = radius.definition_point.y - radius.angle_vertex.y;
        let available = dx.hypot(dy);
        if let Some(text) = stored_text_point(dim) {
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
        let text_width = dimension_text_cells(dim, Some(style))
            .map(|cells| cells * height * CELL_WIDTH + gap * 2.0)
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
        Dimension::Diameter(d) => {
            let delta = d.definition_point - d.angle_vertex;
            let length = (delta.x * delta.x + delta.y * delta.y).sqrt().max(1e-12);
            (d.angle_vertex, d.definition_point, delta / length)
        }
        _ => return false,
    };
    let first_axis = first.x * axis.x + first.y * axis.y;
    let second_axis = second.x * axis.x + second.y * axis.y;
    let lo = first_axis.min(second_axis);
    let hi = first_axis.max(second_axis);
    if let Some(point) = stored_text_point(dim) {
        let position = point.x * axis.x + point.y * axis.y;
        return position < lo || position > hi;
    }
    if style.dimtix {
        return false;
    }
    let scale = if style.dimscale > 1e-9 { style.dimscale } else { 1.0 };
    let height = style.dimtxt * scale;
    let gap = style.dimgap.abs() * scale;
    let text_width = dimension_text_cells(dim, Some(style))
        .map(|cells| cells * height * CELL_WIDTH + gap * 2.0)
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

/// Where a dynamic dimension's lock mark sits: just after the text on its
/// baseline, with the direction that points away from the text.
pub(crate) fn dynamic_dimension_lock_anchor(
    document: &CadDocument,
    dim: &Dimension,
    anno_scale: f64,
) -> Option<(Vector3, Vector3)> {
    let style_name = &dim.base().style_name;
    let source_style = document.dim_styles.iter().find(|s| {
        s.name.eq_ignore_ascii_case(style_name)
            || (style_name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
    });
    let effective_style = source_style.map(|style| resolved_dimension_style(style, dim, document));
    let style = effective_style.as_ref();
    let dim_scale = style
        .map(|s| if s.dimscale > 1e-6 { s.dimscale } else { anno_scale })
        .unwrap_or(1.0);
    let text_height = style.map(|s| s.dimtxt * dim_scale).unwrap_or(2.5);
    let value = dimension_text_value(dim, style)?;
    let stack_scale = style.map(dimtfac_or_one).unwrap_or(1.0);
    let half_width = text_cells(&value, stack_scale) * text_height * CELL_WIDTH * 0.5;
    let pos = dimension_text_pos_f64(dim, style, text_height, dim_scale);
    // The text angle the renderer draws (DIMTIH/DIMTOH overrides make a
    // dynamic dimension's text horizontal).
    let (sr, cr) = dimension_text_rotation(dim, style).sin_cos();
    let outward = Vector3::new(cr, sr, 0.0);
    // A glyph-sized gap keeps the lock clear of the last digit.
    let reach = half_width + text_height * 0.9;
    Some((
        Vector3::new(pos.x + outward.x * reach, pos.y + outward.y * reach, pos.z),
        outward,
    ))
}

fn dimension_text_natural_rotation(dim: &Dimension) -> f64 {
    let angle = match dim {
        Dimension::Linear(d) => d.rotation,
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            dy.atan2(dx)
        }
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) | Dimension::Arc(_) => angular_dimension_frame(dim)
            .map(|(_, start, end, _)| {
                ((start + end) * 0.5 + std::f32::consts::FRAC_PI_2) as f64
            })
            .unwrap_or(0.0),
        Dimension::Radius(d) => (d.definition_point.y - d.angle_vertex.y)
            .atan2(d.definition_point.x - d.angle_vertex.x),
        Dimension::Diameter(d) => (d.definition_point.y - d.angle_vertex.y)
            .atan2(d.definition_point.x - d.angle_vertex.x),
        Dimension::LargeRadial(d) => (d.chord_point.y - d.override_center.y)
            .atan2(d.chord_point.x - d.override_center.x),
        Dimension::Ordinate(d) => {
            let axis_rotation = -d.base.horizontal_direction;
            if d.is_ordinate_type_x {
                axis_rotation + std::f64::consts::FRAC_PI_2
            } else {
                axis_rotation
            }
        }
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
    // so user_text "<>" substitution can re-use it. With DIMLIM the limits
    // stack is the value itself: the source application prints the upper limit over the
    // lower one and no nominal, so the tolerance slot stays empty.
    let value = match limits_text(dim, style, is_angular) {
        Some(limits) => limits,
        None if is_angular => format_angular_value(display_measurement(dim), style),
        None => format_linear_value(display_measurement(dim), style),
    };
    let primary_raw = match dim {
        Dimension::Radius(_) | Dimension::LargeRadial(_) => format!("R{}", value),
        // U+2205 is the native diameter sign and what `%%c` resolves to; the
        // bundled dimension fonts carry it where Latin Ø is missing.
        Dimension::Diameter(_) => format!("∅{}", value),
        _ => value,
    };

    // Build the deviation suffix separately so the caller can render it as
    // its own Text entity at DIMTFAC × DIMTXT height.
    let tolerance_suffix = build_tolerance_suffix(dim, style, is_angular);
    let primary = apply_dimpost(&primary_raw, style);

    // Alternate units appended in brackets when DIMALT is on (linear only).
    let primary = if !is_angular {
        match alternate_units_text(display_measurement(dim) * effective_dimlfac(style), style) {
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

/// The value the text reports. An angular dimension whose extension point
/// sits on its vertex has no measurable angle; the source application then keeps showing the
/// angle it stored (group 42, radians) rather than 0°, and so do we.
fn display_measurement(dim: &Dimension) -> f64 {
    let measured = dim.measurement();
    let stored = dim.base().actual_measurement;
    match dim {
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)
            if measured.abs() < 1e-9 && stored.abs() > 1e-9 =>
        {
            stored.to_degrees()
        }
        _ => measured,
    }
}

/// DIMLIM: the upper limit stacked over the lower one, replacing the nominal.
/// The offsets apply to the displayed number, so DIMLFAC and DIMRND come first.
fn limits_text(dim: &Dimension, style: Option<&DimStyle>, is_angular: bool) -> Option<String> {
    // the source application stacks the two limits even when both offsets are zero and the
    // halves read the same.
    let s = style.filter(|s| s.dimlim)?;
    let measurement = display_measurement(dim);
    if is_angular {
        return Some(format!(
            "\\S{}^{};",
            format_angular_value(measurement + s.dimtp, style),
            format_angular_value(measurement - s.dimtm, style)
        ));
    }
    let mut nominal = measurement * effective_dimlfac(style);
    if s.dimrnd > 1e-12 {
        nominal = (nominal / s.dimrnd).round() * s.dimrnd;
    }
    let (high, low) = limit_pair(nominal + s.dimtp, nominal - s.dimtm, s);
    Some(format!("\\S{}^{};", high, low))
}

/// The two limits keep the same number of decimals so the stack lines up:
/// DIMTZIN drops trailing zeros only as far as both values allow. the source application
/// writes `13.950` over `13.926` with DIMTZIN 8, not `13.95` over `13.926`.
fn limit_pair(high: f64, low: f64, s: &DimStyle) -> (String, String) {
    let dimtdec = s.dimtdec.max(0) as usize;
    let mut high = decimal_text(high, dimtdec);
    let mut low = decimal_text(low, dimtdec);
    if s.dimtzin & 8 != 0 {
        let droppable = |value: &str| match value.find('.') {
            Some(_) => value.len() - value.trim_end_matches('0').len(),
            None => 0,
        };
        let drop = droppable(&high).min(droppable(&low));
        for value in [&mut high, &mut low] {
            value.truncate(value.len() - drop);
            if value.ends_with('.') {
                value.pop();
            }
        }
    }
    let finish = |value: &str| {
        let value = if s.dimtzin & 4 != 0 {
            strip_leading_zero(value)
        } else {
            value.to_string()
        };
        swap_decimal_sep(&value, s.dimdsep)
    };
    (finish(&high), finish(&low))
}

/// Formats a tolerance number with DIMTDEC, DIMTZIN and DIMDSEP.
fn tolerance_formatter(s: &DimStyle) -> impl Fn(f64) -> String + '_ {
    let dimtdec = s.dimtdec.max(0) as usize;
    move |v: f64| {
        let raw = decimal_text(v, dimtdec);
        swap_decimal_sep(&apply_linear_zero_suppression(&raw, s.dimtzin), s.dimdsep)
    }
}

/// DIMTOL deviation suffix drawn beside the nominal. DIMLIM wins over DIMTOL,
/// as in the source application, and is handled by `limits_text`.
fn build_tolerance_suffix(
    dim: &Dimension,
    style: Option<&DimStyle>,
    is_angular: bool,
) -> Option<String> {
    let s = style.filter(|s| !s.dimlim)?;
    let fmt = tolerance_formatter(s);
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
            let raw = decimal_text(x * tolerance_factor, alttdec);
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
            let raw = decimal_text(x * tolerance_factor, alttdec);
            swap_decimal_sep(&apply_linear_zero_suppression(&raw, alttzin), s.dimdsep)
        };
        // Alternate limits stack like the primary ones.
        format!(
            "\\S{}^{};",
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
    let tol_height = primary_height * dimtfac_or_one(s);

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
    let primary_w = primary_value_len as f64 * primary_height * CELL_WIDTH;
    let tol_visible_chars = tol
        .strip_prefix("\\S")
        .and_then(|value| value.strip_suffix(';'))
        .map(|value| value.split('^').map(str::len).max().unwrap_or(0))
        .unwrap_or_else(|| tol.chars().count());
    let tol_w = tol_visible_chars as f64 * tol_height * CELL_WIDTH;
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

/// `resolved_dimension_style` has already applied the DXF sign convention,
/// so a negative value here means the caller passed an unresolved style;
/// fall back to 1.0 rather than reporting a negative length.
fn effective_dimlfac(style: Option<&DimStyle>) -> f64 {
    let lfac = style.map(|s| s.dimlfac).unwrap_or(1.0);
    if lfac.abs() < 1e-12 || lfac < 0.0 {
        1.0
    } else {
        lfac
    }
}

/// Average advance of one character cell as a fraction of the text height.
/// Measured on the bundled stroke fonts by rendering the text of 106 saved
/// dimension pictures: median 0.66, where the old guess of 0.60 left every
/// fit and hook estimate ten percent narrow.
const CELL_WIDTH: f64 = 0.66;

/// DIMTFAC with the unset value 0 read as 1.
fn dimtfac_or_one(s: &DimStyle) -> f64 {
    if s.dimtfac.abs() < 1e-12 {
        1.0
    } else {
        s.dimtfac
    }
}

/// Vertical extent of a dimension's whole value: the first line (or limits
/// stack) plus any `\P` continuation lines at MText's default spacing.
fn dimension_text_extent_height(dim: &Dimension, style: Option<&DimStyle>, text_height: f64) -> f64 {
    let lines = dimension_text_value(dim, style)
        .map(|value| text_lines(&value))
        .unwrap_or(1);
    dimension_text_block_height(style, text_height) + (lines - 1) as f64 * text_height * 5.0 / 3.0
}

/// Vertical extent of the rendered value: one line, or a DIMLIM stack whose
/// halves each sit at DIMTFAC × DIMTXT.
fn dimension_text_block_height(style: Option<&DimStyle>, text_height: f64) -> f64 {
    match style {
        Some(s) if s.dimlim => {
            // From the lower half's baseline to the upper half's cap, as the
            // MText stack lays them out.
            text_height
                * dimtfac_or_one(s)
                * f64::from(
                    crate::entities::text_support::STACK_RAISE
                        + crate::entities::text_support::STACK_HALF_SCALE,
                )
        }
        _ => text_height,
    }
}

/// Approximate width of the rendered value in character cells of the nominal
/// text height. A `\S` stack is as wide as its wider half, drawn at DIMTFAC.
pub(crate) fn dimension_text_cells(dim: &Dimension, style: Option<&DimStyle>) -> Option<f64> {
    let value = dimension_text_value(dim, style)?;
    Some(text_cells(&value, style.map(dimtfac_or_one).unwrap_or(1.0)))
}

/// Width in cells of the widest line; `\P` starts a new line.
fn text_cells(value: &str, stack_scale: f64) -> f64 {
    value
        .split(r"\P")
        .map(|line| line_cells(line, stack_scale))
        .fold(0.0, f64::max)
}

/// Number of lines the value renders as.
fn text_lines(value: &str) -> usize {
    value.split(r"\P").count().max(1)
}

/// Visible cells of one line: MText format codes (`\\f...;`, `\\H...;`, `\\C...;`,
/// braces, underline toggles) take no room, a stack is as wide as its wider half.
fn line_cells(value: &str, stack_scale: f64) -> f64 {
    let mut cells = 0.0;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('S') => {
                    let body: String = chars.by_ref().take_while(|c| *c != ';').collect();
                    let widest = body
                        .split(['^', '/', '#'])
                        .map(|half| half.chars().count())
                        .max()
                        .unwrap_or(0);
                    cells += widest as f64 * stack_scale;
                }
                Some('~') => cells += 1.0,
                // Underline, overline and strike toggles take no room; an
                // escaped backslash or brace is one visible character.
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') => {}
                Some('\\' | '{' | '}') => cells += 1.0,
                Some(_) => {
                    for c in chars.by_ref() {
                        if c == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            _ => cells += 1.0,
        }
    }
    cells
}

/// Format a linear measurement honouring DIMLFAC, DIMRND, DIMDEC, DIMZIN, DIMDSEP, DIMLUNIT.
fn format_linear_value(measurement: f64, style: Option<&DimStyle>) -> String {
    let (dec, zin, rnd, dsep, lunit, frac, sub_factor, sub_suffix) = style
        .map(|s| {
            (
                s.dimdec,
                s.dimzin,
                s.dimrnd,
                s.dimdsep,
                s.dimlunit,
                s.dimfrac,
                s.dimmzf,
                s.dimmzs.as_str(),
            )
        })
        .unwrap_or((4, 8, 0.0, 46, 2, 0, 1.0, ""));

    let lfac = effective_dimlfac(style);
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
        _ => decimal_text(value, dec),
    }
}

/// Fixed-point text rounded half away from zero. A few scaled ULPs settle
/// decimal ties without changing values at higher requested precision.
fn decimal_text(value: f64, decimals: usize) -> String {
    let scale = 10f64.powi(decimals.min(15) as i32);
    let scaled = value * scale;
    let tolerance = (scaled.abs().max(1.0) * f64::EPSILON * 4.0).min(0.25);
    format!(
        "{:.*}",
        decimals,
        (scaled + scaled.signum() * tolerance).round() / scale
    )
}

fn format_engineering(inches: f64, dec: usize) -> String {
    // Round to the printed precision *first*, then split, so inches that round
    // up to a full foot carry into the feet instead of printing an
    // out-of-range "2'-12.00"". Same reasoning as `format_architectural`,
    // where the grid is the fraction denominator rather than a decimal place.
    let scale = 10f64.powi(dec.min(15) as i32);
    let ticks = (inches.abs() * scale).round();
    let per_foot = 12.0 * scale;
    let sign = if inches < 0.0 && ticks != 0.0 { "-" } else { "" };
    let feet = (ticks / per_foot).trunc();
    let rem_in = (ticks - feet * per_foot) / scale;
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
        // 4 = Surveyor's units
        4 => format_surveyor(measurement_deg, adec, azin, decimal_separator),
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

/// Surveyor's units (DIMAUNIT 4): a bearing away from north or south toward
/// east or west, so the quoted angle never exceeds a quarter turn. Due north,
/// south, east and west have no bearing to quote and are written as the single
/// letter, which is also what keeps a quarter turn from reading as a
/// contradictory `N 90d0'0" E`. The bearing is DMS, so it carries DIMADEC,
/// DIMAZIN and DIMDSEP the same way the DMS format does.
fn format_surveyor(deg: f64, sec_dec: usize, azin: i16, decimal_separator: i16) -> String {
    // Quantise before choosing a cardinal or a quadrant, so a value a hair
    // under a cardinal direction does not print a quarter-turn bearing.
    let scale = 3600.0 * 10f64.powi(sec_dec.min(8) as i32);
    let turn = 360.0 * scale;
    let ticks = (deg.rem_euclid(360.0) * scale).round().rem_euclid(turn);
    let at = |d: f64| (ticks - d * scale).abs() < 0.5;
    if at(0.0) {
        return "E".into();
    }
    if at(90.0) {
        return "N".into();
    }
    if at(180.0) {
        return "W".into();
    }
    if at(270.0) {
        return "S".into();
    }
    let d = ticks / scale;
    // Measured from the nearer pole, toward the side the angle falls on.
    let (pole, bearing, side) = if d < 90.0 {
        ("N", 90.0 - d, "E")
    } else if d < 180.0 {
        ("N", d - 90.0, "W")
    } else if d < 270.0 {
        ("S", 270.0 - d, "W")
    } else {
        ("S", d - 270.0, "E")
    };
    format!(
        "{pole} {} {side}",
        format_dms(bearing, sec_dec, azin, decimal_separator)
    )
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
    horizontal_outside: bool,
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
            // Past the two-arrow stub the dimension line grows outside the
            // extension line, then the gap that `text_w` already carries.
            along = hi + 2.0 * arrow + text_w * 0.5;
            if horizontal_outside && ay.abs() > 1e-3 {
                // Horizontal text beside a hook: the line runs out two arrow
                // lengths, the hook turns one arrow toward the text-up side
                // and the text sits against it, centred on the hook.
                let elbow_along = hi + 2.0 * arrow;
                let side = if px >= 0.0 { 1.0 } else { -1.0 };
                let elbow_x = defpt.x + ax * elbow_along;
                let elbow_y = defpt.y + ay * elbow_along;
                let vertical = if tad == 0 { 0.0 } else { perp_off * perp_sign };
                return Vector3::new(
                    elbow_x + side * (arrow + text_w * 0.5),
                    elbow_y + vertical,
                    defpt.z,
                );
            }
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

fn text_on_single_arrow_dim_line(
    first: Vector3,
    second: Vector3,
    defpt: Vector3,
    ax: f64,
    ay: f64,
    perp_off: f64,
    text_width: f64,
    arrow: f64,
    dimtix: bool,
    dimatfit: i16,
    tad: i16,
) -> Vector3 {
    let (mut nx, mut ny) = (ax, ay);
    if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
        nx = -nx;
        ny = -ny;
    }
    let (perpendicular_x, perpendicular_y) = (-ny, nx);
    let perpendicular_sign = match tad {
        4 => -1.0,
        2 => {
            let offset = (defpt.x - first.x) * perpendicular_x
                + (defpt.y - first.y) * perpendicular_y;
            if offset >= 0.0 { 1.0 } else { -1.0 }
        }
        _ => 1.0,
    };
    let first_along = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
    let second_along = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;
    let low = first_along.min(second_along);
    let high = first_along.max(second_along);
    let available = high - low;
    let insufficient = text_width + arrow > available;
    let text_outside = !dimtix
        && insufficient
        && match dimatfit {
            0 | 2 => true,
            1 | 3 => text_width > available,
            _ => text_width > available,
        };
    let along = if text_outside {
        high + 2.0 * arrow + text_width * 0.5
    } else {
        (first_along + second_along) * 0.5
    };
    Vector3::new(
        defpt.x + ax * along + perpendicular_x * perp_off * perpendicular_sign,
        defpt.y + ay * along + perpendicular_y * perp_off * perpendicular_sign,
        defpt.z,
    )
}

/// The stored text middle point (group 11) applies to automatic and hand-placed
/// text alike. A regenerated dimension keeps any non-zero point; only a cleared
/// point is placed from the style.
fn stored_text_point(dim: &Dimension) -> Option<Vector3> {
    let p = dim.base().text_middle_point;
    (p.x * p.x + p.y * p.y + p.z * p.z > 1e-16).then_some(p)
}

/// The point on the circle a radial leader leaves from: the arc point of a
/// radius, or whichever end of a diameter's chord is nearer the text.
fn radial_leader_tip(dim: &Dimension, text: Vector3) -> Vector3 {
    match dim {
        Dimension::Radius(d) => d.definition_point,
        Dimension::Diameter(d) => {
            if d.angle_vertex.distance(&text) <= d.definition_point.distance(&text) {
                d.angle_vertex
            } else {
                d.definition_point
            }
        }
        _ => dim.base().text_middle_point,
    }
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
        dimension_text_extent_height(dim, style, text_height) * 0.5 + dimgap
    };
    // Rough text width + arrow allowance, used to decide text-outside fit.
    let text_w = dimension_text_cells(dim, style)
        .map(|cells| cells * text_height * CELL_WIDTH + 2.0 * dimgap)
        .unwrap_or(0.0);
    let arrow = style
        .map(|style| style.dimasz * dim_scale)
        .filter(|arrow| *arrow > 1e-9)
        .unwrap_or(text_height);
    // DIMTOH: text that lands outside reads horizontally.
    let horizontal_outside = style.is_some_and(|style| style.dimtoh);

    if let Some(point) = stored_text_point(dim) {
        return match dim {
            // DIMTMOVE 1 stores where the leader's hook starts: one arrow of
            // hook, a gap, then the text, all running away from the arc.
            // Measured on 39 such dimensions the stored point sits 2.4 text
            // heights before the text, which is DIMASZ plus DIMGAP.
            Dimension::Radius(_) | Dimension::Diameter(_)
                if style.is_some_and(|s| s.dimtmove == 1) =>
            {
                let tip = radial_leader_tip(dim, point);
                let side = if point.x >= tip.x { 1.0 } else { -1.0 };
                Vector3::new(point.x + side * (arrow + text_w * 0.5), point.y, point.z)
            }
            // A diameter whose text the user dragged keeps the grip point on
            // the leader and lifts the text by the DIMTAD offset (#1323).
            Dimension::Diameter(d) if base.text_user_positioned && dimtad != 0 => {
                let dx = d.definition_point.x - d.angle_vertex.x;
                let dy = d.definition_point.y - d.angle_vertex.y;
                let len = (dx * dx + dy * dy).sqrt().max(1e-12);
                // Match the readable orientation used by text_on_dim_line().
                let (mut nx, mut ny) = (dx / len, dy / len);
                if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
                    nx = -nx;
                    ny = -ny;
                }
                let (px, py) = (-ny, nx);
                let perp_sign = if dimtad == 4 { -1.0 } else { 1.0 };
                Vector3::new(
                    point.x + px * perp_off * perp_sign,
                    point.y + py * perp_off * perp_sign,
                    point.z,
                )
            }
            _ => point,
        };
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
                horizontal_outside,
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
                horizontal_outside,
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
        Dimension::LargeRadial(d) => {
            let dx = d.chord_point.x - d.override_center.x;
            let dy = d.chord_point.y - d.override_center.y;
            let length = dx.hypot(dy).max(1.0e-12);
            text_on_single_arrow_dim_line(
                d.override_center,
                d.chord_point,
                d.definition_point,
                dx / length,
                dy / length,
                perp_off,
                text_w,
                arrow,
                dimtix,
                dimatfit,
                dimtad,
            )
        }
        Dimension::Diameter(d) => {
            let dx = d.definition_point.x - d.angle_vertex.x;
            let dy = d.definition_point.y - d.angle_vertex.y;
            let len = (dx * dx + dy * dy).sqrt().max(1e-12);
            text_on_dim_line(
                d.angle_vertex,
                d.definition_point,
                d.angle_vertex,
                dx / len,
                dy / len,
                dimjust,
                perp_off,
                text_w,
                arrow,
                dimtix,
                dimatfit,
                dimtad,
                horizontal_outside,
            )
        }
        Dimension::Ordinate(d) => {
            let (x_axis, y_axis) = d.local_axes();
            let text_axis = if d.is_ordinate_type_x { y_axis } else { x_axis };
            let perpendicular = if d.is_ordinate_type_x { x_axis } else { y_axis };
            let delta = d.leader_endpoint - d.feature_location;
            let direction_sign = if delta.dot(&text_axis) < 0.0 { -1.0 } else { 1.0 };
            let perpendicular_sign = if delta.dot(&perpendicular) < 0.0 {
                -1.0
            } else {
                1.0
            };
            let vertical = match dimtad {
                0 => dimtvp * text_height,
                4 => -perp_off,
                _ => perp_off,
            };
            d.leader_endpoint
                + text_axis * (direction_sign * (text_w * 0.5 + dimgap))
                + perpendicular * (perpendicular_sign * vertical)
        }
        _ => {
            // Auto-placed text follows the style offset.
            let mid = match dim {
                Dimension::Angular2Ln(d) => d.dimension_arc,
                Dimension::Angular3Pt(d) => d.definition_point,
                Dimension::Arc(d) => d.definition_point,
                _ => base.text_middle_point,
            };
            Vector3::new(mid.x, mid.y + perp_off * perp_sign_default(), mid.z)
        }
    }
}

fn perp_sign_default() -> f64 {
    1.0
}

pub(crate) fn baked_large_radial_geometry(
    dimension: &Dimension,
    document: &CadDocument,
) -> Option<DimGeom> {
    if !matches!(dimension, Dimension::LargeRadial(_)) {
        return None;
    }
    let style_name = dimension.base().style_name.as_str();
    let effective_style = document
        .dim_styles
        .iter()
        .find(|style| {
            style.name.eq_ignore_ascii_case(style_name)
                || (style_name.trim().is_empty()
                    && style.name.eq_ignore_ascii_case("Standard"))
        })
        .map(|style| resolved_dimension_style(style, dimension, document));
    let style = effective_style.as_ref();
    let scale = style
        .map(|style| dimension_style_scale(style, 1.0))
        .unwrap_or(1.0);
    let text_height = style.map(|style| style.dimtxt * scale).unwrap_or(2.5 * scale);
    let arrow_size_raw = style.map(|style| style.dimasz * scale).unwrap_or(0.18 * scale);
    let tick_size = style.map(|style| style.dimtsz * scale).unwrap_or(0.0);
    let arrow_size = (arrow_size_raw as f32).max(0.001);
    let arrow = if tick_size > 1.0e-9 {
        ArrowKind::Tick {
            size: (tick_size as f32).max(0.001),
        }
    } else if let Some(style) = style {
        arrow_from_block(document, style.dimldrblk, arrow_size)
    } else {
        ArrowKind::Triangle {
            size: arrow_size,
            filled: true,
            size_mul: 1.0,
        }
    };
    let text = dimension_text_layout(dimension, style, text_height, scale);
    let mut geometry = dimension_geometry(
        dimension,
        &arrow,
        &arrow,
        DimLineParams {
            dimexo: style.map(|style| (style.dimexo * scale) as f32).unwrap_or(0.0),
            dimexe: style.map(|style| (style.dimexe * scale) as f32).unwrap_or(0.0),
            dimdle: style.map(|style| (style.dimdle * scale) as f32).unwrap_or(0.0),
            dimfxl: style.map(|style| (style.dimfxl * scale) as f32).unwrap_or(1.0),
            dimfxlon: style.is_some_and(|style| style.dimfxlon),
            dimsoxd: style.is_some_and(|style| style.dimsoxd),
            dimcen: style.map(|style| (style.dimcen * scale) as f32).unwrap_or(0.09),
            ticks: tick_size > 1.0e-9,
            arrow_len: arrow_size,
            text_width: text.width,
            dimatfit: style.map(|style| style.dimatfit).unwrap_or(3),
            dimtix: style.is_some_and(|style| style.dimtix),
            dimtofl: style.is_some_and(|style| style.dimtofl),
            text_position: text.position,
            horizontal_text: text.horizontal,
            text_movement: style.map(|style| style.dimtmove).unwrap_or(0),
            text_break: text.break_box,
            leader_anchor: None,
        },
        SuppressFlags {
            ext1: style.is_some_and(|style| style.dimse1),
            ext2: style.is_some_and(|style| style.dimse2),
            dim1: style.is_some_and(|style| style.dimsd1),
            dim2: style.is_some_and(|style| style.dimsd2),
        },
    );
    if !style.is_some_and(|style| style.dimse1) {
        if let Some(points) = crate::scene::dimension_assoc::radial_extension_points(
            document,
            dimension.base().common.handle,
            style.map(|style| style.dimexo * scale).unwrap_or(0.0),
            style.map(|style| style.dimexe * scale).unwrap_or(0.0),
        ) {
            let points: Vec<_> = points.into_iter().map(vec3_local).collect();
            add_polyline(&mut geometry.ext_lines, &points);
        }
    }
    if style.is_some_and(|style| style.dimtmove == 1) {
        if let Some((anchor, endpoint, under_text)) =
            dimtmove_leader_endpoints(dimension, text)
        {
            if under_text || anchor.distance(endpoint) > text_height as f32 {
                add_segment_with_text_break(
                    &mut geometry.dim_lines,
                    anchor,
                    endpoint,
                    text.break_box,
                );
            }
        }
    }
    apply_dimension_breaks(
        document,
        dimension.base().common.handle,
        &mut geometry.dim_lines,
    );
    apply_dimension_breaks(
        document,
        dimension.base().common.handle,
        &mut geometry.ext_lines,
    );
    Some(geometry)
}

/// Build measurement text for a saved dimension block.
pub(crate) fn baked_dimension_text_entity(
    dim: &Dimension,
    document: &CadDocument,
    anno_scale: f64,
) -> Option<EntityType> {
    let style_name = dim.base().style_name.as_str();
    let effective_style = document
        .dim_styles
        .iter()
        .find(|style| {
            style.name.eq_ignore_ascii_case(style_name)
                || (style_name.trim().is_empty()
                    && style.name.eq_ignore_ascii_case("Standard"))
        })
        .map(|style| resolved_dimension_style(style, dim, document));
    let style = effective_style.as_ref();
    let dim_scale = style
        .map(|style| dimension_style_scale(style, anno_scale))
        .unwrap_or(1.0);
    let dim_txt = style
        .map(|s| s.dimtxt * dim_scale)
        .unwrap_or(2.5 * dim_scale);
    let mut ent = dimension_text_entity(dim, dim_txt, style, document, dim_scale)?;
    if let EntityType::Text(t) = &mut ent {
        t.alignment_point = Some(t.insertion_point);
    }
    Some(ent)
}

pub(crate) fn baked_arc_length_symbol_points(
    dim: &Dimension,
    document: &CadDocument,
    anno_scale: f64,
) -> Vec<Vector3> {
    if !matches!(dim, Dimension::Arc(_)) {
        return Vec::new();
    }
    let style_name = dim.base().style_name.as_str();
    let style = document.dim_styles.iter().find(|style| {
        style.name.eq_ignore_ascii_case(style_name)
            || (style_name.trim().is_empty() && style.name.eq_ignore_ascii_case("Standard"))
    });
    let dim_scale = style
        .map(|style| {
            if style.dimscale > 1.0e-6 {
                style.dimscale
            } else {
                anno_scale
            }
        })
        .unwrap_or(1.0);
    let text_height = style
        .map(|style| style.dimtxt * dim_scale)
        .unwrap_or(2.5 * dim_scale);
    let symbol_position = crate::entities::dim_override::int(
        &dim.base().common.extended_data,
        crate::entities::dim_override::DIMARCSYM,
    )
    .or_else(|| style.map(|style| style.dimarcsym))
    .unwrap_or(0);

    arc_length_symbol_points(dim, style, text_height, dim_scale, symbol_position)
        .unwrap_or_default()
        .into_iter()
        .map(|point| Vector3::new(point.x as f64, point.y as f64, point.z as f64))
        .collect()
}

pub(crate) fn dimension_text_grip_position(
    dim: &Dimension,
    document: &CadDocument,
    anno_scale: f64,
) -> Option<Vector3> {
    // A stored text point is where the text is drawn; the grip goes there.
    if let Some(p) = stored_text_point(dim) {
        return Some(p);
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
        let fwd = text_on_dim_line(first, second, defpt, 1.0, 0.0, 0, perp, 0.0, 1.0, false, 3, 1, false);
        let rev = text_on_dim_line(first, second, defpt, -1.0, 0.0, 0, perp, 0.0, 1.0, false, 3, 1, false);
        assert!(fwd.y > 0.0, "above must be +Y, got {}", fwd.y);
        assert!(rev.y > 0.0, "above must be +Y even reversed, got {}", rev.y);
        let below = text_on_dim_line(first, second, defpt, 1.0, 0.0, 0, perp, 0.0, 1.0, false, 3, 4, false);
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
        false,
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
        false,
    );
        assert!(
            below.y < -10.0,
            "outside must clear the object side, got {}",
            below.y
        );
    }
}

#[cfg(test)]
mod angular_unit_tests {
    use super::format_angular_value;
    use acadrust::tables::DimStyle;

    fn style(dimaunit: i16, dimadec: i16) -> DimStyle {
        let mut s = DimStyle::standard();
        s.dimaunit = dimaunit;
        s.dimadec = dimadec;
        s
    }

    // DIMAUNIT 4 is Surveyor's units. It used to fall through to the decimal
    // branch, so an angular dimension in a drawing authored with surveyor
    // units read as plain degrees while AUNITS=4 elsewhere wrote a bearing.
    #[test]
    fn surveyor_units_write_a_bearing() {
        assert_eq!(format_angular_value(45.0, Some(&style(4, 0))), "N 45\u{b0}0'0\" E");
        assert_eq!(format_angular_value(135.0, Some(&style(4, 0))), "N 45\u{b0}0'0\" W");
        assert_eq!(format_angular_value(200.0, Some(&style(4, 0))), "S 70\u{b0}0'0\" W");
        assert_eq!(format_angular_value(300.0, Some(&style(4, 0))), "S 30\u{b0}0'0\" E");
        // A hair under a cardinal must not quote a quarter-turn bearing.
        assert_eq!(format_angular_value(89.99999, Some(&style(4, 0))), "N");
        assert_eq!(format_angular_value(90.0, Some(&style(4, 0))), "N");
        assert_eq!(format_angular_value(0.0, Some(&style(4, 0))), "E");
    }

    // The other unit formats are untouched.
    #[test]
    fn other_angular_units_unchanged() {
        assert_eq!(format_angular_value(45.5, Some(&style(1, 0))), "45\u{b0}30'0\"");
        assert_eq!(format_angular_value(90.0, Some(&style(2, 1))), "100.0g");
    }
}

#[cfg(test)]
mod dim_color_override_tests {
    use super::dim_color_index;
    use crate::entities::dim_override::{DIMCLRD, DIMCLRE, DIMCLRT};
    use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

    // An ACAD/DSTYLE record holding the overrides the Properties panel writes.
    fn xdata(overrides: &[(i16, i16)]) -> ExtendedData {
        let mut values = vec![
            XDataValue::String("DSTYLE".to_string()),
            XDataValue::ControlString("{".to_string()),
        ];
        for (code, aci) in overrides {
            values.push(XDataValue::Integer16(*code));
            values.push(XDataValue::Integer16(*aci));
        }
        values.push(XDataValue::ControlString("}".to_string()));
        let mut record = ExtendedDataRecord::new("ACAD");
        for value in values {
            record.add_value(value);
        }
        let mut xd = ExtendedData::new();
        xd.add_record(record);
        xd
    }

    // Every dim colour honours its per-object override, not just DIMCLRD.
    // Text and extension-line colours edited on the Properties panel used to
    // show there and render with the style/layer colour anyway. (#898)
    #[test]
    fn override_wins_over_style_for_every_dim_colour() {
        let xd = xdata(&[(DIMCLRD, 1), (DIMCLRE, 3), (DIMCLRT, 5)]);
        assert_eq!(dim_color_index(&xd, DIMCLRD, 256), 1);
        assert_eq!(dim_color_index(&xd, DIMCLRE, 256), 3);
        assert_eq!(dim_color_index(&xd, DIMCLRT, 256), 5);
    }

    // Only the overridden variable changes; the rest still inherit the style.
    #[test]
    fn absent_override_falls_back_to_the_style_value() {
        let xd = xdata(&[(DIMCLRT, 5)]);
        assert_eq!(dim_color_index(&xd, DIMCLRT, 256), 5);
        assert_eq!(dim_color_index(&xd, DIMCLRD, 7), 7);
        assert_eq!(dim_color_index(&xd, DIMCLRE, 256), 256);
    }

    // An entity with no override record at all is unaffected.
    #[test]
    fn no_record_uses_the_style_value() {
        let xd = ExtendedData::new();
        assert_eq!(dim_color_index(&xd, DIMCLRT, 2), 2);
    }

    // ByLayer (256) and ByBlock (0) stay meaningful as override values, so an
    // explicit "back to ByLayer" edit is not mistaken for "no override".
    #[test]
    fn bylayer_override_is_kept() {
        let xd = xdata(&[(DIMCLRT, 256)]);
        assert_eq!(dim_color_index(&xd, DIMCLRT, 5), 256);
    }
}

#[cfg(test)]
mod arch_format_tests {
    use super::{format_architectural, format_engineering, format_fractional};

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

    // Engineering units round to DIMDEC before the feet/inches split, so a
    // remainder that rounds up to 12" carries instead of printing "2'-12.00"".
    #[test]
    fn engineering_carries_inches_up_to_feet() {
        assert_eq!(format_engineering(35.999, 2), "3'-0.00\"");
        assert_eq!(format_engineering(11.999, 2), "1'-0.00\"");
        assert_eq!(format_engineering(11.9, 0), "1'-0\"");
    }

    #[test]
    fn engineering_normal_values_unchanged() {
        assert_eq!(format_engineering(30.5, 2), "2'-6.50\"");
        assert_eq!(format_engineering(36.0, 2), "3'-0.00\"");
        assert_eq!(format_engineering(0.0, 2), "0'-0.00\"");
        assert_eq!(format_engineering(-30.25, 2), "-2'-6.25\"");
    }

    // A magnitude that rounds away to zero must not keep a lone minus sign.
    #[test]
    fn engineering_negative_zero_has_no_sign() {
        assert_eq!(format_engineering(-0.001, 2), "0'-0.00\"");
    }
}

#[cfg(test)]
mod dimtmove_leader_tests {
    use super::*;
    use acadrust::entities::DimensionLinear;
    use acadrust::types::Vector3;

    fn v(x: f64, y: f64) -> Vector3 {
        Vector3::new(x, y, 0.0)
    }

    fn layout(position: Vec3, half_width: f32) -> DimensionTextLayout {
        DimensionTextLayout {
            position,
            width: half_width * 2.0,
            break_box: Some(TextBreak {
                center: position,
                half_width,
                half_height: 1.0,
                padding: 0.0,
                rotation: 0.0,
            }),
            horizontal: true,
        }
    }

    /// Text outside along the axis → the dim line extends underneath the text.
    #[test]
    fn text_outside_extends_dim_line_under_text() {
        let mut d = DimensionLinear::horizontal(v(0.0, 0.0), v(20.0, 0.0));
        d.definition_point = v(0.0, -5.0); // dimension line at y=-5
        let dim = Dimension::Linear(d);
        // text anchor outside on the right: centre x=30, half width 3 → far edge 33
        let txt = vec3_local(v(30.0, -3.0));
        let (anchor, end, under) = dimtmove_leader_endpoints(&dim, layout(txt, 3.0)).expect("leader");
        assert!(under, "outside text extends the dimension line");
        assert!((anchor.x - 20.0).abs() < 1e-4 && (anchor.y + 5.0).abs() < 1e-4, "near end should be the dim line's right end (20,-5), got {anchor:?}");
        assert!((end.x - 33.0).abs() < 1e-4 && (end.y + 5.0).abs() < 1e-4, "far end should be the text's far edge (33,-5), got {end:?}");
    }

    /// Text outside on the left → extends leftwards.
    #[test]
    fn text_outside_left_extends_leftwards() {
        let mut d = DimensionLinear::horizontal(v(0.0, 0.0), v(20.0, 0.0));
        d.definition_point = v(0.0, -5.0);
        let dim = Dimension::Linear(d);
        let txt = vec3_local(v(-12.0, -3.0));
        let (anchor, end, under) = dimtmove_leader_endpoints(&dim, layout(txt, 2.0)).expect("leader");
        assert!(under);
        assert!((anchor.x - 0.0).abs() < 1e-4, "near end should be the dim line's left end, got {anchor:?}");
        assert!((end.x + 14.0).abs() < 1e-4, "far end should be the text's far edge at -14, got {end:?}");
    }

    /// Text still inside (only moved up/down) → leader from the dim line to the text.
    #[test]
    fn text_inside_connects_to_text() {
        let mut d = DimensionLinear::horizontal(v(0.0, 0.0), v(20.0, 0.0));
        d.definition_point = v(0.0, -5.0);
        let dim = Dimension::Linear(d);
        let txt = vec3_local(v(10.0, 2.0));
        let (anchor, end, under) = dimtmove_leader_endpoints(&dim, layout(txt, 3.0)).expect("leader");
        assert!(!under, "inside → keep the connect-to-text leader (text break allowed)");
        assert!((anchor.x - 10.0).abs() < 1e-4 && (anchor.y + 5.0).abs() < 1e-4, "anchor should be the dim line point facing the text, got {anchor:?}");
        assert!((end.y - 2.0).abs() < 1e-4, "end should be the text position, got {end:?}");
    }

    /// Radius dimensions draw no leader (unchanged).
    #[test]
    fn radius_has_no_leader() {
        let r = acadrust::entities::DimensionRadius::new(v(0.0, 0.0), v(0.0, 5.0));
        assert!(dimtmove_leader_endpoints(&Dimension::Radius(r), layout(vec3_local(v(0.0, 8.0)), 2.0)).is_none());
    }

    #[test]
    fn extension_does_not_draw_through_text() {
        let mut d = DimensionLinear::horizontal(v(0.0, 0.0), v(20.0, 0.0));
        d.definition_point = v(0.0, -5.0);
        let text = layout(Vec3::new(30.0, -5.0, 0.0), 3.0);
        let (start, end, outside) = dimtmove_leader_endpoints(&Dimension::Linear(d), text).unwrap();
        assert!(outside);
        let mut points = Vec::new();
        add_segment_with_text_break(&mut points, start, end, text.break_box);
        let finite: Vec<_> = points.into_iter().filter(|point| point[0].is_finite()).collect();
        assert_eq!(finite, vec![[20.0, -5.0, 0.0], [27.0, -5.0, 0.0]]);
    }
}

#[cfg(test)]
mod linear_transform_tests {
    use super::*;
    use acadrust::entities::DimensionLinear;
    use std::f64::consts::FRAC_PI_2;

    /// A horizontal linear dimension measuring 10 units along X.
    fn horizontal() -> Dimension {
        let mut d =
            DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 5.0, 0.0);
        Dimension::Linear(d)
    }

    fn rotation(dim: &Dimension) -> f64 {
        match dim {
            Dimension::Linear(d) => d.rotation,
            _ => unreachable!("linear dimension expected"),
        }
    }

    /// The stored and the live value both still read 10.
    fn assert_measures_ten(dim: &Dimension) {
        let stored = dim.base().actual_measurement;
        assert!(
            (stored - 10.0).abs() < 1e-9,
            "stored measurement must stay 10, got {stored}"
        );
        let live = dim.measurement();
        assert!(
            (live - 10.0).abs() < 1e-9,
            "measurement must stay 10, got {live}"
        );
    }

    /// ROTATE in the drawing plane turns the measured axis with the points.
    /// Left at 0, a horizontal dimension turned 90° measured its now-vertical
    /// points along X and dropped to 0.
    #[test]
    fn rotate_turns_the_measured_axis() {
        let mut dim = horizontal();
        apply_transform(
            &mut dim,
            &EntityTransform::Rotate {
                center: DVec3::ZERO,
                axis: DVec3::Z,
                angle_rad: FRAC_PI_2,
            },
        );
        assert_measures_ten(&dim);
        let turned = rotation(&dim);
        assert!(
            (turned - FRAC_PI_2).abs() < 1e-9,
            "rotation must follow the rotate, got {turned}"
        );
    }

    /// MIRROR in the drawing plane reflects the measured axis. Mirrored about
    /// y = x, the horizontal dimension becomes a vertical one of the same value.
    #[test]
    fn mirror_reflects_the_measured_axis() {
        let mut dim = horizontal();
        apply_transform(
            &mut dim,
            &EntityTransform::Mirror {
                p1: DVec3::ZERO,
                p2: DVec3::new(1.0, 1.0, 0.0),
                working_normal: DVec3::Z,
            },
        );
        assert_measures_ten(&dim);
        let reflected = rotation(&dim);
        assert!(
            (reflected - FRAC_PI_2).abs() < 1e-9,
            "rotation must be reflected, got {reflected}"
        );
    }

    /// A zero-length mirror line leaves the points alone, so the axis too.
    #[test]
    fn degenerate_mirror_keeps_the_measured_axis() {
        let mut dim = horizontal();
        apply_transform(
            &mut dim,
            &EntityTransform::Mirror {
                p1: DVec3::new(3.0, 3.0, 0.0),
                p2: DVec3::new(3.0, 3.0, 0.0),
                working_normal: DVec3::Z,
            },
        );
        assert_eq!(rotation(&dim), 0.0);
        assert_measures_ten(&dim);
    }

    /// The point the dimension line passes through.
    fn definition_point(dim: &Dimension) -> DVec3 {
        let p = dimension_definition_point(dim);
        DVec3::new(p.x, p.y, p.z)
    }

    fn assert_near(actual: DVec3, expected: DVec3, what: &str) {
        assert!(
            actual.abs_diff_eq(expected, 1e-9),
            "{what}: expected {expected:?}, got {actual:?}"
        );
    }

    /// ROTATE about an axis off world Z (a working plane other than world XY)
    /// turns the dimension like the geometry it measures. It used to only move
    /// by where the origin went, leaving the dimension line and plane unturned.
    #[test]
    fn rotate_off_world_z_turns_the_dimension() {
        let center = DVec3::new(0.0, 10.0, 0.0);
        let turn = glam::DQuat::from_axis_angle(DVec3::X, FRAC_PI_2);
        let mut dim = horizontal();
        apply_transform(
            &mut dim,
            &EntityTransform::Rotate {
                center,
                axis: DVec3::X,
                angle_rad: FRAC_PI_2,
            },
        );
        assert_near(
            definition_point(&dim),
            turn * (DVec3::new(0.0, 5.0, 0.0) - center) + center,
            "definition point",
        );
        let normal = dim.base().normal;
        assert_near(
            DVec3::new(normal.x, normal.y, normal.z),
            turn * DVec3::Z,
            "normal",
        );
        assert_measures_ten(&dim);
    }

    /// MIRROR in a working plane off world Z reflects the dimension through
    /// the mirror plane instead of only moving it.
    #[test]
    fn mirror_off_world_z_reflects_the_dimension() {
        // Working plane YZ; the line runs along Z at y = 2, so the mirror
        // plane is y = 2.
        let mut dim = horizontal();
        apply_transform(
            &mut dim,
            &EntityTransform::Mirror {
                p1: DVec3::new(0.0, 2.0, 0.0),
                p2: DVec3::new(0.0, 2.0, 1.0),
                working_normal: DVec3::X,
            },
        );
        assert_near(
            definition_point(&dim),
            DVec3::new(0.0, -1.0, 0.0),
            "definition point",
        );
        assert_measures_ten(&dim);
    }

    /// Every stroke point the dimension draws, with its low residual added.
    fn stroke_points(dim: &Dimension) -> Vec<DVec3> {
        let document = CadDocument::new();
        let wires = dim.tessellate(
            &document,
            Handle::new(0x40),
            false,
            [1.0; 4],
            1.0,
            1.0,
            &rustc_hash::FxHashSet::default(),
            None,
            [0.0, 0.0, 0.0, 1.0],
            None,
            None,
        );
        let mut points = Vec::new();
        for wire in &wires {
            for (index, high) in wire.points.iter().enumerate() {
                if !high[0].is_finite() {
                    continue;
                }
                let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
                points.push(DVec3::new(
                    high[0] as f64 + low[0] as f64,
                    high[1] as f64 + low[1] as f64,
                    high[2] as f64 + low[2] as f64,
                ));
            }
        }
        points
    }

    /// Turned 90° about X into the XZ plane, the dimension has to draw the
    /// same strokes turned with it.
    fn assert_draws_turned_with_its_plane(flat: Dimension) {
        let turn = glam::DQuat::from_axis_angle(DVec3::X, FRAC_PI_2);
        let expected: Vec<DVec3> = stroke_points(&flat).into_iter().map(|p| turn * p).collect();
        assert!(!expected.is_empty(), "the dimension draws no strokes");
        let mut turned = flat;
        apply_transform(
            &mut turned,
            &EntityTransform::Rotate {
                center: DVec3::ZERO,
                axis: DVec3::X,
                angle_rad: FRAC_PI_2,
            },
        );
        let drawn = stroke_points(&turned);
        assert_eq!(drawn.len(), expected.len(), "stroke point count");
        for (index, (got, want)) in drawn.iter().zip(&expected).enumerate() {
            assert!(
                got.abs_diff_eq(*want, 1e-3),
                "point {index}: expected {want:?}, got {got:?}"
            );
        }
    }

    /// A linear dimension in a working plane off world Z draws in that plane.
    /// Its offset used to be read in world XY only, so once it ran along Z the
    /// dimension line fell onto the measured points and the extension lines
    /// stuck out along world Y.
    #[test]
    fn a_linear_dimension_off_world_z_draws_in_its_own_plane() {
        assert_draws_turned_with_its_plane(horizontal());
    }

    /// An aligned dimension draws through the same builder and plane.
    #[test]
    fn an_aligned_dimension_off_world_z_draws_in_its_own_plane() {
        let mut d = DimensionAligned::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(6.0, 8.0, 0.0));
        d.definition_point = Vector3::new(2.0, 11.0, 0.0);
        assert_draws_turned_with_its_plane(Dimension::Aligned(d));
    }
}


#[cfg(test)]
mod limits_format_tests {
    use super::*;
    use acadrust::entities::DimensionLinear;

    /// A horizontal linear dimension measuring 10 units along X.
    fn horizontal() -> Dimension {
        let mut d =
            DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 5.0, 0.0);
        Dimension::Linear(d)
    }

    fn style() -> DimStyle {
        let mut s = DimStyle::standard();
        s.dimdec = 3;
        s.dimtdec = 3;
        // Keep trailing zeros so the expectations read like the drawing.
        s.dimzin = 0;
        s.dimtzin = 0;
        s.dimtp = 0.02;
        s.dimtm = 0.05;
        s
    }

    // the source application's DIMLIM shows the upper limit over the lower one and nothing
    // else. The renderer used to keep the nominal and hang the stack beside
    // it, so a drawing with limits-only styles grew an extra number on every
    // regenerated dimension.
    #[test]
    fn limits_replace_the_nominal() {
        let mut s = style();
        s.dimlim = true;
        assert_eq!(
            dimension_text_parts(&horizontal(), Some(&s)),
            Some((r"\S10.020^9.950;".to_string(), None))
        );
    }

    // Stacked limits keep a common number of decimals: with DIMTZIN 8 the
    // zero in 10.020 stays because 9.965 has nothing to drop, while a pair
    // that both end in zero shortens together.
    #[test]
    fn limits_drop_trailing_zeros_only_together() {
        let mut s = style();
        s.dimlim = true;
        s.dimtzin = 8;
        s.dimtm = 0.035;
        assert_eq!(
            dimension_text_parts(&horizontal(), Some(&s)),
            Some((r"\S10.020^9.965;".to_string(), None))
        );
        s.dimtm = 0.08;
        assert_eq!(
            dimension_text_parts(&horizontal(), Some(&s)),
            Some((r"\S10.02^9.92;".to_string(), None))
        );
    }

    // The offsets apply to the number the reader sees, which for a paper
    // space dimension is the DIMLFAC-scaled distance (and DIMRND-rounded).
    #[test]
    fn limits_are_offsets_from_the_scaled_measurement() {
        let mut s = style();
        s.dimlim = true;
        s.dimlfac = 7.5;
        assert_eq!(
            dimension_text_parts(&horizontal(), Some(&s)),
            Some((r"\S75.020^74.950;".to_string(), None))
        );
        s.dimrnd = 0.5;
        s.dimtm = 0.0;
        assert_eq!(
            dimension_text_parts(&horizontal(), Some(&s)),
            Some((r"\S75.020^75.000;".to_string(), None))
        );
    }

    // the source application keeps the stack when both offsets are zero: production
    // drawings show `\S11.469^11.469;` for such dimensions.
    #[test]
    fn limits_without_offsets_still_stack() {
        let mut s = style();
        s.dimlim = true;
        s.dimtp = 0.0;
        s.dimtm = 0.0;
        assert_eq!(
            dimension_text_parts(&horizontal(), Some(&s)),
            Some((r"\S10.000^10.000;".to_string(), None))
        );
    }

    #[test]
    fn limits_take_precedence_over_deviation() {
        let mut s = style();
        s.dimlim = true;
        s.dimtol = true;
        let (_, suffix) = dimension_text_parts(&horizontal(), Some(&s)).unwrap();
        assert_eq!(suffix, None);
    }

    #[test]
    fn deviation_keeps_the_nominal_and_a_separate_suffix() {
        let mut s = style();
        s.dimtol = true;
        assert_eq!(
            dimension_text_parts(&horizontal(), Some(&s)),
            Some(("10.000".to_string(), Some(r"\S+0.020^-0.050;".to_string())))
        );
    }

    // A per-entity DSTYLE override turning limits off (group 72 = 0) is how
    // the reference callouts in a limits-style drawing show a plain value.
    #[test]
    fn limits_off_shows_the_plain_value() {
        let s = style();
        assert_eq!(
            dimension_text_parts(&horizontal(), Some(&s)),
            Some(("10.000".to_string(), None))
        );
    }

    #[test]
    fn user_text_substitutes_the_stack_for_the_measurement() {
        let mut s = style();
        s.dimlim = true;
        let mut dim = horizontal();
        dim.base_mut().text = "<> REF".to_string();
        let (value, _) = dimension_text_parts(&dim, Some(&s)).unwrap();
        assert_eq!(value, r"\S10.020^9.950; REF");
    }

    // Fit decisions size the text by its visible cells: a stack is as wide as
    // its wider half, drawn at DIMTFAC, not as wide as its control codes.
    #[test]
    fn text_cells_measure_the_wider_stack_half() {
        assert_eq!(text_cells("10.000", 0.5), 6.0);
        assert_eq!(text_cells(r"R\S1.020^0.98;", 0.5), 1.0 + 5.0 * 0.5);
        assert_eq!(text_cells(r"\S10.020^9.950; REF", 1.0), 6.0 + 4.0);
        assert_eq!(text_cells(r"\S1.020^0.980;\PCutouts", 1.0), 7.0, "widest line, not the sum");
        assert_eq!(text_lines(r"4X <>\PNominal"), 2);
        assert_eq!(
            text_cells(r"{\fArial|b0|i1|c0|p34;Cutouts}", 1.0),
            7.0,
            "format codes and braces take no room"
        );
        assert_eq!(text_cells(r"\H0.7x;10\L Ref\l", 1.0), 6.0);
    }

    // A three-point angular dimension whose first extension point coincides
    // with the vertex measures nothing; the stored angle keeps the text.
    #[test]
    fn degenerate_angular_dimension_reports_its_stored_angle() {
        use acadrust::entities::DimensionAngular3Pt;
        let mut d = DimensionAngular3Pt::default();
        d.angle_vertex = Vector3::new(6.0, 4.85, 0.0);
        d.first_point = d.angle_vertex;
        d.second_point = Vector3::new(7.0, 4.85, 0.0);
        d.definition_point = Vector3::new(8.0, 5.4, 0.0);
        d.base.actual_measurement = std::f64::consts::FRAC_PI_4;
        let dim = Dimension::Angular3Pt(d);
        let mut s = style();
        s.dimadec = 0;
        assert_eq!(
            dimension_text_parts(&dim, Some(&s)),
            Some(("45°".to_string(), None))
        );
        s.dimlim = true;
        s.dimtp = 0.1;
        s.dimtm = 0.1;
        let (value, _) = dimension_text_parts(&dim, Some(&s)).unwrap();
        assert!(value.starts_with(r"\S45"), "{value}");
    }

    // the source application rounds a printed value half away from zero; Rust's formatter
    // rounds the binary value, which sits a hair below the decimal tie.
    #[test]
    fn decimal_text_rounds_half_away_from_zero() {
        assert_eq!(decimal_text(0.4255, 3), "0.426");
        assert_eq!(decimal_text(0.4085, 3), "0.409");
        assert_eq!(decimal_text(-0.4255, 3), "-0.426");
        assert_eq!(decimal_text(2.5, 0), "3");
        assert_eq!(decimal_text(0.1234, 3), "0.123");
        assert_eq!(decimal_text(0.0000000000004, 12), "0.000000000000");
    }

    // Alternate units multiply the displayed (DIMLFAC-scaled) value and stack
    // their limits like the primary ones.
    #[test]
    fn alternate_limits_stack_from_the_scaled_value() {
        let mut s = style();
        s.dimlim = true;
        s.dimlfac = 2.0;
        s.dimalt = true;
        s.dimaltf = 25.4;
        s.dimaltd = 2;
        s.dimalttd = 2;
        s.dimaltz = 0;
        s.dimalttz = 0;
        s.dimtp = 0.02;
        s.dimtm = 0.05;
        let (value, _) = dimension_text_parts(&horizontal(), Some(&s)).unwrap();
        assert_eq!(value, r"\S20.020^19.950; [\S508.51^506.73;]");
    }

    #[test]
    fn a_limits_stack_is_two_halves_tall() {
        let mut s = style();
        assert_eq!(dimension_text_block_height(Some(&s), 2.0), 2.0);
        s.dimlim = true;
        s.dimtfac = 0.5;
        let expected = 2.0
            * 0.5
            * f64::from(
                crate::entities::text_support::STACK_RAISE
                    + crate::entities::text_support::STACK_HALF_SCALE,
            );
        assert_eq!(dimension_text_block_height(Some(&s), 2.0), expected);
    }
}

#[cfg(test)]
mod layout_parity_tests {
    use super::*;
    use acadrust::entities::{DimensionDiameter, DimensionLinear, DimensionRadius};
    use glam::DVec3;

    /// A document whose Standard style has imperial sizes and horizontal text.
    fn document() -> CadDocument {
        let mut document = CadDocument::new();
        let style = document
            .dim_styles
            .get_mut("Standard")
            .expect("Standard style");
        style.dimasz = 0.18;
        style.dimtxt = 0.18;
        style.dimgap = 0.09;
        style.dimtih = true;
        style.dimtoh = true;
        style.dimtad = 0;
        style.dimdec = 2;
        document
    }

    fn style(document: &CadDocument) -> DimStyle {
        document.dim_styles.get("Standard").expect("Standard style").clone()
    }

    /// Stroke endpoints and arrowhead triangle count of the drawn dimension.
    fn drawn(document: &CadDocument, dim: &Dimension) -> (Vec<DVec3>, usize) {
        let wires = dim.tessellate(
            document,
            Handle::new(0x40),
            false,
            [1.0; 4],
            1.0,
            1.0,
            &rustc_hash::FxHashSet::default(),
            None,
            [0.0, 0.0, 0.0, 1.0],
            None,
            None,
        );
        let mut points = Vec::new();
        let mut fills = 0;
        for wire in &wires {
            for p in wire.points.iter().filter(|p| p[0].is_finite()) {
                points.push(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
            }
            fills += wire.fill_tris.len() / 3;
        }
        (points, fills)
    }

    fn vertical(top: f64) -> DimensionLinear {
        let mut d = DimensionLinear::default();
        d.first_point = Vector3::new(0.0, 0.0, 0.0);
        d.second_point = Vector3::new(0.0, top, 0.0);
        d.definition_point = Vector3::new(-0.5, top, 0.0);
        d.rotation = std::f64::consts::FRAC_PI_2;
        d
    }

    // Group 11 is authoritative whenever present, whether or not the user moved
    // the text.
    #[test]
    fn stored_text_point_places_automatic_text() {
        let document = document();
        let mut d = DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 5.0, 0.0);
        d.base.text_middle_point = Vector3::new(7.0, 5.2, 0.0);
        d.base.text_user_positioned = false;
        let dim = Dimension::Linear(d);
        let position = dimension_text_pos_f64(&dim, Some(&style(&document)), 0.18, 1.0);
        assert_eq!((position.x, position.y), (7.0, 5.2));
    }

    // Text pushed outside by a narrow gap sits past the two-arrow stub and a
    // gap, not directly against the arrowhead.
    #[test]
    fn outside_text_clears_the_stub_and_a_gap() {
        let document = document();
        let mut d = DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.2, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 1.0, 0.0);
        let dim = Dimension::Linear(d);
        let s = style(&document);
        let position = dimension_text_pos_f64(&dim, Some(&s), 0.18, 1.0);
        let half_width = dimension_text_cells(&dim, Some(&s)).unwrap() * 0.18 * CELL_WIDTH * 0.5;
        let text_start = position.x - half_width;
        let stub_end = 0.2 + 2.0 * 0.18;
        assert!(
            (text_start - (stub_end + 0.09)).abs() < 1e-6,
            "text starts at {text_start}, expected {}",
            stub_end + 0.09
        );
    }

    // Horizontal text near the top of a vertical dimension runs into the top
    // arrowhead, so both arrowheads flip outside and stubs grow past the
    // extension lines. Text in the middle leaves them inside.
    #[test]
    fn text_that_hits_an_arrow_moves_the_arrows_outside() {
        let document = document();
        let top = 1.12;
        let mut near_top = vertical(top);
        near_top.base.text_middle_point = Vector3::new(-0.5, 0.98, 0.0);
        let (points, _) = drawn(&document, &Dimension::Linear(near_top));
        assert!(
            points.iter().any(|p| p.y > top + 0.1) && points.iter().any(|p| p.y < -0.1),
            "arrows should be outside with stubs past both extension lines"
        );

        let mut centred = vertical(top);
        centred.base.text_middle_point = Vector3::new(-0.5, top * 0.5, 0.0);
        let (points, _) = drawn(&document, &Dimension::Linear(centred));
        assert!(
            points.iter().all(|p| p.y <= top + 0.01 && p.y >= -0.01),
            "centred text leaves the arrows inside"
        );
    }

    // A negative DIMLFAC scales only a paper-space dimension that measures
    // model space through a viewport. The association record carries the
    // applied factor; zero means the dimension reads its sheet distance.
    #[test]
    fn negative_dimlfac_follows_the_recorded_association_factor() {
        use acadrust::xdata::{ExtendedDataRecord, XDataValue};
        let document = CadDocument::new();
        let mut source = style(&document);
        source.dimlfac = -4.0;
        let mut d = DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(4.457, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 1.0, 0.0);
        let with_factor = |factor: f64| {
            let mut d = d.clone();
            let mut record = ExtendedDataRecord::new("ACAD_DIMASSOC_CALC_DIMLFAC");
            record.add_value(XDataValue::Real(factor));
            d.base.common.extended_data.add_record(record);
            Dimension::Linear(d)
        };
        let sheet = resolved_dimension_style(&source, &with_factor(0.0), &document);
        assert_eq!(sheet.dimlfac, 1.0, "no factor applied: sheet distance");
        let through_viewport = resolved_dimension_style(&source, &with_factor(-4.0), &document);
        assert_eq!(through_viewport.dimlfac, 4.0, "the recorded factor, made positive");
        let unknown = resolved_dimension_style(&source, &Dimension::Linear(d.clone()), &document);
        assert_eq!(unknown.dimlfac, 1.0, "model space without a record: unscaled");
        let invalid = resolved_dimension_style(&source, &with_factor(f64::NAN), &document);
        assert_eq!(invalid.dimlfac, 1.0, "invalid recorded factor: unscaled");
    }

    // DIMTMOVE 1: the stored point is where the hook starts. The hook runs
    // one arrow toward the text, and the text begins a gap beyond it.
    #[test]
    fn dimtmove_one_anchors_the_hook_start_at_the_stored_point() {
        let mut document = document();
        document.dim_styles.get_mut("Standard").unwrap().dimtmove = 1;
        let mut d = DimensionRadius::default();
        d.angle_vertex = Vector3::new(0.0, 0.0, 0.0);
        d.definition_point = Vector3::new(1.0, 0.0, 0.0);
        d.base.text_middle_point = Vector3::new(3.0, 1.5, 0.0);
        let dim = Dimension::Radius(d);
        let (points, _) = drawn(&document, &dim);
        let hook: Vec<f64> = points.iter().filter(|p| (p.y - 1.5).abs() < 1e-6).map(|p| p.x).collect();
        let (lo, hi) = (hook.iter().cloned().fold(f64::MAX, f64::min), hook.iter().cloned().fold(f64::MIN, f64::max));
        assert!((lo - 3.0).abs() < 1e-6 && (hi - 3.18).abs() < 1e-6, "hook from the stored point one arrow toward the text: {lo}..{hi}");
        let s = style(&document);
        let position = dimension_text_pos_f64(&dim, Some(&s), 0.18, 1.0);
        let half = dimension_text_cells(&dim, Some(&s)).unwrap() * 0.18 * CELL_WIDTH * 0.5;
        assert!((position.x - half - 0.09 - 3.18).abs() < 1e-6, "text starts a gap past the hook end, got {}", position.x - half);
    }

    // Text stored well past the second extension line of a horizontal
    // dimension: the dimension line runs out to the text, not just two arrow
    // lengths.
    #[test]
    fn outside_text_on_the_axis_pulls_the_line_out_to_it() {
        let document = document();
        let mut d = DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.8, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 2.0, 0.0);
        d.base.text_middle_point = Vector3::new(2.5, 2.0, 0.0);
        let (points, _) = drawn(&document, &Dimension::Linear(d));
        let furthest = points.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        assert!(
            furthest > 2.0 && furthest < 2.5,
            "line reaches the text's near edge, got {furthest}"
        );
    }

    // Horizontal text stored above a vertical dimension: the dimension line
    // runs up to the text's height and hooks one arrow toward it.
    #[test]
    fn horizontal_outside_text_gets_a_leg_and_a_hook() {
        let document = document();
        let top = 0.3;
        let mut d = vertical(top);
        d.base.text_middle_point = Vector3::new(-0.5 + 0.9, 1.0, 0.0);
        let (points, _) = drawn(&document, &Dimension::Linear(d));
        assert!(
            points.iter().any(|p| (p.y - 1.0).abs() < 1e-6 && (p.x + 0.5).abs() < 1e-6),
            "the dimension line reaches the text height at the extension line"
        );
        assert!(
            points.iter().any(|p| (p.y - 1.0).abs() < 1e-6 && (p.x - (-0.5 + 0.18)).abs() < 1e-6),
            "a hook one arrow long turns toward the text"
        );
    }

    // Radius text outside the arc: no line to the centre, a leader from the arc
    // point to a hook the text sits against, arrowhead body pointing outward.
    #[test]
    fn radius_text_outside_draws_a_hooked_leader_only() {
        let document = document();
        let mut d = DimensionRadius::default();
        d.angle_vertex = Vector3::new(0.0, 0.0, 0.0);
        d.definition_point = Vector3::new(1.0, 0.0, 0.0);
        d.base.text_middle_point = Vector3::new(3.0, 0.8, 0.0);
        let (points, fills) = drawn(&document, &Dimension::Radius(d));
        assert!(
            !points.iter().any(|p| p.length() < 1e-6),
            "no dimension line reaches the centre"
        );
        let hook: Vec<_> = points.iter().filter(|p| (p.y - 0.8).abs() < 1e-6).collect();
        assert!(hook.len() >= 2, "a horizontal hook at the text height: {hook:?}");
        assert!(
            points.iter().any(|p| p.x > 1.05 && p.x < 1.3 && p.y > 0.0),
            "arrowhead body lies outside the arc along the leader"
        );
        assert_eq!(fills, 1);
    }

    // Diameter text outside the circle without DIMTOFL: a single leader from
    // the near side, one arrowhead, and no line across the circle.
    #[test]
    fn diameter_text_outside_draws_one_leader_and_no_chord() {
        let document = document();
        let mut d = DimensionDiameter::default();
        d.angle_vertex = Vector3::new(-1.0, 0.0, 0.0);
        d.definition_point = Vector3::new(1.0, 0.0, 0.0);
        d.base.text_middle_point = Vector3::new(3.0, 0.8, 0.0);
        let (points, fills) = drawn(&document, &Dimension::Diameter(d));
        assert!(
            !points.iter().any(|p| p.x < -0.5),
            "nothing is drawn on the far side of the circle"
        );
        assert_eq!(fills, 1, "one arrowhead");
        assert!(points.iter().any(|p| (p.y - 0.8).abs() < 1e-6), "hook at the text height");
    }
}
