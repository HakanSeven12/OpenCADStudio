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
//
//   With objects already selected, the command opens at step 3 instead, the
//   preview following the cursor. At step 3 a typed distance places the
//   offset at once on the cursor's side; a negative one, on the other side.

use crate::entities::curve::{entity_curve, lwpolyline_world_xy};
use codec::entities::LwVertex;
use codec::entities::{
    Arc as ArcEnt, Circle as CircleEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline,
    Spline as SplineEnt, XLine as XLineEnt,
};
use codec::{EntityType, Handle};
// Polyline offsetting, and the angle normalisation that goes with it, come
// from the kernel; only the entity conversion stays here.
use kernel::geom2d::{
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

// ── Ellipse and spline offset ──────────────────────────────────────────────
//
// Neither has a parallel of its own kind — an ellipse's is not an ellipse —
// so both come back as a spline fitted through true offset points, within a
// millionth of the curve's size, by the kernel. The work happens in the
// curve's own plane, so a tilted or extruded one offsets in place.

fn offset_ellipse(e: &EllipseEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let source = EntityType::Ellipse(e.clone());
    let mut template = SplineEnt::new();
    template.common = e.common.clone();
    offset_curved(&source, template, dist, side_pt)
}

fn offset_spline(spl: &SplineEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    offset_curved(&EntityType::Spline(spl.clone()), spl.clone(), dist, side_pt)
}

fn offset_curved(
    source: &EntityType,
    mut template: SplineEnt,
    dist: f64,
    side_pt: Vec3,
) -> Option<EntityType> {
    let planar = entity_curve(source)?;
    let side = planar
        .plane
        .project([side_pt.x as f64, side_pt.y as f64, side_pt.z as f64])?;
    // Left of the curve's direction of travel is the kernel's positive side.
    let curve = &planar.curve;
    let at = curve.parameter_at(side);
    let step = 1e-6;
    let (before, after) = if curve.is_closed() {
        (curve.point_at((at - step).rem_euclid(1.0)), curve.point_at((at + step).rem_euclid(1.0)))
    } else {
        (curve.point_at((at - step).max(0.0)), curve.point_at((at + step).min(1.0)))
    };
    let foot = curve.point_at(at);
    let along = [after[0] - before[0], after[1] - before[1]];
    let toward = [side[0] - foot[0], side[1] - foot[1]];
    let left = along[0] * toward[1] - along[1] * toward[0] > 0.0;
    let signed = if left { dist.abs() } else { -dist.abs() };
    let tolerance = (curve.length() * 1e-6).max(1e-9);
    let kernel::geom2d::Curve::Nurbs(parallel) =
        kernel::geom2d::offset_curve(curve, signed, tolerance)?
    else {
        return None;
    };
    template.common.handle = Handle::NULL;
    template.degree = parallel.degree() as i32;
    template.knots = parallel.knots().to_vec();
    template.control_points = parallel
        .control_points()
        .iter()
        .map(|uv| {
            let [x, y, z] = planar.plane.point_at(*uv);
            codec::types::Vector3::new(x, y, z)
        })
        .collect();
    template.weights = if parallel.is_rational() { parallel.weights().to_vec() } else { Vec::new() };
    template.flags.rational = parallel.is_rational();
    template.flags.periodic = false;
    template.flags.closed = curve.is_closed();
    if let Some([x, y, z]) = planar.plane.normal() {
        template.normal = codec::types::Vector3::new(x, y, z);
    }
    // A parallel is no longer the curve through the source's fit points.
    template.fit_points.clear();
    template.begin_tangent = codec::types::Vector3::new(0.0, 0.0, 0.0);
    template.end_tangent = codec::types::Vector3::new(0.0, 0.0, 0.0);
    Some(EntityType::Spline(template))
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
    closest_on_wire(entity, pt).map_or(0.0, |(_, d)| d)
}

/// Nearest point of the entity outline to `pt`, and its distance.
fn closest_on_wire(entity: &EntityType, pt: Vec3) -> Option<([f64; 2], f64)> {
    let pts = entity_wire_pts(entity);
    if pts.len() < 2 {
        return None;
    }
    let px = pt.x as f64;
    let py = pt.y as f64;
    let mut best = ([px, py], f64::INFINITY);
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
        if d < best.1 {
            best = ([cx, cy], d);
        }
    }
    Some(best)
}

