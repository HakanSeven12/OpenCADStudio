use acadrust::{EntityType, Handle};
use crate::t;

use crate::scene::model::object::{PropSection, PropValue, Property};

pub fn general_section(entity: &EntityType) -> PropSection {
    let common = entity.common();
    let linetype_display = if common.linetype.is_empty() {
        "ByLayer".to_string()
    } else {
        common.linetype.clone()
    };
    // Alpha 0 is the ByLayer default (Transparency::BY_LAYER); show it by name
    // and fall back to a rounded percentage only for an explicit value.
    let transp_display = if common.transparency.alpha() == 0 {
        "ByLayer".to_string()
    } else {
        format!(
            "{}",
            (common.transparency.alpha() as f64 / 255.0 * 100.0).round() as u32
        )
    };

    // Hyperlink is stored in XDATA under the "PE_URL" application.
    let hyperlink = common
        .extended_data
        .get_record("PE_URL")
        .and_then(|r| {
            r.values.iter().find_map(|v| match v {
                acadrust::xdata::XDataValue::String(s) if !s.is_empty() => Some(s.clone()),
                _ => None,
            })
        })
        .unwrap_or_default();

    let mut section = PropSection {
        title: t!("General").into_owned(),
        props: vec![
            Property {
                label: t!("Handle").into_owned(),
                field: "handle",
                value: PropValue::ReadOnly(common.handle.value().to_string()),
            },
            Property {
                label: t!("Color").into_owned(),
                field: "color",
                value: PropValue::ColorChoice(common.color),
            },
            Property {
                label: t!("Layer").into_owned(),
                field: "layer",
                value: PropValue::LayerChoice(common.layer.clone()),
            },
            Property {
                label: t!("Linetype").into_owned(),
                field: "linetype",
                value: PropValue::LinetypeChoice(linetype_display),
            },
            Property {
                label: t!("Linetype scale").into_owned(),
                field: "linetype_scale",
                value: PropValue::EditText(format!("{:.4}", common.linetype_scale)),
            },
            Property {
                label: t!("Plot style").into_owned(),
                field: "plot_style",
                value: PropValue::ReadOnly(
                    match common.plotstyle_flags {
                        0 => "ByLayer",
                        1 => "ByBlock",
                        _ => "ByColor",
                    }
                    .into(),
                ),
            },
            Property {
                label: t!("Lineweight").into_owned(),
                field: "lineweight",
                value: PropValue::LwChoice(common.line_weight),
            },
            Property {
                label: t!("Transparency").into_owned(),
                field: "transparency",
                value: PropValue::EditText(transp_display),
            },
            Property {
                label: t!("Hyperlink").into_owned(),
                field: "hyperlink",
                value: PropValue::PlainText(hyperlink),
            },
        ],
    };

    if matches!(entity, EntityType::LwPolyline(polyline) if crate::entities::lwpolyline::is_rectangle(polyline)) {
        section.props.retain(|prop| prop.field != "handle");
    }

    if matches!(entity, EntityType::Point(_)) {
        section.props.retain(|prop| prop.field != "handle");
        let hyperlink = section.props.iter().position(|prop| prop.field == "hyperlink");
        let transparency = section.props.iter().position(|prop| prop.field == "transparency");
        if let (Some(hyperlink), Some(transparency)) = (hyperlink, transparency) {
            section.props.swap(hyperlink, transparency);
        }
    }

    // Thickness (DXF 39) is a General-group property, but only the entity
    // types that carry an extrusion thickness expose it (line, circle, arc,
    // polyline, text, 2D solid, …). Show it right after Hyperlink for those.
    if let Some(t) = crate::scene::view::dispatch::entity_thickness(entity) {
        section
            .props
            .push(crate::entities::common::edit_prop(t!("Thickness").as_ref(), "thickness", t));
    }

    section
}

/// The "3D Visualization" group (Material), common to every graphical object.
/// Material source is flag-based; a custom material handle is shown as "Custom"
/// (name resolution needs the doc).
pub fn visualization_section(entity: &EntityType) -> Option<PropSection> {
    if matches!(
        entity,
        EntityType::Block(_)
            | EntityType::BlockEnd(_)
            | EntityType::Seqend(_)
            | EntityType::Leader(_)
            | EntityType::Unknown(_)
            // Non-plotting drawing-view border: never rendered, no properties.
            | EntityType::ViewBorder(_)
    ) {
        return None;
    }
    let common = entity.common();
    let material = match common.material_flags {
        0 => "ByLayer",
        1 => "ByBlock",
        _ => "Custom",
    };
    Some(PropSection {
        title: t!("3D Visualization").into_owned(),
        props: vec![Property {
            label: t!("Material").into_owned(),
            field: "material",
            value: PropValue::ReadOnly(material.into()),
        }],
    })
}

pub fn fallback_properties(_handle: Handle, entity: &EntityType) -> PropSection {
    PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![Property {
            label: t!("Type").into_owned(),
            field: "type",
            value: PropValue::ReadOnly(
                crate::t!(crate::entities::names::ui_name_or_class(entity)).into_owned(),
            ),
        }],
    }
}
