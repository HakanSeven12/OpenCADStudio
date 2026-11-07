//! Headless interaction coverage for the cell editor.
use super::{ArrowKey, Message, OpenCADStudio};
use document::entities::Table;
use document::types::Vector3;
use document::{EntityType, Handle};
use glam::DVec3;

pub(super) fn fixture(raw: &str) -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let mut table = Table::new(Vector3::ZERO, 3, 4);
    for col in &mut table.columns {
        col.width = 20.0;
    }
    for row in &mut table.rows {
        row.height = 10.0;
    }
    table.cell_mut(0, 0).unwrap().set_text(raw);
    table.cell_mut(0, 1).unwrap().set_text("neighbour");
    let i = app.active_tab;
    let handle = app.tabs[i].scene.add_entity(EntityType::Table(Box::new(table)));
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    (app, handle)
}

fn start(app: &mut OpenCADStudio, handle: Handle) {
    let _ = app.dispatch_command("TABLEDIT");
    let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(10.0, -5.0, 0.0)));
    assert_eq!(app.table_cell_editor.as_ref().map(|c| c.handle), Some(handle),
        "a visible editor must own the picked cell");
}

fn source(app: &OpenCADStudio, handle: Handle) -> String {
    let Some(EntityType::Table(table)) = app.tabs[app.active_tab].scene.document.get_entity(handle) else { panic!("table") };
    table.cell(0, 0).unwrap().contents[0].value.text.clone()
}

fn entities(app: &OpenCADStudio) -> Vec<EntityType> {
    app.tabs[app.active_tab].scene.document.entities().cloned().collect()
}

#[test]
fn opens_with_source_caret_style_and_highlight_without_document_mutation() {
    let (mut app, handle) = fixture("=SUM(A1:D1)");
    let before = entities(&app);
    let objects = app.tabs[app.active_tab].scene.document.objects.clone();
    app.tabs[app.active_tab].dirty = false;
    start(&mut app, handle);
    let ed = app.mtext_editor.as_ref().unwrap();
    assert_eq!(ed.content.text(), "=SUM(A1:D1)");
    assert_eq!(ed.caret, "=SUM(A1:D1)".chars().count());
    let cell = app.table_cell_editor.as_ref().unwrap();
    assert_eq!(ed.height_value(), cell.picture.text.height);
    assert_eq!(ed.style, cell.picture.text.style);
    assert_eq!(app.tabs[app.active_tab].scene.table_cells.unwrap().anchor, (0, 0));
    let canvas = app.table_cell_canvas().expect("projected cell");
    let center = iced::Point::new((canvas.corners[0].x + canvas.corners[2].x) * 0.5,
        (canvas.corners[0].y + canvas.corners[2].y) * 0.5);
    assert!(canvas.contains(center));
    let (x, y) = canvas.unproject(canvas.project(1.25, -0.75));
    assert!((x - 1.25).abs() < 1e-4 && (y + 0.75).abs() < 1e-4);
    let _ = app.update(Message::CommandAppendChar("x".into()));
    assert_eq!(entities(&app), before, "typing changed the document");
    assert_eq!(app.tabs[app.active_tab].scene.document.objects, objects);
    assert!(!app.tabs[app.active_tab].dirty);
    let _ = app.run_action("CANCEL");
    assert!(app.table_cell_editor.is_none());
    assert!(app.mtext_editor.is_none());
    assert_eq!(entities(&app), before);
    assert!(app.tabs[app.active_tab].scene.preview_wires.is_empty());
}

#[test]
fn formula_character_edit_outside_commit_and_drawing_undo() {
    let (mut app, handle) = fixture("=SUM(A1:D1)");
    start(&mut app, handle);
    let _ = app.update(Message::ArrowKeyPressed {
        direction: ArrowKey::Left, shortcut: "LEFT".into(), extend_selection: false,
    });
    let _ = app.update(Message::CommandBackspace);
    let _ = app.update(Message::CommandAppendChar("2".into()));
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "=SUM(A1:D2)");
    assert_eq!(source(&app, handle), "=SUM(A1:D1)");
    let _ = app.update(Message::TableEditorClick(iced::Point::new(-1000.0, -1000.0)));
    assert_eq!(source(&app, handle), "=SUM(A1:D2)");
    assert!(app.table_cell_editor.is_none());
    assert!(app.mtext_editor.is_none());
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    let _ = app.update(Message::Undo);
    assert_eq!(source(&app, handle), "=SUM(A1:D1)");
    let _ = app.dispatch_command("LINE");
    assert_eq!(app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(), "LINE");
}

