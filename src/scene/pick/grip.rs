//! OpenCADStudio-style grip editing.

use codec::Handle;
use glam::{DVec3, Mat4, Vec2};
use iced::{Point, Rectangle};

use crate::scene::model::object::{GripDef, GripShape};

/// Pixel radius for grip hit-detection.
pub const GRIP_THRESHOLD_PX: f32 = 8.0;
/// Half-size of the rendered grip square / diamond in pixels.
pub const GRIP_HALF_PX: f32 = 5.0;
/// Screen offset for dropdown selectors anchored to a vertex.
pub const GRIP_DROPDOWN_OFFSET_X_PX: f32 = 24.0;
pub const GRIP_DROPDOWN_OFFSET_Y_PX: f32 = 27.0;
pub const GRIP_ADJACENT_DROPDOWN_OFFSET_X_PX: f32 = 12.0;

fn marker_screen_position(mut point: Point, shape: GripShape) -> Point {
    match shape {
        GripShape::Dropdown => {
            point.x += GRIP_DROPDOWN_OFFSET_X_PX;
            point.y += GRIP_DROPDOWN_OFFSET_Y_PX;
        }
        GripShape::DropdownAdjacent => {
            point.x += GRIP_ADJACENT_DROPDOWN_OFFSET_X_PX;
        }
        _ => {}
    }
    point
}

/// Screen length of a move-gizmo arrow.
pub const GIZMO_AXIS_PX: f32 = 72.0;

pub fn gizmo_axis(k: u8) -> DVec3 {
    [DVec3::X, DVec3::Y, DVec3::Z][k as usize % 3]
}

pub fn gizmo_plane_axes(k: u8) -> (u8, u8) {
    [(0, 1), (1, 2), (2, 0)][k as usize % 3]
}

/// Unit screen directions of the three gizmo axes from `center`; an axis
/// seen nearly end-on is left out.
pub fn gizmo_screen_axes(
    center: Vec2,
    world: DVec3,
    project: impl Fn(DVec3) -> Option<Vec2>,
) -> [Option<Vec2>; 3] {
    let spans = [0, 1, 2].map(|k| project(world + gizmo_axis(k)).map(|p| p - center));
    let longest = spans.iter().flatten().map(|v| v.length()).fold(0.0, f32::max);
    spans.map(|span| span.filter(|v| longest > 0.0 && v.length() > 0.15 * longest).map(|v| v.normalize()))
}

/// Marker position (and direction) of a grip whose centre projects to
/// `projected`: gizmo grips sit off their centre, others keep their offset.
fn placed_marker(
    g: &GripDef,
    projected: Option<Vec2>,
    project: impl Fn(DVec3) -> Option<Vec2>,
) -> Option<(Vec2, Option<[f32; 2]>)> {
    let p = projected?;
    match g.shape {
        GripShape::GizmoAxis(k) => {
            let d = gizmo_screen_axes(p, g.world, project)[k as usize % 3]?;
            Some((p + d * GIZMO_AXIS_PX, Some([d.x, -d.y])))
        }
        GripShape::GizmoPlane(k) => {
            let axes = gizmo_screen_axes(p, g.world, project);
            let (a, b) = gizmo_plane_axes(k);
            let (a, b) = (axes[a as usize]?, axes[b as usize]?);
            Some((p + (a + b) * 0.3 * GIZMO_AXIS_PX, None))
        }
        _ => {
            let dir = g.dir.and_then(|dir| Some(marker_direction(p, project(g.world + dir)?)));
            let s = marker_screen_position(Point::new(p.x, p.y), g.shape);
            Some((Vec2::new(s.x, s.y), dir))
        }
    }
}

/// Pixel distance from `cursor` to a marker; a gizmo arrow counts along its
/// whole shaft, not only at its tip.
fn marker_distance(shape: GripShape, screen: Vec2, dir: Option<[f32; 2]>, cursor: Point) -> f32 {
    let cursor = Vec2::new(cursor.x, cursor.y);
    match (shape, dir) {
        (GripShape::GizmoAxis(_), Some([dx, dy])) => {
            let d = Vec2::new(dx, -dy);
            let start = screen - d * (GIZMO_AXIS_PX - 14.0);
            let t = (cursor - start).dot(d).clamp(0.0, GIZMO_AXIS_PX - 14.0);
            cursor.distance(start + d * t)
        }
        _ => cursor.distance(screen),
    }
}

