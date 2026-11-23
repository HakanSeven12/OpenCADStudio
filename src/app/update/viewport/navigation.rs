use super::*;
use super::super::util::*;

impl OpenCADStudio {
    /// Screen positions of the UCS-icon grips (origin + axis tips, absolute px)
    /// for the active pane, or `None` when the icon is not shown / not anchored
    /// at its on-screen origin / a command is active. The single source of truth
    /// for both hover and grip hit-testing, so they match what is drawn.
    pub(in crate::app) fn ucs_icon_hit_info(
        &self,
        i: usize,
        vw: f32,
        vh: f32,
    ) -> Option<crate::ui::overlay::UcsIconHit> {
        if !self.show_ucs_icon
            || self.tabs[i].active_cmd.is_some()
            || self.tabs[i].active_block_edit_session().is_some()
        {
            return None;
        }
        let tab = &self.tabs[i];
        let (_, ux, uy, uz) = tab.ucs_xform().axes();
        // Project through whichever pane owns the icon — a floating viewport's
        // own camera, else the active model tile. Bare paper space shows none.
        let (cam, bounds) = if let Some((c, full)) = tab.scene.viewport_edit_frame((vw, vh)) {
            (c, full)
        } else if tab.scene.current_layout == "Model" {
            (
                tab.scene.camera.borrow().clone(),
                tab.scene.active_model_tile_bounds(vw, vh),
            )
        } else {
            return None;
        };
        // Mirror the draw: anchor at the projected origin only when ORigin mode
        // is on AND the origin is on-screen; otherwise `None` parks it in the
        // corner — still selectable/draggable there.
        let os = if self.ucs_icon_at_origin {
            cam.project(tab.ucs_origin_world(), bounds)
                .map(|q| Point::new(bounds.x + q.x, bounds.y + q.y))
        } else {
            None
        };
        crate::ui::overlay::ucs_icon_hit(
            cam.view_proj_rte(bounds),
            bounds,
            (ux.as_vec3(), uy.as_vec3(), uz.as_vec3()),
            os,
        )
    }

