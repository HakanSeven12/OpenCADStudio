use super::*;

use crate::app::{Message, OpenCADStudio};
use crate::ui::window::plot::PlotDlgMsg;

const GEO_ORIGIN: [f64; 2] = [-742_925.271_389_167_2, -1_043_020.310_123_933_5];
const GEO_SIZE: [f64; 2] = [20.0, 10.0];

fn set_epsg(app: &mut OpenCADStudio, code: u16) {
    let crs = crate::io::crs::CrsDefinition::from_epsg(code).unwrap();
    crate::io::crs::set_document_crs(&mut app.tabs[app.active_tab].scene.document, &crs)
        .unwrap();
}

fn remove_crs(app: &mut OpenCADStudio) {
    let document = &mut app.tabs[app.active_tab].scene.document;
    let handle = crate::io::crs::model_space_geo_data_handle(document).unwrap();
    assert!(document.objects.remove(&handle).is_some());
    // Removing a GEODATA through the native owner graph also detaches the
    // extension-dictionary entry, leaving no stale link behind.
    let model_space = document.block_records.get("*Model_Space").unwrap().handle;
    let dictionary = document.extension_dictionary_handle(model_space).unwrap();
    if let Some(document::objects::ObjectType::Dictionary(dictionary)) =
        document.objects.get_mut(&dictionary)
    {
        dictionary
            .entries
            .retain(|(key, _)| !key.eq_ignore_ascii_case(crate::io::crs::GEODATA_DICTIONARY_KEY));
    }
}

fn geopdf_model_app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.add_entity(document::EntityType::Line(
        document::entities::Line::from_points(
            document::types::Vector3::new(GEO_ORIGIN[0], GEO_ORIGIN[1], 0.0),
            document::types::Vector3::new(
                GEO_ORIGIN[0] + GEO_SIZE[0],
                GEO_ORIGIN[1] + GEO_SIZE[1],
                0.0,
            ),
        ),
    ));
    app.tabs[i].scene.document.header.insertion_units = 6;
    set_epsg(&mut app, 5514);
    app.plot_dialog.geopdf = true;
    app.plot_dialog.to_file = true;
    app.plot_dialog.file_svg = false;
    app.plot_dialog.paper_space = false;
    app.plot_dialog.area = "Extents".into();
    app
}

fn vp_by_page(bytes: Vec<u8>) -> Vec<bool> {
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes)).unwrap();
    pdf.pages()
        .iter()
        .map(|page| page.raw().contains_key(b"VP"))
        .collect()
}

#[test]
fn geopdf_extents_frame_preserves_original_f64_model_coordinates() {
    let app = geopdf_model_app();
    let page = app.extents_plot_job().expect("georeferenced model extents");
    let frame = page.geopdf.expect("explicit GeoPDF frame");
    let expected_visible = {
        let scene = &app.tabs[app.active_tab].scene;
        let (min, max) = scene
            .visible_block_bounds(scene.model_space_block_handle())
            .unwrap();
        [min.x, min.y, max.x, max.y]
    };
    assert_eq!(frame.model_window, expected_visible);
    assert!(frame
        .model_window
        .iter()
        .zip(expected_visible)
        .any(|(actual, expected)| {
            (actual - GEO_ORIGIN[0]).abs() < 1e-6
                && *actual != expected as f32 as f64
        }));
    let survey_window = [
        GEO_ORIGIN[0],
        GEO_ORIGIN[1],
        GEO_ORIGIN[0] + GEO_SIZE[0],
        GEO_ORIGIN[1] + GEO_SIZE[1],
    ];
    let explicit = app
        .area_plot_job((
            survey_window[0],
            survey_window[1],
            survey_window[2],
            survey_window[3],
        ))
        .unwrap();
    assert_eq!(explicit.geopdf.unwrap().model_window, survey_window);

    let mut changed_output = app;
    changed_output.plot_dialog.file_svg = true;
    assert!(
        changed_output.extents_plot_job().unwrap().geopdf.is_some(),
        "a later PDF destination must not inherit the SVG device's suppression"
    );
}

#[test]
fn geopdf_missing_crs_or_units_falls_back_but_unsupported_scenes_are_errors() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.plot_dialog.geopdf = true;
    let window = (
        GEO_ORIGIN[0],
        GEO_ORIGIN[1],
        GEO_ORIGIN[0] + 1.0,
        GEO_ORIGIN[1] + 1.0,
    );
    assert!(app.geopdf_frame_for_window(window).unwrap().is_none());
    // A GEODATA that stops carrying a CRS (or exists with none) keeps the
    // same fallback: the export proceeds as a plain PDF.
    set_epsg(&mut app, 5514);
    remove_crs(&mut app);
    assert!(app.geopdf_frame_for_window(window).unwrap().is_none());

    set_epsg(&mut app, 5514);
    app.tabs[app.active_tab]
        .scene
        .document
        .header
        .insertion_units = 0;
    assert!(app.geopdf_frame_for_window(window).unwrap().is_none());
    app.tabs[app.active_tab]
        .scene
        .document
        .header
        .insertion_units = 6;
    assert!(app
        .geopdf_frame_for_window(window)
        .unwrap()
        .is_some());

    let original_rotation = app.tabs[app.active_tab].scene.camera.borrow().rotation;
    app.tabs[app.active_tab]
        .scene
        .camera
        .borrow_mut()
        .projection = crate::scene::view::camera::Projection::Perspective;
    assert!(app
        .geopdf_frame_for_window(window)
        .unwrap_err()
        .contains("perspective"));
    {
        let scene = &app.tabs[app.active_tab].scene;
        let mut camera = scene.camera.borrow_mut();
        camera.projection = crate::scene::view::camera::Projection::Orthographic;
        camera.rotation = glam::Quat::from_rotation_x(0.25);
    }
    assert!(app
        .geopdf_frame_for_window(window)
        .unwrap_err()
        .contains("top-down"));
    app.tabs[app.active_tab].scene.camera.borrow_mut().rotation = original_rotation;

    app.tabs[app.active_tab].scene.current_layout = "Layout1".into();
    assert!(app
        .geopdf_frame_for_window(window)
        .unwrap_err()
        .contains("root model-space"));
    app.tabs[app.active_tab].scene.current_layout = "Model".into();
    let block = app.tabs[app.active_tab].scene.model_space_block_handle();
    app.tabs[app.active_tab].scene.block_edit_block = Some(block);
    assert!(app
        .geopdf_frame_for_window(window)
        .unwrap_err()
        .contains("block editor"));
}

