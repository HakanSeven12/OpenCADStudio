use acadrust::entities::Spline;
use crate::t;
use acadrust::kernel::space::NurbsCurve3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, parse_f64, ro_prop as ro, square_grip};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};

fn to_render(spl: &Spline) -> RenderEntity {
    let n = spl.control_points.len();
    if n < 2 {
        // A fit-point spline (DWG scenario 2 / R2013+) stores only the points
        // the curve passes through — no control points or knots. Interpolate a
        // smooth Catmull-Rom curve through them and hand it back as a dense
        // polyline so it still draws.
        if spl.fit_points.len() >= 2 {
            // Drawn from the same interpolation TRIM and snap use, so that
            // what is on screen is the curve those commands cut. The two had
            // parted: this drew a C² cubic solved here (or a Catmull-Rom for a
            // closed spline), while the kernel interpolated its own — near
            // enough to look right and far enough apart that a trim landed
            // beside the line it was aimed at.
            let planar = crate::entities::curve::spline_curve(spl);
            let pts = match &planar {
                Some(curve) => crate::entities::curve::curve_points(curve),
                // A fit spline through points in space is not a planar curve,
                // so the kernel has nothing to say about it and the solve
                // here remains the only description of its shape.
                None if spl.flags.closed || spl.flags.periodic => {
                    catmull_rom_polyline(&spl.fit_points, true)
                }
                None => fit_spline_polyline(spl),
            };
            let snap = planar
                .as_ref()
                .map(crate::entities::curve::snap_from)
                .unwrap_or_default();
            // Fit points are on the curve, so they stay on offer as ends in
            // their own right — but never through `key_vertices`, whose
            // consecutive entries the snap engine joins and offers the middle
            // of. The chord between two fit points of a curvy spline does not
            // run along it.
            let mut snap_pts = snap.snap_pts;
            snap_pts.extend(spl.fit_points.iter().map(|p| {
                (
                    glam::DVec3::new(p.x, p.y, p.z),
                    crate::scene::model::wire_model::SnapHint::Endpoint,
                )
            }));
            let key_vertices = if planar.is_some() {
                Vec::new()
            } else {
                spl.fit_points.iter().map(|p| [p.x, p.y, p.z]).collect()
            };
            return RenderEntity {
                pick_tris: Vec::new(),
                object: RenderObject::Lines(pts),
                snap_pts,
                tangent_geoms: vec![],
                key_vertices,
                fill_tris: vec![],
            };
        }
        return RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(Vec::new()),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    // Snap sources from the spline's own curve. A spline is not a chain of
    // straight segments, so nothing goes in `key_vertices`: consecutive
    // entries there are joined and their midpoints offered, and the chord
    // between two fit points of a curvy spline does not run along it.
    //
    // Fit points are on the curve and stay on offer, as ends in their own
    // right. Control points are not on the curve and are dropped — snapping
    // to one put the cursor in empty space beside the geometry.
    let curve_snap = crate::entities::curve::spline_curve(spl)
        .map(|curve| crate::entities::curve::snap_from(&curve));
    let (snap_pts, key_vertices) = match curve_snap {
        Some(snap) => {
            let mut points = snap.snap_pts;
            points.extend(spl.fit_points.iter().map(|p| {
                (
                    glam::DVec3::new(p.x, p.y, p.z),
                    crate::scene::model::wire_model::SnapHint::Endpoint,
                )
            }));
            (points, Vec::new())
        }
        // A spline through points in space is not a planar curve, so the
        // kernel has nothing to say about it. Its stored points remain the
        // only thing to offer.
        None => {
            let source = if spl.fit_points.is_empty() {
                &spl.control_points
            } else {
                &spl.fit_points
            };
            (
                Vec::new(),
                source.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(),
            )
        }
    };

    // Sampled through the spline's own curve — the same definition EXTRUDE
    // and REVOLVE read — and closed back onto its start when the flags say
    // it is periodic and the two ends do not already meet.
    let is_closed = spl.flags.closed || spl.flags.periodic;
    let mut points = crate::entities::curve::spline_curve(spl)
        .map(|planar| crate::entities::curve::curve_points(&planar))
        .unwrap_or_default();
    if is_closed {
        if let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) {
            let gap = ((last[0] - first[0]).powi(2)
                + (last[1] - first[1]).powi(2)
                + (last[2] - first[2]).powi(2))
            .sqrt();
            if gap > 1e-6 {
                points.push(first);
            }
        }
    }
    let object = RenderObject::Lines(points);

    RenderEntity {
        pick_tris: Vec::new(),
        object,
        snap_pts,
        tangent_geoms: vec![],
        key_vertices,
        fill_tris: vec![],
    }
}

