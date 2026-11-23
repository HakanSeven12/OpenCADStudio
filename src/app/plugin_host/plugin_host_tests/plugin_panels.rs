use super::*;

fn api16_text_panel() -> ocs_plugin_api::dialog::PanelDef {
    use ocs_plugin_api::dialog::{DialogControl, PanelDef};

    PanelDef::new("panel", "Panel")
        .with_control(DialogControl::Text {
            id: "length".into(),
            label: "Length".into(),
            default: "1.0".into(),
        })
        .with_control(DialogControl::Button {
            id: "submit".into(),
            label: "Submit".into(),
            action: Some("PANEL_SUBMIT".into()),
            align_h: None,
            width: None,
        })
}

fn api16_number_panel() -> ocs_plugin_api::dialog::PanelDef {
    let mut panel = api16_text_panel();
    panel.controls[0] = ocs_plugin_api::dialog::DialogControl::Number {
        id: "length".into(),
        label: "Length".into(),
        default: 1.0,
        min: None,
        max: None,
        step: None,
    };
    panel
}

#[test]
fn api16_panel_registration_is_idempotent_and_owner_authenticated() {
    use ocs_plugin_api::dialog::DialogValue;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let tab_id = app.tabs[0].id;
    let mut host = HostSession::new(&mut app, 0);

    assert!(host.register_panel_for_plugin("OWNER", api16_text_panel()));
    assert!(host.update_panel_for_plugin(
        "OWNER",
        "panel",
        std::collections::HashMap::from([(
            "length".to_string(),
            DialogValue::Text("2.5".to_string()),
        )]),
    ));
    assert!(
        host.register_panel_for_plugin("OWNER", api16_text_panel()),
        "unchanged re-registration must not reset the live draft"
    );
    let before = host.app.plugin_panels["panel"].clone();

    let mut foreign_panel = api16_text_panel();
    foreign_panel.title = "Foreign replacement".into();
    assert!(!host.register_panel_for_plugin("OTHER", foreign_panel));
    assert!(!host.update_panel_for_plugin(
        "OTHER",
        "panel",
        std::collections::HashMap::from([(
            "length".to_string(),
            DialogValue::Text("99.0".to_string()),
        )]),
    ));
    assert!(!host.toggle_panel_for_plugin("OTHER", "panel", Some(true)));
    assert!(host.panel_snapshot_for_plugin("OTHER", "panel").is_err());
    assert_eq!(host.app.plugin_panels.get("panel"), Some(&before));

    let snapshot = host.panel_snapshot_for_plugin("OWNER", "panel").unwrap();
    assert_eq!(snapshot.plugin_id, "OWNER");
    assert_eq!(snapshot.panel_id, "panel");
    assert_eq!(snapshot.tab_id, tab_id);
    assert_eq!(snapshot.values.len(), 1);
    assert_eq!(
        snapshot.values.get("length"),
        Some(&DialogValue::Text("2.5".to_string()))
    );
    assert_eq!(snapshot.raw_text.len(), 1);
    assert_eq!(
        snapshot.raw_text.get("length").map(String::as_str),
        Some("2.5")
    );
    assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
    let direct = host
        .panel_snapshot("PANEL")
        .expect("ordinary HostApi command read-back works");
    assert_eq!(direct.tab_id, tab_id);
    assert_eq!(direct.values.get("length"), snapshot.values.get("length"));
    assert!(!host.app.plugin_panels["panel"].visible);
    assert_eq!(host.app.plugin_panels.get("panel"), Some(&before));
}

#[test]
fn api16_panel_snapshot_uses_the_real_session_tab_without_document_writes() {
    use ocs_plugin_api::dialog::DialogValue;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let first_tab_id = app.tabs[0].id;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host.register_panel_for_plugin("OWNER", api16_text_panel()));
    }
    let second_tab_id = app.push_test_document();
    assert_eq!(second_tab_id, app.tabs[1].id);
    assert_ne!(first_tab_id, second_tab_id);
    assert_eq!(app.active_tab, 0);

    let first_document = app.tabs[0].scene.document.clone();
    let second_document = app.tabs[1].scene.document.clone();
    let first = HostSession::new(&mut app, 0)
        .panel_snapshot_for_plugin("OWNER", "panel")
        .unwrap();
    let second = HostSession::new(&mut app, 1)
        .panel_snapshot_for_plugin("OWNER", "panel")
        .unwrap();

    assert_eq!(first.tab_id, first_tab_id);
    assert_eq!(second.tab_id, second_tab_id);
    assert_ne!(first.tab_id, second.tab_id);
    for snapshot in [&first, &second] {
        assert_eq!(snapshot.plugin_id, "OWNER");
        assert_eq!(snapshot.panel_id, "panel");
        assert_eq!(snapshot.values.len(), 1);
        assert_eq!(
            snapshot.values.get("length"),
            Some(&DialogValue::Text("1.0".to_string()))
        );
        assert_eq!(snapshot.raw_text.len(), 1);
        assert_eq!(
            snapshot.raw_text.get("length").map(String::as_str),
            Some("1.0")
        );
        assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
    }
    assert_eq!(app.tabs[0].scene.document, first_document);
    assert_eq!(app.tabs[1].scene.document, second_document);
    assert_eq!(app.active_tab, 0);
    assert!(app.tabs[0].history.pending.is_none());
    assert!(app.tabs[1].history.pending.is_none());
}

