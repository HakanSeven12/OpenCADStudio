// Offset tool — ribbon definition + interactive command.
//
// Command:  OFFSET (O)
//   OFFSET: Creates a parallel copy of an object (line, arc, circle,
//   or lwpolyline) at a specified distance on the chosen side.
//
//   Steps:
//     1. Text input: "Specify offset distance <last>:" → enter float or Enter for default
//     2. Pick object to offset (Line, Arc, Circle, LwPolyline)
//     3. Pick a point on the side to offset toward, or choose Multiple
//        to keep offsetting the newly created result at the same distance

use crate::entities::curve::{entity_curve, lwpolyline_world_xy};
use crate::modules::draw::modify::spline_ops::spline_sample_xy;
use acadrust::entities::LwVertex;
use acadrust::entities::{
    Arc as ArcEnt, Circle as CircleEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline,
    Spline as SplineEnt, XLine as XLineEnt,
};
use acadrust::{EntityType, Handle};
// Polyline offsetting, and the angle normalisation that goes with it, come
// from the kernel; only the entity conversion stays here.
use cadkernel::geom2d::nurbs::clamped_uniform_knots;
use cadkernel::geom2d::{
    offset_polyline, Polyline as KernelPolyline, PolylineVertex as KernelVertex,
};
use glam::{DVec3, Vec3};
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::modules::draw::defaults;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

use super::entity_index::ModifyEntityIndex;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "OFFSET",
        label: "Offset",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/offset.svg")),
        event: ModuleEvent::Command("OFFSET".to_string()),
    }
}

// ── Line offset ────────────────────────────────────────────────────────────

fn offset_line(l: &LineEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let dx = l.end.x - l.start.x;
    let dy = l.end.y - l.start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return None;
    }

    let nx = -dy / len; // left-perpendicular
    let ny = dx / len;

    let vx = side_pt.x as f64 - l.start.x;
    let vy = side_pt.y as f64 - l.start.y;
    let cross = dx * vy - dy * vx;
    let sign = if cross >= 0.0 { 1.0 } else { -1.0 };

    let ox = sign * nx * dist;
    let oy = sign * ny * dist;

    let mut new_l = l.clone();
    new_l.common.handle = Handle::NULL;
    new_l.start.x += ox;
    new_l.start.y += oy;
    new_l.end.x += ox;
    new_l.end.y += oy;
    Some(EntityType::Line(new_l))
}

// ── XLine (infinite construction line) offset ────────────────────────────────
// A parallel infinite line: same direction, base point shifted perpendicular by
// `dist` toward the side the cursor is on. (#296)

fn offset_xline(x: &XLineEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let dx = x.direction.x;
    let dy = x.direction.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return None;
    }

    let nx = -dy / len; // left-perpendicular
    let ny = dx / len;

    let vx = side_pt.x as f64 - x.base_point.x;
    let vy = side_pt.y as f64 - x.base_point.y;
    let cross = dx * vy - dy * vx;
    let sign = if cross >= 0.0 { 1.0 } else { -1.0 };

    let mut new_x = x.clone();
    new_x.common.handle = Handle::NULL;
    new_x.base_point.x += sign * nx * dist;
    new_x.base_point.y += sign * ny * dist;
    Some(EntityType::XLine(new_x))
}

// ── Circle offset ──────────────────────────────────────────────────────────

fn offset_circle(c: &CircleEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let plane = crate::entities::curve::circle_curve(c).plane;
    let [px, py] = plane.project(side_pt.as_dvec3().to_array())?;
    let dc = ((px - c.center.x).powi(2) + (py - c.center.y).powi(2)).sqrt();

    let new_r = if dc < c.radius {
        c.radius - dist
    } else {
        c.radius + dist
    };
    if new_r <= 1e-9 {
        return None;
    }

    let mut new_c = c.clone();
    new_c.common.handle = Handle::NULL;
    new_c.radius = new_r;
    Some(EntityType::Circle(new_c))
}

