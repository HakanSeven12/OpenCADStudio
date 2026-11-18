//! The nodes a solid is made of, and how they refer to each other.
//!
//! ```text
//! Body ─► Lump ─► Shell ─► Face ─► Loop ─► Coedge ─► Edge ─► Vertex
//!                          │                 │
//!                          └► Surface        └► the coedge on the far side
//! ```
//!
//! A **lump** is one connected piece of solid; a body with two lumps is two
//! separate solids that a single operation produced. A **shell** is one
//! closed surface bounding a lump — a hollow cube is one lump with two
//! shells, an outer and an inner. A **face** is a bounded patch of one
//! surface; its **loops** are the rings that bound it, the first enclosing
//! and the rest cutting holes. A **coedge** is one face's use of an **edge**,
//! so a manifold edge has exactly two, one per side.
//!
//! # Why a coedge is not just a directed edge
//!
//! It records which way its loop traverses the edge, which is what makes a
//! face's boundary orientable and therefore what makes "inside" mean
//! anything. Two faces sharing an edge traverse it in opposite senses; when
//! they do not, the solid is inside out, and that is a thing
//! [`Body::validate`] can say rather than something a boolean discovers by
//! producing nonsense.
//!
//! # Why loops hold a `Vec` and not a linked ring
//!
//! ACIS links coedges into a ring with next and previous pointers, which
//! buys O(1) splicing on a loop of any size. Loops here are small — a face
//! with more than a dozen coedges is unusual and one with thousands does not
//! occur — and a `Vec` in traversal order has one ordering rather than two
//! that must be kept agreeing. Where a boolean would splice, it rebuilds.
//!
//! # Why an edge does not name its partner
//!
//! An edge holds the coedges that use it, and a coedge's partner is the other
//! one. Storing the partnership on both sides as well would be a second copy
//! of the same fact, and the two copies would eventually disagree.

use super::arena::{Arena, Key};
use super::geometry::{Curve3, Surface};
use super::Provenance;
use ocs_geom::space::Vec3;

/// Handle to a vertex.
pub type VertexKey = Key<Vertex>;
/// Handle to an edge.
pub type EdgeKey = Key<Edge>;
/// Handle to a coedge.
pub type CoedgeKey = Key<Coedge>;
/// Handle to a loop.
pub type LoopKey = Key<Loop>;
/// Handle to a face.
pub type FaceKey = Key<Face>;
/// Handle to a shell.
pub type ShellKey = Key<Shell>;
/// Handle to a lump.
pub type LumpKey = Key<Lump>;
/// Handle to a surface.
pub type SurfaceKey = Key<Surface>;
/// Handle to a curve.
pub type CurveKey = Key<Curve3>;

/// A point where edges meet.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    /// Where it is.
    pub point: [f64; 3],
    /// Where it came from.
    pub provenance: Provenance,
}

/// A curve between two vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The curve it runs along.
    pub curve: CurveKey,
    /// Parameter on that curve where the edge begins.
    pub start_parameter: f64,
    /// And where it ends. Greater than the start: an edge's direction is the
    /// curve's, and a coedge that wants the other way says so with its sense.
    pub end_parameter: f64,
    /// The vertex at the start parameter.
    pub start: VertexKey,
    /// The vertex at the end parameter.
    pub end: VertexKey,
    /// The coedges that use this edge — two for a manifold interior edge, one
    /// at the boundary of an open shell.
    pub coedges: Vec<CoedgeKey>,
    /// Where it came from.
    pub provenance: Provenance,
}

/// One face's use of an edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Coedge {
    /// The edge.
    pub edge: EdgeKey,
    /// Whether the loop runs along the edge in the curve's own direction.
    pub forward: bool,
    /// This use of the edge in its face's parameter space.
    pub pcurve: Option<ocs_geom::geom2d::Curve>,
    /// The loop it belongs to.
    pub owner: LoopKey,
    /// Where it came from.
    pub provenance: Provenance,
}

