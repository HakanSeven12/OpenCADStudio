//! Reading an ACIS document into Solidcore records.
//!
//! Each body's vertices, edges (on their curves, between their vertices) and faces (on their
//! surfaces, bounded by their loops) go through Solidcore's assembler
//! ([`solidcore_step::assembly`]), the stage its STEP reader builds records with: it makes the
//! seams, every use's pcurve, the face domains, tolerances, regions and the canonical order, so
//! the file's own pcurves are not read. What a record cannot hold is listed in [`Loss`].

use std::collections::HashMap;
use std::f64::consts::{FRAC_1_SQRT_2, TAU};

use document::entities::acis::types::{
    SatBSplineSurface, SatBody, SatCoedge, SatConeSurface, SatDocument, SatEdge, SatEllipseCurve,
    SatFace, SatIntCurve, SatLoop, SatLump, SatPlaneSurface, SatPoint, SatPointer, SatRecord,
    SatShell, SatSphereSurface, SatSplineSurface, SatStraightCurve, SatTorusSurface, SatVertex,
    Sense,
};
use solidcore_record::brep as r;
use solidcore_step::assembly::{Assembly, Bound, Kind};

use crate::geometry::{
    add, cross, dot, frame, length, parameter_on, points_on, scale, sub, turn_near, unit, Place, Traced,
};

/// What a lift could not represent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loss {
    /// Records whose surface kind has no record equivalent, by index.
    pub surfaces: Vec<usize>,
    /// Records whose curve kind has no record equivalent, by index.
    pub curves: Vec<usize>,
    /// Records the pointer graph named but that are missing or malformed, and bodies the
    /// assembler refused.
    pub broken: Vec<usize>,
    /// Why the assembler refused a body or a face.
    pub refusals: Vec<String>,
}

impl Loss {
    /// Whether the document lifted completely.
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty() && self.curves.is_empty() && self.broken.is_empty() && self.refusals.is_empty()
    }
}

/// The assembler's tolerance, ACIS's resabs.
const TOLERANCE: f64 = 1e-6;

static TRIMS: Traced = Traced;

/// A body read from a document.
#[derive(Debug, Clone)]
pub struct Lifted {
    /// The body.
    pub brep: r::Brep,
    /// The SAT `face` record each record face was read from, in the record's face order: what
    /// the document's face attributes (colours, materials) name.
    pub face_records: Vec<u32>,
    /// The SAT `edge` record each record edge was read from, where one was: what the
    /// document's edge attributes name.
    pub edge_records: Vec<Option<u32>>,
    /// The record index of the body's `body` record.
    pub body_record: Option<u32>,
}

/// Every body in the document, in the order they appear.
pub fn lift(document: &SatDocument) -> (Vec<Lifted>, Loss) {
    let mut loss = Loss::default();
    let indices: Vec<Option<u32>> = document
        .records_of_type("body")
        .iter()
        .map(|record| index_of(record))
        .collect();
    let bodies = document
        .bodies()
        .into_iter()
        .enumerate()
        .filter_map(|(order, body)| {
            lift_one(document, &body, indices.get(order).copied().flatten(), &mut loss)
        })
        .collect();
    (bodies, loss)
}

/// One body, named by the record index of its `body` record.
pub fn lift_body(document: &SatDocument, record: usize) -> Option<(Lifted, Loss)> {
    let source = document.record(record)?;
    let body = SatBody::from_record(source)?;
    let mut loss = Loss::default();
    let lifted = lift_one(document, &body, index_of(source), &mut loss)?;
    Some((lifted, loss))
}

/// A body being read.
struct Reader<'d> {
    document: &'d SatDocument,
    assembly: Assembly<'static>,
    /// SAT vertex record → assembled vertex.
    vertices: HashMap<u32, u32>,
    /// SAT edge record → assembled edge, and whether it runs the SAT edge's way (start to end).
    edges: HashMap<u32, Option<(u32, bool)>>,
    /// Each assembled edge's curve and interval, to sample.
    curves: HashMap<u32, (r::Curve, [f64; 2])>,
    /// The SAT edge record each assembled edge was read from.
    edge_origins: HashMap<u32, u32>,
    /// The edges each assembled face's loops use.
    face_edges: HashMap<u32, Vec<u32>>,
    /// Where the body is placed.
    place: Place,
    loss: Loss,
}

