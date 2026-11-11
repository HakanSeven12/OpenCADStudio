//! Writing a whole body into a document as new records.
//!
//! Records are allocated before their circular topology pointers are filled.

use document::entities::acis::types::{SatDocument, SatPointer, SatRecord, SatToken};
use document::entities::acis::SplineNet;
use kernel::brep::{
    Body, CoedgeKey, Curve3, CurveKey, EdgeKey, FaceKey, LoopKey, LumpKey, ShellKey, Surface,
    SurfaceKey, VertexKey,
};
use kernel::geom2d::Curve as Curve2;
use kernel::space::Vec3;
use std::collections::HashMap;

/// Why a body could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unappendable {
    /// A curve or pcurve has no supported SAT record form.
    Curve,
    /// A surface whose frame is degenerate, so there is no normal to write.
    Surface,
    /// The body's own topology does not hold together. Writing it would
    /// produce a document that parses into something else.
    Inconsistent,
}

/// Where the body ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Record index of the `body` record.
    pub body: i32,
    /// How many records were added in total.
    pub records: usize,
}

/// Appends a body to a document as a fresh set of records.
///
/// The document keeps everything it already had; the body is added beside it.
/// A caller replacing an existing solid removes the old records itself —
/// which record refers to a body from outside the ACIS stream is the caller's
/// business, not this function's.
pub fn append(body: &Body, document: &mut SatDocument) -> Result<Written, Unappendable> {
    // A non-manifold edge is legal ACIS: its coedges' partners form one
    // ring round the edge (see `radial_next`). Anything else is not.
    if body
        .validate()
        .iter()
        .any(|flaw| !matches!(flaw, kernel::brep::Flaw::NonManifoldEdge(_)))
    {
        return Err(Unappendable::Inconsistent);
    }
    let before = document.record_count();
    let mut ids = Ids::default();

    // First pass: every record, with geometry but no pointers.
    for (key, vertex) in body.vertices.iter() {
        let point = add(document, "point", vec![position(vertex.point)]);
        ids.points.insert(key, point);
        ids.vertices.insert(key, add(document, "vertex", Vec::new()));
    }
    for (key, curve) in body.curves.iter() {
        let id = add_curve(document, curve).ok_or(Unappendable::Curve)?;
        ids.curves.insert(key, id);
    }
    for (key, surface) in body.surfaces.iter() {
        let id = match surface {
            Surface::Nurbs(surface) => add_nurbs_surface(document, surface),
            Surface::Offset(offset) => add_offset_surface(document, offset),
            _ => {
                let (kind, tokens) = surface_record(surface).ok_or(Unappendable::Surface)?;
                add(document, kind, tokens)
            }
        };
        ids.surfaces.insert(key, id);
    }
    for key in body.edges.keys() {
        ids.edges.insert(key, add(document, "edge", Vec::new()));
    }
    for (key, coedge) in body.coedges.iter() {
        if let Some(curve) = &coedge.pcurve {
            let face = body
                .loops
                .get(coedge.owner)
                .and_then(|ring| body.faces.get(ring.owner))
                .ok_or(Unappendable::Inconsistent)?;
            let needs_curve = body.surfaces.get(face.surface).is_some_and(Surface::is_spline);
            // A pcurve that cannot be put in the edge's parameterisation (a
            // degenerate edge has no range to map onto) is written as it is,
            // as before, rather than refusing the whole body.
            let curve = acis_pcurve(body, coedge, face.surface, curve).unwrap_or_else(|| curve.clone());
            match add_pcurve(document, &curve, ids.surface(face.surface)) {
                Some(id) => {
                    ids.pcurves.insert(key, id);
                }
                None if needs_curve => return Err(Unappendable::Curve),
                None => {}
            }
        }
        ids.coedges.insert(key, add(document, "coedge", Vec::new()));
    }
    for key in body.loops.keys() {
        ids.loops.insert(key, add(document, "loop", Vec::new()));
    }
    for key in body.faces.keys() {
        ids.faces.insert(key, add(document, "face", Vec::new()));
    }
    for key in body.shells.keys() {
        ids.shells.insert(key, add(document, "shell", Vec::new()));
    }
    for key in body.lumps.keys() {
        ids.lumps.insert(key, add(document, "lump", Vec::new()));
    }
    let body_id = add(document, "body", Vec::new());

    // Second pass: the pointers, now that every index exists.
    for (key, vertex) in body.vertices.iter() {
        let edge = body
            .edges
            .iter()
            .find(|(_, node)| node.start == key || node.end == key)
            .map(|(edge, _)| ids.edge(edge))
            .unwrap_or(NULL);
        set(
            document,
            ids.vertex(key),
            vec![null(), pointer(edge), pointer(ids.point(key))],
        );
        let _ = vertex;
    }

    for (key, edge) in body.edges.iter() {
        let coedge = edge.coedges.first().map(|c| ids.coedge(*c)).unwrap_or(NULL);
        set(
            document,
            ids.edge(key),
            vec![
                null(),
                pointer(ids.vertex(edge.start)),
                SatToken::Float(edge.start_parameter),
                pointer(ids.vertex(edge.end)),
                SatToken::Float(edge.end_parameter),
                pointer(coedge),
                pointer(ids.curve(edge.curve)),
                sense(true),
            ],
        );
    }

    for (key, ring) in body.loops.iter() {
        let count = ring.coedges.len();
        for (order, coedge) in ring.coedges.iter().enumerate() {
            let node = body
                .coedges
                .get(*coedge)
                .ok_or(Unappendable::Inconsistent)?;
            let next = ring.coedges[(order + 1) % count];
            let previous = ring.coedges[(order + count - 1) % count];
            let partner = body.radial_next(*coedge).map(|c| ids.coedge(c)).unwrap_or(NULL);
            set(
                document,
                ids.coedge(*coedge),
                vec![
                    null(),
                    pointer(ids.coedge(next)),
                    pointer(ids.coedge(previous)),
                    pointer(partner),
                    pointer(ids.edge(node.edge)),
                    sense(node.forward),
                    pointer(ids.loop_(key)),
                    pointer(ids.pcurve(*coedge)),
                ],
            );
        }
        let face = body.faces.get(ring.owner).ok_or(Unappendable::Inconsistent)?;
        set(
            document,
            ids.loop_(key),
            vec![
                null(),
                pointer(next_in(&face.loops, key, &ids.loops)),
                pointer(ring.coedges.first().map(|c| ids.coedge(*c)).unwrap_or(NULL)),
                pointer(ids.face(ring.owner)),
            ],
        );
    }

    for (key, face) in body.faces.iter() {
        let shell = body.shells.get(face.owner).ok_or(Unappendable::Inconsistent)?;
        set(
            document,
            ids.face(key),
            vec![
                null(),
                pointer(next_in(&shell.faces, key, &ids.faces)),
                pointer(face.loops.first().map(|l| ids.loop_(*l)).unwrap_or(NULL)),
                pointer(ids.shell(face.owner)),
                null(),
                pointer(ids.surface(face.surface)),
                // A torus written with a negative minor radius faces into
                // its tube once read, so the face turns back for it.
                sense(face.forward != inward_torus(body.surfaces.get(face.surface))),
                SatToken::Ident("single".to_string()),
            ],
        );
    }

    for (key, shell) in body.shells.iter() {
        let lump = body.lumps.get(shell.owner).ok_or(Unappendable::Inconsistent)?;
        set(
            document,
            ids.shell(key),
            vec![
                null(),
                pointer(next_in(&lump.shells, key, &ids.shells)),
                null(),
                pointer(shell.faces.first().map(|f| ids.face(*f)).unwrap_or(NULL)),
                null(),
                pointer(ids.lump(shell.owner)),
            ],
        );
    }

    for (key, lump) in body.lumps.iter() {
        set(
            document,
            ids.lump(key),
            vec![
                null(),
                pointer(next_in(&body.roots, key, &ids.lumps)),
                pointer(lump.shells.first().map(|s| ids.shell(*s)).unwrap_or(NULL)),
                pointer(body_id),
            ],
        );
    }

    let first_lump = body.roots.first().map(|l| ids.lump(*l)).unwrap_or(NULL);
    set(
        document,
        body_id,
        vec![null(), pointer(first_lump), null(), null()],
    );

    Ok(Written {
        body: body_id,
        records: document.record_count() - before,
    })
}

