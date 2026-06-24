//! Host-side IPC request handler.

use crate::host::HostApi;
use crate::ipc::protocol::{PluginRequest, PluginResponse};

/// Apply one plugin request to the host's `HostApi` implementation.
///
/// `on_start_interactive` is called when the plugin starts an interactive
/// command; the host should install an adapter that sends
/// `HostRequest::InteractiveEvent` back to the plugin process.
pub fn handle_plugin_request(
    host: &mut dyn HostApi,
    req: PluginRequest,
    on_start_interactive: &mut dyn FnMut(u64),
) -> PluginResponse {
    use PluginRequest::*;
    match req {
        PushInfo(msg) => {
            host.push_info(&msg);
            PluginResponse::Ok
        }
        PushOutput(msg) => {
            host.push_output(&msg);
            PluginResponse::Ok
        }
        PushError(msg) => {
            host.push_error(&msg);
            PluginResponse::Ok
        }
        AddEntity(entity) => PluginResponse::Handle(host.add_entity(entity)),
        BumpGeometry => {
            host.bump_geometry();
            PluginResponse::Ok
        }
        ReadRecord { handle, app_name } => {
            PluginResponse::Record(host.read_record(handle, &app_name).cloned())
        }
        WriteRecord { handle, record } => PluginResponse::Bool(host.write_record(handle, record)),
        RemoveRecord { handle, app_name } => {
            PluginResponse::Bool(host.remove_record(handle, &app_name))
        }
        PushUndo { label } => {
            host.push_undo(&label);
            PluginResponse::Ok
        }
        SetDirty => {
            host.set_dirty();
            PluginResponse::Ok
        }
        StartInteractive { command_id } => {
            on_start_interactive(command_id);
            PluginResponse::Ok
        }
        DocumentSnapshot => PluginResponse::Document(host.document().clone()),
        OpenDocumentView => match host.document_view() {
            Some(info) => PluginResponse::DocumentView {
                path: info.path,
                version: info.version,
            },
            None => PluginResponse::Error("shared document view unavailable".to_string()),
        },
    }
}
