//! XCLIP clip-boundary application for block references.
//!
//! A block reference (INSERT) may carry an `AcDbSpatialFilter` under its
//! extension dictionary (`ACAD_FILTER` → `SPATIAL`). When present and enabled,
//! only the portion of the block's geometry inside the boundary polygon is
//! drawn. This module resolves the filter, builds the world-space boundary
//! polygon, and clips the expanded block [`WireModel`]s to it.
//!
//! Geometry is assumed planar in the boundary's +Z plane — the standard XCLIP
//! case. The clip is performed in 2D (XY) after the INSERT transform has been
//! applied, matching the space the block wires are already emitted in.

use acadrust::entities::Insert;
use acadrust::objects::{ObjectType, SpatialFilter};
use acadrust::types::{Handle, Transform, Vector3};
use acadrust::CadDocument;
use cadkernel::geom2d::{contains, Curve, Line, Tolerance};

use crate::scene::model::wire_model::WireModel;

const NAN3: [f32; 3] = [f32::NAN, f32::NAN, f32::NAN];

/// Resolve the enabled XCLIP spatial filter for `ins`, if any.
///
/// Walks the INSERT's extension dictionary: `xdictionary → ACAD_FILTER
/// (dictionary) → SPATIAL (SpatialFilter)`. Returns `None` when there is no
/// filter, it is disabled (code 71 = 0), or it has fewer than two boundary
/// points.
pub fn insert_spatial_filter<'a>(
    doc: &'a CadDocument,
    ins: &Insert,
) -> Option<&'a SpatialFilter> {
    let xdict = ins.common.xdictionary_handle?;
    let acad_filter = dict_entry(doc, xdict, "ACAD_FILTER")?;
    let spatial = dict_entry(doc, acad_filter, "SPATIAL")?;
    match doc.objects.get(&spatial)? {
        ObjectType::SpatialFilter(sf)
            if sf.display_enabled && sf.boundary_points.len() >= 2 =>
        {
            Some(sf)
        }
        _ => None,
    }
}

fn dict_entry(doc: &CadDocument, dict: Handle, key: &str) -> Option<Handle> {
    match doc.objects.get(&dict)? {
        ObjectType::Dictionary(d) => {
            d.entries.iter().find(|(k, _)| k == key).map(|(_, h)| *h)
        }
        _ => None,
    }
}

/// Build the clip boundary as a closed world-space ring in the same f32 XY
/// space as the emitted wires (i.e. with `world_offset` already subtracted).
///
/// Boundary points are stored in the clip-definition coordinate system. The
/// `inverse_block_transform` maps them into the block's coordinate system, then
/// the INSERT transform maps that to world — `world = T_insert · (M⁻¹ · vert)`.
/// (For a clip made against the current insert, `M⁻¹` is the insert's own
/// inverse and the two transforms cancel, leaving the vertices in WCS; when the
/// insert was later rescaled the stored `M⁻¹` still places the clip correctly.)
/// Two points describe a rectangle (opposite corners); three or more an
/// explicit polygon.
/// Build the clip boundary in absolute f64 world coordinates so clipping at
/// large coordinate values remains precise.
#[cfg(test)]
fn world_clip_polygon_f64(
    sf: &SpatialFilter,
    ins: &Insert,
) -> Vec<[f64; 2]> {
    world_clip_polygon_for_transform(sf, &ins.get_transform())
}

/// Build the clip boundary with the exact transform used by the corresponding
/// block instance. Callers that resolve block base points or parent instances
/// pass their composed transform here so geometry and clipping share one space.
pub fn world_clip_polygon_for_transform(
    sf: &SpatialFilter,
    xform: &Transform,
) -> Vec<[f64; 2]> {
    let inv_block = &sf.inverse_block_transform;
    let local: Vec<[f64; 2]> = if sf.boundary_points.len() == 2 {
        let a = sf.boundary_points[0];
        let b = sf.boundary_points[1];
        vec![[a.x, a.y], [b.x, a.y], [b.x, b.y], [a.x, b.y]]
    } else {
        sf.boundary_points.iter().map(|p| [p.x, p.y]).collect()
    };
    local
        .into_iter()
        .map(|[x, y]| {
            let block = inv_block.transform_point(Vector3::new(x, y, 0.0));
            let w = xform.apply(block);
            [w.x, w.y]
        })
        .collect()
}