/// Appends a straight-segment polyline as a wire body, the form the
/// reference modeler keeps a polyline sweep path in. A closed polyline
/// ends where it starts; an open one's end coedges point at themselves.
pub fn append_polyline_wire(
    points: &[[f64; 3]],
    closed: bool,
    document: &mut SatDocument,
) -> Result<Written, Unappendable> {
    let mut points = points.to_vec();
    // A closed list may repeat its first point at the end, to rounding.
    let extent = points
        .iter()
        .map(|point| Vec3::from(*point).distance(Vec3::from(points[0])))
        .fold(0.0, f64::max);
    let repeated = |a: [f64; 3], b: [f64; 3]| Vec3::from(a).distance(Vec3::from(b)) <= extent * 1e-12;
    if closed && points.len() > 2 && repeated(points[0], points[points.len() - 1]) {
        points.pop();
    }
    let spans = if closed { points.len() } else { points.len().saturating_sub(1) };
    if spans == 0
        || (closed && points.len() < 3)
        || points.iter().flatten().any(|value| !value.is_finite())
    {
        return Err(Unappendable::Inconsistent);
    }
    let before = document.record_count();
    let mut vertex_ids = Vec::new();
    let mut point_ids = Vec::new();
    for point in &points {
        point_ids.push(add(document, "point", vec![position(*point)]));
        vertex_ids.push(add(document, "vertex", Vec::new()));
    }
    let mut curves = Vec::new();
    let mut lengths = Vec::new();
    for index in 0..spans {
        let (start, end) = (Vec3::from(points[index]), Vec3::from(points[(index + 1) % points.len()]));
        let length = start.distance(end);
        let direction = (end - start).normalize().ok_or(Unappendable::Inconsistent)?;
        let line = Curve3::Line(kernel::brep::Line3 { origin: start.to_array(), direction: direction.to_array() });
        curves.push(add_curve(document, &line).ok_or(Unappendable::Curve)?);
        lengths.push(length);
    }
    let edges = (0..spans).map(|_| add(document, "edge", Vec::new())).collect::<Vec<_>>();
    let coedges = (0..spans).map(|_| add(document, "coedge", Vec::new())).collect::<Vec<_>>();
    let wire = add(document, "wire", Vec::new());
    let shell = add(document, "shell", Vec::new());
    let lump = add(document, "lump", Vec::new());
    let body = add(document, "body", Vec::new());
    for (index, vertex) in vertex_ids.iter().enumerate() {
        let edge = edges[index.min(spans - 1)];
        set(document, *vertex, vec![null(), pointer(edge), pointer(point_ids[index])]);
    }
    for index in 0..spans {
        set(document, edges[index], vec![
            null(),
            pointer(vertex_ids[index]),
            SatToken::Float(0.0),
            pointer(vertex_ids[(index + 1) % points.len()]),
            SatToken::Float(lengths[index]),
            pointer(coedges[index]),
            pointer(curves[index]),
            sense(true),
        ]);
        let next = if index + 1 < spans { coedges[index + 1] } else if closed { coedges[0] } else { coedges[index] };
        let previous = if index > 0 { coedges[index - 1] } else if closed { coedges[spans - 1] } else { coedges[0] };
        set(document, coedges[index], vec![
            null(),
            pointer(next),
            pointer(previous),
            null(),
            pointer(edges[index]),
            sense(true),
            pointer(wire),
            null(),
        ]);
    }
    set(document, wire, vec![null(), null(), pointer(coedges[0]), pointer(shell), null(), SatToken::Ident("out".to_string())]);
    set(document, shell, vec![null(), null(), null(), null(), pointer(wire), pointer(lump)]);
    set(document, lump, vec![null(), null(), pointer(shell), pointer(body)]);
    set(document, body, vec![null(), pointer(lump), null(), null()]);
    Ok(Written { body, records: document.record_count() - before })
}

