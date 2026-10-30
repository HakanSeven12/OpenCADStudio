//! Viewport overlay widgets.

pub mod authoring;

use std::cell::RefCell;

use glam::{Mat4, Vec3};
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Element, Length, Point, Size, Theme};

use crate::app::settings::{CursorType, IsoPlane};
use crate::app::Message;
use crate::scene::model::object::GripShape;
use crate::scene::SelectionState;

use crate::snap::SnapType;
use std::sync::Arc;

/// Original crosshair geometry retained at the default setting values.
pub const CROSSHAIR_SQ: f32 = 7.5;
pub const CROSSHAIR_ARM: f32 = 60.0;
pub(crate) const TRACKING_DASH_PX: [f32; 2] = [6.0, 4.0];
const DEFAULT_CURSOR_SIZE: i32 = 5;
const DEFAULT_PICK_BOX: i32 = 3;
const DEFAULT_PICK_APERTURE: f32 = 8.0;
const CONSTRAINT_HOVER_MARKER_RADIUS: f32 = 7.0;

use crate::scene::parametric_constraints::{
    glyph_hit_test_entries, glyph_is_compact, glyph_is_fixed, glyph_is_vertical, GlyphEntry,
    GLYPH_SIZE,
};

/// The lock mark a dynamic dimension carries (`DYNAMIC_DIMENSION_GLYPH`).
fn is_dynamic_dimension_glyph(label: &str) -> bool {
    label == crate::scene::parametric_constraints::DYNAMIC_DIMENSION_GLYPH
}

fn draw_smooth_constraint_glyph(frame: &mut canvas::Frame, center: Point, color: Color) {
    let curve = canvas::Path::new(|builder| {
        for step in 0..=16 {
            let x = -7.0 + step as f32 * 0.875;
            let y = 0.012 * x * x * x;
            let point = Point::new(center.x + x, center.y - y);
            if step == 0 {
                builder.move_to(point);
            } else {
                builder.line_to(point);
            }
        }
    });
    frame.stroke(
        &curve,
        canvas::Stroke::default().with_color(color).with_width(1.45),
    );
    frame.fill(&canvas::Path::circle(center, 1.35), color);
}

fn draw_tangent_constraint_glyph(frame: &mut canvas::Frame, center: Point, color: Color) {
    let radius = 4.5;
    let circle_center = Point::new(center.x - 0.3, center.y + 2.75);
    let diagonal = radius * std::f32::consts::FRAC_1_SQRT_2;
    let contact = Point::new(circle_center.x - diagonal, circle_center.y - diagonal);
    let tangent_end = Point::new(contact.x + 8.0, contact.y - 8.0);
    let stroke = canvas::Stroke::default().with_color(color).with_width(1.35);

    frame.stroke(&canvas::Path::circle(circle_center, radius), stroke.clone());
    frame.stroke(&canvas::Path::line(contact, tangent_end), stroke);
}

fn draw_concentric_constraint_glyph(frame: &mut canvas::Frame, center: Point, color: Color) {
    let stroke = canvas::Stroke::default().with_color(color).with_width(1.25);
    frame.stroke(&canvas::Path::circle(center, 4.7), stroke.clone());
    frame.stroke(&canvas::Path::circle(center, 2.15), stroke);
}

/// Strokes a closed four-corner polygon — the outline half of the brush glyph.
fn stroke_quad(frame: &mut canvas::Frame, corners: [Point; 4], stroke: &canvas::Stroke<'_>) {
    let path = canvas::Path::new(|p| {
        p.move_to(corners[0]);
        p.line_to(corners[1]);
        p.line_to(corners[2]);
        p.line_to(corners[3]);
        p.close();
    });
    frame.stroke(&path, stroke.clone());
}

/// MATCHPROP's paintbrush cursor.
///
/// While the user is transferring properties the CAD crosshair is replaced by a
/// paintbrush whose tip sits exactly on the pick point, as the reference does. The
/// body leans up-and-right so it never covers the object being picked.
///
/// Deliberately monochrome and geometric: the crosshair's own colour, a single
/// stroke weight and one filled wedge, so the glyph reads as part of the CAD
/// cursor family rather than clip-art.
fn draw_match_brush(frame: &mut canvas::Frame, tip: Point, color: Color) {
    let inv = std::f32::consts::FRAC_1_SQRT_2;
    // `along` runs up-and-right (handle direction); `across` is perpendicular.
    let along = (inv, -inv);
    let across = (inv, inv);
    let at = |a: f32, w: f32| {
        Point::new(
            tip.x + along.0 * a + across.0 * w,
            tip.y + along.1 * a + across.1 * w,
        )
    };
    let stroke = canvas::Stroke {
        width: 1.25,
        style: canvas::Style::Solid(color),
        line_join: canvas::LineJoin::Miter,
        ..Default::default()
    };

    // Bristles: the only solid mass, so the working end carries the weight and
    // the silhouette still reads at cursor size.
    let bristles = canvas::Path::new(|p| {
        p.move_to(at(0.0, 0.0));
        p.line_to(at(9.0, -3.2));
        p.line_to(at(12.0, -3.2));
        p.line_to(at(12.0, 3.2));
        p.line_to(at(9.0, 3.2));
        p.close();
    });
    frame.fill(&bristles, color);

    // Ferrule and handle stay outline-only: line work, not clip-art.
    stroke_quad(
        frame,
        [at(12.0, -3.2), at(15.5, -3.2), at(15.5, 3.2), at(12.0, 3.2)],
        &stroke,
    );
    stroke_quad(
        frame,
        [at(15.5, -2.3), at(30.0, -1.9), at(30.0, 1.9), at(15.5, 2.3)],
        &stroke,
    );
}

/// The reference bar's padlock: white with a small orange point marker
/// when one point is held, red when a whole curve or segment is.
fn draw_fixed_constraint_glyph(
    frame: &mut canvas::Frame,
    center: Point,
    lock: Color,
    badge: Color,
    point_marker: bool,
) {
    let shackle = canvas::Path::new(|builder| {
        let shackle_center = Point::new(center.x, center.y - 1.6);
        for step in 0..=12 {
            let angle = std::f32::consts::PI * (1.0 - step as f32 / 12.0);
            let point = Point::new(
                shackle_center.x + angle.cos() * 3.1,
                shackle_center.y - angle.sin() * 3.1,
            );
            if step == 0 {
                builder.move_to(Point::new(point.x, center.y - 0.6));
                builder.line_to(point);
            } else {
                builder.line_to(point);
            }
        }
        builder.line_to(Point::new(center.x + 3.1, center.y - 0.6));
    });
    frame.stroke(
        &shackle,
        canvas::Stroke::default().with_color(lock).with_width(1.5),
    );
    let body = canvas::Path::rounded_rectangle(
        Point::new(center.x - 5.0, center.y - 0.8),
        Size::new(10.0, 7.6),
        1.4.into(),
    );
    frame.fill(&body, lock);
    frame.stroke(
        &canvas::Path::line(
            Point::new(center.x, center.y + 1.4),
            Point::new(center.x, center.y + 4.6),
        ),
        canvas::Stroke::default().with_color(badge).with_width(1.3),
    );
    if point_marker {
        frame.stroke(
            &canvas::Path::rectangle(
                Point::new(center.x + 4.0, center.y + 3.6),
                Size::new(4.6, 4.6),
            ),
            canvas::Stroke::default()
                .with_color(Color::from_rgb8(224, 130, 62))
                .with_width(1.2),
        );
    }
}

/// The reference bar's Vertical mark: a red datum line hatched on its left,
/// the constrained direction beside it, plus the orange point marker for a
/// two-point relation.
fn draw_vertical_constraint_glyph(
    frame: &mut canvas::Frame,
    center: Point,
    text: Color,
    point_marker: bool,
) {
    let datum = Color::from_rgb8(214, 76, 76);
    let datum_x = center.x - 2.5;
    let stroke = canvas::Stroke::default().with_color(datum).with_width(1.5);
    frame.stroke(
        &canvas::Path::line(
            Point::new(datum_x, center.y - 6.5),
            Point::new(datum_x, center.y + 6.5),
        ),
        stroke.clone(),
    );
    for step in 0..3 {
        let y = center.y - 4.5 + step as f32 * 4.0;
        frame.stroke(
            &canvas::Path::line(Point::new(datum_x, y), Point::new(datum_x - 4.0, y + 3.5)),
            stroke.clone(),
        );
    }
    frame.stroke(
        &canvas::Path::line(
            Point::new(center.x + 2.5, center.y - 6.0),
            Point::new(center.x + 2.5, center.y + 6.0),
        ),
        canvas::Stroke::default().with_color(text).with_width(1.4),
    );
    if point_marker {
        frame.stroke(
            &canvas::Path::rectangle(
                Point::new(center.x + 4.5, center.y - 2.3),
                Size::new(4.6, 4.6),
            ),
            canvas::Stroke::default()
                .with_color(Color::from_rgb8(224, 130, 62))
                .with_width(1.2),
        );
    }
}

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

/// Which interactive navigation tool is armed. Each one owns the whole
/// viewport and hides the CAD crosshair, but they do not share a cursor: the
/// hand reads as "drag the sheet", which is wrong for a zoom or an orbit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NavCursor {
    #[default]
    None,
    Pan,
    Orbit,
    Zoom,
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
    /// A client pick (`user_select` / `getpoint`) is waiting for the person:
    /// the crosshair arms disappear and a blue pickbox remains — the
    /// screen-level "the client wants YOU to pick" signal.
    pub pick_pending: bool,
    /// MATCHPROP is asking for its source object (hollow pickbox).
    pub matchprop_source_pick: bool,
    /// Keep the MATCHPROP pickbox visible while selecting destination objects.
    pub matchprop_pickbox: bool,
    /// Draw only the pickbox: the active command previews a line that the
    /// full-length arms would cover.
    pub hide_arms: bool,
    /// Keep the arms at the default short length even when CURSORSIZE asks
    /// for full-screen ones: an infinite-line preview through the cursor
    /// must stay visible past them, but the cursor itself must not vanish.
    pub short_arms: bool,
    /// Explicit object-snap marker colour; `None` picks one that reads on
    /// the canvas.
    pub snap_color: Option<[u8; 3]>,
    /// Object-snap marker half-size in pixels (SNAPMARKERSIZE, 1–20).
    pub snap_size: f32,
}

/// Rendering style for the viewport grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridStyle {
    pub opacity: u8,
    pub bg_luminance: f32,
    pub color: [f32; 3],
    /// Draw the coloured X/Y axis lines through the grid origin.
    pub axes: bool,
}