#[test]
fn empty_cell_commit_and_escape_have_different_effects() {
    let (mut app, handle) = fixture("keep");
    start(&mut app, handle);
    app.mtext_select_all();
    let _ = app.update(Message::DeleteSelected);
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "");
    assert!(app.commit_table_cell_editor());
    assert_eq!(source(&app, handle), "");
    start(&mut app, handle);
    let _ = app.update(Message::CommandAppendChar("discard".into()));
    let _ = app.update(Message::MTextCancel);
    assert_eq!(source(&app, handle), "");
}

#[test]
fn tab_enter_and_arrows_move_the_editor_and_highlight() {
    let (mut app, handle) = fixture("first");
    start(&mut app, handle);
    let _ = app.update(Message::CommandAppendChar("!".into()));
    let _ = app.update(Message::DynTabNext);
    assert_eq!(source(&app, handle), "first!");
    let cell = app.table_cell_editor.as_ref().unwrap();
    assert_eq!((cell.row, cell.column), (0, 1));
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "neighbour");
    let _ = app.update(Message::CommandFinalize);
    let cell = app.table_cell_editor.as_ref().unwrap();
    assert_eq!((cell.row, cell.column), (1, 1));
    let _ = app.update(Message::ArrowKeyPressed {
        direction: ArrowKey::Up, shortcut: "UP".into(), extend_selection: false,
    });
    assert_eq!(app.tabs[app.active_tab].scene.table_cells.unwrap().anchor, (0, 1));
    let _ = app.update(Message::DynTabPrev);
    assert_eq!(app.tabs[app.active_tab].scene.table_cells.unwrap().anchor, (0, 0));
}

#[test]
fn find_inserts_a_native_cell_field_and_editor_undo_is_local() {
    use crate::ui::window::field_dialog::{FieldDialogMsg, CATEGORIES};
    let (mut app, handle) = fixture("prefix ");
    app.tabs[app.active_tab].scene.document.summary_info.custom_properties = vec![("Job No".into(), "42".into())];
    start(&mut app, handle);
    let before = source(&app, handle);
    let _ = app.update(Message::CommandAppendChar("x".into()));
    let _ = app.update(Message::Undo);
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), before);
    assert_eq!(source(&app, handle), before);
    let _ = app.run_action("FIND");
    assert!(app.field_dialog.is_some(), "FIND must insert a field while editing");
    let category = CATEGORIES.iter().position(|(name, _)| *name == "Document").unwrap();
    let _ = app.on_field_dialog(FieldDialogMsg::Category(category));
    let pick = app.field_dialog.as_ref().unwrap().entries().into_iter().find(|e| e.0 == "Job No").unwrap();
    let _ = app.on_field_dialog(pick.2);
    let _ = app.on_field_dialog(FieldDialogMsg::Ok);
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "prefix 42");
    assert!(app.commit_table_cell_editor());
    let Some(EntityType::Table(table)) = app.tabs[app.active_tab].scene.document.get_entity(handle) else { panic!("table") };
    assert!(table.cell(0, 0).unwrap().contents[0].field_handle.is_some());
    start(&mut app, handle);
    app.mtext_type(" suffix");
    assert!(app.commit_table_cell_editor());
    let Some(EntityType::Table(table)) = app.tabs[app.active_tab].scene.document.get_entity(handle) else { panic!("table") };
    assert!(table.cell(0, 0).unwrap().contents[0].field_handle.is_some(), "editing around a field detached it");
    assert_eq!(table.cell_text(0, 0), Some("prefix 42 suffix"));
    let task = app.run_action("FIND");
    use iced::futures::StreamExt;
    if let Some(mut stream) = iced_runtime::task::into_stream(task) {
        while let Some(action) = iced::futures::executor::block_on(stream.next()) {
            if let iced_runtime::Action::Output(message) = action {
                let _ = app.update(message);
            }
        }
    }
    assert_eq!(app.active_modal, Some(super::ModalKind::FindReplace));
}

