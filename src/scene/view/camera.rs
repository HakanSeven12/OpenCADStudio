// Arcball orbit camera — quaternion-based rotation, no gimbal lock.
//
// The camera orbits around a `target` point using a unit quaternion (`rotation`)
// that maps the canonical "camera looks down -Z" pose to the current view.
//
// Pan:       translates `target` in the view-plane (no rotation change).
// Orbit:     updates `rotation` via arcball delta — converts screen drag delta
//            to a rotation axis/angle, then pre-multiplies the current quaternion.
// Zoom:      adjusts `distance` (exponential feel).
// Snap:      directly assigns yaw+pitch encoded as a quaternion (for ViewCube).
//
// Coordinate convention: Z-up world space (same as the rest of OpenCADStudio).

use glam::camera::rh::proj::directx::{orthographic, perspective};
use glam::camera::rh::view::look_at_mat4;
use glam::{DVec3, Mat4, Quat, Vec3};
use iced::{Point, Rectangle};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Projection {
    Orthographic,
    Perspective,
}

#[derive(Clone)]
pub struct Camera {
    /// World-space pivot point the camera orbits around. Kept in f64 so a far
    /// pan (large offset-relative coordinate) doesn't lose precision in the
    /// pivot itself — the eventual relative-to-eye render path needs an exact
    /// eye, which derives from this target.
    pub target: DVec3,
    /// Arcball rotation: maps canonical pose to current orientation.
    pub rotation: Quat,
    /// Distance from eye to target.
    pub distance: f32,
    /// Vertical field of view in radians (perspective only).
    pub fov_y: f32,
    pub projection: Projection,

    // --- Legacy yaw/pitch exposed only for ViewCube hit-test compatibility ---
    // Kept in sync with `rotation` whenever orbit() or snap_angles() is called.
    pub yaw: f32,
    pub pitch: f32,

    /// Half-depth of the orthographic frustum in **world units**, sized from the
    /// drawing's extent (set by `fit_to_bounds`). The ortho near/far are placed
    /// at `distance ± depth_half_range`, so depth-buffer precision stays fixed
    /// as the user zooms. `0.0` means "unset" — the projection falls back to a
    /// distance-scaled range, whose precision collapses when zoomed out and
    /// makes coincident solids / meshes / wires flip draw order.
    ///
    /// Only a fallback now: when `model_bounds` is set, the near/far depth is
    /// derived from it per frame (see [`Camera::ortho_depth_range`]).
    pub depth_half_range: f32,

    /// Cached model AABB in world space, used to recompute the ortho near/far
    /// depth for the CURRENT eye direction every frame. A frozen scalar range
    /// (`depth_half_range`) is only correct for the orientation it was fitted
    /// in: a 3-D drawing fitted straight-down (top view) has a shallow depth
    /// span, but once orbited its width loads onto the eye axis and overruns
    /// that span, clipping the drawing (#473). Re-projecting the box each frame
    /// keeps near/far exactly as deep as the current view needs. `None` falls
    /// back to `depth_half_range`.
    pub model_bounds: Option<(Vec3, Vec3)>,
}

impl Default for Camera {
    fn default() -> Self {
        // Default: look straight down at the XY drawing plane (top view, Z-up).
        // yaw = 0, pitch = PI/2  →  eye is directly above target.
        let yaw = 0.0_f32;
        let pitch = std::f32::consts::FRAC_PI_2;
        Self {
            target: DVec3::ZERO,
            rotation: yaw_pitch_to_quat(yaw, pitch, 0.0),
            distance: 60.36,
            fov_y: 45.0_f32.to_radians(),
            projection: Projection::Orthographic,
            yaw,
            pitch,
            depth_half_range: 0.0,
            model_bounds: None,
        }
    }
}

pub const OPENGL_TO_WGPU: Mat4 = glam::mat4(
    glam::vec4(1.0, 0.0, 0.0, 0.0),
    glam::vec4(0.0, 1.0, 0.0, 0.0),
    glam::vec4(0.0, 0.0, 0.5, 0.0),
    glam::vec4(0.0, 0.0, 0.5, 1.0),
);

impl Camera {
    // ── Eye position ───────────────────────────────────────────────────────

    /// Eye position in full f64 precision (world space). The whole pipeline is
    /// relative-to-eye, so this is the canonical eye; direction-only callers
    /// (view-matrix basis, ray casts) take `.as_vec3()`.
    pub fn eye(&self) -> DVec3 {
        let eye_dir = (self.rotation * Vec3::Z).as_dvec3();
        self.target + eye_dir * self.distance as f64
    }

    /// Half-height of the orthographic frustum in world units.
    pub fn ortho_size(&self) -> f32 {
        self.distance * (self.fov_y * 0.5).tan()
    }

