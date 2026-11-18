//! Constant edge chamfers and fillets on convex planar solids.

use super::blend_local::{blend_lines, blend_rims, LocalBlend, LocalError};
use super::bounds::operation_tolerance;
use super::geometry::{Circle3, Curve3, Cylinder, Ellipse3, Line3, Sphere, Surface};
use super::topology::{Body, Coedge, Edge, EdgeKey, Face, FaceKey, Loop, Lump, Shell, Vertex};
use super::Provenance;
use ocs_geom::space::{Plane, Vec3};
use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};
use std::fmt;

#[derive(Clone, Copy)]
struct Halfspace {
    origin: Vec3,
    normal: Vec3,
    offset: f64,
    added: bool,
}

struct EdgeFrame {
    point: Vec3,
    axis: Vec3,
    faces: [FaceKey; 2],
    first_normal: Vec3,
    second_normal: Vec3,
    first_inward: Vec3,
    second_inward: Vec3,
    halfspaces: Vec<Halfspace>,
}

#[derive(Clone)]
struct ExistingFillet {
    cut: Halfspace,
    cylinder: Cylinder,
    forward: bool,
    /// Outward normals of the two planar faces the fillet rounds between.
    sides: [Vec3; 2],
}

/// Why one or more selected edges could not be filleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilletError {
    /// No edges were selected.
    EmptySelection,
    /// Radius was zero, negative, NaN or infinite.
    InvalidRadius,
    /// At least one selected key does not name an edge in this body.
    UnknownEdge,
    /// The input body's ownership or adjacency is inconsistent.
    InvalidBodyTopology,
    /// This operation currently accepts one lump, one shell and no face holes.
    UnsupportedBodyTopology,
    /// A selected edge is not straight.
    UnsupportedEdgeCurve(EdgeKey),
    /// A selected edge is not shared by exactly two faces.
    NonManifoldEdge(EdgeKey),
    /// A selected edge is not bounded by two planar faces.
    UnsupportedAdjacentSurface(EdgeKey),
    /// An edge or one of its adjacent surface frames is degenerate.
    DegenerateGeometry(EdgeKey),
    /// Adjacent selected edges need a corner-blend solver.
    AdjacentSelections(EdgeKey, EdgeKey),
    /// The body contains a surface this convex planar operation cannot preserve.
    UnsupportedBodySurface,
    /// The planar body is not the intersection of its face halfspaces.
    NonConvexBody,
    /// A previously created fillet does not have the supported cylindrical form.
    UnsupportedExistingFillet,
    /// Radius removes the body or makes selected blend regions meet.
    RadiusTooLargeOrInteracting,
    /// An end of the selected edge needs a non-circular cylinder section.
    UnsupportedEndCondition(EdgeKey),
    /// Constructed topology or edge geometry did not validate.
    InvalidResult,
}

impl fmt::Display for FilletError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySelection => formatter.write_str("no edges were selected"),
            Self::InvalidRadius => formatter.write_str("fillet radius must be finite and positive"),
            Self::UnknownEdge => formatter.write_str("a selected edge does not belong to the body"),
            Self::InvalidBodyTopology => formatter.write_str("the input body has invalid topology"),
            Self::UnsupportedBodyTopology => formatter.write_str(
                "filleting currently requires one lump, one shell and faces without holes",
            ),
            Self::UnsupportedEdgeCurve(edge) => {
                write!(formatter, "selected edge {edge:?} is not straight")
            }
            Self::NonManifoldEdge(edge) => write!(
                formatter,
                "selected edge {edge:?} is not shared by exactly two faces"
            ),
            Self::UnsupportedAdjacentSurface(edge) => write!(
                formatter,
                "selected edge {edge:?} is not bounded by two planar faces"
            ),
            Self::DegenerateGeometry(edge) => {
                write!(formatter, "selected edge {edge:?} has degenerate geometry")
            }
            Self::AdjacentSelections(first, second) => write!(
                formatter,
                "selected edges {first:?} and {second:?} meet; corner blends are unsupported"
            ),
            Self::UnsupportedBodySurface => formatter.write_str(
                "the body contains a surface this convex planar fillet cannot preserve",
            ),
            Self::NonConvexBody => {
                formatter.write_str("the body is not a convex intersection of planar halfspaces")
            }
            Self::UnsupportedExistingFillet => formatter.write_str(
                "an existing cylindrical face is not a supported constant edge fillet",
            ),
            Self::RadiusTooLargeOrInteracting => formatter.write_str(
                "the radius removes the body or makes selected fillet regions interact",
            ),
            Self::UnsupportedEndCondition(edge) => write!(
                formatter,
                "selected edge {edge:?} needs an unsupported fillet end condition"
            ),
            Self::InvalidResult => formatter.write_str("the filleted body did not validate"),
        }
    }
}

impl std::error::Error for FilletError {}

/// Why one or more selected edges could not be chamfered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChamferError {
    EmptySelection,
    InvalidDistance,
    UnknownEdge,
    UnknownBaseFace,
    EdgeOutsideBaseFace(EdgeKey),
    InvalidBodyTopology,
    UnsupportedBodyTopology,
    UnsupportedEdgeCurve(EdgeKey),
    NonManifoldEdge(EdgeKey),
    UnsupportedAdjacentSurface(EdgeKey),
    DegenerateGeometry(EdgeKey),
    UnsupportedBodySurface,
    NonConvexBody,
    UnsupportedExistingFillet,
    DistanceTooLargeOrInteracting,
    InvalidResult,
}

impl fmt::Display for ChamferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySelection => formatter.write_str("no edges were selected"),
            Self::InvalidDistance => {
                formatter.write_str("chamfer distances must be finite and positive")
            }
            Self::UnknownEdge => formatter.write_str("a selected edge does not belong to the body"),
            Self::UnknownBaseFace => formatter.write_str("the base face does not belong to the body"),
            Self::EdgeOutsideBaseFace(edge) => write!(
                formatter,
                "selected edge {edge:?} does not belong to the common base face"
            ),
            Self::InvalidBodyTopology => formatter.write_str("the input body has invalid topology"),
            Self::UnsupportedBodyTopology => formatter.write_str(
                "chamfering currently requires one lump, one shell and faces without holes",
            ),
            Self::UnsupportedEdgeCurve(edge) => {
                write!(formatter, "selected edge {edge:?} is not straight")
            }
            Self::NonManifoldEdge(edge) => write!(
                formatter,
                "selected edge {edge:?} is not shared by exactly two faces"
            ),
            Self::UnsupportedAdjacentSurface(edge) => write!(
                formatter,
                "selected edge {edge:?} is not bounded by two planar faces"
            ),
            Self::DegenerateGeometry(edge) => {
                write!(formatter, "selected edge {edge:?} has degenerate geometry")
            }
            Self::UnsupportedBodySurface => formatter.write_str(
                "the body contains a surface this convex planar chamfer cannot preserve",
            ),
            Self::NonConvexBody => {
                formatter.write_str("the body is not a convex intersection of planar halfspaces")
            }
            Self::UnsupportedExistingFillet => formatter.write_str(
                "an existing cylindrical face is not a supported constant edge fillet",
            ),
            Self::DistanceTooLargeOrInteracting => formatter.write_str(
                "a distance removes the body or makes selected chamfer regions interact",
            ),
            Self::InvalidResult => formatter.write_str("the chamfered body did not validate"),
        }
    }
}

impl std::error::Error for ChamferError {}

/// Cuts a symmetric chamfer at one straight edge.
pub fn chamfer(body: &Body, edge: EdgeKey, distance: f64) -> Option<Body> {
    let frame = edge_frame(body, edge)?;
    chamfer_edges(body, &[edge], frame.faces[0], distance, distance).ok()
}

/// Bevels several straight edges on one base face in one atomic operation.
///
/// `base_distance` is measured on `base_face`; `other_distance` is measured
/// on the other face adjacent to each selected edge. Selection order and
/// duplicate keys do not affect the result.
pub fn chamfer_edges(
    body: &Body,
    selected: &[EdgeKey],
    base_face: FaceKey,
    base_distance: f64,
    other_distance: f64,
) -> Result<Body, ChamferError> {
    if !base_distance.is_finite()
        || !other_distance.is_finite()
        || base_distance <= 0.0
        || other_distance <= 0.0
    {
        return Err(ChamferError::InvalidDistance);
    }
    if selected.is_empty() {
        return Err(ChamferError::EmptySelection);
    }
    if !body.validate().is_empty() {
        return Err(ChamferError::InvalidBodyTopology);
    }
    // Solidcore's chamfer first; this kernel's own for what it refuses.
    if let Ok(bevelled) =
        super::bridge::chamfer(body, selected, base_face, base_distance, other_distance)
    {
        return Ok(bevelled);
    }
    if !body.faces.contains(base_face) {
        return Err(ChamferError::UnknownBaseFace);
    }
    if selected.iter().any(|edge| !body.edges.contains(*edge)) {
        return Err(ChamferError::UnknownEdge);
    }

    let mut selected = selected.to_vec();
    selected.sort_by_key(EdgeKey::slot);
    selected.dedup();
    if let Some(result) = super::chamfer_circular::chamfer_circular(
        body,
        &selected,
        base_face,
        base_distance,
        other_distance,
    ) {
        return result;
    }
    // Set back equally on both faces, a bevel does not care which is the
    // base: edges off the base face — every edge of a block at once — are
    // bevelled all the same, the base face only naming the distances.
    let symmetric = base_distance == other_distance;
    let rim = LocalBlend::Chamfer { base_face, base: base_distance, other: other_distance };
    match blend_rims(body, &selected, rim) {
        Some(Err(LocalError::OutsideBaseFace(_))) if symmetric => {}
        Some(result) => {
            return result.map_err(|error| match error {
                LocalError::TooLarge => ChamferError::DistanceTooLargeOrInteracting,
                LocalError::OutsideBaseFace(edge) => ChamferError::EdgeOutsideBaseFace(edge),
                LocalError::InvalidResult => ChamferError::InvalidResult,
            });
        }
        None => {}
    }
    let prismatic = super::chamfer_prismatic::chamfer_prismatic(
        body,
        &selected,
        base_face,
        base_distance,
        other_distance,
    );
    if let Some(Ok(result)) = prismatic {
        return Ok(result);
    }
    let tolerance = operation_tolerance(&[body]);
    if is_open_sheet(body) {
        return chamfer_sheet(
            body,
            &selected,
            base_face,
            base_distance,
            other_distance,
            tolerance,
        );
    }
    let distances = (base_distance, other_distance);
    let convex = prismatic
        .unwrap_or_else(|| chamfer_convex(body, &selected, base_face, distances, tolerance));
    // A body the convex solver cannot rebuild may still take a local cut.
    let Err(error) = convex else { return convex };
    let local = LocalBlend::Chamfer { base_face, base: base_distance, other: other_distance };
    let local = match blend_lines(body, &selected, local) {
        Some(result) => result.map_err(|error| match error {
            LocalError::TooLarge => ChamferError::DistanceTooLargeOrInteracting,
            LocalError::OutsideBaseFace(edge) => ChamferError::EdgeOutsideBaseFace(edge),
            LocalError::InvalidResult => ChamferError::InvalidResult,
        }),
        None => Err(error),
    };
    // Last, the bevel cut by a boolean, as a fillet's round is.
    match local {
        Err(
            error @ (ChamferError::UnsupportedBodyTopology
            | ChamferError::NonConvexBody
            | ChamferError::UnsupportedExistingFillet
            | ChamferError::UnsupportedBodySurface
            | ChamferError::DistanceTooLargeOrInteracting
            | ChamferError::UnsupportedEdgeCurve(_)
            | ChamferError::InvalidResult),
        ) => {
            let bevel = Section::Bevel { base_face, base: base_distance, other: other_distance };
            blend_by_cut(body, &selected, bevel).ok_or(error)
        }
        result => result,
    }
}

/// Cuts straight edges of a convex planar body, keeping earlier fillets.
fn chamfer_convex(
    body: &Body,
    selected: &[EdgeKey],
    base_face: FaceKey,
    (base_distance, other_distance): (f64, f64),
    tolerance: f64,
) -> Result<Body, ChamferError> {
    let existing = existing_fillets(body).ok_or(ChamferError::UnsupportedExistingFillet)?;
    let corners = existing_corners(body).ok_or(ChamferError::UnsupportedExistingFillet)?;
    validate_chamfer_body(body, &existing, tolerance)?;

    let mut frames = Vec::with_capacity(selected.len());
    for &edge in selected {
        validate_chamfer_edge(body, edge)?;
        let frame = edge_frame(body, edge).ok_or(ChamferError::DegenerateGeometry(edge))?;
        if !frame.faces.contains(&base_face) && base_distance != other_distance {
            return Err(ChamferError::EdgeOutsideBaseFace(edge));
        }
        frames.push((edge, frame));
    }

    let mut halfspaces = frames[0].1.halfspaces.clone();
    for (edge, frame) in &frames {
        let distances = if frame.faces[0] == base_face {
            (base_distance, other_distance)
        } else {
            (other_distance, base_distance)
        };
        let cut = cut_halfspace_distances(frame, distances.0, distances.1, tolerance)
            .ok_or(ChamferError::DegenerateGeometry(*edge))?;
        halfspaces.push(cut);
    }
    let (mut result, _) = convex_body(&halfspaces, tolerance)
        .ok_or(ChamferError::DistanceTooLargeOrInteracting)?;
    restore_fillets(&mut result, &existing, &corners, tolerance)
        .ok_or(ChamferError::UnsupportedExistingFillet)?;
    if !result.validate().is_empty()
        || result.worst_vertex_gap() > tolerance
        || !blend_boundaries_fit(&result, tolerance)
    {
        return Err(ChamferError::InvalidResult);
    }
    Ok(result)
}

fn is_open_sheet(body: &Body) -> bool {
    body.edges.iter().any(|(_, edge)| edge.coedges.len() == 1)
        && body.edges.iter().all(|(_, edge)| !edge.coedges.is_empty() && edge.coedges.len() <= 2)
}

