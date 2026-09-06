//! Viewport overlay widgets.

use std::cell::RefCell;

use glam::{Mat4, Vec3};
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Element, Length, Point, Size, Theme};

use crate::app::Message;
use crate::app::settings::{CursorType, IsoPlane};
use crate::scene::model::object::GripShape;
use crate::scene::SelectionState;

use crate::snap::SnapType;
use std::sync::Arc;

/// Original crosshair geometry retained at the default setting values.
pub const CROSSHAIR_SQ: f32 = 7.5;
pub const CROSSHAIR_ARM: f32 = 60.0;
const DEFAULT_CURSOR_SIZE: i32 = 5;
const DEFAULT_PICK_BOX: i32 = 3;
const DEFAULT_PICK_APERTURE: f32 = 8.0;

/// Convert CURSORSIZE to a screen-space arm length while keeping the original
/// 60 px cursor at the default value and the full-viewport result at 100.
pub(crate) fn crosshair_arm_px(bounds: iced::Rectangle, value: i32) -> f32 {
    let value = value.clamp(1, 100);
    if value <= DEFAULT_CURSOR_SIZE {
        return CROSSHAIR_ARM * value as f32 / DEFAULT_CURSOR_SIZE as f32;
    }

    let full = bounds.width.hypot(bounds.height).max(CROSSHAIR_ARM);
    let scale = (value - DEFAULT_CURSOR_SIZE) as f32 / (100 - DEFAULT_CURSOR_SIZE) as f32;
    CROSSHAIR_ARM + (full - CROSSHAIR_ARM) * scale
}

/// Convert PICKBOX to the visible half-size while retaining the original
/// 15 x 15 px center box at the default value.
pub(crate) fn pick_box_half_px(value: i32) -> f32 {
    let value = value.clamp(0, 50);
    if value <= DEFAULT_PICK_BOX {
        return CROSSHAIR_SQ * value as f32 / DEFAULT_PICK_BOX as f32;
    }

    let scale = (value - DEFAULT_PICK_BOX) as f32 / (50 - DEFAULT_PICK_BOX) as f32;
    CROSSHAIR_SQ + (50.0 - CROSSHAIR_SQ) * scale
}

/// Convert PICKBOX to the real click/hover aperture. A zero setting hides the
/// box but retains the one-pixel minimum needed for direct hits.
pub(crate) fn pick_box_aperture_px(value: i32) -> f32 {
    let value = value.clamp(0, 50);
    let aperture = if value <= DEFAULT_PICK_BOX {
        DEFAULT_PICK_APERTURE * value as f32 / DEFAULT_PICK_BOX as f32
    } else {
        let scale = (value - DEFAULT_PICK_BOX) as f32 / (50 - DEFAULT_PICK_BOX) as f32;
        DEFAULT_PICK_APERTURE + (50.0 - DEFAULT_PICK_APERTURE) * scale
    };
    aperture.max(1.0)
}

#[derive(Clone, Copy)]
pub struct CrosshairOptions {
    pub size_percent: i32,
    pub pick_box: i32,
    pub cursor_type: CursorType,
    pub color: Option<[u8; 3]>,
    pub isometric: bool,
    pub iso_plane: IsoPlane,
    pub snap_angle_deg: f32,
    pub point_mode: bool,
}

/// Rendering style for the viewport grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridStyle {
    pub opacity: u8,
    pub bg_luminance: f32,
}

impl Default for GridStyle {
    fn default() -> Self {
        Self {
            opacity: 18,
            bg_luminance: 0.15,
        }
    }
}

/// Visual effect options for selection windows, crossings, entity highlights, and grips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionVisualOptions {
    pub area: bool,
    pub opacity: u8,
    pub window_color: u8,
    pub crossing_color: u8,
    pub highlight_color: u8,
    pub grip_size: f32,
    pub grip_color: u8,
    pub grip_hot: u8,
    pub grip_hover: u8,
}

impl Default for SelectionVisualOptions {
    fn default() -> Self {
        Self {
            area: true,
            opacity: 12,
            window_color: 0,
            crossing_color: 0,
            highlight_color: 0,
            grip_size: 5.0,
            grip_color: 0,
            grip_hot: 0,
            grip_hover: 0,
        }
    }
}

/// Standard CAD crossing selection color (emerald green: `#33B870` / RGB `0.20, 0.72, 0.44`).
pub const DEFAULT_CROSSING_COLOR: Color = Color {
    r: 0.20,
    g: 0.72,
    b: 0.44,
    a: 1.0,
};

/// Standard CAD window selection color (cobalt blue: `#3370B8` / RGB `0.20, 0.44, 0.72`).
pub const DEFAULT_WINDOW_COLOR: Color = Color {
    r: 0.20,
    g: 0.44,
    b: 0.72,
    a: 1.0,
};

/// Returns the curated (crossing_color, window_color) pair tailored for a specific theme.
/// Crossing is always a theme-harmonious green/teal variant, and Window is always a theme-harmonious blue/cyan variant.
pub fn theme_selection_colors(theme: &Theme) -> (Color, Color) {
    match theme {
        Theme::Dark => (
            DEFAULT_CROSSING_COLOR,
            DEFAULT_WINDOW_COLOR,
        ),
        Theme::Light => (
            Color::from_rgb(0.12, 0.52, 0.30), // Engineering Forest Green
            Color::from_rgb(0.13, 0.40, 0.70), // Engineering Blueprint Blue
        ),
        Theme::Oxocarbon => (
            Color::from_rgb(0.26, 0.75, 0.40), // Oxocarbon Green (#42be65)
            Color::from_rgb(0.20, 0.69, 1.00), // Oxocarbon Vibrant Cyan (#33b1ff)
        ),
        Theme::Dracula => (
            Color::from_rgb(0.31, 0.98, 0.48), // Dracula Neon Green (#50fa7b)
            Color::from_rgb(0.54, 0.91, 0.99), // Dracula Vibrant Cyan (#8be9fd)
        ),
        Theme::Nord => (
            Color::from_rgb(0.64, 0.75, 0.55), // Nord Aurora Green (#a3be8c)
            Color::from_rgb(0.53, 0.75, 0.82), // Nord Frost Cyan (#88c0d0)
        ),
        Theme::GruvboxDark => (
            Color::from_rgb(0.72, 0.73, 0.15), // Gruvbox Bright Green (#b8bb26)
            Color::from_rgb(0.51, 0.65, 0.60), // Gruvbox Blue/Aqua (#83a598)
        ),
        Theme::GruvboxLight => (
            Color::from_rgb(0.47, 0.46, 0.05), // Gruvbox Dark Olive Green (#78750d)
            Color::from_rgb(0.03, 0.40, 0.47), // Gruvbox Deep Teal (#076678)
        ),
        Theme::SolarizedDark => (
            Color::from_rgb(0.52, 0.60, 0.00), // Solarized Green (#859900)
            Color::from_rgb(0.15, 0.55, 0.82), // Solarized Blue (#268bd2)
        ),
        Theme::SolarizedLight => (
            Color::from_rgb(0.44, 0.53, 0.00), // Solarized Deep Green (#708700)
            Color::from_rgb(0.12, 0.47, 0.72), // Solarized Deep Blue (#1f78b8)
        ),
        Theme::TokyoNight | Theme::TokyoNightStorm => (
            Color::from_rgb(0.45, 0.85, 0.79), // Tokyo Night Teal Green (#73daca)
            Color::from_rgb(0.48, 0.64, 0.97), // Tokyo Night Electric Blue (#7aa2f7)
        ),
        Theme::TokyoNightLight => (
            Color::from_rgb(0.22, 0.45, 0.37), // Tokyo Night Dark Teal (#38735e)
            Color::from_rgb(0.20, 0.35, 0.60), // Tokyo Night Deep Blue (#335999)
        ),
        Theme::KanagawaWave => (
            Color::from_rgb(0.60, 0.73, 0.42), // Spring Green (#98bb6c)
            Color::from_rgb(0.49, 0.61, 0.85), // Crystal Blue (#7e9cd8)
        ),
        Theme::KanagawaDragon => (
            Color::from_rgb(0.53, 0.66, 0.53), // Dragon Green (#87a987)
            Color::from_rgb(0.40, 0.52, 0.58), // Dragon Blue (#668594)
        ),
        Theme::KanagawaLotus => (
            Color::from_rgb(0.38, 0.49, 0.25), // Lotus Deep Green (#617d40)
            Color::from_rgb(0.24, 0.38, 0.53), // Lotus Deep Blue (#3d6187)
        ),
        Theme::CatppuccinMocha => (
            Color::from_rgb(0.65, 0.89, 0.63), // Mocha Green (#a6e3a1)
            Color::from_rgb(0.54, 0.71, 0.98), // Mocha Sapphire (#89b4fa)
        ),
        Theme::CatppuccinMacchiato => (
            Color::from_rgb(0.65, 0.85, 0.58), // Macchiato Green (#a6da95)
            Color::from_rgb(0.54, 0.68, 0.96), // Macchiato Blue (#8aadf4)
        ),
        Theme::CatppuccinFrappe => (
            Color::from_rgb(0.65, 0.82, 0.54), // Frappe Green (#a6d189)
            Color::from_rgb(0.55, 0.67, 0.93), // Frappe Blue (#8caaee)
        ),
        Theme::CatppuccinLatte => (
            Color::from_rgb(0.25, 0.63, 0.17), // Latte Green (#40a02b)
            Color::from_rgb(0.12, 0.40, 0.96), // Latte Blue (#1e66f5)
        ),
        Theme::Moonfly => (
            Color::from_rgb(0.55, 0.78, 0.37), // Moonfly Lime Green (#8cc85f)
            Color::from_rgb(0.50, 0.63, 1.00), // Moonfly Sky Blue (#80a0ff)
        ),
        Theme::Nightfly => (
            Color::from_rgb(0.13, 0.85, 0.43), // Nightfly Emerald (#21d96e)
            Color::from_rgb(0.51, 0.67, 1.00), // Nightfly Electric Blue (#82aaff)
        ),
        Theme::Ferra => (
            Color::from_rgb(0.69, 0.87, 0.63), // Ferra Sage Green (#b1dda1)
            Color::from_rgb(0.69, 0.84, 0.97), // Ferra Ice Blue (#b1d5f7)
        ),
        _ => (
            DEFAULT_CROSSING_COLOR,
            DEFAULT_WINDOW_COLOR,
        ),
    }
}

/// Selection colors for light surfaces (paper space sheet or user-configured light model space background).
/// Uses deep-contrast pairs for the six light themes and the shared engineering pair (#1F854D / #2166B3)
/// for dark themes rendered on a light canvas.
pub fn light_canvas_color(crossing: bool, theme: &Theme) -> Color {
    let (crossing_color, window_color) = match theme {
        Theme::Light => (
            Color::from_rgb8(0x1F, 0x85, 0x4D), // Engineering Forest Green (#1F854D)
            Color::from_rgb8(0x21, 0x66, 0xB3), // Engineering Blueprint Blue (#2166B3)
        ),
        Theme::GruvboxLight => (
            Color::from_rgb8(0x78, 0x75, 0x0D), // Gruvbox Dark Olive Green (#78750d)
            Color::from_rgb8(0x07, 0x66, 0x78), // Gruvbox Deep Teal (#076678)
        ),
        Theme::SolarizedLight => (
            Color::from_rgb8(0x70, 0x87, 0x00), // Solarized Deep Green (#708700)
            Color::from_rgb8(0x1F, 0x78, 0xB8), // Solarized Deep Blue (#1f78b8)
        ),
        Theme::TokyoNightLight => (
            Color::from_rgb8(0x38, 0x73, 0x5E), // Tokyo Night Dark Teal (#38735e)
            Color::from_rgb8(0x33, 0x59, 0x99), // Tokyo Night Deep Blue (#335999)
        ),
        Theme::KanagawaLotus => (
            Color::from_rgb8(0x61, 0x7D, 0x40), // Lotus Deep Green (#617d40)
            Color::from_rgb8(0x3D, 0x61, 0x87), // Lotus Deep Blue (#3d6187)
        ),
        Theme::CatppuccinLatte => (
            Color::from_rgb8(0x40, 0xA0, 0x2B), // Latte Green (#40a02b)
            Color::from_rgb8(0x1E, 0x66, 0xF5), // Latte Blue (#1e66f5)
        ),
        Theme::Dark
        | Theme::Oxocarbon
        | Theme::Dracula
        | Theme::Nord
        | Theme::GruvboxDark
        | Theme::SolarizedDark
        | Theme::TokyoNight
        | Theme::TokyoNightStorm
        | Theme::KanagawaWave
        | Theme::KanagawaDragon
        | Theme::CatppuccinMocha
        | Theme::CatppuccinMacchiato
        | Theme::CatppuccinFrappe
        | Theme::Moonfly
        | Theme::Nightfly
        | Theme::Ferra
        | Theme::Custom(_) => (
            Color::from_rgb8(0x1F, 0x85, 0x4D), // Shared engineering crossing (#1F854D)
            Color::from_rgb8(0x21, 0x66, 0xB3), // Shared engineering window (#2166B3)
        ),
    };
    if crossing {
        crossing_color
    } else {
        window_color
    }
}

/// Calculates the fill opacity for selection marquees.
/// The 1.35× scale and 0.45 ceiling on light canvases compensate for the reduced
/// perceived contrast of translucent fills on light surfaces, ensuring the fill
/// remains perceptible without washing out underlying drawing geometry.
pub fn selection_fill_alpha(user_opacity_percent: f32, canvas_light: bool) -> f32 {
    let base = (user_opacity_percent / 100.0).clamp(0.0, 1.0);
    if canvas_light {
        (base * 1.35).clamp(0.0, 0.45)
    } else {
        base
    }
}

/// Resolve the base color for selection marquee / polygon.
/// Resolves in this priority order:
/// 1. User ACI override (if custom > 0)
/// 2. Light-canvas palette (if canvas background is light)
/// 3. Theme curated palette (for dark/normal canvas)
pub fn resolve_selection_base_color(
    crossing: bool,
    theme: &Theme,
    visual: &SelectionVisualOptions,
    canvas_bg: [f32; 4],
) -> Color {
    let custom = if crossing {
        visual.crossing_color
    } else {
        visual.window_color
    };
    // ACI 0 (BYBLOCK) and 256 (BYLAYER) are not explicit overrides;
    // the sysvar uses 0 as the unset sentinel; valid user picks are 1..=255.
    if custom > 0 {
        if let Some((r, g, b)) = acadrust::types::aci_table::aci_to_rgb(custom) {
            return Color::from_rgb8(r, g, b);
        }
    }
    if crate::ui::style::common::canvas_is_light(canvas_bg) {
        return light_canvas_color(crossing, theme);
    }
    let (theme_crossing, theme_window) = theme_selection_colors(theme);
    if crossing {
        theme_crossing
    } else {
        theme_window
    }
}

// ── Grip marker data ──────────────────────────────────────────────────────

/// Describes one grip to be drawn in the viewport overlay.
#[derive(Clone, Debug)]
pub struct GripMarker {
    /// Screen-space position (viewport-relative pixels).
    pub pos: Point,
    /// Explicit marker shape.
    pub shape: GripShape,
    /// True → grip is currently being dragged (drawn filled red).
    pub is_hot: bool,
    /// True → pointer is over this grip (drawn with the hover fill).
    pub is_hovered: bool,
    /// Screen-plane direction with positive Y pointing up.
    pub dir: Option<[f32; 2]>,
}

// ── Grid display params ───────────────────────────────────────────────────

/// Passed to the canvas when the GRID display is active.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridParams {
    /// Rotation-only view-projection (Camera::view_proj_rte). Grid points are
    /// made relative to `eye` in f64 before projecting, so the grid stays
    /// precise / jitter-free at UTM-scale absolute coordinates.
    pub view_rot: Mat4,
    /// Camera eye in absolute world f64 — subtracted from each grid point.
    pub eye: glam::DVec3,
    pub bounds: iced::Rectangle,
    /// Adaptive world-space spacing derived from camera zoom at its pivot.
    /// Rotation does not affect this value, so orbiting cannot rescale the grid.
    pub step: f32,
    /// Grid origin in absolute world f64 and the active UCS axis directions.
    /// The grid always lies on the active UCS XY plane. Plain WCS passes
    /// `(ZERO, X, Y, Z)`.
    pub origin: glam::DVec3,
    pub axes: (Vec3, Vec3, Vec3),
    /// WCS XY drawing limits. When present, grid lines stop at this rectangle
    /// instead of extending across the full viewport.
    pub limits: Option<(glam::DVec2, glam::DVec2)>,
}

/// Pure result of grid projection — segments in canvas-local coordinates plus
/// the axis extent (in world units along the active UCS axes) that the wrapper
/// uses to size the coloured UCS axes overlay. Returned from `grid_segments` so
/// the renderer-free geometry construction can be unit-tested and benchmarked
/// without an iced `Renderer` (Mission #1, 2026-08-26 bench-first plan).
pub(crate) struct GridGeometry {
    pub segments: Vec<(Point, Point)>,
    pub axis_extent: f32,
}

impl GridGeometry {
    /// Empty geometry — no segments drawn, axes suppressed. Returned by the
    /// early-exit branches of `grid_segments` (zero-sized bounds, no visible
    /// samples, non-finite step) so the caller never needs to special-case
    /// the `None` path.
    fn empty() -> Self {
        Self { segments: Vec::new(), axis_extent: 0.0 }
    }
}

