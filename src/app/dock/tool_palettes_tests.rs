//! Synthetic test suite for the Tool Palettes workflow (`TOOLPALETTES` / `TP`, `TOOLPALETTESCLOSE`).
//!
//! Tests command routing, panel lifecycle, tool creation, tab management,
//! cross-drawing insertion, dynamic block preservation, undo/redo, and persistence.

use super::tests::fresh;
use crate::app::Message;
use crate::command::CadCommand;
use crate::ui::dock::PanelId;
use crate::ui::window::tool_palettes::{
    BlockTool, ToolPaletteTab, ToolPalettesData, ToolPalettesMsg,
};
use document::entities::{Insert, Line};
use document::types::{Color, Vector3};
use document::EntityType;

fn define_block(app: &mut crate::app::OpenCADStudio, name: &str) {
    let i = app.active_tab;
    let line = Line::from_points(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
    app.tabs[i]
        .scene
        .define_block_raw(name, Vector3::ZERO, vec![EntityType::Line(line)]);
}

fn fixture_dynamic_block(app: &mut crate::app::OpenCADStudio) -> String {
    let name = "DYN_VALVE";
    define_block(app, name);
    let i = app.active_tab;
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(name, Vector3::ZERO)))
        .unwrap();
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,2 3,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let doc = &mut app.tabs[i].scene.document;
    let p = crate::scene::block_actions::all_parameters(doc, definition)[0].handle;
    let target = doc.block_records.get(name).unwrap().entity_handles[0];
    crate::scene::block_actions::create(doc, definition, crate::scene::block_actions::Kind::Move, p, &[target], glam::DVec3::ZERO).unwrap();
    let _ = app.dispatch_command("BEDIT_SAVE");
    let _ = app.dispatch_command("BCLOSE");
    name.to_string()
}

#[test]
fn toolpalettes_command_routing_transparency_and_distinct_panels() {
    let mut app = fresh();
    let i = app.active_tab;

    // Start a command (LINE) to test command transparency
    let _ = app.dispatch_command("LINE");
    assert_eq!(
        app.tabs[i].active_cmd.as_deref().map(CadCommand::name),
        Some("LINE")
    );

    // TOOLPALETTES opens the palette and is transparent
    let _ = app.dispatch_command("TOOLPALETTES");
    assert_eq!(
        app.tabs[i].active_cmd.as_deref().map(CadCommand::name),
        Some("LINE"),
        "TOOLPALETTES must preserve active command"
    );
    assert!(
        app.dock_panel_visible(PanelId::ToolPalettes),
        "TOOLPALETTES must open Tool Palettes panel"
    );

    // TOOLPALETTESCLOSE closes the palette and is transparent
    let _ = app.dispatch_command("TOOLPALETTESCLOSE");
    assert_eq!(
        app.tabs[i].active_cmd.as_deref().map(CadCommand::name),
        Some("LINE"),
        "TOOLPALETTESCLOSE must preserve active command"
    );
    assert!(
        !app.dock_panel_visible(PanelId::ToolPalettes),
        "TOOLPALETTESCLOSE must close Tool Palettes panel"
    );

    // Alias TP opens the palette and is transparent
    let _ = app.dispatch_command("TP");
    assert_eq!(
        app.tabs[i].active_cmd.as_deref().map(CadCommand::name),
        Some("LINE"),
        "TP must preserve active command"
    );
    assert!(
        app.dock_panel_visible(PanelId::ToolPalettes),
        "TP alias must open Tool Palettes panel"
    );

    // Cancel LINE command
    let _ = app.update(Message::CommandEscape);
    assert!(app.tabs[i].active_cmd.is_none());

    // Verify distinct from AuthoringPalettes and BlockPalette in Block Editor
    let mut authoring = super::tests::authoring_app();
    assert!(!authoring.dock_panel_visible(PanelId::AuthoringPalettes));
    let _ = authoring.dispatch_command("BAUTHORPALETTE");
    assert!(
        authoring.dock_panel_visible(PanelId::AuthoringPalettes),
        "Authoring Palettes must open in block editor"
    );
    let _ = authoring.dispatch_command("TOOLPALETTES");
    assert!(
        authoring.dock_panel_visible(PanelId::ToolPalettes),
        "Tool Palettes opens alongside Authoring Palettes"
    );
    assert!(
        authoring.dock_panel_visible(PanelId::AuthoringPalettes),
        "Authoring Palettes remains open"
    );

    let _ = authoring.dispatch_command("TOOLPALETTESCLOSE");
    assert!(!authoring.dock_panel_visible(PanelId::ToolPalettes));
    assert!(
        authoring.dock_panel_visible(PanelId::AuthoringPalettes),
        "Authoring Palettes remains open after TOOLPALETTESCLOSE"
    );

    let _ = authoring.dispatch_command("BAUTHORPALETTECLOSE");
    assert!(!authoring.dock_panel_visible(PanelId::AuthoringPalettes));
}