const NULL: i32 = -1;

/// The record after `key` in an ownership list, or null at the end.
///
/// ACIS strings siblings together rather than listing them, so a shell holds
/// its first face and every face holds the next. Getting this wrong loses
/// every face after the first without anything else noticing.
fn next_in<T: Copy + PartialEq + std::hash::Hash + Eq>(
    order: &[T],
    key: T,
    ids: &HashMap<T, i32>,
) -> i32 {
    order
        .iter()
        .position(|item| *item == key)
        .and_then(|at| order.get(at + 1))
        .and_then(|next| ids.get(next).copied())
        .unwrap_or(NULL)
}

#[derive(Default)]
struct Ids {
    points: HashMap<VertexKey, i32>,
    vertices: HashMap<VertexKey, i32>,
    curves: HashMap<CurveKey, i32>,
    surfaces: HashMap<SurfaceKey, i32>,
    edges: HashMap<EdgeKey, i32>,
    coedges: HashMap<CoedgeKey, i32>,
    pcurves: HashMap<CoedgeKey, i32>,
    loops: HashMap<LoopKey, i32>,
    faces: HashMap<FaceKey, i32>,
    shells: HashMap<ShellKey, i32>,
    lumps: HashMap<LumpKey, i32>,
}

macro_rules! lookup {
    ($name:ident, $field:ident, $key:ty) => {
        fn $name(&self, key: $key) -> i32 {
            self.$field.get(&key).copied().unwrap_or(NULL)
        }
    };
}

