//! The `ATTDIA` insertion form: every attribute the placement would prompt for
//! is answered at once, and the answers go back through the parked insert.
//! With the switch off, the same placement still walks the command line.
use super::{Message, ModalKind, OpenCADStudio};
use crate::command::{CadCommand, CmdResult};
use crate::modules::insert::insert_block::InsertBlockCommand;
use crate::ui::window::insert_attr_dialog::InsertAttrMsg;
use codec::entities::AttributeDefinition;
use codec::types::Vector3;
use codec::EntityType;

pub(super) fn askable(tag: &str, prompt: &str, default: &str) -> AttributeDefinition {
    AttributeDefinition::new(tag.to_string(), prompt.to_string(), default.to_string())
}

fn member_line() -> EntityType {
    EntityType::Line(codec::entities::Line::from_points(
        Vector3::ZERO,
        Vector3::new(20.0, 0.0, 0.0),
    ))
}

pub(super) fn define(app: &mut OpenCADStudio, tab: usize, name: &str, attdefs: Vec<AttributeDefinition>) {
    let mut members = vec![member_line()];
    for (index, attdef) in attdefs.into_iter().enumerate() {
        members.push(EntityType::AttributeDefinition(AttributeDefinition {
            insertion_point: Vector3::new(1.0 + index as f64, 2.0, 0.0),
            height: 1.0,
            ..attdef
        }));
    }
    app.tabs[tab]
        .scene
        .define_block_raw(name, Vector3::ZERO, members);
}

/// Drive a placement up to the point where it needs the definition's
/// attributes, then let the host resolve them.
pub(super) fn park_insert(app: &mut OpenCADStudio, tab: usize, block: &str) {
    let mut cmd =
        InsertBlockCommand::classic_for_block(block.to_string(), Vec::new(), String::new());
    let _ = cmd.on_point(glam::DVec3::ZERO);
    let _ = cmd.on_text_input("2");
    let _ = cmd.on_text_input("2");
    assert!(
        matches!(cmd.on_text_input("0"), Some(CmdResult::AttreqNeeded { .. })),
        "the attributes are the last thing a placement asks for"
    );
    app.tabs[tab].active_cmd = Some(Box::new(cmd));
    let _ = app.apply_cmd_result(CmdResult::AttreqNeeded {
        block_name: block.to_string(),
    });
}

pub(super) fn placed_attributes(app: &OpenCADStudio, tab: usize, block: &str) -> Vec<(String, String)> {
    app.tabs[tab]
        .scene
        .document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Insert(ins) if ins.block_name == block => Some(
                ins.attributes
                    .iter()
                    .map(|attribute| (attribute.tag.clone(), attribute.value.clone()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn the_form_asks_every_attribute_at_once_and_the_answers_place_the_block() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    define(
        &mut app,
        i,
        "ATTDIA_ASK",
        vec![
            askable("TAG1", "Tag one", "100"),
            askable("TAG2", "Tag two", ""),
        ],
    );

    park_insert(&mut app, i, "ATTDIA_ASK");
    assert_eq!(
        app.active_modal,
        Some(ModalKind::InsertAttributes),
        "the insertion form opens instead of the prompt chain"
    );
    let rows = app
        .insert_attr_dialog
        .clone()
        .expect("the form holds the attributes it is asking for")
        .rows;
    assert_eq!(rows.len(), 2, "one row per attribute asked");
    assert_eq!(rows[0].label, "Tag one", "a row is labelled by its prompt");
    assert_eq!(
        rows[0].value, "100",
        "a row opens with the default the prompt would have offered"
    );

    let _ = app.update(Message::InsertAttrDialog(InsertAttrMsg::Value(
        0,
        "250".to_string(),
    )));
    let _ = app.update(Message::InsertAttrDialog(InsertAttrMsg::Value(
        1,
        "AB".to_string(),
    )));
    let _ = app.update(Message::InsertAttrDialog(InsertAttrMsg::Ok));

    assert_eq!(app.active_modal, None, "answering the form closes it");
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the form finishes the placement it was asked for"
    );
    assert_eq!(
        placed_attributes(&app, i, "ATTDIA_ASK"),
        vec![
            ("TAG1".to_string(), "250".to_string()),
            ("TAG2".to_string(), "AB".to_string())
        ],
        "each answer reaches the attribute of its row, in order"
    );
}

#[test]
fn cancelling_the_form_drops_the_placement() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    define(
        &mut app,
        i,
        "ATTDIA_CANCEL",
        vec![askable("TAG1", "Tag one", "")],
    );

    park_insert(&mut app, i, "ATTDIA_CANCEL");
    assert_eq!(app.active_modal, Some(ModalKind::InsertAttributes));
    let _ = app.update(Message::CloseModal);

    assert_eq!(app.active_modal, None, "the form is gone");
    assert!(
        app.insert_attr_dialog.is_none(),
        "the answers are taken with the form"
    );
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the parked placement is dropped, not left waiting"
    );
    assert!(
        placed_attributes(&app, i, "ATTDIA_CANCEL").is_empty(),
        "nothing was placed"
    );
}

