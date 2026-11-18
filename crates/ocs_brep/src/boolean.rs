//! Combining two solids.
//!
//! Once both have been imprinted with the curves they share, every face of
//! each is wholly inside the other or wholly outside it. What remains is
//! choosing which of those faces the result keeps, and turning the chosen set
//! back into a solid.
//!
//! | operation    | keeps of A | keeps of B          |
//! |--------------|------------|---------------------|
//! | union        | outside B  | outside A           |
//! | intersection | inside B   | inside A            |
//! | difference   | outside B  | inside A, flipped   |
//!
//! The flip in the last row is the whole of what makes a difference a
//! difference: B's faces become the walls of the cavity it leaves, so they
//! have to face the other way.
//!
//! # Why it refuses so readily
//!
//! A boolean that half-works produces a solid — one with a wall missing,
//! which looks finished, saves, and fails much later in something else's
//! hands. Every step that cannot be completed exactly is returned as a
//! [`Snag`] instead: a face pair with no closed form, a face that cannot be
//! classified, a coincident pair. Refusing is cheap; a leaking solid is not.

use super::classify::{contains_point, Containment};
use super::geometry::{Curve3, Line3, Surface};
use super::imprint::{imprint_reaching, Snag};
use super::pcurve;
use super::topology::{Body, CoedgeKey, EdgeKey, Face, FaceKey, Lump, Shell, VertexKey};
use super::{Placement, Provenance};
use ocs_geom::geom2d::Curve;
use ocs_geom::space::Vec3;
use std::collections::{HashMap, HashSet, VecDeque};

/// Which combination to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Everything in either.
    Union,
    /// Only what is in both.
    Intersection,
    /// What is in the first and not the second.
    Difference,
}

/// Combines two solids.
///
/// Neither input is consumed; the result is a new body. Both are imprinted
/// along the way, which is why they are taken by value — an imprinted copy is
/// not the body the caller handed over, and returning it silently would be
/// worse than asking for ownership.
pub fn combine(a: Body, b: Body, how: Operation, tolerance: f64) -> Result<Body, Snag> {
    // Solidcore's Boolean first; this kernel's own cut stays for what Solidcore refuses.
    let op = match how {
        Operation::Union => solidcore_ops::BooleanOp::Union,
        Operation::Intersection => solidcore_ops::BooleanOp::Intersect,
        Operation::Difference => solidcore_ops::BooleanOp::Subtract,
    };
    if let Ok(body) = super::bridge::boolean(&a, &b, op) {
        return Ok(body);
    }
    // Far from the origin — survey coordinates — every intersection loses
    // the digits the offset takes. Worked about the pair's own centre and
    // moved back, they keep them.
    let centre = super::body_bounds(&a).zip(super::body_bounds(&b)).and_then(|(one, other)| {
        let low = Vec3::from(std::array::from_fn(|axis| one.min[axis].min(other.min[axis])));
        let high = Vec3::from(std::array::from_fn(|axis| one.max[axis].max(other.max[axis])));
        let centre = (low + high) * 0.5;
        (centre.length() > (high - low).length()).then_some(centre)
    });
    let moved = centre.and_then(|centre| {
        let there = Placement::at((-centre).to_array());
        Some((super::transform(&a, &there)?, super::transform(&b, &there)?, centre))
    });
    let Some((a, b, centre)) = moved else {
        return combine_dividing(a, b, how, tolerance);
    };
    let result = combine_dividing(a, b, how, tolerance)?;
    super::transform(&result, &Placement::at(centre.to_array())).ok_or(Snag::CutRefused)
}

/// Combines one solid with many: `base` united with every tool, or with
/// every tool taken away. An intersection is folded one tool at a time.
///
/// Tools whose boxes keep clear of each other go in together, as one body of
/// several lumps, so a plate drilled with a hundred holes is cut once rather
/// than re-imprinted, re-classified and re-joined a hundred times. A batch
/// that refuses as one is cut one tool at a time instead. Tools that
/// overlap each other are never put together: a point inside two of them
/// would count its way out again.
///
/// After Solidcore's list Booleans (`boolean/merge/p3.rs`, `boolean_lists`,
/// which combine each list into one operand first). Apache-2.0, see
/// THIRD_PARTY.md.
pub fn combine_all(
    base: Body,
    tools: Vec<Body>,
    how: Operation,
    tolerance: f64,
) -> Result<Body, Snag> {
    if how == Operation::Intersection {
        return tools
            .into_iter()
            .try_fold(base, |result, tool| combine(result, tool, how, tolerance));
    }
    let clear = tolerance * 10.0;
    let mut batches: Vec<(Vec<Body>, Vec<super::Aabb>)> = Vec::new();
    for tool in tools {
        let Some(bounds) = super::body_bounds(&tool) else {
            batches.push((vec![tool], Vec::new()));
            continue;
        };
        let bounds = bounds.grown(clear);
        let fits = batches.iter().position(|(members, boxes)| {
            boxes.len() == members.len() && boxes.iter().all(|other| !other.overlaps(&bounds))
        });
        match fits {
            Some(index) => {
                batches[index].0.push(tool);
                batches[index].1.push(bounds);
            }
            None => batches.push((vec![tool], vec![bounds])),
        }
    }
    let mut result = base;
    for (members, _) in batches {
        if members.len() == 1 {
            let tool = members.into_iter().next().ok_or(Snag::CutRefused)?;
            result = combine(result, tool, how, tolerance)?;
            continue;
        }
        let together = lumps_of(&members, tolerance)
            .and_then(|tool| combine(result.clone(), tool, how, tolerance));
        result = match together {
            Ok(body) => body,
            Err(_) => members
                .into_iter()
                .try_fold(result, |result, tool| combine(result, tool, how, tolerance))?,
        };
    }
    Ok(result)
}

/// Bodies that do not touch, as one body: every shell of each, a lump of
/// its own.
fn lumps_of(bodies: &[Body], tolerance: f64) -> Result<Body, Snag> {
    let mut joined = Body::new();
    for body in bodies {
        for root in &body.roots {
            let shells = body.lumps.get(*root).ok_or(Snag::CutRefused)?.shells.clone();
            let lump = joined.lumps.insert(Lump {
                shells: Vec::new(),
                provenance: Provenance::Synthesized,
            });
            joined.roots.push(lump);
            for from in shells {
                let faces = body.shells.get(from).ok_or(Snag::CutRefused)?.faces.clone();
                let shell = joined.shells.insert(Shell {
                    faces: Vec::new(),
                    owner: lump,
                    provenance: Provenance::Synthesized,
                });
                joined.lumps.get_mut(lump).ok_or(Snag::CutRefused)?.shells.push(shell);
                for face in faces {
                    copy_face_with_tolerance(&mut joined, body, face, shell, false, tolerance)?;
                }
            }
        }
    }
    match joined.validate().is_empty() {
        true => Ok(joined),
        false => Err(Snag::CutRefused),
    }
}

/// Which whole round bodies are divided into seamless patches before a
/// Boolean (see `divided_torus`). A sphere always is.
#[derive(Clone, Copy, PartialEq)]
enum Divide {
    Nothing,
    TorusAndCone,
    AlsoCylinder,
}

/// [`combine_here`] with a whole torus or cone divided into seamless
/// patches first, as it came where that does not give a result whose every
/// face meshes, and with a cylinder divided too where neither does.
fn combine_dividing(a: Body, b: Body, how: Operation, tolerance: f64) -> Result<Body, Snag> {
    // Cut edges run on both bodies' surfaces to within the fit of a traced
    // curve, which the retries open up tenfold; a little more is only
    // a face meshed that need not have been.
    let tolerance_of_cut = tolerance * 1000.0;
    // Only round faces are asked: dividing round bodies is all the ways of
    // trying differ by, and no way of dividing them mends a flat face that
    // will not mesh — while a plate's face, with every hole cut so far, was
    // meshed again at every cut of a long sequence.
    //
    // Nor is a face the cut never reached: one with no edge on both bodies'
    // surfaces is a face of an input, or a patch the dividing made, as it
    // came. Meshing every round face of the result was most of a curved
    // boolean's time; the cut ones are what a way of dividing can spoil.
    let surfaces_of = |body: &Body| -> Vec<Surface> {
        body.faces
            .iter()
            .filter_map(|(_, face)| body.surfaces.get(face.surface))
            .cloned()
            .collect()
    };
    let inputs = [surfaces_of(&a), surfaces_of(&b)];
    let on_both = |point: [f64; 3], fit: f64| {
        inputs.iter().all(|surfaces| {
            surfaces.iter().any(|surface| surface.distance_to(point).abs() <= fit)
        })
    };
    let meshes = |body: &Body| {
        let tolerance =
            super::mesh::TessellationTolerance::new(ocs_geom::tessellation::DEFAULT_ANGLE, 1e-9);
        let cut_edge = |edge: EdgeKey| {
            body.edges.get(edge).is_some_and(|edge| {
                body.curves.get(edge.curve).is_some_and(|curve| {
                    let middle = 0.5 * (edge.start_parameter + edge.end_parameter);
                    on_both(curve.point_at(middle), tolerance_of_cut)
                })
            })
        };
        body.face_keys().all(|face| {
            let flat = body
                .faces
                .get(face)
                .and_then(|node| body.surfaces.get(node.surface))
                .is_some_and(|surface| matches!(surface, Surface::Plane(_)));
            let cut = || {
                body.face_coedges(face).iter().any(|coedge| {
                    body.coedges.get(*coedge).is_some_and(|coedge| cut_edge(coedge.edge))
                })
            };
            flat || !cut() || super::mesh::face_meshes(body, face, tolerance)
        })
    };
    // A way of dividing that finds nothing more to divide than the one
    // before it repeats that one's work exactly, so it is not tried.
    let carries = |round: fn(&Surface) -> bool| {
        [&a, &b].iter().any(|body| body.surfaces.iter().any(|(_, surface)| round(surface)))
    };
    let tori_or_cones = carries(|surface| matches!(surface, Surface::Torus(_) | Surface::Cone(_)));
    let cylinders = carries(|surface| matches!(surface, Surface::Cylinder(_)));
    let ways: Vec<Divide> = [Divide::TorusAndCone, Divide::Nothing, Divide::AlsoCylinder]
        .into_iter()
        .filter(|divide| match divide {
            Divide::Nothing => tori_or_cones,
            Divide::AlsoCylinder => cylinders,
            Divide::TorusAndCone => true,
        })
        .collect();
    // Every way of dividing is tried plainly before any is tried again more
    // loosely: a way that refuses outright (a ball divided at corners its
    // crossing runs through) was retried three times over, at the cost of
    // the whole boolean each time, before the next way was asked.
    let single = ways.len() == 1;
    // The first result made stands where none meshes whole, and the first
    // refusal where none is made.
    let mut first: Option<Result<Body, Snag>> = None;
    for retry in Retry::ALL {
        for divide in &ways {
            let result = combine_attempt(a.clone(), b.clone(), how, tolerance, *divide, retry);
            // With no other way to try, whether it meshes changes nothing:
            // the first result made stands either way. Meshing every round
            // face of it only to find that out was most of a sphere's
            // boolean.
            if single && result.is_ok() {
                return result;
            }
            if result.as_ref().is_ok_and(|body| body.faces.is_empty() || meshes(body)) {
                return result;
            }
            if first.as_ref().is_none_or(|first| first.is_err() && result.is_ok()) {
                first = Some(result);
            }
        }
    }
    first.unwrap_or(Err(Snag::CutRefused))
}

/// How a refused [`combine_here`] is tried again.
#[derive(Clone, Copy, PartialEq)]
enum Retry {
    /// As asked.
    Plain,
    /// Imprinting reaching curves (see [`imprint_reaching`]).
    Reach,
    /// The tolerance opened up tenfold: cuts landing a hair apart, a hair
    /// from a corner, read as one.
    Looser,
    /// The pair the other way round: the imprint cuts the second body
    /// against the first's cuts, and the order can settle a corner. Taken
    /// only where every face of it still meshes.
    Swapped,
}