impl Ids {
    lookup!(point, points, VertexKey);
    lookup!(vertex, vertices, VertexKey);
    lookup!(curve, curves, CurveKey);
    lookup!(surface, surfaces, SurfaceKey);
    lookup!(edge, edges, EdgeKey);
    lookup!(coedge, coedges, CoedgeKey);
    lookup!(pcurve, pcurves, CoedgeKey);
    lookup!(loop_, loops, LoopKey);
    lookup!(face, faces, FaceKey);
    lookup!(shell, shells, ShellKey);
    lookup!(lump, lumps, LumpKey);
}

fn add(document: &mut SatDocument, kind: &str, mut tokens: Vec<SatToken>) -> i32 {
    // Every record leads with its attribute pointer; the accessors index from
    // one.
    let mut all = vec![null()];
    all.append(&mut tokens);
    document.add_record(SatRecord {
        index: -1,
        entity_type: kind.to_string(),
        sub_type: None,
        attribute: SatPointer::new(NULL),
        subtype_id: -1,
        tokens: all,
        raw_text: None,
    })
}

fn set(document: &mut SatDocument, id: i32, tokens: Vec<SatToken>) {
    // Ids from `add_record` are array positions, so this is a direct index
    // rather than a search.
    if let Some(record) = document.record_mut(id as usize) {
        record.tokens = tokens;
    }
}

fn distinct_knots(knots: &[f64]) -> Vec<(f64, i32)> {
    let mut out: Vec<(f64, i32)> = Vec::new();
    for knot in knots {
        match out.last_mut() {
            Some((value, multiplicity)) if *value == *knot => *multiplicity += 1,
            _ => out.push((*knot, 1)),
        }
    }
    out
}

/// A spline's control net laid out as ACIS takes it: distinct knots with
/// their multiplicities, control points v-major, weights only when they vary.
struct Net {
    rational: bool,
    degrees: (i32, i32),
    periodic: [bool; 2],
    u_knots: Vec<(f64, i32)>,
    v_knots: Vec<(f64, i32)>,
    points: Vec<[f64; 3]>,
    weights: Vec<f64>,
}

