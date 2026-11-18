//! Surface modelling on Solidcore: surfaces through curve networks and boundaries, blends and
//! fillets between sheets, extension, trimming and untrimming, offsets, sewing sheets into a
//! solid, curves projected onto a face, and a sheet's control net read, moved and rebuilt.
//!
//! Every operation takes and gives this crate's bodies; curves are Solidcore's record curves
//! ([`record_curve`] makes one of this crate's), each over its natural range.

use solidcore_ops::blend_surface::{self, BlendEdges, BlendSide, FilletSheets, Section};
use solidcore_ops::continuity::ContinuityTolerances;
use solidcore_ops::extend::Continuation;
use solidcore_ops::sweep::railed::Continuity;
use solidcore_record::brep as r;

use crate::bridge::{from_record, from_solidcore, to_record, to_solidcore};
use crate::{Body, Curve3, EdgeKey, FaceKey};

/// Seam tolerances the blends and fillets are measured with: a hundredth of a millimetre, half
/// a degree, and loose bounds past tangency (reported, never refused on).
fn seam_tolerances() -> ContinuityTolerances {
    ContinuityTolerances {
        gap_mm: 1e-2,
        angle_rad: 0.5_f64.to_radians(),
        curvature_per_mm: 1e-2,
        curvature_change_per_mm2: 1e-1,
        curvature_change_rate_per_mm3: 1.0,
    }
}

fn err(what: &str) -> impl Fn(solidcore_ops::Error) -> String + '_ {
    move |e| format!("{what}: {e:?}")
}

/// `curve` over `[t0, t1]` as a record curve over its natural range: a line as the segment, a
/// conic or spline as its exact NURBS form.
pub fn record_curve(curve: &Curve3, interval: [f64; 2]) -> Option<r::Curve> {
    match curve {
        Curve3::Line(_) => Some(r::Curve::Line {
            start: curve.point_at(interval[0]),
            end: curve.point_at(interval[1]),
        }),
        _ => {
            let nurbs = crate::nurbs_curve_form(curve, interval)?;
            Some(crate::bridge::record_spline(&nurbs))
        }
    }
}

/// A rational curve (an arc, an ellipse) as a cubic spline within `tolerance`, which the network
/// and patch builders take where they refuse a rational one; any other curve as it is.
///
/// # Errors
/// What Solidcore's fit refuses.
pub fn polynomial(curve: r::Curve, tolerance: f64) -> Result<r::Curve, String> {
    let r::Curve::RationalSpline { knots, homogeneous } = &curve else {
        return Ok(curve);
    };
    let degree = knots.len().saturating_sub(homogeneous.len() + 1);
    let (lo, hi) = (knots[degree], knots[knots.len() - 1 - degree]);
    let (model, _) = crate::bridge::model_curve(&curve, [lo, hi]).ok_or("a curve with no form here")?;
    // Through points closely enough spaced for the fit to follow the conic within `tolerance`.
    let count = 65;
    let points: Vec<[f64; 3]> = (0..count)
        .map(|i| model.point_at(lo + (hi - lo) * i as f64 / (count - 1) as f64))
        .collect();
    let (fitted, _) = solidcore_ops::curve_fit::ep_curve(&points, 3).map_err(err("fit"))?;
    let _ = tolerance;
    Ok(fitted)
}

/// A record curve's natural range: a line's 0 → 1, a spline's knot domain, a conic's turn.
fn natural_range(curve: &r::Curve) -> [f64; 2] {
    let domain = |knots: &[f64], count: usize| {
        let degree = knots.len().saturating_sub(count + 1);
        [knots[degree], knots[knots.len() - 1 - degree]]
    };
    match curve {
        r::Curve::Line { .. } => [0.0, 1.0],
        r::Curve::Circle { .. } | r::Curve::Ellipse { .. } => [0.0, std::f64::consts::TAU],
        r::Curve::Polyline { points } => [0.0, points.len().saturating_sub(1) as f64],
        r::Curve::Spline { knots, points } => domain(knots, points.len()),
        r::Curve::RationalSpline { knots, homogeneous } => domain(knots, homogeneous.len()),
        r::Curve::SurfaceIntersection { leader, .. } => natural_range(leader),
    }
}

