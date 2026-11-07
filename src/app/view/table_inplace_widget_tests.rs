//! Keyboard capture through the real widget tree and subscriptions, headless.
use super::*;
use iced::futures::{executor::block_on, stream, FutureExt, StreamExt};
use iced_test::core::{renderer::Headless, shell, window};
use iced_test::futures::subscription;
use iced_test::runtime::user_interface::{Cache, UserInterface};

fn focus(app: &OpenCADStudio, renderer: &mut iced::Renderer, cache: &mut Cache, task: Task<Message>) {
    let mut ui = UserInterface::build(
        app.view_main(), iced::Size::new(1600.0, 900.0), std::mem::take(cache), renderer,
    );
    if let Some(mut actions) = iced_runtime::task::into_stream(task) {
        while let Some(action) = block_on(actions.next()) {
            if let iced_runtime::Action::Widget(mut operation) = action {
                loop {
                    ui.operate(renderer, operation.as_mut());
                    match operation.finish() {
                        iced::advanced::widget::operation::Outcome::Chain(next) => operation = next,
                        _ => break,
                    }
                }
            }
        }
    }
    *cache = ui.into_cache();
}

fn key(
    app: &mut OpenCADStudio, renderer: &mut iced::Renderer, cache: &mut Cache,
    logical: keyboard::Key, code: keyboard::key::Code, text: Option<&str>, modifiers: keyboard::Modifiers,
) {
    let events = [
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: logical.clone(), modified_key: logical,
            physical_key: keyboard::key::Physical::Code(code),
            location: keyboard::Location::Standard, modifiers,
            text: text.map(Into::into), repeat: false,
        }),
    ];
    let mut ui = UserInterface::build(
        app.view_main(), iced::Size::new(1600.0, 900.0), std::mem::take(cache), renderer,
    );
    let mut messages = Vec::new();
    let (_, statuses) = ui.update(
        &window::Headless, &shell::Waker::noop(), &events,
        iced::mouse::Cursor::Unavailable, renderer, &mut messages,
    );
    *cache = ui.into_cache();
    let window = window::Id::unique();
    for recipe in subscription::into_recipes(app.subscription()) {
        let events: Vec<_> = events.iter().zip(&statuses).map(|(event, status)| {
            subscription::Event::Interaction { window, event: event.clone(), status: *status }
        }).collect();
        let mut output = recipe.stream(Box::pin(stream::iter(events)));
        while let Some(Some(message)) = output.next().now_or_never() { messages.push(message); }
    }
    for message in messages {
        let task = app.update(message);
        focus(app, renderer, cache, task);
    }
}

#[test]
fn table_pick_releases_command_input_and_routes_real_keys_into_the_cell() {
    let _guard = crate::headless_ui_lock();
    let (mut app, handle) = crate::app::table_inplace_tests::fixture("abcd");
    let mut renderer = block_on(iced::Renderer::new(
        iced_test::core::renderer::Settings::default(), Some("tiny-skia"),
    )).expect("headless renderer");
    let mut cache = Cache::default();
    focus(&app, &mut renderer, &mut cache, app.focus_cmd_input());
    let task = app.dispatch_command("TABLEDIT");
    focus(&app, &mut renderer, &mut cache, task);
    let task = app.feed_command(crate::command::StepInput::Point(glam::DVec3::new(10.0, -5.0, 0.0)));
    focus(&app, &mut renderer, &mut cache, task);
    assert_eq!(app.table_cell_editor.as_ref().map(|c| c.handle), Some(handle));
    key(&mut app, &mut renderer, &mut cache,
        keyboard::Key::Named(keyboard::key::Named::ArrowLeft), keyboard::key::Code::ArrowLeft,
        None, keyboard::Modifiers::empty());
    key(&mut app, &mut renderer, &mut cache,
        keyboard::Key::Named(keyboard::key::Named::Backspace), keyboard::key::Code::Backspace,
        None, keyboard::Modifiers::empty());
    key(&mut app, &mut renderer, &mut cache,
        keyboard::Key::Character("X".into()), keyboard::key::Code::KeyX,
        Some("X"), keyboard::Modifiers::empty());
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "abXd");
    assert!(app.command_line.input.is_empty());
    let accel = if cfg!(target_os = "macos") { keyboard::Modifiers::LOGO } else { keyboard::Modifiers::CTRL };
    key(&mut app, &mut renderer, &mut cache,
        keyboard::Key::Character("z".into()), keyboard::key::Code::KeyZ, None, accel);
    assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "abd");
    key(&mut app, &mut renderer, &mut cache,
        keyboard::Key::Character("f".into()), keyboard::key::Code::KeyF, None, accel);
    assert!(app.field_dialog.is_some());
}
