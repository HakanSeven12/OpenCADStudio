#[cfg(test)]
mod dim_update_regen_tests {
    use crate::app::OpenCADStudio;
    use crate::entities::dimension::as_dimension;
    use document::entities::Dimension;
    use document::entities::Line;
    use document::types::Vector3;

    /// A line with a linear dimension associated to its two ends.
    fn app_with_associative_dimension() -> (OpenCADStudio, document::Handle, document::Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(40.0, 0.0, 0.0),
            )));
        let dimension = app.tabs[i].scene.add_entity(
            crate::modules::annotate::linear_dim::linear_dimension_entity(
                glam::DVec3::ZERO,
                glam::DVec3::new(40.0, 0.0, 0.0),
                glam::DVec3::new(20.0, 10.0, 0.0),
                glam::DVec3::X,
                None,
            ),
        );
        app.tabs[i]
            .scene
            .attach_dimension_association(dimension, vec![Some(line), Some(line)]);
        (app, line, dimension)
    }

    fn last_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .rev()
            .find(|entry| matches!(entry.kind, crate::ui::command_line::EntryKind::Output))
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    }

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .filter(|entry| matches!(entry.kind, crate::ui::command_line::EntryKind::Output))
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn dimregen_solves_the_dimension_after_an_external_geometry_change() {
        let (mut app, line, dimension) = app_with_associative_dimension();
        let i = app.active_tab;
        if let Some(document::EntityType::Line(line)) =
            app.tabs[i].scene.document.get_entity_mut(line)
        {
            line.start = Vector3::new(10.0, 0.0, 0.0);
            line.end = Vector3::new(60.0, 0.0, 0.0);
        }

        let _ = app.run_command_line("DIMREGEN");

        let dimension = as_dimension(
            app.tabs[i]
                .scene
                .document
                .get_entity(dimension)
                .expect("dimension"),
        )
        .expect("dimension");
        let Dimension::Linear(linear) = dimension else {
            panic!("linear dimension");
        };
        assert!((linear.first_point.x - 10.0).abs() < 1e-9, "{linear:?}");
        assert!((linear.second_point.x - 60.0).abs() < 1e-9, "{linear:?}");
        assert!((linear.base.actual_measurement - 50.0).abs() < 1e-9);
    }

    #[test]
    fn dimupdate_pushes_the_current_style_onto_the_selection() {
        let (mut app, _, dimension) = app_with_associative_dimension();
        let i = app.active_tab;
        let mut style = document::tables::DimStyle::new("TALL");
        style.dimtxt = 5.0;
        let _ = app.tabs[i].scene.document.dim_styles.add(style);
        app.tabs[i].scene.document.header.current_dimstyle_name = "TALL".into();

        app.tabs[i].scene.select_entity(dimension, false);
        let _ = app.run_command_line("DIMUPDATE");

        let dimension = as_dimension(
            app.tabs[i]
                .scene
                .document
                .get_entity(dimension)
                .expect("dimension"),
        )
        .expect("dimension");
        assert_eq!(dimension.base().style_name, "TALL");
        assert!(
            last_output(&app).contains("applied 'TALL'"),
            "{}",
            last_output(&app)
        );
    }

    #[test]
    fn dimregen_reports_a_broken_association_instead_of_skipping_it() {
        let (mut app, line, _) = app_with_associative_dimension();
        let i = app.active_tab;
        app.tabs[i].scene.document.remove_entity(line);

        let _ = app.run_command_line("DIMREGEN");

        let output = all_output(&app);
        assert!(output.contains("1 broken"), "{output}");
        assert!(
            output.contains("no longer resolves"),
            "a broken reference must be named: {output}"
        );
    }
}
