use super::*;
#[cfg(target_arch = "wasm32")]
use crate::io::plot_style::PlotStyleTableExt as _;

impl OpenCADStudio {
    /// The drawing templates (.dwt) in the default template's folder.
    pub(in crate::app) fn template_folder_files(&self) -> Vec<std::path::PathBuf> {
        let Some(dir) = std::path::Path::new(self.qnew_template.trim()).parent() else {
            return Vec::new();
        };
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("dwt"))
            })
            .collect();
        files.sort_by_key(|path| path.to_string_lossy().to_lowercase());
        files
    }

    pub(super) fn dispatch_fileops(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // QNEW starts from the default template (Options > Files); NEW
            // asks which template to start from (#1635).
            "QNEW" => return Some(Task::done(Message::TabNew)),
            "NEW" => {
                self.start_templates = None;
                // Open where the default template lives, so its neighbours in
                // a template folder are one click away (#1836).
                let template_dir = std::path::Path::new(self.qnew_template.trim())
                    .parent()
                    .filter(|dir| dir.is_dir())
                    .map(std::path::Path::to_path_buf);
                return Some(Task::perform(
                    async move {
                        let dialog = crate::sys::file_dialog();
                        let dialog = match template_dir {
                            Some(dir) => dialog.set_directory(dir),
                            None => dialog,
                        };
                        dialog
                            .set_title(crate::t!("Select Template").as_ref())
                            .add_filter(
                                crate::t!("Drawing Template (*.dwt)").as_ref(),
                                &["dwt", "DWT", "dwg", "DWG", "dxf", "DXF"],
                            )
                            .pick_file()
                            .await
                            .map(|handle| crate::sys::handle_path(&handle))
                    },
                    Message::NewFromTemplate,
                ));
            }
            // With FILEDIA 0, or while a script runs, OPEN and SAVEAS ask on
            // the command line instead of showing a file dialog.
            "OPEN" if self.file_prompts_on_command_line() => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("OPEN", "OPEN  Enter name of drawing to open:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            "OPEN" => return Some(Task::done(Message::OpenFile)),
            cmd if cmd.starts_with("OPEN ") => {
                let path = cmd["OPEN ".len()..].trim().trim_matches('"');
                if path.is_empty() {
                    return Some(Task::none());
                }
                return Some(self.open_from_command_line(std::path::PathBuf::from(path)));
            }
            "SAVE" | "QSAVE" => return Some(Task::done(Message::SaveFile)),
            // SAVEALL — write every open drawing that already has a file path.
            // Tabs without a path (never saved) are skipped with a note.
            "SAVEALL" => {
                if self.read_only {
                    self.command_line.push_error(
                        crate::t!("Read-only session (--read-only): saving is disabled.").as_ref(),
                    );
                    return Some(Task::none());
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let mut saved = 0usize;
                    let mut skipped = 0usize;
                    for t in 0..self.tabs.len() {
                        if self.tabs[t].is_start || self.tabs[t].block_test.is_some() {
                            continue;
                        }
                        if let Some(path) = self.tabs[t].current_path.clone() {
                            if self.active_save_jobs.contains_key(&self.tabs[t].id) {
                                self.command_line.push_info(
                                    crate::tf!(
                                        "SAVEALL: {} already has a save running",
                                        path.display()
                                    )
                                    .as_ref(),
                                );
                                skipped += 1;
                                continue;
                            }
                            match self.save_tab_synchronously_protected(t, path.clone(), false) {
                                Ok(()) => {
                                    saved += 1;
                                }
                                Err(e) => self.command_line.push_error(
                                    crate::tf!("SAVEALL: {} failed: {e}", path.display()).as_ref(),
                                ),
                            }
                        } else {
                            skipped += 1;
                        }
                    }
                    self.command_line.push_output(
                        crate::tf!(
                            "SAVEALL: saved {saved} drawing(s){}.",
                            if skipped > 0 {
                                crate::tf!("; {skipped} need SAVEAS (no file path yet)")
                                    .into_owned()
                            } else {
                                String::new()
                            }
                        )
                        .as_ref(),
                    );
                }
                #[cfg(target_arch = "wasm32")]
                {
                    // Web: every dirty saved tab is serialized and downloaded
                    // in one pass — see `saveall_web` for the flow.
                    return Some(self.saveall_web());
                }
                #[cfg(not(target_arch = "wasm32"))]
                return Some(Task::none());
            }
            "SAVEAS" if self.file_prompts_on_command_line() => {
                let doc = &self.tabs[i].scene.document;
                let is_dxf = crate::io::source_is_dxf(self.tabs[i].current_path.as_deref(), doc);
                let format = if is_dxf {
                    "DXF".to_string()
                } else {
                    let name = crate::io::format_for_version(doc.version, false);
                    name.trim_start_matches("DWG ").to_string()
                };
                let path = self.tabs[i]
                    .current_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("{}.dwg", self.tabs[i].tab_title));
                let flow = saveas_flow(format, path);
                let c = crate::command::ScriptPromptCommand::new("SAVEAS", flow);
                self.command_line.push_info(&c.prompt());
                self.command_line.set_step_options(c.options());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            "SAVEAS" => return Some(Task::done(Message::SaveAs)),
            cmd if cmd.starts_with("SAVEAS ") => {
                return Some(self.save_as_from_command_line(i, cmd["SAVEAS ".len()..].trim()));
            }
            // UNDO <n> — step back n operations at once; bare UNDO / U is one step.
            cmd if cmd.starts_with("UNDO ") => {
                let arg = cmd["UNDO ".len()..].trim();
                match arg.parse::<usize>() {
                    Ok(0) => return Some(Task::none()),
                    Ok(n) => return Some(Task::done(Message::UndoMany(n))),
                    Err(_) => {
                        self.command_line
                            .push_error(crate::t!("Usage: UNDO [number of steps]").as_ref());
                        return Some(Task::none());
                    }
                }
            }
            "UNDO" => return Some(Task::done(Message::Undo)),
            "REDO" => return Some(Task::done(Message::Redo)),
            // OOPS — restore the objects removed by the most recent ERASE,
            // without undoing any work done since.
            "OOPS" => {
                if self.oops_cache.is_empty() {
                    self.command_line
                        .push_info(crate::t!("OOPS: nothing to restore.").as_ref());
                } else {
                    let restored = std::mem::take(&mut self.oops_cache);
                    let pending = self.begin_undo(i, "OOPS", restored.len(), true);
                    let restored = self.tabs[i].scene.restore_erased_entities(restored);
                    let n = restored.len();
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    if let Some(pending) = pending {
                        self.commit_undo_delta(i, pending);
                    }
                    self.command_line
                        .push_output(crate::tf!("OOPS: restored {n} object(s).").as_ref());
                }
            }
            "CLEAR" | "CLR" => return Some(Task::done(Message::ClearScene)),
            // Visual-style commands. OCS renders either a wireframe or a shaded
            // view; the named styles map onto the closest of the two and the
            // chosen style is reported so the mapping is explicit. (`SOLID` is
            // intentionally NOT a visual-style verb — it is the 2D filled-polygon
            // draw command; the shaded ribbon button drives `SetWireframe`.)
            // One interactive picker behind every verb that asks for a style,
            // offering exactly what the render-mode widget offers. (#621)
            "VS" | "VSCURRENT" | "SHADEMODE" | "VISUALSTYLES" => {
                use crate::command::KeywordCommand;
                use crate::modules::view::visual_style;
                let c = KeywordCommand::new(
                    "VSCURRENT",
                    visual_style::keyword_prompt(),
                    visual_style::keyword_choices(),
                )
                .with_aliases(visual_style::SHORT_KEYS);
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            // A style by name: `VSCURRENT FLAT`, and what the picker dispatches.
            cmd if cmd.starts_with("VSCURRENT ")
                || cmd.starts_with("SHADEMODE ")
                || cmd.starts_with("VS ") =>
            {
                use crate::modules::view::visual_style;
                let style = cmd.split_whitespace().nth(1).unwrap_or("");
                let Some(mode) = visual_style::mode_for_keyword(style) else {
                    self.command_line
                        .push_error(crate::t!(visual_style::keyword_prompt()).as_ref());
                    return Some(Task::none());
                };
                let label = crate::t!(visual_style::label_for(mode));
                self.command_line
                    .push_output(crate::tf!("Visual style: {label}.").as_ref());
                return Some(Task::done(Message::SetRenderMode(mode)));
            }
            // CLOSE — close the active drawing tab (with the unsaved-changes
            // prompt the tab-close handler already runs).
            // Unsaved changes are asked about on the command line where a
            // script can answer (FILEDIA 0 or a running script).
            "CLOSE"
                if self.tabs[i].dirty
                    && !self.tabs[i].is_start
                    && self.file_prompts_on_command_line() =>
            {
                let name = self.drawing_file_name(i);
                let prompt =
                    crate::t!("CLOSE  Save changes to %{name}? [Yes/No] <Yes>:", name = name);
                self.ask_save_changes("CLOSE", prompt.into_owned());
            }
            // The close runs as its own message: this arm may be reached from
            // inside the prompt's own command step, which still indexes the tab.
            "CLOSE NO" => {
                self.tabs[i].dirty = false;
                return Some(Task::done(Message::TabClose(self.tabs[i].id)));
            }
            "CLOSE YES" => return Some(self.save_then(i, false)),
            "CLOSE" => {
                return Some(match self.tabs.get(self.active_tab) {
                    Some(tab) => Task::done(Message::TabClose(tab.id)),
                    None => Task::none(),
                });
            }

            // ARCHIVE / ETRANSMIT — package the drawing and its referenced files
            // (xrefs + raster images) into a sibling "<name>_archive" folder.
            // Web: there is no folder to copy into — the same dependency set is
            // collected from the session's bytes registries and delivered as a
            // "<name>_transmit.zip" download (`archive_web` below).
            "ARCHIVE" | "ETRANSMIT" => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use std::path::PathBuf;
                    if let Some(src) = self.tabs[i].current_path.clone() {
                        let stem = src
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "drawing".into());
                        let parent = src
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| PathBuf::from("."));
                        let folder = parent.join(format!("{stem}_archive"));
                        match std::fs::create_dir_all(&folder) {
                            Ok(()) => {
                                let mut copied = 0usize;
                                if let Some(fname) = src.file_name() {
                                    if std::fs::copy(&src, folder.join(fname)).is_ok() {
                                        copied += 1;
                                    }
                                }
                                // Resolve every reference the way the loaders
                                // did (stored path, relative to the drawing,
                                // then the bare name next to it), so what
                                // displays is what gets packaged.
                                let doc = &self.tabs[i].scene.document;
                                let mut found: Vec<PathBuf> = Vec::new();
                                let mut missing: Vec<String> = Vec::new();
                                let mut note = |raw: &str, hit: Option<PathBuf>| match hit {
                                    Some(p) if !found.contains(&p) => found.push(p),
                                    Some(_) => {}
                                    None if !missing.iter().any(|m| m == raw) => {
                                        missing.push(raw.to_string())
                                    }
                                    None => {}
                                };
                                for br in doc.block_records.iter() {
                                    let raw = br.xref_path.trim();
                                    if !raw.is_empty() {
                                        note(raw, crate::io::xref::resolve_path(raw, &parent));
                                    }
                                }
                                let file = |raw: &str| {
                                    crate::io::resolve_image_file(raw, Some(&parent))
                                        .map(PathBuf::from)
                                };
                                for e in doc.entities() {
                                    if let codec::EntityType::RasterImage(img) = e {
                                        let raw = img.file_path.trim();
                                        if !raw.is_empty() {
                                            note(raw, file(raw));
                                        }
                                    }
                                }
                                for o in doc.objects.values() {
                                    if let codec::objects::ObjectType::UnderlayDefinition(d) = o {
                                        let raw = d.file_path.trim();
                                        if !raw.is_empty() {
                                            note(raw, file(raw));
                                        }
                                    }
                                }
                                for resolved in &found {
                                    if let Some(fname) = resolved.file_name() {
                                        if std::fs::copy(resolved, folder.join(fname)).is_ok() {
                                            copied += 1;
                                        }
                                    }
                                }
                                self.command_line.push_output(
                                    crate::tf!(
                                        "{cmd}: packaged {copied} file(s) into {}",
                                        folder.display()
                                    )
                                    .as_ref(),
                                );
                                if !missing.is_empty() {
                                    let n = missing.len();
                                    let names = missing.join(", ");
                                    self.command_line.push_error(
                                        crate::tf!(
                                            "{cmd}: {n} referenced file(s) not found: {names}"
                                        )
                                        .as_ref(),
                                    );
                                }
                            }
                            Err(e) => self.command_line.push_error(
                                crate::tf!("{cmd}: cannot create folder ({e}).").as_ref(),
                            ),
                        }
                    } else {
                        self.command_line.push_error(
                            crate::t!("ARCHIVE: save the drawing first (it has no file path yet).")
                                .as_ref(),
                        );
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    // The drawing serialization runs on the serializer
                    // worker; the ZIP assembly + download continue in the
                    // completion message (`archive_web` below).
                    let task = self.archive_web(i, cmd);
                    return Some(Task::batch([task, self.finish_dispatch(cmd)]));
                }
            }

            // With FILEDIA 0 or in a script, each unsaved drawing is asked
            // about on the command line, so a script can end the program.
            "EXIT" | "QUIT"
                if cfg!(not(target_arch = "wasm32")) && self.file_prompts_on_command_line() =>
            {
                return Some(self.exit_from_command_line());
            }
            "EXIT NO" => {
                if let Some(d) = self.first_unsaved_drawing() {
                    self.tabs[d].dirty = false;
                }
                return Some(self.exit_from_command_line());
            }
            "EXIT YES" => {
                return Some(match self.first_unsaved_drawing() {
                    Some(d) => self.save_then(d, true),
                    None => self.exit_from_command_line(),
                });
            }
            "EXIT" | "QUIT" => {
                // Funnel through the OS close path so the unsaved-changes
                // dialog runs before `iced::exit()`. Falls back to a hard
                // exit if there's no main window registered yet.
                if let Some(id) = self.main_window {
                    return Some(Task::done(Message::WindowCloseRequested(id)));
                }
                return Some(self.exit_app());
            }

            // ── Frame-budget HUD (Phase 5.3) ───────────────────────────────
            // Toggle the per-rebuild wire-tessellation readout overlay.
            "PERF" => {
                self.perf_hud = !self.perf_hud;
                crate::perf::set_ui_enabled(self.perf_hud);
                self.command_line.push_info(
                    crate::t!(if self.perf_hud {
                        "PERF panel on — tracing render and interaction costs"
                    } else {
                        "PERF panel off"
                    })
                    .as_ref(),
                );
                return Some(Task::none());
            }

            // ── Background color ───────────────────────────────────────────
            // Usage:  BACKGROUND <r> <g> <b>      (0–255 each)
            //         BACKGROUND DEFAULT|BLACK|DARKGRAY|GRAY|LIGHTGRAY|WHITE  (preset)
            //         BACKGROUND DEFAULT          (restore the app default, rgb 33,40,48)
            // The chosen colour is also stored as the persisted default
            // (`default_bg_color` / `default_paper_bg_color`) so it survives
            // restarts and applies to new drawings (#188).
            // Bare BACKGROUND enters an interactive prompt for the colour, so
            // the command works both as a one-shot (`BACKGROUND BLACK`) and as a
            // type-then-choose flow (`BACKGROUND` ⏎, then `BLACK`). The prompt
            // delegates back to the inline handler below via `Dispatch`.
            "BACKGROUND" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "BACKGROUND",
                    "BACKGROUND  colour [Theme/Classic/Default/Black/DarkGray/Gray/LightGray/White] or R G B (0–255):",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("BACKGROUND ") => {
                let args = cmd.split_whitespace().skip(1).collect::<Vec<_>>();
                let is_paper = self.tabs[i].scene.current_layout != "Model";
                let first_arg = args.first().map(|s| s.to_uppercase()).unwrap_or_default();
                if first_arg == "THEME" || first_arg == "AUTO" {
                    self.model_space.mode = crate::app::config::ModelSpaceMode::MatchTheme;
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                    self.command_line.push_output(
                        crate::t!("Model space background set to Match Theme.").as_ref(),
                    );
                } else if first_arg == "CLASSIC" {
                    self.model_space.mode = crate::app::config::ModelSpaceMode::ClassicDark;
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                    self.command_line.push_output(
                        crate::t!("Model space background set to Classic CAD Dark.").as_ref(),
                    );
                } else if first_arg == "DEFAULT" || first_arg == "RESET" {
                    if is_paper {
                        self.model_space.custom_paper_bg = None;
                        self.paper_bg_input.clear();
                    } else {
                        self.model_space.mode = crate::app::config::ModelSpaceMode::MatchTheme;
                        self.model_space.custom_bg = None;
                        self.model_bg_input.clear();
                    }
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                    self.command_line
                        .push_output(crate::t!("Background reset to default.").as_ref());
                } else if first_arg == "DESK" {
                    let sub_args = &args[1..];
                    let sub_first = sub_args
                        .first()
                        .map(|s| s.to_uppercase())
                        .unwrap_or_default();
                    if sub_first == "DEFAULT" || sub_first == "RESET" {
                        self.model_space.custom_desk_bg = None;
                        self.sync_model_space_theme(false);
                        self.persist_settings_if_changed();
                        self.command_line.push_output(
                            crate::t!("Paper desk surround background reset to default.").as_ref(),
                        );
                    } else if let Some(rgba) = parse_background_color(sub_args) {
                        let [r, g, b, _] = rgba;
                        let rgb = [
                            (r * 255.0).round() as u8,
                            (g * 255.0).round() as u8,
                            (b * 255.0).round() as u8,
                        ];
                        self.model_space.custom_desk_bg = Some(rgb);
                        self.sync_model_space_theme(false);
                        self.persist_settings_if_changed();
                        self.command_line.push_output(
                            crate::tf!(
                                "Desk surround background: rgb({}, {}, {})",
                                rgb[0],
                                rgb[1],
                                rgb[2]
                            )
                            .as_ref(),
                        );
                    } else {
                        self.command_line.push_info(
                            crate::t!("Usage: BACKGROUND DESK <r> <g> <b> | DEFAULT").as_ref(),
                        );
                    }
                } else if let Some(rgba) = parse_background_color(&args) {
                    let [r, g, b, _] = rgba;
                    let rgb = [
                        (r * 255.0).round() as u8,
                        (g * 255.0).round() as u8,
                        (b * 255.0).round() as u8,
                    ];
                    if is_paper {
                        self.model_space.custom_paper_bg = Some(rgb);
                        self.paper_bg_input = crate::app::config::rgb_to_hex(rgb);
                    } else {
                        self.model_space.mode = crate::app::config::ModelSpaceMode::Custom;
                        self.model_space.custom_bg = Some(rgb);
                        self.model_bg_input = crate::app::config::rgb_to_hex(rgb);
                    }
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                    self.command_line.push_output(
                        crate::tf!("Background: rgb({}, {}, {})", rgb[0], rgb[1], rgb[2]).as_ref(),
                    );
                } else {
                    self.command_line.push_info(
                        crate::t!("Usage: BACKGROUND <r> <g> <b> (0–255) | THEME|CLASSIC|DEFAULT|DESK|BLACK|DARKGRAY|GRAY|LIGHTGRAY|WHITE").as_ref(),
                    );
                }
            }
            // ORTHO toggles the orthogonal cursor constraint — the standard
            // drafting aid, the same state the status-bar pill drives. Camera
            // projection is a separate concern: PARALLEL / PERSP, driven by the
            // Projection ribbon group.
            "ORTHO" => return Some(Task::done(Message::ToggleOrtho)),
            "PARALLEL" => return Some(Task::done(Message::SetProjection(true))),
            "PERSP" => return Some(Task::done(Message::SetProjection(false))),
            // LAYERS opens the Layer Manager pallet (its close button or
            // the toggle message close it).
            "LAYERS" => self.open_layers_pallet(),

            // SCRIPT <path> — run a command script: each non-comment
            // line is fed through the same command path the `--script` startup
            // flag uses. Blank rows submit Enter to the active command.
            "SCRIPT" | "SCR" => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    return Some(Task::perform(
                        async move {
                            crate::sys::file_dialog()
                                .add_filter("Script files (*.scr)", &["scr"])
                                .pick_file()
                                .await
                                .map(|h| crate::sys::handle_path(&h))
                        },
                        |path| match path {
                            Some(p) => Message::Command(format!("SCRIPT \"{}\"", p.display())),
                            None => Message::Noop,
                        },
                    ));
                }
                #[cfg(target_arch = "wasm32")]
                {
                    use crate::command::ValuePromptCommand;
                    let c = ValuePromptCommand::new("SCRIPT", "SCRIPT  path to the .scr file:");
                    self.command_line.push_info(&c.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(c));
                }
            }
            cmd if cmd.starts_with("SCRIPT ") || cmd.starts_with("SCR ") => {
                let path = cmd.split_once(' ').map(|(_, r)| r.trim().to_string());
                match path {
                    Some(p) if !p.is_empty() => {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let clean_path = p.trim().trim_matches(|c| c == '"' || c == '\'');
                            match std::fs::read_to_string(clean_path) {
                                Ok(text) => return Some(self.run_script(clean_path, &text)),
                                Err(e) => {
                                    self.command_line.push_error(
                                        crate::tf!("SCRIPT: cannot read {clean_path}: {e}")
                                            .as_ref(),
                                    );
                                }
                            }
                        }
                        // Web: nothing is behind a typed path — pick the
                        // script through the browser and read its bytes at
                        // pick time (the `pick_and_load_web` pattern); the
                        // picked text is fed to the same runner.
                        #[cfg(target_arch = "wasm32")]
                        {
                            let _ = &p;
                            return Some(Task::perform(
                                crate::io::pick_script_web(),
                                Message::ScriptFilePicked,
                            ));
                        }
                    }
                    _ => {
                        self.command_line
                            .push_info(crate::t!("Usage: SCRIPT <path to .scr file>").as_ref());
                    }
                }
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }

    /// Announce a script and feed its rows to the same runner the `--script`
    /// startup flag uses — shared by the native `SCRIPT <path>` read and the
    /// web picker's `ScriptFilePicked`.
    pub(in crate::app) fn run_script(&mut self, path: &str, text: &str) -> Task<Message> {
        let p = path;
        let lines = script_command_lines(text);
        self.command_line.push_output(
            crate::tf!("SCRIPT: running {} command(s) from {p}.", lines.len()).as_ref(),
        );
        self.script_queue.extend(lines);
        Task::done(Message::ScriptPump)
    }

    /// Unattended run with no window (`--batch` / `--each`): the script's
    /// rows run once, or once per drawing in `files` (opened first, closed
    /// after without saving what the script did not save). The command line
    /// is printed as it runs. Returns 0, or 1 when a row reported an error.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn run_batch(&mut self, text: &str, files: &[std::path::PathBuf]) -> i32 {
        let lines = script_command_lines(text);
        let errors = self.command_line.error_revision;
        if files.is_empty() {
            self.run_batch_rows(lines.clone());
        }
        for file in files {
            if self.exit_requested {
                break;
            }
            let before = self.command_line.error_revision;
            self.run_batch_rows(vec![format!("OPEN \"{}\"", file.display())]);
            if self.command_line.error_revision == before {
                self.run_batch_rows(lines.clone());
            }
            let open: Vec<u64> = self
                .tabs
                .iter()
                .filter(|tab| !tab.is_start)
                .map(|tab| tab.id)
                .collect();
            for id in open {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.dirty = false;
                }
                let task = self.update(Message::TabClose(id));
                let _ = self.drive_headless_task(task);
            }
        }
        i32::from(self.command_line.error_revision != errors)
    }

    /// Feed batch rows one at a time, printing what each one wrote to the
    /// command line. Stops at EXIT / QUIT.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_rows(&mut self, rows: Vec<String>) {
        self.script_queue.extend(rows);
        while let Some(row) = self.script_queue.pop_front() {
            if self.exit_requested {
                break;
            }
            self.script_running = true;
            self.command_line.history.clear();
            let task = self.update(Message::ScriptLine(row));
            if let Err(error) = self.drive_headless_task(task) {
                if !self.exit_requested {
                    self.command_line.push_error(&error);
                }
            }
            self.pump_plugin_requests();
            for entry in &self.command_line.history {
                if entry.kind == crate::ui::command_line::EntryKind::Error {
                    eprintln!("{}", entry.text);
                } else {
                    println!("{}", entry.text);
                }
            }
        }
        self.script_queue.clear();
    }

    /// Run the next script row once the previous one has finished,
    /// including the open or save it started; `update` resumes a paused
    /// script when that work ends.
    pub(in crate::app) fn pump_script(&mut self) -> Task<Message> {
        if self.script_busy() {
            self.script_paused = true;
            return Task::none();
        }
        let Some(line) = self.script_queue.pop_front() else {
            self.script_running = false;
            return Task::none();
        };
        self.script_running = true;
        self.feed_script_line(&line)
            .chain(Task::done(Message::ScriptPump))
    }

    /// An open or a save is still running.
    pub(in crate::app) fn script_busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        let saving =
            !self.active_save_jobs.is_empty() || self.pending_native_thumbnail_save.is_some();
        #[cfg(target_arch = "wasm32")]
        let saving = false;
        self.opening.is_some() || !self.pending_opens.is_empty() || saving
    }

    /// File commands ask on the command line, not in a dialog.
    pub(in crate::app) fn file_prompts_on_command_line(&self) -> bool {
        !crate::sys::file_dia() || self.script_running
    }

    /// The drawing's file name, or its tab title before its first save.
    fn drawing_file_name(&self, i: usize) -> String {
        self.tabs[i]
            .current_path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.tabs[i].tab_title.clone())
    }

    /// Ask "Save changes? [Yes/No]" on the command line; the answer runs
    /// `<command> YES` or `<command> NO`.
    fn ask_save_changes(&mut self, command: &'static str, prompt: String) {
        let flow: crate::command::PromptFlow = Box::new(move |answers| {
            let mut q = crate::command::PromptAnswers::new(answers);
            match q.yes_no(prompt.clone(), true) {
                Ok(save) => crate::command::PromptStep::Run(format!(
                    "{command} {}",
                    if save { "YES" } else { "NO" }
                )),
                Err(step) => step,
            }
        });
        let c = crate::command::ScriptPromptCommand::new(command, flow);
        self.command_line.push_info(&c.prompt());
        self.command_line.set_step_options(c.options());
        let i = self.active_tab;
        self.tabs[i].active_cmd = Some(Box::new(c));
    }

    /// The first drawing EXIT still has to ask about.
    fn first_unsaved_drawing(&self) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.dirty && !tab.is_start && tab.block_test.is_none())
    }

    /// EXIT / QUIT on the command line: asks about each drawing with
    /// unsaved changes in turn, then exits.
    pub(in crate::app) fn exit_from_command_line(&mut self) -> Task<Message> {
        let Some(d) = self.first_unsaved_drawing() else {
            return self.request_quit_with_block_edits();
        };
        let name = self.drawing_file_name(d);
        let prompt = crate::t!("EXIT  Save changes to %{name}? [Yes/No] <Yes>:", name = name);
        self.ask_save_changes("EXIT", prompt.into_owned());
        Task::none()
    }

    /// OPEN with a typed path. Under SDI the current drawing is closed by
    /// the open itself (`OpenPathPicked`).
    fn open_from_command_line(&mut self, path: std::path::PathBuf) -> Task<Message> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !path.is_file() {
                self.command_line
                    .push_error(crate::tf!("OPEN: file not found: {}", path.display()).as_ref());
                return Task::none();
            }
            if let Some(idx) = self.tab_showing(&path) {
                return Task::done(Message::TabSwitch(idx));
            }
            if self.opening.is_some() {
                self.pending_opens.push_back(path);
                return Task::none();
            }
            // As a message: under SDI the open closes this tab, which the
            // OPEN prompt's own command step may still be indexing.
            Task::done(Message::OpenRecent(path))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            Task::done(Message::OpenFile)
        }
    }

    /// SAVEAS answered on the command line: `[format, path]` from the
    /// prompts, or `[format] path` typed on one line.
    fn save_as_from_command_line(&mut self, i: usize, rest: &str) -> Task<Message> {
        let (format, path) = match serde_json::from_str::<Vec<String>>(rest) {
            Ok(answers) if answers.len() == 2 => (answers[0].clone(), answers[1].clone()),
            _ => match rest.split_once(char::is_whitespace) {
                Some((first, path))
                    if first.eq_ignore_ascii_case("DXF")
                        || crate::io::parse_target_version(first).is_ok() =>
                {
                    (first.to_string(), path.trim().to_string())
                }
                _ => (String::new(), rest.to_string()),
            },
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.read_only {
                self.command_line.push_error(
                    crate::t!("Read-only session (--read-only): saving is disabled.").as_ref(),
                );
                return Task::none();
            }
            let mut path = std::path::PathBuf::from(path.trim().trim_matches('"'));
            let dxf = format.eq_ignore_ascii_case("DXF");
            let version = crate::io::parse_target_version(&format)
                .unwrap_or(self.tabs[i].scene.document.version);
            let extension = path
                .extension()
                .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
            if dxf || extension.is_none() {
                let dxf = dxf || crate::io::source_is_dxf(None, &self.tabs[i].scene.document);
                path.set_extension(if dxf { "dxf" } else { "dwg" });
            }
            if self.main_window.is_none() {
                match self.save_tab_synchronously_protected_as(i, path.clone(), version, true) {
                    Ok(()) => {
                        let filename = path.display().to_string();
                        self.command_line
                            .push_output(crate::tf!("Saved: {filename}").as_ref());
                    }
                    Err(error) => self.command_line.push_error(&error.to_string()),
                }
                return Task::none();
            }
            self.prepare_native_save(i);
            self.queue_native_save(
                i,
                path,
                version,
                crate::app::SavePurpose::SaveAs,
                crate::app::SaveContinuation::None,
                true,
                true,
            )
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (i, format, path);
            Task::done(Message::SaveAs)
        }
    }

    /// CLOSE or EXIT answered Yes: save to the drawing's file, then close
    /// it, or go on with the exit (`quit`).
    fn save_then(&mut self, i: usize, quit: bool) -> Task<Message> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.tabs[i]
                .current_path
                .clone()
                .filter(|_| !self.tabs[i].recovery_save_as_required);
            let Some(path) = path else {
                let name = &self.tabs[i].tab_title;
                let error = if quit {
                    crate::tf!("EXIT: \"{}\" has no file path yet; use SAVEAS first.", name)
                } else {
                    crate::tf!("CLOSE: \"{}\" has no file path yet; use SAVEAS first.", name)
                };
                self.command_line.push_error(error.as_ref());
                return Task::none();
            };
            let version = self.tabs[i].scene.document.version;
            if self.main_window.is_none() {
                if let Err(error) =
                    self.save_tab_synchronously_protected_as(i, path, version, false)
                {
                    self.command_line.push_error(&error.to_string());
                    return Task::none();
                }
                self.tabs[i].dirty = false;
                return if quit {
                    self.exit_from_command_line()
                } else {
                    Task::done(Message::TabClose(self.tabs[i].id))
                };
            }
            self.prepare_native_save(i);
            self.queue_native_save(
                i,
                path,
                version,
                crate::app::SavePurpose::Manual,
                if quit {
                    crate::app::SaveContinuation::Quit
                } else {
                    crate::app::SaveContinuation::CloseTab
                },
                false,
                true,
            )
        }
        #[cfg(target_arch = "wasm32")]
        if quit {
            self.request_quit_with_block_edits()
        } else {
            Task::done(Message::TabClose(self.tabs[i].id))
        }
    }
}

