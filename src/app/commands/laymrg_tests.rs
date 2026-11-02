use super::*;
use crate::command::StepInput;
use codec::entities::{AttributeEntity, Insert, Line};
use codec::tables::{BlockRecord, Layer};
use codec::{EntityType, Handle, Vector3};

fn fixture() -> (OpenCADStudio, [Handle; 3]) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut handles = [Handle::NULL; 3];
    for (index, name) in ["Source A", "B", "Target T"].into_iter().enumerate() {
        let _ = app.tabs[i].scene.document.layers.add(Layer::new(name));
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.layer = name.into();
        handles[index] = app.tabs[i].scene.add_entity(EntityType::Line(line));
    }
    (app, handles)
}

fn prompt(app: &OpenCADStudio) -> String {
    app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .expect("LAYMRG must stay active")
        .prompt()
}

fn start(app: &mut OpenCADStudio) {
    let _ = app.dispatch_command("LAYMRG");
    app.finish_pending_history(app.active_tab);
}

fn text(app: &mut OpenCADStudio, value: &str) {
    let _ = app.feed_active_cmd(value);
    app.finish_pending_history(app.active_tab);
}

fn pick(app: &mut OpenCADStudio, handle: Handle) {
    let i = app.active_tab;
    // Match the viewport: each click adds to the scene, then sends ALL handles.
    app.tabs[i].scene.select_entity(handle, false);
    let accumulated = app.tabs[i]
        .scene
        .selected_entities()
        .into_iter()
        .map(|(h, _)| h)
        .collect();
    let _ = app.feed_command(StepInput::SelectionComplete(accumulated));
    // The GUI closes first-touch history at the end of each update message.
    app.finish_pending_history(i);
}

fn layer(app: &OpenCADStudio, handle: Handle) -> &str {
    &app.tabs[app.active_tab]
        .scene
        .document
        .get_entity(handle)
        .unwrap()
        .common()
        .layer
}

fn sources_gone(app: &OpenCADStudio) {
    let document = &app.tabs[app.active_tab].scene.document;
    assert!(
        !document.layers.contains("Source A"),
        "first source layer must be removed"
    );
    assert!(
        !document.layers.contains("B"),
        "second source layer must be removed"
    );
    assert!(document.layers.contains("Target T"));
}

#[test]
fn laymrg_accumulated_picks_merge_all_spaces_and_undo_once() {
    let (mut app, [a, b, target]) = fixture();
    let i = app.active_tab;
    // Extra objects on source layers: a definition member, paper space and
    // an insert attribute, which is not a top-level document entity.
    let mut record = BlockRecord::new("DEFINITION");
    record.handle = app.tabs[i].scene.document.allocate_handle();
    let owner = record.handle;
    app.tabs[i]
        .scene
        .document
        .block_records
        .add(record)
        .unwrap();
    let mut member = Line::new();
    member.common.layer = "Source A".into();
    member.common.owner_handle = owner;
    let member = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Line(member))
        .unwrap();
    let mut paper = Line::new();
    paper.common.layer = "B".into();
    paper.common.entity_mode = Some(1);
    let paper = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Line(paper))
        .unwrap();
    let paper_owner = app.tabs[i]
        .scene
        .document
        .get_entity(paper)
        .unwrap()
        .common()
        .owner_handle;
    assert!(
        !paper_owner.is_null(),
        "paper-space fixture must have an owner"
    );
    assert_eq!(
        paper_owner,
        app.tabs[i].scene.document.header.paper_space_block_handle
    );
    let mut insert = Insert::new("DEFINITION", Vector3::ZERO);
    let mut attribute = AttributeEntity::new("TAG".into(), "value".into());
    attribute.common.layer = "B".into();
    insert.attributes.push(attribute);
    let insert = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
    let before = app.tabs[i].history.undo_stack.len();
    start(&mut app);
    pick(&mut app, a);
    assert!(app
        .command_line
        .history
        .iter()
        .any(|entry| entry.text == "Selected layers: Source A."));
    pick(&mut app, b);
    assert!(app
        .command_line
        .history
        .iter()
        .any(|entry| entry.text == "Selected layers: Source A, B."));
    assert_eq!(prompt(&app), "Select object on layer to merge or [Name]:");
    text(&mut app, "");
    assert_eq!(prompt(&app), "Select object on target layer or [Name]:");
    pick(&mut app, target);
    assert_eq!(prompt(&app), "Do you wish to continue? [Yes/No] <No>:");
    assert!(app.command_line.history.iter().any(|entry| entry
        .text
        .contains("******** WARNING ********")
        && entry
            .text
            .contains("You are about to merge 2 layer(s) into layer \"Target T\".")));
    assert_eq!(
        layer(&app, a),
        "Source A",
        "warning must not mutate objects"
    );
    text(&mut app, "Yes");
    sources_gone(&app);
    for handle in [a, b, member, paper] {
        assert_eq!(layer(&app, handle), "Target T");
    }
    let attribute_layer = |app: &OpenCADStudio| {
        let EntityType::Insert(insert) = app.tabs[i].scene.document.get_entity(insert).unwrap()
        else {
            panic!("insert missing")
        };
        insert.attributes[0].common.layer.clone()
    };
    assert_eq!(attribute_layer(&app), "Target T");
    assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
    assert!(app.tabs[i].active_cmd.is_none());
    app.undo_active_tab();
    for handle in [a, member] {
        assert_eq!(layer(&app, handle), "Source A");
    }
    for handle in [b, paper] {
        assert_eq!(layer(&app, handle), "B");
    }
    assert_eq!(attribute_layer(&app), "B");
    assert!(app.tabs[i].scene.document.layers.contains("Source A"));
    assert!(app.tabs[i].scene.document.layers.contains("B"));
    assert_eq!(app.tabs[i].history.undo_stack.len(), before);
}

