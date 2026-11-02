//! Apply carries a text style's new height, width and oblique angle over to
//! the texts that follow the style; values set on a text stay, texts in
//! external references stay, and one Undo brings everything back (#2413).

use super::*;
use codec::entities::{AttributeEntity, EntityType, Insert, MText, Text};
use codec::tables::{BlockRecord, TextStyle};
use codec::types::{Handle, Vector3};

/// A clean drawing with the style "Notes" (height 2.5, width 1).
fn app_with_notes() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let doc = &mut app.tabs[app.active_tab].scene.document;
    let mut style = TextStyle::new("Notes");
    style.height = 2.5;
    style.handle = doc.allocate_handle();
    doc.text_styles.add(style).unwrap();
    app
}

fn add_text(app: &mut OpenCADStudio, height: f64, width: f64, owner: Option<Handle>) -> Handle {
    let mut text = Text::with_value("A", Vector3::ZERO);
    text.style = "Notes".to_string();
    text.height = height;
    text.width_factor = width;
    if let Some(owner) = owner {
        text.common.owner_handle = owner;
    }
    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.add_entity(EntityType::Text(text)).unwrap()
}

fn add_mtext(app: &mut OpenCADStudio, value: &str, height: f64) -> Handle {
    let mut mtext = MText::with_value(value, Vector3::ZERO);
    mtext.style = "Notes".to_string();
    mtext.height = height;
    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap()
}

/// A block definition record; `xref` makes it an attached reference.
fn add_block(app: &mut OpenCADStudio, name: &str, xref: bool) -> Handle {
    let doc = &mut app.tabs[app.active_tab].scene.document;
    let mut block = BlockRecord::new(name);
    block.handle = doc.allocate_handle();
    block.flags.is_xref = xref;
    let handle = block.handle;
    doc.block_records.add(block).unwrap();
    handle
}

fn text(app: &OpenCADStudio, handle: Handle) -> (f64, f64) {
    match app.tabs[app.active_tab].scene.document.get_entity(handle) {
        Some(EntityType::Text(t)) => (t.height, t.width_factor),
        Some(EntityType::MText(t)) => (t.height, 0.0),
        other => panic!("not a text: {other:?}"),
    }
}

fn edit(app: &mut OpenCADStudio, field: &'static str, value: &str) {
    let _ = app.update(Message::TextStyleDialogOpen);
    let _ = app.update(Message::TextStyleDialogSelect("Notes".to_string()));
    let _ = app.update(Message::TextStyleEdit {
        field,
        value: value.to_string(),
    });
}

#[test]
fn a_new_height_reaches_the_texts_that_follow_it_and_one_undo_takes_it_back() {
    let mut app = app_with_notes();
    let block = add_block(&mut app, "Tag", false);
    let plain = add_text(&mut app, 2.5, 1.0, None);
    let own = add_text(&mut app, 5.0, 1.0, None);
    let mtext = add_mtext(&mut app, "B", 2.5);
    let in_block = add_text(&mut app, 2.5, 1.0, Some(block));
    let mut insert = Insert::new("Tag", Vector3::ZERO);
    insert.attributes.push(AttributeEntity {
        text_style: "Notes".to_string(),
        height: 2.5,
        ..Default::default()
    });
    let doc = &mut app.tabs[app.active_tab].scene.document;
    let insert = doc.add_entity(EntityType::Insert(insert)).unwrap();
    let attribute_height =
        |app: &OpenCADStudio| match app.tabs[app.active_tab].scene.document.get_entity(insert) {
            Some(EntityType::Insert(insert)) => insert.attributes[0].height,
            other => panic!("not an insert: {other:?}"),
        };

    edit(&mut app, "height", "3.5");
    let _ = app.update(Message::TextStyleApply);
    app.close_active_modal();

    assert_eq!(text(&app, plain).0, 3.5);
    assert_eq!(text(&app, mtext).0, 3.5);
    assert_eq!(text(&app, in_block).0, 3.5);
    assert_eq!(attribute_height(&app), 3.5);
    assert_eq!(text(&app, own).0, 5.0);

    let _ = app.update(Message::Undo);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(doc.text_styles.get("Notes").unwrap().height, 2.5);
    for handle in [plain, mtext, in_block] {
        assert_eq!(text(&app, handle).0, 2.5);
    }
    assert_eq!(attribute_height(&app), 2.5);
}

