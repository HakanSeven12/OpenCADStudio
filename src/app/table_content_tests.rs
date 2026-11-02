//! A table can keep a TABLECONTENT copy of its cells; an edit must reach that
//! copy, or a reader that rebuilds the table from it shows the old one.

use super::{Message, OpenCADStudio};
use crate::scene::table_content::table_content_handle;
use codec::entities::Table;
use codec::objects::{Dictionary, ObjectType, XRecord, XRecordEntry};
use codec::types::Vector3;
use codec::{Document, EntityType, Handle};

fn table(doc: &Document, handle: Handle) -> &Table {
    let Some(EntityType::Table(table)) = doc.get_entity(handle) else {
        panic!("TABLE")
    };
    table
}

fn content(doc: &Document, table: Handle) -> &Table {
    let handle = table_content_handle(doc, table).expect("TABLECONTENT link");
    let Some(ObjectType::TableContent(content)) = doc.objects.get(&handle) else {
        panic!("TABLECONTENT")
    };
    content
}

fn widths(table: &Table) -> Vec<f64> {
    table.columns.iter().map(|column| column.width).collect()
}

/// A 3x4 table with columns 20 wide and the TABLECONTENT chain DWG files
/// carry: TABLE -> extension dictionary -> roundtrip XRECORD -360->
/// TABLECONTENT.
fn fixture() -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut source = Table::new(Vector3::ZERO, 3, 4);
    for column in &mut source.columns {
        column.width = 20.0;
    }
    for row in &mut source.rows {
        row.height = 10.0;
    }
    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::Table(Box::new(source)));
    let doc = &mut app.tabs[i].scene.document;
    let xdict = doc.allocate_handle();
    let record = doc.allocate_handle();
    let content_handle = doc.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = xdict;
    dictionary.owner = handle;
    dictionary.add_entry("ACAD_XREC_ROUNDTRIP", record);
    let mut roundtrip = XRecord::new();
    roundtrip.handle = record;
    roundtrip.owner = xdict;
    roundtrip.reactors.push(xdict);
    roundtrip.entries.push(XRecordEntry::string(
        102,
        "ACAD_ROUNDTRIP_2008_TABLE_ENTITY",
    ));
    roundtrip
        .entries
        .push(XRecordEntry::handle(360, content_handle));
    roundtrip.synchronize_object_references();
    let mut copy = table(doc, handle).clone();
    copy.common = Default::default();
    copy.common.handle = content_handle;
    copy.common.owner_handle = record;
    doc.objects
        .insert(xdict, ObjectType::Dictionary(dictionary));
    doc.objects.insert(record, ObjectType::XRecord(roundtrip));
    doc.objects
        .insert(content_handle, ObjectType::TableContent(copy));
    if let Some(entity) = doc.get_entity_mut(handle) {
        entity.common_mut().xdictionary_handle = Some(xdict);
    }
    assert_eq!(widths(content(doc, handle)), vec![20.0; 4]);
    (app, handle)
}

/// Widen every column to 30 and change one cell, the way a cell or grip edit
/// does it: drop the picture so the scene redraws the table.
fn edit(app: &mut OpenCADStudio, handle: Handle) {
    edit_with(app, handle, false);
}

/// `delta`: record the edit as an entity delta (`begin_undo`), which restores
/// only the objects put on record, instead of a snapshot.
fn edit_with(app: &mut OpenCADStudio, handle: Handle, delta: bool) {
    let i = app.active_tab;
    let pending = if delta {
        app.begin_undo(i, "TABLEEDIT", 1, true)
    } else {
        app.push_undo_snapshot(i, "TABLEEDIT");
        None
    };
    let mut edited = table(&app.tabs[i].scene.document, handle).clone();
    for column in &mut edited.columns {
        column.width = 30.0;
    }
    edited.set_cell_text(1, 2, "NEW");
    edited.block_record_handle = None;
    assert!(app.tabs[i]
        .scene
        .update_entity(EntityType::Table(Box::new(edited))));
    match pending {
        Some(pending) => app.commit_undo_delta(i, pending),
        None => app.finish_pending_history(i),
    }
}

fn assert_content_matches(doc: &Document, handle: Handle) {
    let entity = table(doc, handle);
    let mut copy = content(doc, handle).clone();
    assert_ne!(
        copy.common.handle, handle,
        "the object keeps its own handle"
    );
    copy.common = entity.common.clone();
    assert_eq!(widths(&copy), widths(entity), "stale TABLECONTENT widths");
    assert!(copy == *entity, "TABLECONTENT differs from the table");
}

#[test]
fn table_content_follows_table_edit() {
    let (mut app, handle) = fixture();
    edit(&mut app, handle);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(widths(table(doc, handle)), vec![30.0; 4]);
    assert_content_matches(doc, handle);
}

#[test]
fn table_content_edit_survives_dwg() {
    let (mut app, handle) = fixture();
    edit(&mut app, handle);
    let bytes = codec::DwgWriter::write_to_vec(&app.tabs[app.active_tab].scene.document)
        .expect("DWG write");
    let reopened = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .expect("DWG read");
    assert_eq!(
        widths(content(&reopened, handle)),
        vec![30.0; 4],
        "stale TABLECONTENT widths in DWG"
    );
}

#[test]
fn table_content_edit_undo_restores_old_content() {
    check_undo(false);
}

#[test]
fn table_content_delta_edit_undo_restores_old_content() {
    check_undo(true);
}

fn check_undo(delta: bool) {
    let (mut app, handle) = fixture();
    edit_with(&mut app, handle, delta);
    let _ = app.update(Message::Undo);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(widths(table(doc, handle)), vec![20.0; 4]);
    assert_eq!(widths(content(doc, handle)), vec![20.0; 4]);
    let _ = app.update(Message::Redo);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(widths(table(doc, handle)), vec![30.0; 4]);
    assert_content_matches(doc, handle);
}
