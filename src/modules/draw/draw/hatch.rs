// Hatch, gradient, and boundary commands.

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::IconKind;
use crate::scene::model::hatch_model::{GradientKind, HatchModel, HatchPattern, PatFamily};
use crate::scene::model::wire_model::WireModel;
use crate::t;
use super::polyline::{ArcStep, Sub};
use document::Handle;
use glam::DVec3;
use kernel::geom2d::{
    bounded_faces, contains, ring_nesting_depths, signed_area, Curve, Line, Tolerance,
};

// ── Icons ──────────────────────────────────────────────────────────────────

const ICON_HATCH: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_lines.svg"
));
const ICON_GRADIENT: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_gradient.svg"
));
const ICON_BOUNDARY: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_boundary.svg"
));

// ── Dropdown metadata ──────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "HATCH";
pub const ICON: IconKind = ICON_HATCH;

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    ("HATCH", "Hatch", ICON_HATCH),
    ("GRADIENT", "Gradient", ICON_GRADIENT),
    ("BOUNDARY", "Boundary", ICON_BOUNDARY),
];

// ── Shared ─────────────────────────────────────────────────────────────────

/// The largest gap a boundary pick may bridge (HPGAPTOL range).
pub const MAX_GAP_TOLERANCE: f64 = 5000.0;

/// A typed gap tolerance, if it is a number in `0..=MAX_GAP_TOLERANCE`.
fn parse_gap_tolerance(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|value| (0.0..=MAX_GAP_TOLERANCE).contains(value))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HatchMode {
    PickInside,
    SelectObjects,
    Manual,
}

// ── CPU point-in-polygon (ray casting) ────────────────────────────────────

fn point_in_polygon(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let vi = poly[i];
        let vj = poly[j];
        if (vi[1] > p[1]) != (vj[1] > p[1]) {
            let x_int = (vj[0] - vi[0]) * (p[1] - vi[1]) / (vj[1] - vi[1]) + vi[0];
            if p[0] < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Shoelace-area magnitude of a polygon. Used to pick the smallest enclosing
/// outline when a click falls inside several nested boundaries.
fn polygon_area(poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let origin = poly[0];
    let mut a = 0.0;
    for i in 1..n - 1 {
        let current = poly[i];
        let next = poly[i + 1];
        a += (current[0] - origin[0]) * (next[1] - origin[1])
            - (next[0] - origin[0]) * (current[1] - origin[1]);
    }
    (a * 0.5).abs()
}

/// True when every vertex of `inner` lies inside `outer`. Sufficient to
/// recognise a closed hatch outline as nested inside another for the common
/// rectangle / closed-polyline case.
fn polygon_contains_polygon(outer: &[[f64; 2]], inner: &[[f64; 2]]) -> bool {
    if inner.len() < 3 {
        return false;
    }
    inner.iter().all(|&v| point_in_polygon(v, outer))
}

/// Resolve the innermost clicked ring and its direct holes.
pub(crate) fn resolve_hatch_rings(outlines: &[Vec<[f64; 2]>], p: [f64; 2]) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut containing: Vec<(usize, f64)> = outlines
        .iter()
        .enumerate()
        .filter(|(_, o)| point_in_polygon(p, o))
        .map(|(i, o)| (i, polygon_area(o)))
        .collect();
    if containing.is_empty() {
        return None;
    }
    // Innermost (smallest-area) outline containing the point is the fill.
    containing.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let outer_idx = containing[0].0;
    let outer = &outlines[outer_idx];

    let mut rings = vec![outer.clone()];
    for (i, o) in outlines.iter().enumerate() {
        if i == outer_idx {
            continue;
        }
        // Candidate hole: fully nested inside the fill outline. (An outline the
        // click sits in cannot qualify — it would have been the smaller fill.)
        if !polygon_contains_polygon(outer, o) || point_in_polygon(p, o) {
            continue;
        }
        // Only DIRECT children become holes. If another outline sits strictly
        // between `outer` and `o` (inside `outer`, and enclosing `o`), then `o`
        // belongs to that intermediate region's own fill; flagging it here would
        // re-fill it under even-odd once nesting reaches three levels.
        let has_intermediate = outlines.iter().enumerate().any(|(k, x)| {
            k != i
                && k != outer_idx
                && polygon_contains_polygon(outer, x)
                && polygon_contains_polygon(x, o)
        });
        if !has_intermediate {
            rings.push(o.clone());
        }
    }
    Some(rings)
}

/// HATCH's picked region: [`resolve_hatch_rings`] plus every outline nested
/// deeper inside it. The reference keeps all of them, whatever the island
/// style; the style picks the drawn loops from their path flags.
fn picked_hatch_rings(outlines: &[Vec<[f64; 2]>], p: [f64; 2]) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut rings = resolve_hatch_rings(outlines, p)?;
    let outer = rings[0].clone();
    for outline in outlines {
        if !rings.contains(outline)
            && polygon_contains_polygon(&outer, outline)
            && !point_in_polygon(p, outline)
        {
            rings.push(outline.clone());
        }
    }
    Some(rings)
}

/// Pack one or more rings (outer boundary + optional holes) into the Hatch
/// model storage: the `boundary` f32 ring list (NaN-separated) plus the exact
/// `boundary_wcs` (NaN-separated) used for persistence. The first vertex of the
/// first ring anchors the shared origin.
fn pack_rings(rings: &[Vec<[f64; 2]>]) -> (Vec<[f32; 2]>, [f64; 2], Vec<[f64; 2]>) {
    let mut wcs: Vec<[f64; 2]> = Vec::new();
    let mut first = true;
    for ring in rings {
        if !first {
            wcs.push([f64::NAN, f64::NAN]);
        }
        first = false;
        wcs.extend(ring.iter().copied());
    }
    let (rel, origin) = rte_boundary(wcs.iter().map(|&[x, y]| (x, y)));
    (rel, origin, wcs)
}

/// Store boundary points as precise-origin-relative offsets.
fn rte_boundary(pts: impl Iterator<Item = (f64, f64)>) -> (Vec<[f32; 2]>, [f64; 2]) {
    let pts: Vec<(f64, f64)> = pts.collect();
    let Some(&(ox, oy)) = pts.first() else {
        return (vec![], [0.0; 2]);
    };
    let rel = pts
        .iter()
        .map(|&(x, y)| [(x - ox) as f32, (y - oy) as f32])
        .collect();
    (rel, [ox, oy])
}

// ── HATCH / GRADIENT command ───────────────────────────────────────────────

thread_local! {
    /// HPNAME: the pattern a new hatch starts with, for this session only.
    static HPNAME: std::cell::RefCell<String> = std::cell::RefCell::new("_SOLID".to_string());
}

/// HPNAME as it is stored (`_SOLID` at first, `SOLID` after a gradient).
pub fn hpname() -> String {
    HPNAME.with(|name| name.borrow().clone())
}

pub fn set_hpname(name: &str) {
    HPNAME.with(|hpname| *hpname.borrow_mut() = name.to_string());
}

/// The pattern a typed HPNAME names, spelled as the pattern file has it
/// (a leading `_` kept); `.` is the first pattern of the file.
pub fn hpname_value(typed: &str) -> Option<String> {
    let typed = typed.trim().trim_matches('"');
    if typed == "." {
        return crate::scene::model::hatch_patterns::catalog()
            .iter()
            .find(|entry| matches!(entry.gpu, HatchPattern::Pattern(_)))
            .map(|entry| entry.name.clone());
    }
    let bare = typed.strip_prefix('_').unwrap_or(typed);
    crate::scene::model::hatch_patterns::find(bare).map(|entry| {
        let prefix = if typed.starts_with('_') { "_" } else { "" };
        format!("{prefix}{}", entry.name)
    })
}

/// HPNAME when it names a pattern, else `fallback`.
pub fn last_pattern_name(fallback: &str) -> String {
    let name = hpname().trim_start_matches('_').to_string();
    if name.eq_ignore_ascii_case("SOLID") {
        fallback.to_string()
    } else {
        name
    }
}

/// The boundary method and draw mode HATCH and GRADIENT start with, kept in
/// the profile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HatchPrefs {
    /// 0 pick internal point, 1 select objects, 2 draw.
    pub method: u8,
    /// Draw mode Path (else Area).
    pub path: bool,
    /// Path width.
    pub width: f64,
    /// Path alignment: 0 center, 1 inside, 2 outside.
    pub alignment: u8,
}

impl Default for HatchPrefs {
    fn default() -> Self {
        Self {
            method: 0,
            path: false,
            width: 0.25,
            alignment: 0,
        }
    }
}

/// What a hatch is filled with.
#[derive(Clone, Debug, PartialEq)]
pub enum HatchFill {
    Solid,
    Pattern(String),
    /// Parallel lines `spacing` apart, crossed at 90° when `double`.
    User { spacing: f64, double: bool },
    Gradient(GradientKind, bool),
}

/// HATCH's settings as the Hatch panel shows and edits them.
#[derive(Clone, Debug, PartialEq)]
pub struct HatchSettings {
    /// Solid, a named pattern, user-defined lines or a gradient.
    pub fill: HatchFill,
    pub angle_deg: f64,
    pub scale: f64,
    pub origin: [f64; 2],
    /// 0 pick internal point, 1 select objects, 2 draw.
    pub method: u8,
    pub islands: document::entities::HatchStyleType,
    pub associative: bool,
    pub separate: bool,
    pub keep_boundaries: bool,
    pub gap_tolerance: f64,
    /// `None`: the current colour.
    pub color: Option<document::types::Color>,
    /// `None`: no background colour.
    pub background: Option<document::types::Color>,
    /// `None`: the current layer.
    pub layer: Option<String>,
    /// `None`: the current transparency; else 0–90 percent.
    pub transparency: Option<u8>,
    /// GFCLR1 / GFCLR2.
    pub gradient_colors: [document::types::Color; 2],
    /// HPDRAWORDER: 0 none, 1 back, 2 front, 3 behind the boundary,
    /// 4 in front of the boundary.
    pub draw_order: u8,
    /// HPANNOTATIVE.
    pub annotative: bool,
}

/// The prompt HATCH is at besides its boundary method's own prompt.
#[derive(Clone, Copy, PartialEq)]
enum HatchStep {
    /// The method's prompt: pick, select, or the draw start / next point.
    Main,
    /// Draw: Mode's Area/Path question.
    DrawMode,
    /// Draw: Path alignment.
    Alignment,
    /// Draw: Path width.
    Width,
    /// Draw: Length's distance along the last segment.
    Length,
}

/// A draw-mode boundary made by a drawing command's own steps.
enum Shape {
    Rect(super::shapes::RectCommand),
    Circle(super::circle::CircleCommand),
    ThreePoint(super::circle::Circle3PCommand),
    TwoPoint(super::circle::Circle2PCommand),
    Ttr(super::circle::CircleTTRCommand),
}

impl Shape {
    fn command(&self) -> &dyn CadCommand {
        match self {
            Shape::Rect(c) => c,
            Shape::Circle(c) => c,
            Shape::ThreePoint(c) => c,
            Shape::TwoPoint(c) => c,
            Shape::Ttr(c) => c,
        }
    }

    fn command_mut(&mut self) -> &mut dyn CadCommand {
        match self {
            Shape::Rect(c) => c,
            Shape::Circle(c) => c,
            Shape::ThreePoint(c) => c,
            Shape::TwoPoint(c) => c,
            Shape::Ttr(c) => c,
        }
    }

