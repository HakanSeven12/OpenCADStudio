//! Text style manager engine: the draft keeps untouched values exact, list
//! operations keep typed values, and Close / Undo bring back the texts a
//! rename touched (#2413).

use super::*;
use crate::ui::font_picker::{FontChoice, FontPickerEvent};
use codec::entities::{AttributeDefinition, AttributeEntity, EntityType, Insert, MText, Text};
use codec::tables::{BlockRecord, TextStyle};
use codec::types::{Handle, Vector3};

fn exact_style(name: &str) -> TextStyle {
    let mut style = TextStyle::new(name);
    style.width_factor = 0.123456789;
    style.oblique_angle = 0.1;
    style.height = 2.123456789;
    style
}

/// A clean drawing with the style "Exact", a TEXT and an MTEXT using it.
fn app_with_exact_style() -> (OpenCADStudio, Handle, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let mut style = exact_style("Exact");
    style.handle = doc.allocate_handle();
    doc.text_styles.add(style).unwrap();
    let mut text = Text::with_value("A", Vector3::ZERO);
    text.style = "Exact".to_string();
    let text = doc.add_entity(EntityType::Text(text)).unwrap();
    let mut mtext = MText::with_value("B", Vector3::ZERO);
    mtext.style = "Exact".to_string();
    let mtext = doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    app.tabs[i].dirty = false;
    (app, text, mtext)
}

/// Pick `choice` in the text style manager's font picker.
pub(super) fn pick(choice: FontChoice) -> Message {
    Message::TextStyleFont(FontPickerEvent::Pick(choice))
}

fn stored(app: &OpenCADStudio, name: &str) -> Option<TextStyle> {
    app.tabs[app.active_tab]
        .scene
        .document
        .text_styles
        .get(name)
        .cloned()
}

fn style_of(app: &OpenCADStudio, handle: Handle) -> String {
    match app.tabs[app.active_tab].scene.document.get_entity(handle) {
        Some(EntityType::Text(text)) => text.style.clone(),
        Some(EntityType::MText(text)) => text.style.clone(),
        other => panic!("not a text: {other:?}"),
    }
}

fn open_on(app: &mut OpenCADStudio, name: &str) {
    let _ = app.update(Message::TextStyleDialogOpen);
    let _ = app.update(Message::TextStyleDialogSelect(name.to_string()));
}

fn rename(app: &mut OpenCADStudio, old: &str, new: &str) {
    let _ = app.update(Message::StyleRenameStart(StyleKind::Text, old.to_string()));
    let _ = app.update(Message::StyleRenameEdit(new.to_string()));
    let _ = app.update(Message::StyleRenameCommit(StyleKind::Text));
}

#[test]
fn open_apply_switch_and_close_keep_the_style_exact() {
    let (mut app, _, _) = app_with_exact_style();
    let i = app.active_tab;
    let depth = app.tabs[i].history.undo_stack.len();

    let before = app.tabs[i].scene.document.clone();
    open_on(&mut app, "Exact");
    let _ = app.update(Message::TextStyleApply);
    let _ = app.update(Message::TextStyleDialogSelect("Standard".to_string()));
    let _ = app.update(Message::TextStyleDialogSelect("Exact".to_string()));
    let _ = app.update(Message::TextStyleApply);
    app.close_active_modal();

    let mut expected = exact_style("Exact");
    expected.handle = stored(&app, "Exact").unwrap().handle;
    assert_eq!(stored(&app, "Exact"), Some(expected));
    // Not only the style: nothing in the drawing changed.
    assert!(app.tabs[i].scene.document == before, "the drawing changed");
    assert!(!app.tabs[i].dirty, "an untouched Apply must not dirty the tab");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
}

#[test]
fn a_changed_field_is_written_and_the_rest_stays_exact() {
    let (mut app, _, _) = app_with_exact_style();
    open_on(&mut app, "Exact");
    let _ = app.update(Message::TextStyleEdit {
        field: "height",
        value: "3.5".to_string(),
    });
    let _ = app.update(Message::TextStyleApply);

    let style = stored(&app, "Exact").unwrap();
    assert_eq!(style.height, 3.5);
    assert_eq!(style.width_factor, 0.123456789);
    assert_eq!(style.oblique_angle, 0.1);
    assert!(app.tabs[app.active_tab].dirty);
}