pub(crate) fn measurement_polyline(spl: &Spline) -> Vec<[f64; 3]> {
    let count = spl.control_points.len();
    if count < 2 {
        if spl.fit_points.len() < 2 {
            return spl.control_points.iter().map(|p| [p.x, p.y, p.z]).collect();
        }
        return if spl.flags.closed || spl.flags.periodic {
            catmull_rom_polyline(&spl.fit_points, true)
        } else {
            fit_spline_polyline(spl)
        };
    }

    let degree = spl.degree.max(0) as usize;
    if degree == 0 || degree >= count {
        return spl.control_points.iter().map(|p| [p.x, p.y, p.z]).collect();
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for point in &spl.control_points {
        min[0] = min[0].min(point.x);
        min[1] = min[1].min(point.y);
        min[2] = min[2].min(point.z);
        max[0] = max[0].max(point.x);
        max[1] = max[1].max(point.y);
        max[2] = max[2].max(point.z);
    }
    let diagonal = ((max[0] - min[0]).powi(2)
        + (max[1] - min[1]).powi(2)
        + (max[2] - min[2]).powi(2))
    .sqrt();
    let tolerance = crate::scene::convert::tess_util::fill_chord_tol(diagonal.max(1.0));

    // The kernel's space curve holds the rational and the polynomial case
    // alike — weights absent means polynomial — and samples to a chord
    // tolerance, refined where the curve bends rather than evenly.
    let controls: Vec<[f64; 3]> = spl
        .control_points
        .iter()
        .map(|point| [point.x, point.y, point.z])
        .collect();
    let weights = (spl.weights.len() == count).then(|| {
        spl.weights
            .iter()
            .map(|weight| if weight.abs() < 1e-12 { 1.0 } else { *weight })
            .collect()
    });
    match NurbsCurve3::new(degree, controls, spl.knots.clone(), weights) {
        Some(curve) => curve.tessellate_within(tolerance),
        None => spl.control_points.iter().map(|p| [p.x, p.y, p.z]).collect(),
    }
}

/// Sample a Catmull-Rom spline through `pts` into a dense polyline. The curve
/// passes through every input point; open ends use reflected phantom points so
/// they don't kink, closed curves wrap around.
fn catmull_rom_polyline(pts: &[acadrust::types::Vector3], closed: bool) -> Vec<[f64; 3]> {
    const STEPS: usize = 16;
    let n = pts.len();
    if n < 2 {
        return pts.iter().map(|p| [p.x, p.y, p.z]).collect();
    }
    let get = |i: isize| -> [f64; 3] {
        let j = if closed {
            let m = n as isize;
            (((i % m) + m) % m) as usize
        } else {
            i.clamp(0, n as isize - 1) as usize
        };
        [pts[j].x, pts[j].y, pts[j].z]
    };
    let seg_count = if closed { n } else { n - 1 };
    let mut out: Vec<[f64; 3]> = Vec::with_capacity(seg_count * STEPS + 1);
    for seg in 0..seg_count {
        let p1 = get(seg as isize);
        let p2 = get(seg as isize + 1);
        // Reflect at open ends so the tangent isn't pulled toward a clamped dup.
        let p0 = if !closed && seg == 0 {
            [
                2.0 * p1[0] - p2[0],
                2.0 * p1[1] - p2[1],
                2.0 * p1[2] - p2[2],
            ]
        } else {
            get(seg as isize - 1)
        };
        let p3 = if !closed && seg == seg_count - 1 {
            [
                2.0 * p2[0] - p1[0],
                2.0 * p2[1] - p1[1],
                2.0 * p2[2] - p1[2],
            ]
        } else {
            get(seg as isize + 2)
        };
        // Emit t in [0, 1); the final segment also emits t = 1 so the curve
        // closes onto the last point (no duplicate shared vertices otherwise).
        let last = if seg == seg_count - 1 {
            STEPS
        } else {
            STEPS - 1
        };
        for s in 0..=last {
            let t = s as f64 / STEPS as f64;
            let (t2, t3) = (t * t, t * t * t);
            let mut q = [0.0f64; 3];
            for k in 0..3 {
                q[k] = 0.5
                    * (2.0 * p1[k]
                        + (-p0[k] + p2[k]) * t
                        + (2.0 * p0[k] - 5.0 * p1[k] + 4.0 * p2[k] - p3[k]) * t2
                        + (-p0[k] + 3.0 * p1[k] - 3.0 * p2[k] + p3[k]) * t3);
            }
            out.push(q);
        }
    }
    out
}

/// Interpolate an open fit-point spline into a dense polyline: the C² cubic that
/// passes through every fit point, clamped to the stored start/end tangents when
/// present (natural end otherwise), so its ends follow the specified tangents
/// instead of the local slopes Catmull-Rom would use.
fn fit_spline_polyline(spl: &Spline) -> Vec<[f64; 3]> {
    let p: Vec<[f64; 3]> = spl.fit_points.iter().map(|q| [q.x, q.y, q.z]).collect();
    let n = p.len();
    if n < 2 {
        return p;
    }

    // Parameterise the fit points (unnormalised, so a unit end tangent — how the
    // tangents are stored — is a consistent dP/dt). Match the spline's knot
    // parameterisation: 2 = uniform, 1 = centripetal (√chord), else chord.
    let dist = |a: [f64; 3], b: [f64; 3]| {
        let (dx, dy, dz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    let mut t = vec![0.0f64; n];
    for i in 1..n {
        let d = dist(p[i - 1], p[i]).max(1e-9);
        let step = match spl.knot_parameterization {
            2 => 1.0,
            1 => d.sqrt(),
            _ => d,
        };
        t[i] = t[i - 1] + step;
    }
    let h: Vec<f64> = (0..n - 1).map(|i| (t[i + 1] - t[i]).max(1e-9)).collect();

    let nonzero = |v: &acadrust::types::Vector3| v.x * v.x + v.y * v.y + v.z * v.z > 1e-18;
    let begin = nonzero(&spl.begin_tangent).then(|| {
        [
            spl.begin_tangent.x,
            spl.begin_tangent.y,
            spl.begin_tangent.z,
        ]
    });
    let end = nonzero(&spl.end_tangent)
        .then(|| [spl.end_tangent.x, spl.end_tangent.y, spl.end_tangent.z]);

    // Solve for the knot slopes m_i = dP/dt per coordinate. The tridiagonal is
    // the C² continuity system; the end rows are the clamped tangent (m fixed)
    // or the natural condition (S'' = 0) when no tangent is stored.
    let mut slopes = [vec![0.0f64; n], vec![0.0f64; n], vec![0.0f64; n]];
    for k in 0..3 {
        let seg_slope = |i: usize| (p[i + 1][k] - p[i][k]) / h[i];
        let (mut a, mut b, mut c, mut d) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
        match begin {
            Some(bt) => {
                b[0] = 1.0;
                d[0] = bt[k];
            }
            None => {
                b[0] = 2.0;
                c[0] = 1.0;
                d[0] = 3.0 * seg_slope(0);
            }
        }
        for i in 1..n - 1 {
            a[i] = h[i];
            b[i] = 2.0 * (h[i - 1] + h[i]);
            c[i] = h[i - 1];
            d[i] = 3.0 * (h[i] * seg_slope(i - 1) + h[i - 1] * seg_slope(i));
        }
        match end {
            Some(et) => {
                b[n - 1] = 1.0;
                d[n - 1] = et[k];
            }
            None => {
                a[n - 1] = 1.0;
                b[n - 1] = 2.0;
                d[n - 1] = 3.0 * seg_slope(n - 2);
            }
        }
        thomas_solve(&a, &b, &c, &mut d);
        slopes[k] = d;
    }

    // Evaluate each segment as a cubic Hermite (dP/du = slope · h_i).
    const STEPS: usize = 32;
    let mut out = Vec::with_capacity((n - 1) * STEPS + 1);
    for i in 0..n - 1 {
        let last = if i == n - 2 { STEPS } else { STEPS - 1 };
        for s in 0..=last {
            let u = s as f64 / STEPS as f64;
            let (u2, u3) = (u * u, u * u * u);
            let h00 = 2.0 * u3 - 3.0 * u2 + 1.0;
            let h10 = u3 - 2.0 * u2 + u;
            let h01 = -2.0 * u3 + 3.0 * u2;
            let h11 = u3 - u2;
            let mut q = [0.0f64; 3];
            for k in 0..3 {
                let m0 = slopes[k][i] * h[i];
                let m1 = slopes[k][i + 1] * h[i];
                q[k] = h00 * p[i][k] + h10 * m0 + h01 * p[i + 1][k] + h11 * m1;
            }
            out.push(q);
        }
    }
    out
}

/// In-place Thomas solve for a tridiagonal system (`a` sub-, `b` main-, `c`
/// super-diagonal; `d` right-hand side, overwritten with the solution).
fn thomas_solve(a: &[f64], b: &[f64], c: &[f64], d: &mut [f64]) {
    let n = d.len();
    let mut cp = vec![0.0f64; n];
    cp[0] = c[0] / b[0];
    d[0] /= b[0];
    for i in 1..n {
        let m = b[i] - a[i] * cp[i - 1];
        cp[i] = c[i] / m;
        d[i] = (d[i] - a[i] * d[i - 1]) / m;
    }
    for i in (0..n - 1).rev() {
        d[i] -= cp[i] * d[i + 1];
    }
}

fn grips(spline: &Spline) -> Vec<GripDef> {
    // A fit-point spline carries no control points, so its editable points — and
    // therefore its grips — are the fit points it interpolates. A CV spline
    // grips its control vertices.
    let source = if spline.control_points.is_empty() {
        &spline.fit_points
    } else {
        &spline.control_points
    };
    source
        .iter()
        .enumerate()
        .map(|(i, p)| square_grip(i, glam::DVec3::new(p.x, p.y, p.z)))
        .collect()
}

fn properties(spline: &Spline) -> Vec<PropSection> {
    let show = if spline.fit_points.is_empty() {
        "Control Vertices"
    } else {
        "Fit Points"
    };
    let cp0 = spline.control_points.first();
    let fp0 = spline.fit_points.first();
    let w0 = spline.weights.first().copied();

    // Polyline-approximation length over the effective defining points.
    let pts = if spline.fit_points.is_empty() {
        &spline.control_points
    } else {
        &spline.fit_points
    };
    let mut length = 0.0;
    for w in pts.windows(2) {
        let dx = w[1].x - w[0].x;
        let dy = w[1].y - w[0].y;
        let dz = w[1].z - w[0].z;
        length += (dx * dx + dy * dy + dz * dz).sqrt();
    }

    let closed = spline.flags.closed || spline.flags.periodic;
    let yes_no = |b: bool| if b { "Yes" } else { "No" };

    // Knot parameterization method (R2013+ DWG); older splines report 0.
    let knot_param = match spline.knot_parameterization {
        0 => "Chord",
        1 => "Square Root",
        2 => "Uniform",
        _ => "Custom",
    };

    // Closed splines enclose an area; approximate it with the shoelace
    // formula over the defining points projected to the XY plane, matching
    // the polyline approximation already used for Length.
    let area = if closed && pts.len() >= 3 {
        let mut acc = 0.0;
        for w in pts.windows(2) {
            acc += w[0].x * w[1].y - w[1].x * w[0].y;
        }
        if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
            acc += last.x * first.y - first.x * last.y;
        }
        format!("{:.4}", acc.abs() * 0.5)
    } else {
        String::new()
    };

    vec![
        PropSection {
            title: t!("Data Points").into_owned(),
            props: vec![
                ro(t!("Show").as_ref(), "show", show),
                ro(t!("Degree").as_ref(), "degree", spline.degree.to_string()),
                ro(
                    t!("Control Point Count").as_ref(),
                    "ctrl_pt_count",
                    spline.control_points.len().to_string(),
                ),
                ro(t!("Control Point").as_ref(), "ctrl_pt_index", "0"),
                edit(
                    t!("Control Point X").as_ref(),
                    "ctrl_pt_x",
                    cp0.map(|p| p.x).unwrap_or(0.0),
                ),
                edit(
                    t!("Control Point Y").as_ref(),
                    "ctrl_pt_y",
                    cp0.map(|p| p.y).unwrap_or(0.0),
                ),
                edit(
                    t!("Control Point Z").as_ref(),
                    "ctrl_pt_z",
                    cp0.map(|p| p.z).unwrap_or(0.0),
                ),
                edit(t!("Weight").as_ref(), "weight", w0.unwrap_or(1.0)),
                ro(t!("Knot Parameterization").as_ref(), "knot_param", knot_param),
                ro(
                    t!("Fit Point Count").as_ref(),
                    "fit_pt_count",
                    spline.fit_points.len().to_string(),
                ),
                ro(t!("Fit Point").as_ref(), "fit_pt_index", "0"),
                edit(t!("Fit Point X").as_ref(), "fit_pt_x", fp0.map(|p| p.x).unwrap_or(0.0)),
                edit(t!("Fit Point Y").as_ref(), "fit_pt_y", fp0.map(|p| p.y).unwrap_or(0.0)),
                edit(t!("Fit Point Z").as_ref(), "fit_pt_z", fp0.map(|p| p.z).unwrap_or(0.0)),
                edit(t!("Fit Tolerance").as_ref(), "fit_tolerance", spline.fit_tolerance),
                edit(t!("Start Tangent X").as_ref(), "start_tan_x", spline.begin_tangent.x),
                edit(t!("Start Tangent Y").as_ref(), "start_tan_y", spline.begin_tangent.y),
                edit(t!("Start Tangent Z").as_ref(), "start_tan_z", spline.begin_tangent.z),
                edit(t!("End Tangent X").as_ref(), "end_tan_x", spline.end_tangent.x),
                edit(t!("End Tangent Y").as_ref(), "end_tan_y", spline.end_tangent.y),
                edit(t!("End Tangent Z").as_ref(), "end_tan_z", spline.end_tangent.z),
            ],
        },
        PropSection {
            title: t!("Misc").into_owned(),
            props: vec![
                ro(t!("Closed").as_ref(), "closed", yes_no(closed)),
                ro(t!("Planar").as_ref(), "planar", yes_no(spline.flags.planar)),
                ro(t!("Length").as_ref(), "length", format!("{length:.4}")),
                ro(t!("Area").as_ref(), "area", area),
            ],
        },
    ]
}

fn apply_geom_prop(spline: &mut Spline, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else { return };
    match field {
        "ctrl_pt_x" => {
            if let Some(cp) = spline.control_points.first_mut() {
                cp.x = v;
            }
        }
        "ctrl_pt_y" => {
            if let Some(cp) = spline.control_points.first_mut() {
                cp.y = v;
            }
        }
        "ctrl_pt_z" => {
            if let Some(cp) = spline.control_points.first_mut() {
                cp.z = v;
            }
        }
        "weight" => {
            if let Some(w) = spline.weights.first_mut() {
                *w = v;
            }
        }
        "fit_pt_x" => {
            if let Some(fp) = spline.fit_points.first_mut() {
                fp.x = v;
            }
        }
        "fit_pt_y" => {
            if let Some(fp) = spline.fit_points.first_mut() {
                fp.y = v;
            }
        }
        "fit_pt_z" => {
            if let Some(fp) = spline.fit_points.first_mut() {
                fp.z = v;
            }
        }
        "fit_tolerance" => spline.fit_tolerance = v,
        "start_tan_x" => spline.begin_tangent.x = v,
        "start_tan_y" => spline.begin_tangent.y = v,
        "start_tan_z" => spline.begin_tangent.z = v,
        "end_tan_x" => spline.end_tangent.x = v,
        "end_tan_y" => spline.end_tangent.y = v,
        "end_tan_z" => spline.end_tangent.z = v,
        _ => {}
    }
}

fn apply_grip(spline: &mut Spline, grip_id: usize, apply: GripApply) {
    // Match the grip source: dragging a fit-point spline's grip moves the fit
    // point (the curve re-fits through it on the next tessellation, keeping the
    // stored end tangents); a CV spline moves the control vertex.
    let target = if spline.control_points.is_empty() {
        spline.fit_points.get_mut(grip_id)
    } else {
        spline.control_points.get_mut(grip_id)
    };
    if let Some(cp) = target {
        match apply {
            GripApply::Absolute(p) => {
                cp.x = p.x as f64;
                cp.y = p.y as f64;
                cp.z = p.z as f64;
            }
            GripApply::Translate(d) => {
                cp.x += d.x as f64;
                cp.y += d.y as f64;
                cp.z += d.z as f64;
            }
        }
    }
}

fn apply_transform(spline: &mut Spline, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(spline, t, |entity, p1, p2| {
        for cp in &mut entity.control_points {
            crate::scene::view::transform::reflect_xy_point(&mut cp.x, &mut cp.y, p1, p2);
        }
        for fp in &mut entity.fit_points {
            crate::scene::view::transform::reflect_xy_point(&mut fp.x, &mut fp.y, p1, p2);
        }
    });
}

impl RenderConvertible for Spline {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self))
    }
}

