use super::{tests::request, OpenCADStudio};
use crate::app::{ModalKind, SaveContinuation, SavePurpose};
use crate::io::edit_lock::FileFingerprint;
use serde_json::json;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ocs-control-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_drawing(path: &Path, windowed: bool) -> OpenCADStudio {
    {
        let mut seed = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut seed, json!({"op":"new"}))["ok"], true);
        assert_eq!(
            request(&mut seed, json!({"op":"run","cmd":"LINE 0,0 10,0"}))["ok"],
            true
        );
        let saved = request(&mut seed, json!({"op":"save","path":path}));
        assert_eq!(saved["ok"], true, "{saved}");
    }
    reopen_drawing(path, windowed)
}

fn reopen_drawing(path: &Path, windowed: bool) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.backup_on_save = false;
    let opened = request(&mut app, json!({"op":"open","path":path}));
    assert_eq!(opened["ok"], true, "{opened}");
    let tab = &app.tabs[app.active_tab];
    assert!(!tab.edit_lock_conflict);
    assert_eq!(
        tab.disk_fingerprint,
        Some(FileFingerprint::capture(path).unwrap())
    );
    if windowed {
        // Select the real asynchronous GUI save branch. No drawn viewport
        // means no screenshot runtime is needed to drive its tasks here.
        assert!(
            crate::ui::wrap_bar::dropdown_bounds(crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID)
                .is_none()
        );
        app.main_window = Some(iced::window::Id::unique());
    }
    app
}

fn edit(app: &mut OpenCADStudio) {
    let edited = request(app, json!({"op":"run","cmd":"LINE 0,5 10,5"}));
    assert_eq!(edited["ok"], true, "{edited}");
    assert!(app.tabs[app.active_tab].dirty);
}

