//! FLATTEN: project entities onto the XY plane (Z=0).
//!
//! Extracted verbatim from `app/commands/inquiry.rs` so the command arm and
//! the COMMANDS benchmark share one implementation.

use codec::{EntityType, Handle};

use crate::scene::Scene;
pub fn flatten_entity_z(entity: &codec::EntityType) -> Option<codec::EntityType> {
    use codec::types::Vector3;

    let flattened = match entity {
        codec::EntityType::Circle(_)
        | codec::EntityType::Arc(_)
        | codec::EntityType::Ellipse(_)
        | codec::EntityType::LwPolyline(_)
        | codec::EntityType::Polyline2D(_) => flatten_curve_entity(entity)?,
        codec::EntityType::Line(source) => {
            let mut line = source.clone();
            line.start = flatten_point(line.start)?;
            line.end = flatten_point(line.end)?;
            line.thickness = 0.0;
            line.normal = Vector3::UNIT_Z;
            codec::EntityType::Line(line)
        }
        codec::EntityType::Polyline(source) => {
            let mut polyline = source.clone();
            for vertex in &mut polyline.vertices {
                vertex.location = flatten_point(vertex.location)?;
            }
            codec::EntityType::Polyline(polyline)
        }
        codec::EntityType::Polyline3D(source) => {
            let mut polyline = source.clone();
            polyline.elevation = 0.0;
            polyline.normal = Vector3::UNIT_Z;
            for vertex in &mut polyline.vertices {
                vertex.position = flatten_point(vertex.position)?;
            }
            codec::EntityType::Polyline3D(polyline)
        }
        codec::EntityType::Text(source) if positive_z_normal(source.normal) => {
            let mut text = source.clone();
            text.insertion_point = flatten_point(text.insertion_point)?;
            if let Some(alignment) = text.alignment_point {
                text.alignment_point = Some(flatten_point(alignment)?);
            }
            text.thickness = 0.0;
            text.normal = Vector3::UNIT_Z;
            codec::EntityType::Text(text)
        }
        codec::EntityType::MText(source) if positive_z_normal(source.normal) => {
            let mut text = source.clone();
            text.insertion_point = flatten_point(text.insertion_point)?;
            text.normal = Vector3::UNIT_Z;
            codec::EntityType::MText(text)
        }
        codec::EntityType::Point(source) => {
            let mut point = source.clone();
            point.location = flatten_point(point.location)?;
            point.thickness = 0.0;
            point.normal = Vector3::UNIT_Z;
            codec::EntityType::Point(point)
        }
        codec::EntityType::Spline(source) => {
            let mut spline = source.clone();
            for point in &mut spline.control_points {
                *point = flatten_point(*point)?;
            }
            for point in &mut spline.fit_points {
                *point = flatten_point(*point)?;
            }
            spline.begin_tangent = flatten_vector(spline.begin_tangent)?;
            spline.end_tangent = flatten_vector(spline.end_tangent)?;
            spline.normal = Vector3::UNIT_Z;
            spline.flags.planar = true;
            codec::EntityType::Spline(spline)
        }
        codec::EntityType::Solid(source) => {
            let corners = crate::entities::solid::wcs_corners(source);
            let mut solid = source.clone();
            solid.first_corner = flatten_array(corners[0])?;
            solid.second_corner = flatten_array(corners[1])?;
            solid.third_corner = flatten_array(corners[2])?;
            solid.fourth_corner = flatten_array(corners[3])?;
            solid.normal = Vector3::UNIT_Z;
            solid.thickness = 0.0;
            codec::EntityType::Solid(solid)
        }
        codec::EntityType::Face3D(source) => {
            let mut face = source.clone();
            face.first_corner = flatten_point(face.first_corner)?;
            face.second_corner = flatten_point(face.second_corner)?;
            face.third_corner = flatten_point(face.third_corner)?;
            face.fourth_corner = flatten_point(face.fourth_corner)?;
            codec::EntityType::Face3D(face)
        }
        _ => return None,
    };

    (flattened != *entity).then_some(flattened)
}

