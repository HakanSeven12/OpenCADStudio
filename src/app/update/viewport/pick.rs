use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn solid_select_hit(
        &self,
        i: usize,
        p: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&rustc_hash::FxHashSet<Handle>>,
        aperture: f32,
    ) -> Option<Handle> {
        let scene = &self.tabs[i].scene;
        scene
            .solid_edge_hover_hit(p, view_rot, eye, bounds, candidate_handles, aperture)
            .or_else(|| {
                (!self.tabs[i].wireframe)
                    .then(|| scene.solid_click_hit(p, view_rot, eye, bounds, candidate_handles))?
            })
    }

    pub(in crate::app) fn select_command_marker_region(
        &mut self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        bounds: iced::Rectangle,
        polygon: &[Point],
        crossing: bool,
        replace: bool,
        geometry_hit: bool,
    ) -> bool {
        let (view, eye) = match edit_cam {
            Some(camera) => (camera.view_proj_rte(bounds), camera.eye()),
            None => {
                let camera = self.tabs[i].scene.camera.borrow();
                (camera.view_proj_rte(bounds), camera.eye())
            }
        };
        let context = crate::command::PointPickContext {
            view,
            eye,
            bounds,
            aperture_px: crate::ui::overlay::pick_box_aperture_px(self.pick_box),
        };
        let remove = self.shift_down || self.select_remove_mode;
        let tab = &mut self.tabs[i];
        let names = tab
            .active_cmd
            .as_mut()
            .map(|command| {
                command.on_selection_marker_region(
                    &tab.scene,
                    polygon,
                    context,
                    crossing,
                    remove,
                    replace,
                    geometry_hit,
                )
            })
            .unwrap_or_default();
        for name in &names {
            self.command_line
                .push_info(&format!("{} {}", if remove { "−" } else { "+" }, name));
        }
        !names.is_empty()
    }

    pub(in crate::app) fn active_construction_ray(
        &self,
        tab: usize,
        screen: Point,
        cursor: glam::DVec3,
        base: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Option<(glam::DVec3, glam::DVec3)> {
        let (target, _) = self.drafting_point(tab, screen, cursor, base, view_rot, eye, bounds)?;
        Some((base, target))
    }

    /// Ortho / polar from `base`, for the cursor, the click and grip drags
    /// alike. In-plane the point keeps `base`'s elevation, so the band runs
    /// parallel to the UCS XY plane. In a view that shows the UCS Z axis,
    /// that axis is a drafting direction too (#2000): with the cursor
    /// (`screen`, pane-local) nearer its projection than the in-plane
    /// candidate, the point runs straight up or down from `base`. Returns
    /// the point and whether it lies on that Z axis; `None` while neither
    /// mode is engaged.
    pub(in crate::app) fn drafting_point(
        &self,
        tab: usize,
        screen: Point,
        cursor: glam::DVec3,
        base: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Option<(glam::DVec3, bool)> {
        // Holding Shift turns ortho off for as long as it is held; with
        // ortho off it is the axis lock instead (`active_axis_lock`).
        let ortho = self.ortho_mode && !self.shift_down;
        if !ortho && !self.polar_mode {
            return None;
        }
        let xf = self.tabs[tab].ucs_xform();
        let (_, ux, uy, uz) = xf.axes();
        // A cursor on another level than `base` is lifted onto its plane.
        let level = (xf.to_ucs(cursor).z - xf.to_ucs(base).z).abs();
        let cursor = if level > 1e-9 * (1.0 + base.length()) {
            cursor_on_plane(screen, bounds, view_rot, eye, base, uz).unwrap_or(cursor)
        } else {
            cursor
        };
        let planar = if ortho {
            Some(drafting_constrain(
                cursor,
                base,
                &xf,
                self.isometric_drafting,
                self.iso_plane,
                self.snap_angle_deg,
            ))
        } else {
            polar_constrain_if_near(
                cursor,
                base,
                self.polar_increment_deg,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
                &xf,
            )
        };
        let to_screen = |w: glam::DVec3| {
            let ndc = view_rot.project_point3((w - eye).as_vec3());
            glam::Vec2::new(
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            )
        };
        let at = glam::Vec2::new(screen.x, screen.y);
        let gap = |w: glam::DVec3| to_screen(w).distance(at);
        let along =
            |dir: glam::DVec3| cursor_on_projected_axis(screen, bounds, view_rot, eye, base, dir);
        // A plan view shows the Z axis end-on: no third direction there.
        let reach = (cursor - base).length();
        let origin = to_screen(base);
        let shown = |dir: glam::DVec3| to_screen(base + dir * reach).distance(origin);
        let z_point = (reach > 0.0 && shown(uz) > 0.05 * shown(ux).max(shown(uy)))
            .then(|| along(uz))
            .flatten();
        let Some(z_point) = z_point else {
            return planar.map(|point| (point, false));
        };
        let z_gap = gap(z_point);
        let planar_gap = if ortho {
            let (a, b, _) = drafting_axes(
                ux,
                uy,
                uz,
                self.isometric_drafting,
                self.iso_plane,
                self.snap_angle_deg,
            );
            [a, b]
                .into_iter()
                .filter_map(along)
                .map(gap)
                .fold(f32::INFINITY, f32::min)
        } else {
            planar.map_or(f32::INFINITY, gap)
        };
        let engaged = ortho || z_gap <= self.snapper.osnap_radius_px;
        if engaged && z_gap < planar_gap {
            Some((z_point, true))
        } else {
            planar.map(|point| (point, false))
        }
    }

    pub(in crate::app) fn active_axis_lock(
        &mut self,
        tab: usize,
        cursor: glam::DVec3,
        base: glam::DVec3,
        allowed: bool,
    ) -> Option<glam::DVec3> {
        if let Some(direction) = self.angle_override.filter(|_| allowed) {
            self.axis_lock_dir = Some(direction);
            return self.axis_lock_dir;
        }
        if self.shift_down && allowed && !self.ortho_mode {
            if self.axis_lock_dir.is_none() {
                // Right after a point is placed the cursor still sits on it
                // and its direction is noise: wait until it has moved away,
                // so each segment takes the axis it is drawn along (#1422).
                let height = self.tabs[tab]
                    .scene
                    .selection
                    .borrow()
                    .view
                    .vp_size
                    .1
                    .max(1.0);
                let ortho_size = self.tabs[tab].scene.camera.borrow().ortho_size();
                let world_per_px = 2.0 * f64::from(ortho_size) / f64::from(height);
                if (cursor - base).length() < world_per_px * 8.0 {
                    return None;
                }
                let ucs = self.tabs[tab].ucs_xform();
                self.axis_lock_dir = axis_lock_capture(
                    cursor,
                    base,
                    self.polar_mode,
                    self.polar_increment_deg,
                    &ucs,
                    self.isometric_drafting,
                    self.iso_plane,
                    self.snap_angle_deg,
                );
            }
        } else {
            self.axis_lock_dir = None;
        }
        self.axis_lock_dir
    }

    pub(in crate::app) fn active_otrack_hit(
        &self,
        tab: usize,
        cursor: glam::DVec3,
        snap: Option<crate::snap::SnapResult>,
        base: Option<glam::DVec3>,
        drafting: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Option<crate::snap::OtrackHit> {
        let intersection = snap.filter(|hit| hit.snap_type == crate::snap::SnapType::Intersection);

        // Ordinary object snaps suppress OTRACK. Extension and Intersection are
        // exceptions: both may lie on an active tracking vector.
        if snap.is_some_and(|hit| {
            !matches!(
                hit.snap_type,
                crate::snap::SnapType::Extension | crate::snap::SnapType::Intersection
            )
        }) {
            return None;
        }

        let required_crossing_ray = match snap {
            Some(extension) if extension.snap_type == crate::snap::SnapType::Extension => {
                Some((extension.extension_origin?, extension.extension_dir?))
            }
            _ => None,
        };

        // When Intersection has already won OSNAP selection, test OTRACK at the
        // exact intersection rather than at the free cursor position.
        let track_cursor = intersection.map(|hit| hit.world).unwrap_or(cursor);

        let step = (self.polar_mode && drafting).then_some(self.polar_increment_deg);
        let (_, (ucs_x, ucs_y, _)) = self.drafting_grid_basis(tab);

        let mut hit = self.snapper.otrack_snap_stable(
            track_cursor,
            view_rot,
            eye,
            bounds,
            step,
            base,
            required_crossing_ray,
            self.ortho_mode && !self.shift_down && drafting,
            ucs_x.as_dvec3(),
            ucs_y.as_dvec3(),
            self.otrack_active,
        )?;

        if let Some(intersection) = intersection {
            // Only keep the vector when it really passes through the highlighted
            // intersection. This avoids showing an unrelated nearby tracking ray.
            let ndc = view_rot.project_point3((hit.aligned - eye).as_vec3());
            let aligned_screen = iced::Point::new(
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            );

            let dx = aligned_screen.x - intersection.screen.x;
            let dy = aligned_screen.y - intersection.screen.y;

            if dx * dx + dy * dy > 4.0 {
                return None;
            }

            // OTRACK is only providing the visual/reference vector here.
            // Intersection remains the exact picked point.
            hit.aligned = intersection.world;
        }

        Some(hit)
    }

    /// Begin a staged Point Style edit. The active drawing is untouched until
    /// the user accepts the dialog.
    pub(in crate::app) fn begin_point_style_dialog(&mut self) {
        let i = self.active_tab;
        let header = &self.tabs[i].scene.document.header;
        let mode = header.point_display_mode;
        let size = header.point_display_size;
        self.point_style_tab_id = Some(self.tabs[i].id);
        self.point_mode_buf = Some(mode);
        self.point_size_relative = size <= 0.0;
        self.point_size_buf = format!("{}", size.abs());
        self.active_modal = Some(ModalKind::PointStyle);
    }

    /// Parse and validate the dialog's magnitude without changing the drawing.
    pub(in crate::app) fn validate_point_size_input(&mut self) -> bool {
        let valid = self
            .point_size_buf
            .trim()
            .parse::<f64>()
            .is_ok_and(f64::is_finite);
        if !valid {
            self.command_line
                .push_error(crate::t!("Point size must be a valid finite number.").as_ref());
        }
        valid
    }

    /// Commit the staged glyph and size together as one drawing undo step.
    /// Returns false on invalid text so OK can leave the dialog open for repair.
    pub(in crate::app) fn commit_point_style_dialog(&mut self) -> bool {
        let Some(magnitude) = self
            .point_size_buf
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(f64::abs)
        else {
            self.command_line
                .push_error(crate::t!("Point size must be a valid finite number.").as_ref());
            return false;
        };
        let Some(tab_id) = self.point_style_tab_id else {
            return false;
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            self.command_line
                .push_error(crate::t!("The drawing for Point Style is no longer open.").as_ref());
            return false;
        };

        let mut magnitude = magnitude;
        if !self.point_size_relative && magnitude == 0.0 {
            let wpp = self.tabs[i].scene.world_per_pixel().unwrap_or(0.0);
            let viewport_height = self.tabs[i].scene.selection.borrow().view.vp_size.1;
            magnitude = if wpp > 0.0 {
                crate::entities::point::relative_world_size(0.0, wpp, viewport_height)
            } else {
                1.0
            };
            self.point_size_buf = format!("{magnitude:.4}");
        }
        let next_size = if self.point_size_relative {
            -magnitude
        } else {
            magnitude
        };
        let next_mode = self
            .point_mode_buf
            .unwrap_or(self.tabs[i].scene.document.header.point_display_mode);
        let header = &self.tabs[i].scene.document.header;
        if header.point_display_mode == next_mode && header.point_display_size == next_size {
            return true;
        }

        self.push_undo_snapshot(i, "DDPTYPE");
        let header = &mut self.tabs[i].scene.document.header;
        header.point_display_mode = next_mode;
        header.point_display_size = next_size;
        self.tabs[i].scene.invalidate_point_dependencies();
        self.tabs[i].dirty = true;
        true
    }

    /// Stage a replacement for the `mask` bits of PDMODE from the Point Style
    /// (DDPTYPE) dialog. The drawing is changed only when OK commits the stage.
    pub(in crate::app) fn set_point_mode_bits(&mut self, mask: i16, value: i16) {
        if self.point_style_tab_id.is_none() {
            return;
        }
        let Some(cur) = self.point_mode_buf else {
            return;
        };
        let next = (cur & !mask) | (value & mask);
        self.point_mode_buf = Some(next);
    }

    /// Mirror the live grid display + grid-snap toggles onto tab `i`'s active
    /// model tile so a save writes them to that viewport's VPort entry (#121).
    pub(in crate::app) fn sync_vport_display(&mut self, i: usize) {
        let grid_on = self.show_grid;
        let snap_on = self.snapper.grid_snap();
        self.tabs[i]
            .scene
            .set_active_tile_grid_snap(grid_on, snap_on);
    }

    /// Grid origin plus the rotated/isometric axes used by both drawing and snap.
    pub(in crate::app) fn drafting_grid_basis(
        &self,
        i: usize,
    ) -> (glam::Vec3, (glam::Vec3, glam::Vec3, glam::Vec3)) {
        let (origin, rotation) = self.tabs[i].ucs_grid_basis();
        let x = rotation.transform_vector3(glam::Vec3::X).as_dvec3();
        let y = rotation.transform_vector3(glam::Vec3::Y).as_dvec3();
        let z = rotation.transform_vector3(glam::Vec3::Z).as_dvec3();
        let (x, y, z) = drafting_axes(
            x,
            y,
            z,
            self.isometric_drafting,
            self.iso_plane,
            self.snap_angle_deg,
        );
        (origin, (x.as_vec3(), y.as_vec3(), z.as_vec3()))
    }

    /// Adopt the active viewport's display state into the live toggles. Called
    /// on load and whenever the active tab or viewport changes.
    ///
    /// Grid *snap* (`SnapType::Grid`) is deliberately NOT adopted here: it stays
    /// off by default everywhere and is controlled solely by the user's snap
    /// toggle, so it never silently leaks into object-snap when entering a
    /// viewport whose stored `snap_on` flag happens to be set.
    pub(in crate::app) fn adopt_view_display(&mut self, i: usize) {
        if let Some((grid_on, _snap_on)) = self.tabs[i].scene.active_tile_grid_snap() {
            self.show_grid = grid_on;
        }
        let ortho =
            self.tabs[i].scene.active_camera_projection() == crate::scene::Projection::Orthographic;
        self.ribbon.set_ortho(ortho);
    }

    pub(in crate::app) fn sync_render_mode_to_active_tile(&mut self, i: usize) {
        use document::entities::ViewportRenderMode as M;
        if self.tabs[i].scene.current_layout != "Model" {
            return;
        }
        let mode = self.tabs[i].scene.active_model_tile_render_mode();
        if self.tabs[i].render_mode == mode {
            return;
        }
        let label = match mode {
            M::Wireframe2D => "Wireframe 2D",
            M::Wireframe3D => "Wireframe 3D",
            M::HiddenLine => "Hidden Line",
            M::FlatShaded => "Flat Shaded",
            M::GouraudShaded => "Gouraud Shaded",
            M::FlatShadedWithEdges => "Flat Shaded + Edges",
            M::GouraudShadedWithEdges => "Gouraud Shaded + Edges",
        };
        self.tabs[i].render_mode = mode;
        let wf = matches!(mode, M::Wireframe2D | M::Wireframe3D);
        self.tabs[i].wireframe = wf;
        self.ribbon.set_wireframe(wf);
        self.tabs[i].visual_style = label.into();
        self.tabs[i].scene.bump_geometry_no_blocks();
    }

    /// Project a pane-local cursor onto the active drawing plane and return a
    /// **model-space** point. Inside a floating viewport `edit_cam` is the
    /// viewport's own camera (a target-plane / UCS pick there already yields
    /// model coords); otherwise the paper camera projects onto the sheet and
    /// the result is mapped paper→model. Used by the readout, snap and click
    /// paths so all three agree on the cursor's model location.
    pub(in crate::app) fn cursor_model_point(
        &self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        p: iced::Point,
        bounds: iced::Rectangle,
    ) -> glam::DVec3 {
        // Model-space input always belongs to a drawing plane. With no active
        // UCS that plane is world XY; using the camera target plane and only
        // clearing Z afterwards shifts the picked point on screen in an
        // oblique view. Plain paper space still uses the camera target plane.
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i]
                .active_cmd
                .as_ref()
                .and_then(|command| command.cursor_plane())
                .or_else(|| {
                    Some(match self.tabs[i].active_ucs.as_ref() {
                        Some(ucs) => (
                            ucs_z_axis(ucs),
                            glam::DVec3::new(ucs.origin.x, ucs.origin.y, ucs.origin.z),
                        ),
                        None => (glam::DVec3::Z, glam::DVec3::ZERO),
                    })
                })
        } else {
            None
        };
        let pick = |cam: &crate::scene::view::camera::Camera| match plane {
            Some((normal, origin)) => cam.pick_on_plane(p, bounds, normal.as_vec3(), origin),
            None => cam.pick_on_target_plane(p, bounds),
        };
        match edit_cam {
            Some(cam) => pick(cam),
            None => {
                let paper = {
                    let c = self.tabs[i].scene.camera.borrow();
                    pick(&c)
                };
                self.tabs[i].scene.paper_to_model(paper)
            }
        }
    }

    /// An acquired planar profile can lie away from the current UCS plane.
    /// Reproject the cursor onto that profile before using it as a drag anchor
    /// or looking for its supporting solid face.
    pub(in crate::app) fn profile_pick_point(
        &self,
        i: usize,
        handle: Handle,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        cursor: iced::Point,
        bounds: iced::Rectangle,
    ) -> Option<glam::DVec3> {
        let entity = self.tabs[i].scene.document.get_entity(handle)?;
        let (plane, _, _) = crate::scene::model::presspull_model::profile_geometry(entity)?;
        let normal = glam::DVec3::from_array(plane.normal()?).as_vec3();
        let origin = glam::DVec3::from_array(plane.origin);
        let point = match edit_cam {
            Some(camera) => camera.pick_on_plane(cursor, bounds, normal, origin),
            None => self.tabs[i]
                .scene
                .camera
                .borrow()
                .pick_on_plane(cursor, bounds, normal, origin),
        };
        point.is_finite().then_some(point)
    }

    /// A deferred solid pick: a face seen edge-on under the cursor first,
    /// since it draws as a line the ray never meets, then the face the ray
    /// meets.
    pub(in crate::app) fn solid_pick_point(
        &self,
        i: usize,
        handle: Handle,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        cursor: iced::Point,
        bounds: iced::Rectangle,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
    ) -> Option<glam::DVec3> {
        let scene = &self.tabs[i].scene;
        let camera = edit_cam
            .clone()
            .unwrap_or_else(|| scene.camera.borrow().clone());
        let view = (camera.rotation * glam::Vec3::NEG_Z).as_dvec3();
        let reach = crate::ui::overlay::pick_box_aperture_px(self.pick_box) as f64
            * camera.ortho_size() as f64
            * 2.0
            / (bounds.height as f64).max(1.0);
        crate::scene::model::presspull_model::edge_on_face_point(
            scene,
            handle,
            camera.pick_on_target_plane(cursor, bounds),
            view,
            reach,
        )
        .or_else(|| scene.solid_click_point_for(cursor, view_rot, eye, bounds, handle))
    }

    // Projection + hit-test wires for the active pane. Inside a floating
    // viewport (`edit_cam` Some) it returns the viewport camera and the live
    // **model** wires, so wire / hatch picking lands on the entity under the
    // cursor exactly where the GPU draws it; otherwise the model/paper camera
    // and the normal hit-test wires. `bounds` is the pane-local rectangle.

    /// Keep only the latest cursor position until a window frame can answer it.
    pub(in crate::app) fn queue_box_selection_preview(&mut self, i: usize, p: Point) {
        let tab = &self.tabs[i];
        let enabled = match tab.active_cmd.as_ref() {
            None => self.model_space.selection_preview & 1 != 0,
            Some(command) => {
                command.is_selection_gathering() && self.model_space.selection_preview & 2 != 0
            }
        };
        let selection = tab.scene.selection.borrow();
        let active = selection.gesture.box_anchor.is_some() || selection.gesture.poly_active;
        drop(selection);
        if enabled && active && !tab.orbit_mode && !tab.pan_mode && !tab.zoom_dynamic_mode {
            self.box_preview_pending = Some((p, i, tab.id));
        } else {
            self.box_preview_pending = None;
            self.box_preview_due = None;
            self.box_preview_cache = None;
            // A Fence / polygon being picked keeps its own preview.
            let path = self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.selection_path().is_some());
            if !path {
                self.tabs[i].scene.set_box_hover_highlights(&[]);
            }
        }
    }

    pub(in crate::app) fn on_box_preview_tick(&mut self) -> Task<Message> {
        let Some((p, i, tab_id)) = self.box_preview_pending else {
            return Task::none();
        };
        if i != self.active_tab || i >= self.tabs.len() || self.tabs[i].id != tab_id || {
            let state = self.tabs[i].scene.selection.borrow();
            state.gesture.box_anchor.is_none() && !state.gesture.poly_active
        } {
            self.box_preview_pending = None;
            self.box_preview_due = None;
            self.box_preview_cache = None;
            if i < self.tabs.len() && self.tabs[i].id == tab_id {
                self.tabs[i].scene.set_box_hover_highlights(&[]);
            }
            return Task::none();
        }
        // A coalesced input position is newer than the pending preview. Wait
        // until the move handler has installed that position instead of
        // briefly drawing a preview for a cursor the user has already left.
        if self.move_owed.is_some_and(|(_, tab)| tab == i)
            || self.box_preview_due.is_some_and(|due| Instant::now() < due)
        {
            return Task::none();
        }
        self.box_preview_pending = None;
        let started = Instant::now();
        self.refresh_box_selection_preview(i, p);
        // Heavy previews yield time to pointer input. A stopped pointer still
        // receives its last queued position on a later frame.
        let cost = started.elapsed();
        let next_frame = started + std::time::Duration::from_millis(16);
        let recovery = Instant::now() + cost.min(std::time::Duration::from_millis(100));
        self.box_preview_due = Some(next_frame.max(recovery));
        Task::none()
    }

    /// SELECTIONPREVIEW while a selection box is drawn (#2172): what the box
    /// would catch with its corner at `p` (canvas pixels) rolls over before
    /// the release commits it, with the release's filters.
    pub(in crate::app) fn refresh_box_selection_preview(&mut self, i: usize, p: iced::Point) {
        let enabled = match self.tabs[i].active_cmd.as_ref() {
            None => self.model_space.selection_preview & 1 != 0,
            Some(command) => {
                command.is_selection_gathering() && self.model_space.selection_preview & 2 != 0
            }
        };
        let navigating =
            self.tabs[i].orbit_mode || self.tabs[i].pan_mode || self.tabs[i].zoom_dynamic_mode;
        let lasso_active = self.tabs[i].scene.selection.borrow().gesture.poly_active;
        if lasso_active && enabled && !navigating {
            self.box_preview_cache = None;
            let scene = &self.tabs[i].scene;
            let canvas_size = scene.selection.borrow().view.vp_size;
            let frame = scene.viewport_edit_frame(canvas_size);
            let tile = frame.as_ref().map_or_else(
                || scene.active_model_tile_bounds(canvas_size.0, canvas_size.1),
                |(_, tile)| *tile,
            );
            let edit_cam = frame.map(|(cam, _)| cam);
            let bounds = iced::Rectangle::new(Point::ORIGIN, tile.size());
            let (mut handles, _) =
                self.lasso_selection_handles(i, &edit_cam, bounds, p, Point::new(tile.x, tile.y));
            handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
            self.filter_handles_reject_locked(i, &mut handles);
            self.tabs[i].scene.set_box_hover_highlights_limited(
                &handles,
                self.model_space.selection_preview_limit as usize,
            );
            return;
        }
        let (anchor, crossing) = {
            let sel = self.tabs[i].scene.selection.borrow();
            let anchor = sel.gesture.box_anchor;
            let crossing = match anchor {
                Some(a) if !sel.gesture.box_crossing_locked => p.x < a.x,
                _ => sel.gesture.box_crossing,
            };
            (anchor, crossing)
        };
        let Some(anchor) = anchor.filter(|_| enabled && !navigating) else {
            self.box_preview_cache = None;
            self.tabs[i].scene.set_box_hover_highlights(&[]);
            return;
        };
        let canvas_sz = self.tabs[i].scene.selection.borrow().view.vp_size;
        let edit_frame = self.tabs[i].scene.viewport_edit_frame(canvas_sz);
        let tile = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i]
                .scene
                .active_model_tile_bounds(canvas_sz.0, canvas_sz.1),
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let local = |point: iced::Point| iced::Point::new(point.x - tile.x, point.y - tile.y);
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, tile.size());
        let (a, p) = (local(anchor), local(p));
        let started = crate::perf::enabled().then(Instant::now);
        let (view, eye, wires) = self.pick_view(i, &edit_cam, bounds);
        let flat = view.z_axis.x.abs() < 1e-9
            && view.z_axis.y.abs() < 1e-9
            && (view.w_axis.w - 1.0).abs() < 1e-6;
        // Only the resident flat model-space path has the complete whole-wire
        // broad-phase guarantees needed to update a previous result safely.
        let eligible = flat
            && edit_cam.is_none()
            && self.tabs[i].scene.current_layout == "Model"
            && self.tabs[i].active_ucs.is_none()
            && self.tabs[i]
                .active_cmd
                .as_ref()
                .is_none_or(|c| c.cursor_plane().is_none());
        let index = eligible
            .then(|| {
                self.tabs[i]
                    .scene
                    .box_preview_interaction_index(&wires, bounds.height)
            })
            .flatten();
        let cache = self.box_preview_cache.take();
        let mut incremental = false;
        let mut affected_count = 0;
        let mut cached_hits = None;
        if let (Some(index), Some(mut cache), Some(new_rect)) =
            (index.as_ref(), cache, box_preview_rect(a, p, bounds))
        {
            let scene = &self.tabs[i].scene;
            let same_source = cache.tab_id == self.tabs[i].id
                && cache.geometry_epoch == scene.geometry_epoch
                && cache.camera_generation == scene.camera_generation
                && cache.anchor == a
                && cache.crossing == crossing
                && cache.bounds == bounds
                && cache.view == view
                && cache.eye == eye
                && Arc::ptr_eq(&cache.wires, &wires)
                && Arc::ptr_eq(&cache.index, index);
            if let Some(old_rect) = same_source
                .then(|| box_preview_rect(cache.anchor, cache.cursor, bounds))
                .flatten()
            {
                let mut bands = Vec::new();
                box_rect_subtract(old_rect, new_rect, &mut bands);
                box_rect_subtract(new_rect, old_rect, &mut bands);
                let changed_area: f32 = bands.iter().map(|(x, y)| (y.x - x.x) * (y.y - x.y)).sum();
                let full_area = (new_rect[2] - new_rect[0]) * (new_rect[3] - new_rect[1]);
                // A large reversal is cheaper and simpler as a full pick.
                if changed_area <= full_area * 0.75 {
                    if let Some(affected) = self.box_preview_affected_handles(
                        i, &edit_cam, bounds, &bands, view, eye, &wires,
                    ) {
                        affected_count = affected.len();
                        let replacement = if bands.is_empty() {
                            Some((Vec::new(), BoxSelectTiming::default()))
                        } else {
                            self.box_selection_handles_filtered(
                                i,
                                &edit_cam,
                                bounds,
                                a,
                                p,
                                crossing,
                                Some(&affected),
                            )
                        };
                        if let Some((hits, timing)) = replacement {
                            if !bands.is_empty() {
                                cache.handles.retain(|handle| !affected.contains(handle));
                                cache.handles.extend(hits);
                            }
                            cached_hits = Some((cache.handles, timing));
                            incremental = true;
                        }
                    }
                }
            }
        }
        let (mut raw_hits, timing) = cached_hits.unwrap_or_else(|| {
            let (hits, timing) = self.box_selection_handles(i, &edit_cam, bounds, a, p, crossing);
            (hits.into_iter().collect(), timing)
        });
        // An opt-in diagnostic checks the optimization against the original
        // full pick on real drawings. The reference result wins on mismatch.
        #[cfg(not(target_arch = "wasm32"))]
        if incremental && std::env::var_os("OCS_VERIFY_BOX_PREVIEW").is_some() {
            let (reference, _) = self.box_selection_handles(i, &edit_cam, bounds, a, p, crossing);
            let reference: rustc_hash::FxHashSet<_> = reference.into_iter().collect();
            if reference != raw_hits {
                eprintln!("[perf] box-preview incremental mismatch; used full result");
                raw_hits = reference;
            } else {
                crate::perf_record!("[perf] box-preview incremental verified");
            }
        }
        let mut handles: Vec<Handle> = raw_hits.iter().copied().collect();
        if let Some(index) = index {
            self.box_preview_cache = Some(crate::app::BoxPreviewCache {
                tab_id: self.tabs[i].id,
                geometry_epoch: self.tabs[i].scene.geometry_epoch,
                camera_generation: self.tabs[i].scene.camera_generation,
                anchor: a,
                cursor: p,
                crossing,
                bounds,
                view,
                eye,
                wires,
                index,
                handles: raw_hits,
                #[cfg(test)]
                incremental,
            });
        }
        handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
        self.filter_handles_reject_locked(i, &mut handles);
        self.tabs[i].scene.set_box_hover_highlights_limited(
            &handles,
            self.model_space.selection_preview_limit as usize,
        );
        let elapsed = crate::perf::elapsed_ms(started);
        if elapsed >= 4.0 {
            crate::perf_record!(
                "[perf] box-preview total={elapsed:.1}ms incremental={incremental} affected={affected_count} candidates={:.1}ms handles={:.1}ms wires={:.1}ms hatch={:.1}ms hit={:.1}ms picked={}",
                timing.candidates, timing.handles, timing.wires, timing.hatch, timing.hit, handles.len(),
            );
        }
    }

    /// Broad-phase entities along the changed edges, plus fills and meshes
    /// that cannot be inferred from wire bounds alone. `None` falls back to
    /// the full, unchanged selection implementation.
    pub(in crate::app) fn box_preview_affected_handles(
        &self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        bounds: iced::Rectangle,
        bands: &[(Point, Point)],
        view: glam::Mat4,
        eye: glam::DVec3,
        wires: &Arc<Vec<crate::scene::WireModel>>,
    ) -> Option<rustc_hash::FxHashSet<Handle>> {
        let scene = &self.tabs[i].scene;
        let mut affected = scene.box_preview_non_wire_handles();
        for &(a, b) in bands {
            let points = [a, Point::new(a.x, b.y), b, Point::new(b.x, a.y)];
            let world_aabb = points
                .map(|p| self.cursor_model_point(i, edit_cam, p, bounds))
                .into_iter()
                .fold(
                    [
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ],
                    |mut aabb, p| {
                        aabb[0] = aabb[0].min(p.x);
                        aabb[1] = aabb[1].min(p.y);
                        aabb[2] = aabb[2].max(p.x);
                        aabb[3] = aabb[3].max(p.y);
                        aabb
                    },
                );
            let rect = [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)];
            let candidates = scene.interaction_box_candidates(
                Arc::clone(wires),
                world_aabb,
                rect,
                view,
                eye,
                bounds,
            );
            // Materializing block stubs turns an indexed result into shared
            // runs, but retains the XY query that produced it.
            if candidates.query_aabb().is_none() || candidates.segments().is_some() {
                return None;
            }
            affected.extend(scene.interaction_candidate_handles(&candidates)?);
        }
        Some(affected)
    }

    /// The entities a selection box from `a` to `p` (tile pixels) catches,
    /// as a window or crossing, through the wire, hatch and mesh tests — the
    /// one box test the drag and click-move-click gestures and automation
    /// share. The selection filter is the caller's.
    pub(in crate::app) fn box_selection_handles(
        &self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        bounds: iced::Rectangle,
        a: iced::Point,
        p: iced::Point,
        crossing: bool,
    ) -> (Vec<Handle>, BoxSelectTiming) {
        self.box_selection_handles_filtered(i, edit_cam, bounds, a, p, crossing, None)
            .expect("an unrestricted box pick always returns a result")
    }

    /// When `only` is supplied, do the existing exact whole-box tests only
    /// for those entities. A source that cannot be restricted without changing
    /// its hit-test semantics signals the caller to perform a full pick.
    pub(in crate::app) fn box_selection_handles_filtered(
        &self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        bounds: iced::Rectangle,
        a: iced::Point,
        p: iced::Point,
        crossing: bool,
        only: Option<&rustc_hash::FxHashSet<Handle>>,
    ) -> Option<(Vec<Handle>, BoxSelectTiming)> {
        let (view_rot, eye, all_wires) = self.pick_view(i, edit_cam, bounds);
        let world_aabb = [a, iced::Point::new(a.x, p.y), p, iced::Point::new(p.x, a.y)]
            .map(|point| self.cursor_model_point(i, edit_cam, point, bounds))
            .into_iter()
            .fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |mut aabb, world| {
                    aabb[0] = aabb[0].min(world.x);
                    aabb[1] = aabb[1].min(world.y);
                    aabb[2] = aabb[2].max(world.x);
                    aabb[3] = aabb[3].max(world.y);
                    aabb
                },
            );
        // A large box selection was slow and two guesses about which step
        // owns the time were both wrong, so each step is timed.
        let elapsed = |start: Option<Instant>| {
            start.map_or(0.0, |start| start.elapsed().as_secs_f64() * 1000.0)
        };
        let mut timing = BoxSelectTiming::default();
        let t_sel = crate::perf::enabled().then(Instant::now);
        let rect = [a.x.min(p.x), a.y.min(p.y), a.x.max(p.x), a.y.max(p.y)];
        let mut area_candidates = self.tabs[i]
            .scene
            .interaction_box_candidates(all_wires, world_aabb, rect, view_rot, eye, bounds);
        timing.candidates = elapsed(t_sel);
        let t_hit = crate::perf::enabled().then(Instant::now);
        let scene = &self.tabs[i].scene;
        let mut candidate_handles = scene.interaction_candidate_handles(&area_candidates);
        if let Some(only) = only {
            if !area_candidates.retain_box_handles(only) {
                return None;
            }
            candidate_handles
                .as_mut()?
                .retain(|handle| only.contains(handle));
        }
        timing.handles = elapsed(t_hit);
        let mut handles: Vec<Handle> =
            scene::pick::hit_test::box_hit(a, p, crossing, &area_candidates, view_rot, eye, bounds)
                .into_iter()
                .filter_map(Scene::handle_from_wire_name)
                .collect();
        timing.wires = elapsed(t_hit);
        handles.extend(scene::pick::hit_test::box_hit_hatch(
            a,
            p,
            crossing,
            &scene.visible_hatches_for_click(candidate_handles.as_ref()),
            view_rot,
            eye,
            bounds,
            candidate_handles.as_ref(),
        ));
        handles.extend(scene::pick::hit_test::box_hit_insert_hatch(
            a,
            p,
            crossing,
            scene.insert_hatches_for_click().as_ref(),
            view_rot,
            eye,
            bounds,
            candidate_handles.as_ref(),
        ));
        timing.hatch = elapsed(t_hit);
        handles.extend(scene.mesh_box_hit(
            a,
            p,
            crossing,
            view_rot,
            eye,
            bounds,
            candidate_handles.as_ref(),
        ));
        handles.extend(scene.block_mesh_box_hit(
            a,
            p,
            crossing,
            view_rot,
            eye,
            bounds,
            candidate_handles.as_ref(),
        ));
        // Reference-only entities (API v9) are plugin-owned display geometry:
        // a box/window selection must not pick them.
        handles.retain(|h| !scene.is_reference_only(*h));
        if let Some(only) = only {
            // Mesh pickers currently scan all solid instances; restrict their
            // results too, so the cache only replaces affected entities.
            handles.retain(|handle| only.contains(handle));
        }
        timing.hit = elapsed(t_hit);
        Some((handles, timing))
    }

    pub(in crate::app) fn pick_view(
        &self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        bounds: iced::Rectangle,
    ) -> (
        glam::Mat4,
        glam::DVec3,
        std::sync::Arc<Vec<crate::scene::WireModel>>,
    ) {
        match edit_cam {
            Some(cam) => {
                let wires = match self.tabs[i].scene.active_viewport {
                    Some(h) => self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(h, bounds.height),
                    None => self.tabs[i].scene.hit_test_wires(),
                };
                (cam.view_proj_rte(bounds), cam.eye(), wires)
            }
            None => {
                let (view_rot, eye) = {
                    let c = self.tabs[i].scene.camera.borrow();
                    (c.view_proj_rte(bounds), c.eye())
                };
                (view_rot, eye, self.tabs[i].scene.hit_test_wires())
            }
        }
    }

    /// Discrete snaps against the original geometry retained by STRETCH.
    /// These stationary reference wires are separate from the moving overlay:
    /// only endpoints/vertices/midpoints/etc. are eligible, so a nearest or
    /// tangent snap cannot pull the cursor continuously onto its old outline.
    pub(in crate::app) fn command_original_snap_hit(
        &self,
        i: usize,
        screen: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Option<crate::snap::SnapResult> {
        let originals = self.tabs[i]
            .active_cmd
            .as_ref()?
            .preview_original_snap_wires();
        if originals.is_empty() {
            return None;
        }
        self.snapper
            .snap_discrete_preview(screen, originals, view_rot, eye, bounds)
            // Do not capture the STRETCH base point at zero displacement:
            // it would hold the moving cursor at its starting position.
            .filter(|hit| {
                self.last_point
                    .is_none_or(|base| (hit.world - base).length_squared() > 1e-16)
            })
    }

    /// Filters out handles on locked layers if the active command rejects them,
    /// reporting the dropped count to the command line.
    pub(in crate::app) fn filter_handles_reject_locked(
        &mut self,
        i: usize,
        handles: &mut Vec<Handle>,
    ) {
        if !self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.selection_rejects_locked_layers())
        {
            return;
        }
        let (kept, dropped): (Vec<Handle>, Vec<Handle>) = handles
            .drain(..)
            .partition(|h| !self.tabs[i].scene.is_layer_locked(*h));
        *handles = kept;
        if !dropped.is_empty() {
            let count = dropped.len();
            self.command_line.push_info(&if count == 1 {
                "1 was on a locked layer.".to_string()
            } else {
                format!("{count} were on a locked layer.")
            });
        }
    }

    pub(in crate::app) fn grip_edit_for_hit(
        &mut self,
        i: usize,
        handle: Handle,
        grip_id: usize,
        is_translate: bool,
        world: glam::DVec3,
    ) -> GripEdit {
        let axis = self.tabs[i]
            .selected_grip_handles
            .iter()
            .copied()
            .zip(self.tabs[i].selected_grips.iter())
            .find(|(owner, grip)| *owner == handle && grip.id == grip_id)
            .and_then(|(_, grip)| grip.axis);

        // A move-gizmo part moves its cloud alone, along its axis or plane.
        let gizmo_shape = self.tabs[i]
            .selected_grip_handles
            .iter()
            .copied()
            .zip(self.tabs[i].selected_grips.iter())
            .find(|(owner, grip)| *owner == handle && grip.id == grip_id)
            .map(|(_, grip)| grip.shape)
            .filter(|shape| {
                matches!(
                    shape,
                    crate::scene::model::object::GripShape::GizmoAxis(_)
                        | crate::scene::model::object::GripShape::GizmoPlane(_)
                )
            });
        if let Some(shape) = gizmo_shape {
            self.tabs[i].hot_grips.clear();
            let mut edit = GripEdit::single(handle, grip_id, true, world);
            edit.axis = axis;
            edit.gizmo = true;
            if let crate::scene::model::object::GripShape::GizmoPlane(k) = shape {
                let (a, b) = crate::scene::pick::grip::gizmo_plane_axes(k);
                edit.plane = Some(
                    crate::scene::pick::grip::gizmo_axis(a)
                        .cross(crate::scene::pick::grip::gizmo_axis(b)),
                );
            }
            return edit;
        }

        let clicked_is_hot = self.tabs[i].hot_grips.contains(&(handle, grip_id));

        let mut targets: Vec<GripTarget> = if clicked_is_hot {
            // Explicit hot grips keep their existing multi-grip behaviour.
            self.tabs[i]
                .selected_grip_handles
                .iter()
                .copied()
                .zip(self.tabs[i].selected_grips.iter())
                .filter(|(owner, grip)| self.tabs[i].hot_grips.contains(&(*owner, grip.id)))
                .filter(|(_, grip)| {
                    grip.id != crate::app::visibility::VIS_GRIP_ID
                        && grip.id != crate::app::visibility::TABLE_GRIP_ID
                        && !crate::app::block_actions::is_test_grip(grip.id)
                        && !crate::app::block_authoring_edit::is_editor_grip(grip.id)
                        && !crate::scene::assoc_array_grips::is_array_grip(grip.id)
                })
                .map(|(owner, grip)| GripTarget {
                    handle: owner,
                    grip_id: grip.id,
                    is_translate: grip.is_midpoint,
                    last_world: grip.world,
                })
                .collect()
        } else {
            // A normal click on coincident grips stretches all selected grips that
            // share the same geometric point. Use a tiny world-space tolerance:
            // visually-near grips caused by zoom must remain independent.
            self.tabs[i].hot_grips.clear();

            const COINCIDENT_GRIP_EPSILON: f64 = 1.0e-9;
            let epsilon_sq = COINCIDENT_GRIP_EPSILON * COINCIDENT_GRIP_EPSILON;

            self.tabs[i]
                .selected_grip_handles
                .iter()
                .copied()
                .zip(self.tabs[i].selected_grips.iter())
                .filter(|(_, grip)| {
                    grip.id != crate::app::visibility::VIS_GRIP_ID
                        && grip.id != crate::app::visibility::TABLE_GRIP_ID
                        && !crate::app::block_actions::is_test_grip(grip.id)
                        && !crate::app::block_authoring_edit::is_editor_grip(grip.id)
                        && !crate::scene::assoc_array_grips::is_array_grip(grip.id)
                })
                .filter(|(_, grip)| (grip.world - world).length_squared() <= epsilon_sq)
                .map(|(owner, grip)| GripTarget {
                    handle: owner,
                    grip_id: grip.id,
                    is_translate: grip.is_midpoint,
                    last_world: grip.world,
                })
                .collect()
        };

        // A midpoint/centre grip translates its whole entity. If that entity also
        // has coincident point grips, applying both would move it twice; one
        // translate target owns that entity in that case.
        let translate_handles: rustc_hash::FxHashSet<_> = targets
            .iter()
            .filter(|target| target.is_translate)
            .map(|target| target.handle)
            .collect();

        let mut used_translates = rustc_hash::FxHashSet::default();

        targets.retain(|target| {
            if translate_handles.contains(&target.handle) {
                target.is_translate && used_translates.insert(target.handle)
            } else {
                true
            }
        });

        if targets.is_empty() {
            let mut edit = GripEdit::single(handle, grip_id, is_translate, world);
            edit.axis = axis;
            return edit;
        }

        // An axis constraint belongs to a single grip. When several coincident
        // grips participate, let the common cursor delta drive all of them.
        let axis = (targets.len() == 1).then_some(axis).flatten();

        // A circle's quadrant grip edits its radius, as the grip menu's
        // Radius does: Dynamic Input shows and takes the radius.
        let radius = matches!(
            targets.as_slice(),
            [target] if !target.is_translate
                && (1..=4).contains(&target.grip_id)
                && matches!(
                    self.tabs[i].scene.document.get_entity(target.handle),
                    Some(document::EntityType::Circle(_))
                )
        );

        GripEdit {
            handle,
            grip_id,
            origin_world: world,
            last_world: world,
            mode: if radius {
                GripEditMode::Radius
            } else {
                GripEditMode::Stretch
            },
            axis,
            targets,
            rectangle_frame: None,
            plane: None,
            grab: None,
            gizmo: false,
        }
    }

    pub(in crate::app) fn update_grip_hover(&mut self, i: usize, p: iced::Point) {
        const HOVER_OPEN_MS: u128 = 1_000;
        const POPUP_DISMISS_PX: f32 = 80.0;
        // API v11: plugin-declared custom grips hover-highlight through the
        // same pipeline — and, per the always-visible policy, they hover
        // without any selection. No hover while any drag (built-in or
        // custom) or command runs.
        #[cfg(not(target_arch = "wasm32"))]
        if self.tabs[i].active_plugin_grip.is_some()
            || self.tabs[i].active_grip.is_some()
            || self.tabs[i].active_cmd.is_some()
        {
            self.plugin_grip_hover = None;
        } else {
            self.plugin_grip_hover = self.hit_plugin_grip(i, p).map(|candidate| {
                crate::plugin::grip_points::PluginGripHover {
                    plugin: candidate.plugin,
                    grip_id: candidate.grip_id,
                }
            });
        }
        if self.tabs[i].active_cmd.is_some()
            || self.tabs[i].active_grip.is_some()
            || self.tabs[i].selected_grips.is_empty()
        {
            self.grip_hover = None;
            self.grip_popup = None;
            return;
        }
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: vw,
            height: vh,
        };
        let is_paper = self.tabs[i].scene.current_layout != "Model";
        // In-viewport grips are model-space — project with the viewport camera
        // at the viewport's own rect; the cursor is mapped into that rect.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let hit = if self.tabs[i].active_block_edit_session().is_some()
            && self.tabs[i].selected_authoring.is_some()
        {
            self.authoring_grip_hit(p, bounds)
        } else if let Some((cam, full)) = &edit_frame {
            let local = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: full.width,
                height: full.height,
            };
            let p_local = iced::Point::new(p.x - full.x, p.y - full.y);
            find_hit_grip_rte(
                p_local,
                &self.tabs[i].selected_grips,
                cam.view_proj_rte(local),
                cam.eye(),
                local,
            )
        } else if is_paper {
            let cam = self.tabs[i].scene.camera.borrow();
            let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
            let half_h = cam.ortho_size();
            let half_w = half_h * aspect;
            let tx = cam.target.x as f32;
            let ty = cam.target.y as f32;
            drop(cam);
            find_hit_grip_paper(
                p,
                &self.tabs[i].selected_grips,
                tx,
                ty,
                half_w,
                half_h,
                bounds,
            )
        } else {
            let cam = self.tabs[i].scene.camera.borrow();
            find_hit_grip(p, &self.tabs[i].selected_grips, &cam, bounds)
        };
        match hit {
            Some((grip_index, grip_id, _, _)) => {
                let Some(&handle) = self.tabs[i].selected_grip_handles.get(grip_index) else {
                    self.grip_hover = None;
                    self.grip_popup = None;
                    return;
                };
                let same = self.grip_hover.as_ref().is_some_and(|h| {
                    h.document == self.tabs[i].id && h.handle == handle && h.grip_id == grip_id
                });
                if !same {
                    self.grip_hover = Some(crate::app::GripHover {
                        document: self.tabs[i].id,
                        handle,
                        grip_id,
                        screen: p,
                        started: iced::time::Instant::now(),
                    });
                    if !self.grip_popup.as_ref().is_some_and(|popup| popup.pinned) {
                        self.grip_popup = None;
                    }
                } else if let Some(h) = self.grip_hover.as_mut() {
                    h.screen = p;
                }
                // Open popup once dwell crosses the threshold. The visibility
                // grip has its own click-to-open dropdown, so it gets no
                // hover grip-menu.
                if self.grip_popup.is_none()
                    && grip_id != crate::app::visibility::VIS_GRIP_ID
                    && grip_id != crate::app::visibility::TABLE_GRIP_ID
                    && !crate::app::block_actions::is_test_grip(grip_id)
                    && !crate::app::block_authoring_edit::is_editor_grip(grip_id)
                    && self
                        .grip_hover
                        .as_ref()
                        .is_some_and(|h| h.started.elapsed().as_millis() >= HOVER_OPEN_MS)
                {
                    let entity_opt = self.tabs[i].scene.document.get_entity(handle);
                    if let Some(e) = entity_opt {
                        use crate::entities::traits::EntityTypeOps;
                        let items = e.grip_menu(grip_id);
                        if !items.is_empty() {
                            let selected = items
                                .iter()
                                .position(|item| item.label.starts_with('✓'))
                                .unwrap_or(0);
                            self.grip_popup = Some(crate::app::GripPopup {
                                handle,
                                grip_id,
                                anchor: p,
                                items,
                                selected,
                                pinned: false,
                            });
                        }
                    }
                }
            }
            None => {
                self.grip_hover = None;
                if let Some(popup) = self.grip_popup.as_ref().filter(|popup| !popup.pinned) {
                    let dx = p.x - popup.anchor.x;
                    let dy = p.y - popup.anchor.y;
                    if (dx * dx + dy * dy).sqrt() > POPUP_DISMISS_PX {
                        self.grip_popup = None;
                    }
                }
            }
        }
    }

}
