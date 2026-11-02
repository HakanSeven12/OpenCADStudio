//! Native reference Properties values share the action-grip evaluator and caches.
use super::block_actions::{self as actions, instance, Parameter, Value};
use super::model::object::{
    BlockParameterComponent as Component, BlockParameterKey as Key, PropValue, Property,
};
use codec::objects::{DynamicBlockData as Data, ObjectType};
use codec::{Document, EntityType, Handle};
use glam::DVec3;

mod table;

pub(crate) fn current_table_row(doc: &Document, baseline: Option<&Document>, insert: Handle) -> Option<usize> {
    let definition = instance::definition(doc, insert)?;
    let view = super::block_properties_table::table(baseline.unwrap_or(doc), definition).ok()??;
    table::current_row(doc, baseline, insert, definition, &view).ok().flatten()
}

struct Descriptor<'a> {
    component: Component,
    label: String,
    description: &'a str,
}
fn descriptors<'a>(
    doc: &'a Document,
    definition: Handle,
    p: &Parameter,
) -> Result<Vec<Descriptor<'a>>, &'static str> {
    super::block_authoring::metadata::read(doc, definition, p.handle)?;
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&p.handle) else {
        return Err("The reference parameter is unavailable.");
    };
    let mut result = Vec::new();
    let mut add = |component, label: &str, description| {
        result.push(Descriptor {
            component,
            label: label.to_owned(),
            description,
        })
    };
    match &object.data {
        Data::PointParameter(p) if p.parameter.parameter.show_properties => {
            add(
                Component::X,
                &format!("{} X", p.position_name),
                &p.position_description,
            );
            add(
                Component::Y,
                &format!("{} Y", p.position_name),
                &p.position_description,
            );
        }
        Data::LinearParameter(p) if p.parameter.parameter.show_properties => add(
            Component::Distance,
            &p.distance_name,
            &p.distance_description,
        ),
        Data::PolarParameter(p) if p.parameter.parameter.show_properties => {
            add(
                Component::Distance,
                &p.distance_name,
                &p.distance_description,
            );
            add(Component::Angle, &p.angle_name, &p.angle_description);
        }
        Data::XYParameter(p) if p.parameter.parameter.show_properties => {
            add(Component::X, &p.x_label, &p.x_label_description);
            add(Component::Y, &p.y_label, &p.y_label_description);
        }
        Data::RotationParameter(p) if p.parameter.parameter.show_properties => {
            add(Component::Angle, &p.angle_name, &p.angle_description)
        }
        Data::FlipParameter(p) if p.parameter.parameter.show_properties => {
            add(Component::Flip, &p.flip_label, &p.flip_label_description)
        }
        _ => {}
    }
    Ok(result)
}
fn flip_labels(doc: &Document, parameter: Handle) -> Vec<String> {
    [false, true]
        .into_iter()
        .filter_map(|state| flip_state_label(doc, parameter, state))
        .collect()
}

/// Empty native labels use UI defaults; authored labels are never translated.
pub(crate) fn flip_state_label(
    doc: &Document,
    parameter: Handle,
    state: bool,
) -> Option<String> {
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&parameter) else {
        return None;
    };
    let Data::FlipParameter(p) = &object.data else {
        return None;
    };
    let label = if state {
        &p.flipped_state_label
    } else {
        &p.base_state_label
    };
    Some(if label.is_empty() {
        if state {
            crate::tr!("block-grip", "flipped")
        } else {
            crate::tr!("block-grip", "not-flipped")
        }
    } else {
        label.clone()
    })
}

fn frame(
    doc: &Document,
    baseline: Option<&Document>,
    insert: Handle,
    p: &Parameter,
) -> Result<instance::ParameterFrame, &'static str> {
    if baseline.is_some() {
        instance::test_frame(doc, insert, p)
    } else {
        instance::frame(doc, insert, p)
    }
}
fn number(
    p: &Parameter,
    frame: instance::ParameterFrame,
    component: Component,
) -> Result<f64, &'static str> {
    let delta = frame.end - frame.base;
    let value = match component {
        Component::X if p.point || p.xy => delta.x,
        Component::Y if p.point || p.xy => delta.y,
        Component::Distance if p.linear || p.polar => delta.length(),
        Component::Angle if p.polar || p.rotation => delta
            .y
            .atan2(delta.x)
            .rem_euclid(std::f64::consts::TAU)
            .to_degrees(),
        _ => return Err("This parameter does not support that property component."),
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err("The reference parameter value is not finite.")
    }
}

/// Stable component name shared by the row id and the protocol metadata.
pub(crate) fn component_id(component: &Component) -> String {
    match component {
        Component::X => "X".to_owned(),
        Component::Y => "Y".to_owned(),
        Component::Distance => "Distance".to_owned(),
        Component::Angle => "Angle".to_owned(),
        Component::Flip => "Flip".to_owned(),
        Component::TableRow(_) => "TableRow".to_owned(),
        Component::LookupOutput { column, .. } => format!("Lookup{column}"),
    }
}

/// One dynamic row per parameter component, so a caller can address it alone.
pub(crate) fn field_id(key: &Key) -> String {
    format!(
        "dyn_{:X}_{}",
        key.parameter.value(),
        component_id(&key.component).to_ascii_lowercase()
    )
}

/// Rows follow the drawing's precision: lengths at LUPREC, angles at AUPREC
/// as plain numbers (the reference prints parameter angles without a mark).
fn display(value: f64, component: Component) -> String {
    if component == Component::Angle {
        let precision = crate::entities::common::unit_context()
            .auprec
            .max(0)
            .min(15) as usize;
        return crate::entities::common::without_negative_zero(format!(
            "{:.*}",
            precision, value
        ));
    }
    crate::entities::common::format_length(value)
}

