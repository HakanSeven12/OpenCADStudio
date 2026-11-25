use super::*;

#[test]
fn plugin_add_layer_with_defaults_and_modify() {
    use ocs_plugin_api::host::LayerConfig;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    // 1. Add layer with minimal config (only name specified)
    let config = LayerConfig {
        name: "ELECTRICAL".to_string(),
        ..Default::default()
    };
    let handle = host.add_layer(config).expect("should create layer");
    assert_ne!(handle, document::Handle::NULL);

    // Verify defaults were applied
    let layer = host
        .document()
        .layers
        .get("ELECTRICAL")
        .expect("layer should exist");
    assert_eq!(layer.color, document::types::Color::Index(7));
    assert_eq!(layer.line_type, "Continuous");
    assert_eq!(layer.line_weight, document::types::LineWeight::ByLayer);
    assert!(!layer.flags.off);
    assert!(!layer.flags.frozen);
    assert!(!layer.flags.locked);
    assert!(layer.is_plottable);

    // 2. Duplicate add_layer should be rejected (return None)
    let dup_config = LayerConfig {
        name: "ELECTRICAL".to_string(),
        color: Some(document::types::Color::Index(1)),
        ..Default::default()
    };
    assert!(
        host.add_layer(dup_config).is_none(),
        "duplicate layer should return None"
    );

    // 3. Modify only color and locked; other properties should remain untouched
    let mod_config = LayerConfig {
        name: "ELECTRICAL".to_string(),
        color: Some(document::types::Color::Index(1)),
        locked: Some(true),
        ..Default::default()
    };
    assert!(host.modify_layer(mod_config));

    let updated = host
        .document()
        .layers
        .get("ELECTRICAL")
        .expect("layer should exist");
    assert_eq!(updated.color, document::types::Color::Index(1)); // Modified to red
    assert!(updated.flags.locked); // Modified to locked
    assert_eq!(updated.line_type, "Continuous"); // Kept as-is
    assert_eq!(updated.line_weight, document::types::LineWeight::ByLayer); // Kept as-is
    assert!(!updated.flags.off); // Kept as-is

    // 4. Modify nonexistent layer returns false
    let non_existent = LayerConfig {
        name: "DOES_NOT_EXIST".to_string(),
        color: Some(document::types::Color::Index(2)),
        ..Default::default()
    };
    assert!(!host.modify_layer(non_existent));
}

/// The catalog linetypes a new drawing leaves out: the ones that draw
/// shapes only this application defines.
const LEFT_OUT_OF_A_NEW_DRAWING: [&str; 5] = ["POINT1", "POINT2", "PLANT", "DRAINAGE", "DRAINAGE2"];

fn layer_on(name: &str, linetype: &str) -> ocs_plugin_api::host::LayerConfig {
    ocs_plugin_api::host::LayerConfig {
        name: name.to_string(),
        linetype: Some(linetype.to_string()),
        ..Default::default()
    }
}

/// A plugin that names a catalog linetype the drawing lacks for a new
/// layer gets it loaded, as a pick in the layer list does.
#[test]
fn a_plugin_layer_is_created_on_a_catalog_linetype_the_drawing_lacks() {
    for linetype in LEFT_OUT_OF_A_NEW_DRAWING {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        assert!(
            host.document().line_types.get(linetype).is_none(),
            "{linetype}: a new drawing leaves it out"
        );

        let created = host.add_layer(layer_on("SITE", &linetype.to_lowercase()));

        assert!(created.is_some(), "{linetype}: the layer is created");
        assert_eq!(
            host.document().layers.get("SITE").unwrap().line_type,
            linetype
        );
        assert!(
            host.document()
                .line_types
                .get(linetype)
                .is_some_and(|loaded| loaded.is_complex()),
            "{linetype}: loaded with its shapes"
        );
    }
}

#[test]
fn a_plugin_moves_a_layer_to_a_catalog_linetype_the_drawing_lacks() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    host.add_layer(ocs_plugin_api::host::LayerConfig {
        name: "SITE".to_string(),
        ..Default::default()
    })
    .expect("a layer on Continuous");

    assert!(host.modify_layer(layer_on("SITE", "POINT2")));

    assert_eq!(
        host.document().layers.get("SITE").unwrap().line_type,
        "POINT2"
    );
    assert!(host.document().line_types.get("POINT2").is_some());
}

#[test]
fn the_layer_table_route_takes_a_catalog_linetype_the_drawing_lacks() {
    use ocs_plugin_api::host::TableOperation;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    let created = host.table_operation(TableOperation::LayerCreate {
        config: layer_on("DRAINS", "DRAINAGE"),
    });
    assert!(created.is_ok(), "create: {created:?}");
    assert_eq!(
        host.document().layers.get("DRAINS").unwrap().line_type,
        "DRAINAGE"
    );

    let modified = host.table_operation(TableOperation::LayerModify {
        config: layer_on("DRAINS", "PLANT"),
    });
    assert!(modified.is_ok(), "modify: {modified:?}");
    assert_eq!(
        host.document().layers.get("DRAINS").unwrap().line_type,
        "PLANT"
    );
}

/// Naming a linetype for a layer loads that one alone: the other catalog
/// linetypes the drawing lacks stay out of it.
#[test]
fn a_plugin_layer_loads_only_the_linetype_it_names() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    for purged in ["DASHED", "CENTER"] {
        assert!(host.document_mut().line_types.remove(purged).is_some());
    }
    let before = host.document().line_types.iter().count();

    assert!(host.add_layer(layer_on("AXES", "Center")).is_some());

    assert_eq!(
        host.document().layers.get("AXES").unwrap().line_type,
        "CENTER"
    );
    assert!(
        host.document().line_types.get("DASHED").is_none(),
        "a purged linetype nobody named stays purged"
    );
    assert_eq!(host.document().line_types.iter().count(), before + 1);
}

#[test]
fn a_plugin_layer_is_refused_a_linetype_nobody_defines() {
    use ocs_plugin_api::host::TableOperation;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let before = host.document().line_types.iter().count();

    assert!(host.add_layer(layer_on("SITE", "NO_SUCH")).is_none());
    let refusal = host
        .table_operation(TableOperation::LayerCreate {
            config: layer_on("SITE", "NO_SUCH"),
        })
        .unwrap_err();

    assert!(refusal.contains("does not exist"), "{refusal}");
    assert!(host.document().layers.get("SITE").is_none());
    assert_eq!(host.document().line_types.iter().count(), before);
}
