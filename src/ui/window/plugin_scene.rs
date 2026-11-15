//! Renderer-free projection of plugin panel scenes into host paint
//! primitives.
//!
//! Ownership split (ADR-003): the plugin owns the *semantic* scene — the
//! `ocs_plugin_api::scene::PanelScene` tree, what it means and holds; the
//! host owns the *projection* — flattening that tree into ordered draw
//! primitives, and (in later tasks) caches, per-layer retention, and
//! hit-testing on top of the same flatten. Plugin and host share nothing
//! but the serializable tree.
//!
//! Two stages, one file:
//! 1. [`project_scene`] walks a validated [`PanelScene`] and materializes
//!    [`ScenePrim`]s in paint order — deterministic, no per-pixel work, no
//!    renderer required, so the whole stage is unit-testable.
//! 2. [`emit_prims`] paints the prim list onto a `canvas::Frame` with the
//!    exact overlay drawing idioms (`canvas::Path::new` builders,
//!    `frame.fill`/`frame.stroke`, `frame.fill_text` — see
//!    `src/ui/overlay.rs`).
//!
//! Identity survives projection: every prim carries the [`StableId`] of the
//! element it came from, so future hit-testing (SCN-020) and per-layer
//! retention (SCN-011) can map prims back to plugin elements without
//! re-walking the tree.
//!
//! Stage 1 is deliberately interaction-free: coordinates map 1:1 from scene
//! units to logical pixels (the scene's top-left pinned to the frame origin
//! at scale 1.0); pan/zoom is a later interaction-phase transform (SCN-011)
//! layered over the same projection. No caches are engaged yet.

// Stage-1 surface: `project_scene`/`emit_prims` are exercised by the unit
// tests in this module only until the panel-canvas wiring consumes them; the
// allow mirrors `ocs_plugin_api`'s scene-transport helper (wired by a later
// task the same way).
#![allow(dead_code)]

use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Size};
use ocs_plugin_api::scene::{
    AppliedPatchReport, CubicSegment, MarkerKind, PanelScene, Rect2D, SceneColor, SceneElement,
    SceneError, SceneRevision, StableId, Stroke as ApiStroke, ThemeColor, Vec2,
};

/// Mirror of `ocs_plugin_api::scene`'s crate-private `MAX_SCENE_NODES`
/// (10_000): a validated scene never carries more element nodes than this,
/// so the projection budgets its output against the same number. Parity is
/// pinned by tests (`projection_is_bounded` proves the API cap is exactly
/// this value by falling one node over it).
pub(crate) const MAX_SCENE_NODES: usize = 10_000;

/// Hard bound on the prim list one projection may produce, in terms of the
/// node budget: every element node paints at most one prim, and a
/// clip-carrying group paints at most two extra (un-shared) boundary prims —
/// its [`ScenePrim::ClipPush`] and [`ScenePrim::ClipPop`] — instead of
/// geometry, so the total can never exceed twice the node count. Enforced
/// by construction and re-asserted (in debug builds) at the end of
/// [`project_scene`].
pub(crate) const MAX_SCENE_PRIMS: usize = 2 * MAX_SCENE_NODES;

/// Cubic flattening: scene units per straight sub-step a bezier segment
/// targets before the cap kicks in. The full rule (pinned by tests):
/// `N = clamp(ceil(segment chord length / 8.0), 4, 64)`.
const CUBIC_CHORD_UNIT: f64 = 8.0;
/// Smallest number of straight sub-steps one [`CubicSegment`] flattens
/// into (`N = clamp(ceil(segment chord length / 8.0), 4, 64)`, lower end).
const CUBIC_MIN_STEPS: usize = 4;
/// Largest number of straight sub-steps one [`CubicSegment`] flattens
/// into (`N = clamp(ceil(segment chord length / 8.0), 4, 64)`, upper end).
const CUBIC_MAX_STEPS: usize = 64;

/// Alpha factor applied to the `Muted` theme text slot (`Muted` resolves as
/// the `Normal` text colour with alpha scaled x0.55).
const MUTED_ALPHA_SCALE: f32 = 0.55;

/// Radius of the [`SceneElement::Port`] disc (and of a [`MarkerKind::Dot`]):
/// small round decoration, approximated as a deterministic polygon.
const PORT_RADIUS: f32 = 4.0;
/// Straight-edge segments of the deterministic circle approximation for
/// ports and dot markers (16 segments: reviewable and pixel-comparable).
const CIRCLE_SEGMENTS: usize = 16;

/// Length of the arrow marker's tip ahead of its anchor, in scene units.
const MARKER_ARROW_TIP: f32 = 8.0;
/// Setback of the arrow marker's flat base behind its anchor.
const MARKER_ARROW_BASE: f32 = 4.0;
/// Half-width of the arrow marker's flat base.
const MARKER_ARROW_HALF_WIDTH: f32 = 5.0;
/// Half-diagonal of the diamond marker (rotated square), in scene units.
const MARKER_DIAMOND_HALF_DIAGONAL: f32 = 8.0;

/// Stroke width of the placeholder asset-frame outline, in scene units
/// (stage 1 maps them 1:1), so the frame silhouette is exactly one px.
const ASSET_FRAME_STROKE_WIDTH: f32 = 1.0;

/// One draw primitive a projected scene is made of, in paint order.
///
/// Each variant carries the [`StableId`] of the scene element it came from
/// (identity survives projection for hit-testing/retention). Vertices are
/// plain logical-pixel points at stage 1's 1:1 mapping; ring-shaped vertex
/// lists are closed by REPEATING their first vertex where a stroke must
/// complete a loop (the emitter adds no implicit closures).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ScenePrim {
    /// Fill the vertex polygon with one colour (fills close implicitly in
    /// the rasterizer, so open rings are fine).
    Fill {
        /// Source element.
        element: StableId,
        /// Ring vertices in paint order.
        vertices: Vec<Point>,
        /// Resolved fill colour.
        color: Color,
    },
    /// Stroke the vertex polyline with one colour and width.
    Stroke {
        /// Source element.
        element: StableId,
        /// Polyline vertices in paint order.
        vertices: Vec<Point>,
        /// Resolved stroke colour.
        color: Color,
        /// Stroke width, logical pixels (stage 1: scene units 1:1).
        width: f32,
    },
    /// A single path painted twice — fill first, then stroke — used by
    /// Rect/Path/CubicPath elements carrying both a fill and a stroke, so
    /// the two paints stay adjacent in the prim stream instead of splitting
    /// one path across the list.
    FillStroke {
        /// Source element.
        element: StableId,
        /// Ring vertices in paint order.
        vertices: Vec<Point>,
        /// Resolved fill colour.
        color: Color,
        /// Resolved stroke colour.
        stroke_color: Color,
        /// Stroke width, logical pixels (stage 1: scene units 1:1).
        stroke_width: f32,
    },
    /// One text run at a scene position (content is already bounded to
    /// `MAX_TEXT_BYTES` by the scene validator).
    Text {
        /// Source element.
        element: StableId,
        /// Run content.
        content: String,
        /// Baseline anchor of the run.
        position: Point,
        /// Font size, logical pixels (stage 1: scene units 1:1).
        size: f32,
        /// Resolved text colour.
        color: Color,
    },
    /// Enter a group's clip rectangle: everything up to the matching
    /// [`ScenePrim::ClipPop`] paints only inside `rect`.
    ClipPush {
        /// Source group element.
        element: StableId,
        /// Clip rectangle in logical pixels (stage 1: scene units 1:1).
        rect: Rectangle,
    },
    /// Leave the clip rectangle opened by the matching
    /// [`ScenePrim::ClipPush`] (same [`StableId`]).
    ClipPop {
        /// Source group element.
        element: StableId,
    },
}

/// Resolve a [`SceneColor`] against the host theme.
///
/// This table is part of the PINNED projection surface
/// (`palette_mapping_table_is_exhaustive_and_pinned` proves every binding
/// against a distinctive-value palette; iced palette drift breaks the test
/// instead of silently re-skinning plugin panels). The vendored iced
/// `theme::palette` layout binds the slots:
///
/// - `Palette.background: Background` - `Background.base: Pair`, where
///   `Pair.color` is the surface colour and `Pair.text` the readable
///   foreground.
/// - `Palette.primary | secondary | success | warning | danger: Swatch` -
///   each a `Swatch { base, weak, strong: Pair }`.
///
/// Exhaustive, pinned mapping:
///
/// | [`ThemeColor`] variant | iced slot bound |
/// |------------------------|-----------------|
/// | `Normal`               | `palette.background.base.text` (regular full-contrast fill) |
/// | `Accent`               | `palette.primary.base.color` (panel/brand accent) |
/// | `Muted`                | `palette.background.base.text` with alpha scaled x0.55 (`Color::scale_alpha(MUTED_ALPHA_SCALE)`) |
/// | `Strong`               | `palette.primary.strong.color` (emphasised fill) |
/// | `Success`              | `palette.success.base.text` |
/// | `Warning`              | `palette.warning.base.text` |
/// | `Danger`               | `palette.danger.base.text` |
/// | [`SceneColor::Rgba`]   | exact bytes: `Color::from_rgba8(r, g, b, a / 255.0)` |
///
/// The `match` is exhaustive over `SceneColor` by construction (no catch-all
/// arm), so a new theme token fails to compile until it is pinned here.
pub(crate) fn scene_color(palette: &iced::theme::Palette, color: SceneColor) -> Color {
    match color {
        SceneColor::Theme(ThemeColor::Normal) => palette.background.base.text,
        SceneColor::Theme(ThemeColor::Accent) => palette.primary.base.color,
        SceneColor::Theme(ThemeColor::Muted) => {
            palette.background.base.text.scale_alpha(MUTED_ALPHA_SCALE)
        }
        SceneColor::Theme(ThemeColor::Strong) => palette.primary.strong.color,
        SceneColor::Theme(ThemeColor::Success) => palette.success.base.text,
        SceneColor::Theme(ThemeColor::Warning) => palette.warning.base.text,
        SceneColor::Theme(ThemeColor::Danger) => palette.danger.base.text,
        SceneColor::Rgba(rgba) => Color::from_rgba8(
            rgba.0[0],
            rgba.0[1],
            rgba.0[2],
            f32::from(rgba.0[3]) / 255.0,
        ),
    }
}