    /// Change projection while keeping the fitted model's screen-space frame.
    ///
    /// `distance` is both the orthographic zoom scale and the perspective eye
    /// distance. Reusing it unchanged only preserves scale on the target plane;
    /// geometry closer to the eye can grow dramatically. Match the maximum
    /// projected extent of the model box instead, leaving target, rotation and
    /// field of view untouched.
    pub fn set_projection_preserving_frame(
        &mut self,
        projection: Projection,
        aspect: f32,
    ) {
        if self.projection == projection {
            return;
        }

        let Some((min, max)) = self.model_bounds else {
            self.projection = projection;
            return;
        };

        let aspect = aspect.max(0.01);
        let tan_half_fov = (self.fov_y * 0.5).tan();
        if !tan_half_fov.is_finite() || tan_half_fov <= 1e-6 {
            self.projection = projection;
            return;
        }

        let corners = self.bounds_in_view(min, max);
        let radius = |corner: &Vec3| corner.y.abs().max(corner.x.abs() / aspect);
        let max_radius = corners.iter().map(radius).fold(0.0_f32, f32::max);
        let min_z = corners
            .iter()
            .map(|corner| corner.z)
            .fold(f32::INFINITY, f32::min);
        let max_z = corners
            .iter()
            .map(|corner| corner.z)
            .fold(f32::NEG_INFINITY, f32::max);
        let depth_margin = ((max_z - min_z).abs() * 0.001).max(0.001);
        let near_safe = if max_z > 0.0 {
            (max_z / (1.0 - 0.001)) * 1.001
        } else {
            0.001
        };
        let far_safe = if min_z < 0.0 {
            (-min_z / (1000.0 - 1.0)) * 1.001
        } else {
            0.001
        };
        let depth_safe_distance = (max_z + depth_margin)
            .max(near_safe)
            .max(far_safe)
            .max(0.001);
        if !max_radius.is_finite() || max_radius <= 1e-6 {
            if projection == Projection::Perspective && depth_safe_distance.is_finite() {
                self.distance = self.distance.max(depth_safe_distance);
            }
            self.projection = projection;
            return;
        }

        // Extent is the model box's largest absolute NDC X/Y displacement.
        // When an existing perspective eye is already inside the box, its
        // projected envelope is undefined; fall back to its target-plane scale
        // so switching back to orthographic still recovers a usable view.
        let old_extent = match self.projection {
            Projection::Orthographic => max_radius / self.ortho_size().max(1e-6),
            Projection::Perspective => {
                let near = (self.distance * 0.001).max(1e-6);
                let mut extent = 0.0_f32;
                let mut all_in_front = true;
                for corner in &corners {
                    let depth = self.distance - corner.z;
                    if depth <= near {
                        all_in_front = false;
                        break;
                    }
                    extent = extent.max(radius(corner) / (depth * tan_half_fov));
                }
                if all_in_front {
                    extent
                } else {
                    max_radius / (self.distance.max(0.001) * tan_half_fov)
                }
            }
        };

        if !old_extent.is_finite() || old_extent <= 1e-6 {
            if projection == Projection::Perspective && depth_safe_distance.is_finite() {
                self.distance = self.distance.max(depth_safe_distance);
            }
            self.projection = projection;
            return;
        }

        match projection {
            Projection::Orthographic => {
                let distance = max_radius / (old_extent * tan_half_fov);
                if distance.is_finite() {
                    self.distance = distance.max(0.001);
                }
            }
            Projection::Perspective => {
                // For every corner, radius / ((distance - z) * tan(fov/2))
                // must be no larger than the old screen extent. The maximum
                // required distance gives the tightest perspective frame with
                // at least one corner touching the old envelope.
                let extent_at_depth = old_extent * tan_half_fov;
                let mut distance = f32::NEG_INFINITY;
                for corner in &corners {
                    distance = distance.max(corner.z + radius(corner) / extent_at_depth);
                }

                // Keep the complete box ahead of the perspective near plane,
                // and its far plane, including corners whose X/Y radius does
                // not constrain framing.
                distance = distance.max(depth_safe_distance);
                if distance.is_finite() {
                    self.distance = distance.max(0.001);
                }
            }
        }

        self.projection = projection;
    }

    // ── Projection matrices ────────────────────────────────────────────────

    /// Orthographic near/far that CENTRE the target plane at ndc-z ≈ 0.5.
    ///
    /// The draw-order depth bias shifts clip-z by ±`DRAW_ORDER_BIAS` (0.001).
    /// The old range (`near = distance*0.001`, `far = distance*1000`) parked the
    /// geometry at ndc-z ≈ 0.001 — right on the near plane — so a front-biased
    /// entity landed exactly at z = 0 and got clipped the moment f32 rounding
    /// (worse at high zoom) tipped it past the plane, making the drawing vanish.
    /// A symmetric range gives the bias half the depth buffer of headroom on
    /// each side; ortho permits a negative near.
    fn ortho_depth_range(&self) -> (f32, f32) {
        // Prefer a drawing-sized, zoom-independent half-range so depth-buffer
        // precision stays constant as the user zooms. A distance-scaled range
        // (the `else`) balloons the near/far span when zoomed out — at large
        // `distance` the f32 depth buffer can no longer separate coincident
        // solids / meshes / wires, so they flip draw order (issue: meshes drew
        // in front of solids only when zoomed out).
        let r = if let Some((min, max)) = self.model_bounds {
            // Depth extent along the CURRENT eye direction, recomputed each
            // frame so orbiting a 3-D drawing never clips it (#473): as the
            // view tilts off top, the box's width rotates onto the eye axis and
            // the span grows to match. Same tight, zoom-independent precision as
            // a fitted scalar, but always oriented to the live view.
            self.depth_extent_in_view(min, max)
        } else if self.depth_half_range > 0.0 {
            self.depth_half_range
        } else {
            (self.distance * 1000.0).max(1.0)
        };
        (self.distance - r, self.distance + r)
    }

