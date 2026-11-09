//! The Linetype Manager, driven by the messages its window sends.
use super::*;
use crate::scene::linetype_service as service;
use crate::ui::style::linetype::{ElementKind, LinetypeMsg as M, LIBRARY_TAB};
use document::entities::{AttributeEntity, Insert, Line};
use document::tables::linetype::{LineTypeComplexContent, LineTypeComplexData, LineTypeElement};
use document::tables::{Layer, LineType, TextStyle};
use document::types::{Handle, Vector3};
use document::{Document, EntityType};

fn fresh() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app.tabs[app.active_tab].is_start = false;
    app
}

fn doc(app: &OpenCADStudio) -> &Document {
    &app.tabs[app.active_tab].scene.document
}

fn doc_mut(app: &mut OpenCADStudio) -> &mut Document {
    let i = app.active_tab;
    &mut app.tabs[i].scene.document
}

fn send(app: &mut OpenCADStudio, message: M) {
    let _ = app.update(Message::Linetype(message));
}

fn typed(app: &mut OpenCADStudio, line: &str) {
    let _ = app.update(Message::CommandInput(line.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

fn is_open(app: &OpenCADStudio) -> bool {
    app.active_modal == Some(ModalKind::LinetypeManager)
}

fn undo_depth(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].history.undo_stack.len()
}

fn lengths(app: &OpenCADStudio, name: &str) -> Vec<f64> {
    doc(app)
        .line_types
        .get(name)
        .unwrap_or_else(|| panic!("{name} missing"))
        .elements
        .iter()
        .map(|element| element.length)
        .collect()
}

fn add_simple(app: &mut OpenCADStudio, name: &str, pattern: &[f64]) {
    let linetype = service::plan_create(doc(app), name, "Own", pattern).unwrap();
    service::apply_create(doc_mut(app), linetype).unwrap();
}

/// A drawing with linetype FENCE and the manager open on it.
fn open_on_fence() -> OpenCADStudio {
    let mut app = fresh();
    add_simple(&mut app, "FENCE", &[8.0, -2.0, 3.0, -1.0]);
    send(&mut app, M::Open);
    send(&mut app, M::Select("FENCE".into()));
    app
}

fn add_line(app: &mut OpenCADStudio, linetype: &str) -> Handle {
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(10.0, 4.0, 0.0));
    line.common.linetype = linetype.to_string();
    doc_mut(app).add_entity(EntityType::Line(line)).unwrap()
}

fn entity_linetype(app: &OpenCADStudio, handle: Handle) -> String {
    doc(app)
        .get_entity(handle)
        .unwrap()
        .common()
        .linetype
        .clone()
}

fn add_layer(app: &mut OpenCADStudio, name: &str, linetype: &str) {
    let mut layer = Layer::new(name);
    layer.handle = doc_mut(app).allocate_handle();
    layer.line_type = linetype.to_string();
    doc_mut(app).layers.add(layer).unwrap();
}

fn close(app: &mut OpenCADStudio) {
    let _ = app.update(Message::CloseModal);
}

fn complex_linetype(app: &mut OpenCADStudio) {
    let mut style = TextStyle::new("NOTES");
    style.handle = doc_mut(app).allocate_handle();
    let style_handle = style.handle;
    doc_mut(app).text_styles.add(style).unwrap();
    let mut linetype = LineType::new("GASLINE");
    linetype.handle = doc_mut(app).allocate_handle();
    linetype.add_element(LineTypeElement::dash(12.0));
    linetype.add_element(LineTypeElement {
        length: -5.0,
        complex: Some(LineTypeComplexData {
            content: LineTypeComplexContent::Text { text: "GAS".into() },
            style_handle,
            ..Default::default()
        }),
    });
    linetype.pattern_length = 17.0;
    doc_mut(app).line_types.add(linetype).unwrap();
}

fn blocked(app: &OpenCADStudio) -> Option<String> {
    app.linetype_draft_error().map(|reason| reason.into_owned())
}

// ── Opening ──────────────────────────────────────────────────────────────

#[test]
fn each_command_opens_the_manager() {
    for command in ["LINETYPE", "linetype", "LT", "DDLTYPE", "LINETYPE DIALOG"] {
        let mut app = fresh();
        typed(&mut app, command);
        assert!(is_open(&app), "{command}");
    }
}

#[test]
fn it_opens_on_the_current_linetype() {
    let mut app = fresh();
    add_simple(&mut app, "FENCE", &[8.0, -2.0]);
    typed(&mut app, "LINETYPE SET FENCE");
    assert!(!is_open(&app), "the one-line form opens no window");
    assert_eq!(doc(&app).header.current_linetype_name, "FENCE");
    send(&mut app, M::Open);
    assert_eq!(app.linetype_selected, "FENCE");
    assert_eq!(app.linetype_draft.rows.len(), 2);
}

#[test]
fn a_script_keeps_the_prompts() {
    let mut app = fresh();
    add_simple(&mut app, "FENCE", &[8.0, -2.0]);
    for line in ["LINETYPE", "SET", "FENCE"] {
        let _ = app.update(Message::ScriptLine(line.into()));
    }
    assert!(!is_open(&app));
    assert_eq!(doc(&app).header.current_linetype_name, "FENCE");
}

#[test]
fn the_list_puts_the_built_ins_first_and_locks_them() {
    let app = open_on_fence();
    let (names, rows) = app.linetype_list();
    assert_eq!(&names[..3], ["ByLayer", "ByBlock", "Continuous"]);
    assert!(rows[..3].iter().all(|row| row.locked));
    let fence = names.iter().position(|name| name == "FENCE").unwrap();
    assert!(!rows[fence].locked);
}

#[test]
fn running_the_command_again_keeps_what_the_window_holds() {
    let mut app = open_on_fence();
    send(&mut app, M::ElementLength(0, "42".into()));
    send(&mut app, M::SetCurrent);
    typed(&mut app, "LINETYPE");
    assert!(is_open(&app));
    assert_eq!(app.linetype_selected, "FENCE", "still on the same linetype");
    assert_eq!(app.linetype_draft.rows[0].length, "42", "the edit is kept");
    close(&mut app);
    assert_ne!(
        doc(&app).header.current_linetype_name,
        "FENCE",
        "Close still restores what the first open captured"
    );
}

#[test]
fn only_a_window_of_its_own_is_raised() {
    let kind = ModalKind::LinetypeManager;
    let mut app = open_on_fence();
    assert!(
        app.raise_tool_window(kind).is_none(),
        "no window of its own"
    );
    app.os_windows.push(tool_windows::OsWindow {
        id: iced::window::Id::unique(),
        content: tool_windows::Detached::Tool(kind),
        placement: tool_windows::WindowPlacement {
            x: 0.0,
            y: 0.0,
            w: 960.0,
            h: 680.0,
        },
        scale: 1.0,
        placing: false,
    });
    assert!(app.raise_tool_window(kind).is_some());
    assert!(app.raise_tool_window(ModalKind::MlStyle).is_none());
}

// ── Arrow keys ───────────────────────────────────────────────────────────

fn key(app: &mut OpenCADStudio, name: &str) {
    let _ = app.update(Message::ShortcutPressed(name.into()));
}

fn row_after(app: &OpenCADStudio, name: &str) -> String {
    let (names, _) = app.linetype_list();
    let index = names.iter().position(|n| n == name).unwrap();
    names[index + 1].clone()
}

#[test]
fn up_and_down_step_through_the_list_and_stop_at_the_ends() {
    let mut app = fresh();
    send(&mut app, M::Open);
    send(&mut app, M::Select("ByLayer".into()));
    key(&mut app, "ARROWUP");
    assert_eq!(app.linetype_selected, "ByLayer", "stops at the top");
    key(&mut app, "ARROWDOWN");
    assert_eq!(app.linetype_selected, "ByBlock");
    assert_eq!(app.linetype_draft.rows.len(), 0, "the editor follows");
    key(&mut app, "ARROWDOWN");
    assert_eq!(app.linetype_selected, "Continuous");
    key(&mut app, "ARROWUP");
    assert_eq!(app.linetype_selected, "ByBlock");
    let (names, _) = app.linetype_list();
    let last = names.last().unwrap().clone();
    send(&mut app, M::Select(last.clone()));
    key(&mut app, "ARROWDOWN");
    assert_eq!(app.linetype_selected, last, "stops at the bottom");
    assert!(is_open(&app), "the keys do not close it");
}

#[test]
fn the_list_stays_still_until_the_selection_reaches_its_edge() {
    use crate::ui::style::linetype::ListView;
    let mut app = fresh();
    send(&mut app, M::Open);
    send(&mut app, M::Select("ByLayer".into()));
    let (names, _) = app.linetype_list();
    let count = names.len() as f32;
    // Rows 22 tall with 1 between, showing four and a bit.
    send(
        &mut app,
        M::ListScrolled(ListView {
            offset: 0.0,
            height: 100.0,
            content: count * 23.0 - 1.0,
        }),
    );
    let offset = |app: &OpenCADStudio| app.linetype_list_view.unwrap().offset;
    for step in 1..=3 {
        key(&mut app, "ARROWDOWN");
        assert_eq!(app.linetype_selected, names[step]);
        assert_eq!(offset(&app), 0.0, "row {step} is already in view");
    }
    key(&mut app, "ARROWDOWN");
    assert_eq!(offset(&app), 14.0, "just far enough for row 4");
    key(&mut app, "ARROWDOWN");
    assert_eq!(offset(&app), 37.0, "one row more");
    for _ in 0..3 {
        key(&mut app, "ARROWUP");
    }
    assert_eq!(offset(&app), 37.0, "up inside the view moves nothing");
    key(&mut app, "ARROWUP");
    assert_eq!(app.linetype_selected, names[1]);
    assert_eq!(offset(&app), 23.0, "the top edge pulls it back");
}

#[test]
fn a_list_that_fits_never_scrolls() {
    let mut app = fresh();
    send(&mut app, M::Open);
    send(&mut app, M::Select("ByLayer".into()));
    key(&mut app, "ARROWDOWN");
    assert_eq!(app.linetype_selected, "ByBlock");
    assert_eq!(app.linetype_list_view, None);
}

#[test]
fn the_arrow_key_message_of_a_frame_steps_too() {
    let mut app = fresh();
    send(&mut app, M::Open);
    send(&mut app, M::Select("ByLayer".into()));
    let arrow = |direction| Message::ArrowKeyPressed {
        direction,
        shortcut: String::new(),
        extend_selection: false,
    };
    let _ = app.update(arrow(ArrowKey::Down));
    assert_eq!(app.linetype_selected, "ByBlock");
    let _ = app.update(arrow(ArrowKey::Up));
    assert_eq!(app.linetype_selected, "ByLayer");
}

#[test]
fn the_arrows_follow_the_in_use_filter() {
    let mut app = fresh();
    add_simple(&mut app, "AAA-USED", &[8.0, -2.0]);
    add_simple(&mut app, "AAB-UNUSED", &[4.0, -2.0]);
    add_simple(&mut app, "AAC-USED", &[3.0, -2.0]);
    add_line(&mut app, "AAA-USED");
    add_line(&mut app, "AAC-USED");
    send(&mut app, M::Open);
    send(&mut app, M::InUseFilter(true));
    send(&mut app, M::Select("AAA-USED".into()));
    assert_eq!(row_after(&app, "AAA-USED"), "AAC-USED");
    key(&mut app, "ARROWDOWN");
    assert_eq!(
        app.linetype_selected, "AAC-USED",
        "the hidden row is skipped"
    );
}

#[test]
fn an_arrow_keeps_a_valid_edit_and_a_bad_one_stays_put() {
    let mut app = open_on_fence();
    let next = row_after(&app, "FENCE");
    send(&mut app, M::ElementKind(0, ElementKind::Dot));
    key(&mut app, "ARROWDOWN");
    assert_eq!(app.linetype_selected, next);
    send(&mut app, M::Select("FENCE".into()));
    assert_eq!(app.linetype_draft.rows[0].kind, ElementKind::Dot, "staged");

    // Invalid: leading gap. The row must not change.
    send(&mut app, M::ElementKind(0, ElementKind::Gap));
    assert!(blocked(&app).is_some());
    key(&mut app, "ARROWDOWN");
    assert_eq!(app.linetype_selected, "FENCE");
}

#[test]
fn typing_in_a_field_hands_the_arrows_to_it_until_a_row_is_pressed() {
    type Edit = fn(&mut OpenCADStudio);
    let edits: [Edit; 3] = [
        |app| send(app, M::Description("Words".into())),
        |app| send(app, M::ElementLength(0, "6".into())),
        |app| send(app, M::Scale("ltscale", "2".into())),
    ];
    for edit in edits {
        let mut app = open_on_fence();
        edit(&mut app);
        key(&mut app, "ARROWDOWN");
        assert_eq!(app.linetype_selected, "FENCE");
        send(&mut app, M::Select("FENCE".into()));
        key(&mut app, "ARROWDOWN");
        assert_ne!(app.linetype_selected, "FENCE", "a press gives them back");
    }
}

#[test]
fn the_arrows_wait_for_a_rename() {
    let mut app = open_on_fence();
    let _ = app.update(Message::StyleRenameStart(
        StyleKind::Linetype,
        "FENCE".into(),
    ));
    key(&mut app, "ARROWDOWN");
    assert_eq!(app.linetype_selected, "FENCE");
}

#[test]
fn another_managers_arrows_do_nothing_here() {
    let mut app = fresh();
    send(&mut app, M::Open);
    send(&mut app, M::Select("ByLayer".into()));
    close(&mut app);
    let _ = app.update(Message::MlStyleDialogOpen);
    key(&mut app, "ARROWDOWN");
    assert_eq!(app.linetype_selected, "ByLayer");
    assert_eq!(app.active_modal, Some(ModalKind::MlStyle));
}

// ── New and Copy ─────────────────────────────────────────────────────────

#[test]
fn new_makes_a_visible_pattern_under_a_unique_name() {
    let mut app = open_on_fence();
    send(&mut app, M::New);
    assert_eq!(app.linetype_selected, "Linetype1");
    assert_eq!(lengths(&app, "Linetype1"), vec![10.0, -5.0]);
    let _ = app.update(Message::StyleRenameCancel);
    send(&mut app, M::New);
    assert_eq!(app.linetype_selected, "Linetype2");
}

#[test]
fn copy_keeps_text_and_shape_elements() {
    let mut app = fresh();
    complex_linetype(&mut app);
    send(&mut app, M::Open);
    send(&mut app, M::Select("GASLINE".into()));
    send(&mut app, M::Copy);
    assert_eq!(app.linetype_selected, "GASLINE (1)");
    let copy = doc(&app).line_types.get("GASLINE (1)").unwrap();
    assert!(copy.is_complex());
    assert_ne!(
        copy.handle,
        doc(&app).line_types.get("GASLINE").unwrap().handle
    );
}

#[test]
fn a_copy_of_continuous_gets_a_pattern_and_of_an_xref_is_the_drawings_own() {
    let mut app = fresh();
    let mut bound = LineType::new("XR|DASH");
    bound.handle = doc_mut(&mut app).allocate_handle();
    bound.xref_dependent = true;
    bound.add_element(LineTypeElement::dash(4.0));
    bound.add_element(LineTypeElement::space(1.0));
    doc_mut(&mut app).line_types.add(bound).unwrap();
    send(&mut app, M::Open);
    send(&mut app, M::Select("Continuous".into()));
    send(&mut app, M::Copy);
    assert_eq!(lengths(&app, "Continuous (1)"), vec![10.0, -5.0]);
    let _ = app.update(Message::StyleRenameCancel);
    send(&mut app, M::Select("XR|DASH".into()));
    send(&mut app, M::Copy);
    let copy = doc(&app).line_types.get("DASH (1)").expect("copied");
    assert!(!copy.xref_dependent);
}

#[test]
fn new_and_copy_are_one_undo_step_on_apply() {
    let mut app = open_on_fence();
    let depth = undo_depth(&app);
    send(&mut app, M::New);
    let _ = app.update(Message::StyleRenameCancel);
    send(&mut app, M::Copy);
    let _ = app.update(Message::StyleRenameCancel);
    assert_eq!(undo_depth(&app), depth, "staged until Apply");
    send(&mut app, M::Apply);
    app.finish_pending_history(app.active_tab);
    assert_eq!(undo_depth(&app), depth + 1);
    close(&mut app);
    app.undo_steps(1);
    assert!(!doc(&app).line_types.contains("Linetype1"));
}

// ── Pattern editing ──────────────────────────────────────────────────────

/// FENCE edited to the asymmetric 1.5,-0.25,dot,-0.75.
fn edit_to_asymmetric(app: &mut OpenCADStudio) {
    send(app, M::ElementLength(0, "1.5".into()));
    send(app, M::ElementLength(1, "0.25".into()));
    send(app, M::ElementKind(2, ElementKind::Dot));
    send(app, M::ElementLength(3, "0.75".into()));
}

#[test]
fn an_asymmetric_pattern_applies_exactly() {
    let mut app = open_on_fence();
    let line = add_line(&mut app, "FENCE");
    edit_to_asymmetric(&mut app);
    send(&mut app, M::Description("Edited".into()));
    assert_eq!(blocked(&app), None);
    assert_eq!(
        lengths(&app, "FENCE"),
        vec![8.0, -2.0, 3.0, -1.0],
        "not before Apply"
    );
    let epoch = app.tabs[app.active_tab].scene.geometry_epoch;
    send(&mut app, M::Apply);
    assert_eq!(lengths(&app, "FENCE"), vec![1.5, -0.25, 0.0, -0.75]);
    let stored = doc(&app).line_types.get("FENCE").unwrap();
    assert_eq!(stored.pattern_length, 2.5);
    assert_eq!(stored.description, "Edited");
    assert!(app.tabs[app.active_tab].dirty);
    assert_ne!(
        app.tabs[app.active_tab].scene.geometry_epoch, epoch,
        "redrawn"
    );
    assert_eq!(entity_linetype(&app, line), "FENCE");
}

#[test]
fn add_delete_and_reorder_change_the_rows() {
    let mut app = open_on_fence();
    send(&mut app, M::ElementMove(2, true));
    send(&mut app, M::Apply);
    assert_eq!(lengths(&app, "FENCE"), vec![8.0, 3.0, -2.0, -1.0]);
    send(&mut app, M::ElementMove(1, false));
    send(&mut app, M::ElementMove(3, false)); // Off the end: nothing.
    send(&mut app, M::ElementMove(0, true)); // Off the start: nothing.
    send(&mut app, M::ElementDelete(3));
    send(&mut app, M::ElementAdd);
    assert_eq!(app.linetype_draft.rows[3].kind, ElementKind::Gap);
    send(&mut app, M::ElementAdd);
    assert_eq!(app.linetype_draft.rows[4].kind, ElementKind::Dash);
    send(&mut app, M::Apply);
    assert_eq!(lengths(&app, "FENCE"), vec![8.0, -2.0, 3.0, -5.0, 5.0]);
}

#[test]
fn add_stops_at_twelve() {
    let mut app = open_on_fence();
    for _ in 0..20 {
        send(&mut app, M::ElementAdd);
    }
    assert_eq!(app.linetype_draft.rows.len(), 12);
}

#[test]
fn a_kind_change_keeps_a_usable_length() {
    let mut app = open_on_fence();
    send(&mut app, M::ElementKind(0, ElementKind::Dot));
    assert_eq!(app.linetype_draft.rows[0].length, "");
    send(&mut app, M::ElementKind(0, ElementKind::Dash));
    assert_eq!(app.linetype_draft.rows[0].length, "1");
    assert_eq!(blocked(&app), None);
}

#[test]
fn invalid_edits_block_apply_with_the_reason_and_change_nothing() {
    type Break = fn(&mut OpenCADStudio);
    let cases: [(&str, Break); 7] = [
        ("leading gap", |app| {
            send(app, M::ElementKind(0, ElementKind::Gap))
        }),
        ("no gap", |app| {
            send(app, M::ElementKind(1, ElementKind::Dash));
            send(app, M::ElementKind(3, ElementKind::Dash));
        }),
        ("one element", |app| {
            for _ in 0..3 {
                send(app, M::ElementDelete(0));
            }
        }),
        ("none", |app| {
            for _ in 0..4 {
                send(app, M::ElementDelete(0));
            }
        }),
        ("not a number", |app| {
            send(app, M::ElementLength(0, "abc".into()))
        }),
        ("empty length", |app| {
            send(app, M::ElementLength(1, "".into()))
        }),
        ("zero length", |app| {
            send(app, M::ElementLength(1, "0".into()))
        }),
    ];
    for (label, breaking) in cases {
        let mut app = open_on_fence();
        breaking(&mut app);
        assert!(blocked(&app).is_some(), "{label}");
        let count = doc(&app).line_types.len();
        send(&mut app, M::Apply);
        send(&mut app, M::New);
        send(&mut app, M::Copy);
        send(&mut app, M::Delete);
        send(&mut app, M::Select("HIDDEN".into()));
        assert_eq!(
            app.linetype_selected, "FENCE",
            "{label}: the edit is not lost"
        );
        assert_eq!(doc(&app).line_types.len(), count, "{label}");
        assert_eq!(
            lengths(&app, "FENCE"),
            vec![8.0, -2.0, 3.0, -1.0],
            "{label}"
        );
    }
}

#[test]
fn switching_keeps_a_valid_edit_for_apply() {
    let mut app = open_on_fence();
    add_simple(&mut app, "OTHER", &[4.0, -4.0]);
    edit_to_asymmetric(&mut app);
    send(&mut app, M::Select("OTHER".into()));
    assert_eq!(app.linetype_draft.rows.len(), 2);
    send(&mut app, M::Select("FENCE".into()));
    assert_eq!(app.linetype_draft.rows[0].length, "1.5");
    send(&mut app, M::Apply);
    assert_eq!(lengths(&app, "FENCE"), vec![1.5, -0.25, 0.0, -0.75]);
}

#[test]
fn closing_without_apply_drops_every_change() {
    let mut app = open_on_fence();
    let dirty = app.tabs[app.active_tab].dirty;
    let line = add_line(&mut app, "FENCE");
    add_layer(&mut app, "WALLS", "FENCE");
    doc_mut(&mut app).capture_layer_state("SAVED", "");
    // The stage opened before these references existed; open it afresh.
    close(&mut app);
    send(&mut app, M::Open);
    send(&mut app, M::Select("FENCE".into()));
    edit_to_asymmetric(&mut app);
    send(&mut app, M::Scale("ltscale", "4".into()));
    send(&mut app, M::SetCurrent);
    let _ = app.update(Message::StyleRenameStart(
        StyleKind::Linetype,
        "FENCE".into(),
    ));
    let _ = app.update(Message::StyleRenameEdit("POSTS".into()));
    let _ = app.update(Message::StyleRenameCommit(StyleKind::Linetype));
    assert!(doc(&app).line_types.contains("POSTS"));
    send(&mut app, M::New);
    close(&mut app);
    assert!(!is_open(&app));
    assert!(doc(&app).line_types.contains("FENCE"));
    assert!(!doc(&app).line_types.contains("POSTS"));
    assert!(!doc(&app).line_types.contains("Linetype1"));
    assert_eq!(lengths(&app, "FENCE"), vec![8.0, -2.0, 3.0, -1.0]);
    assert_eq!(entity_linetype(&app, line), "FENCE");
    assert_eq!(doc(&app).layers.get("WALLS").unwrap().line_type, "FENCE");
    let state = doc(&app).layer_state("SAVED").unwrap();
    assert!(state.layers.iter().any(|l| l.line_type == "FENCE"));
    assert_eq!(doc(&app).header.linetype_scale, 1.0);
    assert_ne!(doc(&app).header.current_linetype_name, "FENCE");
    let _ = dirty;
}

#[test]
fn another_managers_close_leaves_a_layer_edit_alone() {
    let mut app = fresh();
    add_simple(&mut app, "FENCE", &[8.0, -2.0]);
    add_layer(&mut app, "WALLS", "Continuous");
    let _ = app.update(Message::MlStyleDialogOpen);
    doc_mut(&mut app).layers.get_mut("WALLS").unwrap().line_type = "FENCE".into();
    close(&mut app);
    assert_eq!(doc(&app).layers.get("WALLS").unwrap().line_type, "FENCE");
}

// ── Rename ───────────────────────────────────────────────────────────────

fn rename(app: &mut OpenCADStudio, old: &str, new: &str) {
    let _ = app.update(Message::StyleRenameStart(StyleKind::Linetype, old.into()));
    let _ = app.update(Message::StyleRenameEdit(new.into()));
    let _ = app.update(Message::StyleRenameCommit(StyleKind::Linetype));
}

#[test]
fn an_inline_rename_updates_references_and_undoes_in_one_step() {
    let mut app = fresh();
    add_simple(&mut app, "FENCE", &[8.0, -2.0, 3.0, -1.0]);
    let line = add_line(&mut app, "FENCE");
    let mut attribute = AttributeEntity::new("TAG".into(), "VALUE".into());
    attribute.common.linetype = "FENCE".into();
    let mut insert = Insert::new("HOLDER", Vector3::ZERO);
    insert.attributes.push(attribute);
    let insert = doc_mut(&mut app)
        .add_entity(EntityType::Insert(insert))
        .unwrap();
    add_layer(&mut app, "WALLS", "FENCE");
    doc_mut(&mut app).capture_layer_state("SAVED", "");
    let handle = doc(&app).line_types.get("FENCE").unwrap().handle;
    send(&mut app, M::Open);
    send(&mut app, M::Select("FENCE".into()));
    let depth = undo_depth(&app);
    rename(&mut app, "FENCE", "POSTS");
    assert_eq!(app.linetype_selected, "POSTS");
    send(&mut app, M::Apply);
    app.finish_pending_history(app.active_tab);
    assert_eq!(doc(&app).line_types.get("POSTS").unwrap().handle, handle);
    assert_eq!(entity_linetype(&app, line), "POSTS");
    let attribute_of = |app: &OpenCADStudio| match doc(app).get_entity(insert) {
        Some(EntityType::Insert(insert)) => insert.attributes[0].common.linetype.clone(),
        _ => panic!("not an insert"),
    };
    assert_eq!(attribute_of(&app), "POSTS");
    assert_eq!(doc(&app).layers.get("WALLS").unwrap().line_type, "POSTS");
    assert_eq!(undo_depth(&app), depth + 1);
    close(&mut app);
    app.undo_steps(1);
    assert!(doc(&app).line_types.contains("FENCE"));
    assert_eq!(entity_linetype(&app, line), "FENCE");
    assert_eq!(attribute_of(&app), "FENCE");
    assert_eq!(doc(&app).layers.get("WALLS").unwrap().line_type, "FENCE");
    let state = doc(&app).layer_state("SAVED").unwrap();
    assert!(state.layers.iter().any(|l| l.line_type == "FENCE"));
}

#[test]
fn a_rename_refuses_catalog_taken_and_built_in_names() {
    let mut app = open_on_fence();
    add_simple(&mut app, "OTHER", &[4.0, -4.0]);
    for new in ["other", "ByBlock", "A,B"] {
        rename(&mut app, "FENCE", new);
        assert!(doc(&app).line_types.contains("FENCE"), "{new}");
    }
    rename(&mut app, "ByLayer", "NOPE");
    assert!(doc(&app).line_types.contains("ByLayer"));
    assert!(!doc(&app).line_types.contains("NOPE"));
    // A catalog name the drawing has not loaded.
    doc_mut(&mut app).line_types.remove("DASHED");
    rename(&mut app, "FENCE", "DASHED");
    assert!(doc(&app).line_types.contains("FENCE"));
    assert!(!doc(&app).line_types.contains("DASHED"));
}

// ── Delete ───────────────────────────────────────────────────────────────

#[test]
fn delete_refuses_what_is_used_current_or_locked() {
    let mut app = open_on_fence();
    add_line(&mut app, "FENCE");
    send(&mut app, M::Delete);
    assert!(doc(&app).line_types.contains("FENCE"), "used");

    let mut app = open_on_fence();
    send(&mut app, M::SetCurrent);
    send(&mut app, M::Delete);
    assert!(doc(&app).line_types.contains("FENCE"), "current");

    let mut app = open_on_fence();
    send(&mut app, M::Select("Continuous".into()));
    send(&mut app, M::Delete);
    assert!(doc(&app).line_types.contains("Continuous"), "built-in");
}

#[test]
fn delete_agrees_with_purge_and_undoes() {
    let mut app = open_on_fence();
    let mut wanted = [false; crate::app::commands::purge::KINDS];
    wanted[crate::app::commands::purge::LINETYPES] = true;
    let list = crate::app::commands::purge::purge_pass(doc(&app), &[], wanted, "");
    assert!(list[crate::app::commands::purge::LINETYPES].contains(&"FENCE".to_string()));
    send(&mut app, M::Delete);
    assert!(!doc(&app).line_types.contains("FENCE"));
    assert_eq!(app.linetype_selected, "ByLayer", "the list moved on");
    send(&mut app, M::Apply);
    app.finish_pending_history(app.active_tab);
    close(&mut app);
    app.undo_steps(1);
    assert_eq!(lengths(&app, "FENCE"), vec![8.0, -2.0, 3.0, -1.0]);
}

// ── Set Current ──────────────────────────────────────────────────────────

fn drawn_linetype(app: &mut OpenCADStudio) -> String {
    let known = doc(app).entities().count();
    typed(app, "LINE");
    typed(app, "0,0");
    typed(app, "@20<30");
    let _ = app.update(Message::CommandEscape);
    let _ = app.update(Message::CommandEscape);
    doc(app)
        .entities()
        .nth(known)
        .expect("drawn")
        .common()
        .linetype
        .clone()
}

#[test]
fn set_current_reaches_the_next_line_and_the_ribbon() {
    let mut app = open_on_fence();
    send(&mut app, M::SetCurrent);
    send(&mut app, M::Apply);
    close(&mut app);
    assert_eq!(app.ribbon.active_linetype, "FENCE");
    assert_eq!(drawn_linetype(&mut app), "FENCE");
}

#[test]
fn the_built_ins_can_be_made_current() {
    for name in ["ByBlock", "Continuous", "ByLayer"] {
        let mut app = open_on_fence();
        send(&mut app, M::SetCurrent);
        send(&mut app, M::Apply);
        send(&mut app, M::Select(name.into()));
        send(&mut app, M::SetCurrent);
        send(&mut app, M::Apply);
        assert_eq!(doc(&app).header.current_linetype_name, name);
        assert_eq!(app.ribbon.active_linetype, name);
    }
}

// ── Read-only ────────────────────────────────────────────────────────────

#[test]
fn complex_and_locked_patterns_cannot_be_edited() {
    let mut app = fresh();
    complex_linetype(&mut app);
    send(&mut app, M::Open);
    send(&mut app, M::Select("GASLINE".into()));
    assert!(app.linetype_is_complex());
    send(&mut app, M::ElementLength(0, "99".into()));
    send(&mut app, M::Description("Changed".into()));
    send(&mut app, M::ElementDelete(0));
    send(&mut app, M::ElementAdd);
    send(&mut app, M::Apply);
    let stored = doc(&app).line_types.get("GASLINE").unwrap();
    assert_eq!(stored.elements.len(), 2);
    assert_eq!(stored.elements[0].length, 12.0);
    assert!(stored.is_complex());
    assert_eq!(stored.description, "");
    let preview = app.linetype_preview();
    assert_eq!(preview[1].label.as_deref(), Some("GAS"));

    send(&mut app, M::Select("Continuous".into()));
    send(&mut app, M::ElementAdd);
    assert!(app.linetype_draft.rows.is_empty());
    assert_eq!(
        blocked(&app),
        None,
        "a locked entry does not block the list"
    );
}

// ── Counts and filter ────────────────────────────────────────────────────

#[test]
fn counts_include_objects_and_layers_and_the_filter_hides_unused() {
    let mut app = fresh();
    add_simple(&mut app, "FENCE", &[8.0, -2.0]);
    add_simple(&mut app, "UNUSED", &[4.0, -2.0]);
    add_line(&mut app, "FENCE");
    add_line(&mut app, "fence");
    add_layer(&mut app, "WALLS", "FENCE");
    send(&mut app, M::Open);
    let count_of = |app: &OpenCADStudio, wanted: &str| {
        let (names, rows) = app.linetype_list();
        names
            .iter()
            .position(|name| name == wanted)
            .map(|index| rows[index].count)
    };
    assert_eq!(count_of(&app, "FENCE"), Some(3));
    assert_eq!(count_of(&app, "UNUSED"), Some(0));
    send(&mut app, M::InUseFilter(true));
    assert_eq!(count_of(&app, "UNUSED"), None, "hidden");
    assert_eq!(count_of(&app, "FENCE"), Some(3));
    send(&mut app, M::InUseFilter(false));
    send(&mut app, M::Select("UNUSED".into()));
    send(&mut app, M::InUseFilter(true));
    assert_eq!(count_of(&app, "UNUSED"), Some(0), "the selected row stays");
}

#[test]
fn the_count_updates_after_a_change() {
    let mut app = open_on_fence();
    add_line(&mut app, "FENCE");
    send(&mut app, M::Copy);
    let _ = app.update(Message::StyleRenameCancel);
    send(&mut app, M::Select("FENCE".into()));
    let (names, rows) = app.linetype_list();
    let index = names.iter().position(|name| name == "FENCE").unwrap();
    assert_eq!(rows[index].count, 1);
}

// ── Preview ──────────────────────────────────────────────────────────────

#[test]
fn the_preview_follows_the_editor() {
    let mut app = open_on_fence();
    send(&mut app, M::ElementLength(0, "20".into()));
    assert_eq!(app.linetype_preview()[0].length, 20.0);
    // An unreadable row falls back to the stored pattern.
    send(&mut app, M::ElementLength(1, "abc".into()));
    assert_eq!(app.linetype_preview()[0].length, 8.0);
}

// ── Scale ────────────────────────────────────────────────────────────────

#[test]
fn each_scale_writes_its_variable_on_apply_and_undoes() {
    let mut app = open_on_fence();
    let ms_before = app.tabs[app.active_tab].scene.model_linetype_scaling();
    let ps_before = doc(&app).header.paper_space_linetype_scaling;
    send(&mut app, M::Scale("ltscale", "2.5".into()));
    send(&mut app, M::Scale("celtscale", "0.75".into()));
    send(&mut app, M::ScaleToggle("msltscale", !ms_before));
    send(&mut app, M::ScaleToggle("psltscale", !ps_before));
    assert_eq!(doc(&app).header.linetype_scale, 1.0, "not before Apply");
    send(&mut app, M::Apply);
    app.finish_pending_history(app.active_tab);
    assert_eq!(doc(&app).header.linetype_scale, 2.5);
    assert_eq!(doc(&app).header.current_entity_linetype_scale, 0.75);
    assert_eq!(
        app.tabs[app.active_tab].scene.model_linetype_scaling(),
        !ms_before
    );
    assert_eq!(doc(&app).header.paper_space_linetype_scaling, !ps_before);
    close(&mut app);
    app.undo_steps(1);
    assert_eq!(doc(&app).header.linetype_scale, 1.0);
    assert_eq!(
        app.tabs[app.active_tab].scene.model_linetype_scaling(),
        ms_before
    );
}

#[test]
fn a_bad_scale_blocks_apply() {
    for bad in ["0", "-1", "abc", ""] {
        let mut app = open_on_fence();
        send(&mut app, M::Scale("ltscale", bad.into()));
        assert!(blocked(&app).is_some(), "{bad:?}");
        send(&mut app, M::Apply);
        assert_eq!(doc(&app).header.linetype_scale, 1.0, "{bad:?}");
    }
}

// ── Import and Library ───────────────────────────────────────────────────

#[test]
fn an_import_is_staged_with_the_other_changes() {
    let source = "*ZZ-IMPORTED,Loaded\nA,6,-3,1,-2\n".to_string();
    let load = |app: &mut OpenCADStudio| {
        let _ = app.update(Message::LinetypeLoaded(Some((None, source.clone()))));
    };
    let mut app = open_on_fence();
    load(&mut app);
    assert!(doc(&app).line_types.contains("ZZ-IMPORTED"));
    close(&mut app);
    assert!(
        !doc(&app).line_types.contains("ZZ-IMPORTED"),
        "Close drops it"
    );

    let mut app = open_on_fence();
    let depth = undo_depth(&app);
    load(&mut app);
    send(&mut app, M::Apply);
    app.finish_pending_history(app.active_tab);
    assert_eq!(lengths(&app, "ZZ-IMPORTED"), vec![6.0, -3.0, 1.0, -2.0]);
    assert_eq!(undo_depth(&app), depth + 1);
    close(&mut app);
    app.undo_steps(1);
    assert!(!doc(&app).line_types.contains("ZZ-IMPORTED"));
}

#[test]
fn a_load_with_the_manager_closed_is_its_own_undo_step() {
    let mut app = fresh();
    let depth = undo_depth(&app);
    let source = "*ZZ-CLOSED,Loaded\nA,6,-3\n".to_string();
    let _ = app.update(Message::LinetypeLoaded(Some((None, source))));
    app.finish_pending_history(app.active_tab);
    assert!(doc(&app).line_types.contains("ZZ-CLOSED"));
    assert_eq!(undo_depth(&app), depth + 1);
}

fn library_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::app::commands::linetype_library_lock()
}