/// Project a plugin scene into ordered paint primitives.
///
/// Paint order is document order, depth-first: children paint between their
/// parent group's [`ScenePrim::ClipPush`] and [`ScenePrim::ClipPop`] (which
/// exist only when the group carries a clip rect - a clip-less group is a
/// transparent container). The projection re-runs the scene validator
/// defensively before drawing anything, so budget or structural failures
/// surface here as [`SceneError`]; they never degrade silently.
///
/// `bounds` is the paint surface the prim list is destined for. Stage 1
/// maps scene units 1:1 onto logical pixels with the scene's top-left
/// pinned to the frame origin (scale 1.0), so `bounds` itself drives
/// nothing yet; the interaction-phase pan/zoom transform (SCN-011) consumes
/// it from the same signature.
///
/// Fixed budget: the returned list never exceeds [`MAX_SCENE_PRIMS`] prims
/// (see the constant). Nothing here touches pixels, caches, or a renderer:
/// the caller paints the list with [`emit_prims`].
pub(crate) fn project_scene(
    scene: &PanelScene,
    palette: &iced::theme::Palette,
    bounds: Size,
) -> Result<Vec<ScenePrim>, SceneError> {
    // Defensive revalidation: budget and structural failures must surface
    // here, never silently.
    scene.validate()?;

    // `bounds` only anchors the (later) interaction-phase transform; the
    // assert keeps the surface assumption honest until SCN-011 consumes it.
    debug_assert!(
        bounds.width.is_finite() && bounds.height.is_finite(),
        "projection surface must be finite"
    );

    let mut prims = Vec::new();
    for element in &scene.elements {
        project_element(element, palette, &mut prims);
    }
    debug_assert!(
        prims.len() <= MAX_SCENE_PRIMS,
        "scene projection exceeded its prim budget ({} > {MAX_SCENE_PRIMS})",
        prims.len()
    );
    Ok(prims)
}

/// Depth-first, document-order walk of one element; appends prims as it
/// goes. Recursion depth is bounded by the scene validator's
/// `MAX_SCENE_DEPTH` (64), which [`project_scene`] propagated before the
/// call.
fn project_element(
    element: &SceneElement,
    palette: &iced::theme::Palette,
    prims: &mut Vec<ScenePrim>,
) {
    match element {
        SceneElement::Group {
            id, children, clip, ..
        } => {
            // A group does not establish a transform and emits no geometry
            // of its own: it either brackets its children with clip prims
            // (clipped group) or is a plain container (no clip prims).
            if let Some(clip) = clip {
                prims.push(ScenePrim::ClipPush {
                    element: *id,
                    rect: rect2d_to_rectangle(clip),
                });
            }
            for child in children {
                project_element(child, palette, prims);
            }
            if clip.is_some() {
                prims.push(ScenePrim::ClipPop { element: *id });
            }
        }
        SceneElement::Rect {
            id,
            pos,
            size,
            fill,
            stroke,
            ..
        } => {
            // Rectangles are closed shapes: the first corner repeats so a
            // stroke rings all four sides (fills close anyway).
            let vertices = rectangle_vertices(pos, size);
            push_shape(*id, vertices, *fill, *stroke, palette, prims);
        }
        SceneElement::Path {
            id,
            points,
            closed,
            stroke,
            fill,
            ..
        } => {
            // Polygon through the points; `closed=true` rings the polygon
            // back to the first vertex (recorded in the prim data so fill
            // and stroke both read the closure).
            let mut vertices: Vec<Point> = points.iter().map(scene_point).collect();
            if *closed {
                if let Some(first) = vertices.first().copied() {
                    vertices.push(first);
                }
            }
            push_shape(*id, vertices, *fill, *stroke, palette, prims);
        }
        SceneElement::CubicPath {
            id,
            segments,
            closed,
            stroke,
            fill,
            ..
        } => {
            // Host-side flattening (see `flatten_cubic_chain`): the plugin
            // states the curve, the host samples it deterministically.
            let vertices = flatten_cubic_chain(segments, *closed);
            push_shape(*id, vertices, *fill, *stroke, palette, prims);
        }
        SceneElement::Text {
            id,
            content,
            pos,
            size,
            color,
            ..
        } => {
            prims.push(ScenePrim::Text {
                element: *id,
                content: content.clone(),
                position: scene_point(pos),
                size: *size,
                color: scene_color(palette, *color),
            });
        }
        SceneElement::Asset { id, frame, .. } => {
            // Minimal stage-1 asset treatment: a rectangle outline so the
            // frame is reviewable (and skippable) in the prim stream.
            // Asset rasterization - URI loading, thumbnails, per-panel
            // caches - is host-side cache work for a later task; the
            // placeholder stroke carries the element id. Colour pinned to
            // the Accent slot (documentation: reviewable placeholder).
            prims.push(ScenePrim::Stroke {
                element: *id,
                vertices: rectangle_vertices(&frame.pos, &frame.size),
                color: palette.primary.base.color,
                width: ASSET_FRAME_STROKE_WIDTH,
            });
        }
        SceneElement::Port { id, at, .. } => {
            // Ports are connection points the scene gives no colour for;
            // the projection pins them to the Accent theme slot (same
            // visual role as grips elsewhere in the host). Disc approximated
            // as a deterministic 16-segment polygon (a fill closes
            // implicitly). The optional `label` is host presentation for a
            // later pass; the minimal projection draws the disc only.
            prims.push(ScenePrim::Fill {
                element: *id,
                vertices: circle_vertices(scene_point(at), PORT_RADIUS, CIRCLE_SEGMENTS),
                color: palette.primary.base.color,
            });
        }
        SceneElement::Marker {
            id,
            at,
            kind,
            color,
            ..
        } => {
            prims.push(ScenePrim::Fill {
                element: *id,
                vertices: marker_vertices(scene_point(at), *kind),
                color: scene_color(palette, *color),
            });
        }
        SceneElement::Edge {
            id, route, stroke, ..
        } => {
            // The route paints as a plain polyline through the route points
            // (exactly those; no closing ring, even when a plugin loops a
            // route back onto its first point). Endpoint decoration is NOT
            // emitted in this task: `start_marker`/`end_marker` are consumed
            // by the later presentation pass, so an edge prints one stroke
            // prim at most.
            if let Some(stroke) = stroke {
                prims.push(ScenePrim::Stroke {
                    element: *id,
                    vertices: route.iter().map(scene_point).collect(),
                    color: scene_color(palette, stroke.color),
                    width: stroke.width,
                });
            }
            // A stroke-less edge declares nothing to paint; the uniform
            // visibility rule (colour-bearing elements only) applies.
        }
    }
}

/// Materialize the prims for one shape element from its vertices, fill, and
/// stroke. The uniform visibility rule: an element paints only what its
/// colour fields ask for - fill alone becomes [`ScenePrim::Fill`], stroke
/// alone [`ScenePrim::Stroke`], both a single [`ScenePrim::FillStroke`]
/// (fill then stroke, one path), neither nothing at all.
fn push_shape(
    element: StableId,
    vertices: Vec<Point>,
    fill: Option<SceneColor>,
    stroke: Option<ApiStroke>,
    palette: &iced::theme::Palette,
    prims: &mut Vec<ScenePrim>,
) {
    match (fill, stroke) {
        (Some(fill), Some(stroke)) => prims.push(ScenePrim::FillStroke {
            element,
            vertices,
            color: scene_color(palette, fill),
            stroke_color: scene_color(palette, stroke.color),
            stroke_width: stroke.width,
        }),
        (Some(fill), None) => prims.push(ScenePrim::Fill {
            element,
            vertices,
            color: scene_color(palette, fill),
        }),
        (None, Some(stroke)) => prims.push(ScenePrim::Stroke {
            element,
            vertices,
            color: scene_color(palette, stroke.color),
            width: stroke.width,
        }),
        (None, None) => {}
    }
}

/// Flatten a bezier chain into consecutive polyline vertices.
///
/// Subdivision rule (documented verbatim; pinned by tests): each
/// [`CubicSegment`] subdivides with `N` straight-line steps where
/// `N = clamp(ceil(segment chord length / 8.0), 4, 64)` - the chord being
/// the straight-line distance `start` -> `end` of that segment.
/// Deterministic: fixed step count per fixed input, identical inputs always
/// flatten to identical vertices.
///
/// The chain starts at the first segment's `start` and ends at the last
/// segment's `end`. A segment that does not continue its predecessor still
/// joins the chain: its `start` anchor is pushed too, so the flattened
/// polyline covers the gap verbatim instead of hiding it. `closed=true`
/// re-anchors the ring at the FIRST segment's `start` after the last
/// segment's `end` (closed cubic loops close + append start).
fn flatten_cubic_chain(segments: &[CubicSegment], closed: bool) -> Vec<Point> {
    let mut vertices: Vec<Point> = Vec::new();
    for segment in segments {
        // N = clamp(ceil(segment chord length / 8.0), 4, 64)
        let chord = {
            let dx = segment.end.0[0] - segment.start.0[0];
            let dy = segment.end.0[1] - segment.start.0[1];
            // Coordinates are validator-finite; the f64 clamp below also
            // absorbs a saturated `sqrt`/`ceil` for extreme inputs.
            (dx * dx + dy * dy).sqrt()
        };
        let steps = (chord / CUBIC_CHORD_UNIT)
            .ceil()
            .clamp(CUBIC_MIN_STEPS as f64, CUBIC_MAX_STEPS as f64) as usize;
        let start = scene_point(&segment.start);
        let continues = matches!(vertices.last(), Some(last) if *last == start);
        if !continues {
            // The first segment opens the chain; a later segment that does
            // not continue its predecessor joins with its own anchor.
            vertices.push(start);
        }
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            vertices.push(scene_point(&cubic_point(segment, t)));
        }
    }
    if closed {
        if let Some(first) = segments.first() {
            vertices.push(scene_point(&first.start));
        }
    }
    vertices
}

/// Point on one cubic segment at parameter `t` (direct evaluation of the
/// control quad), in scene space.
fn cubic_point(segment: &CubicSegment, t: f64) -> Vec2 {
    let one = 1.0 - t;
    let a = one * one * one;
    let b = 3.0 * one * one * t;
    let c = 3.0 * one * t * t;
    let d = t * t * t;
    Vec2([
        a * segment.start.0[0]
            + b * segment.ctrl_a.0[0]
            + c * segment.ctrl_b.0[0]
            + d * segment.end.0[0],
        a * segment.start.0[1]
            + b * segment.ctrl_a.0[1]
            + c * segment.ctrl_b.0[1]
            + d * segment.end.0[1],
    ])
}

