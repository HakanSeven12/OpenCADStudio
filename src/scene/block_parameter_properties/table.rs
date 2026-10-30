use super::*;
use crate::scene::block_properties_table::{self as tables, TableView};
use std::hash::{Hash, Hasher};

fn revision(view: &TableView) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    view.handle.hash(&mut hash);
    view.table
        .parameter
        .parameter
        .element
        .eval
        .node_id
        .hash(&mut hash);
    view.must_match.hash(&mut hash);
    for column in &view.columns {
        column.handle.hash(&mut hash);
        column.node_id.hash(&mut hash);
        column.connection_name.hash(&mut hash);
        column.type_code.hash(&mut hash);
    }
    view.rows.len().hash(&mut hash);
    for row in &view.rows {
        row.len().hash(&mut hash);
        for value in row {
            value.to_bits().hash(&mut hash);
        }
    }
    hash.finish()
}

pub(super) fn current_row(
    doc: &Document,
    baseline: Option<&Document>,
    insert: Handle,
    definition: Handle,
    view: &TableView,
) -> Result<Option<usize>, &'static str> {
    let parameters = actions::all_parameters(baseline.unwrap_or(doc), definition);
    let values = view
        .columns
        .iter()
        .map(|column| {
            let p = parameters
                .iter()
                .find(|p| p.handle == column.handle)
                .ok_or("The table parameter is unavailable.")?;
            let frame = frame(doc, baseline, insert, p)?;
            let delta = frame.end - frame.base;
            let value = if column.connection_name.eq_ignore_ascii_case("UpdatedAngle") {
                delta.y.atan2(delta.x).rem_euclid(std::f64::consts::TAU)
            } else if column
                .connection_name
                .eq_ignore_ascii_case("UpdatedDistance")
            {
                delta.length()
            } else {
                return Err("The table property connection is unsupported.");
            };
            if !value.is_finite() {
                return Err("The table parameter value is not finite.");
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let matches = |row: usize| {
        view.rows[row]
            .iter()
            .zip(&values)
            .zip(&view.columns)
            .all(|((a, b), column)| {
                let difference = if column.connection_name.eq_ignore_ascii_case("UpdatedAngle") {
                    actions::angle_distance(*a, *b)
                } else {
                    (a - b).abs()
                };
                difference <= 1e-9
            })
    };
    Ok(instance::table_history_row(doc, insert, view)
        .filter(|row| matches(*row))
        .or_else(|| (0..view.rows.len()).find(|row| matches(*row))))
}

pub(super) fn property(
    doc: &Document,
    baseline: Option<&Document>,
    document: u64,
    insert: Handle,
    definition: Handle,
) -> Option<Property> {
    let view = tables::table(baseline.unwrap_or(doc), definition).ok()??;
    if !view.table.parameter.parameter.show_properties {
        return None;
    }
    let description = view
        .columns
        .iter()
        .map(|column| column.display_name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let value = match current_row(doc, baseline, insert, definition, &view) {
        Ok(selected) => PropValue::BlockChoice {
            key: Key {
                document,
                definition,
                insert,
                parameter: view.handle,
                component: Component::TableRow(revision(&view)),
            },
            selected: selected.unwrap_or(usize::MAX),
            options: view
                .rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let cells = row
                        .iter()
                        .zip(&view.columns)
                        .map(|(value, column)| {
                            if column.connection_name.eq_ignore_ascii_case("UpdatedAngle") {
                                format!("{}°", value.to_degrees())
                            } else {
                                value.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}: {cells}", index + 1)
                })
                .collect(),
            description,
        },
        Err(error) => PropValue::ReadOnlyWithTooltip {
            value: "Unavailable".into(),
            tooltip: error.into(),
        },
    };
    Some(Property {
        label: if view
            .table
            .parameter
            .parameter
            .element
            .name
            .trim()
            .is_empty()
        {
            "Block Table".into()
        } else {
            view.table.parameter.parameter.element.name.clone()
        },
        field: "dyn_table",
        value,
    })
}

pub(super) fn evaluate(
    doc: &Document,
    baseline: Option<&Document>,
    key: Key,
    expected: u64,
    input: Input,
) -> Result<Option<Document>, &'static str> {
    let view = tables::table(baseline.unwrap_or(doc), key.definition)?
        .ok_or("The Block Properties Table is unavailable.")?;
    let current =
        tables::table(doc, key.definition)?.ok_or("The Block Properties Table is unavailable.")?;
    if view.handle != key.parameter || revision(&view) != expected || revision(&current) != expected
    {
        return Err("The reference table property is stale; refresh Properties.");
    }
    if !view.table.parameter.parameter.show_properties
        || !current.table.parameter.parameter.show_properties
    {
        return Err("The reference table property is hidden.");
    }
    let Input::Choice(row) = input else {
        return Err("Choose a row from the Block Properties Table.");
    };
    if row >= view.rows.len() {
        return Err("The selected table row is unavailable.");
    }
    if baseline.is_some() {
        let Some(EntityType::Insert(reference)) = doc.get_entity(key.insert) else {
            return Err("Select a test reference.");
        };
        if doc.entities().any(|e| matches!(e, EntityType::Insert(other) if other.common.handle != key.insert
            && (other.block_name == reference.block_name || doc.dynamic_definition_for_insert(other.common.handle) == Some(key.definition)))) {
            return Err("Test actions with one reference in Test Block. Remove the extra references or reopen Test Block.");
        }
    }
    if current_row(doc, baseline, key.insert, key.definition, &view)? == Some(row) {
        return Ok(None);
    }
    let staged = if let Some(baseline) = baseline {
        instance::evaluate_test_table_row(doc, baseline, key.insert, &view.columns, row)
    } else {
        instance::evaluate_table_row(doc, key.insert, &view.columns, row)
    }?;
    if let Some(staged) = &staged {
        if current_row(staged, baseline, key.insert, key.definition, &view)? != Some(row) {
            return Err("The table row values do not satisfy the parameter value sets.");
        }
    }
    Ok(staged)
}
