//! Surface modelling on Solidcore: surfaces through curve networks and boundaries, blends and
//! fillets between sheets, extension, trimming and untrimming, offsets, sewing sheets into a
//! solid, curves projected onto a face, and a sheet's control net read, moved and rebuilt.
//!
//! Every operation takes and gives record bodies, faces and edges by their record index; curves
//! are record curves, each with the interval of its parameter it covers.

use solidcore_ops::blend_surface::{self, BlendEdges, BlendSide, FilletSheets, Section};
use solidcore_ops::continuity::ContinuityTolerances;
use solidcore_ops::extend::Continuation;
use solidcore_ops::sweep::railed::Continuity;
use solidcore_record::brep as r;

use crate::geometry::OnSurface;
use crate::model::{open, record, Brep};

/// A record curve and the interval of its parameter it covers.
pub type Span = (r::Curve, [f64; 2]);

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

/// A record curve's natural range: a line's 0 → 1, a spline's knot domain, a conic's turn.
pub fn natural_range(curve: &r::Curve) -> [f64; 2] {
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

/// A whole curve: `curve` over its natural range.
pub fn whole(curve: r::Curve) -> Span {
    let range = natural_range(&curve);
    (curve, range)
}

/// A span as a polynomial curve over its natural range, which the network and patch builders
/// take: a line or a whole polynomial spline as it is, anything else (an arc, an ellipse, a
/// rational or partial spline) as a cubic fitted through closely spaced points of it.
///
/// # Errors
/// What Solidcore's fit refuses.
pub fn polynomial((curve, interval): &Span) -> Result<r::Curve, String> {
    let whole = *interval == natural_range(curve);
    match curve {
        r::Curve::Line { start, end } => {
            let at = |t: f64| std::array::from_fn(|k| start[k] + (end[k] - start[k]) * t);
            return Ok(r::Curve::Line { start: at(interval[0]), end: at(interval[1]) });
        }
        r::Curve::Spline { .. } | r::Curve::Polyline { .. } if whole => return Ok(curve.clone()),
        _ => {}
    }
    let points = crate::geometry::points_on(curve, *interval, 64).ok_or("a curve with no points")?;
    let (fitted, _) = solidcore_ops::curve_fit::ep_curve(&points, 3).map_err(err("fit"))?;
    Ok(fitted)
}

/// Spans as the sweep layer's curves.
fn sweep_curves(curves: &[Span]) -> Result<Vec<solidcore_ops::sweep::Curve>, String> {
    let mut out = Vec::new();
    for (c, interval) in curves {
        out.extend(solidcore_ops::sweep::Curve::from_record(c, *interval).map_err(err("curve"))?);
    }
    Ok(out)
}

/// A sweep-layer curve as a span.
pub fn span_of_sweep(curve: &solidcore_ops::sweep::Curve) -> Span {
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

/// `curve` as a record spline over its knot domain.
pub fn record_nurbs(curve: &ocs_geom::space::NurbsCurve3) -> r::Curve {
    let (points, weights, knots) = (curve.control_points(), curve.weights(), curve.knots().to_vec());
    if weights.iter().all(|w| (w - 1.0).abs() <= 1e-15) {
        r::Curve::Spline { knots, points: points.to_vec() }
    } else {
        r::Curve::RationalSpline {
            knots,
            homogeneous: points.iter().zip(weights).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w]).collect(),
        }
    }
}

/// The surface through a network of curves: `us` crossing `vs` (SURFNETWORK).
///
/// # Errors
/// What Solidcore refuses (curves that do not cross once, a family out of order, …).
pub fn network(us: &[Span], vs: &[Span]) -> Result<Brep, String> {
    let us = us.iter().map(polynomial).collect::<Result<Vec<_>, _>>()?;
    let vs = vs.iter().map(polynomial).collect::<Result<Vec<_>, _>>()?;
    Ok(solidcore_ops::network::gordon(&us, &vs).map_err(err("network"))?.0)
}