fn lift_one(document: &SatDocument, source: &SatBody<'_>, at: Option<u32>, loss: &mut Loss) -> Option<Lifted> {
    // The topology is stored in body space; the body's `transform` record places it, applied
    // as the records are read.
    let place = match resolve(document, source.transform()) {
        None => Place::NONE,
        Some(record) => match body_placement(record).and_then(|matrix| Place::of(&matrix)) {
            Some(place) => place,
            None => {
                note_broken(loss, record);
                return None;
            }
        },
    };
    let mut reader = Reader {
        document,
        assembly: Assembly::new(TOLERANCE, Some(&TRIMS)),
        vertices: HashMap::new(),
        edges: HashMap::new(),
        curves: HashMap::new(),
        edge_origins: HashMap::new(),
        face_edges: HashMap::new(),
        place,
        loss: Loss::default(),
    };
    // Lumps, shells, faces and loops are each a linked list: the record holds the first, and
    // every one holds the next.
    let mut lumps: Vec<Vec<Vec<(u32, bool)>>> = Vec::new();
    let mut lump_pointer = source.lump();
    while let Some(record) = resolve(document, lump_pointer) {
        let Some(lump) = SatLump::from_record(record) else {
            note_broken(&mut reader.loss, record);
            break;
        };
        let mut shells = Vec::new();
        let mut shell_pointer = lump.shell();
        while let Some(record) = resolve(document, shell_pointer) {
            let Some(shell) = SatShell::from_record(record) else {
                note_broken(&mut reader.loss, record);
                break;
            };
            let mut faces = Vec::new();
            let mut face_pointer = shell.face();
            let mut walked = 0;
            while let Some(record) = resolve(document, face_pointer) {
                let Some(face) = SatFace::from_record(record) else {
                    note_broken(&mut reader.loss, record);
                    break;
                };
                if let Some(added) = reader.face(&face, record) {
                    faces.push(added);
                }
                face_pointer = face.next_face();
                walked += 1;
                if walked > document.record_count() {
                    break;
                }
            }
            if !faces.is_empty() {
                shells.push(faces);
            }
            shell_pointer = shell.next_shell();
        }
        if !shells.is_empty() {
            lumps.push(shells);
        }
        lump_pointer = lump.next_lump();
    }
    loss.surfaces.append(&mut reader.loss.surfaces);
    loss.curves.append(&mut reader.loss.curves);
    loss.broken.append(&mut reader.loss.broken);
    loss.refusals.append(&mut reader.loss.refusals);
    if lumps.is_empty() {
        return None;
    }
    let all_closed = lumps
        .iter()
        .all(|shells| shells.iter().all(|faces| reader.closed(faces)));
    let kind = match &lumps[..] {
        [shells] if all_closed && shells.len() == 1 => Kind::Solid,
        [_] if all_closed => Kind::Voids,
        _ => Kind::Model,
    };
    let shells: Vec<Vec<(u32, bool)>> = lumps.into_iter().flatten().collect();
    let item = at.map_or(0, u64::from);
    let Reader { assembly, curves, edge_origins, .. } = reader;
    let solid = match assembly.finish(item, String::new(), shells, kind) {
        Ok(solid) => solid,
        Err(why) => {
            if let Some(index) = at {
                loss.broken.push(index as usize);
            }
            loss.refusals.push(why);
            return None;
        }
    };
    let edge_records = edge_records(&curves, &edge_origins, &solid.brep);
    Some(Lifted {
        brep: solid.brep,
        face_records: solid.face_origin.iter().map(|o| *o as u32).collect(),
        edge_records,
        body_record: at,
    })
}

#[allow(dead_code)]
const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];


/// `world = scale * (p * M) + T`, from the 3x3 row matrix, the translation and the scale at the
/// head of a `transform` record, as Solidcore's affine (rows the images of x, y and z, then
/// the translation). SAB groups the rows and the translation as positions, and some writers
/// put the whole payload in one long string; all three forms are read.
pub(crate) fn body_placement(record: &SatRecord) -> Option<[[f64; 4]; 4]> {
    if record.entity_type != "transform" {
        return None;
    }
    let mut values = Vec::with_capacity(13);
    for token in &record.tokens {
        if values.len() >= 13 {
            break;
        }
        if let Some((components, len)) = token.coordinate_components() {
            values.extend_from_slice(&components[..len]);
        } else if let Some(value) = token.as_float() {
            values.push(value);
        } else if let Some(value) = token.as_integer() {
            values.push(value as f64);
        } else if let Some(text) = token.as_string() {
            values.extend(text.split_ascii_whitespace().map_while(|word| word.parse::<f64>().ok()));
        }
    }
    if values.len() < 13 || !values[..13].iter().all(|value| value.is_finite()) || values[12] <= 0.0 {
        return None;
    }
    let s = values[12];
    let row = |at: usize| [s * values[at], s * values[at + 1], s * values[at + 2], 0.0];
    Some([row(0), row(3), row(6), [values[9], values[10], values[11], 1.0]])
}

/// The SAT edge each edge of the finished record was read from: the assembler renumbers (and
/// may split) edges, so each is matched by its middle to the read edge it lies on.
fn edge_records(curves: &HashMap<u32, (r::Curve, [f64; 2])>, origins: &HashMap<u32, u32>, brep: &r::Brep) -> Vec<Option<u32>> {
    {
        let mut read: Vec<(u32, [f64; 3])> = curves
            .iter()
            .filter_map(|(edge, (curve, interval))| {
                let middle = points_on(curve, *interval, 2)?[1];
                Some((*origins.get(edge)?, middle))
            })
            .collect();
        // Sorted along x, so each edge looks only at those within reach of it there.
        read.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
        brep.edges
            .iter()
            .map(|edge| {
                let middle = points_on(edge.parameters(), edge.interval, 2)?[1];
                let reach = 1e-3 * (1.0 + length(middle));
                let first = read.partition_point(|(_, p)| p[0] < middle[0] - reach);
                read[first..]
                    .iter()
                    .take_while(|(_, p)| p[0] <= middle[0] + reach)
                    .map(|(origin, p)| (*origin, length(sub(*p, middle))))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .filter(|(_, d)| *d <= reach)
                    .map(|(origin, _)| origin)
            })
            .collect()
    }
}

