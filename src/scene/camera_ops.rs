// Camera / view operations on `Scene`: zoom, fit-all, named-view restore,
// camera<->document sync, and per-tile grid/snap. (The actual screen-space
// hit-testing lives in `scene::pick::hit_test`.)
use super::*;

fn rendered_wire_center(wires: &[WireModel], fallback_z: f64) -> Option<glam::DVec3> {
    let mut min = glam::DVec3::splat(f64::INFINITY);
    let mut max = glam::DVec3::splat(f64::NEG_INFINITY);
    let mut include = |point: glam::DVec3| {
        if point.is_finite() {
            min = min.min(point);
            max = max.max(point);
        }
    };

    for wire in wires {
        let mut has_precise_position = false;
        for (index, &[x, y, z]) in wire.points.iter().enumerate() {
            let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
            include(glam::DVec3::new(
                x as f64 + low[0] as f64,
                y as f64 + low[1] as f64,
                z as f64 + low[2] as f64,
            ));
            has_precise_position = true;
        }
        for vertex in &wire.text_verts {
            include(glam::DVec3::new(
                vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
                vertex.pos[2] as f64 + vertex.pos_low[2] as f64,
            ));
            has_precise_position = true;
        }
        for &[x, y, z] in &wire.key_vertices {
            include(glam::DVec3::new(x, y, z));
            has_precise_position = true;
        }
        for &(point, _) in &wire.snap_pts {
            include(point);
            has_precise_position = true;
        }

        if !has_precise_position {
            let [x0, y0, x1, y1] = wire.aabb;
            if wire.aabb != WireModel::UNBOUNDED_AABB
                && [x0, y0, x1, y1].iter().all(|value| value.is_finite())
                && x0 <= x1
                && y0 <= y1
            {
                include(glam::DVec3::new(x0 as f64, y0 as f64, fallback_z));
                include(glam::DVec3::new(x1 as f64, y1 as f64, fallback_z));
            }
        }
    }

    if min.is_finite() && max.is_finite() {
        Some((min + max) * 0.5)
    } else {
        None
    }
}

fn block_entity_transform(
    document: &CadDocument,
    block_name: &str,
    target: Handle,
    visited: &mut Vec<String>,
) -> Option<acadrust::types::Transform> {
    if visited
        .iter()
        .any(|name| name.eq_ignore_ascii_case(block_name))
    {
        return None;
    }
    let record = document
        .block_records
        .iter()
        .find(|record| record.name.eq_ignore_ascii_case(block_name))?;
    if record.entity_handles.contains(&target) {
        return Some(acadrust::types::Transform::identity());
    }

    visited.push(record.name.clone());
    let transform = record.entity_handles.iter().find_map(|handle| {
        let EntityType::Insert(insert) = document.get_entity(*handle)? else {
            return None;
        };
        let inner =
            block_entity_transform(document, &insert.block_name, target, visited)?;
        Some(inner.then(&insert.get_transform()))
    });
    visited.pop();
    transform
}

/// The model-space active viewport is reserved as `*Active`, but that name is
/// case-insensitive in DXF/DWG — a file may store it as `*ACTIVE`. Match it
/// accordingly, otherwise an uppercased record reads as a *distinct* viewport:
/// on save it survives the "drop the old *Active" filter and sits next to the
/// fresh entry we add, so the file gets two overlapping full-window viewports
/// that render "one on top of the other". (#319)
fn is_active_vport_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("*Active")
}

impl Scene {
    // ── Hit-test convenience: wire name → Handle ──────────────────────────

    pub fn handle_from_wire_name(name: &str) -> Option<Handle> {
        name.parse::<u64>().ok().map(Handle::new)
    }

    /// Restore camera to a named view from the document view table.
    pub fn restore_named_view(&mut self, view: &acadrust::tables::View) {
        use glam::Vec3;
        let cam = &mut *self.camera.borrow_mut();
        // view.target is the look-at point; view.direction is eye→target direction.
        cam.target = glam::DVec3::new(view.target.x, view.target.y, view.target.z);
        // direction in acadrust = from-target-to-eye (same as AutoCAD convention).
        let eye_dir = Vec3::new(
            view.direction.x as f32,
            view.direction.y as f32,
            view.direction.z as f32,
        );
        let eye_dir = if eye_dir.length_squared() > 1e-10 {
            eye_dir.normalize()
        } else {
            Vec3::Z
        };
        // Build rotation: canonical eye is +Z, rotate to eye_dir.
        cam.rotation = glam::Quat::from_rotation_arc(Vec3::Z, eye_dir);
        // Sync yaw/pitch from new rotation (for ViewCube).
        let pitch = eye_dir.z.clamp(-1.0, 1.0).asin();
        let yaw = eye_dir.x.atan2(eye_dir.y);
        cam.yaw = yaw;
        cam.pitch = pitch;
        // Derive distance from view height and fov.
        let h = view.height as f32;
        cam.distance = if h > 0.0 {
            h / (2.0 * (cam.fov_y * 0.5).tan())
        } else {
            cam.distance
        };
        self.camera_generation += 1;
    }

    /// Save the current camera state into a new named view entry.
    /// Returns the view; caller must push it into document.views.
    pub fn current_as_named_view(&self, name: &str) -> acadrust::tables::View {
        use acadrust::types::Vector3;
        let cam = self.camera.borrow();
        let eye_dir = cam.rotation * glam::Vec3::Z;
        let height = cam.ortho_size() * 2.0;
        let width = height; // caller can adjust; rough square
        let mut view = acadrust::tables::View::new(name);
        view.center = Vector3 {
            x: cam.target.x as f64,
            y: cam.target.y as f64,
            z: 0.0,
        };
        view.target = Vector3 {
            x: cam.target.x as f64,
            y: cam.target.y as f64,
            z: cam.target.z as f64,
        };
        view.direction = Vector3 {
            x: eye_dir.x as f64,
            y: eye_dir.y as f64,
            z: eye_dir.z as f64,
        };
        view.height = height as f64;
        view.width = width as f64;
        view.lens_length = 50.0;
        view.front_clip = 0.0;
        view.back_clip = 0.0;
        view.twist_angle = 0.0;
        // OCS saves an orthographic model view, not a perspective camera.
        view.perspective = false;
        view
    }

