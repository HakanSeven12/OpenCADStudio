use codec::entities::{
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

/// Recompute the fields a script cannot set: the base definition point and
/// the stored measurement, exactly as grip edits do.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn normalize_scripted_dimension(dim: &mut Dimension) {
    let definition_point = dimension_definition_point(dim);
    dim.base_mut().definition_point = definition_point;
    dim.base_mut().actual_measurement = dim.stored_measurement();
}

fn dimension_definition_point(dim: &Dimension) -> codec::types::Vector3 {
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

fn base_props(
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

fn linear_like_props(
    first: codec::types::Vector3,
    second: codec::types::Vector3,
    definition: codec::types::Vector3,
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
    center: codec::types::Vector3,
    point: codec::types::Vector3,
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
    vertex: codec::types::Vector3,
    first: codec::types::Vector3,
    second: codec::types::Vector3,
    definition: codec::types::Vector3,
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

fn apply_linear_fields_aligned(d: &mut DimensionAligned, field: &str, value: &str) {
    if field == "rotation" {
        let Some(angle) = parse_f64(value).map(f64::to_radians) else {
            return;
        };
        let old_angle =
            (d.second_point.y - d.first_point.y).atan2(d.second_point.x - d.first_point.x);
        let delta = angle - old_angle;
        let origin_x = d.first_point.x;
        let origin_y = d.first_point.y;
        let rotate = |point: &mut codec::types::Vector3| {
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
    first: &mut codec::types::Vector3,
    second: &mut codec::types::Vector3,
    definition: &mut codec::types::Vector3,
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
    center: &mut codec::types::Vector3,
    point: &mut codec::types::Vector3,
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
    vertex: &mut codec::types::Vector3,
    first: &mut codec::types::Vector3,
    second: &mut codec::types::Vector3,
    definition: &mut codec::types::Vector3,
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
        let rotate = |point: &mut codec::types::Vector3| {
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
    // A zero text point is the "placed automatically" sentinel, not a
    // location: moving it to wherever the origin lands left the text there.
    let auto_text = {
        let base = dim.base();
        let p = base.text_middle_point;
        !base.text_user_positioned && p.x == 0.0 && p.y == 0.0 && p.z == 0.0
    };
    crate::scene::view::transform::apply_standard_entity_transform(dim, t, |entity, p1, p2| {
        codec::Entity::apply_transform(
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
fn dv3(v: &codec::types::Vector3) -> glam::DVec3 {
    glam::DVec3::new(v.x, v.y, v.z)
}

fn set_v3(target: &mut codec::types::Vector3, p: DVec3) {
    target.x = p.x;
    target.y = p.y;
    target.z = p.z;
}

fn translate_v3(target: &mut codec::types::Vector3, d: DVec3) {
    target.x += d.x;
    target.y += d.y;
    target.z += d.z;
}

fn apply_to_v3(target: &mut codec::types::Vector3, apply: &GripApply) {
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

    let perpendicular_offset = (text.x - line_x) * px + (text.y - line_y) * py;

    Some(DimTextRelativePosition {
        along_fraction,
        perpendicular_offset,
        z_offset: text.z - defpt.z,
    })
}

fn restore_dim_text_relative_position(dim: &mut Dimension, saved: DimTextRelativePosition) {
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
    let off1 = (defpt.x - first.x) * px + (defpt.y - first.y) * py;
    let off2 = (defpt.x - second.x) * px + (defpt.y - second.y) * py;

    let p1 = DVec3::new(first.x + px * off1, first.y + py * off1, defpt.z);

    let p2 = DVec3::new(second.x + px * off2, second.y + py * off2, defpt.z);

    Some((p1 + p2) * 0.5)
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
                dv3(&dimension_text_pos_f64(self, None, 2.5, 1.0, None))
            }
        };
        match self {
            Dimension::Linear(d) => vec![
                square_grip(0, dv3(&d.first_point)),
                center_grip(1, dv3(&d.second_point)),
                center_grip(
                    2,
                    dimension_line_grip_position(self).unwrap_or_else(|| dv3(&d.definition_point)),
                ),
                center_grip(3, text),
            ],
            Dimension::Aligned(d) => vec![
                square_grip(0, dv3(&d.first_point)),
                center_grip(1, dv3(&d.second_point)),
                center_grip(
                    2,
                    dimension_line_grip_position(self).unwrap_or_else(|| dv3(&d.definition_point)),
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
            let old_text = dimension_text_pos_f64(self, None, 2.5, 1.0, None);
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
            Dimension::Arc(d) => {
                if d.has_leader {
                    6
                } else {
                    4
                }
            }
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

            // A radius turns about its centre the same way: the point on the
            // arc follows the text instead of the line kinking to reach it.
            if let Dimension::Radius(d) = self {
                let center = d.angle_vertex;
                let radius = d.measurement();
                let offset = d.base.text_middle_point - center;
                if offset.length_squared() > 1e-24 && radius > 1e-12 {
                    d.definition_point = center + offset.normalize() * radius;
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
        self.base_mut().actual_measurement = self.stored_measurement();
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
            // The caller sets DIMTAD = Above as an override; the text goes
            // back to automatic placement so DIMGAP and the text height set
            // how far above the line it sits, at any drawing scale. (#543)
            A::AboveDimLine if grip_id == text_grip => {
                let b = self.base_mut();
                b.text_middle_point = Vector3::new(0.0, 0.0, 0.0);
                b.text_user_positioned = false;
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
            Dimension::Arc(d) => {
                if d.has_leader {
                    6
                } else {
                    4
                }
            }
            Dimension::LargeRadial(_) => 4,
        };
        (grip_id == text_grip && matches!(action, A::RotateText)).then_some("Specify text rotation")
    }

    fn apply_grip_menu_value(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        if self.grip_menu_value_prompt(grip_id, action).is_some() && matches!(action, A::RotateText)
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

use codec::entities::{MText, Text};
use codec::tables::DimStyle;
use codec::types::{Color as AcadColor, Vector3};

pub(crate) fn resolved_dimension_style(
    source: &DimStyle,
    dimension: &Dimension,
    document: &Document,
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
        if let Some(record) = document
            .text_styles
            .iter()
            .find(|record| record.handle == value)
        {
            style.dimtxsty = record.name.clone();
        }
    }

    // The dimension's own annotation context decides whether it scales with
    // the annotation scale; the style only seeds new dimensions. A dimension
    // turned non-annotative under an annotative style uses its DIMSCALE.
    // An arc length dimension carries no context, so its style decides.
    // (#1917)
    if !matches!(dimension, Dimension::Arc(_)) {
        style.annotative =
            crate::scene::annotative::object_is_annotative(document, &dimension.base().common);
    }
    // An annotative dimension sizes its features by the annotation scale; the
    // DIMSCALE it still stores is inert.
    if style.annotative {
        style.dimscale = 0.0;
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
                codec::xdata::XDataValue::Real(factor) if factor.is_finite() => Some(*factor),
                _ => None,
            })
        })
}

/// Whether the dimension's owner is paper space, even without a Layout object.
pub(crate) fn dimension_in_paper_space(dimension: &Dimension, document: &Document) -> bool {
    let owner = dimension.base().common.owner_handle;
    if !owner.is_valid() {
        return false;
    }
    document.objects.values().any(|object| {
        matches!(object,
            codec::objects::ObjectType::Layout(layout)
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
    document: &Document,
) -> Vec<PropSection> {
    use crate::entities::dim_override as ov;

    let effective_style = resolved_dimension_style(style, dimension, document);
    let data = &dimension.base().common.extended_data;
    let real = |code, inherited| ov::real(data, code).unwrap_or(inherited);
    let int = |code, inherited| ov::int(data, code).unwrap_or(inherited);
    let string =
        |code, inherited: &str| ov::string(data, code).unwrap_or_else(|| inherited.to_string());
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
    let choice =
        |label: &str, field: &'static str, selected: &str, options: &[&str], editable: bool| {
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
    let tolerance_enabled = matches!(tolerance_display, "Symmetrical" | "Deviation" | "Limits");
    let alternate_tolerance_enabled = tolerance_enabled && dimalt;
    let dimzin = int(ov::DIMZIN, s.dimzin);
    let dimaltz = int(ov::DIMALTZ, s.dimaltz);
    let dimtzin = int(ov::DIMTZIN, s.dimtzin);
    let dimalttz = int(ov::DIMALTTZ, s.dimalttz);
    let annotative = s.annotative;

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
            overridden_text_style.is_some_and(|handle| record.handle == handle)
                || (overridden_text_style.is_none()
                    && record.name.eq_ignore_ascii_case(&text_style_name))
        })
        .is_none_or(|record| !record.has_fixed_height());

    let (dim_prefix, dim_suffix) = split_measurement_template(&string(ov::DIMPOST, &s.dimpost));
    let (alt_prefix, alt_suffix) = split_measurement_template(&string(ov::DIMAPOST, &s.dimapost));
    let decimal_separator = {
        let value = int(ov::DIMDSEP, s.dimdsep);
        let character = value as u8 as char;
        if value > 0 && !character.is_control() {
            character.to_string()
        } else {
            value.to_string()
        }
    };

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
    let linetype =
        |code, inherited| linetype_name(document, ov::handle(data, code).unwrap_or(inherited));
    let arrow_1 = if matches!(dimension, Dimension::Radius(_) | Dimension::LargeRadial(_)) {
        block_name(
            document,
            ov::handle(data, ov::DIMLDRBLK).unwrap_or(s.dimldrblk),
            "Closed filled",
        )
    } else if s.dimsah {
        arrow_name(ov::DIMBLK1, s.dimblk1, &s.dimblk1_name)
    } else {
        // Separate arrows off: both ends draw DIMBLK, as the renderer does.
        block_name(document, s.dimblk, &s.dimblk_name)
    };
    let arrow_2 = if s.dimsah {
        arrow_name(ov::DIMBLK2, s.dimblk2, &s.dimblk2_name)
    } else {
        block_name(document, s.dimblk, &s.dimblk_name)
    };
    // The standard arrowheads, then any user arrow block in use. (#1820)
    let arrow_options =
        crate::entities::arrowheads::options([&arrow_1, &arrow_2].map(String::as_str));
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
    let fill_color =
        ov::color(data, ov::DIMTFILLCLR).unwrap_or_else(|| AcadColor::from_index(s.dimtfillclr));
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
                property(t!("Fill color").as_ref(), "dim_text_fill_color", fill_value),
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
                    on(int(ov::DIMTOH, s.dimtoh as i16) == 0),
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
                    on(int(ov::DIMTIH, s.dimtih as i16) == 0),
                    &["On", "Off"],
                    dimtix
                        || matches!(
                            dimension,
                            Dimension::LargeRadial(_)
                                | Dimension::Angular2Ln(_)
                                | Dimension::Angular3Pt(_)
                        ),
                ),
                property(
                    t!("Text position X").as_ref(),
                    "text_x",
                    PropValue::EditText(format!("{:.4}", dimension.base().text_middle_point.x)),
                ),
                property(
                    t!("Text position Y").as_ref(),
                    "text_y",
                    PropValue::EditText(format!("{:.4}", dimension.base().text_middle_point.y)),
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
                    on(int(ov::DIMSOXD, s.dimsoxd as i16) == 0),
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
                let text_position = styled_dimension_text_position(dimension, s, 1.0, Some(document));
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
                    &["Preceding dimension text", "Above dimension text", "None"],
                    true,
                ),
            );
        }
    }

    if matches!(
        dimension,
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)
    ) {
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
        let by_order = |order: &'static [&'static str]| {
            move |property: &Property| {
                order
                    .iter()
                    .position(|field| *field == property.field)
                    .unwrap_or(order.len())
            }
        };
        if let Some(tolerances) = sections
            .iter_mut()
            .find(|section| section.title == t!("Tolerances").as_ref())
        {
            tolerances.props.retain(|property| {
                !property.field.starts_with("dim_alt_tolerance_")
                    && !matches!(
                        property.field,
                        "dim_tolerance_suppress_zero_feet" | "dim_tolerance_suppress_zero_inches"
                    )
            });
            tolerances.props.sort_by_key(by_order(&[
                "dim_tolerance_alignment",
                "dim_tolerance_display",
                "dim_tolerance_limit_lower",
                "dim_tolerance_limit_upper",
                "dim_tolerance_pos_vert",
                "dim_tolerance_precision",
                "dim_tolerance_suppress_leading_zeros",
                "dim_tolerance_suppress_trailing_zeros",
                "dim_tolerance_text_height",
            ]));
        }
        // An arc has no dimension line extension.
        if let Some(lines) = sections
            .iter_mut()
            .find(|section| section.title == t!("Lines & Arrows").as_ref())
        {
            lines
                .props
                .retain(|property| property.field != "dim_line_ext");
        }
        if let Some(fit) = sections
            .iter_mut()
            .find(|section| section.title == t!("Fit").as_ref())
        {
            fit.props.sort_by_key(by_order(&[
                "dim_line_forced",
                "dim_line_inside",
                "dim_scale_overall",
                "dim_fit",
                "dim_text_inside",
                "dim_text_movement",
            ]));
        }
        if let Some(units) = sections
            .iter_mut()
            .find(|section| section.title == t!("Primary Units").as_ref())
        {
            units.props.sort_by_key(by_order(&[
                "dim_decimal_separator",
                "dim_prefix",
                "dim_suffix",
                "dim_angle_suppress_leading_zeros",
                "dim_angle_suppress_trailing_zeros",
                "dim_angle_precision",
                "dim_angle_units",
            ]));
        }
        // The measurement reads at the angular precision, and automatic text
        // reports where it is drawn rather than an unset point.
        if let Some(text_section) = sections
            .iter_mut()
            .find(|section| section.title == t!("Text").as_ref())
        {
            let automatic = stored_text_point(dimension)
                .is_none()
                .then(|| styled_dimension_text_position(dimension, s, 1.0, Some(document)));
            for property in &mut text_section.props {
                match property.field {
                    "measurement" => {
                        // The row shows the number at the angular precision,
                        // without the text's degree sign.
                        let value = format_angular_value(
                            displayed_measurement(dimension, Some(s)),
                            Some(s),
                        );
                        property.value =
                            PropValue::ReadOnly(value.trim_end_matches('°').to_string());
                    }
                    "text_x" => {
                        if let Some(position) = automatic {
                            property.value = PropValue::EditText(format!("{:.4}", position.x));
                        }
                    }
                    "text_y" => {
                        if let Some(position) = automatic {
                            property.value = PropValue::EditText(format!("{:.4}", position.y));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if matches!(dimension, Dimension::Ordinate(_)) {
        for section in &mut sections {
            section
                .props
                .retain(|property| match section.title.as_str() {
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
                    title if title == t!("Fit").as_ref() => {
                        matches!(property.field, "dim_text_movement" | "dim_scale_overall")
                    }
                    _ => true,
                });
        }
        let dim_scale = if s.dimscale > 1e-6 { s.dimscale } else { 1.0 };
        let text_position = dimension_text_pos_f64(
            dimension,
            Some(s),
            real(ov::DIMTXT, s.dimtxt) * dim_scale,
            dim_scale,
            Some(document),
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
    document: Option<&Document>,
) -> Vector3 {
    let scale = dimension_style_scale(style, fallback_scale);
    dimension_text_pos_f64(dimension, Some(style), style.dimtxt * scale, scale, document)
}

pub(crate) fn materialize_large_radial_text_position(
    document: &mut Document,
    handle: Handle,
    value: &str,
) {
    if parse_f64(value).is_none() {
        return;
    }
    let Some(EntityType::Dimension(dimension)) = document.get_entity(handle) else {
        return;
    };
    if !matches!(&**dimension, Dimension::LargeRadial(_)) || dimension.base().text_user_positioned {
        return;
    }
    let source = document.dim_styles.iter().find(|style| {
        style
            .name
            .eq_ignore_ascii_case(&dimension.base().style_name)
            || (dimension.base().style_name.trim().is_empty()
                && style.name.eq_ignore_ascii_case("Standard"))
    });
    let position = source
        .map(|style| resolved_dimension_style(style, dimension, document))
        .map(|style| styled_dimension_text_position(dimension, &style, 1.0, Some(&*document)))
        .unwrap_or_else(|| dimension_text_pos_f64(dimension, None, 2.5, 1.0, None));
    if let Some(Dimension::LargeRadial(radial)) =
        document.get_entity_mut(handle).and_then(as_dimension_mut)
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

fn choice_value(selected: &str, options: &[&str]) -> crate::scene::model::object::PropValue {
    crate::scene::model::object::PropValue::Choice {
        selected: selected.to_string(),
        options: options.iter().map(|value| (*value).to_string()).collect(),
    }
}

/// The arrowhead picker's name for `handle`; `fallback` names a block the
/// drawing does not hold.
fn block_name(document: &Document, handle: codec::Handle, fallback: &str) -> String {
    if !handle.is_null() && !document.block_records.iter().any(|b| b.handle == handle) {
        return crate::entities::arrowheads::label_for_block(fallback);
    }
    crate::entities::arrowheads::label(document, handle)
}

fn linetype_name(document: &Document, handle: codec::Handle) -> String {
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

use codec::{Document, EntityType, Handle};

use crate::scene::convert::tess_util::aci_to_rgba;
use crate::scene::convert::tessellate::{
    add_polyline, add_segment, append_arrow, arrow_from_block,
    arrow_from_block_with_deferred_hatch, normalized_or, ArrowKind, DimGeom,
};
use crate::scene::model::wire_model::{SnapHint, TangentGeom, WireModel};

fn apply_dimension_breaks(
    document: &Document,
    dimension: Handle,
    lines: &mut Vec<[f32; 3]>,
    origin: Vector3,
) {
    let references: Vec<_> = document
        .objects
        .values()
        .filter_map(|object| {
            let codec::objects::ObjectType::DataObject(object) = object else {
                return None;
            };
            let codec::objects::DataObjectData::BreakData(data) = &object.data else {
                return None;
            };
            (data.dimension_reference == dimension).then_some(&data.point_references)
        })
        .flatten()
        .filter(|reference| {
            let points = [reference.first_point, reference.second_point];
            points
                .iter()
                .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
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
                let first = vec3_local(point_from_origin(reference.first_point, origin));
                let second = vec3_local(point_from_origin(reference.second_point, origin));
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
                    add_segment(&mut output, a + direction * start, a + direction * end);
                }
            }
        }
    }
    *lines = output;
}

/// The jog position stored on a dimension (WCS, full f64), if any.
fn dimension_jog_point(dimension: &Dimension) -> Option<Vector3> {
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
                codec::xdata::XDataValue::Point3D(point) => Some(*point),
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
    let coordinates = segments
        .iter()
        .map(|(_, segment)| *segment)
        .collect::<Vec<_>>();
    let Some((candidate, center)) =
        kernel::space::nearest_segment_point(&coordinates, requested.to_array().map(f64::from))
    else {
        return;
    };
    let target = segments[candidate].0;
    let Some(jog) = kernel::space::dimension_jog_points(
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
        if !output.last().is_some_and(|point| point[0].is_nan()) && !lines[target + 2][0].is_nan() {
            output.push([f32::NAN; 3]);
        }
        output.extend_from_slice(&lines[target + 2..]);
    }
    *lines = output;
}

pub trait DimensionTess {
    #[allow(clippy::too_many_arguments)]
    fn tessellate(
        &self,
        document: &Document,
        handle: Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        selected_set: &rustc_hash::FxHashSet<codec::Handle>,
        active_viewport: Option<codec::Handle>,
        bg_color: [f32; 4],
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        reference_only: &rustc_hash::FxHashSet<codec::Handle>,
    ) -> Vec<WireModel>;
}

impl DimensionTess for Dimension {
    #[allow(clippy::too_many_arguments)]
    fn tessellate(
        &self,
        document: &Document,
        handle: Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        selected_set: &rustc_hash::FxHashSet<codec::Handle>,
        active_viewport: Option<codec::Handle>,
        bg_color: [f32; 4],
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        reference_only: &rustc_hash::FxHashSet<codec::Handle>,
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
            reference_only,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn tessellate_dimension_inner(
    document: &Document,
    handle: Handle,
    dim: &Dimension,
    selected: bool,
    entity_color: [f32; 4],
    line_weight_px: f32,
    anno_scale: f32,
    // LOD hints — when present, synthesised dim text routes through the
    // top-level LOD ladder (baseline / greek / full) instead of the render
    // path so far-out drawings collapse to a colored rect or baseline.
    selected_set: &rustc_hash::FxHashSet<codec::Handle>,
    active_viewport: Option<codec::Handle>,
    bg_color: [f32; 4],
    view_aabb: Option<[f32; 4]>,
    world_per_pixel: Option<f32>,
    reference_only: &rustc_hash::FxHashSet<codec::Handle>,
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
                reference_only,
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
                reference_only,
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
            reference_only,
        );
        for wire in &mut wires {
            map_wire_ocs_to_wcs(wire, normal);
        }
        return wires;
    }
    let name = handle.value().to_string();
    // (Baked-block fast path moved up into scene::tessellate_entity so the
    // recursive call goes through the LOD ladder, not the kernel path.)

    // Dimension geometry is f32. Tessellate it in a local frame anchored at a
    // point of the dimension, then fold the origin back in as a double-single
    // residual, so dimensions stay glued to their measured points on
    // UTM-scale drawings instead of drifting by up to half a metre.
    let source_dim = dim;
    let origin = dimension_precision_origin(dim);
    let shifted = (origin != Vector3::ZERO).then(|| shift_dimension(dim, origin));
    let dim = shifted.as_ref().unwrap_or(dim);

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
            && crate::scene::render_graph::block_contains_hatch(document, name, &mut memo)
    };
    let (arrow1, arrow2) = if dimtsz_raw > 1e-9 {
        let t = ArrowKind::Tick {
            size: (dimtsz_raw as f32).max(0.001),
            width: 0.0,
        };
        (t.clone(), t)
    } else if let Some(s) = style {
        if matches!(dim, Dimension::Radius(_) | Dimension::LargeRadial(_)) {
            let handle = crate::entities::dim_override::handle(
                &dim.base().common.extended_data,
                crate::entities::dim_override::DIMLDRBLK,
            )
            .unwrap_or(s.dimldrblk);
            let arrow =
                arrow_from_block_with_deferred_hatch(document, handle, dimasz, defer_arrow_hatches);
            (arrow.clone(), arrow)
        } else if matches!(dim, Dimension::Diameter(_)) {
            let data = &dim.base().common.extended_data;
            let first =
                crate::entities::dim_override::handle(data, crate::entities::dim_override::DIMBLK1)
                    .unwrap_or(if dimsah { s.dimblk1 } else { s.dimblk });
            let second =
                crate::entities::dim_override::handle(data, crate::entities::dim_override::DIMBLK2)
                    .unwrap_or(if dimsah { s.dimblk2 } else { s.dimblk });
            (
                arrow_from_block_with_deferred_hatch(document, first, dimasz, defer_arrow_hatches),
                arrow_from_block_with_deferred_hatch(document, second, dimasz, defer_arrow_hatches),
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

    let text_layout = dimension_text_layout(dim, style, dim_txt, dim_scale, Some(document));
    let moved_text = dimtmove_one_text(dim, style, dim_txt as f64, dim_scale, Some(document));
    // Text out on its own leader leaves the dimension line as DIMTMOVE 2
    // does: no line chasing it, no turn toward it.
    let text_movement = if moved_text.as_ref().is_some_and(|moved| !moved.lines.is_empty()) {
        2
    } else {
        style.map(|s| s.dimtmove).unwrap_or(0)
    };

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
            // Oblique-stroke and architectural-tick arrow blocks are ticks
            // too: they sit on the extension line and need no room of their
            // own, and DIMDLE applies to them. Given arrow-sized room, an
            // 18-long dimension flipped its strokes outside (#898).
            ticks: dimtsz_raw > 1e-9
                || (matches!(arrow1, ArrowKind::Tick { .. })
                    && matches!(arrow2, ArrowKind::Tick { .. })),
            arrow_len: dimasz,
            text_width: text_layout.width,
            text_height: dim_txt as f32,
            constraint: dynamic_constraint_dimension(dim),
            dimatfit: style.map(|s| s.dimatfit).unwrap_or(3),
            dimtix: style.is_some_and(|s| s.dimtix),
            dimtofl: style.map(|s| s.dimtofl).unwrap_or(false),
            text_position: text_layout.position,
            horizontal_text: text_layout.horizontal,
            text_lifted: style.map(|s| s.dimtad).unwrap_or(1) != 0,
            text_movement,
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
            let points: Vec<Vec3> = points
                .into_iter()
                .map(|point| vec3_local(point_from_origin(point, origin)))
                .collect();
            add_polyline(&mut geom.ext_lines, &points);
        }
    }

    // DIMTMOVE=1 connects the dimension line to rendered text.
    if let Some(s) = style {
        if let Some(moved) = moved_text.as_ref() {
            for (start, end) in &moved.lines {
                add_segment(&mut geom.dim_lines, vec3_local(*start), vec3_local(*end));
            }
        } else if s.dimtmove == 1 {
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
    if let Some(point) = dimension_jog_point(source_dim) {
        let jog_angle = style
            .map(|style| style.dimjogang as f32)
            .unwrap_or(std::f32::consts::FRAC_PI_4);
        apply_dimension_jog(
            &mut geom.dim_lines,
            vec3_local(point_from_origin(point, origin)),
            vec3_local(dim.base().normal),
            dim_txt as f32 * CELL_WIDTH as f32,
            jog_angle,
        );
    }
    apply_dimension_breaks(document, handle, &mut geom.dim_lines, origin);
    apply_dimension_breaks(document, handle, &mut geom.ext_lines, origin);
    geom.origin = origin;
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

    let snap_pts = dimension_snap_pts(source_dim);
    let key_vertices: Vec<[f64; 3]> = geom
        .dim_lines
        .iter()
        .chain(geom.ext_lines.iter())
        .copied()
        .filter(|p| !(p[0].is_nan() || p[1].is_nan() || p[2].is_nan()))
        // Still in the local frame here; `translate_wire_from_origin` folds
        // the origin in once, together with the wire's point buffers.
        .map(|[x, y, z]| [x as f64, y as f64, z as f64])
        .collect();

    // DIMLWD (dim line + arrows) and DIMLWE (extension lines). Negative
    // codes fall through to the entity's own resolved weight.
    let lw_dim = resolve_dim_lineweight_px(style.map(|s| s.dimlwd).unwrap_or(-2), line_weight_px);
    let lw_ext = resolve_dim_lineweight_px(style.map(|s| s.dimlwe).unwrap_or(-2), line_weight_px);

    // DIMLTEX (dim line) / DIMLTEX1 (ext1) / DIMLTEX2 (ext2) — linetype
    // handles → pattern. Looked up in document.line_types by handle.
    let lt_scale = document.header.linetype_scale as f32 * dim.base().common.linetype_scale as f32;
    let pattern = |line_type: Option<codec::types::Handle>| {
        resolve_pattern_by_handle(
            document,
            line_type.unwrap_or(codec::types::Handle::NULL),
            &dim.base().common,
            active_viewport,
            lt_scale,
        )
    };
    let (dim_pat_len, dim_pat) = pattern(style.map(|s| s.dimltex_handle));
    let (ext1_pat_len, ext1_pat) = pattern(style.map(|s| s.dimltex1_handle));
    let (ext2_pat_len, ext2_pat) = pattern(style.map(|s| s.dimltex2_handle));

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
                    snap_only: false,
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

                    ..Default::default()
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
                    snap_only: false,
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

                    ..Default::default()
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
                snap_only: false,
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

                ..Default::default()
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
        snap_only: false,
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

        ..Default::default()
    });
    // Arrowheads and ticks: the dimension line's colour and weight, never
    // its dashes.
    if !geom.arrow_lines.is_empty() {
        if let Some(line) = wires.last() {
            let mut arrows = line.clone();
            arrows.points = std::mem::take(&mut geom.arrow_lines);
            arrows.pattern_length = 0.0;
            arrows.pattern = [0.0; 8];
            arrows.fill_tris = Vec::new();
            arrows.snap_pts = Vec::new();
            arrows.key_vertices = Vec::new();
            arrows.tangent_geoms = Vec::new();
            wires.push(arrows);
        }
    }

    if let Some(symbol) = style.and_then(|style| {
        arc_length_symbol_points(dim, Some(style), dim_txt, dim_scale, style.dimarcsym, Some(document))
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
            snap_only: false,
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
            color: if selected {
                WireModel::SELECTED
            } else {
                text_color
            },
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

            ..Default::default()
        });
    }

    // DIMTFILL: 0=none, 1=drawing background (mask), 2=DIMTFILLCLR.
    if let Some(s) = style {
        if s.dimtfill == 1 || s.dimtfill == 2 {
            if let Some(rect) = text_fill_rect(dim, style, dim_txt, dim_scale, Some(document)) {
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
                    snap_only: false,
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

                    ..Default::default()
                });
            }
        }
    }

    // DIMINSPECT: label, value and rate in their own frame.
    let inspection = crate::entities::dim_inspect::read(&dim.base().common.extended_data);
    // A negative DIMGAP denotes the Basic tolerance display: frame the
    // dimension text while keeping the absolute gap as the frame margin.
    if inspection.is_none() && style.is_some_and(|s| s.dimgap < 0.0) {
        if let Some(rect) = text_fill_rect(dim, style, dim_txt, dim_scale, Some(document)) {
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
                snap_only: false,
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
                color: if selected {
                    WireModel::SELECTED
                } else {
                    text_color
                },
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

                ..Default::default()
            });
        }
    }

    // Everything above is local-frame geometry: fold the origin back in as a
    // double-single residual. The synthesised text below is built from the
    // original f64 position, so it stays world-space and must not be shifted.
    if origin != Vector3::ZERO {
        for wire in &mut wires {
            translate_wire_from_origin(wire, origin);
        }
    }

    if let Some(mut synth_text_entity) =
        dimension_text_entity(source_dim, dim_txt, style, document, dim_scale)
    {
        // The glyphs take their colour from the text entity itself, which
        // copies the dimension's (usually ByLayer); an explicit DIMCLRT has to
        // be on the entity or the layer colour wins. (#898)
        let text_index = dim_color_index(
            xd,
            crate::entities::dim_override::DIMCLRT,
            style.map(|s| s.dimclrt).unwrap_or(0),
        );
        if (1..=255).contains(&text_index) {
            synth_text_entity.common_mut().color = AcadColor::from_index(text_index);
        }
        // Tolerance Text rendered separately so DIMTFAC scales its height
        // and DIMTOLJ aligns it vertically against the primary text.
        let tol_entity = dimension_tolerance_entity(source_dim, style, &synth_text_entity, dim_txt);
        let text_entities = match &inspection {
            Some(inspection) => crate::entities::dim_inspect::layout(
                inspection,
                synth_text_entity,
                document,
                style.map(|s| s.dimgap.abs()).unwrap_or(0.0) * dim_scale,
            ),
            None => vec![synth_text_entity],
        };
        // Route synthesised dim text through tessellate_entity so the
        // baseline/greek/full LOD ladder applies (zoom-out behaviour
        // matches top-level Text / MText). The text already has dim_scale
        // baked into its height, so anno_scale stays 1.0.
        for text_entity in &text_entities {
            let text_wires = crate::scene::tessellate_entity_dim_text(
                document,
                selected_set,
                active_viewport,
                bg_color,
                1.0,
                text_entity,
                view_aabb,
                world_per_pixel,
                text_color,
                reference_only,
            );
            for mut w in text_wires {
                w.name = name.clone();
                wires.push(w);
            }
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
                reference_only,
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
fn dim_color_index(xd: &codec::xdata::ExtendedData, code: i16, inherited: i16) -> i16 {
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
/// An arrowhead of a dimension, kept apart from its dimension line: the
/// line takes DIMLTYPE's dashes, an arrowhead or tick is always drawn solid —
/// dashed, a tick broke into two stubs once the line followed its layer's
/// linetype. (#898)
fn append_dim_arrow(g: &mut DimGeom, tip: Vec3, dir: Vec3, arrow: &ArrowKind) {
    let mut head = DimGeom::new();
    append_arrow(&mut head, tip, dir, arrow);
    if !head.dim_lines.is_empty() {
        if g.arrow_lines.last().is_some_and(|point| !point[0].is_nan()) {
            g.arrow_lines.push([f32::NAN; 3]);
        }
        g.arrow_lines.extend(head.dim_lines);
    }
    g.arrow_fill.extend(head.arrow_fill);
}

/// A dimension line's dash pattern. A linetype the style names draws as
/// itself; none or ByBlock takes the dimension's own linetype (ByLayer: its
/// layer's), which is what the ByBlock lines of its baked `*D` block drew —
/// an edited dimension, drawn live, lost its layer's dashes. (#898)
fn resolve_pattern_by_handle(
    doc: &Document,
    handle: codec::types::Handle,
    common: &codec::entities::EntityCommon,
    viewport: Option<codec::Handle>,
    scale: f32,
) -> (f32, [f32; 8]) {
    let named = (!handle.is_null())
        .then(|| doc.line_types.iter().find(|lt| lt.handle == handle))
        .flatten()
        .map(|lt| lt.name.as_str())
        .filter(|name| !name.eq_ignore_ascii_case("byblock"));
    let name = match named {
        Some(name) if name.eq_ignore_ascii_case("bylayer") => doc
            .layers
            .get(&common.layer)
            .map_or("Continuous", |layer| layer.line_type.as_str()),
        Some(name) => name,
        None => {
            crate::scene::view::render::linetype_name_for_common_viewport(doc, common, viewport)
        }
    };
    crate::scene::view::render::resolve_pattern(&doc.line_types, name, scale)
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

/// DIMTMOVE 1 text the user placed on a linear, aligned or angular
/// dimension, laid out from its stored point.
struct MovedText {
    /// The text's middle.
    center: Vector3,
    /// The leader from the dimension and the hook along the text; none while
    /// the text stays on its dimension line.
    lines: Vec<(Vector3, Vector3)>,
}

/// How far DIMTMOVE 1 text may move off its dimension line before it hangs
/// on a leader: `max(DIMASZ + DIMGAP, DIMTXT / 2 + DIMGAP + DIMEXE)`.
fn moved_text_reach(style: &DimStyle, text_height: f64, dim_scale: f64) -> f64 {
    let gap = style.dimgap.abs() * dim_scale;
    (style.dimasz * dim_scale + gap).max(text_height * 0.5 + gap + style.dimexe * dim_scale)
}

/// How close (as the sine of the angle) the arc's tangent at its middle must
/// be to the text direction for the text to stand like a linear dimension's
/// (measured: lifted at 0.00001 degrees off, not at 0.0001).
const ANGULAR_LEVEL_TOLERANCE: f64 = 1e-6;

/// DIMTMOVE 1 angular text the user placed.
fn angular_text_moved(dim: &Dimension, style: Option<&DimStyle>) -> bool {
    style.is_some_and(|s| s.dimtmove == 1)
        && matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_))
        && dim.base().text_user_positioned
}

/// DIMTMOVE 1 angular text placed off its arc, on a leader.
fn angular_text_on_leader(
    dim: &Dimension,
    style: Option<&DimStyle>,
    document: Option<&Document>,
) -> bool {
    let Some(s) = style.filter(|s| s.dimtmove == 1) else {
        return false;
    };
    if !matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_))
        || !dim.base().text_user_positioned
    {
        return false;
    }
    let (Some(point), Some((vertex, start, end, radius))) =
        (stored_text_point(dim), angular_dimension_frame(dim))
    else {
        return false;
    };
    let scale = if s.dimscale > 1e-9 { s.dimscale } else { 1.0 };
    let height = s.dimtxt * scale;
    let width = dimension_text_width(dim, style, height, document);
    let mid = ((start + end) * 0.5) as f64;
    let direction = dim.base().horizontal_direction;
    !angular_text_meets_arc(
        [vertex.x as f64, vertex.y as f64],
        radius as f64,
        mid,
        [point.x, point.y],
        [direction.cos(), direction.sin()],
        width,
        height,
        s,
        scale,
    )
}

/// Whether DIMTMOVE 1 angular text at `point` still sits on its arc: the
/// text box, with the hook (a gap and an arrow) on the side facing the arc's
/// middle, comes within DIMEXE of the arc's circle. The box is the text's
/// drawn width (its ink in the drawing's font) by its height. Measured
/// against the reference for moves away from and towards the arc's centre
/// (any arc direction, txt/simplex/TrueType, heights 2.5 and 5); for moves
/// along the arc the reference keeps the text on the arc longer.
#[allow(clippy::too_many_arguments)]
fn angular_text_meets_arc(
    vertex: [f64; 2],
    radius: f64,
    mid: f64,
    point: [f64; 2],
    u: [f64; 2],
    width: f64,
    height: f64,
    style: &DimStyle,
    dim_scale: f64,
) -> bool {
    let hook = (style.dimasz + style.dimgap.abs()) * dim_scale;
    let exe = style.dimexe * dim_scale;
    let v = [-u[1], u[0]];
    let to_mid = [
        vertex[0] + radius * mid.cos() - point[0],
        vertex[1] + radius * mid.sin() - point[1],
    ];
    let half = width * 0.5;
    let (u0, u1) = if to_mid[0] * u[0] + to_mid[1] * u[1] <= 0.0 {
        (-half - hook, half)
    } else {
        (-half, half + hook)
    };
    let (v0, v1) = (-height * 0.5, height * 0.5);
    let o = [vertex[0] - point[0], vertex[1] - point[1]];
    let ou = o[0] * u[0] + o[1] * u[1];
    let ov = o[0] * v[0] + o[1] * v[1];
    let near = (ou - ou.clamp(u0, u1)).hypot(ov - ov.clamp(v0, v1));
    let far = [(u0, v0), (u0, v1), (u1, v0), (u1, v1)]
        .iter()
        .map(|(cu, cv)| (ou - cu).hypot(ov - cv))
        .fold(0.0_f64, f64::max);
    near < radius + exe && far > radius - exe
}

/// Where DIMTMOVE 1 puts user-placed text and its leader.
///
/// Within `max(DIMASZ + DIMGAP, DIMTXT / 2 + DIMGAP + DIMEXE)` of the
/// dimension line the text stays on the line: the stored point is its
/// middle, lifted by DIMTAD. Further away a leader runs from the middle of
/// the dimension line (of the arc for an angular dimension) to a hook along
/// the text:
/// * linear and aligned: the stored point is where the text's near edge
///   meets the hook line. The hook starts a gap before it and, with the text
///   above or below it (DIMTAD), runs a gap past the far edge; with centred
///   text (DIMTAD 0) it is two gaps long and ends a gap before the text.
///   Text turned by its own rotation starts at the stored point unlifted.
/// * horizontal text beyond the extension lines of a dimension that is not
///   horizontal: the dimension line runs on to the hook's height and the hook
///   turns along it to a gap past the far edge of the text, which is centred
///   on the stored point but keeps an arrow and a gap from the turn. Text
///   turned by its own rotation stays centred on the stored point, unlifted,
///   and the hook stops a gap before it.
/// * angular: the text leaves its arc once its box, with the hook, clears the
///   arc's circle by DIMEXE (`angular_text_meets_arc`); the stored point is
///   the text's middle and a hook one arrow long ends a gap before it.
fn dimtmove_one_text(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
    document: Option<&Document>,
) -> Option<MovedText> {
    let s = style.filter(|s| s.dimtmove == 1)?;
    let base = dim.base();
    if !base.text_user_positioned {
        return None;
    }
    let point = stored_text_point(dim)?;
    // Measured in the drawing's font only for text the user placed.
    let text_width = dimension_text_width(dim, style, text_height, document);
    let gap = s.dimgap.abs() * dim_scale;
    let arrow = s.dimasz * dim_scale;
    let reach = moved_text_reach(s, text_height, dim_scale);
    let lift = match s.dimtad {
        0 => 0.0,
        tad => {
            let half = dimension_text_extent_height(dim, style, text_height) * 0.5 + gap;
            if tad == 4 {
                -half
            } else {
                half
            }
        }
    };
    let turned = base.text_rotation.abs() > 1e-9;
    // Text turned by its own rotation hangs from a hook along the
    // dimension's horizontal direction, like horizontal text does.
    let angle = if turned {
        base.horizontal_direction
    } else {
        dimension_text_rotation(dim, style)
    };
    let u = [angle.cos(), angle.sin()];
    let v = [-u[1], u[0]];
    let p = [point.x, point.y];
    let at = |o: [f64; 2], d: [f64; 2], k: f64| [o[0] + d[0] * k, o[1] + d[1] * k];
    let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    let v3 = |q: [f64; 2]| Vector3::new(q[0], q[1], point.z);
    let lifted = |q: [f64; 2]| v3(at(q, v, lift));

    if let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) {
        if !matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)) {
            return None;
        }
        let mid = ((start + end) * 0.5) as f64;
        let m = [
            vertex.x as f64 + radius as f64 * mid.cos(),
            vertex.y as f64 + radius as f64 * mid.sin(),
        ];
        // With the arc's tangent at its middle along the text, the text
        // stands like a linear dimension's: on the arc it is lifted by DIMTAD
        // (below for 2 and 4), on a leader it stands on a hook line running
        // from a gap past its near edge to its far edge.
        let tangent = [-mid.sin(), mid.cos()];
        let along = (tangent[0] * u[1] - tangent[1] * u[0]).abs() < ANGULAR_LEVEL_TOLERANCE;
        let level_lift = if along {
            let half = dimension_text_extent_height(dim, style, text_height) * 0.5 + gap;
            match s.dimtad {
                0 => 0.0,
                2 | 4 => -half,
                _ => half,
            }
        } else {
            0.0
        };
        if !angular_text_on_leader(dim, style, document) {
            return Some(MovedText {
                center: v3(at(p, v, level_lift)),
                lines: Vec::new(),
            });
        }
        // Text straight above or below the middle hangs its hook on the right.
        let side = if dot(sub(p, m), u) > 0.0 { 1.0 } else { -1.0 };
        if level_lift != 0.0 {
            let line = at(p, v, -level_lift);
            let hook_start = at(line, u, -side * (text_width * 0.5 + gap));
            let hook_end = at(line, u, side * text_width * 0.5);
            return Some(MovedText {
                center: point,
                lines: vec![(v3(m), v3(hook_start)), (v3(hook_start), v3(hook_end))],
            });
        }
        let hook_end = at(p, u, -side * (text_width * 0.5 + gap));
        let hook_start = at(hook_end, u, -side * arrow);
        return Some(MovedText {
            center: point,
            lines: vec![(v3(m), v3(hook_start)), (v3(hook_start), v3(hook_end))],
        });
    }

    let (first, second, definition, axis) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            [d.rotation.cos(), d.rotation.sin()],
        ),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = dx.hypot(dy);
            if len < 1e-12 {
                return None;
            }
            (
                d.first_point,
                d.second_point,
                d.definition_point,
                [dx / len, dy / len],
            )
        }
        _ => return None,
    };
    let dline = [definition.x, definition.y];
    let foot = |q: Vector3| at(dline, axis, dot(sub([q.x, q.y], dline), axis));
    let (d1, d2) = (foot(first), foot(second));
    let m = [(d1[0] + d2[0]) * 0.5, (d1[1] + d2[1]) * 0.5];
    let normal = [-axis[1], axis[0]];
    if dot(sub(p, dline), normal).abs() < reach {
        return Some(MovedText {
            center: lifted(p),
            lines: Vec::new(),
        });
    }
    let span = sub(d2, d1);
    let length = dot(span, span).sqrt();
    let along = if length > 1e-12 {
        dot(sub(p, d1), span) / length
    } else {
        0.0
    };
    let crosses = (u[0] * axis[1] - u[1] * axis[0]).abs() > 1e-3;
    if crosses && length > 1e-12 && (along < 0.0 || along > length) {
        let (end, out) = if along < 0.0 {
            (d1, [-span[0] / length, -span[1] / length])
        } else {
            (d2, [span[0] / length, span[1] / length])
        };
        let rise = dot(out, v);
        let travel = dot(sub(p, end), v) / rise;
        if rise.abs() > 1e-9 && travel >= 0.0 {
            let elbow = at(end, out, travel);
            let mut offset = dot(sub(p, elbow), u);
            let side = if offset >= 0.0 { 1.0 } else { -1.0 };
            let clear = arrow + gap;
            if side * offset - text_width * 0.5 < clear {
                offset = side * (clear + text_width * 0.5);
            }
            // Turned text sits on the stored point unlifted and the hook
            // stops a gap before it; level text sits on the hook.
            let (reach_past, center) = if turned {
                (-side * (text_width * 0.5 + gap), v3(at(elbow, u, offset)))
            } else {
                (side * (text_width * 0.5 + gap), lifted(at(elbow, u, offset)))
            };
            let hook_end = at(elbow, u, offset + reach_past);
            return Some(MovedText {
                center,
                lines: vec![(v3(end), v3(elbow)), (v3(elbow), v3(hook_end))],
            });
        }
    }
    let side = if dot(sub(p, m), u) >= 0.0 { 1.0 } else { -1.0 };
    let (hook_start, hook_end) = if s.dimtad == 0 {
        (at(p, u, -side * 3.0 * gap), at(p, u, -side * gap))
    } else {
        (at(p, u, -side * gap), at(p, u, side * (text_width + gap)))
    };
    let center = if turned {
        let r = [base.text_rotation.cos(), base.text_rotation.sin()];
        v3(at(p, r, side * text_width * 0.5))
    } else {
        lifted(at(p, u, side * text_width * 0.5))
    };
    Some(MovedText {
        center,
        lines: vec![(v3(m), v3(hook_start)), (v3(hook_start), v3(hook_end))],
    })
}

/// Endpoints for the DIMTMOVE=1 leader of a diameter or jogged radius
/// (linear, aligned and angular text: [`dimtmove_one_text`]).
fn dimtmove_leader_endpoints(
    dim: &Dimension,
    text: DimensionTextLayout,
) -> Option<(Vec3, Vec3, bool)> {
    let lv = |v| vec3_local(v);
    let txt = text.position;
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
        Dimension::LargeRadial(d) => {
            let chord = lv(d.chord_point);
            let jog = lv(d.jog_point);
            let override_center = lv(d.override_center);
            let (near, far) = jogged_radial_break(chord, jog, override_center, d.jog_angle as f32);
            let curves =
                [(chord, near), (near, far), (far, override_center)].map(|(start, end)| {
                    kernel::geom2d::Curve::Line(kernel::geom2d::Line {
                        start: [start.x as f64, start.y as f64],
                        end: [end.x as f64, end.y as f64],
                    })
                });
            let (_, closest) =
                kernel::geom2d::nearest_of(curves.iter(), [txt.x as f64, txt.y as f64])?;
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
    document: Option<&Document>,
) -> Option<Vec<[f32; 3]>> {
    let value = dimension_text_value(dim, style)?;
    if value.is_empty() {
        return None;
    }
    let pos = dimension_text_pos_f64(dim, style, text_height, dim_scale, document);
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
    document: Option<&Document>,
) -> Option<Vec<Vec3>> {
    if !matches!(dim, Dimension::Arc(_)) || symbol_position == 2 {
        return None;
    }
    let value = dimension_text_value(dim, style)?;
    if value.is_empty() || text_height <= 1.0e-12 {
        return None;
    }

    let position = dimension_text_pos_f64(dim, style, text_height, dim_scale, document);
    let rotation = dimension_text_rotation(dim, style);
    let (sin_rotation, cos_rotation) = rotation.sin_cos();
    let stack_scale = style.map(dimtfac_or_one).unwrap_or(1.0);
    let text_width = text_cells(&value, stack_scale) * text_height * CELL_WIDTH;
    let symbol_width = text_height * 0.62;
    let symbol_height = text_height * 0.20;
    let (center_x, center_y) = if symbol_position == 1 {
        (0.0, text_height * 0.72)
    } else {
        (
            -(text_width * 0.5 + symbol_width * 0.70),
            text_height * 0.02,
        )
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
    document: Option<&Document>,
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
        document,
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
    let horizontal =
        style.is_some_and(|style| (outside && style.dimtoh) || (!outside && style.dimtih));
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
    /// Text height (DIMTXT, scaled) — a radial leader is sized by it.
    text_height: f32,
    /// This dimension is a dimensional constraint's dynamic dimension, which
    /// the reference draws by its own radial rules.
    constraint: bool,
    dimatfit: i16,
    dimtix: bool,
    dimtofl: bool,
    text_position: Vec3,
    horizontal_text: bool,
    /// DIMTAD lifts aligned text off its dimension line.
    text_lifted: bool,
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
            if params.constraint {
                // A dimensional constraint reads on its own dimension line,
                // which runs from the centre to the arc point and breaks
                // where the text sits on it. Text that no longer fits inside
                // is reached by a leader from the arc point, and the
                // arrowhead stays on the arc either way.
                if !jogged && !suppress.dim2 {
                    add_segment_with_text_break(&mut g.dim_lines, center, point, params.text_break);
                }
                if text_is_outside && !suppress.dim2 {
                    append_radial_leader(&mut g, point, point - center, text, &params);
                }
                if !suppress.dim2 {
                    // The arrowhead's tip is on the arc, its body inside.
                    append_dim_arrow(
                        &mut g,
                        point,
                        normalized_or(center - point, Vec3::X),
                        arrow1,
                    );
                }
                // The centre mark belongs to a radius drawn without its
                // inside line; the line itself already marks the centre.
                if jogged || suppress.dim2 {
                    append_center_mark(&mut g, center, params.dimcen, radius);
                }
            } else {
                // Text inside the arc: the dimension line runs from the arc
                // point to the centre. Text outside: only a leader from the
                // arc point to the text, unless DIMTOFL asks for the inside
                // line as well.
                let inside_line =
                    !jogged && !suppress.dim2 && (!text_is_outside || params.dimtofl);
                if inside_line {
                    add_segment(&mut g.dim_lines, center, point);
                }
                if text_is_outside && !suppress.dim2 {
                    append_radial_leader(&mut g, point, point - center, text, &params);
                }
                if !suppress.dim2 {
                    // The arrowhead sits on the arc with its body toward the text.
                    let body = if text_is_outside {
                        point - center
                    } else {
                        center - point
                    };
                    append_dim_arrow(&mut g, point, normalized_or(body, Vec3::X), arrow1);
                }
                // The centre mark only goes with a radius whose dimension
                // line does not reach the centre.
                if text_is_outside && !inside_line {
                    append_center_mark(&mut g, center, params.dimcen, radius);
                }
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
                Some((vertex, start, end)) => {
                    // An extension line runs from a side's nearer end out to
                    // the arc only where the arc lies beyond that side; where
                    // the arc crosses the side itself there is nothing to add.
                    let radius = vertex.distance(arc_point);
                    let side = |angle: f32| -> (Vec3, bool) {
                        let dir = Vec3::new(angle.cos(), angle.sin(), 0.0);
                        let deviation =
                            |a: Vec3, b: Vec3| normalized_or(b - a, dir).cross(dir).length();
                        let (a, b) = if deviation(p1, p2) <= deviation(p3, p4) {
                            (p1, p2)
                        } else {
                            (p3, p4)
                        };
                        let along = |p: Vec3| (p - vertex).dot(dir);
                        let (lo, hi) = (along(a).min(along(b)), along(a).max(along(b)));
                        let arc_end = vertex + dir * radius;
                        let near = if a.distance(arc_end) <= b.distance(arc_end) {
                            a
                        } else {
                            b
                        };
                        (near, radius >= lo - 1e-6 && radius <= hi + 1e-6)
                    };
                    let (first, first_covered) = side(start);
                    let (second, second_covered) = side(end);
                    let suppress = SuppressFlags {
                        ext1: suppress.ext1 || first_covered,
                        ext2: suppress.ext2 || second_covered,
                        dim1: suppress.dim1,
                        dim2: suppress.dim2,
                    };
                    append_angular_dimension(
                        &mut g,
                        vertex,
                        first,
                        second,
                        arc_point,
                        arrow1,
                        arrow2,
                        Some((start, end)),
                        params,
                        suppress,
                    )
                }
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
            // The sweep starts at whichever ray the arc point says; the
            // extension lines swap with it, or they would cross the angle.
            let (first, second, explicit_sweep) =
                match three_point_frame(vertex, first, second, arc_point) {
                    Some((start, end, true)) => (second, first, Some((start, end))),
                    Some((start, end, false)) => (first, second, Some((start, end))),
                    None => (first, second, None),
                };
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
                let fixed_length = params.dimfxlon.then_some(params.dimfxl.max(0.0) as f64);
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
            let (near, far) = jogged_radial_break(chord, jog, override_center, d.jog_angle as f32);
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
                append_dim_arrow(
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
    let segment_curve = kernel::geom2d::Curve::Line(kernel::geom2d::Line {
        start: [start.x as f64, start.y as f64],
        end: [end.x as f64, end.y as f64],
    });
    let tolerance = kernel::geom2d::Tolerance::new((length as f64 * 1.0e-9).max(1.0e-9));
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
                kernel::geom2d::Curve::Line(kernel::geom2d::Line {
                    start: [corners[index].x as f64, corners[index].y as f64],
                    end: [corners[next].x as f64, corners[next].y as f64],
                })
            })
            .collect();
        kernel::geom2d::inside_spans(&boundary, &segment_curve, tolerance)
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
        // Never past the DIMEXO gap at the definition point: a length longer
        // than the line itself would otherwise shoot far beyond the object.
        let fxl1 = fxl.min((offset1.abs() - params.dimexo).max(0.0));
        let fxl2 = fxl.min((offset2.abs() - params.dimexo).max(0.0));
        let s1 = d1 - ext_dir * (sign1 * fxl1);
        let e1 = d1 + ext_dir * (sign1 * params.dimexe);
        let s2 = d2 - ext_dir * (sign2 * fxl2);
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
    let reach =
        |default: f32, toward_text: Option<f32>| toward_text.map_or(default, |t| t.max(default));
    let stub = params.arrow_len * 2.0;
    let text_beyond_d1 = text_span
        .filter(|(_, hi)| *hi < 0.0 && !hooks_out_horizontally)
        .map(|(_, hi)| -hi as f32);
    let text_beyond_d2 = text_span
        .filter(|(lo, _)| *lo > gap as f64 && !hooks_out_horizontally)
        .map(|(lo, _)| (lo - gap as f64) as f32);
    // DIMTMOVE=2 leaves moved text outside the dimension without a fine
    // leader or an extension that chases the text. Keep genuine arrow-fit
    // stubs, but do not turn a user-placed text point into another segment.
    let chase_text = params.text_movement != 2;
    let beyond_d1 = chase_text.then_some(text_beyond_d1).flatten();
    let beyond_d2 = chase_text.then_some(text_beyond_d2).flatten();
    if (arrows_outside && !params.dimsoxd) || beyond_d1.is_some() || beyond_d2.is_some() {
        if !suppress.dim1 && (arrows_outside || beyond_d1.is_some()) {
            add_segment(
                &mut g.dim_lines,
                d1 - dir_d1_to_d2 * reach(stub, beyond_d1),
                d1,
            );
        }
        if !suppress.dim2 && (arrows_outside || beyond_d2.is_some()) {
            add_segment(
                &mut g.dim_lines,
                d2,
                d2 + dir_d1_to_d2 * reach(stub, beyond_d2),
            );
        }
    }

    if arrows_outside {
        // Tip on the ext line, body pointing outward.
        append_dim_arrow(g, d1, normalized_or(d1 - d2, -axis), arrow1);
        append_dim_arrow(g, d2, normalized_or(d2 - d1, axis), arrow2);
    } else {
        append_dim_arrow(g, d1, normalized_or(d2 - d1, axis), arrow1);
        append_dim_arrow(g, d2, normalized_or(d1 - d2, -axis), arrow2);
    }

    // Horizontal text outside the extension lines of a dimension that is not
    // horizontal itself: the dimension line runs on to the text's height and
    // turns a hook one arrow long toward the text, which sits against it.
    if params.text_movement != 2
        && params.horizontal_text
        && !params.ticks
        && dir_d1_to_d2.y.abs() > 1e-3
    {
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
                    let side = if text_break.center.x >= elbow.x {
                        1.0
                    } else {
                        -1.0
                    };
                    let hook_end = Vec3::new(elbow.x + side * params.arrow_len, elbow.y, elbow.z);
                    add_segment(&mut g.dim_lines, end, elbow);
                    add_segment(&mut g.dim_lines, elbow, hook_end);
                }
            }
        }
    }
}

/// Leader from a point on the circle to text outside it.
///
/// A dimensional constraint's dimension leaves the rim along its dimension
/// line, elbows level with the text and hooks under it. An ordinary radial
/// dimension keeps its measured layout: horizontal text gets a hook one arrow
/// long that the text sits against, and aligned text is reached by a straight
/// leader that stops a gap short of it. `text_width` already carries a gap on
/// each side.
fn append_radial_leader(
    g: &mut DimGeom,
    tip: Vec3,
    direction: Vec3,
    text: Vec3,
    params: &DimLineParams,
) {
    if params.constraint {
        let side = if text.x >= tip.x { 1.0 } else { -1.0 };
        let edge = match params.leader_anchor {
            Some(anchor) => anchor,
            None => Vec3::new(text.x - side * params.text_width * 0.5, text.y, text.z),
        };
        let elbow = tip
            + normalized_or(direction, Vec3::X)
                * radial_leader_run(params.text_height as f64) as f32;
        // The hook is horizontal at the text's height; the run reaches it.
        let elbow = Vec3::new(elbow.x, edge.y, edge.z);
        add_segment(&mut g.dim_lines, tip, elbow);
        add_segment(&mut g.dim_lines, elbow, edge);
        return;
    }
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
    } else if params.text_lifted {
        // Text above or below the line: the line carries on out from the
        // rim, under the whole text to its far end.
        let along = normalized_or(direction, Vec3::X);
        let reach = (text - tip).dot(along) + params.text_width * 0.5;
        if reach > 0.0 {
            add_segment(&mut g.dim_lines, tip, tip + along * reach);
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
    if text_is_outside && (params.constraint || !params.dimtofl) {
        // A dimensional constraint carries its text on a leader from the
        // nearer end of the dimension line, and the line and both arrowheads
        // stay inside the circle. An ordinary diameter draws no line across
        // the circle at all: just that leader, with one arrowhead on the
        // circle and its body toward the text. DIMTMOVE 2 keeps the arrowhead
        // and drops the leader.
        let (tip, suppressed) = if params.text_position.distance_squared(chord)
            <= params.text_position.distance_squared(far_chord)
        {
            (chord, suppress.dim1)
        } else {
            (far_chord, suppress.dim2)
        };
        if !suppressed && params.text_movement != 2 {
            append_radial_leader(g, tip, tip - center, params.text_position, &params);
        }
        if !params.constraint {
            if !suppressed {
                append_dim_arrow(g, tip, normalized_or(tip - center, axis), arrow1);
            }
            return;
        }
    }
    // Text projected outside the diameter requires inward-pointing arrowheads.
    let text_along = (params.text_position - chord).dot(axis);
    let text_outside = text_along < 0.0 || text_along > diameter;

    let arrows_outside = if params.ticks || params.arrow_len <= 1e-6 {
        false
    } else if text_is_outside {
        // The leader carries the text; the arrowheads keep the circle.
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

    // The centre mark only goes with a diameter whose dimension line does not
    // cross the circle.
    if !draw_inside_line {
        let center = (chord + far_chord) * 0.5;
        append_center_mark(g, center, params.dimcen, diameter * 0.5);
    }
    if arrows_outside {
        append_dim_arrow(g, chord, -axis, arrow1);
        append_dim_arrow(g, far_chord, axis, arrow2);
    } else {
        append_dim_arrow(g, chord, axis, arrow1);
        append_dim_arrow(g, far_chord, -axis, arrow2);
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
    let (vertex, start, end) = two_line_angle_frame_f64(
        a1.as_dvec3(),
        a2.as_dvec3(),
        b1.as_dvec3(),
        b2.as_dvec3(),
        arc_point.as_dvec3(),
    )?;
    Some((vertex.as_vec3(), start as f32, end as f32))
}

/// The box the drawing leaves open in the dimension line around the text, in
/// world coordinates: centre, half width and half height (the gap included)
/// and rotation. The saved block breaks its dimension line the same way.
pub(crate) fn dimension_text_break_world(
    dim: &Dimension,
    document: &Document,
) -> Option<(DVec3, f64, f64, f64)> {
    let origin = dimension_precision_origin(dim);
    let shifted = (origin != Vector3::ZERO).then(|| shift_dimension(dim, origin));
    let local = shifted.as_ref().unwrap_or(dim);
    let style_name = &local.base().style_name;
    let style = document
        .dim_styles
        .iter()
        .find(|s| {
            s.name.eq_ignore_ascii_case(style_name)
                || (style_name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
        })
        .map(|style| resolved_dimension_style(style, local, document));
    let style = style.as_ref();
    let scale = style.map_or(1.0, |s| if s.dimscale > 1e-6 { s.dimscale } else { 1.0 });
    let text_height = style.map_or(2.5, |s| s.dimtxt * scale);
    let gap = dimension_text_layout(local, style, text_height, scale, Some(document)).break_box?;
    Some((
        gap.center.as_dvec3() + DVec3::new(origin.x, origin.y, origin.z),
        f64::from(gap.half_width + gap.padding),
        f64::from(gap.half_height + gap.padding),
        gap.rotation,
    ))
}

/// [`two_line_angle_frame`] in full precision, for the saved block.
pub(crate) fn two_line_angle_frame_f64(
    a1: DVec3,
    a2: DVec3,
    b1: DVec3,
    b2: DVec3,
    arc_point: DVec3,
) -> Option<(DVec3, f64, f64)> {
    let (u, v) = (a2 - a1, b2 - b1);
    let (t, _) = kernel::geom2d::line_line([a1.x, a1.y], [u.x, u.y], [b1.x, b1.y], [v.x, v.y])?;
    let vertex = a1 + u * t;

    // Two lines cross at four angles; the arc point picks one. Each line
    // contributes its direction and its reverse, so try the four pairs and keep
    // the one whose sweep both contains the arc point and is the shorter way
    // round — the dimension marks an angle, never its reflex twin.
    let angle_of = |d: DVec3| d.y.atan2(d.x);
    let target = angle_of(arc_point - vertex);
    let mut best: Option<(f64, f64, f64)> = None;
    for su in [1.0f64, -1.0] {
        for sv in [1.0f64, -1.0] {
            let (from_u, from_v) = (angle_of(u * su), angle_of(v * sv));
            // Either line can be the one the sweep starts from; taking only
            // u→v leaves out half the angles at the crossing, and the half left
            // out is the one wanted whenever the lines are nearly parallel.
            for (start, end) in [(from_u, from_v), (from_v, from_u)] {
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                if sweep <= 1e-6 || sweep > std::f64::consts::PI {
                    continue;
                }
                let into_target = (target - start).rem_euclid(std::f64::consts::TAU);
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

/// The sweep of a three-point angular dimension: counter-clockwise from the
/// first ray to the second when the arc point lies in that sweep, otherwise
/// the other way round (`swapped`). Rays need no crossing, so a straight
/// angle draws, and a reflex one is kept as picked.
fn three_point_frame(
    vertex: Vec3,
    first: Vec3,
    second: Vec3,
    arc_point: Vec3,
) -> Option<(f32, f32, bool)> {
    let angle_of = |d: Vec3| d.y.atan2(d.x);
    let (r1, r2, at) = (first - vertex, second - vertex, arc_point - vertex);
    if r1.length_squared() <= 1e-12 || r2.length_squared() <= 1e-12 || at.length_squared() <= 1e-12
    {
        return None;
    }
    let tau = std::f32::consts::TAU;
    let (a1, a2) = (angle_of(r1), angle_of(r2));
    let sweep = (a2 - a1).rem_euclid(tau);
    if sweep <= 1e-6 {
        return None;
    }
    if (angle_of(at) - a1).rem_euclid(tau) <= sweep {
        Some((a1, a1 + sweep, false))
    } else {
        Some((a2, a2 + (tau - sweep), true))
    }
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

pub(crate) fn arc_dimension_in_ocs(dimension: &DimensionArc) -> Option<(DimensionArc, Vector3)> {
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
    wire.text_verts = crate::scene::model::wire_model::map_text_verts(&wire.text_verts, map);
    // OCS→WCS is rigid: unit height scale, no in-plane rotation change.
    wire.map_searchable_runs(
        &|p| {
            let mapped = map(p[0], p[1], p[2]);
            [mapped.0, mapped.1, mapped.2]
        },
        1.0,
        0.0,
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
                let center = map(center[0] as f64, center[1] as f64, center[2] as f64);
                let (x_axis, y_axis) = crate::scene::view::transform::ocs_axes(normal_tuple);
                *tangent = TangentGeom::PlanarCircle {
                    center: [center.0, center.1, center.2],
                    axis_x: [x_axis.0, x_axis.1, x_axis.2],
                    axis_y: [y_axis.0, y_axis.1, y_axis.2],
                    radius: *radius as f64,
                };
            }
            TangentGeom::PlanarCircle {
                center,
                axis_x,
                axis_y,
                ..
            }
            | TangentGeom::Arc {
                center,
                axis_x,
                axis_y,
                ..
            } => {
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
            let (vertex, start, end) =
                two_line_angle_frame(first_start, first_end, second_start, second_end, arc_point)?;
            (vertex, start, end, arc_point)
        }
        Dimension::Angular3Pt(value) => {
            let vertex = vec3_local(value.angle_vertex);
            let first = vec3_local(value.first_point);
            let second = vec3_local(value.second_point);
            let arc_point = vec3_local(value.definition_point);
            let (start, end, _) = three_point_frame(vertex, first, second, arc_point)?;
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
            // Never past the DIMEXO gap at the measured point.
            let free = (endpoint.distance(origin) - params.dimexo).max(0.0);
            (
                endpoint - direction * params.dimfxl.max(0.0).min(free),
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
    let arc_extension = if params.ticks {
        params.dimdle / radius
    } else {
        0.0
    };
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
        append_dim_arrow(
            g,
            arc_start,
            if arrows_outside {
                -start_tangent
            } else {
                start_tangent
            },
            arrow1,
        );
    }
    if draw_arrows && !suppress.dim2 {
        append_dim_arrow(
            g,
            arc_end,
            if arrows_outside {
                end_tangent
            } else {
                -end_tangent
            },
            arrow2,
        );
    }
}

fn append_sampled_arc(lines: &mut Vec<[f32; 3]>, vertex: Vec3, radius: f32, start: f32, end: f32) {
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
    let mut points: Vec<_> = kernel::geom2d::tessellate::arc(
        [vertex.x as f64, vertex.y as f64],
        radius as f64,
        from as f64,
        to as f64,
        vertex.z as f64,
        kernel::geom2d::tessellate::DEFAULT_SEGMENTS_PER_RADIAN,
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
    let lv = |v: codec::types::Vector3| glam::DVec3::new(v.x, v.y, v.z);
    let node = |v: codec::types::Vector3| (lv(v), SnapHint::Node);
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
        Dimension::Ordinate(d) => vec![node(d.feature_location), node(d.leader_endpoint)],
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

/// The text style a dimension's text draws with: DIMTXSTY by handle (it
/// survives a rename), then DIMTXSTY by name, then the dimension's own style
/// name.
fn dimension_text_style_name(
    dim: &Dimension,
    style: Option<&DimStyle>,
    document: &Document,
) -> String {
    style
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
        .unwrap_or_else(|| dim.base().style_name.clone())
}

/// The width of a dimension's text as it draws: the extent of its ink in
/// the drawing's text style font. Without the drawing (or for formatted
/// multi-line text) the cell estimate stands in.
fn dimension_text_width(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    document: Option<&Document>,
) -> f64 {
    let estimate = || {
        dimension_text_cells(dim, style)
            .map(|cells| cells * text_height * CELL_WIDTH)
            .unwrap_or(0.0)
    };
    let Some(document) = document else {
        return estimate();
    };
    let Some(value) = dimension_text_value(dim, style) else {
        return 0.0;
    };
    if value_has_mtext_codes(&value) {
        return estimate();
    }
    let text = crate::entities::text_support::resolve_dxf_special_chars(&value);
    let font = crate::entities::text_support::resolve_text_style(
        &dimension_text_style_name(dim, style, document),
        document,
    );
    crate::entities::text_support::text_local_bounds(
        &font.font_name,
        &text,
        text_height as f32,
        font.width_factor,
        font.oblique_angle,
    )
    .map_or_else(estimate, |bounds| {
        (bounds.ink_max[0] - bounds.ink_min[0]).max(0.0) as f64
    })
}

fn dimension_text_entity(
    dim: &Dimension,
    text_height: f64,
    style: Option<&DimStyle>,
    document: &Document,
    dim_scale: f64,
) -> Option<EntityType> {
    // Deviations are emitted by `dimension_tolerance_entity` at their own
    // height and alignment; keep the primary entity free of duplicate text.
    let (value, _) = dimension_text_parts(dim, style)?;
    // Use f64 position directly to avoid f32 round-trip precision loss at large
    // coordinates (e.g. Turkish UTM ~4,000,000 m). tessellate() will apply
    // world_offset when rendering this synthetic entity.
    let pos_f64 = dimension_text_pos_f64(dim, style, text_height, dim_scale, Some(document));
    // A limits stack draws each half at DIMTFAC × DIMTXT, like the source application.
    let text_height = match style {
        Some(s) if s.dimlim => text_height * dimtfac_or_one(s),
        _ => text_height,
    };
    let base = dim.base();

    let rotation = dimension_text_rotation(dim, style);

    let style_name = dimension_text_style_name(dim, style, document);

    // Route through MText whenever the value carries inline format codes
    // (`\f`, `\C`, `\H`, `\S`, brace scopes, …). Otherwise stay on the Text
    // path — single-line dim text doesn't need the full MText pipeline.
    if value_has_mtext_codes(&value) {
        use codec::entities::dimension::AttachmentPointType as DA;
        use codec::entities::AttachmentPoint as MA;
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
        mtext.common = synthesized_text_common(base);
        return Some(EntityType::MText(Box::new(mtext)));
    }

    let value = if style.is_some_and(|style| style.dimtxtdirection) {
        value.chars().rev().collect()
    } else {
        value
    };
    let mut text = Text::with_value(value, pos_f64)
        .with_height(text_height)
        .with_rotation(rotation);
    text.width_factor = style_width_factor(&style_name, document);
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

    text.common = synthesized_text_common(base);
    Some(EntityType::Text(text))
}

/// Width factor for a synthesized dimension TEXT. Dimension text is drawn at
/// its text style's width factor; a TEXT's own factor (1.0 when synthesized)
/// would otherwise win over the style's.
fn style_width_factor(style_name: &str, document: &Document) -> f64 {
    f64::from(crate::entities::text_support::resolve_text_style(style_name, document).width_factor)
}

/// The dimension's properties for its synthesized text, less its extension
/// dictionary and annotative markers. The text height already carries the
/// annotation scale; with the dimension's per-scale contexts or flag the text
/// would resolve as annotative itself and scale a second time at every scale
/// but the default one. (#1939)
fn synthesized_text_common(base: &DimensionBase) -> codec::entities::EntityCommon {
    let mut common = base.common.clone();
    common.xdictionary_handle = None;
    for marker in ["AcadAnnotative", "AcAnnoPO", "AcAnnotativeData"] {
        common.extended_data.remove_record(marker);
    }
    common
}

fn attachment_to_text_align(
    attach: codec::entities::dimension::AttachmentPointType,
) -> (
    codec::entities::text::TextHorizontalAlignment,
    codec::entities::text::TextVerticalAlignment,
) {
    use codec::entities::dimension::AttachmentPointType as A;
    use codec::entities::text::{TextHorizontalAlignment as H, TextVerticalAlignment as V};
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
/// DIMTIH/DIMTOH force-horizontal (along horizontal_direction), else the
/// natural dim-line angle (+90° for DIMJUST 3/4). Shared by the live text entity and
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
    } else if angular_text_moved(dim, style) {
        // DIMTMOVE 1 angular text a user placed reads horizontally, on its
        // arc as on a leader (measured).
        base.horizontal_direction
    } else if (outside && dimtoh) || (!outside && dimtih) {
        // Text forced horizontal reads along the dimension's horizontal
        // direction; text aligned with the dimension line ignores it.
        base.horizontal_direction
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
            return (angle - start).rem_euclid(std::f32::consts::TAU) > end - start + 1.0e-6;
        }
        if style.dimtix {
            return false;
        }
        let scale = if style.dimscale > 1e-9 {
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
            return (text.x - radius.angle_vertex.x).hypot(text.y - radius.angle_vertex.y)
                > available + 1e-9;
        }
        if style.dimtix {
            return false;
        }
        let scale = if style.dimscale > 1e-9 {
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
    let scale = if style.dimscale > 1e-9 {
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
    document: &Document,
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
        .map(|s| {
            if s.dimscale > 1e-6 {
                s.dimscale
            } else {
                anno_scale
            }
        })
        .unwrap_or(1.0);
    let text_height = style.map(|s| s.dimtxt * dim_scale).unwrap_or(2.5);
    let value = dimension_text_value(dim, style)?;
    let stack_scale = style.map(dimtfac_or_one).unwrap_or(1.0);
    let half_width = text_cells(&value, stack_scale) * text_height * CELL_WIDTH * 0.5;
    let pos = dimension_text_pos_f64(dim, style, text_height, dim_scale, Some(document));
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

/// The two ends of an angular dimension's arc, each with the direction
/// pointing away from the arc: where a dynamic angle's triangle grips sit.
pub(crate) fn angular_arc_ends(dim: &Dimension) -> Option<[(glam::DVec3, glam::DVec3); 2]> {
    let (vertex, start, end, radius) = angular_dimension_frame(dim)?;
    let point = |angle: f32| {
        let p = vertex + Vec3::new(angle.cos(), angle.sin(), 0.0) * radius;
        glam::DVec3::new(p.x as f64, p.y as f64, p.z as f64)
    };
    let away = |angle: f32, sign: f32| {
        glam::DVec3::new(
            (-angle.sin() * sign) as f64,
            (angle.cos() * sign) as f64,
            0.0,
        )
    };
    Some([
        (point(start), away(start, -1.0)),
        (point(end), away(end, 1.0)),
    ])
}

fn dimension_text_natural_rotation(dim: &Dimension) -> f64 {
    let angle =
        match dim {
            Dimension::Linear(d) => d.rotation,
            Dimension::Aligned(d) => {
                let dx = d.second_point.x - d.first_point.x;
                let dy = d.second_point.y - d.first_point.y;
                dy.atan2(dx)
            }
            Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) | Dimension::Arc(_) => {
                angular_dimension_frame(dim)
                    .map(|(_, start, end, _)| {
                        ((start + end) * 0.5 + std::f32::consts::FRAC_PI_2) as f64
                    })
                    .unwrap_or(0.0)
            }
            Dimension::Radius(d) => (d.definition_point.y - d.angle_vertex.y)
                .atan2(d.definition_point.x - d.angle_vertex.x),
            Dimension::Diameter(d) => (d.definition_point.y - d.angle_vertex.y)
                .atan2(d.definition_point.x - d.angle_vertex.x),
            Dimension::LargeRadial(d) => {
                (d.chord_point.y - d.override_center.y).atan2(d.chord_point.x - d.override_center.x)
            }
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
            return Some(format!("\\S+{}{}^-{}{};", upper, unit, lower, unit));
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
    let use_sub_units = s.dimaltz & 4 != 0 && scaled.abs() < 1.0 && s.dimaltmzf.abs() > 1e-12;
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
        mtext.attachment_point = codec::entities::AttachmentPoint::MiddleCenter;
        mtext.common = primary_common;
        return Some(EntityType::MText(Box::new(mtext)));
    }

    let mut t = Text::with_value(tol, pos)
        .with_height(tol_height)
        .with_rotation(rot);
    t.style = primary_style;
    t.common = primary_common;
    t.horizontal_alignment = codec::entities::text::TextHorizontalAlignment::Center;
    t.vertical_alignment = codec::entities::text::TextVerticalAlignment::Middle;
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
fn dimension_text_extent_height(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
) -> f64 {
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
    let sign = if inches < 0.0 && ticks != 0.0 {
        "-"
    } else {
        ""
    };
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
                swap_decimal_sep(
                    &apply_angular_zero_suppression(&raw, azin),
                    decimal_separator
                )
            )
        }
        // 3 = Radians
        3 => {
            let r = measurement_deg.to_radians();
            let raw = format!("{:.*}", adec, r);
            format!(
                "{}r",
                swap_decimal_sep(
                    &apply_angular_zero_suppression(&raw, azin),
                    decimal_separator
                )
            )
        }
        // 4 = Surveyor's units
        4 => format_surveyor(measurement_deg, adec, azin, decimal_separator),
        // 0 or unknown = Decimal Degrees
        _ => {
            let raw = format!("{:.*}", adec, measurement_deg);
            format!(
                "{}°",
                swap_decimal_sep(
                    &apply_angular_zero_suppression(&raw, azin),
                    decimal_separator
                )
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
    // Engineering units end in the inch mark: the inches lose their zeros.
    if let Some(number) = s.strip_suffix('"') {
        return format!("{}\"", strip_trailing_zeros(number));
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

/// WCS point used as the local frame origin while one dimension is
/// tessellated.
///
/// Dimension geometry is assembled in `Vec3` (f32). On a survey drawing at
/// UTM-scale coordinates (eastings ~1.65e6, northings ~4.95e6) an absolute
/// f32 has an ulp of ~0.5 m, so the dimensional lines, extension lines and
/// arrows snap to a half-metre grid and visibly drift away from the points
/// they measure. Shifting the dimension's definition data to a nearby origin
/// first keeps every f32 operation exact to well under a millimetre; the
/// origin is folded back into the double-single wire buffers on output.
fn dimension_precision_origin(dim: &Dimension) -> Vector3 {
    let point = match dim {
        Dimension::Aligned(d) => d.first_point,
        Dimension::Linear(d) => d.first_point,
        Dimension::Radius(d) => d.angle_vertex,
        Dimension::Diameter(d) => d.angle_vertex,
        Dimension::Angular2Ln(d) => d.angle_vertex,
        Dimension::Angular3Pt(d) => d.angle_vertex,
        Dimension::Ordinate(d) => d.feature_location,
        Dimension::Arc(d) => d.center_point,
        Dimension::LargeRadial(d) => d.chord_point,
    };
    (point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        .then_some(point)
        .unwrap_or_else(|| {
            let base = dim.base();
            if base.definition_point.x.is_finite() {
                base.definition_point
            } else {
                Vector3::ZERO
            }
        })
}

fn point_from_origin(point: Vector3, origin: Vector3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

/// Same, but keeps the all-zero "not stored" sentinel some base fields use
/// (text placement and the legacy insertion point). Shifting a zero there
/// would turn "automatic" into a bogus explicit point at the far origin.
fn stored_point_from_origin(point: Vector3, origin: Vector3) -> Vector3 {
    if point == Vector3::ZERO {
        point
    } else {
        point_from_origin(point, origin)
    }
}

/// `dim` with every definition point moved into the local frame at `origin`.
/// Measurements, angles and vectors are translation-invariant, so the shifted
/// dimension reports the same values while its `Vec3` geometry stays small.
fn shift_dimension(dim: &Dimension, origin: Vector3) -> Dimension {
    let mut shifted = dim.clone();
    let base = shifted.base_mut();
    base.definition_point = point_from_origin(base.definition_point, origin);
    base.text_middle_point = stored_point_from_origin(base.text_middle_point, origin);
    base.insertion_point = stored_point_from_origin(base.insertion_point, origin);
    match &mut shifted {
        Dimension::Aligned(d) => {
            d.first_point = point_from_origin(d.first_point, origin);
            d.second_point = point_from_origin(d.second_point, origin);
            d.definition_point = point_from_origin(d.definition_point, origin);
        }
        Dimension::Linear(d) => {
            d.first_point = point_from_origin(d.first_point, origin);
            d.second_point = point_from_origin(d.second_point, origin);
            d.definition_point = point_from_origin(d.definition_point, origin);
        }
        Dimension::Radius(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.angle_vertex = point_from_origin(d.angle_vertex, origin);
        }
        Dimension::Diameter(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.angle_vertex = point_from_origin(d.angle_vertex, origin);
        }
        Dimension::Angular2Ln(d) => {
            d.dimension_arc = point_from_origin(d.dimension_arc, origin);
            d.first_point = point_from_origin(d.first_point, origin);
            d.second_point = point_from_origin(d.second_point, origin);
            d.angle_vertex = point_from_origin(d.angle_vertex, origin);
            d.definition_point = point_from_origin(d.definition_point, origin);
        }
        Dimension::Angular3Pt(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.first_point = point_from_origin(d.first_point, origin);
            d.second_point = point_from_origin(d.second_point, origin);
            d.angle_vertex = point_from_origin(d.angle_vertex, origin);
        }
        Dimension::Ordinate(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.feature_location = point_from_origin(d.feature_location, origin);
            d.leader_endpoint = point_from_origin(d.leader_endpoint, origin);
        }
        Dimension::Arc(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.first_extension_point = point_from_origin(d.first_extension_point, origin);
            d.second_extension_point = point_from_origin(d.second_extension_point, origin);
            d.center_point = point_from_origin(d.center_point, origin);
            d.first_leader_point = point_from_origin(d.first_leader_point, origin);
            d.second_leader_point = point_from_origin(d.second_leader_point, origin);
        }
        Dimension::LargeRadial(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.chord_point = point_from_origin(d.chord_point, origin);
            d.override_center = point_from_origin(d.override_center, origin);
            d.jog_point = point_from_origin(d.jog_point, origin);
        }
    }
    shifted
}

/// Move one wire built in the local frame back to world space, splitting each
/// vertex into the double-single pair the renderer reconstructs relative to
/// the eye. Leaves `snap_pts` alone: those are absolute source points, not
/// locally-computed geometry.
fn translate_wire_from_origin(wire: &mut WireModel, origin: Vector3) {
    let translate = |high: &mut Vec<[f32; 3]>, low: &mut Vec<[f32; 3]>| {
        if high.is_empty() {
            return;
        }
        let old_low = std::mem::take(low);
        low.reserve(high.len());
        for (index, point) in high.iter_mut().enumerate() {
            if !point[0].is_finite() {
                low.push([0.0; 3]);
                continue;
            }
            let residual = old_low.get(index).copied().unwrap_or([0.0; 3]);
            let x = point[0] as f64 + residual[0] as f64 + origin.x;
            let y = point[1] as f64 + residual[1] as f64 + origin.y;
            let z = point[2] as f64 + residual[2] as f64 + origin.z;
            let (hx, lx) = WireModel::split_ds(x);
            let (hy, ly) = WireModel::split_ds(y);
            let (hz, lz) = WireModel::split_ds(z);
            *point = [hx, hy, hz];
            low.push([lx, ly, lz]);
        }
    };
    translate(&mut wire.points, &mut wire.points_low);
    translate(&mut wire.fill_tris, &mut wire.fill_tris_low);
    translate(&mut wire.pick_tris, &mut wire.pick_tris_low);
    for point in &mut wire.key_vertices {
        point[0] += origin.x;
        point[1] += origin.y;
        point[2] += origin.z;
    }
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
    gap: f64,
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
    // DIMJUST: 0=centred, 1/3=near first ext, 2/4=near second ext. Beside
    // an extension line (1/2) the text starts an arrow and two gaps from it,
    // towards the other one; `text_w` already carries a gap each side.
    let inward = if t2 >= t1 { 1.0 } else { -1.0 };
    let beside = arrow + gap + (text_w * 0.5).max(0.0);
    let mut along = match dimjust {
        1 => t1 + inward * beside,
        2 => t2 - inward * beside,
        3 => t1,
        4 => t2,
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
            let offset =
                (defpt.x - first.x) * perpendicular_x + (defpt.y - first.y) * perpendicular_y;
            if offset >= 0.0 {
                1.0
            } else {
                -1.0
            }
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

/// Where a dimension's text sits by its style in this drawing (its
/// overrides applied) when the user has not placed it. Text beside an
/// extension line (DIMJUST 1/2) is measured by the glyphs of its font, not
/// by character cells.
pub(crate) fn automatic_text_position(document: &Document, dim: &Dimension) -> Option<Vector3> {
    let name = &dim.base().style_name;
    let source = document.dim_styles.iter().find(|s| {
        s.name.eq_ignore_ascii_case(name)
            || (name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
    })?;
    let style = resolved_dimension_style(source, dim, document);
    let scale = if style.dimscale > 1e-6 { style.dimscale } else { 1.0 };
    let height = style.dimtxt * scale;
    let mut position = dimension_text_pos_f64(dim, Some(&style), height, scale, Some(document));
    // Text beside an extension line, or a radius's text, which sits out past
    // its arc point two arrows and a gap from it.
    let outside = dimension_text_is_outside(dim, Some(&style));
    let measured = match dim {
        Dimension::Radius(_) => true,
        _ => matches!(style.dimjust, 1 | 2) && !outside,
    };
    if !measured {
        return Some(position);
    }
    let Some(ink) = dimension_text_ink_width(document, dim, &style, height) else {
        return Some(position);
    };
    // The layout spaced the text by character cells; it moves by half the
    // difference to its inked width, away from the line it sits beside
    // (or from the arc).
    let cells = dimension_text_cells(dim, Some(&style)).unwrap_or(0.0) * height * CELL_WIDTH;
    let half = (ink - cells) * 0.5;
    let axis = |first: Vector3, second: Vector3, defpt: Vector3, ax: f64, ay: f64| {
        let t1 = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
        let t2 = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;
        let towards_second = if t2 >= t1 { 1.0 } else { -1.0 };
        let inward = if style.dimjust == 1 { towards_second } else { -towards_second };
        (ax * half * inward, ay * half * inward)
    };
    match dim {
        Dimension::Linear(d) => {
            let (dx, dy) = axis(
                d.first_point,
                d.second_point,
                d.definition_point,
                d.rotation.cos(),
                d.rotation.sin(),
            );
            position.x += dx;
            position.y += dy;
        }
        Dimension::Aligned(d) => {
            let (ex, ey) = (d.second_point.x - d.first_point.x, d.second_point.y - d.first_point.y);
            let len = ex.hypot(ey).max(1e-12);
            let (dx, dy) =
                axis(d.first_point, d.second_point, d.definition_point, ex / len, ey / len);
            position.x += dx;
            position.y += dy;
        }
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => {
            if let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) {
                let toward_end = if end >= start { 1.0 } else { -1.0 };
                let sign = if style.dimjust == 1 { -toward_end } else { toward_end };
                let (sin, cos) = (half * sign / f64::from(radius).max(1e-12)).sin_cos();
                let (cx, cy) = (f64::from(vertex.x), f64::from(vertex.y));
                let (px, py) = (position.x - cx, position.y - cy);
                position.x = cx + px * cos - py * sin;
                position.y = cy + px * sin + py * cos;
            }
        }
        Dimension::Radius(d) => {
            let (dx, dy) = (
                d.definition_point.x - d.angle_vertex.x,
                d.definition_point.y - d.angle_vertex.y,
            );
            let len = dx.hypot(dy).max(1e-12);
            let reach = (2.0 * style.dimasz + style.dimgap.abs()) * scale + ink * 0.5;
            position.x = d.definition_point.x + dx / len * reach;
            position.y = d.definition_point.y + dy / len * reach;
        }
        _ => {}
    }
    Some(position)
}

/// The inked width of a dimension's text: its characters (formatting codes
/// left out) in its font (an `\F` font in the text, else the style's text
/// style) at `height`.
fn dimension_text_ink_width(
    document: &Document,
    dim: &Dimension,
    style: &DimStyle,
    height: f64,
) -> Option<f64> {
    let value = dimension_text_value(dim, Some(style))?;
    let mut plain = String::new();
    let mut font = None;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('F' | 'f') => {
                    let body: String = chars.by_ref().take_while(|c| *c != ';').collect();
                    font.get_or_insert_with(|| body.split('|').next().unwrap_or("").to_string());
                }
                Some('C' | 'c' | 'H' | 'h' | 'W' | 'w' | 'Q' | 'q' | 'T' | 't' | 'A' | 'a' | 'p') => {
                    chars.by_ref().find(|c| *c == ';');
                }
                Some('P') => plain.push(' '),
                Some('~') => plain.push(' '),
                Some(other @ ('\\' | '{' | '}')) => plain.push(other),
                _ => {}
            },
            '~' => plain.push(' '),
            other => plain.push(other),
        }
    }
    let font = font.filter(|f| !f.is_empty()).unwrap_or_else(|| {
        crate::entities::text_support::resolve_text_style(&style.dimtxsty, document).font_name
    });
    let bounds = crate::entities::text_support::text_local_bounds(
        &font,
        plain.trim(),
        height as f32,
        1.0,
        0.0,
    )?;
    Some(f64::from(bounds.ink_max[0] - bounds.ink_min[0]))
}

/// Clear an automatic text position so the dimension is laid out from its
/// current settings again. A loaded point is kept so a drawing looks as it
/// was drawn; a new dimension, or one whose style or overrides just changed,
/// has no valid one. Text the user placed keeps its point (#1412).
pub(crate) fn reset_automatic_text_position(base: &mut DimensionBase) {
    if !base.text_user_positioned {
        base.text_middle_point = Vector3::new(0.0, 0.0, 0.0);
    }
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

/// A dimensional constraint's dynamic dimension: the reference keeps it on
/// its own hidden layer and draws it by its own rules.
fn dynamic_constraint_dimension(dim: &Dimension) -> bool {
    crate::scene::parametric_constraints::is_dynamic_dimension_layer(&dim.base().common.layer)
}

/// How far a radial leader runs out from the rim before its hook, and how
/// long that hook is: about one text height, so the text always clears the
/// circle.
fn radial_leader_run(text_height: f64) -> f64 {
    text_height
}

/// Where a radius or diameter dimension's text sits once it no longer fits
/// inside: beside the circle, level with the end of the leader's run, its
/// near edge one arrow away so the hook has room.
fn radial_outside_text(
    tip: Vector3,
    ux: f64,
    uy: f64,
    text_height: f64,
    text_width: f64,
) -> Vector3 {
    let run = radial_leader_run(text_height);
    let elbow = Vector3::new(tip.x + ux * run, tip.y + uy * run, tip.z);
    let side = if ux >= 0.0 { 1.0 } else { -1.0 };
    Vector3::new(elbow.x + side * (run + text_width * 0.5), elbow.y, elbow.z)
}

/// Whether `point` lies on the diameter's leader line — the line through its
/// two defining points — rather than a DIMTAD offset away from it.
fn diameter_point_on_leader(
    d: &codec::entities::dimension::DimensionDiameter,
    point: Vector3,
    offset: f64,
) -> bool {
    point_on_line(d.angle_vertex, d.definition_point, point, offset)
}

/// Whether `point` lies on the line through `a` and `b` rather than an
/// `offset` (a DIMTAD lift) away from it.
fn point_on_line(a: Vector3, b: Vector3, point: Vector3, offset: f64) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx.hypot(dy);
    if length < 1e-12 {
        return true;
    }
    let across = ((point.x - a.x) * dy - (point.y - a.y) * dx) / length;
    across.abs() < offset.abs() * 0.5
}

/// `point` on the line through `a` and `b`, lifted `offset` to the side the
/// line's readable text stands on (below for DIMTAD 4).
fn lift_off_line(a: Vector3, b: Vector3, point: Vector3, offset: f64, dimtad: i16) -> Vector3 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt().max(1e-12);
    // Match the readable orientation used by text_on_dim_line().
    let (mut nx, mut ny) = (dx / len, dy / len);
    if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
        nx = -nx;
        ny = -ny;
    }
    let (px, py) = (-ny, nx);
    let sign = if dimtad == 4 { -1.0 } else { 1.0 };
    Vector3::new(
        point.x + px * offset * sign,
        point.y + py * offset * sign,
        point.z,
    )
}

/// DIMTOH / DIMTIH turn this dimension's text horizontal where it sits.
fn dimension_text_reads_horizontal(dim: &Dimension, style: Option<&DimStyle>) -> bool {
    let outside = dimension_text_is_outside(dim, style);
    style.is_some_and(|style| (outside && style.dimtoh) || (!outside && style.dimtih))
}

fn dimension_text_pos_f64(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
    document: Option<&Document>,
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

    if let Some(moved) =
        dimtmove_one_text(dim, style, text_height, dim_scale, document)
    {
        return moved.center;
    }
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
            // A radius places its text where it was clicked, on the line
            // through the centre and the arc point; DIMTAD lifts aligned text
            // off that line as it does on a diameter, and the line runs on
            // under it. A point already off the line is the text's own
            // middle, as a file saved elsewhere writes it. (#1675)
            Dimension::Radius(d)
                if dimtad != 0
                    && !dimension_text_reads_horizontal(dim, style)
                    && point_on_line(d.angle_vertex, d.definition_point, point, perp_off) =>
            {
                lift_off_line(d.angle_vertex, d.definition_point, point, perp_off, dimtad)
            }
            // A diameter whose text the user dragged keeps the grip point on
            // the leader and lifts the text by the DIMTAD offset (#1323).
            // A stored point already standing off the leader is the text's
            // own middle, as a file saved elsewhere writes it; lifting that
            // again floated the text a text height clear of its line (#898).
            Dimension::Diameter(d)
                if base.text_user_positioned
                    && dimtad != 0
                    && diameter_point_on_leader(d, point, perp_off) =>
            {
                lift_off_line(d.angle_vertex, d.definition_point, point, perp_off, dimtad)
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
                dimgap,
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
                dimgap,
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
            // Beside an extension line the text starts an arrow and three
            // gaps along the arc from it when it breaks the arc (one gap off
            // the arc): DIMJUST 1 by the second line, 2 by the first
            // (`text_w` carries a gap each side).
            let gaps = if dimtad == 0 { 2.0 } else { 0.0 };
            let beside =
                (arrow + gaps * dimgap + text_w * 0.5) / (radius as f64).max(1.0e-12);
            let toward_end = if end >= start { 1.0 } else { -1.0 };
            let angle = if move_outside {
                end as f64 + (text_w * 0.5 + dimgap + arrow) / (radius as f64).max(1.0e-12)
            } else if dimjust == 1 {
                end as f64 - toward_end * beside
            } else if dimjust == 2 {
                start as f64 + toward_end * beside
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
            if outside && dynamic_constraint_dimension(dim) {
                // A constraint's text moves beside the circle at the end of
                // its leader; an ordinary radius keeps its measured offset.
                return radial_outside_text(d.definition_point, ux, uy, text_height, text_w);
            }
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
            if dimension_text_is_outside(dim, style) && dynamic_constraint_dimension(dim) {
                // Beyond the end the dimension line was placed towards.
                return radial_outside_text(
                    d.angle_vertex,
                    -dx / len,
                    -dy / len,
                    text_height,
                    text_w,
                );
            }
            text_on_dim_line(
                d.angle_vertex,
                d.definition_point,
                d.angle_vertex,
                dx / len,
                dy / len,
                // DIMJUST places linear and angular text only.
                0,
                perp_off,
                text_w,
                dimgap,
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
            let direction_sign = if delta.dot(&text_axis) < 0.0 {
                -1.0
            } else {
                1.0
            };
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
    document: &Document,
) -> Option<DimGeom> {
    if !matches!(dimension, Dimension::LargeRadial(_)) {
        return None;
    }
    // Same local frame as the render path: the geometry is f32, so build it
    // around a nearby origin and let `DimGeom::origin` carry it back.
    let origin = dimension_precision_origin(dimension);
    let shifted = (origin != Vector3::ZERO).then(|| shift_dimension(dimension, origin));
    let dimension = shifted.as_ref().unwrap_or(dimension);
    let style_name = dimension.base().style_name.as_str();
    let effective_style = document
        .dim_styles
        .iter()
        .find(|style| {
            style.name.eq_ignore_ascii_case(style_name)
                || (style_name.trim().is_empty() && style.name.eq_ignore_ascii_case("Standard"))
        })
        .map(|style| resolved_dimension_style(style, dimension, document));
    let style = effective_style.as_ref();
    let scale = style
        .map(|style| dimension_style_scale(style, 1.0))
        .unwrap_or(1.0);
    let text_height = style
        .map(|style| style.dimtxt * scale)
        .unwrap_or(2.5 * scale);
    let arrow_size_raw = style
        .map(|style| style.dimasz * scale)
        .unwrap_or(0.18 * scale);
    let tick_size = style.map(|style| style.dimtsz * scale).unwrap_or(0.0);
    let arrow_size = (arrow_size_raw as f32).max(0.001);
    let arrow = if tick_size > 1.0e-9 {
        ArrowKind::Tick {
            size: (tick_size as f32).max(0.001),
            width: 0.0,
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
    let text = dimension_text_layout(dimension, style, text_height, scale, Some(document));
    let mut geometry = dimension_geometry(
        dimension,
        &arrow,
        &arrow,
        DimLineParams {
            dimexo: style
                .map(|style| (style.dimexo * scale) as f32)
                .unwrap_or(0.0),
            dimexe: style
                .map(|style| (style.dimexe * scale) as f32)
                .unwrap_or(0.0),
            dimdle: style
                .map(|style| (style.dimdle * scale) as f32)
                .unwrap_or(0.0),
            dimfxl: style
                .map(|style| (style.dimfxl * scale) as f32)
                .unwrap_or(1.0),
            dimfxlon: style.is_some_and(|style| style.dimfxlon),
            dimsoxd: style.is_some_and(|style| style.dimsoxd),
            dimcen: style
                .map(|style| (style.dimcen * scale) as f32)
                .unwrap_or(0.09),
            ticks: tick_size > 1.0e-9,
            arrow_len: arrow_size,
            text_width: text.width,
            text_height: text_height as f32,
            // A LargeRadial is never a dimensional constraint's dimension.
            constraint: false,
            dimatfit: style.map(|style| style.dimatfit).unwrap_or(3),
            dimtix: style.is_some_and(|style| style.dimtix),
            dimtofl: style.is_some_and(|style| style.dimtofl),
            text_position: text.position,
            horizontal_text: text.horizontal,
            text_lifted: style.map(|style| style.dimtad).unwrap_or(1) != 0,
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
    geometry.origin = origin;
    if !style.is_some_and(|style| style.dimse1) {
        if let Some(points) = crate::scene::dimension_assoc::radial_extension_points(
            document,
            dimension.base().common.handle,
            style.map(|style| style.dimexo * scale).unwrap_or(0.0),
            style.map(|style| style.dimexe * scale).unwrap_or(0.0),
        ) {
            let points: Vec<_> = points
                .into_iter()
                .map(|point| vec3_local(point_from_origin(point, origin)))
                .collect();
            add_polyline(&mut geometry.ext_lines, &points);
        }
    }
    if style.is_some_and(|style| style.dimtmove == 1) {
        if let Some((anchor, endpoint, under_text)) = dimtmove_leader_endpoints(dimension, text) {
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
        origin,
    );
    apply_dimension_breaks(
        document,
        dimension.base().common.handle,
        &mut geometry.ext_lines,
        origin,
    );
    Some(geometry)
}

/// Build measurement text for a saved dimension block.
pub(crate) fn baked_dimension_text_entity(
    dim: &Dimension,
    document: &Document,
    anno_scale: f64,
) -> Option<EntityType> {
    let (ent, _) = baked_dimension_text(dim, document, anno_scale)?;
    let mut ent = ent;
    if let EntityType::Text(t) = &mut ent {
        t.alignment_point = Some(t.insertion_point);
    }
    Some(ent)
}

/// The measurement text of a saved dimension block, with the DIMINSPECT
/// label, rate and frame when the dimension has them.
pub(crate) fn baked_dimension_text_entities(
    dim: &Dimension,
    document: &Document,
    anno_scale: f64,
) -> Vec<EntityType> {
    let Some((ent, gap)) = baked_dimension_text(dim, document, anno_scale) else {
        return Vec::new();
    };
    let mut entities =
        match crate::entities::dim_inspect::read(&dim.base().common.extended_data) {
            Some(inspection) => crate::entities::dim_inspect::layout(&inspection, ent, document, gap),
            None => vec![ent],
        };
    for ent in &mut entities {
        if let EntityType::Text(t) = ent {
            t.alignment_point = Some(t.insertion_point);
        }
    }
    entities
}

/// The measurement text entity and the scaled DIMGAP of a saved dimension.
fn baked_dimension_text(
    dim: &Dimension,
    document: &Document,
    anno_scale: f64,
) -> Option<(EntityType, f64)> {
    let style_name = dim.base().style_name.as_str();
    let effective_style = document
        .dim_styles
        .iter()
        .find(|style| {
            style.name.eq_ignore_ascii_case(style_name)
                || (style_name.trim().is_empty() && style.name.eq_ignore_ascii_case("Standard"))
        })
        .map(|style| resolved_dimension_style(style, dim, document));
    let style = effective_style.as_ref();
    let dim_scale = style
        .map(|style| dimension_style_scale(style, anno_scale))
        .unwrap_or(1.0);
    let dim_txt = style
        .map(|s| s.dimtxt * dim_scale)
        .unwrap_or(2.5 * dim_scale);
    let gap = style.map(|s| s.dimgap.abs()).unwrap_or(0.0) * dim_scale;
    Some((dimension_text_entity(dim, dim_txt, style, document, dim_scale)?, gap))
}

pub(crate) fn baked_arc_length_symbol_points(
    dim: &Dimension,
    document: &Document,
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

    arc_length_symbol_points(dim, style, text_height, dim_scale, symbol_position, Some(document))
        .unwrap_or_default()
        .into_iter()
        .map(|point| Vector3::new(point.x as f64, point.y as f64, point.z as f64))
        .collect()
}

pub(crate) fn dimension_text_grip_position(
    dim: &Dimension,
    document: &Document,
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
    use codec::types::Vector3;

    fn v(x: f64, y: f64) -> Vector3 {
        Vector3::new(x, y, 0.0)
    }

    // DIMTAD=Above must place text on the geometric "up" side of a horizontal
    // dimension whichever way the dimension runs; DIMTAD=Below flips it. (#144)
    #[test]
    fn above_is_up_regardless_of_direction() {
        let (first, second, defpt) = (v(0.0, 10.0), v(20.0, 10.0), v(0.0, 0.0));
        let perp = 5.0;
        let fwd = text_on_dim_line(
            first, second, defpt, 1.0, 0.0, 0, perp, 0.0, 0.0, 1.0, false, 3, 1, false,
        );
        let rev = text_on_dim_line(
            first, second, defpt, -1.0, 0.0, 0, perp, 0.0, 0.0, 1.0, false, 3, 1, false,
        );
        assert!(fwd.y > 0.0, "above must be +Y, got {}", fwd.y);
        assert!(rev.y > 0.0, "above must be +Y even reversed, got {}", rev.y);
        let below = text_on_dim_line(
            first, second, defpt, 1.0, 0.0, 0, perp, 0.0, 0.0, 1.0, false, 3, 4, false,
        );
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
    use codec::tables::DimStyle;

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
        assert_eq!(
            format_angular_value(45.0, Some(&style(4, 0))),
            "N 45\u{b0}0'0\" E"
        );
        assert_eq!(
            format_angular_value(135.0, Some(&style(4, 0))),
            "N 45\u{b0}0'0\" W"
        );
        assert_eq!(
            format_angular_value(200.0, Some(&style(4, 0))),
            "S 70\u{b0}0'0\" W"
        );
        assert_eq!(
            format_angular_value(300.0, Some(&style(4, 0))),
            "S 30\u{b0}0'0\" E"
        );
        // A hair under a cardinal must not quote a quarter-turn bearing.
        assert_eq!(format_angular_value(89.99999, Some(&style(4, 0))), "N");
        assert_eq!(format_angular_value(90.0, Some(&style(4, 0))), "N");
        assert_eq!(format_angular_value(0.0, Some(&style(4, 0))), "E");
    }

    // The other unit formats are untouched.
    #[test]
    fn other_angular_units_unchanged() {
        assert_eq!(
            format_angular_value(45.5, Some(&style(1, 0))),
            "45\u{b0}30'0\""
        );
        assert_eq!(format_angular_value(90.0, Some(&style(2, 1))), "100.0g");
    }
}

#[cfg(test)]
mod dim_color_override_tests {
    use super::dim_color_index;
    use crate::entities::dim_override::{DIMCLRD, DIMCLRE, DIMCLRT};
    use codec::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

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
    use codec::types::Vector3;

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

    /// Radius dimensions draw no leader (unchanged).
    #[test]
    fn radius_has_no_leader() {
        let r = codec::entities::DimensionRadius::new(v(0.0, 0.0), v(0.0, 5.0));
        assert!(dimtmove_leader_endpoints(
            &Dimension::Radius(r),
            layout(vec3_local(v(0.0, 8.0)), 2.0)
        )
        .is_none());
    }
}

#[cfg(test)]
mod linear_transform_tests {
    use super::*;
    use codec::entities::DimensionLinear;
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
        let document = Document::new();
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
            &rustc_hash::FxHashSet::default(),
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
    use codec::entities::DimensionLinear;

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
        assert_eq!(
            text_cells(r"\S1.020^0.980;\PCutouts", 1.0),
            7.0,
            "widest line, not the sum"
        );
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
        use codec::entities::DimensionAngular3Pt;
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
    use codec::entities::{DimensionDiameter, DimensionLinear, DimensionRadius};
    use glam::DVec3;

    /// A document whose Standard style has imperial sizes and horizontal text.
    fn document() -> Document {
        let mut document = Document::new();
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

    fn style(document: &Document) -> DimStyle {
        document
            .dim_styles
            .get("Standard")
            .expect("Standard style")
            .clone()
    }

    /// Stroke endpoints and arrowhead triangle count of the drawn dimension.
    fn drawn(document: &Document, dim: &Dimension) -> (Vec<DVec3>, usize) {
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
            &rustc_hash::FxHashSet::default(),
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

    /// Like [`drawn`], but reconstructs each vertex from its double-single
    /// pair — the coordinates the renderer actually shows. The high f32 alone
    /// is ~0.5 m off at UTM scale, which is exactly the defect under test.
    fn drawn_precise(document: &Document, dim: &Dimension) -> (Vec<DVec3>, usize) {
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
            &rustc_hash::FxHashSet::default(),
        );
        let mut points = Vec::new();
        let mut fills = 0;
        let collect = |high: &[[f32; 3]], low: &[[f32; 3]], points: &mut Vec<DVec3>| {
            for (index, p) in high.iter().enumerate() {
                if !p[0].is_finite() {
                    continue;
                }
                let l = low.get(index).copied().unwrap_or([0.0; 3]);
                points.push(DVec3::new(
                    p[0] as f64 + l[0] as f64,
                    p[1] as f64 + l[1] as f64,
                    p[2] as f64 + l[2] as f64,
                ));
            }
        };
        for wire in &wires {
            collect(&wire.points, &wire.points_low, &mut points);
            collect(&wire.fill_tris, &wire.fill_tris_low, &mut points);
            // Key vertices are dually used for snapping/picking; they must be
            // world-space like the rendered points.
            points.extend(
                wire.key_vertices
                    .iter()
                    .map(|p| DVec3::new(p[0], p[1], p[2])),
            );
            fills += wire.fill_tris.len() / 3;
        }
        (points, fills)
    }

    #[test]
    fn linear_dimension_stays_on_its_points_at_utm_scale() {
        let document = document();
        let mut local = DimensionLinear::horizontal(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
        local.definition_point = Vector3::new(0.0, 5.0, 0.0);
        local.base.definition_point = local.definition_point;

        let offset = Vector3::new(1_649_905.55, 4_945_694.94, 0.0);
        let mut moved = local.clone();
        moved.first_point = moved.first_point + offset;
        moved.second_point = moved.second_point + offset;
        moved.definition_point = moved.definition_point + offset;
        moved.base.definition_point = moved.base.definition_point + offset;

        let (local_points, local_fills) = drawn_precise(&document, &Dimension::Linear(local));
        let (moved_points, moved_fills) = drawn_precise(&document, &Dimension::Linear(moved));
        assert_eq!(local_points.len(), moved_points.len());
        assert_eq!(local_fills, moved_fills);
        let shift = DVec3::new(offset.x, offset.y, offset.z);
        for (local, moved) in local_points.iter().zip(&moved_points) {
            let moved = *moved - shift;
            assert!(
                local.distance(moved) < 1e-3,
                "local {local:?} vs translated-back {moved:?} (offset {:?})",
                local.distance(moved)
            );
        }
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
        let mut d =
            DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 5.0, 0.0);
        d.base.text_middle_point = Vector3::new(7.0, 5.2, 0.0);
        d.base.text_user_positioned = false;
        let dim = Dimension::Linear(d);
        let position = dimension_text_pos_f64(&dim, Some(&style(&document)), 0.18, 1.0, None);
        assert_eq!((position.x, position.y), (7.0, 5.2));
    }

    // Text pushed outside by a narrow gap sits past the two-arrow stub and a
    // gap, not directly against the arrowhead.
    #[test]
    fn outside_text_clears_the_stub_and_a_gap() {
        let document = document();
        let mut d =
            DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.2, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 1.0, 0.0);
        let dim = Dimension::Linear(d);
        let s = style(&document);
        let position = dimension_text_pos_f64(&dim, Some(&s), 0.18, 1.0, None);
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

    #[test]
    fn dimtmove_one_hangs_far_text_on_a_leader_from_the_line_middle() {
        // Dimension line y=2 from x=0 to 0.8; text three units above it, beyond
        // max(DIMASZ + DIMGAP, DIMTXT / 2 + DIMGAP + DIMEXE) of the line.
        let make = |movement, text_y| {
            let mut document = document();
            document.dim_styles.get_mut("Standard").unwrap().dimtmove = movement;
            let mut d = DimensionLinear::horizontal(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(0.8, 0.0, 0.0),
            );
            d.definition_point = Vector3::new(0.0, 2.0, 0.0);
            d.base.text_middle_point = Vector3::new(0.4, text_y, 0.0);
            d.base.text_user_positioned = true;
            drawn(&document, &Dimension::Linear(d)).0
        };
        let leader_start = |points: &[DVec3]| {
            points
                .iter()
                .any(|p| (p.x - 0.4).abs() <= 1e-6 && (p.y - 2.0).abs() <= 1e-6)
        };
        let far = make(1, 5.0);
        assert!(
            leader_start(&far),
            "DIMTMOVE=1 leader starts at the dimension line's middle: {far:?}"
        );
        assert!(
            !leader_start(&make(2, 5.0)),
            "DIMTMOVE=2 never draws a leader"
        );
        assert!(
            !leader_start(&make(1, 2.1)),
            "text near its dimension line keeps to it without a leader"
        );
    }

    // A negative DIMLFAC scales only a paper-space dimension that measures
    // model space through a viewport. The association record carries the
    // applied factor; zero means the dimension reads its sheet distance.
    #[test]
    fn negative_dimlfac_follows_the_recorded_association_factor() {
        use codec::xdata::{ExtendedDataRecord, XDataValue};
        let document = Document::new();
        let mut source = style(&document);
        source.dimlfac = -4.0;
        let mut d =
            DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(4.457, 0.0, 0.0));
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
        assert_eq!(
            through_viewport.dimlfac, 4.0,
            "the recorded factor, made positive"
        );
        let unknown = resolved_dimension_style(&source, &Dimension::Linear(d.clone()), &document);
        assert_eq!(
            unknown.dimlfac, 1.0,
            "model space without a record: unscaled"
        );
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
        let hook: Vec<f64> = points
            .iter()
            .filter(|p| (p.y - 1.5).abs() < 1e-6)
            .map(|p| p.x)
            .collect();
        let (lo, hi) = (
            hook.iter().cloned().fold(f64::MAX, f64::min),
            hook.iter().cloned().fold(f64::MIN, f64::max),
        );
        assert!(
            (lo - 3.0).abs() < 1e-6 && (hi - 3.18).abs() < 1e-6,
            "hook from the stored point one arrow toward the text: {lo}..{hi}"
        );
        let s = style(&document);
        let position = dimension_text_pos_f64(&dim, Some(&s), 0.18, 1.0, None);
        let half = dimension_text_cells(&dim, Some(&s)).unwrap() * 0.18 * CELL_WIDTH * 0.5;
        assert!(
            (position.x - half - 0.09 - 3.18).abs() < 1e-6,
            "text starts a gap past the hook end, got {}",
            position.x - half
        );
    }

    // Text stored well past the second extension line of a horizontal
    // dimension: the dimension line runs out to the text, not just two arrow
    // lengths.
    #[test]
    fn outside_text_on_the_axis_pulls_the_line_out_to_it() {
        let document = document();
        let mut d =
            DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.8, 0.0, 0.0));
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
            points
                .iter()
                .any(|p| (p.y - 1.0).abs() < 1e-6 && (p.x + 0.5).abs() < 1e-6),
            "the dimension line reaches the text height at the extension line"
        );
        assert!(
            points
                .iter()
                .any(|p| (p.y - 1.0).abs() < 1e-6 && (p.x - (-0.5 + 0.18)).abs() < 1e-6),
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
        assert!(
            hook.len() >= 2,
            "a horizontal hook at the text height: {hook:?}"
        );
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
        assert!(
            points.iter().any(|p| (p.y - 0.8).abs() < 1e-6),
            "hook at the text height"
        );
    }
}

/// The dimension an entity is, if it is one. The variant holds it boxed, so
/// a pattern cannot reach into it; match on this instead.
pub(crate) fn as_dimension(entity: &codec::EntityType) -> Option<&codec::entities::Dimension> {
    match entity {
        codec::EntityType::Dimension(dimension) => Some(dimension),
        _ => None,
    }
}

/// [`as_dimension`], mutably.
pub(crate) fn as_dimension_mut(
    entity: &mut codec::EntityType,
) -> Option<&mut codec::entities::Dimension> {
    match entity {
        codec::EntityType::Dimension(dimension) => Some(dimension),
        _ => None,
    }
}

#[cfg(test)]
mod text_width_factor_tests {
    use super::*;
    use codec::entities::DimensionLinear;
    use codec::tables::{DimStyle as DimStyleRecord, TextStyle};
    use codec::types::Vector3;

    #[test]
    fn dimension_text_takes_its_style_width_factor() {
        // A drawing whose dimension style writes with a 0.6-wide text style:
        // AutoCAD 2025 draws the measurement 0.6 as wide; OCS drew it at 1.0.
        let mut doc = Document::default();
        let mut narrow = TextStyle::new("narrow");
        narrow.handle = doc.allocate_handle();
        narrow.width_factor = 0.6;
        let narrow_handle = narrow.handle;
        doc.text_styles.add(narrow).unwrap();
        let mut style = DimStyleRecord::new("NarrowDims");
        style.dimtxsty = "narrow".to_string();
        style.dimtxsty_handle = narrow_handle;
        doc.dim_styles.add(style).unwrap();

        let mut d =
            DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(200.0, 0.0, 0.0));
        d.base.style_name = "NarrowDims".to_string();
        let Some(EntityType::Text(text)) =
            baked_dimension_text_entity(&Dimension::Linear(d), &doc, 1.0)
        else {
            panic!("a plain measurement is synthesized as TEXT");
        };
        assert!(
            (text.width_factor - 0.6).abs() < 1.0e-6,
            "dimension text uses its style's width factor, got {}",
            text.width_factor
        );
    }
}