impl Reader<'_> {
    /// Whether every edge the shell's faces use is used twice: a closed shell.
    fn closed(&self, faces: &[(u32, bool)]) -> bool {
        let mut uses: HashMap<u32, usize> = HashMap::new();
        for (face, _) in faces {
            for edge in self.face_edges.get(face).into_iter().flatten() {
                *uses.entry(*edge).or_insert(0) += 1;
            }
        }
        !uses.is_empty() && uses.values().all(|count| *count == 2)
    }

    fn vertex(&mut self, pointer: SatPointer) -> Option<(u32, [f64; 3])> {
        let record = resolve(self.document, pointer)?;
        let index = index_of(record)?;
        let Some(raw) = vertex_point(self.document, record) else {
            note_broken(&mut self.loss, record);
            return None;
        };
        if let Some(found) = self.vertices.get(&index) {
            return Some((*found, raw));
        }
        let added = self.assembly.add_vertex(self.place.point(raw)).ok()?;
        self.vertices.insert(index, added);
        Some((added, raw))
    }

    /// The assembled edge of `coedge`'s edge, and whether it runs the SAT edge's way.
    fn edge(&mut self, coedge: &SatCoedge<'_>) -> Option<(u32, bool)> {
        let record = resolve(self.document, coedge.edge())?;
        let index = index_of(record)?;
        if let Some(found) = self.edges.get(&index) {
            return *found;
        }
        let made = self.make_edge(record, index);
        self.edges.insert(index, made);
        made
    }

    fn make_edge(&mut self, record: &SatRecord, index: u32) -> Option<(u32, bool)> {
        let Some(source) = SatEdge::from_record(record) else {
            note_broken(&mut self.loss, record);
            return None;
        };
        let (start, start_point) = self.vertex(source.start_vertex())?;
        let (end, end_point) = self.vertex(source.end_vertex())?;
        let Some(curve_record) = resolve(self.document, source.curve()) else {
            note_broken(&mut self.loss, record);
            return None;
        };
        let sense_forward = source.sense() == Sense::Forward;
        // In the curve's direction: from the vertex at the low parameter to the one at the high.
        let (from, to, from_point, to_point) = if sense_forward {
            (start, end, start_point, end_point)
        } else {
            (end, start, end_point, start_point)
        };
        let stored = [source.start_param(), source.end_param()];
        let Some((curve, interval)) = read_edge_curve(
            self.document,
            curve_record,
            [from_point, to_point],
            stored,
            sense_forward,
        ) else {
            self.loss.curves.push(index_of(curve_record).unwrap_or(index) as usize);
            return None;
        };
        let Some((curve, interval)) = self.place.curve(curve, interval) else {
            self.loss.curves.push(index_of(curve_record).unwrap_or(index) as usize);
            return None;
        };
        let (added, as_given) = match self.assembly.add_edge(u64::from(index), curve.clone(), interval, [from, to]) {
            Ok(added) => added,
            Err(why) => {
                self.loss.refusals.push(why);
                return None;
            }
        };
        self.curves.insert(added, (curve, interval));
        self.edge_origins.insert(added, index);
        // The edge runs from the vertex at its interval's start; the SAT edge from its start
        // vertex. A closed edge has one vertex: its sense says.
        let at_start = if as_given { from } else { to };
        let along = if start == end { sense_forward } else { at_start == start };
        Some((added, along))
    }

    fn face(&mut self, source: &SatFace<'_>, record: &SatRecord) -> Option<(u32, bool)> {
        let index = index_of(record)?;
        // The loops first: their edges bound a boundary patch when the surface cannot be read,
        // and an elliptical cone's height.
        let mut loops: Vec<Vec<(u32, bool)>> = Vec::new();
        let mut vertex_loops: Vec<u32> = Vec::new();
        let mut loop_pointer = source.first_loop();
        while let Some(loop_record) = resolve(self.document, loop_pointer) {
            let Some(ring) = SatLoop::from_record(loop_record) else {
                note_broken(&mut self.loss, loop_record);
                break;
            };
            let first = ring.first_coedge();
            let mut pointer = first;
            let mut uses = Vec::new();
            let mut lone_vertex = None;
            let mut walked = 0;
            loop {
                let Some(coedge_record) = resolve(self.document, pointer) else {
                    break;
                };
                let Some(coedge) = SatCoedge::from_record(coedge_record) else {
                    note_broken(&mut self.loss, coedge_record);
                    return None;
                };
                let edge = resolve(self.document, coedge.edge()).and_then(SatEdge::from_record);
                match edge {
                    // A degenerate edge without a curve (a cone's apex): a vertex loop.
                    Some(edge) if edge.curve().is_null() => {
                        lone_vertex = self.vertex(edge.start_vertex()).map(|(v, _)| v);
                    }
                    _ => {
                        // A loop with an edge left out does not close: the face is left out.
                        let (edge, along) = self.edge(&coedge)?;
                        uses.push((edge, (coedge.sense() == Sense::Forward) == along));
                    }
                }
                pointer = coedge.next();
                walked += 1;
                if pointer == first || pointer.is_null() || walked > self.document.record_count() {
                    break;
                }
            }
            match (uses.is_empty(), lone_vertex) {
                (false, _) => loops.push(uses),
                (true, Some(vertex)) => vertex_loops.push(vertex),
                (true, None) => {}
            }
            loop_pointer = ring.next_loop();
        }

        let surface_record = resolve(self.document, source.surface())?;
        let (read, flipped) = match read_surface(self.document, surface_record) {
            Some(read) => read,
            None => {
                self.loss.surfaces.push(index_of(surface_record).unwrap_or(index) as usize);
                // Built from the placed edges, so already in the world.
                (Read::Placed(self.boundary_patch(&loops)?), false)
            }
        };
        let place = self.place;
        // A reflection turns a spline's natural normal (its u and v keep their order) but not
        // an analytic surface's, whose frame is made right-handed again.
        let (surface, turned_by_place) = match read {
            Read::Surface(surface) => {
                let spline = matches!(surface, r::Surface::Spline { .. } | r::Surface::RationalSpline { .. });
                (place.surface(surface)?, place.mirror && spline)
            }
            Read::Placed(surface) => (surface, false),
            Read::EllipticCone(cone) => {
                let cone = cone.placed(&place);
                (cone.surface(self.heights(&cone, &loops)?)?, place.mirror)
            }
        };
        // The face's side against the surface's natural normal: its sense, and a surface that
        // faces inward, runs its v the other way, or was reflected.
        let forward = (source.sense() == Sense::Forward) != (flipped != turned_by_place);
        // Loops run counter-clockwise about the face's normal (clockwise once reflected); the
        // assembler takes them about the surface's natural normal.
        let mut bounds: Vec<Bound> = loops
            .iter()
            .map(|uses| {
                let reversed = forward == place.mirror;
                Bound::Edges(if reversed {
                    uses.iter().rev().map(|(edge, along)| (*edge, !along)).collect()
                } else {
                    uses.clone()
                })
            })
            .collect();
        bounds.extend(vertex_loops.into_iter().map(Bound::Vertex));
        let added = match self.assembly.add_face(u64::from(index), surface, bounds, None, forward) {
            Ok(added) => added,
            Err(why) => {
                self.loss.refusals.push(why);
                return None;
            }
        };
        self.face_edges
            .insert(added, loops.iter().flatten().map(|(edge, _)| *edge).collect());
        Some((added, forward))
    }

    /// The points along the edges `loops` use.
    fn loop_points(&self, uses: &[(u32, bool)], count: usize) -> Option<Vec<[f64; 3]>> {
        let mut out: Vec<[f64; 3]> = Vec::new();
        for (edge, along) in uses {
            let (curve, interval) = self.curves.get(edge)?;
            let mut piece = points_on(curve, *interval, count)?;
            if !along {
                piece.reverse();
            }
            if out.is_empty() {
                out.extend(piece);
            } else {
                out.extend(piece.into_iter().skip(1));
            }
        }
        Some(out)
    }

    /// How far along its axis an elliptical cone's face reaches: its edges' heights, a little
    /// past them so meshing never clamps a boundary point.
    fn heights(&self, cone: &EllipticCone, loops: &[Vec<(u32, bool)>]) -> Option<[f64; 2]> {
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for uses in loops {
            for point in self.loop_points(uses, 32)? {
                let height = dot(sub(point, cone.centre), cone.axis);
                low = low.min(height);
                high = high.max(height);
            }
        }
        if !(high >= low) {
            return None;
        }
        let pad = ((high - low) * 0.05).max(1e-6 * (1.0 + low.abs().max(high.abs())));
        Some([low - pad, high + pad])
    }

    /// A stand-in for a surface that cannot be read (a vertex blend saves no fitted spline,
    /// only its boundary): Solidcore's n-sided patch over the face's outer loop, each edge a
    /// curve through its points.
    // ponytail: a patch over the boundary, not the blend's own surface; its cross-boundary
    // tangents differ from the neighbouring blends'. Evaluate the blend itself if seams show.
    fn boundary_patch(&self, loops: &[Vec<(u32, bool)>]) -> Option<r::Surface> {
        let outer = loops.first()?;
        let sides: Vec<r::Curve> = outer
            .iter()
            .map(|(edge, along)| {
                let points = self.loop_points(&[(*edge, *along)], 8)?;
                solidcore_ops::curve_fit::ep_curve(&points, 3).ok().map(|(curve, _)| curve)
            })
            .collect::<Option<_>>()?;
        let (sheet, _) = solidcore_ops::surface::n_sided(&sides).ok()?;
        let face = sheet.faces.first()?;
        Some(face.parameters().clone())
    }
}