/// SAVEAS on the command line: the file format, then the path.
fn saveas_flow(format: String, path: String) -> crate::command::PromptFlow {
    use crate::command::{PromptAnswers, PromptStep};
    const FORMATS: [(&str, &str); 8] = [
        ("R14", "R14"),
        ("2000", "2000"),
        ("2004", "2004"),
        ("2007", "2007"),
        ("2010", "2010"),
        ("2013", "2013"),
        ("2018", "2018"),
        ("DXF", "DXF"),
    ];
    Box::new(move |answers| {
        let mut q = PromptAnswers::new(answers);
        let run = |q: &mut PromptAnswers| -> Result<PromptStep, PromptStep> {
            let format = q.keyword(
                crate::t!(
                    "SAVEAS  Enter file format [R14/2000/2004/2007/2010/2013/2018/DXF] <%{name}>:",
                    name = format
                )
                .into_owned(),
                Some(format.as_str()),
                &FORMATS,
            )?;
            let path = q.text(
                crate::t!("SAVEAS  Save drawing as <%{name}>:", name = path).into_owned(),
                Some(path.clone()),
            )?;
            Ok(PromptStep::Run(format!(
                "SAVEAS {}",
                serde_json::json!([format, path])
            )))
        };
        run(&mut q).unwrap_or_else(|step| step)
    })
}

