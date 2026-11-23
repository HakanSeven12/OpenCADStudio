use super::block_editor::block_reference_count;
use super::{Message, ModalKind, OpenCADStudio};
use document::entities::{Insert, Line};
use document::types::Vector3;
use document::{EntityType, Handle};

mod construction_visibility;
mod dxf_reference_integrity;
mod flip_frame_grips;
mod flip_state_labels;
mod grip_descriptions;
mod lookup_properties;
mod nested_properties;
mod properties_internal_rows;
mod retained_save_as;
mod synthetic_reference_grips;
mod table_properties;
mod visibility_properties;
mod window_scale;

fn fixture() -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "EDIT_ME",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("EDIT_ME", Vector3::ZERO)))
        .unwrap();
    app.tabs[i].dirty = false;
    (app, reference)
}

fn begin(app: &mut OpenCADStudio, reference: Handle) {
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
    assert!(app.tabs[app.active_tab].active_block_edit.is_some());
}

fn circle(app: &mut OpenCADStudio, x: f64) {
    let result = app.automation_op(&format!(r#"{{"op":"run","cmd":"CIRCLE {x},0 2"}}"#));
    assert_eq!(result["ok"], true, "{result}");
    app.finish_pending_history(app.active_tab);
}

fn members(app: &OpenCADStudio) -> Vec<Handle> {
    app.tabs[app.active_tab]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .entity_handles
        .clone()
}

fn assert_block_ends_after_its_body(app: &OpenCADStudio, name: &str) {
    let document = &app.tabs[app.active_tab].scene.document;
    let record = document
        .block_records
        .get(name)
        .expect("the record is added");
    let marker = record.block_entity_handle.value();
    let end = record.block_end_handle.value();
    let body: Vec<_> = record
        .entity_handles
        .iter()
        .map(|handle| handle.value())
        .collect();
    assert!(
        body.iter().all(|handle| *handle > marker),
        "body handles follow the BLOCK marker: {body:?}"
    );
    assert!(
        end > *body.iter().max().expect("the body is not empty"),
        "the ENDBLK handle follows the body: marker {marker}, body {body:?}, end {end}"
    );
    assert!(
        matches!(
            document.get_entity(Handle::new(end)),
            Some(EntityType::BlockEnd(_))
        ),
        "the record's end handle names an ENDBLK entity"
    );
}

fn main_depth(app: &OpenCADStudio) -> usize {
    let tab = &app.tabs[app.active_tab];
    tab.block_edit_history
        .as_ref()
        .map(|h| h.main.undo_stack.len())
        .unwrap_or(tab.history.undo_stack.len())
}

fn feed(app: &mut OpenCADStudio, text: &str) {
    let _ = app.update(Message::CommandInput(text.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

/// Author one attribute definition through the command line, the way an
/// operator in the block editor does: modes, tag, prompt, default, then the
/// place, height and rotation it is put at. The command keeps its modes in
/// process-wide state and its last placed definition per drawing, so the run
/// starts from the defaults a fresh drawing has and puts that state back as it
/// found it; otherwise whichever test ran last would write the prompts this
/// one answers.
fn author_attribute(app: &mut OpenCADStudio, tag: &str) {
    use crate::modules::draw::draw::attdef::{session, AttdefSession, AFLAG_LOCK};
    let remembered = {
        let mut live = session();
        let remembered = AttdefSession {
            aflags: live.aflags,
            annotative: live.annotative,
            last: live.last.clone(),
            on_screen: live.on_screen,
            editor_value: live.editor_value.clone(),
        };
        *live = AttdefSession {
            aflags: AFLAG_LOCK,
            annotative: false,
            last: Vec::new(),
            on_screen: true,
            editor_value: None,
        };
        remembered
    };
    let i = app.active_tab;
    let _ = app.dispatch_command("-ATTDEF");
    feed(app, ""); // modes: done
    feed(app, tag);
    feed(app, "prompt text");
    feed(app, ""); // default value
    feed(app, ""); // single line
    feed(app, "0,5");
    feed(app, "2");
    feed(app, "0");
    app.finish_pending_history(i);
    *session() = remembered;
}

fn definition_is_attributed(app: &OpenCADStudio) -> Option<bool> {
    app.tabs[app.active_tab]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .map(|record| record.flags.has_attributes)
}

/// What the written file says about the edited definition, read back from it.
fn written_attribute_marker(app: &OpenCADStudio, extension: &str) -> Option<bool> {
    let document = &app.tabs[app.active_tab].scene.document;
    let bytes = crate::io::save_to_bytes(document, extension, document.version)
        .unwrap_or_else(|error| panic!("save {extension}: {error}"));
    let reloaded = crate::io::load_bytes(&format!("editor.{extension}"), bytes)
        .unwrap_or_else(|error| panic!("load {extension}: {error}"));
    reloaded
        .block_records
        .get("EDIT_ME")
        .map(|record| record.flags.has_attributes)
}

#[test]
fn an_attribute_authored_in_the_editor_marks_the_definition_attributed() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    assert_eq!(
        definition_is_attributed(&app),
        Some(false),
        "a definition with no attribute says so"
    );
    author_attribute(&mut app, "TAG1");
    assert_eq!(
        definition_is_attributed(&app),
        Some(true),
        "the record advertises the attribute its body now holds"
    );
    let _ = app.dispatch_command("BSAVE");
    assert_eq!(
        definition_is_attributed(&app),
        Some(true),
        "saving keeps the definition attributed"
    );
    for extension in ["dxf", "dwg"] {
        assert_eq!(
            written_attribute_marker(&app, extension),
            Some(true),
            "{extension}: the written record does not advertise the authored attribute"
        );
    }
    let _ = app.dispatch_command("BCLOSE");
}

#[test]
fn saving_after_the_authored_attribute_is_erased_unmarks_the_definition() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    begin(&mut app, reference);
    author_attribute(&mut app, "TAG1");
    assert_eq!(
        definition_is_attributed(&app),
        Some(true),
        "the attribute is on the record before it is erased"
    );
    let attribute = *members(&app).last().unwrap();
    app.push_undo_snapshot(i, "Erase attribute");
    app.tabs[i].scene.erase_entities(&[attribute]);
    app.finish_pending_history(i);
    let _ = app.dispatch_command("BSAVE");
    assert_eq!(
        definition_is_attributed(&app),
        Some(false),
        "the body holds no attribute, so neither does the record"
    );
    for extension in ["dxf", "dwg"] {
        assert_eq!(
            written_attribute_marker(&app, extension),
            Some(false),
            "{extension}: the written record still advertises an attribute the body lost"
        );
    }
    let _ = app.dispatch_command("BCLOSE");
}

fn edit_action_set(app: &mut OpenCADStudio, action: Handle, mode: &str, targets: Vec<Handle>) {
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    app.tabs[app.active_tab].selected_authoring = Some(action);
    let _ = app.dispatch_command("BACTIONSET");
    feed(app, mode);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
}

mod constraints_and_save;

mod block_actions_save;

#[test]
fn block_parameters_rotation_only_save_close_reopen_then_flip() {
    use crate::scene::block_actions::{self as actions, Kind};
    for save_command in ["BEDIT_SAVE", "BSAVE"] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let _ = app.run_command_line("BPARAMETER ROTATION 0,0 10 0 5,5");
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Rotate).len(),
            1
        );
        let _ = app.dispatch_command(save_command);
        if save_command == "BSAVE" {
            let _ = app.dispatch_command("BCLOSE");
        }
        assert!(app.tabs[i].active_block_edit.is_none());
        begin(&mut app, reference);
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Rotate).len(),
            1,
            "{save_command}"
        );
        let _ = app.run_command_line("BPARAMETER FLIP 0,0 10,0 -2,2");
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Flip).len(),
            1
        );
        let _ = app.dispatch_command("BEDIT_SAVE");
        begin(&mut app, reference);
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Rotate).len(),
            1
        );
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Flip).len(),
            1
        );
    }
}

#[test]
fn block_actions_save_close_and_reedit_evaluated_reference() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].clone();
    let targets = members(&app);
    actions::create(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Move,
        p.handle,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    assert!(app.tabs[i].active_block_edit.is_none());
    assert_eq!(
        actions::parameters(&app.tabs[i].scene.document, def, Kind::Move).len(),
        1
    );
    assert_eq!(actions::actions(&app.tabs[i].scene.document, def).len(), 1);
    app.tabs[i].scene.selected.insert(reference);
    app.refresh_properties();
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| super::block_actions::is_test_grip(g.id))
        .unwrap()
        .id;
    assert!(app.start_test_action_grip(reference, grip));
    feed(&mut app, "12,0");
    let EntityType::Insert(insert) = app.tabs[i].scene.document.get_entity(reference).unwrap()
    else {
        panic!()
    };
    assert_ne!(insert.block_name, "EDIT_ME");
    // Native save/reload must preserve the link from the evaluated variant to
    // the definition that owns the parameters and actions.
    let bytes = document::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    app.tabs[i].scene.document = document::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    app.tabs[i].scene.selected.insert(reference);
    let _ = app.dispatch_command("BEDIT");
    assert_eq!(
        app.tabs[i].active_block_edit_session().unwrap().br_handle,
        def
    );
    assert_eq!(
        actions::parameters(&app.tabs[i].scene.document, def, Kind::Move).len(),
        1
    );
    assert_eq!(actions::actions(&app.tabs[i].scene.document, def).len(), 1);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    app.tabs[i].scene.selected.insert(reference);
    app.refresh_properties();
    assert!(app.tabs[i]
        .selected_grips
        .iter()
        .any(|g| super::block_actions::is_test_grip(g.id)));
}

mod authoring_grips;

#[test]
fn block_actions_mouse_selection_waits_for_enter_for_move_and_scale() {
    use crate::scene::block_actions::{self as actions, Kind};
    for (kind, command) in [(Kind::Move, "BACTION MOVE"), (Kind::Scale, "BACTION SCALE")] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let _ = app.run_command_line("CIRCLE 0.5,0.5 0.2");
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 2,0 1,-0.2");
        let definition = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        app.tabs[source].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
        app.tabs[source]
            .scene
            .camera
            .borrow_mut()
            .fit_to_bounds_f64(
                glam::DVec3::new(-0.5, -0.5, 0.),
                glam::DVec3::new(2.5, 1.5, 0.),
                1600. / 900.,
            );
        let _ = app.dispatch_command(command);
        let click = |app: &mut OpenCADStudio, point: [f64; 3], modifiers: &str| {
            let scene = &app.tabs[app.active_tab].scene;
            let (vw, vh) = scene.selection.borrow().view.vp_size;
            let tile = scene.active_model_tile_bounds(vw, vh);
            let screen = scene
                .camera
                .borrow()
                .project(
                    glam::DVec3::from_array(point),
                    iced::Rectangle {
                        x: 0.,
                        y: 0.,
                        width: tile.width,
                        height: tile.height,
                    },
                )
                .unwrap();
            let cursor = iced::Point::new(screen.x + tile.x, screen.y + tile.y);
            let previous = (app.ctrl_down, app.shift_down);
            app.ctrl_down = modifiers.contains("ctrl");
            app.shift_down = modifiers.contains("shift");
            let _ = app.update(Message::ViewportLeftPressAt(cursor));
            let _ = app.update(Message::ViewportLeftRelease);
            (app.ctrl_down, app.shift_down) = previous;
        };
        click(&mut app, [2., 0., 0.], "");
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        feed(&mut app, ""); // Empty selection does not cancel or advance.
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        for (point, modifiers, count) in [
            ([0.5, 0., 0.], "", 1),
            ([0.5, 0.7, 0.], "", 2),
            ([0.5, 0., 0.], "shift", 1),
            ([0.5, 0., 0.], "", 2),
        ] {
            click(&mut app, point, modifiers);
            assert!(
                app.tabs[source]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .is_selection_gathering(),
                "every mouse pick/removal stays in object selection"
            );
            assert_eq!(app.tabs[source].scene.selected.len(), count);
            assert!(actions::actions(&app.tabs[source].scene.document, definition).is_empty());
        }
        feed(&mut app, "");
        assert!(!app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        click(&mut app, [1.5, 1., 0.], "");
        assert!(app.tabs[source].active_cmd.is_none());
        let authored = actions::actions(&app.tabs[source].scene.document, definition);
        assert_eq!(authored.len(), 1);
        assert_eq!(authored[0].2, kind);
        assert_eq!(authored[0].1.dependencies.len(), 2);
        let original = app.tabs[source].scene.document.clone();
        let _ = app.dispatch_command("BTESTBLOCK");
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-0.5, -0.5, 0.),
            glam::DVec3::new(4.5, 1.5, 0.),
            1600. / 900.,
        );
        click(&mut app, [0.5, 0., 0.], "");
        click(&mut app, [2., 0., 0.], "");
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().map(|c| c.name()),
            Some("BTESTBLOCK")
        );
        click(&mut app, [4., 0., 0.], "");
        assert!(app.tabs[i].active_cmd.is_none());
        let circle = app.tabs[i]
            .scene
            .document
            .entities_in_block("EDIT_ME")
            .find_map(|e| {
                if let EntityType::Circle(c) = e {
                    Some(c)
                } else {
                    None
                }
            })
            .unwrap();
        let (center, radius) = if kind == Kind::Move {
            (Vector3::new(2.5, 0.5, 0.), 0.2)
        } else {
            (Vector3::new(1., 1., 0.), 0.4)
        };
        // Mouse projection uses f32 screen coordinates; allow sub-pixel roundoff.
        assert!(
            (circle.center - center).length() < 1e-5,
            "{kind:?}: actual {:?}, expected {center:?}",
            circle.center
        );
        assert!((circle.radius - radius).abs() < 1e-5);
        assert_eq!(app.tabs[source].scene.document, original);
    }
}