fn vertex_point(document: &SatDocument, record: &SatRecord) -> Option<[f64; 3]> {
    let vertex = SatVertex::from_record(record)?;
    let point = SatPoint::from_record(resolve(document, vertex.point())?)?;
    let (x, y, z) = point.position();
    Some([x, y, z])
}

/// An edge's curve as a record curve and the interval the edge covers on it, from the vertex
/// at the low parameter to the one at the high (`ends`, in the curve's direction). The vertices
/// fix where an open piece runs; the stored range (negated on an edge running against its
/// curve, `sense_forward` false) only which turn of a closed one.
fn read_edge_curve(
    document: &SatDocument,
    record: &SatRecord,
    ends: [[f64; 3]; 2],
    stored: [f64; 2],
    sense_forward: bool,
) -> Option<(r::Curve, [f64; 2])> {
    let apart = length(sub(ends[0], ends[1])) > 1e-9;
    let near = if sense_forward {
        stored[0].min(stored[1])
    } else {
        -stored[0].max(stored[1])
    };
    if SatStraightCurve::from_record(record).is_some() {
        // A line edge closed on itself has no length.
        return apart.then(|| (r::Curve::Line { start: ends[0], end: ends[1] }, [0.0, 1.0]));
    }
    if let Some(ellipse) = SatEllipseCurve::from_record(record) {
        let (cx, cy, cz) = ellipse.center();
        let (nx, ny, nz) = ellipse.normal();
        let (mx, my, mz) = ellipse.major_axis();
        let major = [mx, my, mz];
        let radius = length(major);
        let ratio = ellipse.ratio();
        let f = frame([cx, cy, cz], major, [nx, ny, nz])?;
        let radii = [radius, radius * ratio];
        let angle = |p: [f64; 3]| {
            let d = sub(p, f.origin);
            (dot(d, f.y_axis) / radii[1]).atan2(dot(d, f.x_axis) / radii[0])
        };
        let curve = if (ratio - 1.0).abs() < 1e-12 {
            r::Curve::Circle { frame: f.clone(), radius }
        } else {
            r::Curve::Ellipse { frame: f.clone(), radii }
        };
        // A whole circle or ellipse has one vertex, which fixes where the turn starts.
        let low = turn_near(angle(ends[0]), near);
        let mut high = if apart { turn_near(angle(ends[1]), low) } else { low + TAU };
        while high <= low + 1e-12 {
            high += TAU;
        }
        return Some((curve, [low, high]));
    }
    if let Some(spline) = SatIntCurve::from_record(record) {
        let closed = spline.is_closed_in(document);
        let (_, mut knots, mut controls) = spline.bspline_in(document).or_else(|| {
            let (surface, pcurve, offset) = spline.support_in(document)?;
            supported_curve(surface, pcurve, offset)
        })?;
        // A reversed intcurve runs against its spline: c(t) = s(-t), and the edges on it store
        // parameters in that negated range (#1538).
        if record.token_sense(1) == Sense::Reversed {
            controls.reverse();
            knots = knots.into_iter().rev().map(|knot| -knot).collect();
        }
        let curve = spline_curve(knots.clone(), &controls)?;
        let degree = knots.len().checked_sub(controls.len() + 1)?;
        let domain = [knots[degree], knots[knots.len() - degree - 1]];
        // A spline written open whose ends meet is closed all the same.
        let closed = closed || {
            let [first, last] = [controls.first()?, controls.last()?].map(|c| [c[0] / c[3], c[1] / c[3], c[2] / c[3]]);
            let size = controls.iter().map(|c| length([c[0] / c[3], c[1] / c[3], c[2] / c[3]])).fold(1.0, f64::max);
            length(sub(first, last)) <= size * 1e-12
        };
        let period = domain[1] - domain[0];
        let interval = if !apart && closed {
            // A whole closed spline: the vertex fixes where the turn starts, which on a record
            // spline is its domain.
            domain
        } else if apart {
            let a = parameter_on(&curve, ends[0])?;
            let b = parameter_on(&curve, ends[1])?;
            if b > a {
                [a, b]
            } else if closed {
                // A piece over the seam of a closed spline (#1563): the curve through its points.
                let mut points = points_on(&curve, [a, domain[1]], 32)?;
                points.extend(points_on(&curve, [domain[0], b], 32)?.into_iter().skip(1));
                let (fitted, _) = solidcore_ops::curve_fit::ep_curve(&points, 3).ok()?;
                let range = curve_range(&fitted)?;
                return Some((fitted, range));
            } else {
                [b, a]
            }
        } else {
            let _ = period;
            return None;
        };
        return Some((curve, interval));
    }
    None
}