impl Retry {
    const ALL: [Self; 4] = [Self::Plain, Self::Reach, Self::Looser, Self::Swapped];
}

/// One attempt at [`combine_here`].
fn combine_attempt(
    a: Body,
    b: Body,
    how: Operation,
    tolerance: f64,
    divide: Divide,
    retry: Retry,
) -> Result<Body, Snag> {
    match retry {
        Retry::Plain => combine_here(a, b, how, tolerance, false, false, divide),
        Retry::Reach => combine_here(a, b, how, tolerance, false, true, divide),
        Retry::Looser => combine_here(a, b, how, tolerance * 10.0, false, false, divide),
        Retry::Swapped => {
            let result = combine_here(b, a, how, tolerance, true, false, divide)?;
            let mesh = super::mesh::tessellate(
                &result,
                super::mesh::TessellationTolerance::new(0.2, 1e-3),
            );
            (mesh.missing_faces.is_empty() && !mesh.mesh.is_empty())
                .then_some(result)
                .ok_or(Snag::CutRefused)
        }
    }
}

/// `swapped`: the bodies come second first, which only a difference minds —
/// the first is then the one taken away. `reach`: see
/// [`imprint_reaching`].
fn combine_here(
    mut a: Body,
    mut b: Body,
    how: Operation,
    tolerance: f64,
    swapped: bool,
    reach: bool,
    divide: Divide,
) -> Result<Body, Snag> {
    let divided = |body: &Body, other: &Body| {
        divided_sphere(body, other, tolerance)
            .or_else(|| match divide {
                Divide::Nothing => None,
                _ => divided_torus(body, other, tolerance)
                    .or_else(|| divided_cone(body, other, tolerance)),
            })
            .or_else(|| match divide {
                Divide::AlsoCylinder => divided_cylinder(body, other, tolerance),
                _ => None,
            })
    };
    let (divided_a, divided_b) = (divided(&a, &b), divided(&b, &a));
    if let Some(divided) = divided_a {
        a = divided;
    }
    if let Some(divided) = divided_b {
        b = divided;
    }
    let original_a = a.clone();
    let original_b = b.clone();
    // A solid may arrive with edges more than two faces share: a ball held
    // against the walls of its pocket along a line (#1563). Those stay as
    // they came, so the result may carry them too.
    let shared_edges = [&a, &b]
        .iter()
        .any(|body| body.edges.iter().any(|(_, edge)| edge.coedges.len() > 2));
    imprint_reaching(&mut a, &mut b, tolerance, reach)?;

    let (keep_a, keep_b, flip_a, flip_b) = match (how, swapped) {
        (Operation::Union, _) => (Containment::Outside, Containment::Outside, false, false),
        (Operation::Intersection, _) => (Containment::Inside, Containment::Inside, false, false),
        (Operation::Difference, false) => {
            (Containment::Outside, Containment::Inside, false, true)
        }
        (Operation::Difference, true) => (Containment::Inside, Containment::Outside, true, false),
    };

    let mut result = Body::new();
    let lump = result.lumps.insert(Lump {
        shells: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let shell = result.shells.insert(Shell {
        faces: Vec::new(),
        owner: lump,
        provenance: Provenance::Synthesized,
    });
    result.lumps.get_mut(lump).expect("just inserted").shells = vec![shell];
    result.roots = vec![lump];

    let mut kept = 0;
    // Each body's face boxes, measured once for the twin search: index 0 the
    // second body's (asked while the first is copied), 1 the first's.
    let boxes: [HashMap<FaceKey, Option<super::Aabb>>; 2] = [&b, &a]
        .map(|body| body.face_keys().map(|face| (face, super::face_bounds(body, face))).collect());
    // The faces taken from the first body, once it is done.
    let mut from_first = HashSet::new();
    for (body, other, classifier, wanted, flip, flip_other, first) in [
        (&a, &b, &original_b, keep_a, flip_a, flip_b, true),
        (&b, &a, &original_a, keep_b, flip_b, flip_a, false),
    ] {
        if !first {
            from_first = result.face_keys().collect();
        }
        // Faces meeting along an edge off every surface of the other body
        // are on one side of it: the side is asked once a group.
        let groups = side_groups(body, classifier, tolerance);
        let mut sides: HashMap<usize, Containment> = HashMap::new();
        for face in body.face_keys() {
            // A shared wall is settled by the two normals rather than by
            // which side it is on: it is on both.
            let other_boxes = if first { &boxes[0] } else { &boxes[1] };
            if let Some(twin) = coincident_twin(body, other, face, other_boxes, tolerance) {
                if keeps_shared_wall(body, face, other, twin, [flip, flip_other], first) {
                    copy_face_with_tolerance(&mut result, body, face, shell, flip, tolerance)?;
                    kept += 1;
                }
                continue;
            }
            let group = groups.get(&face).copied();
            let side = match group.and_then(|group| sides.get(&group)) {
                Some(side) => *side,
                None => {
                    // Read at the face's edges on the other body first: any
                    // face of the group settles the whole group.
                    let local = match group {
                        Some(group) => body
                            .face_keys()
                            .filter(|key| groups.get(key) == Some(&group))
                            .find_map(|key| local_side(body, classifier, key, tolerance)),
                        None => local_side(body, classifier, face, tolerance),
                    };
                    let side =
                        local.unwrap_or_else(|| face_side(body, classifier, face, tolerance));
                    if let (Some(group), Containment::Inside | Containment::Outside) = (group, side)
                    {
                        sides.insert(group, side);
                    }
                    side
                }
            };
            match side {
                // A piece of a shared wall the imprint divided differently
                // on the two bodies has no twin of its own, but it lies on
                // a face of the other on the same surface: the normals
                // settle it as they do a twin.
                Containment::OnBoundary => {
                    let wall = wall_under(body, face, other, tolerance).ok_or(Snag::Coincident)?;
                    if keeps_shared_wall(body, face, other, wall, [flip, flip_other], first) {
                        copy_face_with_tolerance(&mut result, body, face, shell, flip, tolerance)?;
                        kept += 1;
                    }
                }
                Containment::Unknown => return Err(Snag::CutRefused),
                side if side == wanted => {
                    copy_face_with_tolerance(&mut result, body, face, shell, flip, tolerance)?;
                    kept += 1;
                }
                _ => {}
            }
        }
    }
    if kept == 0 {
        // Nothing survived: the operation's answer is genuinely empty — two
        // solids that do not touch, intersected.
        return Ok(Body::new());
    }
    separate_touching_edges(&mut result, &from_first);
    orient_shell(&mut result)?;
    if result.edges.iter().any(|(_, edge)| {
        let uses = edge.coedges.len();
        uses != 2 && !(shared_edges && uses > 2 && uses % 2 == 0)
    })
        || !result.validate().is_empty()
    {
        return Err(Snag::CutRefused);
    }
    regroup_shells(&mut result, tolerance)?;
    merge_coplanar_faces(&mut result, tolerance);
    split_pinched_loops(&mut result);
    Ok(result)
}

/// Parts a flat face's loop that passes through one vertex twice — a hole
/// cut tangent to the face's rim, its circle and the rim's run one after the
/// other through the point they share — into the rim and the hole, each a
/// loop of its own: what a face with a hole is everywhere else, and what
/// meshing and measuring it expect. The loop running counter-clockwise about
/// the face's outward normal stays first. A body that would not validate
/// afterwards is left as it was.
fn split_pinched_loops(body: &mut Body) {
    let mut parted = body.clone();
    let mut changed = false;
    for face in parted.face_keys().collect::<Vec<_>>() {
        let Some(outward) = parted.faces.get(face).and_then(|node| {
            let Surface::Plane(plane) = parted.surfaces.get(node.surface)? else {
                return None;
            };
            Some(Vec3::from(plane.normal()?) * if node.forward { 1.0 } else { -1.0 })
        }) else {
            continue;
        };
        let mut index = 0;
        while let Some(ring) =
            parted.faces.get(face).and_then(|node| node.loops.get(index)).copied()
        {
            match part_loop(&mut parted, ring, outward) {
                Some(()) => changed = true,
                None => index += 1,
            }
        }
    }
    if changed && parted.validate().is_empty() {
        *body = parted;
    }
}

/// Moves the stretch of `ring` between two visits to one vertex into a new
/// loop of the same face. `None` when no vertex repeats.
fn part_loop(body: &mut Body, ring: super::LoopKey, outward: Vec3) -> Option<()> {
    let coedges = body.loops.get(ring)?.coedges.clone();
    let start_of = |coedge: CoedgeKey| -> Option<VertexKey> {
        let coedge = body.coedges.get(coedge)?;
        let edge = body.edges.get(coedge.edge)?;
        Some(if coedge.forward { edge.start } else { edge.end })
    };
    let starts = coedges.iter().map(|coedge| start_of(*coedge)).collect::<Option<Vec<_>>>()?;
    let (from, to) = (0..starts.len()).find_map(|from| {
        (from + 1..starts.len()).find(|to| starts[*to] == starts[from]).map(|to| (from, to))
    })?;
    let inner: Vec<CoedgeKey> = coedges[from..to].to_vec();
    let outer: Vec<CoedgeKey> = coedges[..from].iter().chain(&coedges[to..]).copied().collect();
    // Which way each runs round the face's normal, from its edges' points.
    let winding = |part: &[CoedgeKey]| -> Option<f64> {
        let mut points = Vec::new();
        for coedge in part {
            let node = body.coedges.get(*coedge)?;
            let mut along = super::edge_points(body, node.edge, 1e-3)?;
            if !node.forward {
                along.reverse();
            }
            points.extend(along.into_iter().map(Vec3::from));
        }
        let mut area = Vec3::new(0.0, 0.0, 0.0);
        for (index, point) in points.iter().enumerate() {
            area = area + point.cross(points[(index + 1) % points.len()]);
        }
        Some(area.dot(outward))
    };
    let (first, second) = match (winding(&outer)?, winding(&inner)?) {
        (o, i) if o > 0.0 && i < 0.0 => (outer, inner),
        (o, i) if i > 0.0 && o < 0.0 => (inner, outer),
        _ => return None,
    };
    let face = body.loops.get(ring)?.owner;
    let provenance = body.loops.get(ring)?.provenance.clone();
    body.loops.get_mut(ring)?.coedges = first;
    let hole = body.loops.insert(super::topology::Loop {
        coedges: second.clone(),
        owner: face,
        provenance,
    });
    for coedge in second {
        body.coedges.get_mut(coedge)?.owner = hole;
    }
    let loops = &mut body.faces.get_mut(face)?.loops;
    let at = loops.iter().position(|key| *key == ring)?;
    loops.insert(at + 1, hole);
    Some(())
}

/// A closed shell's volume, signed by which way its faces point.
fn meshed_volume(piece: &Body, tolerance: f64) -> Result<f64, Snag> {
    let mesh =
        super::mesh::tessellate(piece, super::mesh::TessellationTolerance::new(0.3, tolerance));
    if !mesh.missing_faces.is_empty() || mesh.mesh.is_empty() {
        return Err(Snag::CutRefused);
    }
    let pivot = Vec3::from(mesh.mesh.positions[0]);
    Ok(mesh
        .mesh
        .triangles
        .iter()
        .map(|triangle| {
            let a = Vec3::from(mesh.mesh.positions[triangle[0]]) - pivot;
            let b = Vec3::from(mesh.mesh.positions[triangle[1]]) - pivot;
            let c = Vec3::from(mesh.mesh.positions[triangle[2]]) - pivot;
            a.dot(b.cross(c)) / 6.0
        })
        .sum())
}

/// A closed shell's box volume, negative where its faces point into what
/// it encloses — a cavity's: a step off a face along its outward normal
/// lands inside the shell. Meshing each shell for its signed volume was
/// most of a block's time being bitten into again and again. `None` where
/// the step reads neither way.
fn shell_sense(piece: &Body, tolerance: f64) -> Option<f64> {
    let bounds = super::body_bounds(piece)?;
    let size = Vec3::from(bounds.min).distance(Vec3::from(bounds.max));
    let extent: f64 =
        (0..3).map(|axis| (bounds.max[axis] - bounds.min[axis]).max(tolerance)).product();
    let face = piece.face_keys().next()?;
    let node = piece.faces.get(face)?;
    let surface = piece.surfaces.get(node.surface)?;
    let point = interior_point(piece, face, tolerance)?;
    let (u, v) = surface.parameters_at(point)?;
    let normal = Vec3::from(surface.normal_at(u, v)?).normalize()?;
    let outward = if node.forward { normal } else { -normal };
    let step = (size * 1e-4).max(tolerance * 100.0);
    match contains_point(piece, (Vec3::from(point) + outward * step).to_array(), tolerance) {
        Containment::Outside => Some(extent),
        Containment::Inside => Some(-extent),
        _ => None,
    }
}

/// Groups of `body`'s faces joined across edges that keep off every surface
/// of `other`: after the imprint each face lies wholly to one side of the
/// other body, and crossing an edge that is not on its boundary does not
/// change the side. A face on its boundary forms no group.
fn side_groups(body: &Body, other: &Body, tolerance: f64) -> HashMap<FaceKey, usize> {
    let surfaces: Vec<&Surface> =
        other.faces.iter().filter_map(|(_, face)| other.surfaces.get(face.surface)).collect();
    let near = |point: [f64; 3]| {
        surfaces.iter().any(|surface| surface.distance_to(point).abs() <= tolerance * 10.0)
    };
    let mut parent: HashMap<FaceKey, FaceKey> =
        body.face_keys().map(|face| (face, face)).collect();
    fn root(parent: &mut HashMap<FaceKey, FaceKey>, mut face: FaceKey) -> FaceKey {
        while parent[&face] != face {
            let up = parent[&parent[&face]];
            parent.insert(face, up);
            face = up;
        }
        face
    }
    for (_, edge) in body.edges.iter() {
        let [first, second] = edge.coedges[..] else { continue };
        let (Some(one), Some(other_face)) = (owner_face(body, first), owner_face(body, second))
        else {
            continue;
        };
        let Some(curve) = body.curves.get(edge.curve) else { continue };
        let span = edge.end_parameter - edge.start_parameter;
        let crosses = [0.25, 0.5, 0.75]
            .into_iter()
            .any(|at| near(curve.point_at(edge.start_parameter + span * at)));
        if !crosses {
            let (a, b) = (root(&mut parent, one), root(&mut parent, other_face));
            parent.insert(a, b);
        }
    }
    let faces: Vec<FaceKey> = body.face_keys().collect();
    let mut ids: HashMap<FaceKey, usize> = HashMap::new();
    let mut groups = HashMap::new();
    for face in faces {
        let top = root(&mut parent, face);
        let next = ids.len();
        groups.insert(face, *ids.entry(top).or_insert(next));
    }
    groups
}

/// Gives each body its own copy of an edge the two only touch along: two
/// boxes sharing an edge, two cylinders side by side, a hole tangent to the
/// wall it is cut in. Sewn by position, such an edge carries two faces of
/// each body; each pair already closes its own body's side, so each pair
/// keeps an edge of its own, and the result is two solids touching, or one
/// pinched, rather than four faces on one edge. An edge whose faces do not
/// part two and two between the bodies, each pair running both ways along
/// it, is left as it is.
fn separate_touching_edges(body: &mut Body, from_first: &HashSet<FaceKey>) {
    let touching: Vec<(EdgeKey, Vec<CoedgeKey>)> = body
        .edges
        .iter()
        .filter(|(_, edge)| edge.coedges.len() == 4)
        .filter_map(|(key, edge)| {
            let second: Vec<CoedgeKey> = edge
                .coedges
                .iter()
                .copied()
                .filter(|coedge| {
                    owner_face(body, *coedge).is_some_and(|face| !from_first.contains(&face))
                })
                .collect();
            let first: Vec<CoedgeKey> =
                edge.coedges.iter().copied().filter(|coedge| !second.contains(coedge)).collect();
            let both_ways = |pair: &[CoedgeKey]| {
                pair.len() == 2
                    && pair
                        .iter()
                        .filter_map(|coedge| body.coedges.get(*coedge))
                        .map(|coedge| coedge.forward)
                        .collect::<HashSet<_>>()
                        .len()
                        == 2
            };
            (both_ways(&first) && both_ways(&second)).then_some((key, second))
        })
        .collect();
    for (key, second) in touching {
        let Some(edge) = body.edges.get_mut(key) else { continue };
        edge.coedges.retain(|coedge| !second.contains(coedge));
        let mut copy = edge.clone();
        copy.coedges = second.clone();
        let copy = body.edges.insert(copy);
        for coedge in second {
            if let Some(coedge) = body.coedges.get_mut(coedge) {
                coedge.edge = copy;
            }
        }
    }
}

/// Joins what the cut left split along a line it no longer needs: coplanar
/// neighbours, such as the side of a box and of the prism pulled from its
/// top, become one face, and straight edges left in line become one edge.
/// A body that would not validate afterwards is left as it was.
pub(super) fn merge_coplanar_faces(body: &mut Body, tolerance: f64) {
    merge_coplanar(body, tolerance, false);
}

/// [`merge_coplanar_faces`] along curved edges too: a fill put back into a
/// rounded corner leaves its end in a piece of the end wall, cut off along
/// the round's arc.
pub(super) fn merge_coplanar_pieces(body: &mut Body, tolerance: f64) {
    merge_coplanar(body, tolerance, true);
}

/// [`merge_coplanar_pieces`], joining edges left in line even where no
/// faces were joined: what a solid's clean-up asks for.
pub(super) fn clean_body(body: &mut Body, tolerance: f64) {
    merge_coplanar_with(body, tolerance, true, true);
}

fn merge_coplanar(body: &mut Body, tolerance: f64, curved: bool) {
    merge_coplanar_with(body, tolerance, curved, false);
}

fn merge_coplanar_with(body: &mut Body, tolerance: f64, curved: bool, always: bool) {
    let mut merged = body.clone();
    let mut changed = always;
    loop {
        let next = merged
            .edges
            .keys()
            .find(|edge| coplanar_split(&merged, *edge, tolerance, curved).is_some());
        let Some(edge) = next else { break };
        if join_faces(&mut merged, edge, tolerance, curved).is_none() {
            return;
        }
        changed = true;
    }
    if !changed {
        return;
    }
    loop {
        let next = merged.vertices.keys().find(|vertex| collinear_pair(&merged, *vertex).is_some());
        let Some(vertex) = next else { break };
        if join_edges(&mut merged, vertex).is_none() {
            return;
        }
    }
    if merged.validate().is_empty() {
        *body = merged;
    }
}

fn owner_face(body: &Body, coedge: CoedgeKey) -> Option<FaceKey> {
    Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner)
}

/// The two faces a straight edge splits one plane into, when it is the only
/// edge they share and neither carries parameter-space curves.
fn coplanar_split(
    body: &Body,
    key: EdgeKey,
    tolerance: f64,
    curved: bool,
) -> Option<[FaceKey; 2]> {
    let edge = body.edges.get(key)?;
    let straight = matches!(body.curves.get(edge.curve)?, Curve3::Line(_));
    if edge.coedges.len() != 2 || !(straight || curved) {
        return None;
    }
    let faces = [owner_face(body, edge.coedges[0])?, owner_face(body, edge.coedges[1])?];
    if faces[0] == faces[1] {
        return None;
    }
    let mut planes = [(Vec3::from([0.0; 3]), 0.0); 2];
    for (slot, face) in faces.iter().enumerate() {
        let face = body.faces.get(*face)?;
        let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
            return None;
        };
        let normal = Vec3::from(plane.normal()?) * if face.forward { 1.0 } else { -1.0 };
        planes[slot] = (normal, normal.dot(Vec3::from(plane.origin)));
    }
    if planes[0].0.dot(planes[1].0) < 1.0 - 1e-9 || (planes[0].1 - planes[1].1).abs() > tolerance {
        return None;
    }
    let edges_of = |face: FaceKey| -> Option<Vec<EdgeKey>> {
        let mut edges = Vec::new();
        for ring in &body.faces.get(face)?.loops {
            for coedge in &body.loops.get(*ring)?.coedges {
                let coedge = body.coedges.get(*coedge)?;
                if coedge.pcurve.is_some() {
                    return None;
                }
                edges.push(coedge.edge);
            }
        }
        Some(edges)
    };
    let second = edges_of(faces[1])?;
    let shared = edges_of(faces[0])?.into_iter().filter(|edge| second.contains(edge)).count();
    (shared == 1).then_some(faces)
}