#[test]
fn block_actions_command_selection_cancel_undo_and_test_block_isolation() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let p =
        actions::parameters(&app.tabs[source].scene.document, definition, Kind::Move)[0].clone();
    let targets = members(&app);
    let before = app.tabs[source].scene.document.clone();
    let depth = app.tabs[source].history.undo_stack.len();
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &p.name);
    assert!(app.tabs[source]
        .active_cmd
        .as_ref()
        .unwrap()
        .is_selection_gathering());
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
        targets.clone(),
    ));
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[source].scene.document, before);
    assert_eq!(app.tabs[source].history.undo_stack.len(), depth);
    for kind in ["Move", "Scale"] {
        let _ = app.dispatch_command(&format!("BACTION {kind}"));
        feed(&mut app, &p.name);
        let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
            targets.clone(),
        ));
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        let _ = app.feed_command(crate::command::StepInput::Enter);
        feed(&mut app, "3,4");
        assert!(app.tabs[source].active_cmd.is_none());
    }
    assert_eq!(
        actions::actions(&app.tabs[source].scene.document, definition).len(),
        2
    );
    assert_eq!(app.tabs[source].history.undo_stack.len(), depth + 2);
    app.undo_steps(1);
    assert_eq!(
        actions::actions(&app.tabs[source].scene.document, definition).len(),
        1
    );
    app.redo_steps(1);
    assert_eq!(
        actions::actions(&app.tabs[source].scene.document, definition).len(),
        2
    );
    let original = app.tabs[source].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    let test = app.active_tab;
    let insert = app.tabs[test].block_test.as_ref().unwrap().insert;
    app.tabs[test].selected_handle = Some(insert);
    app.refresh_test_action_grips([0.; 3]);
    let grip = app.tabs[test].selected_grips.last().unwrap().id;
    assert!(app.start_test_action_grip(insert, grip));
    feed(&mut app, "20,5");
    let EntityType::Line(line) = app.tabs[test]
        .scene
        .document
        .get_entity(targets[0])
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(line.start, Vector3::new(20., 0., 0.));
    assert_eq!(line.end, Vector3::new(22., 0., 0.));
    assert_eq!(app.tabs[source].scene.document, original);
    app.undo_steps(1);
    let EntityType::Line(line) = app.tabs[test]
        .scene
        .document
        .get_entity(targets[0])
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(line.start, Vector3::ZERO);
    app.redo_steps(1);
    let EntityType::Line(line) = app.tabs[test]
        .scene
        .document
        .get_entity(targets[0])
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(line.start, Vector3::new(20., 0., 0.));
    app.tabs[test]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::new(50., 0., 0.),
        )))
        .unwrap();
    let shared_before = app.tabs[test].scene.document.clone();
    let id = app.tabs[test].id;
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
        grip: None,
        insert: None,
        document: id,
        definition,
        parameter: p.handle,
        value: crate::scene::block_actions::Value::Point(glam::DVec3::new(30., 0., 0.)),
    });
    assert_eq!(
        app.tabs[test].scene.document, shared_before,
        "shared references are refused rather than silently modified together"
    );
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.active_tab, source);
    assert_eq!(app.tabs[source].scene.document, original);
}

#[test]
fn stretch_pair_sets_guide_both_frames_with_undo_and_cancel() {
    use crate::command::StepInput;
    use crate::scene::block_actions::{self as actions, Kind};
    for label in [
        "Linear Stretch Pair",
        "Polar Stretch Pair",
        "Linear Move Pair",
    ] {
        let kind = if label.contains("Stretch") {
            Kind::Stretch
        } else {
            Kind::Move
        };
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let targets = members(&app);
        let _ = app.dispatch_command(&format!("BPARAMETERSET {label}"));
        for point in [
            glam::DVec3::ZERO,
            glam::DVec3::new(10., 0., 0.),
            glam::DVec3::new(5., 2., 0.),
        ] {
            let _ = app.feed_command(StepInput::Point(point));
        }
        if label.starts_with("Linear") {
            let _ = app.feed_command(StepInput::Enter);
        }
        for (side, expected) in [
            (0, ["BaseXDelta", "BaseYDelta"]),
            (1, ["EndXDelta", "EndYDelta"]),
        ] {
            if kind == Kind::Stretch {
                assert!(app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .prompt()
                    .contains(if side == 0 { "base" } else { "endpoint" }));
                for point in [
                    glam::DVec3::new(side as f64 - 0.25, -1., 0.),
                    glam::DVec3::new(side as f64 + 0.25, 1., 0.),
                ] {
                    let _ = app.feed_command(StepInput::Point(point));
                }
            }
            assert!(app.tabs[i]
                .active_cmd
                .as_ref()
                .unwrap()
                .is_selection_gathering());
            let _ = app.feed_command(StepInput::SelectionComplete(targets.clone()));
            let _ = app.feed_command(StepInput::Enter);
            let _ = app.feed_command(StepInput::Point(glam::DVec3::new(3., 4., 0.)));
            let authored = actions::actions(&app.tabs[i].scene.document, definition);
            assert_eq!(authored.len(), side + 1);
            assert_eq!(authored[side].2, kind);
            let document::objects::ObjectType::DynamicBlock(object) =
                &app.tabs[i].scene.document.objects[&authored[side].0]
            else {
                panic!()
            };
            let connections = match &object.data {
                document::objects::DynamicBlockData::StretchAction(a) => &a.connections,
                document::objects::DynamicBlockData::MoveAction(a) => &a.connections,
                _ => panic!(),
            };
            assert_eq!(
                [connections[0].name.as_str(), connections[1].name.as_str()],
                expected
            );
        }
        assert!(app.tabs[i].pending_action_pair.is_none());
        assert!(app.tabs[i].active_cmd.is_none());
        app.undo_steps(1);
        assert_eq!(
            actions::actions(&app.tabs[i].scene.document, definition).len(),
            1
        );
        app.redo_steps(1);
        assert_eq!(
            actions::actions(&app.tabs[i].scene.document, definition).len(),
            2
        );

        let before = app.tabs[i].scene.document.clone();
        let _ = app.dispatch_command(&format!("BPARAMETERSET {label}"));
        for point in [
            glam::DVec3::ZERO,
            glam::DVec3::new(10., 0., 0.),
            glam::DVec3::new(5., 2., 0.),
        ] {
            let _ = app.feed_command(StepInput::Point(point));
        }
        let _ = app.update(Message::CommandEscape);
        assert!(app.tabs[i].pending_action_pair.is_none());
        assert_eq!(app.tabs[i].scene.document, before);

        let _ = app.dispatch_command(&format!("BPARAMETERSET {label}"));
        for point in [
            glam::DVec3::ZERO,
            glam::DVec3::new(10., 0., 0.),
            glam::DVec3::new(5., 2., 0.),
        ] {
            let _ = app.feed_command(StepInput::Point(point));
        }
        if label.starts_with("Linear") {
            let _ = app.feed_command(StepInput::Enter);
        }
        assert!(app.tabs[i].pending_action_pair.is_some());
        let _ = app.dispatch_command("LINE");
        assert!(app.tabs[i].pending_action_pair.is_none());
        assert!(app.tabs[i].pending_parameter_set_follow_up.is_none());
        let _ = app.update(Message::CommandEscape);
    }
}

#[test]
fn cancelling_a_parameter_set_takes_the_parameter_with_it() {
    use crate::command::StepInput;
    use crate::scene::block_distance_parameter as distance;
    for label in [
        "Linear Move Pair",
        "Polar Stretch Pair",
        "XY Move Box Set",
        "XY Stretch Box Set",
    ] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let before = app.tabs[i].scene.document.clone();

        let box_set = label.contains("Box Set");
        let _ = app.dispatch_command(&format!("BPARAMETERSET {label}"));
        let end = glam::DVec3::new(10., if box_set { 10. } else { 0. }, 0.);
        let _ = app.feed_command(StepInput::Point(glam::DVec3::ZERO));
        let _ = app.feed_command(StepInput::Point(end));
        if box_set {
            let _ = app.feed_command(StepInput::Enter);
        }
        let _ = app.feed_command(StepInput::Point(glam::DVec3::new(5., 2., 0.)));
        if label.starts_with("Linear") {
            let _ = app.feed_command(StepInput::Enter);
        }
        assert!(
            !distance::parameters(&app.tabs[i].scene.document, definition).is_empty(),
            "{label}: the parameter is placed before the action step"
        );

        let _ = app.update(Message::CommandEscape);

        assert!(
            distance::parameters(&app.tabs[i].scene.document, definition).is_empty(),
            "{label}: cancelling the set removes the parameter it placed"
        );
        assert_eq!(
            app.tabs[i].scene.document, before,
            "{label}: the definition is back where the set started"
        );
    }
}

#[test]
fn xy_box_set_guides_four_separate_action_selections() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::scene::block_distance_parameter as distance;
    for kind in [Kind::Move, Kind::Stretch] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let parameter = distance::create(
            &mut app.tabs[i].scene.document,
            definition,
            distance::Kind::XY,
            Vector3::ZERO,
            Vector3::new(10., 10., 0.),
            Vector3::new(5., 5., 0.),
        )
        .unwrap();
        app.tabs[i].pending_parameter_set_follow_up = Some(if kind == Kind::Move {
            "XY_MOVE_BOX_SET"
        } else {
            "XY_STRETCH_BOX_SET"
        });
        let _ = app.finish_parameter_set_placement(i, parameter);
        let targets = members(&app);
        for (key_point, expected_names) in [
            ("base", ["BaseXDelta", "BaseYDelta"]),
            ("X-axis", ["EndXDelta", "BaseYDelta"]),
            ("Y-axis", ["BaseXDelta", "EndYDelta"]),
            ("end", ["EndXDelta", "EndYDelta"]),
        ] {
            if kind == Kind::Stretch {
                assert!(app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .prompt()
                    .contains(key_point));
                for point in [
                    glam::DVec3::new(-0.25, -1., 0.),
                    glam::DVec3::new(0.25, 1., 0.),
                ] {
                    let _ = app.feed_command(crate::command::StepInput::Point(point));
                }
            }
            assert!(app.tabs[i]
                .active_cmd
                .as_ref()
                .unwrap()
                .prompt()
                .contains(key_point));
            let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
                targets.clone(),
            ));
            let _ = app.feed_command(crate::command::StepInput::Enter);
            let _ = app.feed_command(crate::command::StepInput::Point(glam::DVec3::ZERO));
            let authored = actions::actions(&app.tabs[i].scene.document, definition);
            let action = authored.last().unwrap();
            let document::objects::ObjectType::DynamicBlock(object) =
                &app.tabs[i].scene.document.objects[&action.0]
            else {
                panic!()
            };
            let connections = match &object.data {
                document::objects::DynamicBlockData::MoveAction(a) => &a.connections,
                document::objects::DynamicBlockData::StretchAction(a) => &a.connections,
                _ => panic!(),
            };
            assert_eq!(
                [connections[0].name.as_str(), connections[1].name.as_str(),],
                expected_names
            );
        }
        assert_eq!(
            actions::actions(&app.tabs[i].scene.document, definition).len(),
            4
        );
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move).len(),
            1
        );
        assert!(app.tabs[i].pending_xy_box_set.is_none());
    }
}

#[test]
fn block_reset_selection_cancel_and_undo_preserve_reference_isolation() {
    use crate::command::StepInput;
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let parameter =
        actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    let targets = members(&app);
    actions::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Move,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    assert!(app.save_block_edit(i));
    app.close_block_edit(i, false);
    app.tabs[i].scene.document = actions::instance::evaluate(
        &app.tabs[i].scene.document,
        reference,
        parameter,
        glam::DVec3::new(20., 0., 0.),
    )
    .unwrap()
    .unwrap();
    app.tabs[i].scene.deselect_all();
    let changed = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("RESETBLOCK");
    let _ = app.feed_command(StepInput::SelectionComplete(vec![reference]));
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, changed);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.dispatch_command("RESETBLOCK");
    let _ = app.feed_command(StepInput::SelectionComplete(vec![reference]));
    let _ = app.feed_command(StepInput::Enter);
    let reset = app.tabs[i].scene.document.clone();
    assert_ne!(reset, changed);
    let p = actions::parameters(&reset, definition, Kind::Move)[0].clone();
    assert_eq!(
        actions::instance::current(&reset, reference, &p).unwrap(),
        p.end
    );
    assert_eq!(reset.get_entity(targets[0]), changed.get_entity(targets[0]));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, changed);
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document, reset);
    app.tabs[i].scene.select_entity(reference, true);
    let _ = app.dispatch_command("RESETBLOCK");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
}

#[test]
fn block_reset_test_window_restores_its_baseline_and_keeps_the_editor_unchanged() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let parameter =
        actions::parameters(&app.tabs[source].scene.document, definition, Kind::Move)[0].handle;
    let targets = members(&app);
    actions::create(
        &mut app.tabs[source].scene.document,
        definition,
        Kind::Move,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let editor = app.tabs[source].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    let i = app.active_tab;
    let insert = app.tabs[i].block_test.as_ref().unwrap().insert;
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
        grip: None,
        insert: None,
        document: app.tabs[i].id,
        definition,
        parameter,
        value: actions::Value::Point(glam::DVec3::new(20., 0., 0.)),
    });
    let changed = app.tabs[i].scene.document.clone();
    assert_ne!(
        changed.get_entity(targets[0]),
        editor.get_entity(targets[0])
    );
    app.tabs[i].scene.select_entity(insert, true);
    let _ = app.dispatch_command("RESETBLOCK");
    assert_eq!(
        app.tabs[i].scene.document.get_entity(targets[0]),
        editor.get_entity(targets[0])
    );
    assert_eq!(app.tabs[source].scene.document, editor);
    let reset = app.tabs[i].scene.document.clone();
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, changed);
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document, reset);
}

#[test]
fn block_reset_and_table_context_guards_preserve_running_commands() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BTABLE");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, reference);
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("RESETBLOCK");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    let _ = app.update(Message::CommandEscape);
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let _ = app.dispatch_command("BTABLE");
    assert_eq!(app.active_modal, Some(ModalKind::BlockPropertiesTable));
    let _ = app.update(Message::CloseModal);
    let _ = app.update(Message::NamedParametersOpen);
    assert_eq!(app.active_modal, Some(ModalKind::NamedParameters));
    let _ = app.dispatch_command("PARAMETERSCLOSE");
    assert_eq!(app.active_modal, None);
}

