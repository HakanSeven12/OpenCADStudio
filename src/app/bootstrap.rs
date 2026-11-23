use super::*;


#[cfg(not(target_arch = "wasm32"))]
pub fn run() -> iced::Result {
    // UI text follows the desktop's interface font: generic sans-serif, iced's
    // default font, is remapped to it before the first window opens.
    crate::sys::startup_times::mark("run: entry");
    crate::sys::ui_font::apply(crate::sys::ui_font::detect().as_ref());
    crate::sys::startup_times::mark("run: ui_font applied");
    iced::daemon(
        OpenCADStudio::boot,
        OpenCADStudio::update_root,
        OpenCADStudio::view,
    )
    .settings(iced::Settings {
        power_preference: iced::backend::PowerPreference::HighPerformance,
        ..iced::Settings::default()
    })
    .subscription(OpenCADStudio::subscription)
    .scale_factor(|state: &OpenCADStudio, _| {
        let scale = f32::from(state.ui_scale_percent) / 100.0;
        // Icons fit their art to the physical pixels iced draws at: the
        // window's scale times this one. Noted where iced reads it, so the
        // two never disagree.
        crate::ui::icons::set_ui_scale(scale);
        scale
    })
    .title(|state: &OpenCADStudio, window_id: window::Id| {
        // A tool dialog or pallet in a window of its own carries its name.
        if let Some(title) = state.os_window_title(window_id) {
            return title;
        }
        if let Some(tab) = state.tabs.get(state.active_tab) {
            let dot = if tab.dirty { "● " } else { "" };
            let name = tab.tab_display_name();
            format!(
                "{}Open CAD Studio {} - {}",
                dot,
                env!("OCS_APP_VERSION"),
                name
            )
        } else {
            concat!("Open CAD Studio ", env!("OCS_APP_VERSION")).to_string()
        }
    })
    .theme(|state: &OpenCADStudio, _| state.active_theme.clone())
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .run()
    .inspect(|()| {
        // Started once this process has let go of its window and its
        // single-instance port, so the new one boots as the only editor.
        if RESTART_AFTER_EXIT.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
        }
    })
}

/// Set when the user quits through Restart (the Plugin Manager after an
/// install); `run` starts the app again once its loop ends.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) static RESTART_AFTER_EXIT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl Drop for OpenCADStudio {
    fn drop(&mut self) {
        #[cfg(target_arch = "wasm32")]
        crate::sys::set_unsaved_changes_warning(false);
        // Kill plugin runner processes as soon as the application state is
        // dropped, instead of waiting for the thread-local manager destructor.
        // This makes host shutdown deterministic and fast on every exit path.
        #[cfg(not(target_arch = "wasm32"))]
        crate::plugin::external::shutdown_plugins();
    }
}

/// Single-window entry for the web (wasm) build. Uses `iced::application`
/// instead of `iced::daemon`: the browser canvas is the only window, so the
/// main-window view is rendered directly and the manager/dialog windows are
/// unavailable for now (see issue #45). Native keeps the multi-window `run`.
#[cfg(target_arch = "wasm32")]
pub fn run_web() -> iced::Result {
    // winit never emits DroppedFile on the web, so the document drag & drop
    // listeners are installed here and feed the control poll (#344).
    crate::io::web_drop::install();
    // Best-effort, fire-and-forget: ask the browser not to evict the origin's
    // storage under pressure — settings (localStorage) and the OPFS recent
    // copies / autosave snapshots live there. A denial changes nothing.
    if !crate::io::host_bridge::is_embed_mode() {
        wasm_bindgen_futures::spawn_local(async {
            if let Some(window) = web_sys::window() {
                if let Ok(promise) = window.navigator().storage().persist() {
                    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                }
            }
        });
    }
    iced::application(
        OpenCADStudio::boot_web,
        OpenCADStudio::update_root,
        OpenCADStudio::view_main,
    )
    .subscription(OpenCADStudio::subscription)
    .title(|_state: &OpenCADStudio| {
        concat!("Open CAD Studio ", env!("OCS_APP_VERSION")).to_string()
    })
    .theme(|state: &OpenCADStudio| state.active_theme.clone())
    .backend(iced::Backend::Hardware(iced::backend::Api::OpenGL))
    // No MSAA for canvas geometry: WebGL resolves the multisampled target with
    // the last draw's scissor still on, so only that rectangle came through.
    // Pattern previews stayed blank, stale crosshairs and grips lingered over
    // other panels (#2248, #2249, #2250).
    .antialiasing(false)
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .run()
}
