// opencadcodec entity -> what the renderer draws.
//
// Every entity becomes one of a handful of shapes: a run of points, a run of
// text strokes, a band of varying width, or a single dot. Curved entities are
// sampled through `entities::curve`, which is where each one's geometry is
// defined once — so a circle drawn here and a circle extruded by the Model
// tab come from the same definition and cannot disagree.

use codec::{Document, EntityType};

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
    pub plane: Option<TextPlane>,
    /// Layout inputs to rebuild this run as per-glyph SDF quads (see
    /// `scene::text::glyph_quads`). `Some` on runs wired for the SDF text
    /// renderer; `None` leaves the run to the stroke path only. Heights are
    /// raw (pre annotation-scale), matching `strokes` — the SDF collector
    /// applies annotation scale the same way `tessellate` does for strokes.
    pub run: Option<GlyphRun>,
}

#[derive(Clone, Copy, Debug)]
pub struct TextPlane {
    pub origin: [f64; 3],
    pub scale_origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
}

/// The plane a run lies in when its entity's normal is not the drawing's
/// Z axis: the run's origin (in the entity's object coordinates, at
/// elevation `z`) and the object system's X and Y axes, in world space.
/// `None` for a run in the XY plane, which is drawn as it is.
pub fn ocs_text_plane(
    normal: codec::types::Vector3,
    origin: [f64; 2],
    z: f64,
) -> Option<TextPlane> {
    if normal.x.abs() < 1e-12 && normal.y.abs() < 1e-12 && normal.z > 0.0 {
        return None;
    }
    let m = codec::types::Matrix3::arbitrary_axis(normal);
    let world = |v: codec::types::Vector3| {
        let w = m * v;
        [w.x, w.y, w.z]
    };
    let at = world(codec::types::Vector3::new(origin[0], origin[1], z));
    Some(TextPlane {
        origin: at,
        scale_origin: at,
        x_axis: world(codec::types::Vector3::new(1.0, 0.0, 0.0)),
        y_axis: world(codec::types::Vector3::new(0.0, 1.0, 0.0)),
    })
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
    /// The run's inline colour when given as an index (`\C`), which the
    /// stroke colour alone cannot tell from a true colour.
    pub aci: Option<u8>,
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
    /// Kernel band boundary with centreline-based linetype stations.
    BoundaryLines {
        points: Vec<[f64; 3]>,
        stations: Vec<f32>,
        point_segments: Vec<i32>,
        station_pieces: Vec<crate::scene::model::wire_model::PatternStationPiece>,
        source_length: f32,
        plinegen: bool,
        /// Each NaN-separated run of `points` is a line of its own: it takes
        /// the linetype's end alignment by itself, as a single line does.
        separate_runs: bool,
        /// NaN-separated lines drawn Continuous beside the patterned ones.
        solid: Vec<[f64; 3]>,
    },
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

pub fn convert(entity: &EntityType, document: &Document) -> Option<RenderEntity> {
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
