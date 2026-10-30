//! Native Block Properties Table authoring.
use super::block_authoring::{self as authoring, *};
use codec::objects::{
    BlockElement, BlockEvalValue, BlockGrip, BlockGripExpression, BlockOnePointParameter,
    BlockParameter, BlockPropertiesTable, BlockPropertiesTableField, BlockPropertiesTableValue,
    DynamicBlockData, ObjectType,
};
use codec::{Document, Handle};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Column {
    pub handle: Handle,
    pub node_id: i32,
    pub connection_name: String,
    pub display_name: String,
    pub type_code: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TableView {
    pub handle: Handle,
    pub table: BlockPropertiesTable,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<f64>>,
    pub row_start: usize,
    pub trailer_start: usize,
    pub must_match: bool,
    pub column_metadata: Vec<Vec<BlockPropertiesTableField>>,
}

pub(crate) type Grid = (Handle, Vec<Column>, Vec<Vec<String>>);

pub(crate) fn graph_handle(document: &Document, definition: Handle) -> Option<Handle> {
    let xdict = super::block_authoring::definition_dictionary(document, definition)?;
    let ObjectType::Dictionary(dictionary) = document.objects.get(&xdict)? else {
        return None;
    };
    dictionary.get("ACAD_ENHANCEDBLOCK")
}

pub(crate) fn table(
    document: &Document,
    definition: Handle,
) -> Result<Option<TableView>, &'static str> {
    let graph = graph_handle(document, definition).ok_or("The authoring graph is unavailable.")?;
    let mut found = document.objects.values().filter_map(|object| match object {
        ObjectType::DynamicBlock(object) if object.owner == graph => match &object.data {
            DynamicBlockData::PropertiesTable(table) => Some((object.handle, table.clone())),
            _ => None,
        },
        _ => None,
    });
    let Some((handle, table)) = found.next() else {
        return Ok(None);
    };
    if found.next().is_some() {
        return Err("A block can contain only one Block Properties Table.");
    }
    let mut view = parse(handle, table)?;
    for column in &mut view.columns {
        column.node_id = super::block_actions::all_parameters(document, definition)
            .iter()
            .find(|parameter| parameter.handle == column.handle)
            .map(|parameter| parameter.id)
            .unwrap_or(0);
        if column.node_id != 0 {
            column.type_code = i64::from(column.node_id);
        }
        column.display_name = super::block_lookup::properties(document, definition)
            .into_iter()
            .find(|property| {
                property.column.node_id == column.node_id
                    && property
                        .column
                        .connection_name
                        .eq_ignore_ascii_case(&column.connection_name)
            })
            .map(|property| property.label)
            .unwrap_or_else(|| column.connection_name.clone());
    }
    Ok(Some(view))
}

fn integer(field: &BlockPropertiesTableField) -> Option<i64> {
    match field.value {
        BlockPropertiesTableValue::Integer(value) => Some(value),
        _ => None,
    }
}

fn real(field: &BlockPropertiesTableField) -> Option<f64> {
    match field.value {
        BlockPropertiesTableValue::Real(value) => Some(value),
        _ => None,
    }
}

fn handle(field: &BlockPropertiesTableField) -> Option<Handle> {
    match field.value {
        BlockPropertiesTableValue::Handle(value) => Some(value),
        _ => None,
    }
}