impl crate::entities::traits::Grippable for Spline {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
    fn grip_menu(&self, _grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Add Vertex",
                action: GripMenuAction::AddVertex,
            },
            GripMenuItem {
                label: "Remove Vertex",
                action: GripMenuAction::RemoveVertex,
            },
            GripMenuItem {
                label: "Refine Vertices",
                action: GripMenuAction::RefineVertices,
            },
        ]
    }
    fn apply_grip_menu(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        let n = self.control_points.len();
        let min_cv = (self.degree as usize).saturating_add(1).max(2);
        match action {
            A::AddVertex if grip_id < n => {
                let i1 = (grip_id + 1).min(n - 1);
                if i1 == grip_id {
                    return;
                }
                let p0 = &self.control_points[grip_id];
                let p1 = &self.control_points[i1];
                let mid = acadrust::types::Vector3::new(
                    (p0.x + p1.x) * 0.5,
                    (p0.y + p1.y) * 0.5,
                    (p0.z + p1.z) * 0.5,
                );
                self.control_points.insert(i1, mid);
                if !self.weights.is_empty() && self.weights.len() == n {
                    let w = (self.weights[grip_id] + self.weights[i1.min(self.weights.len() - 1)])
                        * 0.5;
                    self.weights.insert(i1, w);
                }
                // Clear knots so to_render rebuilds a uniform knot vector
                // for the new CV count.
                self.knots.clear();
            }
            A::RemoveVertex if grip_id < n && n > min_cv => {
                self.control_points.remove(grip_id);
                if grip_id < self.weights.len() {
                    self.weights.remove(grip_id);
                }
                self.knots.clear();
            }
            A::RefineVertices => {
                // Insert a CV between every adjacent pair (chord midpoints)
                // and rebuild a uniform knot vector.
                if n >= 2 {
                    let mut refined = Vec::with_capacity(n * 2 - 1);
                    let mut refined_w = Vec::with_capacity(n * 2 - 1);
                    let has_w = !self.weights.is_empty() && self.weights.len() == n;
                    for i in 0..n {
                        refined.push(self.control_points[i].clone());
                        if has_w {
                            refined_w.push(self.weights[i]);
                        }
                        if i + 1 < n {
                            let a = &self.control_points[i];
                            let b = &self.control_points[i + 1];
                            refined.push(acadrust::types::Vector3::new(
                                (a.x + b.x) * 0.5,
                                (a.y + b.y) * 0.5,
                                (a.z + b.z) * 0.5,
                            ));
                            if has_w {
                                refined_w.push((self.weights[i] + self.weights[i + 1]) * 0.5);
                            }
                        }
                    }
                    self.control_points = refined;
                    if has_w {
                        self.weights = refined_w;
                    }
                    self.knots.clear();
                }
            }
            _ => {}
        }
    }
}

impl crate::entities::traits::PropertyEditable for Spline {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }
    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl crate::entities::traits::Transformable for Spline {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