/// Cache key for the grid overlay. Identical `GridParams` for every pane plus
/// identical overlay `bounds` ⇒ byte-identical grid geometry; that is the
/// invariant the key encodes.
///
/// Bounds are part of the key, not the only key: a tile layout can pan/zoom
/// inside a single bounds rect, so bounds-only reuse (iced's
/// `geometry::Cache`) would serve a stale grid. The full per-pane
/// `GridParams` set is required for correctness.
///
/// Added 2026-08-26 by Mission #1 (grid overlay cache, Tier 1 #1).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GridKey {
    pub grids: Vec<GridParams>,
    pub bounds: iced::Rectangle,
    pub style: GridStyle,
}

impl GridKey {
    /// Build a key from the per-pane `GridParams`, overlay bounds, and grid style.
    pub(crate) fn from_grids(grids: &[GridParams], bounds: iced::Rectangle, style: GridStyle) -> Self {
        Self { grids: grids.to_vec(), bounds, style }
    }
}

/// Cache hit decision: `true` iff `cached` is `Some` and structurally equal to
/// `new`. Reference-based to avoid moving the (potentially large) `Vec` of
/// per-pane params; the caller borrows from `RefCell<Option<GridKey>>` on
/// both sides.
pub(crate) fn should_reuse(cached: Option<&GridKey>, new: &GridKey) -> bool {
    match cached {
        Some(old) => old == new,
        None => false,
    }
}

/// `Program::State` for `GridCanvas`. Stores the `GridKey` of the geometry
/// currently in the cache and the `canvas::Cache` itself.
///
/// `RefCell<Option<GridKey>>` because `Program::draw` takes `&self`; the key
/// is updated on every frame regardless of hit/miss. The `canvas::Cache`
/// provides an Arc-clone hit path when the iced-level bounds match, and we
/// use `clear()` on a params-key miss to force a real rebuild even if the
/// bounds happen to be unchanged (e.g. a pan within the same canvas size).
///
/// Added 2026-08-26 by Mission #1 (grid overlay cache, Tier 1 #1).
#[derive(Default)]
pub(crate) struct GridCanvasState {
    pub key: RefCell<Option<GridKey>>,
    pub cache: canvas::Cache<iced::Renderer>,
}

/// Compute the adaptive grid step size (world units) from camera zoom.
///
/// Returns the smallest power-of-5 multiple of 1.0 that places grid lines at
/// least `MIN_GRID_PX` pixels apart at the camera pivot. Both orthographic and
/// perspective cameras have the same vertical scale there. Camera rotation is
/// intentionally absent so orbiting cannot rescale the visible grid or snap.
/// Clip the screen segment `p0`→`p1` to `bounds` (Liang–Barsky), returning the
/// visible part, or `None` when it misses entirely.
///
/// Use this before stroking anything **dashed**. A dash pattern is measured in
/// pixels, so the tessellator emits a quad every few pixels and a path's cost
/// scales with its screen length. Guides are built from projected world points,
/// which land millions of pixels away for a far or huge entity — millions of
/// quads, gigabytes, and the process dies (#406). It is tempting to assume the
/// canvas clips this for us; it does not — clipping happens after tessellation,
/// so the quads are all built first. Clip up front instead.
///
/// The box is padded by one dash period's worth so a guide still visibly runs
/// off the edge rather than stopping flush against it.
fn clip_seg(p0: Point, p1: Point, bounds: iced::Rectangle) -> Option<(Point, Point)> {
    const PAD: f32 = 16.0;
    let (xmin, ymin) = (-PAD, -PAD);
    let (xmax, ymax) = (bounds.width + PAD, bounds.height + PAD);
    let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-dx, p0.x - xmin), (dx, xmax - p0.x), (-dy, p0.y - ymin), (dy, ymax - p0.y)] {
        if p == 0.0 {
            // Parallel to this edge: outside it means the whole segment is out.
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return None;
            }
            t0 = t0.max(r);
        } else {
            if r < t0 {
                return None;
            }
            t1 = t1.min(r);
        }
    }
    Some((
        Point::new(p0.x + t0 * dx, p0.y + t0 * dy),
        Point::new(p0.x + t1 * dx, p0.y + t1 * dy),
    ))
}

pub fn compute_grid_step(distance: f32, fov_y: f32, bounds: iced::Rectangle) -> f32 {
    let half_height = distance * (fov_y * 0.5).tan();
    if !half_height.is_finite() || half_height <= 1e-9 || bounds.height <= 0.0 {
        return 1.0;
    }
    let px_per_unit = bounds.height / (2.0 * half_height);
    let mut s = 1.0_f32;
    while s * px_per_unit < MIN_GRID_PX {
        s *= 5.0;
        if s > 1e9 {
            return 1.0;
        }
    }
    s
}

/// Parameters for the screen-space UCS icon drawn in the viewport corner.
pub struct UcsIconParams {
    /// View-projection matrix used to project world axis directions to screen.
    pub view_proj: Mat4,
    /// Viewport bounds (used for NDC → pixel conversion).
    pub bounds: iced::Rectangle,
    /// The active UCS axis directions in world space (X, Y, Z). Plain WCS is
    /// `(Vec3::X, Vec3::Y, Vec3::Z)`; a UCS rotates the tripod to match.
    pub axes: (Vec3, Vec3, Vec3),
    /// Absolute screen position of the UCS origin, when the icon should track
    /// it (UCSICON ORigin). `None` → pin to the corner. The tripod still snaps
    /// back to the corner if this point falls outside the viewport bounds.
    pub origin_screen: Option<Point>,
    /// Cursor is over the icon — brighten the tripod (hover affordance).
    pub hover: bool,
    /// Icon is selected — draw draggable grip squares at the origin and tips.
    pub selected: bool,
}

// ── Selection overlay ───────────────────────────────────────────────────

/// An acquired OST tracking point with its screen position.
#[derive(Clone, Debug)]
pub struct OstTrackPoint {
    pub screen: Point,
}

pub fn grid_overlay<'a>(
    grid: Vec<GridParams>,
    style: GridStyle,
) -> Element<'a, Message> {
    canvas(GridCanvas { grid, style })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

struct GridCanvas {
    grid: Vec<GridParams>,
    style: GridStyle,
}

impl canvas::Program<Message> for GridCanvas {
    type State = GridCanvasState;

    fn draw(
        &self,
        state: &GridCanvasState,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let key = GridKey::from_grids(&self.grid, bounds, self.style);

        // Hit check: same params, same bounds, same style ⇒ the cached geometry is still
        // valid. The key includes bounds and style, so a `should_reuse` match implies
        // both are equal, and the fork's `draw_with_bounds` will return the
        // cached `Arc` clone (essentially free).
        let hit = should_reuse(state.key.borrow().as_ref(), &key);

        let geometry = if hit {
            // No-op closure: the fork short-circuits on bounds match and
            // returns the cached geometry without invoking the closure.
            state.cache.draw_with_bounds(renderer, bounds, |_frame| {})
        } else {
            // Params, style, or bounds changed. Clear so the closure runs even when
            // bounds happen to match the previously-cached frame.
            state.cache.clear();
            state.cache.draw_with_bounds(renderer, bounds, |frame| {
                for g in &self.grid {
                    let gb = g.bounds;
                    let cx0 = gb.x.max(0.0);
                    let cy0 = gb.y.max(0.0);
                    let cx1 = (gb.x + gb.width).min(bounds.width);
                    let cy1 = (gb.y + gb.height).min(bounds.height);
                    if cx1 <= cx0 || cy1 <= cy0 {
                        continue;
                    }
                    let clip = iced::Rectangle {
                        x: cx0,
                        y: cy0,
                        width: cx1 - cx0,
                        height: cy1 - cy0,
                    };
                    frame.with_clip(clip, |f| {
                        draw_grid(
                            f,
                            g.view_rot,
                            g.eye,
                            gb,
                            g.step,
                            g.origin,
                            g.axes,
                            g.limits,
                            self.style,
                        )
                    });
                }
            })
        };

        // Update the stored key.
        *state.key.borrow_mut() = Some(key);

        vec![geometry]
    }
}