#[test]
fn geopdf_opt_in_falls_back_to_plain_pdf_without_crs_or_units() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.tabs[app.active_tab]
        .scene
        .add_entity(document::EntityType::Line(
            document::entities::Line::from_points(
                document::types::Vector3::ZERO,
                document::types::Vector3::new(10.0, 10.0, 0.0),
            ),
        ));
    app.plot_dialog.geopdf = true;
    app.plot_dialog.to_file = true;
    app.plot_dialog.file_svg = false;
    app.plot_dialog.area = "Extents".into();
    let _ = app.on_plot_dlg(PlotDlgMsg::Commit);

    let path = std::env::temp_dir().join(format!(
        "ocs-geopdf-plain-fallback-{}.pdf",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    app.export_pdf_direct(&path, false)
        .expect("missing CRS must fall back to plain PDF");
    assert!(path.exists());
    assert_eq!(vp_by_page(std::fs::read(&path).expect("fallback PDF")), [false]);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn pdf_destination_controls_geopdf_even_when_svg_was_selected() {
    let mut app = geopdf_model_app();
    app.plot_dialog.file_svg = true;
    app.plot_dialog.background = false;
    app.plot_dialog.open_when_done = false;
    let gui_path = std::env::temp_dir().join(format!(
        "ocs-geopdf-pdf-after-svg-{}.pdf",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&gui_path);

    let _ = app.on_plot_export_path_some(gui_path.clone());

    assert_eq!(
        vp_by_page(std::fs::read(&gui_path).expect("GUI PDF after SVG selection")),
        [true]
    );
    let _ = std::fs::remove_file(&gui_path);

    let direct_path = std::env::temp_dir().join(format!(
        "ocs-geopdf-direct-after-svg-{}.pdf",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&direct_path);
    app.export_pdf_direct(&direct_path, false).unwrap();
    assert_eq!(
        vp_by_page(std::fs::read(&direct_path).expect("direct EXPORTPDF after SVG selection")),
        [true]
    );
    let _ = std::fs::remove_file(&direct_path);
}

#[test]
fn an_export_path_ending_in_dwf_writes_a_dwf() {
    use crate::scene::model::underlay_vector::dwf as reader;
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.tabs[app.active_tab]
        .scene
        .add_entity(document::EntityType::Line(
            document::entities::Line::from_points(
                document::types::Vector3::ZERO,
                document::types::Vector3::new(10.0, 10.0, 0.0),
            ),
        ));
    app.plot_dialog.background = false;
    app.plot_dialog.open_when_done = false;
    let layout = app.tabs[app.active_tab].scene.current_layout.clone();
    let path = std::env::temp_dir().join(format!(
        "ocs-export-by-extension-{}.DWF",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    let _ = app.on_plot_export_path_some(path.clone());

    let bytes = std::fs::read(&path).expect("the export writes the file");
    let _ = std::fs::remove_file(&path);
    assert!(bytes.starts_with(b"(DWF V06.00)"), "a DWF, not a PDF");
    assert_eq!(reader::sheet_names(&bytes), Some(vec![layout]));
}

#[test]
fn quick_print_preserves_the_plot_area_instead_of_setting_window_permanently() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let line = app.tabs[app.active_tab]
        .scene
        .add_entity(document::EntityType::Line(
            document::entities::Line::from_points(
                document::types::Vector3::ZERO,
                document::types::Vector3::new(10.0, 10.0, 0.0),
            ),
        ));
    app.plot_dialog.geopdf = true;
    app.plot_dialog.file_svg = true;
    app.plot_dialog.area = "Extents".into();
    app.plot_dialog.background = false;
    app.plot_dialog.open_when_done = false;
    let old_error_revision = app.command_line.error_revision;

    let _ = app.on_quick_print_handles(vec![line]);

    assert_eq!(app.plot_dialog.area, "Extents");
    assert_eq!(
        app.command_line.error_revision, old_error_revision,
        "QuickPrint with missing CRS falls back to ordinary PDF output"
    );
}

#[test]
fn geopdf_flag_cannot_be_toggled_while_its_checkbox_is_hidden() {
    let mut app = geopdf_model_app();
    app.plot_dialog.geopdf = false;
    let flag = crate::ui::window::plot::PlotFlag::GeoPdf;
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(flag));
    assert!(app.plot_dialog.geopdf);

    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_SVG.into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(flag));
    assert!(
        app.plot_dialog.geopdf,
        "hidden SVG state leaves it unchanged"
    );

    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    app.plot_dialog.paper_space = true;
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(flag));
    assert!(
        app.plot_dialog.geopdf,
        "hidden paper-space state leaves it unchanged"
    );
}

#[test]
fn svg_only_plot_preferences_are_stored_but_ignored_by_pdf() {
    // Both SVG-only flags follow the same path: stored when SVG is the output,
    // ignored by the PDF options. One app covers both instead of two.
    let mut app = OpenCADStudio::new_for_test();
    assert!(!app.plot_dialog.non_scaling_stroke);
    assert!(!app.plot_dialog.text_as_outlines);
    assert!(!super::OpenCADStudio::pdf_plot_options(&app.plot_dialog).non_scaling_stroke);
    assert!(!super::OpenCADStudio::pdf_plot_options(&app.plot_dialog).text_as_outlines);

    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_SVG.into()));
    for flag in [
        crate::ui::window::plot::PlotFlag::NonScalingStroke,
        crate::ui::window::plot::PlotFlag::TextAsOutlines,
    ] {
        let _ = app.on_plot_dlg(PlotDlgMsg::Flag(flag));
    }
    assert!(app.plot_dialog.non_scaling_stroke);
    assert!(app.plot_dialog.text_as_outlines);
    assert!(super::OpenCADStudio::pdf_plot_options(&app.plot_dialog).non_scaling_stroke);
    assert!(super::OpenCADStudio::pdf_plot_options(&app.plot_dialog).text_as_outlines);

    // PDF ignores the stored SVG preference, and its hidden checkbox cannot
    // be toggled by a synthetic message.
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    for flag in [
        crate::ui::window::plot::PlotFlag::NonScalingStroke,
        crate::ui::window::plot::PlotFlag::TextAsOutlines,
    ] {
        let _ = app.on_plot_dlg(PlotDlgMsg::Flag(flag));
    }
    assert!(app.plot_dialog.non_scaling_stroke);
    assert!(app.plot_dialog.text_as_outlines);
}

/// A Model plot resolves top-level fills against the white sheet, not the
/// dark screen: colour 7 and ByBlock fills used to plot white, and a
/// hatch background colour was left out of the plot. (#2001)
#[test]
fn colour_7_and_byblock_hatches_plot_dark_from_model() {
    use document::entities::{BoundaryEdge, BoundaryPath, Hatch, HatchPattern, PolylineEdge};
    use document::types::{Color, Vector2};
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let colors = [Color::ByLayer, Color::Index(7), Color::ByBlock];
    for (k, color) in colors.into_iter().enumerate() {
        let x = k as f64 * 20.0;
        let mut path = BoundaryPath::new();
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            [[x, 0.0], [x + 10.0, 0.0], [x + 10.0, 10.0], [x, 10.0]]
                .into_iter()
                .map(|p| Vector2::new(p[0], p[1]))
                .collect(),
            true,
        )));
        let mut hatch = if k == 2 {
            Hatch::with_pattern(HatchPattern::new("ANSI31"))
        } else {
            Hatch::solid()
        };
        hatch.common.color = color;
        hatch.paths.push(path);
        if k == 2 {
            crate::entities::hatch::set_background_color(&mut hatch, &Color::Index(1));
        }
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Hatch(Box::new(hatch)));
    }
    let page = app.layout_plot_page();
    let colors: Vec<_> = page.content.hatches.iter().map(|h| h.color).collect();
    assert_eq!(colors.len(), 4, "{colors:?}");
    let red = colors.iter().filter(|c| c[..3] == [1.0, 0.0, 0.0]).count();
    let black = colors.iter().filter(|c| c[..3] == [0.0; 3]).count();
    assert_eq!((red, black), (1, 3), "{colors:?}");
}

/// Solids draw on screen as GPU meshes and a thickened wide polyline's band
/// is a fill: both must reach model and layout plots (#1904, #1882).
#[test]
fn solids_and_thick_band_fills_reach_model_and_layout_plots() {
    use document::entities::{LwPolyline, LwVertex, Solid3D};
    use document::types::Vector2;
    use document::EntityType;
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
        .expect("kernel box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&base).expect("sat");
    let mut solid = Solid3D::new();
    solid.set_sat_document(&sat);
    let solid = app.tabs[i].scene.add_entity(EntityType::Solid3D(solid));
    let mut band = LwPolyline::new();
    band.vertices = vec![
        LwVertex::new(Vector2::new(-20.0, 0.0)),
        LwVertex::new(Vector2::new(20.0, 0.0)),
    ];
    band.constant_width = 2.0;
    band.thickness = 3.0;
    let band = app.tabs[i].scene.add_entity(EntityType::LwPolyline(band));

    let check = |app: &OpenCADStudio, space: &str| {
        let page = app.layout_plot_page();
        let named = |handle: document::Handle| {
            let name = handle.value().to_string();
            page.content
                .wires
                .iter()
                .filter(move |wire| wire.name == name)
        };
        assert!(
            named(solid).any(|wire| wire.points.iter().any(|p| p[0].is_finite())),
            "{space}: the solid plots no edges"
        );
        assert!(
            named(band).any(|wire| !wire.fill_tris.is_empty()),
            "{space}: the thickened band plots no fill"
        );
    };
    check(&app, "model");
    let _ = app.update(Message::LayoutSwitch("Layout1".into()));
    let _ = app.run_command_line("MSPACE");
    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("PSPACE");
    check(&app, "layout");
}

/// A drawing on `Layout1` whose page setup names its sheet the way a
/// Windows printer driver does — a bare `A4` with landscape dimensions.
fn app_with_printer_named_sheet() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let _ = app.update(Message::LayoutSwitch("Layout1".into()));
    let i = app.active_tab;
    // Millimetre drawing units, so a ratio plots as written; the metre
    // case has its own test.
    app.tabs[i].scene.document.header.insertion_units = 4;
    let mut ps = app.tabs[i]
        .scene
        .plot_settings_for("Layout1")
        .expect("a fresh layout carries plot settings");
    ps.paper_size = "A4".into();
    ps.paper_width = 297.0;
    ps.paper_height = 210.0;
    // Such drivers spell the orientation in the dimensions, not the rotation.
    ps.rotation = document::objects::PlotRotation::None;
    assert!(app.tabs[i].scene.set_layout_plot_settings("Layout1", &ps));
    app
}

fn layout_paper(app: &OpenCADStudio) -> (String, f64, f64) {
    let ps = layout_settings(app);
    (ps.paper_size, ps.paper_width, ps.paper_height)
}

fn layout_settings(app: &OpenCADStudio) -> document::objects::PlotSettings {
    app.tabs[app.active_tab]
        .scene
        .plot_settings_for("Layout1")
        .unwrap()
}

fn layout_rotation(app: &OpenCADStudio) -> document::objects::PlotRotation {
    layout_settings(app).rotation
}

/// Store the drawing's plot settings for `Layout1` after `edit` touched them.
fn edit_layout_settings(
    app: &mut OpenCADStudio,
    edit: impl FnOnce(&mut document::objects::PlotSettings),
) {
    let i = app.active_tab;
    let mut ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    edit(&mut ps);
    assert!(app.tabs[i].scene.set_layout_plot_settings("Layout1", &ps));
}

