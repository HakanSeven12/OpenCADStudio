//! An OLE picture's raster follows the zoom of the layout it is in, model
//! space or a sheet, once the view settles. At a zoom step, each underlay
//! and OLE picture is drawn again once, not twice (#3292).

use super::*;
use crate::io::ole_embed::{build_embedded_ole, EmbeddedImage};
use document::types::Vector3;
use std::time::Duration;

/// A 100×100 OLE picture (a metafile) on the sheet of layout "Sheet", which
/// is the current layout.
fn sheet_with_ole() -> (Scene, Handle) {
    let mut scene = Scene::new();
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());
    let picture = EmbeddedImage {
        bytes: crate::scene::model::gdi::test_hairline_emf(None),
        pixel_width: 100,
        pixel_height: 100,
        name: "picture.emf".to_string(),
        source: None,
    };
    let ole = build_embedded_ole(
        &picture,
        Vector3::new(0.0, 100.0, 0.0),
        Vector3::new(100.0, 0.0, 0.0),
    );
    let handle = scene.add_entity(EntityType::Ole2Frame(Box::new(ole)));
    (scene, handle)
}

/// Look at the sheet rectangle `[x0, y0]`–`[x1, y1]` in a 1000-pixel square
/// view, and let the view rest until the rasters have followed it.
fn look_at(scene: &mut Scene, x0: f32, y0: f32, x1: f32, y1: f32) {
    scene.zoom_to_window(glam::Vec3::new(x0, y0, 0.0), glam::Vec3::new(x1, y1, 0.0));
    scene.set_render_pixel_scale(1000.0, 1000.0);
    let start = iced::time::Instant::now();
    assert!(
        scene.underlay_resolution_stale(),
        "the new view asks for frames"
    );
    scene.refresh_underlay_resolution(start);
    scene.refresh_underlay_resolution(start + Duration::from_millis(200));
    // The rasters are drawn by a worker; the app takes them when it ends.
    if let Some(job) = scene.take_reseed_job() {
        scene.apply_reseed(job.render());
    }
    assert!(
        !scene.underlay_resolution_stale(),
        "the rasters followed the view"
    );
}

#[test]
fn an_ole_picture_on_a_sheet_follows_the_sheet_zoom() {
    let (mut scene, handle) = sheet_with_ole();
    assert_eq!(
        scene.images[&handle].width, 1600,
        "before the view is known, the picture keeps its default size"
    );

    // The whole picture in view: drawn near the size it shows, never larger.
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);
    let whole = &scene.images[&handle];
    assert!(
        (400..=1000).contains(&whole.width),
        "drawn at the size it shows on the sheet, got {}",
        whole.width
    );

    // Zoomed in on a tenth of it: that part again at screen resolution.
    look_at(&mut scene, 40.0, 40.0, 50.0, 50.0);
    let zoomed = &scene.images[&handle];
    assert!(
        zoomed.height > 1600 + 2 + 400,
        "the part in view is drawn sharper below the whole picture, got {}×{}",
        zoomed.width,
        zoomed.height
    );

    // A plot does not depend on the zoom: the picture plots at its own size.
    let entity = scene.document.get_entity(handle).expect("the picture");
    let printed = scene
        .print_underlay_image(entity)
        .expect("a picture sized to the view plots at its own size");
    assert_eq!((printed.width, printed.height), (1600, 1600));
}

// ── The sheet shows the raster a zoom step draws again ──

/// The frame of the sheet as the scene is now, on a 1000-pixel square
/// canvas, and the signature of each of its viewports.
fn sheet_frame(scene: &Scene) -> (crate::scene::view::render::Primitive, Vec<u64>) {
    let canvas = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 1000.0,
    };
    let primitive = scene.build_viewports(
        canvas,
        document::entities::ViewportRenderMode::Wireframe2D,
        None,
        false,
        false,
        [1.0; 4],
    );
    let signatures = primitive.frame_signatures(canvas);
    (primitive, signatures)
}

