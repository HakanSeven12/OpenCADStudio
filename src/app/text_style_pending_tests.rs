//! The text style manager's summary before Apply lists every style the
//! session changed, not only the selected one, from the same changes Apply
//! writes; the list keeps long names and large counts readable (#2413).

use super::text_style_users::{Property, StyleChange};
use super::*;
use codec::entities::{EntityType, MText, Text};
use codec::tables::TextStyle;
use codec::types::{Handle, Vector3};
use iced_test::Simulator;

/// A clean drawing with the styles `names`, each height 2.5 and width 1.
fn drawing(names: &[&str]) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let doc = &mut app.tabs[app.active_tab].scene.document;
    for name in names {
        let mut style = TextStyle::new(*name);
        style.height = 2.5;
        style.handle = doc.allocate_handle();
        doc.text_styles.add(style).unwrap();
    }
    app
}

fn add_text(app: &mut OpenCADStudio, style: &str, height: f64, width: f64) -> Handle {
    let mut text = Text::with_value("A", Vector3::ZERO);
    text.style = style.to_string();
    text.height = height;
    text.width_factor = width;
    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.add_entity(EntityType::Text(text)).unwrap()
}

fn add_mtext(app: &mut OpenCADStudio, style: &str, value: &str) -> Handle {
    let mut mtext = MText::with_value(value, Vector3::ZERO);
    mtext.style = style.to_string();
    mtext.height = 2.5;
    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap()
}

fn open(app: &mut OpenCADStudio) {
    let _ = app.update(Message::TextStyleDialogOpen);
}

fn select(app: &mut OpenCADStudio, name: &str) {
    let _ = app.update(Message::TextStyleDialogSelect(name.to_string()));
}

fn edit(app: &mut OpenCADStudio, field: &'static str, value: &str) {
    let _ = app.update(Message::TextStyleEdit {
        field,
        value: value.to_string(),
    });
}

fn pending(app: &OpenCADStudio) -> Vec<StyleChange> {
    app.textstyle_pending_changes()
}

fn names(changes: &[StyleChange]) -> Vec<&str> {
    changes.iter().map(|c| c.name.as_str()).collect()
}

fn lines(change: &StyleChange) -> Vec<String> {
    super::text_style_users::summary_lines(change)
}

fn style<'a>(app: &'a OpenCADStudio, name: &str) -> &'a TextStyle {
    app.tabs[app.active_tab]
        .scene
        .document
        .text_styles
        .get(name)
        .unwrap()
}

fn text(app: &OpenCADStudio, handle: Handle) -> (f64, f64) {
    match app.tabs[app.active_tab].scene.document.get_entity(handle) {
        Some(EntityType::Text(t)) => (t.height, t.width_factor),
        other => panic!("not a text: {other:?}"),
    }
}

fn window(app: &OpenCADStudio, width: f32, height: f32) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(width, height),
        app.view_main(),
    )
}

fn shown(simulator: &mut Simulator<'_, Message>, text: &str) -> iced::Rectangle {
    simulator
        .find(text.to_string())
        .unwrap_or_else(|_| panic!("{text:?} is not shown"))
        .visible_bounds()
        .unwrap_or_else(|| panic!("{text:?} is not visible"))
}

#[test]
fn one_property_of_one_style() {
    let mut app = drawing(&["Notes"]);
    add_text(&mut app, "Notes", 2.5, 1.0);
    add_text(&mut app, "Notes", 2.5, 1.0);
    open(&mut app);
    select(&mut app, "Notes");
    edit(&mut app, "height", "3.5");

    let changes = pending(&app);
    assert_eq!(names(&changes), ["Notes"]);
    assert_eq!(changes[0].properties.len(), 1);
    assert_eq!(
        lines(&changes[0]),
        ["Height: 2.5 → 3.5. 2 texts that follow the style are updated."]
    );
}