/// Record curves as the sweep layer's curves, each over its natural range.
fn sweep_curves(curves: &[r::Curve]) -> Result<Vec<solidcore_ops::sweep::Curve>, String> {
    let mut out = Vec::new();
    for c in curves {
        out.extend(solidcore_ops::sweep::Curve::from_record(c, natural_range(c)).map_err(err("curve"))?);
    }
    Ok(out)
}

/// A sweep-layer curve as a record curve over its interval.
fn record_of_sweep(curve: &solidcore_ops::sweep::Curve) -> (r::Curve, [f64; 2]) {
    use solidcore_ops::sweep::Curve as S;
    match curve {
        S::Line { start, end } => (r::Curve::Line { start: *start, end: *end }, [0.0, 1.0]),
        S::Arc { center, axis, reference, radius, start, end } => {
            let x = ocs_geom::space::Vec3::from(*reference);
            let y = ocs_geom::space::Vec3::from(*axis).cross(x);
            (
                r::Curve::Circle {
                    frame: r::Frame { origin: *center, x_axis: x.to_array(), y_axis: y.to_array() },
                    radius: *radius,
                },
                [*start, *end],
            )
        }
        S::Nurbs { degree, points, knots } => {
            let domain = [knots[*degree], knots[knots.len() - 1 - degree]];
            let rational = points.iter().any(|p| (p[3] - 1.0).abs() > 1e-15);
            let c = if rational {
                r::Curve::RationalSpline {
                    knots: knots.clone(),
                    homogeneous: points.iter().map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]).collect(),
                }
            } else {
                r::Curve::Spline { knots: knots.clone(), points: points.iter().map(|p| [p[0], p[1], p[2]]).collect() }
            };
            (c, domain)
        }
    }
}

fn sheet(record: &r::Brep) -> Result<Body, String> {
    Ok(from_record(record)?.0)
}

/// The surface through a network of curves: `us` crossing `vs` (SURFNETWORK).
///
/// # Errors
/// What Solidcore refuses (curves that do not cross once, a family out of order, …).
pub fn network(us: &[r::Curve], vs: &[r::Curve], tolerance: f64) -> Result<Body, String> {
    let us = us.iter().cloned().map(|c| polynomial(c, tolerance)).collect::<Result<Vec<_>, _>>()?;
    let vs = vs.iter().cloned().map(|c| polynomial(c, tolerance)).collect::<Result<Vec<_>, _>>()?;
    let (record, _) = solidcore_ops::network::gordon(&us, &vs).map_err(err("network"))?;
    sheet(&record)
}

/// The patch filling a closed loop of boundary curves (SURFPATCH): a Coons patch for three or
/// four sides, an n-sided fill otherwise.
///
/// # Errors
/// What Solidcore refuses (curves that do not meet end to end, …).
pub fn patch(curves: &[r::Curve], tolerance: f64) -> Result<Body, String> {
    let curves = curves.iter().cloned().map(|c| polynomial(c, tolerance)).collect::<Result<Vec<_>, _>>()?;
    let record = if (3..=4).contains(&curves.len()) {
        solidcore_ops::surface::boundary(&curves).map_err(err("patch"))?
    } else {
        solidcore_ops::surface::n_sided(&curves).map_err(err("patch"))?.0
    };
    sheet(&record)
}

/// The ruled surface between two curves.
///
/// # Errors
/// What Solidcore refuses.
pub fn ruled(a: &r::Curve, b: &r::Curve) -> Result<Body, String> {
    sheet(&solidcore_ops::surface::ruled(a, b, false).map_err(err("ruled"))?)
}

/// G0 to G3, from an AutoCAD-style continuity number.
pub fn continuity(order: u8) -> Continuity {
    match order {
        0 => Continuity::G0,
        1 => Continuity::G1,
        2 => Continuity::G2,
        _ => Continuity::G3,
    }
}

