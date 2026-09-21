use acadrust::{EntityType, Handle, Transparency};
use crate::t;

use crate::scene::model::object::{PropSection, PropValue, Property};

pub fn general_section(entity: &EntityType) -> PropSection {
    let common = entity.common();
    let linetype_display = if common.linetype.is_empty() {
        "ByLayer".to_string()
    } else {
        common.linetype.clone()
    };
    let transp_display = match common.transparency {
        Transparency::ByLayer => "ByLayer".to_string(),
        Transparency::ByBlock => "ByBlock".to_string(),
        Transparency::Explicit(alpha) => {
            ((alpha as f64 / 255.0 * 100.0).round() as u32).to_string()
        }
    };
    let color_value = common.color_name.as_deref().map_or_else(
        || PropValue::ColorChoice(common.color),
        |identity| PropValue::NamedColorChoice {
            color: common.color,
            name: identity
                .split_once('$')
                .map(|(_, color_name)| color_name)
                .filter(|color_name| !color_name.is_empty())
                .unwrap_or(identity)
                .to_string(),
        },
    );

    let hyperlink = crate::scene::pe_url_description_of(entity)
        .or_else(|| crate::scene::pe_url_of(entity))
        .unwrap_or_default()
        .to_owned();

    let section = PropSection {
        title: t!("General").into_owned(),
        props: vec![
            Property {
                label: t!("Color").into_owned(),
                field: "color",
                value: color_value,
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
                value: PropValue::EditChoice {
                    value: transp_display,
                    options: vec!["ByLayer".to_string(), "ByBlock".to_string()],
                },
            },
            Property {
                label: t!("Hyperlink").into_owned(),
                field: "hyperlink",
                value: PropValue::Hyperlink(hyperlink),
            },
        ],
    };

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
            | EntityType::Wipeout(_)
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
        2 => "Global",
        _ => "Custom",
    };
    let mut options = vec![
        "ByLayer".to_string(),
        "ByBlock".to_string(),
        "Global".to_string(),
    ];
    if !options.iter().any(|o| o == material) && !material.is_empty() {
        options.push(material.to_string());
    }
    Some(PropSection {
        title: t!("3D Visualization").into_owned(),
        props: vec![Property {
            label: t!("Material").into_owned(),
            field: "material",
            value: PropValue::Choice {
                selected: material.to_string(),
                options,
            },
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