/// A ring of coedges bounding part of a face.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    /// The coedges in traversal order. The ring closes from the last back to
    /// the first.
    pub coedges: Vec<CoedgeKey>,
    /// The face it bounds.
    pub owner: FaceKey,
    /// Where it came from.
    pub provenance: Provenance,
}

/// A bounded patch of a surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// The surface it lies on.
    pub surface: SurfaceKey,
    /// Whether the face's outward normal agrees with the surface's. A cube
    /// built from one plane per side has three faces disagreeing with theirs.
    pub forward: bool,
    /// Its loops. The first bounds the face; any others cut holes in it.
    pub loops: Vec<LoopKey>,
    /// The shell it belongs to.
    pub owner: ShellKey,
    /// Where it came from.
    pub provenance: Provenance,
}

/// A connected set of faces bounding a region.
#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    /// Its faces.
    pub faces: Vec<FaceKey>,
    /// The lump it bounds part of.
    pub owner: LumpKey,
    /// Where it came from.
    pub provenance: Provenance,
}

/// One connected piece of solid.
#[derive(Debug, Clone, PartialEq)]
pub struct Lump {
    /// Its shells. The first is the outside; any others are voids within it.
    pub shells: Vec<ShellKey>,
    /// Where it came from.
    pub provenance: Provenance,
}

/// A whole solid, and everything it is made of.
#[derive(Debug, Clone)]
pub struct Body {
    /// Vertices.
    pub vertices: Arena<Vertex>,
    /// Edges.
    pub edges: Arena<Edge>,
    /// Coedges.
    pub coedges: Arena<Coedge>,
    /// Loops.
    pub loops: Arena<Loop>,
    /// Faces.
    pub faces: Arena<Face>,
    /// Shells.
    pub shells: Arena<Shell>,
    /// Lumps.
    pub lumps: Arena<Lump>,
    /// Surfaces, shared between faces that lie on the same one.
    pub surfaces: Arena<Surface>,
    /// Curves, shared between edges that run along the same one.
    pub curves: Arena<Curve3>,
    /// The lumps this body is made of, in order.
    pub roots: Vec<LumpKey>,
    /// Where the body itself came from.
    pub provenance: Provenance,
}

impl Body {
    /// An empty body.
    pub fn new() -> Self {
        Self {
            vertices: Arena::new(),
            edges: Arena::new(),
            coedges: Arena::new(),
            loops: Arena::new(),
            faces: Arena::new(),
            shells: Arena::new(),
            lumps: Arena::new(),
            surfaces: Arena::new(),
            curves: Arena::new(),
            roots: Vec::new(),
            // A body built here has no record to fall back on. One lifted
            // from a file has its own set by the lift.
            provenance: Provenance::Synthesized,
        }
    }

    /// The coedge on the other side of `coedge`'s edge, if the edge is shared.
    ///
    /// `None` at the boundary of an open shell, and for an edge used by more
    /// than two coedges — which is a non-manifold junction where "the other
    /// side" is not a question with one answer.
    pub fn partner(&self, coedge: CoedgeKey) -> Option<CoedgeKey> {
        let edge = self.edges.get(self.coedges.get(coedge)?.edge)?;
        match edge.coedges.as_slice() {
            [first, second] if *first == coedge => Some(*second),
            [first, second] if *second == coedge => Some(*first),
            _ => None,
        }
    }

    /// The next coedge round `coedge`'s edge, as ACIS chains partners: the
    /// other side for a manifold edge, and for a non-manifold one the next of
    /// its coedges, the last pointing back to the first. `None` when the
    /// edge has only this coedge.
    pub fn radial_next(&self, coedge: CoedgeKey) -> Option<CoedgeKey> {
        let edge = self.edges.get(self.coedges.get(coedge)?.edge)?;
        let at = edge.coedges.iter().position(|c| *c == coedge)?;
        (edge.coedges.len() >= 2).then(|| edge.coedges[(at + 1) % edge.coedges.len()])
    }