/// A record spline curve from full knots and homogeneous `[x w, y w, z w, w]` controls.
fn spline_curve(knots: Vec<f64>, controls: &[[f64; 4]]) -> Option<r::Curve> {
    if controls.iter().any(|c| !c[3].is_finite() || c[3] <= 0.0) {
        return None;
    }
    Some(if controls.iter().all(|c| (c[3] - 1.0).abs() < 1e-14) {
        r::Curve::Spline {
            knots,
            points: controls.iter().map(|c| [c[0], c[1], c[2]]).collect(),
        }
    } else {
        r::Curve::RationalSpline {
            knots,
            homogeneous: controls.to_vec(),
        }
    })
}

/// The parameter range of a record spline curve.
fn curve_range(curve: &r::Curve) -> Option<[f64; 2]> {
    let (knots, count) = match curve {
        r::Curve::Spline { knots, points } => (knots, points.len()),
        r::Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
        _ => return None,
    };
    let degree = knots.len().checked_sub(count + 1)?;
    Some([knots[degree], knots[knots.len() - degree - 1]])
}

/// A procedural curve saved without its spline, rebuilt from its support surface and pcurve:
/// sampled evenly in the pcurve's parameter and fitted through the samples.
fn supported_curve(
    surface: SatBSplineSurface,
    pcurve: (usize, Vec<f64>, Vec<[f64; 3]>),
    offset: f64,
) -> Option<(usize, Vec<f64>, Vec<[f64; 4]>)> {
    let surface = spline_surface(surface)?;
    let converted = solidcore_convert::surface(&surface).ok()?;
    let eval = solidcore::eval::SurfaceEval::new(&converted).ok()?;
    let (degree, knots, controls) = pcurve;
    let pc = r::Curve2::RationalSpline {
        knots: knots.clone(),
        homogeneous: controls.clone(),
    };
    let (start, end) = (knots[degree], knots[knots.len() - degree - 1]);
    const SAMPLES: usize = 64;
    let samples: Vec<[f64; 3]> = (0..=SAMPLES)
        .map(|i| {
            let uv = solidcore_ops::pcurve::eval(&pc, start + (end - start) * i as f64 / SAMPLES as f64)?;
            let (p, su, sv) = eval.derivs(uv[0], uv[1])?;
            let p = solidcore_convert::record_vec3(p);
            let p = [p.x, p.y, p.z];
            if offset == 0.0 {
                return Some(p);
            }
            let (su, sv) = (solidcore_convert::record_vec3(su), solidcore_convert::record_vec3(sv));
            let normal = unit(cross([su.x, su.y, su.z], [sv.x, sv.y, sv.z]))?;
            Some(add(p, scale(normal, offset)))
        })
        .collect::<Option<_>>()?;
    let (fitted, _) = solidcore_ops::curve_fit::ep_curve(&samples, 3).ok()?;
    let range = curve_range(&fitted)?;
    let (knots, homogeneous) = match fitted {
        r::Curve::Spline { knots, points } => (knots, points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()),
        r::Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous),
        _ => return None,
    };
    // Onto the pcurve's own parameter, so an edge's stored span still selects its piece.
    let knots = knots
        .iter()
        .map(|k| start + (k - range[0]) / (range[1] - range[0]) * (end - start))
        .collect();
    Some((3, knots, homogeneous))
}

