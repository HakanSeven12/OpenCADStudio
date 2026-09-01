use acadrust::entities::Solid3D;
use acadrust::objects::{
    DynamicBlockData, ObjectType, SolidHistoryBox, SolidHistoryBrep, SolidHistoryCone,
    SolidHistoryCylinder, SolidHistoryNodeBase, SolidHistoryOperation,
    SolidHistoryPyramid, SolidHistorySphere, SolidHistorySweep, SolidHistoryTorus,
};
use cadkernel::brep::Body;

use crate::command::EntityTransform;
use crate::scene::model::object::{
    GripApply, GripDef, GripShape, PropSection, PropValue, Property,
};
use crate::t;

pub const GRIP_LENGTH: usize = 10_001;
pub const GRIP_WIDTH: usize = 10_002;
pub const GRIP_HEIGHT: usize = 10_003;
pub const GRIP_RADIUS: usize = 10_004;
pub const GRIP_OUTER_RADIUS: usize = 10_005;
pub const GRIP_INNER_RADIUS: usize = 10_006;
pub const GRIP_SIDES: usize = 10_007;
pub const GRIP_MAJOR_RADIUS: usize = 10_008;
pub const GRIP_MINOR_RADIUS: usize = 10_009;
pub const GRIP_TOP_RADIUS: usize = 10_010;
pub const GRIP_POSITION: usize = 10_011;
pub const GRIP_BOX_CORNER_FIRST: usize = 10_100;
pub const GRIP_BOX_FACE_X_MIN: usize = 10_110;
pub const GRIP_BOX_FACE_X_MAX: usize = 10_111;
pub const GRIP_BOX_FACE_Y_MIN: usize = 10_112;
pub const GRIP_BOX_FACE_Y_MAX: usize = 10_113;
pub const GRIP_BOX_FACE_Z_MIN: usize = 10_114;
pub const GRIP_BOX_FACE_Z_MAX: usize = 10_115;

pub const PROP_LENGTH: &str = "solid_history_length";
pub const PROP_WIDTH: &str = "solid_history_width";
pub const PROP_HEIGHT: &str = "solid_history_height";
pub const PROP_RADIUS: &str = "solid_history_radius";
pub const PROP_DIAMETER: &str = "solid_history_diameter";
pub const PROP_BASE_RADIUS: &str = "solid_history_base_radius";
pub const PROP_TOP_RADIUS: &str = "solid_history_top_radius";
pub const PROP_BASE_MAJOR_RADIUS: &str = "solid_history_base_major_radius";
pub const PROP_BASE_MINOR_RADIUS: &str = "solid_history_base_minor_radius";
pub const PROP_TOP_MAJOR_RADIUS: &str = "solid_history_top_major_radius";
pub const PROP_TOP_MINOR_RADIUS: &str = "solid_history_top_minor_radius";
pub const PROP_MAJOR_RADIUS: &str = "solid_history_major_radius";
pub const PROP_MINOR_RADIUS: &str = "solid_history_minor_radius";
pub const PROP_ELLIPTICAL: &str = "solid_history_elliptical";
pub const PROP_OUTER_RADIUS: &str = "solid_history_outer_radius";
pub const PROP_INNER_RADIUS: &str = "solid_history_inner_radius";
pub const PROP_SIDES: &str = "solid_history_sides";
pub const PROP_POSITION_X: &str = "solid_history_position_x";
pub const PROP_POSITION_Y: &str = "solid_history_position_y";
pub const PROP_POSITION_Z: &str = "solid_history_position_z";
pub const PROP_ROTATION: &str = "solid_history_rotation";
pub const PROP_PROFILE_ROTATION: &str = "solid_history_profile_rotation";
pub const PROP_BANK: &str = "solid_history_bank";
pub const PROP_TWIST_ALONG_PATH: &str = "solid_history_twist_along_path";
pub const PROP_SCALE_ALONG_PATH: &str = "solid_history_scale_along_path";
pub const PROP_SWEEP_LENGTH: &str = "solid_history_sweep_length";
pub const PROP_PYRAMID_TYPE: &str = "solid_history_pyramid_type";
pub const PROP_HISTORY: &str = "solid_history_record";
pub const PROP_SHOW_HISTORY: &str = "solid_history_show";

fn history_prop(label: &str, field: &'static str, value: impl ToString) -> Property {
    Property {
        label: label.to_string(),
        field,
        value: PropValue::EditText(value.to_string()),
    }
}

fn history_flags(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
) -> Option<(bool, bool, i16)> {
    let graph = document.solid_history_graph(handle)?;
    let ObjectType::DynamicBlock(object) = document.objects.get(&graph.root)? else {
        return None;
    };
    let DynamicBlockData::SolidHistory(history) = &object.data else {
        return None;
    };
    Some((
        history.record_history,
        history.show_history,
        document.header.show_solid_history.clamp(0, 2),
    ))
}

fn displayed_history_state(object_show_history: bool, show_history_mode: i16) -> (bool, bool) {
    match show_history_mode {
        0 => (false, false),
        2 => (true, false),
        _ => (object_show_history, true),
    }
}

pub fn has_specialized_primitive_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
) -> bool {
    matches!(
        document.solid_history_operation(handle),
        Some(
            SolidHistoryOperation::Box(_)
                | SolidHistoryOperation::Wedge(_)
                | SolidHistoryOperation::Sphere(_)
                | SolidHistoryOperation::Cone(_)
                | SolidHistoryOperation::Cylinder(_)
                | SolidHistoryOperation::Torus(_)
                | SolidHistoryOperation::Pyramid(_)
                | SolidHistoryOperation::Sweep(_)
        )
    )
}

pub fn reference_point(operation: &SolidHistoryOperation) -> Option<glam::DVec3> {
    match operation {
        SolidHistoryOperation::Box(value) | SolidHistoryOperation::Wedge(value) => world_point(
            value.base.transform,
            [value.length * 0.5, value.width * 0.5, 0.0],
        ),
        SolidHistoryOperation::Sphere(value) => {
            world_point(value.base.transform, [0.0, 0.0, 0.0])
        }
        SolidHistoryOperation::Cone(value) => world_point(value.base.transform, [0.0; 3]),
        SolidHistoryOperation::Cylinder(value) => world_point(value.base.transform, [0.0; 3]),
        SolidHistoryOperation::Sweep(value) => world_point(
            value.base.transform,
            [
                value.reference_point.x,
                value.reference_point.y,
                value.reference_point.z,
            ],
        ),
        SolidHistoryOperation::Torus(value) => world_point(value.base.transform, [0.0; 3]),
        SolidHistoryOperation::Pyramid(value) => world_point(value.base.transform, [0.0; 3]),
        _ => None,
    }
}

fn sweep_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
    value: &SolidHistorySweep,
) -> Vec<PropSection> {
    let (record_history, object_show_history, show_history_mode) =
        history_flags(document, handle).unwrap_or((false, false, 1));
    let (show_history, show_history_editable) =
        displayed_history_state(object_show_history, show_history_mode);
    let show_value = if show_history { "Yes" } else { "No" };
    let length = sweep_path_length(value)
        .map(crate::entities::common::format_length)
        .unwrap_or_default();
    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                Property {
                    label: t!("Solid type").into_owned(),
                    field: "solid_history_type",
                    value: PropValue::ReadOnly(t!("Sweep").into_owned()),
                },
                Property {
                    label: t!("Profile rotation").into_owned(),
                    field: PROP_PROFILE_ROTATION,
                    value: PropValue::EditText(
                        crate::entities::common::format_direction(value.align_angle),
                    ),
                },
                Property {
                    label: t!("Bank").into_owned(),
                    field: PROP_BANK,
                    value: PropValue::ReadOnly(
                        if value.bank { "Yes" } else { "No" }.to_string(),
                    ),
                },
                Property {
                    label: t!("Twist along path").into_owned(),
                    field: PROP_TWIST_ALONG_PATH,
                    value: PropValue::EditText(
                        crate::entities::common::format_angle(value.twist_angle),
                    ),
                },
                Property {
                    label: t!("Scale along path").into_owned(),
                    field: PROP_SCALE_ALONG_PATH,
                    value: PropValue::EditText(value.scale_factor.to_string()),
                },
                Property {
                    label: t!("Length").into_owned(),
                    field: PROP_SWEEP_LENGTH,
                    value: PropValue::ReadOnly(length),
                },
            ],
        },
        PropSection {
            title: t!("Solid History").into_owned(),
            props: vec![
                Property {
                    label: t!("History").into_owned(),
                    field: PROP_HISTORY,
                    value: PropValue::Choice {
                        selected: if record_history { "Record" } else { "None" }.to_string(),
                        options: vec!["None".to_string(), "Record".to_string()],
                    },
                },
                Property {
                    label: t!("Show History").into_owned(),
                    field: PROP_SHOW_HISTORY,
                    value: if show_history_editable {
                        PropValue::Choice {
                            selected: show_value.to_string(),
                            options: vec!["No".to_string(), "Yes".to_string()],
                        }
                    } else {
                        PropValue::ReadOnly(show_value.to_string())
                    },
                },
            ],
        },
    ]
}

