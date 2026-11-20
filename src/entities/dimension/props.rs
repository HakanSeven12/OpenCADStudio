// Property editing and transforms for dimensions.

use super::*;

pub(crate) fn base_props(
    base: &DimensionBase,
    measurement: f64,
) -> Vec<crate::scene::model::object::Property> {
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
            format!("{:.4}", measurement),
        ),
    ]
}

pub(crate) fn properties(dim: &Dimension) -> Vec<PropSection> {
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
                edit(t!("Leader Length").as_ref(), "leader_length", leader_length),
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
        Dimension::Linear(d) => Some((&d.base, d.rotation, d.ext_line_rotation)),
        Dimension::Aligned(d) => Some((
            &d.base,
            (d.second_point.y - d.first_point.y).atan2(d.second_point.x - d.first_point.x),
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
    // The measurement in display units: an angular one stores radians.
    let mut props = base_props(dim.base(), dim.measurement());
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
            props.push(edit_angle(
                t!("Rotation").as_ref(),
                "rotation",
                d.rotation.to_degrees(),
            ));
            props.push(edit(
                t!("Ext Rotation (deg)").as_ref(),
                "ext_line_rotation",
                d.ext_line_rotation.to_degrees(),
            ));
        }
        Dimension::Radius(d) => {
            props.extend(radius_like_props(d.angle_vertex, d.definition_point));
            props.push(edit(
                t!("Leader Length").as_ref(),
                "leader_length",
                d.leader_length,
            ));
        }
        Dimension::Diameter(d) => {
            props.extend(radius_like_props(d.angle_vertex, d.definition_point));
            props.push(edit(
                t!("Leader Length").as_ref(),
                "leader_length",
                d.leader_length,
            ));
        }
        Dimension::Angular2Ln(d) => {
            props.extend(angular_props(
                d.angle_vertex,
                d.first_point,
                d.second_point,
                d.definition_point,
            ));
            props.push(edit(
                t!("Arc X").as_ref(),
                "dimension_arc_x",
                d.dimension_arc.x,
            ));
            props.push(edit(
                t!("Arc Y").as_ref(),
                "dimension_arc_y",
                d.dimension_arc.y,
            ));
            props.push(edit(
                t!("Arc Z").as_ref(),
                "dimension_arc_z",
                d.dimension_arc.z,
            ));
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
            props.push(edit(
                t!("Origin X").as_ref(),
                "definition_x",
                d.definition_point.x,
            ));
            props.push(edit(
                t!("Origin Y").as_ref(),
                "definition_y",
                d.definition_point.y,
            ));
            props.push(edit(
                t!("Origin Z").as_ref(),
                "definition_z",
                d.definition_point.z,
            ));
            props.push(edit(
                t!("Feature X").as_ref(),
                "feature_x",
                d.feature_location.x,
            ));
            props.push(edit(
                t!("Feature Y").as_ref(),
                "feature_y",
                d.feature_location.y,
            ));
            props.push(edit(
                t!("Feature Z").as_ref(),
                "feature_z",
                d.feature_location.z,
            ));
            props.push(edit(
                t!("Leader X").as_ref(),
                "leader_x",
                d.leader_endpoint.x,
            ));
            props.push(edit(
                t!("Leader Y").as_ref(),
                "leader_y",
                d.leader_endpoint.y,
            ));
            props.push(edit(
                t!("Leader Z").as_ref(),
                "leader_z",
                d.leader_endpoint.z,
            ));
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
            props.push(ro(
                t!("Partial").as_ref(),
                "is_partial",
                d.is_partial.to_string(),
            ));
            props.push(ro(
                t!("Has Leader").as_ref(),
                "has_leader",
                d.has_leader.to_string(),
            ));
            props.push(edit(
                t!("Leader 1 X").as_ref(),
                "leader1_x",
                d.first_leader_point.x,
            ));
            props.push(edit(
                t!("Leader 1 Y").as_ref(),
                "leader1_y",
                d.first_leader_point.y,
            ));
            props.push(edit(
                t!("Leader 1 Z").as_ref(),
                "leader1_z",
                d.first_leader_point.z,
            ));
            props.push(edit(
                t!("Leader 2 X").as_ref(),
                "leader2_x",
                d.second_leader_point.x,
            ));
            props.push(edit(
                t!("Leader 2 Y").as_ref(),
                "leader2_y",
                d.second_leader_point.y,
            ));
            props.push(edit(
                t!("Leader 2 Z").as_ref(),
                "leader2_z",
                d.second_leader_point.z,
            ));
        }
        Dimension::LargeRadial(_) => unreachable!("handled by compact large-radial properties"),
    }
    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props,
    }]
}