#[test]
fn toolpalettes_panel_lifecycle_and_cleanscreen() {
    let mut app = fresh();

    // Directly open through set_tool_palettes_open
    let _ = app.set_tool_palettes_open(true);
    assert!(app.dock_panel_visible(PanelId::ToolPalettes));
    assert!(app.show_tool_palettes);

    // Toggle collapse/pin
    let _ = app.on_dock(crate::ui::dock::DockMsg::AutoCollapseToggle(
        PanelId::ToolPalettes,
    ));
    assert!(app.dock.auto_hides(PanelId::ToolPalettes));

    let _ = app.on_dock(crate::ui::dock::DockMsg::AutoCollapseToggle(
        PanelId::ToolPalettes,
    ));
    assert!(!app.dock.auto_hides(PanelId::ToolPalettes));

    // Close through DockMsg::Close
    let _ = app.on_dock(crate::ui::dock::DockMsg::Close(PanelId::ToolPalettes));
    assert!(!app.dock_panel_visible(PanelId::ToolPalettes));
    assert!(!app.show_tool_palettes);

    // Reopen and test Clean Screen toggle
    let _ = app.set_tool_palettes_open(true);
    assert!(app.dock_panel_visible(PanelId::ToolPalettes));

    let _ = app.update(Message::ToggleCleanScreen);
    assert!(
        !app.dock_panel_visible(PanelId::ToolPalettes),
        "Clean screen on must hide Tool Palettes"
    );

    let _ = app.update(Message::ToggleCleanScreen);
    assert!(
        app.dock_panel_visible(PanelId::ToolPalettes),
        "Clean screen off must restore Tool Palettes"
    );
}

