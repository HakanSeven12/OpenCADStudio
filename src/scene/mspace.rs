// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

impl Scene {
    // ── MSPACE helpers ───────────────────────────────────────────────────

    /// Convert a **paper-space** world coordinate to **model-space** using the
    /// geometry of the currently active viewport.  Returns the input unchanged
    /// when there is no active viewport.
    /// Convert a paper-space point to model space (precise at UTM scale).
    pub fn paper_to_model(&self, paper_pt: glam::DVec3) -> glam::DVec3 {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return paper_pt,
        };
        let vp = match self.document.get_entity(vp_handle) {
            Some(acadrust::EntityType::Viewport(vp)) => vp,
            _ => return paper_pt,
        };
        // Uses the viewport's own `view_target` — kept valid by
        // `normalize_active_viewport_view` on entry, which folds a stale UTM
        // saved view onto the auto-fit centre so the display, pan/zoom and this
        // inverse all agree. Cheap (no per-call camera rebuild).
        let scale = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
        if scale.abs() < 1e-9 {
            return paper_pt;
        }
        let tx = vp.view_target.x;
        let ty = vp.view_target.y;
        let pcx = vp.center.x;
        let pcy = vp.center.y;
        glam::DVec3::new(
            (paper_pt.x - pcx) / scale + tx,
            (paper_pt.y - pcy) / scale + ty,
            paper_pt.z,
        )
    }

    /// Inverse of [`paper_to_model`]: map a model-space point to the paper
    /// sheet through the active viewport. Returns the input unchanged when
    /// there is no active viewport. Kept as the inverse companion to
    /// `paper_to_model`; in-viewport overlays now project via the viewport
    /// camera ([`viewport_edit_frame`]) rather than mapping onto the sheet.
    #[allow(dead_code)]
    pub fn model_to_paper(&self, model_pt: glam::DVec3) -> glam::DVec3 {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return model_pt,
        };
        let vp = match self.document.get_entity(vp_handle) {
            Some(acadrust::EntityType::Viewport(vp)) => vp,
            _ => return model_pt,
        };
        let scale = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
        glam::DVec3::new(
            (model_pt.x - vp.view_target.x) * scale + vp.center.x,
            (model_pt.y - vp.view_target.y) * scale + vp.center.y,
            model_pt.z,
        )
    }

    /// In-viewport (MSPACE) editing frame: the active floating viewport's own
    /// camera — *exactly* the one the GPU renders its content with
    /// ([`camera_for_viewport`]) — together with the viewport's full screen
    /// rectangle in canvas pixels ([`viewport_screen_rect`]).
    ///
    /// This is the unified editing adapter (the "süzgeç"). Inside a viewport,
    /// editing IS model-space: treat the returned camera as *the* camera, the
    /// returned rect as *the* pane, and the cursor relative to that rect — then
    /// the existing model-space snap / hit-test / grip / preview / plane-pick
    /// code runs unchanged and lands on the same pixels the GPU draws. Results
    /// come back as model coordinates directly (no paper round-trip).
    ///
    /// Because the camera is the real GPU camera, this tracks the viewport's
    /// pan / zoom / twist / oblique view correctly — unlike a linear
    /// paper-projection, whose auto-fit / saved-view / crop divergence left the
    /// snap stale after pan/zoom. Returns `None` when not editing inside a
    /// floating viewport, or the camera / rect cannot be derived.
    pub fn viewport_edit_frame(
        &self,
        canvas_px: (f32, f32),
    ) -> Option<(view::camera::Camera, iced::Rectangle)> {
        self.viewport_edit_frame_for(self.active_viewport?, canvas_px)
    }

    /// Camera and canvas rectangle for snapping through an inactive viewport.
    pub fn viewport_edit_frame_for(
        &self,
        vp_handle: Handle,
        canvas_px: (f32, f32),
    ) -> Option<(view::camera::Camera, iced::Rectangle)> {
        let cam = self.camera_for_viewport(vp_handle)?;
        let full = self.viewport_screen_rect(vp_handle, canvas_px)?;
        Some((cam, full))
    }

    /// Planar paper/model mapping derived from the viewport's display camera.
    /// Oblique and perspective views have no supported planar mapping.
    pub fn viewport_frame(
        &self,
        vp_handle: Handle,
    ) -> Option<crate::scene::viewport_ref::ViewportFrame> {
        use crate::scene::viewport_ref::ViewportFrame;

        let (paper_center, vp_height, locked) = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => (
                glam::DVec2::new(vp.center.x, vp.center.y),
                vp.height,
                vp.status.locked,
            ),
            _ => return None,
        };
        if !paper_center.is_finite() || !vp_height.is_finite() || vp_height < 1e-9 {
            return None;
        }
        let cam = self.camera_for_viewport(vp_handle)?;
        if cam.projection != view::camera::Projection::Orthographic {
            return None;
        }
        // Screen right/up of the viewport camera, in model space. A planar
        // frame exists only when both stay in the model XY plane and form a
        // right-handed pair (a plan view, possibly twisted). Anything else is
        // an oblique 3-D view: unsupported.
        let right = (cam.rotation * glam::Vec3::X).as_dvec3();
        let up = (cam.rotation * glam::Vec3::Y).as_dvec3();
        if !right.is_finite() || !up.is_finite() || right.z.abs() > 1e-6 || up.z.abs() > 1e-6 {
            return None;
        }
        let normal = right.cross(up);
        if normal.z < 1.0 - 1e-6 {
            return None; // mirrored or degenerate
        }
        // Model units visible across the viewport's height -> paper per model.
        let model_height = cam.ortho_size() as f64 * 2.0;
        if !model_height.is_finite() || model_height < 1e-12 {
            return None;
        }
        let scale = vp_height / model_height;
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        // `ViewportFrame` rotates model geometry by `twist` (CCW) before
        // scaling; the row that produces the paper X component is `right`, so
        // cos(twist) = right.x and sin(twist) = -right.y.
        let twist = (-right.y).atan2(right.x);
        Some(ViewportFrame {
            viewport: vp_handle,
            paper_center,
            model_target: glam::DVec2::new(cam.target.x as f64, cam.target.y as f64),
            scale,
            twist,
            locked,
        })
    }

    /// Visible, supported content viewports at a sheet point, topmost first.
    /// Shared by snapping and explicit dimension object picking.
    pub fn viewport_frames_at_paper_point(
        &self,
        paper: glam::DVec3,
    ) -> Vec<crate::scene::viewport_ref::ViewportFrame> {
        if self.current_layout == "Model" || self.active_viewport.is_some() {
            return Vec::new();
        }
        self.layout_content_viewports()
            .iter()
            .rev()
            .filter_map(|&handle| {
                if !self.viewport_displays_content(handle)
                    || !self.viewport_displays_paper_point(handle, paper.truncate())
                {
                    return None;
                }
                self.viewport_frame(handle)
            })
            .collect()
    }

    /// Test the viewport rectangle and optional nonrectangular clip boundary.
    pub fn viewport_displays_paper_point(&self, vp_handle: Handle, paper: glam::DVec2) -> bool {
        let Some(EntityType::Viewport(vp)) = self.document.get_entity(vp_handle) else {
            return false;
        };
        let hw = (vp.width * 0.5).abs();
        let hh = (vp.height * 0.5).abs();
        if hw < 1e-9 || hh < 1e-9 {
            return false;
        }
        if (paper.x - vp.center.x).abs() > hw || (paper.y - vp.center.y).abs() > hh {
            return false;
        }
        if vp.clip_boundary_handle.is_null() {
            return true;
        }
        let Some(boundary) = self.document.get_entity(vp.clip_boundary_handle)
            .and_then(crate::entities::curve::entity_curve)
        else {
            return false;
        };
        let Some(normal) = boundary.plane.normal() else {
            return false;
        };
        if !boundary.curve.is_closed() || normal[0].abs() > 1e-9 || normal[1].abs() > 1e-9 {
            return false;
        }
        let Some(point) = boundary.plane.project([paper.x, paper.y, vp.center.z]) else {
            return false;
        };
        cadkernel::geom2d::contains(
            &[boundary.curve],
            point,
            cadkernel::geom2d::Tolerance::new(1e-9),
        )
    }
}

