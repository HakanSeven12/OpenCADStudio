//! A PDF underlay on the sheet of a layout follows the zoom of that sheet,
//! as an OLE picture there does: its page is drawn for the size it shows on
//! the sheet and, zoomed in, again for the part in view. The scale the model
//! view was left at has no say in it.

use super::*;
use document::types::Vector3;
use std::time::Duration;

/// A PDF underlay of a one-inch page, drawn 100 by 100 on the sheet of
/// layout "Sheet", which is the current layout.
fn sheet_with_underlay(name: &str) -> (Scene, Handle) {
    use printpdf::{Mm, PdfDocument, PdfPage, PdfSaveOptions};
    let mut pdf = PdfDocument::new("sheet underlay");
    pdf.pages.push(PdfPage::new(Mm(25.4), Mm(25.4), Vec::new()));
    let bytes = pdf.save(&PdfSaveOptions::default(), &mut Vec::new());
    let path = format!("memory://sheet-underlay-{name}.pdf");
    crate::scene::model::pdf_raster::register_source(&path, std::sync::Arc::new(bytes));
    let mut scene = Scene::new();
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());
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
    underlay.x_scale = 100.0;
    underlay.y_scale = 100.0;
    underlay.z_scale = 100.0;
    let handle = scene.add_entity(EntityType::Underlay(underlay));
    let owner = scene
        .document
        .get_entity(handle)
        .expect("the underlay")
        .common()
        .owner_handle;
    assert_eq!(
        owner,
        scene.current_layout_block_handle(),
        "the page is on the sheet"
    );
    (scene, handle)
}

/// Looks at the rectangle `[x0, y0]`–`[x1, y1]` of the current layout in a
/// 1000-pixel square view and lets the view rest; the job, if the rest made
/// one, is left for the caller.
fn rest_at(scene: &mut Scene, x0: f32, y0: f32, x1: f32, y1: f32) {
    scene.zoom_to_window(glam::Vec3::new(x0, y0, 0.0), glam::Vec3::new(x1, y1, 0.0));
    scene.set_render_pixel_scale(1000.0, 1000.0);
    let start = iced::time::Instant::now();
    scene.refresh_underlay_resolution(start);
    scene.refresh_underlay_resolution(start + Duration::from_millis(200));
}

/// The same, and the rasters follow: the worker's job is drawn and taken.
fn look_at(scene: &mut Scene, x0: f32, y0: f32, x1: f32, y1: f32) {
    scene.zoom_to_window(glam::Vec3::new(x0, y0, 0.0), glam::Vec3::new(x1, y1, 0.0));
    scene.set_render_pixel_scale(1000.0, 1000.0);
    assert!(
        scene.underlay_resolution_stale(),
        "the new view asks for frames"
    );
    let start = iced::time::Instant::now();
    scene.refresh_underlay_resolution(start);
    scene.refresh_underlay_resolution(start + Duration::from_millis(200));
    let job = scene.take_reseed_job().expect("a job for the worker");
    assert_eq!(
        scene.apply_reseed(job.render()),
        1,
        "the page is drawn again"
    );
    assert!(
        !scene.underlay_resolution_stale(),
        "the raster followed the view"
    );
}

#[test]
fn an_underlay_on_a_sheet_follows_the_sheet_zoom() {
    let (mut scene, handle) = sheet_with_underlay("follows");

    // The whole page in view.
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);
    let whole = scene.images[&handle].clone();

    // Zoomed in on a hundredth of it: that part again, at the scale of the
    // view, in the raster with the whole page.
    look_at(&mut scene, 40.0, 40.0, 50.0, 50.0);
    let zoomed = &scene.images[&handle];
    assert!(
        !std::sync::Arc::ptr_eq(&whole.pixels, &zoomed.pixels),
        "the zoom step drew the page again"
    );
    assert!(
        zoomed.verts.len() > 6,
        "the part in view is cut out of the page: {} corners",
        zoomed.verts.len()
    );

    // The sheet's own list has the new raster, and so the frame.
    let (_, _, listed) = scene.paper_sheet_render_models();
    let page = listed
        .iter()
        .find(|model| model.entity == Some(handle))
        .expect("the page is on the sheet");
    assert!(
        std::sync::Arc::ptr_eq(&page.pixels, &zoomed.pixels),
        "the sheet shows the new raster"
    );
}

