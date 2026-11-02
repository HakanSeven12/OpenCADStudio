//! Exercise widget capture and the installed subscription, not run_shortcut.
use super::*;
use iced::futures::{executor::block_on, stream, FutureExt, StreamExt};
use iced_test::core::{renderer::Headless, shell, window};
use iced_test::futures::subscription;
use iced_test::runtime::user_interface::{Cache, UserInterface};

fn apply_focus_task(
    app: &OpenCADStudio,
    renderer: &mut iced::Renderer,
    cache: &mut Cache,
    task: Task<Message>,
) {
    let mut ui = UserInterface::build(
        app.view_main(),
        iced::Size::new(1600.0, 900.0),
        std::mem::take(cache),
        renderer,
    );
    if let Some(mut actions) = iced_runtime::task::into_stream(task) {
        while let Some(action) = block_on(actions.next()) {
            if let iced_runtime::Action::Widget(mut operation) = action {
                ui.operate(renderer, operation.as_mut());
            }
        }
    }
    *cache = ui.into_cache();
}

fn key_messages(
    app: &OpenCADStudio,
    renderer: &mut iced::Renderer,
    cache: &mut Cache,
    events: &[iced::Event],
) -> (Vec<Message>, Vec<iced::event::Status>) {
    let mut ui = UserInterface::build(
        app.view_main(),
        iced::Size::new(1600.0, 900.0),
        std::mem::take(cache),
        renderer,
    );
    let mut messages = Vec::new();
    let (_, statuses) = ui.update(
        &window::Headless,
        &shell::Waker::noop(),
        events,
        iced::mouse::Cursor::Unavailable,
        renderer,
        &mut messages,
    );
    *cache = ui.into_cache();
    // Broadcast the real widget result to every installed recipe, just as
    // iced's window runtime does. Pending timers/control streams are dropped.
    let window = window::Id::unique();
    for recipe in subscription::into_recipes(app.subscription()) {
        let interactions: Vec<_> = events
            .iter()
            .zip(&statuses)
            .map(|(event, status)| subscription::Event::Interaction {
                window,
                event: event.clone(),
                status: *status,
            })
            .collect();
        let mut output = recipe.stream(Box::pin(stream::iter(interactions)));
        while let Some(Some(message)) = output.next().now_or_never() {
            messages.push(message);
        }
    }
    (messages, statuses)
}

fn copy_events() -> [iced::Event; 2] {
    let modifiers = if cfg!(target_os = "macos") {
        keyboard::Modifiers::LOGO
    } else {
        keyboard::Modifiers::CTRL
    };
    [
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("c".into()),
            modified_key: keyboard::Key::Character("c".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyC),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }),
    ]
}

fn check_copy_after(start: Option<Message>, typed: &str, expected: usize) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let handles = app.tabs[i].scene.add_entities(
        (0..2)
            .map(|y| {
                codec::EntityType::Line(codec::entities::Line::from_points(
                    codec::types::Vector3::new(0.0, y as f64, 0.0),
                    codec::types::Vector3::new(10.0, y as f64, 0.0),
                ))
            })
            .collect(),
    );
    app.tabs[i].scene.select_entities(&handles[..1]);
    let mut renderer = block_on(iced::Renderer::new(
        iced_test::core::renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("headless renderer");
    let mut cache = Cache::default();
    if let Some(start) = start {
        if matches!(start, Message::CommandSubmit) {
            app.command_line.input = typed.into();
        }
        apply_focus_task(&app, &mut renderer, &mut cache, app.focus_cmd_input());
        // Verify the harness really routes a focused field's captured event:
        // it must keep the native text shortcut and leave CAD clipboard alone.
        let (messages, statuses) = key_messages(&app, &mut renderer, &mut cache, &copy_events());
        assert_eq!(statuses[1], iced::event::Status::Captured);
        assert!(!messages
            .iter()
            .any(|m| matches!(m, Message::ShortcutPressed(_))));
        assert!(app.clipboard.is_empty());
        let task = app.update(start);
        apply_focus_task(&app, &mut renderer, &mut cache, task);
    }
    // Baseline (no start): the same committed selection used by a canvas pick.
    assert_eq!(app.tabs[i].scene.selected.len(), expected);
    assert!(app.tabs[i].active_cmd.is_none());
    let (messages, statuses) = key_messages(&app, &mut renderer, &mut cache, &copy_events());
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ShortcutPressed(_))),
        "Ctrl+C did not reach the shortcut subscription; widget status: {:?}",
        statuses[1]
    );
    for message in messages {
        let _ = app.update(message);
    }
    assert_eq!(
        app.clipboard.len(),
        expected,
        "COPYCLIP must copy the committed selection"
    );
}

#[test]
fn context_menu_select_similar_copies_via_iced_keyboard() {
    check_copy_after(
        Some(Message::ContextMenuPick(
            crate::ui::popup::context_menu::MenuAction::SelectSimilar,
        )),
        "",
        2,
    );
}

#[test]
fn context_menu_invert_selection_copies_via_iced_keyboard() {
    // One of the two lines is selected; inverting leaves the other one.
    check_copy_after(
        Some(Message::ContextMenuPick(
            crate::ui::popup::context_menu::MenuAction::InvertSelection,
        )),
        "",
        1,
    );
}

#[test]
fn typed_select_all_copies_via_iced_keyboard() {
    check_copy_after(Some(Message::CommandSubmit), "SELECTALL", 2);
}

#[test]
fn typed_select_similar_copies_via_iced_keyboard() {
    // CommandSubmit is the real message published by the command text_input.
    check_copy_after(Some(Message::CommandSubmit), "SELECTSIMILAR", 2);
}

#[test]
fn canvas_selection_copies_via_iced_keyboard() {
    check_copy_after(None, "", 1);
}
