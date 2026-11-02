//! The MTEXT editor follows the text style: the font, height, width and
//! oblique fields show the style's values and say where each comes from,
//! "Reset to style" drops a value of the text's own, and a new style brings
//! its values (#2413).

use super::mtext_editor::{MTextEditorState, ValueOrigin};
use super::*;
use codec::tables::TextStyle;
use glam::DVec3;

/// A drawing whose current style "Notes" has a fixed height of 2.5, width
/// 0.8 and a 15° oblique angle, and "Tall" a fixed height of 3.5; the
/// editor open on `raw` at `height`.
fn editing(raw: &str, height: f64) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    let doc = &mut app.tabs[1].scene.document;
    let mut notes = TextStyle::new("Notes");
    notes.height = 2.5;
    notes.width_factor = 0.8;
    notes.oblique_angle = 15f64.to_radians();
    notes.handle = doc.allocate_handle();
    doc.text_styles.add(notes).unwrap();
    let mut tall = TextStyle::new("Tall");
    tall.height = 3.5;
    tall.handle = doc.allocate_handle();
    doc.text_styles.add(tall).unwrap();
    doc.header.current_text_style_name = "Notes".to_string();
    app.open_mtext_editor(DVec3::ZERO, None, raw, height, None);
    app
}

fn ed(app: &OpenCADStudio) -> &MTextEditorState {
    app.mtext_editor.as_ref().unwrap()
}

fn raw(app: &OpenCADStudio) -> String {
    ed(app).content.text()
}

fn origins(app: &OpenCADStudio) -> [Option<ValueOrigin>; 4] {
    let ed = ed(app);
    [
        Some(ed.value_origin("font")),
        ed.height_origin(),
        Some(ed.value_origin("width")),
        Some(ed.value_origin("oblique")),
    ]
}

#[test]
fn a_new_text_shows_the_style_values_by_style() {
    let app = editing("Text", 2.5);
    let ed = ed(&app);
    assert_eq!(
        (ed.height.as_str(), ed.width.as_str(), ed.oblique.as_str()),
        ("2.5", "0.8", "15")
    );
    assert_eq!(origins(&app), [Some(ValueOrigin::Style); 4]);
}

#[test]
fn own_values_are_custom_and_partial_ones_mixed() {
    let app = editing("\\fArial|b0|i0;\\W1.2;\\Q20;Text", 5.0);
    assert_eq!(origins(&app), [Some(ValueOrigin::Custom); 4]);
    assert_eq!(ed(&app).oblique, "20");

    let app = editing("a{\\Q20;b}{\\H2x;c}", 2.5);
    assert_eq!(ed(&app).height_origin(), Some(ValueOrigin::Mixed));
    assert_eq!(ed(&app).value_origin("oblique"), ValueOrigin::Mixed);
}

#[test]
fn reset_to_style_drops_the_whole_text_codes_and_sets_the_style_height() {
    let mut app = editing("\\fArial|b0|i0;\\W1.2;\\Q20;Te{\\Q5;x}t", 5.0);
    for field in ["font", "height", "width", "oblique"] {
        let _ = app.update(Message::MTextResetToStyle(field));
    }
    // The run with its own oblique keeps it.
    assert_eq!(raw(&app), "Te{\\Q5;x}t");
    let ed = ed(&app);
    assert_eq!(
        (ed.height.as_str(), ed.width.as_str(), ed.oblique.as_str()),
        ("2.5", "0.8", "15")
    );
    assert_eq!(ed.height_origin(), Some(ValueOrigin::Style));
}

#[test]
fn typing_the_style_oblique_writes_no_code_and_zero_writes_one() {
    let mut app = editing("Text", 2.5);
    let _ = app.update(Message::MTextOblique("0".to_string()));
    assert_eq!(raw(&app), "\\Q0;Text");
    let _ = app.update(Message::MTextOblique("15".to_string()));
    assert_eq!(raw(&app), "Text");
}

#[test]
fn a_new_style_brings_its_height_and_oblique_to_the_fields_that_follow() {
    let mut app = editing("Text", 2.5);
    let _ = app.update(Message::MTextStyle("Tall".to_string()));
    let shown = ed(&app);
    assert_eq!(
        (shown.height.as_str(), shown.oblique.as_str()),
        ("3.5", "0")
    );
    assert_eq!(origins(&app), [Some(ValueOrigin::Style); 4]);

    // A height of the text's own stays when the style changes.
    let mut app = editing("Text", 5.0);
    let _ = app.update(Message::MTextStyle("Tall".to_string()));
    assert_eq!(ed(&app).height, "5");
}

#[test]
fn a_right_click_on_a_custom_field_offers_reset_to_style() {
    use iced::mouse::{Button, Event as MouseEvent};
    use iced_test::simulator::Simulator;
    let app = editing("\\Q20;Text", 2.5);
    let mut ui = Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1280.0, 800.0),
        app.view_main(),
    );
    let caption = crate::t!("Oblique").into_owned();
    let bounds = ui.find(caption).unwrap().visible_bounds().unwrap();
    ui.point_at(bounds.center());
    ui.simulate([
        iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Right)),
        iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Right)),
    ]);
    let _ = ui.click("Reset to style (15°)");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::MTextResetToStyle("oblique"))),
        "{messages:?}"
    );
}

#[test]
fn a_font_picked_for_the_text_leaves_the_style_alone() {
    use crate::ui::font_picker::{FontChoice, FontPickerEvent};
    let mut app = editing("Text", 2.5);
    let before = app.tabs[1].scene.document.text_styles.get("Notes").cloned();
    for font in [
        FontChoice::Stroke("simplex.shx".to_string()),
        FontChoice::TrueType("Arial".to_string()),
    ] {
        let _ = app.update(Message::MTextFontPicker(FontPickerEvent::Pick(font)));
    }
    assert!(raw(&app).contains(r"\f"), "{}", raw(&app));
    assert!(app.mtext_commit());

    let doc = &app.tabs[1].scene.document;
    assert_eq!(doc.text_styles.get("Notes").cloned(), before);
}