/// The blend sheet from an edge of one sheet to an edge of another (SURFBLEND), meeting each with
/// its continuity, `bulge` how fast it leaves each side (0 the plain blend).
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn blend(
    a: &Body,
    edge_a: EdgeKey,
    b: &Body,
    edge_b: EdgeKey,
    continuities: [u8; 2],
    bulge: [f64; 2],
) -> Result<Body, String> {
    let (sa, oa, ma) = to_solidcore(a)?;
    let (sb, ob, mb) = to_solidcore(b)?;
    let ea = ma.edge_index(edge_a).ok_or("an edge the record lost")? as usize;
    let eb = mb.edge_index(edge_b).ok_or("an edge the record lost")? as usize;
    let how = BlendEdges {
        stations: 33,
        crown: bulge,
        stitch_tolerance: 1e-3,
        seam: seam_tolerances(),
    };
    let (blended, _) = blend_surface::blend_edges(
        BlendSide { body: &sa, order: &oa, edge: ea, continuity: continuity(continuities[0]) },
        BlendSide { body: &sb, order: &ob, edge: eb, continuity: continuity(continuities[1]) },
        &how,
    )
    .map_err(err("blend"))?;
    Ok(from_solidcore(&blended)?.0)
}

/// Which side of the one-face sheet `body` `point` lies on: where its normal points, or not.
fn side_of(body: &Body, point: [f64; 3]) -> Option<blend_surface::SheetSide> {
    let face_key = body.face_keys().next()?;
    let face = body.faces.get(face_key)?;
    let surface = body.surfaces.get(face.surface)?;
    let (u, v) = surface.parameters_at(point)?;
    let normal = ocs_geom::space::Vec3::from(surface.normal_at(u, v)?);
    let foot = ocs_geom::space::Vec3::from(surface.point_at(u, v));
    let along = (ocs_geom::space::Vec3::from(point) - foot).dot(normal) * if face.forward { 1.0 } else { -1.0 };
    Some(if along >= 0.0 { blend_surface::SheetSide::Front } else { blend_surface::SheetSide::Back })
}

/// The fillet sheet where two sheets meet (SURFFILLET), and with `trim` the two sheets cut back
/// to it. Sheets that cross are filleted in the quadrant the picks name: the side of `a` the
/// pick on `b` lies on, and the side of `b` the pick on `a` lies on.
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn fillet(
    a: &Body,
    b: &Body,
    radius: f64,
    trim: bool,
    picks: Option<([f64; 3], [f64; 3])>,
) -> Result<(Body, Option<(Body, Body)>), String> {
    let (sa, _, _) = to_solidcore(a)?;
    let (sb, _, _) = to_solidcore(b)?;
    let how = FilletSheets {
        section: Section::Radius(radius),
        trim,
        tolerance: 1e-3,
        stations: 33,
        seam: seam_tolerances(),
    };
    let made = blend_surface::fillet_sheets(&sa, &sb, &how)
        .or_else(|_| {
            blend_surface::fillet_crossing(
                &sa,
                &sb,
                &blend_surface::FilletCrossing {
                    sides: picks
                        .and_then(|(on_a, on_b)| Some([side_of(a, on_b)?, side_of(b, on_a)?]))
                        .unwrap_or([blend_surface::SheetSide::Front; 2]),
                    fillet: how,
                },
            )
        })
        .map_err(err("fillet"))?;
    let fillet = from_solidcore(&made.fillet)?.0;
    let trimmed = match made.trimmed {
        Some((x, y)) => Some((from_solidcore(&x)?.0, from_solidcore(&y)?.0)),
        None => None,
    };
    Ok((fillet, trimmed))
}

/// The sheet extended `distance` past `edge` (SURFEXTEND): the surface itself continued
/// (`stretch` false, AutoCAD's Extend), or stretched tangent (Stretch).
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn extend(sheet: &Body, edge: EdgeKey, distance: f64, stretch: bool) -> Result<Body, String> {
    let (s, order, map) = to_solidcore(sheet)?;
    let e = map.edge_index(edge).ok_or("an edge the record lost")? as usize;
    let how = if stretch { Continuation::Tangent } else { Continuation::Natural };
    let made = solidcore_ops::extend::extend_sheet(&s, &order, e, distance, how).map_err(err("extend"))?;
    Ok(from_solidcore(&made)?.0)
}

