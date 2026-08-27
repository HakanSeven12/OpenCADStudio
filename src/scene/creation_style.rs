use acadrust::objects::ObjectType;
use acadrust::tables::{DimStyle, TextStyle};
use acadrust::{CadDocument, EntityType, Handle};

#[derive(Debug, Clone)]
pub struct TextCreationDefaults {
    pub style_name: String,
    pub height: f64,
    pub width_factor: f64,
    pub oblique_angle: f64,
}

#[derive(Debug, Clone)]
pub struct DimensionCreationDefaults {
    pub style_name: String,
    pub style_handle: Handle,
    pub text_style_name: String,
    pub text_height: f64,
    pub gap: f64,
    pub arrow_size: f64,
    pub jog_angle: f64,
    pub scale: f64,
    pub annotative: bool,
}

fn named_text_style(doc: &CadDocument, name: &str) -> Option<TextStyle> {
    doc.text_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            doc.text_styles
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case("Standard"))
        })
        .cloned()
}

fn named_dim_style(doc: &CadDocument, name: &str) -> Option<DimStyle> {
    doc.dim_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            doc.dim_styles
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case("Standard"))
        })
        .cloned()
}

pub fn current_text_defaults(doc: &CadDocument) -> TextCreationDefaults {
    let requested = doc.header.current_text_style_name.trim();
    let style = named_text_style(doc, requested);
    let variable_height = if doc.header.text_height > 1.0e-9 {
        doc.header.text_height
    } else {
        style
            .as_ref()
            .map(|style| style.last_height)
            .filter(|height| *height > 1.0e-9)
            .unwrap_or(2.5)
    };
    TextCreationDefaults {
        style_name: style
            .as_ref()
            .map(|style| style.name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Standard".to_string()),
        height: style
            .as_ref()
            .map(|style| style.height)
            .filter(|height| *height > 1.0e-9)
            .unwrap_or(variable_height),
        width_factor: style.as_ref().map_or(1.0, |style| style.width_factor),
        oblique_angle: style.as_ref().map_or(0.0, |style| style.oblique_angle),
    }
}

pub fn current_dimension_defaults(doc: &CadDocument) -> DimensionCreationDefaults {
    let requested = doc.header.current_dimstyle_name.trim();
    let style = named_dim_style(doc, requested);
    DimensionCreationDefaults {
        style_name: style
            .as_ref()
            .map(|style| style.name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Standard".to_string()),
        style_handle: style.as_ref().map_or(Handle::NULL, |style| style.handle),
        text_style_name: style
            .as_ref()
            .map(|style| style.dimtxsty.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| current_text_defaults(doc).style_name),
        text_height: style.as_ref().map_or(2.5, |style| style.dimtxt),
        gap: style.as_ref().map_or(0.625, |style| style.dimgap),
        arrow_size: style.as_ref().map_or(2.5, |style| style.dimasz),
        jog_angle: style
            .as_ref()
            .map_or(std::f64::consts::FRAC_PI_4, |style| style.dimjogang),
        scale: style.as_ref().map_or(1.0, |style| {
            if style.annotative {
                1.0
            } else if style.dimscale > 1.0e-9 {
                style.dimscale
            } else {
                1.0
            }
        }),
        annotative: style.as_ref().is_some_and(|style| style.annotative),
    }
}

fn entity_uses_default_style(name: &str) -> bool {
    name.trim().is_empty() || name.eq_ignore_ascii_case("Standard")
}

fn apply_text_defaults(doc: &CadDocument, entity: &mut EntityType) {
    let active = current_text_defaults(doc);
    let style_name = match entity {
        EntityType::Text(text) => &mut text.style,
        EntityType::MText(text) => &mut text.style,
        EntityType::AttributeEntity(attribute) => &mut attribute.text_style,
        EntityType::AttributeDefinition(attribute) => &mut attribute.text_style,
        _ => return,
    };
    if style_name.trim().is_empty() {
        style_name.clone_from(&active.style_name);
    }
    let resolved = named_text_style(doc, style_name)
        .unwrap_or_else(|| TextStyle::new(style_name.clone()));
    match entity {
        EntityType::Text(text) => {
            if resolved.height > 1.0e-9 {
                text.height = resolved.height;
            }
            if (text.width_factor - 1.0).abs() <= 1.0e-9 {
                text.width_factor = resolved.width_factor;
            }
            if text.oblique_angle.abs() <= 1.0e-9 {
                text.oblique_angle = resolved.oblique_angle;
            }
        }
        EntityType::MText(text) => {
            if resolved.height > 1.0e-9 {
                text.height = resolved.height;
            }
        }
        EntityType::AttributeEntity(attribute) => {
            if resolved.height > 1.0e-9 {
                attribute.height = resolved.height;
            }
            if (attribute.width_factor - 1.0).abs() <= 1.0e-9 {
                attribute.width_factor = resolved.width_factor;
            }
            if attribute.oblique_angle.abs() <= 1.0e-9 {
                attribute.oblique_angle = resolved.oblique_angle;
            }
            if attribute.text_generation_flags == 0 {
                attribute.text_generation_flags =
                    (if resolved.flags.backward { 2 } else { 0 })
                        | (if resolved.flags.upside_down { 4 } else { 0 });
            }
        }
        EntityType::AttributeDefinition(attribute) => {
            if resolved.height > 1.0e-9 {
                attribute.height = resolved.height;
            }
            if (attribute.width_factor - 1.0).abs() <= 1.0e-9 {
                attribute.width_factor = resolved.width_factor;
            }
            if attribute.oblique_angle.abs() <= 1.0e-9 {
                attribute.oblique_angle = resolved.oblique_angle;
            }
            if attribute.text_generation_flags == 0 {
                attribute.text_generation_flags =
                    (if resolved.flags.backward { 2 } else { 0 })
                        | (if resolved.flags.upside_down { 4 } else { 0 });
            }
        }
        _ => {}
    }
}

fn apply_dimension_defaults(doc: &CadDocument, entity: &mut EntityType) {
    let active = current_dimension_defaults(doc);
    match entity {
        EntityType::Dimension(dimension) => {
            if entity_uses_default_style(&dimension.base().style_name) {
                dimension.base_mut().style_name.clone_from(&active.style_name);
            }
        }
        EntityType::Leader(leader) => {
            if entity_uses_default_style(&leader.dimension_style) {
                leader.dimension_style.clone_from(&active.style_name);
                leader.text_height = active.text_height;
                leader.dimension_gap = active.gap;
                leader.arrow_size = active.arrow_size;
            }
        }
        EntityType::Tolerance(tolerance) => {
            if entity_uses_default_style(&tolerance.dimension_style_name) {
                tolerance.dimension_style_name.clone_from(&active.style_name);
                tolerance.dimension_style_handle = (!active.style_handle.is_null())
                    .then_some(active.style_handle);
                tolerance.text_height = active.text_height;
                tolerance.dimension_gap = active.gap;
            }
        }
        _ => {}
    }
}

fn apply_object_defaults(doc: &CadDocument, entity: &mut EntityType) {
    match entity {
        EntityType::MultiLeader(leader) if leader.style_handle.is_none() => {
            let name = doc.header.current_mleader_style_name.trim();
            if let Some(style) = doc.objects.iter().find_map(|(handle, object)| match object {
                ObjectType::MultiLeaderStyle(style) if style.name.eq_ignore_ascii_case(name) => {
                    let mut style = style.clone();
                    style.handle = *handle;
                    Some(style)
                }
                _ => None,
            }) {
                crate::scene::annotative::apply_mleader_style(leader, &style);
            }
        }
        EntityType::Table(table) if table.table_style_handle.is_none() => {
            let name = doc.header.current_table_style_name.trim();
            table.table_style_handle = doc.objects.iter().find_map(|(handle, object)| match object {
                ObjectType::TableStyle(style) if style.name.eq_ignore_ascii_case(name) => {
                    Some(*handle)
                }
                _ => None,
            });
        }
        EntityType::MLine(mline) if mline.style_handle.is_none() => {
            let name = doc.header.multiline_style.trim();
            if let Some((handle, style_name)) =
                doc.objects.iter().find_map(|(handle, object)| match object {
                    ObjectType::MLineStyle(style) if style.name.eq_ignore_ascii_case(name) => {
                        Some((*handle, style.name.clone()))
                    }
                    _ => None,
                })
            {
                mline.style_handle = Some(handle);
                mline.style_name = style_name;
            }
        }
        _ => {}
    }
}

pub fn apply_current_creation_styles(doc: &CadDocument, entity: &mut EntityType) {
    apply_text_defaults(doc, entity);
    apply_dimension_defaults(doc, entity);
    apply_object_defaults(doc, entity);
}
