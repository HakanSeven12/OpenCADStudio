//! Process management for out-of-process plugins.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use interprocess::local_socket::traits::Listener;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};

use crate::host::{CommandStep, HostApi};
use crate::ipc::protocol::{
    HostRequest, HostResponse, HostToPlugin, InteractiveEvent, PluginToHost,
};
use crate::ipc::server::handle_plugin_request;
use crate::ipc::transport::{recv, send};
use crate::ribbon::owned::{OwnedPluginManifest, OwnedRibbonGroup as OwnedRibbonGroupAlias};

use serde::de::DeserializeOwned;

mod manager;
pub use manager::{DispatchResult, PluginManager};

/// Maximum time to wait for the plugin runner to connect back to the host.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(10);

fn spawn_timeout() -> Duration {
    std::env::var("OCS_PLUGIN_SPAWN_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(SPAWN_TIMEOUT)
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("transport error: {0}")]
    Transport(#[from] crate::ipc::transport::TransportError),
    #[error("plugin runner error: {0}")]
    Runner(String),
    #[error("spawn timeout: runner did not connect within {0:?}")]
    SpawnTimeout(Duration),
    #[error("runner exited before connecting")]
    RunnerExited,
    #[error("unexpected response: {0:?}")]
    UnexpectedResponse(HostResponse),
}

/// One spawned plugin process.
pub struct PluginProcess {
    stream: Mutex<Option<Stream>>,
    child: Mutex<Option<Child>>,
    id: String,
    manifest: OwnedPluginManifest,
    ribbon: Vec<OwnedRibbonGroupAlias>,
}

impl PluginProcess {
    /// Spawn the plugin cdylib in a separate process and connect to it.
    pub fn spawn(cdylib_path: &Path, host: &mut dyn HostApi) -> Result<Self, PluginError> {
        let socket_name = generate_socket_name();
        let socket_name_ref: interprocess::local_socket::Name = socket_name
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("valid namespaced name");
        let runner_path = runner_executable()?;
        eprintln!(
            "[plugin] spawning runner {} for {}",
            runner_path.display(),
            cdylib_path.display()
        );

        // Create the listener before spawning so the runner can connect immediately.
        let listener = ListenerOptions::new().name(socket_name_ref).create_sync()?;

        let mut child = Command::new(&runner_path)
            .arg("--ocs-plugin-runner")
            .arg(&socket_name)
            .arg(cdylib_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        // Accept the runner connection with a timeout so a hung/crashed runner
        // does not block the host indefinitely.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(listener.accept());
        });
        let stream = match rx.recv_timeout(spawn_timeout()) {
            Ok(Ok(stream)) => {
                eprintln!("[plugin] runner connected");
                Mutex::new(Some(stream))
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                return Err(PluginError::SpawnTimeout(spawn_timeout()));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = child.kill();
                return Err(PluginError::RunnerExited);
            }
        };

        // The runner first answers GetManifest and GetRibbon so the host can
        // build the UI without keeping the plugin object alive.
        let no_op = &mut |_| {};
        let manifest = match call(&stream, host, HostRequest::GetManifest, no_op)? {
            HostResponse::Manifest(m) => m,
            other => return Err(PluginError::UnexpectedResponse(other)),
        };
        let ribbon = match call(&stream, host, HostRequest::GetRibbon, no_op)? {
            HostResponse::Ribbon(r) => r,
            other => return Err(PluginError::UnexpectedResponse(other)),
        };

        let id = manifest.id.clone();
        Ok(Self {
            stream,
            child: Mutex::new(Some(child)),
            id,
            manifest,
            ribbon,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn manifest(&self) -> &OwnedPluginManifest {
        &self.manifest
    }

    pub fn ribbon(&self) -> &[OwnedRibbonGroupAlias] {
        &self.ribbon
    }

    pub fn dispatch(
        &self,
        host: &mut dyn HostApi,
        cmd: &str,
        on_start_interactive: &mut dyn FnMut(u64),
    ) -> Result<bool, PluginError> {
        eprintln!("[plugin] dispatching {cmd}");
        let result = match call(
            &self.stream,
            host,
            HostRequest::Dispatch {
                cmd: cmd.to_string(),
            },
            on_start_interactive,
        )? {
            HostResponse::Bool(b) => Ok(b),
            other => Err(PluginError::UnexpectedResponse(other)),
        };
        eprintln!("[plugin] dispatch {cmd} result: {result:?}");
        result
    }

    /// Send an interactive event for `command_id` and return the step the
    /// plugin command produces. Interactive events are not expected to trigger
    /// nested host API calls, so this path does not supply a `HostApi`.
    pub fn interactive_event(
        &self,
        command_id: u64,
        event: InteractiveEvent,
    ) -> Result<CommandStep, PluginError> {
        self.send_request(HostRequest::InteractiveEvent { command_id, event })?;
        loop {
            match self.recv_response::<PluginToHost>()? {
                PluginToHost::Response(HostResponse::CommandStep(s)) => return Ok(s),
                PluginToHost::Response(other) => {
                    return Err(PluginError::UnexpectedResponse(other))
                }
                PluginToHost::Request(req) => {
                    let resp = crate::ipc::protocol::PluginResponse::Error(format!(
                        "unexpected nested request during interactive event: {req:?}"
                    ));
                    self.send_response(resp)?;
                }
            }
        }
    }

    /// Ask the plugin process for the current prompt of an interactive command.
    pub fn get_prompt(&self, command_id: u64) -> Result<String, PluginError> {
        self.send_request(HostRequest::GetPrompt { command_id })?;
        loop {
            match self.recv_response::<PluginToHost>()? {
                PluginToHost::Response(HostResponse::Text(s)) => return Ok(s),
                PluginToHost::Response(other) => {
                    return Err(PluginError::UnexpectedResponse(other))
                }
                PluginToHost::Request(req) => {
                    let resp = crate::ipc::protocol::PluginResponse::Error(format!(
                        "unexpected nested request during get_prompt: {req:?}"
                    ));
                    self.send_response(resp)?;
                }
            }
        }
    }

    /// Ask the plugin process whether an interactive command wants object picks.
    pub fn needs_entity_pick(&self, command_id: u64) -> Result<bool, PluginError> {
        self.send_request(HostRequest::NeedsEntityPick { command_id })?;
        loop {
            match self.recv_response::<PluginToHost>()? {
                PluginToHost::Response(HostResponse::Bool(b)) => return Ok(b),
                PluginToHost::Response(other) => {
                    return Err(PluginError::UnexpectedResponse(other))
                }
                PluginToHost::Request(req) => {
                    let resp = crate::ipc::protocol::PluginResponse::Error(format!(
                        "unexpected nested request during needs_entity_pick: {req:?}"
                    ));
                    self.send_response(resp)?;
                }
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        let mut guard = self.child.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => true,
                Ok(Some(_)) | Err(_) => false,
            },
            None => false,
        }
    }

    /// Tear down the plugin process without blocking the caller. The stream is
    /// closed and the child is killed synchronously; the blocking `wait()` is
    /// done in a detached background thread so the host never waits on a plugin.
    pub fn shutdown(&self) {
        let (stream, child) = self.take_resources();
        drop(stream);
        if let Some(mut child) = child {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    /// Take the stream and child handles out of the process. After this the
    /// process is considered shut down and any further IPC will fail.
    fn take_resources(&self) -> (Option<Stream>, Option<Child>) {
        let stream = self.stream.lock().unwrap_or_else(|e| e.into_inner()).take();
        let child = self.child.lock().unwrap_or_else(|e| e.into_inner()).take();
        (stream, child)
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl PluginProcess {
    fn send_request(&self, req: HostRequest) -> Result<(), PluginError> {
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        let stream = guard.as_mut().ok_or_else(shutdown_error)?;
        send(stream, &HostToPlugin::Request(req)).map_err(Into::into)
    }

    fn send_response(&self, resp: crate::ipc::protocol::PluginResponse) -> Result<(), PluginError> {
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        let stream = guard.as_mut().ok_or_else(shutdown_error)?;
        send(stream, &HostToPlugin::Response(resp)).map_err(Into::into)
    }

    fn recv_response<T: DeserializeOwned>(&self) -> Result<T, PluginError> {
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        let stream = guard.as_mut().ok_or_else(shutdown_error)?;
        recv(stream).map_err(Into::into)
    }
}

fn shutdown_error() -> PluginError {
    PluginError::Io(std::io::Error::new(
        std::io::ErrorKind::NotConnected,
        "plugin process has been shut down",
    ))
}

/// Send a host request and wait for the response, handling any nested plugin
/// requests inline using the supplied `HostApi`.
fn call(
    stream: &Mutex<Option<Stream>>,
    host: &mut dyn HostApi,
    req: HostRequest,
    on_start_interactive: &mut dyn FnMut(u64),
) -> Result<HostResponse, PluginError> {
    eprintln!("[plugin] host -> runner: {req:?}");
    {
        let mut guard = stream.lock().unwrap_or_else(|e| e.into_inner());
        let stream = guard.as_mut().ok_or_else(shutdown_error)?;
        send(stream, &HostToPlugin::Request(req))?;
    }
    loop {
        let msg = {
            let mut guard = stream.lock().unwrap_or_else(|e| e.into_inner());
            let stream = guard.as_mut().ok_or_else(shutdown_error)?;
            recv::<PluginToHost>(stream)?
        };
        eprintln!("[plugin] runner -> host: {msg:?}");
        match msg {
            PluginToHost::Response(resp) => return Ok(resp),
            PluginToHost::Request(plugin_req) => {
                let resp = handle_plugin_request(host, plugin_req, on_start_interactive);
                eprintln!("[plugin] host -> runner response: {resp:?}");
                let mut guard = stream.lock().unwrap_or_else(|e| e.into_inner());
                let stream = guard.as_mut().ok_or_else(shutdown_error)?;
                send(stream, &HostToPlugin::Response(resp))?;
            }
        }
    }
}

/// Locate the executable to spawn for running a plugin.
///
/// The host spawns *itself* in runner mode (`--ocs-plugin-runner`), so the
/// runner is always available and stays in sync with the host binary. This
/// avoids shipping a separate `ocs_plugin_runner` binary and works the same on
/// Windows, macOS, and Linux.
///
/// For testing or unusual deployment layouts, set `OCS_PLUGIN_RUNNER_EXE` to
/// the host executable path.
fn runner_executable() -> Result<PathBuf, PluginError> {
    static RUNNER: Mutex<Option<PathBuf>> = Mutex::new(None);
    let mut cached = RUNNER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref path) = *cached {
        return Ok(path.clone());
    }

    let path = if let Ok(path) = std::env::var("OCS_PLUGIN_RUNNER_EXE") {
        let path = PathBuf::from(path);
        if path.exists() {
            path
        } else {
            return Err(PluginError::Runner(format!(
                "OCS_PLUGIN_RUNNER_EXE does not exist: {}",
                path.display()
            )));
        }
    } else {
        let host = std::env::current_exe()?;
        if !host.exists() {
            return Err(PluginError::Runner(format!(
                "cannot find current executable at {}",
                host.display()
            )));
        }

        // Create a hard link with a distinct name next to the host binary. This
        // makes runner processes visible as separate sub-processes in task
        // managers / ps, while keeping the runner the exact same binary as the
        // host so they can never drift out of sync.
        let runner = distinct_runner_path(&host);
        let _ = std::fs::remove_file(&runner);
        match std::fs::hard_link(&host, &runner) {
            Ok(()) => runner,
            Err(_) => host,
        }
    };

    *cached = Some(path.clone());
    Ok(path)
}

/// Build a runner path like `<host>-plugin-runner<ext>` in the same directory.
/// Using a distinct image name lets task managers show plugin processes as
/// children/sub-processes of the host instead of collapsing them into one row.
fn distinct_runner_path(host: &Path) -> PathBuf {
    let mut runner = host.as_os_str().to_owned();
    if let Some(ext) = host.extension().and_then(|s| s.to_str()) {
        let base = host.file_stem().unwrap_or_default();
        runner =
            std::ffi::OsString::from(format!("{}-plugin-runner.{}", base.to_string_lossy(), ext));
    } else {
        runner.push("-plugin-runner");
    }
    let mut path = host
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    path.push(runner);
    path
}

/// Generate a unique local socket name.
fn generate_socket_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ocs_plugin_{}_{}", std::process::id(), n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_runner_path_appends_suffix() {
        let host = PathBuf::from("/app/OpenCADStudio.exe");
        let runner = distinct_runner_path(&host);
        assert_eq!(
            runner,
            PathBuf::from("/app/OpenCADStudio-plugin-runner.exe")
        );
    }

    #[test]
    fn distinct_runner_path_handles_no_extension() {
        let host = PathBuf::from("/app/OpenCADStudio");
        let runner = distinct_runner_path(&host);
        assert_eq!(runner, PathBuf::from("/app/OpenCADStudio-plugin-runner"));
    }
}
