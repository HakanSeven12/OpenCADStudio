//! `viewport` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    axis_lock_apply, axis_lock_capture, drafting_axes, drafting_constrain, parse_coord,
    polar_constrain_if_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis, CoordKind,
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
use document::types::Color as AcadColor;
use document::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};
use std::sync::Arc;

/// Pixel radius for grabbing a UCS-icon grip (origin dot or an axis tip).
const UCS_GRIP_HIT_PX: f32 = 9.0;
/// Pixel reach for hovering/clicking the icon body (origin, tips, or an arm).
const UCS_ICON_PICK_PX: f32 = 10.0;
/// Pixel half-width for clicking an icon arm (line segment).
const UCS_ICON_ARM_PX: f32 = 6.0;

/// Rectangles covering A minus B. Boundaries overlap deliberately, because
/// an entity on a selection edge must be reconsidered as the edge moves.
fn box_rect_subtract(a: [f32; 4], b: [f32; 4], out: &mut Vec<(Point, Point)>) {
    let intersection = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    if intersection[0] >= intersection[2] || intersection[1] >= intersection[3] {
        out.push((Point::new(a[0], a[1]), Point::new(a[2], a[3])));
        return;
    }
    let [x0, y0, x1, y1] = intersection;
    for rect in [
        [a[0], a[1], a[2], y0],
        [a[0], y1, a[2], a[3]],
        [a[0], y0, x0, y1],
        [x1, y0, a[2], y1],
    ] {
        if rect[0] < rect[2] && rect[1] < rect[3] {
            out.push((Point::new(rect[0], rect[1]), Point::new(rect[2], rect[3])));
        }
    }
}

fn box_preview_rect(a: Point, b: Point, bounds: iced::Rectangle) -> Option<[f32; 4]> {
    if ![a.x, a.y, b.x, b.y, bounds.width, bounds.height]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    // Wire picking clips to the pane, but hatch and mesh picking currently do
    // not. Use a full pick outside the pane rather than caching a clipped box
    // that could miss a change in those un-clipped categories.
    if [a, b]
        .iter()
        .any(|p| p.x < 0.0 || p.y < 0.0 || p.x > bounds.width || p.y > bounds.height)
    {
        return None;
    }
    let rect = [
        a.x.min(b.x).max(0.0),
        a.y.min(b.y).max(0.0),
        a.x.max(b.x).min(bounds.width),
        a.y.max(b.y).min(bounds.height),
    ];
    (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
}

/// Decide whether a selection press has become a drag. Without a click
/// alternative a few logical pixels of travel filter jitter. When a click can
/// also arm a click-click window, the hand may still be moving as the button
/// goes down: a drag then needs the button held, or for a rectangle a long
/// fast stroke. A fast movement alone never starts a lasso.
fn selection_drag_should_start(
    press: Point,
    cursor: Point,
    held_ms: u128,
    click_window: bool,
    lasso: bool,
) -> bool {
    const JITTER_DIST2: f32 = 5.0 * 5.0;
    const CLICK_DIST2: f32 = 12.0 * 12.0;
    const RECT_FAST_DIST2: f32 = 24.0 * 24.0;
    let dx = cursor.x - press.x;
    let dy = cursor.y - press.y;
    let dist2 = dx * dx + dy * dy;
    if !click_window {
        return dist2 >= JITTER_DIST2;
    }
    dist2 > CLICK_DIST2 && (held_ms >= POLY_START_DELAY_MS || (!lasso && dist2 > RECT_FAST_DIST2))
}

/// For a short selection click, use the actual mouse-down position even if
/// the user starts moving immediately. Active drag gestures use the live
/// cursor; point commands capture their own press.
fn selection_click_position(
    last: Point,
    pressed: Option<Point>,
    is_click: bool,
    use_press: bool,
) -> Point {
    if is_click && use_press {
        pressed.unwrap_or(last)
    } else {
        last
    }
}

#[cfg(test)]
mod selection_gesture_priority_tests;

/// Whether a grip drag is a table's fill diamond: the fill reaches the cell
/// under the cursor, so no snap, tracking or polar may move that point.
fn table_fill_follows_cursor(
    originals: &[(document::Handle, document::EntityType)],
    handle: document::Handle,
    grip_id: usize,
) -> bool {
    grip_id == crate::entities::table::CELL_GRIP_FILL
        && originals.iter().any(|(original, entity)| {
            *original == handle && matches!(entity, document::EntityType::Table(_))
        })
}

#[cfg(test)]
mod table_fill_cursor_tests;

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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod underlay_reseed_tests;

#[cfg(test)]
mod extension_canvas_tests;

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
    /// Shift + trackpad movement to orbit by, in screen pixels.
    Orbit { dx: f32, dy: f32 },
}