fn chamfer_sheet(
    body: &Body,
    selected: &[EdgeKey],
    base_face: FaceKey,
    base_distance: f64,
    other_distance: f64,
    tolerance: f64,
) -> Result<Body, ChamferError> {
    if body.roots.len() != 1
        || body
            .lumps
            .get(body.roots[0])
            .is_none_or(|lump| lump.shells.len() != 1)
        || body.faces.iter().any(|(_, face)| face.loops.len() != 1)
        || body.faces.iter().any(|(_, face)| {
            !matches!(body.surfaces.get(face.surface), Some(Surface::Plane(_)))
        })
    {
        return Err(ChamferError::UnsupportedBodyTopology);
    }
    let (supports, boundaries) = sheet_halfspaces(body, tolerance)
        .ok_or(ChamferError::UnsupportedBodySurface)?;
    let mut halfspaces = supports
        .iter()
        .map(|(_, halfspace)| *halfspace)
        .chain(boundaries)
        .collect::<Vec<_>>();
    let mut cuts = Vec::with_capacity(selected.len());
    for edge in selected {
        validate_chamfer_edge(body, *edge)?;
        let frame = sheet_edge_frame(body, *edge, &supports, halfspaces.clone())
            .ok_or(ChamferError::DegenerateGeometry(*edge))?;
        if !frame.faces.contains(&base_face) {
            return Err(ChamferError::EdgeOutsideBaseFace(*edge));
        }
        let distances = if frame.faces[0] == base_face {
            (base_distance, other_distance)
        } else {
            (other_distance, base_distance)
        };
        let cut = cut_halfspace_distances(&frame, distances.0, distances.1, tolerance)
            .ok_or(ChamferError::DegenerateGeometry(*edge))?;
        halfspaces.push(cut);
        cuts.push(cut);
    }
    let (closed, _) = convex_body(&halfspaces, tolerance)
        .ok_or(ChamferError::DistanceTooLargeOrInteracting)?;
    let kept = supports
        .iter()
        .map(|(_, halfspace)| *halfspace)
        .chain(cuts)
        .filter_map(|halfspace| face_on(&closed, halfspace, tolerance))
        .collect::<HashSet<_>>();
    if kept.len() < supports.len() + selected.len() {
        return Err(ChamferError::DistanceTooLargeOrInteracting);
    }

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
    let mut kept = kept.into_iter().collect::<Vec<_>>();
    kept.sort_by_key(FaceKey::slot);
    for face in kept {
        super::boolean::copy_face(&mut result, &closed, face, shell, false)
            .map_err(|_| ChamferError::InvalidResult)?;
    }
    result.lumps.get_mut(lump).ok_or(ChamferError::InvalidResult)?.shells = vec![shell];
    result.roots = vec![lump];
    if !result.validate().is_empty()
        || result.worst_vertex_gap() > tolerance
        || !blend_boundaries_fit(&result, tolerance)
    {
        return Err(ChamferError::InvalidResult);
    }
    Ok(result)
}

fn sheet_halfspaces(
    body: &Body,
    tolerance: f64,
) -> Option<(Vec<(FaceKey, Halfspace)>, Vec<Halfspace>)> {
    let points = body
        .vertices
        .iter()
        .map(|(_, vertex)| Vec3::from(vertex.point))
        .collect::<Vec<_>>();
    let centre = points.iter().copied().fold(Vec3::ZERO, |sum, point| sum + point)
        / points.len() as f64;
    let mut supports = Vec::new();
    for (face_key, face) in body.faces.iter() {
        let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
            return None;
        };
        let mut normal = Vec3::from(plane.normal()?);
        let mut offset = normal.dot(Vec3::from(plane.origin));
        if normal.dot(centre) > offset {
            normal = -normal;
            offset = -offset;
        }
        supports.push((face_key, Halfspace {
            origin: Vec3::from(plane.origin),
            normal,
            offset,
            added: false,
        }));
    }

    let mut boundaries = Vec::new();
    for (_, edge) in body.edges.iter().filter(|(_, edge)| edge.coedges.len() == 1) {
        let coedge = body.coedges.get(edge.coedges[0])?;
        let face = body.loops.get(coedge.owner)?.owner;
        let face_normal = supports.iter().find(|(key, _)| *key == face)?.1.normal;
        let start = Vec3::from(body.vertices.get(edge.start)?.point);
        let end = Vec3::from(body.vertices.get(edge.end)?.point);
        let axis = (end - start).normalize()?;
        let face_points = body
            .face_coedges(face)
            .into_iter()
            .filter_map(|key| body.coedges.get(key))
            .filter_map(|coedge| body.edges.get(coedge.edge))
            .flat_map(|edge| [edge.start, edge.end])
            .filter_map(|vertex| body.vertices.get(vertex))
            .map(|vertex| Vec3::from(vertex.point))
            .collect::<Vec<_>>();
        let face_centre = face_points
            .iter()
            .copied()
            .fold(Vec3::ZERO, |sum, point| sum + point)
            / face_points.len() as f64;
        let mut normal = face_normal.cross(axis).normalize()?;
        let mut offset = normal.dot(start);
        if normal.dot(face_centre) > offset + tolerance {
            normal = -normal;
            offset = -offset;
        }
        if supports.iter().any(|(_, candidate)| same_halfspace(*candidate, normal, offset, tolerance))
            || boundaries
                .iter()
                .any(|candidate| same_halfspace(*candidate, normal, offset, tolerance))
        {
            continue;
        }
        boundaries.push(Halfspace {
            origin: start,
            normal,
            offset,
            added: false,
        });
    }
    Some((supports, boundaries))
}

fn same_halfspace(
    candidate: Halfspace,
    normal: Vec3,
    offset: f64,
    tolerance: f64,
) -> bool {
    candidate.normal.dot(normal) > 1.0 - 1.0e-8
        && (candidate.offset - offset).abs() <= tolerance
}

fn sheet_edge_frame(
    body: &Body,
    edge_key: EdgeKey,
    supports: &[(FaceKey, Halfspace)],
    halfspaces: Vec<Halfspace>,
) -> Option<EdgeFrame> {
    let edge = body.edges.get(edge_key)?;
    if !matches!(body.curves.get(edge.curve)?, Curve3::Line(_)) || edge.coedges.len() != 2 {
        return None;
    }
    let start = Vec3::from(body.vertices.get(edge.start)?.point);
    let end = Vec3::from(body.vertices.get(edge.end)?.point);
    let axis = (end - start).normalize()?;
    let face_of = |coedge| {
        let loop_key = body.coedges.get(coedge)?.owner;
        Some(body.loops.get(loop_key)?.owner)
    };
    let faces = [face_of(edge.coedges[0])?, face_of(edge.coedges[1])?];
    let normal_of = |face| supports.iter().find(|(key, _)| *key == face).map(|(_, value)| value.normal);
    let first_normal = normal_of(faces[0])?;
    let second_normal = normal_of(faces[1])?;
    let dot = first_normal.dot(second_normal);
    if dot.abs() > 1.0 - 1.0e-9 {
        return None;
    }
    let first_inward = -(second_normal - first_normal * dot).normalize()?;
    let second_inward = -(first_normal - second_normal * dot).normalize()?;
    Some(EdgeFrame {
        point: start,
        axis,
        faces,
        first_normal,
        second_normal,
        first_inward,
        second_inward,
        halfspaces,
    })
}

/// Rounds one straight edge with a constant-radius cylindrical face.
pub fn fillet(body: &Body, edge: EdgeKey, radius: f64) -> Option<Body> {
    fillet_edges(body, &[edge], radius).ok()
}

/// Rounds several straight edges in one atomic operation.
///
/// Selection order and duplicate keys do not affect the result. Orthogonal
/// corners use exact elliptical seams or spherical three-edge patches.
pub fn fillet_edges(
    body: &Body,
    selected: &[EdgeKey],
    radius: f64,
) -> Result<Body, FilletError> {
    if !radius.is_finite() || radius <= 0.0 {
        return Err(FilletError::InvalidRadius);
    }
    if selected.is_empty() {
        return Err(FilletError::EmptySelection);
    }
    if !body.validate().is_empty() {
        return Err(FilletError::InvalidBodyTopology);
    }
    // Solidcore's rolling-ball round first; this kernel's own for what it refuses.
    if let Ok(rounded) = super::bridge::fillet(body, selected, radius) {
        return Ok(rounded);
    }
    if selected.iter().any(|edge| !body.edges.contains(*edge)) {
        return Err(FilletError::UnknownEdge);
    }

    let mut selected = selected.to_vec();
    selected.sort_by_key(EdgeKey::slot);
    selected.dedup();
    if let Some(result) = super::fillet_circular::fillet_circular(body, &selected, radius) {
        return result;
    }
    if let Some(result) = blend_rims(body, &selected, LocalBlend::Fillet(radius)) {
        return result.map_err(|error| match error {
            LocalError::TooLarge => FilletError::RadiusTooLargeOrInteracting,
            LocalError::OutsideBaseFace(_) | LocalError::InvalidResult => {
                FilletError::InvalidResult
            }
        });
    }
    let prismatic = super::fillet_prismatic::fillet_prismatic(body, &selected, radius);
    if let Some(Ok(result)) = prismatic {
        return Ok(result);
    }

    let convex = match prismatic.unwrap_or_else(|| fillet_convex(body, &selected, radius)) {
        Err(
            error @ (FilletError::RadiusTooLargeOrInteracting
            | FilletError::UnsupportedEndCondition(_)
            | FilletError::InvalidResult),
        ) => refillet_from_sharp(body, &selected, radius).ok_or(error),
        result => result,
    };
    // A body the convex solver cannot rebuild may still take a local cut.
    let Err(error) = convex else { return convex };
    let local = match blend_lines(body, &selected, LocalBlend::Fillet(radius)) {
        Some(result) => result.map_err(|error| match error {
            LocalError::TooLarge => FilletError::RadiusTooLargeOrInteracting,
            LocalError::OutsideBaseFace(_) | LocalError::InvalidResult => {
                FilletError::InvalidResult
            }
        }),
        None => Err(error),
    };
    // Last, the edge rounded by a boolean: whatever else the body is, the
    // corner between two planes and the rolling cylinder is a prism — or,
    // round a plane's circle on a cylinder or cone, a ring.
    match local {
        Err(
            error @ (FilletError::UnsupportedBodyTopology
            | FilletError::NonConvexBody
            | FilletError::UnsupportedExistingFillet
            | FilletError::UnsupportedBodySurface
            | FilletError::UnsupportedEndCondition(_)
            | FilletError::RadiusTooLargeOrInteracting
            | FilletError::UnsupportedEdgeCurve(_)
            | FilletError::InvalidResult),
        ) => blend_by_cut(body, &selected, Section::Round(radius))
            // Last of all, a ball rolled along a closed chain between two
            // faces of any surfaces that offset exactly: the saddle where
            // two cylinders join.
            .or_else(|| super::fillet_rolling::fillet_rolling(body, &selected, radius))
            .ok_or(error),
        result => result,
    }
}

/// Whether an edge's curve runs straight: a line, or a first-degree spline
/// with its points in a row — what a boolean leaves of a line it cut to
/// the stretch both solids share.
fn straight(curve: &Curve3) -> bool {
    match curve {
        Curve3::Line(_) => true,
        Curve3::Nurbs(spline) if spline.degree() == 1 => {
            let points = spline.control_points();
            let (Some(first), Some(last)) = (points.first(), points.last()) else {
                return false;
            };
            let (first, last) = (Vec3::from(*first), Vec3::from(*last));
            let Some(along) = (last - first).normalize() else {
                return false;
            };
            let span = first.distance(last);
            points.iter().all(|point| {
                let offset = Vec3::from(*point) - first;
                (offset - along * offset.dot(along)).length() <= span * 1e-12
            })
        }
        _ => false,
    }
}

/// Whether a boolean with a blend's tool changed the solid by no more than
/// the tool holds, to within what a volume is measured to: a misread face
/// can drop a whole stretch of the solid and still leave it closed, a round
/// taking thousands of times its own volume.
fn within_tool(body: &Body, tool: &Body, result: &Body) -> bool {
    let volume = |body: &Body| super::mass_properties(body).map(|mass| mass.volume);
    match (volume(body), volume(tool), volume(result)) {
        (Some(before), Some(tool), Some(after)) => {
            (after - before).abs() <= tool.abs() * 1.01 + before.abs() * 1e-6
        }
        _ => false,
    }
}

/// What a cut along an edge leaves: a round, or a bevel set back by one
/// distance on a base face and another on the face across the edge.
#[derive(Clone, Copy)]
enum Section {
    Round(f64),
    Bevel { base_face: FaceKey, base: f64, other: f64 },
}

/// Straight edges between two planes rounded or bevelled one by one with
/// booleans: the region between the two faces and the round or bevel, run
/// the edge's length, cut away from a convex edge or filled into a concave
/// one. `None` unless the section rests on both faces along every edge.
fn blend_by_cut(body: &Body, selected: &[EdgeKey], section: Section) -> Option<Body> {
    let tolerance = operation_tolerance(&[body]);
    // Each boolean renames the edges; they are found again by their ends,
    // and a circle by its centre and radius as well.
    let marks: Vec<(Vec3, Vec3, Option<(Vec3, f64)>)> = selected
        .iter()
        .map(|edge| {
            let edge = body.edges.get(*edge)?;
            let ring = match body.curves.get(edge.curve)? {
                Curve3::Circle(circle) => Some((Vec3::from(circle.plane.origin), circle.radius)),
                _ => None,
            };
            Some((
                Vec3::from(body.vertices.get(edge.start)?.point),
                Vec3::from(body.vertices.get(edge.end)?.point),
                ring,
            ))
        })
        .collect::<Option<_>>()?;
    let mut result = body.clone();
    for (start, end, ring) in marks {
        let near = |p: Vec3, q: Vec3| p.distance(q) <= tolerance * 10.0;
        // A straight one: an arc may join the same two corners. A straight
        // edge a round before shortened — two edges meeting at a corner,
        // the first rounded — is what is left of it: the longest straight
        // edge lying along the stretch it ran.
        let along = |point: Vec3| point.distance_to_segment(start, end) <= tolerance * 10.0;
        let edge = result
            .edges
            .iter()
            .filter_map(|(key, edge)| {
                let a = Vec3::from(result.vertices.get(edge.start)?.point);
                let b = Vec3::from(result.vertices.get(edge.end)?.point);
                match (result.curves.get(edge.curve)?, ring) {
                    (curve, None) if straight(curve) => {
                        (along(a) && along(b)).then_some((key, a.distance(b)))
                    }
                    (Curve3::Circle(circle), Some((centre, radius)))
                        if near(Vec3::from(circle.plane.origin), centre)
                            && (circle.radius - radius).abs() <= tolerance * 10.0 =>
                    {
                        ((near(a, start) && near(b, end)) || (near(a, end) && near(b, start)))
                            .then_some((key, 0.0))
                    }
                    _ => None,
                }
            })
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(key, _)| key)?;
        result = match ring {
            None => cut_along_edge(&result, edge, section, tolerance)
                .or_else(|| cut_along_ruling(&result, edge, section, tolerance))?,
            Some(_) => turn_along_edge(&result, edge, section, tolerance)?,
        };
    }
    Some(result)
}