/// Clip every wire in `wires` to the boundary `poly` (a closed ring in f32 XY,
/// world_offset-subtracted). Polylines are split into NaN-separated inside
/// runs; fill triangles are clipped against the polygon; snap / key vertices
/// outside the boundary are dropped. Wires left with no geometry are removed.
pub fn clip_wires(wires: &mut Vec<WireModel>, poly: &[[f64; 2]]) {
    if poly.len() < 3 {
        return;
    }
    // Clip in a frame relative to the boundary's first vertex: the wire points
    // arrive as absolute coordinates (double-single high+low), which at UTM
    // scale are ~5.7e6 and lose ~0.5 m in f32. Subtracting the f64 reference
    // makes every coordinate small, so the f32 Sutherland–Hodgman math is exact;
    // the reference is added back afterwards and re-split into the high/low pair
    // the relative-to-eye renderer expects.
    let (rx, ry) = (poly[0][0], poly[0][1]);
    let lpoly: Vec<[f32; 2]> = poly
        .iter()
        .map(|&[x, y]| [(x - rx) as f32, (y - ry) as f32])
        .collect();
    let boundary: Vec<Curve> = poly
        .iter()
        .zip(poly.iter().cycle().skip(1))
        .take(poly.len())
        .map(|(&start, &end)| Curve::Line(Line { start, end }))
        .collect();

    // Reconstruct an absolute-f64 wire point from its high/low pair, NaN-safe.
    let abs = |hi: [f32; 3], lo: [f32; 3]| -> [f64; 3] {
        [
            hi[0] as f64 + lo[0] as f64,
            hi[1] as f64 + lo[1] as f64,
            hi[2] as f64 + lo[2] as f64,
        ]
    };

    for w in wires.iter_mut() {
        let keep_marker = w.point_marker.map(|marker| {
            contains(
                &boundary,
                [marker.origin.x, marker.origin.y],
                Tolerance::default(),
            )
        });
        if keep_marker == Some(false) {
            w.points.clear();
            w.points_low.clear();
            w.point_marker = None;
        } else if keep_marker.is_none() && !w.points.is_empty() {
            // Absolute → local f32 (NaN separators preserved).
            let local: Vec<[f32; 3]> = (0..w.points.len())
                .map(|i| {
                    let hi = w.points[i];
                    if !hi[0].is_finite() || !hi[1].is_finite() {
                        return NAN3;
                    }
                    let lo = w.points_low.get(i).copied().unwrap_or([0.0; 3]);
                    let a = abs(hi, lo);
                    [(a[0] - rx) as f32, (a[1] - ry) as f32, a[2] as f32]
                })
                .collect();
            let clipped = clip_polyline(&local, &lpoly);
            // Local → absolute, re-split into double-single high/low.
            let mut hi = Vec::with_capacity(clipped.len());
            let mut lo = Vec::with_capacity(clipped.len());
            for p in clipped {
                if !p[0].is_finite() || !p[1].is_finite() {
                    hi.push(NAN3);
                    lo.push([0.0; 3]);
                    continue;
                }
                let (hx, lx) = split_ds(p[0] as f64 + rx);
                let (hy, ly) = split_ds(p[1] as f64 + ry);
                let (hz, lz) = split_ds(p[2] as f64);
                hi.push([hx, hy, hz]);
                lo.push([lx, ly, lz]);
            }
            w.points = hi;
            w.points_low = lo;
        }
        if !w.fill_tris.is_empty() {
            let local: Vec<[f32; 3]> = (0..w.fill_tris.len())
                .map(|i| {
                    let hi = w.fill_tris[i];
                    let lo = w.fill_tris_low.get(i).copied().unwrap_or([0.0; 3]);
                    let a = abs(hi, lo);
                    [(a[0] - rx) as f32, (a[1] - ry) as f32, a[2] as f32]
                })
                .collect();
            let clipped = clip_triangles(&local, &lpoly);
            let mut hi = Vec::with_capacity(clipped.len());
            let mut lo = Vec::with_capacity(clipped.len());
            for p in clipped {
                let (hx, lx) = split_ds(p[0] as f64 + rx);
                let (hy, ly) = split_ds(p[1] as f64 + ry);
                let (hz, lz) = split_ds(p[2] as f64);
                hi.push([hx, hy, hz]);
                lo.push([lx, ly, lz]);
            }
            w.fill_tris = hi;
            w.fill_tris_low = lo;
        }
        // Clip SDF text: keep each glyph quad (6 verts = 2 triangles) whose
        // centroid is inside the boundary. Whole-glyph granularity so block-
        // instance text is bounded by the xclip region instead of bleeding past
        // it — the stroke path clips per-segment, but per-glyph reads cleanly.
        if !w.text_verts.is_empty() {
            let mut kept = Vec::with_capacity(w.text_verts.len());
            for quad in w.text_verts.chunks(6) {
                let n = quad.len() as f64;
                let (mut cx, mut cy) = (0.0f64, 0.0f64);
                for v in quad {
                    cx += v.pos[0] as f64 + v.pos_low[0] as f64;
                    cy += v.pos[1] as f64 + v.pos_low[1] as f64;
                }
                if point_in_poly((cx / n - rx) as f32, (cy / n - ry) as f32, &lpoly) {
                    kept.extend_from_slice(quad);
                }
            }
            w.text_verts = kept;
        }
        w.key_vertices
            .retain(|v| point_in_poly((v[0] - rx) as f32, (v[1] - ry) as f32, &lpoly));
        w.snap_pts
            .retain(|(p, _)| point_in_poly((p.x - rx) as f32, (p.y - ry) as f32, &lpoly));
        // AABB from surviving points + fills, then fold in the surviving glyph
        // quads so a text-only wire keeps a tight (non-degenerate) pick box.
        let mut bb = recompute_aabb(&w.points, &w.fill_tris);
        if !w.text_verts.is_empty() {
            let (mut nx, mut ny, mut xx, mut xy) = if bb == [0.0; 4] {
                (f32::MAX, f32::MAX, f32::MIN, f32::MIN)
            } else {
                (bb[0], bb[1], bb[2], bb[3])
            };
            for v in &w.text_verts {
                nx = nx.min(v.pos[0]);
                ny = ny.min(v.pos[1]);
                xx = xx.max(v.pos[0]);
                xy = xy.max(v.pos[1]);
            }
            if nx <= xx {
                bb = [nx, ny, xx, xy];
            }
        }
        w.aabb = bb;
    }
    wires.retain(|w| {
        !w.points.is_empty() || !w.fill_tris.is_empty() || !w.text_verts.is_empty()
    });
}

