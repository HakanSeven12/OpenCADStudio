//! Drawing names typed while no command runs: each kind does what its
//! command does with the name.

use crate::app::{Message, OpenCADStudio};
use crate::command::StepInput;
use crate::ui::command_line::ContentKind;
use glam::DVec3;

fn fresh_app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app
}

/// Type `text` at the command line and press Enter.
fn type_and_enter(app: &mut OpenCADStudio, text: &str) {
    let _ = app.update(Message::CommandInput(text.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

fn add_layer(app: &mut OpenCADStudio, name: &str) {
    let i = app.active_tab;
    let mut layer = document::tables::Layer::new(name);
    layer.handle = app.tabs[i].scene.document.allocate_handle();
    let _ = app.tabs[i].scene.document.layers.add(layer);
}

fn define_block(app: &mut OpenCADStudio, name: &str) {
    let i = app.active_tab;
    let mut line = document::entities::Line::new();
    line.end = document::types::Vector3::new(10.0, 0.0, 0.0);
    app.tabs[i]
        .scene
        .define_block_from_owned_entities(vec![document::EntityType::Line(line)], name, DVec3::ZERO)
        .unwrap();
}

fn active_command(app: &OpenCADStudio) -> Option<&'static str> {
    app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .map(|command| command.name())
}

#[test]
fn a_typed_layer_name_shows_under_the_commands_and_enter_makes_it_current() {
    let mut app = fresh_app();
    add_layer(&mut app, "Kitchen walls");

    let _ = app.update(Message::CommandInput("kitchen".to_string()));
    let rows = app.command_line.content_rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, ContentKind::Layer);
    assert_eq!(rows[0].name, "Kitchen walls");

    let _ = app.update(Message::CommandSubmit);
    let i = app.active_tab;
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_name,
        "Kitchen walls"
    );
    assert!(active_command(&app).is_none());
}

#[test]
fn a_typed_block_name_inserts_that_block() {
    let mut app = fresh_app();
    define_block(&mut app, "Sink unit");

    type_and_enter(&mut app, "Sink");
    assert_eq!(active_command(&app), Some("-INSERT"));
    let i = app.active_tab;
    let _ = app.feed_command(StepInput::Point(DVec3::new(5.0, 5.0, 0.0)));
    for _ in 0..5 {
        if app.tabs[i].active_cmd.is_none() {
            break;
        }
        let _ = app.feed_command(StepInput::Enter);
    }

    let inserted: Vec<String> = app.tabs[i]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            document::EntityType::Insert(insert) => Some(insert.block_name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(inserted, ["Sink unit"]);
}

#[test]
fn a_typed_hatch_pattern_starts_hatch_with_that_pattern() {
    let mut app = fresh_app();

    type_and_enter(&mut app, "ansi31");
    let i = app.active_tab;
    let command = app.tabs[i].active_cmd.as_ref().expect("HATCH runs");
    assert_eq!(command.name(), "HATCH");
    let settings = command.hatch_settings().expect("HATCH has panel settings");
    assert!(
        matches!(
            &settings.fill,
            crate::modules::draw::draw::hatch::HatchFill::Pattern(name) if name == "ANSI31"
        ),
        "{:?}",
        settings.fill
    );
}

#[test]
fn a_typed_view_name_restores_that_view() {
    let mut app = fresh_app();
    let i = app.active_tab;
    let mut view = document::tables::View::new("Porch north");
    view.center = document::types::Vector3::new(500.0, 300.0, 0.0);
    view.height = 40.0;
    view.width = 60.0;
    app.tabs[i].scene.document.views.add_or_replace(view);
    let before = app.tabs[i].scene.camera.borrow().target;

    type_and_enter(&mut app, "porch");

    let after = app.tabs[i].scene.camera.borrow().target;
    assert!(
        (after.x - 500.0).abs() < 1e-6 && (after.y - 300.0).abs() < 1e-6,
        "the camera moved from {before:?} to {after:?}"
    );
    assert!(active_command(&app).is_none());
}

#[test]
fn a_clicked_block_row_inserts_that_block() {
    let mut app = fresh_app();
    define_block(&mut app, "Sink unit");
    let _ = app.update(Message::CommandInput("sink".to_string()));
    let row = app.command_line.content_rows()[0].clone();

    let _ = app.update(Message::ContentSuggestionPick(row));
    assert_eq!(active_command(&app), Some("-INSERT"));
    assert!(app.command_line.input.is_empty());
}

#[test]
fn a_command_name_still_runs_when_a_layer_has_the_same_name() {
    let mut app = fresh_app();
    add_layer(&mut app, "LINE");

    let _ = app.update(Message::CommandInput("LINE".to_string()));
    assert!(app
        .command_line
        .content_rows()
        .iter()
        .any(|row| row.kind == ContentKind::Layer && row.name == "LINE"));
    let _ = app.update(Message::CommandSubmit);
    assert_eq!(active_command(&app), Some("LINE"));
    let i = app.active_tab;
    assert_eq!(app.tabs[i].scene.document.header.current_layer_name, "0");
}

#[test]
fn no_drawing_names_show_while_a_command_runs() {
    let mut app = fresh_app();
    add_layer(&mut app, "Kitchen walls");
    let _ = app.run_command_line("LINE");

    let _ = app.update(Message::CommandInput("kitchen".to_string()));
    assert!(app.command_line.content_rows().is_empty());
}

#[test]
fn undo_takes_back_a_layer_made_current_from_the_suggestions() {
    let mut app = fresh_app();
    add_layer(&mut app, "Kitchen walls");
    type_and_enter(&mut app, "kitchen");
    let i = app.active_tab;
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_name,
        "Kitchen walls"
    );

    let _ = app.update(Message::Undo);
    assert_eq!(app.tabs[i].scene.document.header.current_layer_name, "0");
}