fn flatten_curve_entity(entity: &codec::EntityType) -> Option<codec::EntityType> {
    use codec::types::{Vector2, Vector3};
    use kernel::geom2d::Curve;

    match entity {
        codec::EntityType::Ellipse(source)
            if source.center.z == 0.0
                && source.major_axis.z == 0.0
                && source.normal == Vector3::UNIT_Z =>
        {
            return None;
        }
        codec::EntityType::LwPolyline(source)
            if !z_axis_normal(source.normal)
                && (source.constant_width != 0.0
                    || source
                        .vertices
                        .iter()
                        .any(|vertex| vertex.start_width != 0.0 || vertex.end_width != 0.0)) =>
        {
            return None;
        }
        codec::EntityType::Polyline2D(source)
            if !z_axis_normal(source.normal)
                && (source.start_width != 0.0
                    || source.end_width != 0.0
                    || source
                        .vertices
                        .iter()
                        .any(|vertex| vertex.start_width != 0.0 || vertex.end_width != 0.0)) =>
        {
            return None;
        }
        codec::EntityType::LwPolyline(source) if source.vertices.len() < 2 => {
            let plane = crate::entities::curve::ocs_plane(source.normal, source.elevation);
            let mut polyline = source.clone();
            for vertex in &mut polyline.vertices {
                let point = flatten_array(plane.point_at([
                    vertex.location.x,
                    vertex.location.y,
                ]))?;
                vertex.location = Vector2::new(point.x, point.y);
            }
            polyline.elevation = 0.0;
            polyline.thickness = 0.0;
            polyline.normal = Vector3::UNIT_Z;
            return Some(codec::EntityType::LwPolyline(polyline));
        }
        codec::EntityType::Polyline2D(source) if source.vertices.len() < 2 => {
            let plane = crate::entities::curve::ocs_plane(source.normal, source.elevation);
            let mut polyline = source.clone();
            for vertex in &mut polyline.vertices {
                let point = flatten_array(plane.point_at([
                    vertex.location.x,
                    vertex.location.y,
                ]))?;
                vertex.location = point;
            }
            polyline.elevation = 0.0;
            polyline.thickness = 0.0;
            polyline.normal = Vector3::UNIT_Z;
            return Some(codec::EntityType::Polyline2D(polyline));
        }
        _ => {}
    }

    let curve = crate::entities::curve::entity_curve_xy(entity)?;
    match (entity, curve) {
        (codec::EntityType::Circle(source), Curve::Circle(curve)) => {
            let mut circle = source.clone();
            circle.center = Vector3::new(curve.centre[0], curve.centre[1], 0.0);
            circle.radius = curve.radius;
            circle.thickness = 0.0;
            circle.normal = Vector3::UNIT_Z;
            Some(codec::EntityType::Circle(circle))
        }
        (codec::EntityType::Arc(source), Curve::Arc(curve)) => {
            let mut arc = source.clone();
            arc.center = Vector3::new(curve.centre[0], curve.centre[1], 0.0);
            arc.radius = curve.radius;
            arc.start_angle = curve.start_angle;
            arc.end_angle = curve.end_angle;
            arc.thickness = 0.0;
            arc.normal = Vector3::UNIT_Z;
            Some(codec::EntityType::Arc(arc))
        }
        (codec::EntityType::Circle(source), Curve::Ellipse(curve)) => {
            flattened_ellipse(source.common.clone(), curve)
        }
        (codec::EntityType::Arc(source), Curve::Ellipse(curve)) => {
            flattened_ellipse(source.common.clone(), curve)
        }
        (codec::EntityType::Ellipse(source), Curve::Ellipse(curve)) => {
            flattened_ellipse(source.common.clone(), curve)
        }
        (codec::EntityType::LwPolyline(source), Curve::Polyline(curve)) => {
            if curve.vertices.len() != source.vertices.len() {
                return None;
            }
            let mut polyline = source.clone();
            for (target, projected) in polyline.vertices.iter_mut().zip(curve.vertices) {
                target.location = Vector2::new(projected.position[0], projected.position[1]);
                target.bulge = projected.bulge;
            }
            polyline.elevation = 0.0;
            polyline.thickness = 0.0;
            polyline.normal = Vector3::UNIT_Z;
            Some(codec::EntityType::LwPolyline(polyline))
        }
        (codec::EntityType::Polyline2D(source), Curve::Polyline(curve)) => {
            if curve.vertices.len() != source.vertices.len() {
                return None;
            }
            let mut polyline = source.clone();
            for (target, projected) in polyline.vertices.iter_mut().zip(curve.vertices) {
                target.location =
                    Vector3::new(projected.position[0], projected.position[1], 0.0);
                target.bulge = projected.bulge;
            }
            polyline.elevation = 0.0;
            polyline.thickness = 0.0;
            polyline.normal = Vector3::UNIT_Z;
            Some(codec::EntityType::Polyline2D(polyline))
        }
        _ => None,
    }
}

