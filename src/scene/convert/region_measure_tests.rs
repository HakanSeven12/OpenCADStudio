// Tests of the measures of a region built from loops and drawn by the
// scene (`entities::solid3d`).
use document::EntityType;
use ocs_geom::geom2d::{Curve, Line};

use crate::entities::solid3d::region_mass_props;
use crate::entities::traits::MassProps;

fn ring(points: &[[f64; 2]]) -> Vec<Curve> {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .take(points.len())
        .map(|(start, end)| Curve::Line(Line { start, end }))
        .collect()
}

#[test]
fn decoded_region_reports_exact_hole_area_perimeter_and_bounds() {
    let loops = vec![
        ring(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]),
        ring(&[[2.0, 2.0], [2.0, 4.0], [4.0, 4.0], [4.0, 2.0]]),
    ];
    let EntityType::Region(mut region) = crate::scene::model::presspull_model::region_from_loops(
        &loops,
        crate::entities::transform::WorkingPlane::default(),
    )
    .unwrap() else {
        unreachable!()
    };
    region.wires.clear();

    let MassProps {
        area, perimeter, ..
    } = region_mass_props(&region);
    assert!((area - 96.0).abs() < 1e-8, "{area}");
    assert!((perimeter - 48.0).abs() < 1e-8, "{perimeter}");
    let (min, max) = crate::scene::convert::tess::entity_bounds(&EntityType::Region(region));
    assert_eq!(min, [0.0, 0.0, 0.0]);
    assert_eq!(max, [10.0, 10.0, 0.0]);
}
