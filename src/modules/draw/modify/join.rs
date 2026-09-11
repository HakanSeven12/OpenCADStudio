// JOIN command — stitch Lines and Arcs that touch end-to-end into one
// entity. Segments join wherever their endpoints meet; the angle between
// them is irrelevant (a broken polyline rejoins fine).
//
// Result:
//   collinear straight run → single Line
//   planar chain           → LwPolyline (arcs carried as bulges)
//   chain with varying Z   → Polyline3D (straight segments only)
//
// Workflow: select objects then press Enter to join.

use acadrust::types::{Vector2, Vector3};
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};

// ── Command ────────────────────────────────────────────────────────────────

pub struct JoinCommand {
    source: Option<(Handle, EntityType)>,
    picked: Option<EntityType>,
    handles: Vec<Handle>,
}
impl JoinCommand {
    pub fn new() -> Self { Self { source: None, picked: None, handles: Vec::new() } }
    pub fn with_source(mut self, handle: Handle, entity: EntityType) -> Self {
        self.picked = Some(entity); self.on_entity_pick(handle, DVec3::ZERO); self
    }
}
impl CadCommand for JoinCommand {
    fn name(&self) -> &'static str { "JOIN" }
    fn prompt(&self) -> String {
        match self.source.as_ref().map(|(_, entity)| entity) {
            None => "JOIN  Select source object:".into(),
            Some(EntityType::Line(_)) => "JOIN  Select lines to join to source (Enter to apply):".into(),
            Some(EntityType::Arc(_)) => "JOIN  Select arcs to join to source or [cLose] (Enter to apply):".into(),
            Some(EntityType::Spline(_)) => "JOIN  Select open curves to join to source (Enter to apply):".into(),
            _ => "JOIN  Select objects to join to source (Enter to apply):".into(),
        }
    }
    fn options(&self) -> Vec<crate::command::CmdOption> {
        if matches!(&self.source,Some((_,EntityType::Arc(_)))) { vec![crate::command::CmdOption::new("Close","L")] } else {Vec::new()}
    }
    fn on_text_input(&mut self,text:&str) -> Option<CmdResult> {
        if matches!(text.trim().to_ascii_uppercase().as_str(),"L"|"CLOSE") {
            if let Some((handle,EntityType::Arc(arc)))=&self.source {
                let mut circle=acadrust::entities::Circle::new();
                circle.common=arc.common.clone();circle.common.handle=Handle::NULL;
                circle.center=arc.center.clone();circle.normal=arc.normal.clone();circle.radius=arc.radius;circle.thickness=arc.thickness;
                return Some(CmdResult::ReplaceMany(vec![(*handle,vec![EntityType::Circle(circle)])],Vec::new()));
            }
        }
        None
    }
    fn needs_entity_pick(&self) -> bool { self.source.is_none() }
    fn inject_before_entity_pick(&self) -> bool { true }
    fn inject_picked_entity(&mut self, entity: EntityType) { self.picked = Some(entity); }
    fn on_entity_pick(&mut self, handle: Handle, _: DVec3) -> CmdResult {
        if handle.is_null() { return CmdResult::NeedPoint; }
        if let Some(entity) = self.picked.take() {
            let supported = match &entity {
                EntityType::Line(_) | EntityType::Arc(_) => true,
                EntityType::LwPolyline(p) => !p.is_closed,
                EntityType::Polyline2D(p) => !p.is_closed(),
                EntityType::Spline(p) => !p.flags.closed && !p.flags.periodic,
                _ => false,
            };
            if supported { self.source = Some((handle, entity)); }
        }
        CmdResult::NeedPoint
    }
    fn is_selection_gathering(&self) -> bool { self.source.is_some() }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles.into_iter().filter(|handle| self.source.as_ref().is_none_or(|(source,_)| handle != source)).collect(); CmdResult::NeedPoint
    }
    fn on_point(&mut self, _: DVec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult {
        let Some((source, _)) = self.source.as_ref() else { return CmdResult::Cancel; };
        if self.handles.is_empty() { return CmdResult::Cancel; }
        CmdResult::JoinToSource { source: *source, handles: self.handles.clone() }
    }
}

