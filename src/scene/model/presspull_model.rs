//! PRESSPULL selection and curve-only previews. Geometry operations stay in the kernel.

use acadrust::entities::{AcisData, EmbeddedEntity, Region, Wire};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use cadkernel::brep::{self, Body, FaceKey};
use cadkernel::geom2d::{contains, ring_nesting_depths, signed_area, Curve, Line, Tolerance};
use cadkernel::space::Plane;
use glam::DVec3;
use rustc_hash::FxHashMap;

use crate::command::WorkingPlane;
use crate::scene::{boundary_faces, exact_hatch_paths, ring_source_handles, BoundarySource, Scene};
use super::{sweep_model, wire_model::WireModel};

const BOUNDARY_TOLERANCE: f64 = 1e-6;

#[derive(Clone)]
pub enum PresspullTargetKind {
    Profile { entity: EntityType, source: Option<Handle>, owner: Option<Handle> },
    Face { handle: Handle, face: FaceKey, body: Body, offset: bool },
}

#[derive(Clone)]
pub struct PresspullTarget {
    pub kind: PresspullTargetKind,
    pub anchor: DVec3,
    pub direction: DVec3,
}

/// Extract exact embedded REGION loops before falling back to polygonal entities.
/// In particular, preview and final extrusion must not approximate REGION arcs
/// from their display wires or discard holes.
pub fn profile_geometry(entity: &EntityType) -> Option<(Plane, Vec<Vec<Curve>>, bool)> {
    if matches!(entity, EntityType::Ray(_) | EntityType::XLine(_)) {
        return None;
    }
    let embedded = match entity {
        EntityType::Region(region) => Some((
            EmbeddedEntity::Region(region.clone()), glam::DMat4::IDENTITY.to_cols_array(),
        )),
        _ => sweep_model::embedded_revolve_profile(entity),
    };
    if let Some((entity, transform)) = embedded {
        if let Ok(geometry) = cadkernel::acis::sweep_profile_geometry(&entity, transform) {
            return Some(geometry);
        }
        // Exact modeler data is authoritative, not a hint to replace by chords.
        if matches!(entity, EmbeddedEntity::Region(ref region) if region.acis_data.has_data()) {
            return None;
        }
    }
    let (profile, closed) = sweep_model::extrusion_profile_of(entity)?;
    Some((profile.plane, vec![profile.pieces], closed))
}

/// Builds the final solid or surface without discarding exact REGION loops.
pub fn extrusion_body(entity: &EntityType, direction: [f64; 3]) -> Option<Body> {
    let (plane, loops, closed) = profile_geometry(entity)?;
    if closed {
        brep::extrude_region(plane, &loops, direction)
    } else if loops.len() == 1 {
        brep::extrude_surface(plane, &loops[0], direction)
    } else {
        None
    }
}

fn eligible(scene: &Scene, entity: &EntityType) -> bool {
    let common = entity.common();
    !common.invisible
        && !scene.entity_temporarily_hidden(common.handle)
        && !scene.layer_hidden(&common.layer)
        && !scene.interaction_layer_frozen(&common.layer)
        && !scene.is_layer_locked(common.handle)
        && scene.belongs_to_visible_block(
            common.handle, common.owner_handle, scene.interaction_block_handle(),
        )
        && !crate::scene::annotative::annotative_offscale_for(
            &scene.document, common, scene.displayed_annotation_scale_handle(),
            scene.annotation_all_visible(),
        )
}

fn working_plane(plane: Plane) -> WorkingPlane {
    WorkingPlane::new(
        DVec3::from_array(plane.origin), DVec3::from_array(plane.x_axis),
        DVec3::from_array(plane.y_axis),
    )
}

fn curves_on_plane(source: Plane, curves: &[Curve], plane: WorkingPlane) -> Vec<Curve> {
    let origin = plane.to_local(DVec3::from_array(source.origin));
    let x_axis = plane.vector_to_local(DVec3::from_array(source.x_axis));
    let y_axis = plane.vector_to_local(DVec3::from_array(source.y_axis));
    let transform = cadkernel::geom2d::Transform {
        origin: [origin.x, origin.y].into(),
        x_axis: [x_axis.x, x_axis.y].into(),
        y_axis: [y_axis.x, y_axis.y].into(),
    };
    curves.iter().filter_map(|curve| {
        // A line belongs to more than one plane. Its arbitrary source axes
        // must not disqualify an edge which lies in the requested work plane.
        if let Curve::Line(line) = curve {
            let start = plane.to_local(DVec3::from_array(source.point_at(line.start)));
            let end = plane.to_local(DVec3::from_array(source.point_at(line.end)));
            return (start.z.abs() <= BOUNDARY_TOLERANCE && end.z.abs() <= BOUNDARY_TOLERANCE)
                .then_some(Curve::Line(Line { start: [start.x, start.y], end: [end.x, end.y] }));
        }
        if origin.z.abs() > BOUNDARY_TOLERANCE || x_axis.z.abs() > 1e-9 || y_axis.z.abs() > 1e-9 {
            return None;
        }
        curve.transformed(&transform)
    }).collect()
}

