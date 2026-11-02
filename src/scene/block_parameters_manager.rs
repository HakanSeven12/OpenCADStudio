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
    pub show: bool,
    pub order: String,
    pub description: String,
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
    let mut rows = Vec::new();
    for p in actions::all_parameters(doc, definition) {
        let Ok(fields) = properties::descriptors(doc, definition, &p) else {
            continue;
        };
        let type_name = parameter_type(&p);
        for field in fields {
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
    rows.extend(attribute_rows(doc, definition));
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