fn sweep_path_length(value: &SolidHistorySweep) -> Option<f64> {
    use acadrust::entities::EmbeddedEntity;
    use acadrust::EntityType;

    let entity = match value.path_entity.as_ref()? {
        EmbeddedEntity::Line(value) => EntityType::Line(value.clone()),
        EmbeddedEntity::Arc(value) => EntityType::Arc(value.clone()),
        EmbeddedEntity::Circle(value) => EntityType::Circle(value.clone()),
        EmbeddedEntity::Ellipse(value) => EntityType::Ellipse(value.clone()),
        EmbeddedEntity::Spline(value) => EntityType::Spline(value.clone()),
        EmbeddedEntity::LwPolyline(value) => EntityType::LwPolyline(value.clone()),
        _ => return None,
    };
    let curve = crate::entities::curve::entity_curve(&entity)?;
    let transform = matrix(value.path_entity_transform)?;
    let x_scale = transform
        .transform_vector3(glam::DVec3::from_array(curve.plane.x_axis))
        .length();
    let y_scale = transform
        .transform_vector3(glam::DVec3::from_array(curve.plane.y_axis))
        .length();
    if !x_scale.is_finite()
        || !y_scale.is_finite()
        || (x_scale - y_scale).abs() > 1e-9 * x_scale.max(y_scale).max(1.0)
    {
        return None;
    }
    let length = curve.curve.length() * (x_scale + y_scale) * 0.5;
    (length.is_finite() && length >= 0.0).then_some(length)
}

fn torus_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
    value: &SolidHistoryTorus,
) -> Vec<PropSection> {
    let Some(position) = world_point(value.base.transform, [0.0; 3]) else {
        return Vec::new();
    };
    let (record_history, object_show_history, show_history_mode) =
        history_flags(document, handle).unwrap_or((false, false, 1));
    let (show_history, _) = displayed_history_state(object_show_history, show_history_mode);
    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                Property {
                    label: t!("Solid type").into_owned(),
                    field: "solid_history_type",
                    value: PropValue::ReadOnly(t!("Torus").into_owned()),
                },
                crate::entities::common::edit_prop(
                    t!("Position X").as_ref(),
                    PROP_POSITION_X,
                    position.x,
                ),
                crate::entities::common::edit_prop(
                    t!("Position Y").as_ref(),
                    PROP_POSITION_Y,
                    position.y,
                ),
                crate::entities::common::edit_prop(
                    t!("Position Z").as_ref(),
                    PROP_POSITION_Z,
                    position.z,
                ),
                history_prop(
                    t!("Torus radius").as_ref(),
                    PROP_MAJOR_RADIUS,
                    value.major_radius,
                ),
                history_prop(
                    t!("Tube radius").as_ref(),
                    PROP_MINOR_RADIUS,
                    value.minor_radius,
                ),
            ],
        },
        PropSection {
            title: t!("Solid History").into_owned(),
            props: vec![
                Property {
                    label: t!("History").into_owned(),
                    field: PROP_HISTORY,
                    value: PropValue::ReadOnly(
                        if record_history { "Record" } else { "None" }.to_string(),
                    ),
                },
                Property {
                    label: t!("Show History").into_owned(),
                    field: PROP_SHOW_HISTORY,
                    value: PropValue::ReadOnly(
                        if show_history { "Yes" } else { "No" }.to_string(),
                    ),
                },
            ],
        },
    ]
}

fn cone_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
    value: &SolidHistoryCone,
) -> Vec<PropSection> {
    let Some(position) = world_point(value.base.transform, [0.0; 3]) else {
        return Vec::new();
    };
    let scale = value
        .base_x_radius
        .abs()
        .max(value.base_y_radius.abs())
        .max(1.0);
    let elliptical = (value.base_x_radius - value.base_y_radius).abs() > 1e-9 * scale;
    let rotation = matrix(value.base.transform)
        .map(|matrix| matrix.x_axis.y.atan2(matrix.x_axis.x))
        .unwrap_or(0.0);
    let top_minor_radius = if value.base_x_radius.abs() > 1e-9 {
        value.top_radius * value.base_y_radius / value.base_x_radius
    } else {
        0.0
    };
    let (record_history, object_show_history, show_history_mode) =
        history_flags(document, handle).unwrap_or((false, false, 1));
    let (show_history, show_history_editable) =
        displayed_history_state(object_show_history, show_history_mode);
    let show_value = if show_history { "Yes" } else { "No" };
    let mut geometry = vec![
        Property {
            label: t!("Solid type").into_owned(),
            field: "solid_history_type",
            value: PropValue::ReadOnly(t!("Cone").into_owned()),
        },
        crate::entities::common::edit_prop(
            t!("Position X").as_ref(),
            PROP_POSITION_X,
            position.x,
        ),
        crate::entities::common::edit_prop(
            t!("Position Y").as_ref(),
            PROP_POSITION_Y,
            position.y,
        ),
        crate::entities::common::edit_prop(
            t!("Position Z").as_ref(),
            PROP_POSITION_Z,
            position.z,
        ),
        Property {
            label: t!("Elliptical").into_owned(),
            field: PROP_ELLIPTICAL,
            value: PropValue::ReadOnly(if elliptical { "Yes" } else { "No" }.to_string()),
        },
    ];
    if elliptical {
        geometry.extend([
            history_prop(
                t!("Base major radius").as_ref(),
                PROP_BASE_MAJOR_RADIUS,
                value.base_x_radius,
            ),
            history_prop(
                t!("Base minor radius").as_ref(),
                PROP_BASE_MINOR_RADIUS,
                value.base_y_radius,
            ),
            history_prop(
                t!("Top major radius").as_ref(),
                PROP_TOP_MAJOR_RADIUS,
                value.top_radius,
            ),
            history_prop(
                t!("Top minor radius").as_ref(),
                PROP_TOP_MINOR_RADIUS,
                top_minor_radius,
            ),
            Property {
                label: t!("Rotation").into_owned(),
                field: PROP_ROTATION,
                value: PropValue::ReadOnly(crate::entities::common::format_direction(rotation)),
            },
        ]);
    } else {
        geometry.extend([
            history_prop(
                t!("Base radius").as_ref(),
                PROP_BASE_RADIUS,
                value.base_x_radius,
            ),
            history_prop(
                t!("Top radius").as_ref(),
                PROP_TOP_RADIUS,
                value.top_radius,
            ),
        ]);
    }
    geometry.push(history_prop(
        t!("Height").as_ref(),
        PROP_HEIGHT,
        value.height,
    ));
    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: geometry,
        },
        PropSection {
            title: t!("Solid History").into_owned(),
            props: vec![
                Property {
                    label: t!("History").into_owned(),
                    field: PROP_HISTORY,
                    value: PropValue::Choice {
                        selected: if record_history { "Record" } else { "None" }.to_string(),
                        options: vec!["None".to_string(), "Record".to_string()],
                    },
                },
                Property {
                    label: t!("Show History").into_owned(),
                    field: PROP_SHOW_HISTORY,
                    value: if show_history_editable {
                        PropValue::Choice {
                            selected: show_value.to_string(),
                            options: vec!["No".to_string(), "Yes".to_string()],
                        }
                    } else {
                        PropValue::ReadOnly(show_value.to_string())
                    },
                },
            ],
        },
    ]
}