/// [`cut_along_edge`] down a straight edge where a plane meets a cylinder
/// along one of its rulings — a flat ground on a shaft, a slot's side
/// running into its round end at an angle. Across the edge the plane is a
/// line and the cylinder a circle: the ball sits a radius off the line and
/// a radius in from (or out round) the circle, and the section between its
/// two touches and the corner is run the edge's length.
fn cut_along_ruling(
    body: &Body,
    edge_key: EdgeKey,
    section: Section,
    tolerance: f64,
) -> Option<Body> {
    let edge = body.edges.get(edge_key)?;
    if !straight(body.curves.get(edge.curve)?) || edge.coedges.len() != 2 {
        return None;
    }
    let start = Vec3::from(body.vertices.get(edge.start)?.point);
    let end = Vec3::from(body.vertices.get(edge.end)?.point);
    let length = start.distance(end);
    let axis = (end - start).normalize()?;
    let face_of = |coedge| Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner);
    let faces = [face_of(edge.coedges[0])?, face_of(edge.coedges[1])?];
    let surface_of =
        |face: FaceKey| body.faces.get(face).and_then(|node| body.surfaces.get(node.surface));
    // The plane first, the cylinder second, its axis along the edge.
    let (flat, round) = match (surface_of(faces[0])?, surface_of(faces[1])?) {
        (Surface::Plane(_), Surface::Cylinder(_)) => (faces[0], faces[1]),
        (Surface::Cylinder(_), Surface::Plane(_)) => (faces[1], faces[0]),
        _ => return None,
    };
    let Surface::Cylinder(cylinder) = surface_of(round)? else {
        return None;
    };
    if Vec3::from(cylinder.base.normal()?).cross(axis).length() > 1e-9 {
        return None;
    }
    let outward = |face: FaceKey, point: Vec3| -> Option<Vec3> {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        let (u, v) = surface.parameters_at(point.to_array())?;
        let normal = Vec3::from(surface.normal_at(u, v)?);
        Some(if node.forward { normal } else { -normal })
    };
    let (flat_out, round_out) = (outward(flat, start)?, outward(round, start)?);
    if flat_out.dot(round_out).abs() > 0.996 {
        return None;
    }
    let middle = start.lerp(end, 0.5);
    let probe = middle + (outward(round, middle)? - outward(flat, middle)?) * (length * 0.01);
    let convex = super::classify::contains_point(body, probe.to_array(), tolerance)
        == super::Containment::Outside;
    // The section, square to the edge at its start: x along the plane away
    // from the edge, the cylinder's centre and radius in the same frame.
    let along_flat = axis.cross(flat_out).normalize()?;
    let along_flat = if super::classify::face_distance(
        body,
        flat,
        (start + along_flat * (length * 1e-3).min(cylinder.radius * 1e-3)).to_array(),
        tolerance,
    )? <= tolerance * 10.0
    {
        along_flat
    } else {
        -along_flat
    };
    let plane = Plane::orthonormal(start.to_array(), along_flat.to_array(), axis.to_array())?;
    let flat2 = |point: Vec3| plane.project(point.to_array());
    let centre = {
        let origin = Vec3::from(cylinder.base.origin);
        origin + axis * (start - origin).dot(axis)
    };
    let c = flat2(centre)?;
    let radius_b = cylinder.radius;
    // Into the material from the plane, in the section.
    let into = {
        let n = flat2(start - flat_out)?;
        [n[0], n[1]]
    };
    let into = {
        let length = into[0].hypot(into[1]);
        [into[0] / length, into[1] / length]
    };
    // Material within the cylinder where its outward normal points from
    // the axis.
    let inside = round_out.dot(start - centre) > 0.0;
    let side = if convex { 1.0 } else { -1.0 };
    let radius = match section {
        Section::Round(radius) => radius,
        Section::Bevel { base, other, .. } => base.max(other),
    };
    let (touch_flat, touch_round, ball) = match section {
        Section::Round(_) => {
            // The ball's centre: a radius off the line on the material's
            // side of a convex edge (outside a concave one), and the
            // cylinder's radius less or more the ball's from its axis.
            let offset = [into[0] * radius * side, into[1] * radius * side];
            let reach = if inside == convex { radius_b - radius } else { radius_b + radius };
            if reach <= 0.0 {
                return None;
            }
            // Along x from the offset line's foot: |(s, oy) - c| = reach.
            let dy = offset[1] - c[1];
            let disc = reach * reach - dy * dy;
            if disc < 0.0 {
                return None;
            }
            let roots = [c[0] - disc.sqrt(), c[0] + disc.sqrt()];
            let s = roots
                .into_iter()
                .filter(|s| *s > tolerance)
                .min_by(|a, b| a.total_cmp(b))?;
            let ball = [s + offset[0], offset[1]];
            let touch_flat = [s, 0.0];
            let away = [ball[0] - c[0], ball[1] - c[1]];
            let length = away[0].hypot(away[1]);
            let touch_round =
                [c[0] + away[0] / length * radius_b, c[1] + away[1] / length * radius_b];
            (touch_flat, touch_round, Some(ball))
        }
        Section::Bevel { base_face, base, other } => {
            let (on_flat, on_round) = if base_face == flat { (base, other) } else { (other, base) };
            // Round the circle from the corner, the way the cylinder face
            // runs.
            let corner = [0.0, 0.0];
            let from = (corner[1] - c[1]).atan2(corner[0] - c[0]);
            let turn = on_round / radius_b;
            let candidates = [from + turn, from - turn].map(|angle| {
                [c[0] + radius_b * angle.cos(), c[1] + radius_b * angle.sin()]
            });
            let on_face = |point: [f64; 2]| {
                let spatial = Vec3::from(plane.point_at(point));
                super::classify::face_distance(body, round, spatial.to_array(), tolerance)
                    .is_some_and(|gap| gap <= tolerance * 10.0)
            };
            let touch_round = candidates.into_iter().find(|point| on_face(*point))?;
            ([on_flat, 0.0], touch_round, None)
        }
    };
    // A round or bevel hugs its corner: touches more than a few radii off
    // it are a ball sat round the far side of the cylinder, and the cut
    // between would take the material in between.
    let reach = |point: [f64; 2]| point[0].hypot(point[1]);
    if reach(touch_flat) > radius * 4.0 || reach(touch_round) > radius * 4.0 {
        return None;
    }
    // Both touches on their faces, all along the edge.
    let lift_to = |point: [f64; 2], at: f64| {
        Vec3::from(plane.point_at(point)) + axis * (length * at)
    };
    for at in [0.02, 0.5, 0.98] {
        for (point, face) in [(touch_flat, flat), (touch_round, round)] {
            let spatial = lift_to(point, at).to_array();
            let gap = super::classify::face_distance(body, face, spatial, tolerance)?;
            if gap > tolerance * 10.0 {
                return None;
            }
        }
    }
    let line = |start: [f64; 2], end: [f64; 2]| {
        ocs_geom::geom2d::Curve::Line(ocs_geom::geom2d::Line { start, end })
    };
    let arc_between = |centre: [f64; 2],
                       radius: f64,
                       from: [f64; 2],
                       to: [f64; 2],
                       toward: [f64; 2]| {
        let angle = |p: [f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
        let (a, b, t) = (angle(from), angle(to), angle(toward));
        let turn = |x: f64, y: f64| (y - x).rem_euclid(TAU);
        let (low, high) = if turn(a, t) < turn(a, b) { (a, b) } else { (b, a) };
        ocs_geom::geom2d::Curve::Arc(ocs_geom::geom2d::Arc {
            centre,
            radius,
            start_angle: low,
            end_angle: low + turn(low, high),
        })
    };
    let corner = [0.0, 0.0];
    let blend = match ball {
        Some(ball) => arc_between(ball, radius, touch_flat, touch_round, corner),
        None => line(touch_flat, touch_round),
    };
    // The cylinder's own arc from its touch back to the corner, the short
    // way.
    let rim_middle = {
        let mid = [(touch_round[0] + corner[0]) / 2.0, (touch_round[1] + corner[1]) / 2.0];
        let away = [mid[0] - c[0], mid[1] - c[1]];
        let length = away[0].hypot(away[1]);
        [c[0] + away[0] / length * radius_b, c[1] + away[1] / length * radius_b]
    };
    let rim = arc_between(c, radius_b, touch_round, corner, rim_middle);
    let profile = if convex {
        // Stood a little off both faces, outside, as along two planes.
        let clear = radius * 0.25;
        let out_flat = [-into[0] * clear, -into[1] * clear];
        let radial = |p: [f64; 2]| {
            let d = [p[0] - c[0], p[1] - c[1]];
            let l = d[0].hypot(d[1]);
            let sign = if inside { 1.0 } else { -1.0 };
            [p[0] + d[0] / l * clear * sign, p[1] + d[1] / l * clear * sign]
        };
        let flat_out = [touch_flat[0] + out_flat[0], touch_flat[1] + out_flat[1]];
        let round_out = radial(touch_round);
        let corner_out = [out_flat[0] + radial(corner)[0], out_flat[1] + radial(corner)[1]];
        vec![
            line(touch_flat, flat_out),
            line(flat_out, corner_out),
            line(corner_out, round_out),
            line(round_out, touch_round),
            blend,
        ]
    } else {
        vec![line(corner, touch_flat), blend, rim]
    };
    let tool = super::extrude_region(plane, &[profile], (axis * length).to_array())?;
    let how = if convex {
        super::boolean::Operation::Difference
    } else {
        super::boolean::Operation::Union
    };
    let rounded = super::combine(body.clone(), tool.clone(), how, tolerance).ok()?;
    let sound = rounded.validate().is_empty() && !rounded.roots.is_empty();
    (sound && within_tool(body, &tool, &rounded)).then_some(rounded)
}

/// [`cut_along_edge`] round a circle where a plane square to its axis meets
/// a cylinder or cone about it — a hole's rim, a boss's, a countersink's —
/// the same section swept round the axis over the edge's span rather than
/// along a line.
fn turn_along_edge(
    body: &Body,
    edge_key: EdgeKey,
    section: Section,
    tolerance: f64,
) -> Option<Body> {
    let edge = body.edges.get(edge_key)?;
    let curve = body.curves.get(edge.curve)?;
    let Curve3::Circle(circle) = curve else {
        return None;
    };
    let span = edge.end_parameter - edge.start_parameter;
    if edge.coedges.len() != 2 || !(span > 0.0 && span <= TAU + 1e-9) {
        return None;
    }
    let centre = Vec3::from(circle.plane.origin);
    let axis = Vec3::from(circle.plane.normal()?);
    let face_of = |coedge| Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner);
    let faces = [face_of(edge.coedges[0])?, face_of(edge.coedges[1])?];
    let square = faces.iter().all(|face| {
        match body.faces.get(*face).and_then(|node| body.surfaces.get(node.surface)) {
            Some(Surface::Plane(plane)) => plane
                .normal()
                .is_some_and(|normal| Vec3::from(normal).cross(axis).length() <= 1e-9),
            Some(Surface::Cylinder(cylinder)) => {
                let own = cylinder.base.normal().map(Vec3::from);
                let offset = Vec3::from(cylinder.base.origin) - centre;
                own.is_some_and(|own| own.cross(axis).length() <= 1e-9)
                    && (offset - axis * offset.dot(axis)).length() <= tolerance * 10.0
                    && (cylinder.radius - circle.radius).abs() <= tolerance * 10.0
            }
            // A countersink's: straight in the plane through the axis too.
            Some(Surface::Cone(cone)) => {
                let own = cone.base.normal().map(Vec3::from);
                let offset = Vec3::from(cone.base.origin) - centre;
                own.is_some_and(|own| own.cross(axis).length() <= 1e-9)
                    && (offset - axis * offset.dot(axis)).length() <= tolerance * 10.0
            }
            _ => false,
        }
    });
    if !square {
        return None;
    }
    // Outward, at a point of the edge: in the plane through the axis there.
    let outward = |face: FaceKey, point: Vec3| -> Option<Vec3> {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        let (u, v) = surface.parameters_at(point.to_array())?;
        let normal = Vec3::from(surface.normal_at(u, v)?);
        Some(if node.forward { normal } else { -normal })
    };
    let point_at =
        |fraction: f64| Vec3::from(curve.point_at(edge.start_parameter + span * fraction));
    let normals_at = |point: Vec3| Some([outward(faces[0], point)?, outward(faces[1], point)?]);
    let start = point_at(0.0);
    let normals = normals_at(start)?;
    let dot = normals[0].dot(normals[1]);
    if dot.abs() > 1.0 - 1e-9 {
        return None;
    }
    let middle = point_at(0.5);
    let crossed = normals_at(middle)?;
    let probe = middle + (crossed[1] - crossed[0]) * (circle.radius * 0.01);
    let convex = super::classify::contains_point(body, probe.to_array(), tolerance)
        == super::Containment::Outside;
    let side = if convex { -1.0 } else { 1.0 };
    let radius = match section {
        Section::Round(radius) => radius,
        Section::Bevel { base, other, .. } => base.max(other),
    };
    let touch = |at: Vec3, normals: [Vec3; 2], index: usize| -> Option<Vec3> {
        let dot = normals[0].dot(normals[1]);
        Some(match section {
            Section::Round(_) => {
                at + (normals[0] + normals[1]) * (side * radius / (1.0 + dot))
                    - normals[index] * (side * radius)
            }
            Section::Bevel { base_face, base, other } => {
                let across = (normals[1 - index] - normals[index] * dot).normalize()?;
                at + across * (side * if faces[index] == base_face { base } else { other })
            }
        })
    };
    // The ball rests on both faces all round, as along a line.
    for fraction in [0.02, 0.25, 0.5, 0.75, 0.98] {
        let at = point_at(fraction);
        let normals = normals_at(at)?;
        for index in 0..2 {
            let point = touch(at, normals, index)?.to_array();
            if super::classify::face_distance(body, faces[index], point, tolerance)? > tolerance {
                return None;
            }
        }
    }
    // The section in the plane through the axis at the edge's start.
    let radial = (start - centre - axis * (start - centre).dot(axis)).normalize()?;
    let plane =
        Plane::orthonormal(centre.to_array(), radial.to_array(), radial.cross(axis).to_array())?;
    let flat = |point: Vec3| plane.project(point.to_array());
    let clear = if convex { radius * 0.25 } else { 0.0 };
    let lift = |point: Vec3, index: usize| point + normals[index] * clear;
    let (first_at, second_at) = (touch(start, normals, 0)?, touch(start, normals, 1)?);
    let (corner, first, second, ball) = (
        flat(start + (normals[0] + normals[1]) * clear)?,
        flat(first_at)?,
        flat(second_at)?,
        flat(start + (normals[0] + normals[1]) * (side * radius / (1.0 + dot)))?,
    );
    let (first_out, second_out) = (flat(lift(first_at, 0))?, flat(lift(second_at, 1))?);
    let angle = |p: [f64; 2]| (p[1] - ball[1]).atan2(p[0] - ball[0]);
    let (a, b, c) = (angle(first), angle(second), angle(corner));
    let turn = |from: f64, to: f64| (to - from).rem_euclid(TAU);
    let (from, to) = if turn(a, c) < turn(a, b) { (a, b) } else { (b, a) };
    let line = |start, end| ocs_geom::geom2d::Curve::Line(ocs_geom::geom2d::Line { start, end });
    let arc = match section {
        Section::Round(_) => ocs_geom::geom2d::Curve::Arc(ocs_geom::geom2d::Arc {
            centre: ball,
            radius,
            start_angle: from,
            end_angle: from + turn(from, to),
        }),
        Section::Bevel { .. } => line(second, first),
    };
    let profile = if convex {
        vec![
            line(first, first_out),
            line(first_out, corner),
            line(corner, second_out),
            line(second_out, second),
            arc,
        ]
    } else {
        vec![line(corner, first), arc, line(second, corner)]
    };
    let tool = super::revolve(plane, &profile, centre.to_array(), axis.to_array(), span)?;
    let how = if convex {
        super::boolean::Operation::Difference
    } else {
        super::boolean::Operation::Union
    };
    let rounded = super::combine(body.clone(), tool.clone(), how, tolerance).ok()?;
    let sound = rounded.validate().is_empty() && !rounded.roots.is_empty();
    (sound && within_tool(body, &tool, &rounded)).then_some(rounded)
}