/// One runnable row of a `.scr` script: trimmed, comment rows (`#`, `;`)
/// dropped, blank rows kept so they submit Enter to the active command.
fn script_command_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.starts_with(';'))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod script_tests {
    use super::script_command_lines;

    /// Comments drop, every other row survives trimmed, and blank or
    /// whitespace-only rows stay as empty strings (Enter presses).
    #[test]
    fn comments_and_blanks_follow_the_scr_rules() {
        let lines =
            script_command_lines("# comment\n; also a comment\nLINE 0,0 10,0\n\n   \n  TRIM  \n");
        assert_eq!(
            lines,
            vec![
                "LINE 0,0 10,0".to_string(),
                String::new(),
                String::new(),
                "TRIM".to_string(),
            ]
        );
    }
}

/// Parse the argument list of the `BACKGROUND` command into an `[r,g,b,a]`
/// colour (channels 0.0–1.0, `a` always 1.0). Accepts:
///   * three whitespace-separated 0–255 values: `255 255 255`
///   * a named preset: WHITE / BLACK / GRAY|GREY / DARKGRAY|DARKGREY / LTGRAY
/// Returns `None` if the arguments don't match either form.
fn parse_background_color(args: &[&str]) -> Option<[f32; 4]> {
    let to_rgba = |[r, g, b]: [u8; 3]| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0];
    // Single token: a named preset or comma-separated "r,g,b".
    if args.len() == 1 {
        let text = args[0].trim();
        let parts: Vec<&str> = text.split(',').collect();
        if parts.len() == 3 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                parts[0].trim().parse::<u8>(),
                parts[1].trim().parse::<u8>(),
                parts[2].trim().parse::<u8>(),
            ) {
                return Some(to_rgba([r, g, b]));
            }
        }
        let preset = match text.to_ascii_uppercase().as_str() {
            "WHITE" => [255, 255, 255],
            "BLACK" => [0, 0, 0],
            "GRAY" | "GREY" => [128, 128, 128],
            "DARKGRAY" | "DARKGREY" | "DKGRAY" => [64, 64, 64],
            "LTGRAY" | "LIGHTGRAY" | "LIGHTGREY" => [192, 192, 192],
            _ => return None,
        };
        return Some(to_rgba(preset));
    }
    // Three separate tokens: `r g b`.
    if args.len() >= 3 {
        let r = args[0].parse::<u8>().ok()?;
        let g = args[1].parse::<u8>().ok()?;
        let b = args[2].parse::<u8>().ok()?;
        return Some(to_rgba([r, g, b]));
    }
    None
}