pub fn selection_overlay<'a>(
    selection: Arc<RefCell<SelectionState>>,
    snap: Option<(Point, SnapType)>,
    snap_ext_base: Option<Point>,
    snap_ext_base2: Option<Point>,
    grips: Vec<GripMarker>,
    control_polygon: Option<(Vec<Point>, bool)>,
    grip_clip: Option<iced::Rectangle>,
    ucs_icons: Vec<UcsIconParams>,
    ost_points: Vec<OstTrackPoint>,
    otrack_line: Option<(Point, Point)>,
    parallel_ref_marker: Option<Point>,
    show_viewcube: bool,
    dividers: Vec<iced::Rectangle>,
    pane_move_rect: Option<iced::Rectangle>,
    pane_drop_rect: Option<iced::Rectangle>,
    pan_mode: bool,
    suppressed: bool,
    hover_locked: bool,
    crosshair_bg: [f32; 4],
    crosshair: CrosshairOptions,
    selection_visual: SelectionVisualOptions,
) -> Element<'a, Message> {
    canvas(SelectionCanvas {
        selection,
        snap,
        snap_ext_base,
        snap_ext_base2,
        grips,
        control_polygon,
        grip_clip,
        ucs_icons,
        ost_points,
        otrack_line,
        parallel_ref_marker,
        show_viewcube,
        dividers,
        pane_move_rect,
        pane_drop_rect,
        pan_mode,
        suppressed,
        hover_locked,
        crosshair_bg,
        crosshair,
        selection_visual,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct SelectionCanvas {
    selection: Arc<RefCell<SelectionState>>,
    snap: Option<(Point, SnapType)>,
    /// Screen position of the endpoint an active Extension snap extends from,
    /// so the dashed extension guide line can be drawn back to it. (#238)
    snap_ext_base: Option<Point>,
    /// Second extension-guide base, present only for an extended intersection so
    /// both crossing extensions stay drawn when the crossing is caught. (#247, #259)
    snap_ext_base2: Option<Point>,
    grips: Vec<GripMarker>,
    /// Selected spline control polygon and closed state.
    control_polygon: Option<(Vec<Point>, bool)>,
    /// Active 3D pane / floating viewport rectangle. Grip markers are clipped
    /// here so orbiting cannot leak them into paper space or adjacent panes.
    grip_clip: Option<iced::Rectangle>,
    /// One UCS icon per Model pane (each viewport shows its own at its origin);
    /// a single entry for paper / floating-viewport. Only the active pane's
    /// entry carries hover/selected (grips).
    ucs_icons: Vec<UcsIconParams>,
    ost_points: Vec<OstTrackPoint>,
    /// Active OTRACK alignment: (acquired tracking point, locked cursor), both
    /// in screen space. Drawn as a dashed guide extended a little past the
    /// cursor so the extension / tracking line the user snapped to is visible.
    /// (#219)
    otrack_line: Option<(Point, Point)>,
    /// The acquired Parallel-snap reference point (screen), marked with a small
    /// ∥ glyph so the user sees which line is the parallel reference. (#277)
    parallel_ref_marker: Option<Point>,
    show_viewcube: bool,
    /// Divider bars (pixel rects, canvas-relative) between Model panes — drawn
    /// as filled lines and used to suppress the crosshair over a divider.
    dividers: Vec<iced::Rectangle>,
    /// When a pane move is armed (drag handle pressed), the source pane's rect
    /// (px) — dimmed, with a ghost card dragged along under the cursor.
    pane_move_rect: Option<iced::Rectangle>,
    /// The pane under the cursor during a pane move (drop target), highlighted.
    pane_drop_rect: Option<iced::Rectangle>,
    /// Interactive PAN mode: the crosshair is hidden and the cursor becomes a
    /// hand so the viewport reads as a draggable surface.
    pan_mode: bool,
    /// A ribbon dropdown (or similar overlay) is open over the viewport. The
    /// crosshair is not drawn and the OS cursor is shown normally so the panel
    /// is usable instead of the cursor vanishing over it. (#227)
    suppressed: bool,
    /// The entity under the crosshair is on a locked layer — draw a small lock
    /// badge by the cursor so the user knows it can't be edited.
    hover_locked: bool,
    /// Background of the active drawing space. Crosshair contrast follows this
    /// rather than the UI theme, which may be light over a dark model viewport.
    crosshair_bg: [f32; 4],
    crosshair: CrosshairOptions,
    selection_visual: SelectionVisualOptions,
}

fn draw_grip_marker(
    frame: &mut canvas::Frame,
    grip: &GripMarker,
    theme: &Theme,
    visual: &SelectionVisualOptions,
) {
    let sp = grip.pos;
    let h = visual.grip_size.clamp(1.0, 25.0);
    let path = match grip.shape {
        GripShape::Square => canvas::Path::rectangle(
            Point::new(sp.x - h, sp.y - h),
            Size::new(h * 2.0, h * 2.0),
        ),
        GripShape::Rectangle => {
            // Mid-segment stretch handle: small box, longer along the segment
            // direction so the affordance reads as "stretch perpendicular".
            let half_long = h * 1.4;
            let half_short = h * 0.7;
            let (cos_t, sin_t) = match grip.dir {
                Some([dx, dy]) if (dx * dx + dy * dy) > 1e-12 => {
                    let n = (dx * dx + dy * dy).sqrt();
                    (dx / n, -dy / n)
                }
                _ => (1.0, 0.0),
            };
            let ax = (cos_t * half_long, sin_t * half_long);
            let ay = (-sin_t * half_short, cos_t * half_short);
            canvas::Path::new(|b| {
                b.move_to(Point::new(sp.x + ax.0 + ay.0, sp.y + ax.1 + ay.1));
                b.line_to(Point::new(sp.x + ax.0 - ay.0, sp.y + ax.1 - ay.1));
                b.line_to(Point::new(sp.x - ax.0 - ay.0, sp.y - ax.1 - ay.1));
                b.line_to(Point::new(sp.x - ax.0 + ay.0, sp.y - ax.1 + ay.1));
                b.close();
            })
        }
        GripShape::Triangle => canvas::Path::new(|b| {
            let (forward_x, forward_y) = match grip.dir {
                Some([dx, dy]) if (dx * dx + dy * dy) > 1e-12 => {
                    let length = (dx * dx + dy * dy).sqrt();
                    (dx / length, -dy / length)
                }
                _ => (0.0, -1.0),
            };
            let side_x = -forward_y;
            let side_y = forward_x;
            let tip = Point::new(sp.x + forward_x * h, sp.y + forward_y * h);
            let base = Point::new(sp.x - forward_x * h, sp.y - forward_y * h);
            b.move_to(tip);
            b.line_to(Point::new(base.x + side_x * h, base.y + side_y * h));
            b.line_to(Point::new(base.x - side_x * h, base.y - side_y * h));
            b.close();
        }),
        GripShape::Circle => canvas::Path::circle(Point::new(sp.x, sp.y), h),
        GripShape::Dropdown => canvas::Path::new(|b| {
            b.move_to(Point::new(sp.x - h, sp.y - h * 0.5));
            b.line_to(Point::new(sp.x + h, sp.y - h * 0.5));
            b.line_to(Point::new(sp.x, sp.y + h));
            b.close();
        }),
    };

    if grip.is_hot {
        let hot_color = if visual.grip_hot > 0 {
            if let Some((r, g, b)) = acadrust::types::aci_table::aci_to_rgb(visual.grip_hot) {
                Color::from_rgb8(r, g, b)
            } else {
                theme.palette().danger.base.color
            }
        } else {
            theme.palette().danger.base.color
        };
        frame.fill(&path, hot_color);
    } else if grip.is_hovered {
        let pair = theme.palette().primary.strong;
        let hover_color = if visual.grip_hover > 0 {
            if let Some((r, g, b)) = acadrust::types::aci_table::aci_to_rgb(visual.grip_hover) {
                Color::from_rgb8(r, g, b)
            } else {
                pair.color
            }
        } else {
            pair.color
        };
        frame.fill(&path, hover_color);
        frame.stroke(
            &path,
            canvas::Stroke {
                width: 1.5,
                style: canvas::Style::Solid(pair.text),
                ..Default::default()
            },
        );
    } else {
        let palette = theme.palette();
        let color = if visual.grip_color > 0 {
            if let Some((r, g, b)) = acadrust::types::aci_table::aci_to_rgb(visual.grip_color) {
                Color::from_rgb8(r, g, b)
            } else {
                palette.primary.base.color
            }
        } else {
            palette.primary.base.color
        };
        let fill = if grip.shape == GripShape::Dropdown {
            color
        } else {
            palette.background.base.color.scale_alpha(0.7)
        };
        frame.fill(&path, fill);
        frame.stroke(
            &path,
            canvas::Stroke {
                width: 1.5,
                style: canvas::Style::Solid(color),
                ..Default::default()
            },
        );
    }
}

impl SelectionCanvas {
    /// True when the cursor sits on a Model-pane divider (within a few px), so
    /// `draw` can suppress the CAD crosshair there. The resize cursor itself is
    /// supplied by the input pane_grid layered above.
    fn divider_under(&self, cursor: mouse::Cursor, bounds: iced::Rectangle) -> bool {
        const TOL_PX: f32 = 3.0;
        let Some(pos) = cursor.position_in(bounds) else {
            return false;
        };
        self.dividers.iter().any(|d| {
            pos.x >= d.x - TOL_PX
                && pos.x <= d.x + d.width + TOL_PX
                && pos.y >= d.y - TOL_PX
                && pos.y <= d.y + d.height + TOL_PX
        })
    }
}

impl canvas::Program<Message> for SelectionCanvas {
    type State = ();

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: iced::Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // A dropdown/overlay is open over the viewport — show the normal OS
        // cursor over the whole canvas instead of hiding it for the crosshair,
        // so the cursor doesn't vanish while using the panel. (#227)
        if self.suppressed {
            return mouse::Interaction::default();
        }
        // PAN mode owns the whole viewport: an open hand when hovering, a
        // closed hand while dragging.
        if self.pan_mode && cursor.is_over(bounds) {
            return if self.selection.borrow().middle_down {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::Grab
            };
        }
        if self.show_viewcube {
            if let Some(pos) = cursor.position_in(bounds) {
                use crate::scene::{VIEWCUBE_PAD, VIEWCUBE_REGION_PX};
                let vc_x = bounds.width - VIEWCUBE_REGION_PX - VIEWCUBE_PAD;
                let vc_y = VIEWCUBE_PAD;
                if pos.x >= vc_x
                    && pos.x <= vc_x + VIEWCUBE_REGION_PX
                    && pos.y >= vc_y
                    && pos.y <= vc_y + VIEWCUBE_REGION_PX
                {
                    return mouse::Interaction::None;
                }
            }
        }
        // The resize cursor over a divider is supplied by the input pane_grid
        // layered above this overlay; `draw` only suppresses the CAD crosshair
        // there (see `divider_under`).
        // Over the viewport (no divider, no viewcube): hide the system
        // cursor entirely. `Interaction::None` would let the stack fall
        // through to a sibling — `Hidden` is the explicit "no cursor"
        // signal that actually suppresses the OS arrow.
        if cursor.is_over(bounds) && self.crosshair.cursor_type == CursorType::Crosshair {
            mouse::Interaction::Hidden
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // ── Pane dividers (Model-space tiled layout) ──────────────────────
        // Filled bars in the pane_grid spacing gaps, so adjacent panes read as
        // distinct viewports. Drawn first so all other overlays sit on top.
        if !self.dividers.is_empty() {
            let divider = theme.palette().background.neutral.color;
            for d in &self.dividers {
                let bar = canvas::Path::rectangle(
                    Point::new(d.x, d.y),
                    iced::Size::new(d.width.max(1.0), d.height.max(1.0)),
                );
                frame.fill(&bar, divider);
            }
        }

        // ── Pane move (drag-to-swap) ──────────────────────────────────────
        // While armed: dim the lifted source pane, highlight the drop target
        // under the cursor, and drag a translucent ghost card along the cursor
        // so the pane is visibly "moving".
        if let Some(src) = self.pane_move_rect {
            let accent = theme.palette().primary.base.color;
            // Source pane: dimmed + dashed-feel outline (it has been lifted).
            let src_path =
                canvas::Path::rectangle(Point::new(src.x, src.y), iced::Size::new(src.width, src.height));
            frame.fill(
                &src_path,
                theme.palette().background.strong.color.scale_alpha(0.28),
            );
            frame.stroke(
                &src_path,
                canvas::Stroke {
                    width: 1.5,
                    style: canvas::Style::Solid(Color { a: 0.6, ..accent }),
                    ..Default::default()
                },
            );
            // Drop target pane: bright fill + outline.
            if let Some(dst) = self.pane_drop_rect {
                let dst_path = canvas::Path::rectangle(
                    Point::new(dst.x, dst.y),
                    iced::Size::new(dst.width, dst.height),
                );
                frame.fill(&dst_path, Color { a: 0.16, ..accent });
                frame.stroke(
                    &dst_path,
                    canvas::Stroke {
                        width: 2.5,
                        style: canvas::Style::Solid(accent),
                        ..Default::default()
                    },
                );
            }
            // Ghost card dragged under the cursor — a 0.32× preview of the
            // source pane, centred on the cursor.
            if let Some(c) = self.selection.borrow().last_move_pos {
                let gw = (src.width * 0.32).clamp(60.0, 280.0);
                let gh = (src.height * 0.32).clamp(40.0, 200.0);
                let g = canvas::Path::rectangle(
                    Point::new(c.x - gw * 0.5, c.y - gh * 0.5),
                    iced::Size::new(gw, gh),
                );
                frame.fill(&g, Color { a: 0.30, ..accent });
                frame.stroke(
                    &g,
                    canvas::Stroke {
                        width: 2.0,
                        style: canvas::Style::Solid(Color { a: 0.95, ..accent }),
                        ..Default::default()
                    },
                );
            }
        }


        // Draw a selection marquee (green crossing / blue window) as a filled,
        // stroked rectangle between two canvas points. Shared by the live
        // box-selection and the preview-only window marquee (#291).
        fn draw_marquee(
            frame: &mut canvas::Frame,
            a: Point,
            b: Point,
            crossing: bool,
            theme: &Theme,
            visual: &SelectionVisualOptions,
            canvas_bg: [f32; 4],
        ) {
            let base = resolve_selection_base_color(crossing, theme, visual, canvas_bg);
            let canvas_light = crate::ui::style::common::canvas_is_light(canvas_bg);
            // Desk-spanning marquee edge case: deliberate limitation to avoid mid-drag rectangle color splits when dragged past sheet onto desk.
            let x0 = a.x.min(b.x);
            let y0 = a.y.min(b.y);
            let w = (a.x - b.x).abs();
            let h = (a.y - b.y).abs();
            let rect = canvas::Path::rectangle(Point::new(x0, y0), Size::new(w, h));
            if visual.area && visual.opacity > 0 {
                let alpha = selection_fill_alpha(visual.opacity as f32, canvas_light);
                let fill = base.scale_alpha(alpha);
                frame.fill(&rect, fill);
            }
            let stroke_alpha = if canvas_light { 0.95 } else { 0.90 };
            let stroke = base.scale_alpha(stroke_alpha);
            frame.stroke(
                &rect,
                canvas::Stroke {
                    width: 1.0,
                    style: canvas::Style::Solid(stroke),
                    line_dash: if crossing {
                        canvas::LineDash {
                            segments: &[4.0, 4.0],
                            offset: 0,
                        }
                    } else {
                        canvas::LineDash::default()
                    },
                    ..Default::default()
                },
            );
        }

        if let (Some(a), Some(b)) = (self.selection.borrow().box_anchor, self.selection.borrow().box_current) {
            draw_marquee(&mut frame, a, b, self.selection.borrow().box_crossing, theme, &self.selection_visual, self.crosshair_bg);
        }
        // Preview marquee for point-picked windows (STRETCH) — same look, no pick.
        if let Some((a, b, crossing)) = self.selection.borrow().preview_box {
            draw_marquee(&mut frame, a, b, crossing, theme, &self.selection_visual, self.crosshair_bg);
        }

        if self.selection.borrow().poly_active && self.selection.borrow().poly_points.len() > 1 {
            let crossing = self.selection.borrow().poly_crossing;
            let base = resolve_selection_base_color(crossing, theme, &self.selection_visual, self.crosshair_bg);
            let canvas_light = crate::ui::style::common::canvas_is_light(self.crosshair_bg);
            if self.selection_visual.area && self.selection_visual.opacity > 0 {
                let alpha = selection_fill_alpha(self.selection_visual.opacity as f32, canvas_light);
                let fill = base.scale_alpha(alpha);
                if let Some(cur) = self.selection.borrow().last_move_pos {
                    let start = self.selection.borrow().poly_points[0];
                    let fill_path = canvas::Path::new(|p| {
                        p.move_to(start);
                        for pt in &self.selection.borrow().poly_points[1..] {
                            p.line_to(*pt);
                        }
                        p.line_to(cur);
                        p.line_to(start);
                    });
                    frame.fill(&fill_path, fill);
                }
            }
            let stroke_alpha = if canvas_light { 0.95 } else { 0.90 };
            let stroke = base.scale_alpha(stroke_alpha);
            let path = canvas::Path::new(|p| {
                p.move_to(self.selection.borrow().poly_points[0]);
                for pt in &self.selection.borrow().poly_points[1..] {
                    p.line_to(*pt);
                }
            });
            let stroke_style = canvas::Stroke {
                width: 1.0,
                style: canvas::Style::Solid(stroke),
                line_dash: if crossing {
                    canvas::LineDash {
                        segments: &[4.0, 4.0],
                        offset: 0,
                    }
                } else {
                    canvas::LineDash::default()
                },
                ..Default::default()
            };
            frame.stroke(&path, stroke_style.clone());
            if let Some(cur) = self.selection.borrow().last_move_pos {
                let start = self.selection.borrow().poly_points[0];
                let last = *self.selection.borrow().poly_points.last().unwrap();
                let preview = canvas::Path::new(|p| {
                    p.move_to(last);
                    p.line_to(cur);
                    p.line_to(start);
                });
                frame.stroke(&preview, stroke_style);
            }
        }

        // ── Grip markers ──────────────────────────────────────────────────
        let grip_bounds = self.grip_clip.unwrap_or(bounds);
        let grip_x0 = grip_bounds.x.max(0.0);
        let grip_y0 = grip_bounds.y.max(0.0);
        let grip_x1 = (grip_bounds.x + grip_bounds.width).min(bounds.width);
        let grip_y1 = (grip_bounds.y + grip_bounds.height).min(bounds.height);
        if grip_x1 > grip_x0 && grip_y1 > grip_y0 {
            let grip_clip = iced::Rectangle {
                x: grip_x0,
                y: grip_y0,
                width: grip_x1 - grip_x0,
                height: grip_y1 - grip_y0,
            };
            frame.with_clip(grip_clip, |frame| {
                if let Some((points, closed)) = &self.control_polygon {
                    if points.len() >= 2 {
                        let polygon = canvas::Path::new(|builder| {
                            builder.move_to(points[0]);
                            for point in points.iter().skip(1) {
                                builder.line_to(*point);
                            }
                            if *closed {
                                builder.line_to(points[0]);
                            }
                        });
                        frame.stroke(
                            &polygon,
                            canvas::Stroke {
                                width: 1.0,
                                style: canvas::Style::Solid(Color::from_rgb(0.72, 0.32, 0.15)),
                                line_dash: canvas::LineDash {
                                    segments: &[4.0, 4.0],
                                    offset: 0,
                                },
                                ..Default::default()
                            },
                        );
                    }
                }
                for grip in &self.grips {
                    draw_grip_marker(frame, grip, theme, &self.selection_visual);
                }
            });
        }

        // ── Snap marker ───────────────────────────────────────────────────
        if let Some((sp, snap_type)) = self.snap {
            let (r, g, b) = if snap_type == SnapType::ObjectPick {
                (0.95_f32, 0.50, 0.08) // orange object-snap marker
            } else {
                (1.0, 0.9, 0.1) // classic yellow OSNAP
            };
            let marker = Color { r, g, b, a: 1.0 };
            let stroke = canvas::Stroke {
                width: if snap_type == SnapType::ObjectPick { 2.0 } else { 1.5 },
                style: canvas::Style::Solid(marker),
                ..Default::default()
            };
            match snap_type {
                SnapType::ObjectPick => {
                    // Target box + center dot (object-acquisition glyph).
                    let h = 7.0_f32;
                    let rect = canvas::Path::rectangle(
                        Point::new(sp.x - h, sp.y - h),
                        Size::new(h * 2.0, h * 2.0),
                    );
                    frame.stroke(&rect, stroke.clone());
                    let r = 3.0_f32;
                    frame.fill(
                        &canvas::Path::circle(sp, r),
                        Color {
                            r: 0.95,
                            g: 0.50,
                            b: 0.08,
                            a: 0.85,
                        },
                    );
                }
                SnapType::Endpoint => {
                    let h = 5.0_f32;
                    let rect = canvas::Path::rectangle(
                        Point::new(sp.x - h, sp.y - h),
                        Size::new(h * 2.0, h * 2.0),
                    );
                    frame.stroke(&rect, stroke);
                }
                SnapType::Midpoint => {
                    let r = 6.0_f32;
                    let path = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x, sp.y - r));
                        b.line_to(Point::new(sp.x + r * 0.866, sp.y + r * 0.5));
                        b.line_to(Point::new(sp.x - r * 0.866, sp.y + r * 0.5));
                        b.close();
                    });
                    frame.stroke(&path, stroke);
                }
                SnapType::Center => {
                    let r = 5.5_f32;
                    let path = canvas::Path::circle(sp, r);
                    frame.stroke(&path, stroke);
                }
                SnapType::Node => {
                    // Circle with an inscribed X.
                    let r = 5.5_f32;
                    let cpath = canvas::Path::circle(sp, r);
                    frame.stroke(&cpath, stroke.clone());
                    let d = r * std::f32::consts::FRAC_1_SQRT_2;
                    let x1 = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - d, sp.y - d));
                        b.line_to(Point::new(sp.x + d, sp.y + d));
                    });
                    let x2 = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - d, sp.y + d));
                        b.line_to(Point::new(sp.x + d, sp.y - d));
                    });
                    frame.stroke(&x1, stroke.clone());
                    frame.stroke(&x2, stroke);
                }
                SnapType::Quadrant => {
                    let r = 6.0_f32;
                    let path = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x, sp.y - r));
                        b.line_to(Point::new(sp.x + r, sp.y));
                        b.line_to(Point::new(sp.x, sp.y + r));
                        b.line_to(Point::new(sp.x - r, sp.y));
                        b.close();
                    });
                    frame.stroke(&path, stroke);
                }
                SnapType::Intersection => {
                    // An extended intersection sets one or two extension bases;
                    // draw a dashed guide from each endpoint through the crossing
                    // so both contributing extension paths stay visible. A real
                    // on-segment crossing carries no bases, so nothing draws here.
                    // (#247, #259)
                    for base in [self.snap_ext_base, self.snap_ext_base2]
                        .into_iter()
                        .flatten()
                    {
                        let dx = sp.x - base.x;
                        let dy = sp.y - base.y;
                        let len = (dx * dx + dy * dy).sqrt();
                        if len > 1e-3 {
                            let dash = canvas::Stroke {
                                line_dash: canvas::LineDash {
                                    segments: &[4.0, 4.0],
                                    offset: 0,
                                },
                                ..canvas::Stroke::default().with_color(marker).with_width(1.0)
                            };
                            let tip = Point::new(
                                sp.x + dx / len * 18.0,
                                sp.y + dy / len * 18.0,
                            );
                            // `base` is a projected entity endpoint: clip before
                            // stroking (see `clip_seg`).
                            if let Some((a, b)) = clip_seg(base, tip, bounds) {
                                frame.stroke(&canvas::Path::line(a, b), dash);
                            }
                        }
                    }
                    let r = 5.0_f32;
                    let p1 = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - r, sp.y - r));
                        b.line_to(Point::new(sp.x + r, sp.y + r));
                    });
                    let p2 = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - r, sp.y + r));
                        b.line_to(Point::new(sp.x + r, sp.y - r));
                    });
                    frame.stroke(&p1, stroke.clone());
                    frame.stroke(&p2, stroke);
                }
                SnapType::ApparentIntersection => {
                    // X like Intersection, framed by a small square so the
                    // two are visually distinguishable.
                    let r = 5.0_f32;
                    let rect = canvas::Path::rectangle(
                        Point::new(sp.x - r, sp.y - r),
                        Size::new(r * 2.0, r * 2.0),
                    );
                    frame.stroke(&rect, stroke.clone());
                    let xr = r - 1.5;
                    let p1 = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - xr, sp.y - xr));
                        b.line_to(Point::new(sp.x + xr, sp.y + xr));
                    });
                    let p2 = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - xr, sp.y + xr));
                        b.line_to(Point::new(sp.x + xr, sp.y - xr));
                    });
                    frame.stroke(&p1, stroke.clone());
                    frame.stroke(&p2, stroke);
                }
                SnapType::Insertion => {
                    // Two overlapping rectangles (a small "tag" glyph).
                    let r = 5.0_f32;
                    let inner = canvas::Path::rectangle(
                        Point::new(sp.x - r * 0.5, sp.y - r),
                        Size::new(r, r * 2.0),
                    );
                    let outer = canvas::Path::rectangle(
                        Point::new(sp.x - r, sp.y - r * 0.5),
                        Size::new(r * 2.0, r),
                    );
                    frame.stroke(&outer, stroke.clone());
                    frame.stroke(&inner, stroke);
                }
                SnapType::Perpendicular => {
                    // Right-angle hook in the lower-left quadrant.
                    let r = 6.0_f32;
                    let p = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - r, sp.y - r));
                        b.line_to(Point::new(sp.x - r, sp.y + r));
                        b.line_to(Point::new(sp.x + r, sp.y + r));
                    });
                    let foot = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - r, sp.y));
                        b.line_to(Point::new(sp.x, sp.y));
                        b.line_to(Point::new(sp.x, sp.y + r));
                    });
                    frame.stroke(&p, stroke.clone());
                    frame.stroke(&foot, stroke);
                }
                SnapType::Tangent => {
                    // Circle with a tangent bar across the top.
                    let r = 5.5_f32;
                    let c = canvas::Path::circle(sp, r);
                    frame.stroke(&c, stroke.clone());
                    let bar = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - r, sp.y - r));
                        b.line_to(Point::new(sp.x + r, sp.y - r));
                    });
                    frame.stroke(&bar, stroke);
                }
                SnapType::Nearest => {
                    // Bowtie / hourglass — two opposed triangles meeting at sp.
                    let r = 5.5_f32;
                    let path = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - r, sp.y - r));
                        b.line_to(Point::new(sp.x + r, sp.y - r));
                        b.line_to(Point::new(sp.x - r, sp.y + r));
                        b.line_to(Point::new(sp.x + r, sp.y + r));
                        b.close();
                    });
                    frame.stroke(&path, stroke);
                }
                SnapType::Extension => {
                    // Unit direction from the endpoint the snap extends from
                    // toward the snap point, so the guide line and the three
                    // dots follow the actual extension path (#238).
                    let dir = self.snap_ext_base.and_then(|base| {
                        let dx = sp.x - base.x;
                        let dy = sp.y - base.y;
                        let len = (dx * dx + dy * dy).sqrt();
                        (len > 1e-3).then(|| (base, Point::new(dx / len, dy / len)))
                    });
                    // Dashed guide line from that endpoint, through the snap
                    // point and a little beyond, so the extension path is
                    // visible as the cursor tracks along it.
                    if let Some((base, u)) = dir {
                        let dash = canvas::Stroke {
                            line_dash: canvas::LineDash {
                                segments: &[4.0, 4.0],
                                offset: 0,
                            },
                            ..canvas::Stroke::default().with_color(marker).with_width(1.0)
                        };
                        let tip = Point::new(sp.x + u.x * 18.0, sp.y + u.y * 18.0);
                        // `base` is a projected entity endpoint: clip before
                        // stroking (see `clip_seg`).
                        if let Some((a, b)) = clip_seg(base, tip, bounds) {
                            frame.stroke(&canvas::Path::line(a, b), dash);
                        }
                    }
                    // Three dots at the snap point, strung along the extension
                    // direction (horizontal fallback when the base is unknown).
                    let u = dir.map(|(_, u)| u).unwrap_or(Point::new(1.0, 0.0));
                    let r = 1.4_f32;
                    for k in [-7.0_f32, 0.0, 7.0] {
                        let dot =
                            canvas::Path::circle(Point::new(sp.x + u.x * k, sp.y + u.y * k), r);
                        frame.fill(&dot, marker);
                    }
                }
                SnapType::Parallel => {
                    // Two short parallel diagonal bars.
                    let r = 6.0_f32;
                    let off = 3.0_f32;
                    let b1 = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - r - off, sp.y + r));
                        b.line_to(Point::new(sp.x + r - off, sp.y - r));
                    });
                    let b2 = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - r + off, sp.y + r));
                        b.line_to(Point::new(sp.x + r + off, sp.y - r));
                    });
                    frame.stroke(&b1, stroke.clone());
                    frame.stroke(&b2, stroke);
                }
                SnapType::Grid => {
                    let arm = 4.0_f32;
                    let h = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x - arm, sp.y));
                        b.line_to(Point::new(sp.x + arm, sp.y));
                    });
                    let v = canvas::Path::new(|b| {
                        b.move_to(Point::new(sp.x, sp.y - arm));
                        b.line_to(Point::new(sp.x, sp.y + arm));
                    });
                    frame.stroke(&h, stroke.clone());
                    frame.stroke(&v, stroke);
                }
            }
        }

        // ── CAD crosshair cursor ──────────────────────────────────────────────
        let over_viewcube = self.show_viewcube && {
            use crate::scene::{VIEWCUBE_PAD, VIEWCUBE_REGION_PX};
            cursor.position_in(bounds).map_or(false, |pos| {
                let vc_x = bounds.width - VIEWCUBE_REGION_PX - VIEWCUBE_PAD;
                let vc_y = VIEWCUBE_PAD;
                pos.x >= vc_x
                    && pos.x <= vc_x + VIEWCUBE_REGION_PX
                    && pos.y >= vc_y
                    && pos.y <= vc_y + VIEWCUBE_REGION_PX
            })
        };
        // Over a Model-tile divider the OS cursor switches to a resize
        // arrow (see `mouse_interaction`); drawing the CAD crosshair on
        // top of it would double up the visual feedback.
        let over_divider = self.divider_under(cursor, bounds);
        // PAN mode replaces the crosshair with a hand cursor.
        if !over_viewcube
            && !over_divider
            && !self.pan_mode
            && !self.suppressed
            && self.crosshair.cursor_type == CursorType::Crosshair
        {
            if let Some(cp) = self.selection.borrow().last_move_pos {
                let [r, g, b, a] = self.crosshair.color.map_or_else(
                    || {
                        crate::scene::view::render::adapt_to_bg(
                            [1.0, 1.0, 1.0, 0.90],
                            self.crosshair_bg,
                        )
                    },
                    |[r, g, b]| {
                        [
                            r as f32 / 255.0,
                            g as f32 / 255.0,
                            b as f32 / 255.0,
                            0.90,
                        ]
                    },
                );
                let color = Color { r, g, b, a };
                let stroke = canvas::Stroke {
                    width: 1.0,
                    style: canvas::Style::Solid(color),
                    ..Default::default()
                };
                let point_mode = self.crosshair.point_mode;
                let sq = if point_mode {
                    0.0
                } else {
                    pick_box_half_px(self.crosshair.pick_box)
                };
                let arm = crosshair_arm_px(bounds, self.crosshair.size_percent);
                let base_angles: [f64; 2] = if self.crosshair.isometric {
                    self.crosshair.iso_plane.angles()
                } else {
                    [0.0, 90.0]
                };
                for angle in base_angles {
                    let rad = (angle + self.crosshair.snap_angle_deg as f64).to_radians();
                    let dir = Point::new(rad.cos() as f32, -rad.sin() as f32);
                    let gap = if point_mode {
                        9.0
                    } else if sq > 0.0 {
                        sq / dir.x.abs().max(dir.y.abs()).max(1e-6)
                    } else {
                        0.0
                    };
                    let arms = canvas::Path::new(|path| {
                        path.move_to(Point::new(cp.x + dir.x * gap, cp.y + dir.y * gap));
                        path.line_to(Point::new(cp.x + dir.x * arm, cp.y + dir.y * arm));
                        path.move_to(Point::new(cp.x - dir.x * gap, cp.y - dir.y * gap));
                        path.line_to(Point::new(cp.x - dir.x * arm, cp.y - dir.y * arm));
                    });
                    frame.stroke(&arms, stroke.clone());
                }
                if point_mode {
                    let dot = canvas::Path::circle(cp, 1.75);
                    frame.fill(&dot, color);
                } else if sq > 0.0 {
                    let square = canvas::Path::rectangle(
                        Point::new(cp.x - sq, cp.y - sq),
                        Size::new(sq * 2.0, sq * 2.0),
                    );
                    frame.stroke(&square, stroke);
                }

                // Locked-layer badge: a small padlock beside the crosshair when
                // the hovered object sits on a locked layer (issue: locked
                // objects are visible, snappable and selectable but not editable).
                if self.hover_locked {
                    let warning = theme.palette().warning.base;
                    let amber = warning.color.scale_alpha(0.98);
                    let dark = warning.text;
                    let bx = cp.x + sq + 7.0;
                    let by = cp.y - sq - 13.0;
                    // Lock body (filled).
                    let body = canvas::Path::rectangle(
                        Point::new(bx, by + 6.0),
                        Size::new(12.0, 9.0),
                    );
                    frame.fill(&body, amber);
                    // Shackle: an inverted-U above the body (squared so the
                    // shape is unambiguous regardless of arc winding).
                    let shackle = canvas::Path::new(|b| {
                        b.move_to(Point::new(bx + 2.5, by + 6.0));
                        b.line_to(Point::new(bx + 2.5, by + 2.5));
                        b.line_to(Point::new(bx + 9.5, by + 2.5));
                        b.line_to(Point::new(bx + 9.5, by + 6.0));
                    });
                    frame.stroke(
                        &shackle,
                        canvas::Stroke {
                            width: 1.8,
                            style: canvas::Style::Solid(amber),
                            line_join: canvas::LineJoin::Round,
                            ..Default::default()
                        },
                    );
                    // Keyhole.
                    let hole = canvas::Path::circle(Point::new(bx + 6.0, by + 10.5), 1.4);
                    frame.fill(&hole, dark);
                }
            }
        } // end !over_viewcube

        // ── UCS icon (one per Model pane) ─────────────────────────────────
        for ucs in &self.ucs_icons {
            draw_ucs_icon(
                &mut frame,
                ucs.view_proj,
                ucs.bounds,
                ucs.axes,
                ucs.origin_screen,
                ucs.hover,
                ucs.selected,
                self.crosshair_bg,
            );
        }

        // ── Object Snap Tracking ─────────────────────────────────────────────
        let track_color = theme.palette().primary.base.color.scale_alpha(0.7);
        // The alignment line the cursor is currently locked to — drawn at its
        // real angle from the acquired point through the lock and a little
        // beyond, dashed so it reads as a construction guide. This covers the
        // ortho (0°/90°), polar, and edge-extension cases uniformly (#219).
        if let Some((base, tip)) = self.otrack_line {
            let dx = tip.x - base.x;
            let dy = tip.y - base.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1e-3 {
                // Extend the alignment path well past both ends along its
                // direction so it reads as a full construction line through the
                // acquired point, not a stub between the corner and the cursor
                // (#219), then clip it to the viewport ourselves: the canvas
                // clips only after tessellating, which is too late for a dash
                // pattern (see `clip_seg`).
                let (ux, uy) = (dx / len, dy / len);
                const L: f32 = 5000.0;
                let p0 = Point::new(base.x - ux * L, base.y - uy * L);
                let p1 = Point::new(tip.x + ux * L, tip.y + uy * L);
                let dash = canvas::Stroke {
                    line_dash: canvas::LineDash {
                        segments: &[6.0, 4.0],
                        offset: 0,
                    },
                    ..canvas::Stroke::default()
                        .with_color(track_color)
                        .with_width(1.0)
                };
                if let Some((a, b)) = clip_seg(p0, p1, bounds) {
                    frame.stroke(&canvas::Path::line(a, b), dash);
                }
            }
        }
        // The acquired Parallel-snap reference — a small ∥ glyph on its line so
        // the user sees which line the parallel is measured from. (#277)
        if let Some(m) = self.parallel_ref_marker {
            let stroke = canvas::Stroke::default()
                .with_color(track_color)
                .with_width(1.5);
            let r = 6.0_f32;
            let off = 3.0_f32;
            let b1 = canvas::Path::new(|b| {
                b.move_to(Point::new(m.x - r - off, m.y + r));
                b.line_to(Point::new(m.x + r - off, m.y - r));
            });
            let b2 = canvas::Path::new(|b| {
                b.move_to(Point::new(m.x - r + off, m.y + r));
                b.line_to(Point::new(m.x + r + off, m.y - r));
            });
            frame.stroke(&b1, stroke.clone());
            frame.stroke(&b2, stroke);
        }
        // Small cross at each acquired tracking point.
        for ost in &self.ost_points {
            let tp = ost.screen;
            let stroke = canvas::Stroke::default()
                .with_color(track_color)
                .with_width(1.0);
            let sz = 5.0_f32;
            let h = canvas::Path::line(
                Point {
                    x: tp.x - sz,
                    y: tp.y,
                },
                Point {
                    x: tp.x + sz,
                    y: tp.y,
                },
            );
            let v = canvas::Path::line(
                Point {
                    x: tp.x,
                    y: tp.y - sz,
                },
                Point {
                    x: tp.x,
                    y: tp.y + sz,
                },
            );
            frame.stroke(&h, stroke.clone());
            frame.stroke(&v, stroke);
        }

        vec![frame.into_geometry()]
    }
}

