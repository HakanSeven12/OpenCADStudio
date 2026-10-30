//! `viewport` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    axis_lock_apply, axis_lock_capture, drafting_axes, drafting_constrain, parse_coord,
    polar_constrain_if_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
    CoordKind,
};
use crate::app::{Message, ModalKind, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::model::object::GripApply;
use crate::scene::pick::grip::{
    find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit, GripEditMode, GripTarget,
};
use crate::scene::pick::interaction_index::ChainedWireSource;
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use codec::types::Color as AcadColor;
use codec::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};
use std::sync::Arc;

/// Pixel radius for grabbing a UCS-icon grip (origin dot or an axis tip).
const UCS_GRIP_HIT_PX: f32 = 9.0;
/// Pixel reach for hovering/clicking the icon body (origin, tips, or an arm).
const UCS_ICON_PICK_PX: f32 = 10.0;
/// Pixel half-width for clicking an icon arm (line segment).
const UCS_ICON_ARM_PX: f32 = 6.0;

/// Decide whether a selection press has become a drag. A fast mouse
/// movement alone must not turn the first click of a click-click rectangle
/// into a lasso. Hold-to-lasso gets the normal dwell; PICKDRAG=1 keeps its
/// deliberate immediate rectangular-drag shortcut.
fn selection_drag_should_start(dist2: f32, held_ms: u128, pick_drag_rect: bool) -> bool {
    const MIN_DIST2: f32 = 12.0 * 12.0;
    const RECT_FAST_DIST2: f32 = 24.0 * 24.0;
    dist2 > MIN_DIST2
        && (held_ms >= POLY_START_DELAY_MS || (pick_drag_rect && dist2 > RECT_FAST_DIST2))
}

/// For a short selection click, use the actual mouse-down position even if
/// the user starts moving immediately. Commands and active drag gestures
/// continue using the live cursor position.
fn selection_click_position(
    last: Point,
    pressed: Option<Point>,
    is_click: bool,
    no_command: bool,
) -> Point {
    if is_click && no_command {
        pressed.unwrap_or(last)
    } else {
        last
    }
}

#[cfg(test)]
mod selection_gesture_priority_tests {
    use super::*;

    #[test]
    fn fast_first_click_never_becomes_a_lasso_solely_from_cursor_speed() {
        assert!(!selection_drag_should_start(25.0 * 25.0, 20, false));
        assert!(!selection_drag_should_start(120.0 * 120.0, 80, false));
        assert!(!selection_drag_should_start(
            500.0 * 500.0,
            POLY_START_DELAY_MS - 1,
            false
        ));
        assert!(selection_drag_should_start(
            25.0 * 25.0,
            POLY_START_DELAY_MS,
            false
        ));
        assert!(!selection_drag_should_start(
            11.0 * 11.0,
            POLY_START_DELAY_MS + 500,
            false
        ));
    }

    #[test]
    fn pickdrag_rectangle_keeps_its_fast_drag_shortcut() {
        assert!(selection_drag_should_start(25.0 * 25.0, 20, true));
        assert!(!selection_drag_should_start(23.0 * 23.0, 20, true));
        assert!(selection_drag_should_start(
            13.0 * 13.0,
            POLY_START_DELAY_MS,
            true
        ));
    }

    #[test]
    fn quick_selection_click_anchors_where_the_button_went_down() {
        let start = Point::new(100.0, 100.0);
        let moved = Point::new(190.0, 135.0);
        assert_eq!(
            selection_click_position(moved, Some(start), true, true),
            start
        );
        // During a command or a genuine press-drag, use the moving cursor.
        assert_eq!(
            selection_click_position(moved, Some(start), true, false),
            moved
        );
        assert_eq!(
            selection_click_position(moved, Some(start), false, true),
            moved
        );
        assert_eq!(selection_click_position(moved, None, true, true), moved);
    }
}

/// Snapper coordinates are local to the active pane, while the overlay
/// consumes canvas coordinates. Apply the same translation to the visible
/// snap glyph and both Extension guide origins.
fn snap_pixels_to_canvas(hit: &mut crate::snap::SnapResult, pane: iced::Rectangle) {
    let translate = |point: &mut Point| {
        point.x += pane.x;
        point.y += pane.y;
    };
    translate(&mut hit.screen);
    if let Some(base) = hit.extension_base.as_mut() {
        translate(base);
    }
    if let Some(base) = hit.extension_base2.as_mut() {
        translate(base);
    }
}

#[cfg(test)]
mod extension_canvas_tests {
    use super::*;

    #[test]
    fn translates_extension_anchors_with_the_snap_marker() {
        let mut hit = crate::snap::SnapResult {
            world: glam::DVec3::ZERO,
            screen: Point::new(150.0, 75.0),
            snap_type: crate::snap::SnapType::Extension,
            tangent_obj: None,
            extension_base: Some(Point::new(100.0, 150.0)),
            extension_base2: Some(Point::new(75.0, 200.0)),
            extension_origin: None,
            extension_dir: None,
            viewport: None,
            source: None,
            secondary_source: None,
            model_point: None,
        };
        let pane = iced::Rectangle {
            x: 32.0,
            y: 56.0,
            width: 600.0,
            height: 400.0,
        };
        snap_pixels_to_canvas(&mut hit, pane);
        assert_eq!(hit.screen, Point::new(182.0, 131.0));
        assert_eq!(hit.extension_base, Some(Point::new(132.0, 206.0)));
        assert_eq!(hit.extension_base2, Some(Point::new(107.0, 256.0)));
    }
}

fn pt_pt_d2(a: Point, b: Point) -> f32 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}

/// What one scroll delta asks the viewport to do.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ScrollIntent {
    /// Wheel notches to zoom by; positive zooms in.
    Zoom { notches: f32 },
    /// Trackpad movement to pan by, in screen pixels.
    ///
    Pan { dx: f32, dy: f32 },
}

/// How far a ZOOM Dynamic drag must travel vertically before the magnifier
/// cursor flips between "+" and "-". Without the dead zone a hand holding
/// still at the top of a drag strobes the glyph on every jittered pixel.
const ZOOM_DIR_FLIP_PX: f32 = 2.0;

/// Scroll zooms unless the user has enabled touchpad panning.
///
/// Delta units are not a reliable device identifier: smooth-scrolling mice
/// can report pixels on macOS too. Honor the explicit touchpad option for
/// both kinds of delta, with Ctrl+scroll always available for zooming.
fn scroll_intent(delta: mouse::ScrollDelta, touchpad_pan: bool, ctrl: bool) -> ScrollIntent {
    /// Screen pixels one wheel line pans.
    const LINE_PX: f32 = 40.0;
    // A notched wheel reports lines and a trackpad never does there, so on
    // macOS a line delta zooms even with touchpad panning on.
    let wheel_notch =
        cfg!(target_os = "macos") && matches!(delta, mouse::ScrollDelta::Lines { .. });
    if touchpad_pan && !ctrl && !wheel_notch {
        return match delta {
            mouse::ScrollDelta::Pixels { x, y } => ScrollIntent::Pan { dx: x, dy: y },
            mouse::ScrollDelta::Lines { x, y } => ScrollIntent::Pan {
                dx: x * LINE_PX,
                dy: y * LINE_PX,
            },
        };
    }
    match delta {
        mouse::ScrollDelta::Pixels { y, .. } => ScrollIntent::Zoom { notches: y * 0.01 },
        mouse::ScrollDelta::Lines { y, .. } => ScrollIntent::Zoom { notches: y },
    }
}

/// The camera step that pinches the view 1:1 with the fingers, from AppKit's
/// magnification delta (0.05 = the fingers moved 5% further apart).
///
/// AppKit reports the increment of a scale factor, and a 5% wider pinch has to
/// leave the view 5% closer, so the step is the one that scales the camera
/// distance by exactly `1 / (1 + m)`. The camera is asked for that step
/// instead of being told it: inverting the zoom law here by hand (it read
/// `1 - step / 10 = 1 / (1 + m)`) is what made the pinch stop tracking the
/// fingers the moment the law itself changed.
///
/// The ratio is bounded because a magnification of -1 or beyond — which the
/// gesture should never report — has no finite answer, and because no single
/// gesture event has any business moving the view a hundredfold.
fn pinch_zoom_steps(magnification: f32) -> f32 {
    let ratio = (1.0 / (1.0 + magnification)).clamp(0.01, 100.0);
    crate::scene::view::camera::Camera::zoom_steps_for_ratio(ratio)
}

/// Whether a command point keeps the elevation of its snap instead of being
/// clamped to the world XY plane.
///
/// A genuine object snap carries its Z into the command point, so 3D vertices
/// stay snappable. Grid snaps, misses and entity picks stay on the
/// construction plane, preserving the long-standing 2D behavior exactly
/// (a planar snap still resolves to z = 0 as before).
pub(in crate::app) fn snap_keeps_elevation(hit: Option<crate::snap::SnapType>) -> bool {
    matches!(hit, Some(t) if t != crate::snap::SnapType::Grid)
}

/// Point on a command's construction axis (height / distance steps). A
/// genuine object snap supplies its own coordinate along the axis, so a
/// height or push distance seats on existing geometry — as axis grips do.
/// Otherwise the cursor maps onto the axis on screen.
fn command_axis_point(
    snap: Option<crate::snap::SnapResult>,
    cursor: Point,
    bounds: iced::Rectangle,
    view: glam::Mat4,
    eye: glam::DVec3,
    origin: glam::DVec3,
    direction: glam::DVec3,
) -> Option<glam::DVec3> {
    match snap.filter(|hit| snap_keeps_elevation(Some(hit.snap_type))) {
        Some(hit) => {
            let axis = direction.try_normalize()?;
            Some(origin + axis * (hit.world - origin).dot(axis))
        }
        None => cursor_on_projected_axis(cursor, bounds, view, eye, origin, direction),
    }
}

fn cursor_on_projected_axis(
    cursor: Point,
    bounds: iced::Rectangle,
    view: glam::Mat4,
    eye: glam::DVec3,
    origin: glam::DVec3,
    direction: glam::DVec3,
) -> Option<glam::DVec3> {
    let direction = direction.normalize_or_zero();
    if direction.length_squared() <= 1e-12 {
        return None;
    }
    let relative = (origin - eye).as_vec3();
    let start = view * relative.extend(1.0);
    let next = view * (relative + direction.as_vec3()).extend(1.0);
    if start.w.abs() <= 1e-9 {
        return None;
    }
    let delta = next - start;
    let ndc = start.truncate() / start.w;
    let derivative = glam::Vec2::new(
        (delta.x * start.w - start.x * delta.w) / start.w.powi(2),
        (delta.y * start.w - start.y * delta.w) / start.w.powi(2),
    );
    let screen_origin = glam::Vec2::new(
        (ndc.x + 1.0) * 0.5 * bounds.width,
        (1.0 - ndc.y) * 0.5 * bounds.height,
    );
    let screen_direction = glam::Vec2::new(
        derivative.x * 0.5 * bounds.width,
        -derivative.y * 0.5 * bounds.height,
    );
    let length_squared = screen_direction.length_squared();
    if length_squared <= 1e-8 {
        return None;
    }
    let cursor = glam::Vec2::new(cursor.x, cursor.y);
    let screen = screen_origin
        + screen_direction * (cursor - screen_origin).dot(screen_direction) / length_squared;
    let target = glam::Vec2::new(
        screen.x / bounds.width * 2.0 - 1.0,
        1.0 - screen.y / bounds.height * 2.0,
    );
    let x_denominator = target.x * delta.w - delta.x;
    let y_denominator = target.y * delta.w - delta.y;
    if x_denominator.abs().max(y_denominator.abs()) <= 1e-9 {
        return None;
    }
    let distance = if x_denominator.abs() >= y_denominator.abs() {
        (start.x - target.x * start.w) / x_denominator
    } else {
        (start.y - target.y * start.w) / y_denominator
    };
    distance
        .is_finite()
        .then_some(origin + direction * distance as f64)
}

/// The point under `cursor` on the plane through `origin` with `normal`.
fn cursor_on_plane(
    cursor: Point,
    bounds: iced::Rectangle,
    view: glam::Mat4,
    eye: glam::DVec3,
    origin: glam::DVec3,
    normal: glam::DVec3,
) -> Option<glam::DVec3> {
    let inverse = view.as_dmat4().inverse();
    let x = cursor.x as f64 / bounds.width as f64 * 2.0 - 1.0;
    let y = 1.0 - cursor.y as f64 / bounds.height as f64 * 2.0;
    let near = inverse.project_point3(glam::DVec3::new(x, y, 0.0));
    let far = inverse.project_point3(glam::DVec3::new(x, y, 1.0));
    let direction = far - near;
    let along = direction.dot(normal);
    if along.abs() <= 1e-12 {
        return None;
    }
    let t = ((origin - eye) - near).dot(normal) / along;
    t.is_finite().then(|| eye + near + direction * t)
}

fn is_added_polyline_vertex(
    original: &AcadEntityType,
    current: &AcadEntityType,
    vertex_id: usize,
) -> bool {
    match (original, current) {
        (AcadEntityType::LwPolyline(before), AcadEntityType::LwPolyline(after)) => {
            after.vertices.len() == before.vertices.len() + 1 && vertex_id < after.vertices.len()
        }
        (AcadEntityType::Polyline2D(before), AcadEntityType::Polyline2D(after)) => {
            after.vertices.len() == before.vertices.len() + 1 && vertex_id < after.vertices.len()
        }
        _ => false,
    }
}

/// Return the source bulge when `vertex_id` is the provisional point inserted
/// into an arc segment. Straight segments stay straight during placement.
fn added_arc_bulge(
    original: &AcadEntityType,
    current: &AcadEntityType,
    vertex_id: usize,
) -> Option<f64> {
    if !is_added_polyline_vertex(original, current, vertex_id) {
        return None;
    }
    let prev = vertex_id.checked_sub(1)?;
    let (n, closed, bulge) = match original {
        AcadEntityType::LwPolyline(polyline) => (
            polyline.vertices.len(),
            polyline.is_closed,
            polyline.vertices.get(prev)?.bulge,
        ),
        AcadEntityType::Polyline2D(polyline) => (
            polyline.vertices.len(),
            polyline.is_closed(),
            polyline.vertices.get(prev)?.bulge,
        ),
        _ => return None,
    };
    ((closed || prev + 1 < n) && bulge.abs() >= 1e-9).then_some(bulge)
}

