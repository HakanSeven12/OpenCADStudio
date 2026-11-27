//! NAMEDSEL: the typed form, its undo steps, and the sets in a saved file.

use crate::app::{Message, OpenCADStudio};
use document::entities::Line;
use document::types::Vector3;
use document::{EntityType, Handle};

fn new_app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app
}

fn add_lines(app: &mut OpenCADStudio, count: usize) -> Vec<Handle> {
    let i = app.active_tab;
    (0..count)
        .map(|row| {
            let y = row as f64 * 5.0;
            app.tabs[i]
                .scene
                .add_entity(EntityType::Line(Line::from_points(
                    Vector3::new(0.0, y, 0.0),
                    Vector3::new(10.0, y, 0.0),
                )))
        })
        .collect()
}

fn hex(handles: &[Handle]) -> String {
    handles
        .iter()
        .map(|handle| format!("{:X}", handle.value()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn set(app: &OpenCADStudio, name: &str) -> Option<(String, Vec<Handle>)> {
    app.tabs[app.active_tab].scene.named_selection(name)
}

fn selected(app: &OpenCADStudio) -> Vec<Handle> {
    let mut handles: Vec<Handle> = app.tabs[app.active_tab]
        .scene
        .selected_entities()
        .into_iter()
        .map(|(handle, _)| handle)
        .collect();
    handles.sort_by_key(|handle| handle.value());
    handles
}

fn out(app: &OpenCADStudio) -> String {
    app.command_line.history_plain_text()
}

#[test]
fn save_then_recall_selects_the_members() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 3);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE Frame {}", hex(&lines[..2])));
    assert_eq!(
        set(&app, "FRAME"),
        Some(("Frame".to_string(), lines[..2].to_vec()))
    );
    assert!(app.tabs[app.active_tab].dirty);

    app.tabs[app.active_tab].scene.deselect_all();
    let _ = app.dispatch_command("NAMEDSEL RECALL frame");
    assert_eq!(selected(&app), lines[..2].to_vec());
    assert!(out(&app).contains("2 found"), "{}", out(&app));
}

#[test]
fn recall_adds_to_the_current_selection() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE A {}", hex(&lines[..1])));
    app.tabs[app.active_tab]
        .scene
        .select_entity(lines[1], false);
    let _ = app.dispatch_command("NAMEDSEL RECALL A");
    assert_eq!(selected(&app), lines);
}

#[test]
fn save_rename_delete_each_undo_in_one_step() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 1);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE A {}", hex(&lines)));
    let _ = app.dispatch_command("NAMEDSEL RENAME A B");
    assert!(set(&app, "A").is_none());
    assert!(set(&app, "B").is_some());
    let _ = app.dispatch_command("NAMEDSEL DELETE B");
    assert!(set(&app, "B").is_none());

    let _ = app.update(Message::Undo);
    assert!(set(&app, "B").is_some(), "undo of DELETE");
    let _ = app.update(Message::Undo);
    assert!(
        set(&app, "A").is_some() && set(&app, "B").is_none(),
        "undo of RENAME"
    );
    let _ = app.update(Message::Undo);
    assert!(
        app.tabs[app.active_tab].scene.named_selections().is_empty(),
        "undo of SAVE"
    );
    assert!(
        app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(lines[0])
            .is_some(),
        "the undo takes back the set, not the line"
    );

    let _ = app.update(Message::Redo);
    assert_eq!(
        set(&app, "A"),
        Some(("A".to_string(), lines.clone())),
        "redo of SAVE"
    );
}

#[test]
fn undo_of_a_replacing_save_gives_back_the_old_members() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE A {}", hex(&lines[..1])));
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE a {}", hex(&lines[1..])));
    assert_eq!(set(&app, "A").unwrap().1, lines[1..].to_vec());
    assert!(out(&app).contains("replaced"), "{}", out(&app));

    let _ = app.update(Message::Undo);
    assert_eq!(set(&app, "A").unwrap().1, lines[..1].to_vec());
}

