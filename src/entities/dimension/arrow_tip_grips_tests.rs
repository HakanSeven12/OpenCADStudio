// Tests for arrow_tip_grip.
use super::*;
use crate::t;

use super::*;
use crate::scene::model::object::{GripMenuAction, GripShape};

fn rotated() -> Dimension {
    let mut d =
        DimensionLinear::horizontal(Vector3::new(2.0, 3.0, 0.0), Vector3::new(12.0, 7.0, 0.0));
    d.rotation = std::f64::consts::PI / 6.0;
    d.definition_point = Vector3::new(4.0, 11.0, 0.0);
    d.base.text_middle_point = Vector3::new(20.0, 25.0, 0.0);
    d.base.text_user_positioned = true;
    Dimension::Linear(d)
}

fn aligned() -> Dimension {
    let mut d = DimensionAligned::new(Vector3::new(2.0, 3.0, 0.0), Vector3::new(8.0, 11.0, 0.0));
    d.definition_point = Vector3::new(1.0, 10.0, 0.0);
    d.base.text_middle_point = Vector3::new(20.0, 25.0, 0.0);
    d.base.text_user_positioned = true;
    Dimension::Aligned(d)
}

fn assert_grips(mut dim: Dimension, second: DVec3, tips: [DVec3; 2]) {
    let grips = dim.grips();
    assert_eq!(grips.len(), 6, "two arrow-tip grips must be appended");
    let expected = [
        DVec3::new(2.0, 3.0, 0.0),
        second,
        (tips[0] + tips[1]) * 0.5,
        DVec3::new(20.0, 25.0, 0.0),
        tips[0],
        tips[1],
    ];
    for (id, point) in expected.into_iter().enumerate() {
        assert_eq!(grips[id].id, id);
        assert!(
            grips[id].world.distance(point) < 1e-9,
            "grip {id}: {:?}",
            grips[id]
        );
        assert_eq!(grips[id].is_midpoint, id != 0);
        assert_eq!(grips[id].shape, GripShape::Square);
    }
    for id in [4, 5] {
        assert!(dim
            .grip_menu(id)
            .iter()
            .any(|item| item.action == GripMenuAction::FlipArrow));
        dim.apply_grip_menu(id, GripMenuAction::FlipArrow);
    }
    assert!(dim.base().flip_arrow1 && dim.base().flip_arrow2);
}

#[test]
fn rotated_linear_has_projected_tips_and_keeps_ids_0_to_3() {
    let s = 3.0_f64.sqrt();
    assert_grips(
        rotated(),
        DVec3::new(12.0, 7.0, 0.0),
        [
            DVec3::new(2.5 - 2.0 * s, 9.0 - s * 0.5, 0.0),
            DVec3::new(10.0 - s, 10.0 + 2.0 * s, 0.0),
        ],
    );
}

#[test]
fn aligned_has_projected_tips_and_keeps_ids_0_to_3() {
    assert_grips(
        aligned(),
        DVec3::new(8.0, 11.0, 0.0),
        [DVec3::new(-2.0, 6.0, 0.0), DVec3::new(4.0, 14.0, 0.0)],
    );
}

#[test]
fn dragging_either_tip_moves_the_line_through_the_target() {
    let target = DVec3::new(9.0, 17.0, 0.0);
    for dim in [rotated(), aligned()] {
        for id in [4, 5] {
            let mut moved = dim.clone();
            moved.apply_grip(id, GripApply::Absolute(target));
            let definition = dv3(&dimension_definition_point(&moved));
            assert!(
                definition.distance(target) < 1e-9,
                "grip {id} did not move definition_point"
            );
            assert!(dv3(&moved.base().definition_point).distance(target) < 1e-9);
            let tips = moved.grips();
            let axis = (tips[5].world - tips[4].world).normalize();
            assert!((target - tips[4].world).cross(axis).length() < 1e-9);
            let mut middle = dim.clone();
            middle.apply_grip(2, GripApply::Absolute(target));
            assert_eq!(
                moved.base().text_middle_point,
                middle.base().text_middle_point
            );
            let original_tip = dim.grips()[id].world;
            let mut translated = dim.clone();
            translated.apply_grip(id, GripApply::Translate(target - original_tip));
            let tips = translated.grips();
            let axis = (tips[5].world - tips[4].world).normalize();
            assert!((target - tips[4].world).cross(axis).length() < 1e-9);
            assert_eq!(translated.grips()[0].world, dim.grips()[0].world);
            assert_eq!(translated.grips()[1].world, dim.grips()[1].world);
        }
    }
}
