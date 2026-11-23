//! A PDF underlay of model space seen through the viewports of a sheet is
//! drawn for that sheet: adjusted to the paper it shows on, at the scale of
//! the viewport that shows it largest, and again for the part of the model
//! the viewports in sight show. Back in model space it follows the model
//! view.

use super::*;
use document::entities::Viewport;
use document::types::Vector3;
use std::time::Duration;

/// A PDF underlay of a one-inch page drawn 100 by 100 at the origin of model
/// space, and layout "Sheet", the current one, with a viewport for each
/// `(centre, side, target, view height)`: a square of `side` at `centre` of
/// the sheet that looks at `target` of the model.
fn sheet_over_a_page(name: &str, viewports: &[([f64; 2], f64, [f64; 2], f64)]) -> (Scene, Handle) {
    use printpdf::{Mm, PdfDocument, PdfPage, PdfSaveOptions};
    let mut pdf = PdfDocument::new("viewport underlay");
    pdf.pages.push(PdfPage::new(Mm(25.4), Mm(25.4), Vec::new()));
    let bytes = pdf.save(&PdfSaveOptions::default(), &mut Vec::new());
    let path = format!("memory://viewport-underlay-{name}.pdf");
    crate::scene::model::pdf_raster::register_source(&path, std::sync::Arc::new(bytes));
    let mut scene = Scene::new();
    let definition = scene.document.allocate_handle();
    scene.document.objects.insert(
        definition,
        document::objects::ObjectType::UnderlayDefinition(
            document::objects::UnderlayDefinition::pdf(&path, "1"),
        ),
    );
    let mut underlay = document::entities::Underlay::pdf();
    underlay.definition_handle = definition;
    underlay.x_scale = 100.0;
    underlay.y_scale = 100.0;
    underlay.z_scale = 100.0;
    let handle = scene.add_entity(EntityType::Underlay(underlay));
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());
    for &(centre, side, target, view_height) in viewports {
        let mut viewport = Viewport::new();
        viewport.center = Vector3::new(centre[0], centre[1], 0.0);
        viewport.width = side;
        viewport.height = side;
        viewport.view_target = Vector3::new(target[0], target[1], 0.0);
        viewport.view_height = view_height;
        scene.add_entity(EntityType::Viewport(Box::new(viewport)));
    }
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

/// The same, and the page follows: the worker's job is drawn and taken.
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

/// What the raster of the underlay `handle` is drawn for now: its background,
/// and the scale and the model rectangle it follows.
fn drawn_for(scene: &Scene, handle: Handle) -> ([f32; 4], Option<f64>, Option<[f64; 4]>) {
    let entity = scene.document.get_entity(handle).expect("the underlay");
    match scene.raster_input_for(entity) {
        Some(crate::scene::raster_reseed::RasterInput::Underlay {
            background,
            world_per_pixel,
            view,
            ..
        }) => (background, world_per_pixel, view),
        _ => panic!("an underlay with its definition"),
    }
}

/// `value` is `expected`, taken up to the next quarter-octave step.
fn on_the_step_of(value: f64, expected: f64) -> bool {
    value >= expected * 0.999 && value <= expected * 1.2
}

#[test]
fn a_model_underlay_follows_the_zoom_of_the_sheet_it_shows_on() {
    // One viewport over the whole page, one to one.
    let (mut scene, handle) =
        sheet_over_a_page("follows", &[([50.0, 50.0], 100.0, [50.0, 50.0], 100.0)]);
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);
    let whole = scene.images[&handle].clone();

    // Zoomed in on a hundredth of the viewport: that part of the model again,
    // at the scale it shows at.
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
    let (_, scale, view) = drawn_for(&scene, handle);
    assert!(
        on_the_step_of(scale.expect("a scale"), 0.01),
        "ten units of the model over a thousand pixels: {scale:?}"
    );
    // The part of the model in sight, 40 to 50, with half of it around.
    assert_eq!(view, Some([35.0, 35.0, 55.0, 55.0]));
}

