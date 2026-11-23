use super::*;


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
