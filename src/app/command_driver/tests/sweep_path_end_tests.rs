#[cfg(test)]
mod sweep_path_end_tests {
    use crate::app::OpenCADStudio;
    use crate::command::CmdResult;
    use document::entities::{LwPolyline, LwVertex};
    use document::types::Vector2;

    /// Picked near its far end, a path is swept from that end. A path whose
    /// start lies farther from the world origin than its end used to fail.
    #[test]
    fn sweep_from_the_picked_end_of_a_bent_path() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut hexagon = LwPolyline::new();
        for k in 0..6 {
            let a = (60.0 * k as f64).to_radians();
            hexagon
                .vertices
                .push(LwVertex::new(Vector2::new(5.0 * a.cos(), 5.0 * a.sin())));
        }
        hexagon.is_closed = true;
        let profile = app.tabs[i]
            .scene
            .add_entity(document::EntityType::LwPolyline(hexagon));
        let mut path = LwPolyline::new();
        path.vertices.push(LwVertex::new(Vector2::new(20.0, 0.0)));
        path.vertices.push(LwVertex::with_bulge(
            Vector2::new(60.0, 0.0),
            22.5f64.to_radians().tan(),
        ));
        path.vertices.push(LwVertex::new(Vector2::new(80.0, 20.0)));
        path.vertices.push(LwVertex::new(Vector2::new(80.0, 60.0)));
        let path = app.tabs[i]
            .scene
            .add_entity(document::EntityType::LwPolyline(path));
        let options = crate::command::SweepOptions {
            path_pick: Some(glam::DVec3::new(80.0, 40.0, 0.0)),
            ..crate::command::SweepOptions::default()
        };
        let _ = app.apply_cmd_result(CmdResult::SweepEntities {
            handles: vec![profile],
            path_handle: path,
            mode: crate::command::ExtrudeMode::Solid,
            options,
            color: [1.0; 4],
            expressions: [None, None],
        });
        let swept = app.tabs[i]
            .scene
            .document
            .entities()
            .any(|entity| matches!(entity, document::EntityType::Solid3D(_)));
        assert!(swept, "the sweep from the picked end makes a solid");
    }
}