/// Join compatible candidates while preserving the source identity and style.
pub fn join_to_source(source: &EntityType, candidates: &[(Handle, &EntityType)]) -> Option<(EntityType, Vec<Handle>)> {
    let mut result = source.clone(); let mut consumed = Vec::new();
    loop {
        let mut progress = false;
        for (handle, candidate) in candidates {
            if consumed.contains(handle) { continue; }
            let next = match (&result, *candidate) {
                (EntityType::Line(a), EntityType::Line(b)) => {
                    let point = |p: &Vector3| [p.x,p.y,p.z];
                    cadkernel::space::source_join::join_collinear_lines([point(&a.start),point(&a.end)],[point(&b.start),point(&b.end)],JOIN_EPS).map(|span| {
                        let mut line = a.clone(); line.start = Vector3::new(span[0][0],span[0][1],span[0][2]); line.end = Vector3::new(span[1][0],span[1][1],span[1][2]); EntityType::Line(line)
                    })
                }
                (EntityType::Arc(a), EntityType::Arc(b)) => {
                    let data = |a: &acadrust::entities::Arc| ([a.center.x,a.center.y,a.center.z],[a.normal.x,a.normal.y,a.normal.z],a.radius,[a.start_angle,a.end_angle]);
                    cadkernel::space::source_join::join_cocircular_arcs(data(a),data(b),JOIN_EPS).map(|span| {
                        if span[1]-span[0] >= std::f64::consts::TAU {
                            let mut circle = acadrust::entities::Circle::new(); circle.common = a.common.clone(); circle.center = a.center.clone(); circle.normal = a.normal.clone(); circle.radius = a.radius; circle.thickness = a.thickness; EntityType::Circle(circle)
                        } else { let mut arc = a.clone(); arc.start_angle = span[0]; arc.end_angle = span[1].rem_euclid(std::f64::consts::TAU); EntityType::Arc(arc) }
                    })
                }
                (EntityType::Spline(source), candidate) => {
                    let curve = |s:&acadrust::entities::Spline| {
                        if s.flags.closed || s.flags.periodic {return None;}
                        if s.control_points.is_empty() && s.fit_points.len() >= 2 {
                            use cadkernel::space::{NurbsCurve3, Parameterization};
                            let points: Vec<_> = s.fit_points.iter().map(|point| [point.x,point.y,point.z]).collect();
                            let parameterization = match s.knot_parameterization {
                                1 => Parameterization::Centripetal, 2 => Parameterization::Uniform, _ => Parameterization::Chord,
                            };
                            let tangent = |point: &Vector3| (point.x != 0.0 || point.y != 0.0 || point.z != 0.0)
                                .then_some([point.x,point.y,point.z]);
                            return NurbsCurve3::interpolate_fit(&points,tangent(&s.begin_tangent),tangent(&s.end_tangent),parameterization)?
                                .compact_knots(s.control_tolerance.max(1e-9));
                        }
                        let weights=if s.weights.is_empty(){vec![1.0;s.control_points.len()]}else{s.weights.clone()};
                        cadkernel::space::NurbsCurve3::new_strict(s.degree as usize,s.control_points.iter().map(|p|[p.x,p.y,p.z]).collect(),s.knots.clone(),weights)
                    };
                    curve(source).and_then(|a| {
                        let b=match candidate {
                            EntityType::Spline(s)=>curve(s),
                            EntityType::Line(line)=>cadkernel::space::source_join::line_as_nurbs([[line.start.x,line.start.y,line.start.z],[line.end.x,line.end.y,line.end.z]],a.degree()),
                            _=>None,
                        }?;
                        let joined=cadkernel::space::source_join::join_nurbs_curves(&a,&b,JOIN_EPS)?;
                        let mut spline=source.clone();
                        spline.degree=joined.degree() as i32;
                        spline.control_points=joined.control_points().iter().map(|p|Vector3::new(p[0],p[1],p[2])).collect();
                        spline.knots=joined.knots().to_vec();spline.weights=joined.weights().to_vec();
                        spline.fit_points.clear();spline.flags.rational=joined.is_rational();
                        spline.dwg_flags1 &= !1;spline.dxf_flags &= !32;
                        spline.flags.planar=cadkernel::space::are_coplanar(joined.control_points(),&[]);
                        spline.begin_tangent=Vector3::new(0.0,0.0,0.0);spline.end_tangent=Vector3::new(0.0,0.0,0.0);
                        Some(EntityType::Spline(spline))
                    })
                }
                (EntityType::LwPolyline(_) | EntityType::Polyline2D(_), _) => {
                    join_entities(&[(result.common().handle,&result),(*handle,*candidate)]).and_then(|(_,mut entities)| {
                        let entity = entities.pop()?;
                        if matches!(&entity,EntityType::Line(_)) { super::pedit::convert_to_polyline(&entity) } else { Some(entity) }
                    })
                }
                _ => None,
            };
            if let Some(mut entity) = next {
                *entity.common_mut() = source.common().clone(); result = entity; consumed.push(*handle); progress = true;
            }
        }
        if !progress { break; }
    }
    (!consumed.is_empty()).then_some((result,consumed))
}

