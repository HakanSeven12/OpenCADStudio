//! MTEXT Import Text reads its file on a worker.

use super::*;
use glam::DVec3;

/// An app with the MTEXT editor open on an empty new text.
fn editing() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    app.tabs[1].scene.selection.borrow_mut().view.vp_size = (1280.0, 650.0);
    app.open_mtext_editor(DVec3::ZERO, None, "", 2.5, None);
    app
}

fn raw(app: &OpenCADStudio) -> String {
    app.mtext_editor.as_ref().unwrap().content.text()
}

/// A text file with `text` in a new folder; its path.
fn text_file(stem: &str, text: &str) -> String {
    let dir = std::env::temp_dir().join(format!("ocs-mtext-import-{stem}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("notes.txt");
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

/// The file is read on a worker: the text goes in when the read ends.
#[test]
fn the_import_reads_its_file_on_a_worker() {
    let mut app = editing();
    let path = text_file("worker", "Hello");
    let task = app.update(Message::MTextImport(path));
    assert_eq!(raw(&app), "", "nothing goes in during the read");
    let message = OpenCADStudio::first_task_message(task);
    assert!(
        matches!(&message, Message::MTextImportRead(_, Ok(source)) if source == "Hello"),
        "{message:?}"
    );
    let _ = app.update(message);
    assert!(raw(&app).contains("Hello"), "{}", raw(&app));
}

/// A read that ends after its editor closed does not go into the editor
/// that is open then.
#[test]
fn a_read_for_a_closed_editor_is_dropped() {
    let mut app = editing();
    let path = text_file("closed", "Hello");
    let task = app.update(Message::MTextImport(path));
    app.mtext_editor = None;
    app.open_mtext_editor(DVec3::ZERO, None, "", 2.5, None);
    let _ = app.update(OpenCADStudio::first_task_message(task));
    assert_eq!(raw(&app), "", "the read was for the editor that closed");
}

/// A file that cannot be read is reported, as before; nothing goes in.
#[test]
fn a_file_that_cannot_be_read_is_reported() {
    let mut app = editing();
    let errors = app.command_line.error_revision;
    let task = app.update(Message::MTextImport("/no/such/folder/notes.txt".into()));
    let _ = app.update(OpenCADStudio::first_task_message(task));
    assert_eq!(raw(&app), "");
    assert!(
        app.command_line.error_revision > errors,
        "the error is shown"
    );
}

/// A reader panic is caught on the worker: the editor stays, unchanged.
#[test]
fn an_import_reader_panic_keeps_the_editor() {
    let mut app = editing();
    let task = app.mtext_import_with("notes.txt".into(), |_| panic!("a reader bug"));
    let message = OpenCADStudio::first_task_message(task);
    assert!(
        matches!(message, Message::BackgroundTaskFailed { .. }),
        "{message:?}"
    );
    let _ = app.update(message);
    assert!(app.mtext_editor.is_some(), "the editor stays");
    assert_eq!(raw(&app), "");
}
