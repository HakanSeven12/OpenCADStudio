//! The text style windows as the manual test asked: the font list right
//! under its field and coloured by kind, the MTEXT origin marks under their
//! labels, the style's font in the MTEXT Font field, the real stand-in of a
//! missing font, and the note on what a TrueType font turns off (#2413).

use super::*;
use crate::ui::font_picker::{drawn_with, FontChoice, FontPickerEvent};
use codec::tables::TextStyle;
use iced_test::Simulator;

fn style(name: &str, font_file: &str, family: &str) -> TextStyle {
    let mut style = TextStyle::new(name);
    style.font_file = font_file.to_string();
    style.true_type_font = family.to_string();
    style
}

/// A clean drawing with these text styles added.
fn app_with(styles: Vec<TextStyle>) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let doc = &mut app.tabs[app.active_tab].scene.document;
    for mut style in styles {
        style.handle = doc.allocate_handle();
        doc.text_styles.add(style).unwrap();
    }
    app
}

/// The manager open on "Notes" with its font list open; the drawing also
/// names a TrueType font and an SHX file nothing here has. The TrueType font
/// is the bundled web font, so every machine has it, not only those with
/// office fonts installed.
fn font_list_open() -> OpenCADStudio {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/web/fonts/latin.ttf"))
        .expect("bundled web font");
    let (family, _) =
        crate::scene::text::sysfont::register_font_file("FontList-Test.ttf", &bytes)
            .expect("a face");
    let mut app = app_with(vec![
        style("Notes", "txt.shx", ""),
        style("Office", "FontList-Test.ttf", &family),
        style("Lost", "lostfont.shx", ""),
    ]);
    let _ = app.update(Message::TextStyleDialogOpen);
    let _ = app.update(Message::TextStyleDialogSelect("Notes".into()));
    let _ = app.update(Message::TextStyleFont(FontPickerEvent::Toggle));
    app
}

fn window(app: &OpenCADStudio, width: f32) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(width, 760.0),
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

/// Snapshots of `app` in the light and the dark theme, for a look: only with
/// `OCS_TEXT_STYLE_CAPTURES` set to a folder, as rendering them on every run
/// slows the parallel suite enough to break its timing-sensitive tests.
fn snapshots(app: &OpenCADStudio, folder: &str, width: f32) {
    let Some(root) = std::env::var_os("OCS_TEXT_STYLE_CAPTURES") else {
        return;
    };
    let folder = std::path::PathBuf::from(root).join(folder);
    let _ = std::fs::remove_dir_all(&folder);
    for theme in [iced::Theme::Light, iced::Theme::Dark] {
        let mut ui = window(app, width);
        let snapshot = ui.snapshot(&theme).expect("a snapshot");
        assert!(snapshot
            .matches_image(folder.join(format!("{width}-{theme}")))
            .expect("written"));
    }
}

/// B1: the list opens below the Font field, its left edge on the field's,
/// wide and narrow, never flipped over the style list.
#[test]
fn the_font_list_opens_right_under_the_font_field() {
    let app = font_list_open();
    for width in [1280.0, 780.0] {
        let mut ui = window(&app, width);
        // The Font style field sits under the Font field, left edges lined
        // up; the open list covers it, framed and padded by 2 + 6.
        let field = shown(&mut ui, "Regular");
        let title = shown(&mut ui, "IN THIS DRAWING");
        assert!((title.x - field.x - 8.0).abs() < 1.0, "{width}: {field:?} {title:?}");
        assert!((title.y - field.y).abs() < 20.0, "{width}: {field:?} {title:?}");
    }
}

/// B2: SHX, TT and "!" tags, the stand-in in orange, in both themes.
#[test]
fn the_font_list_is_coloured_in_both_themes() {
    let app = font_list_open();
    let mut ui = window(&app, 1280.0);
    for text in ["SHX", "TT", "!", "missing · drawn with Standard"] {
        shown(&mut ui, text);
    }
    drop(ui);
    snapshots(&app, "text-style-font-list", 1280.0);
}

/// B5: the stand-in is the font that really draws: the bundled stroke
/// font a name stands for, else Standard.
#[test]
fn a_missing_font_names_the_font_drawn_in_its_place() {
    let unknown = drawn_with(&FontChoice::Stroke("lostfont.shx".into()));
    let alias = drawn_with(&FontChoice::Stroke("complex.shx".into()));
    assert_eq!(unknown, "Standard");
    assert_eq!(alias, "Roman Complex");
}

/// "Notes" draws with a fixed height of 2.5 and the text style "Roman" with
/// romans.shx; the MTEXT editor open on `raw`.
fn mtext_editor(raw: &str) -> OpenCADStudio {
    let mut notes = style("Notes", "lostfont.shx", "");
    notes.height = 2.5;
    let mut app = app_with(vec![notes, style("Roman", "romans.shx", "")]);
    app.tabs[app.active_tab]
        .scene
        .document
        .header
        .current_text_style_name = "Notes".into();
    app.open_mtext_editor(glam::DVec3::ZERO, None, raw, 2.5, None);
    app
}

/// B3: each mark on its own line under its label; B5: the style's missing
/// font is marked as soon as the editor opens.
#[test]
fn the_origin_marks_sit_under_their_labels_in_both_themes() {
    let app = mtext_editor("\\W1.2;a{\\Q20;b}");
    let mut ui = window(&app, 1280.0);
    for (label, mark) in [("Width factor", "Custom"), ("Oblique", "Mixed")] {
        let label = shown(&mut ui, label);
        let mark = shown(&mut ui, mark);
        assert!(mark.y >= label.y + label.height - 1.0, "{label:?} {mark:?}");
        assert!((mark.x - label.x).abs() < 1.0, "{label:?} {mark:?}");
    }
    shown(&mut ui, "By style");
    shown(&mut ui, "!");
    drop(ui);
    snapshots(&app, "mtext-origin-marks", 1280.0);
}

/// B4: without a font of its own the Font field shows the style's font, the
/// new style's after a switch.
#[test]
fn the_font_field_shows_the_style_font_after_a_style_switch() {
    let mut app = mtext_editor("Text");
    shown(&mut window(&app, 1280.0), "lostfont.shx");
    let _ = app.update(Message::MTextStyle("Roman".into()));
    let mut ui = window(&app, 1280.0);
    shown(&mut ui, "RomanS");
    assert!(ui.find("[Style default]").is_err());
}

/// B6: switching to a TrueType font says what it turned off.
#[test]
fn a_truetype_font_switch_says_what_it_turned_off() {
    let mut vertical = style("Notes", "txt.shx", "");
    vertical.is_vertical = true;
    let mut app = app_with(vec![vertical]);
    let _ = app.update(Message::TextStyleDialogOpen);
    let _ = app.update(Message::TextStyleDialogSelect("Notes".into()));
    let _ = app.update(Message::TextStyleFont(FontPickerEvent::Pick(
        FontChoice::TrueType("Arial".into()),
    )));
    shown(
        &mut window(&app, 1280.0),
        "Turned off for the TrueType font: Vertical.",
    );
}