#[test]
fn api16_panel_snapshot_rejects_malformed_and_missing_panels_without_writes() {
    use ocs_plugin_api::dialog::DialogValue;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let document_before = app.tabs[0].scene.document.clone();
    let undo_before = app.tabs[0].history.undo_stack.len();
    let mut host = HostSession::new(&mut app, 0);
    assert!(host.register_panel_for_plugin("OWNER", api16_number_panel()));
    assert!(host.update_panel_for_plugin(
        "OWNER",
        "panel",
        std::collections::HashMap::from([(
            "length".to_string(),
            DialogValue::Text("-".to_string()),
        )]),
    ));
    let snapshot = host.panel_snapshot_for_plugin("OWNER", "panel").unwrap();

    assert_eq!(
        snapshot.raw_text.get("length").map(String::as_str),
        Some("-")
    );
    assert!(!snapshot.values.contains_key("length"));
    assert!(
        snapshot.errors.iter().any(|error| {
            error.contains("invalid finite number")
                && error.contains("'length'")
                && error.contains("'-'")
        }),
        "{:?}",
        snapshot.errors
    );

    let error = host
        .panel_snapshot_for_plugin("OWNER", "missing")
        .unwrap_err();
    assert!(error.contains("not registered"), "{error}");
    assert_eq!(host.document(), &document_before);
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), undo_before);
    assert!(host.app.tabs[0].history.pending.is_none());
}

#[test]
fn api17_panel_reset_preserves_owner_and_aliases_remain_authenticated() {
    use crate::app::Message;
    use ocs_plugin_api::dialog::DialogValue;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host.register_panel_for_plugin("OWNER", api16_text_panel()));
        assert!(host.update_panel_for_plugin(
            "OWNER",
            "panel",
            std::collections::HashMap::from([(
                "length".to_string(),
                DialogValue::Text("2.5".to_string()),
            )]),
        ));
    }

    // The built-in reset path must retain the authenticated owner and
    // restore the declaration's defaults, not brick subsequent access.
    let _ = app.update(Message::PluginPanelButtonClick(
        "panel".to_string(),
        "reset".to_string(),
        Some("reset".to_string()),
    ));
    let mut host = HostSession::new(&mut app, 0);
    let snapshot = host
        .panel_snapshot_for_plugin("OWNER", "panel")
        .expect("owner remains able to read after reset");
    assert_eq!(
        snapshot.values.get("length"),
        Some(&DialogValue::Text("1.0".into()))
    );
    assert!(host.register_panel_for_plugin("OWNER", api16_text_panel()));
    assert!(host.update_panel_for_plugin(
        "OWNER",
        "PANEL",
        std::collections::HashMap::from([(
            "length".to_string(),
            DialogValue::Text("3.0".to_string()),
        )]),
    ));
    assert!(host.toggle_panel_for_plugin("OWNER", "Panel", Some(true)));
    assert!(!host.update_panel_for_plugin("OTHER", "panel", std::collections::HashMap::new(),));
    assert!(host.panel_snapshot_for_plugin("OTHER", "Panel").is_err());
}

#[test]
fn api17_explicit_plugin_panel_reopens_in_clean_screen_without_document_writes() {
    use ocs_plugin_api::dialog::{DialogControl, PanelDef};

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    app.clean_screen = true;
    let mut host = HostSession::new(&mut app, 0);
    for id in ["cvifc.ifc_h_properties", "cvifc.ifc_v_properties"] {
        let panel = PanelDef::new(id, "IFC Alignment Segment")
            .with_command(format!(
                "CVI_IFC_{}_PANEL",
                if id.ends_with("h_properties") {
                    "H"
                } else {
                    "V"
                }
            ))
            .with_control(DialogControl::Text {
                id: "source_record".into(),
                label: "Source record".into(),
                default: "Alignment-Axis-h-seg-001".into(),
            })
            .with_control(DialogControl::Button {
                id: "apply".into(),
                label: "Apply".into(),
                action: Some("CVI_IFC_PANEL_APPLY".into()),
                align_h: None,
                width: None,
            });
        assert!(host.register_panel_for_plugin("opencad.civil_ifc", panel));
    }
    let panel_id = crate::ui::dock::PanelId::Plugin(1);
    let other_id = crate::ui::dock::PanelId::Plugin(2);
    assert!(!host.app.dock_panel_visible(panel_id));
    assert!(host.toggle_panel_for_plugin(
        "opencad.civil_ifc",
        "cvifc.ifc_h_properties",
        Some(true),
    ));
    assert!(host.app.dock_panel_visible(panel_id));
    assert!(
        matches!(
            host.app.dock.location(panel_id),
            Some((crate::app::config::DockSide::Right, _))
        ),
        "an explicit show places a new panel into the default dock"
    );
    assert!(host.app.dock_panel_on_screen(panel_id));
    assert!(!host.app.dock_panel_visible(other_id));
    assert!(host.app.plugin_panels["cvifc.ifc_h_properties"].visible);
    assert!(!host.app.plugin_panels["cvifc.ifc_v_properties"].visible);
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), 0);
    assert!(host.app.tabs[0].history.pending.is_none());
}

