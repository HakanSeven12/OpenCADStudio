//! The undo record guard: it is silent for a change that has an undo step
//! and for a name of the list, and it reports each other change.

use std::collections::HashSet;

use document::entities::Line;
use document::types::Vector3;
use document::EntityType;
use serde_json::json;

use super::undo_record_guard::{
    begin_request, check, command_word, end, full_name, message_name, parse_known, request_name,
    Evidence,
};
use crate::app::{Message, OpenCADStudio};

fn app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app
}

fn names(list: &[&'static str]) -> HashSet<&'static str> {
    list.iter().copied().collect()
}

/// A line from (1, 2, 0) to (7, 5, 0), added with no undo record.
fn add_line_with_no_record(app: &mut OpenCADStudio) {
    let i = app.active_tab;
    let line = Line::from_points(Vector3::new(1.0, 2.0, 0.0), Vector3::new(7.0, 5.0, 0.0));
    app.tabs[i].scene.add_entity(EntityType::Line(line));
}

fn undo_depth(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].history.undo_stack.len()
}

#[test]
fn a_message_name_is_its_variant_and_the_variant_of_its_first_field() {
    assert_eq!(message_name("LayoutSwitch"), "LayoutSwitch");
    assert_eq!(message_name("Linetype(SetCurrent)"), "Linetype(SetCurrent)");
    assert_eq!(
        message_name("PropColorChanged(Index(9))"),
        "PropColorChanged(Index)"
    );
    // A field that is a number or a text is not a part of the name.
    assert_eq!(
        message_name("PanePressAt(0, Point { x: 1.5 })"),
        "PanePressAt"
    );
    assert_eq!(
        message_name("CommandOptionPick(\"C\")"),
        "CommandOptionPick"
    );
    assert_eq!(message_name("ScriptLine(\"tilemode 0\")"), "ScriptLine");
    assert_eq!(
        message_name("SetModifiers { shift: false, ctrl: true }"),
        "SetModifiers"
    );
    // The text can stop in the middle of a field.
    assert_eq!(message_name("FileOpened(Ok(Docu"), "FileOpened(Ok)");
    assert_eq!(message_name("FileOpened(Ok"), "FileOpened(Ok)");
}

#[test]
fn a_command_line_message_gives_the_command_that_it_starts() {
    assert_eq!(
        command_word("ScriptLine(\"tilemode 0\")").as_deref(),
        Some("TILEMODE")
    );
    assert_eq!(
        command_word("CommandInput(\"-LAYER NEW Walls\")").as_deref(),
        Some("-LAYER")
    );
    // Only the two command line messages: the text of another one is data.
    assert_eq!(command_word("LayoutSwitch(\"Layout1\")"), None);
    assert_eq!(command_word("CommandOptionPick(\"C\")"), None);
    // A text that does not start with a word, and a message with no text.
    assert_eq!(command_word("CommandInput(\"\")"), None);
    assert_eq!(command_word("ScriptLine(\"  \")"), None);
    assert_eq!(command_word("LayoutSwitch"), None);
}

#[test]
fn the_command_is_a_part_of_the_name() {
    let text = |value: &str| Some(value.to_string());
    assert_eq!(
        full_name("LayoutSwitch".to_string(), None, None),
        "LayoutSwitch"
    );
    assert_eq!(
        full_name("ScriptLine".to_string(), None, text("TILEMODE")),
        "ScriptLine TILEMODE"
    );
    assert_eq!(
        full_name("CommandEscape".to_string(), text("BACTION"), None),
        "CommandEscape during BACTION"
    );
    // In a command, a command line is an answer, not a new command.
    assert_eq!(
        full_name("ScriptLine".to_string(), text("LINETYPE"), text("FENCE")),
        "ScriptLine during LINETYPE"
    );
}

#[test]
fn a_request_name_has_its_kind_its_operation_and_the_command_of_a_run() {
    assert_eq!(
        request_name("automation", &json!({"op": "save"})),
        "automation:save"
    );
    assert_eq!(
        request_name("automation", &json!({"op": "run", "cmd": "tilemode 0"})),
        "automation:run TILEMODE"
    );
    assert_eq!(
        request_name(
            "control",
            &json!({"protocol": 1, "op": "layout_activate", "name": "A"})
        ),
        "control:layout_activate"
    );
    // A `cmd` field names the command only for `run`.
    assert_eq!(
        request_name("control", &json!({"op": "input", "cmd": "MOVE"})),
        "control:input"
    );
    assert_eq!(request_name("control", &json!({})), "control:?");
    assert_eq!(
        request_name("automation", &json!({"op": "run"})),
        "automation:run"
    );
}

#[test]
fn only_a_change_with_no_record_and_no_history_step_is_reported() {
    let evidence = |changed, record_open, history_moved| Evidence {
        changed,
        record_open,
        history_moved,
    };
    assert!(evidence(true, false, false).has_no_undo_record());
    assert!(!evidence(false, false, false).has_no_undo_record());
    assert!(!evidence(true, true, false).has_no_undo_record());
    assert!(!evidence(true, false, true).has_no_undo_record());
    assert!(!evidence(true, true, true).has_no_undo_record());
}

#[test]
fn the_list_file_has_names_comments_and_empty_lines() {
    let list = parse_known("# reason\nLayoutSwitch\n\n  automation:run TILEMODE  \n# end\n");
    assert_eq!(list, names(&["LayoutSwitch", "automation:run TILEMODE"]));
}

#[test]
fn the_list_has_each_name_one_time() {
    let text = include_str!("undo_record_guard_known.txt");
    let mut seen = HashSet::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        assert!(seen.insert(line), "`{line}` is in the list two times");
    }
    assert_eq!(seen, parse_known(text));
}