/// Splices the second face's loop into the first's across their edge.
fn join_faces(body: &mut Body, key: EdgeKey, tolerance: f64, curved: bool) -> Option<()> {
    let [keep, gone] = coplanar_split(body, key, tolerance, curved)?;
    let edge = body.edges.remove(key)?;
    let (kept_use, gone_use) = if owner_face(body, edge.coedges[0])? == keep {
        (edge.coedges[0], edge.coedges[1])
    } else {
        (edge.coedges[1], edge.coedges[0])
    };
    let kept_loop = body.coedges.remove(kept_use)?.owner;
    let gone_loop = body.coedges.remove(gone_use)?.owner;
    let gone_ring = body.loops.remove(gone_loop)?.coedges;
    let at = gone_ring.iter().position(|coedge| *coedge == gone_use)?;
    let spliced = gone_ring[at + 1..].iter().chain(&gone_ring[..at]).copied().collect::<Vec<_>>();
    for coedge in &spliced {
        body.coedges.get_mut(*coedge)?.owner = kept_loop;
    }
    let ring = &mut body.loops.get_mut(kept_loop)?.coedges;
    let at = ring.iter().position(|coedge| *coedge == kept_use)?;
    ring.splice(at..=at, spliced);

    let face = body.faces.remove(gone)?;
    for ring in face.loops.iter().filter(|ring| **ring != gone_loop) {
        body.loops.get_mut(*ring)?.owner = keep;
        body.faces.get_mut(keep)?.loops.push(*ring);
    }
    body.shells.get_mut(face.owner)?.faces.retain(|other| *other != gone);
    if !body.faces.iter().any(|(_, other)| other.surface == face.surface) {
        body.surfaces.remove(face.surface);
    }
    Some(())
}

/// Two straight edges meeting in line at a vertex nothing else uses, both
/// between the same two faces.
pub(super) fn collinear_pair(body: &Body, vertex: VertexKey) -> Option<[EdgeKey; 2]> {
    let mut edges = body
        .edges
        .iter()
        .filter(|(_, edge)| edge.start == vertex || edge.end == vertex)
        .map(|(key, _)| key);
    let pair = [edges.next()?, edges.next()?];
    if edges.next().is_some() {
        return None;
    }
    let mut directions = [Vec3::from([0.0; 3]); 2];
    let mut faces = [Vec::new(), Vec::new()];
    for (slot, key) in pair.iter().enumerate() {
        let edge = body.edges.get(*key)?;
        if edge.start == edge.end
            || edge.coedges.len() != 2
            || !matches!(body.curves.get(edge.curve)?, Curve3::Line(_))
        {
            return None;
        }
        let start = Vec3::from(body.vertices.get(edge.start)?.point);
        let end = Vec3::from(body.vertices.get(edge.end)?.point);
        directions[slot] = (end - start).normalize()?;
        faces[slot] = edge
            .coedges
            .iter()
            .map(|coedge| owner_face(body, *coedge))
            .collect::<Option<Vec<_>>>()?;
        faces[slot].sort_by_key(FaceKey::slot);
    }
    (directions[0].dot(directions[1]).abs() > 1.0 - 1e-12 && faces[0] == faces[1]).then_some(pair)
}