/// Double-single split of an f64 into (high, low) f32 — mirrors
/// `WireModel::split_ds` so clipped points match the renderer's reconstruction.
fn split_ds(v: f64) -> (f32, f32) {
    let high = v as f32;
    (high, (v - high as f64) as f32)
}

/// Build the visible clip-boundary frame (XCLIPFRAME 1/2) as a closed ring
/// over the world-space boundary polygon. The frame is display chrome for the
/// clipped INSERT: it rides the insert's resolved colour / line weight and is
/// deliberately NOT clipped — it *is* the boundary.
pub fn frame_wire(
    poly: &[[f64; 2]],
    name: String,
    color: [f32; 4],
    selected: bool,
    line_weight_px: f32,
) -> WireModel {
    let mut hi = Vec::with_capacity(poly.len() + 1);
    let mut lo = Vec::with_capacity(poly.len() + 1);
    for &[x, y] in poly.iter().chain(std::iter::once(&poly[0])) {
        let (hx, lx) = split_ds(x);
        let (hy, ly) = split_ds(y);
        hi.push([hx, hy, 0.0]);
        lo.push([lx, ly, 0.0]);
    }
    WireModel {
        point_marker: None,
        taper_widths: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        plot_visible: true,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        render_instance: None,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
        dash_from_start: false,
        dash_align_end: None,
        text_verts: Vec::new(),
        name,
        points: hi,
        points_low: lo,
        color,
        selected,
        aci: 0,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px,
        snap_pts: vec![],
        tangent_geoms: vec![],
        key_vertices: vec![],
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}

/// Clip a hatch fill boundary to `poly`.
///
/// `boundary` is a hatch's NaN-separated loops in f32 offsets from
/// `world_origin` (the [`HatchModel`](crate::scene::model::hatch_model::HatchModel)
/// representation); `poly` is the clip ring in the same world space the hatch
/// occupies. Each loop is intersected with the clip polygon independently —
/// the even-odd island structure is preserved because intersecting every loop
/// with the same region distributes over the even-odd fill. Returns the
/// clipped loops in offsets from `world_origin`; empty if nothing survives.
pub fn clip_hatch_boundary(
    boundary: &[[f32; 2]],
    world_origin: [f64; 2],
    poly: &[[f32; 2]],
) -> Vec<[f32; 2]> {
    if poly.len() < 3 {
        return boundary.to_vec();
    }
    let (ox, oy) = (world_origin[0] as f32, world_origin[1] as f32);
    let mut out: Vec<[f32; 2]> = Vec::new();
    let mut i = 0;
    while i < boundary.len() {
        if !boundary[i][0].is_finite() || !boundary[i][1].is_finite() {
            i += 1;
            continue;
        }
        let start = i;
        while i < boundary.len()
            && boundary[i][0].is_finite()
            && boundary[i][1].is_finite()
        {
            i += 1;
        }
        // Lift the loop into the clip polygon's world space.
        let loop_abs: Vec<[f32; 2]> = boundary[start..i]
            .iter()
            .map(|p| [p[0] + ox, p[1] + oy])
            .collect();
        let clipped = clip_polygon_2d(&loop_abs, poly);
        if clipped.len() >= 3 {
            if !out.is_empty() {
                out.push([f32::NAN, f32::NAN]);
            }
            // Lower back into the loop's `world_origin`-relative space so the
            // result keeps the same f32-precision anchor the renderer adds back.
            for p in clipped {
                out.push([p[0] - ox, p[1] - oy]);
            }
        }
    }
    out
}

/// Sutherland–Hodgman clip of a closed 2D polygon `subject` against the convex
/// polygon `clip`.
fn clip_polygon_2d(subject: &[[f32; 2]], clip: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let n = clip.len();
    let mut area2 = 0.0f32;
    let mut j = n - 1;
    for i in 0..n {
        area2 += clip[j][0] * clip[i][1] - clip[i][0] * clip[j][1];
        j = i;
    }
    let ccw = area2 > 0.0;

    let mut output: Vec<[f32; 2]> = subject.to_vec();
    let mut j = n - 1;
    for i in 0..n {
        if output.is_empty() {
            break;
        }
        let (a, b) = (clip[j], clip[i]);
        j = i;
        let inside = |p: &[f32; 2]| {
            let cr = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
            if ccw {
                cr >= 0.0
            } else {
                cr <= 0.0
            }
        };
        let input = std::mem::take(&mut output);
        let len = input.len();
        for k in 0..len {
            let cur = input[k];
            let prev = input[(k + len - 1) % len];
            let cur_in = inside(&cur);
            let prev_in = inside(&prev);
            if cur_in {
                if !prev_in {
                    output.push(line_cross_2d(prev, cur, a, b));
                }
                output.push(cur);
            } else if prev_in {
                output.push(line_cross_2d(prev, cur, a, b));
            }
        }
    }
    output
}

fn line_cross_2d(p0: [f32; 2], p1: [f32; 2], a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let r = (p1[0] - p0[0], p1[1] - p0[1]);
    let s = (b[0] - a[0], b[1] - a[1]);
    let denom = r.0 * s.1 - r.1 * s.0;
    let t = if denom.abs() < 1e-12 {
        0.0
    } else {
        ((a[0] - p0[0]) * s.1 - (a[1] - p0[1]) * s.0) / denom
    };
    [p0[0] + r.0 * t, p0[1] + r.1 * t]
}

/// Ray-cast point-in-polygon test for a closed ring.
fn point_in_poly(x: f32, y: f32, poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (poly[i][0], poly[i][1]);
        let (xj, yj) = (poly[j][0], poly[j][1]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Clip a NaN-separated polyline to `poly`, returning a NaN-separated polyline
/// of only the inside portions.
fn clip_polyline(pts: &[[f32; 3]], poly: &[[f32; 2]]) -> Vec<[f32; 3]> {
    let mut out: Vec<[f32; 3]> = Vec::new();
    let mut i = 0;
    while i < pts.len() {
        if !pts[i][0].is_finite() || !pts[i][1].is_finite() {
            i += 1;
            continue;
        }
        let start = i;
        while i < pts.len() && pts[i][0].is_finite() && pts[i][1].is_finite() {
            i += 1;
        }
        let seg = &pts[start..i];
        let mut last: Option<[f32; 3]> = None;
        for j in 0..seg.len().saturating_sub(1) {
            for (a, b) in clip_segment(seg[j], seg[j + 1], poly) {
                let contiguous = last.is_some_and(|l| {
                    (l[0] - a[0]).abs() <= 1e-4 && (l[1] - a[1]).abs() <= 1e-4
                });
                if !contiguous {
                    if !out.is_empty() {
                        out.push(NAN3);
                    }
                    out.push(a);
                }
                out.push(b);
                last = Some(b);
            }
        }
    }
    out
}

/// Return the inside-the-polygon sub-segments of `p0`→`p1` as endpoint pairs.
/// Handles convex and concave boundaries by testing the midpoint of every
/// interval between consecutive boundary crossings.
fn clip_segment(
    p0: [f32; 3],
    p1: [f32; 3],
    poly: &[[f32; 2]],
) -> Vec<([f32; 3], [f32; 3])> {
    let mut ts: Vec<f32> = vec![0.0, 1.0];
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        if let Some(t) = seg_cross_t(p0, p1, poly[j], poly[i]) {
            if t > 0.0 && t < 1.0 {
                ts.push(t);
            }
        }
        j = i;
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-7);

    let lerp = |t: f32| {
        [
            p0[0] + (p1[0] - p0[0]) * t,
            p0[1] + (p1[1] - p0[1]) * t,
            p0[2] + (p1[2] - p0[2]) * t,
        ]
    };
    let mut out = Vec::new();
    for w in ts.windows(2) {
        let mid = lerp(0.5 * (w[0] + w[1]));
        if point_in_poly(mid[0], mid[1], poly) {
            out.push((lerp(w[0]), lerp(w[1])));
        }
    }
    out
}

/// Parameter `t` along segment `p0`→`p1` where it crosses the boundary edge
/// `a`→`b`, or `None` if they do not cross within the edge's extent.
fn seg_cross_t(p0: [f32; 3], p1: [f32; 3], a: [f32; 2], b: [f32; 2]) -> Option<f32> {
    let r = (p1[0] - p0[0], p1[1] - p0[1]);
    let s = (b[0] - a[0], b[1] - a[1]);
    let denom = r.0 * s.1 - r.1 * s.0;
    if denom.abs() < 1e-12 {
        return None;
    }
    let qp = (a[0] - p0[0], a[1] - p0[1]);
    let t = (qp.0 * s.1 - qp.1 * s.0) / denom;
    let u = (qp.0 * r.1 - qp.1 * r.0) / denom;
    if (0.0..=1.0).contains(&u) {
        Some(t)
    } else {
        None
    }
}

/// Clip a flat triangle list against `poly` (Sutherland–Hodgman per triangle,
/// fan-triangulating the clipped convex result). Exact for convex boundaries;
/// approximate for concave ones.
fn clip_triangles(tris: &[[f32; 3]], poly: &[[f32; 2]]) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    for tri in tris.chunks_exact(3) {
        let clipped = sutherland_hodgman(tri, poly);
        for k in 1..clipped.len().saturating_sub(1) {
            out.push(clipped[0]);
            out.push(clipped[k]);
            out.push(clipped[k + 1]);
        }
    }
    out
}

fn sutherland_hodgman(tri: &[[f32; 3]], poly: &[[f32; 2]]) -> Vec<[f32; 3]> {
    // Boundary orientation decides which half-plane is "inside".
    let mut area2 = 0.0f32;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        area2 += poly[j][0] * poly[i][1] - poly[i][0] * poly[j][1];
        j = i;
    }
    let ccw = area2 > 0.0;

    let mut output: Vec<[f32; 3]> = tri.to_vec();
    let mut j = n - 1;
    for i in 0..n {
        if output.is_empty() {
            break;
        }
        let (a, b) = (poly[j], poly[i]);
        j = i;
        let inside = |p: &[f32; 3]| {
            let cr = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
            if ccw {
                cr >= 0.0
            } else {
                cr <= 0.0
            }
        };
        let input = std::mem::take(&mut output);
        let len = input.len();
        for k in 0..len {
            let cur = input[k];
            let prev = input[(k + len - 1) % len];
            let cur_in = inside(&cur);
            let prev_in = inside(&prev);
            if cur_in {
                if !prev_in {
                    output.push(line_cross(prev, cur, a, b));
                }
                output.push(cur);
            } else if prev_in {
                output.push(line_cross(prev, cur, a, b));
            }
        }
    }
    output
}

