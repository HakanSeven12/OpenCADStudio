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
) -> Vec<Message> {
    let mut ui = UserInterface::build(
        app.view_main(),
        iced::Size::new(1600.0, 900.0),
        std::mem::take(cache),
        renderer,
    );
    let mut messages = Vec::new();
    if let Some(mut actions) = iced_runtime::task::into_stream(task) {
        while let Some(action) = block_on(actions.next()) {
            match action {
                iced_runtime::Action::Widget(mut operation) => {
                    loop {
                        ui.operate(renderer, operation.as_mut());
                        // iced completes widget queries after traversing the
                        // UI; finish delivers the native paste focus result.
                        match operation.finish() {
                            iced::advanced::widget::operation::Outcome::Chain(next) => {
                                operation = next;
                            }
                            _ => break,
                        }
                    }
                }
                iced_runtime::Action::Output(message) => messages.push(message),
                _ => {}
            }
        }
    }
    *cache = ui.into_cache();
    messages
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
    shortcut_events("c", keyboard::key::Code::KeyC)
}

fn shortcut_events(character: &str, code: keyboard::key::Code) -> [iced::Event; 2] {
    let modifiers = if cfg!(target_os = "macos") {
        keyboard::Modifiers::LOGO
    } else {
        keyboard::Modifiers::CTRL
    };
    [
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character(character.into()),
            modified_key: keyboard::Key::Character(character.into()),
            physical_key: keyboard::key::Physical::Code(code),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }),
    ]
}

fn check_copy_after(start: Option<Message>, typed: &str, expected: usize) {
    let _guard = crate::headless_ui_lock();
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
        let _ = apply_focus_task(&app, &mut renderer, &mut cache, app.focus_cmd_input());
        // Verify the harness really routes a focused field's captured event:
        // it must keep the native text shortcut and leave CAD clipboard alone.
        let (messages, statuses) = key_messages(&app, &mut renderer, &mut cache, &copy_events());
        assert_eq!(statuses[1], iced::event::Status::Captured);
        assert!(!messages
            .iter()
            .any(|m| matches!(m, Message::ShortcutPressed(_))));
        assert!(app.clipboard.is_empty());
        let task = app.update(start);
        let _ = apply_focus_task(&app, &mut renderer, &mut cache, task);
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

#[test]
fn copied_blocks_follow_cursor_when_pasted_via_iced_keyboard() {
    use codec::entities::{Insert, Line};
    use codec::types::Vector3;
    use codec::EntityType;
    use glam::DVec3;

    let _guard = crate::headless_ui_lock();
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .define_block_from_owned_entities(
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 5.0, 0.0),
            ))],
            "ClipboardPreview",
            DVec3::ZERO,
        )
        .unwrap();
    let handles: Vec<_> = [20.0, 70.0]
        .into_iter()
        .map(|x| {
            app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
                "ClipboardPreview",
                Vector3::new(x, 10.0, 0.0),
            )))
        })
        .collect();
    app.tabs[i].scene.select_entities(&handles);
    app.tabs[i].scene.wire_models_for(&handles);

    let mut renderer = block_on(iced::Renderer::new(
        iced_test::core::renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("headless renderer");
    let mut cache = Cache::default();
    // Dispatch both keys through widget capture, subscription, shortcut mapping
    // and the focus probe used by the native paste task.
    for events in [
        copy_events(),
        shortcut_events("v", keyboard::key::Code::KeyV),
    ] {
        let (messages, _) = key_messages(&app, &mut renderer, &mut cache, &events);
        assert!(!messages.is_empty());
        let mut pending = std::collections::VecDeque::from(messages);
        while let Some(message) = pending.pop_front() {
            let task = app.update(message);
            pending.extend(apply_focus_task(&app, &mut renderer, &mut cache, task));
        }
    }
    assert_eq!(app.clipboard.len(), 2);
    let base = app.clipboard_base;
    let command = app.tabs[i]
        .active_cmd
        .as_mut()
        .expect("Ctrl+V starts paste");
    assert_eq!(command.name(), "PASTECLIP");
    for cursor in [DVec3::new(100.0, 80.0, 0.0), DVec3::new(-30.0, 15.0, 0.0)] {
        let preview = command.on_preview_wires(cursor);
        let expanded = crate::scene::model::instance_model::expand_stubs(&preview);
        let points: Vec<_> = expanded
            .iter()
            .flat_map(|wire| (0..wire.points.len()).map(|index| wire.point_world(index, 100.0)))
            .filter(|point| point.is_finite())
            .collect();
        let delta = cursor - base;
        assert_eq!(points.len(), 4, "{points:?}");
        for x in [20.0, 70.0] {
            for source in [DVec3::new(x, 10.0, 0.0), DVec3::new(x + 10.0, 15.0, 0.0)] {
                let expected = source + delta;
                assert!(
                    points.iter().any(|point| point.distance(expected) < 1e-4),
                    "missing {expected:?} in {points:?}"
                );
            }
        }
    }
    let result = command.on_point(DVec3::new(100.0, 80.0, 0.0));
    let _ = app.apply_cmd_result(result);
    assert!(app.tabs[i].active_cmd.is_none());
    // The command driver clears selection when placement finishes. Inspect
    // the committed document rather than relying on a post-command selection.
    let pasted: Vec<_> = app.tabs[i]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Insert(insert) if !handles.contains(&insert.common.handle) => Some(insert),
            _ => None,
        })
        .collect();
    assert_eq!(pasted.len(), 2);
    for insert in pasted {
        assert_eq!(insert.block_name, "ClipboardPreview");
        assert!((insert.insert_point.y - (90.0 - base.y)).abs() < 1e-6);
    }
}