#[test]
fn rename_then_close_restores_the_texts() {
    let (mut app, text, mtext) = app_with_exact_style();
    open_on(&mut app, "Exact");
    rename(&mut app, "Exact", "Notes");
    assert_eq!(style_of(&app, text), "Notes");

    app.close_active_modal();

    assert!(stored(&app, "Exact").is_some());
    assert!(stored(&app, "Notes").is_none());
    assert_eq!(style_of(&app, text), "Exact");
    assert_eq!(style_of(&app, mtext), "Exact");
    assert!(!app.tabs[app.active_tab].dirty);
}

#[test]
fn rename_apply_then_one_undo_restores_the_texts() {
    let (mut app, text, mtext) = app_with_exact_style();
    open_on(&mut app, "Exact");
    rename(&mut app, "Exact", "Notes");
    let _ = app.update(Message::TextStyleApply);
    app.close_active_modal();
    assert_eq!(style_of(&app, mtext), "Notes");

    let _ = app.update(Message::Undo);

    assert!(stored(&app, "Exact").is_some());
    assert!(stored(&app, "Notes").is_none());
    assert_eq!(style_of(&app, text), "Exact");
    assert_eq!(style_of(&app, mtext), "Exact");

    let _ = app.update(Message::Redo);
    assert_eq!(style_of(&app, text), "Notes");
}

#[test]
fn copy_takes_the_value_typed_before_it() {
    let (mut app, _, _) = app_with_exact_style();
    open_on(&mut app, "Exact");
    let _ = app.update(Message::TextStyleEdit {
        field: "width",
        value: "0.8".to_string(),
    });
    let _ = app.update(Message::TextStyleDialogCopy);

    let copy = stored(&app, "Exact (1)").unwrap();
    assert_eq!(copy.width_factor, 0.8);
    assert_eq!(copy.height, 2.123456789);
    assert_eq!(stored(&app, "Exact").unwrap().width_factor, 0.8);
}

#[test]
fn rename_start_keeps_the_typed_value() {
    let (mut app, _, _) = app_with_exact_style();
    open_on(&mut app, "Exact");
    let _ = app.update(Message::TextStyleEdit {
        field: "height",
        value: "5".to_string(),
    });
    let _ = app.update(Message::StyleRenameStart(StyleKind::Text, "Exact".to_string()));
    assert_eq!(app.textstyle_draft.height.text, "5.0000");
}

#[test]
fn merged_xref_text_style_is_read_only() {
    let (mut app, _, _) = app_with_exact_style();
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .document
        .text_styles
        .add(exact_style("Plan|Notes"))
        .unwrap();
    let before = stored(&app, "Plan|Notes");

    open_on(&mut app, "Plan|Notes");
    assert!(app.selected_textstyle_is_read_only());
    let _ = app.update(Message::TextStyleEdit {
        field: "height",
        value: "9".to_string(),
    });
    let _ = app.update(Message::TextStyleToggle("backward"));
    let _ = app.update(Message::TextStyleDialogSetCurrent);
    let _ = app.update(Message::TextStyleDialogDelete);
    rename(&mut app, "Plan|Notes", "Mine");
    let _ = app.update(Message::TextStyleApply);

    assert_eq!(stored(&app, "Plan|Notes"), before);
    assert!(stored(&app, "Mine").is_none());
    assert_ne!(
        app.tabs[i].scene.document.header.current_text_style_name,
        "Plan|Notes"
    );
}

#[test]
fn a_value_that_cannot_be_applied_keeps_apply_off_until_fixed() {
    let (mut app, _, _) = app_with_exact_style();
    open_on(&mut app, "Exact");
    let _ = app.update(Message::TextStyleEdit {
        field: "width",
        value: "0".to_string(),
    });
    let _ = app.update(Message::TextStyleEdit {
        field: "height",
        value: "3.5".to_string(),
    });
    let _ = app.update(Message::TextStyleApply);

    assert_eq!(stored(&app, "Exact").unwrap().height, 2.123456789);
    assert!(!app.tabs[app.active_tab].dirty);
    assert_eq!(app.textstyle_draft.width.text, "0");
    assert_eq!(app.textstyle_draft.invalid_fields(), 1);

    let _ = app.update(Message::TextStyleEdit {
        field: "width",
        value: "0.8".to_string(),
    });
    let _ = app.update(Message::TextStyleApply);
    let style = stored(&app, "Exact").unwrap();
    assert_eq!((style.width_factor, style.height), (0.8, 3.5));
}