/// Intersection of segment `p0`→`p1` with the infinite line through `a`→`b`,
/// interpolating Z.
fn line_cross(p0: [f32; 3], p1: [f32; 3], a: [f32; 2], b: [f32; 2]) -> [f32; 3] {
    let r = (p1[0] - p0[0], p1[1] - p0[1]);
    let s = (b[0] - a[0], b[1] - a[1]);
    let denom = r.0 * s.1 - r.1 * s.0;
    let t = if denom.abs() < 1e-12 {
        0.0
    } else {
        ((a[0] - p0[0]) * s.1 - (a[1] - p0[1]) * s.0) / denom
    };
    [
        p0[0] + r.0 * t,
        p0[1] + r.1 * t,
        p0[2] + (p1[2] - p0[2]) * t,
    ]
}

fn recompute_aabb(points: &[[f32; 3]], tris: &[[f32; 3]]) -> [f32; 4] {
    let mut bb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    let mut any = false;
    for p in points.iter().chain(tris.iter()) {
        if p[0].is_finite() && p[1].is_finite() {
            bb[0] = bb[0].min(p[0]);
            bb[1] = bb[1].min(p[1]);
            bb[2] = bb[2].max(p[0]);
            bb[3] = bb[3].max(p[1]);
            any = true;
        }
    }
    if any {
        bb
    } else {
        [0.0; 4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<[f32; 2]> {
        vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
    }

    #[test]
    fn point_in_poly_basic() {
        let p = square();
        assert!(point_in_poly(5.0, 5.0, &p));
        assert!(!point_in_poly(15.0, 5.0, &p));
        assert!(!point_in_poly(-1.0, 5.0, &p));
    }

    #[test]
    fn segment_clipped_to_boundary() {
        // Horizontal line crossing the square from outside to outside.
        let segs = clip_segment([-5.0, 5.0, 0.0], [15.0, 5.0, 0.0], &square());
        assert_eq!(segs.len(), 1);
        let (a, b) = segs[0];
        assert!((a[0] - 0.0).abs() < 1e-3);
        assert!((b[0] - 10.0).abs() < 1e-3);
    }

    #[test]
    fn segment_fully_outside_dropped() {
        let segs = clip_segment([20.0, 5.0, 0.0], [30.0, 5.0, 0.0], &square());
        assert!(segs.is_empty());
    }

    #[test]
    fn polyline_keeps_inside_run() {
        // Polyline that dips outside then comes back: expect a NaN break.
        let pts = vec![
            [5.0, 5.0, 0.0],
            [15.0, 5.0, 0.0],
            [15.0, 8.0, 0.0],
            [5.0, 8.0, 0.0],
        ];
        let out = clip_polyline(&pts, &square());
        assert!(out.iter().any(|p| p[0].is_nan()));
        assert!(out.iter().all(|p| p[0].is_nan() || p[0] <= 10.0 + 1e-3));
    }

    #[test]
    fn resolves_filter_and_clips_block_geometry() {
        use acadrust::objects::Dictionary;
        use acadrust::types::Vector2;

        // Handles: insert, xdict, acad_filter dict, spatial filter.
        let (h_ins, h_xdict, h_filter, h_spatial) = (
            Handle::new(0x10),
            Handle::new(0x11),
            Handle::new(0x12),
            Handle::new(0x13),
        );

        let mut doc = CadDocument::new();

        // xdictionary → ACAD_FILTER → SPATIAL chain.
        let mut xdict = Dictionary::new();
        xdict.handle = h_xdict;
        xdict.add_entry("ACAD_FILTER", h_filter);
        doc.objects.insert(h_xdict, ObjectType::Dictionary(xdict));

        let mut filter_dict = Dictionary::new();
        filter_dict.handle = h_filter;
        filter_dict.add_entry("SPATIAL", h_spatial);
        doc.objects
            .insert(h_filter, ObjectType::Dictionary(filter_dict));

        let mut sf = SpatialFilter::new();
        sf.handle = h_spatial;
        sf.display_enabled = true;
        sf.boundary_points = vec![Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0)];
        doc.objects.insert(h_spatial, ObjectType::SpatialFilter(sf));

        // Identity-transform insert (origin, unit scale, no rotation).
        let mut ins = Insert::new("BLK", Vector3::new(0.0, 0.0, 0.0));
        ins.common.handle = h_ins;
        ins.common.xdictionary_handle = Some(h_xdict);

        let resolved = insert_spatial_filter(&doc, &ins).expect("filter resolves");
        let poly = world_clip_polygon_f64(resolved, &ins);
        assert_eq!(poly.len(), 4);

        // A polyline half inside, half outside the 0..10 square.
        let mut wires = vec![WireModel {
            text_verts: Vec::new(),
            points: vec![[5.0, 5.0, 0.0], [15.0, 5.0, 0.0]],
            ..Default::default()
        }];
        clip_wires(&mut wires, &poly);

        assert_eq!(wires.len(), 1);
        let pts = &wires[0].points;
        assert!(pts.iter().all(|p| p[0].is_nan() || p[0] <= 10.0 + 1e-3));
        assert!(pts.iter().any(|p| (p[0] - 10.0).abs() < 1e-3));
    }

    #[test]
    fn hatch_boundary_clipped_kept_and_dropped() {
        let clip = square(); // 0..10
        // Hatch loop straddling the right edge → clipped to x<=10.
        let straddle = [[5.0, 5.0], [15.0, 5.0], [15.0, 8.0], [5.0, 8.0]];
        let out = clip_hatch_boundary(&straddle, [0.0, 0.0], &clip);
        assert!(!out.is_empty());
        assert!(out.iter().all(|p| p[0].is_nan() || p[0] <= 10.0 + 1e-3));

        // Hatch fully inside → unchanged vertex count.
        let inside = [[1.0, 1.0], [4.0, 1.0], [4.0, 4.0], [1.0, 4.0]];
        let out_in = clip_hatch_boundary(&inside, [0.0, 0.0], &clip);
        assert_eq!(out_in.len(), 4);

        // Hatch fully outside → dropped.
        let outside = [[20.0, 20.0], [25.0, 20.0], [25.0, 25.0]];
        let out_out = clip_hatch_boundary(&outside, [0.0, 0.0], &clip);
        assert!(out_out.is_empty());
    }

    #[test]
    fn hatch_boundary_respects_world_origin() {
        // Same geometry as the straddle case but expressed as offsets from a
        // large world_origin — clipping must account for the origin shift.
        let clip = vec![[1000.0, 1000.0], [1010.0, 1000.0], [1010.0, 1010.0], [1000.0, 1010.0]];
        let origin = [1000.0, 1000.0];
        let loop_off = [[5.0, 5.0], [15.0, 5.0], [15.0, 8.0], [5.0, 8.0]];
        let out = clip_hatch_boundary(&loop_off, origin, &clip);
        assert!(!out.is_empty());
        // Offsets must stay <= 10 (i.e. world x <= 1010).
        assert!(out.iter().all(|p| p[0].is_nan() || p[0] <= 10.0 + 1e-3));
    }

    #[test]
    fn world_polygon_applies_inverse_block_then_insert() {
        use acadrust::types::{Matrix4, Vector2};
        // Clip stored against a normalized space: inverse_block_transform scales
        // the small boundary points up by 1000 into block space, then the insert
        // (scale 0.1 + translation) maps them to world.
        let mut sf = SpatialFilter::new();
        sf.boundary_points = vec![Vector2::new(580.0, 4528.0), Vector2::new(581.0, 4529.0)];
        sf.inverse_block_transform = Matrix4 {
            m: [
                [1000.0, 0.0, 0.0, 0.0],
                [0.0, 1000.0, 0.0, 0.0],
                [0.0, 0.0, 1000.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let mut ins = Insert::new("BLK", Vector3::new(581668.0, 4064155.0, 0.0));
        ins.set_x_scale(0.1);
        ins.set_y_scale(0.1);

        let poly = world_clip_polygon_f64(&sf, &ins);
        // vert (580,4528) → ×1000 → (580000,4528000) → ×0.1 + insert →
        // (639668, 4516955).
        let xs: Vec<f64> = poly.iter().map(|p| p[0]).collect();
        let ys: Vec<f64> = poly.iter().map(|p| p[1]).collect();
        let minx = xs.iter().cloned().fold(f64::MAX, f64::min);
        let miny = ys.iter().cloned().fold(f64::MAX, f64::min);
        assert!((minx - 639668.0).abs() < 1.0, "minx={minx}");
        assert!((miny - 4516955.0).abs() < 1.0, "miny={miny}");
    }

    #[test]
    fn triangle_clipped_to_square() {
        // Triangle straddling the right edge → clipped, area reduced.
        let tri = [[5.0, 5.0, 0.0], [15.0, 5.0, 0.0], [5.0, 9.0, 0.0]];
        let out = clip_triangles(&tri, &square());
        assert!(!out.is_empty());
        assert!(out.iter().all(|p| p[0] <= 10.0 + 1e-3));
    }
}
