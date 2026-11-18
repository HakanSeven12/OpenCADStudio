//! Fence / WPolygon / CPolygon typed at the "Select objects:" prompt of an
//! ordinary editing command, not only inside SELECT.

use super::super::*;
use document::entities::Line;
use document::types::Vector3;
use document::Handle;

fn drawing() -> (OpenCADStudio, Handle, Handle, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut line = |a: [f64; 2], b: [f64; 2]| {
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::new(a[0], a[1], 0.0),
                Vector3::new(b[0], b[1], 0.0),
            )))
    };
    // Inside the polygon below, crossing its edge, and far away.
    let inside = line([4.0, 4.0], [6.0, 6.0]);
    let crossing = line([9.0, 5.0], [14.0, 5.0]);
    let away = line([30.0, 30.0], [32.0, 30.0]);
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-2., -2., 0.),
        glam::DVec3::new(34., 34., 0.),
        1600. / 900.,
    );
    (app, inside, crossing, away)
}

fn feed(app: &mut OpenCADStudio, text: &str) {
    let _ = app.update(Message::CommandInput(text.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

fn exists(app: &OpenCADStudio, h: Handle) -> bool {
    app.tabs[app.active_tab]
        .scene
        .document
        .get_entity(h)
        .is_some()
}

fn erase_with(keyword: &str, points: &[&str]) -> (bool, bool, bool) {
    let (mut app, inside, crossing, away) = drawing();
    let _ = app.run_command_line("ERASE");
    feed(&mut app, keyword);
    for p in points {
        feed(&mut app, p);
    }
    feed(&mut app, ""); // end the path
    feed(&mut app, ""); // end the selection
    assert!(
        app.tabs[app.active_tab].active_cmd.is_none(),
        "ERASE must finish"
    );
    (
        !exists(&app, inside),
        !exists(&app, crossing),
        !exists(&app, away),
    )
}

const SQUARE: [&str; 4] = ["2,2", "10,2", "10,8", "2,8"];

#[test]
fn erase_wpolygon_takes_only_what_lies_inside() {
    assert_eq!(erase_with("WP", &SQUARE), (true, false, false));
}

#[test]
fn erase_cpolygon_also_takes_what_crosses_the_edge() {
    assert_eq!(erase_with("CP", &SQUARE), (true, true, false));
}

#[test]
fn erase_fence_takes_what_the_line_cuts() {
    assert_eq!(erase_with("F", &["12,0", "12,10"]), (false, true, false));
}

/// A left click at a world point, as the mouse gives it.
fn click(app: &mut OpenCADStudio, x: f64, y: f64) {
    let s = screen(app, x, y);
    let _ = app.update(Message::ViewportLeftPressAt(s));
    let _ = app.update(Message::ViewportLeftRelease);
}

/// The cursor moved over a world point.
fn hover(app: &mut OpenCADStudio, x: f64, y: f64) {
    let s = screen(app, x, y);
    let _ = app.update(Message::ViewportMove(s));
}

fn screen(app: &OpenCADStudio, x: f64, y: f64) -> iced::Point {
    let scene = &app.tabs[app.active_tab].scene;
    let (w, h) = scene.selection.borrow().view.vp_size;
    let b = scene.active_model_tile_bounds(w, h);
    let s = scene
        .camera
        .borrow()
        .project(
            glam::DVec3::new(x, y, 0.0),
            iced::Rectangle {
                x: 0.,
                y: 0.,
                width: b.width,
                height: b.height,
            },
        )
        .unwrap();
    iced::Point::new(s.x + b.x, s.y + b.y)
}

fn history(app: &OpenCADStudio) -> String {
    app.command_line
        .history
        .iter()
        .map(|e| e.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn selected(app: &OpenCADStudio, h: Handle) -> bool {
    app.tabs[app.active_tab].scene.selected.contains(&h)
}

/// Click a box corner in empty space with no command running, type the
/// keyword, click the other vertices, Enter.
fn implied_path(keyword: &str) -> (OpenCADStudio, Handle, Handle, Handle) {
    let (mut app, inside, crossing, away) = drawing();
    click(&mut app, 2.0, 2.0);
    assert!(
        history(&app).contains("Specify opposite corner or [Fence/WPolygon/CPolygon]:"),
        "a clicked box offers the paths:\n{}",
        history(&app)
    );
    feed(&mut app, keyword);
    click(&mut app, 10.0, 2.0);
    click(&mut app, 10.0, 8.0);
    click(&mut app, 2.0, 8.0);
    feed(&mut app, "");
    assert!(
        app.tabs[app.active_tab].active_cmd.is_none(),
        "the path is the whole selection:\n{}",
        history(&app)
    );
    (app, inside, crossing, away)
}

#[test]
fn a_clicked_box_turns_into_a_window_polygon_from_the_clicked_point() {
    let (app, inside, crossing, away) = implied_path("WP");
    assert!(selected(&app, inside));
    assert!(!selected(&app, crossing));
    assert!(!selected(&app, away));
    assert!(exists(&app, inside));
}

#[test]
fn a_clicked_box_turns_into_a_crossing_polygon_from_the_clicked_point() {
    let (app, inside, crossing, away) = implied_path("CP");
    assert!(selected(&app, inside));
    assert!(selected(&app, crossing));
    assert!(!selected(&app, away));
}

#[test]
fn a_box_clicked_at_select_objects_turns_into_a_fence() {
    let (mut app, inside, crossing, away) = drawing();
    let _ = app.run_command_line("ERASE");
    click(&mut app, 12.0, 0.0);
    feed(&mut app, "F");
    click(&mut app, 12.0, 10.0);
    feed(&mut app, ""); // end the fence
    feed(&mut app, ""); // end the selection
    assert!(
        app.tabs[app.active_tab].active_cmd.is_none(),
        "{}",
        history(&app)
    );
    assert!(exists(&app, inside));
    assert!(!exists(&app, crossing), "{}", history(&app));
    assert!(exists(&app, away));
}

fn highlighted(app: &OpenCADStudio, h: Handle) -> bool {
    app.tabs[app.active_tab]
        .scene
        .box_hover_highlights()
        .contains(&h)
}

/// What the path would catch rolls over while it is drawn, the cursor
/// standing for its next vertex, and goes when Enter applies it.
fn preview_with(keyword: &str) -> (bool, bool, bool) {
    let (mut app, inside, crossing, away) = drawing();
    click(&mut app, 2.0, 2.0);
    feed(&mut app, keyword);
    click(&mut app, 10.0, 2.0);
    click(&mut app, 10.0, 8.0);
    hover(&mut app, 2.0, 8.0);
    let lit = (
        highlighted(&app, inside),
        highlighted(&app, crossing),
        highlighted(&app, away),
    );
    feed(&mut app, "");
    assert!(
        app.tabs[app.active_tab]
            .scene
            .box_hover_highlights()
            .is_empty(),
        "the preview ends with the path"
    );
    lit
}

#[test]
fn a_window_polygon_highlights_what_it_would_take_while_drawn() {
    assert_eq!(preview_with("WP"), (true, false, false));
}

#[test]
fn a_crossing_polygon_highlights_what_it_would_take_while_drawn() {
    assert_eq!(preview_with("CP"), (true, true, false));
}

/// A click that adds a vertex leaves the rolled-over objects lit: they must
/// not go dark until the next pointer move brings them back.
#[test]
fn a_vertex_click_keeps_the_path_highlights() {
    let (mut app, _, crossing, away) = drawing();
    click(&mut app, 2.0, 2.0);
    feed(&mut app, "CP");
    click(&mut app, 10.0, 2.0);
    hover(&mut app, 10.0, 8.0);
    assert!(highlighted(&app, crossing), "lit before the click");
    click(&mut app, 10.0, 8.0);
    assert!(
        highlighted(&app, crossing) && !highlighted(&app, away),
        "the click must not clear the preview"
    );
    assert!(
        app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.selection_path().is_some()),
        "the path goes on after the click"
    );
}

#[test]
fn escape_drops_a_path_and_its_highlights() {
    let (mut app, inside, _, _) = drawing();
    click(&mut app, 2.0, 2.0);
    feed(&mut app, "CP");
    click(&mut app, 10.0, 2.0);
    click(&mut app, 10.0, 8.0);
    hover(&mut app, 2.0, 8.0);
    assert!(highlighted(&app, inside));
    let _ = app.update(Message::CommandEscape);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    assert!(app.tabs[app.active_tab]
        .scene
        .box_hover_highlights()
        .is_empty());
    assert!(!selected(&app, inside));
}

const BOX_PROMPT: &str = "Specify opposite corner or [Fence/WPolygon/CPolygon]:";

fn option_keywords(app: &OpenCADStudio) -> Vec<String> {
    app.command_line
        .step_options()
        .iter()
        .map(|option| option.keyword.clone())
        .collect()
}

fn pinned_prompt(app: &OpenCADStudio) -> Option<String> {
    app.command_line
        .history
        .iter()
        .rev()
        .find(|entry| entry.pinned)
        .map(|entry| entry.text.clone())
}

/// Whether the line the command line keeps and hangs the option buttons on
/// is the box prompt.
fn box_prompt_pinned(app: &OpenCADStudio) -> bool {
    pinned_prompt(app).is_some_and(|text| text.ends_with(BOX_PROMPT))
}

#[test]
fn a_clicked_box_asks_with_the_paths_as_its_options() {
    let (mut app, _, _, _) = drawing();
    click(&mut app, 2.0, 2.0);
    // The prompt stays, its three paths shown the way a command step's are.
    assert!(box_prompt_pinned(&app), "{:?}", pinned_prompt(&app));
    assert_eq!(option_keywords(&app), ["F", "WP", "CP"]);
    // Once the path is running the prompt and the options are its own.
    feed(&mut app, "F");
    assert!(!box_prompt_pinned(&app), "{:?}", pinned_prompt(&app));
    assert_ne!(option_keywords(&app), ["F", "WP", "CP"]);
    let _ = app.update(Message::CommandEscape);
    assert!(option_keywords(&app).is_empty());
}

#[test]
fn letters_typed_at_a_clicked_box_open_no_command_suggestions() {
    let (mut app, _, _, _) = drawing();
    assert!(app.autocomplete_allowed());
    click(&mut app, 2.0, 2.0);
    // F answers the prompt here; it is not the start of FILLET.
    let _ = app.update(Message::CommandInput("F".to_string()));
    assert!(!app.autocomplete_allowed());
    // Escape takes the typed letter first and the box second, as anywhere.
    let _ = app.update(Message::CommandEscape);
    assert!(app.command_line.input.is_empty());
    assert!(!app.autocomplete_allowed());
    let _ = app.update(Message::CommandEscape);
    assert!(app.autocomplete_allowed());
}

#[test]
fn escape_at_a_clicked_box_takes_its_options_away() {
    let (mut app, _, _, _) = drawing();
    click(&mut app, 2.0, 2.0);
    assert_eq!(option_keywords(&app), ["F", "WP", "CP"]);
    let _ = app.update(Message::CommandEscape);
    assert!(option_keywords(&app).is_empty());
    assert!(!box_prompt_pinned(&app), "{:?}", pinned_prompt(&app));
}

#[test]
fn a_path_button_at_a_clicked_box_starts_from_the_clicked_point() {
    let (mut app, inside, crossing, away) = drawing();
    click(&mut app, 2.0, 2.0);
    let _ = app.update(Message::CommandOptionPick("CP".to_string()));
    click(&mut app, 10.0, 2.0);
    click(&mut app, 10.0, 8.0);
    click(&mut app, 2.0, 8.0);
    feed(&mut app, "");
    assert!(
        app.tabs[app.active_tab].active_cmd.is_none(),
        "{}",
        history(&app)
    );
    assert!(selected(&app, inside), "{}", history(&app));
    assert!(selected(&app, crossing));
    assert!(!selected(&app, away));
}