#[test]
fn cursor_motion_with_nearest_snap_never_runs_snapping_during_editing() {
    let (mut app, handle) = fixture("typing");
    app.snapper.snap_enabled = true;
    app.snapper.enabled.insert(crate::snap::SnapType::Nearest);
    start(&mut app, handle);
    let epoch = app.tabs[app.active_tab].scene.geometry_epoch;
    for p in [iced::Point::new(400.0, 300.0), iced::Point::new(410.0, 320.0)] {
        let _ = app.update(Message::ViewportMove(p));
        let _ = app.update(Message::SnapDwellTick);
        assert!(app.tabs[app.active_tab].snap_result.is_none());
        assert!(app.snap_dwell.is_none());
    }
    assert_eq!(app.tabs[app.active_tab].scene.geometry_epoch, epoch);
    assert_eq!(source(&app, handle), "typing");
}

#[test]
fn scripted_table_edit_keeps_the_free_text_command_path() {
    let (mut app, handle) = fixture("before");
    let _ = app.run_command_line("TABLEDIT 10,-5 after");
    assert!(app.table_cell_editor.is_none());
    assert_eq!(source(&app, handle), "after");
    let _ = app.dispatch_command("-TABLEDIT");
    let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(10.0, -5.0, 0.0)));
    assert!(app.tabs[app.active_tab].active_cmd.as_ref().unwrap().table_cell().is_some());
    assert!(app.mtext_editor.is_none());
    let _ = app.update(Message::CommandEscape);
    let _ = app.begin_table_cell_edit(app.active_tab, handle, DVec3::new(10.0, -5.0, 0.0));
    assert!(app.table_cell_editor.is_some(), "a cancelled command-line edit must not change double-click editing");
}

#[test]
fn double_click_entry_preserves_authored_text_geometry() {
    let (mut app, handle) = fixture("authored");
    let i = app.active_tab;
    let Some(EntityType::Table(table)) = app.tabs[i].scene.document.get_entity(handle) else { panic!("table") };
    let picture = crate::entities::table::cell_picture(&app.tabs[i].scene.document, table, 0, 0).unwrap();
    let member = picture.members[0];
    if let Some(EntityType::MText(text)) = app.tabs[i].scene.document.get_entity_mut(member) {
        text.height = 2.25;
        text.rotation = 0.3;
        text.rectangle_width = 13.5;
    }
    let before = entities(&app);
    let result = app.begin_table_cell_edit(i, handle, DVec3::new(10.0, -5.0, 0.0));
    assert_eq!(result, crate::modules::annotate::table_cmd::TableCellEditStart::Started);
    let ed = app.mtext_editor.as_ref().expect("in-place text surface");
    assert_eq!(ed.height_value(), 2.25);
    assert!((ed.rotation_input_value().unwrap() - 0.3).abs() < 1e-8);
    assert_eq!(ed.rect_width, 13.5);
    app.mtext_cancel();
    assert_eq!(entities(&app), before);
}

#[test]
fn pictureless_table_preview_and_cancel_never_create_document_entities() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let mut table = Table::new(Vector3::ZERO, 1, 1);
    table.columns[0].width = 20.0;
    table.rows[0].height = 10.0;
    table.cell_mut(0, 0).unwrap().set_text("source");
    let i = app.active_tab;
    let handle = app.tabs[i].scene.document.add_entity(EntityType::Table(Box::new(table))).unwrap();
    let before = entities(&app);
    start(&mut app, handle);
    assert_eq!(entities(&app), before);
    app.mtext_type("draft");
    app.mtext_cancel();
    assert_eq!(entities(&app), before);
}