#[test]
fn tool_creation_from_drawing_block_and_tab_operations() {
    let mut app = fresh();
    define_block(&mut app, "CHAIR");

    // Add block definition to active tool palette tab
    let added = app.add_block_to_tool_palette("CHAIR");
    assert!(added, "block CHAIR must be added to tool palette");

    let active_tab = app.tool_palettes.active_tab();
    assert_eq!(active_tab.tools.len(), 1);
    assert_eq!(active_tab.tools[0].name, "CHAIR");
    assert_eq!(active_tab.tools[0].scale, 1.0);
    assert_eq!(active_tab.tools[0].rotation_deg, 0.0);
    assert!(!active_tab.tools[0].dynamic);

    // Create a new palette tab
    let _ = app.on_tool_palettes(ToolPalettesMsg::NewPaletteNameChanged(
        "Furniture".to_string(),
    ));
    let _ = app.on_tool_palettes(ToolPalettesMsg::NewPalette);
    assert_eq!(app.tool_palettes.palettes.len(), 2);
    assert_eq!(app.tool_palettes.palettes[1].name, "Furniture");

    // Select the new tab
    let _ = app.on_tool_palettes(ToolPalettesMsg::SelectPalette(1));
    assert_eq!(app.tool_palettes.active_palette, 1);

    // Add another block
    define_block(&mut app, "DESK");
    let added_desk = app.add_block_to_tool_palette("DESK");
    assert!(added_desk);
    define_block(&mut app, "LAMP");
    let added_lamp = app.add_block_to_tool_palette("LAMP");
    assert!(added_lamp);

    assert_eq!(app.tool_palettes.active_tab().tools.len(), 2);
    assert_eq!(app.tool_palettes.active_tab().tools[0].name, "DESK");
    assert_eq!(app.tool_palettes.active_tab().tools[1].name, "LAMP");

    // Move tool down
    let _ = app.on_tool_palettes(ToolPalettesMsg::MoveToolDown(0));
    assert_eq!(app.tool_palettes.active_tab().tools[0].name, "LAMP");
    assert_eq!(app.tool_palettes.active_tab().tools[1].name, "DESK");

    // Move tool up
    let _ = app.on_tool_palettes(ToolPalettesMsg::MoveToolUp(1));
    assert_eq!(app.tool_palettes.active_tab().tools[0].name, "DESK");
    assert_eq!(app.tool_palettes.active_tab().tools[1].name, "LAMP");

    // Remove tool
    let _ = app.on_tool_palettes(ToolPalettesMsg::RemoveTool(0));
    assert_eq!(app.tool_palettes.active_tab().tools.len(), 1);
    assert_eq!(app.tool_palettes.active_tab().tools[0].name, "LAMP");

    // Delete palette tab
    let _ = app.on_tool_palettes(ToolPalettesMsg::DeletePalette(1));
    assert_eq!(app.tool_palettes.palettes.len(), 1);
    assert_eq!(app.tool_palettes.active_palette, 0);
}

#[test]
fn tool_creation_from_selected_insert_preserves_transform() {
    let mut app = fresh();
    define_block(&mut app, "WINDOW");

    let i = app.active_tab;
    let mut ins = Insert::new("WINDOW".to_string(), Vector3::new(5.0, 5.0, 0.0));
    ins.set_x_scale(2.5);
    ins.set_y_scale(2.5);
    ins.set_z_scale(2.5);
    ins.rotation = 45.0_f64.to_radians();

    let ins_handle = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(ins))
        .expect("add_entity");
    app.tabs[i].scene.select_entity(ins_handle, true);

    let count = app.add_selected_blocks_to_tool_palette();
    assert_eq!(count, 1, "must capture 1 selected block tool");

    let active = app.tool_palettes.active_tab();
    let tool = active
        .tools
        .iter()
        .find(|t| t.name == "WINDOW")
        .expect("WINDOW tool created");
    assert_eq!(tool.name, "WINDOW");
    assert!((tool.scale - 2.5).abs() < 1e-6);
    assert!((tool.rotation_deg - 45.0).abs() < 1e-4);
}

#[test]
fn cross_drawing_insertion_imports_block_and_dependencies() {
    let mut app = fresh();

    // Tab 0: Create block with a custom layer
    let i = app.active_tab;
    let mut custom_layer = document::tables::Layer::new("EQUIPMENT_LAYER".to_string());
    custom_layer.color = Color::from_index(3);
    let _ = app.tabs[i].scene.document.layers.add(custom_layer);

    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(20.0, 0.0, 0.0));
    line.common.layer = "EQUIPMENT_LAYER".to_string();
    app.tabs[i]
        .scene
        .define_block_raw("EQUIPMENT", Vector3::ZERO, vec![EntityType::Line(line)]);

    // Capture into tool palette
    let added = app.add_block_to_tool_palette("EQUIPMENT");
    assert!(added);

    // Open a second drawing tab (Tab 1)
    let _ = app.update(Message::TabNew);
    let new_tab = app.active_tab;
    assert_ne!(new_tab, i, "must be a new tab");
    assert!(
        app.tabs[new_tab]
            .scene
            .document
            .block_records
            .get("EQUIPMENT")
            .is_none(),
        "EQUIPMENT must not exist in fresh tab initially"
    );
    assert!(
        app.tabs[new_tab]
            .scene
            .document
            .layers
            .get("EQUIPMENT_LAYER")
            .is_none(),
        "EQUIPMENT_LAYER must not exist in fresh tab initially"
    );

    // Insert from tool palette into new tab at (15, 25, 0)
    let drop_point = glam::DVec3::new(15.0, 25.0, 0.0);
    let _ = app.insert_from_tool_palette(0, Some(drop_point));

    // Verify block and layer were imported into Tab 1
    assert!(
        app.tabs[new_tab]
            .scene
            .document
            .block_records
            .get("EQUIPMENT")
            .is_some(),
        "EQUIPMENT block definition must be imported"
    );
    assert!(
        app.tabs[new_tab]
            .scene
            .document
            .layers
            .get("EQUIPMENT_LAYER")
            .is_some(),
        "EQUIPMENT_LAYER table record must be imported"
    );

    // Verify an Insert entity was created at the drop point
    let inserted = app.tabs[new_tab]
        .scene
        .document
        .entities()
        .find(|e| matches!(e, EntityType::Insert(ins) if ins.block_name == "EQUIPMENT"));
    assert!(inserted.is_some(), "INSERT entity must exist in new drawing");
    if let Some(EntityType::Insert(ins)) = inserted {
        assert_eq!(ins.insert_point.x, 15.0);
        assert_eq!(ins.insert_point.y, 25.0);
    }
}