// ── Arc offset ─────────────────────────────────────────────────────────────

fn offset_arc(a: &ArcEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let plane = crate::entities::curve::arc_curve(a).plane;
    let [px, py] = plane.project(side_pt.as_dvec3().to_array())?;
    let dc = ((px - a.center.x).powi(2) + (py - a.center.y).powi(2)).sqrt();

    let new_r = if dc < a.radius {
        a.radius - dist
    } else {
        a.radius + dist
    };
    if new_r <= 1e-9 {
        return None;
    }

    let mut new_a = a.clone();
    new_a.common.handle = Handle::NULL;
    new_a.radius = new_r;
    Some(EntityType::Arc(new_a))
}

// ── LwPolyline offset ──────────────────────────────────────────────────────
//
// A raw exact line/arc offset can fold over itself at concave corners or when
// the distance is larger than a narrow part of the polyline.  The selected
// algorithm splits that raw curve at every intersection, rejects slices whose
// distance to the source is below the requested offset, and stitches the
// remaining slices.  This can legitimately return several disconnected
// polylines.

/// Convert an acad LWPOLYLINE to the line/arc representation used by the
/// topology pass. Coordinates are translated and divided by `dist`, so the
/// offset passed to the algorithm is always ±1. This avoids fixed-epsilon
/// failures on tiny drawings and on UTM-scale coordinates.
fn offset_lwpolylines(p: &LwPolyline, dist: f64, side_pt: Vec3) -> Vec<EntityType> {
    let Some(p) = lwpolyline_world_xy(p) else {
        return Vec::new();
    };
    let source = KernelPolyline {
        closed: p.is_closed,
        vertices: p
            .vertices
            .iter()
            .map(|v| KernelVertex {
                position: [v.location.x, v.location.y],
                bulge: if v.bulge.is_finite() { v.bulge } else { 0.0 },
            })
            .collect(),
    };

    offset_polyline(&source, dist, [side_pt.x as f64, side_pt.y as f64])
        .into_iter()
        .map(|result| {
            let mut new_polyline = p.clone();
            new_polyline.common.handle = Handle::NULL;
            new_polyline.is_closed = result.closed;
            new_polyline.vertices = result
                .vertices
                .iter()
                .map(|vertex| {
                    let mut output =
                        LwVertex::from_coords(vertex.position[0], vertex.position[1]);
                    output.bulge = vertex.bulge;
                    output
                })
                .collect();
            EntityType::LwPolyline(new_polyline)
        })
        .collect()
}

#[cfg(test)]
fn offset_lwpolyline(p: &LwPolyline, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    offset_lwpolylines(p, dist, side_pt).into_iter().next()
}

// ── Ellipse offset ─────────────────────────────────────────────────────────
//
// A true offset of an ellipse is a Lamé curve, not an ellipse. As an
// acceptable CAD approximation we scale both semi-axes uniformly and keep
// the same orientation, center and parameter range.  The sign of the offset
// is determined by whether side_pt is inside or outside the ellipse.

fn offset_ellipse(e: &EllipseEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
    if a < 1e-9 {
        return None;
    }
    let b = a * e.minor_axis_ratio;
    let nx = e.major_axis.x / a;
    let ny = e.major_axis.y / a;
    // Project side_pt onto ellipse local frame and test inside/outside.
    let rx = side_pt.x as f64 - e.center.x;
    let ry = side_pt.y as f64 - e.center.y;
    let xl = rx * nx + ry * ny;
    let yl = -rx * ny + ry * nx;
    let inside = (xl / a).powi(2) + (yl / b).powi(2) < 1.0;
    let sign = if inside { -1.0 } else { 1.0 };

    let new_a = a + sign * dist;
    let new_b = b + sign * dist;
    if new_a <= 1e-9 || new_b <= 1e-9 {
        return None;
    }

    let mut new_e = e.clone();
    new_e.common.handle = Handle::NULL;
    // Scale the major_axis vector proportionally.
    let scale = new_a / a;
    new_e.major_axis.x *= scale;
    new_e.major_axis.y *= scale;
    new_e.major_axis.z *= scale;
    new_e.minor_axis_ratio = new_b / new_a;
    Some(EntityType::Ellipse(new_e))
}