#[test]
fn block_actions_rejected_outside_editor_preserve_running_command_and_stale_results_are_inert() {
    use crate::scene::block_actions::Kind;
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("LINE");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BACTION MOVE");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER POINT 2,3 4,5");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = crate::scene::block_actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)
        [0]
    .handle;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_action_result(crate::command::CmdResult::PlaceBlockAction {
        document: app.tabs[i].id + 1,
        definition: def,
        kind: Kind::Move,
        parameter: p,
        xy_key_point: None,
        point_choices: None,
        targets: members(&app),
        location: glam::DVec3::ZERO,
        frame: None,
        array_offsets: None,
        displacement_overrides: None,
        rotate_only: vec![],
    });
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BACTION SCALE");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
}

#[test]
fn block_actions_test_reference_transform_visibility_and_noop_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let _ = app.run_command_line("BPARAMETER POINT 2,3 4,5");
    let _ = app.run_command_line("BPARAMETER VISIBILITY 8,8");
    let _ = app.run_command_line("BPARAMETER BASEPOINT 1,2");
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let p =
        actions::parameters(&app.tabs[source].scene.document, definition, Kind::Move)[0].clone();
    let targets = members(&app);
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &p.name);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app, "4,6");
    let _ = app.dispatch_command("BTESTBLOCK");
    let i = app.active_tab;
    let insert = app.tabs[i].block_test.as_ref().unwrap().insert;
    let depth = app.tabs[i].history.undo_stack.len();
    let id = app.tabs[i].id;
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity(insert).unwrap() else {
        panic!()
    };
    let current_world = actions::v(
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, ins)
            .apply(actions::native(p.end)),
    );
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
        grip: None,
        insert: None,
        document: id,
        definition,
        parameter: p.handle,
        value: crate::scene::block_actions::Value::Point(current_world),
    });
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    // Existing visibility UI creates a per-reference anonymous definition.
    app.apply_visibility_state(insert, 0);
    app.finish_pending_history(i);
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity(insert).unwrap() else {
        panic!()
    };
    let after_world = actions::v(
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, ins)
            .apply(actions::native(p.end)),
    );
    assert_eq!(
        after_world, current_world,
        "visibility representation retains the native Basepoint anchor"
    );
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity_mut(insert).unwrap() else {
        panic!()
    };
    ins.insert_point = Vector3::new(100., 200., 0.);
    ins.rotation = std::f64::consts::FRAC_PI_2;
    ins.set_x_scale(2.);
    ins.set_y_scale(3.);
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity(insert).unwrap() else {
        panic!()
    };
    let name = ins.block_name.clone();
    assert_ne!(name, "EDIT_ME");
    let world = actions::v(
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, ins)
            .apply(Vector3::new(12., 13., 0.)),
    );
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
        grip: None,
        insert: None,
        document: id,
        definition,
        parameter: p.handle,
        value: crate::scene::block_actions::Value::Point(world),
    });
    let anon = app.tabs[i]
        .scene
        .document
        .entities_in_block(&name)
        .find_map(|e| {
            if let EntityType::Line(l) = e {
                Some(l)
            } else {
                None
            }
        })
        .unwrap();
    assert!((anon.start.x - 10.).abs() < 1e-9 && (anon.start.y - 10.).abs() < 1e-9);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
}

fn authored_points(app: &OpenCADStudio) -> Vec<document::objects::BlockPointParameter> {
    let tab = &app.tabs[app.active_tab];
    let definition = tab
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    crate::scene::block_point_parameter::parameters(&tab.scene.document, definition)
        .into_iter()
        .map(|(_, parameter)| parameter.clone())
        .collect()
}

fn distance_parameter_count(app: &OpenCADStudio) -> usize {
    let tab = &app.tabs[app.active_tab];
    let definition = tab
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    crate::scene::block_distance_parameter::parameters(&tab.scene.document, definition).len()
}

fn orientation_parameter_count(app: &OpenCADStudio) -> usize {
    let tab = &app.tabs[app.active_tab];
    let definition = tab
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    crate::scene::block_orientation_parameter::parameters(&tab.scene.document, definition).len()
}

#[test]
fn lookup_basepoint_placement_history_test_block_and_saved_parent_undo() {
    use crate::scene::{block_basepoint, block_lookup, render_graph};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let members_before = members(&app);
    let _ = app.run_command_line("BPARAMETER BASEPOINT 3,4");
    let (handle, p) = block_basepoint::parameter(&app.tabs[i].scene.document, definition).unwrap();
    assert_eq!(p.point, Vector3::new(3.0, 4.0, 0.0));
    let depth = app.tabs[i].history.undo_stack.len();
    assert_eq!(depth, 1);
    let _ = app.run_command_line("BPARAMETER BASEPOINT 3,4");
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        depth,
        "same anchor is a no-op"
    );
    let _ = app.run_command_line("BPARAMETER BASEPOINT 5,6");
    assert_eq!(
        block_basepoint::parameter(&app.tabs[i].scene.document, definition)
            .unwrap()
            .0,
        handle
    );
    app.undo_steps(1);
    assert_eq!(
        block_basepoint::parameter(&app.tabs[i].scene.document, definition)
            .unwrap()
            .1
            .point,
        Vector3::new(3.0, 4.0, 0.0)
    );
    app.redo_steps(1);
    let _ = app.dispatch_command("BPARAMETER");
    feed(&mut app, "lookup");
    feed(&mut app, "2,2");
    feed(&mut app, "");
    assert_eq!(
        block_lookup::parameters(&app.tabs[i].scene.document, definition).len(),
        1
    );
    assert_eq!(
        members(&app),
        members_before,
        "authoring objects are not drawing entities"
    );
    let before_test = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    let doc = &app.tabs[app.active_tab].scene.document;
    let insert = doc
        .entities()
        .find_map(|e| match e {
            EntityType::Insert(i) => Some(i),
            _ => None,
        })
        .unwrap();
    assert!(
        (render_graph::insert_transform(doc, insert).apply(Vector3::new(5.0, 6.0, 0.0))
            - insert.insert_point)
            .length()
            < 1e-9
    );
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.tabs[i].scene.document, before_test);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert!(block_basepoint::parameter(&app.tabs[i].scene.document, definition).is_some());
    app.undo_steps(1);
    assert!(block_basepoint::parameter(&app.tabs[i].scene.document, definition).is_none());
    app.redo_steps(1);
    assert!(block_basepoint::parameter(&app.tabs[i].scene.document, definition).is_some());
}

/// The insertion grip is the marker on its own until the definition carries a
/// basepoint parameter; only then does it earn the `Basepoint` caption, because
/// a permanent caption on the origin marker reads as a parameter nobody authored.
#[test]
fn the_basepoint_caption_appears_only_with_a_basepoint_parameter() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let origin = Vector3::new(0.0, 0.0, 0.0);
    let marker = crate::scene::block_basepoint::marker_graphic(
        &app.tabs[i].scene.document,
        definition,
        origin,
    );
    assert_eq!(
        marker.base_grips,
        vec![origin],
        "the insertion grip is drawn either way"
    );
    assert!(
        marker.labels.is_empty(),
        "the plain insertion grip carries no caption"
    );
    let _ = app.run_command_line("BPARAMETER BASEPOINT 3,4");
    let anchor = crate::scene::block_basepoint::parameter(&app.tabs[i].scene.document, definition)
        .unwrap()
        .1
        .point;
    let marker = crate::scene::block_basepoint::marker_graphic(
        &app.tabs[i].scene.document,
        definition,
        anchor,
    );
    assert_eq!(
        marker.base_grips,
        vec![anchor],
        "a definition with a basepoint parameter still draws its grip"
    );
    assert_eq!(
        marker.labels,
        vec![(anchor, "Basepoint".to_string())],
        "a definition that carries a basepoint parameter captions its grip"
    );
}

fn lookup_editor_fixture() -> (OpenCADStudio, Handle, Handle) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 2,0 0,1");
    let _ = app.run_command_line("BPARAMETER LOOKUP 1,1");
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)[0].0;
    (app, definition, parameter)
}
fn edit_lookup(app: &mut OpenCADStudio) {
    use crate::ui::window::block_lookup::Action as A;
    let _ = app.update(Message::BlockLookup(A::Property(0)));
    let _ = app.update(Message::BlockLookup(A::AddProperty));
    let _ = app.update(Message::BlockLookup(A::AddRow));
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "20".into())));
    let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Small".into())));
}

mod lookup_editor;

mod lookup_prompts;

mod parameter_placement;

mod orientation_parameters;

mod distance_xy_point_parameters;

fn visibility_editor() -> (OpenCADStudio, crate::ui::window::block_visibility::Target) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let target = app
        .block_visibility_context()
        .expect("placed parameter")
        .target;
    (app, target)
}

fn visibility_action(app: &mut OpenCADStudio, action: crate::ui::window::block_visibility::Action) {
    let _ = app.update(Message::BlockVisibility(action));
}

#[test]
fn visibility_ribbon_manager_cancel_validation_and_single_undo() {
    use crate::ui::window::block_visibility::Action as A;
    let (mut app, target) = visibility_editor();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    visibility_action(&mut app, A::Open(target));
    visibility_action(&mut app, A::Name("Alternate".into()));
    visibility_action(&mut app, A::New);
    assert_eq!(app.tabs[i].scene.document, before, "dialog is staged");
    let _ = app.update(Message::CommandEscape);
    assert!(app.block_visibility.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(app.tabs[i].scene.document, before);
    visibility_action(&mut app, A::Open(target));
    visibility_action(&mut app, A::Name("visibilitystate0".into()));
    visibility_action(&mut app, A::New);
    assert!(app.block_visibility.as_ref().unwrap().error.is_some());
    visibility_action(&mut app, A::Delete);
    assert_eq!(
        app.block_visibility.as_ref().unwrap().edited.states.len(),
        1
    );
    visibility_action(&mut app, A::Name("Alternate".into()));
    visibility_action(&mut app, A::New);
    visibility_action(&mut app, A::SetCurrent);
    visibility_action(&mut app, A::Name("Alternate renamed".into()));
    visibility_action(&mut app, A::Rename);
    visibility_action(&mut app, A::Move(true));
    visibility_action(&mut app, A::Apply);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Alternate renamed")
    );
    app.undo_steps(1);
    assert_eq!(
        app.block_visibility_context().unwrap().names,
        ["VisibilityState0"]
    );
    app.redo_steps(1);
    assert_eq!(
        app.block_visibility_context().unwrap().names,
        ["Alternate renamed", "VisibilityState0"]
    );
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Alternate renamed")
    );
}

/// The dialog carries a Help control that explains itself on the command
/// line; it must not close the dialog or disturb the staged edits.
#[test]
fn visibility_dialog_help_explains_itself_without_touching_the_staged_edits() {
    use crate::ui::window::block_visibility::Action as A;
    let (mut app, target) = visibility_editor();
    let i = app.active_tab;
    visibility_action(&mut app, A::Open(target));
    visibility_action(&mut app, A::Name("Alternate".into()));
    visibility_action(&mut app, A::New);
    let staged = app.block_visibility.as_ref().unwrap().edited.states.len();
    let selected = app.block_visibility.as_ref().unwrap().selected;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    visibility_action(&mut app, A::Help);
    assert_eq!(
        app.active_modal,
        Some(ModalKind::BlockVisibility),
        "Help must leave the dialog open"
    );
    let help = app.command_line.history.last().unwrap().text.clone();
    assert!(
        help.contains("visibility state"),
        "Help should explain what the dialog does, got {help:?}"
    );
    assert_eq!(
        app.block_visibility.as_ref().unwrap().edited.states.len(),
        staged,
        "Help must not change the staged state list"
    );
    assert_eq!(
        app.block_visibility.as_ref().unwrap().selected,
        selected,
        "Help must not change the selection"
    );
    assert_eq!(app.tabs[i].scene.document, before, "Help must not apply");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
}

/// Creating a state from the command line offers the same three visibility
/// choices as the dialog; the default leaves the current state's set alone.
#[test]
fn visibility_creation_from_the_command_line_asks_about_existing_objects() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let _ = app.dispatch_command("BVSTATE");
    feed(&mut app, "N");
    feed(&mut app, "Solo");
    let prompt = app.tabs[i]
        .active_cmd
        .as_ref()
        .map(|command| command.prompt())
        .unwrap_or_default();
    assert!(
        prompt.contains("Show all objects") && prompt.contains("Hide all objects"),
        "the name should be followed by the visibility choice, got {prompt:?}"
    );
    feed(&mut app, "");
    let block = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, block).unwrap();
    let created = parameter
        .states
        .iter()
        .find(|state| state.name == "Solo")
        .expect("the default choice still creates the state");
    assert_eq!(
        created.visible_blocks, parameter.states[0].visible_blocks,
        "Enter keeps what the current state shows"
    );
    let _ = parameter;
    let member = members(&app)[0];
    let _ = app.dispatch_command("BVSTATE");
    feed(&mut app, "N");
    feed(&mut app, "Loud");
    feed(&mut app, "S");
    let parameter =
        crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, block).unwrap();
    let loud = parameter
        .states
        .iter()
        .find(|state| state.name == "Loud")
        .expect("the Show keyword creates the state");
    assert!(
        loud.visible_blocks.contains(&member),
        "the Show keyword shows the members in the new state"
    );
}

/// `Show` and `Hide` govern every member and set the new state's set.
#[test]
fn visibility_state_created_with_show_or_hide_sets_the_new_states_visibility() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let member = members(&app)[0];
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let _ = app.dispatch_command("_BVSTATE\u{1f}NEW\u{1f}Shown\u{1f}SHOW");
    let _ = app.dispatch_command("_BVSTATE\u{1f}NEW\u{1f}Hidden\u{1f}HIDE");
    let block = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, block).unwrap();
    let state = |name: &str| {
        parameter
            .states
            .iter()
            .find(|state| state.name == name)
            .unwrap_or_else(|| panic!("state {name} was created"))
    };
    assert!(
        state("Shown").visible_blocks.contains(&member),
        "Show all objects shows the members in the new state"
    );
    assert!(
        state("Hidden").visible_blocks.is_empty(),
        "Hide all objects hides the members in the new state"
    );
    assert!(parameter.all_blocks.contains(&member));
}

