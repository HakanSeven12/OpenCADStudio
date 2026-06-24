//! Out-of-process plugin runner logic.
//!
//! This module is used by the host when it spawns itself in runner mode
//! (`--ocs-plugin-runner <socket> <cdylib>`). Keeping the runner code inside
//! `ocs_plugin_api` means the host only needs to know the CLI contract, not the
//! internal plugin-loading and IPC details.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use crate::host::{BuiltinPlugin, InteractiveCommand};
use crate::ipc::client::{InteractiveRegistry, IpcClient, PluginHostApi};
use crate::ipc::protocol::{
    HostRequest, HostResponse, HostToPlugin, InteractiveEvent, PluginToHost,
};
use crate::ipc::transport::{recv, send};
use crate::ribbon::owned::OwnedRibbonGroup;

/// Entry point for the plugin runner child process.
///
/// Connects back to the host on `socket_name`, loads the cdylib at
/// `cdylib_path`, and runs the request loop until the host sends `Shutdown`.
/// This function never returns normally; it exits the process on shutdown or
/// fatal error so the child does not fall through to the host's GUI main.
pub fn run(socket_name: &str, cdylib_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[runner] starting for {cdylib_path:?} on {socket_name}");
    let plugin = unsafe { load_plugin(cdylib_path)? };
    let interactive: InteractiveRegistry = Rc::new(RefCell::new(HashMap::new()));

    let client = IpcClient::connect(socket_name)?;
    eprintln!("[runner] connected to host");

    loop {
        let msg: HostToPlugin = recv(&mut client.stream_ref())?;
        eprintln!("[runner] host -> runner: {msg:?}");
        match msg {
            HostToPlugin::Request(req) => {
                let resp = handle_host_request(&*plugin, &interactive, &client, req);
                eprintln!("[runner] runner -> host: {resp:?}");
                send(&mut client.stream_ref(), &PluginToHost::Response(resp))?;
            }
            HostToPlugin::Response(_) => {
                // Responses are consumed by PluginHostApi::request synchronously.
                // Reaching here means the host sent a response without a pending
                // plugin request.
                eprintln!("[runner] unexpected HostToPlugin::Response");
            }
        }
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
                        .into_iter()
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
                Ok(s) => HostResponse::CommandStep(s),
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
        HostRequest::Shutdown => {
            // The runner will exit after this response is sent.
            std::process::exit(0);
        }
    }
}

unsafe fn load_plugin(path: &Path) -> Result<Box<dyn BuiltinPlugin>, Box<dyn std::error::Error>> {
    let lib = libloading::Library::new(path)?;

    let version: libloading::Symbol<extern "C" fn() -> u32> = lib
        .get(b"ocs_plugin_api_version")
        .map_err(|_| "missing ocs_plugin_api_version symbol")?;
    let v = version();
    if !crate::host_accepts_plugin_version(v) {
        return Err(format!(
            "API version {v} is incompatible (host supports {}-{})",
            crate::API_VERSION_MIN_SUPPORTED,
            crate::API_VERSION
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

    Ok(plugin)
}
