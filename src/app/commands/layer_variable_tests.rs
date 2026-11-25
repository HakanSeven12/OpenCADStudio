//! REVCLOUDLAYER, TABLELAYER and VIEWPORTLAYER: the commands they govern
//! put new objects on their layers; other objects keep their own.

use crate::app::OpenCADStudio;
use crate::command::StepInput;
use document::{EntityType, Handle};
use glam::DVec3;
use std::collections::HashSet;

/// A new drawing whose current layer is `Current`, so that a fault cannot
/// hide behind layer "0".
fn new_app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.ensure_layer("Current");
    app.tabs[i].active_layer = "Current".to_string();
    app
}

fn handles(app: &OpenCADStudio) -> HashSet<Handle> {
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .document
        .entities()
        .map(|entity| entity.common().handle)
        .collect()
}

/// The handles of the objects added after `before`, in drawing order.
fn new_handles(app: &OpenCADStudio, before: &HashSet<Handle>) -> Vec<Handle> {
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .document
        .entities()
        .map(|entity| entity.common().handle)
        .filter(|handle| !before.contains(handle))
        .collect()
}

fn entity(app: &OpenCADStudio, handle: Handle) -> EntityType {
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .document
        .get_entity(handle)
        .cloned()
        .expect("the object exists")
}

/// The new tables after `before`.
fn new_tables(app: &OpenCADStudio, before: &HashSet<Handle>) -> Vec<Handle> {
    new_handles(app, before)
        .into_iter()
        .filter(|handle| matches!(entity(app, *handle), EntityType::Table(_)))
        .collect()
}

fn layer_of(app: &OpenCADStudio, handle: Handle) -> String {
    entity(app, handle).as_entity().layer().to_string()
}

fn has_layer(app: &OpenCADStudio, name: &str) -> bool {
    let i = app.active_tab;
    app.tabs[i].scene.document.layers.contains(name)
}

/// A closed 10 x 10 square on `layer`.
fn square_on(app: &mut OpenCADStudio, layer: &str) -> Handle {
    let i = app.active_tab;
    app.tabs[i].scene.ensure_layer(layer);
    let mut polyline = document::entities::LwPolyline::new();
    polyline.vertices = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        .into_iter()
        .map(|(x, y)| document::entities::LwVertex::new(document::types::Vector2::new(x, y)))
        .collect();
    polyline.is_closed = true;
    let mut entity = EntityType::LwPolyline(polyline);
    entity.as_entity_mut().set_layer(layer.to_string());
    app.commit_entity_handle_preserve_layer(entity)
        .expect("the square is added")
}

#[test]
fn the_three_variables_are_set_typed_and_with_setvar() {
    let mut app = new_app();
    let _ = app.run_command_line("REVCLOUDLAYER Clouds");
    let _ = app.run_command_line("SETVAR TABLELAYER Tables");
    let _ = app.run_command_line("VIEWPORTLAYER Viewports");

    let i = app.active_tab;
    let doc = &app.tabs[i].scene.document;
    for (name, value) in [
        ("REVCLOUDLAYER", "Clouds"),
        ("TABLELAYER", "Tables"),
        ("VIEWPORTLAYER", "Viewports"),
    ] {
        assert_eq!(
            crate::io::drawing_variable(doc, name).as_deref(),
            Some(value),
            "{name}"
        );
        assert!(crate::modules::registry::is_system_variable(name), "{name}");
    }
}

#[test]
fn a_new_revision_cloud_goes_on_revcloudlayer() {
    let mut app = new_app();
    let _ = app.run_command_line("REVCLOUDLAYER Clouds");
    let before = handles(&app);

    let _ = app.run_command_line("REVCLOUD R 0,0 10,10");

    let added = new_handles(&app, &before);
    assert_eq!(added.len(), 1, "one cloud");
    assert!(matches!(entity(&app, added[0]), EntityType::LwPolyline(_)));
    assert_eq!(layer_of(&app, added[0]), "Clouds");
    assert!(has_layer(&app, "Clouds"), "the missing layer is made");
}