// ── Opposite side ─────────────────────────────────────────────────────────
//
// A negative typed distance offsets away from the cursor. Rather than teach
// every offset function a sign, the cursor is mirrored through its nearest
// point on the outline, which puts it on the other side for every type: the
// other half-plane of a line, inside instead of outside a circle or closed
// loop. The mirror stops at half the offset distance so that, in a narrow
// part of a shape, it cannot cross the far wall and come back to the
// cursor's own side.

fn far_side(entity: &EntityType, pt: Vec3, dist: f64) -> Option<Vec3> {
    let ([qx, qy], d) = closest_on_wire(entity, pt)?;
    if d < 1e-9 {
        // On the outline itself: there is no side to be opposite to.
        return None;
    }
    let step = d.min(dist * 0.5) / d;
    Some(Vec3::new(
        (qx + (qx - pt.x as f64) * step) as f32,
        (qy + (qy - pt.y as f64) * step) as f32,
        pt.z,
    ))
}

// ── Wire preview points ─────────────────────────────────────────────────────

/// Preview density, matching the figure the drawn wires use.
const PREVIEW_SEGMENTS_PER_RADIAN: f64 = kernel::geom2d::DEFAULT_SEGMENTS_PER_RADIAN;

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
    /// Type the offset distance, accept the previous value with Enter,
    /// choose Through, or pick the first point of a reference distance.
    Distance,

    /// First reference point has been picked; the second point defines
    /// the offset distance.
    ReferenceSecond { first: DVec3 },

    SelectObject { locked: Option<f64> },

    PickSide {
        targets: Vec<EntityType>,
        locked: Option<f64>,
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
    /// Last cursor position the preview saw. A distance typed at the side
    /// step places the offset relative to it.
    cursor: Option<DVec3>,
    /// The command opened on pre-selected objects (#422) and has made nothing
    /// yet: Enter takes the last distance, and Erase / Layer, which the
    /// skipped distance step would have offered, are offered here.
    drag_start: bool,
    /// A Yes/No or Current/Source answer is being collected for this option.
    awaiting: Option<Await>,
    /// Erase option: replace the source object with its offset.
    erase_source: bool,
    /// Layer option: put offsets on the current layer instead of the source's.
    layer_current: bool,
    /// The side step's targets are document objects (not chained offsets), so
    /// Erase can replace them.
    targets_are_sources: bool,
    /// Offsets committed by this command that Undo can still take back.
    made: usize,
}