/// Squared distance from `p` to the segment `a`–`b`.
fn pt_seg_d2(p: Point, a: Point, b: Point) -> f32 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 1e-6 {
        (((p.x - a.x) * vx + (p.y - a.y) * vy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    pt_pt_d2(p, Point::new(a.x + t * vx, a.y + t * vy))
}

/// Which UCS grip (if any) the cursor `p` is on.
fn ucs_grip_under(p: Point, h: &crate::ui::overlay::UcsIconHit) -> Option<crate::app::UcsGripKind> {
    let r2 = UCS_GRIP_HIT_PX * UCS_GRIP_HIT_PX;
    if pt_pt_d2(p, h.origin) <= r2 {
        Some(crate::app::UcsGripKind::Origin)
    } else if pt_pt_d2(p, h.tips[0]) <= r2 {
        Some(crate::app::UcsGripKind::XAxis)
    } else if pt_pt_d2(p, h.tips[1]) <= r2 {
        Some(crate::app::UcsGripKind::YAxis)
    } else {
        None
    }
}

/// True when `p` is over the icon body (origin, a tip, or an arm) — the pick
/// region for hover-highlight and select.
fn over_ucs_icon(p: Point, h: &crate::ui::overlay::UcsIconHit) -> bool {
    let pick2 = UCS_ICON_PICK_PX * UCS_ICON_PICK_PX;
    if pt_pt_d2(p, h.origin) <= pick2 || h.tips.iter().any(|t| pt_pt_d2(p, *t) <= pick2) {
        return true;
    }
    let arm2 = UCS_ICON_ARM_PX * UCS_ICON_ARM_PX;
    h.tips.iter().any(|t| pt_seg_d2(p, h.origin, *t) <= arm2)
}

impl OpenCADStudio {
    fn active_construction_ray(
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
    fn drafting_point(
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
        let along = |dir: glam::DVec3| {
            cursor_on_projected_axis(screen, bounds, view_rot, eye, base, dir)
        };
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

    fn active_axis_lock(
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
                let height = self.tabs[tab].scene.selection.borrow().view.vp_size.1.max(1.0);
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

    fn active_otrack_hit(
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

        let mut hit = self.snapper.otrack_snap(
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
        use codec::entities::ViewportRenderMode as M;
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
    fn profile_pick_point(
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
    fn solid_pick_point(
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
        let camera = edit_cam.clone().unwrap_or_else(|| scene.camera.borrow().clone());
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

    /// Projection + hit-test wires for the active pane. Inside a floating
    /// viewport (`edit_cam` Some) it returns the viewport camera and the live
    /// **model** wires, so wire / hatch picking lands on the entity under the
    /// cursor exactly where the GPU draws it; otherwise the model/paper camera
    /// and the normal hit-test wires. `bounds` is the pane-local rectangle.

    /// SELECTIONPREVIEW while a selection box is drawn (#2172): what the box
    /// would catch with its corner at `p` (canvas pixels) rolls over before
    /// the release commits it, with the release's filters.
    fn refresh_box_selection_preview(&mut self, i: usize, p: iced::Point) {
        let enabled = match self.tabs[i].active_cmd.as_ref() {
            None => self.model_space.selection_preview & 1 != 0,
            Some(command) => {
                command.is_selection_gathering() && self.model_space.selection_preview & 2 != 0
            }
        };
        let navigating =
            self.tabs[i].orbit_mode || self.tabs[i].pan_mode || self.tabs[i].zoom_dynamic_mode;
        let (anchor, crossing) = {
            let sel = self.tabs[i].scene.selection.borrow();
            let anchor = sel.gesture.box_anchor.filter(|_| !sel.gesture.poly_active);
            let crossing = match anchor {
                Some(a) if !sel.gesture.box_crossing_locked => p.x < a.x,
                _ => sel.gesture.box_crossing,
            };
            (anchor, crossing)
        };
        let Some(anchor) = anchor.filter(|_| enabled && !navigating) else {
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
        let (mut handles, _) =
            self.box_selection_handles(i, &edit_cam, bounds, local(anchor), local(p), crossing);
        handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
        self.filter_handles_reject_locked(i, &mut handles);
        self.tabs[i].scene.set_box_hover_highlights(&handles);
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
        let area_candidates = self.tabs[i].scene.interaction_candidates_in_aabb(
            all_wires,
            world_aabb,
            [a.x.min(p.x), a.y.min(p.y), a.x.max(p.x), a.y.max(p.y)],
            view_rot,
            eye,
            bounds,
        );
        timing.candidates = elapsed(t_sel);
        let t_hit = crate::perf::enabled().then(Instant::now);
        let scene = &self.tabs[i].scene;
        let candidate_handles = scene.interaction_candidate_handles(&area_candidates);
        timing.handles = elapsed(t_hit);
        let mut handles: Vec<Handle> =
            scene::pick::hit_test::box_hit(a, p, crossing, &area_candidates, view_rot, eye, bounds)
                .into_iter()
                .filter_map(|s| Scene::handle_from_wire_name(s))
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
        timing.hit = elapsed(t_hit);
        (handles, timing)
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
            .filter(|hit| self.last_point.is_none_or(|base| {
                (hit.world - base).length_squared() > 1e-16
            }))
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

        GripEdit {
            handle,
            grip_id,
            origin_world: world,
            last_world: world,
            mode: GripEditMode::Stretch,
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
                let same = self
                    .grip_hover
                    .as_ref()
                    .map_or(false, |h| h.document == self.tabs[i].id && h.handle == handle && h.grip_id == grip_id);
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
                        .map_or(false, |h| h.started.elapsed().as_millis() >= HOVER_OPEN_MS)
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

    pub(super) fn on_tick(&mut self, t: Instant) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.update(t - self.start);
        // Dynamic dimensions keep their screen size across zoom bands.
        self.tabs[i].scene.refresh_dynamic_dimension_scales(false);
        // Underlays are rasterised again for the zoom once it settles.
        self.tabs[i].scene.refresh_underlay_resolution(t);

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

        Task::none()
    }

    pub(super) fn on_set_render_mode(
        &mut self,
        mode: codec::entities::ViewportRenderMode,
    ) -> Task<Message> {
        use codec::entities::ViewportRenderMode as M;
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
        mode: codec::entities::ViewportRenderMode,
        label: &str,
    ) {
        use codec::entities::ViewportRenderMode as M;
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

    pub(super) fn on_cursor_moved(
        &mut self,
        p: Point,
        expected_viewport: Option<codec::Handle>,
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
        Task::none()
    }

    /// A client automation pick (`getpoint`/`user_select`) is waiting on tab
    /// `i`'s document — the cursor behaves like a command point pick: the snap
    /// engine runs on every move and the marker stays until the pick settles.
    pub(super) fn client_pick_pending(&self, i: usize) -> bool {
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

    pub(in crate::app) fn on_viewport_move(&mut self, p: Point) -> Task<Message> {
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
        self.refresh_box_selection_preview(i, p);
        // Modifier-driven selection must be known before cursor_plane/axis and
        // drafting constraints are read, not merely before the final callback.
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
        }
        let perf_move = crate::perf::enabled();
        let move_started = Instant::now();

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

        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.input.last_move_pos = Some(p);

        if sel.input.left_down {
            let press = sel.input.left_press_pos.unwrap_or(p);
            let dx = p.x - press.x;
            let dy = p.y - press.y;
            let dist2 = dx * dx + dy * dy;
            let elapsed_ms = sel
                .input
                .left_press_time
                .map(|t| Instant::now().duration_since(t).as_millis())
                .unwrap_or(u128::MAX);
            // In default PICKDRAG=0, cursor speed must never promote a
            // quick click into a lasso. The 250 ms press dwell distinguishes
            // an intentional hold-drag from a fast click-move-click box.
            // PICKDRAG=1 retains fast press-drag rectangular selection;
            // grip dragging is handled separately.
            let dragged = selection_drag_should_start(dist2, elapsed_ms, self.pick_drag_rect);
            if !sel.input.left_dragging && dragged {
                sel.input.left_dragging = true;
                if self.pick_drag_rect {
                    // PICKDRAG 1 (#226): press-drag spans a RECTANGLE
                    // marquee — drive the existing box machinery (its
                    // overlay and completion) instead of the lasso.
                    sel.gesture.box_anchor = Some(press);
                    sel.gesture.box_current = Some(p);
                    if !sel.gesture.box_crossing_locked {
                        sel.gesture.box_crossing = crate::scene::selection_box_crossing(press, p);
                    }
                } else {
                    sel.gesture.poly_active = true;
                    sel.gesture.poly_crossing = p.x < press.x;
                    sel.gesture.poly_points.clear();
                    sel.gesture.poly_points.push(press);
                    sel.gesture.poly_points.push(p);
                }
            } else if sel.input.left_dragging && sel.gesture.poly_active {
                if sel.gesture.poly_points.last().map_or(true, |lp| {
                    let ddx = p.x - lp.x;
                    let ddy = p.y - lp.y;
                    ddx * ddx + ddy * ddy > 16.0
                }) {
                    sel.gesture.poly_points.push(p);
                }
            } else if sel.input.left_dragging {
                if let Some(a) = sel.gesture.box_anchor {
                    sel.gesture.box_current = Some(p);
                    if !sel.gesture.box_crossing_locked {
                        sel.gesture.box_crossing = crate::scene::selection_box_crossing(a, p);
                    }
                }
            }
        } else if sel.gesture.box_anchor.is_some() {
            sel.gesture.box_current = Some(p);
            if let Some(a) = sel.gesture.box_anchor {
                if !sel.gesture.box_crossing_locked {
                    sel.gesture.box_crossing = crate::scene::selection_box_crossing(a, p);
                }
            }
        }

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
                    let dx = if self.tabs[i].zoom_dynamic_mode { dx } else { 0.0 };
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

        // ── Custom plugin-grip drag (API v11) ────────────────────────────
        // The plugin's preview is the only live feedback: no host ghost, no
        // entity mutation, the anchor entity never moves by host action. The
        // point is computed like a built-in drag's (drafting plane through
        // the grip's own elevation), never snapped (plugin semantics), and
        // the declared axis is a HARD constraint.
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
            let point = match active.axis {
                Some(axis) => {
                    let (view_rot, eye) = match &edit_cam {
                        Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                        None => {
                            let cam = self.tabs[i].scene.camera.borrow();
                            (cam.view_proj_rte(bounds), cam.eye())
                        }
                    };
                    cursor_on_projected_axis(p, bounds, view_rot, eye, active.origin_world, axis)
                        .unwrap_or(raw)
                }
                None => raw,
            };
            self.tabs[i].last_cursor_world = point;
            self.tabs[i].last_cursor_screen = p_full;
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
                let preview_handles = self.tabs[i].scene.parametric_connected_handles(
                    self.tabs[i].current_parametric_scope(),
                    &edited_handles,
                    true,
                );
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
                    && square_grip;
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
            let snap_hit = self.snapper.snap(
                raw,
                p,
                &snap_candidates,
                view_rot,
                eye,
                bounds,
                go,
                gr,
                construction_ray,
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
                        let apply = if target.is_translate {
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
            for (handle, grip_id, apply) in actions {
                if !rebuilt_arcs.contains(&handle) {
                    self.tabs[i].scene.apply_grip(handle, grip_id, apply);
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
            let refreshed_targets: Vec<_> = self.tabs[i]
                .selected_grip_handles
                .iter()
                .copied()
                .zip(self.tabs[i].selected_grips.iter())
                .map(|(handle, grip)| ((handle, grip.id), grip.world))
                .collect();
            if let Some(active) = self.tabs[i].active_grip.as_mut() {
                for target in &mut active.targets {
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
            let snap_hit = self.snapper.snap(
                cursor_world,
                p,
                &snap_candidates,
                view_rot,
                eye,
                bounds,
                go,
                gr,
                None,
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
        if !dragging && (self.tabs[i].active_cmd.is_none() || deferred_command_hover || is_gathering_cmd) {
            // On dense drawings, clearing a rollover immediately schedules a
            // second full scene frame just as motion resumes. Keep the previous
            // highlight until the next settled pick replaces it.
            if self.tabs[i].scene.last_tess_wires.get() < crate::app::HOVER_DWELL_DENSE_WIRES {
                self.tabs[i].scene.set_hover_highlight(None);
            }
            self.hover_dwell = Some(crate::app::HoverDwell {
                last_move_at: Instant::now(),
                point: p,
                tile_size: vp_size,
                tab: i,
            });
        } else {
            // Suppress the rollover during a command or a drag.
            self.tabs[i].scene.set_hover_highlight(None);
            self.hover_dwell = None;
        }

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
                    || self.snap_dwell.as_ref().is_some_and(|dwell| {
                        dwell.discrete_preview_due_at(Instant::now())
                    }));
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
                let snap =
                    self.snapper
                        .snap_discrete_preview(p, &snap_candidates, view_rot, eye, bounds);
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
                // Keep the previous marker only while it is still inside the
                // current aperture. This avoids flicker between throttled
                // samples without showing a snap that is no longer nearby.
                self.tabs[i].snap_result.filter(|hit| {
                    let dx = hit.screen.x - p_full.x;
                    let dy = hit.screen.y - p_full.y;
                    dx * dx + dy * dy <= self.snapper.osnap_radius_px.powi(2)
                })
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
            let world = match snap {
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
            // Fast previews are not stable OTRACK observations. Start its
            // acquisition timer only after movement settles, not while a
            // cursor brushes a candidate or leaves an old candidate behind.
            // Remaining within the acquisition aperture preserves the acquired
            // latch so stationary ticks do not toggle or restart repeatedly;
            // genuine departure resets the latch so returning can reacquire.
            let within_aperture = self.snapper.last_snap_world.is_some_and(|target| {
                snap.as_ref()
                    .is_some_and(|hit| (hit.world - target).length() < 0.001)
            });
            if within_aperture {
                if !self.snapper.dwell_acquired {
                    self.snapper.last_snap_world = None;
                    self.snapper.dwell_since = None;
                }
            } else {
                self.snapper.dwell_acquired = false;
                self.snapper.last_snap_world = None;
                self.snapper.dwell_since = None;
            }
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

                self.snapper.snap_with_additional_construction_rays(
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
                )
            };
            // Restore discrete snap targets from STRETCH's original snapshot
            // even though the corresponding resident objects are hidden.
            let original_snap = self.command_original_snap_hit(i, p, view_rot, eye, bounds);
            self.tabs[i].snap_result = crate::snap::merge_snap(
                self.tabs[i].snap_result, original_snap, p,
            );
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

            let effective = {
                // Ortho / polar from the last point keep its elevation.
                let mut drafted_elevation = false;
                let mut pt: glam::DVec3 =
                    if let (Some(dir), Some(base)) = (axis_lock, self.last_point) {
                        let point = self.tabs[i]
                            .snap_result
                            .map(|snap| snap.world)
                            .unwrap_or(cursor_world);
                        axis_lock_apply(point, base, dir)
                    } else if let Some(h) = otrack_hit.filter(|_| {
                        // Exact object snaps win over tracking alignment, but
                        // Extension is a construction guide and must remain
                        // eligible for OTRACK intersections.
                        !self.tabs[i].snap_result.is_some_and(|s| {
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
                        let mut pt = self.tabs[i]
                            .snap_result
                            .map(|s| s.world)
                            .unwrap_or(cursor_world);
                        let osnap_locked = self.tabs[i]
                            .snap_result
                            .is_some_and(|s| s.snap_type != crate::snap::SnapType::Grid);
                        let drafted = (!osnap_locked
                            && !free_point
                            && !uses_command_cursor_plane)
                            .then_some(self.last_point)
                            .flatten()
                            .and_then(|base| {
                                let hit = self
                                    .drafting_point(i, p, pt, base, view_rot, eye, bounds)?;
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
                                self.otrack_active =
                                    Some((base, (point - base).normalize()));
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
                    && !snap_keeps_elevation(self.tabs[i].snap_result.map(|s| s.snap_type))
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
                        command_axis_point(
                            self.tabs[i].snap_result,
                            p,
                            bounds,
                            view_rot,
                            eye,
                            origin,
                            direction,
                        )
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
            let lps = anchor.map(|a| proj(a));
            let drs = dyn_ref.map(|r| proj(r));
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
            let mut previews = if let Some((wires, hidden)) =
                self.point_modifier_preview(i, effective, modifier_tangent)
            {
                modifier_hidden = Some(hidden);
                wires
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
                    let entities = cmd.preview_entities(&tab.scene.document, effective);
                    p.extend(tab.scene.wires_for_entities(&entities));
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
                .and_then(|s| Scene::handle_from_wire_name(s))
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
                let hover_handle = hovered.unwrap_or(codec::Handle::NULL);
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
                        xy_min_max.0[0], xy_min_max.0[1],
                        xy_min_max.1[0], xy_min_max.1[1],
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
                    if let Some(mut box_preview) = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .and_then(|command| {
                            command.on_drag_selection_preview(
                                &fence, window, crossing, &nearby,
                            )
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
                            taper_widths: Vec::new(),
                            pattern_stations: Vec::new(),
                            world_width: 0.0,
                            depth_override: None,
                            display_visible: true,
                            snap_only: false,
                            plot_visible: true,
                            fill_is_3d: false,
                            fill_is_2d_solid: false,
                            render_instance: None,
                            pick_tris: Vec::new(),
                            pick_tris_low: Vec::new(),
                            dash_from_start: false,
                            dash_align_end: None,
                            text_verts: Vec::new(),
                            name: "__polar_guide__".into(),
                            points: vec![
                                [far_neg.x, far_neg.y, far_neg.z],
                                [far_pos.x, far_pos.y, far_pos.z],
                            ],
                            points_low: Vec::new(),
                            color: [0.2, 0.7, 0.9, 0.6],
                            selected: false,
                            aci: 0,
                            pattern_length: 0.8,
                            pattern: [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                            line_weight_px: 1.0,
                            snap_pts: vec![],
                            tangent_geoms: vec![],
                            key_vertices: vec![],
                            aabb: crate::scene::WireModel::UNBOUNDED_AABB,
                            plinegen: true,
                            fill_tris: vec![],
                            fill_tris_low: Vec::new(),

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
            });
        } else if !self.client_pick_pending(i) {
            // Idle (no command, no pending client pick): the snap marker is
            // command-only, so clear it — a pending client pick keeps the
            // marker its move-time snap block above computed.
            self.tabs[i].snap_result = None;
        }

        self.sync_dyn_fields();
        let move_ms = move_started.elapsed().as_secs_f64() * 1000.0;
        if perf_move
            && (self.tabs[i].active_cmd.is_some() || self.tabs[i].scene.active_viewport.is_some())
            && move_ms >= 16.7
        {
            let mode = if self.tabs[i].active_cmd.is_some() {
                "command"
            } else {
                "MSPACE"
            };
            crate::perf_record!("[perf] pointer-move mode={mode:<7} {:>7.1}ms", move_ms,);
        }
        Task::none()
    }

    /// The navigation style's action for `held` and the modifiers now down.
    fn nav_style_action(
        &self,
        held: crate::app::settings::NavButtons,
        dragging: bool,
    ) -> Option<crate::app::settings::NavAction> {
        self.navigation_style.action(
            held,
            self.shift_down,
            self.ctrl_down,
            self.alt_down,
            dragging,
        )
    }

    /// What the navigation drag under way does: an armed tool decides, then
    /// CATIA's zoom latch, then the style for the buttons held. A drag the
    /// style has no meaning for pans.
    fn nav_drag_action(&self, i: usize) -> crate::app::settings::NavAction {
        use crate::app::settings::NavAction;
        let tab = &self.tabs[i];
        if tab.zoom_dynamic_mode {
            NavAction::Zoom
        } else if tab.orbit_mode {
            NavAction::Orbit
        } else if tab.pan_mode {
            NavAction::Pan
        } else if self.nav_zoom_latched {
            NavAction::Zoom
        } else {
            self.nav_style_action(self.nav_held, true)
                .unwrap_or(NavAction::Pan)
        }
    }

    /// Starts the drag a pending left/right press turns into once it moves,
    /// and the modifier-held drag of the Touchpad style (ended when its keys
    /// are up).
    fn nav_pointer_moved(&mut self, i: usize, p: Point) {
        use crate::app::settings::{NavButtons, NavigationStyle};
        if let Some((_, from, _)) = &self.nav_pending {
            let (dx, dy) = (p.x - from.x, p.y - from.y);
            if dx * dx + dy * dy > 16.0 {
                let from = *from;
                self.nav_pending = None;
                self.nav_start_drag(i, from);
            }
            return;
        }
        if self.navigation_style != NavigationStyle::Touchpad
            || self.nav_held != NavButtons::default()
        {
            return;
        }
        let keyed = self.nav_style_action(NavButtons::default(), false).is_some();
        let (dragging, previous) = {
            let sel = self.tabs[i].scene.selection.borrow();
            (sel.input.middle_down, sel.input.last_move_pos)
        };
        if keyed && !dragging {
            if let Some(previous) = previous {
                self.nav_start_drag(i, previous);
            }
        } else if !keyed && dragging && self.nav_drag_by_button {
            self.nav_end_drag(i);
        }
    }

    /// Run a navigation drag from `from` on the middle-button movement path,
    /// dropping whatever the left or right button had begun.
    fn nav_start_drag(&mut self, i: usize, from: Point) {
        self.clear_navigation_hover(i);
        self.tabs[i].scene.remember_current_view();
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.clear_left_selection_gesture();
        sel.input.left_down = false;
        sel.input.right_down = false;
        sel.input.middle_down = true;
        sel.input.middle_last_pos = Some(from);
        sel.input.middle_pan_from = None;
        drop(sel);
        self.nav_drag_by_button = true;
    }

    fn nav_end_drag(&mut self, i: usize) {
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.input.middle_down = false;
        sel.input.middle_last_pos = None;
        sel.orbit.pivot = None;
        drop(sel);
        self.nav_drag_by_button = false;
        self.nav_zoom_latched = false;
        self.arm_hover_after_navigation(i);
    }

    /// A left (`left`) or right press: the navigation style takes it when
    /// the buttons now held drag the view. A press that may still be a
    /// click waits for the pointer to move. `None` lets the press run its
    /// normal path.
    pub(in crate::app) fn nav_press(
        &mut self,
        left: bool,
        press: &Message,
    ) -> Option<Task<Message>> {
        self.nav_resync_held();
        if left {
            self.nav_held.left = true;
        } else {
            self.nav_held.right = true;
        }
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.orbit_mode || tab.pan_mode || tab.zoom_dynamic_mode || tab.is_start {
            return None;
        }
        let (pos, dragging) = {
            let sel = tab.scene.selection.borrow();
            (sel.input.last_move_pos?, sel.input.middle_down)
        };
        let held = self.nav_held;
        if dragging {
            self.nav_style_action(held, true)?;
            if !left && held.middle {
                self.nav_right_press = Some(pos);
            }
        } else {
            self.nav_style_action(held, false)?;
            let other_held = if left { held.right } else { held.left };
            if other_held {
                // Both buttons together navigate at once; whatever the
                // other one had begun gives way.
                if left {
                    self.nav_claimed.right = true;
                } else {
                    self.nav_claimed.left = true;
                }
                self.nav_pending = None;
                self.nav_start_drag(i, pos);
            } else {
                self.nav_pending = Some((left, pos, press.clone()));
            }
        }
        if left {
            self.nav_claimed.left = true;
        } else {
            self.nav_claimed.right = true;
        }
        Some(Task::none())
    }

    /// A left (`left`) or right release. Navigation keeps the release of a
    /// press it took: a press that never moved is replayed as the click it
    /// was. `None` lets the release run its normal path.
    pub(in crate::app) fn nav_release(&mut self, left: bool) -> Option<Task<Message>> {
        use crate::app::settings::NavigationStyle;
        if left {
            self.nav_held.left = false;
        } else {
            self.nav_held.right = false;
        }
        let claimed = if left {
            &mut self.nav_claimed.left
        } else {
            &mut self.nav_claimed.right
        };
        if !std::mem::take(claimed) {
            return None;
        }
        let i = self.active_tab;
        if let Some((_, _, press)) = self.nav_pending.take_if(|(l, _, _)| *l == left) {
            let release = if left {
                Message::ViewportLeftRelease
            } else {
                Message::ViewportRightRelease
            };
            self.nav_replay = true;
            let tasks = [self.update(press), self.update(release)];
            self.nav_replay = false;
            return Some(Task::batch(tasks));
        }
        let (at, dragging) = {
            let sel = self.tabs[i].scene.selection.borrow();
            (sel.input.last_move_pos, sel.input.middle_down)
        };
        // CATIA: a right click while the middle button drags turns it into
        // a zoom.
        if let (false, Some(from), Some(at)) = (left, self.nav_right_press.take(), at) {
            let (dx, dy) = (at.x - from.x, at.y - from.y);
            if self.navigation_style == NavigationStyle::Catia
                && self.nav_held.middle
                && dx * dx + dy * dy < 9.0
            {
                self.nav_zoom_latched = true;
            }
        }
        if dragging
            && self.nav_drag_by_button
            && self.nav_style_action(self.nav_held, true).is_none()
        {
            self.nav_end_drag(i);
        }
        Some(Task::none())
    }

    /// Forget a left or right button whose release never arrived (let go
    /// outside the window): one is held only while its normal press is
    /// down or navigation holds its release.
    pub(in crate::app) fn nav_resync_held(&mut self) {
        let sel = self.tabs[self.active_tab].scene.selection.borrow();
        self.nav_held.left &= sel.input.left_down || self.nav_claimed.left;
        self.nav_held.right &= sel.input.right_down || self.nav_claimed.right;
    }

    /// Run a press or release the navigation style passed on through its
    /// normal handler.
    pub(in crate::app) fn update_navigation_passthrough(
        &mut self,
        message: Message,
    ) -> Task<Message> {
        self.nav_replay = true;
        let task = self.update(message);
        self.nav_replay = false;
        task
    }

    /// Bring the point under `p` to the centre of the model view.
    pub(in crate::app) fn nav_recenter_at(&mut self, i: usize, p: Point) {
        let scene = &self.tabs[i].scene;
        if scene.active_viewport.is_some() || scene.current_layout != "Model" {
            return;
        }
        let (width, height) = scene.selection.borrow().view.vp_size;
        let bounds = scene.active_model_tile_bounds(width, height);
        let local = Point::new(p.x - bounds.x, p.y - bounds.y);
        let local_bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: bounds.width,
            height: bounds.height,
        };
        {
            let mut camera = scene.camera.borrow_mut();
            let target = camera.pick_on_target_plane(local, local_bounds);
            if target.is_finite() {
                camera.target = target;
            }
        }
        self.tabs[i].scene.camera_generation += 1;
    }

    pub(crate) fn on_viewport_exit(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].measurement.live_visible = false;
        self.hover_dwell = None;
        self.snap_dwell = None;
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.entity_pick_deferred_hover())
        {
            self.tabs[i].scene.clear_preview_wire();
        }
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.input.pointer_away = true;
        sel.input.left_down = false;
        sel.input.left_press_pos = None;
        sel.input.left_press_time = None;
        sel.input.left_dragging = false;
        sel.input.right_down = false;
        sel.input.right_press_pos = None;
        sel.input.right_press_time = None;
        sel.input.right_last_pos = None;
        sel.input.right_dragging = false;
        sel.input.right_click_entered = false;
        // A pan or orbit keeps following the pointer outside the viewport;
        // its input layer reports the moves and the release there.
        if !sel.input.middle_down {
            sel.input.middle_down = false;
            sel.input.middle_last_pos = None;
            sel.orbit.pivot = None;
        }
        sel.gesture.box_anchor = None;
        sel.gesture.box_anchor_world = None;
        sel.gesture.box_current = None;
        sel.gesture.box_crossing = false;
        sel.gesture.box_crossing_locked = false;
        sel.gesture.poly_active = false;
        sel.gesture.poly_points.clear();
        sel.gesture.poly_crossing = false;
        drop(sel);
        // Clear the rollover highlight when the cursor leaves the
        // viewport so it doesn't stick while the mouse is over the
        // ribbon / panels.
        self.tabs[i].scene.set_hover_highlight(None);
        self.tabs[i].scene.set_constraint_hover_highlights(&[]);
        // Don't touch `context_menu` here. ViewportExit also fires
        // when an upper overlay (the right-click menu panel) takes
        // the cursor, so clearing the menu state on every exit
        // would close the menu the moment it opens. Outside-click
        // dismiss is handled in `ViewportLeftPress`.
        Task::none()
    }

    /// Screen positions of the UCS-icon grips (origin + axis tips, absolute px)
    /// for the active pane, or `None` when the icon is not shown / not anchored
    /// at its on-screen origin / a command is active. The single source of truth
    /// for both hover and grip hit-testing, so they match what is drawn.
    fn ucs_icon_hit_info(
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
    /// (automation's `snap` query).
    pub(in crate::app) fn snap_query(
        &mut self,
        i: usize,
        world: glam::DVec3,
        from: Option<glam::DVec3>,
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
        let all_wires = self.tabs[i].scene.hit_test_wires();
        let candidates = self.tabs[i].scene.interaction_candidates_near(
            all_wires,
            raw,
            view_rot,
            eye,
            bounds,
            self.snapper.osnap_radius_px,
        );
        // Perpendicular and tangent measure from `from`, else the running
        // command's last point; the live cursor's base point is restored.
        let live_from = self.snapper.from_point;
        self.snapper.from_point = from.or(self.last_point);
        let (go, gr) = self.drafting_grid_basis(i);
        let hit = self
            .snapper
            .snap(raw, p, &candidates, view_rot, eye, bounds, go, gr, None);
        self.snapper.from_point = live_from;
        hit
    }

    /// Apply one frame of a UCS-icon grip drag: map the cursor onto the active
    /// UCS plane (so the move stays in-plane) and either slide the origin there
    /// or rotate the chosen axis to point at it, keeping a right-handed frame
    /// with Z fixed. Live — the commit (persist) happens on release.
    fn drag_ucs_grip(&mut self, i: usize, kind: crate::app::UcsGripKind, p_full: Point) {
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
        let raw = self.cursor_model_point(i, &edit_cam, p, bounds);

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

        use codec::types::Vector3;
        let v3 = |d: glam::DVec3| Vector3::new(d.x, d.y, d.z);
        {
            let ucs = self.tabs[i]
                .active_ucs
                .get_or_insert_with(|| codec::tables::Ucs::new("*ACTIVE*"));
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
            |a: Option<&codec::tables::Ucs>, b: Option<&codec::tables::Ucs>| match (a, b) {
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
    pub(super) fn focus_model_pane(&mut self, idx: usize) {
        let i = self.active_tab;
        if self.tabs[i].scene.set_active_model_tile(idx) {
            self.tabs[i].scene.camera_generation += 1;
            self.sync_render_mode_to_active_tile(i);
            self.adopt_view_display(i);
        }
    }

    /// Convert a pane-local cursor point (from the pane's `mouse_area`) to the
    /// canvas-relative point the viewport handlers expect.
    pub(super) fn pane_canvas_point(&self, idx: usize, local: Point) -> Point {
        let i = self.active_tab;
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let o = self.tabs[i].scene.pane_origin_px(idx, vw, vh);
        Point::new(o.x + local.x, o.y + local.y)
    }

    pub(super) fn on_pane_resized(
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

    pub(super) fn on_pane_clicked(&mut self, pane: iced::widget::pane_grid::Pane) -> Task<Message> {
        let i = self.active_tab;
        if let Some(&idx) = self.tabs[i].scene.model_panes.get(pane) {
            self.focus_model_pane(idx);
        }
        Task::none()
    }

    pub(super) fn on_pane_dragged(
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

    fn constraint_glyph_under(
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
    fn selected_object_under(
        &self,
        i: usize,
        p: Point,
        bounds: iced::Rectangle,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
    ) -> bool {
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
        .and_then(Scene::handle_from_wire_name)
        .is_some_and(|handle| scene.selected.contains(&handle));
        let hit_ms = crate::perf::elapsed_ms(hit_started);
        let total_ms = crate::perf::elapsed_ms(started);
        if total_ms >= 1.0 {
            crate::perf_record!(
                "[perf] selected-drag-probe total={total_ms:.1}ms candidates={candidate_ms:.1}ms hit={hit_ms:.1}ms wires={} candidates_count={} selected_hit={hit}",
                wire_count,
                candidates.len(),
            );
        }
        hit
    }

    /// Once a press on a selected object has moved far enough, MOVE (COPY
    /// with Ctrl) runs on the selection from the grabbed point, so the drop
    /// gets the command's preview, snaps and ortho.
    fn start_drag_move(&mut self, i: usize, p: Point) -> Option<Task<Message>> {
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

    pub(super) fn command_picks_on_press(&self) -> bool {
        let tab = &self.tabs[self.active_tab];
        tab.active_cmd.as_ref().is_some_and(|command| {
            !command.is_selection_gathering() && !command.accepts_drag_selection()
        }) && tab.scene.selection.borrow().gesture.box_anchor.is_none()
    }

    /// Capture the button event without doing the expensive hover/snap pass.
    /// Point commands immediately invoke the release path, which resolves
    /// snapping once using this exact screen position.
    pub(super) fn set_cursor_for_press(&mut self, position: Point) {
        let i = self.active_tab;
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
        }
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
        self.tabs[i].last_cursor_world = self.cursor_model_point(i, &camera, local, bounds);
        self.tabs[i].last_cursor_screen = position;
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
            .input
            .last_move_pos = Some(position);
    }

    pub(super) fn on_viewport_left_press(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].measurement.dismiss_result();
        self.block_palette.pressed = None;
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
        }
        // A left-click during a command resets the right-click cycle, so
        // the next right-click acts as Enter again rather than opening
        // the context menu.
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
            .input
            .right_click_entered = false;
        // A click in the viewport dismisses any open ribbon dropdown
        // (e.g. the annotation style combo), which has no backdrop of
        // its own to catch outside clicks.
        self.ribbon.close_dropdown();
        // Likewise dismiss the Properties color dropdowns — a viewport
        // press starts a box selection, so without this they'd only
        // close on the second click (issue #104).
        self.tabs[i].properties.color_picker_open = false;
        // Click anywhere outside the popup dismisses it. A hover popup must
        // not swallow a grip click beneath it; a pinned popup was opened by an
        // explicit click, so its outside click only dismisses the menu.
        if let Some(popup) = self.grip_popup.take() {
            self.grip_hover = None;
            if popup.pinned {
                return Task::none();
            }
        }
        // Outside-click dismiss for the visibility-state dropdown
        // (its buttons sit above this mouse_area).
        if self.visibility_popup.take().is_some() {
            return Task::none();
        }
        if self.table_popup.take().is_some() {
            return Task::none();
        }
        if self.flip_popup.take().is_some() {
            return Task::none();
        }
        // Same dismiss-on-outside-click for the right-click
        // context menu: its panel is opaque, so a press that
        // reaches here is outside the menu.
        {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            if sel.menu.open_at.take().is_some() {
                return Task::none();
            }
        }
        // A pending client `getpoint` request turns this click into the
        // answer: the snapped world point under the cursor. The pick must
        // not change the selection while the person is answering, and only
        // the pinned document's viewport may answer.
        if self.tabs[i].active_cmd.is_none() {
            let answers = self
                .control
                .get_point
                .as_ref()
                .is_some_and(|session| session.document_id == self.tabs[i].id);
            if answers {
                let world = self.tabs[i].last_cursor_world;
                self.resolve_get_point(Some([world.x, world.y, world.z]));
                return Task::none();
            }
        }
        let (p, vp_size) = {
            let sel = self.tabs[i].scene.selection.borrow();
            let p = match sel.input.last_move_pos {
                Some(p) => p,
                None => return Task::none(),
            };
            (p, sel.view.vp_size)
        };
        let (vw, vh) = vp_size;

        if self.tabs[i].active_cmd.is_none() {
            if let Some((kind, references)) = self.constraint_glyph_under(i, p) {
                let mut handles: Vec<_> = references
                    .iter()
                    .map(|reference| reference.entity)
                    .collect();
                handles.sort_unstable_by_key(|handle| handle.value());
                handles.dedup();
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entities(&handles);
                self.refresh_selected_grips();
                self.refresh_properties();
                self.command_line
                    .push_info(&format!("{kind:?} constraint selected."));
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                return Task::none();
            }
        }

        // An engaged grip owns the next left press (click-move-click placement
        // or the release of a press-drag). Do not let the same press arm the
        // normal box/lasso state or trigger another viewport control; the
        // release path below will commit the grip.
        if self.tabs[i].active_grip.is_some() {
            self.tabs[i]
                .scene
                .selection
                .borrow_mut()
                .clear_left_selection_gesture();
            return Task::none();
        }

        // Interactive navigation tools reuse the middle-button movement path,
        // so no selection/pick logic runs while the left button drives them.
        if self.tabs[i].orbit_mode || self.tabs[i].pan_mode || self.tabs[i].zoom_dynamic_mode {
            self.clear_navigation_hover(i);
            self.tabs[i].scene.remember_current_view();
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.middle_down = true;
            sel.input.middle_last_pos = Some(p);
            sel.input.middle_pan_from = None;
            if self.tabs[i].zoom_dynamic_mode {
                sel.gesture.box_anchor = Some(p);
                sel.gesture.box_current = Some(p);
                sel.gesture.box_crossing = false;
                sel.gesture.box_crossing_locked = true;
                // A fresh drag starts neutral: the magnifier shows "+" until
                // the pointer has actually moved far enough to mean zoom out.
                sel.input.zoom_dir_out = false;
            }
            return Task::none();
        }

        if vw > 1.0 && vh > 1.0 {
            let rot = self.tabs[i].scene.active_view_rotation_mat();
            // Map the cursor into whichever area owns the cube (active
            // viewport rect in paper, active tile in model) so the
            // consume-check lines up with the gizmo — same framing as
            // ViewportClick's snap hit-test.
            let (cx, cy, w, h) = match self.tabs[i]
                .scene
                .active_viewport
                .and_then(|hndl| self.tabs[i].scene.viewport_screen_rect(hndl, (vw, vh)))
            {
                Some(rect) => (p.x - rect.x, p.y - rect.y, rect.width, rect.height),
                None => {
                    let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                    (p.x - tb.x, p.y - tb.y, tb.width, tb.height)
                }
            };
            if scene::hit_test(cx, cy, w, h, rot, VIEWCUBE_PX).is_some() {
                return Task::none();
            }
        }

        let custom_grip_at_cursor = self.tabs[i].active_cmd.is_none() && self.custom_block_grip_at_cursor(i, p, vw, vh);
        if custom_grip_at_cursor {
            self.ucs_icon_selected = false;
            self.ucs_icon_hover = false;
        }

        // UCS icon: click to select (grips appear), then drag a grip to
        // move/rotate. A click off the icon clears the selection. Works
        // in the corner too (parked icon), not just at the origin.
        if self.show_ucs_icon
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].active_block_edit_session().is_none()
            && !custom_grip_at_cursor
        {
            if let Some(hit) = self.ucs_icon_hit_info(i, vw, vh) {
                // Already selected → a press on a grip starts the drag.
                if self.ucs_icon_selected {
                    if let Some(kind) = ucs_grip_under(p, &hit) {
                        self.ucs_grip_drag = Some(kind);
                        return Task::none();
                    }
                }
                // Press on the icon body selects it (and shows grips).
                if over_ucs_icon(p, &hit) {
                    self.ucs_icon_selected = true;
                    self.ucs_icon_hover = true;
                    return Task::none();
                }
            }
            // Press elsewhere drops the selection, then falls through to
            // the normal pick / box-select so the click still lands.
            self.ucs_icon_selected = false;
        }

        // Divider resize is handled natively by the input pane_grid
        // (`on_resize`); the active pane already follows the cursor via
        // `focus_model_pane`. So a press here goes straight to picking.

        // From here the click targets the active tile: map the
        // cursor into it and use the tile's size for picking, so
        // grip / selection hit-tests land in the right pane.
        let p_full = p;
        // Inside a floating viewport the pane is the viewport's own rect
        // + camera (matches the GPU); otherwise the active model tile.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let tile_b = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = iced::Point {
            x: p_full.x - tile_b.x,
            y: p_full.y - tile_b.y,
        };
        let (vw, vh) = (tile_b.width, tile_b.height);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: vw,
            height: vh,
        };

        // STRETCH: Shift+click an already selected vertex to subtract only
        // that vertex (not its entire entity). Use the same grip hit testing
        // as a normal click, including paper-space and viewport transforms.
        if self.shift_down
            && self.tabs[i].active_cmd.is_some()
            && !self.tabs[i].selected_grips.is_empty()
        {
            let grip_hit = if let Some(cam) = &edit_cam {
                find_hit_grip_rte(
                    p, &self.tabs[i].selected_grips,
                    cam.view_proj_rte(bounds), cam.eye(), bounds,
                )
            } else if self.tabs[i].scene.current_layout != "Model" {
                let cam = self.tabs[i].scene.camera.borrow();
                let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
                let half_h = cam.ortho_size();
                let half_w = half_h * aspect;
                find_hit_grip_paper(
                    p, &self.tabs[i].selected_grips,
                    cam.target.x as f32, cam.target.y as f32,
                    half_w, half_h, bounds,
                )
            } else {
                let cam = self.tabs[i].scene.camera.borrow();
                find_hit_grip(p, &self.tabs[i].selected_grips, &cam, bounds)
            };
            if let Some((index, grip_id, _, world)) = grip_hit {
                if let Some(&handle) = self.tabs[i].selected_grip_handles.get(index) {
                    let removed = self.tabs[i].active_cmd.as_mut().is_some_and(|command| {
                        command.remove_stretch_point(handle, world)
                    });
                    if removed {
                        self.tabs[i].selected_grips.remove(index);
                        self.tabs[i].selected_grip_handles.remove(index);
                        self.tabs[i].hot_grips.remove(&(handle, grip_id));
                        self.grip_hover = None;
                        self.tabs[i].scene.selection.borrow_mut().clear_left_selection_gesture();
                        return Task::none();
                    }
                }
            }
        }

        if self.tabs[i].active_cmd.is_none()
            && self.tabs[i].active_grip.is_none()
            && !self.tabs[i].selected_grips.is_empty()
        {
            {
                let is_paper = self.tabs[i].scene.current_layout != "Model";
                // In-viewport grips are model-space; project them with the
                // viewport camera so they hit-test where the GPU draws
                // them. Paper-space entities use the 2-D paper transform;
                // the model tab uses the model camera.
                let grip_hit = if self.tabs[i].active_block_edit_session().is_some()
                    && self.tabs[i].selected_authoring.is_some()
                {
                    self.authoring_grip_hit(p, bounds)
                } else if let Some(cam) = &edit_cam {
                    find_hit_grip_rte(
                        p,
                        &self.tabs[i].selected_grips,
                        cam.view_proj_rte(bounds),
                        cam.eye(),
                        bounds,
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
                if let Some((grip_index, grip_id, is_translate, world)) = grip_hit {
                    let Some(&handle) = self.tabs[i].selected_grip_handles.get(grip_index) else {
                        return Task::none();
                    };
                    let grip_shape = self.tabs[i].selected_grips[grip_index].shape;
                    if matches!(
                        grip_shape,
                        crate::scene::model::object::GripShape::Dropdown
                            | crate::scene::model::object::GripShape::DropdownAdjacent
                    ) {
                        use crate::entities::traits::EntityTypeOps;
                        let items = self.tabs[i]
                            .scene
                            .document
                            .get_entity(handle)
                            .map(|entity| entity.grip_menu(grip_id))
                            .unwrap_or_default();
                        if !items.is_empty() {
                            let selected = items
                                .iter()
                                .position(|item| item.label.starts_with('✓'))
                                .unwrap_or(0);
                            self.grip_popup = Some(crate::app::GripPopup {
                                handle,
                                grip_id,
                                anchor: p_full,
                                items,
                                selected,
                                pinned: true,
                            });
                        }
                        self.grip_hover = None;
                        return Task::none();
                    }
                    // The visibility (lookup) grip opens a state
                    // dropdown instead of starting a stretch drag.
                    if grip_id == crate::app::visibility::VIS_GRIP_ID {
                        self.open_visibility_popup(p);
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    if grip_id == crate::app::visibility::TABLE_GRIP_ID {
                        self.open_table_popup(p);
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    // A flip parameter's chevron lists its two state labels
                    // outside the block editor instead of toggling or
                    // starting a command.
                    if self.open_flip_popup_at(handle, grip_id, p_full) {
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    if self.start_test_action_grip(handle, grip_id) {
                        return Task::none();
                    }
                    if self.start_authoring_grip(handle, grip_id) {
                        return Task::none();
                    }
                    if self.shift_down {
                        let key = (handle, grip_id);
                        if !self.tabs[i].hot_grips.remove(&key) {
                            self.tabs[i].hot_grips.insert(key);
                        }
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    let mut edit = self.grip_edit_for_hit(i, handle, grip_id, is_translate, world);
                    if edit.gizmo {
                        // Where the gizmo part was grabbed: the drag keeps
                        // that offset from the cloud's insertion point.
                        let (view_rot, eye) = match &edit_cam {
                            Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                            None => {
                                let cam = self.tabs[i].scene.camera.borrow();
                                (cam.view_proj_rte(bounds), cam.eye())
                            }
                        };
                        let grabbed = match (edit.axis, edit.plane) {
                            (Some(axis), _) => {
                                cursor_on_projected_axis(p, bounds, view_rot, eye, world, axis)
                            }
                            (_, Some(normal)) => {
                                cursor_on_plane(p, bounds, view_rot, eye, world, normal)
                            }
                            _ => None,
                        };
                        edit.grab = grabbed.map(|point| point - world);
                    }
                    self.tabs[i].active_grip = Some(edit);
                    self.grip_engage_press = Some(p);
                    self.grip_hover = None;
                    self.grip_popup = None;
                    self.tabs[i]
                        .scene
                        .selection
                        .borrow_mut()
                        .clear_left_selection_gesture();

                    // A grip edit is not a CAD command, so explicitly seed the shared
                    // dynamic-input fields for the newly engaged grip.
                    self.sync_dyn_fields();

                    return Task::none();
                }
            }
        }

        // API v11: plugin-declared custom grips engage on press regardless of
        // the selection (always-visible display state). Grips are tested
        // before any wire pick, so a custom grip visually coincident with its
        // own reference-only marker — or any click-through geometry — stays
        // fully hittable and wins the hit.
        #[cfg(not(target_arch = "wasm32"))]
        if self.tabs[i].active_cmd.is_none() && self.tabs[i].active_grip.is_none() {
            if let Some(candidate) = self.hit_plugin_grip(i, p) {
                self.grip_hover = None;
                self.grip_popup = None;
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                return self.begin_plugin_grip_drag(i, &candidate);
            }
        }

        // Navigation, overlays and grips retain priority over document hyperlinks.
        if self.ctrl_down
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].scene.current_layout == "Model"
        {
            let (view_rot, eye) = {
                let cam = self.tabs[i].scene.camera.borrow();
                (cam.view_proj_rte(bounds), cam.eye())
            };
            let wires = self.tabs[i].scene.hit_test_wires();
            let url = scene::pick::hit_test::click_hit(
                p,
                &*wires,
                view_rot,
                eye,
                bounds,
                self.tabs[i].scene.document.header.lineweight_display,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
            )
            .and_then(Scene::handle_from_wire_name)
            .and_then(|handle| self.tabs[i].scene.document.get_entity(handle))
            .and_then(scene::pe_url_of)
            .and_then(crate::sys::web_hyperlink);
            if let Some(url) = url {
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                self.command_line
                    .push_info(&format!("{}: {url}", crate::t!("Hyperlink")));
                return crate::sys::open_url(&url, self.main_window);
            }
        }

        // Ctrl+click on a polyline picks the segment under the cursor; the
        // edit commands then move just that part.
        if self.ctrl_down && self.tabs[i].active_cmd.is_none() {
            let (view_rot, eye) = {
                let cam = self.tabs[i].scene.camera.borrow();
                (cam.view_proj_rte(bounds), cam.eye())
            };
            let wires = self.tabs[i].scene.hit_test_wires();
            let hit = scene::pick::hit_test::click_hit(
                p,
                &*wires,
                view_rot,
                eye,
                bounds,
                self.tabs[i].scene.document.header.lineweight_display,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
            )
            .and_then(Scene::handle_from_wire_name)
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
            drop(wires);
            let point = self.tabs[i].last_cursor_world;
            if hit.is_some_and(|handle| self.tabs[i].scene.toggle_sub_segment(handle, point)) {
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                self.refresh_properties();
                return Task::none();
            }
        }

        let authoring_label = self.authoring_label_at(p, bounds);
        if self.pick_authoring_object(p, bounds) {
            if let Some((handle, name)) = authoring_label
                .filter(|(handle, _)| self.tabs[i].selected_authoring == Some(*handle))
            {
                let now = Instant::now();
                let is_double = self.last_vp_click_time.is_some_and(|last| {
                    now.duration_since(last).as_millis() < 400
                        && self.last_vp_click_pos.is_some_and(|previous| {
                            (p.x - previous.x).hypot(p.y - previous.y) < 8.0
                        })
                });
                self.last_vp_click_time = Some(now);
                self.last_vp_click_pos = Some(p);
                if is_double && self.authoring_name_is_editable(handle) {
                    use crate::scene::block_authoring::metadata::{self, Field};
                    let (field, name) = self.tabs[i].active_block_edit_session()
                        .and_then(|session| metadata::read(&self.tabs[i].scene.document, session.br_handle, handle).ok())
                        .and_then(|metadata| {
                            let text = metadata.text.iter().find(|text| matches!(text.field, Field::Name | Field::SecondaryName) && text.value == name)
                                .or_else(|| metadata.text.iter().find(|text| text.field == Field::Name))?;
                            Some((text.field.key(), text.value.to_owned()))
                        }).unwrap_or(("authoring_name", name));
                    let key = crate::ui::properties::FieldKey::Geom(field);
                    self.tabs[i].properties.edit_buf.insert(key, name);
                    self.tabs[i].properties.active_field = None;
                    let id = crate::ui::properties::prop_geom_field_id(field);
                    return Task::batch([
                        iced::widget::operation::focus(id.clone()),
                        iced::widget::operation::select_all(id),
                    ]);
                }
            }
            return Task::none();
        }

        // Ctrl+click on a composite solid picks the solid of it under the
        // cursor — the one the Properties palette and grips then edit.
        if self.ctrl_down
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].scene.current_layout == "Model"
        {
            let (view_rot, eye) = {
                let cam = self.tabs[i].scene.camera.borrow();
                (cam.view_proj_rte(bounds), cam.eye())
            };
            let hit = self.tabs[i]
                .scene
                .solid_click_hit_point(p, view_rot, eye, bounds);
            if let Some((handle, _)) = hit.filter(|(handle, point)| {
                crate::scene::model::solid_history::select_operand_at(
                    &self.tabs[i].scene.document,
                    *handle,
                    *point,
                )
            }) {
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entity(handle, false);
                self.refresh_properties();
                return Task::none();
            }
        }

        // A command step that takes a point or a pick commits on the press,
        // so the cursor moving before the button comes up cannot move it.
        // Selection steps and drag-fence picks still wait for the release.
        let picks_on_press = self.command_picks_on_press();
        // A press on a selected object may become a drag of the selection.
        self.drag_move = (self.tabs[i].active_cmd.is_none()
            && !self.shift_down
            && !self.tabs[i].scene.selected.is_empty()
            && self.selected_object_under(i, p, bounds, &edit_cam))
        .then_some((self.tabs[i].last_cursor_world, false));
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.input.left_down = true;
        // Stored in full-canvas space (like ViewportMove's cursor and
        // the overlay box / lasso drawing); release maps it into the
        // active tile. Tile-local here would double-offset the anchor.
        sel.input.left_press_pos = Some(p_full);
        sel.input.left_press_time = Some(Instant::now());
        sel.input.left_dragging = false;
        drop(sel);
        if picks_on_press {
            // The command path clears `left_down`, so the real release
            // that follows finds nothing pressed and does nothing.
            return self.on_viewport_left_release();
        }
        Task::none()
    }

    pub(super) fn on_viewport_left_release(&mut self) -> Task<Message> {
        let i = self.active_tab;
        // A click fires after the cursor has settled — clear any pending
        // deferred snap so the release's own snap runs at full fidelity.
        self.snap_dwell = None;
        // The release commits (or restarts) any box; its preview is done.
        self.tabs[i].scene.set_box_hover_highlights(&[]);
        // A block dragged from the Blocks palette lands here.
        if let Some(item) = self.take_block_drop() {
            return self.drop_block(item);
        }
        // A dragged selection drops where the button comes up: the release
        // is the MOVE / COPY destination click, and a COPY places one copy.
        if let Some((_, true)) = self.drag_move.take() {
            {
                // Starting the command reset the press; the drop reads as a
                // click at the cursor.
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.input.left_down = true;
                sel.input.left_dragging = false;
            }
            let placed = self.on_viewport_left_release();
            let copying = self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.name() == "COPY");
            if !copying {
                return placed;
            }
            return Task::batch([placed, self.feed_command(crate::command::StepInput::Enter)]);
        }
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
            if command.consume_stretch_remove_release() {
                self.tabs[i].scene.selection.borrow_mut().clear_left_selection_gesture();
                return Task::none();
            }
        }

        // Navigation mode: end this drag but keep the tool armed for the next
        // left drag (exit is Esc / another command). Mirror of the press.
        if self.tabs[i].orbit_mode || self.tabs[i].pan_mode || self.tabs[i].zoom_dynamic_mode {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.middle_down = false;
            sel.input.middle_last_pos = None;
            sel.orbit.pivot = None;
            sel.gesture.box_anchor = None;
            sel.gesture.box_anchor_world = None;
            sel.gesture.box_current = None;
            sel.gesture.box_crossing_locked = false;
            sel.input.zoom_dir_out = false;
            drop(sel);
            self.arm_hover_after_navigation(i);
            return Task::none();
        }

        // Commit a UCS icon grip drag: persist the new UCS so it
        // round-trips, and clear the lingering press state. In BEDIT this
        // rebases only the block; elsewhere it writes the model/viewport UCS.
        if self.ucs_grip_drag.take().is_some() {
            self.commit_active_ucs_change(i, "UCS");
            self.tabs[i].snap_result = None;
            self.snapper.from_point = None;
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.left_down = false;
            sel.input.left_press_pos = None;
            sel.input.left_dragging = false;
            return Task::none();
        }

        let (p, is_click, is_down) = {
            let sel = self.tabs[i].scene.selection.borrow();
            let last = match sel.input.last_move_pos {
                Some(p) => p,
                None => return Task::none(),
            };
            let is_click = !sel.input.left_dragging;
            let p = selection_click_position(
                last,
                sel.input.left_press_pos,
                is_click,
                self.tabs[i].active_cmd.is_none(),
            );
            (p, is_click, sel.input.left_down)
        };

        // Custom plugin-grip drag (API v11): mouse-up settles — the plugin
        // applies its own edits inside its own undo scope; the host records
        // nothing. A bare click (no move delivered) still settles: the
        // plugin decides what a no-move settle means (typically nothing,
        // zero writes, zero undo steps).
        #[cfg(not(target_arch = "wasm32"))]
        if self.tabs[i].active_plugin_grip.is_some() {
            {
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.clear_left_selection_gesture();
            }
            return self.settle_plugin_grip_drag();
        }

        // Grip editing: click-move-click (plus legacy press-drag).
        // The grip engages on press (active_grip set). This release
        // commits only if the grip has actually moved, or if it was a
        // press-drag. A bare engaging click (no movement yet) keeps the
        // grip hot so the user can move the cursor and click again to
        // place it. Escape cancels (handled elsewhere).
        if let Some(grip) = self.tabs[i].active_grip.clone() {
            // Reset mouse state so the lingering press from the engaging
            // click doesn't read as an in-progress drag on later moves.
            {
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.clear_left_selection_gesture();
            }
            // Grip-menu Base Point: this click re-bases the gesture at the
            // cursor instead of placing anything.
            if self.tabs[i].grip_base_pending {
                self.tabs[i].grip_base_pending = false;
                if let Some(active) = self.tabs[i].active_grip.as_mut() {
                    active.origin_world = active.last_world;
                }
                return Task::none();
            }
            // The press that picked the grip up, released within the drag
            // threshold, was a click however the hand trembled: stay hot
            // and wait for the placement click.
            let engaging_click = self.grip_engage_press.take().is_some_and(|press| {
                let (dx, dy) = (p.x - press.x, p.y - press.y);
                dx * dx + dy * dy <= 12.0 * 12.0
            });
            let moved = grip.last_world != grip.origin_world;
            if engaging_click || (is_click && !moved) {
                return Task::none();
            }
            return self.commit_active_grip_edit();
        }

        // Map the release point into the active Model tile so the
        // click's pick / on_point / selection use the active pane's
        // camera + bounds. `p_full` keeps the canvas point for the
        // box/poly selection rectangle (drawn in canvas space).
        let p_full = p;
        // Inside a floating viewport the pane is the viewport's own rect
        // and camera (see the MoveCursor path); pick / snap then run in
        // model space exactly where the GPU draws the content.
        let canvas_sz = self.tabs[i].scene.selection.borrow().view.vp_size;
        let edit_frame = self.tabs[i].scene.viewport_edit_frame(canvas_sz);
        let (tile_vw, tile_vh, tile_off) = match &edit_frame {
            Some((_, full)) => (full.width, full.height, iced::Point::new(full.x, full.y)),
            None => {
                let tb = self.tabs[i]
                    .scene
                    .active_model_tile_bounds(canvas_sz.0, canvas_sz.1);
                (tb.width, tb.height, iced::Point::new(tb.x, tb.y))
            }
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = iced::Point {
            x: p_full.x - tile_off.x,
            y: p_full.y - tile_off.y,
        };

        let is_gathering = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.is_selection_gathering())
            .unwrap_or(false);
        let selection_box_active = self.tabs[i]
            .scene
            .selection
            .borrow()
            .gesture
            .box_anchor
            .is_some();
        let selection_pick_add = self.pick_add
            || self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.selection_forces_add());
        // A committed window corner must stay free of the Ortho/Polar
        // lock too, so the picked rectangle isn't flattened (#291).
        let is_window_corner = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.window_corner_pick())
            .unwrap_or(false);
        // An internal-point pick commits the bare cursor position (#2009).
        let raw_pick = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.wants_raw_point());
        let free_point = is_window_corner || raw_pick;

        if is_down
            && is_click
            && self.tabs[i].active_cmd.is_some()
            && !is_gathering
            && !selection_box_active
        {
            let (vw, vh) = (tile_vw, tile_vh);
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };

            // Hover may still describe an earlier pointer position while
            // snap is deferred. Only the snap computed for this click may
            // supply tangent metadata or decide whether DUCS applies.
            self.tabs[i].snap_result = None;
            let snapped;
            let tangent_obj_at_click;

            let world_pt = {
                // Cursor → model point (viewport camera inside a viewport,
                // else paper sheet → model). Model space throughout.
                let raw = self.cursor_model_point(i, &edit_cam, p, bounds);
                let (view_rot, eye) = match &edit_cam {
                    Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                    None => {
                        let c = self.tabs[i].scene.camera.borrow();
                        (c.view_proj_rte(bounds), c.eye())
                    }
                };
                let snap_cursor = raw;
                let click_source_started = crate::perf::enabled().then(Instant::now);
                let all_wires =
                    if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                        self.tabs[i]
                            .scene
                            .model_wires_for_viewport_arc(h, bounds.height)
                    } else {
                        self.tabs[i].scene.hit_test_wires()
                    };
                let click_source_ms = crate::perf::elapsed_ms(click_source_started);
                let click_wire_count = all_wires.len();
                let click_query_started = crate::perf::enabled().then(Instant::now);
                let snap_candidates = self.tabs[i].scene.interaction_candidates_near(
                    all_wires,
                    snap_cursor,
                    view_rot,
                    eye,
                    bounds,
                    self.snapper.osnap_radius_px,
                );
                let click_query_ms = crate::perf::elapsed_ms(click_query_started);
                let click_evaluate_started = crate::perf::enabled().then(Instant::now);
                let needs_tan = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.needs_tangent_pick())
                    .unwrap_or(false);
                let is_gathering = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.is_selection_gathering())
                    .unwrap_or(false);
                let needs_entity_click = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.needs_entity_pick())
                    .unwrap_or(false);
                let entity_click_takes_points = needs_entity_click
                    && self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|command| command.entity_pick_accepts_points());
                // Probe the active OTRACK ray before snapping so its geometry
                // intersection can be tested in the same candidate pass.
                let tracking_probe = if !needs_entity_click && !needs_tan && !free_point {
                    self.active_otrack_hit(
                        i,
                        raw,
                        None,
                        self.last_point,
                        true,
                        view_rot,
                        eye,
                        bounds,
                    )
                } else {
                    None
                };
                let mut snap_hit = if (needs_entity_click && !entity_click_takes_points) || is_gathering {
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
                    self.snapper.from_point = self.last_point;

                    let construction_ray = if free_point {
                        None
                    } else {
                        self.last_point.and_then(|base| {
                            self.active_construction_ray(
                                i,
                                p,
                                snap_cursor,
                                base,
                                view_rot,
                                eye,
                                bounds,
                            )
                        })
                    };
                    let additional_rays =
                        tracking_probe.map(|track| [(track.base, track.base + track.dir)]);

                    self.snapper.snap_with_additional_construction_rays(
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
                    )
                };
                // Paper-space snapping through layout viewports. Mirrors
                // the cursor-move pass: the click recomputes snapping from
                // scratch, so the viewport query has to run here too. The hit
                // comes back already projected onto the sheet.
                let mut click_frame: Option<crate::scene::viewport_ref::ViewportFrame> = None;
                if edit_cam.is_none() && !needs_entity_click && !needs_tan {
                    if let Some((vp_hit, frame)) =
                        self.paper_viewport_snap(i, p_full, (vw, vh), raw)
                    {
                        let merged = crate::snap::merge_snap(snap_hit, Some(vp_hit), p_full);
                        if merged.is_some_and(|hit| hit.viewport.is_some()) {
                            click_frame = Some(frame);
                        }
                        snap_hit = merged;
                    }
                }
                // Solid-face snaps (Nearest/Perpendicular to face), mirroring
                // the cursor-move pass so click and preview agree. Merged
                // before the accepted-snap record so face sources associate.
                if !needs_entity_click
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
                        snap_hit = crate::snap::merge_snap(snap_hit, face_hit, p);
                    }
                }
                // Mouse-up must resolve the same original snap as mouse-move;
                // otherwise the highlighted reference point and committed
                // STRETCH destination could disagree.
                snap_hit = crate::snap::merge_snap(
                    snap_hit,
                    self.command_original_snap_hit(i, p, view_rot, eye, bounds),
                    p,
                );
                let click_evaluate_ms = crate::perf::elapsed_ms(click_evaluate_started);
                if crate::perf::enabled()
                    && click_source_ms + click_query_ms + click_evaluate_ms >= 1.0
                {
                    crate::perf_record!(
                        "[perf] snap-click source={click_source_ms:.2}ms query={click_query_ms:.2}ms evaluate={click_evaluate_ms:.2}ms input_wires={click_wire_count} candidates={} snap_on={} snap3d_on={} tangent={needs_tan}",
                        snap_candidates.len(),
                        self.snapper.snap_enabled,
                        self.snapper.snap3d_enabled,
                    );
                }
                if raw_pick {
                    snap_hit = None;
                }
                self.pending_click_snap = snap_hit.map(|hit| (hit, click_frame));
                snapped = snap_hit.is_some();
                tangent_obj_at_click = snap_hit.and_then(|hit| hit.tangent_obj);
                // Snap runs in model space; the result is already model.
                let mut pt = snap_hit.map(|s| s.world).unwrap_or(raw);
                // When no UCS is active clamp to world XY; with a UCS the point is
                // already constrained to that plane by the ray–plane intersection.
                // A genuine 3D object snap keeps its elevation — only grid snaps
                // and misses are flattened.
                let uses_command_cursor_plane = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.cursor_plane())
                    .is_some();
                let clamp_world_xy = self.tabs[i].active_ucs.is_none()
                    && !uses_command_cursor_plane
                    && !snap_keeps_elevation(snap_hit.map(|s| s.snap_type));
                if clamp_world_xy {
                    pt.z = 0.0;
                }
                let axis_lock = if let Some(base) = self.last_point {
                    self.active_axis_lock(
                        i,
                        raw,
                        base,
                        !needs_entity_click
                            && !needs_tan
                            && !free_point
                            && !uses_command_cursor_plane,
                    )
                } else {
                    self.axis_lock_dir = None;
                    None
                };
                if let (Some(dir), Some(base)) = (axis_lock, self.last_point) {
                    pt = axis_lock_apply(pt, base, dir);
                    if clamp_world_xy {
                        pt.z = 0.0;
                    }
                }
                let otrack = if axis_lock.is_none() {
                    self.active_otrack_hit(
                        i,
                        raw,
                        snap_hit,
                        self.last_point,
                        !free_point,
                        view_rot,
                        eye,
                        bounds,
                    )
                } else {
                    None
                };
                if let Some(h) = otrack.filter(|_| {
                    // Match the hover path: preserve exact object snaps, but
                    // allow Extension to resolve a crossing with OTRACK.
                    !snap_hit.is_some_and(|s| {
                        !matches!(
                            s.snap_type,
                            crate::snap::SnapType::Grid | crate::snap::SnapType::Extension
                        )
                    })
                }) {
                    // Commit the actual OTRACK intersection, not the cursor
                    // projection onto Extension.
                    pt = h.aligned;
                    if clamp_world_xy {
                        pt.z = 0.0;
                    }
                } else if axis_lock.is_none()
                    && !free_point
                    && !uses_command_cursor_plane
                    && !snap_hit.is_some_and(|s| s.snap_type != crate::snap::SnapType::Grid)
                {
                    // Object snap wins over ortho/polar — a snapped point
                    // commits as-is. Grid snap still combines. (#132)
                    if let Some((point, _)) = self.last_point.and_then(|base| {
                        self.drafting_point(i, p, pt, base, view_rot, eye, bounds)
                    }) {
                        pt = point;
                    }
                }
                pt = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.cursor_axis())
                    .and_then(|(origin, direction)| {
                        command_axis_point(snap_hit, p, bounds, view_rot, eye, origin, direction)
                    })
                    .unwrap_or(pt);
                // A click while dynamic-input fields hold typed values
                // commits the CONSTRAINED point — the same resolution
                // the preview shows and Enter would commit (#356).
                if !needs_entity_click
                    && self.tabs[i].active_cmd.is_some()
                    && self.tabs[i].dyn_fields.iter().any(|f| f.buffer.is_some())
                {
                    self.tabs[i].last_cursor_world = pt;
                    if let Some(r) = self.dyn_resolve_point() {
                        pt = r;
                    }
                }
                if entity_click_takes_points {
                    snap_hit.map_or(raw, |hit| hit.world)
                } else {
                    pt
                }
            };

            // `world_pt` is in offset-relative (local) space, matching
            // the camera and the point-creation commands. Entity-pick /
            // tangent / structure-pick commands instead compare the
            // click against WCS document entities, so they need the
            // world_offset added back (model space only — paper-space
            // entities are already in sheet coordinates). Without this,
            // TRIM/EXTEND/FILLET pick the wrong side on UTM-scale files.
            let pick_wcs = {
                let wo = if self.tabs[i].scene.current_layout == "Model" {
                    [0.0_f64; 3]
                } else {
                    [0.0; 3]
                };
                world_pt + glam::DVec3::new(wo[0], wo[1], wo[2])
            };

            if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                command.set_ctrl(self.ctrl_down);
                command.set_shift(self.shift_down);
            }
            let result = if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_structure_point_pick())
                .unwrap_or(false)
            {
                let pick = self.tabs[i].active_cmd.as_ref().and_then(|c| {
                    c.resolve_object_pick(&self.tabs[i].scene, pick_wcs.x as f64, pick_wcs.y as f64)
                });
                if let Some(pick) = pick {
                    let center = glam::DVec3::new(pick.x, pick.y, pick_wcs.z);
                    let result = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_structure_pick(pick.handle, center));
                    self.command_line
                        .push_info(crate::tf!("{} acquired.", pick.label).as_ref());
                    result
                } else {
                    let msg = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.object_pick_miss_message())
                        .unwrap_or("No object near click.");
                    if msg.is_empty() {
                        self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .map(|c| c.on_point(pick_wcs))
                    } else {
                        self.command_line.push_error(msg);
                        None
                    }
                }
            } else if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_entity_pick())
                .unwrap_or(false)
            {
                let pick_aperture = crate::ui::overlay::pick_box_aperture_px(self.pick_box);
                let (view_rot2, eye2, all_wires2) = self.pick_view(i, &edit_cam, bounds);
                let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                    all_wires2,
                    world_pt,
                    view_rot2,
                    eye2,
                    bounds,
                    pick_aperture * 2.0,
                );
                let include_fills = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.entity_pick_includes_fills())
                    .unwrap_or(false);
                let candidate_handles = if include_fills {
                    self.tabs[i]
                        .scene
                        .interaction_candidate_handles(&click_candidates)
                } else {
                    None
                };
                let hit = scene::pick::hit_test::click_hit(
                    p,
                    &click_candidates,
                    view_rot2,
                    eye2,
                    bounds,
                    self.tabs[i].scene.document.header.lineweight_display,
                    pick_aperture,
                    &self.tabs[i].scene.draw_depth_map(),
                )
                .and_then(|s| Scene::handle_from_wire_name(s))
                .or_else(|| {
                    if !include_fills {
                        return None;
                    }
                    scene::pick::hit_test::click_hit_hatch(
                        p,
                        &self.tabs[i]
                            .scene
                            .visible_hatches_for_click(candidate_handles.as_ref()),
                        view_rot2,
                        eye2,
                        bounds,
                        candidate_handles.as_ref(),
                    )
                })
                .or_else(|| {
                    include_fills.then(|| {
                        scene::pick::hit_test::click_hit_insert_hatch(
                            p,
                            self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                            view_rot2,
                            eye2,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })?
                })
                .or_else(|| {
                    include_fills.then(|| {
                        self.tabs[i].scene.solid_click_hit(
                            p,
                            view_rot2,
                            eye2,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })?
                })
                .or_else(|| {
                    // A solid's edge: a face seen edge-on draws only there.
                    include_fills.then(|| {
                        self.tabs[i].scene.solid_edge_hover_hit(
                            p,
                            view_rot2,
                            eye2,
                            bounds,
                            candidate_handles.as_ref(),
                            pick_aperture,
                        )
                    })?
                });
                // A dimension pick inside a viewport measures the model
                // geometry shown there, not the viewport entity itself.
                let hit = hit.filter(|&handle| {
                    let measures = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.measures_through_viewports());
                    if !measures
                        || !matches!(
                            self.tabs[i].scene.document.get_entity(handle),
                            Some(codec::EntityType::Viewport(_))
                        )
                    {
                        return true;
                    }
                    let per_px = self.tabs[i].scene.camera.borrow().ortho_size() as f64 * 2.0
                        / (bounds.height as f64).max(1.0);
                    self.tabs[i]
                        .scene
                        .dimension_pick_through_viewport(pick_wcs, pick_aperture as f64 * per_px)
                        .is_none()
                });
                if let Some(handle) = hit {
                    let uses_surface_point = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|command| command.entity_pick_uses_surface_point());
                    let entity_pick_point = if uses_surface_point {
                        self.solid_pick_point(i, handle, &edit_cam, p, bounds, view_rot2, eye2)
                            .or_else(|| {
                                self.tabs[i]
                                    .active_cmd
                                    .as_ref()
                                    .is_some_and(|command| command.entity_pick_deferred_hover())
                                    .then(|| {
                                        self.profile_pick_point(i, handle, &edit_cam, p, bounds)
                                    })
                                    .flatten()
                            })
                            .unwrap_or_else(|| {
                                let bounded_pick = self.tabs[i]
                                    .active_cmd
                                    .as_ref()
                                    .is_some_and(|command| command.entity_pick_deferred_hover());
                                if bounded_pick
                                    && matches!(
                                        self.tabs[i].scene.document.get_entity(handle),
                                        Some(codec::EntityType::Solid3D(_)),
                                    )
                                {
                                    // The aperture caught an edge outside its face.
                                    // Do not reinterpret the working-plane point as
                                    // a hit on another cap of the same body.
                                    glam::DVec3::NAN
                                } else {
                                    pick_wcs
                                }
                            })
                    } else {
                        pick_wcs
                    };
                    let entity_pick_direction =
                        if uses_surface_point && entity_pick_point.is_finite() {
                            if matches!(
                                self.tabs[i].scene.document.get_entity(handle),
                                Some(codec::EntityType::Solid3D(_))
                            ) {
                                self.tabs[i]
                                    .scene
                                    .solid_planar_face_normal_at(handle, entity_pick_point)
                            } else {
                                self.tabs[i]
                                    .scene
                                    .document
                                    .get_entity(handle)
                                    .and_then(crate::entities::curve::entity_curve)
                                    .and_then(|curve| curve.plane.normal())
                                    .map(glam::DVec3::from_array)
                            }
                        } else {
                            None
                        };
                    // A dimension pick on a block reference measures the
                    // nested entity under the click, as a pick through a
                    // viewport does.
                    let nested = if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.measures_through_viewports())
                        && matches!(
                            self.tabs[i].scene.document.get_entity(handle),
                            Some(codec::EntityType::Insert(_))
                        )
                    {
                        let scene = &self.tabs[i].scene;
                        scene.resolve_measurable_entity(handle, entity_pick_point, None)
                    } else {
                        None
                    };
                    let (handle, block_path) = match &nested {
                        Some((entity, path)) => {
                            let nested_handle = entity.as_entity().handle();
                            (
                                if nested_handle.is_valid() {
                                    nested_handle
                                } else {
                                    handle
                                },
                                path.clone(),
                            )
                        }
                        None => (handle, Vec::new()),
                    };
                    // Some commands (e.g. SS_CATCHMENT) need the entity
                    // body before `on_entity_pick` can advance.
                    let inject_first = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.inject_before_entity_pick())
                        .unwrap_or(false);
                    if inject_first {
                        let surface_area = self.tabs[i]
                            .scene
                            .meshes
                            .get(&handle)
                            .or_else(|| self.tabs[i].scene.block_meshes.get(&handle))
                            .map(|mesh| mesh.metrics.surface_area);
                        if let Some(entity) = nested.map(|(entity, _)| entity).or_else(|| {
                            self.tabs[i].scene.document.get_entity(handle).cloned()
                        }) {
                            if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                                cmd.inject_picked_entity(entity);
                                if let Some(area) = surface_area {
                                    cmd.inject_picked_surface_area(area);
                                }
                            }
                        }
                    }

                    if !self.dimension_acquisition_allowed(i, None) {
                        self.finish_command_click(i);
                        return Task::none();
                    }
                    let shift = self.shift_down;
                    let result = self.tabs[i].active_cmd.as_mut().map(|c| {
                        // Shift-swap state for TRIM/EXTEND (#336).
                        c.set_shift(shift);
                        c.set_entity_pick_direction(entity_pick_direction);
                        c.on_entity_pick(handle, entity_pick_point)
                    });
                    self.record_dimension_entity_points(i, None, handle, block_path);
                    // HATCHEDIT: after pick, inject hatch model data into the command.
                    if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.name() == "HATCHEDIT")
                        .unwrap_or(false)
                    {
                        if let Some(model) = self.tabs[i].scene.hatches.get(&handle).cloned() {
                            let entity = self.tabs[i].scene.document.get_entity(handle);
                            let annotative = entity.is_some_and(|entity| {
                                crate::scene::annotative::is_annotative(
                                    &self.tabs[i].scene.document,
                                    entity,
                                )
                            });
                            let (scale, angle) = match entity {
                                Some(codec::EntityType::Hatch(hatch)) => (
                                    hatch.pattern_scale as f32,
                                    hatch.pattern_angle.to_degrees() as f32,
                                ),
                                _ => (model.scale, model.angle_offset.to_degrees()),
                            };
                            use crate::command::CadCommand;
                            use crate::modules::draw::draw::hatchedit::HatcheditCommand;
                            let current_color =
                                self.tabs[i].scene.document.header.current_entity_color;
                            let current_transparency =
                                self.tabs[i].scene.document.current_entity_transparency();
                            let current_origin = self.tabs[i].scene.document.hatch_origin();
                            let cmd: Box<dyn CadCommand> = Box::new(
                                HatcheditCommand::with_handle(
                                    handle,
                                    model.name.clone(),
                                    scale,
                                    angle,
                                    annotative,
                                )
                                .with_appearance(entity, current_color, current_transparency)
                                .with_origin(current_origin),
                            );
                            self.command_line.push_info(&cmd.prompt());
                            self.tabs[i].active_cmd = Some(cmd);
                        } else {
                            self.command_line
                                .push_error(crate::t!("HATCHEDIT: not a hatch entity.").as_ref());
                            self.tabs[i].active_cmd = None;
                        }
                    }
                    // DIMTEDIT: inject cloned entity via trait.
                    {
                        let needs_inject = self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .map(|c| {
                                matches!(c.name(), "DIMTEDIT")
                            })
                            .unwrap_or(false);
                        if needs_inject {
                            if let Some(entity) =
                                self.tabs[i].scene.document.get_entity(handle).cloned()
                            {
                                if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                                    cmd.inject_picked_entity(entity);
                                    let prompt = cmd.prompt();
                                    self.command_line.push_info(&prompt);
                                }
                            }
                        }
                    }
                    result
                } else if let Some(result) = {
                    let dimension = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.measures_through_viewports());
                    if dimension {
                        let per_px = self.tabs[i].scene.camera.borrow().ortho_size() as f64 * 2.0
                            / (bounds.height as f64).max(1.0);
                        self.try_dimension_viewport_entity_pick(
                            i,
                            pick_wcs,
                            crate::ui::overlay::pick_box_aperture_px(self.pick_box) as f64 * per_px,
                        )
                    } else {
                        None
                    }
                } {
                    Some(result)
                } else if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.entity_pick_accepts_points())
                {
                    let pending = self.pending_click_snap.take();
                    if !self.record_accepted_snap(
                        i,
                        pending.map(|(hit, _)| hit),
                        pending.and_then(|(_, frame)| frame),
                        pick_wcs,
                    ) {
                        self.finish_command_click(i);
                        return Task::none();
                    }
                    self.refresh_command_point_pick_context(i);
                    self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|command| command.on_point(pick_wcs))
                } else if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.accepts_drag_selection())
                {
                    let anchor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                    let mut selection = self.tabs[i].scene.selection.borrow_mut();
                    selection.gesture.box_anchor = Some(p_full);
                    selection.gesture.box_current = Some(p_full);
                    selection.gesture.box_anchor_world = Some(anchor_world);
                    if !selection.gesture.box_crossing_locked {
                        selection.gesture.box_crossing = false;
                    }
                    None
                } else {
                    self.command_line
                        .push_info(crate::t!("Nothing found at that point.").as_ref());
                    None
                }
            } else if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_tangent_pick())
                .unwrap_or(false)
            {
                if let Some(obj) = tangent_obj_at_click {
                    self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_tangent_point(obj, pick_wcs))
                } else {
                    self.command_line
                        .push_info(crate::t!("Select a tangent object.").as_ref());
                    None
                }
            } else if !self.command_point_allowed(i, world_pt) {
                None
            } else {
                // A scalar typed into the dynamic-input box but not
                // yet confirmed with Enter is applied before the
                // point pick — e.g. an OFFSET distance typed and then
                // clicked takes effect rather than being discarded.
                let wants_text = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.input_kind().wants_text())
                    .unwrap_or(false);
                if wants_text {
                    if let Some(text) = self.tabs[i]
                        .dyn_fields
                        .iter()
                        .find_map(|f| f.buffer.clone())
                    {
                        let text = crate::app::expr_eval::eval_to_string(text.trim());
                        if let Some(c) = self.tabs[i].active_cmd.as_mut() {
                            c.on_text_input(&text);
                        }
                        for f in self.tabs[i].dyn_fields.iter_mut() {
                            f.buffer = None;
                        }
                        self.tabs[i].dyn_active = 0;
                    }
                }
                // Record what this point step accepted, alongside the point
                // itself: paper point, model point, viewport, frame and
                // geometry identity. Commands that don't care keep consuming
                // `world_pt` exactly as before.
                let pending = self.pending_click_snap.take();
                if !self.record_accepted_snap(
                    i,
                    pending.map(|(hit, _)| hit),
                    pending.and_then(|(_, frame)| frame),
                    world_pt,
                ) {
                    self.finish_command_click(i);
                    return Task::none();
                }
                // An unsnapped first point on a solid face lands on the face.
                let world_pt = match self.apply_dynamic_ucs(snapped.then_some(world_pt)) {
                    Some(on_face) if !snapped => on_face,
                    _ => world_pt,
                };
                self.last_point = Some(world_pt);
                // The one-shot snap override is spent by this pick —
                // restore the running osnap configuration (#337).
                self.snapper.clear_override();
                self.dyn_user_reshaped = false;
                self.dyn_coord_absolute = false;
                self.sync_dyn_fields();
                self.reset_tracking_after_point();
                // A running Tangent snap may carry a tangent object; a
                // command can consume it to resolve a deferred tangent
                // (LINE tangent to two circles, which needs both). When
                // it does, sync last_point to the command's resolved
                // anchor since it replaced the picked coordinate.
                self.refresh_command_point_pick_context(i);
                let handled = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .and_then(|c| c.on_point_with_tangent(world_pt, tangent_obj_at_click));
                if handled.is_some() {
                    if let Some(a) = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .and_then(|c| c.resolved_anchor())
                    {
                        self.last_point = Some(a);
                    }
                    handled
                } else {
                    self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_point(world_pt))
                }
            };

            let panel_updates = self.tabs[i]
                .active_cmd
                .as_mut()
                .map(|c| c.take_pending_panel_updates())
                .unwrap_or_default();
            for (panel_id, values) in panel_updates {
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    panel.update_values(values);
                }
            }

            self.sync_dimension_snaps(i);
            if let Some(r) = result {
                let task = self.apply_cmd_result(r);
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.input.left_down = false;
                sel.input.left_press_pos = None;
                sel.input.left_press_time = None;
                sel.input.left_dragging = false;
                drop(sel);
                self.arm_resolved_command_snap_dwell(i, p_full);
                return task;
            }
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.left_down = false;
            sel.input.left_press_pos = None;
            sel.input.left_press_time = None;
            sel.input.left_dragging = false;
            drop(sel);
            self.arm_resolved_command_snap_dwell(i, p_full);
            return Task::none();
        }

        let (is_down2, is_dragging, box_anchor, box_crossing, _vp_size, poly_drag) = {
            let sel = self.tabs[i].scene.selection.borrow();
            (
                sel.input.left_down,
                sel.input.left_dragging,
                sel.gesture.box_anchor,
                sel.gesture.box_crossing,
                sel.view.vp_size,
                sel.gesture.poly_active,
            )
        };

        let mut selection_just_completed = false;

        // Active-tile-local selection: tile-sized bounds and the box
        // anchor mapped into the tile, so box / crossing selection
        // matches the active pane (p is already tile-local).
        let vp_size = (tile_vw, tile_vh);
        let box_anchor = box_anchor.map(|a| iced::Point {
            x: a.x - tile_off.x,
            y: a.y - tile_off.y,
        });

        if is_down2 {
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vp_size.0,
                height: vp_size.1,
            };

            if is_dragging {
                let drag_geometry = if poly_drag {
                    let mut points: Vec<iced::Point> = self.tabs[i]
                        .scene
                        .selection
                        .borrow()
                        .gesture
                        .poly_points
                        .iter()
                        .map(|point| iced::Point {
                            x: point.x - tile_off.x,
                            y: point.y - tile_off.y,
                        })
                        .collect();
                    if points.last().is_none_or(|last| {
                        (last.x - p.x).abs() > f32::EPSILON || (last.y - p.y).abs() > f32::EPSILON
                    }) {
                        points.push(p);
                    }
                    if let Some(first) = points.first().copied() {
                        points.push(first);
                    }
                    let fence = points
                        .into_iter()
                        .map(|point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    Some((fence, None))
                } else if let Some(anchor) = box_anchor {
                    let points = [
                        anchor,
                        iced::Point::new(anchor.x, p.y),
                        p,
                        iced::Point::new(p.x, anchor.y),
                        anchor,
                    ];
                    let fence = points
                        .into_iter()
                        .map(|point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    let (min, max) = fence.iter().fold(
                        (
                            [f64::INFINITY, f64::INFINITY],
                            [f64::NEG_INFINITY, f64::NEG_INFINITY],
                        ),
                        |(mut min, mut max), point| {
                            min[0] = min[0].min(point[0]);
                            min[1] = min[1].min(point[1]);
                            max[0] = max[0].max(point[0]);
                            max[1] = max[1].max(point[1]);
                            (min, max)
                        },
                    );
                    Some((fence, Some((min, max))))
                } else {
                    None
                };

                if let Some((fence, window)) = drag_geometry {
                    if self.tabs[i].active_cmd.is_none() {
                        self.tabs[i].last_crossing_window = window.filter(|_| box_crossing);
                    }
                    let result = self.tabs[i].active_cmd.as_mut().and_then(|command| {
                        command.set_shift(self.shift_down);
                        command.on_drag_selection(&fence, window, box_crossing)
                    });
                    if let Some(result) = result {
                        let mut selection = self.tabs[i].scene.selection.borrow_mut();
                        selection.input.left_down = false;
                        selection.input.left_press_pos = None;
                        selection.input.left_press_time = None;
                        selection.input.left_dragging = false;
                        selection.gesture.poly_active = false;
                        selection.gesture.poly_points.clear();
                        selection.gesture.poly_crossing = false;
                        selection.gesture.box_anchor = None;
                        selection.gesture.box_anchor_world = None;
                        selection.gesture.box_current = None;
                        selection.gesture.box_crossing = false;
                        selection.gesture.box_crossing_locked = false;
                        drop(selection);
                        return self.apply_cmd_result(result);
                    }
                }

                if !poly_drag {
                    // PICKDRAG 1 (#226): the press-drag spanned a
                    // rectangle through the box machinery — complete
                    // it here on release.
                    if let Some(a) = box_anchor {
                        let crossing = box_crossing;
                        let (mut handles, timing) =
                            self.box_selection_handles(i, &edit_cam, bounds, a, p, crossing);
                        // The drag gesture applies the selection filter as the click one does.
                        handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
                        self.filter_handles_reject_locked(i, &mut handles);
                        let cand_ms = timing.candidates;
                        // Box/lasso accumulates like individual picks
                        // (issue #83): a plain box adds to the current
                        // selection, Shift+box removes the boxed
                        // entities. Esc / empty-space click still clears.
                        // PICKADD 0 (#226): a plain box REPLACES.
                        let hit_ms = timing.hit;
                        let t_apply = crate::perf::enabled().then(Instant::now);
                        if self.shift_down || self.select_remove_mode {
                            self.tabs[i].scene.deselect_entities(&handles);
                        } else {
                            if !selection_pick_add && !handles.is_empty() {
                                self.tabs[i].scene.deselect_all();
                            }
                            self.tabs[i].scene.select_entities(&handles);
                            self.tabs[i].scene.expand_selection_for_groups(&handles);
                        }
                        self.pick_authoring_box(a, p, crossing, !handles.is_empty());
                        let apply_ms = t_apply.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
                        let t_props = crate::perf::enabled().then(Instant::now);
                        self.refresh_properties();
                        if crate::perf::enabled() {
                            crate::perf_record!(
                                "[perf] select-commit kind=drag-box crossing={crossing} \
candidates={cand_ms:.1}ms hit={hit_ms:.1}ms apply={apply_ms:.1}ms \
properties={:.1}ms picked={}",
                                t_props.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0),
                                handles.len(),
                            );
                        }
                        selection_just_completed = true;
                    }
                } else {
                    let (poly_pts, crossing) = {
                        let sel = self.tabs[i].scene.selection.borrow();
                        // Map lasso points into the active tile.
                        let pts: Vec<iced::Point> = sel
                            .gesture
                            .poly_points
                            .iter()
                            .map(|pp| iced::Point {
                                x: pp.x - tile_off.x,
                                y: pp.y - tile_off.y,
                            })
                            .collect();
                        (pts, sel.gesture.poly_crossing)
                    };
                    let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
                    let mut handles = self.tabs[i].scene.path_hit_handles(
                        &poly_pts,
                        crossing,
                        false,
                        all_wires,
                        view_rot,
                        eye,
                        bounds,
                        |point| self.cursor_model_point(i, &edit_cam, point, bounds),
                    );
                    handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
                    self.filter_handles_reject_locked(i, &mut handles);
                    // Accumulate like the box path (issue #83): plain
                    // lasso adds, Shift+lasso removes. An empty lasso
                    // leaves the current selection untouched so a stray
                    // drag never discards hard-won picks.
                    if is_gathering {
                        self.last_pick =
                            Some((handles.clone(), self.shift_down || self.select_remove_mode));
                    }
                    if self.shift_down || self.select_remove_mode {
                        self.tabs[i].scene.deselect_entities(&handles);
                    } else {
                        // PICKADD 0 (#226): a plain marquee REPLACES
                        // the selection (empty results still leave it
                        // alone, matching the box rule).
                        if !selection_pick_add && !handles.is_empty() {
                            self.tabs[i].scene.deselect_all();
                        }
                        self.tabs[i].scene.select_entities(&handles);
                        self.tabs[i].scene.expand_selection_for_groups(&handles);
                    }
                    self.refresh_properties();
                    selection_just_completed = true;
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.gesture.poly_active = false;
                sel.gesture.poly_points.clear();
                sel.gesture.poly_crossing = false;
                sel.gesture.box_anchor = None;
                sel.gesture.box_anchor_world = None;
                sel.gesture.box_current = None;
            } else {
                if box_anchor.is_none() {
                    // A parametric-constraint glyph draws above its geometry
                    // and takes click priority over the entity beneath it.
                    let glyph_hit = (self.tabs[i].scene.current_layout == "Model")
                        .then(|| {
                            let scope = self.tabs[i].current_parametric_scope();
                            self.tabs[i].scene.constraint_glyph_hit(
                                scope,
                                canvas_sz,
                                self.show_constraint_values,
                                self.constraint_bar_display,
                                self.constraint_bar_mode,
                                p_full,
                            )
                        })
                        .flatten();
                    if let Some(id) = glyph_hit {
                        self.tabs[i].scene.deselect_all();
                        self.tabs[i].scene.selected_constraint = Some(id);
                        self.refresh_properties();
                        selection_just_completed = true;
                    }
                    if glyph_hit.is_none() {
                        let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
                        let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                        let t_arm = crate::perf::enabled().then(Instant::now);
                        let prior_selection = self.tabs[i].scene.selected.len();
                        let pick_aperture = crate::ui::overlay::pick_box_aperture_px(self.pick_box);
                        let query_started = crate::perf::enabled().then(Instant::now);
                        let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                            all_wires,
                            click_world,
                            view_rot,
                            eye,
                            bounds,
                            pick_aperture * 2.0,
                        );
                        let candidate_ms = crate::perf::elapsed_ms(query_started);
                        let handles_started = crate::perf::enabled().then(Instant::now);
                        let candidate_handles = self.tabs[i]
                            .scene
                            .interaction_candidate_handles(&click_candidates);
                        let handles_ms = crate::perf::elapsed_ms(handles_started);

                        // Selection cycling: where two or more objects
                        // overlap, open a list box to pick which one; a
                        // single object falls through to the normal click.
                        // Gated behind the toggle, so default picking is
                        // unchanged when off.
                        let mut handled_by_cycling = false;
                        if self.selection_cycling {
                            let cands: Vec<Handle> = scene::pick::hit_test::click_hits_all(
                                p,
                                &click_candidates,
                                view_rot,
                                eye,
                                bounds,
                                self.tabs[i].scene.document.header.lineweight_display,
                                pick_aperture,
                            )
                            .into_iter()
                            .filter_map(|s| Scene::handle_from_wire_name(s))
                            .filter(|&h| self.tabs[i].scene.passes_selection_filter(h))
                            .filter(|&h| !self.tabs[i].scene.is_reference_only(h))
                            .collect();
                            // An object drawn by several wires (a dimension's
                            // lines, arrows and text) is one candidate.
                            let mut seen = std::collections::HashSet::new();
                            let cands: Vec<Handle> =
                                cands.into_iter().filter(|h| seen.insert(*h)).collect();
                            if cands.len() >= 2 {
                                // Overlap: open the list box at the cursor.
                                self.cycle_candidates = Some((p_full, cands));
                                handled_by_cycling = true;
                            }
                        }

                        if !handled_by_cycling {
                            let wire_started = crate::perf::enabled().then(Instant::now);
                            let mut hit = scene::pick::hit_test::click_hit(
                                p,
                                &click_candidates,
                                view_rot,
                                eye,
                                bounds,
                                self.tabs[i].scene.document.header.lineweight_display,
                                pick_aperture,
                                &self.tabs[i].scene.draw_depth_map(),
                            )
                            .and_then(|s| Scene::handle_from_wire_name(s));
                            let wire_ms = crate::perf::elapsed_ms(wire_started);
                            let mut hatch_models_ms = 0.0;
                            let mut hatch_test_ms = 0.0;
                            let mut hatch_count = 0usize;
                            if hit.is_none() {
                                let models_started = crate::perf::enabled().then(Instant::now);
                                let hatches = self.tabs[i]
                                    .scene
                                    .visible_hatches_for_click(candidate_handles.as_ref());
                                hatch_count = hatches.len();
                                hatch_models_ms = crate::perf::elapsed_ms(models_started);
                                let hatch_started = crate::perf::enabled().then(Instant::now);
                                hit = scene::pick::hit_test::click_hit_hatch(
                                    p,
                                    &hatches,
                                    view_rot,
                                    eye,
                                    bounds,
                                    candidate_handles.as_ref(),
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
                                    candidate_handles.as_ref(),
                                );
                                insert_ms = crate::perf::elapsed_ms(insert_started);
                            }
                            let mut solid_ms = 0.0;
                            if hit.is_none() {
                                // Normal 3D selection follows the displayed B-rep
                                // edges. Face-interior picking remains reserved for
                                // modelling commands that explicitly request it.
                                let solid_started = crate::perf::enabled().then(Instant::now);
                                hit = self.tabs[i].scene.solid_edge_click_hit(
                                    p,
                                    view_rot,
                                    eye,
                                    bounds,
                                    candidate_handles.as_ref(),
                                    pick_aperture,
                                );
                                solid_ms = crate::perf::elapsed_ms(solid_started);
                            }
                            let hit_hatch = hit.is_some_and(|handle| {
                                matches!(
                                    self.tabs[i].scene.document.get_entity(handle),
                                    Some(codec::EntityType::Hatch(_))
                                )
                            });
                            let pick_ms = crate::perf::elapsed_ms(t_arm);
                            if crate::perf::enabled() && pick_ms >= 1.0 {
                                crate::perf_record!(
                                    "[perf] select-click total={pick_ms:.1}ms candidates={candidate_ms:.2}ms handles={handles_ms:.2}ms wire={wire_ms:.2}ms hatch-models={hatch_models_ms:.2}ms hatch-test={hatch_test_ms:.2}ms insert={insert_ms:.2}ms solid={solid_ms:.2}ms candidate_wires={} hatch_candidates={hatch_count} hit_hatch={hit_hatch}",
                                    click_candidates.len(),
                                );
                            }
                            // Selection filter: drop a pick whose type is excluded.
                            let hit =
                                hit.filter(|&h| self.tabs[i].scene.passes_selection_filter(h));
                            if let Some(handle) = hit {
                                if is_gathering {
                                    self.last_pick = Some((
                                        vec![handle],
                                        self.shift_down || self.select_remove_mode,
                                    ));
                                }
                                if self.tabs[i]
                                    .active_cmd
                                    .as_ref()
                                    .is_some_and(|c| c.selection_rejects_locked_layers())
                                    && self.tabs[i].scene.is_layer_locked(handle)
                                {
                                    self.command_line.push_info("1 was on a locked layer.");
                                } else {
                                    // Individual picks accumulate (issue #47):
                                    // each plain click adds to the selection,
                                    // Shift+click removes the picked entity.
                                    // PICKADD 0 (#226): a plain click
                                    // REPLACES the selection instead and
                                    // Shift+click toggles membership.
                                    if self.shift_down || self.select_remove_mode {
                                        // Remove was asked for by name, so it only
                                        // ever takes away — the toggle below is
                                        // Shift's PICKADD-0 behaviour, not its.
                                        if !self.select_remove_mode
                                            && !selection_pick_add
                                            && !self.tabs[i].scene.selected.contains(&handle)
                                        {
                                            self.tabs[i].scene.select_entity(handle, false);
                                            self.tabs[i].scene.expand_selection_for_groups(&[handle]);
                                        } else {
                                            self.tabs[i].scene.deselect_entity(handle);
                                        }
                                    } else {
                                        self.tabs[i]
                                            .scene
                                            .select_entity(handle, !selection_pick_add);
                                        self.tabs[i].scene.expand_selection_for_groups(&[handle]);
                                    }
                                    self.refresh_properties();
                                    selection_just_completed = true;
                                }
                            } else {
                                // Empty-space click only ARMS a box here; it
                                // no longer clears the selection, so a box can
                                // add to it (issue #83). The box completion
                                // (or Esc) decides what happens to the
                                // selection.
                                // PICKADD 0 (#226): OS convention — the
                                // empty click also drops the selection.
                                if !selection_pick_add && !self.shift_down {
                                    self.tabs[i].scene.deselect_all();
                                    self.refresh_properties();
                                }
                                // Pin the anchor to the world point under it
                                // so a zoom/pan mid-drag re-projects it
                                // instead of leaving it frozen in pixels
                                // (#234). Computed before the selection
                                // borrow so the &self projection can't clash.
                                let anchor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                                if let Some(t) = t_arm {
                                    let arm_ms = t.elapsed().as_secs_f64() * 1000.0;
                                    if arm_ms >= 5.0 {
                                        crate::perf_record!(
                                            "[perf] select-arm {arm_ms:>7.1}ms pick+clear, \
was_selected={}",
                                            prior_selection,
                                        );
                                    }
                                }
                                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                                // Full-canvas space: ViewportMove updates
                                // box_current in canvas coords and the overlay
                                // draws there; release maps back into the tile.
                                sel.gesture.box_anchor = Some(p_full);
                                sel.gesture.box_current = Some(p_full);
                                sel.gesture.box_anchor_world = Some(anchor_world);
                                if !sel.gesture.box_crossing_locked {
                                    sel.gesture.box_crossing = false;
                                }
                            }
                        }
                    }
                } else {
                    let a = box_anchor.unwrap();
                    let box_points = [
                        a,
                        iced::Point::new(a.x, p.y),
                        p,
                        iced::Point::new(p.x, a.y),
                        a,
                    ];
                    let command_fence = box_points
                        .into_iter()
                        .map(|point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    let (command_min, command_max) = command_fence.iter().fold(
                        (
                            [f64::INFINITY, f64::INFINITY],
                            [f64::NEG_INFINITY, f64::NEG_INFINITY],
                        ),
                        |(mut min, mut max), point| {
                            min[0] = min[0].min(point[0]);
                            min[1] = min[1].min(point[1]);
                            max[0] = max[0].max(point[0]);
                            max[1] = max[1].max(point[1]);
                            (min, max)
                        },
                    );
                    let command_result = self.tabs[i].active_cmd.as_mut().and_then(|command| {
                        command.set_shift(self.shift_down);
                        command.on_drag_selection(
                            &command_fence,
                            Some((command_min, command_max)),
                            box_crossing,
                        )
                    });
                    if let Some(result) = command_result {
                        let mut selection = self.tabs[i].scene.selection.borrow_mut();
                        selection.input.left_down = false;
                        selection.input.left_press_pos = None;
                        selection.input.left_press_time = None;
                        selection.input.left_dragging = false;
                        selection.gesture.box_anchor = None;
                        selection.gesture.box_anchor_world = None;
                        selection.gesture.box_current = None;
                        selection.gesture.box_crossing = false;
                        selection.gesture.box_crossing_locked = false;
                        drop(selection);
                        return self.apply_cmd_result(result);
                    }

                    let crossing = box_crossing;
                    let (mut handles, timing) =
                        self.box_selection_handles(i, &edit_cam, bounds, a, p, crossing);
                    let (cand_ms, hit_ms) = (timing.candidates, timing.hit);
                    let (m_handles, m_wires, m_hatch) =
                        (Some(timing.handles), Some(timing.wires), Some(timing.hatch));
                    let t_filter = crate::perf::enabled().then(Instant::now);
                    // Selection filter: keep only allowed types.
                    handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
                    self.filter_handles_reject_locked(i, &mut handles);
                    let filter_ms = t_filter.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
                    let t_apply = crate::perf::enabled().then(Instant::now);
                    // Accumulate (issue #83): a plain box adds to the
                    // current selection, Shift+box removes the boxed
                    // entities. An empty box leaves the selection alone
                    // so an accidental empty drag never discards it.
                    // PICKADD 0 (#226): a plain box REPLACES instead.
                    if is_gathering {
                        self.last_pick =
                            Some((handles.clone(), self.shift_down || self.select_remove_mode));
                    }
                    if self.shift_down || self.select_remove_mode {
                        self.tabs[i].scene.deselect_entities(&handles);
                    } else {
                        if !selection_pick_add && !handles.is_empty() {
                            self.tabs[i].scene.deselect_all();
                        }
                        self.tabs[i].scene.select_entities(&handles);
                        self.tabs[i].scene.expand_selection_for_groups(&handles);
                    }
                    let apply_ms = t_apply.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
                    let t_props = crate::perf::enabled().then(Instant::now);
                    self.pick_authoring_box(a, p, crossing, !handles.is_empty());
                    self.refresh_properties();
                    if crate::perf::enabled() {
                        crate::perf_record!(
                            "[perf] select-commit kind=window crossing={crossing} \
candidates={cand_ms:.1}ms hit={hit_ms:.1}ms [handles={:.1} wires={:.1} \
hatch={:.1} mesh={:.1}] filter={filter_ms:.1}ms apply={apply_ms:.1}ms \
properties={:.1}ms picked={}",
                            m_handles.unwrap_or(0.0),
                            m_wires.unwrap_or(0.0) - m_handles.unwrap_or(0.0),
                            m_hatch.unwrap_or(0.0) - m_wires.unwrap_or(0.0),
                            hit_ms - m_hatch.unwrap_or(0.0),
                            t_props.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0),
                            handles.len(),
                        );
                    }
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    sel.gesture.box_anchor = None;
                    sel.gesture.box_anchor_world = None;
                    sel.gesture.box_current = None;
                    sel.gesture.box_crossing = false;
                    sel.gesture.box_crossing_locked = false;
                    selection_just_completed = true;
                }
            }

            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.left_down = false;
            sel.input.left_press_pos = None;
            sel.input.left_press_time = None;
            sel.input.left_dragging = false;
        }

        if selection_just_completed {
            self.quick_properties_anchor = p_full;
        }

        if is_gathering && selection_just_completed {
            let handles: Vec<Handle> = self.tabs[i]
                .scene
                .selected_entities()
                .into_iter()
                .map(|(h, _)| h)
                .collect();
            return self.feed_command(crate::command::StepInput::SelectionComplete(handles));
        }

        // ── Double-click in Model Space: DDEDIT for Text/MText ────
        if is_click
            && is_down
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].scene.current_layout == "Model"
        {
            let now = Instant::now();
            let is_double_model = self
                .last_vp_click_time
                .map(|t| {
                    let dt = now.duration_since(t).as_millis();
                    let last = self.last_vp_click_pos.unwrap_or(p);
                    let d = (p.x - last.x).hypot(p.y - last.y);
                    dt < 400 && d < 8.0
                })
                .unwrap_or(false);

            self.last_vp_click_time = Some(now);
            self.last_vp_click_pos = Some(p);

            if is_double_model {
                let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                let (view_rot, eye) = {
                    let c = self.tabs[i].scene.camera.borrow();
                    (c.view_proj_rte(bounds), c.eye())
                };
                let all_wires = self.tabs[i].scene.hit_test_wires();
                let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
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
                .and_then(|s| Scene::handle_from_wire_name(s))
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
                        candidate_handles.as_ref(),
                    )
                })
                .or_else(|| {
                    self.tabs[i].scene.solid_edge_click_hit(
                        p,
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                        crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                    )
                });
                if let Some(handle) = hit {
                    // Locked layer: double-click must not open any editor
                    // (text / attribute / in-place block edit).
                    if let Some(layer) = self.tabs[i].scene.locked_layer_name(handle) {
                        self.command_line.push_info(
                            crate::tf!(
                            "Object is on locked layer \"{layer}\" — unlock the layer to edit it."
                        )
                            .as_ref(),
                        );
                        return Task::none();
                    }
                    // Table double-click: shared TABLEDIT editor setup. A
                    // grid miss returns NoCell and falls through to the
                    // other text/entity editors below; an editable cell or
                    // a locked cell (indicator armed, no editor) ends the
                    // double-click handling here.
                    if self.begin_table_cell_edit(i, handle, click_world)
                        != crate::modules::annotate::table_cmd::TableCellEditStart::NoCell
                    {
                        return Task::none();
                    }
                    // A constraint dimension edits its parameter from the
                    // command line, standing in for the reference's in-place
                    // value editor.
                    if let Some(prompt) = self.dynamic_dimension_value_command(i, handle) {
                        use crate::command::CadCommand;
                        self.command_line.push_info(&prompt.prompt());
                        self.tabs[i].active_cmd = Some(Box::new(prompt));
                        return Task::none();
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
                        return self.begin_text_edit(handle);
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
                        return self.dispatch_command("HATCHEDIT");
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
                        return Task::done(Message::AttrEditorOpen(handle));
                    }
                    let is_insert = matches!(
                        self.tabs[i].scene.document.get_entity(handle),
                        Some(AcadEntityType::Insert(_))
                    );
                    if is_insert && self.tabs[i].refedit_session.is_none() {
                        if self.block_editor_preferences.locked && !self.double_click_block_refedit
                        {
                            self.tabs[i].scene.deselect_all();
                            self.tabs[i].scene.select_entity(handle, true);
                            self.tabs[i].selected_handle = Some(handle);
                            self.refresh_properties();
                            return self.dock_set_open(crate::ui::dock::PanelId::Properties, true);
                        }
                        let command = if self.double_click_block_refedit {
                            "REFEDIT_BEGIN"
                        } else {
                            "BEDIT_BEGIN"
                        };
                        return Task::done(Message::Command(format!(
                            "{}:{}",
                            command,
                            handle.value()
                        )));
                    }
                }
            }
        }

        // ── Double-click: enter/exit MSPACE ───────────────────────
        // Only when no command is running, no drag, and we're in paper space.
        if is_click
                    && is_down   // ensures there was a matching left-press
                    && self.tabs[i].active_cmd.is_none()
                    && self.tabs[i].scene.current_layout != "Model"
        {
            let now = Instant::now();
            let is_double = self
                .last_vp_click_time
                .map(|t| {
                    let dt = now.duration_since(t).as_millis();
                    let last = self.last_vp_click_pos.unwrap_or(p);
                    let d = (p.x - last.x).hypot(p.y - last.y);
                    dt < 400 && d < 8.0
                })
                .unwrap_or(false);

            self.last_vp_click_time = Some(now);
            self.last_vp_click_pos = Some(p);

            if is_double {
                let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };

                let (view_rot, eye) = {
                    let c = self.tabs[i].scene.camera.borrow();
                    (c.view_proj_rte(bounds), c.eye())
                };

                let all_wires = self.tabs[i].scene.hit_test_wires();
                let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                    all_wires,
                    click_world,
                    view_rot,
                    eye,
                    bounds,
                    crate::ui::overlay::pick_box_aperture_px(self.pick_box) * 2.0,
                );

                let raw_wire_hit = scene::pick::hit_test::click_hit(
                    p,
                    &click_candidates,
                    view_rot,
                    eye,
                    bounds,
                    self.tabs[i].scene.document.header.lineweight_display,
                    crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                    &self.tabs[i].scene.draw_depth_map(),
                )
                .and_then(|s| Scene::handle_from_wire_name(s));

                // In paper space, text editing takes precedence over entering MSPACE.
                // This mirrors the model-space double-click behaviour.
                if let Some(handle) = raw_wire_hit {
                    let is_editable_text = self.tabs[i]
                        .scene
                        .document
                        .get_entity(handle)
                        .is_some_and(|entity| {
                            crate::app::text_inline::read_text_field(entity).is_some()
                                || matches!(entity, AcadEntityType::Leader(_))
                        });

                    if is_editable_text {
                        if let Some(layer) = self.tabs[i].scene.locked_layer_name(handle) {
                            self.command_line.push_info(
                                crate::tf!(
                                    "Object is on locked layer \"{layer}\" — unlock the layer to edit it."
                                )
                                .as_ref(),
                            );
                            return Task::none();
                        }

                        return self.begin_text_edit(handle);
                    }
                }

                // If the double-click was not on editable text, keep the existing
                // paper-space behaviour and try to enter the clicked viewport.
                let wire_hit: Option<codec::Handle> = raw_wire_hit.and_then(|h| {
                    if let Some(AcadEntityType::Viewport(vp)) =
                        self.tabs[i].scene.document.get_entity(h)
                    {
                        if !Scene::is_sheet_viewport(&self.tabs[i].scene.document, vp) {
                            Some(h)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                // 2) Decide which viewport (if any) the click enters,
                //    keyed on the *visible* on-screen rectangle. Screen-space
                //    (not the full paper rect) so a click on the empty area
                //    beside an off-screen viewport doesn't match its
                //    partly-off-canvas rect and switch to it by mistake.
                //    The wire hit only refines *which* visible viewport
                //    (border precision / overlap), and only when the click
                //    actually lands inside THAT viewport's visible rect —
                //    otherwise its border-wire pick tolerance could match a
                //    far viewport whose edge merely passes near the cursor
                //    (e.g. clicking the overlap of two viewports while a
                //    third's border runs nearby) and enter the wrong one.
                let screen_hit = self.tabs[i]
                    .scene
                    .viewport_at_screen_point(p.x, p.y, (vw, vh));
                let wire_in_visible = wire_hit.filter(|&h| {
                    self.tabs[i]
                        .scene
                        .viewport_screen_rect(h, (vw, vh))
                        .is_some_and(|r| {
                            let x0 = r.x.max(0.0);
                            let y0 = r.y.max(0.0);
                            let x1 = (r.x + r.width).min(vw);
                            let y1 = (r.y + r.height).min(vh);
                            p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1
                        })
                });
                let hit_vp = wire_in_visible.or(screen_hit);

                if let Some(handle) = hit_vp {
                    return Task::done(Message::EnterViewport(handle));
                } else if self.tabs[i].scene.active_viewport.is_some() {
                    // Double-clicked outside all viewports while in MSPACE → exit.
                    return Task::done(Message::ExitViewport);
                }
            }
        }

        Task::none()
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
    fn arm_resolved_command_snap_dwell(&mut self, i: usize, point: iced::Point) {
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
            });
        }
    }

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
        });
    }

    pub(super) fn on_viewport_middle_press(&mut self) -> Task<Message> {
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

    /// Apply the user's scroll navigation preference to either delta unit.
    pub(super) fn on_viewport_scroll(&mut self, delta: mouse::ScrollDelta) -> Task<Message> {
        match scroll_intent(delta, self.touchpad_pan, self.ctrl_down) {
            ScrollIntent::Zoom { notches } => {
                crate::perf::note_scroll_zoom();
                let mut s = notches * crate::app::settings::zoom_notch_steps(self.zoom_factor);
                if self.zoom_wheel_reversed {
                    s = -s;
                }
                self.zoom_view_at_cursor(s)
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
    pub(super) fn on_touch(&mut self, event: iced::touch::Event) -> Task<Message> {
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
                TouchAction::Orbit(delta) => {
                    if self.tabs[i].scene.active_viewport.is_some() {
                        self.tabs[i].scene.orbit_active_viewport(delta.x, delta.y);
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
                            .orbit(delta.x, delta.y, Some(pivot));
                    }
                    self.tabs[i].scene.camera_generation += 1;
                }
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

    pub(super) fn on_pinch_zoom(&mut self, magnification: f32) -> Task<Message> {
        if magnification == 0.0 {
            return Task::none();
        }
        self.zoom_view_at_cursor(pinch_zoom_steps(magnification))
    }

    /// Pan the active view by `dx`/`dy` screen pixels: the body shared by the
    /// middle-button drag and the two-finger gesture.
    fn pan_active_view(&mut self, i: usize, dx: f32, dy: f32, started: Instant) {
        // Pan scale uses the active tile's size (ortho size is relative to
        // viewport height), so a tiled pane pans at the correct rate.
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let bounds = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
        // A display-locked viewport keeps its scale: navigating inside it
        // pans the sheet instead.
        if self.tabs[i].scene.active_viewport.is_some()
            && !self.tabs[i].scene.navigation_locked()
        {
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
        self.tabs[i]
            .scene
            .record_nav_perf(crate::scene::NavPerfOp::Pan, started);
    }

    /// Zoom the active view by `s`, in the camera's own steps
    /// (`Camera::zoom`: `distance *= ZOOM_STEP.powf(s)`). Shared by the
    /// wheel and the pinch.
    fn zoom_view_at_cursor(&mut self, s: f32) -> Task<Message> {
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
        if self.tabs[i].scene.active_viewport.is_some()
            && !self.tabs[i].scene.navigation_locked()
        {
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
        self.tabs[i]
            .scene
            .record_nav_perf(crate::scene::NavPerfOp::Zoom, nav_started);
        self.arm_hover_after_navigation(i);
        Task::none()
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

    pub(super) fn on_viewport_click(
        &mut self,
        expected_viewport: Option<codec::Handle>,
    ) -> Task<Message> {
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

    pub(super) fn on_view_cube_snap(&mut self, region: CubeRegion) -> Task<Message> {
        // The cube is oriented in the active UCS, so snap in the UCS frame.
        let r_ucs = self.tabs[self.active_tab].scene.viewcube_ucs_mat();
        self.snap_view_region(region, r_ucs)
    }

    /// Compass cardinals are world-fixed, so they snap in the world frame
    /// (no UCS composition).
    pub(super) fn on_view_cube_snap_world(&mut self, region: CubeRegion) -> Task<Message> {
        self.snap_view_region(region, glam::Mat4::IDENTITY)
    }

    fn snap_view_region(&mut self, region: CubeRegion, r_ucs: glam::Mat4) -> Task<Message> {
        self.snap_view_region_as(region, r_ucs, true)
    }

    /// A standard view preset (-VIEW): the world view itself, kept when it
    /// is shown already, with no cube message.
    pub(in crate::app) fn snap_view_preset(&mut self, region: CubeRegion) -> Task<Message> {
        self.snap_view_region_as(region, glam::Mat4::IDENTITY, false)
    }

    fn snap_view_region_as(
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
        self.tabs[i].scene.camera_generation += 1;
        if cube {
            self.command_line
                .push_output(crate::tf!("View: {}", crate::t!(region.label())).as_ref());
        }
        Task::none()
    }

    pub(super) fn on_hover_dwell_tick(&mut self) -> Task<Message> {
        let Some(dwell) = self.hover_dwell.clone() else {
            return Task::none();
        };
        let dwell_ms = crate::app::HOVER_DWELL_MS;
        if Instant::now()
            .duration_since(dwell.last_move_at)
            .as_millis()
            < dwell_ms
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
        let mut hovered = scene::pick::hit_test::click_hit(
            p,
            &hover_candidates,
            view_rot,
            eye,
            bounds,
            self.tabs[i].scene.document.header.lineweight_display,
            crate::ui::overlay::pick_box_aperture_px(self.pick_box),
            &self.tabs[i].scene.draw_depth_map(),
        )
        .and_then(Scene::handle_from_wire_name);
        let wire_ms = wire_started.elapsed().as_secs_f64() * 1000.0;
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
                self.tabs[i].scene.solid_edge_hover_hit(
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
                    Some(codec::EntityType::Solid3D(_)),
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
                    matches!(entity, codec::EntityType::Solid3D(_))
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
        self.hover_dwell = None;
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
    pub(super) fn on_snap_dwell_tick(&mut self) -> Task<Message> {
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
        if Instant::now()
            .duration_since(dwell.last_move_at)
            .as_millis()
            < crate::app::SNAP_DWELL_MS
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
        self.on_viewport_move(dwell.point)
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
        Some(Task::perform(
            async move {
                let started = iced::time::Instant::now();
                let index = Arc::new(
                    crate::scene::pick::interaction_index::InteractionIndex::build(&wires),
                );
                index.prepare_screen();
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

    pub(crate) fn on_layout_switch(&mut self, name: String) -> Task<Message> {
        if self.tabs[self.active_tab].active_block_edit.is_some() {
            return self.request_block_edit_close(
                self.active_tab,
                crate::app::block_editor::BlockEditContinuation::Layout(name),
            );
        }
        self.on_layout_switch_inner(name, false)
    }

    pub(crate) fn on_block_edit_switch(&mut self, name: String) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            return Task::none();
        }
        let Some(target_index) = self.tabs[i]
            .block_edits
            .iter()
            .position(|session| session.block_name == name)
        else {
            self.command_line
                .push_error(crate::tf!("BEDIT: block tab \"{name}\" is not open.").as_ref());
            return Task::none();
        };
        if self.tabs[i].active_block_edit == Some(target_index) {
            return Task::none();
        }

        let cancel_task = self.cancel_active_command_for_space_change();
        let current_camera = self.tabs[i].scene.camera.borrow().clone();
        let current_ucs = self.tabs[i].active_ucs.clone();
        if let Some(active_index) = self.tabs[i].active_block_edit {
            if let Some(session) = self.tabs[i].block_edits.get_mut(active_index) {
                session.editor_camera = current_camera;
                session.editor_ucs = current_ucs;
            }
        } else {
            self.tabs[i].scene.sync_camera_to_document();
        }

        let (br_handle, editor_camera) = {
            let session = &self.tabs[i].block_edits[target_index];
            (session.br_handle, session.editor_camera.clone())
        };
        self.layout_rename_state = None;
        self.flip_popup = None;
        self.tabs[i].scene.active_viewport = None;
        self.tabs[i].scene.set_current_layout("Model".to_string());
        self.tabs[i].scene.block_edit_block = Some(br_handle);
        self.reset_block_constraint_display(i);
        self.tabs[i].active_block_edit = Some(target_index);
        *self.tabs[i].scene.camera.borrow_mut() = editor_camera;
        self.tabs[i].scene.camera_generation += 1;
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        self.tabs[i].scene.deselect_all();
        self.tabs[i].active_grip = None;
        self.grip_hover = None;
        self.grip_popup = None;
        self.visibility_popup = None;
        self.table_popup = None;
        self.tabs[i].refresh_active_ucs();
        self.tabs[i].scene.bump_geometry_no_blocks();
        self.refresh_properties();
        self.adopt_view_display(i);
        self.sync_dyn_fields();
        cancel_task
    }

    /// MVIEW's "Insert view > New" flow deliberately visits Model space to
    /// define a view and then returns to its paper layout. This is the only
    /// layout transition allowed to preserve an active command.
    pub(crate) fn on_layout_switch_preserving_command(&mut self, name: String) -> Task<Message> {
        if self.tabs[self.active_tab].active_block_edit.is_some() {
            return self.request_block_edit_close(
                self.active_tab,
                crate::app::block_editor::BlockEditContinuation::Layout(name),
            );
        }
        self.on_layout_switch_inner(name, true)
    }

    fn on_layout_switch_inner(
        &mut self,
        name: String,
        preserve_active_command: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            self.command_line
                .push_info(crate::t!("Open or create a drawing to switch layouts.").as_ref());
            return Task::none();
        }
        let perf = crate::perf::enabled();
        let perf_total = Instant::now();
        let leaving_block_edit = self.tabs[i].active_block_edit.is_some();
        let perf_from = self.tabs[i]
            .active_block_edit_session()
            .map(|session| session.block_name.clone())
            .unwrap_or_else(|| self.tabs[i].scene.current_layout.clone());
        let context_changed = leaving_block_edit
            || self.tabs[i].scene.current_layout != name
            || self.tabs[i].scene.active_viewport.is_some();
        let preserve_active_command = preserve_active_command
            && self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| matches!(command.name(), "MVIEW" | "MVSETUP"));
        if context_changed {
            if preserve_active_command {
                // MVIEW and MVSETUP keep their step data, but all host-owned
                // cursor/snap/dynamic-input state belongs to the old space.
                self.reset_space_interaction_state();
            }
        }
        let cancel_task = if context_changed && !preserve_active_command {
            self.cancel_active_command_for_space_change()
        } else {
            Task::none()
        };
        if let Some(active_index) = self.tabs[i].active_block_edit.take() {
            let camera = self.tabs[i].scene.camera.borrow().clone();
            let editor_ucs = self.tabs[i].active_ucs.clone();
            let (return_layout, return_camera) = {
                let session = &mut self.tabs[i].block_edits[active_index];
                session.editor_camera = camera;
                session.editor_ucs = editor_ucs;
                (session.return_layout.clone(), session.return_camera.clone())
            };
            let return_layout = if self.tabs[i].scene.layout_names().contains(&return_layout) {
                return_layout
            } else {
                "Model".to_string()
            };
            self.tabs[i].scene.block_edit_block = None;
            self.tabs[i].scene.set_current_layout(return_layout);
            *self.tabs[i].scene.camera.borrow_mut() = return_camera;
            self.tabs[i].scene.camera_generation += 1;
            self.tabs[i].scene.bump_geometry_no_blocks();
        }
        let going_to_paper = name != "Model";
        // Persist the camera of the layout we're leaving BEFORE switching
        // so returning to it restores where the user left off (the
        // periodic sync only fires on a tick, which may not have run
        // since the last pan/zoom).
        let perf_phase = Instant::now();
        self.tabs[i].scene.sync_camera_to_document();
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        let sync_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // Cancel any pending rename and active viewport when switching.
        self.layout_rename_state = None;
        self.tabs[i].scene.active_viewport = None;
        let perf_phase = Instant::now();
        self.tabs[i].scene.set_current_layout(name.clone());
        self.tabs[i].scene.deselect_all();
        let switch_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // UCS follows the active model, layout, or floating viewport pane.
        let perf_phase = Instant::now();
        self.tabs[i].refresh_active_ucs();
        self.tabs[i].scene.restore_saved_camera();
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        // `deselect_all` invalidates the scene highlight, but grips and the
        // Properties panel are separate caches owned by the app.
        self.refresh_properties();
        // Grid/snap are per-view: load the layout we just entered (its
        // sheet viewport in paper space, the model tile in model space)
        // so model and each layout keep independent grid state.
        self.adopt_view_display(i);
        let restore_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // Paper-space tools live in the right-edge side toolbar now, so
        // entering/leaving a layout no longer hijacks the ribbon tab.
        let _ = going_to_paper;
        // Refresh VP freeze columns for the new layout.
        let perf_phase = Instant::now();
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        // A walk in flight belongs to the space on screen, so it moves with
        // the layout instead of silently hiding a drawing the user left.
        self.walk_space_changed();
        let layers_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // Give the layout rebuild a visible notice first.
        self.layout_settling = true;
        if perf {
            crate::perf_record!(
                "[perf] layout-switch from={} to={} total={:.2}ms sync={:.2}ms switch={:.2}ms restore={:.2}ms layers={:.2}ms epoch={}",
                perf_from,
                name,
                perf_total.elapsed().as_secs_f64() * 1000.0,
                sync_ms,
                switch_ms,
                restore_ms,
                layers_ms,
                self.tabs[i].scene.geometry_epoch,
            );
        }
        if context_changed {
            self.sync_dyn_fields();
        }
        cancel_task
    }

    pub(super) fn on_layout_create(&mut self) -> Task<Message> {
        self.create_layout_named(None)
    }

    /// The next free `LayoutN` name, counting from the number of layouts.
    pub(in crate::app) fn next_layout_name(existing: &[String]) -> String {
        (existing.len()..)
            .map(|n| format!("Layout{n}"))
            .find(|candidate| !existing.contains(candidate))
            .unwrap_or_default()
    }

    /// A new layout called `name`, or the next free `LayoutN`.
    pub(in crate::app) fn create_layout_named(&mut self, name: Option<String>) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            self.command_line
                .push_info(crate::t!("Open or create a drawing to add a layout.").as_ref());
            return Task::none();
        }
        // Find a unique name (e.g. Layout2, Layout3, ...).
        let existing = self.tabs[i].scene.layout_names();
        if let Some(name) = name.as_ref().filter(|name| {
            existing
                .iter()
                .any(|layout| layout.eq_ignore_ascii_case(name))
        }) {
            self.command_line
                .push_error(crate::tf!("\"{}\" name already in use", name).as_ref());
            return Task::none();
        }
        let cancel_task = self.cancel_active_command_for_space_change();
        let new_name = name.unwrap_or_else(|| Self::next_layout_name(&existing));
        self.push_undo_snapshot(i, "LAYOUT");
        match self.tabs[i].scene.add_layout(&new_name) {
            Ok(_) => {
                let layout_flags = i16::from(
                    self.tabs[i]
                        .scene
                        .document
                        .header
                        .paper_space_linetype_scaling,
                ) | (i16::from(
                    self.tabs[i].scene.document.header.paper_space_limit_check,
                ) << 1);
                let plot_style = self
                    .active_plot_style
                    .as_ref()
                    .map(|style| style.name.clone())
                    .unwrap_or_default();
                self.tabs[i].scene.new_layout_plot_style = plot_style.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let codec::objects::ObjectType::Layout(l) = obj {
                        if l.name == new_name {
                            l.flags = layout_flags;
                            crate::scene::apply_default_page_setup(l, &plot_style);
                            break;
                        }
                    }
                }
                // Safety net — `add_layout` already creates the overall
                // sheet viewport; this covers any path that doesn't.
                self.tabs[i].scene.ensure_sheet_viewport(&new_name);
                let switch_task = self.on_layout_switch(new_name.clone());
                self.tabs[i].scene.fit_all();
                // Opened, it already has its first viewport unless
                // LAYOUTCREATEVIEWPORT is off.
                if crate::scene::layout_create_viewport() {
                    let name = new_name.clone();
                    self.command_line
                        .push_output(crate::tf!("Layout '{name}' created.").as_ref());
                } else {
                    self.command_line.push_output(
                        crate::tf!("Layout \"{new_name}\" created — use MVIEW to add a viewport")
                            .as_ref(),
                    );
                }
                // The option that greets every new layout with its page setup.
                let switch_task = if self.plot_dialog.page_setup_on_new_layout {
                    Task::batch([switch_task, self.on_plot_dialog_open()])
                } else {
                    switch_task
                };
                self.tabs[i].dirty = true;
                return Task::batch([cancel_task, switch_task]);
            }
            Err(e) => self
                .command_line
                .push_error(crate::tf!("Failed to create layout: {e}").as_ref()),
        }
        cancel_task
    }

    /// Copy layout `source` as "source (2)": its page setup and every
    /// paper-space object, viewports included. The copy opens.
    pub(super) fn on_layout_copy(&mut self, source: String) -> Task<Message> {
        self.copy_layout_named(source, None)
    }

    /// Copy layout `source` as `name`, or as the next free "source (n)".
    pub(in crate::app) fn copy_layout_named(
        &mut self,
        source: String,
        name: Option<String>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start || source == "Model" {
            return Task::none();
        }
        let existing = self.tabs[i].scene.layout_names();
        if let Some(name) = name.as_ref().filter(|name| existing.contains(name)) {
            self.command_line
                .push_error(crate::tf!("\"{}\" name already in use", name).as_ref());
            return Task::none();
        }
        let Some(name) = name.or_else(|| {
            (2..)
                .map(|n| format!("{source} ({n})"))
                .find(|candidate| !existing.contains(candidate))
        }) else {
            return Task::none();
        };
        let cancel_task = self.cancel_active_command_for_space_change();
        self.push_undo_snapshot(i, "LAYOUT");
        let scene = &mut self.tabs[i].scene;
        if let Err(e) = scene.add_layout(&name) {
            self.command_line
                .push_error(crate::tf!("Failed to create layout: {e}").as_ref());
            return cancel_task;
        }
        scene.copy_layout_page(&source, &name);
        let handles = scene.layout_paper_entities(&source);
        let (entities, deps) = crate::app::copy_to_clipboard_kernel(&scene.document, &handles);
        crate::app::paste_entities_kernel(
            &mut scene.document,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Layout(&name),
        );
        scene.populate_meshes_from_document();
        scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.command_line
            .push_output(crate::tf!("Layout \"{source}\" copied as \"{name}\"").as_ref());
        let switch_task = self.on_layout_switch(name);
        Task::batch([cancel_task, switch_task])
    }

    pub(in crate::app) fn on_layout_rename_commit(&mut self) -> Task<Message> {
        if let Some((orig, new_name)) = self.layout_rename_state.take() {
            let new_name = new_name.trim().to_string();
            if !new_name.is_empty() && new_name != orig {
                let i = self.active_tab;
                // BEDIT block tab: renaming it renames the BLOCK itself —
                // its record, marker and every INSERT reference. (#261)
                let is_block_tab = self.tabs[i]
                    .active_block_edit_session()
                    .is_some_and(|session| session.block_name == orig);
                if is_block_tab {
                    if self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .get(&new_name)
                        .is_some()
                    {
                        self.command_line.push_error(
                            crate::tf!("\"{}\" name already in use", new_name).as_ref(),
                        );
                    } else {
                        self.push_undo_snapshot(i, "BLOCK RENAME");
                        if self.tabs[i].scene.rename_block(&orig, &new_name) {
                            if let Some(session) = self.tabs[i].active_block_edit_session_mut() {
                                session.block_name = new_name.clone();
                            }
                            for session in &mut self.tabs[i].block_edits {
                                if session.return_block.as_deref() == Some(orig.as_str()) {
                                    session.return_block = Some(new_name.clone());
                                }
                            }
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(
                                crate::tf!("Block \"{orig}\" → \"{new_name}\"").as_ref(),
                            );
                        } else {
                            self.command_line.push_error(
                                crate::tf!("Could not rename block \"{orig}\"").as_ref(),
                            );
                        }
                    }
                    return Task::none();
                }
                let exists = self.tabs[i]
                    .scene
                    .layout_names()
                    .iter()
                    .any(|n| *n == new_name);
                if exists {
                    self.command_line
                        .push_error(crate::tf!("\"{}\" name already in use", new_name).as_ref());
                } else {
                    self.push_undo_snapshot(i, "LAYOUT RENAME");
                    self.tabs[i].scene.rename_layout(&orig, &new_name);
                    if self.tabs[i].scene.current_layout == orig {
                        self.tabs[i].scene.set_current_layout(new_name.clone());
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("Layout \"{orig}\" → \"{new_name}\"").as_ref());
                }
            }
        }
        Task::none()
    }

    /// Commit the active grip edit at its current (already applied) position:
    /// one undoable group per gesture, every edited entity back into the
    /// resident tessellation. With the grip-menu Copy toggle on, the
    /// originals are put back and the moved shapes are added as new
    /// entities, and the grip stays hot for the next copy.
    pub(in crate::app) fn commit_active_grip_edit(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(grip) = self.tabs[i].active_grip.clone() else {
            return Task::none();
        };
        if grip.last_world == grip.origin_world {
            // Nothing placed yet — the grip simply stays hot.
            return Task::none();
        }
        if self.tabs[i].grip_copy {
            return self.commit_active_grip_edit_as_copy(grip);
        }
        self.tabs[i].grip_base_pending = false;
        if matches!(
            grip.mode,
            GripEditMode::Lengthen
                | GripEditMode::Radius
                | GripEditMode::ArcLength
                | GripEditMode::RectangleWidth
                | GripEditMode::RectangleHeight
                | GripEditMode::MoveParallel
        ) {
            self.grip_pending = None;
            self.command_line.input.clear();
        }
        let added_vertex_focus = grip.targets.iter().find_map(|target| {
            let original = self
                .grip_originals
                .iter()
                .find(|(handle, _)| *handle == target.handle)
                .map(|(_, entity)| entity)?;
            let current = self.tabs[i].scene.document.get_entity(target.handle)?;
            is_added_polyline_vertex(original, current, target.grip_id).then_some(target.grip_id)
        });
        // Keep originals available until every history shape has a valid
        // final display. A rejected rebuild cancels the entire gesture.
        let history_handles: Vec<_> = self
            .grip_preview_handles
            .iter()
            .copied()
            .filter(|handle| {
                self.tabs[i]
                    .scene
                    .document
                    .solid_history_operation(*handle)
                    .is_some()
            })
            .collect();
        for handle in history_handles {
            if !self.tabs[i].scene.finalize_solid_history(handle) {
                self.cancel_active_grip_edit();
                self.command_line.push_error(crate::t!("The edited shape could not be displayed; the original geometry was restored.").as_ref());
                self.refresh_properties();
                return Task::none();
            }
        }
        self.tabs[i].active_grip = None;
        // Commit the grip drag as one undoable group, then put every
        // edited entity back into the resident tessellation.
        let handles = std::mem::take(&mut self.grip_preview_handles);
        let originals = std::mem::take(&mut self.grip_originals);
        let history_originals = std::mem::take(&mut self.grip_history_originals);
        let dirty_before = self.grip_dirty_before.take().unwrap_or(self.tabs[i].dirty);
        if !handles.is_empty() {
            self.grip_reference_wires.clear();
            self.grip_text_verts = Vec::new();
            self.grip_text_slide = false;
            for &handle in &handles {
                self.tabs[i].scene.preview_hidden.remove(&handle);
            }
            self.tabs[i].scene.clear_preview_wire();
            let changes: Vec<_> = handles
                .into_iter()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            // Dimensions that follow the edited shapes update here; record
            // them so the one undo also brings back their saved pictures.
            self.tabs[i].scene.begin_undo_recording();
            self.tabs[i]
                .scene
                .bump_entities_after_parametric_solve(&changes);
            let (followers, follower_objects, ..) = self.tabs[i]
                .scene
                .take_undo_recording()
                .map(|recording| recording.into_recorded_images())
                .unwrap_or_default();
            // A grip that detaches a centre object says so, as moving it does.
            for (handle, original) in &originals {
                let codec::EntityType::Line(line) = original else {
                    continue;
                };
                let data = &line.common.extended_data;
                let was_associated = codec::entities::CenterMarkAssociation::read(data)
                    .map(|association| association.associated)
                    .or_else(|| {
                        codec::entities::CenterLineAssociation::read(data)
                            .map(|association| association.associated)
                    });
                if was_associated == Some(true) {
                    if let Some((mark, false)) = self.tabs[i].scene.center_object_state(*handle) {
                        self.command_line.push_output(&if mark {
                            crate::tr!("center-cmd", "mark-disassociated")
                        } else {
                            crate::tr!("center-cmd", "line-disassociated")
                        });
                    }
                }
            }
            if !originals.is_empty() {
                let mut before: Vec<_> = originals
                    .into_iter()
                    .map(|(handle, entity)| (handle, std::sync::Arc::new(entity)))
                    .collect();
                for (handle, entity) in followers {
                    if let Some(entity) = entity {
                        if !before.iter().any(|(known, _)| *known == handle) {
                            before.push((handle, entity));
                        }
                    }
                }
                let mut object_before: Vec<_> = history_originals
                    .into_iter()
                    .flat_map(|(_, objects)| objects)
                    .collect();
                for (handle, object) in follower_objects {
                    if let Some(object) = object {
                        if !object_before.iter().any(|(known, _)| *known == handle) {
                            object_before.push((handle, object));
                        }
                    }
                }
                self.push_entity_group_history(i, "GRIP", before, object_before, dirty_before);
                self.tabs[i].dirty = true;
            }
        }
        // Placement confirmed — keep the just-added leader.
        self.grip_add_provisional = None;
        self.tabs[i].snap_result = None;
        if let Some(vertex_id) = added_vertex_focus {
            self.tabs[i].properties.prop_vertex = vertex_id;
            self.tabs[i].properties.prop_vertex_indicator_active = true;
            crate::scene::view::dispatch::set_prop_current_vertex(vertex_id);
        }
        self.refresh_properties();
        Task::none()
    }

    /// Grip-menu Copy: leave the originals where they were and add the
    /// edited shapes as new entities, then re-arm the same grip at its
    /// origin so the next placement makes another copy (grip Copy, as in commercial solutions).
    fn commit_active_grip_edit_as_copy(&mut self, grip: GripEdit) -> Task<Message> {
        let i = self.active_tab;
        let handles = std::mem::take(&mut self.grip_preview_handles);
        let originals = std::mem::take(&mut self.grip_originals);
        let _ = std::mem::take(&mut self.grip_history_originals);
        self.grip_dirty_before = None;
        self.grip_add_provisional = None;
        self.grip_reference_wires.clear();
        self.grip_text_verts = Vec::new();
        self.grip_text_slide = false;
        for &handle in &handles {
            self.tabs[i].scene.preview_hidden.remove(&handle);
        }
        self.tabs[i].scene.clear_preview_wire();
        if !originals.is_empty() {
            self.push_undo_snapshot(i, "GRIP");
            let mut restored = Vec::with_capacity(originals.len());
            for (handle, original) in &originals {
                let Some(moved) = self.tabs[i].scene.document.get_entity(*handle).cloned() else {
                    continue;
                };
                if self.tabs[i].scene.update_entity(original.clone()) {
                    restored.push((*handle, crate::scene::ChangeKind::Modified));
                }
                self.tabs[i].scene.add_entity_clone(moved);
            }
            self.tabs[i]
                .scene
                .bump_entities_after_parametric_solve(&restored);
            self.finish_pending_history(i);
            self.tabs[i].dirty = true;
        }
        self.tabs[i].snap_result = None;
        // Re-arm: the grip sits at its origin again over the restored shape.
        let mut again = grip;
        let delta = again.last_world - again.origin_world;
        again.last_world = again.origin_world;
        for target in &mut again.targets {
            target.last_world -= delta;
        }
        self.tabs[i].active_grip = Some(again);
        self.refresh_properties();
        Task::none()
    }
}

#[cfg(test)]
mod hatch_double_click_tests {
    use crate::app::{Message, OpenCADStudio};
    use codec::entities::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge};
    use codec::types::Vector2;

    /// A solid hatch over (0,0)–(10,10), and the window point at its centre.
    fn app_with_hatch() -> (OpenCADStudio, codec::Handle, iced::Point, iced::Point) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        let mut path = BoundaryPath::new();
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .into_iter()
                .map(|p| Vector2::new(p[0], p[1]))
                .collect(),
            true,
        )));
        hatch.paths.push(path);
        let handle = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Hatch(Box::new(hatch)));

        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-5.0, -5.0, 0.0),
            glam::DVec3::new(15.0, 15.0, 0.0),
            800.0 / 600.0,
        );
        let tile = app.tabs[i].scene.active_model_tile_bounds(800.0, 600.0);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: tile.width,
            height: tile.height,
        };
        let inside = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(glam::DVec3::new(5.0, 5.0, 0.0), bounds)
            .unwrap();
        let cursor_local = iced::Point::new(inside.x, inside.y);
        let cursor = iced::Point::new(tile.x + cursor_local.x, tile.y + cursor_local.y);
        (app, handle, cursor, cursor_local)
    }

    fn click(app: &mut OpenCADStudio, cursor: iced::Point) {
        let _ = app.update(Message::ViewportMove(cursor));
        let _ = app.update(Message::ViewportLeftPressAt(cursor));
        let _ = app.update(Message::ViewportLeftRelease);
    }

    /// Double-clicking inside a hatch's fill selects it and starts HATCHEDIT
    /// on it. A hatch has no boundary wire, so the double-click must pick it
    /// by its fill, as a single click does; before, nothing happened.
    #[test]
    fn double_clicking_a_hatch_starts_hatchedit_on_it() {
        let (mut app, handle, cursor, cursor_local) = app_with_hatch();
        let i = app.active_tab;
        // The first click of the pair, 100 ms ago at the same spot.
        app.last_vp_click_time =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(100));
        app.last_vp_click_pos = Some(cursor_local);
        click(&mut app, cursor);

        let command = app.tabs[i].active_cmd.as_ref().map(|cmd| cmd.name());
        assert_eq!(command, Some("HATCHEDIT"));
        assert_eq!(app.tabs[i].selected_handle, Some(handle));
        // Selected first, HATCHEDIT skipped its pick and edits this hatch.
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .map(|cmd| cmd.needs_entity_pick()),
            Some(false)
        );
    }

    /// With the hatch selected, the right-click menu offers Edit Hatch.
    #[test]
    fn a_selected_hatch_gets_edit_hatch_in_the_context_menu() {
        use crate::ui::popup::context_menu::MenuContext;
        let (mut app, handle, _, _) = app_with_hatch();
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(handle, true);
        assert!(matches!(
            app.context_menu_context(),
            MenuContext::Idle {
                single_hatch: true,
                single_mleader: false,
                ..
            }
        ));
    }

    /// HATCHEDIT started with nothing selected picks a hatch by clicking
    /// inside its fill; before, its pick only hit wires and no hatch could
    /// be chosen.
    #[test]
    fn hatchedit_picks_a_hatch_by_its_fill() {
        let (mut app, _handle, cursor, _) = app_with_hatch();
        let i = app.active_tab;
        let _ = app.dispatch_command("HATCHEDIT");
        let picking = |app: &OpenCADStudio| {
            app.tabs[i]
                .active_cmd
                .as_ref()
                .map(|cmd| (cmd.name(), cmd.needs_entity_pick()))
        };
        assert_eq!(picking(&app), Some(("HATCHEDIT", true)));

        click(&mut app, cursor);
        assert_eq!(picking(&app), Some(("HATCHEDIT", false)), "hatch picked");
    }
}