#[test]
fn an_erase_undone_puts_the_member_back_in_the_recall() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE FRAME {}", hex(&lines)));
    app.tabs[app.active_tab]
        .scene
        .select_entity(lines[1], false);
    let _ = app.run_command_line("ERASE");
    let recall = app.tabs[app.active_tab]
        .scene
        .named_selection_recall("FRAME")
        .unwrap();
    assert_eq!((recall.selectable, recall.erased), (lines[..1].to_vec(), 1));

    let _ = app.update(Message::Undo);
    let recall = app.tabs[app.active_tab]
        .scene
        .named_selection_recall("FRAME")
        .unwrap();
    assert_eq!((recall.selectable, recall.erased), (lines.clone(), 0));
}

#[test]
fn a_recall_says_how_many_members_it_left_out() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE FRAME {}", hex(&lines)));
    app.tabs[app.active_tab]
        .scene
        .select_entity(lines[1], false);
    let _ = app.run_command_line("ERASE");

    let _ = app.dispatch_command("NAMEDSEL RECALL FRAME");
    let history = out(&app);
    assert!(history.contains("1 found"), "{history}");
    assert!(
        history.contains("1 erased objects not selected."),
        "{history}"
    );
    for absent in ["in other spaces", "hidden or locked"] {
        assert!(
            !history.contains(absent),
            "no line for a count of 0: {history}"
        );
    }
}

#[test]
fn a_recall_leaves_out_a_member_on_a_frozen_layer() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE FRAME {}", hex(&lines)));
    let scene = &mut app.tabs[app.active_tab].scene;
    let mut frozen = document::tables::Layer::new("FROZEN");
    frozen.flags.frozen = true;
    let _ = scene.document.layers.add(frozen);
    scene
        .document
        .get_entity_mut(lines[1])
        .unwrap()
        .common_mut()
        .layer = "FROZEN".to_string();

    let _ = app.dispatch_command("NAMEDSEL RECALL FRAME");
    assert_eq!(selected(&app), lines[..1].to_vec());
    assert!(
        out(&app).contains("1 hidden or locked objects not selected."),
        "{}",
        out(&app)
    );
}

#[test]
fn recall_in_a_layout_of_a_model_space_set_selects_nothing() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE FRAME {}", hex(&lines)));
    let document = app.automation_op(r#"{"protocol":1,"op":"state"}"#)["document_id"].clone();
    let activated = app.automation_op(
        &serde_json::json!({"protocol":1,"op":"layout_activate","request_id":"l1",
            "document_id":document,"name":"Layout1"})
        .to_string(),
    );
    assert_eq!(activated["ok"], true, "{activated}");

    let _ = app.dispatch_command("NAMEDSEL RECALL FRAME");
    assert!(selected(&app).is_empty());
    assert!(
        out(&app).contains("2 objects in other spaces not selected."),
        "{}",
        out(&app)
    );
    assert!(
        !out(&app).contains("erased objects"),
        "no line for a count of 0: {}",
        out(&app)
    );
}

#[test]
fn bad_names_and_unknown_sets_are_refused() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 1);
    let objects = app.tabs[app.active_tab].scene.document.objects.len();
    for typed in [
        format!("NAMEDSEL SAVE A*B {}", hex(&lines)),
        "NAMEDSEL SAVE A".to_string(),
        "NAMEDSEL RECALL GHOST".to_string(),
        "NAMEDSEL RENAME GHOST C".to_string(),
        "NAMEDSEL DELETE GHOST".to_string(),
    ] {
        app.command_line.last_error = None;
        let _ = app.dispatch_command(&typed);
        assert!(app.command_line.last_error.is_some(), "{typed} is refused");
    }
    assert_eq!(
        app.tabs[app.active_tab].scene.document.objects.len(),
        objects
    );

    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE A {}", hex(&lines)));
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE B {}", hex(&lines)));
    app.command_line.last_error = None;
    let _ = app.dispatch_command("NAMEDSEL RENAME A b");
    assert!(
        app.command_line.last_error.is_some(),
        "a used name is refused"
    );
    assert!(set(&app, "A").is_some());
}

