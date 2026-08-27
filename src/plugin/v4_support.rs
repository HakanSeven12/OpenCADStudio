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
pub fn open_document_view_v4(
    tab_id: u64,
    doc: &acadrust::CadDocument,
) -> Option<DocumentViewInfo> {
    mgr().open(tab_id, doc)
}

/// Close the V4 shared document view for `tab_id`.
pub fn close_document_view_v4(tab_id: u64) {
    mgr().close(tab_id);
}

/// Publish the latest document to the V4 shared view for `tab_id` and broadcast
/// a change notification when successful.
pub fn publish_document_view_v4(tab_id: u64, doc: &acadrust::CadDocument) {
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
pub fn publish_selection_changed_v4(tab_id: u64, handles: Vec<acadrust::Handle>) {
    broadcast(HostNotification::SelectionChangedV4 { tab_id, handles });
}

/// V4 notification handler installed on the plugin manager.
///
/// Forwards REPL status messages to the log; other notifications are handled
/// by the manager's per-plugin routing.
pub fn notification_handler() -> NotificationHandler {
    Arc::new(|plugin_id, _command_id, notification| {
        if let PluginNotification::ReplStatus { status, message } = notification {
            eprintln!("[{plugin_id}] REPL status {status}: {message}");
        }
    })
}

fn broadcast(notification: HostNotification) {
    crate::plugin::external::with_manager(|mgr| {
        mgr.broadcast_notification(None, notification);
    });
}