/// The model view left far out, where the page would show ten pixels wide,
/// does not make the page on the sheet a ten-pixel raster.
#[test]
fn the_model_view_has_no_say_in_a_sheet_underlay() {
    let (mut scene, handle) = sheet_with_underlay("model-view");
    scene.set_current_layout("Model".to_string());
    rest_at(&mut scene, -5000.0, -5000.0, 5000.0, 5000.0);
    if let Some(job) = scene.take_reseed_job() {
        scene.apply_reseed(job.render());
    }

    scene.set_current_layout("Sheet".to_string());
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);
    let shown = &scene.images[&handle];
    assert!(
        shown.width >= 400,
        "drawn for the size it shows on the sheet, got {} pixels",
        shown.width
    );
}

/// A sheet's zoom is not the model's: a sheet with no viewport shows
/// nothing of the model, and its zoom leaves an underlay of model space as
/// it is.
#[test]
fn a_sheet_zoom_leaves_a_model_underlay_alone() {
    let (mut scene, on_sheet) = sheet_with_underlay("model-underlay");
    scene.set_current_layout("Model".to_string());
    let definition = match scene.document.get_entity(on_sheet) {
        Some(EntityType::Underlay(underlay)) => underlay.definition_handle,
        _ => panic!("the underlay"),
    };
    let mut underlay = document::entities::Underlay::pdf();
    underlay.definition_handle = definition;
    let in_model = scene.add_entity(EntityType::Underlay(underlay));
    scene.set_current_layout("Sheet".to_string());
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);
    let before = std::sync::Arc::clone(&scene.images[&in_model].pixels);

    rest_at(&mut scene, 40.0, 40.0, 50.0, 50.0);
    let job = scene.take_reseed_job().expect("a job for the worker");
    assert_eq!(job.len(), 1, "the page on the sheet alone");
    scene.apply_reseed(job.render());
    assert!(
        std::sync::Arc::ptr_eq(&before, &scene.images[&in_model].pixels),
        "the page in model space keeps its raster"
    );
}

/// What the raster of `underlay` is drawn for: its background, and the scale
/// and view it follows.
fn drawn_for(
    scene: &Scene,
    underlay: document::entities::Underlay,
) -> ([f32; 4], Option<f64>, Option<[f64; 4]>) {
    match scene.raster_input_for(&EntityType::Underlay(underlay)) {
        Some(crate::scene::raster_reseed::RasterInput::Underlay {
            background,
            world_per_pixel,
            view,
            ..
        }) => (background, world_per_pixel, view),
        _ => panic!("an underlay with its definition"),
    }
}

/// An underlay is drawn first while it is on its way into the drawing,
/// before it has an owner: with a sheet in view it is drawn for that sheet,
/// as it is once it lies there.
#[test]
fn a_page_on_its_way_to_a_sheet_is_drawn_for_the_sheet() {
    let (mut scene, handle) = sheet_with_underlay("on-its-way");
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);
    let Some(EntityType::Underlay(stored)) = scene.document.get_entity(handle).cloned() else {
        panic!("the underlay");
    };
    let mut coming = stored.clone();
    coming.common.owner_handle = Handle::NULL;
    assert_ne!(
        scene.bg_color, scene.paper_bg_color,
        "a sheet is not the canvas"
    );
    let on_sheet = drawn_for(&scene, stored);
    assert_eq!(on_sheet.0, scene.paper_bg_color, "adjusted to the sheet");
    assert!(on_sheet.1.is_some(), "at the scale of the sheet's view");
    assert_eq!(drawn_for(&scene, coming), on_sheet);
}

/// A sheet is the block of a paper layout. An underlay in a block
/// definition is drawn wherever the block is inserted, and follows the model
/// view as it did.
#[test]
fn an_underlay_in_a_block_follows_the_model_view() {
    let (mut scene, handle) = sheet_with_underlay("in-a-block");
    scene.set_current_layout("Model".to_string());
    rest_at(&mut scene, 0.0, 0.0, 8.0, 8.0);
    let Some(EntityType::Underlay(stored)) = scene.document.get_entity(handle).cloned() else {
        panic!("the underlay");
    };
    let model = {
        let mut underlay = stored.clone();
        underlay.common.owner_handle = scene.model_space_block_handle();
        drawn_for(&scene, underlay)
    };
    assert!(model.1.is_some(), "the model view has a scale");
    let mut in_block = stored.clone();
    in_block.common.owner_handle = scene.document.allocate_handle();
    let block = drawn_for(&scene, in_block);
    assert_eq!((block.1, block.2), (model.1, model.2));
    // The page on the sheet that is not in view follows no view.
    let away = drawn_for(&scene, stored);
    assert_eq!((away.1, away.2), (None, None));
}