/// The underscore-prefixed forms create, hide and show through the selection.
#[test]
fn underscore_visibility_commands_act_on_states_and_the_selection() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let member = members(&app)[0];
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let _ = app.dispatch_command("_BVSTATE\u{1f}NEW\u{1f}Other");
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Other"),
        "_BVSTATE NEW switches to the new state like the plain form"
    );
    assert!(
        !app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible,
        "the member starts visible"
    );
    app.tabs[i].scene.select_entity(member, true);
    let _ = app.dispatch_command("_BVHIDE");
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible,
        "_BVHIDE hides the selection like the plain form"
    );
    app.tabs[i].scene.select_entity(member, true);
    let _ = app.dispatch_command("_BVSHOW");
    assert!(
        !app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible,
        "_BVSHOW restores it"
    );
}

/// BVMODE 1 draws state-hidden members dimmed; BVMODE 0 hides them.
#[test]
fn visibility_mode_dims_the_hidden_members_it_reveals() {
    use crate::ui::window::block_visibility::{Action as A, NewVisibility as V};
    let (mut app, target) = visibility_editor();
    let i = app.active_tab;
    let member = members(&app)[0];
    visibility_action(&mut app, A::Open(target));
    visibility_action(&mut app, A::Name("Hidden".into()));
    visibility_action(&mut app, A::NewVisibility(V::Hide));
    visibility_action(&mut app, A::New);
    visibility_action(&mut app, A::Apply);
    visibility_action(&mut app, A::Current(target, "Hidden".into()));
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible,
        "the Hidden state hid the member"
    );
    let wire_color = |app: &OpenCADStudio| {
        app.tabs[i]
            .scene
            .model_tile_wires_arc(
                0,
                &crate::scene::view::camera::Camera::default(),
                1.0,
                800.0,
            )
            .iter()
            .find(|wire| crate::scene::Scene::handle_from_wire_name(&wire.name) == Some(member))
            .map(|wire| wire.color)
    };
    let _ = app.run_command_line("BVMODE 0");
    assert!(wire_color(&app).is_none(), "hidden geometry is not drawn");
    let _ = app.run_command_line("BVMODE 1");
    let dimmed = wire_color(&app).expect("visibility mode draws the hidden member");
    let _ = app.run_command_line("BVMODE 0");
    visibility_action(&mut app, A::Current(target, "VisibilityState0".into()));
    let plain = wire_color(&app).expect("the first state shows the member");
    assert_ne!(
        dimmed, plain,
        "the revealed member is drawn dimmed, not in its usual colour"
    );
}

#[test]
fn visibility_ribbon_new_modes_state_selection_and_native_roundtrip() {
    use crate::ui::window::block_visibility::{Action as A, NewVisibility as V};
    let (mut app, target) = visibility_editor();
    let i = app.active_tab;
    let member = members(&app)[0];
    visibility_action(&mut app, A::Open(target));
    for (name, mode) in [("Hidden", V::Hide), ("Shown", V::Show), ("Copy", V::Keep)] {
        visibility_action(&mut app, A::Name(name.into()));
        visibility_action(&mut app, A::NewVisibility(mode));
        visibility_action(&mut app, A::New);
    }
    visibility_action(&mut app, A::Apply);
    let parameter =
        crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, target.definition)
            .unwrap();
    assert!(parameter.states[0].visible_blocks.contains(&member));
    assert!(parameter.states[1].visible_blocks.is_empty());
    assert!(parameter.states[2].visible_blocks.contains(&member));
    assert!(parameter.states[3].visible_blocks.contains(&member));
    visibility_action(&mut app, A::Current(target, "Hidden".into()));
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible
    );
    visibility_action(&mut app, A::Current(target, "Shown".into()));
    visibility_action(&mut app, A::Current(target, "Copy".into()));
    app.undo_steps(1);
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Shown"),
        "identical states retain their identity in history"
    );
    app.redo_steps(1);
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Copy")
    );
    app.save_block_edit(i);
    for ext in ["dwg", "dxf"] {
        let document = &app.tabs[i].scene.document;
        let bytes = crate::io::save_to_bytes(document, ext, document.version).unwrap();
        let loaded = crate::io::load_bytes(&format!("visibility.{ext}"), bytes).unwrap();
        let parameter =
            crate::scene::block_visibility::parameter(&loaded, target.definition).unwrap();
        assert_eq!(
            parameter
                .states
                .iter()
                .map(|state| state.name.as_str())
                .collect::<Vec<_>>(),
            ["VisibilityState0", "Hidden", "Shown", "Copy"]
        );
        assert!(parameter.states[1].visible_blocks.is_empty());
        assert!(parameter.states[2].visible_blocks.contains(&member));
    }
}

#[test]
fn visibility_ribbon_mode_and_make_visible_cancel_do_not_change_saved_flags() {
    use crate::ui::window::block_visibility::Action as A;
    let (mut app, target) = visibility_editor();
    let i = app.active_tab;
    let member = members(&app)[0];
    app.tabs[i].scene.select_entity(member, true);
    let _ = app.run_command_line("BVHIDE");
    app.finish_pending_history(i);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let dirty = app.tabs[i].dirty;
    let wires = |app: &OpenCADStudio| {
        app.tabs[i]
            .scene
            .entity_wires()
            .iter()
            .filter(|wire| crate::scene::Scene::handle_from_wire_name(&wire.name) == Some(member))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert!(wires(&app).is_empty());
    visibility_action(&mut app, A::Mode(target));
    assert!(
        !wires(&app).is_empty(),
        "hidden member is rendered for picking"
    );
    assert_eq!(app.tabs[i].scene.document, before);
    visibility_action(&mut app, A::Mode(target));
    assert!(wires(&app).is_empty());
    let _ = app.run_command_line("BVSHOW");
    app.sync_block_visibility_display();
    assert!(!wires(&app).is_empty());
    let _ = app.update(Message::CommandEscape);
    assert!(wires(&app).is_empty());
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(app.tabs[i].dirty, dirty);
    let _ = app.run_command_line("BVMODE 1");
    assert!(app.block_visibility_mode);
    let _ = app.run_command_line("SETVAR BVMODE 2");
    assert!(app.block_visibility_mode);
    let _ = app.run_command_line("BVMODE 0");
    assert!(!app.block_visibility_mode);
    assert!(
        app.pending_setvar.is_none(),
        "inline variable must close its probe prompt"
    );
    let _ = app.run_command_line("BVSHOW");
    app.tabs[i].scene.select_entity(member, true);
    let result = app.tabs[i]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_selection_complete(vec![member]);
    let _ = app.apply_cmd_result(result);
    feed(&mut app, "");
    assert!(
        !app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible
    );
    app.undo_steps(1);
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible,
        "undo uses the hidden flags from before picking"
    );
}

#[test]
fn visibility_ribbon_manager_rejects_stale_target_and_stale_document() {
    use crate::ui::window::block_visibility::Action as A;
    let (mut app, target) = visibility_editor();
    visibility_action(&mut app, A::Open(target));
    visibility_action(&mut app, A::Name("Alternate".into()));
    visibility_action(&mut app, A::New);
    app.tabs[app.active_tab].scene.bump_geometry();
    visibility_action(&mut app, A::Apply);
    assert!(app.block_visibility.as_ref().unwrap().error.is_some());
    assert_eq!(
        app.block_visibility_context().unwrap().names,
        ["VisibilityState0"]
    );
    let _ = app.update(Message::CloseModal);
    let mut stale = target;
    stale.document += 1;
    visibility_action(&mut app, A::Open(stale));
    assert!(app.block_visibility.is_none());
    assert!(app.active_modal.is_none());
    let _ = app.run_command_line("BVSTATE DIALOG");
    assert_eq!(app.active_modal, Some(ModalKind::BlockVisibility));
}

#[test]
fn local_undo_preserves_imported_native_networks_when_constraints_are_unchanged() {
    use document::objects::{
        AssocNetwork, AssociativeData, AssociativeObject, Dictionary, ObjectType,
    };
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let owner = doc.block_records.get("EDIT_ME").unwrap().handle;
    let dictionary_handle = doc.allocate_handle();
    let network_handle = doc.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = dictionary_handle;
    dictionary.owner = owner;
    dictionary.hard_owner = true;
    dictionary.add_entry("ACAD_ASSOCNETWORK", network_handle);
    doc.objects
        .insert(dictionary_handle, ObjectType::Dictionary(dictionary));
    let network = AssociativeObject {
        handle: network_handle,
        owner: dictionary_handle,
        reactors: vec![dictionary_handle],
        dxf_name: "ASSOCNETWORK".into(),
        cpp_class_name: "AcDbAssocNetwork".into(),
        data: AssociativeData::Network(AssocNetwork::default()),
        ..Default::default()
    };
    doc.objects
        .insert(network_handle, ObjectType::Associative(network));
    begin(&mut app, reference);
    let original_objects = app.tabs[i].scene.document.objects.clone();
    circle(&mut app, 1.0);
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 1);
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
    app.redo_steps(1);
    assert_eq!(members(&app).len(), 2);
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
    assert!(app.save_block_edit_as(i, "COPY"));
    for (handle, object) in original_objects {
        let mut actual = app.tabs[i].scene.document.objects[&handle].clone();
        if handle == app.tabs[i].scene.document.header.named_objects_dict_handle {
            // Copying the first scope installs the drawing's root network.
            let ObjectType::Dictionary(dictionary) = &mut actual else {
                panic!()
            };
            dictionary
                .entries
                .retain(|(key, _)| key != "ACAD_ASSOCNETWORK");
        }
        assert_eq!(actual, object);
    }
    assert_eq!(members(&app).len(), 1);
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("COPY")
            .unwrap()
            .entity_handles
            .len(),
        2
    );
}

mod save_as_sessions;

mod locked_bedit;

mod visibility_parameters;

mod display_and_association;

mod grip_cycling;

mod action_bars;

mod action_overrides;

#[test]
fn block_action_overrides_command_options_create_one_atomic_action() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].clone();
    let targets = members(&app);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &parameter.name);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app, "Multiplier");
    feed(&mut app, "NaN");
    assert!(app.tabs[i].active_cmd.is_some());
    assert_eq!(app.tabs[i].scene.document, before);
    feed(&mut app, "0.5");
    feed(&mut app, "Offset");
    feed(&mut app, "90");
    feed(&mut app, "4,4");
    assert!(app.tabs[i].active_cmd.is_none());
    let native = actions::actions(&app.tabs[i].scene.document, definition);
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].2, Kind::Move);
    let action = native[0].0;
    let offsets = actions::options::offsets(&app.tabs[i].scene.document, action).unwrap();
    assert_eq!(offsets.distance_multiplier, 0.5);
    assert_eq!(offsets.angle_offset, std::f64::consts::FRAC_PI_2);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert!(actions::actions(&app.tabs[i].scene.document, definition).is_empty());
    app.redo_steps(1);
    assert!(actions::options::offsets(&app.tabs[i].scene.document, action).is_some());
}

#[test]
fn block_polygon_stretch_command_cancel_close_replace_and_local_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].clone();
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BACTION STRETCH");
    feed(&mut app, &parameter.name);
    feed(&mut app, "CPolygon");
    feed(&mut app, "0,0");
    feed(&mut app, "2,0");
    feed(&mut app, "Close");
    assert!(app.tabs[i].active_cmd.is_some());
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.dispatch_command("BACTION STRETCH");
    feed(&mut app, &parameter.name);
    feed(&mut app, "CPolygon");
    for value in ["-1,-1", "2,-1", "8,8", "Undo", "2,2", "-1,2"] {
        feed(&mut app, value);
    }
    let _ = app.feed_command(crate::command::StepInput::Enter);
    let targets = members(&app);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app, "4,4");
    assert!(app.tabs[i].active_cmd.is_none());
    let action = actions::actions(&app.tabs[i].scene.document, definition)
        .iter()
        .find(|(_, _, kind, _)| *kind == Kind::Stretch)
        .unwrap()
        .0;
    assert_eq!(
        actions::frame_outline(&app.tabs[i].scene.document, action).len(),
        4
    );
    let created = app.tabs[i].scene.document.objects[&action].clone();
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.update(Message::BlockActionBarSelect(action));
    let _ = app.update(Message::BlockActionBarEdit(action));
    feed(&mut app, "Frame");
    feed(&mut app, "CPolygon");
    for value in ["0,-1", "2,-1", "0,2"] {
        feed(&mut app, value);
    }
    feed(&mut app, "Close");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        actions::frame_outline(&app.tabs[i].scene.document, action).len(),
        3
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document.objects[&action], created);
    app.undo_steps(1);
    assert!(actions::actions(&app.tabs[i].scene.document, definition).is_empty());
    app.redo_steps(2);
    assert_eq!(
        actions::frame_outline(&app.tabs[i].scene.document, action).len(),
        3
    );
}