/// How far a ZOOM Dynamic drag must travel vertically before the magnifier
/// cursor flips between "+" and "-". Without the dead zone a hand holding
/// still at the top of a drag strobes the glyph on every jittered pixel.
const ZOOM_DIR_FLIP_PX: f32 = 2.0;

/// Scroll zooms unless the user has enabled touchpad panning.
///
/// Delta units are not a reliable device identifier: smooth-scrolling mice
/// can report pixels on macOS too. Honor the explicit touchpad option for
/// both kinds of delta, with Ctrl+scroll always available for zooming and
/// Shift+scroll orbiting instead of panning.
fn scroll_intent(
    delta: mouse::ScrollDelta,
    touchpad_pan: bool,
    ctrl: bool,
    shift: bool,
) -> ScrollIntent {
    /// Screen pixels one wheel line pans.
    const LINE_PX: f32 = 40.0;
    // A notched wheel reports lines and a trackpad never does there, so on
    // macOS a line delta zooms even with touchpad panning on.
    let wheel_notch =
        cfg!(target_os = "macos") && matches!(delta, mouse::ScrollDelta::Lines { .. });
    if touchpad_pan && !ctrl && !wheel_notch {
        let (dx, dy) = match delta {
            mouse::ScrollDelta::Pixels { x, y } => (x, y),
            mouse::ScrollDelta::Lines { x, y } => (x * LINE_PX, y * LINE_PX),
        };
        return if shift {
            ScrollIntent::Orbit { dx, dy }
        } else {
            ScrollIntent::Pan { dx, dy }
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
    } else if pt_pt_d2(p, h.tips[2]) <= r2 {
        Some(crate::app::UcsGripKind::ZAxis)
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

mod pick;
mod interaction;
mod pointer_move;
mod preview;
mod navigation;
mod panes;
mod buttons;
mod double_click;
mod input;
mod layout;

pub(crate) use self::pick::*;
pub(crate) use self::interaction::*;
pub(crate) use self::pointer_move::*;
pub(crate) use self::preview::*;
pub(crate) use self::navigation::*;
pub(crate) use self::panes::*;
pub(crate) use self::buttons::*;
pub(crate) use self::double_click::*;
pub(crate) use self::input::*;
pub(crate) use self::layout::*;

#[cfg(test)]
mod table_grip_picture_tests;

#[cfg(test)]
mod selection_click_wobble_tests;

#[cfg(test)]
mod hatch_double_click_tests;

#[cfg(test)]
mod scroll_intent_tests;

#[cfg(test)]
mod rapid_drawing_tests;

#[cfg(test)]
mod zoom_dynamic_cursor_tests;

#[cfg(test)]
mod snap_elevation_tests;

#[cfg(test)]
mod selection_preview_tests;

#[cfg(test)]
mod behind_eye_snap_tests;

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
mod navigation_style_tests;

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

#[cfg(test)]
mod stretch_shift_click_tests;

#[cfg(test)]
mod grip_mtp_tests;

#[cfg(test)]
mod pointer_move_coalescing_tests;

#[cfg(test)]
mod selection_drag_tracking_tests;

#[cfg(test)]
mod box_preview_schedule_tests;
