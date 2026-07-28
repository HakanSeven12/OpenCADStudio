//! Viewport overlay widgets.

use glam::{Mat4, Vec3};
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Element, Length, Point, Size, Theme};

use crate::app::Message;
use crate::scene::model::object::GripShape;
use crate::scene::SelectionState;

/// Half-size of the crosshair center square in screen pixels (square = SQ*2 × SQ*2).
pub const CROSSHAIR_SQ: f32 = 7.5;
/// Arm length of the crosshair from center — used as the snap aperture radius.
pub const CROSSHAIR_ARM: f32 = 60.0;
use crate::snap::SnapType;

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
    /// World-XY direction vector — only consumed by the `Rectangle`
    /// shape to orient the box along its segment. `None` for grips
    /// that don't need rotation.
    pub dir: Option<[f32; 2]>,
}

// ── Grid display params ───────────────────────────────────────────────────

/// Passed to the canvas when the GRID display is active.
#[derive(Clone)]
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
) -> Element<'a, Message> {
    canvas(GridCanvas { grid })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

struct GridCanvas {
    grid: Vec<GridParams>,
}

impl canvas::Program<Message> for GridCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

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
                draw_grid(f, g.view_rot, g.eye, gb, g.step, g.origin, g.axes, g.limits)
            });
        }

        vec![frame.into_geometry()]
    }
}