#[test]
fn a_new_style_starts_from_the_selected_one() {
    let (mut app, _, _) = app_with_exact_style();
    open_on(&mut app, "Exact");
    let _ = app.update(Message::TextStyleDialogNew);

    let new = app.textstyle_selected.clone();
    assert_ne!(new, "Exact");
    let mut expected = exact_style(&new);
    expected.handle = stored(&app, &new).unwrap().handle;
    assert_eq!(stored(&app, &new), Some(expected));
}

#[test]
fn use_big_font_clears_the_file_and_a_truetype_font_turns_it_off() {
    let (mut app, _, _) = app_with_exact_style();
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    doc.text_styles.get_mut("Exact").unwrap().big_font_file = "gbcbig.shx".to_string();
    open_on(&mut app, "Exact");
    assert!(app.textstyle_draft.big_font_on);

    let _ = app.update(Message::TextStyleToggle("use_big_font"));
    let _ = app.update(Message::TextStyleEdit {
        field: "bigfont",
        value: "ignored.shx".to_string(),
    });
    let _ = app.update(Message::TextStyleApply);
    assert!(stored(&app, "Exact").unwrap().big_font_file.is_empty());

    let _ = app.update(Message::TextStyleToggle("use_big_font"));
    let _ = app.update(Message::TextStyleEdit {
        field: "bigfont",
        value: "chineset.shx".to_string(),
    });
    let _ = app.update(pick(FontChoice::TrueType("Arial".to_string())));
    assert!(!app.textstyle_draft.big_font_on);
    let _ = app.update(Message::TextStyleApply);
    assert!(stored(&app, "Exact").unwrap().big_font_file.is_empty());
}

#[test]
fn the_window_names_the_field_to_fix_and_why() {
    let (mut app, _, _) = app_with_exact_style();
    open_on(&mut app, "Exact");
    let _ = app.update(Message::TextStyleEdit {
        field: "oblique",
        value: "90".to_string(),
    });

    let mut simulator = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1280.0, 800.0),
        app.view_main(),
    );
    for shown in [
        "1 field needs fixing before Apply.",
        "Must be between \u{2212}85° and 85°.",
    ] {
        assert!(simulator.find(shown).is_ok(), "{shown:?} is not shown");
    }
}

/// The style of every text a rename has to follow: the TEXT and MTEXT, an
/// attribute definition and a TEXT in a block definition, and an INSERT's
/// attribute.
fn styles_of(app: &OpenCADStudio, handles: &[Handle]) -> Vec<String> {
    let doc = &app.tabs[app.active_tab].scene.document;
    handles
        .iter()
        .map(|handle| match doc.get_entity(*handle) {
            Some(EntityType::Text(text)) => text.style.clone(),
            Some(EntityType::MText(text)) => text.style.clone(),
            Some(EntityType::AttributeDefinition(a)) => a.text_style.clone(),
            Some(EntityType::Insert(insert)) => insert.attributes[0].text_style.clone(),
            other => panic!("not a text: {other:?}"),
        })
        .collect()
}