// ── Active drag state ─────────────────────────────────────────────────────

/// Stored on `OpenCADStudio` while a grip is being dragged.
#[derive(Clone, Debug)]
pub struct GripEdit {
    /// Handle of the entity being edited.
    pub handle: Handle,
    /// Index into the entity's grip list.
    pub grip_id: usize,
    /// World-space position of the grip when the drag started (ortho/polar base).
    pub origin_world: DVec3,
    /// Last world-space cursor position (needed for incremental delta on translate drags).
    pub last_world: DVec3,
    /// How cursor movement modifies the selected grip.
    pub mode: GripEditMode,
    /// Optional world-space drag axis.
    pub axis: Option<DVec3>,
    /// Every hot grip moved by this edit. A normal grip edit contains one target.
    pub targets: Vec<GripTarget>,
    /// Opposite corner and local width/height axes for rectangle corner resize.
    pub rectangle_frame: Option<(DVec3, DVec3, DVec3)>,
    /// Normal of the plane a move-gizmo square drags in.
    pub plane: Option<DVec3>,
    /// Move gizmo: offset from the gizmo centre to where its part was
    /// grabbed, taken on the first move so the drag does not jump.
    pub grab: Option<DVec3>,
    pub gizmo: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripEditMode {
    Stretch,
    Lengthen,
    Radius,
    ArcLength,
    RectangleWidth,
    RectangleHeight,
    RectangleResize,
    MoveParallel,
}

impl GripEditMode {
    /// Whether typed text belongs in the grip's single dynamic-input field.
    pub fn uses_scalar_dynamic_input(self) -> bool {
        matches!(
            self,
            Self::Lengthen
                | Self::Radius
                | Self::ArcLength
                | Self::RectangleWidth
                | Self::RectangleHeight
                | Self::MoveParallel
        )
    }
}

#[derive(Clone, Debug)]
pub struct GripTarget {
    pub handle: Handle,
    pub grip_id: usize,
    pub is_translate: bool,
    pub last_world: DVec3,
}

impl GripEdit {
    pub fn single(
        handle: Handle,
        grip_id: usize,
        is_translate: bool,
        world: DVec3,
    ) -> Self {
        Self {
            handle,
            grip_id,
            origin_world: world,
            last_world: world,
            mode: GripEditMode::Stretch,
            axis: None,
            targets: vec![GripTarget {
                handle,
                grip_id,
                is_translate,
                last_world: world,
            }],
            rectangle_frame: None,
            plane: None,
            grab: None,
            gizmo: false,
        }
    }

    pub fn lengthen(handle: Handle, grip_id: usize, world: DVec3) -> Self {
        let mut edit = Self::single(handle, grip_id, false, world);
        edit.mode = GripEditMode::Lengthen;
        edit
    }

    pub fn radius(handle: Handle, grip_id: usize, world: DVec3) -> Self {
        let mut edit = Self::single(handle, grip_id, false, world);
        edit.mode = GripEditMode::Radius;
        edit
    }

    pub fn arc_length(handle: Handle, grip_id: usize, world: DVec3) -> Self {
        let mut edit = Self::single(handle, grip_id, false, world);
        edit.mode = GripEditMode::ArcLength;
        edit
    }

    pub fn rectangle_width(handle: Handle, grip_id: usize, world: DVec3) -> Self {
        let mut edit = Self::single(handle, grip_id, false, world);
        edit.mode = GripEditMode::RectangleWidth;
        edit
    }

    pub fn rectangle_height(handle: Handle, grip_id: usize, world: DVec3) -> Self {
        let mut edit = Self::single(handle, grip_id, false, world);
        edit.mode = GripEditMode::RectangleHeight;
        edit
    }

    pub fn move_parallel(handle: Handle, grip_id: usize, world: DVec3) -> Self {
        let mut edit = Self::single(handle, grip_id, false, world);
        edit.mode = GripEditMode::MoveParallel;
        edit
    }

