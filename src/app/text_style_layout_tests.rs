//! The text style manager's layout: list rows with their counts and lock,
//! "Styles in use", the two columns that stack in a narrow window, the
//! preview of the draft, and where the style is used (#2413).

use super::text_style_users::{TextKind, Usage};
use super::*;
use codec::entities::{AttributeDefinition, EntityType, MText, Text};
use codec::tables::{BlockRecord, TextStyle};
use codec::types::{Handle, Vector3};
use iced_test::Simulator;

/// A drawing with the styles "Notes" (height 2.5, used by two TEXTs, an
/// MTEXT and an attribute definition in a block), "Spare" (unused) and the
/// referenced "PLAN|Notes"; the manager open on "Notes".
fn manager() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let doc = &mut app.tabs[app.active_tab].scene.document;
    for name in ["Notes", "Spare", "PLAN|Notes"] {
        let mut style = TextStyle::new(name);
        style.height = 2.5;
        style.handle = doc.allocate_handle();
        doc.text_styles.add(style).unwrap();
    }
    let mut block = BlockRecord::new("Title");
    block.handle = doc.allocate_handle();
    let owner = block.handle;
    doc.block_records.add(block).unwrap();
    for _ in 0..2 {
        let mut text = Text::with_value("A", Vector3::ZERO);
        text.style = "Notes".into();
        text.height = 2.5;
        doc.add_entity(EntityType::Text(text)).unwrap();
    }
    let mut mtext = MText::with_value("B", Vector3::ZERO);
    mtext.style = "notes".into();
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    let mut attdef = AttributeDefinition::default();
    attdef.text_style = "Notes".into();
    attdef.common.owner_handle = owner;
    let handle = doc
        .add_entity(EntityType::AttributeDefinition(attdef))
        .unwrap();
    add_to_block(&mut app, owner, handle);
    let _ = app.update(Message::TextStyleDialogOpen);
    let _ = app.update(Message::TextStyleDialogSelect("Notes".into()));
    app
}