fn cylinder_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
    value: &SolidHistoryCylinder,
) -> Vec<PropSection> {
    let Some(position) = world_point(value.base.transform, [0.0; 3]) else {
        return Vec::new();
    };
    let scale = value
        .major_radius
        .abs()
        .max(value.minor_radius.abs())
        .max(1.0);
    let elliptical = (value.major_radius - value.minor_radius).abs() > 1e-9 * scale;
    let rotation = matrix(value.base.transform)
        .map(|matrix| matrix.x_axis.y.atan2(matrix.x_axis.x))
        .unwrap_or(0.0);
    let (record_history, object_show_history, show_history_mode) =
        history_flags(document, handle).unwrap_or((false, false, 1));
    let (show_history, show_history_editable) =
        displayed_history_state(object_show_history, show_history_mode);
    let show_value = if show_history { "Yes" } else { "No" };
    let mut geometry = vec![
        Property {
            label: t!("Solid type").into_owned(),
            field: "solid_history_type",
            value: PropValue::ReadOnly(t!("Cylinder").into_owned()),
        },
        crate::entities::common::edit_prop(
            t!("Position X").as_ref(),
            PROP_POSITION_X,
            position.x,
        ),
        crate::entities::common::edit_prop(
            t!("Position Y").as_ref(),
            PROP_POSITION_Y,
            position.y,
        ),
        crate::entities::common::edit_prop(
            t!("Position Z").as_ref(),
            PROP_POSITION_Z,
            position.z,
        ),
        Property {
            label: t!("Elliptical").into_owned(),
            field: PROP_ELLIPTICAL,
            value: PropValue::ReadOnly(if elliptical { "Yes" } else { "No" }.to_string()),
        },
    ];
    if elliptical {
        geometry.extend([
            history_prop(
                t!("Major radius").as_ref(),
                PROP_MAJOR_RADIUS,
                value.major_radius,
            ),
            history_prop(
                t!("Minor radius").as_ref(),
                PROP_MINOR_RADIUS,
                value.minor_radius,
            ),
            Property {
                label: t!("Rotation").into_owned(),
                field: PROP_ROTATION,
                value: PropValue::ReadOnly(crate::entities::common::format_direction(rotation)),
            },
        ]);
    } else {
        geometry.push(history_prop(
            t!("Radius").as_ref(),
            PROP_RADIUS,
            value.major_radius,
        ));
    }
    geometry.push(history_prop(
        t!("Height").as_ref(),
        PROP_HEIGHT,
        value.height,
    ));
    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: geometry,
        },
        PropSection {
            title: t!("Solid History").into_owned(),
            props: vec![
                Property {
                    label: t!("History").into_owned(),
                    field: PROP_HISTORY,
                    value: PropValue::Choice {
                        selected: if record_history { "Record" } else { "None" }.to_string(),
                        options: vec!["None".to_string(), "Record".to_string()],
                    },
                },
                Property {
                    label: t!("Show History").into_owned(),
                    field: PROP_SHOW_HISTORY,
                    value: if show_history_editable {
                        PropValue::Choice {
                            selected: show_value.to_string(),
                            options: vec!["No".to_string(), "Yes".to_string()],
                        }
                    } else {
                        PropValue::ReadOnly(show_value.to_string())
                    },
                },
            ],
        },
    ]
}

fn pyramid_is_inscribed(value: &SolidHistoryPyramid) -> bool {
    value.operation_minor == -1
}

fn pyramid_apothem_factor(sides: i32) -> f64 {
    (std::f64::consts::PI / sides.clamp(3, 32) as f64).cos()
}

fn pyramid_display_radius(value: &SolidHistoryPyramid, radius: f64) -> f64 {
    if pyramid_is_inscribed(value) {
        radius
    } else {
        radius * pyramid_apothem_factor(value.sides)
    }
}

fn pyramid_display_rotation(value: &SolidHistoryPyramid) -> Option<f64> {
    let vertex_rotation = matrix(value.base.transform)
        .map(|matrix| matrix.x_axis.y.atan2(matrix.x_axis.x))?;
    if pyramid_is_inscribed(value) {
        Some(vertex_rotation)
    } else {
        Some(vertex_rotation + std::f64::consts::PI / value.sides.clamp(3, 32) as f64)
    }
}

fn pyramid_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
    value: &SolidHistoryPyramid,
) -> Vec<PropSection> {
    let Some(position) = world_point(value.base.transform, [0.0; 3]) else {
        return Vec::new();
    };
    let rotation = pyramid_display_rotation(value).unwrap_or(0.0);
    let (record_history, object_show_history, show_history_mode) =
        history_flags(document, handle).unwrap_or((false, false, 1));
    let (show_history, show_history_editable) =
        displayed_history_state(object_show_history, show_history_mode);
    let show_value = if show_history { "Yes" } else { "No" };
    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                Property {
                    label: t!("Solid type").into_owned(),
                    field: "solid_history_type",
                    value: PropValue::ReadOnly(t!("Pyramid").into_owned()),
                },
                crate::entities::common::edit_prop(
                    t!("Position X").as_ref(),
                    PROP_POSITION_X,
                    position.x,
                ),
                crate::entities::common::edit_prop(
                    t!("Position Y").as_ref(),
                    PROP_POSITION_Y,
                    position.y,
                ),
                crate::entities::common::edit_prop(
                    t!("Position Z").as_ref(),
                    PROP_POSITION_Z,
                    position.z,
                ),
                Property {
                    label: t!("Type").into_owned(),
                    field: PROP_PYRAMID_TYPE,
                    value: PropValue::Choice {
                        selected: if pyramid_is_inscribed(value) {
                            "Inscribed"
                        } else {
                            "Circumscribed"
                        }
                        .to_string(),
                        options: vec!["Circumscribed".to_string(), "Inscribed".to_string()],
                    },
                },
                history_prop(
                    t!("Base radius").as_ref(),
                    PROP_BASE_RADIUS,
                    pyramid_display_radius(value, value.radius),
                ),
                history_prop(
                    t!("Top radius").as_ref(),
                    PROP_TOP_RADIUS,
                    pyramid_display_radius(value, value.top_radius),
                ),
                history_prop(t!("Sides").as_ref(), PROP_SIDES, value.sides),
                Property {
                    label: t!("Rotation").into_owned(),
                    field: PROP_ROTATION,
                    value: PropValue::EditText(
                        crate::entities::common::format_direction(rotation),
                    ),
                },
                history_prop(t!("Height").as_ref(), PROP_HEIGHT, value.height),
            ],
        },
        PropSection {
            title: t!("Solid History").into_owned(),
            props: vec![
                Property {
                    label: t!("History").into_owned(),
                    field: PROP_HISTORY,
                    value: PropValue::Choice {
                        selected: if record_history { "Record" } else { "None" }.to_string(),
                        options: vec!["None".to_string(), "Record".to_string()],
                    },
                },
                Property {
                    label: t!("Show History").into_owned(),
                    field: PROP_SHOW_HISTORY,
                    value: if show_history_editable {
                        PropValue::Choice {
                            selected: show_value.to_string(),
                            options: vec!["No".to_string(), "Yes".to_string()],
                        }
                    } else {
                        PropValue::ReadOnly(show_value.to_string())
                    },
                },
            ],
        },
    ]
}

fn sphere_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
    value: &SolidHistorySphere,
) -> Vec<PropSection> {
    let Some(position) = world_point(value.base.transform, [0.0; 3]) else {
        return Vec::new();
    };
    let (record_history, object_show_history, show_history_mode) =
        history_flags(document, handle).unwrap_or((false, false, 1));
    let (show_history, show_history_editable) =
        displayed_history_state(object_show_history, show_history_mode);
    let show_value = if show_history { "Yes" } else { "No" };
    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                Property {
                    label: t!("Solid type").into_owned(),
                    field: "solid_history_type",
                    value: PropValue::ReadOnly(t!("Sphere").into_owned()),
                },
                crate::entities::common::edit_prop(
                    t!("Position X").as_ref(),
                    PROP_POSITION_X,
                    position.x,
                ),
                crate::entities::common::edit_prop(
                    t!("Position Y").as_ref(),
                    PROP_POSITION_Y,
                    position.y,
                ),
                crate::entities::common::edit_prop(
                    t!("Position Z").as_ref(),
                    PROP_POSITION_Z,
                    position.z,
                ),
                crate::entities::common::edit_prop(
                    t!("Radius").as_ref(),
                    PROP_RADIUS,
                    value.radius,
                ),
                crate::entities::common::edit_prop(
                    t!("Diameter").as_ref(),
                    PROP_DIAMETER,
                    value.radius * 2.0,
                ),
            ],
        },
        PropSection {
            title: t!("Solid History").into_owned(),
            props: vec![
                Property {
                    label: t!("History").into_owned(),
                    field: PROP_HISTORY,
                    value: PropValue::Choice {
                        selected: if record_history { "Record" } else { "None" }.to_string(),
                        options: vec!["None".to_string(), "Record".to_string()],
                    },
                },
                Property {
                    label: t!("Show History").into_owned(),
                    field: PROP_SHOW_HISTORY,
                    value: if show_history_editable {
                        PropValue::Choice {
                            selected: show_value.to_string(),
                            options: vec!["No".to_string(), "Yes".to_string()],
                        }
                    } else {
                        PropValue::ReadOnly(show_value.to_string())
                    },
                },
            ],
        },
    ]
}

