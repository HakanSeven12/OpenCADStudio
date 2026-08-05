use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_display(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // Interactive pan: left-drag pans the view until Esc. The only pan
            // path when there is no middle mouse button (trackpad / web).
            "PAN" => {
                self.tabs[i].pan_mode = true;
                self.clear_navigation_hover(i);
                self.command_line
                    .push_output(crate::t!("PAN: drag with the left mouse button. Press Esc to exit.").as_ref());
            }

            // ── TABLE cell editing ─────────────────────────────────────────────
            // TABLE CELL <row> <col> <text> — set text for a cell in the selected Table
            cmd if cmd.starts_with("TABLE ") => {
                let rest = cmd.trim_start_matches("TABLE").trim();
                let sub_up = rest.split_whitespace().next().unwrap_or("").to_uppercase();
                if sub_up == "CELL" {
                    let parts: Vec<&str> = rest.splitn(4, char::is_whitespace).collect();
                    // parts: ["CELL", "<row>", "<col>", "<text>"]
                    let row_res = parts.get(1).and_then(|s| s.parse::<usize>().ok());
                    let col_res = parts.get(2).and_then(|s| s.parse::<usize>().ok());
                    let text = parts.get(3).copied().unwrap_or("");
                    match (row_res, col_res) {
                        (Some(row), Some(col)) => {
                            let selected_handles: Vec<acadrust::Handle> = self.tabs[i]
                                .scene
                                .selected_entities()
                                .iter()
                                .map(|(h, _)| *h)
                                .collect();
                            let mut found = false;
                            for sh in &selected_handles {
                                if let Some(acadrust::EntityType::Table(tbl)) = self.tabs[i]
                                    .scene
                                    .document
                                    .entities_mut()
                                    .find(|e| e.common().handle == *sh)
                                {
                                    if tbl.set_cell_text(row, col, text) {
                                        found = true;
                                    }
                                }
                            }
                            if found {
                                self.push_undo_snapshot(i, "TABLE CELL");
                                self.tabs[i].dirty = true;
                                self.command_line.push_output(crate::tf!(
                                    "TABLE CELL: set [{row},{col}] = \"{text}\"."
                                ).as_ref());
                            } else {
                                self.command_line.push_error(
                                    crate::t!("TABLE CELL: select a Table entity first, or row/col out of range.").as_ref()
                                );
                            }
                        }
                        _ => {
                            self.command_line
                                .push_info(crate::t!("Usage: TABLE CELL <row> <col> <text>").as_ref());
                        }
                    }
                } else {
                    self.command_line.push_info(
                        "Usage: TABLE  (creates new table)  or  TABLE CELL <row> <col> <text>",
                    );
                }
            }

            // ── UCSICON — toggle UCS icon visibility on all viewports ────────────
            // UCSICON ON       — show UCS icon in all viewports
            // UCSICON OFF      — hide UCS icon in all viewports
            // UCSICON NOORIGIN — show icon but not at origin (show at corner)
            // UCSICON ORIGIN   — show icon at UCS origin
            "UCSICON" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "UCSICON",
                    "UCSICON  [On / Off / NoOrigin / Origin]:",
                    vec![
                        ("On", "ON", None),
                        ("Off", "OFF", None),
                        ("NoOrigin", "NOORIGIN", None),
                        ("Origin", "ORIGIN", None),
                    ],
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("UCSICON ") => {
                let sub = cmd.split_whitespace().nth(1).unwrap_or("").to_uppercase();
                match sub.as_str() {
                    "ON" | "OFF" | "NOORIGIN" | "ORIGIN" => {
                        self.push_undo_snapshot(i, "UCSICON");
                        let visible = sub != "OFF";
                        let at_origin = sub == "ORIGIN";
                        // Update model-space icon flags.
                        self.show_ucs_icon = visible;
                        self.ribbon.set_ucs_icon(visible);
                        if sub == "NOORIGIN" || sub == "ORIGIN" {
                            self.ucs_icon_at_origin = at_origin;
                        }
                        let mut count = 0usize;
                        for entity in self.tabs[i].scene.document.entities_mut() {
                            if let acadrust::EntityType::Viewport(vp) = entity {
                                vp.status.ucs_icon_visible = visible;
                                if sub == "NOORIGIN" || sub == "ORIGIN" {
                                    vp.status.ucs_icon_at_origin = at_origin;
                                }
                                count += 1;
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::tf!(
                            "UCSICON {sub}: updated {count} viewport(s) + model space."
                        ).as_ref());
                    }
                    "" => {
                        // Bare UCSICON toggles visibility.
                        self.push_undo_snapshot(i, "UCSICON");
                        let visible = !self.show_ucs_icon;
                        self.show_ucs_icon = visible;
                        self.ribbon.set_ucs_icon(visible);
                        for entity in self.tabs[i].scene.document.entities_mut() {
                            if let acadrust::EntityType::Viewport(vp) = entity {
                                vp.status.ucs_icon_visible = visible;
                            }
                        }
                        self.tabs[i].dirty = true;
                        let state = if visible { "ON" } else { "OFF" };
                        self.command_line.push_output(crate::tf!("UCSICON {state}").as_ref());
                    }
                    _ => {
                        self.command_line
                            .push_info(crate::t!("Usage: UCSICON ON | OFF | NOORIGIN | ORIGIN").as_ref());
                    }
                }
            }

            // ── NAVVCUBE — toggle ViewCube visibility ────────────────────────────
            "NAVVCUBE" => {
                return Some(Task::done(Message::ToggleViewCube));
            }

            // ── LIMITS — drawing/grid boundary for the active space ─────────────
            "LIMITS" => {
                use crate::modules::view::limits::LimitsCommand;
                let (min, max) = self.tabs[i]
                    .scene
                    .current_drawing_limits()
                    .unwrap_or((glam::DVec2::ZERO, glam::DVec2::new(12.0, 9.0)));
                let command = LimitsCommand::new(min, max);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "LIMITS ON" | "LIMITS OFF" => {
                let enabled = cmd.ends_with("ON");
                if self.tabs[i].scene.drawing_limit_check_enabled() != enabled {
                    self.push_undo_snapshot(i, "LIMITS");
                    self.tabs[i].scene.set_drawing_limit_check(enabled);
                    self.tabs[i].dirty = true;
                }
                self.command_line.push_output(if enabled {
                    "Limits checking ON."
                } else {
                    "Limits checking OFF."
                });
            }
            cmd if cmd.starts_with("LIMITS SET ") => {
                let tokens: Vec<&str> = cmd["LIMITS SET ".len()..].split_whitespace().collect();
                let values: Result<Vec<f64>, _> =
                    tokens.iter().map(|value| value.parse()).collect();
                let Ok(values) = values else {
                    self.command_line
                        .push_error(crate::t!("LIMITS: four numeric coordinates required.").as_ref());
                    return Some(Task::none());
                };
                if tokens.len() != 4 || !values.iter().all(|value| value.is_finite()) {
                    self.command_line
                        .push_error(crate::t!("LIMITS: four finite numeric coordinates required.").as_ref());
                } else {
                    let first = glam::DVec2::new(values[0], values[1]);
                    let opposite = glam::DVec2::new(values[2], values[3]);
                    let min = first.min(opposite);
                    let max = first.max(opposite);
                    if min.x == max.x || min.y == max.y {
                        self.command_line
                            .push_error(crate::t!("LIMITS: corners must define a non-zero area.").as_ref());
                    } else {
                        self.push_undo_snapshot(i, "LIMITS");
                        self.tabs[i].scene.set_current_drawing_limits(min, max);
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::tf!(
                            "Drawing limits: {:.4},{:.4} to {:.4},{:.4}.",
                            min.x, min.y, max.x, max.y
                        ).as_ref());
                    }
                }
            }

            // ── PROPERTIES — toggle Properties panel visibility ──────────────────
            "PROPERTIES" | "PROPS" => {
                return Some(Task::done(Message::ToggleProperties));
            }

            // ── FILETAB — toggle file/document tabs ──────────────────────────────
            "FILETAB" => {
                return Some(Task::done(Message::ToggleFileTabs));
            }

            // ── LAYOUTTAB — toggle layout/paper-space tabs ───────────────────────
            "LAYOUTTAB" => {
                return Some(Task::done(Message::ToggleLayoutTabs));
            }

            // ── Drafting aids — same toggles the status-bar pills drive, also
            //    reachable by name from the command line. ─────────────────────────
            // GRID — show / hide the reference grid.
            "GRID" => {
                return Some(Task::done(Message::ToggleGrid));
            }
            // SNAP — toggle cursor snapping to the grid.
            "SNAP" => {
                return Some(Task::done(Message::ToggleGridSnap));
            }
            // POLAR — toggle polar tracking.
            "POLAR" => {
                return Some(Task::done(Message::TogglePolar));
            }
            // DSETTINGS / OSNAP — open the drafting-settings popup, which is OCS's
            // settings surface (the persisted DYN/ORTHO/POLAR/OSNAP prefs).
            "DSETTINGS" | "OSNAP" => {
                return Some(Task::done(Message::ToggleSnapPopup));
            }
            // UNITS — open the drawing-units picker (linear / angular format).
            "UNITS" | "DDUNITS" => {
                return Some(Task::done(Message::ToggleUnitsPopup));
            }

            // ── CLEANSCREEN — collapse the surrounding panels for a full canvas ──
            "CLEANSCREEN" => {
                return Some(Task::done(Message::ToggleCleanScreen));
            }
            // ── QUICKPROPERTIES — toggle the floating quick-properties readout ───
            "QUICKPROPERTIES" => {
                return Some(Task::done(Message::ToggleQuickProperties));
            }

            // ── TOOLPALETTES — not yet implemented ───────────────────────────────
            "TOOLPALETTES" => {
                self.command_line
                    .push_info(crate::t!("TOOLPALETTES: Tool Palettes not yet implemented.").as_ref());
            }

            // ── SHEETSET — not yet implemented ───────────────────────────────────
            "SHEETSET" => {
                self.command_line
                    .push_info(crate::t!("SHEETSET: Sheet Set Manager not yet implemented.").as_ref());
            }

            // ── XDATA — read/write extended entity data ──────────────────────────
            // XDATA LIST             — show all xdata records on selected entities
            // XDATA SET <app> <str>  — append a string xdata value for <app>
            // XDATA CLEAR            — remove all xdata from selected entities
            // XDATA CLEAR <app>      — remove xdata for a specific application
            "XDATA" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "XDATA",
                    "XDATA  [List / Clear]  (SET <app> <value> by typing):",
                    vec![("List", "LIST", None), ("Clear", "CLEAR", None)],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("XDATA ") => {
                use acadrust::xdata::{ExtendedDataRecord, XDataValue};
                let rest = cmd.trim_start_matches("XDATA").trim();
                let parts: Vec<&str> = rest.splitn(3, char::is_whitespace).collect();
                let sub = parts.first().map(|s| s.to_uppercase()).unwrap_or_default();
                let selected_handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                if selected_handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("XDATA: select entities first.").as_ref());
                } else {
                    match sub.as_str() {
                        "LIST" | "" => {
                            for sh in &selected_handles {
                                if let Some(entity) = self.tabs[i].scene.document.get_entity(*sh) {
                                    let xd = &entity.common().extended_data;
                                    if xd.is_empty() {
                                        self.command_line
                                            .push_output(crate::tf!("  {:x}: no xdata.", sh.value()).as_ref());
                                    } else {
                                        for rec in xd.records() {
                                            self.command_line.push_output(crate::tf!(
                                                "  {:x} [{}]: {} value(s)",
                                                sh.value(),
                                                rec.application_name,
                                                rec.values.len()
                                            ).as_ref());
                                            for v in &rec.values {
                                                self.command_line
                                                    .push_output(crate::tf!("    {:?}", v).as_ref());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "SET" => {
                            let app = parts.get(1).copied().unwrap_or("OpenCADStudio");
                            let val = parts.get(2).copied().unwrap_or("");
                            self.push_undo_snapshot(i, "XDATA SET");
                            for sh in &selected_handles {
                                if let Some(entity) =
                                    self.tabs[i].scene.document.get_entity_mut(*sh)
                                {
                                    let mut rec = ExtendedDataRecord::new(app);
                                    rec.add_value(XDataValue::String(val.to_string()));
                                    entity.common_mut().extended_data.add_record(rec);
                                }
                            }
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(crate::tf!(
                                "XDATA: set [{app}] = \"{val}\" on {} entity/entities.",
                                selected_handles.len()
                            ).as_ref());
                        }
                        "CLEAR" => {
                            let app_filter = parts.get(1).copied();
                            self.push_undo_snapshot(i, "XDATA CLEAR");
                            for sh in &selected_handles {
                                if let Some(entity) =
                                    self.tabs[i].scene.document.get_entity_mut(*sh)
                                {
                                    let xd = &mut entity.common_mut().extended_data;
                                    if let Some(app) = app_filter {
                                        // Rebuild without the matching app.
                                        let kept: Vec<_> = xd
                                            .records()
                                            .iter()
                                            .filter(|r| r.application_name != app)
                                            .cloned()
                                            .collect();
                                        xd.clear();
                                        for r in kept {
                                            xd.add_record(r);
                                        }
                                    } else {
                                        xd.clear();
                                    }
                                }
                            }
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(crate::t!("XDATA: cleared.").as_ref());
                        }
                        _ => {
                            self.command_line
                                .push_info(crate::t!("Usage: XDATA LIST | SET <app> <value> | CLEAR [app]").as_ref());
                        }
                    }
                }
            }

            // BOX / SPHERE / CYLINDER / CONE / WEDGE / TORUS are handled by the
            // Model-tab primitive command above (with truck boolean caching).

            // ── EXTRUDE ────────────────────────────────────────────────────
            // PRESSPULL on a closed boundary creates a solid by extruding it to a
            // height — the same operation as EXTRUDE. THICKEN turns a closed planar
            // profile into a solid of the given thickness, which is also an extrude.
            "EXTRUDE" | "PRESSPULL" | "THICKEN" => {
                use crate::modules::insert::solid3d_cmds::ExtrudeCommand;
                // If a single entity is already selected, skip the pick step.
                let selected: Vec<_> = self.tabs[i].scene.selected_entities().into_iter().collect();
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                if selected.len() == 1 {
                    let handle = selected[0].0;
                    let mut cmd = ExtrudeCommand::new(color);
                    cmd.on_entity_pick(handle, glam::DVec3::ZERO);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let cmd = ExtrudeCommand::new(color);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            // ── REVOLVE ────────────────────────────────────────────────────
            "REVOLVE" => {
                use crate::modules::insert::solid3d_cmds::RevolveCommand;
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                let cmd = RevolveCommand::new(color);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // ── SWEEP ──────────────────────────────────────────────────────
            "SWEEP" => {
                use crate::modules::insert::solid3d_cmds::SweepCommand;
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                let cmd = SweepCommand::new(color);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // ── LOFT ───────────────────────────────────────────────────────
            "LOFT" => {
                use crate::modules::insert::solid3d_cmds::LoftCommand;
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                let cmd = LoftCommand::new(color);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // ── OBJ import ───────────────────────────────────────────────
            "IMPORTOBJ" | "OBJIMPORT" => {
                return Some(Task::done(Message::ObjImport));
            }

            // ── STL export ────────────────────────────────────────────────
            "STLOUT" | "EXPORTSTL" => {
                return Some(Task::done(Message::StlExport));
            }

            // STEPOUT — export 3D meshes to STEP AP203 format
            "STEPOUT" | "EXPORTSTEP" | "STPOUT" => {
                return Some(Task::done(Message::StepExport));
            }

            // ── Plot Style Editor GUI ─────────────────────────────────────
            "PLOTSTYLEPANEL" | "PLOTSTYLEEDITOR" | "STYLESMANAGER" => {
                return Some(Task::done(Message::PlotStylePanelOpen));
            }

            // ── Plot / Page Setup ──────────────────────────────────────────
            // PLOT / PRINT open the full plot dialog (printer, paper, scale,
            // options); EXPORT / EXPORTPDF stay a direct PDF export.
            "PLOT" | "PRINT" => {
                return Some(Task::done(Message::PlotDialogOpen));
            }
            "PRINTALL" => {
                return Some(Task::done(Message::PrintAllOpen));
            }
            "EXPORT" | "EXPORTPDF" => {
                return Some(Task::done(Message::PlotExport));
            }
            // PLOTSTYLE — load or clear CTB/STB plot style table
            cmd if cmd == "PLOTSTYLE" || cmd.starts_with("PLOTSTYLE ") => {
                let sub = cmd
                    .split_once(' ')
                    .map(|(_, r)| r.trim().to_uppercase())
                    .unwrap_or_default();
                match sub.as_str() {
                    "CLEAR" | "NONE" => {
                        return Some(Task::done(Message::PlotStyleClear));
                    }
                    "" | "LOAD" => {
                        let active = self
                            .active_plot_style
                            .as_ref()
                            .map(|t| format!("Active: {}", t.name))
                            .unwrap_or_else(|| "No plot style loaded.".into());
                        self.command_line.push_info(&active);
                        return Some(Task::done(Message::PlotStyleLoad));
                    }
                    "?" | "STATUS" => {
                        let msg = self
                            .active_plot_style
                            .as_ref()
                            .map(|t| {
                                format!(
                                    "Plot style: {}  ({} color overrides)",
                                    t.name,
                                    t.aci_entries.iter().filter(|e| e.color.is_some()).count()
                                )
                            })
                            .unwrap_or_else(|| "No plot style table loaded.".into());
                        self.command_line.push_output(&msg);
                    }
                    _ => {
                        self.command_line
                            .push_error(crate::t!("Usage: PLOTSTYLE [LOAD | CLEAR | STATUS]").as_ref());
                    }
                }
            }
            // UNDERLAY — edit properties of selected PDF/DWF/DGN underlay entities.
            // Usage:
            //   UNDERLAY FADE <0-80>
            //   UNDERLAY CONTRAST <0-100>
            //   UNDERLAY ON | OFF
            //   UNDERLAY CLIP ON | OFF
            //   UNDERLAY MONO ON | OFF
            "UNDERLAY" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "UNDERLAY",
                    "UNDERLAY  [Fade / Contrast / On / Off / Mono / Clip]:",
                    vec![
                        ("Fade", "FADE", Some("UNDERLAY  fade 0-100:")),
                        ("Contrast", "CONTRAST", Some("UNDERLAY  contrast 0-100:")),
                        ("On", "ON", None),
                        ("Off", "OFF", None),
                        ("Mono", "MONO", Some("UNDERLAY MONO  [On / Off]:")),
                        ("Clip", "CLIP", Some("UNDERLAY CLIP  [On / Off]:")),
                    ],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("UNDERLAY ") => {
                let sub = cmd
                    .split_once(' ')
                    .map(|(_, r)| r.trim().to_uppercase())
                    .unwrap_or_default();
                let handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("UNDERLAY: select underlay entities first.").as_ref());
                } else {
                    let parts: Vec<&str> = sub.splitn(2, char::is_whitespace).collect();
                    let action = parts.first().copied().unwrap_or("");
                    let arg = parts.get(1).copied().unwrap_or("").trim();
                    let mut changed = 0usize;
                    self.push_undo_snapshot(i, "UNDERLAY");
                    for h in &handles {
                        if let Some(acadrust::EntityType::Underlay(ul)) = self.tabs[i]
                            .scene
                            .document
                            .entities_mut()
                            .find(|e| e.common().handle == *h)
                        {
                            match action {
                                "FADE" => {
                                    if let Ok(v) = arg.parse::<u8>() {
                                        ul.set_fade(v);
                                        changed += 1;
                                    }
                                }
                                "CONTRAST" => {
                                    if let Ok(v) = arg.parse::<u8>() {
                                        ul.set_contrast(v);
                                        changed += 1;
                                    }
                                }
                                "ON" => {
                                    ul.set_on(true);
                                    changed += 1;
                                }
                                "OFF" => {
                                    ul.set_on(false);
                                    changed += 1;
                                }
                                "CLIP" => match arg {
                                    "ON" => {
                                        ul.flags |=
                                            acadrust::entities::UnderlayDisplayFlags::CLIPPING;
                                        changed += 1;
                                    }
                                    "OFF" => {
                                        ul.clear_clip();
                                        changed += 1;
                                    }
                                    _ => {}
                                },
                                "MONO" => match arg {
                                    "ON" => {
                                        ul.set_monochrome(true);
                                        changed += 1;
                                    }
                                    "OFF" => {
                                        ul.set_monochrome(false);
                                        changed += 1;
                                    }
                                    _ => {}
                                },
                                _ => {
                                    // No sub-command: print status.
                                    self.command_line.push_output(crate::tf!(
                                        "Underlay {:x}: fade={}, contrast={}, on={}, clip={}, mono={}",
                                        h.value(),
                                        ul.fade,
                                        ul.contrast,
                                        ul.is_on(),
                                        ul.is_clipping(),
                                        ul.is_monochrome(),
                                    ).as_ref());
                                }
                            }
                        }
                    }
                    if changed > 0 {
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_info(crate::tf!("Updated {changed} underlay(s).").as_ref());
                    } else if !action.is_empty() {
                        self.command_line.push_error(
                            crate::t!("Usage: UNDERLAY [FADE <n>|CONTRAST <n>|ON|OFF|CLIP ON|OFF|MONO ON|OFF]").as_ref()
                        );
                    }
                }
            }

            // PAGESETUP is folded into the unified plot dialog.
            "PAGESETUP" => {
                return Some(Task::done(Message::PlotDialogOpen));
            }

            // ── Recognized commands whose full implementation is pending ─────────
            // These verbs are surfaced by the ribbon / menus but their feature is
            // still being built. Acknowledge them with an honest status so the
            // button responds instead of reporting an unknown command; each is
            // replaced by its real handler as the feature lands.
            // OBJECTSCALE ADD — add the active scale representation to every
            // selected object that supports per-scale context data.
            "OBJECTSCALE ADD" => {
                let handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("OBJECTSCALE: select objects first.").as_ref());
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "OBJECTSCALE");
                let Some(scale) = self.tabs[i].scene.creation_annotation_scale_handle() else {
                    self.command_line
                        .push_error(crate::t!("OBJECTSCALE: the active annotation scale is unavailable.").as_ref());
                    return Some(Task::none());
                };
                let mut n = 0usize;
                for h in &handles {
                    if crate::scene::annotative::create_annotation_context(
                        &mut self.tabs[i].scene.document,
                        *h,
                        scale,
                    ) {
                        crate::scene::annotative::set_entity_annotative(
                            &mut self.tabs[i].scene.document,
                            *h,
                            true,
                        );
                        n += 1;
                    }
                }
                let changes: Vec<_> = handles
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities(&changes);
                self.tabs[i].dirty = true;
                self.command_line.push_output(crate::tf!(
                    "OBJECTSCALE: added the active scale to {n} object(s)."
                ).as_ref());
                return Some(Task::none());
            }

            // HYPERLINK <url> — attach a hyperlink to the selected objects, stored
            // in the standard PE_URL XData record so it round-trips in the file.
            "HYPERLINK" => {
                use crate::command::SelectThenValueCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenValueCommand::new(
                    "HYPERLINK",
                    "HYPERLINK  URL to attach:",
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("HYPERLINK ") => {
                use acadrust::xdata::{ExtendedDataRecord, XDataValue};
                let url = cmd.strip_prefix("HYPERLINK").unwrap_or("").trim().to_string();
                if url.is_empty() {
                    self.command_line.push_info(crate::t!("Usage: HYPERLINK <url>   (select objects first)").as_ref());
                    return Some(Task::none());
                }
                let handles: Vec<acadrust::Handle> =
                    self.tabs[i].scene.selected_entities().iter().map(|(h, _)| *h).collect();
                if handles.is_empty() {
                    self.command_line.push_error(crate::t!("HYPERLINK: select objects first.").as_ref());
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "HYPERLINK");
                let mut n = 0usize;
                for h in &handles {
                    if let Some(e) = self.tabs[i].scene.document.get_entity_mut(*h) {
                        let xd = &mut e.common_mut().extended_data;
                        let mut rec = ExtendedDataRecord::new("PE_URL");
                        rec.add_value(XDataValue::String(url.clone()));
                        xd.add_record(rec);
                        n += 1;
                    }
                }
                self.tabs[i].dirty = true;
                self.command_line
                    .push_output(crate::tf!("HYPERLINK: attached to {n} object(s).").as_ref());
                return Some(Task::none());
            }

            // ADJUST — set brightness / contrast / fade on selected raster images
            //   ADJUST BRIGHTNESS|CONTRAST|FADE <0-100>
            "ADJUST" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "ADJUST",
                    "ADJUST  [Brightness / Contrast / Fade]:",
                    vec![
                        ("Brightness", "BRIGHTNESS", Some("ADJUST  brightness 0-100:")),
                        ("Contrast", "CONTRAST", Some("ADJUST  contrast 0-100:")),
                        ("Fade", "FADE", Some("ADJUST  fade 0-100:")),
                    ],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ADJUST ") => {
                let rest = cmd.trim_start_matches("ADJUST").trim();
                let parts: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
                let action = parts.first().map(|s| s.to_uppercase()).unwrap_or_default();
                let arg = parts.get(1).copied().unwrap_or("").trim();
                let handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("ADJUST: select raster image(s) first.").as_ref());
                } else if action.is_empty() {
                    self.command_line
                        .push_info(crate::t!("Usage: ADJUST BRIGHTNESS|CONTRAST|FADE <0-100>").as_ref());
                } else if let Ok(v) = arg.parse::<u8>() {
                    let v = v.min(100);
                    self.push_undo_snapshot(i, "ADJUST");
                    let mut changed = 0usize;
                    let mut changed_handles = Vec::new();
                    for h in &handles {
                        if let Some(acadrust::EntityType::RasterImage(img)) = self.tabs[i]
                            .scene
                            .document
                            .entities_mut()
                            .find(|e| e.common().handle == *h)
                        {
                            match action.as_str() {
                                "BRIGHTNESS" => {
                                    img.brightness = v;
                                    changed += 1;
                                    changed_handles.push(*h);
                                }
                                "CONTRAST" => {
                                    img.contrast = v;
                                    changed += 1;
                                    changed_handles.push(*h);
                                }
                                "FADE" => {
                                    img.fade = v;
                                    changed += 1;
                                    changed_handles.push(*h);
                                }
                                _ => {}
                            }
                        }
                    }
                    if changed > 0 {
                        self.tabs[i].dirty = true;
                        for &handle in &changed_handles {
                            self.tabs[i].scene.reseed_derived_caches(handle);
                        }
                        let changes: Vec<_> = changed_handles
                            .into_iter()
                            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                            .collect();
                        self.tabs[i].scene.bump_entities(&changes);
                        self.command_line
                            .push_output(crate::tf!("ADJUST: {action} = {v} on {changed} image(s).").as_ref());
                    } else {
                        self.command_line.push_error(
                            "ADJUST: no raster images selected, or unknown property (use BRIGHTNESS|CONTRAST|FADE).",
                        );
                    }
                } else {
                    self.command_line.push_error(crate::t!("ADJUST: value must be 0-100.").as_ref());
                }
            }

            // ANNOSCALE / CANNOSCALE <ratio> — set the current annotation scale
            // (e.g. 1:50, 2:1, or a plain factor). Drives annotative-object size
            // in model space and is written to the drawing header.
            "ANNOSCALE" | "CANNOSCALE" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "ANNOSCALE",
                    "ANNOSCALE  new annotation scale  (e.g. 1:50, 2:1, or a factor):",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            "ANNOALLVISIBLE" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "ANNOALLVISIBLE",
                    "ANNOALLVISIBLE  new value [0/1]:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ANNOALLVISIBLE ") => {
                let value = cmd.split_whitespace().nth(1).unwrap_or("");
                match value {
                    "0" | "OFF" | "FALSE" => {
                        self.tabs[i].scene.set_annotation_all_visible(false);
                        self.tabs[i].dirty = true;
                    }
                    "1" | "ON" | "TRUE" => {
                        self.tabs[i].scene.set_annotation_all_visible(true);
                        self.tabs[i].dirty = true;
                    }
                    _ => self
                        .command_line
                        .push_error(crate::t!("ANNOALLVISIBLE: enter 0 or 1.").as_ref()),
                }
            }
            "ANNOAUTOSCALE" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "ANNOAUTOSCALE",
                    "ANNOAUTOSCALE  new value [-4..4]:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ANNOAUTOSCALE ") => {
                let value = cmd.split_whitespace().nth(1).unwrap_or("");
                match value.parse::<i8>() {
                    Ok(mode @ -4..=4) => self.annotation_auto_scale = mode,
                    _ => self.command_line.push_error(
                        "ANNOAUTOSCALE: enter an integer from -4 through 4.",
                    ),
                }
            }
            "ANNOUPDATE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(handle, _)| *handle)
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("ANNOUPDATE: select annotation objects first.").as_ref());
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "ANNOUPDATE");
                let scale = self.tabs[i].scene.creation_annotation_scale_handle();
                let mut updated = 0usize;
                for handle in &handles {
                    if crate::scene::annotative::update_entity_from_annotation_style(
                        &mut self.tabs[i].scene.document,
                        *handle,
                        scale,
                    ) {
                        updated += 1;
                    }
                }
                if updated > 0 {
                    let changes: Vec<_> = handles
                        .into_iter()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.tabs[i].dirty = true;
                }
                self.command_line
                    .push_output(crate::tf!("ANNOUPDATE: updated {updated} object(s).").as_ref());
                return Some(Task::none());
            }
            cmd if cmd.starts_with("ANNOSCALE ") || cmd.starts_with("CANNOSCALE ") => {
                let arg = cmd
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if arg.is_empty() {
                    let name = self.tabs[i]
                        .scene
                        .document
                        .header
                        .current_annotation_scale
                        .clone();
                    self.command_line
                        .push_output(crate::tf!("Current annotation scale: {name}").as_ref());
                    return Some(Task::none());
                }
                let previous = self.tabs[i].scene.displayed_annotation_scale_handle();
                match self.tabs[i].scene.set_annotation_scale_named(&arg) {
                    Some(handle) => {
                        if self.annotation_auto_scale > 0 {
                            self.tabs[i].scene.add_annotation_scale_to_objects(
                                handle,
                                previous,
                                self.annotation_auto_scale as u8,
                            );
                        }
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("Annotation scale: {arg}").as_ref());
                    }
                    None => self
                        .command_line
                        .push_error(crate::t!("Usage: ANNOSCALE <ratio>  e.g. 1:50, 2:1, or a factor").as_ref()),
                }
            }

            // SCALELISTEDIT — list / add / delete the drawing's annotation scales.
            //   SCALELISTEDIT              list
            //   SCALELISTEDIT ADD 1:50     add (name is a paper:drawing ratio)
            //   SCALELISTEDIT DELETE 1:50  remove (not the current scale)
            "SCALELISTEDIT" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "SCALELISTEDIT",
                    "SCALELISTEDIT  [Add / Delete]:",
                    vec![
                        ("Add", "ADD", Some("SCALELISTEDIT ADD  new scale ratio (e.g. 1:50):")),
                        ("Delete", "DELETE", Some("SCALELISTEDIT DELETE  scale ratio to remove:")),
                    ],
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("SCALELISTEDIT ") => {
                let rest = cmd.trim_start_matches("SCALELISTEDIT").trim();
                let mut parts = rest.splitn(2, char::is_whitespace);
                let sub = parts.next().unwrap_or("").to_uppercase();
                let arg = parts.next().unwrap_or("").trim();
                match sub.as_str() {
                    "ADD" => match arg.split_once(':') {
                        Some((p, d)) => match (p.trim().parse::<f64>(), d.trim().parse::<f64>()) {
                            (Ok(paper), Ok(drawing)) if paper > 0.0 && drawing > 0.0 => {
                                self.push_undo_snapshot(i, "SCALELISTEDIT");
                                if self.tabs[i].scene.add_scale(arg, paper, drawing) {
                                    self.tabs[i].dirty = true;
                                    self.command_line
                                        .push_output(crate::tf!("Added annotation scale {arg}.").as_ref());
                                } else {
                                    self.command_line
                                        .push_info(crate::tf!("Scale {arg} already exists.").as_ref());
                                }
                            }
                            _ => self
                                .command_line
                                .push_error(crate::t!("SCALELISTEDIT ADD: use a ratio like 1:50.").as_ref()),
                        },
                        None => self
                            .command_line
                            .push_error(crate::t!("SCALELISTEDIT ADD: use a ratio like 1:50.").as_ref()),
                    },
                    "DELETE" | "REMOVE" => {
                        let current = self.tabs[i]
                            .scene
                            .document
                            .header
                            .current_annotation_scale
                            .clone();
                        if arg.is_empty() {
                            self.command_line.push_info(crate::t!("Usage: SCALELISTEDIT DELETE <name>").as_ref());
                        } else if arg.eq_ignore_ascii_case(&current) {
                            self.command_line.push_error(crate::tf!(
                                "Cannot delete the current annotation scale ({arg})."
                            ).as_ref());
                        } else {
                            self.push_undo_snapshot(i, "SCALELISTEDIT");
                            if self.tabs[i].scene.remove_scale(arg) {
                                self.tabs[i].dirty = true;
                                self.command_line
                                    .push_output(crate::tf!("Removed annotation scale {arg}.").as_ref());
                            } else {
                                self.command_line
                                    .push_info(crate::tf!("No annotation scale named {arg}.").as_ref());
                            }
                        }
                    }
                    "" => {
                        let names: Vec<String> = self.tabs[i]
                            .scene
                            .scale_list()
                            .into_iter()
                            .map(|(n, _, _)| n)
                            .collect();
                        if names.is_empty() {
                            self.command_line.push_info(crate::t!("No annotation scales defined.").as_ref());
                        } else {
                            self.command_line
                                .push_output(crate::tf!("Annotation scales: {}", names.join(", ")).as_ref());
                        }
                    }
                    _ => self
                        .command_line
                        .push_info(crate::t!("Usage: SCALELISTEDIT [ADD 1:50 | DELETE 1:50]").as_ref()),
                }
            }

            // OBJECTSCALE — open the Annotation Object Scale dialog for the
            // selected object (add / remove its per-object scale representations).
            // Reachable now that the immediate "add current scale" quick action
            // moved to the explicit `OBJECTSCALE ADD` keyword above.
            "OBJECTSCALE" => {
                return Some(Task::done(Message::AnnoObjectScaleOpen));
            }

            // DATALINK <path.csv> — import a CSV file into a table placed at the
            // origin (one-time import; a live re-reading link is future work).
            "DATALINK" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("DATALINK", "DATALINK  path to the .csv file:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("DATALINK ") => {
                let path = cmd.trim_start_matches("DATALINK").trim();
                if path.is_empty() {
                    self.command_line.push_info(
                        "Usage: DATALINK <path-to-.csv>  — imports the CSV into a table at the origin.",
                    );
                    return Some(Task::none());
                }
                match std::fs::read_to_string(path) {
                    Ok(text) => {
                        let rows_data: Vec<Vec<String>> = text
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .map(|line| line.split(',').map(|s| s.trim().to_string()).collect())
                            .collect();
                        let nrows = rows_data.len();
                        let ncols = rows_data.iter().map(|r| r.len()).max().unwrap_or(0);
                        if nrows == 0 || ncols == 0 {
                            self.command_line
                                .push_error(crate::t!("DATALINK: the CSV file is empty.").as_ref());
                            return Some(Task::none());
                        }
                        use acadrust::entities::TableBuilder;
                        use acadrust::types::Vector3;
                        let mut table = TableBuilder::new(nrows, ncols)
                            .at(Vector3::new(0.0, 0.0, 0.0))
                            .row_height(0.5)
                            .column_width(2.0)
                            .build();
                        for (r, row) in rows_data.iter().enumerate() {
                            for (c, cell) in row.iter().enumerate() {
                                table.set_cell_text(r, c, cell);
                            }
                        }
                        self.push_undo_snapshot(i, "DATALINK");
                        self.tabs[i]
                            .scene
                            .add_entity_clone(acadrust::EntityType::Table(table));
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::tf!(
                            "DATALINK: imported {nrows}×{ncols} cells into a table at the origin."
                        ).as_ref());
                    }
                    Err(e) => {
                        self.command_line
                            .push_error(crate::tf!("DATALINK: cannot read \"{path}\": {e}").as_ref());
                    }
                }
            }

            // LANDXMLIMPORT <path> — import survey points (LandXML <CgPoint>
            // elements) as Point objects. Reads the coordinate text content
            // (northing easting elevation) → Point at (easting, northing, elev).
            "LANDXMLIMPORT" => {
                use crate::command::ValuePromptCommand;
                let c =
                    ValuePromptCommand::new("LANDXMLIMPORT", "LANDXMLIMPORT  path to the .xml file:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("LANDXMLIMPORT ") => {
                let path = cmd.trim_start_matches("LANDXMLIMPORT").trim();
                if path.is_empty() {
                    self.command_line.push_info(
                        "Usage: LANDXMLIMPORT <path-to-.xml>  (imports CgPoint survey points)",
                    );
                    return Some(Task::none());
                }
                match std::fs::read_to_string(path) {
                    Ok(xml) => {
                        let pts = parse_landxml_cgpoints(&xml);
                        if pts.is_empty() {
                            self.command_line
                                .push_info(crate::t!("LANDXMLIMPORT: no <CgPoint> survey points found.").as_ref());
                            return Some(Task::none());
                        }
                        self.push_undo_snapshot(i, "LANDXMLIMPORT");
                        for [x, y, z] in &pts {
                            let mut p = acadrust::entities::Point::new();
                            p.location = acadrust::types::Vector3::new(*x, *y, *z);
                            self.tabs[i]
                                .scene
                                .add_entity_clone(acadrust::EntityType::Point(p));
                        }
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::tf!(
                            "LANDXMLIMPORT: imported {} survey point(s). Use ZOOM EXTENTS to view.",
                            pts.len()
                        ).as_ref());
                    }
                    Err(e) => self
                        .command_line
                        .push_error(crate::tf!("LANDXMLIMPORT: cannot read \"{path}\": {e}").as_ref()),
                }
            }

            "POINTCLOUDATTACH" | "RECAP" | "SYNCPVIEWPORTS" | "UNDERLAYLAYERS"
            | "UOSNAP" => {
                self.command_line
                    .push_info(crate::tf!("{cmd}: not yet implemented.").as_ref());
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}

// Scan LandXML text for <CgPoint> survey points. Each element's text content is
// "northing easting elevation"; returned as [easting, northing, elevation] so it
// maps to a Point at (X=easting, Y=northing, Z=elevation). Tolerant manual scan
// (no XML dependency); handles the standard text-content form.
// (landxml cgpoint scan)
fn parse_landxml_cgpoints(xml: &str) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find("<CgPoint") {
        let after = &rest[open + "<CgPoint".len()..];
        // Skip the container element "<CgPoints>".
        if !matches!(
            after.chars().next(),
            Some(' ') | Some('>') | Some('\t') | Some('\n') | Some('\r')
        ) {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let body = &after[gt + 1..];
        let Some(close) = body.find("</CgPoint>") else {
            break;
        };
        let text = &body[..close];
        let nums: Vec<f64> = text
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() >= 3 {
            out.push([nums[1], nums[0], nums[2]]);
        }
        rest = &body[close + "</CgPoint>".len()..];
    }
    out
}
