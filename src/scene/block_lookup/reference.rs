//! Indexed reference choices for each visible output of a shared Lookup action.
use super::*;
use crate::scene::block_parameter_properties::Input;
use crate::scene::model::object::{
    BlockParameterComponent as Component, BlockParameterKey as Key, PropValue, Property as Row,
};
use std::hash::{Hash, Hasher};

fn revision(handle: Handle, table: &BlockLookupAction) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    handle.hash(&mut hash);
    table.action.element.eval.node_id.hash(&mut hash);
    table.row_count.hash(&mut hash);
    table.column_count.hash(&mut hash);
    for column in &table.columns {
        column.node_id.hash(&mut hash);
        column.value_type.hash(&mut hash);
        column.property_type.hash(&mut hash);
        column.lookup_property.hash(&mut hash);
        column.writable.hash(&mut hash);
        column.connection_name.hash(&mut hash);
        column.unmatched_name.hash(&mut hash);
    }
    table.expressions.hash(&mut hash);
    hash.finish()
}

pub(crate) fn reference_properties(
    document: &Document,
    baseline: Option<&Document>,
    document_id: u64,
    insert: Handle,
) -> Vec<Row> {
    let Some(definition) = crate::scene::block_actions::instance::definition(document, insert)
    else {
        return vec![];
    };
    let source = baseline.unwrap_or(document);
    let lookups = parameters(source, definition);
    let mut seen = std::collections::HashSet::new();
    let mut result = vec![];
    for (owner, _) in &lookups {
        let Ok(Some((handle, table))) = table(source, definition, *owner) else {
            continue;
        };
        if !seen.insert(handle) || validate(&table, Some(&table)).is_err() {
            continue;
        }
        let selected = reference_row_context(document, baseline, insert, definition, &table);
        for (column, output) in table
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.lookup_property)
        {
            let Some((_, parameter)) = lookups
                .iter()
                .find(|(_, p)| p.parameter.parameter.element.eval.node_id == output.node_id)
            else {
                continue;
            };
            if !parameter.parameter.parameter.show_properties {
                continue;
            }
            let options: Vec<_> = (0..table.row_count as usize)
                .map(|row| {
                    table.expressions[row * table.columns.len() + column]
                        .trim()
                        .to_owned()
                })
                .collect();
            let value = if output.writable {
                PropValue::BlockChoice {
                    key: Key {
                        document: document_id,
                        definition,
                        insert,
                        parameter: *owner,
                        component: Component::LookupOutput {
                            column,
                            revision: revision(handle, &table),
                        },
                    },
                    selected: selected.unwrap_or(usize::MAX),
                    options,
                    description: parameter.lookup_description.clone(),
                }
            } else {
                PropValue::ReadOnlyWithTooltip {
                    value: selected
                        .and_then(|row| options.get(row))
                        .cloned()
                        .unwrap_or_else(|| {
                            if output.unmatched_name.is_empty() {
                                "Custom".into()
                            } else {
                                output.unmatched_name.clone()
                            }
                        }),
                    tooltip: parameter.lookup_description.clone(),
                }
            };
            result.push(Row {
                label: parameter.lookup_name.clone(),
                field: "dyn_lookup",
                value,
            });
        }
    }
    result
}

pub(crate) fn evaluate_property(
    document: &Document,
    baseline: Option<&Document>,
    key: Key,
    column: usize,
    expected: u64,
    input: Input,
) -> Result<Option<Document>, &'static str> {
    let source = baseline.unwrap_or(document);
    let (handle, view) =
        table(source, key.definition, key.parameter)?.ok_or("The Lookup table is unavailable.")?;
    let (current_handle, current) = table(document, key.definition, key.parameter)?
        .ok_or("The Lookup table is unavailable.")?;
    if revision(handle, &view) != expected || revision(current_handle, &current) != expected {
        return Err("The reference Lookup property is stale; refresh Properties.");
    }
    let output = view
        .columns
        .get(column)
        .filter(|c| c.lookup_property && c.writable)
        .ok_or("The Lookup output is read-only or unavailable.")?;
    for doc in [source, document] {
        if !parameters(doc, key.definition).into_iter().any(|(_, p)| {
            p.parameter.parameter.element.eval.node_id == output.node_id
                && p.parameter.parameter.show_properties
        }) {
            return Err("The Lookup output is hidden or unavailable.");
        }
    }
    let Input::Choice(row) = input else {
        return Err("Choose a value from the Lookup output.");
    };
    evaluate_row_context(document, baseline, key.insert, key.parameter, row)
}
