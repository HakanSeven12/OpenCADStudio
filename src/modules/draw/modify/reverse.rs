// REVERSE command — reverse the direction (vertex / point order) of a single
// picked curve.
//
// The user picks one open or closed curve; the command builds a reversed copy
// and swaps it in via `CmdResult::ReplaceEntity`. Supported curve types:
//
//   * Line        — swap start / end.
//   * LwPolyline / Polyline2D — reverse the vertex list. The bulge stored at a
//                   vertex
//                   describes the arc on the segment *starting* at that vertex,
//                   so a plain `vertices.reverse()` would attach each bulge to
//                   the wrong segment (and with the wrong sense). We therefore
//                   recompute every bulge from the original segment bulges (see
//                   `reverse_lwpolyline`).
//   * Polyline / Polyline3D — reverse the vertex list (no bulges to reconcile).
//   * Spline      — reverse control points and fit points, then regenerate the
//                   clamped knot vector (mirrors the REVERSE branch of
//                   `splinedit::apply_spline_op`).
//
// Any other entity type is left untouched: the command returns
// `CmdResult::NeedPoint` and keeps prompting so nothing is corrupted.

use acadrust::entities::Spline;
use acadrust::{EntityType, Handle};
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

// ── Ribbon definition ───────────────────────────────────────────────────────

#[allow(dead_code)] // ribbon definition ready for wiring; command works via the command line
pub fn tool() -> ToolDef {
    ToolDef {
        id: "REVERSE",
        label: "Reverse",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("REVERSE".to_string()),
    }
}

// ── Command implementation ──────────────────────────────────────────────────

pub struct ReverseCommand {
    /// The picked entity, injected by the host before `on_entity_pick` runs.
    /// `None` until the host injects it.
    picked: Option<EntityType>,
}

impl ReverseCommand {
    pub fn new() -> Self {
        Self { picked: None }
    }

    /// Build a reversed copy of `entity`, or `None` for an unsupported type.
    pub fn reversed(entity: &EntityType) -> Option<EntityType> {
        match entity {
            EntityType::Line(line) => {
                let mut out = line.clone();
                std::mem::swap(&mut out.start, &mut out.end);
                Some(EntityType::Line(out))
            }
            EntityType::LwPolyline(pl) => Some(EntityType::LwPolyline(reverse_lwpolyline(pl))),
            EntityType::Polyline2D(pl) => {
                Some(EntityType::Polyline2D(reverse_polyline2d(pl)))
            }
            EntityType::Polyline(pl) => {
                let mut out = pl.clone();
                out.vertices.reverse();
                Some(EntityType::Polyline(out))
            }
            EntityType::Polyline3D(pl) => {
                let mut out = pl.clone();
                out.vertices.reverse();
                Some(EntityType::Polyline3D(out))
            }
            EntityType::Spline(sp) => Some(EntityType::Spline(reverse_spline(sp))),
            EntityType::Helix(helix) => crate::entities::helix::reversed(helix).map(EntityType::Helix),
            _ => None,
        }
    }
}