    /// Relative-to-eye view-projection: identical projection, but the view
    /// matrix carries rotation only (translation zeroed). Positions fed to it
    /// must already be expressed relative to the eye (done per-vertex with
    /// double-single precision in the shader), so the large eye translation
    /// never enters the f32 matrix and large-coordinate jitter disappears.
    pub fn view_proj_rte(&self, bounds: Rectangle) -> Mat4 {
        let aspect = bounds.width / bounds.height;
        let up_dir = self.rotation * Vec3::Y;

        // The basis, from the rotation alone. Zeroing the translation column
        // afterwards is not enough: `look_at` builds the basis out of
        // `target - eye`, and handing it two absolute positions cast to f32
        // rounds both to the f32 grid first. At UTM that grid is half a metre
        // across, so the difference — and with it the whole view basis —
        // jitters as the camera moves, which is the shimmer a survey-placed
        // drawing showed.
        //
        // It never needed them. `eye` is `target + rotation·Z·distance`, so
        // the direction between the two is the rotation's own Z, a unit
        // vector that carries no coordinate at all.
        let mut view = look_at_mat4(Vec3::ZERO, -(self.rotation * Vec3::Z), up_dir);
        // Already zero, since the eye is the origin here; kept so the invariant
        // survives someone reworking the line above.
        view.w_axis = glam::vec4(0.0, 0.0, 0.0, 1.0);

        let proj = match self.projection {
            Projection::Perspective => {
                perspective(self.fov_y, aspect, self.distance * 0.001, self.distance * 1000.0)
            }
            Projection::Orthographic => {
                let h = self.ortho_size();
                let w = h * aspect;
                let (near, far) = self.ortho_depth_range();
                orthographic(-w, w, -h, h, near, far)
            }
        };
        OPENGL_TO_WGPU * proj * view
    }

    /// Project a world point to screen pixels with full f64 precision: the
    /// point is made eye-relative in f64 (small numbers near the view) before
    /// the rotation-only projection, so it stays exact at large absolute
    /// coordinates — the CPU equivalent of the GPU's relative-to-eye path.
    /// Returns `None` for points at/behind the eye plane (w ≈ 0).
    pub fn project(&self, p: glam::DVec3, bounds: Rectangle) -> Option<glam::Vec2> {
        let rel = (p - self.eye()).as_vec3();
        let clip = self.view_proj_rte(bounds) * rel.extend(1.0);
        if clip.w.abs() < 1e-9 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(glam::vec2(
            (ndc.x * 0.5 + 0.5) * bounds.width,
            (0.5 - ndc.y * 0.5) * bounds.height,
        ))
    }

    /// Unproject a screen point onto an arbitrary world plane in f64. The ray
    /// is built in eye-relative space (precise), intersected with the plane
    /// expressed relative to the eye, then shifted back by the f64 eye — so the
    /// returned world point keeps full precision at large absolute coordinates.
    pub fn unproject_on_plane(
        &self,
        screen: Point,
        bounds: Rectangle,
        plane_normal: Vec3,
        plane_point: glam::DVec3,
    ) -> glam::DVec3 {
        let eye = self.eye();
        let ndc_x = (screen.x / bounds.width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen.y / bounds.height) * 2.0;
        let inv = self.view_proj_rte(bounds).inverse();
        // Ray origin / direction in eye-relative space.
        let (ray_origin, ray_dir) = match self.projection {
            Projection::Perspective => {
                let near_pt = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
                let far_pt = inv.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
                (near_pt, (far_pt - near_pt).normalize())
            }
            Projection::Orthographic => {
                let origin = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
                let forward = self.rotation * Vec3::NEG_Z;
                (origin, forward)
            }
        };
        // Plane point relative to the eye (small) for a precise intersection.
        let plane_rel = (plane_point - eye).as_vec3();
        let denom = ray_dir.dot(plane_normal);
        let rel_hit = if denom.abs() < 1e-6 {
            plane_rel
        } else {
            let t = (plane_rel - ray_origin).dot(plane_normal) / denom;
            ray_origin + ray_dir * t
        };
        eye + rel_hit.as_dvec3()
    }

    /// Eye position split into two f32 (high + low) emulating f64, for the
    /// double-single relative-to-eye shaders. `high + low ≈ eye` to ~f64
    /// precision; the shader subtracts these from each vertex's own high/low.
    pub fn eye_high_low(&self) -> ([f32; 3], [f32; 3]) {
        let e = self.eye();
        let high = [e.x as f32, e.y as f32, e.z as f32];
        let low = [
            (e.x - high[0] as f64) as f32,
            (e.y - high[1] as f64) as f32,
            (e.z - high[2] as f64) as f32,
        ];
        (high, low)
    }

