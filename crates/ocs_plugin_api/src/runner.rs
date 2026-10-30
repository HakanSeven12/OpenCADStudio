//! Out-of-process plugin runner logic.
//!
//! This module is used by the host when it spawns itself in runner mode
//! (`--ocs-plugin-runner <socket> <cdylib>`). Keeping the runner code inside
//! `ocs_plugin_api` means the host only needs to know the CLI contract, not the
//! internal plugin-loading and IPC details.
//!
//! Entry points and loops:
//!
//! - [`run`] — loads the cdylib with `libloading`, connects back to the host,
//!   and dispatches to either the V2/V3 loop or the V4 loop based on the plugin's
//!   declared API version.
//! - `run_v3` — synchronous request/response loop over [`crate::ipc::client::IpcClient`].
//! - `run_v4` — multiplexed frame loop over [`crate::ipc::v4::client::V4Client`];
//!   drains notifications and dispatches host requests.
//!
//! Panics inside plugin callbacks are caught by `catch_unwind` and converted to
//! error responses so a buggy plugin callback does not tear down the runner.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use crate::host::{BuiltinPlugin, CommandStep, InteractiveCommand};
use crate::ipc::client::{InteractiveRegistry, IpcClient, PluginHostApi};
use crate::ipc::protocol::{
    HostRequest, HostResponse, HostToPlugin, InteractiveEvent, PluginToHost, PLUGIN_TOKEN_ENV,
};
use crate::ipc::transport::{recv, send};
use crate::ipc::v4::client::{RunnerFrame, V4Client};
use crate::ribbon::owned::{OwnedEntityClasses, OwnedPluginManifest, OwnedRibbonGroup};

/// Entry point for the plugin runner child process.
///
/// Connects back to the host on `socket_name`, loads the cdylib at
/// `cdylib_path`, and runs the request loop until the host sends `Shutdown`.
/// This function never returns normally; it exits the process on shutdown or
/// fatal error so the child does not fall through to the host's GUI main.
pub fn run(socket_name: &str, cdylib_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[runner] starting for {cdylib_path:?} on {socket_name}");
    // Let the plugin locate its own cdylib so it can copy it into generated
    // workspaces (e.g., as a Python extension). This is set only in the child
    // runner process, so it does not leak into the host environment.
    std::env::set_var("OCS_PLUGIN_CDYLIB_PATH", cdylib_path.as_os_str());
    if let Some(code) = unsafe { run_self_hosted(socket_name, cdylib_path)? } {
        std::process::exit(code);
    }
    let (version, plugin) = unsafe { load_plugin(cdylib_path)? };
    run_with_plugin(socket_name, version, plugin)
}

/// The runner loop for `plugin`. A plugin of API v14 and later calls it from
/// its own `ocs_plugin_run` (see `export_plugin!`), so the loop, the plugin and
/// every Rust type they share come from one build: the plugin's.
pub fn run_with_plugin(
    socket_name: &str,
    version: u32,
    mut plugin: Box<dyn BuiltinPlugin>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::version_info::{wire_schema, SELF_HOSTED_API_VERSION, WIRE_SCHEMA_ENV};
    if version >= SELF_HOSTED_API_VERSION {
        if let Ok(host) = std::env::var(WIRE_SCHEMA_ENV) {
            if host != wire_schema() {
                return Err(format!(
                    "plugin reads codec types of wire schema {}, but the host writes {host}; \
                     rebuild the plugin against this host's ocs_plugin_api",
                    wire_schema()
                )
                .into());
            }
        }
    }
    let interactive: InteractiveRegistry = Rc::new(RefCell::new(HashMap::new()));

    let token = match std::env::var(PLUGIN_TOKEN_ENV) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("[runner] missing {PLUGIN_TOKEN_ENV}; exiting");
            std::process::exit(1);
        }
    };

    if version >= 4 {
        let client = V4Client::connect_handshake(socket_name, &token)?;
        if version >= 5 {
            invoke_on_load(&mut *plugin, &client, &interactive)?;
        }
        run_v4(&mut *plugin, &interactive, client, version)
    } else {
        let client = IpcClient::connect(socket_name)?;
        eprintln!("[runner] connected to host");
        client.send_handshake(&token)?;
        // V2/V3 plugins receive HostApi only during dispatch, so on_load is not
        // meaningful there; skip it to keep the old protocol unchanged.
        run_v3(&*plugin, &interactive, client)
    }
}

