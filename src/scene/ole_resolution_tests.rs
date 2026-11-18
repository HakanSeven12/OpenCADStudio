//! An OLE picture's raster follows the zoom of the layout it is in, model
//! space or a sheet, once the view settles.

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