impl Default for GridStyle {
    fn default() -> Self {
        Self {
            opacity: 18,
            bg_luminance: 0.15,
            color: [0.80, 0.80, 0.80],
            axes: true,
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
    pub grip_dynamic: u8,
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
            grip_dynamic: 140,
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
        Theme::Dark => (DEFAULT_CROSSING_COLOR, DEFAULT_WINDOW_COLOR),
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
        _ => (DEFAULT_CROSSING_COLOR, DEFAULT_WINDOW_COLOR),
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
        if let Some((r, g, b)) = codec::types::aci_table::aci_to_rgb(custom) {
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
///
/// `PartialEq` feeds the cached grips layer key: the static grip geometry is
/// only rebuilt when a marker's position, shape, or interaction flag changes.
#[derive(Clone, Debug, PartialEq)]
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
    /// True → a plugin-declared custom grip (API v11): drawn with the
    /// distinct plugin accent so it reads differently from built-in
    /// selection grips.
    pub is_plugin: bool,
    pub is_authoring: bool,
    pub is_dynamic: bool,
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
    /// World-space X spacing (GRIDUNIT X, after adaptive scaling).
    /// Rotation does not affect this value, so orbiting cannot rescale the grid.
    pub step_x: f32,
    /// World-space Y spacing (GRIDUNIT Y, after adaptive scaling).
    pub step_y: f32,
    /// Draw every Nth line as a brighter major line. < 2 disables majors.
    pub major_every: u32,
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
/// without an iced `Renderer`.
#[doc(hidden)]
pub struct GridGeometry {
    pub segments: Vec<(Point, Point)>,
    /// Brighter every-Nth lines (major grid). Stroked separately in `draw_grid`.
    pub major_segments: Vec<(Point, Point)>,
    pub axis_extent: f32,
}

impl GridGeometry {
    /// Empty geometry — no segments drawn, axes suppressed. Returned by the
    /// early-exit branches of `grid_segments` (zero-sized bounds, no visible
    /// samples, non-finite step) so the caller never needs to special-case
    /// the `None` path.
    #[doc(hidden)]
    pub fn empty() -> Self {
        Self {
            segments: Vec::new(),
            major_segments: Vec::new(),
            axis_extent: 0.0,
        }
    }
}

/// Cache key for one pane's grid overlay. Identical `GridParams` plus the
/// identical pane clip rectangle and `GridStyle` ⇒ byte-identical pane
/// geometry; that is the invariant the key encodes.
///
/// Bounds are part of the key, not the only key: a pane can pan/zoom inside a
/// single clip rect, so a bounds-only reuse (iced's `geometry::Cache`) would
/// serve a stale grid. The full `GridParams` is required for correctness.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct GridKey {
    pub grids: Vec<GridParams>,
    pub bounds: iced::Rectangle,
    pub style: GridStyle,
}

impl GridKey {
    /// Build a key from the pane's `GridParams`, its clip rectangle, and the
    /// grid style.
    #[doc(hidden)]
    pub fn from_grids(grids: &[GridParams], bounds: iced::Rectangle, style: GridStyle) -> Self {
        Self {
            grids: grids.to_vec(),
            bounds,
            style,
        }
    }
}

/// Cache hit decision: `true` iff `cached` is `Some` and structurally equal to
/// `new`. Reference-based to avoid moving the (potentially large) `Vec` of
/// per-pane params; the caller borrows from the pane entry on both sides.
#[doc(hidden)]
pub fn should_reuse(cached: Option<&GridKey>, new: &GridKey) -> bool {
    match cached {
        Some(old) => old == new,
        None => false,
    }
}

/// One pane's cached grid geometry plus the key that produced it.
struct PaneCache {
    key: GridKey,
    cache: canvas::Cache<iced::Renderer>,
}

impl PaneCache {
    fn new(key: GridKey) -> Self {
        Self {
            key,
            cache: canvas::Cache::default(),
        }
    }
}

/// `Program::State` for `GridCanvas`: one cache entry per pane so a navigation
/// in one tile only rebuilds that tile's geometry.
///
/// `RefCell<Vec<...>>` because `Program::draw` takes `&self`; the vec is
/// resized to the current pane count on every frame. Pane identity is the vec
/// index — a layout change that moves a pane to another slot changes that
/// slot's key and forces a rebuild rather than replaying stale geometry.
#[doc(hidden)]
#[derive(Default)]
pub struct GridCanvasState {
    panes: RefCell<Vec<Option<PaneCache>>>,
}

impl GridCanvasState {
    /// Match the per-pane storage to the current pane count: drop caches for
    /// panes that no longer exist and leave new panes empty (first draw misses).
    fn resize_panes(&self, count: usize) {
        self.panes.borrow_mut().resize_with(count, || None);
    }

    /// `true` when this pane's stored key still describes the request.
    #[doc(hidden)]
    pub fn pane_reuses(&self, index: usize, key: &GridKey) -> bool {
        self.panes
            .borrow()
            .get(index)
            .and_then(Option::as_ref)
            .is_some_and(|pane| should_reuse(Some(&pane.key), key))
    }

    /// Bench/test seam: install `key` for pane `index` without a renderer.
    /// Decision-only — the pane's cache stays empty, so this exercises
    /// `pane_reuses` but a later `draw_cached` on such a pane would replay
    /// empty geometry.
    #[doc(hidden)]
    pub fn store_pane_key(&self, index: usize, key: GridKey) {
        self.resize_panes(index + 1);
        self.panes.borrow_mut()[index] = Some(PaneCache::new(key));
    }

    /// Arc-clone the cached pane geometry; the fork short-circuits on a bounds
    /// match and never invokes the closure.
    fn draw_cached(
        &self,
        index: usize,
        renderer: &iced::Renderer,
        clip: iced::Rectangle,
    ) -> canvas::Geometry {
        let panes = self.panes.borrow();
        let pane = panes[index]
            .as_ref()
            .expect("pane cache present after pane_reuses");
        pane.cache.draw_with_bounds(renderer, clip, |_frame| {})
    }

    /// Rebuild one pane: store its new key, clear its cache (so the closure
    /// runs even when the clip rect is unchanged) and draw only that pane.
    #[allow(clippy::too_many_arguments)]
    fn draw_pane(
        &self,
        index: usize,
        renderer: &iced::Renderer,
        clip: iced::Rectangle,
        params: &GridParams,
        style: GridStyle,
        key: GridKey,
    ) -> canvas::Geometry {
        let mut panes = self.panes.borrow_mut();
        let pane = panes[index].get_or_insert_with(|| PaneCache::new(key.clone()));
        pane.key = key;
        pane.cache.clear();
        pane.cache.draw_with_bounds(renderer, clip, |frame| {
            draw_grid(
                frame,
                params.view_rot,
                params.eye,
                params.bounds,
                params.step_x,
                params.step_y,
                params.major_every,
                params.origin,
                params.axes,
                params.limits,
                style,
            )
        })
    }
}

/// Compute the adaptive grid step size (world units) from camera zoom.
///
/// Returns the smallest power-of-5 multiple of 1.0 that places grid lines at
/// least `MIN_GRID_PX` pixels apart at the camera pivot. Both orthographic and
/// perspective cameras have the same vertical scale there. Camera rotation is
/// intentionally absent so orbiting cannot rescale the visible grid or snap.
/// World → viewport-local screen via relative-to-eye: subtract the f64 eye
/// first so points near the camera stay precise at UTM-scale coords.
/// Rejects points on/behind the perspective eye plane before dividing by W —
/// without this a behind-camera point wraps to a finite mirrored coordinate
/// (screen centre for on-axis points) and strokes garbage (#46). The guard is
/// shared with picking and snapping through `scene::view::projection`.
fn project_point(
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    world: glam::DVec3,
) -> Option<Point> {
    crate::scene::view::projection::project_screen(view_rot, eye, bounds, world)
}

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
    for (p, q) in [
        (-dx, p0.x - xmin),
        (dx, xmax - p0.x),
        (-dy, p0.y - ymin),
        (dy, ymax - p0.y),
    ] {
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

/// Build a snap-guide end on the acquired screen ray, not on the noisy
/// cursor-to-base line. Returns the normalized ray and its forward tip.
fn extension_guide_tip(
    base: Point,
    snapped: Point,
    acquired_direction: Option<Point>,
) -> Option<(Point, Point)> {
    let delta = acquired_direction.unwrap_or(Point::new(snapped.x - base.x, snapped.y - base.y));
    let len = delta.x.hypot(delta.y);
    if !len.is_finite() || len <= 1e-3 {
        return None;
    }
    let u = Point::new(delta.x / len, delta.y / len);
    let along = (snapped.x - base.x) * u.x + (snapped.y - base.y) * u.y;
    let tip = Point::new(base.x + u.x * (along + 18.0), base.y + u.y * (along + 18.0));
    Some((u, tip))
}

#[cfg(test)]
mod extension_guide_tests {
    use super::*;

    #[test]
    fn acquired_extension_guide_stays_on_its_origin_when_cursor_wanders() {
        let base = Point::new(100.0, 200.0);
        for snap in [
            Point::new(140.0, 202.0),
            Point::new(175.0, 193.0),
            Point::new(240.0, 215.0),
        ] {
            let (unit, tip) =
                extension_guide_tip(base, snap, Some(Point::new(300.0, 0.0))).unwrap();
            assert!((unit.y).abs() < 1e-5);
            assert!((tip.y - base.y).abs() < 1e-5);
            assert!((tip.x - (snap.x + 18.0)).abs() < 1e-5);
        }
    }

    #[test]
    fn extension_guide_fallback_keeps_original_behavior_without_acquired_ray() {
        let base = Point::new(100.0, 200.0);
        let snap = Point::new(130.0, 240.0);
        let (u, tip) = extension_guide_tip(base, snap, None).unwrap();
        assert!((u.x - 0.6).abs() < 1e-5);
        assert!((u.y - 0.8).abs() < 1e-5);
        assert!((tip.x - 140.8).abs() < 1e-4);
        assert!((tip.y - 254.4).abs() < 1e-4);
    }
}

pub fn compute_grid_step(distance: f32, fov_y: f32, bounds: iced::Rectangle) -> f32 {
    compute_grid_steps(1.0, 1.0, distance, fov_y, bounds, true).0
}

/// Adaptive world-space grid spacing for non-uniform GRIDUNIT X/Y.
///
/// BUG FIX: the old `compute_grid_step` hardcoded a 1.0 base, so the
/// Drafting Settings grid spacing had no effect on the display. The base now
/// comes from the user's GRIDUNIT setting. With `adaptive == true`, each axis
/// is scaled up by powers of 5 until neighbouring lines are at least
/// `MIN_GRID_PX` apart; with `adaptive == false` the raw base is returned so
/// the grid resizes exactly as typed.
pub fn compute_grid_steps(
    base_x: f32,
    base_y: f32,
    distance: f32,
    fov_y: f32,
    bounds: iced::Rectangle,
    adaptive: bool,
) -> (f32, f32) {
    let sanitize = |b: f32| {
        if b.is_finite() && b > 0.0 && b <= 1e9 {
            b
        } else {
            1.0
        }
    };
    let bx = sanitize(base_x);
    let by = sanitize(base_y);
    if !adaptive {
        return (bx, by);
    }
    let half_height = distance * (fov_y * 0.5).tan();
    if !half_height.is_finite() || half_height <= 1e-9 || bounds.height <= 0.0 {
        return (bx, by);
    }
    let px_per_unit = bounds.height / (2.0 * half_height);
    if !px_per_unit.is_finite() || px_per_unit <= 0.0 {
        return (bx, by);
    }
    let adapt = |mut s: f32, base: f32| {
        // Grow-only: shrinking below the user's base would silently change the
        // requested GRIDUNIT when zoomed in. Guard the loop so a degenerate
        // px_per_unit can never spin forever. The overflow fallback returns
        // this axis's own base (returning the other axis's base mixed X/Y).
        let mut guard = 0;
        while s * px_per_unit < MIN_GRID_PX && guard < 32 {
            s *= 5.0;
            guard += 1;
            if !s.is_finite() || s > 1e9 {
                return base;
            }
        }
        s
    };
    (adapt(bx, bx), adapt(by, by))
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
///
/// `PartialEq` feeds the cached tracking-marks layer key: two frames show the
/// same crosses only when every acquired point sits on the same pixel.
#[derive(Clone, Debug, PartialEq)]
pub struct OstTrackPoint {
    pub screen: Point,
}

pub fn grid_overlay<'a>(grid: Vec<GridParams>, style: GridStyle) -> Element<'a, Message> {
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
        state.resize_panes(self.grid.len());

        let mut layers = Vec::with_capacity(self.grid.len());
        for (index, g) in self.grid.iter().enumerate() {
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

            // Hit: this pane's params, clip and the style are unchanged, so its
            // cached geometry is still valid. Other panes are unaffected by a
            // change here — that is the per-pane win.
            let key = GridKey::from_grids(std::slice::from_ref(g), clip, self.style);
            let geometry = if state.pane_reuses(index, &key) {
                state.draw_cached(index, renderer, clip)
            } else {
                state.draw_pane(index, renderer, clip, g, self.style, key)
            };
            layers.push(geometry);
        }
        layers
    }
}

pub fn selection_overlay<'a>(
    selection: Arc<RefCell<SelectionState>>,
    snap: Option<(Point, SnapType)>,
    snap_ext_base: Option<Point>,
    snap_ext_base2: Option<Point>,
    snap_ext_dir: Option<Point>,
    grips: Vec<GripMarker>,
    control_polygon: Option<(Vec<Point>, bool)>,
    grip_clip: Option<iced::Rectangle>,
    ucs_icons: Vec<UcsIconParams>,
    ost_points: Vec<OstTrackPoint>,
    otrack_lines: Vec<(Point, Point)>,
    parallel_ref_marker: Option<Point>,
    show_viewcube: bool,
    dividers: Vec<iced::Rectangle>,
    pane_move_rect: Option<iced::Rectangle>,
    pane_drop_rect: Option<iced::Rectangle>,
    nav: NavCursor,
    suppressed: bool,
    hover_locked: bool,
    crosshair_bg: [f32; 4],
    crosshair: CrosshairOptions,
    // MATCHPROP is armed: paintbrush cursor replaces the crosshair.
    show_match_brush: bool,
    selection_visual: SelectionVisualOptions,
    constraint_glyphs: Arc<[GlyphEntry]>,
    // Per-glyph selection flags, parallel to `constraint_glyphs`, applied
    // post-hoc by the view layer — deliberately outside the memoised entries
    // so selection changes never invalidate the placement cache.
    constraint_glyph_selected: Arc<[bool]>,
    constraint_glyph_tooltip: Option<String>,
    constraint_cursor_badge: Option<String>,
    authoring_visibility: Option<(Point, String, iced::Rectangle, bool)>,
    authoring_points: Vec<(Point, Point, String, iced::Rectangle, bool)>,
    authoring_dimensions: Vec<AuthoringDimension>,
    authoring_visual: authoring::AuthoringVisualOptions,
    authoring_feedback: authoring::AuthoringFeedback,
) -> Element<'a, Message> {
    canvas(SelectionCanvas {
        selection,
        snap,
        snap_ext_base,
        snap_ext_base2,
        snap_ext_dir,
        grips,
        control_polygon,
        grip_clip,
        ucs_icons,
        ost_points,
        otrack_lines,
        parallel_ref_marker,
        show_viewcube,
        dividers,
        pane_move_rect,
        pane_drop_rect,
        nav,
        suppressed,
        hover_locked,
        crosshair_bg,
        crosshair,
        show_match_brush,
        selection_visual,
        constraint_glyphs,
        constraint_glyph_selected,
        constraint_glyph_tooltip,
        constraint_cursor_badge,
        authoring_visibility,
        authoring_points,
        authoring_dimensions,
        authoring_visual,
        authoring_feedback,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// One authoring dimension's static drawing batch (witness lines, round/base
/// grips, segments, labels, clip). `PartialEq` feeds the cached authoring
/// layer key so this geometry is only rebuilt when the batch actually changes.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthoringDimension {
    pub selected: bool,
    pub is_action: bool,
    pub lines: Vec<[Point; 2]>,
    pub round_grips: Vec<Point>,
    pub base_grips: Vec<Point>,
    pub lookup_grips: Vec<Point>,
    pub dimensions: Vec<[Point; 2]>,
    pub labels: Vec<(Point, String)>,
    pub grips: Vec<(Point, Option<Point>)>,
    pub clip: iced::Rectangle,
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
    snap_ext_dir: Option<Point>,
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
    /// Active OTRACK alignments, each (acquired tracking point, locked cursor)
    /// in screen space. Drawn as dashed guides extended a little past the
    /// cursor so the extension / tracking line the user snapped to is visible
    /// (#219). An intersection lock contributes both of its crossing vectors,
    /// so the point reads as their meeting rather than a lone guide (#1313).
    otrack_lines: Vec<(Point, Point)>,
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
    /// Which interactive navigation tool is armed. Any of them hides the
    /// crosshair — the viewport is being driven, not drawn on — and each
    /// picks its own cursor in `mouse_interaction`.
    nav: NavCursor,
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
    /// MATCHPROP is armed: the crosshair gives way to a paintbrush cursor
    /// whose tip marks the pick point.
    show_match_brush: bool,
    selection_visual: SelectionVisualOptions,
    /// Memoised constraint-glyph entries (precomputed anchor/outward/label +
    /// layout) shared with the hit-test and tooltip paths — draw reads
    /// `top_left`/`size` directly and never recomputes offsets or clones
    /// labels.
    constraint_glyphs: Arc<[GlyphEntry]>,
    /// Per-glyph selection flags, parallel to `constraint_glyphs`, applied
    /// post-hoc by the view layer (outside the cache).
    constraint_glyph_selected: Arc<[bool]>,
    /// Localized kind name made visible after the app-level hover dwell.
    constraint_glyph_tooltip: Option<String>,
    /// Symbol shown beside the cursor while it targets an entity that already
    /// participates in an enabled geometric constraint.
    constraint_cursor_badge: Option<String>,
    authoring_visibility: Option<(Point, String, iced::Rectangle, bool)>,
    authoring_points: Vec<(Point, Point, String, iced::Rectangle, bool)>,
    authoring_dimensions: Vec<AuthoringDimension>,
    authoring_visual: authoring::AuthoringVisualOptions,
    authoring_feedback: authoring::AuthoringFeedback,
}

/// The move gizmo: an arrow per axis (X red, Y green, Z blue) from the
/// centre grip and a square per axis pair; the hovered or dragged part is
/// yellow.
fn draw_move_gizmo(frame: &mut canvas::Frame, grips: &[GripMarker]) {
    use crate::scene::pick::grip::{gizmo_plane_axes, GIZMO_AXIS_PX};
    const COLORS: [Color; 3] = [
        Color::from_rgb(0.90, 0.22, 0.20),
        Color::from_rgb(0.27, 0.70, 0.29),
        Color::from_rgb(0.18, 0.52, 0.93),
    ];
    let active = Color::from_rgb(1.0, 0.84, 0.0);
    // Unit screen direction (y down) and tip of each shown arrow.
    let mut axes: [Option<(Point, iced::Vector)>; 3] = [None; 3];
    for grip in grips {
        if let (GripShape::GizmoAxis(k), Some([dx, dy])) = (grip.shape, grip.dir) {
            axes[k as usize % 3] = Some((grip.pos, iced::Vector::new(dx, -dy)));
        }
    }
    let Some(center) = axes
        .iter()
        .flatten()
        .next()
        .map(|(tip, d)| Point::new(tip.x - d.x * GIZMO_AXIS_PX, tip.y - d.y * GIZMO_AXIS_PX))
    else {
        return;
    };
    for grip in grips {
        let GripShape::GizmoPlane(k) = grip.shape else {
            continue;
        };
        let (a, b) = gizmo_plane_axes(k);
        let (Some((_, a)), Some((_, b))) = (axes[a as usize], axes[b as usize]) else {
            continue;
        };
        let corner = |u: f32, v: f32| {
            Point::new(
                center.x + (a.x * u + b.x * v) * GIZMO_AXIS_PX,
                center.y + (a.y * u + b.y * v) * GIZMO_AXIS_PX,
            )
        };
        let quad = canvas::Path::new(|p| {
            p.move_to(corner(0.18, 0.18));
            p.line_to(corner(0.42, 0.18));
            p.line_to(corner(0.42, 0.42));
            p.line_to(corner(0.18, 0.42));
            p.close();
        });
        let lit = grip.is_hovered || grip.is_hot;
        let color = if lit {
            active
        } else {
            Color::from_rgb(0.85, 0.85, 0.85)
        };
        frame.fill(&quad, color.scale_alpha(if lit { 0.55 } else { 0.25 }));
        frame.stroke(
            &quad,
            canvas::Stroke::default().with_width(1.0).with_color(color),
        );
    }
    for grip in grips {
        let (GripShape::GizmoAxis(k), Some((tip, d))) = (
            grip.shape,
            grip.dir
                .map(|[dx, dy]| (grip.pos, iced::Vector::new(dx, -dy))),
        ) else {
            continue;
        };
        let color = if grip.is_hovered || grip.is_hot {
            active
        } else {
            COLORS[k as usize % 3]
        };
        let base = Point::new(tip.x - d.x * 13.0, tip.y - d.y * 13.0);
        frame.stroke(
            &canvas::Path::line(center, base),
            canvas::Stroke::default().with_width(2.5).with_color(color),
        );
        let head = canvas::Path::new(|p| {
            p.move_to(tip);
            p.line_to(Point::new(base.x - d.y * 6.0, base.y + d.x * 6.0));
            p.line_to(Point::new(base.x + d.y * 6.0, base.y - d.x * 6.0));
            p.close();
        });
        frame.fill(&head, color);
    }
}

fn draw_grip_marker(
    frame: &mut canvas::Frame,
    grip: &GripMarker,
    palette: &iced::theme::Palette,
    visual: &SelectionVisualOptions,
    authoring: &authoring::AuthoringVisualOptions,
) {
    let sp = grip.pos;
    let h = if grip.is_authoring {
        authoring.grip_size / 2.0
    } else {
        visual.grip_size.clamp(1.0, 25.0)
    };
    let path = match grip.shape {
        GripShape::Square => {
            canvas::Path::rectangle(Point::new(sp.x - h, sp.y - h), Size::new(h * 2.0, h * 2.0))
        }
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
        GripShape::GizmoAxis(_) | GripShape::GizmoPlane(_) => return,
        GripShape::Dropdown | GripShape::DropdownAdjacent => canvas::Path::new(|b| {
            b.move_to(Point::new(sp.x - h, sp.y - h * 0.5));
            b.line_to(Point::new(sp.x + h, sp.y - h * 0.5));
            b.line_to(Point::new(sp.x, sp.y + h));
            b.close();
        }),
    };

    if grip.is_hot {
        let hot_color = if visual.grip_hot > 0 {
            if let Some((r, g, b)) = codec::types::aci_table::aci_to_rgb(visual.grip_hot) {
                Color::from_rgb8(r, g, b)
            } else {
                palette.danger.base.color
            }
        } else {
            palette.danger.base.color
        };
        frame.fill(&path, hot_color);
    } else if grip.is_hovered {
        let pair = palette.primary.strong;
        let hover_color = if visual.grip_hover > 0 {
            if let Some((r, g, b)) = codec::types::aci_table::aci_to_rgb(visual.grip_hover) {
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
        let color = if grip.is_authoring {
            authoring.grip
        } else if grip.is_dynamic {
            codec::types::aci_table::aci_to_rgb(visual.grip_dynamic)
                .map(|(r,g,b)| Color::from_rgb8(r,g,b)).unwrap_or(palette.primary.base.color)
        } else if grip.is_plugin {
            // Plugin-declared custom grip (API v11): a distinct accent so
            // it never reads as a built-in selection grip. Hot and hover
            // treatments stay identical to built-ins.
            palette.success.base.color
        } else if visual.grip_color > 0 {
            if let Some((r, g, b)) = codec::types::aci_table::aci_to_rgb(visual.grip_color) {
                Color::from_rgb8(r, g, b)
            } else {
                palette.primary.base.color
            }
        } else {
            palette.primary.base.color
        };
        let fill = if grip.is_dynamic || matches!(
            grip.shape,
            GripShape::Dropdown | GripShape::DropdownAdjacent
        ) {
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

// ── Selection overlay frame caches ─────────────────────────────────────────

/// Cache hit decision for one selection-overlay layer: `true` iff `cached`
/// is `Some` and structurally equal to `new`. Reference-based so the stored
/// key never has to be moved out of its `RefCell`.
#[doc(hidden)]
pub fn should_reuse_layer<K: PartialEq>(cached: Option<&K>, new: &K) -> bool {
    match cached {
        Some(old) => old == new,
        None => false,
    }
}

/// One cached static layer of the selection overlay: the key of the geometry
/// currently in the cache plus the `canvas::Cache` holding it. Mirrors
/// `GridCanvasState` — one instance per layer, so a change to one layer's
/// inputs (say, a glyph selection) never rebuilds the others.
///
/// Every layer's key contains the frame bounds, so a key match implies the
/// iced-level bounds match too and `draw_with_bounds`'s early return
/// applies. `clear()` on a key miss forces the closure to run even when the
/// bounds happen to be unchanged (e.g. a document edit at the same canvas
/// size).
#[doc(hidden)]
pub struct CachedOverlayLayer<K> {
    pub key: RefCell<Option<K>>,
    pub cache: canvas::Cache<iced::Renderer>,
}

impl<K> Default for CachedOverlayLayer<K> {
    fn default() -> Self {
        Self {
            key: RefCell::new(None),
            cache: canvas::Cache::new(),
        }
    }
}

impl<K: Clone + PartialEq> CachedOverlayLayer<K> {
    /// Draw one layer, replaying the cached geometry when `key` matches the
    /// layer's stored key. `bounds` is the frame bounds every layer is built
    /// with (`Rectangle::with_size(bounds.size())`), which reproduces the
    /// clip bounds the overlay's single frame had before it was split.
    pub fn draw(
        &self,
        renderer: &iced::Renderer,
        bounds: iced::Rectangle,
        key: &K,
        build: impl FnOnce(&mut canvas::Frame),
    ) -> canvas::Geometry {
        // Scoped borrow: `self.key` is written again after `build` has run,
        // so the decision borrow must end first.
        let hit = {
            let cached = self.key.borrow();
            should_reuse_layer(cached.as_ref(), key)
        };
        let geometry = if hit {
            // No-op closure: the fork short-circuits on bounds match and
            // returns the cached geometry without invoking the closure.
            self.cache.draw_with_bounds(renderer, bounds, |_frame| {})
        } else {
            // Key or bounds changed. Clear so the closure runs even when the
            // bounds happen to match the previously cached frame.
            self.cache.clear();
            self.cache.draw_with_bounds(renderer, bounds, build)
        };
        *self.key.borrow_mut() = Some(key.clone());
        geometry
    }
}

/// Build a one-frame geometry for a dynamic layer (cursor, marquees, hover
/// feedback) that is rebuilt every frame. The frame keeps the clip bounds
/// the pre-split single frame had, so staged clip regions resolve
/// identically.
#[doc(hidden)]
pub fn transient_layer(
    renderer: &iced::Renderer,
    bounds: iced::Rectangle,
    build: impl FnOnce(&mut canvas::Frame),
) -> canvas::Geometry {
    let mut frame = canvas::Frame::new(renderer, bounds.size());
    build(&mut frame);
    frame.into_geometry()
}

/// Key for the static pane-divider layer.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct DividerLayerKey {
    pub bounds: iced::Rectangle,
    pub palette: iced::theme::Palette,
    pub dividers: Vec<iced::Rectangle>,
}

/// Flat, comparable copy of `authoring::AuthoringVisualOptions` (the source
/// type lives outside this module and carries no `PartialEq`).
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct AuthoringStyleKey {
    pub action: Color,
    pub parameter: Color,
    pub grip: Color,
    pub grip_size: f32,
    pub text_size: f32,
    pub font: String,
    pub horizontal: bool,
}

impl From<&authoring::AuthoringVisualOptions> for AuthoringStyleKey {
    fn from(value: &authoring::AuthoringVisualOptions) -> Self {
        Self {
            action: value.action,
            parameter: value.parameter,
            grip: value.grip,
            grip_size: value.grip_size,
            text_size: value.text_size,
            font: value.font.clone(),
            horizontal: value.horizontal,
        }
    }
}

/// Key for the static grips / control-polygon / authoring-feedback layer.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct GripLayerKey {
    pub bounds: iced::Rectangle,
    pub palette: iced::theme::Palette,
    pub grips: Vec<GripMarker>,
    pub control_polygon: Option<(Vec<Point>, bool)>,
    pub grip_clip: Option<iced::Rectangle>,
    pub selection_visual: SelectionVisualOptions,
    pub authoring_style: AuthoringStyleKey,
    pub feedback_clip: iced::Rectangle,
    pub feedback_dependencies: Vec<[Point; 2]>,
    pub feedback_markers: Vec<Point>,
}

/// Key for the static authoring visibility / point / dimension layer.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct AuthoringLayerKey {
    pub bounds: iced::Rectangle,
    pub crosshair_bg: [f32; 4],
    pub visibility: Option<(Point, String, iced::Rectangle, bool)>,
    pub points: Vec<(Point, Point, String, iced::Rectangle, bool)>,
    pub dimensions: Vec<AuthoringDimension>,
    pub authoring_style: AuthoringStyleKey,
}

/// Key for the static constraint-glyph pill layer.
///
/// The memoised `GlyphEntry` slice has no `PartialEq`, so `eq` compares the
/// entries field by field (via `same_glyph_entries`) instead of deriving it.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct GlyphLayerKey {
    pub bounds: iced::Rectangle,
    pub glyphs: Arc<[GlyphEntry]>,
    pub selected: Arc<[bool]>,
    pub palette: iced::theme::Palette,
}

impl GlyphLayerKey {
    /// Build a key from the memoised entry Arcs. The Arcs are cloned (not
    /// the entries), so key construction is cheap; the upstream cache owns
    /// the entries, and pinning them here keeps a hit valid even if a later
    /// rebuild replaces the shared slice.
    #[doc(hidden)]
    pub fn new(
        bounds: iced::Rectangle,
        glyphs: &Arc<[GlyphEntry]>,
        selected: &Arc<[bool]>,
        palette: iced::theme::Palette,
    ) -> Self {
        Self {
            bounds,
            glyphs: Arc::clone(glyphs),
            selected: Arc::clone(selected),
            palette,
        }
    }
}

impl PartialEq for GlyphLayerKey {
    fn eq(&self, other: &Self) -> bool {
        self.bounds == other.bounds
            && self.palette == other.palette
            && *self.selected == *other.selected
            && same_glyph_entries(&self.glyphs, &other.glyphs)
    }
}

/// Structural equality for two memoised glyph-entry slices. `Arc::ptr_eq`
/// fast path first — the view hands back the same allocation on a memoised
/// hit — then a field-by-field compare. All `f32` fields compare by bit
/// pattern so a NaN anchor cannot pin the layer to a permanent cache miss.
fn same_glyph_entries(a: &Arc<[GlyphEntry]>, b: &Arc<[GlyphEntry]>) -> bool {
    if Arc::ptr_eq(a, b) {
        return true;
    }
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| {
                x.id == y.id
                    && x.anchor.x.to_bits() == y.anchor.x.to_bits()
                    && x.anchor.y.to_bits() == y.anchor.y.to_bits()
                    && x.outward[0].to_bits() == y.outward[0].to_bits()
                    && x.outward[1].to_bits() == y.outward[1].to_bits()
                    && (Arc::ptr_eq(&x.label, &y.label) || x.label == y.label)
                    && x.conflicting == y.conflicting
                    && (Arc::ptr_eq(&x.hover, &y.hover)
                        || (x.hover.len() == y.hover.len()
                            && x.hover.iter().zip(y.hover.iter()).all(|(a, b)| {
                                a.x.to_bits() == b.x.to_bits()
                                    && a.y.to_bits() == b.y.to_bits()
                            })))
                    && x.size.width.to_bits() == y.size.width.to_bits()
                    && x.size.height.to_bits() == y.size.height.to_bits()
                    && x.tangent_dx.to_bits() == y.tangent_dx.to_bits()
                    && x.top_left.x.to_bits() == y.top_left.x.to_bits()
                    && x.top_left.y.to_bits() == y.top_left.y.to_bits()
            })
}

/// Key for the static object-snap-tracking crosses layer.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct TrackingMarksLayerKey {
    pub bounds: iced::Rectangle,
    pub palette: iced::theme::Palette,
    pub ost_points: Vec<OstTrackPoint>,
}

/// `Program::State` for `SelectionCanvas`: one `CachedOverlayLayer` per
/// static layer of the overlay. The dynamic layers (cursor, marquees,
/// hover feedback) are rebuilt each frame and keep no state.
#[doc(hidden)]
#[derive(Default)]
pub struct SelectionCanvasState {
    pub dividers: CachedOverlayLayer<DividerLayerKey>,
    pub grips: CachedOverlayLayer<GripLayerKey>,
    pub authoring: CachedOverlayLayer<AuthoringLayerKey>,
    pub glyphs: CachedOverlayLayer<GlyphLayerKey>,
    pub tracking_marks: CachedOverlayLayer<TrackingMarksLayerKey>,
}

/// Draw the static constraint-glyph pills (badge, selection ring, symbol)
/// for one frame. Split out of `SelectionCanvas::draw` so the cached layer
/// and the benchmark share exactly one implementation of this path.
/// Returns the number of pill badges emitted — one per entry with a finite
/// anchor; callers ignore it outside the benchmark.
#[doc(hidden)]
pub fn draw_constraint_glyph_layer(
    frame: &mut canvas::Frame,
    glyphs: &[GlyphEntry],
    selected: &[bool],
    palette: &iced::theme::Palette,
) -> usize {
    let mut pills = 0usize;
    let normal_bg = Color::from_rgb8(103, 109, 118);
    let normal_fg = Color::WHITE;
    // A redundant or conflicting constraint gets the danger palette instead
    // of the ordinary primary one — same information a resolver panel would
    // show, surfaced right on the geometry.
    let conflict_bg = palette.danger.base.color;
    let conflict_fg = palette.danger.base.text;
    let selected_ring = palette.primary.strong.color;
    let coincident_bg = Color::from_rgb8(35, 145, 230);
    for (entry, is_selected) in glyphs.iter().zip(selected.iter()) {
        let label: &str = &entry.label;
        if !entry.anchor.x.is_finite() || !entry.anchor.y.is_finite() {
            continue;
        }
        pills += 1;
        let compact_coincident = glyph_is_compact(label);
        let (bg, fg) = if entry.conflicting {
            (conflict_bg, conflict_fg)
        } else if compact_coincident {
            (coincident_bg, normal_fg)
        } else {
            (normal_bg, normal_fg)
        };
        let (top_left, size) = (entry.top_left, entry.size);
        let pill = canvas::Path::rounded_rectangle(
            top_left,
            size,
            (if compact_coincident {
                1.0
            } else {
                size.height * 0.5
            })
            .into(),
        );
        // A dynamic dimension's lock sits bare on the drawing.
        if !is_dynamic_dimension_glyph(label) {
            frame.fill(&pill, bg);
        }
        if *is_selected {
            frame.stroke(
                &pill,
                canvas::Stroke::default()
                    .with_color(selected_ring)
                    .with_width(2.0),
            );
        }
        if !compact_coincident {
            let glyph_center = Point::new(
                top_left.x + size.width * 0.5,
                top_left.y + size.height * 0.5,
            );
            if label == "T" {
                draw_tangent_constraint_glyph(frame, glyph_center, fg);
            } else if label == "G²" {
                draw_smooth_constraint_glyph(frame, glyph_center, fg);
            } else if label == "◎" {
                draw_concentric_constraint_glyph(frame, glyph_center, fg);
            } else if is_dynamic_dimension_glyph(label) {
                draw_fixed_constraint_glyph(frame, glyph_center, fg, bg, false);
            } else if glyph_is_fixed(label) {
                let point = label == crate::scene::parametric_constraints::FIXED_POINT_GLYPH;
                // The object-mode lock reads red, the point-mode one white.
                let lock = if point {
                    fg
                } else {
                    Color::from_rgb8(214, 76, 76)
                };
                draw_fixed_constraint_glyph(frame, glyph_center, lock, bg, point);
            } else if glyph_is_vertical(label) {
                draw_vertical_constraint_glyph(
                    frame,
                    glyph_center,
                    fg,
                    label == crate::scene::parametric_constraints::VERTICAL_POINTS_GLYPH,
                );
            } else {
                // The Equal symbol reads green on its badge, as asked.
                let color = if label == "=" {
                    Color::from_rgb8(72, 199, 116)
                } else {
                    fg
                };
                frame.fill_text(canvas::Text {
                    content: label.to_string(),
                    position: glyph_center,
                    color,
                    size: iced::Pixels(GLYPH_SIZE),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    shaping: iced::advanced::text::Shaping::Advanced,
                    ..Default::default()
                });
            }
        }
    }
    pills
}

/// Count the pill badges [`draw_constraint_glyph_layer`] emits for these
/// entries — one per entry with a finite anchor. Used as the benchmark's
/// renderer-free sink so both timing arms produce the same value.
#[doc(hidden)]
pub fn count_constraint_glyph_pills(glyphs: &[GlyphEntry]) -> usize {
    glyphs
        .iter()
        .filter(|entry| entry.anchor.x.is_finite() && entry.anchor.y.is_finite())
        .count()
}

impl canvas::Program<Message> for SelectionCanvas {
    type State = SelectionCanvasState;

    fn mouse_interaction(
        &self,
        _state: &SelectionCanvasState,
        bounds: iced::Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // A dropdown/overlay is open over the viewport — show the normal OS
        // cursor over the whole canvas instead of hiding it for the crosshair,
        // so the cursor doesn't vanish while using the panel. (#227)
        if self.suppressed {
            return mouse::Interaction::default();
        }
        // An armed navigation tool owns the whole viewport, and the cursor
        // says which one: a hand to drag the sheet, four-way arrows to swing
        // the model, a magnifier that carries the sign of the zoom actually
        // under way. Hovering before the drag starts shows the neutral "+"
        // magnifier rather than guessing a direction.
        if self.nav != NavCursor::None && cursor.is_over(bounds) {
            let dragging = self.selection.borrow().input.middle_down;
            return match self.nav {
                NavCursor::Pan => {
                    if dragging {
                        mouse::Interaction::Grabbing
                    } else {
                        mouse::Interaction::Grab
                    }
                }
                NavCursor::Orbit => mouse::Interaction::AllScroll,
                NavCursor::Zoom => {
                    if dragging && self.selection.borrow().input.zoom_dir_out {
                        mouse::Interaction::ZoomOut
                    } else {
                        mouse::Interaction::ZoomIn
                    }
                }
                NavCursor::None => unreachable!("guarded above"),
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
        if let Some(pos) = cursor.position_in(bounds) {
            if glyph_hit_test_entries(&self.constraint_glyphs, pos).is_some() {
                return mouse::Interaction::Pointer;
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
        state: &SelectionCanvasState,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let frame_bounds = iced::Rectangle::with_size(bounds.size());
        let mut layers: Vec<canvas::Geometry> = Vec::new();

        // ── Pane dividers (Model-space tiled layout) ──────────────────────
        // Filled bars in the pane_grid spacing gaps, so adjacent panes read as
        // distinct viewports. Drawn first so all other overlays sit on top.
        if !self.dividers.is_empty() {
            let key = DividerLayerKey {
                bounds,
                palette: *theme.palette(),
                dividers: self.dividers.clone(),
            };
            layers.push(state.dividers.draw(renderer, frame_bounds, &key, |frame| {
            let divider = theme.palette().background.neutral.color;
            for d in &self.dividers {
                let bar = canvas::Path::rectangle(
                    Point::new(d.x, d.y),
                    iced::Size::new(d.width.max(1.0), d.height.max(1.0)),
                );
                frame.fill(&bar, divider);
            }
            }));
        }

        // ── Pane move (drag-to-swap) ──────────────────────────────────────
        // While armed: dim the lifted source pane, highlight the drop target
        // under the cursor, and drag a translucent ghost card along the cursor
        // so the pane is visibly "moving".
        layers.push(transient_layer(renderer, bounds, |frame| {
        if let Some(src) = self.pane_move_rect {
            let accent = theme.palette().primary.base.color;
            // Source pane: dimmed + dashed-feel outline (it has been lifted).
            let src_path = canvas::Path::rectangle(
                Point::new(src.x, src.y),
                iced::Size::new(src.width, src.height),
            );
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
            if let Some(c) = self.selection.borrow().input.last_move_pos {
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

        // ZOOM Dynamic reuses `box_anchor` to remember where the drag began,
        // so the marquee would otherwise rubber-band across the screen while
        // the view zooms. Suppress the drawing, not the state — the anchor is
        // still re-projected as the camera moves.
        if self.nav != NavCursor::Zoom {
            if let (Some(a), Some(b)) = (
                self.selection.borrow().gesture.box_anchor,
                self.selection.borrow().gesture.box_current,
            ) {
                draw_marquee(
                    frame,
                    a,
                    b,
                    self.selection.borrow().gesture.box_crossing,
                    theme,
                    &self.selection_visual,
                    self.crosshair_bg,
                );
            }
        }
        // Preview marquee for point-picked windows (STRETCH) — same look, no pick.
        if let Some((a, b, crossing)) = self.selection.borrow().gesture.preview_box {
            draw_marquee(
                frame,
                a,
                b,
                crossing,
                theme,
                &self.selection_visual,
                self.crosshair_bg,
            );
        }

        if self.selection.borrow().gesture.poly_active
            && self.selection.borrow().gesture.poly_points.len() > 1
        {
            let crossing = self.selection.borrow().gesture.poly_crossing;
            let base = resolve_selection_base_color(
                crossing,
                theme,
                &self.selection_visual,
                self.crosshair_bg,
            );
            let canvas_light = crate::ui::style::common::canvas_is_light(self.crosshair_bg);
            if self.selection_visual.area && self.selection_visual.opacity > 0 {
                let alpha =
                    selection_fill_alpha(self.selection_visual.opacity as f32, canvas_light);
                let fill = base.scale_alpha(alpha);
                if let Some(cur) = self.selection.borrow().input.last_move_pos {
                    let start = self.selection.borrow().gesture.poly_points[0];
                    let fill_path = canvas::Path::new(|p| {
                        p.move_to(start);
                        for pt in &self.selection.borrow().gesture.poly_points[1..] {
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
                p.move_to(self.selection.borrow().gesture.poly_points[0]);
                for pt in &self.selection.borrow().gesture.poly_points[1..] {
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
            if let Some(cur) = self.selection.borrow().input.last_move_pos {
                let start = self.selection.borrow().gesture.poly_points[0];
                let last = *self.selection.borrow().gesture.poly_points.last().unwrap();
                let preview = canvas::Path::new(|p| {
                    p.move_to(last);
                    p.line_to(cur);
                    p.line_to(start);
                });
                frame.stroke(&preview, stroke_style);
            }
        }
        }));

        // ── Grip markers ──────────────────────────────────────────────────
        let grip_key = GripLayerKey {
            bounds,
            palette: *theme.palette(),
            grips: self.grips.clone(),
            control_polygon: self.control_polygon.clone(),
            grip_clip: self.grip_clip,
            selection_visual: self.selection_visual,
            authoring_style: AuthoringStyleKey::from(&self.authoring_visual),
            feedback_clip: self.authoring_feedback.clip,
            feedback_dependencies: self.authoring_feedback.dependencies.clone(),
            feedback_markers: self.authoring_feedback.markers.clone(),
        };
        layers.push(state.grips.draw(renderer, frame_bounds, &grip_key, |frame| {
        frame.with_clip(self.authoring_feedback.clip, |frame| {
            let color = self.authoring_visual.parameter;
            for [a,b] in &self.authoring_feedback.dependencies {
                let line = canvas::Path::line(*a,*b);
                frame.stroke(&line,canvas::Stroke::default().with_color(color.scale_alpha(0.35)).with_width(7.0));
                frame.stroke(&line,canvas::Stroke::default().with_color(color).with_width(1.0));
            }
            for point in &self.authoring_feedback.markers {
                let marker = canvas::Path::new(|path| {
                    path.move_to(Point::new(point.x-3.0,point.y));
                    path.line_to(Point::new(point.x+3.0,point.y));
                    path.move_to(Point::new(point.x,point.y-3.0));
                    path.line_to(Point::new(point.x,point.y+3.0));
                });
                frame.stroke(&marker,canvas::Stroke::default().with_color(color).with_width(1.5));
            }
        });
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
                draw_move_gizmo(frame, &self.grips);
                for grip in &self.grips {
                    if !matches!(
                        grip.shape,
                        GripShape::GizmoAxis(_) | GripShape::GizmoPlane(_)
                    ) {
                        draw_grip_marker(
                            frame,
                            grip,
                            theme.palette(),
                            &self.selection_visual,
                            &self.authoring_visual,
                        );
                    }
                }
            });
        }
        }));

        // ── Snap marker ───────────────────────────────────────────────────
        layers.push(transient_layer(renderer, bounds, |frame| {
        if let Some((sp, snap_type)) = self.snap {
            let (r, g, b) = if snap_type == SnapType::ObjectPick {
                (0.95_f32, 0.50, 0.08) // orange object-snap marker
            } else if let Some([r, g, b]) = self.crosshair.snap_color {
                (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
            } else if crate::ui::style::common::canvas_is_light(self.crosshair_bg) {
                (0.80, 0.45, 0.0) // dark amber: yellow vanishes on a light canvas
            } else {
                (1.0, 0.9, 0.1) // classic yellow OSNAP
            };
            let marker = Color { r, g, b, a: 1.0 };
            let stroke = canvas::Stroke {
                width: if snap_type == SnapType::ObjectPick {
                    2.0
                } else {
                    1.5
                },
                style: canvas::Style::Solid(marker),
                ..Default::default()
            };
            // Construction guides are canvas-space geometry, NOT snap glyphs.
            // They must be drawn before the SNAPMARKERSIZE transform below.
            // Scaling a guide around the moving snap position makes a fixed
            // acquired endpoint visibly slide as the cursor moves.
            let extension_ray = self.snap_ext_base.and_then(|base| {
                let acquired = self.snap_ext_dir.or_else(|| {
                    self.otrack_lines.iter().find_map(|&(origin, tip)| {
                        let offset = (origin.x - base.x).hypot(origin.y - base.y);
                        (offset <= 2.0)
                            .then_some(Point::new(tip.x - origin.x, tip.y - origin.y))
                    })
                });
                extension_guide_tip(base, sp, acquired)
                    .map(|(unit, tip)| (base, unit, tip))
            });
            let guide_dash = canvas::Stroke {
                line_dash: canvas::LineDash {
                    segments: &[4.0, 4.0],
                    offset: 0,
                },
                ..canvas::Stroke::default().with_color(marker).with_width(1.0)
            };
            match snap_type {
                SnapType::Extension => {
                    if let Some((base, _, tip)) = extension_ray {
                        if let Some((a, b)) = clip_seg(base, tip, bounds) {
                            frame.stroke(&canvas::Path::line(a, b), guide_dash.clone());
                        }
                    }
                }
                SnapType::Intersection => {
                    // Extended-intersection guide origins must also remain
                    // fixed when SNAPMARKERSIZE changes.
                    for base in [self.snap_ext_base, self.snap_ext_base2]
                        .into_iter()
                        .flatten()
                    {
                        let dx = sp.x - base.x;
                        let dy = sp.y - base.y;
                        let len = dx.hypot(dy);
                        if len > 1e-3 {
                            let tip = Point::new(sp.x + dx / len * 18.0, sp.y + dy / len * 18.0);
                            if let Some((a, b)) = clip_seg(base, tip, bounds) {
                                frame.stroke(&canvas::Path::line(a, b), guide_dash.clone());
                            }
                        }
                    }
                }
                _ => {}
            }

            // SNAPMARKERSIZE scales the glyphs drawn for the default 5 px.
            let scale = self.crosshair.snap_size.clamp(1.0, 20.0) / 5.0;
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(sp.x, sp.y));
                frame.scale(scale);
                frame.translate(iced::Vector::new(-sp.x, -sp.y));
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
                    SnapType::GeometricCenter => {
                        // Pentagon around a centre dot: the area centroid.
                        let r = 6.5_f32;
                        let path = canvas::Path::new(|b| {
                            for k in 0..5 {
                                let a = -std::f32::consts::FRAC_PI_2
                                    + k as f32 * std::f32::consts::TAU / 5.0;
                                let p = Point::new(sp.x + r * a.cos(), sp.y + r * a.sin());
                                if k == 0 {
                                    b.move_to(p);
                                } else {
                                    b.line_to(p);
                                }
                            }
                            b.close();
                        });
                        frame.stroke(&path, stroke.clone());
                        frame.fill(&canvas::Path::circle(sp, 1.6), marker);
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
                        // The guide is drawn unscaled above; only the X glyph
                        // belongs inside the SNAPMARKERSIZE transform.
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
                        // Only the three-dot marker is scaled. The actual
                        // guide and diagnostic dot are drawn in canvas space.
                        let dir = extension_ray;
                        // Three dots stay aligned with the *same* acquired ray
                        // used by the dashed guide (rather than cursor jitter).
                        let u = dir.map(|(_, u, _)| u).unwrap_or(Point::new(1.0, 0.0));
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
                    SnapType::Vertex => {
                        // Filled square: the solid rhyme of Endpoint's hollow box.
                        let h = 5.0_f32;
                        frame.fill(
                            &canvas::Path::rectangle(
                                Point::new(sp.x - h, sp.y - h),
                                Size::new(h * 2.0, h * 2.0),
                            ),
                            marker,
                        );
                    }
                    SnapType::EdgeMidpoint => {
                        // Filled triangle: the solid rhyme of Midpoint's outline.
                        let r = 6.0_f32;
                        let path = canvas::Path::new(|b| {
                            b.move_to(Point::new(sp.x, sp.y - r));
                            b.line_to(Point::new(sp.x + r * 0.866, sp.y + r * 0.5));
                            b.line_to(Point::new(sp.x - r * 0.866, sp.y + r * 0.5));
                            b.close();
                        });
                        frame.fill(&path, marker);
                    }
                    SnapType::FaceCenter => {
                        // Filled disc: the solid rhyme of Center's outline.
                        frame.fill(&canvas::Path::circle(sp, 5.5_f32), marker);
                    }
                    SnapType::Knot => {
                        // Filled diamond: distinct from Quadrant's outline.
                        let r = 5.5_f32;
                        let path = canvas::Path::new(|b| {
                            b.move_to(Point::new(sp.x, sp.y - r));
                            b.line_to(Point::new(sp.x + r, sp.y));
                            b.line_to(Point::new(sp.x, sp.y + r));
                            b.line_to(Point::new(sp.x - r, sp.y));
                            b.close();
                        });
                        frame.fill(&path, marker);
                    }
                    SnapType::FacePerpendicular => {
                        // Right-angle hook like the 2D marker, plus a filled foot
                        // dot marking the face contact.
                        let r = 6.0_f32;
                        let p = canvas::Path::new(|b| {
                            b.move_to(Point::new(sp.x - r, sp.y - r));
                            b.line_to(Point::new(sp.x - r, sp.y + r));
                            b.line_to(Point::new(sp.x + r, sp.y + r));
                        });
                        frame.stroke(&p, stroke.clone());
                        frame.fill(&canvas::Path::circle(sp, 2.0_f32), marker);
                    }
                    SnapType::NearestFace => {
                        // Filled bowtie: the solid rhyme of Nearest's outline.
                        let r = 5.5_f32;
                        let path = canvas::Path::new(|b| {
                            b.move_to(Point::new(sp.x - r, sp.y - r));
                            b.line_to(Point::new(sp.x + r, sp.y - r));
                            b.line_to(Point::new(sp.x - r, sp.y + r));
                            b.line_to(Point::new(sp.x + r, sp.y + r));
                            b.close();
                        });
                        frame.fill(&path, marker);
                    }
                }
            });
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
        // Off the viewport (or under an overlay that took the pointer) the
        // cursor is the OS arrow; no crosshair or brush lingers at the last spot.
        let pointer_away = self.selection.borrow().input.pointer_away;
        // ── MATCHPROP paintbrush cursor ───────────────────────────────
        // While MATCHPROP is armed the user is transferring properties from a
        // source object onto others, so the crosshair gives way to a paintbrush
        // whose tip rides the pick point, as the reference does. Drawn *instead of*
        // the crosshair below, never on top of it.
        if !over_viewcube
            && !over_divider
            && !pointer_away
            && self.nav == NavCursor::None
            && !self.suppressed
            && self.show_match_brush
        {
            if let Some(cp) = self
                .grips
                .iter()
                .find(|grip| grip.is_hovered)
                .map(|grip| grip.pos)
                .or(self.selection.borrow().input.last_move_pos)
            {
                // Inherit the crosshair's own colour (including a custom
                // crosshair colour) so only the shape changes, never the ink.
                let [cr, cg, cb, ca] = self.crosshair.color.map_or_else(
                    || {
                        crate::scene::view::render::adapt_to_bg(
                            [1.0, 1.0, 1.0, 0.90],
                            self.crosshair_bg,
                        )
                    },
                    |[r, g, b]| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 0.90],
                );
                let pickbox_half = if self.crosshair.matchprop_pickbox {
                    pick_box_aperture_px(self.crosshair.pick_box)
                } else {
                    0.0
                };
                // The brush points into the pickbox from its upper-right
                // corner; its bristle width just meets the frame without
                // covering the box or moving the actual click point.
                let brush_gap = pickbox_half + 3.2 * std::f32::consts::FRAC_1_SQRT_2;
                let brush_tip = if self.crosshair.matchprop_pickbox {
                    Point::new(cp.x + brush_gap, cp.y - brush_gap)
                } else {
                    cp
                };
                draw_match_brush(
                    frame,
                    brush_tip,
                    Color {
                        r: cr,
                        g: cg,
                        b: cb,
                        a: ca,
                    },
                );
                if self.crosshair.matchprop_pickbox {
                    let half = pickbox_half;
                    let square = canvas::Path::rectangle(
                        Point::new(cp.x - half, cp.y - half),
                        Size::new(half * 2.0, half * 2.0),
                    );
                    if !self.crosshair.matchprop_source_pick {
                        frame.fill(
                            &square,
                            Color {
                                r: 0.24,
                                g: 0.52,
                                b: 0.98,
                                a: 0.88,
                            },
                        );
                    }
                    frame.stroke(
                        &square,
                        canvas::Stroke {
                            width: 1.25,
                            style: canvas::Style::Solid(Color::from_rgb(0.30, 0.55, 0.98)),
                            ..Default::default()
                        },
                    );
                }
            }
        }

        // An armed navigation tool replaces the crosshair with its own cursor.
        if !over_viewcube
            && !over_divider
            && !pointer_away
            && self.nav == NavCursor::None
            && !self.suppressed
            && !self.show_match_brush
            && self.crosshair.cursor_type == CursorType::Crosshair
        {
            // Over a grip the crosshair locks onto it, so the click lands on
            // the grip exactly.
            let hovered_grip = self
                .grips
                .iter()
                .find(|grip| grip.is_hovered)
                .map(|grip| grip.pos);
            if let Some(cp) = hovered_grip.or(self.selection.borrow().input.last_move_pos) {
                let [r, g, b, a] = self.crosshair.color.map_or_else(
                    || {
                        crate::scene::view::render::adapt_to_bg(
                            [1.0, 1.0, 1.0, 0.90],
                            self.crosshair_bg,
                        )
                    },
                    |[r, g, b]| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 0.90],
                );
                let color = Color { r, g, b, a };
                // Pending client pick: drop the arms, keep a blue pickbox —
                // the same blue as the MCP "waiting for you to pick" pill.
                let pick_pending = self.crosshair.pick_pending;
                let color = if pick_pending {
                    Color::from_rgb(0.30, 0.55, 0.98)
                } else {
                    color
                };
                let stroke = canvas::Stroke {
                    width: if pick_pending { 1.5 } else { 1.0 },
                    style: canvas::Style::Solid(color),
                    ..Default::default()
                };
                let point_mode = self.crosshair.point_mode;
                let sq = if point_mode {
                    0.0
                } else {
                    pick_box_half_px(self.crosshair.pick_box)
                };
                let mut arm = crosshair_arm_px(bounds, self.crosshair.size_percent);
                if self.crosshair.short_arms {
                    arm = arm.min(CROSSHAIR_ARM);
                }
                let base_angles: [f64; 2] = if self.crosshair.isometric {
                    self.crosshair.iso_plane.angles()
                } else {
                    [0.0, 90.0]
                };
                // The arms stay only for the normal (non-pending) cursor: a
                // waiting pick shows the box alone, regardless of the UCS
                // rotation, so the square is unmistakable.
                if !pick_pending && !self.crosshair.hide_arms {
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
                }
                if point_mode {
                    let dot = canvas::Path::circle(cp, 1.75);
                    frame.fill(&dot, color);
                } else {
                    // A pending pick keeps the square visible even when the
                    // user's PICKBOX setting would hide it.
                    let sq = if pick_pending { sq.max(8.0) } else { sq };
                    if sq > 0.0 {
                        let square = canvas::Path::rectangle(
                            Point::new(cp.x - sq, cp.y - sq),
                            Size::new(sq * 2.0, sq * 2.0),
                        );
                        frame.stroke(&square, stroke);
                    }
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
                    let body =
                        canvas::Path::rectangle(Point::new(bx, by + 6.0), Size::new(12.0, 9.0));
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
                if let Some(label) = &self.constraint_cursor_badge {
                    let center = Point::new(
                        cp.x + sq + if self.hover_locked { 30.0 } else { 12.0 },
                        cp.y - sq - 7.0,
                    );
                    let blue = Color::from_rgb8(35, 145, 230);
                    let badge = canvas::Path::circle(center, 8.5);
                    frame.fill(&badge, blue);
                    if label == "◎" {
                        draw_concentric_constraint_glyph(frame, center, Color::WHITE);
                    } else if label == "T" {
                        draw_tangent_constraint_glyph(frame, center, Color::WHITE);
                    } else {
                        frame.fill_text(canvas::Text {
                            content: label.clone(),
                            position: center,
                            color: Color::WHITE,
                            size: iced::Pixels(11.0),
                            align_x: iced::alignment::Horizontal::Center.into(),
                            align_y: iced::alignment::Vertical::Center,
                            shaping: iced::advanced::text::Shaping::Advanced,
                            ..Default::default()
                        });
                    }
                }
            }
        } // end !over_viewcube

        // ── UCS icon (one per Model pane) ─────────────────────────────────
        for ucs in &self.ucs_icons {
            draw_ucs_icon(
                frame,
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
        for &(base, tip) in &self.otrack_lines {
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
                        segments: &TRACKING_DASH_PX,
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
        }));

        let authoring_active = self.authoring_visibility.is_some()
            || !self.authoring_points.is_empty()
            || !self.authoring_dimensions.is_empty();
        if authoring_active {
            let authoring_key = AuthoringLayerKey {
                bounds,
                crosshair_bg: self.crosshair_bg,
                visibility: self.authoring_visibility.clone(),
                points: self.authoring_points.clone(),
                dimensions: self.authoring_dimensions.clone(),
                authoring_style: AuthoringStyleKey::from(&self.authoring_visual),
            };
            layers.push(state.authoring.draw(renderer, frame_bounds, &authoring_key, |frame| {
            if let Some((point, label, clip, selected)) = &self.authoring_visibility {
            frame.with_clip(*clip, |frame| {
                let style = &self.authoring_visual;
                let color = style.parameter;
                let r = style.grip_size / 2.0;
                let triangle = canvas::Path::new(|builder| {
                    builder.move_to(Point::new(point.x - r, point.y - r));
                    builder.line_to(Point::new(point.x + r, point.y - r));
                    builder.line_to(Point::new(point.x, point.y + r));
                    builder.close();
                });
                if *selected {
                    frame.stroke(
                        &canvas::Path::circle(*point, 10.0),
                        canvas::Stroke::default().with_color(color).with_width(2.0),
                    );
                }
                frame.fill(&triangle, style.grip);
                authoring::draw_label(
                    frame,
                    Point::new(point.x + 5.0, point.y - 4.0),
                    label,
                    color,
                    None,
                    0.0,
                    style,
                );
            });
        }
        for (point, label_point, label, clip, selected) in &self.authoring_points {
            frame.with_clip(*clip, |frame| {
                let style = &self.authoring_visual;
                let color = style.parameter;
                if *selected {
                    frame.stroke(
                        &canvas::Path::circle(*point, 10.0),
                        canvas::Stroke::default().with_color(color).with_width(2.0),
                    );
                }
                frame.stroke(
                    &canvas::Path::line(*point, *label_point),
                    canvas::Stroke::default().with_color(color),
                );
                frame.fill(
                    &authoring::grip_rectangle(*point, style.grip_size),
                    style.grip,
                );
                authoring::draw_label(
                    frame,
                    Point::new(label_point.x + 5.0, label_point.y - 4.0),
                    label,
                    color,
                    None,
                    0.0,
                    style,
                );
            });
        }
        for dimension in &self.authoring_dimensions {
            frame.with_clip(dimension.clip, |frame| {
                let style = &self.authoring_visual;
                let color = if dimension.is_action {
                    style.action
                } else {
                    style.parameter
                };
                let grip_scale = style.grip_size / 8.0;
                if dimension.selected {
                    for point in dimension
                        .base_grips
                        .iter()
                        .chain(dimension.lookup_grips.iter())
                        .chain(dimension.round_grips.iter())
                        .chain(dimension.grips.iter().map(|(point, _)| point))
                    {
                        frame.stroke(
                            &canvas::Path::circle(*point, 10.0),
                            canvas::Stroke::default().with_color(color).with_width(2.0),
                        );
                    }
                }
                for [a, b] in &dimension.lines {
                    frame.stroke(
                        &canvas::Path::line(*a, *b),
                        canvas::Stroke::default().with_color(color),
                    );
                }
                for [a, b] in &dimension.dimensions {
                    let dx = b.x - a.x;
                    let dy = b.y - a.y;
                    let length = dx.hypot(dy).max(1.0);
                    let (x, y) = (dx / length, dy / length);
                    for (point, sign) in [(a, 1.0), (b, -1.0)] {
                        let tip = Point::new(point.x + sign * 7.0 * x, point.y + sign * 7.0 * y);
                        let arrow = canvas::Path::new(|path| {
                            path.move_to(*point);
                            path.line_to(Point::new(tip.x - 2.5 * y, tip.y + 2.5 * x));
                            path.line_to(Point::new(tip.x + 2.5 * y, tip.y - 2.5 * x));
                            path.close();
                        });
                        frame.fill(&arrow, color);
                    }
                }
                for (index, (point, label)) in dimension.labels.iter().enumerate() {
                    // Keep diagonal dimension lines from crossing the label.
                    let background = Color::from_rgba(
                        self.crosshair_bg[0],
                        self.crosshair_bg[1],
                        self.crosshair_bg[2],
                        self.crosshair_bg[3],
                    );
                    let angle = authoring::label_angle(&dimension.dimensions, index, style.horizontal);
                    authoring::draw_label(
                        frame,
                        Point::new(point.x + 5.0, point.y - 4.0),
                        label,
                        color,
                        Some(background),
                        angle,
                        style,
                    );
                }
                for point in &dimension.base_grips {
                    let stroke = canvas::Stroke::default().with_color(style.grip);
                    frame.stroke(&canvas::Path::circle(*point, 6.0 * grip_scale), stroke);
                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(point.x - 9.0 * grip_scale, point.y),
                            Point::new(point.x + 9.0 * grip_scale, point.y),
                        ),
                        stroke,
                    );
                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(point.x, point.y - 9.0 * grip_scale),
                            Point::new(point.x, point.y + 9.0 * grip_scale),
                        ),
                        stroke,
                    );
                }
                for point in &dimension.lookup_grips {
                    frame.fill(
                        &canvas::Path::new(|p| {
                            p.move_to(Point::new(
                                point.x - 5.0 * grip_scale,
                                point.y - 4.0 * grip_scale,
                            ));
                            p.line_to(Point::new(
                                point.x + 5.0 * grip_scale,
                                point.y - 4.0 * grip_scale,
                            ));
                            p.line_to(Point::new(point.x, point.y + 5.0 * grip_scale));
                            p.close();
                        }),
                        style.grip,
                    );
                }
                for point in &dimension.round_grips {
                    frame.fill(&canvas::Path::circle(*point, 3.5 * grip_scale), style.grip);
                }
                for (point, base) in &dimension.grips {
                    let marker = if let Some(base) = base {
                        let dx = point.x - base.x;
                        let dy = point.y - base.y;
                        let length = dx.hypot(dy).max(1.0);
                        let (x, y) = (dx / length, dy / length);
                        canvas::Path::new(|path| {
                            path.move_to(Point::new(
                                point.x + 5.0 * x * grip_scale,
                                point.y + 5.0 * y * grip_scale,
                            ));
                            path.line_to(Point::new(
                                point.x - (4.0 * x + 4.0 * y) * grip_scale,
                                point.y - (4.0 * y - 4.0 * x) * grip_scale,
                            ));
                            path.line_to(Point::new(
                                point.x - (4.0 * x - 4.0 * y) * grip_scale,
                                point.y - (4.0 * y + 4.0 * x) * grip_scale,
                            ));
                            path.close();
                        })
                    } else {
                        authoring::grip_rectangle(*point, style.grip_size)
                    };
                    frame.fill(&marker, style.grip);
                }
            });
        }
            }));
        }

        // Draw + hover + tooltip all read the memoised entries' precomputed
        // `top_left`/`size` — one shared `hovered` lookup feeds both the
        // draw and the hover, with no offset recompute or label clones.
        let hovered = cursor
            .position_in(bounds)
            .and_then(|point| glyph_hit_test_entries(&self.constraint_glyphs, point));
        if !self.constraint_glyphs.is_empty() {
            let glyph_key = GlyphLayerKey::new(
                bounds,
                &self.constraint_glyphs,
                &self.constraint_glyph_selected,
                *theme.palette(),
            );
            layers.push(state.glyphs.draw(renderer, frame_bounds, &glyph_key, |frame| {
            draw_constraint_glyph_layer(
                frame,
                &self.constraint_glyphs,
                &self.constraint_glyph_selected,
                theme.palette(),
            );
            }));
        }
        if let Some(index) = hovered {
            layers.push(transient_layer(renderer, bounds, |frame| {
                let red = Color::from_rgb(1.0, 0.0, 0.0);
                let stroke = canvas::Stroke::default().with_color(red).with_width(1.5);
                for point in self.constraint_glyphs[index].hover.iter() {
                    let first = canvas::Path::line(
                        Point::new(
                            point.x - CONSTRAINT_HOVER_MARKER_RADIUS,
                            point.y - CONSTRAINT_HOVER_MARKER_RADIUS,
                        ),
                        Point::new(
                            point.x + CONSTRAINT_HOVER_MARKER_RADIUS,
                            point.y + CONSTRAINT_HOVER_MARKER_RADIUS,
                        ),
                    );
                    let second = canvas::Path::line(
                        Point::new(
                            point.x - CONSTRAINT_HOVER_MARKER_RADIUS,
                            point.y + CONSTRAINT_HOVER_MARKER_RADIUS,
                        ),
                        Point::new(
                            point.x + CONSTRAINT_HOVER_MARKER_RADIUS,
                            point.y - CONSTRAINT_HOVER_MARKER_RADIUS,
                        ),
                    );
                    frame.stroke(&first, stroke.clone());
                    frame.stroke(&second, stroke.clone());
                }
                if let Some(label) = &self.constraint_glyph_tooltip {
                    let (glyph_top_left, glyph_size) = (
                        self.constraint_glyphs[index].top_left,
                        self.constraint_glyphs[index].size,
                    );
                    let width = (label.chars().count() as f32 * 7.0 + 14.0).max(54.0);
                    let height = 24.0;
                    let left = glyph_top_left
                        .x
                        .clamp(2.0, (bounds.width - width - 2.0).max(2.0));
                    let top = (glyph_top_left.y + glyph_size.height + 4.0)
                        .clamp(2.0, (bounds.height - height - 2.0).max(2.0));
                    let tooltip = canvas::Path::rounded_rectangle(
                        Point::new(left, top),
                        Size::new(width, height),
                        4.0.into(),
                    );
                    frame.fill(&tooltip, theme.palette().background.strong.color);
                    frame.stroke(
                        &tooltip,
                        canvas::Stroke::default()
                            .with_color(theme.palette().background.strong.text)
                            .with_width(1.0),
                    );
                    frame.fill_text(canvas::Text {
                        content: label.to_string(),
                        position: Point::new(left + width * 0.5, top + height * 0.5),
                        color: theme.palette().background.strong.text,
                        size: iced::Pixels(12.0),
                        align_x: iced::alignment::Horizontal::Center.into(),
                        align_y: iced::alignment::Vertical::Center,
                        shaping: iced::advanced::text::Shaping::Advanced,
                        ..Default::default()
                    });
                }
            }));
        }
        // Small cross at each acquired tracking point.
        if !self.ost_points.is_empty() {
            let tracking_key = TrackingMarksLayerKey {
                bounds,
                palette: *theme.palette(),
                ost_points: self.ost_points.clone(),
            };
            layers.push(state.tracking_marks.draw(renderer, frame_bounds, &tracking_key, |frame| {
            let track_color = theme.palette().primary.base.color.scale_alpha(0.7);
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
            }));
        }

        layers
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
    step_x: f32,
    step_y: f32,
    major_every: u32,
    grid_origin: glam::DVec3,
    grid_axes: (Vec3, Vec3, Vec3),
    limits: Option<(glam::DVec2, glam::DVec2)>,
    style: GridStyle,
) {
    let alpha = (style.opacity as f32 / 100.0).clamp(0.02, 1.0);
    // Major lines stay subtle: ~1.5x the minor alpha with a slightly heavier
    // stroke, matching the reference hierarchy without glaring.
    let major_alpha = (alpha * 1.5).clamp(0.03, 1.0);
    let [r, g, b] = style.color;
    let minor_stroke = canvas::Stroke {
        width: 0.5,
        style: canvas::Style::Solid(Color { r, g, b, a: alpha }),
        ..Default::default()
    };
    let major_stroke = canvas::Stroke {
        width: 0.75,
        style: canvas::Style::Solid(Color {
            r,
            g,
            b,
            a: major_alpha,
        }),
        ..Default::default()
    };
    let geometry = grid_segments(
        view_rot,
        eye,
        bounds,
        step_x,
        step_y,
        major_every,
        grid_origin,
        grid_axes,
        limits,
    );
    if !geometry.segments.is_empty() {
        let path = canvas::Path::new(|builder| {
            for (p0, p1) in &geometry.segments {
                builder.move_to(*p0);
                builder.line_to(*p1);
            }
        });
        frame.stroke(&path, minor_stroke);
    }
    if !geometry.major_segments.is_empty() {
        let path = canvas::Path::new(|builder| {
            for (p0, p1) in &geometry.major_segments {
                builder.move_to(*p0);
                builder.line_to(*p1);
            }
        });
        frame.stroke(&path, major_stroke);
    }
    if style.axes && geometry.axis_extent > 0.0 {
        let (gx, gy, gz) = grid_axes;
        let extent = (geometry.axis_extent + step_x.max(step_y)) * 1.5;
        draw_axes(
            frame,
            view_rot,
            eye,
            bounds,
            extent.max(10.0),
            grid_origin,
            (gx, gy, gz),
            style.bg_luminance,
        );
    }
}

/// Captured inputs and derived clip-space constants for one grid projection.
///
/// Pure data: every method below is one phase of the former monolithic
/// `grid_segments` body, moved verbatim so each can be tested in isolation.
struct GridProjection {
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    grid_origin: glam::DVec3,
    axis1: Vec3,
    axis2: Vec3,
    inv: Mat4,
    plane_normal: Vec3,
    plane_rel: Vec3,
    step_x: f32,
    step_y: f32,
    derivative_x: f32,
    derivative_y: f32,
    major_every: u32,
    row_x_norm: f32,
    row_y_norm: f32,
    row_w_norm: f32,
    grid_clip_origin: glam::Vec4,
    grid_clip_axis1: glam::Vec4,
    grid_clip_axis2: glam::Vec4,
}

impl GridProjection {
    #[allow(clippy::too_many_arguments)]
    fn new(
        view_rot: Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        step_x: f32,
        step_y: f32,
        major_every: u32,
        grid_origin: glam::DVec3,
        grid_axes: (Vec3, Vec3, Vec3),
    ) -> Self {
        // Row norms of the view-projection: for a step vector `v` (w = 0),
        // |(view_rot · v).row| ≤ |v| · row_norm. Hoisted once per rebuild; used
        // by the cross-family finiteness bound below.
        let view_rows = view_rot.transpose();
        let row_x_norm = view_rows.x_axis.truncate().length();
        let row_y_norm = view_rows.y_axis.truncate().length();
        let row_w_norm = view_rows.w_axis.truncate().length();

        // Grid is intentionally restricted to the active UCS XY plane.
        let (gx, gy, _gz) = grid_axes;
        let axis1 = gx.normalize_or(Vec3::X);
        let axis2 = gy.normalize_or(Vec3::Y);
        let inv = view_rot.inverse();
        let plane_normal = axis1.cross(axis2).normalize_or(Vec3::Z);
        let plane_rel = (grid_origin - eye).as_vec3();

        // A full grid step is needed to measure adjacent-line distance, but it
        // is too large for the line's local tangent near the eye. A small
        // derivative keeps the tangent measurable without crossing the eye.
        let derivative_x = (step_x * 0.01).max(1e-4);
        let derivative_y = (step_y * 0.01).max(1e-4);

        // Project an infinite world-grid line to its exact screen-space line and
        // intersect it with the viewport rectangle. This remains valid across the
        // perspective horizon; projecting a finite world-space bounding rectangle
        // is only correct for the perpendicular/top view.
        let grid_clip_origin = view_rot * (grid_origin - eye).as_vec3().extend(1.0);
        let grid_clip_axis1 = view_rot * axis1.extend(0.0);
        let grid_clip_axis2 = view_rot * axis2.extend(0.0);

        Self {
            view_rot,
            eye,
            bounds,
            grid_origin,
            axis1,
            axis2,
            inv,
            plane_normal,
            plane_rel,
            step_x,
            step_y,
            derivative_x,
            derivative_y,
            major_every,
            row_x_norm,
            row_y_norm,
            row_w_norm,
            grid_clip_origin,
            grid_clip_axis1,
            grid_clip_axis2,
        }
    }

    /// World → viewport-local screen via relative-to-eye: subtract the f64 eye
    /// first so grid points near the camera stay precise at UTM-scale coords.
    /// Shared eye-plane guard — see `project_point` (#46).
    fn project(&self, world: glam::DVec3) -> Option<Point> {
        project_point(self.view_rot, self.eye, self.bounds, world)
    }

    /// Same world→screen projection as `project` (bit-identical arithmetic),
    /// additionally returning the homogeneous clip position. The clip
    /// coordinates feed the cross-family finiteness bound in `grid_gap`.
    fn project_clip(&self, world: glam::DVec3) -> Option<(Point, glam::Vec4)> {
        let rel = (world - self.eye).as_vec3();
        let clip = self.view_rot * rel.extend(1.0);
        if !clip.is_finite() || clip.w <= 1e-7 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let screen = Point::new(
            (ndc.x + 1.0) * 0.5 * self.bounds.width,
            (1.0 - ndc.y) * 0.5 * self.bounds.height,
        );
        (screen.x.is_finite() && screen.y.is_finite()).then_some((screen, clip))
    }

    /// Intersect a viewport-local screen ray with the real XY plane. Unlike the
    /// old mid-depth approximation, this covers the complete viewport after an
    /// orbit and also tells us when a ray crosses the perspective horizon.
    /// Returns the world hit together with its validated world→screen
    /// projection (and clip position): every consumer of the hit point needs
    /// `project(world)` as well (gap measurement base), and computing it once
    /// here instead of once per consumer removes a whole projection per
    /// evaluated sample.
    fn unproject(&self, sx: f32, sy: f32) -> Option<(glam::DVec3, Point, glam::Vec4)> {
        let ndc_x = (sx / self.bounds.width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (sy / self.bounds.height) * 2.0;
        let near = self.inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let far = self.inv.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
        let ray = far - near;
        let denom = ray.dot(self.plane_normal);
        if !near.is_finite() || !far.is_finite() || !denom.is_finite() || denom.abs() < 1e-7 {
            return None;
        }
        let t = (self.plane_rel - near).dot(self.plane_normal) / denom;
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
        let world = self.eye + hit.as_dvec3();
        self.project_clip(world).map(|(p, clip)| (world, p, clip))
    }

    /// Use several interior points because the UCS origin may be off-screen or
    /// arbitrarily close to the perspective horizon. `p` is the hit point's
    /// already-computed screen position (see `unproject`).
    fn sample_grid_plane(&self) -> Vec<(glam::Vec2, glam::DVec3, Point)> {
        const SAMPLE_FRACTIONS: [f32; 5] = [0.02, 0.25, 0.5, 0.75, 0.98];
        let mut samples = Vec::with_capacity(SAMPLE_FRACTIONS.len().pow(2));
        for fy in SAMPLE_FRACTIONS {
            for fx in SAMPLE_FRACTIONS {
                let screen = glam::Vec2::new(self.bounds.width * fx, self.bounds.height * fy);
                if let Some((world, p, _)) = self.unproject(screen.x, screen.y) {
                    samples.push((screen, world, p));
                }
            }
        }
        samples
    }

    fn projected_deltas(
        &self,
        world: glam::DVec3,
        p: Point,
        axis: Vec3,
        amount: f32,
    ) -> [Option<glam::Vec2>; 2] {
        [amount, -amount].map(|signed_step| {
            self.project(world + (axis * signed_step).as_dvec3())
                .map(|next| glam::Vec2::new(next.x - p.x, next.y - p.y))
        })
    }

    /// Perpendicular screen gap between the two neighbouring lines of ONE
    /// family at a point on the grid. Measuring the perpendicular component,
    /// rather than point-to-point distance, remains correct for a skewed
    /// perspective grid. Family 0 (lines at a fixed axis1 coordinate) measures
    /// with the axis1 step neighbours against the axis2 tangent; family 1 is
    /// the mirror image — every consumer only reads one family, so computing
    /// both doubled the projections per evaluated sample for nothing.
    ///
    /// BUG FIX: the tangent used a single shared step for both families, so
    /// a non-square GRIDUNIT (step_x != step_y) measured the wrong local
    /// direction. Each family now uses its own derivative.
    fn family_gap(&self, p: Point, world: glam::DVec3, family: usize) -> f32 {
        let (neighbour_axis, tangent_axis, step, derivative) = if family == 0 {
            (self.axis1, self.axis2, self.step_x, self.derivative_y)
        } else {
            (self.axis2, self.axis1, self.step_y, self.derivative_x)
        };
        let neighbours = self.projected_deltas(world, p, neighbour_axis, step);
        let tangents = self.projected_deltas(world, p, tangent_axis, derivative);

        // At the near side of a perspective plane a large +step neighbour may
        // cross behind the eye while the -step neighbour remains perfectly
        // visible (or vice versa). Requiring only +X/+Y cut away that entire
        // near side after zoom-out. Use whichever visible neighbour gives the
        // readable separation for each line family.
        let mut gap = 0.0_f32;
        for neighbour in neighbours.into_iter().flatten() {
            for tangent in tangents.into_iter().flatten() {
                let tangent_len = tangent.length();
                if tangent_len > 1e-6 {
                    let area = (neighbour.x * tangent.y - neighbour.y * tangent.x).abs();
                    gap = gap.max(area / tangent_len);
                }
            }
        }
        gap
    }

    /// Both families in one pass (shared projections), used where both are
    /// read — the anchors for the two line families. The old code ran the
    /// full two-family gap math once per family over the same samples.
    fn grid_gaps(&self, p: Point, world: glam::DVec3) -> Option<(f32, f32)> {
        let gap1 = self.family_gap(p, world, 0);
        let gap2 = self.family_gap(p, world, 1);
        (gap1.is_finite() && gap2.is_finite()).then_some((gap1, gap2))
    }

    /// Conservative bound proving that the OTHER family's gap must be finite
    /// at this world point, so its projections can be skipped.
    ///
    /// The other family's gap is `max(|cross product| / tangent_len)` over
    /// projections of `world ± step·axis`; it is non-finite only when one of
    /// those cross products overflows f32 (~3.4e38), which needs screen-space
    /// neighbour/tangent deltas of ~1e19 px. Bound every projection of
    /// `world ± v` (|v| = `step`, a unit axis): its clip position is the
    /// point's own clip plus `view_rot · v`, and any projection that survives
    /// the eye-plane guard has |ndc| ≤ |clip_xy| · 1e7 (clip.w > 1e-7) — or
    /// the tighter |clip_xy| / w_min when the step cannot push the neighbour
    /// near the eye plane. Screen deltas are then ≤ 2·bound, the cross product
    /// ≤ 8·bound², and the gap ≤ 8e6·bound² (tangent_len > 1e-6), so
    /// bound² < 4e31 guarantees finiteness. When the bound does not apply
    /// (absurd steps ~1e9 world units, degenerate view matrices), the cross
    /// gap is computed exactly as before — correct, just not faster.
    fn cross_gap_bounded(&self, clip: glam::Vec4, step: f32) -> bool {
        // The tangent derivative is max(0.01·step, 1e-4), never longer than
        // max(step, 1e-4) — bound both the neighbour and the tangent steps.
        // 1.01 headroom for f32 rounding in the neighbour's mat-vec product.
        let step = step.max(1e-4) * 1.01;
        let w_min = clip.w - step * self.row_w_norm;
        let inv_w = if w_min > 1e-7 { 1.0 / w_min } else { 1e7_f32 };
        let ndc_bound_x = (clip.x.abs() + step * self.row_x_norm) * inv_w;
        let ndc_bound_y = (clip.y.abs() + step * self.row_y_norm) * inv_w;
        let bound_x = (ndc_bound_x + 1.0) * 0.5 * self.bounds.width;
        let bound_y = (ndc_bound_y + 1.0) * 0.5 * self.bounds.height;
        let bound = bound_x.max(bound_y);
        bound.is_finite() && bound * bound < 4.0e31
    }

    /// One family, for the visibility predicates (`trim_line`, `collect_extent`).
    ///
    /// EXACT-semantics note: the old two-family helper returned `None` when
    /// EITHER family's gap was non-finite, but every caller immediately
    /// compared only its own family against `MIN_HORIZON_GRID_PX`. A non-finite
    /// gap is `>= MIN_HORIZON_GRID_PX` too, so the own-family gate is kept
    /// verbatim, and the cross-family finiteness gate is only evaluated when
    /// it is observable — i.e. when this family's gap actually passes the
    /// threshold (otherwise both the old `None` and the new `Some(gap)` read
    /// as "not visible"). Where the gate is observable, it is answered with
    /// the provable `cross_gap_bounded` bound (no-op cost) or, failing that,
    /// by computing the cross gap with the same math as before — the
    /// visibility predicate stays bit-identical to the old code.
    fn grid_gap(
        &self,
        p: Point,
        clip: glam::Vec4,
        world: glam::DVec3,
        family: usize,
    ) -> Option<f32> {
        let gap = self.family_gap(p, world, family);
        if !gap.is_finite() {
            return None;
        }
        if gap >= MIN_HORIZON_GRID_PX {
            let cross_step = if family == 0 {
                self.step_y
            } else {
                self.step_x
            };
            if !self.cross_gap_bounded(clip, cross_step)
                && !self.family_gap(p, world, 1 - family).is_finite()
            {
                return None;
            }
        }
        Some(gap)
    }

    /// Trace a family-specific visible region around the viewport perimeter.
    /// When a boundary ray points through the horizon, binary-search back toward
    /// a readable anchor and stop where neighbouring lines reach the minimum gap.
    fn collect_extent(
        &self,
        family: usize,
        anchor_screen: glam::Vec2,
        anchor_world: glam::DVec3,
    ) -> Vec<glam::DVec3> {
        let visible_at = |screen: glam::Vec2| -> Option<glam::DVec3> {
            let (world, p, clip) = self.unproject(screen.x, screen.y)?;
            let gap = self.grid_gap(p, clip, world, family)?;
            (gap >= MIN_HORIZON_GRID_PX).then_some(world)
        };

        const EDGE_STEPS: usize = 12;
        const SEARCH_STEPS: usize = 16;
        let mut hits = vec![anchor_world];
        for i in 0..=EDGE_STEPS {
            let f = i as f32 / EDGE_STEPS as f32;
            let targets = [
                glam::Vec2::new(self.bounds.width * f, 0.0),
                glam::Vec2::new(self.bounds.width * f, self.bounds.height),
                glam::Vec2::new(0.0, self.bounds.height * f),
                glam::Vec2::new(self.bounds.width, self.bounds.height * f),
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
    }

    fn best_anchors(
        &self,
        samples: &[(glam::Vec2, glam::DVec3, Point)],
    ) -> [Option<(glam::Vec2, glam::DVec3, f32)>; 2] {
        let mut best: [Option<(glam::Vec2, glam::DVec3, f32)>; 2] = [None, None];
        for (screen, world, p) in samples {
            let Some((gap1, gap2)) = self.grid_gaps(*p, *world) else {
                continue;
            };
            for (slot, gap) in best.iter_mut().zip([gap1, gap2]) {
                if slot.map_or(true, |(_, _, best_gap)| gap > best_gap) {
                    *slot = Some((*screen, *world, gap));
                }
            }
        }
        best
    }

    fn axis_range(&self, hits: &[glam::DVec3], axis: Vec3) -> Option<(f32, f32)> {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for world in hits {
            let value = (*world - self.grid_origin).as_vec3().dot(axis);
            if value.is_finite() {
                min = min.min(value);
                max = max.max(value);
            }
        }
        (min <= max).then_some((min, max))
    }

    fn line_range(&self, min: f32, max: f32, anchor: f32, step: f32) -> (i32, i32) {
        let mut start = (min / step).floor() as i32;
        let mut end = (max / step).ceil() as i32;
        // The pixel-gap cut-off naturally bounds this by viewport resolution. Keep
        // malformed projection data from creating an unbounded CPU loop.
        let limit = ((self.bounds.width + self.bounds.height).ceil() as i32 + 64).max(128);
        let center = (anchor / step).round() as i32;
        start = start.max(center.saturating_sub(limit));
        end = end.min(center.saturating_add(limit));
        (start, end)
    }

    fn project_line(&self, family: usize, value: f32) -> Option<(glam::Vec2, glam::Vec2)> {
        let (base, direction) = if family == 0 {
            (
                self.grid_clip_origin + self.grid_clip_axis1 * value,
                self.grid_clip_axis2,
            )
        } else {
            (
                self.grid_clip_origin + self.grid_clip_axis2 * value,
                self.grid_clip_axis1,
            )
        };
        let screen_h = |clip: glam::Vec4| {
            glam::Vec3::new(
                (clip.x + clip.w) * 0.5 * self.bounds.width,
                (-clip.y + clip.w) * 0.5 * self.bounds.height,
                clip.w,
            )
        };
        let line = screen_h(base).cross(screen_h(base + direction));
        if !line.is_finite() || line.x.abs() + line.y.abs() < 1e-8 {
            return None;
        }

        const EDGE_EPS: f32 = 0.5;
        // Stack buffer instead of `Vec::with_capacity(4)`: `project_line` runs
        // once per grid line, and this path used to allocate a heap buffer per
        // line just to hold at most four viewport-edge intersections.
        let mut points = [glam::Vec2::ZERO; 4];
        let mut point_count = 0usize;
        let mut add_point = |point: glam::Vec2| {
            if !point.is_finite()
                || point.x < -EDGE_EPS
                || point.x > self.bounds.width + EDGE_EPS
                || point.y < -EDGE_EPS
                || point.y > self.bounds.height + EDGE_EPS
            {
                return;
            }
            let point = glam::Vec2::new(
                point.x.clamp(0.0, self.bounds.width),
                point.y.clamp(0.0, self.bounds.height),
            );
            if points[..point_count]
                .iter()
                .all(|p: &glam::Vec2| p.distance_squared(point) > 1e-4)
            {
                points[point_count] = point;
                point_count += 1;
            }
        };
        if line.y.abs() > 1e-8 {
            add_point(glam::Vec2::new(0.0, -line.z / line.y));
            add_point(glam::Vec2::new(
                self.bounds.width,
                -(line.x * self.bounds.width + line.z) / line.y,
            ));
        }
        if line.x.abs() > 1e-8 {
            add_point(glam::Vec2::new(-line.z / line.x, 0.0));
            add_point(glam::Vec2::new(
                -(line.y * self.bounds.height + line.z) / line.x,
                self.bounds.height,
            ));
        }
        if point_count < 2 {
            return None;
        }

        let mut best = (points[0], points[1]);
        let mut best_distance = best.0.distance_squared(best.1);
        for i in 0..point_count {
            for j in i + 1..point_count {
                let distance = points[i].distance_squared(points[j]);
                if distance > best_distance {
                    best = (points[i], points[j]);
                    best_distance = distance;
                }
            }
        }
        Some(best)
    }

    /// Keep only the parts of one projected line where its neighbouring line is
    /// at least the configured physical-pixel gap away. The visible interval is
    /// found in screen space, so oblique/trapezoidal views no longer inherit
    /// rectangular world bounds from the top view. Segments are pushed into
    /// `out` so a fully hidden line no longer allocates (and drops) an empty
    /// `Vec` per line.
    fn trim_line(
        &self,
        family: usize,
        p0: glam::Vec2,
        p1: glam::Vec2,
        out: &mut Vec<(Point, Point)>,
    ) {
        let visible = |t: f32| {
            let screen = p0.lerp(p1, t);
            let Some((world, p, clip)) = self.unproject(screen.x, screen.y) else {
                return false;
            };
            let Some(gap) = self.grid_gap(p, clip, world, family) else {
                return false;
            };
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
                    out.push((
                        Point::new(a.x + self.bounds.x, a.y + self.bounds.y),
                        Point::new(b.x + self.bounds.x, b.y + self.bounds.y),
                    ));
                }
            }
            previous_t = t;
            previous_visible = is_visible;
        }
        if let Some(start) = run_start {
            let a = p0.lerp(p1, start);
            out.push((
                Point::new(a.x + self.bounds.x, a.y + self.bounds.y),
                Point::new(p1.x + self.bounds.x, p1.y + self.bounds.y),
            ));
        }
    }

    fn is_major(&self, index: i32) -> bool {
        self.major_every >= 2 && (index.rem_euclid(self.major_every as i32) == 0)
    }

    fn coordinate_range(&self, corners: &[glam::DVec3; 4], axis: Vec3) -> (f32, f32) {
        corners
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), corner| {
                let value = (*corner - self.grid_origin).as_vec3().dot(axis);
                (min.min(value), max.max(value))
            })
    }

    fn clip_world_line(
        &self,
        family: usize,
        value: f32,
        limit_min: glam::DVec2,
        limit_max: glam::DVec2,
    ) -> Option<(Point, Point)> {
        let (base, direction) = if family == 0 {
            (
                self.grid_origin + (self.axis1 * value).as_dvec3(),
                self.axis2.as_dvec3(),
            )
        } else {
            (
                self.grid_origin + (self.axis2 * value).as_dvec3(),
                self.axis1.as_dvec3(),
            )
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
        let p0 = self.project(base + direction * t0)?;
        let p1 = self.project(base + direction * t1)?;
        let local_bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: self.bounds.width,
            height: self.bounds.height,
        };
        clip_seg(p0, p1, local_bounds).map(|(p0, p1)| {
            (
                Point::new(p0.x + self.bounds.x, p0.y + self.bounds.y),
                Point::new(p1.x + self.bounds.x, p1.y + self.bounds.y),
            )
        })
    }

    /// LIMITS bounds the grid, not the UCS axes. Size the axes from the
    /// visible grid plane so X/Y/Z still span the viewport even when the
    /// finite grid rectangle is small or currently off-screen.
    fn limits_extent(&self, samples: &[(glam::Vec2, glam::DVec3, Point)]) -> f32 {
        samples
            .iter()
            .fold(0.0_f32, |extent, (_, world, _)| {
                let delta = (*world - self.grid_origin).as_vec3();
                extent
                    .max(delta.dot(self.axis1).abs())
                    .max(delta.dot(self.axis2).abs())
            })
    }

    /// A finite LIMITS rectangle replaces the usual viewport/horizon extent.
    /// Clip each UCS grid line analytically against the WCS XY rectangle, then
    /// project only that finite segment. This keeps the grid bounded even when
    /// the active UCS is rotated.
    fn segments_within_limits(
        &self,
        samples: &[(glam::Vec2, glam::DVec3, Point)],
        limit_min: glam::DVec2,
        limit_max: glam::DVec2,
    ) -> GridGeometry {
        let corners = [
            glam::DVec3::new(limit_min.x, limit_min.y, self.grid_origin.z),
            glam::DVec3::new(limit_max.x, limit_min.y, self.grid_origin.z),
            glam::DVec3::new(limit_max.x, limit_max.y, self.grid_origin.z),
            glam::DVec3::new(limit_min.x, limit_max.y, self.grid_origin.z),
        ];
        let (min1, max1) = self.coordinate_range(&corners, self.axis1);
        let (min2, max2) = self.coordinate_range(&corners, self.axis2);
        let mut minor = Vec::new();
        let mut major = Vec::new();
        let [anchor1, anchor2] = self.best_anchors(samples);
        if let Some((_, anchor_world, gap)) = anchor1 {
            if gap >= MIN_HORIZON_GRID_PX {
                let anchor = (anchor_world - self.grid_origin).as_vec3().dot(self.axis1);
                let (start, end) = self.line_range(min1, max1, anchor, self.step_x);
                for index in start..=end {
                    if let Some(segment) =
                        self.clip_world_line(0, index as f32 * self.step_x, limit_min, limit_max)
                    {
                        if self.is_major(index) {
                            major.push(segment);
                        } else {
                            minor.push(segment);
                        }
                    }
                }
            }
        }
        if let Some((_, anchor_world, gap)) = anchor2 {
            if gap >= MIN_HORIZON_GRID_PX {
                let anchor = (anchor_world - self.grid_origin).as_vec3().dot(self.axis2);
                let (start, end) = self.line_range(min2, max2, anchor, self.step_y);
                for index in start..=end {
                    if let Some(segment) =
                        self.clip_world_line(1, index as f32 * self.step_y, limit_min, limit_max)
                    {
                        if self.is_major(index) {
                            major.push(segment);
                        } else {
                            minor.push(segment);
                        }
                    }
                }
            }
        }
        let mut axis_extent = 0.0_f32;
        let limits_extent = self.limits_extent(samples);
        if limits_extent > 0.0 {
            axis_extent = limits_extent;
        }
        GridGeometry {
            segments: minor,
            major_segments: major,
            axis_extent,
        }
    }

    fn segments_without_limits(
        &self,
        samples: &[(glam::Vec2, glam::DVec3, Point)],
    ) -> GridGeometry {
        let mut all_segments: Vec<(Point, Point)> = Vec::new();
        let mut all_major: Vec<(Point, Point)> = Vec::new();
        let mut axis_extent = 0.0_f32;
        let [anchor1, anchor2] = self.best_anchors(samples);
        // Lines parallel to axis2 (varying axis1 position).
        if let Some((anchor_screen, anchor_world, gap)) = anchor1 {
            if gap >= MIN_HORIZON_GRID_PX {
                let hits = self.collect_extent(0, anchor_screen, anchor_world);
                if let (Some((min1, max1)), Some((min2, max2))) = (
                    self.axis_range(&hits, self.axis1),
                    self.axis_range(&hits, self.axis2),
                ) {
                    let anchor1 = (anchor_world - self.grid_origin).as_vec3().dot(self.axis1);
                    let (start, end) = self.line_range(min1, max1, anchor1, self.step_x);
                    for i in start..=end {
                        let value = i as f32 * self.step_x;
                        if let Some((p0, p1)) = self.project_line(0, value) {
                            if self.is_major(i) {
                                self.trim_line(0, p0, p1, &mut all_major);
                            } else {
                                self.trim_line(0, p0, p1, &mut all_segments);
                            }
                        }
                    }
                    axis_extent = axis_extent
                        .max(min1.abs().max(max1.abs()).max(min2.abs()).max(max2.abs()));
                }
            }
        }

        // Lines parallel to axis1 (varying axis2 position).
        if let Some((anchor_screen, anchor_world, gap)) = anchor2 {
            if gap >= MIN_HORIZON_GRID_PX {
                let hits = self.collect_extent(1, anchor_screen, anchor_world);
                if let (Some((min1, max1)), Some((min2, max2))) = (
                    self.axis_range(&hits, self.axis1),
                    self.axis_range(&hits, self.axis2),
                ) {
                    let anchor2 = (anchor_world - self.grid_origin).as_vec3().dot(self.axis2);
                    let (start, end) = self.line_range(min2, max2, anchor2, self.step_y);
                    for i in start..=end {
                        let value = i as f32 * self.step_y;
                        if let Some((p0, p1)) = self.project_line(1, value) {
                            if self.is_major(i) {
                                self.trim_line(1, p0, p1, &mut all_major);
                            } else {
                                self.trim_line(1, p0, p1, &mut all_segments);
                            }
                        }
                    }
                    axis_extent = axis_extent
                        .max(min1.abs().max(max1.abs()).max(min2.abs()).max(max2.abs()));
                }
            }
        }

        GridGeometry {
            segments: all_segments,
            major_segments: all_major,
            axis_extent,
        }
    }
}

/// Pure, renderer-free projection of the grid for one pane. Returns canvas-local
/// `(Point, Point)` segments plus the axis extent (in world units along the
/// active UCS axes) used by `draw_grid` to size the coloured UCS axes overlay.
///
/// Extracted from `draw_grid` so the geometry construction can be unit-tested
/// and benchmarked without an iced `Renderer`. Behaviour is identical to the
/// inlined version that preceded it.
/// The phases live on `GridProjection`; this orchestrator only
/// validates the inputs, samples the plane and picks the LIMITS or infinite
/// branch.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn grid_segments(
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    step_x: f32,
    step_y: f32,
    major_every: u32,
    grid_origin: glam::DVec3,
    grid_axes: (Vec3, Vec3, Vec3),
    limits: Option<(glam::DVec2, glam::DVec2)>,
) -> GridGeometry {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return GridGeometry::empty();
    }

    let projection = GridProjection::new(
        view_rot,
        eye,
        bounds,
        step_x,
        step_y,
        major_every,
        grid_origin,
        grid_axes,
    );
    let samples = projection.sample_grid_plane();
    if samples.is_empty() {
        return GridGeometry::empty();
    }

    // Step follows camera zoom only. The previous visible-sample calculation
    // changed depth while orbiting and made the grid jump 1 → 5 → 25.
    // BUG FIX: a single shared step forced square grids; X and Y are now
    // validated independently so GRIDUNIT X/Y resize each family.
    if !step_x.is_finite() || step_x <= 0.0 || !step_y.is_finite() || step_y <= 0.0 {
        return GridGeometry::empty();
    }

    if let Some((limit_min, limit_max)) = limits {
        projection.segments_within_limits(&samples, limit_min, limit_max)
    } else {
        projection.segments_without_limits(&samples)
    }
}

// ── Coloured UCS axes ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
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
    let axis_stroke = |r: f32, g: f32, b: f32| canvas::Stroke {
        width: 1.5,
        style: canvas::Style::Solid(Color { r, g, b, a: 0.85 }),
        ..Default::default()
    };
    let y_green = if bg_luminance > 0.5 { 0.60 } else { 0.85 };
    for ((a, b), (r, g, bl)) in axes_segments(view_rot, eye, bounds, extent, origin, axes)
        .into_iter()
        .zip([
            (0.90, 0.20, 0.20),
            (0.20, y_green, 0.20),
            (0.20, 0.40, 0.90),
        ])
    {
        frame.stroke(
            &canvas::Path::new(|p| {
                p.move_to(a);
                p.line_to(b);
            }),
            axis_stroke(r, g, bl),
        );
    }
}

/// Pure geometry for [`draw_axes`]: per-axis canvas-space segments in X/Y/Z
/// order after the eye-plane guard and frame clip. An axis with either end
/// behind the eye is absent entirely (#46) — screen-space clipping of a
/// mirror-wrapped segment would be garbage, same policy as grid lines.
/// Extracted so the skip/clip/offset contract is unit-testable without a
/// renderer (iced `Frame` needs one).
fn axes_segments(
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    extent: f32,
    origin: glam::DVec3,
    axes: (Vec3, Vec3, Vec3),
) -> Vec<(Point, Point)> {
    // Axes run through the UCS origin along the UCS axis directions. NOTE: the
    // pane origin offset below reproduces draw_axes' historical canvas placement
    // verbatim — whether grid geometry as a whole should be pane-local or
    // canvas-placed is a separate open question (see #60), not this fix.
    let w2s = |world: glam::DVec3| -> Option<Point> {
        project_point(view_rot, eye, bounds, world)
            .map(|p| Point::new(p.x + bounds.x, p.y + bounds.y))
    };
    // clip_seg works frame-local: shift, clip, shift back.
    let local = |p: Point| Point::new(p.x - bounds.x, p.y - bounds.y);
    let back = |p: Point| Point::new(p.x + bounds.x, p.y + bounds.y);
    let (ax, ay, az) = axes;
    [ax, ay, az]
        .into_iter()
        .filter_map(|dir| {
            let (p0, p1) = (
                w2s(origin - (dir * extent).as_dvec3())?,
                w2s(origin + (dir * extent).as_dvec3())?,
            );
            let (c0, c1) = clip_seg(local(p0), local(p1), bounds)?;
            Some((back(c0), back(c1)))
        })
        .collect()
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
    let Some((icon_origin, at_origin, geom)) = ucs_icon_geometry(vp, bounds, axes, origin_screen)
    else {
        return;
    };

    // The icon is interactive wherever it is drawn (including parked in the
    // corner), so hover/selection highlight applies there too.
    let _ = at_origin;
    let highlight = hover || selected;
    let is_light_bg =
        0.299 * crosshair_bg[0] + 0.587 * crosshair_bg[1] + 0.114 * crosshair_bg[2] > 0.5;
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
    /// Optional exact viewport position, used by rotated rectangle dimensions.
    pub center: Option<Point>,
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
            style: canvas::Style::Solid(theme.palette().background.neutral.color.scale_alpha(0.9)),
            line_dash: canvas::LineDash {
                segments: &[2.0, 3.0],
                offset: 0,
            },
            ..Default::default()
        }
    }

    fn box_content(b: &DynBox) -> String {
        match b.role {
            // Formatted live values already carry their unit marker; typed
            // buffers stay unadorned while they are being edited.
            DynRole::Angle => b.value.clone(),
            _ if b.label.is_empty() => b.value.clone(),
            _ => format!("{}{}", b.label, b.value),
        }
    }

    fn box_width(b: &DynBox) -> f32 {
        (Self::box_content(b).len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0
    }

    /// Slide a box centred at `center` straight away from the crosshair until
    /// it neither covers the aim point nor a box already placed (#1546).
    fn clear_of(center: Point, w: f32, cursor: Point, placed: &[iced::Rectangle]) -> Point {
        const KEEP: f32 = 18.0;
        let hits = |c: Point| {
            let r = iced::Rectangle {
                x: c.x - w * 0.5,
                y: c.y - DYN_BOX_H * 0.5,
                width: w,
                height: DYN_BOX_H,
            };
            r.expand(KEEP).contains(cursor) || placed.iter().any(|p| p.intersects(&r))
        };
        let (mut dx, mut dy) = (center.x - cursor.x, center.y - cursor.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1.0 {
            (dx, dy) = (0.0, 1.0);
        } else {
            (dx, dy) = (dx / len, dy / len);
        }
        let mut c = center;
        for _ in 0..60 {
            if !hits(c) {
                break;
            }
            c.x += dx * 4.0;
            c.y += dy * 4.0;
        }
        c
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
        let rect = canvas::Path::rectangle(
            Point { x, y },
            Size {
                width: w,
                height: DYN_BOX_H,
            },
        );
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
            position: Point {
                x: x + DYN_PAD,
                y: y + DYN_PAD,
            },
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
            content: self.prompt.clone(),
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

    fn draw_tracking_hint(&self, frame: &mut canvas::Frame, pos: Point, theme: &Theme) {
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
        let corner = Point {
            x: cursor.x,
            y: base.y,
        }; // axis-delta elbow

        // Perp / PerpDim: perpendicular direction to the reference line, the
        // measured endpoint along it (`end`), and an offset dimension segment
        // (`off_base`→`off_end`) drawn clear of the edge for PerpDim.
        let perp_info = self.ref_screen.map(|r| {
            let (ax, ay) = (r.x - base.x, r.y - base.y);
            let al = (ax * ax + ay * ay).sqrt().max(1.0);
            let (ux, uy) = (ax / al, ay / al); // axis unit (base → ref)
            let (px, py) = (-uy, ux); // perpendicular unit
            let signed = (cursor.x - base.x) * px + (cursor.y - base.y) * py;
            let end = Point {
                x: base.x + px * signed,
                y: base.y + py * signed,
            };
            const OFF: f32 = 16.0; // dimension offset, away from the reference
            let off_base = Point {
                x: base.x - ux * OFF,
                y: base.y - uy * OFF,
            };
            let off_end = Point {
                x: end.x - ux * OFF,
                y: end.y - uy * OFF,
            };
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
                        p.line_to(Point {
                            x: base.x,
                            y: cursor.y,
                        });
                        p.line_to(cursor);
                    });
                    frame.stroke(&rest, Self::dotted(theme));
                }
            }
            DynGuide::None => {}
        }

        // ── Box placement by role ──
        let mut placed: Vec<iced::Rectangle> = Vec::with_capacity(self.boxes.len());
        for b in &self.boxes {
            let center = b.center.unwrap_or_else(|| match b.role {
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
                    y: if self.guide == DynGuide::RectSides {
                        base.y - (cursor.y - base.y).signum() * 14.0
                    } else {
                        base.y + 14.0
                    },
                },
                DynRole::Y | DynRole::Height => Point {
                    x: if self.guide == DynGuide::RectSides {
                        corner.x + (cursor.x - base.x).signum() * 18.0
                    } else {
                        corner.x + 18.0
                    },
                    y: (base.y + cursor.y) * 0.5,
                },
                // Perpendicular measure: on the measured segment / dim line.
                _ if matches!(self.guide, DynGuide::Perp | DynGuide::PerpDim)
                    && perp_info.is_some() =>
                {
                    let (end, ob, oe) = perp_info.unwrap();
                    if self.guide == DynGuide::PerpDim {
                        Point {
                            x: (ob.x + oe.x) * 0.5 + 8.0,
                            y: (ob.y + oe.y) * 0.5,
                        }
                    } else {
                        Point {
                            x: (base.x + end.x) * 0.5 + 8.0,
                            y: (base.y + end.y) * 0.5,
                        }
                    }
                }
                // Distance / Radius / Diameter and anything else ride the line.
                _ => Point {
                    x: base.x + dx * len * 0.5 + nx * 16.0,
                    y: base.y + dy * len * 0.5 + ny * 16.0,
                },
            });
            let w = Self::box_width(b);
            let center = Self::clear_of(center, w, cursor, &placed);
            placed.push(iced::Rectangle {
                x: center.x - w * 0.5,
                y: center.y - DYN_BOX_H * 0.5,
                width: w,
                height: DYN_BOX_H,
            });
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

            self.draw_tracking_hint(frame, Point { x: hx, y: hy }, theme);
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
        let total_w: f32 = widths.iter().sum::<f32>() + DYN_GAP * (self.boxes.len() as f32 - 1.0);

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
            let rect = canvas::Path::rectangle(
                Point { x, y: by },
                Size {
                    width: w,
                    height: DYN_BOX_H,
                },
            );
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
                position: Point {
                    x: x + DYN_PAD,
                    y: by + DYN_PAD,
                },
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

            self.draw_tracking_hint(frame, Point { x: bx, y: hy }, theme);
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

                    self.draw_tracking_hint(&mut frame, Point { x: px, y: hint_y }, theme);
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
    // Shared eye-plane projection shared by grid_segments and draw_axes (#46).
    // Behind-eye points must project to None, never to a mirrored finite point.
    #[test]
    fn project_point_rejects_behind_eye() {
        use glam::camera::rh::proj::directx;
        let eye = glam::DVec3::new(0.0, 0.0, 500.0);
        let view_rot: glam::Mat4 =
            directx::perspective(60_f32.to_radians(), 800.0 / 600.0, 0.5, 5000.0);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        assert!(
            project_point(view_rot, eye, bounds, glam::DVec3::new(0.0, 0.0, 600.0)).is_none(),
            "behind-eye must be rejected"
        );
        assert!(
            project_point(view_rot, eye, bounds, glam::DVec3::new(0.0, 0.0, 400.0)).is_some(),
            "in-front must project"
        );
    }

    fn axes_cam() -> (glam::Mat4, glam::DVec3) {
        use glam::camera::rh::proj::directx;
        let eye = glam::DVec3::new(0.0, 0.0, 500.0);
        let view_rot: glam::Mat4 =
            directx::perspective(60_f32.to_radians(), 800.0 / 600.0, 0.5, 5000.0);
        (view_rot, eye)
    }

    /// draw_axes-level contract without a renderer: all three axes present up
    /// front, each clipped inside the frame.
    #[test]
    fn axes_segments_draws_three_when_in_front() {
        let (view_rot, eye) = axes_cam();
        let segs = super::axes_segments(
            view_rot,
            eye,
            b(),
            100.0,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
        );
        assert_eq!(segs.len(), 3, "all three axes must survive");
        for (a, c) in &segs {
            for p in [a, c] {
                assert!(
                    p.x >= -16.0 && p.x <= 816.0 && p.y >= -16.0 && p.y <= 616.0,
                    "segment must be frame-clipped, got ({}, {})",
                    p.x,
                    p.y
                );
            }
        }
    }

    /// Straddling axis (origin in front, far tip behind the eye) is skipped
    /// whole — screen-space clipping of a wrapped segment would be garbage.
    #[test]
    fn axes_segments_skips_straddling_axis() {
        let (view_rot, eye) = axes_cam();
        // Origin 100 in front; Z extent 200 puts the +Z tip 100 behind.
        let segs = super::axes_segments(
            view_rot,
            eye,
            b(),
            200.0,
            glam::DVec3::new(0.0, 0.0, 400.0),
            (Vec3::X, Vec3::Y, Vec3::Z),
        );
        assert_eq!(segs.len(), 2, "straddling Z axis must be skipped, X/Y kept");
    }

    /// Fully behind-eye origin draws nothing at all.
    #[test]
    fn axes_segments_empty_when_behind_eye() {
        let (view_rot, eye) = axes_cam();
        let segs = super::axes_segments(
            view_rot,
            eye,
            b(),
            100.0,
            glam::DVec3::new(0.0, 0.0, 600.0),
            (Vec3::X, Vec3::Y, Vec3::Z),
        );
        assert!(segs.is_empty(), "nothing projectable, nothing drawn");
    }

    /// Legacy canvas placement: a pane origin offset shifts every segment by
    /// exactly that offset (pins the historical draw_axes mapping).
    #[test]
    fn axes_segments_preserves_pane_offset() {
        let (view_rot, eye) = axes_cam();
        let plain = super::axes_segments(
            view_rot,
            eye,
            b(),
            100.0,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
        );
        let shifted_bounds = iced::Rectangle {
            x: 100.0,
            y: 50.0,
            width: 800.0,
            height: 600.0,
        };
        let shifted = super::axes_segments(
            view_rot,
            eye,
            shifted_bounds,
            100.0,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
        );
        assert_eq!(plain.len(), shifted.len());
        for ((a0, c0), (a1, c1)) in plain.iter().zip(shifted.iter()) {
            for (p0, p1, dx, dy) in [(a0, a1, 100.0, 50.0), (c0, c1, 100.0, 50.0)] {
                assert!(
                    (p1.x - p0.x - dx).abs() < 1e-3 && (p1.y - p0.y - dy).abs() < 1e-3,
                    "pane offset must carry through exactly"
                );
            }
        }
    }
    use super::*;

    fn b() -> iced::Rectangle {
        iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        }
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
        assert!(clip_seg(
            Point::new(-9000.0, -9000.0),
            Point::new(-8000.0, -8000.0),
            b()
        )
        .is_none());
    }
}

#[cfg(test)]
mod constraint_glyph_tests {
    use super::*;
    use crate::scene::parametric_constraints::{constraint_glyph_box, constraint_glyph_offsets};
    use std::sync::Arc;

    /// Build cache-shaped entries the same way
    /// `Scene::cached_glyph_placements` does: one offsets pass, then size +
    /// box per glyph. Keeps this test on the single source of truth instead
    /// of a local layout copy.
    fn entries_for(glyphs: &[(Point, [f32; 2], &str)]) -> Arc<[GlyphEntry]> {
        let offsets = constraint_glyph_offsets(glyphs);
        Arc::from(
            glyphs
                .iter()
                .zip(offsets)
                .enumerate()
                .map(|(i, ((anchor, outward, label), tangent_dx))| {
                    let size = crate::scene::parametric_constraints::constraint_glyph_size(label);
                    let (top_left, _) = constraint_glyph_box(*anchor, *outward, label, tangent_dx);
                    GlyphEntry {
                        id: i as u32,
                        anchor: *anchor,
                        outward: *outward,
                        label: Arc::from(*label),
                        conflicting: false,
                        hover: Arc::from([]),
                        size,
                        tangent_dx,
                        top_left,
                    }
                })
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn enlarged_glyph_box_stays_clear_of_its_geometry_anchor() {
        let anchor = Point::new(100.0, 80.0);
        let (right, size) = constraint_glyph_box(anchor, [1.0, 0.0], "⊥", 0.0);
        assert_eq!(size.height, 22.0);
        assert!((right.x - anchor.x - 6.0).abs() < 1e-6);

        let (above, size) = constraint_glyph_box(anchor, [0.0, -1.0], "↔ 25.00", 0.0);
        assert!((anchor.y - (above.y + size.height) - 6.0).abs() < 1e-6);
    }

    #[test]
    fn coincident_glyphs_are_arranged_side_by_side() {
        let anchor = Point::new(100.0, 80.0);
        let entries = entries_for(&[(anchor, [0.0, -1.0], "—"), (anchor, [0.0, -1.0], "∥")]);
        let (left, left_size) = (entries[0].top_left, entries[0].size);
        let (right, right_size) = (entries[1].top_left, entries[1].size);

        assert!((left.y - right.y).abs() < 1e-6);
        assert!((right.x - (left.x + left_size.width) - 4.0).abs() < 1e-4);
        assert!((anchor.y - (left.y + right_size.height) - 6.0).abs() < 1e-6);

        let click = Point::new(
            left.x + left_size.width * 0.5,
            left.y + left_size.height * 0.5,
        );
        assert_eq!(glyph_hit_test_entries(&entries, click), Some(0));
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
            step_x: 80.0,
            step_y: 80.0,
            major_every: 5,
            origin: glam::DVec3::new(0.0, 0.0, 0.0),
            axes: (Vec3::X, Vec3::Y, Vec3::Z),
            limits: None,
        }
    }

    /// Same `Vec<GridParams>` + same bounds + same style ⇒ keys compare equal.
    #[test]
    fn grid_key_matches_identical_params() {
        let grids = vec![baseline_params(), baseline_params()];
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 720.0,
        };
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
        let baseline_key =
            GridKey::from_grids(&baseline_grids, baseline_bounds, GridStyle::default());

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

        // step_x: zoom in
        let mut p = baseline_params();
        p.step_x = 40.0;
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "step_x change must invalidate"
        );

        // step_y: non-square grid resize
        let mut p = baseline_params();
        p.step_y = 40.0;
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "step_y change must invalidate"
        );

        // major_every: major-line frequency change
        let mut p = baseline_params();
        p.major_every = 10;
        assert_ne!(
            GridKey::from_grids(&[p], baseline_bounds, GridStyle::default()),
            baseline_key,
            "major_every change must invalidate"
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
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 720.0,
        };
        let pane1 = baseline_params();
        let pane2 = baseline_params();
        let both = vec![pane1.clone(), pane2.clone()];
        let baseline = GridKey::from_grids(&both, bounds, GridStyle::default());

        let mut pane2_changed = pane2;
        pane2_changed.step_x = 160.0;
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
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 720.0,
        };
        let key = GridKey::from_grids(&grids, bounds, GridStyle::default());
        assert!(!should_reuse(None, &key));
    }

    /// `should_reuse(Some(&old), &same)` ⇒ `true` (structural equality).
    #[test]
    fn should_reuse_equal() {
        let grids = vec![baseline_params()];
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 720.0,
        };
        let key = GridKey::from_grids(&grids, bounds, GridStyle::default());
        assert!(should_reuse(Some(&key), &key));
    }

