//! A copied table owns its own cell fields.
//!
//! A table cell names its FIELD object by handle (`CellContent::field_handle`)
//! and the table lists them (`Table::field_handles`). A copy that keeps those
//! handles shares the source's fields: an edit of one table's field changes
//! the other, and in another drawing the handles point at nothing. COPY and
//! paste give the copy fresh clones of the fields instead.

use document::entities::Table;
use document::objects::{Field, ObjectType};
use document::{Document, EntityType, Handle};

/// The fields a table's cells and field list name.
fn field_roots(table: &Table) -> impl Iterator<Item = Handle> + '_ {
    table
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .flat_map(|cell| &cell.contents)
        .filter_map(|content| content.field_handle)
        .chain(table.field_handles.iter().copied())
}

/// The FIELD objects a table uses, nested fields included, each with its
/// own handle filled in.
pub(crate) fn table_fields(doc: &Document, table: &Table) -> Vec<Field> {
    let mut out = Vec::new();
    let mut seen = rustc_hash::FxHashSet::default();
    let mut stack: Vec<Handle> = field_roots(table).collect();
    while let Some(handle) = stack.pop() {
        if handle.is_null() || !seen.insert(handle) {
            continue;
        }
        if let Some(ObjectType::Field(field)) = doc.objects.get(&handle) {
            stack.extend(field.child_fields.iter().copied());
            let mut field = field.clone();
            field.handle = handle;
            out.push(field);
        }
    }
    out
}

/// Give the table `copy` (a copy of the table `source`) its own clones of the
/// fields it names, taken from `fields` (the source's, by source handle), and
/// point its cells and field list at them. A field `fields` does not hold is
/// let go: the cell keeps the text it shows. Returns the new FIELD handles.
pub(crate) fn adopt_table_fields(
    doc: &mut Document,
    copy: Handle,
    source: Handle,
    fields: &[Field],
) -> Vec<Handle> {
    let Some(EntityType::Table(table)) = doc.get_entity(copy) else {
        return Vec::new();
    };
    let by_handle: rustc_hash::FxHashMap<Handle, &Field> =
        fields.iter().map(|field| (field.handle, field)).collect();
    // The fields this table reaches, nested ones included.
    let mut order = Vec::new();
    let mut stack: Vec<Handle> = field_roots(table).collect();
    let mut seen = rustc_hash::FxHashSet::default();
    while let Some(handle) = stack.pop() {
        if !seen.insert(handle) {
            continue;
        }
        if let Some(field) = by_handle.get(&handle) {
            stack.extend(field.child_fields.iter().copied());
            order.push(handle);
        }
    }
    let remap: rustc_hash::FxHashMap<Handle, Handle> = order
        .iter()
        .map(|&handle| (handle, doc.allocate_handle()))
        .collect();
    let rewrite = |handle: Handle| {
        if handle == source {
            copy
        } else {
            remap.get(&handle).copied().unwrap_or(handle)
        }
    };
    let mut created = Vec::with_capacity(order.len());
    for handle in &order {
        let mut field = by_handle[handle].clone();
        field.handle = remap[handle];
        field.owner = rewrite(field.owner);
        field.child_fields = field
            .child_fields
            .iter()
            .filter_map(|child| remap.get(child).copied())
            .collect();
        for reference in &mut field.referenced_objects {
            *reference = rewrite(*reference);
        }
        created.push(field.handle);
        doc.objects.insert(field.handle, ObjectType::Field(field));
    }
    if let Some(EntityType::Table(table)) = doc.get_entity_mut(copy) {
        for content in table
            .rows
            .iter_mut()
            .flat_map(|row| &mut row.cells)
            .flat_map(|cell| &mut cell.contents)
        {
            if let Some(field) = content.field_handle {
                content.field_handle = remap.get(&field).copied();
            }
        }
        let listed = std::mem::take(&mut table.field_handles);
        table.field_handles = listed
            .into_iter()
            .filter_map(|field| remap.get(&field).copied())
            .collect();
    }
    created
}
