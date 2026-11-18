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
                document::EntityType::Line(document::entities::Line::from_points(
                    document::types::Vector3::new(0.0, y as f64, 0.0),
                    document::types::Vector3::new(10.0, y as f64, 0.0),
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
    use document::entities::{Insert, Line};
    use document::types::Vector3;
    use document::EntityType;
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

#[test]
fn a_polygon_picked_from_a_clicked_box_copies_via_iced_keyboard() {
    let _guard = crate::headless_ui_lock();
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let line = |a: [f64; 2], b: [f64; 2]| {
        document::EntityType::Line(document::entities::Line::from_points(
            document::types::Vector3::new(a[0], a[1], 0.0),
            document::types::Vector3::new(b[0], b[1], 0.0),
        ))
    };
    // One line inside the polygon below, one far away.
    let handles = app.tabs[i].scene.add_entities(vec![
        line([4.0, 4.0], [6.0, 6.0]),
        line([30.0, 30.0], [32.0, 30.0]),
    ]);
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-2., -2., 0.),
        glam::DVec3::new(34., 34., 0.),
        1600. / 900.,
    );
    let mut renderer = block_on(iced::Renderer::new(
        iced_test::core::renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("headless renderer");
    let mut cache = Cache::default();
    // The command field holds the keyboard, as it does while the path is
    // typed and its vertices are entered.
    let _ = apply_focus_task(&app, &mut renderer, &mut cache, app.focus_cmd_input());
    let screen = |app: &OpenCADStudio, x: f64, y: f64| {
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
    };
    let mut steps = vec![(2.0, 2.0, Some("CP"))];
    steps.extend([(10.0, 2.0, None), (10.0, 8.0, None), (2.0, 8.0, Some(""))]);
    for (x, y, typed) in steps {
        let at = screen(&app, x, y);
        let mut messages = vec![
            Message::ViewportLeftPressAt(at),
            Message::ViewportLeftRelease,
        ];
        if let Some(text) = typed {
            messages.push(Message::CommandInput(text.to_string()));
            messages.push(Message::CommandSubmit);
        }
        let mut pending = std::collections::VecDeque::from(messages);
        while let Some(message) = pending.pop_front() {
            let task = app.update(message);
            pending.extend(apply_focus_task(&app, &mut renderer, &mut cache, task));
        }
    }
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        app.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [handles[0]]
    );
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
        1,
        "COPYCLIP must copy what the polygon selected"
    );
}