impl Net {
    fn of(surface: &kernel::space::NurbsSurface3) -> Self {
        let (u_degree, v_degree) = surface.degrees();
        let (u_knots, v_knots) = surface.knots();
        let controls = surface.control_points();
        let weights = surface.weights();
        let rational = weights
            .iter()
            .flatten()
            .next()
            .is_some_and(|first| {
                weights
                    .iter()
                    .flatten()
                    .any(|weight| (*weight - *first).abs() > 1e-12)
            });
        let mut points = Vec::new();
        let mut flat_weights = Vec::new();
        for v in 0..controls[0].len() {
            for u in 0..controls.len() {
                points.push(controls[u][v]);
                flat_weights.push(weights[u][v]);
            }
        }
        Self {
            rational,
            degrees: (u_degree as i32, v_degree as i32),
            periodic: surface.periodicity(),
            u_knots: distinct_knots(u_knots),
            v_knots: distinct_knots(v_knots),
            points,
            weights: flat_weights,
        }
    }

    fn view(&self) -> SplineNet<'_> {
        SplineNet {
            rational: self.rational,
            deg_u: self.degrees.0,
            deg_v: self.degrees.1,
            u_closed: self.periodic[0],
            v_closed: self.periodic[1],
            u_knots: &self.u_knots,
            v_knots: &self.v_knots,
            control: &self.points,
            weights: self.rational.then_some(self.weights.as_slice()),
            fit_tol: 0.0,
        }
    }
}

fn add_nurbs_surface(document: &mut SatDocument, surface: &kernel::space::NurbsSurface3) -> i32 {
    let net = Net::of(surface);
    let view = net.view();
    document.add_spline_surface(
        surface.v_reversed(),
        view.rational,
        view.deg_u,
        view.deg_v,
        view.u_closed,
        view.v_closed,
        view.u_knots,
        view.v_knots,
        view.control,
        view.weights,
        view.fit_tol,
    )
}

/// An offset surface as its ACIS 7 `offsur` record: the base, the distance
/// and the approximation the kernel evaluates.
fn add_offset_surface(document: &mut SatDocument, offset: &kernel::brep::OffsetSurface) -> i32 {
    let (base, approximation) = (Net::of(&offset.base), Net::of(&offset.approximation));
    document.add_offset_spline_surface(
        offset.approximation.v_reversed(),
        offset.base.v_reversed(),
        &base.view(),
        offset.distance,
        &approximation.view(),
    )
}

fn add_curve(document: &mut SatDocument, curve: &Curve3) -> Option<i32> {
    let spline = match curve {
        Curve3::PlanarSpline { plane, curve } => {
            let (start, end) = curve.domain();
            let width = end - start;
            if !width.is_finite() || width <= 0.0 {
                return None;
            }
            let knots = curve
                .knots()
                .iter()
                .map(|knot| (knot - start) / width)
                .collect();
            kernel::space::NurbsCurve3::new_strict(
                curve.degree(),
                curve
                    .control_points()
                    .iter()
                    .map(|point| plane.point_at(*point))
                    .collect(),
                knots,
                curve.weights().to_vec(),
            )?
            .with_periodicity(curve.is_closed())
        }
        Curve3::Nurbs(curve) => curve.clone(),
        _ => {
            let (kind, tokens) = curve_record(curve)?;
            return Some(add(document, kind, tokens));
        }
    };
    Some(document.add_spline_curve(
        spline.is_rational(),
        spline.degree() as i32,
        spline.periodicity(),
        &distinct_knots(spline.knots()),
        spline.control_points(),
        spline.is_rational().then_some(spline.weights()),
        0.0,
    ))
}