/// What a surface record reads as.
enum Read {
    /// A surface in the body's space.
    Surface(r::Surface),
    /// A surface already in the world.
    Placed(r::Surface),
    /// An elliptical cone or cylinder, whose height its faces fix.
    EllipticCone(EllipticCone),
}

/// A surface record as a record surface, and whether a face on it turns against its natural
/// normal (a cone, sphere or torus facing inward, a spline running its v the other way).
fn read_surface(document: &SatDocument, record: &SatRecord) -> Option<(Read, bool)> {
    if let Some(plane) = SatPlaneSurface::from_record(record) {
        let (x, y, z) = plane.root_point();
        let (nx, ny, nz) = plane.normal();
        let (ux, uy, uz) = plane.u_direction();
        let f = frame([x, y, z], [ux, uy, uz], [nx, ny, nz])?;
        return Some((
            Read::Surface(r::Surface::Plane {
                origin: f.origin,
                u_axis: f.x_axis,
                v_axis: f.y_axis,
            }),
            false,
        ));
    }
    if let Some(cone) = SatConeSurface::from_record(record) {
        // A negative cosine has the normal towards the axis (the inner wall of a bend).
        let inward = cone.cos_half_angle() < 0.0;
        if let Some(elliptic) = EllipticCone::from_record(&cone) {
            return Some((Read::EllipticCone(elliptic), inward));
        }
        let (cx, cy, cz) = cone.center();
        let (ax, ay, az) = cone.axis();
        let (mx, my, mz) = cone.major_axis();
        // The radius is the length of the major axis, not the `radius` token: reading the
        // token instead turns a disc into a ring.
        let radius = length([mx, my, mz]);
        let f = frame([cx, cy, cz], [mx, my, mz], [ax, ay, az])?;
        let (sine, cosine) = (cone.sin_half_angle(), cone.cos_half_angle());
        let (sine, cosine) = if cosine < 0.0 { (-sine, -cosine) } else { (sine, cosine) };
        let surface = if sine.abs() < 1e-12 {
            r::Surface::Cylinder { frame: f, radius }
        } else {
            r::Surface::Cone {
                frame: f,
                radius_at_origin: radius,
                half_angle: sine.atan2(cosine),
            }
        };
        return Some((Read::Surface(surface), inward));
    }
    if let Some(sphere) = SatSphereSurface::from_record(record) {
        let (cx, cy, cz) = sphere.center();
        let (ux, uy, uz) = sphere.u_direction();
        let (px, py, pz) = sphere.pole();
        return Some((
            Read::Surface(r::Surface::Sphere {
                frame: frame([cx, cy, cz], [ux, uy, uz], [px, py, pz])?,
                radius: sphere.radius().abs(),
            }),
            sphere.radius() < 0.0,
        ));
    }
    if let Some(torus) = SatTorusSurface::from_record(record) {
        let (cx, cy, cz) = torus.center();
        let (nx, ny, nz) = torus.normal();
        let (ux, uy, uz) = torus.u_direction();
        return Some((
            Read::Surface(r::Surface::Torus {
                frame: frame([cx, cy, cz], [ux, uy, uz], [nx, ny, nz])?,
                major_radius: torus.major_radius(),
                minor_radius: torus.minor_radius().abs(),
                tube_start: 0.0,
            }),
            torus.minor_radius() < 0.0,
        ));
    }
    if let Some(spline) = SatSplineSurface::from_record(record) {
        let reversed = spline.sense() == Sense::Reversed;
        // An offset's saved spline is its approximation; one saved without it is not read.
        let surface = spline_surface(spline.bspline(document)?)?;
        return Some((Read::Surface(surface), reversed));
    }
    None
}