/// The patch filling a closed loop of boundary curves (SURFPATCH): a Coons patch for three or
/// four sides, an n-sided fill otherwise.
///
/// # Errors
/// What Solidcore refuses (curves that do not meet end to end, …).
pub fn patch(curves: &[Span]) -> Result<Brep, String> {
    let curves = curves.iter().map(polynomial).collect::<Result<Vec<_>, _>>()?;
    if (3..=4).contains(&curves.len()) {
        solidcore_ops::surface::boundary(&curves).map_err(err("patch"))
    } else {
        Ok(solidcore_ops::surface::n_sided(&curves).map_err(err("patch"))?.0)
    }
}

/// G0 to G3, from a continuity number.
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
/// What Solidcore refuses.
pub fn blend(a: &Brep, edge_a: u32, b: &Brep, edge_b: u32, continuities: [u8; 2], bulge: [f64; 2]) -> Result<Brep, String> {
    let (sa, oa) = open(a).map_err(err("blend"))?;
    let (sb, ob) = open(b).map_err(err("blend"))?;
    let how = BlendEdges {
        stations: 33,
        crown: bulge,
        stitch_tolerance: 1e-3,
        seam: seam_tolerances(),
    };
    let (blended, _) = blend_surface::blend_edges(
        BlendSide { body: &sa, order: &oa, edge: edge_a as usize, continuity: continuity(continuities[0]) },
        BlendSide { body: &sb, order: &ob, edge: edge_b as usize, continuity: continuity(continuities[1]) },
        &how,
    )
    .map_err(err("blend"))?;
    record(&blended).map_err(err("blend"))
}

/// Which side of the one-face sheet `body` `point` lies on: where its normal points, or not.
fn side_of(body: &Brep, point: [f64; 3]) -> Option<blend_surface::SheetSide> {
    let face = body.faces.first()?;
    let surface = OnSurface::of(face.parameters())?;
    let uv = surface.uv(point)?;
    let normal = surface.normal(uv)?;
    let foot = surface.point(uv)?;
    let forward = crate::model::face_sides(body).first().copied().unwrap_or(true);
    let along = crate::geometry::dot(crate::geometry::sub(point, foot), normal) * if forward { 1.0 } else { -1.0 };
    Some(if along >= 0.0 { blend_surface::SheetSide::Front } else { blend_surface::SheetSide::Back })
}

/// The fillet sheet where two sheets meet (SURFFILLET), and with `trim` the two sheets cut back
/// to it. Sheets that cross are filleted in the quadrant the picks name: the side of `a` the
/// pick on `b` lies on, and the side of `b` the pick on `a` lies on.
///
/// # Errors
/// What Solidcore refuses.
pub fn fillet(
    a: &Brep,
    b: &Brep,
    radius: f64,
    trim: bool,
    picks: Option<([f64; 3], [f64; 3])>,
) -> Result<(Brep, Option<(Brep, Brep)>), String> {
    let (sa, _) = open(a).map_err(err("fillet"))?;
    let (sb, _) = open(b).map_err(err("fillet"))?;
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
    let fillet = record(&made.fillet).map_err(err("fillet"))?;
    let trimmed = match made.trimmed {
        Some((x, y)) => Some((record(&x).map_err(err("fillet"))?, record(&y).map_err(err("fillet"))?)),
        None => None,
    };
    Ok((fillet, trimmed))
}

/// The sheet extended `distance` past `edge` (SURFEXTEND): the surface itself continued
/// (`stretch` false), or stretched tangent.
///
/// # Errors
/// What Solidcore refuses.
pub fn extend(sheet: &Brep, edge: u32, distance: f64, stretch: bool) -> Result<Brep, String> {
    let (s, order) = open(sheet).map_err(err("extend"))?;
    let how = if stretch { Continuation::Tangent } else { Continuation::Natural };
    let made = solidcore_ops::extend::extend_sheet(&s, &order, edge as usize, distance, how).map_err(err("extend"))?;
    record(&made).map_err(err("extend"))
}

