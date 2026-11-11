//! Thickened bodies written as ACIS records read back whole.

use document::entities::acis::types::SatDocument;
use kernel::brep::{analytic_mass_properties, revolve_surface, thicken, Body};
use kernel::geom2d::{Curve, Line};
use kernel::space::{Plane, Vec3};
use std::f64::consts::PI;

fn cylinder_sheet(angle: f64) -> Body {
    revolve_surface(
        Plane::from_axes([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        &[Curve::Line(Line {
            start: [4.0, 0.0],
            end: [4.0, 3.0],
        })],
        [0.0; 3],
        [0.0, 0.0, 1.0],
        angle,
    )
    .unwrap()
}

#[test]
fn thickened_sector_round_trips_through_acis() {
    let solid = thicken(&cylinder_sheet(PI * 1.99), 0.5).unwrap();
    let expected = analytic_mass_properties(&solid).unwrap();
    let mut sat = SatDocument::default();
    ocs_acis::append(&solid, &mut sat).unwrap();
    let (bodies, loss) = ocs_acis::lift(&sat);
    assert_eq!(bodies.len(), 1, "{loss:?}");
    assert!(bodies[0].validate().is_empty());
    assert!(bodies[0].edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
    let restored = analytic_mass_properties(&bodies[0]).unwrap();
    assert!((restored.volume - expected.volume).abs() < 1e-8);
    assert!(Vec3::from(restored.centroid).distance(Vec3::from(expected.centroid)) < 1e-8);
}