fn boundary_source(curves: Vec<Curve>) -> Option<BoundarySource> {
    let curves = curves.into_iter().filter(|curve| {
        let length = curve.length();
        length.is_finite() && length > BOUNDARY_TOLERANCE
    }).collect::<Vec<_>>();
    let segments = curves.iter().flat_map(|curve| {
        curve.tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE)
            .windows(2).filter_map(|pair| {
                let start = pair[0];
                let end = pair[1];
                (start.iter().chain(&end).all(|value| value.is_finite()) && start != end)
                    .then_some(Line { start, end })
            }).collect::<Vec<_>>()
    }).collect::<Vec<_>>();
    (!segments.is_empty()).then_some(BoundarySource { segments, curves })
}

fn boundary_sources(scene: &Scene, plane: WorkingPlane) -> FxHashMap<Handle, BoundarySource> {
    // Read native curve geometry, not shaded meshes, face isolines, temporary
    // construction-history overlays or camera-clipped display segments.
    scene.document.entities().filter(|entity| eligible(scene, entity)).filter_map(|entity| {
        let (source, loops, _) = profile_geometry(entity)?;
        let curves = loops.iter().flat_map(|ring| curves_on_plane(source, ring, plane)).collect();
        boundary_source(curves).map(|source| (entity.common().handle, source))
    }).collect()
}

fn ring_curves(ring: &[[f64; 2]]) -> Vec<Curve> {
    ring.iter().copied().zip(ring.iter().copied().cycle().skip(1))
        .take(ring.len()).map(|(start, end)| Curve::Line(Line { start, end })).collect()
}

fn selected_rings(sources: &FxHashMap<Handle, BoundarySource>, point: [f64; 2]) -> Option<Vec<Vec<[f64; 2]>>> {
    let rings = boundary_faces(sources, BOUNDARY_TOLERANCE);
    let tolerance = Tolerance::new(BOUNDARY_TOLERANCE);
    let outer = rings.iter().enumerate()
        .filter(|(_, ring)| contains(&ring_curves(ring), point, tolerance))
        .min_by(|(_, left), (_, right)| signed_area(left).abs().total_cmp(&signed_area(right).abs()))?
        .0;
    let depths = ring_nesting_depths(&rings);
    let boundary = ring_curves(&rings[outer]);
    let mut result = vec![rings[outer].clone()];
    for (index, ring) in rings.iter().enumerate() {
        if index != outer && depths.get(index) == depths.get(outer).map(|depth| depth + 1).as_ref()
            && ring.first().is_some_and(|point| contains(&boundary, *point, tolerance))
        {
            result.push(ring.clone());
        }
    }
    Some(result)
}

fn boundary_region(
    sources: &FxHashMap<Handle, BoundarySource>, rings: &[Vec<[f64; 2]>], plane: WorkingPlane,
) -> Option<EntityType> {
    let exterior = (0..rings.len()).map(|index| index == 0).collect::<Vec<_>>();
    let paths = exact_hatch_paths(rings, &exterior, sources, BOUNDARY_TOLERANCE);
    if paths.len() != rings.len() {
        return None;
    }
    let loops = paths.iter().map(|path| {
        path.edges.iter().map(crate::entities::hatch::edge_curve).collect::<Option<Vec<_>>>()
    }).collect::<Option<Vec<_>>>()?;
    let kernel_plane = Plane::from_axes(plane.origin.to_array(), plane.x.to_array(), plane.y.to_array());
    let sheet = brep::planar_region(kernel_plane, &loops)?;
    let document = crate::scene::convert::acis_export::solid_to_sat(&sheet)?;
    let mut region = Region::new();
    region.acis_data = AcisData::from_sat(&document.to_sat_string());
    region.point_of_reference = Vector3::new(plane.origin.x, plane.origin.y, plane.origin.z);
    region.wires = loops.iter().map(|ring| {
        let mut points = Vec::new();
        for curve in ring {
            let skip = usize::from(!points.is_empty());
            points.extend(curve.tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE)
                .into_iter().skip(skip).map(|point| {
                    let world = kernel_plane.point_at(point);
                    Vector3::new(world[0], world[1], world[2])
                }));
        }
        Wire::from_points(points)
    }).collect();
    Some(EntityType::Region(region))
}