    /// `should_reuse(Some(&old), &new)` with keys built from different inputs
    /// ⇒ `false` (must recompute, not serve stale geometry).
    #[test]
    fn should_reuse_changed() {
        let grids_a = vec![baseline_params()];
        let mut pane2 = baseline_params();
        pane2.step_x = 160.0;
        let grids_b = vec![pane2];
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 720.0,
        };
        let old = GridKey::from_grids(&grids_a, bounds, GridStyle::default());
        let new = GridKey::from_grids(&grids_b, bounds, GridStyle::default());
        assert!(!should_reuse(Some(&old), &new));
    }

    /// Adaptive steps grow from the GRIDUNIT base (the old hardcoded 1.0 base
    /// ignored the DSettings grid spacing). Fixed mode returns the base verbatim.
    #[test]
    fn grid_steps_follow_base_and_adaptive_flag() {
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 720.0,
        };
        // Far zoom: 10-unit base must scale up to stay readable.
        let (sx, sy) = compute_grid_steps(10.0, 10.0, 5000.0, 0.6, bounds, true);
        assert!(sx >= 10.0 && sy >= 10.0);
        assert_eq!((sx, sy), (sx.max(10.0), sy.max(10.0)));
        // Non-uniform bases stay non-uniform.
        let (nx, ny) = compute_grid_steps(10.0, 2.0, 5000.0, 0.6, bounds, true);
        assert!(nx >= 10.0 && ny >= 2.0);
        // Fixed mode returns exactly what was typed.
        assert_eq!(
            compute_grid_steps(7.5, 2.5, 5000.0, 0.6, bounds, false),
            (7.5, 2.5)
        );
        // Degenerate input sanitizes to 1.0 instead of emptying the grid.
        assert_eq!(
            compute_grid_steps(0.0, -3.0, 5000.0, 0.6, bounds, false),
            (1.0, 1.0)
        );
    }

    /// Overflow fallback keeps each axis on its own base: an extreme zoom-out
    /// that pushes the 5x growth past 1e9 must return (bx, by), never (bx, bx).
    #[test]
    fn grid_steps_overflow_falls_back_per_axis() {
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 720.0,
        };
        assert_eq!(
            compute_grid_steps(10.0, 2.0, 1e15, 0.6, bounds, true),
            (10.0, 2.0)
        );
    }

    /// Non-uniform steps still produce grid geometry on both families.
    #[test]
    fn grid_segments_support_non_square_spacing() {
        // Top-down orthographic-ish view over the origin: deterministic lines.
        let view_rot = glam::camera::rh::proj::directx::orthographic(
            -400.0, 400.0, -300.0, 300.0, 0.1, 2000.0,
        ) * glam::camera::rh::view::look_at_mat4(
            Vec3::new(0.0, 0.0, 500.0),
            Vec3::ZERO,
            Vec3::Y,
        );
        let eye = glam::DVec3::new(0.0, 0.0, 500.0);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let g = grid_segments(
            view_rot,
            eye,
            bounds,
            10.0,
            2.0,
            5,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
            None,
        );
        assert!(!g.segments.is_empty(), "grid lines expected");
    }

    /// Every-Nth line lands in `major_segments`, the rest in `segments`.
    #[test]
    fn grid_segments_partition_major_lines() {
        // Top-down orthographic-ish view over the origin: deterministic lines.
        let view_rot = glam::camera::rh::proj::directx::orthographic(
            -400.0, 400.0, -300.0, 300.0, 0.1, 2000.0,
        ) * glam::camera::rh::view::look_at_mat4(
            Vec3::new(0.0, 0.0, 500.0),
            Vec3::ZERO,
            Vec3::Y,
        );
        let eye = glam::DVec3::new(0.0, 0.0, 500.0);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let g = grid_segments(
            view_rot,
            eye,
            bounds,
            10.0,
            10.0,
            5,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
            None,
        );
        assert!(!g.segments.is_empty(), "minor lines expected");
        assert!(!g.major_segments.is_empty(), "major lines expected");
        // major_every = 1 disables the split: everything is minor.
        let flat = grid_segments(
            view_rot,
            eye,
            bounds,
            10.0,
            10.0,
            1,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
            None,
        );
        assert!(flat.major_segments.is_empty());
        assert!(!flat.segments.is_empty());
    }
}