/// Which option prompt is waiting for an answer.
#[derive(Clone, Copy, PartialEq)]
enum Await {
    Erase,
    Layer,
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
            cursor: None,
            drag_start: false,
            awaiting: None,
            erase_source: false,
            layer_current: false,
            targets_are_sources: false,
            made: 0,
        }
    }

    /// Pick-first flow (#422): the objects are already chosen, so the command
    /// opens at the side step with the preview following the cursor. Clicking
    /// offsets through the cursor; typing a distance places it at that
    /// distance; Enter repeats the last distance (#418).
    pub fn with_selection(all_entities: Vec<EntityType>, targets: Vec<EntityType>) -> Self {
        let mut cmd = Self::new(all_entities);
        if !targets.is_empty() {
            cmd.step = Step::PickSide {
                targets,
                locked: None,
                multiple: false,
            };
            cmd.targets_are_sources = true;
            cmd.drag_start = true;
        }
        cmd
    }

    /// Leave the distance step with the given mode (Some = locked distance,
    /// None = through) for the object-pick loop.
    fn advance_from_distance(&mut self, locked: Option<f64>) -> CmdResult {
        self.step = Step::SelectObject { locked };
        CmdResult::NeedPoint
    }

    /// Undo option: take back the last offset and return to the object pick.
    fn undo_last(&mut self) -> Option<CmdResult> {
        if self.made == 0 {
            return None;
        }
        self.made -= 1;
        let locked = match &self.step {
            Step::SelectObject { locked } | Step::PickSide { locked, .. } => *locked,
            _ => return None,
        };
        self.step = Step::SelectObject { locked };
        Some(CmdResult::UndoDocument)
    }
    fn accept_distance(&mut self, distance: f64) -> CmdResult {
        let distance = distance.abs().max(1e-9);

        defaults::set_offset_dist(distance);
        self.advance_from_distance(Some(distance));

        let d = format!("{:.4}", distance);

        CmdResult::ReportMeasurement(
            t!("OFFSET distance = %{d}", d = d).into_owned()
        )
    }

    /// Offset the side step's targets with `pt` choosing the side: toward it,
    /// or away from it when `flip` is set. Each target offsets by its own
    /// through-distance, or by the locked magnitude.
    fn place(&mut self, pt: DVec3, flip: bool) -> CmdResult {
        let (locked, targets, multiple) = match &self.step {
            Step::PickSide {
                locked,
                targets,
                multiple,
            } => (*locked, targets.clone(), *multiple),

            _ => return CmdResult::NeedPoint,
        };
        let per_target: Vec<(Handle, Vec<EntityType>)> = targets
            .iter()
            .filter_map(|entity| {
                let mag = locked.unwrap_or_else(|| perp_distance(entity, pt.as_vec3()));
                if mag < 1e-9 {
                    return None;
                }
                let side = if flip {
                    far_side(entity, pt.as_vec3(), mag)?
                } else {
                    pt.as_vec3()
                };
                let offsets = compute_offsets(entity, mag, side);
                (!offsets.is_empty()).then(|| (entity.common().handle, offsets))
            })
            .collect();
        let mut news: Vec<EntityType> = per_target
            .iter()
            .flat_map(|(_, offsets)| offsets.iter().cloned())
            .collect();
        if news.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.made += 1;
        self.drag_start = false;
        // Erase option: the offset replaces its source object. Only the
        // objects picked from the drawing qualify — a Multiple chain's
        // targets are the offsets just made, which are not replaced again.
        let erase = self.erase_source && self.targets_are_sources;
        self.targets_are_sources = false;
        if multiple {
            // Multiple mode chains from the result just created, matching
            // OFFSET's repeated fixed-distance behavior (parallel/concentric
            // series instead of duplicate copies at the first offset).
            self.step = Step::PickSide {
                targets: news.clone(),
                locked,
                multiple: true,
            };
        } else {
            // Classic loop (#418): commit this offset and go back to the object
            // pick at the same distance, until Enter / Esc finishes.
            self.step = Step::SelectObject { locked };
        }
        if erase {
            return CmdResult::ReplaceManyContinue(per_target);
        }
        if news.len() == 1 {
            CmdResult::CommitEntity(news.pop().unwrap())
        } else {
            CmdResult::CommitEntities(news)
        }
    }
}

