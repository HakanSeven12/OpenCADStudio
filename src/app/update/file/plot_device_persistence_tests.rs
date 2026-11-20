use super::*;

use super::*;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn picked_plot_style_rebinds_same_basename_across_ifccad_save_and_open() {
    use crate::io::plot_style::{PickedPlotStyle, PlotStyleEntry, PlotStyleTable};

    let mut app = OpenCADStudio::new_for_test();
    let i = app.active_tab;
    let layout = app.tabs[i].scene.current_layout.clone();
    let file = "review-binding.stb";
    app.tabs[i].scene.document.header.plotstyle_mode = false;
    let make_pick = |rgb| {
        let mut table = PlotStyleTable::identity(file);
        table.is_stb = true;
        table.named_entries.insert(
            "Normal".into(),
            PlotStyleEntry {
                name: "Normal".into(),
                color: Some(rgb),
                ..Default::default()
            },
        );
        let bytes = table.to_bytes().unwrap();
        PickedPlotStyle::from_bytes(file.into(), bytes).unwrap()
    };
    let initial = make_pick([0, 0, 255]);
    let replacement = make_pick([255, 0, 0]);
    assert_ne!(initial.source.sha256, replacement.source.sha256);
    for picked in [&initial, &replacement] {
        let id = app.tabs[i].id;
        let _task = app.update(Message::PlotStyleLoaded(
            id,
            layout.clone(),
            Ok(Some(picked.clone())),
        ));
        assert!(app.tabs[i].dirty);
        let tab = &app.tabs[i];
        let bytes = ocs_save::write_external_for_edit(
            &tab.scene.document,
            ocs_save::external_format("ifccad").unwrap(),
            true,
            tab.ifccad_session.as_ref(),
        )
        .unwrap();
        let (opened, session) = ocs_save::read_ifccad_for_edit(&bytes).unwrap();
        let restored = session
            .as_ref()
            .unwrap()
            .plot_style_source(&opened.document, &layout, file)
            .unwrap();
        assert_eq!(restored, &picked.source);
        app.tabs[i].scene.document = opened.document;
        app.tabs[i].note_document_loaded(session);
        let table = app.load_layout_plot_style(file).unwrap();
        assert_eq!(
            table.named_entries["Normal"].color,
            picked.table.named_entries["Normal"].color
        );
    }
}

/// Reopening the Plot dialog must keep the user's printer. Layouts in
/// real drawings carry plot settings with no device, and the auto-applied
/// page setup used to reset the choice to the system default on every
/// open.
#[test]
fn reopening_the_plot_dialog_keeps_the_chosen_printer() {
    let mut app = OpenCADStudio::new_for_test();
    // A real drawing's layout carries plot settings with no device; the
    // auto-applied setup used to reset the choice to the default.
    let mut ps = document::objects::PlotSettings::default();
    ps.printer_name = String::new();
    let layout = app.tabs[app.active_tab].scene.current_layout.clone();
    assert!(
        app.tabs[app.active_tab]
            .scene
            .set_layout_plot_settings(&layout, &ps),
        "the test layout must accept plot settings"
    );
    app.plot_dialog.printer = Some("Brother DCP-L2520D series".into());
    app.plot_dialog.to_file = false;
    let _ = app.on_plot_dialog_open();
    assert_eq!(
        app.plot_dialog.printer.as_deref(),
        Some("Brother DCP-L2520D series"),
        "the chosen printer must survive the auto-applied page setup"
    );
    assert!(!app.plot_dialog.to_file);
}