fn parse(table_handle: Handle, table: BlockPropertiesTable) -> Result<TableView, &'static str> {
    let fields = &table.fields;
    let column_count_index = fields
        .iter()
        .position(|field| field.code == 91)
        .ok_or("The Block Properties Table has no property columns.")?;
    let column_count = usize::try_from(
        integer(&fields[column_count_index])
            .ok_or("The Block Properties Table column count is invalid.")?,
    )
    .map_err(|_| "The Block Properties Table column count is invalid.")?;
    if column_count == 0 || column_count > 64 {
        return Err("The Block Properties Table column count is unsupported.");
    }
    let mut columns = Vec::with_capacity(column_count);
    let mut column_metadata = Vec::with_capacity(column_count);
    let mut index = column_count_index + 1;
    for column in 0..column_count {
        let leading_handle = fields.get(index).filter(|f| f.code == 340).and_then(handle);
        if leading_handle.is_some() {
            index += 1;
        }
        if column == 0 && leading_handle.is_none() {
            return Err("A Block Properties Table property binding is invalid.");
        }
        let end = fields
            .iter()
            .enumerate()
            .skip(index)
            .take_while(|(_, field)| field.code != 92)
            .find(|(_, field)| field.code == 302)
            .map(|(i, _)| i + 1)
            .ok_or("A Block Properties Table property record is incomplete.")?;
        let metadata = &fields[index..end];
        let mut name = None;
        for field in metadata {
            if field.code == 301 && name.is_none() {
                if let BlockPropertiesTableValue::Text(value) = &field.value {
                    name = Some(value.clone());
                }
            } else if field.code == 90 && name.is_some() {
                break;
            }
        }
        let name = name.ok_or("A Block Properties Table property name is missing.")?;
        let property_id = metadata
            .iter()
            .find(|field| field.code == 90)
            .and_then(integer)
            .ok_or("A Block Properties Table property type is missing.")?;
        column_metadata.push(metadata.to_vec());
        index = end;
        let column_handle = if let Some(binding) = leading_handle {
            // Earlier authored tables included a null separator after each column.
            if fields.get(index).filter(|f| f.code == 340).and_then(handle) == Some(Handle::NULL) {
                index += 1;
            }
            binding
        } else {
            let binding = fields
                .get(index)
                .filter(|f| f.code == 340)
                .and_then(handle)
                .ok_or("A Block Properties Table property binding is invalid.")?;
            index += 1;
            binding
        };
        columns.push(Column {
            handle: column_handle,
            node_id: 0,
            connection_name: name.clone(),
            display_name: name,
            type_code: property_id,
        });
    }
    let row_count_index = fields
        .iter()
        .enumerate()
        .skip(index)
        .find(|(_, field)| field.code == 92)
        .map(|(i, _)| i)
        .ok_or("The Block Properties Table row data is unavailable.")?;
    let row_count = usize::try_from(
        integer(&fields[row_count_index])
            .ok_or("The Block Properties Table row count is invalid.")?,
    )
    .map_err(|_| "The Block Properties Table row count is invalid.")?;
    if row_count == 0 || row_count > 1000 {
        return Err("The Block Properties Table row count is unsupported.");
    }
    let row_start = row_count_index + 1;
    let mut rows = Vec::with_capacity(row_count);
    index = row_start;
    for expected_row in 0..row_count {
        let row_index = fields
            .get(index)
            .filter(|field| field.code == 90)
            .and_then(integer)
            .ok_or("A Block Properties Table row index is invalid.")?;
        if row_index != expected_row as i64 {
            return Err("The Block Properties Table rows are not ordered.");
        }
        index += 1;
        let mut row = Vec::with_capacity(column_count);
        for _ in 0..column_count {
            let value_type = fields
                .get(index)
                .filter(|field| field.code == 170)
                .and_then(integer)
                .ok_or("A Block Properties Table cell type is unsupported.")?;
            if value_type != 40 {
                return Err("Only numeric Block Properties Table cells are supported.");
            }
            let value = fields
                .get(index + 1)
                .filter(|field| field.code == 140)
                .and_then(real)
                .filter(|value| value.is_finite())
                .ok_or("A Block Properties Table cell value is invalid.")?;
            row.push(value);
            index += 2;
        }
        rows.push(row);
    }
    let trailer_start = fields
        .iter()
        .enumerate()
        .skip(index)
        .find(|(_, field)| field.code == 93)
        .map(|(i, _)| i)
        .ok_or("The Block Properties Table trailer is unavailable.")?;
    let must_match = fields
        .get(trailer_start + 1)
        .and_then(|f| match f.value {
            BlockPropertiesTableValue::Boolean(v) if f.code == 290 => Some(v),
            _ => None,
        })
        .unwrap_or(false);
    Ok(TableView {
        handle: table_handle,
        table,
        columns,
        rows,
        row_start,
        trailer_start,
        must_match,
        column_metadata,
    })
}