/// `face` of `sheet` cut along `curves` lying on it, the region holding `remove` taken away
/// (SURFTRIM).
///
/// # Errors
/// What the bridge or Solidcore refuse, or no region holding the point.
pub fn trim(sheet: &Body, face: FaceKey, curves: &[r::Curve], remove: [f64; 3]) -> Result<Body, String> {
    let (s, order, map) = to_solidcore(sheet)?;
    let f = map.face_index(face).ok_or("a face the record lost")? as usize;
    let tolerance = 1e-3;
    let (cut, regions) =
        solidcore_ops::trim::split_face_by_curves(&s, &order, f, &sweep_curves(curves)?, tolerance)
            .map_err(err("trim"))?;
    let (_, cut_order) = cut.to_record().map_err(err("trim"))?;
    let nearest = regions
        .iter()
        .min_by(|x, y| {
            let d = |p: [f64; 3]| (0..3).map(|i| (p[i] - remove[i]).powi(2)).sum::<f64>();
            d(x.inside).total_cmp(&d(y.inside))
        })
        .ok_or("the curves cut no region")?;
    let kept = solidcore_ops::trim::remove_faces(&cut, &cut_order, &[nearest.face]).map_err(err("trim"))?;
    Ok(from_solidcore(&kept)?.0)
}

/// `face` of `sheet` restored towards its surface (SURFUNTRIM): its holes taken away, and with
/// `outer` its outer boundary too.
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn untrim(sheet: &Body, face: FaceKey, outer: bool) -> Result<Body, String> {
    let (s, order, map) = to_solidcore(sheet)?;
    let f = map.face_index(face).ok_or("a face the record lost")? as usize;
    let made = solidcore_ops::untrim::untrim_face(&s, &order, f, solidcore_ops::untrim::Holes::All, outer, false)
        .map_err(err("untrim"))?;
    Ok(from_solidcore(&made)?.0)
}

/// The sheet moved `distance` along its normals (SURFOFFSET).
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn offset(sheet: &Body, distance: f64) -> Result<Body, String> {
    let (s, _, _) = to_solidcore(sheet)?;
    solidcore_ops::offset::offset_sheet(&s, distance).map_err(err("offset"))?;
    Ok(from_solidcore(&s)?.0)
}

/// Sheets sewn edge to edge (SURFSCULPT): the body, and whether it closed into a solid.
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn sew(sheets: &[&Body], tolerance: f64) -> Result<(Body, bool), String> {
    let made = sheets.iter().map(|b| to_solidcore(b).map(|x| x.0)).collect::<Result<Vec<_>, _>>()?;
    let refs: Vec<&solidcore_ops::Body> = made.iter().collect();
    let sewn = solidcore_ops::stitch::sew(&refs, tolerance).map_err(err("sew"))?;
    Ok((from_solidcore(&sewn.body)?.0, sewn.closed))
}

/// `curves` projected along `direction` onto `face` of `body` (PROJECTGEOMETRY), each with its
/// interval.
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn project(
    body: &Body,
    face: FaceKey,
    curves: &[r::Curve],
    direction: [f64; 3],
) -> Result<Vec<(r::Curve, [f64; 2])>, String> {
    let (s, order, map) = to_solidcore(body)?;
    let f = map.face_index(face).ok_or("a face the record lost")? as usize;
    let projected = solidcore_ops::sweep::project_curves(&s, &order, f, &sweep_curves(curves)?, direction, 1e-3)
        .map_err(err("project"))?;
    Ok(projected.iter().map(|(c, _)| record_of_sweep(c)).collect())
}

/// The control net of a one-face sheet (CVSHOW), its points row by row.
///
/// # Errors
/// What the bridge or Solidcore refuse (a sheet of more than one face, a face that is no
/// untrimmed spline).
pub fn control_net(sheet: &Body) -> Result<solidcore_ops::surface_cvs::Net, String> {
    let (record, _) = to_record(sheet)?;
    solidcore_ops::surface_cvs::control_net(&record).map_err(err("control net"))
}