// ── Web ARCHIVE / ETRANSMIT ──────────────────────────────────────────────────
//
// The browser has no folder to copy into, so the transmittal is assembled in
// memory and downloaded as one ZIP — the same dependency set the native
// folder walk collects (drawing, xrefs, raster images) plus the web-only
// bytes-backed kinds (PDF underlays, session-picked plot tables). Anything
// whose bytes never entered the session is reported as skipped rather than
// silently dropped, the web counterpart of native's missing-file skip.
#[cfg(target_arch = "wasm32")]
impl OpenCADStudio {
    pub(in crate::app) fn archive_web(&mut self, i: usize, cmd: &str) -> Task<Message> {
        let Some(src) = self.tabs[i].current_path.clone() else {
            self.command_line.push_error(
                crate::t!("ARCHIVE: save the drawing first (it has no file path yet).").as_ref(),
            );
            return Task::none();
        };
        let stem = src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "drawing".into());
        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "dwg".into());
        let filename = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{stem}.{ext}"));
        let version = self.tabs[i].scene.document.version;

        // The same save preparation a web Save performs — without it the
        // archived copy ships kernel solids without their ACIS data and a
        // staler view/sysvar header than a real save would produce.
        self.sync_view_state_for_save(i);
        crate::app::update::util::sync_annotation_scale_header(&mut self.tabs[i].scene);
        self.stamp_header_sysvars(i);
        self.sync_solid_models_for_save(i);
        self.tabs[i].scene.sync_native_parametric_graph();

        // The dependency set (xrefs, raster images, PDF underlays, plot
        // tables) is collected here, at command time — cheap in-memory
        // registry reads — so the transmittal matches the drawing snapshot
        // serialized below even if the tab changes while the writer runs.
        let mut deps: Vec<(String, Vec<u8>)> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();

        // Xrefs — the block-record walk native copies; the bytes come from
        // the session's xref registry (picked, dropped or opened files).
        for br in self.tabs[i].scene.document.block_records.iter() {
            if br.xref_path.trim().is_empty() {
                continue;
            }
            match crate::io::xref::find_registered_source(&br.xref_path)
                .and_then(|key| crate::io::xref::registered_bytes(&key))
            {
                Some(bytes) => push_entry(
                    &mut deps,
                    transmit_file_name(&br.xref_path),
                    (*bytes).clone(),
                ),
                None => push_skipped(&mut skipped, &transmit_file_name(&br.xref_path)),
            }
        }

        // Raster images — picked/dropped bytes registered for the reference
        // stand in for the file native would copy.
        for e in self.tabs[i].scene.document.entities() {
            if let codec::EntityType::RasterImage(img) = e {
                if img.file_path.trim().is_empty() {
                    continue;
                }
                match crate::scene::model::image_model::registered_source_bytes(&img.file_path) {
                    Some(bytes) => push_entry(
                        &mut deps,
                        transmit_file_name(&img.file_path),
                        (*bytes).clone(),
                    ),
                    None => push_skipped(&mut skipped, &transmit_file_name(&img.file_path)),
                }
            }
        }

        // PDF underlays — bytes registered at attach time.
        for o in self.tabs[i].scene.document.objects.values() {
            if let codec::objects::ObjectType::UnderlayDefinition(def) = o {
                if !matches!(def.underlay_type, codec::entities::UnderlayType::Pdf)
                    || def.file_path.trim().is_empty()
                {
                    continue;
                }
                match crate::scene::model::pdf_raster::registered_source_bytes(&def.file_path) {
                    Some(bytes) => push_entry(
                        &mut deps,
                        transmit_file_name(&def.file_path),
                        (*bytes).clone(),
                    ),
                    None => push_skipped(&mut skipped, &transmit_file_name(&def.file_path)),
                }
            }
        }

        // Plot tables named by the layouts (and the session's active one).
        // Only tables picked this session have bytes; the built-ins ship
        // with the app, so they are neither packaged nor reported missing.
        let mut style_names: Vec<String> = self.tabs[i]
            .scene
            .document
            .objects
            .values()
            .filter_map(|o| {
                if let codec::objects::ObjectType::Layout(l) = o {
                    Some(l.plot_style_sheet.clone())
                } else {
                    None
                }
            })
            .collect();
        if let Some(table) = self.active_plot_style.as_ref() {
            style_names.push(table.name.clone());
        }
        for name in style_names {
            if name.trim().is_empty()
                || crate::io::plot_style::PlotStyleTable::builtin(&name).is_ok()
            {
                continue;
            }
            match crate::io::plot_style::session_table(&name) {
                Some(bytes) => push_entry(&mut deps, name, (*bytes).clone()),
                None => push_skipped(&mut skipped, &name),
            }
        }

        // The drawing itself: the same worker-routed serialization a web
        // Save performs (native copies the last-saved file; the web build
        // has none, so the current document is re-serialized). The ZIP
        // assembly and download are cheap and stay on the main thread —
        // they continue in the completion message.
        let zip_name = format!("{stem}_transmit.zip");
        let prepared =
            crate::io::prepare_document_for_save(self.tabs[i].scene.document_for_save(), version);
        let cmd = cmd.to_string();
        Task::perform(
            async move { crate::io::serialize_prepared_document(&prepared, &ext, false).await },
            move |result| Message::WebArchiveSerialized {
                cmd,
                zip_name,
                filename,
                deps,
                skipped,
                result,
            },
        )
    }

    /// Web ARCHIVE completion: the serializer worker (or the inline
    /// fallback) produced the drawing bytes. Assemble the transmittal ZIP —
    /// the drawing entry first, then the dependencies under the same
    /// name-collision rule the synchronous pass used — download it and
    /// report, including the skipped-dependency lines.
    pub(in crate::app) fn on_web_archive_serialized(
        &mut self,
        cmd: String,
        zip_name: String,
        filename: String,
        deps: Vec<(String, Vec<u8>)>,
        skipped: Vec<String>,
        result: Result<Vec<u8>, String>,
    ) -> Task<Message> {
        let drawing = match result {
            Ok(bytes) => bytes,
            Err(e) => {
                self.command_line
                    .push_error(crate::tf!("{cmd}: cannot package the drawing ({e}).").as_ref());
                return Task::none();
            }
        };
        let mut entries: Vec<(String, Vec<u8>)> = vec![(filename, drawing)];
        for (name, data) in deps {
            push_entry(&mut entries, name, data);
        }
        match build_zip(&entries) {
            Ok(zip) => {
                crate::sys::download_bytes(&zip_name, &zip);
                // The same report shape the native folder transmittal makes.
                let copied = entries.len();
                self.command_line.push_output(
                    crate::tf!("{cmd}: packaged {copied} file(s) into {}", zip_name).as_ref(),
                );
            }
            Err(e) => self
                .command_line
                .push_error(crate::tf!("{cmd}: cannot build the transmittal ZIP ({e}).").as_ref()),
        }
        if !skipped.is_empty() {
            let n = skipped.len();
            let names = skipped.join(", ");
            self.command_line.push_output(
                crate::tf!(
                    "{cmd}: skipped {n} referenced file(s) not loaded in this session: {names}"
                )
                .as_ref(),
            );
        }
        Task::none()
    }
}

