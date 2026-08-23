//! OpenCADStudio-style grip editing.

use acadrust::Handle;
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

fn marker_screen_position(mut point: Point, shape: GripShape) -> Point {
    if shape == GripShape::Dropdown {
        point.x += GRIP_DROPDOWN_OFFSET_X_PX;
        point.y += GRIP_DROPDOWN_OFFSET_Y_PX;
    }
    point
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripEditMode {
    Stretch,
    Lengthen,
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
        }
    }

    pub fn lengthen(handle: Handle, grip_id: usize, world: DVec3) -> Self {
        let mut edit = Self::single(handle, grip_id, false, world);
        edit.mode = GripEditMode::Lengthen;
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
            let projected = camera.project(g.world, bounds);
            let screen = match projected {
                Some(p) => Point::new(bounds.x + p.x, bounds.y + p.y),
                None => Point::new(f32::NAN, f32::NAN),
            };
            let dir = g.dir.and_then(|dir| {
                Some(marker_direction(
                    projected?,
                    camera.project(g.world + dir, bounds)?,
                ))
            });
            let screen = marker_screen_position(screen, g.shape);
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
        let Some(screen) = camera.project(g.world, bounds) else {
            continue;
        };
        let screen = marker_screen_position(Point::new(screen.x, screen.y), g.shape);
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

/// Project an f64 world point with an explicit relative-to-eye `(view_rot,
/// eye)` pair — the camera-less form of `Camera::project`. Used by the
/// in-viewport editing path, which supplies a *composed* model→screen view
/// (see `Scene::composed_viewport_view`) instead of a real camera.
fn project_rte(world: DVec3, view_rot: Mat4, eye: DVec3, bounds: Rectangle) -> Option<Vec2> {
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
            let projected = project_rte(g.world, view_rot, eye, bounds);
            let screen = match projected {
                Some(p) => Point::new(bounds.x + p.x, bounds.y + p.y),
                None => Point::new(f32::NAN, f32::NAN),
            };
            let dir = g.dir.and_then(|dir| {
                Some(marker_direction(
                    projected?,
                    project_rte(g.world + dir, view_rot, eye, bounds)?,
                ))
            });
            let screen = marker_screen_position(screen, g.shape);
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
        let Some(screen) = project_rte(g.world, view_rot, eye, bounds) else {
            continue;
        };
        let screen = marker_screen_position(Point::new(screen.x, screen.y), g.shape);
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