fn rectangular_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
    value: &SolidHistoryBox,
    solid_type: &str,
) -> Vec<PropSection> {
    let Some(position) = world_point(
        value.base.transform,
        [value.length * 0.5, value.width * 0.5, 0.0],
    ) else {
        return Vec::new();
    };
    let rotation = matrix(value.base.transform)
        .map(|matrix| matrix.x_axis.y.atan2(matrix.x_axis.x))
        .unwrap_or(0.0);
    let (record_history, object_show_history, show_history_mode) =
        history_flags(document, handle).unwrap_or((false, false, 1));
    let (show_history, show_history_editable) =
        displayed_history_state(object_show_history, show_history_mode);
    let show_value = if show_history { "Yes" } else { "No" };
    vec![
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                Property {
                    label: t!("Solid type").into_owned(),
                    field: "solid_history_type",
                    value: PropValue::ReadOnly(t!(solid_type).into_owned()),
                },
                crate::entities::common::edit_prop(
                    t!("Position X").as_ref(),
                    PROP_POSITION_X,
                    position.x,
                ),
                crate::entities::common::edit_prop(
                    t!("Position Y").as_ref(),
                    PROP_POSITION_Y,
                    position.y,
                ),
                crate::entities::common::edit_prop(
                    t!("Position Z").as_ref(),
                    PROP_POSITION_Z,
                    position.z,
                ),
                crate::entities::common::edit_prop(
                    t!("Length").as_ref(),
                    PROP_LENGTH,
                    value.length,
                ),
                crate::entities::common::edit_prop(
                    t!("Width").as_ref(),
                    PROP_WIDTH,
                    value.width,
                ),
                crate::entities::common::edit_prop(
                    t!("Height").as_ref(),
                    PROP_HEIGHT,
                    value.height,
                ),
                Property {
                    label: t!("Rotation").into_owned(),
                    field: PROP_ROTATION,
                    value: PropValue::EditText(
                        crate::entities::common::format_direction(rotation),
                    ),
                },
            ],
        },
        PropSection {
            title: t!("Solid History").into_owned(),
            props: vec![
                Property {
                    label: t!("History").into_owned(),
                    field: PROP_HISTORY,
                    value: PropValue::Choice {
                        selected: if record_history { "Record" } else { "None" }.to_string(),
                        options: vec!["None".to_string(), "Record".to_string()],
                    },
                },
                Property {
                    label: t!("Show History").into_owned(),
                    field: PROP_SHOW_HISTORY,
                    value: if show_history_editable {
                        PropValue::Choice {
                            selected: show_value.to_string(),
                            options: vec!["No".to_string(), "Yes".to_string()],
                        }
                    } else {
                        PropValue::ReadOnly(show_value.to_string())
                    },
                },
            ],
        },
    ]
}

pub fn primitive_properties(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
) -> Vec<PropSection> {
    let Some(operation) = document.solid_history_operation(handle) else {
        return Vec::new();
    };
    match operation {
        SolidHistoryOperation::Box(value) => {
            rectangular_properties(document, handle, value, "Box")
        }
        SolidHistoryOperation::Wedge(value) => {
            rectangular_properties(document, handle, value, "Wedge")
        }
        SolidHistoryOperation::Sphere(value) => sphere_properties(document, handle, value),
        SolidHistoryOperation::Cone(value) => cone_properties(document, handle, value),
        SolidHistoryOperation::Cylinder(value) => cylinder_properties(document, handle, value),
        SolidHistoryOperation::Pyramid(value) => pyramid_properties(document, handle, value),
        SolidHistoryOperation::Torus(value) => torus_properties(document, handle, value),
        SolidHistoryOperation::Sweep(value) => sweep_properties(document, handle, value),
        _ => Vec::new(),
    }
}

pub fn is_primitive_property(field: &str) -> bool {
    matches!(
        field,
        PROP_LENGTH
            | PROP_WIDTH
            | PROP_HEIGHT
            | PROP_RADIUS
            | PROP_DIAMETER
            | PROP_BASE_RADIUS
            | PROP_TOP_RADIUS
            | PROP_BASE_MAJOR_RADIUS
            | PROP_BASE_MINOR_RADIUS
            | PROP_TOP_MAJOR_RADIUS
            | PROP_TOP_MINOR_RADIUS
            | PROP_MAJOR_RADIUS
            | PROP_MINOR_RADIUS
            | PROP_OUTER_RADIUS
            | PROP_INNER_RADIUS
            | PROP_SIDES
            | PROP_POSITION_X
            | PROP_POSITION_Y
            | PROP_POSITION_Z
            | PROP_ROTATION
            | PROP_PROFILE_ROTATION
            | PROP_TWIST_ALONG_PATH
            | PROP_SCALE_ALONG_PATH
            | PROP_PYRAMID_TYPE
    )
}

pub fn is_history_choice(field: &str) -> bool {
    matches!(field, PROP_HISTORY | PROP_SHOW_HISTORY)
}

pub fn is_specialized_property(field: &str) -> bool {
    matches!(field, "solid_history_type" | PROP_BANK | PROP_SWEEP_LENGTH)
        || is_primitive_property(field)
        || is_history_choice(field)
}

pub fn apply_history_choice(
    document: &mut acadrust::CadDocument,
    handle: acadrust::Handle,
    field: &str,
    value: &str,
) -> bool {
    let show_history_mode = document.header.show_solid_history.clamp(0, 2);
    let Some(graph) = document.solid_history_graph(handle) else {
        return false;
    };
    let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&graph.root) else {
        return false;
    };
    let DynamicBlockData::SolidHistory(history) = &mut object.data else {
        return false;
    };
    let before = (history.record_history, history.show_history);
    match field {
        PROP_HISTORY => {
            history.record_history = if value.eq_ignore_ascii_case("Record") {
                true
            } else if value.eq_ignore_ascii_case("None") {
                false
            } else {
                return false;
            };
        }
        PROP_SHOW_HISTORY if show_history_mode == 1 => {
            history.show_history = if value.eq_ignore_ascii_case("Yes") {
                true
            } else if value.eq_ignore_ascii_case("No") {
                false
            } else {
                return false;
            };
        }
        _ => return false,
    }
    before != (history.record_history, history.show_history)
}

fn apply_rectangular_geometry_property(
    value: &mut SolidHistoryBox,
    field: &str,
    text: &str,
) -> Option<bool> {
    if field == PROP_ROTATION {
        let target = crate::entities::common::parse_direction(text)?;
        if !target.is_finite() {
            return Some(false);
        }
        let current = matrix(value.base.transform)?;
        let center = current.transform_point3(glam::DVec3::new(
            value.length * 0.5,
            value.width * 0.5,
            0.0,
        ));
        if !center.is_finite() {
            return Some(false);
        }
        let projected = current.x_axis.truncate();
        if projected.length_squared() <= 1e-12 {
            return Some(false);
        }
        let current_angle = projected.y.atan2(projected.x);
        let delta = (target - current_angle + std::f64::consts::PI)
            .rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        if delta.abs() <= 1e-12 {
            return Some(false);
        }
        let updated = glam::DMat4::from_translation(center)
            * glam::DMat4::from_rotation_z(delta)
            * glam::DMat4::from_translation(-center)
            * current;
        if !updated.is_finite() || updated.determinant().abs() <= 1e-12 {
            return Some(false);
        }
        value.base.transform = updated.to_cols_array();
        return Some(true);
    }
    let axis = match field {
        PROP_POSITION_X => 0,
        PROP_POSITION_Y => 1,
        PROP_POSITION_Z => 2,
        _ => return None,
    };
    let target = crate::entities::common::parse_length(text)?;
    if !target.is_finite() {
        return Some(false);
    }
    let current = matrix(value.base.transform)?;
    let center = current.transform_point3(glam::DVec3::new(
        value.length * 0.5,
        value.width * 0.5,
        0.0,
    ));
    if !center.is_finite() {
        return Some(false);
    }
    if target == center[axis] {
        return Some(false);
    }
    let mut delta = glam::DVec3::ZERO;
    delta[axis] = target - center[axis];
    value.base.transform = (glam::DMat4::from_translation(delta) * current).to_cols_array();
    Some(true)
}

fn apply_sphere_geometry_property(
    value: &mut SolidHistorySphere,
    field: &str,
    text: &str,
) -> Option<bool> {
    let axis = match field {
        PROP_POSITION_X => 0,
        PROP_POSITION_Y => 1,
        PROP_POSITION_Z => 2,
        _ => return None,
    };
    let target = crate::entities::common::parse_length(text)?;
    if !target.is_finite() {
        return Some(false);
    }
    let current = matrix(value.base.transform)?;
    let center = current.transform_point3(glam::DVec3::ZERO);
    if !center.is_finite() || (target - center[axis]).abs() <= 1e-12 {
        return Some(false);
    }
    let mut delta = glam::DVec3::ZERO;
    delta[axis] = target - center[axis];
    let updated = glam::DMat4::from_translation(delta) * current;
    if !updated.is_finite() || updated.determinant().abs() <= 1e-12 {
        return Some(false);
    }
    value.base.transform = updated.to_cols_array();
    Some(true)
}

