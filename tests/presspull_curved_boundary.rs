use acadrust::{entities::Circle, types::Vector3, EntityType};
use glam::DVec3;
use OpenCADStudio::command::WorkingPlane;
use OpenCADStudio::scene::{model::presspull_model, Scene};

#[test]
fn area_picks_follow_exact_curves_and_keep_holes() {
    let mut scene = Scene::new();
    for radius in [1000.0, 500.0] {
        scene
            .document
            .add_entity(EntityType::Circle(Circle::from_center_radius(
                Vector3::new(0.0, 0.0, 0.0),
                radius,
            )))
            .unwrap();
    }
    // Halfway between display vertices, these points lie beyond the chords.
    let angle = std::f64::consts::PI / 48.0;
    for (radius, expected_loops) in [(999.0, 2), (499.0, 1)] {
        let point = DVec3::new(radius * angle.cos(), radius * angle.sin(), 0.0);
        let target =
            presspull_model::resolve_target(&scene, None, point, WorkingPlane::default(), false)
                .expect("point inside the exact circular boundary");
        let presspull_model::PresspullTargetKind::Profile { entity, .. } = target.kind else {
            panic!("expected a bounded profile");
        };
        let (_, loops, closed) = presspull_model::profile_geometry(&entity).unwrap();
        assert!(closed);
        assert_eq!(loops.len(), expected_loops);
        assert!(loops
            .iter()
            .flatten()
            .all(|curve| !matches!(curve, cadkernel::geom2d::Curve::Line(_))));
        let body = presspull_model::extrusion_body(&entity, [0.0, 0.0, 10.0]).unwrap();
        assert!(body.validate().is_empty());
    }
    let outside = DVec3::new(1001.0 * angle.cos(), 1001.0 * angle.sin(), 0.0);
    assert!(
        presspull_model::resolve_target(&scene, None, outside, WorkingPlane::default(), false,)
            .is_err()
    );
}