#[test]
fn a_new_width_reaches_text_at_the_old_width_and_mtext_without_its_own() {
    let mut app = app_with_notes();
    let plain = add_text(&mut app, 2.5, 1.0, None);
    let own = add_text(&mut app, 2.5, 0.7, None);
    add_mtext(&mut app, "B", 2.5);
    let own_mtext = add_mtext(&mut app, "\\W1.2;B", 2.5);

    edit(&mut app, "width", "0.8");
    let users = &app.textstyle_users;
    let old = app
        .text_style_baseline(stored_handle(&app))
        .unwrap()
        .clone();
    let changes = text_style_users::value_changes(users, &old, &app.textstyle_draft.resolved());
    // The plain TEXT and the MTEXT without a code follow; 0.7 and \W keep theirs.
    assert_eq!((changes[0].updated, changes[0].kept), (2, 2));

    let _ = app.update(Message::TextStyleApply);
    assert_eq!(text(&app, plain).1, 0.8);
    assert_eq!(text(&app, own).1, 0.7);
    match app.tabs[app.active_tab]
        .scene
        .document
        .get_entity(own_mtext)
    {
        Some(EntityType::MText(t)) => assert_eq!(t.value, "\\W1.2;B"),
        other => panic!("not an MTEXT: {other:?}"),
    }
}

fn stored_handle(app: &OpenCADStudio) -> Handle {
    let doc = &app.tabs[app.active_tab].scene.document;
    doc.text_styles.get("Notes").unwrap().handle
}

#[test]
fn a_first_fixed_height_leaves_the_existing_texts_alone() {
    let mut app = app_with_notes();
    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.text_styles.get_mut("Notes").unwrap().height = 0.0;
    let text_handle = add_text(&mut app, 1.8, 1.0, None);
    let mtext = add_mtext(&mut app, "B", 0.0);

    edit(&mut app, "height", "2.5");
    let _ = app.update(Message::TextStyleApply);

    assert_eq!(text(&app, text_handle).0, 1.8);
    assert_eq!(text(&app, mtext).0, 0.0);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(doc.text_styles.get("Notes").unwrap().height, 2.5);

    // A text placed now takes the style's height without asking for one.
    app.close_active_modal();
    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.header.current_text_style_name = "Notes".to_string();
    let result = app.automation_op(r#"{"op":"run","cmd":"TEXT 0,0 0 New"}"#);
    assert_eq!(result["added"].as_i64(), Some(1), "{result}");
    let doc = &app.tabs[app.active_tab].scene.document;
    let new = doc.entities().find_map(|entity| match entity {
        EntityType::Text(t) if t.value == "New" => Some(t.height),
        _ => None,
    });
    assert_eq!(new, Some(2.5));
}

#[test]
fn an_mtext_with_its_own_whole_text_height_keeps_it() {
    let mut app = app_with_notes();
    let own = add_mtext(&mut app, r"\H5;B", 2.5);
    let follows = add_mtext(&mut app, "B", 2.5);

    edit(&mut app, "height", "3.5");
    let _ = app.update(Message::TextStyleApply);

    assert_eq!(text(&app, own).0, 2.5);
    assert_eq!(text(&app, follows).0, 3.5);
}

#[test]
fn texts_in_an_external_reference_are_not_changed() {
    let mut app = app_with_notes();
    let xref = add_block(&mut app, "Site", true);
    let referenced = add_text(&mut app, 2.5, 1.0, Some(xref));
    edit(&mut app, "height", "3.5");
    let _ = app.update(Message::TextStyleApply);
    assert_eq!(text(&app, referenced).0, 2.5);
}

#[test]
fn close_without_apply_leaves_the_texts_as_they_were() {
    let mut app = app_with_notes();
    let plain = add_text(&mut app, 2.5, 1.0, None);
    edit(&mut app, "height", "3.5");
    // Switching styles writes the draft into the style, not into the texts.
    let _ = app.update(Message::TextStyleDialogSelect("Standard".to_string()));
    app.close_active_modal();
    assert_eq!(text(&app, plain).0, 2.5);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(doc.text_styles.get("Notes").unwrap().height, 2.5);
}

#[test]
fn the_window_says_how_many_texts_apply_updates() {
    let mut app = app_with_notes();
    add_text(&mut app, 2.5, 1.0, None);
    add_text(&mut app, 2.5, 1.0, None);
    add_text(&mut app, 5.0, 1.0, None);
    edit(&mut app, "height", "3.5");

    let mut simulator = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1280.0, 800.0),
        app.view_main(),
    );
    let shown = "Height: 2.5 → 3.5. 2 texts that follow the style are updated. 1 text with its own value keeps it.";
    assert!(simulator.find(shown).is_ok(), "{shown:?} is not shown");
}