#[test]
fn the_list_shows_each_set_and_its_size() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command("NAMEDSEL ?");
    assert!(out(&app).contains("no selection sets"), "{}", out(&app));
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE B {}", hex(&lines)));
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE A {}", hex(&lines[..1])));
    let _ = app.dispatch_command("NAMEDSEL ?");
    let text = out(&app);
    let a = text.rfind("A (1 objects)").expect(&text);
    let b = text.rfind("B (2 objects)").expect(&text);
    assert!(a < b, "sorted by name: {text}");
}

fn save_and_open(extension: &str) {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE FRAME {}", hex(&lines)));
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join(format!("named-selections.{extension}"));
    let path = path.to_string_lossy().to_string();
    let saved = app.automation_op(&serde_json::json!({"op":"save","path":path}).to_string());
    assert_eq!(saved["ok"], true, "the save reported success: {saved}");
    // A new app: its only copy of the drawing is the one read from the file.
    let mut reopened = OpenCADStudio::new_for_test();
    let opened = reopened.automation_op(&serde_json::json!({"op":"open","path":path}).to_string());
    assert_eq!(opened["ok"], true, "the open reported success: {opened}");
    assert_eq!(set(&reopened, "FRAME"), Some(("FRAME".to_string(), lines)));
}

#[test]
fn the_sets_survive_a_dxf_save_and_open() {
    save_and_open("dxf");
}

#[test]
fn the_sets_survive_a_dwg_save_and_open() {
    save_and_open("dwg");
}

#[test]
fn namedsel_saves_the_pickfirst_selection() {
    use crate::command::StepInput;
    let mut app = new_app();
    let lines = add_lines(&mut app, 3);
    let i = app.active_tab;
    for &handle in &lines[..2] {
        app.tabs[i].scene.select_entity(handle, false);
    }
    let _ = app.dispatch_command("NAMEDSEL");
    let _ = app.feed_command(StepInput::Text("S".into()));
    let _ = app.feed_command(StepInput::Text("FRAME".into()));
    assert_eq!(
        set(&app, "FRAME").map(|(_, members)| members),
        Some(lines[..2].to_vec())
    );
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the command ends after the save"
    );
}

