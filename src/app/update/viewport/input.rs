use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn arm_hover_after_navigation(&mut self, i: usize) {
        if i >= self.tabs.len()
            || self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| !command.entity_pick_deferred_hover())
        {
            return;
        }
        let (cursor, canvas_size) = {
            let selection = self.tabs[i].scene.selection.borrow();
            (selection.input.last_move_pos, selection.view.vp_size)
        };
        let Some(cursor) = cursor else {
            return;
        };
        let tile = self.tabs[i]
            .scene
            .viewport_edit_frame(canvas_size)
            .map(|(_, rect)| rect)
            .unwrap_or_else(|| {
                self.tabs[i]
                    .scene
                    .active_model_tile_bounds(canvas_size.0, canvas_size.1)
            });
        self.hover_dwell = Some(crate::app::HoverDwell {
            last_move_at: Instant::now(),
            point: Point::new(cursor.x - tile.x, cursor.y - tile.y),
            tile_size: (tile.width, tile.height),
            tab: i,
            camera_generation: self.tabs[i].scene.camera_generation,
        });
    }

    pub(in crate::app) fn on_viewport_middle_press(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].measurement.dismiss_result();
        self.clear_navigation_hover(i);
        self.ribbon.close_dropdown();
        self.tabs[i].scene.remember_current_view();
        let now = Instant::now();
        let is_double = {
            let sel = self.tabs[i].scene.selection.borrow();
            sel.input
                .middle_last_press_time
                .map(|t| now.duration_since(t).as_millis() < 300)
                .unwrap_or(false)
        };
        {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            let Some(p) = sel.input.last_move_pos else {
                return Task::none();
            };
            sel.input.middle_down = true;
            sel.input.middle_last_pos = Some(p);
            sel.input.middle_last_press_time = Some(now);
            sel.input.middle_pan_from = None;
        }
        if is_double {
            self.tabs[i].scene.fit_all();
            self.tabs[i]
                .scene
                .record_nav_perf(crate::scene::NavPerfOp::Zoom, now);
            self.command_line
                .push_output(crate::t!("Zoom Extents").as_ref());
        }
        Task::none()
    }

    /// Orbit the active view by a screen-pixel movement: the active paper
    /// viewport's view, or the model camera about the selection or the
    /// surface at the view centre.
    pub(in crate::app) fn orbit_active_view(&mut self, i: usize, dx: f32, dy: f32) {
        if self.tabs[i].scene.active_viewport.is_some() {
            self.tabs[i].scene.orbit_active_viewport(dx, dy);
        } else if self.tabs[i].scene.current_layout == "Model" {
            let scene = &self.tabs[i].scene;
            let (vw, vh) = scene.selection.borrow().view.vp_size;
            let bounds = scene.active_model_tile_bounds(vw, vh);
            let pivot = scene
                .orbit_pivot()
                .or_else(|| scene.view_center_surface_pivot(bounds))
                .unwrap_or_else(|| scene.camera.borrow().target);
            self.tabs[i].scene.refresh_projection_bounds();
            self.tabs[i]
                .scene
                .camera
                .borrow_mut()
                .orbit(dx, dy, Some(pivot));
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.tabs[i].scene.prepare_projected_interaction();
        self.tabs[i].scene.camera_generation += 1;
    }

    /// Apply the user's scroll navigation preference to either delta unit.
    pub(in crate::app) fn on_viewport_scroll(&mut self, delta: mouse::ScrollDelta) -> Task<Message> {
        match scroll_intent(delta, self.touchpad_pan, self.ctrl_down, self.shift_down) {
            ScrollIntent::Zoom { notches } => {
                crate::perf::note_scroll_zoom();
                let mut s = notches * crate::app::settings::zoom_notch_steps(self.zoom_factor);
                if self.zoom_wheel_reversed {
                    s = -s;
                }
                self.zoom_view_at_cursor(s)
            }
            ScrollIntent::Orbit { dx, dy } => {
                let i = self.active_tab;
                if !self.tabs[i].is_start {
                    self.clear_navigation_hover(i);
                    self.orbit_active_view(i, dx, dy);
                }
                Task::none()
            }
            ScrollIntent::Pan { dx, dy } => {
                let i = self.active_tab;
                self.tabs[i].scene.remember_current_view();
                self.clear_navigation_hover(i);
                self.pan_active_view(i, dx, dy, Instant::now());
                self.arm_hover_after_navigation(i);
                Task::none()
            }
        }
    }

    /// Pinch the active view to zoom, as reported by the trackpad monitor
    /// (`src/input/trackpad.rs`). The pivot is the cursor, like the wheel, and
    /// the direction is the fingers' own, so ZOOMWHEEL does not apply to it.
    /// A touch screen finger event: multi-finger gestures pan, zoom and
    /// orbit the view (see `input::touch`).
    pub(in crate::app) fn on_touch(&mut self, event: iced::touch::Event) -> Task<Message> {
        use crate::input::touch::TouchAction;
        let i = self.active_tab;
        if self.tabs[i].is_start {
            return Task::none();
        }
        // A lone finger drives the cursor, so where it is in the window and
        // in the drawing area tells the drawing area's offset.
        if let iced::touch::Event::FingerMoved { position, .. } = event {
            let cursor = self.tabs[i].scene.selection.borrow().input.last_move_pos;
            if let Some(cursor) = cursor.filter(|_| self.touch.fingers() == 1) {
                self.touch_offset = Some(position - cursor);
            }
        }
        let mut tasks = Vec::new();
        for action in self.touch.event(event) {
            match action {
                TouchAction::Cancel => {
                    self.tabs[i]
                        .scene
                        .selection
                        .borrow_mut()
                        .clear_left_selection_gesture();
                    self.drag_move = None;
                    self.tabs[i].scene.remember_current_view();
                    self.clear_navigation_hover(i);
                }
                TouchAction::Pan(delta) => {
                    self.pan_active_view(i, delta.x, delta.y, Instant::now());
                }
                TouchAction::Zoom { factor, centre } => {
                    // Zoom about the fingers once the drawing area's place in
                    // the window is known; until then about the first finger.
                    if let Some(offset) = self.touch_offset {
                        self.tabs[i]
                            .scene
                            .selection
                            .borrow_mut()
                            .input
                            .last_move_pos = Some(centre - offset);
                    }
                    tasks.push(self.zoom_view_at_cursor(pinch_zoom_steps(factor - 1.0)));
                }
                TouchAction::Orbit(delta) => self.orbit_active_view(i, delta.x, delta.y),
                TouchAction::Tap(2) => {
                    tasks.push(self.update(Message::ViewportRightPress));
                    tasks.push(self.update(Message::ViewportRightRelease));
                }
                TouchAction::Tap(_) => {
                    tasks.push(Task::done(Message::Command("ZOOM E".to_string())));
                }
                TouchAction::SwipeUp => tasks.push(self.apply_view_preset("SEISO")),
            }
        }
        Task::batch(tasks)
    }

    pub(in crate::app) fn on_pinch_zoom(&mut self, magnification: f32) -> Task<Message> {
        if magnification == 0.0 {
            return Task::none();
        }
        self.zoom_view_at_cursor(pinch_zoom_steps(magnification))
    }

    /// Pan the active view by `dx`/`dy` screen pixels: the body shared by the
    /// middle-button drag and the two-finger gesture.
    pub(in crate::app) fn pan_active_view(&mut self, i: usize, dx: f32, dy: f32, started: Instant) {
        // Pan scale uses the active tile's size (ortho size is relative to
        // viewport height), so a tiled pane pans at the correct rate.
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let bounds = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
        // A display-locked viewport keeps its scale: navigating inside it
        // pans the sheet instead.
        if self.tabs[i].scene.active_viewport.is_some() && !self.tabs[i].scene.navigation_locked() {
            self.tabs[i].scene.pan_active_viewport(dx, dy, bounds);
            // Bump so the GPU re-uploads the viewport's re-culled wire set —
            // otherwise newly-revealed lines stay invisible until MSPACE is
            // exited.
            self.tabs[i].scene.camera_generation += 1;
        } else {
            // `bounds` is the active tile; pan by its height so the point
            // under the cursor tracks correctly.
            self.tabs[i]
                .scene
                .camera
                .borrow_mut()
                .pan_screen(dx, dy, bounds.height);
            self.tabs[i].scene.camera_generation += 1;
            // Keep an in-progress box selection pinned to the drawing as the
            // view pans under it (#234).
            self.reproject_box_anchor(i, vw, vh);
        }
        self.reproject_dyn_guide(i);
        self.tabs[i]
            .scene
            .record_nav_perf(crate::scene::NavPerfOp::Pan, started);
    }

    /// Zoom the active view by `s`, in the camera's own steps
    /// (`Camera::zoom`: `distance *= ZOOM_STEP.powf(s)`). Shared by the
    /// wheel and the pinch.
    pub(in crate::app) fn zoom_view_at_cursor(&mut self, s: f32) -> Task<Message> {
        let nav_started = Instant::now();
        let i = self.active_tab;
        self.tabs[i].scene.remember_current_view();
        self.clear_navigation_hover(i);
        let cursor = self.tabs[i].scene.selection.borrow().input.last_move_pos;
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: vw,
            height: vh,
        };
        if self.tabs[i].scene.active_viewport.is_some() && !self.tabs[i].scene.navigation_locked() {
            // In MSPACE: zoom the active viewport's model-space view,
            // keeping the model point under the cursor stationary. A
            // display-locked viewport keeps its scale, so the sheet zooms.
            let cursor_paper = cursor.map(|cp| {
                let pt = self.tabs[i]
                    .scene
                    .camera
                    .borrow()
                    .pick_on_target_plane(cp, bounds);
                glam::Vec2::new(pt.x as f32, pt.y as f32)
            });
            self.tabs[i].scene.zoom_active_viewport(s, cursor_paper);
            // Bump so the GPU re-uploads the viewport's re-culled wire
            // set after zooming inside it.
            self.tabs[i].scene.camera_generation += 1;
        } else {
            // Model space: zoom about the cursor within the active
            // tile so the point under it stays put in that pane.
            let tile_b = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
            let mut cam = self.tabs[i].scene.camera.borrow_mut();
            if let Some(cursor) = cursor {
                let local = iced::Point {
                    x: cursor.x - tile_b.x,
                    y: cursor.y - tile_b.y,
                };
                let tb = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: tile_b.width,
                    height: tile_b.height,
                };
                cam.zoom_about_point(local, tb, s);
            } else {
                cam.zoom(s);
            }
            drop(cam);
            self.tabs[i].scene.camera_generation += 1;
            // Keep an in-progress box selection pinned to the drawing
            // as the view zooms under it (#234).
            self.reproject_box_anchor(i, vw, vh);
        }
        self.reproject_dyn_guide(i);
        self.tabs[i]
            .scene
            .record_nav_perf(crate::scene::NavPerfOp::Zoom, nav_started);
        self.arm_hover_after_navigation(i);
        Task::none()
    }

    /// A view drag ended while a command or a grip drag waits for a point:
    /// run the cursor's place through the move path again, so the rubber
    /// band, the guide and the value boxes come back where the cursor is now
    /// and not where it was before the view moved.
    pub(in crate::app) fn refresh_pointer_after_navigation(&mut self, i: usize) -> Task<Message> {
        if i >= self.tabs.len()
            || (self.tabs[i].active_cmd.is_none() && self.tabs[i].active_grip.is_none())
        {
            return Task::none();
        }
        let at = self.tabs[i].scene.selection.borrow().input.last_move_pos;
        match at {
            Some(at) => self.on_viewport_move(at),
            None => Task::none(),
        }
    }

    /// Re-project the dynamic-input guide anchors (`last_point_screen`,
    /// `dyn_ref_screen`) from their world points after the view moves
    /// (zoom/pan), using the same projection as the cursor-move handler, so
    /// the guide and value boxes stay on the geometry instead of waiting for
    /// the next cursor move.
    pub(in crate::app) fn reproject_dyn_guide(&mut self, i: usize) {
        if self.tabs[i].active_cmd.is_none() {
            return;
        }
        let vp_size = self.tabs[i].scene.selection.borrow().view.vp_size;
        let edit_frame = self.tabs[i].scene.viewport_edit_frame(vp_size);
        let tile_b = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i]
                .scene
                .active_model_tile_bounds(vp_size.0, vp_size.1),
        };
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: tile_b.width,
            height: tile_b.height,
        };
        let (view_rot, eye) = match &edit_frame {
            Some((cam, _)) => (cam.view_proj_rte(bounds), cam.eye()),
            None => {
                let cam = self.tabs[i].scene.camera.borrow();
                (cam.view_proj_rte(bounds), cam.eye())
            }
        };
        let proj = |bp: glam::DVec3| {
            let ndc = view_rot.project_point3((bp - eye).as_vec3());
            iced::Point::new(
                (ndc.x + 1.0) * 0.5 * bounds.width + tile_b.x,
                (1.0 - ndc.y) * 0.5 * bounds.height + tile_b.y,
            )
        };
        let anchor = self.tabs[i].dyn_anchor.or(self.last_point);
        let dyn_ref = self.tabs[i].dyn_ref;
        self.tabs[i].last_point_screen = anchor.map(&proj);
        self.tabs[i].dyn_ref_screen = dyn_ref.map(&proj);
    }

    /// Re-pin an active box-selection anchor to its stored world point after the
    /// model-space view moves (zoom/pan), so the selection rectangle tracks the
    /// drawing instead of staying frozen at its original pixel. No-op when no
    /// box is in progress. (#234)
    pub(in crate::app) fn reproject_box_anchor(&mut self, i: usize, vw: f32, vh: f32) {
        let world = self.tabs[i]
            .scene
            .selection
            .borrow()
            .gesture
            .box_anchor_world;
        let Some(world) = world else { return };
        let tile_b = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
        let tile_local = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: tile_b.width,
            height: tile_b.height,
        };
        if let Some(sp) = self.tabs[i]
            .scene
            .camera
            .borrow()
            .project(world, tile_local)
        {
            self.tabs[i].scene.selection.borrow_mut().gesture.box_anchor =
                Some(iced::Point::new(sp.x + tile_b.x, sp.y + tile_b.y));
        }
    }

    pub(in crate::app) fn on_viewport_click(
        &mut self,
        expected_viewport: Option<document::Handle>,
    ) -> Task<Message> {
        if self.table_cell_editor.is_some() && self.tabs[self.active_tab].active_cmd.is_none() {
            return self.table_editor_click(self.cursor_pos);
        }
        let i = self.active_tab;
        if self.tabs[i].scene.active_viewport != expected_viewport
            || (expected_viewport.is_none() && self.tabs[i].scene.current_layout != "Model")
        {
            return Task::none();
        }
        let rot = self.tabs[i].scene.active_view_rotation_mat();
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        // The ViewCube draws in the top-right of whichever area
        // owns it: the full canvas in model space, or the active
        // viewport's screen rectangle in a paper layout. Map the
        // cursor into that area before hit-testing so paper-space
        // picks line up with the gizmo.
        let (cx, cy, w, h) = match self.tabs[i]
            .scene
            .active_viewport
            .and_then(|hndl| self.tabs[i].scene.viewport_screen_rect(hndl, (vw, vh)))
        {
            Some(rect) => (
                self.cursor_pos.x - rect.x,
                self.cursor_pos.y - rect.y,
                rect.width,
                rect.height,
            ),
            None => {
                // Model layout: hit-test within the active tile.
                let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                (
                    self.cursor_pos.x - tb.x,
                    self.cursor_pos.y - tb.y,
                    tb.width,
                    tb.height,
                )
            }
        };
        // Prefer the currently-highlighted region: hover is recomputed
        // on every move straight from the cube's own overlay, so it is
        // immune to `cursor_pos` being overwritten by the viewport's
        // move handler between the last move and this press.
        if let Some(id) = self.tabs[i].scene.viewcube_hover.get() {
            let region = if id < 6 {
                scene::CubeRegion::Face(id)
            } else if id < 18 {
                scene::CubeRegion::Edge(id)
            } else {
                scene::CubeRegion::Corner(id)
            };
            return self.on_view_cube_snap(region);
        }
        if let Some(region) = scene::hit_test(cx, cy, w, h, rot, VIEWCUBE_PX) {
            return self.on_view_cube_snap(region);
        }
        // Compass cardinals are world-fixed: hit-test through the camera-
        // only rotation (strip the UCS) so the target matches the drawn
        // N/E/S/W, and snap in world frame.
        let rot_world = rot * self.tabs[i].scene.viewcube_ucs_mat().inverse();
        if let Some(card) = scene::hit_test_cardinal(cx, cy, w, h, rot_world, VIEWCUBE_PX) {
            return self.on_view_cube_snap_world(card.face_region());
        }
        Task::none()
    }

    pub(in crate::app) fn on_view_cube_snap(&mut self, region: CubeRegion) -> Task<Message> {
        // The cube is oriented in the active UCS, so snap in the UCS frame.
        let i = self.active_tab;
        let r_ucs = self.tabs[i].scene.viewcube_ucs_mat();
        let task = self.snap_view_region(region, r_ucs);
        // Alt+click on a face also sets the UCS to that view (#2272).
        if self.alt_down
            && matches!(region, CubeRegion::Face(_))
            && self.tabs[i].active_cmd.is_none()
        {
            return task.chain(self.dispatch_command("UCS V"));
        }
        task
    }

    /// Compass cardinals are world-fixed, so they snap in the world frame
    /// (no UCS composition).
    pub(in crate::app) fn on_view_cube_snap_world(&mut self, region: CubeRegion) -> Task<Message> {
        self.snap_view_region(region, glam::Mat4::IDENTITY)
    }

    pub(in crate::app) fn snap_view_region(&mut self, region: CubeRegion, r_ucs: glam::Mat4) -> Task<Message> {
        self.snap_view_region_as(region, r_ucs, true)
    }

    /// A standard view preset (-VIEW): the world view itself, kept when it
    /// is shown already, with no cube message.
    pub(in crate::app) fn snap_view_preset(&mut self, region: CubeRegion) -> Task<Message> {
        self.snap_view_region_as(region, glam::Mat4::IDENTITY, false)
    }

    pub(in crate::app) fn snap_view_region_as(
        &mut self,
        region: CubeRegion,
        r_ucs: glam::Mat4,
        cube: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.clear_navigation_hover(i);
        self.tabs[i].scene.remember_current_view();
        let mut region = region;
        // "Already there → flip to opposite" check for a cube click: compare
        // the current gaze direction with the region's target gaze.
        let target_dir = r_ucs.transform_vector3(region.snap_direction());
        let cur_dir = self.tabs[i].scene.active_gaze_dir();
        if cube && cur_dir.dot(target_dir) > 0.9999 {
            region = region.opposite();
        }
        let eye_dir = r_ucs.transform_vector3(region.snap_direction());

        // Faces and edges/corners all snap deterministically (world +Z
        // horizon, north +Y for top/bottom) so a cube click is repeatable
        // and matches the turntable orbit; snap_to_direction preserves an
        // intentional upside-down sense by sign-flipping only.
        let is_face = matches!(region, scene::CubeRegion::Face(_));
        if self.tabs[i].scene.active_viewport.is_some() {
            if is_face {
                self.tabs[i]
                    .scene
                    .mutate_active_viewport_camera(|c| c.snap_to_face(eye_dir, r_ucs));
            } else {
                self.tabs[i]
                    .scene
                    .snap_active_viewport_to_direction(eye_dir, r_ucs);
            }
        } else {
            self.tabs[i].scene.refresh_projection_bounds();
            let mut cam = self.tabs[i].scene.camera.borrow_mut();
            if is_face {
                cam.snap_to_face(eye_dir, r_ucs);
            } else {
                cam.snap_to_direction(eye_dir, r_ucs);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.tabs[i].scene.prepare_projected_interaction();
        self.tabs[i].scene.camera_generation += 1;
        if cube {
            self.command_line
                .push_output(crate::tf!("View: {}", crate::t!(region.label())).as_ref());
        }
        Task::none()
    }

    pub(in crate::app) fn on_hover_dwell_tick(&mut self) -> Task<Message> {
        let Some(dwell) = self.hover_dwell.clone() else {
            return Task::none();
        };
        let dwell_ms = crate::app::HOVER_DWELL_MS;
        let now = Instant::now();
        if now.duration_since(dwell.last_move_at).as_millis() < dwell_ms
            && now < self.hover_pick_due
        {
            return Task::none();
        }
        let perf = crate::perf::enabled();
        let hover_started = Instant::now();
        let i = dwell.tab;
        // Re-check the gate — drag / command may have started
        // between the move that armed the dwell and this tick.
        if i >= self.tabs.len() || i != self.active_tab {
            self.hover_dwell = None;
            return Task::none();
        }
        // A wheel zoom keeps the cursor still, so a dwell armed before the
        // zoom would otherwise fire mid-navigation and stall it with the
        // O(N) settled pick. The camera generation moved since the cursor
        // came to rest: restart the rest clock on the new view and pick
        // only once that has settled too (#3339).
        if self.tabs[i].scene.camera_generation != dwell.camera_generation {
            self.hover_dwell = Some(crate::app::HoverDwell {
                last_move_at: Instant::now(),
                point: dwell.point,
                tile_size: dwell.tile_size,
                tab: dwell.tab,
                camera_generation: self.tabs[i].scene.camera_generation,
            });
            return Task::none();
        }
        // While the quad's own hover dwell is counting with no quad open
        // (its configured delay is beyond the rollover's), re-running the
        // whole settled pick below would repeat identically: none of its
        // inputs moved since the last completed pick. Skip the O(N) sweep
        // and only drive the quad step / re-arm (docs/quad-menu.md §3.4) —
        // the sweep re-runs as soon as the cursor (move count) or a scene /
        // camera pick token changed.
        if self.quad_hover_counting() && self.tabs[i].active_cmd.is_none() {
            let unchanged = self.quad_dwell_pick.as_ref().is_some_and(|pick| {
                pick.tab == i
                    && pick.point == dwell.point
                    && pick.move_seq == self.hover_move_seq
                    && pick.geometry_epoch == self.tabs[i].scene.geometry_epoch
                    && pick.camera_generation == self.tabs[i].scene.camera_generation
                    && pick.selection_preview == self.model_space.selection_preview
                    && pick.pick_box == self.pick_box
            });
            if unchanged {
                let rest = dwell.last_move_at.elapsed().as_millis() as u64;
                self.quad_hover_step(i, dwell.point, rest);
                // The settled pick's re-arm discipline when it leaves the
                // dwell counting, applied here too: keep polling at the
                // rollover cadence until the quad opened or the rest
                // disqualified (a further move re-arms from the move).
                if self.quad_hover_counting() {
                    self.hover_dwell = Some(crate::app::HoverDwell {
                        last_move_at: Instant::now(),
                        point: dwell.point,
                        tile_size: dwell.tile_size,
                        tab: dwell.tab,
                        camera_generation: self.tabs[dwell.tab].scene.camera_generation,
                    });
                } else {
                    self.hover_dwell = None;
                }
                return Task::none();
            }
        }
        self.push_ucs_to_cmd(i);
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
        }
        let deferred_command = self.tabs[i].active_cmd.as_ref().is_some_and(|command| {
            command.needs_entity_pick() && command.entity_pick_deferred_hover()
        });
        let is_gathering = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.is_selection_gathering());
        let navigating = self.tabs[i].scene.selection.borrow().input.middle_down;
        if (self.tabs[i].active_cmd.is_some() && !deferred_command && !is_gathering) || navigating {
            self.clear_navigation_hover(i);
            return Task::none();
        }
        let cursor = self.tabs[i].scene.selection.borrow().input.last_move_pos;
        self.constraint_glyph_tooltip =
            cursor.and_then(|point| self.constraint_glyph_under(i, point).map(|(kind, _)| kind));
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: dwell.tile_size.0,
            height: dwell.tile_size.1,
        };
        let p = dwell.point;
        // Inside a viewport, hover-pick through the viewport camera +
        // model wires so the rollover highlights the entity under the
        // cursor (dwell.point / dwell.tile_size are already pane-local).
        let canvas_sz = self.tabs[i].scene.selection.borrow().view.vp_size;
        let edit_cam = self.tabs[i]
            .scene
            .viewport_edit_frame(canvas_sz)
            .map(|(cam, _)| cam);
        let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
        if let Some(task) =
            self.prepare_interaction_index_task(i, Arc::clone(&all_wires), bounds.height)
        {
            // Keep the dwell armed. While the index is pending later timer
            // ticks return immediately; once installed, the next tick performs
            // the exact pick at the latest cursor position.
            return task;
        }
        let hover_world = self.cursor_model_point(i, &edit_cam, p, bounds);
        let candidate_started = Instant::now();
        let hover_candidates = self.tabs[i].scene.interaction_hover_candidates_near(
            all_wires,
            hover_world,
            view_rot,
            eye,
            bounds,
            crate::ui::overlay::pick_box_aperture_px(self.pick_box) * 2.0,
        );
        let candidate_ms = candidate_started.elapsed().as_secs_f64() * 1000.0;
        let candidate_count = hover_candidates.len();
        let handles_started = Instant::now();
        let candidate_handles = self.tabs[i]
            .scene
            .interaction_candidate_handles(&hover_candidates);
        let handles_ms = handles_started.elapsed().as_secs_f64() * 1000.0;
        // Mirror the click-selection pick order so the rollover
        // highlights every selectable object: wire → hatch →
        // block-internal hatch → shaded 3D solid body.
        let wire_started = Instant::now();
        // A pathological draw (tens of thousands of segments inside the pick
        // aperture) would still cost more than a hover highlight is worth;
        // skip the wire test rather than stall the UI thread (#3339). Click
        // picks keep their full sweep — a deliberate user action earns it.
        let over_candidate_cap = candidate_count > crate::app::HOVER_CANDIDATE_CAP;
        self.cycle_badge = if self.selection_cycling > 0
            && !over_candidate_cap
            && (is_gathering || (self.tabs[i].active_cmd.is_none() && self.pick_first))
        {
            let scene = &self.tabs[i].scene;
            let mut hits = scene::pick::hit_test::click_hits_all(
                p,
                &hover_candidates,
                view_rot,
                eye,
                bounds,
                scene.document.header.lineweight_display,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
            )
            .into_iter()
            .filter_map(Scene::handle_from_wire_name)
            .filter(|&handle| {
                scene.passes_selection_filter(handle) && !scene.is_reference_only(handle)
            });
            hits.next()
                .filter(|first| hits.any(|other| other != *first))
                .and(cursor.map(|point| (self.tabs[i].id, point)))
        } else {
            None
        };
        let mut hovered = if over_candidate_cap {
            None
        } else {
            scene::pick::hit_test::click_hit(
                p,
                &hover_candidates,
                view_rot,
                eye,
                bounds,
                self.tabs[i].scene.document.header.lineweight_display,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
            )
            .and_then(Scene::handle_from_wire_name)
        };
        let wire_ms = wire_started.elapsed().as_secs_f64() * 1000.0;
        if over_candidate_cap && perf {
            crate::perf_record!(
                "[perf] hover-capped   candidates={} wire sweep skipped",
                candidate_count
            );
        }
        let mut hatch_ms = 0.0;
        let mut insert_ms = 0.0;
        let mut solid_ms = 0.0;
        if hovered.is_none() {
            let started = Instant::now();
            hovered = scene::pick::hit_test::click_hit_hatch(
                p,
                &self.tabs[i]
                    .scene
                    .visible_hatches_for_click(candidate_handles.as_ref()),
                view_rot,
                eye,
                bounds,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
                candidate_handles.as_ref(),
            );
            hatch_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if hovered.is_none() {
            let started = Instant::now();
            hovered = scene::pick::hit_test::click_hit_insert_hatch(
                p,
                self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                view_rot,
                eye,
                bounds,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
                candidate_handles.as_ref(),
            );
            insert_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if hovered.is_none() {
            let started = Instant::now();
            hovered = if deferred_command {
                self.tabs[i]
                    .scene
                    .solid_click_hit(p, view_rot, eye, bounds, candidate_handles.as_ref())
                    .or_else(|| {
                        self.tabs[i].scene.solid_edge_hover_hit(
                            p,
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                            crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                        )
                    })
            } else {
                self.solid_select_hit(
                    i,
                    p,
                    view_rot,
                    eye,
                    bounds,
                    candidate_handles.as_ref(),
                    crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                )
            };
            solid_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if deferred_command {
            let surface_point = hovered.and_then(|handle| {
                self.solid_pick_point(i, handle, &edit_cam, p, bounds, view_rot, eye)
                    .or_else(|| self.profile_pick_point(i, handle, &edit_cam, p, bounds))
            });
            let point = surface_point.unwrap_or(hover_world);
            let is_solid = hovered.is_some_and(|handle| {
                matches!(
                    self.tabs[i].scene.document.get_entity(handle),
                    Some(document::EntityType::Solid3D(_)),
                )
            });
            // An aperture may catch an edge beside, rather than on, a face.
            // Never send its XY-plane fallback as a 3D face pick.
            if is_solid && surface_point.is_none() {
                self.tabs[i].scene.set_hover_highlight(None);
                self.tabs[i].scene.clear_preview_wire();
                self.hover_dwell = None;
                return Task::none();
            }
            let handles = self.tabs[i]
                .scene
                .document
                .entities()
                .filter_map(|entity| {
                    matches!(entity, document::EntityType::Solid3D(_))
                        .then_some(entity.common().handle)
                })
                .collect::<Vec<_>>();
            self.tabs[i].scene.restore_solid_models(&handles);
            let show = (self.model_space.selection_preview & 2) != 0;
            let previews = if show {
                let tab = &mut self.tabs[i];
                tab.active_cmd
                    .as_mut()
                    .map(|command| command.on_deferred_entity_hover(&tab.scene, hovered, point))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            self.tabs[i].scene.set_hover_highlight(None);
            self.tabs[i].scene.set_preview_wires(previews);
            self.hover_dwell = None;
            return Task::none();
        }
        let preview_enabled = if self.tabs[i].active_cmd.is_none() {
            (self.model_space.selection_preview & 1) != 0
        } else {
            (self.model_space.selection_preview & 2) != 0
        };
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.selection_rejects_locked_layers())
        {
            if let Some(h) = hovered {
                if self.tabs[i].scene.is_layer_locked(h) {
                    hovered = None;
                }
            }
        }
        if preview_enabled {
            self.tabs[i].scene.set_hover_highlight(hovered);
        } else {
            self.tabs[i].scene.set_hover_highlight(None);
        }
        // Record what this settled pick saw so this tick's quad poll can
        // skip an identical repetition while the cursor rests (see the skip
        // gate at the top; the tokens are the pick's own inputs).
        self.quad_dwell_pick = Some(crate::app::QuadDwellPick {
            tab: i,
            point: p,
            move_seq: self.hover_move_seq,
            geometry_epoch: self.tabs[i].scene.geometry_epoch,
            camera_generation: self.tabs[i].scene.camera_generation,
            selection_preview: self.model_space.selection_preview,
            pick_box: self.pick_box,
        });
        // The quad cursor menu's hover dwell (docs/quad-menu.md §3.4) feeds
        // off this authoritative settled pick: `rest_ms` is how long the
        // cursor has been resting, so a dwell that already crossed the
        // configured delay opens the quad at once. While the quad's own
        // delay is still counting, the tick re-arms itself — a cursor that
        // rests forever generates no move events, and without the re-arm a
        // delay beyond this tick would never be crossed.
        self.quad_hover_step(i, p, dwell.last_move_at.elapsed().as_millis() as u64);
        if self.quad_hover_counting() {
            // Restart the dwell clock so polling happens at the original
            // rollover cadence (a tick every HOVER_DWELL_MS), not every
            // frame; a cancellation on the next move re-arms from the move.
            self.hover_dwell = Some(crate::app::HoverDwell {
                last_move_at: Instant::now(),
                point: dwell.point,
                tile_size: dwell.tile_size,
                tab: dwell.tab,
                camera_generation: self.tabs[i].scene.camera_generation,
            });
        } else {
            self.hover_dwell = None;
        }
        let gap = std::time::Duration::from_millis(crate::app::SNAP_MOVING_MS)
            .max(hover_started.elapsed() * 4);
        self.hover_pick_due = Instant::now() + gap;
        let hover_ms = hover_started.elapsed().as_secs_f64() * 1000.0;
        if perf && hover_ms >= 5.0 {
            crate::perf_record!(
                "[perf] hover-detail       query={candidate_ms:.1} handles={handles_ms:.1} wire={wire_ms:.1} hatch={hatch_ms:.1} insert={insert_ms:.1} solid={solid_ms:.1} candidates={candidate_count}",
            );
            crate::perf_record!(
                "[perf] hover-dwell        {:>7.1}ms wires={} hit={}",
                hover_ms,
                self.tabs[i].scene.last_tess_wires.get(),
                hovered.is_some(),
            );
        }
        Task::none()
    }

    /// Runs when the cursor has been still for `SNAP_DWELL_MS` while a
    /// command is active. Clears the deferred-snap gate so the next
    /// pointer position runs the full snap pipeline. Preview work is limited
    /// to one rebuild per frame while the pointer is still moving.
    pub(in crate::app) fn on_snap_dwell_tick(&mut self) -> Task<Message> {
        let Some(dwell) = self.snap_dwell.clone() else {
            return Task::none();
        };
        if dwell.snap_resolved && !dwell.preview_dirty {
            return Task::none();
        }
        // Re-check the gate — command may have ended or the tab changed
        // between the move that armed the dwell and this tick.
        let i = dwell.tab;
        if i >= self.tabs.len() || i != self.active_tab || self.tabs[i].active_cmd.is_none() {
            self.snap_dwell = None;
            return Task::none();
        }
        let now = Instant::now();
        if now.duration_since(dwell.last_move_at).as_millis() < crate::app::SNAP_DWELL_MS
            && now < dwell.full_snap_due
        {
            if dwell.preview_dirty {
                self.refresh_active_cmd_preview(i);
                if let Some(pending) = self.snap_dwell.as_mut() {
                    pending.preview_dirty = false;
                }
            }
            return Task::none();
        }
        // Resolve synchronously. Queuing ViewportMove here can replay this
        // old position AFTER a newer move/press and send the cursor backwards.
        self.snap_dwell = None;
        // on_viewport_move re-arms the throttle. Pending stationary tracking
        // acquisition keeps it unresolved until the candidate is acquired.
        // Other resolved snaps stop requesting ticks until the next move.
        let task = self.on_viewport_move(dwell.point);
        let cost = now.elapsed();
        if let Some(armed) = self.snap_dwell.as_mut() {
            let gap = std::time::Duration::from_millis(crate::app::SNAP_MOVING_MS).max(cost * 4);
            armed.full_snap_due = Instant::now() + gap;
        }
        task
    }
    pub(in crate::app) fn prepare_interaction_index_task(
        &mut self,
        i: usize,
        wires: Arc<Vec<crate::scene::WireModel>>,
        screen_height_px: f32,
    ) -> Option<Task<Message>> {
        let (epoch, source) = self.tabs[i]
            .scene
            .interaction_index_build_key(&wires, screen_height_px)?;
        let tab_id = self.tabs[i].id;
        let key = (tab_id, epoch, source);
        if let Some(active) = self.active_interaction_index {
            if active != key {
                if let Some(position) = self
                    .queued_interaction_indices
                    .iter()
                    .position(|(queued_tab, ..)| *queued_tab == tab_id)
                {
                    self.queued_interaction_indices.remove(position);
                }
                self.queued_interaction_indices.push_back((
                    tab_id,
                    epoch,
                    source,
                    wires,
                    screen_height_px,
                ));
            }
            return Some(Task::none());
        }
        self.tabs[i]
            .scene
            .mark_interaction_index_pending(epoch, source);
        self.active_interaction_index = Some(key);
        let weak = Arc::downgrade(&wires);
        #[cfg(not(target_arch = "wasm32"))]
        let projected = self.tabs[i].scene.view_needs_projected_interaction();
        Some(Task::perform(
            async move {
                let started = iced::time::Instant::now();
                let index = Arc::new(
                    crate::scene::pick::interaction_index::InteractionIndex::build(&wires),
                );
                #[cfg(not(target_arch = "wasm32"))]
                if projected {
                    index.prepare_screen_in_background();
                }
                Message::InteractionIndexReady {
                    tab_id,
                    epoch,
                    source,
                    wires: weak,
                    index,
                    build_ms: started.elapsed().as_secs_f64() * 1000.0,
                }
            },
            |message| message,
        ))
    }

}