#[test]
fn several_properties_of_one_style() {
    let mut app = drawing(&["Notes"]);
    add_text(&mut app, "Notes", 2.5, 1.0);
    open(&mut app);
    select(&mut app, "Notes");
    edit(&mut app, "height", "3.5");
    edit(&mut app, "width", "0.8");
    let _ = app.update(Message::TextStyleToggle("backward"));
    let _ = app.update(Message::TextStyleToggle("annotative"));

    let changes = pending(&app);
    let properties: Vec<Property> = changes[0].properties.iter().map(|p| p.property).collect();
    assert_eq!(
        properties,
        [
            Property::Height,
            Property::Width,
            Property::Annotative,
            Property::Backward
        ]
    );
    let lines = lines(&changes[0]);
    assert!(lines[2].starts_with("Annotative: Off → On. Existing texts keep"));
    assert_eq!(lines[3], "Backward: Off → On.");
    // Backward is drawn from the style: the one text is redrawn.
    assert_eq!(
        lines[4],
        "1 text that uses the style is redrawn; formatting set inside MTEXT stays."
    );
}

#[test]
fn three_styles_stay_listed_when_the_selection_moves_on() {
    let mut app = drawing(&["Alpha", "Beta", "Gamma", "Delta"]);
    open(&mut app);
    for (name, value) in [("Alpha", "3"), ("Beta", "4"), ("Gamma", "5")] {
        select(&mut app, name);
        edit(&mut app, "height", value);
    }
    // Gamma's edit is still in the draft; Alpha and Beta are staged.
    assert_eq!(names(&pending(&app)), ["Alpha", "Beta", "Gamma"]);
    select(&mut app, "Delta");
    let changes = pending(&app);
    assert_eq!(names(&changes), ["Alpha", "Beta", "Gamma"]);
    assert_eq!(
        changes
            .iter()
            .map(|c| c.properties[0].to.as_str())
            .collect::<Vec<_>>(),
        ["3", "4", "5"]
    );

    // The summary is what Apply writes.
    let _ = app.update(Message::TextStyleApply);
    for (name, height) in [
        ("Alpha", 3.0),
        ("Beta", 4.0),
        ("Gamma", 5.0),
        ("Delta", 2.5),
    ] {
        assert_eq!(style(&app, name).height, height, "{name}");
    }
    assert!(pending(&app).is_empty());
}