pub(crate) fn columns(document: &Document, definition: Handle) -> Vec<Column> {
    let parameters = super::block_actions::all_parameters(document, definition);
    super::block_lookup::properties(document, definition)
        .into_iter()
        .filter_map(|property| {
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.id == property.column.node_id)?;
            if !property
                .column
                .connection_name
                .eq_ignore_ascii_case("UpdatedDistance")
                && !property
                    .column
                    .connection_name
                    .eq_ignore_ascii_case("UpdatedAngle")
            {
                return None;
            }
            Some(Column {
                handle: parameter.handle,
                node_id: parameter.id,
                connection_name: property.column.connection_name,
                display_name: property.label,
                type_code: i64::from(parameter.id),
            })
        })
        .collect()
}

pub(crate) fn grid_table(
    document: &Document,
    definition: Handle,
) -> Result<Option<Grid>, &'static str> {
    let Some(view) = table(document, definition)? else {
        return Ok(None);
    };
    let rows = view
        .rows
        .iter()
        .map(|row| row.iter().map(ToString::to_string).collect())
        .collect();
    Ok(Some((view.handle, view.columns, rows)))
}

pub(crate) fn apply(
    document: &mut Document,
    definition: Handle,
    columns: &[Column],
    rows: &[Vec<f64>],
    must_match: bool,
) -> Result<Handle, &'static str> {
    if columns.is_empty() || columns.len() > 64 || rows.is_empty() || rows.len() > 1000 {
        return Err("Add at least one property and one complete row.");
    }
    if rows
        .iter()
        .any(|row| row.len() != columns.len() || row.iter().any(|value| !value.is_finite()))
    {
        return Err("Complete each numeric property cell with a finite value.");
    }
    let mut working = document.clone();
    graph_handle(&working, definition).ok_or("The authoring graph is unavailable.")?;
    let previous = table(&working, definition)?;
    let mut new_table_eval = None;
    let handle = if let Some(view) = &previous {
        view.handle
    } else {
        let (graph, slots) = authoring::prepare(&mut working, definition)
            .map_err(|_| "The block authoring graph cannot accept a Block Properties Table.")?;
        register_classes(
            &mut working,
            &[("BLOCKPROPERTIESTABLE", "AcDbBlockPropertiesTable", 1153)],
        );
        let handle = working.allocate_handle();
        let element = BlockElement {
            eval: expression(slots.eval, NO_VALUE, BlockEvalValue::None),
            name: "Block Table".into(),
            major: MAJOR,
            minor: MINOR,
            eed_1071: 0,
        };
        new_table_eval = Some(slots.eval);
        let parameter = BlockOnePointParameter {
            parameter: BlockParameter {
                element,
                show_properties: true,
                chain_actions: false,
            },
            property_count: slots.eval + 1,
            ..Default::default()
        };
        let table = BlockPropertiesTable {
            parameter,
            fields: vec![],
        };
        working.objects.insert(
            handle,
            ObjectType::DynamicBlock(dynamic(
                handle,
                graph,
                "BLOCKPROPERTIESTABLE",
                "AcDbBlockPropertiesTable",
                DynamicBlockData::PropertiesTable(table),
            )),
        );
        authoring::append_single(&mut working, graph, slots, handle);
        handle
    };
    let old_fields = previous.as_ref().map(|view| view.table.fields.as_slice());
    let new_fields = encode_fields(columns, rows, old_fields, must_match)?;
    let Some(ObjectType::DynamicBlock(object)) = working.objects.get_mut(&handle) else {
        return Err("The Block Properties Table object is unavailable.");
    };
    let DynamicBlockData::PropertiesTable(table) = &mut object.data else {
        return Err("The selected object is not a Block Properties Table.");
    };
    table.fields = new_fields;
    if let Some(eval) = new_table_eval {
        table.parameter.property_count = eval + 1;
    }
    let graph = graph_handle(&working, definition).ok_or("The authoring graph is unavailable.")?;
    if new_table_eval.is_some() || previous.as_ref().is_none_or(|view| view.table.parameter.property_count > 0) {
        ensure_grip(&mut working, definition, graph, handle)?;
    }
    let _ = graph;
    *document = working;
    Ok(handle)
}