fn apply_cone_position_property(
    value: &mut SolidHistoryCone,
    field: &str,
    text: &str,
) -> Option<bool> {
    let axis = match field {
        PROP_POSITION_X => 0,
        PROP_POSITION_Y => 1,
        PROP_POSITION_Z => 2,
        _ => return None,
    };
    let target = crate::entities::common::parse_length(text)?;
    if !target.is_finite() {
        return Some(false);
    }
    let current = matrix(value.base.transform)?;
    let origin = current.transform_point3(glam::DVec3::ZERO);
    if !origin.is_finite() || (target - origin[axis]).abs() <= 1e-12 {
        return Some(false);
    }
    let mut delta = glam::DVec3::ZERO;
    delta[axis] = target - origin[axis];
    let updated = glam::DMat4::from_translation(delta) * current;
    if !updated.is_finite() || updated.determinant().abs() <= 1e-12 {
        return Some(false);
    }
    value.base.transform = updated.to_cols_array();
    Some(true)
}

fn apply_cylinder_position_property(
    value: &mut SolidHistoryCylinder,
    field: &str,
    text: &str,
) -> Option<bool> {
    let axis = match field {
        PROP_POSITION_X => 0,
        PROP_POSITION_Y => 1,
        PROP_POSITION_Z => 2,
        _ => return None,
    };
    let target = crate::entities::common::parse_length(text)?;
    if !target.is_finite() {
        return Some(false);
    }
    let current = matrix(value.base.transform)?;
    let origin = current.transform_point3(glam::DVec3::ZERO);
    if !origin.is_finite() || (target - origin[axis]).abs() <= 1e-12 {
        return Some(false);
    }
    let mut delta = glam::DVec3::ZERO;
    delta[axis] = target - origin[axis];
    let updated = glam::DMat4::from_translation(delta) * current;
    if !updated.is_finite() || updated.determinant().abs() <= 1e-12 {
        return Some(false);
    }
    value.base.transform = updated.to_cols_array();
    Some(true)
}

fn apply_sweep_geometry_property(
    value: &mut SolidHistorySweep,
    field: &str,
    text: &str,
) -> Option<bool> {
    match field {
        PROP_PROFILE_ROTATION => {
            let target = crate::entities::common::parse_direction(text)?;
            if !target.is_finite() || (target - value.align_angle).abs() <= 1e-12 {
                return Some(false);
            }
            value.align_angle = target;
            Some(true)
        }
        PROP_TWIST_ALONG_PATH => {
            let target = crate::entities::common::parse_angle(text)?;
            if !target.is_finite() || (target - value.twist_angle).abs() <= 1e-12 {
                return Some(false);
            }
            value.twist_angle = target;
            Some(true)
        }
        PROP_SCALE_ALONG_PATH => {
            let target = text.trim().replace(',', ".").parse::<f64>().ok()?;
            if !target.is_finite()
                || target <= 1e-9
                || (target - value.scale_factor).abs() <= 1e-12
            {
                return Some(false);
            }
            value.scale_factor = target;
            Some(true)
        }
        _ => None,
    }
}

fn apply_torus_position_property(
    value: &mut SolidHistoryTorus,
    field: &str,
    text: &str,
) -> Option<bool> {
    let axis = match field {
        PROP_POSITION_X => 0,
        PROP_POSITION_Y => 1,
        PROP_POSITION_Z => 2,
        _ => return None,
    };
    let target = crate::entities::common::parse_length(text)?;
    if !target.is_finite() {
        return Some(false);
    }
    let current = matrix(value.base.transform)?;
    let origin = current.transform_point3(glam::DVec3::ZERO);
    if !origin.is_finite() || (target - origin[axis]).abs() <= 1e-12 {
        return Some(false);
    }
    let mut delta = glam::DVec3::ZERO;
    delta[axis] = target - origin[axis];
    let updated = glam::DMat4::from_translation(delta) * current;
    if !updated.is_finite() || updated.determinant().abs() <= 1e-12 {
        return Some(false);
    }
    value.base.transform = updated.to_cols_array();
    Some(true)
}

fn canonicalize_cylinder_radii(value: &mut SolidHistoryCylinder) -> bool {
    if value.minor_radius <= value.major_radius {
        value.x_radius = value.major_radius;
        return true;
    }
    let Some(current) = matrix(value.base.transform) else {
        return false;
    };
    std::mem::swap(&mut value.major_radius, &mut value.minor_radius);
    value.x_radius = value.major_radius;
    value.base.transform =
        (current * glam::DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2)).to_cols_array();
    true
}

fn apply_pyramid_geometry_property(
    value: &mut SolidHistoryPyramid,
    field: &str,
    text: &str,
) -> Option<bool> {
    if field == PROP_PYRAMID_TYPE {
        let next_inscribed = if text.eq_ignore_ascii_case("Inscribed") {
            true
        } else if text.eq_ignore_ascii_case("Circumscribed") {
            false
        } else {
            return Some(false);
        };
        let old_inscribed = pyramid_is_inscribed(value);
        if next_inscribed == old_inscribed {
            return Some(false);
        }
        let factor = pyramid_apothem_factor(value.sides);
        if factor <= 1e-9 {
            return Some(false);
        }
        let base_display = if old_inscribed { value.radius } else { value.radius * factor };
        let top_display = if old_inscribed {
            value.top_radius
        } else {
            value.top_radius * factor
        };
        let Some(current) = matrix(value.base.transform) else {
            return Some(false);
        };
        value.radius = if next_inscribed { base_display } else { base_display / factor };
        value.top_radius = if next_inscribed { top_display } else { top_display / factor };
        let half = std::f64::consts::PI / value.sides.clamp(3, 32) as f64;
        let delta = if next_inscribed { half } else { -half };
        value.base.transform =
            (current * glam::DMat4::from_rotation_z(delta)).to_cols_array();
        value.operation_minor = if next_inscribed { -1 } else { 0 };
        return Some(true);
    }
    if field == PROP_ROTATION {
        let target = crate::entities::common::parse_direction(text)?;
        if !target.is_finite() {
            return Some(false);
        }
        let current = matrix(value.base.transform)?;
        let projected = current.x_axis.truncate();
        if projected.length_squared() <= 1e-12 {
            return Some(false);
        }
        let current_angle = projected.y.atan2(projected.x);
        let target_vertex_angle = if pyramid_is_inscribed(value) {
            target
        } else {
            target - std::f64::consts::PI / value.sides.clamp(3, 32) as f64
        };
        let delta = (target_vertex_angle - current_angle + std::f64::consts::PI)
            .rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        if delta.abs() <= 1e-12 {
            return Some(false);
        }
        let updated = current * glam::DMat4::from_rotation_z(delta);
        if !updated.is_finite() || updated.determinant().abs() <= 1e-12 {
            return Some(false);
        }
        value.base.transform = updated.to_cols_array();
        return Some(true);
    }
    let axis = match field {
        PROP_POSITION_X => 0,
        PROP_POSITION_Y => 1,
        PROP_POSITION_Z => 2,
        _ => return None,
    };
    let target = crate::entities::common::parse_length(text)?;
    if !target.is_finite() {
        return Some(false);
    }
    let current = matrix(value.base.transform)?;
    let origin = current.transform_point3(glam::DVec3::ZERO);
    if !origin.is_finite() || (target - origin[axis]).abs() <= 1e-12 {
        return Some(false);
    }
    let mut delta = glam::DVec3::ZERO;
    delta[axis] = target - origin[axis];
    let updated = glam::DMat4::from_translation(delta) * current;
    if !updated.is_finite() || updated.determinant().abs() <= 1e-12 {
        return Some(false);
    }
    value.base.transform = updated.to_cols_array();
    Some(true)
}

fn set_pyramid_sides(value: &mut SolidHistoryPyramid, sides: i32) -> bool {
    if !(3..=32).contains(&sides) || sides == value.sides {
        return false;
    }
    if !pyramid_is_inscribed(value) {
        let Some(current) = matrix(value.base.transform) else {
            return false;
        };
        let old_factor = pyramid_apothem_factor(value.sides);
        let new_factor = pyramid_apothem_factor(sides);
        value.radius = value.radius * old_factor / new_factor;
        value.top_radius = value.top_radius * old_factor / new_factor;
        let old_half = std::f64::consts::PI / value.sides.clamp(3, 32) as f64;
        let new_half = std::f64::consts::PI / sides as f64;
        value.base.transform =
            (current * glam::DMat4::from_rotation_z(old_half - new_half)).to_cols_array();
    }
    value.sides = sides;
    true
}

