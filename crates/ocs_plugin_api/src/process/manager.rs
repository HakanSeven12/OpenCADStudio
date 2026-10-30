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
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::host::{HostApi, HostNotification, PluginNotification};
use crate::process::{PluginError, PluginProcess};
use crate::ribbon::owned::{to_shared_module, OwnedEntityClasses, SharedCadModule};

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
    /// API v13 notification-attach state (interior: broadcasts reach the
    /// manager through a shared reference): notifications to this process
    /// are deferred (parked) until its runner acks
    /// `PluginNotification::ChannelReady` — the main loop entered and is
    /// draining — or the boot grace window expires (an older runner never
    /// acks; the host cannot distinguish it from a slow boot, so after the
    /// grace the queue flushes, exactly like the dispatch table's boot
    /// retry heals the command side).
    attach: std::sync::Mutex<NotificationAttach>,
}

/// Per-plugin notification-attach bookkeeping (API v13).
#[derive(Default)]
struct NotificationAttach {
    attached: bool,
    /// Boot grace deadline (set at spawn); `None` once attached or expired.
    grace_deadline: Option<std::time::Instant>,
    /// Set when the grace expired without an ack (logged once).
    grace_expired: bool,
    /// Deferred boot-window notifications, flushed on attach or grace.
    pending: Vec<(Option<u64>, HostNotification)>,
}

/// The boot-grace window a runner has to ack `ChannelReady` before the
/// host treats it as an older (never-acking) runner and flushes anyway.
fn notification_boot_grace() -> std::time::Duration {
    std::time::Duration::from_millis(NOTIFICATION_BOOT_GRACE_MS.load(Ordering::Relaxed))
}

