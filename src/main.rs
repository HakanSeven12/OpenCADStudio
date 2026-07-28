#![allow(non_snake_case)]
// On Windows release builds, hide the console window the OS would
// otherwise spawn alongside the GUI. Debug builds keep stdout/stderr
// attached so eprintln! / panics stay visible while developing.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod config;
#[cfg(not(target_arch = "wasm32"))]
mod cli;
mod command;
mod entities;
mod io;
mod modules;
mod patreon;
mod discussions;
mod videos;
mod plugin;
mod perf;
mod scene;
mod snap;
mod ui;
mod par;
mod sys;

fn main() -> iced::Result {
    // Web (wasm) uses the single-window entry; native uses the multi-window
    // daemon. Trunk calls `main` from its generated JS bootstrap. The web build
    // takes no CLI args, so it skips parsing entirely.
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        // After the console hook so the chained panic mirror keeps it; also
        // installs the log-facade listener that surfaces wgpu/naga errors as
        // a copyable on-page banner (#414 — an empty canvas otherwise gives
        // reporters nothing to paste).
        sys::web_diag::init();
        return app::run_web();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use clap::Parser;
        let args = cli::Cli::parse();

        // Plugin runner mode: the host spawns itself with this hidden flag to
        // load a plugin cdylib in an isolated process. Hand off immediately so
        // the child never touches GUI state.
        if let Some(runner_args) = &args.ocs_plugin_runner {
            if runner_args.len() != 2 {
                eprintln!("--ocs-plugin-runner expects <socket> <cdylib>");
                std::process::exit(1);
            }
            let socket = &runner_args[0];
            let cdylib = std::path::Path::new(&runner_args[1]);
            if let Err(e) = ocs_plugin_api::runner::run(socket, cdylib) {
                eprintln!("[runner] fatal: {e}");
                std::process::exit(1);
            }
            return Ok(());
        }

        // Thumbnail mode: the OS file-manager thumbnailer invokes us as
        // `--dwg-thumbnail <in> <out> <size>`. Extract the DWG's embedded
        // preview to a PNG and exit — never touch the GUI.
        if let Some(a) = &args.dwg_thumbnail {
            let size = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(256);
            let ok = io::thumbnail::extract_to_png(
                std::path::Path::new(&a[0]),
                std::path::Path::new(&a[1]),
                size,
            );
            std::process::exit(if ok { 0 } else { 1 });
        }

        // Opt-in logging. `--log LEVEL` seeds RUST_LOG; the subscriber then
        // surfaces wgpu / iced / winit diagnostics that are otherwise silent.
        if let Some(level) = &args.log {
            std::env::set_var("RUST_LOG", level);
        }
        if std::env::var_os("RUST_LOG").is_some() {
            let _ = env_logger::try_init();
        }

        // GPU backend selection. Explicit `--backend` wins; `--safe-mode`
        // forces GL for flaky drivers. On Windows, fall back to DX12/Vulkan so
        // the AMD OpenGL ICD (atio6axx.dll) is never touched at startup — it
        // access-violates on some hybrid-GPU laptops before any window appears
        // (#55). An already-set WGPU_BACKEND always wins.
        if let Some(backend) = &args.backend {
            std::env::set_var("WGPU_BACKEND", backend);
        } else if args.safe_mode {
            std::env::set_var("WGPU_BACKEND", "gl");
        }
        #[cfg(target_os = "windows")]
        if std::env::var_os("WGPU_BACKEND").is_none() {
            std::env::set_var("WGPU_BACKEND", "dx12,vulkan");
        }

        // Headless modes exit without ever creating a window.
        if args.serve {
            // `app::serve` reads --port itself from the raw args.
            app::serve();
            return Ok(());
        }
        if let Some(io) = &args.export {
            // clap enforces exactly two values for --export.
            let code = app::export_headless(&io[0], &io[1]);
            std::process::exit(code);
        }

        // Single instance: a double-clicked drawing belongs as a tab in the
        // editor that is already open, not in a second copy of the app.
        //
        // The gate is POSITIONAL, and that is the point: every headless mode
        // has already returned above — the plugin runner, which is this same
        // binary re-spawning itself, most of all. A flag list here would rot
        // the first time a mode is added; a position cannot.
        if !args.new_instance {
            if let io::single_instance::Claim::Existing(stream) = io::single_instance::claim() {
                // Only bare files forward. `--read-only` / `--script` / `--new`
                // configure the whole editor rather than a tab, so they always
                // get a process of their own.
                let plain_open =
                    !args.read_only && args.script.is_none() && !args.new && !args.files.is_empty();
                if plain_open && io::single_instance::handoff(stream, &args.files) {
                    return Ok(());
                }
                // Nothing to forward, or the far end never acknowledged: fall
                // through and boot our own window. We hold no listener, so the
                // editor that owns the port keeps serving.
            }
        }

        // GUI: stash the startup config for `app::boot` to pick up.
        let script_lines = args
            .script
            .as_ref()
            .map(|p| match std::fs::read_to_string(p) {
                Ok(text) => text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_string)
                    .collect(),
                Err(e) => {
                    eprintln!("--script: cannot read {}: {e}", p.display());
                    Vec::new()
                }
            })
            .unwrap_or_default();
        let _ = cli::GUI_CONFIG.set(cli::GuiConfig {
            files: if args.new { Vec::new() } else { args.files },
            new: args.new,
            read_only: args.read_only,
            compat_renderer: args.compat_renderer,
            script_lines,
        });

        // Register (or refresh) the freedesktop DWG thumbnailer so file managers
        // show OCS-embedded previews. Idempotent, best-effort, no consent step —
        // it only points a `.thumbnailer` at this same binary's `--dwg-thumbnail`
        // mode. Silently ignored on failure or non-Linux.
        io::file_association::install_thumbnailer();

        app::run()
    }
}