#[cfg(test)]
mod scroll_intent_tests {
    use super::{pinch_zoom_steps, scroll_intent, ScrollIntent};
    use crate::scene::view::camera::Camera;
    use iced::mouse::ScrollDelta;

    /// With the touchpad option, a scroll pans (wheel lines as pixels) and
    /// Ctrl+scroll — a pinch — still zooms. On macOS lines come from a
    /// notched wheel only, which zooms.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn touchpad_pan_scrolls_pan_and_ctrl_zooms() {
        assert_eq!(
            scroll_intent(ScrollDelta::Lines { x: 0.5, y: -0.25 }, true, false),
            ScrollIntent::Pan {
                dx: 20.0,
                dy: -10.0
            }
        );
        assert_eq!(
            scroll_intent(ScrollDelta::Lines { x: 0.0, y: 1.0 }, true, true),
            ScrollIntent::Zoom { notches: 1.0 }
        );
    }

    #[test]
    fn wheel_notches_zoom() {
        assert_eq!(
            scroll_intent(ScrollDelta::Lines { x: 0.0, y: 1.0 }, false, false),
            ScrollIntent::Zoom { notches: 1.0 }
        );
    }

    #[test]
    fn smooth_mouse_scroll_zooms_without_touchpad_mode() {
        for (y, notches) in [(100.0, 1.0), (-100.0, -1.0), (0.0, 0.0)] {
            assert_eq!(
                scroll_intent(ScrollDelta::Pixels { x: 4.0, y }, false, false),
                ScrollIntent::Zoom { notches }
            );
        }
    }

    /// Preserve both axes when touchpad panning is explicitly enabled.
    #[test]
    fn two_finger_scroll_pans_in_touchpad_mode() {
        assert_eq!(
            scroll_intent(ScrollDelta::Pixels { x: 4.0, y: 6.0 }, true, false),
            ScrollIntent::Pan { dx: 4.0, dy: 6.0 }
        );
    }

    #[test]
    fn ctrl_pixel_scroll_zooms_in_touchpad_mode() {
        assert_eq!(
            scroll_intent(ScrollDelta::Pixels { x: 4.0, y: 100.0 }, true, true),
            ScrollIntent::Zoom { notches: 1.0 }
        );
    }

    #[test]
    fn smooth_mouse_scroll_changes_camera_zoom_and_honors_reverse() {
        use crate::app::{Message, OpenCADStudio};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.touchpad_pan = false;
        app.ctrl_down = false;
        app.zoom_wheel_reversed = false;
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let before = app.tabs[i].scene.camera.borrow().distance;
        let scroll = ScrollDelta::Pixels { x: 0.0, y: 100.0 };

        let _ = app.update(Message::ViewportScroll(scroll));
        let zoomed = app.tabs[i].scene.camera.borrow().distance;
        assert!(zoomed < before, "scroll must zoom instead of panning");

        app.zoom_wheel_reversed = true;
        let _ = app.update(Message::ViewportScroll(scroll));
        let reversed = app.tabs[i].scene.camera.borrow().distance;
        assert!(reversed > zoomed, "ZOOMWHEEL must reverse smooth scrolling");
        assert!((reversed - before).abs() < before * 1e-5);
    }

    /// A pinch is a scale factor, so the view has to end up exactly that much
    /// closer: fingers 5% apart divide the camera distance by 1.05.
    ///
    /// Asked of the camera, not of the formula the step was derived from. The
    /// version that recomputed `1 - step / 10` here kept passing when the
    /// camera moved to an exponential zoom and the pinch quietly stopped
    /// being 1:1.
    #[test]
    fn pinch_is_one_to_one() {
        for magnification in [0.01_f32, 0.05, 0.2, 0.5, 1.0] {
            let mut camera = Camera::default();
            let before = camera.distance;
            camera.zoom(pinch_zoom_steps(magnification));
            let want = before / (1.0 + magnification);
            assert!(
                (camera.distance - want).abs() < 1e-4 * want,
                "m = {magnification}: {} wanted {want}",
                camera.distance
            );
        }
        assert!(pinch_zoom_steps(-0.05) < 0.0, "pinching in zooms out");
        // No pinch, no step: a zero delta must not divide the view away.
        assert_eq!(pinch_zoom_steps(0.0), 0.0);
        let mut camera = Camera::default();
        let before = camera.distance;
        camera.zoom(pinch_zoom_steps(0.0));
        assert_eq!(camera.distance, before);
    }

    /// A gesture the platform should never send must not be able to throw the
    /// camera out of the world.
    #[test]
    fn a_degenerate_pinch_stays_finite() {
        for magnification in [-1.0_f32, -2.0, f32::NAN, f32::INFINITY] {
            let mut camera = Camera::default();
            camera.zoom(pinch_zoom_steps(magnification));
            assert!(
                camera.distance.is_finite() && camera.distance > 0.0,
                "m = {magnification} left distance at {}",
                camera.distance
            );
        }
    }
}