#[test]
fn plugin_panels_toggle_like_graphic_attributes_including_clean_screen() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    app.clean_screen = true;
    let graphic_id = crate::ui::dock::PanelId::GraphicAttributes;
    assert!(!app.dock_panel_visible(graphic_id));
    let _ = app.update(crate::app::Message::ToggleGraphicAttributes);
    assert!(app.show_graphic_attributes);
    assert!(!app.dock_panel_visible(graphic_id));
    let _ = app.update(crate::app::Message::ToggleGraphicAttributes);
    assert!(!app.show_graphic_attributes);
    assert!(!app.dock_panel_visible(graphic_id));
    let _ = app.update(crate::app::Message::ToggleGraphicAttributes);
    assert!(app.show_graphic_attributes);
    assert!(!app.dock_panel_visible(graphic_id));

    let mut host = HostSession::new(&mut app, 0);
    let panel_id = crate::ui::dock::PanelId::Plugin(1);
    assert!(host.register_panel_for_plugin("opencad.civil_ifc", api16_text_panel()));
    // Like Graphic Attributes, the panel stays hidden in Clean Screen;
    // opening it still binds its values without a document edit.
    app.clean_screen = false;
    let mut host = HostSession::new(&mut app, 0);
    assert!(!host.app.dock_panel_visible(panel_id));
    assert!(host.toggle_panel_for_plugin("opencad.civil_ifc", "panel", None));
    assert!(host.app.dock_panel_visible(panel_id));
    assert!(host.app.dock_panel_on_screen(panel_id));
    assert!(host.toggle_panel_for_plugin("opencad.civil_ifc", "panel", None));
    assert!(!host.app.dock_panel_visible(panel_id));
    assert!(!host.app.plugin_panels["panel"].visible);
    assert!(host.toggle_panel_for_plugin("opencad.civil_ifc", "panel", None));
    assert!(host.app.dock_panel_visible(panel_id));
    assert!(host.app.plugin_panels["panel"].visible);
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), 0);
    assert!(host.app.tabs[0].history.pending.is_none());
}

#[test]
fn api17_update_panel_for_plugin_allows_owned_modal_dialog() {
    use ocs_plugin_api::dialog::{DialogControl, DialogDef, DialogValue};

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let dlg = DialogDef::new("Test Dialog").with_control(DialogControl::Text {
        id: "field1".into(),
        label: "Field 1".into(),
        default: "initial".into(),
    });
    app.open_plugin_dialog_on_tab(0, dlg, Some("MY_PLUGIN".to_string()));

    let mut host = HostSession::new(&mut app, 0);

    // Foreign plugin cannot update the dialog
    assert!(!host.update_panel_for_plugin(
        "OTHER_PLUGIN",
        "Test Dialog",
        std::collections::HashMap::from([("field1".into(), DialogValue::Text("hacked".into()))]),
    ));

    // Owner plugin can update by title
    assert!(host.update_panel_for_plugin(
        "MY_PLUGIN",
        "Test Dialog",
        std::collections::HashMap::from([("field1".into(), DialogValue::Text("updated".into()))]),
    ));
    let field1 = |host: &HostSession| {
        host.app
            .plugin_dialog_state
            .as_ref()
            .unwrap()
            .text_values
            .get("field1")
            .cloned()
    };
    assert_eq!(field1(&host).as_deref(), Some("updated"));

    // Owner plugin can also update with empty panel_id (active dialog convention)
    assert!(host.update_panel_for_plugin(
        "MY_PLUGIN",
        "",
        std::collections::HashMap::from([(
            "field1".into(),
            DialogValue::Text("empty_id_update".into())
        )]),
    ));
    assert_eq!(field1(&host).as_deref(), Some("empty_id_update"));
}