/// The one-face sheet with control points `(row, column)` moved to new places.
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn move_control_points(sheet: &Body, moves: &[((usize, usize), [f64; 3])]) -> Result<Body, String> {
    let (record, _) = to_record(sheet)?;
    let moved = solidcore_ops::surface_cvs::move_cvs(&record, moves).map_err(err("move control points"))?;
    Ok(from_record(&moved)?.0)
}

/// The one-face sheet rebuilt to degrees and span counts (CVREBUILD), and how far it moved.
///
/// # Errors
/// What the bridge or Solidcore refuse.
pub fn rebuild(sheet: &Body, degree: [usize; 2], spans: [usize; 2], tolerance: f64) -> Result<(Body, f64), String> {
    let (record, _) = to_record(sheet)?;
    let (made, moved) = solidcore_ops::surface_cvs::rebuild(&record, degree[0], degree[1], spans[0], spans[1], tolerance)
        .map_err(err("rebuild"))?;
    Ok((from_record(&made)?.0, moved))
}

/// `curve` as a record curve over its knot domain.
pub fn record_nurbs(curve: &ocs_geom::space::NurbsCurve3) -> r::Curve {
    crate::bridge::record_spline(curve)
}

/// A record curve over `interval` as this crate's curve, and its interval there.
pub fn model_curve(curve: &r::Curve, interval: [f64; 2]) -> Option<(Curve3, [f64; 2])> {
    crate::bridge::model_curve(curve, interval)
}

/// The box of `face`'s surface parameters its boundary covers: each loop's edges sampled and
/// read back on the surface, angles kept continuous round each loop.
fn face_parameter_box(body: &Body, face: FaceKey) -> Option<([f64; 2], [f64; 2])> {
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    let periodic = match surface {
        crate::Surface::Cylinder(_) | crate::Surface::Cone(_) | crate::Surface::Sphere(_) => [true, false],
        crate::Surface::Torus(_) => [true, true],
        _ => [false, false],
    };
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for &loop_key in &node.loops {
        let mut previous: Option<[f64; 2]> = None;
        for &coedge_key in &body.loops.get(loop_key)?.coedges {
            let coedge = body.coedges.get(coedge_key)?;
            let edge = body.edges.get(coedge.edge)?;
            let curve = body.curves.get(edge.curve)?;
            for k in 0..=16 {
                let s = k as f64 / 16.0;
                let s = if coedge.forward { s } else { 1.0 - s };
                let t = edge.start_parameter + (edge.end_parameter - edge.start_parameter) * s;
                let (u, v) = surface.parameters_at(curve.point_at(t))?;
                let mut uv = [u, v];
                if let Some(p) = previous {
                    for d in 0..2 {
                        if periodic[d] {
                            uv[d] -= std::f64::consts::TAU * ((uv[d] - p[d]) / std::f64::consts::TAU).round();
                        }
                    }
                }
                for d in 0..2 {
                    lo[d] = lo[d].min(uv[d]);
                    hi[d] = hi[d].max(uv[d]);
                }
                previous = Some(uv);
            }
        }
    }
    (lo[0] < hi[0] && lo[1] < hi[1]).then_some(([lo[0], hi[0]], [lo[1], hi[1]]))
}

/// The isoline of `face` through `point`: along u (or along v) across the face's parameter
/// box, as a cubic through its points.
pub fn isoline(body: &Body, face: FaceKey, point: [f64; 3], along_v: bool) -> Option<ocs_geom::space::NurbsCurve3> {
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    let (u, v) = surface.parameters_at(point)?;
    let (us, vs) = face_parameter_box(body, face)?;
    let points: Vec<[f64; 3]> = (0..=32)
        .map(|k| {
            let s = k as f64 / 32.0;
            if along_v {
                surface.point_at(u, vs[0] + (vs[1] - vs[0]) * s)
            } else {
                surface.point_at(us[0] + (us[1] - us[0]) * s, v)
            }
        })
        .collect();
    ocs_geom::space::NurbsCurve3::interpolate_fit(&points, None, None, ocs_geom::space::Parameterization::Chord)
}