// ── Grid line drawing ─────────────────────────────────────────────────────

/// Minimum pixel gap between adjacent grid lines before stepping up to next spacing.
const MIN_GRID_PX: f32 = 20.0;
/// Stop an infinite perspective grid before adjacent lines merge at the horizon.
const MIN_HORIZON_GRID_PX: f32 = 5.0;

#[allow(clippy::too_many_arguments)]
fn draw_grid(
    frame: &mut canvas::Frame,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    step: f32,
    grid_origin: glam::DVec3,
    grid_axes: (Vec3, Vec3, Vec3),
    limits: Option<(glam::DVec2, glam::DVec2)>,
    style: GridStyle,
) {
    let alpha = (style.opacity as f32 / 100.0).clamp(0.02, 1.0);
    let gc = if style.bg_luminance > 0.5 {
        // Light background: subtle dark grid lines
        Color {
            r: 0.10,
            g: 0.10,
            b: 0.10,
            a: alpha,
        }
    } else {
        // Dark background: subtle light grid lines
        Color {
            r: 0.80,
            g: 0.80,
            b: 0.80,
            a: alpha,
        }
    };
    let st = canvas::Stroke {
        width: 0.5,
        style: canvas::Style::Solid(gc),
        ..Default::default()
    };
    let geometry = grid_segments(view_rot, eye, bounds, step, grid_origin, grid_axes, limits);
    if !geometry.segments.is_empty() {
        let path = canvas::Path::new(|builder| {
            for (p0, p1) in &geometry.segments {
                builder.move_to(*p0);
                builder.line_to(*p1);
            }
        });
        frame.stroke(&path, st);
    }
    if geometry.axis_extent > 0.0 {
        let (gx, gy, gz) = grid_axes;
        let extent = (geometry.axis_extent + step) * 1.5;
        draw_axes(frame, view_rot, eye, bounds, extent.max(10.0), grid_origin, (gx, gy, gz), style.bg_luminance);
    }
}