/// A SAT B-spline surface as a record spline surface (`points[i][j]`, i along u).
fn spline_surface(spline: SatBSplineSurface) -> Option<r::Surface> {
    let (nu, nv) = (spline.control_count_u, spline.control_count_v);
    let at = |u: usize, v: usize| spline.control_points.get(v * nu + u).copied();
    let mut rational = false;
    let mut homogeneous = vec![vec![[0.0; 4]; nv]; nu];
    for (u, row) in homogeneous.iter_mut().enumerate() {
        for (v, slot) in row.iter_mut().enumerate() {
            let c = at(u, v)?;
            if !c[3].is_finite() || c[3] <= 0.0 {
                return None;
            }
            rational |= (c[3] - 1.0).abs() > 1e-14;
            *slot = c;
        }
    }
    Some(if rational {
        r::Surface::RationalSpline {
            u_knots: spline.u_knots,
            v_knots: spline.v_knots,
            homogeneous,
        }
    } else {
        r::Surface::Spline {
            u_knots: spline.u_knots,
            v_knots: spline.v_knots,
            points: homogeneous
                .into_iter()
                .map(|row| row.into_iter().map(|c| [c[0], c[1], c[2]]).collect())
                .collect(),
        }
    })
}

/// A cone or cylinder whose cross-section is an ellipse, which the record's analytic cones are
/// not: the exact rational surface, a full rational quadratic ellipse swept linearly along the
/// axis, scaling as it goes.
struct EllipticCone {
    centre: [f64; 3],
    major: [f64; 3],
    minor: [f64; 3],
    axis: [f64; 3],
    /// How the section scales per unit of height (zero for a cylinder).
    shrink: f64,
}

impl EllipticCone {
    fn from_record(cone: &SatConeSurface<'_>) -> Option<Self> {
        let ratio = cone.ratio();
        if !ratio.is_finite() || ratio <= 0.0 || (ratio - 1.0).abs() <= 1e-12 {
            return None;
        }
        let (cx, cy, cz) = cone.center();
        let (ax, ay, az) = cone.axis();
        let (mx, my, mz) = cone.major_axis();
        let radius = length([mx, my, mz]);
        let f = frame([cx, cy, cz], [mx, my, mz], [ax, ay, az])?;
        let (sine, cosine) = (cone.sin_half_angle(), cone.cos_half_angle());
        let (sine, cosine) = if cosine < 0.0 { (-sine, -cosine) } else { (sine, cosine) };
        let tangent = sine / cosine;
        (radius > 0.0 && tangent.is_finite()).then(|| Self {
            centre: [cx, cy, cz],
            major: scale(f.x_axis, radius),
            minor: scale(f.y_axis, radius * ratio),
            axis: cross(f.x_axis, f.y_axis),
            shrink: -tangent / radius,
        })
    }

    /// This cone moved by `place`.
    fn placed(&self, place: &Place) -> EllipticCone {
        EllipticCone {
            centre: place.point(self.centre),
            major: place.vector(self.major),
            minor: place.vector(self.minor),
            axis: unit(place.vector(self.axis)).unwrap_or(self.axis),
            shrink: self.shrink / place.scale,
        }
    }