pub fn apply_primitive_property(
    operation: &mut SolidHistoryOperation,
    field: &str,
    value: &str,
) -> bool {
    if let SolidHistoryOperation::Box(rectangular_value)
    | SolidHistoryOperation::Wedge(rectangular_value) = operation
    {
        if let Some(applied) =
            apply_rectangular_geometry_property(rectangular_value, field, value)
        {
            return applied;
        }
    }
    if let SolidHistoryOperation::Sphere(sphere_value) = operation {
        if let Some(applied) = apply_sphere_geometry_property(sphere_value, field, value) {
            return applied;
        }
    }
    if let SolidHistoryOperation::Cone(cone_value) = operation {
        if let Some(applied) = apply_cone_position_property(cone_value, field, value) {
            return applied;
        }
    }
    if let SolidHistoryOperation::Cylinder(cylinder_value) = operation {
        if let Some(applied) = apply_cylinder_position_property(cylinder_value, field, value) {
            return applied;
        }
    }
    if let SolidHistoryOperation::Sweep(sweep_value) = operation {
        if let Some(applied) = apply_sweep_geometry_property(sweep_value, field, value) {
            return applied;
        }
    }
    if let SolidHistoryOperation::Torus(torus_value) = operation {
        if let Some(applied) = apply_torus_position_property(torus_value, field, value) {
            return applied;
        }
    }
    if let SolidHistoryOperation::Pyramid(pyramid_value) = operation {
        if let Some(applied) = apply_pyramid_geometry_property(pyramid_value, field, value) {
            return applied;
        }
    }
    let Some(number) = (if field == PROP_SIDES {
        value.trim().parse::<f64>().ok()
    } else {
        crate::entities::common::parse_length(value)
    }) else {
        return false;
    };
    if !number.is_finite() {
        return false;
    }
    let positive = || (number > 0.0).then_some(number);
    if let SolidHistoryOperation::Box(value) | SolidHistoryOperation::Wedge(value) = operation {
        let Some(next) = positive() else {
            return false;
        };
        let (current_size, local_shift) = match field {
            PROP_LENGTH => (
                value.length,
                glam::DVec3::new((value.length - next) * 0.5, 0.0, 0.0),
            ),
            PROP_WIDTH => (
                value.width,
                glam::DVec3::new(0.0, (value.width - next) * 0.5, 0.0),
            ),
            PROP_HEIGHT => (value.height, glam::DVec3::ZERO),
            _ => return false,
        };
        if !current_size.is_finite() || current_size < 1e-6 || next == current_size {
            return false;
        }
        if local_shift != glam::DVec3::ZERO {
            let Some(current) = matrix(value.base.transform) else {
                return false;
            };
            value.base.transform =
                (current * glam::DMat4::from_translation(local_shift)).to_cols_array();
        }
        match field {
            PROP_LENGTH => value.length = next,
            PROP_WIDTH => value.width = next,
            PROP_HEIGHT => value.height = next,
            _ => unreachable!(),
        }
        return true;
    }
    match operation {
        SolidHistoryOperation::Cylinder(value) => match field {
            PROP_RADIUS => {
                let Some(radius) = positive() else {
                    return false;
                };
                value.major_radius = radius;
                value.minor_radius = radius;
                value.x_radius = radius;
            }
            PROP_MAJOR_RADIUS => {
                value.major_radius = positive().unwrap_or(value.major_radius);
                if !canonicalize_cylinder_radii(value) {
                    return false;
                }
            }
            PROP_MINOR_RADIUS => {
                value.minor_radius = positive().unwrap_or(value.minor_radius);
                if !canonicalize_cylinder_radii(value) {
                    return false;
                }
            }
            PROP_HEIGHT => value.height = positive().unwrap_or(value.height),
            _ => return false,
        },
        SolidHistoryOperation::Cone(value) => match field {
            PROP_BASE_RADIUS => {
                let Some(radius) = positive() else {
                    return false;
                };
                let ratio = if value.base_x_radius > 1e-9 {
                    value.base_y_radius / value.base_x_radius
                } else {
                    1.0
                };
                value.base_x_radius = radius;
                value.base_y_radius = radius * ratio;
            }
            PROP_BASE_MAJOR_RADIUS => {
                value.base_x_radius = positive().unwrap_or(value.base_x_radius);
            }
            PROP_BASE_MINOR_RADIUS => {
                value.base_y_radius = positive().unwrap_or(value.base_y_radius);
            }
            PROP_TOP_RADIUS => {
                if number < 0.0 {
                    return false;
                }
                value.top_radius = number;
            }
            PROP_TOP_MAJOR_RADIUS => {
                if number < 0.0 {
                    return false;
                }
                value.top_radius = number;
            }
            PROP_TOP_MINOR_RADIUS => {
                if number < 0.0 || value.base_y_radius.abs() <= 1e-9 {
                    return false;
                }
                value.top_radius = number * value.base_x_radius / value.base_y_radius;
            }
            PROP_HEIGHT => value.height = positive().unwrap_or(value.height),
            _ => return false,
        },
        SolidHistoryOperation::Sphere(value) => {
            let Some(next) = positive() else {
                return false;
            };
            let radius = match field {
                PROP_RADIUS => next,
                PROP_DIAMETER => next * 0.5,
                _ => return false,
            };
            if (radius - value.radius).abs() <= 1e-12 {
                return false;
            }
            value.radius = radius;
        }
        SolidHistoryOperation::Torus(value) => match field {
            PROP_MAJOR_RADIUS => {
                let Some(radius) = positive() else {
                    return false;
                };
                if (radius - value.major_radius).abs() <= 1e-12 {
                    return false;
                }
                value.major_radius = radius;
            }
            PROP_MINOR_RADIUS => {
                let Some(radius) = positive() else {
                    return false;
                };
                if (radius - value.minor_radius).abs() <= 1e-12 {
                    return false;
                }
                value.minor_radius = radius;
            }
            _ => return false,
        },
        SolidHistoryOperation::Pyramid(value) => match field {
            PROP_BASE_RADIUS => {
                let Some(displayed) = positive() else {
                    return false;
                };
                value.radius = if pyramid_is_inscribed(value) {
                    displayed
                } else {
                    displayed / pyramid_apothem_factor(value.sides)
                };
            }
            PROP_TOP_RADIUS => {
                if number < 0.0 {
                    return false;
                }
                value.top_radius = if pyramid_is_inscribed(value) {
                    number
                } else {
                    number / pyramid_apothem_factor(value.sides)
                };
            }
            PROP_HEIGHT => value.height = positive().unwrap_or(value.height),
            PROP_SIDES => {
                let rounded = number.round();
                if (number - rounded).abs() > 1e-9 {
                    return false;
                }
                return set_pyramid_sides(value, rounded as i32);
            }
            _ => return false,
        },
        _ => return false,
    }
    positive().is_some()
        || field == PROP_SIDES
        || matches!(
            field,
            PROP_TOP_RADIUS | PROP_TOP_MAJOR_RADIUS | PROP_TOP_MINOR_RADIUS
        )
}

fn matrix(transform: [f64; 16]) -> Option<glam::DMat4> {
    let matrix = glam::DMat4::from_cols_array(&transform);
    (matrix.is_finite() && matrix.determinant().abs() > 1e-12).then_some(matrix)
}

fn codec_matrix(transform: &acadrust::types::Transform) -> glam::DMat4 {
    let matrix = transform.matrix.m;
    glam::DMat4::from_cols_array(&[
        matrix[0][0], matrix[1][0], matrix[2][0], matrix[3][0],
        matrix[0][1], matrix[1][1], matrix[2][1], matrix[3][1],
        matrix[0][2], matrix[1][2], matrix[2][2], matrix[3][2],
        matrix[0][3], matrix[1][3], matrix[2][3], matrix[3][3],
    ])
}

fn transform_matrix(transform: &EntityTransform) -> Option<glam::DMat4> {
    Some(match transform {
        EntityTransform::Translate(delta) => glam::DMat4::from_translation(*delta),
        EntityTransform::Rotate {
            center,
            axis,
            angle_rad,
        } => {
            let axis = axis.normalize_or_zero();
            if axis.length_squared() <= 1e-12 {
                return None;
            }
            glam::DMat4::from_translation(*center)
                * glam::DMat4::from_axis_angle(axis, *angle_rad)
                * glam::DMat4::from_translation(-*center)
        }
        EntityTransform::Scale { center, factor } => {
            glam::DMat4::from_translation(*center)
                * glam::DMat4::from_scale(glam::DVec3::splat(*factor))
                * glam::DMat4::from_translation(-*center)
        }
        EntityTransform::Mirror {
            p1,
            p2,
            working_normal,
        } => codec_matrix(&crate::scene::view::transform::reflection_about_working_line(
            *p1,
            *p2,
            *working_normal,
        )),
        EntityTransform::Affine(value) => codec_matrix(value),
    })
}

