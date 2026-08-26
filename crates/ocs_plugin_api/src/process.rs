//! Process management for out-of-process plugins.
//!
//! This module defines the lifecycle of a single plugin process:
//!
//! 1. `PluginProcess::spawn` creates a local socket, spawns the host binary in
//!    runner mode (`--ocs-plugin-runner <socket> <cdylib>`), and waits for the
//!    runner to connect back.
//! 2. The runner presents a pre-shared token via [`crate::ipc::protocol::PLUGIN_TOKEN_ENV`];
//!    the host rejects the connection on mismatch.
//! 3. The host requests the manifest, checks `api_version` and (for v4+) the
//!    acadrust source gate, then keeps the process alive.
//! 4. Host → plugin calls (`dispatch`, `execute_code`, interactive events) are
//!    sent over the socket with a configurable per-call timeout.
//! 5. Stdout/stderr of the child are drained into `PluginIoLine` records for
//!    logging and UI display.
//!
//! `NullHost` is a dummy `HostApi` implementation used for V4 paths that do not
//! supply a real host surface (e.g., interactive events). Timeouts and message
//! size limits are centralized here.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::Listener;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};

use crate::host::{CommandSource, CommandStep, ExecutionResult, HostApi, HostNotification, PluginNotification};
use crate::ipc::protocol::{
    CadDocument, EntityType, Handle, HostRequest, HostResponse, HostToPlugin, InteractiveEvent,
    PluginToHost, RunnerHandshake, PLUGIN_TOKEN_ENV,
};
use crate::ipc::server::handle_plugin_request;
use crate::ipc::transport::{recv, send};
use crate::process::v4::V4Connection;
use crate::ribbon::owned::{OwnedPluginManifest, OwnedRibbonGroup as OwnedRibbonGroupAlias};

use serde::de::DeserializeOwned;
use std::sync::Arc;

mod manager;
mod v4;
pub use manager::{DispatchResult, NotificationHandler, PluginManager};

/// A line emitted by a plugin process on stdout or stderr.
#[derive(Debug, Clone)]
pub struct PluginIoLine {
    /// Which stream the line came from.
    pub source: IoStream,
    /// Plugin id that produced the line.
    pub plugin_id: String,
    /// Text content without the trailing newline.
    pub text: String,
}

/// Stream source for a [`PluginIoLine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoStream {
    Stdout,
    Stderr,
}

impl std::fmt::Display for IoStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoStream::Stdout => write!(f, "stdout"),
            IoStream::Stderr => write!(f, "stderr"),
        }
    }
}

/// A dummy `HostApi` used for V4 paths that do not supply a real host surface
/// (e.g., interactive events). Nested plugin requests receive safe defaults.
struct NullHost;

impl HostApi for NullHost {
    fn tab_index(&self) -> usize {
        0
    }
    fn document(&self) -> &CadDocument {
        panic!("NullHost: document not available")
    }
    fn document_mut(&mut self) -> &mut CadDocument {
        panic!("NullHost: document_mut not available")
    }
    fn document_reader(&self) -> Box<dyn crate::host::DocumentReader + '_> {
        Box::new(NullReader)
    }
    fn add_entity(&mut self, _entity: EntityType) -> Handle {
        Handle::default()
    }
    fn bump_geometry(&mut self) {}
    fn read_record(
        &self,
        _handle: Handle,
        _app_name: &str,
    ) -> Option<&crate::ipc::protocol::ExtendedDataRecord> {
        None
    }
    fn write_record(&mut self, _handle: Handle, _record: crate::ipc::protocol::ExtendedDataRecord) -> bool {
        false
    }
    fn remove_record(&mut self, _handle: Handle, _app_name: &str) -> bool {
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
        panic!("NullHost: ensure_plugin_state_any not available")
    }
}

