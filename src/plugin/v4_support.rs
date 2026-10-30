//! Host-side V4 snapshot and notification support for the Python REPL plugin.

use std::sync::Arc;

use ocs_plugin_api::host::{HostNotification, PluginNotification};
use ocs_plugin_api::host_v4::{manager, HostV4SnapshotManager};
use ocs_plugin_api::process::NotificationHandler;
use ocs_plugin_api::shm::DocumentViewInfo;

fn mgr() -> std::sync::MutexGuard<'static, HostV4SnapshotManager> {
    manager().lock().unwrap()
}

/// Open (or refresh) the V4 shared document view for `tab_id`.
pub fn open_document_view_v4(tab_id: u64, doc: &codec::Document) -> Option<DocumentViewInfo> {
    mgr().open(tab_id, doc)
}

/// Close the V4 shared document view for `tab_id`.
pub fn close_document_view_v4(tab_id: u64) {
    mgr().close(tab_id);
}

/// Publish the latest document to the V4 shared view for `tab_id` and broadcast
/// a change notification when successful.
pub fn publish_document_view_v4(tab_id: u64, doc: &codec::Document) {
    let info = mgr().publish(tab_id, doc);
    if let Some(info) = info {
        broadcast(HostNotification::DocumentChangedV4 {
            tab_id,
            version: info.version,
        });
    }
}

/// Notify V4 plugins that a tab has been closed and clean up its snapshot.
pub fn on_tab_closed(tab_id: u64) {
    close_document_view_v4(tab_id);
    broadcast(HostNotification::DocumentTabClosed { tab_id });
}

/// Broadcast a selection change to V4 plugins.
///
/// `HostNotification::SelectionChangedV4` carries the active tab id and the
/// current selection set. The caller is responsible for only calling this when
/// the selection actually changed; see `OpenCADStudio::notify_plugins_selection_changed`.
pub fn publish_selection_changed_v4(tab_id: u64, handles: Vec<codec::Handle>) {
    broadcast(HostNotification::SelectionChangedV4 { tab_id, handles });
}

/// Broadcast the tab-independent `HostNotification::SelectionChanged` to every
/// loaded native plugin (API v9): the plain-handle variant of the selection
/// event, dispatched at the same fingerprint-guarded message boundary as the
/// tab-scoped one above (`OpenCADStudio::notify_plugins_selection_changed`).
pub fn publish_selection_changed(handles: Vec<codec::Handle>) {
    broadcast(HostNotification::SelectionChanged { handles });
}

/// Broadcast the tab-independent `HostNotification::DocumentChanged` to every
/// loaded native plugin (API v9): the geometry epoch of the active tab's
/// document after a mutation settled, dispatched at the fingerprint-guarded
/// message boundary (`OpenCADStudio::notify_plugins_document_changed`).
pub fn publish_document_changed(version: u64) {
    broadcast(HostNotification::DocumentChanged { version });
}

/// Broadcast the DIRTY REGION payload (API v13): the identity set of what
/// changed since the previous boundary — touched entity handles and
/// (entity, app) record pairs. `DocumentChanged` follows the same
/// fingerprint-guarded boundary; this rides next to it so a plugin's
/// boundary work visits only the touched alignment instead of re-walking
/// its whole store. Empty sets mean "too much changed / unknown — walk
/// everything" (an honest degradation, never a wrong claim).
pub fn publish_document_changed_regions(
    tab_id: u64,
    version: u64,
    entities: Vec<codec::Handle>,
    records: Vec<(codec::Handle, String)>,
) {
    broadcast(HostNotification::DocumentChangedRegions {
        tab_id,
        version,
        entities,
        records,
    });
}

/// Broadcast `HostNotification::DocumentOpened` to every loaded native plugin
/// (API v9): a whole document became resident in a tab — opened from file,
/// created from a template, or loaded by a system open. The counterpart of
/// [`HostNotification::DocumentTabClosed`], so a plugin can re-seed runtime
/// per-document state (e.g. reference-only designations) from load to close.
pub fn publish_document_opened(tab_id: u64) {
    broadcast(HostNotification::DocumentOpened { tab_id });
}

/// Broadcast a change in a tab's active interactive command.
pub fn publish_command_state_changed(tab_id: u64, command: Option<String>) {
    broadcast(HostNotification::CommandStateChanged { tab_id, command });
}

/// Broadcast drawing changes independently of shared-view subscriptions.
pub fn publish_drawing_changed(tab_id: u64, epoch: u64) {
    broadcast(HostNotification::DrawingChanged { tab_id, epoch });
}

/// Broadcast a panel control interaction or button click to plugins.
pub fn publish_panel_action(
    panel_id: String,
    control_id: String,
    action: String,
    value: Option<ocs_plugin_api::dialog::DialogValue>,
) {
    broadcast(HostNotification::PanelAction {
        panel_id,
        control_id,
        action,
        value,
    });
}

