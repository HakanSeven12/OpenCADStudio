// Polyline tool — ribbon definition + interactive command.
//
// Command:  PLINE (PL)
//   Each click adds a vertex. Keywords match commercial solutions:
//   Line mode:  Arc / Close / Halfwidth / Length / Undo / Width
//   Arc mode:   Angle / CEnter / CLose / Direction / Halfwidth / Line /
//               Radius / Second pt / Undo / Width
//   Enter = finish, Escape = finish as-is (a keyword sub-prompt is left
//   first), C / CL = close and finish.
//
// Arc mode: arcs are tangent-continuous with the preceding segment unless a
// keyword (Angle / CEnter / Direction / Radius / Second pt) defines them.
// Bulge is stored per vertex (segment i→i+1); positive = CCW, negative = CW.
// Widths are stored per segment as (start, end).

use acadrust::entities::LwVertex;
use acadrust::types::Vector2;
use acadrust::{EntityType, Handle, LwPolyline};
use glam::{DVec2, DVec3, Vec2, Vec3};
use crate::t;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::{TangentGeom, WireModel};

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "PLINE",
        label: "Polyline",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/polyline.svg")),
        event: ModuleEvent::Command("PLINE".to_string()),
    }
}

// ── Segment mode ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum SegMode {
    Line,
    Arc,
}

/// A keyword sub-step inside PLINE (the option prompts of commercial solutions). `None` is the
/// regular "specify next point" step for the current segment mode. Every
/// sub-step returns to `None` once it has produced a vertex, or when Undo /
/// Enter / Escape backs out of it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Sub {
    None,
    /// Width / Halfwidth: starting width, then ending width. `half` scales the
    /// prompt and the typed value by two.
    WidthStart { half: bool },
    WidthEnd { half: bool, start: f64 },
    /// Length: a line segment of the typed length along the last tangent.
    Length,
    /// Arc → Angle: typed included angle, then an endpoint [CEnter/Radius].
    ArcAngle,
    ArcAngleEnd { angle: f64 },
    ArcAngleCenter { angle: f64 },
    ArcAngleRadius { angle: f64 },
    ArcAngleRadiusDir { angle: f64, r: f64 },
    /// Arc → CEnter: centre pick, then an endpoint [Angle/Length].
    ArcCenter,
    ArcCenterEnd { c: DVec2 },
    ArcCenterAngle { c: DVec2 },
    ArcCenterLength { c: DVec2 },
    /// Arc → Direction: a tangent-direction pick, then the endpoint.
    ArcDirection,
    ArcDirectionEnd { dir: DVec2 },
    /// Arc → Radius: typed radius, then an endpoint [Angle].
    ArcRadius,
    ArcRadiusEnd { r: f64 },
    ArcRadiusAngle { r: f64 },
    ArcRadiusAngleDir { r: f64, angle: f64 },
    /// Arc → Second pt: a point on the arc, then the endpoint (3-point arc).
    ArcSecond,
    ArcSecondEnd { s: DVec2 },
}

impl Sub {
    /// The step reads a single typed value (width, length, angle, radius).
    fn is_scalar(self) -> bool {
        matches!(
            self,
            Sub::WidthStart { .. }
                | Sub::WidthEnd { .. }
                | Sub::Length
                | Sub::ArcAngle
                | Sub::ArcAngleRadius { .. }
                | Sub::ArcCenterAngle { .. }
                | Sub::ArcCenterLength { .. }
                | Sub::ArcRadius
                | Sub::ArcRadiusAngle { .. }
        )
    }

    /// The typed value is an angle (degrees) rather than a length.
    fn is_angle(self) -> bool {
        matches!(
            self,
            Sub::ArcAngle | Sub::ArcCenterAngle { .. } | Sub::ArcRadiusAngle { .. }
        )
    }
}

// ── Command implementation ─────────────────────────────────────────────────

pub struct PlineCommand {
    vertices: Vec<DVec3>,
    /// Bulge for segment i → i+1 (one entry per vertex; last entry unused on commit).
    bulges: Vec<f64>,
    /// `(start, end)` width for segment i → i+1, parallel to `bulges`.
    widths: Vec<(f64, f64)>,
    /// Width pair applied to the next segment. After a segment lands its end
    /// width becomes the next start width, as in commercial solutions.
    cur_width: (f64, f64),
    mode: SegMode,
    sub: Sub,
    /// Unit direction of the last committed segment (for arc tangent continuity).
    last_tangent: Option<Vec2>,
    /// Handle of the live polyline entity once it exists (created at the 2nd
    /// vertex). `None` while only the start point is placed. Having it in the
    /// document makes the partial polyline snappable while later vertices are
    /// being placed. (#119)
    live_handle: Option<Handle>,
    plane: WorkingPlane,
}

impl PlineCommand {
    pub fn new() -> Self {
        let w = super::super::defaults::get_pline_width();
        Self {
            vertices: Vec::new(),
            bulges: Vec::new(),
            widths: Vec::new(),
            cur_width: (w, w),
            mode: SegMode::Line,
            sub: Sub::None,
            last_tangent: None,
            live_handle: None,
            plane: WorkingPlane::default(),
        }
    }

    /// Drop the last placed vertex but keep drawing (#352): the U option and
    /// mid-command Ctrl+Z both land here. Repeatable down to (and including)
    /// the start point.
    fn undo_last_vertex(&mut self) -> CmdResult {
        if self.vertices.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.vertices.pop();
        self.bulges.pop();
        if let Some((start, _)) = self.widths.pop() {
            // The undone segment's start width is the current one again.
            self.cur_width = (start, self.cur_width.1);
        }
        // Restore the exit tangent of the segment that is now last so a
        // following Arc segment stays tangent-continuous.
        let n = self.vertices.len();
        self.last_tangent = if n >= 2 {
            seg_exit_tangent(
                self.plane.to_local(self.vertices[n - 2]),
                self.plane.to_local(self.vertices[n - 1]),
                self.bulges[n - 2],
            )
        } else {
            None
        };
        match n {
            // Start point undone too — back to "Specify start point".
            0 => CmdResult::NeedPoint,
            // One vertex left: the live entity needs two, so it leaves the
            // document until the next point re-creates it.
            1 => match self.live_handle.take() {
                Some(h) => CmdResult::RemoveLiveEntity(h),
                None => CmdResult::NeedPoint,
            },
            _ => self.sync_live(false, false),
        }
    }

    /// Build the result that publishes the current vertices to the document:
    /// create the live entity at the 2nd vertex, then replace it in place as
    /// more vertices land. `finish` ends the command (used for close/done).
    fn sync_live(&self, closed: bool, finish: bool) -> CmdResult {
        let entity = self.build_entity(closed);
        match (entity, self.live_handle) {
            (Some(e), Some(handle)) => CmdResult::UpdateLiveEntity {
                handle,
                entity: e,
                finish,
            },
            (Some(e), None) => CmdResult::CommitLiveEntity(e),
            // Fewer than 2 vertices: nothing to publish.
            (None, _) => CmdResult::Cancel,
        }
    }