/// Reverse an LwPolyline, reconciling per-segment bulges.
///
/// Bulge semantics: `vertices[k].bulge` is the bulge of the segment that
/// *starts* at vertex `k`. For an open polyline with `n` vertices there are
/// `n - 1` segments (the trailing vertex's bulge is unused); for a closed
/// polyline there are `n` segments (the trailing vertex's bulge is the closing
/// segment back to vertex 0).
///
/// After reversal the new vertex `i` is the original vertex `n-1-i`. The new
/// segment from new-vertex `i` to new-vertex `i+1` is the original segment
/// between original vertices `n-1-i` and `n-2-i`, traversed backwards — i.e.
/// the original segment that *starts* at vertex `n-2-i`. Reversing the
/// traversal direction flips an arc's sense, so the new bulge is the negation
/// of that original segment's bulge:
///
///   new_vertices[i].bulge = -old_vertices[n-2-i].bulge        (open / interior)
///
/// For a closed polyline the closing segment wraps, so the general modular form
/// is used: new seg `i` (from new vertex `i`) corresponds to original segment
/// starting at original vertex `(n-1-i-1).rem_euclid(n)`. The widths follow the
/// Widths are segment-relative. Reversing a segment swaps its start and end
/// widths in addition to moving them to the corresponding reversed segment.
fn reverse_lwpolyline(pl: &acadrust::LwPolyline) -> acadrust::LwPolyline {
    let mut out = pl.clone();
    let n = pl.vertices.len();
    if n < 2 {
        return out;
    }

    // Reverse the complete vertex payload first. Live segment attributes are
    // remapped below; the open terminal vertex keeps its unused payload.
    out.vertices = pl.vertices.iter().rev().copied().collect();

    // Recompute bulges from the original segment bulges.
    if pl.is_closed {
        // n segments; closing segment included.
        for i in 0..n {
            let src_seg_start = (n - 1 - i + n - 1) % n; // = (2n - 2 - i) % n
            out.vertices[i].bulge = -pl.vertices[src_seg_start].bulge;
            out.vertices[i].start_width = pl.vertices[src_seg_start].end_width;
            out.vertices[i].end_width = pl.vertices[src_seg_start].start_width;
        }
    } else {
        // n - 1 segments. New segment i (for i in 0..n-1) maps to original
        // segment starting at vertex n-2-i. The trailing vertex's bulge is
        // unused for an open polyline; clear it for cleanliness.
        for i in 0..n - 1 {
            let src_seg_start = n - 2 - i;
            out.vertices[i].bulge = -pl.vertices[src_seg_start].bulge;
            out.vertices[i].start_width = pl.vertices[src_seg_start].end_width;
            out.vertices[i].end_width = pl.vertices[src_seg_start].start_width;
        }
        out.vertices[n - 1].bulge = 0.0;
    }

    out
}

/// Reverse a heavy 2D polyline with the same segment semantics as LWPolyline.
fn reverse_polyline2d(pl: &acadrust::entities::Polyline2D) -> acadrust::entities::Polyline2D {
    let mut out = pl.clone();
    let n = pl.vertices.len();
    if n < 2 {
        return out;
    }

    out.vertices = pl.vertices.iter().rev().cloned().collect();
    out.start_width = pl.end_width;
    out.end_width = pl.start_width;
    for vertex in &mut out.vertices {
        let tangent_flag = acadrust::entities::VertexFlags::CURVE_FIT_TANGENT.bits();
        if vertex.flags.bits() & tangent_flag != 0 {
            vertex.curve_tangent = (vertex.curve_tangent + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU);
        }
    }
    if pl.is_closed() {
        for i in 0..n {
            let src_seg_start = (n - 1 - i + n - 1) % n;
            out.vertices[i].bulge = -pl.vertices[src_seg_start].bulge;
            out.vertices[i].start_width = pl.vertices[src_seg_start].end_width;
            out.vertices[i].end_width = pl.vertices[src_seg_start].start_width;
        }
    } else {
        for i in 0..n - 1 {
            let src_seg_start = n - 2 - i;
            out.vertices[i].bulge = -pl.vertices[src_seg_start].bulge;
            out.vertices[i].start_width = pl.vertices[src_seg_start].end_width;
            out.vertices[i].end_width = pl.vertices[src_seg_start].start_width;
        }
        out.vertices[n - 1].bulge = 0.0;
    }
    out
}

/// Reverse stored coordinates in 3D and let the kernel mirror the knot domain.
pub(super) fn reverse_spline(sp: &Spline) -> Spline {
    let mut out = sp.clone();
    out.control_points.reverse();
    out.fit_points.reverse();
    // Weights track control points one-to-one for rational splines.
    if out.weights.len() == out.control_points.len() {
        out.weights.reverse();
    }
    if let Some(curve) = super::spline_ops::spline_to_nurbs(sp) {
        out.knots = curve.reversed().knots().to_vec();
    }
    out.begin_tangent = acadrust::types::Vector3::new(-sp.end_tangent.x, -sp.end_tangent.y, -sp.end_tangent.z);
    out.end_tangent = acadrust::types::Vector3::new(-sp.begin_tangent.x, -sp.begin_tangent.y, -sp.begin_tangent.z);
    out
}

impl CadCommand for ReverseCommand {
    fn name(&self) -> &'static str {
        "REVERSE"
    }

    fn prompt(&self) -> String {
        t!("REVERSE  Select line, polyline, spline or helix to reverse:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.picked.as_ref().and_then(Self::reversed) {
            Some(reversed) => CmdResult::ReplaceEntity(handle, vec![reversed]),
            // Unsupported entity type — keep prompting, do not corrupt it.
            None => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ───────────────────────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["REVERSE"]
}); // ReverseCommand