fn cut_along_edge(
    body: &Body,
    edge_key: EdgeKey,
    section: Section,
    tolerance: f64,
) -> Option<Body> {
    let edge = body.edges.get(edge_key)?;
    if !straight(body.curves.get(edge.curve)?) || edge.coedges.len() != 2 {
        return None;
    }
    let start = Vec3::from(body.vertices.get(edge.start)?.point);
    let end = Vec3::from(body.vertices.get(edge.end)?.point);
    let length = start.distance(end);
    let axis = (end - start).normalize()?;
    let face_of = |coedge| Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner);
    let faces = [face_of(edge.coedges[0])?, face_of(edge.coedges[1])?];
    let normal_of = |face_key: FaceKey| {
        let face = body.faces.get(face_key)?;
        let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
            return None;
        };
        let normal = Vec3::from(plane.normal()?);
        Some(if face.forward { normal } else { -normal })
    };
    let normals = [normal_of(faces[0])?, normal_of(faces[1])?];
    let dot = normals[0].dot(normals[1]);
    if dot.abs() > 1.0 - 1e-9 {
        return None;
    }
    // Convex where the solid is missing just behind the first face's plane
    // and in front of the second's; a point taken locally, as a far
    // interior point of a non-convex face can sit past the edge.
    let probe = start.lerp(end, 0.5) + (normals[1] - normals[0]) * (length * 0.01);
    let convex = super::classify::contains_point(body, probe.to_array(), tolerance)
        == super::Containment::Outside;
    // The ball's centre is a radius off both planes, on the solid's side of
    // a convex edge and outside a concave one; a bevel's ends lie set back
    // across each face from the edge.
    let side = if convex { -1.0 } else { 1.0 };
    let radius = match section {
        Section::Round(radius) => radius,
        Section::Bevel { base, other, .. } => base.max(other),
    };
    let offset = (normals[0] + normals[1]) * (side * radius / (1.0 + dot));
    let across = |index: usize| (normals[1 - index] - normals[index] * dot).normalize();
    let setback = |index: usize| match section {
        Section::Round(_) => radius,
        Section::Bevel { base_face, base, other } => {
            if faces[index] == base_face { base } else { other }
        }
    };
    let touch = |at: Vec3, index: usize| match section {
        Section::Round(_) => at + offset - normals[index] * (side * radius),
        Section::Bevel { .. } => {
            at + across(index).unwrap_or(Vec3::new(0.0, 0.0, 0.0)) * (side * setback(index))
        }
    };
    if matches!(section, Section::Bevel { .. }) && (across(0).is_none() || across(1).is_none()) {
        return None;
    }
    // The section square to the edge, at a start reached back past it for
    // a convex edge so the cut runs out through any slanted end face —
    // unless the solid goes on past that end (the edge stops at a wall),
    // where the cut stops square at the end instead.
    let reach = |at: Vec3, out: Vec3| {
        let probe = at + out * radius + (normals[0] + normals[1]) * (-0.25 * radius);
        let inside = super::classify::contains_point(body, probe.to_array(), tolerance);
        if convex && inside == super::Containment::Outside {
            radius * 2.0
        } else {
            0.0
        }
    };
    let (margin, beyond) = (reach(start, -axis), reach(end, axis));
    // The ball must rest on both faces all along, not run off either —
    // except over an end the cut runs out through, where a face slanting
    // away from the edge only trims the round.
    for (at, open) in [(0.02, margin > 0.0), (0.5, false), (0.98, beyond > 0.0)] {
        let along = start.lerp(end, at);
        for index in 0..2 {
            let point = touch(along, index).to_array();
            let gap = super::classify::face_distance(body, faces[index], point, tolerance)?;
            if gap > tolerance && !open {
                return None;
            }
        }
    }
    let origin = start - axis * margin;
    let plane = Plane::orthonormal(origin.to_array(), normals[0].to_array(), axis.to_array())?;
    let flat = |point: Vec3| plane.project(point.to_array());
    // The prism's flat sides stand a little off the two faces, outside a
    // convex edge: lying on the faces they overlapped them, and the
    // boolean refuses an overlap. The solid ends at the faces anyway, so
    // nothing more is cut.
    let clear = if convex { radius * 0.25 } else { 0.0 };
    let lift = |point: Vec3, index: usize| point + normals[index] * clear;
    let (corner, first, second, centre) = (
        flat(start + (normals[0] + normals[1]) * clear)?,
        flat(touch(start, 0))?,
        flat(touch(start, 1))?,
        flat(start + offset)?,
    );
    let first_out = flat(lift(touch(start, 0), 0))?;
    let second_out = flat(lift(touch(start, 1), 1))?;
    let angle = |p: [f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
    let (a, b, c) = (angle(first), angle(second), angle(corner));
    let span = |from: f64, to: f64| (to - from).rem_euclid(TAU);
    // The arc of the ball between its two touches facing the corner.
    let (from, to) = if span(a, c) < span(a, b) { (a, b) } else { (b, a) };
    let line = |start, end| ocs_geom::geom2d::Curve::Line(ocs_geom::geom2d::Line { start, end });
    let arc = match section {
        Section::Round(_) => ocs_geom::geom2d::Curve::Arc(ocs_geom::geom2d::Arc {
            centre,
            radius,
            start_angle: from,
            end_angle: from + span(from, to),
        }),
        Section::Bevel { .. } => line(second, first),
    };
    let profile = if convex {
        vec![
            line(first, first_out),
            line(first_out, corner),
            line(corner, second_out),
            line(second_out, second),
            arc,
        ]
    } else {
        vec![line(corner, first), arc, line(second, corner)]
    };
    let run = axis * (length + margin + beyond);
    let tool = super::extrude_region(plane, &[profile], run.to_array())?;
    let how = if convex {
        super::boolean::Operation::Difference
    } else {
        super::boolean::Operation::Union
    };
    let rounded = super::combine(body.clone(), tool.clone(), how, tolerance).ok()?;
    let sound = rounded.validate().is_empty() && !rounded.roots.is_empty();
    (sound && within_tool(body, &tool, &rounded)).then_some(rounded)
}

/// An edge next to an earlier fillet of the same radius: rebuild the sharp
/// body from its planar faces and round the earlier edges together with the
/// new ones, so the corner between them gets the same seam or patch an
/// all-at-once selection would. Earlier fillets of another radius, or a body
/// that is not the intersection of its face planes, are left to the caller's
/// error.
fn refillet_from_sharp(body: &Body, selected: &[EdgeKey], radius: f64) -> Option<Body> {
    let tolerance = operation_tolerance(&[body]);
    let existing = existing_fillets(body)?;
    if existing.is_empty()
        || existing
            .iter()
            .any(|fillet| (fillet.cylinder.radius - radius).abs() > tolerance)
    {
        return None;
    }
    let sharp = sharp_convex(body, tolerance)?;
    let mut edges = Vec::with_capacity(existing.len() + selected.len());
    for fillet in &existing {
        edges.push(sharp_edge(&sharp, fillet.sides)?);
    }
    for edge in selected {
        let frame = edge_frame(body, *edge)?;
        edges.push(sharp_edge(&sharp, [frame.first_normal, frame.second_normal])?);
    }
    fillet_convex(&sharp, &edges, radius).ok()
}

/// The convex body bounded by each distinct face plane once, or `None` when
/// the body is not that intersection.
fn sharp_convex(body: &Body, tolerance: f64) -> Option<Body> {
    let mut halfspaces: Vec<Halfspace> = Vec::new();
    for (_, face) in body.faces.iter() {
        let Some(Surface::Plane(plane)) = body.surfaces.get(face.surface) else {
            continue;
        };
        let mut normal = Vec3::from(plane.normal()?);
        if !face.forward {
            normal = -normal;
        }
        let offset = normal.dot(Vec3::from(plane.origin));
        // Coplanar faces bound the same halfspace once.
        if halfspaces.iter().any(|other| {
            other.normal.dot(normal) > 1.0 - 1e-8 && (other.offset - offset).abs() <= tolerance
        }) {
            continue;
        }
        halfspaces.push(Halfspace {
            origin: Vec3::from(plane.origin),
            normal,
            offset,
            added: false,
        });
    }
    // Only a convex body is the intersection of its face planes.
    if body.vertices.iter().any(|(_, vertex)| {
        let point = Vec3::from(vertex.point);
        halfspaces
            .iter()
            .any(|plane| plane.normal.dot(point) > plane.offset + tolerance)
    }) {
        return None;
    }
    // `convex_body` reports one marked face; which one does not matter here.
    halfspaces.first_mut()?.added = true;
    convex_body(&halfspaces, tolerance).map(|(sharp, _)| sharp)
}

/// The edge of a convex body named by the outward normals of its two faces.
fn sharp_edge(sharp: &Body, pair: [Vec3; 2]) -> Option<EdgeKey> {
    let same = |a: Vec3, b: Vec3| a.dot(b) > 1.0 - 1e-8;
    sharp.edges.iter().find_map(|(key, _)| {
        let frame = edge_frame(sharp, key)?;
        let normals = [frame.first_normal, frame.second_normal];
        ((same(normals[0], pair[0]) && same(normals[1], pair[1]))
            || (same(normals[0], pair[1]) && same(normals[1], pair[0])))
        .then_some(key)
    })
}

/// Rounds straight edges of a convex planar body, keeping earlier fillets.
fn fillet_convex(body: &Body, selected: &[EdgeKey], radius: f64) -> Result<Body, FilletError> {
    let mut selected = selected.to_vec();
    selected.sort_by_key(EdgeKey::slot);
    selected.dedup();
    let tolerance = operation_tolerance(&[body]);
    let existing = existing_fillets(body).ok_or(FilletError::UnsupportedExistingFillet)?;
    let old_corners = existing_corners(body).ok_or(FilletError::UnsupportedExistingFillet)?;
    validate_supported_body(body, &existing, tolerance)?;

    let mut frames = Vec::with_capacity(selected.len());
    for edge in &selected {
        validate_selected_edge(body, *edge)?;
        let frame = edge_frame(body, *edge).ok_or(FilletError::DegenerateGeometry(*edge))?;
        frames.push((*edge, frame));
    }
    reject_interacting_regions(body, &selected, radius, tolerance)?;

    let mut halfspaces = frames[0].1.halfspaces.clone();
    let mut blends = Vec::with_capacity(frames.len());
    for (edge, frame) in &frames {
        let (cut, cylinder) = fillet_geometry(frame, radius, tolerance)
            .ok_or(FilletError::DegenerateGeometry(*edge))?;
        halfspaces.push(cut);
        blends.push((*edge, cut, cylinder));
    }
    let corners = corner_blends(body, &selected, radius, tolerance)?;
    halfspaces.extend(corners.iter().map(|(cut, _)| *cut));
    let (mut result, _) = convex_body(&halfspaces, tolerance)
        .ok_or(FilletError::RadiusTooLargeOrInteracting)?;

    let mut occupied = HashSet::new();
    let mut existing_faces = Vec::with_capacity(existing.len());
    for old in &existing {
        let face = face_on(&result, old.cut, tolerance)
            .ok_or(FilletError::RadiusTooLargeOrInteracting)?;
        if !occupied.insert(face) {
            return Err(FilletError::RadiusTooLargeOrInteracting);
        }
        existing_faces.push(face);
    }
    let mut targets = Vec::with_capacity(blends.len());
    for (edge, cut, cylinder) in blends {
        let face = face_on(&result, cut, tolerance)
            .ok_or(FilletError::RadiusTooLargeOrInteracting)?;
        if !occupied.insert(face) {
            return Err(FilletError::RadiusTooLargeOrInteracting);
        }
        if existing_faces
            .iter()
            .any(|existing| faces_touch(&result, *existing, face))
        {
            return Err(FilletError::RadiusTooLargeOrInteracting);
        }
        targets.push((edge, face, cylinder));
    }
    restore_fillets(&mut result, &existing, &old_corners, tolerance)
        .ok_or(FilletError::UnsupportedExistingFillet)?;
    // Assign every support first: a shared seam must see both true surfaces,
    // rather than the temporary halfspace faces used to build connectivity.
    for (_, face, cylinder) in &targets {
        let surface = result.faces.get(*face).ok_or(FilletError::InvalidResult)?.surface;
        *result.surfaces.get_mut(surface).ok_or(FilletError::InvalidResult)? =
            Surface::Cylinder(*cylinder);
    }
    for (cut, sphere) in corners {
        let face = face_on(&result, cut, tolerance)
            .ok_or(FilletError::RadiusTooLargeOrInteracting)?;
        let surface = result.faces.get(face).ok_or(FilletError::InvalidResult)?.surface;
        *result.surfaces.get_mut(surface).ok_or(FilletError::InvalidResult)? =
            Surface::Sphere(sphere);
    }
    for (edge, face, cylinder) in targets {
        if round_face(&mut result, face, &cylinder, true, tolerance)
            .filter(|rounded| *rounded == 2)
            .is_none()
        {
            return Err(FilletError::UnsupportedEndCondition(edge));
        }
    }
    if !result.validate().is_empty() || result.worst_vertex_gap() > tolerance
        || !blend_boundaries_fit(&result, tolerance)
    {
        return Err(FilletError::InvalidResult);
    }
    Ok(result)
}