    /// Project a screen point onto an arbitrary world-space plane.
    ///
    /// The plane is defined by `plane_normal` (unit vector) and a `plane_point`
    /// that lies on it. Returns the ray–plane intersection (falling back to
    /// `plane_point` when nearly parallel), eye-relative in f64 so the cursor
    /// stays precise at UTM-scale coordinates.
    pub fn pick_on_plane(
        &self,
        screen: Point,
        bounds: Rectangle,
        plane_normal: Vec3,
        plane_point: glam::DVec3,
    ) -> glam::DVec3 {
        self.unproject_on_plane(screen, bounds, plane_normal, plane_point)
    }

    /// Project a screen point onto the plane through the orbit target.
    pub fn pick_on_target_plane(&self, screen: Point, bounds: Rectangle) -> glam::DVec3 {
        let forward = (self.target.as_vec3() - self.eye().as_vec3()).normalize_or(Vec3::NEG_Z);
        self.unproject_on_plane(screen, bounds, forward, self.target)
    }


    // ── ViewCube rotation matrix ───────────────────────────────────────────

    /// Returns the rotation matrix for the ViewCube.
    ///
    /// The camera quaternion maps canonical pose (+Z eye) → current view.
    /// The ViewCube needs the inverse so the cube stays world-aligned.
    /// Inverse of a unit quaternion = its conjugate.
    pub fn view_rotation_mat(&self) -> Mat4 {
        Mat4::from_quat(self.rotation.conjugate())
    }

    /// Carry the camera through a rigid model-space transform. BEDIT uses this
    /// when a transient UCS is baked into block-local geometry: the contents
    /// retain their on-screen framing while their canonical coordinates change.
    pub fn apply_rigid_transform(&mut self, transform: &acadrust::types::Transform) {
        let point = acadrust::types::Vector3::new(
            self.target.x,
            self.target.y,
            self.target.z,
        );
        let point = transform.apply(point);
        self.target = DVec3::new(point.x, point.y, point.z);

        let matrix = &transform.matrix.m;
        let rotation = glam::Mat3::from_cols(
            Vec3::new(matrix[0][0] as f32, matrix[1][0] as f32, matrix[2][0] as f32),
            Vec3::new(matrix[0][1] as f32, matrix[1][1] as f32, matrix[2][1] as f32),
            Vec3::new(matrix[0][2] as f32, matrix[1][2] as f32, matrix[2][2] as f32),
        );
        self.rotation = (Quat::from_mat3(&rotation) * self.rotation).normalize();
        self.sync_yaw_pitch();

        if let Some((min, max)) = self.model_bounds {
            let corners = [
                Vec3::new(min.x, min.y, min.z),
                Vec3::new(max.x, min.y, min.z),
                Vec3::new(min.x, max.y, min.z),
                Vec3::new(max.x, max.y, min.z),
                Vec3::new(min.x, min.y, max.z),
                Vec3::new(max.x, min.y, max.z),
                Vec3::new(min.x, max.y, max.z),
                Vec3::new(max.x, max.y, max.z),
            ];
            let mut new_min = Vec3::splat(f32::INFINITY);
            let mut new_max = Vec3::splat(f32::NEG_INFINITY);
            for corner in corners {
                let transformed = transform.apply(acadrust::types::Vector3::new(
                    corner.x as f64,
                    corner.y as f64,
                    corner.z as f64,
                ));
                let transformed =
                    Vec3::new(transformed.x as f32, transformed.y as f32, transformed.z as f32);
                new_min = new_min.min(transformed);
                new_max = new_max.max(transformed);
            }
            self.model_bounds = Some((new_min, new_max));
        }
    }

    /// The camera's roll — rotation about the view axis, in radians — the
    /// inverse of the `roll` argument to [`yaw_pitch_to_quat`]. Recovered by
    /// removing the yaw/pitch frame from the live rotation, so a saved view can
    /// store its twist and round-trip it. Exact for a plan view (the twisted-
    /// UCS case); approximate after a free 3D orbit.
    pub fn roll(&self) -> f32 {
        let q_yp = yaw_pitch_to_quat(self.yaw, self.pitch, 0.0);
        let q_roll = q_yp.conjugate() * self.rotation;
        // q_roll is (nominally) a rotation about Z; extract its angle.
        2.0 * q_roll.z.atan2(q_roll.w)
    }

    // ── Navigation ────────────────────────────────────────────────────────