/// Values are in definition-plane drawing units, with angles in degrees.
pub(crate) fn properties(
    doc: &Document,
    baseline: Option<&Document>,
    document: u64,
    insert: Handle,
) -> Vec<Property> {
    // The rows print with the drawing's LUNITS/LUPREC/AUPREC, so a headless
    // caller or a non-active tab never formats with another drawing's settings.
    crate::entities::common::set_unit_context(
        crate::entities::common::UnitContext::from_header(&doc.header),
    );
    let Some(definition) = instance::definition(doc, insert) else {
        return vec![];
    };
    let source = baseline.unwrap_or(doc);
    let mut rows = Vec::new();
    for p in actions::all_parameters(source, definition) {
        let Ok(fields) = descriptors(source, definition, &p) else {
            continue;
        };
        for field in fields {
            let key = Key {
                document,
                definition,
                insert,
                parameter: p.handle,
                component: field.component,
            };
            let value = if p.flip {
                let value = if baseline.is_some() {
                    instance::test_value(doc, insert, &p)
                } else {
                    instance::value(doc, insert, &p)
                };
                value.and_then(|value| match value {
                    Value::Flip(state) => Ok(PropValue::BlockChoice {
                        key,
                        selected: usize::from(state),
                        options: flip_labels(source, p.handle),
                        description: field.description.to_owned(),
                    }),
                    _ => Err("The reference Flip state is unavailable."),
                })
            } else {
                frame(doc, baseline, insert, &p)
                    .and_then(|frame| number(&p, frame, field.component))
                    .map(|number| PropValue::BlockNumber {
                        key,
                        value: display(number, field.component),
                        description: field.description.to_owned(),
                    })
            };
            rows.push(Property {
                label: field.label,
                field: "dyn_parameter",
                value: value.unwrap_or_else(|error| PropValue::ReadOnlyWithTooltip {
                    value: "Unavailable".into(),
                    tooltip: error.into(),
                }),
            });
        }
    }
    rows.extend(table::property(doc, baseline, document, insert, definition));
    rows.extend(super::block_lookup::reference_properties(doc, baseline, document, insert));
    rows
}
#[derive(Clone, Copy)]
pub(crate) enum Input {
    Number(f64),
    Choice(usize),
}

/// Stage the same complete-frame evaluation used by native grips.
pub(crate) fn evaluate(
    doc: &Document,
    baseline: Option<&Document>,
    key: Key,
    input: Input,
) -> Result<Option<Document>, &'static str> {
    if instance::definition(doc, key.insert) != Some(key.definition) {
        return Err("The reference parameter property is stale.");
    }
    let source = baseline.unwrap_or(doc);
    if let Component::TableRow(revision) = key.component {
        return table::evaluate(doc, baseline, key, revision, input);
    }
    if let Component::LookupOutput { column, revision } = key.component {
        return super::block_lookup::evaluate_property(doc, baseline, key, column, revision, input);
    }
    let p = actions::all_parameters(source, key.definition)
        .into_iter()
        .find(|p| p.handle == key.parameter)
        .ok_or("The reference parameter is unavailable.")?;
    if !descriptors(source, key.definition, &p)?
        .iter()
        .any(|field| field.component == key.component)
    {
        return Err("The reference parameter property is hidden or unavailable.");
    }
    let value = match input {
        Input::Choice(state) if key.component == Component::Flip && state < 2 && p.flip => {
            Value::Flip(state != 0)
        }
        Input::Number(input) if input.is_finite() && !p.flip => {
            let mut frame = frame(doc, baseline, key.insert, &p)?;
            // Check component compatibility before modifying the endpoint.
            number(&p, frame, key.component)?;
            let delta = frame.end - frame.base;
            match key.component {
                Component::X => frame.end.x = frame.base.x + input,
                Component::Y => frame.end.y = frame.base.y + input,
                Component::Distance => {
                    if input <= 1e-9 {
                        return Err("Specify a positive parameter length.");
                    }
                    let axis = if p.linear {
                        (p.end - p.base).normalize()
                    } else {
                        delta
                            .try_normalize()
                            .ok_or("The parameter direction is unavailable.")?
                    };
                    frame.end = frame.base + axis * input;
                }
                Component::Angle => {
                    let angle = input.rem_euclid(360.).to_radians();
                    frame.end =
                        frame.base + DVec3::new(angle.cos(), angle.sin(), 0.) * delta.length();
                }
                Component::Flip | Component::TableRow(_) | Component::LookupOutput { .. } => unreachable!(),
            }
            if !frame.end.is_finite() {
                return Err("Specify a finite parameter value.");
            }
            Value::Frame {
                base: frame.base,
                end: frame.end,
            }
        }
        _ => return Err("Specify a finite numeric value or a valid parameter state."),
    };
    if let Some(baseline) = baseline {
        let Some(EntityType::Insert(insert)) = doc.get_entity(key.insert) else {
            return Err("Select a test reference.");
        };
        if doc.entities().any(|e| matches!(e, EntityType::Insert(other) if other.common.handle != key.insert && (other.block_name == insert.block_name || doc.dynamic_definition_for_insert(other.common.handle) == Some(key.definition)))) {
            return Err("Test actions with one reference in Test Block. Remove the extra references or reopen Test Block.");
        }
        instance::evaluate_test_value(
            doc,
            baseline,
            key.definition,
            key.insert,
            key.parameter,
            value,
        )
    } else {
        instance::evaluate_value(doc, key.insert, key.parameter, value)
    }
}

#[cfg(test)]
mod tests;
