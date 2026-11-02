//! Parameters Manager rows: one row per parameter component of a definition.
use super::block_actions::{self as actions, instance::ParameterFrame};
use super::block_authoring::{self as authoring, graph};
use super::block_parameter_properties::{self as properties};
use super::model::object::BlockParameterComponent as Component;
use codec::objects::{DynamicBlockData as Data, ObjectType};
use codec::{Document, EntityType, Handle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    ActionParameters,
    Attributes,
}

impl Group {
    /// The manager lists the action parameters first, then the attributes.
    pub const ALL: [Self; 2] = [Self::ActionParameters, Self::Attributes];
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::ActionParameters => "Action Parameters",
            Self::Attributes => "Attributes",
        }
    }
    fn rank(self) -> u8 {
        match self {
            Self::ActionParameters => 0,
            Self::Attributes => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub group: Group,
    pub name: String,
    pub expression: String,
    pub value: String,
    pub type_name: &'static str,
    /// Whether the row's two numeric cells name a quantity the manager writes
    /// back. A point's axes and a flip's state print without being numbers the
    /// manager has a rule for, so their cells stay as they are printed.
    pub numeric: bool,
    pub show: bool,
    pub order: String,
    pub description: String,
}

fn writable(parameter: &actions::Parameter, component: Component) -> bool {
    match component {
        Component::X | Component::Y => parameter.xy,
        Component::Distance => parameter.linear || parameter.polar,
        Component::Angle => parameter.polar || parameter.rotation,
        _ => false,
    }
}

fn parameter_type(p: &actions::Parameter) -> &'static str {
    if p.polar {
        "Polar"
    } else if p.linear {
        "Linear"
    } else if p.xy {
        "XY"
    } else if p.rotation {
        "Rotation"
    } else if p.flip {
        "Flip"
    } else {
        "Point"
    }
}

fn number(p: &actions::Parameter, component: Component) -> Result<f64, &'static str> {
    properties::number(
        p,
        ParameterFrame {
            base: p.base,
            end: p.end,
        },
        component,
    )
}

fn parameter_rows(doc: &Document, definition: Handle) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for p in actions::all_parameters(doc, definition) {
        let Ok(fields) = properties::descriptors(doc, definition, &p) else {
            continue;
        };
        let type_name = parameter_type(&p);
        for field in fields {
            // Rows are addressed by name, so two components printed under one
            // label would be one cell that writes both.
            if rows
                .iter()
                .any(|row| row.name.eq_ignore_ascii_case(&field.label))
            {
                continue;
            }
            let number = number(&p, field.component).ok();
            let expression = number.map_or_else(String::new, properties::stored);
            let value = number.map_or_else(String::new, |value| {
                properties::rendered(value, field.component)
            });
            rows.push(Row {
                group: Group::ActionParameters,
                name: field.label,
                expression,
                value,
                type_name,
                numeric: writable(&p, field.component),
                show: field.show,
                order: String::new(),
                description: field.description.to_owned(),
            });
        }
    }
    let Some(graph) = graph(doc, definition).ok().flatten() else {
        return rows;
    };
    for object in doc.objects.values() {
        let listed = match object {
            ObjectType::DynamicBlock(o) if o.owner == graph => match &o.data {
                Data::LookupParameter(p) => Some((
                    "Lookup",
                    p.parameter.parameter.show_properties,
                    p.lookup_name.clone(),
                )),
                _ => None,
            },
            ObjectType::BlockVisibilityParameter(p) if p.owner == graph => {
                Some(("Visibility", p.show_properties, p.name.clone()))
            }
            _ => None,
        };
        let Some((type_name, show, name)) = listed else {
            continue;
        };
        // Rows are keyed by name, and the list sorts case-insensitively, so a
        // lookup that differs only in case from a parameter is the same row.
        if name.is_empty()
            || rows
                .iter()
                .any(|row| row.name.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        rows.push(Row {
            group: Group::ActionParameters,
            name,
            expression: String::new(),
            value: String::new(),
            type_name,
            numeric: false,
            show,
            order: String::new(),
            description: String::new(),
        });
    }
    rows
}

fn attribute_rows(doc: &Document, definition: Handle) -> Vec<Row> {
    authoring::members(doc, definition)
        .into_iter()
        .filter_map(|handle| doc.get_entity(handle))
        .filter_map(|entity| match entity {
            EntityType::AttributeDefinition(a) => Some(a.tag.clone()),
            _ => None,
        })
        .map(|tag| Row {
            group: Group::Attributes,
            name: tag,
            expression: String::new(),
            value: String::new(),
            type_name: "Variable",
            numeric: false,
            show: true,
            order: String::new(),
            description: String::new(),
        })
        .collect()
}

/// Every row the manager lists: parameter components, then the attributes.
pub fn rows(doc: &Document, definition: Handle) -> Vec<Row> {
    crate::entities::common::set_unit_context(crate::entities::common::UnitContext::from_header(
        &doc.header,
    ));
    let mut rows = parameter_rows(doc, definition);
    // Rows are addressed by name, so an attribute tagged like a parameter would
    // be two rows answering to one name.
    let mut attributes = attribute_rows(doc, definition);
    attributes.retain(|attribute| {
        !rows
            .iter()
            .any(|parameter| parameter.name.eq_ignore_ascii_case(&attribute.name))
    });
    rows.extend(attributes);
    rows.sort_by(|a, b| {
        (a.group.rank(), a.name.to_lowercase()).cmp(&(b.group.rank(), b.name.to_lowercase()))
    });
    rows
}

