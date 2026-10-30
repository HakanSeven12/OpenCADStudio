//! Native parameter text and visibility metadata, validated against definition ownership.
use codec::objects::{DynamicBlockData as Data, ObjectType};
use codec::{Document, Handle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Name,
    Description,
    SecondaryName,
    SecondaryDescription,
    BaseStateLabel,
    FlippedStateLabel,
}
impl Field {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Name => "authoring_name",
            Self::Description => "authoring_description",
            Self::SecondaryName => "authoring_secondary_name",
            Self::SecondaryDescription => "authoring_secondary_description",
            Self::BaseStateLabel => "authoring_base_state_label",
            Self::FlippedStateLabel => "authoring_flipped_state_label",
        }
    }
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        [
            Self::Name,
            Self::Description,
            Self::SecondaryName,
            Self::SecondaryDescription,
            Self::BaseStateLabel,
            Self::FlippedStateLabel,
        ]
        .into_iter()
        .find(|field| field.key() == key)
    }
    fn is_name(self) -> bool {
        matches!(self, Self::Name | Self::SecondaryName)
    }
}
pub(crate) struct Text<'a> {
    pub field: Field,
    pub label: &'static str,
    pub value: &'a str,
}
pub(crate) struct Metadata<'a> {
    pub text: Vec<Text<'a>>,
    pub visible: Option<bool>,
}