#[cfg(test)]
mod grid_segments_tests {
    use super::*;

    /// Top view: X maps 1:1 to pixels (800 px over 800 world units) and Y
    /// likewise (600 px over 600 units), so screen = (x + 400, 300 - y).
    fn top_view() -> (Mat4, glam::DVec3, iced::Rectangle) {
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let view = glam::camera::rh::proj::directx::orthographic(
            -400.0, 400.0, -300.0, 300.0, 0.1, 2000.0,
        ) * glam::camera::rh::view::look_at_mat4(
            Vec3::new(0.0, 0.0, 500.0),
            Vec3::ZERO,
            Vec3::Y,
        );
        (view, glam::DVec3::new(0.0, 0.0, 500.0), bounds)
    }

    fn projection(step_x: f32, step_y: f32, major_every: u32) -> GridProjection {
        let (view, eye, bounds) = top_view();
        GridProjection::new(
            view,
            eye,
            bounds,
            step_x,
            step_y,
            major_every,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
        )
    }

    #[test]
    fn projection_and_unprojection_round_trip_on_the_grid_plane() {
        let grid = projection(10.0, 10.0, 5);
        let world = glam::DVec3::new(10.0, -20.0, 0.0);
        let screen = grid.project(world).expect("front point");
        assert!((screen.x - 410.0).abs() < 1e-3, "{screen:?}");
        assert!((screen.y - 320.0).abs() < 1e-3, "{screen:?}");
        let (back, pixel, clip) = grid.unproject(screen.x, screen.y).expect("plane hit");
        assert!((back - world).length() < 1e-3, "{back:?}");
        assert!((pixel.x - screen.x).abs() < 1e-3 && (pixel.y - screen.y).abs() < 1e-3);
        assert!(clip.w > 0.0);
    }

