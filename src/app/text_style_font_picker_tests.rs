//! The shared font picker: what it lists, how the search narrows it, and
//! what a pick writes into the text style or the MTEXT (#2413).

use super::text_style_manager_tests::pick;
use super::*;
use crate::entities::text_support::{TTF_FLAG_BOLD, TTF_FLAG_ITALIC};
use crate::ui::font_picker::{
    catalogue, FaceStyle, FontChoice, FontPickerEvent, FontPickerState, Section,
};
use codec::tables::TextStyle;

fn style(name: &str, font_file: &str, family: &str) -> TextStyle {
    let mut style = TextStyle::new(name);
    style.font_file = font_file.to_string();
    style.true_type_font = family.to_string();
    style
}

/// A clean drawing with these text styles added.
fn app_with_styles(styles: Vec<TextStyle>) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let doc = &mut app.tabs[app.active_tab].scene.document;
    for mut style in styles {
        style.handle = doc.allocate_handle();
        doc.text_styles.add(style).unwrap();
    }
    app
}

fn open_on(app: &mut OpenCADStudio, name: &str) {
    let _ = app.update(Message::TextStyleDialogOpen);
    let _ = app.update(Message::TextStyleDialogSelect(name.to_string()));
}

#[test]
fn the_drawing_fonts_come_first_and_a_missing_one_is_marked() {
    let app = app_with_styles(vec![
        style("Plan", "vnarial.shx", ""),
        style("Notes", "romans.shx", ""),
        style("Office", "nosuchfont.ttf", "No Such Family"),
    ]);
    let doc = &app.tabs[app.active_tab].scene.document;
    let entries = catalogue(doc, false);
    let drawing: Vec<_> = entries
        .iter()
        .take_while(|entry| entry.section == Section::Drawing)
        .map(|entry| (entry.choice.label().into_owned(), entry.missing.is_some()))
        .collect();
    assert!(
        drawing.contains(&("vnarial.shx".to_string(), true)),
        "{drawing:?}"
    );
    assert!(
        drawing.contains(&("RomanS".to_string(), false)),
        "{drawing:?}"
    );
    assert!(
        drawing.contains(&("No Such Family".to_string(), true)),
        "{drawing:?}"
    );
    // The new drawing's Standard style uses txt.shx: listed once, by name.
    assert_eq!(
        drawing
            .iter()
            .filter(|(label, _)| label == "Standard")
            .count(),
        1
    );
    // The bundled stroke fonts follow, each by the real SHX file it stores.
    let stroke: Vec<_> = entries
        .iter()
        .filter(|entry| entry.section == Section::Stroke)
        .map(|entry| entry.choice.clone())
        .collect();
    assert!(stroke.contains(&FontChoice::Stroke("isocp.shx".to_string())));
    assert!(stroke
        .iter()
        .all(|choice| matches!(choice, FontChoice::Stroke(file) if file.ends_with(".shx"))));
}

#[test]
fn the_search_finds_a_font_by_name_or_file_and_offers_what_it_lacks() {
    let app = app_with_styles(Vec::new());
    let doc = &app.tabs[app.active_tab].scene.document;
    let mut picker = FontPickerState::default();
    let _ = picker.update(FontPickerEvent::Search("isocp".to_string()), doc, false);
    assert!(picker.open);
    let found: Vec<_> = picker.matches().iter().map(|e| e.choice.label()).collect();
    assert!(found.iter().any(|label| label == "ISO"), "{found:?}");

    let _ = picker.update(FontPickerEvent::Search("Simplex".to_string()), doc, false);
    assert!(picker
        .matches()
        .iter()
        .any(|entry| entry.choice == FontChoice::Stroke("simplex.shx".to_string())));

    // A file nobody lists can still be named: it is offered as typed.
    let typed = "company.shx".to_string();
    let _ = picker.update(FontPickerEvent::Search(typed.clone()), doc, false);
    let picked = picker.update(
        FontPickerEvent::Pick(FontChoice::Stroke(typed.clone())),
        doc,
        false,
    );
    assert_eq!(picked, Some(FontChoice::Stroke(typed)));
    assert!(!picker.open && picker.query.is_empty());
}