/// Reopening Plot in the same tab/layout keeps the user's most recent
/// paper, scale and CTB choices instead of reloading the layout page setup
/// over them. Regression test for #1654.
#[test]
fn reopening_same_plot_context_keeps_recent_settings() {
    let mut app = OpenCADStudio::new_for_test();

    // First open seeds from the drawing/layout as usual.
    let _ = app.on_plot_dialog_open();

    // User edits the high-frequency Plot settings.
    app.plot_dialog.paper = "ISO_A3_(297.00_x_420.00_MM)".into();
    app.plot_dialog.paper_width_mm = 297.0;
    app.plot_dialog.paper_height_mm = 420.0;
    app.plot_dialog.scale = "1:100".into();
    app.plot_dialog.fit_to_paper = false;
    app.plot_dialog.style_name = "office.ctb".into();
    app.plot_dialog.apply_plot_styles = true;

    // Reopening Ctrl+P in the same layout must not reapply the layout's
    // old page setup and erase those choices.
    let _ = app.on_plot_dialog_open();

    assert_eq!(app.plot_dialog.paper, "ISO_A3_(297.00_x_420.00_MM)");
    assert_eq!(app.plot_dialog.scale, "1:100");
    assert!(!app.plot_dialog.fit_to_paper);
    assert_eq!(app.plot_dialog.style_name, "office.ctb");
    assert!(app.plot_dialog.apply_plot_styles);
}

/// Opening a different drawing in a reused tab must invalidate the Plot
/// session cache. The first Plot dialog must read paper and CTB from the
/// newly opened document instead of showing the startup drawing's values.
#[test]
fn newly_opened_document_forces_first_plot_dialog_to_read_layout() {
    let mut app = OpenCADStudio::new_for_test();
    let i = app.active_tab;

    app.tabs[i].scene.current_layout = "Layout1".into();

    let mut ps = app.tabs[i]
        .scene
        .plot_settings_for("Layout1")
        .expect("Layout1 must exist");

    ps.paper_size = "ISO_A2_(420.00_x_594.00_MM)".into();
    ps.paper_width = 420.0;
    ps.paper_height = 594.0;
    ps.current_style_sheet = crate::io::plot_style::MONOCHROME_PLOT_STYLE.into();
    ps.flags.plot_plot_styles = true;

    assert!(app.tabs[i].scene.set_layout_plot_settings("Layout1", &ps));

    // Reproduce stale state inherited from the startup/previous drawing.
    app.plot_dialog.paper = "ISO_A4_(210.00_x_297.00_MM)".into();
    app.plot_dialog.style_name.clear();
    app.plot_dialog.apply_plot_styles = false;

    app.plot_prev = Some(app.plot_dialog.clone());
    app.plot_prev_context = Some((i, "Layout1".into()));

    // Installing another drawing must make that state unusable.
    app.reset_plot_session_for_opened_document();

    assert!(app.plot_prev.is_none());
    assert!(app.plot_prev_context.is_none());

    let _ = app.on_plot_dialog_open();

    assert_eq!(
        app.plot_dialog.paper, "ISO_A2_(420.00_x_594.00_MM)",
        "the first Plot opening must use the new drawing's paper"
    );

    assert!(
        app.plot_dialog
            .style_name
            .eq_ignore_ascii_case(crate::io::plot_style::MONOCHROME_PLOT_STYLE),
        "the first Plot opening must use the new drawing's CTB"
    );

    assert!(
        app.plot_dialog.apply_plot_styles,
        "the stored CTB must already be enabled on the first Plot opening"
    );
}

/// Print All Options is a shared batch-settings editor. On the first
/// opening after restarting OCS it must keep the persisted user choices
/// instead of replacing them with the active layout's page setup.
#[test]
fn print_all_options_keeps_persisted_plot_preferences_on_first_open() {
    let mut app = OpenCADStudio::new_for_test();

    // Simulate preferences restored from the previous application session.
    app.plot_dialog.paper = "ISO_A2_(420.00_x_594.00_MM)".into();
    app.plot_dialog.paper_width_mm = 420.0;
    app.plot_dialog.paper_height_mm = 594.0;
    app.plot_dialog.scale = "1:1000".into();
    app.plot_dialog.style_name = crate::io::plot_style::MONOCHROME_PLOT_STYLE.into();
    app.plot_dialog.apply_plot_styles = true;

    // Fresh process: no explicit Print All override exists yet.
    app.print_all_settings_override = false;

    let _ = app.on_print_all_options();

    assert_eq!(
        app.plot_dialog.paper, "ISO_A2_(420.00_x_594.00_MM)",
        "first Print All Options opening must keep the persisted paper"
    );

    assert_eq!(
        app.plot_dialog.scale, "1:1000",
        "first Print All Options opening must keep the persisted scale"
    );

    assert!(
        app.plot_dialog
            .style_name
            .eq_ignore_ascii_case(crate::io::plot_style::MONOCHROME_PLOT_STYLE),
        "first Print All Options opening must keep the persisted CTB"
    );

    assert!(
        app.plot_dialog.apply_plot_styles,
        "the persisted CTB must remain enabled"
    );

    assert!(
        app.print_all_options,
        "the dialog must remain in Print All Options mode"
    );
}