#[test]
fn a_typed_hatch_pattern_wins_over_the_pattern_of_a_selected_hatch() {
    use crate::modules::draw::draw::hatch::HatchFill;
    let mut app = fresh_app();
    let i = app.active_tab;
    let mut square = document::entities::LwPolyline::from_points(vec![
        document::types::Vector2::new(0.0, 0.0),
        document::types::Vector2::new(10.0, 0.0),
        document::types::Vector2::new(10.0, 10.0),
        document::types::Vector2::new(0.0, 10.0),
    ]);
    square.is_closed = true;
    let _ = app.tabs[i]
        .scene
        .add_entity(document::EntityType::LwPolyline(square));
    let _ = app.dispatch_command("HATCH");
    let command = app.tabs[i].active_cmd.as_mut().expect("HATCH runs");
    let mut settings = command.hatch_settings().expect("HATCH has panel settings");
    settings.fill = HatchFill::Pattern("ANSI37".to_string());
    command.set_hatch_settings(settings);
    let _ = app.feed_command(StepInput::Point(DVec3::new(5.0, 5.0, 0.0)));
    if app.tabs[i].active_cmd.is_some() {
        let _ = app.feed_command(StepInput::Enter);
    }
    let hatch = app.tabs[i]
        .scene
        .document
        .entities()
        .find_map(|entity| match entity {
            document::EntityType::Hatch(hatch) => Some(hatch.common.handle),
            _ => None,
        })
        .expect("a hatch was made");
    app.tabs[i].scene.selected.insert(hatch);

    type_and_enter(&mut app, "ansi31");
    let command = app.tabs[i].active_cmd.as_ref().expect("HATCH runs");
    let settings = command.hatch_settings().expect("HATCH has panel settings");
    assert!(
        matches!(&settings.fill, HatchFill::Pattern(name) if name == "ANSI31"),
        "{:?}",
        settings.fill
    );
}