fn invoke_on_load(
    plugin: &mut dyn BuiltinPlugin,
    client: &V4Client,
    interactive: &InteractiveRegistry,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut proxy = client.plugin_host_api(0, interactive.clone());
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.on_load(&mut proxy)))
        .map_err(|_| std::io::Error::other("plugin on_load() panicked"))?;
    Ok(())
}

fn run_v3(
    plugin: &dyn BuiltinPlugin,
    interactive: &InteractiveRegistry,
    client: IpcClient,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let msg: HostToPlugin = recv(&mut client.stream_ref())?;
        eprintln!("[runner] host -> runner: {msg:?}");
        match msg {
            HostToPlugin::Request(req) => {
                let resp = handle_host_request(plugin, interactive, &client, req);
                eprintln!("[runner] runner -> host: {resp:?}");
                send(&mut client.stream_ref(), &PluginToHost::Response(resp))?;
            }
            HostToPlugin::Response(_) => {
                eprintln!("[runner] unexpected HostToPlugin::Response");
            }
        }
    }
}

fn run_v4(
    plugin: &mut dyn BuiltinPlugin,
    interactive: &InteractiveRegistry,
    client: V4Client,
    api_version: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    // API v13: the ChannelReady attach ack is sent AFTER the first fully
    // served request — at that point the manifest handshake is complete on
    // the host side GetManifest is the first request served, so the host has
    // pinned this process's real manifest id and the reader-thread ack
    // attaches under the right name. A pre-handshake ack would arrive while
    // the host still addresses the process by its empty placeholder id and
    // be ignored.
    let mut boot_acked = false;
    loop {
        // Drain any host notifications and dispatch them to the plugin's
        // state-only callback. Panics are caught so a bad callback does not
        // tear down the connection.
        while let Some((command_id, notification)) = client.try_recv_notification() {
            if notification_api_version(&notification) > api_version {
                continue;
            }
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.on_notification(command_id, notification)
            }));
        }

        match client.recv_runner_frame_timeout(std::time::Duration::from_millis(50)) {
            Ok(RunnerFrame::Request { id, payload }) => {
                if let Some(resp) = handle_host_request_v4(
                    &mut *plugin,
                    interactive,
                    &client,
                    id,
                    api_version,
                    payload,
                ) {
                    client.send_response(id, resp)?;
                    // API v13: the notification subscription is verifiably
                    // live — a request has been fully served, so the loop is
                    // draining and the host knows this process's id. The host
                    // defers a process's boot-window broadcasts until this
                    // ack (and flushes them then): a fresh boot's
                    // notifications can never race the runner's startup.
                    if !boot_acked {
                        boot_acked = true;
                        client.notify_plugin(None, crate::host::PluginNotification::ChannelReady);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

/// The API version that added a notification variant. A cdylib built against
/// an older API has no such variant in its enum, so handing it one would be
/// undefined behaviour; the runner drops it instead.
fn notification_api_version(notification: &crate::host::HostNotification) -> u32 {
    use crate::host::HostNotification as N;
    match notification {
        N::DocumentChangedRegions { .. } => 13,
        N::DocumentOpened { .. } => 9,
        N::CommandStateChanged { .. } | N::DrawingChanged { .. } => 7,
        N::DocumentChangedV4 { .. }
        | N::DocumentTabClosed { .. }
        | N::SelectionChangedV4 { .. } => 4,
        _ => 0,
    }
}

fn handle_host_request(
    plugin: &dyn BuiltinPlugin,
    interactive: &InteractiveRegistry,
    client: &IpcClient,
    req: HostRequest,
) -> HostResponse {
    match req {
        HostRequest::GetManifest => {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.manifest())) {
                Ok(m) => HostResponse::Manifest(m.into()),
                Err(_) => HostResponse::Error("plugin manifest() panicked".to_string()),
            }
        }
        HostRequest::GetRibbon => {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.ribbon())) {
                Ok(groups) => HostResponse::Ribbon(
                    groups
                        .ribbon_groups()
                        .iter()
                        .map(OwnedRibbonGroup::from)
                        .collect(),
                ),
                Err(_) => HostResponse::Error("plugin ribbon() panicked".to_string()),
            }
        }
        HostRequest::Dispatch { cmd } => {
            // The host supplies the active tab index as part of the dispatch
            // context. We cache it inside PluginHostApi.
            let mut proxy = PluginHostApi::new(client.clone(), 0, interactive.clone());
            let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.dispatch(&mut proxy, &cmd)
            }));
            match handled {
                Ok(b) => HostResponse::Bool(b),
                Err(_) => HostResponse::Error("plugin dispatch panicked".to_string()),
            }
        }
        HostRequest::InteractiveEvent { command_id, event } => {
            if matches!(event, InteractiveEvent::Cancel) {
                interactive.borrow_mut().remove(&command_id);
                return HostResponse::CommandStep(Box::new(CommandStep::Cancel));
            }
            let (step, done) = {
                let mut registry = interactive.borrow_mut();
                let Some(cmd) = registry.get_mut(&command_id) else {
                    return HostResponse::Error(format!(
                        "unknown interactive command {command_id}"
                    ));
                };
                let cmd_ref: &mut dyn InteractiveCommand = cmd.as_mut();
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match event {
                    InteractiveEvent::Point(pt) => cmd_ref.on_point(pt),
                    InteractiveEvent::Enter => cmd_ref.on_enter(),
                    InteractiveEvent::ObjectPick { handle, pt } => {
                        cmd_ref.on_object_pick(handle, pt)
                    }
                    InteractiveEvent::SelectionComplete(handles) => {
                        cmd_ref.on_selection_complete(handles)
                    }
                    InteractiveEvent::Cancel => CommandStep::Cancel,
                    InteractiveEvent::DialogResponse(res) => cmd_ref.on_dialog_response(res),
                }));
                match res {
                    Ok(s) => {
                        let is_done = matches!(
                            s,
                            CommandStep::Done
                                | CommandStep::Cancel
                                | CommandStep::CommitAndEnd(_)
                                | CommandStep::CommitManyAndEnd(_)
                        );
                        (Ok(s), is_done)
                    }
                    Err(_) => (Err(()), true),
                }
            };
            if done {
                interactive.borrow_mut().remove(&command_id);
            }
            match step {
                Ok(s) => HostResponse::CommandStep(Box::new(s)),
                Err(_) => HostResponse::Error("interactive command panicked".to_string()),
            }
        }
        HostRequest::DropInteractive { command_id } => {
            interactive.borrow_mut().remove(&command_id);
            HostResponse::Bool(true)
        }
        HostRequest::GetPrompt { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cmd.prompt()))
                })
            };
            match result {
                Some(Ok(s)) => HostResponse::Text(s),
                Some(Err(_)) => HostResponse::Error("prompt() panicked".to_string()),
                None => HostResponse::Error(format!("unknown interactive command {command_id}")),
            }
        }
        HostRequest::NeedsEntityPick { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.needs_object_pick()
                    }))
                })
            };
            match result {
                Some(Ok(b)) => HostResponse::Bool(b),
                Some(Err(_)) => HostResponse::Error("needs_object_pick() panicked".to_string()),
                None => HostResponse::Error(format!("unknown interactive command {command_id}")),
            }
        }
        HostRequest::IsSelectionGathering { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.is_selection_gathering()
                    }))
                })
            };
            match result {
                Some(Ok(b)) => HostResponse::Bool(b),
                Some(Err(_)) => HostResponse::Error("is_selection_gathering() panicked".to_string()),
                None => HostResponse::Error(format!("unknown interactive command {command_id}")),
            }
        }
        HostRequest::AcceptsDragSelection { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.accepts_drag_selection()
                    }))
                })
            };
            match result {
                Some(Ok(b)) => HostResponse::Bool(b),
                Some(Err(_)) => HostResponse::Error("accepts_drag_selection() panicked".to_string()),
                None => HostResponse::Error(format!("unknown interactive command {command_id}")),
            }
        }
        HostRequest::SelectionRejectsLockedLayers { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.selection_rejects_locked_layers()
                    }))
                })
            };
            match result {
                Some(Ok(b)) => HostResponse::Bool(b),
                Some(Err(_)) => HostResponse::Error("selection_rejects_locked_layers() panicked".to_string()),
                None => HostResponse::Error(format!("unknown interactive command {command_id}")),
            }
        }
        HostRequest::CursorMove { command_id, pt } => {
            let result = {
                let mut registry = interactive.borrow_mut();
                registry.get_mut(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.on_cursor_move(pt)
                    }))
                })
            };
            match result {
                Some(Ok(wires)) => HostResponse::PreviewWires(wires),
                Some(Err(_)) => HostResponse::PreviewWires(Vec::new()),
                None => HostResponse::PreviewWires(Vec::new()),
            }
        }
        HostRequest::GetModalDialog { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).and_then(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.modal_dialog()
                    }))
                    .ok()
                    .flatten()
                })
            };
            HostResponse::ModalDialog(result)
        }
        HostRequest::ExecuteCode { .. } => {
            HostResponse::Error("ExecuteCode requires V4 protocol".to_string())
        }
        HostRequest::CustomGripEvent { .. } => {
            HostResponse::Error("custom grips require the V4 protocol".to_string())
        }
        HostRequest::Shutdown => {
            // Return a positive Ack so the host knows we received the request.
            // run_v3 will then observe the closed stream, break its loop, and
            // return, allowing the plugin object to be dropped and clean up
            // spawned child processes before the runner process exits.
            HostResponse::Bool(true)
        }
    }
}