#[test]
fn laymrg_name_option_accepts_spaces_and_retries_unknown_names() {
    let (mut app, [a, b, _]) = fixture();
    start(&mut app);
    text(&mut app, "Name");
    assert_eq!(prompt(&app), "Enter layer name:");
    assert!(app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .unwrap()
        .input_kind()
        .is_free_text());
    text(&mut app, "missing layer");
    assert_eq!(prompt(&app), "Enter layer name:");
    text(&mut app, "source a");
    text(&mut app, "N");
    text(&mut app, "b");
    text(&mut app, "");
    text(&mut app, "Name");
    text(&mut app, "missing target");
    assert_eq!(prompt(&app), "Enter layer name:");
    text(&mut app, "target t");
    assert_eq!(prompt(&app), "Do you wish to continue? [Yes/No] <No>:");
    text(&mut app, "Y");
    sources_gone(&app);
    assert_eq!(layer(&app, a), "Target T");
    assert_eq!(layer(&app, b), "Target T");
}

#[test]
fn laymrg_refuses_source_target_and_warning_defaults_to_no() {
    for answer in ["No", "", "ESC"] {
        let (mut app, [a, b, target]) = fixture();
        let i = app.active_tab;
        let before = serde_json::to_value(&app.tabs[i].scene.document).unwrap();
        let history = app.tabs[i].history.undo_stack.len();
        let dirty = app.tabs[i].dirty;
        start(&mut app);
        pick(&mut app, a);
        text(&mut app, "");
        pick(&mut app, a);
        assert_eq!(prompt(&app), "Select object on target layer or [Name]:");
        text(&mut app, "Name");
        text(&mut app, "Source A");
        assert_eq!(prompt(&app), "Select object on target layer or [Name]:");
        pick(&mut app, target);
        assert_eq!(prompt(&app), "Do you wish to continue? [Yes/No] <No>:");
        // Picks during the warning cannot replace the chosen target.
        pick(&mut app, b);
        if answer == "ESC" {
            let _ = app.feed_command(StepInput::Escape);
        } else {
            text(&mut app, answer);
        }
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(
            serde_json::to_value(&app.tabs[i].scene.document).unwrap(),
            before
        );
        assert_eq!(app.tabs[i].history.undo_stack.len(), history);
        assert!(app.tabs[i].history.pending.is_none());
        assert_eq!(app.tabs[i].dirty, dirty);
    }
}

#[test]
fn laymrg_preselection_starts_at_target_and_ignores_old_handles() {
    let (mut app, [a, b, target]) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.select_entity(a, false);
    app.tabs[i].scene.select_entity(b, false);
    start(&mut app);
    assert_eq!(prompt(&app), "Select object on target layer or [Name]:");
    pick(&mut app, target);
    assert_eq!(prompt(&app), "Do you wish to continue? [Yes/No] <No>:");
    text(&mut app, "Yes");
    sources_gone(&app);
    assert_eq!(layer(&app, a), "Target T");
    assert_eq!(layer(&app, b), "Target T");
}

#[test]
fn laymrg_typed_form_keeps_immediate_script_merge() {
    let (mut app, [a, b, _]) = fixture();
    let i = app.active_tab;
    let before = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("LAYMRG B 0");
    app.finish_pending_history(i);
    assert_eq!(layer(&app, b), "0", "typed LAYMRG must merge immediately");
    assert_eq!(layer(&app, a), "Source A");
    assert!(!app.tabs[i].scene.document.layers.contains("B"));
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
    app.undo_active_tab();
    assert_eq!(layer(&app, b), "B");
    assert!(app.tabs[i].scene.document.layers.contains("B"));
}

#[test]
fn laymrg_protects_zero_and_current_layer_and_empty_enter_exits() {
    let (mut app, [a, _, target]) = fixture();
    let i = app.active_tab;
    app.tabs[i].active_layer = "B".into();
    start(&mut app);
    for name in ["0", "B"] {
        text(&mut app, "Name");
        text(&mut app, name);
        assert_eq!(prompt(&app), "Select object on layer to merge or [Name]:");
    }
    text(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    start(&mut app);
    pick(&mut app, a);
    text(&mut app, "");
    pick(&mut app, target);
    text(&mut app, "Yes");
    assert!(app.tabs[i].scene.document.layers.contains("0"));
    assert!(app.tabs[i].scene.document.layers.contains("B"));
    assert!(!app.tabs[i].scene.document.layers.contains("Source A"));
}