#[test]
fn a_new_drawing_starts_with_a_compatible_default_layout() {
    use document::objects::{PlotRotation, PlotType, ScaledType};
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let _ = app.update(Message::LayoutSwitch("Layout1".into()));
    let ps = layout_settings(&app);
    assert_eq!(ps.paper_size, "ISO_A4_(210.00_x_297.00_MM)");
    assert_eq!((ps.paper_width, ps.paper_height), (210.0, 297.0));
    assert_eq!(ps.rotation, PlotRotation::Degrees90);
    assert_eq!(ps.printer_name, "none_device");
    assert_eq!(ps.plot_type, PlotType::Layout);
    assert_eq!(ps.scale_type, ScaledType::OneToOne);
    assert!(ps.flags.use_standard_scale && ps.flags.print_lineweights);
    // The sheet on screen is the landscape A4 the rotation describes.
    let ((x0, y0), (x1, y1)) = app.tabs[app.active_tab].scene.paper_limits().unwrap();
    assert_eq!((x1 - x0, y1 - y0), (297.0, 210.0));
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_dialog.orientation, "Landscape");
    assert!(!app.plot_dialog.upside_down);
}

#[test]
fn orientation_is_stored_as_a_rotation_of_the_medium() {
    use crate::ui::window::plot::PlotFlag;
    use document::objects::PlotRotation as R;
    // A newly picked sheet is the catalogue's portrait medium…
    let portrait_medium = [
        ("Portrait", false, R::None),
        ("Landscape", false, R::Degrees90),
        ("Portrait", true, R::Degrees180),
        ("Landscape", true, R::Degrees270),
    ];
    // …while the drawing's own landscape `A4` medium is kept, so the
    // same choices rotate relative to it.
    let landscape_medium = [
        ("Landscape", false, R::None),
        ("Portrait", false, R::Degrees90),
        ("Landscape", true, R::Degrees180),
        ("Portrait", true, R::Degrees270),
    ];
    for (pick_a3, cases) in [(true, portrait_medium), (false, landscape_medium)] {
        for (orientation, upside_down, rotation) in cases {
            let mut app = app_with_printer_named_sheet();
            let _ = app.on_plot_dialog_open();
            if pick_a3 {
                let a3 = "ISO_A3_(297.00_x_420.00_MM)";
                let _ = app.on_plot_dlg(PlotDlgMsg::Paper(a3.into()));
            }
            let _ = app.on_plot_dlg(PlotDlgMsg::Orientation(orientation.into()));
            if upside_down {
                let _ = app.on_plot_dlg(PlotDlgMsg::Flag(PlotFlag::UpsideDown));
            }
            let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
            let ps = layout_settings(&app);
            let medium = if pick_a3 {
                (297.0, 420.0)
            } else {
                (297.0, 210.0)
            };
            assert_eq!((ps.paper_width, ps.paper_height), medium, "{orientation}");
            assert_eq!(
                ps.rotation, rotation,
                "{orientation} upside-down={upside_down}"
            );
            // The next dialog reads the same choices back out of the rotation.
            let _ = app.on_plot_dialog_open();
            assert_eq!(app.plot_dialog.orientation, orientation);
            assert_eq!(app.plot_dialog.upside_down, upside_down);
            let sheet = super::plot_dialog_sheet_mm(&app.plot_dialog);
            let long_side = medium.0.max(medium.1);
            assert_eq!(sheet.0 > sheet.1, orientation == "Landscape", "{sheet:?}");
            assert_eq!(sheet.0.max(sheet.1), long_side);
        }
    }
}

#[test]
fn standard_scales_are_written_by_code_and_others_as_ratios() {
    use document::objects::ScaledType;
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Extents".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(
        crate::ui::window::plot::PlotFlag::FitToPaper,
    ));
    assert!(!app.plot_dialog.fit_to_paper);
    let _ = app.on_plot_dlg(PlotDlgMsg::Scale("1:100".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(ps.scale_type, ScaledType::OneToHundred);
    assert!(
        !ps.flags.use_standard_scale,
        "the bit stays clear, as the source application writes it"
    );
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 100.0));
    // The stored factor follows the millimetre convention: ratio × 25.4.
    assert!((ps.standard_scale_factor - 0.254).abs() < 1e-12);
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_dialog.scale, "1:100");
    // A ratio the format has no code for is a custom scale, spelled as the ratio.
    let _ = app.on_plot_dlg(PlotDlgMsg::Scale("1:3".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(ps.scale_type, ScaledType::CustomScale);
    assert!(!ps.flags.use_standard_scale);
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 3.0));
    // …and survives a reopen even though the drawing's scale list lacks it.
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_dialog.scale, "1:3");
    assert!((super::plot_dialog_scale_factor(&app.plot_dialog) - 1.0 / 3.0).abs() < 1e-12);
    // Fit to paper is its own scale type and carries the bit; a Layout
    // plot goes back to the 1:1 the dialog holds it at.
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(
        crate::ui::window::plot::PlotFlag::FitToPaper,
    ));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(ps.scale_type, ScaledType::ScaleToFit);
    assert!(ps.flags.use_standard_scale);
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Layout".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(ps.scale_type, ScaledType::OneToOne);
    assert!(!ps.flags.use_standard_scale);
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 1.0));
    assert!((ps.standard_scale_factor - 25.4).abs() < 1e-12);
}

#[test]
fn a_metre_drawing_stores_the_millimetres_it_really_plots_per_unit() {
    use document::objects::ScaledType;
    let mut app = app_with_printer_named_sheet();
    // Insertion units: metres. "1:100" then plots 10 mm per drawing unit.
    app.tabs[app.active_tab]
        .scene
        .document
        .header
        .insertion_units = 6;
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Extents".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(
        crate::ui::window::plot::PlotFlag::FitToPaper,
    ));
    let _ = app.on_plot_dlg(PlotDlgMsg::Scale("1:100".into()));
    assert!((super::plot_dialog_scale_factor(&app.plot_dialog) - 10.0).abs() < 1e-9);
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(
        ps.scale_type,
        ScaledType::CustomScale,
        "not the format's 1:100"
    );
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (10.0, 1.0));
    assert!((ps.standard_scale_factor - 254.0).abs() < 1e-9);
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_dialog.scale, "1:100");
}

#[test]
fn metre_model_a3_layout_uses_millimetre_paper_space() {
    use document::objects::{PlotPaperUnits, PlotType, ScaledType};

    let mut app = app_with_printer_named_sheet();
    let i = app.active_tab;
    app.tabs[i].scene.document.header.insertion_units = 6; // Metres.
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A3_(297.00_x_420.00_MM)".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Orientation("Landscape".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Layout".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);

    let ps = layout_settings(&app);
    assert_eq!(ps.plot_type, PlotType::Layout);
    assert_eq!(ps.paper_units, PlotPaperUnits::Millimeters);
    assert_eq!(ps.scale_type, ScaledType::OneToOne);
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 1.0));
    assert!((app.tabs[i].scene.paper_space_unit_factor() - 1.0).abs() < 1e-12);

    let ((x0, y0), (x1, y1)) = app.tabs[i].scene.paper_limits().unwrap();
    assert!(((x1 - x0) - 420.0).abs() < 1e-9);
    assert!(((y1 - y0) - 297.0).abs() < 1e-9);
    assert_eq!(app.tabs[i].scene.document.header.insertion_units, 6);
}

#[test]
fn metre_model_inch_page_still_uses_millimetre_paper_space() {
    use document::objects::{PlotPaperUnits, PlotType};

    let mut app = app_with_printer_named_sheet();
    let i = app.active_tab;
    app.tabs[i].scene.document.header.insertion_units = 6; // Metres.
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::PaperUnits("Inches".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Layout".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);

    let ps = layout_settings(&app);
    assert_eq!(ps.plot_type, PlotType::Layout);
    assert_eq!(ps.paper_units, PlotPaperUnits::Inches);
    assert!((ps.scale_numerator - 1.0).abs() < 1e-12);
    assert!((ps.scale_denominator - 25.4).abs() < 1e-12);
    assert!((app.tabs[i].scene.paper_space_unit_factor() - 1.0).abs() < 1e-12);
    let ((x0, y0), (x1, y1)) = app.tabs[i].scene.paper_limits().unwrap();
    assert!(((x1 - x0) - 297.0).abs() < 1e-9);
    assert!(((y1 - y0) - 210.0).abs() < 1e-9);
    assert_eq!(app.tabs[i].scene.document.header.insertion_units, 6);
}

#[test]
fn an_inch_layout_keeps_its_unit_and_converts_the_scale() {
    use document::objects::{PlotPaperUnits, ScaledType};
    let mut app = app_with_printer_named_sheet();
    edit_layout_settings(&mut app, |ps| ps.paper_units = PlotPaperUnits::Inches);
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Extents".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(
        crate::ui::window::plot::PlotFlag::FitToPaper,
    ));
    let _ = app.on_plot_dlg(PlotDlgMsg::Scale("1:100".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(
        ps.paper_units,
        PlotPaperUnits::Inches,
        "the layout's unit is not ours to change"
    );
    // 1 mm = 100 units is 1 inch = 2540 units on an inch page setup; the
    // standard code still names the choice, as the source application does.
    assert_eq!(ps.scale_type, ScaledType::OneToHundred);
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 2540.0));
    assert!((ps.standard_scale_factor - 1.0 / 2540.0).abs() < 1e-15);
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_dialog.scale, "1:100");
    // A layout plot keeps the paper space in millimetres: "1:1" is
    // `1 in = 25.4 units`.
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Layout".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(ps.scale_type, ScaledType::OneToOne);
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 25.4));
    assert!((ps.standard_scale_factor - 1.0 / 25.4).abs() < 1e-15);
}

