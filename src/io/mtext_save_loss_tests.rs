//! The MTEXT save-loss table against the DWG/DXF writer itself: a warning is
//! given for exactly what a target really drops, and for nothing else.
use super::{load_bytes, mtext_losses_on_save, save_to_bytes, MTextTargetLoss};
use codec::entities::{LineSpacingStyle, MText};
use codec::types::{Color, Vector3};
use codec::{Document, DxfVersion, EntityType, Handle};

const TARGETS: [DxfVersion; 7] = [
    DxfVersion::AC1014,
    DxfVersion::AC1015,
    DxfVersion::AC1018,
    DxfVersion::AC1021,
    DxfVersion::AC1024,
    DxfVersion::AC1027,
    DxfVersion::AC1032,
];

/// One MTEXT carrying every feature the table knows about.
fn rich() -> (Document, Handle) {
    let mut text = MText::with_value("rich", Vector3::new(1., 2., 0.));
    text.height = 2.5;
    text.rectangle_width = 40.;
    text.column_data.column_type = 1;
    text.column_data.column_count = 2;
    text.column_data.width = 18.;
    text.column_data.gutter = 2.;
    text.rectangle_height = Some(30.);
    text.background_fill_flags = 1;
    text.background_scale = 2.;
    text.background_color = Color::Index(3);
    text.line_spacing_factor = 1.5;
    text.line_spacing_style = LineSpacingStyle::Exactly;
    let mut doc = Document::new();
    let handle = doc.add_entity(EntityType::MText(Box::new(text))).unwrap();
    (doc, handle)
}

fn mtext(doc: &Document, handle: Handle) -> &MText {
    match doc.get_entity(handle) {
        Some(EntityType::MText(t)) => t,
        _ => panic!("the MTEXT survives"),
    }
}

/// Whether the saved copy still holds this feature as written.
fn kept(loss: MTextTargetLoss, before: &MText, after: &MText) -> bool {
    match loss {
        MTextTargetLoss::Columns => before.column_data == after.column_data,
        MTextTargetLoss::BoxHeight => before.rectangle_height == after.rectangle_height,
        MTextTargetLoss::Background => {
            before.background_fill_flags == after.background_fill_flags
                && before.background_scale == after.background_scale
                && before.background_color == after.background_color
                && before.background_transparency == after.background_transparency
        }
        MTextTargetLoss::LineSpacing => {
            before.line_spacing_factor == after.line_spacing_factor
                && before.line_spacing_style == after.line_spacing_style
        }
    }
}

#[test]
fn the_warning_names_exactly_what_each_target_drops() {
    let (doc, handle) = rich();
    for version in TARGETS {
        for ext in ["dwg", "dxf"] {
            let bytes = save_to_bytes(&doc, ext, version).expect("save");
            let saved = load_bytes(&format!("loss.{ext}"), bytes).expect("load");
            let (before, after) = (mtext(&doc, handle), mtext(&saved, handle));
            let dropped: Vec<_> = MTextTargetLoss::ALL
                .into_iter()
                .filter(|loss| !kept(*loss, before, after))
                .map(|loss| (loss, 1))
                .collect();
            assert_eq!(
                mtext_losses_on_save(&doc, version, ext == "dxf"),
                dropped,
                "{ext} {version:?}"
            );
        }
    }
}

#[test]
fn plain_mtext_never_warns_on_any_target() {
    let mut doc = Document::new();
    let mut text = MText::with_value("plain", Vector3::new(1., 2., 0.));
    text.rotation = 0.5;
    text.rectangle_width = 40.;
    doc.add_entity(EntityType::MText(Box::new(text))).unwrap();
    for version in TARGETS {
        for is_dxf in [false, true] {
            assert!(mtext_losses_on_save(&doc, version, is_dxf).is_empty());
        }
    }
}
