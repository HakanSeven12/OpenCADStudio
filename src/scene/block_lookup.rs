//! Native Lookup parameter and property-table authoring.
use super::block_authoring::graph;
use super::block_authoring::{self as authoring, *};
use codec::objects::{
    BlockAction, BlockElement, BlockEvalValue, BlockGrip, BlockGripExpression, BlockLookupAction,
    BlockLookupColumn, BlockLookupParameter, BlockOnePointParameter, BlockParameter,
    DynamicBlockData, ObjectType,
};
use codec::{
    types::{Transform, Vector3},
    CadDocument, Handle,
};
#[cfg(test)]
mod tests;

fn graph_handle(document: &CadDocument, definition: Handle) -> Option<Handle> {
    let ObjectType::Dictionary(d) = document
        .objects
        .get(&document.extension_dictionary_handle(definition)?)?
    else {
        return None;
    };
    d.get("ACAD_ENHANCEDBLOCK")
}
pub(crate) fn parameters(
    document: &CadDocument,
    definition: Handle,
) -> Vec<(Handle, &BlockLookupParameter)> {
    let Some(graph) = graph_handle(document, definition) else {
        return vec![];
    };
    let Some(ObjectType::DynamicBlock(o)) = document.objects.get(&graph) else {
        return vec![];
    };
    let DynamicBlockData::EvaluationGraph(g) = &o.data else {
        return vec![];
    };
    g.nodes
        .iter()
        .filter_map(|n| match document.objects.get(&n.expression) {
            Some(ObjectType::DynamicBlock(o)) if o.owner == graph => match &o.data {
                DynamicBlockData::LookupParameter(p) => Some((o.handle, p)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}
pub(crate) fn placement_error(document: &CadDocument, definition: Handle) -> Option<&'static str> {
    authoring::graph(document, definition).err()
}
pub(crate) fn create(
    document: &mut CadDocument,
    definition: Handle,
    location: Vector3,
) -> Result<Handle, &'static str> {
    if ![location.x, location.y, location.z]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err("Specify a finite parameter location.");
    }
    let names = parameters(document, definition)
        .iter()
        .map(|(_, p)| p.lookup_name.clone())
        .collect::<Vec<_>>();
    let name = (1..)
        .map(|n| format!("Lookup{n}"))
        .find(|s| names.iter().all(|n| !n.eq_ignore_ascii_case(s)))
        .unwrap();
    let (graph, slots) = authoring::prepare(document, definition)?;
    register_classes(
        document,
        &[
            ("BLOCKLOOKUPPARAMETER", "AcDbBlockLookUpParameter", 55),
            ("BLOCKLOOKUPGRIP", "AcDbBlockLookUpGrip", 55),
            ("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr", 20),
        ],
    );
    let handles = [(); 4].map(|_| document.allocate_handle());
    let element = |offset, name: &str| BlockElement {
        eval: expression(slots.eval + offset, NO_VALUE, BlockEvalValue::None),
        name: name.into(),
        major: MAJOR,
        minor: MINOR,
        eed_1071: 0,
    };
    let parameter = BlockLookupParameter {
        parameter: BlockOnePointParameter {
            parameter: BlockParameter {
                element: element(0, "Lookup"),
                show_properties: true,
                chain_actions: false,
            },
            definition_point: location,
            property_count: slots.eval + 1,
            ..Default::default()
        },
        lookup_name: name,
        ..Default::default()
    };
    document.objects.insert(
        handles[0],
        ObjectType::DynamicBlock(dynamic(
            handles[0],
            graph,
            "BLOCKLOOKUPPARAMETER",
            "AcDbBlockLookUpParameter",
            DynamicBlockData::LookupParameter(parameter),
        )),
    );
    let grip = BlockGrip {
        element: element(1, "Grip"),
        flags_91: slots.eval + 2,
        flags_92: slots.eval + 3,
        location,
        insert_cycling: false,
        insert_cycling_weight: -1,
    };
    document.objects.insert(
        handles[1],
        ObjectType::DynamicBlock(dynamic(
            handles[1],
            graph,
            "BLOCKLOOKUPGRIP",
            "AcDbBlockLookUpGrip",
            DynamicBlockData::LookupGrip(grip),
        )),
    );
    for (offset, axis) in [(2, "UpdatedX"), (3, "UpdatedY")] {
        let h = handles[offset as usize];
        document.objects.insert(
            h,
            ObjectType::DynamicBlock(dynamic(
                h,
                graph,
                "BLOCKGRIPLOCATIONCOMPONENT",
                "AcDbBlockGripExpr",
                DynamicBlockData::GripLocationComponent(BlockGripExpression {
                    eval: expression(slots.eval + offset, 40, BlockEvalValue::Real(0.0)),
                    grip_type: slots.eval,
                    expression: axis.into(),
                }),
            )),
        );
    }
    authoring::append(document, graph, slots, handles);
    Ok(handles[0])
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Property {
    pub label: String,
    pub column: BlockLookupColumn,
    pub value: String,
}
/// These distance and angle bindings match the native Lookup connection names.
pub(crate) fn properties(document: &CadDocument, definition: Handle) -> Vec<Property> {
    super::block_distance_parameter::parameters(document, definition)
        .iter()
        .flat_map(|p| {
            let node_id = p.native.parameter.element.eval.node_id;
            let distance = (p.native.definition_end_point - p.native.definition_base_point)
                .length()
                .to_string();
            let mut properties = vec![Property {
                label: p.labels[0].clone(),
                column: BlockLookupColumn {
                    node_id,
                    value_type: 40,
                    property_type: 2,
                    connection_name: "UpdatedDistance".into(),
                    unmatched_name: "Custom".into(),
                    ..Default::default()
                },
                value: distance,
            }];
            if p.kind == super::block_distance_parameter::Kind::Polar {
                let delta = p.native.definition_end_point - p.native.definition_base_point;
                let angle = delta.y.atan2(delta.x).rem_euclid(std::f64::consts::TAU);
                properties.push(Property {
                    label: p.labels[1].clone(),
                    column: BlockLookupColumn {
                        node_id,
                        value_type: 40,
                        property_type: 2,
                        connection_name: "UpdatedAngle".into(),
                        unmatched_name: "Custom".into(),
                        ..Default::default()
                    },
                    value: angle.to_string(),
                });
            }
            properties
        })
        .collect()
}
pub(crate) fn table(
    document: &CadDocument,
    definition: Handle,
    parameter: Handle,
) -> Result<Option<(Handle, BlockLookupAction)>, &'static str> {
    let p = parameters(document, definition)
        .into_iter()
        .find(|(h, _)| *h == parameter)
        .ok_or("The Lookup parameter is no longer available.")?
        .1;
    if p.index == 0 {
        return Ok(None);
    }
    let graph = graph_handle(document, definition).ok_or("The authoring graph is unavailable.")?;
    document
        .objects
        .values()
        .find_map(|o| match o {
            ObjectType::DynamicBlock(o) if o.owner == graph => match &o.data {
                DynamicBlockData::LookupAction(a) if a.action.element.eval.node_id == p.index => {
                    Some((o.handle, a.clone()))
                }
                _ => None,
            },
            _ => None,
        })
        .map(Some)
        .ok_or("The Lookup action cannot be edited.")
}

/// Resolve the active row for a saved reference from its numeric input values.
fn reference_row(
    document: &CadDocument,
    insert: Handle,
    definition: Handle,
    table: &BlockLookupAction,
) -> Option<usize> {
    let params = super::block_actions::all_parameters(document, definition);
    let rows = usize::try_from(table.row_count).ok()?;
    let columns = table.columns.len();
    (0..rows).find(|row| {
        table.columns.iter().enumerate().all(|(column, property)| {
            if property.lookup_property {
                return true;
            }
            if property.value_type != 40 {
                return false;
            }
            let Some(parameter) = params.iter().find(|p| p.id == property.node_id) else {
                return false;
            };
            let is_distance = property
                .connection_name
                .eq_ignore_ascii_case("UpdatedDistance");
            let is_angle = property
                .connection_name
                .eq_ignore_ascii_case("UpdatedAngle");
            if !(is_distance && (parameter.linear || parameter.polar)
                || is_angle && parameter.polar)
            {
                return false;
            }
            let Ok(frame) = super::block_actions::instance::frame(document, insert, parameter)
            else {
                return false;
            };
            let Some(expected) = table
                .expressions
                .get(row * columns + column)
                .and_then(|value| value.parse::<f64>().ok())
            else {
                return false;
            };
            let actual = if is_angle {
                (frame.end.y - frame.base.y)
                    .atan2(frame.end.x - frame.base.x)
                    .rem_euclid(std::f64::consts::TAU)
            } else {
                frame.end.distance(frame.base)
            };
            let difference = if is_angle {
                (actual - expected + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI
            } else {
                actual - expected
            };
            actual.is_finite()
                && expected.is_finite()
                && difference.abs() <= 1e-8 * actual.abs().max(expected.abs()).max(1.0)
        })
    })
}

/// A writable Lookup output as a per-reference property choice.
pub(crate) fn reference_property(
    document: &CadDocument,
    insert: Handle,
) -> Option<crate::scene::model::object::Property> {
    let definition = super::block_actions::instance::definition(document, insert)?;
    for (parameter, lookup) in parameters(document, definition) {
        if !lookup.parameter.parameter.show_properties { continue; }
        let Some((_, table)) = table(document, definition, parameter).ok().flatten() else {
            continue;
        };
        let Some((column, _)) = table
            .columns
            .iter()
            .enumerate()
            .find(|(_, column)| column.lookup_property && column.writable)
        else {
            continue;
        };
        let rows = usize::try_from(table.row_count).ok()?;
        let options = (0..rows)
            .filter_map(|row| table.expressions.get(row * table.columns.len() + column))
            .map(|value| value.trim().to_string())
            .collect::<Vec<_>>();
        if options.is_empty() {
            continue;
        }
        let selected = reference_row(document, insert, definition, &table)
            .and_then(|row| table.expressions.get(row * table.columns.len() + column))
            .cloned()
            .unwrap_or_default();
        return Some(crate::scene::model::object::Property {
            label: lookup.lookup_name.clone(),
            field: "dyn_lookup",
            value: crate::scene::model::object::PropValue::Choice { selected, options },
        });
    }
    None
}

/// Apply a writable Lookup row to a saved reference and materialize its geometry.
pub(crate) fn evaluate_reference_value(
    document: &CadDocument,
    insert: Handle,
    lookup_parameter: Handle,
    selected: &str,
) -> Result<Option<CadDocument>, &'static str> {
    evaluate_value_context(document, None, insert, lookup_parameter, selected)
}

fn evaluate_value_context(
    document: &CadDocument, baseline: Option<&CadDocument>, insert: Handle,
    lookup_parameter: Handle, selected: &str,
) -> Result<Option<CadDocument>, &'static str> {
    let definition = super::block_actions::instance::definition(document, insert)
        .ok_or("Select a uniformly scaled dynamic block reference.")?;
    let _lookup = parameters(document, definition)
        .into_iter()
        .find(|(handle, _)| *handle == lookup_parameter)
        .ok_or("The Lookup parameter is unavailable.")?;
    let (_, table) =
        table(document, definition, lookup_parameter)?.ok_or("The Lookup table is unavailable.")?;
    let (output_column, _) = table
        .columns
        .iter()
        .enumerate()
        .find(|(_, column)| column.lookup_property && column.writable)
        .ok_or("The Lookup table has no writable output.")?;
    let rows = usize::try_from(table.row_count).map_err(|_| "The Lookup table is invalid.")?;
    let columns = table.columns.len();
    let row = (0..rows)
        .find(|row| {
            table
                .expressions
                .get(row * columns + output_column)
                .is_some_and(|value| value.trim().eq_ignore_ascii_case(selected.trim()))
        })
        .ok_or("Choose a value from the Lookup table.")?;
    if baseline.is_none() && reference_row(document, insert, definition, &table) == Some(row) {
        return Ok(None);
    }

    let parameters = super::block_actions::all_parameters(document, definition);
    let mut evaluated = document.clone();
    let frames = if baseline.is_some() { super::block_actions::instance::load_test_frames(document, &mut evaluated, insert, &parameters)? }
        else { super::block_actions::instance::load_frames(document, &mut evaluated, insert, &parameters)? };
    let mut endpoints = std::collections::BTreeMap::new();
    for (column, property) in table.columns.iter().enumerate() {
        if property.lookup_property {
            continue;
        }
        if property.value_type != 40 {
            return Err("This Lookup input property is unsupported.");
        }
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.id == property.node_id)
            .ok_or("A Lookup input property is unavailable.")?;
        let is_distance = property
            .connection_name
            .eq_ignore_ascii_case("UpdatedDistance");
        let is_angle = property
            .connection_name
            .eq_ignore_ascii_case("UpdatedAngle");
        if !(is_distance && (parameter.linear || parameter.polar) || is_angle && parameter.polar) {
            return Err("This Lookup input property is unsupported.");
        }
        let value = table
            .expressions
            .get(row * columns + column)
            .ok_or("The selected Lookup row is incomplete.")?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && (!is_distance || *value > 1e-9))
            .ok_or("The selected Lookup row has an invalid parameter value.")?;
        let endpoint = if let Some(endpoint) = endpoints.get_mut(&parameter.id) {
            endpoint
        } else {
            let current = frames[&parameter.handle].end;
            endpoints.entry(parameter.id).or_insert(current)
        };
        let base = frames[&parameter.handle].base;
        let delta = *endpoint - base;
        if is_distance {
            let axis = delta
                .try_normalize()
                .or_else(|| (parameter.end - parameter.base).try_normalize())
                .ok_or("A Lookup input parameter has no direction.")?;
            *endpoint = base + axis * value;
        } else {
            let radius = delta.length();
            *endpoint = base + glam::DVec3::new(value.cos(), value.sin(), 0.0) * radius;
        }
    }
    for parameter in &parameters {
        if let Some(endpoint) = endpoints.get(&parameter.id) {
            super::block_actions::set_value(
                &mut evaluated,
                parameter,
                super::block_actions::Value::Point(*endpoint),
            )?;
        }
    }
    super::block_actions::rebuild_framed(&mut evaluated, baseline.unwrap_or(document), definition, &frames)?;
    let values = parameters.iter().map(|parameter| super::block_actions::value(&evaluated, parameter)).collect::<Result<Vec<_>, _>>()?;
    let mut result = super::block_actions::instance::materialize(document, insert, definition, &evaluated)?;
    for (parameter, value) in parameters.into_iter().zip(values) {
        super::block_actions::instance::save(&mut result, insert, &parameter, value)?;
    }
    Ok(Some(result))
}

