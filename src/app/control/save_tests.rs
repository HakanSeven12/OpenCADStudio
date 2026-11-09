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