#[cfg(test)]
mod rapid_drawing_tests {
    use super::*;

    #[test]
    fn discrete_snap_preview_is_rate_limited_to_60_hz() {
        let last = Instant::now();
        let dwell = crate::app::SnapDwell {
            last_move_at: last,
            last_preview_snap_at: last,
            point: Point::new(0.0, 0.0),
            tab: 0,
            preview_dirty: false,
            snap_resolved: false,
        };
        assert!(!dwell.discrete_preview_due_at(last + std::time::Duration::from_millis(15)));
        assert!(dwell.discrete_preview_due_at(last + std::time::Duration::from_millis(16)));
    }

    fn drawing_app(command: &str) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.tabs[app.active_tab]
            .scene
            .selection
            .borrow_mut()
            .view
            .vp_size = (800.0, 600.0);
        app.snapper.snap_enabled = false;
        app.snapper.snap3d_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.dynamic_ucs = false;
        let _ = app.run_command_line(command);
        assert!(app.tabs[app.active_tab].active_cmd.is_some(), "{command}");
        app
    }

    fn screen(app: &OpenCADStudio, point: glam::DVec3) -> Point {
        let p = app.tabs[app.active_tab]
            .scene
            .camera
            .borrow()
            .project(
                point,
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .unwrap();
        Point::new(p.x, p.y)
    }

    #[test]
    fn point_modifier_mouse_moves_keep_the_parent_live_on_full_and_fast_paths() {
        use crate::command::StepInput;
        for from in [false, true] {
            let mut app = drawing_app("LINE");
            let i = app.active_tab;
            let _ = app.feed_command(StepInput::Point(glam::DVec3::ZERO));
            if from {
                app.start_from_modifier(i);
            } else {
                app.start_mtp_modifier(i);
            }
            for second_step in [false, true] {
                if second_step {
                    let _ = app.feed_command(StepInput::Point(glam::DVec3::new(3.0, 7.0, 0.0)));
                }
                // No pending dwell takes the full path; the next move takes the fast path.
                app.snap_dwell = None;
                for target in [
                    glam::DVec3::new(10.0, 20.0, 0.0),
                    glam::DVec3::new(25.0, 30.0, 0.0),
                ] {
                    let at = screen(&app, target);
                    let _ = app.update(Message::ViewportMove(at));
                    let cursor = app.tabs[i].last_cursor_world;
                    let expected = app.tabs[i]
                        .suspended_cmd
                        .as_mut()
                        .unwrap()
                        .on_preview_wires(cursor);
                    let actual = &app.tabs[i].scene.preview_wires;
                    assert!(!expected.is_empty());
                    assert_eq!(actual.first().unwrap().points, expected[0].points);
                    assert_eq!(
                        actual.len(),
                        1 + if second_step {
                            if from {
                                1
                            } else {
                                2
                            }
                        } else {
                            0
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn otrack_rapid_moves_defer_and_stationary_ticks_acquire_once() {
        use codec::{entities::Line, types::Vector3, EntityType};
        use crate::snap::SnapType;
        let mut app = drawing_app("LINE");
        let i = app.active_tab;
        app.tabs[i].scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(40.0, 30.0, 0.0),
            Vector3::new(80.0, 30.0, 0.0),
        )));
        app.snapper.snap_enabled = true;
        app.snapper.enabled.clear();
        app.snapper.enabled.insert(SnapType::Endpoint);
        app.snapper.otrack_enabled = true;
        let _ = app.update(Message::CommandEscape);
        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");
        let at = screen(&app, glam::DVec3::new(40.0, 30.0, 0.0));
        assert!((0.0..800.0).contains(&at.x) && (0.0..600.0).contains(&at.y));
        let _ = app.update(Message::ViewportMove(at));
        let _ = app.update(Message::ViewportMove(at));
        assert!(!app.snap_dwell.as_ref().unwrap().snap_resolved,
            "OTRACK must not bypass the movement throttle");
        assert!(app.snapper.tracking_points.is_empty());

        // Model frame ticks and elapsed dwell without sleeping.
        for _ in 0..6 {
            app.snap_dwell.as_mut().unwrap().last_move_at =
                Instant::now() - std::time::Duration::from_millis(100);
            if app.snapper.dwell_since.is_some() {
                app.snapper.dwell_since =
                    Some(Instant::now() - std::time::Duration::from_millis(300));
            }
            let _ = app.update(Message::SnapDwellTick);
            assert!(app.tabs[i].snap_result.is_some(), "settled endpoint must snap");
        }
        assert_eq!(app.snapper.tracking_points.len(), 1);
        assert!((app.snapper.tracking_points[0] - glam::DVec3::new(40.0, 30.0, 0.0)).length() < 0.001);
        assert!(app.snap_dwell.as_ref().unwrap().snap_resolved,
            "acquired dwell must stop scheduling full snap passes");
        let _ = app.update(Message::SnapDwellTick);
        assert_eq!(app.snapper.tracking_points.len(), 1, "must not toggle twice");
        let _ = app.update(Message::ViewportLeftPressAt(at));
        assert!((app.last_point.unwrap() - glam::DVec3::new(40.0, 30.0, 0.0)).length() < 0.001);
    }

    #[test]
    fn dynamic_ucs_point_command_uses_the_exact_button_position() {
        let mut app = drawing_app("LINE");
        app.dynamic_ucs = true;
        let target = glam::DVec3::new(40.0, 30.0, 0.0);
        let at = screen(&app, target);

        let _ = app.update(Message::ViewportLeftPressAt(at));

        assert!((app.last_point.unwrap() - target).length() < 0.001);
        assert!(app.dynamic_ucs);
        assert!(app.tabs[app.active_tab].active_cmd.is_some());
    }

    #[test]
    fn ortho_in_a_3d_view_runs_up_the_z_axis_and_keeps_that_level() {
        let mut app = drawing_app("LINE");
        let _ = app.apply_view_preset("SWISO");
        app.ortho_mode = true;
        let click = |app: &mut OpenCADStudio, aim: glam::DVec3| {
            let at = screen(app, aim);
            let _ = app.update(Message::ViewportMove(at));
            let _ = app.update(Message::ViewportLeftPressAt(at));
            let _ = app.update(Message::ViewportLeftRelease);
            app.last_point.unwrap()
        };
        let start = click(&mut app, glam::DVec3::ZERO);
        // Straight up: exactly on the axis, as high as the cursor shows.
        let up = click(&mut app, glam::DVec3::new(0.5, 0.0, 50.0));
        assert!((up - start).truncate().length() < 1e-9, "{start:?} -> {up:?}");
        assert!((up.z - 50.0).abs() < 0.5, "{up:?}");
        // Then along X at that level, not back down to the ground plane.
        let across = click(&mut app, glam::DVec3::new(30.0, 0.5, up.z));
        assert!((across.y - up.y).abs() < 1e-9, "{across:?}");
        assert!((across.z - up.z).abs() < 1e-6, "{across:?}");
        assert!((across.x - 30.0).abs() < 0.5, "{across:?}");
    }

    #[test]
    fn rapid_drawing_zigzag_uses_button_positions_without_hover_or_duplicate_release() {
        for command in ["LINE", "PLINE"] {
            for pane in [false, true] {
                let mut app = drawing_app(command);
                for (x, y) in [(40.0, 30.0), (80.0, 60.0), (120.0, 30.0), (160.0, 60.0)] {
                    let target = glam::DVec3::new(x, y, 0.0);
                    let old = screen(&app, target + glam::DVec3::new(-15.0, 10.0, 0.0));
                    let _ = app.update(Message::ViewportMove(old));
                    let at = screen(&app, target);
                    let _ = app.update(if pane {
                        Message::PanePressAt(0, at)
                    } else {
                        Message::ViewportLeftPressAt(at)
                    });
                    assert!(
                        (app.last_point.unwrap() - target).length() < 0.001,
                        "{command} pane={pane}: {:?} != {target:?}",
                        app.last_point
                    );
                    assert!(
                        app.snap_dwell
                            .as_ref()
                            .is_some_and(|dwell| dwell.snap_resolved),
                        "{command} pane={pane}: accepted click should leave the snap gate resolved"
                    );
                    let after_press = app.last_point;
                    let _ = app.update(Message::ViewportMove(old));
                    let _ = app.update(Message::ViewportLeftRelease);
                    assert_eq!(app.last_point, after_press);
                }
                let _ = app.feed_command(crate::command::StepInput::Enter);
                let entities: Vec<_> = app.tabs[app.active_tab].scene.document.entities().collect();
                assert_eq!(entities.len(), if command == "LINE" { 3 } else { 1 });
            }
        }
    }

    #[test]
    fn rapid_drawing_preview_coalesces_moves_and_flushes_latest_position() {
        for command in ["LINE", "PLINE", "RECTANG", "CIRCLE"] {
            let mut app = drawing_app(command);
            let at = screen(&app, glam::DVec3::new(40.0, 30.0, 0.0));
            let _ = app.update(Message::ViewportLeftPressAt(at));
            let at = screen(&app, glam::DVec3::new(50.0, 40.0, 0.0));
            let _ = app.update(Message::ViewportMove(at));
            let i = app.active_tab;
            app.snap_dwell.as_mut().unwrap().last_move_at = Instant::now();
            let _ = app.on_snap_dwell_tick();
            assert!(!app.tabs[i].scene.preview_wires.is_empty(), "{command}");
            let before = app.tabs[i].scene.preview_wires[0].points.clone();
            for x in 60..90 {
                let at = screen(&app, glam::DVec3::new(f64::from(x), 55.0, 0.0));
                let _ = app.update(Message::ViewportMove(at));
            }
            assert_eq!(
                app.tabs[i].scene.preview_wires[0].points, before,
                "{command}"
            );
            assert!(app.snap_dwell.as_ref().unwrap().preview_dirty);
            let latest = app.tabs[i].last_cursor_world;
            app.snap_dwell.as_mut().unwrap().last_move_at = Instant::now();
            let _ = app.on_snap_dwell_tick();
            assert!(!app.snap_dwell.as_ref().unwrap().preview_dirty);
            assert_ne!(
                app.tabs[i].scene.preview_wires[0].points, before,
                "{command}"
            );
            assert_eq!(app.tabs[i].last_cursor_world, latest);
            // Expire without sleeping: the full pass must finish in this
            // update, not emit an old ViewportMove for the daemon to replay.
            app.snap_dwell.as_mut().unwrap().last_move_at =
                Instant::now() - std::time::Duration::from_millis(100);
            let _ = app.on_snap_dwell_tick();
            assert!(app.snap_dwell.as_ref().unwrap().snap_resolved);
            assert!((app.tabs[i].last_cursor_world - latest).length() < 0.001);
            let resolved_point = app.tabs[i].last_cursor_world;
            let _ = app.on_snap_dwell_tick();
            assert!(app.snap_dwell.as_ref().unwrap().snap_resolved);
            assert_eq!(app.tabs[i].last_cursor_world, resolved_point);
        }
    }

    #[test]
    fn stretch_cursor_move_keeps_live_lines_visible_even_during_fast_pointer_updates() {
        use crate::command::{CadCommand, CmdResult};
        use crate::modules::draw::modify::stretch::StretchCommand;
        use codec::{entities::Line, types::Vector3, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        app.snapper.snap_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;

        let handle = app.tabs[i].scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(40.0, 30.0, 0.0),
            Vector3::new(80.0, 30.0, 0.0),
        )));
        let wires = app.tabs[i].scene.wire_models_for(&[handle]);
        let window = (
            glam::DVec3::new(39.0, 29.0, -1.0),
            glam::DVec3::new(81.0, 31.0, 1.0),
        );
        let base = glam::DVec3::new(40.0, 30.0, 0.0);
        let mut cmd = StretchCommand::preselected(vec![handle], wires, window);
        assert!(matches!(cmd.on_point(base), CmdResult::NeedPoint));
        app.tabs[i].active_cmd = Some(Box::new(cmd));

        let preview_has_endpoint = |app: &OpenCADStudio, x: f32, y: f32| {
            app.tabs[i].scene.preview_wires.iter().any(|wire| {
                wire.points.iter().any(|pt| {
                    (pt[0] - x).abs() < 0.1 && (pt[1] - y).abs() < 0.1
                })
            })
        };
        let first = glam::DVec3::new(45.0, 40.0, 0.0);
        let _ = app.on_viewport_move(screen(&app, first));
        assert!(
            preview_has_endpoint(&app, 85.0, 40.0),
            "first cursor move must show the translated STRETCH line, not just its guide"
        );

        // A subsequent move normally takes the low-cost SnapDwell path.
        // Make the 60 Hz overlay budget due without sleeping.
        let dwell = app.snap_dwell.as_mut().expect("STRETCH dwell");
        dwell.last_preview_snap_at =
            Instant::now() - std::time::Duration::from_millis(100);
        let second = glam::DVec3::new(48.0, 42.0, 0.0);
        let _ = app.on_viewport_move(screen(&app, second));
        assert!(
            preview_has_endpoint(&app, 88.0, 42.0),
            "fast cursor move must update the STRETCH geometry, not hide it"
        );
    }

    /// Shift held through several PLINE points: each segment takes the axis
    /// it is drawn along, without releasing Shift in between (#1422).
    #[test]
    fn shift_axis_lock_follows_each_new_segment() {
        let mut app = drawing_app("PLINE");
        let start = screen(&app, glam::DVec3::ZERO);
        let _ = app.update(Message::ViewportLeftPressAt(start));
        let _ = app.update(Message::ViewportLeftRelease);
        app.shift_down = true;
        app.snap_dwell = None;
        let at = screen(&app, glam::DVec3::new(40.0, 3.0, 0.0));
        let _ = app.update(Message::ViewportMove(at));
        let first = app.axis_lock_dir.expect("locked to the first axis");
        assert!(first.x.abs() > 0.99, "{first:?}");
        let _ = app.update(Message::ViewportLeftPressAt(at));
        let _ = app.update(Message::ViewportLeftRelease);
        // Shift is still down: the next segment goes up.
        // The first move after the click drifts on along the old axis.
        for to in [glam::DVec3::new(40.4, 0.05, 0.0), glam::DVec3::new(43.0, 40.0, 0.0)] {
            app.snap_dwell = None;
            let at = screen(&app, to);
            let _ = app.update(Message::ViewportMove(at));
        }
        let second = app.axis_lock_dir.expect("locked to the second axis");
        assert!(second.y.abs() > 0.99, "{second:?}");
    }

    #[test]
    fn rapid_drafting_polar_matches_authoritative_point() {
        for (target, engaged) in [
            (glam::DVec3::new(40.0, 0.25, 0.0), true),
            (glam::DVec3::new(40.0, 20.0, 0.0), false),
        ] {
            let exercise = |force_full: bool| {
                let mut app = drawing_app("LINE");
                let i = app.active_tab;
                let start = screen(&app, glam::DVec3::ZERO);
                let _ = app.update(Message::ViewportLeftPressAt(start));
                app.polar_mode = true;
                app.polar_increment_deg = 90.0;
                let first = screen(&app, glam::DVec3::new(20.0, 0.0, 0.0));
                let _ = app.update(Message::ViewportMove(first));
                assert!(app.snap_dwell.is_some());
                if force_full { app.snap_dwell = None; }
                let at = screen(&app, target);
                let _ = app.update(Message::ViewportMove(at));
                assert_eq!(app.last_point, Some(glam::DVec3::ZERO));
                assert!(app.tabs[i].active_cmd.is_some());
                if !force_full {
                    assert!(!app.snap_dwell.as_ref().unwrap().snap_resolved);
                }
                app.tabs[i].last_cursor_world
            };
            let full = exercise(true);
            let rapid = exercise(false);
            assert!(rapid.distance(full) < 0.001, "engaged={engaged}: {rapid:?} != {full:?}");
            if engaged {
                assert!(rapid.y.abs() < 0.001);
            } else {
                assert!(rapid.distance(target) < 0.001);
            }
        }
    }

    #[test]
    fn rapid_drafting_3d_matches_authoritative_z_axis() {
        for polar in [false, true] {
            let exercise = |force_full: bool| {
                let mut app = drawing_app("LINE");
                let i = app.active_tab;
                let _ = app.apply_view_preset("SWISO");
                let start = screen(&app, glam::DVec3::ZERO);
                let _ = app.update(Message::ViewportLeftPressAt(start));
                let anchor = app.last_point.expect("initial LINE point");
                app.ortho_mode = !polar;
                app.polar_mode = polar;
                app.polar_increment_deg = 90.0;
                let first = screen(&app, glam::DVec3::new(0.0, 0.0, 10.0));
                let _ = app.update(Message::ViewportMove(first));
                assert!(app.snap_dwell.is_some());
                if force_full { app.snap_dwell = None; }
                let at = screen(&app, glam::DVec3::new(0.0, 0.0, 20.0));
                let _ = app.update(Message::ViewportMove(at));
                assert_eq!(app.last_point, Some(anchor));
                (app.tabs[i].last_cursor_world, app.otrack_kind)
            };
            let full = exercise(true);
            let rapid = exercise(false);
            assert!(rapid.0.distance(full.0) < 0.001, "polar={polar}: {rapid:?} != {full:?}");
            assert_eq!(rapid.1, full.1);
            assert!(rapid.0.distance(glam::DVec3::new(0.0, 0.0, 20.0)) < 0.001);
            assert_eq!(rapid.1, Some(crate::snap::TrackingKind::PlusZ));
        }
    }

    #[test]
    fn throttle_axis_lock_matches_authoritative_move() {
        for explicit_angle in [false, true] {
            let exercise = |force_full: bool| {
                let mut app = drawing_app("LINE");
                // Shift is the axis lock with ortho off; with it on, Shift frees.
                app.ortho_mode = explicit_angle;
                let i = app.active_tab;
                let start = screen(&app, glam::DVec3::ZERO);
                let _ = app.update(Message::ViewportLeftPressAt(start));
                app.shift_down = !explicit_angle;
                if explicit_angle {
                    app.angle_override = Some(glam::DVec3::X);
                }
                app.snap_dwell = None;
                let at = screen(&app, glam::DVec3::new(40.0, 10.0, 0.0));
                let _ = app.update(Message::ViewportMove(at));
                assert!(app.axis_lock_dir.is_some());
                if force_full {
                    app.snap_dwell = None;
                }
                let at = screen(&app, glam::DVec3::new(10.0, 40.0, 0.0));
                let _ = app.update(Message::ViewportMove(at));
                (app.tabs[i].last_cursor_world, app.axis_lock_dir)
            };
            let full = exercise(true);
            let moving = exercise(false);
            assert!(moving.0.distance(full.0) < 0.001, "explicit angle={explicit_angle}: {moving:?} != {full:?}");
            assert_eq!(moving.1, full.1);
        }
    }

    #[test]
    fn throttle_axis_command_matches_authoritative_move() {
        struct AxisCommand;
        impl crate::command::CadCommand for AxisCommand {
            fn name(&self) -> &'static str { "AXIS_TEST" }
            fn prompt(&self) -> String { "Point on axis:".into() }
            fn on_enter(&mut self) -> crate::command::CmdResult {
                crate::command::CmdResult::NeedPoint
            }
            fn on_point(&mut self, _: glam::DVec3) -> crate::command::CmdResult {
                crate::command::CmdResult::NeedPoint
            }
            fn cursor_axis(&self) -> Option<(glam::DVec3, glam::DVec3)> {
                Some((glam::DVec3::ZERO, glam::DVec3::X))
            }
        }
        let exercise = |force_full: bool| {
            let mut app = drawing_app("LINE");
            app.ortho_mode = true;
            let i = app.active_tab;
            app.tabs[i].active_cmd = Some(Box::new(AxisCommand));
            let at = screen(&app, glam::DVec3::new(40.0, 10.0, 0.0));
            let _ = app.update(Message::ViewportMove(at));
            if force_full { app.snap_dwell = None; }
            let at = screen(&app, glam::DVec3::new(10.0, 40.0, 0.0));
            let _ = app.update(Message::ViewportMove(at));
            app.tabs[i].last_cursor_world
        };
        assert!(exercise(false).distance(exercise(true)) < 0.001);
    }

    #[test]
    fn dimension_and_constrained_moves_coalesce_and_flush_preview() {
        let mut app = drawing_app("DIMLINEAR");
        app.ortho_mode = true;
        let i = app.active_tab;
        let first = screen(&app, glam::DVec3::new(0.0, 0.0, 0.0));
        let _ = app.update(Message::ViewportLeftPressAt(first));
        let second = screen(&app, glam::DVec3::new(40.0, 0.0, 0.0));
        let _ = app.update(Message::ViewportLeftPressAt(second));
        let start = screen(&app, glam::DVec3::new(40.0, 15.0, 0.0));
        let _ = app.update(Message::ViewportMove(start));
        app.snap_dwell.as_mut().unwrap().last_move_at = Instant::now();
        let _ = app.on_snap_dwell_tick();
        assert!(!app.tabs[i].scene.preview_wires.is_empty());
        let bits = |wires: &[crate::scene::WireModel]| -> Vec<[u32; 3]> {
            wires[0]
                .points
                .iter()
                .map(|p| [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()])
                .collect()
        };
        let initial = bits(&app.tabs[i].scene.preview_wires);
        for y in 20..35 {
            let at = screen(&app, glam::DVec3::new(40.0, f64::from(y), 0.0));
            let _ = app.update(Message::ViewportMove(at));
        }
        assert!(
            app.snap_dwell.as_ref().unwrap().preview_dirty,
            "rapid constrained dimension moves must defer full snap/preview rebuild"
        );
        assert!(!app.snap_dwell.as_ref().unwrap().snap_resolved);
        assert_eq!(bits(&app.tabs[i].scene.preview_wires), initial);
        app.snap_dwell.as_mut().unwrap().last_move_at = Instant::now();
        let _ = app.on_snap_dwell_tick();
        assert!(!app.snap_dwell.as_ref().unwrap().preview_dirty);
        assert_ne!(bits(&app.tabs[i].scene.preview_wires), initial);
    }

    #[test]
    fn otrack_departure_resets_acquired_latch_after_removing_final_reference() {
        use codec::{entities::Line, types::Vector3, EntityType};
        use crate::snap::SnapType;
        let mut app = drawing_app("LINE");
        let i = app.active_tab;
        app.tabs[i].scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(40.0, 30.0, 0.0),
            Vector3::new(80.0, 30.0, 0.0),
        )));
        app.snapper.snap_enabled = true;
        app.snapper.enabled.clear();
        app.snapper.enabled.insert(SnapType::Endpoint);
        app.snapper.otrack_enabled = true;
        let _ = app.update(Message::CommandEscape);
        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");

        let at = screen(&app, glam::DVec3::new(40.0, 30.0, 0.0));
        let away = screen(&app, glam::DVec3::new(40.0, 100.0, 0.0));
        let at_jitter = Point::new(at.x + 1.0, at.y + 1.0);

        // 1. Move to A and stationary dwell to acquire A.
        let _ = app.update(Message::ViewportMove(at));
        for _ in 0..6 {
            app.snap_dwell.as_mut().unwrap().last_move_at =
                Instant::now() - std::time::Duration::from_millis(100);
            if app.snapper.dwell_since.is_some() {
                app.snapper.dwell_since =
                    Some(Instant::now() - std::time::Duration::from_millis(300));
            }
            let _ = app.update(Message::SnapDwellTick);
        }
        assert_eq!(app.snapper.tracking_points.len(), 1, "point A must be acquired initially");

        // Small jitter within aperture must not toggle or reset.
        let _ = app.update(Message::ViewportMove(at_jitter));
        let _ = app.update(Message::SnapDwellTick);
        assert_eq!(app.snapper.tracking_points.len(), 1, "jitter within aperture must not toggle");

        // 2. Leave and return.
        let _ = app.update(Message::ViewportMove(away));
        let _ = app.update(Message::ViewportMove(at));

        // 3. Stationary dwell removes final point A.
        for _ in 0..6 {
            app.snap_dwell.as_mut().unwrap().last_move_at =
                Instant::now() - std::time::Duration::from_millis(100);
            if app.snapper.dwell_since.is_some() {
                app.snapper.dwell_since =
                    Some(Instant::now() - std::time::Duration::from_millis(300));
            }
            let _ = app.update(Message::SnapDwellTick);
        }
        assert_eq!(app.snapper.tracking_points.len(), 0, "dwelling on A again removes final reference");

        // 4. Rapid departure and return.
        let _ = app.update(Message::ViewportMove(away));
        let _ = app.update(Message::ViewportMove(at));

        // 5. Stationary dwell must reacquire A.
        for _ in 0..6 {
            if let Some(dwell) = app.snap_dwell.as_mut() {
                dwell.last_move_at =
                    Instant::now() - std::time::Duration::from_millis(100);
            }
            if app.snapper.dwell_since.is_some() {
                app.snapper.dwell_since =
                    Some(Instant::now() - std::time::Duration::from_millis(300));
            }
            let _ = app.update(Message::SnapDwellTick);
        }
        assert_eq!(
            app.snapper.tracking_points.len(),
            1,
            "point A must reacquire after departure and stationary dwell"
        );
    }

    #[test]
    fn parallel_snap_acquires_reference_on_line_interior() {
        use codec::{entities::Line, types::Vector3, EntityType};
        use crate::snap::SnapType;
        let mut app = drawing_app("LINE");
        let i = app.active_tab;
        app.tabs[i].scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(40.0, 30.0, 0.0),
            Vector3::new(80.0, 30.0, 0.0),
        )));
        app.snapper.snap_enabled = true;
        app.snapper.enabled.clear();
        app.snapper.enabled.insert(SnapType::Parallel);
        app.snapper.otrack_enabled = true;
        let _ = app.update(Message::CommandEscape);
        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");

        let interior = screen(&app, glam::DVec3::new(60.0, 30.0, 0.0));
        assert!((0.0..800.0).contains(&interior.x) && (0.0..600.0).contains(&interior.y));

        let _ = app.update(Message::ViewportMove(interior));
        std::thread::sleep(std::time::Duration::from_millis(160));
        let _ = app.update(Message::ViewportMove(interior));

        assert!(
            app.snapper.parallel_ref.is_some(),
            "Parallel snap must acquire reference on line interior when Parallel is enabled"
        );
        let (dir, pt) = app.snapper.parallel_ref.unwrap();
        assert!((dir.x.abs() - 1.0).abs() < 0.001);
        assert!((pt.y - 30.0).abs() < 0.001);
    }
}