pub fn selection_overlay<'a>(
    selection: SelectionState,
    snap: Option<(Point, SnapType)>,
    snap_ext_base: Option<Point>,
    snap_ext_base2: Option<Point>,
    grips: Vec<GripMarker>,
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
) -> Element<'a, Message> {
    canvas(SelectionCanvas {
        selection,
        snap,
        snap_ext_base,
        snap_ext_base2,
        grips,
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
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct SelectionCanvas {
    selection: SelectionState,
    snap: Option<(Point, SnapType)>,
    /// Screen position of the endpoint an active Extension snap extends from,
    /// so the dashed extension guide line can be drawn back to it. (#238)
    snap_ext_base: Option<Point>,
    /// Second extension-guide base, present only for an extended intersection so
    /// both crossing extensions stay drawn when the crossing is caught. (#247, #259)
    snap_ext_base2: Option<Point>,
    grips: Vec<GripMarker>,
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
    /// badge by the cursor so the user knows it can't be selected/edited.
    hover_locked: bool,
}

fn draw_grip_marker(frame: &mut canvas::Frame, grip: &GripMarker, theme: &Theme) {
    let sp = grip.pos;
    let h = crate::scene::pick::grip::GRIP_HALF_PX;
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
            b.move_to(Point::new(sp.x, sp.y - h));
            b.line_to(Point::new(sp.x + h, sp.y + h));
            b.line_to(Point::new(sp.x - h, sp.y + h));
            b.close();
        }),
        GripShape::Circle => canvas::Path::circle(Point::new(sp.x, sp.y), h),
    };

    if grip.is_hot {
        frame.fill(&path, theme.extended_palette().danger.base.color);
    } else {
        let palette = theme.extended_palette();
        let color = palette.primary.base.color;
        frame.fill(
            &path,
            palette.background.base.color.scale_alpha(0.7),
        );
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
            return if self.selection.middle_down {
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
        if cursor.is_over(bounds) {
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
            let divider = theme.extended_palette().background.neutral.color;
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
            let accent = theme.extended_palette().primary.base.color;
            // Source pane: dimmed + dashed-feel outline (it has been lifted).
            let src_path =
                canvas::Path::rectangle(Point::new(src.x, src.y), iced::Size::new(src.width, src.height));
            frame.fill(
                &src_path,
                theme.extended_palette().background.strong.color.scale_alpha(0.28),
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
            if let Some(c) = self.selection.last_move_pos {
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
        ) {
            let base = if crossing {
                theme.extended_palette().success.base.color
            } else {
                theme.extended_palette().primary.base.color
            };
            let fill = base.scale_alpha(0.12);
            let stroke = base.scale_alpha(0.9);
            let x0 = a.x.min(b.x);
            let y0 = a.y.min(b.y);
            let w = (a.x - b.x).abs();
            let h = (a.y - b.y).abs();
            let rect = canvas::Path::rectangle(Point::new(x0, y0), Size::new(w, h));
            frame.fill(&rect, fill);
            frame.stroke(
                &rect,
                canvas::Stroke {
                    width: 1.0,
                    style: canvas::Style::Solid(stroke),
                    ..Default::default()
                },
            );
        }

        if let (Some(a), Some(b)) = (self.selection.box_anchor, self.selection.box_current) {
            draw_marquee(&mut frame, a, b, self.selection.box_crossing, theme);
        }
        // Preview marquee for point-picked windows (STRETCH) — same look, no pick.
        if let Some((a, b, crossing)) = self.selection.preview_box {
            draw_marquee(&mut frame, a, b, crossing, theme);
        }

        if self.selection.poly_active && self.selection.poly_points.len() > 1 {
            let base = if self.selection.poly_crossing {
                theme.extended_palette().success.base.color
            } else {
                theme.extended_palette().primary.base.color
            };
            let fill = base.scale_alpha(0.12);
            let stroke = base.scale_alpha(0.9);
            if let Some(cur) = self.selection.last_move_pos {
                let start = self.selection.poly_points[0];
                let fill_path = canvas::Path::new(|p| {
                    p.move_to(start);
                    for pt in &self.selection.poly_points[1..] {
                        p.line_to(*pt);
                    }
                    p.line_to(cur);
                    p.line_to(start);
                });
                frame.fill(&fill_path, fill);
            }
            let path = canvas::Path::new(|p| {
                p.move_to(self.selection.poly_points[0]);
                for pt in &self.selection.poly_points[1..] {
                    p.line_to(*pt);
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke {
                    width: 1.0,
                    style: canvas::Style::Solid(stroke),
                    ..Default::default()
                },
            );
            if let Some(cur) = self.selection.last_move_pos {
                let start = self.selection.poly_points[0];
                let last = *self.selection.poly_points.last().unwrap();
                let preview = canvas::Path::new(|p| {
                    p.move_to(last);
                    p.line_to(cur);
                    p.line_to(start);
                });
                frame.stroke(
                    &preview,
                    canvas::Stroke {
                        width: 1.0,
                        style: canvas::Style::Solid(stroke),
                        ..Default::default()
                    },
                );
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
                for grip in &self.grips {
                    draw_grip_marker(frame, grip, theme);
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
        if !over_viewcube && !over_divider && !self.pan_mode && !self.suppressed {
            if let Some(cp) = self.selection.last_move_pos {
                let color = theme
                    .extended_palette()
                    .background
                    .base
                    .text
                    .scale_alpha(0.90);
                let stroke = canvas::Stroke {
                    width: 1.0,
                    style: canvas::Style::Solid(color),
                    ..Default::default()
                };
                let sq = CROSSHAIR_SQ; // square half-size → 15×15
                let arm = CROSSHAIR_ARM; // crosshair arm length from center

                // Horizontal arms (start at square edge, end at arm length)
                let h_left = canvas::Path::new(|b| {
                    b.move_to(Point::new(cp.x - sq, cp.y));
                    b.line_to(Point::new(cp.x - arm, cp.y));
                });
                let h_right = canvas::Path::new(|b| {
                    b.move_to(Point::new(cp.x + sq, cp.y));
                    b.line_to(Point::new(cp.x + arm, cp.y));
                });
                // Vertical arms
                let v_top = canvas::Path::new(|b| {
                    b.move_to(Point::new(cp.x, cp.y - sq));
                    b.line_to(Point::new(cp.x, cp.y - arm));
                });
                let v_bot = canvas::Path::new(|b| {
                    b.move_to(Point::new(cp.x, cp.y + sq));
                    b.line_to(Point::new(cp.x, cp.y + arm));
                });
                // Center square
                let square = canvas::Path::rectangle(
                    Point::new(cp.x - sq, cp.y - sq),
                    Size::new(sq * 2.0, sq * 2.0),
                );

                frame.stroke(&h_left, stroke.clone());
                frame.stroke(&h_right, stroke.clone());
                frame.stroke(&v_top, stroke.clone());
                frame.stroke(&v_bot, stroke.clone());
                frame.stroke(&square, stroke);

                // Locked-layer badge: a small padlock beside the crosshair when
                // the hovered object sits on a locked layer (issue: locked
                // objects are visible + snappable but not selectable/editable).
                if self.hover_locked {
                    let warning = theme.extended_palette().warning.base;
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
            );
        }

        // ── Object Snap Tracking ─────────────────────────────────────────────
        let track_color = theme.extended_palette().primary.base.color.scale_alpha(0.7);
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

fn draw_grid(
    frame: &mut canvas::Frame,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    step: f32,
    grid_origin: glam::DVec3,
    grid_axes: (Vec3, Vec3, Vec3),
    limits: Option<(glam::DVec2, glam::DVec2)>,
) {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
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
        return;
    };

    // Step follows camera zoom only. The previous visible-sample calculation
    // changed depth while orbiting and made the grid jump 1 → 5 → 25.
    if !step.is_finite() || step <= 0.0 {
        return;
    }
    let s = step;

    let gc = Color {
        r: 0.28,
        g: 0.28,
        b: 0.28,
        a: 0.7,
    };
    let st = canvas::Stroke {
        width: 0.5,
        style: canvas::Style::Solid(gc),
        ..Default::default()
    };

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
    let draw_segments = |frame: &mut canvas::Frame, segments: &[(Point, Point)]| {
        if segments.is_empty() {
            return;
        }
        let path = canvas::Path::new(|builder| {
            for (p0, p1) in segments {
                builder.move_to(*p0);
                builder.line_to(*p1);
            }
        });
        frame.stroke(&path, st.clone());
    };

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
        draw_segments(frame, &segments);
        return;
    }

    let mut axis_extent = 0.0_f32;

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
                draw_segments(frame, &segments);
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
                draw_segments(frame, &segments);
                axis_extent =
                    axis_extent.max(min1.abs().max(max1.abs()).max(min2.abs()).max(max2.abs()));
            }
        }
    }

    // Coloured axes drawn on top of the grid lines, along the same UCS basis.
    if axis_extent > 0.0 {
        let extent = (axis_extent + s) * 1.5;
        draw_axes(frame, view_rot, eye, bounds, extent.max(10.0), grid_origin, (gx, gy, gz));
    }
}

// ── Coloured UCS axes ──────────────────────────────────────────────────────

fn draw_axes(
    frame: &mut canvas::Frame,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    extent: f32,
    origin: glam::DVec3,
    axes: (Vec3, Vec3, Vec3),
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
    line(ax, 0.90, 0.20, 0.20); // X — red
    line(ay, 0.20, 0.85, 0.20); // Y — green
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

    // Project to NDC (including depth) then to screen pixels.
    let w2ndc = |world: Vec3| -> Option<Vec3> {
        let ndc = vp.project_point3(world);
        if !ndc.x.is_finite() || !ndc.y.is_finite() || !ndc.z.is_finite() {
            return None;
        }
        Some(ndc)
    };
    let ndc2s = |ndc: Vec3| -> Point {
        Point::new(
            bounds.x + (ndc.x + 1.0) * 0.5 * bounds.width,
            bounds.y + (1.0 - ndc.y) * 0.5 * bounds.height,
        )
    };

    // Project the UCS axis directions (not fixed world axes) so the tripod
    // rotates to the active UCS. Directions are translation-invariant, so a
    // common origin is fine.
    let (ax, ay, az) = axes;
    let org = w2ndc(Vec3::ZERO)?;
    let xn = w2ndc(ax)?;
    let yn = w2ndc(ay)?;
    let zn = w2ndc(az)?;

    let org_s = ndc2s(org);
    let corner = Point::new(
        bounds.x + UCS_ICON_MARGIN,
        bounds.y + (bounds.height - UCS_ICON_MARGIN).max(UCS_ICON_MARGIN),
    );
    // Snap the tripod to the projected UCS origin when it is on-screen
    // (UCSICON ORigin); otherwise keep it in the corner. `org_s` (the
    // rotation-only origin) stays the reference for axis-tip directions —
    // those are translation-invariant, so only the anchor point moves.
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

    // Raw screen-space displacement for each axis tip.
    let raw = |ndc_tip: Vec3| -> (f32, f32, f32) {
        let s = ndc2s(ndc_tip);
        let dx = s.x - org_s.x;
        let dy = s.y - org_s.y;
        (dx, dy, (dx * dx + dy * dy).sqrt())
    };

    let (xdx, xdy, xlen) = raw(xn);
    let (ydx, ydy, ylen) = raw(yn);
    let (zdx, zdy, zlen) = raw(zn);

    // Scale so the longest projected axis fills UCS_ICON_LEN; shorter axes
    // stay proportionally shorter (this IS the foreshortening effect).
    let max_len = xlen.max(ylen).max(zlen).max(1e-4);
    let sc = UCS_ICON_LEN / max_len;

    let mk = |dx: f32, dy: f32, len: f32, tip_z: f32| IconAxis {
        dx: dx * sc,
        dy: dy * sc,
        sc_len: len * sc,
        // depth > 0 → tip farther than origin (into screen); < 0 → toward viewer.
        depth: tip_z - org.z,
    };
    Some((
        icon_origin,
        at_origin,
        [
            mk(xdx, xdy, xlen, xn.z),
            mk(ydx, ydy, ylen, yn.z),
            mk(zdx, zdy, zlen, zn.z),
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
            g: 0.85,
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
    frame.fill(
        &circle,
        Color {
            r: 0.9,
            g: 0.9,
            b: 0.9,
            a: 0.95,
        },
    );

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
    guide: DynGuide,
    boxes: Vec<DynBox>,
    prompt: String,
) -> Element<'a, Message> {
    canvas(DynInputCanvas {
        cursor_screen,
        base_screen,
        ref_screen,
        guide,
        boxes,
        prompt,
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
    guide: DynGuide,
    boxes: Vec<DynBox>,
    /// The active command's current prompt, drawn just above the boxes.
    prompt: String,
}

impl DynInputCanvas {
    fn dotted(theme: &Theme) -> canvas::Stroke<'static> {
        canvas::Stroke {
            width: 1.0,
            style: canvas::Style::Solid(
                theme.extended_palette().background.neutral.color.scale_alpha(0.9)
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

    /// Draw a value box centred at `center`, clamped inside `bounds`.
    fn draw_box(
        frame: &mut canvas::Frame,
        b: &DynBox,
        center: Point,
        bounds: iced::Rectangle,
        theme: &Theme,
    ) {
        let content = Self::box_content(b);
        let w = (content.len() as f32 * DYN_CHAR_W) + DYN_PAD * 2.0;
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
        let palette = theme.extended_palette();
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
        let palette = theme.extended_palette();
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
                DynRole::Angle => {
                    let a_mid = a_ref + sweep * 0.5;
                    // Pull the box back along the ray and lift it to the side
                    // opposite the distance box. A near-zero sweep collapses the
                    // mid-angle direction onto the cursor ray, so placing the box
                    // at full `len` would plant it on the cursor / snap point and
                    // hide it. (#124)
                    let r = (len - DYN_BOX_H * 2.0).max(len * 0.5);
                    Point {
                        x: base.x + a_mid.cos() * r - nx * 18.0,
                        y: base.y + a_mid.sin() * r - ny * 18.0,
                    }
                }
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
