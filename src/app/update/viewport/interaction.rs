use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn on_tick(&mut self, t: Instant) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.update(t - self.start);
        // Dynamic dimensions keep their screen size across zoom bands. The
        // rewrite edits entities (a full geometry bump), so it waits until
        // the view settles; `is_settling` keeps ticks coming until then.
        if !self.tabs[i].scene.navigating_lod() {
            self.tabs[i].scene.refresh_dynamic_dimension_scales(false);
        }
        // Underlays are rasterised again for the zoom once it settles, on a
        // worker; the old rasters stay on screen until it ends.
        self.tabs[i].scene.refresh_underlay_resolution(t);
        let reseed = self.start_underlay_reseed(i);

        // If the camera moved since we last synced, write it back to
        // the document and mark the file dirty.
        let gen = self.tabs[i].scene.camera_generation;
        if gen != self.tabs[i].last_synced_camera_gen {
            self.tabs[i].last_synced_camera_gen = gen;
            if self.tabs[i].active_block_edit.is_some() {
                let camera = self.tabs[i].scene.camera.borrow().clone();
                if let Some(session) = self.tabs[i].active_block_edit_session_mut() {
                    session.editor_camera = camera;
                }
            } else if self.tabs[i].scene.active_viewport.is_some() {
                // Floating-viewport navigation writes its camera straight to
                // the viewport entity. Syncing the separate main camera here
                // can overwrite the saved Model/Paper view with a camera that
                // does not own this change.
                self.tabs[i].dirty = true;
            } else if self.tabs[i].scene.sync_camera_to_document() {
                self.tabs[i].dirty = true;
            }
        }

        // Surface any plugin-guard panic messages that piled up since
        // the last tick (the host singleton isn't reachable from inside
        // the plugin hooks, so they queue and we flush here). (#145)
        #[cfg(not(target_arch = "wasm32"))]
        for msg in crate::plugin::drain_errors() {
            self.command_line.push_error(&msg);
        }

        reseed
    }

    /// Starts the worker for the rasters a zoom step asked for in drawing
    /// `i` ([`crate::scene::Scene::take_reseed_job`]).
    pub(in crate::app) fn start_underlay_reseed(&mut self, i: usize) -> Task<Message> {
        let Some(job) = self.tabs[i].scene.take_reseed_job() else {
            return Task::none();
        };
        let (tab_id, generation) = (self.tabs[i].id, self.tabs[i].document_generation);
        crate::app::update::background_task(
            "Underlay raster",
            move || job.render(),
            move |done| Message::UnderlayRastersDrawn(tab_id, generation, Box::new(done)),
        )
    }

    /// Takes the rasters a worker drew for a zoom step, into the drawing it
    /// was started for while that drawing is still the same.
    pub(in crate::app) fn finish_underlay_reseed(
        &mut self,
        tab_id: u64,
        generation: u64,
        done: crate::scene::ReseedDone,
    ) {
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id && tab.document_generation == generation)
        {
            tab.scene.apply_reseed(done);
        }
    }

    pub(in crate::app) fn on_set_render_mode(
        &mut self,
        mode: document::entities::ViewportRenderMode,
    ) -> Task<Message> {
        use document::entities::ViewportRenderMode as M;
        let i = self.active_tab;
        let label = match mode {
            M::Wireframe2D => "Wireframe 2D",
            M::Wireframe3D => "Wireframe 3D",
            M::HiddenLine => "Hidden Line",
            M::FlatShaded => "Flat Shaded",
            M::GouraudShaded => "Gouraud Shaded",
            M::FlatShadedWithEdges => "Flat Shaded + Edges",
            M::GouraudShadedWithEdges => "Gouraud Shaded + Edges",
        };
        // In a paper layout with an active (double-clicked)
        // viewport, the picker drives that viewport entity's own
        // render mode; the model-layout tab style is untouched.
        if self.tabs[i].scene.set_active_viewport_render_mode(mode) {
            self.tabs[i].scene.set_active_visual_style_named(
                crate::modules::view::visual_style::style_name_for(mode),
            );
            self.tabs[i].dirty = true;
            self.command_line
                .push_output(crate::tf!("Viewport visual style: {label}").as_ref());
            return Task::none();
        }
        self.apply_render_mode(i, mode, label);
        // The viewport record carries the style the mode is saved as.
        self.tabs[i].scene.set_active_visual_style_named(
            crate::modules::view::visual_style::style_name_for(mode),
        );
        self.command_line
            .push_output(crate::tf!("Visual style: {label}").as_ref());
        // The point cloud move gizmo follows the visual style.
        if i == self.active_tab {
            self.refresh_selected_grips();
        }
        Task::none()
    }

    /// The model tab's visual style, without a message.
    pub(crate) fn apply_render_mode(
        &mut self,
        i: usize,
        mode: document::entities::ViewportRenderMode,
        label: &str,
    ) {
        use document::entities::ViewportRenderMode as M;
        self.tabs[i].render_mode = mode;
        // Write the style onto the active Model tile alone so it
        // sticks when that tile loses focus and the other tiles keep
        // their own styles.
        self.tabs[i].scene.set_active_model_tile_render_mode(mode);
        // Keep the legacy `wireframe` bool synced — both wireframe
        // modes set it, everything else clears it.
        let wf = matches!(mode, M::Wireframe2D | M::Wireframe3D);
        self.tabs[i].wireframe = wf;
        self.ribbon.set_wireframe(wf);
        self.tabs[i].visual_style = label.into();
        // No geometry bump: every render pass keys on the viewport's own
        // mode, and a bump re-tessellated and re-uploaded the whole drawing.
        self.tabs[i].dirty = true;
    }

    pub(in crate::app) fn on_cursor_moved(
        &mut self,
        p: Point,
        expected_viewport: Option<document::Handle>,
    ) -> Task<Message> {
        if self.color_picker.is_some() {
            return Task::none();
        }

        // `p` is relative to the ViewCube hit area's top-left. Map
        // it back to full-canvas coordinates so ViewportClick's
        // hit-test lines up. The hit area sits in the top-right of
        // the full canvas in model space, or of the active
        // viewport's screen rectangle in a paper layout.
        let i = self.active_tab;
        if self.tabs[i].scene.active_viewport != expected_viewport
            || (expected_viewport.is_none() && self.tabs[i].scene.current_layout != "Model")
        {
            return Task::none();
        }
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let (ox, oy) = match self.tabs[i]
            .scene
            .active_viewport
            .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (vw, vh)))
        {
            Some(rect) => (
                rect.x + rect.width - VIEWCUBE_PAD - VIEWCUBE_HIT_SIZE,
                rect.y + VIEWCUBE_PAD,
            ),
            None => {
                // Model layout: the cube sits in the active tile's
                // top-right corner.
                let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                (
                    tb.x + tb.width - VIEWCUBE_PAD - VIEWCUBE_HIT_SIZE,
                    tb.y + VIEWCUBE_PAD,
                )
            }
        };
        self.cursor_pos = iced::Point::new(ox + p.x, oy + p.y);

        // Drive the ViewCube hover highlight directly from this
        // message — it fires whenever the cube's hit-area overlay
        // sees motion, so we don't depend on the shader widget's
        // `Program::update` receiving the same event (overlays sit
        // above the shader and can mask it). Map the cursor into
        // the active viewport's local box and use that box's size,
        // since that's where the cube is actually drawn.
        let tile = match self.tabs[i]
            .scene
            .active_viewport
            .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (vw, vh)))
        {
            Some(rect) => rect,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let cam_rot = self.tabs[i].scene.active_view_rotation_mat();
        let hover = hover_id(
            self.cursor_pos.x - tile.x,
            self.cursor_pos.y - tile.y,
            tile.width,
            tile.height,
            cam_rot,
            VIEWCUBE_PX,
        );
        self.tabs[i].scene.viewcube_hover.set(hover);
        self.tabs[i].scene.viewcube_opacity.set(
            crate::scene::pipeline::viewcube::viewcube_opacity(
                self.cursor_pos.x - tile.x,
                self.cursor_pos.y - tile.y,
                tile.width,
                self.navvcube_opacity,
            ),
        );
        Task::none()
    }

    /// A client automation pick (`getpoint`/`user_select`) is waiting on tab
    /// `i`'s document — the cursor behaves like a command point pick: the snap
    /// engine runs on every move and the marker stays until the pick settles.
    pub(in crate::app) fn client_pick_pending(&self, i: usize) -> bool {
        self.control
            .get_point
            .as_ref()
            .is_some_and(|session| session.document_id == self.tabs[i].id)
            || self
                .control
                .user_select
                .as_ref()
                .is_some_and(|session| session.document_id == self.tabs[i].id)
    }

    /// A pointer move straight from the window, which may be coalesced.
    ///
    /// Only genuine input comes through here. Everything that replays a
    /// position it already decided to answer — the snap dwell tick, a
    /// navigation replay, the drag that has just started — calls
    /// [`Self::on_viewport_move`] and is never withheld.
    pub(in crate::app) fn on_pointer_move_coalesced(&mut self, p: Point) -> Task<Message> {
        if self.defer_pointer_move(p) {
            return Task::none();
        }
        self.on_viewport_move(p)
    }

    /// Whether to leave this position for a later frame instead of answering
    /// it now.
    ///
    /// Nothing used to drop a pointer move. On a large drawing a full pass
    /// costs ~15 ms and a mouse reports every few, so the queue could only
    /// grow: a measured drag spent 13.5 s across seven frames working through
    /// 986 positions the cursor had long left, every one of them computed and
    /// then overwritten before any frame drew it.
    ///
    /// A withheld position is not lost, and nothing reads a cursor it did not
    /// write. [`Message::MoveCoalesceTick`] answers it once the gap has
    /// passed; a newer move supersedes it, which only ever drops a position no
    /// frame showed; a press that carries its own position supersedes it in
    /// `set_cursor_for_press`; and anything else is preceded by
    /// [`Self::flush_owed_pointer_move`], so a bare `ViewportLeftPress`
    /// resolving through `last_move_pos` still sees where the pointer went.
    pub(in crate::app) fn defer_pointer_move(&mut self, p: Point) -> bool {
        // `None` until a pass expensive enough to need coalescing: on a
        // drawing where the pass fits inside a frame, no move is ever withheld.
        let Some(due) = self.move_pass_due else {
            return false;
        };
        if Instant::now() >= due {
            return false;
        }
        let i = self.active_tab;
        // SKETCH and freehand REVCLOUD build their result from the path the
        // pointer took, so for them a withheld move is a vertex the drawing
        // never gets.
        if i >= self.tabs.len()
            || self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.samples_motion())
        {
            return false;
        }
        self.move_owed = Some((p, i));
        self.move_withheld = self.move_withheld.saturating_add(1);
        true
    }

    /// Answer a withheld pointer move before a message that may read the
    /// cursor position it would have written.
    ///
    /// A move and its own tick are exempt: the first supersedes the withheld
    /// position and the second is what finally answers it.
    pub(in crate::app) fn flush_owed_pointer_move(&mut self, msg: &Message) -> Task<Message> {
        if self.move_owed.is_none()
            || matches!(
                msg,
                Message::ViewportMove(_)
                    | Message::PaneMove(..)
                    | Message::MoveCoalesceTick
                    | Message::BoxPreviewTick
            )
        {
            return Task::none();
        }
        let Some((p, tab)) = self.move_owed.take() else {
            return Task::none();
        };
        if tab >= self.tabs.len() || tab != self.active_tab {
            return Task::none();
        }
        self.on_viewport_move(p)
    }

    /// Answer the position a coalesced move withheld.
    pub(in crate::app) fn on_move_coalesce_tick(&mut self) -> Task<Message> {
        let Some((p, tab)) = self.move_owed else {
            return Task::none();
        };
        if tab >= self.tabs.len() || tab != self.active_tab {
            self.move_owed = None;
            return Task::none();
        }
        // Still resting. The subscription brings another frame, and the
        // position stays owed until one of them is past the gap.
        if self.move_pass_due.is_some_and(|due| Instant::now() < due) {
            return Task::none();
        }
        self.on_viewport_move(p)
    }

    pub(in crate::app) fn on_viewport_move(&mut self, p: Point) -> Task<Message> {
        // While a cell is typed in the cursor moves (and pans) as usual but
        // takes no snaps.
        let typing =
            self.table_cell_editor.is_some() && self.tabs[self.active_tab].active_cmd.is_none();
        let started = Instant::now();
        let task = self.on_viewport_move_inner(p);
        let index = self.command_interaction_index_task();
        let cost = started.elapsed();
        // Give the next pass as long to arrive as this one took to answer.
        // The moves that arrive meanwhile cost nothing, so the queue drains
        // instead of growing, and the one that outlives the gap is the only
        // position the cursor is actually at. A pass under the threshold
        // leaves the gate shut: it is already keeping up.
        self.move_pass_due = (cost.as_millis() as u64 >= crate::app::MOVE_COALESCE_FROM_MS)
            .then(|| Instant::now() + cost);
        if crate::perf::enabled() {
            crate::perf_record!(
                "[perf] move-pass {:>7.2}ms withheld={} gate={}",
                cost.as_secs_f64() * 1000.0,
                self.move_withheld,
                self.move_pass_due.is_some(),
            );
        }
        self.move_withheld = 0;
        self.move_owed = None;
        if typing {
            self.tabs[self.active_tab].snap_result = None;
            self.snap_dwell = None;
        }
        Task::batch([task, index])
    }

    /// The hover dwell starts the interaction index build, but it does not
    /// run while a command is active. A command's snaps and picks query the
    /// same index, and without it each one scans every wire: after an edit
    /// replaced the wire set (a LAYOFF pick on a large drawing), every move
    /// and click of the command took a second or more. A move during a
    /// command asks for the index here instead.
    pub(in crate::app) fn command_interaction_index_task(&mut self) -> Task<Message> {
        let i = self.active_tab;
        if i >= self.tabs.len() || self.tabs[i].active_cmd.is_none() {
            return Task::none();
        }
        let canvas_sz = self.tabs[i].scene.selection.borrow().view.vp_size;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: canvas_sz.0,
            height: canvas_sz.1,
        };
        let edit_cam = self.tabs[i]
            .scene
            .viewport_edit_frame(canvas_sz)
            .map(|(cam, _)| cam);
        let (_, _, wires) = self.pick_view(i, &edit_cam, bounds);
        self.prepare_interaction_index_task(i, wires, bounds.height)
            .unwrap_or_else(Task::none)
    }

    pub(in crate::app) fn set_selection_cycling(&mut self, mode: i16) {
        if self.selection_cycling != mode {
            self.selection_cycling = mode;
            self.cycle_candidates = None;
            self.cycle_badge = None;
            self.tabs[self.active_tab].scene.set_hover_highlight(None);
        }
    }

    pub(in crate::app) fn toggle_selection_cycling(&mut self, enabled: bool) {
        if enabled != (self.selection_cycling > 0) {
            let mode = if enabled && self.selection_cycling == 0 {
                2
            } else {
                -self.selection_cycling
            };
            self.set_selection_cycling(mode);
        }
    }

    pub(in crate::app) fn commit_point_selection(
        &mut self,
        i: usize,
        handle: Handle,
        shift: bool,
        remove: bool,
    ) -> bool {
        let tab = &mut self.tabs[i];
        if tab
            .active_cmd
            .as_ref()
            .is_some_and(|cmd| cmd.selection_rejects_locked_layers())
            && tab.scene.is_layer_locked(handle)
        {
            self.command_line.push_info("1 was on a locked layer.");
            return false;
        }
        let add = self.pick_add
            || tab
                .active_cmd
                .as_ref()
                .is_some_and(|cmd| cmd.selection_forces_add());
        let selected = tab.scene.selected.contains(&handle);
        let remove = remove || (shift && (add || selected));
        let replace = !add && !shift && !remove;
        if tab
            .active_cmd
            .as_ref()
            .is_some_and(|cmd| cmd.is_selection_gathering())
        {
            self.last_pick = Some((vec![handle], remove));
        }
        if remove {
            tab.scene.deselect_entity(handle);
        } else {
            if replace {
                if let Some(command) = tab.active_cmd.as_mut() {
                    command.clear_selection_markers();
                }
                tab.last_crossing_window = None;
            }
            tab.scene.select_entity(handle, replace);
            tab.scene.expand_selection_for_groups(&[handle]);
        }
        self.queue_properties_refresh(i);
        true
    }

    pub(in crate::app) fn commit_area_selection(
        &mut self,
        i: usize,
        handles: &[Handle],
        marker_hit: bool,
        add: bool,
        gathering: bool,
    ) {
        let scene = &mut self.tabs[i].scene;
        if !self.select_remove_mode && self.shift_down && !add {
            let grouped = scene.handles_expanded_for_selectable_groups(handles);
            let (removed, added): (Vec<_>, Vec<_>) = grouped
                .into_iter()
                .partition(|handle| scene.selected.contains(handle));
            scene.deselect_entities(&removed);
            scene.select_entities(&added);
            self.last_pick = None;
            return;
        }
        let remove = self.select_remove_mode || (self.shift_down && add);
        if gathering {
            self.last_pick = Some((handles.to_vec(), remove));
        }
        if remove {
            scene.deselect_entities(handles);
        } else {
            if !add && (!handles.is_empty() || marker_hit) {
                scene.deselect_all();
            }
            scene.select_entities(handles);
            scene.expand_selection_for_groups(handles);
        }
    }

    pub(in crate::app) fn set_pick_first(&mut self, enabled: bool) {
        self.pick_first = enabled;
        if !enabled {
            for tab in self.tabs.iter_mut().filter(|tab| tab.active_cmd.is_none()) {
                tab.scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                tab.scene.deselect_all();
                tab.scene.set_hover_highlight(None);
            }
            self.cycle_candidates = None;
            self.cycle_badge = None;
            self.refresh_properties();
        }
    }

    pub(in crate::app) fn cycle_lasso_mode(&mut self) -> bool {
        let i = self.active_tab;
        let cursor = {
            let mut state = self.tabs[i].scene.selection.borrow_mut();
            if !state.gesture.poly_active {
                return false;
            }
            state.gesture.cycle_lasso();
            state.input.last_move_pos
        };
        if let Some(cursor) = cursor {
            self.queue_box_selection_preview(i, cursor);
        }
        true
    }

    pub(in crate::app) fn lasso_selection_handles(
        &self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        bounds: iced::Rectangle,
        cursor: Point,
        offset: Point,
    ) -> (Vec<Handle>, Vec<Point>) {
        let tab = &self.tabs[i];
        let (mut points, crossing, open_fence) = {
            let state = tab.scene.selection.borrow();
            (
                state.gesture.lasso_path(cursor),
                state.gesture.poly_crossing,
                state.gesture.poly_fence,
            )
        };
        for point in &mut points {
            point.x -= offset.x;
            point.y -= offset.y;
        }
        let (view, eye, wires) = self.pick_view(i, edit_cam, bounds);
        let handles = tab.scene.path_hit_handles(
            &points,
            crossing,
            open_fence,
            wires,
            view,
            eye,
            bounds,
            |point| self.cursor_model_point(i, edit_cam, point, bounds),
        );
        (handles, points)
    }

    pub(in crate::app) fn update_selection_gesture(&self, i: usize, cursor: Point) {
        let tab = &self.tabs[i];
        let mut state = tab.scene.selection.borrow_mut();
        state.input.last_move_pos = Some(cursor);
        if let Some(anchor) = state.gesture.box_anchor {
            state.gesture.box_current = Some(cursor);
            if !state.gesture.box_crossing_locked {
                state.gesture.box_crossing = crate::scene::selection_box_crossing(anchor, cursor);
            }
            return;
        }
        if !state.input.left_down {
            return;
        }
        if state.gesture.poly_active {
            if state.gesture.poly_points.last().is_none_or(|point| {
                let dx = cursor.x - point.x;
                let dy = cursor.y - point.y;
                dx * dx + dy * dy > 16.0
            }) {
                state.gesture.poly_points.push(cursor);
            }
            return;
        }
        let Some(press) = state
            .input
            .left_press_pos
            .filter(|_| !state.input.left_dragging && !state.input.area_drag_blocked)
        else {
            return;
        };
        let held_ms = state
            .input
            .left_press_time
            .map_or(u128::MAX, |time| time.elapsed().as_millis());
        let lasso = self.pick_auto & 4 != 0;
        // A click may also arm a click-click window (PICKDRAG 0 or 2).
        let click_window = self.pick_auto & 1 != 0 && self.pick_drag_mode != 1;
        if !selection_drag_should_start(press, cursor, held_ms, click_window, lasso) {
            return;
        }
        let active_cmd = tab.active_cmd.as_ref();
        let idle = active_cmd.is_none();
        let over_object = idle && self.pick_auto & 2 != 0;
        let implied_window = self.pick_auto & 1 != 0;
        let command_drag = active_cmd.is_some_and(|cmd| cmd.accepts_drag_selection());
        let enabled = if idle {
            self.pick_first
        } else {
            command_drag || active_cmd.is_some_and(|cmd| cmd.is_selection_gathering())
        };
        if !enabled
            || self.drag_move.is_some()
            || (!command_drag
                && (self.pick_drag_mode == 0 || !(lasso || implied_window || over_object)))
        {
            return;
        }

        let (width, height) = state.view.vp_size;
        drop(state);
        let frame = tab.scene.viewport_edit_frame((width, height));
        let tile = frame.as_ref().map_or_else(
            || tab.scene.active_model_tile_bounds(width, height),
            |(_, tile)| *tile,
        );
        let edit_cam = frame.map(|(cam, _)| cam);
        let bounds = iced::Rectangle::new(Point::ORIGIN, tile.size());
        let local_press = Point::new(press.x - tile.x, press.y - tile.y);
        let force_rectangle = !lasso || (command_drag && self.pick_drag_mode == 0);
        // With over-object dragging enabled, the press target cannot change
        // a forced rectangle. Table-cell ranges are resolved during the drag.
        let needs_press_hit = !force_rectangle || !over_object;
        let table_cell = needs_press_hit
            && idle
            && self
                .table_cell_under(i, local_press, bounds, &edit_cam, 0.0)
                .is_some();
        let hit = needs_press_hit
            && !command_drag
            && !table_cell
            && self
                .selection_object_under(i, local_press, bounds, &edit_cam)
                .is_some();

        let rectangle = force_rectangle || table_cell || hit;
        let world_anchor =
            rectangle.then(|| self.cursor_model_point(i, &edit_cam, local_press, bounds));
        let mut state = tab.scene.selection.borrow_mut();
        if hit && !over_object {
            state.input.area_drag_blocked = true;
            return;
        }
        state.input.left_dragging = true;
        let crossing = crate::scene::selection_box_crossing(press, cursor);
        if rectangle {
            state.gesture.box_anchor = Some(press);
            state.gesture.box_anchor_world = world_anchor;
            state.gesture.box_current = Some(cursor);
            state.gesture.box_crossing = crossing;
        } else {
            state.gesture.poly_active = true;
            state.gesture.poly_crossing = crossing;
            state.gesture.poly_fence = false;
            state.gesture.poly_points.clear();
            state.gesture.poly_points.extend([press, cursor]);
        }
    }

}
