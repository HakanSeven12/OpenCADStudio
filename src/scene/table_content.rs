//! The TABLECONTENT object of a table.
//!
//! A table can keep a second copy of its content (columns, rows, cells) in a
//! TABLECONTENT object reached through the table's extension dictionary:
//! TABLE -> `ACAD_XREC_ROUNDTRIP` XRECORD -> group 360 -> TABLECONTENT. A
//! reader may rebuild the table from that copy when it opens the DWG, so the
//! copy must follow every edit of the table.

use codec::objects::ObjectType;
use codec::{Document, EntityType, Handle};

const ROUNDTRIP: &str = "ACAD_XREC_ROUNDTRIP";

/// The TABLECONTENT object kept for `table`, if the table has one.
pub(crate) fn table_content_handle(document: &Document, table: Handle) -> Option<Handle> {
    let record = document.xrecord(table, ROUNDTRIP)?;
    let content = record
        .entries
        .iter()
        .filter(|entry| entry.code == 360)
        .find_map(|entry| entry.value.as_handle())?;
    matches!(
        document.objects.get(&content),
        Some(ObjectType::TableContent(_))
    )
    .then_some(content)
}

/// Copy the content of table `table` into its TABLECONTENT object, keeping the
/// object's own handle, owner, reactors and dictionary. Returns the object's
/// handle and its old value when it changed, for the undo record.
pub(crate) fn sync_table_content_object(
    document: &mut Document,
    table: Handle,
) -> Option<(Handle, ObjectType)> {
    let content = table_content_handle(document, table)?;
    let Some(EntityType::Table(source)) = document.get_entity(table) else {
        return None;
    };
    let mut copy = (**source).clone();
    let Some(ObjectType::TableContent(target)) = document.objects.get_mut(&content) else {
        return None;
    };
    copy.common = target.common.clone();
    if copy == *target {
        return None;
    }
    let before = std::mem::replace(target, copy);
    Some((content, ObjectType::TableContent(before)))
}