#[test]
fn polar_stretch_authoring_collects_rotation_selection_and_edits_it_with_local_undo() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    circle(&mut app, 3.);
    let _ = app.run_command_line("BPARAMETER POLAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].clone();
    let targets = members(&app);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BACTION POLAR STRETCH");
    feed(&mut app, &parameter.name);
    feed(&mut app, "9,-1");
    feed(&mut app, "11,1");
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .prompt()
        .contains("rotate only"));
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![
        targets[1],
    ]));
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.dispatch_command("BACTION POLAR STRETCH");
    feed(&mut app, &parameter.name);
    feed(&mut app, "9,-1");
    feed(&mut app, "11,1");
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![
        targets[0],
    ]));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .prompt()
        .contains("rotate only"));
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![
        targets[1],
    ]));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app, "Multiplier");
    feed(&mut app, "2");
    feed(&mut app, "Offset");
    feed(&mut app, "90");
    assert_eq!(
        app.tabs[i].scene.document, before,
        "all selections and overrides are staged"
    );
    feed(&mut app, "4,4");
    assert!(app.tabs[i].active_cmd.is_none());
    let action = actions::actions(&app.tabs[i].scene.document, definition)
        .iter()
        .find(|(_, _, kind, _)| *kind == Kind::PolarStretch)
        .unwrap()
        .0;
    let native = app.tabs[i].scene.document.objects[&action].clone();
    let document::objects::ObjectType::DynamicBlock(object) = &native else {
        panic!()
    };
    let document::objects::DynamicBlockData::PolarStretchAction(polar) = &object.data else {
        panic!()
    };
    assert_eq!(polar.handles, vec![targets[1]]);
    assert_eq!(polar.action.dependencies, targets);
    assert_eq!(polar.distance_multiplier, 2.);
    assert_eq!(polar.angle_offset, std::f64::consts::FRAC_PI_2);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.update(Message::BlockActionBarSelect(action));
    let _ = app.update(Message::BlockActionBarEdit(action));
    feed(&mut app, "RotateOnly");
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].active_cmd.is_none());
    let document::objects::ObjectType::DynamicBlock(object) =
        &app.tabs[i].scene.document.objects[&action]
    else {
        panic!()
    };
    let document::objects::DynamicBlockData::PolarStretchAction(polar) = &object.data else {
        panic!()
    };
    assert!(polar.handles.is_empty());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document.objects[&action], native);
    app.undo_steps(1);
    assert!(actions::actions(&app.tabs[i].scene.document, definition).is_empty());
    app.redo_steps(2);
    assert!(actions::options::offsets(&app.tabs[i].scene.document, action).is_some());
}

#[test]
fn block_action_base_properties_edit_native_pivot_with_local_undo() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 2,3 12,3 7,5");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
    let targets = members(&app);
    let action = actions::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Scale,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.update(Message::BlockActionBarSelect(action));
    let initial = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropBoolToggle("authoring_action_base_dependent"));
    let base = actions::options::base(&app.tabs[i].scene.document, action).unwrap();
    assert!(!base.dependent);
    assert_eq!(base.base_point, document::types::Vector3::new(2., 3., 0.));
    assert!(app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .any(|p| p.field == "authoring_action_base_x"));
    let _ = app.update(Message::PropGeomInput {
        field: "authoring_action_base_x",
        value: "9/2".into(),
    });
    let _ = app.update(Message::PropGeomCommit("authoring_action_base_x"));
    assert_eq!(
        actions::options::base(&app.tabs[i].scene.document, action)
            .unwrap()
            .base_point
            .x,
        4.5
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    let changed = app.tabs[i].scene.document.clone();
    for input in ["NaN", "inf", "unknown_name"] {
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_action_base_y",
            value: input.into(),
        });
        let _ = app.update(Message::PropGeomCommit("authoring_action_base_y"));
        assert_eq!(app.tabs[i].scene.document, changed);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    }
    app.undo_steps(2);
    assert_eq!(
        app.tabs[i].scene.document.objects[&action],
        initial.objects[&action]
    );
    app.redo_steps(2);
    assert_eq!(
        app.tabs[i].scene.document.objects[&action],
        changed.objects[&action]
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropBoolToggle("authoring_action_base_dependent"));
    let _ = app.update(Message::PropGeomCommit("authoring_action_base_x"));
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn block_action_base_offset_properties_refresh_references_and_keep_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    use glam::DVec3;
    for kind in [Kind::Scale, Kind::Rotate] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let _ = app.run_command_line(if kind == Kind::Scale {
            "BPARAMETER LINEAR 0,0 10,0 5,2"
        } else {
            "BPARAMETER ROTATION 0,0 10 0 5,5"
        });
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
        let targets = members(&app);
        let action = actions::create(
            &mut app.tabs[i].scene.document,
            definition,
            kind,
            parameter,
            &targets,
            DVec3::new(8., 9., 0.),
        )
        .unwrap();
        let _ = app.update(Message::BlockActionBarSelect(action));
        let initial = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        assert!(app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .any(|p| p.field == "authoring_action_base_offset_x"));
        for (field, input) in [
            ("authoring_action_base_offset_x", "6/2"),
            ("authoring_action_base_offset_y", "2"),
        ] {
            let _ = app.update(Message::PropGeomInput {
                field,
                value: input.into(),
            });
            let _ = app.update(Message::PropGeomCommit(field));
        }
        let offset = Vector3::new(3., 2., 0.);
        assert_eq!(
            actions::options::base(&app.tabs[i].scene.document, action)
                .unwrap()
                .offset,
            offset
        );
        let changed = app.tabs[i].scene.document.clone();
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
        for input in ["NaN", "inf", "unknown_name", "2"] {
            let _ = app.update(Message::PropGeomInput {
                field: "authoring_action_base_offset_y",
                value: input.into(),
            });
            let _ = app.update(Message::PropGeomCommit("authoring_action_base_offset_y"));
            assert_eq!(app.tabs[i].scene.document, changed);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
        }
        app.undo_steps(2);
        assert_eq!(
            app.tabs[i].scene.document.objects[&action],
            initial.objects[&action]
        );
        app.redo_steps(2);
        assert_eq!(
            app.tabs[i].scene.document.objects[&action],
            changed.objects[&action]
        );
        let _ = app.update(Message::BlockActionBarSelect(action));
        let _ = app.update(Message::PropBoolToggle("authoring_action_base_dependent"));
        let base = actions::options::base(&app.tabs[i].scene.document, action).unwrap();
        assert_eq!(base.base_point, offset);
        assert_eq!(base.offset, offset);
        let independent = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_action_base_offset_x",
            value: "20".into(),
        });
        let _ = app.update(Message::PropGeomCommit("authoring_action_base_offset_x"));
        assert_eq!(app.tabs[i].scene.document, independent);
        app.undo_steps(1);
        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
        let desired = if kind == Kind::Scale {
            DVec3::new(20., 0., 0.)
        } else {
            DVec3::new(0., 10., 0.)
        };
        let staged =
            actions::instance::evaluate(&app.tabs[i].scene.document, reference, parameter, desired)
                .unwrap()
                .unwrap();
        app.apply_action_document(i, staged);
        begin(&mut app, reference);
        let _ = app.update(Message::BlockActionBarSelect(action));
        // Editing the offset also rebuilds the previously evaluated reference.
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_action_base_offset_x",
            value: "4".into(),
        });
        let _ = app.update(Message::PropGeomCommit("authoring_action_base_offset_x"));
        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(native) = doc.get_entity(reference).unwrap() else {
            panic!()
        };
        let line = doc
            .block_records
            .get(&native.block_name)
            .unwrap()
            .entity_handles
            .iter()
            .find_map(|h| match doc.get_entity(*h) {
                Some(EntityType::Line(line)) => Some(line),
                _ => None,
            })
            .unwrap();
        let expected = if kind == Kind::Scale {
            Vector3::new(-4., -2., 0.)
        } else {
            Vector3::new(6., -2., 0.)
        };
        assert!(
            line.start.distance(&expected) < 1e-8,
            "{kind:?}: {:?}",
            line.start
        );
        let _ = app.dispatch_command("BEDIT_DISCARD");
        let before = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::PropGeomCommit("authoring_action_base_offset_x"));
        assert_eq!(app.tabs[i].scene.document, before);
    }
}

#[test]
fn block_polar_stretch_end_binding_command_scratch_and_reference_history() {
    use crate::command::{CmdResult, StepInput};
    use crate::scene::block_actions::{self as actions, Kind};
    use document::objects::{DynamicBlockData, ObjectType};
    use glam::DVec3;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let _ = app.dispatch_command("BACTIONBARMODE 0");
        let _ = app.run_command_line("BPARAMETER POLAR 0,0 10,0 5,2");
        let definition = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let parameter =
            actions::all_parameters(&app.tabs[source].scene.document, definition).remove(0);
        let targets = members(&app);
        let initial = app.tabs[source].scene.document.clone();
        let depth = app.tabs[source].history.undo_stack.len();
        let _ = app.dispatch_command("BACTION POLARSTRETCH");
        feed(&mut app, &parameter.name);
        feed(&mut app, "End");
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt()
            .contains("<End>"));
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[source].scene.document, initial);
        assert_eq!(app.tabs[source].history.undo_stack.len(), depth);
        let _ = app.dispatch_command("BACTION POLARSTRETCH");
        feed(&mut app, &parameter.name);
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .options()
            .iter()
            .any(|option| option.keyword == "End"));
        feed(&mut app, "Base");
        feed(&mut app, "End");
        feed(&mut app, "0.5,-0.5");
        feed(&mut app, "1.5,0.5");
        let _ = app.feed_command(StepInput::SelectionComplete(targets));
        let _ = app.feed_command(StepInput::Enter);
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(app.tabs[source].scene.document, initial);
        feed(&mut app, "4,4");
        assert!(app.tabs[source].active_cmd.is_none());
        let action = actions::actions(&app.tabs[source].scene.document, definition)
            .into_iter()
            .find(|(_, _, kind, _)| *kind == Kind::PolarStretch)
            .unwrap()
            .0;
        let native = app.tabs[source].scene.document.objects[&action].clone();
        let ObjectType::DynamicBlock(object) = &native else {
            panic!()
        };
        let DynamicBlockData::PolarStretchAction(polar) = &object.data else {
            panic!()
        };
        assert_eq!(polar.connections[0].name, "EndXDelta");
        assert_eq!(polar.connections[1].name, "EndYDelta");
        assert_eq!(app.tabs[source].history.undo_stack.len(), depth + 1);
        app.undo_steps(1);
        assert!(actions::actions(&app.tabs[source].scene.document, definition).is_empty());
        app.redo_steps(1);
        assert_eq!(app.tabs[source].scene.document.objects[&action], native);
        let ObjectType::DynamicBlock(object) = app.tabs[source]
            .scene
            .document
            .objects
            .get_mut(&action)
            .unwrap()
        else {
            panic!()
        };
        let DynamicBlockData::PolarStretchAction(polar) = &mut object.data else {
            panic!()
        };
        for connection in &mut polar.connections {
            connection.code = -1;
        }
        let orphan = app.tabs[source].scene.document.objects[&action].clone();
        app.tabs[source].selected_authoring = Some(action);
        let _ = app.dispatch_command("BASSOCIATE");
        feed(&mut app, &parameter.name);
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt()
            .contains("Base/End"));
        feed(&mut app, "End");
        assert!(app.tabs[source].active_cmd.is_none());
        assert_eq!(app.tabs[source].scene.document.objects[&action], native);
        app.undo_steps(1);
        assert_eq!(app.tabs[source].scene.document.objects[&action], orphan);
        app.redo_steps(1);
        assert_eq!(app.tabs[source].scene.document.objects[&action], native);
        if testing {
            let _ = app.open_block_test(source);
        } else {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        }
        let source_before = app.tabs[source].scene.document.clone();
        let i = app.active_tab;
        let insert = app.tabs[i]
            .block_test
            .as_ref()
            .map_or(reference, |test| test.insert);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let document = app.tabs[i].id;
        let _ = app.handle_block_action_result(CmdResult::EvaluateBlockParameter {
            grip: None,
            insert: (!testing).then_some(insert),
            document,
            definition,
            parameter: parameter.handle,
            value: actions::Value::Point(DVec3::new(0., 15., 0.)),
        });
        let geometry = |doc: &document::Document| {
            let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
                panic!()
            };
            let line = doc
                .block_records
                .get(&reference.block_name)
                .unwrap()
                .entity_handles
                .iter()
                .find_map(|h| match doc.get_entity(*h) {
                    Some(EntityType::Line(line)) => Some(line),
                    _ => None,
                })
                .unwrap();
            (line.start, line.end)
        };
        let expected = (Vector3::ZERO, Vector3::new(0., 6., 0.));
        let changed = geometry(&app.tabs[i].scene.document);
        assert!(changed.0.distance(&expected.0) < 1e-8);
        assert!(changed.1.distance(&expected.1) < 1e-8);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        assert_eq!(app.tabs[i].scene.document.objects[&action], native);
        app.undo_steps(1);
        assert_eq!(geometry(&app.tabs[i].scene.document), geometry(&before));
        app.redo_steps(1);
        assert_eq!(geometry(&app.tabs[i].scene.document), changed);
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
    }
}