#[test]
fn the_blocks_of_layouts_and_anonymous_blocks_are_not_suggested() {
    let mut app = fresh_app();
    define_block(&mut app, "Sheet unit");
    let i = app.active_tab;
    let document = &mut app.tabs[i].scene.document;
    let mut layout = document::tables::BlockRecord::new("Sheet A");
    layout.handle = document.allocate_handle();
    layout.layout = document.allocate_handle();
    let _ = document.block_records.add(layout);
    let mut anonymous = document::tables::BlockRecord::new("Sheet copy");
    anonymous.handle = document.allocate_handle();
    anonymous.flags.anonymous = true;
    let _ = document.block_records.add(anonymous);

    let _ = app.update(Message::CommandInput("sheet".to_string()));
    let blocks: Vec<&str> = app
        .command_line
        .content_rows()
        .iter()
        .filter(|row| row.kind == ContentKind::Block)
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(blocks, ["Sheet unit"]);
}

fn add_text_style(app: &mut OpenCADStudio, name: &str, shape_file: bool) {
    let i = app.active_tab;
    let document = &mut app.tabs[i].scene.document;
    let mut style = document::tables::TextStyle::new(name);
    style.handle = document.allocate_handle();
    style.is_shape_file = shape_file;
    let _ = document.text_styles.add(style);
}

fn add_dim_style(app: &mut OpenCADStudio, name: &str) -> document::types::Handle {
    let i = app.active_tab;
    let document = &mut app.tabs[i].scene.document;
    let mut style = document::tables::DimStyle::new(name);
    style.handle = document.allocate_handle();
    let handle = style.handle;
    let _ = document.dim_styles.add(style);
    handle
}

#[test]
fn a_typed_text_style_name_makes_that_style_current() {
    let mut app = fresh_app();
    add_text_style(&mut app, "Room labels", false);
    let i = app.active_tab;
    app.tabs[i].dirty = false;

    let _ = app.update(Message::CommandInput("labels".to_string()));
    let rows = app.command_line.content_rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, ContentKind::TextStyle);

    let _ = app.update(Message::CommandSubmit);
    let header = &app.tabs[i].scene.document.header;
    assert_eq!(header.current_text_style_name, "Room labels");
    let handle = app.tabs[i]
        .scene
        .document
        .text_styles
        .get("Room labels")
        .map(|style| style.handle);
    assert_eq!(Some(header.current_text_style_handle), handle);
    assert_eq!(app.ribbon.active_text_style, "Room labels");
    assert!(app.tabs[i].dirty);
    assert!(active_command(&app).is_none());
}

#[test]
fn a_shape_file_style_is_not_suggested() {
    let mut app = fresh_app();
    add_text_style(&mut app, "Symbols shapes", true);

    let _ = app.update(Message::CommandInput("symbols".to_string()));
    assert!(app.command_line.content_rows().is_empty());
}

#[test]
fn a_typed_dimension_style_name_makes_that_style_current() {
    let mut app = fresh_app();
    let handle = add_dim_style(&mut app, "Site plan 1-100");

    let _ = app.update(Message::CommandInput("site".to_string()));
    let rows = app.command_line.content_rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, ContentKind::DimStyle);

    let _ = app.update(Message::CommandSubmit);
    let i = app.active_tab;
    let header = &app.tabs[i].scene.document.header;
    assert_eq!(header.current_dimstyle_name, "Site plan 1-100");
    assert_eq!(header.current_dimstyle_handle, handle);
    assert_eq!(app.ribbon.active_dim_style, "Site plan 1-100");
    assert!(active_command(&app).is_none());
}

#[test]
fn a_typed_visual_style_name_draws_the_drawing_in_that_style() {
    use document::entities::ViewportRenderMode;
    let mut app = fresh_app();
    let i = app.active_tab;
    assert_ne!(app.tabs[i].render_mode, ViewportRenderMode::GouraudShaded);

    let _ = app.update(Message::CommandInput("gouraud".to_string()));
    let rows = app.command_line.content_rows();
    assert!(
        rows.iter()
            .any(|row| row.kind == ContentKind::VisualStyle && row.name == "Gouraud Shaded"),
        "{rows:?}"
    );

    let task = app.update(Message::CommandSubmit);
    app.drive_headless_task(task).unwrap();
    assert_eq!(app.tabs[i].render_mode, ViewportRenderMode::GouraudShaded);
    assert!(active_command(&app).is_none());
}

