use super::*;

impl OpenCADStudio {
    pub(crate) fn dispatch_view(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            "DONATE" => {
                self.command_line.push_info(crate::t!("Opening Patreon page...").as_ref());
                return Some(crate::sys::open_url(
                    "https://patreon.com/HakanSeven12",
                    self.main_window,
                ));
            }

            "WEBVERSION" => {
                self.command_line.push_info(crate::t!("Opening OCS Web...").as_ref());
                return Some(crate::sys::open_url(
                    "https://hakanseven12.github.io/OpenCADStudio/",
                    self.main_window,
                ));
            }

            "HELP" => {
                self.command_line
                    .push_info(crate::t!("Opening OCS Discussions for help and questions...").as_ref());
                return Some(crate::sys::open_url(
                    "https://github.com/HakanSeven12/OpenCADStudio/discussions",
                    self.main_window,
                ));
            }

            // ── DWGPROPS — print round-trip-only HeaderVariables ─────────
            // No UI dialog for these yet; the command surfaces them so
            // users can confirm the values that the parser populated and
            // the writer will round-trip on save.
            "DWGPROPS" | "DWGPROP" => {
                let i = self.active_tab;
                let h = &self.tabs[i].scene.document.header;
                let path_label = self.tabs[i]
                    .current_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(unsaved)".to_string());
                self.command_line
                    .push_output(crate::tf!("Drawing: {}", path_label).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Created (Julian):  {:.6}", h.create_date_julian).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Updated (Julian):  {:.6}", h.update_date_julian).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Total edit time:   {:.4}", h.total_editing_time).as_ref());
                self.command_line
                    .push_output(crate::tf!("  User elapsed:      {:.4}", h.user_elapsed_time).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Last saved by:     {}",
                    if h.last_saved_by.is_empty() {
                        "(unknown)"
                    } else {
                        &h.last_saved_by
                    }
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Fingerprint GUID:  {}",
                    if h.fingerprint_guid.is_empty() {
                        "(none)"
                    } else {
                        &h.fingerprint_guid
                    }
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Version GUID:      {}",
                    if h.version_guid.is_empty() {
                        "(none)"
                    } else {
                        &h.version_guid
                    }
                ).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Code page:         {}", h.code_page).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Menu name:         {}",
                    if h.menu_name.is_empty() {
                        "(none)"
                    } else {
                        &h.menu_name
                    }
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Hyperlink base:    {}",
                    if h.hyperlink_base.is_empty() {
                        "(none)"
                    } else {
                        &h.hyperlink_base
                    }
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Project name:      {}",
                    if h.project_name.is_empty() {
                        "(none)"
                    } else {
                        &h.project_name
                    }
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Stylesheet:        {}",
                    if h.stylesheet.is_empty() {
                        "(none)"
                    } else {
                        &h.stylesheet
                    }
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Required versions: {:#018x}",
                    h.required_versions
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  Measurement:       {} ({})",
                    h.measurement,
                    if h.measurement == 1 {
                        "Metric"
                    } else {
                        "Imperial"
                    }
                ).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Proxy graphics:    {}", h.proxy_graphics).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Tree depth:        {}", h.tree_depth).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  User vars (int):   {} {} {} {} {}",
                    h.user_int1, h.user_int2, h.user_int3, h.user_int4, h.user_int5
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  User vars (real):  {:.6} {:.6} {:.6} {:.6} {:.6}",
                    h.user_real1, h.user_real2, h.user_real3, h.user_real4, h.user_real5
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "  User timer:        {}",
                    if h.user_timer { "On" } else { "Off" }
                ).as_ref());
            }

            // Edit a USERI1..USERI5 / USERR1..USERR5 slot. Lets the user
            // store drawing-scoped scalars (and save them through round-trip)
            // even though we don't have a LISP / DIESEL runtime yet.
            //   USERI 1 42        → header.user_int1 = 42
            //   USERR 3 1.5e-3    → header.user_real3 = 0.0015
            "USERI" | "USERR" => {
                use crate::command::UserRegCommand;
                let name = if cmd == "USERR" { "USERR" } else { "USERI" };
                let c = UserRegCommand::new(name);
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("USERI ") || cmd.starts_with("USERR ") => {
                let is_real = cmd.starts_with("USERR");
                let rest = if is_real {
                    cmd.trim_start_matches("USERR").trim()
                } else {
                    cmd.trim_start_matches("USERI").trim()
                };
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                let slot: Option<usize> = parts.first().and_then(|s| s.parse().ok());
                let value = parts.get(1).copied().unwrap_or("").trim();
                let i = self.active_tab;
                let h = &mut self.tabs[i].scene.document.header;
                match (slot, value, is_real) {
                    (Some(n @ 1..=5), v, true) => {
                        if let Ok(val) = v.parse::<f64>() {
                            match n {
                                1 => h.user_real1 = val,
                                2 => h.user_real2 = val,
                                3 => h.user_real3 = val,
                                4 => h.user_real4 = val,
                                _ => h.user_real5 = val,
                            }
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(crate::tf!("USERR{n} = {val}").as_ref());
                        } else {
                            self.command_line.push_info(crate::t!("Usage: USERR <1-5> <real>").as_ref());
                        }
                    }
                    (Some(n @ 1..=5), v, false) => {
                        if let Ok(val) = v.parse::<i16>() {
                            match n {
                                1 => h.user_int1 = val,
                                2 => h.user_int2 = val,
                                3 => h.user_int3 = val,
                                4 => h.user_int4 = val,
                                _ => h.user_int5 = val,
                            }
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(crate::tf!("USERI{n} = {val}").as_ref());
                        } else {
                            self.command_line.push_info(crate::t!("Usage: USERI <1-5> <integer>").as_ref());
                        }
                    }
                    _ => self
                        .command_line
                        .push_info(crate::t!("Usage: USERI <1-5> <int> | USERR <1-5> <real>").as_ref()),
                }
            }

            "REPORT" => {
                // Pre-fill the GitHub issue body with version + platform so
                // reports arrive with the basics already filled in.
                let body = format!(
                    "<!-- Describe the issue and the steps to reproduce it. -->\n\n\n\
                     ---\n- Open CAD Studio: v{}\n- Platform: {}\n",
                    env!("CARGO_PKG_VERSION"),
                    crate::sys::platform_info(),
                );
                let url = format!(
                    "https://github.com/HakanSeven12/OpenCADStudio/issues/new?body={}",
                    crate::sys::percent_encode(&body)
                );
                self.command_line.push_info(crate::t!("Opening feedback page...").as_ref());
                return Some(crate::sys::open_url(&url, self.main_window));
            }

            "ABOUT" => {
                return Some(Task::done(Message::AboutOpen));
            }

            "PLUGINS" | "PLUGINMANAGER" => {
                return Some(Task::done(Message::PluginManagerOpen));
            }

            "CHANGELOG" => {
                self.command_line.push_info(crate::t!("Opening release notes...").as_ref());
                return Some(crate::sys::open_url(
                    "https://github.com/HakanSeven12/OpenCADStudio/releases",
                    self.main_window,
                ));
            }

            // ── ALIASEDIT — command-alias editor ───────────────────────────
            // Opens the command-alias table editor (ocad.pgp). Keyboard
            // key-bindings are edited separately via CUI (below).
            "ALIASEDIT" => {
                return Some(Task::done(Message::AliasEditorOpen));
            }

            // ── CUI — keyboard shortcut / key-binding editor ───────────────
            "CUI" => {
                return Some(Task::done(Message::ShortcutsPanelOpen));
            }

            // CUIEXPORT <path> — write the keyboard-shortcut customizations
            // (the drawing-independent CUI data) to a plain "KEY COMMAND" file.
            "CUIEXPORT" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("CUIEXPORT", "CUIEXPORT  file to save shortcuts to:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("CUIEXPORT ") => {
                let path = cmd.trim_start_matches("CUIEXPORT").trim();
                if path.is_empty() {
                    self.command_line.push_info(
                        "Usage: CUIEXPORT <path> — save the keyboard shortcuts to a file.",
                    );
                    return Some(Task::none());
                }
                let mut keys: Vec<(&String, &String)> = self.shortcut_bindings.iter().collect();
                keys.sort_by(|a, b| a.0.cmp(b.0));
                let text: String = keys.iter().map(|(k, v)| format!("{k} {v}\n")).collect();
                let count = self.shortcut_bindings.len();
                match std::fs::write(path, text) {
                    Ok(()) => self.command_line.push_output(crate::tf!(
                        "CUIEXPORT: wrote {count} shortcut(s) to \"{path}\"."
                    ).as_ref()),
                    Err(e) => self
                        .command_line
                        .push_error(crate::tf!("CUIEXPORT: cannot write \"{path}\": {e}").as_ref()),
                }
            }

            // CUIIMPORT / CUILOAD <path> — load shortcut customizations from a
            // "KEY COMMAND" file (lines starting with # are ignored).
            "CUIIMPORT" | "CUILOAD" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("CUIIMPORT", "CUIIMPORT  shortcuts file to load:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("CUIIMPORT ") || cmd.starts_with("CUILOAD ") => {
                let path = cmd
                    .trim_start_matches("CUIIMPORT")
                    .trim_start_matches("CUILOAD")
                    .trim();
                if path.is_empty() {
                    self.command_line.push_info(
                        "Usage: CUIIMPORT <path> — load keyboard shortcuts from a file.",
                    );
                    return Some(Task::none());
                }
                match std::fs::read_to_string(path) {
                    Ok(text) => {
                        let mut n = 0usize;
                        for line in text.lines() {
                            let line = line.trim();
                            if line.is_empty() || line.starts_with('#') {
                                continue;
                            }
                            if let Some((k, v)) = line.split_once(char::is_whitespace) {
                                let key = crate::app::shortcuts::normalize_key(k);
                                if !key.is_empty() {
                                    self.shortcut_bindings
                                        .insert(key, v.trim().to_uppercase());
                                    n += 1;
                                }
                            }
                        }
                        self.persist_settings_if_changed();
                        self.command_line.push_output(crate::tf!(
                            "CUIIMPORT: loaded {n} shortcut(s) from \"{path}\"."
                        ).as_ref());
                    }
                    Err(e) => self
                        .command_line
                        .push_error(crate::tf!("CUIIMPORT: cannot read \"{path}\": {e}").as_ref()),
                }
            }

            // ── Keyboard Shortcuts panel ──────────────────────────────────
            cmd if cmd == "SHORTCUTS" || cmd.starts_with("SHORTCUTS ") => {
                let raw_rest = cmd.trim_start_matches("SHORTCUTS").trim();
                let parts: Vec<&str> = raw_rest.splitn(3, ' ').collect();
                let sub = parts.first().map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    "" | "LIST" | "?" => {
                        return Some(Task::done(Message::ShortcutsPanelOpen));
                    }
                    "SET" | "S" => {
                        // SHORTCUTS SET <key> <command>
                        // e.g. SHORTCUTS SET CTRL+D DIST
                        let key = parts.get(1).map(|s| s.to_uppercase()).unwrap_or_default();
                        let cmd_str = parts.get(2).map(|s| s.to_uppercase()).unwrap_or_default();
                        if key.is_empty() || cmd_str.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: SHORTCUTS SET <key> <command>  e.g. SHORTCUTS SET CTRL+D DIST").as_ref());
                        } else {
                            let key = crate::app::shortcuts::normalize_key(&key);
                            if key.is_empty() {
                                self.command_line.push_error(crate::t!("Usage: SHORTCUTS SET <key> <command>  e.g. SHORTCUTS SET CTRL+D DIST").as_ref());
                            } else {
                                self.shortcut_bindings.insert(key.clone(), cmd_str.clone());
                                self.persist_settings_if_changed();
                                self.command_line
                                    .push_output(crate::tf!("Shortcut set: {key} → {cmd_str}").as_ref());
                            }
                        }
                    }
                    "CLEAR" | "DELETE" | "REMOVE" => {
                        let key = parts.get(1).map(|s| s.to_uppercase()).unwrap_or_default();
                        if key.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: SHORTCUTS CLEAR <key>").as_ref());
                        } else if self
                            .shortcut_bindings
                            .remove(&crate::app::shortcuts::normalize_key(&key))
                            .is_some()
                        {
                            self.persist_settings_if_changed();
                            self.command_line
                                .push_output(crate::tf!("Shortcut '{key}' removed.").as_ref());
                        } else {
                            self.command_line
                                .push_error(crate::tf!("Shortcut '{key}' not found.").as_ref());
                        }
                    }
                    _ => {
                        self.command_line
                            .push_info(crate::t!("Usage: SHORTCUTS LIST | SET <key> <cmd> | CLEAR <key>").as_ref());
                    }
                }
            }

            // ── Color Scheme / Theme selector ─────────────────────────────
            cmd if cmd == "COLORSCHEME" || cmd.starts_with("COLORSCHEME ") => {
                use iced::Theme;
                let sub = cmd
                    .split_once(' ')
                    .map(|(_, r)| r.trim())
                    .unwrap_or("")
                    .to_uppercase();
                // Map name to Theme variant.
                let theme: Option<Theme> = match sub.as_str() {
                    "DARK" => Some(Theme::Dark),
                    "LIGHT" => Some(Theme::Light),
                    "DRACULA" => Some(Theme::Dracula),
                    "NORD" => Some(Theme::Nord),
                    "SOLARIZED_LIGHT" | "SOLARIZEDLIGHT" => Some(Theme::SolarizedLight),
                    "SOLARIZED_DARK" | "SOLARIZEDDARK" => Some(Theme::SolarizedDark),
                    "GRUVBOX_LIGHT" | "GRUVBOXLIGHT" => Some(Theme::GruvboxLight),
                    "GRUVBOX_DARK" | "GRUVBOXDARK" => Some(Theme::GruvboxDark),
                    "TOKYONIGHT" | "TOKYO_NIGHT" => Some(Theme::TokyoNight),
                    "TOKYONIGHTSTORM" | "TOKYO_NIGHT_STORM" => Some(Theme::TokyoNightStorm),
                    "TOKYONIGHTLIGHT" | "TOKYO_NIGHT_LIGHT" => Some(Theme::TokyoNightLight),
                    "KANAGAWAWAVE" | "KANAGAWA_WAVE" => Some(Theme::KanagawaWave),
                    "KANAGAWADRAGON" | "KANAGAWA_DRAGON" => Some(Theme::KanagawaDragon),
                    "KANAGAWALOTUS" | "KANAGAWA_LOTUS" => Some(Theme::KanagawaLotus),
                    "MOONFLY" => Some(Theme::Moonfly),
                    "NIGHTFLY" => Some(Theme::Nightfly),
                    "OXOCARBON" => Some(Theme::Oxocarbon),
                    "FERRA" => Some(Theme::Ferra),
                    "" | "LIST" | "?" => {
                        self.command_line.push_output(
                            "Available themes: DARK LIGHT DRACULA NORD SOLARIZED_LIGHT SOLARIZED_DARK \
                             GRUVBOX_LIGHT GRUVBOX_DARK TOKYONIGHT TOKYONIGHTSTORM TOKYONIGHTLIGHT \
                             KANAGAWAWAVE KANAGAWADRAGON KANAGAWALOTUS MOONFLY NIGHTFLY OXOCARBON FERRA"
                        );
                        return Some(Task::none());
                    }
                    _ => {
                        self.command_line.push_error(crate::tf!(
                            "COLORSCHEME: unknown theme '{}'. Type COLORSCHEME LIST for options.",
                            sub
                        ).as_ref());
                        return Some(Task::none());
                    }
                };
                if let Some(t) = theme {
                    let name = format!("{:?}", t);
                    self.command_line
                        .push_output(crate::tf!("Color scheme set to '{name}'.").as_ref());
                    return Some(Task::done(Message::SetTheme(t)));
                }
                return Some(Task::none());
            }

            // ── Layout Manager GUI ─────────────────────────────────────────
            "LAYOUTMANAGER" | "LAYOUTPANEL" => {
                return Some(Task::done(Message::LayoutManagerOpen));
            }

            // ── Layout / viewport ──────────────────────────────────────────
            "MVIEW" => {
                if self.tabs[i].scene.current_layout == "Model" {
                    self.command_line
                        .push_error(crate::t!("MVIEW: switch to a paper space layout first.").as_ref());
                } else {
                    use crate::modules::layout::mview::MviewCommand;
                    let scene = &self.tabs[i].scene;
                    let layout = scene.current_layout.clone();
                    let paper_bounds = scene
                        .printable_area_limits()
                        .or_else(|| scene.paper_limits())
                        .unwrap_or(((0.0, 0.0), (297.0, 210.0)));
                    let views = scene.document.views.iter().cloned().collect();
                    let new_cmd = MviewCommand::new(layout, paper_bounds, views);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            // ── MSPACE / PSPACE ───────────────────────────────────────────
            "MSPACE" => {
                return Some(Task::done(Message::MspaceCommand));
            }
            "PSPACE" => {
                return Some(Task::done(Message::PspaceCommand));
            }

            // ── Viewport arrangement shortcuts ────────────────────────────
            // Tile the model viewports into preset splits. Each delegates to the
            // matching VPORTS configuration so the Model/paper handling stays in
            // one place.
            "HORIZONTAL" => return self.dispatch_view("VPORTS 2H", i),
            "VERTICAL" => return self.dispatch_view("VPORTS 2V", i),
            "VPJOIN" => return self.dispatch_view("VPORTS SINGLE", i),
            "CASCADE" => return self.dispatch_view("VPORTS 4", i),

            // ── VPORTS — list or create preset viewport configurations ────
            cmd if cmd == "VPORTS" || cmd.starts_with("VPORTS ") => {
                let sub = cmd.split_whitespace().nth(1).unwrap_or("").to_uppercase();
                let scene = &self.tabs[i].scene;
                if scene.current_layout == "Model" {
                    // Bare VPORTS → ask for the configuration interactively;
                    // the next command-line entry supplies it.
                    if sub.is_empty() {
                        self.awaiting_vports = true;
                        self.command_line
                            .push_info(crate::t!("VPORTS  Configuration [SIngle/2H/2V/4]:").as_ref());
                        return Some(self.focus_cmd_input());
                    }
                    // Model space: split the tiled viewport layout via pane_grid.
                    use iced::widget::pane_grid::{Axis, Configuration as C};
                    let split = |axis, a, b| C::Split {
                        axis,
                        ratio: 0.5,
                        a: Box::new(a),
                        b: Box::new(b),
                    };
                    let config: Option<(C<usize>, usize)> = match sub.as_str() {
                        "SINGLE" | "SI" | "1" => Some((C::Pane(0), 1)),
                        "2H" | "2" => Some((split(Axis::Horizontal, C::Pane(0), C::Pane(1)), 2)),
                        "2V" => Some((split(Axis::Vertical, C::Pane(0), C::Pane(1)), 2)),
                        "4" => Some((
                            split(
                                Axis::Vertical,
                                split(Axis::Horizontal, C::Pane(0), C::Pane(2)),
                                split(Axis::Horizontal, C::Pane(1), C::Pane(3)),
                            ),
                            4,
                        )),
                        _ => None,
                    };
                    match config {
                        Some((config, n)) => {
                            self.tabs[i].scene.set_model_panes(config);
                            self.tabs[i].scene.camera_generation += 1;
                            self.command_line
                                .push_output(crate::tf!("VPORTS: {n} viewport(s).").as_ref());
                        }
                        None => {
                            self.command_line
                                .push_error(crate::t!("VPORTS: use SINGLE | 2H | 2V | 4.").as_ref());
                        }
                    }
                } else if sub.is_empty() {
                    // ── List existing viewports ──────────────────────────
                    let layout_block = scene.current_layout_block_handle_pub();
                    let viewports: Vec<_> = scene
                        .document
                        .entities()
                        .filter_map(|e| {
                            if let acadrust::EntityType::Viewport(vp) = e {
                                if vp.id > 1 && vp.common.owner_handle == layout_block {
                                    Some((
                                        vp.id,
                                        vp.center.clone(),
                                        vp.width,
                                        vp.height,
                                        crate::scene::vp_effective_scale(
                                            vp.custom_scale,
                                            vp.view_height,
                                            vp.height,
                                        ),
                                        vp.status.is_on,
                                        vp.status.locked,
                                    ))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .collect();
                    if viewports.is_empty() {
                        self.command_line.push_info(crate::t!("No viewports. Use MVIEW to create one, or VPORTS 2H / 2V / 4 / SINGLE.").as_ref());
                    } else {
                        self.command_line.push_output(crate::tf!(
                            "{} viewport(s) in layout \"{}\":",
                            viewports.len(),
                            scene.current_layout
                        ).as_ref());
                        for (id, center, w, h, scale, is_on, locked) in &viewports {
                            let state = match (is_on, locked) {
                                (true, true) => "On, Locked",
                                (true, false) => "On",
                                (false, _) => "Off",
                            };
                            self.command_line.push_output(crate::tf!(
                                "  VP #{id}: {w:.1}×{h:.1} @ ({:.1},{:.1})  scale={scale:.4}  [{state}]",
                                center.x, center.y
                            ).as_ref());
                        }
                    }
                } else {
                    // ── Preset viewport layout ───────────────────────────
                    // Determine paper dimensions from PlotSettings (fallback A4 landscape).
                    let layout_name = scene.current_layout.clone();
                    let (paper_w, paper_h) = {
                        use acadrust::objects::ObjectType;
                        let mut pw = 297.0_f64;
                        let mut ph = 210.0_f64;
                        for (_, obj) in &scene.document.objects {
                            if let ObjectType::PlotSettings(ps) = obj {
                                if ps.page_name == layout_name && ps.paper_width > 0.0 {
                                    pw = ps.paper_width;
                                    ph = ps.paper_height;
                                    break;
                                }
                            }
                        }
                        (pw, ph)
                    };
                    let margin = 5.0_f64; // mm margin around the usable area
                    let uw = paper_w - 2.0 * margin; // usable width
                    let uh = paper_h - 2.0 * margin; // usable height
                                                     // Collect rectangle specs: (cx, cz, w, h) in mm
                    let rects: Vec<(f64, f64, f64, f64)> = match sub.as_str() {
                        "2H" => {
                            // Two viewports side by side (horizontal split)
                            let vw = (uw - 2.0) / 2.0;
                            vec![
                                (margin + vw / 2.0, margin + uh / 2.0, vw, uh),
                                (margin + vw + 2.0 + vw / 2.0, margin + uh / 2.0, vw, uh),
                            ]
                        }
                        "2V" => {
                            // Two viewports stacked (vertical split)
                            let vh = (uh - 2.0) / 2.0;
                            vec![
                                (margin + uw / 2.0, margin + vh + 2.0 + vh / 2.0, uw, vh),
                                (margin + uw / 2.0, margin + vh / 2.0, uw, vh),
                            ]
                        }
                        "4" => {
                            // Four equal viewports (2×2 grid)
                            let vw = (uw - 2.0) / 2.0;
                            let vh = (uh - 2.0) / 2.0;
                            vec![
                                (margin + vw / 2.0, margin + vh + 2.0 + vh / 2.0, vw, vh),
                                (
                                    margin + vw + 2.0 + vw / 2.0,
                                    margin + vh + 2.0 + vh / 2.0,
                                    vw,
                                    vh,
                                ),
                                (margin + vw / 2.0, margin + vh / 2.0, vw, vh),
                                (margin + vw + 2.0 + vw / 2.0, margin + vh / 2.0, vw, vh),
                            ]
                        }
                        "SINGLE" | "1" => {
                            // Single full-page viewport
                            vec![(margin + uw / 2.0, margin + uh / 2.0, uw, uh)]
                        }
                        _ => {
                            self.command_line.push_error(
                                "VPORTS: unknown option. Use VPORTS 2H | 2V | 4 | SINGLE",
                            );
                            vec![]
                        }
                    };
                    if !rects.is_empty() {
                        // Remove existing user viewports in this layout first.
                        let layout_block = self.tabs[i].scene.current_layout_block_handle_pub();
                        let to_erase: Vec<acadrust::Handle> = self.tabs[i]
                            .scene
                            .document
                            .entities()
                            .filter_map(|e| {
                                if let acadrust::EntityType::Viewport(vp) = e {
                                    if vp.id > 1 && vp.common.owner_handle == layout_block {
                                        Some(vp.common.handle)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if to_erase
                            .iter()
                            .any(|handle| self.tabs[i].scene.is_layer_locked(*handle))
                        {
                            self.command_line.push_error(
                                crate::t!("VPORTS: unlock existing viewport layers first.")
                                    .as_ref(),
                            );
                            return Some(Task::none());
                        }
                        self.push_undo_snapshot(i, "VPORTS");
                        self.tabs[i].scene.erase_entities(&to_erase);
                        // Create new viewports.
                        for (cx, cz, w, h) in &rects {
                            let mut vp = acadrust::entities::Viewport::new();
                            vp.center = acadrust::types::Vector3::new(*cx, 0.0, *cz);
                            vp.width = *w;
                            vp.height = *h;
                            vp.id = 2; // commit_entity will assign unique IDs
                            match self.tabs[i].scene.document.add_entity_to_layout(
                                acadrust::EntityType::Viewport(vp),
                                &layout_name,
                            ) {
                                Ok(handle) => {
                                    self.tabs[i].scene.auto_fit_viewport(handle);
                                }
                                Err(e) => {
                                    self.command_line.push_error(crate::tf!("VPORTS: {e}").as_ref());
                                }
                            }
                        }
                        // Re-assign unique IDs (1 + existing max per viewport).
                        let layout_block2 = self.tabs[i].scene.current_layout_block_handle_pub();
                        let mut id_counter = 2_i16;
                        let handles: Vec<acadrust::Handle> = self.tabs[i]
                            .scene
                            .document
                            .entities()
                            .filter_map(|e| {
                                if let acadrust::EntityType::Viewport(vp) = e {
                                    if vp.id >= 2 && vp.common.owner_handle == layout_block2 {
                                        Some(vp.common.handle)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                            .collect();
                        for h in handles {
                            if let Some(acadrust::EntityType::Viewport(vp)) =
                                self.tabs[i].scene.document.get_entity_mut(h)
                            {
                                vp.id = id_counter;
                                id_counter += 1;
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::tf!(
                            "VPORTS: created {} viewport(s) [{}].",
                            rects.len(),
                            sub
                        ).as_ref());
                    }
                }
            }

            // ── VPLAYER — per-viewport layer freeze/thaw ──────────────────
            "VPLAYER" => {
                let scene = &self.tabs[i].scene;
                if scene.current_layout == "Model" {
                    self.command_line
                        .push_error(crate::t!("VPLAYER: switch to a paper space layout first.").as_ref());
                } else if scene.active_viewport.is_none() {
                    self.command_line
                        .push_error(crate::t!("VPLAYER: enter a viewport first (double-click or MS).").as_ref());
                } else {
                    use crate::modules::layout::vplayer::VplayerCommand;
                    let vp_handle = scene.active_viewport.unwrap();
                    // Collect current frozen layer names for display.
                    let frozen_names: Vec<String> = {
                        if let Some(acadrust::EntityType::Viewport(vp)) =
                            scene.document.get_entity(vp_handle)
                        {
                            vp.frozen_layers
                                .iter()
                                .filter_map(|h| {
                                    scene
                                        .document
                                        .layers
                                        .iter()
                                        .find(|l| l.handle == *h)
                                        .map(|l| l.name.clone())
                                })
                                .collect()
                        } else {
                            vec![]
                        }
                    };
                    if frozen_names.is_empty() {
                        self.command_line
                            .push_info(crate::t!("VPLAYER: no frozen layers in active viewport.").as_ref());
                    } else {
                        self.command_line.push_info(crate::tf!(
                            "VPLAYER: frozen layers: {}",
                            frozen_names.join(", ")
                        ).as_ref());
                    }
                    let new_cmd = VplayerCommand::new(vp_handle);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            // ── Draw Order ────────────────────────────────────────────────
            // TEXTTOFRONT / TEXTTOBACK — move every text and dimension object to
            // the front (or back) of the draw order via the DRAWORDER machinery.
            "TEXTTOFRONT" | "TEXTTOBACK" => {
                let to_front = cmd.ends_with("FRONT");
                let block_handle = self.tabs[i].scene.current_layout_block_handle_pub();
                let handles: rustc_hash::FxHashSet<acadrust::Handle> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter(|e| {
                        let c = e.common();
                        (c.owner_handle == block_handle || c.owner_handle.is_null())
                            && matches!(
                                e,
                                acadrust::EntityType::Text(_)
                                    | acadrust::EntityType::MText(_)
                                    | acadrust::EntityType::Dimension(_)
                            )
                    })
                    .map(|e| e.common().handle)
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_info(crate::tf!("{cmd}: no text or dimension objects.").as_ref());
                    return Some(Task::none());
                }
                self.tabs[i].scene.replace_selection(handles);
                return self.dispatch_view(
                    if to_front {
                        "DRAWORDER FRONT"
                    } else {
                        "DRAWORDER BACK"
                    },
                    i,
                );
            }

            // HATCHTOBACK ÔÇö move every hatch object in the active space to the back of the draw order.
            "HATCHTOBACK" => {
                use acadrust::objects::{ObjectType, SortEntitiesTable};
                let block_handle = self.tabs[i].scene.current_layout_block_handle_pub();
                let doc_ref = &self.tabs[i].scene.document;

                // 1. Single scan over objects to find existing SortEntitiesTable handle & overrides.
                let mut existing_table_handle = None;
                let mut overrides: Option<rustc_hash::FxHashMap<u64, u64>> = None;
                for (h, obj) in &doc_ref.objects {
                    if let ObjectType::SortEntitiesTable(t) = obj {
                        if t.block_owner_handle == block_handle {
                            existing_table_handle = Some(*h);
                            if !t.is_empty() {
                                overrides = Some(
                                    t.entries()
                                        .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
                                        .collect(),
                                );
                            }
                            break;
                        }
                    }
                }

                // 2. Pre-filter locked layers with zero string allocation.
                let locked_layers: rustc_hash::FxHashSet<&str> = doc_ref
                    .layers
                    .iter()
                    .filter(|l| l.is_locked())
                    .map(|l| l.name.as_str())
                    .collect();
                let has_locked_layers = !locked_layers.is_empty();

                // 3. Single pass over entities in the active space.
                let mut hatches_to_move: Vec<acadrust::Handle> = Vec::new();
                let mut min_eff = u64::MAX;

                for e in doc_ref.entities() {
                    let c = e.common();
                    if c.owner_handle != block_handle && !c.owner_handle.is_null() {
                        continue;
                    }
                    let hv = c.handle.value();
                    if matches!(e, acadrust::EntityType::Hatch(_)) {
                        if !has_locked_layers || !locked_layers.contains(c.layer.as_str()) {
                            hatches_to_move.push(c.handle);
                        }
                    } else {
                        let eff = match &overrides {
                            Some(map) => map.get(&hv).copied().unwrap_or(hv),
                            None => hv,
                        };
                        min_eff = min_eff.min(eff);
                    }
                }

                if hatches_to_move.is_empty() {
                    self.command_line
                        .push_info(crate::tf!("{cmd}: no hatch objects.").as_ref());
                    return Some(Task::none());
                }

                if min_eff == u64::MAX {
                    min_eff = 1;
                }

                // Compute all sort-key assignments before any mutable step:
                // the exhausted-floor path needs one more read-only pass over
                // entities to find colliding siblings to lift.
                let assigns = assign_back_group_keys(
                    doc_ref,
                    block_handle,
                    &hatches_to_move,
                    min_eff,
                    overrides.as_ref(),
                    &locked_layers,
                );

                // 4. Ultra-fast targeted Delta Undo (snapshots ONLY the SortEntitiesTable, zero full-drawing clone).
                let pending_delta = self.begin_undo(i, "DRAWORDER", hatches_to_move.len(), true);

                // 5. Update or insert SortEntitiesTable directly.
                let table_before = existing_table_handle
                    .and_then(|h| self.tabs[i].scene.document.objects.get(&h).cloned());
                let th = existing_table_handle.unwrap_or_else(|| {
                    let nh = acadrust::Handle::new(self.tabs[i].scene.document.next_handle());
                    let mut table = SortEntitiesTable::for_block(block_handle);
                    table.handle = nh;
                    self.tabs[i]
                        .scene
                        .document
                        .objects
                        .insert(nh, ObjectType::SortEntitiesTable(table));
                    nh
                });
                self.tabs[i].scene.record_undo_object_before(th, table_before);

                if let Some(ObjectType::SortEntitiesTable(table)) =
                    self.tabs[i].scene.document.objects.get_mut(&th)
                {
                    for (h, sort) in &assigns {
                        table.add_entry(*h, acadrust::Handle::new(*sort));
                    }
                }

                if let Some(pending) = pending_delta {
                    self.commit_undo_delta(i, pending);
                }

                // 6. Invalidate ONLY draw-depth cache without dropping whole-drawing tessellations or spatial indexes.
                let changes: Vec<(acadrust::Handle, crate::scene::ChangeKind)> = hatches_to_move
                    .iter()
                    .map(|h| (*h, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities(&changes);
                self.tabs[i].scene.invalidate_draw_depth();

                self.tabs[i].dirty = true;
                self.command_line.push_info(
                    crate::tf!("DRAWORDER: moved {} entities to back.", hatches_to_move.len()).as_ref(),
                );

                return Some(Task::none());
            }

            "DRAWORDER" => {
                let selected: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                let c = DrawOrderCommand::new(selected);
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("DRAWORDER ") => {
                use acadrust::objects::{ObjectType, SortEntitiesTable};
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                let option = parts.get(1).unwrap_or(&"").to_uppercase();
                let i = self.active_tab;
                let selected: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if selected.is_empty() {
                    self.command_line
                        .push_error(crate::t!("DRAWORDER: select entities first.").as_ref());
                } else {
                    // Parse relative target handle for ABOVE/UNDER.
                    let relative_target: Option<(bool, acadrust::Handle)> = match option.as_str() {
                        "A" | "ABOVE" => {
                            let h_val = parts.get(2).and_then(|s| u64::from_str_radix(s, 16).ok());
                            h_val.map(|v| (true, acadrust::Handle::new(v)))
                        }
                        "U" | "UNDER" | "BELOW" => {
                            let h_val = parts.get(2).and_then(|s| u64::from_str_radix(s, 16).ok());
                            h_val.map(|v| (false, acadrust::Handle::new(v)))
                        }
                        _ => None,
                    };
                    let to_front_opt = match option.as_str() {
                        "F" | "FRONT" => Some(true),
                        "B" | "BACK" => Some(false),
                        _ => None,
                    };

                    if relative_target.is_some() || to_front_opt.is_some() {
                        self.push_undo_snapshot(i, "DRAWORDER");
                        let block_handle = self.tabs[i].scene.current_layout_block_handle_pub();

                        // For FRONT/BACK, anchor the new sort handle to the
                        // block's current effective draw-order range so the moved
                        // entities land strictly above/below every sibling —
                        // including ones not yet in the table, which sort by
                        // their own handle. (min_eff, max_eff) over siblings.
                        let mut back_assigns: Option<Vec<(acadrust::Handle, u64)>> = None;
                        let fb_baseline: Option<(u64, u64)> = if to_front_opt.is_some() {
                            let selected_set: rustc_hash::FxHashSet<u64> =
                                selected.iter().map(|h| h.value()).collect();
                            let doc_ref = &self.tabs[i].scene.document;
                            let overrides: rustc_hash::FxHashMap<u64, u64> = doc_ref
                                .objects
                                .values()
                                .find_map(|obj| {
                                    if let ObjectType::SortEntitiesTable(t) = obj {
                                        if t.block_owner_handle == block_handle {
                                            return Some(
                                                t.entries()
                                                    .map(|e| {
                                                        (
                                                            e.entity_handle.value(),
                                                            e.sort_handle.value(),
                                                        )
                                                    })
                                                    .collect(),
                                            );
                                        }
                                    }
                                    None
                                })
                                .unwrap_or_default();
                            let mut max_eff = 0u64;
                            let mut min_eff = u64::MAX;
                            for e in doc_ref.entities() {
                                let c = e.common();
                                let hv = c.handle.value();
                                if selected_set.contains(&hv) {
                                    continue;
                                }
                                if c.owner_handle != block_handle && !c.owner_handle.is_null() {
                                    continue;
                                }
                                let eff = overrides.get(&hv).copied().unwrap_or(hv);
                                max_eff = max_eff.max(eff);
                                min_eff = min_eff.min(eff);
                            }
                            if min_eff == u64::MAX {
                                min_eff = 1;
                            }
                            if to_front_opt == Some(false) {
                                // Same exhausted-floor handling as HATCHTOBACK:
                                // never clamp the group onto tied keys.
                                let locked_layers: rustc_hash::FxHashSet<&str> = doc_ref
                                    .layers
                                    .iter()
                                    .filter(|l| l.is_locked())
                                    .map(|l| l.name.as_str())
                                    .collect();
                                back_assigns = Some(assign_back_group_keys(
                                    doc_ref,
                                    block_handle,
                                    &selected,
                                    min_eff,
                                    Some(&overrides),
                                    &locked_layers,
                                ));
                            }
                            Some((min_eff, max_eff))
                        } else {
                            None
                        };

                        let doc = &mut self.tabs[i].scene.document;
                        let table_handle = doc.objects.iter().find_map(|(h, obj)| {
                            if let ObjectType::SortEntitiesTable(t) = obj {
                                if t.block_owner_handle == block_handle {
                                    Some(*h)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        });
                        let th = table_handle.unwrap_or_else(|| {
                            let nh = acadrust::Handle::new(doc.next_handle());
                            let mut table = SortEntitiesTable::for_block(block_handle);
                            table.handle = nh;
                            doc.objects.insert(nh, ObjectType::SortEntitiesTable(table));
                            nh
                        });
                        if let Some(ObjectType::SortEntitiesTable(table)) = doc.objects.get_mut(&th)
                        {
                            if let Some((above, target)) = relative_target {
                                // move_above/move_below recompute target±1 per
                                // call, so looping them over N selected entities
                                // ties the whole group onto one key. Read the
                                // reference key once and hand out per-index
                                // keys instead, keeping the moved entities a
                                // distinct, selection-ordered block adjacent to
                                // the reference.
                                let target_sort = match table.get_sort_handle(target) {
                                    Some(h) => h.value(),
                                    None => {
                                        // A reference object that was never
                                        // reordered isn't in the table yet;
                                        // seed it with its own handle as the
                                        // implicit sort key.
                                        table.add_entry(target, target);
                                        table
                                            .get_sort_handle(target)
                                            .map_or(target.value(), |h| h.value())
                                    }
                                };
                                for (k, h) in selected.iter().enumerate() {
                                    let offset = 1 + k as u64;
                                    let sort = if above {
                                        target_sort.saturating_add(offset)
                                    } else {
                                        target_sort.saturating_sub(offset).max(1)
                                    };
                                    table.add_entry(*h, acadrust::Handle::new(sort));
                                }
                                let rel = if above { "above" } else { "below" };
                                self.command_line.push_info(crate::tf!(
                                    "DRAWORDER: moved {} entities {} {:x}.",
                                    selected.len(),
                                    rel,
                                    target.value()
                                ).as_ref());
                            } else if let Some(to_front) = to_front_opt {
                                if to_front {
                                    let (_, max_eff) = fb_baseline.unwrap_or((1, 0));
                                    for (k, h) in selected.iter().enumerate() {
                                        let sort = max_eff.saturating_add(1 + k as u64);
                                        table.add_entry(*h, acadrust::Handle::new(sort));
                                    }
                                } else if let Some(assigns) = &back_assigns {
                                    for (h, sort) in assigns {
                                        table.add_entry(*h, acadrust::Handle::new(*sort));
                                    }
                                }
                                let dir = if to_front { "front" } else { "back" };
                                self.command_line.push_info(crate::tf!(
                                    "DRAWORDER: moved {} entities to {}.",
                                    selected.len(),
                                    dir
                                ).as_ref());
                            }
                        }
                        // Sort order lives in SortEntitiesTable, which the
                        // render-side `sort_cache` rebuilds per geometry epoch.
                        // Bump it so the new draw order shows immediately
                        // instead of waiting for an unrelated geometry change.
                        // Draw order changes submission order only; all
                        // per-entity tessellation remains valid.
                        self.tabs[i].scene.bump_geometry_no_blocks();
                        self.tabs[i].dirty = true;
                    } else {
                        self.command_line.push_info(
                            crate::t!("Usage: DRAWORDER F|FRONT | B|BACK | A|ABOVE <handle> | U|UNDER <handle>").as_ref()
                        );
                    }
                }
            }

            // SYNCPVIEWPORTS — copy the first selected viewport's display settings
            // (view direction/target, scale, snap/grid, frozen layers) to the rest.
            "SYNCPVIEWPORTS" | "VPSYNC" => {
                let vps: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter(|(_, e)| matches!(e, acadrust::EntityType::Viewport(_)))
                    .map(|(h, _)| *h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if vps.len() < 2 {
                    self.command_line.push_error(
                        "SYNCPVIEWPORTS: select two or more viewports (the first is the master).",
                    );
                    return Some(Task::none());
                }
                let src = match self.tabs[i].scene.document.get_entity(vps[0]) {
                    Some(acadrust::EntityType::Viewport(vp)) => vp.clone(),
                    _ => {
                        self.command_line
                            .push_error(crate::t!("SYNCPVIEWPORTS: master is not a viewport.").as_ref());
                        return Some(Task::none());
                    }
                };
                self.push_undo_snapshot(i, "SYNCPVIEWPORTS");
                let mut n = 0usize;
                for h in &vps[1..] {
                    if let Some(acadrust::EntityType::Viewport(vp)) =
                        self.tabs[i].scene.document.get_entity_mut(*h)
                    {
                        vp.view_direction = src.view_direction;
                        vp.view_target = src.view_target;
                        vp.view_height = src.view_height;
                        vp.snap_base = src.snap_base;
                        vp.snap_spacing = src.snap_spacing;
                        vp.snap_angle = src.snap_angle;
                        vp.grid_spacing = src.grid_spacing;
                        vp.grid_major = src.grid_major;
                        vp.frozen_layers = src.frozen_layers.clone();
                        n += 1;
                    }
                }
                self.tabs[i].dirty = true;
                self.command_line.push_output(crate::tf!(
                    "SYNCPVIEWPORTS: synced {n} viewport(s) to the master."
                ).as_ref());
            }

            // HIDE — hidden-line view of the active viewport.
            "HIDE" => {
                return Some(Task::done(Message::SetRenderMode(
                    acadrust::entities::ViewportRenderMode::HiddenLine,
                )));
            }

            // VISUALSTYLES <name> — put the active viewport in one of the seven
            // render modes. The style-definition manager is not modelled.
            cmd if cmd.starts_with("VISUALSTYLES ") => {
                use crate::modules::view::visual_style;
                let name = cmd.strip_prefix("VISUALSTYLES").unwrap_or("").trim();
                match visual_style::mode_for_keyword(name) {
                    Some(mode) => return Some(Task::done(Message::SetRenderMode(mode))),
                    // Listed from the table, so the names offered are the names
                    // that work.
                    None => self
                        .command_line
                        .push_info(visual_style::keyword_prompt()),
                }
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}

// ── Draw Order: interactive command ──────────────────────────
 
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DrawOrderStep {
    SelectObjects,
    ChooseVerb,
    PickReference { above: bool },
}

/// Interactive front-end for the DRAWORDER command.
///
/// Flow:
/// 1. If nothing is selected, prompt to select objects (Enter confirms).
/// 2. Prompt for verb: `[Above / Under / Front / Back]`.
///    - `Front` / `F`: moves selection to front.
///    - `Back` / `B` / Enter: moves selection to back.
///    - `Above` / `A` / `Under` / `U`: advances to reference object pick.
/// 3. Reference object pick: user can click the reference entity in the viewport
///    or type its hex handle on the command line.
pub(crate) struct DrawOrderCommand {
    selected: Vec<acadrust::Handle>,
    step: DrawOrderStep,
}

impl DrawOrderCommand {
    pub(crate) fn new(selected: Vec<acadrust::Handle>) -> Self {
        let step = if selected.is_empty() {
            DrawOrderStep::SelectObjects
        } else {
            DrawOrderStep::ChooseVerb
        };
        Self { selected, step }
    }

    pub(crate) fn for_reference_pick(selected: Vec<acadrust::Handle>, above: bool) -> Self {
        Self {
            selected,
            step: DrawOrderStep::PickReference { above },
        }
    }
}

impl CadCommand for DrawOrderCommand {
    fn name(&self) -> &'static str {
        "DRAWORDER"
    }

    fn prompt(&self) -> String {
        match self.step {
            DrawOrderStep::SelectObjects => {
                crate::t!("DRAWORDER  select objects, then press Enter:").into_owned()
            }
            DrawOrderStep::ChooseVerb => {
                crate::t!("DRAWORDER  [Above / Under / Front / Back] <Back>:").into_owned()
            }
            DrawOrderStep::PickReference { above: true } => {
                crate::t!("DRAWORDER  Select reference object (move selection above):").into_owned()
            }
            DrawOrderStep::PickReference { above: false } => {
                crate::t!("DRAWORDER  Select reference object (move selection under):").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        match self.step {
            DrawOrderStep::ChooseVerb => vec![
                crate::command::CmdOption::new("Above", "ABOVE"),
                crate::command::CmdOption::new("Under", "UNDER"),
                crate::command::CmdOption::new("Front", "FRONT"),
                crate::command::CmdOption::new("Back", "BACK"),
            ],
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, DrawOrderStep::SelectObjects)
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, DrawOrderStep::SelectObjects)
    }

    fn on_selection_complete(&mut self, handles: Vec<acadrust::Handle>) -> crate::command::CmdResult {
        self.selected = handles;
        crate::command::CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> crate::command::CmdResult {
        match self.step {
            DrawOrderStep::SelectObjects => {
                if self.selected.is_empty() {
                    crate::command::CmdResult::Cancel
                } else {
                    self.step = DrawOrderStep::ChooseVerb;
                    crate::command::CmdResult::NeedPoint
                }
            }
            DrawOrderStep::ChooseVerb => {
                // Bare Enter defaults to Back
                let handles = std::mem::take(&mut self.selected);
                crate::command::CmdResult::Relaunch("DRAWORDER BACK".into(), handles)
            }
            DrawOrderStep::PickReference { .. } => crate::command::CmdResult::Cancel,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<crate::command::CmdResult> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        match self.step {
            DrawOrderStep::SelectObjects => None,
            DrawOrderStep::ChooseVerb => {
                let up = t.to_uppercase();
                match up.as_str() {
                    "F" | "FRONT" => {
                        let handles = std::mem::take(&mut self.selected);
                        Some(crate::command::CmdResult::Relaunch("DRAWORDER FRONT".into(), handles))
                    }
                    "B" | "BACK" => {
                        let handles = std::mem::take(&mut self.selected);
                        Some(crate::command::CmdResult::Relaunch("DRAWORDER BACK".into(), handles))
                    }
                    "A" | "ABOVE" => {
                        self.step = DrawOrderStep::PickReference { above: true };
                        Some(crate::command::CmdResult::NeedPoint)
                    }
                    "U" | "UNDER" | "BELOW" => {
                        self.step = DrawOrderStep::PickReference { above: false };
                        Some(crate::command::CmdResult::NeedPoint)
                    }
                    _ => Some(crate::command::CmdResult::NeedPoint),
                }
            }
            DrawOrderStep::PickReference { above } => {
                let hex_str = t.trim_start_matches("0x").trim_start_matches("0X");
                if let Ok(val) = u64::from_str_radix(hex_str, 16) {
                    let opt = if above { "A" } else { "U" };
                    let cmd = format!("DRAWORDER {} {:x}", opt, val);
                    let handles = std::mem::take(&mut self.selected);
                    Some(crate::command::CmdResult::Relaunch(cmd, handles))
                } else {
                    Some(crate::command::CmdResult::NeedPoint)
                }
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, DrawOrderStep::PickReference { .. })
    }

    fn on_entity_pick(
        &mut self,
        handle: acadrust::Handle,
        _pt: glam::DVec3,
    ) -> crate::command::CmdResult {
        if handle.is_null() {
            return crate::command::CmdResult::NeedPoint;
        }
        if let DrawOrderStep::PickReference { above } = self.step {
            let opt = if above { "A" } else { "U" };
            let cmd = format!("DRAWORDER {} {:x}", opt, handle.value());
            let handles = std::mem::take(&mut self.selected);
            crate::command::CmdResult::Relaunch(cmd, handles)
        } else {
            crate::command::CmdResult::NeedPoint
        }
    }

    fn on_point(&mut self, _pt: glam::DVec3) -> crate::command::CmdResult {
        crate::command::CmdResult::NeedPoint
    }
}

/// Sort-key assignments sending `group` to the back of the active space.
///
/// Normal case: the floor is the lowest effective sort key among non-moved
/// siblings, and there are enough unused slots below it — hand them out in
/// ascending order so the group lands as one block strictly behind everything
/// else with its internal stacking order preserved.
///
/// Exhausted case (`floor <= group.len()`, e.g. a sibling already pinned at
/// key 1 or 2): decrementing would clamp every member onto one tied key via
/// `.max(1)`. Instead pin the group to keys 1..=n and lift every other
/// in-space sibling whose effective key collides with that range above it,
/// shifted by `n` so lifted siblings keep their relative order. Locked-layer
/// entities are never moved or lifted.
fn assign_back_group_keys(
    doc: &acadrust::CadDocument,
    block_handle: acadrust::Handle,
    group: &[acadrust::Handle],
    floor: u64,
    overrides: Option<&rustc_hash::FxHashMap<u64, u64>>,
    locked_layers: &rustc_hash::FxHashSet<&str>,
) -> Vec<(acadrust::Handle, u64)> {
    let n = group.len() as u64;
    let mut out: Vec<(acadrust::Handle, u64)> = Vec::with_capacity(group.len());
    if n == 0 {
        return out;
    }
    if floor > n {
        for (k, h) in group.iter().enumerate() {
            out.push((*h, floor - n + k as u64));
        }
        return out;
    }
    for (k, h) in group.iter().enumerate() {
        out.push((*h, 1 + k as u64));
    }
    let moved: rustc_hash::FxHashSet<u64> = group.iter().map(|h| h.value()).collect();
    let has_locked = !locked_layers.is_empty();
    let mut lifts: Vec<(acadrust::Handle, u64)> = Vec::new();
    for e in doc.entities() {
        let c = e.common();
        if c.owner_handle != block_handle && !c.owner_handle.is_null() {
            continue;
        }
        let hv = c.handle.value();
        if moved.contains(&hv) {
            continue;
        }
        if has_locked && locked_layers.contains(c.layer.as_str()) {
            continue;
        }
        let eff = overrides.map_or(hv, |m| m.get(&hv).copied().unwrap_or(hv));
        if eff <= n {
            lifts.push((c.handle, eff.saturating_add(n)));
        }
    }
    out.extend(lifts);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use acadrust::objects::ObjectType;
    use acadrust::EntityType;

    fn fresh_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn hatchtoback_no_hatches_warns_and_noop() {
        let mut app = fresh_app();
        let _ = app.run_command_line("HATCHTOBACK");
        let i = app.active_tab;
        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let table_exists = app.tabs[i].scene.document.objects.values().any(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                t.block_owner_handle == block_handle
            } else {
                false
            }
        });
        assert!(!table_exists, "No SortEntitiesTable created when no hatches exist");
    }

    #[test]
    fn hatchtoback_moves_hatches_to_back() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line1 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_line2 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));

        let _ = app.run_command_line("HATCHTOBACK");

        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let table = app.tabs[i].scene.document.objects.values().find_map(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if t.block_owner_handle == block_handle {
                    return Some(t);
                }
            }
            None
        }).expect("SortEntitiesTable should exist for the active layout block");

        let entries: rustc_hash::FxHashMap<u64, u64> = table
            .entries()
            .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
            .collect();

        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let line1_sort = entries.get(&h_line1.value()).copied().unwrap_or(h_line1.value());
        let line2_sort = entries.get(&h_line2.value()).copied().unwrap_or(h_line2.value());

        assert!(
            hatch_sort < line1_sort,
            "Hatch sort handle ({hatch_sort}) must be behind line 1 ({line1_sort})"
        );
        assert!(
            hatch_sort < line2_sort,
            "Hatch sort handle ({hatch_sort}) must be behind line 2 ({line2_sort})"
        );
    }

    #[test]
    fn hb_alias_moves_hatches_to_back() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        let _ = app.run_command_line("HB");

        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let table = app.tabs[i].scene.document.objects.values().find_map(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if t.block_owner_handle == block_handle {
                    return Some(t);
                }
            }
            None
        }).expect("SortEntitiesTable should exist for the active layout block");

        let entries: rustc_hash::FxHashMap<u64, u64> = table
            .entries()
            .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
            .collect();

        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let line_sort = entries.get(&h_line.value()).copied().unwrap_or(h_line.value());

        assert!(
            hatch_sort < line_sort,
            "Hatch sort handle ({hatch_sort}) must be behind line ({line_sort}) via HB alias"
        );
    }

    #[test]
    fn hatchtoback_ignores_hatches_in_other_spaces() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let other_block = acadrust::Handle::new(0x9999);
        let h_foreign = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(h_foreign) {
            entity.common_mut().owner_handle = other_block;
        }

        // When no hatches exist in the current layout, HATCHTOBACK should no-op.
        let _ = app.run_command_line("HATCHTOBACK");

        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let table_exists = app.tabs[i].scene.document.objects.values().any(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                t.block_owner_handle == block_handle
            } else {
                false
            }
        });
        assert!(!table_exists, "Foreign hatch must not trigger table creation in active layout");

        // Now add a line and a hatch in the active layout.
        let h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        let _ = app.run_command_line("HATCHTOBACK");

        let table = app.tabs[i].scene.document.objects.values().find_map(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if t.block_owner_handle == block_handle {
                    return Some(t);
                }
            }
            None
        }).expect("SortEntitiesTable should exist for current layout");

        let entries: rustc_hash::FxHashMap<u64, u64> = table
            .entries()
            .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
            .collect();

        assert!(!entries.contains_key(&h_foreign.value()), "Foreign hatch must not be in active layout table");
        assert!(entries.contains_key(&h_hatch.value()), "Active layout hatch must be in table");
        let hatch_sort = entries[&h_hatch.value()];
        let line_sort = entries.get(&h_line.value()).copied().unwrap_or(h_line.value());
        assert!(hatch_sort < line_sort);
    }

    #[test]
    fn hatchtoback_multiple_calls_are_idempotent() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch1 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_hatch2 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        let _ = app.run_command_line("HATCHTOBACK");
        let _ = app.run_command_line("HATCHTOBACK");

        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let table = app.tabs[i].scene.document.objects.values().find_map(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if t.block_owner_handle == block_handle {
                    return Some(t);
                }
            }
            None
        }).expect("SortEntitiesTable should exist");

        let entries: rustc_hash::FxHashMap<u64, u64> = table
            .entries()
            .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
            .collect();

        let h1_sort = entries.get(&h_hatch1.value()).copied().unwrap_or(h_hatch1.value());
        let h2_sort = entries.get(&h_hatch2.value()).copied().unwrap_or(h_hatch2.value());
        let line_sort = entries.get(&h_line.value()).copied().unwrap_or(h_line.value());

        assert!(h1_sort < line_sort);
        assert!(h2_sort < line_sort);
    }

    #[test]
    fn hatchtoback_preserves_active_selection() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let _h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        // User had the line selected before running HATCHTOBACK.
        app.tabs[i].scene.replace_selection(std::iter::once(h_line).collect());

        let _ = app.run_command_line("HATCHTOBACK");

        let selected = app.tabs[i].scene.selected_handles_in_order();
        assert_eq!(selected, vec![h_line], "HATCHTOBACK must not overwrite existing user selection");
    }

    #[test]
    fn hatchtoback_skips_hatches_on_locked_layers() {
        let mut app = fresh_app();
        let i = app.active_tab;

        // Lock layer "LOCKED_LAYER".
        app.tabs[i].scene.ensure_layer("LOCKED_LAYER");
        if let Some(layer) = app.tabs[i].scene.document.layers.get_mut("LOCKED_LAYER") {
            layer.flags.locked = true;
        }

        let mut locked_hatch = acadrust::entities::Hatch::default();
        locked_hatch.common.layer = "LOCKED_LAYER".into();
        let h_locked_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(locked_hatch));

        let h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));

        // Run HATCHTOBACK when only a locked hatch exists: it should not move the locked hatch.
        let _ = app.run_command_line("HATCHTOBACK");

        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let table_exists = app.tabs[i].scene.document.objects.values().any(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                t.block_owner_handle == block_handle
            } else {
                false
            }
        });
        assert!(!table_exists, "Locked hatch must not trigger table creation");

        // Now add an unlocked hatch.
        let h_unlocked_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        let _ = app.run_command_line("HATCHTOBACK");

        let table = app.tabs[i].scene.document.objects.values().find_map(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if t.block_owner_handle == block_handle {
                    return Some(t);
                }
            }
            None
        }).expect("SortEntitiesTable should exist for unlocked hatch");

        let entries: rustc_hash::FxHashMap<u64, u64> = table
            .entries()
            .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
            .collect();

        assert!(!entries.contains_key(&h_locked_hatch.value()), "Locked hatch must not be reordered");
        assert!(entries.contains_key(&h_unlocked_hatch.value()), "Unlocked hatch must be reordered");
        let hatch_sort = entries[&h_unlocked_hatch.value()];
        let line_sort = entries.get(&h_line.value()).copied().unwrap_or(h_line.value());
        assert!(hatch_sort < line_sort);
    }

    // HATCHTOBACK must preserve the moved group's internal stacking order.
    // Written while auditing the descending key assignment, which silently
    // REVERSES hatch-to-hatch order; this test fails while that holds.
    #[test]
    fn hatchtoback_preserves_hatch_stacking_order() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch1 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_hatch2 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_hatch3 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        let _ = app.run_command_line("HATCHTOBACK");

        let entries = effective_sort_map(&app);
        let line_sort = entries.get(&h_line.value()).copied().unwrap_or(h_line.value());
        let s1 = entries.get(&h_hatch1.value()).copied().unwrap_or(h_hatch1.value());
        let s2 = entries.get(&h_hatch2.value()).copied().unwrap_or(h_hatch2.value());
        let s3 = entries.get(&h_hatch3.value()).copied().unwrap_or(h_hatch3.value());

        assert!(s3 > s2 && s2 > s1, "stacking order must be preserved, got {s1}, {s2}, {s3}");
        assert!(s1 < line_sort && s2 < line_sort && s3 < line_sort, "all hatches must stay behind the line");
    }

    // When the key space below the floor is exhausted (an entity already sits
    // at a very low sort key), HATCHTOBACK must not clamp every hatch onto one
    // tied key: hatches stay pairwise distinct and no sibling may sink below
    // them. Fails while `.max(1)` clamping is in place.
    #[test]
    fn hatchtoback_exhausted_floor_keeps_hatches_distinct_and_behind() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch1 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_hatch2 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_hatch3 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        // Pin the line to sort key 2: only ONE slot below the floor exists,
        // but three hatches need to fit behind it.
        {
            use acadrust::objects::{ObjectType, SortEntitiesTable};
            let doc = &mut app.tabs[i].scene.document;
            let nh = acadrust::Handle::new(doc.next_handle());
            let mut table = SortEntitiesTable::for_block(block_handle);
            table.handle = nh;
            table.add_entry(h_line, acadrust::Handle::new(2));
            doc.objects.insert(nh, ObjectType::SortEntitiesTable(table));
        }

        let _ = app.run_command_line("HATCHTOBACK");

        let entries = effective_sort_map(&app);
        let line_sort = entries.get(&h_line.value()).copied().unwrap_or(h_line.value());
        let s1 = entries.get(&h_hatch1.value()).copied().unwrap_or(h_hatch1.value());
        let s2 = entries.get(&h_hatch2.value()).copied().unwrap_or(h_hatch2.value());
        let s3 = entries.get(&h_hatch3.value()).copied().unwrap_or(h_hatch3.value());

        assert_ne!(s1, s2, "hatches must not tie on one clamped key");
        assert_ne!(s1, s3, "hatches must not tie on one clamped key");
        assert_ne!(s2, s3, "hatches must not tie on one clamped key");
        assert!(
            line_sort > s1.max(s2).max(s3),
            "the pinned line ({line_sort}) must still render above every hatch ({s1}, {s2}, {s3})"
        );
    }

    #[test]
    fn hatchtoback_undo_restores_draw_order() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let _h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        let _ = app.run_command_line("HATCHTOBACK");

        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let table = app.tabs[i].scene.document.objects.values().find_map(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if t.block_owner_handle == block_handle {
                    return Some(t);
                }
            }
            None
        }).expect("SortEntitiesTable should exist");
        assert!(table.entries().any(|e| e.entity_handle == h_hatch));

        // Perform UNDO.
        let _ = app.update(crate::app::Message::Undo);

        let table_after_undo = app.tabs[i].scene.document.objects.values().find_map(|obj| {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if t.block_owner_handle == block_handle {
                    return Some(t);
                }
            }
            None
        });
        // Table was created fresh by HATCHTOBACK, so undo should remove it or leave it empty of the hatch.
        let has_hatch = table_after_undo.map_or(false, |t| t.entries().any(|e| e.entity_handle == h_hatch));
        assert!(!has_hatch, "Undo must revert the SortEntitiesTable entry");
    }

    fn effective_sort_map(
        app: &OpenCADStudio,
    ) -> rustc_hash::FxHashMap<u64, u64> {
        let i = app.active_tab;
        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        app.tabs[i]
            .scene
            .document
            .objects
            .values()
            .find_map(|obj| {
                if let ObjectType::SortEntitiesTable(t) = obj {
                    if t.block_owner_handle == block_handle {
                        return Some(
                            t.entries()
                                .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
                                .collect(),
                        );
                    }
                }
                None
            })
            .unwrap_or_default()
    }

    // Regression guard for DRAWORDER BACK anchoring. Written while auditing
    // the draw-order code, where BACK *looked* like it skipped the baseline
    // scan and clamped every moved entity onto sort key 1 (a tie). Running
    // this test disproved that — `to_front_opt` is Some(false) for BACK, so
    // the baseline scan does run. Kept to lock in the strict-order behavior:
    // a second BACK must land strictly below earlier assignments, never tie.
    #[test]
    fn draworder_back_twice_keeps_strict_order() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line1 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        // Send the hatch to back, then send line 1 behind it: the second BACK
        // must anchor strictly below the first assignment, never tie with it.
        app.tabs[i].scene.replace_selection(std::iter::once(h_hatch).collect());
        let _ = app.run_command_line("DRAWORDER BACK");
        app.tabs[i].scene.replace_selection(std::iter::once(h_line1).collect());
        let _ = app.run_command_line("DRAWORDER BACK");

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let line1_sort = entries.get(&h_line1.value()).copied().unwrap_or(h_line1.value());

        assert!(
            line1_sort < hatch_sort,
            "second BACK must land strictly below the first ({line1_sort} vs {hatch_sort})"
        );
    }

    // Regression test for a suspected multi-select tie in DRAWORDER
    // ABOVE/UNDER: the loop called move_above/move_below per entity, and each
    // call recomputes target±1, so N selected entities should all collapse to
    // one key. This test must fail while that bug exists.
    // Same floor-exhaustion scenario as hatchtoback_exhausted_floor_…, but
    // through the interactive DRAWORDER BACK path: two selected lines must
    // land distinct and strictly behind a sibling pinned at key 2, instead of
    // both clamping onto key 1.
    #[test]
    fn draworder_back_exhausted_floor_keeps_distinct_and_behind() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let block_handle = app.tabs[i].scene.current_layout_block_handle_pub();
        let h_pinned = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_line1 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_line2 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));

        {
            use acadrust::objects::{ObjectType, SortEntitiesTable};
            let doc = &mut app.tabs[i].scene.document;
            let nh = acadrust::Handle::new(doc.next_handle());
            let mut table = SortEntitiesTable::for_block(block_handle);
            table.handle = nh;
            table.add_entry(h_pinned, acadrust::Handle::new(2));
            doc.objects.insert(nh, ObjectType::SortEntitiesTable(table));
        }

        app.tabs[i]
            .scene
            .replace_selection([h_line1, h_line2].into_iter().collect());
        let _ = app.run_command_line("DRAWORDER BACK");

        let entries = effective_sort_map(&app);
        let pinned_sort = entries.get(&h_pinned.value()).copied().unwrap_or(h_pinned.value());
        let s1 = entries.get(&h_line1.value()).copied().unwrap_or(h_line1.value());
        let s2 = entries.get(&h_line2.value()).copied().unwrap_or(h_line2.value());

        assert_ne!(s1, s2, "BACK must not clamp both selected entities onto one tied key");
        assert!(
            pinned_sort > s1.max(s2),
            "pinned sibling ({pinned_sort}) must stay above the moved pair ({s1}, {s2})"
        );
    }

    #[test]
    fn draworder_above_multi_select_keeps_distinct_order() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_ref = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch1 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_hatch2 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        app.tabs[i]
            .scene
            .replace_selection([h_hatch1, h_hatch2].into_iter().collect());
        let cmd = format!("DRAWORDER ABOVE {:x}", h_ref.value());
        let _ = app.run_command_line(&cmd);

        let entries = effective_sort_map(&app);
        let ref_sort = entries.get(&h_ref.value()).copied().unwrap_or(h_ref.value());
        let hatch1_sort = entries.get(&h_hatch1.value()).copied().unwrap_or(h_hatch1.value());
        let hatch2_sort = entries.get(&h_hatch2.value()).copied().unwrap_or(h_hatch2.value());

        assert!(hatch1_sort > ref_sort, "hatch 1 ({hatch1_sort}) must be above reference ({ref_sort})");
        assert!(hatch2_sort > ref_sort, "hatch 2 ({hatch2_sort}) must be above reference ({ref_sort})");
        assert_ne!(hatch1_sort, hatch2_sort, "multi-select ABOVE must not tie selected entities");
        assert!(
            hatch2_sort > hatch1_sort,
            "selection order must be preserved within the moved group ({hatch1_sort}, {hatch2_sort})"
        );
    }

    #[test]
    fn draworder_under_multi_select_keeps_distinct_order() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_ref = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch1 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_hatch2 = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        app.tabs[i]
            .scene
            .replace_selection([h_hatch1, h_hatch2].into_iter().collect());
        let cmd = format!("DRAWORDER UNDER {:x}", h_ref.value());
        let _ = app.run_command_line(&cmd);

        let entries = effective_sort_map(&app);
        let ref_sort = entries.get(&h_ref.value()).copied().unwrap_or(h_ref.value());
        let hatch1_sort = entries.get(&h_hatch1.value()).copied().unwrap_or(h_hatch1.value());
        let hatch2_sort = entries.get(&h_hatch2.value()).copied().unwrap_or(h_hatch2.value());

        assert!(hatch1_sort < ref_sort, "hatch 1 ({hatch1_sort}) must be under reference ({ref_sort})");
        assert!(hatch2_sort < ref_sort, "hatch 2 ({hatch2_sort}) must be under reference ({ref_sort})");
        assert_ne!(hatch1_sort, hatch2_sort, "multi-select UNDER must not tie selected entities");
        assert!(
            hatch1_sort > hatch2_sort,
            "selection order must be preserved within the moved group ({hatch1_sort}, {hatch2_sort})"
        );
    }

    #[test]
    fn draworder_front_moves_hatch_in_front_of_all() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line1 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_line2 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));

        app.tabs[i].scene.replace_selection(std::iter::once(h_hatch).collect());
        let _ = app.run_command_line("DRAWORDER FRONT");

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let line1_sort = entries.get(&h_line1.value()).copied().unwrap_or(h_line1.value());
        let line2_sort = entries.get(&h_line2.value()).copied().unwrap_or(h_line2.value());

        assert!(hatch_sort > line1_sort, "hatch ({hatch_sort}) must render in front of line 1 ({line1_sort})");
        assert!(hatch_sort > line2_sort, "hatch ({hatch_sort}) must render in front of line 2 ({line2_sort})");
    }

    #[test]
    fn draworder_back_moves_hatch_behind_all() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line1 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));
        let h_line2 = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));

        app.tabs[i].scene.replace_selection(std::iter::once(h_hatch).collect());
        let _ = app.run_command_line("DRAWORDER BACK");

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let line1_sort = entries.get(&h_line1.value()).copied().unwrap_or(h_line1.value());
        let line2_sort = entries.get(&h_line2.value()).copied().unwrap_or(h_line2.value());

        assert!(hatch_sort < line1_sort, "hatch ({hatch_sort}) must render behind line 1 ({line1_sort})");
        assert!(hatch_sort < line2_sort, "hatch ({hatch_sort}) must render behind line 2 ({line2_sort})");
    }

    #[test]
    fn draworder_above_reference_puts_hatch_in_front_of_object() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_ref = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        app.tabs[i].scene.replace_selection(std::iter::once(h_hatch).collect());
        let cmd = format!("DRAWORDER ABOVE {:x}", h_ref.value());
        let _ = app.run_command_line(&cmd);

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let ref_sort = entries.get(&h_ref.value()).copied().unwrap_or(h_ref.value());

        assert!(hatch_sort > ref_sort, "hatch ({hatch_sort}) must be above reference object ({ref_sort})");
    }

    #[test]
    fn draworder_under_reference_puts_hatch_behind_object() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let h_ref = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        app.tabs[i].scene.replace_selection(std::iter::once(h_hatch).collect());
        let cmd = format!("DRAWORDER UNDER {:x}", h_ref.value());
        let _ = app.run_command_line(&cmd);

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let ref_sort = entries.get(&h_ref.value()).copied().unwrap_or(h_ref.value());

        assert!(hatch_sort < ref_sort, "hatch ({hatch_sort}) must be under reference object ({ref_sort})");
    }

    #[test]
    fn draworder_interactive_shortcuts_f_b_a_u() {
        use crate::command::CadCommand;

        let mut app = fresh_app();
        let i = app.active_tab;
        let _h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        // 1. Shortcut 'F' -> Front
        let mut cmd = DrawOrderCommand::new(vec![h_hatch]);
        assert_eq!(cmd.wants_text_input(), true);
        let res = cmd.on_text_input("F");
        match res {
            Some(crate::command::CmdResult::Relaunch(c, handles)) => {
                assert_eq!(c, "DRAWORDER FRONT");
                assert_eq!(handles, vec![h_hatch]);
            }
            _ => panic!("Expected Relaunch for shortcut F"),
        }

        // 2. Shortcut 'B' -> Back
        let mut cmd = DrawOrderCommand::new(vec![h_hatch]);
        let res = cmd.on_text_input("B");
        match res {
            Some(crate::command::CmdResult::Relaunch(c, handles)) => {
                assert_eq!(c, "DRAWORDER BACK");
                assert_eq!(handles, vec![h_hatch]);
            }
            _ => panic!("Expected Relaunch for shortcut B"),
        }

        // 3. Shortcut 'A' -> Advances to PickReference (above)
        let mut cmd = DrawOrderCommand::new(vec![h_hatch]);
        assert!(!cmd.needs_entity_pick());
        let res = cmd.on_text_input("A");
        assert!(matches!(res, Some(crate::command::CmdResult::NeedPoint)));
        assert!(cmd.needs_entity_pick(), "Needs entity pick after choosing 'A'");

        // 4. Shortcut 'U' -> Advances to PickReference (under)
        let mut cmd = DrawOrderCommand::new(vec![h_hatch]);
        let res = cmd.on_text_input("U");
        assert!(matches!(res, Some(crate::command::CmdResult::NeedPoint)));
        assert!(cmd.needs_entity_pick(), "Needs entity pick after choosing 'U'");
    }

    #[test]
    fn draworder_interactive_above_under_viewport_entity_pick() {
        use crate::command::CadCommand;

        let mut app = fresh_app();
        let i = app.active_tab;
        let h_ref = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        // Above with viewport pick
        let mut cmd = DrawOrderCommand::new(vec![h_hatch]);
        let _ = cmd.on_text_input("A");
        assert!(cmd.needs_entity_pick());
        let pick_res = cmd.on_entity_pick(h_ref, glam::DVec3::ZERO);
        match pick_res {
            crate::command::CmdResult::Relaunch(relaunch_cmd, handles) => {
                assert_eq!(relaunch_cmd, format!("DRAWORDER A {:x}", h_ref.value()));
                assert_eq!(handles, vec![h_hatch]);
                app.tabs[i].scene.replace_selection(handles.into_iter().collect());
                let _ = app.run_command_line(&relaunch_cmd);
            }
            _ => panic!("Expected Relaunch from entity pick"),
        }

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let ref_sort = entries.get(&h_ref.value()).copied().unwrap_or(h_ref.value());
        assert!(hatch_sort > ref_sort, "Hatch must be above reference after viewport pick");

        // Under with viewport pick
        let mut cmd = DrawOrderCommand::new(vec![h_hatch]);
        let _ = cmd.on_text_input("U");
        assert!(cmd.needs_entity_pick());
        let pick_res = cmd.on_entity_pick(h_ref, glam::DVec3::ZERO);
        match pick_res {
            crate::command::CmdResult::Relaunch(relaunch_cmd, handles) => {
                assert_eq!(relaunch_cmd, format!("DRAWORDER U {:x}", h_ref.value()));
                assert_eq!(handles, vec![h_hatch]);
                app.tabs[i].scene.replace_selection(handles.into_iter().collect());
                let _ = app.run_command_line(&relaunch_cmd);
            }
            _ => panic!("Expected Relaunch from entity pick"),
        }

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let ref_sort = entries.get(&h_ref.value()).copied().unwrap_or(h_ref.value());
        assert!(hatch_sort < ref_sort, "Hatch must be under reference after viewport pick");
    }

    #[test]
    fn draworder_interactive_typed_hex_handle() {
        use crate::command::CadCommand;

        let mut app = fresh_app();
        let i = app.active_tab;
        let h_ref = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        let mut cmd = DrawOrderCommand::new(vec![h_hatch]);
        let _ = cmd.on_text_input("Above");
        let hex_input = format!("0x{:x}", h_ref.value());
        let typed_res = cmd.on_text_input(&hex_input);
        match typed_res {
            Some(crate::command::CmdResult::Relaunch(relaunch_cmd, handles)) => {
                assert_eq!(relaunch_cmd, format!("DRAWORDER A {:x}", h_ref.value()));
                assert_eq!(handles, vec![h_hatch]);
                app.tabs[i].scene.replace_selection(handles.into_iter().collect());
                let _ = app.run_command_line(&relaunch_cmd);
            }
            _ => panic!("Expected Relaunch from typed hex handle"),
        }

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let ref_sort = entries.get(&h_ref.value()).copied().unwrap_or(h_ref.value());
        assert!(hatch_sort > ref_sort);
    }

    #[test]
    fn draworder_interactive_without_preselection() {
        use crate::command::CadCommand;

        let mut app = fresh_app();
        let i = app.active_tab;
        let h_line = app.tabs[i].scene.add_entity_clone(EntityType::Line(Default::default()));
        let h_hatch = app.tabs[i].scene.add_entity_clone(EntityType::Hatch(Default::default()));

        // Start command with no pre-selection
        let mut cmd = DrawOrderCommand::new(vec![]);
        assert!(cmd.is_selection_gathering());
        assert!(!cmd.wants_text_input());

        // Gather selection
        let _ = cmd.on_selection_complete(vec![h_hatch]);
        let enter_res = cmd.on_enter();
        assert!(matches!(enter_res, crate::command::CmdResult::NeedPoint));
        assert!(!cmd.is_selection_gathering());
        assert!(cmd.wants_text_input());

        // Choose verb Front
        let res = cmd.on_text_input("F");
        match res {
            Some(crate::command::CmdResult::Relaunch(relaunch_cmd, handles)) => {
                assert_eq!(relaunch_cmd, "DRAWORDER FRONT");
                assert_eq!(handles, vec![h_hatch]);
                app.tabs[i].scene.replace_selection(handles.into_iter().collect());
                let _ = app.run_command_line(&relaunch_cmd);
            }
            _ => panic!("Expected Relaunch"),
        }

        let entries = effective_sort_map(&app);
        let hatch_sort = entries.get(&h_hatch.value()).copied().unwrap_or(h_hatch.value());
        let line_sort = entries.get(&h_line.value()).copied().unwrap_or(h_line.value());
        assert!(hatch_sort > line_sort);
    }
}
