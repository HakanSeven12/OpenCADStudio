use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_display(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // Interactive pan: left-drag pans the view until Esc. The only pan
            // path when there is no middle mouse button (trackpad / web).
            "PAN" => {
                self.tabs[i].pan_mode = true;
                self.clear_navigation_hover(i);
                self.command_line.push_output(
                    crate::t!("PAN: drag with the left mouse button. Press Esc to exit.").as_ref(),
                );
            }

            // ── TABLE structure and cell editing ───────────────────────────────
            cmd if cmd.starts_with("TABLE ") => {
                let rest = cmd.trim_start_matches("TABLE").trim();
                #[derive(Clone)]
                enum TableAction {
                    Cell(usize, usize, String),
                    InsertRow(usize),
                    DeleteRow(usize),
                    InsertColumn(usize),
                    DeleteColumn(usize),
                    Merge(usize, usize, usize, usize),
                    Unmerge(usize, usize),
                    RowHeight(usize, f64),
                    ColumnWidth(usize, f64),
                    Lock(usize, usize, bool),
                    Block(usize, usize, String),
                    Formula(usize, usize, String),
                    Field(usize, usize, codec::Handle),
                }
                let words: Vec<&str> = rest.split_whitespace().collect();
                let integer = |index: usize| {
                    words
                        .get(index)
                        .and_then(|value| value.parse::<usize>().ok())
                };
                let real =
                    |index: usize| words.get(index).and_then(|value| value.parse::<f64>().ok());
                let action = match words
                    .first()
                    .map(|word| word.to_ascii_uppercase())
                    .as_deref()
                {
                    Some("CELL") => {
                        let parts: Vec<&str> = rest.splitn(4, char::is_whitespace).collect();
                        match (integer(1), integer(2)) {
                            (Some(row), Some(column)) => Some(TableAction::Cell(
                                row,
                                column,
                                parts.get(3).copied().unwrap_or("").to_string(),
                            )),
                            _ => None,
                        }
                    }
                    Some("INSERTROW") => integer(1).map(TableAction::InsertRow),
                    Some("DELETEROW") => integer(1).map(TableAction::DeleteRow),
                    Some("INSERTCOLUMN") | Some("INSERTCOL") => {
                        integer(1).map(TableAction::InsertColumn)
                    }
                    Some("DELETECOLUMN") | Some("DELETECOL") => {
                        integer(1).map(TableAction::DeleteColumn)
                    }
                    Some("MERGE") => match (integer(1), integer(2), integer(3), integer(4)) {
                        (Some(r1), Some(c1), Some(r2), Some(c2)) => {
                            Some(TableAction::Merge(r1, c1, r2, c2))
                        }
                        _ => None,
                    },
                    Some("UNMERGE") => match (integer(1), integer(2)) {
                        (Some(row), Some(column)) => Some(TableAction::Unmerge(row, column)),
                        _ => None,
                    },
                    Some("ROWHEIGHT") => match (integer(1), real(2)) {
                        (Some(row), Some(height)) if height > 0.0 => {
                            Some(TableAction::RowHeight(row, height))
                        }
                        _ => None,
                    },
                    Some("COLUMNWIDTH") | Some("COLWIDTH") => match (integer(1), real(2)) {
                        (Some(column), Some(width)) if width > 0.0 => {
                            Some(TableAction::ColumnWidth(column, width))
                        }
                        _ => None,
                    },
                    Some("LOCK") => match (integer(1), integer(2), words.get(3)) {
                        (Some(row), Some(column), Some(value)) => Some(TableAction::Lock(
                            row,
                            column,
                            !value.eq_ignore_ascii_case("OFF"),
                        )),
                        _ => None,
                    },
                    Some("BLOCK") => {
                        let parts: Vec<&str> = rest.splitn(4, char::is_whitespace).collect();
                        match (integer(1), integer(2), parts.get(3)) {
                            (Some(row), Some(column), Some(name)) if !name.trim().is_empty() => {
                                Some(TableAction::Block(row, column, name.trim().to_string()))
                            }
                            _ => None,
                        }
                    }
                    Some("FORMULA") => {
                        let parts: Vec<&str> = rest.splitn(4, char::is_whitespace).collect();
                        match (integer(1), integer(2), parts.get(3)) {
                            (Some(row), Some(column), Some(expression))
                                if !expression.trim().is_empty() =>
                            {
                                Some(TableAction::Formula(
                                    row,
                                    column,
                                    expression.trim().to_string(),
                                ))
                            }
                            _ => None,
                        }
                    }
                    Some("FIELD") => match (integer(1), integer(2), words.get(3)) {
                        (Some(row), Some(column), Some(value)) => {
                            u64::from_str_radix(value.trim_start_matches("0x"), 16)
                                .ok()
                                .map(|handle| {
                                    TableAction::Field(row, column, codec::Handle::new(handle))
                                })
                        }
                        _ => None,
                    },
                    _ => None,
                };
                let Some(action) = action else {
                    self.command_line.push_info(
                        crate::t!("Usage: TABLE CELL | INSERTROW | DELETEROW | INSERTCOL | DELETECOL | MERGE | UNMERGE | ROWHEIGHT | COLWIDTH | LOCK | BLOCK | FORMULA | FIELD").as_ref(),
                    );
                    return Some(Task::none());
                };
                let selected_handles: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(handle, _)| *handle)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if selected_handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("TABLE: select a table first.").as_ref());
                    return Some(Task::none());
                }
                let block_handle = if let TableAction::Block(_, _, name) = &action {
                    self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .iter()
                        .find(|record| record.name.eq_ignore_ascii_case(name))
                        .map(|record| record.handle)
                } else {
                    None
                };
                if matches!(action, TableAction::Block(_, _, _)) && block_handle.is_none() {
                    self.command_line
                        .push_error(crate::t!("TABLE: block definition not found.").as_ref());
                    return Some(Task::none());
                }
                if let TableAction::Field(_, _, field_handle) = &action {
                    if !self.tabs[i]
                        .scene
                        .document
                        .fields
                        .contains_key(field_handle)
                    {
                        self.command_line
                            .push_error(crate::t!("TABLE: field handle not found.").as_ref());
                        return Some(Task::none());
                    }
                }
                self.push_undo_snapshot(i, "TABLE EDIT");
                let mut changed = false;
                for handle in &selected_handles {
                    let Some(codec::EntityType::Table(table)) =
                        self.tabs[i].scene.document.get_entity_mut(*handle)
                    else {
                        continue;
                    };
                    // `changed` tracks this table while the action runs.
                    let changed_before = std::mem::take(&mut changed);
                    match &action {
                        TableAction::Cell(row, column, text) => {
                            if let Some(cell) = table.cell_mut(*row, *column) {
                                use codec::entities::table::CellStateFlags;
                                if !cell.state.intersects(
                                    CellStateFlags::CONTENT_LOCKED
                                        | CellStateFlags::CONTENT_READ_ONLY,
                                ) {
                                    cell.set_text(text);
                                    changed = true;
                                }
                            }
                        }
                        TableAction::InsertRow(row) if *row <= table.row_count() => {
                            let height = table
                                .rows
                                .get((*row).min(table.row_count().saturating_sub(1)))
                                .map(|row| row.height)
                                .unwrap_or(0.5);
                            table.insert_row(*row);
                            table.set_row_height(*row, height);
                            changed = true;
                        }
                        TableAction::DeleteRow(row)
                            if table.row_count() > 1 && *row < table.row_count() =>
                        {
                            table.remove_row(*row);
                            changed = true;
                        }
                        TableAction::InsertColumn(column) if *column <= table.column_count() => {
                            let width = table
                                .columns
                                .get((*column).min(table.column_count().saturating_sub(1)))
                                .map(|column| column.width)
                                .unwrap_or(2.0);
                            table.insert_column(*column, width);
                            changed = true;
                        }
                        TableAction::DeleteColumn(column)
                            if table.column_count() > 1 && *column < table.column_count() =>
                        {
                            table.remove_column(*column);
                            changed = true;
                        }
                        TableAction::Merge(r1, c1, r2, c2)
                            if *r1 < table.row_count()
                                && *r2 < table.row_count()
                                && *c1 < table.column_count()
                                && *c2 < table.column_count() =>
                        {
                            table.merge_cells(codec::entities::table::CellRange::new(
                                (*r1).min(*r2),
                                (*c1).min(*c2),
                                (*r1).max(*r2),
                                (*c1).max(*c2),
                            ));
                            changed = true;
                        }
                        TableAction::Unmerge(row, column) => {
                            table.unmerge_cell(*row, *column);
                            changed = true;
                        }
                        TableAction::RowHeight(row, height) if *row < table.row_count() => {
                            table.set_row_height(*row, *height);
                            changed = true;
                        }
                        TableAction::ColumnWidth(column, width)
                            if *column < table.column_count() =>
                        {
                            table.set_column_width(*column, *width);
                            changed = true;
                        }
                        TableAction::Lock(row, column, locked) => {
                            if let Some(cell) = table.cell_mut(*row, *column) {
                                use codec::entities::table::CellStateFlags;
                                cell.state.set(CellStateFlags::CONTENT_LOCKED, *locked);
                                cell.state.set(CellStateFlags::FORMAT_LOCKED, *locked);
                                changed = true;
                            }
                        }
                        TableAction::Block(row, column, _) => {
                            if let (Some(handle), Some(cell)) =
                                (block_handle, table.cell_mut(*row, *column))
                            {
                                use codec::entities::table::{
                                    CellContent, CellStateFlags, CellType,
                                };
                                if !cell.state.intersects(
                                    CellStateFlags::CONTENT_LOCKED
                                        | CellStateFlags::CONTENT_READ_ONLY,
                                ) {
                                    cell.contents.clear();
                                    cell.contents.push(CellContent::block(handle));
                                    cell.cell_type = CellType::Block;
                                    changed = true;
                                }
                            }
                        }
                        TableAction::Formula(row, column, expression) => {
                            if let Some(cell) = table.cell_mut(*row, *column) {
                                use codec::entities::table::CellStateFlags;
                                if !cell.state.intersects(
                                    CellStateFlags::CONTENT_LOCKED
                                        | CellStateFlags::CONTENT_READ_ONLY,
                                ) {
                                    let formula = if expression.starts_with('=') {
                                        expression.clone()
                                    } else {
                                        format!("={expression}")
                                    };
                                    cell.set_text(&formula);
                                    changed = true;
                                }
                            }
                        }
                        TableAction::Field(row, column, field_handle) => {
                            let mut attached = false;
                            if let Some(cell) = table.cell_mut(*row, *column) {
                                use codec::entities::table::{
                                    CellContent, CellStateFlags, CellType,
                                };
                                if !cell.state.intersects(
                                    CellStateFlags::CONTENT_LOCKED
                                        | CellStateFlags::CONTENT_READ_ONLY,
                                ) {
                                    let mut content = CellContent::text("");
                                    content.field_handle = Some(*field_handle);
                                    cell.contents.clear();
                                    cell.contents.push(content);
                                    cell.cell_type = CellType::Text;
                                    attached = true;
                                }
                            }
                            if attached {
                                if !table.field_handles.contains(field_handle) {
                                    table.field_handles.push(*field_handle);
                                }
                                changed = true;
                            }
                        }
                        _ => {}
                    }
                    if changed {
                        // The stored picture no longer shows the cells: draw
                        // from them, and redraw the picture.
                        table.block_record_handle = None;
                    }
                    changed |= changed_before;
                }
                if changed {
                    self.invalidate_property_targets(i, &selected_handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    self.command_line
                        .push_output(crate::t!("TABLE: edit applied.").as_ref());
                } else {
                    self.command_line.push_error(
                        crate::t!("TABLE: row/column is out of range or the cell is locked.")
                            .as_ref(),
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
                            if let codec::EntityType::Viewport(vp) = entity {
                                vp.status.ucs_icon_visible = visible;
                                if sub == "NOORIGIN" || sub == "ORIGIN" {
                                    vp.status.ucs_icon_at_origin = at_origin;
                                }
                                count += 1;
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(
                            crate::tf!("UCSICON {sub}: updated {count} viewport(s) + model space.")
                                .as_ref(),
                        );
                    }
                    "" => {
                        // Bare UCSICON toggles visibility.
                        self.push_undo_snapshot(i, "UCSICON");
                        let visible = !self.show_ucs_icon;
                        self.show_ucs_icon = visible;
                        self.ribbon.set_ucs_icon(visible);
                        for entity in self.tabs[i].scene.document.entities_mut() {
                            if let codec::EntityType::Viewport(vp) = entity {
                                vp.status.ucs_icon_visible = visible;
                            }
                        }
                        self.tabs[i].dirty = true;
                        let state = if visible { "ON" } else { "OFF" };
                        self.command_line
                            .push_output(crate::tf!("UCSICON {state}").as_ref());
                    }
                    _ => {
                        self.command_line.push_info(
                            crate::t!("Usage: UCSICON ON | OFF | NOORIGIN | ORIGIN").as_ref(),
                        );
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
                self.command_line.push_output(
                    crate::t!(if enabled {
                        "Limits checking ON."
                    } else {
                        "Limits checking OFF."
                    })
                    .as_ref(),
                );
            }
            cmd if cmd.starts_with("LIMITS SET ") => {
                let tokens: Vec<&str> = cmd["LIMITS SET ".len()..].split_whitespace().collect();
                let values: Result<Vec<f64>, _> =
                    tokens.iter().map(|value| value.parse()).collect();
                let Ok(values) = values else {
                    self.command_line.push_error(
                        crate::t!("LIMITS: four numeric coordinates required.").as_ref(),
                    );
                    return Some(Task::none());
                };
                if tokens.len() != 4 || !values.iter().all(|value| value.is_finite()) {
                    self.command_line.push_error(
                        crate::t!("LIMITS: four finite numeric coordinates required.").as_ref(),
                    );
                } else {
                    let first = glam::DVec2::new(values[0], values[1]);
                    let opposite = glam::DVec2::new(values[2], values[3]);
                    let min = first.min(opposite);
                    let max = first.max(opposite);
                    if min.x == max.x || min.y == max.y {
                        self.command_line.push_error(
                            crate::t!("LIMITS: corners must define a non-zero area.").as_ref(),
                        );
                    } else {
                        self.push_undo_snapshot(i, "LIMITS");
                        self.tabs[i].scene.set_current_drawing_limits(min, max);
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(
                            crate::tf!(
                                "Drawing limits: {:.4},{:.4} to {:.4},{:.4}.",
                                min.x,
                                min.y,
                                max.x,
                                max.y
                            )
                            .as_ref(),
                        );
                    }
                }
            }

            // ── PROPERTIES — toggle Properties panel visibility ──────────────────
            "PROPERTIES" | "PROPS" => {
                return Some(Task::done(Message::ToggleProperties));
            }

            // ── GRAPHICATTRIBUTES — toggle Graphic Attributes palette ───────────
            "GRAPHICATTRIBUTES" => {
                return Some(Task::done(Message::ToggleGraphicAttributes));
            }

            // ── FILETAB — toggle file/document tabs ──────────────────────────────
            "FILETAB" => {
                return Some(Task::done(Message::ToggleFileTabs));
            }

            // ── LAYOUTTAB — toggle layout/paper-space tabs ───────────────────────
            "LAYOUTTAB" => {
                return Some(Task::done(Message::ToggleLayoutTabs));
            }

            // ── REDRAW / REGEN ──────────────────────────────────────────────
            // REDRAW — force a full re-rasterize of the current viewport next
            // frame, WITHOUT touching the DB (never bumps geometry_epoch /
            // block_epoch, never pushes undo). Scope: Active. This arm does not
            // itself clear previews or cancel commands (the normal non-transparent
            // dispatch teardown, commands/mod.rs:90-96, governs that separately);
            // it only queues a per-viewport cache invalidation.
            "REDRAW" => {
                use crate::scene::ViewportRefreshScope;
                self.tabs[i]
                    .scene
                    .request_refresh(ViewportRefreshScope::Active);
                self.command_line
                    .push_output(crate::t!("REDRAW: viewport refreshed.").as_ref());
                return Some(Task::none());
            }
            // REDRAWALL — force re-rasterize of every generated viewport.
            "REDRAWALL" => {
                use crate::scene::ViewportRefreshScope;
                self.tabs[i]
                    .scene
                    .request_refresh(ViewportRefreshScope::All);
                self.command_line
                    .push_output(crate::t!("REDRAWALL: viewports refreshed.").as_ref());
                return Some(Task::none());
            }
            // Rebuild tessellation caches without modifying the document.
            "REGEN" | "REGENALL" => {
                // A regen is a field evaluation event (FIELDEVAL bit 16).
                self.tabs[i].scene.update_fields(16, None);
                self.tabs[i].scene.populate_meshes_from_document();
                self.command_line
                    .push_output(crate::t!("REGEN: regenerated model.").as_ref());
                return Some(Task::none());
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
            // ISOPLANE — cycle the isometric drafting axis pair (F5).
            "ISOPLANE" => {
                return Some(Task::done(Message::CycleIsoPlane));
            }
            cmd if cmd.starts_with("ISOPLANE ") => {
                let plane = match cmd.trim_start_matches("ISOPLANE").trim() {
                    "LEFT" | "L" => Some(crate::app::settings::IsoPlane::Left),
                    "TOP" | "T" => Some(crate::app::settings::IsoPlane::Top),
                    "RIGHT" | "R" => Some(crate::app::settings::IsoPlane::Right),
                    _ => None,
                };
                if let Some(plane) = plane {
                    return Some(Task::done(Message::SetIsoPlane(plane)));
                }
                self.command_line
                    .push_error(crate::t!("ISOPLANE: expected Left, Top, or Right.").as_ref());
            }
            // ISODRAFT — enable or disable isometric drafting.
            "ISODRAFT" => {
                return Some(Task::done(Message::ToggleIsometricDrafting));
            }
            cmd if cmd.starts_with("ISODRAFT ") => {
                let requested = match cmd.trim_start_matches("ISODRAFT").trim() {
                    "1" | "ON" => Some(true),
                    "0" | "OFF" => Some(false),
                    _ => None,
                };
                match requested {
                    Some(value) if value != self.isometric_drafting => {
                        return Some(Task::done(Message::ToggleIsometricDrafting));
                    }
                    Some(_) => {}
                    None => self
                        .command_line
                        .push_error(crate::t!("ISODRAFT: expected On or Off.").as_ref()),
                }
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
            // UNITS — length and angle formats, plus the insertion unit. The
            // status-bar button covers the length format alone; everything else
            // about how this drawing writes numbers is here.
            "UNITS" | "DDUNITS" => {
                return Some(Task::done(Message::OpenDrawingUnits));
            }

            // ── CLEANSCREEN — collapse the surrounding panels for a full canvas ──
            "CLEANSCREEN" => {
                return Some(Task::done(Message::ToggleCleanScreen));
            }
            // ── RIBBON / RIBBONCLOSE — show or hide the ribbon ───────────────────
            "RIBBON" | "RIBBONCLOSE" => {
                self.ribbon_closed = cmd == "RIBBONCLOSE";
                self.persist_settings_if_changed();
            }
            // ── QUICKPROPERTIES — toggle the floating quick-properties readout ───
            "QUICKPROPERTIES" => {
                return Some(Task::done(Message::ToggleQuickProperties));
            }

            "BAUTHORPALETTE" | "BAUTHORPALETTECLOSE" => {
                let open = cmd == "BAUTHORPALETTE" && !self.show_authoring_palettes;
                return Some(self.dock_set_open(crate::ui::dock::PanelId::AuthoringPalettes, open));
            }

            // ── TOOLPALETTES — not yet implemented ───────────────────────────────
            "TOOLPALETTES" => {
                self.command_line.push_info(
                    crate::t!("TOOLPALETTES: Tool Palettes not yet implemented.").as_ref(),
                );
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
                use codec::xdata::{ExtendedDataRecord, XDataValue};
                let rest = cmd.trim_start_matches("XDATA").trim();
                let parts: Vec<&str> = rest.splitn(3, char::is_whitespace).collect();
                let sub = parts.first().map(|s| s.to_uppercase()).unwrap_or_default();
                let selected_handles: Vec<codec::Handle> = self.tabs[i]
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
                                        self.command_line.push_output(
                                            crate::tf!("  {:x}: no xdata.", sh.value()).as_ref(),
                                        );
                                    } else {
                                        for rec in xd.records() {
                                            self.command_line.push_output(
                                                crate::tf!(
                                                    "  {:x} [{}]: {} value(s)",
                                                    sh.value(),
                                                    rec.application_name,
                                                    rec.values.len()
                                                )
                                                .as_ref(),
                                            );
                                            for v in &rec.values {
                                                self.command_line.push_output(
                                                    crate::tf!("    {:?}", v).as_ref(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "SET" => {
                            let app = parts.get(1).copied().unwrap_or("OpenCADStudio");
                            let val = parts.get(2).copied().unwrap_or("");
                            let editable: Vec<_> = selected_handles
                                .iter()
                                .copied()
                                .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                                .collect();
                            if editable.is_empty() {
                                self.command_line.push_error(
                                    crate::t!("XDATA: selected entities are on locked layers.")
                                        .as_ref(),
                                );
                                return Some(Task::none());
                            }
                            self.push_undo_snapshot(i, "XDATA SET");
                            for sh in &editable {
                                if let Some(entity) =
                                    self.tabs[i].scene.document.get_entity_mut(*sh)
                                {
                                    let mut rec = ExtendedDataRecord::new(app);
                                    rec.add_value(XDataValue::String(val.to_string()));
                                    entity.common_mut().extended_data.add_record(rec);
                                }
                            }
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(
                                crate::tf!(
                                    "XDATA: set [{app}] = \"{val}\" on {} entity/entities.",
                                    editable.len()
                                )
                                .as_ref(),
                            );
                        }
                        "CLEAR" => {
                            let app_filter = parts.get(1).copied();
                            let editable: Vec<_> = selected_handles
                                .iter()
                                .copied()
                                .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                                .collect();
                            if editable.is_empty() {
                                self.command_line.push_error(
                                    crate::t!("XDATA: selected entities are on locked layers.")
                                        .as_ref(),
                                );
                                return Some(Task::none());
                            }
                            self.push_undo_snapshot(i, "XDATA CLEAR");
                            for sh in &editable {
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
                            self.command_line
                                .push_output(crate::t!("XDATA: cleared.").as_ref());
                        }
                        _ => {
                            self.command_line.push_info(
                                crate::t!("Usage: XDATA LIST | SET <app> <value> | CLEAR [app]")
                                    .as_ref(),
                            );
                        }
                    }
                }
            }

            // ── EXTRUDE ────────────────────────────────────────────────────
            "EXTRUDE" => {
                use crate::modules::insert::solid3d_cmds::ExtrudeCommand;
                // A preselection becomes the complete source set; otherwise
                // the interactive command gathers any number of profiles.
                let selected: Vec<_> = self.tabs[i].scene.selected_entities().into_iter().collect();
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                if !selected.is_empty() {
                    let mut cmd = ExtrudeCommand::new_named(cmd, color);
                    let first_curve = selected
                        .iter()
                        .find_map(|(_, entity)| crate::entities::curve::entity_curve(entity));
                    let anchor = first_curve
                        .as_ref()
                        .map(|curve| glam::DVec3::from_array(curve.plane.origin))
                        .unwrap_or(glam::DVec3::ZERO);
                    let direction = first_curve
                        .and_then(|curve| curve.plane.normal())
                        .map(glam::DVec3::from_array);
                    cmd.set_preselection(
                        selected
                            .iter()
                            .map(|(handle, entity)| (*handle, (*entity).clone()))
                            .collect(),
                        anchor,
                        direction,
                    );
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let cmd = ExtrudeCommand::new_named(cmd, color);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            "THICKEN" => {
                use crate::modules::insert::solid3d_cmds::ThickenCommand;
                let selected = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(handle, entity)| (handle, entity.clone()))
                    .collect();
                let command = ThickenCommand::new(selected);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "PRESSPULL" => {
                use crate::modules::insert::solid3d_cmds::PresspullCommand;
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                let mut command = PresspullCommand::new(color);
                command.set_isolines(crate::entities::solid3d::clamp_header_isolines(
                    self.tabs[i].scene.document.header.isolines,
                ));
                command.set_preselection(self.presspull_preselection());
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            // ── REVOLVE ────────────────────────────────────────────────────
            "REVOLVE" => {
                use crate::modules::insert::solid3d_cmds::RevolveCommand;
                let selected: Vec<_> = self.tabs[i].scene.selected_entities().into_iter().collect();
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                let isolines = crate::entities::solid3d::clamp_header_isolines(
                    self.tabs[i].scene.document.header.isolines,
                );
                let mut cmd = RevolveCommand::new(color, isolines);
                if !selected.is_empty() {
                    cmd.set_preselection(
                        selected
                            .iter()
                            .map(|(handle, entity)| (*handle, (*entity).clone()))
                            .collect(),
                    );
                }
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // ── SWEEP ──────────────────────────────────────────────────────
            "SWEEP" => {
                use crate::modules::insert::solid3d_cmds::SweepCommand;
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                let isolines = crate::entities::solid3d::clamp_header_isolines(
                    self.tabs[i].scene.document.header.isolines,
                );
                let mut cmd = SweepCommand::new(color, isolines);
                cmd.set_parameters(self.tabs[i].scene.named_parameters().clone());
                let selected = self.tabs[i]
                    .scene
                    .selected_handles_in_order()
                    .into_iter()
                    .filter_map(|handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(handle)
                            .cloned()
                            .map(|entity| (handle, entity))
                    })
                    .collect::<Vec<_>>();
                if !selected.is_empty() {
                    cmd.set_preselection(selected);
                }
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // ── LOFT ───────────────────────────────────────────────────────
            "LOFT" => {
                use crate::modules::insert::solid3d_cmds::LoftCommand;
                let color = self.tabs[i].scene.layer_color(&self.tabs[i].active_layer);
                let isolines = crate::entities::solid3d::clamp_header_isolines(
                    self.tabs[i].scene.document.header.isolines,
                );
                let selected = self.tabs[i]
                    .scene
                    .selected_handles_in_order()
                    .into_iter()
                    .filter_map(|handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(handle)
                            .cloned()
                            .map(|entity| (handle, entity))
                    })
                    .collect();
                let available = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .map(|entity| (entity.common().handle, entity.clone()))
                    .collect();
                let cmd = LoftCommand::new(
                    color,
                    isolines,
                    crate::command::ExtrudeMode::Solid,
                    selected,
                    available,
                );
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

            // FBXEXPORT — export 3D meshes to FBX for visualisation tools
            "FBXEXPORT" | "EXPORTFBX" => {
                return Some(Task::done(Message::FbxExport));
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
                // DXEVAL: the plot command compares extraction tables
                // before its dialog opens; -PLOT does not.
                if let Some(task) = self.evaluate_extraction_tables(
                    i,
                    super::extraction_eval::EVENT_PLOT,
                    Some(Message::Command(cmd.to_string())),
                ) {
                    return Some(task);
                }
                return Some(Task::done(Message::PlotDialogOpen));
            }
            // PUBLISH: the layouts of the drawing, each with its own page
            // setup, into one multi-page PDF or to the printer.
            "PRINTALL" | "PUBLISH" => {
                return Some(Task::done(Message::PrintAllOpen));
            }
            // PRINTERS — what the system reports about printing, on the
            // command line: the default printer, every printer it lists (or
            // why it could not), and how a plot will reach them. The report
            // a user pastes into a bug report instead of a screenshot.
            "PRINTERS" => {
                for line in crate::io::print_to_printer::printer_report() {
                    self.command_line.push_output(&line);
                }
            }
            // PRINTERS <name> — what that printer reports about its sheets
            // and printable area (asks the driver; may take a moment for an
            // offline network queue).
            cmd if cmd.starts_with("PRINTERS ") => {
                let name = cmd["PRINTERS ".len()..].trim();
                for line in crate::io::print_to_printer::printer_media_report(name) {
                    self.command_line.push_output(&line);
                }
            }
            "EXPORT" => {
                return Some(Task::done(Message::PlotExport));
            }
            "EXPORTPDF" => {
                self.plot_dialog.file_svg = false;
                return Some(Task::done(Message::PlotExport));
            }
            // EXPORTSVG — the same direct export, but the save dialog
            // proposes the SVG device's defaults (`stem.svg`).
            "EXPORTSVG" => {
                self.plot_dialog.to_file = true;
                self.plot_dialog.file_svg = true;
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
                            .map(|t| crate::tf!("Active: {}", t.name).into_owned())
                            .unwrap_or_else(|| crate::t!("No plot style loaded.").into_owned());
                        self.command_line.push_info(&active);
                        return Some(Task::done(Message::PlotStyleLoad));
                    }
                    "?" | "STATUS" => {
                        let msg = self
                            .active_plot_style
                            .as_ref()
                            .map(|t| {
                                crate::tf!(
                                    "Plot style: {}  ({} color overrides)",
                                    t.name,
                                    t.aci_entries.iter().filter(|e| e.color.is_some()).count()
                                )
                                .into_owned()
                            })
                            .unwrap_or_else(|| {
                                crate::t!("No plot style table loaded.").into_owned()
                            });
                        self.command_line.push_output(&msg);
                    }
                    _ => {
                        self.command_line.push_error(
                            crate::t!("Usage: PLOTSTYLE [LOAD | CLEAR | STATUS]").as_ref(),
                        );
                    }
                }
            }
            // TRANSPARENCY — raster images show transparent pixels (ON) or
            // draw every pixel in its colour (OFF).
            "TRANSPARENCY" => {
                let c = crate::modules::insert::image_transparency::TransparencyCommand::new();
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            "TRANSPARENCY MODE" | "TRANSPARENCY ON" | "TRANSPARENCY OFF" => {
                use codec::entities::ImageDisplayFlags;
                let images: Vec<(codec::Handle, bool)> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter(|(handle, _)| !self.tabs[i].scene.is_layer_locked(*handle))
                    .filter_map(|(handle, _)| {
                        match self.tabs[i].scene.document.get_entity(*handle) {
                            Some(codec::EntityType::RasterImage(image)) => Some((
                                *handle,
                                image.flags.contains(ImageDisplayFlags::TRANSPARENCY_ON),
                            )),
                            _ => None,
                        }
                    })
                    .collect();
                let Some(&(_, first_on)) = images.first() else {
                    return Some(Task::none());
                };
                let handles: Vec<codec::Handle> =
                    images.iter().map(|(handle, _)| *handle).collect();
                if cmd == "TRANSPARENCY MODE" {
                    let c = crate::modules::insert::image_transparency::TransparencyCommand::mode(
                        handles, first_on,
                    );
                    self.command_line.push_info(&c.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(c));
                } else {
                    let on = cmd == "TRANSPARENCY ON";
                    self.push_undo_snapshot(i, "TRANSPARENCY");
                    for handle in &handles {
                        if let Some(codec::EntityType::RasterImage(image)) =
                            self.tabs[i].scene.document.get_entity_mut(*handle)
                        {
                            image.flags.set(ImageDisplayFlags::TRANSPARENCY_ON, on);
                        }
                    }
                    let changes: Vec<_> = handles
                        .iter()
                        .map(|handle| (*handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.tabs[i].dirty = true;
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
                let handles: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    self.command_line.push_error(
                        crate::t!("UNDERLAY: select underlay entities first.").as_ref(),
                    );
                } else {
                    let parts: Vec<&str> = sub.splitn(2, char::is_whitespace).collect();
                    let action = parts.first().copied().unwrap_or("");
                    let arg = parts.get(1).copied().unwrap_or("").trim();
                    let mut changed = 0usize;
                    self.push_undo_snapshot(i, "UNDERLAY");
                    for h in &handles {
                        if let Some(codec::EntityType::Underlay(ul)) = self.tabs[i]
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
                                        ul.flags |= codec::entities::UnderlayDisplayFlags::CLIPPING;
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

            // Add or remove the current representation on the selected
            // annotative objects. With no preselection, gather one through the
            // regular selection engine before applying the action.
            action @ ("OBJECTSCALE ADD" | "OBJECTSCALE DELETE") => {
                let selected: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if selected.is_empty() {
                    let action = if action.ends_with("ADD") {
                        "OBJECTSCALE ADD"
                    } else {
                        "OBJECTSCALE DELETE"
                    };
                    let command = crate::modules::annotate::annotation_scale::AnnotationScaleSelectionCommand::new(
                        "OBJECTSCALE",
                        action,
                    );
                    self.command_line
                        .push_info(&crate::command::CadCommand::prompt(&command));
                    self.tabs[i].active_cmd = Some(Box::new(command));
                    return Some(Task::none());
                }
                let Some(scale) = self.tabs[i].scene.creation_annotation_scale_handle() else {
                    self.command_line.push_error(
                        crate::t!("OBJECTSCALE: the active annotation scale is unavailable.")
                            .as_ref(),
                    );
                    return Some(Task::none());
                };

                let adding = action.ends_with("ADD");
                let handles: Vec<_> = selected
                    .into_iter()
                    .filter(|handle| {
                        let memberships = crate::scene::annotative::object_scale_memberships(
                            &self.tabs[i].scene.document,
                            *handle,
                        );
                        let member = memberships.iter().any(|(_, current)| *current == scale);
                        if adding {
                            !member
                                && self.tabs[i].scene.document.get_entity(*handle).is_some_and(
                                    crate::scene::annotative::supports_annotation_context,
                                )
                        } else {
                            member
                        }
                    })
                    .collect();
                if handles.is_empty() {
                    self.command_line.push_info(
                        if adding {
                            crate::t!("OBJECTSCALE: the selected objects already have the current scale, or do not support annotation scales.")
                        } else {
                            crate::t!("OBJECTSCALE: the selected objects do not have the current scale.")
                        }
                        .as_ref(),
                    );
                    return Some(Task::none());
                }

                self.push_undo_snapshot(i, "OBJECTSCALE");
                let mut changed = Vec::new();
                for handle in handles {
                    let ok = if adding {
                        let created = crate::scene::annotative::create_annotation_context(
                            &mut self.tabs[i].scene.document,
                            handle,
                            scale,
                        );
                        if created {
                            crate::scene::annotative::set_entity_annotative(
                                &mut self.tabs[i].scene.document,
                                handle,
                                true,
                            );
                        }
                        created
                    } else {
                        crate::scene::annotative::remove_annotation_context_for_scale(
                            &mut self.tabs[i].scene.document,
                            handle,
                            scale,
                        )
                    };
                    if ok {
                        changed.push(handle);
                    }
                }
                let changes: Vec<_> = changed
                    .iter()
                    .copied()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities(&changes);
                self.tabs[i].dirty = true;
                self.command_line.push_output(
                    crate::tf!(
                        "OBJECTSCALE: {} the current scale on {} object(s).",
                        if adding { "added" } else { "removed" },
                        changed.len()
                    )
                    .as_ref(),
                );
                return Some(Task::none());
            }

            // IMAGEADJUST: chosen images (pickfirst first), then the dialog.
            "IMAGEADJUST" => {
                let keep = crate::modules::insert::xadjust::keeps("I");
                let chosen: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(_, e)| keep(e))
                    .map(|(h, _)| h)
                    .collect();
                if chosen.is_empty() {
                    self.tabs[i].scene.deselect_all();
                    self.refresh_properties();
                    let command = crate::modules::draw::select::SelectObjectsCommand::filtered(
                        "IMAGEADJUST",
                        "_IMAGEADJUSTSEL",
                        crate::modules::insert::xadjust::select_prompt("I"),
                        keep,
                    );
                    use crate::command::CadCommand;
                    self.command_line.push_info(&command.prompt());
                    self.command_line.set_step_options(command.options());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                } else {
                    self.tabs[i]
                        .scene
                        .replace_selection(chosen.into_iter().collect());
                    return Some(self.dispatch_command("_IMAGEADJUSTSEL"));
                }
            }
            "_IMAGEADJUSTSEL" => {
                self.open_image_adjust(i);
                return Some(self.finish_dispatch("IMAGEADJUST"));
            }
            // ADJUST / -IMAGEADJUST / PDFADJUST / DWFADJUST / DGNADJUST:
            // chosen images or underlays (pickfirst first), then the option
            // and the value.
            "ADJUST" | "-IMAGEADJUST" | "PDFADJUST" | "DWFADJUST" | "DGNADJUST" => {
                let mode = match cmd {
                    "-IMAGEADJUST" => "I",
                    "PDFADJUST" => "P",
                    "DWFADJUST" => "W",
                    "DGNADJUST" => "G",
                    _ => "A",
                };
                let keep = crate::modules::insert::xadjust::keeps(mode);
                let chosen: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(_, e)| keep(e))
                    .map(|(h, _)| h)
                    .collect();
                if !chosen.is_empty() {
                    self.tabs[i]
                        .scene
                        .replace_selection(chosen.into_iter().collect());
                    return Some(self.dispatch_command(&format!("_ADJUSTSEL {mode}")));
                }
                self.start_adjust_selection(i, cmd, mode);
            }
            // The gathered objects: one kind at a time, then the option.
            c if c.starts_with("_ADJUSTSEL ") => {
                use crate::command::CadCommand;
                use crate::modules::insert::xadjust::{self, AdjustCommand};
                let mode = c["_ADJUSTSEL ".len()..].trim().to_string();
                let keep = xadjust::keeps(&mode);
                let name = xadjust::command_name(&mode);
                let objects: Vec<(codec::Handle, codec::EntityType)> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(h, e)| keep(e) && !self.tabs[i].scene.is_layer_locked(*h))
                    .map(|(h, e)| (h, e.clone()))
                    .collect();
                match AdjustCommand::for_objects(&objects) {
                    None => {}
                    Some(Err(message)) => {
                        self.command_line.push_error(crate::t!(message).as_ref());
                        self.start_adjust_selection(i, name, &mode);
                    }
                    Some(Ok(command)) => {
                        self.command_line.push_info(&command.prompt());
                        self.command_line.set_step_options(command.options());
                        self.tabs[i].active_cmd = Some(Box::new(command));
                    }
                }
                // Repeating runs the command, not this step.
                return Some(self.finish_dispatch(name));
            }
            // `_ADJUSTAPPLY <BRIGHTNESS|CONTRAST|FADE|MONOCHROME> <value> <handles…>`
            // writes the value to every listed image or underlay, one undo step.
            c if c.starts_with("_ADJUSTAPPLY ") => {
                let mut parts = c.split_whitespace().skip(1);
                let prop = parts.next().unwrap_or("").to_string();
                let value: u8 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let handles: Vec<codec::Handle> = parts
                    .filter_map(|h| u64::from_str_radix(h, 16).ok())
                    .map(codec::Handle::new)
                    .collect();
                self.push_undo_snapshot(i, "ADJUST");
                let mut changed = Vec::new();
                for h in &handles {
                    let done = match self.tabs[i].scene.document.get_entity_mut(*h) {
                        Some(codec::EntityType::RasterImage(img)) => match prop.as_str() {
                            "BRIGHTNESS" => {
                                img.brightness = value;
                                true
                            }
                            "CONTRAST" => {
                                img.contrast = value;
                                true
                            }
                            "FADE" => {
                                img.fade = value;
                                true
                            }
                            _ => false,
                        },
                        Some(codec::EntityType::Underlay(u)) => match prop.as_str() {
                            "CONTRAST" => {
                                u.set_contrast(value);
                                true
                            }
                            "FADE" => {
                                u.set_fade(value);
                                true
                            }
                            "MONOCHROME" => {
                                u.set_monochrome(value != 0);
                                true
                            }
                            _ => false,
                        },
                        _ => false,
                    };
                    if done {
                        self.tabs[i].scene.reseed_derived_caches(*h);
                        changed.push((*h, crate::scene::ChangeKind::Modified));
                    }
                }
                if !changed.is_empty() {
                    self.tabs[i].scene.bump_entities(&changed);
                    self.tabs[i].dirty = true;
                }
                // The command used the set up; nothing stays chosen.
                self.tabs[i].scene.deselect_all();
                self.refresh_properties();
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
                let c =
                    ValuePromptCommand::new("ANNOALLVISIBLE", "ANNOALLVISIBLE  new value [0/1]:");
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
                let c =
                    ValuePromptCommand::new("ANNOAUTOSCALE", "ANNOAUTOSCALE  new value [-4..4]:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ANNOAUTOSCALE ") => {
                let value = cmd.split_whitespace().nth(1).unwrap_or("");
                match value.parse::<i8>() {
                    Ok(mode @ -4..=4) => self.annotation_auto_scale = mode,
                    _ => self.command_line.push_error(
                        crate::t!("ANNOAUTOSCALE: enter an integer from -4 through 4.").as_ref(),
                    ),
                }
            }
            "ANNOUPDATE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(handle, _)| *handle)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    self.command_line.push_error(
                        crate::t!("ANNOUPDATE: select annotation objects first.").as_ref(),
                    );
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "ANNOUPDATE");
                let scale = self.tabs[i].scene.creation_annotation_scale_handle();
                let paper_to_model = self.tabs[i].scene.creation_annotation_multiplier();
                let mut updated = 0usize;
                for handle in &handles {
                    if crate::scene::annotative::update_entity_from_annotation_style(
                        &mut self.tabs[i].scene.document,
                        *handle,
                        scale,
                        paper_to_model,
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
                    None => self.command_line.push_error(
                        crate::t!("Usage: ANNOSCALE <ratio>  e.g. 1:50, 2:1, or a factor").as_ref(),
                    ),
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
                        (
                            "Add",
                            "ADD",
                            Some("SCALELISTEDIT ADD  new scale ratio (e.g. 1:50):"),
                        ),
                        (
                            "Delete",
                            "DELETE",
                            Some("SCALELISTEDIT DELETE  scale ratio to remove:"),
                        ),
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
                                    self.command_line.push_output(
                                        crate::tf!("Added annotation scale {arg}.").as_ref(),
                                    );
                                } else {
                                    self.command_line.push_info(
                                        crate::tf!("Scale {arg} already exists.").as_ref(),
                                    );
                                }
                            }
                            _ => self.command_line.push_error(
                                crate::t!("SCALELISTEDIT ADD: use a ratio like 1:50.").as_ref(),
                            ),
                        },
                        None => self.command_line.push_error(
                            crate::t!("SCALELISTEDIT ADD: use a ratio like 1:50.").as_ref(),
                        ),
                    },
                    "DELETE" | "REMOVE" => {
                        let current = self.tabs[i]
                            .scene
                            .document
                            .header
                            .current_annotation_scale
                            .clone();
                        if arg.is_empty() {
                            self.command_line.push_info(
                                crate::t!("Usage: SCALELISTEDIT DELETE <name>").as_ref(),
                            );
                        } else if arg.eq_ignore_ascii_case(&current) {
                            self.command_line.push_error(
                                crate::tf!("Cannot delete the current annotation scale ({arg}).")
                                    .as_ref(),
                            );
                        } else {
                            self.push_undo_snapshot(i, "SCALELISTEDIT");
                            if self.tabs[i].scene.remove_scale(arg) {
                                self.tabs[i].dirty = true;
                                self.command_line.push_output(
                                    crate::tf!("Removed annotation scale {arg}.").as_ref(),
                                );
                            } else {
                                self.command_line.push_info(
                                    crate::tf!("No annotation scale named {arg}.").as_ref(),
                                );
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
                            self.command_line
                                .push_info(crate::t!("No annotation scales defined.").as_ref());
                        } else {
                            self.command_line.push_output(
                                crate::tf!("Annotation scales: {}", names.join(", ")).as_ref(),
                            );
                        }
                    }
                    _ => self.command_line.push_info(
                        crate::t!("Usage: SCALELISTEDIT [ADD 1:50 | DELETE 1:50]").as_ref(),
                    ),
                }
            }

            // OBJECTSCALE — open the Annotation Object Scale dialog for the
            // selected object (add / remove its per-object scale representations).
            // Reachable now that the immediate "add current scale" quick action
            // moved to the explicit `OBJECTSCALE ADD` keyword above.
            "OBJECTSCALE" => {
                return Some(Task::done(Message::AnnoObjectScaleOpen));
            }

            // Reset every alternate scale representation to the position of
            // the representation visible at the current annotation scale.
            "ANNORESET" => {
                let handles: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(handle, _)| *handle)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    let command = crate::modules::annotate::annotation_scale::AnnotationScaleSelectionCommand::new(
                        "ANNORESET",
                        "ANNORESET",
                    );
                    self.command_line
                        .push_info(&crate::command::CadCommand::prompt(&command));
                    self.tabs[i].active_cmd = Some(Box::new(command));
                    return Some(Task::none());
                }
                let scale = self.tabs[i].scene.creation_annotation_scale_handle();
                let candidates: Vec<_> = handles
                    .into_iter()
                    .filter(|handle| {
                        crate::scene::annotative::object_scale_memberships(
                            &self.tabs[i].scene.document,
                            *handle,
                        )
                        .len()
                            > 1
                    })
                    .collect();
                if candidates.is_empty() {
                    self.command_line.push_info(
                        crate::t!(
                            "ANNORESET: no selected object has alternate scale representations."
                        )
                        .as_ref(),
                    );
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "ANNORESET");
                let mut changed = Vec::new();
                for handle in candidates {
                    if crate::scene::annotative::reset_annotation_context_positions(
                        &mut self.tabs[i].scene.document,
                        handle,
                        scale,
                    ) {
                        changed.push(handle);
                    }
                }
                if !changed.is_empty() {
                    let changes: Vec<_> = changed
                        .iter()
                        .copied()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.tabs[i].dirty = true;
                }
                self.command_line.push_output(
                    crate::tf!(
                        "ANNORESET: synchronized {} object(s) from the current scale.",
                        changed.len()
                    )
                    .as_ref(),
                );
                return Some(Task::none());
            }

            // DATALINK <path.csv> — create a persistent linked table.
            "DATALINK" => {
                self.open_data_link_manager(false);
            }
            cmd if cmd.starts_with("DATALINK ") => {
                let path = cmd.trim_start_matches("DATALINK").trim();
                if path.is_empty() {
                    self.command_line
                        .push_info(crate::t!("Usage: DATALINK <path-to-.csv>").as_ref());
                    return Some(Task::none());
                }
                // Web: nothing is behind a typed path — pick the .csv through
                // the browser, read its bytes at pick time, and register them
                // in the picked-source registry; the same table-insert path
                // then runs from `DataLinkCsvPicked`.
                #[cfg(target_arch = "wasm32")]
                {
                    // Say the typed argument is superseded — a silent
                    // redirect looks like the input was lost.
                    if !path.trim().is_empty() {
                        self.command_line.push_info(
                            crate::t!(
                                "The browser cannot open a typed path; pick the file instead."
                            )
                            .as_ref(),
                        );
                    }
                    return Some(Task::perform(
                        crate::io::pick_datalink_csv_web(),
                        Message::DataLinkCsvPicked,
                    ));
                }
                #[cfg(not(target_arch = "wasm32"))]
                match std::fs::read_to_string(path) {
                    Ok(text) => {
                        let link_path = std::fs::canonicalize(path)
                            .map(|value| value.to_string_lossy().into_owned())
                            .unwrap_or_else(|_| path.to_string());
                        self.place_data_link_table(&text, &link_path);
                    }
                    Err(e) => {
                        self.command_line.push_error(
                            crate::tf!("DATALINK: cannot read \"{path}\": {e}").as_ref(),
                        );
                    }
                }
            }

            "DATALINKUPDATE" => {
                let names = crate::app::annotation_data::data_link_entries(
                    &self.tabs[i].scene.document,
                )
                .into_iter()
                .map(|(name, _)| name)
                .collect();
                let command =
                    crate::modules::annotate::data_link::DataLinkUpdateCommand::new(names);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            // DATALINKUPDATE _RUN <U|W> <ALL|SEL|NAME name> — the end of the
            // interactive command: update the chosen linked tables from their
            // sources, or write them back.
            cmd if cmd.starts_with("DATALINKUPDATE _RUN") => {
                let args = cmd.trim_start_matches("DATALINKUPDATE _RUN").trim();
                if args == "NONE" {
                    self.command_line.push_output(
                        crate::t!("No data links exist in the current drawing.").as_ref(),
                    );
                    self.command_line.push_output(
                        crate::t!("To create a data link, use the DATALINK command.").as_ref(),
                    );
                    return Some(Task::none());
                }
                let write = args.starts_with('W');
                let scope = args.get(2..).unwrap_or("").trim();
                let document = &self.tabs[i].scene.document;
                let link_of = |table: &codec::entities::Table| {
                    table
                        .rows
                        .iter()
                        .flat_map(|row| row.cells.iter())
                        .find_map(|cell| cell.data_link_handle)
                };
                let wanted_link = scope.strip_prefix("NAME ").and_then(|name| {
                    crate::app::annotation_data::data_link_entries(document)
                        .into_iter()
                        .find(|(known, _)| known.eq_ignore_ascii_case(name.trim()))
                        .map(|(_, handle)| handle)
                });
                let selected: Vec<codec::Handle> = if scope == "SEL" {
                    self.tabs[i]
                        .scene
                        .selected_entities()
                        .into_iter()
                        .map(|(handle, _)| handle)
                        .collect()
                } else {
                    Vec::new()
                };
                let jobs: Vec<(codec::Handle, codec::Handle)> = document
                    .entities()
                    .filter_map(|entity| match entity {
                        codec::EntityType::Table(table) => {
                            link_of(table).map(|link| (entity.common().handle, link))
                        }
                        _ => None,
                    })
                    .filter(|(table, link)| match scope {
                        "SEL" => selected.contains(table),
                        "ALL" => true,
                        _ => Some(*link) == wanted_link,
                    })
                    .collect();
                self.command_line.push_output(
                    crate::tf!("{} object(s) found.", jobs.len()).as_ref(),
                );
                if write {
                    let mut refused = 0usize;
                    let mut written = 0usize;
                    for (table_handle, link_handle) in &jobs {
                        let document = &self.tabs[i].scene.document;
                        if !crate::app::annotation_data::data_link_allows_write(
                            document,
                            *link_handle,
                        ) {
                            refused += 1;
                            continue;
                        }
                        let Some(codec::EntityType::Table(table)) =
                            document.get_entity(*table_handle)
                        else {
                            continue;
                        };
                        let csv = table_to_csv(table);
                        match crate::app::annotation_data::data_link_write_path(
                            document,
                            *link_handle,
                        )
                        .and_then(|path| {
                            std::fs::write(path, csv).map_err(|error| error.to_string())
                        }) {
                            Ok(()) => written += 1,
                            Err(error) => self.command_line.push_error(&error),
                        }
                    }
                    if refused > 0 {
                        self.command_line.push_output(
                            crate::tf!(
                                "{} data link(s) do not allow their data to be written out.",
                                refused
                            )
                            .as_ref(),
                        );
                    }
                    self.command_line.push_output(
                        crate::tf!("{} data link(s) written out successfully.", written).as_ref(),
                    );
                    return Some(Task::none());
                }
                let mut updates = Vec::new();
                let mut failed = 0usize;
                for (table_handle, link_handle) in &jobs {
                    let document = &self.tabs[i].scene.document;
                    // An extraction table is refreshed by running its saved
                    // extraction again, under its title row.
                    let (read, first_linked_row) =
                        match crate::app::annotation_data::extraction_table_rows(
                            document,
                            self.tabs[i].current_path.as_deref(),
                            *table_handle,
                            *link_handle,
                        ) {
                            Some(rows) => (rows, 1),
                            None => (
                                crate::app::annotation_data::read_data_link(
                                    document,
                                    *link_handle,
                                ),
                                0,
                            ),
                        };
                    match read {
                        Ok(rows) if !rows.is_empty() => {
                            updates.push((*table_handle, *link_handle, rows, first_linked_row))
                        }
                        _ => failed += 1,
                    }
                }
                let updated = self.apply_link_updates(i, updates);
                if failed > 0 {
                    self.command_line.push_output(
                        crate::tf!("{} object(s) could not be updated.", failed).as_ref(),
                    );
                }
                self.command_line.push_output(
                    crate::tf!(
                        "{} object(s) updated successfully.",
                        updated
                    )
                    .as_ref(),
                );
            }

            // LANDXMLIMPORT <path> — import survey points (LandXML <CgPoint>
            // elements) as Point objects. Reads the coordinate text content
            // (northing easting elevation) → Point at (easting, northing, elev).
            "LANDXMLIMPORT" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "LANDXMLIMPORT",
                    "LANDXMLIMPORT  path to the .xml file:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("LANDXMLIMPORT ") => {
                let path = cmd.trim_start_matches("LANDXMLIMPORT").trim();
                if path.is_empty() {
                    self.command_line.push_info(
                        crate::t!(
                            "Usage: LANDXMLIMPORT <path-to-.xml>  (imports CgPoint survey points)"
                        )
                        .as_ref(),
                    );
                    return Some(Task::none());
                }
                match std::fs::read_to_string(path) {
                    Ok(xml) => {
                        let pts = parse_landxml_cgpoints(&xml);
                        if pts.is_empty() {
                            self.command_line.push_info(
                                crate::t!("LANDXMLIMPORT: no <CgPoint> survey points found.")
                                    .as_ref(),
                            );
                            return Some(Task::none());
                        }
                        self.push_undo_snapshot(i, "LANDXMLIMPORT");
                        for [x, y, z] in &pts {
                            let mut p = codec::entities::Point::new();
                            p.location = codec::types::Vector3::new(*x, *y, *z);
                            self.tabs[i]
                                .scene
                                .add_entity_clone(codec::EntityType::Point(p));
                        }
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(
                            crate::tf!(
                            "LANDXMLIMPORT: imported {} survey point(s). Use ZOOM EXTENTS to view.",
                            pts.len()
                        )
                            .as_ref(),
                        );
                    }
                    Err(e) => self.command_line.push_error(
                        crate::tf!("LANDXMLIMPORT: cannot read \"{path}\": {e}").as_ref(),
                    ),
                }
            }

            "ULAYERS" => self.open_underlay_layers_dialog(i),
            "RECAP" | "SYNCPVIEWPORTS" => {
                self.command_line
                    .push_info(crate::tf!("{cmd}: not yet implemented.").as_ref());
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }

    /// Build the linked table from CSV text and start the placement command —
    /// the shared tail of the native typed-path `DATALINK` read and the web
    /// picker's `DataLinkCsvPicked` (which registers the picked bytes in the
    /// picked-source registry first so the link re-reads in-session).
    pub(in crate::app) fn place_data_link_table(&mut self, csv_text: &str, link_path: &str) {
        let i = self.active_tab;
        let rows_data = parse_csv_table(csv_text);
        let nrows = rows_data.len();
        let ncols = rows_data.iter().map(|r| r.len()).max().unwrap_or(0);
        if nrows == 0 || ncols == 0 {
            self.command_line
                .push_error(crate::t!("DATALINK: the CSV file is empty.").as_ref());
            return;
        }
        use codec::entities::TableBuilder;
        use codec::types::Vector3;
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
        let doc = &self.tabs[i].scene.document;
        let current_style = doc.header.current_table_style_name.clone();
        table.table_style_handle = doc
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                codec::objects::ObjectType::TableStyle(style)
                    if style.name.eq_ignore_ascii_case(&current_style) =>
                {
                    Some(*handle)
                }
                _ => None,
            });
        let command =
            crate::modules::annotate::data_link::DataLinkPlaceCommand::new(table, link_path);
        self.command_line
            .push_info(&crate::command::CadCommand::prompt(&command));
        self.tabs[i].active_cmd = Some(Box::new(command));
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

pub(crate) fn parse_csv_table(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => row.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                if field.ends_with('\r') {
                    field.pop();
                }
                row.push(std::mem::take(&mut field));
                if row.iter().any(|value| !value.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => field.push(ch),
        }
    }
    if field.ends_with('\r') {
        field.pop();
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|value| !value.is_empty()) {
            rows.push(row);
        }
    }
    rows
}

/// Replaces a linked table's cells with the rows read from its link, growing
/// or shrinking the grid to fit, and locks every cell to the link.
pub(crate) fn apply_link_rows(
    table: &mut codec::entities::Table,
    link_handle: codec::Handle,
    rows: &[Vec<String>],
    first_linked_row: usize,
) {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    while table.row_count() < rows.len() {
        // A new row repeats the row above it (height, cell formats); its
        // texts are written below.
        match table.rows.last().cloned() {
            Some(row) => table.rows.push(row),
            None => {
                table.add_row();
            }
        }
    }
    while table.row_count() > rows.len() {
        table.remove_row(table.row_count() - 1);
    }
    while table.column_count() < columns {
        let width = table
            .columns
            .last()
            .map(|column| column.width)
            .unwrap_or(2.0);
        table.add_column(width);
    }
    while table.column_count() > columns {
        table.remove_column(table.column_count() - 1);
    }
    for (row, values) in rows.iter().enumerate() {
        for column in 0..columns {
            if let Some(cell) = table.cell_mut(row, column) {
                cell.set_text(values.get(column).map(String::as_str).unwrap_or(""));
            }
        }
    }
    crate::modules::annotate::data_link::link_cells(table, link_handle, first_linked_row);
}

pub(crate) fn table_to_csv(table: &codec::entities::Table) -> String {
    fn escape(value: &str) -> String {
        if value.contains([',', '"', '\r', '\n']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    }
    table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| escape(cell.text_value()))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

impl OpenCADStudio {
    /// The ADJUST family's object prompt, keeping only the kinds `mode`
    /// takes (see `xadjust::keeps`).
    fn start_adjust_selection(&mut self, i: usize, name: &str, mode: &str) {
        use crate::command::CadCommand;
        use crate::modules::insert::xadjust;
        self.tabs[i].scene.deselect_all();
        self.refresh_properties();
        let command = crate::modules::draw::select::SelectObjectsCommand::filtered(
            name,
            &format!("_ADJUSTSEL {mode}"),
            xadjust::select_prompt(mode),
            xadjust::keeps(mode),
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[i].active_cmd = Some(Box::new(command));
    }

    /// The Image Adjust dialog for the chosen images, starting from the
    /// first one's values; nothing opens without an image.
    fn open_image_adjust(&mut self, i: usize) {
        let keep = crate::modules::insert::xadjust::keeps("I");
        let scene = &self.tabs[i].scene;
        let images: Vec<(codec::Handle, codec::EntityType)> = scene
            .selected_entities()
            .into_iter()
            .filter(|(h, e)| keep(e) && !scene.is_layer_locked(*h))
            .map(|(h, e)| (h, e.clone()))
            .collect();
        let Some((_, codec::EntityType::RasterImage(first))) = images.first() else {
            return;
        };
        let canvas = scene.bg_color;
        let state = crate::ui::window::pdf_dialogs::ImageAdjustState::new(
            images.iter().map(|(h, _)| *h).collect(),
            &first.file_path,
            [first.brightness, first.contrast, first.fade],
            [0, 1, 2].map(|k| (canvas[k].clamp(0.0, 1.0) * 255.0).round() as u8),
        );
        self.image_adjust = Some(state);
        self.active_modal = Some(crate::app::ModalKind::ImageAdjust);
    }

    pub(in crate::app) fn update_image_adjust(
        &mut self,
        message: crate::ui::window::pdf_dialogs::PdfDialogMsg,
    ) -> Task<Message> {
        use crate::ui::window::pdf_dialogs::{PdfDialogMsg as M, IMAGE_ADJUST_DEFAULTS};
        let i = self.active_tab;
        let Some(state) = self.image_adjust.as_mut() else {
            return Task::none();
        };
        match message {
            M::IaValue(row, value) => state.set(row as usize, value),
            M::IaStep(row, dir) => {
                let k = row as usize;
                state.set(k, state.values[k].saturating_add_signed(dir.signum()));
            }
            // A box takes whole numbers 0–100 only; anything else is not
            // typed in. An emptied box keeps the value until a number comes.
            M::IaText(row, typed) => {
                let k = row as usize;
                let typed = typed.trim().to_string();
                if typed.is_empty() {
                    state.texts[k] = typed;
                } else if let Some(value) = typed
                    .parse::<u8>()
                    .ok()
                    .filter(|v| *v <= 100 && typed.chars().all(|c| c.is_ascii_digit()))
                {
                    state.set(k, value);
                }
            }
            M::IaReset => state.set_all(IMAGE_ADJUST_DEFAULTS),
            M::IaApply => {
                let Some(state) = self.image_adjust.take() else {
                    return Task::none();
                };
                self.close_active_modal();
                let [brightness, contrast, fade] = state.values;
                self.push_undo_snapshot(i, "IMAGEADJUST");
                let mut changed = Vec::new();
                for h in &state.handles {
                    if let Some(codec::EntityType::RasterImage(img)) =
                        self.tabs[i].scene.document.get_entity_mut(*h)
                    {
                        img.brightness = brightness;
                        img.contrast = contrast;
                        img.fade = fade;
                        self.tabs[i].scene.reseed_derived_caches(*h);
                        changed.push((*h, crate::scene::ChangeKind::Modified));
                    }
                }
                if !changed.is_empty() {
                    self.tabs[i].scene.bump_entities(&changed);
                    self.tabs[i].dirty = true;
                }
                self.tabs[i].scene.deselect_all();
                self.refresh_properties();
            }
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;

    fn fresh_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn redraw_requests_and_leaves_geometry_untouched() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let geom_before = app.tabs[i].scene.geometry_epoch;
        let block_before = app.tabs[i].scene.block_epoch;
        let _ = app.run_command_line("REDRAW");
        assert!(
            app.tabs[i].scene.refresh_pending_any(),
            "REDRAW must leave a pending force request"
        );
        assert_eq!(
            app.tabs[i].scene.geometry_epoch, geom_before,
            "REDRAW must not regen"
        );
        assert_eq!(
            app.tabs[i].scene.block_epoch, block_before,
            "REDRAW must not regen blocks"
        );
    }

    #[test]
    fn aliases_route_like_full_verbs() {
        let mut full = fresh_app();
        let mut short = fresh_app();
        let _ = full.run_command_line("REDRAW");
        let _ = short.run_command_line("R");
        let i = full.active_tab;
        assert!(
            short.tabs[i].scene.refresh_pending_any(),
            "'R' must trigger REDRAW"
        );
        assert_eq!(
            short.tabs[i].scene.refresh_pending_any(),
            full.tabs[i].scene.refresh_pending_any(),
            "'R' and 'REDRAW' must leave the same refresh state"
        );
    }

    #[test]
    fn redrawall_marks_all_tiles() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let _ = app.run_command_line("REDRAWALL");
        assert!(
            app.tabs[i].scene.refresh_pending_any(),
            "REDRAWALL must leave a force request"
        );
    }

    #[test]
    fn regen_rebuilds_but_does_not_dirty_document() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let geom_before = app.tabs[i].scene.geometry_epoch;
        let block_before = app.tabs[i].scene.block_epoch;
        app.tabs[i].dirty = false;
        let _ = app.run_command_line("REGEN");
        assert_ne!(
            app.tabs[i].scene.geometry_epoch, geom_before,
            "REGEN must regenerate geometry"
        );
        assert_ne!(
            app.tabs[i].scene.block_epoch, block_before,
            "REGEN must regenerate block epoch"
        );
        assert!(
            !app.tabs[i].dirty,
            "REGEN must NOT mark the document as modified (no DB change)"
        );
        let _ = app.run_command_line("REGENALL");
        assert!(
            !app.tabs[i].dirty,
            "REGENALL must not dirty the document either"
        );
    }

    #[test]
    fn regen_rebuilds_the_mesh_map_rather_than_only_bumping_the_epoch() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let stale = codec::Handle::new(0xDEAD);
        app.tabs[i].scene.meshes.insert(stale, stale_mesh());
        let epoch_before = app.tabs[i].scene.geometry_epoch;

        let _ = app.run_command_line("REGEN");

        assert!(
            !app.tabs[i].scene.meshes.contains_key(&stale),
            "REGEN left a stale mesh",
        );
        assert_ne!(
            app.tabs[i].scene.geometry_epoch, epoch_before,
            "REGEN did not bump the geometry epoch",
        );
    }

    #[test]
    fn the_isolines_slider_rebuilds_once_on_release_and_not_while_dragging() {
        use crate::app::Message;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let seed = |app: &mut OpenCADStudio, i: usize| {
            let stale = codec::Handle::new(0xBEEF);
            app.tabs[i].scene.meshes.insert(stale, stale_mesh());
            stale
        };
        let stale = seed(&mut app, i);

        for value in [4i16, 3, 2, 1, 0] {
            let _ = app.update(Message::IsolinesChanged(value));
        }
        assert!(
            app.tabs[i].scene.meshes.contains_key(&stale),
            "the drag itself must not rebuild the meshes",
        );
        assert_eq!(app.tabs[i].scene.document.header.isolines, 0);

        let _ = app.update(Message::IsolinesReleased);
        assert!(
            !app.tabs[i].scene.meshes.contains_key(&stale),
            "releasing after a change rebuilds them",
        );

        let stale = seed(&mut app, i);
        let _ = app.update(Message::IsolinesChanged(0));
        let _ = app.update(Message::IsolinesReleased);
        assert!(
            app.tabs[i].scene.meshes.contains_key(&stale),
            "releasing without a change must rebuild nothing",
        );
    }

    fn stale_mesh() -> crate::scene::model::mesh_model::MeshLodSet {
        crate::scene::model::mesh_model::MeshLodSet {
            lods: Vec::new(),
            material: None,
            face_materials: Default::default(),
            visual_style: None,
            complete: true,
            edge_verts: Vec::new(),
            edge_verts_low: Vec::new(),
            curved_gens: Vec::new(),
            metrics: Default::default(),
            world_aabb: [0.0; 4],
            z_aabb: [0.0; 2],
            instance_source: None,
            instance_transform: None,
            instance_handle: None,
            instance_color: None,
            edge_color: None,
            instance_aabb: None,
        }
    }
}

#[cfg(test)]
mod printers_command_tests {
    use crate::app::OpenCADStudio;

    #[test]
    fn printers_reports_the_default_and_the_list() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let before = app.command_line.history.len();
        let _ = app.run_command_line("PRINTERS");
        let lines: Vec<String> = app.command_line.history[before..]
            .iter()
            .map(|line| line.text.clone())
            .collect();
        // One line for the default printer, at least one for the list (a
        // count and names, "none", or the system's error).
        assert!(lines.len() >= 2, "{lines:?}");
    }
}
