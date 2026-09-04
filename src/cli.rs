//! Command-line interface.
//!
//! Parsing lives here; `main` interprets the result. Four run modes split out
//! of the parsed args:
//!   - `--mcp`              local stdio MCP server for AI clients
//!   - `--serve`            headless JSON automation server (see `app::serve`)
//!   - `--export IN OUT`    one-shot headless format conversion, then exit
//!   - otherwise            launch the GUI editor, configured via [`GuiConfig`]
//!
//! GUI-only options (open-file, `--new`, `--read-only`, `--script`) are stashed
//! in [`GUI_CONFIG`] for `app::boot` to read, since the iced daemon boots with
//! no arguments of its own.

use std::path::PathBuf;
use std::sync::OnceLock;

use clap::Parser;

/// Open CAD Studio command-line options.
#[derive(Parser, Debug, Default)]
#[command(
    name = "OpenCADStudio",
    version,
    about = "Open CAD Studio — 2D/3D CAD editor",
    long_about = None,
)]
pub struct Cli {
    /// CAD files to open at startup (.dwg / .dxf). Also how the OS file
    /// association launches us when drawings are double-clicked — selecting
    /// several hands them all to one launch, so this takes a list.
    pub files: Vec<PathBuf>,

    /// Start with a new empty drawing, ignoring any file argument.
    #[arg(long)]
    pub new: bool,

    /// Always start a new editor process, even when one is already running.
    /// Without this, opening a drawing hands it to the running editor as a tab.
    #[arg(long)]
    pub new_instance: bool,

    /// Open read-only: editing is allowed but saving is disabled.
    #[arg(long)]
    pub read_only: bool,

    /// Restrict the GPU backend (e.g. dx12, vulkan, gl, metal). Sets WGPU_BACKEND.
    #[arg(long, value_name = "BACKEND")]
    pub backend: Option<String>,

    /// Safe mode: force the GL backend, for flaky/hybrid GPU drivers.
    #[arg(long, visible_alias = "no-gpu")]
    pub safe_mode: bool,

    /// Force the packed renderer path that avoids shader storage buffers.
    /// Normally selected automatically for adapters with insufficient limits.
    #[arg(long)]
    pub compat_renderer: bool,

    /// Run the headless JSON automation server (stdin/stdout, or --port).
    #[arg(long)]
    pub serve: bool,

    /// Expose the running desktop editor to AI clients over MCP stdio.
    #[arg(long)]
    pub mcp: bool,

    /// TCP port for --serve (defaults to stdin/stdout).
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    /// Headless convert: read IN, write OUT (format from OUT's extension), exit.
    #[arg(long, num_args = 2, value_names = ["IN", "OUT"])]
    pub export: Option<Vec<PathBuf>>,

    /// Run a command script at startup: one command line per line of FILE.
    #[arg(long, value_name = "FILE")]
    pub script: Option<PathBuf>,

    /// Log level (error|warn|info|debug|trace). Also honours RUST_LOG.
    #[arg(long, value_name = "LEVEL")]
    pub log: Option<String>,

    /// Internal: run as the plugin runner child process.
    #[arg(long, value_names = ["SOCKET", "CDYLIB"], num_args = 2, hide = true)]
    pub ocs_plugin_runner: Option<Vec<String>>,

    /// Internal: write a DWG's embedded preview to a PNG for the OS file-manager
    /// thumbnailer (`<IN> <OUT> <SIZE>`). Handled before the GUI starts.
    #[arg(long, value_names = ["IN", "OUT", "SIZE"], num_args = 3, hide = true)]
    pub dwg_thumbnail: Option<Vec<String>>,
}

/// GUI startup configuration, handed from `main` to `app::boot` out-of-band
/// because the iced daemon's boot closure takes no arguments.
#[derive(Debug, Default, Clone)]
pub struct GuiConfig {
    /// Files to open on launch (empty for a blank session).
    pub files: Vec<PathBuf>,
    /// Open a fresh drawing tab on launch instead of the welcome screen.
    pub new: bool,
    /// Saving disabled for this session.
    pub read_only: bool,
    /// Force storage-buffer-free wire and hatch pipelines.
    pub compat_renderer: bool,
    /// Command lines to run once the editor is up.
    pub script_lines: Vec<String>,
}

/// Set once by `main` before the GUI boots; read by `app::boot`.
pub static GUI_CONFIG: OnceLock<GuiConfig> = OnceLock::new();

/// The GUI config, or a default empty one if `main` never set it (e.g. tests).
pub fn gui_config() -> GuiConfig {
    GUI_CONFIG.get().cloned().unwrap_or_default()
}