#[test]
fn block_action_self_grip_selection_edits_keep_native_links_and_reference_history() {
    use crate::scene::{block_actions as actions, block_grips::config};
    use document::objects::{DynamicBlockData, ObjectType};
    use glam::DVec3;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        circle(&mut app, 3.);
        let _ = app.dispatch_command("BACTIONBARMODE 0");
        let _ = app.run_command_line("BPARAMETER POLAR 0,0 10,0 5,2");
        let definition = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let parameter =
            actions::all_parameters(&app.tabs[source].scene.document, definition).remove(0);
        let targets = members(&app);
        let action = actions::create_with_frame(
            &mut app.tabs[source].scene.document,
            definition,
            actions::Kind::PolarStretch,
            parameter.handle,
            &targets[..1],
            DVec3::new(4., 4., 0.),
            Some([DVec3::new(0.5, -1., 0.), DVec3::new(1.5, 1., 0.)]),
        )
        .unwrap();
        let grip = config::locations(
            &app.tabs[source].scene.document,
            definition,
            parameter.handle,
        )
        .unwrap()
        .into_iter()
        .find(|grip| grip.role == 1)
        .unwrap()
        .handle;
        let ObjectType::DynamicBlock(object) = app.tabs[source]
            .scene
            .document
            .objects
            .get_mut(&action)
            .unwrap()
        else {
            panic!()
        };
        let DynamicBlockData::PolarStretchAction(native) = &mut object.data else {
            panic!()
        };
        native.action.dependencies.insert(0, grip);
        let grip_before = app.tabs[source].scene.document.objects[&grip].clone();
        let original = app.tabs[source].scene.document.objects[&action].clone();
        edit_action_set(&mut app, action, "Add", targets[1..].to_vec());
        edit_action_set(&mut app, action, "Remove", targets[..1].to_vec());
        let deps = |app: &OpenCADStudio| {
            actions::actions(&app.tabs[source].scene.document, definition)
                .into_iter()
                .find(|(h, ..)| *h == action)
                .unwrap()
                .1
                .dependencies
                .clone()
        };
        assert_eq!(deps(&app), vec![grip, targets[1]]);
        app.undo_steps(2);
        assert_eq!(app.tabs[source].scene.document.objects[&action], original);
        app.redo_steps(2);
        app.tabs[source].selected_authoring = Some(action);
        let _ = app.dispatch_command("BACTIONSET");
        feed(&mut app, "Frame");
        feed(&mut app, "2,-1");
        feed(&mut app, "4,1");
        assert!(app.tabs[source].active_cmd.is_none());
        edit_action_set(&mut app, action, "RotateOnly", targets[1..].to_vec());
        assert_eq!(deps(&app), vec![grip, targets[1]]);
        assert_eq!(app.tabs[source].scene.document.objects[&grip], grip_before);
        if testing {
            let _ = app.open_block_test(source);
        } else {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        }
        let source_before = app.tabs[source].scene.document.clone();
        let i = app.active_tab;
        let insert = app.tabs[i]
            .block_test
            .as_ref()
            .map_or(reference, |test| test.insert);
        let document = app.tabs[i].id;
        let before = app.tabs[i].scene.document.clone();
        let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
            grip: None,
            insert: (!testing).then_some(insert),
            document,
            definition,
            parameter: parameter.handle,
            value: actions::Value::Point(DVec3::new(0., 15., 0.)),
        });
        let circle_center = |doc: &document::Document| {
            let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
                panic!()
            };
            doc.block_records
                .get(&reference.block_name)
                .unwrap()
                .entity_handles
                .iter()
                .find_map(|h| match doc.get_entity(*h) {
                    Some(EntityType::Circle(circle)) => Some(circle.center),
                    _ => None,
                })
                .unwrap()
        };
        assert!(
            circle_center(&app.tabs[i].scene.document).distance(&Vector3::new(0., 3., 0.)) < 1e-8
        );
        assert_eq!(app.tabs[i].scene.document.objects[&grip], grip_before);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.tabs[i].selected_grips.clear();
        app.tabs[i].selected_grip_handles.clear();
        app.refresh_test_action_grips([0.; 3]);
        let id = crate::scene::block_grips::runtime::id(parameter.id, 1).unwrap();
        assert!(
            app.tabs[i]
                .selected_grips
                .iter()
                .find(|grip| grip.id == id)
                .unwrap()
                .world
                .distance(DVec3::new(0., 15., 0.))
                < 1e-8
        );
        app.undo_steps(1);
        assert_eq!(
            circle_center(&app.tabs[i].scene.document),
            circle_center(&before)
        );
        app.redo_steps(1);
        assert!(
            circle_center(&app.tabs[i].scene.document).distance(&Vector3::new(0., 3., 0.)) < 1e-8
        );
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
    }
}

mod grip_counts;

#[test]
fn block_runtime_native_roles_drag_cancel_undo_redo_and_preserve_relocated_grips() {
    use crate::scene::{
        block_actions as actions,
        block_grips::{config, runtime},
    };
    use actions::{instance::ParameterFrame, XyKeyPoint};
    use glam::DVec3;
    for testing in [false, true] {
        for (family, role, desired, expected) in [
            ("POINT", 0, DVec3::new(3., 5., 0.), DVec3::new(3., 5., 0.)),
            (
                "LINEAR",
                0,
                DVec3::new(-2., 4., 0.),
                DVec3::new(-2., 0., 0.),
            ),
            ("LINEAR", 1, DVec3::new(13., 4., 0.), DVec3::new(3., 0., 0.)),
            (
                "POLAR",
                0,
                DVec3::new(-2., -3., 0.),
                DVec3::new(-2., -3., 0.),
            ),
            (
                "POLAR",
                1,
                DVec3::new(13., 15., 0.),
                DVec3::new(3., 15., 0.),
            ),
            ("XY", 0, DVec3::new(-2., -3., 0.), DVec3::new(-2., -3., 0.)),
            ("XY", 1, DVec3::new(13., 15., 0.), DVec3::new(3., 5., 0.)),
            ("XY", 2, DVec3::new(13., -3., 0.), DVec3::new(3., -3., 0.)),
            ("XY", 3, DVec3::new(-2., 15., 0.), DVec3::new(-2., 5., 0.)),
        ] {
            let (mut app, reference) = fixture();
            begin(&mut app, reference);
            let source = app.active_tab;
            let command = if family == "POINT" {
                "BPARAMETER POINT 0,0 5,2".to_owned()
            } else {
                format!(
                    "BPARAMETER {family} 0,0 {} 5,2",
                    if family == "XY" { "10,10" } else { "10,0" }
                )
            };
            let _ = app.run_command_line(&command);
            let definition = app.tabs[source]
                .active_block_edit_session()
                .unwrap()
                .br_handle;
            let p = actions::all_parameters(&app.tabs[source].scene.document, definition).remove(0);
            let key = [
                XyKeyPoint::Base,
                XyKeyPoint::End,
                XyKeyPoint::X,
                XyKeyPoint::Y,
            ][role];
            let targets = members(&app);
            actions::create_with_options(
                &mut app.tabs[source].scene.document,
                definition,
                actions::Kind::Move,
                p.handle,
                (family != "POINT").then_some(key),
                &targets,
                DVec3::ZERO,
                None,
                None,
            )
            .unwrap();
            let maximum = *config::choices(&app.tabs[source].scene.document, definition, p.handle)
                .last()
                .unwrap();
            config::set(
                &mut app.tabs[source].scene.document,
                definition,
                p.handle,
                Some(maximum),
            )
            .unwrap();
            let stable_ids = runtime::grips(
                &app.tabs[source].scene.document,
                definition,
                &p,
                actions::Value::Point(p.end),
            )
            .into_iter()
            .map(|grip| grip.id)
            .collect::<Vec<_>>();
            config::set(
                &mut app.tabs[source].scene.document,
                definition,
                p.handle,
                Some(0),
            )
            .unwrap();
            assert!(runtime::grips(
                &app.tabs[source].scene.document,
                definition,
                &p,
                actions::Value::Point(p.end)
            )
            .is_empty());
            config::set(
                &mut app.tabs[source].scene.document,
                definition,
                p.handle,
                Some(maximum),
            )
            .unwrap();
            assert_eq!(
                runtime::grips(
                    &app.tabs[source].scene.document,
                    definition,
                    &p,
                    actions::Value::Point(p.end)
                )
                .into_iter()
                .map(|grip| grip.id)
                .collect::<Vec<_>>(),
                stable_ids
            );
            let offset = Vector3::new(15., 7., 0.);
            for grip in
                config::locations(&app.tabs[source].scene.document, definition, p.handle).unwrap()
            {
                config::set_location(
                    &mut app.tabs[source].scene.document,
                    definition,
                    p.handle,
                    grip.role,
                    grip.point + offset,
                )
                .unwrap();
            }
            if testing {
                let _ = app.open_block_test(source);
            } else {
                let _ = app.dispatch_command("BSAVE");
                let _ = app.dispatch_command("BCLOSE");
            }
            let i = app.active_tab;
            let insert = app.tabs[i]
                .block_test
                .as_ref()
                .map(|test| test.insert)
                .unwrap_or(reference);
            let EntityType::Insert(native) =
                app.tabs[i].scene.document.get_entity_mut(insert).unwrap()
            else {
                panic!()
            };
            native.insert_point = Vector3::new(100., 200., 30.);
            native.normal = Vector3::new(
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            );
            native.rotation = std::f64::consts::FRAC_PI_2;
            native.set_x_scale(2.);
            native.set_y_scale(2.);
            native.set_z_scale(2.);
            let to_world = crate::scene::render_graph::insert_transform(
                &app.tabs[i].scene.document,
                match app.tabs[i].scene.document.get_entity(insert).unwrap() {
                    EntityType::Insert(ins) => ins,
                    _ => panic!(),
                },
            );
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1000., 800.);
            app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
                DVec3::new(40., 180., 0.),
                DVec3::new(120., 280., 0.),
                1.25,
            );
            let click = |app: &mut OpenCADStudio, point: DVec3| {
                let scene = &app.tabs[i].scene;
                let tile = scene.active_model_tile_bounds(1000., 800.);
                let at = scene
                    .camera
                    .borrow()
                    .project(
                        point,
                        iced::Rectangle::with_size(iced::Size::new(tile.width, tile.height)),
                    )
                    .unwrap();
                let point = iced::Point::new(at.x + tile.x, at.y + tile.y);
                let _ = app.update(Message::ViewportLeftPressAt(point));
                let _ = app.update(Message::ViewportLeftRelease);
            };
            let select = |app: &mut OpenCADStudio| {
                app.tabs[i].scene.select_entity(insert, true);
                app.tabs[i].selected_handle = Some(insert);
                app.tabs[i].selected_grips.clear();
                app.tabs[i].selected_grip_handles.clear();
                app.refresh_test_action_grips([0.; 3]);
            };
            select(&mut app);
            let id = runtime::id(p.id, role).unwrap();
            assert_eq!(
                app.tabs[i]
                    .selected_grips
                    .iter()
                    .filter(|grip| super::block_actions::is_test_grip(grip.id))
                    .count(),
                if family == "XY" { 3 } else { 1 },
                "grips sharing action input coordinates are eligible"
            );
            assert!(app.tabs[i].selected_grips.iter().any(|grip| grip.id == id));
            let before = app.tabs[i].scene.document.clone();
            let displayed = app.tabs[i]
                .selected_grips
                .iter()
                .find(|grip| grip.id == id)
                .unwrap()
                .world;
            click(&mut app, displayed);
            assert_eq!(
                app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.block_parameter_grip_id()),
                Some(id)
            );
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            select(&mut app);
            let native_grips = config::locations(&app.tabs[i].scene.document, definition, p.handle)
                .unwrap()
                .into_iter()
                .map(|grip| {
                    (
                        grip.handle,
                        app.tabs[i].scene.document.objects[&grip.handle].clone(),
                    )
                })
                .collect::<Vec<_>>();
            click(&mut app, displayed);
            assert_eq!(
                app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.block_parameter_grip_id()),
                Some(id)
            );
            let world = actions::v(to_world.apply(actions::native(desired) + offset));
            click(&mut app, world);
            let frame = actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap();
            let changed = app.tabs[i].scene.document.clone();
            let check = |app: &OpenCADStudio, delta: DVec3| {
                let EntityType::Insert(reference) =
                    app.tabs[i].scene.document.get_entity(insert).unwrap()
                else {
                    panic!()
                };
                let line = app.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&reference.block_name)
                    .unwrap()
                    .entity_handles
                    .iter()
                    .find_map(
                        |handle| match app.tabs[i].scene.document.get_entity(*handle) {
                            Some(EntityType::Line(line)) => Some(line),
                            _ => None,
                        },
                    )
                    .unwrap();
                assert!(
                    actions::v(line.start).distance(delta) < 1e-4,
                    "{testing} {family} role{role}: {:?} != {delta:?}",
                    line.start
                );
            };
            check(&app, expected);
            for (handle, native) in &native_grips {
                assert_eq!(&changed.objects[handle], native);
            }
            app.undo_steps(1);
            check(&app, DVec3::ZERO);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap(),
                ParameterFrame {
                    base: p.base,
                    end: p.end
                }
            );
            assert_eq!(
                app.tabs[i].scene.document.objects.len(),
                before.objects.len()
            );
            app.redo_steps(1);
            check(&app, expected);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap(),
                frame
            );
            select(&mut app);
            assert!(app.tabs[i].selected_grips.iter().any(|grip| grip.id == id));
        }
    }
}

#[test]
fn alignment_parameter_defaults_to_perpendicular_and_accepts_an_explicit_type() {
    use crate::scene::block_orientation_parameter as orientation;
    use document::objects::{DynamicBlockData, ObjectType};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let perpendicular = |doc: &document::Document, handle: Handle| match &doc.objects[&handle] {
        ObjectType::DynamicBlock(object) => match &object.data {
            DynamicBlockData::AlignmentParameter(p) => p.align_perpendicular,
            _ => panic!(),
        },
        _ => panic!(),
    };
    let _ = app.dispatch_command("BPARAMETER ALIGNMENT");
    feed(&mut app, "0,0");
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .prompt()
        .contains("<Perpendicular>"));
    feed(&mut app, "10,0");
    let first = orientation::parameters(&app.tabs[i].scene.document, definition)[0].handle;
    assert!(perpendicular(&app.tabs[i].scene.document, first));
    let _ = app.dispatch_command("BPARAMETER ALIGNMENT");
    feed(&mut app, "0,0");
    feed(&mut app, "TYPE");
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .prompt()
        .contains("Choose alignment type"));
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .prompt()
        .contains("<Perpendicular>"));
    feed(&mut app, "T");
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .prompt()
        .contains("<Tangent>"));
    feed(&mut app, "10,0");
    let second = orientation::parameters(&app.tabs[i].scene.document, definition)
        .into_iter()
        .find(|p| p.handle != first)
        .unwrap()
        .handle;
    assert!(!perpendicular(&app.tabs[i].scene.document, second));
    assert!(perpendicular(&app.tabs[i].scene.document, first));
}