fn type_line(app: &mut OpenCADStudio, text: &str) {
    let _ = app.update(Message::CommandInput(text.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

/// Typed at the command line, a set name keeps its case, and a Shift
/// letter bound to a shortcut types its letter.
#[test]
fn a_name_typed_at_the_command_line_keeps_its_case() {
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let i = app.active_tab;
    for &handle in &lines {
        app.tabs[i].scene.select_entity(handle, false);
    }
    app.shortcut_bindings
        .insert("SHIFT+F".to_string(), "QUADTOGGLE".to_string());
    let before = app.quad;

    type_line(&mut app, "NAMEDSEL");
    assert_eq!(
        app.tabs[i]
            .active_cmd
            .as_ref()
            .map(|command| command.name()),
        Some("NAMEDSEL")
    );
    type_line(&mut app, "S");
    app.shift_down = true;
    let _ = app.update(Message::CommandAppendChar("F".into()));
    app.shift_down = false;
    for glyph in ["r", "a", "m", "e"] {
        let _ = app.update(Message::CommandAppendChar(glyph.into()));
    }
    let _ = app.update(Message::CommandSubmit);

    assert_eq!(app.quad, before, "SHIFT+F must not run its shortcut");
    assert_eq!(
        set(&app, "frame"),
        Some(("Frame".to_string(), lines)),
        "{}",
        out(&app)
    );
}

#[test]
fn namedsel_recall_at_the_command_prompt_shows_grips() {
    use crate::command::StepInput;
    let mut app = new_app();
    let lines = add_lines(&mut app, 2);
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE FRAME {}", hex(&lines[1..])));
    let _ = app.dispatch_command("NAMEDSEL");
    let _ = app.feed_command(StepInput::Text("R".into()));
    let _ = app.feed_command(StepInput::Text("frame".into()));
    assert_eq!(selected(&app), lines[1..].to_vec());

    let document = app.automation_op(r#"{"protocol":1,"op":"state"}"#)["document_id"].clone();
    let grips = app.automation_op(
        &serde_json::json!({"protocol":1,"op":"grips","request_id":"g1","document_id":document})
            .to_string(),
    );
    let rows: Vec<f64> = grips["grips"]
        .as_array()
        .unwrap_or_else(|| panic!("{grips}"))
        .iter()
        .filter_map(|grip| grip["point"][1].as_f64())
        .collect();
    assert!(
        !rows.is_empty() && rows.iter().all(|y| (y - 5.0).abs() < 1e-9),
        "{grips}"
    );
}

fn line_start_x(app: &OpenCADStudio, handle: Handle) -> f64 {
    match app.tabs[app.active_tab].scene.document.get_entity(handle) {
        Some(EntityType::Line(line)) => line.start.x,
        _ => panic!("a line"),
    }
}

fn start_move_with_frame(app: &mut OpenCADStudio, lines: &[Handle]) {
    let _ = app.dispatch_command(&format!("NAMEDSEL SAVE FRAME {}", hex(&lines[..2])));
    app.tabs[app.active_tab].scene.deselect_all();
    let _ = app.run_command_line("MOVE");
    assert!(app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .is_some_and(|command| command.is_selection_gathering()));
}

#[test]
fn move_takes_a_named_set_at_select_objects() {
    use crate::command::StepInput;
    use glam::DVec3;
    let mut app = new_app();
    let lines = add_lines(&mut app, 3);
    start_move_with_frame(&mut app, &lines);
    let _ = app.feed_command(StepInput::Text("N".into()));
    assert!(
        out(&app).contains("Enter selection set name:"),
        "{}",
        out(&app)
    );
    let _ = app.feed_command(StepInput::Text("frame".into()));
    assert_eq!(selected(&app), lines[..2].to_vec());
    let _ = app.feed_command(StepInput::Enter);
    let _ = app.feed_command(StepInput::Point(DVec3::ZERO));
    let _ = app.feed_command(StepInput::Point(DVec3::new(100.0, 0.0, 0.0)));
    let moved: Vec<f64> = lines.iter().map(|&line| line_start_x(&app, line)).collect();
    assert_eq!(moved, [100.0, 100.0, 0.0]);
}

#[test]
fn the_named_keyword_removes_in_remove_mode() {
    use crate::command::StepInput;
    let mut app = new_app();
    let lines = add_lines(&mut app, 3);
    start_move_with_frame(&mut app, &lines);
    for typed in ["ALL", "R", "NAMED", "FRAME"] {
        let _ = app.feed_command(StepInput::Text(typed.into()));
    }
    assert_eq!(selected(&app), lines[2..].to_vec());
}

#[test]
fn an_unknown_set_name_asks_again() {
    use crate::command::StepInput;
    let mut app = new_app();
    let lines = add_lines(&mut app, 3);
    start_move_with_frame(&mut app, &lines);
    let _ = app.feed_command(StepInput::Text("N".into()));
    let _ = app.feed_command(StepInput::Text("GHOST".into()));
    assert!(
        app.command_line
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("Invalid selection set name.")),
        "{}",
        out(&app)
    );
    assert!(selected(&app).is_empty());
    let _ = app.feed_command(StepInput::Text("FRAME".into()));
    assert_eq!(
        selected(&app),
        lines[..2].to_vec(),
        "the name is asked again"
    );
}

/// Esc at "Enter selection set name:" ends the Named option: the next
/// "Select objects:" reads `L` as Last, not as a set name.
#[test]
fn escape_at_the_set_name_ends_the_named_option() {
    use crate::command::StepInput;
    let mut app = new_app();
    let lines = add_lines(&mut app, 3);
    start_move_with_frame(&mut app, &lines);
    let _ = app.feed_command(StepInput::Text("N".into()));
    let _ = app.update(Message::CommandEscape);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());

    let _ = app.run_command_line("ERASE");
    let _ = app.feed_command(StepInput::Text("L".into()));
    assert!(
        !out(&app).contains("Invalid selection set name."),
        "{}",
        out(&app)
    );
    assert_eq!(selected(&app), lines[2..].to_vec());
}

#[test]
fn the_question_mark_lists_named() {
    use crate::command::StepInput;
    let mut app = new_app();
    let lines = add_lines(&mut app, 3);
    start_move_with_frame(&mut app, &lines);
    let _ = app.feed_command(StepInput::Text("?".into()));
    assert!(out(&app).contains("/Group/Named/"), "{}", out(&app));
}
