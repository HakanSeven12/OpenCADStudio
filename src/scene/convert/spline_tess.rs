// B-spline (NURBS) surface tessellation for ACIS `spline-surface` faces.
//
// Lofted / swept / revolved surfaces store their geometry as an ACIS
// `nubs` (non-uniform B-spline) block inside the `spline-surface` record.
// Rather than evaluate the basis functions by hand, we parse the control net
// and knot vectors out of the SAT tokens, hand them to the kernel's
// `BSplineSurface` (the same NURBS kernel the Model tab already builds on),
// and sample its parametric grid into triangles.

use acadrust::entities::acis::types::Sense;
use acadrust::entities::acis::{
    SatCoedge, SatDocument, SatFace, SatLoop, SatPCurve, SatRecord, SatSplineSurface, SatToken,
};
use rustc_hash::FxHashSet;
use acadrust::kernel::space::NurbsSurface3;

use crate::scene::convert::solid3d_tess::LodConfig;

// A `spline-surface` block is either a non-rational `nubs`, whose control
// points are plain xyz, or a rational `nurbs`, whose carry a weight. The
// kernel's surface holds both — weights absent means polynomial — so there is
// nothing here to tell apart.

/// Tessellate one `spline-surface` face by sampling its B-spline surface.
/// Appends triangles to the shared mesh buffers; a no-op when the surface
/// record can't be parsed into a B-spline.
pub fn tess_spline_face(
    sat: &SatDocument,
    face: &SatFace,
    lod: LodConfig,
    verts: &mut Vec<[f64; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) -> bool {
    let Some(surf_rec) = sat.resolve(face.surface()) else {
        return false;
    };
    let Some(surface) = build_spline_surface(sat, surf_rec) else {
        return false;
    };

    // Sample over the knot domain and clip cells against ACIS pcurves when the
    // face carries a parametric trim. This preserves holes and non-rectangular
    // spline faces instead of always filling the complete UV rectangle.
    let ((u0, u1), (v0, v1)) = surface.domain();
    if !(u1 > u0) || !(v1 > v0) {
        return false;
    }

    // A B-spline patch has no single analytic radius to drive a chord-tolerance
    // count, so sample at the LOD's nominal density (its unit-circle segment
    // count). Floor 8 so a curved patch stays smooth.
    let n = crate::scene::convert::solid3d_tess::nominal_segs(lod.chord_frac).max(8);
    let (su, sv) = (n, n);
    let trim_loops = collect_trim_loops(sat, face, n);
    let reversed = matches!(face.sense(), Sense::Reversed);
    let index_start = indices.len();

    let base = verts.len() as u32;
    for j in 0..=sv {
        let v = v0 + (v1 - v0) * (j as f64 / sv as f64);
        for i in 0..=su {
            let u = u0 + (u1 - u0) * (i as f64 / su as f64);
            let p = surface.point_at_knot(u, v);
            // A pole, or a row of coincident control points, has no plane to
            // be perpendicular to. Up is as good an answer as any there and
            // better than an invented one, since the patch has no area at
            // that point to shade.
            let mut n = surface.normal_at_knot(u, v).unwrap_or([0.0, 0.0, 1.0]);
            if reversed {
                n = [-n[0], -n[1], -n[2]];
            }
            verts.push(p);
            normals.push([n[0] as f32, n[1] as f32, n[2] as f32]);
        }
    }

    let row = (su + 1) as u32;
    for j in 0..sv as u32 {
        for i in 0..su as u32 {
            if let Some(loops) = trim_loops.as_ref() {
                let u = u0 + (u1 - u0) * ((i as f64 + 0.5) / su as f64);
                let v = v0 + (v1 - v0) * ((j as f64 + 0.5) / sv as f64);
                if !inside_trim((u, v), loops, (u0, u1, v0, v1)) {
                    continue;
                }
            }
            let a = base + j * row + i;
            let b = a + 1;
            let c = a + row;
            let d = c + 1;
            if reversed {
                indices.extend_from_slice(&[a, d, b, a, c, d]);
            } else {
                indices.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
    }
    indices.len() > index_start
}

/// Collect complete face-loop pcurves in UV space. Missing pcurves disable
/// clipping for that face; a partial trim would be worse than the old full
/// patch fallback.
fn collect_trim_loops(
    sat: &SatDocument,
    face: &SatFace,
    segments: usize,
) -> Option<Vec<Vec<(f64, f64)>>> {
    let mut result = Vec::new();
    let mut loop_ptr = face.first_loop();
    let mut seen_loops = FxHashSet::default();
    while !loop_ptr.is_null() && seen_loops.insert(loop_ptr.0) {
        let sat_loop = SatLoop::from_record(sat.resolve(loop_ptr)?)?;
        let first = sat_loop.first_coedge();
        let mut coedge_ptr = first;
        let mut seen_coedges = FxHashSet::default();
        let mut polygon: Vec<(f64, f64)> = Vec::new();
        while !coedge_ptr.is_null() && seen_coedges.insert(coedge_ptr.0) {
            let coedge = SatCoedge::from_record(sat.resolve(coedge_ptr)?)?;
            let pcurve = SatPCurve::from_record(sat.resolve(coedge.pcurve())?)?;
            let mut points = pcurve.sample_in(sat, segments);
            if points.len() < 2 {
                return None;
            }
            if matches!(coedge.sense(), Sense::Reversed) {
                points.reverse();
            }
            if let Some(&last) = polygon.last() {
                let first_gap =
                    (last.0 - points[0].0).powi(2) + (last.1 - points[0].1).powi(2);
                let end = points[points.len() - 1];
                let last_gap = (last.0 - end.0).powi(2) + (last.1 - end.1).powi(2);
                if last_gap < first_gap {
                    points.reverse();
                }
            }
            points.pop();
            polygon.extend(points);
            coedge_ptr = coedge.next();
            if coedge_ptr == first {
                break;
            }
        }
        if polygon.len() < 3 {
            return None;
        }
        result.push(polygon);
        loop_ptr = sat_loop.next_loop();
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn inside_trim(
    point: (f64, f64),
    loops: &[Vec<(f64, f64)>],
    domain: (f64, f64, f64, f64),
) -> bool {
    let domain_area = ((domain.1 - domain.0) * (domain.3 - domain.2)).abs();
    let area_epsilon = domain_area.max(1.0) * 1e-10;
    let periodic_boundary = loops.iter().any(|polygon| {
        if polygon_area(polygon).abs() > area_epsilon {
            return false;
        }
        let bounds = polygon_bounds(polygon);
        let u_span = (bounds[2] - bounds[0]).abs();
        let v_span = (bounds[3] - bounds[1]).abs();
        u_span >= (domain.1 - domain.0).abs() * 0.9
            || v_span >= (domain.3 - domain.2).abs() * 0.9
    });
    if periodic_boundary {
        return loops.iter().all(|polygon| {
            polygon_area(polygon).abs() <= area_epsilon
                || !point_in_polygon(point, polygon)
        });
    }

    let Some((outer_index, _)) = loops
        .iter()
        .enumerate()
        .map(|(index, polygon)| (index, polygon_area(polygon).abs()))
        .max_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return true;
    };
    point_in_polygon(point, &loops[outer_index])
        && loops
            .iter()
            .enumerate()
            .all(|(index, polygon)| index == outer_index || !point_in_polygon(point, polygon))
}

fn polygon_bounds(polygon: &[(f64, f64)]) -> [f64; 4] {
    polygon.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |mut bounds, &(u, v)| {
            bounds[0] = bounds[0].min(u);
            bounds[1] = bounds[1].min(v);
            bounds[2] = bounds[2].max(u);
            bounds[3] = bounds[3].max(v);
            bounds
        },
    )
}

fn polygon_area(polygon: &[(f64, f64)]) -> f64 {
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
        .map(|(&(ax, ay), &(bx, by))| ax * by - bx * ay)
        .sum::<f64>()
        * 0.5
}

fn point_in_polygon(point: (f64, f64), polygon: &[(f64, f64)]) -> bool {
    let (x, y) = point;
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        let crosses = (current.1 > y) != (previous.1 > y)
            && x
                < (previous.0 - current.0) * (y - current.1)
                    / (previous.1 - current.1)
                    + current.0;
        if crosses {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

/// Parse the `nubs` control net + knot vectors out of a `spline-surface`
/// record's token stream into a kernel surface.
fn build_spline_surface(sat: &SatDocument, rec: &SatRecord) -> Option<NurbsSurface3> {
    if let Some(surface) = build_decoded_spline_surface(sat, rec) {
        return Some(surface);
    }

    let mut toks = rec.tokens.as_slice();
    if let Some(reference) = primary_subtype_reference(toks) {
        toks = sat.subtype_tokens(reference)?;
    }
    // Locate the real B-spline block. `nullbs` placeholders (for absent
    // rail/path surfaces) precede it; the actual surface is `nubs` (plain xyz
    // control points) or `nurbs` (rational — each control point carries a
    // weight, so it is stored as xyzw).
    let start = toks
        .iter()
        .rposition(|t| matches!(t, SatToken::Ident(s) if s == "nubs" || s == "nurbs"))?;
    let rational = matches!(&toks[start], SatToken::Ident(s) if s == "nurbs");

    let mut p = start + 1;
    let deg_u = read_int(toks, &mut p)? as usize;
    let deg_v = read_int(toks, &mut p)? as usize;
    // Four form flags (closure / singularity in u and v) — skip.
    for _ in 0..4 {
        read_int(toks, &mut p)?;
    }
    let n_uknot = read_int(toks, &mut p)? as usize;
    let n_vknot = read_int(toks, &mut p)? as usize;

    let raw_u_knots = read_knot_vec(toks, &mut p, n_uknot)?;
    let raw_v_knots = read_knot_vec(toks, &mut p, n_vknot)?;
    let stride = if rational { 4 } else { 3 };
    let available = toks[p..]
        .iter()
        .take_while(|token| token.as_float().is_some())
        .count();
    let base_u = raw_u_knots.len().checked_sub(deg_u + 1)?;
    let base_v = raw_v_knots.len().checked_sub(deg_v + 1)?;
    let mut best: Option<(usize, usize, bool, bool, usize)> = None;
    for clamp_u in [false, true] {
        for clamp_v in [false, true] {
            let n_ctrl_u = base_u + usize::from(clamp_u) * 2;
            let n_ctrl_v = base_v + usize::from(clamp_v) * 2;
            if n_ctrl_u <= deg_u || n_ctrl_v <= deg_v {
                continue;
            }
            let needed = n_ctrl_u.checked_mul(n_ctrl_v)?.checked_mul(stride)?;
            if needed > available {
                continue;
            }
            let remaining = available - needed;
            if best.as_ref().is_none_or(|candidate| remaining < candidate.4) {
                best = Some((n_ctrl_u, n_ctrl_v, clamp_u, clamp_v, remaining));
            }
        }
    }
    let Some((n_ctrl_u, n_ctrl_v, clamp_u, clamp_v, _)) = best else {
        if std::env::var_os("OCS_TESS_DEBUG").is_some() {
            eprintln!(
                "acis_spline_parse[{}]: no control-net match degree={deg_u}x{deg_v} raw_knots={}x{} available={available}",
                rec.index,
                raw_u_knots.len(),
                raw_v_knots.len()
            );
        }
        return None;
    };
    let u_knots = with_clamped_ends(raw_u_knots, clamp_u)?;
    let v_knots = with_clamped_ends(raw_v_knots, clamp_v)?;

    // Control points are stored row-major with u varying fastest (a full row
    // of u control points per v step). the kernel wants `ctrl[i_u][j_v]`.
    let total = n_ctrl_u * n_ctrl_v;
    let mut flat: Vec<[f64; 3]> = Vec::with_capacity(total);
    let mut flat_weights: Vec<f64> = Vec::with_capacity(total);
    for _ in 0..total {
        let x = read_float(toks, &mut p)?;
        let y = read_float(toks, &mut p)?;
        let z = read_float(toks, &mut p)?;
        // A rational net stores the weight alongside each point, and the
        // point itself unweighted — the kernel carries the two separately and
        // does the homogeneous multiply where it belongs.
        flat_weights.push(if rational { read_float(toks, &mut p)? } else { 1.0 });
        flat.push([x, y, z]);
    }
    let mut net = vec![Vec::with_capacity(n_ctrl_v); n_ctrl_u];
    let mut weights = vec![Vec::with_capacity(n_ctrl_v); n_ctrl_u];
    for v in 0..n_ctrl_v {
        for u in 0..n_ctrl_u {
            net[u].push(flat[v * n_ctrl_u + u]);
            weights[u].push(flat_weights[v * n_ctrl_u + u]);
        }
    }
    let surface = NurbsSurface3::new(
        deg_u,
        deg_v,
        net,
        u_knots,
        v_knots,
        rational.then_some(weights),
    );
    if surface.is_none() && std::env::var_os("OCS_TESS_DEBUG").is_some() {
        eprintln!(
            "acis_spline_parse[{}]: surface rejected degree={deg_u}x{deg_v} control={n_ctrl_u}x{n_ctrl_v}",
            rec.index
        );
    }
    surface
}

fn build_decoded_spline_surface(sat: &SatDocument, rec: &SatRecord) -> Option<NurbsSurface3> {
    let spline = SatSplineSurface::from_record(rec)?;
    let decoded = spline.bspline(sat)?;

    // Row-major with u varying fastest, which is how ACIS writes it and the
    // other way round from the net the kernel reads.
    let mut net = vec![Vec::with_capacity(decoded.control_count_v); decoded.control_count_u];
    let mut weights = vec![Vec::with_capacity(decoded.control_count_v); decoded.control_count_u];
    for v in 0..decoded.control_count_v {
        for u in 0..decoded.control_count_u {
            let point = decoded.control_points[v * decoded.control_count_u + u];
            net[u].push([point[0], point[1], point[2]]);
            weights[u].push(point[3]);
        }
    }
    NurbsSurface3::new(
        decoded.degree_u,
        decoded.degree_v,
        net,
        decoded.u_knots,
        decoded.v_knots,
        decoded.rational.then_some(weights),
    )
}

/// Read `count` `(knot value, multiplicity)` pairs into an expanded raw knot
/// vector. Some ACIS families store degree-sized ends and others already carry
/// the complete knot vector; the control-net size decides that after both axes
/// have been read.
fn read_knot_vec(
    toks: &[SatToken],
    p: &mut usize,
    count: usize,
) -> Option<Vec<f64>> {
    let mut knots: Vec<f64> = Vec::new();
    for _ in 0..count {
        let value = read_float(toks, p)?;
        let mult = read_int(toks, p)? as usize;
        for _ in 0..mult {
            knots.push(value);
        }
    }
    if knots.len() < 2 {
        return None;
    }
    Some(knots)
}

fn with_clamped_ends(mut knots: Vec<f64>, clamp: bool) -> Option<Vec<f64>> {
    if clamp {
        let first = *knots.first()?;
        let last = *knots.last()?;
        knots.insert(0, first);
        knots.push(last);
    }
    Some(knots)
}

fn primary_subtype_reference(tokens: &[SatToken]) -> Option<usize> {
    let start = tokens
        .iter()
        .position(|token| token.as_ident() == Some("{"))?;
    if tokens.get(start + 1).and_then(SatToken::as_ident) != Some("ref")
        || tokens.get(start + 3).and_then(SatToken::as_ident) != Some("}")
    {
        return None;
    }
    tokens
        .get(start + 2)?
        .as_integer()
        .and_then(|index| usize::try_from(index).ok())
}

fn read_int(toks: &[SatToken], p: &mut usize) -> Option<i64> {
    while *p < toks.len() {
        let t = &toks[*p];
        *p += 1;
        match t {
            SatToken::Integer(v) => return Some(*v),
            SatToken::Float(v) => return Some(*v as i64),
            // Skip block delimiters / idents that may appear inline.
            SatToken::Ident(_) | SatToken::Enum(_) => continue,
            _ => return None,
        }
    }
    None
}

fn read_float(toks: &[SatToken], p: &mut usize) -> Option<f64> {
    while *p < toks.len() {
        let t = &toks[*p];
        *p += 1;
        match t {
            SatToken::Float(v) => return Some(*v),
            SatToken::Integer(v) => return Some(*v as f64),
            SatToken::Ident(_) | SatToken::Enum(_) => continue,
            _ => return None,
        }
    }
    None
}
