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
    about = crate::t!("Open CAD Studio — 2D/3D CAD editor").into_owned(),
    long_about = None,
)]
pub struct Cli {
    /// CAD files to open at startup (.dwg / .dxf). Also how the OS file
    /// association launches us when drawings are double-clicked — selecting
    /// several hands them all to one launch, so this takes a list.
    #[arg(help = crate::t!("CAD files to open at startup (.dwg / .dxf).").into_owned(), long_help = None)]
    pub files: Vec<PathBuf>,

    /// Start with a new empty drawing, ignoring any file argument.
    #[arg(long)]
    #[arg(help = crate::t!("Start with a new empty drawing.").into_owned(), long_help = None)]
    pub new: bool,

    /// Always start a new editor process, even when one is already running.
    /// Without this, opening a drawing hands it to the running editor as a tab.
    #[arg(long)]
    #[arg(help = crate::t!("Start a new editor process.").into_owned(), long_help = None)]
    pub new_instance: bool,

    /// Open read-only: editing is allowed but saving is disabled.
    #[arg(long)]
    #[arg(help = crate::t!("Open read-only: saving is disabled.").into_owned(), long_help = None)]
    pub read_only: bool,

    /// Restrict the GPU backend (e.g. dx12, vulkan, gl, metal). Sets WGPU_BACKEND.
    #[arg(long, value_name = "BACKEND")]
    #[arg(help = crate::t!("GPU backend (dx12, vulkan, gl, metal). Sets WGPU_BACKEND.").into_owned(), long_help = None)]
    pub backend: Option<String>,

    /// Safe mode: force the GL backend, for flaky/hybrid GPU drivers.
    #[arg(long, visible_alias = "no-gpu")]
    #[arg(help = crate::t!("Safe mode: use the GL backend.").into_owned(), long_help = None)]
    pub safe_mode: bool,

    /// Force the packed renderer path that avoids shader storage buffers.
    /// Normally selected automatically for adapters with insufficient limits.
    #[arg(long)]
    #[arg(help = crate::t!("Use the renderer for GPUs without shader storage buffers.").into_owned(), long_help = None)]
    pub compat_renderer: bool,

    /// Run the headless JSON automation server (stdin/stdout, or --port).
    #[arg(long)]
    #[arg(help = crate::t!("Run the headless JSON automation server (stdin/stdout, or --port).").into_owned(), long_help = None)]
    pub serve: bool,

    /// Expose the running desktop editor to AI clients over MCP stdio.
    #[arg(long)]
    #[arg(help = crate::t!("Connect AI clients to the running editor over MCP stdio.").into_owned(), long_help = None)]
    pub mcp: bool,

    /// TCP port for --serve (defaults to stdin/stdout).
    #[arg(long, value_name = "PORT")]
    #[arg(help = crate::t!("TCP port for --serve (defaults to stdin/stdout).").into_owned(), long_help = None)]
    pub port: Option<u16>,

    /// Headless convert: read IN, write OUT (format from OUT's extension), exit.
    #[arg(long, num_args = 2, value_names = ["IN", "OUT"])]
    #[arg(help = crate::t!("Convert IN to OUT using the output file extension, then exit.").into_owned(), long_help = None)]
    pub export: Option<Vec<PathBuf>>,

    /// Run a command script at startup: one command line per line of FILE.
    #[arg(long, value_name = "FILE")]
    #[arg(help = crate::t!("Run a command script at startup, one command per line.").into_owned(), long_help = None)]
    pub script: Option<PathBuf>,

    /// Log level (error|warn|info|debug|trace). Also honours RUST_LOG.
    #[arg(long, value_name = "LEVEL")]
    #[arg(help = crate::t!("Log level (error|warn|info|debug|trace). Also reads RUST_LOG.").into_owned(), long_help = None)]
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
