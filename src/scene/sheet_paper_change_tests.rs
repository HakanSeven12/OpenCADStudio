//! A paper change that arrives without a geometry bump (a plugin's page
//! setup does this) still moves the white sheet: the sheet fill is cached
//! per layout, so the paper setter has to drop that copy itself.

use super::*;

fn sheet_fill_bounds(scene: &Scene) -> (f32, f32, f32, f32) {
    let (hatches, _, _) = scene.paper_sheet_render_models();
    let sheet = hatches.first().expect("the white sheet");
    let xs = sheet.boundary.iter().map(|p| p[0]);
    let ys = sheet.boundary.iter().map(|p| p[1]);
    (
        xs.clone().fold(f32::MAX, f32::min),
        ys.clone().fold(f32::MAX, f32::min),
        xs.fold(f32::MIN, f32::max),
        ys.fold(f32::MIN, f32::max),
    )
}

fn set_paper(scene: &mut Scene, width: f64, height: f64, left: f64, bottom: f64) {
    let mut ps = scene.plot_settings_for("Sheet").expect("plot settings");
    ps.paper_width = width;
    ps.paper_height = height;
    ps.margins.left = left;
    ps.margins.bottom = bottom;
    assert!(scene.set_layout_plot_settings("Sheet", &ps));
}

#[test]
fn a_paper_change_without_a_geometry_bump_moves_the_white_sheet() {
    let mut scene = Scene::new();
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());

    set_paper(&mut scene, 841.0, 594.0, 0.0, 0.0);
    scene.bump_geometry_no_blocks();
    let a1 = sheet_fill_bounds(&scene);

    // The page setup path of a plugin: settings written, no bump.
    set_paper(&mut scene, 420.0, 297.0, 20.0, 5.0);
    let a3 = sheet_fill_bounds(&scene);
    let limits = scene.paper_limits().expect("paper limits");
    let want = (
        limits.0 .0 as f32,
        limits.0 .1 as f32,
        limits.1 .0 as f32,
        limits.1 .1 as f32,
    );
    assert_ne!(a1, a3, "the sheet kept the A1 size after the change to A3");
    assert_eq!(a3, want, "the white sheet matches the new paper limits");

    set_paper(&mut scene, 841.0, 594.0, 0.0, 0.0);
    assert_eq!(sheet_fill_bounds(&scene), a1, "back on A1 the sheet is where it was");
}