/// Every face of `body` as a one-face sheet on its surface's NURBS form (CONVTONURBS): the
/// face's edges kept, its surface the exact NURBS over the parameters it covers.
///
/// # Errors
/// A face whose surface has no NURBS form here.
pub fn to_nurbs_sheets(body: &Body) -> Result<Vec<Body>, String> {
    let mut out = Vec::new();
    for face in body.face_keys() {
        let mut sheet = crate::face_sheet(body, &[face]).ok_or("a face could not be copied")?;
        let keys: Vec<FaceKey> = sheet.face_keys().collect();
        for f in keys {
            let surface_key = sheet.faces.get(f).ok_or("a face")?.surface;
            let surface = sheet.surfaces.get(surface_key).ok_or("a surface")?.clone();
            if matches!(surface, crate::Surface::Nurbs(_)) {
                continue;
            }
            let (us, vs) = face_parameter_box(&sheet, f).ok_or("a face whose parameters could not be read")?;
            let nurbs = crate::nurbs_form(&surface, us, vs).ok_or("a surface with no NURBS form")?;
            *sheet.surfaces.get_mut(surface_key).ok_or("a surface")? = crate::Surface::Nurbs(nurbs);
            // The old pcurves are in the analytic surface's parameters.
            let coedges: Vec<_> = sheet.face_coedges(f);
            for c in coedges {
                if let Some(coedge) = sheet.coedges.get_mut(c) {
                    coedge.pcurve = None;
                }
            }
        }
        out.push(sheet);
    }
    Ok(out)
}

/// The one-face sheet's NURBS surface as a record (Solidcore's NURBS form of an analytic one).
fn sheet_spline(sheet: &Body) -> Result<r::Surface, String> {
    let (record, _) = to_record(sheet)?;
    let nurbs = solidcore_ops::surface_cvs::move_cvs(&record, &[]).map_err(err("NURBS form"))?;
    let face = nurbs.faces.first().ok_or("a sheet without its face")?;
    Ok(face.nurbs.clone().unwrap_or_else(|| face.surface.clone()))
}