/// The largest radius, up to `radius`, the edges are found to round to
/// together — what a refusal of `radius` can name instead. `None` where
/// nothing was found to fit.
///
/// Straight edges between planes start from their band's room: a round of
/// radius `r` touches each face `r / tan(phi / 2)` from its edge, `phi` the
/// angle between the faces' directions away from it, so a face with room
/// `s` across from the edge takes radii below `s tan(phi / 2)` (after
/// Solidcore's `band_room`; see `THIRD_PARTY.md`). The rest is found by
/// halving between nothing and that.
pub fn fillet_fit(body: &Body, edges: &[EdgeKey], radius: f64) -> Option<f64> {
    let upper = edges
        .iter()
        .filter_map(|edge| band_room(body, *edge))
        .map(|(rooms, angle)| rooms[0].1.min(rooms[1].1) * (angle * 0.5).tan())
        .fold(f64::INFINITY, f64::min);
    largest_fit(radius, upper, |size| fillet_edges(body, edges, size).is_ok())
}

/// The largest base distance, up to `base_distance`, the edges are found to
/// bevel to together, the other distance kept in proportion: what a refusal
/// can name instead. `None` where nothing was found to fit. A bevel's
/// distances are its setbacks, so a face's room across from its edge bounds
/// its distance directly.
pub fn chamfer_fit(
    body: &Body,
    edges: &[EdgeKey],
    base_face: FaceKey,
    base_distance: f64,
    other_distance: f64,
) -> Option<f64> {
    let ratio = other_distance / base_distance;
    let upper = edges
        .iter()
        .filter_map(|edge| band_room(body, *edge))
        .flat_map(|(rooms, _)| rooms)
        .map(|(face, room)| if face == base_face { room } else { room / ratio })
        .fold(f64::INFINITY, f64::min);
    largest_fit(base_distance, upper, |size| {
        chamfer_edges(body, edges, base_face, size, size * ratio).is_ok()
    })
}

/// The largest size up to `asked` that `fits`, below the open bound `upper`:
/// tried just under the bound first, then halved towards what fits.
fn largest_fit(asked: f64, upper: f64, fits: impl Fn(f64) -> bool) -> Option<f64> {
    if !asked.is_finite() || asked <= 0.0 {
        return None;
    }
    // A round as wide as its face leaves a sliver: stay a little short.
    let mut high = asked.min(upper * (1.0 - 1e-3));
    if high > 0.0 && fits(high) {
        return Some(high);
    }
    // ponytail: up to 12 more blends, each a full attempt; a closed-form
    // fit per corner would cut this where refusals need to be quick.
    let mut low = 0.0;
    for _ in 0..12 {
        let middle = 0.5 * (low + high);
        if fits(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low > 0.0).then_some(low)
}

/// The room a straight edge between two planes has on each face: how far
/// from it, square to it and into the face, the face's boundary is first
/// met — the least of three stations along the edge — and the angle
/// between the two faces' directions away from it.
fn band_room(body: &Body, edge_key: EdgeKey) -> Option<([(FaceKey, f64); 2], f64)> {
    let edge = body.edges.get(edge_key)?;
    if !straight(body.curves.get(edge.curve)?) || edge.coedges.len() != 2 {
        return None;
    }
    let start = Vec3::from(body.vertices.get(edge.start)?.point);
    let end = Vec3::from(body.vertices.get(edge.end)?.point);
    let axis = (end - start).normalize()?;
    let length = start.distance(end);
    let face_of = |coedge| Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner);
    let mut rooms = [(face_of(edge.coedges[0])?, 0.0), (face_of(edge.coedges[1])?, 0.0)];
    let mut inward = [Vec3::ZERO; 2];
    for (index, coedge_key) in edge.coedges.iter().enumerate() {
        let coedge = body.coedges.get(*coedge_key)?;
        let face_key = rooms[index].0;
        let face = body.faces.get(face_key)?;
        let Surface::Plane(plane) = body.surfaces.get(face.surface)? else { return None };
        let normal = Vec3::from(plane.normal()?) * if face.forward { 1.0 } else { -1.0 };
        // A face lies left of the way its loops walk, seen from outside.
        let walk = if coedge.forward { axis } else { -axis };
        let into = normal.cross(walk).normalize()?;
        inward[index] = into;
        let mut room = f64::INFINITY;
        for station in [0.02, 0.5, 0.98] {
            let from = start + axis * (length * station);
            for other in body.face_coedges(face_key) {
                let other = body.coedges.get(other)?.edge;
                if other == edge_key {
                    continue;
                }
                let node = body.edges.get(other)?;
                let curve = body.curves.get(node.curve)?;
                let steps = if straight(curve) { 1 } else { 32 };
                let at = |step: usize| {
                    let t = node.start_parameter
                        + (node.end_parameter - node.start_parameter) * step as f64 / steps as f64;
                    let offset = Vec3::from(curve.point_at(t)) - from;
                    (offset.dot(into), offset.dot(axis))
                };
                for step in 0..steps {
                    let ((x0, y0), (x1, y1)) = (at(step), at(step + 1));
                    if (y0 > 0.0 && y1 > 0.0) || (y0 < 0.0 && y1 < 0.0) || y0 == y1 {
                        continue;
                    }
                    let x = x0 + (x1 - x0) * y0 / (y0 - y1);
                    if x > length * 1e-9 {
                        room = room.min(x);
                    }
                }
            }
        }
        rooms[index].1 = room;
    }
    let angle = inward[0].dot(inward[1]).clamp(-1.0, 1.0).acos();
    rooms.iter().all(|(_, room)| room.is_finite()).then_some((rooms, angle))
}

/// Moves a planar face while extending or trimming its neighbouring surfaces.
pub fn presspull(body: &Body, face_key: FaceKey, distance: f64) -> Option<Body> {
    super::presspull_face(body, face_key, distance, super::PresspullMode::Offset)
}

fn circle_parameters(plane: &Plane, start: Vec3, end: Vec3) -> Option<(f64, f64)> {
    let parameter = |point: Vec3| {
        let local = plane.project(point.to_array())?;
        Some(local[1].atan2(local[0]))
    };
    let start = parameter(start)?;
    let span = (parameter(end)? - start).rem_euclid(TAU);
    (span > 1e-12).then_some((start, start + span))
}

fn cut_halfspace(frame: &EdgeFrame, setback: f64, tolerance: f64) -> Option<Halfspace> {
    cut_halfspace_distances(frame, setback, setback, tolerance)
}

fn cut_halfspace_distances(
    frame: &EdgeFrame,
    first_setback: f64,
    second_setback: f64,
    tolerance: f64,
) -> Option<Halfspace> {
    if !first_setback.is_finite()
        || !second_setback.is_finite()
        || first_setback <= 0.0
        || second_setback <= 0.0
    {
        return None;
    }
    let first = frame.point + frame.first_inward * first_setback;
    let second = frame.point + frame.second_inward * second_setback;
    let chord = second - first;
    let mut normal = frame.axis.cross(chord).normalize()?;
    if normal.dot(frame.first_normal + frame.second_normal) < 0.0 {
        normal = -normal;
    }
    let offset = 0.5 * (normal.dot(first) + normal.dot(second));
    let added = Halfspace {
        origin: first,
        normal,
        offset,
        added: true,
    };
    if normal.dot(frame.point) <= offset + tolerance {
        return None;
    }
    Some(added)
}

fn fillet_geometry(
    frame: &EdgeFrame,
    radius: f64,
    tolerance: f64,
) -> Option<(Halfspace, Cylinder)> {
    let normals_angle = frame
        .first_normal
        .dot(frame.second_normal)
        .clamp(-1.0, 1.0)
        .acos();
    let interior = PI - normals_angle;
    let setback = radius / (interior * 0.5).tan();
    let cut = cut_halfspace(frame, setback, tolerance)?;
    if cut.normal.dot(frame.first_normal + frame.second_normal) <= 0.0 {
        return None;
    }
    let tangent = frame.point + frame.first_inward * setback;
    let centre = tangent - frame.first_normal * radius;
    let base = Plane::orthonormal(
        centre.to_array(),
        frame.first_normal.to_array(),
        frame.axis.to_array(),
    )?;
    Some((cut, Cylinder { base, radius }))
}

fn edge_frame(body: &Body, edge_key: EdgeKey) -> Option<EdgeFrame> {
    let edge = body.edges.get(edge_key)?;
    if !matches!(body.curves.get(edge.curve)?, Curve3::Line(_)) || edge.coedges.len() != 2 {
        return None;
    }
    let start = Vec3::from(body.vertices.get(edge.start)?.point);
    let end = Vec3::from(body.vertices.get(edge.end)?.point);
    let axis = (end - start).normalize()?;
    let face_of = |coedge| {
        let loop_key = body.coedges.get(coedge)?.owner;
        Some(body.loops.get(loop_key)?.owner)
    };
    let faces = [face_of(edge.coedges[0])?, face_of(edge.coedges[1])?];
    let plane_of = |face_key| {
        let face = body.faces.get(face_key)?;
        let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
            return None;
        };
        let mut normal = Vec3::from(plane.normal()?);
        if !face.forward {
            normal = -normal;
        }
        Some(normal)
    };
    let first_normal = plane_of(faces[0])?;
    let second_normal = plane_of(faces[1])?;
    let dot = first_normal.dot(second_normal);
    if dot.abs() > 1.0 - 1e-9 {
        return None;
    }
    let first_inward = -(second_normal - first_normal * dot).normalize()?;
    let second_inward = -(first_normal - second_normal * dot).normalize()?;
    let mut halfspaces = Vec::new();
    for (_, face) in body.faces.iter() {
        let Some(Surface::Plane(plane)) = body.surfaces.get(face.surface) else {
            continue;
        };
        let mut normal = Vec3::from(plane.normal()?);
        if !face.forward {
            normal = -normal;
        }
        halfspaces.push(Halfspace {
            origin: Vec3::from(plane.origin),
            normal,
            offset: normal.dot(Vec3::from(plane.origin)),
            added: false,
        });
    }
    halfspaces.extend(existing_fillets(body)?.into_iter().map(|fillet| fillet.cut));
    halfspaces.extend(existing_corners(body)?.into_iter().map(|(cut, _)| cut));
    Some(EdgeFrame {
        point: start,
        axis,
        faces,
        first_normal,
        second_normal,
        first_inward,
        second_inward,
        halfspaces,
    })
}

fn validate_selected_edge(body: &Body, edge_key: EdgeKey) -> Result<(), FilletError> {
    let edge = body.edges.get(edge_key).ok_or(FilletError::UnknownEdge)?;
    if !matches!(body.curves.get(edge.curve), Some(Curve3::Line(_))) {
        return Err(FilletError::UnsupportedEdgeCurve(edge_key));
    }
    if edge.coedges.len() != 2 {
        return Err(FilletError::NonManifoldEdge(edge_key));
    }
    let start = body
        .vertices
        .get(edge.start)
        .ok_or(FilletError::InvalidBodyTopology)?;
    let end = body
        .vertices
        .get(edge.end)
        .ok_or(FilletError::InvalidBodyTopology)?;
    if (Vec3::from(end.point) - Vec3::from(start.point))
        .normalize()
        .is_none()
    {
        return Err(FilletError::DegenerateGeometry(edge_key));
    }
    for coedge in &edge.coedges {
        let coedge = body
            .coedges
            .get(*coedge)
            .ok_or(FilletError::InvalidBodyTopology)?;
        let ring = body
            .loops
            .get(coedge.owner)
            .ok_or(FilletError::InvalidBodyTopology)?;
        let face = body
            .faces
            .get(ring.owner)
            .ok_or(FilletError::InvalidBodyTopology)?;
        let Some(Surface::Plane(plane)) = body.surfaces.get(face.surface) else {
            return Err(FilletError::UnsupportedAdjacentSurface(edge_key));
        };
        if plane.normal().is_none() {
            return Err(FilletError::DegenerateGeometry(edge_key));
        }
    }
    Ok(())
}

fn corner_blends(
    body: &Body,
    selected: &[EdgeKey],
    radius: f64,
    tolerance: f64,
) -> Result<Vec<(Halfspace, Sphere)>, FilletError> {
    let mut owner = HashMap::<_, Vec<EdgeKey>>::new();
    for edge_key in selected {
        let edge = body.edges.get(*edge_key).ok_or(FilletError::UnknownEdge)?;
        for vertex in [edge.start, edge.end] {
            owner.entry(vertex).or_default().push(*edge_key);
        }
    }
    let mut vertices = owner.into_iter().collect::<Vec<_>>();
    vertices.sort_by_key(|(vertex, _)| vertex.slot());
    let mut corners = Vec::new();
    for (vertex, edges) in vertices {
        if edges.len() < 2 { continue; }
        let unsupported = FilletError::AdjacentSelections(edges[0], edges[1]);
        let incident = body.edges.iter().filter(|(_, edge)|
            edge.start == vertex || edge.end == vertex).count();
        if incident != 3 || edges.len() > 3 { return Err(unsupported); }
        let mut normals = Vec::<Vec3>::new();
        for edge in &edges {
            let frame = edge_frame(body, *edge).ok_or(unsupported)?;
            for normal in [frame.first_normal, frame.second_normal] {
                if normals.iter().all(|other| normal.dot(*other) < 1.0 - 1e-8) {
                    normals.push(normal);
                }
            }
        }
        if normals.len() != 3 {
            return Err(unsupported);
        }
        let point = Vec3::from(body.vertices.get(vertex)
            .ok_or(FilletError::InvalidBodyTopology)?.point);
        if edges.len() == 2 {
            // Two rounds meet on the edge left sharp only where both reach
            // it at the same place: each touches the face it alone rests
            // on `r (1 - cos)` of their shared face's angle to it from that
            // face, so the two angles at the shared face must agree.
            let frames = [edge_frame(body, edges[0]).ok_or(unsupported)?,
                edge_frame(body, edges[1]).ok_or(unsupported)?];
            let cosine = |frame: &EdgeFrame| frame.first_normal.dot(frame.second_normal);
            if (cosine(&frames[0]) - cosine(&frames[1])).abs() > 1e-9 {
                return Err(unsupported);
            }
            continue;
        }
        // The ball a radius in from all three planes, `n_i . c = n_i . p - r`
        // solved by Cramer's rule (after Solidcore's `planes_meet`; see
        // `THIRD_PARTY.md`); it touches each at `c + r n_i`, and the
        // corner is the sphere's triangle between those touches.
        let [a, b, c] = [normals[0], normals[1], normals[2]];
        let determinant = a.dot(b.cross(c));
        if determinant.abs() < 1e-9 {
            return Err(unsupported);
        }
        let centre = point - (b.cross(c) + c.cross(a) + a.cross(b)) * (radius / determinant);
        let touches = [centre + a * radius, centre + b * radius, centre + c * radius];
        let mut normal = (touches[1] - touches[0]).cross(touches[2] - touches[0])
            .normalize().ok_or(unsupported)?;
        if normal.dot(point - touches[0]) < 0.0 {
            normal = -normal;
        }
        let origin = touches[0];
        if normal.dot(point - origin) <= tolerance || normal.dot(origin - centre) <= tolerance {
            return Err(unsupported);
        }
        // Keep both parameter poles and the longitude seam outside the
        // trimmed patch so downstream meshing has a regular UV domain:
        // `a - b` lies in the triangle's plane and outside its cone.
        let pole = (a - b).normalize().ok_or(unsupported)?;
        let frame = Plane::orthonormal(centre.to_array(), normal.to_array(),
            pole.to_array()).ok_or(unsupported)?;
        corners.push((Halfspace { origin, normal, offset: normal.dot(origin), added: true },
            Sphere { frame, radius }));
    }
    Ok(corners)
}

