//! Block-editor autosave (crash recovery).
//!
//! The block editor keeps "Save Block" edits provisional until the session is
//! closed (BEDIT_SAVE / BEDIT_DISCARD), so a crash inside the editor loses
//! them even though the user pressed Save. `on_autosave` therefore also writes
//! a `.ocs-block-autosave.sv$` copy beside the drawing while a block session is
//! open, and the open path offers it back on the next launch.
//!
//! The naming and the "is it worth offering" decision live here as pure
//! functions so they can be tested without an app instance.

/// Recovery-copy path for the block editor of the drawing at `path`.
///
/// Mirrors the regular autosave naming so both copies sit side by side:
/// `<name>.ocs-block-autosave.sv$` next to `<name>.ocs-autosave.sv$`.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn block_autosave_path(path: &std::path::Path) -> std::path::PathBuf {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "drawing".to_string());
    path.with_file_name(format!("{name}.ocs-block-autosave.sv$"))
}

/// Whether a block-editor recovery copy is worth offering.
///
/// Only when it is strictly newer than the drawing's regular autosave copy:
/// older or equal means the regular snapshot already carries the edits (or
/// there are none), and offering it would resurrect a stale block.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn should_offer_block_recovery(
    block: std::time::SystemTime,
    main: Option<std::time::SystemTime>,
) -> bool {
    match main {
        Some(main) => block > main,
        // No regular copy: this one is the only recovery point left.
        None => true,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{block_autosave_path, should_offer_block_recovery};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    #[test]
    fn recovery_copy_sits_beside_the_drawing() {
        let path = Path::new("/tmp/plano N4.dwg");
        assert_eq!(
            block_autosave_path(path),
            Path::new("/tmp/plano N4.dwg.ocs-block-autosave.sv$")
        );
    }

    #[test]
    fn recovery_copy_differs_from_the_regular_autosave() {
        let path = Path::new("/tmp/plano.dwg");
        let block = block_autosave_path(path);
        let main = path.with_file_name("plano.dwg.ocs-autosave.sv$");
        assert_ne!(
            block, main,
            "the two snapshots must not overwrite each other"
        );
    }

    #[test]
    fn newer_block_copy_is_offered() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(should_offer_block_recovery(
            base + Duration::from_secs(60),
            Some(base)
        ));
    }

    #[test]
    fn equal_or_older_block_copy_is_not_offered() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(!should_offer_block_recovery(base, Some(base)));
        assert!(!should_offer_block_recovery(
            base - Duration::from_secs(60),
            Some(base)
        ));
    }

    #[test]
    fn missing_regular_copy_still_offers_the_block_one() {
        let block = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(should_offer_block_recovery(block, None));
    }

    /// Filesystem simulation of the crash: a drawing whose block autosave is
    /// newer than its regular autosave must be offered; once the regular copy
    /// catches up, the offer must stop.
    #[test]
    fn filesystem_simulation() {
        let dir =
            std::env::temp_dir().join(format!("ocs-block-autosave-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let drawing = dir.join("plano.dwg");
        let main = dir.join("plano.dwg.ocs-autosave.sv$");
        let block = block_autosave_path(&drawing);

        std::fs::write(&drawing, b"DWG").unwrap();
        // Real-world order: the regular autosave lands first, THEN the block
        // editor writes its own copy while the session stays open.
        std::fs::write(&main, b"MAIN").unwrap();
        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(&block, b"BLOCK").unwrap();

        let block_t = std::fs::metadata(&block).unwrap().modified().unwrap();
        let main_t = std::fs::metadata(&main).unwrap().modified().unwrap();
        assert!(
            should_offer_block_recovery(block_t, Some(main_t)),
            "block copy written after the regular one must be offered"
        );

        // A later regular autosave supersedes it.
        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(&main, b"MAIN2").unwrap();
        let main_t2 = std::fs::metadata(&main).unwrap().modified().unwrap();
        assert!(
            !should_offer_block_recovery(block_t, Some(main_t2)),
            "an older block copy must not be offered"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