/// A coedge's pcurve in the parameterisation ACIS expects: it runs with the
/// edge and over the edge's own parameter range on a forward coedge, and
/// over the negated range (so against the edge) on a reversed one. The
/// kernel keeps pcurves as plain traces in their own parameters, often in
/// the loop's direction, which the reference rejects as an invalid solid.
fn acis_pcurve(
    body: &Body,
    coedge: &kernel::brep::Coedge,
    surface: SurfaceKey,
    pcurve: &Curve2,
) -> Option<Curve2> {
    use kernel::geom2d::NurbsCurve;
    let nurbs = match pcurve {
        Curve2::Line(line) => NurbsCurve::new_strict(1, vec![line.start, line.end], vec![0.0, 0.0, 1.0, 1.0], vec![1.0, 1.0])?,
        Curve2::Nurbs(curve) => curve.clone(),
        _ => return None,
    };
    let surface = body.surfaces.get(surface)?;
    let edge = body.edges.get(coedge.edge)?;
    let curve = body.curves.get(edge.curve)?;
    let (a, b) = (edge.start_parameter, edge.end_parameter);
    let (start, end) = (Vec3::from(curve.point_at(a)), Vec3::from(curve.point_at(b)));
    let image = |curve: &NurbsCurve, t: f64| {
        let uv = curve.point_at(t);
        Vec3::from(surface.point_at(uv[0], uv[1]))
    };
    let (t0, t1) = nurbs.domain();
    let (first, last) = (image(&nurbs, t0), image(&nurbs, t1));
    let along = first.distance(start) + last.distance(end);
    let against = first.distance(end) + last.distance(start);
    let mut nurbs = if against < along { nurbs.reversed() } else { nurbs };
    let (low, high) = if coedge.forward { (a, b) } else {
        nurbs = nurbs.reversed();
        (-b, -a)
    };
    let (t0, t1) = nurbs.domain();
    let scale = (high - low) / (t1 - t0);
    let knots = nurbs.knots().iter().map(|knot| low + (knot - t0) * scale).collect();
    let controls = nurbs.control_points().iter().map(|point| acis_uv(surface, *point)).collect();
    NurbsCurve::new_strict(nurbs.degree(), controls, knots, nurbs.weights().to_vec())
        .map(Curve2::Nurbs)
}

/// A surface point's kernel parameters in the modeller's own: a cone or
/// cylinder runs u along its generators, in units of the base radius, and v
/// round it; a torus or sphere takes its two angles the other way round.
/// Both maps are linear, so a pcurve's control points map exactly.
pub(super) fn acis_uv(surface: &Surface, [u, v]: [f64; 2]) -> [f64; 2] {
    match surface {
        Surface::Cylinder(cylinder) => [v / cylinder.radius, u],
        Surface::Cone(cone) => [v / (cone.radius * cone.half_angle.cos()), u],
        Surface::Torus(_) | Surface::Sphere(_) => [v, u],
        _ => [u, v],
    }
}

/// The inverse of [`acis_uv`].
pub(super) fn kernel_uv(surface: &Surface, [u, v]: [f64; 2]) -> [f64; 2] {
    match surface {
        Surface::Cylinder(cylinder) => [v, u * cylinder.radius],
        Surface::Cone(cone) => [v, u * cone.radius * cone.half_angle.cos()],
        Surface::Torus(_) | Surface::Sphere(_) => [v, u],
        _ => [u, v],
    }
}

fn add_pcurve(
    document: &mut SatDocument,
    curve: &Curve2,
    support_surface: i32,
) -> Option<i32> {
    let (degree, knots, controls, weights, rational, closed) = match curve {
        Curve2::Line(line) => (
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![line.start, line.end],
            vec![1.0, 1.0],
            false,
            false,
        ),
        Curve2::Nurbs(curve) => (
            curve.degree(),
            curve.knots().to_vec(),
            curve.control_points().to_vec(),
            curve.weights().to_vec(),
            curve.is_rational(),
            curve.point_at_knot(curve.domain().0) == curve.point_at_knot(curve.domain().1),
        ),
        _ => return None,
    };
    Some(document.add_pcurve(
        rational,
        degree as i32,
        closed,
        &distinct_knots(&knots),
        &controls,
        rational.then_some(weights.as_slice()),
        0.0,
        support_surface,
        // The spline knots carry the parameter interval. These are UV
        // translations on the support surface, not edge-domain endpoints.
        (0.0, 0.0),
    ))
}

