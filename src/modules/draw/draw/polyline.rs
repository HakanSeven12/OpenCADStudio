// Polyline tool — ribbon definition + interactive command.
//
// Command:  PLINE (PL)
//   Each click adds a vertex.
//   Type A = switch to Arc segment mode.
//   Type L = switch back to Line segment mode.
//   Enter / C = close and commit.  Escape = commit as-is (if ≥2 vertices).
//
// Arc mode: arcs are tangent-continuous with the preceding segment.
// Bulge is stored per vertex (segment i→i+1); positive = CCW, negative = CW.

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

// ── Command implementation ─────────────────────────────────────────────────

pub struct PlineCommand {
    vertices: Vec<DVec3>,
    /// Bulge for segment i → i+1 (one entry per vertex; last entry unused on commit).
    bulges: Vec<f64>,
    mode: SegMode,
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
        Self {
            vertices: Vec::new(),
            bulges: Vec::new(),
            mode: SegMode::Line,
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

impl CadCommand for PlineCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "PLINE"
    }

    fn prompt(&self) -> String {
        let mode_tag = match self.mode {
            SegMode::Line => t!("Line"),
            SegMode::Arc => t!("Arc"),
        };
        if self.vertices.is_empty() {
            t!("PLINE  Specify start point:").into_owned()
        } else {
            t!(
                "PLINE [%{mode}]  Next pt  [%{count}pts]:",
                mode = mode_tag,
                count = self.vertices.len()
            )
            .into_owned()
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        // Options appear once at least one vertex is placed: the segment step
        // accepts A / L / C, and Enter finishes.
        if self.vertices.is_empty() {
            Vec::new()
        } else {
            vec![
                CmdOption::new(t!("Arc").as_ref(), "A"),
                CmdOption::new(t!("Line").as_ref(), "L"),
                CmdOption::new(t!("Close").as_ref(), "C"),
                CmdOption::new(t!("Undo").as_ref(), "U"),
                CmdOption::enter(t!("Done").as_ref()),
            ]
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if !self.vertices.is_empty() {
            let last = *self.vertices.last().unwrap();
            let last_local = self.plane.to_local(last);
            let pt_local = self.plane.to_local(pt);
            let last_idx = self.vertices.len() - 1;

            let bulge = match self.mode {
                SegMode::Line => {
                    let d = DVec2::new(pt_local.x - last_local.x, pt_local.y - last_local.y);
                    if d.length_squared() > 1e-10 {
                        // Direction only — f32 is sufficient for tangent continuity.
                        self.last_tangent = Some(d.normalize().as_vec2());
                    }
                    0.0
                }
                SegMode::Arc => {
                    let a = DVec2::new(last_local.x, last_local.y);
                    let b = DVec2::new(pt_local.x, pt_local.y);
                    let tangent = self
                        .last_tangent
                        .map(|t| t.as_dvec2())
                        // No previous tangent: default to pointing right (arbitrary).
                        .unwrap_or(DVec2::new(1.0, 0.0));
                    let bulge = compute_bulge(a, tangent, b);
                    update_tangent_after_arc(&mut self.last_tangent, bulge);
                    bulge
                }
            };
            self.bulges[last_idx] = bulge;
        }

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
                return self.sync_live(true, true);
            }
        }

        self.vertices.push(pt);
        self.bulges.push(0.0);
        // Publish to the document as soon as the polyline has a segment so it
        // becomes snappable while the rest is drawn. (#119)
        if self.vertices.len() >= 2 {
            self.sync_live(false, false)
        } else {
            CmdResult::NeedPoint
        }
    }

    fn set_live_handle(&mut self, handle: Handle) {
        self.live_handle = Some(handle);
    }

    fn on_enter(&mut self) -> CmdResult {
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
        // Accept A / L / C once we have at least the first point.
        !self.vertices.is_empty()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // Each segment is a point pick that also accepts A / L / C / U, so the
        // polar dynamic-input distance/angle stays visible.
        !self.vertices.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "A" | "ARC" => {
                self.mode = SegMode::Arc;
                Some(CmdResult::NeedPoint)
            }
            "L" | "LINE" => {
                self.mode = SegMode::Line;
                Some(CmdResult::NeedPoint)
            }
            "C" | "CLOSE" => Some(self.sync_live(true, true)),
            // Undo: drop the last placed vertex but keep drawing (#352).
            // Repeatable — each U backs off one more segment, down to (and
            // including) the start point.
            "U" | "UNDO" => Some(self.undo_last_vertex()),
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        // Ctrl+Z while drawing steps back one vertex; with nothing placed
        // yet the document undo takes over.
        if self.vertices.is_empty() {
            None
        } else {
            Some(self.undo_last_vertex())
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let pt_world = pt;
        // The committed vertices already render as a real document entity, so
        // the preview is just the pending segment from the last vertex to the
        // cursor. (#119)  Downcast to f32 for the pixel-space rubber band.
        let last_world = *self.vertices.last()?;
        let last = self.plane.to_local(last_world).as_vec3();
        let pt = self.plane.to_local(pt_world).as_vec3();

        let mut pts: Vec<[f32; 3]> = Vec::new();
        let mut tangent_geom = None;
        match self.mode {
            SegMode::Line => {
                pts.push([last.x, last.y, last.z]);
                pts.push([pt.x, pt.y, pt.z]);
            }
            SegMode::Arc => {
                let a = DVec2::new(last.x as f64, last.y as f64);
                let b = DVec2::new(pt.x as f64, pt.y as f64);
                let tangent = self
                    .last_tangent
                    .map(|t| t.as_dvec2())
                    .unwrap_or(DVec2::new(1.0, 0.0));
                let bulge = compute_bulge(a, tangent, b);
                if bulge.abs() >= 1e-9 && (b - a).length_squared() >= 1e-12 {
                    if let Some(arc) =
                        crate::entities::common::BulgeArc::from_bulge([a.x, a.y], [b.x, b.y], bulge)
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
                            .to_world(DVec3::new(arc.center[0], arc.center[1], last.z as f64));
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
                        let arc_pts = arc_sample_points(last, bulge, pt, 64);
                        pts.extend_from_slice(&arc_pts);
                    }
                } else {
                    pts.push([last.x, last.y, last.z]);
                    pts.push([pt.x, pt.y, pt.z]);
                }
            }
        }

        let world_points = pts
            .into_iter()
            .map(|point| self.plane.to_world(Vec3::from_array(point).as_dvec3()).as_vec3().to_array())
            .collect();
        let mut wire = WireModel::solid(
            "rubber_band".into(),
            world_points,
            WireModel::CYAN,
            false,
        );
        if let Some(tg) = tangent_geom {
            wire.tangent_geoms.push(tg);
        }
        Some(wire)
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