    fn build_entity(&self, closed: bool) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }
        let local: Vec<DVec3> = self
            .vertices
            .iter()
            .map(|vertex| self.plane.to_local(*vertex))
            .collect();
        let lw_verts: Vec<LwVertex> = local
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let mut lv = LwVertex::new(Vector2::new(v.x, v.y));
                lv.bulge = self.bulges.get(i).copied().unwrap_or(0.0);
                let (sw, ew) = self.widths.get(i).copied().unwrap_or((0.0, 0.0));
                lv.start_width = sw;
                lv.end_width = ew;
                lv
            })
            .collect();
        let pline = LwPolyline {
            vertices: lw_verts,
            elevation: local.first().map_or(0.0, |point| point.z),
            is_closed: closed,
            ..Default::default()
        };
        Some(self.plane.place_entity(EntityType::LwPolyline(pline)))
    }

    // ── Sub-step geometry (all in working-plane local 2-D) ─────────────────

    /// Last vertex in local 2-D plus its Z (every vertex shares the plane).
    fn last_local(&self) -> Option<(DVec2, f64)> {
        let last = self.plane.to_local(*self.vertices.last()?);
        Some((DVec2::new(last.x, last.y), last.z))
    }

    fn local2(&self, pt: DVec3) -> DVec2 {
        let p = self.plane.to_local(pt);
        DVec2::new(p.x, p.y)
    }

    fn tangent2(&self) -> DVec2 {
        self.last_tangent
            .map(|t| t.as_dvec2())
            .filter(|t| t.length_squared() > 1e-12)
            .unwrap_or(DVec2::X)
    }

    /// Sign for an arc whose direction is not fixed by its definition
    /// (CEnter / Radius endpoints, chord length): bend the way the previous
    /// segment was heading so the polyline flows on, else counter-clockwise.
    fn free_arc_sign(&self, a: DVec2, e: DVec2) -> f64 {
        let chord = e - a;
        let cross = match self.last_tangent {
            Some(t) => t.as_dvec2().perp_dot(chord),
            None => 1.0,
        };
        if cross < 0.0 {
            -1.0
        } else {
            1.0
        }
    }

    /// Bulge of the arc from `a` to `e` through `s` (3-point arc). `None` when
    /// the points are collinear or coincident.
    fn bulge_through(a: DVec2, s: DVec2, e: DVec2) -> Option<f64> {
        let d = 2.0 * (a.x * (s.y - e.y) + s.x * (e.y - a.y) + e.x * (a.y - s.y));
        if d.abs() < 1e-12 {
            return None;
        }
        let a2 = a.length_squared();
        let s2 = s.length_squared();
        let e2 = e.length_squared();
        let cx = (a2 * (s.y - e.y) + s2 * (e.y - a.y) + e2 * (a.y - s.y)) / d;
        let cy = (a2 * (e.x - s.x) + s2 * (a.x - e.x) + e2 * (s.x - a.x)) / d;
        let c = DVec2::new(cx, cy);
        let start = (a - c).y.atan2((a - c).x);
        let end = (e - c).y.atan2((e - c).x);
        // The arc bends towards `s`: with `s` right of the chord a → e the
        // arc runs counter-clockwise, left of it clockwise.
        let ccw = (e - a).perp_dot(s - a) < 0.0;
        let mut sweep = end - start;
        if ccw {
            if sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
        } else if sweep >= 0.0 {
            sweep -= std::f64::consts::TAU;
        }
        Some((sweep / 4.0).tan())
    }

    /// Endpoint of the arc about `c` starting at `a` with signed `sweep`.
    fn rotate_about(c: DVec2, a: DVec2, sweep: f64) -> DVec2 {
        let (sn, cs) = sweep.sin_cos();
        let v = a - c;
        c + DVec2::new(v.x * cs - v.y * sn, v.x * sn + v.y * cs)
    }

    /// Resolve the arc a sub-step describes for a candidate `cursor`/point:
    /// `(endpoint, bulge)`. `None` when the geometry is degenerate for that
    /// input (e.g. a chord longer than the diameter).
    fn arc_for(&self, sub: Sub, a: DVec2, p: DVec2) -> Option<(DVec2, f64)> {
        let sweep_ok = |s: f64| s.abs() > 1e-9 && s.abs() < std::f64::consts::TAU;
        match sub {
            Sub::ArcAngleEnd { angle } if sweep_ok(angle) => {
                if (p - a).length_squared() < 1e-12 {
                    return None;
                }
                Some((p, (angle / 4.0).tan()))
            }
            Sub::ArcAngleCenter { angle } if sweep_ok(angle) => {
                if (a - p).length_squared() < 1e-12 {
                    return None;
                }
                Some((Self::rotate_about(p, a, angle), (angle / 4.0).tan()))
            }
            Sub::ArcAngleRadiusDir { angle, r } if sweep_ok(angle) && r > 0.0 => {
                let dir = (p - a).try_normalize()?;
                let chord = 2.0 * r * (angle.abs() * 0.5).sin();
                Some((a + dir * chord, (angle / 4.0).tan()))
            }
            Sub::ArcCenterEnd { c } => {
                let r = (a - c).length();
                if r < 1e-9 {
                    return None;
                }
                let dir = (p - c).try_normalize()?;
                let e = c + dir * r;
                let start = (a - c).y.atan2((a - c).x);
                let end = (e - c).y.atan2((e - c).x);
                let sign = self.free_arc_sign(a, e);
                let mut sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                if sign < 0.0 {
                    sweep -= std::f64::consts::TAU;
                }
                if !sweep_ok(sweep) {
                    return None;
                }
                Some((e, (sweep / 4.0).tan()))
            }
            Sub::ArcCenterAngle { c } => {
                // `p` carries the typed angle in `x`.
                let angle = p.x;
                if (a - c).length_squared() < 1e-12 || !sweep_ok(angle) {
                    return None;
                }
                Some((Self::rotate_about(c, a, angle), (angle / 4.0).tan()))
            }
            Sub::ArcCenterLength { c } => {
                // `p` carries the typed chord length in `x`.
                let len = p.x;
                let r = (a - c).length();
                if r < 1e-9 || len <= 0.0 || len > 2.0 * r {
                    return None;
                }
                let sweep = 2.0 * (len / (2.0 * r)).asin();
                // Minor arc; bend on the side the previous segment favours.
                let e_ccw = Self::rotate_about(c, a, sweep);
                let sign = self.free_arc_sign(a, e_ccw);
                let sweep = sweep * sign;
                Some((Self::rotate_about(c, a, sweep), (sweep / 4.0).tan()))
            }
            Sub::ArcDirectionEnd { dir } => {
                if (p - a).length_squared() < 1e-12 {
                    return None;
                }
                Some((p, compute_bulge(a, dir, p)))
            }
            Sub::ArcRadiusEnd { r } if r > 0.0 => {
                let chord = (p - a).length();
                if chord < 1e-9 || chord > 2.0 * r {
                    return None;
                }
                let sweep = 2.0 * (chord / (2.0 * r)).asin() * self.free_arc_sign(a, p);
                Some((p, (sweep / 4.0).tan()))
            }
            Sub::ArcRadiusAngleDir { r, angle } if r > 0.0 && sweep_ok(angle) => {
                let dir = (p - a).try_normalize()?;
                let chord = 2.0 * r * (angle.abs() * 0.5).sin();
                Some((a + dir * chord, (angle / 4.0).tan()))
            }
            Sub::ArcSecondEnd { s } => {
                if (p - a).length_squared() < 1e-12 {
                    return None;
                }
                Some((p, Self::bulge_through(a, s, p)?))
            }
            _ => None,
        }
    }

    /// Land a segment ending at local `e` with `bulge`, publish it, and
    /// return to the regular next-point step.
    fn push_segment(&mut self, e: DVec2, z: f64, bulge: f64) -> CmdResult {
        let last_idx = self.vertices.len() - 1;
        let a = self.local2(self.vertices[last_idx]);
        self.bulges[last_idx] = bulge;
        self.last_tangent = seg_exit_tangent(
            DVec3::new(a.x, a.y, z),
            DVec3::new(e.x, e.y, z),
            bulge,
        );
        self.sub = Sub::None;
        self.push_vertex(self.plane.to_world(DVec3::new(e.x, e.y, z)))
    }

    /// Append a vertex (bulge/width of the segment *into* it are already set
    /// on the previous entry) and publish.
    fn push_vertex(&mut self, pt: DVec3) -> CmdResult {
        if !self.vertices.is_empty() {
            let last = self.widths.len() - 1;
            self.widths[last] = self.cur_width;
            // The end width flows into the next segment's start width.
            self.cur_width.0 = self.cur_width.1;
        }
        self.vertices.push(pt);
        self.bulges.push(0.0);
        self.widths.push(self.cur_width);
        // Publish to the document as soon as the polyline has a segment so it
        // becomes snappable while the rest is drawn. (#119)
        if self.vertices.len() >= 2 {
            self.sync_live(false, false)
        } else {
            CmdResult::NeedPoint
        }
    }

    /// A typed value for a scalar sub-step. Widths are lengths ≥ 0; the angle
    /// steps read degrees (positive = counter-clockwise); Length may be
    /// negative (draws backwards).
    fn on_scalar(&mut self, value: f64) -> Option<CmdResult> {
        if !value.is_finite() {
            return None;
        }
        let (a, z) = self.last_local()?;
        match self.sub {
            Sub::WidthStart { half } => {
                if value < 0.0 {
                    return None;
                }
                let start = if half { value * 2.0 } else { value };
                self.sub = Sub::WidthEnd { half, start };
                Some(CmdResult::NeedPoint)
            }
            Sub::WidthEnd { half, start } => {
                if value < 0.0 {
                    return None;
                }
                let end = if half { value * 2.0 } else { value };
                self.cur_width = (start, end);
                super::super::defaults::set_pline_width(end);
                self.sub = Sub::None;
                Some(CmdResult::NeedPoint)
            }
            Sub::Length => {
                if value.abs() < 1e-9 {
                    return None;
                }
                let e = a + self.tangent2() * value;
                Some(self.push_segment(e, z, 0.0))
            }
            Sub::ArcAngle => {
                let angle = value.to_radians();
                if angle.abs() < 1e-9 || angle.abs() >= std::f64::consts::TAU {
                    return None;
                }
                self.sub = Sub::ArcAngleEnd { angle };
                Some(CmdResult::NeedPoint)
            }
            Sub::ArcAngleRadius { angle } => {
                if value <= 0.0 {
                    return None;
                }
                self.sub = Sub::ArcAngleRadiusDir { angle, r: value };
                Some(CmdResult::NeedPoint)
            }
            Sub::ArcCenterAngle { c } => {
                let (e, bulge) =
                    self.arc_for(Sub::ArcCenterAngle { c }, a, DVec2::new(value.to_radians(), 0.0))?;
                Some(self.push_segment(e, z, bulge))
            }
            Sub::ArcCenterLength { c } => {
                let (e, bulge) = self.arc_for(Sub::ArcCenterLength { c }, a, DVec2::new(value, 0.0))?;
                Some(self.push_segment(e, z, bulge))
            }
            Sub::ArcRadius => {
                if value <= 0.0 {
                    return None;
                }
                self.sub = Sub::ArcRadiusEnd { r: value };
                Some(CmdResult::NeedPoint)
            }
            Sub::ArcRadiusAngle { r } => {
                let angle = value.to_radians();
                if angle.abs() < 1e-9 || angle.abs() >= std::f64::consts::TAU {
                    return None;
                }
                self.sub = Sub::ArcRadiusAngleDir { r, angle };
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    /// Leave the current sub-step without placing anything.
    fn leave_sub(&mut self) -> CmdResult {
        self.sub = Sub::None;
        CmdResult::NeedPoint
    }

    /// Rubber-band wire for the arc a → e with `bulge` (falls back to a line
    /// for a degenerate bulge).
    fn arc_wire(&self, a: DVec2, e: DVec2, z: f64, bulge: f64) -> WireModel {
        let last = Vec3::new(a.x as f32, a.y as f32, z as f32);
        let pt = Vec3::new(e.x as f32, e.y as f32, z as f32);
        let mut pts: Vec<[f32; 3]> = Vec::new();
        let mut tangent_geom = None;
        if bulge.abs() >= 1e-9 && (e - a).length_squared() >= 1e-12 {
            if let Some(arc) =
                crate::entities::common::BulgeArc::from_bulge([a.x, a.y], [e.x, e.y], bulge)
            {
                for s in arc.tessellate_angle(std::f64::consts::TAU / 64.0) {
                    pts.push([s[0] as f32, s[1] as f32, last.z]);
                }
                let (sa, ea) = if arc.sweep >= 0.0 {
                    (arc.start_angle, arc.end_angle)
                } else {
                    (arc.end_angle, arc.start_angle)
                };
                let world_center = self
                    .plane
                    .to_world(DVec3::new(arc.center[0], arc.center[1], z));
                if arc.radius > 0.0
                    && arc.radius.is_finite()
                    && arc.radius <= 1e6
                    && sa.is_finite()
                    && ea.is_finite()
                {
                    tangent_geom = Some(TangentGeom::Arc {
                        center: world_center.to_array(),
                        axis_x: self.plane.x.to_array(),
                        axis_y: self.plane.y.to_array(),
                        radius: arc.radius,
                        start_angle: sa,
                        end_angle: ea,
                    });
                }
            } else {
                pts.extend_from_slice(&arc_sample_points(last, bulge, pt, 64));
            }
        } else {
            pts.push([last.x, last.y, last.z]);
            pts.push([pt.x, pt.y, pt.z]);
        }
        let world_points = pts
            .into_iter()
            .map(|point| self.plane.to_world(Vec3::from_array(point).as_dvec3()).as_vec3().to_array())
            .collect();
        let mut wire = WireModel::solid("rubber_band".into(), world_points, WireModel::CYAN, false);
        if let Some(tg) = tangent_geom {
            wire.tangent_geoms.push(tg);
        }
        wire
    }

    fn line_wire(&self, a: DVec2, e: DVec2, z: f64) -> WireModel {
        let a = self.plane.to_world(DVec3::new(a.x, a.y, z)).as_vec3();
        let e = self.plane.to_world(DVec3::new(e.x, e.y, z)).as_vec3();
        WireModel::solid(
            "rubber_band".into(),
            vec![a.to_array(), e.to_array()],
            WireModel::CYAN,
            false,
        )
    }
}

// ── Arc geometry helpers ───────────────────────────────────────────────────

/// Compute the bulge for the arc from `a` to `b` that is tangent to `tangent` at `a`.
/// Returns 0.0 if the points are coincident or the tangent is parallel to the chord.
pub(crate) fn compute_bulge(a: DVec2, tangent: DVec2, b: DVec2) -> f64 {
    let d = b - a;
    let len_sq = d.length_squared();
    if len_sq < 1e-10 {
        return 0.0;
    }
    // Perpendicular to tangent (CCW) — this is the direction to the arc center.
    let perp = DVec2::new(-tangent.y, tangent.x);
    let dot = d.dot(perp);
    if dot.abs() < 1e-10 {
        // Tangent is perpendicular to chord → straight line (bulge = 0).
        return 0.0;
    }
    // t = distance from a to center along perp.
    let t = len_sq / (2.0 * dot);
    let center = a + perp * t;

    // Arc angle from start to end (signed).
    let start_angle = (a - center).y.atan2((a - center).x);
    let end_angle = (b - center).y.atan2((b - center).x);
    let mut arc_angle = end_angle - start_angle;

    if t > 0.0 {
        // CCW arc: ensure arc_angle is in (0, 2π].
        if arc_angle <= 0.0 {
            arc_angle += std::f64::consts::TAU;
        }
    } else {
        // CW arc: ensure arc_angle is in [-2π, 0).
        if arc_angle >= 0.0 {
            arc_angle -= std::f64::consts::TAU;
        }
    }
    (arc_angle / 4.0).tan()
}

/// Exit tangent of the segment `a` → `b` with `bulge` (0 = straight line):
/// the chord direction rotated by half the arc sweep (the chord bisects the
/// entry/exit tangents of a bulge arc). Used to restore tangent continuity
/// after Undo pops a segment.
pub(crate) fn seg_exit_tangent(a: DVec3, b: DVec3, bulge: f64) -> Option<Vec2> {
    let d = DVec2::new(b.x - a.x, b.y - a.y);
    if d.length_squared() < 1e-10 {
        return None;
    }
    // Full sweep is 4·atan(bulge); the exit tangent sits half that past the chord.
    let ang = d.y.atan2(d.x) + 2.0 * bulge.atan();
    Some(Vec2::new(ang.cos() as f32, ang.sin() as f32))
}

/// Update `tangent` after an arc segment described by `bulge` from `a` to `b`.
pub(crate) fn update_tangent_after_arc(tangent: &mut Option<Vec2>, bulge: f64) {
    let Some(t) = *tangent else {
        return;
    };
    // The arc sweeps theta = 4*atan(bulge) radians, so the exit tangent is
    // the entry tangent rotated by that angle.
    let theta = 4.0 * (bulge as f32).atan();
    let (sin_t, cos_t) = theta.sin_cos();
    *tangent =
        Some(Vec2::new(t.x * cos_t - t.y * sin_t, t.x * sin_t + t.y * cos_t).normalize_or_zero());
}

/// Sample a circular arc defined by bulge into `n` line-segment points.
/// Returns the sampled [x, y, z] points (uses `z` from `a`).
pub(crate) fn arc_sample_points(a: Vec3, bulge: f64, b: Vec3, n: usize) -> Vec<[f32; 3]> {
    let ax = a.x as f64;
    let ay = a.y as f64;
    let bx = b.x as f64;
    let by = b.y as f64;

    let dx = bx - ax;
    let dy = by - ay;
    let chord_len = (dx * dx + dy * dy).sqrt();
    if chord_len < 1e-10 || bulge.abs() < 1e-10 {
        return vec![[a.x, a.y, a.z], [b.x, b.y, b.z]];
    }

    // Center of the arc.
    // Formula: center = midpoint + offset * perp_unit
    // where offset = chord_len * (1 - bulge²) / (4 * bulge).
    let b2 = bulge * bulge;
    let offset = chord_len * (1.0 - b2) / (4.0 * bulge);
    let perp_x = -dy / chord_len;
    let perp_y = dx / chord_len;
    let mx = (ax + bx) / 2.0;
    let my = (ay + by) / 2.0;
    let cx = mx + offset * perp_x;
    let cy = my + offset * perp_y;

    let r = ((ax - cx) * (ax - cx) + (ay - cy) * (ay - cy)).sqrt();
    let start_angle = (ay - cy).atan2(ax - cx);
    // Total arc angle (signed).
    let theta = 4.0 * bulge.atan();

    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let angle = start_angle + t * theta;
        pts.push([
            (cx + r * angle.cos()) as f32,
            (cy + r * angle.sin()) as f32,
            a.z,
        ]);
    }
    pts
}

// ── CadCommand impl ────────────────────────────────────────────────────────

impl PlineCommand {
    /// Close the polyline: in Arc mode the closing segment is an arc tangent
    /// to the last segment (CLose in commercial solutions), in Line mode a straight one.
    fn close(&mut self) -> CmdResult {
        if self.vertices.len() < 2 {
            return CmdResult::NeedPoint;
        }
        let last_idx = self.vertices.len() - 1;
        let a = self.local2(self.vertices[last_idx]);
        let first = self.local2(self.vertices[0]);
        self.bulges[last_idx] = match self.mode {
            SegMode::Line => 0.0,
            SegMode::Arc => compute_bulge(a, self.tangent2(), first),
        };
        self.widths[last_idx] = self.cur_width;
        self.sub = Sub::None;
        self.sync_live(true, true)
    }

    fn width_prompt(&self, half: bool, ending: bool, default: f64) -> String {
        let shown = crate::entities::common::format_length(if half { default * 0.5 } else { default });
        match (half, ending) {
            (false, false) => t!("PLINE  Specify starting width <%{w}>:", w = shown),
            (false, true) => t!("PLINE  Specify ending width <%{w}>:", w = shown),
            (true, false) => t!("PLINE  Specify starting half-width <%{w}>:", w = shown),
            (true, true) => t!("PLINE  Specify ending half-width <%{w}>:", w = shown),
        }
        .into_owned()
    }
}

impl CadCommand for PlineCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "PLINE"
    }

    fn prompt(&self) -> String {
        if self.vertices.is_empty() {
            return t!("PLINE  Specify start point:").into_owned();
        }
        match self.sub {
            Sub::None => match self.mode {
                SegMode::Line => t!("PLINE (Line)  Specify next point:").into_owned(),
                SegMode::Arc => t!("PLINE (Arc)  Specify endpoint of arc:").into_owned(),
            },
            Sub::WidthStart { half } => self.width_prompt(half, false, self.cur_width.0),
            Sub::WidthEnd { half, start } => self.width_prompt(half, true, start),
            Sub::Length => t!("PLINE  Specify length of line:").into_owned(),
            Sub::ArcAngle | Sub::ArcCenterAngle { .. } | Sub::ArcRadiusAngle { .. } => {
                t!("PLINE  Specify included angle:").into_owned()
            }
            Sub::ArcAngleEnd { .. }
            | Sub::ArcCenterEnd { .. }
            | Sub::ArcDirectionEnd { .. }
            | Sub::ArcRadiusEnd { .. }
            | Sub::ArcSecondEnd { .. } => t!("PLINE  Specify endpoint of arc:").into_owned(),
            Sub::ArcAngleCenter { .. } | Sub::ArcCenter => {
                t!("PLINE  Specify center point of arc:").into_owned()
            }
            Sub::ArcAngleRadius { .. } | Sub::ArcRadius => {
                t!("PLINE  Specify radius of arc:").into_owned()
            }
            Sub::ArcAngleRadiusDir { .. } | Sub::ArcRadiusAngleDir { .. } => {
                t!("PLINE  Specify direction of chord for arc:").into_owned()
            }
            Sub::ArcCenterLength { .. } => t!("PLINE  Specify length of chord:").into_owned(),
            Sub::ArcDirection => {
                t!("PLINE  Specify tangent direction for the start point of arc:").into_owned()
            }
            Sub::ArcSecond => t!("PLINE  Specify second point on arc:").into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.vertices.is_empty() {
            return Vec::new();
        }
        let can_close = self.vertices.len() >= 3
            || (self.vertices.len() == 2 && matches!(self.mode, SegMode::Arc));
        match self.sub {
            // The keyword sets of commercial solutions, in their order.
            Sub::None => match self.mode {
                SegMode::Line => {
                    let mut opts = vec![CmdOption::new("Arc", "A")];
                    if can_close {
                        opts.push(CmdOption::new("Close", "C"));
                    }
                    opts.extend([
                        CmdOption::new("Halfwidth", "H"),
                        CmdOption::new("Length", "L"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::new("Width", "W"),
                        CmdOption::enter("Done"),
                    ]);
                    opts
                }
                SegMode::Arc => {
                    let mut opts = vec![
                        CmdOption::new("Angle", "A"),
                        CmdOption::new("CEnter", "CE"),
                    ];
                    if can_close {
                        opts.push(CmdOption::new("CLose", "CL"));
                    }
                    opts.extend([
                        CmdOption::new("Direction", "D"),
                        CmdOption::new("Halfwidth", "H"),
                        CmdOption::new("Line", "L"),
                        CmdOption::new("Radius", "R"),
                        CmdOption::new("Second pt", "S"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::new("Width", "W"),
                        CmdOption::enter("Done"),
                    ]);
                    opts
                }
            },
            Sub::WidthStart { half } => {
                let d = if half { self.cur_width.0 * 0.5 } else { self.cur_width.0 };
                vec![CmdOption::enter(&format!("<{}>", crate::entities::common::format_length(d)))]
            }
            Sub::WidthEnd { half, start } => {
                let d = if half { start * 0.5 } else { start };
                vec![CmdOption::enter(&format!("<{}>", crate::entities::common::format_length(d)))]
            }
            Sub::ArcAngleEnd { .. } => vec![
                CmdOption::new("CEnter", "CE"),
                CmdOption::new("Radius", "R"),
            ],
            Sub::ArcCenterEnd { .. } => vec![
                CmdOption::new("Angle", "A"),
                CmdOption::new("Length", "L"),
            ],
            Sub::ArcRadiusEnd { .. } => vec![CmdOption::new("Angle", "A")],
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.vertices.is_empty() {
            return self.push_vertex(pt);
        }
        let Some((a, z)) = self.last_local() else {
            return CmdResult::NeedPoint;
        };
        let p = self.local2(pt);

        match self.sub {
            Sub::None => {}
            // A picked point at a typed-value prompt reads as the distance
            // (or angle) from the last vertex, as commercial solutions allow.
            sub if sub.is_scalar() => {
                let value = if sub.is_angle() {
                    (p - a).y.atan2((p - a).x).to_degrees()
                } else {
                    (p - a).length()
                };
                return self.on_scalar(value).unwrap_or(CmdResult::NeedPoint);
            }
            Sub::ArcCenter => {
                if (p - a).length_squared() < 1e-12 {
                    return CmdResult::NeedPoint;
                }
                self.sub = Sub::ArcCenterEnd { c: p };
                return CmdResult::NeedPoint;
            }
            Sub::ArcDirection => {
                let Some(dir) = (p - a).try_normalize() else {
                    return CmdResult::NeedPoint;
                };
                self.sub = Sub::ArcDirectionEnd { dir };
                return CmdResult::NeedPoint;
            }
            Sub::ArcSecond => {
                if (p - a).length_squared() < 1e-12 {
                    return CmdResult::NeedPoint;
                }
                self.sub = Sub::ArcSecondEnd { s: p };
                return CmdResult::NeedPoint;
            }
            sub => {
                // Endpoint / centre / chord-direction pick that completes an arc.
                return match self.arc_for(sub, a, p) {
                    Some((e, bulge)) => self.push_segment(e, z, bulge),
                    None => CmdResult::NeedPoint,
                };
            }
        }

        let last_idx = self.vertices.len() - 1;
        let bulge = match self.mode {
            SegMode::Line => {
                let d = p - a;
                if d.length_squared() > 1e-10 {
                    // Direction only — f32 is sufficient for tangent continuity.
                    self.last_tangent = Some(d.normalize().as_vec2());
                }
                0.0
            }
            SegMode::Arc => {
                let bulge = compute_bulge(a, self.tangent2(), p);
                update_tangent_after_arc(&mut self.last_tangent, bulge);
                bulge
            }
        };
        self.bulges[last_idx] = bulge;

        // A point landing back on the FIRST vertex (endpoint snap) closes the
        // polyline instead of stacking a duplicate vertex there — the segment
        // bulge just computed above already describes the closing segment
        // (#421). Line mode needs two real segments first so a doubled-back
        // line isn't "closed"; an arc segment closes from two vertices.
        if let Some(first) = self.vertices.first() {
            let enough = self.vertices.len() >= 3
                || (self.vertices.len() == 2 && matches!(self.mode, SegMode::Arc));
            let d2 = (pt.x - first.x).powi(2) + (pt.y - first.y).powi(2);
            if enough && d2 < 1e-12 {
                self.widths[last_idx] = self.cur_width;
                return self.sync_live(true, true);
            }
        }

        self.push_vertex(pt)
    }

    fn set_live_handle(&mut self, handle: Handle) {
        self.live_handle = Some(handle);
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.sub {
            // Enter keeps the offered default width.
            Sub::WidthStart { half } => {
                let start = self.cur_width.0;
                self.sub = Sub::WidthEnd { half, start };
                return CmdResult::NeedPoint;
            }
            Sub::WidthEnd { start, .. } => {
                self.cur_width = (start, start);
                super::super::defaults::set_pline_width(start);
                return self.leave_sub();
            }
            // Enter at any other sub-prompt backs out of it.
            Sub::None => {}
            _ => return self.leave_sub(),
        }
        // Every landed vertex is already published into the live document
        // entity. Finishing must only close its history/command state; replacing
        // the identical polyline again would advance geometry_epoch and make
        // large drawings do another cache/GPU patch for no visual change.
        match self.live_handle {
            Some(handle) => CmdResult::FinalizeLiveEntity(handle),
            None => CmdResult::Cancel,
        }
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.vertices.is_empty()
    }

    fn on_escape(&mut self) -> CmdResult {
        // Escape inside a keyword sub-prompt returns to the next-point step;
        // a second Escape ends the command keeping what is drawn.
        if self.sub != Sub::None {
            return self.leave_sub();
        }
        match self.live_handle {
            Some(handle) => CmdResult::FinalizeLiveEntity(handle),
            None => CmdResult::Cancel,
        }
    }

    fn on_space_change(&mut self) -> CmdResult {
        match self.live_handle {
            Some(handle) => CmdResult::FinalizeLiveEntity(handle),
            None => CmdResult::Cancel,
        }
    }

    fn wants_text_input(&self) -> bool {
        // Keywords once the first point is placed; typed values in the
        // scalar sub-steps.
        !self.vertices.is_empty()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // Each segment is a point pick that also accepts keywords, so the
        // polar dynamic-input distance/angle stays visible. Scalar sub-steps
        // are pure typed-value prompts.
        !self.vertices.is_empty() && !self.sub.is_scalar()
    }

    fn dyn_field(&self) -> crate::command::DynField {
        use crate::command::DynField;
        match self.sub {
            Sub::WidthStart { .. } | Sub::WidthEnd { .. } => DynField::Scalar,
            Sub::Length | Sub::ArcRadius | Sub::ArcAngleRadius { .. } | Sub::ArcCenterLength { .. } => {
                DynField::Distance
            }
            sub if sub.is_angle() => DynField::Angle,
            _ => DynField::Point,
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        self.sub.is_scalar()
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        let (a, _) = self.last_local()?;
        let p = self.local2(cursor);
        match self.sub {
            // Length: the cursor's projection onto the continuation direction.
            Sub::Length => Some((p - a).dot(self.tangent2())),
            Sub::ArcRadius | Sub::ArcAngleRadius { .. } | Sub::ArcCenterLength { .. } => {
                Some((p - a).length())
            }
            _ => None,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let upper = text.trim().to_uppercase();
        if self.vertices.is_empty() {
            return None;
        }
        // Keywords valid at any step: Undo backs out of a sub-prompt first,
        // then pops vertices.
        match upper.as_str() {
            "U" | "UNDO" => {
                return Some(if self.sub != Sub::None {
                    self.leave_sub()
                } else {
                    self.undo_last_vertex()
                });
            }
            _ => {}
        }
        match self.sub {
            Sub::None => match (self.mode, upper.as_str()) {
                (_, "W" | "WIDTH") => {
                    self.sub = Sub::WidthStart { half: false };
                    Some(CmdResult::NeedPoint)
                }
                (_, "H" | "HALFWIDTH") => {
                    self.sub = Sub::WidthStart { half: true };
                    Some(CmdResult::NeedPoint)
                }
                (SegMode::Line, "A" | "ARC") => {
                    self.mode = SegMode::Arc;
                    Some(CmdResult::NeedPoint)
                }
                (SegMode::Line, "C" | "CLOSE") => Some(self.close()),
                (SegMode::Line, "L" | "LENGTH") => {
                    self.sub = Sub::Length;
                    Some(CmdResult::NeedPoint)
                }
                (SegMode::Arc, "L" | "LINE") => {
                    self.mode = SegMode::Line;
                    Some(CmdResult::NeedPoint)
                }
                (SegMode::Arc, "A" | "ANGLE") => {
                    self.sub = Sub::ArcAngle;
                    Some(CmdResult::NeedPoint)
                }
                (SegMode::Arc, "CE" | "CENTER" | "CENTRE") => {
                    self.sub = Sub::ArcCenter;
                    Some(CmdResult::NeedPoint)
                }
                // Commercial solutions want CL in arc mode; a bare C is accepted as well.
                (SegMode::Arc, "CL" | "C" | "CLOSE") => Some(self.close()),
                (SegMode::Arc, "D" | "DIRECTION") => {
                    self.sub = Sub::ArcDirection;
                    Some(CmdResult::NeedPoint)
                }
                (SegMode::Arc, "R" | "RADIUS") => {
                    self.sub = Sub::ArcRadius;
                    Some(CmdResult::NeedPoint)
                }
                (SegMode::Arc, "S" | "SECOND" | "SECONDPT") => {
                    self.sub = Sub::ArcSecond;
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            Sub::ArcAngleEnd { angle } => match upper.as_str() {
                "CE" | "CENTER" | "CENTRE" => {
                    self.sub = Sub::ArcAngleCenter { angle };
                    Some(CmdResult::NeedPoint)
                }
                "R" | "RADIUS" => {
                    self.sub = Sub::ArcAngleRadius { angle };
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            Sub::ArcCenterEnd { c } => match upper.as_str() {
                "A" | "ANGLE" => {
                    self.sub = Sub::ArcCenterAngle { c };
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LENGTH" => {
                    self.sub = Sub::ArcCenterLength { c };
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            Sub::ArcRadiusEnd { r } => match upper.as_str() {
                "A" | "ANGLE" => {
                    self.sub = Sub::ArcRadiusAngle { r };
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            sub if sub.is_scalar() => {
                let value = if sub.is_angle() {
                    crate::entities::common::parse_typed_angle(text.trim()).map(f64::to_degrees)
                } else {
                    crate::entities::common::parse_typed_length(text.trim())
                }?;
                self.on_scalar(value)
            }
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        // Ctrl+Z while drawing steps back one vertex; with nothing placed
        // yet the document undo takes over.
        if self.vertices.is_empty() {
            None
        } else if self.sub != Sub::None {
            Some(self.leave_sub())
        } else {
            Some(self.undo_last_vertex())
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        // The committed vertices already render as a real document entity, so
        // the preview is just the pending segment from the last vertex to the
        // cursor. (#119)
        let (a, z) = self.last_local()?;
        let p = self.local2(pt);
        match self.sub {
            Sub::None => match self.mode {
                SegMode::Line => Some(self.line_wire(a, p, z)),
                SegMode::Arc => {
                    let bulge = compute_bulge(a, self.tangent2(), p);
                    Some(self.arc_wire(a, p, z, bulge))
                }
            },
            // The continuation line, as far as the cursor projects onto it.
            Sub::Length => {
                let e = a + self.tangent2() * (p - a).dot(self.tangent2());
                Some(self.line_wire(a, e, z))
            }
            // Picks that only fix a centre / direction / second point show
            // the radius / tangent / chord line to the cursor.
            Sub::ArcCenter | Sub::ArcDirection | Sub::ArcSecond | Sub::ArcAngleCenter { .. } => {
                match self.arc_for(self.sub, a, p) {
                    Some((e, bulge)) => Some(self.arc_wire(a, e, z, bulge)),
                    None => Some(self.line_wire(a, p, z)),
                }
            }
            sub if sub.is_scalar() => None,
            sub => match self.arc_for(sub, a, p) {
                Some((e, bulge)) => Some(self.arc_wire(a, e, z, bulge)),
                None => Some(self.line_wire(a, p, z)),
            },
        }
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["PLINE"] });  // PlineCommand

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_contains_only_the_pending_segment() {
        let mut command = PlineCommand::new();
        command.on_point(DVec3::new(0.0, 0.0, 0.0));
        command.on_point(DVec3::new(10.0, 0.0, 0.0));
        command.on_point(DVec3::new(10.0, 5.0, 0.0));

        let preview = command
            .on_mouse_move(DVec3::new(15.0, 5.0, 0.0))
            .expect("polyline with vertices should have a preview");

        assert_eq!(
            preview.points,
            vec![[10.0, 5.0, 0.0], [15.0, 5.0, 0.0]]
        );
    }

    #[test]
    fn committed_vertices_update_one_live_polyline() {
        let mut command = PlineCommand::new();
        assert!(matches!(
            command.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));

        match command.on_point(DVec3::new(10.0, 0.0, 0.0)) {
            CmdResult::CommitLiveEntity(EntityType::LwPolyline(polyline)) => {
                assert_eq!(polyline.vertices.len(), 2);
            }
            _ => panic!("the first segment should create a live polyline"),
        }

        let handle = Handle::new(42);
        command.set_live_handle(handle);
        match command.on_point(DVec3::new(10.0, 5.0, 0.0)) {
            CmdResult::UpdateLiveEntity {
                handle: updated,
                entity: EntityType::LwPolyline(polyline),
                finish,
            } => {
                assert_eq!(updated, handle);
                assert_eq!(polyline.vertices.len(), 3);
                assert!(!finish);
            }
            _ => panic!("later segments should update the same live polyline"),
        }
    }

    #[test]
    fn finishing_live_polyline_does_not_publish_duplicate_update() {
        let handle = Handle::new(42);

        let mut enter = PlineCommand::new();
        enter.live_handle = Some(handle);
        assert!(matches!(
            enter.on_enter(),
            CmdResult::FinalizeLiveEntity(h) if h == handle
        ));

        let mut escape = PlineCommand::new();
        escape.live_handle = Some(handle);
        assert!(matches!(
            escape.on_escape(),
            CmdResult::FinalizeLiveEntity(h) if h == handle
        ));
    }

    fn pl(points: &[(f64, f64)]) -> PlineCommand {
        let mut cmd = PlineCommand::new();
        for &(x, y) in points {
            cmd.on_point(DVec3::new(x, y, 0.0));
        }
        cmd.set_live_handle(Handle::new(1));
        cmd
    }

    fn entity_of(res: CmdResult) -> LwPolyline {
        match res {
            CmdResult::UpdateLiveEntity {
                entity: EntityType::LwPolyline(p),
                ..
            }
            | CmdResult::CommitLiveEntity(EntityType::LwPolyline(p)) => p,
            other => panic!("expected a polyline result, got {}", std::any::type_name_of_val(&other)),
        }
    }

    fn keywords(cmd: &PlineCommand) -> Vec<String> {
        cmd.options().into_iter().map(|o| o.keyword).collect()
    }

    #[test]
    fn options_per_mode_match_compatible_keywords() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        assert_eq!(keywords(&cmd), ["A", "H", "L", "U", "W", ""]);
        cmd.on_point(DVec3::new(10.0, 5.0, 0.0));
        assert_eq!(keywords(&cmd), ["A", "C", "H", "L", "U", "W", ""]);
        cmd.on_text_input("A");
        assert_eq!(
            keywords(&cmd),
            ["A", "CE", "CL", "D", "H", "L", "R", "S", "U", "W", ""]
        );
        cmd.on_text_input("A");
        assert!(keywords(&cmd).is_empty(), "typed-angle prompt has no keywords");
        cmd.on_text_input("90");
        assert_eq!(keywords(&cmd), ["CE", "R"]);
        cmd.on_text_input("CE");
        assert!(keywords(&cmd).is_empty());
    }

    #[test]
    fn width_option_sets_segment_widths_in_entity() {
        let mut cmd = pl(&[(0.0, 0.0)]);
        assert!(matches!(cmd.on_text_input("W"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cmd.sub, Sub::WidthStart { half: false }));
        assert!(matches!(cmd.on_text_input("1"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cmd.sub, Sub::WidthEnd { half: false, .. }));
        assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cmd.sub, Sub::None));
        let e = entity_of(cmd.on_point(DVec3::new(10.0, 0.0, 0.0)));
        assert_eq!((e.vertices[0].start_width, e.vertices[0].end_width), (1.0, 2.0));
        // The end width carries over as the next segment's start width.
        let e = entity_of(cmd.on_point(DVec3::new(20.0, 0.0, 0.0)));
        assert_eq!((e.vertices[1].start_width, e.vertices[1].end_width), (2.0, 2.0));
        assert_eq!(super::super::super::defaults::get_pline_width(), 2.0);
        super::super::super::defaults::set_pline_width(0.0);
    }

    #[test]
    fn halfwidth_doubles_value_and_enter_keeps_default() {
        let mut cmd = pl(&[(0.0, 0.0)]);
        cmd.on_text_input("H");
        assert!(matches!(cmd.sub, Sub::WidthStart { half: true }));
        cmd.on_text_input("0.5");
        // Enter at the ending prompt keeps the starting value.
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
        assert_eq!(cmd.cur_width, (1.0, 1.0));
        super::super::super::defaults::set_pline_width(0.0);
    }

    #[test]
    fn length_extends_along_last_tangent() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("L");
        assert!(matches!(cmd.sub, Sub::Length));
        let e = entity_of(cmd.on_text_input("5").expect("length consumed"));
        assert_eq!(e.vertices.len(), 3);
        assert!((e.vertices[2].location.x - 15.0).abs() < 1e-9);
        assert!(e.vertices[2].location.y.abs() < 1e-9);
    }

    #[test]
    fn length_after_arc_follows_exit_tangent() {
        // Line east, then a tangent semicircle up to (10,10): exit heads west.
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("A");
        cmd.on_point(DVec3::new(10.0, 10.0, 0.0));
        cmd.on_text_input("L"); // back to Line mode
        cmd.on_text_input("L"); // Length
        let e = entity_of(cmd.on_text_input("5").expect("length consumed"));
        assert!((e.vertices[3].location.x - 5.0).abs() < 1e-6, "{:?}", e.vertices[3].location);
        assert!((e.vertices[3].location.y - 10.0).abs() < 1e-6);
    }

    #[test]
    fn arc_close_closes_with_bulge() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        cmd.on_text_input("A");
        let res = cmd.on_text_input("CL").expect("CLose consumed");
        match res {
            CmdResult::UpdateLiveEntity { entity: EntityType::LwPolyline(p), finish, .. } => {
                assert!(finish);
                assert!(p.is_closed);
                assert!(p.vertices[2].bulge.abs() > 1e-6, "closing segment should be an arc");
            }
            _ => panic!("expected a finishing update"),
        }
    }

    #[test]
    fn arc_angle_endpoint_bulge_is_tan_quarter() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("A");
        cmd.on_text_input("A");
        cmd.on_text_input("90");
        let e = entity_of(cmd.on_point(DVec3::new(10.0, 10.0, 0.0)));
        let expect = (std::f64::consts::FRAC_PI_2 / 4.0).tan();
        assert!((e.vertices[1].bulge - expect).abs() < 1e-9);
        assert_eq!(e.vertices.len(), 3);
    }

    #[test]
    fn arc_center_endpoint_projects_to_radius() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("A");
        cmd.on_text_input("CE");
        assert!(matches!(cmd.sub, Sub::ArcCenter));
        cmd.on_point(DVec3::new(10.0, 5.0, 0.0));
        assert_eq!(keywords(&cmd), ["A", "L"]);
        let e = entity_of(cmd.on_point(DVec3::new(20.0, 5.0, 0.0)));
        let v = e.vertices[2].location;
        assert!((v.x - 15.0).abs() < 1e-9 && (v.y - 5.0).abs() < 1e-9, "{v:?}");
        let expect = (std::f64::consts::FRAC_PI_2 / 4.0).tan();
        assert!((e.vertices[1].bulge - expect).abs() < 1e-9, "quarter turn CCW");
    }

    #[test]
    fn arc_center_length_uses_chord() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("A");
        cmd.on_text_input("CE");
        cmd.on_point(DVec3::new(10.0, 5.0, 0.0));
        cmd.on_text_input("L");
        // Chord 10 on a radius 5 circle: a semicircle ending at (10,10).
        let e = entity_of(cmd.on_text_input("10").expect("chord length"));
        let v = e.vertices[2].location;
        assert!((v.x - 10.0).abs() < 1e-6 && (v.y - 10.0).abs() < 1e-6, "{v:?}");
    }

    #[test]
    fn arc_radius_rejects_long_chord() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("A");
        cmd.on_text_input("R");
        cmd.on_text_input("3");
        assert!(matches!(cmd.on_point(DVec3::new(20.0, 0.0, 0.0)), CmdResult::NeedPoint));
        assert_eq!(cmd.vertices.len(), 2, "chord longer than the diameter is refused");
        let e = entity_of(cmd.on_point(DVec3::new(16.0, 0.0, 0.0)));
        assert_eq!(e.vertices.len(), 3);
        assert!(e.vertices[1].bulge.abs() > 0.0);
    }

    #[test]
    fn arc_second_point_three_point_arc_passes_through_second() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("A");
        cmd.on_text_input("S");
        cmd.on_point(DVec3::new(15.0, 5.0, 0.0));
        let e = entity_of(cmd.on_point(DVec3::new(20.0, 0.0, 0.0)));
        // Semicircle over the chord, running clockwise (bulge -1).
        assert!((e.vertices[1].bulge + 1.0).abs() < 1e-9, "{}", e.vertices[1].bulge);
    }

    #[test]
    fn arc_direction_uses_picked_tangent() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("A");
        cmd.on_text_input("D");
        cmd.on_point(DVec3::new(10.0, 10.0, 0.0)); // tangent +Y
        let e = entity_of(cmd.on_point(DVec3::new(20.0, 0.0, 0.0)));
        assert!((e.vertices[1].bulge + 1.0).abs() < 1e-9, "{}", e.vertices[1].bulge);
    }

    #[test]
    fn undo_in_substep_only_leaves_substep() {
        let mut cmd = pl(&[(0.0, 0.0), (10.0, 0.0)]);
        cmd.on_text_input("W");
        assert!(matches!(cmd.on_text_input("U"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cmd.sub, Sub::None));
        assert_eq!(cmd.vertices.len(), 2);
        // Escape likewise backs out of a sub-prompt without finishing.
        cmd.on_text_input("A");
        cmd.on_text_input("R");
        assert!(matches!(cmd.on_escape(), CmdResult::NeedPoint));
        assert!(matches!(cmd.sub, Sub::None));
        // A second Undo pops the vertex.
        cmd.on_text_input("U");
        assert_eq!(cmd.vertices.len(), 1);
    }

    #[test]
    fn test_draw_line_then_arc() {
        let mut cmd = PlineCommand::new();
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let res = cmd.on_point(DVec3::new(10.0, 0.0, 0.0));
        assert!(matches!(res, CmdResult::CommitLiveEntity(_)));
        cmd.set_live_handle(Handle::new(1));
        cmd.on_text_input("A");
        let wire = cmd.on_mouse_move(DVec3::new(10.0, 5.0, 0.0)).expect("arc wire");
        assert_eq!(wire.tangent_geoms.len(), 1);
        assert!(matches!(wire.tangent_geoms[0], TangentGeom::Arc { .. }));
        assert!(wire.points.len() >= 16);
        let res2 = cmd.on_point(DVec3::new(10.0, 5.0, 0.0));
        assert!(matches!(res2, CmdResult::UpdateLiveEntity { .. }));
    }
}
