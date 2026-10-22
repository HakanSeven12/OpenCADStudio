use super::*;
use codec::entities::{
    Arc, AttributeEntity, Circle, Ellipse, Line as NativeLine, LwPolyline, LwVertex, Ray, Spline,
    XLine,
};
use codec::types::{Vector2, Vector3};
use std::f64::consts::{FRAC_PI_2, PI};

fn fixture() -> (Scene, Handle, Handle) {
    let mut scene = Scene::new();
    let base = Vector3::new(3., 4., 0.);
    scene.define_block_raw(
        "ALIGN",
        base,
        vec![EntityType::Line(NativeLine::from_points(
            base,
            base + Vector3::UNIT_X,
        ))],
    );
    let definition = scene.document.block_records.get("ALIGN").unwrap().handle;
    let parameter = orientation::create(
        &mut scene.document,
        definition,
        orientation::Kind::Alignment,
        base,
        base + Vector3::UNIT_X,
        base,
    )
    .unwrap();
    block_grips::config::set_location(
        &mut scene.document,
        definition,
        parameter,
        0,
        base + Vector3::new(2., 3., 0.),
    )
    .unwrap();
    let insert = scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "ALIGN",
            Vector3::new(100., 200., 30.),
        )))
        .unwrap();
    (scene, definition, insert)
}

#[test]
fn block_alignment_reference_pose_preserves_native_frames_and_attributes() {
    for scales in [[2., 3., 4.], [-2., 3., 4.], [2., -3., -4.]] {
        for normal in [Vector3::UNIT_Z, Vector3::new(0., 0.6, 0.8)] {
            for perpendicular in [false, true] {
                let (mut scene, definition, insert) = fixture();
                let parameter = grips(&scene.document, definition, insert)[0].parameter;
                orientation::set_alignment_type(
                    &mut scene.document,
                    definition,
                    parameter,
                    perpendicular,
                )
                .unwrap();
                let EntityType::Insert(reference) = scene.document.get_entity_mut(insert).unwrap()
                else {
                    panic!()
                };
                reference.set_x_scale(scales[0]);
                reference.set_y_scale(scales[1]);
                reference.set_z_scale(scales[2]);
                reference.normal = normal;
                reference.rotation = PI / 6.;
                let mut attribute = AttributeEntity::new("PART".into(), "A-17".into());
                attribute.insertion_point = Vector3::new(105., 207., 30.);
                attribute.alignment_point = attribute.insertion_point;
                reference.attributes.push(attribute);
                let alignment = grips(&scene.document, definition, insert).remove(0);
                let a = alignment.plane.to_world(DVec3::new(50., -20., 0.));
                let b = alignment.plane.to_world(DVec3::new(50., 20., 0.));
                scene
                    .document
                    .add_entity(EntityType::Line(NativeLine::from_points(
                        actions::native(a),
                        actions::native(b),
                    )))
                    .unwrap();
                let before = scene.document.clone();
                let targets = Targets::from_scene(&scene, insert, alignment.plane);
                assert_eq!(targets.curves.len(), 1, "exclude moving reference geometry");
                let cursor = alignment.plane.to_world(DVec3::new(50., 7., 0.));
                let (destination, angle) = targets.pose(&alignment, cursor, None, false).unwrap();
                let (_, flipped) = targets.pose(&alignment, cursor, None, true).unwrap();
                assert!((flipped - angle - PI).abs() < 1e-9);
                let changed = alignment.transformed(destination, angle).unwrap();
                let EntityType::Insert(reference) = &changed else {
                    panic!()
                };
                assert!((reference.x_scale() - scales[0]).abs() < 1e-9);
                assert!((reference.y_scale() - scales[1]).abs() < 1e-9);
                assert!((reference.z_scale() - scales[2]).abs() < 1e-9);
                assert!(reference.normal.distance(&normal) < 1e-9);
                assert_eq!(reference.attributes[0].value, "A-17");
                let expected_attribute = alignment.anchor
                    + glam::DQuat::from_axis_angle(alignment.plane.z, angle)
                        * (actions::v(alignment.reference.attributes[0].insertion_point)
                            - alignment.anchor)
                    + destination
                    - alignment.anchor;
                let attribute = &reference.attributes[0];
                let attribute_world = codec::types::Matrix3::arbitrary_axis(attribute.normal)
                    * attribute.insertion_point;
                assert!(actions::v(attribute_world).distance(expected_attribute) < 1e-8);
                *scene.document.get_entity_mut(insert).unwrap() = changed;
                let after = grips(&scene.document, definition, insert).remove(0);
                assert!(after.anchor.distance(cursor) < 1e-8);
                let tangent = (b - a).normalize();
                if perpendicular {
                    assert!(after.direction.dot(tangent).abs() < 1e-9);
                } else {
                    assert!(after.direction.cross(tangent).length() < 1e-9);
                }
                assert_eq!(scene.document.objects, before.objects);
                for entity in before
                    .entities()
                    .filter(|entity| entity.common().handle != insert)
                {
                    assert_eq!(
                        scene.document.get_entity(entity.common().handle),
                        Some(entity)
                    );
                }
            }
        }
    }
}