    /// Arcball orbit: drag delta (dx, dy) in screen pixels.
    /// Orbit the view by a screen drag, turntable-style: horizontal drag yaws
    /// about world +Z, vertical drag pitches about the camera's (always
    /// horizontal) right axis — so the horizon never banks. `pivot` is the world
    /// point to revolve around (selection or model centre); `None` keeps the
    /// current target as the centre. (#229)
    pub fn orbit(&mut self, delta_x: f32, delta_y: f32, pivot: Option<DVec3>) {
        if delta_x.abs() < 1e-6 && delta_y.abs() < 1e-6 {
            return;
        }
        let old_rot = self.rotation;
        let speed = 0.005_f32;

        let yaw = Quat::from_rotation_z(-delta_x * speed);
        self.rotation = (yaw * self.rotation).normalize();
        let cam_right = self.rotation * Vec3::X;
        let cur_z = (self.rotation * Vec3::Z).z;
        let pitched =
            (Quat::from_axis_angle(cam_right, -delta_y * speed) * self.rotation).normalize();
        let new_z = (pitched * Vec3::Z).z;
        // Accept the pitch while the gaze stays clear of the poles, OR when it
        // moves back AWAY from a pole — so the straight-down default view (z = 1)
        // can still be tilted out of (a hard `< 0.9995` gate alone would freeze
        // small drags there). Only a step deeper INTO a pole is rejected.
        if new_z.abs() < 0.9995 || new_z.abs() < cur_z.abs() {
            self.rotation = pitched;
        }

        // Revolve the target around the pivot by the same rotation delta so the
        // view orbits the chosen centre instead of the fixed file centre (#229).
        // Do it in f64: casting `self.target - p` to f32 first quantizes the
        // offset to the drawing's extent scale (a large model at ANY origin, not
        // just UTM), so every orbit step nudged the whole view by a visible
        // fraction of a unit — the "jump" when starting a rotate on big files.
        if let Some(p) = pivot {
            let delta = self.rotation * old_rot.conjugate();
            let delta = glam::DQuat::from_xyzw(
                delta.x as f64,
                delta.y as f64,
                delta.z as f64,
                delta.w as f64,
            );
            self.target = p + delta * (self.target - p);
        }

        // Sync legacy yaw/pitch for hit-test functions.
        self.sync_yaw_pitch();
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (1.0 - delta * 0.1)).max(0.001);
    }

    /// World-space offset from `target` to the point under `screen` on the
    /// target plane. Computed in the camera frame (small numbers) and rotated
    /// to world — it never touches the large absolute target, so it stays
    /// precise at UTM-scale coordinates. For perspective this is the offset
    /// evaluated at the target plane (the correct pivot for zoom-to-cursor).
    fn cursor_offset_on_target_plane(&self, screen: Point, bounds: Rectangle) -> Vec3 {
        let ndc_x = (screen.x / bounds.width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen.y / bounds.height) * 2.0;
        let aspect = bounds.width / bounds.height;
        let half_h = self.ortho_size();
        let half_w = half_h * aspect;
        let cam_right = self.rotation * Vec3::X;
        let cam_up = self.rotation * Vec3::Y;
        cam_right * (ndc_x * half_w) + cam_up * (ndc_y * half_h)
    }

    pub fn zoom_about_point(&mut self, screen: Point, bounds: Rectangle, delta: f32) {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            self.zoom(delta);
            return;
        }

        // Keep the point under the cursor fixed by working with its offset
        // *relative to target* before and after the zoom. Both offsets are
        // small (camera-frame) numbers, so their difference is exact even at
        // UTM coordinates — the old absolute view_proj.inverse() picks each
        // carried ~0.5 m of f32 error that didn't cancel, making the whole
        // scene jump on every zoom step.
        let before = self.cursor_offset_on_target_plane(screen, bounds);
        self.zoom(delta);
        let after = self.cursor_offset_on_target_plane(screen, bounds);
        self.target += (before - after).as_dvec3();
    }

    /// Pan so the world point under the cursor tracks it: screen pixels are
    /// converted to world units via the ortho world-per-pixel scale of a
    /// viewport `viewport_height` pixels tall. Used by tiled panes where the
    /// pane height differs from the full canvas.
    pub fn pan_screen(&mut self, delta_x: f32, delta_y: f32, viewport_height: f32) {
        let wpp = if viewport_height > 0.0 {
            (2.0 * self.ortho_size()) / viewport_height
        } else {
            0.0
        };
        let cam_right = self.rotation * Vec3::X;
        let cam_up = self.rotation * Vec3::Y;
        self.target -= (cam_right * delta_x * wpp).as_dvec3();
        self.target += (cam_up * delta_y * wpp).as_dvec3();
    }

    /// The 8 corners of `min..max`, expressed in camera space relative to the
    /// current target: `x`/`y` span the screen plane, `z` runs along the eye
    /// direction. The offset is taken in f64 before the cast, so a corner at
    /// UTM scale doesn't lose the difference to cancellation.
    fn bounds_in_view(&self, min: Vec3, max: Vec3) -> [Vec3; 8] {
        let inv = self.rotation.inverse();
        let mut out = [Vec3::ZERO; 8];
        for (i, slot) in out.iter_mut().enumerate() {
            let corner = Vec3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            );
            *slot = inv * (corner.as_dvec3() - self.target).as_vec3();
        }
        out
    }

    /// Half-extent of `min..max` along the current eye direction (with the
    /// same 5% margin `ortho_depth_range` needs), measured from the target.
    /// This is exactly what `distance ± r` has to contain to avoid clipping.
    fn depth_extent_in_view(&self, min: Vec3, max: Vec3) -> f32 {
        let depth_r = self
            .bounds_in_view(min, max)
            .iter()
            .fold(0.0_f32, |m, c| m.max(c.z.abs()));
        (depth_r * 1.05).max(1.0)
    }

    /// Fit the camera to `min..max` — pose and depth both.
    ///
    /// Zoom and clipping are sized from DIFFERENT axes on purpose. The zoom must
    /// frame what the view actually shows (the extent across the screen plane),
    /// while near/far must span what lies along the eye direction. Sizing both
    /// from the 3-D diagonal — as this did — makes one far-off Z drag the
    /// horizontal zoom out with it: a 140-unit drawing carrying a single entity
    /// 800 km below its plane zoomed out to 800 km and became a dot. The two
    /// agree on a flat drawing, which is why it went unnoticed.
    pub fn fit_to_bounds(&mut self, min: Vec3, max: Vec3, aspect: f32) {
        self.target = ((min + max) * 0.5).as_dvec3();
        let corners = self.bounds_in_view(min, max);
        // Half-height the view needs so the box fits BOTH axes at the given
        // viewport aspect. The old circumscribed-radius rule fitted the box's
        // DIAGONAL into the height regardless of shape, which left a wide
        // drawing filling only a small central patch of the window (#364).
        let a = aspect.max(0.01);
        let half_h = corners
            .iter()
            .fold(0.0_f32, |m, c| m.max(c.y.abs()).max(c.x.abs() / a))
            .max(1e-6);
        // `ortho_size` (the half-height) is `distance * tan(fov/2)`, so invert
        // that to get the distance which just contains `half_h`, plus a small
        // border margin.
        self.distance = (half_h / (self.fov_y * 0.5).tan() * 1.1).max(1e-3);
        self.fit_depth_to_bounds(min, max);
    }

    /// Size only the near/far span to `min..max`, leaving the pose alone.
    ///
    /// Split out of [`fit_to_bounds`] for the camera restored from a file's
    /// saved view: that pose must not move, but its depth range still has to
    /// cover the model or geometry outside it is silently clipped away.
    pub fn fit_depth_to_bounds(&mut self, min: Vec3, max: Vec3) {
        // Cache the box so `ortho_depth_range` re-derives the near/far depth for
        // the live eye direction every frame — orbiting off the fitted pose then
        // never clips the drawing (#473). Keeping it tied to the model (not to
        // `distance`) also holds depth-buffer precision constant across zoom, so
        // coincident solids / meshes / wires never flip draw order.
        self.model_bounds = Some((min, max));
        // Also seed the scalar fallback for the fitted orientation, in case a
        // later reader consults it before the next projection.
        self.depth_half_range = self.depth_extent_in_view(min, max);
    }

    pub(crate) fn fitted_model_bounds(&self) -> Option<(Vec3, Vec3)> {
        self.model_bounds
    }

    // ── ViewCube snap ─────────────────────────────────────────────────────

    /// Snap to a canonical view direction (called by ViewCubeSnap).
    /// `eye_dir` is the unit vector from the target toward the camera.
    ///
    /// Up vector resolution:
    ///  1. Take the current up.
    ///  2. Pick the world axis (±X, ±Y, ±Z) whose dot product with the
    ///     current up is highest — skipping any axis (anti-)parallel to
    ///     the new gaze direction.
    ///  3. Project that axis onto the plane ⊥ `new_eye` and use that as
    ///     the new up.
    ///
    /// Result: small tilts collapse onto the nearest world axis (so the
    /// view always lands cleanly aligned), while genuine flips of the
    /// up-sense (e.g. orbited upside-down) are preserved.
    pub fn snap_to_direction(&mut self, eye_dir: Vec3, ucs: glam::Mat4) {
        let new_eye = eye_dir.normalize_or(Vec3::Z);
        let cur_up = self.rotation * Vec3::Y;
        // Candidate up axes are the UCS axes, not world X/Y/Z, so a face snap
        // lands the view square to the user's coordinate system (in-plane roll
        // included). Identity `ucs` reproduces the world-aligned snap.
        let ux = ucs.transform_vector3(Vec3::X).normalize_or(Vec3::X);
        let uy = ucs.transform_vector3(Vec3::Y).normalize_or(Vec3::Y);
        let uz = ucs.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
        let cardinals = [ux, -ux, uy, -uy, uz, -uz];
        let mut best_score = f32::NEG_INFINITY;
        let mut best_up = uz;
        for axis in cardinals {
            // Skip axes (nearly) collinear with the new gaze — they can't
            // serve as up because the projection onto the plane would
            // vanish.
            if axis.dot(new_eye).abs() > 0.999 {
                continue;
            }
            let score = axis.dot(cur_up);
            if score > best_score {
                best_score = score;
                best_up = axis;
            }
        }
        // Project the chosen axis onto the plane ⊥ new_eye and normalize.
        let projected = best_up - new_eye * best_up.dot(new_eye);
        let new_up = projected.normalize_or(if new_eye.dot(uz).abs() < 0.99 {
            (uz - new_eye * uz.dot(new_eye)).normalize()
        } else {
            (uy - new_eye * uy.dot(new_eye)).normalize()
        });
        let new_right = new_up.cross(new_eye).normalize();
        // Camera rotation columns: [cam_x | cam_y | cam_z] where
        // cam_z = eye_dir (canonical "+Z is toward eye"), cam_y = up.
        let mat = glam::Mat3::from_cols(new_right, new_up, new_eye);
        self.rotation = Quat::from_mat3(&mat).normalize();
        self.sync_yaw_pitch();
    }

    /// Snap to a canonical face view: looks along `eye_dir` with a fixed
    /// upright orientation — north (UCS +Y) up for top/bottom, world (UCS +Z)
    /// up for the side elevations. Unlike [`snap_to_direction`] this ignores
    /// the current up-sense, so a face click always lands square and never
    /// upside-down, even when the drawing opened with a twisted view.
    pub fn snap_to_face(&mut self, eye_dir: Vec3, ucs: glam::Mat4) {
        let new_eye = eye_dir.normalize_or(Vec3::Z);
        let uy = ucs.transform_vector3(Vec3::Y).normalize_or(Vec3::Y);
        let uz = ucs.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
        // Looking along the UCS Z axis (top/bottom) has no "world up" to use,
        // so fall back to north (+Y); every side view uses world up.
        let up_ref = if new_eye.dot(uz).abs() > 0.9 { uy } else { uz };
        let projected = up_ref - new_eye * up_ref.dot(new_eye);
        let new_up = projected.normalize_or(uy);
        let new_right = new_up.cross(new_eye).normalize();
        let mat = glam::Mat3::from_cols(new_right, new_up, new_eye);
        self.rotation = Quat::from_mat3(&mat).normalize();
        self.sync_yaw_pitch();
    }

    /// Jump to the default "home" view — a canonical top-down view (north up),
    /// expressed in the active UCS. Doubles as a "reset" for a twisted view.
    pub fn home_view(&mut self, ucs: glam::Mat4) {
        let dir = ucs.transform_vector3(Vec3::Z);
        self.snap_to_face(dir, ucs);
    }

    /// Roll the camera about its own view axis by `angle` radians. The gaze
    /// direction is unchanged; only the up-sense twists.
    pub fn roll_by(&mut self, angle: f32) {
        self.rotation = (self.rotation * Quat::from_rotation_z(angle)).normalize();
        self.sync_yaw_pitch();
    }

    /// Tip / spin the view 90° about a screen axis. `horizontal = false` tips
    /// up/down (rotation about the camera's right axis); `true` spins
    /// left/right (about the camera's up axis).
    pub fn nudge_90(&mut self, horizontal: bool, positive: bool) {
        let axis = if horizontal {
            self.rotation * Vec3::Y
        } else {
            self.rotation * Vec3::X
        };
        let ang = if positive {
            std::f32::consts::FRAC_PI_2
        } else {
            -std::f32::consts::FRAC_PI_2
        };
        let delta = Quat::from_axis_angle(axis, ang);
        self.rotation = (delta * self.rotation).normalize();
        self.sync_yaw_pitch();
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    /// Derive yaw and pitch from the current quaternion for the ViewCube
    /// hit-test functions (hit_test / hover_id). These use yaw/pitch to
    /// compute the same rotation matrix as the shader, so they must match.
    fn sync_yaw_pitch(&mut self) {
        // Eye direction in world space (canonical eye dir is +Z).
        let eye_dir = self.rotation * Vec3::Z;
        // pitch: angle above/below the XY plane.
        self.pitch = eye_dir.z.clamp(-0.999, 0.999).asin();
        // yaw: atan2(x, y) matches from_rotation_z(yaw) used in view_rotation_mat.
        self.yaw = eye_dir.x.atan2(eye_dir.y);
    }
}