// ── Spline offset ──────────────────────────────────────────────────────────
//
// Strategy: sample the spline into N points, offset each sample point by
// `dist` along the local perpendicular (based on the finite-difference
// tangent), then fit a new spline through the offset points.

fn offset_spline(spl: &SplineEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let (ts_knot, pts) = spline_sample_xy(spl, 64);
    let n = pts.len();
    if n < 2 {
        return None;
    }

    // Determine offset sign from the first non-degenerate tangent.
    let sign: f64 = (0..n - 1).find_map(|i| {
        let dx = pts[i + 1][0] - pts[i][0];
        let dy = pts[i + 1][1] - pts[i][1];
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 {
            return None;
        }
        let vx = side_pt.x as f64 - pts[i][0];
        let vy = side_pt.y as f64 - pts[i][1];
        let cross = dx * vy - dy * vx;
        Some(if cross >= 0.0 { 1.0 } else { -1.0 })
    })?;

    // Offset each sample point along the local normal.
    let offset_pts: Vec<acadrust::types::Vector3> = pts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            // Tangent via central / forward / backward difference.
            let (dx, dy) = if i == 0 {
                let d = [pts[1][0] - pts[0][0], pts[1][1] - pts[0][1]];
                (d[0], d[1])
            } else if i == n - 1 {
                let d = [pts[n - 1][0] - pts[n - 2][0], pts[n - 1][1] - pts[n - 2][1]];
                (d[0], d[1])
            } else {
                (
                    (pts[i + 1][0] - pts[i - 1][0]) * 0.5,
                    (pts[i + 1][1] - pts[i - 1][1]) * 0.5,
                )
            };
            let len = (dx * dx + dy * dy).sqrt().max(1e-12);
            let nx = -dy / len; // left perpendicular
            let ny = dx / len;
            let z = spl.control_points.first().map(|v| v.z).unwrap_or(0.0);
            acadrust::types::Vector3::new(p[0] + sign * nx * dist, p[1] + sign * ny * dist, z)
        })
        .collect();

    let _ = ts_knot;
    // Build a new spline from the offset control points (treat sample pts as fit pts → ctrl pts).
    let degree = spl.degree.max(1) as usize;
    let new_ctrl: Vec<acadrust::types::Vector3> = offset_pts;
    let n_ctrl = new_ctrl.len();
    let mut new_spl = spl.clone();
    new_spl.common.handle = Handle::NULL;
    new_spl.control_points = new_ctrl;
    new_spl.knots = clamped_uniform_knots(degree, n_ctrl);
    new_spl.fit_points.clear();
    new_spl.weights.clear();
    Some(EntityType::Spline(new_spl))
}

// ── Dispatch ───────────────────────────────────────────────────────────────

fn compute_offsets(entity: &EntityType, dist: f64, side_pt: Vec3) -> Vec<EntityType> {
    match entity {
        EntityType::Line(l) => offset_line(l, dist, side_pt).into_iter().collect(),
        EntityType::Circle(c) => offset_circle(c, dist, side_pt).into_iter().collect(),
        EntityType::Arc(a) => offset_arc(a, dist, side_pt).into_iter().collect(),
        EntityType::LwPolyline(p) => offset_lwpolylines(p, dist, side_pt),
        EntityType::Ellipse(e) => offset_ellipse(e, dist, side_pt).into_iter().collect(),
        EntityType::Spline(s) => offset_spline(s, dist, side_pt).into_iter().collect(),
        EntityType::XLine(x) => offset_xline(x, dist, side_pt).into_iter().collect(),
        _ => Vec::new(),
    }
}