#[test]
fn dynamic_block_preservation_across_drawings() {
    let mut app = fresh();
    let tab0 = app.active_tab;

    // Define dynamic block in Tab 0
    let dyn_name = fixture_dynamic_block(&mut app);
    let record_handle = app.tabs[tab0]
        .scene
        .document
        .block_records
        .get(&dyn_name)
        .map(|br| br.handle)
        .expect("DYN_VALVE exists");
    assert!(
        app.tabs[tab0].scene.document.is_dynamic_block(record_handle),
        "DYN_VALVE must be recognized as dynamic in Tab 0"
    );

    // Add to tool palette
    let added = app.add_block_to_tool_palette(&dyn_name);
    assert!(added);
    let tool = app
        .tool_palettes
        .active_tab()
        .tools
        .iter()
        .find(|t| t.name == dyn_name)
        .expect("tool created");
    assert!(tool.dynamic, "captured tool must be flagged as dynamic");

    // Open Tab 1 and insert tool
    let _ = app.update(Message::TabNew);
    let tab1 = app.active_tab;
    assert_ne!(tab1, tab0);

    let _ = app.insert_from_tool_palette(0, Some(glam::DVec3::new(0.0, 0.0, 0.0)));

    // Verify imported definition in Tab 1 preserves dynamic status
    let imported_record = app.tabs[tab1]
        .scene
        .document
        .block_records
        .get(&dyn_name)
        .expect("DYN_VALVE definition imported in Tab 1");
    assert!(
        app.tabs[tab1]
            .scene
            .document
            .is_dynamic_block(imported_record.handle),
        "Imported block definition must retain dynamic block status in new drawing"
    );
}

#[test]
fn undo_redo_tool_palette_insertion() {
    let mut app = fresh();
    define_block(&mut app, "STOOL");
    assert!(app.add_block_to_tool_palette("STOOL"));

    let i = app.active_tab;
    let count_before = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|e| matches!(e, EntityType::Insert(ins) if ins.block_name == "STOOL"))
        .count();
    assert_eq!(count_before, 0);

    // Insert tool
    let _ = app.insert_from_tool_palette(0, Some(glam::DVec3::new(5.0, 5.0, 0.0)));

    let count_after_insert = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|e| matches!(e, EntityType::Insert(ins) if ins.block_name == "STOOL"))
        .count();
    assert_eq!(count_after_insert, 1, "inserted entity exists");

    // UNDO
    let _ = app.update(Message::Undo);
    let count_after_undo = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|e| matches!(e, EntityType::Insert(ins) if ins.block_name == "STOOL"))
        .count();
    assert_eq!(count_after_undo, 0, "undo removes inserted entity");

    // REDO
    let _ = app.update(Message::Redo);
    let count_after_redo = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|e| matches!(e, EntityType::Insert(ins) if ins.block_name == "STOOL"))
        .count();
    assert_eq!(count_after_redo, 1, "redo restores inserted entity");
}