    /// The object snap the cursor would get over `world` in the model view,
    /// through the same candidate gathering and snapper as a cursor move
    /// (automation's `snap` query). `wires` limits it to those wires (the
    /// object under a typed point, say) instead of the drawing's candidates.
    pub(in crate::app) fn snap_query(
        &mut self,
        i: usize,
        world: glam::DVec3,
        from: Option<glam::DVec3>,
        wires: Option<&Vec<crate::scene::model::wire_model::WireModel>>,
    ) -> Option<crate::snap::SnapResult> {
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        if vw <= 1.0 || vh <= 1.0 {
            return None;
        }
        let tile_b = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: tile_b.width,
            height: tile_b.height,
        };
        let (view_rot, eye, px) = {
            let cam = self.tabs[i].scene.camera.borrow();
            (
                cam.view_proj_rte(bounds),
                cam.eye(),
                cam.project(world, bounds)?,
            )
        };
        let p = Point::new(px.x, px.y);
        let raw = self.cursor_model_point(i, &None, p, bounds);
        // Perpendicular and tangent measure from `from`, else the running
        // command's last point; the live cursor's base point is restored.
        let live_from = self.snapper.from_point;
        self.snapper.from_point = from.or(self.last_point);
        let (go, gr) = self.drafting_grid_basis(i);
        let hit = match wires {
            Some(wires) => self
                .snapper
                .snap(raw, p, wires, view_rot, eye, bounds, go, gr, None),
            None => {
                let all_wires = self.tabs[i].scene.hit_test_wires();
                let candidates = self.tabs[i].scene.interaction_candidates_near(
                    all_wires,
                    raw,
                    view_rot,
                    eye,
                    bounds,
                    self.snapper.osnap_radius_px,
                );
                self.snapper
                    .snap(raw, p, &candidates, view_rot, eye, bounds, go, gr, None)
            }
        };
        self.snapper.from_point = live_from;
        hit
    }

    /// Apply one frame of a UCS-icon grip drag: map the cursor onto the active
    /// UCS plane (so the move stays in-plane) and either slide the origin there
    /// or rotate the chosen axis to point at it, keeping a right-handed frame
    /// with Z fixed. Live — the commit (persist) happens on release.
    pub(in crate::app) fn drag_ucs_grip(&mut self, i: usize, kind: crate::app::UcsGripKind, p_full: Point) {
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        if vw <= 1.0 || vh <= 1.0 {
            return;
        }
        // Same pane framing as the rest of the move handler: pane-local cursor
        // and origin-zero bounds, with the floating-viewport camera when inside
        // one. `cursor_model_point` handles the paper→model round-trip.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let tile_b = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = Point::new(p_full.x - tile_b.x, p_full.y - tile_b.y);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: tile_b.width,
            height: tile_b.height,
        };
        // The Z grip turns the UCS plane itself, so its cursor lies on a
        // plane through the origin facing the view instead.
        let raw = if kind == crate::app::UcsGripKind::ZAxis {
            let origin = self.tabs[i].ucs_origin_world();
            let pick = |cam: &crate::scene::view::camera::Camera| {
                let facing = (cam.eye() - cam.target).as_vec3();
                cam.pick_on_plane(p, bounds, facing, origin)
            };
            match &edit_cam {
                Some(cam) => pick(cam),
                None => {
                    let paper = pick(&self.tabs[i].scene.camera.borrow());
                    self.tabs[i].scene.paper_to_model(paper)
                }
            }
        } else {
            self.cursor_model_point(i, &edit_cam, p, bounds)
        };

        // Object/grid snap, same path as an entity grip or command drag: the
        // dragged UCS point sticks to endpoints/midpoints/grid under the cursor,
        // and the snap marker is published via `snap_result`.
        let (dgx, dgy, dadaptive) = (self.grid_spacing_x, self.grid_spacing_y, self.grid_adaptive);
        let dvisible_step = |distance: f32, fov_y: f32| {
            let (sx, sy) = crate::ui::overlay::compute_grid_steps(
                dgx, dgy, distance, fov_y, bounds, dadaptive,
            );
            sx.max(sy)
        };
        let (view_rot, eye, grid_spacing) = match &edit_cam {
            Some(cam) => (
                cam.view_proj_rte(bounds),
                cam.eye(),
                dvisible_step(cam.distance, cam.fov_y),
            ),
            None => {
                let cam = self.tabs[i].scene.camera.borrow();
                (
                    cam.view_proj_rte(bounds),
                    cam.eye(),
                    dvisible_step(cam.distance, cam.fov_y),
                )
            }
        };
        let all_wires = if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport)
        {
            self.tabs[i]
                .scene
                .model_wires_for_viewport_arc(h, bounds.height)
        } else {
            self.tabs[i].scene.hit_test_wires()
        };
        let snap_candidates = self.tabs[i].scene.interaction_candidates_near(
            all_wires,
            raw,
            view_rot,
            eye,
            bounds,
            self.snapper.osnap_radius_px,
        );
        self.snapper.grid_spacing = grid_spacing;
        // No rubber-band origin (perp/extension feet don't apply to a free drag).
        self.snapper.from_point = None;
        let (go, gr) = self.drafting_grid_basis(i);
        self.sync_osnap_z_plane(i);
        let snap_hit = self.snapper.snap(
            raw,
            p,
            &snap_candidates,
            view_rot,
            eye,
            bounds,
            go,
            gr,
            None,
        );
        let world = snap_hit.map(|s| s.world).unwrap_or(raw);
        self.tabs[i].snap_result = snap_hit;
        if let Some(s) = self.tabs[i].snap_result.as_mut() {
            snap_pixels_to_canvas(s, tile_b);
        }

        use document::types::Vector3;
        let v3 = |d: glam::DVec3| Vector3::new(d.x, d.y, d.z);
        {
            let ucs = self.tabs[i]
                .active_ucs
                .get_or_insert_with(|| document::tables::Ucs::new("*ACTIVE*"));
            match kind {
                crate::app::UcsGripKind::Origin => {
                    ucs.origin = v3(world);
                }
                crate::app::UcsGripKind::XAxis | crate::app::UcsGripKind::YAxis => {
                    let o = glam::dvec3(ucs.origin.x, ucs.origin.y, ucs.origin.z);
                    let x = glam::dvec3(ucs.x_axis.x, ucs.x_axis.y, ucs.x_axis.z);
                    let y = glam::dvec3(ucs.y_axis.x, ucs.y_axis.y, ucs.y_axis.z);
                    let z = x.cross(y).normalize_or(glam::DVec3::Z);
                    // Direction from origin to cursor, flattened into the UCS
                    // plane; bail on a degenerate (cursor on the origin).
                    let dir = world - o;
                    let d = dir - z * dir.dot(z);
                    if d.length_squared() < 1e-12 {
                        return;
                    }
                    let d = d.normalize();
                    // Z fixed; the dragged axis = d, the other = right-handed
                    // completion (z×x = y, y×z = x).
                    let (nx, ny) = match kind {
                        crate::app::UcsGripKind::XAxis => (d, z.cross(d)),
                        _ => (d.cross(z), d),
                    };
                    ucs.x_axis = v3(nx);
                    ucs.y_axis = v3(ny);
                }
                crate::app::UcsGripKind::ZAxis => {
                    let o = glam::dvec3(ucs.origin.x, ucs.origin.y, ucs.origin.z);
                    let x = glam::dvec3(ucs.x_axis.x, ucs.x_axis.y, ucs.x_axis.z);
                    let y = glam::dvec3(ucs.y_axis.x, ucs.y_axis.y, ucs.y_axis.z);
                    let Some(nz) = (world - o).try_normalize() else {
                        return;
                    };
                    // X keeps what it can of itself; when Z turns onto it,
                    // the old Y takes its place.
                    let Some(nx) = (x - nz * x.dot(nz))
                        .try_normalize()
                        .or_else(|| (y - nz * y.dot(nz)).try_normalize())
                    else {
                        return;
                    };
                    ucs.x_axis = v3(nx);
                    ucs.y_axis = v3(nz.cross(nx));
                }
            }
        }
        self.tabs[i].sync_ucs_to_scene();
        self.tabs[i].scene.camera_generation += 1;
    }

    /// Commit the live UCS to its owning space. Model/viewport panes persist
    /// their normal UCS fields. A BEDIT pane has no persistent UCS of its own:
    /// its transient origin and axes are baked into the block definition, then
    /// the editor returns to canonical local coordinates.
    pub(in crate::app) fn commit_active_ucs_change(&mut self, i: usize, label: &'static str) {
        if let Some((session_index, block_record)) =
            self.tabs[i].active_block_edit.and_then(|index| {
                self.tabs[i]
                    .block_edits
                    .get(index)
                    .map(|session| (index, session.br_handle))
            })
        {
            let frame = self.tabs[i].ucs_xform();
            if !frame.is_identity() {
                self.push_undo_snapshot(i, label);
                let local_from_old = frame.to_ucs_transform();
                let changed = self.tabs[i]
                    .scene
                    .reframe_block_definition(block_record, &local_from_old);
                if changed > 0 {
                    // Carry the editor camera through the same rigid transform
                    // so changing the block coordinate frame does not make its
                    // contents jump on screen.
                    self.tabs[i]
                        .scene
                        .camera
                        .borrow_mut()
                        .apply_rigid_transform(&local_from_old);
                    self.tabs[i].scene.camera_generation += 1;
                    self.tabs[i].dirty = true;
                }
            }

            // Blocks do not own a saved UCS. The chosen frame has now become
            // their identity coordinates.
            self.tabs[i].active_ucs = None;
            let editor_camera = self.tabs[i].scene.camera.borrow().clone();
            if let Some(session) = self.tabs[i].block_edits.get_mut(session_index) {
                session.editor_ucs = None;
                session.editor_camera = editor_camera;
            }
            self.tabs[i].sync_ucs_to_scene();
            return;
        }

        let persisted = if let Some(handle) = self.tabs[i].scene.active_viewport {
            self.tabs[i].ucs_from_viewport(handle)
        } else if self.tabs[i].scene.current_layout == "Model" {
            self.tabs[i].model_ucs_from_header()
        } else {
            None
        };
        let same_basis =
            |a: Option<&document::tables::Ucs>, b: Option<&document::tables::Ucs>| match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.origin == b.origin
                        && a.x_axis == b.x_axis
                        && a.y_axis == b.y_axis
                        && a.handle == b.handle
                        && a.name.eq_ignore_ascii_case(&b.name)
                }
                _ => false,
            };
        let changed = !same_basis(self.tabs[i].active_ucs.as_ref(), persisted.as_ref());
        if changed {
            self.push_undo_snapshot(i, label);
            self.tabs[i].persist_active_ucs();
            self.tabs[i].dirty = true;
        }
        self.tabs[i].sync_ucs_to_scene();
        if changed && self.tabs[i].scene.ucs_follow() {
            let rotation = self.tabs[i].scene.viewcube_ucs_mat();
            self.plan_snap(i, rotation);
        }
    }

    // ── Per-pane Model viewport (pane_grid) ───────────────────────────────

    /// Focus Model pane `idx` (cursor entered it): swap in its camera, sync the
    /// render-mode / grid display. No-op if already active.
    pub(in crate::app) fn focus_model_pane(&mut self, idx: usize) {
        let i = self.active_tab;
        if self.tabs[i].scene.set_active_model_tile(idx) {
            self.tabs[i].scene.camera_generation += 1;
            self.sync_render_mode_to_active_tile(i);
            self.adopt_view_display(i);
        }
    }

}