fn face_at_point(scene: &Scene, point: DVec3, preferred: Option<Handle>) -> Option<(Handle, FaceKey, &Body)> {
    let mut handles = scene.solid_models.keys().copied().collect::<Vec<_>>();
    handles.sort_by_key(|handle| handle.value());
    for handle in handles {
        if preferred.is_some_and(|preferred| preferred != handle) {
            continue;
        }
        let Some(entity) = scene.document.get_entity(handle) else { continue; };
        if !matches!(entity, EntityType::Solid3D(_)) || !eligible(scene, entity) {
            continue;
        }
        let body = &scene.solid_models[&handle];
        if let Some(face) = brep::planar_face_at_point(body, point.to_array(), BOUNDARY_TOLERANCE) {
            return Some((handle, face, body));
        }
    }
    None
}

fn profile_owner(scene: &Scene, plane: WorkingPlane, anchor: DVec3) -> Option<(Handle, DVec3)> {
    let mut handles = scene.solid_models.keys().copied().collect::<Vec<_>>();
    handles.sort_by_key(|handle| handle.value());
    for handle in handles {
        let Some(entity) = scene.document.get_entity(handle) else { continue; };
        if !matches!(entity, EntityType::Solid3D(_)) || !eligible(scene, entity) {
            continue;
        }
        let body = &scene.solid_models[&handle];
        for face in body.face_keys() {
            let Some(profile) = brep::planar_face_profile(body, face) else { continue; };
            let outward = DVec3::from_array(profile.outward);
            if outward.dot(plane.z).abs() < 1.0 - 1e-9
                || profile.plane.distance_to(anchor.to_array())?.abs() > BOUNDARY_TOLERANCE
            {
                continue;
            }
            let local = profile.plane.project(anchor.to_array())?;
            let boundary = profile.loops.into_iter().flatten().collect::<Vec<_>>();
            if contains(&boundary, local, Tolerance::new(BOUNDARY_TOLERANCE)) {
                return Some((handle, outward));
            }
        }
    }
    None
}

/// A supplied curve handle means explicit edge/preselection. An absent handle
/// means a bounded-area pick; closed profiles therefore retain their distinct
/// edge and interior behaviors. A solid handle prefers its exact trimmed face.
pub fn resolve_target(
    scene: &Scene, handle: Option<Handle>, point: DVec3, plane: WorkingPlane, offset: bool,
) -> Result<PresspullTarget, String> {
    if !point.is_finite() {
        return Err("PRESSPULL: the selected point is not finite.".to_owned());
    }
    if let Some(handle) = handle {
        let entity = scene.document.get_entity(handle)
            .ok_or("PRESSPULL: the selected object no longer exists.")?;
        if !eligible(scene, entity) {
            return Err("PRESSPULL: select a visible object on an unlocked layer.".to_owned());
        }
        if let Some((source_plane, _, closed)) = profile_geometry(entity) {
            let profile_plane = working_plane(source_plane);
            let local = profile_plane.to_local(point);
            let anchor = profile_plane.to_world(DVec3::new(local.x, local.y, 0.0));
            let attached = closed.then(|| profile_owner(scene, profile_plane, anchor)).flatten();
            let owner = attached.map(|(owner, _)| owner);
            let direction = attached.map(|(_, outward)| outward).unwrap_or(profile_plane.z);
            return Ok(PresspullTarget {
                kind: PresspullTargetKind::Profile { entity: entity.clone(), source: Some(handle), owner },
                anchor, direction,
            });
        }
        if !matches!(entity, EntityType::Solid3D(_)) {
            return Err("PRESSPULL: select a planar curve, bounded area or planar solid face.".to_owned());
        }
    }

    if let Some((owner, face, body)) = face_at_point(scene, point, handle) {
        let profile = brep::planar_face_profile(body, face)
            .ok_or("PRESSPULL: the selected solid face has no valid planar boundary.")?;
        let face_plane = working_plane(profile.plane);
        let local = face_plane.to_local(point);
        let anchor = face_plane.to_world(DVec3::new(local.x, local.y, 0.0));
        let direction = DVec3::from_array(profile.outward);
        if !offset {
            let mut sources = boundary_sources(scene, face_plane);
            if let Some(source) = boundary_source(profile.loops.into_iter().flatten().collect()) {
                sources.insert(owner, source);
            }
            if let Some(rings) = selected_rings(&sources, [local.x, local.y]) {
                let bounded_by_drawing = rings.iter().any(|ring| {
                    ring_source_handles(ring, &sources).into_iter().any(|source| source != owner)
                });
                if bounded_by_drawing {
                    let entity = boundary_region(&sources, &rings, face_plane)
                        .ok_or("PRESSPULL: the selected boundary could not be reconstructed exactly.")?;
                    return Ok(PresspullTarget {
                        kind: PresspullTargetKind::Profile { entity, source: None, owner: Some(owner) },
                        anchor, direction,
                    });
                }
            }
        }
        return Ok(PresspullTarget {
            kind: PresspullTargetKind::Face { handle: owner, face, body: body.clone(), offset },
            anchor, direction,
        });
    }
    if handle.is_some() {
        return Err("PRESSPULL: select inside a trimmed planar face, outside its holes.".to_owned());
    }
    let local = plane.to_local(point);
    if local.z.abs() > BOUNDARY_TOLERANCE {
        return Err("PRESSPULL: the bounded area is not on the current working plane.".to_owned());
    }
    let sources = boundary_sources(scene, plane);
    let rings = selected_rings(&sources, [local.x, local.y])
        .ok_or("PRESSPULL: no closed boundary contains the selected point.")?;
    let entity = boundary_region(&sources, &rings, plane)
        .ok_or("PRESSPULL: the selected boundary could not be reconstructed exactly.")?;
    Ok(PresspullTarget {
        kind: PresspullTargetKind::Profile { entity, source: None, owner: None },
        anchor: point, direction: plane.z,
    })
}

