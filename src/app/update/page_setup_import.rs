//! Importing named page setups from another drawing (`PSETUPIN`): the
//! "Import…" chooser of the Plot dialog and the command that drives the same
//! path from the command line.

use crate::app::{Message, OpenCADStudio};
use crate::ui::window::plot::{PageSetupImportDraft, PageSetupImportMsg, PlotDlgMsg};
use iced::Task;
use std::path::{Path, PathBuf};

/// Read the named page setups of the drawing at `path` off the UI thread.
/// The result names the file the way the chooser shows it.
fn read_page_setups_task(path: PathBuf) -> Task<Message> {
    super::file::background_task(
        move || {
            let file = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            crate::io::load_file(&path)
                .map(|document| (file, crate::scene::document_page_setups(&document)))
        },
        |result| Message::PlotDlg(PlotDlgMsg::Import(PageSetupImportMsg::Loaded(result))),
    )
}

impl OpenCADStudio {
    pub(super) fn on_page_setup_import(&mut self, msg: PageSetupImportMsg) -> Task<Message> {
        match msg {
            PageSetupImportMsg::Pick => Task::perform(
                async {
                    let handle = rfd::AsyncFileDialog::new()
                        .set_title(crate::t!("Select Drawing to Import Page Setups From").as_ref())
                        .add_filter(
                            crate::t!("Drawings and templates").as_ref(),
                            &["dwg", "dxf", "dwt", "DWG", "DXF", "DWT"],
                        )
                        .pick_file()
                        .await;
                    handle.map(|h| crate::sys::handle_path(&h))
                },
                |picked| match picked {
                    Some(path) => Message::PageSetupImportFile(path),
                    None => Message::Noop,
                },
            ),
            PageSetupImportMsg::Loaded(result) => {
                self.plot_dialog.import_draft = Some(match result {
                    Ok((file, setups)) => PageSetupImportDraft {
                        file,
                        setups: setups.into_iter().map(|(name, ps)| (name, ps, true)).collect(),
                        error: None,
                    },
                    Err(error) => PageSetupImportDraft {
                        file: String::new(),
                        setups: Vec::new(),
                        error: Some(error),
                    },
                });
                Task::none()
            }
            PageSetupImportMsg::Toggle(name) => {
                if let Some(draft) = self.plot_dialog.import_draft.as_mut() {
                    if let Some(entry) = draft.setups.iter_mut().find(|(n, _, _)| *n == name) {
                        entry.2 = !entry.2;
                    }
                }
                Task::none()
            }
            PageSetupImportMsg::All(on) => {
                if let Some(draft) = self.plot_dialog.import_draft.as_mut() {
                    for entry in &mut draft.setups {
                        entry.2 = on;
                    }
                }
                Task::none()
            }
            PageSetupImportMsg::Apply => {
                let Some(draft) = self.plot_dialog.import_draft.take() else {
                    return Task::none();
                };
                let chosen: Vec<(String, acadrust::objects::PlotSettings)> = draft
                    .selected()
                    .map(|(name, ps)| (name.to_string(), ps.clone()))
                    .collect();
                let imported = self.import_page_setups(chosen);
                if let Some(first) = imported.first() {
                    let first = first.clone();
                    self.select_page_setup(&first);
                }
                Task::none()
            }
            PageSetupImportMsg::Cancel => {
                self.plot_dialog.import_draft = None;
                Task::none()
            }
        }
    }

    /// A drawing was picked for import: read it in the background; the
    /// chooser appears in the Plot dialog once it is read.
    pub(super) fn on_page_setup_import_file(&mut self, path: PathBuf) -> Task<Message> {
        // The chooser lives in the Plot dialog; open it if the import was
        // started from the command line.
        let open = if self.active_modal == Some(crate::app::ModalKind::Plot) {
            Task::none()
        } else {
            self.on_plot_dialog_open()
        };
        Task::batch([open, read_page_setups_task(path)])
    }

