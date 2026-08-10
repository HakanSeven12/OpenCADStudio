// Shared helpers used by per-entity tessellation impls in `crate::entities`
// and the dispatcher in `crate::scene::convert::tessellate`.
//
// Anything cross-entity (colour resolution, arc sampling, chord tolerance)
// lives here. Entity-specific helpers (dim format strings, mleader path
// types, etc.) stay with their entity file.

use acadrust::types::Color as AcadColor;
use glam::Vec3;

use crate::scene::model::wire_model::{SnapHint, TangentGeom, WireModel};

/// Output of the fallback per-entity geometry path used by entities not
/// covered by the render conversion pipeline (Viewport, Insert, Hatch
/// outline, Ole2Frame). Tuple form preserved to avoid touching every
/// callsite when the dispatcher wraps these into a WireModel.
///
/// Layout: `(points, snap_pts, tangent_geoms, key_vertices)`.
///
/// `points` are ABSOLUTE world coordinates in f64 — the dispatcher splits them
/// into the double-single high/low pair the relative-to-eye renderer needs, so
/// fallback outlines (hatch boundary, viewport/insert/ole2frame frames) stay
/// glued to their fills at UTM scale instead of quantizing ~0.5 m in f32.
pub type FallbackGeometry = (
    Vec<[f64; 3]>,
    Vec<(Vec3, SnapHint)>,
    Vec<TangentGeom>,
    Vec<[f64; 3]>,
);

// ── Arc tessellation helpers ─────────────────────────────────────────────

/// Convert hatch-boundary arc `(start, end, ccw)` into a
/// `(start, signed_span)` ready for the sampling loop. Matches the
/// legacy `(TAU - sa, TAU - ea)` flip used here for years — direction
/// semantics are preserved on real files. (Wrap-through-2π is a known
/// edge case in that convention; do not "fix" it without a wider audit
/// of how upstream writers emit CW boundary arcs.)
///
/// VERIFIED against real AutoCAD output (KSR-039 DWG, CW arc edges with
/// line neighbours): the stored angles of a `ccw = false` arc edge are
/// MIRRORED — the true point is `center + r·(cos(TAU-θ), sin(TAU-θ))`.
/// Sampling the mirrored parameter directly (this function's flip) gives
/// exact endpoint continuity with the adjacent edges (Δ = 0.000000),
/// while interpreting them as true angles lands tens of units away. Any
/// code that *produces* CW boundary arcs (mirror transforms, explode)
/// must therefore store mirrored angles, not geometric ones.
pub fn arc_signed_span(start: f64, end: f64, ccw: bool) -> (f64, f64) {
    const TAU: f64 = std::f64::consts::TAU;
    let (sa, ea) = if ccw {
        (start, end)
    } else {
        (TAU - start, TAU - end)
    };
    (sa, ea - sa)
}

/// Segment count for an arc, targeting `chord_tol_world` chord-height
/// error in world units. Floor 8, cap 512.
///
/// Two production callers:
/// - hatch fill polygon (built at load / on edit) passes a radius-
///   relative tol via [`fill_chord_tol`] — ~0.1% radius, zoom-free so
///   the polygon stays sharp at extreme zoom-in without re-tessellation.
/// - hatch wire outline (re-tessellated every frame inside the render
///   scope) passes a zoom-adaptive tol via [`wire_chord_tol`] — pulls
///   from the per-frame override set by `Scene::wires_for_block` so
///   far-out arcs collapse to a handful of segments.
pub fn arc_segments(radius: f64, span_abs: f64, chord_tol_world: f64) -> u32 {
    arc_segments_floored(radius, span_abs, chord_tol_world, 8)
}

/// Like [`arc_segments`] but with a caller-chosen segment floor. The 2-D wire
/// wants floor 8 so a circle reads as round; a curved-surface grid patch or a
/// short arc wants a lower floor so a small span isn't forced to 8 steps. Cap
/// stays 512.
pub fn arc_segments_floored(radius: f64, span_abs: f64, chord_tol_world: f64, floor: u32) -> u32 {
    if span_abs < 1e-9 || radius < 1e-9 {
        return floor.max(1);
    }
    let tol = chord_tol_world.max(1e-9).min(radius * 0.99);
    // θ where r * (1 - cos(θ/2)) = tol  →  θ = 2 * acos(1 - tol/r).
    let max_step = (2.0 * (1.0 - tol / radius).acos()).max(1e-6);
    ((span_abs / max_step).ceil() as u32).clamp(floor, 512)
}

/// Chord tolerance for the load-time fill polygon: 0.1% of radius,
/// floor 1 µm so degenerate radii still produce a workable count.
pub fn fill_chord_tol(radius: f64) -> f64 {
    (radius * 0.001).max(1e-6)
}

/// Chord tolerance for the per-frame wire outline: pulls the active
/// `curve_tol::set_curve_tol_override` value (Scene sets it to
/// `world_per_pixel × 0.5` so curves stay at ~half-pixel chord error at
/// the current zoom). When no override is active (snap / hit-test
/// passes, load-time builds), falls back to [`fill_chord_tol`] so we
/// never under-sample.
pub fn wire_chord_tol(radius: f64) -> f64 {
    match crate::scene::convert::curve_tol::active_curve_tol() {
        Some(t) => t.min(fill_chord_tol(radius)).max(1e-9),
        None => fill_chord_tol(radius),
    }
}

// ── Colour helper ──────────────────────────────────────────────────────────

/// Convert an acadrust Color (ACI index or true-color) to a GPU RGBA value.
pub fn aci_to_rgba(color: &AcadColor) -> [f32; 4] {
    if let Some((r, g, b)) = color.rgb() {
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
    } else {
        WireModel::WHITE
    }
}
