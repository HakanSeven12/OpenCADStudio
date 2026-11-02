//! Text style data: a font choice stores what another program reads (a real
//! SHX file, or a TrueType file and family), drops settings the new font
//! cannot have, and saves and loads without losing or doubling data (#2413).

use super::text_style_manager_tests::pick;
use super::*;
use crate::entities::text_support::{TTF_FLAG_BOLD, TTF_FLAG_ITALIC};
use crate::ui::font_picker::FontChoice;
use codec::entities::{AttributeDefinition, EntityType};
use codec::tables::TextStyle;
use codec::types::Handle;
use codec::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

/// Character set and pitch bits a TrueType style carries besides bold/italic.
const CHARSET_PITCH: i32 = 34;

/// A clean drawing with the style "Notes" open in the text style manager.
fn app_editing(style: TextStyle) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let mut style = style;
    style.handle = doc.allocate_handle();
    doc.text_styles.add(style).unwrap();
    let _ = app.update(Message::TextStyleDialogOpen);
    let _ = app.update(Message::TextStyleDialogSelect("Notes".to_string()));
    app
}

fn true_type_style() -> TextStyle {
    let mut style = TextStyle::new("Notes");
    style.font_file = "arial.ttf".to_string();
    style.true_type_font = "Arial".to_string();
    style.true_type_font_flags = CHARSET_PITCH | TTF_FLAG_BOLD | TTF_FLAG_ITALIC;
    style
}

fn notes(doc: &codec::Document) -> TextStyle {
    doc.text_styles.get("Notes").unwrap().clone()
}

/// Save through the app's save preparation and load the bytes back.
fn round_trip(doc: &codec::Document, ext: &str) -> codec::Document {
    let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
    crate::io::load_bytes(&format!("drawing.{ext}"), bytes).unwrap()
}

fn record_names(doc: &codec::Document, handle: Handle) -> Vec<String> {
    doc.object_xdata
        .get(&handle)
        .map_or_else(Vec::new, |xdata| {
            xdata
                .records()
                .iter()
                .map(|record| record.application_name.clone())
                .collect()
        })
}

#[test]
fn a_stroke_font_stores_its_shx_file_and_drops_bold_and_italic() {
    let mut app = app_editing(true_type_style());
    // Loaded from DXF, the style still carries the family record it was read
    // with; it must not bring the family back.
    let i = app.active_tab;
    app.tabs[i].scene.document = round_trip(&app.tabs[i].scene.document, "dxf");
    let _ = app.update(Message::TextStyleDialogSelect("Notes".to_string()));
    let _ = app.update(pick(FontChoice::Stroke("simplex.shx".to_string())));
    let _ = app.update(Message::TextStyleApply);

    let doc = &app.tabs[i].scene.document;
    let style = notes(doc);
    assert_eq!(style.font_file, "simplex.shx");
    assert!(style.true_type_font.is_empty());
    assert_eq!(style.true_type_font_flags, CHARSET_PITCH);
    for ext in ["dxf", "dwg"] {
        let back = notes(&round_trip(doc, ext));
        assert_eq!(back.font_file, "simplex.shx", "{ext}");
        assert!(
            back.true_type_font.is_empty(),
            "{ext}: the old family came back"
        );
    }
}

#[test]
fn a_system_font_stores_its_file_with_the_family_and_drops_stroke_settings() {
    use crate::scene::text::sysfont;
    // The bundled web font, on every machine; and a family with an
    // installed file, where the machine has one.
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/web/fonts/latin.ttf"))
        .expect("bundled web font");
    let (bundled, _) = sysfont::register_font_file("TextStyle-Test.ttf", &bytes).expect("a face");
    let installed = sysfont::families()
        .into_iter()
        .find(|family| sysfont::face_file(family, false, false).is_some());
    for family in std::iter::once(bundled).chain(installed) {
        let file = sysfont::face_file(&family, false, false).unwrap_or_default();
        let mut style = TextStyle::new("Notes");
        style.font_file = "txt.shx".to_string();
        style.big_font_file = "gbcbig.shx".to_string();
        style.is_vertical = true;
        let mut app = app_editing(style);
        let _ = app.update(pick(FontChoice::TrueType(family.clone())));
        let _ = app.update(Message::TextStyleApply);

        let doc = &app.tabs[app.active_tab].scene.document;
        let style = notes(doc);
        assert_eq!(
            (style.font_file.as_str(), style.true_type_font.as_str()),
            (file.as_str(), family.as_str())
        );
        assert!(style.big_font_file.is_empty());
        assert!(!style.is_vertical);
        for ext in ["dxf", "dwg"] {
            let back = notes(&round_trip(doc, ext));
            assert_eq!(
                (back.font_file, back.true_type_font),
                (file.clone(), family.clone()),
                "{family} {ext}"
            );
        }
    }
}

#[test]
fn a_dxf_round_trip_keeps_one_current_family_record() {
    let mut app = app_editing(true_type_style());
    let i = app.active_tab;
    let first = round_trip(&app.tabs[i].scene.document, "dxf");
    app.tabs[i].scene.document = first;
    let _ = app.update(Message::TextStyleDialogSelect("Notes".to_string()));
    let _ = app.update(pick(FontChoice::TrueType("Verdana".to_string())));
    let _ = app.update(Message::TextStyleApply);

    let back = round_trip(&app.tabs[i].scene.document, "dxf");
    let style = notes(&back);
    assert_eq!(style.true_type_font, "Verdana");
    assert_eq!(record_names(&back, style.handle), ["ACAD"]);
}

#[test]
fn unknown_xdata_and_character_set_bits_survive_save_and_load() {
    let mut app = app_editing(true_type_style());
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let handle = notes(doc).handle;
    let mut record = ExtendedDataRecord::new("OCS_KEEP");
    record.add_value(XDataValue::String("kept".to_string()));
    let mut xdata = ExtendedData::new();
    xdata.add_record(record);
    doc.object_xdata.insert(handle, xdata);
    let _ = doc.app_ids.add(codec::tables::AppId::new("OCS_KEEP"));

    for ext in ["dxf", "dwg"] {
        let back = round_trip(&app.tabs[i].scene.document, ext);
        let style = notes(&back);
        assert_eq!(
            style.true_type_font_flags,
            CHARSET_PITCH | TTF_FLAG_BOLD | TTF_FLAG_ITALIC,
            "{ext}"
        );
        let kept = back
            .object_xdata
            .get(&style.handle)
            .and_then(|xdata| xdata.get_record("OCS_KEEP"))
            .map(|record| record.values.clone());
        assert_eq!(
            kept,
            Some(vec![XDataValue::String("kept".to_string())]),
            "{ext}"
        );
    }
}

#[test]
fn a_new_attribute_follows_a_backward_style() {
    let mut style = TextStyle::new("Notes");
    style.flags.backward = true;
    style.flags.upside_down = true;
    let app = app_editing(style);
    let doc = &app.tabs[app.active_tab].scene.document;
    let mut attribute = AttributeDefinition::new("TAG".to_string(), String::new(), String::new());
    attribute.text_style = "Notes".to_string();
    let mut entity = EntityType::AttributeDefinition(attribute);
    crate::scene::creation_style::apply_current_creation_styles(doc, &mut entity);
    let EntityType::AttributeDefinition(attribute) = entity else {
        unreachable!()
    };
    // Drawn as the style XOR its own flags: a copy of the style's would cancel it.
    assert_eq!(attribute.text_generation_flags, 0);
}