/// Extends the first edge over the second through `vertex`.
pub(super) fn join_edges(body: &mut Body, vertex: VertexKey) -> Option<()> {
    let [keep, gone] = collinear_pair(body, vertex)?;
    let removed = body.edges.remove(gone)?;
    let far = if removed.start == vertex { removed.end } else { removed.start };
    for coedge in &removed.coedges {
        let owner = body.coedges.remove(*coedge)?.owner;
        body.loops.get_mut(owner)?.coedges.retain(|other| other != coedge);
    }
    body.vertices.remove(vertex);
    let edge = body.edges.get_mut(keep)?;
    if edge.start == vertex {
        edge.start = far;
    } else {
        edge.end = far;
    }
    let (start, end) = (edge.start, edge.end);
    let from = Vec3::from(body.vertices.get(start)?.point);
    let to = Vec3::from(body.vertices.get(end)?.point);
    let curve = body.curves.insert(Curve3::Line(Line3 {
        origin: from.to_array(),
        direction: (to - from).to_array(),
    }));
    let edge = body.edges.get_mut(keep)?;
    edge.curve = curve;
    edge.start_parameter = 0.0;
    edge.end_parameter = 1.0;
    Some(())
}

/// A whole sphere that `other` cuts, rebuilt in eight faces, each with its
/// poles and seam outside it, so the cuts it takes stay plain regions of
/// `(u, v)`. One left whole — nested, apart — keeps its single face.
fn divided_sphere(body: &Body, other: &Body, tolerance: f64) -> Option<Body> {
    let mut faces = body.face_keys();
    let face = body.faces.get(faces.next()?)?;
    if faces.next().is_some() || !face.forward {
        return None;
    }
    let surface = body.surfaces.get(face.surface)?;
    let Surface::Sphere(sphere) = surface else {
        return None;
    };
    meets(surface, other, tolerance)
        .then(|| super::make::sphere_in_octants(sphere.frame.origin, sphere.radius))?
}

/// Whether any face of `other` meets `surface` along a curve. A meeting with
/// no closed form is traced, and the trace needs the divided faces as much
/// as a circle does.
fn meets(surface: &Surface, other: &Body, tolerance: f64) -> bool {
    other.face_keys().any(|key| {
        other
            .faces
            .get(key)
            .and_then(|face| other.surfaces.get(face.surface))
            .is_some_and(|wall| {
                matches!(
                    super::intersect::surfaces(surface, wall, tolerance),
                    super::intersect::Meeting::Curves(_) | super::intersect::Meeting::Unknown
                )
            })
    })
}

/// [`divided_sphere`] for a whole ring torus: the same torus in patches
/// with no seam inside any of them (see `make::torus_in_patches`).
fn divided_torus(body: &Body, other: &Body, tolerance: f64) -> Option<Body> {
    let mut faces = body.face_keys();
    let face = body.faces.get(faces.next()?)?;
    if faces.next().is_some() || !face.forward {
        return None;
    }
    let surface = body.surfaces.get(face.surface)?;
    let Surface::Torus(torus) = surface else {
        return None;
    };
    meets(surface, other, tolerance).then(|| {
        super::make::torus_in_patches(torus.frame, torus.major_radius, torus.minor_radius)
    })?
}

/// [`divided_sphere`] for a whole right cylinder: its wall in quarters, no
/// seam inside any (see `make::cylinder_in_patches`).
fn divided_cylinder(body: &Body, other: &Body, tolerance: f64) -> Option<Body> {
    if body.faces.len() != 3 {
        return None;
    }
    let mut wall = None;
    let mut caps = 0;
    for key in body.face_keys() {
        let face = body.faces.get(key)?;
        match body.surfaces.get(face.surface)? {
            Surface::Cylinder(cylinder) if face.forward => wall = Some((key, *cylinder)),
            Surface::Plane(_) => {
                let [ring] = face.loops[..] else { return None };
                let [coedge] = body.loops.get(ring)?.coedges[..] else { return None };
                let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
                caps += matches!(body.curves.get(edge.curve), Some(Curve3::Circle(_))) as usize;
            }
            _ => return None,
        }
    }
    let (face, cylinder) = wall?;
    let normal = Vec3::from(cylinder.base.normal()?);
    let origin = Vec3::from(cylinder.base.origin);
    let heights = body
        .vertices
        .iter()
        .map(|(_, vertex)| normal.dot(Vec3::from(vertex.point) - origin));
    let (low, high) = heights.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), along| {
        (low.min(along), high.max(along))
    });
    if caps != 2 || !(high - low > tolerance) {
        return None;
    }
    let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
    let base = ocs_geom::space::Plane::orthonormal(
        (origin + normal * low).to_array(),
        cylinder.base.x_axis,
        normal.to_array(),
    )?;
    meets(surface, other, tolerance)
        .then(|| super::make::cylinder_in_patches(base, cylinder.radius, high - low))?
}

/// [`divided_sphere`] for a whole pointed cone on a flat base: the same cone
/// with its wall in quarters, no seam inside any (see
/// `make::cone_in_patches`).
fn divided_cone(body: &Body, other: &Body, tolerance: f64) -> Option<Body> {
    if body.faces.len() != 2 || body.vertices.len() != 2 {
        return None;
    }
    let mut wall = None;
    let mut rim = None;
    for key in body.face_keys() {
        let face = body.faces.get(key)?;
        match body.surfaces.get(face.surface)? {
            Surface::Cone(_) => wall = Some(key),
            Surface::Plane(_) => {
                let [ring] = face.loops[..] else { return None };
                let [coedge] = body.loops.get(ring)?.coedges[..] else { return None };
                let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
                let Some(Curve3::Circle(circle)) = body.curves.get(edge.curve) else {
                    return None;
                };
                rim = Some((*circle, edge.start));
            }
            _ => return None,
        }
    }
    let (face, (circle, on_rim)) = (wall?, rim?);
    let apex = body
        .vertices
        .iter()
        .find(|(key, _)| *key != on_rim)
        .map(|(_, vertex)| Vec3::from(vertex.point))?;
    let centre = Vec3::from(circle.plane.origin);
    let height = apex.distance(centre);
    let towards = (apex - centre).normalize()?;
    // Pointed: the apex lies on the axis, square over the rim's centre.
    let normal = Vec3::from(circle.plane.normal()?);
    if towards.cross(normal).length() > 1e-6 || height <= tolerance {
        return None;
    }
    let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
    let base = ocs_geom::space::Plane::orthonormal(
        circle.plane.origin,
        circle.plane.x_axis,
        towards.to_array(),
    )?;
    meets(surface, other, tolerance)
        .then(|| super::make::cone_in_patches(base, circle.radius, height))?
}

/// A Boolean initially sews all retained faces into one provisional shell.
/// Restore connected shells and attach inward-facing cavity shells to the
/// containing outer lump, rather than turning a void into a separate solid.
pub(super) fn regroup_shells(body: &mut Body, tolerance: f64) -> Result<(), Snag> {
    let faces = body.face_keys().collect::<Vec<_>>();
    let components = super::sweep::face_components(body, &faces).ok_or(Snag::CutRefused)?;
    if components.len() <= 1 { return Ok(()); }
    let mut pieces = Vec::new();
    for faces in &components {
        let mut piece = Body::new();
        let lump = piece.lumps.insert(Lump { shells: Vec::new(), provenance: Provenance::Synthesized });
        let shell = piece.shells.insert(Shell { faces: Vec::new(), owner: lump, provenance: Provenance::Synthesized });
        piece.lumps.get_mut(lump).ok_or(Snag::CutRefused)?.shells.push(shell);
        piece.roots.push(lump);
        for face in faces {
            copy_face_with_tolerance(&mut piece, body, *face, shell, false, tolerance)?;
        }
        let volume = match shell_sense(&piece, tolerance) {
            Some(volume) => volume,
            None => meshed_volume(&piece, tolerance)?,
        };
        if !volume.is_finite() || volume.abs() <= tolerance.powi(3) { return Err(Snag::CutRefused); }
        pieces.push((piece, volume));
    }
    let mut owners = Vec::new();
    for (index, (_, volume)) in pieces.iter().enumerate() {
        if *volume > 0.0 {
            owners.push(index);
            continue;
        }
        let point = interior_point(body, components[index][0], tolerance).ok_or(Snag::CutRefused)?;
        let owner = pieces.iter().enumerate().filter(|(_, (piece, volume))| {
            *volume > 0.0 && contains_point(piece, point, tolerance) == Containment::Inside
        }).min_by(|(_, (_, a)), (_, (_, b))| a.total_cmp(b)).map(|(index, _)| index)
            .ok_or(Snag::CutRefused)?;
        owners.push(owner);
    }
    for shell in body.shells.keys().collect::<Vec<_>>() { body.shells.remove(shell); }
    for lump in body.lumps.keys().collect::<Vec<_>>() { body.lumps.remove(lump); }
    body.roots.clear();
    let mut lumps = HashMap::new();
    for (index, (_, volume)) in pieces.iter().enumerate() {
        if *volume > 0.0 {
            let lump = body.lumps.insert(Lump { shells: Vec::new(), provenance: Provenance::Synthesized });
            body.roots.push(lump);
            lumps.insert(index, lump);
        }
    }
    // Exterior shells precede cavities even when copying retained faces put
    // an interior component earlier in the provisional face order.
    for exterior in [true, false] {
        for (index, faces) in components.iter().enumerate() {
            if (pieces[index].1 > 0.0) != exterior { continue; }
            let owner = *lumps.get(&owners[index]).ok_or(Snag::CutRefused)?;
            let shell = body.shells.insert(Shell { faces: faces.clone(), owner, provenance: Provenance::Synthesized });
            body.lumps.get_mut(owner).ok_or(Snag::CutRefused)?.shells.push(shell);
            for face in faces { body.faces.get_mut(*face).ok_or(Snag::CutRefused)?.owner = shell; }
        }
    }
    if body.validate().is_empty() { Ok(()) } else { Err(Snag::CutRefused) }
}

pub(super) fn orient_shell(body: &mut Body) -> Result<(), Snag> {
    let mut adjacent: HashMap<FaceKey, Vec<(FaceKey, bool)>> = HashMap::new();
    for (_, edge) in body.edges.iter() {
        let [first, second] = edge.coedges.as_slice() else {
            continue;
        };
        let first = body.coedges.get(*first).ok_or(Snag::CutRefused)?;
        let second = body.coedges.get(*second).ok_or(Snag::CutRefused)?;
        let first_face = body
            .loops
            .get(first.owner)
            .ok_or(Snag::CutRefused)?
            .owner;
        let second_face = body
            .loops
            .get(second.owner)
            .ok_or(Snag::CutRefused)?
            .owner;
        if first_face == second_face {
            if first.forward == second.forward {
                return Err(Snag::CutRefused);
            }
            continue;
        }
        let differ = first.forward == second.forward;
        adjacent.entry(first_face).or_default().push((second_face, differ));
        adjacent.entry(second_face).or_default().push((first_face, differ));
    }

    let mut flips = HashMap::new();
    for face in body.face_keys().collect::<Vec<_>>() {
        if flips.contains_key(&face) {
            continue;
        }
        flips.insert(face, false);
        let mut pending = VecDeque::from([face]);
        while let Some(current) = pending.pop_front() {
            let current_flip = flips[&current];
            for (other, differ) in adjacent.get(&current).into_iter().flatten() {
                let wanted = current_flip != *differ;
                match flips.get(other) {
                    Some(existing) if *existing != wanted => return Err(Snag::CutRefused),
                    Some(_) => {}
                    None => {
                        flips.insert(*other, wanted);
                        pending.push_back(*other);
                    }
                }
            }
        }
    }

    for (face, flip) in flips {
        if !flip {
            continue;
        }
        let loops = body.faces.get(face).ok_or(Snag::CutRefused)?.loops.clone();
        for ring in loops {
            let coedges = body
                .loops
                .get(ring)
                .ok_or(Snag::CutRefused)?
                .coedges
                .clone();
            for key in &coedges {
                let coedge = body.coedges.get_mut(*key).ok_or(Snag::CutRefused)?;
                coedge.forward = !coedge.forward;
                if let Some(curve) = coedge.pcurve.take() {
                    coedge.pcurve = Some(super::split::reverse_closed_pcurve(&curve).ok_or(Snag::CutRefused)?);
                }
            }
            body
                .loops
                .get_mut(ring)
                .ok_or(Snag::CutRefused)?
                .coedges
                .reverse();
        }
    }
    Ok(())
}