#[test]
fn a_model_underlay_seen_on_a_sheet_is_adjusted_to_the_paper() {
    let (mut scene, handle) =
        sheet_over_a_page("paper", &[([50.0, 50.0], 100.0, [50.0, 50.0], 100.0)]);
    assert_ne!(
        scene.bg_color, scene.paper_bg_color,
        "a sheet is not the canvas"
    );
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);
    assert_eq!(drawn_for(&scene, handle).0, scene.paper_bg_color);

    // Back in model space it shows on the canvas, at the scale of that view.
    scene.set_current_layout("Model".to_string());
    look_at(&mut scene, 0.0, 0.0, 10.0, 10.0);
    let (background, scale, _) = drawn_for(&scene, handle);
    assert_eq!(background, scene.bg_color);
    assert!(on_the_step_of(scale.expect("a scale"), 0.01), "{scale:?}");
}

/// Two viewports in sight, one to one and four to one: the page is drawn for
/// the one that shows it largest, and for what both show of the model.
#[test]
fn the_viewport_that_shows_the_page_largest_decides() {
    let (mut scene, handle) = sheet_over_a_page(
        "largest",
        &[
            ([50.0, 50.0], 100.0, [50.0, 50.0], 100.0),
            ([170.0, 50.0], 100.0, [20.0, 20.0], 25.0),
        ],
    );
    // The sheet from 0 to 250: a quarter of a unit to the pixel.
    look_at(&mut scene, 0.0, -75.0, 250.0, 175.0);
    let (_, scale, view) = drawn_for(&scene, handle);
    assert!(
        on_the_step_of(scale.expect("a scale"), 0.25 / 4.0),
        "a quarter of a unit to the pixel, four to one: {scale:?}"
    );
    // The first shows the model from 0 to 100, the second from 7.5 to 32.5;
    // together 0 to 100, with half of that around.
    assert_eq!(view, Some([-50.0, -50.0, 150.0, 150.0]));
}

/// A viewport that is not in sight shows nothing: the page is left as it is.
#[test]
fn a_viewport_out_of_sight_has_no_say() {
    let (mut scene, handle) = sheet_over_a_page(
        "out-of-sight",
        &[([50.0, 50.0], 100.0, [50.0, 50.0], 100.0)],
    );
    look_at(&mut scene, 0.0, 0.0, 100.0, 100.0);
    let before = std::sync::Arc::clone(&scene.images[&handle].pixels);

    rest_at(&mut scene, 300.0, 300.0, 310.0, 310.0);
    assert!(
        scene.take_reseed_job().is_none(),
        "nothing of the model is in sight"
    );
    assert!(std::sync::Arc::ptr_eq(
        &before,
        &scene.images[&handle].pixels
    ));
}

/// A whole plan beside a detail of it, sixty-four to one: what both show
/// together is too long to draw at the detail's scale, so the page is drawn
/// again for what the detail shows, and the plan is left to the whole raster.
#[test]
fn a_detail_beside_the_whole_plan_takes_the_rectangle() {
    let (mut scene, handle) = sheet_over_a_page(
        "detail",
        &[
            ([50.0, 50.0], 100.0, [50.0, 50.0], 100.0),
            ([170.0, 50.0], 100.0, [20.0, 20.0], 1.5625),
        ],
    );
    // The sheet from 0 to 250: a quarter of a unit to the pixel.
    look_at(&mut scene, 0.0, -75.0, 250.0, 175.0);
    let (_, scale, view) = drawn_for(&scene, handle);
    assert!(
        on_the_step_of(scale.expect("a scale"), 0.25 / 64.0),
        "a quarter of a unit to the pixel, sixty-four to one: {scale:?}"
    );
    // The detail shows the model from 19.21875 to 20.78125, a hundred units
    // of plan would be 25,600 pixels at its scale: its own part, with half
    // of it around.
    assert_eq!(view, Some([18.4375, 18.4375, 21.5625, 21.5625]));
    assert!(
        scene.images[&handle].verts.len() > 6,
        "the detail's part is cut out of the page"
    );
}