    #[test]
    fn project_rejects_points_behind_a_perspective_eye() {
        let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
        let view = glam::camera::rh::proj::directx::perspective(
            45.0_f32.to_radians(),
            4.0 / 3.0,
            0.1,
            2000.0,
        );
        let grid = GridProjection::new(
            view,
            glam::DVec3::ZERO,
            bounds,
            10.0,
            10.0,
            5,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
        );
        assert!(grid.project(glam::DVec3::new(0.0, 0.0, -100.0)).is_some());
        assert!(grid.project(glam::DVec3::new(0.0, 0.0, 100.0)).is_none());
    }

    #[test]
    fn samples_cover_the_viewport_from_the_grid_plane() {
        let grid = projection(10.0, 10.0, 5);
        let samples = grid.sample_grid_plane();
        assert_eq!(samples.len(), 25, "5x5 interior samples");
        for (screen, world, pixel) in &samples {
            assert_eq!(world.z, 0.0);
            assert!(
                (pixel.x - screen.x).abs() < 1e-2 && (pixel.y - screen.y).abs() < 1e-2,
                "{pixel:?} vs {screen:?}"
            );
            let reprojected = grid.project(*world).expect("sample world point projects");
            assert!((reprojected.x - pixel.x).abs() < 1e-2);
        }
    }