#[cfg(test)]
mod zoom_dynamic_cursor_tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use crate::ui::overlay::NavCursor;

    fn zoom_armed_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.tabs[0].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let _ = app.dispatch_command("ZOOM DYNAMIC");
        app
    }

    fn drag_to(app: &mut OpenCADStudio, y: f32) {
        let _ = app.update(Message::ViewportMove(Point::new(400.0, y)));
    }

    fn zoom_dir_out(app: &OpenCADStudio) -> bool {
        app.tabs[0].scene.selection.borrow().input.zoom_dir_out
    }

    /// The magnifier has to carry the sign of the zoom actually under way:
    /// dragging up zooms in, dragging back down zooms out.
    #[test]
    fn the_magnifier_follows_the_drag_direction() {
        let mut app = zoom_armed_app();
        assert!(app.tabs[0].zoom_dynamic_mode);
        drag_to(&mut app, 300.0);
        let _ = app.update(Message::ViewportLeftPress);
        // A fresh drag starts neutral — "+" until told otherwise.
        assert!(!zoom_dir_out(&app));

        drag_to(&mut app, 200.0);
        assert!(!zoom_dir_out(&app), "dragging up zooms in");

        drag_to(&mut app, 400.0);
        assert!(zoom_dir_out(&app), "dragging down zooms out");

        drag_to(&mut app, 250.0);
        assert!(!zoom_dir_out(&app), "dragging up again zooms in");

        // Releasing disarms the direction, so the next hover shows "+".
        let _ = app.update(Message::ViewportLeftRelease);
        assert!(!zoom_dir_out(&app));
    }

    /// Sub-pixel wobble while a hand holds still must not strobe the glyph.
    #[test]
    fn jitter_below_the_dead_zone_does_not_flip_the_glyph() {
        let mut app = zoom_armed_app();
        drag_to(&mut app, 300.0);
        let _ = app.update(Message::ViewportLeftPress);
        drag_to(&mut app, 400.0);
        assert!(zoom_dir_out(&app));

        // Back up by less than ZOOM_DIR_FLIP_PX: still zooming out.
        drag_to(&mut app, 399.0);
        assert!(zoom_dir_out(&app), "1 px of wobble must not flip the sign");
    }

    /// Each navigation tool owns its own cursor. They used to share the pan
    /// hand, which read as "drag the sheet" during a zoom and an orbit.
    #[test]
    fn each_navigation_tool_reports_its_own_mode() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        for (command, want) in [
            ("PAN", NavCursor::Pan),
            ("ORBIT", NavCursor::Orbit),
            ("ZOOM DYNAMIC", NavCursor::Zoom),
        ] {
            let _ = app.dispatch_command(command);
            assert_eq!(
                crate::app::view::nav_cursor(&app.tabs[0]),
                want,
                "{command} must have its own cursor"
            );
            let _ = app.update(Message::CommandEscape);
            assert_eq!(
                crate::app::view::nav_cursor(&app.tabs[0]),
                NavCursor::None,
                "Esc must disarm {command}"
            );
        }
    }
}