    /// The vertices a coedge runs from and to, in the loop's own direction.
    pub fn coedge_vertices(&self, coedge: CoedgeKey) -> Option<(VertexKey, VertexKey)> {
        let coedge = self.coedges.get(coedge)?;
        let edge = self.edges.get(coedge.edge)?;
        Some(if coedge.forward {
            (edge.start, edge.end)
        } else {
            (edge.end, edge.start)
        })
    }

    /// Every face in the body.
    pub fn face_keys(&self) -> impl Iterator<Item = FaceKey> + '_ {
        self.faces.keys()
    }

    /// Every edge in the body. What a wireframe walks.
    pub fn edge_keys(&self) -> impl Iterator<Item = EdgeKey> + '_ {
        self.edges.keys()
    }

    /// The coedges of a face, outer loop first.
    pub fn face_coedges(&self, face: FaceKey) -> Vec<CoedgeKey> {
        let Some(face) = self.faces.get(face) else {
            return Vec::new();
        };
        face.loops
            .iter()
            .filter_map(|key| self.loops.get(*key))
            .flat_map(|ring| ring.coedges.iter().copied())
            .collect()
    }

    /// The point an edge starts and ends at, read from its curve rather than
    /// from its vertices.
    ///
    /// The two should agree; where they do not, the file said one thing and
    /// meant another, which is what [`validate`](Self::validate) reports.
    pub fn edge_endpoints(&self, edge: EdgeKey) -> Option<([f64; 3], [f64; 3])> {
        let edge = self.edges.get(edge)?;
        let curve = self.curves.get(edge.curve)?;
        Some((
            curve.point_at(edge.start_parameter),
            curve.point_at(edge.end_parameter),
        ))
    }

    /// Marks a node and everything that owns it as edited, so lowering
    /// re-emits them instead of copying their source records through.
    ///
    /// Ownership, not adjacency: moving a vertex dirties the edges that end
    /// on it and the faces those bound, because their records name it. It
    /// does not dirty the face across the drawing that happens to be
    /// coplanar.
    pub fn soil_vertex(&mut self, vertex: VertexKey) {
        if let Some(node) = self.vertices.get_mut(vertex) {
            node.provenance.soil();
        }
        let touched: Vec<EdgeKey> = self
            .edges
            .iter()
            .filter(|(_, edge)| edge.start == vertex || edge.end == vertex)
            .map(|(key, _)| key)
            .collect();
        for edge in touched {
            self.soil_edge(edge);
        }
    }

    /// Marks an edge, its coedges, their loops and those faces as edited.
    pub fn soil_edge(&mut self, edge: EdgeKey) {
        let Some(node) = self.edges.get_mut(edge) else {
            return;
        };
        node.provenance.soil();
        let coedges = node.coedges.clone();
        for key in coedges {
            if let Some(coedge) = self.coedges.get_mut(key) {
                coedge.provenance.soil();
                let owner = coedge.owner;
                if let Some(ring) = self.loops.get_mut(owner) {
                    ring.provenance.soil();
                    let face = ring.owner;
                    if let Some(face) = self.faces.get_mut(face) {
                        face.provenance.soil();
                    }
                }
            }
        }
    }

    /// Everything wrong with the body's topology.
    ///
    /// Empty means consistent. Run after a lift, so a malformed file is
    /// reported where it was read rather than discovered later as a boolean
    /// that produces nothing; and after an edit, where it is the check that
    /// the edit put the topology back together.
    pub fn validate(&self) -> Vec<Flaw> {
        let mut flaws = Vec::new();

        for (key, lump) in self.lumps.iter() {
            if lump.shells.is_empty() {
                flaws.push(Flaw::EmptyLump(key));
            }
            for shell in &lump.shells {
                if !self.shells.contains(*shell) {
                    flaws.push(Flaw::DanglingKey("lump names a shell that is gone"));
                }
            }
        }

        for (key, shell) in self.shells.iter() {
            if shell.faces.is_empty() {
                flaws.push(Flaw::EmptyShell(key));
            }
            for face in &shell.faces {
                match self.faces.get(*face) {
                    Some(face) if face.owner != key => flaws.push(Flaw::BrokenOwnership(
                        "a face's shell is not the shell that lists it",
                    )),
                    Some(_) => {}
                    None => flaws.push(Flaw::DanglingKey("shell names a face that is gone")),
                }
            }
        }

        for (key, face) in self.faces.iter() {
            if !self.surfaces.contains(face.surface) {
                flaws.push(Flaw::DanglingKey("face names a surface that is gone"));
            }
            let closed_surface = self.surfaces.get(face.surface).is_some_and(|surface| {
                match surface {
                    Surface::Sphere(_) | Surface::Torus(_) => true,
                    Surface::Nurbs(_) | Surface::Offset(_) => {
                        surface.spline().is_some_and(|s| s.periodicity() == [true, true])
                    }
                    _ => false,
                }
            });
            if face.loops.is_empty() && !closed_surface {
                flaws.push(Flaw::UnboundedFace(key));
            }
            for ring in &face.loops {
                match self.loops.get(*ring) {
                    Some(ring) if ring.owner != key => flaws.push(Flaw::BrokenOwnership(
                        "a loop's face is not the face that lists it",
                    )),
                    Some(_) => {}
                    None => flaws.push(Flaw::DanglingKey("face names a loop that is gone")),
                }
            }
        }

        for (key, ring) in self.loops.iter() {
            // One coedge is a loop only when its edge closes on itself — the
            // rim of a disc, the seam of a full revolution. Anything else
            // needs at least two to get back where it started.
            let closed_alone = ring.coedges.len() == 1
                && self
                    .coedges
                    .get(ring.coedges[0])
                    .and_then(|coedge| self.edges.get(coedge.edge))
                    .is_some_and(|edge| edge.start == edge.end);
            let singular = ring.coedges.is_empty()
                && self
                    .faces
                    .get(ring.owner)
                    .and_then(|face| self.surfaces.get(face.surface))
                    .is_some_and(|surface| {
                        matches!(surface, Surface::Sphere(_))
                            || matches!(surface, Surface::Cone(cone) if cone.half_angle.tan().abs() > 1e-15)
                            || matches!(
                                surface,
                                Surface::Torus(torus)
                                    if torus.minor_radius.abs() > f64::EPSILON
                                        && torus.major_radius.abs()
                                            <= torus.minor_radius.abs()
                            )
                    });
            if !singular
                && (ring.coedges.is_empty()
                    || (ring.coedges.len() < 2 && !closed_alone))
            {
                flaws.push(Flaw::DegenerateLoop(key));
                continue;
            }
            for coedge in &ring.coedges {
                match self.coedges.get(*coedge) {
                    Some(coedge) if coedge.owner != key => flaws.push(Flaw::BrokenOwnership(
                        "a coedge's loop is not the loop that lists it",
                    )),
                    Some(_) => {}
                    None => flaws.push(Flaw::DanglingKey("loop names a coedge that is gone")),
                }
            }
            // The ring has to close: each coedge must end where the next
            // begins. A loop that does not is a face with a gap in its
            // boundary, and every containment question asked of it afterwards
            // has no answer.
            let count = ring.coedges.len();
            for index in 0..count {
                let here = ring.coedges[index];
                let next = ring.coedges[(index + 1) % count];
                let (Some((_, ends)), Some((begins, _))) =
                    (self.coedge_vertices(here), self.coedge_vertices(next))
                else {
                    continue;
                };
                if ends != begins {
                    flaws.push(Flaw::OpenLoop(key));
                    break;
                }
            }
        }

        for (key, edge) in self.edges.iter() {
            if !self.curves.contains(edge.curve) {
                flaws.push(Flaw::DanglingKey("edge names a curve that is gone"));
            }
            if !self.vertices.contains(edge.start) || !self.vertices.contains(edge.end) {
                flaws.push(Flaw::DanglingKey("edge names a vertex that is gone"));
            }
            let mut seam = vec![false; edge.coedges.len()];
            for first in 0..edge.coedges.len() {
                if seam[first] {
                    continue;
                }
                let Some(first_coedge) = self.coedges.get(edge.coedges[first]) else {
                    continue;
                };
                for second in first + 1..edge.coedges.len() {
                    if seam[second] {
                        continue;
                    }
                    let Some(second_coedge) = self.coedges.get(edge.coedges[second]) else {
                        continue;
                    };
                    if first_coedge.owner == second_coedge.owner
                        && first_coedge.forward != second_coedge.forward
                    {
                        seam[first] = true;
                        seam[second] = true;
                        break;
                    }
                }
            }
            let effective: Vec<_> = edge
                .coedges
                .iter()
                .enumerate()
                .filter_map(|(index, key)| (!seam[index]).then_some(*key))
                .collect();
            match effective.len() {
                0 if edge.coedges.is_empty() => flaws.push(Flaw::UnusedEdge(key)),
                0..=2 => {}
                _ => flaws.push(Flaw::NonManifoldEdge(key)),
            }
            for coedge in &edge.coedges {
                match self.coedges.get(*coedge) {
                    Some(coedge) if coedge.edge != key => flaws.push(Flaw::BrokenOwnership(
                        "a coedge's edge is not the edge that lists it",
                    )),
                    Some(_) => {}
                    None => flaws.push(Flaw::DanglingKey("edge names a coedge that is gone")),
                }
            }
            // Two coedges on one edge must run it opposite ways, or the two
            // faces they belong to face the same direction along their shared
            // border and the solid is inside out.
            if let [first, second] = effective.as_slice() {
                if let (Some(first), Some(second)) =
                    (self.coedges.get(*first), self.coedges.get(*second))
                {
                    if first.forward == second.forward {
                        flaws.push(Flaw::SameSidedEdge(key));
                    }
                }
            }
        }

        flaws
    }

    /// How far a vertex sits from the end of an edge that claims it, at
    /// worst.
    ///
    /// Separate from [`validate`](Self::validate) because it is a measurement
    /// rather than a yes or no: a file written by another modeller routinely
    /// disagrees with itself by a rounding, and what counts as too far is the
    /// caller's tolerance to set.
    pub fn worst_vertex_gap(&self) -> f64 {
        let mut worst: f64 = 0.0;
        for (key, edge) in self.edges.iter() {
            let Some((start, end)) = self.edge_endpoints(key) else {
                continue;
            };
            for (point, vertex) in [(start, edge.start), (end, edge.end)] {
                if let Some(vertex) = self.vertices.get(vertex) {
                    worst = worst.max(Vec3::from(point).distance(Vec3::from(vertex.point)));
                }
            }
        }
        worst
    }

    /// `V − E + F` over the whole body.
    ///
    /// For a single closed shell of genus zero this is 2, and for one with
    /// `g` handles it is `2 − 2g`. It is the cheapest check that a solid's
    /// topology hangs together, and the one that catches a face left out or
    /// an edge counted twice.
    pub fn euler_characteristic(&self) -> i64 {
        self.vertices.len() as i64 - self.edges.len() as i64 + self.faces.len() as i64
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::new()
    }
}

/// Something wrong with a body's topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flaw {
    /// A key that no longer names anything.
    DanglingKey(&'static str),
    /// A node and its owner disagree about who owns whom.
    BrokenOwnership(&'static str),
    /// A lump with no shells.
    EmptyLump(LumpKey),
    /// A shell with no faces.
    EmptyShell(ShellKey),
    /// A face with no loops, so nothing bounds it.
    UnboundedFace(FaceKey),
    /// A loop of fewer than two coedges.
    DegenerateLoop(LoopKey),
    /// A loop whose coedges do not join end to end.
    OpenLoop(LoopKey),
    /// An edge no coedge uses.
    UnusedEdge(EdgeKey),
    /// An edge used by more than two coedges.
    NonManifoldEdge(EdgeKey),
    /// An edge whose two coedges run it the same way, so the faces either
    /// side face the same direction.
    SameSidedEdge(EdgeKey),
}