    fn prompt(&self) -> String {
        match self {
            Shape::Rect(c) => c.hatch_prompt(),
            Shape::Circle(c) => c.hatch_prompt(),
            Shape::ThreePoint(c) => c.hatch_prompt(),
            Shape::TwoPoint(c) => c.hatch_prompt(),
            Shape::Ttr(c) => c.hatch_prompt(),
        }
    }

    /// Ttr waiting for a tangent object.
    fn wants_tangent(&self) -> bool {
        matches!(self, Shape::Ttr(c) if c.needs_tangent_pick())
    }
}

/// The method HATCH starts with, as the profile stores it.
fn method_code(mode: HatchMode) -> u8 {
    match mode {
        HatchMode::PickInside => 0,
        HatchMode::SelectObjects => 1,
        HatchMode::Manual => 2,
    }
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// The pattern a fill draws with, and its name.
/// GFCLR1 / GFCLR2: the two colours a new gradient starts with.
pub const GRADIENT_COLORS: [document::types::Color; 2] =
    [document::types::Color::Index(5), document::types::Color::Index(2)];

/// A colour as the fill renders it.
pub fn color_rgba(color: &document::types::Color, alpha: f32) -> [f32; 4] {
    let (r, g, b) = match color {
        document::types::Color::Rgb { r, g, b } => (*r, *g, *b),
        document::types::Color::Index(index) => {
            document::types::aci_table::aci_to_rgb(*index).unwrap_or((128, 128, 128))
        }
        _ => (255, 255, 255),
    };
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, alpha]
}

pub fn fill_pattern(
    fill: &HatchFill,
    angle_deg: f32,
    colors: &[document::types::Color; 2],
) -> (String, HatchPattern) {
    match fill {
        HatchFill::Solid => ("SOLID".into(), HatchPattern::Solid),
        HatchFill::Pattern(name) => crate::scene::model::hatch_patterns::find(name)
            .map(|entry| (entry.name.clone(), entry.gpu.clone()))
            .unwrap_or_else(|| ("SOLID".into(), HatchPattern::Solid)),
        HatchFill::User { spacing, double } => {
            let family = |angle_deg: f32| PatFamily {
                angle_deg,
                x0: 0.0,
                y0: 0.0,
                dx: 0.0,
                dy: *spacing as f32,
                dashes: vec![],
            };
            let mut families = vec![family(0.0)];
            if *double {
                families.push(family(90.0));
            }
            ("_USER".into(), HatchPattern::Pattern(families))
        }
        HatchFill::Gradient(kind, invert) => (
            kind.dxf_name(*invert).into(),
            HatchPattern::Gradient {
                angle_deg,
                color2: color_rgba(&colors[1], 0.0),
                kind: *kind,
                invert: *invert,
                shift: 0.0,
            },
        ),
    }
}

fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    let scale = 1.0 + a[0].abs().max(a[1].abs());
    (a[0] - b[0]).abs() <= 1.0e-9 * scale && (a[1] - b[1]).abs() <= 1.0e-9 * scale
}

/// A loop of points and bulges with its arcs tessellated.
fn bulged_ring(points: &[[f64; 2]], bulges: &[f64]) -> Vec<[f64; 2]> {
    let count = points.len();
    let mut ring = Vec::new();
    for (index, &start) in points.iter().enumerate() {
        let end = points[(index + 1) % count];
        let bulge = bulges.get(index).copied().unwrap_or(0.0);
        match kernel::geom2d::BulgeArc::from_bulge(start, end, bulge)
            .filter(|_| bulge.abs() > 1.0e-12)
        {
            Some(arc) => {
                let mut arc_points = arc.tessellate_angle(std::f64::consts::PI / 32.0);
                arc_points.pop();
                ring.extend(arc_points);
            }
            None => ring.push(start),
        }
    }
    ring
}

/// One side of a path band: the path's segments moved `distance` to the left
/// of travel (a negative distance to the right), joined where they meet.
/// Lines meeting lines are mitred; segments that already meet (a tangent
/// arc) join at that point. `None` for a joint it cannot make that way.
fn offset_rail(
    points: &[[f64; 2]],
    bulges: &[f64],
    closed: bool,
    distance: f64,
) -> Option<(Vec<[f64; 2]>, Vec<f64>)> {
    let count = points.len();
    let segments: Vec<([f64; 2], [f64; 2], f64)> = (0..bulges.len())
        .map(|k| (points[k], points[(k + 1) % count], bulges[k]))
        .collect();
    let mut moved = Vec::with_capacity(segments.len());
    for &(a, b, bulge) in &segments {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx.hypot(dy);
        if length <= 0.0 {
            return None;
        }
        if bulge.abs() <= 1.0e-12 {
            let n = [-dy / length * distance, dx / length * distance];
            moved.push(([a[0] + n[0], a[1] + n[1]], [b[0] + n[0], b[1] + n[1]], 0.0));
            continue;
        }
        let arc = kernel::geom2d::BulgeArc::from_bulge(a, b, bulge)?;
        // A counter-clockwise arc has its centre on the left.
        let radius = if bulge > 0.0 {
            arc.radius - distance
        } else {
            arc.radius + distance
        };
        if radius <= 0.0 {
            return None;
        }
        let at = |p: [f64; 2]| {
            let (vx, vy) = (p[0] - arc.center[0], p[1] - arc.center[1]);
            let r = vx.hypot(vy);
            [arc.center[0] + vx / r * radius, arc.center[1] + vy / r * radius]
        };
        moved.push((at(a), at(b), bulge));
    }
    let join = |k: usize, next: usize| -> Option<[f64; 2]> {
        let (a0, a1, ab) = moved[k];
        let (b0, b1, bb) = moved[next];
        if same_point(a1, b0) {
            return Some(a1);
        }
        if ab.abs() > 1.0e-12 || bb.abs() > 1.0e-12 {
            return None;
        }
        let (d1, d2) = ([a1[0] - a0[0], a1[1] - a0[1]], [b1[0] - b0[0], b1[1] - b0[1]]);
        let cross = d1[0] * d2[1] - d1[1] * d2[0];
        if cross.abs() <= 1.0e-12 * d1[0].hypot(d1[1]) * d2[0].hypot(d2[1]) {
            return Some(a1);
        }
        let t = ((b0[0] - a0[0]) * d2[1] - (b0[1] - a0[1]) * d2[0]) / cross;
        Some([a0[0] + d1[0] * t, a0[1] + d1[1] * t])
    };
    let n = moved.len();
    let mut out = Vec::with_capacity(n + 1);
    if closed {
        for k in 0..n {
            out.push(join((k + n - 1) % n, k)?);
        }
    } else {
        out.push(moved[0].0);
        for k in 1..n {
            out.push(join(k - 1, k)?);
        }
        out.push(moved[n - 1].1);
    }
    let mut out_bulges: Vec<f64> = segments.iter().map(|s| s.2).collect();
    if !closed {
        out_bulges.push(0.0);
    }
    Some((out, out_bulges))
}

/// Join boundary edges end to end into closed loops, dropping vertices in
/// the middle of a straight run.
fn chain_loops(mut edges: Vec<[[f64; 2]; 2]>) -> Vec<Vec<[f64; 2]>> {
    let size = edges
        .iter()
        .flatten()
        .fold(1.0_f64, |size, p| size.max(p[0].abs()).max(p[1].abs()));
    let tolerance = size * 1.0e-9;
    let same = |a: [f64; 2], b: [f64; 2]| {
        (a[0] - b[0]).abs() <= tolerance && (a[1] - b[1]).abs() <= tolerance
    };
    let mut loops = Vec::new();
    while let Some(first) = edges.pop() {
        let mut ring = vec![first[0], first[1]];
        loop {
            let end = ring[ring.len() - 1];
            if same(end, ring[0]) {
                ring.pop();
                break;
            }
            let Some(index) = edges.iter().position(|e| same(e[0], end) || same(e[1], end))
            else {
                break;
            };
            let edge = edges.swap_remove(index);
            ring.push(if same(edge[0], end) { edge[1] } else { edge[0] });
        }
        let count = ring.len();
        let ring: Vec<[f64; 2]> = (0..count)
            .filter(|&i| {
                let [a, b, c] = [ring[(i + count - 1) % count], ring[i], ring[(i + 1) % count]];
                let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
                cross.abs() > tolerance * tolerance
            })
            .map(|i| ring[i])
            .collect();
        if ring.len() >= 3 {
            loops.push(ring);
        }
    }
    loops
}

pub struct HatchCommand {
    boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    point_regions: Vec<Vec<Vec<[f64; 2]>>>,
    object_regions: Vec<Vec<Vec<[f64; 2]>>>,
    selected_objects: Vec<Handle>,
    mode: HatchMode,
    step: HatchStep,
    prefs: HatchPrefs,
    /// The method or draw mode changed: kept for the next start.
    prefs_changed: bool,
    /// GRADIENT: the same prompts, a gradient fill.
    gradient: bool,
    /// A point was picked: later picks only analyse islands.
    analyzed: bool,
    manual_pts: Vec<DVec3>,
    manual_bulges: Vec<f64>,
    manual_arc_mode: bool,
    /// The arc option being answered (PLINE's arc steps); `None` asks for
    /// the endpoint of a tangent arc.
    arc_sub: Sub,
    shape: Option<Shape>,
    retain_boundaries: bool,
    /// The fill chosen in the settings (else the selected hatch's or HPNAME).
    fill: Option<HatchFill>,
    angle_override: Option<f32>,
    scale_override: Option<f32>,
    default_origin: [f64; 2],
    associative: bool,
    separate_hatches: bool,
    island_style: document::entities::HatchStyleType,
    /// The Hatch panel's entity settings (see `HatchSettings`).
    color: Option<document::types::Color>,
    background: Option<document::types::Color>,
    layer: Option<String>,
    transparency: Option<u8>,
    draw_order: u8,
    annotative: bool,
    gradient_colors: [document::types::Color; 2],
    /// The elevation of the boundary being finished (a rectangle's).
    elevation: f64,
    inherited: Option<(HatchModel, document::types::Color, document::types::Transparency)>,
    plane: WorkingPlane,
    /// HPGAPTOL: gaps up to this size count as closed for a picked point.
    gap_tolerance: f64,
}

impl HatchCommand {
    pub fn new(
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        selected_objects: Vec<Handle>,
        inherited: Option<(HatchModel, document::types::Color, document::types::Transparency)>,
        plane: WorkingPlane,
    ) -> Self {
        let selected_objects: Vec<_> = selected_objects
            .into_iter()
            .filter(|handle| boundary_sources.contains_key(handle))
            .collect();
        let has_selection = !selected_objects.is_empty();
        let mut command = Self {
            boundary_sources,
            point_regions: Vec::new(),
            object_regions: Vec::new(),
            selected_objects: Vec::new(),
            mode: if has_selection {
                HatchMode::SelectObjects
            } else {
                HatchMode::PickInside
            },
            step: HatchStep::Main,
            prefs: HatchPrefs::default(),
            prefs_changed: false,
            gradient: false,
            analyzed: false,
            manual_pts: vec![],
            manual_bulges: vec![],
            manual_arc_mode: false,
            arc_sub: Sub::None,
            shape: None,
            retain_boundaries: false,
            fill: None,
            angle_override: None,
            scale_override: None,
            default_origin: [0.0, 0.0],
            associative: true,
            separate_hatches: false,
            island_style: inherited
                .as_ref()
                .map(|(model, _, _)| model.style)
                .unwrap_or(document::entities::HatchStyleType::Normal),
            color: None,
            background: None,
            layer: None,
            transparency: None,
            // HPDRAWORDER starts behind the boundary.
            draw_order: 3,
            annotative: false,
            gradient_colors: GRADIENT_COLORS,
            elevation: 0.0,
            inherited,
            plane,
            gap_tolerance: 0.0,
        };
        command.set_object_selection(selected_objects);
        command
    }