#[test]
fn rename_reaches_attributes_inserts_and_block_texts_and_undo_brings_them_back() {
    let (mut app, text, mtext) = app_with_exact_style();
    let doc = &mut app.tabs[app.active_tab].scene.document;
    let mut block = BlockRecord::new("Title");
    block.handle = doc.allocate_handle();
    let owner = block.handle;
    doc.block_records.add(block).unwrap();
    let mut attdef = AttributeDefinition {
        text_style: "Exact".into(),
        ..Default::default()
    };
    attdef.common.owner_handle = owner;
    let attdef = doc
        .add_entity(EntityType::AttributeDefinition(attdef))
        .unwrap();
    let mut inner = Text::with_value("C", Vector3::ZERO);
    inner.style = "Exact".into();
    inner.common.owner_handle = owner;
    let inner = doc.add_entity(EntityType::Text(inner)).unwrap();
    if let Some(record) = doc.block_records.iter_mut().find(|r| r.handle == owner) {
        record.entity_handles.extend([attdef, inner]);
    }
    let mut insert = Insert::new("Title", Vector3::ZERO);
    insert.attributes.push(AttributeEntity {
        text_style: "Exact".into(),
        ..Default::default()
    });
    let insert = doc.add_entity(EntityType::Insert(insert)).unwrap();
    let all = [text, mtext, attdef, inner, insert];

    open_on(&mut app, "Exact");
    rename(&mut app, "Exact", "Notes");
    assert_eq!(styles_of(&app, &all), vec!["Notes"; 5]);
    app.close_active_modal();
    assert_eq!(styles_of(&app, &all), vec!["Exact"; 5], "Close restores");

    open_on(&mut app, "Exact");
    rename(&mut app, "Exact", "Notes");
    let _ = app.update(Message::TextStyleApply);
    app.close_active_modal();
    assert_eq!(styles_of(&app, &all), vec!["Notes"; 5]);
    let _ = app.update(Message::Undo);
    assert_eq!(styles_of(&app, &all), vec!["Exact"; 5], "one Undo restores");
}

#[test]
fn a_copy_or_new_style_from_a_referenced_one_is_the_drawings_own() {
    let (mut app, _, _) = app_with_exact_style();
    let doc = &mut app.tabs[app.active_tab].scene.document;
    let mut referenced = exact_style("PLAN|Notes");
    referenced.xref_dependent = true;
    referenced.handle = doc.allocate_handle();
    doc.text_styles.add(referenced).unwrap();

    open_on(&mut app, "PLAN|Notes");
    let _ = app.update(Message::TextStyleDialogCopy);
    assert_eq!(app.textstyle_selected, "Notes (1)");
    assert!(!app.selected_textstyle_is_read_only());
    assert!(!stored(&app, "Notes (1)").unwrap().xref_dependent);
    let _ = app.update(Message::StyleRenameEdit("Labels".to_string()));
    let _ = app.update(Message::StyleRenameCommit(StyleKind::Text));
    let _ = app.update(Message::TextStyleEdit {
        field: "height",
        value: "4".to_string(),
    });
    let _ = app.update(Message::TextStyleApply);
    assert_eq!(stored(&app, "Labels").unwrap().height, 4.0);

    let _ = app.update(Message::TextStyleDialogSelect("PLAN|Notes".to_string()));
    let _ = app.update(Message::TextStyleDialogNew);
    assert!(!app.selected_textstyle_is_read_only());
    assert!(stored(&app, "PLAN|Notes").unwrap().xref_dependent);
}

#[test]
fn a_value_that_cannot_be_applied_stays_and_the_list_operations_wait() {
    let (mut app, _, _) = app_with_exact_style();
    open_on(&mut app, "Exact");
    let _ = app.update(Message::TextStyleEdit {
        field: "width",
        value: "abc".to_string(),
    });
    let names = app.style_names(StyleKind::Text);
    let _ = app.update(Message::TextStyleDialogSelect("Standard".to_string()));
    let _ = app.update(Message::TextStyleDialogNew);
    let _ = app.update(Message::TextStyleDialogCopy);
    let _ = app.update(Message::TextStyleDialogDelete);
    let _ = app.update(Message::StyleRenameStart(
        StyleKind::Text,
        "Standard".to_string(),
    ));

    assert_eq!(app.textstyle_selected, "Exact");
    assert_eq!(app.textstyle_draft.width.text, "abc");
    assert_eq!(app.style_names(StyleKind::Text), names);
    assert!(app.style_rename.is_none());

    let mut simulator = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1280.0, 800.0),
        app.view_main(),
    );
    for button in ["New", "Copy", "Delete"] {
        let _ = simulator.click(button);
    }
    let pressed: Vec<_> = simulator
        .into_messages()
        .filter(|message| {
            matches!(
                message,
                Message::TextStyleDialogNew
                    | Message::TextStyleDialogCopy
                    | Message::TextStyleDialogDelete
            )
        })
        .collect();
    assert!(pressed.is_empty(), "the buttons are off: {pressed:?}");
}
