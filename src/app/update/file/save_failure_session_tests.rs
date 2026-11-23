//! What a save that fails leaves in the session: the message, the edited
//! mark, the path of the drawing and its recovery copy.

use crate::app::{Message, OpenCADStudio, SaveContinuation, SaveOutcome, SavePurpose};
use iced::futures::StreamExt;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ocs-save-session-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch folder");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An app with one edited drawing saved at `plan.dxf`, and its recovery
/// copies in the `recovery` folder of `scratch`.
fn app_with_saved_drawing(scratch: &Scratch) -> (OpenCADStudio, PathBuf) {
    let mut app = OpenCADStudio::new_for_test();
    app.save_file_path = scratch.0.join("recovery").to_string_lossy().into_owned();
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    app.active_tab = app.tabs.len() - 1;
    let drawing = scratch.0.join("plan.dxf");
    let saved = save(&mut app, &drawing, SavePurpose::SaveAs);
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let _ = app.on_save_finished(saved);
    let i = app.active_tab;
    assert_eq!(app.tabs[i].current_path.as_deref(), Some(drawing.as_path()));
    app.tabs[i].dirty = true;
    (app, drawing)
}

/// Runs the save worker of `task` and returns its result, not yet given to
/// the app.
fn finished(task: iced::Task<Message>) -> SaveOutcome {
    let mut stream = iced_runtime::task::into_stream(task).expect("the save runs a task");
    while let Some(action) = iced::futures::executor::block_on(stream.next()) {
        if let iced_runtime::Action::Output(Message::SaveFinished(outcome)) = action {
            return outcome;
        }
    }
    panic!("the save gave no result")
}

fn save(app: &mut OpenCADStudio, path: &Path, purpose: SavePurpose) -> SaveOutcome {
    let i = app.active_tab;
    let version = app.tabs[i].scene.document.version;
    finished(app.queue_native_save(
        i,
        path.to_path_buf(),
        version,
        purpose,
        SaveContinuation::None,
        purpose != SavePurpose::Manual,
        false,
    ))
}

fn autosave(app: &mut OpenCADStudio) -> SaveOutcome {
    finished(app.on_autosave())
}

#[test]
fn a_failed_save_as_keeps_the_path_the_edited_mark_and_the_recovery_copy() {
    let scratch = Scratch::new("save-as");
    let (mut app, drawing) = app_with_saved_drawing(&scratch);
    let i = app.active_tab;
    let copy = app.autosave_target(i);
    let autosaved = autosave(&mut app);
    let _ = app.on_save_finished(autosaved);
    assert!(copy.is_file(), "the autosave wrote the recovery copy");
    let before = std::fs::read(&drawing).unwrap();

    let missing = scratch.0.join("no-such-folder").join("plan.dxf");
    let failed = save(&mut app, &missing, SavePurpose::SaveAs);
    assert!(failed.result.is_err());
    let _ = app.on_save_finished(failed);

    let error = app.command_line.last_error.as_deref().unwrap_or_default();
    assert!(error.starts_with("Save failed"), "{error}");
    assert!(app.tabs[i].dirty, "the drawing is still edited");
    assert_eq!(app.tabs[i].current_path.as_deref(), Some(drawing.as_path()));
    assert!(copy.is_file(), "the recovery copy stays");
    assert_eq!(std::fs::read(&drawing).unwrap(), before);

    // The session continues: the next save writes the drawing.
    let saved = save(&mut app, &drawing, SavePurpose::Manual);
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let _ = app.on_save_finished(saved);
    assert!(!app.tabs[i].dirty);
    assert!(!copy.exists(), "a saved drawing has no recovery copy");
}

#[test]
fn a_failed_autosave_is_reported_and_the_next_one_writes_the_copy() {
    let scratch = Scratch::new("autosave");
    let (mut app, drawing) = app_with_saved_drawing(&scratch);
    let i = app.active_tab;
    let before = std::fs::read(&drawing).unwrap();
    let folder = std::mem::take(&mut app.save_file_path);
    // The recovery folder is a file: no copy can be written.
    let blocked = scratch.0.join("blocked");
    std::fs::write(&blocked, b"not a folder").unwrap();
    app.save_file_path = blocked.to_string_lossy().into_owned();

    let failed = autosave(&mut app);
    assert!(failed.result.is_err());
    let _ = app.on_save_finished(failed);

    let error = app.command_line.last_error.as_deref().unwrap_or_default();
    assert!(error.starts_with("Save failed"), "{error}");
    assert!(app.tabs[i].dirty, "the drawing is still edited");
    assert_eq!(std::fs::read(&drawing).unwrap(), before);

    app.save_file_path = folder;
    let autosaved = autosave(&mut app);
    assert!(autosaved.result.is_ok(), "{:?}", autosaved.result);
    let _ = app.on_save_finished(autosaved);
    assert!(app.autosave_target(i).is_file());
}

#[test]
fn the_copy_of_an_ended_session_is_moved_aside_and_reported() {
    let scratch = Scratch::new("ended-session");
    let folder = scratch.0.join("recovery");
    std::fs::create_dir_all(&folder).unwrap();
    let mut app = OpenCADStudio::new_for_test();
    app.save_file_path = folder.to_string_lossy().into_owned();
    // The copy of the ended session is empty: the move does not read it.
    let ended = "OpenCADStudio_Drawing1_ended_3.sv$";
    let running = format!("OpenCADStudio_Drawing1_{}_3.sv$", super::autosave_session());
    let other = "notes.txt";
    std::fs::write(folder.join(ended), b"").unwrap();
    std::fs::write(folder.join(&running), b"copy").unwrap();
    std::fs::write(folder.join(other), b"notes").unwrap();

    app.recover_ended_session_autosaves();

    let kept = "OpenCADStudio_Drawing1_ended_3.ocs-recovered.sv$";
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut expected = vec![kept.to_string(), running, other.to_string()];
    expected.sort();
    assert_eq!(names, expected);
    assert!(
        app.command_line.history_plain_text().contains(kept),
        "the command line names the copy"
    );

    // A second start leaves the moved copy as it is.
    app.recover_ended_session_autosaves();
    assert!(folder.join(kept).is_file());
}
