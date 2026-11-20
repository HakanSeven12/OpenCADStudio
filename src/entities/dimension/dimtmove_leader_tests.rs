// Tests for dimtmove_leader.
use super::*;
use crate::t;

use super::*;

fn v(x: f64, y: f64) -> Vector3 {
    Vector3::new(x, y, 0.0)
}

fn layout(position: Vec3, half_width: f32) -> DimensionTextLayout {
    DimensionTextLayout {
        position,
        width: half_width * 2.0,
        break_box: Some(TextBreak {
            center: position,
            half_width,
            half_height: 1.0,
            padding: 0.0,
            rotation: 0.0,
            through: false,
        }),
        horizontal: true,
    }
}

/// Radius dimensions draw no leader (unchanged).
#[test]
fn radius_has_no_leader() {
    let r = document::entities::DimensionRadius::new(v(0.0, 0.0), v(0.0, 5.0));
    assert!(
        dimtmove_leader_endpoints(&Dimension::Radius(r), layout(vec3_local(v(0.0, 8.0)), 2.0))
            .is_none()
    );
}