// ── Through-mode distance ─────────────────────────────────────────────────
//
// Nearest distance from the cursor to the entity outline, used by "through"
// mode so the offset copy passes through the cursor. Measured against the
// tessellated wire (point-to-segment), which approximates the perpendicular
// distance for every supported entity type.

fn perp_distance(entity: &EntityType, pt: Vec3) -> f64 {
    let pts = entity_wire_pts(entity);
    if pts.len() < 2 {
        return 0.0;
    }
    let px = pt.x as f64;
    let py = pt.y as f64;
    let mut best = f64::INFINITY;
    for w in pts.windows(2) {
        let ax = w[0][0] as f64;
        let ay = w[0][1] as f64;
        let bx = w[1][0] as f64;
        let by = w[1][1] as f64;
        let dx = bx - ax;
        let dy = by - ay;
        let len2 = dx * dx + dy * dy;
        let t = if len2 < 1e-12 {
            0.0
        } else {
            (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
        };
        let cx = ax + t * dx;
        let cy = ay + t * dy;
        let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
        if d < best {
            best = d;
        }
    }
    best
}

// ── Wire preview points ─────────────────────────────────────────────────────

/// Preview density, matching the figure the drawn wires use.
const PREVIEW_SEGMENTS_PER_RADIAN: f64 = cadkernel::geom2d::DEFAULT_SEGMENTS_PER_RADIAN;

/// Preview points for an entity, from its own curve.
///
/// One tessellation rather than a case per type. What that case-per-type
/// version had accumulated: an arc and a circle read their `center.x/.y` as
/// world coordinates when the entity stores them in its OCS, so an extruded
/// one previewed on the wrong side of the drawing; every curved kind was cut
/// at a fixed twenty segments per radian whatever its size; and an ellipse
/// rebuilt its own axis frame from the major axis' X and Y alone, dropping
/// the Z the entity actually stores.
///
/// The narrowing to `f32` stays here rather than in the kernel: a preview
/// wire leaves `points_low` empty and accepts the loss, while the resident
/// render path splits the `f64` into a high/low pair instead. Only a caller
/// knows which of the two it is.
fn entity_wire_pts(e: &EntityType) -> Vec<[f32; 3]> {
    // The two kinds with no finite point list of their own, so how far to run
    // them is a decision rather than a measurement. Long enough to read as
    // infinite at any working zoom; the committed entity renders true. A ray
    // runs one way only.
    const HL: f64 = 1.0e6;
    let unbounded = match e {
        EntityType::XLine(x) => Some((x.base_point, x.direction, -HL)),
        EntityType::Ray(r) => Some((r.base_point, r.direction, 0.0)),
        _ => None,
    };
    if let Some((base, direction, back)) = unbounded {
        let at = |k: f64| {
            [
                (base.x + direction.x * k) as f32,
                (base.y + direction.y * k) as f32,
                (base.z + direction.z * k) as f32,
            ]
        };
        return vec![at(back), at(HL)];
    }
    let Some(curve) = entity_curve(e) else {
        return vec![];
    };
    curve
        .tessellate(PREVIEW_SEGMENTS_PER_RADIAN)
        .into_iter()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect::<Vec<[f32; 3]>>()
}


// ── Command implementation ─────────────────────────────────────────────────

enum Step {
    /// Classic first step (#418): type the offset distance, press Enter /
    /// Space to accept the last one, or choose Through mode.
    Distance,
    /// Pick the object to offset. `locked == None` is "through" mode: the
    /// magnitude follows the cursor (perpendicular distance to the object).
    SelectObject { locked: Option<f64> },
    PickSide {
        /// The object(s) being offset — one from a pick, or the whole
        /// pre-selection when OFFSET starts with objects selected (#422).
        targets: Vec<EntityType>,
        locked: Option<f64>,
        /// Keep the side-pick step active and use each new result as the source
        /// for the next offset.
        multiple: bool,
    },
}

pub struct OffsetCommand {
    step: Step,
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
    /// Live entity supplied by the scene before an object-pick is handled.
    /// Unlike the command's opening snapshot, this also includes objects
    /// created by earlier offsets while the command remains active.
    picked: Option<EntityType>,
    /// Pre-selected offsettable objects (pick-first, #422); consumed when the
    /// distance step resolves.
    preselected: Vec<EntityType>,
}

/// The entity types `compute_offsets` can offset.
pub fn is_offsettable(e: &EntityType) -> bool {
    matches!(
        e,
        EntityType::Line(_)
            | EntityType::Circle(_)
            | EntityType::Arc(_)
            | EntityType::LwPolyline(_)
            | EntityType::Ellipse(_)
            | EntityType::Spline(_)
            | EntityType::XLine(_)
    )
}

impl OffsetCommand {
    pub fn new(all_entities: Vec<EntityType>) -> Self {
        let entity_index = ModifyEntityIndex::build(&all_entities);
        Self {
            step: Step::Distance,
            all_entities,
            entity_index,
            picked: None,
            preselected: Vec::new(),
        }
    }

    /// Pick-first flow (#422): the distance step still comes first, then the
    /// pre-selected objects go straight to the side step.
    pub fn with_selection(all_entities: Vec<EntityType>, targets: Vec<EntityType>) -> Self {
        let entity_index = ModifyEntityIndex::build(&all_entities);
        Self {
            step: Step::Distance,
            all_entities,
            entity_index,
            picked: None,
            preselected: targets,
        }
    }

    /// Leave the distance step with the given mode (Some = locked distance,
    /// None = through): pre-selected objects jump to the side step, otherwise
    /// the object-pick loop starts.
    fn advance_from_distance(&mut self, locked: Option<f64>) -> CmdResult {
        if self.preselected.is_empty() {
            self.step = Step::SelectObject { locked };
        } else {
            self.step = Step::PickSide {
                targets: std::mem::take(&mut self.preselected),
                locked,
                multiple: false,
            };
        }
        CmdResult::NeedPoint
    }
}

impl CadCommand for OffsetCommand {
    fn name(&self) -> &'static str {
        "OFFSET"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Distance => {
                let d = format!("{:.4}", defaults::get_offset_dist());
                t!("OFFSET  Specify offset distance or [Through] <%{d}>:", d = d).into_owned()
            }
            Step::SelectObject { .. } => {
                t!("OFFSET  Select object to offset (Enter to finish):").into_owned()
            }
            Step::PickSide {
                targets,
                locked,
                multiple,
                ..
            } => {
                let n: std::borrow::Cow<'_, str> = if targets.len() > 1 {
                    t!(" (%{count} objects)", count = targets.len())
                } else {
                    std::borrow::Cow::Borrowed("")
                };
                match (locked, multiple) {
                    (Some(d), false) => {
                        let d = format!("{:.4}", d);
                        t!(
                            "OFFSET%{n}  Click side or [Multiple]  [distance %{d}]:",
                            n = n,
                            d = d
                        )
                        .into_owned()
                    }
                    (Some(d), true) => {
                        let d = format!("{:.4}", d);
                        t!(
                            "OFFSET%{n} Multiple  Click next side [distance %{d}]:",
                            n = n,
                            d = d
                        )
                        .into_owned()
                    }
                    (None, false) => {
                        t!("OFFSET%{n}  Click through point or [Multiple]:", n = n).into_owned()
                    }
                    (None, true) => {
                        t!("OFFSET%{n} Multiple  Click next through point:", n = n).into_owned()
                    }
                }
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        match &self.step {
            Step::Distance => vec![
                crate::command::CmdOption::new(t!("Through").as_ref(), "T"),
                crate::command::CmdOption::enter(&format!(
                    "{:.4}",
                    defaults::get_offset_dist()
                )),
            ],
            Step::PickSide {
                multiple: false, ..
            } => vec![crate::command::CmdOption::new(t!("Multiple").as_ref(), "M")],
            _ => Vec::new(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::SelectObject { .. })
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        let locked = match &self.step {
            Step::SelectObject { locked } => *locked,
            _ => return CmdResult::NeedPoint,
        };
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }

        let entity = self.picked.take().or_else(|| {
            self.entity_index
                .get(&self.all_entities, handle)
                .cloned()
        });

        // Accept every type compute_offsets can offset — including XLine (#296),
        // and Ellipse/Spline whose offset functions existed but weren't reachable.
        match entity {
            Some(e) if is_offsettable(&e) => {
                self.step = Step::PickSide {
                    targets: vec![e],
                    locked,
                    multiple: false,
                };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    // The distance step takes a typed magnitude; the side step accepts one
    // too, re-locking the distance mid-command.
    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::Distance | Step::PickSide { .. })
    }

    fn dyn_field(&self) -> crate::command::DynField {
        match self.step {
            Step::Distance | Step::PickSide { .. } => crate::command::DynField::Scalar,
            _ => crate::command::DynField::Point,
        }
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        match &self.step {
            Step::Distance => Some(defaults::get_offset_dist()),
            Step::PickSide { targets, locked, .. } => Some(locked.unwrap_or_else(|| {
                targets
                    .first()
                    .map(|e| perp_distance(e, cursor.as_vec3()))
                    .unwrap_or(0.0)
            })),
            _ => None,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim().replace(',', ".");
        match &mut self.step {
            Step::Distance => {
                if t.eq_ignore_ascii_case("t") || t.eq_ignore_ascii_case("through") {
                    return Some(self.advance_from_distance(None));
                }
                if let Some(d) = crate::entities::common::parse_typed_length(&t) {
                    let d = d.abs().max(1e-9);
                    defaults::set_offset_dist(d);
                    return Some(self.advance_from_distance(Some(d)));
                }
                Some(CmdResult::NeedPoint)
            }
            Step::PickSide {
                locked,
                multiple,
                ..
            } => {
                if t.eq_ignore_ascii_case("m") || t.eq_ignore_ascii_case("multiple") {
                    *multiple = true;
                    return Some(CmdResult::NeedPoint);
                }
                if !t.is_empty() {
                    if let Some(d) = crate::entities::common::parse_typed_length(&t) {
                        let d = d.abs().max(1e-9);
                        defaults::set_offset_dist(d);
                        *locked = Some(d);
                    }
                }
                // Stay on the side step — the click chooses which side.
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_hover_entity(&mut self, handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        if handle.is_null() || !matches!(self.step, Step::SelectObject { .. }) {
            return vec![];
        }
        if let Some(entity) = self
            .entity_index.get(&self.all_entities, handle)
        {
            let pts = entity_wire_pts(entity);
            if !pts.is_empty() {
                return vec![WireModel::solid(
                    "offset_hover".into(),
                    pts,
                    WireModel::CYAN,
                    false,
                )];
            }
        }
        vec![]
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let (locked, targets, multiple) = match &self.step {
            Step::PickSide {
                locked,
                targets,
                multiple,
            } => (*locked, targets.clone(), *multiple),
            _ => return CmdResult::NeedPoint,
        };
        // Each target offsets by its own through-distance (or the locked
        // magnitude), toward the clicked side.
        let mut news: Vec<EntityType> = Vec::new();
        for entity in &targets {
            let mag = locked.unwrap_or_else(|| perp_distance(entity, pt.as_vec3()));
            if mag < 1e-9 {
                continue;
            }
            news.extend(compute_offsets(entity, mag, pt.as_vec3()));
        }
        if news.is_empty() {
            return CmdResult::NeedPoint;
        }
        if multiple {
            // Multiple mode chains from the result just created, matching
            // OFFSET's repeated fixed-distance behavior (parallel/concentric
            // series instead of duplicate copies at the first offset).
            self.step = Step::PickSide {
                targets: news.clone(),
                locked,
                multiple: true,
            };
            return if news.len() == 1 {
                CmdResult::CommitEntity(news.pop().unwrap())
            } else {
                CmdResult::CommitEntities(news)
            };
        }
        // Classic loop (#418): commit this offset and go back to the object
        // pick at the same distance, until Enter / Esc finishes.
        self.step = Step::SelectObject { locked };
        if news.len() == 1 {
            CmdResult::CommitEntity(news.pop().unwrap())
        } else {
            CmdResult::CommitEntities(news)
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let (locked, targets) = match &self.step {
            Step::PickSide { locked, targets, .. } => (*locked, targets.clone()),
            _ => return vec![],
        };
        let mut wires = Vec::new();
        for (target_index, entity) in targets.iter().enumerate() {
            let mag = locked.unwrap_or_else(|| perp_distance(entity, pt.as_vec3()));
            if mag < 1e-9 {
                continue;
            }
            for (result_index, result) in compute_offsets(entity, mag, pt.as_vec3())
                .into_iter()
                .enumerate()
            {
                let pts = entity_wire_pts(&result);
                if !pts.is_empty() {
                    wires.push(WireModel::solid(
                        format!("offset_preview_{target_index}_{result_index}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
            }
        }
        wires
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.step {
            // Enter / Space on the distance step accepts the last distance —
            // the "repeat the same value with just Space" flow (#418).
            Step::Distance => {
                let d = defaults::get_offset_dist();
                self.advance_from_distance(Some(d.abs().max(1e-9)))
            }
            _ => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["OFFSET"] });  // OffsetCommand

#[cfg(test)]
mod offset_tests {
    use super::*;
    use acadrust::types::Vector2;

    fn rect(corners: &[[f64; 2]]) -> LwPolyline {
        LwPolyline {
            vertices: corners
                .iter()
                .map(|&[x, y]| LwVertex::new(Vector2::new(x, y)))
                .collect(),
            is_closed: true,
            ..Default::default()
        }
    }

    /// Offset `corners` by 10 toward `side` and return the result's XY bounds.
    fn offset_bbox(corners: &[[f64; 2]], side: Vec3) -> [f64; 4] {
        let out = offset_lwpolyline(&rect(corners), 10.0, side);
        let Some(EntityType::LwPolyline(r)) = out else {
            panic!("offset did not return an lwpolyline");
        };
        let (mut minx, mut miny, mut maxx, mut maxy) =
            (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for v in &r.vertices {
            minx = minx.min(v.location.x);
            miny = miny.min(v.location.y);
            maxx = maxx.max(v.location.x);
            maxy = maxy.max(v.location.y);
        }
        [minx, miny, maxx, maxy]
    }

    fn approx(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    // A pick *outside* the loop must offset outward and a pick *inside* must
    // offset inward, regardless of the rectangle's winding. Regression for the
    // first-segment sign heuristic, which sent a CCW rectangle inward when the
    // outward pick sat beside it (issue 166).
    #[test]
    fn rect_offset_direction_is_winding_independent() {
        let ccw = [[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]];
        let cw = [[0.0, 0.0], [0.0, 60.0], [100.0, 60.0], [100.0, 0.0]];
        let out = [-10.0, -10.0, 110.0, 70.0];
        let inn = [10.0, 10.0, 90.0, 50.0];
        // Pick beside the rectangle (outside, mid-height) → outward, both windings.
        assert!(approx(offset_bbox(&ccw, Vec3::new(-10.0, 30.0, 0.0)), out));
        assert!(approx(offset_bbox(&cw, Vec3::new(-10.0, 30.0, 0.0)), out));
        // Pick inside → inward, both windings.
        assert!(approx(offset_bbox(&ccw, Vec3::new(50.0, 30.0, 0.0)), inn));
        assert!(approx(offset_bbox(&cw, Vec3::new(50.0, 30.0, 0.0)), inn));
        // Pick clearly outside below → outward (the case that worked before).
        assert!(approx(offset_bbox(&ccw, Vec3::new(50.0, -10.0, 0.0)), out));
    }
}