fn ensure_grip(
    document: &mut Document,
    definition: Handle,
    graph: Handle,
    table_handle: Handle,
) -> Result<(), &'static str> {
    let already = document.objects.values().any(|value| matches!(value, ObjectType::DynamicBlock(object) if object.owner == graph && matches!(&object.data, DynamicBlockData::PropertiesTableGrip(_))));
    let Some(ObjectType::DynamicBlock(table_obj)) = document.objects.get(&table_handle) else {
        return Err("The Block Properties Table object is unavailable.");
    };
    let DynamicBlockData::PropertiesTable(table_data) = &table_obj.data else {
        return Err("The selected object is not a Block Properties Table.");
    };
    let location = table_data.parameter.definition_point;
    let table_eval = table_data.parameter.parameter.element.eval.node_id;
    if already {
        let grip_eval = document.objects.values().find_map(|value| match value {
            ObjectType::DynamicBlock(object) if object.owner == graph => match &object.data {
                DynamicBlockData::PropertiesTableGrip(grip) => Some(grip.element.eval.node_id),
                _ => None,
            },
            _ => None,
        });
        if let Some(grip_eval) = grip_eval {
            if let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&table_handle)
            {
                if let DynamicBlockData::PropertiesTable(table) = &mut object.data {
                    table.parameter.property_count = grip_eval;
                }
            }
        }
        repair_grip_graph(document, definition, table_handle)?;
        return Ok(());
    }
    let (graph, slots) = authoring::prepare(document, definition)
        .map_err(|_| "The block authoring graph cannot accept a table grip.")?;
    authoring::register_classes(
        document,
        &[
            ("BLOCKPROPERTIESTABLEGRIP", "AcDbBlockGrip", 55),
            ("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr", 20),
        ],
    );
    let grip = document.allocate_handle();
    let x = document.allocate_handle();
    let y = document.allocate_handle();
    let grip_data = BlockGrip {
        element: BlockElement {
            eval: expression(slots.eval, NO_VALUE, BlockEvalValue::None),
            name: "Grip".into(),
            major: MAJOR,
            minor: MINOR,
            eed_1071: 0,
        },
        flags_91: slots.eval + 1,
        flags_92: slots.eval + 2,
        location,
        insert_cycling: false,
        insert_cycling_weight: -1,
    };
    document.objects.insert(
        grip,
        ObjectType::DynamicBlock(dynamic(
            grip,
            graph,
            "BLOCKPROPERTIESTABLEGRIP",
            "AcDbBlockGrip",
            DynamicBlockData::PropertiesTableGrip(grip_data),
        )),
    );
    for (handle, eval, axis) in [
        (x, slots.eval + 1, "UpdatedX"),
        (y, slots.eval + 2, "UpdatedY"),
    ] {
        document.objects.insert(
            handle,
            ObjectType::DynamicBlock(dynamic(
                handle,
                graph,
                "BLOCKGRIPLOCATIONCOMPONENT",
                "AcDbBlockGripExpr",
                DynamicBlockData::GripLocationComponent(BlockGripExpression {
                    eval: expression(eval, 40, BlockEvalValue::Real(0.0)),
                    grip_type: table_eval,
                    expression: axis.into(),
                }),
            )),
        );
    }
    for (index, handle) in [grip, x, y].into_iter().enumerate() {
        authoring::append_single(document, graph, authoring::Slots { node: slots.node + index as i32, eval: slots.eval + index as i32, edge: slots.edge }, handle);
    }
    let ObjectType::DynamicBlock(object) = document.objects.get_mut(&graph).unwrap() else { unreachable!() };
    let DynamicBlockData::EvaluationGraph(native) = &mut object.data else { unreachable!() };
    let parameter_node = native.nodes.iter().find(|node| node.next_id == table_eval).ok_or("The table parameter node is unavailable.")?.id;
    authoring::graph::link(native, slots.node, parameter_node)?;
    authoring::graph::link(native, parameter_node, slots.node + 1)?;
    authoring::graph::link(native, parameter_node, slots.node + 2)?;
    if let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&table_handle) {
        if let DynamicBlockData::PropertiesTable(table) = &mut object.data {
            table.parameter.property_count = slots.eval;
        }
    }
    Ok(())
}

