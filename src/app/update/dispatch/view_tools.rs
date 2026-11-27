use super::super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_view_tools(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::ScaleManagerSelect(name) => {
                // Ctrl toggles a row, Shift picks the run from the last
                // clicked row, a plain click picks the row alone.
                let i = self.active_tab;
                let order = self.tabs[i].scene.scale_names_in_list_order();
                let at = |n: &str| order.iter().position(|o| o.eq_ignore_ascii_case(n));
                let anchor = self.scale_ui.selected.clone();
                if self.shift_down && !anchor.is_empty() {
                    if let (Some(a), Some(b)) = (at(&anchor), at(&name)) {
                        let (a, b) = if a <= b { (a, b) } else { (b, a) };
                        self.scale_ui.multi = order[a..=b].to_vec();
                    }
                    return Task::none();
                }
                if self.ctrl_down {
                    if let Some(pos) = self
                        .scale_ui.multi
                        .iter()
                        .position(|s| s.eq_ignore_ascii_case(&name))
                    {
                        self.scale_ui.multi.remove(pos);
                        // Edit and Move act on the selected row: the last
                        // one still picked, not the one just dropped.
                        if let Some(last) = self.scale_ui.multi.last().cloned() {
                            self.load_scale_editor(&last);
                        }
                        return Task::none();
                    }
                    self.scale_ui.multi.push(name.clone());
                } else {
                    self.scale_ui.multi = vec![name.clone()];
                }
                self.load_scale_editor(&name);
                Task::none()
            }
            Message::ScaleManagerPaperBuf(s) => {
                if self.scale_card_units_locked() {
                    return Task::none();
                }
                self.scale_ui.paper_buf = s;
                self.scale_ui.edit_error = None;
                Task::none()
            }
            Message::ScaleManagerDrawingBuf(s) => {
                if self.scale_card_units_locked() {
                    return Task::none();
                }
                self.scale_ui.drawing_buf = s;
                self.scale_ui.edit_error = None;
                Task::none()
            }
            Message::ScaleManagerNew => {
                // Add opens the card with a fresh name and 1 : 1; the scale
                // joins the list's end only when the card is applied.
                // The name starts empty as the reference's Add Scale does.
                self.scale_ui.edit_adding = true;
                self.scale_ui.edit_error = None;
                self.scale_ui.rename_buf = String::new();
                self.scale_ui.rename = Some(String::new());
                self.scale_ui.paper_buf = "1".into();
                self.scale_ui.drawing_buf = "1".into();
                iced::widget::operation::focus(crate::ui::style::scale_manager::rename_input_id())
            }
            Message::ScaleManagerEdit => {
                let name = self.scale_ui.selected.clone();
                if name.is_empty() || self.scale_ui.multi.len() > 1 {
                    return Task::none();
                }
                self.update(Message::ScaleRenameStart(name))
            }
            Message::ScaleManagerEditCancel => {
                self.scale_ui.rename = None;
                self.scale_ui.edit_error = None;
                self.scale_ui.edit_adding = false;
                let selected = self.scale_ui.selected.clone();
                self.load_scale_editor(&selected);
                Task::none()
            }
            Message::ScaleManagerCopy => {
                // Duplicate the selected scale under a unique name (staged).
                self.scale_apply_current();
                let i = self.active_tab;
                let sel = self.scale_ui.selected.clone();
                if !sel.is_empty() {
                    let (paper, drawing) = self.tabs[i]
                        .scene
                        .scale_paper_drawing(&sel)
                        .unwrap_or((1.0, 1.0));
                    let name = self.unique_scale_name(&sel);
                    if self.tabs[i].scene.add_scale(&name, paper, drawing) {
                        self.load_scale_editor(&name);
                        self.scale_stage_mark();
                    }
                }
                Task::none()
            }
            Message::ScaleRenameStart(name) => {
                // Double-click or Edit...: the card edits this scale.
                self.scale_ui.edit_adding = false;
                self.scale_ui.edit_error = None;
                self.scale_ui.multi = vec![name.clone()];
                self.load_scale_editor(&name);
                self.scale_ui.rename_buf = name.clone();
                self.scale_ui.rename = Some(name);
                iced::widget::operation::focus(crate::ui::style::scale_manager::rename_input_id())
            }
            Message::ScaleRenameEdit(s) => {
                self.scale_ui.rename_buf = s;
                self.scale_ui.edit_error = None;
                Task::none()
            }
            Message::ScaleRenameCommit => {
                let i = self.active_tab;
                // Refusals as the reference words them: a missing name, a
                // name another scale has, then a ratio that is not positive.
                {
                    let name = self.scale_ui.rename_buf.trim().to_string();
                    let old = self.scale_ui.rename.clone().unwrap_or_default();
                    let units_open = self.scale_ui.edit_adding || !self.scale_units_locked(i, &old);
                    let number = |text: &str| text.trim().parse::<f64>().ok().filter(|v| *v > 0.0);
                    let refusal = if name.is_empty() {
                        Some(
                            crate::t!("No name specified for this scale.  Please enter a name.")
                                .into_owned(),
                        )
                    } else if !name.eq_ignore_ascii_case(&old)
                        && self.tabs[i].scene.scale_paper_drawing(&name).is_some()
                    {
                        Some(crate::tf!("{} already exists.", name).into_owned())
                    } else if units_open
                        && (number(&self.scale_ui.paper_buf).is_none()
                            || number(&self.scale_ui.drawing_buf).is_none())
                    {
                        Some(crate::t!("Invalid scale number.").into_owned())
                    } else {
                        None
                    };
                    if refusal.is_some() {
                        self.scale_ui.edit_error = refusal;
                        return Task::none();
                    }
                    self.scale_ui.edit_error = None;
                    if !units_open {
                        // Locked units: the stored ratio stays.
                        let selected = self.scale_ui.selected.clone();
                        self.load_scale_editor(&selected);
                        self.scale_ui.rename_buf = name;
                    }
                }
                if self.scale_ui.edit_adding {
                    // Add: a unique name and a positive ratio make the scale.
                    let name = self.scale_ui.rename_buf.trim().to_string();
                    let paper = self.scale_ui.paper_buf.trim().parse::<f64>().ok();
                    let drawing = self.scale_ui.drawing_buf.trim().parse::<f64>().ok();
                    let (Some(paper), Some(drawing)) = (paper, drawing) else {
                        return Task::none();
                    };
                    if name.is_empty()
                        || paper <= 0.0
                        || drawing <= 0.0
                        || self.tabs[i].scene.scale_paper_drawing(&name).is_some()
                    {
                        return Task::none();
                    }
                    if self.tabs[i].scene.add_scale(&name, paper, drawing) {
                        self.scale_stage_mark();
                        self.scale_ui.edit_adding = false;
                        self.scale_ui.rename = None;
                        self.scale_ui.multi = vec![name.clone()];
                        self.load_scale_editor(&name);
                    }
                    return Task::none();
                }
                // Edit: the ratio first, then the name.
                self.scale_apply_current();
                if let Some(old) = self.scale_ui.rename.take() {
                    let new = self.scale_ui.rename_buf.trim().to_string();
                    if !new.is_empty() && !new.eq_ignore_ascii_case(&old) {
                        let (paper, drawing) = self.tabs[i]
                            .scene
                            .scale_paper_drawing(&old)
                            .unwrap_or((1.0, 1.0));
                        // Only fall back to add_scale for a built-in fallback (no
                        // real object); never for a real scale whose rename was
                        // rejected (name collision) — that would duplicate it.
                        let ok = self.tabs[i].scene.edit_scale(&old, &new, paper, drawing)
                            || (self.tabs[i].scene.scale_paper_drawing(&old).is_none()
                                && self.tabs[i].scene.add_scale(&new, paper, drawing));
                        if ok {
                            if self.tabs[i]
                                .scene
                                .document
                                .header
                                .current_annotation_scale
                                .eq_ignore_ascii_case(&old)
                            {
                                self.tabs[i].scene.document.header.current_annotation_scale =
                                    new.clone();
                            }
                            if self.scale_ui.selected.eq_ignore_ascii_case(&old) {
                                self.load_scale_editor(&new);
                            }
                            self.scale_stage_mark();
                        }
                    }
                }
                Task::none()
            }
            Message::ScaleManagerMove(step) => {
                if self.scale_ui.multi.len() > 1 {
                    return Task::none();
                }
                let i = self.active_tab;
                let selected = self.scale_ui.selected.clone();
                if self.tabs[i].scene.move_scale(&selected, step) {
                    self.scale_stage_mark();
                }
                Task::none()
            }
            Message::ScaleManagerReset => {
                self.scale_ui.reset_asking = true;
                Task::none()
            }
            Message::ScaleManagerResetChoice(choice) => {
                self.scale_ui.reset_asking = false;
                if choice < 3 {
                    // Reset takes effect at once, as the reference's does:
                    // the scales in use stay, the rest go, the chosen default
                    // list is added after them.
                    let i = self.active_tab;
                    let keep: Vec<String> = self.tabs[i]
                        .scene
                        .scale_names_in_list_order()
                        .into_iter()
                        .filter(|name| self.anno_scale_referenced(i, name))
                        .collect();
                    self.tabs[i]
                        .scene
                        .reset_scale_list_to(&keep, choice != 1, choice != 0);
                    self.scale_stage_mark();
                    self.scale_stage_commit();
                    let first = self.tabs[i]
                        .scene
                        .scale_names_in_list_order()
                        .into_iter()
                        .next();
                    if let Some(first) = first {
                        self.scale_ui.multi = vec![first.clone()];
                        self.load_scale_editor(&first);
                    }
                }
                Task::none()
            }
            Message::ScaleManagerOk => {
                self.scale_stage_commit();
                self.close_active_modal();
                Task::none()
            }
            Message::ScaleManagerApply => {
                // Fold the editor into the selected scale, then commit the staged
                // transaction (this edit plus any New / Copy / Delete since open)
                // as one undo entry.
                self.scale_apply_current();
                self.scale_stage_commit();
                Task::none()
            }
            Message::ScaleManagerDelete => {
                // Staged: reverted on close unless OK commits it. A scale in
                // use is never deleted; with some in use among the picked
                // ones the window asks before dropping the rest.
                let i = self.active_tab;
                let picked = self.scale_ui.multi.clone();
                let used = picked
                    .iter()
                    .filter(|name| self.anno_scale_referenced(i, name))
                    .count();
                if picked.is_empty() || used == picked.len() {
                    return Task::none();
                }
                if used > 0 {
                    self.scale_ui.delete_asking = true;
                    return Task::none();
                }
                self.update(Message::ScaleManagerDeleteChoice(true))
            }
            Message::ScaleManagerDeleteChoice(delete) => {
                self.scale_ui.delete_asking = false;
                if delete {
                    let i = self.active_tab;
                    let picked = std::mem::take(&mut self.scale_ui.multi);
                    let unused: Vec<String> = picked
                        .into_iter()
                        .filter(|name| !self.anno_scale_referenced(i, name))
                        .collect();
                    let mut removed = false;
                    for name in &unused {
                        removed |= self.tabs[i].scene.remove_scale(name);
                    }
                    if removed {
                        self.scale_ui.selected.clear();
                        self.scale_ui.paper_buf.clear();
                        self.scale_ui.drawing_buf.clear();
                        self.scale_stage_mark();
                    }
                }
                Task::none()
            }
            Message::ScaleManagerSetCurrent => {
                // Set Current takes effect immediately, exactly like the scale
                // pill — it isn't part of the staged list transaction, so it is
                // never rolled back when the manager closes.
                let i = self.active_tab;
                let sel = self.scale_ui.selected.clone();
                let previous = self.tabs[i].scene.displayed_annotation_scale_handle();
                if let Some(scale) = self.tabs[i].scene.set_annotation_scale_named(&sel) {
                    if self.annotation_auto_scale > 0 {
                        self.tabs[i].scene.add_annotation_scale_to_objects(
                            scale,
                            previous,
                            self.annotation_auto_scale as u8,
                        );
                    }
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }
            Message::ToggleLayoutList => {
                if self.tabs[self.active_tab].is_start {
                    self.layout_list_open = false;
                    return Task::none();
                }
                self.layout_list_open ^= true;
                Task::none()
            }
            Message::CloseLayoutList => {
                self.layout_list_open = false;
                Task::none()
            }
            Message::ToggleStatusBarMenu => {
                self.statusbar_menu_open ^= true;
                Task::none()
            }
            Message::CloseStatusBarMenu => {
                self.statusbar_menu_open = false;
                Task::none()
            }
            Message::ToggleStatusPill(pill) => {
                // Keep the menu open so several pills can be toggled in a row.
                self.statusbar_config.toggle(pill);
                self.save_config();
                Task::none()
            }
            Message::ToggleCleanScreen => {
                self.clean_screen ^= true;
                self.clean_screen_properties = false;
                self.clean_screen_parameters_manager = false;
                self.clean_screen_authoring_palettes = false;
                self.clean_screen_tool_palettes = false;
                if self.clean_screen {
                    // The panel goes with the chrome; its list cannot stay open.
                    self.tabs[self.active_tab].properties.open_block_picker = None;
                    self.tabs[self.active_tab].properties.open_choice = None;
                }
                Task::none()
            }
            Message::ToggleTransparencyDisplay => {
                let i = self.active_tab;
                if i < self.tabs.len() {
                    // No retessellate — the wire shader reads the flag from uniforms.
                    self.tabs[i].scene.transparency_display ^= true;
                }
                Task::none()
            }
            Message::ToggleQuickProperties => {
                self.quick_properties ^= true;
                // The Quick Properties copy of the row, and its list, go with
                // the panel.
                self.tabs[self.active_tab].properties.open_block_picker = None;
                self.tabs[self.active_tab].properties.open_choice = None;
                if self.quick_properties {
                    self.quick_properties_anchor = self.tabs[self.active_tab].last_cursor_screen;
                }
                self.save_config();
                Task::none()
            }
            Message::ToggleSelectionCycling => {
                self.toggle_selection_cycling(self.selection_cycling <= 0);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::CycleSelect(handle) => {
                let Some(cycle) = self.cycle_candidates.take() else {
                    return Task::none();
                };
                let i = self.active_tab;
                if cycle.tab_id != self.tabs[i].id || !cycle.handles.contains(&handle) {
                    return Task::none();
                }
                self.tabs[i].scene.set_hover_highlight(None);
                if let Some((shift, remove)) = cycle.modifiers {
                    if !self.commit_point_selection(i, handle, shift, remove) {
                        return Task::none();
                    }
                    self.quick_properties_anchor = cycle.point;
                    if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|cmd| cmd.is_selection_gathering())
                    {
                        let handles = self.tabs[i]
                            .scene
                            .selected_entities()
                            .into_iter()
                            .map(|(handle, _)| handle)
                            .collect();
                        return self
                            .feed_command(crate::command::StepInput::SelectionComplete(handles));
                    }
                } else {
                    self.tabs[i].scene.select_entity(handle, false);
                    self.tabs[i].scene.expand_selection_for_groups(&[handle]);
                    self.queue_properties_refresh(i);
                    self.quick_properties_anchor = cycle.point;
                }
                Task::none()
            }
            Message::CycleHover(handle) => {
                let i = self.active_tab;
                self.tabs[i].scene.set_hover_highlight(handle);
                Task::none()
            }
            Message::CycleHoverExit(handle) => {
                // Only clear if another row hasn't already taken the highlight;
                // enter/exit can fire out of order when moving between rows.
                let i = self.active_tab;
                if self.tabs[i].scene.hover_highlight == Some(handle) {
                    self.tabs[i].scene.set_hover_highlight(None);
                }
                Task::none()
            }
            Message::CycleCancel => {
                self.cycle_candidates = None;
                self.tabs[self.active_tab].scene.set_hover_highlight(None);
                Task::none()
            }
            Message::ToggleSelectionFilterPopup => {
                self.selection_filter_popup_open ^= true;
                Task::none()
            }
            Message::CloseSelectionFilterPopup => {
                self.selection_filter_popup_open = false;
                Task::none()
            }
            Message::ToggleSelectionFilterType(name) => {
                let f = &mut self.tabs[self.active_tab].scene.selection_filter;
                if !f.remove(&name) {
                    f.insert(name);
                }
                Task::none()
            }
            Message::SelectionFilterSelectAll => {
                self.tabs[self.active_tab].scene.selection_filter.clear();
                Task::none()
            }
            Message::SelectionFilterClearAll => {
                let i = self.active_tab;
                let types = self.tabs[i].scene.entity_type_names_in_layout();
                let f = &mut self.tabs[i].scene.selection_filter;
                for t in types.iter() {
                    f.insert(t.clone());
                }
                Task::none()
            }
            Message::ToggleUnitsPopup => {
                self.units_popup_open ^= true;
                Task::none()
            }
            Message::CloseUnitsPopup => {
                self.units_popup_open = false;
                Task::none()
            }
            Message::OpenDrawingUnits => {
                self.units_popup_open = false;
                let header = &self.tabs[self.active_tab].scene.document.header;
                self.drawing_units = Some(crate::ui::window::drawing_units::State {
                    linear_format: header.linear_unit_format,
                    linear_precision: header.linear_unit_precision,
                    angular_format: header.angular_unit_format,
                    angular_precision: header.angular_unit_precision,
                    clockwise: header.angle_direction != 0,
                    base_angle: format!("{:.6}", header.angle_base.to_degrees())
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string(),
                    insertion_units: header.insertion_units,
                });
                self.active_modal = Some(crate::app::ModalKind::DrawingUnits);
                Task::none()
            }
            Message::DrawingUnitsField(field) => {
                use crate::ui::window::drawing_units::Field;
                let Some(state) = self.drawing_units.as_mut() else {
                    return Task::none();
                };
                match field {
                    Field::LinearFormat(v) => state.linear_format = v,
                    Field::LinearPrecision(v) => state.linear_precision = v,
                    Field::AngularFormat(v) => state.angular_format = v,
                    Field::AngularPrecision(v) => state.angular_precision = v,
                    Field::Clockwise(v) => state.clockwise = v,
                    // Kept as typed so a lone "-" or a trailing "." survives
                    // until the number it is becoming is finished.
                    Field::BaseAngle(v) => state.base_angle = v,
                    Field::InsertionUnits(v) => state.insertion_units = v,
                }
                Task::none()
            }
            Message::DrawingUnitsApply => {
                let Some(state) = self.drawing_units.take() else {
                    self.active_modal = None;
                    return Task::none();
                };
                self.active_modal = None;
                let i = self.active_tab;
                self.push_undo_snapshot(i, "UNITS");
                let header = &mut self.tabs[i].scene.document.header;
                header.linear_unit_format = state.linear_format;
                header.linear_unit_precision = state.linear_precision;
                header.angular_unit_format = state.angular_format;
                header.angular_unit_precision = state.angular_precision;
                header.angle_direction = i16::from(state.clockwise);
                // A base angle that will not parse is a half-finished edit, not
                // an instruction to move zero — leave the drawing's own value.
                if let Ok(degrees) = state.base_angle.trim().parse::<f64>() {
                    header.angle_base = degrees.to_radians();
                }
                let units_before = header.insertion_units;
                header.insertion_units = state.insertion_units;
                self.tabs[i].scene.insertion_units_changed(units_before);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.bump_geometry();
                self.refresh_properties();
                crate::entities::common::set_unit_context(
                    crate::entities::common::UnitContext::from_header(
                        &self.tabs[i].scene.document.header,
                    ),
                );
                // Same relabel-only note as the status-bar unit menu: a new
                // unit with the geometry sitting still reads as a no-op. (#3683)
                if state.insertion_units != units_before {
                    let label = crate::modules::draw::units::label(state.insertion_units);
                    self.command_line.push_output(
                        crate::tf!(
                            "Drawing unit is now {label}. Geometry unchanged — use DWGUNITS to convert it."
                        )
                        .as_ref(),
                    );
                }
                Task::none()
            }
            Message::BlockDefName(name) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.name = name;
                    state.error_message = None;
                    state.confirm_redefine = None;
                }
                self.refresh_block_definition_preview();
                Task::none()
            }
            Message::BlockDefNameSelect(chosen) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.name = chosen.clone();
                    state.error_message = None;
                    state.confirm_redefine = None;
                    // Redefining an existing block starts from the base point
                    // that definition already has, printed at the drawing's
                    // precision; a name that is not a definition starts at zero.
                    let base = crate::scene::render_graph::block_base_point(
                        &self.tabs[self.active_tab].scene.document,
                        &chosen,
                    );
                    state.set_base_point(glam::DVec3::new(base.x, base.y, base.z));
                }
                self.refresh_block_definition_preview();
                Task::none()
            }
            Message::BlockDefBaseOnScreen(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.base_point_specify_onscreen = val;
                }
                Task::none()
            }
            Message::BlockDefPickPoint => {
                self.active_modal = None;
                let cmd = crate::modules::insert::create_block::BlockPickBasePointCommand;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(cmd));
                Task::none()
            }
            Message::BlockDefBaseX(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.base_point_x = val;
                }
                Task::none()
            }
            Message::BlockDefBaseY(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.base_point_y = val;
                }
                Task::none()
            }
            Message::BlockDefBaseZ(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.base_point_z = val;
                }
                Task::none()
            }
            Message::BlockDefObjectsOnScreen(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.objects_specify_onscreen = val;
                }
                Task::none()
            }
            Message::BlockDefSelectObjects => {
                self.active_modal = None;
                use crate::modules::draw::select::SelectObjectsCommand;
                let cmd = SelectObjectsCommand::plain("BLOCK", "BLOCK_OBJECTS_GATHERED");
                self.command_line.push_info(&cmd.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(cmd));
                Task::none()
            }
            Message::BlockDefQuickSelect => {
                self.active_modal = None;
                self.on_qselect_open()
            }
            Message::BlockDefObjectMode(mode) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.object_mode = mode;
                }
                Task::none()
            }
            Message::BlockDefAnnotative(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.annotative = val;
                    if !val {
                        state.match_orientation = false;
                    }
                }
                Task::none()
            }
            Message::BlockDefMatchOrientation(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.match_orientation = val;
                }
                Task::none()
            }
            Message::BlockDefScaleUniformly(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.scale_uniformly = val;
                }
                Task::none()
            }
            Message::BlockDefAllowExploding(val) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.allow_exploding = val;
                }
                Task::none()
            }
            Message::BlockDefUnit(unit) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.unit = unit;
                }
                Task::none()
            }
            Message::BlockDefDescription(desc) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.description = desc;
                }
                Task::none()
            }
            Message::BlockDefDescriptionAction(action) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.description_content.perform(action);
                    state.description = state.description_content.text();
                }
                Task::none()
            }
            Message::BlockDefHyperlink => {
                self.hyperlink_editor_block = None;
                if let Some(state) = self.block_definition.as_ref() {
                    self.hyperlink_editor_url = state.hyperlink_url.clone();
                    self.hyperlink_editor_description = state.hyperlink_desc.clone();
                    self.hyperlink_editor_mixed = false;
                    self.hyperlink_editor_dirty = false;
                    self.hyperlink_editor_handles.clear();
                    self.hyperlink_options =
                        crate::ui::window::hyperlink_dialog::HyperlinkOptions {
                            edit: !self.hyperlink_editor_url.is_empty(),
                            dwf: true,
                            address_only: true,
                            ..Default::default()
                        };
                    self.active_modal = Some(crate::app::ModalKind::Hyperlink);
                }
                Task::none()
            }
            Message::BlockDefDismissError => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.error_message = None;
                }
                Task::none()
            }
            Message::BlockDefOpenInEditor(on) => {
                if let Some(state) = self.block_definition.as_mut() {
                    state.open_in_editor = on;
                }
                Task::none()
            }
            Message::BlockDefHelp => {
                self.command_line.push_info(
                    crate::t!("BLOCK creates a block definition from objects you select.").as_ref(),
                );
                Task::none()
            }
            Message::BlockDefConfirmRedefine(confirmed) => {
                if !confirmed {
                    if let Some(state) = self.block_definition.as_mut() {
                        state.confirm_redefine = None;
                    }
                    return Task::none();
                }
                self.commit_block_definition(true)
            }
            m @ (Message::BlockDefApply | Message::BlockDefCreateEmpty) => {
                let create_empty = matches!(m, Message::BlockDefCreateEmpty);
                let Some(state) = self.block_definition.as_mut() else {
                    return Task::none();
                };
                let name = state.name.trim().to_string();
                if name.is_empty() {
                    state.error_message = Some(
                        crate::t!("The name must be at least one character long.").into_owned(),
                    );
                    return Task::none();
                }
                if let Some(_ch) = state.invalid_name_char() {
                    state.error_message = Some(
                        crate::t!("The name contains unsupported characters. The following characters are not supported: \\ < > / ? \" : ; * | , = `")
                            .into_owned(),
                    );
                    return Task::none();
                }
                if name.starts_with('*') {
                    state.error_message =
                        Some(crate::t!("Block name cannot start with '*'.").into_owned());
                    return Task::none();
                }
                // No objects: the band offers to select some or to define
                // an empty block.
                if !state.objects_specify_onscreen
                    && state.selected_handles.is_empty()
                    && !create_empty
                {
                    state.empty_prompt = true;
                    return Task::none();
                }
                state.empty_prompt = false;
                let i = self.active_tab;
                if self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&name)
                    .is_some()
                {
                    state.confirm_redefine = Some(name);
                    return Task::none();
                }
                self.commit_block_definition(false)
            }
            Message::WblockSourceMode(mode) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.source_mode = mode;
                }
                Task::none()
            }
            Message::WblockBlockName(name) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.block_name = name;
                }
                Task::none()
            }
            Message::WblockBlockSelect(name) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.block_name = name.clone();
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        let current_path = std::path::Path::new(&state.file_path);
                        let file_stem = current_path.file_stem().and_then(|s| s.to_str());
                        let is_default_or_block = file_stem == Some("new_block")
                            || state
                                .existing_blocks
                                .iter()
                                .any(|b| Some(b.as_str()) == file_stem);
                        if is_default_or_block {
                            let new_file_name = format!("{}.dwg", trimmed);
                            if let Some(parent) = current_path.parent() {
                                if !parent.as_os_str().is_empty() {
                                    state.file_path =
                                        parent.join(new_file_name).to_string_lossy().to_string();
                                } else {
                                    state.file_path = new_file_name;
                                }
                            } else {
                                state.file_path = new_file_name;
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::WblockPickPoint => {
                self.active_modal = None;
                let cmd = crate::modules::insert::wblock::WblockPickBasePointCommand;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(cmd));
                Task::none()
            }
            Message::WblockBaseX(val) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.base_point_x = val;
                }
                Task::none()
            }
            Message::WblockBaseY(val) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.base_point_y = val;
                }
                Task::none()
            }
            Message::WblockBaseZ(val) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.base_point_z = val;
                }
                Task::none()
            }
            Message::WblockSelectObjects => {
                self.active_modal = None;
                use crate::modules::draw::select::SelectObjectsCommand;
                let cmd = SelectObjectsCommand::plain("WBLOCK", "WBLOCK_OBJECTS_GATHERED");
                self.command_line.push_info(&cmd.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(cmd));
                Task::none()
            }
            Message::WblockQuickSelect => {
                self.active_modal = None;
                self.on_qselect_open()
            }
            Message::WblockObjectMode(mode) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.object_mode = mode;
                }
                Task::none()
            }
            Message::WblockFilePath(path) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.file_path = path;
                }
                Task::none()
            }
            Message::WblockBrowsePath => {
                let default_name = if let Some(state) = self.wblock.as_ref() {
                    let p = std::path::Path::new(&state.file_path);
                    p.file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or("new_block.dwg")
                        .to_string()
                } else {
                    "new_block.dwg".to_string()
                };
                Task::perform(
                    async move {
                        let path = crate::sys::file_dialog()
                            .set_title(crate::t!("Save Block As").as_ref())
                            .set_file_name(&default_name)
                            .add_filter(crate::t!("DWG Files").as_ref(), &["dwg"])
                            .add_filter(crate::t!("DXF Files").as_ref(), &["dxf"])
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h));
                        path
                    },
                    Message::WblockBrowsePathResult,
                )
            }
            Message::WblockBrowsePathResult(opt_path) => {
                if let Some(path) = opt_path {
                    if let Some(state) = self.wblock.as_mut() {
                        state.file_path = path.to_string_lossy().to_string();
                    }
                }
                Task::none()
            }
            Message::WblockUnit(unit) => {
                if let Some(state) = self.wblock.as_mut() {
                    state.unit = unit;
                }
                Task::none()
            }
            Message::WblockDismissError => {
                if let Some(state) = self.wblock.as_mut() {
                    state.error_message = None;
                }
                Task::none()
            }
            Message::WblockHelp => {
                self.command_line.push_info(
                    crate::t!("WBLOCK writes objects, a block, or the entire drawing to a new drawing file.").as_ref(),
                );
                Task::none()
            }
            Message::WblockApply => {
                if self.read_only {
                    self.wblock = None;
                    self.active_modal = None;
                    self.command_line.push_error(
                        crate::t!("Read-only session (--read-only): saving is disabled.").as_ref(),
                    );
                    return Task::none();
                }
                let Some(state) = self.wblock.as_mut() else {
                    return Task::none();
                };
                use crate::ui::window::wblock::{WblockObjectMode, WblockSourceMode};
                match state.source_mode {
                    WblockSourceMode::Block => {
                        let name = state.block_name.trim();
                        if name.is_empty() {
                            state.error_message = Some(
                                crate::t!("Please select or enter a block name.").into_owned(),
                            );
                            return Task::none();
                        }
                        let i = self.active_tab;
                        if self.tabs[i]
                            .scene
                            .document
                            .block_records
                            .get(name)
                            .is_none()
                        {
                            state.error_message = Some(
                                crate::tf!("Block \"{}\" does not exist in drawing.", name)
                                    .into_owned(),
                            );
                            return Task::none();
                        }
                    }
                    WblockSourceMode::Objects => {
                        if state.selected_handles.is_empty() {
                            state.error_message = Some(
                                crate::t!("No objects selected. You must select objects to define a block.")
                                    .into_owned(),
                            );
                            return Task::none();
                        }
                    }
                    WblockSourceMode::EntireDrawing => {}
                }

                let path_str = state.file_path.trim().to_string();
                if path_str.is_empty() {
                    state.error_message =
                        Some(crate::t!("Please specify a file name and path.").into_owned());
                    return Task::none();
                }

                let mut path = std::path::PathBuf::from(path_str);
                if path.extension().is_none() {
                    path.set_extension("dwg");
                }

                let i = self.active_tab;
                if let Some(current_path) = self.tabs[i].current_path.as_ref() {
                    if current_path == &path {
                        state.error_message = Some(
                            crate::t!("Cannot write to the current drawing file.").into_owned(),
                        );
                        return Task::none();
                    }
                }

                let state = self.wblock.take().unwrap();
                self.active_modal = None;
                use crate::modules::insert::block_cmdline::WblockSource;
                use crate::ui::window::block_definition::BlockObjectMode;
                let mode = match state.object_mode {
                    WblockObjectMode::Retain => BlockObjectMode::Retain,
                    WblockObjectMode::Convert => BlockObjectMode::Convert,
                    WblockObjectMode::Delete => BlockObjectMode::Delete,
                };
                crate::modules::insert::block_cmdline::defaults().wblock_mode = mode;
                self.persist_settings_if_changed();
                let source = match state.source_mode {
                    WblockSourceMode::Block => {
                        WblockSource::Block(state.block_name.trim().to_string())
                    }
                    WblockSourceMode::EntireDrawing => WblockSource::Drawing,
                    WblockSourceMode::Objects => WblockSource::Objects {
                        handles: state.selected_handles.clone(),
                        base: state.parse_base_point(),
                        mode,
                    },
                };
                self.wblock_write(i, source, path, state.unit)
            }
            Message::ToleranceDialogField(field) => {
                if let Some(state) = self.geometric_tolerance.as_mut() {
                    state.apply_field(field);
                }
                Task::none()
            }
            Message::ToleranceDialogToggle(toggle) => {
                if let Some(state) = self.geometric_tolerance.as_mut() {
                    state.apply_toggle(toggle);
                }
                Task::none()
            }
            Message::ToleranceDialogApply => {
                self.apply_tolerance_dialog_edit();
                Task::none()
            }
            Message::ToleranceDialogOk => {
                let editing = self
                    .geometric_tolerance
                    .as_ref()
                    .and_then(|state| state.editing)
                    .is_some();
                if editing {
                    if self.apply_tolerance_dialog_edit() {
                        self.geometric_tolerance = None;
                        self.active_modal = None;
                        self.reset_modal_geometry();
                    }
                } else {
                    self.begin_tolerance_placement();
                    self.reset_modal_geometry();
                }
                Task::none()
            }
            Message::SetLinearFormat(code) => {
                self.units_popup_open = false;
                let i = self.active_tab;
                if self.tabs[i].scene.document.header.linear_unit_format == code {
                    return Task::none();
                }
                // Every displayed length is written through this, so the whole
                // drawing re-reads at once — no geometry moves, only the way it
                // is written down.
                self.push_undo_snapshot(i, "LUNITS");
                self.tabs[i].scene.document.header.linear_unit_format = code;
                self.tabs[i].dirty = true;
                self.tabs[i].scene.bump_geometry();
                self.refresh_properties();
                crate::entities::common::set_unit_context(
                    crate::entities::common::UnitContext::from_header(
                        &self.tabs[i].scene.document.header,
                    ),
                );
                let label = crate::modules::draw::units::linear_format_label(code);
                self.command_line
                    .push_output(crate::tf!("Length format is now {label}.").as_ref());
                Task::none()
            }
            Message::SetDrawingUnits(code) => {
                self.units_popup_open = false;
                let i = self.active_tab;
                let current = self.tabs[i].scene.document.header.insertion_units;
                if current == code {
                    return Task::none();
                }
                // Relabelling only: the geometry keeps every number it had, and
                // now says they count something else. Say so, because picking a
                // unit and seeing the drawing sit still otherwise reads as the
                // menu having done nothing. (#668)
                self.push_undo_snapshot(i, "UNITS");
                self.tabs[i].scene.document.header.insertion_units = code;
                self.tabs[i].scene.insertion_units_changed(current);
                self.tabs[i].dirty = true;
                let label = crate::modules::draw::units::label(code);
                self.command_line.push_output(
                    crate::tf!(
                        "Drawing unit is now {label}. Geometry unchanged — use DWGUNITS to convert it."
                    )
                    .as_ref(),
                );
                // Engineering / Architectural lengths read as feet and inches
                // whatever the unit, so a metric unit alone still shows inches
                // while drawing (#3683).
                let metric = matches!(code, 4..=7 | 12..=17);
                let lunits = self.tabs[i].scene.document.header.linear_unit_format;
                if metric && matches!(lunits, 3 | 4) {
                    self.command_line.push_output(
                        crate::t!(
                            "Lengths still show in feet and inches: set Length type to Decimal in UNITS."
                        )
                        .as_ref(),
                    );
                }
                Task::none()
            }
            Message::ToggleIsolatePopup => {
                self.isolate_popup_open ^= true;
                Task::none()
            }
            Message::CloseIsolatePopup => {
                self.isolate_popup_open = false;
                Task::none()
            }
            Message::ToggleSnap(t) => {
                self.snapper.toggle(t);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ToggleSnapPopup => {
                if self.active_modal == Some(super::super::super::ModalKind::DraftingSettings) {
                    if !self.drafting_settings_close_confirm && self.drafting_settings_dirty() {
                        self.drafting_settings_close_confirm = true;
                        return Task::none();
                    }
                    self.close_active_modal();
                    self.snap_popup_open = false;
                } else {
                    let st =
                        crate::ui::window::drafting_settings::DraftingSettingsState::from_app(self);
                    self.drafting_settings_saved = Some(st.clone());
                    self.drafting_settings_state = Some(st);
                    self.drafting_settings_close_confirm = false;
                    self.active_modal = Some(super::super::super::ModalKind::DraftingSettings);
                    self.snap_popup_open = true;
                }
                Task::none()
            }
            Message::CloseSnapPopup => {
                self.snap_popup_open = false;
                if self.active_modal == Some(super::super::super::ModalKind::DraftingSettings) {
                    self.close_active_modal();
                }
                Task::none()
            }
            Message::SnapSelectAll => {
                self.snapper.enable_all();
                Task::none()
            }
            Message::SnapClearAll => {
                self.snapper.disable_all();
                Task::none()
            }

            // ── Drafting Settings Dialog ──────────────────────────────────
            Message::OpenDraftingSettings(tab) => {
                let task = if self.focus_dialog(super::super::super::ModalKind::DraftingSettings) {
                    Task::none()
                } else {
                    self.update(Message::ToggleSnapPopup)
                };
                if let Some(state) = &mut self.drafting_settings_state {
                    state.active_tab = tab;
                }
                task
            }
            Message::DraftingSettingsTabChanged(tab) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.active_tab = tab;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleGrid => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.grid_on = !state.grid_on;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleSnap => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.snap_on = !state.snap_on;
                }
                Task::none()
            }
            Message::DraftingSettingsSnapXChanged(value) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.snap_x_input = value.clone();
                    if state.snap_equal {
                        state.snap_y_input = value;
                    }
                }
                Task::none()
            }
            Message::DraftingSettingsSnapYChanged(value) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.snap_y_input = value.clone();
                    if state.snap_equal {
                        state.snap_x_input = value;
                    }
                }
                Task::none()
            }
            Message::DraftingSettingsGridXChanged(value) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.grid_x_input = value;
                }
                Task::none()
            }
            Message::DraftingSettingsGridYChanged(value) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.grid_y_input = value;
                }
                Task::none()
            }
            Message::DraftingSettingsGridMajorChanged(value) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.grid_major_input = value;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleAdaptiveGrid => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.grid_adaptive = !state.grid_adaptive;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleBeyondLimits => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.grid_beyond_limits = !state.grid_beyond_limits;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleGridAxes => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.grid_axes = !state.grid_axes;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleEqualSnap => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.snap_equal = !state.snap_equal;
                    if state.snap_equal {
                        state.snap_y_input = state.snap_x_input.clone();
                    }
                }
                Task::none()
            }
            Message::DraftingSettingsToggleIsometric => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.isometric = !state.isometric;
                }
                Task::none()
            }
            Message::DraftingSettingsSetIsoPlane(plane) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.isometric = true;
                    state.iso_plane = plane;
                }
                Task::none()
            }
            Message::DraftingSettingsResetRotation => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.snap_angle_deg = 0.0;
                }
                Task::none()
            }
            Message::DraftingSettingsTogglePolar => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.polar_on = !state.polar_on;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleOrtho => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.ortho_on = !state.ortho_on;
                }
                Task::none()
            }
            Message::DraftingSettingsOsnapApertureChanged(value) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.osnap_aperture_input = value;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleOsnap => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.osnap_on = !state.osnap_on;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleOtrack => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.otrack_on = !state.otrack_on;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleSnapMode(snap_type) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    if !state.snap_modes.remove(&snap_type) {
                        state.snap_modes.insert(snap_type);
                    }
                }
                Task::none()
            }
            Message::DraftingSettingsToggleSnapMode3d(snap_type) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    if !state.snap3d_modes.remove(&snap_type) {
                        state.snap3d_modes.insert(snap_type);
                    }
                }
                Task::none()
            }
            Message::DraftingSettingsSnapSelectAll => {
                if let Some(state) = &mut self.drafting_settings_state {
                    for &(snap_type, _, _) in crate::snap::ALL_SNAP_MODES {
                        state.snap_modes.insert(snap_type);
                    }
                }
                Task::none()
            }
            Message::DraftingSettingsSnapClearAll => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.snap_modes.clear();
                }
                Task::none()
            }
            Message::DraftingSettingsToggle3dOsnap => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.osnap3d_on = !state.osnap3d_on;
                }
                Task::none()
            }
            Message::DraftingSettingsDynPrefs(prefs) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.dyn_prefs = prefs;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleDynInput => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.dyn_input_on = !state.dyn_input_on;
                }
                Task::none()
            }
            Message::DraftingSettingsToggleQuickProps => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.quick_props_on = !state.quick_props_on;
                }
                Task::none()
            }
            Message::DraftingSettingsQuickPropsFloat(float) => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.quick_props_float = float;
                }
                Task::none()
            }
            Message::QuickPropertiesGrab => {
                if !self.quick_properties_float {
                    // Lift the panel from the cursor where it was drawn.
                    self.quick_properties_float = true;
                    self.quick_properties_pos =
                        self.quick_properties_anchor + iced::Vector::new(12.0, 12.0);
                }
                self.dock_drag = Some(crate::ui::dock::DockDrag::QuickProperties);
                self.dock_drag_last = None;
                Task::none()
            }
            Message::DraftingSettingsToggleSelCycling => {
                if let Some(state) = &mut self.drafting_settings_state {
                    state.selection_cycling_on = !state.selection_cycling_on;
                }
                Task::none()
            }
            Message::DraftingSettingsApply => {
                if self.apply_drafting_settings() {
                    self.drafting_settings_saved = self.drafting_settings_state.clone();
                    self.persist_settings_if_changed();
                }
                Task::none()
            }
            Message::DraftingSettingsOk => {
                if self.apply_drafting_settings() {
                    self.drafting_settings_saved = self.drafting_settings_state.clone();
                    self.drafting_settings_close_confirm = false;
                    self.persist_settings_if_changed();
                    self.close_active_modal();
                }
                Task::none()
            }
            Message::DraftingSettingsClose => {
                if !self.drafting_settings_close_confirm && self.drafting_settings_dirty() {
                    self.drafting_settings_close_confirm = true;
                    return Task::none();
                }
                self.drafting_settings_close_confirm = false;
                self.close_active_modal();
                Task::none()
            }
            Message::DraftingSettingsCloseDiscard => {
                self.drafting_settings_close_confirm = false;
                self.close_active_modal();
                Task::none()
            }
            Message::DraftingSettingsCloseKeep => {
                self.drafting_settings_close_confirm = false;
                Task::none()
            }
            Message::AutoConstrainSelectRow(index) => {
                if index < self.auto_constrain_settings.priority.len() {
                    self.auto_constrain_selected_row = index;
                }
                Task::none()
            }
            Message::AutoConstrainToggleKind(kind) => {
                if let Some(index) = self
                    .auto_constrain_settings
                    .enabled
                    .iter()
                    .position(|candidate| *candidate == kind)
                {
                    self.auto_constrain_settings.enabled.remove(index);
                } else {
                    self.auto_constrain_settings.enabled.push(kind);
                }
                Task::none()
            }
            Message::AutoConstrainMoveUp => {
                let index = self.auto_constrain_selected_row;
                if index > 0 && index < self.auto_constrain_settings.priority.len() {
                    self.auto_constrain_settings.priority.swap(index, index - 1);
                    self.auto_constrain_selected_row -= 1;
                }
                Task::none()
            }
            Message::AutoConstrainMoveDown => {
                let index = self.auto_constrain_selected_row;
                if index + 1 < self.auto_constrain_settings.priority.len() {
                    self.auto_constrain_settings.priority.swap(index, index + 1);
                    self.auto_constrain_selected_row += 1;
                }
                Task::none()
            }
            Message::AutoConstrainSelectAll => {
                self.auto_constrain_settings.enabled =
                    super::super::super::settings::AutoConstraintKind::ALL.to_vec();
                Task::none()
            }
            Message::AutoConstrainClearAll => {
                self.auto_constrain_settings.enabled.clear();
                Task::none()
            }
            Message::AutoConstrainReset => {
                self.auto_constrain_settings = super::super::super::settings::AutoConstrainSettings::default();
                self.auto_constrain_selected_row = 0;
                self.auto_constrain_distance_input =
                    format!("{}", self.auto_constrain_settings.distance_tolerance);
                self.auto_constrain_angle_input =
                    format!("{}", self.auto_constrain_settings.angle_tolerance_deg);
                Task::none()
            }
            Message::AutoConstrainToggleTangentPoint => {
                self.auto_constrain_settings.tangent_must_share_point =
                    !self.auto_constrain_settings.tangent_must_share_point;
                Task::none()
            }
            Message::AutoConstrainTogglePerpendicularIntersection => {
                self.auto_constrain_settings.perpendicular_must_intersect =
                    !self.auto_constrain_settings.perpendicular_must_intersect;
                Task::none()
            }
            Message::AutoConstrainDistanceChanged(value) => {
                self.auto_constrain_distance_input = value;
                Task::none()
            }
            Message::AutoConstrainAngleChanged(value) => {
                self.auto_constrain_angle_input = value;
                Task::none()
            }
            action @ (Message::AutoConstrainApply | Message::AutoConstrainOk) => {
                let distance = self.auto_constrain_distance_input.trim().parse::<f64>();
                let angle = self.auto_constrain_angle_input.trim().parse::<f64>();
                match (distance, angle) {
                    (Ok(distance), Ok(angle))
                        if distance.is_finite()
                            && distance >= 0.0
                            && angle.is_finite()
                            && angle >= 0.0 =>
                    {
                        self.auto_constrain_settings.distance_tolerance = distance;
                        self.auto_constrain_settings.angle_tolerance_deg = angle;
                        self.auto_constrain_settings.sanitize();
                        self.auto_constrain_saved = Some(self.auto_constrain_settings.clone());
                        self.persist_settings_if_changed();
                        if matches!(action, Message::AutoConstrainOk) {
                            self.close_active_modal();
                        }
                    }
                    _ => self.command_line.push_error(
                        crate::t!("Distance and angle tolerances must be non-negative numbers.")
                            .as_ref(),
                    ),
                }
                Task::none()
            }
            Message::AutoConstrainCancel => {
                if let Some(saved) = self.auto_constrain_saved.take() {
                    self.auto_constrain_settings = saved;
                }
                self.close_active_modal();
                Task::none()
            }

            // ── Ribbon dropdowns ──────────────────────────────────────────
            Message::ToggleRibbonDropdown(id) => {
                let mut task = Task::none();
                // The minimize button's menu works without a drawing.
                if self.tabs[self.active_tab].is_start
                    && !crate::ui::ribbon::works_without_drawing(&id)
                {
                    self.ribbon.close_dropdown();
                } else {
                    if id == crate::modules::insert::insert_block::GALLERY_ID
                        && self.ribbon.open_dropdown.as_deref() != Some(id.as_str())
                    {
                        if self.block_palette_stale() {
                            self.refresh_block_palette();
                        }
                        // Its previews in view are worked out after this
                        // message, like the palette's.
                        task = self.block_palette.open_gallery();
                    }
                    // The per-viewport freeze column follows the active viewport.
                    if crate::ui::ribbon::qat_list_source(&id).unwrap_or(&id)
                        == crate::ui::ribbon::widgets::LAYER_COMBO_ID
                    {
                        self.refresh_layer_panel();
                        // Refreshing the table restores creation defaults;
                        // the combo must still show the selected object's layer.
                        self.sync_ribbon_from_selection();
                        if self.ribbon.open_dropdown.as_deref() != Some(id.as_str()) {
                            self.ribbon.layer_filter.clear();
                        }
                    }
                    self.ribbon.toggle_dropdown(&id);
                    // Typing goes straight to the layer list's search box.
                    if self.ribbon.open_dropdown.as_deref().is_some_and(|open| {
                        crate::ui::ribbon::qat_list_source(open).unwrap_or(open)
                            == crate::ui::ribbon::widgets::LAYER_COMBO_ID
                    }) {
                        task = task.chain(iced::widget::operation::focus(iced::widget::Id::new(
                            crate::ui::ribbon::widgets::LAYER_SEARCH_ID,
                        )));
                    }
                }
                task
            }
            Message::ToggleRibbonPanel(id) => {
                if self.tabs[self.active_tab].is_start {
                    self.ribbon.close_dropdown();
                } else {
                    self.ribbon.toggle_collapsed_panel(&id);
                }
                Task::none()
            }
            Message::CloseRibbonDropdown => {
                self.ribbon.close_dropdown();
                Task::none()
            }
            Message::XrefFadeSlide(amount) => {
                let sign = if self.ribbon.xref_fade < 0 { -1 } else { 1 };
                self.ribbon.xref_fade = sign * amount as i32;
                Task::none()
            }
            Message::XrefFadeCommit => {
                self.set_xref_fade(self.ribbon.xref_fade);
                Task::none()
            }
            Message::RibbonTextFind(value) => {
                self.ribbon.text_find = value;
                Task::none()
            }
            // The text goes to Find and Replace, which lists where it occurs.
            Message::RibbonTextFindSubmit => {
                let search = self.ribbon.text_find.trim().to_string();
                if search.is_empty() {
                    return Task::none();
                }
                self.find_text_from_ribbon(search)
            }
            Message::RibbonTextHeight(value) => {
                self.ribbon.text_height = value;
                self.ribbon.text_height_editing = true;
                Task::none()
            }
            Message::RibbonTextHeightSubmit => {
                self.ribbon.text_height_editing = false;
                let i = self.active_tab;
                let typed = self.ribbon.text_height.trim().replace(',', ".");
                match typed.parse::<f64>() {
                    Ok(height) if height.is_finite() && height > 0.0 => {
                        if (self.tabs[i].scene.document.header.text_height - height).abs() > 1e-12 {
                            self.tabs[i].scene.document.header.text_height = height;
                            self.tabs[i].dirty = true;
                        }
                    }
                    _ => self
                        .command_line
                        .push_rejection(crate::t!("Cannot set TEXTSIZE to that value.").as_ref()),
                }
                self.sync_frame_dropdown();
                Task::none()
            }
            Message::RibbonTextHeightFocus => iced::widget::operation::focus(
                iced::widget::Id::new(crate::ui::ribbon::widgets::TEXT_HEIGHT_ID),
            ),
            Message::RibbonDimLayer(layer) => {
                self.ribbon.close_dropdown();
                let value = if layer.is_empty() {
                    ".".to_string()
                } else {
                    layer
                };
                self.dispatch_command(&format!("DIMLAYER {value}"))
            }
            Message::RibbonMLeaderLayer(layer) => {
                self.ribbon.close_dropdown();
                let value = if layer.is_empty() {
                    ".".to_string()
                } else {
                    layer
                };
                self.dispatch_command(&format!("MLEADERLAYER {value}"))
            }
            Message::RibbonCenterLayer(layer) => {
                self.ribbon.close_dropdown();
                let value = if layer.is_empty() {
                    ".".to_string()
                } else {
                    layer
                };
                self.dispatch_command(&format!("CENTERLAYER {value}"))
            }
            Message::RibbonTextLayer(layer) => {
                self.ribbon.close_dropdown();
                let value = if layer.is_empty() {
                    ".".to_string()
                } else {
                    layer
                };
                self.dispatch_command(&format!("TEXTLAYER {value}"))
            }
            Message::XrefFadeToggle => {
                let fade = crate::scene::cache::block_cache::xref_fade_ctl();
                self.set_xref_fade(if fade == 0 { 50 } else { -fade });
                Task::none()
            }
            Message::DropdownSelectItem { dropdown_id, cmd } => {
                // A tool picked from a minimized ribbon's popup closes it.
                self.ribbon.close_popup();
                if self.tabs[self.active_tab].is_start {
                    self.ribbon.close_dropdown();
                    return Task::none();
                }
                // Save As is a secondary operation; the save face keeps BSAVE.
                let face = if dropdown_id == "BLOCK_SAVE" {
                    "BSAVE"
                } else {
                    cmd
                };
                self.ribbon.select_dropdown_item(dropdown_id, face);
                self.ribbon.activate_tool(cmd);
                self.dispatch_ribbon_command(cmd)
            }

            Message::DeleteSelected => {
                // In the MText preview, Delete removes text at the caret.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    self.mtext_delete();
                    return Task::none();
                }
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().menu.close();
                // A selected constraint-glyph pill takes Delete before entity
                // erase — the two selections are mutually exclusive (see
                // `Scene::selected_constraint`).
                if let Some(id) = self.tabs[i].scene.selected_constraint {
                    self.delete_parametric_constraint(id);
                    return Task::none();
                }
                if let (Some(handle), Some(session)) = (
                    self.tabs[i].selected_authoring,
                    self.tabs[i].active_block_edit_session(),
                ) {
                    let definition = session.br_handle;
                    let mut staged = self.tabs[i].scene.document.clone();
                    match crate::scene::block_authoring::edit::remove_parameter(
                        &mut staged,
                        definition,
                        handle,
                    ) {
                        Ok(()) => {
                            self.push_undo_snapshot(i, "Delete block parameter");
                            // Deleting a visibility parameter unhides objects.
                            self.tabs[i]
                                .scene
                                .document
                                .record_all_entities_for_transaction();
                            self.tabs[i].scene.document = staged;
                            self.tabs[i].selected_authoring = None;
                            self.tabs[i].scene.bump_geometry();
                            self.tabs[i].dirty = true;
                            self.finish_pending_history(i);
                            self.refresh_properties();
                        }
                        Err(message) => self.command_line.push_error(crate::t!(message).as_ref()),
                    }
                    return Task::none();
                }
                // Picked table cells: Delete empties them, not the table.
                if self.tabs[i].scene.table_cells.is_some() {
                    if let Some(task) = self.dispatch_table_cells("_TCELLCLEAR", i) {
                        return task;
                    }
                }
                let handles: Vec<_> = self.tabs[i].scene.selected.iter().cloned().collect();
                if !handles.is_empty() {
                    self.erase_selected_handles(i, &handles);
                }
                Task::none()
            }

            Message::SetModifiers { shift, ctrl, alt } => {
                self.alt_down = alt;
                let ctrl_changed = self.ctrl_down != ctrl;
                self.shift_down = shift;
                self.ctrl_down = ctrl;
                // Releasing Shift drops the hard axis lock immediately (#312)
                // — without this a lock could linger until the next move.
                if !shift {
                    self.axis_lock_dir = None;
                    self.release_held_override();
                }
                // A live command may key its preview off Ctrl (arc-direction
                // flip). Rebuild it at the current cursor so the flip shows
                // without waiting for the next mouse move.
                let i = self.active_tab;
                if ctrl_changed
                    && ctrl
                    && self.tabs[i].active_cmd.is_none()
                    && self.tabs[i].active_grip.is_some()
                {
                    return self.cycle_grip_ctrl_mode();
                }
                if ctrl_changed && self.tabs[i].active_cmd.is_some() {
                    if ctrl
                        && self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .is_some_and(|command| command.cycle_insertion_point())
                    {
                        if let Some(command) = self.tabs[i].active_cmd.as_ref() {
                            self.command_line.push_info(&command.prompt());
                        }
                        self.refresh_active_cmd_preview(i);
                    }
                    let p = self.tabs[i].last_cursor_screen;
                    return Task::done(Message::ViewportMove(p));
                }
                Task::none()
            }

            // ── In-place MText editor ───────────────────────────────────
            Message::TableEditorClick(point) => self.table_editor_click(point),
            Message::MTextEdit(action) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.content.perform(action);
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextFmt(kind) => {
                self.mtext_apply_fmt(kind);
                Task::none()
            }
            Message::MTextHeight(s) => {
                self.mtext_apply_span_number("height", s);
                Task::none()
            }
            Message::MTextColorChanged(color) => {
                self.mtext_apply_color(color);
                Task::none()
            }
            Message::MTextColorPickerToggle => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.menu = None;
                    ed.color_picker_open = !ed.color_picker_open;
                }
                Task::none()
            }
            Message::MTextStyle(s) => {
                self.mtext_pick_style(s);
                Task::none()
            }
            Message::MTextImport(path) => self.mtext_import(&path),
            Message::MTextImportRead(opened, read) => {
                self.finish_mtext_import(opened, read);
                Task::none()
            }
            Message::MTextFontPicker(event) => {
                let is_toggle = matches!(event, crate::ui::font_picker::FontPickerEvent::Toggle);
                let doc = &self.tabs[self.active_tab].scene.document;
                let picked = self
                    .mtext_editor
                    .as_mut()
                    .and_then(|ed| ed.font_picker.update(event, doc, true));
                match picked {
                    Some(FontChoice::StyleDefault) => self.mtext_apply_font("[Style default]"),
                    Some(FontChoice::Stroke(font) | FontChoice::TrueType(font)) => {
                        self.mtext_apply_font(&font)
                    }
                    None => {}
                }
                if is_toggle
                    && self
                        .mtext_editor
                        .as_ref()
                        .is_some_and(|ed| ed.font_picker.open)
                {
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        crate::ui::font_picker::FONT_PICKER_SEARCH_ID,
                    ));
                }
                Task::none()
            }
            Message::MTextResetToStyle(field) => {
                self.mtext_reset_to_style(field);
                Task::none()
            }
            Message::MTextNewStyleOpen => self.mtext_new_style_open(),
            Message::MTextNewStyleName(name) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.new_style_name = name;
                }
                Task::none()
            }
            Message::MTextNewStyleCreate => {
                let name = self
                    .mtext_editor
                    .as_ref()
                    .map(|ed| ed.new_style_name.clone());
                if let Some(name) = name {
                    self.mtext_style_from_selection(Some(name));
                }
                Task::none()
            }
            Message::MTextUpdateStyleFromSelection => {
                self.mtext_style_from_selection(None);
                Task::none()
            }
            Message::MTextStyleManager => {
                let style = self.mtext_editor.as_ref().map(|ed| ed.style.clone());
                let task = self.on_text_style_dialog_open();
                let i = self.active_tab;
                if let Some(style) =
                    style.filter(|s| self.tabs[i].scene.document.text_styles.get(s).is_some())
                {
                    self.textstyle_selected = style;
                    self.load_textstyle_draft(i);
                }
                task
            }
            Message::MTextOblique(s) => {
                self.mtext_apply_span_number("oblique", s);
                Task::none()
            }
            Message::MTextWidth(s) => {
                self.mtext_apply_span_number("width", s);
                Task::none()
            }
            Message::MTextCharSpace(s) => {
                self.mtext_apply_span_number("tracking", s);
                Task::none()
            }
            Message::MTextUndo => {
                self.mtext_undo();
                Task::none()
            }
            Message::MTextRedo => {
                self.mtext_redo();
                Task::none()
            }
            Message::MTextInsertField => {
                self.mtext_insert_field_open();
                Task::none()
            }
            Message::MTextStack => {
                self.mtext_stack_selection();
                Task::none()
            }
            Message::MTextScript(superscript) => {
                self.mtext_script_selection(superscript);
                Task::none()
            }
            Message::MTextClearFormatting => {
                self.mtext_clear_formatting();
                Task::none()
            }
            Message::MTextInsert(value) => {
                self.mtext_type(&value);
                Task::none()
            }
            Message::MTextMenu(menu) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.menu = if ed.menu == menu { None } else { menu };
                    ed.color_picker_open = false;
                }
                Task::none()
            }
            Message::MTextList(kind) => {
                self.mtext_list(kind);
                Task::none()
            }
            Message::MTextTab => {
                self.mtext_type("\t");
                Task::none()
            }
            Message::MTextLineSpacingInput(value) => {
                self.mtext_line_spacing_input(value);
                Task::none()
            }
            Message::MTextLineSpacingMode(exact) => {
                self.mtext_line_spacing_mode(exact);
                Task::none()
            }
            Message::MTextMaskToggle => {
                self.mtext_mask_toggle();
                Task::none()
            }
            Message::MTextMaskColor(color) => {
                self.mtext_mask_color(color);
                Task::none()
            }
            Message::MTextMaskScale(value) => {
                self.mtext_mask_scale(value);
                Task::none()
            }
            Message::MTextDirection(direction) => {
                self.mtext_direction(direction);
                Task::none()
            }
            Message::MTextFractionOpen => {
                self.mtext_fraction_open();
                Task::none()
            }
            Message::MTextFractionPart(part, value) => {
                self.mtext_fraction_part(part, value);
                Task::none()
            }
            Message::MTextFractionKind(kind) => {
                self.mtext_fraction_kind(kind);
                Task::none()
            }
            Message::MTextFractionInsert => {
                self.mtext_fraction_insert();
                Task::none()
            }
            Message::MTextRectWidthField(value) => {
                self.mtext_rect_width_field(value);
                Task::none()
            }
            Message::MTextHeightDrag(height) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    if let Some(height) = height.filter(|h| h.is_finite() && *h > 0.0) {
                        if !ed.height_dragging {
                            ed.record_undo();
                            ed.height_dragging = true;
                        }
                        ed.rect_height = height.to_string();
                    } else {
                        ed.height_dragging = false;
                    }
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextBoxDrag(width) => {
                self.mtext_box_drag(width);
                Task::none()
            }
            Message::MTextContextOpen(at, offset) => {
                self.mtext_context_open(at, offset);
                Task::none()
            }
            Message::MTextContextGroup(group) => {
                if let Some(menu) = self.mtext_editor.as_mut().and_then(|ed| ed.context_menu.as_mut()) {
                    menu.open = if menu.open == group { None } else { group };
                }
                Task::none()
            }
            Message::MTextContextClose => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.context_menu = None;
                    ed.smart_tag_open = false;
                }
                Task::none()
            }
            Message::MTextContextRun(command) => self.mtext_context_run(&command),
            Message::MTextSmartTag(open) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.smart_tag_open = open;
                    ed.context_menu = None;
                }
                Task::none()
            }
            Message::MTextPasteAs(mode, text) => {
                self.mtext_paste_as(&mode, text);
                Task::none()
            }
            Message::MTextMenuPick(message) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.menu = None;
                }
                self.update(*message)
            }
            Message::MTextRotation(value) => {
                self.mtext_rotation(value);
                Task::none()
            }
            Message::MTextAnnotative(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.record_undo();
                    ed.annotative = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnMode(mode) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    match mode.as_str() {
                        "Static" => {
                            ed.column_type = 1;
                            ed.column_auto_height = false;
                        }
                        "Dynamic auto" => {
                            ed.column_type = 2;
                            ed.column_auto_height = true;
                        }
                        "Dynamic manual" => {
                            ed.column_type = 2;
                            ed.column_auto_height = false;
                        }
                        _ => ed.column_type = 0,
                    }
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnCount(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.column_count = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnWidth(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.column_width = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnGutter(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.column_gutter = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnHeight(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.rect_height = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnFlowReversed(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.column_flow_reversed = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextParagraphNumber(field, value) => {
                self.mtext_apply_paragraph_number(field, value);
                Task::none()
            }
            Message::MTextFindText(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.find_text = value;
                }
                Task::none()
            }
            Message::MTextReplaceText(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.replace_text = value;
                }
                Task::none()
            }
            Message::MTextFindNext => {
                self.mtext_find_next();
                Task::none()
            }
            Message::MTextReplaceNext => {
                self.mtext_replace_next();
                Task::none()
            }
            Message::MTextReplaceAll => {
                self.mtext_replace_all();
                Task::none()
            }
            Message::MTextJustify(ap) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.record_undo();
                    ed.attachment = ap;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextAlign(a) => {
                self.mtext_apply_align(a);
                Task::none()
            }
            Message::MTextLineSpacing(f) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.record_undo();
                    ed.line_spacing = f;
                    ed.line_spacing_draft = None;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextShowPreview(on) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.show_preview = on;
                }
                self.rebuild_mtext_preview();
                // Focus the text area when switching to Edit so the caret
                // shows and typing/clicking edits immediately.
                if on {
                    Task::none()
                } else {
                    iced::widget::operation::focus(iced::widget::Id::new(
                        super::super::super::view::MTEXT_TEXT_ID,
                    ))
                }
            }
            Message::MTextSelStart(off) => {
                // A hit past the last glyph (an empty text's phantom line)
                // is the end of the text.
                let off = off.min(self.mtext_vis_count());
                // Count quick same-spot clicks: 1 = place caret, 2 = select the
                // word, 3 = select all.
                let now = Instant::now();
                let count = match self.mtext_click_time {
                    Some(t)
                        if now.duration_since(t).as_millis() < 400
                            && off.abs_diff(self.mtext_click_off) <= 1 =>
                    {
                        (self.mtext_click_count + 1).min(3)
                    }
                    _ => 1,
                };
                self.mtext_click_time = Some(now);
                self.mtext_click_off = off;
                self.mtext_click_count = count;
                match count {
                    2 => self.mtext_select_word(off),
                    3 => self.mtext_select_all(),
                    _ => {
                        if let Some(ed) = self.mtext_editor.as_mut() {
                            ed.typing_format = Default::default();
                            ed.sel_anchor = off;
                            ed.sel = Some((off, off));
                            ed.caret = off;
                            ed.caret_blink_on = true;
                        }
                    }
                }
                self.unfocus_widgets()
            }
            _ => Task::none(),
        }
    }
}
