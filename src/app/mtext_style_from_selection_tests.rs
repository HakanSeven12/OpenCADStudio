//! A text style from the MTEXT editor's selection: "New style from
//! selection" and "Update <style> from selection" read what the selected
//! characters share of the values a style stores, the change reaches the
//! texts that follow the style, and the edited text then follows it (#2413).

use super::mtext_editor::{EditorMenu, MTextEditorState, ValueOrigin};
use super::*;
use crate::entities::text_support::TTF_FLAG_BOLD;
use crate::ui::font_picker::FontChoice;
use codec::entities::{EntityType, Text};
use codec::tables::TextStyle;
use codec::types::Vector3;
use glam::DVec3;

/// A drawing whose style "Notes" has a fixed height of 2.5, width 0.8 and a
/// 15° oblique angle, with one TEXT in it that follows all three; the editor
/// open on `raw` at `height` in "Notes".
fn editing(raw: &str, height: f64) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let doc = &mut app.tabs[app.active_tab].scene.document;
    let mut notes = TextStyle::new("Notes");
    notes.height = 2.5;
    notes.width_factor = 0.8;
    notes.oblique_angle = 15f64.to_radians();
    notes.handle = doc.allocate_handle();
    doc.text_styles.add(notes).unwrap();
    let mut text = Text::with_value("A", Vector3::ZERO);
    text.style = "Notes".to_string();
    text.height = 2.5;
    text.width_factor = 0.8;
    text.oblique_angle = 15f64.to_radians();
    doc.add_entity(EntityType::Text(text)).unwrap();
    doc.header.current_text_style_name = "Notes".to_string();
    app.tabs[app.active_tab].dirty = false;
    app.open_mtext_editor(DVec3::ZERO, None, raw, height, None);
    app
}

fn ed(app: &OpenCADStudio) -> &MTextEditorState {
    app.mtext_editor.as_ref().unwrap()
}

fn style(app: &OpenCADStudio, name: &str) -> Option<TextStyle> {
    app.tabs[app.active_tab]
        .scene
        .document
        .text_styles
        .get(name)
        .cloned()
}