// ── Free helpers ───────────────────────────────────────────────────────────

/// Build a rotation quaternion from yaw (rotation around Z) and pitch
/// (tilt toward Z). Matches the coordinate convention of the ViewCube
/// so snap angles continue to work unchanged.
///
/// Convention (Z-up, Y-forward):
///   yaw   = 0          → camera looks along +Y axis (front view)
///   pitch = PI/2       → camera looks down -Z (top view)
///   pitch = 0          → camera in the XY plane
/// Build a rotation quaternion from yaw, pitch and roll.
/// Positive yaw rotates the view direction clockwise when seen from above (Z-up).
/// Roll rotates the camera around its own view axis (post-multiplied so it
/// composes after the yaw/pitch gaze direction is set).
pub fn yaw_pitch_to_quat(yaw: f32, pitch: f32, roll: f32) -> Quat {
    // +yaw so ViewCube faces match camera direction (FRONT at yaw=0 = +Y world axis).
    let q_yaw = Quat::from_rotation_z(yaw);
    let q_pitch = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 - pitch);
    let q_roll = Quat::from_rotation_z(roll);
    (q_yaw * q_pitch * q_roll).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drawing 140 units wide carrying one entity 800 km below its plane must
    /// still zoom to the 140 units — the outlier belongs to the depth range, not
    /// to the zoom. (Sizing both from the 3-D diagonal made the drawing a dot.)
    #[test]
    fn a_far_off_plane_outlier_does_not_drag_the_zoom_out() {
        let flat_min = Vec3::new(-1200100.0, -800081.5, 0.0);
        let flat_max = Vec3::new(-1199960.0, -800000.0, 10.0);

        let mut flat = Camera::default();
        flat.fit_to_bounds(flat_min, flat_max, 1.0);

        // Same drawing, plus the benchmark's entity 800 km down.
        let mut deep = Camera::default();
        deep.fit_to_bounds(Vec3::new(flat_min.x, flat_min.y, -800017.5), flat_max, 1.0);

        // Top view: the outlier is along the eye direction, so the on-screen
        // framing must barely move.
        let ratio = deep.distance / flat.distance;
        assert!(
            (0.5..2.0).contains(&ratio),
            "zoom moved {ratio}x because of a depth-only outlier \
             (flat={}, deep={})",
            flat.distance,
            deep.distance
        );
    }

    /// …and that same outlier must be inside near/far, or it is clipped away.
    #[test]
    fn the_outlier_lands_inside_the_depth_range() {
        let min = Vec3::new(-1200100.0, -800081.5, -800017.5);
        let max = Vec3::new(-1199960.0, -800000.0, 10.0);
        let mut cam = Camera::default();
        cam.fit_to_bounds(min, max, 1.0);

        let (near, far) = cam.ortho_depth_range();
        // Depth of a point from the eye, along the view direction. Top view, so
        // the outlier at z=-800015 sits `distance + 800015`-ish away.
        let outlier = Vec3::new(-1200082.1, -800015.4, -800015.4);
        let local = cam.rotation.inverse() * (outlier.as_dvec3() - cam.target).as_vec3();
        let depth = cam.distance - local.z;
        assert!(
            depth > near && depth < far,
            "outlier at depth {depth} is outside near/far ({near}, {far})"
        );
    }

    /// A flat drawing must be unaffected: the 3-D diagonal and the screen-plane
    /// extent agree there, so the framing has to match the old behaviour.
    #[test]
    fn a_flat_drawing_frames_about_as_before() {
        let min = Vec3::new(-70.0, -40.0, 0.0);
        let max = Vec3::new(70.0, 40.0, 0.0);
        let mut cam = Camera::default();
        cam.fit_to_bounds(min, max, 1.0);
        // Old rule: distance = 3-D diagonal * 1.5.
        let old = (max - min).length() * 1.5;
        let ratio = cam.distance / old;
        assert!(
            (0.75..1.25).contains(&ratio),
            "flat framing drifted {ratio}x from the old rule (was {old}, now {})",
            cam.distance
        );
        // And the whole drawing still fits the half-height.
        let half_h = cam.ortho_size();
        assert!(
            half_h >= 40.0,
            "half-height {half_h} no longer contains the drawing"
        );
    }
}