#[test]
fn tool_palettes_persistence_roundtrip() {
    let mut data = ToolPalettesData::default();
    let custom_tab = ToolPaletteTab {
        name: "Mechanical".to_string(),
        tools: vec![
            BlockTool {
                name: "PUMP".to_string(),
                label: "Pump Standard".to_string(),
                description: "Centrifugal pump".to_string(),
                source: "catalog.dwg".to_string(),
                dynamic: true,
                annotative: false,
                scale: 1.5,
                scale_xyz: None,
                rotation_deg: 90.0,
                explode: false,
                captured: None,
                preview: std::rc::Rc::new(crate::ui::window::block_palette::BlockPreview::default()),
            },
            BlockTool {
                name: "FLANGE".to_string(),
                label: "Flange DN50".to_string(),
                description: "Weld neck flange".to_string(),
                source: "catalog.dwg".to_string(),
                dynamic: false,
                annotative: true,
                scale: 2.0,
                scale_xyz: None,
                rotation_deg: 0.0,
                explode: true,
                captured: None,
                preview: std::rc::Rc::new(crate::ui::window::block_palette::BlockPreview::default()),
            },
        ],
    };
    data.palettes.push(custom_tab);
    data.active_palette = 1;
    data.options.explode = true;
    data.options.uniform = true;

    // Serialize to JSON and deserialize back
    let serialized = serde_json::to_string(&data).expect("serialize ToolPalettesData");
    let deserialized: ToolPalettesData =
        serde_json::from_str(&serialized).expect("deserialize ToolPalettesData");

    assert_eq!(deserialized.palettes.len(), 2);
    assert_eq!(deserialized.active_palette, 1);
    assert_eq!(deserialized.palettes[1].name, "Mechanical");
    assert_eq!(deserialized.palettes[1].tools.len(), 2);

    let pump = &deserialized.palettes[1].tools[0];
    assert_eq!(pump.name, "PUMP");
    assert_eq!(pump.label, "Pump Standard");
    assert!(pump.dynamic);
    assert_eq!(pump.scale, 1.5);
    assert_eq!(pump.rotation_deg, 90.0);
    assert!(!pump.explode);

    let flange = &deserialized.palettes[1].tools[1];
    assert_eq!(flange.name, "FLANGE");
    assert!(flange.annotative);
    assert_eq!(flange.scale, 2.0);
    assert!(flange.explode);

    // Apply to ToolPalettesState
    let mut state = crate::ui::window::tool_palettes::ToolPalettesState::default();
    state.apply_persisted(deserialized);
    assert_eq!(state.active_palette, 1);
    assert_eq!(state.active_tab().tools.len(), 2);
}

#[test]
fn selected_tool_preserves_mirrored_axis_scales_on_insertion() {
    let mut app = fresh();
    define_block(&mut app, "MIRRORED_TOOL");
    let i = app.active_tab;
    let mut source = Insert::new("MIRRORED_TOOL", Vector3::ZERO);
    source.set_x_scale(-2.);
    source.set_y_scale(3.);
    source.set_z_scale(4.);
    source.rotation = 0.5;
    let handle = app.tabs[i].scene.document.add_entity(EntityType::Insert(source)).unwrap();
    app.tabs[i].scene.select_entity(handle, true);
    assert_eq!(app.add_selected_blocks_to_tool_palette(), 1);
    let _ = app.update(Message::TabNew);
    let j = app.active_tab;
    let _ = app.insert_from_tool_palette(0, Some(glam::DVec3::new(20., 30., 0.)));
    let inserted = app.tabs[j].scene.document.entities().find_map(|e| if let EntityType::Insert(i) = e { Some(i) } else { None }).unwrap();
    assert_eq!([inserted.x_scale(), inserted.y_scale(), inserted.z_scale()], [-2., 3., 4.]);
    assert!((inserted.rotation - 0.5).abs() < 1e-9);
}

