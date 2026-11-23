use super::*;

    use crate::scene::model::wire_model::TangentGeom;

    fn plain(name: &str) -> WireModel {
        WireModel::solid(
            name.to_string(),
            vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            [1.0; 4],
            false,
        )
    }

    fn circle(name: &str) -> WireModel {
        let mut wire = plain(name);
        wire.tangent_geoms.push(TangentGeom::PlanarCircle {
            center: [0.0, 0.0, 0.0],
            axis_x: [1.0, 0.0, 0.0],
            axis_y: [0.0, 1.0, 0.0],
            radius: 2.0,
        });
        wire
    }

    fn ellipse(name: &str) -> WireModel {
        let mut wire = plain(name);
        wire.tangent_geoms.push(TangentGeom::PlanarEllipse {
            center: [0.0, 0.0, 0.0],
            major_axis: [2.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            minor_axis_ratio: 0.5,
            start_param: 0.0,
            end_param: std::f64::consts::TAU,
        });
        wire
    }

    #[test]
    fn each_wire_lands_in_the_bucket_the_old_predicate_chose() {
        let wires = [plain("1"), circle("2"), ellipse("3"), plain("4")];
        let refs: Vec<&WireModel> = wires.iter().collect();
        let depth_map = rustc_hash::FxHashMap::default();

        let (circles, ellipses, regular, blocks) =
            Pipeline::classify_highlight_wires(&refs, None, &depth_map);

        for wire in &refs {
            let was_regular = wire.render_instance.is_none()
                && circle_gpu::extract_circle_instances(wire, 0.0).is_none()
                && ellipse_gpu::extract_ellipse_instances(wire, 0.0).is_none();
            assert_eq!(
                was_regular,
                regular.iter().any(|kept| std::ptr::eq(*kept, *wire)),
                "wire {} disagrees with the old predicate",
                wire.name,
            );
        }

        assert_eq!(regular.len(), 2, "two plain lines");
        assert_eq!(circles.len(), 1, "one circle instance");
        assert_eq!(ellipses.len(), 1, "one ellipse instance");
        assert!(blocks.is_empty(), "no block wires in this sample");
    }

    // Hover recolours every instance; a selection only recolours when a tint is
    // configured. Both go through the same argument, so it has to actually
    // reach the instances.
    #[test]
    fn the_colour_override_reaches_the_instances() {
        let wires = [circle("2"), ellipse("3")];
        let refs: Vec<&WireModel> = wires.iter().collect();
        let depth_map = rustc_hash::FxHashMap::default();

        let (tinted_circles, tinted_ellipses, _, _) =
            Pipeline::classify_highlight_wires(&refs, Some(WireModel::HOVER), &depth_map);
        assert_eq!(tinted_circles[0].color, WireModel::HOVER);
        assert_eq!(tinted_ellipses[0].color, WireModel::HOVER);

        let (plain_circles, _, _, _) = Pipeline::classify_highlight_wires(&refs, None, &depth_map);
        assert_ne!(
            plain_circles[0].color,
            WireModel::HOVER,
            "without a tint the extractor's own colour stands",
        );
    }
