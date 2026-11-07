//! A table can keep a TABLECONTENT copy of its cells; an edit must reach that
//! copy, or a reader that rebuilds the table from it shows the old one.

use super::{Message, OpenCADStudio};
use crate::scene::table_content::table_content_handle;
use document::entities::Table;
use document::objects::{Dictionary, ObjectType, XRecord, XRecordEntry};
use document::types::Vector3;
use document::{Document, EntityType, Handle};

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
    let bytes = document::DwgWriter::write_to_vec(&app.tabs[app.active_tab].scene.document)
        .expect("DWG write");
    let reopened = document::DwgReader::from_stream(std::io::Cursor::new(bytes))
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

#[test]
fn table_scaling_command_grip_and_affine_rebuild_picture_and_content() {
    use crate::command::{EntityTransform, StepInput};
    use crate::scene::pick::grip::GripEdit;
    use crate::ui::popup::context_menu::GripMenuCmd;
    use glam::DVec3;

    for (path, factor) in (0..3).flat_map(|path| [0.01, 3.0].map(|factor| (path, factor))) {
        let (mut app, h) = fixture();
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(h, false);
        let before = table(&app.tabs[i].scene.document, h).clone();
        if path == 2 {
            app.push_undo_snapshot(i, "SCALE");
            app.tabs[i].scene.transform_entities(
                &[h],
                &EntityTransform::Affine(document::types::Transform::from_scaling_with_origin(
                    Vector3::new(factor, factor, factor),
                    Vector3::ZERO,
                )),
            );
        } else {
            if path == 0 {
                let _ = app.dispatch_command("SCALE");
                let _ = app.feed_command(StepInput::Point(DVec3::ZERO));
            } else {
                app.tabs[i].active_grip = Some(GripEdit::single(h, 0, false, DVec3::ZERO));
                let _ = app.on_grip_context_pick(GripMenuCmd::Scale);
            }
            let input = if path == 1 {
                StepInput::Point(DVec3::X * factor)
            } else {
                StepInput::Text(factor.to_string())
            };
            let _ = app.feed_command(input);
        }
        let doc = &app.tabs[i].scene.document;
        let scaled = table(doc, h);
        assert!((scaled.total_width() - 80.0 * factor).abs() < 1e-6);
        assert_eq!(widths(scaled), widths(content(doc, h)), "stale TABLECONTENT");
        assert_table_scaling_picture_width(doc, h, 80.0 * factor);
        if path == 0 {
            for ext in ["dwg", "dxf"] {
                let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
                let reopened = crate::io::load_bytes(&format!("scaled-table.{ext}"), bytes).unwrap();
                let saved = reopened
                    .entities()
                    .find_map(|entity| {
                        matches!(entity, EntityType::Table(_)).then_some(entity.common().handle)
                    })
                    .unwrap();
                assert!((table(&reopened, saved).total_width() - 80.0 * factor).abs() < 1e-6);
                assert_table_scaling_picture_width(&reopened, saved, 80.0 * factor);
            }
        }
        app.undo_steps(1);
        assert_eq!(widths(table(&app.tabs[i].scene.document, h)), widths(&before));
        assert_eq!(
            widths(content(&app.tabs[i].scene.document, h)),
            widths(&before)
        );
        assert_table_scaling_picture_width(&app.tabs[i].scene.document, h, 80.0);
        app.redo_steps(1);
        assert_eq!(
            widths(table(&app.tabs[i].scene.document, h)),
            vec![20.0 * factor; 4]
        );
        assert_table_scaling_picture_width(&app.tabs[i].scene.document, h, 80.0 * factor);
    }
}

#[test]
fn table_scaling_keeps_shared_style_dimensions_on_other_instances() {
    use crate::command::EntityTransform;
    use document::entities::table::CellStylePropertyFlags as P;
    let (mut app, h) = fixture();
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let style_handle = doc.allocate_handle();
    let mut shared = document::objects::TableStyle::new("Shared dimensions");
    shared.handle = style_handle;
    shared.set_all_text_heights(5.0);
    shared.horizontal_margin = 1.0;
    shared.vertical_margin = 1.0;
    doc.objects
        .insert(style_handle, ObjectType::TableStyle(shared.clone()));
    let Some(EntityType::Table(source)) = doc.get_entity_mut(h) else {
        panic!("table");
    };
    source.table_style_handle = Some(style_handle);
    source.set_cell_text(2, 0, "Scaled content");
    let other = app.tabs[i]
        .scene
        .copy_entities(&[h], &EntityTransform::Translate(glam::DVec3::X))[0];
    app.tabs[i].scene.transform_entities(
        &[h],
        &EntityTransform::Scale {
            center: glam::DVec3::ZERO,
            factor: 0.01,
        },
    );
    let doc = &app.tabs[i].scene.document;
    let scaled = table(doc, h);
    let style = crate::entities::table::style_for_property(
        scaled,
        &scaled.rows[2],
        0,
        &scaled.rows[2].cells[0],
        P::TEXT_HEIGHT,
    )
    .expect("instance text height");
    assert!((style.text_height - 0.05).abs() < 1e-9);
    assert!((style.margin_left - 0.025).abs() < 1e-9);
    assert_eq!(table(doc, other).total_width(), 80.0);
    let Some(ObjectType::TableStyle(unchanged)) = doc.objects.get(&style_handle) else {
        panic!("style");
    };
    assert_eq!(unchanged, &shared);
}

fn assert_table_scaling_picture_width(doc: &Document, handle: Handle, width: f64) {
    let table = table(doc, handle);
    let picture = table
        .block_record_handle
        .and_then(|handle| doc.block_records.iter().find(|r| r.handle == handle))
        .or_else(|| doc.block_records.get(&table.block_name))
        .expect("picture block");
    let max_x = picture
        .entity_handles
        .iter()
        .filter_map(|handle| match doc.get_entity(*handle) {
            Some(EntityType::Line(line)) => Some(line.start.x.max(line.end.x)),
            _ => None,
        })
        .fold(0.0_f64, f64::max);
    assert!(
        (max_x - width).abs() < 1e-5,
        "stale picture: {max_x} != {width}"
    );
}