#[cfg(test)]
mod clip_tests {
    use super::*;

    #[test]
    fn concave_clip_boundary_rejects_the_notch() {
        let mut scene = Scene::new();
        let mut poly = acadrust::entities::LwPolyline::from_points(
            [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (5.0, 5.0), (5.0, 10.0), (0.0, 10.0)]
                .map(|(x, y)| acadrust::types::Vector2::new(x, y))
                .to_vec(),
        );
        poly.is_closed = true;
        let clip = scene.add_entity(EntityType::LwPolyline(poly));
        let mut viewport = acadrust::entities::Viewport::new();
        viewport.center = acadrust::types::Vector3::new(5.0, 5.0, 0.0);
        viewport.width = 20.0;
        viewport.height = 20.0;
        viewport.clip_boundary_handle = clip;
        let viewport = scene.add_entity(EntityType::Viewport(viewport));
        for (x, y, inside) in [
            (2.0, 2.0, true), (8.0, 2.0, true), (8.0, 8.0, false),
            (-1.0, 5.0, false), (5.0, 7.0, true),
        ] {
            assert_eq!(
                scene.viewport_displays_paper_point(viewport, glam::DVec2::new(x, y)), inside,
            );
        }
    }
}

impl Scene {

    /// Fold the active viewport's saved view onto the effective camera (the
    /// auto-fit centre for stale UTM views) and persist it into `view_target` /
    /// `view_height`. Called on entering MSPACE so pan/zoom, paper↔model and the
    /// rendered content all share one valid view — otherwise a stale `(0,0,0)`
    /// target left the camera auto-fitting to the model centre while the cursor
    /// math used the origin, and pan toggled the two (jitter).
    pub fn normalize_active_viewport_view(&mut self) {
        let Some(vp_handle) = self.active_viewport else {
            return;
        };
        let Some(cam) = self.camera_for_viewport(vp_handle) else {
            return;
        };
        let eff_h = cam.ortho_size() as f64 * 2.0;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            vp.view_target.x = cam.target.x;
            vp.view_target.y = cam.target.y;
            vp.view_center.x = 0.0;
            vp.view_center.y = 0.0;
            if eff_h > 1e-9 {
                vp.view_height = eff_h;
            }
        }
        self.notify_viewport_changed(vp_handle);
    }

    /// Fit model-space bounds into the active floating viewport without moving
    /// the surrounding paper-space camera.
    pub fn fit_active_viewport_to_bounds(
        &mut self,
        min: glam::Vec3,
        max: glam::Vec3,
    ) -> bool {
        let Some(viewport_handle) = self.active_viewport else {
            return false;
        };
        let (width, height, locked) = match self.document.get_entity(viewport_handle) {
            Some(acadrust::EntityType::Viewport(viewport)) => {
                (viewport.width, viewport.height, viewport.status.locked)
            }
            _ => return false,
        };
        if locked {
            return false;
        }
        let Some(mut camera) = self.camera_for_viewport(viewport_handle) else {
            return false;
        };
        camera.fit_to_bounds(min, max, (width / height.max(1e-9)) as f32);
        if let Some(acadrust::EntityType::Viewport(viewport)) =
            self.document.get_entity_mut(viewport_handle)
        {
            viewport.view_target.x = camera.target.x;
            viewport.view_target.y = camera.target.y;
            viewport.view_target.z = camera.target.z;
            viewport.view_center.x = 0.0;
            viewport.view_center.y = 0.0;
            viewport.view_height = camera.ortho_size() as f64 * 2.0;
            if viewport.view_height > 1e-9 {
                viewport.custom_scale = viewport.height / viewport.view_height;
            }
        }
        self.notify_viewport_changed(viewport_handle);
        self.camera_generation += 1;
        true
    }

    /// Pan the active viewport's model-space view by `(screen_dx, screen_dy)` pixels.
    /// The delta is converted to model-space units using the camera and viewport scale.
    /// No-op when there is no active viewport.
    pub fn pan_active_viewport(&mut self, screen_dx: f32, screen_dy: f32, bounds: iced::Rectangle) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        // Use the viewport's own camera for the pan axes (matches 3-D view orientation).
        let vp_cam = match self.camera_for_viewport(vp_handle) {
            Some(c) => c,
            None => return,
        };

        // Read viewport dims (immutable borrow ends here).
        let (view_height, vp_height, locked) = match self.document.get_entity(vp_handle) {
            Some(acadrust::EntityType::Viewport(vp)) => {
                (vp.view_height as f32, vp.height as f32, vp.status.locked)
            }
            _ => return,
        };
        if locked {
            return;
        }

        // Correct pan speed: how many model units correspond to one screen pixel.
        //
        // The paper camera's ortho_size() gives the visible paper-space half-height
        // (in paper mm). One screen pixel = 2*half_h / canvas_height paper mm.
        // Inside the viewport, one paper mm = view_height / vp_height model units.
        // Together: model_per_pixel = (2*half_h / canvas_height) * (view_height / vp_height)
        let paper_half_h = self.camera.borrow().ortho_size();
        let speed = if bounds.height > 0.0 && paper_half_h > 1e-6 && vp_height > 1e-6 {
            (2.0 * paper_half_h / bounds.height) * (view_height / vp_height)
        } else {
            vp_cam.distance * 0.001
        };

        let cam_right = vp_cam.rotation * glam::Vec3::X;
        let cam_up = vp_cam.rotation * glam::Vec3::Y;
        let model_delta = -(cam_right * screen_dx * speed) + (cam_up * screen_dy * speed);

        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            vp.view_target.x += model_delta.x as f64;
            vp.view_target.y += model_delta.y as f64;
            vp.view_target.z += model_delta.z as f64;
        }
        self.notify_viewport_changed(vp_handle);
    }

    /// Zoom the active viewport's model-space view by `steps` notches.
    /// Positive = zoom in (increase detail), negative = zoom out.
    /// `cursor_paper`: optional paper-space XY of the cursor; when supplied the
    /// model point under the cursor is kept stationary (AutoCAD-style zoom).
    /// No-op when there is no active viewport.
    pub fn zoom_active_viewport(&mut self, steps: f32, cursor_paper: Option<glam::Vec2>) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return;
            }
            // Zoom in = shrink view_height → higher scale → objects appear larger.
            let factor = (1.0_f64 - 0.15 * steps as f64).clamp(0.1, 10.0);

            if let Some(cp) = cursor_paper {
                // Compute the model-space point under the cursor before zoom.
                let scale_before =
                    vp_effective_scale(vp.custom_scale, vp.view_height, vp.height) as f32;
                let cx = vp.center.x as f32;
                let cy = vp.center.y as f32;
                let tx = vp.view_target.x as f32;
                let ty = vp.view_target.y as f32;
                let mx = (cp.x - cx) / scale_before + tx;
                let my = (cp.y - cy) / scale_before + ty;

                // Apply zoom.
                vp.view_height = (vp.view_height * factor).max(1e-6);
                if vp.view_height.abs() > 1e-9 {
                    vp.custom_scale = vp.height / vp.view_height;
                }
                let scale_after = vp.custom_scale as f32;

                // Adjust view_target so the model point under cursor stays there.
                let mx_after = (cp.x - cx) / scale_after + vp.view_target.x as f32;
                let my_after = (cp.y - cy) / scale_after + vp.view_target.y as f32;
                vp.view_target.x += (mx - mx_after) as f64;
                vp.view_target.y += (my - my_after) as f64;
            } else {
                vp.view_height = (vp.view_height * factor).max(1e-6);
                if vp.view_height.abs() > 1e-9 {
                    vp.custom_scale = vp.height / vp.view_height;
                }
            }
        }
        self.notify_viewport_changed(vp_handle);
    }

    /// Orbit the active viewport's view direction by the given screen-pixel delta.
    /// No-op when there is no active viewport or it is locked.
    pub fn orbit_active_viewport(&mut self, delta_x: f32, delta_y: f32) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        let mut cam = match self.camera_for_viewport(vp_handle) {
            Some(c) => c,
            None => return,
        };
        // Floating viewport orbits about its own target (no selection pivot).
        cam.orbit(delta_x, delta_y, None);
        // yaw_pitch_to_quat(y,p)*Z = (cos(p)*sin(y), -cos(p)*cos(y), sin(p))
        // `camera_for_viewport` reconstructs the rotation so that
        // `rotation * Z == view_direction` exactly (its `yaw = atan2(x, -y)`
        // cancels the sign). Store `eye` directly so the orbit round-trips —
        // negating Y here made each drag step read back a Y-mirrored camera,
        // flipping the model between a rotation and its opposite every frame.
        let eye = cam.rotation * glam::Vec3::Z;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return;
            }
            vp.view_direction.x = eye.x as f64;
            vp.view_direction.y = eye.y as f64;
            vp.view_direction.z = eye.z as f64;
        }
    }

    /// Snap the active viewport's view direction to `eye_dir` (unit
    /// vector from target toward camera). Twist angle is left at its
    /// current value so the up-sense is preserved across successive
    /// snaps. No-op when there is no active viewport or it is locked.
    pub fn snap_active_viewport_to_direction(&mut self, eye_dir: glam::Vec3, ucs: glam::Mat4) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        // Build the full UCS-aligned orientation exactly as the model snap does
        // (snap_to_direction picks the in-plane roll from the UCS axes), seeded
        // from the viewport's current camera so the "best up" stays stable, then
        // decode it back to the stored (view_direction, twist_angle). Writing
        // only view_direction loses the roll and the rebuilt camera snaps to
        // WCS-up instead of the UCS the clicked cube was drawn in.
        let mut tmp = self.camera_for_viewport(vp_handle).unwrap_or_default();
        tmp.snap_to_direction(eye_dir, ucs);
        let dir = (tmp.rotation * glam::Vec3::Z).normalize_or(glam::Vec3::Z);
        let desired_up = (tmp.rotation * glam::Vec3::Y).normalize_or(glam::Vec3::Y);

        // camera_from_view rebuilds the rotation with its OWN yaw convention
        // (atan2(x, -y)) and applies roll = -twist, which is *not* the camera's
        // internal yaw/roll convention — so `-tmp.roll()` does not round-trip.
        // Instead reproduce the decoder's zero-twist basis here, then measure
        // the signed roll about the view axis that carries its up onto the
        // desired UCS up. Store twist = -roll (the decoder negates it back).
        let pitch = dir.z.clamp(-1.0, 1.0).asin();
        let yaw = if dir.x.abs() < 1e-6 && dir.y.abs() < 1e-6 {
            0.0
        } else {
            dir.x.atan2(-dir.y)
        };
        let up0 = (view::camera::yaw_pitch_to_quat(yaw, pitch, 0.0) * glam::Vec3::Y)
            .normalize_or(glam::Vec3::Y);
        let roll = up0.cross(desired_up).dot(dir).atan2(up0.dot(desired_up));
        let twist = -roll as f64;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return;
            }
            vp.view_direction.x = dir.x as f64;
            vp.view_direction.y = dir.y as f64;
            vp.view_direction.z = dir.z as f64;
            vp.twist_angle = twist;
        }
        self.notify_viewport_changed(vp_handle);
    }

    /// Mutate the active viewport's camera through a closure, then re-encode the
    /// result to the stored `(view_direction, twist_angle)` — the same decode
    /// the ViewCube snap uses. Lets the home / roll / nudge controls drive a
    /// floating viewport just like the model camera. Returns `false` if there is
    /// no active (unlocked) viewport.
    pub fn mutate_active_viewport_camera(
        &mut self,
        f: impl FnOnce(&mut view::camera::Camera),
    ) -> bool {
        let Some(vp_handle) = self.active_viewport else {
            return false;
        };
        let mut tmp = self.camera_for_viewport(vp_handle).unwrap_or_default();
        f(&mut tmp);
        let dir = (tmp.rotation * glam::Vec3::Z).normalize_or(glam::Vec3::Z);
        let desired_up = (tmp.rotation * glam::Vec3::Y).normalize_or(glam::Vec3::Y);
        let pitch = dir.z.clamp(-1.0, 1.0).asin();
        let yaw = if dir.x.abs() < 1e-6 && dir.y.abs() < 1e-6 {
            0.0
        } else {
            dir.x.atan2(-dir.y)
        };
        let up0 = (view::camera::yaw_pitch_to_quat(yaw, pitch, 0.0) * glam::Vec3::Y)
            .normalize_or(glam::Vec3::Y);
        let roll = up0.cross(desired_up).dot(dir).atan2(up0.dot(desired_up));
        let twist = -roll as f64;
        let mut changed = false;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return false;
            }
            vp.view_direction.x = dir.x as f64;
            vp.view_direction.y = dir.y as f64;
            vp.view_direction.z = dir.z as f64;
            vp.twist_angle = twist;
            vp.status.perspective =
                tmp.projection == view::camera::Projection::Perspective;
            vp.lens_length =
                (12.0 / (tmp.fov_y * 0.5).tan().max(1e-6)) as f64;
            changed = true;
        }
        if changed {
            self.notify_viewport_changed(vp_handle);
        }
        changed
    }

    /// Render mode of the active paper-space viewport, or `None` when no
    /// viewport is active (PSPACE / model layout).
    pub fn active_viewport_render_mode(
        &self,
    ) -> Option<acadrust::entities::ViewportRenderMode> {
        let h = self.active_viewport?;
        match self.document.get_entity(h) {
            Some(acadrust::EntityType::Viewport(vp)) => Some(vp.render_mode),
            _ => None,
        }
    }

    /// Set the active paper-space viewport's render mode. Returns `true`
    /// when a viewport was active and updated; `false` (no-op) otherwise,
    /// so the caller can fall back to the model-layout render mode.
    pub fn set_active_viewport_render_mode(
        &mut self,
        mode: acadrust::entities::ViewportRenderMode,
    ) -> bool {
        let Some(h) = self.active_viewport else {
            return false;
        };
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(h) {
            vp.render_mode = mode;
            true
        } else {
            false
        }
    }

    /// Visual style of the active Model tile (for the render-mode picker).
    pub fn active_model_tile_render_mode(
        &self,
    ) -> acadrust::entities::ViewportRenderMode {
        let tiles = self.model_tiles.borrow();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        tiles
            .get(active)
            .map(|t| t.render_mode)
            .unwrap_or(acadrust::entities::ViewportRenderMode::Wireframe2D)
    }

    /// Set only the active Model tile's render mode. Other tiles keep theirs.
    pub fn set_active_model_tile_render_mode(
        &self,
        mode: acadrust::entities::ViewportRenderMode,
    ) {
        let mut tiles = self.model_tiles.borrow_mut();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        if let Some(t) = tiles.get_mut(active) {
            t.render_mode = mode;
        }
    }

    /// Current gaze direction (canonical +Z eye dir, world space) of whichever
    /// camera owns the ViewCube — the active floating viewport in MSPACE, else
    /// the main camera. Used by the ViewCube "already there → flip to opposite"
    /// check, which must test the camera the cube actually reflects (the paper
    /// camera always looks straight down, so testing it flipped every snap).
    pub fn active_gaze_dir(&self) -> glam::Vec3 {
        if let Some(h) = self.active_viewport {
            if let Some(cam) = self.camera_for_viewport(h) {
                return cam.rotation * glam::Vec3::Z;
            }
        }
        self.camera.borrow().rotation * glam::Vec3::Z
    }

    pub fn active_camera_rotation(&self) -> glam::Quat {
        if let Some(handle) = self.active_viewport {
            if let Some(camera) = self.camera_for_viewport(handle) {
                return camera.rotation;
            }
        }
        self.camera.borrow().rotation
    }

    /// Camera owning navigation, including an entered paper-space viewport.
    pub(crate) fn navigation_camera(&self) -> view::camera::Camera {
        self.active_viewport
            .and_then(|h| self.camera_for_viewport(h))
            .unwrap_or_else(|| self.camera.borrow().clone())
    }

    pub(crate) fn navigation_locked(&self) -> bool {
        self.active_viewport.is_some_and(|h| {
            !matches!(self.document.get_entity(h), Some(acadrust::EntityType::Viewport(v)) if !v.status.locked)
        })
    }

    /// Commit an externally navigated camera through the same floating-view
    /// encoding as the ViewCube. Camera coordinates already include view_center.
    pub(crate) fn apply_navigation_camera(&mut self, camera: view::camera::Camera) -> bool {
        if self.navigation_locked() {
            return false;
        }
        if let Some(handle) = self.active_viewport {
            if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(handle) {
                vp.view_target.x = camera.target.x;
                vp.view_target.y = camera.target.y;
                vp.view_target.z = camera.target.z;
                vp.view_center.x = 0.;
                vp.view_center.y = 0.;
                vp.view_height = camera.ortho_size() as f64 * 2.;
                vp.custom_scale = vp.height / vp.view_height;
            }
            // Notify associations only after all camera fields are persisted.
            if !self.mutate_active_viewport_camera(|c| *c = camera.clone()) {
                return false;
            }
        } else {
            *self.camera.borrow_mut() = camera;
        }
        self.camera_generation += 1;
        true
    }

    pub fn active_camera_projection(&self) -> view::camera::Projection {
        if let Some(handle) = self.active_viewport {
            if let Some(camera) = self.camera_for_viewport(handle) {
                return camera.projection;
            }
        }
        self.camera.borrow().projection
    }

    /// View-rotation matrix for the active viewport (MSPACE), or the
    /// paper-space camera's matrix when not in MSPACE.
    /// Used by ViewCube hit-testing so clicks map to the correct camera.
    pub fn active_view_rotation_mat(&self) -> glam::Mat4 {
        // Must match exactly what the drawn cube uses (see ViewportData's
        // `cam_rotation`): the active context's camera composed with the
        // ViewCube UCS. Inside a floating viewport that's the viewport's own
        // camera; the UCS factor applies in both cases.
        if let Some(h) = self.active_viewport {
            if let Some(cam) = self.camera_for_viewport(h) {
                return cam.view_rotation_mat() * self.viewcube_ucs_mat();
            }
        }
        self.camera.borrow().view_rotation_mat() * self.viewcube_ucs_mat()
    }

    /// The UCS→world rotation the ViewCube should compose with the camera —
    /// the active UCS in model space, identity everywhere else. Render,
    /// hit-test, and click-snap all go through this so they stay in lock-step.
    pub fn viewcube_ucs_mat(&self) -> glam::Mat4 {
        // UCS applies in model space and inside a floating viewport (MSPACE);
        // plain paper space stays WCS.
        if self.current_layout == "Model" || self.active_viewport.is_some() {
            self.viewcube_ucs
        } else {
            glam::Mat4::IDENTITY
        }
    }

    /// Return the handle of the user viewport whose *visible* on-screen
    /// rectangle (clamped to the canvas) contains the given screen-pixel point.
    /// Viewport activation goes through this so a click only enters a viewport
    /// when it lands on the part the user can actually see — clicking the empty
    /// area beside a viewport that runs off-screen no longer matches its full
    /// (partly off-canvas) paper rect and switches to it by mistake.
    pub fn viewport_at_screen_point(
        &self,
        px: f32,
        py: f32,
        canvas: (f32, f32),
    ) -> Option<Handle> {
        let (_, _, handles) = self.paper_viewport_handles();
        handles
            .iter()
            .filter_map(|handle| {
                let Some(EntityType::Viewport(vp)) = self.document.get_entity(*handle) else {
                    return None;
                };
                if !vp.status.is_on {
                    return None;
                }
                let rect = self.viewport_screen_rect(vp.common.handle, canvas)?;
                let x0 = rect.x.max(0.0);
                let y0 = rect.y.max(0.0);
                let x1 = (rect.x + rect.width).min(canvas.0);
                let y1 = (rect.y + rect.height).min(canvas.1);
                if x1 <= x0 || y1 <= y0 {
                    return None; // fully off-canvas → nothing to click
                }
                if px >= x0 && px <= x1 && py >= y0 && py <= y1 {
                    Some((vp.common.handle, (x1 - x0) * (y1 - y0)))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(h, _)| h)
    }

    /// Content (floating) viewports of the current paper layout, in draw order
    /// (the last entry draws on top). Empty in the Model layout.
    pub fn layout_content_viewports(&self) -> Arc<Vec<Handle>> {
        if self.current_layout == "Model" {
            return Arc::new(Vec::new());
        }
        let (_, _, handles) = self.paper_viewport_handles();
        handles
    }

    /// `true` when this viewport's contents are displayed (VP ON). The border
    /// layer's visibility deliberately does NOT matter: a viewport whose frame
    /// layer is off still shows its model content, so it still snaps.
    pub fn viewport_displays_content(&self, vp_handle: Handle) -> bool {
        matches!(
            self.document.get_entity(vp_handle),
            Some(EntityType::Viewport(vp)) if vp.status.is_on
        )
    }

    /// Return the handle of the first active user viewport in the current layout,
    /// or `None` if there are none.  Used by the MS command.
    pub fn first_user_viewport(&self) -> Option<Handle> {
        let (_, _, handles) = self.paper_viewport_handles();
        handles.iter().find_map(|handle| {
            let Some(EntityType::Viewport(vp)) = self.document.get_entity(*handle) else {
                return None;
            };
            if vp.status.is_on {
                Some(vp.common.handle)
            } else {
                None
            }
        })
    }
}