/// Deterministic polygon for a standalone marker shape anchored at `at`:
///
/// - [`MarkerKind::Dot`]: a [`PORT_RADIUS`]-radius disc, 16 segments (the
///   same circle approximation as the port disc).
/// - [`MarkerKind::Arrow`]: a solid triangle pointing +x - tip at
///   `(at.x + MARKER_ARROW_TIP, at.y)`, flat base corners at
///   `(at.x - MARKER_ARROW_BASE, at.y +/- MARKER_ARROW_HALF_WIDTH)` - an
///   8-unit deterministic shape; stage 1 has no route orientation to point
///   standalone markers against.
/// - [`MarkerKind::Diamond`]: a rotated square - corners at
///   `(at.x +/- MARKER_DIAMOND_HALF_DIAGONAL, at.y)` and
///   `(at.x, at.y +/- MARKER_DIAMOND_HALF_DIAGONAL)`.
fn marker_vertices(at: Point, kind: MarkerKind) -> Vec<Point> {
    match kind {
        MarkerKind::Dot => circle_vertices(at, PORT_RADIUS, CIRCLE_SEGMENTS),
        MarkerKind::Arrow => vec![
            Point::new(at.x + MARKER_ARROW_TIP, at.y),
            Point::new(at.x - MARKER_ARROW_BASE, at.y - MARKER_ARROW_HALF_WIDTH),
            Point::new(at.x - MARKER_ARROW_BASE, at.y + MARKER_ARROW_HALF_WIDTH),
        ],
        MarkerKind::Diamond => vec![
            Point::new(at.x + MARKER_DIAMOND_HALF_DIAGONAL, at.y),
            Point::new(at.x, at.y - MARKER_DIAMOND_HALF_DIAGONAL),
            Point::new(at.x - MARKER_DIAMOND_HALF_DIAGONAL, at.y),
            Point::new(at.x, at.y + MARKER_DIAMOND_HALF_DIAGONAL),
        ],
    }
}