fn handle_host_request_v4(
    plugin: &mut dyn BuiltinPlugin,
    interactive: &InteractiveRegistry,
    client: &V4Client,
    id: u64,
    api_version: u32,
    req: HostRequest,
) -> Option<HostResponse> {
    match req {
        HostRequest::GetManifest => {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.manifest())) {
                Ok(m) => {
                    let mut manifest = OwnedPluginManifest::from(m);
                    // API v10: fold the plugin's entity-class declarations into
                    // the registered manifest. The call is guarded by the ABI
                    // version the cdylib was actually built against (the
                    // exported symbol, not the manifest's claim): an older
                    // cdylib's vtable predates the slot, so touching it would
                    // be undefined behavior. Its entities simply keep their
                    // native type names.
                    if api_version >= 10 {
                        let classes =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                plugin.entity_classes()
                            }));
                        manifest.entity_classes = match classes {
                            Ok(classes) => classes.map(OwnedEntityClasses::from),
                            Err(_) => {
                                return Some(HostResponse::Error(
                                    "plugin entity_classes() panicked".to_string(),
                                ))
                            }
                        };
                    }
                    Some(HostResponse::Manifest(manifest))
                }
                Err(_) => Some(HostResponse::Error(
                    "plugin manifest() panicked".to_string(),
                )),
            }
        }
        HostRequest::GetRibbon => {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.ribbon())) {
                Ok(groups) => Some(HostResponse::Ribbon(
                    groups
                        .ribbon_groups()
                        .iter()
                        .map(OwnedRibbonGroup::from)
                        .collect(),
                )),
                Err(_) => Some(HostResponse::Error("plugin ribbon() panicked".to_string())),
            }
        }
        HostRequest::Dispatch { cmd } => {
            let mut proxy = client.plugin_host_api(0, interactive.clone());
            let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.dispatch(&mut proxy, &cmd)
            }));
            match handled {
                Ok(b) => Some(HostResponse::Bool(b)),
                Err(_) => Some(HostResponse::Error("plugin dispatch panicked".to_string())),
            }
        }
        HostRequest::InteractiveEvent { command_id, event } => {
            if matches!(event, InteractiveEvent::Cancel) {
                interactive.borrow_mut().remove(&command_id);
                return Some(HostResponse::CommandStep(Box::new(CommandStep::Cancel)));
            }
            let (step, done) = {
                let mut registry = interactive.borrow_mut();
                let Some(cmd) = registry.get_mut(&command_id) else {
                    return Some(HostResponse::Error(format!(
                        "unknown interactive command {command_id}"
                    )));
                };
                let cmd_ref: &mut dyn InteractiveCommand = cmd.as_mut();
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match event {
                    InteractiveEvent::Point(pt) => cmd_ref.on_point(pt),
                    InteractiveEvent::Enter => cmd_ref.on_enter(),
                    InteractiveEvent::ObjectPick { handle, pt } => {
                        cmd_ref.on_object_pick(handle, pt)
                    }
                    InteractiveEvent::SelectionComplete(handles) => {
                        cmd_ref.on_selection_complete(handles)
                    }
                    InteractiveEvent::Cancel => CommandStep::Cancel,
                    InteractiveEvent::DialogResponse(res) => cmd_ref.on_dialog_response(res),
                }));
                match res {
                    Ok(s) => {
                        let is_done = matches!(
                            s,
                            CommandStep::Done
                                | CommandStep::Cancel
                                | CommandStep::CommitAndEnd(_)
                                | CommandStep::CommitManyAndEnd(_)
                        );
                        (Ok(s), is_done)
                    }
                    Err(_) => (Err(()), true),
                }
            };
            if done {
                interactive.borrow_mut().remove(&command_id);
            }
            match step {
                Ok(s) => Some(HostResponse::CommandStep(Box::new(s))),
                Err(_) => Some(HostResponse::Error(
                    "interactive command panicked".to_string(),
                )),
            }
        }
        HostRequest::DropInteractive { command_id } => {
            interactive.borrow_mut().remove(&command_id);
            Some(HostResponse::Bool(true))
        }
        HostRequest::GetPrompt { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cmd.prompt()))
                })
            };
            match result {
                Some(Ok(s)) => Some(HostResponse::Text(s)),
                Some(Err(_)) => Some(HostResponse::Error("prompt() panicked".to_string())),
                None => Some(HostResponse::Error(format!(
                    "unknown interactive command {command_id}"
                ))),
            }
        }
        HostRequest::NeedsEntityPick { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.needs_object_pick()
                    }))
                })
            };
            match result {
                Some(Ok(b)) => Some(HostResponse::Bool(b)),
                Some(Err(_)) => Some(HostResponse::Error(
                    "needs_object_pick() panicked".to_string(),
                )),
                None => Some(HostResponse::Error(format!(
                    "unknown interactive command {command_id}"
                ))),
            }
        }
        HostRequest::IsSelectionGathering { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.is_selection_gathering()
                    }))
                })
            };
            match result {
                Some(Ok(b)) => Some(HostResponse::Bool(b)),
                Some(Err(_)) => Some(HostResponse::Error(
                    "is_selection_gathering() panicked".to_string(),
                )),
                None => Some(HostResponse::Error(format!(
                    "unknown interactive command {command_id}"
                ))),
            }
        }
        HostRequest::AcceptsDragSelection { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.accepts_drag_selection()
                    }))
                })
            };
            match result {
                Some(Ok(b)) => Some(HostResponse::Bool(b)),
                Some(Err(_)) => Some(HostResponse::Error(
                    "accepts_drag_selection() panicked".to_string(),
                )),
                None => Some(HostResponse::Error(format!(
                    "unknown interactive command {command_id}"
                ))),
            }
        }
        HostRequest::SelectionRejectsLockedLayers { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.selection_rejects_locked_layers()
                    }))
                })
            };
            match result {
                Some(Ok(b)) => Some(HostResponse::Bool(b)),
                Some(Err(_)) => Some(HostResponse::Error(
                    "selection_rejects_locked_layers() panicked".to_string(),
                )),
                None => Some(HostResponse::Error(format!(
                    "unknown interactive command {command_id}"
                ))),
            }
        }
        HostRequest::CursorMove { command_id, pt } => {
            let result = {
                let mut registry = interactive.borrow_mut();
                registry.get_mut(&command_id).map(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.on_cursor_move(pt)
                    }))
                })
            };
            match result {
                Some(Ok(wires)) => Some(HostResponse::PreviewWires(wires)),
                Some(Err(_)) => Some(HostResponse::PreviewWires(Vec::new())),
                None => Some(HostResponse::PreviewWires(Vec::new())),
            }
        }
        HostRequest::GetModalDialog { command_id } => {
            let result = {
                let registry = interactive.borrow();
                registry.get(&command_id).and_then(|cmd| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cmd.modal_dialog()
                    }))
                    .ok()
                    .flatten()
                })
            };
            Some(HostResponse::ModalDialog(result))
        }
        HostRequest::ExecuteCode {
            command_id,
            source,
            code,
            tab_index,
        } => {
            let mut proxy = client.plugin_host_api(tab_index, interactive.clone());
            let respond = client.execute_code_responder(id);
            let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.start_execute_code(&mut proxy, command_id, &code, source, respond)
            }));
            match started {
                Ok(true) => None,
                Ok(false) => Some(HostResponse::Error(
                    "plugin does not support code execution".to_string(),
                )),
                Err(_) => Some(HostResponse::Error(
                    "plugin start_execute_code panicked".to_string(),
                )),
            }
        }
        HostRequest::CustomGripEvent { tab_index, event } => {
            // API v11 gate: a cdylib compiled against API v10 or older has a
            // BuiltinPlugin vtable without this slot — never call it (same
            // rule as entity_classes). The grip was declared by this plugin,
            // so the empty response simply keeps its preview as-is.
            if api_version < 11 {
                return Some(HostResponse::CustomGrip(Default::default()));
            }
            // A full HostApi proxy: the settle phase may write (possibly
            // nested requests, served inline by the host) inside the
            // plugin's own undo scope.
            let mut proxy = client.plugin_host_api(tab_index, interactive.clone());
            let guarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.on_custom_grip_event(&mut proxy, event)
            }));
            match guarded {
                Ok(mut resp) => {
                    // A v11 cdylib's response has no `live_entities` slot:
                    // what sits there is not a Vec, so never read or drop it.
                    if api_version < 12 {
                        std::mem::forget(std::mem::take(&mut resp.live_entities));
                    }
                    Some(HostResponse::CustomGrip(resp))
                }
                Err(_) => Some(HostResponse::Error(
                    "plugin on_custom_grip_event panicked".to_string(),
                )),
            }
        }
        HostRequest::Shutdown => {
            // Return a positive Ack so the host knows we received the request.
            // run_v4 will then observe the closed V4 writer, break its loop,
            // and return, allowing the plugin object to be dropped and clean up
            // spawned child processes before the runner process exits.
            Some(HostResponse::Bool(true))
        }
    }
}