    /// The surface between heights `low` and `high`: u counter-clockwise about the axis and v
    /// up it, so the natural normal faces away from the axis.
    fn surface(&self, [low, high]: [f64; 2]) -> Option<r::Surface> {
        let section: [(f64, f64, f64); 9] = [
            (1.0, 0.0, 1.0),
            (1.0, 1.0, FRAC_1_SQRT_2),
            (0.0, 1.0, 1.0),
            (-1.0, 1.0, FRAC_1_SQRT_2),
            (-1.0, 0.0, 1.0),
            (-1.0, -1.0, FRAC_1_SQRT_2),
            (0.0, -1.0, 1.0),
            (1.0, -1.0, FRAC_1_SQRT_2),
            (1.0, 0.0, 1.0),
        ];
        let at = |x: f64, y: f64, height: f64, w: f64| {
            let s = 1.0 - height * self.shrink;
            let across = scale(add(scale(self.major, x), scale(self.minor, y)), s);
            let p = add(add(self.centre, across), scale(self.axis, height));
            [p[0] * w, p[1] * w, p[2] * w, w]
        };
        let homogeneous = section
            .iter()
            .map(|&(x, y, w)| vec![at(x, y, low, w), at(x, y, high, w)])
            .collect();
        (high > low).then(|| r::Surface::RationalSpline {
            u_knots: vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0],
            v_knots: vec![low, low, high, high],
            homogeneous,
        })
    }
}

fn resolve(document: &SatDocument, pointer: SatPointer) -> Option<&SatRecord> {
    (!pointer.is_null()).then(|| document.resolve(pointer)).flatten()
}

fn index_of(record: &SatRecord) -> Option<u32> {
    u32::try_from(record.index).ok()
}

fn note_broken(loss: &mut Loss, record: &SatRecord) {
    if let Some(index) = index_of(record) {
        loss.broken.push(index as usize);
    }
}

/// One span of a wire body, in the order the wire runs.
#[derive(Debug, Clone)]
pub struct WireSpan {
    /// The edge's curve, a record curve.
    pub curve: r::Curve,
    /// The span's interval on the curve.
    pub interval: [f64; 2],
    /// Where the span starts along the wire.
    pub start: [f64; 3],
    /// Where the span ends along the wire.
    pub end: [f64; 3],
    /// Whether the wire runs along the curve's own direction.
    pub along_curve: bool,
}

/// The spans of the first wire in a wire body (a polyline the modeler keeps as a body), from
/// its open end, or from the coedge the wire names when it is closed. The body's transform, if
/// any, is applied.
pub fn lift_wire(document: &SatDocument) -> Option<Vec<WireSpan>> {
    let wire = document.wires().into_iter().next()?;
    let first = wire
        .record()
        .pointers()
        .into_iter()
        .find(|pointer| resolve(document, *pointer).is_some_and(|record| record.is_a("coedge")))?;
    let index = |pointer| resolve(document, pointer).and_then(index_of);
    // An open wire's first coedge is its own predecessor; a closed one is a ring, walked from
    // the coedge the wire names.
    let mut start = first;
    let mut visited = std::collections::HashSet::new();
    loop {
        visited.insert(index(start)?);
        let previous = SatCoedge::from_record(resolve(document, start)?)?.prev();
        match index(previous) {
            Some(at) if at != index(start)? && !visited.contains(&at) => start = previous,
            Some(at) if at != index(start)? => {
                start = first;
                break;
            }
            _ => break,
        }
    }
    let placement = document
        .bodies()
        .first()
        .and_then(|body| resolve(document, body.transform()))
        .and_then(body_placement);
    let place = |p: [f64; 3]| match placement {
        Some(m) => [0, 1, 2].map(|k| p[0] * m[0][k] + p[1] * m[1][k] + p[2] * m[2][k] + m[3][k]),
        None => p,
    };
    let mut spans = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pointer = start;
    while let Some(record) = resolve(document, pointer) {
        if !seen.insert(index_of(record)?) {
            break;
        }
        let coedge = SatCoedge::from_record(record)?;
        let edge = SatEdge::from_record(resolve(document, coedge.edge())?)?;
        let point = |pointer| -> Option<[f64; 3]> { vertex_point(document, resolve(document, pointer)?) };
        // An edge runs from its start vertex to its end vertex and a reversed coedge runs it
        // backwards; either sense turns it against the curve.
        let coedge_forward = coedge.sense() == Sense::Forward;
        let edge_forward = edge.sense() == Sense::Forward;
        let (from, to) = (place(point(edge.start_vertex())?), place(point(edge.end_vertex())?));
        let in_curve_direction = if edge_forward { [from, to] } else { [to, from] };
        let (curve, interval) = read_edge_curve(
            document,
            resolve(document, edge.curve())?,
            in_curve_direction,
            [edge.start_param(), edge.end_param()],
            edge_forward,
        )?;
        let curve = match placement {
            Some(m) if !matches!(curve, r::Curve::Line { .. }) => {
                solidcore_ops::curve::transform(&curve, &m).ok()?
            }
            _ => curve,
        };
        spans.push(WireSpan {
            curve,
            interval,
            start: if coedge_forward { from } else { to },
            end: if coedge_forward { to } else { from },
            along_curve: edge_forward == coedge_forward,
        });
        pointer = coedge.next();
    }
    (!spans.is_empty()).then_some(spans)
}
