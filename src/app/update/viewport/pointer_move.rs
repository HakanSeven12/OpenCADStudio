use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn on_viewport_move_inner(&mut self, p: Point) -> Task<Message> {
        crate::perf::note_pointer_move();
        // A ribbon dropdown is open over the viewport. Its backdrop
        // cannot swallow cursor motion — in iced 0.14 mouse_area/opaque
        // capture only button presses, never CursorMoved — so the move
        // leaks through the stack to the pane mouse_area beneath and
        // would track the crosshair over the dropdown's empty areas.
        // Drop the move here instead. (#227)
        if self.ribbon.open_dropdown.is_some() || self.color_picker.is_some() {
            return Task::none();
        }
        let i = self.active_tab;
        let perf_move = crate::perf::enabled();
        let move_started = Instant::now();
        self.tabs[i].measurement.live_visible = true;
        self.tabs[i].scene.selection.borrow_mut().input.pointer_away = false;
        self.nav_pointer_moved(i, p);
        if let Some(task) = self.start_drag_move(i, p) {
            return Task::batch([task, self.on_viewport_move(p)]);
        }
        self.constraint_glyph_tooltip = None;
        if self.tabs[i].active_cmd.is_none() {
            let constraint_hover = self
                .constraint_glyph_under(i, p)
                .map(|(_, references)| references)
                .unwrap_or_default();
            self.tabs[i]
                .scene
                .set_constraint_hover_highlights(&constraint_hover);
        } else {
            // Constraint pills are not command targets: command clicks resolve
            // through the command's own picker. Avoid rebuilding the glyph
            // placement cache key (which scans selection/visibility sets) on
            // every pointer event while drawing, and clear any idle hover once.
            self.tabs[i].scene.set_constraint_hover_highlights(&[]);
        }
        self.refresh_ducs_face_highlight(i, p);
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.selection_path().is_some())
        {
            let world = self.cursor_world_at(i, p);
            self.refresh_path_selection_preview(i, world);
        }
        // Modifier-driven selection must be known before cursor_plane/axis and
        // drafting constraints are read, not merely before the final callback.
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
        }
        let pre_grip_ms = crate::perf::elapsed_ms(perf_move.then_some(move_started));

        // UCS icon grip drag: map the cursor onto the UCS plane and
        // slide the origin / rotate the axis. Short-circuits pan & snap.
        if let Some(kind) = self.ucs_grip_drag {
            self.drag_ucs_grip(i, kind, p);
            self.tabs[i]
                .scene
                .selection
                .borrow_mut()
                .input
                .last_move_pos = Some(p);
            return Task::none();
        }

        // Keep the ViewCube hover in sync as the cursor leaves the
        // hit-area overlay and moves over the rest of the viewport.
        // `hover_id` returns None outside the cube box, which clears
        // any stale highlight from the previous `CursorMoved`.
        let (svw, svh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let cube_tile = match self.tabs[i]
            .scene
            .active_viewport
            .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (svw, svh)))
        {
            Some(rect) => rect,
            None => self.tabs[i].scene.active_model_tile_bounds(svw, svh),
        };
        let cam_rot = self.tabs[i].scene.active_view_rotation_mat();
        self.tabs[i].scene.viewcube_hover.set(hover_id(
            p.x - cube_tile.x,
            p.y - cube_tile.y,
            cube_tile.width,
            cube_tile.height,
            cam_rot,
            VIEWCUBE_PX,
        ));
        self.tabs[i].scene.viewcube_opacity.set(
            crate::scene::pipeline::viewcube::viewcube_opacity(
                p.x - cube_tile.x,
                p.y - cube_tile.y,
                cube_tile.width,
                self.navvcube_opacity,
            ),
        );

        let grip_hover_started = perf_move.then(Instant::now);
        let navigating = self.tabs[i].scene.selection.borrow().input.middle_down;
        if navigating {
            self.clear_navigation_hover(i);
        } else {
            // Multi-functional grip hover: detect cursor sitting on a
            // selected entity's grip and, after a dwell, open the
            // popup menu. See scene::model::object::GripMenuItem.
            self.update_grip_hover(i, p);

            // UCS icon hover highlight (suppressed mid grip-drag).
            self.ucs_icon_hover = self.ucs_grip_drag.is_none()
                && !self.custom_block_grip_at_cursor(i, p, svw, svh)
                && self
                    .ucs_icon_hit_info(i, svw, svh)
                    .map(|h| over_ucs_icon(p, &h))
                    .unwrap_or(false);
        }

        let grip_hover_ms = crate::perf::elapsed_ms(grip_hover_started);
        self.update_selection_gesture(i, p);
        self.queue_box_selection_preview(i, p);
        let mut sel = self.tabs[i].scene.selection.borrow_mut();

        let (mid_down, mid_last, vp_size) = (
            sel.input.middle_down,
            sel.input.middle_last_pos,
            sel.view.vp_size,
        );
        if mid_down {
            if let Some(last) = mid_last {
                let (mut dx, mut dy) = (p.x - last.x, p.y - last.y);
                // Shift held during a pan constrains it to the axis it has
                // mostly moved along; the orbit Shift brings is for a drag
                // that starts with it (#2155).
                let constrained_from = sel.input.middle_pan_from.filter(|_| {
                    self.shift_down
                        && !self.tabs[i].orbit_mode
                        && self.navigation_style == crate::app::settings::NavigationStyle::Cad
                });
                if let Some(from) = constrained_from {
                    if (p.x - from.x).abs() >= (p.y - from.y).abs() {
                        dy = 0.0;
                    } else {
                        dx = 0.0;
                    }
                }
                use crate::app::settings::NavAction;
                let action = self.nav_drag_action(i);
                if action == NavAction::Zoom {
                    let bounds = self.tabs[i]
                        .scene
                        .active_model_tile_bounds(vp_size.0, vp_size.1);
                    drop(sel);
                    let zoom_delta = -dy * 0.03;
                    // ZOOM Dynamic also pans sideways; a style's zoom drag
                    // only zooms.
                    let dx = if self.tabs[i].zoom_dynamic_mode {
                        dx
                    } else {
                        0.0
                    };
                    if self.tabs[i].scene.active_viewport.is_some()
                        && !self.tabs[i].scene.navigation_locked()
                    {
                        self.tabs[i].scene.pan_active_viewport(dx, 0.0, bounds);
                        self.tabs[i].scene.zoom_active_viewport(zoom_delta, None);
                    } else {
                        let local = iced::Point {
                            x: p.x - bounds.x,
                            y: p.y - bounds.y,
                        };
                        let local_bounds = iced::Rectangle {
                            x: 0.0,
                            y: 0.0,
                            width: bounds.width,
                            height: bounds.height,
                        };
                        let mut camera = self.tabs[i].scene.camera.borrow_mut();
                        camera.pan_screen(dx, 0.0, bounds.height);
                        camera.zoom_about_point(local, local_bounds, zoom_delta);
                    }
                    self.tabs[i].scene.camera_generation += 1;
                    self.tabs[i]
                        .scene
                        .record_nav_perf(crate::scene::NavPerfOp::Zoom, move_started);
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    // Latch the direction for the magnifier cursor. A dead
                    // zone keeps the +/- glyph from strobing when a slow drag
                    // wobbles a pixel either side of where it started.
                    if dy.abs() >= ZOOM_DIR_FLIP_PX {
                        sel.input.zoom_dir_out = dy > 0.0;
                    }
                    sel.input.middle_last_pos = Some(p);
                    return Task::none();
                }
                // Shift+MMB drag orbits the model view instead of panning
                // — the requested Zoom=wheel / Pan=MMB / Rotate=Shift+MMB
                // scheme (#229). Floating viewports and paper keep the
                // plain MMB pan.
                if action == NavAction::Roll {
                    drop(sel);
                    let scene = &self.tabs[i].scene;
                    if scene.active_viewport.is_none() && scene.current_layout == "Model" {
                        scene.camera.borrow_mut().roll_by(dx * 0.005);
                        self.tabs[i].scene.camera_generation += 1;
                    }
                    self.tabs[i]
                        .scene
                        .selection
                        .borrow_mut()
                        .input
                        .middle_last_pos = Some(p);
                    return Task::none();
                }
                if action == NavAction::Orbit && constrained_from.is_none() {
                    if self.tabs[i].scene.active_viewport.is_some() {
                        // Orbit the floating viewport's own model view.
                        drop(sel);
                        self.tabs[i].scene.orbit_active_viewport(dx, dy);
                        self.tabs[i].scene.camera_generation += 1;
                        self.tabs[i]
                            .scene
                            .record_nav_perf(crate::scene::NavPerfOp::Rotate, move_started);
                        self.tabs[i]
                            .scene
                            .selection
                            .borrow_mut()
                            .input
                            .middle_last_pos = Some(p);
                        return Task::none();
                    } else if self.tabs[i].scene.current_layout == "Model"
                        && self.navigation_style == crate::app::settings::NavigationStyle::Catia
                        && !self.tabs[i].orbit_mode
                    {
                        // CATIA turns the view about the screen centre: an
                        // arcball inside a ring of 42 % of the smaller side,
                        // a roll about the view axis outside it.
                        drop(sel);
                        let bounds = self.tabs[i]
                            .scene
                            .active_model_tile_bounds(vp_size.0, vp_size.1);
                        let center = Point::new(
                            bounds.x + bounds.width * 0.5,
                            bounds.y + bounds.height * 0.5,
                        );
                        let ring = 0.42 * bounds.width.min(bounds.height);
                        let (ax, ay) = (last.x - center.x, last.y - center.y);
                        let (bx, by) = (p.x - center.x, p.y - center.y);
                        {
                            let mut camera = self.tabs[i].scene.camera.borrow_mut();
                            if ax.hypot(ay) <= ring {
                                camera.trackball(dx, dy);
                            } else {
                                camera.roll_by((ax * by - ay * bx).atan2(ax * bx + ay * by));
                            }
                        }
                        self.tabs[i].scene.camera_generation += 1;
                        self.tabs[i]
                            .scene
                            .record_nav_perf(crate::scene::NavPerfOp::Rotate, move_started);
                        self.tabs[i]
                            .scene
                            .selection
                            .borrow_mut()
                            .input
                            .middle_last_pos = Some(p);
                        return Task::none();
                    } else if self.tabs[i].scene.current_layout == "Model" {
                        if sel.orbit.pivot.is_none() {
                            let scene = &self.tabs[i].scene;
                            let bounds = scene.active_model_tile_bounds(vp_size.0, vp_size.1);
                            // The surface under the pointer where the drag
                            // began, so the model turns about what is grabbed —
                            // even with a selection or a command picking points.
                            // The selection centre only stands in over empty space.
                            let grabbed = iced::Point::new(last.x - bounds.x, last.y - bounds.y);
                            sel.orbit.pivot = Some(
                                scene
                                    .surface_pivot_at(bounds, grabbed)
                                    .or_else(|| scene.orbit_pivot())
                                    .or_else(|| scene.view_center_surface_pivot(bounds))
                                    .unwrap_or_else(|| scene.camera.borrow().target),
                            );
                        }
                        let pivot = sel.orbit.pivot;
                        drop(sel);
                        self.tabs[i].scene.refresh_projection_bounds();
                        self.tabs[i].scene.camera.borrow_mut().orbit(dx, dy, pivot);
                        self.tabs[i].scene.camera_generation += 1;
                        self.tabs[i]
                            .scene
                            .record_nav_perf(crate::scene::NavPerfOp::Rotate, move_started);
                        self.tabs[i]
                            .scene
                            .selection
                            .borrow_mut()
                            .input
                            .middle_last_pos = Some(p);
                        return Task::none();
                    }
                    // Paper sheet is top-locked. Shift+MMB keeps its existing
                    // pan fallback; the explicit orbit tool does nothing here.
                    if self.tabs[i].orbit_mode {
                        sel.input.middle_last_pos = Some(p);
                        return Task::none();
                    }
                }
                sel.input.middle_pan_from.get_or_insert(last);
                // Drop `sel` before calling mutable scene methods.
                drop(sel);
                self.pan_active_view(i, dx, dy, move_started);
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .input
                    .middle_last_pos = Some(p);
                return Task::none();
            }
            sel.input.middle_last_pos = Some(p);
        }

        let dragging = sel.input.left_down || sel.input.right_down || sel.input.middle_down;
        let vp_size = sel.view.vp_size;
        drop(sel);

        // The active pane already follows the cursor (each pane's own
        // mouse_area focuses it via `PaneMove` → `focus_model_pane`), so
        // the camera + tile bounds used for picking below are already the
        // pane the cursor is in.

        // Tile-relative picking: shadow `p` with the cursor mapped
        // into the active Model tile and `vp_size` with the tile's
        // size, so every pick / snap / view_proj below operates in
        // the active pane. `p_full` keeps the canvas-space cursor
        // for screen overlays (cursor marker, snap glyph).
        let p_full = p;
        // Inside a floating viewport (MSPACE) the active "pane" is the
        // viewport's own screen rectangle and its own camera — the very
        // camera the GPU draws the content with. Routing picking / snap
        // / projection through it makes in-viewport editing behave like
        // the main model view (model coords, no paper round-trip) and
        // track the viewport's pan / zoom / twist exactly.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame(vp_size);
        let tile_b = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i]
                .scene
                .active_model_tile_bounds(vp_size.0, vp_size.1),
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = iced::Point {
            x: p_full.x - tile_b.x,
            y: p_full.y - tile_b.y,
        };
        let vp_size = (tile_b.width, tile_b.height);
        self.drag_table_cells(i, p, tile_b, &edit_cam);

        // ── Custom plugin-grip drag (API v11) ────────────────────────────
        // Picking is the native entity-grip path: the UCS drafting plane,
        // object/grid snap, OTRACK, ortho/polar and Shift axis lock all feed
        // the same Snapper and the visible snap_result marker. The declared
        // custom axis remains a hard constraint, including native projection
        // of an acquired object point onto that axis.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(active) = self.tabs[i].active_plugin_grip.clone() {
            let (vw, vh) = vp_size;
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };
            let grip_plane = self.tabs[i].editing_model_space().then(|| {
                let normal = self.tabs[i]
                    .active_ucs
                    .as_ref()
                    .map_or(glam::DVec3::Z, ucs_z_axis);
                (normal, active.origin_world)
            });
            let pick = |cam: &crate::scene::view::camera::Camera| match grip_plane {
                Some((normal, origin))
                    if (cam.rotation * glam::Vec3::NEG_Z)
                        .as_dvec3()
                        .dot(normal)
                        .abs()
                        > 0.05 =>
                {
                    cam.pick_on_plane(p, bounds, normal.as_vec3(), origin)
                }
                _ => cam.pick_on_target_plane(p, bounds),
            };
            let raw = match &edit_cam {
                Some(cam) => pick(cam),
                None => {
                    let paper = pick(&self.tabs[i].scene.camera.borrow());
                    self.tabs[i].scene.paper_to_model(paper)
                }
            };
            let (view_rot, eye) = match &edit_cam {
                Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                None => {
                    let cam = self.tabs[i].scene.camera.borrow();
                    (cam.view_proj_rte(bounds), cam.eye())
                }
            };
            let all_wires =
                if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                    self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(h, bounds.height)
                } else {
                    self.tabs[i].scene.hit_test_wires()
                };
            let mut snap_candidates = self.tabs[i].scene.interaction_candidates_near(
                all_wires,
                raw,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
            );
            if let Some(anchor) = active.anchor {
                // Native grips do not snap back onto the entity being edited.
                snap_candidates.exclude_wires(|wire| {
                    crate::scene::Scene::handle_from_wire_name(&wire.name) == Some(anchor)
                });
            }
            let base = active.origin_world;
            self.snapper.from_point = Some(base);
            let (go, gr) = self.drafting_grid_basis(i);
            let construction_ray =
                self.active_construction_ray(i, p, raw, base, view_rot, eye, bounds);
            self.sync_osnap_z_plane(i);
            let snap_hit = self.snapper.snap_stable(
                raw,
                p,
                &snap_candidates,
                view_rot,
                eye,
                bounds,
                go,
                gr,
                construction_ray,
                self.tabs[i].snap_result,
            );
            self.tabs[i].snap_result = snap_hit;
            let dwell_hit = snap_hit.filter(|hit| {
                matches!(
                    hit.snap_type,
                    crate::snap::SnapType::Endpoint
                        | crate::snap::SnapType::Midpoint
                        | crate::snap::SnapType::Center
                        | crate::snap::SnapType::Node
                        | crate::snap::SnapType::Quadrant
                        | crate::snap::SnapType::Intersection
                        | crate::snap::SnapType::Insertion
                        | crate::snap::SnapType::ApparentIntersection
                )
            });
            self.snapper.update_otrack_dwell(
                dwell_hit,
                &snap_candidates,
                view_rot,
                eye,
                bounds,
                Instant::now(),
            );
            let axis_lock = self.active_axis_lock(i, raw, base, true);
            let otrack_hit = if axis_lock.is_none() {
                self.active_otrack_hit(i, raw, snap_hit, Some(base), true, view_rot, eye, bounds)
            } else {
                None
            };
            let drafting_guide = construction_ray
                .and_then(|(base, target)| Some((base, (target - base).try_normalize()?)));
            self.otrack_active = otrack_hit.map(|hit| (hit.base, hit.dir)).or(drafting_guide);
            self.otrack_cross = otrack_hit.and_then(|hit| hit.cross);
            self.otrack_kind = otrack_hit.map(|hit| hit.kind);

            let mut point = if let Some(dir) = axis_lock {
                axis_lock_apply(snap_hit.map(|hit| hit.world).unwrap_or(raw), base, dir)
            } else if let Some(hit) = otrack_hit {
                hit.aligned
            } else {
                snap_hit.map(|hit| hit.world).unwrap_or(raw)
            };
            if let Some(s) = self.tabs[i].snap_result.as_mut() {
                snap_pixels_to_canvas(s, tile_b);
            }
            if axis_lock.is_none()
                && otrack_hit.is_none()
                && !snap_hit.is_some_and(|hit| {
                    !matches!(
                        hit.snap_type,
                        crate::snap::SnapType::Grid | crate::snap::SnapType::Extension
                    )
                })
            {
                if let Some((drafted, _)) =
                    self.drafting_point(i, p, point, base, view_rot, eye, bounds)
                {
                    point = drafted;
                }
            }
            if let Some(axis) = active.axis {
                let acquired_point = otrack_hit.is_some()
                    || snap_hit.is_some_and(|hit| hit.snap_type != crate::snap::SnapType::Grid);
                if acquired_point {
                    if let Some(axis) = axis.try_normalize() {
                        point = base + axis * (point - base).dot(axis);
                    }
                } else {
                    point = cursor_on_projected_axis(p, bounds, view_rot, eye, base, axis)
                        .unwrap_or(point);
                }
            }

            self.tabs[i].last_cursor_world = point;
            self.tabs[i].last_cursor_screen = p_full;
            let anchor_ndc = view_rot.project_point3((base - eye).as_vec3());
            self.tabs[i].last_point_screen = Some(Point::new(
                (anchor_ndc.x + 1.0) * 0.5 * bounds.width + tile_b.x,
                (1.0 - anchor_ndc.y) * 0.5 * bounds.height + tile_b.y,
            ));
            return self.move_plugin_grip_drag(i, point);
        }

        // ── Grip drag ─────────────────────────────────────────────
        if let Some(grip) = self.tabs[i].active_grip.clone() {
            if grip
                .targets
                .iter()
                .any(|target| self.tabs[i].scene.is_layer_locked(target.handle))
            {
                self.cancel_active_grip_edit();
                return Task::none();
            }
            // Remove Vertex / Convert to Line wait for the next click; the
            // cursor has nothing to place in the meantime.
            if self.grip_ctrl_one_shot(i).is_some() {
                self.tabs[i].last_cursor_screen = p_full;
                return Task::none();
            }
            let grip_started = Instant::now();
            let (vw, vh) = vp_size;
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };
            // In a viewport, pick the cursor's model point with the
            // viewport camera directly; otherwise project on the paper
            // sheet and map to model. Either way `raw` is model space.
            //
            // A model-space grip moves in the drawing plane through where it
            // started (the UCS XY plane at the grip's own elevation). The
            // plane facing the screen through the orbit target put the point
            // somewhere else in depth in any tilted view, so a circle jumped
            // to another radius and a vertex left its plane (#1033). Seen
            // edge-on that plane cannot be hit; the screen plane takes over.
            let grip_plane = self.tabs[i].editing_model_space().then(|| {
                let normal = self.tabs[i]
                    .active_ucs
                    .as_ref()
                    .map_or(glam::DVec3::Z, ucs_z_axis);
                (normal, grip.origin_world)
            });
            let pick = |cam: &crate::scene::view::camera::Camera| match grip_plane {
                Some((normal, origin))
                    if (cam.rotation * glam::Vec3::NEG_Z)
                        .as_dvec3()
                        .dot(normal)
                        .abs()
                        > 0.05 =>
                {
                    cam.pick_on_plane(p, bounds, normal.as_vec3(), origin)
                }
                _ => cam.pick_on_target_plane(p, bounds),
            };
            let raw = match &edit_cam {
                Some(cam) => pick(cam),
                None => {
                    let paper = pick(&self.tabs[i].scene.camera.borrow());
                    self.tabs[i].scene.paper_to_model(paper)
                }
            };
            let (view_rot, eye) = match &edit_cam {
                Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                None => {
                    let cam = self.tabs[i].scene.camera.borrow();
                    (cam.view_proj_rte(bounds), cam.eye())
                }
            };

            let mut seen_handles = rustc_hash::FxHashSet::default();
            let edited_handles: Vec<_> = grip
                .targets
                .iter()
                .map(|target| target.handle)
                .filter(|handle| seen_handles.insert(*handle))
                .collect();

            // Initialize once: constraint solves may add neighbors to this gesture.
            // Wire entities use the overlay; solid meshes stay visible and move live.
            if self.grip_preview_handles.is_empty() {
                let init_started = crate::perf::enabled().then(Instant::now);
                let init_mark = |stage: &str| {
                    if let Some(started) = init_started {
                        crate::perf_record!(
                            "[perf] grip-init {stage}={:.1}ms",
                            started.elapsed().as_secs_f64() * 1000.0
                        );
                    }
                };
                let preview_handles = self.tabs[i].scene.parametric_connected_handles(
                    self.tabs[i].current_parametric_scope(),
                    &edited_handles,
                    true,
                );
                init_mark("connected-handles");
                if self.grip_dirty_before.is_none() {
                    self.grip_dirty_before = Some(self.tabs[i].dirty);
                }
                // Interactive Add Vertex seeds this with the entity from
                // before insertion so append + placement is one undo step.
                // Normal grip drags still snapshot their current entities here.
                if self.grip_originals.is_empty() {
                    self.grip_originals = preview_handles
                        .iter()
                        .filter_map(|&handle| {
                            self.tabs[i]
                                .scene
                                .document
                                .get_entity(handle)
                                .cloned()
                                .map(|entity| (handle, entity))
                        })
                        .collect();
                }
                self.capture_grip_history_originals(i, &edited_handles);
                init_mark("history-originals");
                for &handle in &preview_handles {
                    if !self.tabs[i].scene.meshes.contains_key(&handle) {
                        self.tabs[i].scene.preview_hidden.insert(handle);
                    }
                }
                let changes: Vec<_> = preview_handles
                    .iter()
                    .map(|&handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i]
                    .scene
                    .bump_entities_after_parametric_solve(&changes);
                init_mark("bump-after-solve");
                self.grip_preview_handles = preview_handles;
                // Snapshot the entity's glyph quads once so each move can
                // slide the already-shaped text rather than re-shaping it
                // (issue #316). The fast slide path only runs for a rigid
                // whole-entity move of pure text: no wire geometry (so a
                // dimension re-tessellates) and a Square insertion grip (so
                // an MTEXT width handle, a Triangle, still re-tessellates so
                // the re-wrap is exact).
                let snap = self.tabs[i]
                    .scene
                    .wire_models_for(&self.grip_preview_handles);
                init_mark("wire-models-for");

                // Preserve the exact geometry from the instant the grip drag began.
                // This snapshot is visual/reference-only: do NOT append it to
                // `snap_candidates`, otherwise phantom self-snapping can return.
                self.grip_reference_wires = snap.clone();

                // The engaged grip is always an intentional OTRACK reference.
                self.snapper
                    .acquire_grip_tracking_point(grip.origin_world, &self.grip_reference_wires);

                self.grip_text_verts = snap
                    .iter()
                    .flat_map(|w| w.text_verts.iter().copied())
                    .collect();
                let square_grip = self.tabs[i]
                    .selected_grip_handles
                    .iter()
                    .copied()
                    .zip(self.tabs[i].selected_grips.iter())
                    .find(|(owner, grip_def)| *owner == grip.handle && grip_def.id == grip.grip_id)
                    .map(|(_, grip_def)| {
                        grip_def.shape == crate::scene::model::object::GripShape::Square
                    })
                    .unwrap_or(false);
                self.grip_text_slide = self.grip_preview_handles.len() == 1
                    && grip.targets.len() == 1
                    && !self.grip_text_verts.is_empty()
                    && snap.iter().all(|w| w.points.is_empty())
                    && square_grip
                    // An INSERT can contain several text attributes. A grip
                    // moves only one of them, so its glyphs need live tessellation.
                    && !self.grip_originals.iter().any(|(handle, entity)| {
                        *handle == grip.handle
                            && matches!(entity, document::EntityType::Insert(_))
                    });
            }
            let setup_ms = grip_started.elapsed().as_secs_f64() * 1000.0;
            let snap_started = Instant::now();

            // The edited entity is hidden, so it's already absent from
            // `hit_test_wires` — snap against the set directly, no clone
            // and no self-snap.
            let all_wires =
                if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                    self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(h, bounds.height)
                } else {
                    self.tabs[i].scene.hit_test_wires()
                };
            let mut snap_candidates = self.tabs[i].scene.interaction_candidates_near(
                all_wires,
                raw,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
            );
            // A solid stays drawn while its grip moves, so its own edges are
            // still in the set: the grip would snap back onto the shape it is
            // reshaping (a cone's base radius landed on its own side, #1033).
            let edited = &self.grip_preview_handles;
            if !edited.is_empty() {
                snap_candidates.exclude_wires(|wire| {
                    crate::scene::Scene::handle_from_wire_name(&wire.name)
                        .is_some_and(|handle| edited.contains(&handle))
                });
            }
            // The engaged grip is the rubber-band origin. Perpendicular
            // snapping must drop its foot from this point, including when a
            // hot-grip set is moved by the same drag vector.
            self.snapper.from_point = Some(grip.origin_world);
            let (go, gr) = self.drafting_grid_basis(i);
            let base = grip.origin_world;
            let construction_ray =
                self.active_construction_ray(i, p, raw, base, view_rot, eye, bounds);
            // `raw` is already model space (viewport camera or paper→model),
            // and the wires are model space, so the snap result is model.
            // Normal snap: only the rest of the drawing participates here.
            // The frozen grip-reference geometry deliberately stays out, so it cannot
            // pull the edited point back onto its old shape.
            self.sync_osnap_z_plane(i);
            let snap_hit = self.snapper.snap_stable(
                raw,
                p,
                &snap_candidates,
                view_rot,
                eye,
                bounds,
                go,
                gr,
                construction_ray,
                self.tabs[i].snap_result,
            );

            // The frozen pre-drag geometry gets a SECOND snap pass, limited to
            // its discrete points: a corner can be dropped on the old midpoint
            // of its own edge, or acquired for OTRACK, while nearest/tangent-
            // style snaps can never pull the point back onto the old shape.
            // The grip's own starting point is skipped so it does not stick
            // where the drag began.
            let reference_snap_hit = if !self.grip_reference_wires.is_empty() {
                self.snapper
                    .snap(
                        raw,
                        p,
                        &self.grip_reference_wires,
                        view_rot,
                        eye,
                        bounds,
                        go,
                        gr,
                        None,
                    )
                    .filter(|hit| {
                        matches!(
                            hit.snap_type,
                            crate::snap::SnapType::Endpoint
                                | crate::snap::SnapType::Midpoint
                                | crate::snap::SnapType::Center
                                | crate::snap::SnapType::Node
                                | crate::snap::SnapType::Quadrant
                                | crate::snap::SnapType::Intersection
                                | crate::snap::SnapType::Insertion
                                | crate::snap::SnapType::ApparentIntersection
                        ) && hit.world.distance(grip.origin_world) > 1e-9
                    })
            } else {
                None
            };

            // A real object snap on the rest of the drawing wins; otherwise the
            // reference point both shows and places the grip, so the marker is
            // always the point the grip lands on.
            let snap_hit = match snap_hit {
                Some(hit) if hit.snap_type != crate::snap::SnapType::Grid => Some(hit),
                _ => reference_snap_hit.or(snap_hit),
            };

            self.tabs[i].snap_result = snap_hit;

            // OTRACK acquisition needs access to the original wire geometry so, once a
            // reference point has dwelt long enough, it can capture the segment directions
            // meeting at that point.
            //
            // IMPORTANT: this combined set is used ONLY for OTRACK acquisition.
            // It is never passed to the normal movement snap above.
            // Zero-copy: chains the near-cursor candidates with the frozen
            // reference wires instead of deep-cloning both every pointer move.
            let tracking_candidates =
                ChainedWireSource::new(&snap_candidates, &self.grip_reference_wires);

            // Prefer a genuine drawing snap when it is an acquisition-capable point.
            // Otherwise let the frozen-reference snap drive the dwell acquisition.
            let normal_tracking_hit = snap_hit.filter(|hit| {
                matches!(
                    hit.snap_type,
                    crate::snap::SnapType::Endpoint
                        | crate::snap::SnapType::Midpoint
                        | crate::snap::SnapType::Center
                        | crate::snap::SnapType::Node
                        | crate::snap::SnapType::Quadrant
                        | crate::snap::SnapType::Intersection
                        | crate::snap::SnapType::Insertion
                        | crate::snap::SnapType::ApparentIntersection
                )
            });

            let dwell_hit = normal_tracking_hit.or(reference_snap_hit);

            let tracking_started = perf_move.then(Instant::now);
            self.snapper.update_otrack_dwell(
                dwell_hit,
                &tracking_candidates,
                view_rot,
                eye,
                bounds,
                Instant::now(),
            );
            let axis_lock = self.active_axis_lock(i, raw, base, true);
            let otrack_hit = if axis_lock.is_none() {
                self.active_otrack_hit(i, raw, snap_hit, Some(base), true, view_rot, eye, bounds)
            } else {
                None
            };
            let tracking_ms = crate::perf::elapsed_ms(tracking_started);
            if perf_move && tracking_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] grip-snap-tracking {tracking_ms:.2}ms candidates={}",
                    snap_candidates.len()
                );
            }
            // If OTRACK has no acquired-object hit, expose the current polar/ortho
            // construction ray through the same visual guide.
            let drafting_guide = construction_ray.and_then(|(base, target)| {
                let dir = (target - base).try_normalize()?;
                Some((base, dir))
            });
            self.otrack_active = otrack_hit.map(|hit| (hit.base, hit.dir)).or(drafting_guide);
            self.otrack_cross = otrack_hit.and_then(|hit| hit.cross);

            self.otrack_kind = otrack_hit.map(|hit| hit.kind);

            let mut snapped = if let Some(dir) = axis_lock {
                axis_lock_apply(snap_hit.map(|hit| hit.world).unwrap_or(raw), base, dir)
            } else if let Some(hit) = otrack_hit {
                hit.aligned
            } else {
                snap_hit.map(|s| s.world).unwrap_or(raw)
            };
            if let Some(s) = self.tabs[i].snap_result.as_mut() {
                snap_pixels_to_canvas(s, tile_b);
            }

            if axis_lock.is_none()
                && otrack_hit.is_none()
                && !snap_hit.is_some_and(|s| {
                    !matches!(
                        s.snap_type,
                        crate::snap::SnapType::Grid | crate::snap::SnapType::Extension
                    )
                })
            {
                let base = grip.origin_world;
                if let Some((point, _)) =
                    self.drafting_point(i, p, snapped, base, view_rot, eye, bounds)
                {
                    snapped = point;
                }
            }

            // A table fill drag reaches the cell under the cursor: object
            // snaps on the table's own lines, tracking and polar would pull it
            // to an edge or a far corner.
            if table_fill_follows_cursor(&self.grip_originals, grip.handle, grip.grip_id) {
                snapped = raw;
                self.tabs[i].snap_result = None;
                self.otrack_active = None;
                self.otrack_cross = None;
                self.otrack_kind = None;
            }

            if let Some(axis) = grip.axis {
                let acquired_point = otrack_hit.is_some()
                    || snap_hit.is_some_and(|hit| hit.snap_type != crate::snap::SnapType::Grid);
                if acquired_point {
                    if let Some(axis) = axis.try_normalize() {
                        // An axis grip still moves on its construction axis, but
                        // an acquired object point supplies the exact coordinate
                        // along that axis. This lets height and draft arrows seat
                        // back onto existing surface/solid geometry.
                        snapped =
                            grip.origin_world + axis * (snapped - grip.origin_world).dot(axis);
                    }
                } else {
                    snapped =
                        cursor_on_projected_axis(p, bounds, view_rot, eye, grip.origin_world, axis)
                            .unwrap_or(snapped);
                }
            }

            if let Some(normal) = grip.plane {
                if snap_hit.is_none_or(|hit| hit.snap_type == crate::snap::SnapType::Grid) {
                    snapped = cursor_on_plane(p, bounds, view_rot, eye, grip.origin_world, normal)
                        .unwrap_or(snapped);
                }
            }
            if grip.gizmo {
                // The cloud follows the cursor by the offset it was grabbed
                // at (measured on the press; the first move as a fallback).
                let grab = match grip.grab {
                    Some(grab) => grab,
                    None => {
                        let grab = snapped - grip.origin_world;
                        if let Some(active) = self.tabs[i].active_grip.as_mut() {
                            active.grab = Some(grab);
                        }
                        grab
                    }
                };
                snapped -= grab;
            }

            let snap_ms = snap_started.elapsed().as_secs_f64() * 1000.0;
            // The overlay builds the active tracking guide from `otrack_active.base`
            // to `last_cursor_world`. Keep it synchronized with the actual point used
            // by the grip, otherwise the guide and the edited geometry diverge.
            self.tabs[i].last_cursor_world = snapped;
            // A typed (Tab-locked) Distance or Angle pins the stretch, as it
            // pins a command's preview (#356): the preview shows what Enter
            // commits.
            if grip.mode == GripEditMode::Stretch
                && grip.rectangle_frame.is_none()
                && self.tabs[i].dyn_fields.iter().any(|f| f.buffer.is_some())
            {
                if let Some(point) = self.dyn_resolve_point() {
                    snapped = point;
                    self.tabs[i].last_cursor_world = snapped;
                }
            }
            self.tabs[i].last_cursor_screen = p_full;

            // Project the grip's original position into full-canvas coordinates.
            // Dynamic Input uses this as the polar Distance/Angle anchor.
            let anchor_ndc = view_rot.project_point3((grip.origin_world - eye).as_vec3());

            self.tabs[i].last_point_screen = Some(Point::new(
                (anchor_ndc.x + 1.0) * 0.5 * bounds.width + tile_b.x,
                (1.0 - anchor_ndc.y) * 0.5 * bounds.height + tile_b.y,
            ));

            let apply_started = Instant::now();
            if let Some((opposite, width_axis, height_axis)) = grip.rectangle_frame {
                let delta = snapped - opposite;
                let mut width = delta.dot(width_axis);
                let mut height = delta.dot(height_axis);
                for field in &self.tabs[i].dyn_fields {
                    let Some(buffer) = field.buffer.as_ref() else {
                        continue;
                    };
                    let Some(value) = crate::app::expr_eval::eval_number(buffer) else {
                        continue;
                    };
                    match field.role {
                        crate::command::DynRole::Width => {
                            width = value.abs().copysign(width);
                        }
                        crate::command::DynRole::Height => {
                            height = value.abs().copysign(height);
                        }
                        _ => {}
                    }
                }
                snapped = opposite + width_axis * width + height_axis * height;
                self.tabs[i].last_cursor_world = snapped;
            }
            let delta = snapped - grip.last_world;
            // A table grip edits the table as it was when the drag started,
            // at the cursor: a fill, a break or a column line need not follow
            // the cursor, so the drag is not rebased onto them. A viewport
            // corner too: dragged past the opposite edge, the rebased frame
            // would take the cursor as its fixed corner and fold flat (#3327).
            let table_targets: rustc_hash::FxHashSet<Handle> = self
                .grip_originals
                .iter()
                .filter(|(_, entity)| {
                    matches!(
                        entity,
                        document::EntityType::Table(_) | document::EntityType::Viewport(_)
                    )
                })
                .map(|(handle, _)| *handle)
                .collect();
            let menu_action = match grip.mode {
                GripEditMode::Lengthen => {
                    Some(crate::scene::model::object::GripMenuAction::Lengthen)
                }
                GripEditMode::Radius => Some(crate::scene::model::object::GripMenuAction::Radius),
                GripEditMode::ArcLength => {
                    Some(crate::scene::model::object::GripMenuAction::ArcLength)
                }
                GripEditMode::RectangleWidth => {
                    Some(crate::scene::model::object::GripMenuAction::RectangleWidth)
                }
                GripEditMode::RectangleHeight => {
                    Some(crate::scene::model::object::GripMenuAction::RectangleHeight)
                }
                GripEditMode::MoveParallel => {
                    Some(crate::scene::model::object::GripMenuAction::MoveParallel)
                }
                GripEditMode::RectangleResize => None,
                GripEditMode::Stretch => None,
            };
            let actions: Vec<_> = if matches!(grip.mode, GripEditMode::RectangleResize) {
                let Some((opposite, width_axis, height_axis)) = grip.rectangle_frame else {
                    return Task::none();
                };
                let opposite_id = (grip.grip_id + 2) % 4;
                let adjacent_ids = [(opposite_id + 1) % 4, (opposite_id + 3) % 4];
                let mut edits = Vec::with_capacity(3);
                for adjacent_id in adjacent_ids {
                    let original = self.tabs[i]
                        .selected_grip_handles
                        .iter()
                        .zip(self.tabs[i].selected_grips.iter())
                        .find(|(owner, candidate)| {
                            **owner == grip.handle && candidate.id == adjacent_id
                        })
                        .map(|(_, candidate)| candidate.world);
                    if let Some(original) = original {
                        let original_delta = original - opposite;
                        let axis = if original_delta.dot(width_axis).abs()
                            >= original_delta.dot(height_axis).abs()
                        {
                            width_axis
                        } else {
                            height_axis
                        };
                        let point = opposite + axis * (snapped - opposite).dot(axis);
                        edits.push((grip.handle, adjacent_id, GripApply::Absolute(point)));
                    }
                }
                edits.push((grip.handle, grip.grip_id, GripApply::Absolute(snapped)));
                edits
            } else if menu_action.is_some() {
                Vec::new()
            } else {
                grip.targets
                    .iter()
                    .map(|target| {
                        let apply =
                            if target.is_translate && !table_targets.contains(&target.handle) {
                                GripApply::Translate(delta)
                            } else {
                                GripApply::Absolute(target.last_world + delta)
                            };
                        (target.handle, target.grip_id, apply)
                    })
                    .collect()
            };
            if let Some(action) = menu_action {
                let original = self
                    .grip_originals
                    .iter()
                    .find(|(handle, _)| *handle == grip.handle)
                    .map(|(_, entity)| entity.clone());
                if let Some(original) = original {
                    let value = crate::scene::view::dispatch::grip_menu_point_value(
                        &original,
                        grip.grip_id,
                        action,
                        snapped,
                    );
                    if let Some(value) = value {
                        let origin_value = crate::scene::view::dispatch::grip_menu_point_value(
                            &original,
                            grip.grip_id,
                            action,
                            grip.origin_world,
                        );
                        let mut candidate = original.clone();
                        crate::entities::traits::EntityTypeOps::apply_grip_menu_value(
                            &mut candidate,
                            grip.grip_id,
                            action,
                            value,
                        );
                        let returns_to_origin = origin_value.is_some_and(|origin_value| {
                            (value - origin_value).abs()
                                <= 1.0e-9 * value.abs().max(origin_value.abs()).max(1.0)
                        });
                        if let Some(current) =
                            self.tabs[i].scene.document.get_entity_mut(grip.handle)
                        {
                            // Some radius/parallel positions have no geometric
                            // solution. Their entity edit is deliberately a
                            // no-op; retain the last valid preview instead of
                            // flashing back to the drag-start geometry. The
                            // exact origin remains reachable when the cursor
                            // moves back there.
                            if candidate != original || returns_to_origin {
                                *current = candidate;
                            }
                        }
                    }
                }
            }
            // Arc point grips are coupled: moving one point must rebuild the
            // circle from the drag-start start/middle/end set, otherwise the
            // supposedly fixed points drift a little on every mouse event.
            let mut arc_grip_edits: rustc_hash::FxHashMap<Handle, Vec<(usize, glam::DVec3)>> =
                rustc_hash::FxHashMap::default();
            for (handle, grip_id, apply) in &actions {
                if let GripApply::Absolute(point) = apply {
                    if (1..=3).contains(grip_id) {
                        arc_grip_edits
                            .entry(*handle)
                            .or_default()
                            .push((*grip_id, *point));
                    }
                }
            }
            let mut rebuilt_arcs = rustc_hash::FxHashSet::default();
            for (handle, edits) in arc_grip_edits {
                let original = self
                    .grip_originals
                    .iter()
                    .find(|(original_handle, _)| *original_handle == handle)
                    .map(|(_, entity)| entity.clone());
                let Some(original) = original else {
                    continue;
                };
                let Some(current) = self.tabs[i].scene.document.get_entity_mut(handle) else {
                    continue;
                };
                if crate::scene::view::dispatch::refit_arc_grips(current, &original, &edits)
                    .is_some()
                {
                    rebuilt_arcs.insert(handle);
                }
            }
            let refit_arc_targets: Vec<_> = grip
                .targets
                .iter()
                .filter_map(|target| {
                    let original = self
                        .grip_originals
                        .iter()
                        .find(|(handle, _)| *handle == target.handle)
                        .map(|(_, entity)| entity)?;
                    let current = self.tabs[i].scene.document.get_entity(target.handle)?;
                    added_arc_bulge(original, current, target.grip_id)
                        .map(|bulge| (target.handle, target.grip_id, bulge))
                })
                .collect();
            for &handle in &table_targets {
                let original = self
                    .grip_originals
                    .iter()
                    .find(|(original_handle, _)| *original_handle == handle)
                    .map(|(_, entity)| entity.clone());
                if let (Some(original), Some(current)) =
                    (original, self.tabs[i].scene.document.get_entity_mut(handle))
                {
                    *current = original;
                }
            }
            self.tabs[i].scene.table_fill_drag = None;
            for (handle, grip_id, apply) in actions {
                let fill_point = match &apply {
                    GripApply::Absolute(point) => Some(*point),
                    _ => None,
                };
                if !rebuilt_arcs.contains(&handle) {
                    self.tabs[i].scene.apply_grip(handle, grip_id, apply);
                }
                // A fill shows the cells it reaches, filled or empty.
                if let (true, Some(point), Some(range)) = (
                    grip_id == crate::entities::table::CELL_GRIP_FILL
                        && table_targets.contains(&handle),
                    fill_point,
                    crate::entities::table::prop_cell_range(),
                ) {
                    if let Some((_, document::EntityType::Table(original))) = self
                        .grip_originals
                        .iter()
                        .find(|(original_handle, _)| *original_handle == handle)
                    {
                        let (r0, c0, r1, c1) =
                            crate::entities::table::fill_span(original, range, point);
                        self.tabs[i]
                            .scene
                            .select_table_cells(crate::scene::TableCellSelection {
                                table: handle,
                                anchor: (r0, c0),
                                end: (r1, c1),
                            });
                        self.tabs[i].scene.table_fill_drag = Some(handle);
                    }
                }
            }
            for (handle, vertex_id, original_bulge) in refit_arc_targets {
                if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                    crate::entities::lwpolyline::refit_added_arc_vertex(
                        entity,
                        vertex_id,
                        original_bulge,
                    );
                }
            }
            self.solve_grip_constraints(i, &grip);
            let mesh_changes: Vec<_> = self
                .grip_preview_handles
                .iter()
                .copied()
                .filter(|handle| self.tabs[i].scene.meshes.contains_key(handle))
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            if !mesh_changes.is_empty() {
                self.tabs[i]
                    .scene
                    .bump_entities_after_parametric_solve(&mesh_changes);
            }
            let hatch_preview_started = crate::perf::enabled().then(Instant::now);
            self.tabs[i]
                .scene
                .set_preview_hatches(&self.grip_preview_handles);
            let hatch_preview_ms = crate::perf::elapsed_ms(hatch_preview_started);
            if hatch_preview_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] grip-hatch-preview build={hatch_preview_ms:.1}ms handles={} models={}",
                    self.grip_preview_handles.len(),
                    self.tabs[i].scene.preview_hatches.len(),
                );
            }
            self.tabs[i].dirty = true;
            if let Some(active) = self.tabs[i].active_grip.as_mut() {
                active.last_world = snapped;
                for target in &mut active.targets {
                    target.last_world += delta;
                }
            }
            let apply_ms = apply_started.elapsed().as_secs_f64() * 1000.0;
            let preview_started = Instant::now();
            // Overlay the moved entity (hidden from the base). Pure text
            // moved as a whole slides its drag-start glyphs — no re-shaping
            // and no wire re-tess. Anything else (wire geometry, or a point
            // grip that reshapes text) is re-tessellated for an exact
            // preview; either way the glyphs ride the preview-text buffer
            // so dragged text never vanishes mid-drag (issue #316).
            if self.grip_text_slide {
                let d = snapped - grip.origin_world;
                let slid = crate::scene::pipeline::text_gpu::translate_verts(
                    &self.grip_text_verts,
                    [d.x, d.y, d.z],
                );
                self.tabs[i].scene.set_preview_text(slid);
                self.tabs[i].scene.set_preview_wires(Vec::new());
            } else {
                // Current deformed geometry.
                let mut preview = self.tabs[i]
                    .scene
                    .grip_wire_models_for(&self.grip_preview_handles);

                // Also show the drag-start geometry as a faint ghost.
                //
                // This is display-only. Preview wires never participate in the resident
                // hit-test/snap set, so keeping the original shape visible does not
                // reintroduce self-snapping.
                let keep_reference_overlay = edited_handles
                    .iter()
                    .any(|handle| self.tabs[i].scene.preview_hidden.contains(handle));
                let mut reference = if keep_reference_overlay {
                    self.grip_reference_wires.clone()
                } else {
                    Vec::new()
                };

                for wire in &mut reference {
                    wire.selected = false;

                    // Preserve the entity colour but strongly fade it.
                    wire.color[3] *= 0.48;

                    // The original is a positional reference, not another selected object.
                    wire.line_weight_px = wire.line_weight_px.min(1.0);
                }

                // Original first, live/deformed geometry over it.
                reference.append(&mut preview);

                self.tabs[i].scene.set_preview_wires(reference);
            }
            let preview_ms = preview_started.elapsed().as_secs_f64() * 1000.0;
            let geometry_ms = grip_started.elapsed().as_secs_f64() * 1000.0;
            self.refresh_selected_grips();
            // A history rebuild can clamp a requested value or move a derived
            // construction handle non-linearly. Rebase each active target onto
            // the freshly rebuilt grip so the hot arrow remains attached to the
            // live surface and reverses immediately from a geometric limit.
            // Only history-driven targets: a plain curve grip need not sit at
            // the cursor (a circle quadrant is rebuilt at the new radius on its
            // own axis), and rebasing it would drop the cursor offset (#3187).
            let document = &self.tabs[i].scene.document;
            let refreshed_targets: Vec<_> = self.tabs[i]
                .selected_grip_handles
                .iter()
                .copied()
                .zip(self.tabs[i].selected_grips.iter())
                .filter(|(handle, _)| {
                    crate::scene::model::solid_history::edit_operand(document, *handle).is_some()
                        || document.solid_history_operation(*handle).is_some()
                })
                .map(|(handle, grip)| ((handle, grip.id), grip.world))
                .collect();
            if let Some(active) = self.tabs[i].active_grip.as_mut() {
                for target in &mut active.targets {
                    if table_targets.contains(&target.handle) {
                        continue;
                    }
                    if let Some((_, world)) = refreshed_targets
                        .iter()
                        .find(|((handle, id), _)| *handle == target.handle && *id == target.grip_id)
                    {
                        target.last_world = *world;
                    }
                }
            }
            let grips_ms = grip_started.elapsed().as_secs_f64() * 1000.0 - geometry_ms;
            // Properties are refreshed when the grip is committed or cancelled.
            // Rebuilding the inspector on every pointer event adds no drawing
            // value and can monopolize the UI thread during a drag.
            let total_ms = grip_started.elapsed().as_secs_f64() * 1000.0;
            if perf_move && total_ms >= 50.0 {
                crate::perf_record!(
                    "[perf] grip-move          {:>7.1}ms setup={:.1} snap={:.1} apply={:.1} preview={:.1} grips={:.1}",
                    total_ms,
                    setup_ms,
                    snap_ms,
                    apply_ms,
                    preview_ms,
                    grips_ms,
                );
            }
            return Task::none();
        }

        // Keep the coordinate readout live on every move, even with no
        // active command. When a command is running the snap path below
        // overwrites this with the snapped point.
        {
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vp_size.0,
                height: vp_size.1,
            };
            let world = self.cursor_model_point(i, &edit_cam, p, bounds);
            self.tabs[i].last_cursor_world = world;
        }

        // A pending client pick (`getpoint` or `user_select`) is a command-grade
        // pick without a command: run the object-snap engine here too, so the
        // snap marker shows while the person aims. `getpoint` answers with the
        // snapped point (`on_viewport_left_press` resolves from
        // `last_cursor_world`); for `user_select` the marker is aiming feedback
        // — the click still selects the entity under the cursor. Inputs mirror
        // the command snap path below; OTRACK/polar/axis locks stay command-only.
        if self.tabs[i].active_cmd.is_none() && self.client_pick_pending(i) {
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vp_size.0,
                height: vp_size.1,
            };
            let cursor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
            let (gx, gy, adaptive) = (self.grid_spacing_x, self.grid_spacing_y, self.grid_adaptive);
            let visible_step = |distance: f32, fov_y: f32| {
                let (sx, sy) = crate::ui::overlay::compute_grid_steps(
                    gx, gy, distance, fov_y, bounds, adaptive,
                );
                sx.max(sy)
            };
            let (view_rot, eye, grid_spacing) = match &edit_cam {
                Some(cam) => (
                    cam.view_proj_rte(bounds),
                    cam.eye(),
                    visible_step(cam.distance, cam.fov_y),
                ),
                None => {
                    let cam = self.tabs[i].scene.camera.borrow();
                    (
                        cam.view_proj_rte(bounds),
                        cam.eye(),
                        visible_step(cam.distance, cam.fov_y),
                    )
                }
            };
            self.snapper.grid_spacing = grid_spacing;
            let all_wires =
                if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                    self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(h, bounds.height)
                } else {
                    self.tabs[i].scene.hit_test_wires()
                };
            let snap_candidates = self.tabs[i].scene.interaction_candidates_near(
                all_wires,
                cursor_world,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
            );
            let (go, gr) = self.drafting_grid_basis(i);
            self.sync_osnap_z_plane(i);
            let snap_hit = self.snapper.snap_stable(
                cursor_world,
                p,
                &snap_candidates,
                view_rot,
                eye,
                bounds,
                go,
                gr,
                None,
                self.tabs[i].snap_result,
            );
            self.tabs[i].snap_result = snap_hit;
            self.tabs[i].last_cursor_world = snap_hit.map(|hit| hit.world).unwrap_or(cursor_world);
        }

        // Rollover highlight: when idle (no active command, no
        // drag), defer the pick until the cursor stops. The full
        // pick (wires + hatches + block hatches + shaded meshes) is
        // O(N) per frame and stalls the cursor on large drawings,
        // so each move resets the dwell timer — `HoverDwellTick` runs the hit-test only
        // once the cursor has been still for `HOVER_DWELL_MS`.
        let is_gathering_cmd = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.is_selection_gathering());
        let deferred_command_hover = self.tabs[i].active_cmd.as_ref().is_some_and(|command| {
            command.needs_entity_pick() && command.entity_pick_deferred_hover()
        });
        if !dragging
            && (self.tabs[i].active_cmd.is_none() || deferred_command_hover || is_gathering_cmd)
        {
            // Keep the previous highlight: the throttled moving pick
            // (`hover_pick_due`) replaces it while the cursor travels.
            // A cursor move is the strongest "the pick's inputs changed"
            // signal for the quad dwell poll: bump the move count first so
            // the recorded (or pending) settled pick is disqualified.
            self.hover_move_seq += 1;
            self.hover_dwell = Some(crate::app::HoverDwell {
                last_move_at: Instant::now(),
                point: p,
                tile_size: vp_size,
                tab: i,
                camera_generation: self.tabs[i].scene.camera_generation,
            });
        } else {
            // Suppress the rollover during a command or a drag.
            self.tabs[i].scene.set_hover_highlight(None);
            self.hover_dwell = None;
        }
        // The quad cursor menu's hover dwell (docs/quad-menu.md §3.4) rides
        // the same settled-rollover machinery: a move restarts nothing on its
        // own clock (the set-hover step sees the live rollover highlight), a
        // quad already open closes once the cursor drifts or leaves the
        // entity. `rest_ms = 0`: the cursor only just moved.
        self.quad_hover_step(i, p, 0);

        // Throttle: while a plain point pick is active and the cursor is still
        // settling, defer continuous / pairwise snap modes to SnapDwellTick.
        // Keep discrete feature snaps (Endpoint, Node, Midpoint, etc.) live so
        // the marker and rubber-band can still attach to geometry while moving.
        // Entity/structure/tangent picks, selection windows, grid snap,
        // active tracking/parallel guides, explicit angle overrides, active
        // Shift axis locks, and custom command cursor axes take the full pass
        // below to preserve their authoritative constraint resolution. Ortho
        // and Polar constraints take the fast path when none of those special
        // constraints are active.
        let fast_point = (self.tabs[i].scene.current_layout == "Model" || edit_cam.is_some())
            && self.tabs[i].active_cmd.as_ref().is_some_and(|command| {
                !command.needs_entity_pick()
                    && !command.needs_structure_point_pick()
                    && !command.needs_tangent_pick()
                    && !command.is_selection_gathering()
                    && !command.window_corner_pick()
                    && !command.wants_raw_point()
                    && command.cursor_axis().is_none()
            })
            && !self.shift_down
            && self.angle_override.is_none()
            && self.axis_lock_dir.is_none()
            && !(self.snapper.grid_snap()
                || !self.snapper.tracking_points.is_empty()
                || self.snapper.parallel_ref.is_some()
                || (self.snapper.snap_enabled
                    && self.snapper.is_on(crate::snap::SnapType::Parallel)));
        if fast_point && self.snap_dwell.is_some() {
            let throttle_started = crate::perf::enabled().then(Instant::now);
            let (vw, vh) = vp_size;
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };
            let cursor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
            let (view_rot, eye) = match &edit_cam {
                Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                None => {
                    let camera = self.tabs[i].scene.camera.borrow();
                    (camera.view_proj_rte(bounds), camera.eye())
                }
            };
            // Large drawings can spend more time finding discrete snap
            // candidates than updating the cursor. Keep the crosshair and
            // rubber-band current for every event, but cap this expensive
            // preview pass at 60 Hz. Click and settled-dwell paths still run
            // the authoritative full snap at the exact pointer position.
            let discrete_preview_enabled = self.snapper.has_discrete_preview_enabled();
            let run_discrete_preview = discrete_preview_enabled
                && self
                    .snap_dwell
                    .as_ref()
                    .is_some_and(|dwell| dwell.discrete_preview_due_at(Instant::now()));
            // A command that hides its resident sources (STRETCH) must keep
            // its edited geometry updating during the lightweight mouse path,
            // unlike a point-only rubber band. Cap it to the same 60 Hz
            // preview budget rather than running full snapping each event.
            let refresh_stretch_preview = self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|cmd| !cmd.preview_hidden_handles().is_empty())
                && (run_discrete_preview
                    || self
                        .snap_dwell
                        .as_ref()
                        .is_some_and(|dwell| dwell.discrete_preview_due_at(Instant::now())));
            // F3 off (or only continuous modes configured) means this live
            // pass cannot produce a snap. Avoid even querying the large scene
            // index; the full settled/click pass remains authoritative.
            let snap = if run_discrete_preview {
                let all_wires = if let (Some(_), Some(handle)) =
                    (&edit_cam, self.tabs[i].scene.active_viewport)
                {
                    self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(handle, bounds.height)
                } else {
                    self.tabs[i].scene.hit_test_wires()
                };
                let candidate_started = crate::perf::enabled().then(Instant::now);
                let snap_candidates = self.tabs[i].scene.interaction_snap_preview_candidates_near(
                    all_wires,
                    cursor_world,
                    view_rot,
                    eye,
                    bounds,
                    self.snapper.osnap_radius_px,
                );
                let candidate_ms = crate::perf::elapsed_ms(candidate_started);
                let evaluation_started = crate::perf::enabled().then(Instant::now);
                let snap = self.snapper.snap_discrete_preview_stable(
                    p,
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                    self.tabs[i].snap_result,
                );
                // Feed live point candidates into the OTRACK acquisition
                // timer even while the expensive full snap pass is deferred.
                // Previously the 250ms clock often started only after the
                // stationary dwell tick, so a truly still cursor could not
                // acquire a point without extra mouse movements.
                if self.snapper.tracking_active() {
                    self.snapper.update_otrack_dwell(
                        snap,
                        &snap_candidates,
                        view_rot,
                        eye,
                        bounds,
                        Instant::now(),
                    );
                }
                if crate::perf::enabled() {
                    let evaluation_ms = crate::perf::elapsed_ms(evaluation_started);
                    let feature_count = snap_candidates
                        .snap_points()
                        .map_or(0, |items| items.len())
                        .saturating_add(
                            snap_candidates
                                .key_vertices()
                                .map_or(0, |items| items.len()),
                        )
                        .saturating_add(
                            snap_candidates
                                .key_segments()
                                .map_or(0, |items| items.len()),
                        );
                    let snap_preview_ms = candidate_ms + evaluation_ms;
                    if snap_preview_ms >= 1.0 {
                        crate::perf_record!(
                            "[perf] snap-preview {snap_preview_ms:.2}ms query={candidate_ms:.2}ms evaluate={evaluation_ms:.2}ms wires={} features={feature_count}",
                            snap_candidates.len(),
                        );
                    }
                }
                snap
            } else if discrete_preview_enabled {
                // Keep the last sample's marker until the next sample decides.
                // The object under the cursor offers its nearest point from
                // anywhere along it (#1914), so that marker is often outside
                // the aperture; dropping it here made the point jump between
                // the snap and the cursor on every other move (#2882).
                self.tabs[i].snap_result
            } else {
                None
            };
            // Keep the pale, stationary STRETCH reference snappable in the
            // capped fast path, without restoring its hidden resident wires or
            // feeding its geometry into the main drawing interaction index.
            let original_snap = if run_discrete_preview || refresh_stretch_preview {
                self.command_original_snap_hit(i, p, view_rot, eye, bounds)
            } else {
                None
            };
            let snap = crate::snap::merge_snap(snap, original_snap, p);
            // An unsampled move far off the dwelt point still ends that dwell.
            if !run_discrete_preview && self.snapper.tracking_active() {
                let wires = self.tabs[i].scene.hit_test_wires();
                self.snapper.note_unsampled_move(
                    p,
                    wires.as_slice(),
                    view_rot,
                    eye,
                    bounds,
                    Instant::now(),
                );
            }
            let world = match crate::snap::pulling_snap(snap, p, self.snapper.osnap_radius_px) {
                Some(hit) => hit.world,
                None => {
                    let mut pt = cursor_world;
                    let uses_command_cursor_plane = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .and_then(|command| command.cursor_plane())
                        .is_some();
                    if !uses_command_cursor_plane {
                        let mut drafted_elevation = false;
                        if let Some(base) = self.last_point {
                            // Match the authoritative Ortho/Polar path, including
                            // elevated drafting planes and the UCS Z direction.
                            if let Some((point, on_z)) =
                                self.drafting_point(i, p, pt, base, view_rot, eye, bounds)
                            {
                                pt = point;
                                drafted_elevation = true;
                                let up = (point - base).dot(self.tabs[i].ucs_xform().axes().3);
                                if on_z && up != 0.0 {
                                    self.otrack_active = Some((base, (point - base).normalize()));
                                    self.otrack_cross = None;
                                    self.otrack_kind = Some(if up > 0.0 {
                                        crate::snap::TrackingKind::PlusZ
                                    } else {
                                        crate::snap::TrackingKind::MinusZ
                                    });
                                }
                            }
                        }
                        if self.tabs[i].active_ucs.is_none() && !drafted_elevation {
                            pt.z = 0.0;
                        }
                    }
                    pt
                }
            };
            self.tabs[i].last_cursor_world = world;
            self.tabs[i].last_cursor_screen = p_full;
            let mut snap = snap;
            if let Some(hit) = snap.as_mut() {
                hit.screen.x += tile_b.x;
                hit.screen.y += tile_b.y;
            }
            self.tabs[i].snap_result = snap;
            if refresh_stretch_preview
                || self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|cmd| matches!(cmd.name(), "FROM" | "MTP"))
            {
                // Rebuild only the command overlay, not the resident drawing;
                // the full snap pass still waits for the normal dwell tick.
                self.refresh_active_cmd_preview(i);
            }
            // The fast indexed preview above now updates OTRACK's acquisition
            // clock from real, current geometry. Do NOT erase that clock here:
            // resetting dwell_since on every pointer movement made acquisition
            // impossible until the user stopped and triggered extra ticks.
            // A sampled absence / change is handled by update_otrack_dwell;
            // unsampled frames keep the last verified observation untouched.
            // Re-arm the dwell — tick fires SNAP_DWELL_MS after the last move.
            if let Some(dwell) = self.snap_dwell.as_mut() {
                dwell.last_move_at = Instant::now();
                dwell.point = p_full;
                dwell.tab = i;
                dwell.preview_dirty = true;
                dwell.snap_resolved = false;
                if run_discrete_preview || refresh_stretch_preview {
                    dwell.last_preview_snap_at = Instant::now();
                }
            }
            let throttle_ms = crate::perf::elapsed_ms(throttle_started);
            if throttle_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] pointer-move mode=command phase=discrete-snap {throttle_ms:.2}ms"
                );
            }
            return Task::none();
        }
        // Command finished between the last move and now — clear any stale
        // deferred snap so the next command starts with a clean slate.
        if self.tabs[i].active_cmd.is_none() {
            self.snap_dwell = None;
        }

        if self.tabs[i].active_cmd.is_some() {
            let (vw, vh) = vp_size;
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };
            // Inside a floating viewport the cursor, camera and wires are
            // all model-space (the viewport's own camera draws the
            // content), so snap / hit-test / preview run exactly like the
            // main model view — no paper projection, tracks pan/zoom/twist.
            let cursor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
            let (gx, gy, adaptive) = (self.grid_spacing_x, self.grid_spacing_y, self.grid_adaptive);
            let visible_step = |distance: f32, fov_y: f32| {
                let (sx, sy) = crate::ui::overlay::compute_grid_steps(
                    gx, gy, distance, fov_y, bounds, adaptive,
                );
                sx.max(sy)
            };
            let (view_rot, eye, grid_spacing) = match &edit_cam {
                Some(cam) => (
                    cam.view_proj_rte(bounds),
                    cam.eye(),
                    visible_step(cam.distance, cam.fov_y),
                ),
                None => {
                    let cam = self.tabs[i].scene.camera.borrow();
                    (
                        cam.view_proj_rte(bounds),
                        cam.eye(),
                        visible_step(cam.distance, cam.fov_y),
                    )
                }
            };
            // Sync the adaptive visible-grid step. It drives display and
            // snap-aperture tolerances — grid *snap* itself locks to the
            // fixed SNAPUNIT X/Y spacing from the Drafting Settings dialog.
            self.snapper.grid_spacing = grid_spacing;
            // Cursor and wires are model-space; the snap result is model.
            let snap_cursor = cursor_world;

            let hit_test_started = perf_move.then(Instant::now);
            let all_wires =
                if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                    self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(h, bounds.height)
                } else {
                    self.tabs[i].scene.hit_test_wires()
                };
            let hit_test_ms = crate::perf::elapsed_ms(hit_test_started);
            let all_wire_count = all_wires.len();
            let candidate_started = perf_move.then(Instant::now);
            let snap_candidates = self.tabs[i].scene.interaction_candidates_near(
                all_wires,
                snap_cursor,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
            );
            let candidate_ms = crate::perf::elapsed_ms(candidate_started);
            if perf_move && (hit_test_ms >= 1.0 || candidate_ms >= 1.0) {
                crate::perf_record!(
                    "[perf] snap-candidates hit-test={hit_test_ms:.2}ms query={candidate_ms:.2}ms wires={} candidates={}",
                    all_wire_count,
                    snap_candidates.len(),
                );
            }
            let needs_entity = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_entity_pick())
                .unwrap_or(false);
            let needs_structure = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_structure_point_pick())
                .unwrap_or(false);
            let is_gathering = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.is_selection_gathering())
                .unwrap_or(false);
            // A selection-window corner (STRETCH crossing window) is a
            // free point — Ortho/Polar must not pin it to an axis through
            // the first corner, or the rectangle collapses to a line
            // (#291).
            let is_window_corner = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.window_corner_pick())
                .unwrap_or(false);
            // An internal-point pick (HATCH/BOUNDARY) takes the bare cursor:
            // no object snap, tracking or Ortho/Polar (#2009).
            let raw_pick = self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|c| c.wants_raw_point());
            let free_point = is_window_corner || raw_pick;
            let needs_tan = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_tangent_pick())
                .unwrap_or(false);
            // An entity pick that also takes points (constraint point picks)
            // keeps object snap: the point is only recognised when the pick
            // lands exactly on it, and the markers show where that is. (#1526)
            let entity_pick_takes_points = needs_entity
                && self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.entity_pick_accepts_points());
            let wants_point = self.tabs[i].active_cmd.as_ref().is_some_and(|command| {
                !command.needs_entity_pick()
                    && !command.needs_tangent_pick()
                    && !command.is_selection_gathering()
            });
            let uses_command_cursor_plane = self.tabs[i]
                .active_cmd
                .as_ref()
                .and_then(|command| command.cursor_plane())
                .is_some();
            let axis_lock = if let Some(base) = self.last_point {
                self.active_axis_lock(
                    i,
                    cursor_world,
                    base,
                    wants_point && !free_point && !uses_command_cursor_plane,
                )
            } else {
                self.axis_lock_dir = None;
                None
            };
            let tracking_probe = (axis_lock.is_none() && !free_point)
                .then(|| {
                    self.active_otrack_hit(
                        i,
                        cursor_world,
                        None,
                        self.last_point,
                        true,
                        view_rot,
                        eye,
                        bounds,
                    )
                })
                .flatten();
            let snap_started = perf_move.then(Instant::now);
            self.tabs[i].snap_result = if (needs_entity && !entity_pick_takes_points)
                || is_gathering
                || needs_structure
                || raw_pick
            {
                None
            } else if needs_tan {
                self.snapper.snap_tangent_only(
                    snap_cursor.as_vec3(),
                    p,
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                )
            } else {
                let (go, gr) = self.drafting_grid_basis(i);
                // Construction anchor for perpendicular and tangent snaps
                // in full f64 precision.
                self.snapper.from_point = self.last_point;

                let construction_ray = if free_point {
                    None
                } else {
                    self.last_point.and_then(|base| {
                        self.active_construction_ray(i, p, snap_cursor, base, view_rot, eye, bounds)
                    })
                };
                let additional_rays =
                    tracking_probe.map(|track| [(track.base, track.base + track.dir)]);

                self.snapper.snap_with_additional_construction_rays_stable(
                    snap_cursor,
                    p,
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                    go,
                    gr,
                    construction_ray,
                    additional_rays.as_ref().map_or(&[], |rays| rays.as_slice()),
                    self.tabs[i].snap_result,
                )
            };
            // Restore discrete snap targets from STRETCH's original snapshot
            // even though the corresponding resident objects are hidden.
            let original_snap = self.command_original_snap_hit(i, p, view_rot, eye, bounds);
            self.tabs[i].snap_result =
                crate::snap::merge_snap(self.tabs[i].snap_result, original_snap, p);
            let snap_ms = crate::perf::elapsed_ms(snap_started);
            if perf_move && snap_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] snap-evaluate {snap_ms:.2}ms candidates={}",
                    snap_candidates.len()
                );
            }

            // Paper-space snapping through layout viewports. `edit_cam`
            // is None here, so `cursor_world` is the paper point and the sheet
            // fills the canvas (pane-local == canvas pixels). The viewport hit
            // arrives already projected onto the sheet, so everything
            // downstream keeps working in paper coordinates — LINE still draws
            // at the projected paper point.
            self.vp_snap_frame = None;
            if edit_cam.is_none()
                && !needs_entity
                && !is_gathering
                && !needs_structure
                && !needs_tan
                && !raw_pick
            {
                if let Some((vp_hit, frame)) =
                    self.paper_viewport_snap(i, p_full, vp_size, cursor_world)
                {
                    let merged =
                        crate::snap::merge_snap(self.tabs[i].snap_result, Some(vp_hit), p_full);
                    if merged.is_some_and(|hit| hit.viewport.is_some()) {
                        self.vp_snap_frame = Some(frame);
                    }
                    self.tabs[i].snap_result = merged;
                }
            }

            // Solid-face snaps (Nearest/Perpendicular to face): live
            // mesh-triangle evaluation merged priority-aware with the wire
            // snap above. Gated like ordinary point picks; the helper itself
            // no-ops unless a face mode is on.
            if !needs_entity
                && !is_gathering
                && !needs_structure
                && !needs_tan
                && !free_point
                && (self
                    .snapper
                    .is_on_3d(crate::snap::SnapType::FacePerpendicular)
                    || self.snapper.is_on_3d(crate::snap::SnapType::NearestFace))
            {
                let face_hit = self.tabs[i].scene.solid_face_snaps(
                    p,
                    view_rot,
                    eye,
                    bounds,
                    self.snapper.osnap_radius_px,
                    self.last_point,
                    self.snapper
                        .is_on_3d(crate::snap::SnapType::FacePerpendicular),
                    self.snapper.is_on_3d(crate::snap::SnapType::NearestFace),
                );
                if face_hit.is_some() {
                    self.tabs[i].snap_result =
                        crate::snap::merge_snap(self.tabs[i].snap_result, face_hit, p);
                }
            }

            // OTRACK's construction ray was included in the primary snap pass,
            // so discovering its intersection does not evaluate every snap mode
            // over this candidate set a second time.
            let snap_tracking_started = perf_move.then(Instant::now);
            self.snapper.update_otrack_dwell(
                self.tabs[i].snap_result,
                &snap_candidates,
                view_rot,
                eye,
                bounds,
                Instant::now(),
            );
            self.snapper.update_parallel(
                cursor_world,
                &snap_candidates,
                view_rot,
                eye,
                bounds,
                Instant::now(),
            );
            let otrack_hit = if axis_lock.is_none() && !is_gathering {
                self.active_otrack_hit(
                    i,
                    cursor_world,
                    self.tabs[i].snap_result,
                    self.last_point,
                    !free_point,
                    view_rot,
                    eye,
                    bounds,
                )
            } else {
                None
            };
            self.otrack_active = otrack_hit.map(|h| (h.base, h.dir));
            self.otrack_cross = otrack_hit.and_then(|h| h.cross);
            self.otrack_kind = otrack_hit.map(|h| h.kind);

            // Parallel snap: with nothing else snapped or tracked, lock
            // the point onto the line through last_point parallel to the
            // acquired reference, and drive the alignment guide off it.
            // (#277)
            if axis_lock.is_none()
                && !raw_pick
                && !is_gathering
                && self.tabs[i].snap_result.is_none()
                && otrack_hit.is_none()
            {
                if let Some(par) =
                    self.snapper
                        .parallel_snap(cursor_world, self.last_point, view_rot, eye, bounds)
                {
                    if let (Some(base), Some((dir, _))) =
                        (self.last_point, self.snapper.parallel_ref)
                    {
                        self.otrack_active = Some((base, dir));
                        self.otrack_cross = None;
                    }
                    self.tabs[i].snap_result = Some(par);
                }
            }
            let snap_tracking_ms = crate::perf::elapsed_ms(snap_tracking_started);
            if perf_move && snap_tracking_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] snap-tracking {snap_tracking_ms:.2}ms candidates={}",
                    snap_candidates.len()
                );
            }

            let pull = crate::snap::pulling_snap(
                self.tabs[i].snap_result,
                p,
                self.snapper.osnap_radius_px,
            );
            let effective = {
                // Ortho / polar from the last point keep its elevation.
                let mut drafted_elevation = false;
                let mut pt: glam::DVec3 = if let (Some(dir), Some(base)) =
                    (axis_lock, self.last_point)
                {
                    let point = pull.map(|snap| snap.world).unwrap_or(cursor_world);
                    axis_lock_apply(point, base, dir)
                } else if let Some(h) = otrack_hit.filter(|_| {
                    // Exact object snaps win over tracking alignment, but
                    // Extension is a construction guide and must remain
                    // eligible for OTRACK intersections.
                    !pull.is_some_and(|s| {
                        !matches!(
                            s.snap_type,
                            crate::snap::SnapType::Grid | crate::snap::SnapType::Extension
                        )
                    })
                }) {
                    h.aligned
                } else {
                    // Snap runs in model space (viewport camera or the
                    // model/paper view), so the result is already model.
                    let mut pt = pull.map(|s| s.world).unwrap_or(cursor_world);
                    let osnap_locked =
                        pull.is_some_and(|s| s.snap_type != crate::snap::SnapType::Grid);
                    let drafted = (!osnap_locked && !free_point && !uses_command_cursor_plane)
                        .then_some(self.last_point)
                        .flatten()
                        .and_then(|base| {
                            let hit = self.drafting_point(i, p, pt, base, view_rot, eye, bounds)?;
                            Some((base, hit))
                        });
                    if let Some((base, (point, on_z))) = drafted {
                        pt = point;
                        drafted_elevation = true;
                        // Straight up or down from the last point: the
                        // tracking ray carries typed distances and its
                        // guide, and the tip reads +Z / -Z (#2000).
                        let up = (point - base).dot(self.tabs[i].ucs_xform().axes().3);
                        if on_z && up != 0.0 {
                            self.otrack_active = Some((base, (point - base).normalize()));
                            self.otrack_cross = None;
                            self.otrack_kind = Some(if up > 0.0 {
                                crate::snap::TrackingKind::PlusZ
                            } else {
                                crate::snap::TrackingKind::MinusZ
                            });
                        }
                    }
                    pt
                };
                // Clamp to world XY only when no UCS is active; with a
                // UCS the point already lies on the UCS XY plane. A genuine
                // 3D object snap keeps its elevation — only grid snaps and
                // misses are flattened.
                if self.tabs[i].active_cmd.is_some()
                    && self.tabs[i].active_ucs.is_none()
                    && !uses_command_cursor_plane
                    && !drafted_elevation
                    && !snap_keeps_elevation(pull.map(|s| s.snap_type))
                {
                    pt.z = 0.0;
                }
                pt
            };
            let effective = if needs_entity
                && self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.entity_pick_accepts_points())
            {
                cursor_world
            } else {
                self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.cursor_axis())
                    .and_then(|(origin, direction)| {
                        command_axis_point(pull, p, bounds, view_rot, eye, origin, direction)
                    })
                    .unwrap_or(effective)
            };
            // Dynamic-input locked fields constrain the preview point
            // (#356): a typed angle pins the direction, a typed
            // distance pins the radius — the same resolution the Enter
            // commit uses, so preview and commit agree. The lock wins
            // over osnap/ortho/polar.
            let effective = {
                let locked = !needs_entity
                    && self.tabs[i].active_cmd.is_some()
                    && self.tabs[i].dyn_fields.iter().any(|f| f.buffer.is_some());
                if locked {
                    self.tabs[i].last_cursor_world = effective;
                    self.dyn_resolve_point().unwrap_or(effective)
                } else {
                    effective
                }
            };
            self.tabs[i].last_cursor_world = effective;
            self.tabs[i].last_cursor_screen = p_full;
            // Project the step anchor (an explicit `dyn_anchor` or the
            // last point) so the dynamic-input overlay can place its
            // guide geometry and labels.
            // `proj` returns a pane-local pixel; shift by the active pane
            // origin (`tile_b`, the viewport rect inside a viewport) so
            // the DYN guide/labels share the canvas frame with
            // `last_cursor_screen` (= p_full, canvas space).
            let proj = |bp: glam::DVec3| {
                let ndc = view_rot.project_point3((bp - eye).as_vec3());
                iced::Point::new(
                    (ndc.x + 1.0) * 0.5 * bounds.width + tile_b.x,
                    (1.0 - ndc.y) * 0.5 * bounds.height + tile_b.y,
                )
            };
            // Anchors are stored in model coords and `proj` (view_rot /
            // eye) is the model→screen view — the viewport camera inside
            // a viewport, the model/paper camera otherwise — so feed them
            // straight through; no paper mapping needed.
            let anchor = self.tabs[i].dyn_anchor.or(self.last_point);
            let dyn_ref = self.tabs[i].dyn_ref;
            let lps = anchor.map(&proj);
            let drs = dyn_ref.map(&proj);
            self.tabs[i].last_point_screen = lps;
            self.tabs[i].dyn_ref_screen = drs;

            // Point-picked selection window (STRETCH): draw a filled
            // crossing marquee from the first corner to the cursor so it
            // reads like a normal box selection, not a bare outline (#291).
            let window_first = self.tabs[i]
                .active_cmd
                .as_ref()
                .and_then(|c| c.window_first_corner());
            self.tabs[i]
                .scene
                .selection
                .borrow_mut()
                .gesture
                .preview_box = if is_window_corner {
                window_first.map(|c1| (proj(c1), p_full, true))
            } else {
                None
            };

            // Entity-pick previews (TRIM/EXTEND/FILLET…) compare the
            // cursor against WCS document entities and return WCS wires.
            // `effective` is offset-relative, so build a WCS copy for
            // the click and shift the returned wires back to the
            // offset-relative frame the renderer expects (model space
            // only; paper-space entities use sheet coordinates).
            let wo_local = if self.tabs[i].scene.current_layout == "Model" {
                [0.0_f64; 3]
            } else {
                [0.0; 3]
            };
            let wo_f = glam::DVec3::new(wo_local[0], wo_local[1], wo_local[2]);
            let effective_wcs = effective + wo_f;

            // Orange object snap (plugin commands implement resolve_object_pick).
            if needs_structure {
                use crate::snap::{SnapResult, SnapType};
                let pick = self.tabs[i].active_cmd.as_ref().and_then(|c| {
                    c.resolve_object_pick(
                        &self.tabs[i].scene,
                        effective.x as f64,
                        effective.y as f64,
                    )
                });
                if let Some(pick) = pick {
                    let world = glam::DVec3::new(pick.x, pick.y, effective.z);
                    let ndc = view_rot.project_point3((world - eye).as_vec3());
                    let screen = iced::Point::new(
                        (ndc.x + 1.0) * 0.5 * bounds.width,
                        (1.0 - ndc.y) * 0.5 * bounds.height,
                    );
                    self.tabs[i].snap_result = Some(SnapResult {
                        world,
                        screen,
                        snap_type: SnapType::ObjectPick,
                        tangent_obj: None,
                        extension_base: None,
                        extension_base2: None,
                        extension_origin: None,
                        extension_dir: None,
                        viewport: None,
                        source: None,
                        secondary_source: None,
                        model_point: None,
                    });
                    if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                        cmd.set_acquisition_hint(Some(pick.label));
                    }
                } else if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                    cmd.set_acquisition_hint(None);
                }
            }

            // Move Extension's acquired endpoints along with the snap glyph.
            // Previously only s.screen moved to canvas pixels, leaving the
            // dashed Extension line offset from the actual endpoint.
            if let Some(s) = self.tabs[i].snap_result.as_mut() {
                snap_pixels_to_canvas(s, tile_b);
            }

            // Give the command the current UCS + Ctrl state before it
            // builds its rubber-band preview (and, by persistence, its
            // commit).
            self.push_ucs_to_cmd(i);
            self.push_ctrl_to_cmd(i);
            let preview_started = perf_move.then(Instant::now);
            let modifier_tangent = self.tabs[i]
                .snap_result
                .filter(|s| s.snap_type == crate::snap::SnapType::Tangent)
                .and_then(|s| s.tangent_obj);
            let mut modifier_hidden = None;
            let parameter_preview = self.block_parameter_geometry_preview(i, effective);
            let mut previews = if let Some((wires, hidden)) =
                self.point_modifier_preview(i, effective, modifier_tangent)
            {
                modifier_hidden = Some(hidden);
                wires
            } else if let Some(preview) = parameter_preview {
                // Same branch as refresh_active_cmd_preview: without it the
                // move pass and the dwell tick alternate frames (#3010).
                self.tabs[i]
                    .scene
                    .set_command_preview_hatches(preview.hatches);
                modifier_hidden = Some(preview.hidden);
                preview.wires
            } else if needs_structure {
                let mut p = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.object_pick_hover_previews(&self.tabs[i].scene, effective))
                    .unwrap_or_default();
                let live_tangent = self.tabs[i].snap_result.and_then(|s| {
                    if s.snap_type == crate::snap::SnapType::Tangent {
                        s.tangent_obj
                    } else {
                        None
                    }
                });
                self.refresh_command_point_pick_context(i);
                let tab = &mut self.tabs[i];
                if let Some(cmd) = tab.active_cmd.as_mut() {
                    p.extend(cmd.on_preview_wires_with_tangent(effective, live_tangent));
                    if !cmd.preview_wires_replace_entities() {
                        let entities = cmd.preview_entities(&tab.scene.document, effective);
                        p.extend(tab.scene.wires_for_entities(&entities));
                    }
                }
                p
            } else if needs_entity && deferred_command_hover {
                // The dwell callback resolves and outlines bounded areas once.
                // Moving the cursor clears that outline without rebuilding it.
                Vec::new()
            } else if needs_entity {
                let pick_aperture = crate::ui::overlay::pick_box_aperture_px(self.pick_box);
                let include_fills = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.entity_pick_includes_fills())
                    .unwrap_or(false);
                let candidate_handles = if include_fills {
                    self.tabs[i]
                        .scene
                        .interaction_candidate_handles(&snap_candidates)
                } else {
                    None
                };
                let hovered = scene::pick::hit_test::click_hit(
                    p,
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                    self.tabs[i].scene.document.header.lineweight_display,
                    pick_aperture,
                    &self.tabs[i].scene.draw_depth_map(),
                )
                .and_then(Scene::handle_from_wire_name)
                .or_else(|| {
                    if !include_fills {
                        return None;
                    }
                    scene::pick::hit_test::click_hit_hatch(
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
                    )
                })
                .or_else(|| {
                    include_fills.then(|| {
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
                    })?
                })
                .or_else(|| {
                    include_fills.then(|| {
                        self.tabs[i].scene.solid_hover_hit(
                            p,
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })?
                });
                let preview_in_command = (self.model_space.selection_preview & 2) != 0;
                let highlights_hover = self.tabs[i].active_cmd.as_ref().is_some_and(|command| {
                    command.entity_pick_highlights_hover()
                        && command.name() != "MATCHPROP"
                        && preview_in_command
                });
                if highlights_hover {
                    self.tabs[i].scene.set_hover_highlight(hovered);
                } else {
                    self.tabs[i].scene.set_hover_highlight(None);
                }
                let hover_handle = hovered.unwrap_or(document::Handle::NULL);
                let wants_entity = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.wants_hover_entity(hover_handle));
                if wants_entity {
                    if let Some(entity) = self.tabs[i]
                        .scene
                        .document
                        .get_entity(hover_handle)
                        .cloned()
                    {
                        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                            command.inject_hover_entity(hover_handle, entity);
                        }
                    }
                }
                let shift = self.shift_down;
                let mut p = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .map(|c| {
                        // Live shift state drives TRIM/EXTEND's
                        // shift-swap preview (#336).
                        c.set_shift(shift);
                        c.on_hover_entity(hover_handle, effective_wcs)
                    })
                    .unwrap_or_default();
                // on_hover_entity returns WCS wires; shift to the
                // offset-relative frame so the preview lands on the
                // geometry on large-coordinate drawings.
                if wo_f != glam::DVec3::ZERO {
                    for w in p.iter_mut() {
                        for pt in w.points.iter_mut() {
                            pt[0] -= wo_f.x as f32;
                            pt[1] -= wo_f.y as f32;
                            pt[2] -= wo_f.z as f32;
                        }
                    }
                }
                if !hover_handle.is_null() {
                    if let Some(cmd) = self.tabs[i].active_cmd.as_ref() {
                        p.extend(
                            cmd.entity_pick_acquire_previews(&self.tabs[i].scene, hover_handle),
                        );
                    }
                    if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                        if let Some(hint) = cmd.entity_pick_acquire_hint(hover_handle) {
                            cmd.set_acquisition_hint(Some(hint));
                        }
                    }
                }
                p
            } else if is_gathering
                && self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.selection_hover_preview_enabled())
            {
                let candidate_handles = self.tabs[i]
                    .scene
                    .interaction_candidate_handles(&snap_candidates);
                let hovered = if dragging {
                    None
                } else {
                    scene::pick::hit_test::click_hit(
                        p,
                        &snap_candidates,
                        view_rot,
                        eye,
                        bounds,
                        self.tabs[i].scene.document.header.lineweight_display,
                        crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                        &self.tabs[i].scene.draw_depth_map(),
                    )
                    .and_then(Scene::handle_from_wire_name)
                    .or_else(|| {
                        scene::pick::hit_test::click_hit_hatch(
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
                        self.tabs[i].scene.solid_hover_hit(
                            p,
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })
                };
                let preview = self.tabs[i].active_cmd.as_ref().and_then(|command| {
                    command.selection_hover_preview(&self.tabs[i].scene, hovered)
                });
                if let Some(preview) = preview {
                    self.tabs[i]
                        .scene
                        .set_command_preview_hatches(preview.hatches);
                    preview.wires
                } else {
                    Vec::new()
                }
            } else if let Some(wires) = self.dimension_preview_wires(i, effective) {
                wires
            } else {
                let live_tangent = self.tabs[i].snap_result.and_then(|s| {
                    if s.snap_type == crate::snap::SnapType::Tangent {
                        s.tangent_obj
                    } else {
                        None
                    }
                });
                self.refresh_command_point_pick_context(i);
                let tab = &mut self.tabs[i];
                if let Some(cmd) = tab.active_cmd.as_mut() {
                    let mut wires = cmd.on_preview_wires_with_tangent(effective, live_tangent);
                    // STRETCH replaces its source entities while the pointer moves.
                    // Without the edited-entity overlay, its source hide-set makes
                    // the selected lines disappear until the final click.
                    let entities = cmd.preview_entities(&tab.scene.document, effective);
                    wires.extend(tab.scene.wires_for_entities(&entities));
                    wires
                } else {
                    Vec::new()
                }
            };
            // While a command selection box (or lasso) is being drawn,
            // preview the operation for all touched objects before release.
            // The actual selection/commit path remains authoritative. This
            // opt-in only affects commands exposing a drag-selection preview.
            let (box_anchor, box_crossing, poly_active, poly_crossing, poly_points) = {
                let selection = self.tabs[i].scene.selection.borrow();
                (
                    selection.gesture.box_anchor,
                    selection.gesture.box_crossing,
                    selection.gesture.poly_active,
                    selection.gesture.poly_crossing,
                    if selection.gesture.poly_active {
                        selection.gesture.poly_points.clone()
                    } else {
                        Vec::new()
                    },
                )
            };
            if self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.drag_selection_preview_enabled())
                && (box_anchor.is_some() || poly_active)
            {
                let (screen_fence, window, crossing) = if poly_active {
                    let mut points = poly_points;
                    if points.last().is_none_or(|last| {
                        (last.x - p_full.x).abs() > f32::EPSILON
                            || (last.y - p_full.y).abs() > f32::EPSILON
                    }) {
                        points.push(p_full);
                    }
                    if let Some(first) = points.first().copied() {
                        points.push(first);
                    }
                    (points, None, poly_crossing)
                } else {
                    let anchor = box_anchor.unwrap();
                    let corners = vec![
                        anchor,
                        iced::Point::new(anchor.x, p_full.y),
                        p_full,
                        iced::Point::new(p_full.x, anchor.y),
                        anchor,
                    ];
                    let window = Some((
                        [anchor.x.min(p_full.x), anchor.y.min(p_full.y)],
                        [anchor.x.max(p_full.x), anchor.y.max(p_full.y)],
                    ));
                    (corners, window, box_crossing)
                };
                let fence: Vec<[f64; 2]> = screen_fence
                    .iter()
                    .map(|point| {
                        let local = iced::Point::new(point.x - tile_b.x, point.y - tile_b.y);
                        let world = self.cursor_model_point(i, &edit_cam, local, bounds);
                        [world.x, world.y]
                    })
                    .collect();
                if fence.len() >= 3 {
                    let xy_min_max = fence.iter().fold(
                        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
                        |(mut min, mut max), point| {
                            for axis in 0..2 {
                                min[axis] = min[axis].min(point[axis]);
                                max[axis] = max[axis].max(point[axis]);
                            }
                            (min, max)
                        },
                    );
                    let area = [
                        xy_min_max.0[0],
                        xy_min_max.0[1],
                        xy_min_max.1[0],
                        xy_min_max.1[1],
                    ];
                    // For rectangles, use model-space corners: a pixel box
                    // may not map 1:1 onto world coordinates when rotated.
                    let window = window.map(|_| xy_min_max);
                    let mut nearby: Vec<_> = self.tabs[i]
                        .scene
                        .interaction_handles_in_world_aabb(area)
                        .into_iter()
                        .collect();
                    nearby.sort_unstable_by_key(|handle| handle.value());
                    if let Some(mut box_preview) =
                        self.tabs[i].active_cmd.as_mut().and_then(|command| {
                            command.on_drag_selection_preview(&fence, window, crossing, &nearby)
                        })
                    {
                        if wo_f != glam::DVec3::ZERO {
                            for wire in &mut box_preview {
                                for point in &mut wire.points {
                                    point[0] -= wo_f.x as f32;
                                    point[1] -= wo_f.y as f32;
                                    point[2] -= wo_f.z as f32;
                                }
                            }
                        }
                        previews = box_preview;
                        self.tabs[i].scene.set_hover_highlight(None);
                    }
                }
            }
            // Polar tracking guide line: dotted line from last_point along
            // the snapped angle direction, extending across the drawing.
            if self.polar_mode && !needs_entity {
                if let Some(base) = self.last_point {
                    // Screen-space dotted guide (render geometry is f32).
                    let base = base.as_vec3();
                    let effective = effective.as_vec3();
                    let dx = effective.x - base.x;
                    let dy = effective.y - base.y;
                    // Only show the guide while POLAR is actually engaged
                    // — i.e. the point is snapped onto a polar ray, not
                    // floating free near no angle (issue #70).
                    let step = self.polar_increment_deg.to_radians();
                    let angle = dy.atan2(dx);
                    let snapped =
                        step > 1e-6 && ((angle / step).round() * step - angle).abs() < 1e-3;
                    if snapped && (dx * dx + dy * dy).sqrt() > 1e-4 {
                        let far = 1e5_f32;
                        let dir = glam::Vec3::new(dx, dy, 0.0).normalize();
                        let far_pos = base + dir * far;
                        let far_neg = base - dir * far;
                        let guide = crate::scene::WireModel {
                            bg_adapt: None,
                            point_marker: None,
                            world_width: 0.0,
                            depth_override: None,
                            display_visible: true,
                            snap_only: false,
                            plot_visible: true,
                            fill_is_3d: false,
                            fill_is_2d_solid: false,
                            solid_normal: None,
                            render_instance: None,
                            dash_from_start: false,
                            name: "__polar_guide__".into(),
                            color: [0.2, 0.7, 0.9, 0.6],
                            selected: false,
                            aci: 0,
                            pattern_length: 0.8,
                            pattern: [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                            line_weight_px: 1.0,
                            aabb: crate::scene::WireModel::UNBOUNDED_AABB,
                            plinegen: true,
                            geom: std::sync::Arc::new(crate::scene::WireGeom {
                                taper_widths: Vec::new(),
                                pattern_stations: Vec::new(),
                                text_verts: Vec::new(),
                                points: vec![
                                    [far_neg.x, far_neg.y, far_neg.z],
                                    [far_pos.x, far_pos.y, far_pos.z],
                                ],
                                points_low: Default::default(),
                                snap_pts: vec![],
                                tangent_geoms: vec![],
                                key_vertices: vec![],
                                ..crate::scene::WireGeom::default()
                            }),
                            ..Default::default()
                        };
                        previews.push(guide);
                    }
                }
            }
            // OTRACK alignment guide: dashed ray through the tracking
            // point along the aligned direction (issue #69).
            if let Some(h) = otrack_hit {
                if !needs_entity {
                    let far = 1e5_f64;
                    let far_pos = h.base + h.dir * far;
                    let far_neg = h.base - h.dir * far;
                    let mut guide = crate::scene::WireModel::solid_f64(
                        "__otrack_guide__".into(),
                        vec![
                            [far_neg.x, far_neg.y, far_neg.z],
                            [far_pos.x, far_pos.y, far_pos.z],
                        ],
                        [0.2, 0.9, 0.5, 0.6],
                        false,
                    );
                    guide.pattern_length = 0.8;
                    guide.pattern = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                    previews.push(guide);
                }
            }
            self.tint_creation_preview(i, &mut previews);
            let preview_hidden = modifier_hidden.unwrap_or_else(|| {
                self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.preview_hidden_handles().to_vec())
                    .unwrap_or_default()
            });
            self.tabs[i]
                .scene
                .set_command_preview_hidden(&preview_hidden);
            self.tabs[i].scene.set_preview_wires(previews);
            let preview_ms = crate::perf::elapsed_ms(preview_started);
            if perf_move && preview_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] command-preview-compute {preview_ms:.2}ms wires={}",
                    self.tabs[i].scene.preview_wires.len(),
                );
            }
            // Keep the throttle gate armed. The current position has already
            // completed the full snap pass; a later pointer move will defer
            // snapping until its own dwell expires.
            self.snap_dwell = Some(crate::app::SnapDwell {
                last_move_at: Instant::now(),
                last_preview_snap_at: Instant::now(),
                point: p_full,
                tab: i,
                preview_dirty: false,
                // Stationary OTRACK acquisition needs repeated observations,
                // not just the first resolved snap. Keep ticks alive only
                // until that candidate has acquired (or disappeared).
                snap_resolved: !(self.snapper.tracking_active()
                    && self.snapper.last_snap_world.is_some()
                    && !self.snapper.dwell_acquired),
                full_snap_due: Instant::now()
                    + std::time::Duration::from_millis(crate::app::SNAP_MOVING_MS),
            });
        } else if !self.client_pick_pending(i) {
            // Idle (no command, no pending client pick): the snap marker is
            // command-only, so clear it — a pending client pick keeps the
            // marker its move-time snap block above computed.
            self.tabs[i].snap_result = None;
        }

        self.sync_dyn_fields();
        let move_ms = move_started.elapsed().as_secs_f64() * 1000.0;
        if perf_move && move_ms >= 10.0 {
            let mode = if self.tabs[i].active_cmd.is_some() {
                "command"
            } else if self.tabs[i].scene.active_viewport.is_some() {
                "MSPACE"
            } else {
                "idle"
            };
            crate::perf_record!(
                "[perf] pointer-move mode={mode:<7} {:>7.1}ms pre_grip={pre_grip_ms:.2}ms grip_hover={grip_hover_ms:.2}ms",
                move_ms,
            );
        }
        Task::none()
    }

}