/// Pure, renderer-free projection of the grid for one pane. Returns canvas-local
/// `(Point, Point)` segments plus the axis extent (in world units along the
/// active UCS axes) used by `draw_grid` to size the coloured UCS axes overlay.
///
/// Extracted from `draw_grid` (2026-08-26, Mission #1 step 1) so the geometry
/// construction can be unit-tested and benchmarked without an iced
/// `Renderer`. Behaviour is identical to the inlined version that preceded it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn grid_segments(
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    step: f32,
    grid_origin: glam::DVec3,
    grid_axes: (Vec3, Vec3, Vec3),
    limits: Option<(glam::DVec2, glam::DVec2)>,
) -> GridGeometry {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return GridGeometry::empty();
    }

    // World → viewport-local screen via relative-to-eye: subtract the f64 eye
    // first so grid points near the camera stay precise at UTM-scale coords.
    // Reject points on/behind the perspective eye plane before dividing by W.
    let project = |world: glam::DVec3| -> Option<Point> {
        let rel = (world - eye).as_vec3();
        let clip = view_rot * rel.extend(1.0);
        if !clip.is_finite() || clip.w <= 1e-7 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let screen = Point::new(
            (ndc.x + 1.0) * 0.5 * bounds.width,
            (1.0 - ndc.y) * 0.5 * bounds.height,
        );
        (screen.x.is_finite() && screen.y.is_finite()).then_some(screen)
    };

    // Grid is intentionally restricted to the active UCS XY plane.
    let (gx, gy, gz) = grid_axes;
    let axis1 = gx.normalize_or(Vec3::X);
    let axis2 = gy.normalize_or(Vec3::Y);
    let inv = view_rot.inverse();
    let plane_normal = axis1.cross(axis2).normalize_or(Vec3::Z);
    let plane_rel = (grid_origin - eye).as_vec3();

    // Intersect a viewport-local screen ray with the real XY plane. Unlike the
    // old mid-depth approximation, this covers the complete viewport after an
    // orbit and also tells us when a ray crosses the perspective horizon.
    let unproject = |sx: f32, sy: f32| -> Option<glam::DVec3> {
        let ndc_x = (sx / bounds.width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (sy / bounds.height) * 2.0;
        let near = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
        let ray = far - near;
        let denom = ray.dot(plane_normal);
        if !near.is_finite()
            || !far.is_finite()
            || !denom.is_finite()
            || denom.abs() < 1e-7
        {
            return None;
        }
        let t = (plane_rel - near).dot(plane_normal) / denom;
        // `far` defines only the unprojection line direction. Its parameter
        // sign is not a reliable front/behind test after the WGPU depth
        // conversion and was cutting the near side during zoom/orbit. The
        // homogeneous clip-W check in `project` is the canonical eye-plane
        // test and also keeps the infinite grid independent of near/far depth.
        if !t.is_finite() {
            return None;
        }
        let hit = near + ray * t;
        if !hit.is_finite() {
            return None;
        }
        let world = eye + hit.as_dvec3();
        project(world).map(|_| world)
    };

    // Perpendicular screen gap between the two neighbouring lines of each
    // family at a point on the grid. Measuring the perpendicular component,
    // rather than point-to-point distance, remains correct for a skewed
    // perspective grid.
    let grid_gaps = |world: glam::DVec3, step: f32| -> Option<(f32, f32)> {
        let p = project(world)?;
        let projected_deltas = |axis: Vec3, amount: f32| {
            [amount, -amount].map(|signed_step| {
                project(world + (axis * signed_step).as_dvec3())
                    .map(|next| glam::Vec2::new(next.x - p.x, next.y - p.y))
            })
        };
        let neighbours1 = projected_deltas(axis1, step);
        let neighbours2 = projected_deltas(axis2, step);
        // A full grid step is needed to measure adjacent-line distance, but it
        // is too large for the line's local tangent near the eye. A small
        // derivative keeps the tangent measurable without crossing the eye.
        let tangent_step = (step * 0.01).max(1e-4);
        let tangents1 = projected_deltas(axis1, tangent_step);
        let tangents2 = projected_deltas(axis2, tangent_step);

        // At the near side of a perspective plane a large +step neighbour may
        // cross behind the eye while the -step neighbour remains perfectly
        // visible (or vice versa). Requiring only +X/+Y cut away that entire
        // near side after zoom-out. Use whichever visible neighbour gives the
        // readable separation for each line family.
        let mut gap1 = 0.0_f32;
        let mut gap2 = 0.0_f32;
        for neighbour in neighbours1.into_iter().flatten() {
            for tangent in tangents2.into_iter().flatten() {
                let tangent_len = tangent.length();
                if tangent_len > 1e-6 {
                    let area =
                        (neighbour.x * tangent.y - neighbour.y * tangent.x).abs();
                    gap1 = gap1.max(area / tangent_len);
                }
            }
        }
        for neighbour in neighbours2.into_iter().flatten() {
            for tangent in tangents1.into_iter().flatten() {
                let tangent_len = tangent.length();
                if tangent_len > 1e-6 {
                    let area =
                        (neighbour.x * tangent.y - neighbour.y * tangent.x).abs();
                    gap2 = gap2.max(area / tangent_len);
                }
            }
        }
        (gap1.is_finite() && gap2.is_finite()).then_some((gap1, gap2))
    };

    // Use several interior points because the UCS origin may be off-screen or
    // arbitrarily close to the perspective horizon.
    const SAMPLE_FRACTIONS: [f32; 5] = [0.02, 0.25, 0.5, 0.75, 0.98];
    let mut samples = Vec::with_capacity(SAMPLE_FRACTIONS.len().pow(2));
    for fy in SAMPLE_FRACTIONS {
        for fx in SAMPLE_FRACTIONS {
            let screen = glam::Vec2::new(bounds.width * fx, bounds.height * fy);
            if let Some(world) = unproject(screen.x, screen.y) {
                samples.push((screen, world));
            }
        }
    }
    if samples.is_empty() {
        return GridGeometry::empty();
    };

    // Step follows camera zoom only. The previous visible-sample calculation
    // changed depth while orbiting and made the grid jump 1 → 5 → 25.
    if !step.is_finite() || step <= 0.0 {
        return GridGeometry::empty();
    }
    let s = step;

    // Trace a family-specific visible region around the viewport perimeter.
    // When a boundary ray points through the horizon, binary-search back toward
    // a readable anchor and stop where neighbouring lines reach the minimum gap.
    let collect_extent = |
        family: usize,
        anchor_screen: glam::Vec2,
        anchor_world: glam::DVec3,
    | -> Vec<glam::DVec3> {
        let visible_at = |screen: glam::Vec2| -> Option<glam::DVec3> {
            let world = unproject(screen.x, screen.y)?;
            let gaps = grid_gaps(world, s)?;
            let gap = if family == 0 { gaps.0 } else { gaps.1 };
            (gap >= MIN_HORIZON_GRID_PX).then_some(world)
        };

        const EDGE_STEPS: usize = 12;
        const SEARCH_STEPS: usize = 16;
        let mut hits = vec![anchor_world];
        for i in 0..=EDGE_STEPS {
            let f = i as f32 / EDGE_STEPS as f32;
            let targets = [
                glam::Vec2::new(bounds.width * f, 0.0),
                glam::Vec2::new(bounds.width * f, bounds.height),
                glam::Vec2::new(0.0, bounds.height * f),
                glam::Vec2::new(bounds.width, bounds.height * f),
            ];
            for target in targets {
                if let Some(world) = visible_at(target) {
                    hits.push(world);
                    continue;
                }
                let mut near_screen = anchor_screen;
                let mut far_screen = target;
                let mut near_world = anchor_world;
                for _ in 0..SEARCH_STEPS {
                    let middle = (near_screen + far_screen) * 0.5;
                    if let Some(world) = visible_at(middle) {
                        near_screen = middle;
                        near_world = world;
                    } else {
                        far_screen = middle;
                    }
                }
                hits.push(near_world);
            }
        }
        hits
    };

    let best_anchor = |family: usize| -> Option<(glam::Vec2, glam::DVec3, f32)> {
        let mut best = None;
        for (screen, world) in &samples {
            let Some(gaps) = grid_gaps(*world, s) else {
                continue;
            };
            let gap = if family == 0 { gaps.0 } else { gaps.1 };
            if best.map_or(true, |(_, _, best_gap)| gap > best_gap) {
                best = Some((*screen, *world, gap));
            }
        }
        best
    };
    let axis_range = |hits: &[glam::DVec3], axis: Vec3| -> Option<(f32, f32)> {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for world in hits {
            let value = (*world - grid_origin).as_vec3().dot(axis);
            if value.is_finite() {
                min = min.min(value);
                max = max.max(value);
            }
        }
        (min <= max).then_some((min, max))
    };
    let line_range = |min: f32, max: f32, anchor: f32| -> (i32, i32) {
        let mut start = (min / s).floor() as i32;
        let mut end = (max / s).ceil() as i32;
        // The pixel-gap cut-off naturally bounds this by viewport resolution. Keep
        // malformed projection data from creating an unbounded CPU loop.
        let limit = ((bounds.width + bounds.height).ceil() as i32 + 64).max(128);
        let center = (anchor / s).round() as i32;
        start = start.max(center.saturating_sub(limit));
        end = end.min(center.saturating_add(limit));
        (start, end)
    };
    // Project an infinite world-grid line to its exact screen-space line and
    // intersect it with the viewport rectangle. This remains valid across the
    // perspective horizon; projecting a finite world-space bounding rectangle
    // is only correct for the perpendicular/top view.
    let grid_clip_origin = view_rot * (grid_origin - eye).as_vec3().extend(1.0);
    let grid_clip_axis1 = view_rot * axis1.extend(0.0);
    let grid_clip_axis2 = view_rot * axis2.extend(0.0);
    let project_line = |family: usize, value: f32| -> Option<(glam::Vec2, glam::Vec2)> {
        let (base, direction) = if family == 0 {
            (
                grid_clip_origin + grid_clip_axis1 * value,
                grid_clip_axis2,
            )
        } else {
            (
                grid_clip_origin + grid_clip_axis2 * value,
                grid_clip_axis1,
            )
        };
        let screen_h = |clip: glam::Vec4| {
            glam::Vec3::new(
                (clip.x + clip.w) * 0.5 * bounds.width,
                (-clip.y + clip.w) * 0.5 * bounds.height,
                clip.w,
            )
        };
        let line = screen_h(base).cross(screen_h(base + direction));
        if !line.is_finite() || line.x.abs() + line.y.abs() < 1e-8 {
            return None;
        }

        const EDGE_EPS: f32 = 0.5;
        let mut points = Vec::with_capacity(4);
        let mut add_point = |point: glam::Vec2| {
            if !point.is_finite()
                || point.x < -EDGE_EPS
                || point.x > bounds.width + EDGE_EPS
                || point.y < -EDGE_EPS
                || point.y > bounds.height + EDGE_EPS
            {
                return;
            }
            let point = glam::Vec2::new(
                point.x.clamp(0.0, bounds.width),
                point.y.clamp(0.0, bounds.height),
            );
            if points.iter().all(|p: &glam::Vec2| p.distance_squared(point) > 1e-4) {
                points.push(point);
            }
        };
        if line.y.abs() > 1e-8 {
            add_point(glam::Vec2::new(0.0, -line.z / line.y));
            add_point(glam::Vec2::new(
                bounds.width,
                -(line.x * bounds.width + line.z) / line.y,
            ));
        }
        if line.x.abs() > 1e-8 {
            add_point(glam::Vec2::new(-line.z / line.x, 0.0));
            add_point(glam::Vec2::new(
                -(line.y * bounds.height + line.z) / line.x,
                bounds.height,
            ));
        }
        if points.len() < 2 {
            return None;
        }

        let mut best = (points[0], points[1]);
        let mut best_distance = best.0.distance_squared(best.1);
        for i in 0..points.len() {
            for j in i + 1..points.len() {
                let distance = points[i].distance_squared(points[j]);
                if distance > best_distance {
                    best = (points[i], points[j]);
                    best_distance = distance;
                }
            }
        }
        Some(best)
    };

    // Keep only the parts of one projected line where its neighbouring line is
    // at least the configured physical-pixel gap away. The visible interval is found in screen
    // space, so oblique/trapezoidal views no longer inherit rectangular world
    // bounds from the top view.
    let trim_line = |
        family: usize,
        p0: glam::Vec2,
        p1: glam::Vec2,
    | -> Vec<(Point, Point)> {
        let visible = |t: f32| {
            let screen = p0.lerp(p1, t);
            let Some(world) = unproject(screen.x, screen.y) else {
                return false;
            };
            let Some(gaps) = grid_gaps(world, s) else {
                return false;
            };
            let gap = if family == 0 { gaps.0 } else { gaps.1 };
            gap >= MIN_HORIZON_GRID_PX
        };
        let find_transition = |mut low: f32, mut high: f32, low_visible: bool| {
            for _ in 0..14 {
                let middle = (low + high) * 0.5;
                if visible(middle) == low_visible {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            (low + high) * 0.5
        };

        const LINE_SAMPLES: usize = 16;
        let mut result = Vec::with_capacity(2);
        let mut previous_t = 0.0_f32;
        let mut previous_visible = visible(previous_t);
        let mut run_start = previous_visible.then_some(previous_t);
        for i in 1..=LINE_SAMPLES {
            let t = i as f32 / LINE_SAMPLES as f32;
            let is_visible = visible(t);
            if is_visible != previous_visible {
                let boundary = find_transition(previous_t, t, previous_visible);
                if is_visible {
                    run_start = Some(boundary);
                } else if let Some(start) = run_start.take() {
                    let a = p0.lerp(p1, start);
                    let b = p0.lerp(p1, boundary);
                    result.push((
                        Point::new(a.x + bounds.x, a.y + bounds.y),
                        Point::new(b.x + bounds.x, b.y + bounds.y),
                    ));
                }
            }
            previous_t = t;
            previous_visible = is_visible;
        }
        if let Some(start) = run_start {
            let a = p0.lerp(p1, start);
            result.push((
                Point::new(a.x + bounds.x, a.y + bounds.y),
                Point::new(p1.x + bounds.x, p1.y + bounds.y),
            ));
        }
        result
    };

    let mut all_segments: Vec<(Point, Point)> = Vec::new();
    let mut axis_extent = 0.0_f32;

    // A finite LIMITS rectangle replaces the usual viewport/horizon extent.
    // Clip each UCS grid line analytically against the WCS XY rectangle, then
    // project only that finite segment. This keeps the grid bounded even when
    // the active UCS is rotated.
    if let Some((limit_min, limit_max)) = limits {
        let corners = [
            glam::DVec3::new(limit_min.x, limit_min.y, grid_origin.z),
            glam::DVec3::new(limit_max.x, limit_min.y, grid_origin.z),
            glam::DVec3::new(limit_max.x, limit_max.y, grid_origin.z),
            glam::DVec3::new(limit_min.x, limit_max.y, grid_origin.z),
        ];
        let coordinate_range = |axis: Vec3| {
            corners
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), corner| {
                    let value = (*corner - grid_origin).as_vec3().dot(axis);
                    (min.min(value), max.max(value))
                })
        };
        let clip_world_line = |family: usize, value: f32| -> Option<(Point, Point)> {
            let (base, direction) = if family == 0 {
                (grid_origin + (axis1 * value).as_dvec3(), axis2.as_dvec3())
            } else {
                (grid_origin + (axis2 * value).as_dvec3(), axis1.as_dvec3())
            };
            let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
            let mut clip_axis = |origin: f64, delta: f64, low: f64, high: f64| {
                if delta.abs() < 1e-12 {
                    return origin >= low && origin <= high;
                }
                let a = (low - origin) / delta;
                let b = (high - origin) / delta;
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
                t0 <= t1
            };
            if !clip_axis(base.x, direction.x, limit_min.x, limit_max.x)
                || !clip_axis(base.y, direction.y, limit_min.y, limit_max.y)
                || !t0.is_finite()
                || !t1.is_finite()
            {
                return None;
            }
            let p0 = project(base + direction * t0)?;
            let p1 = project(base + direction * t1)?;
            let local_bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: bounds.width,
                height: bounds.height,
            };
            clip_seg(p0, p1, local_bounds).map(|(p0, p1)| {
                (
                    Point::new(p0.x + bounds.x, p0.y + bounds.y),
                    Point::new(p1.x + bounds.x, p1.y + bounds.y),
                )
            })
        };

        let (min1, max1) = coordinate_range(axis1);
        let (min2, max2) = coordinate_range(axis2);
        let mut segments = Vec::new();
        if let Some((_, anchor_world, gap)) = best_anchor(0) {
            if gap >= MIN_HORIZON_GRID_PX {
                let anchor = (anchor_world - grid_origin).as_vec3().dot(axis1);
                let (start, end) = line_range(min1, max1, anchor);
                for index in start..=end {
                    if let Some(segment) = clip_world_line(0, index as f32 * s) {
                        segments.push(segment);
                    }
                }
            }
        }
        if let Some((_, anchor_world, gap)) = best_anchor(1) {
            if gap >= MIN_HORIZON_GRID_PX {
                let anchor = (anchor_world - grid_origin).as_vec3().dot(axis2);
                let (start, end) = line_range(min2, max2, anchor);
                for index in start..=end {
                    if let Some(segment) = clip_world_line(1, index as f32 * s) {
                        segments.push(segment);
                    }
                }
            }
        }
        all_segments.extend(segments);

        // LIMITS bounds the grid, not the UCS axes. Size the axes from the
        // visible grid plane so X/Y/Z still span the viewport even when the
        // finite grid rectangle is small or currently off-screen.
        let limits_extent = samples.iter().fold(0.0_f32, |extent, (_, world)| {
            let delta = (*world - grid_origin).as_vec3();
            extent
                .max(delta.dot(axis1).abs())
                .max(delta.dot(axis2).abs())
        });
        if limits_extent > 0.0 {
            axis_extent = limits_extent;
        }
        return GridGeometry { segments: all_segments, axis_extent };
    }

    // Lines parallel to axis2 (varying axis1 position).
    if let Some((anchor_screen, anchor_world, gap)) = best_anchor(0) {
        if gap >= MIN_HORIZON_GRID_PX {
            let hits = collect_extent(0, anchor_screen, anchor_world);
            if let (Some((min1, max1)), Some((min2, max2))) =
                (axis_range(&hits, axis1), axis_range(&hits, axis2))
            {
                let anchor1 = (anchor_world - grid_origin).as_vec3().dot(axis1);
                let (start, end) = line_range(min1, max1, anchor1);
                let mut segments = Vec::with_capacity((end - start + 1).max(0) as usize);
                for i in start..=end {
                    let value = i as f32 * s;
                    if let Some((p0, p1)) = project_line(0, value) {
                        segments.extend(trim_line(0, p0, p1));
                    }
                }
                all_segments.extend(segments);
                axis_extent =
                    axis_extent.max(min1.abs().max(max1.abs()).max(min2.abs()).max(max2.abs()));
            }
        }
    }

    // Lines parallel to axis1 (varying axis2 position).
    if let Some((anchor_screen, anchor_world, gap)) = best_anchor(1) {
        if gap >= MIN_HORIZON_GRID_PX {
            let hits = collect_extent(1, anchor_screen, anchor_world);
            if let (Some((min1, max1)), Some((min2, max2))) =
                (axis_range(&hits, axis1), axis_range(&hits, axis2))
            {
                let anchor2 = (anchor_world - grid_origin).as_vec3().dot(axis2);
                let (start, end) = line_range(min2, max2, anchor2);
                let mut segments = Vec::with_capacity((end - start + 1).max(0) as usize);
                for i in start..=end {
                    let value = i as f32 * s;
                    if let Some((p0, p1)) = project_line(1, value) {
                        segments.extend(trim_line(1, p0, p1));
                    }
                }
                all_segments.extend(segments);
                axis_extent =
                    axis_extent.max(min1.abs().max(max1.abs()).max(min2.abs()).max(max2.abs()));
            }
        }
    }

    let _ = gz; // gz unused after move; retained for symmetry with `draw_axes` call sites.
    GridGeometry { segments: all_segments, axis_extent }
}

// ── Coloured UCS axes ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_axes(
    frame: &mut canvas::Frame,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    extent: f32,
    origin: glam::DVec3,
    axes: (Vec3, Vec3, Vec3),
    bg_luminance: f32,
) {
    let w2s = |world: glam::DVec3| -> Point {
        let ndc = view_rot.project_point3((world - eye).as_vec3());
        Point::new(
            bounds.x + (ndc.x + 1.0) * 0.5 * bounds.width,
            bounds.y + (1.0 - ndc.y) * 0.5 * bounds.height,
        )
    };
    let e = extent;
    let (ax, ay, az) = axes;
    let axis_stroke = |r: f32, g: f32, b: f32| canvas::Stroke {
        width: 1.5,
        style: canvas::Style::Solid(Color { r, g, b, a: 0.85 }),
        ..Default::default()
    };
    // Axes run through the UCS origin along the UCS axis directions.
    let mut line = |dir: Vec3, r: f32, g: f32, b: f32| {
        frame.stroke(
            &canvas::Path::new(|p| {
                p.move_to(w2s(origin - (dir * e).as_dvec3()));
                p.line_to(w2s(origin + (dir * e).as_dvec3()));
            }),
            axis_stroke(r, g, b),
        );
    };
    let y_green = if bg_luminance > 0.5 { 0.60 } else { 0.85 };
    line(ax, 0.90, 0.20, 0.20); // X — red
    line(ay, 0.20, y_green, 0.20); // Y — green
    line(az, 0.20, 0.40, 0.90); // Z — blue
}

// ── UCS icon ──────────────────────────────────────────────────────────────
//
// Draws a small X/Y/Z axis tripod in the bottom-left corner of the viewport.
// The axis directions are projected from world space so the icon rotates with
// the camera. Axis lengths are proportional (foreshortening preserved), depth
// ordering is computed from NDC Z, and axes going away from the viewer are
// drawn as outlined circles with reduced opacity.

const UCS_ICON_MARGIN: f32 = 50.0;
const UCS_ICON_LEN: f32 = 38.0; // longest axis arm in screen pixels
const UCS_ICON_TIP: f32 = 7.0; // arrowhead size in pixels
const UCS_GRIP_BOX: f32 = 7.0; // selected-grip square size in pixels

/// One projected UCS axis arm: scaled screen delta from the anchor plus its
/// depth (for back-to-front draw order).
struct IconAxis {
    dx: f32,
    dy: f32,
    sc_len: f32,
    depth: f32,
}

/// Screen positions of the UCS icon grips, present only when the tripod is
/// anchored at the on-screen UCS origin (i.e. draggable). `tips` is X, Y, Z.
pub struct UcsIconHit {
    pub origin: Point,
    pub tips: [Point; 3],
}