#[test]
fn objects_gathered_by_select_copy_via_iced_keyboard() {
    let _guard = crate::headless_ui_lock();
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let _ = app.tabs[i].scene.add_entities(
        (0..2)
            .map(|y| {
                document::EntityType::Line(document::entities::Line::from_points(
                    document::types::Vector3::new(0.0, y as f64, 0.0),
                    document::types::Vector3::new(10.0, y as f64, 0.0),
                ))
            })
            .collect(),
    );
    let mut renderer = block_on(iced::Renderer::new(
        iced_test::core::renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("headless renderer");
    let mut cache = Cache::default();
    let _ = apply_focus_task(&app, &mut renderer, &mut cache, app.focus_cmd_input());
    // SELECT, everything, Enter: all typed, so the command field holds the
    // keyboard until the selection is handed over.
    for typed in ["SELECT", "ALL", ""] {
        let mut pending = std::collections::VecDeque::from([
            Message::CommandInput(typed.to_string()),
            Message::CommandSubmit,
        ]);
        while let Some(message) = pending.pop_front() {
            let task = app.update(message);
            pending.extend(apply_focus_task(&app, &mut renderer, &mut cache, task));
        }
    }
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(app.tabs[i].scene.selected.len(), 2);
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
        2,
        "COPYCLIP must copy what SELECT gathered"
    );
}

/// Esc and the keyboard: every key goes through the widget tree, the
/// subscription and the app's own update, as in the window.
mod esc_keyboard {
    use super::*;
    use iced::keyboard::{key::Code, key::Named, Key, Modifiers};

    struct H {
        app: OpenCADStudio,
        r: iced::Renderer,
        c: Cache,
    }

    fn pump(h: &mut H, msgs: Vec<Message>) {
        let mut q = std::collections::VecDeque::from(msgs);
        let mut n = 0;
        while let Some(m) = q.pop_front() {
            n += 1;
            if n > 200 {
                break;
            }
            let t = h.app.update(m);
            q.extend(apply_focus_task(&h.app, &mut h.r, &mut h.c, t));
        }
    }

    fn key(
        h: &mut H,
        k: Key,
        text: Option<&str>,
        mods: Modifiers,
        code: Code,
    ) -> iced::event::Status {
        let evs = [
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)),
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: k.clone(),
                modified_key: k,
                physical_key: keyboard::key::Physical::Code(code),
                location: keyboard::Location::Standard,
                modifiers: mods,
                text: text.map(|t| t.into()),
                repeat: false,
            }),
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::empty())),
        ];
        let (msgs, st) = key_messages(&h.app, &mut h.r, &mut h.c, &evs);
        pump(h, msgs);
        st[1]
    }

    fn esc(h: &mut H) {
        key(
            h,
            Key::Named(Named::Escape),
            None,
            Modifiers::empty(),
            Code::Escape,
        );
    }
    fn enter(h: &mut H) {
        key(
            h,
            Key::Named(Named::Enter),
            None,
            Modifiers::empty(),
            Code::Enter,
        );
    }
    fn type_text(h: &mut H, s: &str) {
        for ch in s.chars() {
            let t = ch.to_string();
            key(
                h,
                Key::Character(t.as_str().into()),
                Some(&t),
                Modifiers::empty(),
                Code::KeyA,
            );
        }
    }
    fn focus(h: &mut H) {
        let t = h.app.focus_cmd_input();
        let ms = apply_focus_task(&h.app, &mut h.r, &mut h.c, t);
        pump(h, ms);
    }
    fn shift_d(h: &mut H) -> (bool, iced::event::Status) {
        let before = h.app.quad;
        let st = key(
            h,
            Key::Character("D".into()),
            Some("D"),
            Modifiers::SHIFT,
            Code::KeyD,
        );
        (h.app.quad != before, st)
    }
    fn new() -> H {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.shortcut_bindings
            .insert("SHIFT+D".into(), "QUADTOGGLE".into());
        app.shortcut_bindings
            .insert("SHIFT+L".into(), "LINE".into());
        let r = block_on(iced::Renderer::new(
            iced_test::core::renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .unwrap();
        H {
            app,
            r,
            c: Cache::default(),
        }
    }
    fn assert_shift_d_runs(h: &mut H, what: &str) {
        let (ran, status) = shift_d(h);
        assert!(
            ran,
            "{what}: Shift+D did not run its shortcut; widget status {status:?}, command line {:?}",
            h.app.command_line.input
        );
        assert!(
            h.app.command_line.input.is_empty(),
            "{what}: the letter was typed"
        );
    }

    #[test]
    fn esc_after_an_unknown_command_returns_the_keyboard() {
        let _g = crate::headless_ui_lock();
        let mut h = new();
        focus(&mut h);
        type_text(&mut h, "NOSUCHCOMMAND");
        enter(&mut h);
        esc(&mut h);
        assert_shift_d_runs(&mut h, "unknown command");
    }

    #[test]
    fn esc_with_typed_text_returns_the_keyboard() {
        let _g = crate::headless_ui_lock();
        let mut h = new();
        focus(&mut h);
        type_text(&mut h, "LI");
        esc(&mut h);
        assert_shift_d_runs(&mut h, "typed text");
    }

    #[test]
    fn esc_that_ends_a_typed_command_returns_the_keyboard() {
        let _g = crate::headless_ui_lock();
        let mut h = new();
        focus(&mut h);
        type_text(&mut h, "LINE");
        enter(&mut h);
        assert!(h.app.tabs[0].active_cmd.is_some(), "LINE is running");
        esc(&mut h);
        assert!(h.app.tabs[0].active_cmd.is_none(), "Esc ended LINE");
        assert_shift_d_runs(&mut h, "LINE at its first point");
    }

    #[test]
    fn esc_in_the_middle_of_a_command_returns_the_keyboard() {
        let _g = crate::headless_ui_lock();
        let mut h = new();
        focus(&mut h);
        type_text(&mut h, "LINE");
        enter(&mut h);
        type_text(&mut h, "0,0");
        enter(&mut h);
        assert!(h.app.tabs[0].active_cmd.is_some(), "LINE is running");
        esc(&mut h);
        assert!(h.app.tabs[0].active_cmd.is_none(), "Esc ended LINE");
        assert_shift_d_runs(&mut h, "LINE after its first point");
    }

    #[test]
    fn esc_after_a_shortcut_started_command_returns_the_keyboard() {
        let _g = crate::headless_ui_lock();
        let mut h = new();
        key(
            &mut h,
            Key::Character("L".into()),
            Some("L"),
            Modifiers::SHIFT,
            Code::KeyL,
        );
        assert!(h.app.tabs[0].active_cmd.is_some(), "Shift+L started LINE");
        esc(&mut h);
        assert!(h.app.tabs[0].active_cmd.is_none(), "Esc ended LINE");
        assert_shift_d_runs(&mut h, "command started by Shift+L");
    }
}
