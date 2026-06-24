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
    // Dynamically-loaded external plugins contribute tabs via the crate manager.
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::plugin::external::with_manager(|manager| {
            let addons = manager.ribbon_modules(|id| disabled.contains(id));
            core.extend(addons.into_iter().map(|(_, module)| {
                Box::new(module) as Box<dyn CadModule>
            }));
        });
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
        let result = {
            let mut host = HostSession::new(app, tab);
            crate::plugin::external::with_manager(|manager| {
                manager.dispatch(&mut host, cmd, |id| disabled.contains(id))
            })
        };

        for id in result.dead_plugins {
            app.push_plugin_error(&format!("Plugin '{id}' process died; skipping dispatch"));
        }
        for (id, err) in result.errors {
            app.push_plugin_error(&format!("Plugin '{id}' dispatch error: {err}"));
        }
        if let Some((process, command_id)) = result.started {
            app.set_active_command(
                tab,
                Box::new(crate::app::plugin_host::PluginProcessInteractiveAdapter::new(
                    process,
                    command_id,
                )),
            );
        }
        return result.handled;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (app, tab, cmd);
        false
    }
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