impl CadCommand for OffsetCommand {
    fn preserve_commit_layer(&self) -> bool {
        !self.layer_current
    }
    fn name(&self) -> &'static str {
        "OFFSET"
    }

    fn prompt(&self) -> String {
        match self.awaiting {
            Some(Await::Erase) => {
                return t!("OFFSET  Erase source object after offsetting? [Yes/No] <No>:").into_owned()
            }
            Some(Await::Layer) => {
                return t!("OFFSET  Enter layer option for offset objects [Current/Source] <Source>:")
                    .into_owned()
            }
            None => {}
        }
        match &self.step {
            Step::Distance => {
                let d = format!("{:.4}", defaults::get_offset_dist());
                t!(
                    "OFFSET  Specify offset distance or first reference point [Through] <%{d}>:",
                    d = d
                )
                .into_owned()
            }

            Step::ReferenceSecond { .. } => {
                t!("OFFSET  Specify second reference point:").into_owned()
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
                if self.drag_start {
                    let d = format!("{:.4}", defaults::get_offset_dist());
                    return if *multiple {
                        t!(
                            "OFFSET%{n} Multiple  Click through point or type distance, negative for the other side [Erase/Layer] <%{d}>:",
                            n = n,
                            d = d
                        )
                    } else {
                        t!(
                            "OFFSET%{n}  Click through point or type distance, negative for the other side [Multiple/Erase/Layer] <%{d}>:",
                            n = n,
                            d = d
                        )
                    }
                    .into_owned();
                }
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
        use crate::command::CmdOption;
        match self.awaiting {
            Some(Await::Erase) => {
                return vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")]
            }
            Some(Await::Layer) => {
                return vec![CmdOption::new("Current", "C"), CmdOption::new("Source", "S")]
            }
            None => {}
        }
        match &self.step {
            Step::Distance => vec![
                CmdOption::new("Through", "T"),
                CmdOption::new("Erase", "E"),
                CmdOption::new("Layer", "L"),
                CmdOption::enter(&format!("{:.4}", defaults::get_offset_dist())),
            ],
            Step::SelectObject { .. } => {
                let mut opts = Vec::new();
                if self.made > 0 {
                    opts.push(CmdOption::new("Undo", "U"));
                }
                opts.push(CmdOption::enter("Exit"));
                opts
            }
            Step::PickSide { multiple, .. } if self.drag_start => {
                let mut opts = Vec::new();
                if !multiple {
                    opts.push(CmdOption::new("Multiple", "M"));
                }
                opts.push(CmdOption::new("Erase", "E"));
                opts.push(CmdOption::new("Layer", "L"));
                opts.push(CmdOption::enter(&format!("{:.4}", defaults::get_offset_dist())));
                opts
            }
            Step::PickSide { multiple, .. } => {
                let mut opts = Vec::new();
                if !multiple {
                    opts.push(CmdOption::new("Multiple", "M"));
                }
                if self.made > 0 {
                    opts.push(CmdOption::new("Undo", "U"));
                }
                opts.push(CmdOption::enter("Exit"));
                opts
            }
            Step::ReferenceSecond { .. } => Vec::new(),
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        self.undo_last()
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
                self.targets_are_sources = true;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    // The distance step takes a typed magnitude; the side step accepts one
    // too, re-locking the distance mid-command. The object pick takes Undo.
    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::Distance | Step::PickSide { .. })
    }

    fn dyn_field(&self) -> crate::command::DynField {
        match self.step {
            Step::Distance | Step::PickSide { .. } => crate::command::DynField::Scalar,

            Step::ReferenceSecond { .. }
            | Step::SelectObject { .. } => crate::command::DynField::Point,
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
        let upper = t.to_uppercase();
        match self.awaiting {
            Some(Await::Erase) => {
                match upper.as_str() {
                    "Y" | "YES" => self.erase_source = true,
                    "N" | "NO" | "" => self.erase_source = false,
                    _ => return Some(CmdResult::NeedPoint),
                }
                self.awaiting = None;
                return Some(CmdResult::NeedPoint);
            }
            Some(Await::Layer) => {
                match upper.as_str() {
                    "C" | "CURRENT" => self.layer_current = true,
                    "S" | "SOURCE" | "" => self.layer_current = false,
                    _ => return Some(CmdResult::NeedPoint),
                }
                self.awaiting = None;
                return Some(CmdResult::NeedPoint);
            }
            None => {}
        }
        if matches!(upper.as_str(), "U" | "UNDO")
            && matches!(self.step, Step::SelectObject { .. } | Step::PickSide { .. })
        {
            return self.undo_last();
        }
        // Erase / Layer belong to the distance step, or to the side step when
        // a pre-selection skipped the distance step.
        if matches!(self.step, Step::Distance) || self.drag_start {
            if upper == "E" || upper == "ERASE" {
                self.awaiting = Some(Await::Erase);
                return Some(CmdResult::NeedPoint);
            }
            if upper == "L" || upper == "LAYER" {
                self.awaiting = Some(Await::Layer);
                return Some(CmdResult::NeedPoint);
            }
        }
        match &mut self.step {
            Step::Distance => {
                if t.eq_ignore_ascii_case("t") || t.eq_ignore_ascii_case("through") {
                    return Some(self.advance_from_distance(None));
                }
                if let Some(d) = crate::entities::common::parse_typed_length(&t) {
                    return Some(self.accept_distance(d));
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
                let Some(d) = crate::entities::common::parse_typed_length(&t) else {
                    return Some(CmdResult::NeedPoint);
                };
                let mag = d.abs().max(1e-9);
                defaults::set_offset_dist(mag);
                *locked = Some(mag);
                // The cursor has already chosen the side, so the value places
                // the offset now: on the cursor's side, or the other side when
                // negative. Before the cursor has been seen, the click still
                // chooses.
                match self.cursor {
                    Some(cursor) => Some(self.place(cursor, d < 0.0)),
                    None => Some(CmdResult::NeedPoint),
                }
            }
            _ => None,
        }
    }

    fn on_entities_committed(&mut self, entities: &[EntityType]) {
        self.all_entities.extend(entities.iter().cloned());
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
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
        match &self.step {
            Step::Distance => {
                self.step = Step::ReferenceSecond { first: pt };
                return CmdResult::NeedPoint;
            }

            Step::ReferenceSecond { first } => {
                let distance = first.distance(pt);

                if distance <= 1e-9 {
                    return CmdResult::NeedPoint;
                }

                return self.accept_distance(distance);
            }

            _ => {}
        }

        self.place(pt, false)
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        self.cursor = Some(pt);
        if let Step::ReferenceSecond { first } = &self.step {
            return vec![WireModel::solid(
                "offset_reference_distance".into(),
                vec![
                    [
                        first.x as f32,
                        first.y as f32,
                        first.z as f32,
                    ],
                    [
                        pt.x as f32,
                        pt.y as f32,
                        pt.z as f32,
                    ],
                ],
                WireModel::CYAN,
                false,
            )];
        }

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
                self.accept_distance(d)
            }
            // Opened on a pre-selection, the same Enter offsets by the last
            // distance on the cursor's side.
            Step::PickSide { .. } if self.drag_start => {
                let Some(cursor) = self.cursor else {
                    return CmdResult::NeedPoint;
                };
                if let Step::PickSide { locked, .. } = &mut self.step {
                    *locked = Some(defaults::get_offset_dist());
                }
                self.place(cursor, false)
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
    use codec::types::Vector2;

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

#[cfg(test)]
mod option_tests {
    use super::*;

    fn line(x1: f64, y1: f64, x2: f64, y2: f64, handle: u64) -> EntityType {
        let mut line = codec::entities::Line::from_coords(x1, y1, 0.0, x2, y2, 0.0);
        line.common.handle = Handle::new(handle);
        EntityType::Line(line)
    }

    fn keywords(cmd: &OffsetCommand) -> Vec<String> {
        cmd.options().into_iter().map(|o| o.keyword).collect()
    }

    #[test]
    fn erase_replaces_the_source_object() {
        let mut cmd = OffsetCommand::new(vec![line(0.0, 0.0, 10.0, 0.0, 1)]);
        assert_eq!(keywords(&cmd), ["T", "E", "L", ""]);
        assert!(matches!(cmd.on_text_input("E"), Some(CmdResult::NeedPoint)));
        assert_eq!(keywords(&cmd), ["Y", "N"]);
        assert!(matches!(cmd.on_text_input("Y"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::ReportMeasurement(_))));
        assert!(matches!(cmd.on_entity_pick(Handle::new(1), DVec3::new(5.0, 0.0, 0.0)), CmdResult::NeedPoint));
        match cmd.on_point(DVec3::new(5.0, 5.0, 0.0)) {
            CmdResult::ReplaceManyContinue(replacements) => {
                assert_eq!(replacements.len(), 1);
                assert_eq!(replacements[0].0, Handle::new(1));
                assert_eq!(replacements[0].1.len(), 1);
            }
            _ => panic!("erase should replace the source with its offset"),
        }
        // Back at the object pick, with Undo now available.
        assert_eq!(keywords(&cmd), ["U", ""]);
        assert!(matches!(cmd.on_text_input("U"), Some(CmdResult::UndoDocument)));
        assert_eq!(keywords(&cmd), [""]);
    }

    #[test]
    fn layer_current_drops_source_layer_preservation() {
        let mut cmd = OffsetCommand::new(vec![line(0.0, 0.0, 10.0, 0.0, 1)]);
        assert!(cmd.preserve_commit_layer());
        cmd.on_text_input("L");
        assert_eq!(keywords(&cmd), ["C", "S"]);
        cmd.on_text_input("C");
        assert!(!cmd.preserve_commit_layer());
        assert!(matches!(cmd.step, Step::Distance));
        cmd.on_text_input("2");
        cmd.on_entity_pick(Handle::new(1), DVec3::new(5.0, 0.0, 0.0));
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 5.0, 0.0)), CmdResult::CommitEntity(_)));
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;
    use codec::types::Vector2;

    fn line_y0() -> EntityType {
        let mut line = codec::entities::Line::from_coords(0.0, 0.0, 0.0, 10.0, 0.0, 0.0);
        line.common.handle = Handle::new(1);
        EntityType::Line(line)
    }

    /// OFFSET opened on `target`, with the cursor hovering at `cursor`.
    fn dragging(target: EntityType, cursor: [f64; 2]) -> OffsetCommand {
        let mut cmd = OffsetCommand::with_selection(vec![target.clone()], vec![target]);
        cmd.on_preview_wires(DVec3::new(cursor[0], cursor[1], 0.0));
        cmd
    }

    fn committed_line_y(result: Option<CmdResult>) -> f64 {
        match result {
            Some(CmdResult::CommitEntity(EntityType::Line(l))) => l.start.y,
            _ => panic!("expected one committed line"),
        }
    }

    #[test]
    fn preselection_opens_on_the_live_preview() {
        let mut cmd = OffsetCommand::with_selection(vec![line_y0()], vec![line_y0()]);
        assert!(matches!(cmd.step, Step::PickSide { locked: None, .. }));
        assert_eq!(cmd.options().into_iter().map(|o| o.keyword).collect::<Vec<_>>(), ["M", "E", "L", ""]);
        // The preview passes through the cursor before anything is typed.
        assert_eq!(cmd.on_preview_wires(DVec3::new(5.0, 3.0, 0.0)).len(), 1);
        assert_eq!(cmd.dyn_live_value(DVec3::new(5.0, 3.0, 0.0)), Some(3.0));
        // A click still offsets through the cursor.
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 3.0, 0.0)), CmdResult::CommitEntity(_)));
    }

    #[test]
    fn typed_distance_places_on_the_cursor_side() {
        let mut cmd = dragging(line_y0(), [5.0, 3.0]);
        assert!((committed_line_y(cmd.on_text_input("2")) - 2.0).abs() < 1e-9);
        // The loop continues at the typed distance.
        assert!(matches!(cmd.step, Step::SelectObject { locked: Some(d) } if d == 2.0));
    }

    #[test]
    fn negative_distance_places_on_the_other_side() {
        let mut cmd = dragging(line_y0(), [5.0, 3.0]);
        assert!((committed_line_y(cmd.on_text_input("-2")) + 2.0).abs() < 1e-9);
        assert_eq!(defaults::get_offset_dist(), 2.0);
    }

    #[test]
    fn negative_distance_inside_a_circle_grows_it() {
        let circle = EntityType::Circle(CircleEnt {
            center: codec::types::Vector3::new(0.0, 0.0, 0.0),
            radius: 10.0,
            ..Default::default()
        });
        let mut cmd = dragging(circle.clone(), [3.0, 0.0]);
        match cmd.on_text_input("-1") {
            Some(CmdResult::CommitEntity(EntityType::Circle(c))) => assert!((c.radius - 11.0).abs() < 1e-9),
            _ => panic!("expected one committed circle"),
        }
        let mut cmd = dragging(circle, [3.0, 0.0]);
        match cmd.on_text_input("1") {
            Some(CmdResult::CommitEntity(EntityType::Circle(c))) => assert!((c.radius - 9.0).abs() < 1e-9),
            _ => panic!("expected one committed circle"),
        }
    }

    // A U whose arms sit 1 apart. From inside the left arm the nearest wall
    // faces the gap; mirroring the cursor its full 1.9 across would land in
    // the right arm, back inside the loop. -0.25 must still go outward.
    #[test]
    fn negative_distance_does_not_mirror_across_a_gap() {
        let u = EntityType::LwPolyline(LwPolyline {
            vertices: [
                [0.0, 0.0], [9.0, 0.0], [9.0, 10.0], [5.0, 10.0],
                [5.0, 2.0], [4.0, 2.0], [4.0, 10.0], [0.0, 10.0],
            ]
            .iter()
            .map(|&[x, y]| LwVertex::new(Vector2::new(x, y)))
            .collect(),
            is_closed: true,
            ..Default::default()
        });
        let mut cmd = dragging(u, [2.1, 6.0]);
        match cmd.on_text_input("-0.25") {
            Some(CmdResult::CommitEntity(EntityType::LwPolyline(p))) => {
                let min_x = p.vertices.iter().map(|v| v.location.x).fold(f64::INFINITY, f64::min);
                assert!((min_x + 0.25).abs() < 1e-6, "offset went inward: min x {min_x}");
            }
            _ => panic!("expected one committed polyline"),
        }
    }

    #[test]
    fn enter_repeats_the_last_distance_on_the_cursor_side() {
        defaults::set_offset_dist(4.0);
        let mut cmd = dragging(line_y0(), [5.0, -1.0]);
        match cmd.on_enter() {
            CmdResult::CommitEntity(EntityType::Line(l)) => assert!((l.start.y + 4.0).abs() < 1e-9),
            _ => panic!("expected one committed line"),
        }
        // After that, Enter finishes as before.
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn erase_is_offered_without_the_distance_step() {
        let mut cmd = dragging(line_y0(), [5.0, 3.0]);
        cmd.on_text_input("E");
        cmd.on_text_input("Y");
        assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::ReplaceManyContinue(_))));
    }

    #[test]
    fn without_preselection_the_distance_comes_first() {
        let cmd = OffsetCommand::new(vec![line_y0()]);
        assert!(matches!(cmd.step, Step::Distance));
    }
}