#[test]
fn block_alignment_type_placement_properties_history_and_native_roundtrips() {
    use crate::scene::block_orientation_parameter::{self as orientation, Kind};
    use document::objects::{DynamicBlockData, ObjectType};
    use std::io::Cursor;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 0,0 TYPE P 10,0");
    let parameter = orientation::parameters(&app.tabs[i].scene.document, definition)
        .into_iter()
        .find(|p| p.kind == Kind::Alignment)
        .unwrap()
        .handle;
    let perpendicular = |doc: &document::Document| match &doc.objects[&parameter] {
        ObjectType::DynamicBlock(object) => match &object.data {
            DynamicBlockData::AlignmentParameter(p) => p.align_perpendicular,
            _ => panic!(),
        },
        _ => panic!(),
    };
    assert!(perpendicular(&app.tabs[i].scene.document));
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();
    assert!(app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .any(|property| property.field == "authoring_alignment_type"));
    let original = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_alignment_type",
        value: "Tangent".into(),
    });
    assert!(!perpendicular(&app.tabs[i].scene.document));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    for (handle, object) in &original.objects {
        if *handle != parameter {
            assert_eq!(app.tabs[i].scene.document.objects.get(handle), Some(object));
        }
    }
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .entities()
            .cloned()
            .collect::<Vec<_>>(),
        original.entities().cloned().collect::<Vec<_>>()
    );
    app.undo_steps(1);
    assert!(perpendicular(&app.tabs[i].scene.document));
    app.redo_steps(1);
    assert!(!perpendicular(&app.tabs[i].scene.document));
    app.tabs[i].selected_authoring = Some(parameter);
    let unchanged = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_alignment_type",
        value: "Tangent".into(),
    });
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_alignment_type",
        value: "Diagonal".into(),
    });
    assert_eq!(app.tabs[i].scene.document, unchanged);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_alignment_type",
        value: "Perpendicular".into(),
    });
    let doc = &app.tabs[i].scene.document;
    for saved in [
        document::DwgReader::from_stream(Cursor::new(
            document::DwgWriter::write_to_vec(doc).unwrap(),
        ))
        .read()
        .unwrap(),
        document::DxfReader::from_reader(Cursor::new(
            document::DxfWriter::new(doc).write_to_vec().unwrap(),
        ))
        .unwrap()
        .read()
        .unwrap(),
    ] {
        assert!(perpendicular(&saved));
    }
    let before = app.tabs[i].scene.document.clone();
    assert!(orientation::set_alignment_type(
        &mut app.tabs[i].scene.document,
        Handle::NULL,
        parameter,
        false
    )
    .is_err());
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 2,2 TYPE");
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    assert!(perpendicular(&app.tabs[i].scene.document));
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_alignment_type",
        value: "Tangent".into(),
    });
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn a_linear_parameter_lists_and_writes_its_base_location() {
    use crate::scene::model::object::PropValue;
    use document::objects::{DynamicBlockData, ObjectType};
    use std::io::Cursor;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "B");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter base location [Startpoint/Midpoint] <Startpoint>:"
    );
    feed(&mut app, "M");
    feed(&mut app, "0,0");
    feed(&mut app, "10,0");
    feed(&mut app, "5,2");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    let parameter =
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .find(|p| p.kind == crate::scene::block_distance_parameter::Kind::Linear)
            .unwrap()
            .handle;
    let base_location = |doc: &document::Document| match &doc.objects[&parameter] {
        ObjectType::DynamicBlock(object) => match &object.data {
            DynamicBlockData::LinearParameter(p) => p.parameter.parameter_base_location,
            _ => panic!(),
        },
        _ => panic!(),
    };
    assert_eq!(base_location(&app.tabs[i].scene.document), 1);
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();
    let listed = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find(|property| property.field == "authoring_base_location")
        .expect("base location row");
    assert_eq!(listed.label, "Base location");
    assert_eq!(
        listed.value,
        PropValue::Choice {
            selected: "Midpoint".into(),
            options: vec!["Startpoint".into(), "Midpoint".into()],
        }
    );
    let original = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_base_location",
        value: "Startpoint".into(),
    });
    assert_eq!(base_location(&app.tabs[i].scene.document), 0);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    for (handle, object) in &original.objects {
        if *handle != parameter {
            assert_eq!(app.tabs[i].scene.document.objects.get(handle), Some(object));
        }
    }
    app.undo_steps(1);
    assert_eq!(base_location(&app.tabs[i].scene.document), 1);
    app.redo_steps(1);
    assert_eq!(base_location(&app.tabs[i].scene.document), 0);
    app.tabs[i].selected_authoring = Some(parameter);
    let unchanged = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_base_location",
        value: "Startpoint".into(),
    });
    app.command_line.last_error = None;
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_base_location",
        value: "Diagonal".into(),
    });
    assert_eq!(app.tabs[i].scene.document, unchanged);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Choose Startpoint or Midpoint.")
    );
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_base_location",
        value: "Midpoint".into(),
    });
    let doc = &app.tabs[i].scene.document;
    for saved in [
        document::DwgReader::from_stream(Cursor::new(
            document::DwgWriter::write_to_vec(doc).unwrap(),
        ))
        .read()
        .unwrap(),
        document::DxfReader::from_reader(Cursor::new(
            document::DxfWriter::new(doc).write_to_vec().unwrap(),
        ))
        .unwrap()
        .read()
        .unwrap(),
    ] {
        let ObjectType::DynamicBlock(object) = &saved.objects[&parameter] else {
            panic!()
        };
        let DynamicBlockData::LinearParameter(p) = &object.data else {
            panic!()
        };
        assert_eq!(p.parameter.parameter_base_location, 1);
    }
    let before = app.tabs[i].scene.document.clone();
    assert!(crate::scene::block_distance_parameter::set_base_location(
        &mut app.tabs[i].scene.document,
        Handle::NULL,
        parameter,
        false
    )
    .is_err());
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "authoring_base_location",
        value: "Startpoint".into(),
    });
    assert_eq!(app.tabs[i].scene.document, before);
}

mod alignment_runtime;

#[test]
fn block_parameter_metadata_properties_history_save_and_discard() {
    use crate::scene::block_authoring::metadata::{self, Field};
    use crate::scene::block_distance_parameter::{self as distance, Kind};
    use crate::scene::model::object::PropValue;
    for kind in [Kind::XY, Kind::Polar] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let handle = distance::create(
            &mut app.tabs[i].scene.document,
            def,
            kind,
            Vector3::ZERO,
            Vector3::new(2., 3., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        app.tabs[i].selected_authoring = Some(handle);
        app.refresh_properties();
        let original = app.tabs[i].scene.document.clone();
        let fields = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .filter(|prop| metadata::Field::from_key(prop.field).is_some())
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 4);
        assert!(fields
            .iter()
            .all(|prop| matches!(prop.value, PropValue::PlainText(_))));
        let primary = metadata::read(&original, def, handle).unwrap().text[0]
            .value
            .to_owned();
        assert_eq!(fields[0].value, PropValue::PlainText(primary.clone()));
        let depth = app.tabs[i].history.undo_stack.len();
        for (field, value) in [
            ("authoring_name", primary.as_str()),
            ("authoring_name", " "),
            ("authoring_secondary_name", primary.as_str()),
        ] {
            let _ = app.update(Message::PropGeomInput {
                field,
                value: value.into(),
            });
            let _ = app.update(Message::PropGeomCommit(field));
            assert_eq!(app.tabs[i].scene.document, original);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        }
        for (field, value) in [
            ("authoring_name", "Travel"),
            ("authoring_secondary_name", "Secondary travel"),
            ("authoring_description", "  1 + 2 / literal text  "),
            ("authoring_secondary_description", "Second description"),
        ] {
            let _ = app.update(Message::PropGeomInput {
                field,
                value: value.into(),
            });
            let _ = app.update(Message::PropGeomCommit(field));
            assert_eq!(
                metadata::read(&app.tabs[i].scene.document, def, handle)
                    .unwrap()
                    .text
                    .into_iter()
                    .find(|text| text.field.key() == field)
                    .unwrap()
                    .value,
                value
            );
        }
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 4);
        let saved = app.tabs[i].scene.document.clone();
        app.undo_steps(4);
        assert_eq!(app.tabs[i].scene.document, original);
        app.redo_steps(4);
        assert_eq!(app.tabs[i].scene.document, saved);
        let _ = app.dispatch_command("BSAVE");
        app.tabs[i].selected_authoring = Some(handle);
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_description",
            value: "".into(),
        });
        let _ = app.update(Message::PropGeomCommit("authoring_description"));
        assert_eq!(
            metadata::read(&app.tabs[i].scene.document, def, handle)
                .unwrap()
                .text
                .into_iter()
                .find(|text| text.field == Field::Description)
                .unwrap()
                .value,
            ""
        );
        let _ = app.dispatch_command("BEDIT_DISCARD");
        begin(&mut app, reference);
        assert_eq!(
            metadata::read(&app.tabs[i].scene.document, def, handle)
                .unwrap()
                .text
                .into_iter()
                .find(|text| text.field == Field::Description)
                .unwrap()
                .value,
            "  1 + 2 / literal text  "
        );
    }
}

#[test]
fn block_visibility_metadata_flag_hides_property_preserving_native_grips_and_states() {
    use crate::scene::{block_authoring::metadata, block_grips, block_visibility};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let handle =
        block_visibility::create(&mut app.tabs[i].scene.document, def, Vector3::ZERO).unwrap();
    metadata::set_text(
        &mut app.tabs[i].scene.document,
        def,
        handle,
        metadata::Field::Name,
        "Display choice",
    )
    .unwrap();
    app.tabs[i].selected_authoring = Some(handle);
    app.refresh_properties();
    assert!(app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .any(|prop| prop.field == "authoring_show_properties"));
    assert_eq!(
        OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference)
            .unwrap()
            .label,
        "Display choice"
    );
    let before = app.tabs[i].scene.document.clone();
    let grips = block_grips::config::locations(&before, def, handle)
        .unwrap()
        .into_iter()
        .map(|g| (g.role, g.handle, g.point, g.key_point, g.direction))
        .collect::<Vec<_>>();
    let states = block_visibility::parameter(&before, def)
        .unwrap()
        .states
        .clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropBoolToggle("authoring_show_properties"));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert!(OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference).is_none());
    assert_eq!(
        block_grips::config::locations(&app.tabs[i].scene.document, def, handle)
            .unwrap()
            .into_iter()
            .map(|g| (g.role, g.handle, g.point, g.key_point, g.direction))
            .collect::<Vec<_>>(),
        grips
    );
    assert_eq!(
        block_visibility::parameter(&app.tabs[i].scene.document, def)
            .unwrap()
            .states,
        states
    );
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    assert!(OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference).is_some());
    app.redo_steps(1);
    assert!(OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference).is_none());
}

