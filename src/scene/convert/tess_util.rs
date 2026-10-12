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
        let rgb = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
        // Only colour 7 swaps white/black with the background; any other
        // colour — a true colour, or ACI 255 — is drawn as authored.
        let [r, g, b] = if matches!(color, AcadColor::Index(7)) {
            rgb
        } else {
            authored_rgb(rgb)
        };
        [r, g, b, 1.0]
    } else {
        WireModel::WHITE
    }
}

/// Pure white and pure black are what the background adaptation swaps, and
/// only colour 7 may be swapped. A colour chosen as-is (255,255,255 used as a
/// white mask on a light sheet, say) is kept a hair off them so it is drawn
/// exactly as authored; the difference is below one display step.
pub fn authored_rgb(rgb: [f32; 3]) -> [f32; 3] {
    const OFF: f32 = 1.0 / 1024.0;
    if rgb == [1.0; 3] {
        [1.0 - OFF; 3]
    } else if rgb == [0.0; 3] {
        [OFF; 3]
    } else {
        rgb
    }
}

/// True for a colour [`authored_rgb`] moved off pure white.
pub fn is_authored_white(rgb: [f32; 3]) -> bool {
    rgb == authored_rgb([1.0; 3])
}