/// V4 notification handler installed on the plugin manager.
///
/// Forwards REPL status messages to the log; other notifications are handled
/// by the manager's per-plugin routing.
pub fn notification_handler() -> NotificationHandler {
    Arc::new(|plugin_id, _command_id, notification| {
        match notification {
            PluginNotification::ReplStatus { status, message } => {
                eprintln!("[{plugin_id}] REPL status {status}: {message}");
            }
            // API v13 attach determinism: the runner's main loop entered and
            // is draining — its notification channel is verifiably live.
            // The ack is ENQUEUED here (this handler runs on the V4 reader
            // thread and must never block: taking the manager lock from
            // here would deadlock the very handshakes the ack rides — the
            // boot loader holds the lock while spawning). The main thread
            // processes the queue at its next message boundary
            // (`DrainPluginRequests`), attaching the channel and flushing
            // everything deferred during the boot window; the broadcasts
            // keep deferring until then, so nothing is lost.
            PluginNotification::ChannelReady => {
                CHANNEL_READY_QUEUE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(plugin_id.to_string());
            }
            _ => {}
        }
    })
}

fn broadcast(notification: HostNotification) {
    #[cfg(test)]
    record_test_dispatch(&notification);
    crate::plugin::external::with_manager(|mgr| {
        mgr.broadcast_notification(None, notification);
    });
}

// ── ChannelReady ack queue (API v13) ────────────────────────────────────────
//
// The reader-thread notification handler pushes attach acks here; the main
// thread drains them at message boundaries (see `drain_channel_ready_acks`),
// where taking the manager lock is safe. PROCESS-GLOBAL by necessity: the
// producer is a reader thread, the consumer the main thread. Stale acks for
// plugins removed meanwhile are ignored by name; an empty (handshake-window)
// ack resolves by elimination among unattached candidates.

static CHANNEL_READY_QUEUE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Process every queued ChannelReady ack on the main thread (API v13): mark
/// the named plugin's notification channel live and flush its deferred boot
/// window. An empty id is the handshake-window placeholder — the manager's
/// attach-by-elimination resolves it. Called from the `DrainPluginRequests`
/// boundary.
pub fn drain_channel_ready_acks() {
    let acks = std::mem::take(
        &mut *CHANNEL_READY_QUEUE
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    for plugin_id in acks {
        crate::plugin::external::with_manager(|manager| {
            let flushed = manager.attach_notification_channel(&plugin_id);
            match flushed {
                Some(count) => {
                    eprintln!(
                        "[{plugin_id}] notification channel attached \
                         ({count} deferred flushed)"
                    );
                }
                None => {
                    eprintln!("[{plugin_id}] ChannelReady from an unloaded plugin; ignored");
                }
            }
        });
    }
}

// ── Test-only dispatch observation ──────────────────────────────────────────
//
// App-level tests assert boundary dispatch behaviour through this per-thread
// log: `cargo test` runs each test on its own thread, so a thread-local log
// records exactly the notifications this test's app instance broadcast,
// without cross-test interference. One recorded entry stands for one fan-out
// to every loaded native plugin (the manager's broadcast), so counts here are
// counts per plugin.

#[cfg(test)]
thread_local! {
    static TEST_DISPATCH_LOG: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn record_test_dispatch(notification: &HostNotification) {
    let kind = match notification {
        HostNotification::DocumentChanged { version } => {
            format!("DocumentChanged({version})")
        }
        HostNotification::SelectionChanged { handles } => {
            format!("SelectionChanged({} handles)", handles.len())
        }
        HostNotification::DocumentOpened { tab_id } => format!("DocumentOpened({tab_id})"),
        HostNotification::DocumentTabClosed { tab_id } => format!("DocumentTabClosed({tab_id})"),
        HostNotification::DrawingChanged { tab_id, epoch } => {
            format!("DrawingChanged({tab_id}, {epoch})")
        }
        HostNotification::SelectionChangedV4 { tab_id, .. } => {
            format!("SelectionChangedV4({tab_id})")
        }
        HostNotification::DocumentChangedV4 { tab_id, version } => {
            format!("DocumentChangedV4({tab_id}, {version})")
        }
        HostNotification::CommandStateChanged { tab_id, .. } => {
            format!("CommandStateChanged({tab_id})")
        }
        HostNotification::DocumentChangedRegions {
            entities, records, ..
        } => format!(
            "DocumentChangedRegions({} entities, {} records)",
            entities.len(),
            records.len()
        ),
        _ => "Other".to_string(),
    };
    TEST_DISPATCH_LOG.with(|log| log.borrow_mut().push(kind));
}

/// Drain this thread's recorded plugin-notification broadcasts (test-only).
#[cfg(test)]
pub fn take_test_dispatch_log() -> Vec<String> {
    TEST_DISPATCH_LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
}