fn flattened_ellipse(
    common: codec::entities::EntityCommon,
    curve: kernel::geom2d::EllipseArc,
) -> Option<codec::EntityType> {
    use codec::types::Vector3;

    let geometry = curve.ellipse;
    if !geometry.major_radius.is_finite() || geometry.major_radius <= 0.0 {
        return None;
    }
    let mut ellipse = codec::entities::Ellipse::new();
    ellipse.common = common;
    ellipse.center = Vector3::new(geometry.centre[0], geometry.centre[1], 0.0);
    ellipse.major_axis = Vector3::new(
        geometry.major_axis[0] * geometry.major_radius,
        geometry.major_axis[1] * geometry.major_radius,
        0.0,
    );
    ellipse.minor_axis_ratio = geometry.minor_radius / geometry.major_radius;
    ellipse.start_parameter = curve.start_parameter;
    ellipse.end_parameter = curve.end_parameter;
    ellipse.normal = Vector3::UNIT_Z;
    Some(codec::EntityType::Ellipse(ellipse))
}

fn flatten_point(point: codec::types::Vector3) -> Option<codec::types::Vector3> {
    flatten_array([point.x, point.y, point.z])
}

fn flatten_array(point: [f64; 3]) -> Option<codec::types::Vector3> {
    use kernel::space::Plane;

    let point = Plane::XY.point_at(Plane::XY.project(point)?);
    Some(codec::types::Vector3::new(point[0], point[1], point[2]))
}

fn flatten_vector(vector: codec::types::Vector3) -> Option<codec::types::Vector3> {
    use kernel::space::Plane;

    let vector = Plane::XY.vector_at(Plane::XY.project_vector([
        vector.x, vector.y, vector.z,
    ])?);
    Some(codec::types::Vector3::new(
        vector[0], vector[1], vector[2],
    ))
}

fn positive_z_normal(normal: codec::types::Vector3) -> bool {
    normal.x == 0.0 && normal.y == 0.0 && normal.z > 0.0
}

fn z_axis_normal(normal: codec::types::Vector3) -> bool {
    normal.x == 0.0 && normal.y == 0.0 && normal.z != 0.0
}

/// Selection-or-all candidate handles: every entity in the active space that
/// sits on an unlocked layer (verbatim from the FLATTEN command arm).
pub fn collect_flatten_handles(scene: &Scene) -> Vec<Handle> {
    let sel = scene.selected_entities();
    if sel.is_empty() {
        let mut out = Vec::with_capacity(scene.document.entities().size_hint().0);
        for handle in scene.document.entities().map(|e| e.common().handle) {
            if scene.entity_belongs_to_active_space(handle) && !scene.is_layer_locked(handle) {
                out.push(handle);
            }
        }
        out
    } else {
        let mut out = Vec::with_capacity(sel.len());
        for (handle, _) in sel {
            if scene.entity_belongs_to_active_space(handle) && !scene.is_layer_locked(handle) {
                out.push(handle);
            }
        }
        out
    }
}

/// The replacement entities for candidates that actually move (None for
/// unsupported / already-flat entities, exactly as the arm filtered them).
pub fn plan_flatten(scene: &Scene, handles: &[Handle]) -> Vec<EntityType> {
    let mut out = Vec::with_capacity(handles.len());
    for &handle in handles {
        if let Some(entity) = scene.document.get_entity(handle) {
            if let Some(flattened) = flatten_entity_z(entity) {
                out.push(flattened);
            }
        }
    }
    out
}

/// Publish flattened replacements; returns how many entities moved.
pub fn apply_flatten_updates(scene: &mut Scene, updates: Vec<EntityType>) -> usize {
    scene.update_entities(updates)
}

#[cfg(test)]
mod flatten_tests {
    use super::flatten_entity_z;
    use codec::types::Vector3;
    use codec::EntityType;

    #[test]
    fn flattens_3d_polyline_vertices() {
        let mut pl = codec::entities::Polyline3D::new();
        for z in [5.0, -2.0, 7.5] {
            pl.vertices
                .push(codec::entities::Vertex3DPolyline::new(Vector3::new(1.0, 2.0, z)));
        }
        pl.elevation = 5.0;

        let entity = EntityType::Polyline3D(pl);
        let entity = flatten_entity_z(&entity).expect("projection");

        let EntityType::Polyline3D(pl) = entity else {
            panic!("wrong variant")
        };
        assert_eq!(pl.elevation, 0.0);
        assert!(pl.vertices.iter().all(|v| v.position.z == 0.0));
        // X/Y must survive the projection.
        assert!(pl.vertices.iter().all(|v| v.position.x == 1.0 && v.position.y == 2.0));
    }