#[test]
fn dynamic_tool_redefinition_is_refused_without_mutation() {
    let mut app = fresh();
    let name = fixture_dynamic_block(&mut app);
    assert!(app.add_block_to_tool_palette(&name));
    app.block_redefine_mode = 2;
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.insert_from_tool_palette(0, Some(glam::DVec3::ZERO));
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert!(app.command_line.last_error.is_some());
}

#[test]
fn recursive_tool_import_is_refused_before_creating_dependencies() {
    let mut app = fresh();
    define_block(&mut app, "CONTAINER");
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw("OUTER_TOOL", Vector3::ZERO,
        vec![EntityType::Insert(Insert::new("CONTAINER", Vector3::ZERO))]);
    assert!(app.add_block_to_tool_palette("OUTER_TOOL"));
    let _ = app.update(Message::TabNew);
    let j = app.active_tab;
    define_block(&mut app, "CONTAINER");
    app.tabs[j].scene.block_edit_block = Some(app.tabs[j].scene.document.block_records.get("CONTAINER").unwrap().handle);
    let before = app.tabs[j].scene.document.clone();
    let _ = app.insert_from_tool_palette(0, Some(glam::DVec3::ZERO));
    assert_eq!(app.tabs[j].scene.document, before);
    assert!(app.command_line.last_error.is_some());
}

#[test]
fn imported_dynamic_tool_evaluates_its_remapped_action_targets() {
    use crate::scene::block_actions as actions;
    let mut app = fresh();
    let name = fixture_dynamic_block(&mut app);
    assert!(app.add_block_to_tool_palette(&name));
    let _ = app.update(Message::TabNew);
    let j = app.active_tab;
    let _ = app.insert_from_tool_palette(0, Some(glam::DVec3::ZERO));
    let doc = &app.tabs[j].scene.document;
    let definition = doc.block_records.get(&name).unwrap().handle;
    let insert = doc.entities().find_map(|e| match e { EntityType::Insert(i) if i.block_name == name => Some(i.common.handle), _ => None }).unwrap();
    let parameter = &actions::all_parameters(doc, definition)[0];
    let evaluated = actions::instance::evaluate(doc, insert, parameter.handle, parameter.end * 2.).unwrap().unwrap();
    let EntityType::Insert(reference) = evaluated.get_entity(insert).unwrap() else { panic!() };
    let line = evaluated.entities_in_block(&reference.block_name).find_map(|e| if let EntityType::Line(l) = e { Some(l) } else { None }).unwrap();
    assert!(line.start.distance(&Vector3::new(6., 2., 0.)) < 1e-8);
    assert!(line.end.distance(&Vector3::new(16., 2., 0.)) < 1e-8);
}

#[test]
fn removing_an_earlier_palette_keeps_the_active_palette_selected() {
    let mut app = fresh();
    for name in ["First", "Second", "Third"] {
        let _ = app.on_tool_palettes(ToolPalettesMsg::NewPaletteNameChanged(name.into()));
        let _ = app.on_tool_palettes(ToolPalettesMsg::NewPalette);
    }
    assert_eq!(app.tool_palettes.active_tab().name, "Third");
    let _ = app.on_tool_palettes(ToolPalettesMsg::DeletePalette(0));
    assert_eq!(app.tool_palettes.active_tab().name, "Third");
}

#[test]
fn cancelled_tool_placement_clears_the_palette_highlight() {
    let mut app = fresh();
    define_block(&mut app, "CANCEL_TOOL");
    assert!(app.add_block_to_tool_palette("CANCEL_TOOL"));
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.insert_from_tool_palette(0, None);
    assert!(app.tabs[i].active_cmd.is_some());
    assert!(app.tool_palettes.placing.is_some());
    let _ = app.update(Message::CommandEscape);
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(app.tool_palettes.placing.is_none());
    assert_eq!(app.tabs[i].scene.document, before);
}