#[test]
fn the_library_tab_matches_the_commands() {
    let _guard = library_lock();
    let library = crate::io::linetypes::writer::library_path().unwrap();
    let _ = std::fs::remove_file(&library);
    crate::io::linetypes::refresh_catalog();

    let mut app = fresh();
    add_simple(&mut app, "ZZ-UI", &[9.0, -2.0, 3.0, -1.0]);
    send(&mut app, M::Open);
    send(&mut app, M::Select("ZZ-UI".into()));
    send(&mut app, M::Tab(LIBRARY_TAB));
    // An unapplied edit is what gets saved.
    send(&mut app, M::ElementLength(0, "7".into()));
    send(&mut app, M::SaveToLibrary);
    assert!(
        app.linetype_library_status.contains("saved"),
        "{}",
        app.linetype_library_status
    );
    assert!(crate::io::linetypes::catalog_has("ZZ-UI"));
    let saved = std::fs::read_to_string(&library).unwrap();
    assert!(saved.contains("A,7,-2,3,-1"), "{saved}");

    let path = std::env::temp_dir().join(format!("ocs-lt-ui-{}.lin", std::process::id()));
    send(
        &mut app,
        M::ExportPicked {
            all: false,
            path: Some(path.display().to_string()),
        },
    );
    assert!(
        app.linetype_library_status.contains("Wrote 1"),
        "{}",
        app.linetype_library_status
    );
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .starts_with("*ZZ-UI"));
    send(
        &mut app,
        M::ExportPicked {
            all: true,
            path: Some(path.display().to_string()),
        },
    );
    assert!(std::fs::read_to_string(&path).unwrap().contains("*HIDDEN"));
    let _ = std::fs::remove_file(path);
    // Cancelling the file dialog does nothing.
    send(
        &mut app,
        M::ExportPicked {
            all: true,
            path: None,
        },
    );

    std::fs::remove_file(&library).unwrap();
    send(&mut app, M::ReloadLibrary);
    assert!(!crate::io::linetypes::catalog_has("ZZ-UI"));
    assert!(app.linetype_library_status.contains("reloaded"));
}