/// The other body's face covering the same ground as this one, if there is
/// one.
fn coincident_twin(
    body: &Body,
    other: &Body,
    face: FaceKey,
    other_boxes: &HashMap<FaceKey, Option<super::Aabb>>,
    tolerance: f64,
) -> Option<FaceKey> {
    let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
    // Faces whose boxes stay apart are no twins; asked first, it spares the
    // surface comparison against every face of the other body.
    let own = super::face_bounds(body, face).map(|bounds| bounds.grown(tolerance * 10.0));
    other.face_keys().find(|candidate| {
        own.is_none_or(|own| {
            other_boxes.get(candidate).copied().flatten().is_none_or(|theirs| own.overlaps(&theirs))
        }) && other
            .faces
            .get(*candidate)
            .and_then(|node| other.surfaces.get(node.surface))
            .is_some_and(|theirs| same_surface(surface, theirs, tolerance))
            && (super::imprint::same_ground(body, face, other, *candidate, tolerance)
                || inside_each_other(body, face, other, *candidate, tolerance))
    })
}

/// Whether each of two faces on one surface has its inside on the other.
/// After the imprint a shared wall's two copies cover the same ground, but
/// where one was cut by a file's spline and the other by the curve that
/// spline stands for, their corners sit a fit apart and are not the same
/// vertices — the ground is still the same.
fn inside_each_other(
    body: &Body,
    face: FaceKey,
    other: &Body,
    candidate: FaceKey,
    tolerance: f64,
) -> bool {
    let on = |from: &Body, face: FaceKey, to: &Body, candidate: FaceKey| {
        interior_point(from, face, tolerance).is_some_and(|point| {
            super::classify::face_distance(to, candidate, point, tolerance)
                .is_some_and(|gap| gap <= tolerance)
        })
    };
    on(body, face, other, candidate) && on(other, candidate, body, face)
}

/// Whether two surfaces are the same one, ignoring how each is parameterised.
fn same_surface(one: &Surface, other: &Surface, tolerance: f64) -> bool {
    if matches!(super::intersect_surfaces(one, other, tolerance), super::Meeting::Coincident) {
        return true;
    }
    if !matches!((one, other), (Surface::Plane(_), Surface::Plane(_))) {
        return false;
    }
    // Compared by what each says about the other's frame origin and by their
    // normals, which is enough for the planar case a shared wall is and does
    // not depend on either having picked the same u direction.
    let (Some(one_frame), Some(other_frame)) = (one.frame(), other.frame()) else {
        return false;
    };
    let (Some(first), Some(second)) = (one_frame.normal(), other_frame.normal()) else {
        return false;
    };
    let parallel = Vec3::from(first).is_parallel_to(Vec3::from(second), tolerance);
    parallel
        && one.distance_to(other_frame.origin).abs() <= tolerance
        && other.distance_to(one_frame.origin).abs() <= tolerance
}

/// The face of `other` on the same surface as `face` that `face`'s interior
/// lies on, if any.
fn wall_under(body: &Body, face: FaceKey, other: &Body, tolerance: f64) -> Option<FaceKey> {
    let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
    let point = interior_point(body, face, tolerance)?;
    other.face_keys().find(|candidate| {
        other
            .faces
            .get(*candidate)
            .and_then(|node| other.surfaces.get(node.surface))
            .is_some_and(|theirs| same_surface(surface, theirs, tolerance))
            && super::classify::face_distance(other, *candidate, point, tolerance)
                .is_some_and(|gap| gap <= tolerance)
    })
}

/// Whether this side's copy of a shared wall survives.
///
/// Two faces covering the same ground are both on the boundary of the result
/// or neither is, and which it is comes from their normals once the
/// operation's own flip has been applied:
///
/// - facing the **same** way, the wall is real and exactly one copy is kept —
///   the first body's, arbitrarily but consistently
/// - facing **opposite** ways, the two solids are on either side of it, so it
///   is interior and both copies go
///
/// A difference flips the second solid, which turns two boxes stacked face to
/// face from opposite into same — and that is what leaves the first one whole
/// instead of open at the top.
fn keeps_shared_wall(
    body: &Body,
    face: FaceKey,
    other: &Body,
    twin: FaceKey,
    [flip_mine, flip_theirs]: [bool; 2],
    is_first: bool,
) -> bool {
    let Some(point) = interior_point(body, face, 1e-9) else { return false; };
    let outward = |body: &Body, face: FaceKey, flipped: bool| -> Option<Vec3> {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        let (u, v) = surface.parameters_at(point)?;
        let normal = Vec3::from(surface.normal_at(u, v)?);
        Some(if node.forward != flipped { normal } else { -normal })
    };
    // Each face is flipped only if the operation turns its solid around.
    let (Some(mine), Some(theirs)) = (
        outward(body, face, flip_mine),
        outward(other, twin, flip_theirs),
    ) else {
        return false;
    };
    mine.dot(theirs) > 0.0 && is_first
}

/// Whether a face is inside the other solid, outside it, or on its surface.
///
/// Asked of a point in the face's interior rather than of a vertex: every
/// vertex of an imprinted face sits on the seam, where the answer is
/// "boundary" for reasons that have nothing to do with which side the face is
/// on.
pub(super) fn face_side(body: &Body, other: &Body, face: FaceKey, tolerance: f64) -> Containment {
    let side = match interior_point(body, face, tolerance) {
        Some(point) => contains_point(other, point, tolerance),
        None => Containment::Unknown,
    };
    if !matches!(side, Containment::OnBoundary | Containment::Unknown) {
        return side;
    }
    // The point found may sit on a cut the other body made across the face —
    // a strip's middle on the curve dividing it from the next — or where the
    // other body only touches it, a ball resting on the face's middle, where
    // no ray settles anything. Another point of the face, clear of the other
    // body's boundary, settles which side it is; a face lying on that
    // boundary throughout has none.
    let clear = |point: [f64; 3]| {
        !matches!(
            contains_point(other, point, tolerance),
            Containment::OnBoundary | Containment::Unknown
        )
    };
    interior_point_where(body, face, tolerance, &clear)
        .map_or(side, |point| contains_point(other, point, tolerance))
}

/// Which side of `other` a face lies on, read where it meets `other` rather
/// than by casting rays through every face of it.
///
/// After the imprint a face that touches `other` does so along edges lying on
/// `other`'s faces. At an edge's middle, where it lies on just one face of
/// `other` — that face smooth there, the solid behind it a half-space to
/// first order — the direction leading from the edge into the face, square
/// to the edge, says the side: against the other face's outward normal is
/// inside it, along it outside. A direction within ten degrees of the other
/// face's tangent plane — a tangency, a coincident wall — settles nothing,
/// and neither does a middle on an edge of `other`, where the solid is a
/// wedge. Every edge that settles it has to agree; `None` where none does,
/// or two disagree, and the rays are asked after all.
///
/// Ported from Solidcore's radial-sector classification at a shared edge
/// (`query/edge.rs`, `radial_sector_classify_between`, driven by
/// `boolean/merge/p4.rs`), reduced to the smooth sector. Apache-2.0, see
/// THIRD_PARTY.md.
fn local_side(body: &Body, other: &Body, face: FaceKey, tolerance: f64) -> Option<Containment> {
    // sin 10°: nearer the tangent plane than this, first order says nothing.
    const CLEAR: f64 = 0.173_648_177_666_930_35;
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    let sense = if node.forward { 1.0 } else { -1.0 };
    let mut boundary: Option<Option<Vec<Curve>>> = None;
    let mut found = None;
    for coedge in body.face_coedges(face) {
        let Some(edge) = body.coedges.get(coedge).and_then(|use_| body.edges.get(use_.edge))
        else {
            continue;
        };
        let Some(curve) = body.curves.get(edge.curve) else { continue };
        let middle = 0.5 * (edge.start_parameter + edge.end_parameter);
        let point = curve.point_at(middle);
        // On exactly one face of `other`, clear of its edges.
        let near = |key: FaceKey| {
            super::classify::face_distance(other, key, point, tolerance * 10.0)
                .is_some_and(|gap| gap <= tolerance * 10.0)
        };
        let mut on = other.face_keys().filter(|key| near(*key));
        let (Some(wall), None) = (on.next(), on.next()) else { continue };
        let wall_node = other.faces.get(wall)?;
        let wall_surface = other.surfaces.get(wall_node.surface)?;
        let Some(outward) = wall_surface
            .parameters_at(point)
            .and_then(|(u, v)| wall_surface.normal_at(u, v))
            .and_then(|normal| Vec3::from(normal).normalize())
            .map(|normal| if wall_node.forward { normal } else { -normal })
        else {
            continue;
        };
        let (Some(along), Some(normal)) = (
            Vec3::from(curve.tangent_at(middle)).normalize(),
            surface
                .parameters_at(point)
                .and_then(|(u, v)| surface.normal_at(u, v))
                .and_then(|normal| Vec3::from(normal).normalize()),
        ) else {
            continue;
        };
        // Square to the edge in the face's tangent plane; which way leads
        // into the face is asked of the face's own boundary a short step off.
        let Some(across) = (normal * sense).cross(along).normalize() else { continue };
        let lean = across.dot(outward);
        if lean.abs() < CLEAR {
            continue;
        }
        let boundary = boundary
            .get_or_insert_with(|| pcurve::face_boundary(body, face, tolerance))
            .as_ref()?;
        let length = Vec3::from(curve.point_at(edge.start_parameter))
            .distance(Vec3::from(curve.point_at(edge.end_parameter)))
            .max(Vec3::from(point).distance(Vec3::from(curve.point_at(edge.start_parameter))));
        let step = (length * 1e-3).max(tolerance * 100.0);
        let inside = |direction: Vec3| {
            surface.parameters_at((Vec3::from(point) + direction * step).to_array()).is_some_and(
                |(u, v)| {
                    pcurve::contains_parameter_facing(
                        surface,
                        boundary,
                        [u, v],
                        ocs_geom::geom2d::Tolerance::new(tolerance),
                        node.forward,
                    )
                },
            )
        };
        let into = match (inside(across), inside(-across)) {
            (true, false) => 1.0,
            (false, true) => -1.0,
            _ => continue,
        };
        let side = if lean * into < 0.0 { Containment::Inside } else { Containment::Outside };
        match found {
            None => found = Some(side),
            Some(seen) if seen != side => return None,
            Some(_) => {}
        }
    }
    found
}

/// A point strictly inside a face, in space.
///
/// Found in the surface's parameter space, where "inside the boundary" is a
/// question [`geom2d`](ocs_geom::geom2d) already answers. The average of the
/// boundary's own points is inside a convex face and can fall outside a
/// concave one, so it is checked rather than assumed, and the midpoints
/// between it and each boundary point are tried after it.
pub(super) fn interior_point(body: &Body, face: FaceKey, tolerance: f64) -> Option<[f64; 3]> {
    interior_point_where(body, face, tolerance, &|_| true)
}