/// Repair the former three-object append layout, preserving all unrelated nodes.
pub(crate) fn repair_grip_graph(doc: &mut Document, definition: Handle, table: Handle) -> Result<bool, &'static str> {
    let graph_handle = authoring::graph(doc, definition)?.ok_or("The authoring graph is unavailable.")?;
    let Some(ObjectType::DynamicBlock(table_object)) = doc.objects.get(&table) else { return Ok(false) };
    let DynamicBlockData::PropertiesTable(table_data) = &table_object.data else { return Ok(false) };
    let id = table_data.parameter.property_count;
    let grip = doc.objects.values().find_map(|object| match object {
        ObjectType::DynamicBlock(object) if object.owner == graph_handle => match &object.data {
            DynamicBlockData::PropertiesTableGrip(grip) if grip.element.eval.node_id == id => Some((object.handle, grip.flags_91, grip.flags_92)),
            _ => None,
        },
        _ => None,
    });
    let Some((grip_handle, x_id, y_id)) = grip else { return Ok(false) };
    let ObjectType::DynamicBlock(object) = &doc.objects[&graph_handle] else { unreachable!() };
    let DynamicBlockData::EvaluationGraph(original) = &object.data else { unreachable!() };
    if original.nodes.iter().all(|node| authoring::graph::valid_lists(original, node.id)) { return Ok(false) }
    let node_for = |id| original.nodes.iter().find(|node| node.next_id == id).map(|node| node.id);
    let (Some(g), Some(x), Some(y), Some(p)) = (
        original.nodes.iter().find(|node| node.expression == grip_handle).map(|node| node.id),
        node_for(x_id), node_for(y_id), original.nodes.iter().find(|node| node.expression == table).map(|node| node.id),
    ) else { return Ok(false) };
    let legacy = [g, x, y];
    if legacy.iter().collect::<std::collections::BTreeSet<_>>().len() != 3 || legacy.contains(&p) { return Ok(false) }
    let edges = original.edges.iter().filter(|edge| legacy.contains(&edge.source_node) || legacy.contains(&edge.destination_node)).collect::<Vec<_>>();
    let actual = edges.iter().map(|edge| (edge.source_node, edge.destination_node)).collect::<std::collections::BTreeSet<_>>();
    let expected = [(x, g), (g, x), (g, y)].into_iter().collect::<std::collections::BTreeSet<_>>();
    if edges.len() != 3 || actual != expected || edges.iter().any(|edge| edge.next_id != 0 || edge.outgoing_edges[4] != -1)
        || original.nodes.iter().filter(|node| !legacy.contains(&node.id)).any(|node| !authoring::graph::valid_lists(original, node.id)) {
        return Ok(false);
    }
    let removed = edges.iter().map(|edge| edge.id).collect::<Vec<_>>();
    let mut graph = original.clone();
    graph.edges.retain(|edge| !removed.contains(&edge.id));
    for node in &mut graph.nodes { if legacy.contains(&node.id) { node.node_data = [-1; 4]; } }
    authoring::graph::link(&mut graph, g, p)?;
    authoring::graph::link(&mut graph, p, x)?;
    authoring::graph::link(&mut graph, p, y)?;
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&graph_handle).unwrap() else { unreachable!() };
    object.data = DynamicBlockData::EvaluationGraph(graph);
    Ok(true)
}