#[test]
fn revcloud_object_keeps_the_layer_of_the_object() {
    let mut app = new_app();
    let square = square_on(&mut app, "Walls");
    let _ = app.run_command_line("REVCLOUDLAYER Clouds");
    let before = entity(&app, square);

    let _ = app.run_command_line("REVCLOUD O");
    let _ = app.feed_active_cmd(&format!("{:X}", square.value()));
    let _ = app.feed_active_cmd("N");

    let i = app.active_tab;
    assert!(app.tabs[i].active_cmd.is_none(), "REVCLOUD is done");
    assert_ne!(entity(&app, square), before, "the square became a cloud");
    assert_eq!(layer_of(&app, square), "Walls");
}

#[test]
fn a_new_table_goes_on_tablelayer() {
    let mut app = new_app();
    let _ = app.run_command_line("TABLELAYER Tables");
    let before = handles(&app);

    let _ = app.run_command_line("-TABLE");
    let _ = app.feed_command(StepInput::Text("3".into()));
    let _ = app.feed_command(StepInput::Text("2".into()));
    let _ = app.feed_command(StepInput::Point(DVec3::new(5.0, 7.0, 0.0)));

    let added = new_tables(&app, &before);
    assert_eq!(added.len(), 1, "one table");
    assert_eq!(layer_of(&app, added[0]), "Tables");
}

#[test]
fn a_table_from_a_data_link_in_the_table_dialog_goes_on_tablelayer() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("parts.csv");
    std::fs::write(&path, "NAME,QTY\nBolt,12\n").unwrap();
    let escaped = path.to_string_lossy().replace('\\', "/");
    let mut app = new_app();
    let _ = app.run_command_line("TABLELAYER Tables");

    // DATALINK places a linked table of its own; it is not TABLE.
    let before = handles(&app);
    let task = app.run_command_line(&format!("DATALINK {escaped}"));
    app.drive_headless_task(task).expect("the read task");
    let _ = app.feed_command(StepInput::Point(DVec3::new(10.0, 10.0, 0.0)));
    let first = new_tables(&app, &before);
    assert_eq!(first.len(), 1, "DATALINK placed one table");
    assert_eq!(layer_of(&app, first[0]), "Current");

    let before = handles(&app);
    let _ = app.run_command_line("TABLE");
    app.table_insert.source = crate::ui::window::annotation_data::TableSource::DataLink;
    app.table_insert.link = app.table_insert.links.first().cloned();
    assert!(app.table_insert.link.is_some(), "the dialog lists the link");
    let _ = app.on_table_insert_apply();
    let i = app.active_tab;
    let prompt = app.tabs[i]
        .active_cmd
        .as_ref()
        .expect("the table waits for its point")
        .prompt();
    assert!(
        prompt.contains("TABLE") && !prompt.contains("DATAEXTRACTION"),
        "{prompt}"
    );
    let _ = app.feed_command(StepInput::Point(DVec3::new(40.0, 10.0, 0.0)));

    let added = new_tables(&app, &before);
    assert_eq!(added.len(), 1, "TABLE placed one table");
    assert_eq!(layer_of(&app, added[0]), "Tables");
}

#[test]
fn other_commands_keep_the_current_layer() {
    let mut app = new_app();
    let _ = app.run_command_line("REVCLOUDLAYER Clouds");
    let _ = app.run_command_line("TABLELAYER Tables");
    let _ = app.run_command_line("VIEWPORTLAYER Viewports");
    let before = handles(&app);

    let _ = app.run_command_line("LINE 0,0 5,5");
    let _ = app.feed_active_cmd("");

    let added = new_handles(&app, &before);
    assert_eq!(added.len(), 1, "one line");
    assert_eq!(layer_of(&app, added[0]), "Current");
}

fn enter_layout(app: &mut OpenCADStudio) {
    let i = app.active_tab;
    let name = app.tabs[i]
        .scene
        .layout_names()
        .into_iter()
        .find(|name| name != "Model")
        .expect("a paper layout");
    app.tabs[i].scene.set_current_layout(name);
    app.tabs[i].scene.load_current_layout_state();
}

/// The user viewports of the current layout.
fn layout_viewports(app: &OpenCADStudio) -> Vec<document::entities::Viewport> {
    let i = app.active_tab;
    let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
    app.tabs[i]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Viewport(viewport)
                if viewport.id > 1 && viewport.common.owner_handle == layout_block =>
            {
                Some((**viewport).clone())
            }
            _ => None,
        })
        .collect()
}

