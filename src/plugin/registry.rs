// Plugin registry — external (dynamically-loaded) plugins only. OpenCADStudio
// ships no built-in add-ons; every plugin is a cdylib loaded from the plugins
// folder at startup (see `external`) and the marketplace installs them there.

use crate::app::OpenCADStudio;
use crate::modules::{registry as core_registry, CadModule};

/// Core ribbon tabs plus every loaded external add-on tab.
pub fn all_ribbon_modules() -> Vec<Box<dyn CadModule>> {
    ribbon_modules_enabled(&rustc_hash::FxHashSet::default())
}

/// Core ribbon tabs plus the tabs of loaded external plugins whose id is **not**
/// in `disabled` (sorted by `manifest.ribbon_order`).
pub fn ribbon_modules_enabled(disabled: &rustc_hash::FxHashSet<String>) -> Vec<Box<dyn CadModule>> {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut core = core_registry::all_modules();
    // Dynamically-loaded external plugins contribute tabs via the crate manager.
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::plugin::external::with_manager(|manager| {
            let addons = manager.ribbon_modules(|id| disabled.contains(id));
            core.extend(
                addons
                    .into_iter()
                    .map(|(_, module)| Box::new(module) as Box<dyn CadModule>),
            );
        });
    }
    let _ = disabled;
    core
}

/// Command names contributed by loaded external plugins whose id is **not** in
/// `disabled` — every ribbon tool's command id plus manifest command prefixes.
/// The host merges these into command-line autocomplete so plugin commands are
/// discoverable by typing, not only via ribbon buttons (#272).
pub fn plugin_command_names(disabled: &rustc_hash::FxHashSet<String>) -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::plugin::external::with_manager(|manager| {
            manager.command_names(|id| disabled.contains(id))
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = disabled;
        Vec::new()
    }
}

/// Dispatch `cmd` to a loaded external plugin (skipping disabled ones).
/// Returns true if one handled it.
///
/// Deterministic boot (API v12 hardening): while a plugin runner is still in
/// its boot window, an unhandled dispatch of a command a loaded plugin lists
/// by exact name is retried a bounded number of times, healing spawn/
/// handshake races; a command still refused after the retries gets a visible
/// headline instead of a silent "unknown command".
pub(crate) fn try_dispatch(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let disabled = app.disabled_plugin_ids();
        let (handled, result) = dispatch_with_boot_retry(app, tab, cmd, &disabled);
        for id in result.dead_plugins {
            app.push_plugin_error(&format!("Plugin '{id}' process died; skipping dispatch"));
        }
        for (id, err) in result.errors {
            app.push_plugin_error(&format!("Plugin '{id}' dispatch error: {err}"));
        }
        if let Some((process, command_id)) = result.started {
            app.set_active_command(
                tab,
                Box::new(
                    crate::app::plugin_host::PluginProcessInteractiveAdapter::new(
                        process, command_id,
                    ),
                ),
            );
        }
        handled
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (app, tab, cmd);
        false
    }
}

/// One dispatch attempt against the live manager.
#[cfg(not(target_arch = "wasm32"))]
fn dispatch_once(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
    disabled: &rustc_hash::FxHashSet<String>,
) -> (bool, ocs_plugin_api::process::DispatchResult) {
    use super::host::HostSession;
    let mut host = HostSession::new(app, tab);
    let result = crate::plugin::external::with_manager(|manager| {
        manager.dispatch(&mut host, cmd, |id| disabled.contains(id))
    });
    (result.handled, result)
}

/// The boot-race retry: when a runner has not attached its notification
/// channel yet, a loaded plugin lists the typed verb by exact name, and the
/// dispatch went unanswered with no error and no dead plugin, retry a
/// bounded number of times — and give a persistent refusal a visible
/// headline so the failure is never silent.
#[cfg(not(target_arch = "wasm32"))]
fn dispatch_with_boot_retry(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
    disabled: &rustc_hash::FxHashSet<String>,
) -> (bool, ocs_plugin_api::process::DispatchResult) {
    let (mut handled, mut result) = dispatch_once(app, tab, cmd, disabled);
    if handled || !result.errors.is_empty() || !result.dead_plugins.is_empty() {
        return (handled, result);
    }
    let booting_plugin_command = crate::plugin::external::with_manager(|manager| {
        let booting = manager
            .alive_ids()
            .iter()
            .any(|id| manager.notification_channel_attached(id) == Some(false));
        booting
            && manager
                .command_names(|id| disabled.contains(id))
                .iter()
                .any(|name| name == cmd)
    });
    if !booting_plugin_command {
        return (handled, result);
    }
    const RETRIES: usize = 2;
    const RETRY_PAUSE: std::time::Duration = std::time::Duration::from_millis(120);
    app.push_plugin_error(&format!(
        "'{cmd}' is listed by a loaded plugin but was not handled yet; retrying \
         ({RETRIES}x, {RETRY_PAUSE:?}) — a runner may still be finishing its boot handshake"
    ));
    for _ in 0..RETRIES {
        std::thread::sleep(RETRY_PAUSE);
        let disabled = app.disabled_plugin_ids();
        let (again, retried) = dispatch_once(app, tab, cmd, &disabled);
        handled = again;
        // Keep the last result's diagnostics; started-commands surfaced above.
        if retried.started.is_some() {
            result.started = retried.started;
        }
        result.errors = retried.errors;
        result.dead_plugins = retried.dead_plugins;
        if handled {
            break;
        }
    }
    if !handled {
        app.push_plugin_error(&format!(
            "plugin command '{cmd}' is listed by its (alive) plugin but the dispatch still \
             went unhandled; the runner may be wedged"
        ));
    }
    (handled, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbon_is_core_only_without_external_plugins() {
        // No external plugins are loaded under test, so the ribbon is exactly
        // the built-in core tabs.
        let titles: Vec<&str> = all_ribbon_modules().iter().map(|m| m.title()).collect();
        assert!(!titles.is_empty(), "expected core ribbon tabs");
        assert_eq!(titles.len(), core_registry::all_modules().len());
    }
}