pub(crate) fn linear_like_props(
    first: document::types::Vector3,
    second: document::types::Vector3,
    definition: document::types::Vector3,
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

pub(crate) fn radius_like_props(
    center: document::types::Vector3,
    point: document::types::Vector3,
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

pub(crate) fn angular_props(
    vertex: document::types::Vector3,
    first: document::types::Vector3,
    second: document::types::Vector3,
    definition: document::types::Vector3,
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

pub(crate) fn apply_base_prop(base: &mut DimensionBase, field: &str, value: &str) -> bool {
    match field {
        "text" | "user_text" | "text_override" => {
            set_dimension_text_override(
                base,
                if value.trim().is_empty() {
                    None
                } else {
                    Some(value.to_string())
                },
            );
            true
        }
        "style_name" => {
            base.style_name = value.to_string();
            reset_automatic_text_position(base);
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

pub(crate) fn assign_f64(value: &str, target: &mut f64) -> bool {
    let Some(v) = parse_f64(value) else {
        return false;
    };
    *target = v;
    true
}

/// Parse a value entered in DEGREES and store it as radians. Dimension angle
/// fields are kept in radians internally but shown/edited in degrees, matching
/// arc.rs / text.rs so the properties panel reads consistently.
pub(crate) fn assign_deg(value: &str, target: &mut f64) -> bool {
    let Some(v) = parse_f64(value) else {
        return false;
    };
    *target = v.to_radians();
    true
}

pub(crate) fn apply_geom_prop(dim: &mut Dimension, field: &str, value: &str) {
    let ordinate_auto_text = match dim {
        Dimension::Ordinate(ordinate) if !ordinate.base.text_user_positioned => {
            Some(dimension_text_pos_f64(dim, None, 2.5, 1.0, None))
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
    dim.base_mut().actual_measurement = dim.stored_measurement();
}

pub(crate) fn apply_linear_fields_aligned(d: &mut DimensionAligned, field: &str, value: &str) {
    if field == "rotation" {
        let Some(angle) = parse_f64(value).map(f64::to_radians) else {
            return;
        };
        let old_angle =
            (d.second_point.y - d.first_point.y).atan2(d.second_point.x - d.first_point.x);
        let delta = angle - old_angle;
        let origin_x = d.first_point.x;
        let origin_y = d.first_point.y;
        let rotate = |point: &mut document::types::Vector3| {
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

pub(crate) fn apply_linear_fields_linear(d: &mut DimensionLinear, field: &str, value: &str) {
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

pub(crate) fn apply_linear_common(
    first: &mut document::types::Vector3,
    second: &mut document::types::Vector3,
    definition: &mut document::types::Vector3,
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

pub(crate) fn apply_radius_fields(d: &mut DimensionRadius, field: &str, value: &str) {
    apply_radius_common(&mut d.angle_vertex, &mut d.definition_point, field, value);
    if field == "leader_length" {
        let _ = assign_f64(value, &mut d.leader_length);
    }
}

pub(crate) fn apply_diameter_fields(d: &mut DimensionDiameter, field: &str, value: &str) {
    apply_radius_common(&mut d.angle_vertex, &mut d.definition_point, field, value);
    if field == "leader_length" {
        let _ = assign_f64(value, &mut d.leader_length);
    }
}

pub(crate) fn apply_radius_common(
    center: &mut document::types::Vector3,
    point: &mut document::types::Vector3,
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

pub(crate) fn apply_angular2_fields(d: &mut DimensionAngular2Ln, field: &str, value: &str) {
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

pub(crate) fn apply_angular3_fields(d: &mut DimensionAngular3Pt, field: &str, value: &str) {
    apply_angular_common(
        &mut d.angle_vertex,
        &mut d.first_point,
        &mut d.second_point,
        &mut d.definition_point,
        field,
        value,
    );
}

pub(crate) fn apply_angular_common(
    vertex: &mut document::types::Vector3,
    first: &mut document::types::Vector3,
    second: &mut document::types::Vector3,
    definition: &mut document::types::Vector3,
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

pub(crate) fn apply_ordinate_fields(d: &mut DimensionOrdinate, field: &str, value: &str) {
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

pub(crate) fn apply_arc_fields(d: &mut DimensionArc, field: &str, value: &str) {
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

pub(crate) fn apply_large_radial_fields(d: &mut DimensionLargeRadial, field: &str, value: &str) {
    if field == "large_radial_rotation" {
        let Some(angle) = parse_f64(value).map(f64::to_radians) else {
            return;
        };
        let delta = angle - large_radial_rotation(d);
        let origin = d.definition_point;
        let rotate = |point: &mut document::types::Vector3| {
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

pub(crate) fn large_radial_rotation(d: &DimensionLargeRadial) -> f64 {
    let delta = d.chord_point - d.definition_point;
    if delta.x.abs() <= 1.0e-12 && delta.y.abs() <= 1.0e-12 {
        let visible = d.chord_point - d.override_center;
        visible.y.atan2(visible.x)
    } else {
        delta.y.atan2(delta.x)
    }
}

pub(crate) fn apply_transform(dim: &mut Dimension, t: &EntityTransform) {
    // A zero text point is the "placed automatically" sentinel, not a
    // location: moving it to wherever the origin lands left the text there.
    let auto_text = {
        let base = dim.base();
        let p = base.text_middle_point;
        !base.text_user_positioned && p.x == 0.0 && p.y == 0.0 && p.z == 0.0
    };
    crate::scene::view::transform::apply_standard_entity_transform(dim, t, |entity, p1, p2| {
        document::Entity::apply_transform(
            entity,
            &crate::scene::view::transform::reflection_about_xy_line(p1, p2),
        );
    });
    if auto_text {
        reset_automatic_text_position(dim.base_mut());
    }
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
pub(crate) fn dv3(v: &document::types::Vector3) -> glam::DVec3 {
    glam::DVec3::new(v.x, v.y, v.z)
}

pub(crate) fn set_v3(target: &mut document::types::Vector3, p: DVec3) {
    target.x = p.x;
    target.y = p.y;
    target.z = p.z;
}

pub(crate) fn translate_v3(target: &mut document::types::Vector3, d: DVec3) {
    target.x += d.x;
    target.y += d.y;
    target.z += d.z;
}

pub(crate) fn apply_to_v3(target: &mut document::types::Vector3, apply: &GripApply) {
    match apply {
        GripApply::Absolute(p) => set_v3(target, *p),
        GripApply::Translate(d) => translate_v3(target, *d),
    }
}