    #[test]
    fn grid_gaps_measure_the_step_of_each_family() {
        let grid = projection(10.0, 2.0, 5);
        let world = glam::DVec3::new(5.0, 5.0, 0.0);
        let pixel = grid.project(world).expect("front point");
        let (gap1, gap2) = grid.grid_gaps(pixel, world).expect("finite gaps");
        assert!((gap1 - 10.0).abs() < 1e-3, "family 0 gap {gap1}");
        assert!((gap2 - 2.0).abs() < 1e-3, "family 1 gap {gap2}");
    }

    #[test]
    fn axis_and_line_ranges_follow_the_extent_and_anchor() {
        let grid = projection(10.0, 10.0, 5);
        let hits = [
            glam::DVec3::new(10.0, -20.0, 0.0),
            glam::DVec3::new(-5.0, 30.0, 0.0),
        ];
        assert_eq!(grid.axis_range(&hits, Vec3::X), Some((-5.0, 10.0)));
        assert_eq!(grid.axis_range(&hits, Vec3::Y), Some((-20.0, 30.0)));
        assert_eq!(grid.line_range(-5.0, 35.0, 1.0, 10.0), (-1, 4));
    }

    #[test]
    fn project_line_spans_the_viewport_for_both_families() {
        let grid = projection(10.0, 10.0, 5);
        // Family 0 at value 0 is the world Y axis: screen x = 400, y = 0..600.
        let (a, b) = grid.project_line(0, 0.0).expect("line crosses viewport");
        assert!((a.x - 400.0).abs() < 1e-2 && (b.x - 400.0).abs() < 1e-2);
        assert!(a.y.min(b.y) <= 1e-2 && a.y.max(b.y) >= 600.0 - 1e-2);
        // Family 1 at value 0 is the world X axis: screen y = 300, x = 0..800.
        let (a, b) = grid.project_line(1, 0.0).expect("line crosses viewport");
        assert!((a.y - 300.0).abs() < 1e-2 && (b.y - 300.0).abs() < 1e-2);
        assert!(a.x.min(b.x) <= 1e-2 && a.x.max(b.x) >= 800.0 - 1e-2);
    }