#[test]
fn a_named_setup_and_the_layout_settings_are_written_alike() {
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A3_(297.00_x_420.00_MM)".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Extents".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(
        crate::ui::window::plot::PlotFlag::FitToPaper,
    ));
    let _ = app.on_plot_dlg(PlotDlgMsg::Scale("1:50".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(
        crate::ui::window::plot::PlotFlag::UpsideDown,
    ));
    let named = app.dialog_to_plotsettings();
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let layout = layout_settings(&app);
    assert_eq!(layout.paper_size, named.paper_size);
    assert_eq!(layout.printer_name, named.printer_name);
    assert_eq!(layout.margins, named.margins);
    assert_eq!(
        (layout.paper_width, layout.paper_height),
        (named.paper_width, named.paper_height)
    );
    assert_eq!(layout.rotation, named.rotation);
    assert_eq!(layout.plot_type, named.plot_type);
    assert_eq!(
        (layout.origin_x, layout.origin_y),
        (named.origin_x, named.origin_y)
    );
    assert_eq!(layout.scale_type, named.scale_type);
    assert_eq!(
        (layout.scale_numerator, layout.scale_denominator),
        (named.scale_numerator, named.scale_denominator)
    );
    assert_eq!(layout.standard_scale_factor, named.standard_scale_factor);
    assert_eq!(layout.flags, named.flags);
    assert_eq!(layout.shade_plot_mode, named.shade_plot_mode);
    assert_eq!(layout.shade_plot_resolution, named.shade_plot_resolution);
    assert_eq!(named.rotation, document::objects::PlotRotation::Degrees270);
    assert_eq!(named.scale_type, document::objects::ScaledType::OneToFifty);
}

#[test]
fn flags_the_dialog_does_not_edit_survive_a_round_trip() {
    let mut app = app_with_printer_named_sheet();
    edit_layout_settings(&mut app, |ps| {
        ps.flags.plot_viewport_borders = true;
        ps.flags.update_paper = true;
        ps.flags.zoom_to_paper_on_update = true;
        ps.flags.unknown_bits = 0x8000;
        ps.paper_image_origin_x = 3.5;
    });
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A3_(297.00_x_420.00_MM)".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert!(ps.flags.plot_viewport_borders);
    assert!(ps.flags.update_paper && ps.flags.zoom_to_paper_on_update);
    assert_eq!(ps.flags.unknown_bits, 0x8000);
    assert_eq!(ps.paper_image_origin_x, 3.5);
}

/// `Layout1` with a content viewport on layer `VPB`, plotted with the page
/// setup's plot-viewport-borders flag off (the reference default).
fn border_plotted(vpb_plots: bool) -> bool {
    let mut app = app_with_printer_named_sheet();
    edit_layout_settings(&mut app, |ps| ps.flags.plot_viewport_borders = false);
    assert!(!layout_settings(&app).flags.plot_viewport_borders);
    let i = app.active_tab;
    let scene = &mut app.tabs[i].scene;
    let mut layer = document::tables::Layer::new("VPB");
    layer.is_plottable = vpb_plots;
    scene.document.layers.add(layer).unwrap();
    let mut viewport = document::entities::Viewport::new();
    viewport.center = document::types::Vector3::new(150.0, 105.0, 0.0);
    viewport.width = 100.0;
    viewport.height = 80.0;
    viewport.id = 2;
    viewport.status = document::entities::ViewportStatusFlags::default_on();
    viewport.common.layer = "VPB".into();
    let handle = scene
        .document
        .add_entity_to_layout(
            document::EntityType::Viewport(Box::new(viewport)),
            "Layout1",
        )
        .unwrap();
    scene.notify_viewport_changed(handle);
    scene.bump_entities(&[(handle, crate::scene::ChangeKind::Added)]);
    let content = super::plot_scene_content(scene, false, None);
    let name = handle.value().to_string();
    content
        .wires
        .iter()
        .any(|wire| wire.name == name && wire.points.iter().any(|p| p[0].is_finite()))
}

#[test]
fn viewport_border_plots_with_the_page_setup_flag_off() {
    assert!(
        border_plotted(true),
        "border of a plotting layer was dropped"
    );
}

#[test]
fn viewport_border_on_a_no_plot_layer_stays_off_the_plot() {
    assert!(
        !border_plotted(false),
        "border of a no-plot layer was plotted"
    );
}

/// Give `Layout1` these stored margins and rotation, then a LIMMIN that does
/// not match them (as after a device change).
fn layout_with_margins_and_stale_limmin(
    margins: (f64, f64, f64, f64),
    rotation: document::objects::PlotRotation,
) -> OpenCADStudio {
    let mut app = app_with_printer_named_sheet();
    edit_layout_settings(&mut app, |ps| {
        ps.paper_size = "ISO_A4_(210.00_x_297.00_MM)".into();
        ps.paper_width = 210.0;
        ps.paper_height = 297.0;
        ps.rotation = rotation;
        ps.margins =
            document::objects::PaperMargin::new(margins.0, margins.1, margins.2, margins.3);
    });
    let i = app.active_tab;
    for object in app.tabs[i].scene.document.objects.values_mut() {
        if let document::objects::ObjectType::Layout(layout) = object {
            if layout.name == "Layout1" {
                layout.min_limits = (-20.0, -7.5);
            }
        }
    }
    app.tabs[i].scene.geometry_epoch += 1;
    app
}

/// Full-bleed sheet (no margins) with a LIMMIN left over from a device
/// that had them: paper (0,0) is the sheet corner, so a point at paper
/// (50,40) plots at 50,40 mm, as in the reference application.
#[test]
fn layout_plot_origin_ignores_a_stale_limmin() {
    use document::objects::PlotRotation;
    let mut app =
        layout_with_margins_and_stale_limmin((0.0, 0.0, 0.0, 0.0), PlotRotation::Degrees90);
    let ((x0, y0), (x1, y1)) = app.tabs[app.active_tab].scene.paper_limits().unwrap();
    assert!(x0.abs() < 1e-9 && y0.abs() < 1e-9, "sheet origin {x0} {y0}");
    assert!(
        (x1 - 297.0).abs() < 1e-9 && (y1 - 210.0).abs() < 1e-9,
        "{x1} {y1}"
    );
    let _ = app.on_plot_dialog_open();
    let page = app.layout_plot_page_for("Layout");
    let scale = page.scale as f64;
    let at = (
        (50.0 + page.offset_x) * scale,
        (40.0 + page.offset_y) * scale,
    );
    assert!(
        (at.0 - 50.0).abs() < 1e-6 && (at.1 - 40.0).abs() < 1e-6,
        "paper (50,40) plots at {at:?}"
    );
}

#[test]
fn layout_plot_moves_the_content_and_keeps_the_printable_clip_on_the_sheet() {
    use document::objects::PlotRotation;

    let mut app =
        layout_with_margins_and_stale_limmin((5.0, 7.0, 11.0, 13.0), PlotRotation::None);
    edit_layout_settings(&mut app, |ps| {
        ps.origin_x = 7.0;
        ps.origin_y = -10.0;
    });
    let _ = app.on_plot_dialog_open();
    let page = app.layout_plot_page_for("Layout");
    assert_eq!((page.offset_x, page.offset_y), (12.0, -3.0));
    assert_eq!(page.clip, Some((5.0, 7.0, 194.0, 277.0)));
    let saved = app.dialog_to_plotsettings();
    assert_eq!((saved.origin_x, saved.origin_y), (7.0, -10.0));
}

