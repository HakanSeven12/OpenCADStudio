use super::*;

fn sheet_width(limits: ((f64, f64), (f64, f64))) -> f64 {
    let ((x0, _), (x1, _)) = limits;
    x1 - x0
}

/// A page-setup change must reach the sheet bounds the next draw or plot reads,
/// without reopening the drawing: `set_layout_plot_settings` drops the
/// layout's cached sheet together with the plot-style caches.
#[test]
fn page_setup_scale_change_refreshes_cached_sheet_bounds() {
    let mut scene = Scene::new();
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());

    let mut ps = scene
        .plot_settings_for("Sheet")
        .expect("layout carries plot settings");
    ps.paper_width = 297.0;
    ps.paper_height = 420.0;
    ps.scale_type = document::objects::ScaledType::CustomScale;
    ps.flags.use_standard_scale = false;
    ps.scale_numerator = 1.0;
    ps.scale_denominator = 50.0;
    ps.standard_scale_factor = 1.0 / 50.0;
    assert!(scene.set_layout_plot_settings("Sheet", &ps));

    // Fill the per-layout cache the way a redraw or a plot does.
    let _ = scene.paper_viewport_handles();
    let at_1_50 = scene.paper_limits().expect("sheet at 1:50");

    ps.scale_denominator = 1.0;
    ps.standard_scale_factor = 1.0;
    assert!(scene.set_layout_plot_settings("Sheet", &ps));
    let at_1_1 = scene.paper_limits().expect("sheet at 1:1");

    let ratio = sheet_width(at_1_50) / sheet_width(at_1_1);
    assert!(
        (ratio - 50.0).abs() < 1e-6,
        "the 1:1 sheet must be 1/50 of the 1:50 sheet, got ratio {ratio} \
         ({at_1_50:?} vs {at_1_1:?})"
    );
}
