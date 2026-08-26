//! Process manager for out-of-process plugins.
//!
//! `PluginManager` is the host's owner of every loaded plugin process. It is
//! responsible for:
//!
//! - Spawning plugins via [`PluginProcess::spawn`] and building their ribbon
//!   modules.
//! - Routing commands to plugins through [`PluginManager::dispatch`].
//! - Broadcasting host notifications to every alive V4 plugin via
//!   [`PluginManager::broadcast_notification`].
//! - Surfacing dead plugins and per-plugin errors in [`DispatchResult`].
//!
//! Notifications are V4-only: `broadcast_notification` skips non-V4 processes.
//! The plugin-to-host notification handler (set via
//! [`PluginManager::set_notification_handler`]) runs on the V4 reader thread
//! and must not block. For heavy work, forward the notification to a channel.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::host::{HostApi, HostNotification, PluginNotification};
use crate::process::{PluginError, PluginProcess};
use crate::ribbon::owned::{to_shared_module, SharedCadModule};

/// Callback the host installs to receive plugin-to-host notifications.
pub type NotificationHandler = Arc<dyn Fn(&str, Option<u64>, PluginNotification) + Send + Sync>;

/// Owner of every spawned plugin process.
pub struct PluginManager {
    plugins: Vec<LoadedPlugin>,
    notification_handler: Option<NotificationHandler>,
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
            notification_handler: None,
        }
    }

    /// Install a handler for plugin-to-host notifications. The handler runs on
    /// the V4 reader thread and must not block or allocate heavily; forward to
    /// a channel if heavy work is required. Should be set before [`load`] is
    /// called so notifications from newly loaded plugins are delivered.
    pub fn set_notification_handler(&mut self, handler: NotificationHandler) {
        self.notification_handler = Some(handler);
    }

    /// Spawn `cdylib_path` as a separate plugin process, build its ribbon
    /// module, and store it. Returns the plugin id on success.
    pub fn load(
        &mut self,
        cdylib_path: &Path,
        host: &mut dyn HostApi,
    ) -> Result<String, PluginError> {
        let handler: NotificationHandler = match self.notification_handler.as_ref() {
            Some(h) => Arc::clone(h),
            None => Arc::new(|_id, _cmd, _notif| {}),
        };
        let process = PluginProcess::spawn(cdylib_path, host, handler)?;
        let id = process.id().to_string();
        let name = process.manifest().name.clone();
        let module = to_shared_module(id.clone(), name, process.ribbon().to_vec());
        self.plugins.push(LoadedPlugin {
            process: Arc::new(process),
            module,
        });
        Ok(id)
    }

    /// Fan out a host notification to every alive V4 plugin. Per-process errors
    /// are logged; this is best-effort.
    pub fn broadcast_notification(
        &self,
        command_id: Option<u64>,
        notification: HostNotification,
    ) {
        for p in &self.plugins {
            if !p.process.is_alive() {
                continue;
            }
            if !p.process.is_v4() {
                if crate::process::verbose() {
                    eprintln!(
                        "[plugin] skipping notification for {} (not V4)",
                        p.process.id()
                    );
                }
                continue;
            }
            if let Err(e) = p.process.notify_plugin(command_id, notification.clone()) {
                eprintln!(
                    "[plugin] broadcast_notification failed for {}: {e}",
                    p.process.id()
                );
            }
        }
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

    /// Command names advertised by every alive, non-disabled plugin: each
    /// ribbon tool's command id plus the manifest's `command_prefixes`. The
    /// host merges these into command-line autocomplete so plugin commands are
    /// discoverable by typing (#272).
    pub fn command_names<F: Fn(&str) -> bool>(&self, is_disabled: F) -> Vec<String> {
        let mut out = Vec::new();
        for p in &self.plugins {
            if is_disabled(p.process.id()) || !p.process.is_alive() {
                continue;
            }
            for group in p.process.ribbon() {
                out.append(&mut group.command_ids());
            }
            out.extend(p.process.manifest().command_prefixes.iter().cloned());
        }
        out
    }

    /// Drain any plugin→host requests that have arrived asynchronously across
    /// all loaded plugins. Errors are logged; this is best-effort.
    pub fn drain_requests(
        &self,
        host: &mut dyn HostApi,
        on_start_interactive: &mut dyn FnMut(u64),
    ) {
        for p in &self.plugins {
            if !p.process.is_alive() {
                continue;
            }
            if let Err(e) = p.process.drain_requests(host, on_start_interactive) {
                eprintln!("[plugin] drain_requests failed for {}: {e}", p.process.id());
            }
        }
    }

    /// Drain any plugin stdout/stderr lines that have accumulated across all
    /// loaded plugins. Errors are logged; this is best-effort.
    pub fn drain_io(&self) -> Vec<crate::process::PluginIoLine> {
        let mut out = Vec::new();
        for p in &self.plugins {
            if !p.process.is_alive() {
                continue;
            }
            out.extend(p.process.drain_io());
        }
        out
    }

    /// Shut down and remove the plugin with `id` from the manager.
    ///
    /// Returns `true` if a plugin with that id was found and removed. The
    /// process is killed and waited on so its DLL is released; this is intended
    /// for uninstall, where the caller needs the files to become deletable on
    /// Windows.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(index) = self.plugins.iter().position(|p| p.process.id() == id) else {
            return false;
        };
        let plugin = self.plugins.remove(index);
        plugin
            .process
            .shutdown_and_wait(Duration::from_secs(5))
            .is_some()
    }

    /// Begin asynchronous shutdown of every plugin process.
    ///
    /// Kills every child synchronously on the calling thread and moves the
    /// blocking `wait()` calls into a single detached reaper thread, so host
    /// shutdown is fast regardless of how many plugins are loaded.
    pub fn shutdown_all(&mut self) {
        let plugins = std::mem::take(&mut self.plugins);
        for p in plugins {
            p.process.shutdown();
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
