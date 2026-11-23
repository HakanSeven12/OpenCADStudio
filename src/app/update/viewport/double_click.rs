use super::*;
use super::super::util::*;

impl OpenCADStudio {
    /// What a double-click on an object opens: a table cell, a constraint
    /// value, a text editor, HATCHEDIT, the attribute editor, or BEDIT /
    /// REFEDIT for a block. `None` when nothing under the cursor has an
    /// editor. Shared by model space, paper space and a viewport's model
    /// space, which picks through the viewport's own view.
    pub(in crate::app) fn double_click_edit(
        &mut self,
        i: usize,
        p: Point,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        bounds: iced::Rectangle,
    ) -> Option<Task<Message>> {
        let (view_rot, eye, all_wires) = self.pick_view(i, edit_cam, bounds);
        let click_world = self.cursor_model_point(i, edit_cam, p, bounds);
        let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
            all_wires,
            click_world,
            view_rot,
            eye,
            bounds,
            crate::ui::overlay::pick_box_aperture_px(self.pick_box) * 2.0,
        );
        let candidate_handles = self.tabs[i]
            .scene
            .interaction_candidate_handles(&click_candidates);
        // Resolve the double-clicked object the way a single click
        // picks it — its wire, then a hatch by its fill (a hatch has
        // no boundary wire), then a block's hatch (its INSERT), or
        // for a block/solid with no wire under the cursor its shaded
        // body, which maps to the parent INSERT.
        let hit = scene::pick::hit_test::click_hit(
            p,
            &click_candidates,
            view_rot,
            eye,
            bounds,
            self.tabs[i].scene.document.header.lineweight_display,
            crate::ui::overlay::pick_box_aperture_px(self.pick_box),
            &self.tabs[i].scene.draw_depth_map(),
        )
        .and_then(Scene::handle_from_wire_name)
        .or_else(|| {
            let hatches = self.tabs[i]
                .scene
                .visible_hatches_for_click(candidate_handles.as_ref());
            scene::pick::hit_test::click_hit_hatch(
                p,
                &hatches,
                view_rot,
                eye,
                bounds,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
                candidate_handles.as_ref(),
            )
        })
        .or_else(|| {
            scene::pick::hit_test::click_hit_insert_hatch(
                p,
                self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                view_rot,
                eye,
                bounds,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
                candidate_handles.as_ref(),
            )
        })
        .or_else(|| {
            self.solid_select_hit(
                i,
                p,
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
            )
        })
        // Inside an empty table cell nothing is drawn to hit: the
        // cell under the cursor takes the double-click.
        .or_else(|| {
            self.tabs[i]
                .scene
                .table_cell_pick(click_world, 0.0)
                .map(|(table, _)| table)
        });
        let handle = hit?;
        // Locked layer: double-click must not open any editor
        // (text / attribute / in-place block edit).
        if let Some(layer) = self.tabs[i].scene.locked_layer_name(handle) {
            self.command_line.push_info(
                crate::tf!("Object is on locked layer \"{layer}\" — unlock the layer to edit it.")
                    .as_ref(),
            );
            return Some(Task::none());
        }
        // An embedded document opens in its own program.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(task) = self.edit_ole_package(i, handle) {
            return Some(task);
        }
        // Table double-click: shared TABLEDIT editor setup. A
        // grid miss returns NoCell and falls through to the
        // other text/entity editors below; an editable cell or
        // a locked cell (indicator armed, no editor) ends the
        // double-click handling here.
        if self.begin_table_cell_edit(i, handle, click_world)
            != crate::modules::annotate::table_cmd::TableCellEditStart::NoCell
        {
            return Some(self.focus_cmd_input());
        }
        // A constraint dimension edits its parameter from the
        // command line, standing in for the reference's in-place
        // value editor.
        if let Some(prompt) = self.dynamic_dimension_value_command(i, handle) {
            use crate::command::CadCommand;
            self.command_line.push_info(&prompt.prompt());
            self.tabs[i].active_cmd = Some(Box::new(prompt));
            return Some(Task::none());
        }
        // Any text-bearing entity opens its in-place editor
        // (plain box or rich MText editor, per type). A
        // Leader resolves to the entity it annotates.
        let is_editable_text = self.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .is_some_and(|e| {
                crate::app::text_inline::read_text_field(e).is_some()
                    || matches!(e, AcadEntityType::Leader(_))
            });
        if is_editable_text {
            return Some(self.begin_text_edit(handle));
        }
        // A hatch starts HATCHEDIT on itself; selected, the
        // command skips its pick. (The reference program's
        // default double-click action for a hatch is Quick
        // Properties, but Properties already follows a single
        // click here, so the double-click edits.)
        if matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(AcadEntityType::Hatch(_))
        ) {
            self.tabs[i].scene.deselect_all();
            self.tabs[i].scene.select_entity(handle, true);
            self.tabs[i].selected_handle = Some(handle);
            self.refresh_properties();
            return Some(self.dispatch_command("HATCHEDIT"));
        }
        // Double-clicking a block with attributes opens the
        // attribute editor (edit its values). For blocks without
        // attributes, Options > Drawing chooses BEDIT (default)
        // or in-place REFEDIT. (#136, #192, #261)
        let insert_has_attrs = matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(AcadEntityType::Insert(ins)) if !ins.attributes.is_empty()
        );
        if insert_has_attrs
            && self.double_click_block_attedit
            && self.tabs[i].active_block_edit.is_none()
        {
            return Some(Task::done(Message::AttrEditorOpen(handle)));
        }
        let is_insert = matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(AcadEntityType::Insert(_))
        );
        if is_insert && self.tabs[i].refedit_session.is_none() {
            if self.block_editor_preferences.locked && !self.double_click_block_refedit {
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entity(handle, true);
                self.tabs[i].selected_handle = Some(handle);
                self.refresh_properties();
                return Some(self.dock_set_open(crate::ui::dock::PanelId::Properties, true));
            }
            let command = if self.double_click_block_refedit {
                "REFEDIT_BEGIN"
            } else {
                "BEDIT_BEGIN"
            };
            return Some(Task::done(Message::Command(format!(
                "{}:{}",
                command,
                handle.value()
            ))));
        }
        None
    }

    /// Remove cursor-derived highlights and cancel a queued rollover pick before
    /// moving the camera. A later idle cursor move re-arms hover normally.
    pub(in crate::app) fn clear_navigation_hover(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs[i].scene.set_hover_highlight(None);
        self.hover_dwell = None;
        self.snap_dwell = None;
        self.constraint_glyph_tooltip = None;
        self.grip_hover = None;
        self.ucs_icon_hover = false;
    }

    /// Queue a fresh rollover pick at the stationary cursor after camera motion
    /// ends. Repeated wheel events replace this timestamp, so the pick runs only
    /// after the final zoom step settles.
    /// Keep the command-move snap throttle armed after a point click has
    /// already resolved the exact snap at that button position.
    pub(in crate::app) fn arm_resolved_command_snap_dwell(&mut self, i: usize, point: iced::Point) {
        let point_pick_pending = self.tabs[i].active_cmd.as_ref().is_some_and(|command| {
            !command.needs_entity_pick()
                && !command.needs_structure_point_pick()
                && !command.needs_tangent_pick()
                && !command.is_selection_gathering()
        });
        if point_pick_pending {
            self.snap_dwell = Some(crate::app::SnapDwell {
                last_move_at: Instant::now(),
                last_preview_snap_at: Instant::now(),
                point,
                tab: i,
                preview_dirty: false,
                snap_resolved: true,
                full_snap_due: Instant::now()
                    + std::time::Duration::from_millis(crate::app::SNAP_MOVING_MS),
            });
        }
    }

}