/// Endpoint-match tolerance (model units). Segments split from a shared
/// vertex meet exactly, so this only absorbs float noise.
const JOIN_EPS: f64 = 1e-6;

/// One directed segment of the join chain. `bulge` is the LwPolyline bulge
/// for the arc from `a` to `b` (0 for a straight line); it is only
/// meaningful when the whole chain turns out planar in XY.
#[derive(Clone)]
struct Seg {
    a: DVec3,
    b: DVec3,
    bulge: f64,
    widths: Option<(f64, f64)>,
}

impl Seg {
    fn flip(&mut self) {
        std::mem::swap(&mut self.a, &mut self.b);
        self.bulge = -self.bulge;
        self.widths = self.widths.map(|(start,end)| (end,start));
    }
}

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

fn extrusion(entity: &EntityType) -> Option<(f64, DVec3)> {
    let mut thickness = crate::scene::view::dispatch::entity_thickness(entity)?;
    let normal = match entity {
        EntityType::Arc(entity) => &entity.normal,
        EntityType::Line(entity) => &entity.normal,
        EntityType::LwPolyline(entity) => &entity.normal,
        EntityType::Polyline2D(entity) => &entity.normal,
        _ => return None,
    };
    let normal = DVec3::new(normal.x, normal.y, normal.z);
    if thickness == 0.0 {
        return Some((0.0, normal.try_normalize().unwrap_or(DVec3::Z)));
    }
    let length = normal.length();
    let normal = normal.try_normalize()?;
    thickness *= length;
    (thickness.is_finite() && normal.is_finite()).then_some((thickness, normal))
}