#[test]
fn a_command_that_makes_an_undo_step_passes() {
    let mut app = app();
    // `update` holds the guard: a report would stop this test here.
    let _ = app.update_root(Message::ScriptLine("CIRCLE 3,4 5".to_string()));
    assert_eq!(undo_depth(&app), 1, "the circle is one undo step");
}

#[test]
fn a_change_inside_an_undo_record_is_not_reported() {
    let mut app = app();
    let frame = begin_request(&app, "test", &json!({"op": "recorded"}));
    let i = app.active_tab;
    app.push_undo_snapshot(i, "TEST");
    add_line_with_no_record(&mut app);
    app.finish_all_pending_history();
    assert_eq!(undo_depth(&app), 1);
    assert_eq!(check(&mut app, frame, &names(&[]), None), None);
}

#[test]
fn a_record_that_stays_open_is_not_reported() {
    let mut app = app();
    let frame = begin_request(&app, "test", &json!({"op": "open_record"}));
    let i = app.active_tab;
    app.push_undo_snapshot(i, "TEST");
    add_line_with_no_record(&mut app);
    // The message boundary did not close it yet: it will have the change.
    assert_eq!(check(&mut app, frame, &names(&[]), None), None);
    app.finish_all_pending_history();
}

#[test]
fn an_added_entity_with_no_undo_record_is_reported_with_its_name() {
    let mut app = app();
    let frame = begin_request(&app, "test", &json!({"op": "add_line"}));
    add_line_with_no_record(&mut app);
    let report = check(&mut app, frame, &names(&[]), None).expect("a report");
    assert!(
        report.contains("`test:add_line` changed a drawing"),
        "{report}"
    );
    assert!(
        report.contains("1 entities added, 0 modified, 0 removed"),
        "{report}"
    );
    assert!(report.contains("undo_record_guard_known.txt"), "{report}");
    assert_eq!(undo_depth(&app), 0);
}

#[test]
fn a_changed_header_with_no_undo_record_is_reported() {
    let mut app = app();
    let frame = begin_request(&app, "test", &json!({"op": "header"}));
    let i = app.active_tab;
    app.tabs[i].scene.document.header.handle_seed += 3;
    let report = check(&mut app, frame, &names(&[]), None).expect("a report");
    assert!(
        report.contains("0 entities added, 0 modified, 0 removed; changed: header"),
        "{report}"
    );
}

#[test]
fn a_modified_and_a_removed_entity_with_no_undo_record_are_reported() {
    let mut app = app();
    add_line_with_no_record(&mut app);
    add_line_with_no_record(&mut app);
    let i = app.active_tab;
    let handles: Vec<_> = app.tabs[i]
        .scene
        .document
        .entities()
        .map(|entity| entity.common().handle)
        .collect();
    let frame = begin_request(&app, "test", &json!({"op": "edit"}));
    let first = app.tabs[i]
        .scene
        .document
        .get_entity_mut(handles[0])
        .expect("the first line");
    first.common_mut().layer = "Walls".to_string();
    app.tabs[i].scene.erase_entities(&handles[1..]);
    let report = check(&mut app, frame, &names(&[]), None).expect("a report");
    assert!(
        report.contains("0 entities added, 1 modified, 1 removed"),
        "{report}"
    );
}

