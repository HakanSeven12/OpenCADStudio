//! The Hatch panel through the real widget tree, headless: in HATCH and
//! in HATCHEDIT on an existing hatch.
use super::*;
use iced::futures::{executor::block_on, stream, FutureExt, StreamExt};
use iced_test::core::{renderer::Headless, shell, window};
use iced_test::futures::subscription;
use iced_test::runtime::user_interface::{Cache, UserInterface};
use iced_test::simulator::Simulator;

const SIZE: iced::Size = iced::Size::new(1600.0, 900.0);

fn hatching() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    let _ = app.dispatch_command("HATCH");
    assert!(
        app.tabs[1]
            .active_cmd
            .as_ref()
            .and_then(|command| command.hatch_settings())
            .is_some(),
        "HATCH runs"
    );
    app
}

fn kind(app: &OpenCADStudio) -> u8 {
    let settings = app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .and_then(|command| command.hatch_settings())
        .expect("HATCH still runs");
    crate::ui::window::hatch_panel::kind_code(&settings.fill)
}

fn label(key: &str) -> String {
    crate::i18n::loader().get_attr("hatch-panel", key)
}

/// One mouse event through the app the way the running program handles it:
/// the widgets see it, the app takes their messages, the view is rebuilt.
fn mouse(
    app: &mut OpenCADStudio,
    renderer: &mut iced::Renderer,
    cache: &mut Cache,
    at: iced::Point,
    event: iced::mouse::Event,
) -> Vec<String> {
    let mut ui = UserInterface::build(app.view_main(), SIZE, std::mem::take(cache), renderer);
    let mut messages = Vec::new();
    let events = [iced::Event::Mouse(event)];
    let (_, statuses) = ui.update(
        &window::Headless,
        &shell::Waker::noop(),
        &events,
        iced::mouse::Cursor::Available(at),
        renderer,
        &mut messages,
    );
    *cache = ui.into_cache();
    let window = window::Id::unique();
    for recipe in subscription::into_recipes(app.subscription()) {
        let events: Vec<_> = events
            .iter()
            .zip(&statuses)
            .map(|(event, status)| subscription::Event::Interaction {
                window,
                event: event.clone(),
                status: *status,
            })
            .collect();
        let mut output = recipe.stream(Box::pin(stream::iter(events)));
        while let Some(Some(message)) = output.next().now_or_never() {
            messages.push(message);
        }
    }
    let seen = messages.iter().map(|m| format!("{m:?}")).collect();
    for message in messages {
        let _ = app.update(message);
    }
    seen
}

fn spots(app: &OpenCADStudio) -> (iced::Point, Vec<iced::Point>) {
    let mut ui = Simulator::with_size(Default::default(), SIZE, app.view_main());
    let b = ui.find(label("type")).unwrap().visible_bounds().unwrap();
    let list = iced::Point::new(b.x + 150.0, b.center_y());
    let rows = (0..4)
        .map(|n| iced::Point::new(list.x, b.center_y() + 10.8 + 21.6 * (n as f32 + 0.5)))
        .collect();
    (list, rows)
}