/// `<previous>` belongs to the layout that produced the snapshot.
/// Returning to that layout with "Save changes to layout" enabled must
/// preserve the restored paper/CTB instead of reloading the old embedded
/// page setup over it.
#[test]
fn previous_snapshot_is_saved_back_to_its_origin_layout() {
    let mut app = OpenCADStudio::new_for_test();
    let i = app.active_tab;

    app.tabs[i].scene.current_layout = "Layout1".into();

    // Layout1 still contains an old page setup.
    let mut stored = app.tabs[i]
        .scene
        .plot_settings_for("Layout1")
        .expect("Layout1 must exist");

    stored.paper_size = "ISO_A4_(210.00_x_297.00_MM)".into();
    stored.paper_width = 210.0;
    stored.paper_height = 297.0;
    stored.current_style_sheet.clear();
    stored.flags.plot_plot_styles = false;

    assert!(app.tabs[i]
        .scene
        .set_layout_plot_settings("Layout1", &stored));

    // `<previous>` is the desired state that originally belonged to
    // Layout1.
    app.print_all_options = true;
    app.plot_prev_context = Some((i, "Layout1".into()));
    app.plot_dialog.selected_setup = crate::ui::window::plot::SETUP_PREV.into();

    app.plot_dialog.paper = "ISO_A2_(420.00_x_594.00_MM)".into();
    app.plot_dialog.paper_width_mm = 420.0;
    app.plot_dialog.paper_height_mm = 594.0;
    app.plot_dialog.style_name = crate::io::plot_style::MONOCHROME_PLOT_STYLE.into();
    app.plot_dialog.apply_plot_styles = true;
    app.plot_dialog.save_to_layout = true;

    let _ = app.on_plot_dlg(crate::ui::window::plot::PlotDlgMsg::SelectSetup(
        "*Layout1*".into(),
    ));

    let saved = app.tabs[i]
        .scene
        .plot_settings_for("Layout1")
        .expect("Layout1 must still exist");

    assert_eq!(saved.paper_size, "ISO_A2_(420.00_x_594.00_MM)");

    assert!(saved
        .current_style_sheet
        .eq_ignore_ascii_case(crate::io::plot_style::MONOCHROME_PLOT_STYLE));

    assert!(saved.flags.plot_plot_styles);

    assert_eq!(app.plot_dialog.paper, "ISO_A2_(420.00_x_594.00_MM)");

    assert!(app
        .plot_dialog
        .style_name
        .eq_ignore_ascii_case(crate::io::plot_style::MONOCHROME_PLOT_STYLE));
}

/// A CTB selected in the dialog must be usable even when the parsed
/// active table was not restored with it. This is the state seen after
/// reopening a drawing: the dropdown can name the correct CTB while the
/// first preview otherwise plots using raw layer colours.
#[test]
fn selected_ctb_is_resolved_when_active_table_is_stale() {
    let mut app = OpenCADStudio::new_for_test();

    app.plot_dialog.style_name = crate::io::plot_style::MONOCHROME_PLOT_STYLE.into();
    app.plot_dialog.apply_plot_styles = true;
    app.plot_dialog.style_missing = false;

    // Reproduce a fresh-session mismatch: the visible selection exists,
    // but no matching parsed table is active yet.
    app.active_plot_style = None;

    let table = app
        .dialog_plot_style(&app.plot_dialog)
        .expect("selected CTB must be resolved by name");

    assert!(
        table
            .name
            .eq_ignore_ascii_case(crate::io::plot_style::MONOCHROME_PLOT_STYLE),
        "the table used for plotting must match the visible CTB selection"
    );
}