/// Hands a v14+ plugin the whole runner through its C entry point and returns
/// its exit code, or `None` for an older plugin the runner drives itself.
unsafe fn run_self_hosted(
    socket_name: &str,
    path: &Path,
) -> Result<Option<i32>, Box<dyn std::error::Error>> {
    let lib = libloading::Library::new(path)?;
    let version: libloading::Symbol<extern "C" fn() -> u32> = lib
        .get(b"ocs_plugin_api_version")
        .map_err(|_| "missing ocs_plugin_api_version symbol")?;
    let v = version();
    if v < crate::version_info::SELF_HOSTED_API_VERSION {
        return Ok(None);
    }
    if !crate::host_accepts_plugin_version(v) {
        return Err(format!(
            "API version {v} is incompatible (host supports {}-{})",
            crate::API_VERSION_MIN_SUPPORTED,
            crate::effective_max_api_version()
        )
        .into());
    }
    let run: libloading::Symbol<extern "C" fn(*const std::os::raw::c_char) -> i32> = lib
        .get(b"ocs_plugin_run")
        .map_err(|_| "missing ocs_plugin_run symbol")?;
    let socket = std::ffi::CString::new(socket_name)?;
    let code = run(socket.as_ptr());
    // The plugin's code may still run on its threads until the process exits.
    let _ = std::mem::ManuallyDrop::new(lib);
    Ok(Some(code))
}