/// Build the chain segments for one entity, or `None` for an entity type
/// JOIN can't carry (which aborts the whole join). Lines and arcs contribute
/// one segment; an OPEN polyline contributes one per span (bulges kept), so
/// polylines merge with their neighbours too — the PEDIT Join set always
/// contains the target polyline (#263). A closed polyline can't be joined.
fn segs_of(e: &EntityType) -> Option<Vec<Seg>> {
    match e {
        EntityType::Line(l) => Some(vec![Seg {
            a: DVec3::new(l.start.x, l.start.y, l.start.z),
            b: DVec3::new(l.end.x, l.end.y, l.end.z),
            bulge: 0.0,
            widths: None,
        }]),
        EntityType::Arc(arc) => {
            // The bulge below assumes the arc lies in a +Z plane; a tilted
            // or flipped normal would invert the CCW sweep, so reject it.
            if arc.normal.x.abs() > 1e-6 || arc.normal.y.abs() > 1e-6 || arc.normal.z <= 0.0 {
                return None;
            }
            let (cx, cy, cz) = (arc.center.x, arc.center.y, arc.center.z);
            let r = arc.radius;
            let (sa, ea) = (arc.start_angle, arc.end_angle);
            let swept = (ea - sa).rem_euclid(std::f64::consts::TAU);
            Some(vec![Seg {
                a: DVec3::new(cx + r * sa.cos(), cy + r * sa.sin(), cz),
                b: DVec3::new(cx + r * ea.cos(), cy + r * ea.sin(), cz),
                bulge: (swept / 4.0).tan(),
                widths: None,
            }])
        }
        EntityType::LwPolyline(p) => {
            let p = crate::entities::curve::lwpolyline_world_xy(p)?;
            if p.is_closed || p.vertices.len() < 2 {
                return None;
            }
            let z = p.elevation;
            Some(
                p.vertices
                    .windows(2)
                    .map(|w| Seg {
                        a: DVec3::new(w[0].location.x, w[0].location.y, z),
                        b: DVec3::new(w[1].location.x, w[1].location.y, z),
                        bulge: w[0].bulge,
                        widths: Some(if p.constant_width != 0.0 { (p.constant_width,p.constant_width) } else { (w[0].start_width,w[0].end_width) }),
                    })
                    .collect(),
            )
        }
        EntityType::Polyline2D(p) => {
            if p.is_closed() || p.vertices.len() < 2 {
                return None;
            }
            let normal = DVec3::new(p.normal.x, p.normal.y, p.normal.z).try_normalize()?;
            if normal.x.abs() > 1e-12 || normal.y.abs() > 1e-12 {
                return None;
            }
            let cadkernel::geom2d::Curve::Polyline(curve) =
                crate::entities::curve::entity_curve_xy(&EntityType::Polyline2D(p.clone()))?
            else {
                return None;
            };
            let z = crate::entities::curve::ocs_plane(p.normal.clone(), p.elevation).origin[2];
            Some(
                curve
                    .vertices
                    .windows(2)
                    .enumerate()
                    .map(|(index,w)| Seg {
                        a: DVec3::new(w[0].position[0], w[0].position[1], z),
                        b: DVec3::new(w[1].position[0], w[1].position[1], z),
                        bulge: w[0].bulge,
                        widths: Some((if p.vertices[index].start_width == 0.0 {p.start_width} else {p.vertices[index].start_width}, if p.vertices[index].end_width == 0.0 {p.end_width} else {p.vertices[index].end_width})),
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

/// Join all `entities` end-to-end into a single entity. Segments join
/// wherever their endpoints touch — the angle between them is irrelevant.
/// A collinear straight run collapses to one `Line`; a planar chain
/// becomes an `LwPolyline` (arcs kept as bulges); a chain with varying Z
/// becomes a `Polyline3D` (straight segments only). Returns
/// `(removed_handles, new_entities)`, or `None` when the selection isn't a
/// single connected chain or holds an unsupported entity.
pub fn join_entities(entities: &[(Handle, &EntityType)]) -> Option<(Vec<Handle>, Vec<EntityType>)> {
    if entities.len() < 2 {
        return None;
    }

    let mut segs = Vec::with_capacity(entities.len());
    for (_, e) in entities {
        segs.extend(segs_of(e)?);
    }
    let handles: Vec<Handle> = entities.iter().map(|(h, _)| *h).collect();
    let common = entities[0].1.common().clone();
    let (thickness, normal) = extrusion(entities[0].1)?;

    let (mut chain, closed) = stitch(segs)?;
    // Newly appended line/arc spans inherit the adjoining polyline width.
    let mut width = chain.iter().find_map(|segment| segment.widths.map(|pair| pair.0)).unwrap_or(0.0);
    for segment in &mut chain {
        if let Some((_,end)) = segment.widths { width = end; }
        else { segment.widths = Some((width,width)); }
    }

    // Ordered vertices, each tagged with the bulge of the segment that
    // starts there. A closed chain reuses the first vertex as the wrap
    // point, so it gets exactly one vertex per segment.
    let mut verts: Vec<(DVec3, f64)> = chain.iter().map(|s| (s.a, s.bulge)).collect();
    if !closed {
        verts.push((chain.last().unwrap().b, 0.0));
    }

    let has_arc = chain.iter().any(|s| s.bulge.abs() > 1e-12);
    let z0 = verts[0].0.z;
    let planar = verts.iter().all(|(p, _)| (p.z - z0).abs() <= JOIN_EPS);

    // An open run of collinear straight segments collapses back to one Line.
    if !closed && !has_arc && chain.iter().all(|segment| segment.widths == Some((0.0,0.0))) && !matches!(entities[0].1, EntityType::LwPolyline(_) | EntityType::Polyline2D(_)) && is_collinear(&verts) {
        let mut line = acadrust::entities::Line::new();
        line.common = common;
        line.common.handle = Handle::NULL;
        line.start = v3(verts.first().unwrap().0);
        line.end = v3(verts.last().unwrap().0);
        line.normal = v3(normal);
        line.thickness = thickness;
        return Some((handles, vec![EntityType::Line(line)]));
    }

    if planar {
        if thickness != 0.0 && normal.x.hypot(normal.y) > 1e-9 {
            return None;
        }
        let flipped = thickness != 0.0 && normal.z < 0.0;
        let lw_verts: Vec<acadrust::entities::LwVertex> = verts
            .iter().enumerate()
            .map(|(index,(p, bulge))| {
                let mut v = acadrust::entities::LwVertex::new(Vector2::new(
                    if flipped { -p.x } else { p.x },
                    p.y,
                ));
                v.bulge = if flipped { -*bulge } else { *bulge };
                let (start,end) = chain.get(index).and_then(|segment| segment.widths)
                    .unwrap_or_else(|| {let width=chain.last().and_then(|segment| segment.widths).map_or(0.0,|pair|pair.1);(width,width)});
                v.start_width = start; v.end_width = end;
                v
            })
            .collect();
        let mut pl = acadrust::entities::LwPolyline::new();
        pl.common = common;
        pl.common.handle = Handle::NULL;
        pl.vertices = lw_verts;
        pl.is_closed = closed;
        pl.elevation = if flipped { -z0 } else { z0 };
        pl.thickness = thickness;
        if flipped {
            pl.normal = Vector3::new(0.0, 0.0, -1.0);
        }
        if let EntityType::Polyline2D(source) = entities[0].1 {
            let mut polyline = source.clone();
            let input = crate::entities::curve::ocs_plane(pl.normal.clone(),pl.elevation);
            let output = crate::entities::curve::ocs_plane(source.normal.clone(),source.elevation);
            polyline.vertices = pl.vertices.iter().map(|vertex| {
                let point = output.project(input.point_at([vertex.location.x,vertex.location.y]))?;
                let mut result = acadrust::entities::polyline::Vertex2D::new(Vector3::new(point[0],point[1],source.elevation));
                result.bulge = if cadkernel::space::Vec3::from(input.normal()?).dot(cadkernel::space::Vec3::from(output.normal()?)) < 0.0 {-vertex.bulge} else {vertex.bulge}; result.start_width = vertex.start_width; result.end_width = vertex.end_width;
                Some(result)
            }).collect::<Option<Vec<_>>>()?;
            polyline.start_width = 0.0; polyline.end_width = 0.0;
            polyline.flags.set_closed(pl.is_closed);
            return Some((handles,vec![EntityType::Polyline2D(polyline)]));
        }
        if let EntityType::LwPolyline(source) = entities[0].1 {
            pl.plinegen = source.plinegen;
            if source.constant_width != 0.0 && pl.vertices.iter().all(|vertex| vertex.start_width == source.constant_width && vertex.end_width == source.constant_width) {
                pl.constant_width = source.constant_width;
            }
        }
        return Some((handles, vec![EntityType::LwPolyline(pl)]));
    }

    // Non-planar: a 3D polyline carries no bulge, so a curved segment can't
    // be represented — refuse rather than silently flatten it.
    if has_arc {
        return None;
    }
    if thickness != 0.0 {
        return None;
    }
    let mut pl = acadrust::entities::Polyline3D::new();
    pl.common = common;
    pl.common.handle = Handle::NULL;
    pl.vertices = verts
        .iter()
        .map(|(p, _)| acadrust::entities::Vertex3DPolyline::new(v3(*p)))
        .collect();
    if closed {
        pl.close();
    }
    Some((handles, vec![EntityType::Polyline3D(pl)]))
}

/// Stitch directed segments into a single chain by matching endpoints,
/// flipping each segment so the chain runs head-to-tail. Returns the
/// ordered chain and whether it closes on itself, or `None` when the
/// segments don't form one connected path (a gap or a branch).
fn stitch(mut segs: Vec<Seg>) -> Option<(Vec<Seg>, bool)> {
    let mut chain = vec![segs.remove(0)];

    // Grow off the tail.
    loop {
        let end = chain.last().unwrap().b;
        let Some(idx) = segs
            .iter()
            .position(|s| s.a.distance(end) <= JOIN_EPS || s.b.distance(end) <= JOIN_EPS)
        else {
            break;
        };
        let mut s = segs.remove(idx);
        if s.a.distance(end) > JOIN_EPS {
            s.flip();
        }
        chain.push(s);
    }

    // Grow off the head.
    loop {
        let start = chain.first().unwrap().a;
        let Some(idx) = segs
            .iter()
            .position(|s| s.a.distance(start) <= JOIN_EPS || s.b.distance(start) <= JOIN_EPS)
        else {
            break;
        };
        let mut s = segs.remove(idx);
        if s.b.distance(start) > JOIN_EPS {
            s.flip();
        }
        chain.insert(0, s);
    }

    if !segs.is_empty() {
        return None; // disconnected or branched selection
    }
    let closed = chain.len() >= 2
        && chain.first().unwrap().a.distance(chain.last().unwrap().b) <= JOIN_EPS;
    Some((chain, closed))
}

#[cfg(test)]
mod join_tests {
    use super::*;
    use acadrust::entities::{Line as LineEnt, LwVertex};
    use acadrust::Handle;

    fn line(x0: f64, y0: f64, x1: f64, y1: f64, thickness: f64) -> EntityType {
        let mut l = LineEnt::new();
        l.start = Vector3::new(x0, y0, 0.0);
        l.end = Vector3::new(x1, y1, 0.0);
        l.thickness = thickness;
        EntityType::Line(l)
    }

    fn lw_2pts(p0: (f64, f64), p1: (f64, f64), thickness: f64) -> EntityType {
        let mut pl = acadrust::entities::LwPolyline::new();
        pl.vertices = vec![
            LwVertex::new(Vector2::new(p0.0, p0.1)),
            LwVertex::new(Vector2::new(p1.0, p1.1)),
        ];
        pl.thickness = thickness;
        EntityType::LwPolyline(pl)
    }

    // JOIN rebuilds the result entity; thickness must follow the chain's first
    // entity (the same source `common` comes from), not reset to 0 (#916).
    #[test]
    fn join_keeps_source_thickness() {
        let h1 = Handle::new(1);
        let h2 = Handle::new(2);
        let mut e1 = lw_2pts((0.0, 0.0), (10.0, 0.0), 2.5);
        let EntityType::LwPolyline(source) = &mut e1 else {
            unreachable!();
        };
        source.normal = Vector3::new(0.0, 0.0, -1.0);
        // The -Z normal maps the source endpoint to world X=-10.
        let e2 = line(-10.0, 0.0, -10.0, 10.0, 0.0);
        let (removed, out) = join_entities(&[(h1, &e1), (h2, &e2)]).expect("chain joins");
        assert_eq!(removed.len(), 2);
        let Some(EntityType::LwPolyline(pl)) = out.first() else {
            panic!("expected joined lwpolyline");
        };
        assert!(
            (pl.thickness - 2.5).abs() < 1e-12,
            "joined polyline must keep source thickness, got {}",
            pl.thickness
        );
        assert_eq!(pl.normal, Vector3::new(0.0, 0.0, -1.0));
    }

    // A collinear straight run collapses to a single Line; its thickness must
    // come from the first source entity too, not default to 0.
    #[test]
    fn collinear_join_keeps_thickness() {
        let h1 = Handle::new(1);
        let h2 = Handle::new(2);
        let e1 = line(0.0, 0.0, 5.0, 0.0, 1.25);
        let e2 = line(5.0, 0.0, 10.0, 0.0, 0.0);
        let (_, out) = join_entities(&[(h1, &e1), (h2, &e2)]).expect("chain joins");
        let Some(EntityType::Line(l)) = out.first() else {
            panic!("expected collapsed line");
        };
        assert!(
            (l.thickness - 1.25).abs() < 1e-12,
            "collapsed line must keep source thickness, got {}",
            l.thickness
        );
    }
}

/// True when every vertex lies on one straight line (within tolerance).
fn is_collinear(verts: &[(DVec3, f64)]) -> bool {
    if verts.len() < 3 {
        return true;
    }
    let dir = verts[1].0 - verts[0].0;
    if dir.length() < JOIN_EPS {
        return false;
    }
    let dir = dir.normalize();
    verts
        .windows(2)
        .all(|w| (w[1].0 - w[0].0).cross(dir).length() <= 1e-6)
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["JOIN"] });  // JoinCommand
