//! Process manager for out-of-process plugins.

use std::path::Path;
use std::process::Child;
use std::sync::Arc;

use crate::host::HostApi;
use crate::process::{PluginError, PluginProcess};
use crate::ribbon::owned::{to_shared_module, SharedCadModule};

/// Owner of every spawned plugin process.
pub struct PluginManager {
    plugins: Vec<LoadedPlugin>,
}

struct LoadedPlugin {
    process: Arc<PluginProcess>,
    module: SharedCadModule,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcome of [`PluginManager::dispatch`].
#[derive(Default)]
pub struct DispatchResult {
    /// A plugin handled the command.
    pub handled: bool,
    /// An interactive command was started by a plugin.
    pub started: Option<(Arc<PluginProcess>, u64)>,
    /// Plugins whose process died before or during dispatch.
    pub dead_plugins: Vec<String>,
    /// Plugins that returned an error while trying to handle the command.
    pub errors: Vec<(String, String)>,
}

impl PluginManager {
    /// Create an empty manager.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Spawn `cdylib_path` as a separate plugin process, build its ribbon
    /// module, and store it. Returns the plugin id on success.
    pub fn load(
        &mut self,
        cdylib_path: &Path,
        host: &mut dyn HostApi,
    ) -> Result<String, PluginError> {
        let process = PluginProcess::spawn(cdylib_path, host)?;
        let id = process.id().to_string();
        let name = process.manifest().name.clone();
        let module = to_shared_module(id.clone(), name, process.ribbon().to_vec());
        self.plugins.push(LoadedPlugin {
            process: Arc::new(process),
            module,
        });
        Ok(id)
    }

    /// Ribbon modules for alive, non-disabled plugins, sorted by `ribbon_order`.
    pub fn ribbon_modules<F: Fn(&str) -> bool>(
        &self,
        is_disabled: F,
    ) -> Vec<(i32, SharedCadModule)> {
        let mut out: Vec<(i32, SharedCadModule)> = self
            .plugins
            .iter()
            .filter(|p| !is_disabled(p.process.id()) && p.process.is_alive())
            .map(|p| (p.process.manifest().ribbon_order, p.module.clone()))
            .collect();
        out.sort_by_key(|(order, _)| *order);
        out
    }

    /// Dispatch `cmd` to each plugin until one handles it.
    ///
    /// `is_disabled` is called for each plugin id so the host can filter
    /// disabled plugins without exposing its set type to the crate.
    pub fn dispatch<F: Fn(&str) -> bool>(
        &self,
        host: &mut dyn HostApi,
        cmd: &str,
        is_disabled: F,
    ) -> DispatchResult {
        let mut result = DispatchResult::default();
        for p in &self.plugins {
            let id = p.process.id().to_string();
            if is_disabled(&id) {
                continue;
            }
            if !p.process.is_alive() {
                result.dead_plugins.push(id);
                continue;
            }
            let process = Arc::clone(&p.process);
            let mut on_start = |command_id: u64| {
                result.started = Some((Arc::clone(&process), command_id));
            };
            match p.process.dispatch(host, cmd, &mut on_start) {
                Ok(true) => {
                    result.handled = true;
                    return result;
                }
                Ok(false) => {}
                Err(e) => result.errors.push((id, e.to_string())),
            }
        }
        result
    }

    /// Plugin ids currently loaded.
    pub fn ids(&self) -> Vec<String> {
        self.plugins
            .iter()
            .map(|p| p.process.id().to_string())
            .collect()
    }

    /// Begin asynchronous shutdown of every plugin process.
    ///
    /// Kills every child synchronously on the calling thread and moves the
    /// blocking `wait()` calls into a single detached reaper thread, so host
    /// shutdown is fast regardless of how many plugins are loaded.
    pub fn shutdown_all(&mut self) {
        let plugins = std::mem::take(&mut self.plugins);
        let mut children: Vec<Child> = Vec::with_capacity(plugins.len());
        for p in plugins {
            let (stream, child) = p.process.take_resources();
            drop(stream);
            if let Some(mut child) = child {
                let _ = child.kill();
                children.push(child);
            }
        }
        if !children.is_empty() {
            std::thread::spawn(move || {
                for mut child in children {
                    let _ = child.wait();
                }
            });
        }
    }
}

impl Drop for PluginManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use super::*;

    #[test]
    fn empty_manager_has_no_ribbon_modules() {
        let manager = PluginManager::new();
        assert!(manager.ribbon_modules(|_| false).is_empty());
        assert!(manager.ids().is_empty());
    }

    #[test]
    fn dispatch_with_no_plugins_is_not_handled() {
        struct EmptyReader;
        impl crate::host::DocumentReader for EmptyReader {
            fn entity_count(&self) -> usize {
                0
            }
            fn for_each_entity(&self, _f: &mut dyn FnMut(crate::host::ReaderEntity<'_>)) {}
            fn layer_name(&self, _handle: acadrust::Handle) -> Option<&str> {
                None
            }
            fn app_id_name(&self, _handle: acadrust::Handle) -> Option<&str> {
                None
            }
        }

        struct DummyHost;
        impl HostApi for DummyHost {
            fn tab_index(&self) -> usize {
                0
            }
            fn document(&self) -> &acadrust::CadDocument {
                panic!("not used")
            }
            fn document_mut(&mut self) -> &mut acadrust::CadDocument {
                panic!("not used")
            }
            fn document_reader(&self) -> Box<dyn crate::host::DocumentReader + '_> {
                Box::new(EmptyReader)
            }
            fn add_entity(&mut self, _entity: acadrust::EntityType) -> acadrust::Handle {
                panic!("not used")
            }
            fn bump_geometry(&mut self) {}
            fn read_record(
                &self,
                _handle: acadrust::Handle,
                _app_name: &str,
            ) -> Option<&acadrust::xdata::ExtendedDataRecord> {
                None
            }
            fn write_record(
                &mut self,
                _handle: acadrust::Handle,
                _record: acadrust::xdata::ExtendedDataRecord,
            ) -> bool {
                false
            }
            fn remove_record(&mut self, _handle: acadrust::Handle, _app_name: &str) -> bool {
                false
            }
            fn push_undo(&mut self, _label: &str) {}
            fn set_dirty(&mut self) {}
            fn push_info(&mut self, _msg: &str) {}
            fn push_output(&mut self, _msg: &str) {}
            fn push_error(&mut self, _msg: &str) {}
            fn start_interactive(&mut self, _command: Box<dyn crate::host::InteractiveCommand>) {}
            fn plugin_state_any(
                &self,
                _plugin_id: &str,
            ) -> Option<&(dyn std::any::Any + Send + Sync)> {
                None
            }
            fn plugin_state_any_mut(
                &mut self,
                _plugin_id: &str,
            ) -> Option<&mut (dyn std::any::Any + Send + Sync)> {
                None
            }
            fn ensure_plugin_state_any(
                &mut self,
                _plugin_id: &'static str,
                _init: &mut dyn FnMut() -> Box<dyn std::any::Any + Send + Sync>,
            ) -> &mut (dyn std::any::Any + Send + Sync) {
                panic!("not used")
            }
        }

        let manager = PluginManager::new();
        let mut host = DummyHost;
        let result = manager.dispatch(&mut host, "FOO", |_| false);
        assert!(!result.handled);
        assert!(result.started.is_none());
    }
}