/// With margins the sheet starts at minus the margins as they land on the
/// turned medium, at every rotation; LIMMIN plays no part.
#[test]
fn layout_sheet_origin_follows_the_rotated_margins() {
    use document::objects::PlotRotation;
    let margins = (5.0, 10.0, 15.0, 20.0); // left, bottom, right, top of the medium
    for (rotation, expected) in [
        (PlotRotation::None, (-5.0, -10.0)),
        (PlotRotation::Degrees90, (-20.0, -5.0)),
        (PlotRotation::Degrees180, (-15.0, -20.0)),
        (PlotRotation::Degrees270, (-10.0, -15.0)),
    ] {
        let app = layout_with_margins_and_stale_limmin(margins, rotation);
        let scene = &app.tabs[app.active_tab].scene;
        let ((x0, y0), _) = scene.paper_limits().unwrap();
        assert!(
            (x0 - expected.0).abs() < 1e-9 && (y0 - expected.1).abs() < 1e-9,
            "{rotation:?}: sheet origin {x0} {y0}, expected {expected:?}"
        );
        // The printable area starts at paper (0,0).
        let ((px, py), _) = scene.printable_area_limits().unwrap();
        assert!(
            px.abs() < 1e-9 && py.abs() < 1e-9,
            "{rotation:?}: printable {px} {py}"
        );
    }
}

/// A layout whose paper image origin (DXF 148/149) is not zero has its
/// printable corner at minus that origin: a sheet border drawn there
/// plots at the paper corner instead of shifted by it (#2864).
#[test]
fn layout_sheet_origin_follows_the_paper_image_origin() {
    use document::objects::PlotRotation;
    let mut app =
        layout_with_margins_and_stale_limmin((2.0, 3.0, 0.0, 0.0), PlotRotation::None);
    let i = app.active_tab;
    for object in app.tabs[i].scene.document.objects.values_mut() {
        if let document::objects::ObjectType::Layout(layout) = object {
            if layout.name == "Layout1" {
                layout.paper_image_origin_x = -40.0;
                layout.paper_image_origin_y = -25.0;
            }
        }
    }
    app.tabs[i].scene.geometry_epoch += 1;
    let ((x0, y0), _) = app.tabs[i].scene.paper_limits().unwrap();
    assert!(
        (x0 - 38.0).abs() < 1e-9 && (y0 - 22.0).abs() < 1e-9,
        "sheet origin {x0} {y0}"
    );
    let ((px, py), _) = app.tabs[i].scene.printable_area_limits().unwrap();
    assert!(
        (px - 40.0).abs() < 1e-9 && (py - 25.0).abs() < 1e-9,
        "{px} {py}"
    );
    let _ = app.on_plot_dialog_open();
    let page = app.layout_plot_page_for("Layout");
    let scale = page.scale as f64;
    let at = (
        (40.0 + page.offset_x) * scale,
        (25.0 + page.offset_y) * scale,
    );
    assert!(
        (at.0 - 2.0).abs() < 1e-6 && (at.1 - 3.0).abs() < 1e-6,
        "printable corner plots at {at:?}"
    );
}

/// Open the metric page-setup fixture written by the commercial
/// application (see `tests/fixtures/plot/README.md`).
#[cfg(not(target_arch = "wasm32"))]
fn app_with_fixture() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/plot/page-setups-metric.dxf"
    );
    // json! escapes the path: a Windows CARGO_MANIFEST_DIR carries
    // backslashes, and a raw format! would put invalid \X escapes in the
    // JSON string.
    let reply = app.automation_op(&serde_json::json!({"op": "open", "path": path}).to_string());
    assert_eq!(reply["ok"], true, "{reply}");
    app
}

/// The page-setup fields the file format stores, minus handles.
fn stored_fields(ps: &document::objects::PlotSettings) -> String {
    format!(
        "{} | {} | {:?} | {:.6} {:.6} | {:.6} {:.6} {:.6} {:.6} | {:.6} {:.6} | {} {} \
         | {:.6} / {:.6} | {:.9} | {:?} | {:?} | {:?} {:?} {}",
        ps.printer_name,
        ps.paper_size,
        ps.paper_units,
        ps.paper_width,
        ps.paper_height,
        ps.margins.left,
        ps.margins.bottom,
        ps.margins.right,
        ps.margins.top,
        ps.origin_x,
        ps.origin_y,
        ps.rotation.to_code(),
        ps.plot_type.to_code(),
        ps.scale_numerator,
        ps.scale_denominator,
        ps.standard_scale_factor,
        ps.scale_type,
        ps.flags,
        ps.shade_plot_mode,
        ps.shade_plot_resolution,
        ps.shade_plot_dpi
    )
}

/// Every layout of the fixture survives "open the dialog, apply" without
/// a single stored field changing — the dialog reads the source
/// application's conventions and writes them back the same way.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fixture_page_setups_round_trip_through_the_dialog_unchanged() {
    use document::objects::{PlotRotation, ScaledType};
    let mut app = app_with_fixture();
    let a3_medium = (297.0106506347656, 419.9889831542968);
    let arch_d_medium = (914.4000244140626, 609.5999755859375);
    let expectations = [
        ("A4 1-100", PlotRotation::None, (297.0, 210.0), true),
        ("A3 - plotter", PlotRotation::Degrees90, a3_medium, false),
        ("ARCH D - PLOTTER", PlotRotation::None, arch_d_medium, true),
    ];
    for (layout, rotation, medium, pdf) in expectations {
        let orientation = "Landscape";
        let _ = app.update(Message::LayoutSwitch(layout.into()));
        let i = app.active_tab;
        let before = app.tabs[i].scene.plot_settings_for(layout).unwrap();
        assert_eq!(before.rotation, rotation, "{layout}");
        assert_eq!(
            (before.paper_width, before.paper_height),
            medium,
            "{layout}"
        );
        assert_eq!(before.scale_type, ScaledType::OneToOne, "{layout}");
        let _ = app.on_plot_dialog_open();
        let d = &app.plot_dialog;
        assert_eq!(d.orientation, orientation, "{layout}");
        assert!(!d.upside_down);
        assert_eq!(d.area, "Layout");
        assert_eq!(d.scale, "1:1", "{layout}: a 1:1 plot in either paper unit");
        assert_eq!(d.to_file, pdf, "{layout}");
        assert_eq!(
            super::plot_dialog_sheet_mm(d).0 > super::plot_dialog_sheet_mm(d).1,
            true
        );
        assert!(
            d.lineweights && d.apply_plot_styles && d.paperspace_last,
            "{layout}"
        );
        let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
        let after = app.tabs[i].scene.plot_settings_for(layout).unwrap();
        assert_eq!(stored_fields(&after), stored_fields(&before), "{layout}");
    }
    // The system printer's sheet keeps its driver's margins and lands on
    // the layout the way the source application places it: the medium's
    // top margin on the left, its left margin at the bottom.
    let _ = app.update(Message::LayoutSwitch("A3 - plotter".into()));
    let ((x0, y0), (x1, y1)) = app.tabs[app.active_tab].scene.paper_limits().unwrap();
    assert!((x0 + 20.02365112304687).abs() < 1e-9 && (y0 + 4.995333194732666).abs() < 1e-9);
    assert!((x1 - 399.96533203125).abs() < 1e-6 && (y1 - 292.015317440033).abs() < 1e-6);
}

/// Picking a new sheet on the fixture's PDF layout writes our canonical
/// portrait medium with the orientation in the rotation and the PDF
/// driver's margins for that medium.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fixture_pdf_layout_takes_a_new_sheet_in_canonical_form() {
    use document::objects::{PaperMargin, PlotRotation};
    let mut app = app_with_fixture();
    let _ = app.update(Message::LayoutSwitch("A4 1-100".into()));
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A3_(297.00_x_420.00_MM)".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = app.tabs[app.active_tab]
        .scene
        .plot_settings_for("A4 1-100")
        .unwrap();
    assert_eq!(ps.printer_name, "DWG To PDF.pc3");
    assert_eq!(ps.paper_size, "ISO_A3_(297.00_x_420.00_MM)");
    assert_eq!((ps.paper_width, ps.paper_height), (297.0, 420.0));
    assert_eq!(ps.rotation, PlotRotation::Degrees90);
    assert_eq!(ps.margins, PaperMargin::new(5.0, 17.0, 6.0, 18.0));
    // The sheet sits at minus its displayed margins: top on the left,
    // left at the bottom.
    let ((x0, y0), _) = app.tabs[app.active_tab].scene.paper_limits().unwrap();
    assert!(
        (x0 + 18.0).abs() < 1e-9 && (y0 + 5.0).abs() < 1e-9,
        "{x0} {y0}"
    );
}

