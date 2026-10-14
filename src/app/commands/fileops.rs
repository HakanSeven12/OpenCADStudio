use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_fileops(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // QNEW starts from the default template (Options > Files); NEW
            // asks which template to start from (#1635).
            "QNEW" => return Some(Task::done(Message::TabNew)),
            "NEW" => {
                return Some(Task::perform(
                    async {
                        crate::sys::file_dialog()
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
            "OPEN" => return Some(Task::done(Message::OpenFile)),
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
            "SAVEAS" => return Some(Task::done(Message::SaveAs)),
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
                );
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
                                let mut deps: Vec<String> = Vec::new();
                                for br in self.tabs[i].scene.document.block_records.iter() {
                                    if !br.xref_path.trim().is_empty() {
                                        deps.push(br.xref_path.clone());
                                    }
                                }
                                for e in self.tabs[i].scene.document.entities() {
                                    if let codec::EntityType::RasterImage(img) = e {
                                        if !img.file_path.trim().is_empty() {
                                            deps.push(img.file_path.clone());
                                        }
                                    }
                                }
                                for d in deps {
                                    let dp = PathBuf::from(&d);
                                    let resolved = if dp.is_absolute() {
                                        dp
                                    } else {
                                        parent.join(&dp)
                                    };
                                    if resolved.exists() {
                                        if let Some(fname) = resolved.file_name() {
                                            if std::fs::copy(&resolved, folder.join(fname)).is_ok()
                                            {
                                                copied += 1;
                                            }
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
        let cmds: Vec<Task<Message>> = script_command_lines(text)
            .into_iter()
            .map(|line| Task::done(Message::ScriptLine(line)))
            .collect();
        self.command_line.push_output(
            crate::tf!("SCRIPT: running {} command(s) from {p}.", cmds.len()).as_ref(),
        );
        Task::batch(cmds)
    }
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
            async move { crate::io::serialize_prepared_document(&prepared, &ext).await },
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