fn encode_fields(
    columns: &[Column],
    rows: &[Vec<f64>],
    old: Option<&[BlockPropertiesTableField]>,
    must_match: bool,
) -> Result<Vec<BlockPropertiesTableField>, &'static str> {
    let previous = old
        .map(|fields| {
            parse(
                Handle::NULL,
                BlockPropertiesTable {
                    parameter: Default::default(),
                    fields: fields.to_vec(),
                },
            )
        })
        .transpose()?;
    let mut fields = if let Some(previous) = &previous {
        let count = previous
            .table
            .fields
            .iter()
            .position(|field| field.code == 91)
            .ok_or("The Block Properties Table has no property columns.")?;
        previous.table.fields[..count].to_vec()
    } else {
        vec![int(90, 2), text(300, "Block Table1"), text(301, "")]
    };
    fields.push(int(91, columns.len() as i64));
    for column in columns {
        let mut metadata = previous
            .as_ref()
            .and_then(|previous| {
                previous
                    .columns
                    .iter()
                    .position(|old| {
                        old.handle == column.handle && old.connection_name == column.connection_name
                    })
                    .map(|i| previous.column_metadata[i].clone())
            })
            .unwrap_or_else(|| {
                vec![
                    int(170, 0),
                    int(171, -1),
                    text(300, ""),
                    text(301, &column.connection_name),
                    int(90, column.node_id as i64),
                    int(170, -9999),
                    int(170, -9999),
                    boolean(290, false),
                    boolean(291, true),
                    boolean(292, true),
                    boolean(
                        293,
                        column.connection_name.eq_ignore_ascii_case("UpdatedAngle"),
                    ),
                    boolean(294, false),
                    text(302, ""),
                ]
            });
        if let Some(field) = metadata.iter_mut().find(|field| field.code == 90) {
            field.value = BlockPropertiesTableValue::Integer(i64::from(column.node_id));
        }
        if let Some(field) = metadata.iter_mut().find(|field| field.code == 293) {
            field.value = BlockPropertiesTableValue::Boolean(
                column.connection_name.eq_ignore_ascii_case("UpdatedAngle"),
            );
        }
        // Every column stores its binding before its metadata and a null
        // handle after it.
        fields.push(handle_field(340, column.handle));
        fields.extend(metadata);
        fields.push(handle_field(340, Handle::NULL));
    }
    fields.push(int(92, rows.len() as i64));
    for (index, row) in rows.iter().enumerate() {
        fields.push(int(90, index as i64));
        for value in row {
            fields.push(int(170, 40));
            fields.push(real_field(140, *value));
        }
    }
    // Group 93 is the next row ID, followed by exactly three flags (must
    // match, runtime parameters only, disabled in the drawing editor).
    let mut trailer = vec![
        boolean(290, must_match),
        boolean(291, true),
        boolean(292, false),
    ];
    if let Some(previous) = previous.as_ref() {
        for (flag, old) in trailer
            .iter_mut()
            .zip(&previous.table.fields[previous.trailer_start + 1..])
            .skip(1)
        {
            if let BlockPropertiesTableValue::Boolean(value) = old.value {
                flag.value = BlockPropertiesTableValue::Boolean(value);
            }
        }
    }
    fields.push(int(93, rows.len() as i64));
    fields.extend(trailer);
    Ok(fields)
}
fn int(code: i32, value: i64) -> BlockPropertiesTableField {
    BlockPropertiesTableField {
        code,
        value: BlockPropertiesTableValue::Integer(value),
    }
}

fn real_field(code: i32, value: f64) -> BlockPropertiesTableField {
    BlockPropertiesTableField {
        code,
        value: BlockPropertiesTableValue::Real(value),
    }
}

fn text(code: i32, value: &str) -> BlockPropertiesTableField {
    BlockPropertiesTableField {
        code,
        value: BlockPropertiesTableValue::Text(value.into()),
    }
}

fn boolean(code: i32, value: bool) -> BlockPropertiesTableField {
    BlockPropertiesTableField {
        code,
        value: BlockPropertiesTableValue::Boolean(value),
    }
}

