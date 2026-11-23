use super::*;

use super::snap_keeps_elevation;
use crate::snap::SnapType;

#[test]
fn only_object_snaps_keep_elevation() {
    assert!(!snap_keeps_elevation(None), "a miss stays on the plane");
    assert!(
        !snap_keeps_elevation(Some(SnapType::Grid)),
        "grid stays planar"
    );
    for t in [
        SnapType::Endpoint,
        SnapType::Midpoint,
        SnapType::Center,
        SnapType::Node,
        SnapType::Quadrant,
        SnapType::Intersection,
        SnapType::Insertion,
        SnapType::Perpendicular,
        SnapType::Tangent,
        SnapType::Nearest,
        SnapType::ApparentIntersection,
        SnapType::Extension,
        SnapType::Parallel,
        SnapType::ObjectPick,
    ] {
        assert!(snap_keeps_elevation(Some(t)), "{t:?} must keep its Z");
    }
}