/// The sheet gathers its own pictures into a list of its own, kept from
/// frame to frame. A zoom step's new raster of a picture goes into that
/// list, with the triangles it is drawn with, and makes another frame.
#[test]
fn the_sheet_shows_the_raster_a_zoom_step_draws_again() {
    let (mut scene, handle) = sheet_with_ole();
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);

    // Zoomed in on a tenth of the picture, the view rests: a job waits.
    scene.zoom_to_window(
        glam::Vec3::new(40.0, 40.0, 0.0),
        glam::Vec3::new(50.0, 50.0, 0.0),
    );
    scene.set_render_pixel_scale(1000.0, 1000.0);
    let start = iced::time::Instant::now();
    scene.refresh_underlay_resolution(start);
    scene.refresh_underlay_resolution(start + Duration::from_millis(200));
    // The frame on screen while the worker draws. The renderer keeps the
    // lists of the frame it drew, as `shown` does here.
    let (shown, before) = sheet_frame(&scene);
    let old = std::sync::Arc::clone(&scene.images[&handle].pixels);

    let job = scene.take_reseed_job().expect("a job for the worker");
    assert_eq!(scene.apply_reseed(job.render()), 1);
    let fresh = scene.images[&handle].clone();
    assert!(
        !std::sync::Arc::ptr_eq(&old, &fresh.pixels),
        "the zoom step drew the picture again"
    );

    let (_, _, listed) = scene.paper_sheet_render_models();
    let picture = listed
        .iter()
        .find(|model| model.entity == Some(handle))
        .expect("the picture is on the sheet");
    assert!(
        std::sync::Arc::ptr_eq(&picture.pixels, &fresh.pixels),
        "the sheet's list has the new raster"
    );
    let corners =
        |model: &crate::scene::model::image_model::ImageModel| -> Vec<([f32; 3], [f32; 2])> {
            model.verts.iter().map(|v| (v.pos, v.uv)).collect()
        };
    assert_eq!(
        corners(picture),
        corners(&fresh),
        "and draws it with the raster's own triangles"
    );
    let (drawn, after) = sheet_frame(&scene);
    assert_ne!(before, after, "the new raster makes another frame");
    assert!(
        !std::sync::Arc::ptr_eq(&shown.viewports[0].images, &drawn.viewports[0].images),
        "with another list than the one the renderer has, so it is uploaded"
    );
}

// ── One render of each picture at a zoom step (#3292) ──

fn seeds() -> usize {
    entity::IMAGE_SEEDS.with(std::cell::Cell::get)
}

/// Four PDF underlays of a one-inch page, side by side in model space.
fn model_with_underlays() -> (Scene, Vec<Handle>) {
    use printpdf::{Mm, PdfDocument, PdfPage, PdfSaveOptions};
    let mut pdf = PdfDocument::new("reseed once");
    pdf.pages.push(PdfPage::new(Mm(25.4), Mm(25.4), Vec::new()));
    let bytes = pdf.save(&PdfSaveOptions::default(), &mut Vec::new());
    let path = "memory://reseed-once-test.pdf";
    crate::scene::model::pdf_raster::register_source(path, std::sync::Arc::new(bytes));
    let mut scene = Scene::new();
    let definition = scene.document.allocate_handle();
    scene.document.objects.insert(
        definition,
        document::objects::ObjectType::UnderlayDefinition(
            document::objects::UnderlayDefinition::pdf(path, "1"),
        ),
    );
    let handles = (0..4)
        .map(|k| {
            let mut underlay = document::entities::Underlay::pdf();
            underlay.definition_handle = definition;
            underlay.insertion_point = Vector3::new(f64::from(k) * 2.0, 0.0, 0.0);
            scene.add_entity(EntityType::Underlay(underlay))
        })
        .collect();
    (scene, handles)
}

/// Two OLE pictures on the sheet of layout "Sheet", the current layout.
fn sheet_with_ole_pictures() -> (Scene, Vec<Handle>) {
    let mut scene = Scene::new();
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());
    let handles = (0..2)
        .map(|k| {
            let picture = EmbeddedImage {
                bytes: crate::scene::model::gdi::test_hairline_emf(None),
                pixel_width: 100,
                pixel_height: 100,
                name: format!("picture-{k}.emf"),
                source: None,
            };
            let x = f64::from(k) * 120.0;
            let ole = build_embedded_ole(
                &picture,
                Vector3::new(x, 100.0, 0.0),
                Vector3::new(x + 100.0, 0.0, 0.0),
            );
            scene.add_entity(EntityType::Ole2Frame(Box::new(ole)))
        })
        .collect();
    (scene, handles)
}

#[test]
fn a_zoom_step_draws_each_underlay_once() {
    let (mut scene, handles) = model_with_underlays();
    assert!(
        handles.iter().all(|h| scene.images.contains_key(h)),
        "the underlays are drawn"
    );
    let before = seeds();
    look_at(&mut scene, 0.0, 0.0, 8.0, 8.0);
    assert_eq!(
        seeds() - before,
        handles.len(),
        "one render of each underlay"
    );
    assert!(
        handles.iter().all(|h| scene.images.contains_key(h)),
        "still drawn"
    );
}

#[test]
fn a_zoom_step_draws_each_ole_picture_once() {
    let (mut scene, handles) = sheet_with_ole_pictures();
    let before = seeds();
    look_at(&mut scene, 0.0, 0.0, 220.0, 100.0);
    assert_eq!(
        seeds() - before,
        handles.len(),
        "one render of each picture"
    );
    assert!(
        handles.iter().all(|h| scene.images.contains_key(h)),
        "still drawn"
    );
}