    /// Zoom the model-space camera in/out by a percentage.
    /// factor > 1 = zoom out, factor < 1 = zoom in.
    pub fn zoom_camera(&mut self, factor: f32) {
        let mut cam = self.camera.borrow_mut();
        cam.distance = (cam.distance * factor).max(0.001);
        drop(cam);
        self.camera_generation += 1;
    }

    /// Refresh model bounds without moving the live camera. Reusing `fit_all`
    /// keeps projection framing on the same visibility and outlier filters as
    /// Zoom Extents instead of accepting every finite cached AABB.
    fn refresh_projection_bounds(&mut self) {
        if self.current_layout != "Model"
            || self.active_viewport.is_some()
            || (self.projection_bounds_epoch.get() == self.geometry_epoch
                && self.camera.borrow().fitted_model_bounds().is_some())
        {
            return;
        }

        let saved_camera = self.camera.borrow().clone();
        let saved_generation = self.camera_generation;
        self.camera.borrow_mut().model_bounds = None;
        self.fit_all();
        let refreshed = self.camera.borrow().fitted_model_bounds();
        *self.camera.borrow_mut() = saved_camera;
        self.camera_generation = saved_generation;

        if let Some((min, max)) = refreshed {
            self.camera.borrow_mut().fit_depth_to_bounds(min, max);
            self.projection_bounds_epoch.set(self.geometry_epoch);
        }
    }

    /// Switch the live camera projection without changing its target or
    /// orientation. Model layout bounds are refreshed first so perspective eye
    /// distance accounts for the current, visible drawing depth.
    pub fn set_projection_preserving_frame(
        &mut self,
        projection: view::camera::Projection,
    ) -> bool {
        if self.camera.borrow().projection == projection {
            return false;
        }

        self.refresh_projection_bounds();

        let aspect = self.active_camera_aspect();
        self.camera
            .borrow_mut()
            .set_projection_preserving_frame(projection, aspect);
        self.camera_generation += 1;
        true
    }