    pub fn rectangle_resize(
        handle: Handle,
        grip_id: usize,
        world: DVec3,
        opposite: DVec3,
        width_axis: DVec3,
        height_axis: DVec3,
    ) -> Self {
        let mut edit = Self::single(handle, grip_id, false, world);
        edit.mode = GripEditMode::RectangleResize;
        edit.rectangle_frame = Some((opposite, width_axis, height_axis));
        edit
    }
}

// ── Screen-space helpers ───────────────────────────────────────────────────

fn marker_direction(from: Vec2, to: Vec2) -> [f32; 2] {
    [to.x - from.x, from.y - to.y]
}

/// Project a slice of `GripDef`s to screen space.
/// Returns `(grip_id, screen_pos, is_midpoint, shape)` for each grip.
pub fn grips_to_screen(
    grips: &[GripDef],
    camera: &crate::scene::view::camera::Camera,
    bounds: Rectangle,
) -> Vec<(usize, Point, bool, GripShape, Option<[f32; 2]>)> {
    grips
        .iter()
        .map(|g| {
            // Project from the f64 grip position via the relative-to-eye path so
            // the grip stays glued to the wire at UTM-scale coordinates (an
            // `as_vec3` cast first would quantize it ~0.5 m off at high zoom).
            let project = |world: DVec3| camera.project(world, bounds);
            let (screen, dir) = match placed_marker(g, project(g.world), project) {
                Some((p, dir)) => (Point::new(bounds.x + p.x, bounds.y + p.y), dir),
                None => (Point::new(f32::NAN, f32::NAN), None),
            };
            (g.id, screen, g.is_midpoint, g.shape, dir)
        })
        .collect()
}

/// Paper-space variant: project grips using the 2-D linear `to_px` transform.
/// Parameters match the `to_px` closure in `paper_canvas.rs`.
pub fn grips_to_screen_paper(
    grips: &[GripDef],
    tx: f32,
    ty: f32,
    half_w: f32,
    half_h: f32,
    bounds: Rectangle,
) -> Vec<(usize, Point, bool, GripShape, Option<[f32; 2]>)> {
    let project = |world: DVec3| {
        Vec2::new(
            (world.x as f32 - tx + half_w) / (2.0 * half_w) * bounds.width,
            (ty + half_h - world.y as f32) / (2.0 * half_h) * bounds.height,
        )
    };
    grips
        .iter()
        .map(|g| {
            let projected = project(g.world);
            let screen = marker_screen_position(Point::new(projected.x, projected.y), g.shape);
            let dir = g.dir.map(|dir| {
                [
                    dir.x as f32 / (2.0 * half_w) * bounds.width,
                    dir.y as f32 / (2.0 * half_h) * bounds.height,
                ]
            });
            (g.id, screen, g.is_midpoint, g.shape, dir)
        })
        .collect()
}

/// Paper-space hit-test variant (mirrors `find_hit_grip` but uses 2-D projection).
pub fn find_hit_grip_paper(
    cursor: Point,
    grips: &[GripDef],
    tx: f32,
    ty: f32,
    half_w: f32,
    half_h: f32,
    bounds: Rectangle,
) -> Option<(usize, usize, bool, DVec3)> {
    let mut best_dist = GRIP_THRESHOLD_PX;
    let mut best: Option<(usize, usize, bool, DVec3)> = None;

    for (index, g) in grips.iter().enumerate() {
        let screen = marker_screen_position(
            Point::new(
                (g.world.x as f32 - tx + half_w) / (2.0 * half_w) * bounds.width,
                (ty + half_h - g.world.y as f32) / (2.0 * half_h) * bounds.height,
            ),
            g.shape,
        );
        let dx = screen.x - cursor.x;
        let dy = screen.y - cursor.y;
        let d = (dx * dx + dy * dy).sqrt();
        if d < best_dist {
            best_dist = d;
            best = Some((index, g.id, g.is_midpoint, g.world));
        }
    }
    best
}

/// Find the closest grip within `GRIP_THRESHOLD_PX` pixels of `cursor`.
/// Returns `(grip_id, is_translate, world_pos)` if found, else `None`.
pub fn find_hit_grip(
    cursor: Point,
    grips: &[GripDef],
    camera: &crate::scene::view::camera::Camera,
    bounds: Rectangle,
) -> Option<(usize, usize, bool, DVec3)> {
    let mut best_dist = GRIP_THRESHOLD_PX;
    let mut best: Option<(usize, usize, bool, DVec3)> = None;

    for (index, g) in grips.iter().enumerate() {
        let project = |world: DVec3| camera.project(world, bounds);
        let Some((screen, dir)) = placed_marker(g, project(g.world), project) else {
            continue;
        };
        let d = marker_distance(g.shape, screen, dir, cursor);
        if d < best_dist {
            best_dist = d;
            best = Some((index, g.id, g.is_midpoint, g.world));
        }
    }
    best
}

/// Project an f64 world point with an explicit relative-to-eye `(view_rot,
/// eye)` pair — the camera-less form of `Camera::project`. Used by the
/// in-viewport editing path, which supplies a *composed* model→screen view
/// (see `Scene::composed_viewport_view`) instead of a real camera. `pub(crate)`
/// so any other single-point world→screen projection (e.g. constraint-glyph
/// anchors) can reuse it instead of re-deriving the same NDC math.
pub(crate) fn project_rte(world: DVec3, view_rot: Mat4, eye: DVec3, bounds: Rectangle) -> Option<Vec2> {
    let rel = (world - eye).as_vec3();
    let clip = view_rot * rel.extend(1.0);
    if clip.w.abs() < 1e-9 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Vec2::new(
        (ndc.x * 0.5 + 0.5) * bounds.width,
        (0.5 - ndc.y * 0.5) * bounds.height,
    ))
}