    #[test]
    fn flattens_2d_polyline_vertices_and_elevation() {
        let mut pl = codec::entities::Polyline2D::new();
        pl.vertices
            .push(codec::entities::Vertex2D::new(Vector3::new(3.0, 4.0, 9.0)));
        pl.elevation = 9.0;

        let entity = EntityType::Polyline2D(pl);
        let entity = flatten_entity_z(&entity).expect("projection");

        let EntityType::Polyline2D(pl) = entity else {
            panic!("wrong variant")
        };
        assert_eq!(pl.elevation, 0.0);
        assert_eq!(pl.vertices[0].location.z, 0.0);
        assert_eq!(pl.vertices[0].location.x, 3.0);
    }

    #[test]
    fn flattens_solid_corners() {
        let solid = codec::entities::Solid::new(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 2.0),
            Vector3::new(1.0, 1.0, 3.0),
            Vector3::new(0.0, 1.0, 4.0),
        );

        let entity = EntityType::Solid(solid);
        let entity = flatten_entity_z(&entity).expect("projection");

        let EntityType::Solid(s) = entity else {
            panic!("wrong variant")
        };
        assert_eq!(
            [s.first_corner.z, s.second_corner.z, s.third_corner.z, s.fourth_corner.z],
            [0.0; 4]
        );
    }

    #[test]
    fn reports_unsupported_entities_as_not_moved() {
        let entity = EntityType::Ray(codec::entities::Ray::new(
            Vector3::new(0.0, 0.0, 5.0),
            Vector3::new(1.0, 0.0, 0.0),
        ));
        assert!(flatten_entity_z(&entity).is_none());
    }

    #[test]
    fn still_flattens_a_line() {
        let mut line = codec::entities::Line::new();
        line.start = Vector3::new(0.0, 0.0, 3.0);
        line.end = Vector3::new(1.0, 1.0, 4.0);
        let entity = EntityType::Line(line);
        let entity = flatten_entity_z(&entity).expect("projection");
        let EntityType::Line(l) = entity else { panic!("wrong variant") };
        assert_eq!((l.start.z, l.end.z), (0.0, 0.0));
    }

    #[test]
    fn already_flat_line_is_not_moved() {
        let mut line = codec::entities::Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(1.0, 1.0, 0.0);
        let entity = EntityType::Line(line);
        assert!(flatten_entity_z(&entity).is_none());
    }

    #[test]
    fn tilted_circle_projects_to_ellipse() {
        let mut circle = codec::entities::Circle::from_center_radius(
            Vector3::new(2.0, 3.0, 4.0),
            5.0,
        );
        circle.normal = Vector3::new(0.0, 0.6, 0.8);
        circle.thickness = 2.0;
        let entity = EntityType::Circle(circle);

        let projected = flatten_entity_z(&entity).expect("projection");
        let EntityType::Ellipse(ellipse) = projected else {
            panic!("wrong variant")
        };
        assert_eq!(ellipse.center.z, 0.0);
        assert_eq!(ellipse.major_axis.z, 0.0);
        assert_eq!(ellipse.normal, Vector3::UNIT_Z);
        assert!(ellipse.minor_axis_ratio < 1.0);
    }

    #[test]
    fn tilted_solid_uses_world_projection() {
        let mut solid = codec::entities::Solid::new(
            Vector3::new(0.0, 0.0, 2.0),
            Vector3::new(1.0, 0.0, 2.0),
            Vector3::new(1.0, 1.0, 2.0),
            Vector3::new(0.0, 1.0, 2.0),
        );
        solid.normal = Vector3::new(0.0, 0.6, 0.8);
        solid.thickness = 3.0;
        let entity = EntityType::Solid(solid);

        let projected = flatten_entity_z(&entity).expect("projection");
        let EntityType::Solid(solid) = projected else {
            panic!("wrong variant")
        };
        assert_eq!(solid.normal, Vector3::UNIT_Z);
        assert_eq!(solid.thickness, 0.0);
        assert_eq!(
            [
                solid.first_corner.z,
                solid.second_corner.z,
                solid.third_corner.z,
                solid.fourth_corner.z,
            ],
            [0.0; 4]
        );
    }
}