    /// Aspect ratio of the active camera's actual render rectangle. Model
    /// panes are separate shader widgets, so the full-canvas render aspect is
    /// wrong whenever a tiled viewport is active.
    pub(super) fn active_camera_aspect(&self) -> f32 {
        if self.current_layout == "Model" {
            let (canvas_w, canvas_h) = self.selection.borrow().vp_size;
            let tiles = self.model_tiles.borrow();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            if let Some(tile) = tiles.get(active) {
                let width = tile.rect.width * canvas_w;
                let height = tile.rect.height * canvas_h;
                if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
                    return width / height;
                }
            }
        }
        self.last_render_aspect.get().max(0.01)
    }

    /// Fit the camera to a world-space bounding box (corners p1, p2).
    pub fn zoom_to_window(&mut self, p1: glam::Vec3, p2: glam::Vec3) {
        let min = p1.min(p2);
        let max = p1.max(p2);
        if min == max {
            return;
        }
        let aspect = self.active_camera_aspect();
        self.camera
            .borrow_mut()
            .fit_to_bounds(min, max, aspect);
        self.projection_bounds_epoch.set(self.geometry_epoch);
        self.camera_generation += 1;
    }

    /// Centre the active camera on one rendered entity without changing zoom
    /// distance, orientation, or viewport scale.
    pub fn center_camera_on_entity(&mut self, handle: Handle) -> bool {
        let fallback_z = self.active_camera_target().z;
        let wires = self.wire_models_for(&[handle]);
        let Some(center) = rendered_wire_center(&wires, fallback_z) else {
            return false;
        };
        self.center_active_camera_on(center)
    }

    /// Centre on one concrete attribute value attached to an INSERT rather
    /// than on the complete block extents.
    pub fn center_camera_on_insert_attribute(&mut self, insert: Handle, index: usize) -> bool {
        let (attribute, fallback) = {
            let Some(EntityType::Insert(entity)) = self.document.get_entity(insert) else {
                return false;
            };
            let Some(attribute) = entity.attributes.get(index) else {
                return false;
            };
            (
                EntityType::AttributeEntity(attribute.clone()),
                glam::DVec3::new(
                    attribute.insertion_point.x,
                    attribute.insertion_point.y,
                    attribute.insertion_point.z,
                ),
            )
        };
        let wires = self.tessellate_one(&attribute);
        let center = rendered_wire_center(&wires, fallback.z).unwrap_or(fallback);
        self.center_active_camera_on(center)
    }

    /// Centre on a text entity stored in a block definition, transformed
    /// through the concrete visible INSERT occurrence that FIND is visiting.
    pub fn center_camera_on_block_entity(&mut self, insert: Handle, entity: Handle) -> bool {
        let (source, mut transform) = {
            let Some(EntityType::Insert(insert_entity)) = self.document.get_entity(insert) else {
                return false;
            };
            let Some(source) = self.document.get_entity(entity) else {
                return false;
            };
            let mut visited = Vec::new();
            let Some(inner) = block_entity_transform(
                &self.document,
                &insert_entity.block_name,
                entity,
                &mut visited,
            ) else {
                return false;
            };
            (
                source.clone(),
                inner.then(&insert_entity.get_transform()),
            )
        };

        if (self.annotation_scale - 1.0).abs() > 1e-6 {
            let Some(EntityType::Insert(insert_entity)) = self.document.get_entity(insert) else {
                return false;
            };
            if insert_entity
                .common
                .extended_data
                .get_record("AcAnnotativeData")
                .is_some()
            {
                let point = insert_entity.insert_point;
                let scale_about =
                    acadrust::types::Transform::from_translation(acadrust::types::Vector3::new(
                        -point.x, -point.y, -point.z,
                    ))
                    .then(&acadrust::types::Transform::from_scale(
                        self.annotation_scale as f64,
                    ))
                    .then(&acadrust::types::Transform::from_translation(
                        acadrust::types::Vector3::new(point.x, point.y, point.z),
                    ));
                transform = transform.then(&scale_about);
            }
        }

        let fallback_z = self.active_camera_target().z;
        let wires = self.tessellate_one(&source);
        let Some(local_center) = rendered_wire_center(&wires, fallback_z) else {
            return false;
        };
        let world = transform.apply(acadrust::types::Vector3::new(
            local_center.x,
            local_center.y,
            local_center.z,
        ));
        self.center_active_camera_on(glam::DVec3::new(world.x, world.y, world.z))
    }

    fn active_camera_target(&self) -> glam::DVec3 {
        self.active_viewport
            .and_then(|handle| self.camera_for_viewport(handle))
            .map_or_else(|| self.camera.borrow().target, |camera| camera.target)
    }

    fn center_active_camera_on(&mut self, center: glam::DVec3) -> bool {
        if !center.is_finite() {
            return false;
        }
        if let Some(handle) = self.active_viewport {
            let Some(EntityType::Viewport(viewport)) = self.document.get_entity_mut(handle) else {
                return false;
            };
            if viewport.status.locked {
                return false;
            }
            viewport.view_target.x = center.x;
            viewport.view_target.y = center.y;
            viewport.view_target.z = center.z;
        } else {
            self.camera.borrow_mut().target = center;
        }
        self.camera_generation += 1;
        true
    }

    /// Apply camera state from an acadrust View table entry, through the shared
    /// `camera_from_view` decoder so the twist round-trips like every other
    /// saved view. `model_space`: if true, subtracts world_offset from target
    /// (wire-space); paper-space entries carry no offset.
    fn apply_camera_from_view_entry(
        &mut self,
        view: &acadrust::tables::View,
        model_space: bool,
    ) -> bool {
        let _ = model_space;
        let Some(cam) = self.camera_from_view_mode(
            view.direction,
            view.target,
            acadrust::types::Vector2 {
                x: view.center.x,
                y: view.center.y,
            },
            view.height,
            view.twist_angle,
            view.perspective,
            view.lens_length,
        ) else {
            return false;
        };
        *self.camera.borrow_mut() = cam;
        self.camera_generation += 1;
        true
    }

    /// Set the model-space camera from the VPORT table's *Active entry.
    /// Returns true if the entry was found and the camera was set.
    fn apply_active_vport_camera(&mut self) -> bool {
        // Restore the single tile's visual style + grid/snap from the *Active
        // entry, independent of where the camera itself comes from below.
        if let Some(vp) = self.document.vports.iter().find(|v| is_active_vport_name(&v.name)) {
            let mode = vp.render_mode;
            let (grid_on, snap_on) = (vp.grid_on, vp.snap_on);
            let mut tiles = self.model_tiles.borrow_mut();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            if let Some(t) = tiles.get_mut(active) {
                t.render_mode = mode;
                t.grid_on = grid_on;
                t.snap_on = snap_on;
            }
        }
        // Restore from the standard *Active VPORT entry. (Earlier builds also
        // wrote an app-specific "OpenCADStudio_Camera_Model" View record and
        // preferred it here — that polluted the file for other CAD programs and
        // is no longer written or read; the view round-trips fine via VPORT.)
        let vp = match self.document.vports.iter().find(|v| is_active_vport_name(&v.name)) {
            Some(v) => v.clone(),
            None => return false,
        };
        let Some(new_cam) = self.camera_from_vport(&vp) else {
            return false;
        };
        *self.camera.borrow_mut() = new_cam;
        self.camera_generation += 1;
        true
    }

    /// Decode a saved view into a `Camera`. This is the single shared decoder
    /// for both a model-space VPORT table entry (tiled) and a paper-space
    /// VIEWPORT entity (floating): tiled vs floating only changes *where* the
    /// fields come from and the floating auto-fit fallback — the projection
    /// math (view direction → yaw/pitch, twist → roll, view_center fold,
    /// view_height → distance) is identical, so it lives here once. Callers pass
    /// their already-effective `view_target` / `view_center` / `view_height`.
    ///
    /// Returns `None` for a zero `view_height` (an uninitialised entry).
    pub(super) fn camera_from_view_mode(
        &self,
        view_direction: acadrust::types::Vector3,
        view_target: acadrust::types::Vector3,
        view_center: acadrust::types::Vector2,
        view_height: f64,
        twist: f64,
        perspective: bool,
        lens_length: f64,
    ) -> Option<Camera> {
        if view_height.abs() < 1e-9 {
            return None;
        }
        let vd = glam::Vec3::new(
            view_direction.x as f32,
            view_direction.y as f32,
            view_direction.z as f32,
        )
        .normalize_or(glam::Vec3::Z);
        let pitch = vd.z.clamp(-1.0, 1.0).asin();
        // view_dir = (sin(yaw)*cos(pitch), -cos(yaw)*cos(pitch), sin(pitch))
        // → yaw = atan2(x, -y), but when looking straight up/down cos(pitch)≈0
        //   both x and y are near zero and atan2(0, -0.0) = π due to IEEE 754.
        let yaw = if vd.x.abs() < 1e-6 && vd.y.abs() < 1e-6 {
            0.0_f32 // plan/nadir view: yaw is undefined, default to 0
        } else {
            vd.x.atan2(-vd.y)
        };
        // The saved view can carry a twist (rotation about the view axis), set
        // when the view was aligned to a rotated UCS. The twist is the angle
        // that rotates world-X onto screen-right, so the world direction that
        // ends up horizontal is its negative; feed that in as the camera roll
        // so the drawing opens square, the way it was saved, instead of in raw
        // world orientation (which looks tilted).
        let rotation = view::camera::yaw_pitch_to_quat(yaw, pitch, -twist as f32);
        let view_right = rotation * glam::Vec3::X;
        let view_up = rotation * glam::Vec3::Y;
        // view_target is WCS; wire-space subtracts world_offset. view_center is
        // a DCS (screen-plane) offset, so fold it through the view basis.
        // Keep the target in f64: casting it to f32 first quantizes the camera
        // to ~0.5 m at UTM scale, so panning/zooming inside a floating viewport
        // (which nudges view_target by sub-metre f64 steps) made the content
        // jump on the f32 grid. The axis directions stay f32 (orientation only);
        // only the position must stay precise — matching the model camera.
        let base = glam::DVec3::new(
            view_target.x,
            view_target.y,
            view_target.z,
        );
        let target = base
            + view_right.as_dvec3() * view_center.x
            + view_up.as_dvec3() * view_center.y;
        let lens_length = lens_length.max(1.0) as f32;
        // The saved-view convention uses a 24 mm vertical aperture. This is
        // the inverse of the projection path's documented
        // `distance = view_height * lens / 24` relation.
        let fov_y = if perspective {
            2.0 * (12.0 / lens_length).atan()
        } else {
            45.0_f32.to_radians()
        };
        let distance = ((view_height as f32 / 2.0) / (fov_y * 0.5).tan()).max(0.001);
        Some(Camera {
            target,
            rotation,
            distance,
            fov_y,
            projection: if perspective {
                view::camera::Projection::Perspective
            } else {
                view::camera::Projection::Orthographic
            },
            yaw,
            pitch,
            // Sized from the saved view height (not a zoom-scaled range) so a
            // restored view keeps stable depth precision. A THIN projection —
            // e.g. a model-documentation section/side view whose cut plane sits
            // half an object-depth from the view target — has content much
            // deeper than its visible height, so the depth half-range must be a
            // generous multiple: `×4` clipped a section view's outline that was
            // ~6× the view height deep. Orthographic depth precision is linear,
            // so the wide range costs no z-fighting.
            depth_half_range: (view_height as f32 * 64.0).max(1.0),
            // A restored view has no model AABB yet; `fit_depth_to_saved_extents`
            // fills it in from $EXTMIN/$EXTMAX right after, so the near/far tracks
            // the eye direction once the view is orbited.
            model_bounds: None,
        })
    }

    /// Decode a VPort table entry (model-space tiled view) into a `Camera`.
    fn camera_from_vport(&self, vp: &acadrust::tables::VPort) -> Option<Camera> {
        self.camera_from_view_mode(
            vp.view_direction,
            vp.view_target,
            vp.view_center,
            vp.view_height,
            vp.view_twist,
            vp.perspective,
            vp.lens_length,
        )
    }

    /// Reverse of `camera_from_vport`: write `cam`'s view target / direction
    /// / height onto a fresh VPort entry with the given `name` and screen
    /// rectangle (0..1 normalized, DXF bottom-left origin convention).
    fn vport_from_camera(
        &self,
        name: &str,
        cam: &Camera,
        lower_left: acadrust::types::Vector2,
        upper_right: acadrust::types::Vector2,
    ) -> acadrust::tables::VPort {
        let view_dir = cam.rotation * glam::Vec3::Z;
        let view_height = cam.ortho_size() * 2.0;
        let target_wcs = acadrust::types::Vector3 {
            x: (cam.target.x as f64) + [0.0_f64; 3][0],
            y: (cam.target.y as f64) + [0.0_f64; 3][1],
            z: (cam.target.z as f64) + [0.0_f64; 3][2],
        };
        let mut entry = acadrust::tables::VPort::new(name);
        entry.lower_left = lower_left;
        entry.upper_right = upper_right;
        entry.view_target = target_wcs;
        entry.view_direction = acadrust::types::Vector3 {
            x: view_dir.x as f64,
            y: view_dir.y as f64,
            z: view_dir.z as f64,
        };
        entry.view_height = view_height as f64;
        entry.perspective = cam.projection == view::camera::Projection::Perspective;
        entry.lens_length = (12.0 / (cam.fov_y * 0.5).tan().max(1e-6)) as f64;
        entry.view_center = acadrust::types::Vector2::ZERO;
        // Stored twist = -roll, matching the decoder (roll = -twist).
        entry.view_twist = -cam.roll() as f64;
        entry
    }

    /// Convert a `ModelTile`'s normalized iced rectangle (top-left origin) to
    /// the (lower_left, upper_right) pair the VPort table uses (bottom-left
    /// origin).
    fn tile_rect_to_vport(rect: iced::Rectangle) -> (acadrust::types::Vector2, acadrust::types::Vector2) {
        let lower_left = acadrust::types::Vector2 {
            x: rect.x as f64,
            y: (1.0 - rect.y - rect.height) as f64,
        };
        let upper_right = acadrust::types::Vector2 {
            x: (rect.x + rect.width) as f64,
            y: (1.0 - rect.y) as f64,
        };
        (lower_left, upper_right)
    }

    /// Inverse of `tile_rect_to_vport`.
    fn vport_to_tile_rect(lower_left: acadrust::types::Vector2, upper_right: acadrust::types::Vector2) -> iced::Rectangle {
        iced::Rectangle {
            x: lower_left.x as f32,
            y: (1.0 - upper_right.y) as f32,
            width: (upper_right.x - lower_left.x) as f32,
            height: (upper_right.y - lower_left.y) as f32,
        }
    }

    /// Restore `model_tiles` from VPort entries that a previous save left in
    /// the document. Native AutoCAD tiled model-space layouts are represented
    /// by duplicate `*Active` VPort entries.
    /// Returns true on success — the caller skips `apply_active_vport_camera`
    /// in that case because the active tile's camera has already been loaded
    /// into `self.camera`.
    fn restore_model_tiles_from_vports(&mut self) -> bool {
        let active_vports: Vec<acadrust::tables::VPort> = self
            .document
            .vports
            .iter()
            .filter(|v| is_active_vport_name(&v.name))
            .cloned()
            .collect();

        if active_vports.len() <= 1 {
            return false;
        }

        let tiles: Vec<ModelTile> = active_vports
            .iter()
            .filter_map(|vp| {
                self.camera_from_vport(vp).map(|cam| ModelTile {
                    rect: Self::vport_to_tile_rect(vp.lower_left, vp.upper_right),
                    camera: cam,
                    render_mode: vp.render_mode,
                    grid_on: vp.grid_on,
                    snap_on: vp.snap_on,
                })
            })
            .collect();

        if tiles.len() <= 1 {
            return false;
        }

        let active_cam = tiles[0].camera.clone();
        *self.model_tiles.borrow_mut() = tiles;
        self.active_model_tile.set(0);
        *self.camera.borrow_mut() = active_cam;
        self.projection_bounds_epoch.set(0);
        // Rebuild the pane_grid layout from the restored tile rects so the panes
        // match (the renderer / input iterate panes, not tiles).
        self.rebuild_panes_from_tiles();
        self.camera_generation += 1;
        true
    }

    /// Persist `model_tiles` to the VPort table. Native AutoCAD tiled model
    /// viewports are written as duplicate `*Active` entries.
    fn save_model_tiles_to_vports(&mut self) {
        // Stash the live camera into the active tile so the about-to-write
        // snapshot reflects the user's most recent orbit / pan / zoom.
        {
            let live_cam = self.camera.borrow().clone();
            let mut tiles = self.model_tiles.borrow_mut();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            if let Some(t) = tiles.get_mut(active) {
                t.camera = live_cam;
            }
        }

        let table_handle = self.document.vports.handle();
        let preserved_vps: Vec<acadrust::tables::VPort> = self
            .document
            .vports
            .iter()
            .filter(|v| !is_active_vport_name(&v.name))
            .cloned()
            .collect();
        let mut new_vports = acadrust::tables::Table::with_handle(table_handle);
        for vp in preserved_vps {
            new_vports.add_or_replace(vp);
        }
        self.document.vports = new_vports;

        let tiles = self.model_tiles.borrow().clone();
        if tiles.is_empty() {
            return;
        }

        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        let mut ordered_tiles = Vec::with_capacity(tiles.len());
        ordered_tiles.push(tiles[active].clone());
        for (i, tile) in tiles.iter().enumerate() {
            if i != active {
                ordered_tiles.push(tile.clone());
            }
        }

        for tile in ordered_tiles {
            let (ll, ur) = Self::tile_rect_to_vport(tile.rect);
            let mut entry = self.vport_from_camera("*Active", &tile.camera, ll, ur);
            entry.render_mode = tile.render_mode;
            // Each viewport persists its own grid display + grid-snap (#121).
            entry.grid_on = tile.grid_on;
            entry.snap_on = tile.snap_on;
            entry.handle = self.document.allocate_handle();
            self.document.vports.add_allow_duplicate(entry);
        }
    }

    /// Mirror the live grid/snap toggles onto the active view's own store so the
    /// state stays independent per viewport: a model tile in model space, the
    /// layout's sheet viewport in paper space. (#121)
    pub fn set_active_tile_grid_snap(&mut self, grid_on: bool, snap_on: bool) {
        if self.current_layout != "Model" {
            // Paper space: target the active floating viewport if the user is
            // working inside one, otherwise the layout's sheet viewport. Each
            // viewport keeps its own grid/snap (round-tripped via status flags).
            let h = self
                .active_viewport
                .filter(|h| h.is_valid())
                .unwrap_or_else(|| self.current_layout_sheet_viewport_handle());
            if h.is_valid() {
                if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(h) {
                    vp.status.grid_on = grid_on;
                    vp.status.snap_on = snap_on;
                }
            }
            return;
        }
        let mut tiles = self.model_tiles.borrow_mut();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        if let Some(t) = tiles.get_mut(active) {
            t.grid_on = grid_on;
            t.snap_on = snap_on;
        }
    }

    /// The active view's grid display + grid-snap, adopted into the live toggles
    /// on load and whenever the active viewport / tab / layout changes. Reads the
    /// model tile in model space, the sheet viewport in paper space. (#121)
    pub fn active_tile_grid_snap(&self) -> Option<(bool, bool)> {
        if self.current_layout != "Model" {
            let h = self
                .active_viewport
                .filter(|h| h.is_valid())
                .unwrap_or_else(|| self.current_layout_sheet_viewport_handle());
            if h.is_valid() {
                if let Some(EntityType::Viewport(vp)) = self.document.get_entity(h) {
                    return Some((vp.status.grid_on, vp.status.snap_on));
                }
            }
            return Some((false, false));
        }
        let tiles = self.model_tiles.borrow();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        tiles.get(active).map(|t| (t.grid_on, t.snap_on))
    }

    /// Set the paper-space camera from the sheet viewport's stored view.
    /// Returns true if a valid sheet viewport was found and the camera was set.
    ///
    /// The sheet viewport entity is the authoritative paper-space view (it
    /// round-trips through both the DXF and DWG writers). An older
    /// `OpenCADStudio_Camera_<layout>` named View is honoured only as a
    /// backward-compatible fallback for files saved under the previous scheme.
    fn apply_sheet_viewport_camera(&mut self) -> bool {
        // The Layout object already owns the authoritative sheet-viewport
        // handle. Do not scan every drawing entity on each Model↔Paper switch.
        let sheet_vp = match self
            .document
            .get_entity(self.current_layout_sheet_viewport_handle())
        {
            Some(EntityType::Viewport(vp)) => Some(vp.clone()),
            _ => None,
        };

        let vp = match sheet_vp {
            Some(v) if v.view_height.abs() >= 1e-9 => v,
            _ => {
                // Back-compat: files OCS saved with the named-View side-channel.
                let view_name = format!("OpenCADStudio_Camera_{}", self.current_layout);
                let fallback =
                    self.document.views.iter().find(|v| v.name == view_name).cloned();
                if let Some(view) = fallback {
                    return self.apply_camera_from_view_entry(&view, false);
                }
                return false;
            }
        };

        // Paper-space entities carry no world_offset → decode with a zero
        // offset, through the same shared decoder (twist included).
        let Some(cam) = self.camera_from_view_mode(
            vp.view_direction,
            vp.view_target,
            acadrust::types::Vector2 {
                x: vp.view_center.x,
                y: vp.view_center.y,
            },
            vp.view_height,
            vp.twist_angle,
            vp.status.perspective,
            vp.lens_length,
        ) else {
            return false;
        };
        *self.camera.borrow_mut() = cam;
        self.camera_generation += 1;
        true
    }

    /// Write the current camera back into the document (VPort or sheet viewport)
    /// so it is saved with the file. Returns true if the document was modified.
    pub fn sync_camera_to_document(&mut self) -> bool {
        let cam = self.camera.borrow().clone();
        let view_dir = cam.rotation * glam::Vec3::Z;
        let view_height = cam.ortho_size() * 2.0;
        // Stored twist is the negative of the camera roll (the decoder applies
        // roll = -twist), so the saved view round-trips square.
        let twist = -cam.roll() as f64;
        let vd3 = acadrust::types::Vector3 {
            x: view_dir.x as f64,
            y: view_dir.y as f64,
            z: view_dir.z as f64,
        };

        if self.current_layout == "Model" {
            let target_wcs = acadrust::types::Vector3 {
                x: (cam.target.x as f64) + [0.0_f64; 3][0],
                y: (cam.target.y as f64) + [0.0_f64; 3][1],
                z: (cam.target.z as f64) + [0.0_f64; 3][2],
            };

            // Write back to the *Active VPort entry (may be overridden by DWG writer).
            if let Some(vp) = self
                .document
                .vports
                .iter_mut()
                .find(|v| is_active_vport_name(&v.name))
            {
                vp.view_target = target_wcs;
                vp.view_center = acadrust::types::Vector2::ZERO;
                vp.view_direction = vd3;
                vp.view_height = view_height as f64;
                vp.view_twist = twist;
                vp.perspective = cam.projection == view::camera::Projection::Perspective;
                vp.lens_length = (12.0 / (cam.fov_y * 0.5).tan().max(1e-6)) as f64;
            }

            // Persist the tiled layout as duplicate `*Active` VPort entries.
            // The view lives entirely in the standard VPORT table now — no
            // app-specific View record.
            self.save_model_tiles_to_vports();
            true
        } else {
            let target_wcs = acadrust::types::Vector3 {
                x: cam.target.x as f64,
                y: cam.target.y as f64,
                z: cam.target.z as f64,
            };

            // The sheet viewport entity is the authoritative paper-space view;
            // it round-trips natively, so no named-View side-channel is needed.
            let sheet_handle = self.current_layout_sheet_viewport_handle();
            if let Some(EntityType::Viewport(vp)) =
                self.document.get_entity_mut(sheet_handle)
            {
                // AutoCAD stores the paper-space view position in
                // `view_center` (DCS) with `view_target` at the origin —
                // writing it the other way round shifts the layout and
                // crashes nothing but renders the sheet off-place. Paper
                // space is always a plan view, so DCS == WCS XY here.
                vp.view_center =
                    acadrust::types::Vector3::new(target_wcs.x, target_wcs.y, 0.0);
                vp.view_target = acadrust::types::Vector3::ZERO;
                vp.view_direction = vd3;
                vp.view_height = view_height as f64;
                vp.twist_angle = twist;
                vp.status.perspective =
                    cam.projection == view::camera::Projection::Perspective;
                vp.lens_length = (12.0 / (cam.fov_y * 0.5).tan().max(1e-6)) as f64;
            }
            true
        }
    }


    /// Restore the camera from the file's saved view (called once on open).
    /// Falls back to fit_all() if no saved view is available.
    pub fn restore_saved_camera(&mut self) {
        let restored = if self.current_layout == "Model" {
            // Tiled-layout restore takes precedence — it sets the camera too.
            // Single-tile files fall through to the *Active branch.
            self.restore_model_tiles_from_vports() || self.apply_active_vport_camera()
        } else {
            // Every paper layout has a full-screen sheet viewport that holds
            // its view; create one if a loaded file lacks it.
            let layout = self.current_layout.clone();
            self.ensure_sheet_viewport(&layout);
            self.apply_sheet_viewport_camera()
        };
        if restored {
            // The pose is the file's and must not move — but nothing has sized
            // its near/far, so `depth_half_range` stays unset and the
            // projection falls back to a guess scaled from the zoom distance.
            // On a drawing whose geometry reaches far off its plane that guess
            // is far too narrow and the geometry is silently clipped away.
            self.fit_depth_to_saved_extents();
        } else {
            self.fit_all();
        }
    }

    /// Size the camera's near/far from the drawing's own recorded extent,
    /// leaving its pose alone.
    ///
    /// `$EXTMIN`/`$EXTMAX` is untrustworthy for *positioning* — stale after an
    /// edit, or the 1e20 sentinel when the writer never computed it (see the
    /// `world_offset` selection notes) — which is why `fit_all` scans entities
    /// instead. It is the right input *here*: a near/far that is too wide only
    /// costs a little depth precision, while one that is too narrow deletes
    /// geometry outright, so erring on the drawing's own claim is the safe
    /// direction. It also costs nothing — no tessellation pass on open.
    fn fit_depth_to_saved_extents(&mut self) {
        // Same bound `model_space_extents`'s header fallback uses; rejects the
        // sentinel and any garbage that would blow the range up.
        const SANE_EXTENT: f64 = 1.0e16;
        let h = &self.document.header;
        let (lo, hi) = (h.model_space_extents_min, h.model_space_extents_max);
        let ok = lo.x <= hi.x
            && lo.y <= hi.y
            && lo.z <= hi.z
            && [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]
                .iter()
                .all(|v| v.is_finite() && v.abs() < SANE_EXTENT);
        if !ok {
            return;
        }
        let min = glam::Vec3::new(lo.x as f32, lo.y as f32, lo.z as f32);
        let max = glam::Vec3::new(hi.x as f32, hi.y as f32, hi.z as f32);
        self.camera.borrow_mut().fit_depth_to_bounds(min, max);
    }

    fn fit_paper_space_extents(&mut self) {
        let layout_block = self.current_layout_block_handle();
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        {
            let mut include = |x: f64, y: f64, z: f64| {
                const SANE_EXTENT: f64 = 1.0e16;
                if x.is_finite()
                    && y.is_finite()
                    && z.is_finite()
                    && x.abs() < SANE_EXTENT
                    && y.abs() < SANE_EXTENT
                    && z.abs() < SANE_EXTENT
                {
                    let point = glam::Vec3::new(x as f32, y as f32, z as f32);
                    min = min.min(point);
                    max = max.max(point);
                }
            };

            // The physical sheet is always part of Paper Space extents, even
            // when the layout contains no entities (#539).
            if let Some(((x0, y0), (x1, y1))) = self.paper_limits() {
                include(x0, y0, 0.0);
                include(x1, y1, 0.0);
            }

            // Paper entities and viewport borders belong to the sheet. Model
            // content projected through those viewports deliberately does not.
            let scale = if self.current_layout == "Model" {
                crate::scene::annotative::scale_handle_by_name(
                    &self.document,
                    &self.document.header.current_annotation_scale,
                )
            } else {
                self.paper_annotation_scale_handle()
            };
            for wire in self.wires_for_block_culled(
                layout_block,
                None,
                None,
                None,
                None,
                scale,
                self.annotation_all_visible(),
                None,
            ) {
                let is_infinite = Self::handle_from_wire_name(&wire.name)
                    .and_then(|handle| self.document.get_entity(handle))
                    .is_some_and(|entity| {
                        matches!(entity, EntityType::XLine(_) | EntityType::Ray(_))
                    });
                if is_infinite {
                    for &[x, y, z] in &wire.key_vertices {
                        include(x, y, z);
                    }
                } else {
                    for (index, &[x, y, z]) in wire.points.iter().enumerate() {
                        let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
                        include(
                            x as f64 + low[0] as f64,
                            y as f64 + low[1] as f64,
                            z as f64 + low[2] as f64,
                        );
                    }
                }
            }

            // Hatches, wipeouts and raster images are GPU-only and may carry
            // no normal wire outline, so include their visible paper bounds.
            let (hatches, wipeouts, images) = self.paper_sheet_render_models();
            for hatch in hatches.iter().chain(wipeouts.iter()) {
                for &[x, y] in hatch.boundary.iter() {
                    include(
                        hatch.world_origin[0] + x as f64,
                        hatch.world_origin[1] + y as f64,
                        0.0,
                    );
                }
            }
            for image in images.iter() {
                for (corner, low) in image.corners.iter().zip(image.corners_low.iter()) {
                    include(
                        corner[0] as f64 + low[0] as f64,
                        corner[1] as f64 + low[1] as f64,
                        corner[2] as f64 + low[2] as f64,
                    );
                }
            }
        }

        if !min.is_finite() || !max.is_finite() {
            return;
        }
        if min == max {
            max += glam::Vec3::splat(1.0);
        }
        self.camera
            .borrow_mut()
            .fit_to_bounds(min, max, self.last_render_aspect.get().max(0.01));
        self.camera_generation += 1;
    }

    pub fn fit_all(&mut self) {
        if self.active_viewport.is_some() {
            if let Some((mut min, mut max)) = self.model_space_extents() {
                if min == max {
                    max += glam::Vec3::splat(1.0);
                    min -= glam::Vec3::splat(1.0);
                }
                self.fit_active_viewport_to_bounds(min, max);
            }
            return;
        }
        if self.current_layout != "Model" {
            self.fit_paper_space_extents();
            return;
        }

        // Use the FULL, un-culled wire set — not `entity_wires()`, which is
        // frustum-culled to the current view. Culled input would fit only the
        // entities already on screen, so each call would zoom out a little and
        // reveal more, converging on the true extent only after several uses
        // (issue #51). `wpp = None` also tessellates at a fixed tolerance so
        // the bounds don't drift with zoom-adaptive curve sampling.
        let layout_block = self.current_layout_block_handle();
        let scale = if self.current_layout == "Model" {
            crate::scene::annotative::scale_handle_by_name(
                &self.document,
                &self.document.header.current_annotation_scale,
            )
        } else {
            self.paper_annotation_scale_handle()
        };
        let mut wires = self.wires_for_block_culled(
            layout_block,
            None,
            None,
            None,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        );
        // Ray / XLine tessellate as ±DISPLAY_EXTENT display segments
        // (entities/ray.rs) — their endpoints are rendering artifacts, not
        // drawing extent. A construction line through the drawing defeats
        // both rejects below: its centroid sits at the base point (inside
        // the consensus cluster), and in a fresh drawing `local_extent_max`
        // is still the 1e9 default, so the far points pass the `lim` filter
        // too (issue #284). Exclude them up front — an infinite construction
        // line should never contribute to ZOOM Extents — and fall back to their
        // base points if the drawing holds nothing else.
        let mut infinite_base_pts: Vec<glam::Vec3> = Vec::new();
        wires.retain(|w| {
            let is_infinite = Self::handle_from_wire_name(&w.name)
                .and_then(|h| self.document.get_entity(h))
                .map(|e| matches!(e, EntityType::XLine(_) | EntityType::Ray(_)))
                .unwrap_or(false);
            if is_infinite {
                infinite_base_pts.extend(
                    w.key_vertices
                        .iter()
                        .map(|v| glam::Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)),
                );
            }
            !is_infinite
        });
        // 3D solids render as meshes, not wires, so collect their complete
        // world boxes separately — a drawing of only solids has no wires to fit.
        let mesh_aabbs: Vec<([f32; 4], [f32; 2])> = self
            .meshes
            .iter()
            .filter(|(h, _)| {
                let handle = **h;
                self.mesh_entity_visible(handle)
                    && self.document.get_entity(handle).is_some_and(|entity| {
                        self.belongs_to_visible_block(
                            handle,
                            entity.common().owner_handle,
                            layout_block,
                        )
                    })
            })
            .map(|(_, set)| (set.world_aabb, set.z_aabb))
            .filter(|(xy, z)| {
                xy[0].is_finite()
                    && xy[1].is_finite()
                    && xy[2].is_finite()
                    && xy[3].is_finite()
                    && z[0].is_finite()
                    && z[1].is_finite()
            })
            .collect();
        if wires.is_empty() && mesh_aabbs.is_empty() && infinite_base_pts.is_empty() {
            return;
        }

        // Per-wire centroid pass — used both for the absolute-magnitude reject
        // (`local_extent_max`) and for the IQR-based outlier reject below.
        // A wire whose centroid sits far outside the drawing's consensus
        // cluster is an orphan (block-defn entity that leaked into MSPACE,
        // bogus hatch boundary, Ray/XLine far point) and must not poison the
        // bounding box.
        struct WireCent {
            idx: usize,
            cx: f32,
            cy: f32,
        }
        let lim = self.local_extent_max;
        let mut cents: Vec<WireCent> = Vec::with_capacity(wires.len());
        for (idx, wire) in wires.iter().enumerate() {
            let mut sx = 0.0_f64;
            let mut sy = 0.0_f64;
            let mut n = 0_usize;
            for &[x, y, _] in &wire.points {
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                sx += x as f64;
                sy += y as f64;
                n += 1;
            }
            if n > 0 {
                cents.push(WireCent {
                    idx,
                    cx: (sx / n as f64) as f32,
                    cy: (sy / n as f64) as f32,
                });
            }
        }
        if cents.is_empty() && mesh_aabbs.is_empty() && infinite_base_pts.is_empty() {
            return;
        }

        // Robust drawing centre (median centroid). `lim` is a span RELATIVE to
        // this centre, so every reject below is distance-from-centre — geometry
        // now reaches fit_all as absolute coordinates (no world_offset), which
        // at UTM scale are ~5.7e6; an absolute `|x| > lim` test would reject the
        // entire drawing and make ZOOM Extents a no-op.
        let (mcx, mcy) = {
            let mut xs: Vec<f32> = cents.iter().map(|c| c.cx).collect();
            let mut ys: Vec<f32> = cents.iter().map(|c| c.cy).collect();
            if xs.is_empty() {
                (0.0_f32, 0.0_f32)
            } else {
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                (xs[xs.len() / 2], ys[ys.len() / 2])
            }
        };

        // IQR-based reject only kicks in with enough samples for the quartiles
        // to be meaningful. Below that, the centre-relative `lim` filter is the
        // only gate (legacy behavior).
        let (rx_lo, rx_hi, ry_lo, ry_hi) = if cents.len() >= 8 {
            let mut xs: Vec<f32> = cents.iter().map(|c| c.cx).collect();
            let mut ys: Vec<f32> = cents.iter().map(|c| c.cy).collect();
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let q = |v: &[f32], frac: f32| v[((v.len() as f32 - 1.0) * frac) as usize];
            let q1x = q(&xs, 0.25);
            let q3x = q(&xs, 0.75);
            let q1y = q(&ys, 0.25);
            let q3y = q(&ys, 0.75);
            // k=10× the inter-quartile span is permissive enough to keep
            // legitimate sparse outlying geometry (annotation labels, scattered
            // dim leaders) but tight enough to drop a single wire stranded at
            // -world_offset. `max(1.0)` guards against a degenerate IQR=0
            // (e.g. all wires at the same centroid).
            const K: f32 = 10.0;
            let dx = (q3x - q1x).max(1.0) * K;
            let dy = (q3y - q1y).max(1.0) * K;
            (q1x - dx, q3x + dx, q1y - dy, q3y + dy)
        } else {
            (mcx - lim, mcx + lim, mcy - lim, mcy + lim)
        };

        let mut min = glam::Vec3::splat(f32::MAX);
        let mut max = glam::Vec3::splat(f32::MIN);
        for c in &cents {
            if c.cx < rx_lo || c.cx > rx_hi || c.cy < ry_lo || c.cy > ry_hi {
                continue;
            }
            let wire = &wires[c.idx];
            for &[x, y, z] in &wire.points {
                if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                    continue;
                }
                if (x - mcx).abs() > lim || (y - mcy).abs() > lim {
                    continue;
                }
                min = min.min(glam::Vec3::new(x, y, z));
                max = max.max(glam::Vec3::new(x, y, z));
            }
        }
        // Fold in 3D-solid mesh AABBs (not subject to the wire IQR reject).
        for ([ax, ay, bx, by], [az, bz]) in &mesh_aabbs {
            min = min.min(glam::Vec3::new(*ax, *ay, *az));
            max = max.max(glam::Vec3::new(*bx, *by, *bz));
        }
        // Drawing holds only infinite construction geometry: fit the view to
        // its base points rather than leaving the camera unchanged.
        if min.x > max.x {
            for p in &infinite_base_pts {
                min = min.min(*p);
                max = max.max(*p);
            }
        }
        // If no usable points found, leave the camera unchanged.
        if min.x > max.x {
            return;
        }
        if min == max {
            max += glam::Vec3::splat(1.0);
        }
        let aspect = self.active_camera_aspect();
        self.camera.borrow_mut().fit_to_bounds(min, max, aspect);
        self.projection_bounds_epoch.set(self.geometry_epoch);
        self.camera_generation += 1;
    }

    /// Fit every tiled Model viewport independently. The drawing bounds are
    /// shared, but each tile keeps its own orientation and uses its own screen
    /// aspect when calculating the required zoom.
    pub fn fit_all_model_viewports(&mut self) {
        if self.current_layout != "Model" || self.model_tiles.borrow().len() <= 1 {
            self.fit_all();
            return;
        }
        let generation_before = self.camera_generation;
        self.fit_all();
        if self.camera_generation == generation_before {
            return;
        }
        let Some((min, max)) = self.camera.borrow().fitted_model_bounds() else {
            return;
        };

        let (canvas_w, canvas_h) = self.selection.borrow().vp_size;
        let fallback = self.last_render_aspect.get().max(0.01);
        let aspects: Vec<f32> = self
            .model_tiles
            .borrow()
            .iter()
            .map(|tile| {
                let width = tile.rect.width * canvas_w;
                let height = tile.rect.height * canvas_h;
                if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
                    width / height
                } else {
                    fallback
                }
            })
            .collect();
        let active = self
            .active_model_tile
            .get()
            .min(aspects.len().saturating_sub(1));
        let live_camera = self.camera.borrow_mut();
        let mut tiles = self.model_tiles.borrow_mut();
        for (index, tile) in tiles.iter_mut().enumerate() {
            let aspect = aspects.get(index).copied().unwrap_or(fallback);
            if index == active {
                tile.camera = live_camera.clone();
            } else {
                tile.camera.fit_to_bounds(min, max, aspect);
            }
        }
        if let Some(aspect) = aspects.get(active) {
            self.last_render_aspect.set(*aspect);
        }
        self.camera_generation += 1;
    }

    pub fn update(&mut self, _dt: Duration) {}
}
