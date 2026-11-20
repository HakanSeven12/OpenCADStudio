use super::*;

use super::background_task;
use crate::app::{Message, OpenCADStudio};
use iced::futures::StreamExt;
use iced::Task;

/// Pull the single message a finished background task produces, the same
/// way the headless automation driver consumes task streams.
fn next_message(task: Task<Message>) -> Message {
    let mut stream =
        iced_runtime::task::into_stream(task).expect("background_task yields a stream");
    match iced::futures::executor::block_on(stream.next()) {
        Some(iced_runtime::Action::Output(message)) => message,
        other => panic!("expected task output, got {other:?}"),
    }
}

#[test]
fn worker_panic_becomes_a_failure_message() {
    let task = background_task(
        "STL export",
        || panic!("worker exploded"),
        |()| Message::Noop,
    );
    match next_message(task) {
        Message::BackgroundTaskFailed { context, detail } => {
            assert_eq!(context, "STL export");
            assert!(
                detail.contains("worker exploded"),
                "detail should carry the panic message, got: {detail}"
            );
        }
        other => panic!("expected BackgroundTaskFailed, got {other:?}"),
    }
}

#[test]
fn worker_panic_is_reported_on_the_command_line() {
    let mut app = OpenCADStudio::new_for_test();
    let task = background_task(
        "STL export",
        || panic!("worker exploded"),
        |()| Message::Noop,
    );
    app.drive_headless_task(task)
        .expect("failed worker task drives headlessly");
    let error = app
        .command_line
        .last_error
        .as_deref()
        .expect("worker panic reported as an error");
    assert!(error.contains("STL export"), "error = {error}");
    assert!(error.contains("worker exploded"), "error = {error}");
}