#[test]
fn twenty_changed_styles_keep_the_window_usable() {
    let names: Vec<String> = (1..=21).map(|n| format!("Style {n:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut app = drawing(&refs);
    open(&mut app);
    for name in &names {
        select(&mut app, name);
        edit(&mut app, "width", "0.8");
    }
    assert_eq!(pending(&app).len(), 21);

    for (width, height) in [(1280.0, 760.0), (1280.0, 520.0), (780.0, 600.0)] {
        let mut ui = window(&app, width, height);
        let head = shown(&mut ui, "Apply will change");
        shown(&mut ui, "21 styles, 21 properties");
        let note = shown(
            &mut ui,
            "Texts in external references do not change. A single Undo reverts every change.",
        );
        // The list scrolls inside the frame: the closing note sits within
        // the summary's cap below the title, whatever the count.
        assert!(
            note.y - head.y < 360.0,
            "{width}×{height}: {head:?} {note:?}"
        );
        if width >= 920.0 {
            // Beside it, the preview keeps its place.
            shown(&mut ui, "Sample text");
        }
    }
}

#[test]
fn reverting_a_value_drops_the_style_from_the_summary() {
    let mut app = drawing(&["Notes", "Spare"]);
    open(&mut app);
    select(&mut app, "Notes");
    edit(&mut app, "height", "3.5");
    select(&mut app, "Spare");
    edit(&mut app, "width", "2");
    assert_eq!(names(&pending(&app)), ["Notes", "Spare"]);

    // Back in the field: the stored value again.
    edit(&mut app, "width", "1.0000");
    assert_eq!(names(&pending(&app)), ["Notes"]);
    // A staged style goes back the same way.
    select(&mut app, "Notes");
    edit(&mut app, "height", "2.5");
    assert!(pending(&app).is_empty());
}

#[test]
fn texts_that_follow_and_texts_with_their_own_value() {
    let mut app = drawing(&["Notes"]);
    let follows = add_text(&mut app, "Notes", 2.5, 1.0);
    let own = add_text(&mut app, "Notes", 2.5, 0.7);
    add_mtext(&mut app, "Notes", "B");
    add_mtext(&mut app, "Notes", "\\W1.2;B");
    open(&mut app);
    select(&mut app, "Notes");
    edit(&mut app, "width", "0.8");

    assert_eq!(
        lines(&pending(&app)[0]),
        [
            "Width factor: 1 → 0.8. 2 texts that follow the style are updated. \
             2 texts with their own value keep them."
        ]
    );
    let _ = app.update(Message::TextStyleApply);
    assert_eq!(text(&app, follows).1, 0.8);
    assert_eq!(text(&app, own).1, 0.7);
}

#[test]
fn changes_to_several_styles_apply_and_undo_in_one_step() {
    let mut app = drawing(&["Notes", "Titles"]);
    let note = add_text(&mut app, "Notes", 2.5, 1.0);
    let title = add_text(&mut app, "Titles", 2.5, 1.0);
    open(&mut app);
    select(&mut app, "Notes");
    edit(&mut app, "height", "3.5");
    select(&mut app, "Titles");
    edit(&mut app, "width", "0.8");
    let _ = app.update(Message::TextStyleApply);
    app.close_active_modal();
    assert_eq!(text(&app, note), (3.5, 1.0));
    assert_eq!(text(&app, title), (2.5, 0.8));

    let _ = app.update(Message::Undo);
    assert_eq!(text(&app, note), (2.5, 1.0));
    assert_eq!(text(&app, title), (2.5, 1.0));
    assert_eq!(style(&app, "Notes").height, 2.5);
    assert_eq!(style(&app, "Titles").width_factor, 1.0);

    let _ = app.update(Message::Redo);
    assert_eq!(text(&app, note), (3.5, 1.0));
    assert_eq!(text(&app, title), (2.5, 0.8));
}

#[test]
fn a_long_name_does_not_hide_a_large_count() {
    const LONG: &str = "Title block annotation style for sheet borders";
    let mut app = drawing(&[LONG, "Short"]);
    for _ in 0..1234 {
        add_text(&mut app, LONG, 2.5, 1.0);
    }
    open(&mut app);
    select(&mut app, "Short");
    for width in [1280.0, 780.0] {
        let mut ui = window(&app, width, 760.0);
        let count = shown(&mut ui, "1234");
        let name = shown(&mut ui, LONG);
        let editor = shown(&mut ui, "Font style");
        // The name stops before the count; the count stays in the list.
        assert!(name.x + name.width <= count.x + 0.5, "{name:?} {count:?}");
        assert!(count.x + count.width < editor.x, "{count:?} {editor:?}");
    }
}

#[test]
fn apply_without_a_change_changes_nothing() {
    let mut app = drawing(&["Notes"]);
    let mut stored = style(&app, "Notes").clone();
    stored.width_factor = 0.123456789;
    stored.oblique_angle = 0.1;
    *app.tabs[app.active_tab]
        .scene
        .document
        .text_styles
        .get_mut("Notes")
        .unwrap() = stored.clone();
    let handle = add_text(&mut app, "Notes", 2.5, 0.123456789);
    open(&mut app);
    select(&mut app, "Notes");
    select(&mut app, "Standard");
    select(&mut app, "Notes");
    assert!(pending(&app).is_empty());
    let mut ui = window(&app, 1280.0, 760.0);
    assert!(ui.find("Apply will change".to_string()).is_err());
    drop(ui);

    let _ = app.update(Message::TextStyleApply);
    assert_eq!(style(&app, "Notes"), &stored);
    assert_eq!(text(&app, handle), (2.5, 0.123456789));
    assert!(!app.tabs[app.active_tab].dirty);
}