    /// GRADIENT: the same boundary steps with a gradient fill.
    pub fn gradient(mut self) -> Self {
        self.gradient = true;
        self.inherited = None;
        self.fill = Some(HatchFill::Gradient(GradientKind::Linear, false));
        self
    }

    /// The boundary method and draw mode used last. Preselected objects
    /// still start in select mode.
    pub fn with_prefs(mut self, prefs: HatchPrefs) -> Self {
        self.prefs = prefs;
        if self.selected_objects.is_empty() {
            self.mode = match prefs.method {
                1 => HatchMode::SelectObjects,
                2 => HatchMode::Manual,
                _ => HatchMode::PickInside,
            };
        }
        self
    }

    /// The line printed before the first prompt (draw mode's current mode).
    pub fn start_message(&self) -> Option<String> {
        (self.mode == HatchMode::Manual).then(|| self.current_mode_text())
    }

    /// HPASSOC: whether the hatch starts out associative.
    pub fn with_associative(mut self, associative: bool) -> Self {
        self.associative = associative;
        self
    }

    /// HPGAPTOL: the largest gap a picked boundary may have.
    pub fn with_gap_tolerance(mut self, gap_tolerance: f64) -> Self {
        self.gap_tolerance = gap_tolerance;
        self
    }

    pub fn with_origin(mut self, origin: [f64; 2]) -> Self {
        self.default_origin = origin;
        self
    }

    pub fn with_pattern(mut self, name: impl Into<String>, pattern: HatchPattern) -> Self {
        let name = name.into();
        self.fill = Some(match pattern {
            HatchPattern::Solid => HatchFill::Solid,
            HatchPattern::Gradient { kind, invert, .. } => HatchFill::Gradient(kind, invert),
            HatchPattern::Pattern(_) => HatchFill::Pattern(name),
        });
        self
    }

    pub fn finish_selected(mut self) -> CmdResult {
        self.on_enter()
    }

    /// Whether `handles` gave no closed boundary.
    fn set_object_selection(&mut self, handles: Vec<Handle>) -> bool {
        let mut segments = Vec::new();
        for handle in &handles {
            if let Some(source) = self.boundary_sources.get(handle) {
                segments.extend(source.segments.iter().copied());
            }
        }
        self.object_regions = bounded_faces(&segments, Tolerance::new(1.0e-6))
            .into_iter()
            .map(|ring| vec![ring])
            .collect();
        let missed = !handles.is_empty() && self.object_regions.is_empty();
        self.selected_objects = handles;
        missed
    }

    fn add_point_region(&mut self, rings: Vec<Vec<[f64; 2]>>) {
        let duplicate = rings.first().is_some_and(|outer| {
            self.point_regions
                .iter()
                .any(|region| region.first() == Some(outer))
        });
        if !duplicate {
            self.point_regions.push(rings);
        }
    }

    fn region_count(&self) -> usize {
        self.point_regions.len() + self.object_regions.len()
    }

    /// Switch to a boundary method typed at a prompt.
    fn choose(&mut self, mode: HatchMode) -> CmdResult {
        self.mode = mode;
        self.prefs.method = method_code(mode);
        self.prefs_changed = true;
        self.step = HatchStep::Main;
        self.shape = None;
        self.reset_drawing();
        if mode == HatchMode::Manual {
            self.current_mode()
        } else {
            CmdResult::NeedPoint
        }
    }