#[test]
fn hide_paperspace_objects_is_its_own_flag() {
    use crate::ui::window::plot::PlotFlag;
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    assert!(!app.plot_dialog.hide_paperspace);
    // Shade plot "Hidden Line" is a shade mode, not this flag.
    let _ = app.on_plot_dlg(PlotDlgMsg::Shade("Hidden Line".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    assert!(!layout_settings(&app).flags.plot_hidden);
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(PlotFlag::HidePaperspace));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    assert!(layout_settings(&app).flags.plot_hidden);
    let _ = app.on_plot_dialog_open();
    assert!(app.plot_dialog.hide_paperspace);
}

#[test]
fn switching_the_page_setup_unit_respells_offsets_and_keeps_the_output() {
    use crate::ui::window::plot::PlotFlag;
    use document::objects::{PlotPaperUnits, ScaledType};
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let sheet_before = app.tabs[app.active_tab].scene.paper_limits().unwrap();
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Extents".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(PlotFlag::FitToPaper));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(PlotFlag::Center));
    let _ = app.on_plot_dlg(PlotDlgMsg::OffsetX("25.4".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Scale("1:100".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::PaperUnits("Inches".into()));
    let d = &app.plot_dialog;
    assert_eq!(d.offset_x, "1", "25.4 mm is 1 inch");
    assert_eq!(
        (
            d.custom_scale_paper.as_str(),
            d.custom_scale_drawing.as_str()
        ),
        ("1", "2540")
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(ps.paper_units, PlotPaperUnits::Inches);
    assert!(
        (ps.origin_x - 25.4).abs() < 1e-9,
        "the file keeps millimetres"
    );
    assert_eq!(ps.scale_type, ScaledType::OneToHundred);
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 2540.0));
    // Back to millimetres: the same physical setup, spelled in mm.
    let _ = app.on_plot_dlg(PlotDlgMsg::PaperUnits("Millimeters".into()));
    assert_eq!(app.plot_dialog.offset_x, "25.4");
    assert_eq!(app.plot_dialog.custom_scale_drawing, "100");
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Layout".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::PaperUnits("Inches".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    // An inch page setup over the millimetre paper space leaves the
    // sheet where it was.
    let ps = layout_settings(&app);
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 25.4));
    let sheet_after = app.tabs[app.active_tab].scene.paper_limits().unwrap();
    let size = |((x0, y0), (x1, y1)): ((f64, f64), (f64, f64))| (x1 - x0, y1 - y0);
    let (before, after) = (size(sheet_before), size(sheet_after));
    assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
}

/// The sheet a window plot leaves on the layout is the page that plot
/// makes: page (0, 0) and (width, height) taken back to paper space.
fn page_of_window_plot(app: &OpenCADStudio) -> ((f64, f64), (f64, f64)) {
    let job = app.window_plot_job().expect("a window plot job");
    let scale = f64::from(job.scale);
    (
        (-job.offset_x, -job.offset_y),
        (
            job.paper_w / scale - job.offset_x,
            job.paper_h / scale - job.offset_y,
        ),
    )
}

#[test]
fn a_fitted_window_plot_leaves_the_sheet_it_plotted() {
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Window".into()));
    // A window away from the sheet's centre, smaller than the sheet.
    app.plot_window = Some((20.0, 120.0, 170.0, 200.0));
    app.plot_dialog.window = app.plot_window;
    app.plot_dialog.fit_to_paper = true;
    app.plot_dialog.center = true;
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    assert_eq!(
        layout_settings(&app).scale_type,
        document::objects::ScaledType::ScaleToFit
    );
    let ((px0, py0), (px1, py1)) = page_of_window_plot(&app);
    let ((sx0, sy0), (sx1, sy1)) = app.tabs[app.active_tab].scene.paper_limits().unwrap();
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        near(sx0, px0) && near(sy0, py0) && near(sx1, px1) && near(sy1, py1),
        "the sheet ({sx0:.2}, {sy0:.2})-({sx1:.2}, {sy1:.2}) must be the page the \
         plot made ({px0:.2}, {py0:.2})-({px1:.2}, {py1:.2})"
    );
}

#[test]
fn custom_scale_fields_follow_and_drive_the_picker() {
    use crate::ui::window::plot::PlotFlag;
    use document::objects::ScaledType;
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Extents".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(PlotFlag::FitToPaper));
    let _ = app.on_plot_dlg(PlotDlgMsg::Scale("1:100".into()));
    let d = &app.plot_dialog;
    assert_eq!(
        (
            d.custom_scale_paper.as_str(),
            d.custom_scale_drawing.as_str()
        ),
        ("1", "100")
    );
    // A typed ratio the list knows is picked under its name…
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomScalePaper("2".into()));
    assert_eq!(app.plot_dialog.scale, "1:50");
    // …one it does not know becomes a ratio entry of its own.
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomScaleDrawing("6".into()));
    assert_eq!(app.plot_dialog.scale, "1:3");
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = layout_settings(&app);
    assert_eq!(ps.scale_type, ScaledType::CustomScale);
    assert_eq!((ps.scale_numerator, ps.scale_denominator), (1.0, 3.0));
    // Half-typed or nonsense values leave the scale alone.
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomScaleDrawing("".into()));
    assert_eq!(app.plot_dialog.scale, "1:3");
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomScaleDrawing("-4".into()));
    assert_eq!(app.plot_dialog.scale, "1:3");
}

#[test]
fn the_plot_origin_counts_from_the_printable_corner_unless_plotoffset_says_edge() {
    use crate::ui::window::plot::PlotFlag;
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Area("Extents".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(PlotFlag::Center));
    let _ = app.on_plot_dlg(PlotDlgMsg::OffsetX("10".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::OffsetY("20".into()));
    // The PDF driver's margins on the drawing's landscape A4 medium:
    // 5 left, 17 bottom.
    assert_eq!(app.dialog_plot_origin_mm(), (15.0, 37.0));
    let _ = app.run_command_line("PLOTOFFSET 1");
    assert_eq!(app.dialog_plot_origin_mm(), (10.0, 20.0));
    let _ = app.on_plot_dlg(PlotDlgMsg::PaperUnits("Inches".into()));
    let (x, y) = app.dialog_plot_origin_mm();
    assert!((x - 10.0).abs() < 1e-4, "unit change keeps the distance");
    assert!((y - 20.0).abs() < 1e-4);
}

#[test]
fn sending_a_plot_saves_the_dialog_into_the_layout_when_asked() {
    use crate::ui::window::plot::PlotFlag;
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    assert!(
        app.plot_dialog.save_to_layout,
        "on by default, as expected of a page setup"
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A3_(297.00_x_420.00_MM)".into()));
    // A preview never writes the layout.
    let _ = app.on_plot_dlg(PlotDlgMsg::Preview);
    assert_eq!(layout_paper(&app).0, "A4");
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A3_(297.00_x_420.00_MM)".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Commit);
    assert_eq!(layout_paper(&app).0, "ISO_A3_(297.00_x_420.00_MM)");
    // Switched off, a plot leaves the layout alone.
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Flag(PlotFlag::SaveToLayout));
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A2_(420.00_x_594.00_MM)".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::Commit);
    assert_eq!(layout_paper(&app).0, "ISO_A3_(297.00_x_420.00_MM)");
}

#[test]
fn paperupdate_decides_what_happens_to_an_unsupported_sheet() {
    use crate::io::paper_catalog::{resolve, Margins};
    use crate::io::plot_device::{PrinterCapabilities, PrinterMedia};
    let a3 = resolve("ISO_A3_(297.00_x_420.00_MM)").unwrap();
    let media = PrinterMedia {
        paper: a3.clone(),
        margins: Margins::uniform(3.0),
        borderless: false,
    };
    let caps = std::sync::Arc::new(PrinterCapabilities {
        media: vec![media],
        default_paper: Some(a3),
    });
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer("OCS Test Printer".into()));
    // PAPERUPDATE 0: the sheet is kept and the mismatch reported.
    let media = |caps| PlotDlgMsg::PrinterMedia("OCS Test Printer".into(), Some(caps));
    let _ = app.on_plot_dlg(media(caps.clone()));
    assert_eq!(app.plot_dialog.paper, "ISO_A4_(210.00_x_297.00_MM)");
    let last = app
        .command_line
        .history
        .last()
        .map(|line| line.text.clone());
    assert!(
        last.as_deref().unwrap_or("").contains("PAPERUPDATE = 0"),
        "{last:?}"
    );
    // PAPERUPDATE 1: the printer's default sheet takes over, in the
    // dialog's orientation.
    let _ = app.run_command_line("PAPERUPDATE 1");
    let _ = app.on_plot_dlg(media(caps));
    assert_eq!(app.plot_dialog.paper, "ISO_A3_(297.00_x_420.00_MM)");
    assert_eq!(
        super::plot_dialog_sheet_mm(&app.plot_dialog),
        (420.0, 297.0)
    );
}

#[test]
fn scale_names_parse_to_their_ratios() {
    use crate::scene::Scene;
    assert_eq!(Scene::parse_scale_name_ratio("1:250"), Some((1.0, 250.0)));
    assert_eq!(Scene::parse_scale_name_ratio("2:1"), Some((2.0, 1.0)));
    assert_eq!(
        Scene::parse_scale_name_ratio("1/8\" = 1'-0\""),
        Some((0.125, 12.0))
    );
    assert_eq!(
        Scene::parse_scale_name_ratio("3/32\" = 1'-0\""),
        Some((0.09375, 12.0))
    );
    assert_eq!(
        Scene::parse_scale_name_ratio("1'-0\" = 1'-0\""),
        Some((12.0, 12.0))
    );
    assert_eq!(
        Scene::parse_scale_name_ratio("6\" = 1'-0\""),
        Some((6.0, 12.0))
    );
    assert_eq!(Scene::parse_scale_name_ratio("Fit"), None);
    assert_eq!(Scene::parse_scale_name_ratio("1:0"), None);
    assert_eq!(super::ratio_scale_name(0.004), "1:250");
    assert_eq!(super::ratio_scale_name(25.4), "25.4:1");
    assert_eq!(super::ratio_scale_name(1.0 / 3.0), "1:3");
}