/// [`interior_point`], among the points `accept` takes.
fn interior_point_where(
    body: &Body,
    face: FaceKey,
    tolerance: f64,
    accept: &dyn Fn([f64; 3]) -> bool,
) -> Option<[f64; 3]> {
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    if let Surface::Sphere(sphere) = surface {
        if node.loops.len() == 1 {
            let ring = body.loops.get(node.loops[0])?;
            if ring.coedges.len() == 1 {
                let coedge = body.coedges.get(ring.coedges[0])?;
                let edge = body.edges.get(coedge.edge)?;
                if edge.start == edge.end {
                    if let Some(super::geometry::Curve3::Circle(circle)) =
                        body.curves.get(edge.curve)
                    {
                        let normal = Vec3::from(circle.plane.normal()?);
                        let sign = if coedge.forward == node.forward { 1.0 } else { -1.0 };
                        return Some(
                            (Vec3::from(sphere.frame.origin) + normal * sphere.radius * sign)
                                .to_array(),
                        );
                    }
                }
            }
        }
    }
    if super::classify::whole_closed_face(body, face) {
        return whole_face_point(body, face, surface, tolerance);
    }
    let boundary = pcurve::face_boundary(body, face, tolerance)?;
    // Up to a cone's apex, too: a cap's rim alone puts every sample on it.
    let boundary = pcurve::apex_closed(surface, &boundary, ocs_geom::geom2d::Tolerance::new(tolerance))
        .unwrap_or(boundary);
    let samples: Vec<[f64; 2]> = boundary
        .iter()
        .flat_map(|curve| {
            (0..4).map(move |step| curve.point_at(step as f64 / 4.0))
        })
        .collect();
    if samples.is_empty() {
        return None;
    }
    let middle = samples.iter().fold([0.0f64; 2], |sum, point| {
        [sum[0] + point[0], sum[1] + point[1]]
    });
    let count = samples.len() as f64;
    let centre = [middle[0] / count, middle[1] / count];

    let usable = |candidate: [f64; 2]| {
        let on_edge = boundary
            .iter()
            .any(|edge| ocs_geom::geom2d::distance_to(edge, candidate) <= tolerance);
        // Asked of the surface, not of the plane: a band's rims and a cone's
        // apex line are no closed polygon, and a plain polygon test over them
        // could put a point of one half of a cut band into the other.
        (!on_edge
            && pcurve::contains_parameter_facing(
                surface,
                &boundary,
                candidate,
                ocs_geom::geom2d::Tolerance::new(tolerance),
                node.forward,
            )
            && accept(surface.point_at(candidate[0], candidate[1])))
        .then_some(candidate)
    };
    let chosen = usable(centre).or_else(|| {
        samples.iter().find_map(|point| {
            [0.5, 0.75, 0.875, 0.9375, 0.25, 0.125].into_iter().find_map(|weight| {
                usable([
                    centre[0] + (point[0] - centre[0]) * weight,
                    centre[1] + (point[1] - centre[1]) * weight,
                ])
            })
        })
    });
    // A thin ring — an annulus cut just inside one rim — has its centroid in
    // the hole and every point between the two there too. Halfway between two
    // boundary points, one on each rim, lands in it (#1563).
    let chosen = chosen.or_else(|| {
        samples.iter().enumerate().find_map(|(index, first)| {
            samples[index + 1..].iter().find_map(|second| {
                usable([0.5 * (first[0] + second[0]), 0.5 * (first[1] + second[1])])
            })
        })
    });
    if let Some(chosen) = chosen {
        return Some(surface.point_at(chosen[0], chosen[1]));
    }
    pcurve::periodic_band_point(surface, &boundary, tolerance)
        .or_else(|| sliver_point(surface, &boundary, centre))
}

/// A point standing for a hairline face — two boundary curves a file's fit
/// tolerance apart, with no inside a point could be put in clear of both.
/// The middle of its boundary is on the face in every sense that matters for
/// which side of another solid it lies on. `None` for a face any wider.
fn sliver_point(
    surface: &Surface,
    boundary: &[ocs_geom::geom2d::Curve],
    centre: [f64; 2],
) -> Option<[f64; 3]> {
    const STEPS: usize = 256;
    let walks: Vec<Vec<Vec3>> = boundary
        .iter()
        .map(|curve| {
            (0..=STEPS)
                .map(|step| {
                    let [u, v] = curve.point_at(step as f64 / STEPS as f64);
                    Vec3::from(surface.point_at(u, v))
                })
                .collect()
        })
        .collect();
    let first = *walks.first()?.first()?;
    let extent = walks
        .iter()
        .flatten()
        .map(|point| point.distance(first))
        .fold(0.0, f64::max);
    let point = Vec3::from(surface.point_at(centre[0], centre[1]));
    let gap = walks
        .iter()
        .flat_map(|walk| {
            walk.windows(2)
                .map(|pair| point.distance_to_segment(pair[0], pair[1]))
        })
        .fold(f64::INFINITY, f64::min);
    (gap <= extent * 1e-3).then_some(point.to_array())
}

/// A point on a face that is its whole sphere or torus, clear of the slit
/// edges it may carry.
fn whole_face_point(
    body: &Body,
    face: FaceKey,
    surface: &Surface,
    tolerance: f64,
) -> Option<[f64; 3]> {
    let edges: Vec<[f64; 3]> = body
        .faces
        .get(face)?
        .loops
        .iter()
        .filter_map(|ring| body.loops.get(*ring))
        .flat_map(|ring| ring.coedges.iter())
        .filter_map(|coedge| body.coedges.get(*coedge))
        .filter_map(|coedge| body.edges.get(coedge.edge))
        .filter_map(|edge| {
            let curve = body.curves.get(edge.curve)?;
            Some((0..=8).map(move |step| {
                curve.point_at(
                    edge.start_parameter
                        + (edge.end_parameter - edge.start_parameter) * step as f64 / 8.0,
                )
            }))
        })
        .flatten()
        .collect();
    let size = edges.first().map_or(1.0, |first| {
        edges
            .iter()
            .map(|point| Vec3::from(*point).distance(Vec3::from(*first)))
            .fold(0.0, f64::max)
    });
    (0..8)
        .flat_map(|around| (1..4).map(move |across| (around, across)))
        .map(|(around, across)| {
            surface.point_at(
                std::f64::consts::TAU * (around as f64 + 0.5) / 8.0,
                std::f64::consts::PI * (across as f64 / 4.0 - 0.5),
            )
        })
        .find(|point| {
            edges.iter().all(|edge| {
                Vec3::from(*edge).distance(Vec3::from(*point)) > size * 0.05 + tolerance
            })
        })
}

/// Copies a face and everything bounding it into the result.
///
/// Geometry is copied rather than shared: the two inputs have their own
/// arenas, and a key from one means nothing in the other.
pub(super) fn copy_face(
    result: &mut Body,
    source: &Body,
    face: FaceKey,
    shell: super::topology::ShellKey,
    flip: bool,
) -> Result<(), Snag> {
    let tolerance = super::operation_tolerance(&[result, source]);
    copy_face_with_tolerance(result, source, face, shell, flip, tolerance)
}

pub(super) fn copy_face_with_tolerance(result: &mut Body, source: &Body, face: FaceKey, shell: super::ShellKey, flip: bool, tolerance: f64) -> Result<(), Snag> {
    let node = source.faces.get(face).ok_or(Snag::CutRefused)?;
    let surface = source
        .surfaces
        .get(node.surface)
        .ok_or(Snag::CutRefused)?
        .clone();
    let surface = result.surfaces.insert(surface);
    let new_face = result.faces.insert(Face {
        surface,
        // A difference turns the second solid's faces into the walls of the
        // cavity it leaves, which face the other way.
        forward: node.forward != flip,
        loops: Vec::new(),
        owner: shell,
        provenance: Provenance::Synthesized,
    });
    let mut rings = Vec::new();
    for ring in &node.loops {
        let source_ring = source.loops.get(*ring).ok_or(Snag::CutRefused)?;
        let new_ring = result.loops.insert(super::topology::Loop {
            coedges: Vec::new(),
            owner: new_face,
            provenance: Provenance::Synthesized,
        });
        let mut coedges = Vec::new();
        for coedge in &source_ring.coedges {
            let source_coedge = source.coedges.get(*coedge).ok_or(Snag::CutRefused)?;
            let source_edge = source
                .edges
                .get(source_coedge.edge)
                .ok_or(Snag::CutRefused)?;
            let curve = source
                .curves
                .get(source_edge.curve)
                .ok_or(Snag::CutRefused)?
                .clone();
            let start = copy_vertex(result, source, source_edge.start, tolerance)?;
            let end = copy_vertex(result, source, source_edge.end, tolerance)?;
            // A short split can collapse when its two vertices are sewn
            // within tolerance — a straight one, or a stretch of spline
            // between two cuts a fit apart. Keeping a zero-length coedge
            // leaves an otherwise valid patch impossible to triangulate.
            // Closed circles still require their single seam vertex.
            if start == end
                && (matches!(curve, super::Curve3::Line(_))
                    || source_edge.start != source_edge.end)
            {
                continue;
            }
            let middle = curve.point_at(
                0.5 * (source_edge.start_parameter + source_edge.end_parameter),
            );
            // Reuse rather than copy where the same edge is already there.
            // This is the stitching: two kept faces that met along a seam in
            // their own solid meet along it here only if their copies share
            // the edge, and without that the result is a shell of loose
            // faces that still passes every local check.
            let here = Vec3::from(curve.tangent_at(
                0.5 * (source_edge.start_parameter + source_edge.end_parameter),
            ));
            let span = (source_edge.start_parameter, source_edge.end_parameter);
            let edge = match find_edge(result, start, end, middle, (&curve, span), tolerance) {
                Some(existing) => existing,
                None => {
                    let curve = result.curves.insert(curve);
                    result.edges.insert(super::topology::Edge {
                        curve,
                        start_parameter: source_edge.start_parameter,
                        end_parameter: source_edge.end_parameter,
                        start,
                        end,
                        coedges: Vec::new(),
                        provenance: Provenance::Synthesized,
                    })
                }
            };
            // A closed edge starts and ends at one vertex, so which way the
            // reused one runs is read off its curve: a circle shared by two
            // solids may be kept by each with its plane facing the other way.
            let reversed = result.edges.get(edge).is_some_and(|node| {
                if node.start != node.end {
                    return node.start != start;
                }
                result.curves.get(node.curve).is_some_and(|existing| {
                    let there = Vec3::from(existing.tangent_at(existing.parameter_at(middle)));
                    here.dot(there) < 0.0
                })
            });
            let reverse_pcurve = flip != reversed;
            let pcurve = match (&source_coedge.pcurve, reverse_pcurve) {
                (Some(curve), true) => Some(super::split::reverse_closed_pcurve(curve).ok_or(Snag::CutRefused)?),
                (Some(curve), false) => Some(curve.clone()),
                (None, _) => None,
            };
            let new_coedge = result.coedges.insert(super::topology::Coedge {
                edge,
                // Flipping a face reverses the way its boundary runs, or the
                // loop would wind the wrong way round the new outward normal.
                // A reused edge may also run the other way than the one this
                // coedge came from, which the sense has to absorb.
                forward: (source_coedge.forward != flip) != reversed,
                pcurve,
                owner: new_ring,
                provenance: Provenance::Synthesized,
            });
            result
                .edges
                .get_mut(edge)
                .ok_or(Snag::CutRefused)?
                .coedges
                .push(new_coedge);
            coedges.push(new_coedge);
        }
        if flip {
            // The loop is traversed the other way as well as each coedge.
            coedges.reverse();
        }
        result
            .loops
            .get_mut(new_ring)
            .ok_or(Snag::CutRefused)?
            .coedges = coedges;
        rings.push(new_ring);
    }
    result
        .faces
        .get_mut(new_face)
        .ok_or(Snag::CutRefused)?
        .loops = rings;
    result
        .shells
        .get_mut(shell)
        .ok_or(Snag::CutRefused)?
        .faces
        .push(new_face);
    Ok(())
}