/// Shared icon geometry used by both the renderer and the grip hit-test, so the
/// two never drift. Projects the UCS axis directions to screen, picks the
/// anchor (on-screen origin when available, else the corner) and returns each
/// axis's scaled screen delta. The bool is `at_origin` — true when anchored at
/// the projected origin, which is the only state where grips are live.
fn ucs_icon_geometry(
    vp: Mat4,
    bounds: iced::Rectangle,
    axes: (Vec3, Vec3, Vec3),
    origin_screen: Option<Point>,
) -> Option<(Point, bool, [IconAxis; 3])> {
    if bounds.width < 10.0 || bounds.height < 10.0 {
        return None;
    }

    // Transform directions directly. Projecting a point at the eye origin is
    // undefined in perspective views.
    let axis_screen = |axis: Vec3| -> Option<(f32, f32, f32, f32)> {
        let clip = vp * axis.extend(0.0);
        if !clip.x.is_finite() || !clip.y.is_finite() || !clip.z.is_finite() {
            return None;
        }
        let dx = clip.x * 0.5 * bounds.width;
        let dy = -clip.y * 0.5 * bounds.height;
        Some((dx, dy, (dx * dx + dy * dy).sqrt(), clip.z))
    };

    let (ax, ay, az) = axes;
    let (xdx, xdy, xlen, xdepth) = axis_screen(ax)?;
    let (ydx, ydy, ylen, ydepth) = axis_screen(ay)?;
    let (zdx, zdy, zlen, zdepth) = axis_screen(az)?;
    let corner = Point::new(
        bounds.x + UCS_ICON_MARGIN,
        bounds.y + (bounds.height - UCS_ICON_MARGIN).max(UCS_ICON_MARGIN),
    );
    // Snap the tripod to the projected UCS origin when it is on-screen
    // (UCSICON ORigin); otherwise keep it in the corner.
    let (icon_origin, at_origin) = match origin_screen {
        Some(p)
            if p.x >= bounds.x
                && p.x <= bounds.x + bounds.width
                && p.y >= bounds.y
                && p.y <= bounds.y + bounds.height =>
        {
            (p, true)
        }
        _ => (corner, false),
    };

    // Scale so the longest projected axis fills UCS_ICON_LEN; shorter axes
    // stay proportionally shorter (this IS the foreshortening effect).
    let max_len = xlen.max(ylen).max(zlen).max(1e-4);
    let sc = UCS_ICON_LEN / max_len;

    let mk = |dx: f32, dy: f32, len: f32, depth: f32| IconAxis {
        dx: dx * sc,
        dy: dy * sc,
        sc_len: len * sc,
        depth,
    };
    Some((
        icon_origin,
        at_origin,
        [
            mk(xdx, xdy, xlen, xdepth),
            mk(ydx, ydy, ylen, ydepth),
            mk(zdx, zdy, zlen, zdepth),
        ],
    ))
}