/// Wildcard match for the filter field: `*` stands for any run of characters.
fn wildcard(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut resume) = (None, 0usize);
    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == text[ti] {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star = Some(pi);
            resume = ti;
            pi += 1;
        } else if let Some(from) = star {
            pi = from + 1;
            resume += 1;
            ti = resume;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// The filter field holds a name pattern (`*` lists all); the search field
/// narrows the rows that pattern left by a case-insensitive name substring.
pub fn matches(row: &Row, filter: &str, search: &str) -> bool {
    let listed = matches!(filter.trim(), "" | "*") || wildcard(filter.trim(), &row.name);
    let query = search.trim().to_lowercase();
    listed && (query.is_empty() || row.name.to_lowercase().contains(&query))
}

pub(crate) fn displayed<'a>(rows: &'a [Row], filter: &str, search: &str) -> Vec<&'a Row> {
    rows.iter()
        .filter(|row| matches(row, filter, search))
        .collect()
}

pub fn footer(total: usize, shown: usize) -> String {
    format!("All: {shown} of {total} parameters displayed")
}

/// Where a row points: the parameter object it names, and the component whose
/// number that row prints. A lookup or visibility row names an object but prints
/// no number of its own, so it has no component.
///
/// A row is opened from this once, and the answer is then written back by handle;
/// resolving the name again at commit would let a parameter deleted or renumbered
/// in between take the number.
pub fn target_of(
    doc: &Document,
    definition: Handle,
    name: &str,
) -> Option<(Handle, Option<Component>)> {
    for parameter in actions::all_parameters(doc, definition) {
        let Ok(fields) = properties::descriptors(doc, definition, &parameter) else {
            continue;
        };
        if let Some(field) = fields
            .into_iter()
            .find(|field| field.label.eq_ignore_ascii_case(name))
        {
            return Some((parameter.handle, Some(field.component)));
        }
    }
    let Ok(Some(graph)) = graph(doc, definition) else {
        return None;
    };
    for (handle, object) in doc.objects.iter() {
        let named = match object {
            ObjectType::DynamicBlock(object) if object.owner == graph => match &object.data {
                Data::LookupParameter(p) => Some(p.lookup_name.as_str()),
                _ => None,
            },
            ObjectType::BlockVisibilityParameter(p) if p.owner == graph => Some(p.name.as_str()),
            _ => None,
        };
        if named.is_some_and(|named| !named.is_empty() && named.eq_ignore_ascii_case(name)) {
            return Some((*handle, None));
        }
    }
    None
}

const NO_NUMBER: &str = "This row has no number to edit.";
/// A row whose `Show` cell is not a parameter's own visibility flag.
pub const NO_FLAG: &str = "This row has no visibility flag to change.";

/// Write a row's number into the parameter it points at. The endpoint is where
/// the row's own measure puts it: a distance along the parameter's direction, an
/// angle as a bearing from the base point, an XY axis as an offset from the base.
pub fn write_number(
    doc: &mut Document,
    definition: Handle,
    parameter: Handle,
    component: Component,
    input: f64,
) -> Result<bool, &'static str> {
    use super::block_authoring::edit::{move_part, Part};
    let authored = actions::all_parameters(doc, definition)
        .into_iter()
        .find(|p| p.handle == parameter)
        .ok_or(NO_NUMBER)?;
    let delta = authored.end - authored.base;
    let at = match component {
        Component::X if authored.xy => {
            glam::DVec3::new(authored.base.x + input, authored.end.y, authored.end.z)
        }
        Component::Y if authored.xy => {
            glam::DVec3::new(authored.end.x, authored.base.y + input, authored.end.z)
        }
        Component::Distance if authored.linear || authored.polar => {
            if input <= 1e-9 {
                return Err("Specify a positive parameter length.");
            }
            let axis = delta
                .try_normalize()
                .ok_or("The parameter direction is unavailable.")?;
            authored.base + axis * input
        }
        Component::Angle if authored.polar || authored.rotation => {
            let angle = input.rem_euclid(360.0).to_radians();
            authored.base + glam::DVec3::new(angle.cos(), angle.sin(), 0.0) * delta.length()
        }
        _ => return Err(NO_NUMBER),
    };
    move_part(
        doc,
        definition,
        parameter,
        Part::End,
        codec::types::Vector3::new(at.x, at.y, at.z),
    )
}

/// Flip whether the parameter shows its own values, as the row's `Show` cell
/// says. A component row shares its parameter's flag with the other rows of the
/// same parameter, which is what one flag for one parameter means.
pub fn toggle_show(
    doc: &mut Document,
    definition: Handle,
    parameter: Handle,
) -> Result<bool, &'static str> {
    use super::block_authoring::metadata;
    let visible = metadata::read(doc, definition, parameter)?
        .visible
        .ok_or(NO_FLAG)?;
    metadata::set_visible(doc, definition, parameter, !visible)
}