/// An edge already joining the two vertices along the same path, if there is
/// one.
///
/// Endpoints alone are not enough — two arcs can join the same pair of
/// vertices going opposite ways round — so the middle is compared too.
fn find_edge(
    result: &Body,
    start: super::topology::VertexKey,
    end: super::topology::VertexKey,
    middle: [f64; 3],
    (curve, span): (&super::Curve3, (f64, f64)),
    tolerance: f64,
) -> Option<super::topology::EdgeKey> {
    result
        .edges
        .iter()
        .find(|(key, edge)| {
            let ends_match = (edge.start == start && edge.end == end)
                || (edge.start == end && edge.end == start);
            if !ends_match {
                return false;
            }
            // One stretch of a spline between two corners is one edge,
            // though each solid's cut ended it a fit apart along the spline.
            // A closed spline has two stretches between the same corners;
            // the spans tell them apart.
            let within = |value: f64, (from, to): (f64, f64)| {
                value >= from.min(to) && value <= from.max(to)
            };
            let same_spline = start != end
                && matches!(curve, super::Curve3::Nurbs(_))
                && result.curves.get(edge.curve) == Some(curve)
                && within(0.5 * (span.0 + span.1), (edge.start_parameter, edge.end_parameter))
                && within(0.5 * (edge.start_parameter + edge.end_parameter), span);
            let same_middle = result
                .edge_endpoints(*key)
                .is_some()
                .then(|| {
                    result.curves.get(edge.curve).map(|curve| {
                        curve.point_at(0.5 * (edge.start_parameter + edge.end_parameter))
                    })
                })
                .flatten()
                .is_some_and(|point| Vec3::from(point).distance(Vec3::from(middle)) <= tolerance);
            // A closed edge has no middle to compare: the same loop written
            // as two different curves starts its parameters anywhere round
            // it. It is the same edge where the one passes through the
            // other's middle.
            let same_loop = start == end
                && edge.start == edge.end
                && result.curves.get(edge.curve).is_some_and(|existing| {
                    let on = existing.point_at(existing.parameter_at(middle));
                    Vec3::from(on).distance(Vec3::from(middle)) <= tolerance
                });
            same_spline || same_middle || same_loop
        })
        .map(|(key, _)| key)
}

/// Copies a vertex, reusing one already at the same place.
///
/// Reuse is what stitches the result: two faces that met along a seam in
/// their own solids meet along it here only if their copies share vertices.
fn copy_vertex(
    result: &mut Body,
    source: &Body,
    vertex: super::topology::VertexKey,
    tolerance: f64,
) -> Result<super::topology::VertexKey, Snag> {
    let point = source.vertices.get(vertex).ok_or(Snag::CutRefused)?.point;
    let existing = result
        .vertices
        .iter()
        .find(|(_, node)| Vec3::from(node.point).distance(Vec3::from(point)) <= tolerance)
        .map(|(key, _)| key);
    Ok(match existing {
        Some(key) => key,
        None => result.vertices.insert(super::topology::Vertex {
            point,
            provenance: Provenance::Synthesized,
        }),
    })
}