#[test]
fn a_clicked_visual_style_row_draws_the_drawing_in_that_style() {
    use document::entities::ViewportRenderMode;
    let mut app = fresh_app();
    let _ = app.update(Message::CommandInput("flat".to_string()));
    let row = app
        .command_line
        .content_rows()
        .iter()
        .find(|row| row.name == "Flat Shaded")
        .cloned()
        .expect("a visual style row");
    assert_eq!(row.kind, ContentKind::VisualStyle);

    let task = app.update(Message::ContentSuggestionPick(row));
    app.drive_headless_task(task).unwrap();
    let i = app.active_tab;
    assert_eq!(app.tabs[i].render_mode, ViewportRenderMode::FlatShaded);
}

#[test]
fn undo_takes_back_a_text_or_dimension_style_made_current_from_the_suggestions() {
    let mut app = fresh_app();
    add_text_style(&mut app, "Room labels", false);
    add_dim_style(&mut app, "Site plan 1-100");
    let i = app.active_tab;
    let header = &app.tabs[i].scene.document.header;
    let (text_before, dim_before) = (
        header.current_text_style_name.clone(),
        header.current_dimstyle_name.clone(),
    );

    type_and_enter(&mut app, "labels");
    type_and_enter(&mut app, "site");
    let header = &app.tabs[i].scene.document.header;
    assert_eq!(header.current_text_style_name, "Room labels");
    assert_eq!(header.current_dimstyle_name, "Site plan 1-100");

    let _ = app.update(Message::Undo);
    let header = &app.tabs[i].scene.document.header;
    assert_eq!(header.current_text_style_name, "Room labels");
    assert_eq!(header.current_dimstyle_name, dim_before);
    let _ = app.update(Message::Undo);
    let header = &app.tabs[i].scene.document.header;
    assert_eq!(header.current_text_style_name, text_before);
}

#[test]
fn a_drawing_visual_style_name_draws_the_closest_style() {
    use document::entities::ViewportRenderMode;
    let mut app = fresh_app();
    let i = app.active_tab;
    assert_ne!(app.tabs[i].render_mode, ViewportRenderMode::GouraudShaded);

    let _ = app.update(Message::CommandInput("realistic".to_string()));
    let rows = app.command_line.content_rows();
    assert!(
        rows.iter()
            .any(|row| row.kind == ContentKind::VisualStyle && row.name == "Realistic"),
        "{rows:?}"
    );

    let task = app.update(Message::CommandSubmit);
    app.drive_headless_task(task).unwrap();
    assert_eq!(app.tabs[i].render_mode, ViewportRenderMode::GouraudShaded);
}

#[test]
fn the_internal_visual_styles_are_not_suggested() {
    let mut app = fresh_app();

    // `Gouraud` is an internal style of the drawing: only the picker's two
    // Gouraud names show.
    let _ = app.update(Message::CommandInput("gouraud".to_string()));
    let names: Vec<&str> = app
        .command_line
        .content_rows()
        .iter()
        .filter(|row| row.kind == ContentKind::VisualStyle)
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(names, ["Gouraud Shaded", "Gouraud Shaded + Edges"]);
}

#[test]
fn a_visual_style_that_ocs_cannot_draw_is_not_suggested() {
    use document::objects::ObjectType;
    let mut app = fresh_app();
    let i = app.active_tab;
    let document = &mut app.tabs[i].scene.document;
    let dictionary = document.header.acad_visualstyle_dict_handle;
    let handle = document.allocate_handle();
    let mut style = document::objects::VisualStyle::new();
    style.handle = handle;
    style.owner = dictionary;
    style.description = "Blueprint".to_string();
    document
        .objects
        .insert(handle, ObjectType::VisualStyle(style));
    let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) else {
        panic!("a new drawing has the visual style dictionary");
    };
    entries.add_entry("Blueprint", handle);
    assert!(
        crate::io::visual_styles::offered(&app.tabs[i].scene.document)
            .iter()
            .any(|(name, _)| name == "Blueprint")
    );

    let _ = app.update(Message::CommandInput("blueprint".to_string()));
    assert!(app.command_line.content_rows().is_empty());
}