static NOTIFICATION_BOOT_GRACE_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(60_000);

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
            attach: std::sync::Mutex::new(NotificationAttach {
                grace_deadline: Some(std::time::Instant::now() + notification_boot_grace()),
                ..Default::default()
            }),
        });
        Ok(id)
    }

    /// Fan out a host notification to every alive V4 plugin. Per-process errors
    /// are logged; this is best-effort.
    ///
    /// API v13 attach determinism: a process whose runner has not acked
    /// `PluginNotification::ChannelReady` yet gets the notification DEFERRED
    /// (parked, flushed on attach) — a fresh boot's notifications can never
    /// race the runner's startup and vanish. The boot grace bounds the
    /// deferral for older runners that never ack: past the deadline the
    /// queue flushes once (logged), and the process is treated as attached
    /// from then on.
    pub fn broadcast_notification(&self, command_id: Option<u64>, notification: HostNotification) {
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
            let flush_now = {
                let mut attach = p.attach.lock().unwrap_or_else(|e| e.into_inner());
                if attach.attached {
                    false
                } else if attach
                    .grace_deadline
                    .is_some_and(|deadline| std::time::Instant::now() >= deadline)
                {
                    // Boot grace expired: an older runner never acks. Flush
                    // the parked boot window once (logged) and treat the
                    // channel as live from here on.
                    if !attach.grace_expired {
                        attach.grace_expired = true;
                        eprintln!(
                            "[plugin] {} did not ack ChannelReady within the boot grace; \
                             flushing {} deferred notifications (older runner assumed)",
                            p.process.id(),
                            attach.pending.len()
                        );
                    }
                    attach.attached = true;
                    attach.grace_deadline = None;
                    let pending = std::mem::take(&mut attach.pending);
                    for (cid, deferred) in pending {
                        if let Err(e) = p.process.notify_plugin(cid, deferred) {
                            eprintln!(
                                "[plugin] deferred notification flush failed for {}: {e}",
                                p.process.id()
                            );
                        }
                    }
                    // The new notification follows below through the normal
                    // send path.
                    true
                } else {
                    attach.pending.push((command_id, notification.clone()));
                    if crate::process::verbose() {
                        eprintln!(
                            "[plugin] deferring notification for {} until ChannelReady \
                             ({} parked)",
                            p.process.id(),
                            attach.pending.len()
                        );
                    }
                    continue;
                }
            };
            let _ = flush_now;
            if let Err(e) = p.process.notify_plugin(command_id, notification.clone()) {
                eprintln!(
                    "[plugin] broadcast_notification failed for {}: {e}",
                    p.process.id()
                );
            }
        }
    }

    /// Mark a plugin's notification channel verifiably live and flush every
    /// notification deferred during its boot window (API v13). Called when
    /// the runner acks `PluginNotification::ChannelReady`. Returns the number
    /// flushed; `None` when no loaded plugin carries that id.
    pub fn attach_notification_channel(&self, plugin_id: &str) -> Option<usize> {
        // A handshake-window ack may reach the reader thread before the spawn
        // thread has pinned the manifest id (the empty placeholder id): the
        // ack itself is what proves the channel is live, so attach by
        // elimination — but only when exactly one plugin waits. With several
        // booting at once the ack is ambiguous; the boot grace covers them.
        if plugin_id.is_empty() {
            let mut waiting = self
                .plugins
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.attach.lock().unwrap_or_else(|e| e.into_inner()).attached)
                .map(|(index, _)| index);
            return match (waiting.next(), waiting.next()) {
                (Some(index), None) => Some(self.attach_by_index(index)),
                _ => Some(0),
            };
        }
        let index = self
            .plugins
            .iter()
            .position(|p| p.process.id() == plugin_id)?;
        Some(self.attach_by_index(index))
    }

    /// Attach one plugin by index (shared helper).
    fn attach_by_index(&self, index: usize) -> usize {
        let p = &self.plugins[index];
        let pending = {
            let mut attach = p.attach.lock().unwrap_or_else(|e| e.into_inner());
            if attach.attached {
                // A second ack (runner restart on the same process, a
                // duplicate frame): idempotent, nothing deferred.
                return 0;
            }
            attach.attached = true;
            attach.grace_deadline = None;
            std::mem::take(&mut attach.pending)
        };
        let flushed = pending.len();
        if flushed > 0 {
            eprintln!(
                "[plugin] {} acked ChannelReady; flushing {} deferred boot notifications",
                p.process.id(),
                flushed
            );
        }
        for (command_id, notification) in pending {
            if let Err(e) = p.process.notify_plugin(command_id, notification) {
                eprintln!(
                    "[plugin] deferred notification flush failed for {}: {e}",
                    p.process.id()
                );
            }
        }
        flushed
    }

    /// Whether a plugin's notification channel has been acked live (or the
    /// boot grace has passed for an older runner).
    pub fn notification_channel_attached(&self, plugin_id: &str) -> Option<bool> {
        self.plugins
            .iter()
            .find(|p| p.process.id() == plugin_id)
            .map(|p| p.attach.lock().unwrap_or_else(|e| e.into_inner()).attached)
    }

    /// Deferred-boot-notification count for one plugin (diagnostics).
    pub fn pending_notification_count(&self, plugin_id: &str) -> Option<usize> {
        self.plugins
            .iter()
            .find(|p| p.process.id() == plugin_id)
            .map(|p| {
                p.attach
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .pending
                    .len()
            })
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

    /// Ids of loaded plugins whose processes are currently alive (API v11).
    pub fn alive_ids(&self) -> Vec<String> {
        self.plugins
            .iter()
            .filter(|p| p.process.is_alive())
            .map(|p| p.process.id().to_string())
            .collect()
    }

    /// Live endpoint of one loaded plugin for custom-grip event routing (API
    /// v11): its process handle, its declared API version, and whether the
    /// process is alive. `None` when no plugin with that id is loaded.
    pub fn grip_endpoint(&self, id: &str) -> Option<(Arc<PluginProcess>, u32, bool)> {
        self.plugins.iter().find(|p| p.process.id() == id).map(|p| {
            (
                Arc::clone(&p.process),
                p.process.manifest().api_version,
                p.process.is_alive(),
            )
        })
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

    /// Entity-class declarations of every alive, non-disabled plugin that
    /// made one (API v10), in load order, as `(plugin id, declaration)`.
    ///
    /// The host consults this at display time only — never cached — so the
    /// result always mirrors the live loaded set: a plugin that failed to
    /// load, is disabled, was removed, or whose process has died contributes
    /// nothing, and entities it stamped fall back to their native type names.
    pub fn entity_class_declarations<F: Fn(&str) -> bool>(
        &self,
        is_disabled: F,
    ) -> Vec<(String, OwnedEntityClasses)> {
        let mut out = Vec::new();
        for p in &self.plugins {
            if is_disabled(p.process.id()) || !p.process.is_alive() {
                continue;
            }
            if let Some(classes) = p.process.manifest().entity_classes.clone() {
                out.push((p.process.id().to_string(), classes));
            }
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
            fn layer_name(&self, _handle: codec::Handle) -> Option<&str> {
                None
            }
            fn app_id_name(&self, _handle: codec::Handle) -> Option<&str> {
                None
            }
        }

        struct DummyHost;
        impl HostApi for DummyHost {
            fn tab_index(&self) -> usize {
                0
            }
            fn document(&self) -> &codec::Document {
                panic!("not used")
            }
            fn document_mut(&mut self) -> &mut codec::Document {
                panic!("not used")
            }
            fn document_reader(&self) -> Box<dyn crate::host::DocumentReader + '_> {
                Box::new(EmptyReader)
            }
            fn add_entity(&mut self, _entity: codec::EntityType) -> codec::Handle {
                panic!("not used")
            }
            fn bump_geometry(&mut self) {}
            fn read_record(
                &self,
                _handle: codec::Handle,
                _app_name: &str,
            ) -> Option<&codec::xdata::ExtendedDataRecord> {
                None
            }
            fn write_record(
                &mut self,
                _handle: codec::Handle,
                _record: codec::xdata::ExtendedDataRecord,
            ) -> bool {
                false
            }
            fn remove_record(&mut self, _handle: codec::Handle, _app_name: &str) -> bool {
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
            ) -> Option<&mut (dyn std::any::Any + Send + Sync)> {
                None
            }
        }

        let manager = PluginManager::new();
        let mut host = DummyHost;
        let result = manager.dispatch(&mut host, "FOO", |_| false);
        assert!(!result.handled);
        assert!(result.started.is_none());
    }

    fn manifest_with_entity_classes(id: &str) -> crate::ribbon::owned::OwnedPluginManifest {
        crate::ribbon::owned::OwnedPluginManifest {
            id: id.to_string(),
            name: format!("{id} name"),
            version: "0.1.0".to_string(),
            description: "test".to_string(),
            api_version: 10,
            ribbon_order: 0,
            xdata_apps: vec![],
            command_prefixes: vec![],
            entity_classes: Some(OwnedEntityClasses {
                app_id: "CVIFC".to_string(),
                class_prefix: "CVIFC-CLASS:".to_string(),
                labels: [("LINE".to_string(), "Linear Element".to_string())]
                    .into_iter()
                    .collect(),
            }),
        }
    }

    fn plain_manifest(id: &str) -> crate::ribbon::owned::OwnedPluginManifest {
        crate::ribbon::owned::OwnedPluginManifest {
            id: id.to_string(),
            name: format!("{id} name"),
            version: "0.1.0".to_string(),
            description: "test".to_string(),
            api_version: 10,
            ribbon_order: 0,
            xdata_apps: vec![],
            command_prefixes: vec![],
            entity_classes: None,
        }
    }

    #[test]
    fn entity_class_declarations_skip_disabled_and_undeclaring_plugins() {
        let mut manager = PluginManager::new();
        manager.plugins.push(LoadedPlugin {
            process: Arc::new(PluginProcess::fake(manifest_with_entity_classes(
                "test.live",
            ))),
            module: to_shared_module("test.live".into(), "Live".into(), vec![]),
            attach: std::sync::Mutex::new(NotificationAttach {
                grace_deadline: Some(std::time::Instant::now() + notification_boot_grace()),
                ..Default::default()
            }),
        });
        manager.plugins.push(LoadedPlugin {
            process: Arc::new(PluginProcess::fake(plain_manifest("test.plain"))),
            module: to_shared_module("test.plain".into(), "Plain".into(), vec![]),
            attach: std::sync::Mutex::new(NotificationAttach {
                grace_deadline: Some(std::time::Instant::now() + notification_boot_grace()),
                ..Default::default()
            }),
        });
        manager.plugins.push(LoadedPlugin {
            process: Arc::new(PluginProcess::fake(manifest_with_entity_classes(
                "test.off",
            ))),
            module: to_shared_module("test.off".into(), "Off".into(), vec![]),
            attach: std::sync::Mutex::new(NotificationAttach {
                grace_deadline: Some(std::time::Instant::now() + notification_boot_grace()),
                ..Default::default()
            }),
        });

        // Alive + enabled + declaring → reported, with the declaration intact.
        let declarations = manager.entity_class_declarations(|id| id == "test.off");
        assert_eq!(
            declarations.len(),
            1,
            "only the live, enabled plugin reports"
        );
        assert_eq!(declarations[0].0, "test.live");
        assert_eq!(declarations[0].1.app_id, "CVIFC");
        assert_eq!(declarations[0].1.class_prefix, "CVIFC-CLASS:");
        assert_eq!(
            declarations[0].1.labels.get("LINE").map(String::as_str),
            Some("Linear Element")
        );

        // Everything disabled → nothing contributes.
        assert!(manager.entity_class_declarations(|_| true).is_empty());
    }

    #[test]
    fn entity_class_declarations_revert_when_the_plugin_process_dies() {
        let mut manager = PluginManager::new();
        manager.plugins.push(LoadedPlugin {
            process: Arc::new(PluginProcess::fake(manifest_with_entity_classes(
                "test.live",
            ))),
            module: to_shared_module("test.live".into(), "Live".into(), vec![]),
            attach: std::sync::Mutex::new(NotificationAttach {
                grace_deadline: Some(std::time::Instant::now() + notification_boot_grace()),
                ..Default::default()
            }),
        });
        assert_eq!(
            manager.entity_class_declarations(|_| false).len(),
            1,
            "declaration visible while the process is alive"
        );

        manager.plugins[0]
            .process
            .shutdown_and_wait(Duration::from_secs(5))
            .expect("child shut down");

        assert!(
            manager.entity_class_declarations(|_| false).is_empty(),
            "a dead plugin process must contribute no declarations"
        );
    }
}