    #[test]
    fn trim_line_keeps_a_fully_visible_line() {
        let grid = projection(10.0, 10.0, 5);
        let mut out = Vec::new();
        grid.trim_line(
            0,
            glam::Vec2::new(400.0, 0.0),
            glam::Vec2::new(400.0, 600.0),
            &mut out,
        );
        assert_eq!(out.len(), 1, "a visible line stays one segment");
        let (a, b) = out[0];
        assert!((a.x - 400.0).abs() < 1e-2 && (b.x - 400.0).abs() < 1e-2);
        assert!(a.y.min(b.y) <= 1e-2 && a.y.max(b.y) >= 600.0 - 1e-2);
    }

    #[test]
    fn clip_world_line_stops_at_the_limits_rectangle() {
        let grid = projection(10.0, 10.0, 5);
        let min = glam::DVec2::new(-20.0, -15.0);
        let max = glam::DVec2::new(20.0, 15.0);
        let (a, b) = grid
            .clip_world_line(0, 0.0, min, max)
            .expect("line inside limits");
        // World x = 0 -> screen x = 400; world y = -15..15 -> screen y = 315..285.
        assert!((a.x - 400.0).abs() < 1e-2 && (b.x - 400.0).abs() < 1e-2);
        assert!((a.y.min(b.y) - 285.0).abs() < 1e-2);
        assert!((a.y.max(b.y) - 315.0).abs() < 1e-2);
        assert!(
            grid.clip_world_line(0, 100.0, min, max).is_none(),
            "a line outside the rectangle is clipped away"
        );
    }