#[test]
fn a_pick_in_the_manager_writes_the_file_and_closes_the_list() {
    let mut app = app_with_styles(vec![style("Notes", "txt.shx", "")]);
    open_on(&mut app, "Notes");
    let _ = app.update(Message::TextStyleFont(FontPickerEvent::Toggle));
    assert!(app.textstyle_font_picker.open);

    let _ = app.update(pick(FontChoice::Stroke("isocp.shx".to_string())));
    assert!(!app.textstyle_font_picker.open);
    let _ = app.update(Message::TextStyleApply);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(doc.text_styles.get("Notes").unwrap().font_file, "isocp.shx");
}

#[test]
fn the_face_style_sets_bold_and_italic_and_keeps_the_other_bits() {
    const CHARSET_PITCH: i32 = 34;
    let mut notes = style("Notes", "arial.ttf", "Arial");
    notes.true_type_font_flags = CHARSET_PITCH | TTF_FLAG_BOLD;
    let mut app = app_with_styles(vec![notes]);
    open_on(&mut app, "Notes");
    let _ = app.update(Message::TextStyleFaceStyle(FaceStyle::Italic));
    let _ = app.update(Message::TextStyleApply);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(
        doc.text_styles.get("Notes").unwrap().true_type_font_flags,
        CHARSET_PITCH | TTF_FLAG_ITALIC
    );
}

#[test]
fn a_face_this_machine_lacks_stays_offered_when_the_style_has_it() {
    let offered = FaceStyle::offered("No Such Family", FaceStyle::BoldItalic);
    assert_eq!(offered, [FaceStyle::Regular, FaceStyle::BoldItalic]);
}

#[test]
fn the_open_list_shows_its_sections_in_the_window() {
    let mut app = app_with_styles(vec![style("Notes", "txt.shx", "")]);
    open_on(&mut app, "Notes");
    let _ = app.update(Message::TextStyleFont(FontPickerEvent::Toggle));
    let mut simulator = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1280.0, 800.0),
        app.view_main(),
    );
    for shown in ["IN THIS DRAWING", "STROKE FONTS (SHX)", "isocp.shx"] {
        assert!(simulator.find(shown).is_ok(), "{shown:?} is not shown");
    }
}

#[test]
fn the_mtext_picker_writes_the_shx_file_the_style_would_store() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.open_mtext_editor(glam::DVec3::ZERO, None, "Text", 2.5, None);
    let _ = app.update(Message::MTextFontPicker(FontPickerEvent::Pick(
        FontChoice::Stroke("simplex.shx".to_string()),
    )));
    let ed = app.mtext_editor.as_ref().unwrap();
    assert!(
        ed.content.text().contains("\\Fsimplex.shx;"),
        "{}",
        ed.content.text()
    );
    assert_eq!(FontChoice::of_name(&ed.font_field()).label(), "Simplex");

    let _ = app.update(Message::MTextFontPicker(FontPickerEvent::Pick(
        FontChoice::StyleDefault,
    )));
    let ed = app.mtext_editor.as_ref().unwrap();
    assert!(!ed.content.text().contains("\\F"), "{}", ed.content.text());
}

#[test]
fn a_bare_stroke_name_in_the_drawing_is_listed_as_its_shx_file() {
    let app = app_with_styles(vec![style("Old", "txt", ""), style("Roman", "RomanS", "")]);
    let doc = &app.tabs[app.active_tab].scene.document;
    let drawing: Vec<_> = catalogue(doc, false)
        .into_iter()
        .take_while(|entry| entry.section == Section::Drawing)
        .map(|entry| entry.choice)
        .collect();
    assert!(
        drawing.contains(&FontChoice::Stroke("txt.shx".to_string())),
        "{drawing:?}"
    );
    assert!(
        drawing.contains(&FontChoice::Stroke("romans.shx".to_string())),
        "{drawing:?}"
    );
    assert!(
        !drawing.contains(&FontChoice::Stroke("txt".to_string())),
        "{drawing:?}"
    );
}