unsafe fn load_plugin(
    path: &Path,
) -> Result<(u32, Box<dyn BuiltinPlugin>), Box<dyn std::error::Error>> {
    let lib = libloading::Library::new(path)?;

    let version: libloading::Symbol<extern "C" fn() -> u32> = lib
        .get(b"ocs_plugin_api_version")
        .map_err(|_| "missing ocs_plugin_api_version symbol")?;
    let v = version();
    if !crate::host_accepts_plugin_version(v) {
        return Err(format!(
            "API version {v} is incompatible (host supports {}-{})",
            crate::API_VERSION_MIN_SUPPORTED,
            crate::effective_max_api_version()
        )
        .into());
    }

    let register: libloading::Symbol<extern "C" fn() -> *mut Box<dyn BuiltinPlugin>> = lib
        .get(b"ocs_plugin_register")
        .map_err(|_| "missing ocs_plugin_register symbol")?;
    let raw = register();
    if raw.is_null() {
        return Err("ocs_plugin_register returned null".into());
    }
    let plugin = *Box::from_raw(raw);

    // Intentionally leak the library so its vtables remain valid for the
    // lifetime of the process. The runner exits when the host disconnects.
    let _ = std::mem::ManuallyDrop::new(lib);

    Ok((v, plugin))
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    use interprocess::local_socket::{
        traits::{Listener, Stream as StreamTrait},
        GenericNamespaced, ListenerOptions, Stream, ToNsName,
    };

    use super::*;
    use crate::host::{BuiltinPlugin, CommandSource, HostApi};
    use crate::ipc::protocol::HostRequest;
    use crate::ipc::v4::client::V4Client;
    use crate::manifest::{ApiVersion, PluginManifest};
    use crate::ribbon::{CadModule, RibbonGroup};

    static TEST_MANIFEST: PluginManifest = PluginManifest {
        id: "test.repl",
        name: "Test REPL",
        version: "0.1.0",
        description: "test",
        api_version: ApiVersion { major: 4 },
        ribbon_order: 0,
        xdata_apps: &[],
        command_prefixes: &[],
    };

    struct EmptyModule;
    impl CadModule for EmptyModule {
        fn id(&self) -> &'static str {
            "test.repl"
        }
        fn title(&self) -> &'static str {
            "Test"
        }
        fn ribbon_groups(&self) -> &[RibbonGroup] {
            &[]
        }
    }

    struct UnsupportedReplPlugin;
    impl BuiltinPlugin for UnsupportedReplPlugin {
        fn manifest(&self) -> &'static PluginManifest {
            &TEST_MANIFEST
        }
        fn ribbon(&self) -> Box<dyn CadModule> {
            Box::new(EmptyModule)
        }
        fn dispatch(&self, _host: &mut dyn HostApi, _cmd: &str) -> bool {
            false
        }
    }

    fn unique_socket_name() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("ocs_plugin_runner_test_{}_{}", std::process::id(), n)
    }

    fn connect_pair() -> (Stream, Stream) {
        let name = unique_socket_name();
        let name_ref = name
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("valid name");
        let listener = ListenerOptions::new()
            .name(name_ref)
            .create_sync()
            .expect("listener");
        let client_name = name.clone();
        let client_thread = thread::spawn(move || {
            StreamTrait::connect(client_name.to_ns_name::<GenericNamespaced>().unwrap())
                .expect("connect")
        });
        let server = listener.accept().expect("accept");
        let client = client_thread.join().expect("client thread");
        (server, client)
    }

    #[test]
    fn v4_runner_execute_code_unsupported_returns_error() {
        let (host_stream, runner_stream) = connect_pair();
        let client = V4Client::from_stream_for_test(runner_stream);
        let interactive: InteractiveRegistry = Rc::new(RefCell::new(HashMap::new()));
        let mut plugin = UnsupportedReplPlugin;
        let req = HostRequest::ExecuteCode {
            command_id: 1,
            source: CommandSource::Editor,
            code: "1+1".to_string(),
            tab_index: 0,
        };
        let resp = handle_host_request_v4(&mut plugin, &interactive, &client, 99, 4, req);
        match resp {
            Some(HostResponse::Error(msg)) => {
                assert!(msg.contains("does not support code execution"));
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
        drop(host_stream);
    }

    static V10_MANIFEST: PluginManifest = PluginManifest {
        id: "test.classed",
        name: "Test Classed",
        version: "0.1.0",
        description: "test",
        api_version: ApiVersion { major: 10 },
        ribbon_order: 0,
        xdata_apps: &["CVIFC"],
        command_prefixes: &[],
    };

    static TEST_ENTITY_CLASSES: crate::manifest::EntityClasses = crate::manifest::EntityClasses {
        app_id: "CVIFC",
        class_prefix: "CVIFC-CLASS:",
        labels: &[
            ("LINE", "Linear Element"),
            ("CIRCULARARC", "Circular Arc Element"),
        ],
    };

    struct ClassedPlugin;
    impl BuiltinPlugin for ClassedPlugin {
        fn manifest(&self) -> &'static PluginManifest {
            &V10_MANIFEST
        }
        fn ribbon(&self) -> Box<dyn CadModule> {
            Box::new(EmptyModule)
        }
        fn dispatch(&self, _host: &mut dyn HostApi, _cmd: &str) -> bool {
            false
        }
        fn entity_classes(&self) -> Option<&'static crate::manifest::EntityClasses> {
            Some(&TEST_ENTITY_CLASSES)
        }
    }

    fn manifest_response_of(
        plugin: &mut dyn BuiltinPlugin,
        api_version: u32,
    ) -> Option<HostResponse> {
        let (host_stream, runner_stream) = connect_pair();
        let client = V4Client::from_stream_for_test(runner_stream);
        let interactive: InteractiveRegistry = Rc::new(RefCell::new(HashMap::new()));
        let resp = handle_host_request_v4(
            plugin,
            &interactive,
            &client,
            99,
            api_version,
            HostRequest::GetManifest,
        );
        drop(host_stream);
        resp
    }

    #[test]
    fn get_manifest_folds_entity_classes_for_v10_plugin() {
        let mut plugin = ClassedPlugin;
        match manifest_response_of(&mut plugin, 10) {
            Some(HostResponse::Manifest(manifest)) => {
                let classes = manifest.entity_classes.expect("declaration folded in");
                assert_eq!(classes.app_id, "CVIFC");
                assert_eq!(classes.class_prefix, "CVIFC-CLASS:");
                assert_eq!(
                    classes.labels.get("LINE").map(String::as_str),
                    Some("Linear Element")
                );
                assert_eq!(
                    classes.labels.get("CIRCULARARC").map(String::as_str),
                    Some("Circular Arc Element")
                );
            }
            other => panic!("expected manifest response, got {other:?}"),
        }
    }

    #[test]
    fn get_manifest_leaves_entity_classes_default_below_v10() {
        // A cdylib whose exported ABI symbol predates v10 must not have the
        // trait slot called, even if the in-process plugin object happens to
        // implement it: the declaration stays `None` (native display types).
        let mut plugin = ClassedPlugin;
        match manifest_response_of(&mut plugin, 9) {
            Some(HostResponse::Manifest(manifest)) => {
                assert!(
                    manifest.entity_classes.is_none(),
                    "entity classes must not be folded in for pre-v10 plugins"
                );
            }
            other => panic!("expected manifest response, got {other:?}"),
        }
    }

    #[test]
    fn get_manifest_reports_entity_classes_panic() {
        struct PanickingClassesPlugin;
        impl BuiltinPlugin for PanickingClassesPlugin {
            fn manifest(&self) -> &'static PluginManifest {
                &V10_MANIFEST
            }
            fn ribbon(&self) -> Box<dyn CadModule> {
                Box::new(EmptyModule)
            }
            fn dispatch(&self, _host: &mut dyn HostApi, _cmd: &str) -> bool {
                false
            }
            fn entity_classes(&self) -> Option<&'static crate::manifest::EntityClasses> {
                panic!("boom")
            }
        }

        let mut plugin = PanickingClassesPlugin;
        match manifest_response_of(&mut plugin, 10) {
            Some(HostResponse::Error(msg)) => {
                assert!(msg.contains("entity_classes() panicked"), "got: {msg}");
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }
}