pub fn transform_operation(
    operation: &mut SolidHistoryOperation,
    transform: &EntityTransform,
) -> bool {
    let Some(base) = operation.base_mut() else {
        return false;
    };
    let Some(current) = matrix(base.transform) else {
        return false;
    };
    let Some(by) = transform_matrix(transform) else {
        return false;
    };
    let transformed = by * current;
    if !transformed.is_finite() || transformed.determinant().abs() <= 1e-12 {
        return false;
    }
    base.transform = transformed.to_cols_array();
    true
}

fn world_point(transform: [f64; 16], point: [f64; 3]) -> Option<glam::DVec3> {
    Some(matrix(transform)?.transform_point3(glam::DVec3::from_array(point)))
}

fn world_vector(transform: [f64; 16], vector: [f64; 3]) -> Option<glam::DVec3> {
    let vector = matrix(transform)?.transform_vector3(glam::DVec3::from_array(vector));
    (vector.length_squared() > 1e-12).then(|| vector.normalize())
}

fn local_point(transform: [f64; 16], point: glam::DVec3) -> Option<glam::DVec3> {
    Some(matrix(transform)?.inverse().transform_point3(point))
}

fn grip(
    id: usize,
    world: glam::DVec3,
    shape: GripShape,
    axis: Option<glam::DVec3>,
) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: false,
        shape,
        dir: (shape == GripShape::Triangle).then_some(axis).flatten(),
        axis,
    }
}

pub fn primitive_grips(
    document: &acadrust::CadDocument,
    handle: acadrust::Handle,
) -> Vec<GripDef> {
    let Some(operation) = document.solid_history_operation(handle) else {
        return Vec::new();
    };
    let mut grips = Vec::new();
    let mut add = |id, transform, point, shape, axis: Option<[f64; 3]>| {
        if let Some(world) = world_point(transform, point) {
            grips.push(grip(
                id,
                world,
                shape,
                axis.and_then(|vector| world_vector(transform, vector)),
            ));
        }
    };
    match operation {
        SolidHistoryOperation::Box(value) => {
            for corner in 0..8 {
                add(
                    GRIP_BOX_CORNER_FIRST + corner,
                    value.base.transform,
                    [
                        if corner & 1 == 0 { 0.0 } else { value.length },
                        if corner & 2 == 0 { 0.0 } else { value.width },
                        if corner & 4 == 0 { 0.0 } else { value.height },
                    ],
                    GripShape::Square,
                    None,
                );
            }
            for (id, point, axis) in [
                (
                    GRIP_BOX_FACE_X_MIN,
                    [0.0, value.width * 0.5, value.height * 0.5],
                    [-1.0, 0.0, 0.0],
                ),
                (
                    GRIP_BOX_FACE_X_MAX,
                    [value.length, value.width * 0.5, value.height * 0.5],
                    [1.0, 0.0, 0.0],
                ),
                (
                    GRIP_BOX_FACE_Y_MIN,
                    [value.length * 0.5, 0.0, value.height * 0.5],
                    [0.0, -1.0, 0.0],
                ),
                (
                    GRIP_BOX_FACE_Y_MAX,
                    [value.length * 0.5, value.width, value.height * 0.5],
                    [0.0, 1.0, 0.0],
                ),
                (
                    GRIP_BOX_FACE_Z_MIN,
                    [value.length * 0.5, value.width * 0.5, 0.0],
                    [0.0, 0.0, -1.0],
                ),
                (
                    GRIP_BOX_FACE_Z_MAX,
                    [value.length * 0.5, value.width * 0.5, value.height],
                    [0.0, 0.0, 1.0],
                ),
            ] {
                add(
                    id,
                    value.base.transform,
                    point,
                    GripShape::Triangle,
                    Some(axis),
                );
            }
        }
        SolidHistoryOperation::Wedge(value) => {
            add(
                GRIP_LENGTH,
                value.base.transform,
                [value.length, value.width * 0.5, 0.0],
                GripShape::Square,
                None,
            );
            add(
                GRIP_WIDTH,
                value.base.transform,
                [value.length * 0.5, value.width, 0.0],
                GripShape::Square,
                None,
            );
            add(
                GRIP_HEIGHT,
                value.base.transform,
                [value.length * 0.5, value.width * 0.5, value.height],
                GripShape::Square,
                Some([0.0, 0.0, 1.0]),
            );
        }
        SolidHistoryOperation::Cylinder(value) => {
            let scale = value
                .major_radius
                .abs()
                .max(value.minor_radius.abs())
                .max(1.0);
            if (value.major_radius - value.minor_radius).abs() > 1e-9 * scale {
                add(
                    GRIP_MAJOR_RADIUS,
                    value.base.transform,
                    [value.major_radius, 0.0, value.height * 0.5],
                    GripShape::Square,
                    None,
                );
                add(
                    GRIP_MINOR_RADIUS,
                    value.base.transform,
                    [0.0, value.minor_radius, value.height * 0.5],
                    GripShape::Square,
                    None,
                );
            } else {
                add(
                    GRIP_RADIUS,
                    value.base.transform,
                    [value.major_radius, 0.0, value.height * 0.5],
                    GripShape::Square,
                    None,
                );
            }
            add(
                GRIP_HEIGHT,
                value.base.transform,
                [0.0, 0.0, value.height],
                GripShape::Square,
                Some([0.0, 0.0, 1.0]),
            );
        }
        SolidHistoryOperation::Cone(value) => {
            add(
                GRIP_RADIUS,
                value.base.transform,
                [value.base_x_radius, 0.0, 0.0],
                GripShape::Square,
                None,
            );
            add(
                GRIP_HEIGHT,
                value.base.transform,
                [0.0, 0.0, value.height],
                GripShape::Square,
                Some([0.0, 0.0, 1.0]),
            );
        }
        SolidHistoryOperation::Sphere(value) => add(
            GRIP_RADIUS,
            value.base.transform,
            [value.radius, 0.0, 0.0],
            GripShape::Square,
            None,
        ),
        SolidHistoryOperation::Torus(value) => {
            add(
                GRIP_MAJOR_RADIUS,
                value.base.transform,
                [value.major_radius, 0.0, 0.0],
                GripShape::Square,
                None,
            );
            add(
                GRIP_MINOR_RADIUS,
                value.base.transform,
                [value.major_radius, 0.0, value.minor_radius],
                GripShape::Square,
                Some([0.0, 0.0, 1.0]),
            );
        }
        SolidHistoryOperation::Pyramid(value) => {
            add(
                GRIP_RADIUS,
                value.base.transform,
                [value.radius, 0.0, 0.0],
                GripShape::Square,
                None,
            );
            add(
                GRIP_TOP_RADIUS,
                value.base.transform,
                [value.top_radius, 0.0, value.height],
                GripShape::Square,
                None,
            );
            add(
                GRIP_HEIGHT,
                value.base.transform,
                [0.0, 0.0, value.height],
                GripShape::Square,
                Some([0.0, 0.0, 1.0]),
            );
            let angle = (value.sides.clamp(3, 32) as f64 * 5.0).to_radians();
            add(
                GRIP_SIDES,
                value.base.transform,
                [value.radius * angle.cos(), value.radius * angle.sin(), 0.0],
                GripShape::Triangle,
                None,
            );
        }
        SolidHistoryOperation::Sweep(value) => add(
            GRIP_POSITION,
            value.base.transform,
            [
                value.reference_point.x,
                value.reference_point.y,
                value.reference_point.z,
            ],
            GripShape::Square,
            None,
        ),
        _ => {}
    }
    grips
}