/// Printer capability discovery on merely opening Plot must never mutate
/// the paper stored in the drawing. PAPERUPDATE is applied only after an
/// explicit device/page-setup choice.
#[test]
fn opening_plot_does_not_replace_sheet_when_printer_media_arrives() {
    use crate::io::paper_catalog::{resolve, Margins};
    use crate::io::plot_device::{PrinterCapabilities, PrinterMedia};

    let a2 = resolve("ISO_A2_(420.00_x_594.00_MM)").expect("A2");
    let a4 = resolve("ISO_A4_(210.00_x_297.00_MM)").expect("A4");

    let caps = std::sync::Arc::new(PrinterCapabilities {
        media: vec![PrinterMedia {
            paper: a4.clone(),
            margins: Margins::uniform(3.0),
            borderless: false,
        }],
        default_paper: Some(a4),
    });

    let mut app = OpenCADStudio::new_for_test();
    app.plot_dialog.to_file = false;
    app.plot_dialog.printer = Some("OCS Test Printer".into());
    app.plot_dialog.paper = a2.canonical.to_string();
    app.plot_dialog.paper_update = 1;

    // No explicit printer change is pending: this models the asynchronous
    // capability reply caused only by opening the Plot dialog.
    app.plot_reconcile_printer_media = None;

    let _ = app.on_plot_dlg(crate::ui::window::plot::PlotDlgMsg::PrinterMedia(
        "OCS Test Printer".into(),
        Some(caps),
    ));

    assert_eq!(
        app.plot_dialog.paper, a2.canonical,
        "opening Plot must preserve the drawing's stored sheet"
    );
}

/// A layout whose plot settings name an explicit device wins over the
/// saved preference: selecting its setup is an explicit device choice.
#[test]
fn layout_device_overrides_the_saved_preference() {
    let mut app = OpenCADStudio::new_for_test();
    app.plot_dialog.printer = Some("Brother DCP-L2520D series".into());
    let mut ps = document::objects::PlotSettings::default();
    ps.printer_name = "Godex G500".into();
    app.load_plotsettings_into_dialog(&ps);
    assert_eq!(app.plot_dialog.printer.as_deref(), Some("Godex G500"));
    assert!(!app.plot_dialog.to_file);
}

/// A persisted plot window (saved with the dialog settings) seeds the
/// runtime window when the dialog reopens, so `area == "Window"` keeps
/// plotting the same rectangle instead of reporting an empty area.
#[test]
fn persisted_window_seeds_the_dialog_on_reopen() {
    let mut app = OpenCADStudio::new_for_test();
    assert!(app.plot_window.is_none());
    app.plot_dialog.area = "Window".into();
    app.plot_dialog.window = Some((-10.0, -5.0, 30.0, 15.0));
    let _ = app.on_plot_dialog_open();
    assert_eq!(app.plot_window, Some((-10.0, -5.0, 30.0, 15.0)));
}

/// A window freshly loaded from a layout's plot settings is mirrored into
/// the dialog so it persists with the next config save.
#[test]
fn layout_window_is_mirrored_into_the_dialog() {
    let mut app = OpenCADStudio::new_for_test();
    let mut ps = document::objects::PlotSettings::default();
    ps.plot_type = document::objects::PlotType::Window;
    ps.set_plot_window(1.0, 2.0, 3.0, 4.0);
    app.load_plotsettings_into_dialog(&ps);
    assert_eq!(app.plot_window, Some((1.0, 2.0, 3.0, 4.0)));
    assert_eq!(app.plot_dialog.window, Some((1.0, 2.0, 3.0, 4.0)));
}

/// A layout pointing at a PDF device switches the dialog to file output.
#[test]
fn pdf_device_switches_to_file_output() {
    let mut app = OpenCADStudio::new_for_test();
    app.plot_dialog.printer = Some("Brother DCP-L2520D series".into());
    let mut ps = document::objects::PlotSettings::default();
    ps.printer_name = "Microsoft Print to PDF".into();
    app.load_plotsettings_into_dialog(&ps);
    assert!(app.plot_dialog.to_file);
    assert!(app.plot_dialog.printer.is_none());
}
