//! The Xref Manager decodes a selected reference's preview on a worker.
use crate::app::{Message, OpenCADStudio};
use crate::ui::window::xref_manager::{SelectExtend, XrefRowId};

/// An app whose drawing references an 8 x 8 PNG, the Xref Manager filled;
/// the image row.
fn app_with_an_image(stem: &str) -> (OpenCADStudio, XrefRowId) {
    use document::objects::{ImageDefinition, ObjectType};
    let dir = std::env::temp_dir().join(format!("ocs-xref-preview-{stem}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("img.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]))
        .save(&path)
        .unwrap();
    let path = path.to_string_lossy().into_owned();
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let doc = &mut app.tabs[app.active_tab].scene.document;
    let h = doc.allocate_handle();
    let mut def = ImageDefinition::with_dimensions(path.clone(), 8, 8);
    def.handle = h;
    doc.objects.insert(h, ObjectType::ImageDefinition(def));
    let mut image =
        document::entities::RasterImage::new(&path, document::types::Vector3::ZERO, 8.0, 8.0);
    image.definition_handle = Some(h);
    doc.add_entity(document::EntityType::RasterImage(image))
        .unwrap();
    app.refresh_xref_manager();
    let row = app
        .xref_manager
        .entries
        .iter()
        .find(|e| e.found_at.is_some())
        .map(|e| XrefRowId {
            key: e.key,
            saved_path: e.saved_path.clone(),
        })
        .expect("the image row");
    (app, row)
}

/// A row click asks for the preview, and a worker decodes it: nothing is
/// decoded during the click, and the preview comes when the worker ends.
#[test]
fn a_row_click_decodes_its_preview_on_a_worker() {
    let (mut app, row) = app_with_an_image("click");
    let task = app.update(Message::XrefManagerSelect(row));
    assert!(
        app.xref_manager.previews.is_empty(),
        "no decode during the click"
    );
    let message = OpenCADStudio::first_task_message(task);
    assert!(
        matches!(message, Message::XrefPreviewRead(_, _, Some(_))),
        "{message:?}"
    );
    let _ = app.update(message);
    assert_eq!(app.xref_manager.previews.len(), 1, "the preview is shown");
}

/// A decoder panic is caught on the worker: no preview, and the app goes
/// on.
#[test]
fn a_preview_decoder_panic_keeps_the_app() {
    let (mut app, row) = app_with_an_image("panic");
    app.xref_manager.click_select(&row, SelectExtend::Single);
    let request = app
        .xref_manager
        .take_preview_request()
        .expect("the click asks for the preview");
    let task = app.read_xref_manager_preview_with(request, |_| panic!("a decoder bug"));
    let message = OpenCADStudio::first_task_message(task);
    assert!(
        matches!(message, Message::BackgroundTaskFailed { .. }),
        "{message:?}"
    );
    let _ = app.update(message);
    assert!(app.xref_manager.previews.is_empty(), "no preview");
}
