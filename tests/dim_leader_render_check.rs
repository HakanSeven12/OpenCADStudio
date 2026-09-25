//! Check DIMTMOVE through the scene's actual dimension tessellation.

use codec::entities::{Dimension, DimensionAligned, DimensionLinear};
use codec::types::Vector3;
use codec::xdata::XDataValue;
use codec::EntityType;
use OpenCADStudio::entities::dim_override as ov;
use OpenCADStudio::scene::Scene;

#[test]
fn dim_leader_extends_only_when_requested() {
    for aligned in [false, true] {
        for movement in [0, 1, 2] {
            for (text_x, text_y) in [
                (-18.0, 89.0),
                (98.0, 89.0),
                (19.9, 89.0),
                (80.1, 89.0),
                (-18.0, 86.0),
                (98.0, 86.0),
            ] {
                let mut scene = Scene::new();
                let first = Vector3::new(20.0, 100.0, 0.0);
                let second = Vector3::new(80.0, 100.0, 0.0);
                let definition = Vector3::new(20.0, 86.0, 0.0);
                let mut dim = if aligned {
                    let mut d = DimensionAligned::new(first, second);
                    d.definition_point = definition;
                    Dimension::Aligned(d)
                } else {
                    let mut d = DimensionLinear::horizontal(first, second);
                    d.definition_point = definition;
                    Dimension::Linear(d)
                };
                let base = dim.base_mut();
                base.definition_point = definition;
                base.text_middle_point = Vector3::new(text_x, text_y, 0.0);
                base.insertion_point = base.text_middle_point;
                base.text_user_positioned = true;
                base.style_name = "Standard".into();
                let handle = scene.add_entity(EntityType::Dimension(dim));
                for (code, value) in [
                    (ov::DIMTMOVE, XDataValue::Integer16(movement)),
                    (ov::DIMTXT, XDataValue::Real(3.5)),
                    (ov::DIMASZ, XDataValue::Real(3.5)),
                    (ov::DIMGAP, XDataValue::Real(1.0)),
                ] {
                    ov::set(&mut scene.document, handle, code, Some(value));
                }
                scene.bump_entities(&[(handle, OpenCADStudio::scene::ChangeKind::Modified)]);
                let wires = scene.entity_wires();
                let extension = wires.iter().any(|wire| {
                    (0..wire.points.len().saturating_sub(1)).any(|index| {
                        let a = wire.point_world(index, 1.0);
                        let b = wire.point_world(index + 1, 1.0);
                        if !a.is_finite() || !b.is_finite() {
                            return false;
                        }
                        let near = if text_x > 80.0 { 80.0 } else { 20.0 };
                        (a.x - near).abs() < 1e-4
                            && (a.y - 86.0).abs() < 1e-4
                            && (b.y - 86.0).abs() < 1e-4
                            && if text_x > 80.0 {
                                b.x > text_x
                            } else {
                                b.x < text_x
                            }
                    })
                });
                assert_eq!(
                    extension,
                    movement == 1 && text_y == 89.0,
                    "aligned={aligned}, DIMTMOVE={movement}, text=({text_x},{text_y})"
                );
            }
        }
    }
}