/// The boundary curves of a face, for a caller inspecting a result.
pub fn boundary_of(body: &Body, face: FaceKey, tolerance: f64) -> Option<Vec<Curve>> {
    pcurve::face_boundary(body, face, tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounds::body_bounds;
    use crate::make::{cuboid, cylinder};

    const TOL: f64 = 1e-9;

    /// Two ten-unit boxes overlapping in a five-unit corner.
    fn pair() -> (Body, Body) {
        (
            cuboid([0.0; 3], [10.0, 10.0, 10.0]).unwrap(),
            cuboid([5.0; 3], [10.0, 10.0, 10.0]).unwrap(),
        )
    }

    #[test]
    fn box_cylinder_booleans_are_closed_solids() {
        let cases = [
            ([10.0, 5.0, 0.0], 10.0),
            ([10.0, 10.0, 0.0], 10.0),
            ([10.0, 5.0, -2.0], 14.0),
            ([9.0, 5.0, -2.0], 14.0),
        ];
        for (base, height) in cases {
            for operation in [
                Operation::Union,
                Operation::Difference,
                Operation::Intersection,
            ] {
                let box_body = cuboid([0.0; 3], [10.0; 3]).unwrap();
                let round = cylinder(base, 3.0, height).unwrap();
                let result = combine(box_body, round, operation, TOL).unwrap();
                assert!(result.validate().is_empty(), "{base:?} {operation:?}");
                assert!(result.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
            }
        }
    }

    #[test]
    fn a_door_cut_retries_rays_that_graze_the_wall_bottom() {
        use ocs_geom::geom2d::{Curve, Line};
        let prism = |points: &[[f64; 2]]| {
            let profile = (0..points.len()).map(|index| Curve::Line(Line {
                start: points[index], end: points[(index + 1) % points.len()],
            })).collect::<Vec<_>>();
            crate::extrude(ocs_geom::space::Plane::XY, &profile, [0.0, 0.0, 3000.0]).unwrap()
        };
        let outer = prism(&[
            [0.0, 2350.608306267448], [0.0, 6299.998941667038],
            [18119.99661333453, 6299.998941667038],
            [18119.99661333453, -299.99964722234654],
            [2650.609364600408, -299.99964722234654],
        ]);
        let inner = prism(&[
            [299.99964722234614, 2474.872823655858],
            [299.99964722234614, 5999.998941667038],
            [17819.995555001562, 5999.998941667038],
            [17819.995555001562, 0.0], [2774.8735292111646, 0.0],
        ]);
        let tolerance = crate::operation_tolerance(&[&outer, &inner]);
        let wall = combine(outer, inner, Operation::Difference, tolerance).unwrap();
        assert_eq!(crate::contains_point(&wall,
            [6159.999129815122, -349.99982361117327, -50.0], tolerance),
            Containment::Outside);
        let cutter = cuboid([6159.999129815122, -400.0, -100.0], [1200.0, 500.0, 2200.0]).unwrap();
        let expected = volume(&wall) - 1200.0 * 299.99964722234654 * 2100.0;
        let result = combine(wall, cutter, Operation::Difference, tolerance).unwrap();
        assert!(result.validate().is_empty());
        assert!(result.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
        assert!((volume(&result) - expected).abs() < 1e-3);
    }

    #[test]
    fn a_union_reaches_across_both() {
        let (a, b) = pair();
        let result = combine(a, b, Operation::Union, TOL).expect("two boxes union");
        let bounds = body_bounds(&result).expect("a planar result is bounded");
        assert_eq!(bounds.min, [0.0; 3]);
        assert_eq!(bounds.max, [15.0; 3]);
    }

    #[test]
    fn an_intersection_is_only_the_overlap() {
        let (a, b) = pair();
        let result = combine(a, b, Operation::Intersection, TOL).expect("two boxes intersect");
        let bounds = body_bounds(&result).unwrap();
        assert_eq!(bounds.min, [5.0; 3]);
        assert_eq!(bounds.max, [10.0; 3]);
    }

    #[test]
    fn a_difference_keeps_the_first_and_hollows_the_second_out_of_it() {
        let (a, b) = pair();
        let result = combine(a, b, Operation::Difference, TOL).expect("two boxes differ");
        let bounds = body_bounds(&result).unwrap();
        // The outer extent is the first box's; the second only removes.
        assert_eq!(bounds.min, [0.0; 3]);
        assert_eq!(bounds.max, [10.0; 3]);
    }

    #[test]
    fn a_difference_keeps_more_faces_than_the_original_had() {
        // The corner bitten out adds walls.
        let (a, b) = pair();
        let result = combine(a, b, Operation::Difference, TOL).unwrap();
        assert!(result.faces.len() > 6, "{}", result.faces.len());
    }

    #[test]
    fn solids_that_do_not_touch_union_to_both_and_intersect_to_nothing() {
        let a = cuboid([0.0; 3], [1.0; 3]).unwrap();
        let b = cuboid([50.0; 3], [1.0; 3]).unwrap();
        let joined = combine(a.clone(), b.clone(), Operation::Union, TOL).unwrap();
        assert_eq!(joined.faces.len(), 12, "both boxes, whole");
        let shared = combine(a, b, Operation::Intersection, TOL).unwrap();
        assert_eq!(shared.faces.len(), 0, "nothing is in both");
    }

    #[test]
    fn a_difference_by_something_far_away_leaves_the_original_alone() {
        let a = cuboid([0.0; 3], [4.0; 3]).unwrap();
        let b = cuboid([50.0; 3], [1.0; 3]).unwrap();
        let result = combine(a, b, Operation::Difference, TOL).unwrap();
        assert_eq!(result.faces.len(), 6);
        let bounds = body_bounds(&result).unwrap();
        assert_eq!(bounds.max, [4.0; 3]);
    }

    /// Two boxes stacked face to face, the shared wall covering exactly the
    /// same ground on both.
    fn stacked() -> (Body, Body) {
        (
            cuboid([0.0; 3], [10.0; 3]).unwrap(),
            cuboid([0.0, 0.0, 10.0], [10.0; 3]).unwrap(),
        )
    }

    #[test]
    fn stacked_boxes_union_into_one_solid_with_no_wall_between() {
        let (a, b) = stacked();
        let result = combine(a, b, Operation::Union, TOL).expect("a stack unions");
        let bounds = body_bounds(&result).unwrap();
        assert_eq!(bounds.min, [0.0; 3]);
        assert_eq!(bounds.max, [10.0, 10.0, 20.0]);
        // The two that met are gone, and the sides join across the seam.
        assert_eq!(result.faces.len(), 6);
        assert!(result.validate().is_empty());
        assert_eq!(result.euler_characteristic(), 2);
    }

    #[test]
    fn a_stack_differenced_leaves_the_first_box_whole() {
        // The flip is what does it: the second solid's bottom turns to face
        // up, matching the first's top, so the wall is real and one copy
        // stays. Without it the result would be open at the top.
        let (a, b) = stacked();
        let result = combine(a, b, Operation::Difference, TOL).expect("a stack differs");
        assert_eq!(result.faces.len(), 6, "the first box, untouched");
        let bounds = body_bounds(&result).unwrap();
        assert_eq!(bounds.max, [10.0; 3]);
        assert!(result.validate().is_empty());
    }

    #[test]
    fn a_stack_intersects_to_nothing() {
        // They share a face and no volume.
        let (a, b) = stacked();
        let result = combine(a, b, Operation::Intersection, TOL).expect("a stack intersects");
        assert_eq!(result.faces.len(), 0);
    }

    #[test]
    fn two_identical_solids_union_to_one_of_them() {
        let a = cuboid([0.0; 3], [4.0; 3]).unwrap();
        let b = cuboid([0.0; 3], [4.0; 3]).unwrap();
        let result = combine(a, b, Operation::Union, TOL).expect("identical boxes union");
        assert_eq!(result.faces.len(), 6, "one box, not two");
        assert!(result.validate().is_empty());
        assert_eq!(result.euler_characteristic(), 2);
    }

    #[test]
    fn a_solid_differenced_by_itself_leaves_nothing() {
        let a = cuboid([0.0; 3], [4.0; 3]).unwrap();
        let b = cuboid([0.0; 3], [4.0; 3]).unwrap();
        let result = combine(a, b, Operation::Difference, TOL).expect("a box minus itself");
        assert_eq!(result.faces.len(), 0);
    }

    /// How much a body encloses once meshed. Positive when it is the right
    /// way out; a shared wall left in twice, or taken out twice, changes it.
    fn volume(solid: &Body) -> f64 {
        let mesh = super::super::mesh::body(
            solid,
            ocs_geom::tessellation::DEFAULT_ANGLE,
            1e-9,
        );
        mesh.triangles
            .iter()
            .map(|triangle| {
                let at = |index: usize| {
                    ocs_geom::space::Vec3::from(mesh.positions[triangle[index]])
                };
                at(0).cross(at(1)).dot(at(2)) / 6.0
            })
            .sum()
    }

    #[test]
    fn a_sphere_and_a_cylinder_off_its_centre_combine() {
        // Their walls meet in curves with no closed form: two loops round the
        // cylinder through the sphere, or one where it only grazes the side.
        for (axis, radius) in [([3.0, 0.0], 4.0), ([8.0, 0.0], 4.0)] {
            let sphere = crate::make::sphere([0.0; 3], 10.0).unwrap();
            let rod = cylinder([axis[0], axis[1], -20.0], radius, 40.0).unwrap();
            let tolerance = crate::operation_tolerance(&[&sphere, &rod]);
            let both = (volume(&sphere), volume(&rod));
            let joined = combine(sphere.clone(), rod.clone(), Operation::Union, tolerance).unwrap();
            let shared =
                combine(sphere.clone(), rod.clone(), Operation::Intersection, tolerance).unwrap();
            let drilled = combine(sphere, rod, Operation::Difference, tolerance).unwrap();
            for result in [&joined, &shared, &drilled] {
                assert!(result.validate().is_empty());
            }
            let (joined, shared, drilled) = (volume(&joined), volume(&shared), volume(&drilled));
            // Within the meshing; a wall lost or kept twice is far more.
            assert!(shared > 0.0);
            assert!((joined + shared - both.0 - both.1).abs() < both.0 * 1e-2);
            assert!((drilled + shared - both.0).abs() < both.0 * 1e-2);
        }
    }

    #[test]
    fn a_wall_shared_in_part_is_resolved_rather_than_refused() {
        // A small box standing on a big one. They meet over a corner of the
        // big one's top face — the shared plane holds all of one region and
        // part of the other — and the imprint now cuts the larger face where
        // the sharing stops, so each piece is wholly shared or wholly not.
        //
        // The volume is the check: a doubled wall or a missing one both leave
        // something that looks finished.
        let a = cuboid([0.0; 3], [10.0; 3]).unwrap();
        let b = cuboid([0.0, 0.0, 10.0], [4.0, 4.0, 4.0]).unwrap();
        let result = combine(a, b, Operation::Union, TOL).expect("a stacked union");
        assert!(result.validate().is_empty());
        assert!(
            (volume(&result) - (1000.0 + 64.0)).abs() < 1e-6,
            "{}",
            volume(&result)
        );
    }

    #[test]
    fn a_box_standing_on_another_can_be_cut_back_out_of_it() {
        // The same pair the other way about. What the difference leaves is
        // the big box untouched — the small one sits on top of it rather
        // than in it — which only comes out right if the shared wall is
        // resolved rather than doubled.
        let a = cuboid([0.0; 3], [10.0; 3]).unwrap();
        let b = cuboid([0.0, 0.0, 10.0], [4.0, 4.0, 4.0]).unwrap();
        let result = combine(a, b, Operation::Difference, TOL).expect("a stacked difference");
        assert!(result.validate().is_empty());
        assert!((volume(&result) - 1000.0).abs() < 1e-6, "{}", volume(&result));
    }

    #[test]
    fn every_face_kept_is_on_the_side_the_operation_asked_for() {
        // Read back from the result rather than trusted: each face's own
        // interior point, tested against the solid it was cut against.
        let (a, b) = pair();
        let original_b = b.clone();
        let result = combine(a, b, Operation::Union, TOL).unwrap();
        for face in result.face_keys() {
            let Some(point) = interior_point(&result, face, 1e-9) else {
                continue;
            };
            assert_ne!(
                contains_point(&original_b, point, 1e-6),
                Containment::Inside,
                "a union kept a face inside the other solid"
            );
        }
    }

    #[test]
    fn a_union_at_survey_coordinates_reaches_the_same_way() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let a = cuboid(origin, [10.0; 3]).unwrap();
        let b = cuboid(
            [origin[0] + 5.0, origin[1] + 5.0, origin[2] + 5.0],
            [10.0; 3],
        )
        .unwrap();
        let result = combine(a, b, Operation::Union, 1e-6).expect("the same union, further out");
        let bounds = body_bounds(&result).unwrap();
        assert!((bounds.max[0] - (origin[0] + 15.0)).abs() < 1e-6, "{bounds:?}");
    }

    #[test]
    fn the_result_carries_its_own_geometry() {
        // Keys from one body mean nothing in another, so every surface,
        // curve and vertex is copied rather than referred to.
        let (a, b) = pair();
        let result = combine(a, b, Operation::Union, TOL).unwrap();
        assert!(result.surfaces.len() >= result.faces.len());
        assert!(!result.curves.is_empty());
        for (_, face) in result.faces.iter() {
            assert!(result.surfaces.contains(face.surface));
        }
        for (_, edge) in result.edges.iter() {
            assert!(result.curves.contains(edge.curve));
            assert!(result.vertices.contains(edge.start));
            assert!(result.vertices.contains(edge.end));
        }
    }

    #[test]
    fn a_union_comes_out_a_closed_solid() {
        // The check that the faces were stitched rather than merely
        // gathered. Copied face by face, an edge shared by two of them
        // becomes two edges with one coedge apiece: every local check still
        // passes and the result is a bag of loose walls.
        let (a, b) = pair();
        let result = combine(a, b, Operation::Union, TOL).unwrap();
        let flaws = result.validate();
        assert!(flaws.is_empty(), "{flaws:?}");
        for (key, edge) in result.edges.iter() {
            assert_eq!(edge.coedges.len(), 2, "edge {key:?} is not shared");
        }
        assert_eq!(result.euler_characteristic(), 2, "a closed surface");
    }

    #[test]
    fn an_intersection_comes_out_a_closed_solid_too() {
        let (a, b) = pair();
        let result = combine(a, b, Operation::Intersection, TOL).unwrap();
        assert!(result.validate().is_empty());
        assert_eq!(result.euler_characteristic(), 2);
        // The overlap of two boxes is a box.
        assert_eq!(result.faces.len(), 6);
    }

    #[test]
    fn a_difference_turns_the_cutting_solid_s_faces_around() {
        // The whole of what makes a difference a difference: B's walls become
        // the inside of the cavity, so they face the other way than they did.
        let (a, b) = pair();
        let before: Vec<bool> = b.faces.iter().map(|(_, f)| f.forward).collect();
        let result = combine(a, b, Operation::Difference, TOL).unwrap();
        let flipped = result.faces.iter().filter(|(_, f)| !f.forward).count();
        assert!(before.iter().all(|forward| *forward), "the input was all forward");
        assert!(flipped > 0, "nothing was turned around");
    }

    #[test]
    fn a_rounded_boss_meets_a_copy_moved_up_and_across() {
        // A cylinder rounded into its top: the copy's round crosses the top
        // where the original's own round meets it, a traced curve touching
        // the top's circle at two points with a lens between.
        let plain = cylinder([0.0; 3], 1.0, 0.6).unwrap();
        let rim = plain
            .edges
            .iter()
            .find(|(_, edge)| {
                matches!(plain.curves.get(edge.curve),
                    Some(Curve3::Circle(circle)) if circle.plane.origin[2] > 0.3)
            })
            .map(|(key, _)| key)
            .unwrap();
        let boss = crate::fillet_edges(&plain, &[rim], 0.3).unwrap();
        let moved = crate::transform(
            &boss,
            &crate::Placement::at([0.37, 0.11, 0.23]),
        )
        .unwrap();
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        let tolerance = 2e-7;
        let union = combine(boss.clone(), moved.clone(), Operation::Union, tolerance).unwrap();
        let common = combine(boss.clone(), moved, Operation::Intersection, tolerance).unwrap();
        let both = volume(&union) + volume(&common);
        assert!((both - 2.0 * volume(&boss)).abs() < 1e-4 * both, "{both}");
    }

    #[test]
    fn a_box_rounded_along_its_top_meets_a_copy_moved_up_and_across() {
        // The copy's back round meets the top in a line that runs on to the
        // copy's side round inside the top: cut only where the two faces'
        // boxes overlap, it never reached the top's own edges.
        let block = cuboid([0.0; 3], [2.0, 1.4, 1.0]).unwrap();
        let rims: Vec<EdgeKey> = block
            .edges
            .iter()
            .filter(|(_, edge)| {
                let curve = block.curves.get(edge.curve).unwrap();
                [edge.start_parameter, edge.end_parameter]
                    .iter()
                    .all(|at| curve.point_at(*at)[2] > 0.5)
            })
            .map(|(key, _)| key)
            .collect();
        let rounded = crate::fillet_edges(&block, &rims, 0.25).unwrap();
        let moved = crate::transform(
            &rounded,
            &crate::Placement::at([0.443, -0.447, 0.082]),
        )
        .unwrap();
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        let tolerance = 2e-7;
        let union = combine(rounded.clone(), moved.clone(), Operation::Union, tolerance).unwrap();
        let common = combine(rounded.clone(), moved, Operation::Intersection, tolerance).unwrap();
        let both = volume(&union) + volume(&common);
        assert!((both - 2.0 * volume(&rounded)).abs() < 1e-4 * both, "{both}");
    }

    #[test]
    fn a_face_at_a_hole_is_read_at_its_edge() {
        // A box with a round bar through it, imprinted: the box's top meets
        // the bar along a circle and lies outside the bar; the bar's wall
        // inside the box lies inside it — both read at the circle, no ray.
        let mut plate = cuboid([0.0; 3], [4.0, 4.0, 1.0]).unwrap();
        let mut bar = cylinder([2.0, 2.0, -1.0], 1.0, 3.0).unwrap();
        let (plain_plate, plain_bar) = (plate.clone(), bar.clone());
        imprint_reaching(&mut plate, &mut bar, TOL, false).unwrap();
        let top = plate
            .face_keys()
            .find(|face| {
                let point = interior_point(&plate, *face, TOL);
                point.is_some_and(|point| (point[2] - 1.0).abs() < 1e-9)
            })
            .unwrap();
        assert_eq!(local_side(&plate, &plain_bar, top, TOL), Some(Containment::Outside));
        let wall = bar
            .face_keys()
            .find(|face| {
                let round = matches!(
                    bar.surfaces.get(bar.faces.get(*face).unwrap().surface),
                    Some(Surface::Cylinder(_))
                );
                round
                    && interior_point(&bar, *face, TOL)
                        .is_some_and(|point| point[2] > 0.0 && point[2] < 1.0)
            })
            .unwrap();
        assert_eq!(local_side(&bar, &plain_plate, wall, TOL), Some(Containment::Inside));
        // A face meeting nothing of the other body is left to the rays.
        let bottom_cap = bar
            .face_keys()
            .find(|face| {
                let point = interior_point(&bar, *face, TOL);
                point.is_some_and(|point| (point[2] + 1.0).abs() < 1e-9)
            })
            .unwrap();
        assert_eq!(local_side(&bar, &plain_plate, bottom_cap, TOL), None);
    }

    #[test]
    fn many_tools_cut_at_once_take_what_each_would() {
        // Nine holes in a plate, cut together; two overlapping bars beside
        // them go one at a time, and still take their union out.
        let plate = cuboid([0.0; 3], [12.0, 12.0, 2.0]).unwrap();
        let mut tools: Vec<Body> = (0..9)
            .map(|index| {
                let (i, j) = ((index % 3) as f64, (index / 3) as f64);
                cylinder([2.0 + 4.0 * i, 2.0 + 4.0 * j, -1.0], 1.0, 4.0).unwrap()
            })
            .collect();
        tools.push(cuboid([-1.0, 5.5, 0.5], [3.0, 1.0, 1.0]).unwrap());
        tools.push(cuboid([1.0, 5.5, 0.5], [3.0, 1.0, 1.0]).unwrap());
        let one_by_one = tools.iter().cloned().fold(plate.clone(), |result, tool| {
            combine(result, tool, Operation::Difference, TOL).unwrap()
        });
        let result = combine_all(plate, tools, Operation::Difference, TOL).unwrap();
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        let (got, expected) = (volume(&result), volume(&one_by_one));
        let pi = std::f64::consts::PI;
        // The bars take more than half a unit beyond the holes: 4 of them in
        // the plate, less the hole at (2, 6) they cross.
        assert!(expected < 12.0 * 12.0 * 2.0 - 9.0 * pi * 2.0 - 0.5);
        assert!((got - expected).abs() < 1e-6 * expected, "{got} against {expected}");
        assert!(result.validate().is_empty());
    }
}