fn renderer() -> iced::Renderer {
    block_on(iced::Renderer::new(
        iced_test::core::renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("headless renderer")
}

/// Click at `at` through the live loop (move, press, release).
fn click(
    app: &mut OpenCADStudio,
    renderer: &mut iced::Renderer,
    cache: &mut Cache,
    at: iced::Point,
) {
    use iced::mouse::{Button, Event as M};
    for event in [
        M::CursorMoved { position: at },
        M::ButtonPressed(Button::Left),
        M::ButtonReleased(Button::Left),
    ] {
        let _ = mouse(app, renderer, cache, at, event);
    }
}

/// Picks "User defined", the fourth row of the open Type list.
fn choose_user_defined(app: &mut OpenCADStudio) {
    let (list, rows) = spots(app);
    let mut renderer = renderer();
    let mut cache = Cache::default();
    click(app, &mut renderer, &mut cache, list);
    click(app, &mut renderer, &mut cache, rows[3]);
}

#[test]
fn the_type_list_switches_a_new_hatch_to_user_defined() {
    let _guard = crate::headless_ui_lock();
    let mut app = hatching();
    choose_user_defined(&mut app);
    assert_eq!(kind(&app), 3, "Type is User defined");
}

/// A solid hatch over (0,0)–(10,10), selected, with HATCHEDIT started on it.
fn editing_hatch() -> (OpenCADStudio, document::Handle) {
    use document::entities::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge};
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    let mut path = BoundaryPath::new();
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
            .into_iter()
            .map(|p| document::types::Vector2::new(p[0], p[1]))
            .collect(),
        true,
    )));
    hatch.paths.push(path);
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Hatch(Box::new(hatch)));
    app.tabs[i].scene.select_entity(handle, true);
    let _ = app.dispatch_command("HATCHEDIT");
    (app, handle)
}

fn the_hatch(app: &OpenCADStudio, handle: document::Handle) -> document::entities::Hatch {
    match app.tabs[app.active_tab].scene.document.get_entity(handle) {
        Some(document::EntityType::Hatch(hatch)) => (**hatch).clone(),
        other => panic!("hatch expected, got {other:?}"),
    }
}

/// HATCHEDIT on a hatch shows the Hatch panel filled from that hatch, the
/// way HATCH shows it for a new one. Before, HATCHEDIT had no panel, so the
/// Type (and every other panel setting) of an existing hatch could not be
/// changed there.
#[test]
fn hatchedit_shows_the_hatch_panel_for_the_hatch() {
    let _guard = crate::headless_ui_lock();
    let (app, _) = editing_hatch();
    let i = app.active_tab;
    let command = app.tabs[i].active_cmd.as_ref().expect("HATCHEDIT runs");
    assert_eq!(command.name(), "HATCHEDIT");
    let settings = command.hatch_settings().expect("HATCHEDIT shows the panel");
    assert_eq!(
        kind(&app),
        0,
        "Type is Solid, from the hatch: {:?}",
        settings.fill
    );
    let mut ui = Simulator::with_size(Default::default(), SIZE, app.view_main());
    assert!(ui.find(label("type")).is_ok(), "the panel is on screen");
}

/// Type → User defined, then OK, turns the solid hatch into a user-defined
/// one, in one undo step.
#[test]
fn hatchedit_panel_ok_changes_the_hatch_type() {
    let _guard = crate::headless_ui_lock();
    let (mut app, handle) = editing_hatch();
    let i = app.active_tab;
    choose_user_defined(&mut app);
    assert_eq!(kind(&app), 3, "Type is User defined");
    let _ = app.update(Message::HatchPanel(
        crate::ui::window::hatch_panel::HatchPanelMsg::Accept,
    ));
    assert!(app.tabs[i].active_cmd.is_none(), "OK ends HATCHEDIT");
    let hatch = the_hatch(&app, handle);
    assert_eq!(
        hatch.pattern_type,
        document::entities::HatchPatternType::UserDefined
    );
    assert!(!hatch.is_solid, "no longer solid");
    // User-defined lines are built from angle + spacing when drawn.
    assert_eq!(hatch.pattern.name, "_USER");

    let _ = app.update(Message::Undo);
    let hatch = the_hatch(&app, handle);
    assert!(hatch.is_solid, "one undo brings the solid fill back");
}

/// Cancel leaves the hatch as it was.
#[test]
fn hatchedit_panel_cancel_leaves_the_hatch() {
    let _guard = crate::headless_ui_lock();
    let (mut app, handle) = editing_hatch();
    let i = app.active_tab;
    choose_user_defined(&mut app);
    let _ = app.update(Message::HatchPanel(
        crate::ui::window::hatch_panel::HatchPanelMsg::Cancel,
    ));
    assert!(app.tabs[i].active_cmd.is_none(), "Cancel ends HATCHEDIT");
    assert!(the_hatch(&app, handle).is_solid);
}