    #[test]
    fn is_major_follows_major_every() {
        let grid = projection(10.0, 10.0, 5);
        assert!(grid.is_major(0) && grid.is_major(5) && grid.is_major(-5));
        assert!(!grid.is_major(1) && !grid.is_major(4));
        let flat = projection(10.0, 10.0, 1);
        assert!(!flat.is_major(0), "major_every < 2 disables majors");
    }

    #[test]
    fn grid_segments_is_empty_for_degenerate_input() {
        let (view, eye, _) = top_view();
        let zero = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 600.0,
        };
        let axes = (Vec3::X, Vec3::Y, Vec3::Z);
        let empty = grid_segments(
            view,
            eye,
            zero,
            10.0,
            10.0,
            5,
            glam::DVec3::ZERO,
            axes,
            None,
        );
        assert!(empty.segments.is_empty() && empty.major_segments.is_empty());
        let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
        let nan = grid_segments(
            view,
            eye,
            bounds,
            f32::NAN,
            10.0,
            5,
            glam::DVec3::ZERO,
            axes,
            None,
        );
        assert!(nan.segments.is_empty());
    }

    #[test]
    fn grid_segments_limits_branch_stays_inside_the_rectangle() {
        let (view, eye, bounds) = top_view();
        let limits = Some((
            glam::DVec2::new(-20.0, -15.0),
            glam::DVec2::new(20.0, 15.0),
        ));
        let g = grid_segments(
            view,
            eye,
            bounds,
            10.0,
            10.0,
            5,
            glam::DVec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
            limits,
        );
        assert!(!g.segments.is_empty() || !g.major_segments.is_empty());
        for (a, b) in g.segments.iter().chain(g.major_segments.iter()) {
            for p in [a, b] {
                assert!(p.x >= 380.0 - 1e-2 && p.x <= 420.0 + 1e-2, "{p:?}");
                assert!(p.y >= 285.0 - 1e-2 && p.y <= 315.0 + 1e-2, "{p:?}");
            }
        }
        assert!(g.axis_extent > 0.0);
    }

    #[test]
    fn grid_segments_is_deterministic() {
        let (view, eye, bounds) = top_view();
        let run = || {
            grid_segments(
                view,
                eye,
                bounds,
                10.0,
                5.0,
                5,
                glam::DVec3::ZERO,
                (Vec3::X, Vec3::Y, Vec3::Z),
                None,
            )
        };
        let first = run();
        let second = run();
        assert_eq!(first.segments, second.segments);
        assert_eq!(first.major_segments, second.major_segments);
        assert_eq!(first.axis_extent, second.axis_extent);
    }
}

#[cfg(test)]
mod grid_canvas_state_tests {
    use super::*;

    fn params(step_x: f32) -> GridParams {
        GridParams {
            view_rot: Mat4::IDENTITY,
            eye: glam::DVec3::ZERO,
            bounds: iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 640.0,
                height: 360.0,
            },
            step_x,
            step_y: 10.0,
            major_every: 5,
            origin: glam::DVec3::ZERO,
            axes: (Vec3::X, Vec3::Y, Vec3::Z),
            limits: None,
        }
    }

    fn clip(x: f32) -> iced::Rectangle {
        iced::Rectangle {
            x,
            y: 0.0,
            width: 320.0,
            height: 360.0,
        }
    }

    fn pane_key(params: &GridParams, clip: iced::Rectangle) -> GridKey {
        GridKey::from_grids(std::slice::from_ref(params), clip, GridStyle::default())
    }

    fn store(state: &GridCanvasState, index: usize, key: GridKey) {
        state.resize_panes(index + 1);
        state.panes.borrow_mut()[index] = Some(PaneCache::new(key));
    }

    /// A fresh state has no per-pane entries, so the first draw of a session
    /// misses on every pane instead of replaying geometry it never built.
    #[test]
    fn default_state_has_no_pane_caches() {
        let state = GridCanvasState::default();
        assert!(state.panes.borrow().is_empty());
    }

    /// Resizing follows the pane count: growing appends empty slots, shrinking
    /// drops the tail, and surviving entries stay cached under their index.
    #[test]
    fn resizing_tracks_the_pane_count_and_keeps_existing_entries() {
        let state = GridCanvasState::default();
        let first = pane_key(&params(10.0), clip(0.0));
        store(&state, 0, first.clone());
        state.resize_panes(3);
        assert_eq!(state.panes.borrow().len(), 3);
        assert!(state.panes.borrow()[0].is_some());
        assert!(state.panes.borrow()[1].is_none());
        state.resize_panes(1);
        assert_eq!(state.panes.borrow().len(), 1);
        assert!(state.pane_reuses(0, &first));
    }

    /// The point of the per-pane cache: a change in one tile must not
    /// invalidate the other tile's cached geometry.
    #[test]
    fn a_changed_pane_misses_while_the_other_pane_hits() {
        let state = GridCanvasState::default();
        let other = pane_key(&params(20.0), clip(320.0));
        store(&state, 0, pane_key(&params(10.0), clip(0.0)));
        store(&state, 1, other.clone());
        let changed = pane_key(&params(11.0), clip(0.0));
        assert!(!state.pane_reuses(0, &changed), "pane 0 changed");
        assert!(state.pane_reuses(1, &other), "pane 1 unchanged");
    }

    /// Style is part of every pane's key: a colour/opacity change repaints all.
    #[test]
    fn a_style_change_misses_the_pane() {
        let state = GridCanvasState::default();
        let p = params(10.0);
        store(&state, 0, pane_key(&p, clip(0.0)));
        let mut style = GridStyle::default();
        style.opacity = 50;
        let styled = GridKey::from_grids(std::slice::from_ref(&p), clip(0.0), style);
        assert!(!state.pane_reuses(0, &styled));
    }

    /// The clip rect is part of the key, so a resize or tile-layout change
    /// cannot replay geometry that was clipped for a different rectangle.
    #[test]
    fn a_clip_change_misses_the_pane() {
        let state = GridCanvasState::default();
        let p = params(10.0);
        store(&state, 0, pane_key(&p, clip(0.0)));
        assert!(!state.pane_reuses(0, &pane_key(&p, clip(320.0))));
    }

    /// Pane identity is the vec index; after a layout change moves a pane to a
    /// different index, that slot must miss rather than replay stale geometry.
    #[test]
    fn reordered_panes_miss_instead_of_replaying_stale_geometry() {
        let state = GridCanvasState::default();
        let a = params(10.0);
        let b = params(20.0);
        store(&state, 0, pane_key(&a, clip(0.0)));
        store(&state, 1, pane_key(&b, clip(320.0)));
        assert!(!state.pane_reuses(0, &pane_key(&b, clip(0.0))));
        assert!(!state.pane_reuses(1, &pane_key(&a, clip(320.0))));
    }
}

#[cfg(test)]
mod selection_canvas_state_tests {
    use super::*;

    /// A fresh `SelectionCanvasState` must hold no cached key on any layer —
    /// the first draw of a session always misses, so no layer can replay
    /// geometry it never built.
    #[test]
    fn default_state_has_no_cached_keys() {
        let state = SelectionCanvasState::default();
        assert!(state.dividers.key.borrow().is_none());
        assert!(state.grips.key.borrow().is_none());
        assert!(state.authoring.key.borrow().is_none());
        assert!(state.glyphs.key.borrow().is_none());
        assert!(state.tracking_marks.key.borrow().is_none());
    }

    /// `should_reuse_layer` is the whole hit/miss decision: no stored key
    /// ⇒ miss, equal stored key ⇒ hit, changed stored key ⇒ miss.
    #[test]
    fn should_reuse_layer_decides_on_key_equality() {
        let bounds = iced::Rectangle::with_size(Size::new(1280.0, 720.0));
        let palette = *Theme::Dark.palette();
        let key = TrackingMarksLayerKey {
            bounds,
            palette,
            ost_points: vec![OstTrackPoint {
                screen: Point::new(10.0, 20.0),
            }],
        };
        assert!(!should_reuse_layer(None::<&TrackingMarksLayerKey>, &key));
        assert!(should_reuse_layer(Some(&key), &key));
        let moved = TrackingMarksLayerKey {
            ost_points: vec![OstTrackPoint {
                screen: Point::new(11.0, 20.0),
            }],
            ..key.clone()
        };
        assert!(!should_reuse_layer(Some(&key), &moved));
    }

    fn glyph_entry(id: u32, label: &str, x: f32) -> GlyphEntry {
        GlyphEntry {
            id,
            anchor: Point::new(x, 30.0),
            outward: [1.0, 0.0],
            label: Arc::from(label),
            conflicting: false,
            hover: Arc::from(vec![Point::new(x, 30.0)]),
            size: Size::new(28.0, 22.0),
            tangent_dx: 14.0,
            top_left: Point::new(x - 14.0, 19.0),
        }
    }

    /// The glyph layer's key must reuse across equal-but-separately-allocated
    /// memoised slices (the view rebuilds its Arcs on edits that leave every
    /// glyph in place) and invalidate whenever any key input changes.
    #[test]
    fn glyph_layer_key_reuses_equal_entries_and_invalidates_on_changes() {
        let bounds = iced::Rectangle::with_size(Size::new(800.0, 600.0));
        let palette = *Theme::Dark.palette();
        let selected = Arc::from(vec![true, false]);
        let glyphs = Arc::from(vec![glyph_entry(1, "H", 10.0), glyph_entry(2, "=", 40.0)]);
        let key = GlyphLayerKey::new(bounds, &glyphs, &selected, palette);

        // Separate Arc allocations with identical contents ⇒ reuse.
        let glyphs2 = Arc::from(vec![glyph_entry(1, "H", 10.0), glyph_entry(2, "=", 40.0)]);
        let selected2 = Arc::from(vec![true, false]);
        assert!(!Arc::ptr_eq(&glyphs, &glyphs2));
        assert_eq!(
            key,
            GlyphLayerKey::new(bounds, &glyphs2, &selected2, palette)
        );

        // Changed label ⇒ rebuild.
        let relabelled = Arc::from(vec![glyph_entry(1, "V", 10.0), glyph_entry(2, "=", 40.0)]);
        assert_ne!(
            key,
            GlyphLayerKey::new(bounds, &relabelled, &selected, palette)
        );

        // Flipped selection ring ⇒ rebuild.
        let flipped = Arc::from(vec![false, true]);
        assert_ne!(key, GlyphLayerKey::new(bounds, &glyphs, &flipped, palette));

        // Theme change ⇒ rebuild (the palette feeds pill colors).
        assert_ne!(
            key,
            GlyphLayerKey::new(bounds, &glyphs, &selected, *Theme::Light.palette())
        );

        // Widget resize ⇒ rebuild (clip bounds changed).
        let resized = iced::Rectangle::with_size(Size::new(640.0, 480.0));
        assert_ne!(
            key,
            GlyphLayerKey::new(resized, &glyphs, &selected, palette)
        );

        // A NaN anchor must not pin the layer to a permanent miss: bit-equal
        // entries compare equal even though the float itself never does.
        let nan_a = Arc::from(vec![glyph_entry(9, "F", f32::NAN)]);
        let nan_b = Arc::from(vec![glyph_entry(9, "F", f32::NAN)]);
        let nan_selected = Arc::from(vec![true]);
        assert_eq!(
            GlyphLayerKey::new(bounds, &nan_a, &nan_selected, palette),
            GlyphLayerKey::new(bounds, &nan_b, &nan_selected, palette),
        );
    }

    /// The grips layer key must invalidate when a marker's interaction flag
    /// flips — the hover fill is baked into the cached geometry.
    #[test]
    fn grip_layer_key_invalidates_on_hover_flip() {
        let make_key = |hovered: bool| GripLayerKey {
            bounds: iced::Rectangle::with_size(Size::new(1024.0, 768.0)),
            palette: *Theme::Dark.palette(),
            grips: vec![GripMarker {
                pos: Point::new(12.0, 34.0),
                shape: GripShape::Square,
                is_hot: false,
                is_hovered: hovered,
                dir: None,
                is_plugin: false,
                is_authoring: false,
                is_dynamic: false,
            }],
            control_polygon: None,
            grip_clip: None,
            selection_visual: SelectionVisualOptions::default(),
            authoring_style: AuthoringStyleKey {
                action: Color::WHITE,
                parameter: Color::WHITE,
                grip: Color::WHITE,
                grip_size: 5.0,
                text_size: 12.0,
                font: String::new(),
                horizontal: true,
            },
            feedback_clip: iced::Rectangle::default(),
            feedback_dependencies: Vec::new(),
            feedback_markers: Vec::new(),
        };
        assert_eq!(make_key(false), make_key(false));
        assert_ne!(make_key(false), make_key(true));
    }

    /// The tracking-marks layer must rebuild when a new OST point lands.
    #[test]
    fn tracking_marks_layer_key_invalidates_on_new_point() {
        let bounds = iced::Rectangle::with_size(Size::new(800.0, 600.0));
        let palette = *Theme::Dark.palette();
        let key = TrackingMarksLayerKey {
            bounds,
            palette,
            ost_points: vec![OstTrackPoint {
                screen: Point::new(5.0, 5.0),
            }],
        };
        let mut grown = key.clone();
        grown.ost_points.push(OstTrackPoint {
            screen: Point::new(6.0, 6.0),
        });
        assert_ne!(key, grown);
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
            assert!(
                c.g > 0.0,
                "Theme {:?} crossing should have green component",
                theme
            );
            assert!(
                w.b > 0.0 || w.g > 0.0,
                "Theme {:?} window should have blue/cyan component",
                theme
            );
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