/// Relative-to-eye variant of [`grips_to_screen`] taking a `(view_rot, eye)`
/// pair instead of a `Camera`, so the composed in-viewport view can project
/// model-space grips exactly like the GPU renders the viewport content.
pub fn grips_to_screen_rte(
    grips: &[GripDef],
    view_rot: Mat4,
    eye: DVec3,
    bounds: Rectangle,
) -> Vec<(usize, Point, bool, GripShape, Option<[f32; 2]>)> {
    grips
        .iter()
        .map(|g| {
            let project = |world: DVec3| project_rte(world, view_rot, eye, bounds);
            let (screen, dir) = match placed_marker(g, project(g.world), project) {
                Some((p, dir)) => (Point::new(bounds.x + p.x, bounds.y + p.y), dir),
                None => (Point::new(f32::NAN, f32::NAN), None),
            };
            (g.id, screen, g.is_midpoint, g.shape, dir)
        })
        .collect()
}

/// Relative-to-eye variant of [`find_hit_grip`] taking a `(view_rot, eye)`
/// pair instead of a `Camera`, for in-viewport (MSPACE) grip hit-testing.
pub fn find_hit_grip_rte(
    cursor: Point,
    grips: &[GripDef],
    view_rot: Mat4,
    eye: DVec3,
    bounds: Rectangle,
) -> Option<(usize, usize, bool, DVec3)> {
    let mut best_dist = GRIP_THRESHOLD_PX;
    let mut best: Option<(usize, usize, bool, DVec3)> = None;

    for (index, g) in grips.iter().enumerate() {
        let project = |world: DVec3| project_rte(world, view_rot, eye, bounds);
        let Some((screen, dir)) = placed_marker(g, project(g.world), project) else {
            continue;
        };
        let d = marker_distance(g.shape, screen, dir, cursor);
        if d < best_dist {
            best_dist = d;
            best = Some((index, g.id, g.is_midpoint, g.world));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::GripEditMode;

    #[test]
    fn scalar_dynamic_input_modes_include_move_parallel() {
        assert!(GripEditMode::MoveParallel.uses_scalar_dynamic_input());
        assert!(GripEditMode::Radius.uses_scalar_dynamic_input());
        assert!(!GripEditMode::Stretch.uses_scalar_dynamic_input());
        assert!(!GripEditMode::RectangleResize.uses_scalar_dynamic_input());
    }
}