    fn alignment_label(&self) -> &'static str {
        match self.prefs.alignment {
            1 => "Inside",
            2 => "Outside",
            _ => "Center",
        }
    }

    fn current_mode_text(&self) -> String {
        if self.prefs.path {
            crate::tr!(
                "hatch-cmd",
                "current-path",
                width = format!("{:.4}", self.prefs.width),
                alignment = self.alignment_label()
            )
        } else {
            crate::tr!("hatch-cmd", "current-mode")
        }
    }

    fn current_mode(&self) -> CmdResult {
        CmdResult::ReportMeasurement(self.current_mode_text())
    }

    /// The fill a new hatch starts with: the setting chosen, else the
    /// selected hatch's, else HPNAME.
    fn current_fill(&self) -> HatchFill {
        if let Some(fill) = &self.fill {
            return fill.clone();
        }
        let name = match &self.inherited {
            Some((model, _, _)) => model.name.clone(),
            None => hpname().trim_start_matches('_').to_string(),
        };
        if name.eq_ignore_ascii_case("SOLID") {
            HatchFill::Solid
        } else {
            HatchFill::Pattern(name)
        }
    }

    /// The settings the Hatch panel shows.
    fn current_settings(&self) -> HatchSettings {
        let inherited = self.inherited.as_ref().map(|(model, _, _)| model);
        HatchSettings {
            fill: self.current_fill(),
            angle_deg: f64::from(
                self.angle_override
                    .or(inherited.map(|model| model.angle_offset))
                    .unwrap_or(0.0),
            )
            .to_degrees(),
            scale: f64::from(
                self.scale_override
                    .or(inherited.map(|model| model.scale))
                    .unwrap_or(1.0),
            ),
            origin: self.default_origin,
            method: method_code(self.mode),
            islands: self.island_style,
            associative: self.associative,
            separate: self.separate_hatches,
            keep_boundaries: self.retain_boundaries,
            gap_tolerance: self.gap_tolerance,
            color: self.color.clone(),
            background: self.background.clone(),
            layer: self.layer.clone(),
            transparency: self.transparency,
            gradient_colors: self.gradient_colors.clone(),
            draw_order: self.draw_order,
            annotative: self.annotative,
        }
    }

    /// Take what the Hatch panel changed; keeping boundaries and separate
    /// hatches exclude each other as at the prompt.
    fn apply_settings(&mut self, settings: HatchSettings) {
        let now = self.current_settings();
        if settings.fill != now.fill {
            if matches!(settings.fill, HatchFill::Solid | HatchFill::Pattern(_)) && !self.gradient {
                let (name, _) = fill_pattern(&settings.fill, 0.0, &self.gradient_colors);
                set_hpname(&name);
            }
            self.fill = Some(settings.fill.clone());
        }
        if settings.angle_deg != now.angle_deg && settings.angle_deg.is_finite() {
            self.angle_override = Some((settings.angle_deg as f32).to_radians());
        }
        if settings.scale != now.scale && settings.scale > 0.0 {
            self.scale_override = Some(settings.scale as f32);
        }
        self.default_origin = settings.origin;
        if settings.method != now.method {
            let _ = self.choose(match settings.method {
                1 => HatchMode::SelectObjects,
                2 => HatchMode::Manual,
                _ => HatchMode::PickInside,
            });
        }
        self.island_style = settings.islands;
        self.associative = settings.associative;
        if settings.separate != now.separate {
            self.separate_hatches = settings.separate;
            self.retain_boundaries &= !settings.separate;
        }
        if settings.keep_boundaries != now.keep_boundaries {
            self.retain_boundaries = settings.keep_boundaries;
            self.separate_hatches &= !settings.keep_boundaries;
        }
        self.gap_tolerance = settings.gap_tolerance.clamp(0.0, MAX_GAP_TOLERANCE);
        self.color = settings.color;
        self.background = settings.background;
        self.layer = settings.layer;
        self.transparency = settings.transparency;
        self.gradient_colors = settings.gradient_colors;
        self.draw_order = settings.draw_order.min(4);
        self.annotative = settings.annotative;
    }

    fn reset_drawing(&mut self) {
        self.manual_pts.clear();
        self.manual_bulges.clear();
        self.manual_arc_mode = false;
        self.arc_sub = Sub::None;
    }

    fn drawn_polyline(&self, closed: bool) -> kernel::geom2d::Polyline {
        kernel::geom2d::Polyline {
            vertices: self
                .manual_pts
                .iter()
                .enumerate()
                .map(|(index, point)| kernel::geom2d::PolylineVertex {
                    position: [point.x, point.y],
                    bulge: self.manual_bulges.get(index).copied().unwrap_or(0.0),
                })
                .collect(),
            closed,
        }
    }

    /// The drawn boundary: points and the bulge of the segment leaving each.
    fn drawn_loop(&self) -> (Vec<[f64; 2]>, Vec<f64>) {
        let points = self.manual_pts.iter().map(|p| [p.x, p.y]).collect();
        let mut bulges = self.manual_bulges.clone();
        bulges.resize(self.manual_pts.len(), 0.0);
        (points, bulges)
    }

    /// The drawn boundary with its arcs tessellated.
    fn drawn_ring(&self) -> Vec<[f64; 2]> {
        let (points, bulges) = self.drawn_loop();
        bulged_ring(&points, &bulges)
    }

    /// Path mode: the band the drawn path sweeps at the path width, on the
    /// alignment's side (Inside = left of the direction of travel). Its
    /// sides follow the path's lines and arcs; corners between lines are
    /// mitred and the ends are square at the path's ends.
    fn path_loops(&self, closed: bool) -> Vec<(Vec<[f64; 2]>, Vec<f64>)> {
        let width = self.prefs.width;
        let (points, mut bulges) = self.drawn_loop();
        if points.len() < 2 || width <= 0.0 {
            return Vec::new();
        }
        if !closed {
            bulges.pop();
        }
        let (left, right) = match self.prefs.alignment {
            1 => (width, 0.0),
            2 => (0.0, -width),
            _ => (width * 0.5, -width * 0.5),
        };
        let rails = (
            offset_rail(&points, &bulges, closed, left),
            offset_rail(&points, &bulges, closed, right),
        );
        let (Some(left), Some(right)) = rails else {
            // ponytail: a rail with a non-tangent arc joint falls back to
            // the kernel's tessellated band.
            return self
                .band_rings(closed)
                .into_iter()
                .map(|ring| {
                    let bulges = vec![0.0; ring.len()];
                    (ring, bulges)
                })
                .collect();
        };
        if closed {
            return vec![left, right];
        }
        // Open: up one side, square across the end, back down the other.
        let (mut points, mut out) = (left.0.clone(), left.1.clone());
        out.pop();
        out.push(0.0);
        let count = right.0.len();
        for k in (0..count).rev() {
            points.push(right.0[k]);
            out.push(if k == 0 { 0.0 } else { -right.1[k - 1] });
        }
        let mut ring: (Vec<[f64; 2]>, Vec<f64>) = (Vec::new(), Vec::new());
        for (point, bulge) in points.into_iter().zip(out) {
            if ring.0.last().is_some_and(|last: &[f64; 2]| same_point(*last, point)) {
                *ring.1.last_mut().unwrap() = bulge;
                continue;
            }
            ring.0.push(point);
            ring.1.push(bulge);
        }
        if ring.0.len() > 1 && same_point(ring.0[0], *ring.0.last().unwrap()) {
            ring.0.pop();
            ring.1.pop();
        }
        vec![ring]
    }

    /// The kernel's band of the drawn path, its arcs tessellated.
    fn band_rings(&self, closed: bool) -> Vec<Vec<[f64; 2]>> {
        let width = self.prefs.width;
        let mut source = self.drawn_polyline(closed);
        if self.prefs.alignment != 0 {
            let [a, b] = [source.vertices[0].position, source.vertices[1].position];
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            if length <= 0.0 {
                return Vec::new();
            }
            let side = if self.prefs.alignment == 1 { 1.0 } else { -1.0 };
            let normal = [-(b[1] - a[1]) / length * side, (b[0] - a[0]) / length * side];
            let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let probe = [mid[0] + normal[0] * width, mid[1] + normal[1] * width];
            match kernel::geom2d::offset_polyline(&source, width * 0.5, probe)
                .into_iter()
                .next()
            {
                Some(offset) => source = offset,
                None => return Vec::new(),
            }
        }
        let segments = if source.closed {
            source.vertices.len()
        } else {
            source.vertices.len() - 1
        };
        let band = kernel::geom2d::polyline_band_boundary(
            &source,
            &vec![[width, width]; segments],
            kernel::tessellation::DEFAULT_ANGLE,
        );
        chain_loops(band.edges.into_iter().map(|edge| edge.points).collect())
    }

    /// Hatch the drawn boundary (closed, or Path's open path) and go back to
    /// the start prompt.
    fn finish_drawn(&mut self, closed: bool) -> CmdResult {
        // A rectangle drawn at an elevation is hatched in that plane.
        let plane = self.plane;
        self.plane.origin += self.plane.z.normalize_or(DVec3::Z) * self.elevation;
        self.elevation = 0.0;
        let loops = if self.prefs.path {
            self.path_loops(closed)
        } else {
            vec![self.drawn_loop()]
        };
        let hatch = (!loops.is_empty()).then(|| {
            let mut hatch = self.make_hatch(loops.iter().map(|(p, b)| bulged_ring(p, b)).collect());
            hatch.boundary_paths = Some(std::sync::Arc::new(
                loops
                    .iter()
                    .map(|(points, bulges)| {
                        let vertices: Vec<[f64; 3]> = points
                            .iter()
                            .enumerate()
                            .map(|(index, p)| [p[0], p[1], bulges.get(index).copied().unwrap_or(0.0)])
                            .collect();
                        crate::scene::drawn_hatch_path(&vertices)
                    })
                    .collect(),
            ));
            hatch
        });
        self.plane = plane;
        self.reset_drawing();
        let Some(hatch) = hatch else {
            return CmdResult::NeedPoint;
        };
        if self.gradient {
            set_hpname("SOLID");
        }
        self.step = HatchStep::Main;
        CmdResult::CommitHatchContinue {
            hatch,
            entity_style: self
                .inherited
                .as_ref()
                .map(|(_, color, transparency)| (color.clone(), *transparency)),
            message: self.current_mode_text(),
        }
    }

    /// The rectangle or circle a shape command finished becomes the boundary.
    fn finish_shape(&mut self, result: CmdResult) -> CmdResult {
        match result {
            CmdResult::CommitAndExit(entity) => {
                self.shape = None;
                match entity {
                    document::EntityType::Circle(circle) => {
                        let centre = DVec3::new(circle.center.x, circle.center.y, 0.0);
                        let offset = DVec3::new(circle.radius, 0.0, 0.0);
                        self.manual_pts = vec![centre + offset, centre - offset];
                        self.manual_bulges = vec![1.0, 1.0];
                    }
                    document::EntityType::LwPolyline(polyline) => {
                        self.elevation = polyline.elevation;
                        self.manual_pts = polyline
                            .vertices
                            .iter()
                            .map(|v| DVec3::new(v.location.x, v.location.y, 0.0))
                            .collect();
                        self.manual_bulges = polyline.vertices.iter().map(|v| v.bulge).collect();
                    }
                    _ => return CmdResult::NeedPoint,
                }
                self.finish_drawn(true)
            }
            CmdResult::Dispatch(name) => {
                let mut shape = match name.as_str() {
                    "CIRCLE_3P" => Shape::ThreePoint(super::circle::Circle3PCommand::new()),
                    "CIRCLE_2P" => Shape::TwoPoint(super::circle::Circle2PCommand::new()),
                    "CIRCLE_TTR" => Shape::Ttr(super::circle::CircleTTRCommand::new()),
                    _ => return CmdResult::NeedPoint,
                };
                shape.command_mut().set_working_plane(self.plane);
                self.shape = Some(shape);
                CmdResult::NeedPoint
            }
            // The shape gave up (Enter at its first step): back to the start.
            CmdResult::Cancel => {
                self.shape = None;
                CmdResult::NeedPoint
            }
            other => other,
        }
    }

    fn start_shape(&mut self, mut shape: Shape) -> CmdResult {
        shape.command_mut().set_working_plane(self.plane);
        let modes = match &shape {
            Shape::Rect(rect) => rect.modes_line(),
            _ => None,
        };
        self.shape = Some(shape);
        modes.map_or(CmdResult::NeedPoint, CmdResult::ReportMeasurement)
    }

    /// The direction the drawn path leaves its last point in: along the last
    /// line, or the last arc's end tangent; `None` before any segment.
    fn end_tangent(&self) -> Option<glam::Vec2> {
        let n = self.manual_pts.len();
        (n >= 2).then(|| {
            super::polyline::seg_exit_tangent(
                self.manual_pts[n - 2],
                self.manual_pts[n - 1],
                self.manual_bulges.get(n - 2).copied().unwrap_or(0.0),
            )
        })?
    }

    fn last_point2(&self) -> Option<glam::DVec2> {
        self.manual_pts.last().map(|p| glam::DVec2::new(p.x, p.y))
    }

    /// An arc from the last point to `end`, tangent to the path there.
    fn add_tangent_arc(&mut self, end: DVec3) {
        let Some(a) = self.last_point2() else {
            return;
        };
        let tangent = self.end_tangent().map_or(glam::DVec2::X, |t| t.as_dvec2());
        let bulge = super::polyline::compute_bulge(a, tangent, glam::DVec2::new(end.x, end.y));
        self.manual_bulges.push(bulge);
        self.manual_pts.push(end);
    }

    /// What an arc option step made of an input.
    fn arc_step(&mut self, step: ArcStep) -> CmdResult {
        match step {
            ArcStep::Next(sub) => self.arc_sub = sub,
            ArcStep::Arc(e, bulge) => {
                let z = self.manual_pts.last().map_or(0.0, |p| p.z);
                self.manual_bulges.push(bulge);
                self.manual_pts.push(DVec3::new(e.x, e.y, z));
                self.arc_sub = Sub::None;
            }
        }
        CmdResult::NeedPoint
    }

    /// The next point `distance` along the last segment (+X after the start).
    fn add_length(&mut self, distance: f64) -> CmdResult {
        self.step = HatchStep::Main;
        let Some(&last) = self.manual_pts.last() else {
            return CmdResult::NeedPoint;
        };
        // ponytail: an arc's chord direction, not its end tangent.
        let direction = match self.manual_pts.len() {
            n if n >= 2 => (last - self.manual_pts[n - 2]).normalize_or(DVec3::X),
            _ => DVec3::X,
        };
        self.manual_bulges.push(0.0);
        self.manual_pts.push(last + direction * distance);
        CmdResult::NeedPoint
    }

    fn undo_drawn_point(&mut self) -> CmdResult {
        if self.arc_sub != Sub::None {
            self.arc_sub = Sub::None;
            return CmdResult::NeedPoint;
        }
        if self.manual_pts.len() > 1 {
            self.manual_pts.pop();
            self.manual_bulges.pop();
            return CmdResult::NeedPoint;
        }
        CmdResult::ReportMeasurement(crate::tr!("hatch-cmd", "segments-undone"))
    }

    /// Enough points to finish: a closed area needs three, a path two.
    fn can_finish(&self) -> bool {
        self.manual_pts.len() >= if self.prefs.path { 2 } else { 3 }
    }

    fn combined_rings(&self) -> Vec<Vec<[f64; 2]>> {
        let mut rings = Vec::new();
        for ring in self
            .point_regions
            .iter()
            .chain(self.object_regions.iter())
            .flat_map(|region| region.iter())
        {
            if !rings.iter().any(|existing| existing == ring) {
                rings.push(ring.clone());
            }
        }
        rings
    }

    fn make_hatch(&self, rings: Vec<Vec<[f64; 2]>>) -> HatchModel {
        let perf_build = crate::perf::enabled();
        let build_started = perf_build.then(iced::time::Instant::now);
        let ring_count = rings.len();
        let input_vertices = if perf_build {
            rings.iter().map(Vec::len).sum::<usize>()
        } else {
            0
        };
        let world_rings: Vec<Vec<[f64; 2]>> = rings
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|&[x, y]| {
                        let point = self.plane.to_world(DVec3::new(x, y, 0.0));
                        [point.x, point.y]
                    })
                    .collect()
            })
            .collect();
        let (rel, origin, wcs) = pack_rings(&world_rings);
        let mut local_boundary = Vec::new();
        for (index, ring) in rings.iter().enumerate() {
            if index != 0 {
                local_boundary.push([f32::NAN, f32::NAN]);
            }
            local_boundary.extend(ring.iter().map(|&[x, y]| [x as f32, y as f32]));
        }
        let fill_plane = crate::scene::model::hatch_model::FillPlane {
            origin: self.plane.origin.to_array(),
            x_axis: self.plane.x.to_array(),
            y_axis: self.plane.y.to_array(),
        };
        let exterior: Vec<bool> = kernel::geom2d::ring_nesting_depths(&rings)
            .into_iter()
            .map(|depth| depth == 0)
            .collect();
        let near = crate::scene::sources_near_rings(&rings, &self.boundary_sources, 1.0e-6);
        let near_source_count = near.len();
        let mut boundary_sources: Vec<Vec<Handle>> = rings
            .iter()
            .map(|ring| crate::scene::ring_source_handles(ring, &near))
            .collect();
        let source_of = |index: usize| {
            if self.point_regions.iter().flatten().any(|ring| *ring == rings[index]) {
                crate::scene::HatchLoopSource::Picked
            } else {
                crate::scene::HatchLoopSource::Selected
            }
        };
        let mut boundary_paths = crate::scene::new_hatch_paths(
            &rings,
            source_of,
            &self.selected_objects,
            &near,
            1.0e-6,
        );

        if !self.associative {
            for handles in &mut boundary_sources {
                handles.clear();
            }
            for path in &mut boundary_paths {
                path.boundary_handles.clear();
            }
        }
        if let Some((source, _, _)) = &self.inherited {
            let (name, mut pattern) = match &self.fill {
                Some(fill) => fill_pattern(
                    fill,
                    self.angle_override.unwrap_or(0.0).to_degrees(),
                    &self.gradient_colors,
                ),
                None => (source.name.clone(), source.pattern.clone()),
            };
            let angle = self.angle_override.unwrap_or(source.angle_offset);
            let scale = self.scale_override.unwrap_or(source.scale).max(1.0e-6);
            if let HatchPattern::Pattern(families) = &mut pattern {
                let (sin, cos) = angle.sin_cos();
                for family in families {
                    let base_x = source.world_origin[0]
                        + (family.x0 as f64 * cos as f64 - family.y0 as f64 * sin as f64)
                            * scale as f64;
                    let base_y = source.world_origin[1]
                        + (family.x0 as f64 * sin as f64 + family.y0 as f64 * cos as f64)
                            * scale as f64;
                    let dx = base_x - origin[0];
                    let dy = base_y - origin[1];
                    family.x0 = ((dx * cos as f64 + dy * sin as f64) / scale as f64) as f32;
                    family.y0 = ((-dx * sin as f64 + dy * cos as f64) / scale as f64) as f32;
                }
            }
            let build_ms = crate::perf::elapsed_ms(build_started);
            if perf_build && build_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] hatch-build total={build_ms:.1}ms rings={ring_count} vertices={input_vertices} sources={} nearby_sources={near_source_count} inherited=true",
                    self.boundary_sources.len(),
                );
            }
            return HatchModel {
                pattern_origin: source.pattern_origin,
                render_instance: None,
                boundary: std::sync::Arc::new(rel),
                pattern,
                name,
                color: source.color,
                aci: source.aci,
                line_weight_px: source.line_weight_px,
                angle_offset: angle,
                scale,
                world_origin: origin,
                boundary_wcs: Some(std::sync::Arc::new(wcs)),
                fill_plane: Some(fill_plane),
                fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
                boundary_exterior: Some(std::sync::Arc::new(exterior)),
                boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
                boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
                style: self.island_style,
                draw_depth: source.draw_depth,
            };
        }
        let (name, mut pattern) =
            fill_pattern(
                &self.current_fill(),
                self.angle_override.unwrap_or(0.0).to_degrees(),
                &self.gradient_colors,
            );
        let angle = self.angle_override.unwrap_or(0.0);
        let scale = self.scale_override.unwrap_or(1.0).max(1.0e-6);
        if let (HatchPattern::Pattern(families), Some(anchor)) =
            (&mut pattern, local_boundary.first())
        {
            let (sin, cos) = (angle as f64).sin_cos();
            let dx = self.default_origin[0] - anchor[0] as f64;
            let dy = self.default_origin[1] - anchor[1] as f64;
            for family in families {
                family.x0 += ((dx * cos + dy * sin) / scale as f64) as f32;
                family.y0 += ((-dx * sin + dy * cos) / scale as f64) as f32;
            }
        }
        // GFCLR1 (blue) for a gradient fill, as the reference starts with.
        let color = if matches!(pattern, HatchPattern::Gradient { .. }) {
            color_rgba(&self.gradient_colors[0], 0.80)
        } else {
            [0.75, 0.75, 0.75, 0.85]
        };
        let build_ms = crate::perf::elapsed_ms(build_started);
        if perf_build && build_ms >= 1.0 {
            crate::perf_record!(
                "[perf] hatch-build total={build_ms:.1}ms rings={ring_count} vertices={input_vertices} sources={} nearby_sources={near_source_count} inherited=false",
                self.boundary_sources.len(),
            );
        }
        HatchModel {
            pattern_origin: Some(self.default_origin),
            render_instance: None,
            boundary: std::sync::Arc::new(rel),
            pattern,
            name,
            color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: self.angle_override.unwrap_or(0.0),
            scale: self.scale_override.unwrap_or(1.0).max(1.0e-6),
            world_origin: origin,
            boundary_wcs: Some(std::sync::Arc::new(wcs)),
            fill_plane: Some(fill_plane),
            fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
            boundary_exterior: Some(std::sync::Arc::new(exterior)),
            boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
            boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
            style: self.island_style,
            draw_depth: 0.0,
        }
    }
}