fn viewport_layers(app: &OpenCADStudio) -> Vec<String> {
    layout_viewports(app)
        .iter()
        .map(|viewport| viewport.common.layer.clone())
        .collect()
}

#[test]
fn a_new_viewport_goes_on_viewportlayer() {
    let mut app = new_app();
    enter_layout(&mut app);
    let _ = app.run_command_line("VIEWPORTLAYER Viewports");

    let _ = app.run_command_line("MVIEW 1,1 5,5");

    assert_eq!(viewport_layers(&app), ["Viewports"]);
}

#[test]
fn vports_presets_go_on_viewportlayer() {
    let mut app = new_app();
    enter_layout(&mut app);
    let _ = app.run_command_line("VIEWPORTLAYER Viewports");

    let _ = app.run_command_line("VPORTS 2V");
    let _ = app.feed_active_cmd("F");

    assert_eq!(viewport_layers(&app), ["Viewports", "Viewports"]);
}

#[test]
fn mview_polygonal_puts_the_viewport_and_its_boundary_on_viewportlayer() {
    let mut app = new_app();
    enter_layout(&mut app);
    let _ = app.run_command_line("VIEWPORTLAYER Viewports");

    let _ = app.run_command_line("MVIEW P 1,1 6,1 6,6 C");

    let viewports = layout_viewports(&app);
    assert_eq!(viewports.len(), 1, "one viewport");
    assert_eq!(viewports[0].common.layer, "Viewports");
    let boundary = viewports[0].clip_boundary_handle;
    assert!(!boundary.is_null(), "a polygonal viewport has a boundary");
    assert_eq!(layer_of(&app, boundary), "Viewports");
}

#[test]
fn mview_object_keeps_the_current_layer_for_the_viewport() {
    let mut app = new_app();
    enter_layout(&mut app);
    let i = app.active_tab;
    app.tabs[i].scene.ensure_layer("Frames");
    let layout = app.tabs[i].scene.current_layout.clone();
    let mut circle = document::Circle::new();
    circle.center = document::types::Vector3::new(5.0, 5.0, 0.0);
    circle.radius = 2.0;
    let mut circle = EntityType::Circle(circle);
    circle.as_entity_mut().set_layer("Frames".to_string());
    let circle = app.tabs[i]
        .scene
        .document
        .add_entity_to_layout(circle, &layout)
        .expect("the circle is added");
    let _ = app.run_command_line("VIEWPORTLAYER Viewports");

    let _ = app.run_command_line("MVIEW");
    let _ = app.feed_active_cmd("O");
    let _ = app.feed_active_cmd(&format!("{:X}", circle.value()));

    let viewports = layout_viewports(&app);
    assert_eq!(
        viewports.len(),
        1,
        "one viewport: {}",
        app.command_line.history_plain_text()
    );
    assert_eq!(viewports[0].clip_boundary_handle, circle);
    assert_eq!(viewports[0].common.layer, "Current");
    assert_eq!(layer_of(&app, circle), "Frames");
}

#[test]
fn vports_restore_puts_the_viewports_on_viewportlayer() {
    let mut app = new_app();
    enter_layout(&mut app);
    let _ = app.run_command_line("VPORTS 2V");
    let _ = app.feed_active_cmd("F");
    let _ = app.run_command_line("VPORTS SAVE TWO");
    let _ = app.run_command_line("VIEWPORTLAYER Viewports");

    let _ = app.run_command_line("VPORTS RESTORE TWO");

    assert_eq!(viewport_layers(&app), ["Viewports", "Viewports"]);
}

#[test]
fn vports_restore_with_dot_uses_the_current_layer() {
    let mut app = new_app();
    enter_layout(&mut app);
    let _ = app.run_command_line("VPORTS 2V");
    let _ = app.feed_active_cmd("F");
    let _ = app.run_command_line("VPORTS SAVE TWO");
    let _ = app.run_command_line("VIEWPORTLAYER .");

    let _ = app.run_command_line("VPORTS RESTORE TWO");

    assert_eq!(viewport_layers(&app), ["Current", "Current"]);
}