fn reject_interacting_regions(
    body: &Body,
    selected: &[EdgeKey],
    radius: f64,
    tolerance: f64,
) -> Result<(), FilletError> {
    let segments = selected
        .iter()
        .map(|edge_key| {
            let edge = body.edges.get(*edge_key).ok_or(FilletError::UnknownEdge)?;
            let start = body
                .vertices
                .get(edge.start)
                .ok_or(FilletError::InvalidBodyTopology)?;
            let end = body
                .vertices
                .get(edge.end)
                .ok_or(FilletError::InvalidBodyTopology)?;
            Ok((Vec3::from(start.point), Vec3::from(end.point)))
        })
        .collect::<Result<Vec<_>, FilletError>>()?;
    for first in 0..segments.len() {
        for second in first + 1..segments.len() {
            let a = body.edges.get(selected[first]).ok_or(FilletError::UnknownEdge)?;
            let b = body.edges.get(selected[second]).ok_or(FilletError::UnknownEdge)?;
            if [a.start, a.end].iter().any(|v| *v == b.start || *v == b.end) {
                continue;
            }
            if segment_distance(segments[first], segments[second])
                <= radius * 2.0 + tolerance
            {
                return Err(FilletError::RadiusTooLargeOrInteracting);
            }
        }
    }
    Ok(())
}

fn segment_distance(first: (Vec3, Vec3), second: (Vec3, Vec3)) -> f64 {
    let first_direction = first.1 - first.0;
    let second_direction = second.1 - second.0;
    let offset = first.0 - second.0;
    let first_length = first_direction.dot(first_direction);
    let second_length = second_direction.dot(second_direction);
    let cross = first_direction.dot(second_direction);
    let first_offset = first_direction.dot(offset);
    let second_offset = second_direction.dot(offset);
    let denominator = first_length * second_length - cross * cross;
    let mut first_parameter;
    let mut second_parameter;
    if denominator > f64::EPSILON * first_length.max(second_length).max(1.0) {
        first_parameter = (cross * second_offset - second_length * first_offset) / denominator;
        first_parameter = first_parameter.clamp(0.0, 1.0);
    } else {
        first_parameter = 0.0;
    }
    second_parameter = (cross * first_parameter + second_offset) / second_length;
    if second_parameter < 0.0 {
        second_parameter = 0.0;
        first_parameter = (-first_offset / first_length).clamp(0.0, 1.0);
    } else if second_parameter > 1.0 {
        second_parameter = 1.0;
        first_parameter = ((cross - first_offset) / first_length).clamp(0.0, 1.0);
    }
    (offset + first_direction * first_parameter - second_direction * second_parameter).length()
}

fn validate_supported_body(
    body: &Body,
    existing: &[ExistingFillet],
    tolerance: f64,
) -> Result<(), FilletError> {
    if body.roots.len() != 1 {
        return Err(FilletError::UnsupportedBodyTopology);
    }
    let lump = body
        .lumps
        .get(body.roots[0])
        .ok_or(FilletError::InvalidBodyTopology)?;
    if lump.shells.len() != 1 {
        return Err(FilletError::UnsupportedBodyTopology);
    }
    let shell = body
        .shells
        .get(lump.shells[0])
        .ok_or(FilletError::InvalidBodyTopology)?;
    if shell.faces.len() != body.faces.len()
        || body.faces.iter().any(|(_, face)| face.loops.len() != 1)
    {
        return Err(FilletError::UnsupportedBodyTopology);
    }
    let cylinders = body
        .faces
        .iter()
        .filter(|(_, face)| {
            matches!(body.surfaces.get(face.surface), Some(Surface::Cylinder(_)))
        })
        .count();
    if cylinders != existing.len() {
        return Err(FilletError::UnsupportedExistingFillet);
    }
    if body.faces.iter().any(|(_, face)| {
        !matches!(
            body.surfaces.get(face.surface),
            Some(Surface::Plane(_) | Surface::Cylinder(_) | Surface::Sphere(_))
        )
    }) {
        return Err(FilletError::UnsupportedBodySurface);
    }

    for (_, face) in body.faces.iter() {
        let Some(Surface::Plane(plane)) = body.surfaces.get(face.surface) else {
            continue;
        };
        let Some(normal) = plane.normal().map(Vec3::from) else {
            return Err(FilletError::UnsupportedBodySurface);
        };
        let normal = if face.forward { normal } else { -normal };
        let offset = normal.dot(Vec3::from(plane.origin));
        if body.vertices.iter().any(|(_, vertex)| {
            normal.dot(Vec3::from(vertex.point)) > offset + tolerance
        }) {
            return Err(FilletError::NonConvexBody);
        }
    }
    Ok(())
}

fn existing_fillets(body: &Body) -> Option<Vec<ExistingFillet>> {
    let mut found = Vec::new();
    for (face_key, face) in body.faces.iter() {
        let Surface::Cylinder(cylinder) = body.surfaces.get(face.surface)? else {
            continue;
        };
        let axis = Vec3::from(cylinder.base.normal()?);
        let mut side_normals = Vec::new();
        let mut points = Vec::new();
        let mut line_edges = 0usize;
        for coedge_key in body.face_coedges(face_key) {
            let coedge = body.coedges.get(coedge_key)?;
            let edge = body.edges.get(coedge.edge)?;
            points.push(Vec3::from(body.vertices.get(edge.start)?.point));
            if !matches!(body.curves.get(edge.curve)?, Curve3::Line(_)) {
                continue;
            }
            line_edges += 1;
            let other_face = edge.coedges.iter().find_map(|candidate| {
                let candidate = body.coedges.get(*candidate)?;
                let owner = body.loops.get(candidate.owner)?.owner;
                (owner != face_key).then_some(owner)
            })?;
            let other = body.faces.get(other_face)?;
            let Surface::Plane(plane) = body.surfaces.get(other.surface)? else {
                return None;
            };
            let mut normal = Vec3::from(plane.normal()?);
            if !other.forward {
                normal = -normal;
            }
            if normal.dot(axis).abs() <= 1e-8 {
                side_normals.push(normal);
            }
        }
        if line_edges != 2 || side_normals.len() != 2 || points.is_empty() {
            continue;
        }
        let normal = (side_normals[0] + side_normals[1]).normalize()?;
        let offset = points.iter().map(|point| normal.dot(*point)).sum::<f64>()
            / points.len() as f64;
        found.push(ExistingFillet {
            cut: Halfspace {
                origin: points[0],
                normal,
                offset,
                added: true,
            },
            cylinder: cylinder.clone(),
            forward: face.forward,
            sides: [side_normals[0], side_normals[1]],
        });
    }
    Some(found)
}

fn restore_fillets(
    body: &mut Body,
    fillets: &[ExistingFillet],
    corners: &[(Halfspace, Sphere)],
    tolerance: f64,
) -> Option<()> {
    let faces = fillets.iter().map(|fillet| face_on(body, fillet.cut, tolerance))
        .collect::<Option<Vec<_>>>()?;
    for (cut, sphere) in corners {
        let face = face_on(body, *cut, tolerance)?;
        let surface = body.faces.get(face)?.surface;
        *body.surfaces.get_mut(surface)? = Surface::Sphere(*sphere);
    }
    for (fillet, face) in fillets.iter().zip(&faces) {
        let surface = body.faces.get(*face)?.surface;
        *body.surfaces.get_mut(surface)? = Surface::Cylinder(fillet.cylinder);
    }
    for (fillet, face) in fillets.iter().zip(faces) {
        if round_face(body, face, &fillet.cylinder, fillet.forward, tolerance)? != 2 {
            return None;
        }
    }
    Some(())
}

fn face_on(body: &Body, halfspace: Halfspace, tolerance: f64) -> Option<FaceKey> {
    body.faces.iter().find_map(|(key, face)| {
        let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
            return None;
        };
        let mut normal = Vec3::from(plane.normal()?);
        if !face.forward {
            normal = -normal;
        }
        (normal.dot(halfspace.normal) > 1.0 - 1e-8
            && (normal.dot(Vec3::from(plane.origin)) - halfspace.offset).abs() <= tolerance)
            .then_some(key)
    })
}

fn validate_chamfer_edge(body: &Body, edge_key: EdgeKey) -> Result<(), ChamferError> {
    let edge = body.edges.get(edge_key).ok_or(ChamferError::UnknownEdge)?;
    if !matches!(body.curves.get(edge.curve), Some(Curve3::Line(_))) {
        return Err(ChamferError::UnsupportedEdgeCurve(edge_key));
    }
    if edge.coedges.len() != 2 {
        return Err(ChamferError::NonManifoldEdge(edge_key));
    }
    let start = body
        .vertices
        .get(edge.start)
        .ok_or(ChamferError::InvalidBodyTopology)?;
    let end = body
        .vertices
        .get(edge.end)
        .ok_or(ChamferError::InvalidBodyTopology)?;
    if (Vec3::from(end.point) - Vec3::from(start.point))
        .normalize()
        .is_none()
    {
        return Err(ChamferError::DegenerateGeometry(edge_key));
    }
    for coedge in &edge.coedges {
        let coedge = body
            .coedges
            .get(*coedge)
            .ok_or(ChamferError::InvalidBodyTopology)?;
        let ring = body
            .loops
            .get(coedge.owner)
            .ok_or(ChamferError::InvalidBodyTopology)?;
        let face = body
            .faces
            .get(ring.owner)
            .ok_or(ChamferError::InvalidBodyTopology)?;
        let Some(Surface::Plane(plane)) = body.surfaces.get(face.surface) else {
            return Err(ChamferError::UnsupportedAdjacentSurface(edge_key));
        };
        if plane.normal().is_none() {
            return Err(ChamferError::DegenerateGeometry(edge_key));
        }
    }
    Ok(())
}

fn validate_chamfer_body(
    body: &Body,
    existing: &[ExistingFillet],
    tolerance: f64,
) -> Result<(), ChamferError> {
    validate_supported_body(body, existing, tolerance).map_err(|error| match error {
        FilletError::InvalidBodyTopology => ChamferError::InvalidBodyTopology,
        FilletError::UnsupportedBodyTopology => ChamferError::UnsupportedBodyTopology,
        FilletError::UnsupportedBodySurface => ChamferError::UnsupportedBodySurface,
        FilletError::NonConvexBody => ChamferError::NonConvexBody,
        FilletError::UnsupportedExistingFillet => ChamferError::UnsupportedExistingFillet,
        _ => ChamferError::InvalidBodyTopology,
    })
}

fn faces_touch(body: &Body, first: FaceKey, second: FaceKey) -> bool {
    let vertices = |face| {
        body.face_coedges(face)
            .into_iter()
            .filter_map(|coedge| body.coedges.get(coedge))
            .filter_map(|coedge| body.edges.get(coedge.edge))
            .flat_map(|edge| [edge.start, edge.end])
            .collect::<HashSet<_>>()
    };
    let first = vertices(first);
    vertices(second).iter().any(|vertex| first.contains(vertex))
}

fn round_face(
    body: &mut Body,
    face: FaceKey,
    cylinder: &Cylinder,
    forward: bool,
    tolerance: f64,
) -> Option<usize> {
    let axis = Vec3::from(cylinder.base.normal()?);
    let centre = Vec3::from(cylinder.base.origin);
    let surface = body.faces.get(face)?.surface;
    *body.surfaces.get_mut(surface)? = Surface::Cylinder(cylinder.clone());
    body.faces.get_mut(face)?.forward = forward;
    let loop_key = *body.faces.get(face)?.loops.first()?;
    let edges = body
        .loops
        .get(loop_key)?
        .coedges
        .iter()
        .filter_map(|coedge| body.coedges.get(*coedge).map(|coedge| coedge.edge))
        .collect::<Vec<_>>();
    let mut rounded = 0usize;
    for edge_key in edges {
        let edge = body.edges.get(edge_key)?.clone();
        let start = Vec3::from(body.vertices.get(edge.start)?.point);
        let end = Vec3::from(body.vertices.get(edge.end)?.point);
        if (end - start).normalize()?.dot(axis).abs() > 1.0 - 1e-8 {
            continue;
        }
        let other_face = edge.coedges.iter().find_map(|key| {
            let owner = body.loops.get(body.coedges.get(*key)?.owner)?.owner;
            (owner != face).then_some(owner)
        })?;
        let other = body.faces.get(other_face)?;
        if let Surface::Cylinder(other_cylinder) = body.surfaces.get(other.surface)? {
            let curve = cylinder_seam(cylinder, other_cylinder, start, end, tolerance)?;
            set_round_edge(body, edge_key, curve, tolerance)?;
            rounded += 1;
            continue;
        }
        let start_height = (start - centre).dot(axis);
        let end_height = (end - centre).dot(axis);
        if (start_height - end_height).abs() > tolerance {
            // An oblique end plane, such as a neighbouring chamfer, cuts the
            // cylinder in an ellipse.
            let Surface::Plane(plane) = body.surfaces.get(other.surface)? else {
                return None;
            };
            let curve = cylinder_section(cylinder, plane)?;
            set_round_edge(body, edge_key, curve, tolerance)?;
            rounded += 1;
            continue;
        }
        let cross_centre = centre + axis * ((start_height + end_height) * 0.5);
        let mut circle_plane = Plane::orthonormal(
            cross_centre.to_array(),
            cylinder.base.x_axis,
            axis.to_array(),
        )?;
        let mut parameters = circle_parameters(&circle_plane, start, end)?;
        if parameters.1 - parameters.0 > PI + 1e-9 {
            circle_plane = Plane::orthonormal(
                cross_centre.to_array(),
                cylinder.base.x_axis,
                (-axis).to_array(),
            )?;
            parameters = circle_parameters(&circle_plane, start, end)?;
        }
        *body.curves.get_mut(edge.curve)? = Curve3::Circle(Circle3 {
            plane: circle_plane,
            radius: cylinder.radius,
        });
        let edge = body.edges.get_mut(edge_key)?;
        edge.start_parameter = parameters.0;
        edge.end_parameter = parameters.1;
        rounded += 1;
    }
    Some(rounded)
}

