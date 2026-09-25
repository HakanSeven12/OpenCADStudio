// Shared helpers used by per-entity tessellation impls in `crate::entities`
// and the dispatcher in `crate::scene::convert::tessellate`.
//
// Cross-entity rendering helpers live here.

use codec::types::Color as AcadColor;
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

// ── Colour helper ──────────────────────────────────────────────────────────

/// Convert an opencadcodec Color (ACI index or true-color) to a GPU RGBA value.
pub fn aci_to_rgba(color: &AcadColor) -> [f32; 4] {
    if let Some((r, g, b)) = color.rgb() {
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
    } else {
        WireModel::WHITE
    }
}
