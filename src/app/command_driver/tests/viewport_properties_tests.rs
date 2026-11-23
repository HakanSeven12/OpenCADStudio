// A layout viewport's standard scale and display lock are driven from its
// Properties rows; the annotation scale is reported there too.
mod viewport_properties_tests {
    use super::super::*;
    use document::entities::Viewport;
    use document::objects::Scale;
    use document::EntityType;

    fn row(app: &OpenCADStudio, field: &str) -> crate::scene::model::object::Property {
        app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find(|property| property.field == field)
            .unwrap_or_else(|| panic!("the {field} row is in Properties"))
            .clone()
    }

    #[test]
    fn a_viewport_scale_and_lock_follow_its_properties_rows() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        {
            let scene = &mut app.tabs[i].scene;
            scene.document.add_layout("Sheet").unwrap();
            scene.set_current_layout("Sheet".into());
            let mut viewport = Viewport::new();
            viewport.id = 2;
            viewport.width = 100.0;
            viewport.height = 50.0;
            viewport.view_height = 1000.0;
            viewport.turn_on();
            scene.add_entity(EntityType::Viewport(Box::new(viewport)));
            for name in ["1:100", "1:50", "1:200"] {
                crate::scene::annotative::ensure_scale_object(
                    &mut scene.document,
                    &Scale::new(name.into(), 1.0, 100.0),
                );
            }
        }
        let handle = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Viewport(viewport) if viewport.id == 2 => Some(viewport.common.handle),
                _ => None,
            })
            .unwrap();
        app.tabs[i].scene.select_entity(handle, true);
        app.tabs[i].selected_handle = Some(handle);
        app.refresh_properties();

        assert!(matches!(
            row(&app, "vscale_std").value,
            crate::scene::model::object::PropValue::Choice { .. }
        ));
        assert!(matches!(
            row(&app, "vp_anno_scale").value,
            crate::scene::model::object::PropValue::ReadOnly(_)
        ));
        assert!(matches!(
            row(&app, "vp_locked").value,
            crate::scene::model::object::PropValue::BoolToggle { .. }
        ));

        let _ = app
            .set_property_value(row(&app, "vscale_std"), &serde_json::json!("1:100"))
            .unwrap();
        app.finish_pending_history(i);
        let document::EntityType::Viewport(before) =
            app.tabs[i].scene.document.get_entity(handle).unwrap()
        else {
            panic!("the viewport stays a Viewport")
        };
        let before_scale = before.custom_scale;
        let _ = app
            .set_property_value(row(&app, "vscale_std"), &serde_json::json!("1:50"))
            .unwrap();
        app.finish_pending_history(i);

        let document::EntityType::Viewport(viewport) =
            app.tabs[i].scene.document.get_entity(handle).unwrap()
        else {
            panic!("the viewport stays a Viewport")
        };
        assert!(
            (viewport.custom_scale - before_scale).abs() > 1e-6,
            "the typed standard scale reached the viewport: {before_scale} -> {}",
            viewport.custom_scale
        );
        let chosen_scale = viewport.custom_scale;

        let _ = app
            .set_property_value(row(&app, "vp_locked"), &serde_json::json!(true))
            .unwrap();
        app.finish_pending_history(i);
        let document::EntityType::Viewport(locked) =
            app.tabs[i].scene.document.get_entity(handle).unwrap()
        else {
            panic!("the viewport stays a Viewport")
        };
        assert!(locked.status.locked, "the Display locked row locked the viewport");

        let _ = app
            .set_property_value(row(&app, "vscale_std"), &serde_json::json!("1:200"))
            .unwrap();
        app.finish_pending_history(i);
        let document::EntityType::Viewport(after) =
            app.tabs[i].scene.document.get_entity(handle).unwrap()
        else {
            panic!("the viewport stays a Viewport")
        };
        assert!(
            (after.custom_scale - chosen_scale).abs() < 1e-6,
            "a locked viewport refuses a new scale"
        );
    }
}
