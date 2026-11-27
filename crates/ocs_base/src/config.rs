//! Shared per-user config directory used by the small settings stores (recent
//! files, status-bar layout, ribbon collapse mode, …). Everything lives under
//! `<platform-config>/OpenCADStudio` so the app keeps a single tidy folder.
//! Also the GUI startup configuration `main` reads off the command line
//! ([`GuiConfig`]).

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// GUI startup configuration, handed from `main` to `app::boot` out-of-band
/// because the iced daemon's boot closure takes no arguments.
#[derive(Debug, Default, Clone)]
pub struct GuiConfig {
    /// Files to open on launch (empty for a blank session).
    pub files: Vec<std::path::PathBuf>,
    /// Open a fresh drawing tab on launch instead of the welcome screen.
    pub new: bool,
    /// Saving disabled for this session.
    pub read_only: bool,
    pub low_memory: bool,
    /// Force storage-buffer-free wire and hatch pipelines.
    pub compat_renderer: bool,
    /// Command lines to run once the editor is up.
    pub script_lines: Vec<String>,
    /// Why this launch switched graphics backend, if it did. Shown once
    /// as a command-line warning so the switch is never silent.
    pub gpu_fallback_notice: Option<String>,
    /// The GPU probe found no shader storage buffers: the packed
    /// compatibility renderer was enabled automatically for this session.
    pub gpu_compat_auto: bool,
}

/// Set once by `main` before the GUI boots; read by `app::boot`.
pub static GUI_CONFIG: std::sync::OnceLock<GuiConfig> = std::sync::OnceLock::new();

pub fn low_memory_viewing() -> bool {
    GUI_CONFIG.get().is_some_and(|cfg| cfg.read_only && cfg.low_memory)
}

/// The GUI config, or a default empty one if `main` never set it (e.g. tests).
pub fn gui_config() -> GuiConfig {
    GUI_CONFIG.get().cloned().unwrap_or_default()
}

/// The OpenCADStudio config directory (not created). `None` when the platform
/// config base can't be resolved (e.g. no `HOME`). Callers `join` their own
/// file name onto it and `create_dir_all` its parent before writing.
#[cfg(not(target_arch = "wasm32"))]
pub fn config_dir() -> Option<PathBuf> {
    // Tests must never read or write the person's real settings: a test run
    // once raced dozens of tests through an alias-table migration and left the
    // user's alias file holding a single line. Every store routes through here,
    // so one per-process scratch folder isolates them all.
    if cfg!(any(test, feature = "test-isolation")) {
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

/// The drawing templates folder (`templates` in the config folder), created
/// on first use: NEW opens its template picker here unless the default
/// template lives somewhere else.
#[cfg(not(target_arch = "wasm32"))]
pub fn templates_dir() -> Option<PathBuf> {
    let dir = config_dir()?.join("templates");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
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

/// Managed mode (`OPENAEC_BEHEERD=1`): a host such as a container image
/// installs, updates and integrates the program, so it asks nothing at
/// start (default app, donation, GPU notice) and never checks for updates.
pub fn managed() -> bool {
    std::env::var_os("OPENAEC_BEHEERD").is_some_and(|value| value == "1")
}

/// The host's working folder (`OPENAEC_MAP`), where file dialogs start.
#[cfg(not(target_arch = "wasm32"))]
fn managed_folder() -> Option<PathBuf> {
    std::env::var_os("OPENAEC_MAP")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// The directory the last file dialog picked or saved into, if it still
/// exists — used to seed the next dialog so pickers reopen where the user
/// left off. Persisted across runs. With a host working folder, dialogs
/// stay inside it and start at its top.
#[cfg(not(target_arch = "wasm32"))]
pub fn last_dialog_dir() -> Option<PathBuf> {
    let last = last_dir_store().lock().ok()?.clone().filter(|p| p.is_dir());
    match managed_folder() {
        Some(root) => Some(last.filter(|dir| dir.starts_with(&root)).unwrap_or(root)),
        None => last,
    }
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
