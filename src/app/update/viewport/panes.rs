use super::*;
use super::super::util::*;

impl OpenCADStudio {
    /// Convert a pane-local cursor point (from the pane's `mouse_area`) to the
    /// canvas-relative point the viewport handlers expect.
    pub(in crate::app) fn pane_canvas_point(&self, idx: usize, local: Point) -> Point {
        let i = self.active_tab;
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let o = self.tabs[i].scene.pane_origin_px(idx, vw, vh);
        Point::new(o.x + local.x, o.y + local.y)
    }

    pub(in crate::app) fn on_pane_resized(
        &mut self,
        ev: iced::widget::pane_grid::ResizeEvent,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.model_panes.resize(ev.split, ev.ratio);
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        self.tabs[i].scene.sync_tiles_from_panes(vw, vh);
        self.tabs[i].scene.camera_generation += 1;
        Task::none()
    }

    pub(in crate::app) fn on_pane_clicked(&mut self, pane: iced::widget::pane_grid::Pane) -> Task<Message> {
        let i = self.active_tab;
        if let Some(&idx) = self.tabs[i].scene.model_panes.get(pane) {
            self.focus_model_pane(idx);
        }
        Task::none()
    }

    pub(in crate::app) fn on_pane_dragged(
        &mut self,
        ev: iced::widget::pane_grid::DragEvent,
    ) -> Task<Message> {
        if let iced::widget::pane_grid::DragEvent::Dropped { pane, target } = ev {
            let i = self.active_tab;
            self.tabs[i].scene.model_panes.drop(pane, target);
            let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
            self.tabs[i].scene.sync_tiles_from_panes(vw, vh);
            self.tabs[i].scene.camera_generation += 1;
        }
        Task::none()
    }

    pub(in crate::app) fn constraint_glyph_under(
        &self,
        i: usize,
        cursor: iced::Point,
    ) -> Option<(
        crate::scene::parametric_constraints::ConstraintKind,
        Vec<crate::scene::parametric_constraints::ParametricRef>,
    )> {
        if self.tabs[i].scene.current_layout != "Model" {
            return None;
        }
        let vp_size = self.tabs[i].scene.selection.borrow().view.vp_size;
        let scope = self.tabs[i].current_parametric_scope();
        let id = self.tabs[i].scene.constraint_glyph_hit(
            scope,
            vp_size,
            self.show_constraint_values,
            self.constraint_bar_display,
            self.constraint_bar_mode,
            cursor,
        )?;
        let set = self.tabs[i].scene.parametric_constraint_set(scope)?;
        let constraint = set.get(id)?;
        Some((constraint.kind, constraint.refs.clone()))
    }

    /// Whether a selected object lies under `p` (tile-local).
    pub(in crate::app) fn selected_object_under(
        &self,
        i: usize,
        p: Point,
        bounds: iced::Rectangle,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
    ) -> bool {
        self.object_under(i, p, bounds, edit_cam)
            .is_some_and(|handle| self.tabs[i].scene.selected.contains(&handle))
    }

    /// The table cell under `p` (clear of the grid lines by `margin`), unless
    /// another object drawn over the table is under it.
    pub(in crate::app) fn table_cell_under(
        &self,
        i: usize,
        p: Point,
        bounds: iced::Rectangle,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        margin: f64,
    ) -> Option<(Handle, (usize, usize))> {
        let world = self.cursor_model_point(i, edit_cam, p, bounds);
        let (table, cell) = self.tabs[i].scene.table_cell_pick(world, margin)?;
        self.object_under(i, p, bounds, edit_cam)
            .is_none_or(|handle| handle == table)
            .then_some((table, cell))
    }