fn handle_field(code: i32, value: Handle) -> BlockPropertiesTableField {
    BlockPropertiesTableField {
        code,
        value: BlockPropertiesTableValue::Handle(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{block_distance_parameter as distance, Scene};
    use codec::{types::Vector3, DwgReader, DwgWriter, DxfReader, DxfWriter};
    use std::io::Cursor;

    fn fixture() -> (Document, Handle, Vec<Column>) {
        let mut scene = Scene::new();
        scene.define_block_raw("TABLE", Vector3::ZERO, vec![]);
        let definition = scene.document.block_records.get("TABLE").unwrap().handle;
        for kind in [distance::Kind::Linear, distance::Kind::Polar] {
            distance::create(
                &mut scene.document,
                definition,
                kind,
                Vector3::ZERO,
                Vector3::new(10., 0., 0.),
                Vector3::new(5., 2., 0.),
            )
            .unwrap();
        }
        let available = columns(&scene.document, definition);
        (scene.document, definition, available)
    }

    #[test]
    fn property_table_grip_graph_is_valid_and_repairs_only_the_legacy_layout() {
        let (mut doc, definition, columns) = fixture();
        let rows = vec![vec![10.; columns.len()]];
        let table_handle = apply(&mut doc, definition, &columns, &rows, false).unwrap();
        let graph_handle = authoring::graph(&doc, definition).unwrap().unwrap();
        let ObjectType::DynamicBlock(table_object) = &doc.objects[&table_handle] else { panic!() };
        let DynamicBlockData::PropertiesTable(native) = &table_object.data else { panic!() };
        let grip_id = native.parameter.property_count;
        let grip = doc.objects.values().find_map(|object| match object {
            ObjectType::DynamicBlock(object) => match &object.data {
                DynamicBlockData::PropertiesTableGrip(grip) if grip.element.eval.node_id == grip_id => Some((object.handle, grip.flags_91, grip.flags_92)),
                _ => None,
            },
            _ => None,
        }).unwrap();
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&graph_handle).unwrap() else { panic!() };
        let DynamicBlockData::EvaluationGraph(graph) = &mut object.data else { panic!() };
        assert!(graph.nodes.iter().all(|node| authoring::graph::valid_lists(graph, node.id)));
        let p = graph.nodes.iter().find(|node| node.expression == table_handle).unwrap().id;
        let g = graph.nodes.iter().find(|node| node.expression == grip.0).unwrap().id;
        let x = graph.nodes.iter().find(|node| node.next_id == grip.1).unwrap().id;
        let y = graph.nodes.iter().find(|node| node.next_id == grip.2).unwrap().id;
        let edge_ids = graph.edges.iter().filter(|edge| [g, x, y].contains(&edge.source_node) || [g, x, y].contains(&edge.destination_node)).map(|edge| edge.id).collect::<Vec<_>>();
        assert_eq!(edge_ids.len(), 3);
        let [e0, e1, e2] = <[i32;3]>::try_from(edge_ids).unwrap();
        for node in &mut graph.nodes {
            if node.id == p { node.node_data = [-1; 4]; }
            if node.id == g { node.node_data = [e0, e0, e1, e2]; }
            if node.id == x { node.node_data = [e1, e1, -1, -1]; }
            if node.id == y { node.node_data = [e2, e2, -1, -1]; }
        }
        for edge in &mut graph.edges {
            if edge.id == e0 { edge.source_node = x; edge.destination_node = g; edge.outgoing_edges = [-1; 5]; }
            if edge.id == e1 { edge.source_node = g; edge.destination_node = x; edge.outgoing_edges = [-1, -1, -1, e2, -1]; }
            if edge.id == e2 { edge.source_node = g; edge.destination_node = y; edge.outgoing_edges = [-1, -1, e1, -1, -1]; }
        }
        let legacy = doc.clone();
        for mut saved in [
            legacy.clone(),
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&legacy).unwrap())).read().unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&legacy).write_to_vec().unwrap())).unwrap().read().unwrap(),
        ] {
            let before = saved.clone();
            assert!(repair_grip_graph(&mut saved, definition, table_handle).unwrap());
            assert_eq!(saved.objects[&table_handle], before.objects[&table_handle]);
            assert_eq!(saved.objects[&grip.0], before.objects[&grip.0]);
            let ObjectType::DynamicBlock(object) = &saved.objects[&graph_handle] else { panic!() };
            let DynamicBlockData::EvaluationGraph(graph) = &object.data else { panic!() };
            assert!(graph.nodes.iter().all(|node| authoring::graph::valid_lists(graph, node.id)));
            assert!(!repair_grip_graph(&mut saved, definition, table_handle).unwrap());
            assert_eq!(table(&saved, definition).unwrap().unwrap().rows, rows);
        }
        let mut unknown = legacy;
        let ObjectType::DynamicBlock(object) = unknown.objects.get_mut(&graph_handle).unwrap() else { panic!() };
        let DynamicBlockData::EvaluationGraph(graph) = &mut object.data else { panic!() };
        graph.edges.iter_mut().find(|edge| edge.id == e0).unwrap().next_id = 9;
        let before = unknown.clone();
        assert!(!repair_grip_graph(&mut unknown, definition, table_handle).unwrap());
        assert_eq!(unknown, before);
    }

    #[test]
    fn native_property_table_columns_and_trailers_roundtrip_independent_of_row_count() {
        for row_count in [1, 2, 4] {
            let (mut doc, definition, columns) = fixture();
            let rows = (0..row_count)
                .map(|row| {
                    columns
                        .iter()
                        .enumerate()
                        .map(|(col, _)| (row + col + 1) as f64)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            apply(&mut doc, definition, &columns, &rows, true).unwrap();
            for saved in [
                DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                    .read()
                    .unwrap(),
                DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                    .unwrap()
                    .read()
                    .unwrap(),
            ] {
                let view = table(&saved, definition).unwrap().unwrap();
                assert_eq!(view.rows, rows);
                assert_eq!(view.columns, columns);
                assert!(view.must_match);
                assert_eq!(view.column_metadata.len(), columns.len());
            }
        }
    }

    #[test]
    fn existing_property_columns_can_be_reordered_added_or_removed_without_losing_metadata() {
        let (mut doc, definition, columns) = fixture();
        let handle = apply(
            &mut doc,
            definition,
            &columns[..2],
            &[vec![10., 20.]],
            false,
        )
        .unwrap();
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&handle).unwrap() else {
            panic!()
        };
        let DynamicBlockData::PropertiesTable(data) = &mut object.data else {
            panic!()
        };
        let label = data
            .fields
            .iter_mut()
            .skip(4)
            .find(|field| field.code == 300)
            .unwrap();
        label.value = BlockPropertiesTableValue::Text("retained column description".into());
        let reordered = vec![columns[1].clone(), columns[2].clone(), columns[0].clone()];
        apply(
            &mut doc,
            definition,
            &reordered,
            &[vec![20., 0.5, 10.]],
            true,
        )
        .unwrap();
        let view = table(&doc, definition).unwrap().unwrap();
        assert_eq!(view.columns, reordered);
        assert!(view.column_metadata[2].iter().any(|field| field.value
            == BlockPropertiesTableValue::Text("retained column description".into())));
        apply(
            &mut doc,
            definition,
            &reordered[1..],
            &[vec![0.5, 10.]],
            false,
        )
        .unwrap();
        assert_eq!(
            table(&doc, definition).unwrap().unwrap().columns,
            reordered[1..]
        );
    }

    #[test]
    fn earlier_authored_column_separators_are_read_and_migrated_to_native_records() {
        let (mut doc, definition, columns) = fixture();
        let handle = apply(
            &mut doc,
            definition,
            &columns,
            &[vec![10., 20., 0.5]],
            false,
        )
        .unwrap();
        let view = table(&doc, definition).unwrap().unwrap();
        let mut fields = view.table.fields[..4].to_vec();
        for (column, metadata) in columns.iter().zip(&view.column_metadata) {
            fields.push(handle_field(340, column.handle));
            fields.extend(metadata.clone());
            fields.push(handle_field(340, Handle::NULL));
        }
        fields.extend_from_slice(&view.table.fields[view.row_start - 1..]);
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&handle).unwrap() else {
            panic!()
        };
        let DynamicBlockData::PropertiesTable(data) = &mut object.data else {
            panic!()
        };
        data.fields = fields;
        assert_eq!(table(&doc, definition).unwrap().unwrap().columns, columns);
        apply(&mut doc, definition, &columns, &[vec![12., 24., 1.]], true).unwrap();
        let saved = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap();
        assert_eq!(
            table(&saved, definition).unwrap().unwrap().rows,
            vec![vec![12., 24., 1.]]
        );
    }
}