#[cfg(test)]
mod snap_elevation_tests {
    use super::snap_keeps_elevation;
    use crate::snap::SnapType;

    #[test]
    fn only_object_snaps_keep_elevation() {
        assert!(!snap_keeps_elevation(None), "a miss stays on the plane");
        assert!(
            !snap_keeps_elevation(Some(SnapType::Grid)),
            "grid stays planar"
        );
        for t in [
            SnapType::Endpoint,
            SnapType::Midpoint,
            SnapType::Center,
            SnapType::Node,
            SnapType::Quadrant,
            SnapType::Intersection,
            SnapType::Insertion,
            SnapType::Perpendicular,
            SnapType::Tangent,
            SnapType::Nearest,
            SnapType::ApparentIntersection,
            SnapType::Extension,
            SnapType::Parallel,
            SnapType::ObjectPick,
        ] {
            assert!(snap_keeps_elevation(Some(t)), "{t:?} must keep its Z");
        }
    }
}

#[cfg(test)]
mod selection_preview_tests {
    use super::*;
    use crate::app::{HoverDwell, OpenCADStudio, HOVER_DWELL_MS};

    #[test]
    fn lower_endpoint_drag_previews_connected_tangent_profile_and_keeps_top_fixed() {
        use crate::scene::parametric_constraints::{
            ConstraintKind, ParametricRef, ParametricScope,
        };
        use codec::{
            entities::{Arc, Line},
            types::Vector3,
            EntityType,
        };

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.constraint_solve_mode = true;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let scene = &mut app.tabs[i].scene;
        let [outer, inner, upper, lower] = [
            ([0.0, 0.0], [0.0, 5.0]),
            ([-1.0, -1.0], [-1.0, 5.0]),
            ([4.0, 0.0], [0.0, 0.0]),
            ([4.0, -1.0], [-1.0, -1.0]),
        ]
        .map(|(a, b)| {
            scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::new(a[0], a[1], 0.0),
                Vector3::new(b[0], b[1], 0.0),
            )))
        });
        let top = scene.add_entity(EntityType::Arc(Arc::from_coords(
            -0.5,
            5.0,
            0.0,
            0.5,
            0.0,
            std::f64::consts::PI,
        )));
        let right = scene.add_entity(EntityType::Arc(Arc::from_coords(
            4.0,
            -0.5,
            0.0,
            0.5,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        )));
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        for (a, ai, b, bi) in [
            (outer, 1, top, 0),
            (top, 1, inner, 1),
            (outer, 0, upper, 1),
            (upper, 0, right, 1),
            (inner, 0, lower, 1),
            (right, 0, lower, 0),
        ] {
            set.add(
                ConstraintKind::Coincident,
                vec![ParametricRef::point(a, ai), ParametricRef::point(b, bi)],
                None,
            );
        }
        for (kind, a, b) in [
            (ConstraintKind::Parallel, outer, inner),
            (ConstraintKind::Parallel, upper, lower),
            (ConstraintKind::Tangent, outer, top),
            (ConstraintKind::Tangent, inner, top),
            (ConstraintKind::Tangent, upper, right),
            (ConstraintKind::Tangent, lower, right),
            (ConstraintKind::Equal, top, right),
        ] {
            set.add(
                kind,
                vec![ParametricRef::whole(a), ParametricRef::whole(b)],
                None,
            );
        }
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(lower)],
            None,
        );
        let handles = if let Ok(path) = std::env::var("OCS_GRIP_TEST_DRAWING") {
            let result =
                app.automation_op(&serde_json::json!({"op":"open","path":path}).to_string());
            assert_eq!(result["ok"], true, "{result}");
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
            [0x57F, 0x57D, 0x57C, 0x57A, 0x57E, 0x57B].map(Handle::new)
        } else {
            [outer, inner, upper, lower, top, right]
        };
        let inner = handles[1];
        let geometry = |app: &OpenCADStudio| {
            handles.map(|handle| {
                app.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .unwrap()
                    .clone()
            })
        };
        let before = geometry(&app);
        let EntityType::Line(original_line) = &before[1] else {
            panic!("line")
        };
        let lower_endpoint = original_line.start;
        let upper_endpoint = original_line.end;
        let EntityType::Arc(original_arc) = &before[4] else {
            panic!("arc")
        };
        let radius = original_arc.radius;
        app.tabs[i].scene.selected.insert(inner);
        app.refresh_selected_grips();
        app.tabs[i].active_grip = Some(GripEdit::single(
            inner,
            0,
            false,
            glam::DVec3::new(lower_endpoint.x, lower_endpoint.y, 0.0),
        ));
        let check_profile = |app: &OpenCADStudio| {
            let entities = geometry(app);
            let EntityType::Line(line) = &entities[1] else {
                panic!("line")
            };
            assert_eq!(line.end, upper_endpoint, "top endpoint drifted");
            let EntityType::Arc(arc) = &entities[4] else {
                panic!("arc")
            };
            assert!(
                (arc.end_point() - line.end).length() < 1e-7,
                "top arc detached"
            );
            for index in [4, 5] {
                let EntityType::Arc(arc) = &entities[index] else {
                    panic!("arc")
                };
                assert!((arc.radius - radius).abs() < 1e-7, "parallel gap changed");
            }
            for (line_index, arc_index, marker) in [(0, 4, 0), (1, 4, 1), (2, 5, 1), (3, 5, 0)] {
                let EntityType::Line(line) = &entities[line_index] else {
                    panic!("line")
                };
                let EntityType::Arc(arc) = &entities[arc_index] else {
                    panic!("arc")
                };
                let contact = if marker == 0 {
                    arc.start_point()
                } else {
                    arc.end_point()
                };
                let endpoint = if line_index < 2 { line.end } else { line.start };
                assert!(
                    (contact - endpoint).length() < 1e-7,
                    "tangent contact detached"
                );
                let direction = line.end - line.start;
                assert!(
                    direction.dot(&(contact - arc.center)).abs() < 1e-7,
                    "tangency lost: line={line:?}, arc={arc:?}, error={}",
                    direction.dot(&(contact - arc.center))
                );
            }
        };
        for offset in [[-0.4, -0.5], [-1.0, -1.0], [-1.0, -1.0], [0.3, 0.4]] {
            let target = [lower_endpoint.x + offset[0], lower_endpoint.y + offset[1]];
            let cursor = app.tabs[i]
                .scene
                .camera
                .borrow()
                .project(
                    glam::DVec3::new(target[0], target[1], 0.0),
                    iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
                )
                .unwrap();
            let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));
            check_profile(&app);
            let EntityType::Line(line) = app.tabs[i].scene.document.get_entity(inner).unwrap()
            else {
                panic!("line")
            };
            assert!((line.start - Vector3::new(target[0], target[1], 0.0)).length() < 1e-5);
            for handle in handles {
                assert!(
                    app.tabs[i]
                        .scene
                        .preview_wires
                        .iter()
                        .any(|wire| wire.name == handle.value().to_string() && wire.color[3] > 0.9),
                    "connected entity {handle:?} missing from preview"
                );
            }
        }
        let preview = geometry(&app);
        let _ = app.on_viewport_left_release();
        check_profile(&app);
        assert_eq!(
            geometry(&app),
            preview,
            "release changed the preview solution"
        );
        app.undo_steps(1);
        assert_eq!(geometry(&app), before);
        app.redo_steps(1);
        assert_eq!(geometry(&app), preview);
    }

    #[test]
    fn grip_moves_keep_perpendicular_constraints_live_and_undoable() {
        use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef};
        use codec::{entities::Line, types::Vector3, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let a = app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(-10.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            )));
        let b = app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(15.0, -10.0, 0.0),
                Vector3::new(15.0, 10.0, 0.0),
            )));
        let _ = app.apply_cmd_result(crate::command::CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Perpendicular,
            refs: vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            driving_param: None,
            label: "Perpendicular constraint",
        });
        let geometry = |app: &OpenCADStudio| {
            [a, b].map(
                |handle| match app.tabs[i].scene.document.get_entity(handle).unwrap() {
                    EntityType::Line(line) => [line.start, line.end],
                    _ => panic!("expected line"),
                },
            )
        };
        let before = geometry(&app);
        app.tabs[i].active_grip = Some(GripEdit::single(
            a,
            1,
            false,
            glam::DVec3::new(10.0, 0.0, 0.0),
        ));
        for cursor in [Point::new(520.0, 220.0), Point::new(490.0, 180.0)] {
            let _ = app.on_viewport_move(cursor);
            let current = geometry(&app);
            for point in current.iter().flatten() {
                assert_eq!(point.z, 0.0, "grip left the drawing plane: {point:?}");
            }
            let direction = |ends: [Vector3; 2]| {
                glam::DVec3::new(
                    ends[1].x - ends[0].x,
                    ends[1].y - ends[0].y,
                    ends[1].z - ends[0].z,
                )
                .normalize()
            };
            assert!(direction(current[0]).dot(direction(current[1])).abs() < 1e-8);
            assert_ne!(
                current[1], before[1],
                "the constrained neighbor must follow the edited line"
            );
            assert_eq!(
                current[0][0], before[0][0],
                "the endpoint opposite the dragged grip must stay fixed"
            );
        }
        let _ = app.on_viewport_left_release();
        assert!(app.tabs[i].active_grip.is_none());
        let after = geometry(&app);
        app.undo_steps(1);
        assert_eq!(geometry(&app), before);
        app.redo_steps(1);
        assert_eq!(geometry(&app), after);
    }

    #[test]
    fn constrained_rectangle_grips_distinguish_perpendicular_corner_and_free_ends() {
        use crate::scene::parametric_constraints::{
            ConstraintKind, ParametricRef, ParametricScope,
        };
        use codec::{entities::LwPolyline, types::Vector2, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.constraint_solve_mode = true;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let mut rectangle = LwPolyline::from_points(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(4.0, 0.0),
            Vector2::new(4.0, 2.0),
            Vector2::new(0.0, 2.0),
        ]);
        rectangle.is_closed = true;
        let handle = app.tabs[i]
            .scene
            .add_entity(EntityType::LwPolyline(rectangle));
        let set = app.tabs[i]
            .scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Parallel,
            vec![
                ParametricRef::segment(handle, 0),
                ParametricRef::segment(handle, 2),
            ],
            None,
        );
        set.add(
            ConstraintKind::Parallel,
            vec![
                ParametricRef::segment(handle, 1),
                ParametricRef::segment(handle, 3),
            ],
            None,
        );
        set.add(
            ConstraintKind::Perpendicular,
            vec![
                ParametricRef::segment(handle, 3),
                ParametricRef::segment(handle, 2),
            ],
            None,
        );
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::segment(handle, 2)],
            None,
        );
        let handle = if let Ok(path) = std::env::var("OCS_GRIP_TEST_DRAWING") {
            let result =
                app.automation_op(&serde_json::json!({"op":"open","path":path}).to_string());
            assert_eq!(result["ok"], true, "{result}");
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
            Handle::new(0x556)
        } else {
            handle
        };
        let vertices = |app: &OpenCADStudio| {
            let EntityType::LwPolyline(polyline) =
                app.tabs[i].scene.document.get_entity(handle).unwrap()
            else {
                panic!("expected polyline")
            };
            polyline
                .vertices
                .iter()
                .map(|vertex| vertex.location)
                .collect::<Vec<_>>()
        };
        let before = vertices(&app);
        let height = before[3].y - before[0].y;
        let width = before[1].x - before[0].x;
        for grip_id in 0..8 {
            let vertex = if grip_id < 4 {
                before[grip_id]
            } else {
                (before[grip_id - 4] + before[(grip_id - 3) % 4]) * 0.5
            };
            let origin = glam::DVec3::new(vertex.x, vertex.y, 0.0);
            app.tabs[i].active_grip = Some(GripEdit::single(handle, grip_id, grip_id >= 4, origin));
            for offset in [
                [-1.0, height],
                [-1.0, 3.0],
                [-2.0, 6.0],
                [-2.0, 6.0],
                [1.0, -4.0],
            ] {
                let target = origin + glam::DVec3::new(offset[0], offset[1], 0.0);
                let cursor = app.tabs[i]
                    .scene
                    .camera
                    .borrow()
                    .project(
                        target,
                        iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
                    )
                    .unwrap();
                let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));
                let after = vertices(&app);
                let (left, right, bottom, top) = match grip_id {
                    0 => (target.x, target.x + width, target.y, before[3].y),
                    1 => (before[0].x, target.x, target.y, before[3].y),
                    2 => (before[0].x, target.x, target.y - height, target.y),
                    3 => (target.x, target.x + width, target.y - height, target.y),
                    4 => (
                        before[0].x + offset[0],
                        before[1].x + offset[0],
                        target.y,
                        before[3].y,
                    ),
                    5 => (
                        before[0].x,
                        target.x,
                        before[0].y + offset[1],
                        before[3].y + offset[1],
                    ),
                    6 => (
                        before[0].x + offset[0],
                        before[1].x + offset[0],
                        before[0].y,
                        target.y,
                    ),
                    7 => (
                        target.x,
                        before[1].x,
                        before[0].y + offset[1],
                        before[3].y + offset[1],
                    ),
                    _ => unreachable!(),
                };
                let expected = [
                    Vector2::new(left, bottom),
                    Vector2::new(right, bottom),
                    Vector2::new(right, top),
                    Vector2::new(left, top),
                ];
                for (actual, expected) in after.iter().zip(expected) {
                    assert!((*actual - expected).length_squared() < 1e-10,
                    "grip {grip_id}: rectangle collapsed or drifted: {after:?}, expected {expected:?}");
                }
            }
            let preview = vertices(&app);
            let _ = app.on_viewport_left_release();
            assert_eq!(vertices(&app), preview);
            app.undo_steps(1);
            assert_eq!(vertices(&app), before);
            app.redo_steps(1);
            assert_eq!(vertices(&app), preview);
            app.undo_steps(1);
        }
    }

    #[test]
    fn axis_constrained_line_grip_changes_length_and_translates_normal_to_axis() {
        use crate::scene::parametric_constraints::{
            ConstraintKind, ParametricRef, ParametricScope,
        };
        use codec::{entities::Line, types::Vector3, EntityType};

        for vertical in [false, true] {
            let mut app = OpenCADStudio::new_for_test();
            app.automation_op(r#"{"op":"new"}"#);
            let i = app.active_tab;
            app.snapper.snap_enabled = false;
            app.snapper.grid_snap_on = false;
            app.snapper.otrack_enabled = false;
            app.ortho_mode = false;
            app.polar_mode = false;
            app.constraint_solve_mode = true;
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
            let end = if vertical {
                Vector3::new(0.0, 4.0, 0.0)
            } else {
                Vector3::new(4.0, 0.0, 0.0)
            };
            let handle = app.tabs[i]
                .scene
                .add_entity(EntityType::Line(Line::from_points(Vector3::ZERO, end)));
            app.tabs[i]
                .scene
                .parametric_constraint_set_mut(ParametricScope::ModelSpace)
                .add(
                    if vertical {
                        ConstraintKind::Vertical
                    } else {
                        ConstraintKind::Horizontal
                    },
                    vec![ParametricRef::whole(handle)],
                    None,
                );
            app.tabs[i].active_grip = Some(GripEdit::single(handle, 0, false, glam::DVec3::ZERO));
            for target in [
                glam::DVec3::new(-1.0, 3.0, 0.0),
                glam::DVec3::new(2.0, -2.0, 0.0),
            ] {
                let cursor = app.tabs[i]
                    .scene
                    .camera
                    .borrow()
                    .project(
                        target,
                        iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
                    )
                    .unwrap();
                let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));
                let EntityType::Line(line) = app.tabs[i].scene.document.get_entity(handle).unwrap()
                else {
                    panic!("line")
                };
                let opposite = if vertical {
                    Vector3::new(target.x, end.y, 0.0)
                } else {
                    Vector3::new(end.x, target.y, 0.0)
                };
                assert!((line.start - Vector3::new(target.x, target.y, 0.0)).length() < 1e-5);
                assert!((line.end - opposite).length() < 1e-5, "{line:?}");
            }
        }
    }

    #[test]
    fn a_point_step_takes_the_point_where_the_button_went_down() {
        use codec::EntityType;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
        let cursor = |app: &OpenCADStudio, x: f64, y: f64| {
            let at = app.tabs[i]
                .scene
                .camera
                .borrow()
                .project(glam::DVec3::new(x, y, 0.0), bounds);
            let at = at.unwrap();
            iced::Point::new(at.x, at.y)
        };
        let _ = app.run_command_line("LINE");
        // Away from the origin, where the UCS icon takes presses.
        for (down, up) in [((40.0, 30.0), (45.0, 34.0)), ((80.0, 30.0), (86.0, 25.0))] {
            let _ = app.on_viewport_move(cursor(&app, down.0, down.1));
            let _ = app.on_viewport_left_press();
            // The hand moves before the button comes up.
            let _ = app.on_viewport_move(cursor(&app, up.0, up.1));
            let _ = app.on_viewport_left_release();
        }
        let _ = app.feed_command(crate::command::StepInput::Enter);
        let lines: Vec<_> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Line(line) => Some(line.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            lines.len(),
            1,
            "one point per click, not one per press and release"
        );
        let line = &lines[0];
        assert!(
            (line.start.x - 40.0).abs() < 0.5 && (line.start.y - 30.0).abs() < 0.5,
            "{line:?}"
        );
        assert!(
            (line.end.x - 80.0).abs() < 0.5 && (line.end.y - 30.0).abs() < 0.5,
            "{line:?}"
        );
    }

    #[test]
    fn xline_direction_grip_drag_changes_direction() {
        use codec::{types::Vector3, EntityType};
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        // Base away from the model origin so the UCS icon cannot swallow
        // the press (it sits at the origin in a fresh drawing).
        let handle = app.tabs[i]
            .scene
            .add_entity(EntityType::XLine(codec::entities::XLine::new(
                Vector3::new(100.0, 50.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )));
        app.tabs[i].scene.selected.insert(handle);
        app.refresh_selected_grips();
        assert_eq!(app.tabs[i].selected_grips.len(), 2);
        // Press on grip 1 (direction handle) and drag it to point along +X.
        let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
        let g1world = app.tabs[i].selected_grips[1].world;
        let g1cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(g1world, bounds)
            .unwrap();
        app.tabs[i].scene.selection.borrow_mut().input.last_move_pos =
            Some(iced::Point::new(g1cursor.x, g1cursor.y));
        let _ = app.on_viewport_left_press();
        assert_eq!(
            app.tabs[i].active_grip.as_ref().map(|g| g.grip_id),
            Some(1),
            "press on the direction grip must engage grip 1"
        );
        let dir_target = glam::DVec3::new(200.0, 50.0, 0.0);
        let dir_cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(dir_target, bounds)
            .unwrap();
        let _ = app.on_viewport_move(iced::Point::new(dir_cursor.x, dir_cursor.y));
        let EntityType::XLine(moved) = app.tabs[i].scene.document.get_entity(handle).unwrap()
        else {
            panic!("expected xline")
        };
        assert!(
            (moved.direction.x - 1.0).abs() < 1e-6 && moved.direction.y.abs() < 1e-6,
            "direction grip drag must point the xline along +X, got {:?}",
            moved.direction
        );
    }

    #[test]
    fn constraint_glyph_tooltip_appears_after_hover_dwell() {
        use crate::scene::parametric_constraints::{
            ConstraintKind, ParametricRef, ParametricScope,
        };
        use codec::{entities::Line, types::Vector3, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let handle = app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )));
        app.constraint_bar_display = 3;
        let _ = app.apply_cmd_result(crate::command::CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(handle)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        let anchor = app.tabs[i].scene.constraint_glyph_placements_screen(
            ParametricScope::ModelSpace,
            (800.0, 600.0),
            true,
            3,
            4095,
        )[0]
        .1;
        let point = (-30..=30)
            .find_map(|y| {
                (-30..=30).find_map(|x| {
                    let point = Point::new(anchor.x + x as f32, anchor.y + y as f32);
                    app.constraint_glyph_under(i, point)
                        .is_some()
                        .then_some(point)
                })
            })
            .expect("projected constraint glyph should be hit-testable");

        let _ = app.on_viewport_move(point);
        assert_eq!(app.constraint_glyph_tooltip, None);
        app.hover_dwell.as_mut().unwrap().last_move_at = Instant::now()
            - std::time::Duration::from_millis(crate::app::HOVER_DWELL_MS as u64 + 1);
        let _ = app.on_hover_dwell_tick();

        assert_eq!(
            app.constraint_glyph_tooltip,
            Some(ConstraintKind::Horizontal)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn hyperlink_click_respects_existing_pointer_owners() {
        use codec::entities::Line;
        use codec::types::Vector3;
        use codec::xdata::{ExtendedDataRecord, XDataValue};
        use codec::EntityType;
        use glam::DVec3;

        for mode in ["link", "plain", "pan", "orbit", "zoom", "grip", "command"] {
            let mut app = OpenCADStudio::new_for_test();
            app.automation_op(r#"{"op":"new"}"#);
            let i = app.active_tab;
            app.show_ucs_icon = false;
            // Keep the browser-opening window task deferred during this test.
            app.main_window = Some(iced::window::Id::unique());
            let mut line =
                Line::from_points(Vector3::new(-10.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
            let mut record = ExtendedDataRecord::new("PE_URL");
            record.add_value(XDataValue::String("https://example.com/linked".into()));
            line.common.extended_data.add_record(record);
            let handle = app.tabs[i].scene.add_entity(EntityType::Line(line));
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
            let _ = app.run_command_line("ZOOM EXTENTS");
            app.tabs[i].scene.selection.borrow_mut().input.last_move_pos =
                Some(Point::new(400.0, 300.0));
            app.ctrl_down = mode != "plain";
            match mode {
                "pan" => app.tabs[i].pan_mode = true,
                "orbit" => app.tabs[i].orbit_mode = true,
                "zoom" => app.tabs[i].zoom_dynamic_mode = true,
                "grip" => {
                    app.tabs[i].active_grip = Some(GripEdit::single(handle, 0, false, DVec3::ZERO))
                }
                "command" => {
                    let _ = app.run_command_line("LINE");
                }
                _ => {}
            }
            let task = app.on_viewport_left_press();
            let opened = app
                .command_line
                .history
                .iter()
                .any(|entry| entry.text.contains("https://example.com/linked"));
            assert_eq!(opened, mode == "link", "{mode}");
            if mode == "link" {
                assert!(task.units() > 0);
                let selection = app.tabs[i].scene.selection.borrow();
                assert!(!selection.input.left_down && selection.gesture.box_anchor.is_none());
            } else if matches!(mode, "pan" | "orbit" | "zoom") {
                assert!(
                    app.tabs[i].scene.selection.borrow().input.middle_down,
                    "{mode}"
                );
            }
        }
    }

    /// Drive one settled rollover pick over a line and report what the scene
    /// ended up highlighting.
    fn rollover_hits(preview: u8) -> bool {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.model_space.selection_preview = preview;
        let _ = app.run_command_line("LINE 0,0 10,10");
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let _ = app.run_command_line("ZOOM EXTENTS");

        // The pick only runs once the cursor has been still for the dwell
        // window, so the arming timestamp is backdated past it.
        app.hover_dwell = Some(HoverDwell {
            last_move_at: Instant::now()
                - std::time::Duration::from_millis(HOVER_DWELL_MS as u64 * 2),
            point: iced::Point::new(400.0, 300.0),
            tile_size: (800.0, 600.0),
            tab: i,
        });
        let _ = app.on_hover_dwell_tick();
        app.tabs[i].scene.hover_highlight.is_some()
    }

    /// Bit 1 of `SELECTIONPREVIEW` is the rollover that runs with no command
    /// active, and the Options card gives it a checkbox. The config comment
    /// used to describe the bits the wrong way round, so which bit does what
    /// is worth asserting rather than reading.
    #[test]
    fn the_idle_rollover_follows_bit_one() {
        assert!(
            rollover_hits(1),
            "bit 1 set: the line under the cursor must be highlighted",
        );
        assert!(rollover_hits(3), "both bits set: still highlighted",);
        assert!(!rollover_hits(0), "preview off: nothing may be highlighted",);
        assert!(
            !rollover_hits(2),
            "only the in-command bit: the idle rollover stays off",
        );
    }

    /// Snapping the first LINE point to a 3D vertex must keep its elevation:
    /// the marker, the preview point and (via the click path) the committed
    /// point all carry the snapped Z instead of dropping to the XY plane.
    #[test]
    fn line_snap_to_3d_endpoint_keeps_elevation() {
        use crate::snap::SnapType;
        use codec::{entities::Line, types::Vector3, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = true;
        app.snapper.enabled.insert(SnapType::Endpoint);
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 25.0),
            )));
        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");
        assert!(app.tabs[i].active_cmd.is_some(), "LINE did not start");

        let cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(
                glam::DVec3::new(10.0, 0.0, 25.0),
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .unwrap();
        let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

        let hit = app.tabs[i]
            .snap_result
            .expect("endpoint snap missed the 3D vertex");
        assert_eq!(hit.snap_type, SnapType::Endpoint);
        assert!(
            (hit.world.z - 25.0).abs() < 1e-6,
            "snap landed off-elevation: {:?}",
            hit.world
        );
        let preview = app.tabs[i].last_cursor_world;
        assert!(
            (preview.z - 25.0).abs() < 1e-6,
            "command point lost its elevation: {preview:?}"
        );
    }

    /// The user's repro: BOX, then LINE snapping its first point to a box
    /// corner. Solid B-rep corners snap as 3D Vertex points, gated by the
    /// separate 3D master toggle — the 2D Node mode must not catch them.
    #[test]
    fn line_snaps_to_box_corner() {
        use crate::snap::SnapType;
        use codec::{entities::Solid3D, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        // 2D osnap fully off: only the 3D system may fire.
        app.snapper.snap_enabled = false;
        app.snapper.snap3d_enabled = true;
        app.snapper.enabled3d.insert(SnapType::Vertex);
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

        // In-app BOX equivalent: kernel box committed as a SAT solid.
        let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
            .expect("kernel box");
        let placed = crate::scene::model::solid_model::placed(
            &base,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        )
        .expect("place box");
        let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
        let mut entity = Solid3D::new();
        entity.set_sat_document(&sat);
        entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
        let h = app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

        // A real corner straight from the solid's edge data: the topmost
        // one, since coincident corners resolve to the nearer eye.
        let target = {
            let set = app.tabs[i].scene.meshes.get(&h).expect("box mesh");
            let (edges, lows) = set.geometry_edges();
            assert!(edges.len() >= 2, "box mesh has no B-rep edges");
            let at = |index: usize| {
                let hi = edges[index];
                let lo = lows.get(index).copied().unwrap_or([0.0; 3]);
                glam::DVec3::new(
                    hi[0] as f64 + lo[0] as f64,
                    hi[1] as f64 + lo[1] as f64,
                    hi[2] as f64 + lo[2] as f64,
                )
            };
            (0..edges.len())
                .map(at)
                .max_by(|a, b| a.z.total_cmp(&b.z))
                .expect("box mesh has no endpoints")
        };

        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");
        assert!(app.tabs[i].active_cmd.is_some(), "LINE did not start");
        let cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(
                target,
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .unwrap();
        let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

        let hit = app.tabs[i]
            .snap_result
            .expect("no snap point on the box corner");
        assert_eq!(
            hit.snap_type,
            SnapType::Vertex,
            "expected a box-corner vertex, got {:?} at {:?}",
            hit.snap_type,
            hit.world
        );
        assert!(
            (hit.world - target).length() < 1e-6,
            "snap missed the corner: {:?} vs {target:?}",
            hit.world
        );
        let preview = app.tabs[i].last_cursor_world;
        assert!(
            (preview - target).length() < 1e-6,
            "command point left the corner: {preview:?} vs {target:?}"
        );
    }

    /// With the 3D master off, solid corners are invisible even when the
    /// Vertex mode is configured — and the 2D system must not catch them.
    #[test]
    fn box_corner_ignores_3d_master_off() {
        use crate::snap::SnapType;
        use codec::{entities::Solid3D, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = true;
        app.snapper.snap3d_enabled = false;
        app.snapper.enabled3d.insert(SnapType::Vertex);
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

        let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
            .expect("kernel box");
        let placed = crate::scene::model::solid_model::placed(
            &base,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        )
        .expect("place box");
        let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
        let mut entity = Solid3D::new();
        entity.set_sat_document(&sat);
        entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
        let h = app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

        let target = {
            let set = app.tabs[i].scene.meshes.get(&h).expect("box mesh");
            let (edges, lows) = set.geometry_edges();
            let hi = edges[0];
            let lo = lows.first().copied().unwrap_or([0.0; 3]);
            glam::DVec3::new(
                hi[0] as f64 + lo[0] as f64,
                hi[1] as f64 + lo[1] as f64,
                hi[2] as f64 + lo[2] as f64,
            )
        };

        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");
        let cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(
                target,
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .unwrap();
        let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

        assert!(
            app.tabs[i].snap_result.is_none(),
            "3D master off must hide solid corners, got {:?}",
            app.tabs[i].snap_result.map(|s| (s.snap_type, s.world))
        );
    }

    /// Face centres snap as their own 3D type at the loop average (exact
    /// for the box's planar faces).
    #[test]
    fn line_snaps_to_box_face_center() {
        use crate::snap::SnapType;
        use codec::{entities::Solid3D, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = false;
        app.snapper.snap3d_enabled = true;
        app.snapper.enabled3d.insert(SnapType::FaceCenter);
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

        let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
            .expect("kernel box");
        let placed = crate::scene::model::solid_model::placed(
            &base,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        )
        .expect("place box");
        let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
        let mut entity = Solid3D::new();
        entity.set_sat_document(&sat);
        entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
        app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

        // Top-face centre of the 10×10×5 box.
        let target = glam::DVec3::new(0.0, 0.0, 5.0);
        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");
        let cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(
                target,
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .unwrap();
        let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

        let hit = app.tabs[i]
            .snap_result
            .expect("no snap point on the box face");
        assert_eq!(
            hit.snap_type,
            SnapType::FaceCenter,
            "expected a face centre, got {:?} at {:?}",
            hit.snap_type,
            hit.world
        );
        assert!(
            (hit.world - target).length() < 1e-6,
            "snap missed the face centre: {:?} vs {target:?}",
            hit.world
        );
    }

    /// Nearest-to-face catches an interior face point with no base point.
    #[test]
    fn line_snaps_to_box_face_nearest() {
        use crate::snap::SnapType;
        use codec::{entities::Solid3D, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = false;
        app.snapper.snap3d_enabled = true;
        app.snapper.enabled3d.insert(SnapType::NearestFace);
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

        let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
            .expect("kernel box");
        let placed = crate::scene::model::solid_model::placed(
            &base,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        )
        .expect("place box");
        let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
        let mut entity = Solid3D::new();
        entity.set_sat_document(&sat);
        entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
        app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

        // Interior point of the top face (off the diagonal so exactly one
        // mesh triangle owns it).
        let target = glam::DVec3::new(2.0, 3.0, 5.0);
        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");
        let cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(
                target,
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .unwrap();
        let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

        let hit = app.tabs[i]
            .snap_result
            .expect("no snap point on the box face");
        assert_eq!(
            hit.snap_type,
            SnapType::NearestFace,
            "expected nearest-to-face, got {:?} at {:?}",
            hit.snap_type,
            hit.world
        );
        assert!(
            (hit.world - target).length() < 1e-3,
            "snap missed the face point: {:?} vs {target:?}",
            hit.world
        );
    }

    /// Perpendicular-to-face drops from an explicit base point: exercised
    /// grey-box (real mesh, real helper) since it needs no event plumbing.
    #[test]
    fn face_perpendicular_needs_base_and_interior_foot() {
        use codec::{entities::Solid3D, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

        let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
            .expect("kernel box");
        let placed = crate::scene::model::solid_model::placed(
            &base,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        )
        .expect("place box");
        let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
        let mut entity = Solid3D::new();
        entity.set_sat_document(&sat);
        entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
        app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));
        let _ = app.run_command_line("ZOOM EXTENTS");

        let scene = &app.tabs[i].scene;
        let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
        let cam = scene.camera.borrow();
        let view_rot = cam.view_proj_rte(bounds);
        let eye = cam.eye();
        let foot = glam::DVec3::new(2.0, 3.0, 5.0);
        let cursor = cam.project(foot, bounds).unwrap();
        drop(cam);
        let cursor = Point::new(cursor.x, cursor.y);

        // Base below the face interior → foot straight above it.
        let hit = app.tabs[i].scene.solid_face_snaps(
            cursor,
            view_rot,
            eye,
            bounds,
            15.0,
            Some(glam::DVec3::new(2.0, 3.0, 0.0)),
            true,
            true,
        );
        let hit = hit.expect("perpendicular foot missed");
        assert_eq!(hit.snap_type, crate::snap::SnapType::FacePerpendicular);
        assert!(
            (hit.world - foot).length() < 1e-3,
            "foot off target: {:?} vs {foot:?}",
            hit.world
        );

        // No base → perpendicular cannot fire (nearest still can).
        let hit = app.tabs[i]
            .scene
            .solid_face_snaps(cursor, view_rot, eye, bounds, 15.0, None, true, false);
        assert!(hit.is_none(), "perp without a base must stay silent");

        // Neither mode wanted → silence.
        let hit = app.tabs[i].scene.solid_face_snaps(
            cursor,
            view_rot,
            eye,
            bounds,
            15.0,
            Some(glam::DVec3::new(2.0, 3.0, 0.0)),
            false,
            false,
        );
        assert!(hit.is_none(), "unwanted modes must stay silent");
    }

    /// Edge midpoints snap as their own 3D type at the segment centre.
    #[test]
    fn line_snaps_to_box_edge_midpoint() {
        use crate::snap::SnapType;
        use codec::{entities::Solid3D, EntityType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = false;
        app.snapper.snap3d_enabled = true;
        app.snapper.enabled3d.insert(SnapType::EdgeMidpoint);
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

        let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
            .expect("kernel box");
        let placed = crate::scene::model::solid_model::placed(
            &base,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        )
        .expect("place box");
        let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
        let mut entity = Solid3D::new();
        entity.set_sat_document(&sat);
        entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
        let h = app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

        // Midpoint of a top edge (both ends at max z): bottom edges project
        // onto the same pixels, and coincident points resolve to the eye.
        let target = {
            let set = app.tabs[i].scene.meshes.get(&h).expect("box mesh");
            let (edges, lows) = set.geometry_edges();
            assert!(edges.len() >= 2, "box mesh has no B-rep edges");
            let at = |index: usize| {
                let hi = edges[index];
                let lo = lows.get(index).copied().unwrap_or([0.0; 3]);
                glam::DVec3::new(
                    hi[0] as f64 + lo[0] as f64,
                    hi[1] as f64 + lo[1] as f64,
                    hi[2] as f64 + lo[2] as f64,
                )
            };
            let top = (0..edges.len())
                .map(at)
                .map(|p| p.z)
                .fold(f64::NEG_INFINITY, f64::max);
            (0..edges.len() / 2)
                .map(|k| (at(2 * k), at(2 * k + 1)))
                .find(|(a, b)| (a.z - top).abs() < 1e-9 && (b.z - top).abs() < 1e-9)
                .map(|(a, b)| (a + b) * 0.5)
                .expect("box mesh has no top edge")
        };

        let _ = app.run_command_line("ZOOM EXTENTS");
        let _ = app.run_command_line("LINE");
        let cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(
                target,
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .unwrap();
        let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

        let hit = app.tabs[i]
            .snap_result
            .expect("no snap point on the box edge");
        assert_eq!(
            hit.snap_type,
            SnapType::EdgeMidpoint,
            "expected an edge midpoint, got {:?} at {:?}",
            hit.snap_type,
            hit.world
        );
        assert!(
            (hit.world - target).length() < 1e-6,
            "snap missed the edge midpoint: {:?} vs {target:?}",
            hit.world
        );
    }
}

#[cfg(test)]
mod behind_eye_snap_tests {
    use super::*;
    use codec::{entities::Solid3D, EntityType};

    fn app_with_box(center_z: f64) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let base = crate::scene::model::solid_model::box_solid(
            [0.0, 0.0, center_z],
            10.0,
            10.0,
            5.0,
        )
        .expect("kernel box");
        let placed = crate::scene::model::solid_model::placed(
            &base,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        )
        .expect("place box");
        let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
        let mut entity = Solid3D::new();
        entity.set_sat_document(&sat);
        entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
        app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));
        app
    }

    /// Face snaps project mesh triangles to screen space. A triangle behind a
    /// perspective eye mirrors onto visible pixels, so without an eye-plane
    /// guard a hidden face can win a snap right under the cursor (#64).
    #[test]
    fn behind_eye_solid_face_does_not_snap() {
        let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
        let eye = glam::DVec3::ZERO;
        let view_rot = glam::camera::rh::proj::directx::perspective(
            45.0_f32.to_radians(),
            bounds.width / bounds.height,
            0.1,
            2000.0,
        );
        // Where the raw projection places a point: in front of the eye this is
        // its real pixel, behind it the mirrored pixel the bug would snap to.
        let pixel = |p: glam::DVec3| {
            let ndc = view_rot.project_point3((p - eye).as_vec3());
            Point::new(
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            )
        };
        let snap = |app: &OpenCADStudio, cursor: Point| {
            app.tabs[app.active_tab].scene.solid_face_snaps(
                cursor,
                view_rot,
                eye,
                bounds,
                15.0,
                None,
                false,
                true,
            )
        };

        // Control: a box in front of the eye snaps on its face.
        let front = app_with_box(-102.5);
        let hit = snap(&front, pixel(glam::DVec3::new(2.0, 3.0, -100.0)))
            .expect("a front solid face must snap");
        assert_eq!(hit.snap_type, crate::snap::SnapType::NearestFace);

        // The same box wholly behind the eye mirrors onto those pixels.
        let behind = app_with_box(102.5);
        assert!(
            snap(&behind, pixel(glam::DVec3::new(2.0, 3.0, 100.0))).is_none(),
            "a behind-eye solid face must not snap"
        );
    }
}

/// Where a box selection spent its time, in milliseconds since each step's
/// start (perf builds only; zero otherwise). `handles`, `wires` and `hatch`
/// are cumulative within `hit`.
#[derive(Default)]
pub(in crate::app) struct BoxSelectTiming {
    pub candidates: f64,
    pub handles: f64,
    pub wires: f64,
    pub hatch: f64,
    pub hit: f64,
}

#[cfg(test)]
mod navigation_style_tests {
    use super::*;
    use crate::app::settings::{NavAction, NavButtons, NavigationStyle};

    fn app(style: NavigationStyle) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        app.navigation_style = style;
        let _ = app.update(Message::ViewportMove(Point::new(400.0, 300.0)));
        app
    }

    fn rotation(app: &OpenCADStudio) -> glam::Quat {
        app.tabs[app.active_tab].scene.camera.borrow().rotation
    }

    #[test]
    fn the_cad_default_still_pans_and_orbits_with_shift() {
        let middle = NavButtons {
            middle: true,
            ..Default::default()
        };
        let cad = NavigationStyle::Cad;
        assert_eq!(cad.action(middle, false, false, false, true), Some(NavAction::Pan));
        assert_eq!(cad.action(middle, true, false, false, true), Some(NavAction::Orbit));
        let left = NavButtons {
            left: true,
            ..Default::default()
        };
        assert_eq!(cad.action(left, false, false, false, false), None);
    }

    #[test]
    fn a_gesture_left_drag_orbits_and_a_left_click_stays_a_click() {
        let mut app = app(NavigationStyle::Gesture);
        let before = rotation(&app);
        let _ = app.update(Message::ViewportLeftPress);
        let _ = app.update(Message::ViewportMove(Point::new(440.0, 300.0)));
        let _ = app.update(Message::ViewportLeftRelease);
        assert_ne!(rotation(&app), before);
        {
            let sel = app.tabs[app.active_tab].scene.selection.borrow();
            assert!(!sel.input.middle_down && !sel.input.left_down);
            assert!(sel.gesture.box_anchor.is_none());
        }
        let turned = rotation(&app);
        let _ = app.update(Message::ViewportLeftPress);
        assert!(app.nav_pending.is_some());
        let _ = app.update(Message::ViewportLeftRelease);
        assert!(app.nav_pending.is_none() && app.nav_held == NavButtons::default());
        assert_eq!(rotation(&app), turned);
    }

    #[test]
    fn a_catia_right_click_during_a_middle_drag_zooms() {
        let mut app = app(NavigationStyle::Catia);
        let i = app.active_tab;
        let _ = app.update(Message::ViewportMiddlePress);
        let _ = app.update(Message::ViewportRightPress);
        let _ = app.update(Message::ViewportRightRelease);
        assert_eq!(app.nav_drag_action(i), NavAction::Zoom);
        let distance = app.tabs[i].scene.camera.borrow().distance;
        let _ = app.update(Message::ViewportMove(Point::new(400.0, 260.0)));
        assert_ne!(app.tabs[i].scene.camera.borrow().distance, distance);
        let _ = app.update(Message::ViewportMiddleRelease);
        assert!(!app.nav_zoom_latched);
        assert!(!app.tabs[i].scene.selection.borrow().input.middle_down);
    }
}

impl OpenCADStudio {
    /// OSNAPZ: object snaps land on the current UCS at the current
    /// elevation while it is on.
    pub(in crate::app) fn sync_osnap_z_plane(&mut self, i: usize) {
        self.snapper.osnap_z_plane = self.osnap_z.then(|| {
            let (origin, _, _, normal) = self.tabs[i].ucs_xform().axes();
            let elevation = self.tabs[i].scene.document.header.elevation;
            (origin + normal * elevation, normal)
        });
    }
}