#[test]
fn a_constant_attribute_keeps_its_place_outside_the_form() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut constant = askable("TAGC", "Tag constant", "42");
    constant.flags.constant = true;
    define(
        &mut app,
        i,
        "ATTDIA_MIXED",
        vec![constant, askable("TAG1", "Tag one", "")],
    );

    park_insert(&mut app, i, "ATTDIA_MIXED");
    let rows = app
        .insert_attr_dialog
        .clone()
        .expect("the mixed definition still asks for its one answerable attribute")
        .rows;
    assert_eq!(
        rows.len(),
        1,
        "a constant attribute takes its definition's value and is never asked"
    );
    assert_eq!(rows[0].label, "Tag one");

    let _ = app.update(Message::InsertAttrDialog(InsertAttrMsg::Value(
        0,
        "7".to_string(),
    )));
    let _ = app.update(Message::InsertAttrDialog(InsertAttrMsg::Ok));
    assert_eq!(
        placed_attributes(&app, i, "ATTDIA_MIXED"),
        vec![
            ("TAGC".to_string(), "42".to_string()),
            ("TAG1".to_string(), "7".to_string())
        ],
        "the definition's value carries the attribute nobody was asked about"
    );
}

#[test]
fn with_the_form_off_the_placement_prompts_on_the_command_line() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.document.header.attribute_dialog = false;
    define(
        &mut app,
        i,
        "ATTDIA_OFF",
        vec![askable("TAG1", "Tag one", "")],
    );

    park_insert(&mut app, i, "ATTDIA_OFF");
    assert_eq!(
        app.active_modal, None,
        "the form is only in front of the chain when the switch is on"
    );
    assert!(
        app.insert_attr_dialog.is_none(),
        "no form is prepared for the placement"
    );
    let result = app.tabs[i]
        .active_cmd
        .as_mut()
        .expect("the placement waits for the value at the command line")
        .on_text_input("VALUE1")
        .expect("the value finishes the placement");
    let _ = app.apply_cmd_result(result);
    assert_eq!(
        placed_attributes(&app, i, "ATTDIA_OFF"),
        vec![("TAG1".to_string(), "VALUE1".to_string())],
        "the prompted answer reaches the same attribute the form would have set"
    );
}

/// The measured form: the block name on its own line, one captioned field per
/// attribute, and the five footer buttons with the steppers disabled.
#[test]
fn the_form_shows_the_block_its_prompts_and_its_buttons() {
    use crate::ui::modal::ModalSizing;
    use crate::ui::window::insert_attr_dialog::{InsertAttrRow, InsertAttrState};
    use iced_test::Simulator;
    let state = InsertAttrState {
        block_name: "ATTR_REF2".to_string(),
        rows: vec![InsertAttrRow {
            tag: "WIDTH".to_string(),
            label: "Width".to_string(),
            value: "100".to_string(),
        }],
    };
    let mut window = Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(500.0, 520.0),
        crate::ui::window::insert_attr_dialog::view(&state, ModalSizing::INTRINSIC),
    );
    for shown in [
        "Block name:",
        "ATTR_REF2",
        "Width:",
        "100",
        "OK",
        "Cancel",
        "Previous",
        "Next",
        "Help",
    ] {
        assert!(
            window.find(shown.to_string()).is_ok(),
            "{shown:?} is not shown"
        );
    }
}

#[test]
fn with_nothing_asked_the_placement_takes_its_defaults_and_no_form_appears() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.document.header.attribute_request = false;
    define(
        &mut app,
        i,
        "ATTREQ_QUIET",
        vec![askable("TAG1", "Tag one", "100")],
    );

    park_insert(&mut app, i, "ATTREQ_QUIET");
    assert_eq!(
        app.active_modal, None,
        "nothing is asked for, so the form stays out of the way"
    );
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the placement finishes on its own"
    );
    assert_eq!(
        placed_attributes(&app, i, "ATTREQ_QUIET"),
        vec![("TAG1".to_string(), "100".to_string())],
        "the definition's default carries the attribute"
    );
}

#[test]
fn an_attribute_without_a_prompt_is_asked_for_by_its_tag() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    define(
        &mut app,
        i,
        "ATTDIA_NOTAG",
        vec![askable("HEIGHT", "", "12")],
    );

    park_insert(&mut app, i, "ATTDIA_NOTAG");
    let rows = app
        .insert_attr_dialog
        .clone()
        .expect("the attribute is still asked for")
        .rows;
    assert_eq!(
        rows[0].label, "HEIGHT",
        "with no prompt to show, the row names the attribute by its tag"
    );
    assert_eq!(rows[0].value, "12", "and opens on its default");
}

#[test]
fn closing_the_tab_behind_the_form_leaves_no_form_behind() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    define(&mut app, i, "ATTDIA_CLOSING", vec![askable("TAG1", "", "7")]);
    park_insert(&mut app, i, "ATTDIA_CLOSING");
    assert_eq!(app.active_modal, Some(ModalKind::InsertAttributes));

    let id = app.tabs[i].id;
    app.tabs[i].dirty = false;
    let _ = app.update(Message::TabClose(id));
    assert_ne!(
        app.active_modal,
        Some(ModalKind::InsertAttributes),
        "the placement the form answers belonged to the tab that just closed"
    );
}
