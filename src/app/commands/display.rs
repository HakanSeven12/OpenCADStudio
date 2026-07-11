use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_display(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // Interactive pan: left-drag pans the view until Esc. The only pan
            // path when there is no middle mouse button (trackpad / web).
            "PAN" => {
                self.tabs[i].pan_mode = true;
                self.command_line
                    .push_output("PAN: drag with the left mouse button. Press Esc to exit.");
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
                                self.command_line.push_output(&format!(
                                    "TABLE CELL: set [{row},{col}] = \"{text}\"."
                                ));
                            } else {
                                self.command_line.push_error(
                                    "TABLE CELL: select a Table entity first, or row/col out of range."
                                );
                            }
                        }
                        _ => {
                            self.command_line
                                .push_info("Usage: TABLE CELL <row> <col> <text>");
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
            cmd if cmd == "UCSICON" || cmd.starts_with("UCSICON ") => {
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
                        self.command_line.push_output(&format!(
                            "UCSICON {sub}: updated {count} viewport(s) + model space."
                        ));
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
                        self.command_line.push_output(&format!("UCSICON {state}"));
                    }
                    _ => {
                        self.command_line
                            .push_info("Usage: UCSICON ON | OFF | NOORIGIN | ORIGIN");
                    }
                }
            }

            // ── NAVVCUBE — toggle ViewCube visibility ────────────────────────────
            "NAVVCUBE" => {
                return Some(Task::done(Message::ToggleViewCube));
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
                    .push_info("TOOLPALETTES: Tool Palettes not yet implemented.");
            }

            // ── SHEETSET — not yet implemented ───────────────────────────────────
            "SHEETSET" => {
                self.command_line
                    .push_info("SHEETSET: Sheet Set Manager not yet implemented.");
            }

            // ── XDATA — read/write extended entity data ──────────────────────────
            // XDATA LIST             — show all xdata records on selected entities
            // XDATA SET <app> <str>  — append a string xdata value for <app>
            // XDATA CLEAR            — remove all xdata from selected entities
            // XDATA CLEAR <app>      — remove xdata for a specific application
            cmd if cmd == "XDATA" || cmd.starts_with("XDATA ") => {
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
                        .push_error("XDATA: select entities first.");
                } else {
                    match sub.as_str() {
                        "LIST" | "" => {
                            for sh in &selected_handles {
                                if let Some(entity) = self.tabs[i].scene.document.get_entity(*sh) {
                                    let xd = &entity.common().extended_data;
                                    if xd.is_empty() {
                                        self.command_line
                                            .push_output(&format!("  {:x}: no xdata.", sh.value()));
                                    } else {
                                        for rec in xd.records() {
                                            self.command_line.push_output(&format!(
                                                "  {:x} [{}]: {} value(s)",
                                                sh.value(),
                                                rec.application_name,
                                                rec.values.len()
                                            ));
                                            for v in &rec.values {
                                                self.command_line
                                                    .push_output(&format!("    {:?}", v));
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
                            self.command_line.push_output(&format!(
                                "XDATA: set [{app}] = \"{val}\" on {} entity/entities.",
                                selected_handles.len()
                            ));
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
                            self.command_line.push_output("XDATA: cleared.");
                        }
                        _ => {
                            self.command_line
                                .push_info("Usage: XDATA LIST | SET <app> <value> | CLEAR [app]");
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
                            .push_error("Usage: PLOTSTYLE [LOAD | CLEAR | STATUS]");
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
            cmd if cmd == "UNDERLAY" || cmd.starts_with("UNDERLAY ") => {
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
                        .push_error("UNDERLAY: select underlay entities first.");
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
                                    self.command_line.push_output(&format!(
                                        "Underlay {:x}: fade={}, contrast={}, on={}, clip={}, mono={}",
                                        h.value(),
                                        ul.fade,
                                        ul.contrast,
                                        ul.is_on(),
                                        ul.is_clipping(),
                                        ul.is_monochrome(),
                                    ));
                                }
                            }
                        }
                    }
                    if changed > 0 {
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_info(&format!("Updated {changed} underlay(s)."));
                    } else if !action.is_empty() {
                        self.command_line.push_error(
                            "Usage: UNDERLAY [FADE <n>|CONTRAST <n>|ON|OFF|CLIP ON|OFF|MONO ON|OFF]"
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
            // OBJECTSCALE — mark the selected objects annotative by attaching the
            // AcAnnotativeData XData record the tessellator already honours, so
            // they scale with the current annotation scale.
            cmd if cmd == "OBJECTSCALE" || cmd.starts_with("OBJECTSCALE ") => {
                use acadrust::xdata::{ExtendedDataRecord, XDataValue};
                let handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error("OBJECTSCALE: select objects first.");
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "OBJECTSCALE");
                let mut n = 0usize;
                for h in &handles {
                    if let Some(e) = self.tabs[i].scene.document.get_entity_mut(*h) {
                        let xd = &mut e.common_mut().extended_data;
                        if xd.get_record("AcAnnotativeData").is_none() {
                            let mut rec = ExtendedDataRecord::new("AcAnnotativeData");
                            rec.add_value(XDataValue::String("1".to_string()));
                            xd.add_record(rec);
                        }
                        n += 1;
                    }
                }
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].dirty = true;
                self.command_line.push_output(&format!(
                    "OBJECTSCALE: marked {n} object(s) annotative (they scale with the annotation scale)."
                ));
                return Some(Task::none());
            }

            // HYPERLINK <url> — attach a hyperlink to the selected objects, stored
            // in the standard PE_URL XData record so it round-trips in the file.
            cmd if cmd == "HYPERLINK" || cmd.starts_with("HYPERLINK ") => {
                use acadrust::xdata::{ExtendedDataRecord, XDataValue};
                let url = cmd.strip_prefix("HYPERLINK").unwrap_or("").trim().to_string();
                if url.is_empty() {
                    self.command_line.push_info("Usage: HYPERLINK <url>   (select objects first)");
                    return Some(Task::none());
                }
                let handles: Vec<acadrust::Handle> =
                    self.tabs[i].scene.selected_entities().iter().map(|(h, _)| *h).collect();
                if handles.is_empty() {
                    self.command_line.push_error("HYPERLINK: select objects first.");
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
                    .push_output(&format!("HYPERLINK: attached to {n} object(s)."));
                return Some(Task::none());
            }

            // ADJUST — set brightness / contrast / fade on selected raster images
            //   ADJUST BRIGHTNESS|CONTRAST|FADE <0-100>
            cmd if cmd == "ADJUST" || cmd.starts_with("ADJUST ") => {
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
                        .push_error("ADJUST: select raster image(s) first.");
                } else if action.is_empty() {
                    self.command_line
                        .push_info("Usage: ADJUST BRIGHTNESS|CONTRAST|FADE <0-100>");
                } else if let Ok(v) = arg.parse::<u8>() {
                    let v = v.min(100);
                    self.push_undo_snapshot(i, "ADJUST");
                    let mut changed = 0usize;
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
                                }
                                "CONTRAST" => {
                                    img.contrast = v;
                                    changed += 1;
                                }
                                "FADE" => {
                                    img.fade = v;
                                    changed += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                    if changed > 0 {
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.bump_geometry();
                        self.command_line
                            .push_output(&format!("ADJUST: {action} = {v} on {changed} image(s)."));
                    } else {
                        self.command_line.push_error(
                            "ADJUST: no raster images selected, or unknown property (use BRIGHTNESS|CONTRAST|FADE).",
                        );
                    }
                } else {
                    self.command_line.push_error("ADJUST: value must be 0-100.");
                }
            }

            // ANNOSCALE / CANNOSCALE <ratio> — set the current annotation scale
            // (e.g. 1:50, 2:1, or a plain factor). Drives annotative-object size
            // in model space and is written to the drawing header.
            cmd if cmd == "ANNOSCALE"
                || cmd == "CANNOSCALE"
                || cmd.starts_with("ANNOSCALE ")
                || cmd.starts_with("CANNOSCALE ") =>
            {
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
                        .push_output(&format!("Current annotation scale: {name}"));
                    return Some(Task::none());
                }
                // anno multiplier = denominator / numerator: 1:50 → 50, 2:1 → 0.5.
                let anno = if let Some((a, b)) = arg.split_once(':') {
                    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
                        (Ok(a), Ok(b)) if a != 0.0 => Some((b / a) as f32),
                        _ => None,
                    }
                } else {
                    arg.parse::<f32>().ok()
                };
                match anno {
                    Some(v) if v > 0.0 => {
                        self.tabs[i].scene.annotation_scale = v;
                        let hdr = &mut self.tabs[i].scene.document.header;
                        hdr.current_annotation_scale = arg.clone();
                        hdr.annotation_scale_value = 1.0 / v as f64;
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(&format!("Annotation scale: {arg}"));
                    }
                    _ => self
                        .command_line
                        .push_error("Usage: ANNOSCALE <ratio>  e.g. 1:50, 2:1, or a factor"),
                }
            }

            // SCALELISTEDIT — list the drawing's annotation scales.
            cmd if cmd == "SCALELISTEDIT" || cmd.starts_with("SCALELISTEDIT ") => {
                let names: Vec<String> = self.tabs[i]
                    .scene
                    .scale_list()
                    .into_iter()
                    .map(|(n, _, _)| n)
                    .collect();
                if names.is_empty() {
                    self.command_line.push_info("No annotation scales defined.");
                } else {
                    self.command_line
                        .push_output(&format!("Annotation scales: {}", names.join(", ")));
                }
            }

            // DATALINK <path.csv> — import a CSV file into a table placed at the
            // origin (one-time import; a live re-reading link is future work).
            cmd if cmd == "DATALINK" || cmd.starts_with("DATALINK ") => {
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
                                .push_error("DATALINK: the CSV file is empty.");
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
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(&format!(
                            "DATALINK: imported {nrows}×{ncols} cells into a table at the origin."
                        ));
                    }
                    Err(e) => {
                        self.command_line
                            .push_error(&format!("DATALINK: cannot read \"{path}\": {e}"));
                    }
                }
            }

            // LANDXMLIMPORT <path> — import survey points (LandXML <CgPoint>
            // elements) as Point objects. Reads the coordinate text content
            // (northing easting elevation) → Point at (easting, northing, elev).
            cmd if cmd == "LANDXMLIMPORT" || cmd.starts_with("LANDXMLIMPORT ") => {
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
                                .push_info("LANDXMLIMPORT: no <CgPoint> survey points found.");
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
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(&format!(
                            "LANDXMLIMPORT: imported {} survey point(s). Use ZOOM EXTENTS to view.",
                            pts.len()
                        ));
                    }
                    Err(e) => self
                        .command_line
                        .push_error(&format!("LANDXMLIMPORT: cannot read \"{path}\": {e}")),
                }
            }

            "POINTCLOUDATTACH" | "RECAP" | "SYNCPVIEWPORTS" | "UNDERLAYLAYERS" | "OBJECTSCALE"
            | "UOSNAP" => {
                self.command_line
                    .push_info(&format!("{cmd}: not yet implemented."));
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