pub(crate) fn evaluate_reference_choice(
    document: &CadDocument,
    insert: Handle,
    selected: &str,
) -> Result<Option<CadDocument>, &'static str> {
    evaluate_choice_context(document, None, insert, selected)
}

pub(crate) fn evaluate_test_choice(
    document: &CadDocument, baseline: &CadDocument, insert: Handle, selected: &str,
) -> Result<Option<CadDocument>, &'static str> {
    evaluate_choice_context(document, Some(baseline), insert, selected)
}

fn evaluate_choice_context(
    document: &CadDocument, baseline: Option<&CadDocument>, insert: Handle, selected: &str,
) -> Result<Option<CadDocument>, &'static str> {
    let definition = super::block_actions::instance::definition(document, insert)
        .ok_or("Select a uniformly scaled dynamic block reference.")?;
    for (handle, _) in parameters(document, definition) {
        let Ok(Some((_, table))) = table(document, definition, handle) else {
            continue;
        };
        let Some((column, _)) = table
            .columns
            .iter()
            .enumerate()
            .find(|(_, column)| column.lookup_property && column.writable)
        else {
            continue;
        };
        let rows = usize::try_from(table.row_count).unwrap_or_default();
        if (0..rows).any(|row| {
            table
                .expressions
                .get(row * table.columns.len() + column)
                .is_some_and(|value| value.trim().eq_ignore_ascii_case(selected.trim()))
        }) {
            return if baseline.is_some() { evaluate_value_context(document, baseline, insert, handle, selected) }
                else { evaluate_reference_value(document, insert, handle, selected) };
        }
    }
    Err("Choose a value from the Lookup table.")
}
pub(crate) fn empty_table(parameter: &BlockLookupParameter) -> BlockLookupAction {
    BlockLookupAction {
        column_count: 1,
        columns: vec![BlockLookupColumn {
            node_id: parameter.parameter.parameter.element.eval.node_id,
            value_type: 1,
            property_type: 0,
            lookup_property: true,
            writable: true,
            connection_name: "lookupString".into(),
            unmatched_name: "Custom".into(),
        }],
        flag_280: true,
        ..Default::default()
    }
}
pub(crate) fn validate(
    table: &BlockLookupAction,
    original: Option<&BlockLookupAction>,
) -> Result<(), String> {
    let rows = usize::try_from(table.row_count).map_err(|_| "Invalid table size.")?;
    let cols = table.columns.len();
    if rows == 0
        || rows > 1000
        || cols < 2
        || cols > 64
        || table.column_count != cols as i32
        || table.expressions.len() != rows * cols
    {
        return Err("Add at least one input property and one complete row.".into());
    }
    if !table.columns.iter().any(|c| !c.lookup_property)
        || !table.columns.iter().any(|c| c.lookup_property)
    {
        return Err("A table needs input and lookup properties.".into());
    }
    let mut inputs = std::collections::BTreeSet::new();
    let mut outputs = vec![std::collections::BTreeSet::new(); cols];
    for row in 0..rows {
        let mut key = vec![];
        for (col, property) in table.columns.iter().enumerate() {
            let cell = &table.expressions[row * cols + col];
            if cell.trim().is_empty() || cell.chars().any(char::is_control) {
                return Err("Complete each cell without control characters.".into());
            }
            if property.lookup_property {
                if property.writable && !outputs[col].insert(cell.trim().to_lowercase()) {
                    return Err("Reverse lookup requires unique lookup values.".into());
                }
            } else {
                let numeric = cell.trim().parse::<f64>().ok().filter(|v| v.is_finite());
                if property.value_type == 40
                    && numeric.is_none()
                    && !original.is_some_and(|old| {
                        old.columns.iter().enumerate().any(|(index, column)| {
                            column == property
                                && old
                                    .expressions
                                    .iter()
                                    .skip(index)
                                    .step_by(old.columns.len())
                                    .any(|value| value == cell)
                        })
                    })
                {
                    return Err("Enter finite decimal input values. Existing expressions can be retained unchanged.".into());
                }
                key.push(
                    numeric
                        .map(|v| if v == 0.0 { "0".into() } else { v.to_string() })
                        .unwrap_or_else(|| cell.trim().to_lowercase()),
                );
            }
        }
        if !inputs.insert(key) {
            return Err("Each input-property row must be unique.".into());
        }
    }
    Ok(())
}
pub(crate) fn apply_table(
    document: &mut CadDocument,
    definition: Handle,
    parameter: Handle,
    edited: BlockLookupAction,
) -> Result<Handle, String> {
    let mut working = document.clone();
    let handle = apply_table_inner(&mut working, definition, parameter, edited)?;
    *document = working;
    Ok(handle)
}
fn apply_table_inner(
    document: &mut CadDocument,
    definition: Handle,
    parameter: Handle,
    mut edited: BlockLookupAction,
) -> Result<Handle, String> {
    let previous = table(document, definition, parameter).map_err(str::to_string)?;
    validate(&edited, previous.as_ref().map(|(_, t)| t))?;
    let (graph, slots) = authoring::prepare(document, definition).map_err(str::to_string)?;
    let handle = if let Some((handle, original)) = &previous {
        edited.action = original.action.clone();
        *handle
    } else {
        register_classes(
            document,
            &[("BLOCKLOOKUPACTION", "AcDbBlockLookupAction", 55)],
        );
        let h = document.allocate_handle();
        let p = parameters(document, definition)
            .into_iter()
            .find(|(h, _)| *h == parameter)
            .unwrap()
            .1;
        edited.action = BlockAction {
            element: BlockElement {
                eval: expression(slots.eval, NO_VALUE, BlockEvalValue::None),
                name: p.lookup_name.clone(),
                major: MAJOR,
                minor: MINOR,
                eed_1071: 0,
            },
            display_location: p.parameter.definition_point,
            ..Default::default()
        };
        authoring::append_single(document, graph, slots, h);
        document.objects.insert(
            h,
            ObjectType::DynamicBlock(dynamic(
                h,
                graph,
                "BLOCKLOOKUPACTION",
                "AcDbBlockLookupAction",
                DynamicBlockData::LookupAction(edited.clone()),
            )),
        );
        h
    };
    let node_id = edited.action.element.eval.node_id;
    graph::bind_ids(
        document,
        graph,
        node_id,
        &edited.columns.iter().map(|c| c.node_id).collect::<Vec<_>>(),
    )
    .map_err(str::to_string)?;
    if let Some(ObjectType::DynamicBlock(o)) = document.objects.get_mut(&handle) {
        o.data = DynamicBlockData::LookupAction(edited);
    }
    if let Some(ObjectType::DynamicBlock(o)) = document.objects.get_mut(&parameter) {
        if let DynamicBlockData::LookupParameter(p) = &mut o.data {
            p.index = node_id;
        }
    }
    Ok(handle)
}
pub(crate) fn reframe(
    document: &mut CadDocument,
    definition: Handle,
    transform: &Transform,
) -> usize {
    let Some(graph) = graph_handle(document, definition) else {
        return 0;
    };
    let owned = parameters(document, definition)
        .iter()
        .map(|(h, p)| (*h, p.parameter.property_count, p.index))
        .collect::<Vec<_>>();
    let mut changed = 0;
    for (handle, grip_id, action_id) in owned {
        if let Some(ObjectType::DynamicBlock(o)) = document.objects.get_mut(&handle) {
            if let DynamicBlockData::LookupParameter(p) = &mut o.data {
                p.parameter.definition_point = transform.apply(p.parameter.definition_point);
                changed += 1;
            }
        }
        for o in document.objects.values_mut() {
            if let ObjectType::DynamicBlock(o) = o {
                if o.owner != graph {
                    continue;
                }
                match &mut o.data {
                    DynamicBlockData::LookupGrip(g) if g.element.eval.node_id == grip_id => {
                        g.location = transform.apply(g.location);
                        changed += 1;
                    }
                    DynamicBlockData::LookupAction(a)
                        if action_id > 0 && a.action.element.eval.node_id == action_id =>
                    {
                        a.action.display_location = transform.apply(a.action.display_location);
                        changed += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    changed
}