fn object(
    document: &Document,
    definition: Handle,
    handle: Handle,
) -> Result<&ObjectType, &'static str> {
    let graph =
        super::graph(document, definition)?.ok_or("The block authoring graph is unavailable.")?;
    let Some(ObjectType::DynamicBlock(graph_object)) = document.objects.get(&graph) else {
        return Err("The block authoring graph is unavailable.");
    };
    let Data::EvaluationGraph(nodes) = &graph_object.data else {
        return Err("The block authoring graph is unavailable.");
    };
    let object = document
        .objects
        .get(&handle)
        .ok_or("The block parameter is unavailable.")?;
    let owned = match object {
        ObjectType::DynamicBlock(p) => p.handle == handle && p.owner == graph,
        ObjectType::BlockVisibilityParameter(p) => p.handle == handle && p.owner == graph,
        _ => false,
    };
    if !owned || !nodes.nodes.iter().any(|node| node.expression == handle) {
        return Err("The parameter does not belong to this block definition.");
    }
    Ok(object)
}
fn describe(object: &ObjectType) -> Result<Metadata<'_>, &'static str> {
    use Field::*;
    let mut text = Vec::new();
    let mut visible = None;
    match object {
        ObjectType::BlockVisibilityParameter(p) => {
            text.push(Text {
                field: Name,
                label: "Parameter name",
                value: &p.name,
            });
            text.push(Text {
                field: Description,
                label: "Description",
                value: &p.description,
            });
            visible = Some(p.show_properties);
        }
        ObjectType::DynamicBlock(object) => match &object.data {
            Data::PointParameter(p) => {
                visible = Some(p.parameter.parameter.show_properties);
                text.push(Text {
                    field: Name,
                    label: "Parameter name",
                    value: &p.position_name,
                });
                text.push(Text {
                    field: Description,
                    label: "Description",
                    value: &p.position_description,
                });
            }
            Data::LinearParameter(p) => {
                visible = Some(p.parameter.parameter.show_properties);
                text.push(Text {
                    field: Name,
                    label: "Distance name",
                    value: &p.distance_name,
                });
                text.push(Text {
                    field: Description,
                    label: "Distance description",
                    value: &p.distance_description,
                });
            }
            Data::PolarParameter(p) => {
                visible = Some(p.parameter.parameter.show_properties);
                text.push(Text {
                    field: Name,
                    label: "Distance name",
                    value: &p.distance_name,
                });
                text.push(Text {
                    field: Description,
                    label: "Distance description",
                    value: &p.distance_description,
                });
                text.push(Text {
                    field: SecondaryName,
                    label: "Angle name",
                    value: &p.angle_name,
                });
                text.push(Text {
                    field: SecondaryDescription,
                    label: "Angle description",
                    value: &p.angle_description,
                });
            }
            Data::XYParameter(p) => {
                visible = Some(p.parameter.parameter.show_properties);
                text.push(Text {
                    field: Name,
                    label: "X name",
                    value: &p.x_label,
                });
                text.push(Text {
                    field: Description,
                    label: "X description",
                    value: &p.x_label_description,
                });
                text.push(Text {
                    field: SecondaryName,
                    label: "Y name",
                    value: &p.y_label,
                });
                text.push(Text {
                    field: SecondaryDescription,
                    label: "Y description",
                    value: &p.y_label_description,
                });
            }
            Data::RotationParameter(p) => {
                visible = Some(p.parameter.parameter.show_properties);
                text.push(Text {
                    field: Name,
                    label: "Angle name",
                    value: &p.angle_name,
                });
                text.push(Text {
                    field: Description,
                    label: "Angle description",
                    value: &p.angle_description,
                });
            }
            Data::FlipParameter(p) => {
                visible = Some(p.parameter.parameter.show_properties);
                text.push(Text {
                    field: Name,
                    label: "Parameter name",
                    value: &p.flip_label,
                });
                text.push(Text {
                    field: Description,
                    label: "Description",
                    value: &p.flip_label_description,
                });
                text.push(Text {
                    field: BaseStateLabel,
                    label: "Base state label",
                    value: &p.base_state_label,
                });
                text.push(Text {
                    field: FlippedStateLabel,
                    label: "Flipped state label",
                    value: &p.flipped_state_label,
                });
            }
            Data::LookupParameter(p) => {
                text.push(Text {
                    field: Name,
                    label: "Parameter name",
                    value: &p.lookup_name,
                });
                text.push(Text {
                    field: Description,
                    label: "Description",
                    value: &p.lookup_description,
                });
                visible = Some(p.parameter.parameter.show_properties);
            }
            Data::AlignmentParameter(p) => {
                text.push(Text {
                    field: Name,
                    label: "Parameter name",
                    value: &p.parameter.parameter.element.name,
                });
            }
            Data::BasePointParameter(p) => {
                text.push(Text {
                    field: Name,
                    label: "Parameter name",
                    value: &p.parameter.parameter.element.name,
                });
            }
            _ => return Err("This authoring object has no editable parameter metadata."),
        },
        _ => return Err("This authoring object has no editable parameter metadata."),
    }
    Ok(Metadata { text, visible })
}
pub(crate) fn read(
    document: &Document,
    definition: Handle,
    handle: Handle,
) -> Result<Metadata<'_>, &'static str> {
    describe(object(document, definition, handle)?)
}
fn text_mut(object: &mut ObjectType, field: Field) -> Option<&mut String> {
    use Field::*;
    match object {
        ObjectType::BlockVisibilityParameter(p) => match field {
            Name => Some(&mut p.name),
            Description => Some(&mut p.description),
            _ => None,
        },
        ObjectType::DynamicBlock(object) => match (&mut object.data, field) {
            (Data::PointParameter(p), Name) => Some(&mut p.position_name),
            (Data::PointParameter(p), Description) => Some(&mut p.position_description),
            (Data::LinearParameter(p), Name) => Some(&mut p.distance_name),
            (Data::LinearParameter(p), Description) => Some(&mut p.distance_description),
            (Data::PolarParameter(p), Name) => Some(&mut p.distance_name),
            (Data::PolarParameter(p), Description) => Some(&mut p.distance_description),
            (Data::PolarParameter(p), SecondaryName) => Some(&mut p.angle_name),
            (Data::PolarParameter(p), SecondaryDescription) => Some(&mut p.angle_description),
            (Data::XYParameter(p), Name) => Some(&mut p.x_label),
            (Data::XYParameter(p), Description) => Some(&mut p.x_label_description),
            (Data::XYParameter(p), SecondaryName) => Some(&mut p.y_label),
            (Data::XYParameter(p), SecondaryDescription) => Some(&mut p.y_label_description),
            (Data::RotationParameter(p), Name) => Some(&mut p.angle_name),
            (Data::RotationParameter(p), Description) => Some(&mut p.angle_description),
            (Data::FlipParameter(p), Name) => Some(&mut p.flip_label),
            (Data::FlipParameter(p), Description) => Some(&mut p.flip_label_description),
            (Data::FlipParameter(p), BaseStateLabel) => Some(&mut p.base_state_label),
            (Data::FlipParameter(p), FlippedStateLabel) => Some(&mut p.flipped_state_label),
            (Data::LookupParameter(p), Name) => Some(&mut p.lookup_name),
            (Data::LookupParameter(p), Description) => Some(&mut p.lookup_description),
            (Data::AlignmentParameter(p), Name) => Some(&mut p.parameter.parameter.element.name),
            (Data::BasePointParameter(p), Name) => Some(&mut p.parameter.parameter.element.name),
            _ => None,
        },
        _ => None,
    }
}
fn sync_visibility(document: &mut Document, handle: Handle) {
    if let Some(ObjectType::BlockVisibilityParameter(parameter)) = document.objects.get(&handle) {
        super::super::block_visibility::store(document, parameter.clone());
    }
}
pub(crate) fn set_text(
    document: &mut Document,
    definition: Handle,
    handle: Handle,
    field: Field,
    input: &str,
) -> Result<bool, &'static str> {
    let value = if field.is_name() { input.trim() } else { input };
    if value.contains('\0') {
        return Err("Parameter text cannot contain a null character.");
    }
    if field.is_name() && value.is_empty() {
        return Err("Parameter name cannot be empty.");
    }
    let metadata = read(document, definition, handle)?;
    let old = metadata
        .text
        .iter()
        .find(|text| text.field == field)
        .ok_or("This parameter does not support that text field.")?;
    if old.value == value {
        return Ok(false);
    }
    if field.is_name() {
        let graph = super::graph(document, definition)?.unwrap();
        let Some(ObjectType::DynamicBlock(graph)) = document.objects.get(&graph) else {
            unreachable!()
        };
        let Data::EvaluationGraph(graph) = &graph.data else {
            unreachable!()
        };
        for node in &graph.nodes {
            if let Ok(other) = read(document, definition, node.expression) {
                if other.text.iter().any(|text| {
                    text.field.is_name()
                        && (node.expression != handle || text.field != field)
                        && text.value.eq_ignore_ascii_case(value)
                }) {
                    return Err("Parameter names must be unique.");
                }
            }
        }
    }
    let tables = if field.is_name() {
        crate::scene::block_lookup::renamed_input_tables(
            document, definition, handle, field, old.value, value,
        )?
    } else {
        vec![]
    };
    *text_mut(document.objects.get_mut(&handle).unwrap(), field).unwrap() = value.to_owned();
    for (table, edited) in tables {
        let ObjectType::DynamicBlock(object) = document.objects.get_mut(&table).unwrap() else {
            unreachable!()
        };
        object.data = Data::LookupAction(edited);
    }
    sync_visibility(document, handle);
    Ok(true)
}
pub(crate) fn set_visible(
    document: &mut Document,
    definition: Handle,
    handle: Handle,
    value: bool,
) -> Result<bool, &'static str> {
    let old = read(document, definition, handle)?
        .visible
        .ok_or("This parameter has no supported reference property row.")?;
    if old == value {
        return Ok(false);
    }
    match document.objects.get_mut(&handle).unwrap() {
        ObjectType::BlockVisibilityParameter(p) => p.show_properties = value,
        ObjectType::DynamicBlock(object) => match &mut object.data {
            Data::PointParameter(p) => p.parameter.parameter.show_properties = value,
            Data::LinearParameter(p) => p.parameter.parameter.show_properties = value,
            Data::PolarParameter(p) => p.parameter.parameter.show_properties = value,
            Data::XYParameter(p) => p.parameter.parameter.show_properties = value,
            Data::RotationParameter(p) => p.parameter.parameter.show_properties = value,
            Data::FlipParameter(p) => p.parameter.parameter.show_properties = value,
            Data::LookupParameter(p) => p.parameter.parameter.show_properties = value,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
    sync_visibility(document, handle);
    Ok(true)
}

#[cfg(test)]
mod tests;