#[test]
fn dialog_shows_the_catalogue_sheet_for_a_driver_name() {
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_dialog.paper, "ISO_A4_(210.00_x_297.00_MM)");
    assert_eq!(app.plot_dialog.orientation, "Landscape");
    assert_eq!(
        super::plot_dialog_sheet_mm(&app.plot_dialog),
        (297.0, 210.0)
    );
}

#[test]
fn applying_an_unchanged_sheet_keeps_the_drawing_media_name() {
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let (name, w, h) = layout_paper(&app);
    assert_eq!(
        name, "A4",
        "the driver's own spelling must survive the dialog"
    );
    // …and so must the driver's landscape medium: unrotated, as stored.
    assert_eq!((w, h), (297.0, 210.0));
    assert_eq!(layout_rotation(&app), document::objects::PlotRotation::None);
}

#[test]
fn picking_another_sheet_writes_the_canonical_name() {
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A3_(297.00_x_420.00_MM)".into()));
    assert_eq!(
        super::plot_dialog_sheet_mm(&app.plot_dialog),
        (420.0, 297.0)
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let (name, w, h) = layout_paper(&app);
    assert_eq!(name, "ISO_A3_(297.00_x_420.00_MM)");
    assert_eq!((w, h), (297.0, 420.0));
    assert_eq!(
        layout_rotation(&app),
        document::objects::PlotRotation::Degrees90
    );
    // The next dialog remembers the sheet the user chose.
    assert_eq!(app.plot_paper.canonical, "ISO_A3_(297.00_x_420.00_MM)");
}

#[test]
fn pdf_output_is_stored_as_the_source_app_pdf_driver() {
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = app.tabs[app.active_tab]
        .scene
        .plot_settings_for("Layout1")
        .unwrap();
    assert_eq!(ps.printer_name, "DWG To PDF.pc3");
    // A changed device takes the PDF driver's printable area for the sheet.
    assert_eq!(
        ps.margins,
        document::objects::PaperMargin::new(5.0, 17.0, 6.0, 18.0)
    );
}

#[test]
fn svg_device_is_session_state_and_stores_as_pdf() {
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    // The SVG device behaves like file output…
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_SVG.into()));
    assert!(app.plot_dialog.to_file);
    assert!(app.plot_dialog.file_svg);
    // …but switching back to PDF (or a printer) clears it.
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    assert!(app.plot_dialog.to_file);
    assert!(!app.plot_dialog.file_svg);
    // The drawing never learns about SVG: page setups only know printers
    // and PDF drivers, so an SVG plot stores the PDF driver spelling.
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_SVG.into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = app.tabs[app.active_tab]
        .scene
        .plot_settings_for("Layout1")
        .unwrap();
    assert_eq!(ps.printer_name, "DWG To PDF.pc3");
}


#[test]
fn legacy_pdf_label_is_upgraded_on_save() {
    let mut app = app_with_printer_named_sheet();
    let i = app.active_tab;
    let mut ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    ps.printer_name = "Save to PDF file…".into();
    assert!(app.tabs[i].scene.set_layout_plot_settings("Layout1", &ps));
    let _ = app.on_plot_dialog_open();
    assert!(
        app.plot_dialog.to_file,
        "the legacy label still means PDF output"
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    assert_eq!(ps.printer_name, "DWG To PDF.pc3");
}

#[test]
fn a_plotter_configuration_from_source_app_is_preserved_with_its_margins() {
    let mut app = app_with_printer_named_sheet();
    let i = app.active_tab;
    let mut ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    ps.printer_name = "DWF6 ePlot.pc3".into();
    ps.paper_size = "ISO_A4_(210.00_x_297.00_MM)".into();
    ps.margins = document::objects::PaperMargin::new(5.793749, 17.793753, 5.793744, 17.793747);
    assert!(app.tabs[i].scene.set_layout_plot_settings("Layout1", &ps));
    let _ = app.on_plot_dialog_open();
    assert!(
        app.plot_dialog.to_file,
        "an unknown plotter plots to PDF here"
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let after = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    assert_eq!(after.printer_name, "DWF6 ePlot.pc3");
    assert_eq!(
        after.margins, ps.margins,
        "the source application's driver margins survive an unchanged setup"
    );
}

#[test]
fn changing_the_sheet_updates_the_printable_area() {
    let mut app = app_with_printer_named_sheet();
    let i = app.active_tab;
    let mut ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    ps.printer_name = "DWG To PDF.pc3".into();
    ps.paper_size = "ISO_A4_(210.00_x_297.00_MM)".into();
    ps.margins = document::objects::PaperMargin::new(5.0, 17.0, 6.0, 18.0);
    assert!(app.tabs[i].scene.set_layout_plot_settings("Layout1", &ps));
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper(
        "ISO_full_bleed_A4_(210.00_x_297.00_MM)".into(),
    ));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    assert_eq!(ps.paper_size, "ISO_full_bleed_A4_(210.00_x_297.00_MM)");
    assert_eq!(
        ps.margins,
        document::objects::PaperMargin::new(0.0, 1.0, 0.0, 1.0)
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::Paper("ISO_A3_(297.00_x_420.00_MM)".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    assert_eq!(
        ps.margins,
        document::objects::PaperMargin::new(5.0, 17.0, 6.0, 18.0)
    );
    assert_eq!((ps.paper_width, ps.paper_height), (297.0, 420.0));
}

#[test]
fn a_selected_printer_is_stored_by_name() {
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer("OCS Test Printer".into()));
    assert!(!app.plot_dialog.to_file);
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = app.tabs[app.active_tab]
        .scene
        .plot_settings_for("Layout1")
        .unwrap();
    assert_eq!(ps.printer_name, "OCS Test Printer");
    // Reopening resolves the stored name back to the same printer.
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_dialog.printer.as_deref(), Some("OCS Test Printer"));
    // A media answer for a printer that is no longer selected is ignored.
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    let caps = std::sync::Arc::new(crate::io::plot_device::PrinterCapabilities::default());
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterMedia(
        "OCS Test Printer".into(),
        Some(caps),
    ));
    assert!(app.plot_dialog.printer_media.is_none());
}

#[test]
fn a_custom_sheet_is_added_selected_persisted_and_written_with_its_margins() {
    use crate::ui::window::plot::{CustomPaperMsg as C, MarginSide};
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer(crate::ui::window::plot::OUT_PDF.into()));
    let custom = |m: C| PlotDlgMsg::CustomPaper(m);
    let _ = app.on_plot_dlg(custom(C::Open));
    // Seeded from the selected ISO A4 on the PDF driver, nameless.
    let draft = app.plot_dialog.custom_editor.clone().expect("editor open");
    assert_eq!(
        (draft.width.as_str(), draft.height.as_str()),
        ("210", "297")
    );
    assert_eq!(draft.margins, ["5", "17", "6", "18"]);
    assert!(draft.name.is_empty());
    let _ = app.on_plot_dlg(custom(C::Name("Roll 24".into())));
    let _ = app.on_plot_dlg(custom(C::Width("1500".into())));
    let _ = app.on_plot_dlg(custom(C::Height("609.6".into())));
    let _ = app.on_plot_dlg(custom(C::Margin(MarginSide::Left, "3".into())));
    let _ = app.on_plot_dlg(custom(C::Margin(MarginSide::Top, "4".into())));
    let _ = app.on_plot_dlg(custom(C::Add));
    assert!(
        app.plot_dialog.custom_editor.is_none(),
        "Add closes the editor"
    );
    assert_eq!(app.plot_dialog.paper, "Roll_24_(609.60_x_1500.00_MM)");
    assert_eq!(
        super::plot_dialog_sheet_mm(&app.plot_dialog),
        (1500.0, 609.6)
    );
    assert_eq!(
        app.current_config().plot.custom_papers.len(),
        1,
        "persisted in the plot config"
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let ps = app.tabs[app.active_tab]
        .scene
        .plot_settings_for("Layout1")
        .unwrap();
    assert_eq!(ps.paper_size, "Roll_24_(609.60_x_1500.00_MM)");
    assert_eq!((ps.paper_width, ps.paper_height), (609.6, 1500.0));
    assert_eq!(ps.rotation, document::objects::PlotRotation::Degrees90);
    assert_eq!(
        ps.margins,
        document::objects::PaperMargin::new(3.0, 17.0, 6.0, 4.0)
    );
    // Re-adding the same size replaces the definition instead of duplicating it.
    let _ = app.on_plot_dlg(custom(C::Open));
    assert_eq!(
        app.plot_dialog
            .custom_editor
            .as_ref()
            .map(|d| d.name.as_str()),
        Some("Roll 24")
    );
    let _ = app.on_plot_dlg(custom(C::Margin(MarginSide::Left, "9".into())));
    let _ = app.on_plot_dlg(custom(C::Add));
    assert_eq!(app.plot_dialog.custom_papers.len(), 1);
    assert_eq!(app.plot_dialog.custom_papers[0].margins.left, 9.0);
    // Remove drops the definition; the sheet itself stays selectable.
    let _ = app.on_plot_dlg(custom(C::Remove));
    assert!(app.plot_dialog.custom_papers.is_empty());
    assert_eq!(app.plot_dialog.paper, "Roll_24_(609.60_x_1500.00_MM)");
}