/// Height, width and oblique (degrees) of the drawing's TEXT.
fn text_values(app: &OpenCADStudio) -> (f64, f64, f64) {
    app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .find_map(|e| match e {
            EntityType::Text(t) => Some((t.height, t.width_factor, t.oblique_angle.to_degrees())),
            _ => None,
        })
        .unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_selection_shares_only_the_values_all_its_characters_have() {
    let mut app = editing("\\fArial|b1|i0;\\W1.5;ab{\\Q20;c}{\\H2x;d}", 5.0);
    let values = ed(&app).selection_values();
    assert_eq!(
        values.font,
        Some((FontChoice::TrueType("Arial".into()), true, false))
    );
    assert!(values.width.is_some_and(|w| close(w, 1.2)));
    assert_eq!((values.height, values.oblique), (None, None));

    app.mtext_editor.as_mut().unwrap().sel = Some((0, 2));
    let values = ed(&app).selection_values();
    assert_eq!(values.height, Some(5.0));
    assert!(values.oblique.is_some_and(|o| close(o, 15.0)));
}

#[test]
fn a_new_style_takes_the_selection_and_the_text_then_follows_it() {
    let mut app = editing("\\fArial|b1|i0;\\W1.5;\\Q20;Text", 5.0);
    let _ = app.update(Message::MTextMenu(Some(EditorMenu::Style)));
    let _ = app.update(Message::MTextNewStyleOpen);
    assert_eq!(ed(&app).menu, Some(EditorMenu::NewStyle));
    assert_eq!(ed(&app).new_style_name, "Notes (1)");
    let _ = app.update(Message::MTextNewStyleName("Heading".into()));
    let _ = app.update(Message::MTextNewStyleCreate);

    let heading = style(&app, "Heading").expect("the new style");
    assert_eq!(heading.true_type_font, "Arial");
    assert_ne!(heading.true_type_font_flags & TTF_FLAG_BOLD, 0);
    assert!(close(heading.width_factor, 1.2));
    assert!(close(heading.oblique_angle.to_degrees(), 20.0));
    assert_eq!(heading.height, 5.0);
    // The source style and its texts are untouched.
    assert_eq!(style(&app, "Notes").unwrap().height, 2.5);
    assert!(close(text_values(&app).1, 0.8));

    let ed = ed(&app);
    assert_eq!((ed.style.as_str(), ed.menu), ("Heading", None));
    assert_eq!(ed.content.text(), "Text");
    for field in ["font", "width", "oblique"] {
        assert_eq!(ed.value_origin(field), ValueOrigin::Style, "{field}");
    }
    assert_eq!(ed.height_origin(), Some(ValueOrigin::Style));
    assert!(app.tabs[app.active_tab].dirty);
}

#[test]
fn updating_the_style_reaches_its_texts_in_one_undo_step() {
    let mut app = editing("a{\\Q20;b}c", 2.5);
    app.mtext_editor.as_mut().unwrap().sel = Some((1, 2));
    let _ = app.update(Message::MTextUpdateStyleFromSelection);

    let notes = style(&app, "Notes").unwrap();
    assert!(close(notes.oblique_angle.to_degrees(), 20.0));
    assert!((notes.height, notes.width_factor) == (2.5, 0.8));
    // The TEXT followed the old angle, so it takes the new one.
    assert!(close(text_values(&app).2, 20.0));
    // The `\Q` formats one character only, so it stays.
    assert_eq!(ed(&app).content.text(), "a{\\Q20;b}c");
    assert_eq!(ed(&app).oblique, "20");

    app.mtext_cancel();
    let _ = app.update(Message::Undo);
    assert!(close(
        style(&app, "Notes").unwrap().oblique_angle.to_degrees(),
        15.0
    ));
    assert!(close(text_values(&app).2, 15.0));
}

#[test]
fn a_whole_text_code_the_updated_style_repeats_goes() {
    let mut app = editing("\\W1.5;\\Q20;Text", 2.5);
    let _ = app.update(Message::MTextUpdateStyleFromSelection);
    let notes = style(&app, "Notes").unwrap();
    assert!(close(notes.width_factor, 1.2));
    assert!(close(notes.oblique_angle.to_degrees(), 20.0));
    assert_eq!(ed(&app).content.text(), "Text");
    assert_eq!(ed(&app).width, "1.2");
}

#[test]
fn values_the_selection_does_not_share_leave_the_style_as_it_was() {
    let mut app = editing("a{\\W2;b}{\\H2x;c}", 2.5);
    let before = style(&app, "Notes").unwrap();
    let _ = app.update(Message::MTextUpdateStyleFromSelection);
    assert_eq!(style(&app, "Notes").unwrap(), before);
    assert!(!app.tabs[app.active_tab].dirty);
}

#[test]
fn a_style_that_asks_for_the_height_keeps_asking() {
    let mut app = editing("Text", 5.0);
    let _ = app.update(Message::MTextStyle("Standard".into()));
    let _ = app.update(Message::MTextUpdateStyleFromSelection);
    assert_eq!(style(&app, "Standard").unwrap().height, 0.0);
}

#[test]
fn a_stroke_font_is_stored_as_its_shx_file() {
    let mut app = editing("\\Fromans.shx;Text", 2.5);
    let _ = app.update(Message::MTextNewStyleName("Roman".into()));
    let _ = app.update(Message::MTextNewStyleCreate);
    let roman = style(&app, "Roman").unwrap();
    assert_eq!(roman.font_file, "romans.shx");
    assert!(roman.true_type_font.is_empty());
}

#[test]
fn a_taken_name_and_a_referenced_style_are_refused() {
    let mut app = editing("\\Q20;Text", 2.5);
    let _ = app.update(Message::MTextNewStyleName("notes".into()));
    let _ = app.update(Message::MTextNewStyleCreate);
    assert!(!app.tabs[app.active_tab].dirty);

    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.text_styles.get_mut("Notes").unwrap().xref_dependent = true;
    let _ = app.update(Message::MTextStyle("Notes".into()));
    assert!(ed(&app).object_style.read_only);
    let _ = app.update(Message::MTextUpdateStyleFromSelection);
    assert!(close(
        style(&app, "Notes").unwrap().oblique_angle.to_degrees(),
        15.0
    ));
}

#[test]
fn the_editor_follows_an_apply_in_the_style_manager() {
    let mut app = editing("Text", 2.5);
    let _ = app.update(Message::MTextStyleManager);
    assert_eq!(app.textstyle_selected, "Notes");
    app.tabs[app.active_tab]
        .scene
        .document
        .text_styles
        .get_mut("Notes")
        .unwrap()
        .width_factor = 0.6;
    app.style_stage_commit();
    assert_eq!(ed(&app).width, "0.6");
}