/// `face` of `sheet` cut along `curves` lying on it, the region holding `remove` taken away
/// (SURFTRIM).
///
/// # Errors
/// What Solidcore refuses, or no region holding the point.
pub fn trim(sheet: &Brep, face: u32, curves: &[Span], remove: [f64; 3]) -> Result<Brep, String> {
    let (s, order) = open(sheet).map_err(err("trim"))?;
    let (cut, regions) =
        solidcore_ops::trim::split_face_by_curves(&s, &order, face as usize, &sweep_curves(curves)?, 1e-3)
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
    record(&kept).map_err(err("trim"))
}

/// `face` of `sheet` restored towards its surface (SURFUNTRIM): its holes taken away, and with
/// `outer` its outer boundary too.
///
/// # Errors
/// What Solidcore refuses.
pub fn untrim(sheet: &Brep, face: u32, outer: bool) -> Result<Brep, String> {
    let (s, order) = open(sheet).map_err(err("untrim"))?;
    let made = solidcore_ops::untrim::untrim_face(&s, &order, face as usize, solidcore_ops::untrim::Holes::All, outer, false)
        .map_err(err("untrim"))?;
    record(&made).map_err(err("untrim"))
}

/// The sheet moved `distance` along its normals (SURFOFFSET).
///
/// # Errors
/// What Solidcore refuses.
pub fn offset(sheet: &Brep, distance: f64) -> Result<Brep, String> {
    crate::model::offset_sheet(sheet, distance).map_err(err("offset"))
}

/// Sheets sewn edge to edge (SURFSCULPT): the body, and whether it closed into a solid.
///
/// # Errors
/// What Solidcore refuses.
pub fn sew(sheets: &[&Brep], tolerance: f64) -> Result<(Brep, bool), String> {
    let made = sheets.iter().map(|b| open(b).map(|x| x.0)).collect::<Result<Vec<_>, _>>().map_err(err("sew"))?;
    let refs: Vec<&solidcore_ops::Body> = made.iter().collect();
    let sewn = solidcore_ops::stitch::sew(&refs, tolerance).map_err(err("sew"))?;
    Ok((record(&sewn.body).map_err(err("sew"))?, sewn.closed))
}

/// `curves` projected along `direction` onto `face` of `body` (PROJECTGEOMETRY).
///
/// # Errors
/// What Solidcore refuses.
pub fn project(body: &Brep, face: u32, curves: &[Span], direction: [f64; 3]) -> Result<Vec<Span>, String> {
    let (s, order) = open(body).map_err(err("project"))?;
    let projected = solidcore_ops::sweep::project_curves(&s, &order, face as usize, &sweep_curves(curves)?, direction, 1e-3)
        .map_err(err("project"))?;
    Ok(projected.iter().map(|(c, _)| span_of_sweep(c)).collect())
}

/// The control net of a one-face sheet (CVSHOW), its points row by row.
///
/// # Errors
/// What Solidcore refuses (a sheet of more than one face, a face that is no untrimmed spline).
pub fn control_net(sheet: &Brep) -> Result<solidcore_ops::surface_cvs::Net, String> {
    solidcore_ops::surface_cvs::control_net(sheet).map_err(err("control net"))
}

/// The one-face sheet with control points `(row, column)` moved to new places.
///
/// # Errors
/// What Solidcore refuses.
pub fn move_control_points(sheet: &Brep, moves: &[((usize, usize), [f64; 3])]) -> Result<Brep, String> {
    solidcore_ops::surface_cvs::move_cvs(sheet, moves).map_err(err("move control points"))
}

/// The one-face sheet rebuilt to degrees and span counts (CVREBUILD), and how far it moved.
///
/// # Errors
/// What Solidcore refuses.
pub fn rebuild(sheet: &Brep, degree: [usize; 2], spans: [usize; 2], tolerance: f64) -> Result<(Brep, f64), String> {
    solidcore_ops::surface_cvs::rebuild(sheet, degree[0], degree[1], spans[0], spans[1], tolerance).map_err(err("rebuild"))
}