#[test]
#[should_panic(expected = "changed a drawing and made no undo step")]
fn the_guard_fails_the_test_for_a_change_with_no_undo_record() {
    // In list mode the guard fails no test: keep this test as it is there.
    if std::env::var_os("OCS_UNDO_GUARD_LIST").is_some() {
        panic!("list mode: changed a drawing and made no undo step");
    }
    let mut app = app();
    let frame = begin_request(&app, "test", &json!({"op": "not_in_the_list"}));
    add_line_with_no_record(&mut app);
    end(&mut app, frame);
}

#[test]
fn a_handler_that_ended_leaves_no_depth_and_no_answer_behind() {
    let mut app = app();
    let list = names(&["test:listed"]);
    let outer = begin_request(&app, "test", &json!({"op": "outer"}));
    let listed = begin_request(&app, "test", &json!({"op": "listed"}));
    assert_eq!((outer.depth(), listed.depth()), (0, 1));
    add_line_with_no_record(&mut app);
    assert_eq!(check(&mut app, listed, &list, None), None);
    // The outer handler stops (a panic) before it looks at the change.
    drop(outer);
    let next = begin_request(&app, "test", &json!({"op": "next"}));
    assert_eq!(next.depth(), 0);
    add_line_with_no_record(&mut app);
    assert!(check(&mut app, next, &list, None).is_some());
}

#[test]
fn a_handler_that_changes_nothing_is_not_reported() {
    let mut app = app();
    let frame = begin_request(&app, "test", &json!({"op": "nothing"}));
    assert_eq!(check(&mut app, frame, &names(&[]), None), None);
}

#[test]
fn a_name_of_the_list_is_not_reported() {
    let mut app = app();
    let frame = begin_request(&app, "test", &json!({"op": "add_line"}));
    add_line_with_no_record(&mut app);
    assert_eq!(
        check(&mut app, frame, &names(&["test:add_line"]), None),
        None
    );
}

#[test]
fn the_handler_around_a_listed_handler_does_not_report_the_same_change() {
    let mut app = app();
    let list = names(&["test:inner"]);
    let outer = begin_request(&app, "test", &json!({"op": "outer"}));
    let middle = begin_request(&app, "test", &json!({"op": "middle"}));
    let inner = begin_request(&app, "test", &json!({"op": "inner"}));
    add_line_with_no_record(&mut app);
    assert_eq!(check(&mut app, inner, &list, None), None);
    assert_eq!(check(&mut app, middle, &list, None), None);
    assert_eq!(check(&mut app, outer, &list, None), None);
    // The next handler starts with nothing answered.
    let next = begin_request(&app, "test", &json!({"op": "next"}));
    add_line_with_no_record(&mut app);
    assert!(check(&mut app, next, &list, None).is_some());
}

#[test]
fn a_handler_after_a_listed_handler_still_reports_its_own_change() {
    let mut app = app();
    let list = names(&["test:listed"]);
    let outer = begin_request(&app, "test", &json!({"op": "outer"}));
    let listed = begin_request(&app, "test", &json!({"op": "listed"}));
    add_line_with_no_record(&mut app);
    assert_eq!(check(&mut app, listed, &list, None), None);
    // A second handler at the same depth is not covered by the first.
    let second = begin_request(&app, "test", &json!({"op": "second"}));
    add_line_with_no_record(&mut app);
    let report = check(&mut app, second, &list, None).expect("a report");
    assert!(report.contains("`test:second`"), "{report}");
    drop(outer);
}

#[test]
fn list_mode_writes_the_name_and_reports_nothing() {
    let mut app = app();
    let dir = tempfile::tempdir().expect("a temporary folder");
    let file = dir.path().join("names.txt");
    for _ in 0..2 {
        let frame = begin_request(&app, "test", &json!({"op": "add_line"}));
        add_line_with_no_record(&mut app);
        assert_eq!(check(&mut app, frame, &names(&[]), Some(&file)), None);
    }
    let written = std::fs::read_to_string(&file).expect("the list file");
    assert_eq!(written, "test:add_line\ntest:add_line\n");
}