/// Screen grip targets for the UCS icon, or `None` when it is pinned to the
/// corner (origin off-screen) and therefore not draggable.
pub fn ucs_icon_hit(
    vp: Mat4,
    bounds: iced::Rectangle,
    axes: (Vec3, Vec3, Vec3),
    origin_screen: Option<Point>,
) -> Option<UcsIconHit> {
    // Grips are available wherever the icon is drawn — including parked in the
    // corner (origin off-screen), where dragging the origin grip relocates it.
    let (o, _at_origin, g) = ucs_icon_geometry(vp, bounds, axes, origin_screen)?;
    Some(UcsIconHit {
        origin: o,
        tips: [
            Point::new(o.x + g[0].dx, o.y + g[0].dy),
            Point::new(o.x + g[1].dx, o.y + g[1].dy),
            Point::new(o.x + g[2].dx, o.y + g[2].dy),
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_ucs_icon(
    frame: &mut canvas::Frame,
    vp: Mat4,
    bounds: iced::Rectangle,
    axes: (Vec3, Vec3, Vec3),
    origin_screen: Option<Point>,
    hover: bool,
    selected: bool,
    crosshair_bg: [f32; 4],
) {
    let Some((icon_origin, at_origin, geom)) =
        ucs_icon_geometry(vp, bounds, axes, origin_screen)
    else {
        return;
    };

    // The icon is interactive wherever it is drawn (including parked in the
    // corner), so hover/selection highlight applies there too.
    let _ = at_origin;
    let highlight = hover || selected;
    let is_light_bg = 0.299 * crosshair_bg[0] + 0.587 * crosshair_bg[1] + 0.114 * crosshair_bg[2] > 0.5;
    let y_green = if is_light_bg { 0.60 } else { 0.85 };

    struct AxisInfo {
        dx: f32,
        dy: f32,
        sc_len: f32,
        depth: f32,
        r: f32,
        g: f32,
        b: f32,
        label: &'static str,
    }
    let mut axes = [
        AxisInfo {
            dx: geom[0].dx,
            dy: geom[0].dy,
            sc_len: geom[0].sc_len,
            depth: geom[0].depth,
            r: 0.90,
            g: 0.22,
            b: 0.22,
            label: "X",
        },
        AxisInfo {
            dx: geom[1].dx,
            dy: geom[1].dy,
            sc_len: geom[1].sc_len,
            depth: geom[1].depth,
            r: 0.22,
            g: y_green,
            b: 0.22,
            label: "Y",
        },
        AxisInfo {
            dx: geom[2].dx,
            dy: geom[2].dy,
            sc_len: geom[2].sc_len,
            depth: geom[2].depth,
            r: 0.22,
            g: 0.45,
            b: 0.90,
            label: "Z",
        },
    ];
    // Back-to-front: draw axis farthest from viewer first.
    axes.sort_by(|a, b| {
        b.depth
            .partial_cmp(&a.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for ax in &axes {
        // On highlight, lerp the axis colour toward white and thicken the shaft
        // so the whole tripod reads as "live".
        let mix = if highlight { 0.45 } else { 0.0 };
        let col = Color {
            r: ax.r + (1.0 - ax.r) * mix,
            g: ax.g + (1.0 - ax.g) * mix,
            b: ax.b + (1.0 - ax.b) * mix,
            a: 1.0,
        };
        let tip = Point::new(icon_origin.x + ax.dx, icon_origin.y + ax.dy);

        // Shaft
        if ax.sc_len > 1.0 {
            let path = canvas::Path::new(|p| {
                p.move_to(icon_origin);
                p.line_to(tip);
            });
            frame.stroke(
                &path,
                canvas::Stroke {
                    width: if highlight { 3.0 } else { 2.0 },
                    style: canvas::Style::Solid(col),
                    line_cap: canvas::LineCap::Butt,
                    ..Default::default()
                },
            );
        }

        // Filled arrowhead at tip.
        if ax.sc_len > 3.0 {
            let (nx, ny) = if ax.sc_len > 1e-3 {
                (ax.dx / ax.sc_len, ax.dy / ax.sc_len)
            } else {
                (1.0, 0.0)
            };
            let px = -ny;
            let py = nx;
            let tl = Point::new(
                tip.x - nx * UCS_ICON_TIP + px * (UCS_ICON_TIP * 0.45),
                tip.y - ny * UCS_ICON_TIP + py * (UCS_ICON_TIP * 0.45),
            );
            let tr = Point::new(
                tip.x - nx * UCS_ICON_TIP - px * (UCS_ICON_TIP * 0.45),
                tip.y - ny * UCS_ICON_TIP - py * (UCS_ICON_TIP * 0.45),
            );
            let arrow = canvas::Path::new(|p| {
                p.move_to(tip);
                p.line_to(tl);
                p.line_to(tr);
                p.close();
            });
            frame.fill(&arrow, col);
        }

        // Axis label (X / Y / Z) beyond the tip.
        if ax.sc_len > 4.0 {
            let (nx, ny) = if ax.sc_len > 1e-3 {
                (ax.dx / ax.sc_len, ax.dy / ax.sc_len)
            } else {
                (1.0, 0.0)
            };
            frame.fill_text(canvas::Text {
                content: ax.label.to_string(),
                // Offset beyond tip along the axis direction; subtract ~half glyph
                // size to visually center the single character on the axis line.
                position: Point::new(tip.x + nx * 8.0 - 3.5, tip.y + ny * 8.0 - 5.0),
                color: col,
                size: iced::Pixels(10.0),
                shaping: iced::advanced::text::Shaping::Advanced,
                ..Default::default()
            });
        }
    }

    // Origin dot.
    let circle = canvas::Path::circle(icon_origin, 3.5);
    let dot_lum = 0.299 * crosshair_bg[0] + 0.587 * crosshair_bg[1] + 0.114 * crosshair_bg[2];
    let dot_color = if dot_lum > 0.5 {
        Color {
            r: 0.15,
            g: 0.15,
            b: 0.15,
            a: 0.95,
        }
    } else {
        Color {
            r: 0.9,
            g: 0.9,
            b: 0.9,
            a: 0.95,
        }
    };
    frame.fill(&circle, dot_color);

    // Draggable grips when selected: a square at the origin and at the X / Y
    // tips. Warm grip colour with a light border, like an entity grip.
    if selected {
        let x_tip = Point::new(icon_origin.x + geom[0].dx, icon_origin.y + geom[0].dy);
        let y_tip = Point::new(icon_origin.x + geom[1].dx, icon_origin.y + geom[1].dy);
        let fill = Color {
            r: 0.20,
            g: 0.85,
            b: 0.95,
            a: 1.0,
        };
        let border = Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.9,
        };
        for c in [icon_origin, x_tip, y_tip] {
            let h = UCS_GRIP_BOX / 2.0;
            let sq = canvas::Path::rectangle(
                Point::new(c.x - h, c.y - h),
                iced::Size::new(UCS_GRIP_BOX, UCS_GRIP_BOX),
            );
            frame.fill(&sq, fill);
            frame.stroke(
                &sq,
                canvas::Stroke {
                    width: 1.0,
                    style: canvas::Style::Solid(border),
                    ..Default::default()
                },
            );
        }
    }
}

// ── Dynamic Input overlay ─────────────────────────────────────────────────

use crate::command::{DynGuide, DynRole};

const DYN_OFFSET_X: f32 = 14.0;
const DYN_PAD: f32 = 4.0;
const DYN_GAP: f32 = 6.0;
const DYN_FONT: f32 = 11.0;
const DYN_CHAR_W: f32 = DYN_FONT * 0.62;
const DYN_BOX_H: f32 = DYN_FONT + DYN_PAD * 2.0;

/// One value box in the dynamic-input overlay. Its `role` drives both the
/// label and where the box is placed relative to the step's guide geometry.
#[derive(Clone)]
pub struct DynBox {
    pub label: String,
    pub value: String,
    /// TAB-focused box — keystrokes edit this one.
    pub active: bool,
    /// User has typed a value (the box no longer tracks the cursor).
    pub locked: bool,
    pub role: DynRole,
}

pub fn dynamic_input_overlay<'a>(
    cursor_screen: Point,
    base_screen: Option<Point>,
    ref_screen: Option<Point>,
    label_screen: Option<Point>,
    guide: DynGuide,
    boxes: Vec<DynBox>,
    prompt: String,
    tracking_hint: Option<String>,
) -> Element<'a, Message> {
    canvas(DynInputCanvas {
        cursor_screen,
        base_screen,
        ref_screen,
        label_screen,
        guide,
        boxes,
        prompt,
        tracking_hint,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct DynInputCanvas {
    cursor_screen: Point,
    /// Step anchor in viewport pixels (projected `dyn_anchor`). Guided layouts
    /// (polar / radius / axis-delta) need it; `None` falls back to a cursor row.
    base_screen: Option<Point>,
    /// Far end of the reference line (projected `dyn_ref`) — for `Perp`.
    ref_screen: Option<Point>,
    /// Command-supplied world-space label point projected by the active camera.
    label_screen: Option<Point>,
    guide: DynGuide,
    boxes: Vec<DynBox>,
    /// The active command's current prompt, drawn just above the boxes.
    prompt: String,
    /// Active tracking-reference label.
    tracking_hint: Option<String>,
}

impl DynInputCanvas {
    fn dotted(theme: &Theme) -> canvas::Stroke<'static> {
        canvas::Stroke {
            width: 1.0,
            style: canvas::Style::Solid(
                theme.palette().background.neutral.color.scale_alpha(0.9)
            ),
            line_dash: canvas::LineDash { segments: &[2.0, 3.0], offset: 0 },
            ..Default::default()
        }
    }

    fn box_content(b: &DynBox) -> String {
        match b.role {
            DynRole::Angle => format!("{}\u{00B0}", b.value),
            _ if b.label.is_empty() => b.value.clone(),
            _ => format!("{}{}", b.label, b.value),
        }
    }

    fn box_width(b: &DynBox) -> f32 {
        (Self::box_content(b).len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0
    }

    /// Draw a value box centred at `center`, clamped inside `bounds`.
    fn draw_box(
        frame: &mut canvas::Frame,
        b: &DynBox,
        center: Point,
        bounds: iced::Rectangle,
        theme: &Theme,
    ) {
        let content = Self::box_content(b);
        let w = Self::box_width(b);
        let x = (center.x - w * 0.5).clamp(0.0, (bounds.width - w).max(0.0));
        let y = (center.y - DYN_BOX_H * 0.5).clamp(0.0, (bounds.height - DYN_BOX_H).max(0.0));
        let rect = canvas::Path::rectangle(Point { x, y }, Size { width: w, height: DYN_BOX_H });
        let (fill, border, text) = Self::box_colors(b, theme);
        frame.fill(&rect, fill);
        frame.stroke(
            &rect,
            canvas::Stroke::default()
                .with_color(border)
                .with_width(if b.active { 1.6 } else { 1.0 }),
        );
        frame.fill_text(canvas::Text {
            content,
            position: Point { x: x + DYN_PAD, y: y + DYN_PAD },
            color: text,
            size: iced::Pixels(DYN_FONT),
            // Force Advanced shaping: the default `Auto` uses Basic shaping for
            // ASCII-only strings, which the web (wgpu/webgl) backend fails to
            // render — so all-digit value boxes came up blank while the angle
            // box (containing the non-ASCII `°`) rendered. (#117)
            shaping: iced::advanced::text::Shaping::Advanced,
            ..Default::default()
        });
    }

    fn box_colors(b: &DynBox, theme: &Theme) -> (Color, Color, Color) {
        let palette = theme.palette();
        if b.active {
            (
                palette.primary.weak.color,
                palette.primary.base.color,
                palette.primary.weak.text,
            )
        } else if b.locked {
            (
                palette.warning.weak.color,
                palette.warning.base.color,
                palette.warning.weak.text,
            )
        } else {
            (
                palette.background.weak.color,
                palette.background.neutral.color,
                palette.background.weak.text,
            )
        }
    }

    /// Prompt pill at `pos`.
    fn draw_prompt(&self, frame: &mut canvas::Frame, pos: Point, theme: &Theme) {
        if self.prompt.is_empty() {
            return;
        }
        let palette = theme.palette();
        let pw = (self.prompt.len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0;
        let rect = canvas::Path::rectangle(pos, Size { width: pw, height: DYN_BOX_H });
        frame.fill(&rect, palette.background.strong.color);
        frame.stroke(
            &rect,
            canvas::Stroke::default()
                .with_color(palette.primary.base.color.scale_alpha(0.9))
                .with_width(1.0),
        );
        frame.fill_text(canvas::Text {
            content: self.prompt.clone(),
            position: Point { x: pos.x + DYN_PAD, y: pos.y + DYN_PAD },
            color: palette.background.strong.text,
            size: iced::Pixels(DYN_FONT),
            shaping: iced::advanced::text::Shaping::Advanced,
            ..Default::default()
        });
    }

    fn draw_tracking_hint(
        &self,
        frame: &mut canvas::Frame,
        pos: Point,
        theme: &Theme,
    ) {
        let Some(text) = self.tracking_hint.as_deref() else {
            return;
        };

        let palette = theme.palette();
        let pw = (text.len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0;

        let rect = canvas::Path::rectangle(
            pos,
            Size {
                width: pw,
                height: DYN_BOX_H,
            },
        );

        frame.fill(&rect, palette.background.strong.color);

        frame.stroke(
            &rect,
            canvas::Stroke::default()
                .with_color(palette.primary.base.color.scale_alpha(0.9))
                .with_width(1.0),
        );

        frame.fill_text(canvas::Text {
            content: text.to_string(),
            position: Point {
                x: pos.x + DYN_PAD,
                y: pos.y + DYN_PAD,
            },
            color: palette.background.strong.text,
            size: iced::Pixels(DYN_FONT),
            shaping: iced::advanced::text::Shaping::Advanced,
            ..Default::default()
        });
    }

    /// Guided layout: draw the guide geometry anchored at `base`, then place
    /// each box according to its role.
    fn draw_guided(
        &self,
        frame: &mut canvas::Frame,
        bounds: iced::Rectangle,
        base: Point,
        theme: &Theme,
    ) {
        let cursor_raw = self.cursor_screen;
        let (vx, vy) = (cursor_raw.x - base.x, cursor_raw.y - base.y);
        let raw_len = (vx * vx + vy * vy).sqrt().max(1.0);
        let (dx, dy) = (vx / raw_len, vy / raw_len);
        // Clamp the drawn length to just past the viewport.
        //
        // These guides are dotted, and a dash pattern is measured in PIXELS: the
        // tessellator emits a quad every few pixels, so the cost scales with a
        // guide's screen length, not with what it represents. Typing a large
        // distance (5000000 into the value box) projects the cursor millions of
        // pixels away — millions of quads, gigabytes of them — and the process
        // dies building a frame whose content is off-screen anyway (#406).
        //
        // Everything past the viewport edge is invisible, so dropping it costs
        // nothing: `dx`/`dy` keep the true direction, the angle and every
        // read-out are computed from `raw_len` above, and only the tail nobody
        // can see is cut. Clamping `cursor` here bounds every guide below —
        // they all derive from it.
        let len = raw_len.min(bounds.width.hypot(bounds.height) * 1.5);
        let cursor = Point {
            x: base.x + dx * len,
            y: base.y + dy * len,
        };
        // Perpendicular pointing to the lower half so labels sit under the line.
        let (mut nx, mut ny) = (-dy, dx);
        if ny < 0.0 {
            nx = -nx;
            ny = -ny;
        }
        // Polar arc reference direction: a supplied reference point (e.g. the
        // ROTATE reference), else the +X axis. The arc sweeps the short way
        // from that reference to the cursor.
        let a_cur = dy.atan2(dx);
        let a_ref = self
            .ref_screen
            .map(|r| (r.y - base.y).atan2(r.x - base.x))
            .unwrap_or(0.0);
        let mut sweep = a_cur - a_ref;
        while sweep > std::f32::consts::PI {
            sweep -= std::f32::consts::TAU;
        }
        while sweep <= -std::f32::consts::PI {
            sweep += std::f32::consts::TAU;
        }
        let corner = Point { x: cursor.x, y: base.y }; // axis-delta elbow

        // Perp / PerpDim: perpendicular direction to the reference line, the
        // measured endpoint along it (`end`), and an offset dimension segment
        // (`off_base`→`off_end`) drawn clear of the edge for PerpDim.
        let perp_info = self.ref_screen.map(|r| {
            let (ax, ay) = (r.x - base.x, r.y - base.y);
            let al = (ax * ax + ay * ay).sqrt().max(1.0);
            let (ux, uy) = (ax / al, ay / al); // axis unit (base → ref)
            let (px, py) = (-uy, ux); // perpendicular unit
            let signed = (cursor.x - base.x) * px + (cursor.y - base.y) * py;
            let end = Point { x: base.x + px * signed, y: base.y + py * signed };
            const OFF: f32 = 16.0; // dimension offset, away from the reference
            let off_base = Point { x: base.x - ux * OFF, y: base.y - uy * OFF };
            let off_end = Point { x: end.x - ux * OFF, y: end.y - uy * OFF };
            (end, off_base, off_end)
        });

        // ── Guide geometry ──
        match self.guide {
            DynGuide::Polar => {
                // Reference line along `a_ref` (the +X axis, or the supplied
                // reference direction), then the arc from it to the cursor.
                let href = canvas::Path::new(|p| {
                    p.move_to(base);
                    p.line_to(Point {
                        x: base.x + a_ref.cos() * len,
                        y: base.y + a_ref.sin() * len,
                    });
                });
                frame.stroke(&href, Self::dotted(theme));
                let arc = canvas::Path::new(|p| {
                    let steps = 48;
                    for k in 0..=steps {
                        let a = a_ref + sweep * (k as f32 / steps as f32);
                        let pt = Point {
                            x: base.x + a.cos() * len,
                            y: base.y + a.sin() * len,
                        };
                        if k == 0 {
                            p.move_to(pt);
                        } else {
                            p.line_to(pt);
                        }
                    }
                });
                frame.stroke(&arc, Self::dotted(theme));
            }
            DynGuide::Radius => {
                let line = canvas::Path::new(|p| {
                    p.move_to(base);
                    p.line_to(cursor);
                });
                frame.stroke(&line, Self::dotted(theme));
            }
            DynGuide::Perp => {
                if let Some((end, _, _)) = perp_info {
                    // The measured semi-axis: anchor → perpendicular endpoint.
                    let line = canvas::Path::new(|p| {
                        p.move_to(base);
                        p.line_to(end);
                    });
                    frame.stroke(&line, Self::dotted(theme));
                }
            }
            DynGuide::PerpDim => {
                if let Some((end, ob, oe)) = perp_info {
                    // Dimension segment offset off the edge, with extension
                    // lines back to the two measured corners.
                    let dim = canvas::Path::new(|p| {
                        p.move_to(ob);
                        p.line_to(oe);
                    });
                    frame.stroke(&dim, Self::dotted(theme));
                    let ext = canvas::Path::new(|p| {
                        p.move_to(base);
                        p.line_to(ob);
                        p.move_to(end);
                        p.line_to(oe);
                    });
                    frame.stroke(&ext, Self::dotted(theme));
                }
            }
            DynGuide::AxisDelta | DynGuide::RectSides => {
                // Dotted legs from the anchor along its axes to the cursor.
                let legs = canvas::Path::new(|p| {
                    p.move_to(base);
                    p.line_to(corner);
                    p.line_to(cursor);
                });
                frame.stroke(&legs, Self::dotted(theme));
                if self.guide == DynGuide::RectSides {
                    // Close the rectangle so both side pairs read as a box.
                    let rest = canvas::Path::new(|p| {
                        p.move_to(base);
                        p.line_to(Point { x: base.x, y: cursor.y });
                        p.line_to(cursor);
                    });
                    frame.stroke(&rest, Self::dotted(theme));
                }
            }
            DynGuide::None => {}
        }

        // ── Box placement by role ──
        for b in &self.boxes {
            let center = match b.role {
                DynRole::Angle => self.label_screen.unwrap_or_else(|| {
                    let a_mid = a_ref + sweep * 0.5;
                    let r = (len - DYN_BOX_H * 2.0).max(len * 0.5);
                    Point {
                        x: base.x + a_mid.cos() * r - nx * 18.0,
                        y: base.y + a_mid.sin() * r - ny * 18.0,
                    }
                }),
                DynRole::X | DynRole::Width => Point {
                    x: (base.x + cursor.x) * 0.5,
                    y: base.y + 14.0,
                },
                DynRole::Y | DynRole::Height => Point {
                    x: corner.x + 18.0,
                    y: (base.y + cursor.y) * 0.5,
                },
                // Perpendicular measure: on the measured segment / dim line.
                _ if matches!(self.guide, DynGuide::Perp | DynGuide::PerpDim)
                    && perp_info.is_some() =>
                {
                    let (end, ob, oe) = perp_info.unwrap();
                    if self.guide == DynGuide::PerpDim {
                        Point { x: (ob.x + oe.x) * 0.5 + 8.0, y: (ob.y + oe.y) * 0.5 }
                    } else {
                        Point { x: (base.x + end.x) * 0.5 + 8.0, y: (base.y + end.y) * 0.5 }
                    }
                }
                // Distance / Radius / Diameter and anything else ride the line.
                _ => Point {
                    x: base.x + dx * len * 0.5 + nx * 16.0,
                    y: base.y + dy * len * 0.5 + ny * 16.0,
                },
            };
            Self::draw_box(frame, b, center, bounds, theme);
        }
        // Keep the tracking hint near the crosshair in guided layouts.
        if let Some(text) = self.tracking_hint.as_deref() {
            let hw = (text.len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0;

            let mut hx = self.cursor_screen.x + DYN_OFFSET_X;
            let mut hy = self.cursor_screen.y + DYN_OFFSET_X;

            if hx + hw > bounds.width {
                hx = (self.cursor_screen.x - hw - 4.0).max(0.0);
            }

            if hy + DYN_BOX_H > bounds.height {
                hy = (self.cursor_screen.y - DYN_BOX_H - 4.0).max(0.0);
            }

            self.draw_tracking_hint(
                frame,
                Point {
                    x: hx,
                    y: hy,
                },
                theme,
            );
        }
    }

    /// Fallback row layout near the cursor (no anchor / `None` guide).
    fn draw_row(&self, frame: &mut canvas::Frame, bounds: iced::Rectangle, theme: &Theme) {
        let texts: Vec<String> = self
            .boxes
            .iter()
            .map(|b| {
                if b.label.is_empty() {
                    b.value.clone()
                } else {
                    format!("{}:{}", b.label, b.value)
                }
            })
            .collect();
        let widths: Vec<f32> = texts
            .iter()
            .map(|t| (t.len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0)
            .collect();
        let total_w: f32 =
            widths.iter().sum::<f32>() + DYN_GAP * (self.boxes.len() as f32 - 1.0);

        // Offset the block off the crosshair by the same gap horizontally and
        // vertically; the prompt sits a gap below the horizontal axis and the
        // value boxes a further gap below the prompt.
        let pad = DYN_OFFSET_X;
        let has_prompt = !self.prompt.is_empty();
        let prompt_w = (self.prompt.len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0;
        let block_w = total_w.max(if has_prompt { prompt_w } else { 0.0 });
        let mut bx = self.cursor_screen.x + pad;
        let mut py = self.cursor_screen.y + pad;
        let mut by = if has_prompt { py + DYN_BOX_H + pad } else { py };
        if bx + block_w > bounds.width {
            bx = (self.cursor_screen.x - block_w - 4.0).max(0.0);
        }
        if by + DYN_BOX_H > bounds.height {
            // Flip the block above the cursor, keeping the same gaps.
            by = (self.cursor_screen.y - pad - DYN_BOX_H).max(0.0);
            py = (by - pad - DYN_BOX_H).max(0.0);
        }
        if has_prompt {
            self.draw_prompt(frame, Point { x: bx, y: py }, theme);
        }

        let mut x = bx;
        for (i, b) in self.boxes.iter().enumerate() {
            let w = widths[i];
            if b.role == DynRole::Angle {
                if let Some(center) = self.label_screen {
                    Self::draw_box(frame, b, center, bounds, theme);
                    x += w + DYN_GAP;
                    continue;
                }
            }
            let rect =
                canvas::Path::rectangle(Point { x, y: by }, Size { width: w, height: DYN_BOX_H });
            let (fill, border, text) = Self::box_colors(b, theme);
            frame.fill(&rect, fill);
            frame.stroke(
                &rect,
                canvas::Stroke::default()
                    .with_color(border)
                    .with_width(if b.active { 1.6 } else { 1.0 }),
            );
            frame.fill_text(canvas::Text {
                content: texts[i].clone(),
                position: Point { x: x + DYN_PAD, y: by + DYN_PAD },
                color: text,
                size: iced::Pixels(DYN_FONT),
                shaping: iced::advanced::text::Shaping::Advanced,
                ..Default::default()
            });
            x += w + DYN_GAP;
        }
        // Place the tracking hint below the value row.
        if self.tracking_hint.is_some() {
            let mut hy = by + DYN_BOX_H + 3.0;

            // Move above when bottom space is insufficient.
            if hy + DYN_BOX_H > bounds.height {
                hy = (py - DYN_BOX_H - 3.0).max(0.0);
            }

            self.draw_tracking_hint(
                frame,
                Point {
                    x: bx,
                    y: hy,
                },
                theme,
            );
        }
    }
}

impl canvas::Program<Message> for DynInputCanvas {
    type State = ();

    fn mouse_interaction(
        &self,
        _state: &(),
        _bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::None
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // No boxes — just the prompt pill near the cursor.
        if self.boxes.is_empty() {
            if !self.prompt.is_empty() {
                let pw = (self.prompt.len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0;
                let mut px = self.cursor_screen.x + DYN_OFFSET_X;
                let mut py = self.cursor_screen.y + DYN_OFFSET_X;
                if px + pw > bounds.width {
                    px = (self.cursor_screen.x - pw - 4.0).max(0.0);
                }
                if py + DYN_BOX_H > bounds.height {
                    py = (self.cursor_screen.y - DYN_BOX_H - 4.0).max(0.0);
                }
                self.draw_prompt(&mut frame, Point { x: px, y: py }, theme);

                if self.tracking_hint.is_some() {
                    let hint_y = py + DYN_BOX_H + 3.0;

                    self.draw_tracking_hint(
                        &mut frame,
                        Point {
                            x: px,
                            y: hint_y,
                        },
                        theme,
                    );
                }
            }
            return vec![frame.into_geometry()];
        }

        // Guided layouts need the anchor; without it fall back to a cursor row.
        match (self.guide, self.base_screen) {
            (DynGuide::None, _) | (_, None) => self.draw_row(&mut frame, bounds, theme),
            (_, Some(base)) => self.draw_guided(&mut frame, bounds, base, theme),
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod clip_tests {
    use super::*;

    fn b() -> iced::Rectangle {
        iced::Rectangle { x: 0.0, y: 0.0, width: 800.0, height: 600.0 }
    }

    /// The #406 shape: a guide from a point projected millions of pixels away,
    /// back to the cursor. Must survive, keep its direction, and come back short.
    #[test]
    fn clips_a_far_guide_to_a_drawable_length() {
        let far = Point::new(-5_000_000.0, -1_500_000.0);
        let cur = Point::new(400.0, 300.0);
        let (a, c) = clip_seg(far, cur, b()).expect("crosses the viewport");
        let len = ((c.x - a.x).powi(2) + (c.y - a.y).powi(2)).sqrt();
        assert!(len < 2000.0, "clipped guide still {len} px long");
        // Same direction as the original. Compare unit vectors, not a cross
        // product: at these magnitudes (800 x 1.5e6) an f32 cross carries ~1e2
        // of rounding, so only a scale-free check means anything.
        let unit = |dx: f32, dy: f32| {
            let l = (dx * dx + dy * dy).sqrt();
            (dx / l, dy / l)
        };
        let (ux, uy) = unit(cur.x - far.x, cur.y - far.y);
        let (vx, vy) = unit(c.x - a.x, c.y - a.y);
        assert!(
            (ux - vx).abs() < 1e-3 && (uy - vy).abs() < 1e-3,
            "clip changed the direction"
        );
        // The end inside the viewport is kept as-is.
        assert!((c.x - cur.x).abs() < 0.5 && (c.y - cur.y).abs() < 0.5);
    }

    #[test]
    fn keeps_a_fully_visible_segment_and_drops_a_missing_one() {
        let (a, c) = clip_seg(Point::new(10.0, 10.0), Point::new(700.0, 500.0), b()).unwrap();
        assert!((a.x - 10.0).abs() < 0.01 && (c.x - 700.0).abs() < 0.01);
        assert!(clip_seg(Point::new(-9000.0, -9000.0), Point::new(-8000.0, -8000.0), b()).is_none());
    }
}

#[cfg(test)]
mod bench_grid_geometry_tests {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    /// Benchmarks the pure grid geometry construction (uncached).
    /// Represents a 2-pane tiled Model layout: pane 1 at x=0..1280, pane 2 at
    /// x=1280..1920. Slight tilt, typical eye, step 80 (pane 1) / 160 (pane 2).
    /// RED: requires `grid_segments(...)` which does not exist yet — compilation
    /// must fail with E0425 "cannot find function `grid_segments`". The bench
    /// becomes meaningful at Step 1 once the helper is extracted.
    #[test]
    #[ignore]
    fn bench_grid_geometry_uncached() {
        let view_rot1 = Mat4::from_rotation_x(0.15) * Mat4::from_rotation_y(0.05);
        let eye1 = glam::DVec3::new(4.0, 3.5, 9.0);
        let bounds1 = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 720.0,
        };
        let step1 = 80.0_f32;
        let grid_origin1 = glam::DVec3::new(0.0, 0.0, 0.0);
        let grid_axes1 = (Vec3::X, Vec3::Y, Vec3::Z);
        let limits1: Option<(glam::DVec2, glam::DVec2)> = None;

        let view_rot2 = Mat4::from_rotation_x(0.15) * Mat4::from_rotation_y(0.05);
        let eye2 = glam::DVec3::new(4.0, 3.5, 9.0);
        let bounds2 = iced::Rectangle {
            x: 1280.0,
            y: 0.0,
            width: 640.0,
            height: 720.0,
        };
        let step2 = 160.0_f32;
        let grid_origin2 = glam::DVec3::new(0.0, 0.0, 0.0);
        let grid_axes2 = (Vec3::X, Vec3::Y, Vec3::Z);
        let limits2: Option<(glam::DVec2, glam::DVec2)> = None;

        for _ in 0..20 {
            let _ = black_box(grid_segments(
                black_box(view_rot1),
                black_box(eye1),
                black_box(bounds1),
                black_box(step1),
                black_box(grid_origin1),
                black_box(grid_axes1),
                black_box(limits1),
            ));
            let _ = black_box(grid_segments(
                black_box(view_rot2),
                black_box(eye2),
                black_box(bounds2),
                black_box(step2),
                black_box(grid_origin2),
                black_box(grid_axes2),
                black_box(limits2),
            ));
        }

        let n = 200u32;
        let start = Instant::now();
        for _ in 0..n {
            let _ = black_box(grid_segments(
                black_box(view_rot1),
                black_box(eye1),
                black_box(bounds1),
                black_box(step1),
                black_box(grid_origin1),
                black_box(grid_axes1),
                black_box(limits1),
            ));
            let _ = black_box(grid_segments(
                black_box(view_rot2),
                black_box(eye2),
                black_box(bounds2),
                black_box(step2),
                black_box(grid_origin2),
                black_box(grid_axes2),
                black_box(limits2),
            ));
        }
        let elapsed = start.elapsed();
        let per_frame = elapsed / n;
        println!(
            "grid_segments uncached: {:?} per frame (n = {}, total {:?})",
            per_frame, n, elapsed
        );
        assert!(per_frame.as_secs_f64() > 0.0, "per-frame time must be positive");
    }

    /// A/B partner of `bench_grid_geometry_uncached` (Mission #1, step 6).
    /// Times the hit-path decision only: build `GridKey` from the current
    /// pane params + canvas bounds, borrow the stored key, call
    /// `should_reuse`. Mirrors the body of the hit branch in
    /// `GridCanvas::draw`. Excludes the iced `canvas::Cache` internals
    /// (Arc-clone + draw_with_bounds fast path) because they live in the
    /// fork and are not what we added; measures only the cost we own.
    #[test]
    #[ignore]
    fn bench_grid_geometry_cached() {
        let view_rot1 = Mat4::from_rotation_x(0.15) * Mat4::from_rotation_y(0.05);
        let eye1 = glam::DVec3::new(4.0, 3.5, 9.0);
        let bounds1 = iced::Rectangle { x: 0.0, y: 0.0, width: 1280.0, height: 720.0 };
        let step1 = 80.0_f32;
        let origin1 = glam::DVec3::new(0.0, 0.0, 0.0);
        let axes1 = (Vec3::X, Vec3::Y, Vec3::Z);
        let limits1: Option<(glam::DVec2, glam::DVec2)> = None;

        let view_rot2 = Mat4::from_rotation_x(0.15) * Mat4::from_rotation_y(0.05);
        let eye2 = glam::DVec3::new(4.0, 3.5, 9.0);
        let bounds2 = iced::Rectangle { x: 1280.0, y: 0.0, width: 640.0, height: 720.0 };
        let step2 = 160.0_f32;
        let origin2 = glam::DVec3::new(0.0, 0.0, 0.0);
        let axes2 = (Vec3::X, Vec3::Y, Vec3::Z);
        let limits2: Option<(glam::DVec2, glam::DVec2)> = None;

        let params1 = GridParams {
            view_rot: view_rot1, eye: eye1, bounds: bounds1, step: step1,
            origin: origin1, axes: axes1, limits: limits1,
        };
        let params2 = GridParams {
            view_rot: view_rot2, eye: eye2, bounds: bounds2, step: step2,
            origin: origin2, axes: axes2, limits: limits2,
        };
        let grids = vec![params1, params2];
        // Overall canvas bounds — what `GridCanvas::draw` receives and
        // passes to `GridKey::from_grids`. The 1920×720 covers the two
        // tiled panes (1280 + 640).
        let canvas_bounds = iced::Rectangle { x: 0.0, y: 0.0, width: 1920.0, height: 720.0 };

        // Pre-seed a `GridCanvasState` with the same key the bench will
        // build each iteration — guaranteed hit path.
        let state = GridCanvasState::default();
        let stored_key = GridKey::from_grids(&grids, canvas_bounds, GridStyle::default());
        *state.key.borrow_mut() = Some(stored_key);

        for _ in 0..20 {
            let key = GridKey::from_grids(black_box(&grids), black_box(canvas_bounds), GridStyle::default());
            let hit = should_reuse(state.key.borrow().as_ref(), &key);
            black_box(hit);
        }

        let n = 200u32;
        let start = Instant::now();
        let mut hit_count = 0u32;
        for _ in 0..n {
            let key = GridKey::from_grids(black_box(&grids), black_box(canvas_bounds), GridStyle::default());
            if should_reuse(state.key.borrow().as_ref(), &key) {
                hit_count += 1;
            }
        }
        let elapsed = start.elapsed();
        let per_frame = elapsed / n;
        assert_eq!(hit_count, n, "bench should always hit (sanity)");
        println!(
            "grid key + should_reuse (hit path): {:?} per frame (n = {}, total {:?})",
            per_frame, n, elapsed
        );
        assert!(per_frame.as_secs_f64() > 0.0, "per-frame time must be positive");
    }
}

#[cfg(test)]
mod grid_key_tests {
    use super::*;

    /// Reference `GridParams` used as the baseline for key-construction tests.
    /// Mirrors a representative Model pane: identity-ish view, eye ~3.5m back,
    /// step 80 world units, WCS, no limits. Tests mutate one field at a time
    /// off this baseline to assert that `GridKey` invalidates on every input
    /// change.
    fn baseline_params() -> GridParams {
        GridParams {
            view_rot: Mat4::from_rotation_x(0.15) * Mat4::from_rotation_y(0.05),
            eye: glam::DVec3::new(4.0, 3.5, 9.0),
            bounds: iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 720.0,
            },
            step: 80.0,
            origin: glam::DVec3::new(0.0, 0.0, 0.0),
            axes: (Vec3::X, Vec3::Y, Vec3::Z),
            limits: None,
        }
    }

    /// Same `Vec<GridParams>` + same bounds + same style ⇒ keys compare equal.
    #[test]
    fn grid_key_matches_identical_params() {
        let grids = vec![baseline_params(), baseline_params()];
        let bounds = iced::Rectangle { x: 0.0, y: 0.0, width: 1920.0, height: 720.0 };
        let a = GridKey::from_grids(&grids, bounds, GridStyle::default());
        let b = GridKey::from_grids(&grids, bounds, GridStyle::default());
        assert_eq!(a, b);
    }

    /// One test, one baseline. Every change of any of the inputs must produce
    /// a key that differs from the baseline. This is the entire correctness
    /// contract for cache hit/miss — if any input is ignored, the cache serves
    /// a stale grid.
    #[test]
    fn grid_key_invalidates_on_changed_fields() {
        let baseline_bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 720.0,
        };
        let baseline_grids = vec![baseline_params()];
        let baseline_key = GridKey::from_grids(&baseline_grids, baseline_bounds, GridStyle::default());

        // view_rot: small extra rotation
        let mut p = baseline_params();
        p.view_rot = Mat4::from_rotation_x(0.15 + 0.01) * Mat4::from_rotation_y(0.05);
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "view_rot change must invalidate"
        );

        // eye: shift in z
        let mut p = baseline_params();
        p.eye = glam::DVec3::new(4.0, 3.5, 9.5);
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "eye change must invalidate"
        );

        // step: zoom in
        let mut p = baseline_params();
        p.step = 40.0;
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "step change must invalidate"
        );

        // origin: translate the UCS origin off-zero
        let mut p = baseline_params();
        p.origin = glam::DVec3::new(100.0, 0.0, 0.0);
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "origin change must invalidate"
        );

        // axes: rotate the active UCS
        let mut p = baseline_params();
        p.axes = (Vec3::Y, Vec3::X, Vec3::Z);
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "axes change must invalidate"
        );

        // limits: switch from None to Some
        let mut p = baseline_params();
        p.limits = Some((glam::DVec2::new(0.0, 0.0), glam::DVec2::new(100.0, 100.0)));
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "limits change must invalidate"
        );

        // bounds: same baseline GridParams but a different overlay bounds
        let other_bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 720.0,
        };
        assert_ne!(
            GridKey::from_grids(&baseline_grids, other_bounds, GridStyle::default()),
            baseline_key,
            "bounds change must invalidate"
        );

        // style opacity: change opacity
        let mut style = GridStyle::default();
        style.opacity = 50;
        assert_ne!(
            GridKey::from_grids(&baseline_grids, baseline_bounds, style),
            baseline_key,
            "style opacity change must invalidate"
        );

        // style bg_luminance: change luminance (dark to light)
        let mut style = GridStyle::default();
        style.bg_luminance = 0.9;
        assert_ne!(
            GridKey::from_grids(&baseline_grids, baseline_bounds, style),
            baseline_key,
            "style bg_luminance change must invalidate"
        );
    }

    /// In a 2-pane tiled layout, changing the second pane's params (with pane 1
    /// unchanged) must produce a different key — the cache cannot share geometry
    /// when any pane is dirty.
    #[test]
    fn grid_key_invalidates_when_any_pane_changes() {
        let bounds = iced::Rectangle { x: 0.0, y: 0.0, width: 1920.0, height: 720.0 };
        let pane1 = baseline_params();
        let pane2 = baseline_params();
        let both = vec![pane1.clone(), pane2.clone()];
        let baseline = GridKey::from_grids(&both, bounds, GridStyle::default());

        let mut pane2_changed = pane2;
        pane2_changed.step = 160.0;
        let dirty = vec![pane1, pane2_changed];
        assert_ne!(
            GridKey::from_grids(&dirty, bounds, GridStyle::default()),
            baseline,
            "second pane change must invalidate"
        );
    }

    /// `should_reuse(None, &key)` ⇒ `false` (no cached key to reuse).
    #[test]
    fn should_reuse_empty() {
        let grids = vec![baseline_params()];
        let bounds = iced::Rectangle { x: 0.0, y: 0.0, width: 1920.0, height: 720.0 };
        let key = GridKey::from_grids(&grids, bounds, GridStyle::default());
        assert!(!should_reuse(None, &key));
    }

    /// `should_reuse(Some(&old), &same)` ⇒ `true` (structural equality).
    #[test]
    fn should_reuse_equal() {
        let grids = vec![baseline_params()];
        let bounds = iced::Rectangle { x: 0.0, y: 0.0, width: 1920.0, height: 720.0 };
        let key = GridKey::from_grids(&grids, bounds, GridStyle::default());
        assert!(should_reuse(Some(&key), &key));
    }

    /// `should_reuse(Some(&old), &new)` with keys built from different inputs
    /// ⇒ `false` (must recompute, not serve stale geometry).
    #[test]
    fn should_reuse_changed() {
        let grids_a = vec![baseline_params()];
        let mut pane2 = baseline_params();
        pane2.step = 160.0;
        let grids_b = vec![pane2];
        let bounds = iced::Rectangle { x: 0.0, y: 0.0, width: 1920.0, height: 720.0 };
        let old = GridKey::from_grids(&grids_a, bounds, GridStyle::default());
        let new = GridKey::from_grids(&grids_b, bounds, GridStyle::default());
        assert!(!should_reuse(Some(&old), &new));
    }
}

#[cfg(test)]
mod grid_canvas_state_tests {
    use super::*;

    /// A freshly-defaulted `GridCanvasState` must have no cached key — the
    /// first draw of a session always misses. This pins the `Default` impl
    /// to a usable empty state (no need for the wrapper to special-case it).
    #[test]
    fn default_state_has_no_cached_key() {
        let state = GridCanvasState::default();
        assert!(state.key.borrow().is_none());
    }

    /// After manually storing a key in the state (the same way `draw` will
    /// on a miss), `should_reuse` must return `true` for the stored key and
    /// `false` for a key built from different inputs. This exercises the
    /// state → decision wiring end-to-end without a real iced `Renderer`.
    #[test]
    fn stored_key_is_recognized_by_should_reuse() {
        let view_rot = Mat4::from_rotation_x(0.15) * Mat4::from_rotation_y(0.05);
        let eye = glam::DVec3::new(4.0, 3.5, 9.0);
        let bounds = iced::Rectangle { x: 0.0, y: 0.0, width: 1280.0, height: 720.0 };
        let step = 80.0_f32;
        let grid_origin = glam::DVec3::new(0.0, 0.0, 0.0);
        let grid_axes = (Vec3::X, Vec3::Y, Vec3::Z);
        let limits: Option<(glam::DVec2, glam::DVec2)> = None;

        let params = GridParams {
            view_rot,
            eye,
            bounds,
            step,
            origin: grid_origin,
            axes: grid_axes,
            limits,
        };
        let key = GridKey::from_grids(&[params], bounds, GridStyle::default());

        let state = GridCanvasState::default();
        *state.key.borrow_mut() = Some(key.clone());

        // Same key in the cache and in the request ⇒ reuse.
        assert!(should_reuse(state.key.borrow().as_ref(), &key));

        // Different bounds on the same params ⇒ different key, do not reuse.
        let other_bounds = iced::Rectangle { x: 0.0, y: 0.0, width: 640.0, height: 480.0 };
        let other_key = GridKey::from_grids(&[params], other_bounds, GridStyle::default());
        assert!(!should_reuse(state.key.borrow().as_ref(), &other_key));
    }
}

#[cfg(test)]
mod selection_visual_color_tests {
    use super::*;

    #[test]
    fn default_selection_colors_match_theme_palette() {
        let visual = SelectionVisualOptions::default();
        let dark_canvas = [0.0, 0.0, 0.0, 1.0];

        // Dark theme uses authentic classic CAD green and blue
        let dark_crossing = resolve_selection_base_color(true, &Theme::Dark, &visual, dark_canvas);
        let dark_window = resolve_selection_base_color(false, &Theme::Dark, &visual, dark_canvas);
        assert_eq!(dark_crossing, DEFAULT_CROSSING_COLOR);
        assert_eq!(dark_window, DEFAULT_WINDOW_COLOR);

        // Oxocarbon uses curated vibrant cyan and green
        let (oxo_crossing, oxo_window) = theme_selection_colors(&Theme::Oxocarbon);
        assert_eq!(oxo_crossing, Color::from_rgb(0.26, 0.75, 0.40));
        assert_eq!(oxo_window, Color::from_rgb(0.20, 0.69, 1.00));

        // Dracula uses iconic neon green and cyan
        let (drac_crossing, drac_window) = theme_selection_colors(&Theme::Dracula);
        assert_eq!(drac_crossing, Color::from_rgb(0.31, 0.98, 0.48));
        assert_eq!(drac_window, Color::from_rgb(0.54, 0.91, 0.99));

        // Nord uses aurora green and frost cyan
        let (nord_crossing, nord_window) = theme_selection_colors(&Theme::Nord);
        assert_eq!(nord_crossing, Color::from_rgb(0.64, 0.75, 0.55));
        assert_eq!(nord_window, Color::from_rgb(0.53, 0.75, 0.82));

        // Gruvbox Dark uses bright green and aqua
        let (gruv_crossing, gruv_window) = theme_selection_colors(&Theme::GruvboxDark);
        assert_eq!(gruv_crossing, Color::from_rgb(0.72, 0.73, 0.15));
        assert_eq!(gruv_window, Color::from_rgb(0.51, 0.65, 0.60));

        // All 22 themes must provide non-zero, full-alpha colors
        for theme in Theme::ALL {
            let (c, w) = theme_selection_colors(theme);
            assert_eq!(c.a, 1.0, "Theme {:?} crossing alpha should be 1.0", theme);
            assert_eq!(w.a, 1.0, "Theme {:?} window alpha should be 1.0", theme);
            assert!(c.g > 0.0, "Theme {:?} crossing should have green component", theme);
            assert!(w.b > 0.0 || w.g > 0.0, "Theme {:?} window should have blue/cyan component", theme);
        }
    }

    #[test]
    fn custom_aci_selection_colors_override_defaults() {
        let mut visual = SelectionVisualOptions::default();
        visual.crossing_color = 1; // Red
        visual.window_color = 5; // Blue
        let dark_canvas = [0.0, 0.0, 0.0, 1.0];
        let light_canvas = [1.0, 1.0, 1.0, 1.0];

        for theme in Theme::ALL {
            let crossing_dark = resolve_selection_base_color(true, theme, &visual, dark_canvas);
            let window_dark = resolve_selection_base_color(false, theme, &visual, dark_canvas);
            assert_eq!(crossing_dark, Color::from_rgb8(255, 0, 0));
            assert_eq!(window_dark, Color::from_rgb8(0, 0, 255));

            let crossing_light = resolve_selection_base_color(true, theme, &visual, light_canvas);
            let window_light = resolve_selection_base_color(false, theme, &visual, light_canvas);
            assert_eq!(crossing_light, Color::from_rgb8(255, 0, 0));
            assert_eq!(window_light, Color::from_rgb8(0, 0, 255));
        }
    }

    #[test]
    fn paper_space_on_dark_themes_uses_light_canvas_palette() {
        let visual = SelectionVisualOptions::default();
        let paper_bg = [1.0, 1.0, 1.0, 1.0];
        for theme in Theme::ALL {
            let crossing = resolve_selection_base_color(true, theme, &visual, paper_bg);
            let window = resolve_selection_base_color(false, theme, &visual, paper_bg);
            let c_lum = crate::ui::style::common::wcag_luminance(crossing);
            let w_lum = crate::ui::style::common::wcag_luminance(window);
            assert!(
                c_lum < 0.5,
                "Theme {:?} crossing on paper ({:.3}) must have luminance < 0.5",
                theme,
                c_lum
            );
            assert!(
                w_lum < 0.5,
                "Theme {:?} window on paper ({:.3}) must have luminance < 0.5",
                theme,
                w_lum
            );
        }
    }

    #[test]
    fn classic_dark_model_bg_resolves_dark_canvas_palette() {
        let visual = SelectionVisualOptions::default();
        let classic_dark = [33.0 / 255.0, 40.0 / 255.0, 48.0 / 255.0, 1.0];
        for theme in Theme::ALL {
            let crossing = resolve_selection_base_color(true, theme, &visual, classic_dark);
            let window = resolve_selection_base_color(false, theme, &visual, classic_dark);
            let (expected_crossing, expected_window) = theme_selection_colors(theme);
            assert_eq!(
                crossing, expected_crossing,
                "Theme {:?} crossing on ClassicDark should match theme pair",
                theme
            );
            assert_eq!(
                window, expected_window,
                "Theme {:?} window on ClassicDark should match theme pair",
                theme
            );
        }
    }

    #[test]
    fn custom_light_background_resolves_light_canvas_palette() {
        let visual = SelectionVisualOptions::default();
        let custom_light = [0.95, 0.95, 0.9, 1.0];
        let crossing = resolve_selection_base_color(true, &Theme::Dark, &visual, custom_light);
        let window = resolve_selection_base_color(false, &Theme::Dark, &visual, custom_light);
        assert_eq!(crossing, light_canvas_color(true, &Theme::Dark));
        assert_eq!(window, light_canvas_color(false, &Theme::Dark));
    }

    #[test]
    fn test_selection_fill_alpha_clamping() {
        // Dark canvas preserves user opacity directly clamped to [0.0, 1.0]
        assert_eq!(selection_fill_alpha(0.0, false), 0.0);
        assert!((selection_fill_alpha(50.0, false) - 0.5).abs() < 1e-5);
        assert_eq!(selection_fill_alpha(100.0, false), 1.0);
        assert_eq!(selection_fill_alpha(150.0, false), 1.0);

        // Light canvas applies 1.35x boost and clamps to [0.0, 0.45]
        assert_eq!(selection_fill_alpha(0.0, true), 0.0);
        assert!((selection_fill_alpha(20.0, true) - 0.27).abs() < 1e-4);
        assert!((selection_fill_alpha(50.0, true) - 0.45).abs() < 1e-5);
        assert!((selection_fill_alpha(100.0, true) - 0.45).abs() < 1e-5);
    }
}

