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
pub fn ribbon_modules_enabled(
    disabled: &rustc_hash::FxHashSet<String>,
) -> Vec<Box<dyn CadModule>> {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut core = core_registry::all_modules();
    // Dynamically-loaded external plugins contribute tabs (their libraries stay
    // resident for the session, so these vtables remain valid).
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut addons: Vec<(i32, Box<dyn CadModule>)> = Vec::new();
        crate::plugin::external::with_loaded(|loaded| {
            for lp in loaded {
                if disabled.contains(lp.id.as_str()) || !lp.process.is_alive() {
                    continue;
                }
                addons.push((
                    lp.process.manifest().ribbon_order,
                    Box::new(lp.module.clone()) as Box<dyn CadModule>,
                ));
            }
        });
        addons.sort_by_key(|(order, _)| *order);
        core.extend(addons.into_iter().map(|(_, ribbon)| ribbon));
    }
    let _ = disabled;
    core
}

/// Dispatch `cmd` to a loaded external plugin (skipping disabled ones).
/// Returns true if one handled it.
pub(crate) fn try_dispatch(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use super::host::HostSession;
        let disabled = app.disabled_plugin_ids();
        let mut started: Option<(u64, std::sync::Arc<ocs_plugin_api::process::PluginProcess>)> = None;
        let mut dead_plugins: Vec<String> = Vec::new();
        let mut dispatch_errors: Vec<(String, String)> = Vec::new();
        let handled = crate::plugin::external::with_loaded(|loaded| {
            let mut host = HostSession::new(app, tab);
            for lp in loaded {
                if disabled.contains(lp.id.as_str()) {
                    continue;
                }
                if !lp.process.is_alive() {
                    dead_plugins.push(lp.id.clone());
                    continue;
                }
                let process = std::sync::Arc::clone(&lp.process);
                let mut start = |command_id: u64| {
                    started = Some((command_id, std::sync::Arc::clone(&process)));
                };
                match crate::plugin::guard("dispatch", || lp.process.dispatch(&mut host, cmd, &mut start)) {
                    Some(Ok(true)) => return true,
                    Some(Ok(false)) => {}
                    Some(Err(e)) => {
                        eprintln!("[plugin] dispatch error for '{}': {e}", lp.id);
                        dispatch_errors.push((lp.id.clone(), e.to_string()));
                    }
                    None => {
                        // Panic already logged by guard.
                    }
                }
            }
            false
        });
        for id in dead_plugins {
            app.push_plugin_error(&format!("Plugin '{id}' process died; skipping dispatch"));
        }
        for (id, err) in dispatch_errors {
            app.push_plugin_error(&format!("Plugin '{id}' dispatch error: {err}"));
        }
        if let Some((command_id, process)) = started {
            app.set_active_command(
                tab,
                Box::new(crate::app::plugin_host::PluginProcessInteractiveAdapter::new(
                    process,
                    command_id,
                )),
            );
        }
        if handled {
            return true;
        }
    }
    let _ = (app, tab, cmd);
    false
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