impl CadCommand for HatchCommand {
    fn name(&self) -> &'static str {
        if self.gradient {
            "GRADIENT"
        } else {
            "HATCH"
        }
    }

    fn prompt(&self) -> String {
        let text = if let Some(shape) = &self.shape {
            shape.prompt()
        } else {
            match self.step {
                HatchStep::DrawMode => crate::tr!(
                    "hatch-cmd",
                    "mode",
                    current = if self.prefs.path { "Path" } else { "Area" }
                ),
                HatchStep::Alignment => {
                    crate::tr!("hatch-cmd", "alignment", current = self.alignment_label())
                }
                HatchStep::Width => crate::tr!(
                    "hatch-cmd",
                    "width",
                    width = format!("{:.4}", self.prefs.width)
                ),
                HatchStep::Length => crate::tr!("hatch-cmd", "length"),
                HatchStep::Main => match self.mode {
                    HatchMode::PickInside => crate::tr!("hatch-cmd", "pick"),
                    HatchMode::SelectObjects => crate::tr!("hatch-cmd", "select"),
                    HatchMode::Manual if self.manual_pts.is_empty() => {
                        if self.prefs.path {
                            crate::tr!("hatch-cmd", "draw-start-path")
                        } else {
                            crate::tr!("hatch-cmd", "draw-start")
                        }
                    }
                    HatchMode::Manual if self.manual_arc_mode => {
                        if let Some(prompt) = self.arc_sub.arc_prompt(self.end_tangent()) {
                            prompt
                        } else if self.manual_pts.len() >= 2 {
                            crate::tr!("hatch-cmd", "arc-endpoint-close")
                        } else {
                            crate::tr!("hatch-cmd", "arc-endpoint")
                        }
                    }
                    HatchMode::Manual => {
                        let close = self.manual_pts.len() >= 3;
                        match (self.prefs.path, close) {
                            (false, false) => crate::tr!("hatch-cmd", "next"),
                            (false, true) => crate::tr!("hatch-cmd", "next-close"),
                            (true, false) => crate::tr!("hatch-cmd", "next-path"),
                            (true, true) => crate::tr!("hatch-cmd", "next-path-close"),
                        }
                    }
                },
            }
        };
        format!("{}  {text}", self.name())
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if let Some(shape) = &self.shape {
            let mut options = shape.command().options();
            options.retain(|option| option.keyword != "TTT");
            return options;
        }
        let accept = |options: &mut Vec<CmdOption>| {
            if self.region_count() > 0 {
                options.push(CmdOption::enter(t!("Accept").as_ref()));
            }
        };
        match self.step {
            HatchStep::DrawMode => vec![CmdOption::new("Area", "A"), CmdOption::new("Path", "P")],
            HatchStep::Alignment => vec![
                CmdOption::new("Center", "C"),
                CmdOption::new("Inside", "I"),
                CmdOption::new("Outside", "O"),
            ],
            HatchStep::Main => match self.mode {
                HatchMode::PickInside => {
                    let mut options = vec![
                        CmdOption::new("Select objects", "S"),
                        CmdOption::new("Draw", "D"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::new("seTtings", "T"),
                    ];
                    accept(&mut options);
                    options
                }
                HatchMode::SelectObjects => {
                    let mut options = vec![
                        CmdOption::new("picK internal point", "K"),
                        CmdOption::new("Draw", "D"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::new("seTtings", "T"),
                    ];
                    accept(&mut options);
                    options
                }
                HatchMode::Manual if self.manual_pts.is_empty() => {
                    let mut options = vec![
                        CmdOption::new("picK internal point", "K"),
                        CmdOption::new("Select objects", "S"),
                        CmdOption::new("Rectangle", "R"),
                        CmdOption::new("Circle", "C"),
                    ];
                    if self.prefs.path {
                        options.push(CmdOption::new("ALignment", "AL"));
                        options.push(CmdOption::new("Width", "W"));
                    }
                    options.push(CmdOption::new("Mode", "M"));
                    options.push(CmdOption::new("seTtings", "T"));
                    options
                }
                HatchMode::Manual if self.manual_arc_mode => {
                    if self.arc_sub != Sub::None {
                        return self.arc_sub.arc_options();
                    }
                    let mut options =
                        vec![CmdOption::new("Angle", "A"), CmdOption::new("CEnter", "CE")];
                    if self.manual_pts.len() >= 2 {
                        options.push(CmdOption::new("CLose", "CL"));
                    }
                    options.push(CmdOption::new("Direction", "D"));
                    options.push(CmdOption::new("Line", "L"));
                    options.push(CmdOption::new("Radius", "R"));
                    options.push(CmdOption::new("Second pt", "S"));
                    options.push(CmdOption::new("Undo", "U"));
                    options
                }
                HatchMode::Manual => {
                    let mut options = Vec::new();
                    if self.prefs.path {
                        options.push(CmdOption::new("ALignment", "AL"));
                        options.push(CmdOption::new("Width", "W"));
                    }
                    options.push(CmdOption::new("Arc", "A"));
                    if self.manual_pts.len() >= 3 {
                        options.push(CmdOption::new("Close", "C"));
                    }
                    options.push(CmdOption::new("Length", "L"));
                    options.push(CmdOption::new("Undo", "U"));
                    options.push(CmdOption::enter("Exit"));
                    options
                }
            },
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(shape) = &mut self.shape {
            if shape.wants_tangent() {
                return CmdResult::ReportMeasurement(crate::tr!("hatch-cmd", "ttr-requires-star"));
            }
            let result = shape.command_mut().on_point(pt);
            return self.finish_shape(result);
        }
        let pt = self.plane.to_local(pt);
        match self.step {
            HatchStep::DrawMode | HatchStep::Alignment => {
                return CmdResult::ReportError(t!("Invalid option keyword.").into_owned());
            }
            HatchStep::Width => return CmdResult::NeedPoint,
            HatchStep::Length => {
                let last = self.manual_pts.last().copied().unwrap_or(pt);
                return self.add_length(last.distance(pt));
            }
            HatchStep::Main => {}
        }
        match self.mode {
            HatchMode::PickInside => {
                let xy = [pt.x, pt.y];
                let faces = crate::scene::boundary_faces_around(
                    &self.boundary_sources,
                    xy,
                    self.gap_tolerance,
                );
                match picked_hatch_rings(&faces, xy) {
                    Some(rings) => {
                        self.add_point_region(rings);
                        let islands = crate::tr!("hatch-cmd", "analyzing-islands");
                        let report = if std::mem::replace(&mut self.analyzed, true) {
                            islands
                        } else {
                            [
                                crate::tr!("hatch-cmd", "selecting"),
                                crate::tr!("hatch-cmd", "selecting-visible"),
                                crate::tr!("hatch-cmd", "analyzing-data"),
                                islands,
                            ]
                            .join("\n")
                        };
                        CmdResult::ReportMeasurement(report)
                    }
                    // The search still reports its first steps, without islands.
                    None => CmdResult::HatchBoundaryMissed(if self.analyzed {
                        String::new()
                    } else {
                        [
                            crate::tr!("hatch-cmd", "selecting"),
                            crate::tr!("hatch-cmd", "selecting-visible"),
                            crate::tr!("hatch-cmd", "analyzing-data"),
                        ]
                        .join("
")
                    }),
                }
            }
            HatchMode::SelectObjects => CmdResult::NeedPoint,
            HatchMode::Manual => {
                if self.manual_pts.is_empty() || !self.manual_arc_mode {
                    if !self.manual_pts.is_empty() {
                        self.manual_bulges.push(0.0);
                    }
                    self.manual_pts.push(pt);
                } else if self.arc_sub != Sub::None {
                    let a = self.last_point2().unwrap_or_default();
                    if let Some(step) =
                        self.arc_sub.arc_point(glam::DVec2::new(pt.x, pt.y), a, self.end_tangent())
                    {
                        return self.arc_step(step);
                    }
                } else {
                    self.add_tangent_arc(pt);
                }
                CmdResult::NeedPoint
            }
        }
    }

    fn needs_tangent_pick(&self) -> bool {
        self.shape.as_ref().is_some_and(Shape::wants_tangent)
    }

    fn on_tangent_point(
        &mut self,
        obj: crate::command::TangentObject,
        hit: DVec3,
    ) -> CmdResult {
        match &mut self.shape {
            Some(shape) => {
                let result = shape.command_mut().on_tangent_point(obj, hit);
                self.finish_shape(result)
            }
            None => self.on_point(hit),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if let Some(shape) = &mut self.shape {
            if shape.wants_tangent() {
                return CmdResult::ReportMeasurement(crate::tr!("hatch-cmd", "ttr-requires"));
            }
            // Enter at a point the shape needs is no answer: it asks again.
            let result = match shape.command_mut().on_enter() {
                CmdResult::Cancel => CmdResult::ReportError(
                    t!("Point or option keyword required.").into_owned(),
                ),
                result => result,
            };
            return self.finish_shape(result);
        }
        match self.step {
            HatchStep::Alignment | HatchStep::Width | HatchStep::Length => {
                self.step = HatchStep::Main;
                return CmdResult::NeedPoint;
            }
            HatchStep::DrawMode => {
                self.step = HatchStep::Main;
                return self.current_mode();
            }
            HatchStep::Main => {}
        }
        if self.mode == HatchMode::Manual {
            if self.manual_arc_mode {
                // A chord direction defaults to the path's direction.
                if self.arc_sub.is_chord_step() {
                    let a = self.last_point2().unwrap_or_default();
                    let tangent = self.end_tangent();
                    let t = tangent.map_or(glam::DVec2::X, |t| t.as_dvec2());
                    let degrees = t.y.atan2(t.x).to_degrees();
                    return match self.arc_sub.arc_chord(degrees, a, tangent) {
                        Some(step) => self.arc_step(step),
                        None => CmdResult::NeedPoint,
                    };
                }
                if self.arc_sub != Sub::None {
                    self.arc_sub = Sub::None;
                    return CmdResult::NeedPoint;
                }
                self.manual_arc_mode = false;
                return CmdResult::NeedPoint;
            }
            if !self.manual_pts.is_empty() {
                if self.can_finish() {
                    return self.finish_drawn(false);
                }
                self.reset_drawing();
                return CmdResult::NeedPoint;
            }
        }
        let rings = self.combined_rings();
        if self.gradient && !rings.is_empty() {
            set_hpname("SOLID");
        }
        let entity_style = self
            .inherited
            .as_ref()
            .map(|(_, color, transparency)| (color.clone(), *transparency));
        if rings.is_empty() {
            CmdResult::Done
        } else if self.separate_hatches && !self.retain_boundaries {
            let hatches = self
                .point_regions
                .iter()
                .chain(self.object_regions.iter())
                .cloned()
                .map(|region| self.make_hatch(region))
                .collect();
            CmdResult::CommitHatches {
                hatches,
                entity_style,
            }
        } else if self.retain_boundaries {
            CmdResult::CommitHatchWithBoundaries {
                hatch: self.make_hatch(rings.clone()),
                boundaries: crate::scene::boundary_entities_from_sources(
                    &rings,
                    self.plane,
                    &self.boundary_sources,
                    1.0e-6,
                ),
                entity_style,
            }
        } else if let Some((color, transparency)) = entity_style {
            CmdResult::CommitStyledHatch {
                hatch: self.make_hatch(rings),
                color,
                transparency,
            }
        } else {
            CmdResult::CommitHatch(self.make_hatch(rings))
        }
    }

    fn wants_raw_point(&self) -> bool {
        self.shape.is_none() && self.mode == HatchMode::PickInside && self.step == HatchStep::Main
    }

    fn hatch_gap_tolerance(&self) -> Option<f64> {
        Some(self.gap_tolerance)
    }

    fn hatch_prefs(&self) -> Option<HatchPrefs> {
        self.prefs_changed.then_some(self.prefs)
    }

    fn point_step_accepts_keywords(&self) -> bool {
        match &self.shape {
            Some(shape) => shape.command().point_step_accepts_keywords(),
            None => self.step == HatchStep::Main && self.mode == HatchMode::Manual,
        }
    }

    fn hatch_settings(&self) -> Option<HatchSettings> {
        Some(self.current_settings())
    }

    fn set_hatch_settings(&mut self, settings: HatchSettings) {
        self.apply_settings(settings);
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if let Some(shape) = &self.shape {
            return shape.command().dyn_field();
        }
        if self.dyn_commit_as_text() {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        match &self.shape {
            Some(shape) => shape.command().dyn_commit_as_text(),
            None => matches!(self.step, HatchStep::Width | HatchStep::Length),
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        self.shape.as_ref().and_then(|shape| shape.command().dyn_spec())
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        self.shape
            .as_ref()
            .and_then(|shape| shape.command().dyn_live_value(cursor))
    }

    fn is_selection_gathering(&self) -> bool {
        self.shape.is_none() && self.mode == HatchMode::SelectObjects && self.step == HatchStep::Main
    }

    fn selection_forces_add(&self) -> bool {
        self.is_selection_gathering()
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if !self.is_selection_gathering() {
            return CmdResult::Cancel;
        }
        if self.set_object_selection(handles) {
            CmdResult::HatchBoundaryMissed(String::new())
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.step != HatchStep::Main || self.shape.is_some() {
            return None;
        }
        match self.mode {
            HatchMode::Manual if !self.manual_pts.is_empty() => Some(self.undo_drawn_point()),
            HatchMode::PickInside if self.point_regions.pop().is_some() => {
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn hatch_preview_models(&self) -> Option<Vec<HatchModel>> {
        let mut rings = self.combined_rings();
        if self.mode == HatchMode::Manual && self.shape.is_none() {
            if self.prefs.path {
                rings.extend(self.path_loops(false).iter().map(|(p, b)| bulged_ring(p, b)));
            } else if self.manual_pts.len() >= 3 {
                rings.push(self.drawn_ring());
            }
        }
        Some(if rings.is_empty() {
            Vec::new()
        } else {
            let mut preview = self.make_hatch(rings);
            preview.color = [0.15, 0.55, 1.0, 0.75];
            vec![preview]
        })
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim();
        let upper = input.to_ascii_uppercase();
        let point_or_keyword =
            || CmdResult::ReportError(t!("Point or option keyword required.").into_owned());
        let invalid_keyword = || CmdResult::ReportError(t!("Invalid option keyword.").into_owned());
        // A typed coordinate is a point for whichever step asks for one.
        let typed_point = input.contains(',') || input.starts_with('@') || input.starts_with('<');
        if let Some(shape) = &mut self.shape {
            if shape.wants_tangent() {
                return Some(CmdResult::ReportMeasurement(crate::tr!(
                    "hatch-cmd",
                    "ttr-requires-star"
                )));
            }
            if matches!(shape, Shape::Circle(_)) && upper == "TTT" {
                return Some(point_or_keyword());
            }
            let point_step = shape.command().dyn_field() == crate::command::DynField::Point;
            return match shape.command_mut().on_text_input(input) {
                Some(result) => Some(self.finish_shape(result)),
                None if typed_point => None,
                None if point_step => Some(point_or_keyword()),
                None => Some(CmdResult::ReportError(crate::tr!("hatch-cmd", "numeric"))),
            };
        }
        match self.step {
            HatchStep::DrawMode => {
                return Some(match upper.as_str() {
                    "A" | "AREA" | "P" | "PATH" => {
                        self.prefs.path = upper.starts_with('P');
                        self.prefs_changed = true;
                        self.step = HatchStep::Main;
                        self.current_mode()
                    }
                    _ => invalid_keyword(),
                });
            }
            HatchStep::Alignment => {
                return Some(match upper.as_str() {
                    "C" | "CENTER" | "I" | "INSIDE" | "O" | "OUTSIDE" => {
                        self.prefs.alignment = match &upper[..1] {
                            "I" => 1,
                            "O" => 2,
                            _ => 0,
                        };
                        self.prefs_changed = true;
                        self.step = HatchStep::Main;
                        CmdResult::NeedPoint
                    }
                    _ => invalid_keyword(),
                });
            }
            HatchStep::Width => {
                return match parse_number(input) {
                    Some(value) if value > 0.0 => {
                        self.prefs.width = value;
                        self.prefs_changed = true;
                        self.step = HatchStep::Main;
                        Some(CmdResult::NeedPoint)
                    }
                    Some(_) => Some(CmdResult::ReportError(crate::tr!("hatch-cmd", "positive"))),
                    None if typed_point => None,
                    None => Some(CmdResult::ReportError(crate::tr!("hatch-cmd", "numeric"))),
                };
            }
            HatchStep::Length => {
                return match parse_number(input) {
                    Some(value) => Some(self.add_length(value)),
                    None if typed_point => None,
                    None => Some(CmdResult::ReportError(crate::tr!("hatch-cmd", "numeric"))),
                };
            }
            HatchStep::Main => {}
        }
        let path_option = |keyword: &str| -> Option<HatchStep> {
            match keyword {
                "AL" | "ALIGNMENT" => Some(HatchStep::Alignment),
                "W" | "WIDTH" => Some(HatchStep::Width),
                _ => None,
            }
        };
        match self.mode {
            HatchMode::Manual if self.manual_pts.is_empty() => {
                if self.prefs.path {
                    if let Some(step) = path_option(upper.as_str()) {
                        self.step = step;
                        return Some(CmdResult::NeedPoint);
                    }
                }
                Some(match upper.as_str() {
                    "K" | "PICK" => self.choose(HatchMode::PickInside),
                    "S" | "SELECT" => self.choose(HatchMode::SelectObjects),
                    "R" | "RECTANGLE" => {
                        self.start_shape(Shape::Rect(super::shapes::RectCommand::for_hatch()))
                    }
                    "C" | "CIRCLE" => {
                        self.start_shape(Shape::Circle(super::circle::CircleCommand::new()))
                    }
                    "M" | "MODE" => {
                        self.step = HatchStep::DrawMode;
                        CmdResult::NeedPoint
                    }
                    "T" | "SETTINGS" => CmdResult::ShowHatchPanel,
                    _ if typed_point => return None,
                    _ => point_or_keyword(),
                })
            }
            HatchMode::Manual => {
                if self.prefs.path && !self.manual_arc_mode {
                    if let Some(step) = path_option(upper.as_str()) {
                        self.step = step;
                        return Some(CmdResult::NeedPoint);
                    }
                }
                if self.manual_arc_mode && self.arc_sub != Sub::None && upper != "U" {
                    let a = self.last_point2().unwrap_or_default();
                    return match self.arc_sub.arc_text(upper.as_str(), a, self.end_tangent()) {
                        Some(step) => Some(self.arc_step(step)),
                        None if typed_point => None,
                        None => Some(invalid_keyword()),
                    };
                }
                if self.manual_arc_mode && upper != "CL" {
                    if let Some(sub) = Sub::arc_option(upper.as_str()) {
                        self.arc_sub = sub;
                        return Some(CmdResult::NeedPoint);
                    }
                }
                match upper.as_str() {
                    "A" | "ARC" if !self.manual_arc_mode => {
                        self.manual_arc_mode = true;
                        self.arc_sub = Sub::None;
                        Some(CmdResult::NeedPoint)
                    }
                    "L" | "LINE" if self.manual_arc_mode => {
                        self.manual_arc_mode = false;
                        self.arc_sub = Sub::None;
                        Some(CmdResult::NeedPoint)
                    }
                    "CL" | "CLOSE" if self.manual_arc_mode && self.manual_pts.len() >= 2 => {
                        let first = self.manual_pts[0];
                        self.add_tangent_arc(first);
                        self.manual_pts.pop();
                        Some(self.finish_drawn(true))
                    }
                    "L" | "LENGTH" => {
                        self.step = HatchStep::Length;
                        Some(CmdResult::NeedPoint)
                    }
                    "C" | "CLOSE" if self.manual_pts.len() >= 3 && !self.manual_arc_mode => {
                        Some(self.finish_drawn(true))
                    }
                    "U" | "UNDO" => Some(self.undo_drawn_point()),
                    "E" | "EXIT" if !self.manual_arc_mode => Some(self.on_enter()),
                    _ if self.manual_arc_mode && !typed_point => Some(point_or_keyword()),
                    _ => None,
                }
            }
            HatchMode::PickInside | HatchMode::SelectObjects => {
                let picking = self.mode == HatchMode::PickInside;
                match upper.as_str() {
                    // The method already running is accepted too, as the
                    // reference takes picK at its own pick prompt.
                    "S" | "SELECT" => Some(self.choose(HatchMode::SelectObjects)),
                    "K" | "PICK" => Some(self.choose(HatchMode::PickInside)),
                    "D" | "DRAW" => Some(self.choose(HatchMode::Manual)),
                    "T" | "SETTINGS" => Some(CmdResult::ShowHatchPanel),
                    "U" | "UNDO" if picking => {
                        self.point_regions.pop();
                        Some(CmdResult::NeedPoint)
                    }
                    // Select mode's Undo takes back the last selection step.
                    _ => None,
                }
            }
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if let Some(shape) = &mut self.shape {
            return shape.command_mut().on_mouse_move(pt);
        }
        if self.mode != HatchMode::Manual || self.manual_pts.is_empty() {
            return None;
        }
        let world = |p: DVec3| self.plane.to_world(p).as_vec3().to_array();
        let mut points: Vec<[f32; 3]> = self.manual_pts.iter().map(|&p| world(p)).collect();
        points.push(pt.as_vec3().to_array());
        if !self.prefs.path {
            points.push(world(self.manual_pts[0]));
        }
        Some(WireModel::solid(
            "rubber_band".into(),
            points,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── BOUNDARY command ───────────────────────────────────────────────────────

pub struct BoundaryCommand {
    sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    active_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    restrict_sources: bool,
    point_regions: Vec<Vec<Vec<[f64; 2]>>>,
    selected_objects: Vec<Handle>,
    mode: BoundaryMode,
    island_style: BoundaryIslandStyle,
    gap_tolerance: f64,
    plane: WorkingPlane,
    missed: bool,
    output_region: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryMode {
    PickInside,
    Advanced,
    ObjectType,
    SelectObjects,
    GapTolerance { return_to_selection: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryIslandStyle {
    Normal,
    Outer,
    Ignore,
}

impl BoundaryCommand {
    pub fn new(
        sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        selected_objects: Vec<Handle>,
        plane: WorkingPlane,
        gap_tolerance: f64,
    ) -> Self {
        let mut command = Self {
            sources,
            active_sources: rustc_hash::FxHashMap::default(),
            restrict_sources: false,
            point_regions: Vec::new(),
            selected_objects: Vec::new(),
            mode: BoundaryMode::PickInside,
            island_style: BoundaryIslandStyle::Normal,
            gap_tolerance,
            plane,
            missed: false,
            output_region: false,
        };
        if !selected_objects.is_empty() {
            command.set_boundary_set(selected_objects);
        }
        command
    }

    fn set_boundary_set(&mut self, handles: Vec<Handle>) {
        let restrict_sources = !handles.is_empty();
        let selected_sources: rustc_hash::FxHashMap<_, _> = handles
            .iter()
            .filter_map(|handle| {
                self.sources
                    .get(handle)
                    .cloned()
                    .map(|source| (*handle, source))
            })
            .collect();
        self.active_sources = selected_sources;
        self.restrict_sources = restrict_sources;
        // Only a chosen boundary set is checked up front: it is small, while
        // the whole drawing is resolved around each pick instead (#1869).
        self.missed = restrict_sources
            && crate::scene::boundary_faces(&self.active_sources, self.gap_tolerance).is_empty();
        self.selected_objects = handles;
        self.point_regions.clear();
    }

    fn rebuild_faces(&mut self) {
        self.set_boundary_set(self.selected_objects.clone());
    }

    fn region_count(&self) -> usize {
        self.point_regions.len()
    }

    fn island_label(&self) -> &'static str {
        match self.island_style {
            BoundaryIslandStyle::Normal => "Normal",
            BoundaryIslandStyle::Outer => "Outer",
            BoundaryIslandStyle::Ignore => "Ignore",
        }
    }

    fn picked_region(&self, point: [f64; 2]) -> Option<Vec<Vec<[f64; 2]>>> {
        let sources = if self.restrict_sources {
            &self.active_sources
        } else {
            &self.sources
        };
        let outlines = crate::scene::boundary_faces_around(sources, point, self.gap_tolerance);
        let tolerance = Tolerance::new(1.0e-6);
        let curves = |outline: &Vec<[f64; 2]>| {
            outline
                .iter()
                .copied()
                .zip(outline.iter().copied().cycle().skip(1))
                .take(outline.len())
                .map(|(start, end)| Curve::Line(Line { start, end }))
                .collect::<Vec<_>>()
        };
        let mut containing: Vec<(usize, f64)> = outlines
            .iter()
            .enumerate()
            .filter(|(_, outline)| contains(&curves(outline), point, tolerance))
            .map(|(index, outline)| (index, signed_area(outline).abs()))
            .collect();
        containing.sort_by(|left, right| left.1.total_cmp(&right.1));
        let outer_index = containing.first()?.0;
        let outer = &outlines[outer_index];
        let outer_curves = curves(outer);
        let depths = ring_nesting_depths(&outlines);
        let outer_depth = depths.get(outer_index).copied().unwrap_or(0);
        let mut rings = vec![outer.clone()];
        if self.island_style == BoundaryIslandStyle::Ignore {
            return Some(rings);
        }
        for (index, candidate) in outlines.iter().enumerate() {
            let Some(seed) = candidate.first().copied() else {
                continue;
            };
            let candidate_curves = curves(candidate);
            let candidate_depth = depths.get(index).copied().unwrap_or(0);
            if index == outer_index
                || candidate_depth <= outer_depth
                || !contains(&outer_curves, seed, tolerance)
                || contains(&candidate_curves, point, tolerance)
            {
                continue;
            }
            if self.island_style == BoundaryIslandStyle::Outer && candidate_depth != outer_depth + 1
            {
                continue;
            }
            rings.push(candidate.clone());
        }
        Some(rings)
    }

    fn add_point_region(&mut self, region: Vec<Vec<[f64; 2]>>) {
        if !self
            .point_regions
            .iter()
            .any(|existing| existing == &region)
        {
            self.point_regions.push(region);
        }
    }

    fn make_entities(&self) -> Vec<document::EntityType> {
        let sources = if self.restrict_sources {
            &self.active_sources
        } else {
            &self.sources
        };
        if self.output_region {
            return self
                .point_regions
                .iter()
                .filter_map(|rings| {
                    crate::scene::model::presspull_model::boundary_region(
                        sources, rings, self.plane,
                    )
                })
                .collect();
        }
        crate::scene::boundary_polyline_entities(
            &self.point_regions,
            self.plane,
            sources,
            1.0e-6,
        )
    }
}

impl CadCommand for BoundaryCommand {
    fn name(&self) -> &'static str {
        "BOUNDARY"
    }

    fn prompt(&self) -> String {
        let miss = if self.missed {
            t!("  ⚠ No closed boundary found.").into_owned()
        } else {
            String::new()
        };
        match self.mode {
            BoundaryMode::PickInside => {
                format!("BOUNDARY  Specify internal point or [Advanced options]:{miss}")
            }
            BoundaryMode::Advanced => "BOUNDARY  Enter an option [Object type]:".into(),
            BoundaryMode::ObjectType => format!(
                "BOUNDARY  Enter type of boundary object [Region/Polyline] <{}>:",
                if self.output_region {
                    "Region"
                } else {
                    "Polyline"
                }
            ),
            BoundaryMode::SelectObjects => {
                t!("%{cmd}  Select objects:", cmd = self.name()).into_owned()
            }
            BoundaryMode::GapTolerance { .. } => {
                format!("BOUNDARY  {} <{}>:", t!("Gap tolerance"), self.gap_tolerance)
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.mode == BoundaryMode::Advanced {
            return vec![CmdOption::new("Object type", "O")];
        }
        if self.mode == BoundaryMode::ObjectType {
            return vec![
                CmdOption::new("Region", "R"),
                CmdOption::new("Polyline", "P"),
            ];
        }
        if matches!(self.mode, BoundaryMode::GapTolerance { .. }) {
            return Vec::new();
        }
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            return vec![CmdOption::enter(t!("Accept").as_ref())];
        }
        let island = format!(
            "{}: {}",
            t!("Island detection style"),
            t!(self.island_label())
        );
        let mut options = vec![
            CmdOption::new("Advanced options", "A"),
            CmdOption::new(t!("Boundary").as_ref(), "O"),
            CmdOption::new(&island, "S"),
            CmdOption::new(
                &format!("{}: {}", t!("Gap tolerance"), self.gap_tolerance),
                "G",
            ),
        ];
        if self.region_count() > 0 {
            options.push(CmdOption::enter("Accept"));
        }
        options
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if matches!(self.mode, BoundaryMode::PickInside) {
            let local = self.plane.to_local(pt);
            match self.picked_region([local.x, local.y]) {
                Some(region) => {
                    self.missed = false;
                    self.add_point_region(region);
                }
                None => self.missed = true,
            }
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.mode == BoundaryMode::ObjectType {
            self.mode = BoundaryMode::Advanced;
            return CmdResult::NeedPoint;
        }
        if self.mode == BoundaryMode::Advanced {
            self.mode = BoundaryMode::PickInside;
            return CmdResult::NeedPoint;
        }
        if let BoundaryMode::GapTolerance {
            return_to_selection,
        } = self.mode
        {
            self.mode = if return_to_selection {
                BoundaryMode::SelectObjects
            } else {
                BoundaryMode::PickInside
            };
            return CmdResult::NeedPoint;
        }
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            self.mode = BoundaryMode::PickInside;
            return CmdResult::NeedPoint;
        }
        let entities = self.make_entities();
        if entities.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::CommitEntitiesAndExit(entities)
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.mode, BoundaryMode::GapTolerance { .. }) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(self.mode, BoundaryMode::GapTolerance { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.mode == BoundaryMode::Advanced {
            if matches!(
                text.trim().to_ascii_uppercase().as_str(),
                "O" | "OBJECT" | "OBJECT TYPE"
            ) {
                self.mode = BoundaryMode::ObjectType;
            }
            return Some(CmdResult::NeedPoint);
        }
        if self.mode == BoundaryMode::ObjectType {
            match text.trim().to_ascii_uppercase().as_str() {
                "R" | "REGION" => self.output_region = true,
                "P" | "POLYLINE" => self.output_region = false,
                _ => return Some(CmdResult::NeedPoint),
            }
            self.mode = BoundaryMode::Advanced;
            return Some(CmdResult::NeedPoint);
        }
        if let BoundaryMode::GapTolerance {
            return_to_selection,
        } = self.mode
        {
            if let Some(value) = parse_gap_tolerance(text) {
                self.gap_tolerance = value;
                self.rebuild_faces();
            }
            self.mode = if return_to_selection {
                BoundaryMode::SelectObjects
            } else {
                BoundaryMode::PickInside
            };
            return Some(CmdResult::NeedPoint);
        }
        match text.trim().to_ascii_uppercase().as_str() {
            "A" | "ADVANCED" => self.mode = BoundaryMode::Advanced,
            "O" | "OBJECT" | "OBJECTS" => {
                self.mode = BoundaryMode::SelectObjects;
                self.missed = false;
            }
            "I" | "INTERNAL" | "POINTS" => {
                self.mode = BoundaryMode::PickInside;
                self.missed = false;
            }
            "S" | "ISLAND" | "ISLANDS" => {
                let style = match self.island_style {
                    BoundaryIslandStyle::Normal => BoundaryIslandStyle::Outer,
                    BoundaryIslandStyle::Outer => BoundaryIslandStyle::Ignore,
                    BoundaryIslandStyle::Ignore => BoundaryIslandStyle::Normal,
                };
                self.island_style = style;
                self.point_regions.clear();
            }
            "NORMAL" => {
                self.island_style = BoundaryIslandStyle::Normal;
                self.point_regions.clear();
            }
            "OUTER" => {
                self.island_style = BoundaryIslandStyle::Outer;
                self.point_regions.clear();
            }
            "IGNORE" => {
                self.island_style = BoundaryIslandStyle::Ignore;
                self.point_regions.clear();
            }
            "G" | "GAP" | "TOLERANCE" => {
                self.mode = BoundaryMode::GapTolerance {
                    return_to_selection: matches!(self.mode, BoundaryMode::SelectObjects),
                };
            }
            _ => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn wants_raw_point(&self) -> bool {
        self.mode == BoundaryMode::PickInside
    }

    fn hatch_gap_tolerance(&self) -> Option<f64> {
        Some(self.gap_tolerance)
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.mode, BoundaryMode::SelectObjects)
    }

    fn selection_forces_add(&self) -> bool {
        matches!(self.mode, BoundaryMode::SelectObjects)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            self.set_boundary_set(handles);
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if matches!(self.mode, BoundaryMode::PickInside) {
            self.point_regions.pop().map(|_| CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if !matches!(self.mode, BoundaryMode::PickInside) {
            return None;
        }
        let local = self.plane.to_local(pt);
        let hovered = self.picked_region([local.x, local.y]);
        let mut rings: Vec<&Vec<[f64; 2]>> = Vec::new();
        for ring in self.point_regions.iter().flat_map(|region| region.iter()) {
            if !rings.iter().any(|existing| *existing == ring) {
                rings.push(ring);
            }
        }
        if let Some(region) = &hovered {
            for ring in region {
                if !rings.iter().any(|existing| *existing == ring) {
                    rings.push(ring);
                }
            }
        }
        if rings.is_empty() {
            return None;
        }
        let mut points = Vec::new();
        for ring in rings {
            if !points.is_empty() {
                points.push([f64::NAN; 3]);
            }
            for [x, y] in ring {
                points.push(self.plane.to_world(DVec3::new(*x, *y, 0.0)).to_array());
            }
            if let Some([x, y]) = ring.first() {
                points.push(self.plane.to_world(DVec3::new(*x, *y, 0.0)).to_array());
            }
        }
        Some(WireModel::solid_f64(
            "boundary_preview".into(),
            points,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["BOUNDARY"]
}); // BoundaryCommand
inventory::submit!(crate::command::CommandRegistration {
    names: &["GRADIENT"]
}); // GradientCommand
inventory::submit!(crate::command::CommandRegistration { names: &["HATCH"] }); // HatchCommand

#[cfg(test)]
mod tests {
    use super::*;
    use document::EntityType;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    // Two nested rectangles, regardless of draw order, the resolution must be
    // deterministic and independent of which was drawn first.
    fn nested(draw_order: bool) -> Vec<Vec<[f64; 2]>> {
        let big = rect(-10.0, -10.0, 10.0, 10.0);
        let small = rect(-5.0, -5.0, 5.0, 5.0);
        if draw_order {
            vec![big, small]
        } else {
            vec![small, big]
        }
    }

    #[test]
    fn click_inside_small_hatches_only_small() {
        for order in [true, false] {
            let rings = resolve_hatch_rings(&nested(order), [0.0, 0.0]).unwrap();
            // Exactly one ring (no hole) and it is the small rectangle.
            assert_eq!(rings.len(), 1, "order {order}");
            assert_eq!(rings[0].len(), 4);
            assert!((rings[0][0][0] - (-5.0)).abs() < 1e-9, "order {order}");
        }
    }

    #[test]
    fn click_between_hatches_ring_with_hole() {
        for order in [true, false] {
            let rings = resolve_hatch_rings(&nested(order), [8.0, 0.0]).unwrap();
            // Outer ring + the small rectangle as a hole.
            assert_eq!(rings.len(), 2, "order {order}");
            // Outer is the big rectangle.
            assert!((rings[0][0][0] - (-10.0)).abs() < 1e-9, "order {order}");
            // Hole is the small rectangle.
            assert!((rings[1][0][0] - (-5.0)).abs() < 1e-9, "order {order}");
        }
    }

    #[test]
    fn click_outside_returns_none() {
        assert!(resolve_hatch_rings(&nested(true), [50.0, 50.0]).is_none());
    }

    #[test]
    fn hatch_pick_finds_region_and_island_among_far_clutter() {
        let mut sources = rustc_hash::FxHashMap::default();
        let mut add_ring = |base: u64, ring: &[[f64; 2]]| {
            for (index, (start, end)) in ring
                .iter()
                .copied()
                .zip(ring.iter().copied().cycle().skip(1))
                .take(ring.len())
                .enumerate()
            {
                let segment = Line { start, end };
                sources.insert(
                    Handle::new(base + index as u64),
                    crate::scene::BoundarySource {
                        segments: vec![segment],
                        curves: vec![Curve::Line(segment)],
                    },
                );
            }
        };
        add_ring(1, &rect(-10.0, -10.0, 10.0, 10.0));
        add_ring(10, &rect(-2.0, -2.0, 2.0, 2.0));
        for index in 0..64 {
            let x = 100.0 + index as f64 * 4.0;
            let segment = Line {
                start: [x, 100.0],
                end: [x + 1.0, 100.0],
            };
            sources.insert(
                Handle::new(100 + index),
                crate::scene::BoundarySource {
                    segments: vec![segment],
                    curves: Vec::new(),
                },
            );
        }

        let mut command = HatchCommand::new(sources, Vec::new(), None, WorkingPlane::default());
        command.on_point(DVec3::new(5.0, 5.0, 0.0));
        assert_eq!(command.point_regions.len(), 1);
        assert_eq!(command.point_regions[0].len(), 2, "outer ring plus island");
    }

    #[test]
    fn three_nested_levels() {
        let a = rect(-30.0, -30.0, 30.0, 30.0);
        let b = rect(-15.0, -15.0, 15.0, 15.0);
        let c = rect(-5.0, -5.0, 5.0, 5.0);
        // Click in the middle ring (between b and c).
        let rings = resolve_hatch_rings(&[a.clone(), b.clone(), c.clone()], [10.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 2, "middle ring fill with inner hole");
        // Click inside the innermost.
        let rings = resolve_hatch_rings(&[a, b, c], [0.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 1, "innermost fill has no hole");
    }

    #[test]
    fn click_outer_band_only_direct_child_is_hole() {
        let a = rect(-30.0, -30.0, 30.0, 30.0);
        let b = rect(-15.0, -15.0, 15.0, 15.0);
        let c = rect(-5.0, -5.0, 5.0, 5.0);
        // Click in the outermost band (between a and b): fill = a with only its
        // direct child b as a hole. The grandchild c must be excluded — adding
        // it would flip the innermost square back on under even-odd fill.
        let rings = resolve_hatch_rings(&[a, b, c], [20.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 2, "outer band = a with b as its only hole");
        assert!((rings[0][0][0] - (-30.0)).abs() < 1e-9, "outer ring is a");
        assert!(
            (rings[1][0][0] - (-15.0)).abs() < 1e-9,
            "hole is direct child b"
        );
    }

    #[test]
    fn boundary_region_keeps_a_selected_hole_in_one_entity() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0), rect(-2.0, -2.0, 2.0, 2.0)];
        let segments: Vec<Line> = rings
            .iter()
            .flat_map(|ring| {
                ring.iter()
                    .copied()
                    .zip(ring.iter().copied().cycle().skip(1))
                    .take(ring.len())
                    .map(|(start, end)| Line { start, end })
            })
            .collect();
        let mut sources = rustc_hash::FxHashMap::default();
        sources.insert(
            Handle::new(1),
            crate::scene::BoundarySource {
                curves: segments.iter().cloned().map(Curve::Line).collect(),
                segments,
            },
        );
        let mut command = BoundaryCommand::new(sources, Vec::new(), WorkingPlane::default(), 0.0);
        command.point_regions = vec![rings];
        command.output_region = true;

        let entities = command.make_entities();
        let [EntityType::Region(region)] = entities.as_slice() else {
            panic!("expected one region");
        };
        assert_eq!(region.wires.len(), 2);
    }

    #[test]
    fn hatch_panel_settings_reach_the_hatch() {
        let mut command = HatchCommand::new(
            Default::default(),
            Vec::new(),
            None,
            WorkingPlane::default(),
        );
        let mut settings = command.hatch_settings().unwrap();
        assert_eq!((settings.fill.clone(), settings.scale), (HatchFill::Solid, 1.0));
        settings.fill = HatchFill::Pattern("ANSI37".into());
        settings.scale = 2.5;
        settings.angle_deg = 30.0;
        settings.method = 2;
        settings.keep_boundaries = true;
        command.set_hatch_settings(settings);
        // Separate hatches turned on drops keeping boundaries, as at the prompt.
        let mut settings = command.hatch_settings().unwrap();
        settings.separate = true;
        command.set_hatch_settings(settings);
        let settings = command.hatch_settings().unwrap();
        assert_eq!(settings.method, 2);
        assert!(settings.separate && !settings.keep_boundaries);
        assert!((settings.angle_deg - 30.0).abs() < 1e-4);
        let hatch = command.make_hatch(vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]]);
        assert_eq!((hatch.name.as_str(), hatch.scale), ("ANSI37", 2.5));
    }
}