/// Equal-radius cylinders at a corner intersect in two planar ellipses.
/// Select the bisector containing the proxy endpoints, then retain the short
/// arc; this is an exact intersection, not a sampled approximation.
fn cylinder_seam(
    first: &Cylinder,
    second: &Cylinder,
    start: Vec3,
    end: Vec3,
    tolerance: f64,
) -> Option<Curve3> {
    if (first.radius - second.radius).abs() > tolerance { return None; }
    let a = Vec3::from(first.base.normal()?);
    let b = Vec3::from(second.base.normal()?);
    let cosine = a.dot(b);
    if cosine.abs() > 1.0 - 1e-8 { return None; }
    let delta = Vec3::from(second.base.origin) - Vec3::from(first.base.origin);
    let along = (a.dot(delta) - cosine * b.dot(delta)) / (1.0 - cosine * cosine);
    let centre = Vec3::from(first.base.origin) + a * along;
    let second_delta = centre - Vec3::from(second.base.origin);
    if (second_delta - b * second_delta.dot(b)).length() > tolerance { return None; }
    for bisector in [a + b, a - b] {
        let normal = bisector.normalize()?;
        if normal.dot(start - centre).abs() > tolerance
            || normal.dot(end - centre).abs() > tolerance { continue; }
        let cosine = normal.dot(a).abs();
        if cosine < 1e-8 { continue; }
        let major = (a - normal * normal.dot(a)).normalize()?;
        let plane = Plane::orthonormal(centre.to_array(), major.to_array(), normal.to_array())?;
        return Some(Curve3::Ellipse(Ellipse3 {
            plane, major_radius: first.radius / cosine, minor_radius: first.radius,
        }));
    }
    None
}

/// Where a plane crossing the axis cuts a cylinder: a circle when square to
/// it, an ellipse otherwise.
pub(super) fn cylinder_section(cylinder: &Cylinder, plane: &Plane) -> Option<Curve3> {
    let axis = Vec3::from(cylinder.base.normal()?);
    let centre = Vec3::from(cylinder.base.origin);
    let normal = Vec3::from(plane.normal()?);
    let cosine = normal.dot(axis);
    if cosine.abs() < 1e-8 {
        return None;
    }
    let along = normal.dot(Vec3::from(plane.origin) - centre) / cosine;
    let origin = (centre + axis * along).to_array();
    let minor = axis.cross(normal);
    if minor.length() < 1e-9 {
        let plane = Plane::orthonormal(origin, cylinder.base.x_axis, normal.to_array())?;
        return Some(Curve3::Circle(Circle3 { plane, radius: cylinder.radius }));
    }
    let minor = minor.normalize()?;
    let plane = Plane::orthonormal(origin, normal.cross(minor).to_array(), normal.to_array())?;
    Some(Curve3::Ellipse(Ellipse3 {
        plane,
        major_radius: cylinder.radius / cosine.abs(),
        minor_radius: cylinder.radius,
    }))
}

pub(super) fn set_round_edge(
    body: &mut Body,
    key: EdgeKey,
    mut curve: Curve3,
    tolerance: f64,
) -> Option<()> {
    let edge = body.edges.get(key)?.clone();
    let start = body.vertices.get(edge.start)?.point;
    let end = body.vertices.get(edge.end)?.point;
    let mut begin = curve.parameter_at(start);
    let mut span = (curve.parameter_at(end) - begin).rem_euclid(TAU);
    if span > PI {
        let plane = match &mut curve {
            Curve3::Ellipse(ellipse) => &mut ellipse.plane,
            Curve3::Circle(circle) => &mut circle.plane,
            _ => return None,
        };
        plane.y_axis = (-Vec3::from(plane.y_axis)).to_array();
        begin = curve.parameter_at(start);
        span = (curve.parameter_at(end) - begin).rem_euclid(TAU);
    }
    if span <= 1e-12 || span > PI + 1e-8
        || Vec3::from(curve.point_at(begin)).distance(Vec3::from(start)) > tolerance
        || Vec3::from(curve.point_at(begin + span)).distance(Vec3::from(end)) > tolerance {
        return None;
    }
    *body.curves.get_mut(edge.curve)? = curve;
    let edge = body.edges.get_mut(key)?;
    edge.start_parameter = begin;
    edge.end_parameter = begin + span;
    Some(())
}

fn existing_corners(body: &Body) -> Option<Vec<(Halfspace, Sphere)>> {
    let tolerance = operation_tolerance(&[body]);
    let mut corners = Vec::new();
    for (key, face) in body.faces.iter() {
        let Surface::Sphere(sphere) = body.surfaces.get(face.surface)? else { continue; };
        if !face.forward || face.loops.len() != 1 { return None; }
        let centre = Vec3::from(sphere.frame.origin);
        let mut points = Vec::new();
        for coedge in body.face_coedges(key) {
            let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
            for vertex in [edge.start, edge.end] {
                let point = Vec3::from(body.vertices.get(vertex)?.point);
                if points.iter().all(|other: &Vec3| other.distance(point) > tolerance) {
                    points.push(point);
                }
            }
            let other = edge.coedges.iter().find_map(|coedge| {
                let owner = body.loops.get(body.coedges.get(*coedge)?.owner)?.owner;
                (owner != key).then_some(owner)
            })?;
            let Surface::Cylinder(cylinder) = body.surfaces.get(body.faces.get(other)?.surface)? else { return None; };
            let axis = Vec3::from(cylinder.base.normal()?);
            let offset = centre - Vec3::from(cylinder.base.origin);
            if (cylinder.radius - sphere.radius).abs() > tolerance
                || (offset - axis * offset.dot(axis)).length() > tolerance { return None; }
        }
        if points.len() != 3 { return None; }
        // The plane through the three touches, facing out of the ball: its
        // normal the sum of their directions only where the corner is square.
        let sum = points.iter().fold(Vec3::ZERO, |sum, point| sum + (*point - centre));
        let mut normal = (points[1] - points[0]).cross(points[2] - points[0]).normalize()?;
        if normal.dot(sum) < 0.0 {
            normal = -normal;
        }
        let origin = points[0];
        if points.iter().any(|point| normal.dot(*point - origin).abs() > tolerance
            || (point.distance(centre) - sphere.radius).abs() > tolerance) { return None; }
        // Framed as a new corner is, poles and seam off the patch: a file's
        // frame may put a pole on a corner, which meshes only through the
        // file's own parameter curves, gone once the face is rebuilt.
        let pole = (points[0] - points[1]).normalize()?;
        let frame = Plane::orthonormal(sphere.frame.origin, normal.to_array(), pole.to_array())?;
        let sphere = Sphere { frame, radius: sphere.radius };
        let plane = Halfspace { origin, normal, offset: normal.dot(origin), added: true };
        corners.push((plane, sphere));
    }
    Some(corners)
}

fn blend_boundaries_fit(body: &Body, tolerance: f64) -> bool {
    body.edges.iter().all(|(_, edge)| {
        let Some(curve) = body.curves.get(edge.curve) else { return false; };
        edge.coedges.iter().all(|key| {
            let Some(coedge) = body.coedges.get(*key) else { return false; };
            let Some(ring) = body.loops.get(coedge.owner) else { return false; };
            let Some(face) = body.faces.get(ring.owner) else { return false; };
            let Some(surface) = body.surfaces.get(face.surface) else { return false; };
            (0..=8).all(|i| {
                let t = edge.start_parameter
                    + (edge.end_parameter - edge.start_parameter) * i as f64 / 8.0;
                let distance = surface.distance_to(curve.point_at(t));
                distance.is_finite() && distance.abs() <= tolerance
            })
        })
    })
}

/// The convex body the planes bound, each given by a point on it and its
/// outward unit normal; `None` where they bound nothing, or the first of them
/// bounds none of it.
pub(super) fn bounded_by(planes: &[(Vec3, Vec3)], tolerance: f64) -> Option<Body> {
    let mut halfspaces: Vec<Halfspace> = planes
        .iter()
        .map(|(origin, normal)| Halfspace {
            origin: *origin,
            normal: *normal,
            offset: normal.dot(*origin),
            added: false,
        })
        .collect();
    halfspaces.first_mut()?.added = true;
    convex_body(&halfspaces, tolerance).map(|(body, _)| body)
}