struct NullReader;
impl crate::host::DocumentReader for NullReader {
    fn entity_count(&self) -> usize {
        0
    }
    fn for_each_entity(&self, _f: &mut dyn FnMut(crate::host::ReaderEntity<'_>)) {}
    fn layer_name(&self, _handle: Handle) -> Option<&str> {
        None
    }
    fn app_id_name(&self, _handle: Handle) -> Option<&str> {
        None
    }
}

/// Whether verbose plugin-IPC logging is on. Off by default so a normal run
/// only prints the one-line "Loaded plugin" notice; set `OCS_PLUGIN_VERBOSE=1`
/// (any value) to see the spawn / handshake / per-dispatch request-response
/// trace.
pub(crate) fn verbose() -> bool {
    use std::sync::OnceLock;
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| std::env::var_os("OCS_PLUGIN_VERBOSE").is_some())
}

/// `eprintln!` that only fires in verbose mode (see [`verbose`]).
macro_rules! vlog {
    ($($arg:tt)*) => {{
        if crate::process::verbose() {
            eprintln!($($arg)*);
        }
    }};
}

/// Maximum time to wait for the plugin runner to connect back to the host.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(30);

fn spawn_timeout() -> Duration {
    std::env::var("OCS_PLUGIN_SPAWN_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(SPAWN_TIMEOUT)
}

/// Default maximum time to wait for a plugin call to respond.
const CALL_TIMEOUT_DEFAULT: Duration = Duration::from_secs(30);

/// Length of the random pre-shared token used to authenticate the runner.
const PLUGIN_TOKEN_LEN: usize = 32;

fn call_timeout() -> Duration {
    std::env::var("OCS_PLUGIN_CALL_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(CALL_TIMEOUT_DEFAULT)
}

/// Per-request-kind timeout floors. The user-configured default is raised to
/// these minima so that no request kind can be configured into an unsafe value.
fn request_timeout(kind: &'static str) -> Duration {
    base_max_floor(call_timeout(), kind)
}

fn execute_code_timeout() -> Duration {
    const DEFAULT: Duration = Duration::from_secs(60);
    std::env::var("OCS_PLUGIN_EXECUTE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT)
        .max(DEFAULT)
}

fn base_max_floor(base: Duration, kind: &'static str) -> Duration {
    // Tests lower the floor via OCS_PLUGIN_TEST_FLOOR_SECS so the suite does not
    // wait out the real 10 s Dispatch minimum. The seam is compiled in only
    // under cfg(test); production always enforces the safety floors below.
    #[cfg(test)]
    if let Some(secs) = std::env::var("OCS_PLUGIN_TEST_FLOOR_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        return base.max(Duration::from_secs(secs));
    }
    let floor = match kind {
        "GetManifest" | "GetRibbon" => Duration::from_secs(5),
        "Dispatch" => Duration::from_secs(10),
        "InteractiveEvent" | "GetPrompt" | "NeedsEntityPick" => Duration::from_secs(2),
        "ExecuteCode" => execute_code_timeout(),
        _ => Duration::from_secs(1),
    };
    base.max(floor)
}

fn request_kind(req: &HostRequest) -> &'static str {
    match req {
        HostRequest::GetManifest => "GetManifest",
        HostRequest::GetRibbon => "GetRibbon",
        HostRequest::Dispatch { .. } => "Dispatch",
        HostRequest::InteractiveEvent { .. } => "InteractiveEvent",
        HostRequest::GetPrompt { .. } => "GetPrompt",
        HostRequest::NeedsEntityPick { .. } => "NeedsEntityPick",
        HostRequest::ExecuteCode { .. } => "ExecuteCode",
        HostRequest::Shutdown => "Shutdown",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("transport error: {0}")]
    Transport(#[from] crate::ipc::transport::TransportError),
    #[error("plugin runner error: {0}")]
    Runner(String),
    #[error("call timeout: {request} did not respond within {duration:?}")]
    CallTimeout {
        request: &'static str,
        duration: Duration,
    },
    #[error("runner exited unexpectedly during {request} ({status})")]
    RunnerCrashed {
        request: &'static str,
        status: String,
    },
    #[error("unexpected response: {0:?}")]
    UnexpectedResponse(Box<HostResponse>),
}

/// One spawned plugin process.
pub struct PluginProcess {
    stream: Mutex<Option<Stream>>,
    v4: Option<V4Connection>,
    child: Mutex<Option<Child>>,
    id: String,
    manifest: OwnedPluginManifest,
    ribbon: Vec<OwnedRibbonGroupAlias>,
    io_lines: Mutex<Option<mpsc::Receiver<PluginIoLine>>>,
}

impl PluginProcess {
    /// Spawn the plugin cdylib in a separate process and connect to it.
    pub fn spawn(
        cdylib_path: &Path,
        host: &mut dyn HostApi,
        notification_handler: NotificationHandler,
    ) -> Result<Self, PluginError> {
        let socket_name = generate_socket_name();
        let socket_name_ref: interprocess::local_socket::Name = socket_name
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("valid namespaced name");
        let runner_path = runner_executable()?;
        vlog!(
            "[plugin] spawning runner {} for {}",
            runner_path.display(),
            cdylib_path.display()
        );

        let token = generate_token()?;

        // Create the listener before spawning so the runner can connect immediately.
        let listener = ListenerOptions::new().name(socket_name_ref).create_sync()?;

        let mut child = Command::new(&runner_path)
            .arg("--ocs-plugin-runner")
            .arg(&socket_name)
            .arg(cdylib_path)
            .env(PLUGIN_TOKEN_ENV, &token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Start draining the child's stdout/stderr immediately. If the
        // plugin prints during GetManifest/GetRibbon, a full pipe buffer
        // would otherwise block (or kill) the runner before the host ever
        // reads it. The plugin id is filled in once we read the manifest.
        let (io_tx, io_rx) = mpsc::channel::<PluginIoLine>();
        let plugin_id_for_io: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let last_stderr: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        if let Some(stdout) = child.stdout.take() {
            let io_tx = io_tx.clone();
            let plugin_id = Arc::clone(&plugin_id_for_io);
            std::thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let id = plugin_id.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let _ = io_tx.send(PluginIoLine {
                        source: IoStream::Stdout,
                        plugin_id: id,
                        text: line,
                    });
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let last_stderr = Arc::clone(&last_stderr);
            let io_tx = io_tx.clone();
            let plugin_id = Arc::clone(&plugin_id_for_io);
            std::thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut guard) = last_stderr.lock() {
                        *guard = line.clone();
                    }
                    let id = plugin_id.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let _ = io_tx.send(PluginIoLine {
                        source: IoStream::Stderr,
                        plugin_id: id,
                        text: line,
                    });
                }
            });
        }
        let child = Mutex::new(Some(child));

        // Accept the runner connection with a timeout so a hung/crashed runner
        // does not block the host indefinitely.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(listener.accept());
        });
        let stream = match rx.recv_timeout(spawn_timeout()) {
            Ok(Ok(stream)) => {
                vlog!("[plugin] runner connected");
                Mutex::new(Some(stream))
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let status = spawn_failure_status(&child, &last_stderr);
                if let Some(child) = child.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    reap(child);
                }
                return Err(PluginError::RunnerCrashed {
                    request: "spawn/accept",
                    status,
                });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = spawn_failure_status(&child, &last_stderr);
                if let Some(child) = child.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    reap(child);
                }
                return Err(PluginError::RunnerCrashed {
                    request: "spawn/accept",
                    status,
                });
            }
        };

        // Verify the runner presented the token it received through the
        // environment before allowing any host→runner requests. The read is
        // bounded by a deadline: `accept` above guards the connect, and this
        // guards the first frame, so a process that connects but never sends —
        // or a runner that dies mid-handshake — cannot block the host forever.
        let handshake_timeout = spawn_timeout();
        let handshake_deadline = Instant::now() + handshake_timeout;
        let handshake = recv_with_deadline::<RunnerHandshake>(
            &stream,
            &child,
            handshake_deadline,
            handshake_timeout,
            "Handshake",
        )?;
        let is_v4 = match verify_runner_handshake(handshake, &token) {
            Ok(v4) => v4,
            Err(e) => {
                mark_dead(&stream, &child);
                return Err(e);
            }
        };

        let no_op = &mut |_| {};
        let manifest;
        let ribbon;
        let v4: Option<V4Connection>;
        if is_v4 {
            // V4 path: split the stream into reader/writer halves and start
            // the V4 reader thread. The notification handler receives an empty
            // plugin id until we read the manifest below; that brief window is
            // fine because the plugin has not yet been asked to do work.
            let stream = stream.lock().unwrap_or_else(|e| e.into_inner()).take().expect("V4 stream");
            let plugin_id_placeholder = Arc::new(std::sync::Mutex::new(String::new()));
            let handler_for_v4: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> = {
                let plugin_id = Arc::clone(&plugin_id_placeholder);
                let base_handler = Arc::clone(&notification_handler);
                Arc::new(move |command_id, notification| {
                    let id = plugin_id.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    base_handler(&id, command_id, notification);
                })
            };
            let v4_conn = V4Connection::new(stream, handler_for_v4)?;
            manifest = match v4_conn.call(host, HostRequest::GetManifest, no_op)? {
                HostResponse::Manifest(m) => m,
                other => return Err(PluginError::UnexpectedResponse(Box::new(other))),
            };
            *plugin_id_placeholder.lock().unwrap_or_else(|e| e.into_inner()) = manifest.id.clone();
            ribbon = match v4_conn.call(host, HostRequest::GetRibbon, no_op)? {
                HostResponse::Ribbon(r) => r,
                other => return Err(PluginError::UnexpectedResponse(Box::new(other))),
            };
            v4 = Some(v4_conn);
        } else {
            // The runner first answers GetManifest and GetRibbon so the host can
            // build the UI without keeping the plugin object alive.
            manifest = match call(&stream, &child, host, HostRequest::GetManifest, no_op)? {
                HostResponse::Manifest(m) => m,
                other => return Err(PluginError::UnexpectedResponse(Box::new(other))),
            };
            ribbon = match call(&stream, &child, host, HostRequest::GetRibbon, no_op)? {
                HostResponse::Ribbon(r) => r,
                other => return Err(PluginError::UnexpectedResponse(Box::new(other))),
            };
            v4 = None;
        }

        // Normal-run notice: just the loaded plugin's name (id + version). The
        // full IPC trace above/below is gated behind OCS_PLUGIN_VERBOSE.
        eprintln!(
            "Loaded plugin: {} ({} {})",
            manifest.name, manifest.id, manifest.version
        );

        let id = manifest.id.clone();
        *plugin_id_for_io.lock().unwrap_or_else(|e| e.into_inner()) = id.clone();

        Ok(Self {
            stream,
            v4,
            child,
            id,
            manifest,
            ribbon,
            io_lines: Mutex::new(Some(io_rx)),
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

    /// Drain any stdout/stderr lines that have accumulated from the plugin
    /// runner. This should be called regularly by the host (e.g., alongside
    /// [`drain_requests`]) so plugin `println!` / `eprintln!` output appears in
    /// the host command line.
    pub fn drain_io(&self) -> Vec<PluginIoLine> {
        let mut rx = self.io_lines.lock().unwrap_or_else(|e| e.into_inner());
        let Some(rx) = rx.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(line) = rx.try_recv() {
            out.push(line);
        }
        out
    }

    pub fn dispatch(
        &self,
        host: &mut dyn HostApi,
        cmd: &str,
        on_start_interactive: &mut dyn FnMut(u64),
    ) -> Result<bool, PluginError> {
        vlog!("[plugin] dispatching {cmd}");
        let result = if let Some(v4) = &self.v4 {
            match v4.call(
                host,
                HostRequest::Dispatch {
                    cmd: cmd.to_string(),
                },
                on_start_interactive,
            )? {
                HostResponse::Bool(b) => Ok(b),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            match call(
                &self.stream,
                &self.child,
                host,
                HostRequest::Dispatch {
                    cmd: cmd.to_string(),
                },
                on_start_interactive,
            )? {
                HostResponse::Bool(b) => Ok(b),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        };
        vlog!("[plugin] dispatch {cmd} result: {result:?}");
        result
    }

    /// Execute a REPL code snippet on a V4 plugin. The session is tied to the
    /// active document tab via `host.tab_index()`.
    pub fn execute_code(
        &self,
        host: &mut dyn HostApi,
        command_id: u64,
        source: CommandSource,
        code: &str,
    ) -> Result<ExecutionResult, PluginError> {
        match &self.v4 {
            Some(v4) => v4.execute_code(host, command_id, source, code),
            None => Err(PluginError::Runner(
                "ExecuteCode requires V4 protocol".into(),
            )),
        }
    }

    /// Send an interactive event for `command_id` and return the step the
    /// plugin command produces. Interactive events are not expected to trigger
    /// nested host API calls, so this path does not supply a `HostApi`.
    pub fn interactive_event(
        &self,
        command_id: u64,
        event: InteractiveEvent,
    ) -> Result<CommandStep, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost,
                HostRequest::InteractiveEvent { command_id, event },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::CommandStep(s) => Ok(*s),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.send_request(HostRequest::InteractiveEvent { command_id, event })?;
            let kind = "InteractiveEvent";
            let timeout = request_timeout(kind);
            let deadline = Instant::now() + timeout;
            loop {
                match recv_with_deadline::<PluginToHost>(
                    &self.stream,
                    &self.child,
                    deadline,
                    timeout,
                    kind,
                )? {
                    PluginToHost::Response(HostResponse::CommandStep(s)) => return Ok(*s),
                    PluginToHost::Response(other) => {
                        return Err(PluginError::UnexpectedResponse(Box::new(other)))
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
    }

    /// Ask the plugin process for the current prompt of an interactive command.
    pub fn get_prompt(&self, command_id: u64) -> Result<String, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost,
                HostRequest::GetPrompt { command_id },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::Text(s) => Ok(s),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.send_request(HostRequest::GetPrompt { command_id })?;
            let kind = "GetPrompt";
            let timeout = request_timeout(kind);
            let deadline = Instant::now() + timeout;
            loop {
                match recv_with_deadline::<PluginToHost>(
                    &self.stream,
                    &self.child,
                    deadline,
                    timeout,
                    kind,
                )? {
                    PluginToHost::Response(HostResponse::Text(s)) => return Ok(s),
                    PluginToHost::Response(other) => {
                        return Err(PluginError::UnexpectedResponse(Box::new(other)))
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
    }

    /// Ask the plugin process whether an interactive command wants object picks.
    pub fn needs_entity_pick(&self, command_id: u64) -> Result<bool, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost,
                HostRequest::NeedsEntityPick { command_id },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::Bool(b) => Ok(b),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.send_request(HostRequest::NeedsEntityPick { command_id })?;
            let kind = "NeedsEntityPick";
            let timeout = request_timeout(kind);
            let deadline = Instant::now() + timeout;
            loop {
                match recv_with_deadline::<PluginToHost>(
                    &self.stream,
                    &self.child,
                    deadline,
                    timeout,
                    kind,
                )? {
                    PluginToHost::Response(HostResponse::Bool(b)) => return Ok(b),
                    PluginToHost::Response(other) => {
                        return Err(PluginError::UnexpectedResponse(Box::new(other)))
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
    }

    pub fn is_alive(&self) -> bool {
        if let Some(v4) = &self.v4 {
            return v4.is_alive();
        }
        let mut guard = self.child.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => true,
                Ok(Some(_)) | Err(_) => false,
            },
            None => false,
        }
    }

    /// Whether this plugin process speaks the V4 protocol.
    pub fn is_v4(&self) -> bool {
        self.v4.is_some()
    }

    /// Send a best-effort host-to-plugin notification. Requires V4; on V3
    /// processes this returns an error.
    pub fn notify_plugin(
        &self,
        command_id: Option<u64>,
        notification: HostNotification,
    ) -> Result<(), PluginError> {
        if let Some(v4) = &self.v4 {
            v4.notify_plugin(command_id, notification)
        } else {
            Err(PluginError::Runner(
                "notify_plugin requires V4 protocol".into(),
            ))
        }
    }

    /// Drain any plugin→host requests that have arrived asynchronously for
    /// this process. On V3 processes this is a no-op.
    pub fn drain_requests(
        &self,
        host: &mut dyn HostApi,
        on_start_interactive: &mut dyn FnMut(u64),
    ) -> Result<(), PluginError> {
        if let Some(v4) = &self.v4 {
            v4.drain_requests(host, on_start_interactive)?;
        }
        Ok(())
    }

    /// Tear down the plugin process without blocking the caller. The child is
    /// killed first so the socket closes, which unblocks the V4 reader thread
    /// join. The stream is dropped and the blocking `wait()` is done in a
    /// detached background thread so the host never waits on a plugin.
    pub fn shutdown(&self) {
        // For V4 connections, request a graceful shutdown first so the plugin
        // can tear down any child processes it created. The V4 shutdown waits
        // a bounded amount of time for the runner to close its side.
        if let Some(v4) = &self.v4 {
            v4.shutdown();
        }
        let (stream, child) = self.take_resources();
        drop(stream);
        if let Some(child) = child {
            reap(child);
        }
    }

    /// Tear down the plugin process and block until the child exits or the
    /// timeout elapses. Use this when the caller needs the DLL/files to be
    /// released before continuing (e.g. uninstalling a plugin on Windows).
    pub fn shutdown_and_wait(&self, timeout: Duration) -> Option<std::process::ExitStatus> {
        if let Some(v4) = &self.v4 {
            v4.shutdown();
        }
        let (stream, child) = self.take_resources();
        drop(stream);
        if let Some(mut child) = child {
            let _ = child.kill();
            let start = Instant::now();
            while start.elapsed() < timeout {
                if let Ok(Some(status)) = child.try_wait() {
                    return Some(status);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            reap(child);
        }
        None
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
        send(stream, &HostToPlugin::Response(Box::new(resp))).map_err(Into::into)
    }
}

/// Kill a child process and reap it without blocking the caller. The blocking
/// `wait()` runs in a detached thread so the host never stalls on a plugin, and
/// the child is reaped rather than left as a zombie on Unix.
fn reap(mut child: Child) {
    let _ = child.kill();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn shutdown_error() -> PluginError {
    PluginError::Io(std::io::Error::new(
        std::io::ErrorKind::NotConnected,
        "plugin process has been shut down",
    ))
}

/// Take the stream and child away from a process and kill the child without
/// blocking the caller. After this the process is considered dead and any
/// further IPC will fail.
fn mark_dead(stream: &Mutex<Option<Stream>>, child: &Mutex<Option<Child>>) {
    // On a timeout the live `Stream` is owned by the detached reader thread, so
    // this `take()` usually clears an already-`None` slot. The host end is not
    // closed here directly: killing the child below shuts its socket end, which
    // unblocks the reader thread's `recv` and lets it drop the `Stream`. If the
    // kill fails the reader thread can stay parked until the OS tears the socket
    // down, but the process is still treated as dead for all further IPC.
    let _ = stream.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(child) = child.lock().unwrap_or_else(|e| e.into_inner()).take() {
        reap(child);
    }
}

/// Check whether a child process has already exited, returning its status if so.
fn child_status(child: &Mutex<Option<Child>>) -> Option<std::process::ExitStatus> {
    let mut guard = child.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(c) => c.try_wait().ok().flatten(),
        None => None,
    }
}

/// Human-readable description of a process exit status.
fn format_exit_status(status: Option<std::process::ExitStatus>) -> String {
    match status {
        Some(s) if s.success() => "exited successfully".to_string(),
        Some(s) => match s.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by signal".to_string(),
        },
        None => "unknown status".to_string(),
    }
}

/// Describe why a runner failed before/during connection, including any stderr
/// it produced.
fn spawn_failure_status(
    child: &Mutex<Option<Child>>,
    last_stderr: &Mutex<String>,
) -> String {
    let mut parts = vec![format_exit_status(child_status(child))];
    if let Ok(line) = last_stderr.lock() {
        if !line.is_empty() {
            parts.push(format!("stderr: {line}"));
        }
    }
    parts.join("; ")
}

/// Receive one message from the plugin runner with a deadline.
///
/// A short-lived reader thread performs the blocking `recv` so that the main
/// thread can time it out. If the deadline passes, the process is marked dead
/// (stream closed, child killed) so that subsequent dispatch attempts are
/// skipped.
fn recv_with_deadline<T: DeserializeOwned + Send + 'static>(
    stream: &Mutex<Option<Stream>>,
    child: &Mutex<Option<Child>>,
    deadline: Instant,
    timeout: Duration,
    request: &'static str,
) -> Result<T, PluginError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        mark_dead(stream, child);
        return Err(PluginError::CallTimeout {
            request,
            duration: timeout,
        });
    }

    let (tx, rx) = mpsc::channel::<(Result<T, PluginError>, Option<Stream>)>();
    let stream_to_thread = stream.lock().unwrap_or_else(|e| e.into_inner()).take();

    std::thread::spawn(move || {
        let result = match stream_to_thread {
            Some(mut stream) => match recv::<T>(&mut stream) {
                Ok(msg) => (Ok(msg), Some(stream)),
                Err(e) => (Err(PluginError::from(e)), Some(stream)),
            },
            None => (Err(shutdown_error()), None),
        };
        let _ = tx.send(result);
    });

    match rx.recv_timeout(remaining) {
        Ok((Ok(msg), stream_opt)) => {
            *stream.lock().unwrap_or_else(|e| e.into_inner()) = stream_opt;
            Ok(msg)
        }
        Ok((Err(e), stream_opt)) => {
            *stream.lock().unwrap_or_else(|e| e.into_inner()) = stream_opt;
            let status = child_status(child);
            let connection_error = matches!(e, PluginError::Transport(_) | PluginError::Io(_));
            if connection_error || status.is_some() {
                let status_text = match status {
                    Some(status) => format_exit_status(Some(status)),
                    None => "connection lost before response; the plugin may have crashed or be ABI-incompatible".to_string(),
                };
                return Err(PluginError::RunnerCrashed {
                    request,
                    status: format!("{status_text}; cause: {e}"),
                });
            }
            Err(e)
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let status = child_status(child);
            mark_dead(stream, child);
            if let Some(status) = status {
                return Err(PluginError::RunnerCrashed {
                    request,
                    status: format_exit_status(Some(status)),
                });
            }
            Err(PluginError::CallTimeout {
                request,
                duration: timeout,
            })
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            if let Some(status) = child_status(child) {
                return Err(PluginError::RunnerCrashed {
                    request,
                    status: format_exit_status(Some(status)),
                });
            }
            Err(shutdown_error())
        }
    }
}

/// Send a host request and wait for the response, handling any nested plugin
/// requests inline using the supplied `HostApi`.
fn call(
    stream: &Mutex<Option<Stream>>,
    child: &Mutex<Option<Child>>,
    host: &mut dyn HostApi,
    req: HostRequest,
    on_start_interactive: &mut dyn FnMut(u64),
) -> Result<HostResponse, PluginError> {
    let kind = request_kind(&req);
    let timeout = request_timeout(kind);
    let deadline = Instant::now() + timeout;
    vlog!("[plugin] host -> runner: {req:?}");
    {
        let mut guard = stream.lock().unwrap_or_else(|e| e.into_inner());
        let stream = guard.as_mut().ok_or_else(shutdown_error)?;
        send(stream, &HostToPlugin::Request(req))?;
    }
    loop {
        let msg = recv_with_deadline::<PluginToHost>(stream, child, deadline, timeout, kind)?;
        vlog!("[plugin] runner -> host: {msg:?}");
        match msg {
            PluginToHost::Response(resp) => return Ok(resp),
            PluginToHost::Request(plugin_req) => {
                let resp = handle_plugin_request(host, *plugin_req, on_start_interactive);
                vlog!("[plugin] host -> runner response: {resp:?}");
                let mut guard = stream.lock().unwrap_or_else(|e| e.into_inner());
                let stream = guard.as_mut().ok_or_else(shutdown_error)?;
                send(stream, &HostToPlugin::Response(Box::new(resp)))?;
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
        // The host spawns itself in runner mode. We used to create a hard link
        // with a distinct name so task managers show plugin processes
        // separately, but re-creating that link on every launch triggers
        // antivirus scanners on some systems and makes the first plugin spawn
        // time out. Spawning the host executable directly avoids that.
        let host = std::env::current_exe()?;
        if !host.exists() {
            return Err(PluginError::Runner(format!(
                "cannot find current executable at {}",
                host.display()
            )));
        }
        host
    };

    *cached = Some(path.clone());
    Ok(path)
}

/// Build a runner path like `<host>-plugin-runner<ext>` in the same directory.
/// Using a distinct image name lets task managers show plugin processes as
/// children/sub-processes of the host instead of collapsing them into one row.
#[allow(dead_code)]
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

/// Verify that the runner's `handshake` presents `expected_token`.
///
/// Returns `Ok(true)` for a V4 handshake, `Ok(false)` for a V3 handshake, and
/// `Err` on token mismatch or unsupported protocol version.
fn verify_runner_handshake(
    handshake: RunnerHandshake,
    expected_token: &str,
) -> Result<bool, PluginError> {
    match handshake {
        RunnerHandshake::Token(ref presented) if presented == expected_token => {
            vlog!("[plugin] runner authenticated (V3)");
            Ok(false)
        }
        RunnerHandshake::TokenV4 {
            token: ref presented,
            protocol_version,
        } if presented == expected_token => {
            if protocol_version != 4 {
                return Err(PluginError::Runner(format!(
                    "unsupported V4 protocol version {protocol_version}"
                )));
            }
            if crate::effective_max_api_version() < 4 {
                return Err(PluginError::Runner(
                    "V4 protocol is disabled by OCS_PLUGIN_MAX_API_VERSION".into(),
                ));
            }
            vlog!("[plugin] runner authenticated (V4)");
            Ok(true)
        }
        _ => Err(PluginError::Runner("authentication failed".into())),
    }
}

/// Generate a unique local socket name.
fn generate_socket_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ocs_plugin_{}_{}", std::process::id(), n)
}

/// Generate a 32-byte random token for runner authentication.
fn generate_token() -> Result<String, PluginError> {
    let mut bytes = [0u8; PLUGIN_TOKEN_LEN];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| PluginError::Runner(format!("token generation failed: {e}")))?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    Ok(out)
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

#[cfg(all(test, feature = "host"))]
mod timeout_tests {
    use super::*;
    use crate::host::{DocumentReader, HostApi, ReaderEntity};
    use crate::ipc::protocol::{
        HostRequest, HostResponse, HostToPlugin, PluginRequest, PluginResponse, PluginToHost,
        RunnerHandshake,
    };
    use crate::ipc::transport::{recv, send};
    use crate::ribbon::owned::OwnedPluginManifest;
    use crate::test_lock::ENV_LOCK;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::{CadDocument, EntityType, Handle};
    use interprocess::local_socket::{
        traits::{Listener, Stream as StreamTrait},
        GenericNamespaced, ListenerOptions, Stream, ToNsName,
    };
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex as StdMutex;
    use std::thread;
    use std::time::Instant;

    struct EmptyReader;

    impl DocumentReader for EmptyReader {
        fn entity_count(&self) -> usize {
            0
        }
        fn for_each_entity(&self, _f: &mut dyn FnMut(ReaderEntity<'_>)) {}
        fn layer_name(&self, _handle: Handle) -> Option<&str> {
            None
        }
        fn app_id_name(&self, _handle: Handle) -> Option<&str> {
            None
        }
    }

    struct DummyHost {
        doc: CadDocument,
        push_info_messages: StdMutex<Vec<String>>,
    }

    impl DummyHost {
        fn new(doc: CadDocument) -> Self {
            Self {
                doc,
                push_info_messages: StdMutex::new(Vec::new()),
            }
        }
        fn take_push_info(&self) -> Vec<String> {
            std::mem::take(&mut *self.push_info_messages.lock().unwrap())
        }
    }

    impl HostApi for DummyHost {
        fn tab_index(&self) -> usize {
            0
        }
        fn document(&self) -> &CadDocument {
            &self.doc
        }
        fn document_mut(&mut self) -> &mut CadDocument {
            &mut self.doc
        }
        fn document_reader(&self) -> Box<dyn DocumentReader + '_> {
            Box::new(EmptyReader)
        }
        fn add_entity(&mut self, _entity: EntityType) -> Handle {
            panic!("not used")
        }
        fn bump_geometry(&mut self) {}
        fn read_record(&self, _handle: Handle, _app_name: &str) -> Option<&ExtendedDataRecord> {
            None
        }
        fn write_record(&mut self, _handle: Handle, _record: ExtendedDataRecord) -> bool {
            false
        }
        fn remove_record(&mut self, _handle: Handle, _app_name: &str) -> bool {
            false
        }
        fn push_undo(&mut self, _label: &str) {}
        fn set_dirty(&mut self) {}
        fn push_info(&mut self, msg: &str) {
            self.push_info_messages.lock().unwrap().push(msg.to_string());
        }
        fn push_output(&mut self, _msg: &str) {}
        fn push_error(&mut self, _msg: &str) {}
        fn start_interactive(&mut self, _command: Box<dyn crate::host::InteractiveCommand>) {}
        fn plugin_state_any(&self, _plugin_id: &str) -> Option<&(dyn std::any::Any + Send + Sync)> {
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

    fn unique_socket_name() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("ocs_plugin_timeout_test_{}_{}", std::process::id(), n)
    }

    fn connected_pair() -> (Stream, Stream) {
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

    fn sleepy_child() -> Child {
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .arg("/c")
                .arg("ping -n 30 127.0.0.1")
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("spawn sleep")
        }
        #[cfg(not(windows))]
        {
            std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .expect("spawn sleep")
        }
    }

    fn fake_manifest() -> OwnedPluginManifest {
        OwnedPluginManifest {
            id: "test.plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "0.1.0".to_string(),
            description: "test".to_string(),
            api_version: 1,
            ribbon_order: 0,
            xdata_apps: vec![],
            command_prefixes: vec![],
        }
    }

    fn fake_process() -> (PluginProcess, Stream) {
        let (host_stream, runner_stream) = connected_pair();
        let process = PluginProcess {
            stream: Mutex::new(Some(host_stream)),
            v4: None,
            child: Mutex::new(Some(sleepy_child())),
            id: "test.plugin".to_string(),
            manifest: fake_manifest(),
            ribbon: vec![],
            io_lines: Mutex::new(None),
        };
        (process, runner_stream)
    }

    #[test]
    fn dispatch_call_timeout_marks_process_dead() {
        let _env_guard = ENV_LOCK.lock().expect("env lock");
        let prev = std::env::var("OCS_PLUGIN_CALL_TIMEOUT_SECS").ok();
        let prev_floor = std::env::var("OCS_PLUGIN_TEST_FLOOR_SECS").ok();
        std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", "1");
        // Drop the Dispatch floor to 0 so the test fires at the 1 s base instead
        // of waiting out the real 10 s safety floor.
        std::env::set_var("OCS_PLUGIN_TEST_FLOOR_SECS", "0");
        let (process, runner_stream) = fake_process();

        let _runner = thread::spawn(move || {
            let mut peer = runner_stream;
            let req = recv::<HostToPlugin>(&mut peer).expect("read dispatch");
            assert!(
                matches!(req, HostToPlugin::Request(HostRequest::Dispatch { ref cmd }) if cmd == "HANG")
            );
            // Block until the host closes the connection after the timeout.
            let _ = recv::<HostToPlugin>(&mut peer);
        });

        let mut host = DummyHost::new(CadDocument::default());
        let start = Instant::now();
        let result = process.dispatch(&mut host, "HANG", &mut |_| {});
        let elapsed = start.elapsed();

        assert!(
            matches!(
                result,
                Err(PluginError::CallTimeout {
                    request: "Dispatch",
                    ..
                })
            ),
            "expected Dispatch timeout, got {result:?}"
        );
        assert!(
            elapsed >= Duration::from_secs(1),
            "timeout should respect the 1 s base: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "timed out too slowly: {elapsed:?}"
        );
        assert!(!process.is_alive(), "process should be marked dead");

        // Do not join the fake runner thread: it blocks until the host closes
        // the socket. In production the killed child process closes its end of
        // the socket and the reader thread exits; this test uses a local thread
        // instead, so we let it be reaped with the test process.
        match prev {
            Some(v) => std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", v),
            None => std::env::remove_var("OCS_PLUGIN_CALL_TIMEOUT_SECS"),
        }
        match prev_floor {
            Some(v) => std::env::set_var("OCS_PLUGIN_TEST_FLOOR_SECS", v),
            None => std::env::remove_var("OCS_PLUGIN_TEST_FLOOR_SECS"),
        }
    }

    #[test]
    fn dispatch_succeeds_with_nested_request_within_deadline() {
        let _env_guard = ENV_LOCK.lock().expect("env lock");
        let prev = std::env::var("OCS_PLUGIN_CALL_TIMEOUT_SECS").ok();
        std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", "2");
        let (process, runner_stream) = fake_process();

        let runner = thread::spawn(move || {
            let mut peer = runner_stream;
            let req = recv::<HostToPlugin>(&mut peer).expect("read dispatch");
            assert!(
                matches!(req, HostToPlugin::Request(HostRequest::Dispatch { ref cmd }) if cmd == "NESTED")
            );
            send(
                &mut peer,
                &PluginToHost::Request(Box::new(PluginRequest::PushInfo("hello".to_string()))),
            )
            .expect("send nested request");
            let resp = recv::<HostToPlugin>(&mut peer).expect("read nested response");
            assert!(matches!(resp, HostToPlugin::Response(ref r) if matches!(**r, PluginResponse::Ok)));
            send(&mut peer, &PluginToHost::Response(HostResponse::Bool(true)))
                .expect("send final response");
        });

        let mut host = DummyHost::new(CadDocument::default());
        let result = process.dispatch(&mut host, "NESTED", &mut |_| {});
        assert!(result.expect("dispatch succeeds"));
        assert!(process.is_alive(), "process should still be alive");
        let infos = host.take_push_info();
        assert_eq!(infos, vec!["hello".to_string()], "push_info should be delivered to host");

        // Clean up the helper child so it does not outlive the test.
        if let Some(mut child) = process
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = child.kill();
        }

        runner.join().expect("runner thread");
        match prev {
            Some(v) => std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", v),
            None => std::env::remove_var("OCS_PLUGIN_CALL_TIMEOUT_SECS"),
        }
    }

    #[test]
    fn runner_handshake_wrong_token_is_rejected() {
        let result = verify_runner_handshake(
            RunnerHandshake::Token("wrong-token".to_string()),
            "expected-token",
        );
        assert!(
            matches!(result, Err(PluginError::Runner(ref s)) if s == "authentication failed"),
            "expected authentication failure, got {result:?}"
        );
    }

    #[test]
    fn runner_handshake_correct_token_is_accepted() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OCS_PLUGIN_MAX_API_VERSION");
        let result = verify_runner_handshake(
            RunnerHandshake::Token("correct-token".to_string()),
            "correct-token",
        );
        assert!(result.is_ok(), "expected authentication success, got {result:?}");
        assert!(!result.unwrap(), "V3 token should select V3 path");
    }

    #[test]
    fn runner_handshake_tokenv4_selects_v4_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OCS_PLUGIN_MAX_API_VERSION");
        let result = verify_runner_handshake(
            RunnerHandshake::TokenV4 {
                token: "correct-token".to_string(),
                protocol_version: 4,
            },
            "correct-token",
        );
        assert!(result.is_ok(), "expected authentication success, got {result:?}");
        assert!(result.unwrap(), "TokenV4 should select V4 path");
    }

    #[test]
    fn runner_handshake_tokenv4_mismatched_version_is_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OCS_PLUGIN_MAX_API_VERSION");
        let result = verify_runner_handshake(
            RunnerHandshake::TokenV4 {
                token: "correct-token".to_string(),
                protocol_version: 5,
            },
            "correct-token",
        );
        assert!(
            matches!(result, Err(PluginError::Runner(ref s)) if s.contains("unsupported")),
            "expected unsupported version error, got {result:?}"
        );
    }

    #[test]
    fn runner_handshake_tokenv4_rejected_when_v4_disabled() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OCS_PLUGIN_MAX_API_VERSION", "3");
        let result = verify_runner_handshake(
            RunnerHandshake::TokenV4 {
                token: "correct-token".to_string(),
                protocol_version: 4,
            },
            "correct-token",
        );
        assert!(
            matches!(result, Err(PluginError::Runner(ref s)) if s.contains("disabled")),
            "expected V4 disabled error, got {result:?}"
        );
        std::env::remove_var("OCS_PLUGIN_MAX_API_VERSION");
    }
}