/// A whole NURBS surface as a one-face sheet bounded by its four boundary curves.
fn spline_sheet(surface: r::Surface) -> Result<Body, String> {
    let (u_knots, v_knots, net): (Vec<f64>, Vec<f64>, Vec<Vec<[f64; 4]>>) = match &surface {
        r::Surface::Spline { u_knots, v_knots, points } => (
            u_knots.clone(),
            v_knots.clone(),
            points.iter().map(|row| row.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()).collect(),
        ),
        r::Surface::RationalSpline { u_knots, v_knots, homogeneous } => (u_knots.clone(), v_knots.clone(), homogeneous.clone()),
        _ => return Err("not a spline surface".into()),
    };
    let rows = net.len();
    let cols = net.first().map_or(0, Vec::len);
    let curve = |points: Vec<[f64; 4]>, knots: &[f64]| -> r::Curve {
        if points.iter().all(|p| (p[3] - 1.0).abs() < 1e-15) {
            r::Curve::Spline { knots: knots.to_vec(), points: points.iter().map(|p| [p[0], p[1], p[2]]).collect() }
        } else {
            r::Curve::RationalSpline { knots: knots.to_vec(), homogeneous: points }
        }
    };
    let euclid = |h: [f64; 4]| [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
    let domain = |knots: &[f64], count: usize| {
        let degree = knots.len() - count - 1;
        [knots[degree], knots[knots.len() - 1 - degree]]
    };
    let (ud, vd) = (domain(&u_knots, rows), domain(&v_knots, cols));
    let trims = NoTrims;
    let mut asm = solidcore_step::assembly::Assembly::new(1e-6, Some(&trims));
    let corners = [net[0][0], net[rows - 1][0], net[rows - 1][cols - 1], net[0][cols - 1]].map(euclid);
    let v: Vec<u32> = corners.iter().map(|p| asm.add_vertex(*p)).collect::<Result<_, _>>()?;
    let bottom = curve((0..rows).map(|i| net[i][0]).collect(), &u_knots);
    let right = curve((0..cols).map(|j| net[rows - 1][j]).collect(), &v_knots);
    let top = curve((0..rows).map(|i| net[i][cols - 1]).collect(), &u_knots);
    let left = curve((0..cols).map(|j| net[0][j]).collect(), &v_knots);
    let (e0, f0) = asm.add_edge(0, bottom, ud, [v[0], v[1]])?;
    let (e1, f1) = asm.add_edge(1, right, vd, [v[1], v[2]])?;
    let (e2, f2) = asm.add_edge(2, top, ud, [v[3], v[2]])?;
    let (e3, f3) = asm.add_edge(3, left, vd, [v[0], v[3]])?;
    let bound = solidcore_step::assembly::Bound::Edges(vec![(e0, f0), (e1, f1), (e2, !f2), (e3, !f3)]);
    let face = asm.add_face(0, surface, vec![bound], Some(0), true)?;
    let solid = asm.finish(0, String::new(), vec![vec![(face, true)]], solidcore_step::assembly::Kind::Model)?;
    Ok(from_record(&solid.brep)?.0)
}

/// The pcurves of a whole spline face's boundary are its parameter box's sides: no projection.
struct NoTrims;

impl solidcore_ops::pcurve::Trims for NoTrims {
    fn trims(&self, face: &solidcore_ops::pcurve::TrimFace<'_>) -> Result<Vec<Vec<r::Curve2>>, String> {
        let [[u0, u1], [v0, v1]] = face.domain;
        let side = |a: [f64; 2], b: [f64; 2], interval: [f64; 2]| r::Curve2::Spline {
            knots: vec![interval[0], interval[0], interval[1], interval[1]],
            points: vec![a, b],
        };
        let mut out = Vec::new();
        for l in &face.loops {
            let mut pcs = Vec::new();
            for (k, u) in l.iter().enumerate() {
                let e = face.edges.get(u.edge as usize).ok_or("edge")?;
                let pc = match k {
                    0 => side([u0, v0], [u1, v0], e.interval),
                    1 => side([u1, v0], [u1, v1], e.interval),
                    2 => side([u0, v1], [u1, v1], e.interval),
                    _ => side([u0, v0], [u0, v1], e.interval),
                };
                pcs.push(u.pcurve.clone().unwrap_or(pc));
            }
            out.push(pcs);
        }
        Ok(out)
    }
}

/// One knot inserted at `t` into a homogeneous control polygon of `degree` on `knots`
/// (Boehm's algorithm, The NURBS Book A5.1 for one insertion).
fn insert_knot(points: &[[f64; 4]], knots: &[f64], degree: usize, t: f64) -> (Vec<[f64; 4]>, Vec<f64>) {
    let n = points.len();
    let k = (degree..n).rev().find(|&k| knots[k] <= t).unwrap_or(degree);
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..=n {
        if i + degree <= k {
            out.push(points[i]);
        } else if i > k {
            out.push(points[i - 1]);
        } else {
            let a = (t - knots[i]) / (knots[i + degree] - knots[i]);
            let (p, q) = (points[i - 1], points[i]);
            out.push(std::array::from_fn(|c| (1.0 - a) * p[c] + a * q[c]));
        }
    }
    let mut new_knots = knots.to_vec();
    new_knots.insert(k + 1, t);
    (out, new_knots)
}

/// The one-face NURBS sheet with a row of control vertices added through `point` (CVADD): a
/// knot inserted there along u, or along v.
///
/// # Errors
/// A sheet that is no single NURBS face, or a point off it.
pub fn add_control_row(sheet: &Body, point: [f64; 3], along_v: bool) -> Result<Body, String> {
    let surface = sheet_spline(sheet)?;
    let kernel = crate::bridge::spline_surface(&surface).ok_or("not a spline surface")?;
    let (u, v) = kernel.parameters_at(point).ok_or("the point is off the surface")?;
    let (u_knots, v_knots, net): (Vec<f64>, Vec<f64>, Vec<Vec<[f64; 4]>>) = match &surface {
        r::Surface::Spline { u_knots, v_knots, points } => (
            u_knots.clone(),
            v_knots.clone(),
            points.iter().map(|row| row.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()).collect(),
        ),
        r::Surface::RationalSpline { u_knots, v_knots, homogeneous } => (u_knots.clone(), v_knots.clone(), homogeneous.clone()),
        _ => return Err("not a spline surface".into()),
    };
    let (rows, cols) = (net.len(), net.first().map_or(0, Vec::len));
    let (net, u_knots, v_knots) = if !along_v {
        let degree = u_knots.len() - rows - 1;
        let mut columns: Vec<Vec<[f64; 4]>> = Vec::with_capacity(cols);
        let mut knots = u_knots.clone();
        for j in 0..cols {
            let column: Vec<[f64; 4]> = (0..rows).map(|i| net[i][j]).collect();
            let (c, k) = insert_knot(&column, &u_knots, degree, u);
            columns.push(c);
            knots = k;
        }
        let net: Vec<Vec<[f64; 4]>> = (0..rows + 1).map(|i| (0..cols).map(|j| columns[j][i]).collect()).collect();
        (net, knots, v_knots)
    } else {
        let degree = v_knots.len() - cols - 1;
        let mut knots = v_knots.clone();
        let net: Vec<Vec<[f64; 4]>> = net
            .iter()
            .map(|row| {
                let (r, k) = insert_knot(row, &v_knots, degree, v);
                knots = k;
                r
            })
            .collect();
        (net, u_knots, knots)
    };
    let rational = net.iter().flatten().any(|p| (p[3] - 1.0).abs() > 1e-15);
    let surface = if rational {
        r::Surface::RationalSpline { u_knots, v_knots, homogeneous: net }
    } else {
        r::Surface::Spline { u_knots, v_knots, points: net.iter().map(|row| row.iter().map(|p| [p[0], p[1], p[2]]).collect()).collect() }
    };
    spline_sheet(surface)
}

/// The one-face NURBS sheet with one row of control vertices fewer along u (or v) (CVREMOVE),
/// rebuilt within `tolerance` where it can be.
///
/// # Errors
/// What Solidcore's rebuild refuses, or a net with no row to spare.
pub fn remove_control_row(sheet: &Body, along_v: bool, tolerance: f64) -> Result<Body, String> {
    let surface = sheet_spline(sheet)?;
    let kernel = crate::bridge::spline_surface(&surface).ok_or("not a spline surface")?;
    let (ud, vd) = kernel.degrees();
    let rows = kernel.control_points().len();
    let cols = kernel.control_points().first().map_or(0, Vec::len);
    let (mut nu, mut nv) = (rows, cols);
    if along_v { nv -= 1 } else { nu -= 1 }
    if nu <= ud || nv <= vd {
        return Err("no row of control vertices to spare".into());
    }
    Ok(rebuild(sheet, [ud, vd], [nu - ud, nv - vd], tolerance)?.0)
}

/// The one-face NURBS sheet pulled so the point picked at `from` moves towards `to` (the CV edit
/// bar): the control vertex nearest the point moved by the difference, its neighbours easing.
///
/// # Errors
/// What Solidcore refuses.
pub fn drag_point(sheet: &Body, from: [f64; 3], to: [f64; 3]) -> Result<Body, String> {
    use solidcore_ops::surface_cvs::{Falloff, Picks, Shape};
    let (record, _) = to_record(sheet)?;
    let net = solidcore_ops::surface_cvs::control_net(&record).map_err(err("control net"))?;
    let at = net
        .points
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let d = |p: &[f64; 3]| (0..3).map(|i| (p[i] - from[i]).powi(2)).sum::<f64>();
            d(a.1).total_cmp(&d(b.1))
        })
        .map(|(k, _)| (k / net.cols, k % net.cols))
        .ok_or("an empty control net")?;
    let delta = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let moved = solidcore_ops::surface_cvs::move_net(
        &record,
        &Picks::Cvs(vec![at]),
        delta,
        Some(Falloff { radius: 1, shape: Shape::Smooth }),
    )
    .map_err(err("move"))?;
    Ok(from_record(&moved)?.0)
}