/// Curves only: mouse movement never triangulates a preview body.
pub fn preview_wires(target: &PresspullTarget, distance: f64, color: [f32; 4], isolines: usize) -> Vec<WireModel> {
    if !distance.is_finite() || distance.abs() <= 1e-9 {
        return Vec::new();
    }
    if let PresspullTargetKind::Face { body, face, offset: true, .. } = &target.kind {
        // Offset changes the intersections with neighbouring surfaces, so a
        // constant-section prism would misrepresent sloped walls and caps.
        // The kernel's analytic offset performs no Boolean or triangulation;
        // the command memoizes this candidate by selected targets and distance.
        // Only return an overlay: failure never hides or edits the resident body.
        return brep::presspull_face(body, *face, distance, brep::PresspullMode::Offset)
            .map(|candidate| preview_body_wires(&candidate, color, isolines))
            .unwrap_or_default();
    }
    let geometry = match &target.kind {
        PresspullTargetKind::Profile { entity, .. } => profile_geometry(entity)
            .map(|(plane, loops, _)| (plane, loops, (target.direction * distance).to_array())),
        PresspullTargetKind::Face { body, face, .. } => {
            // The resident source stays visible. Show only the moving face's
            // exact extrusion cage; its Boolean merge happens only on commit,
            // never inside pointer movement. Unrelated faces and lumps need
            // no rebuilding for a normal face-extrusion preview.
            brep::planar_face_profile(body, *face).map(|profile| (profile.plane, profile.loops,
                (DVec3::from_array(profile.outward) * distance).to_array()))
        }
    };
    let Some((plane, loops, direction)) = geometry else { return Vec::new(); };
    let mut wires = Vec::new();
    // Each loop is a lateral sheet. This wire-only overlay does not need the
    // planar caps or complete solid topology of extrude_region.
    for ring in loops {
        let ring = brep::extrusion_profile_pieces(&ring);
        let Some(body) = brep::extrude_surface(plane, &ring, direction) else { continue; };
        wires.extend(preview_body_wires(&body, color, isolines));
    }
    wires
}

fn preview_body_wires(body: &Body, color: [f32; 4], isolines: usize) -> Vec<WireModel> {
    let wireframe = brep::mesh::tessellate_wireframe(body,
        brep::mesh::TessellationTolerance::new(cadkernel::tessellation::DEFAULT_ANGLE, 1e-9)
            .with_isolines(isolines));
    wireframe.edges.into_iter().map(|edge| edge.positions)
        .chain(wireframe.isolines.into_iter().map(|line| line.positions))
        .filter(|points| points.len() >= 2)
        .map(|points| WireModel::solid_f64("PRESSPULL-PREVIEW".to_owned(), points, color, false))
        .collect()
}
