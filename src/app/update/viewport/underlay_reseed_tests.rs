use super::*;

use crate::app::OpenCADStudio;
use document::types::Vector3;
use document::EntityType;

/// A new drawing with a PDF underlay; returns the app and the
/// underlay's handle.
fn app_with_underlay(name: &str) -> (OpenCADStudio, document::Handle) {
    use printpdf::{Mm, PdfDocument, PdfPage, PdfSaveOptions};
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let mut pdf = PdfDocument::new("reseed app");
    pdf.pages.push(PdfPage::new(Mm(25.4), Mm(25.4), Vec::new()));
    let bytes = pdf.save(&PdfSaveOptions::default(), &mut Vec::new());
    let path = format!("memory://reseed-app-{name}.pdf");
    crate::scene::model::pdf_raster::register_source(&path, std::sync::Arc::new(bytes));
    let scene = &mut app.tabs[app.active_tab].scene;
    let definition = scene.document.allocate_handle();
    scene.document.objects.insert(
        definition,
        document::objects::ObjectType::UnderlayDefinition(
            document::objects::UnderlayDefinition::pdf(&path, "1"),
        ),
    );
    let mut underlay = document::entities::Underlay::pdf();
    underlay.definition_handle = definition;
    underlay.insertion_point = Vector3::new(0.0, 0.0, 0.0);
    let handle = scene.add_entity(EntityType::Underlay(underlay));
    (app, handle)
}

/// Lets the view of drawing `i` rest on a new zoom step.
fn rest(app: &mut OpenCADStudio, i: usize) {
    let scene = &mut app.tabs[i].scene;
    scene.zoom_to_window(
        glam::Vec3::new(0.0, 0.0, 0.0),
        glam::Vec3::new(2.0, 2.0, 0.0),
    );
    scene.set_render_pixel_scale(1000.0, 1000.0);
    let start = iced::time::Instant::now();
    scene.refresh_underlay_resolution(start);
    scene.refresh_underlay_resolution(start + std::time::Duration::from_millis(200));
}

/// The worker's rasters reach the drawing when it ends, and not for a
/// drawing that was replaced meanwhile.
#[test]
fn the_worker_rasters_reach_their_drawing_only() {
    let (mut app, handle) = app_with_underlay("reach");
    let i = app.active_tab;
    let old = std::sync::Arc::clone(&app.tabs[i].scene.images[&handle].pixels);
    rest(&mut app, i);
    let task = app.start_underlay_reseed(i);
    assert!(std::sync::Arc::ptr_eq(
        &old,
        &app.tabs[i].scene.images[&handle].pixels
    ));
    app.drive_headless_task(task).expect("the worker's task");
    let new = std::sync::Arc::clone(&app.tabs[i].scene.images[&handle].pixels);
    assert!(
        !std::sync::Arc::ptr_eq(&old, &new),
        "the new raster is taken"
    );

    rest_again(&mut app, i);
    let task = app.start_underlay_reseed(i);
    app.tabs[i].document_generation += 1;
    app.drive_headless_task(task).expect("the worker's task");
    assert!(
        std::sync::Arc::ptr_eq(&new, &app.tabs[i].scene.images[&handle].pixels),
        "a replaced drawing keeps its raster"
    );
}

fn rest_again(app: &mut OpenCADStudio, i: usize) {
    let scene = &mut app.tabs[i].scene;
    scene.zoom_to_window(
        glam::Vec3::new(0.0, 0.0, 0.0),
        glam::Vec3::new(8.0, 8.0, 0.0),
    );
    let start = iced::time::Instant::now();
    scene.refresh_underlay_resolution(start);
    scene.refresh_underlay_resolution(start + std::time::Duration::from_millis(200));
}

/// The tick after a zoom step starts the worker, and its result
/// reaches the drawing.
#[test]
fn a_tick_after_a_zoom_step_starts_the_worker() {
    let (mut app, handle) = app_with_underlay("tick");
    let i = app.active_tab;
    let old = std::sync::Arc::clone(&app.tabs[i].scene.images[&handle].pixels);
    rest(&mut app, i);
    let task = app.on_tick(iced::time::Instant::now());
    assert!(task.units() > 0, "the tick starts the worker");
    app.drive_headless_task(task).expect("the worker's task");
    assert!(!std::sync::Arc::ptr_eq(
        &old,
        &app.tabs[i].scene.images[&handle].pixels
    ));
}