/// Trailing component of a reference path — the flat name a transmittal
/// packages it under (native copies `file_name()` into one folder).
#[cfg(target_arch = "wasm32")]
fn transmit_file_name(path: &str) -> String {
    path.trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Add one entry under its flat name; a later dependency with the same name
/// replaces the bytes (native's `fs::copy` overwrite into the folder).
#[cfg(target_arch = "wasm32")]
fn push_entry(entries: &mut Vec<(String, Vec<u8>)>, name: String, data: Vec<u8>) {
    if let Some(slot) = entries
        .iter_mut()
        .find(|(entry, _)| entry.eq_ignore_ascii_case(&name))
    {
        slot.1 = data;
    } else {
        entries.push((name, data));
    }
}

/// Pack the transmittal entries into one deflated ZIP archive in memory.
#[cfg(target_arch = "wasm32")]
fn build_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        writer
            .start_file(name.as_str(), options)
            .map_err(|e| e.to_string())?;
        writer.write_all(data).map_err(|e| e.to_string())?;
    }
    writer
        .finish()
        .map(std::io::Cursor::into_inner)
        .map_err(|e| e.to_string())
}

/// Remember a missing dependency for the report, once per name.
#[cfg(target_arch = "wasm32")]
fn push_skipped(skipped: &mut Vec<String>, name: &str) {
    if !name.is_empty() && !skipped.iter().any(|s| s.eq_ignore_ascii_case(name)) {
        skipped.push(name.to_string());
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};

    fn fresh_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// A batch script walks a drawing: OPEN by path (the next rows wait for
    /// the load), SAVEAS a version, CLOSE answering its save question.
    #[test]
    fn batch_script_opens_saves_and_closes() {
        let dir = std::env::temp_dir().join(format!("ocs-batch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a 1.dxf");
        let doc = codec::Document::new();
        std::fs::write(&a, crate::io::save_to_bytes(&doc, "dxf", doc.version).unwrap()).unwrap();
        let out = dir.join("out file.dwg");
        let _ = std::fs::remove_file(&out);
        let mut app = OpenCADStudio::new_for_test();
        app.script_queue.extend([
            format!("OPEN \"{}\"", a.display()),
            "CIRCLE 0,0 5".to_string(),
            format!("SAVEAS 2013 \"{}\"", out.display()),
            "CIRCLE 1,1 2".to_string(),
            "CLOSE".to_string(),
            "Y".to_string(),
        ]);
        let t = app.update(Message::ScriptPump);
        app.drive_headless_task(t).unwrap();
        assert!(out.is_file());
        assert!(app.tabs.iter().all(|t| t.current_path.as_deref() != Some(out.as_path())));
        let back = crate::io::load_file(&out).unwrap();
        assert_eq!(back.entities().count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A headless batch run ends at EXIT: the Yes answer saves the changed
    /// drawing, and the rows after EXIT never run.
    #[test]
    fn batch_exit_saves_and_stops() {
        let dir = std::env::temp_dir().join(format!("ocs-batch-exit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.dxf");
        let doc = codec::Document::new();
        std::fs::write(&a, crate::io::save_to_bytes(&doc, "dxf", doc.version).unwrap()).unwrap();
        let mut app = OpenCADStudio::new_for_test();
        let text = format!("OPEN \"{}\"\nCIRCLE 0,0 5\nEXIT\nY\nCIRCLE 1,1 2\n", a.display());
        assert_eq!(app.run_batch(&text, &[]), 0);
        assert!(app.exit_requested);
        assert_eq!(crate::io::load_file(&a).unwrap().entities().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiline_script_draws_line() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let before_count = app.tabs[i].scene.document.entities().count();

        // Feed standard multi-line CAD script sequence
        let _ = app.update(Message::ScriptLine("LINE".into()));
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "LINE command should be active"
        );

        let _ = app.update(Message::ScriptLine("0,0".into()));
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "LINE command should stay active after first point"
        );

        let _ = app.update(Message::ScriptLine("10,10".into()));
        let _ = app.update(Message::ScriptLine("".into())); // Enter to finish

        let after_count = app.tabs[i].scene.document.entities().count();
        assert_eq!(
            after_count,
            before_count + 1,
            "Line entity should have been created"
        );
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "LINE command should be finished"
        );
    }

    #[test]
    fn single_line_script_still_works() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let before_count = app.tabs[i].scene.document.entities().count();

        let _ = app.update(Message::ScriptLine("CIRCLE 5,5 2".into()));

        let after_count = app.tabs[i].scene.document.entities().count();
        assert_eq!(
            after_count,
            before_count + 1,
            "Circle entity should have been created"
        );
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "CIRCLE command should be finished"
        );
    }

    #[test]
    fn command_name_prefixes_are_stripped() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let before_count = app.tabs[i].scene.document.entities().count();

        let _ = app.update(Message::ScriptLine("_CIRCLE 10,10 3".into()));

        let after_count = app.tabs[i].scene.document.entities().count();
        assert_eq!(
            after_count,
            before_count + 1,
            "_CIRCLE should resolve to CIRCLE and create entity"
        );
    }

    #[test]
    fn test_script_scr_draws_all_entities() {
        let mut app = fresh_app();
        let i = app.active_tab;
        const SCRIPT: &str = r#"
_LINE
0,0
100,0
100,100
0,100
C

_LINE
0,0
100,100

_LINE
0,100
100,0

CIRCLE
50,50
35

CIRCLE 50,50 15

ZOOM EXTENTS
"#;
        for line in SCRIPT
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#') && !l.starts_with(';'))
        {
            let _ = app.update(Message::ScriptLine(line.to_string()));
        }
        // 4 square edges + 2 diagonal lines + 2 circles = 8 entities
        assert_eq!(app.tabs[i].scene.document.entities().count(), 8);
    }

    #[test]
    fn blank_line_repeats_last_command_when_idle() {
        let mut app = fresh_app();
        let i = app.active_tab;

        // Blank line before any command does nothing and does not panic
        let _ = app.update(Message::ScriptLine("".into()));
        assert!(app.tabs[i].active_cmd.is_none());

        // 1. Draw a line
        let _ = app.update(Message::ScriptLine("LINE".into()));
        let _ = app.update(Message::ScriptLine("0,0".into()));
        let _ = app.update(Message::ScriptLine("10,10".into()));
        let _ = app.update(Message::ScriptLine("".into())); // finishes LINE
        assert_eq!(app.tabs[i].scene.document.entities().count(), 1);
        assert!(app.tabs[i].active_cmd.is_none());

        // 2. Blank line when idle repeats LINE
        let _ = app.update(Message::ScriptLine("".into()));
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "Blank line should repeat LINE"
        );
        let _ = app.update(Message::ScriptLine("20,20".into()));
        let _ = app.update(Message::ScriptLine("30,30".into()));
        let _ = app.update(Message::ScriptLine("".into())); // finishes second LINE
        assert_eq!(app.tabs[i].scene.document.entities().count(), 2);
        assert!(app.tabs[i].active_cmd.is_none());

        // 3. Single-line command repeat
        let _ = app.update(Message::ScriptLine("CIRCLE 5,5 2".into()));
        assert_eq!(app.tabs[i].scene.document.entities().count(), 3);
        assert!(app.tabs[i].active_cmd.is_none());

        // Blank line repeats CIRCLE
        let _ = app.update(Message::ScriptLine("".into()));
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "Blank line should repeat CIRCLE"
        );
        let _ = app.update(Message::ScriptLine("15,15".into()));
        let _ = app.update(Message::ScriptLine("3".into()));
        assert_eq!(app.tabs[i].scene.document.entities().count(), 4);
        assert!(app.tabs[i].active_cmd.is_none());
    }
}