    /// Bring the given setups into the drawing, replacing same-named ones,
    /// as one undoable step. Returns the names imported.
    pub(super) fn import_page_setups(
        &mut self,
        setups: Vec<(String, acadrust::objects::PlotSettings)>,
    ) -> Vec<String> {
        if setups.is_empty() {
            return Vec::new();
        }
        let i = self.active_tab;
        self.push_undo_snapshot(i, "PSETUPIN");
        let mut replaced = 0usize;
        let existing = self.tabs[i].scene.page_setup_names();
        let mut names = Vec::with_capacity(setups.len());
        for (name, ps) in setups {
            if existing.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                replaced += 1;
            }
            self.tabs[i].scene.import_page_setup(&name, ps);
            names.push(name);
        }
        self.tabs[i].dirty = true;
        self.refresh_page_setups();
        let count = names.len();
        self.command_line.push_output(
            crate::tf!("PSETUPIN: {count} page setup(s) imported, {replaced} replaced.").as_ref(),
        );
        names
    }

    /// `PSETUPIN` / `-PSETUPIN`: with no file, ask for one and offer its
    /// setups in the Plot dialog; with `<file> *` or `<file> name,name`
    /// import those straight away; with just `<file>` open the chooser on it.
    pub(in crate::app) fn on_psetupin(&mut self, args: &str) -> Task<Message> {
        let args = args.trim();
        if args.is_empty() {
            return self.on_page_setup_import(PageSetupImportMsg::Pick);
        }
        // The file may carry spaces; the setup list, when given, is the last
        // word (`*` or a comma-separated list without spaces), so the
        // arguments are a file followed by names only when what precedes the
        // last word is itself a file.
        let (file, names) = match args.rsplit_once(char::is_whitespace) {
            Some((file, names)) if Path::new(file.trim()).is_file() => {
                (file.trim(), Some(names.trim()))
            }
            _ => (args, None),
        };
        let path = PathBuf::from(file);
        let Some(names) = names else {
            return self.on_page_setup_import_file(path);
        };
        let available = match crate::io::load_file(&path) {
            Ok(document) => crate::scene::document_page_setups(&document),
            Err(error) => {
                self.command_line
                    .push_error(crate::tf!("PSETUPIN: {error}").as_ref());
                return Task::none();
            }
        };
        let chosen: Vec<(String, acadrust::objects::PlotSettings)> = if names == "*" {
            available
        } else {
            let wanted: Vec<&str> = names.split(',').map(str::trim).collect();
            let mut chosen = Vec::new();
            for want in wanted {
                match available.iter().find(|(n, _)| n.eq_ignore_ascii_case(want)) {
                    Some(found) => chosen.push(found.clone()),
                    None => self.command_line.push_error(
                        crate::tf!("PSETUPIN: no page setup named \"{want}\" in {file}.")
                            .as_ref(),
                    ),
                }
            }
            chosen
        };
        if chosen.is_empty() {
            self.command_line
                .push_info(crate::tf!("PSETUPIN: nothing to import from {file}.").as_ref());
            return Task::none();
        }
        self.import_page_setups(chosen);
        Task::none()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::window::plot::{PageSetupImportMsg as I, PlotDlgMsg};

    const FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/plot/page-setups-metric.dxf"
    );

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.update(Message::LayoutSwitch("Layout1".into()));
        app
    }

    fn last_line(app: &OpenCADStudio) -> String {
        app.command_line.history.last().map(|line| line.text.clone()).unwrap_or_default()
    }

    /// The fixture's page setups as the chooser would hold them, read
    /// through the same document reader the command uses.
    fn fixture_setups() -> Vec<(String, acadrust::objects::PlotSettings)> {
        let document = crate::io::load_file(std::path::Path::new(FIXTURE)).unwrap();
        crate::scene::document_page_setups(&document)
    }

    #[test]
    fn the_fixture_reader_lists_its_named_page_setups() {
        // The fixture carries its page setups on the layouts, not as named
        // setups; a drawing with named ones lists them in dictionary order.
        assert!(fixture_setups().is_empty());
        let mut app = app();
        let i = app.active_tab;
        let ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
        app.tabs[i].scene.page_setup_save("Site", ps.clone());
        app.tabs[i].scene.page_setup_save("Detail", ps);
        let names: Vec<String> = crate::scene::document_page_setups(&app.tabs[i].scene.document)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["Site", "Detail"]);
    }

    #[test]
    fn psetupin_imports_named_setups_from_a_saved_drawing() {
        use acadrust::objects::PlotRotation;
        // Author a source drawing with two named setups and save it.
        let dir = std::env::temp_dir().join(format!("ocs-psetupin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.dxf");
        {
            let mut app = app();
            let i = app.active_tab;
            let mut ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
            ps.printer_name = "DWG To PDF.pc3".into();
            ps.paper_size = "ISO_A3_(297.00_x_420.00_MM)".into();
            ps.paper_width = 297.0;
            ps.paper_height = 420.0;
            ps.rotation = PlotRotation::Degrees90;
            ps.plot_view_handle = acadrust::Handle::new(0xBEEF);
            app.tabs[i].scene.page_setup_save("Site", ps.clone());
            ps.paper_size = "ISO_A2_(420.00_x_594.00_MM)".into();
            app.tabs[i].scene.page_setup_save("Overview", ps);
            let save = format!(r#"{{"op":"save","path":"{}"}}"#, source.display());
            let reply = app.automation_op(&save);
            assert_eq!(reply["ok"], true, "{reply}");
        }
        let mut app = app();
        let i = app.active_tab;
        // One named setup already exists under a name the file also has.
        let own = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
        app.tabs[i].scene.page_setup_save("Site", own);
        // `*` imports everything, replacing the same-named one.
        let _ = app.run_command_line(&format!("PSETUPIN {} *", source.display()));
        // The report is translated on localized systems; the counts are not.
        let report = last_line(&app);
        assert!(report.starts_with("PSETUPIN"), "{report}");
        assert!(report.contains('2') && report.contains('1'), "{report}");
        let names = app.tabs[i].scene.page_setup_names();
        assert_eq!(names, ["Site", "Overview"]);
        let site = app.tabs[i].scene.page_setup_get("Site").unwrap();
        assert_eq!(site.paper_size, "ISO_A3_(297.00_x_420.00_MM)");
        assert_eq!(site.rotation, PlotRotation::Degrees90);
        assert!(site.plot_view_handle.is_null(), "foreign handles are dropped");
        assert!(app.tabs[i].dirty);
        // Undo takes the whole import back.
        let _ = app.update(Message::Undo);
        let names = app.tabs[i].scene.page_setup_names();
        assert_eq!(names, ["Site"]);
        assert_ne!(
            app.tabs[i].scene.page_setup_get("Site").unwrap().paper_size,
            "ISO_A3_(297.00_x_420.00_MM)"
        );
        // A name list imports just those and reports the unknown ones.
        let _ = app.run_command_line(&format!("PSETUPIN {} Overview,Nothing", source.display()));
        assert_eq!(app.tabs[i].scene.page_setup_names(), ["Site", "Overview"]);
        // A missing file is an error, not a crash.
        let _ = app.run_command_line("PSETUPIN /nowhere/at/all.dwg *");
        assert!(last_line(&app).starts_with("PSETUPIN:"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_chooser_imports_the_ticked_setups_and_selects_the_first() {
        let mut app = app();
        let i = app.active_tab;
        let ps = app.tabs[i].scene.plot_settings_for("Layout1").unwrap();
        let _ = app.on_plot_dialog_open();
        let setups = ["A", "B", "C"]
            .into_iter()
            .map(|name| (name.to_string(), ps.clone()))
            .collect();
        let loaded = Ok(("other.dwg".to_string(), setups));
        let _ = app.on_plot_dlg(PlotDlgMsg::Import(I::Loaded(loaded)));
        let draft = app.plot_dialog.import_draft.as_ref().expect("chooser open");
        assert_eq!(draft.file, "other.dwg");
        assert_eq!(draft.selected().count(), 3, "everything ticked to start with");
        let _ = app.on_plot_dlg(PlotDlgMsg::Import(I::All(false)));
        assert_eq!(app.plot_dialog.import_draft.as_ref().unwrap().selected().count(), 0);
        let _ = app.on_plot_dlg(PlotDlgMsg::Import(I::Toggle("B".into())));
        let _ = app.on_plot_dlg(PlotDlgMsg::Import(I::Toggle("C".into())));
        let _ = app.on_plot_dlg(PlotDlgMsg::Import(I::Apply));
        assert!(app.plot_dialog.import_draft.is_none(), "Apply closes the chooser");
        assert_eq!(app.tabs[i].scene.page_setup_names(), ["B", "C"]);
        assert_eq!(app.plot_dialog.selected_setup, "B");
        assert!(app.plot_dialog.page_setups.iter().any(|name| name == "C"));
        // A failed read shows its reason in the chooser; Cancel closes it.
        let _ = app.on_plot_dlg(PlotDlgMsg::Import(I::Loaded(Err("unreadable".into()))));
        assert_eq!(
            app.plot_dialog.import_draft.as_ref().unwrap().error.as_deref(),
            Some("unreadable")
        );
        let _ = app.on_plot_dlg(PlotDlgMsg::Import(I::Cancel));
        assert!(app.plot_dialog.import_draft.is_none());
    }

    #[test]
    fn a_new_layout_opens_its_page_setup_when_the_option_is_on() {
        let mut app = app();
        let _ = app.update(Message::LayoutCreate);
        assert_ne!(app.active_modal, Some(crate::app::ModalKind::Plot));
        let _ = app.update(Message::PageSetupOnNewLayoutChanged(true));
        assert!(app.current_config().plot.page_setup_on_new_layout, "persisted");
        let _ = app.update(Message::LayoutCreate);
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::Plot));
        assert_eq!(app.plot_dialog.selected_setup, "*Layout3*");
    }
}