pub(super) fn surface_record(surface: &Surface) -> Option<(&'static str, Vec<SatToken>)> {
    let frame = surface.frame()?;
    let normal = frame.normal()?;
    let origin = frame.origin;
    let u = frame.x_axis;
    Some(match surface {
        Surface::Plane(_) => (
            "plane-surface",
            vec![position(origin), position(normal), position(u)],
        ),
        Surface::Cylinder(cylinder) => (
            "cone-surface",
            cone_tokens(origin, normal, u, cylinder.radius, 0.0),
        ),
        Surface::Cone(cone) => (
            "cone-surface",
            cone_tokens(origin, normal, u, cone.radius, cone.half_angle),
        ),
        Surface::Sphere(sphere) => (
            "sphere-surface",
            vec![
                position(origin),
                SatToken::Float(sphere.radius),
                position(u),
                position(normal),
            ],
        ),
        Surface::Torus(torus) => (
            "torus-surface",
            vec![
                position(origin),
                position(normal),
                SatToken::Float(torus.major_radius),
                SatToken::Float(torus.minor_radius),
                position(u),
            ],
        ),
        Surface::Nurbs(_) | Surface::Offset(_) => return None,
    })
}

/// A cone record, cylinder included — ACIS has no separate cylinder record,
/// only a cone whose half-angle is zero.
///
/// The radius is carried as the *length* of the major axis, which is how it
/// is read back; a unit major axis with the radius beside it produces a cone
/// of radius one. The two continuation tokens before the half-angle are not
/// decoration: the reader looks for the sine at thirteen.
fn cone_tokens(
    origin: [f64; 3],
    axis: [f64; 3],
    u: [f64; 3],
    radius: f64,
    half_angle: f64,
) -> Vec<SatToken> {
    let major = (Vec3::from(u) * radius).to_array();
    let (sine, cosine) = (-half_angle).sin_cos();
    vec![
        position(origin),
        position(axis),
        position(major),
        SatToken::Float(1.0),
        SatToken::Ident("I".to_string()),
        SatToken::Ident("I".to_string()),
        SatToken::Float(sine),
        SatToken::Float(cosine),
        SatToken::Float(radius),
    ]
}

pub(super) fn curve_record(curve: &Curve3) -> Option<(&'static str, Vec<SatToken>)> {
    Some(match curve {
        Curve3::Line(line) => (
            "straight-curve",
            vec![position(line.origin), position(line.direction)],
        ),
        Curve3::Circle(circle) => (
            "ellipse-curve",
            ellipse_tokens(
                circle.plane.origin,
                circle.plane.normal()?,
                circle.plane.x_axis,
                circle.radius,
                1.0,
            ),
        ),
        Curve3::Ellipse(ellipse) => (
            "ellipse-curve",
            ellipse_tokens(
                ellipse.plane.origin,
                ellipse.plane.normal()?,
                ellipse.plane.x_axis,
                ellipse.major_radius,
                ellipse.minor_radius / ellipse.major_radius,
            ),
        ),
        Curve3::PlanarSpline { .. } | Curve3::Nurbs(_) => return None,
    })
}

fn ellipse_tokens(
    centre: [f64; 3],
    normal: [f64; 3],
    u: [f64; 3],
    radius: f64,
    ratio: f64,
) -> Vec<SatToken> {
    vec![
        position(centre),
        position(normal),
        position((Vec3::from(u) * radius).to_array()),
        SatToken::Float(ratio),
    ]
}

pub(super) fn position(value: [f64; 3]) -> SatToken {
    SatToken::Position(value[0], value[1], value[2])
}

fn pointer(id: i32) -> SatToken {
    SatToken::Pointer(SatPointer::new(id))
}

pub(super) fn null() -> SatToken {
    pointer(NULL)
}

fn inward_torus(surface: Option<&Surface>) -> bool {
    matches!(surface, Some(Surface::Torus(torus)) if torus.minor_radius < 0.0)
}

fn sense(forward: bool) -> SatToken {
    SatToken::Ident(if forward { "forward" } else { "reversed" }.to_string())
}
