use super::*;

use crate::app::OpenCADStudio;
use crate::io::MTextTargetLoss::{BoxHeight, Columns};
use document::{DxfVersion, EntityType};

/// A tab holding one MTEXT with columns and a defined box height.
fn app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    let mut text =
        document::entities::MText::with_value("rich", document::types::Vector3::new(0., 0., 0.));
    text.column_data.column_type = 1;
    text.column_data.column_count = 2;
    text.rectangle_height = Some(30.);
    app.tabs[1]
        .scene
        .document
        .add_entity(EntityType::MText(Box::new(text)))
        .unwrap();
    app
}

fn save(app: &mut OpenCADStudio, name: &str, version: DxfVersion) {
    let dir = std::env::temp_dir().join(format!("ocs_mtext_loss_{}", std::process::id()));
    let _ = app.queue_native_save(
        1,
        dir.join(name),
        version,
        crate::app::SavePurpose::SaveAs,
        crate::app::SaveContinuation::None,
        true,
        true,
    );
}

#[test]
fn an_older_dwg_target_asks_before_dropping_mtext_data() {
    let mut app = app();
    save(&mut app, "old.dwg", DxfVersion::AC1018);
    assert_eq!(
        app.active_modal,
        Some(crate::app::ModalKind::ConversionLoss)
    );
    assert_eq!(
        app.conversion_losses,
        vec![Columns.message(1), BoxHeight.message(1)]
    );
    assert_eq!(
        app.pending_conversion_save
            .as_ref()
            .map(crate::app::PendingConversionSave::format_name)
            .as_deref(),
        Some("DWG 2004")
    );
}

#[test]
fn an_older_dxf_target_asks_before_dropping_mtext_columns() {
    let mut app = app();
    save(&mut app, "old.dxf", DxfVersion::AC1018);
    assert_eq!(app.conversion_losses, vec![Columns.message(1)]);
    assert_eq!(
        app.pending_conversion_save
            .as_ref()
            .map(crate::app::PendingConversionSave::format_name)
            .as_deref(),
        Some("DXF 2004")
    );
}

#[test]
fn drawing_properties_and_mtext_losses_share_one_question() {
    let mut app = app();
    app.tabs[1].scene.document.summary_info.title = "Title".into();
    save(&mut app, "r2000.dwg", DxfVersion::AC1015);
    assert_eq!(
        app.conversion_losses,
        vec![
            crate::tr!("dwgprops-dialog", "save-loss", format = "DWG", count = "1"),
            Columns.message(1),
            BoxHeight.message(1),
        ]
    );
}

#[test]
fn dwg_and_dxf_2018_keep_it_all_and_never_ask() {
    for (name, version) in [
        ("new.dwg", DxfVersion::AC1032),
        ("new.dxf", DxfVersion::AC1032),
    ] {
        let mut app = app();
        save(&mut app, name, version);
        assert_ne!(
            app.active_modal,
            Some(crate::app::ModalKind::ConversionLoss),
            "{name}"
        );
        assert!(app.pending_conversion_save.is_none(), "{name}");
    }
}