/// Deterministic ring polygon around `center`: `segments` straight edges
/// starting at angle 0, counterclockwise.
fn circle_vertices(center: Point, radius: f32, segments: usize) -> Vec<Point> {
    let step = std::f32::consts::TAU / segments.max(1) as f32;
    (0..segments)
        .map(|segment| {
            let angle = segment as f32 * step;
            Point::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect()
}

/// The four corners of an axis-aligned rectangle as a closed ring (the
/// first corner repeats), clockwise over the top edge.
fn rectangle_vertices(pos: &Vec2, size: &Vec2) -> Vec<Point> {
    let (x, y) = (pos.0[0] as f32, pos.0[1] as f32);
    let (w, h) = (size.0[0] as f32, size.0[1] as f32);
    vec![
        Point::new(x, y),
        Point::new(x + w, y),
        Point::new(x + w, y + h),
        Point::new(x, y + h),
        Point::new(x, y),
    ]
}

/// Scene-space rectangle ([`Rect2D`]) -> iced clip rectangle (stage 1:
/// 1:1 logical pixels, no anchor math).
fn rect2d_to_rectangle(rect: &Rect2D) -> Rectangle {
    Rectangle {
        x: rect.pos.0[0] as f32,
        y: rect.pos.0[1] as f32,
        width: rect.size.0[0] as f32,
        height: rect.size.0[1] as f32,
    }
}

/// Scene-space pair -> iced point (stage 1: 1:1 logical pixels).
fn scene_point(point: &Vec2) -> Point {
    Point::new(point.0[0] as f32, point.0[1] as f32)
}

/// Paint a whole prim list in order.
///
/// Thin emitter over a `canvas::Frame`, with the exact overlay idioms
/// (`src/ui/overlay.rs`): `canvas::Path::new(|builder| ...)` ring builders,
/// `frame.fill(&path, color)`, `frame.stroke(&path, canvas::Stroke { width,
/// style: canvas::Style::Solid(color), .. })`, and
/// `frame.fill_text(canvas::Text { ... })`.
///
/// [`ScenePrim::ClipPush`]/[`ScenePrim::ClipPop`] pairs chaperone a real
/// clip: the vendored iced `Frame` (graphics/src/geometry/frame.rs at the
/// pinned revision) exposes NO `push_clip`/`pop_clip` pair - its clip
/// primitive is `Frame::with_clip(region, closure)`, which drafts a
/// sub-frame clipped to `region` and pastes it back. The emitter therefore
/// scans ahead from each push to its matching pop (prim streams this module
/// produces are balanced by construction) and emits the enclosed run inside
/// `with_clip`, handing iced the clip rect INTERSECTED with the enclosing
/// bounds, so nested clips stay correct: iced's draft/paste records the
/// inner-most `clip_bounds` per primitive (tiny-skia and wgpu both), so the
/// intersection itself is emitter work - done here. An enclosure whose clip
/// shows nothing (empty intersection with the active bounds) is skipped
/// whole; a lone [`ScenePrim::ClipPush`] with no matching pop (defensive:
/// unbalanced streams are not produced here) clips the surviving tail and
/// stops.
///
/// Stage-1 caveat for SCN-011: canvas text composites above all path layers
/// by iced design (`Frame::fill_text` docs), and text inside a `with_clip`
/// draft depends on how the backend records draft text - clip-sensitive
/// captions are a live-host / milestone-6 concern, not claimed here.
pub(crate) fn emit_prims(frame: &mut canvas::Frame, prims: &[ScenePrim]) {
    emit_prims_within(frame, prims, &Rectangle::with_size(frame.size()));
}

/// Recursion half of [`emit_prims`]: emit `prims` under the given active
/// clip bounds. `active` is read only for region intersection - iced applies
/// the regions themselves via `with_clip` drafts.
fn emit_prims_within(frame: &mut canvas::Frame, prims: &[ScenePrim], active: &Rectangle) {
    let mut index = 0;
    while index < prims.len() {
        match &prims[index] {
            ScenePrim::ClipPush { rect, .. } => {
                let end = matching_clip_pop(prims, index).unwrap_or(prims.len());
                if let Some(region) = active.intersection(rect) {
                    frame.with_clip(region, |inner| {
                        emit_prims_within(inner, &prims[index + 1..end], &region);
                    });
                }
                if end >= prims.len() {
                    // Unmatchable clip consumed the tail; nothing else to do.
                    return;
                }
                index = end + 1;
            }
            prim => {
                emit_prim(frame, prim);
                index += 1;
            }
        }
    }
}

/// Paint one non-clip prim with the overlay idioms. Clip prims are chaperoned
/// by [`emit_prims_within`]'s pairing and never land here.
fn emit_prim(frame: &mut canvas::Frame, prim: &ScenePrim) {
    match prim {
        ScenePrim::Fill {
            vertices, color, ..
        } => {
            frame.fill(&polygon_path(vertices), *color);
        }
        ScenePrim::Stroke {
            vertices,
            color,
            width,
            ..
        } => {
            frame.stroke(
                &polygon_path(vertices),
                canvas::Stroke {
                    width: *width,
                    style: canvas::Style::Solid(*color),
                    ..Default::default()
                },
            );
        }
        ScenePrim::FillStroke {
            vertices,
            color,
            stroke_color,
            stroke_width,
            ..
        } => {
            // One path, painted twice - fill first, then stroke.
            let path = polygon_path(vertices);
            frame.fill(&path, *color);
            frame.stroke(
                &path,
                canvas::Stroke {
                    width: *stroke_width,
                    style: canvas::Style::Solid(*stroke_color),
                    ..Default::default()
                },
            );
        }
        ScenePrim::Text {
            content,
            position,
            size,
            color,
            ..
        } => {
            frame.fill_text(canvas::Text {
                content: content.clone(),
                position: *position,
                color: *color,
                size: iced::Pixels(*size),
                ..Default::default()
            });
        }
        ScenePrim::ClipPush { .. } | ScenePrim::ClipPop { .. } => {}
    }
}

/// Ring/polyline path through consecutive vertices. No implicit closures:
/// the prim vertices already carry their closure state (see [`ScenePrim`]).
fn polygon_path(vertices: &[Point]) -> canvas::Path {
    canvas::Path::new(|builder| {
        if let Some(first) = vertices.first() {
            builder.move_to(*first);
            for vertex in &vertices[1..] {
                builder.line_to(*vertex);
            }
        }
    })
}

/// Index of the clip pop matching the push at `push_index`, or `None` when
/// the stream is unterminated (defensive; projection pairs pushes and pops).
fn matching_clip_pop(prims: &[ScenePrim], push_index: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, prim) in prims.iter().enumerate().skip(push_index + 1) {
        match prim {
            ScenePrim::ClipPush { .. } => depth += 1,
            ScenePrim::ClipPop { .. } => {
                if depth == 0 {
                    return Some(index);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Renderer-free sink for the projection: the direct analogue of
/// `overlay.rs`'s `count_constraint_glyph_pills` (the benchmark's
/// renderer-free sink). Counts prims without constructing any renderer, so
/// tests and benches can assert the stream's shape headlessly.
///
/// Casing is exactly `count_prims` (pinned by the task assignment).
pub(crate) fn count_prims(prims: &[ScenePrim]) -> usize {
    prims.len()
}

// ---------------------------------------------------------------------------
// Per-subtree prim store (SCN-011b): report-driven retained projection
// ---------------------------------------------------------------------------
//
// `ScenePrimStore` retains the projected prims of a panel scene SEGMENTED at
// top-level subtrees, and refreshes the segments a patch report names — so a
// patch's projection cost is O(touched subtrees), not O(scene). It is the
// per-subtree half of the milestone-2 acceptance ("changed-element counts
// tracked independently of total scene size"); `plugin_scene_cache`'s
// `CachedOverlayLayer` stays the pixel-path carrier (the store's counters are
// the honest cost proof, not the layer rebuilds).
//
// ── v1 segmentation rule (DECIDED, verbatim) ──
// SEGMENT AT ROOT-LEVEL ELEMENTS ONLY. A nested group's clip bracket is always
// complete within the root subtree that carries it (`project_element` pushes
// and pops the clip around the group's OWN children), so every segment is
// clip-balanced and the segments concatenate into exactly `project_scene`'s
// prim stream (pinned by test). Nested segmentation (per-child-id Arcs inside
// a root segment) is deferred on purpose: a nested segment cut inside its
// parent's clip span cannot be retained independently (the bracket must not
// split across segments), and the added segmentation buys nothing until root
// segments grow wide. The nested retention half of the report (retired NESTED
// ids, nested touched ids) is carried by the report for that future store.

// The palette fingerprint is SCN-011a's FNV-1a-64 digest (pinned in
// `plugin_scene_cache`); the store compares the SAME digest family as the
// layer key. Layering note: both modules sit in the same `ui` module layer
// (`scripts/arch_check.py` reports no upward dependency for same-layer use);
// the digest lives with the cache module as of SCN-011a.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::ui::window::plugin_scene_cache::palette_fingerprint;

/// One retained prim segment: the prims of ONE top-level subtree, keyed by
/// the root element's stable id, in paint order (see the v1 segmentation
/// rule in the store docs).
pub(crate) struct SubtreePrims {
    /// The segment's root element id — the segment's identity for the
    /// report-driven resolution.
    root_id: StableId,
    /// The subtree's prims in paint order, Arc-shared so retained segments
    /// survive a refresh untouched (the pointer-equality retention contract:
    /// an untouched segment keeps its exact Arc instance bit-for-bit).
    prims: Arc<Vec<ScenePrim>>,
}

/// Why a [`ScenePrimStore`] ran the whole-tree flatten (the counted
/// breakdown of [`StoreCounters::full_reprojects`]; every reason's counter
/// sums into the total).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FullReprojectReason {
    /// A direct [`ScenePrimStore::sync_full`] call (panel snapshot / first
    /// provisioning flows).
    Explicit,
    /// The first report-driven sync on a never-synced store: the segments
    /// must exist before the report can drive their retention.
    FirstSync,
    /// The palette fingerprint moved since the last sync. DOCUMENTED policy:
    /// a palette change is a whole-store event (colors are resolved inside
    /// the prims; no segment can be kept safely), mirroring the layer key's
    /// own palette invalidation.
    Palette,
    /// The report was truncated past either cap
    /// ([`MAX_REPORT_TOUCHED`](ocs_plugin_api::scene::MAX_REPORT_TOUCHED) /
    /// [`MAX_REPORT_RETIRED`](ocs_plugin_api::scene::MAX_REPORT_RETIRED)) —
    /// the documented SAFETY rule: on ANY truncation the incremental path
    /// falls back to sync_full semantics, so a truncated report can never
    /// leave a segment stale by silently missing touched ids.
    ReportTruncation,
    /// A touched/retired id resolved to no root segment — i.e. it names a
    /// NESTED element. The report carries no ancestor fact, and the v1
    /// root-only segmentation has no id→segment index: the conservative
    /// (never-wrong) answer is the whole-tree flatten. Rescuing nested ops
    /// (an id→root index maintained on re-projection) is the documented v2
    /// extension; the fixture graph scenes patch ROOT-level subtrees, so the
    /// O(touched) path is exercised by construction.
    UnmappedTouch,
    /// The report's revision is BEHIND the store's (an out-of-order or
    /// foreign-report delivery): defensively re-sync from the given scene.
    StaleReport,
}

/// Rebuild instrumentation for one [`ScenePrimStore`]: the milestone-2
/// acceptance read. `subtree_reprojects` grows with the PATCH (touched
/// segments), never with the scene; `full_reprojects` (with its per-reason
/// breakdown) is the defect surface — "a full flatten per patch is the
/// defect, not the norm", mirroring the cache module's counter policy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StoreCounters {
    /// Segments re-projected by report-driven incremental syncs (an in-place
    /// Arc swap or a newly appended root segment each count once).
    pub subtree_reprojects: u64,
    /// Segments whose Arc survived an incremental sync untouched (the
    /// retention win, per segment per sync — the milestone's headline
    /// number: patch cost that stays flat as scenes grow).
    pub subtree_retained_hits: u64,
    /// Whole-tree flatten runs (sync_full calls plus every fallback that
    /// runs the same flatten), broken down by the per-reason counters below.
    pub full_reprojects: u64,
    /// Full re-projections caused by [`FullReprojectReason::Explicit`].
    pub full_reprojects_explicit: u64,
    /// Full re-projections caused by [`FullReprojectReason::FirstSync`].
    pub full_reprojects_first_sync: u64,
    /// Full re-projections caused by [`FullReprojectReason::Palette`].
    pub full_reprojects_palette: u64,
    /// Full re-projections caused by [`FullReprojectReason::ReportTruncation`].
    pub full_reprojects_truncation: u64,
    /// Full re-projections caused by [`FullReprojectReason::UnmappedTouch`].
    pub full_reprojects_unmapped: u64,
    /// Full re-projections caused by [`FullReprojectReason::StaleReport`].
    pub full_reprojects_stale: u64,
    /// Segments tombstoned because a patch removed (or did not re-adopt)
    /// their root subtree — one count per dropped segment; the dropped Arc's
    /// prims are released immediately.
    pub retired_purges: u64,
}

/// Per-subtree retained prim store for one panel scene (SCN-011b).
///
/// Structure: `segments` holds one [`SubtreePrims`] per TOP-LEVEL subtree in
/// paint order. A segment the current scene no longer carries becomes a
/// `None` TOMBSTONE: its Arc's prims are released immediately and the hole
/// keeps the paint order of the survivors stable (root-level patches only
/// replace in place, append, or remove, so survivor positions never move —
/// tombstones are collected by the next `sync_full`). `root_index` maps the
/// live root ids to their segment positions for O(1) report resolution; its
/// key set IS the live key set by construction.
///
/// Sync protocol (the documented contract):
///
/// - `sync_full(scene, palette)` — the flatten baseline (panel snapshots,
///   first provisioning, and every internally-triggered fallback; validate
///   first, exactly like `project_scene`).
/// - `sync_incremental(scene, report, palette)` — call ONCE per applied
///   patch, with the post-patch scene and the report the single mutation
///   path returned. The guards (first match wins):
///   1. never-synced store → full (`FirstSync`);
///   2. palette fingerprint moved → full (`Palette`);
///   3. report revision EQUALS the stored one → idempotent re-delivery, an
///      `Ok` no-op (the first sync already consumed it);
///   4. report revision BEHIND the stored one → full (`StaleReport`);
///   5. ANY truncation in the report → full (`ReportTruncation` — the
///      documented safety rule);
///   6. touched/retired resolution (the v1 rule, per id, touched ids then
///      retired ids, in report order, deduped):
///      - id live as a segment root AND a current scene root → re-project
///        that segment in place (root-level Replace);
///      - id NOT a segment root but a current scene root → new segment
///        appended at the end (root-level Add — Add appends);
///      - id a segment root but NOT a current scene root → tombstone (its
///        root subtree left the scene; joint with the defensive retired
///        purge below);
///      - id in NEITHER set → full (`UnmappedTouch` — nested ops; see
///        [`FullReprojectReason::UnmappedTouch`]).
///      Retired ids drive ONLY the defensive purge (a retired root id not
///      covered by the touched resolution gets tombstoned; retired NESTED
///      ids have no store entries at root-level segmentation — nothing to
///      purge, and their prims already vanished with their segment's refresh).
///
/// Cost honesty: the resolution consults the CURRENT scene's root list once
/// (a flat top-level id scan / membership set — no prim work, no per-node
/// walk), then touches exactly the report's ids and their segments. One
/// `Vec<Option<..>>` header pass handles the tombstones. Neither scan
/// materializes prims for untouched subtrees — that is the retention win the
/// counters measure.
///
/// Failure contract: `sync_full` validates first (identical gate to
/// `project_scene`); any failure leaves the store EMPTY and never-synced
/// (the next `is_fresh` is false, so the layer build falls back to the
/// classic flatten) while the counters keep their history.
pub(crate) struct ScenePrimStore {
    /// The panel-scene identity this store serves.
    surface_id: StableId,
    /// Root-level segments in paint order (tombstoned on removal; see the
    /// struct docs).
    segments: Vec<Option<SubtreePrims>>,
    /// Live root id → index into `segments` (the O(1) resolution map; the
    /// key set IS the live key set). Never renumbered: tombstones keep their
    /// slots and are collected by the next full sync.
    root_index: HashMap<StableId, usize>,
    /// Live (non-tombstone) segment count — O(1) retention math.
    live_segments: usize,
    /// Revision of the tree the segments were last synced from; `None` while
    /// never-synced (fresh stores, failed syncs).
    revision: Option<SceneRevision>,
    /// [`palette_fingerprint`] of the palette the segments resolved against.
    /// `0` while never-synced (the FNV digest is never zero — pinned by the
    /// cache module's tests).
    palette_fingerprint: u64,
    /// The counters (the milestone-2 acceptance read).
    counters: StoreCounters,
}

impl ScenePrimStore {
    /// A fresh, empty (never-synced) store for `surface_id`.
    pub(crate) fn new(surface_id: StableId) -> Self {
        Self {
            surface_id,
            segments: Vec::new(),
            root_index: HashMap::new(),
            live_segments: 0,
            revision: None,
            palette_fingerprint: 0,
            counters: StoreCounters::default(),
        }
    }

    /// The panel-scene identity this store serves.
    pub(crate) fn surface_id(&self) -> StableId {
        self.surface_id
    }

    /// The counters (read side).
    pub(crate) fn counters(&self) -> &StoreCounters {
        &self.counters
    }

    /// `true` when the segments are synced at exactly `revision` under
    /// `fingerprint` — the build path may paint from the segment lists.
    pub(crate) fn is_fresh(&self, revision: SceneRevision, fingerprint: u64) -> bool {
        self.revision == Some(revision) && self.palette_fingerprint == fingerprint
    }

    /// The live segments as `(root_id, Arc)` pairs in paint order
    /// (tombstones skipped). Clones Arcs (refcounts), never prims — the
    /// pointer-equality retention tests read these.
    pub(crate) fn live_segments(&self) -> Vec<(StableId, Arc<Vec<ScenePrim>>)> {
        self.segments
            .iter()
            .flatten()
            .map(|segment| (segment.root_id, Arc::clone(&segment.prims)))
            .collect()
    }

    /// The live prim lists in paint order (the layer build path paints these
    /// segment-wise; concatenation equals `project_scene`'s output — pinned).
    pub(crate) fn live_arcs(&self) -> Vec<Arc<Vec<ScenePrim>>> {
        self.segments
            .iter()
            .flatten()
            .map(|segment| Arc::clone(&segment.prims))
            .collect()
    }

    /// Full re-projection (the flatten baseline). One prim-list segment per
    /// TOP-LEVEL subtree (the v1 segmentation rule). Validates first; a
    /// failure leaves the store empty and never-synced (counters keep their
    /// history). Counts as [`FullReprojectReason::Explicit`].
    pub(crate) fn sync_full(
        &mut self,
        scene: &PanelScene,
        palette: &iced::theme::Palette,
    ) -> Result<(), SceneError> {
        self.sync_full_reasoned(scene, palette, FullReprojectReason::Explicit)
    }

    /// `sync_full` with the reason bookkeeping — the internal flatten every
    /// `sync_incremental` fallback shares, so there is exactly one segment
    /// builder and one counter path.
    fn sync_full_reasoned(
        &mut self,
        scene: &PanelScene,
        palette: &iced::theme::Palette,
        reason: FullReprojectReason,
    ) -> Result<(), SceneError> {
        // Defensive revalidation: budget and structural failures must
        // surface here, never silently (identical gate to `project_scene`).
        scene.validate()?;
        let fingerprint = palette_fingerprint(palette);
        let mut segments = Vec::with_capacity(scene.elements.len());
        let mut total = 0usize;
        for element in &scene.elements {
            let mut prims = Vec::new();
            project_element(element, palette, &mut prims);
            debug_assert!(
                prims.len() <= MAX_SCENE_PRIMS,
                "subtree projection exceeded its prim budget ({} > {MAX_SCENE_PRIMS})",
                prims.len()
            );
            total += prims.len();
            segments.push(Some(SubtreePrims {
                root_id: element.stable_id(),
                prims: Arc::new(prims),
            }));
        }
        debug_assert!(total <= MAX_SCENE_PRIMS);
        self.segments = segments;
        self.root_index = self
            .segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                (
                    segment
                        .as_ref()
                        .expect("fresh segments have no holes")
                        .root_id,
                    index,
                )
            })
            .collect();
        self.live_segments = self.segments.len();
        self.revision = Some(scene.revision);
        self.palette_fingerprint = fingerprint;
        self.counters.full_reprojects += 1;
        match reason {
            FullReprojectReason::Explicit => self.counters.full_reprojects_explicit += 1,
            FullReprojectReason::FirstSync => self.counters.full_reprojects_first_sync += 1,
            FullReprojectReason::Palette => self.counters.full_reprojects_palette += 1,
            FullReprojectReason::ReportTruncation => self.counters.full_reprojects_truncation += 1,
            FullReprojectReason::UnmappedTouch => self.counters.full_reprojects_unmapped += 1,
            FullReprojectReason::StaleReport => self.counters.full_reprojects_stale += 1,
        }
        Ok(())
    }

    /// Report-driven incremental retention (the point of the task): refresh
    /// ONLY the segments the patch report names, keep every other segment's
    /// Arc bit-for-bit. See the struct docs for the guard/resolution rules
    /// and the cost honesty note. The scene must be the post-patch tree the
    /// report describes (checked in debug).
    pub(crate) fn sync_incremental(
        &mut self,
        scene: &PanelScene,
        report: &AppliedPatchReport,
        palette: &iced::theme::Palette,
    ) -> Result<(), SceneError> {
        debug_assert_eq!(
            scene.revision, report.revision,
            "sync_incremental requires the post-patch scene the report describes"
        );
        let fingerprint = palette_fingerprint(palette);

        // Guard 1: never-synced → full flatten (the segments must exist
        // before a report can drive their retention).
        let Some(stored) = self.revision else {
            return self.sync_full_reasoned(scene, palette, FullReprojectReason::FirstSync);
        };
        // Guard 2: palette change → full (whole-store event; colors are
        // resolved inside the prims).
        if self.palette_fingerprint != fingerprint {
            return self.sync_full_reasoned(scene, palette, FullReprojectReason::Palette);
        }
        // Guard 3: idempotent re-delivery of the already-applied report.
        if report.revision == stored {
            return Ok(());
        }
        // Guard 4: out-of-order / foreign report → defensive full.
        if report.revision < stored {
            return self.sync_full_reasoned(scene, palette, FullReprojectReason::StaleReport);
        }
        // Guard 5: SAFETY — on ANY truncation the report cannot name its
        // touched set individually, so the incremental return path falls
        // back to sync_full semantics (never wrong-by-truncation staleness).
        if report.touched_truncated > 0 || report.retired_truncated > 0 {
            return self.sync_full_reasoned(scene, palette, FullReprojectReason::ReportTruncation);
        }

        // Guard 6: touched/retired resolution (the v1 rule).
        // Membership sets: the CURRENT scene's roots (the post-patch truth
        // the resolution measures against) — one flat top-level scan.
        let desired_roots: HashSet<StableId> =
            scene.elements.iter().map(|e| e.stable_id()).collect();
        let mut reproject: Vec<StableId> = Vec::new();
        let mut appended: Vec<StableId> = Vec::new();
        let mut dropped: HashSet<StableId> = HashSet::new();
        for event in report
            .touched_subtree_ids
            .iter()
            .chain(report.retired_ids.iter())
            .copied()
        {
            if self.root_index.contains_key(&event) {
                if desired_roots.contains(&event) {
                    if !reproject.contains(&event) {
                        // Root-level Replace: re-project that segment in place.
                        reproject.push(event);
                    }
                } else if dropped.insert(event) {
                    // Root left the scene (root-level Remove; joint with the
                    // defensive retired purge — deduped).
                }
            } else if desired_roots.contains(&event) {
                if !appended.contains(&event) {
                    // New root subtree the patch appended (root-level Add).
                    appended.push(event);
                }
            } else {
                // Nested op id (or unknown): no ancestor fact in the report
                // and no id→segment index at root-level segmentation → the
                // conservative whole-tree flatten (documented v1 rule).
                return self.sync_full_reasoned(scene, palette, FullReprojectReason::UnmappedTouch);
            }
        }

        let live_before = self.live_segments;

        // Tombstones first (one header pass; the Arc's prims drop with the
        // segment).
        for segment in self.segments.iter_mut() {
            if let Some(held) = segment {
                if dropped.contains(&held.root_id) {
                    self.root_index.remove(&held.root_id);
                    *segment = None;
                    self.live_segments -= 1;
                    self.counters.retired_purges += 1;
                }
            }
        }

        // ONE walk over the scene roots: re-project the in-place segments
        // (position-stable — the patch replaced in place) and capture the
        // appended subtrees' prims (root-level Adds append, so the walk's
        // order matches the segment order they end up in).
        let wanted: HashSet<StableId> = reproject.iter().copied().collect();
        let append_wanted: HashSet<StableId> = appended.iter().copied().collect();
        let mut new_segments: Vec<(StableId, Vec<ScenePrim>)> = Vec::new();
        for root in &scene.elements {
            let id = root.stable_id();
            if wanted.contains(&id) {
                let index = self.root_index[&id];
                let mut prims = Vec::new();
                project_element(root, palette, &mut prims);
                debug_assert!(
                    prims.len() <= MAX_SCENE_PRIMS,
                    "subtree projection exceeded its prim budget ({} > {MAX_SCENE_PRIMS})",
                    prims.len()
                );
                let slot = &mut self.segments[index];
                match slot {
                    Some(held) => held.prims = Arc::new(prims),
                    None => unreachable!("re-projected segment is live by resolution"),
                }
                self.counters.subtree_reprojects += 1;
            } else if append_wanted.contains(&id) {
                let mut prims = Vec::new();
                project_element(root, palette, &mut prims);
                debug_assert!(
                    prims.len() <= MAX_SCENE_PRIMS,
                    "subtree projection exceeded its prim budget ({} > {MAX_SCENE_PRIMS})",
                    prims.len()
                );
                new_segments.push((id, prims));
            }
        }
        debug_assert_eq!(new_segments.len(), appended.len());
        for (id, prims) in new_segments {
            self.segments.push(Some(SubtreePrims {
                root_id: id,
                prims: Arc::new(prims),
            }));
            self.root_index.insert(id, self.segments.len() - 1);
            self.live_segments += 1;
            self.counters.subtree_reprojects += 1;
        }

        // Retention math: every live segment not dropped and not re-projected
        // kept its exact Arc.
        self.counters.subtree_retained_hits +=
            (live_before - dropped.len()) as u64 - reproject.len() as u64;

        self.revision = Some(report.revision);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::theme::palette::{Background, Pair, Swatch};
    use ocs_plugin_api::scene::{AssetKind, Rgba8, SceneRevision};

    const SURFACE: Size = Size {
        width: 400.0,
        height: 300.0,
    };

    // -- fixtures ------------------------------------------------------------

    /// A palette whose theme slots carry distinct `(r, 0, 0)` channel values,
    /// built from struct literals (bypassing `Pair::new`'s readability
    /// rewrite) so a test can prove exactly which slot every `ThemeColor`
    /// token reads: Normal=20, Accent=30, Strong=40, Success=60, Warning=70,
    /// Danger=80 (each in the `r` channel).
    fn pinned_palette() -> iced::theme::Palette {
        let null = null_pair();
        iced::theme::Palette {
            background: Background {
                // `base.text` (20) is the Normal/Muted slot; `color` empty.
                base: pair(Color::TRANSPARENT, tint(20, 0, 0)),
                weakest: null,
                weaker: null,
                weak: null,
                neutral: null,
                strong: null,
                stronger: null,
                strongest: null,
            },
            primary: Swatch {
                // `base.color` (30) is the Accent slot; `strong.color` (40)
                // is the Strong slot.
                base: pair(tint(30, 0, 0), Color::TRANSPARENT),
                weak: null,
                strong: pair(tint(40, 0, 0), Color::TRANSPARENT),
            },
            secondary: Swatch {
                base: null,
                weak: null,
                strong: null,
            },
            success: Swatch {
                base: pair(Color::TRANSPARENT, tint(60, 0, 0)),
                weak: null,
                strong: null,
            },
            warning: Swatch {
                base: pair(Color::TRANSPARENT, tint(70, 0, 0)),
                weak: null,
                strong: null,
            },
            danger: Swatch {
                base: pair(Color::TRANSPARENT, tint(80, 0, 0)),
                weak: null,
                strong: null,
            },
            is_dark: false,
        }
    }

    fn null_pair() -> Pair {
        Pair {
            color: Color::TRANSPARENT,
            text: Color::TRANSPARENT,
        }
    }

    fn pair(color: Color, text: Color) -> Pair {
        Pair { color, text }
    }

    /// Distinctive opaque `(r, g, b)` colour.
    fn tint(r: u8, g: u8, b: u8) -> Color {
        Color::from_rgba8(r, g, b, 1.0)
    }

    fn v2(x: f64, y: f64) -> Vec2 {
        Vec2([x, y])
    }

    fn scene(elements: Vec<SceneElement>) -> PanelScene {
        PanelScene {
            revision: SceneRevision(1),
            elements,
        }
    }

    /// A fill-only rectangle anchored at `(x, y)`.
    fn filled_rect(id: u64, x: f64, y: f64) -> SceneElement {
        SceneElement::Rect {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            pos: v2(x, y),
            size: v2(10.0, 8.0),
            fill: Some(SceneColor::Theme(ThemeColor::Normal)),
            stroke: None,
        }
    }

    fn number_rect(id: u64) -> SceneElement {
        filled_rect(id, 0.0, 0.0)
    }

    fn group(id: u64, children: Vec<SceneElement>) -> SceneElement {
        SceneElement::Group {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            children,
            clip: None,
        }
    }

    fn clipped_group(id: u64, clip: Rect2D, children: Vec<SceneElement>) -> SceneElement {
        SceneElement::Group {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            children,
            clip: Some(clip),
        }
    }

    fn clip_at(x: f64, y: f64, w: f64, h: f64) -> Rect2D {
        Rect2D {
            pos: v2(x, y),
            size: v2(w, h),
        }
    }

    fn text_element(id: u64, content: &str) -> SceneElement {
        SceneElement::Text {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            content: content.to_string(),
            pos: v2(1.0, 2.0),
            size: 13.0,
            color: SceneColor::Theme(ThemeColor::Normal),
        }
    }

    fn path_element(id: u64, points: Vec<Vec2>, closed: bool) -> SceneElement {
        SceneElement::Path {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            points,
            closed,
            stroke: None,
            fill: Some(SceneColor::Theme(ThemeColor::Normal)),
        }
    }

    fn cubic_element(id: u64, segments: Vec<CubicSegment>, closed: bool) -> SceneElement {
        SceneElement::CubicPath {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            segments,
            closed,
            stroke: None,
            fill: Some(SceneColor::Theme(ThemeColor::Normal)),
        }
    }

    /// A K-shaped cubic quad curve from `from` to `to` (both control points
    /// on the straight join between the anchors, so the shape is easy to
    /// reason about).
    fn straight_segment(from: (f64, f64), to: (f64, f64)) -> CubicSegment {
        CubicSegment {
            start: v2(from.0, from.1),
            ctrl_a: v2((from.0 + to.0) / 2.0, from.1),
            ctrl_b: v2((from.0 + to.0) / 2.0, to.1),
            end: v2(to.0, to.1),
        }
    }

    fn port(id: u64) -> SceneElement {
        SceneElement::Port {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            at: v2(10.0, 20.0),
            label: None,
        }
    }

    fn marker(id: u64, kind: MarkerKind) -> SceneElement {
        SceneElement::Marker {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            at: v2(5.0, 5.0),
            kind,
            color: SceneColor::Theme(ThemeColor::Accent),
        }
    }

    fn asset(id: u64) -> SceneElement {
        SceneElement::Asset {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            kind: AssetKind::Svg,
            uri: "asset://fixture".to_string(),
            frame: clip_at(0.0, 0.0, 10.0, 10.0),
        }
    }

    fn project(scene: &PanelScene, palette: &iced::theme::Palette) -> Vec<ScenePrim> {
        project_scene(scene, palette, SURFACE).expect("fixture scenes project")
    }

    /// The source element id of a prim.
    fn prim_element(prim: &ScenePrim) -> StableId {
        match prim {
            ScenePrim::Fill { element, .. }
            | ScenePrim::Stroke { element, .. }
            | ScenePrim::FillStroke { element, .. }
            | ScenePrim::Text { element, .. }
            | ScenePrim::ClipPush { element, .. } => *element,
            ScenePrim::ClipPop { element } => *element,
        }
    }

    // -- tests ---------------------------------------------------------------

    #[test]
    fn projection_is_paint_order_depth_first() {
        // Scene: rect(1), group(10){ rect(11), group(12, clip){ text(13) } },
        // rect(2).
        let fixture = scene(vec![
            filled_rect(1, 0.0, 0.0),
            group(
                10,
                vec![
                    filled_rect(11, 0.0, 0.0),
                    clipped_group(
                        12,
                        clip_at(0.0, 0.0, 50.0, 50.0),
                        vec![text_element(13, "T")],
                    ),
                ],
            ),
            filled_rect(2, 0.0, 0.0),
        ]);
        let prims = project(&fixture, &pinned_palette());
        // Document order, depth-first: children paint strictly between their
        // parent group's clip boundaries; the clip-less group(10) paints
        // nothing of its own.
        let stream: Vec<StableId> = prims.iter().map(|prim| prim_element(prim)).collect();
        assert_eq!(
            stream,
            vec![
                StableId(1),
                StableId(11),
                StableId(12), // ClipPush
                StableId(13),
                StableId(12), // ClipPop
                StableId(2),
            ]
        );
        assert_eq!(prims.len(), 6);
    }

    #[test]
    fn clips_only_when_present() {
        let palette = pinned_palette();
        // A group WITHOUT a clip is a transparent container: no clip prims.
        let open = scene(vec![group(10, vec![filled_rect(11, 0.0, 0.0)])]);
        let open_prims = project(&open, &palette);
        assert_eq!(open_prims.len(), 1);
        assert!(
            !open_prims
                .iter()
                .any(|prim| matches!(prim, ScenePrim::ClipPush { .. } | ScenePrim::ClipPop { .. })),
            "clip-less group must not emit clip prims"
        );
        // A group WITH a clip emits push before its children and pop after,
        // both anchored to the group's stable id, and the clip rect is the
        // scene rect mapped 1:1.
        let clipped = scene(vec![clipped_group(
            10,
            clip_at(12.0, 16.0, 64.0, 32.0),
            vec![filled_rect(11, 0.0, 0.0)],
        )]);
        let prims = project(&clipped, &palette);
        assert_eq!(prims.len(), 3);
        assert_eq!(
            prims[0],
            ScenePrim::ClipPush {
                element: StableId(10),
                rect: Rectangle {
                    x: 12.0,
                    y: 16.0,
                    width: 64.0,
                    height: 32.0,
                },
            }
        );
        assert_eq!(prim_element(&prims[2]), StableId(10));
        assert!(matches!(prims[2], ScenePrim::ClipPop { .. }));
    }

    #[test]
    fn cubic_flattening_is_deterministic_and_step_bounded() {
        let palette = pinned_palette();
        // One chain of three mutually-continuing segments exercising all
        // three rule outcomes: chord 4.0 (< 8.0 -> exactly the 4-step floor),
        // chord 10_000.0 (-> the 64-step cap), chord 100.0 (-> 13 steps,
        // mid-rule).
        let segments = vec![
            straight_segment((0.0, 0.0), (4.0, 0.0)),
            straight_segment((4.0, 0.0), (10_004.0, 0.0)),
            straight_segment((10_004.0, 0.0), (10_104.0, 0.0)),
        ];
        let fixture = scene(vec![cubic_element(7, segments, false)]);
        let prims = project(&fixture, &palette);
        let ScenePrim::Fill { vertices, .. } = &prims[0] else {
            panic!("cubic prims are Fill-shaped");
        };
        // 1 chain-start + 4 + 64 + 13 samples.
        assert_eq!(vertices.len(), 1 + CUBIC_MIN_STEPS + CUBIC_MAX_STEPS + 13);
        // The chain starts at the first segment's start and ends at the last
        // segment's end, within tolerance.
        assert!((vertices[0].x - 0.0).abs() < 1e-6);
        assert!((vertices[0].y - 0.0).abs() < 1e-6);
        let last = *vertices.last().expect("chain vertices");
        assert!(
            (last.x - 10_104.0).abs() < 1e-6,
            "chain must end at the last segment's end, got {last:?}"
        );
        assert!(last.y.abs() < 1e-6);
        // Deterministic: the same scene flattens to identical vertices.
        let again = project(&fixture, &palette);
        assert_eq!(prims, again);
        // The boundary cases in isolation, one segment per element: a short
        // segment (chord < 8.0) gets exactly the 4-step floor; a long one
        // the 64-step cap.
        let short = scene(vec![cubic_element(
            1,
            vec![straight_segment((7.0, 7.0), (11.0, 7.0))],
            false,
        )]);
        let short_prims = project(&short, &palette);
        let ScenePrim::Fill { vertices, .. } = &short_prims[0] else {
            panic!("cubic prims are Fill-shaped");
        };
        assert_eq!(
            vertices.len(),
            1 + CUBIC_MIN_STEPS,
            "chord 4.0 < 8.0 gives exactly 4 substeps"
        );
        let long = scene(vec![cubic_element(
            2,
            vec![straight_segment((0.0, 0.0), (10_000.0, 0.0))],
            false,
        )]);
        let long_prims = project(&long, &palette);
        let ScenePrim::Fill { vertices, .. } = &long_prims[0] else {
            panic!("cubic prims are Fill-shaped");
        };
        assert_eq!(
            vertices.len(),
            1 + CUBIC_MAX_STEPS,
            "chord 10_000.0 gives the 64-substep cap"
        );
    }

    #[test]
    fn closed_paths_close() {
        let palette = pinned_palette();
        let path = scene(vec![path_element(
            3,
            vec![v2(0.0, 0.0), v2(10.0, 0.0), v2(5.0, 6.0)],
            true,
        )]);
        let prims = project(&path, &palette);
        let ScenePrim::Fill { vertices, .. } = &prims[0] else {
            panic!("path prims are Fill-shaped");
        };
        assert_eq!(
            vertices.last().copied(),
            vertices.first().copied(),
            "closed path rings back to the first vertex"
        );
        let cubic = scene(vec![cubic_element(
            4,
            vec![straight_segment((1.0, 1.0), (9.0, 1.0))],
            true,
        )]);
        let prims = project(&cubic, &palette);
        let ScenePrim::Fill { vertices, .. } = &prims[0] else {
            panic!("cubic prims are Fill-shaped");
        };
        assert_eq!(
            vertices.last().copied(),
            vertices.first().copied(),
            "closed cubic re-anchors at the first segment's start"
        );
        // And an OPEN path does not self-close.
        let open = scene(vec![path_element(
            5,
            vec![v2(0.0, 0.0), v2(10.0, 0.0), v2(5.0, 6.0)],
            false,
        )]);
        let prims = project(&open, &palette);
        let ScenePrim::Fill { vertices, .. } = &prims[0] else {
            panic!("path prims are Fill-shaped");
        };
        assert_ne!(vertices.last().copied(), vertices.first().copied());
    }

    #[test]
    fn palette_mapping_table_is_exhaustive_and_pinned() {
        let palette = pinned_palette();
        // Every ThemeColor token asserts its documented slot against the
        // distinctive-value palette (the table in `scene_color`'s doc is the
        // pinning this test proves).
        let pinned: &[(ThemeColor, u8)] = &[
            (ThemeColor::Normal, 20),
            (ThemeColor::Accent, 30),
            (ThemeColor::Strong, 40),
            (ThemeColor::Success, 60),
            (ThemeColor::Warning, 70),
            (ThemeColor::Danger, 80),
        ];
        for (token, red) in pinned {
            assert_eq!(
                scene_color(&palette, SceneColor::Theme(*token)),
                tint(*red, 0, 0),
                "{token:?} must resolve against its pinned slot"
            );
        }
        // Muted: the Normal slot channels with alpha scaled x0.55.
        let muted = scene_color(&palette, SceneColor::Theme(ThemeColor::Muted));
        let normal = scene_color(&palette, SceneColor::Theme(ThemeColor::Normal));
        assert_eq!(muted.r, normal.r);
        assert_eq!(muted.g, normal.g);
        assert_eq!(muted.b, normal.b);
        assert_eq!(
            muted.a,
            normal.a * MUTED_ALPHA_SCALE,
            "Muted = Normal text slot with x0.55 alpha"
        );
    }

    #[test]
    fn rgba_passes_through() {
        let palette = pinned_palette();
        assert_eq!(
            scene_color(&palette, SceneColor::Rgba(Rgba8([10, 20, 30, 255]))),
            Color::from_rgba8(10, 20, 30, 1.0),
            "fully opaque RGBA bytes resolve exactly"
        );
        // Half-transparent bytes resolve exactly (a / 255.0, no rounding).
        assert_eq!(
            scene_color(&palette, SceneColor::Rgba(Rgba8([10, 20, 30, 128]))).a,
            128.0f32 / 255.0,
        );
    }

    #[test]
    fn validate_failures_propagate() {
        let palette = pinned_palette();
        // Duplicated ids surface as SceneError::DuplicateId.
        let duplicate = scene(vec![number_rect(9), number_rect(9)]);
        match project_scene(&duplicate, &palette, SURFACE) {
            Err(SceneError::DuplicateId(id)) => assert_eq!(id, StableId(9)),
            other => panic!("expected DuplicateId, got {other:?}"),
        }
        // Depth: 64 nested groups closing on a port put the leaf at depth 65,
        // one past the 64 cap. The 63-deep twin stays inside the budget.
        let mut deep = port(1_000);
        for id in 0..64 {
            deep = group(900 + id, vec![deep]);
        }
        match project_scene(&scene(vec![deep]), &palette, SURFACE) {
            Err(SceneError::DepthBudgetExceeded { depth, max }) => {
                assert_eq!(depth, 65);
                assert_eq!(max, 64);
            }
            other => panic!("expected DepthBudgetExceeded, got {other:?}"),
        }
        let mut boundary = port(1_000);
        for id in 0..63 {
            boundary = group(800 + id, vec![boundary]);
        }
        let _ = project(&scene(vec![boundary]), &palette);
    }

    #[test]
    fn projection_is_bounded() {
        let palette = pinned_palette();
        // Exactly the node budget: 10_000 fill-only rects give exactly
        // 10_000 prims, no overflow, under the 2x prim bound.
        let full = scene((0..MAX_SCENE_NODES as u64).map(number_rect).collect());
        let prims = project(&full, &palette);
        assert_eq!(prims.len(), MAX_SCENE_NODES);
        assert!(prims.len() <= MAX_SCENE_PRIMS);
        // One node over the budget is refused by the propagated validator -
        // which also pins the API's cap at exactly this number.
        let over = scene((0..=MAX_SCENE_NODES as u64).map(number_rect).collect());
        match project_scene(&over, &palette, SURFACE) {
            Err(SceneError::NodeBudgetExceeded { nodes, max }) => {
                assert_eq!(nodes, MAX_SCENE_NODES + 1);
                assert_eq!(max, MAX_SCENE_NODES);
            }
            other => panic!("expected NodeBudgetExceeded, got {other:?}"),
        }
    }

    #[test]
    fn stable_ids_survive_projection() {
        let palette = pinned_palette();
        // group(1, clip){ group(2, clip){ rect(3) } } - every prim keeps its
        // source element id through the nesting.
        let fixture = scene(vec![clipped_group(
            1,
            clip_at(0.0, 0.0, 100.0, 100.0),
            vec![clipped_group(
                2,
                clip_at(10.0, 10.0, 50.0, 50.0),
                vec![filled_rect(3, 20.0, 20.0)],
            )],
        )]);
        let prims = project(&fixture, &palette);
        assert_eq!(
            prims
                .iter()
                .map(|prim| prim_element(prim))
                .collect::<Vec<_>>(),
            vec![
                StableId(1),
                StableId(2),
                StableId(3),
                StableId(2),
                StableId(1),
            ]
        );
        // The middle prim is still the rect's own fill.
        assert!(matches!(
            prims[2],
            ScenePrim::Fill {
                element: StableId(3),
                ..
            }
        ));
    }

    #[test]
    fn render_free_sink_count() {
        let palette = pinned_palette();
        // The renderer-free sink: count_prims counts the stream without
        // constructing a renderer; fast and deterministic, no sleeps.
        let fixture = scene(vec![
            filled_rect(1, 0.0, 0.0),
            group(
                10,
                vec![
                    clipped_group(
                        12,
                        clip_at(0.0, 0.0, 50.0, 50.0),
                        vec![text_element(13, "T")],
                    ),
                    port(14),
                    marker(15, MarkerKind::Dot),
                    asset(16),
                ],
            ),
        ]);
        let prims = project(&fixture, &palette);
        assert_eq!(count_prims(&prims), prims.len());
        // rect + ClipPush(12) + text + ClipPop(12) + port + marker + asset
        // frame = 7 (the clip-less container group(10) paints nothing).
        assert_eq!(count_prims(&prims), 7);
        assert_eq!(count_prims(&[]), 0);
    }

    #[test]
    fn minimal_point_edge_and_asset_prims_are_pinned() {
        let palette = pinned_palette();
        let fixture = scene(vec![
            port(1),
            marker(2, MarkerKind::Dot),
            marker(3, MarkerKind::Arrow),
            marker(4, MarkerKind::Diamond),
            SceneElement::Edge {
                id: StableId(5),
                name: None,
                actions: Vec::new(),
                drag: None,
                a: StableId(1),
                b: StableId(2),
                route: vec![v2(0.0, 0.0), v2(30.0, 0.0), v2(30.0, 20.0)],
                stroke: Some(ApiStroke {
                    color: SceneColor::Theme(ThemeColor::Danger),
                    width: 2.5,
                }),
                start_marker: Some(MarkerKind::Dot),
                end_marker: Some(MarkerKind::Arrow),
            },
            // Stroke-less edge: paints nothing, documented rule.
            SceneElement::Edge {
                id: StableId(6),
                name: None,
                actions: Vec::new(),
                drag: None,
                a: StableId(1),
                b: StableId(2),
                route: vec![v2(0.0, 40.0), v2(10.0, 40.0)],
                stroke: None,
                start_marker: None,
                end_marker: None,
            },
            asset(7),
            SceneElement::Rect {
                id: StableId(8),
                name: None,
                actions: Vec::new(),
                drag: None,
                pos: v2(0.0, 0.0),
                size: v2(4.0, 4.0),
                fill: Some(SceneColor::Theme(ThemeColor::Warning)),
                stroke: Some(ApiStroke {
                    color: SceneColor::Theme(ThemeColor::Muted),
                    width: 0.5,
                }),
            },
        ]);
        let prims = project(&fixture, &palette);
        // Port disc: Accent fill, 16 ring points starting at (at + r, at.y).
        let ScenePrim::Fill {
            vertices, color, ..
        } = &prims[0]
        else {
            panic!("port prims are Fill");
        };
        assert_eq!(vertices.len(), CIRCLE_SEGMENTS);
        assert_eq!(*color, tint(30, 0, 0), "ports pin to the Accent slot");
        assert!((vertices[0].x - 14.0).abs() < 1e-6);
        assert!((vertices[0].y - 20.0).abs() < 1e-6);
        // Dot marker: the same disc geometry as the port, marker colour
        // resolved through the table.
        let ScenePrim::Fill {
            vertices, color, ..
        } = &prims[1]
        else {
            panic!("marker prims are Fill");
        };
        assert_eq!(vertices.len(), CIRCLE_SEGMENTS);
        assert_eq!(*color, tint(30, 0, 0));
        // Arrow: 3-vertex triangle per the documented geometry.
        let ScenePrim::Fill { vertices, .. } = &prims[2] else {
            panic!("arrow prims are Fill");
        };
        assert_eq!(vertices.len(), 3);
        assert!((vertices[0].x - 13.0).abs() < 1e-6, "tip ahead of anchor");
        assert!((vertices[0].y - 5.0).abs() < 1e-6);
        assert!(
            (vertices[1].x - 1.0).abs() < 1e-6,
            "flat base behind anchor"
        );
        assert!(vertices[1].y.abs() < 1e-6);
        assert!((vertices[2].x - 1.0).abs() < 1e-6);
        assert!((vertices[2].y - 10.0).abs() < 1e-6);
        // Diamond: 4-vertex rotated square (the tip vertex first).
        let ScenePrim::Fill { vertices, .. } = &prims[3] else {
            panic!("diamond prims are Fill");
        };
        assert_eq!(vertices.len(), 4);
        assert!((vertices[0].x - 13.0).abs() < 1e-6);
        // Edge: plain polyline through the route points with the stroke's
        // own colour/width; endpoint markers NOT emitted (exactly one prim).
        let ScenePrim::Stroke {
            vertices,
            color,
            width,
            ..
        } = &prims[4]
        else {
            panic!("edge prims are Stroke");
        };
        assert_eq!(vertices.len(), 3);
        assert_eq!(*color, tint(80, 0, 0));
        assert_eq!(*width, 2.5);
        assert_eq!(prim_element(&prims[4]), StableId(5));
        // The stroke-less edge(6) painted nothing: the next prim is the
        // asset(7) frame outline.
        assert_eq!(prim_element(&prims[5]), StableId(7));
        let ScenePrim::Stroke {
            vertices, color, ..
        } = &prims[5]
        else {
            panic!("asset prims are Stroke");
        };
        assert_eq!(
            vertices,
            &[
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 10.0),
                Point::new(0.0, 0.0),
            ]
        );
        assert_eq!(*color, tint(30, 0, 0));
        // Rect with both fill and stroke -> a single FillStroke carrying
        // both resolved colours and the declared stroke width.
        let ScenePrim::FillStroke {
            element: StableId(8),
            stroke_width,
            ..
        } = &prims[6]
        else {
            panic!("two-sided rect is FillStroke");
        };
        assert_eq!(*stroke_width, 0.5);
    }

    // -- SCN-011b additions (per-subtree prim store) --------------------------
    //
    // Headless discipline as everywhere in this module: no renderer, no
    // pixel-path work; the store's retention contract is asserted on Arc
    // identity and deep prim equality.

    use ocs_plugin_api::scene::{PanelScenePatch, SceneOp};

    /// A patch fixture for the host-side flows (mirrors the API crate's
    /// `patch` helper shape; local to these tests).
    fn patch(base: u64, next: u64, ops: Vec<SceneOp>) -> PanelScenePatch {
        PanelScenePatch {
            base_revision: SceneRevision(base),
            revision: SceneRevision(next),
            ops,
        }
    }

    /// A fully synced store over `fixture` (the snapshot-flow baseline).
    fn synced_store(
        fixture: &PanelScene,
        palette: &iced::theme::Palette,
        surface: u64,
    ) -> ScenePrimStore {
        let mut store = ScenePrimStore::new(StableId(surface));
        store
            .sync_full(fixture, palette)
            .expect("fixture store syncs");
        store
    }

    #[test]
    fn store_segments_match_full_projection() {
        let palette = pinned_palette();
        // Root elements with nested clip-carrying groups INSIDE them: the
        // v1 segmentation must keep every clip bracket whole within its
        // root segment, and the segments must concatenate into exactly the
        // full projection's prim stream.
        let fixture = scene(vec![
            filled_rect(1, 0.0, 0.0),
            group(
                10,
                vec![
                    filled_rect(11, 0.0, 0.0),
                    clipped_group(
                        12,
                        clip_at(0.0, 0.0, 50.0, 50.0),
                        vec![text_element(13, "T")],
                    ),
                ],
            ),
            filled_rect(2, 5.0, 5.0),
        ]);
        let store = synced_store(&fixture, &palette, 9_001);
        // One segment per TOP-LEVEL subtree (the v1 segmentation rule).
        assert_eq!(store.live_segments().len(), fixture.elements.len());
        // Concatenated segments == project_scene prims (deep equal).
        let concatenated: Vec<ScenePrim> = store
            .live_arcs()
            .iter()
            .flat_map(|part| part.iter().cloned())
            .collect();
        assert_eq!(concatenated, project(&fixture, &palette));
    }

    #[test]
    fn incremental_sync_touches_only_touched_segments() {
        let palette = pinned_palette();
        // 60 root-level subtrees (one fill-only rect each = one segment).
        let mut scene = scene(
            (0..60u64)
                .map(|id| filled_rect(id, id as f64, 0.0))
                .collect(),
        );
        let mut store = synced_store(&scene, &palette, 9_002);
        assert_eq!(store.counters().full_reprojects, 1);
        assert_eq!(store.counters().full_reprojects_explicit, 1);
        assert_eq!(store.counters().subtree_reprojects, 0);

        // Hold every segment's Arc BEFORE the patch for the identity check.
        let before = store.live_segments();

        // A real host flow: apply the patch through the SINGLE mutation path
        // and feed the returned report to the store's incremental sync.
        let report = scene
            .apply_patch_report(patch(
                1,
                2,
                vec![SceneOp::Replace {
                    element: filled_rect(7, 40.0, 40.0),
                }],
            ))
            .expect("patch applies");
        store
            .sync_incremental(&scene, &report, &palette)
            .expect("incremental sync");

        // Exactly ONE segment re-projected; the other 59 kept their Arcs.
        let counters = store.counters();
        assert_eq!(counters.subtree_reprojects, 1);
        assert_eq!(counters.subtree_retained_hits, 59);
        assert_eq!(counters.full_reprojects, 1, "no fallback fired");
        assert_eq!(counters.retired_purges, 0);
        let after = store.live_segments();
        assert_eq!(after.len(), before.len());
        for ((old_id, old_arc), (new_id, new_arc)) in before.iter().zip(after.iter()) {
            assert_eq!(old_id, new_id);
            if *old_id == StableId(7) {
                // The touched segment's prims were re-projected: the Arc was
                // swapped (old content dropped with the new tree's prims).
                assert!(
                    !Arc::ptr_eq(old_arc, new_arc),
                    "the touched segment must refresh its Arc"
                );
                assert_ne!(**old_arc, **new_arc, "the replacement moved the rect");
            } else {
                // Pointer equality: the retention contract, bit-for-bit.
                assert!(
                    Arc::ptr_eq(old_arc, new_arc),
                    "untouched segment {old_id:?} must keep its exact Arc"
                );
            }
        }
    }

    #[test]
    fn palette_change_full_reproject_documented() {
        let palette = pinned_palette();
        let mut scene = scene(vec![filled_rect(1, 0.0, 0.0), filled_rect(2, 20.0, 0.0)]);
        let mut store = synced_store(&scene, &palette, 9_003);
        let before = store.live_segments();

        let report = scene
            .apply_patch_report(patch(
                1,
                2,
                vec![SceneOp::Replace {
                    element: filled_rect(1, 40.0, 40.0),
                }],
            ))
            .expect("patch applies");

        // The palette moved (a bound slot flips): the documented policy is a
        // whole-store full re-projection — colors are resolved inside the
        // prims, so no segment can be kept safely.
        let mut darkened = pinned_palette();
        darkened.background.base.text = tint(99, 0, 0);
        store
            .sync_incremental(&scene, &report, &darkened)
            .expect("palette-change sync");

        let counters = store.counters();
        assert_eq!(
            counters.full_reprojects, 2,
            "sync_full + the palette fallback"
        );
        assert_eq!(counters.full_reprojects_palette, 1);
        assert_eq!(counters.subtree_reprojects, 0);
        assert_eq!(counters.subtree_retained_hits, 0);
        // No stale Arc reused: every segment is a NEW Arc under the new
        // palette.
        let after = store.live_segments();
        assert_eq!(after.len(), before.len());
        for ((_, old_arc), (_, new_arc)) in before.iter().zip(after.iter()) {
            assert!(!Arc::ptr_eq(old_arc, new_arc));
        }
    }

    #[test]
    fn report_truncation_preserves_segment_safety() {
        let palette = pinned_palette();
        let mut scene = scene(vec![filled_rect(1, 0.0, 0.0)]);
        let mut store = synced_store(&scene, &palette, 9_004);
        let before = store.live_segments();

        // A patch whose touched set runs past the report cap: 70 root adds
        // name 70 touched ids — 64 kept, 6 truncated.
        let ops: Vec<SceneOp> = (100..170u64)
            .map(|id| SceneOp::Add {
                parent: None,
                element: filled_rect(id, id as f64, 0.0),
            })
            .collect();
        let report = scene
            .apply_patch_report(patch(1, 2, ops))
            .expect("70 adds apply");
        assert_eq!(report.touched_truncated, 6, "the safety trigger is armed");

        // The documented SAFETY rule: on ANY truncation sync_incremental
        // returns to sync_full semantics — no wrong-by-truncation staleness.
        store
            .sync_incremental(&scene, &report, &palette)
            .expect("truncation fallback sync");
        let counters = store.counters();
        assert_eq!(counters.full_reprojects_truncation, 1);
        assert_eq!(counters.full_reprojects, 2);
        assert_eq!(
            counters.subtree_reprojects, 0,
            "the fallback is a FULL flatten"
        );
        assert_eq!(counters.subtree_retained_hits, 0);
        // Full-flatten semantics hold exactly: segments == full projection,
        // no stale Arc anywhere.
        let concatenated: Vec<ScenePrim> = store
            .live_arcs()
            .iter()
            .flat_map(|part| part.iter().cloned())
            .collect();
        assert_eq!(concatenated, project(&scene, &palette));
        assert_eq!(store.live_segments().len(), 71);
        let after = store.live_segments();
        for ((_, old_arc), (_, new_arc)) in before.iter().zip(after.iter()) {
            assert!(!Arc::ptr_eq(old_arc, new_arc));
        }
    }

    #[test]
    fn store_rejects_out_of_order_reports() {
        let palette = pinned_palette();
        // The store is synced at revision 3 (panel snapshot flow).
        let scene3 = PanelScene {
            revision: SceneRevision(3),
            elements: vec![filled_rect(1, 0.0, 0.0), filled_rect(2, 20.0, 0.0)],
        };
        let mut store = synced_store(&scene3, &palette, 9_005);

        // A report from an older revision chain (a foreign/late delivery):
        // base 1 → 2 applied to its own scene copy.
        let mut older = PanelScene {
            revision: SceneRevision(1),
            elements: vec![filled_rect(1, 0.0, 0.0)],
        };
        let report = older
            .apply_patch_report(patch(
                1,
                2,
                vec![SceneOp::Add {
                    parent: None,
                    element: filled_rect(5, 5.0, 5.0),
                }],
            ))
            .expect("the older chain applies to its own copy");

        store
            .sync_incremental(&older, &report, &palette)
            .expect("stale-report delivery is handled defensively");
        // The documented defensive answer: a full re-projection from the
        // given scene, counted under its own reason.
        let counters = store.counters();
        assert_eq!(counters.full_reprojects_stale, 1);
        assert_eq!(counters.full_reprojects, 2);
        assert_eq!(counters.subtree_reprojects, 0);
        // The store re-synced at the delivered revision (consistency).
        assert!(store.is_fresh(
            SceneRevision(2),
            crate::ui::window::plugin_scene_cache::palette_fingerprint(&palette)
        ));
        let concatenated: Vec<ScenePrim> = store
            .live_arcs()
            .iter()
            .flat_map(|part| part.iter().cloned())
            .collect();
        assert_eq!(concatenated, project(&older, &palette));
    }
}
