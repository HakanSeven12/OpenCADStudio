//! What a save that fails leaves in the session: the message, the edited
//! mark, the path of the drawing and its recovery copy. A save that fails
//! from the close prompt or at quit keeps the drawing open.

use crate::app::{
    Message, ModalKind, OpenCADStudio, PendingClose, SaveContinuation, SaveOutcome, SavePurpose,
};
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

/// An app with one edited drawing that has no file yet.
fn app_with_untitled_drawing(scratch: &Scratch) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.save_file_path = scratch.0.join("recovery").to_string_lossy().into_owned();
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    app.active_tab = app.tabs.len() - 1;
    app.tabs[app.active_tab].dirty = true;
    app
}

/// The answer of the system save dialog: the save goes to `folder`, with
/// the name and format that the app suggested.
fn save_dialog_answer(app: &mut OpenCADStudio, folder: &Path) -> SaveOutcome {
    let path = folder.join(&app.save_dialog_filename);
    finished(app.on_save_dialog_path_picked(Some(path)))
}

fn has_tab(app: &OpenCADStudio, id: u64) -> bool {
    app.tabs.iter().any(|tab| tab.id == id)
}

/// Runs `step` until it gives a value, for at most 2 s. A child process
/// that another test starts holds a copy of the open files of the test
/// process for a moment, so a released lock can stay taken for some
/// milliseconds (up to 11 ms in the runs tried).
fn when_the_lock_is_free<T>(mut step: impl FnMut() -> Option<T>) -> T {
    let started = std::time::Instant::now();
    loop {
        if let Some(value) = step() {
            return value;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the lock stays taken"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_failed_save_from_the_close_prompt_keeps_the_drawing_open_and_asks_again() {
    let scratch = Scratch::new("close-prompt");
    let mut app = app_with_untitled_drawing(&scratch);
    let i = app.active_tab;
    let id = app.tabs[i].id;
    let _ = app.update(Message::TabClose(id));
    assert_eq!(app.active_modal, Some(ModalKind::Unsaved));

    let _ = app.update(Message::UnsavedDialogSave);
    assert_ne!(app.active_modal, Some(ModalKind::Unsaved));
    let failed = save_dialog_answer(&mut app, &scratch.0.join("no-such-folder"));
    assert!(failed.result.is_err());
    let _ = app.on_save_finished(failed);

    let error = app.command_line.last_error.as_deref().unwrap_or_default();
    assert!(error.starts_with("Save failed"), "{error}");
    assert!(has_tab(&app, id), "the drawing stays open");
    assert!(app.tabs[i].dirty, "the drawing is still edited");
    assert_eq!(app.tabs[i].current_path, None);
    assert!(matches!(app.pending_close, Some(PendingClose::Tab(tab)) if tab == id));
    assert_eq!(
        app.active_modal,
        Some(ModalKind::Unsaved),
        "the prompt asks again"
    );

    // The second answer saves the drawing and closes it.
    let _ = app.update(Message::UnsavedDialogSave);
    let saved = save_dialog_answer(&mut app, &scratch.0);
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let drawing = saved.path.clone();
    let _ = app.on_save_finished(saved);
    assert!(drawing.is_file());
    assert!(!has_tab(&app, id), "the saved drawing closes");
    assert!(app.pending_close.is_none());
}

#[test]
fn a_failed_save_at_quit_keeps_the_app_open_and_asks_again() {
    let scratch = Scratch::new("quit");
    let mut app = app_with_untitled_drawing(&scratch);
    let i = app.active_tab;
    let _ = app.request_quit_with_block_edits();
    assert_eq!(app.active_modal, Some(ModalKind::Unsaved));

    let _ = app.update(Message::UnsavedDialogSave);
    let failed = save_dialog_answer(&mut app, &scratch.0.join("no-such-folder"));
    assert!(failed.result.is_err());
    let _ = app.on_save_finished(failed);

    let error = app.command_line.last_error.as_deref().unwrap_or_default();
    assert!(error.starts_with("Save failed"), "{error}");
    assert!(!app.exit_requested, "the app does not quit");
    assert!(app.tabs[i].dirty, "the drawing is still edited");
    assert!(matches!(app.pending_close, Some(PendingClose::Quit)));
    assert_eq!(
        app.active_modal,
        Some(ModalKind::Unsaved),
        "the prompt asks again"
    );

    // The second answer saves the drawing, and the app quits.
    let _ = app.update(Message::UnsavedDialogSave);
    let saved = save_dialog_answer(&mut app, &scratch.0);
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let drawing = saved.path.clone();
    let _ = app.on_save_finished(saved);
    assert!(drawing.is_file());
    assert!(app.exit_requested);
}

#[test]
fn a_drawing_in_use_at_close_stays_open_until_its_save_succeeds() {
    let scratch = Scratch::new("in-use");
    let (mut app, drawing) = app_with_saved_drawing(&scratch);
    let i = app.active_tab;
    let id = app.tabs[i].id;
    let before = std::fs::read(&drawing).unwrap();
    // Another editor holds the lock of the drawing, as at an open that
    // finds the lock taken.
    app.tabs[i].edit_lease = None;
    let other_editor =
        when_the_lock_is_free(|| crate::io::edit_lock::EditLease::acquire(&drawing).ok());
    app.tabs[i].edit_lock_conflict = true;

    let _ = app.update(Message::TabClose(id));
    let _ = app.update(Message::UnsavedDialogSave);
    assert_eq!(app.active_modal, Some(ModalKind::FileInUse));
    let error = app.command_line.last_error.as_deref().unwrap_or_default();
    assert!(error.starts_with("Unable to save"), "{error}");

    // Cancel: the drawing stays open and edited, and its file does not
    // change.
    let _ = app.update(Message::SaveFileInUseCancel);
    assert_eq!(app.active_modal, None);
    assert!(has_tab(&app, id));
    assert!(app.tabs[i].dirty);
    assert_eq!(std::fs::read(&drawing).unwrap(), before);

    // The next close asks again. The other editor closes the drawing, and
    // Retry saves it and closes the tab.
    let _ = app.update(Message::TabClose(id));
    let _ = app.update(Message::UnsavedDialogSave);
    assert_eq!(app.active_modal, Some(ModalKind::FileInUse));
    assert!(has_tab(&app, id));
    drop(other_editor);
    let saved = when_the_lock_is_free(|| {
        let retry = app.update(Message::SaveFileInUseRetry);
        (app.active_modal != Some(ModalKind::FileInUse)).then(|| finished(retry))
    });
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let _ = app.on_save_finished(saved);
    assert!(!has_tab(&app, id), "the saved drawing closes");
    assert_ne!(std::fs::read(&drawing).unwrap(), before);
}

#[test]
fn a_drawing_changed_outside_at_quit_is_kept_until_the_user_overwrites_it() {
    let scratch = Scratch::new("changed-outside");
    let (mut app, drawing) = app_with_saved_drawing(&scratch);
    let i = app.active_tab;
    // Windows refuses a write to the drawing while the app holds it open.
    #[cfg(target_os = "windows")]
    let _ = app.tabs[i].edit_lease.take();
    let outside = b"changed by another program";
    std::fs::write(&drawing, outside).unwrap();

    let _ = app.request_quit_with_block_edits();
    let failed = finished(app.update(Message::UnsavedDialogSave));
    assert!(failed.result.is_err());
    let _ = app.on_save_finished(failed);
    assert_eq!(app.active_modal, Some(ModalKind::ExternalChange));
    let error = app.command_line.last_error.as_deref().unwrap_or_default();
    assert!(error.starts_with("Save stopped"), "{error}");

    // Cancel: the app stays open, and the other program's file stays.
    let _ = app.update(Message::ExternalChangeCancel);
    assert_eq!(app.active_modal, None);
    assert!(!app.exit_requested, "the app does not quit");
    assert!(app.tabs[i].dirty);
    assert_eq!(std::fs::read(&drawing).unwrap(), outside);

    // The next quit asks again. Overwrite saves the drawing, and the app
    // quits.
    let _ = app.request_quit_with_block_edits();
    let failed = finished(app.update(Message::UnsavedDialogSave));
    let _ = app.on_save_finished(failed);
    assert_eq!(app.active_modal, Some(ModalKind::ExternalChange));
    assert!(!app.exit_requested);
    let saved = finished(app.update(Message::ExternalChangeOverwrite));
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let _ = app.on_save_finished(saved);
    assert!(app.exit_requested);
    assert_ne!(std::fs::read(&drawing).unwrap(), outside);
}

/// Starts an autosave of every edited drawing and returns its task, not
/// yet run: the autosave runs until the task is given to `finished`.
fn autosave_running(app: &mut OpenCADStudio) -> iced::Task<Message> {
    let task = app.on_autosave();
    assert!(!app.active_save_jobs.is_empty(), "the autosave runs");
    task
}

/// Ends the autosave of `task` and returns the save that waited for it.
fn waiting_save(app: &mut OpenCADStudio, autosave: iced::Task<Message>) -> SaveOutcome {
    let autosaved = finished(autosave);
    assert_eq!(autosaved.purpose, SavePurpose::Autosave);
    finished(app.on_save_finished(autosaved))
}

#[test]
fn a_save_during_an_autosave_writes_the_drawing_after_it() {
    let scratch = Scratch::new("save-during-autosave");
    let (mut app, drawing) = app_with_saved_drawing(&scratch);
    let i = app.active_tab;
    let autosave = autosave_running(&mut app);

    let version = app.tabs[i].scene.document.version;
    let refused = app.queue_native_save(
        i,
        drawing.clone(),
        version,
        SavePurpose::Manual,
        SaveContinuation::None,
        false,
        false,
    );
    assert!(iced_runtime::task::into_stream(refused).is_none());

    let saved = waiting_save(&mut app, autosave);
    assert_eq!(saved.purpose, SavePurpose::Manual);
    assert_eq!(saved.path, drawing);
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let _ = app.on_save_finished(saved);
    assert!(!app.tabs[i].dirty);
}

#[test]
fn a_close_prompt_save_during_an_autosave_closes_the_drawing_after_it() {
    let scratch = Scratch::new("close-during-autosave");
    let (mut app, drawing) = app_with_saved_drawing(&scratch);
    let id = app.tabs[app.active_tab].id;
    let autosave = autosave_running(&mut app);
    let _ = app.update(Message::TabClose(id));
    assert_eq!(app.active_modal, Some(ModalKind::Unsaved));

    let _ = app.update(Message::UnsavedDialogSave);
    assert_ne!(app.active_modal, Some(ModalKind::Unsaved));

    let saved = waiting_save(&mut app, autosave);
    assert_eq!(saved.path, drawing);
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let _ = app.on_save_finished(saved);
    assert!(!has_tab(&app, id), "the saved drawing closes");
}

#[test]
fn a_save_at_quit_during_an_autosave_quits_after_it() {
    let scratch = Scratch::new("quit-during-autosave");
    let (mut app, drawing) = app_with_saved_drawing(&scratch);
    let autosave = autosave_running(&mut app);
    let _ = app.request_quit_with_block_edits();
    assert_eq!(app.active_modal, Some(ModalKind::Unsaved));

    let _ = app.update(Message::UnsavedDialogSave);
    assert_ne!(app.active_modal, Some(ModalKind::Unsaved));
    assert!(!app.exit_requested);

    let saved = waiting_save(&mut app, autosave);
    assert_eq!(saved.path, drawing);
    assert!(saved.result.is_ok(), "{:?}", saved.result);
    let _ = app.on_save_finished(saved);
    assert!(app.exit_requested);
}

/// A save that waits for a save of the same drawing has nothing to add:
/// it is refused, as before.
#[test]
fn a_save_during_a_save_of_the_same_drawing_is_refused() {
    let scratch = Scratch::new("save-during-save");
    let (mut app, drawing) = app_with_saved_drawing(&scratch);
    let i = app.active_tab;
    let version = app.tabs[i].scene.document.version;
    let save = |app: &mut OpenCADStudio| {
        app.queue_native_save(
            i,
            drawing.clone(),
            version,
            SavePurpose::Manual,
            SaveContinuation::None,
            false,
            false,
        )
    };
    let first = save(&mut app);
    let refused = save(&mut app);
    assert!(iced_runtime::task::into_stream(refused).is_none());
    let last = app
        .command_line
        .history
        .last()
        .map(|entry| entry.text.as_str());
    assert!(
        last.is_some_and(|text| text.contains("Save already running for this drawing.")),
        "{last:?}"
    );
    assert!(finished(first).result.is_ok());
}
