//! Background mask and drawing direction, through the editor.
use super::*;
use crate::app::{Message, OpenCADStudio};
use codec::types::Color;
use codec::DxfVersion;

/// An existing MTEXT opened in the editor.
fn existing(mt: MText) -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    let height = mt.height;
    let value = mt.value.clone();
    let h = app.tabs[1]
        .scene
        .document
        .add_entity(EntityType::MText(Box::new(mt)))
        .unwrap();
    app.open_mtext_editor(DVec3::ZERO, Some(h), &value, height, None);
    (app, h)
}
fn stored(app: &OpenCADStudio, h: Handle) -> MText {
    match app.tabs[1].scene.document.get_entity(h) {
        Some(EntityType::MText(t)) => (**t).clone(),
        _ => panic!("MTEXT"),
    }
}
fn ed(app: &OpenCADStudio) -> &MTextEditorState {
    app.mtext_editor.as_ref().unwrap()
}
fn text(value: &str) -> MText {
    let mut mt = MText::with_value(value, Vector3::new(0., 0., 0.));
    mt.height = 2.5;
    mt
}

#[test]
fn mask_on_off_keeps_the_frame_bit_transparency_and_untouched_factor() {
    let mut mt = text("mask");
    mt.background_fill_flags = 0x10; // frame only
    mt.background_transparency = 37;
    mt.background_scale = 8.0; // outside 1–5, imported
    let (mut app, h) = existing(mt);
    let _ = app.update(Message::MTextMaskToggle);
    assert!(ed(&app).mask().on);
    app.mtext_apply();
    let saved = stored(&app, h);
    assert_eq!(
        saved.background_fill_flags,
        0x10 | 0x03,
        "drawing background"
    );
    assert_eq!(saved.background_transparency, 37);
    assert_eq!(saved.background_scale, 8.0, "an untouched factor is kept");
    // Off again: only the fill bits go.
    let _ = app.update(Message::MTextMaskToggle);
    app.mtext_apply();
    let saved = stored(&app, h);
    assert_eq!(saved.background_fill_flags, 0x10);
    assert_eq!(saved.background_transparency, 37);
}

#[test]
fn mask_colour_and_factor_are_written_and_a_new_factor_must_be_1_to_5() {
    let (mut app, h) = existing(text("mask"));
    // Off: the colour and factor are inactive.
    let _ = app.update(Message::MTextMaskColor(Some(Color::Index(5))));
    assert!(ed(&app).mask_draft.is_none());
    let _ = app.update(Message::MTextMaskToggle);
    let _ = app.update(Message::MTextMaskColor(Some(Color::Index(5))));
    let _ = app.update(Message::MTextMaskScale("7".into()));
    assert!(!app.mtext_commit(), "an invalid factor blocks saving");
    let _ = app.update(Message::MTextMaskScale("2,5".into()));
    assert!(app.mtext_commit());
    let saved = stored(&app, h);
    assert_eq!(saved.background_fill_flags, 0x01, "its own colour");
    assert_eq!(saved.background_color, Color::Index(5));
    assert_eq!(saved.background_scale, 2.5);
}

#[test]
fn drawing_background_keeps_the_stored_colour_and_undo_steps_back() {
    let mut mt = text("mask");
    mt.background_fill_flags = 0x01;
    mt.background_color = Color::Index(3);
    let (mut app, h) = existing(mt);
    assert!(ed(&app).mask().on && !ed(&app).mask().window);
    let _ = app.update(Message::MTextMaskColor(None));
    assert!(ed(&app).mask().window);
    app.mtext_undo();
    assert!(!ed(&app).mask().window);
    let _ = app.update(Message::MTextMaskColor(None));
    app.mtext_apply();
    let saved = stored(&app, h);
    assert_eq!(saved.background_fill_flags, 0x03);
    assert_eq!(saved.background_color, Color::Index(3));
}

