use super::*;

use crate::app::{Message, OpenCADStudio, SaveContinuation, SavePurpose};
use iced::futures::StreamExt;
use iced_runtime::window::Action as WindowAction;
use iced_runtime::Action;

/// A save driven with the window minimized (the MCP bridge saving while
/// the window sits in the taskbar) skipped the thumbnail check and read
/// a 0x0 window back: iced's renderer panics on that with "buffer slices
/// can not be empty" and took the program down. The save now asks the
/// window's size first and writes the file without a new preview.
#[test]
fn a_minimized_window_saves_without_reading_the_screen_back() {
    let dir = std::env::temp_dir().join(format!(
        "ocs-minimized-save-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("minimized.dxf");

    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    app.active_tab = app.tabs.len() - 1;
    app.main_window = Some(iced::window::Id::unique());
    // A drawn viewport from before the window was minimized.
    crate::ui::wrap_bar::set_drawn_bounds(
        crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID,
        iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(800.0, 600.0)),
    );
    app.thumbnail_capture_clean = true;
    let task = app.queue_native_save(
        app.active_tab,
        path.clone(),
        document::DxfVersion::AC1027,
        SavePurpose::Manual,
        SaveContinuation::None,
        true,
        false,
    );

    let mut asked_size = false;
    let mut outputs = Vec::new();
    let mut stream = iced_runtime::task::into_stream(task).expect("the save runs a task");
    while let Some(action) = iced::futures::executor::block_on(stream.next()) {
        match action {
            Action::Window(WindowAction::GetSize(_, reply)) => {
                asked_size = true;
                let _ = reply.send(iced::Size::ZERO);
            }
            Action::Window(WindowAction::Screenshot(..)) => {
                panic!("a minimized window was read back for the thumbnail")
            }
            Action::Output(message) => outputs.push(message),
            _ => {}
        }
    }

    assert!(
        asked_size,
        "the save asks the window's size before capturing"
    );
    assert!(outputs
        .iter()
        .any(|message| matches!(message, Message::ThumbnailCaptureFinished)));
    let saved = outputs.into_iter().find_map(|message| match message {
        Message::SaveFinished(outcome) => Some(outcome),
        _ => None,
    });
    let saved = saved.expect("the save still runs");
    assert!(
        saved.result.is_ok(),
        "save failed: {:?}",
        saved.result.err()
    );
    assert!(saved.refreshed_preview.is_none());
    assert!(path.is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