#[test]
fn control_save_current_path_preserves_the_opening_fingerprint() {
    for windowed in [false, true] {
        let scratch = Scratch::new();
        let path = scratch.0.join("current.dxf");
        let mut app = open_drawing(&path, windowed);
        edit(&mut app);
        let i = app.active_tab;
        let original = app.tabs[i].disk_fingerprint.clone();
        // Windows denies an in-place write while its drawing handle is
        // leased. Keep F0, but release that handle to exercise the fallback.
        #[cfg(target_os = "windows")]
        let _ = app.tabs[i].edit_lease.take();
        let external = b"external change completed before the control request";
        std::fs::write(&path, external).unwrap();
        assert_ne!(Some(FileFingerprint::capture(&path).unwrap()), original);

        // Even explicit overwrite acknowledges only a different destination;
        // it must not waive the current drawing's external-change guard.
        let saved = request(&mut app, json!({"op":"save","path":path,"overwrite":true}));
        assert_eq!(saved["ok"], false, "{saved}");
        if windowed {
            assert_eq!(app.active_modal, Some(ModalKind::ExternalChange));
            assert_eq!(app.pending_external_change.as_ref().unwrap().path, path);
        } else {
            assert_eq!(saved["code"], "save_failed", "{saved}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), external);
        assert_eq!(app.tabs[i].disk_fingerprint, original);
        assert_eq!(app.tabs[i].current_path.as_ref(), Some(&path));
        assert!(app.tabs[i].dirty);
    }
}

#[test]
fn control_save_current_path_respects_another_editors_lock() {
    for windowed in [false, true] {
        let scratch = Scratch::new();
        let path = scratch.0.join("locked.dxf");
        let owner = open_drawing(&path, false);
        let original = std::fs::read(&path).unwrap();
        let mut app = OpenCADStudio::new_for_test();
        let opened = request(&mut app, json!({"op":"open","path":path}));
        assert_eq!(opened["ok"], true, "{opened}");
        let i = app.active_tab;
        assert!(app.tabs[i].edit_lock_conflict);
        if windowed {
            app.main_window = Some(iced::window::Id::unique());
        }
        edit(&mut app);
        let saved = request(&mut app, json!({"op":"save","path":path,"overwrite":true}));
        assert_eq!(saved["ok"], false, "{saved}");
        if windowed {
            assert_eq!(app.active_modal, Some(ModalKind::FileInUse));
            assert_eq!(app.pending_save_failure.as_ref().unwrap().path, path);
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(app.tabs[i].dirty);

        // A lock conflict on A must still allow the edited copy to escape
        // to a new destination B, without touching the owner's A.
        app.active_modal = None;
        app.pending_save_failure = None;
        let copy = scratch.0.join("copy.dxf");
        let saved = request(&mut app, json!({"op":"save","path":copy}));
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(app.tabs[i].current_path.as_ref(), Some(&copy));
        assert!(!app.tabs[i].edit_lock_conflict);
        assert!(!app.tabs[i].dirty);
        assert!(copy.is_file());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        drop(owner);
    }
}

#[test]
fn control_save_different_existing_path_requires_explicit_overwrite() {
    for windowed in [false, true] {
        let scratch = Scratch::new();
        let path = scratch.0.join("current.dxf");
        let destination = scratch.0.join("other.dxf");
        let mut app = open_drawing(&path, windowed);
        let current_bytes = std::fs::read(&path).unwrap();
        edit(&mut app);
        let other_bytes = b"another drawing must survive an ordinary save";
        std::fs::write(&destination, other_bytes).unwrap();
        for overwrite in [None, Some(json!(false)), Some(json!("true"))] {
            let mut req = json!({"op":"save","path":destination});
            if let Some(overwrite) = overwrite {
                req["overwrite"] = overwrite;
            }
            let refused = request(&mut app, req);
            assert_eq!(refused["code"], "destination_exists", "{refused}");
            assert_eq!(std::fs::read(&destination).unwrap(), other_bytes);
            assert_eq!(app.tabs[app.active_tab].current_path.as_ref(), Some(&path));
            assert!(app.tabs[app.active_tab].dirty);
        }
        let saved = request(
            &mut app,
            json!({"op":"save","path":destination,"overwrite":true}),
        );
        assert_eq!(saved["ok"], true, "{saved}");
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.current_path.as_ref(), Some(&destination));
        assert!(!tab.dirty);
        assert_eq!(
            tab.disk_fingerprint,
            Some(FileFingerprint::capture(&destination).unwrap())
        );
        assert_ne!(std::fs::read(&destination).unwrap(), other_bytes);
        assert_eq!(std::fs::read(&path).unwrap(), current_bytes);
        let loaded = crate::io::load_file(&destination).unwrap();
        assert_eq!(
            loaded.entities().count(),
            tab.scene.document.entities().count()
        );
    }
}

#[test]
fn control_save_after_native_save_as_cannot_replace_the_previous_path() {
    let scratch = Scratch::new();
    let path = scratch.0.join("before.dxf");
    let destination = scratch.0.join("after.dxf");
    let mut app = open_drawing(&path, true);
    let i = app.active_tab;
    let tab_id = app.tabs[i].id;
    app.prepare_native_save(i);
    let task = app.queue_native_save(
        i,
        destination.clone(),
        app.tabs[i].scene.document.version,
        SavePurpose::SaveAs,
        SaveContinuation::None,
        true,
        true,
    );
    app.drive_headless_task(task).unwrap();
    assert_eq!(app.tabs[i].current_path.as_ref(), Some(&destination));
    assert_eq!(app.tabs[i].id, tab_id);
    let before = std::fs::read(&path).unwrap();
    let after = std::fs::read(&destination).unwrap();
    edit(&mut app);

    let saved = request(&mut app, json!({"op":"save","path":path}));
    assert_eq!(saved["code"], "destination_exists", "{saved}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read(&destination).unwrap(), after);
    assert_eq!(app.tabs[i].current_path.as_ref(), Some(&destination));
    assert!(app.tabs[i].dirty);
}

#[test]
fn control_save_new_path_refuses_a_destination_created_after_the_request() {
    let scratch = Scratch::new();
    let path = scratch.0.join("current.dxf");
    let destination = scratch.0.join("new.dxf");
    let mut app = open_drawing(&path, true);
    edit(&mut app);
    let i = app.active_tab;
    let current_bytes = std::fs::read(&path).unwrap();
    let (accepted, task) = app.control_request(json!({
        "protocol":1,
        "op":"save",
        "path":destination,
        "request_id":"create-only-race",
        "document_id":app.tabs[i].id,
        "revision":app.tabs[i].edit_revision,
        "client_id":"test",
    }));
    assert_eq!(accepted["status"], "accepted", "{accepted}");
    assert!(!destination.exists());
    let external = b"a different application created this before the worker ran";
    std::fs::write(&destination, external).unwrap();

    app.drive_headless_task(task).unwrap();
    let saved = app
        .control_request(json!({"op":"operation","request_id":"create-only-race"}))
        .0;
    assert_eq!(saved["ok"], false, "{saved}");
    assert_eq!(app.active_modal, Some(ModalKind::ExternalChange));
    assert_eq!(std::fs::read(&destination).unwrap(), external);
    assert_eq!(std::fs::read(&path).unwrap(), current_bytes);
    assert_eq!(app.tabs[i].current_path.as_ref(), Some(&path));
    assert!(app.tabs[i].dirty);
    assert!(!destination.with_extension("bak").exists());
}

#[test]
fn control_save_current_and_new_paths_still_succeed() {
    for windowed in [false, true] {
        let scratch = Scratch::new();
        let path = scratch.0.join("current.dxf");
        let mut app = open_drawing(&path, windowed);
        for explicit_path in [true, false] {
            edit(&mut app);
            let mut req = json!({"op":"save"});
            if explicit_path {
                // A different spelling of the same path is still current.
                req["path"] = json!(scratch.0.join(".").join("current.dxf"));
            }
            let saved = request(&mut app, req);
            assert_eq!(saved["ok"], true, "{saved}");
            assert!(!app.tabs[app.active_tab].dirty);
        }
        edit(&mut app);
        let destination = scratch.0.join("new.dxf");
        let saved = request(&mut app, json!({"op":"save","path":destination}));
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(
            app.tabs[app.active_tab].current_path.as_ref(),
            Some(&destination)
        );
        assert!(destination.is_file());
    }
}

#[cfg(unix)]
#[test]
fn control_save_directory_alias_keeps_the_external_change_guard() {
    let scratch = Scratch::new();
    let real = scratch.0.join("real");
    let alias = scratch.0.join("alias");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let path = real.join("current.dxf");
    let mut app = open_drawing(&path, true);
    edit(&mut app);
    let original = app.tabs[app.active_tab].disk_fingerprint.clone();
    let external = b"external bytes through the real directory";
    std::fs::write(&path, external).unwrap();
    let saved = request(
        &mut app,
        json!({"op":"save","path":alias.join("current.dxf")}),
    );
    assert_eq!(saved["ok"], false, "{saved}");
    assert_eq!(app.active_modal, Some(ModalKind::ExternalChange));
    assert_eq!(std::fs::read(&path).unwrap(), external);
    assert_eq!(app.tabs[app.active_tab].disk_fingerprint, original);
}

#[cfg(unix)]
#[test]
fn control_save_file_alias_preserves_the_target_and_symlink() {
    for windowed in [false, true] {
        for open_alias in [false, true] {
            for save_alias in [false, true] {
                let scratch = Scratch::new();
                let path = scratch.0.join("current.dxf");
                drop(open_drawing(&path, false));
                let alias = scratch.0.join("alias.dxf");
                std::os::unix::fs::symlink(&path, &alias).unwrap();
                let source = if open_alias { &alias } else { &path };
                let destination = if save_alias { &alias } else { &path };
                let mut app = reopen_drawing(source, windowed);
                let original = std::fs::read(&path).unwrap();
                edit(&mut app);
                let saved = request(&mut app, json!({"op":"save","path":destination}));
                assert_eq!(saved["ok"], true, "{saved}");
                assert!(alias.is_symlink());
                assert_ne!(std::fs::read(&path).unwrap(), original);
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    std::fs::read(&alias).unwrap()
                );
                assert!(!app.tabs[app.active_tab].dirty);
                let count = app.tabs[app.active_tab].scene.document.entities().count();
                drop(app);
                let reopened = reopen_drawing(source, false);
                assert_eq!(
                    reopened.tabs[reopened.active_tab]
                        .scene
                        .document
                        .entities()
                        .count(),
                    count
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn control_save_missing_current_file_through_directory_alias_keeps_fingerprint() {
    for windowed in [false, true] {
        for release_lease in [false, true] {
            let scratch = Scratch::new();
            let real = scratch.0.join("real");
            let alias = scratch.0.join("alias");
            std::fs::create_dir(&real).unwrap();
            std::os::unix::fs::symlink(&real, &alias).unwrap();
            let path = real.join("current.dxf");
            let mut app = open_drawing(&path, windowed);
            edit(&mut app);
            let i = app.active_tab;
            let original = app.tabs[i].disk_fingerprint.clone();
            if release_lease {
                let _ = app.tabs[i].edit_lease.take();
            }
            std::fs::remove_file(&path).unwrap();
            let saved = request(
                &mut app,
                json!({"op":"save","path":alias.join("current.dxf"),"overwrite":true}),
            );
            assert_eq!(saved["ok"], false, "{saved}");
            if windowed {
                assert_eq!(app.active_modal, Some(ModalKind::ExternalChange));
            }
            assert!(!path.exists());
            assert_eq!(app.tabs[i].disk_fingerprint, original);
            assert_eq!(app.tabs[i].current_path.as_ref(), Some(&path));
            assert!(app.tabs[i].dirty);
        }
    }
}

#[cfg(unix)]
fn add_relative_xref(app: &mut OpenCADStudio, saved: &str) {
    let doc = &mut app.tabs[app.active_tab].scene.document;
    let mut record = document::tables::BlockRecord::new("SAVE_ALIAS_REF");
    record.flags.is_xref = true;
    record.xref_path = saved.to_string();
    record.handle = doc.allocate_handle();
    doc.block_records.add(record).unwrap();
    crate::io::xref::ensure_block_entities(doc, "SAVE_ALIAS_REF");
    edit(app);
}

#[cfg(unix)]
fn assert_relative_xref(app: &OpenCADStudio, saved: &str, expected: &Path) {
    let tab = &app.tabs[app.active_tab];
    let record = tab
        .scene
        .document
        .block_records
        .get("SAVE_ALIAS_REF")
        .unwrap();
    assert_eq!(record.xref_path, saved);
    let base = tab.current_path.as_ref().unwrap().parent().unwrap();
    let resolved = crate::io::xref::resolve_path(&record.xref_path, base).unwrap();
    assert_eq!(
        std::fs::canonicalize(resolved).unwrap(),
        std::fs::canonicalize(expected).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn control_save_directory_alias_keeps_parent_relative_xref_base() {
    for windowed in [false, true] {
        for save_alias in [false, true] {
            let scratch = Scratch::new();
            let project = scratch.0.join("data/project");
            let shared = scratch.0.join("data/shared");
            let links = scratch.0.join("links");
            std::fs::create_dir_all(&project).unwrap();
            std::fs::create_dir_all(&shared).unwrap();
            std::fs::create_dir(&links).unwrap();
            std::os::unix::fs::symlink(&project, links.join("project")).unwrap();
            let path = project.join("current.dxf");
            let alias = links.join("project/current.dxf");
            let reference = shared.join("ref.dxf");
            drop(open_drawing(&path, false));
            drop(open_drawing(&reference, false));
            let mut app = reopen_drawing(&alias, windowed);
            add_relative_xref(&mut app, "../shared/ref.dxf");
            assert_relative_xref(&app, "../shared/ref.dxf", &reference);
            let destination = if save_alias { &alias } else { &path };
            let saved = request(&mut app, json!({"op":"save","path":destination}));
            assert_eq!(saved["ok"], true, "{saved}");
            assert_eq!(app.tabs[app.active_tab].current_path.as_ref(), Some(&alias));
            assert_relative_xref(&app, "../shared/ref.dxf", &reference);
            drop(app);
            let reopened = reopen_drawing(&alias, false);
            assert_relative_xref(&reopened, "../shared/ref.dxf", &reference);
        }
    }
}

#[cfg(unix)]
#[test]
fn control_save_file_alias_keeps_cross_directory_xref_base() {
    for windowed in [false, true] {
        for open_alias in [false, true] {
            for save_alias in [false, true] {
                let scratch = Scratch::new();
                let real = scratch.0.join("real");
                let links = scratch.0.join("links/sub");
                std::fs::create_dir(&real).unwrap();
                std::fs::create_dir_all(&links).unwrap();
                let path = real.join("current.dxf");
                let alias = links.join("current.dxf");
                drop(open_drawing(&path, false));
                std::os::unix::fs::symlink(&path, &alias).unwrap();
                let source = if open_alias { &alias } else { &path };
                let reference_dir = source.parent().unwrap().join("x");
                std::fs::create_dir(&reference_dir).unwrap();
                let reference = reference_dir.join("ref.dxf");
                drop(open_drawing(&reference, false));
                let mut app = reopen_drawing(source, windowed);
                add_relative_xref(&mut app, "x/ref.dxf");
                let destination = if save_alias { &alias } else { &path };
                let saved = request(&mut app, json!({"op":"save","path":destination}));
                assert_eq!(saved["ok"], true, "{saved}");
                assert!(alias.is_symlink());
                assert_eq!(app.tabs[app.active_tab].current_path.as_ref(), Some(source));
                assert_relative_xref(&app, "x/ref.dxf", &reference);
                drop(app);
                let reopened = reopen_drawing(source, false);
                assert_relative_xref(&reopened, "x/ref.dxf", &reference);
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn control_save_file_alias_refuses_a_different_physical_extension() {
    for windowed in [false, true] {
        for extension in ["dwg", "bin"] {
            let scratch = Scratch::new();
            let seed = scratch.0.join("seed.dxf");
            drop(open_drawing(&seed, false));
            let original = std::fs::read(&seed).unwrap();
            let physical = scratch.0.join(format!("drawing.{extension}"));
            std::fs::write(&physical, &original).unwrap();
            let alias = scratch.0.join("drawing.dxf");
            std::os::unix::fs::symlink(&physical, &alias).unwrap();
            let mut app = reopen_drawing(&alias, windowed);
            edit(&mut app);
            let saved = request(&mut app, json!({"op":"save","path":alias}));
            assert_eq!(saved["ok"], false, "{saved}");
            assert!(alias.is_symlink());
            assert_eq!(std::fs::read(&physical).unwrap(), original);
            assert_eq!(app.tabs[app.active_tab].current_path.as_ref(), Some(&alias));
            assert!(app.tabs[app.active_tab].dirty);
            drop(app);
            drop(reopen_drawing(&alias, false));
        }
    }
}

#[cfg(unix)]
#[test]
fn control_save_file_alias_refuses_retargeting_before_worker_or_completion() {
    use iced::futures::StreamExt;

    for after_worker in [false, true] {
        for release_lease in [false, true] {
            let scratch = Scratch::new();
            let first = scratch.0.join("first.dxf");
            let second = scratch.0.join("second.dxf");
            drop(open_drawing(&first, false));
            drop(open_drawing(&second, false));
            let first_bytes = std::fs::read(&first).unwrap();
            let second_bytes = std::fs::read(&second).unwrap();
            let alias = scratch.0.join("alias.dxf");
            std::os::unix::fs::symlink(&first, &alias).unwrap();
            let mut app = reopen_drawing(&alias, true);
            edit(&mut app);
            let i = app.active_tab;
            let original = app.tabs[i].disk_fingerprint.clone();
            if release_lease {
                let _ = app.tabs[i].edit_lease.take();
            }
            let (accepted, task) = app.control_request(json!({
                "protocol":1,"op":"save","path":alias,
                "request_id":"alias-retarget","client_id":"test",
                "document_id":app.tabs[i].id,"revision":app.tabs[i].edit_revision,
            }));
            assert_eq!(accepted["status"], "accepted", "{accepted}");
            let task = if after_worker {
                // Execute real disk work, but hold its completion message
                // until the logical alias has changed to the other file.
                let mut stream = iced_runtime::task::into_stream(task).unwrap();
                let action = iced::futures::executor::block_on(stream.next()).unwrap();
                let iced_runtime::Action::Output(crate::app::Message::SaveFinished(outcome)) =
                    action
                else {
                    panic!("expected the native save completion");
                };
                assert!(outcome.result.is_ok(), "{:?}", outcome.result);
                iced::Task::done(crate::app::Message::SaveFinished(outcome))
            } else {
                task
            };
            std::fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(&second, &alias).unwrap();
            app.drive_headless_task(task).unwrap();
            let saved = app
                .control_request(json!({"op":"operation","request_id":"alias-retarget"}))
                .0;
            assert_eq!(saved["ok"], false, "{saved}");
            assert_eq!(app.active_modal, Some(ModalKind::ExternalChange));
            assert_eq!(std::fs::read(&second).unwrap(), second_bytes);
            if after_worker {
                assert_ne!(std::fs::read(&first).unwrap(), first_bytes);
            } else {
                assert_eq!(std::fs::read(&first).unwrap(), first_bytes);
            }
            assert_eq!(app.tabs[i].disk_fingerprint, original);
            assert_eq!(app.tabs[i].current_path.as_ref(), Some(&alias));
            assert!(app.tabs[i].dirty);
        }
    }
}
