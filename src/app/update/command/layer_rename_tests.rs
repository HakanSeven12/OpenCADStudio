use super::*;

use crate::app::OpenCADStudio;
use document::entities::Line;
use document::{EntityType, Handle};

fn app_with_editing_layer() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let _ = app.on_layer_new();
    app
}

fn rename_layer(app: &mut OpenCADStudio, old_name: &str, new_name: &str) {
    let i = app.active_tab;
    assert!(
        app.tabs[i]
            .layers
            .layers
            .iter()
            .any(|layer| layer.name == old_name),
        "layer exists in panel"
    );
    app.tabs[i].layers.editing = Some(old_name.to_string());
    app.tabs[i].layers.edit_buf = new_name.to_string();
    let _ = app.on_layer_rename_commit();
}

#[test]
fn layer_rename_allows_ascii_case_only_change() {
    let mut app = app_with_editing_layer();
    let i = app.active_tab;
    let old_name = app.tabs[i].layers.edit_buf.clone();
    rename_layer(&mut app, &old_name, "TEST");
    let handle = app.tabs[i]
        .scene
        .document
        .layers
        .get("TEST")
        .unwrap()
        .handle;

    rename_layer(&mut app, "TEST", "test");

    let layer = app.tabs[i].scene.document.layers.get("test").unwrap();
    assert_eq!(layer.name, "test");
    assert_eq!(layer.handle, handle);
}

#[test]
fn layer_rename_allows_unicode_case_only_change() {
    let mut app = app_with_editing_layer();
    let i = app.active_tab;
    let old_name = app.tabs[i].layers.edit_buf.clone();
    rename_layer(&mut app, &old_name, "é");

    rename_layer(&mut app, "é", "É");

    assert_eq!(
        app.tabs[i].scene.document.layers.get("É").unwrap().name,
        "É"
    );
}

#[test]
fn current_layer_rename_updates_name_and_allocated_handle() {
    let mut app = app_with_editing_layer();
    let i = app.active_tab;
    let old_name = app.tabs[i]
        .layers
        .editing
        .clone()
        .expect("new layer is being edited");
    let idx = app.tabs[i]
        .layers
        .layers
        .iter()
        .position(|l| l.name == old_name)
        .expect("editing layer in panel");
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut(&old_name)
        .unwrap()
        .handle = Handle::NULL;
    app.tabs[i].layers.selected = Some(idx);
    let _ = app.on_layer_set_current();

    rename_layer(&mut app, &old_name, "Renamed");

    let layer = app.tabs[i].scene.document.layers.get("Renamed").unwrap();
    assert!(layer.handle.is_valid());
    assert_eq!(app.tabs[i].active_layer, "Renamed");
    assert_eq!(app.tabs[i].layers.current_layer, "Renamed");
    assert_eq!(app.ribbon.active_layer, "Renamed");
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_name,
        "Renamed"
    );
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_handle,
        layer.handle
    );
}

#[test]
fn layer_rename_undo_redo_restores_active_layer() {
    let mut app = app_with_editing_layer();
    let i = app.active_tab;
    let old_name = app.tabs[i]
        .layers
        .editing
        .clone()
        .expect("new layer is being edited");
    let idx = app.tabs[i]
        .layers
        .layers
        .iter()
        .position(|l| l.name == old_name)
        .expect("editing layer in panel");
    app.tabs[i].layers.selected = Some(idx);
    let _ = app.on_layer_set_current();

    rename_layer(&mut app, &old_name, "Renamed");
    app.undo_active_tab();

    assert!(app.tabs[i].scene.document.layers.contains(&old_name));
    assert_eq!(app.tabs[i].active_layer, old_name);
    assert_eq!(app.ribbon.active_layer, app.tabs[i].active_layer);

    app.redo_active_tab();

    assert!(app.tabs[i].scene.document.layers.contains("Renamed"));
    assert_eq!(app.tabs[i].active_layer, "Renamed");
    assert_eq!(app.ribbon.active_layer, "Renamed");
}

#[test]
fn layer_rename_keeps_independent_current_layer_mirrors() {
    let mut app = app_with_editing_layer();
    let i = app.active_tab;
    let old_name = app.tabs[i].layers.edit_buf.clone();
    rename_layer(&mut app, &old_name, "A");
    let _ = app.on_layer_new();
    let b_name = app.tabs[i]
        .layers
        .editing
        .clone()
        .expect("new layer is being edited");
    let b_idx = app.tabs[i]
        .layers
        .layers
        .iter()
        .position(|l| l.name == b_name)
        .expect("editing layer in panel");
    app.tabs[i].layers.selected = Some(b_idx);
    let _ = app.on_layer_set_current();
    let header_name = app.tabs[i].scene.document.header.current_layer_name.clone();
    let header_handle = app.tabs[i].scene.document.header.current_layer_handle;
    app.tabs[i].active_layer = "A".to_string();

    rename_layer(&mut app, "A", "Renamed");

    assert_eq!(app.tabs[i].active_layer, "Renamed");
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_name,
        header_name
    );
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_handle,
        header_handle
    );
}

#[test]
fn layer_rename_updates_mixed_case_entity_references() {
    let mut app = app_with_editing_layer();
    let i = app.active_tab;
    let old_name = app.tabs[i].layers.edit_buf.clone();
    rename_layer(&mut app, &old_name, "TEST");
    let mut line = Line::new();
    line.common.layer = "test".to_string();
    let handle = app.tabs[i].scene.add_entity(EntityType::Line(line));
    app.tabs[i]
        .scene
        .invalidate_layer_dependencies(&["TEST".to_string()]);

    rename_layer(&mut app, "TEST", "Renamed");

    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .layer,
        "Renamed"
    );
    let epoch = app.tabs[i].scene.geometry_epoch;
    app.tabs[i]
        .scene
        .invalidate_layer_dependencies(&["Renamed".to_string()]);
    assert_ne!(app.tabs[i].scene.geometry_epoch, epoch);
}