    pub(in crate::app) fn selection_click_hit(
        &self,
        i: usize,
        p: Point,
        context: crate::command::PointPickContext,
        click_candidates: &crate::scene::pick::interaction_index::InteractionCandidates,
        candidate_handles: Option<&rustc_hash::FxHashSet<Handle>>,
        t_arm: Option<Instant>,
        candidate_ms: f64,
        handles_ms: f64,
    ) -> Option<Handle> {
        let crate::command::PointPickContext {
            view: view_rot,
            eye,
            bounds,
            aperture_px: pick_aperture,
        } = context;
        let wire_started = crate::perf::enabled().then(Instant::now);
        let mut hit = scene::pick::hit_test::click_hit(
            p,
            click_candidates,
            view_rot,
            eye,
            bounds,
            self.tabs[i].scene.document.header.lineweight_display,
            pick_aperture,
            &self.tabs[i].scene.draw_depth_map(),
        )
        .and_then(Scene::handle_from_wire_name);
        let wire_ms = crate::perf::elapsed_ms(wire_started);
        let mut hatch_models_ms = 0.0;
        let mut hatch_test_ms = 0.0;
        let mut hatch_count = 0usize;
        if hit.is_none() {
            let models_started = crate::perf::enabled().then(Instant::now);
            let hatches = self.tabs[i]
                .scene
                .visible_hatches_for_click(candidate_handles);
            hatch_count = hatches.len();
            hatch_models_ms = crate::perf::elapsed_ms(models_started);
            let hatch_started = crate::perf::enabled().then(Instant::now);
            hit = scene::pick::hit_test::click_hit_hatch(
                p,
                &hatches,
                view_rot,
                eye,
                bounds,
                pick_aperture,
                &self.tabs[i].scene.draw_depth_map(),
                candidate_handles,
            );
            hatch_test_ms = crate::perf::elapsed_ms(hatch_started);
        }
        let mut insert_ms = 0.0;
        if hit.is_none() {
            // Block-internal hatch: resolve to the parent Insert.
            let insert_started = crate::perf::enabled().then(Instant::now);
            hit = scene::pick::hit_test::click_hit_insert_hatch(
                p,
                self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                view_rot,
                eye,
                bounds,
                pick_aperture,
                &self.tabs[i].scene.draw_depth_map(),
                candidate_handles,
            );
            insert_ms = crate::perf::elapsed_ms(insert_started);
        }
        let mut solid_ms = 0.0;
        if hit.is_none() {
            // Normal 3D selection follows the displayed B-rep
            // edges. Face-interior picking remains reserved for
            // modelling commands that explicitly request it.
            let solid_started = crate::perf::enabled().then(Instant::now);
            hit = self.solid_select_hit(
                i,
                p,
                view_rot,
                eye,
                bounds,
                candidate_handles,
                pick_aperture,
            );
            solid_ms = crate::perf::elapsed_ms(solid_started);
        }
        let hit_hatch = hit.is_some_and(|handle| {
            matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(document::EntityType::Hatch(_))
            )
        });
        let pick_ms = crate::perf::elapsed_ms(t_arm);
        if crate::perf::enabled() && pick_ms >= 1.0 {
            crate::perf_record!(
                "[perf] select-click total={pick_ms:.1}ms candidates={candidate_ms:.2}ms handles={handles_ms:.2}ms wire={wire_ms:.2}ms hatch-models={hatch_models_ms:.2}ms hatch-test={hatch_test_ms:.2}ms insert={insert_ms:.2}ms solid={solid_ms:.2}ms candidate_wires={} hatch_candidates={hatch_count} hit_hatch={hit_hatch}",
                click_candidates.len(),
            );
        }
        hit
    }

    pub(in crate::app) fn selection_object_under(
        &self,
        i: usize,
        p: Point,
        bounds: iced::Rectangle,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
    ) -> Option<Handle> {
        let (view, eye, wires) = self.pick_view(i, edit_cam, bounds);
        let aperture_px = crate::ui::overlay::pick_box_aperture_px(self.pick_box);
        let world = self.cursor_model_point(i, edit_cam, p, bounds);
        let scene = &self.tabs[i].scene;
        let candidates = scene.interaction_pick_candidates_near(
            wires,
            world,
            view,
            eye,
            bounds,
            aperture_px * 2.0,
        );
        let handles = scene.interaction_candidate_handles(&candidates);
        let context = crate::command::PointPickContext {
            view,
            eye,
            bounds,
            aperture_px,
        };
        self.selection_click_hit(i, p, context, &candidates, handles.as_ref(), None, 0.0, 0.0)
    }

    /// The topmost object drawn under `p`.
    pub(in crate::app) fn object_under(
        &self,
        i: usize,
        p: Point,
        bounds: iced::Rectangle,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
    ) -> Option<Handle> {
        let started = crate::perf::enabled().then(Instant::now);
        let (view_rot, eye, wires) = self.pick_view(i, edit_cam, bounds);
        let aperture = crate::ui::overlay::pick_box_aperture_px(self.pick_box);
        let world = self.cursor_model_point(i, edit_cam, p, bounds);
        let scene = &self.tabs[i].scene;
        let wire_count = wires.len();
        let candidate_started = crate::perf::enabled().then(Instant::now);
        let candidates = scene.interaction_hover_candidates_near(
            wires,
            world,
            view_rot,
            eye,
            bounds,
            aperture * 2.0,
        );
        let candidate_ms = crate::perf::elapsed_ms(candidate_started);
        let hit_started = crate::perf::enabled().then(Instant::now);
        let hit = scene::pick::hit_test::click_hit(
            p,
            &candidates,
            view_rot,
            eye,
            bounds,
            scene.document.header.lineweight_display,
            aperture,
            &scene.draw_depth_map(),
        )
        .and_then(Scene::handle_from_wire_name);
        let hit_ms = crate::perf::elapsed_ms(hit_started);
        let total_ms = crate::perf::elapsed_ms(started);
        if total_ms >= 1.0 {
            crate::perf_record!(
                "[perf] object-under-probe total={total_ms:.1}ms candidates={candidate_ms:.1}ms hit={hit_ms:.1}ms wires={} candidates_count={} hit={hit:?}",
                wire_count,
                candidates.len(),
            );
        }
        hit
    }

    /// A press-drag that starts inside a table cell picks the cells from
    /// there to the one under the cursor as it moves — the picked range is
    /// the only feedback (no selection window or lasso is drawn).
    pub(in crate::app) fn drag_table_cells(
        &mut self,
        i: usize,
        p: Point,
        tile: iced::Rectangle,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
    ) {
        if self.tabs[i].active_cmd.is_some() {
            return;
        }
        let press = {
            let sel = self.tabs[i].scene.selection.borrow();
            (sel.input.left_down && sel.input.left_dragging)
                .then_some(sel.input.left_press_pos)
                .flatten()
        };
        let Some(press) = press else {
            return;
        };
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: tile.width,
            height: tile.height,
        };
        let press = Point::new(press.x - tile.x, press.y - tile.y);
        let Some((table, anchor)) = self.table_cell_under(i, press, bounds, edit_cam, 0.0) else {
            return;
        };
        let here = self.cursor_model_point(i, edit_cam, p, bounds);
        let end = self.tabs[i]
            .scene
            .table_cell_pick(here, 0.0)
            .filter(|(hit, _)| *hit == table)
            .map(|(_, end)| end)
            .or_else(|| {
                self.tabs[i]
                    .scene
                    .table_cells
                    .filter(|cells| cells.table == table)
                    .map(|cells| cells.end)
            })
            .unwrap_or(anchor);
        let cells = crate::scene::TableCellSelection { table, anchor, end };
        {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.gesture.poly_active = false;
            sel.gesture.poly_points.clear();
            sel.gesture.box_anchor = None;
            sel.gesture.box_current = None;
        }
        if self.tabs[i].scene.table_cells != Some(cells) {
            self.tabs[i].scene.select_table_cells(cells);
            self.refresh_properties();
        }
    }

    /// Once a press on a selected object has moved far enough, MOVE (COPY
    /// with Ctrl) runs on the selection from the grabbed point, so the drop
    /// gets the command's preview, snaps and ortho.
    pub(in crate::app) fn start_drag_move(&mut self, i: usize, p: Point) -> Option<Task<Message>> {
        let (base, false) = self.drag_move? else {
            return None;
        };
        {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            let press = sel.input.left_press_pos?;
            let held_ms = sel
                .input
                .left_press_time
                .map_or(u128::MAX, |time| time.elapsed().as_millis());
            let (dx, dy) = (p.x - press.x, p.y - press.y);
            if !sel.input.left_down || held_ms < POLY_START_DELAY_MS || dx * dx + dy * dy <= 9.0 {
                return None;
            }
            sel.input.left_dragging = true;
            sel.gesture.clear_left();
        }
        self.drag_move = Some((base, true));
        let command = if self.ctrl_down { "COPY" } else { "MOVE" };
        let started = self.update(Message::Command(command.to_string()));
        if self.tabs[i].active_cmd.is_none() {
            self.drag_move = None;
            return Some(started);
        }
        Some(Task::batch([
            started,
            self.feed_command(crate::command::StepInput::Point(base)),
        ]))
    }

    pub(in crate::app) fn command_picks_on_press(&self) -> bool {
        let tab = &self.tabs[self.active_tab];
        tab.active_cmd.as_ref().is_some_and(|command| {
            !command.is_selection_gathering() && !command.accepts_drag_selection()
        }) && tab.scene.selection.borrow().gesture.box_anchor.is_none()
    }

    /// Capture the button event without doing the expensive hover/snap pass.
    /// Point commands immediately invoke the release path, which resolves
    /// snapping once using this exact screen position.
    /// The drawing point under the screen `position` in tab `i`.
    pub(in crate::app) fn cursor_world_at(&mut self, i: usize, position: Point) -> glam::DVec3 {
        let canvas_size = self.tabs[i].scene.selection.borrow().view.vp_size;
        let frame = self.tabs[i].scene.viewport_edit_frame(canvas_size);
        let tile = frame.as_ref().map(|(_, rect)| *rect).unwrap_or_else(|| {
            self.tabs[i]
                .scene
                .active_model_tile_bounds(canvas_size.0, canvas_size.1)
        });
        let camera = frame.map(|(camera, _)| camera);
        let local = Point::new(position.x - tile.x, position.y - tile.y);
        let bounds = iced::Rectangle::with_size(iced::Size::new(tile.width, tile.height));
        self.cursor_model_point(i, &camera, local, bounds)
    }

    pub(in crate::app) fn set_cursor_for_press(&mut self, position: Point) {
        let i = self.active_tab;
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
        }
        // This position supersedes anything the gate withheld; answering the
        // older one after the press would send the cursor backwards, the
        // hazard `on_snap_dwell_tick` documents for its own replay.
        self.move_owed = None;
        self.tabs[i].last_cursor_world = self.cursor_world_at(i, position);
        self.tabs[i].last_cursor_screen = position;
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
            .input
            .last_move_pos = Some(position);
    }

}