fn convex_body(halfspaces: &[Halfspace], tolerance: f64) -> Option<(Body, FaceKey)> {
    let mut points = Vec::<Vec3>::new();
    for first in 0..halfspaces.len() {
        for second in first + 1..halfspaces.len() {
            for third in second + 1..halfspaces.len() {
                let Some(point) = intersection(
                    halfspaces[first],
                    halfspaces[second],
                    halfspaces[third],
                ) else {
                    continue;
                };
                if halfspaces
                    .iter()
                    .any(|plane| plane.normal.dot(point) > plane.offset + tolerance)
                {
                    continue;
                }
                if points
                    .iter()
                    .all(|other| other.distance(point) > tolerance)
                {
                    points.push(point);
                }
            }
        }
    }
    if points.len() < 4 {
        return None;
    }

    let mut body = Body::new();
    let lump = body.lumps.insert(Lump {
        shells: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let shell = body.shells.insert(Shell {
        faces: Vec::new(),
        owner: lump,
        provenance: Provenance::Synthesized,
    });
    let vertices = points
        .iter()
        .map(|point| {
            body.vertices.insert(Vertex {
                point: point.to_array(),
                provenance: Provenance::Synthesized,
            })
        })
        .collect::<Vec<_>>();
    let mut edge_map = HashMap::<(usize, usize), EdgeKey>::new();
    let mut added_face = None;
    for halfspace in halfspaces {
        let mut indices = points
            .iter()
            .enumerate()
            .filter_map(|(index, point)| {
                ((halfspace.normal.dot(*point) - halfspace.offset).abs() <= tolerance)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if indices.len() < 3 {
            continue;
        }
        let centre = indices
            .iter()
            .fold(Vec3::ZERO, |sum, index| sum + points[*index])
            / indices.len() as f64;
        let seed = if halfspace.normal.x.abs() < 0.8 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let x = halfspace.normal.cross(seed).normalize()?;
        let y = halfspace.normal.cross(x);
        indices.sort_by(|a, b| {
            let angle = |index: usize| {
                let delta = points[index] - centre;
                delta.dot(y).atan2(delta.dot(x))
            };
            angle(*a).total_cmp(&angle(*b))
        });
        remove_collinear(&mut indices, &points, tolerance);
        if indices.len() < 3 {
            continue;
        }
        let plane = Plane::orthonormal(
            halfspace.origin.to_array(),
            x.to_array(),
            halfspace.normal.to_array(),
        )?;
        let surface = body.surfaces.insert(Surface::Plane(plane));
        let face = body.faces.insert(Face {
            surface,
            forward: true,
            loops: Vec::new(),
            owner: shell,
            provenance: Provenance::Synthesized,
        });
        let ring = body.loops.insert(Loop {
            coedges: Vec::new(),
            owner: face,
            provenance: Provenance::Synthesized,
        });
        let mut coedges = Vec::with_capacity(indices.len());
        for position in 0..indices.len() {
            let from = indices[position];
            let to = indices[(position + 1) % indices.len()];
            let key = (from.min(to), from.max(to));
            let edge = if let Some(edge) = edge_map.get(&key).copied() {
                edge
            } else {
                let direction = points[key.1] - points[key.0];
                let curve = body.curves.insert(Curve3::Line(Line3 {
                    origin: points[key.0].to_array(),
                    direction: direction.to_array(),
                }));
                let edge = body.edges.insert(Edge {
                    curve,
                    start_parameter: 0.0,
                    end_parameter: 1.0,
                    start: vertices[key.0],
                    end: vertices[key.1],
                    coedges: Vec::new(),
                    provenance: Provenance::Synthesized,
                });
                edge_map.insert(key, edge);
                edge
            };
            let coedge = body.coedges.insert(Coedge {
                edge,
                forward: from == key.0,
                pcurve: None,
                owner: ring,
                provenance: Provenance::Synthesized,
            });
            body.edges.get_mut(edge)?.coedges.push(coedge);
            coedges.push(coedge);
        }
        body.loops.get_mut(ring)?.coedges = coedges;
        body.faces.get_mut(face)?.loops = vec![ring];
        body.shells.get_mut(shell)?.faces.push(face);
        if halfspace.added {
            added_face = Some(face);
        }
    }
    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    let added_face = added_face?;
    body.validate().is_empty().then_some((body, added_face))
}

fn intersection(first: Halfspace, second: Halfspace, third: Halfspace) -> Option<Vec3> {
    let denominator = first.normal.dot(second.normal.cross(third.normal));
    if denominator.abs() <= 1e-12 {
        return None;
    }
    Some(
        (second.normal.cross(third.normal) * first.offset
            + third.normal.cross(first.normal) * second.offset
            + first.normal.cross(second.normal) * third.offset)
            / denominator,
    )
}

fn remove_collinear(indices: &mut Vec<usize>, points: &[Vec3], tolerance: f64) {
    loop {
        let count = indices.len();
        let remove = (0..count).find(|index| {
            let before = points[indices[(index + count - 1) % count]];
            let here = points[indices[*index]];
            let after = points[indices[(index + 1) % count]];
            let first = here - before;
            let second = after - here;
            first.cross(second).length() <= tolerance * first.length().max(second.length())
        });
        let Some(index) = remove else {
            break;
        };
        indices.remove(index);
        if indices.len() < 3 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make::cuboid;

    fn opposite_edges_on_one_face(body: &Body) -> (FaceKey, [EdgeKey; 2]) {
        let face = body.face_keys().next().unwrap();
        let edges = body
            .face_coedges(face)
            .into_iter()
            .map(|coedge| body.coedges.get(coedge).unwrap().edge)
            .collect::<Vec<_>>();
        (face, [edges[0], edges[2]])
    }

    #[test]
    fn a_boss_rim_rounds_and_its_root_fills_by_their_rings_volumes() {
        // A cylinder standing on a plate: its top rim convex, its root
        // concave. Swept round the axis, each section takes or adds by
        // Pappus its area times the path of its centroid — the round's
        // (10 - 3π)/(12 - 3π) of a radius off the corner, a bevel's a third
        // of each setback: in from the top rim, out at the root.
        let plate = crate::make::cuboid([-5.0, -5.0, 0.0], [10.0, 10.0, 1.0]).unwrap();
        let boss = crate::make::cylinder([0.0, 0.0, 1.0], 2.0, 3.0).unwrap();
        let part = crate::combine(plate, boss, crate::Operation::Union, 1e-9).unwrap();
        let rim = |height: f64| {
            part.edges
                .iter()
                .find(|(_, edge)| {
                    matches!(part.curves.get(edge.curve), Some(Curve3::Circle(circle))
                        if (circle.plane.origin[2] - height).abs() < 1e-9)
                })
                .map(|(key, _)| key)
                .unwrap()
        };
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        let tolerance = operation_tolerance(&[&part]);
        let radius = 0.3;
        let round = (radius * radius * (1.0 - PI / 4.0), (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI));
        for (height, sign) in [(4.0, -1.0), (1.0, 1.0)] {
            let edge = rim(height);
            let plane = part
                .edges
                .get(edge)
                .unwrap()
                .coedges
                .iter()
                .map(|coedge| {
                    part.loops.get(part.coedges.get(*coedge).unwrap().owner).unwrap().owner
                })
                .find(|face| {
                    matches!(part.surfaces.get(part.faces.get(*face).unwrap().surface),
                        Some(Surface::Plane(_)))
                })
                .unwrap();
            let bevel = Section::Bevel { base_face: plane, base: radius, other: radius };
            for (section, (area, off)) in
                [(Section::Round(radius), round), (bevel, (radius * radius / 2.0, 1.0 / 3.0))]
            {
                let cut = turn_along_edge(&part, edge, section, tolerance).unwrap();
                let change = volume(&cut) - volume(&part);
                let expected = sign * area * TAU * (2.0 + sign * off * radius);
                assert!((change - expected).abs() < 1e-3 * expected.abs(), "{change} {expected}");
            }
        }
    }

    #[test]
    fn a_ridge_rounds_where_it_rises_from_its_plate() {
        // A plate with a round ridge along it, the ridge's circle meeting
        // the plate's top at forty-five degrees: two concave rulings. The
        // round fills the corner, the bevel too, a sliver under the ball's
        // square all along.
        use ocs_geom::geom2d::{Arc, Curve as Curve2, Line};
        let line = |start, end| Curve2::Line(Line { start, end });
        let root = std::f64::consts::SQRT_2;
        let profile = vec![
            line([0.0, 0.0], [6.0, 0.0]),
            line([6.0, 0.0], [6.0, 1.0]),
            line([6.0, 1.0], [4.0, 1.0]),
            Curve2::Arc(Arc {
                centre: [3.0, 0.0],
                radius: root,
                start_angle: PI / 4.0,
                end_angle: 3.0 * PI / 4.0,
            }),
            line([2.0, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ];
        let plane = Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        let part = crate::extrude(plane, &profile, [0.0, 0.0, 3.0]).unwrap();
        let ruling = part
            .edges
            .iter()
            .find(|(_, edge)| {
                let (from, to) = (
                    part.vertices.get(edge.start).unwrap().point,
                    part.vertices.get(edge.end).unwrap().point,
                );
                (from[0] - 4.0).abs() < 1e-9
                    && (from[1] - 1.0).abs() < 1e-9
                    && (to[0] - 4.0).abs() < 1e-9
                    && (to[1] - 1.0).abs() < 1e-9
            })
            .map(|(key, _)| key)
            .unwrap();
        let plate = part
            .edges
            .get(ruling)
            .unwrap()
            .coedges
            .iter()
            .map(|coedge| {
                part.loops.get(part.coedges.get(*coedge).unwrap().owner).unwrap().owner
            })
            .find(|face| {
                matches!(part.surfaces.get(part.faces.get(*face).unwrap().surface),
                    Some(Surface::Plane(_)))
            })
            .unwrap();
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        let tolerance = operation_tolerance(&[&part]);
        let bevel = Section::Bevel { base_face: plate, base: 0.2, other: 0.2 };
        for section in [Section::Round(0.2), bevel] {
            let blended = cut_along_ruling(&part, ruling, section, tolerance).unwrap();
            assert!(blended.validate().is_empty());
            let added = volume(&blended) - volume(&part);
            assert!(added > 0.0 && added < 0.2 * 0.2 * 3.0 * 2.0, "{added}");
        }
    }

    #[test]
    fn a_countersinks_rim_rounds_off() {
        // The countersink's cone opens into the plate's top: a convex rim
        // the round takes a ring off.
        let plate = crate::make::cuboid([-5.0, -5.0, 0.0], [10.0, 10.0, 2.0]).unwrap();
        let sink = crate::make::frustum([0.0, 0.0, 0.5], 1.0, 1.0, 2.5, 1.5).unwrap();
        let part =
            crate::combine(plate, sink, crate::Operation::Difference, 1e-9).unwrap();
        let rim = part
            .edges
            .iter()
            .find(|(_, edge)| {
                matches!(part.curves.get(edge.curve), Some(Curve3::Circle(circle))
                    if (circle.plane.origin[2] - 2.0).abs() < 1e-9)
            })
            .map(|(key, _)| key)
            .unwrap();
        let tolerance = operation_tolerance(&[&part]);
        let cut = turn_along_edge(&part, rim, Section::Round(0.2), tolerance).unwrap();
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        assert!(cut.validate().is_empty());
        let change = volume(&part) - volume(&cut);
        // Under the corner's square of the radius swept round the rim.
        assert!(change > 0.0 && change < 0.2 * 0.2 * TAU * 2.5, "{change}");
    }

    #[test]
    fn opposite_box_edges_are_filleted_atomically() {
        let body = cuboid([0.0; 3], [8.0, 6.0, 4.0]).unwrap();
        let (_, edges) = opposite_edges_on_one_face(&body);
        let result = fillet_edges(&body, &edges, 0.5).unwrap();

        assert!(result.validate().is_empty());
        assert!(result.faces.len() > body.faces.len());
    }

    /// Rounding one box edge and then an adjacent one ends with the same
    /// corner as rounding both at once.
    #[test]
    fn adjacent_edge_after_a_fillet_of_the_same_radius() {
        let body = cuboid([0.0; 3], [8.0, 6.0, 4.0]).unwrap();
        let face = body.face_keys().next().unwrap();
        let edges = body
            .face_coedges(face)
            .into_iter()
            .map(|coedge| body.coedges.get(coedge).unwrap().edge)
            .collect::<Vec<_>>();
        let together = fillet_edges(&body, &edges[0..2], 0.5).unwrap();

        let first = fillet_edges(&body, &edges[0..1], 0.5).unwrap();
        let first_frame = edge_frame(&body, edges[1]).unwrap();
        let next = first
            .edges
            .iter()
            .find_map(|(key, _)| {
                let frame = edge_frame(&first, key)?;
                let same = |a: Vec3, b: Vec3| a.dot(b) > 1.0 - 1e-8;
                let (a, b) = (frame.first_normal, frame.second_normal);
                let (c, d) = (first_frame.first_normal, first_frame.second_normal);
                ((same(a, c) && same(b, d)) || (same(a, d) && same(b, c))).then_some(key)
            })
            .unwrap();
        let second = fillet_edges(&first, &[next], 0.5).unwrap();

        assert!(second.validate().is_empty());
        assert_eq!(second.faces.len(), together.faces.len());
    }

    #[test]
    fn edges_of_an_extruded_presspull_are_rounded_and_chamfered() {
        let body = cuboid([0.0; 3], [10.0, 6.0, 4.0]).unwrap();
        let top = body
            .face_keys()
            .find(|face| {
                crate::planar_face_profile(&body, *face)
                    .is_some_and(|profile| profile.outward[2] > 0.5)
            })
            .unwrap();
        let pulled =
            crate::presspull_face(&body, top, 2.0, crate::PresspullMode::Extrude)
                .unwrap();
        // The pulled sides join the box's: still a box.
        assert_eq!(pulled.faces.len(), body.faces.len());
        let (edge, face) = pulled
            .face_keys()
            .find_map(|face| {
                let profile = crate::planar_face_profile(&pulled, face)?;
                (profile.outward[2] > 0.5).then(|| {
                    let coedge = pulled.face_coedges(face)[0];
                    (pulled.coedges.get(coedge).unwrap().edge, face)
                })
            })
            .unwrap();

        let rounded = fillet_edges(&pulled, &[edge], 0.5).unwrap();
        let chamfered = chamfer_edges(&pulled, &[edge], face, 0.5, 0.5).unwrap();

        assert!(rounded.validate().is_empty());
        assert!(chamfered.validate().is_empty());
        assert_eq!(rounded.faces.len(), body.faces.len() + 1);
        assert_eq!(chamfered.faces.len(), body.faces.len() + 1);
    }

    #[test]
    fn chamfers_and_fillets_meet_at_a_corner_in_either_order() {
        let body = cuboid([0.0; 3], [8.0, 6.0, 4.0]).unwrap();
        let face = body.face_keys().next().unwrap();
        let edges = body
            .face_coedges(face)
            .into_iter()
            .map(|coedge| body.coedges.get(coedge).unwrap().edge)
            .collect::<Vec<_>>();
        let later = |first: &Body| {
            let frame = edge_frame(&body, edges[1]).unwrap();
            first
                .edges
                .iter()
                .find_map(|(key, _)| {
                    let other = edge_frame(first, key)?;
                    let same = |a: Vec3, b: Vec3| a.dot(b) > 1.0 - 1e-8;
                    let (a, b) = (other.first_normal, other.second_normal);
                    let (c, d) = (frame.first_normal, frame.second_normal);
                    ((same(a, c) && same(b, d)) || (same(a, d) && same(b, c)))
                        .then_some((key, other.faces[0]))
                })
                .unwrap()
        };

        let rounded = fillet_edges(&body, &edges[0..1], 0.5).unwrap();
        let (edge, base) = later(&rounded);
        let then_chamfered = chamfer_edges(&rounded, &[edge], base, 0.5, 0.5).unwrap();
        let chamfered = chamfer_edges(&body, &edges[0..1], face, 0.5, 0.5).unwrap();
        let (edge, _) = later(&chamfered);
        let then_rounded = fillet_edges(&chamfered, &[edge], 0.5).unwrap();

        for result in [then_chamfered, then_rounded] {
            assert!(result.validate().is_empty());
            assert_eq!(result.faces.len(), body.faces.len() + 2);
            assert!(result.edges.iter().any(|(_, edge)| {
                matches!(result.curves.get(edge.curve), Some(Curve3::Ellipse(_)))
            }));
        }
    }

    #[test]
    fn opposite_box_edges_are_chamfered_atomically() {
        let body = cuboid([0.0; 3], [8.0, 6.0, 4.0]).unwrap();
        let (face, edges) = opposite_edges_on_one_face(&body);
        let result = chamfer_edges(&body, &edges, face, 0.5, 0.75).unwrap();

        assert!(result.validate().is_empty());
        assert!(result.faces.len() > body.faces.len());
    }

    /// An equilateral prism, side 4 and 3 high: its upright edges meet its
    /// ends square, but each other at sixty degrees.
    fn triangular_prism() -> Body {
        use ocs_geom::geom2d::{Curve as Curve2, Line};
        let line = |start, end| Curve2::Line(Line { start, end });
        let apex = [2.0, 2.0 * 3f64.sqrt()];
        let profile = vec![line([0.0, 0.0], [4.0, 0.0]), line([4.0, 0.0], apex), line(apex, [0.0, 0.0])];
        let plane = Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        crate::extrude(plane, &profile, [0.0, 0.0, 3.0]).unwrap()
    }

    fn volume(body: &Body) -> f64 {
        crate::mass_properties(body).unwrap().volume
    }

    #[test]
    fn every_edge_of_a_prism_rounds_with_ball_corners_at_oblique_corners() {
        let body = triangular_prism();
        let edges: Vec<EdgeKey> = body.edges.keys().collect();
        let radius = 0.3;
        let rounded = fillet_edges(&body, &edges, radius).unwrap();
        assert!(rounded.validate().is_empty());
        let corners = rounded
            .faces
            .iter()
            .filter(|(_, face)| matches!(rounded.surfaces.get(face.surface), Some(Surface::Sphere(_))))
            .count();
        assert_eq!(corners, 6);
        // A convex solid rounded all over is its core grown by the ball: the
        // core, its faces moved out by r, its edges' wedges of the cylinder
        // (the turn between their faces) and the whole ball at its corners.
        let inradius = 4.0 / (2.0 * 3f64.sqrt());
        let side = 4.0 * (inradius - radius) / inradius;
        let height = 3.0 - 2.0 * radius;
        let end = 3f64.sqrt() / 4.0 * side * side;
        let area = 2.0 * end + 3.0 * side * height;
        let wedges = 3.0 * height * (2.0 * PI / 3.0) + 6.0 * side * (PI / 2.0);
        let expected = end * height
            + area * radius
            + wedges * 0.5 * radius * radius
            + 4.0 / 3.0 * PI * radius.powi(3);
        let found = volume(&rounded);
        assert!((found - expected).abs() < 1e-4 * expected, "{found} {expected}");
    }

    #[test]
    fn two_edges_meeting_at_equal_angles_round_together_at_an_oblique_corner() {
        // Two bottom edges of the prism meet at sixty degrees, each square
        // to the bottom: the rounds reach the upright edge between their
        // other faces at one height and meet there along an ellipse.
        let body = triangular_prism();
        let bottom: Vec<EdgeKey> = body
            .edges
            .iter()
            .filter(|(_, edge)| {
                [edge.start, edge.end]
                    .iter()
                    .all(|vertex| body.vertices.get(*vertex).unwrap().point[2].abs() < 1e-9)
            })
            .map(|(key, _)| key)
            .collect();
        let radius = 0.3;
        let both = fillet_edges(&body, &bottom[..2], radius).unwrap();
        assert!(both.validate().is_empty());
        let whole = volume(&body);
        let first = whole - volume(&fillet_edges(&body, &bottom[..1], radius).unwrap());
        let second = whole - volume(&fillet_edges(&body, &bottom[1..2], radius).unwrap());
        // The two cuts overlap only by the corner, well inside a ball's cube.
        let removed = whole - volume(&both);
        assert!(removed < first + second && removed > first + second - radius.powi(3));
    }

    #[test]
    fn a_refused_size_names_the_largest_that_fits() {
        let body = cuboid([0.0; 3], [4.0, 4.0, 4.0]).unwrap();
        let (face, edges) = opposite_edges_on_one_face(&body);
        assert!(fillet_edges(&body, &edges[..1], 5.0).is_err());
        // A round on a square edge touches each face its radius from the
        // edge: the face, 4 across, takes radii below 4.
        let fit = fillet_fit(&body, &edges[..1], 5.0).unwrap();
        assert!(fit > 3.99 && fit < 4.0, "{fit}");
        assert!(fillet_edges(&body, &edges[..1], fit).is_ok());
        // Two rounds on opposite edges of one face share it.
        let fit = fillet_fit(&body, &edges, 5.0).unwrap();
        assert!(fit > 1.99 && fit < 2.0, "{fit}");
        // A bevel's distance is its setback, bounded by the room the same.
        assert!(chamfer_edges(&body, &edges[..1], face, 5.0, 5.0).is_err());
        let fit = chamfer_fit(&body, &edges[..1], face, 5.0, 5.0).unwrap();
        assert!(fit > 3.99 && fit < 4.0, "{fit}");
        // What fits is not cut down.
        assert_eq!(fillet_fit(&body, &edges[..1], 1.0), Some(1.0));
    }
}