pub fn apply_primitive_grip(
    operation: &mut SolidHistoryOperation,
    grip_id: usize,
    apply: GripApply,
) -> bool {
    let GripApply::Absolute(world) = apply else {
        return false;
    };
    let Some(transform) = operation.base().map(|base| base.transform) else {
        return false;
    };
    let Some(local) = local_point(transform, world) else {
        return false;
    };
    if !local.is_finite() {
        return false;
    }
    let positive = |value: f64| value.abs().max(1e-6);
    match operation {
        SolidHistoryOperation::Box(value) => {
            let old_size = glam::DVec3::new(value.length, value.width, value.height);
            if !old_size.is_finite() || old_size.min_element() < 1e-6 {
                return false;
            }
            let mut low = glam::DVec3::ZERO;
            let mut high = old_size;
            if (GRIP_BOX_CORNER_FIRST..GRIP_BOX_CORNER_FIRST + 8).contains(&grip_id) {
                let corner = grip_id - GRIP_BOX_CORNER_FIRST;
                for axis in 0..3 {
                    let opposite = if corner & (1 << axis) == 0 {
                        old_size[axis]
                    } else {
                        0.0
                    };
                    low[axis] = local[axis].min(opposite);
                    high[axis] = local[axis].max(opposite);
                }
            } else {
                match grip_id {
                    GRIP_BOX_FACE_X_MIN => low.x = local.x.min(old_size.x - 1e-6),
                    GRIP_BOX_FACE_X_MAX => high.x = local.x.max(1e-6),
                    GRIP_BOX_FACE_Y_MIN => low.y = local.y.min(old_size.y - 1e-6),
                    GRIP_BOX_FACE_Y_MAX => high.y = local.y.max(1e-6),
                    GRIP_BOX_FACE_Z_MIN => low.z = local.z.min(old_size.z - 1e-6),
                    GRIP_BOX_FACE_Z_MAX => high.z = local.z.max(1e-6),
                    GRIP_LENGTH => high.x = positive(local.x),
                    GRIP_WIDTH => high.y = positive(local.y),
                    GRIP_HEIGHT => high.z = local.z.max(1e-6),
                    _ => return false,
                }
            }
            let size = high - low;
            if !size.is_finite() || size.min_element() < 1e-6 {
                return false;
            }
            if low.abs().max_element() <= 1e-12
                && (size - old_size).abs().max_element() <= 1e-12
            {
                return false;
            }
            let Some(current) = matrix(value.base.transform) else {
                return false;
            };
            value.base.transform =
                (current * glam::DMat4::from_translation(low)).to_cols_array();
            value.length = size.x;
            value.width = size.y;
            value.height = size.z;
        }
        SolidHistoryOperation::Wedge(value) => {
            match grip_id {
                GRIP_LENGTH => value.length = positive(local.x),
                GRIP_WIDTH => value.width = positive(local.y),
                GRIP_HEIGHT => value.height = local.z.max(1e-6),
                _ => return false,
            }
        }
        SolidHistoryOperation::Cylinder(value) => {
            match grip_id {
                GRIP_RADIUS => {
                    let radius = local.x.hypot(local.y).max(1e-6);
                    value.major_radius = radius;
                    value.minor_radius = radius;
                    value.x_radius = radius;
                }
                GRIP_MAJOR_RADIUS => {
                    value.major_radius = local.x.abs().max(1e-6);
                    if !canonicalize_cylinder_radii(value) {
                        return false;
                    }
                }
                GRIP_MINOR_RADIUS => {
                    value.minor_radius = local.y.abs().max(1e-6);
                    if !canonicalize_cylinder_radii(value) {
                        return false;
                    }
                }
                GRIP_HEIGHT => value.height = local.z.max(1e-6),
                _ => return false,
            }
        }
        SolidHistoryOperation::Cone(value) => match grip_id {
            GRIP_RADIUS => {
                let radius = local.x.hypot(local.y).max(1e-6);
                let ratio = if value.base_x_radius > 1e-9 {
                    value.base_y_radius / value.base_x_radius
                } else {
                    1.0
                };
                value.base_x_radius = radius;
                value.base_y_radius = radius * ratio;
            }
            GRIP_HEIGHT => value.height = local.z.max(1e-6),
            _ => return false,
        },
        SolidHistoryOperation::Sphere(value) if grip_id == GRIP_RADIUS => {
            value.radius = local.length().max(1e-6);
        }
        SolidHistoryOperation::Torus(value) => match grip_id {
            GRIP_MAJOR_RADIUS => value.major_radius = local.x.hypot(local.y).max(1e-6),
            GRIP_MINOR_RADIUS => value.minor_radius = local.z.abs().max(1e-6),
            _ => return false,
        },
        SolidHistoryOperation::Pyramid(value) => match grip_id {
            GRIP_RADIUS => value.radius = local.x.hypot(local.y).max(1e-6),
            GRIP_TOP_RADIUS => {
                value.top_radius = local.x.hypot(local.y).max(0.0);
            }
            GRIP_HEIGHT => value.height = local.z.max(1e-6),
            GRIP_SIDES => {
                let angle = local.y.atan2(local.x).rem_euclid(std::f64::consts::TAU);
                let sides = ((angle.to_degrees() / 5.0).round() as i32).clamp(3, 32);
                if !set_pyramid_sides(value, sides) {
                    return false;
                }
            }
            _ => return false,
        },
        SolidHistoryOperation::Sweep(value) if grip_id == GRIP_POSITION => {
            let Some(current) = matrix(value.base.transform) else {
                return false;
            };
            let reference = current.transform_point3(glam::DVec3::new(
                value.reference_point.x,
                value.reference_point.y,
                value.reference_point.z,
            ));
            let delta = world - reference;
            if !delta.is_finite() || delta.length_squared() <= 1e-24 {
                return false;
            }
            value.base.transform =
                (glam::DMat4::from_translation(delta) * current).to_cols_array();
        }
        _ => return false,
    }
    true
}

fn base(transform: [f64; 16]) -> SolidHistoryNodeBase {
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = transform;
    base
}

pub fn box_op(
    transform: [f64; 16],
    length: f64,
    width: f64,
    height: f64,
) -> SolidHistoryOperation {
    SolidHistoryOperation::Box(SolidHistoryBox {
        base: base(transform),
        operation_major: 1,
        length,
        width,
        height,
        ..SolidHistoryBox::default()
    })
}

pub fn wedge_op(
    transform: [f64; 16],
    length: f64,
    width: f64,
    height: f64,
) -> SolidHistoryOperation {
    SolidHistoryOperation::Wedge(SolidHistoryBox {
        base: base(transform),
        operation_major: 1,
        length,
        width,
        height,
        ..SolidHistoryBox::default()
    })
}

pub fn cylinder_op(
    transform: [f64; 16],
    radius: f64,
    height: f64,
) -> SolidHistoryOperation {
    SolidHistoryOperation::Cylinder(SolidHistoryCylinder {
        base: base(transform),
        operation_major: 1,
        height,
        major_radius: radius,
        minor_radius: radius,
        x_radius: radius,
        ..SolidHistoryCylinder::default()
    })
}

pub fn elliptical_cylinder_op(
    transform: [f64; 16],
    major_radius: f64,
    minor_radius: f64,
    height: f64,
) -> SolidHistoryOperation {
    SolidHistoryOperation::Cylinder(SolidHistoryCylinder {
        base: base(transform),
        operation_major: 1,
        height,
        major_radius,
        minor_radius,
        x_radius: major_radius,
        ..SolidHistoryCylinder::default()
    })
}

pub fn cone_op(
    transform: [f64; 16],
    base_x_radius: f64,
    base_y_radius: f64,
    top_radius: f64,
    height: f64,
) -> SolidHistoryOperation {
    SolidHistoryOperation::Cone(SolidHistoryCone {
        base: base(transform),
        operation_major: 1,
        height,
        base_x_radius,
        base_y_radius,
        top_radius,
        ..SolidHistoryCone::default()
    })
}

pub fn sphere_op(transform: [f64; 16], radius: f64) -> SolidHistoryOperation {
    SolidHistoryOperation::Sphere(SolidHistorySphere {
        base: base(transform),
        operation_major: 1,
        radius,
        ..SolidHistorySphere::default()
    })
}

pub fn torus_op(
    transform: [f64; 16],
    major_radius: f64,
    minor_radius: f64,
) -> SolidHistoryOperation {
    SolidHistoryOperation::Torus(SolidHistoryTorus {
        base: base(transform),
        operation_major: 1,
        major_radius,
        minor_radius,
        ..SolidHistoryTorus::default()
    })
}

pub fn pyramid_op(
    transform: [f64; 16],
    radius: f64,
    top_radius: f64,
    height: f64,
    sides: usize,
    inscribed: bool,
) -> SolidHistoryOperation {
    SolidHistoryOperation::Pyramid(SolidHistoryPyramid {
        base: base(transform),
        operation_major: 1,
        operation_minor: if inscribed { -1 } else { 0 },
        height,
        sides: sides as i32,
        radius,
        top_radius,
        ..SolidHistoryPyramid::default()
    })
}

pub fn brep_op(body: &Body) -> SolidHistoryOperation {
    let acis_data = crate::scene::convert::acis_export::solid_to_sat(body)
        .map(|document| {
            let mut solid = Solid3D::new();
            solid.set_sat_document(&document);
            solid.acis_data
        })
        .unwrap_or_default();
    SolidHistoryOperation::Brep(SolidHistoryBrep {
        base: base(glam::DMat4::IDENTITY.to_cols_array()),
        operation_major: 1,
        acis_data,
        ..SolidHistoryBrep::default()
    })
}