#[test]
fn tab_skips_merged_followers_and_locks_then_extends_the_last_row() {
    use document::entities::table::{CellRange, CellStateFlags};
    let (mut app, handle) = fixture("merged");
    let i = app.active_tab;
    if let Some(EntityType::Table(table)) = app.tabs[i].scene.document.get_entity_mut(handle) {
        table.merged_ranges.push(CellRange::new(0, 0, 0, 1));
        table.cell_mut(0, 2).unwrap().state.insert(CellStateFlags::CONTENT_LOCKED);
        table.block_record_handle = None;
    }
    app.tabs[i].scene.bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
    start(&mut app, handle);
    let _ = app.update(Message::DynTabNext);
    assert_eq!(app.tabs[i].scene.table_cells.unwrap().anchor, (0, 3));
    assert!(app.open_table_cell_editor(handle, 2, 3));
    let _ = app.update(Message::DynTabNext);
    let Some(EntityType::Table(table)) = app.tabs[i].scene.document.get_entity(handle) else {
        panic!("table")
    };
    assert_eq!(table.row_count(), 4);
    assert_eq!(app.tabs[i].scene.table_cells.unwrap().anchor, (3, 0));
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "");
}

#[test]
fn a_value_type_question_returns_to_the_cell_or_commits_and_moves() {
    use document::entities::table::{CellStyle, CellStylePropertyFlags as P};
    use crate::ui::window::table_cell_dialog::TableCellDialogMsg as M;
    let (mut app, handle) = fixture("12");
    let i = app.active_tab;
    if let Some(EntityType::Table(table)) = app.tabs[i].scene.document.get_entity_mut(handle) {
        let mut style = CellStyle::default();
        style.property_flags.insert(P::DATA_TYPE);
        style.value_data_type = 2;
        table.cell_mut(0, 0).unwrap().style = Some(style);
    }
    start(&mut app, handle);
    app.mtext_select_all();
    app.mtext_type("word");
    let _ = app.update(Message::DynTabNext);
    assert_eq!(app.active_modal, Some(super::ModalKind::TableCellEntry));
    assert_eq!(source(&app, handle), "12");
    let _ = app.update(Message::TableCellDialog(M::EntryValue));
    assert!(app.table_cell_editor.is_some());
    let _ = app.update(Message::DynTabNext);
    let _ = app.update(Message::TableCellDialog(M::EntryType));
    assert_eq!(source(&app, handle), "word");
    assert_eq!(app.tabs[i].scene.table_cells.unwrap().anchor, (0, 1));
}

#[test]
fn field_object_pick_suspends_the_surface_and_returns_to_the_same_draft() {
    use crate::ui::window::field_dialog::{FieldDialogMsg, FieldTarget};
    let (mut app, handle) = fixture("draft");
    let i = app.active_tab;
    let line = app.tabs[i].scene.add_entity(EntityType::Line(document::entities::Line::from_points(
        Vector3::new(100.0, 100.0, 0.0), Vector3::new(120.0, 100.0, 0.0),
    )));
    start(&mut app, handle);
    app.mtext_type("!");
    let _ = app.run_action("FIND");
    let _ = app.on_field_dialog(FieldDialogMsg::SelectObject);
    assert!(app.tabs[i].active_cmd.is_some());
    assert!(app.table_cell_canvas().is_none(), "the cell surface must yield to the field pick");
    let _ = app.feed_command(crate::command::StepInput::EntityPick(line, DVec3::new(110.0, 100.0, 0.0)));
    assert!(app.table_cell_editor.is_some(), "the field callback committed the cell prematurely");
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "draft!");
    assert_eq!(source(&app, handle), "draft");
    assert_eq!(app.field_dialog.as_ref().unwrap().target, FieldTarget::EditorInsert);
    app.close_field_dialog();
    assert!(app.table_cell_canvas().is_some());
    app.mtext_cancel();
    assert_eq!(source(&app, handle), "draft");
}

#[test]
fn cursor_hover_keeps_the_draft_open_and_an_outside_viewport_click_commits() {
    let (mut app, handle) = fixture("draft");
    start(&mut app, handle);
    app.mtext_type("!");
    let _ = app.update(Message::CursorMoved(iced::Point::new(2.0, 2.0), None));
    assert!(app.table_cell_editor.is_some(), "hover must not commit a draft");
    assert_eq!(source(&app, handle), "draft");
    let _ = app.update(Message::ViewportClick(None));
    assert!(app.table_cell_editor.is_none());
    assert_eq!(source(&app, handle), "draft!");
}
