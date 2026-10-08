//! Shared per-user config directory used by the small settings stores (recent
//! files, status-bar layout, ribbon collapse mode, …). Everything lives under
//! `<platform-config>/OpenCADStudio` so the app keeps a single tidy folder.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// The OpenCADStudio config directory (not created). `None` when the platform
/// config base can't be resolved (e.g. no `HOME`). Callers `join` their own
/// file name onto it and `create_dir_all` its parent before writing.
#[cfg(not(target_arch = "wasm32"))]
pub fn config_dir() -> Option<PathBuf> {
    // Tests must never read or write the person's real settings: a test run
    // once raced dozens of tests through an alias-table migration and left the
    // user's alias file holding a single line. Every store routes through here,
    // so one per-process scratch folder isolates them all.
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("ocs-test-config-{}", std::process::id())));
    }
    let base: PathBuf = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)?
    } else if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        p.push("Library");
        p.push("Application Support");
        p
    } else if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(d)
    } else {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        p.push(".config");
        p
    };
    let mut p = base;
    p.push("OpenCADStudio");
    Some(p)
}

// ── Last file-dialog directory ───────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Mutex, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
fn last_dir_store() -> &'static Mutex<Option<PathBuf>> {
    static STORE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    STORE.get_or_init(|| {
        // Seed from the persisted value; discard it if the folder is gone.
        let loaded = config_dir()
            .map(|d| d.join("last_dir.txt"))
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| p.is_dir());
        Mutex::new(loaded)
    })
}

/// The directory the last file dialog picked or saved into, if it still
/// exists — used to seed the next dialog so pickers reopen where the user
/// left off. Persisted across runs.
#[cfg(not(target_arch = "wasm32"))]
pub fn last_dialog_dir() -> Option<PathBuf> {
    last_dir_store().lock().ok()?.clone().filter(|p| p.is_dir())
}

/// Record the directory of a path a file dialog just returned.
#[cfg(not(target_arch = "wasm32"))]
pub fn remember_dialog_dir(file_path: &Path) {
    let Some(dir) = file_path.parent().filter(|d| d.is_dir()) else {
        return;
    };
    if let Ok(mut store) = last_dir_store().lock() {
        if store.as_deref() == Some(dir) {
            return; // unchanged — skip the disk write
        }
        *store = Some(dir.to_path_buf());
    }
    if let Some(cfg) = config_dir() {
        let _ = std::fs::create_dir_all(&cfg);
        let _ = std::fs::write(cfg.join("last_dir.txt"), dir.display().to_string());
    }
}