fn add_to_block(app: &mut OpenCADStudio, block: Handle, entity: Handle) {
    let doc = &mut app.tabs[app.active_tab].scene.document;
    if let Some(record) = doc.block_records.iter_mut().find(|r| r.handle == block) {
        record.entity_handles.push(entity);
    }
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

#[test]
fn the_counts_come_from_one_walk_and_cover_every_kind() {
    let app = manager();
    assert_eq!(app.textstyle_counts.get("notes"), Some(&4));
    assert_eq!(app.textstyle_counts.get("spare"), None);
    assert_eq!(
        Usage::of(&app.textstyle_users),
        Usage {
            kinds: vec![
                (TextKind::Text, 2),
                (TextKind::MText, 1),
                (TextKind::AttributeDefinition, 1),
            ],
            in_blocks: 1,
            blocks: 1,
        }
    );
}

#[test]
fn the_list_shows_counts_and_styles_in_use_hides_the_unused() {
    let mut app = manager();
    let mut ui = window(&app, 1280.0);
    shown(&mut ui, "Spare");
    shown(&mut ui, "PLAN|Notes");
    shown(&mut ui, "4");
    drop(ui);
    let _ = app.update(Message::TextStyleInUseFilter(true));
    let mut ui = window(&app, 1280.0);
    assert!(ui.find("Spare".to_string()).is_err());
    shown(&mut ui, "Standard");
}

#[test]
fn the_where_used_box_and_the_status_count_the_texts() {
    let app = manager();
    let mut ui = window(&app, 1280.0);
    for text in [
        "Single-line text",
        "MTEXT",
        "Attribute definitions",
        "1 of them inside 1 block definition.",
        "Used in drawing · used by 4 texts",
    ] {
        shown(&mut ui, text);
    }
}

#[test]
fn a_changed_value_says_what_was_stored_and_apply_says_what_it_does() {
    let mut app = manager();
    let _ = app.update(Message::TextStyleEdit {
        field: "height",
        value: "3.5".into(),
    });
    let mut ui = window(&app, 1280.0);
    // As the field showed it.
    shown(&mut ui, "Modified · was 2.5000");
    shown(&mut ui, "On Apply");
}

#[test]
fn the_columns_sit_side_by_side_and_stack_in_a_narrow_window() {
    let mut app = manager();
    let _ = app.update(Message::TextStyleEdit {
        field: "height",
        value: "3.5".into(),
    });
    let mut wide = window(&app, 1280.0);
    let font = shown(&mut wide, "Font style");
    let preview = shown(&mut wide, "Sample text");
    assert!(preview.x > font.x + font.width, "{font:?} {preview:?}");
    shown(&mut wide, "Set Current");
    drop(wide);

    // The modal keeps its size; a narrow window makes it narrow. Where the
    // columns stack, the toolbar shows icons only, the names as hints.
    let mut narrow = window(&app, 780.0);
    let summary = shown(&mut narrow, "On Apply");
    let font = shown(&mut narrow, "Font style");
    assert!(summary.y < font.y, "{summary:?} {font:?}");
    assert!(narrow.find("Set Current".to_string()).is_err());
}

#[test]
fn the_preview_draws_the_draft_with_its_sample_text() {
    let mut app = manager();
    let _ = app.update(Message::TextStyleSample("Hello".into()));
    let _ = app.update(Message::TextStyleEdit {
        field: "width",
        value: "2".into(),
    });
    let mut ui = window(&app, 1280.0);
    shown(&mut ui, "Hello");
    shown(&mut ui, "Where used");
}

#[test]
fn a_shape_file_entry_is_not_listed_or_offered_for_comparison() {
    // A new drawing holds the nameless shape file of the complex linetypes.
    let app = manager();
    let doc = &app.tabs[app.active_tab].scene.document;
    assert!(doc
        .text_styles
        .iter()
        .any(|s| s.is_shape_file && s.name.is_empty()));

    let (listed, rows) = app.textstyle_list();
    assert!(listed.iter().all(|name| !name.is_empty()), "{listed:?}");
    assert_eq!(listed.len(), rows.len());
    assert!(!app.style_names(StyleKind::Text).contains(&String::new()));
}

#[test]
fn delete_is_off_for_standard_and_a_style_texts_use() {
    let mut app = manager();
    for (style, on) in [("Standard", false), ("Notes", false), ("Spare", true)] {
        let _ = app.update(Message::TextStyleDialogSelect(style.into()));
        let mut ui = window(&app, 1280.0);
        let _ = ui.click("Delete");
        let deleted = ui
            .into_messages()
            .any(|message| matches!(message, Message::TextStyleDialogDelete));
        assert_eq!(deleted, on, "{style}");
    }
}

#[test]
fn the_preview_pairs_the_big_font_found_next_to_the_drawing() {
    let mut app = manager();
    let folder = std::env::temp_dir().join(format!("text-style-preview-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let big = folder.join("preview-big.shx");
    std::fs::write(&big, b"").unwrap();
    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.source_path = Some(folder.join("plan.dwg").to_string_lossy().into_owned());
    let _ = app.update(Message::TextStyleToggle("use_big_font"));
    let _ = app.update(Message::TextStyleEdit {
        field: "bigfont",
        value: "preview-big.shx".into(),
    });

    let preview = app.textstyle_preview(&app.textstyle_draft.resolved());
    let _ = std::fs::remove_dir_all(&folder);
    let (_, paired) = preview
        .font
        .split_once(crate::scene::text::font_face::BIG_FONT_SEP)
        .unwrap_or_else(|| panic!("no big font in {:?}", preview.font));
    assert!(paired.ends_with("preview-big.shx"), "{paired}");
    assert!(preview.text.contains('漢'), "{}", preview.text);
    assert!(
        preview.caption.contains("+ preview-big.shx"),
        "{}",
        preview.caption
    );
}

#[test]
fn the_preview_draws_the_bold_and_italic_face_the_drawing_draws() {
    use crate::scene::text::sysfont;
    let mut app = manager();
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/web/fonts/latin.ttf"))
        .expect("bundled web font");
    let (bundled, _) = sysfont::register_font_file("Preview-Test.ttf", &bytes).expect("a face");
    // A family with a real italic face, where the machine has one.
    let italic = sysfont::families()
        .into_iter()
        .find(|family| sysfont::styled_family(family, false, true).is_some());
    for family in std::iter::once(bundled).chain(italic.clone()) {
        let _ = app.update(crate::app::text_style_manager_tests::pick(
            crate::ui::font_picker::FontChoice::TrueType(family.clone()),
        ));
        let _ = app.update(Message::TextStyleFaceStyle(
            crate::ui::font_picker::FaceStyle::of(false, true),
        ));
        let edited = app.textstyle_draft.resolved();
        let preview = app.textstyle_preview(&edited);
        let doc = &app.tabs[app.active_tab].scene.document;
        let drawn = crate::entities::text_support::resolve_text_style_record(&edited, doc);
        assert_eq!(preview.font, drawn.font_name, "{family}");
        if italic.as_ref() == Some(&family) {
            assert!(sysfont::split_style(&preview.font).2, "{:?}", preview.font);
        }
    }
}

#[test]
fn a_long_preview_caption_wraps_inside_the_box() {
    let mut app = manager();
    let _ = app.update(Message::TextStyleEdit {
        field: "height",
        value: "125".into(),
    });
    let _ = app.update(crate::app::text_style_manager_tests::pick(
        crate::ui::font_picker::FontChoice::Stroke(
            "a-stroke-font-with-a-rather-long-file-name.shx".into(),
        ),
    ));
    let caption = app
        .textstyle_preview(&app.textstyle_draft.resolved())
        .caption;
    let mut ui = window(&app, 1280.0);
    let label = shown(&mut ui, "Preview");
    let text = shown(&mut ui, &caption);
    // The right column is 320 wide with 14 on each side.
    assert!(text.x + text.width <= label.x + 292.0, "{text:?} {label:?}");
    assert!(text.height > 14.0, "one line: {text:?}");
}

/// Test 10: the window draws, wide and narrow. With
/// `OCS_TEXT_STYLE_CAPTURES` set to a folder it also saves both in the light
/// and dark theme for a look; rendering them on every run slows the parallel
/// suite enough to break its timing-sensitive tests.
#[test]
fn the_window_draws_wide_and_narrow_in_both_themes() {
    let app = manager();
    let folder = std::env::var_os("OCS_TEXT_STYLE_CAPTURES").map(std::path::PathBuf::from);
    for width in [1280.0, 780.0] {
        let mut ui = window(&app, width);
        shown(&mut ui, "Preview");
        let Some(folder) = &folder else { continue };
        for theme in [iced::Theme::Light, iced::Theme::Dark] {
            let snapshot = ui.snapshot(&theme).expect("a snapshot");
            let path = folder.join(format!("{width}-{theme}"));
            assert!(snapshot.matches_image(&path).expect("written"));
        }
    }
}