#[test]
fn block_alignment_exact_curve_targets_cover_native_curve_families() {
    let mut arc = Arc::default();
    arc.center = Vector3::ZERO;
    arc.radius = 10.;
    arc.start_angle = 0.;
    arc.end_angle = PI;
    let mut ellipse = Ellipse::default();
    ellipse.major_axis = Vector3::new(12., 0., 0.);
    ellipse.minor_axis_ratio = 0.4;
    let mut polyline = LwPolyline::default();
    polyline.vertices = vec![
        LwVertex::with_bulge(Vector2::new(0., 0.), 1.),
        LwVertex::from_coords(10., 0.),
    ];
    let mut spline = Spline::default();
    spline.degree = 3;
    spline.fit_points = vec![
        Vector3::ZERO,
        Vector3::new(1., 2., 0.),
        Vector3::new(3., 1., 0.),
        Vector3::new(5., 4., 0.),
    ];
    let entities = vec![
        EntityType::Line(NativeLine::from_points(
            Vector3::ZERO,
            Vector3::new(10., 5., 0.),
        )),
        EntityType::Circle(Circle::from_center_radius(Vector3::ZERO, 10.)),
        EntityType::Arc(arc),
        EntityType::Ellipse(ellipse),
        EntityType::LwPolyline(polyline),
        EntityType::Spline(spline),
        EntityType::Ray(Ray::new(Vector3::ZERO, Vector3::new(1., 2., 0.))),
        EntityType::XLine(XLine::new(Vector3::ZERO, Vector3::new(1., -2., 0.))),
    ];
    for entity in entities {
        let (mut scene, definition, insert) = fixture();
        let EntityType::Insert(reference) = scene.document.get_entity_mut(insert).unwrap() else {
            panic!()
        };
        reference.insert_point.z = 0.;
        let alignment = grips(&scene.document, definition, insert).remove(0);
        let native = crate::entities::curve::entity_curve(&entity).unwrap();
        let segments = native.curve.segments();
        let curve = segments.first().unwrap_or(&native.curve);
        let point = curve.point_at(0.37);
        let tangent = curve.tangent_at(0.37);
        let cursor = DVec3::from_array(native.plane.point_at(point));
        let direction = DVec3::from_array(native.plane.point_at(tangent))
            - DVec3::from_array(native.plane.origin);
        scene.document.add_entity(entity).unwrap();
        let targets = Targets::from_scene(&scene, insert, alignment.plane);
        assert!(!targets.curves.is_empty());
        let (destination, angle) = targets.pose(&alignment, cursor, None, false).unwrap();
        assert!(destination.distance(cursor) < 1e-6);
        let rotated = glam::DQuat::from_axis_angle(alignment.plane.z, angle) * alignment.direction;
        assert!(
            rotated.cross(direction.normalize()).length() < 1e-6,
            "curve {curve:?}, angle {angle}"
        );
    }
}