#[test]
fn an_invalid_custom_sheet_is_refused_with_a_reason() {
    use crate::ui::window::plot::{CustomPaperError, CustomPaperMsg as C};
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomPaper(C::Open));
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomPaper(C::Width("-5".into())));
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomPaper(C::Add));
    let draft = app
        .plot_dialog
        .custom_editor
        .as_ref()
        .expect("editor stays open");
    assert_eq!(draft.error, Some(CustomPaperError::Size));
    assert!(app.plot_dialog.custom_papers.is_empty());
    // Typing again clears the message.
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomPaper(C::Width("500".into())));
    assert!(app
        .plot_dialog
        .custom_editor
        .as_ref()
        .unwrap()
        .error
        .is_none());
    let _ = app.on_plot_dlg(PlotDlgMsg::CustomPaper(C::Cancel));
    assert!(app.plot_dialog.custom_editor.is_none());
}

// The driver-options editor is the CUPS flow; on Windows
// PrinterProperties routes to the system preferences dialog instead, so
// the editor under test never opens there.
#[cfg(not(target_os = "windows"))]
#[test]
fn driver_options_are_edited_remembered_and_sent_with_the_job() {
    use crate::io::print_to_printer::parse_lpoptions;
    const LPOPTIONS: &str = "MediaType/Media Type: *Stationery Photographic\n\
ColorModel/Print Color Mode: *RGB Gray\n\
cupsPrintQuality/Print Quality: *Normal High\n";
    let mut app = app_with_printer_named_sheet();
    let _ = app.on_plot_dialog_open();
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer("OCS Test Printer".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterProperties);
    let draft = app
        .plot_dialog
        .printer_editor
        .as_ref()
        .expect("editor opens");
    assert_eq!(draft.printer, "OCS Test Printer");
    assert!(draft.options.is_none(), "options load in the background");
    // An answer for another printer is ignored; the right one fills the editor.
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionsLoaded(
        "Other".into(),
        Ok(parse_lpoptions(LPOPTIONS)),
    ));
    assert!(app
        .plot_dialog
        .printer_editor
        .as_ref()
        .unwrap()
        .options
        .is_none());
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionsLoaded(
        "OCS Test Printer".into(),
        Ok(parse_lpoptions(LPOPTIONS)),
    ));
    let draft = app.plot_dialog.printer_editor.as_ref().unwrap();
    assert_eq!(draft.options.as_ref().map(|o| o.len()), Some(3));
    assert_eq!(draft.choices["ColorModel"], "RGB");
    assert!(draft.overrides().is_empty(), "defaults are not overrides");
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionSet(
        "ColorModel".into(),
        "Gray".into(),
    ));
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionSet(
        "cupsPrintQuality".into(),
        "High".into(),
    ));
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionsApply);
    assert!(
        app.plot_dialog.printer_editor.is_none(),
        "Apply closes the editor"
    );
    let remembered = &app.plot_dialog.driver_options["OCS Test Printer"];
    assert_eq!(remembered.len(), 2);
    assert_eq!(remembered["ColorModel"], "Gray");
    // The job carries exactly those overrides…
    let opts = app.plot_print_options(&app.plot_dialog);
    assert_eq!(
        opts.driver_options,
        vec![
            ("ColorModel".to_string(), "Gray".to_string()),
            ("cupsPrintQuality".to_string(), "High".to_string())
        ]
    );
    // …they survive in the config, and another printer gets none of them.
    assert_eq!(app.current_config().plot.driver_options.len(), 1);
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer("Another".into()));
    assert!(app
        .plot_print_options(&app.plot_dialog)
        .driver_options
        .is_empty());
    // Reopening the editor starts from the remembered choices; Reset then
    // Apply forgets them.
    let _ = app.on_plot_dlg(PlotDlgMsg::Printer("OCS Test Printer".into()));
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterProperties);
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionsLoaded(
        "OCS Test Printer".into(),
        Ok(parse_lpoptions(LPOPTIONS)),
    ));
    assert_eq!(
        app.plot_dialog.printer_editor.as_ref().unwrap().choices["ColorModel"],
        "Gray"
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionsReset);
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionsApply);
    assert!(app
        .plot_dialog
        .driver_options
        .get("OCS Test Printer")
        .is_none());
    // A failed listing is reported inside the editor, not lost.
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterProperties);
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionsLoaded(
        "OCS Test Printer".into(),
        Err("no cups".into()),
    ));
    assert_eq!(
        app.plot_dialog
            .printer_editor
            .as_ref()
            .unwrap()
            .error
            .as_deref(),
        Some("no cups")
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::PrinterOptionsCancel);
    assert!(app.plot_dialog.printer_editor.is_none());
}

#[test]
fn an_unknown_sheet_from_a_drawing_keeps_its_own_dimensions() {
    let mut app = app_with_printer_named_sheet();
    let i = app.active_tab;
    let mut ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
    ps.paper_size = "Roll_24_(609.60_x_1500.00_MM)".into();
    ps.paper_width = 1500.0;
    ps.paper_height = 609.6;
    assert!(app.tabs[i].scene.set_layout_plot_settings("Layout1", &ps));
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_dialog.paper, "Roll_24_(609.60_x_1500.00_MM)");
    assert_eq!(
        super::plot_dialog_sheet_mm(&app.plot_dialog),
        (1500.0, 609.6)
    );
    let _ = app.on_plot_dlg(PlotDlgMsg::SetCurrent);
    let (name, w, h) = layout_paper(&app);
    assert_eq!(name, "Roll_24_(609.60_x_1500.00_MM)");
    // The drawing's own landscape medium stays as it was stored.
    assert_eq!((w, h), (1500.0, 609.6));
    assert_eq!(layout_rotation(&app), document::objects::PlotRotation::None);
}

#[test]
fn direct_model_preview_normalizes_inherited_paper_layout_scale() {
    use document::entities::Line;
    use document::types::Vector3;

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let from = Vector3::new(10_000.0, 10_000.0, 0.0);
    let to = Vector3::new(20_000.0, 20_000.0, 0.0);
    app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(Line::from_points(from, to)));
    let _ = app.update(Message::LayoutSwitch("Layout1".into()));

    // A saved paper setup uses the paper-only Layout area and a fixed 1:1
    // scale. Direct PREVIEW inherits those fields after switching to Model.
    app.plot_dialog.area = "Layout".into();
    app.plot_dialog.paper_space = true;
    app.plot_dialog.fit_to_paper = false;
    app.plot_dialog.scale = "1:1".into();
    app.plot_dialog.center = true;
    app.plot_dialog.paper_width_mm = 210.0;
    app.plot_dialog.paper_height_mm = 297.0;
    let _ = app.update(Message::LayoutSwitch("Model".into()));

    app.refresh_current_plot_context();
    assert_eq!(app.plot_dialog.area, "Extents");
    assert!(app.plot_dialog.fit_to_paper);
    assert!(!app.plot_dialog.paper_space);

    let page = app.direct_plot_page().expect("model line plots");
    let map = |point: Vector3| {
        (
            (point.x + page.offset_x) * page.scale as f64,
            (point.y + page.offset_y) * page.scale as f64,
        )
    };
    for point in [from, to] {
        let (x, y) = map(point);
        assert!(
            x >= 0.0 && x <= page.paper_w && y >= 0.0 && y <= page.paper_h,
            "fitted model endpoint ({x}, {y}) is outside {} x {} at scale {}",
            page.paper_w,
            page.paper_h,
            page.scale
        );
    }
}

#[test]
fn direct_model_preview_keeps_an_explicit_non_fit_extents_scale() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.plot_dialog.area = "Extents".into();
    app.plot_dialog.fit_to_paper = false;
    app.plot_dialog.scale = "1:100".into();
    app.plot_dialog.center = false;
    app.plot_dialog.offset_x = "12.0".into();

    app.refresh_current_plot_context();

    assert!(!app.plot_dialog.fit_to_paper);
    assert_eq!(app.plot_dialog.scale, "1:100");
    assert!(!app.plot_dialog.center);
    assert_eq!(app.plot_dialog.offset_x, "12.0");
}
