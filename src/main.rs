#![allow(non_snake_case)]
// On Windows release builds, hide the console window the OS would
// otherwise spawn alongside the GUI. Debug builds keep stdout/stderr
// attached so eprintln! / panics stay visible while developing.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
#[cfg(not(target_arch = "wasm32"))]
mod cli;
mod command;
mod entities;
mod io;
mod linetypes;
mod modules;
mod plugin;
mod patterns;
mod scene;
mod snap;
mod ui;
mod par;
mod sys;
mod update_check;

fn main() -> iced::Result {
    // Web (wasm) uses the single-window entry; native uses the multi-window
    // daemon. Trunk calls `main` from its generated JS bootstrap. The web build
    // takes no CLI args, so it skips parsing entirely.
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
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
            file: if args.new { None } else { args.file },
            new: args.new,
            read_only: args.read_only,
            script_lines,
        });
        app::run()
    }
}