#[test]
fn block_alignment_nested_targets_respect_visibility_and_clips() {
    let (mut scene, definition, insert) = fixture();
    let EntityType::Insert(reference) = scene.document.get_entity_mut(insert).unwrap() else {
        panic!()
    };
    reference.insert_point.z = 0.;
    let alignment = grips(&scene.document, definition, insert).remove(0);
    let mut polyline = LwPolyline::default();
    polyline.vertices = vec![
        LwVertex::with_bulge(Vector2::new(0., 0.), 1.),
        LwVertex::from_coords(10., 0.),
    ];
    scene.define_block_raw(
        "TARGET",
        Vector3::ZERO,
        vec![EntityType::LwPolyline(polyline)],
    );
    let target = scene
        .document
        .add_entity(EntityType::Insert(
            Insert::new("TARGET", Vector3::ZERO).with_scale(2., 3., 1.),
        ))
        .unwrap();
    let targets = Targets::from_scene(&scene, insert, alignment.plane);
    assert_eq!(targets.curves.len(), 1);
    assert!(
        matches!(targets.curves[0], Curve::Ellipse(_)),
        "nonuniform bulge retains exact ellipse"
    );
    let cursor = DVec3::new(10., -15., 0.);
    let (_, angle) = targets.pose(&alignment, cursor, None, false).unwrap();
    assert!(angle.abs() < 1e-7);
    assert!(super::super::pick::xclip::set_insert_clip(
        &mut scene.document,
        target,
        &[[0., -20.], [9., -20.], [9., 1.], [0., 1.]],
        false,
        ([0., -20.], [20., 1.])
    ));
    let targets = Targets::from_scene(&scene, insert, alignment.plane);
    assert!(!targets.curves.is_empty());
    for curve in &targets.curves {
        for i in 0..=10 {
            let p = curve.point_at(i as f64 / 10.);
            let world = alignment.plane.to_world(DVec3::new(p[0], p[1], 0.));
            assert!(world.x <= 9. + 1e-7, "clipped target {world}");
        }
    }
    scene.select_entity(target, true);
    scene.hide_selected();
    assert!(Targets::from_scene(&scene, insert, alignment.plane)
        .curves
        .is_empty());
}

#[test]
fn block_alignment_aperture_projection_fallback_and_invalid_input() {
    let (mut scene, definition, insert) = fixture();
    let alignment = grips(&scene.document, definition, insert).remove(0);
    let a = alignment.plane.to_world(DVec3::new(50., -20., 0.));
    let b = alignment.plane.to_world(DVec3::new(50., 20., 0.));
    scene
        .document
        .add_entity(EntityType::Line(NativeLine::from_points(
            actions::native(a),
            actions::native(b),
        )))
        .unwrap();
    let targets = Targets::from_scene(&scene, insert, alignment.plane);
    let cursor = alignment.plane.to_world(DVec3::new(50.02, 7., 30.));
    let context = |scale| PointPickContext {
        view: glam::Mat4::from_scale(glam::Vec3::splat(scale)),
        eye: alignment.anchor,
        bounds: iced::Rectangle::with_size(iced::Size::new(1000., 800.)),
        aperture_px: 5.,
    };
    let (point, angle) = targets
        .pose(&alignment, cursor, Some(context(0.01)), false)
        .unwrap();
    assert!((alignment.plane.to_local(point).x - 50.).abs() < 1e-8);
    assert!((angle.abs() - FRAC_PI_2).abs() < 1e-8);
    let (point, angle) = targets
        .pose(&alignment, cursor, Some(context(1.)), false)
        .unwrap();
    assert!((alignment.plane.to_local(point).x - 50.02).abs() < 1e-8);
    assert_eq!(angle, 0.);
    assert!(targets.pose(&alignment, DVec3::NAN, None, false).is_err());
    assert!(alignment.transformed(cursor, 0.).is_err());
    assert!(alignment.transformed(alignment.anchor, f64::NAN).is_err());
}

#[test]
fn block_alignment_clipped_unbounded_curves_keep_inside_origin() {
    let boundary = [[-2., -2.], [2., -2.], [2., 2.], [-2., 2.]]
        .into_iter()
        .zip([[2., -2.], [2., 2.], [-2., 2.], [-2., -2.]])
        .map(|(start, end)| Curve::Line(Line { start, end }))
        .collect::<Vec<_>>();
    for curve in [
        Curve::Ray(geom2d::Ray {
            origin: [0., 0.],
            direction: [1., 0.],
        }),
        Curve::Ray(geom2d::Ray {
            origin: [0., 0.],
            direction: [2., 0.],
        }),
        Curve::XLine(geom2d::XLine {
            base: [0., 0.],
            direction: [1., 0.],
        }),
    ] {
        let pieces = clipped(curve, &boundary);
        assert_eq!(pieces.len(), 1);
        assert!((pieces[0].point_at(1.)[0] - 2.).abs() < 1e-8);
        assert!(pieces[0].point_at(0.)[0] <= 0.);
    }
}