#[cfg(test)]
mod rte_tests {
    use super::*;
    use iced::Rectangle;

    /// The relative-to-eye view carries rotation and nothing else, so where the
    /// camera stands must not reach it at all.
    ///
    /// It used to. The basis came from `look_at(eye, target)` with both cast to
    /// f32, and at survey coordinates that grid is half a metre across — so the
    /// difference between them, and the whole basis, jittered as the camera
    /// moved. A drawing placed at UTM shimmered.
    #[test]
    fn the_view_basis_does_not_depend_on_where_the_camera_stands() {
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        let mut at_origin = Camera::default();
        at_origin.target = glam::DVec3::ZERO;

        let mut at_utm = at_origin.clone();
        at_utm.target = glam::DVec3::new(639_792.184_2, 4_517_057.531_7, 12.5);

        let here = at_origin.view_proj_rte(bounds);
        let there = at_utm.view_proj_rte(bounds);
        for (a, b) in here.to_cols_array().iter().zip(there.to_cols_array().iter()) {
            assert_eq!(a, b, "the camera's position reached the view matrix");
        }
    }

    /// And it is the same basis as before, which the origin proves: there the
    /// old construction had no large coordinates to lose, so the two must
    /// agree exactly. Anything else would mean the refactor turned the view
    /// round rather than just keeping the coordinates out of it.
    #[test]
    fn the_basis_matches_what_two_positions_gave_where_they_were_exact() {
        for (yaw, pitch) in [(0.0, 1.2), (0.7, 0.3), (-2.1, -0.9)] {
            let mut camera = Camera::default();
            camera.target = glam::DVec3::ZERO;
            camera.rotation =
                Quat::from_rotation_z(yaw) * Quat::from_rotation_x(pitch);
            let up_dir = camera.rotation * Vec3::Y;

            let mut old = look_at_mat4(
                camera.eye().as_vec3(),
                camera.target.as_vec3(),
                up_dir,
            );
            old.w_axis = glam::vec4(0.0, 0.0, 0.0, 1.0);
            let new = {
                let mut view =
                    look_at_mat4(Vec3::ZERO, -(camera.rotation * Vec3::Z), up_dir);
                view.w_axis = glam::vec4(0.0, 0.0, 0.0, 1.0);
                view
            };
            for (a, b) in old.to_cols_array().iter().zip(new.to_cols_array().iter()) {
                assert!((a - b).abs() < 1e-6, "yaw {yaw} pitch {pitch}: {a} vs {b}");
            }
        }
    }
}
