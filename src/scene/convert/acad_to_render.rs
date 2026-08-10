// acadrust entity -> what the renderer draws.
//
// Every entity becomes one of a handful of shapes: a run of points, a run of
// text strokes, a band of varying width, or a single dot. Curved entities are
// sampled through `entities::curve`, which is where each one's geometry is
// defined once — so a circle drawn here and a circle extruded by the Model
// tab come from the same definition and cannot disagree.

use acadrust::{CadDocument, EntityType};

use crate::entities::traits::EntityTypeOps;
use crate::scene::model::wire_model::{SnapHint, TangentGeom};

/// One group of glyph strokes with its world-space origin stored in f64.
/// Strokes are in glyph-local space (origin = [0,0]) so that the large
/// world offset can be subtracted with f64 precision in tessellate.rs.
///
/// `color`, when set, overrides the entity colour for just this group — used
/// by MTEXT inline `\C` / `\c` per-run colour. Strokes sharing the same
/// (color, None) override are merged into one WireModel downstream; runs with
/// distinct colours emit their own WireModel.
pub struct TextStroke {
    pub strokes: Vec<Vec<[f32; 2]>>,
    pub origin: [f64; 2],
    pub color: Option<[f32; 3]>,
    pub fill_tris: Vec<[f32; 2]>,
    /// Layout inputs to rebuild this run as per-glyph SDF quads (see
    /// `scene::text::glyph_quads`). `Some` on runs wired for the SDF text
    /// renderer; `None` leaves the run to the stroke path only. Heights are
    /// raw (pre annotation-scale), matching `strokes` — the SDF collector
    /// applies annotation scale the same way `tessellate` does for strokes.
    pub run: Option<GlyphRun>,
}

/// Per-run text-layout inputs needed to reproduce a run as SDF glyph quads.
#[derive(Clone, Debug)]
pub struct GlyphRun {
    pub text: String,
    pub font: String,
    /// Raw height in drawing units (annotation scale applied later).
    pub height: f32,
    pub rotation: f32,
    pub width_factor: f32,
    pub oblique: f32,
    pub tracking: f32,
    /// Bold run — the SDF glyph bakes with a wider pen (thicker strokes).
    pub bold: bool,
}

#[allow(dead_code)]
pub enum RenderObject {
    /// A single position, drawn as a dot sized in pixels rather than in world
    /// units — what a PDMODE 0 point is.
    Dot([f64; 3]),
    Text(Vec<TextStroke>),
    /// Pre-computed NaN-separated 3-D point list (leader lines, arrowheads, etc.).
    /// Points are stored in WCS as **f64** so the large world_offset can be
    /// subtracted in full precision in tessellate.rs before the f32 cast.
    /// Casting WCS coordinates to f32 in the entity converters used to wreck
    /// rotated sub-glyph precision on drawings far from origin.
    Lines(Vec<[f64; 3]>),
    /// Like Lines but linetype pattern restarts at each NaN-separated segment (plinegen=false).
    SegmentedLines(Vec<[f64; 3]>),
    /// A wide polyline whose band width VARIES (a taper): a continuous WCS point
    /// list paired index-for-index with a per-point full band width. The wire
    /// shader interpolates the two endpoint widths of each segment so the band
    /// tapers smoothly. Points hold no NaN breaks (one continuous band).
    TaperedLines(Vec<[f64; 3]>, Vec<f32>),
}

pub struct RenderEntity {
    pub object: RenderObject,
    pub snap_pts: Vec<(glam::DVec3, SnapHint)>,
    pub tangent_geoms: Vec<TangentGeom>,
    /// Polyline vertex positions in WCS f64; converted to offset-relative f32
    /// at the wire-model boundary.
    pub key_vertices: Vec<[f64; 3]>,
    /// Pre-triangulated fill geometry: flat list of WCS f64 vertices, 3 per
    /// triangle. Non-empty for mesh-like entities (PolyfaceMesh, PolygonMesh)
    /// that need solid fill.
    pub fill_tris: Vec<[f64; 3]>,
    /// Pre-triangulated pick-only geometry: flat list of WCS f64 vertices, 3
    /// per triangle. Non-empty for entities carrying a DXF thickness (code 39)
    /// — the swept wall between each base segment and its extruded copy. Kept
    /// apart from `fill_tris` because only that one reaches the GPU; see
    /// [`WireModel::pick_tris`](crate::scene::model::wire_model::WireModel::pick_tris).
    pub pick_tris: Vec<[f64; 3]>,
}

pub fn convert(entity: &EntityType, document: &CadDocument) -> Option<RenderEntity> {
    entity.to_render_entity(document)
}

/// Triangulate the wall swept by extruding `base` along `extrusion` — the
/// geometry a DXF thickness (code 39) adds to an entity. Two triangles per
/// base segment, in `RenderEntity::pick_tris` order (flat WCS f64, 3 per tri).
///
/// `base` is a point chain in the same form a wire uses, so a NaN entry breaks
/// the sweep exactly as it breaks a segment chain — a polyline with disjoint
/// runs extrudes into one wall per run, not a wall bridging the gap.
pub fn extrusion_wall_tris(base: &[[f64; 3]], extrusion: [f64; 3]) -> Vec<[f64; 3]> {
    let top = |p: [f64; 3]| {
        [
            p[0] + extrusion[0],
            p[1] + extrusion[1],
            p[2] + extrusion[2],
        ]
    };
    let mut out = Vec::new();
    let mut prev: Option<[f64; 3]> = None;
    for &p in base {
        if p[0].is_nan() {
            prev = None;
            continue;
        }
        if let Some(a) = prev {
            out.extend_from_slice(&[a, p, top(p), a, top(p), top(a)]);
        }
        prev = Some(p);
    }
    out
}
