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

use crate::host::{BuiltinPlugin, InteractiveCommand};
use crate::ipc::client::{InteractiveRegistry, IpcClient, PluginHostApi};
use crate::ipc::protocol::{
    HostRequest, HostResponse, HostToPlugin, InteractiveEvent, PluginToHost, PLUGIN_TOKEN_ENV,
};
use crate::ipc::transport::{recv, send};
use crate::ipc::v4::client::{RunnerFrame, V4Client};
use crate::ribbon::owned::OwnedRibbonGroup;

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
    let (version, mut plugin) = unsafe { load_plugin(cdylib_path)? };
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
        // Give the plugin a chance to set up worker threads / cache the request
        // sender before the host starts dispatching user commands.
        invoke_on_load(&mut *plugin, &client, &interactive);
        run_v4(&mut *plugin, &interactive, client)
    } else {
        let client = IpcClient::connect(socket_name)?;
        eprintln!("[runner] connected to host");
        client.send_handshake(&token)?;
        // V2/V3 plugins receive HostApi only during dispatch, so on_load is not
        // meaningful there; skip it to keep the old protocol unchanged.
        run_v3(&*plugin, &interactive, client)
    }
}

fn invoke_on_load(plugin: &mut dyn BuiltinPlugin, client: &V4Client, interactive: &InteractiveRegistry) {
    let mut proxy = client.plugin_host_api(0, interactive.clone());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        plugin.on_load(&mut proxy)
    }));
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
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Drain any host notifications and dispatch them to the plugin's
        // state-only callback. Panics are caught so a bad callback does not
        // tear down the connection.
        while let Some((command_id, notification)) = client.try_recv_notification() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.on_notification(command_id, notification)
            }));
        }

        match client.recv_runner_frame_timeout(std::time::Duration::from_millis(50)) {
            Ok(RunnerFrame::Request { id, payload }) => {
                if let Some(resp) = handle_host_request_v4(&mut *plugin, interactive, &client, id, payload) {
                    client.send_response(id, resp)?;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
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
            let step = {
                let mut registry = interactive.borrow_mut();
                let Some(cmd) = registry.get_mut(&command_id) else {
                    return HostResponse::Error(format!(
                        "unknown interactive command {command_id}"
                    ));
                };
                let cmd_ref: &mut dyn InteractiveCommand = cmd.as_mut();
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match event {
                    InteractiveEvent::Point(pt) => cmd_ref.on_point(pt),
                    InteractiveEvent::Enter => cmd_ref.on_enter(),
                    InteractiveEvent::ObjectPick { handle, pt } => {
                        cmd_ref.on_object_pick(handle, pt)
                    }
                }))
            };
            match step {
                Ok(s) => HostResponse::CommandStep(Box::new(s)),
                Err(_) => HostResponse::Error("interactive command panicked".to_string()),
            }
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
        HostRequest::ExecuteCode { .. } => {
            HostResponse::Error("ExecuteCode requires V4 protocol".to_string())
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
    req: HostRequest,
) -> Option<HostResponse> {
    match req {
        HostRequest::GetManifest => {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.manifest())) {
                Ok(m) => Some(HostResponse::Manifest(m.into())),
                Err(_) => Some(HostResponse::Error("plugin manifest() panicked".to_string())),
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
            let step = {
                let mut registry = interactive.borrow_mut();
                let Some(cmd) = registry.get_mut(&command_id) else {
                    return Some(HostResponse::Error(format!(
                        "unknown interactive command {command_id}"
                    )));
                };
                let cmd_ref: &mut dyn InteractiveCommand = cmd.as_mut();
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match event {
                    InteractiveEvent::Point(pt) => cmd_ref.on_point(pt),
                    InteractiveEvent::Enter => cmd_ref.on_enter(),
                    InteractiveEvent::ObjectPick { handle, pt } => {
                        cmd_ref.on_object_pick(handle, pt)
                    }
                }))
            };
            match step {
                Ok(s) => Some(HostResponse::CommandStep(Box::new(s))),
                Err(_) => Some(HostResponse::Error("interactive command panicked".to_string())),
            }
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
                None => Some(HostResponse::Error(format!("unknown interactive command {command_id}"))),
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
                Some(Err(_)) => Some(HostResponse::Error("needs_object_pick() panicked".to_string())),
                None => Some(HostResponse::Error(format!("unknown interactive command {command_id}"))),
            }
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
                plugin.start_execute_code(
                    &mut proxy,
                    command_id,
                    &code,
                    source,
                    respond,
                )
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
        HostRequest::Shutdown => {
            // Return a positive Ack so the host knows we received the request.
            // run_v4 will then observe the closed V4 writer, break its loop,
            // and return, allowing the plugin object to be dropped and clean up
            // spawned child processes before the runner process exits.
            Some(HostResponse::Bool(true))
        }
    }
}

unsafe fn load_plugin(path: &Path) -> Result<(u32, Box<dyn BuiltinPlugin>), Box<dyn std::error::Error>> {
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
        let resp = handle_host_request_v4(&mut plugin, &interactive, &client, 99, req);
        match resp {
            Some(HostResponse::Error(msg)) => {
                assert!(msg.contains("does not support code execution"));
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
        drop(host_stream);
    }
}