/// The isoline of `face` through `point`: along u (or along v) across the face's parameter
/// box, as a cubic through its points.
pub fn isoline(body: &Brep, face: u32, point: [f64; 3], along_v: bool) -> Option<ocs_geom::space::NurbsCurve3> {
    let node = body.faces.get(face as usize)?;
    let surface = OnSurface::of(node.parameters())?;
    let [u, v] = surface.uv(point)?;
    let [us, vs] = node.domain;
    let points: Vec<[f64; 3]> = (0..=32)
        .map(|k| {
            let s = k as f64 / 32.0;
            if along_v {
                surface.point([u, vs[0] + (vs[1] - vs[0]) * s])
            } else {
                surface.point([us[0] + (us[1] - us[0]) * s, v])
            }
        })
        .collect::<Option<_>>()?;
    ocs_geom::space::NurbsCurve3::interpolate_fit(&points, None, None, ocs_geom::space::Parameterization::Chord)
}

/// Every face of `body` as a one-face sheet on its surface's NURBS form (CONVTONURBS): the
/// face's edges kept, its surface the exact NURBS over the parameters it covers.
///
/// # Errors
/// A face whose surface has no NURBS form here.
pub fn to_nurbs_sheets(body: &Brep) -> Result<Vec<Brep>, String> {
    (0..body.faces.len() as u32)
        .map(|face| {
            let sheet = crate::model::face_sheet(body, &[face]).ok_or("a face could not be copied")?;
            crate::geometry::on_nurbs_forms(&sheet).ok_or_else(|| "a surface with no NURBS form".to_owned())
        })
        .collect()
}

/// The one-face sheet's NURBS surface (Solidcore's NURBS form of an analytic one).
fn sheet_spline(sheet: &Brep) -> Result<r::Surface, String> {
    let nurbs = solidcore_ops::surface_cvs::move_cvs(sheet, &[]).map_err(err("NURBS form"))?;
    let face = nurbs.faces.first().ok_or("a sheet without its face")?;
    Ok(face.parameters().clone())
}

/// A spline surface's knots and homogeneous net, row by row.
fn spline_net(surface: &r::Surface) -> Result<(Vec<f64>, Vec<f64>, Vec<Vec<[f64; 4]>>), String> {
    match surface {
        r::Surface::Spline { u_knots, v_knots, points } => Ok((
            u_knots.clone(),
            v_knots.clone(),
            points.iter().map(|row| row.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()).collect(),
        )),
        r::Surface::RationalSpline { u_knots, v_knots, homogeneous } => Ok((u_knots.clone(), v_knots.clone(), homogeneous.clone())),
        _ => Err("not a spline surface".into()),
    }
}

/// A whole NURBS surface as a one-face sheet bounded by its four boundary curves.
fn spline_sheet(surface: r::Surface) -> Result<Brep, String> {
    let (u_knots, v_knots, net) = spline_net(&surface)?;
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
    Ok(solid.brep)
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
pub fn add_control_row(sheet: &Brep, point: [f64; 3], along_v: bool) -> Result<Brep, String> {
    let surface = sheet_spline(sheet)?;
    let [u, v] = OnSurface::of(&surface).and_then(|s| s.uv(point)).ok_or("the point is off the surface")?;
    let (u_knots, v_knots, net) = spline_net(&surface)?;
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
pub fn remove_control_row(sheet: &Brep, along_v: bool, tolerance: f64) -> Result<Brep, String> {
    let (u_knots, v_knots, net) = spline_net(&sheet_spline(sheet)?)?;
    let (rows, cols) = (net.len(), net.first().map_or(0, Vec::len));
    let (ud, vd) = (u_knots.len() - rows - 1, v_knots.len() - cols - 1);
    let (mut nu, mut nv) = (rows, cols);
    if along_v {
        nv -= 1
    } else {
        nu -= 1
    }
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
pub fn drag_point(sheet: &Brep, from: [f64; 3], to: [f64; 3]) -> Result<Brep, String> {
    use solidcore_ops::surface_cvs::{Falloff, Picks, Shape};
    let net = control_net(sheet)?;
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
    solidcore_ops::surface_cvs::move_net(sheet, &Picks::Cvs(vec![at]), delta, Some(Falloff { radius: 1, shape: Shape::Smooth }))
        .map_err(err("move"))
}
