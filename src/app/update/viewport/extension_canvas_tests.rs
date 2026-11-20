use super::*;

use super::*;

#[test]
fn translates_extension_anchors_with_the_snap_marker() {
    let mut hit = crate::snap::SnapResult {
        world: glam::DVec3::ZERO,
        screen: Point::new(150.0, 75.0),
        snap_type: crate::snap::SnapType::Extension,
        tangent_obj: None,
        extension_base: Some(Point::new(100.0, 150.0)),
        extension_base2: Some(Point::new(75.0, 200.0)),
        extension_origin: None,
        extension_dir: None,
        viewport: None,
        source: None,
        secondary_source: None,
        model_point: None,
    };
    let pane = iced::Rectangle {
        x: 32.0,
        y: 56.0,
        width: 600.0,
        height: 400.0,
    };
    snap_pixels_to_canvas(&mut hit, pane);
    assert_eq!(hit.screen, Point::new(182.0, 131.0));
    assert_eq!(hit.extension_base, Some(Point::new(132.0, 206.0)));
    assert_eq!(hit.extension_base2, Some(Point::new(107.0, 256.0)));
}
