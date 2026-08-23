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
use crate::t;

use crate::command::{CadCommand, CmdResult};

// ── Command ────────────────────────────────────────────────────────────────

pub struct JoinCommand {
    handles: Vec<Handle>,
    gathering: bool,
}

impl JoinCommand {
    pub fn new() -> Self {
        Self {
            handles: vec![],
            gathering: true,
        }
    }
}

impl CadCommand for JoinCommand {
    fn name(&self) -> &'static str {
        "JOIN"
    }

    fn prompt(&self) -> String {
        t!(
            "JOIN  Select objects to join (%{count} selected, Enter to apply):",
            count = self.handles.len()
        )
        .into_owned()
    }

    fn is_selection_gathering(&self) -> bool {
        self.gathering
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.handles.len() < 2 {
            return CmdResult::Cancel;
        }
        self.gathering = false;
        CmdResult::JoinEntities(self.handles.clone())
    }
}

// ── Geometry ───────────────────────────────────────────────────────────────

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
}

impl Seg {
    fn flip(&mut self) {
        std::mem::swap(&mut self.a, &mut self.b);
        self.bulge = -self.bulge;
    }
}

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
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
                    })
                    .collect(),
            )
        }
        EntityType::Polyline2D(p) => {
            if p.is_closed() || p.vertices.len() < 2 {
                return None;
            }
            Some(
                p.vertices
                    .windows(2)
                    .map(|w| Seg {
                        a: DVec3::new(w[0].location.x, w[0].location.y, w[0].location.z),
                        b: DVec3::new(w[1].location.x, w[1].location.y, w[1].location.z),
                        bulge: w[0].bulge,
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

    let (chain, closed) = stitch(segs)?;

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
    if !closed && !has_arc && is_collinear(&verts) {
        let mut line = acadrust::entities::Line::new();
        line.common = common;
        line.common.handle = Handle::NULL;
        line.start = v3(verts.first().unwrap().0);
        line.end = v3(verts.last().unwrap().0);
        return Some((handles, vec![EntityType::Line(line)]));
    }

    if planar {
        let lw_verts: Vec<acadrust::entities::LwVertex> = verts
            .iter()
            .map(|(p, bulge)| {
                let mut v = acadrust::entities::LwVertex::new(Vector2::new(p.x, p.y));
                v.bulge = *bulge;
                v
            })
            .collect();
        let mut pl = acadrust::entities::LwPolyline::new();
        pl.common = common;
        pl.common.handle = Handle::NULL;
        pl.vertices = lw_verts;
        pl.is_closed = closed;
        pl.elevation = z0;
        return Some((handles, vec![EntityType::LwPolyline(pl)]));
    }

    // Non-planar: a 3D polyline carries no bulge, so a curved segment can't
    // be represented — refuse rather than silently flatten it.
    if has_arc {
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