#[test]
fn the_mask_is_drawn_once_switched_on() {
    let wires = |app: &OpenCADStudio, h: Handle| {
        let scene = &app.tabs[1].scene;
        let entity = scene.document.get_entity(h).unwrap().clone();
        crate::scene::convert::tessellate::tessellate(
            &scene.document,
            h,
            &entity,
            false,
            [1.0; 4],
            0.0,
            [0.0; 8],
            1.0,
            1.0,
            None,
            None,
            [0.0, 0.0, 0.0, 1.0],
            false,
        )
        .len()
    };
    // Plain text. Text with run-less strokes (a fraction rule, an
    // underline) takes the stroke route, which draws the mask with #2372.
    for value in ["mask"] {
        let (mut app, h) = existing(text(value));
        let off = wires(&app, h);
        let _ = app.update(Message::MTextMaskToggle);
        app.mtext_apply();
        assert!(wires(&app, h) > off, "{value}: the fill adds geometry");
        let _ = app.update(Message::MTextMaskToggle);
        app.mtext_apply();
        assert_eq!(wires(&app, h), off, "{value}");
        // The stored frame bit draws its rectangle on that route too.
        let mut framed = text(value);
        framed.background_fill_flags = 0x10;
        let (app, h) = existing(framed);
        assert!(wires(&app, h) > off, "{value}: frame");
    }
}

#[test]
fn a_mask_survives_dwg_2004_and_later_and_dxf() {
    let (mut app, h) = existing(text("mask"));
    let _ = app.update(Message::MTextMaskToggle);
    let _ = app.update(Message::MTextMaskColor(Some(Color::Index(4))));
    let _ = app.update(Message::MTextMaskScale("3".into()));
    assert!(app.mtext_commit());
    let want = stored(&app, h);
    for version in [
        DxfVersion::AC1018,
        DxfVersion::AC1021,
        DxfVersion::AC1024,
        DxfVersion::AC1027,
        DxfVersion::AC1032,
    ] {
        for ext in ["dwg", "dxf"] {
            let bytes =
                crate::io::save_to_bytes(&app.tabs[1].scene.document, ext, version).expect("save");
            let doc = crate::io::load_bytes(&format!("mask.{ext}"), bytes).expect("load");
            let Some(EntityType::MText(t)) = doc.get_entity(h) else {
                panic!()
            };
            assert_eq!(
                (
                    t.background_fill_flags,
                    t.background_scale,
                    &t.background_color
                ),
                (
                    want.background_fill_flags,
                    want.background_scale,
                    &want.background_color
                ),
                "{ext} {version:?}"
            );
        }
    }
}

#[test]
fn multileader_text_has_neither_option() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    let ml = codec::entities::MultiLeader::default();
    let h = app.tabs[1]
        .scene
        .document
        .add_entity(EntityType::MultiLeader(Box::new(ml)))
        .unwrap();
    app.open_mtext_editor(DVec3::ZERO, Some(h), "x", 2.5, None);
    assert!(!ed(&app).entity_options_supported());
    let _ = app.update(Message::MTextMaskToggle);
    let _ = app.update(Message::MTextDirection(
        codec::entities::DrawingDirection::TopToBottom,
    ));
    assert!(ed(&app).mask_draft.is_none() && ed(&app).direction.is_none());
}

#[test]
fn direction_normal_vertical_and_by_style_are_saved_and_undone() {
    use codec::entities::DrawingDirection as D;
    let (mut app, h) = existing(text("abc"));
    assert_eq!(ed(&app).direction_value(), D::LeftToRight);
    // (Vertical text's own caret boxes come with #2374.)
    let _ = app.update(Message::MTextDirection(D::TopToBottom));
    assert_eq!(ed(&app).build_mtext().drawing_direction, D::TopToBottom);
    app.mtext_undo();
    assert_eq!(ed(&app).direction_value(), D::LeftToRight);
    let _ = app.update(Message::MTextDirection(D::TopToBottom));
    app.mtext_apply();
    assert_eq!(stored(&app, h).drawing_direction, D::TopToBottom);
    let _ = app.update(Message::MTextDirection(D::ByStyle));
    app.mtext_apply();
    assert_eq!(stored(&app, h).drawing_direction, D::ByStyle);
    for version in [DxfVersion::AC1015, DxfVersion::AC1032] {
        for ext in ["dwg", "dxf"] {
            let bytes =
                crate::io::save_to_bytes(&app.tabs[1].scene.document, ext, version).unwrap();
            let doc = crate::io::load_bytes(&format!("dir.{ext}"), bytes).unwrap();
            let Some(EntityType::MText(t)) = doc.get_entity(h) else {
                panic!()
            };
            assert_eq!(t.drawing_direction, D::ByStyle, "{ext} {version:?}");
        }
    }
}