#[test]
fn block_reference_numeric_properties_history_scratch_expression_and_stale_guards() {
    use crate::scene::model::object::{BlockParameterComponent as Component, PropValue};
    use crate::scene::{
        block_actions as actions, block_distance_parameter as distance, block_grips,
    };
    use crate::ui::properties::FieldKey;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let def = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let parameter = distance::create(
            &mut app.tabs[source].scene.document,
            def,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(10., 0., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        let target = members(&app)[0];
        actions::create(
            &mut app.tabs[source].scene.document,
            def,
            actions::Kind::Move,
            parameter,
            &[target],
            glam::DVec3::ZERO,
        )
        .unwrap();
        block_grips::config::set(
            &mut app.tabs[source].scene.document,
            def,
            parameter,
            Some(0),
        )
        .unwrap();
        if testing {
            let _ = app.open_block_test(source);
        } else {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        }
        let source_before = app.tabs[source].scene.document.clone();
        let i = app.active_tab;
        let insert = app.tabs[i]
            .block_test
            .as_ref()
            .map_or(reference, |test| test.insert);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let field = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find_map(|property| match property.value {
                PropValue::BlockNumber { key, .. } if key.component == Component::Distance => {
                    Some(key)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(field.document, app.tabs[i].id);
        assert!(app.tabs[i]
            .properties
            .prop_field_key_for_id(&FieldKey::BlockParameter(field).widget_id())
            .is_some());
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        for (key, raw) in [
            (field, "10"),
            (field, "invalid"),
            (field, "1/0"),
            (field, "-4"),
            (
                crate::scene::model::object::BlockParameterKey {
                    document: field.document + 1,
                    ..field
                },
                "20",
            ),
        ] {
            let _ = app.update(Message::PropBlockInput {
                key,
                value: raw.into(),
            });
            let _ = app.update(Message::PropBlockCommit(key));
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        }
        let _ = app.update(Message::PropBlockInput {
            key: field,
            value: "10+5".into(),
        });
        let _ = app.update(Message::PropBlockCommit(field));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        assert!(!app.tabs[i]
            .properties
            .edit_buf
            .contains_key(&FieldKey::BlockParameter(field)));
        let changed = app.tabs[i].scene.document.clone();
        let p = actions::all_parameters(&before, def)
            .into_iter()
            .find(|p| p.handle == parameter)
            .unwrap();
        assert_eq!(
            actions::instance::frame(&changed, insert, &p).unwrap().end,
            glam::DVec3::new(15., 0., 0.)
        );
        let EntityType::Insert(reference) = changed.get_entity(insert).unwrap() else {
            panic!()
        };
        let line = changed
            .block_records
            .get(&reference.block_name)
            .unwrap()
            .entity_handles
            .iter()
            .find_map(|h| match changed.get_entity(*h) {
                Some(EntityType::Line(line)) => Some(line),
                _ => None,
            })
            .unwrap();
        assert_eq!(line.start, Vector3::new(5., 0., 0.));
        assert_eq!(line.end, Vector3::new(6., 0., 0.));
        app.undo_steps(1);
        let mut expected_undo = before.clone();
        // Entity undo preserves the monotonic handle allocator.
        while expected_undo.next_handle() < app.tabs[i].scene.document.next_handle() {
            expected_undo.allocate_handle();
        }
        assert_eq!(app.tabs[i].scene.document, expected_undo);
        app.redo_steps(1);
        assert_eq!(app.tabs[i].scene.document, changed);
        let _ = app.update(Message::PropBlockInput {
            key: field,
            value: "15".into(),
        });
        let _ = app.update(Message::PropBlockCommit(field));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut("0")
            .unwrap()
            .lock();
        let locked = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::PropBlockInput {
            key: field,
            value: "30".into(),
        });
        let _ = app.update(Message::PropBlockCommit(field));
        assert_eq!(app.tabs[i].scene.document, locked);
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
    }
}

#[test]
fn escape_reverts_a_pending_block_parameter_edit_and_keeps_the_selection() {
    use crate::scene::model::object::{BlockParameterComponent as Component, PropValue};
    use crate::scene::{block_actions as actions, block_distance_parameter as distance};
    use crate::ui::properties::FieldKey;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let def = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let parameter = distance::create(
        &mut app.tabs[source].scene.document,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    let target = members(&app)[0];
    actions::create(
        &mut app.tabs[source].scene.document,
        def,
        actions::Kind::Move,
        parameter,
        &[target],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let i = app.active_tab;
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let field = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find_map(|property| match property.value {
            PropValue::BlockNumber { key, .. } if key.component == Component::Distance => Some(key),
            _ => None,
        })
        .unwrap();
    let key = FieldKey::BlockParameter(field);
    app.tabs[i].properties.active_field = Some(key.clone());
    let _ = app.update(Message::PropBlockInput {
        key: field,
        value: "40".into(),
    });
    assert_eq!(
        app.tabs[i].properties.edit_buf.get(&key),
        Some(&"40".to_string())
    );
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::CommandEscape);
    assert_eq!(
        app.tabs[i].properties.edit_buf.get(&key),
        None,
        "the typed text is discarded"
    );
    assert_eq!(app.tabs[i].scene.document, before, "nothing is committed");
    assert_eq!(
        app.tabs[i].selected_handle,
        Some(reference),
        "the row keeps its reference selected"
    );
    assert_eq!(app.tabs[i].properties.active_field, Some(key));
}

#[test]
fn a_written_parameter_value_moves_the_reference_geometry() {
    use crate::scene::model::object::{BlockParameterComponent as Component, PropValue};
    use crate::scene::{block_actions as actions, block_distance_parameter as distance};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = distance::create(
        &mut app.tabs[i].scene.document,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    let target = members(&app)[0];
    actions::create(
        &mut app.tabs[i].scene.document,
        def,
        actions::Kind::Move,
        parameter,
        &[target],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let i = app.active_tab;
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let field = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find_map(|property| match property.value {
            PropValue::BlockNumber { key, .. } if key.component == Component::Distance => Some(key),
            _ => None,
        })
        .unwrap();
    let starts = |doc: &document::Document| {
        doc.entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(line) => Some(line.start),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let before = starts(&app.tabs[i].scene.document);
    let _ = app.update(Message::PropBlockInput {
        key: field,
        value: "40".into(),
    });
    let _ = app.update(Message::PropBlockCommit(field));
    let after = starts(&app.tabs[i].scene.document);
    let moved = |points: &[Vector3]| {
        points
            .iter()
            .any(|p| (p.x - 30.).abs() < 1e-9 && p.y.abs() < 1e-9)
    };
    assert!(
        !moved(&before),
        "nothing starts 30 along the parameter before the value is written"
    );
    assert!(
        moved(&after),
        "the written value moved a copy of the member 30 along the parameter"
    );
    let definition_member = match app.tabs[i].scene.document.get_entity(target) {
        Some(document::EntityType::Line(line)) => line.start,
        other => panic!("the definition member is not a plain line: {other:?}"),
    };
    assert_eq!(
        definition_member,
        Vector3::ZERO,
        "editing one reference leaves the definition's own member where it was authored"
    );
}

#[test]
fn block_reference_properties_keys_do_not_retarget_other_selections() {
    use crate::scene::model::object::PropValue;
    use crate::scene::{block_actions as actions, block_distance_parameter as distance};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = distance::create(
        &mut app.tabs[i].scene.document,
        def,
        distance::Kind::XY,
        Vector3::ZERO,
        Vector3::new(10., 10., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let other = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::new(20., 0., 0.),
        )))
        .unwrap();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let keys = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .filter_map(|p| match p.value {
            PropValue::BlockNumber { key, .. } => Some(key),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 2);
    assert_ne!(
        crate::ui::properties::FieldKey::BlockParameter(keys[0]).widget_id(),
        crate::ui::properties::FieldKey::BlockParameter(keys[1]).widget_id()
    );
    let _ = app.update(Message::PropBlockInput {
        key: keys[0],
        value: "20".into(),
    });
    app.tabs[i].scene.deselect_all();
    app.tabs[i].scene.select_entity(other, true);
    app.tabs[i].selected_handle = Some(other);
    app.refresh_properties();
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropBlockCommit(keys[0]));
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(actions::all_parameters(&before, def).len(), 1);
}

#[test]
fn block_parameter_without_actions_hides_runtime_grips_and_rejects_stale_activation() {
    use crate::scene::block_grips::{config, runtime};
    use crate::scene::model::object::PropValue;
    use crate::scene::{
        block_actions as actions, block_distance_parameter as distance,
        block_orientation_parameter as orientation, block_point_parameter as point,
    };
    use document::objects::{DynamicBlockData, ObjectType};
    for testing in [false, true] {
        for kind in 0..6 {
            let (mut app, reference) = fixture();
            begin(&mut app, reference);
            let source = app.active_tab;
            let def = app.tabs[source]
                .active_block_edit_session()
                .unwrap()
                .br_handle;
            let doc = &mut app.tabs[source].scene.document;
            let base = Vector3::new(2., 3., 0.);
            let end = Vector3::new(12., 13., 0.);
            let parameter = match kind {
                0 => point::create(doc, def, base, end),
                1..=3 => distance::create(
                    doc,
                    def,
                    match kind {
                        1 => distance::Kind::Linear,
                        2 => distance::Kind::Polar,
                        _ => distance::Kind::XY,
                    },
                    base,
                    end,
                    Vector3::new(4., 5., 0.),
                ),
                _ => orientation::create(
                    doc,
                    def,
                    if kind == 4 {
                        orientation::Kind::Rotation
                    } else {
                        orientation::Kind::Flip
                    },
                    base,
                    end,
                    end,
                ),
            }
            .unwrap();
            if testing {
                let _ = app.open_block_test(source);
            } else {
                let _ = app.dispatch_command("BSAVE");
                let _ = app.dispatch_command("BCLOSE");
            }
            let source_before = app.tabs[source].scene.document.clone();
            let i = app.active_tab;
            let insert = app.tabs[i]
                .block_test
                .as_ref()
                .map_or(reference, |test| test.insert);
            app.tabs[i].scene.select_entity(insert, true);
            app.tabs[i].selected_handle = Some(insert);
            app.refresh_properties();
            app.tabs[i].selected_grips.clear();
            app.tabs[i].selected_grip_handles.clear();
            app.refresh_test_action_grips([0.; 3]);
            assert!(!app.tabs[i]
                .selected_grips
                .iter()
                .any(|grip| runtime::decode(grip.id).is_some()));
            assert!(app.tabs[i]
                .properties
                .sections
                .iter()
                .flat_map(|section| &section.props)
                .any(|prop| matches!(
                    prop.value,
                    PropValue::BlockNumber { .. } | PropValue::BlockChoice { .. }
                )));
            let p = actions::all_parameters(&app.tabs[i].scene.document, def)
                .into_iter()
                .find(|p| p.handle == parameter)
                .unwrap();
            let before = app.tabs[i].scene.document.clone();
            let depth = app.tabs[i].history.undo_stack.len();
            let locations = config::locations(&before, def, parameter).unwrap();
            assert!(!locations.is_empty());
            for grip in locations {
                assert!(!app.start_test_action_grip(insert, runtime::id(p.id, grip.role).unwrap()));
                assert!(app.tabs[i].active_cmd.is_none());
            }
            let document = app.tabs[i].id;
            let _ =
                app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
                    grip: None,
                    document,
                    definition: def,
                    parameter,
                    insert: (!testing).then_some(insert),
                    value: if kind == 5 {
                        actions::Value::Flip(true)
                    } else {
                        actions::Value::Point(glam::DVec3::new(20., 20., 0.))
                    },
                });
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
            if testing {
                assert_eq!(app.tabs[source].scene.document, source_before);
            }
            // Adding a valid native association restores the same configured grip.
            let member = app.tabs[i]
                .scene
                .document
                .block_records
                .get("EDIT_ME")
                .unwrap()
                .entity_handles[0];
            let action = actions::create(
                &mut app.tabs[i].scene.document,
                def,
                match kind {
                    4 => actions::Kind::Rotate,
                    5 => actions::Kind::Flip,
                    _ => actions::Kind::Move,
                },
                parameter,
                &[member],
                glam::DVec3::ZERO,
            )
            .unwrap();
            let baseline = app.tabs[i].scene.document.clone();
            if let Some(test) = &mut app.tabs[i].block_test {
                test.baseline = std::sync::Arc::new(baseline);
            }
            app.tabs[i].selected_grips.clear();
            app.tabs[i].selected_grip_handles.clear();
            app.refresh_test_action_grips([0.; 3]);
            let grip = app.tabs[i]
                .selected_grips
                .iter()
                .find(|grip| runtime::decode(grip.id).is_some_and(|(node, _)| node == p.id))
                .unwrap()
                .id;
            assert!(actions::has_action(
                &app.tabs[i].scene.document,
                def,
                parameter
            ));
            if kind != 5 {
                assert!(app.start_test_action_grip(insert, grip));
                assert!(app.tabs[i].active_cmd.is_some());
                let _ = app.update(Message::CommandEscape);
            }
            // A retained command result cannot mutate values after disassociation.
            let ObjectType::DynamicBlock(object) =
                app.tabs[i].scene.document.objects.get_mut(&action).unwrap()
            else {
                panic!()
            };
            let action = match &mut object.data {
                DynamicBlockData::MoveAction(a) => &mut a.connections[0],
                DynamicBlockData::RotateAction(a) => &mut a.connections[0],
                DynamicBlockData::FlipAction(a) => &mut a.connections[0],
                _ => unreachable!(),
            };
            action.code = -1;
            let baseline = app.tabs[i].scene.document.clone();
            if let Some(test) = &mut app.tabs[i].block_test {
                test.baseline = std::sync::Arc::new(baseline);
            }
            let before = app.tabs[i].scene.document.clone();
            let _ =
                app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
                    grip: None,
                    document,
                    definition: def,
                    parameter,
                    insert: (!testing).then_some(insert),
                    value: if kind == 5 {
                        actions::Value::Flip(true)
                    } else {
                        actions::Value::Point(glam::DVec3::new(20., 20., 0.))
                    },
                });
            assert_eq!(app.tabs[i].scene.document, before);
        }
    }
}

#[test]
fn block_key_point_grip_eligibility_and_pending_result_follow_reassociation() {
    use crate::scene::{
        block_actions as actions, block_distance_parameter as distance,
        block_grips::{config, runtime},
    };
    use document::objects::{DynamicBlockData, ObjectType};
    use glam::DVec3;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let def = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let parameter = distance::create(
            &mut app.tabs[source].scene.document,
            def,
            distance::Kind::XY,
            Vector3::ZERO,
            Vector3::new(10., 10., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        let target = members(&app)[0];
        let action = actions::create(
            &mut app.tabs[source].scene.document,
            def,
            actions::Kind::Move,
            parameter,
            &[target],
            DVec3::ZERO,
        )
        .unwrap();
        config::set(
            &mut app.tabs[source].scene.document,
            def,
            parameter,
            Some(4),
        )
        .unwrap();
        crate::scene::block_authoring::metadata::set_visible(
            &mut app.tabs[source].scene.document,
            def,
            parameter,
            false,
        )
        .unwrap();
        if testing {
            let _ = app.open_block_test(source);
        } else {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        }
        let source_before = app.tabs[source].scene.document.clone();
        let i = app.active_tab;
        let insert = app.tabs[i]
            .block_test
            .as_ref()
            .map_or(reference, |test| test.insert);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.tabs[i].selected_grips.clear();
        app.tabs[i].selected_grip_handles.clear();
        app.refresh_test_action_grips([0.; 3]);
        let p = actions::all_parameters(&app.tabs[i].scene.document, def)
            .into_iter()
            .find(|p| p.handle == parameter)
            .unwrap();
        let end_id = runtime::id(p.id, 1).unwrap();
        assert_eq!(
            app.tabs[i]
                .selected_grips
                .iter()
                .filter_map(|grip| runtime::decode(grip.id))
                .collect::<Vec<_>>(),
            vec![(p.id, 1), (p.id, 2), (p.id, 3)]
        );
        assert_eq!(
            config::locations(&app.tabs[i].scene.document, def, parameter)
                .unwrap()
                .len(),
            4
        );
        assert!(!app.start_test_action_grip(insert, runtime::id(p.id, 0).unwrap()));
        assert!(app.start_test_action_grip(insert, end_id));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .unwrap()
                .block_parameter_grip_id(),
            Some(end_id)
        );
        let ObjectType::DynamicBlock(object) =
            app.tabs[i].scene.document.objects.get_mut(&action).unwrap()
        else {
            panic!()
        };
        let DynamicBlockData::MoveAction(movement) = &mut object.data else {
            panic!()
        };
        movement.connections[0].name = "BaseXDelta".into();
        movement.connections[1].name = "BaseYDelta".into();
        let baseline = app.tabs[i].scene.document.clone();
        if let Some(test) = &mut app.tabs[i].block_test {
            test.baseline = std::sync::Arc::new(baseline);
        }
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(20., 20., 0.)));
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        assert!(!app.start_test_action_grip(insert, end_id));
        app.tabs[i].selected_grips.clear();
        app.tabs[i].selected_grip_handles.clear();
        app.refresh_test_action_grips([0.; 3]);
        assert_eq!(
            app.tabs[i]
                .selected_grips
                .iter()
                .filter_map(|grip| runtime::decode(grip.id))
                .collect::<Vec<_>>(),
            vec![(p.id, 0), (p.id, 2), (p.id, 3)]
        );
        let base_id = runtime::id(p.id, 0).unwrap();
        assert!(app.start_test_action_grip(insert, base_id));
        let _ = app.update(Message::CommandEscape);
        assert!(runtime::action_grip_available(
            &app.tabs[i].scene.document,
            def,
            parameter,
            base_id
        ));
        assert!(!runtime::action_grip_available(
            &app.tabs[i].scene.document,
            def,
            parameter,
            end_id
        ));
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
    }
}

mod scalar_grip_bindings;

mod alignment_insertion;

mod parameter_preview;

mod action_parameter_first;
mod array_spacing_prompts;
mod lookup_expressions;
mod lookup_set_row;
mod parameter_type_prompts;
mod parameters_manager;
mod visibility_set_first_state;
