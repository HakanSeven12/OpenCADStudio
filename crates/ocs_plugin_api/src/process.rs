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
//!    document source gate, then keeps the process alive.
//! 4. Host → plugin calls (`dispatch`, `execute_code`, interactive events) are
//!    sent over the socket with a configurable per-call timeout.
//! 5. Stdout/stderr of the child are drained into `PluginIoLine` records for
//!    logging and UI display.
//!
//! `NullHost` is a dummy `HostApi` implementation used for V4 paths that do not
//! supply a real host surface (e.g., interactive events). Timeouts and message
//! size limits are centralized here.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::Listener;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};

use crate::host::{
    CommandSource, CommandStep, ExecutionResult, HostApi, HostNotification, PluginNotification,
};
use crate::ipc::protocol::{
    Document, EntityType, Handle, HostRequest, HostResponse, HostToPlugin, InteractiveEvent,
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
pub use manager::{AttachOutcome, DispatchResult, NotificationHandler, PluginManager};

/// An owned Windows kernel HANDLE that stays usable from any thread. The host
/// hands runner spawn handles (the kill-on-close Job Object) between threads
/// (the reap runs wherever the owner lasts locks the cell); kernel object
/// handles are thread-portable, which raw-pointer [`Send`]/[`Sync`]
/// derivations do not express — hence this newtype.
#[cfg(windows)]
#[derive(Clone, Copy)]
struct SystemHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
// SAFETY: a HANDLE is an integer-like, thread-portable reference to a kernel
// object; nothing about it binds to the creating thread.
unsafe impl Send for SystemHandle {}

/// One runner child plus the OS handles that tie its process tree to the host
/// lifetime. The runner runs in its own process group (or Windows Job) so
/// reaping signals the whole tree: a runner's own children (e.g. a spawned
/// Python child) otherwise inherit the runner's socket and token and keep the
/// host's connection semi-open after an abandoned plugin.
struct RunnerChild {
    child: Child,
    /// Windows kill-on-close Job Object (null when the host itself is
    /// inside a Job that forbids assignment, making this a best-effort net).
    /// The tree dies when the job terminates or the handle closes.
    #[cfg(windows)]
    job: SystemHandle,
}

type RunnerChildCell = Mutex<Option<RunnerChild>>;
type SharedRunnerChild = Arc<RunnerChildCell>;

impl RunnerChild {
    fn new(child: Child) -> Self {
        #[cfg(windows)]
        {
            // Bind the job BEFORE taking ownership of the child in the
            // struct: the job assignment needs the child's raw handle.
            let job = SystemHandle(create_kill_on_close_job(&child));
            Self { child, job }
        }
        #[cfg(unix)]
        {
            // `setsid` ran in the child's pre-exec hook: its pgid is its own
            // pid, and the kill signaling uses `-pid` below.
            Self { child }
        }
        #[cfg(not(any(windows, unix)))]
        {
            Self { child }
        }
    }
}

/// Create a kill-on-close Job Object with `child` assigned. Returns a null
/// handle when the OS refuses (the direct-child kill still applies then).
#[cfg(windows)]
fn create_kill_on_close_job(child: &Child) -> windows_sys::Win32::Foundation::HANDLE {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // SAFETY: all calls use plain OS handles and zeroed-parameter patterns
    // from the windows-sys contract; no pointers outlive the call.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            eprintln!("[plugin] cannot create the runner Job Object");
            return std::ptr::null_mut();
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            eprintln!("[plugin] cannot set kill-on-close on the runner Job Object");
            let _ = CloseHandle(job);
            return std::ptr::null_mut();
        }
        if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
            eprintln!("[plugin] cannot bind the runner to its Job Object");
            let _ = CloseHandle(job);
            return std::ptr::null_mut();
        }
        job
    }
}

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
#[derive(Default)]
struct NullHost {
    scratch_doc: Option<Document>,
}

impl HostApi for NullHost {
    fn tab_index(&self) -> usize {
        0
    }
    fn document(&self) -> &Document {
        if let Some(doc) = &self.scratch_doc {
            doc
        } else {
            static NULL_DOC: std::sync::OnceLock<Document> = std::sync::OnceLock::new();
            NULL_DOC.get_or_init(Document::new)
        }
    }
    fn document_mut(&mut self) -> &mut Document {
        self.scratch_doc.get_or_insert_with(Document::new)
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
    fn write_record(
        &mut self,
        _handle: Handle,
        _record: crate::ipc::protocol::ExtendedDataRecord,
    ) -> bool {
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
    ) -> Option<&mut (dyn std::any::Any + Send + Sync)> {
        None
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
///
/// The host spawns itself (a 100+ MB executable) as the runner, and an
/// antivirus scan of that image can take longer than 30 s on Windows. A
/// runner slowed that way is healthy, so the wait is 90 s;
/// `OCS_PLUGIN_SPAWN_TIMEOUT_SECS` overrides it.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(90);

/// How often the connect wait checks whether the runner has already exited.
const SPAWN_EXIT_POLL: Duration = Duration::from_millis(50);

/// How long a runner that exited before connecting gets to flush its stderr
/// into the spawn error.
const SPAWN_STDERR_GRACE: Duration = Duration::from_millis(500);

/// Maximum lines drained from a plugin's stdout/stderr per `drain_io` call:
/// bounds the host's per-tick work without ever losing the queue contents
/// (undrained lines stay queued for the next tick).
const MAX_IO_LINES_PER_DRAIN: usize = 2048;

/// Maximum characters kept from one plugin output line. The tail of an
/// over-long line is skipped with an explicit marker, so a stream that never
/// emits a newline cannot grow host memory without bound.
const MAX_IO_LINE_LEN: usize = 16 * 1024;

/// Maximum queued plugin output lines. When a plugin floods faster than the
/// host drains, new lines are dropped instead of growing the queue.
const MAX_IO_QUEUE: usize = 8192;

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
pub(crate) fn request_timeout(kind: &'static str) -> Duration {
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
        "Dispatch" | "CustomGripSettle" => Duration::from_secs(10),
        "InteractiveEvent" | "GetPrompt" | "NeedsEntityPick" | "CustomGripBegin"
        | "CustomGripCancel" => Duration::from_secs(2),
        "CursorMove" | "CustomGripMove" => Duration::from_millis(500),
        "ExecuteCode" => execute_code_timeout(),
        _ => Duration::from_secs(1),
    };
    let effective = base.max(floor);
    // The floors only protect against an unsafe LOW setting. The default
    // global timeout (30 s) used to dominate them in the other direction, so
    // every cursor/preview tick froze the UI for the whole global budget. The
    // UI-blocking kinds are now capped as well: a stalled preview fails within
    // its cap and the command reports an honest timeout instead of freezing
    // the app (bounded behavior change; commands stay responsive).
    if let Some(cap) = ui_blocking_cap(kind) {
        effective.min(cap.max(floor))
    } else {
        effective
    }
}

/// Upper bounds for request kinds that run inside the UI loop while the user
/// waits: a preview tick can never exceed 2 s and a per-step interactive probe
/// never exceeds 10 s, no matter how the operator configured the global
/// timeout. `Dispatch`, dialogs and ExecuteCode keep the global budget.
fn ui_blocking_cap(kind: &'static str) -> Option<Duration> {
    match kind {
        "CursorMove" | "CustomGripMove" => Some(Duration::from_secs(2)),
        "InteractiveEvent" | "GetPrompt" | "NeedsEntityPick" | "IsSelectionGathering"
        | "AcceptsDragSelection" | "SelectionRejectsLockedLayers" | "DropInteractive"
        | "CustomGripBegin" | "CustomGripCancel" | "GetModalDialog" => {
            Some(Duration::from_secs(10))
        }
        _ => None,
    }
}

pub(crate) fn request_kind(req: &HostRequest) -> &'static str {
    match req {
        HostRequest::GetManifest => "GetManifest",
        HostRequest::GetRibbon => "GetRibbon",
        HostRequest::Dispatch { .. } => "Dispatch",
        HostRequest::InteractiveEvent { .. } => "InteractiveEvent",
        HostRequest::GetPrompt { .. } => "GetPrompt",
        HostRequest::NeedsEntityPick { .. } => "NeedsEntityPick",
        HostRequest::IsSelectionGathering { .. } => "IsSelectionGathering",
        HostRequest::AcceptsDragSelection { .. } => "AcceptsDragSelection",
        HostRequest::SelectionRejectsLockedLayers { .. } => "SelectionRejectsLockedLayers",
        HostRequest::ExecuteCode { .. } => "ExecuteCode",
        HostRequest::DropInteractive { .. } => "DropInteractive",
        HostRequest::CursorMove { .. } => "CursorMove",
        HostRequest::CustomGripEvent { event, .. } => match event.phase {
            crate::host::CustomGripPhase::Begin => "CustomGripBegin",
            crate::host::CustomGripPhase::Move => "CustomGripMove",
            crate::host::CustomGripPhase::Settle => "CustomGripSettle",
            crate::host::CustomGripPhase::Cancel => "CustomGripCancel",
        },
        HostRequest::GetModalDialog { .. } => "GetModalDialog",
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
    /// Arc'd together with the spawn guard so a failed bootstrap tears the
    /// socket down without racing a partially built `Self`.
    stream: Arc<Mutex<Option<Stream>>>,
    v4: Option<V4Connection>,
    child: SharedRunnerChild,
    id: String,
    manifest: OwnedPluginManifest,
    ribbon: Vec<OwnedRibbonGroupAlias>,
    io_lines: Mutex<Option<mpsc::Receiver<PluginIoLine>>>,
}

impl PluginProcess {
    /// Spawn the plugin cdylib in a separate process and connect to it.
    ///
    /// `expected_plugin_id` — when the caller knows the package manifest's
    /// plugin id (e.g. from `plugin.toml`), the runner's own manifest must
    /// agree; the host refuses to adopt a self-asserted identity otherwise.
    pub fn spawn(
        cdylib_path: &Path,
        host: &mut dyn HostApi,
        notification_handler: NotificationHandler,
        expected_plugin_id: Option<&str>,
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

        let mut command = Command::new(&runner_path);
        command
            .arg("--ocs-plugin-runner")
            .arg(&socket_name)
            .arg(cdylib_path)
            .env(PLUGIN_TOKEN_ENV, &token)
            .env(
                crate::version_info::WIRE_SCHEMA_ENV,
                crate::version_info::wire_schema(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: the closure runs between fork and exec in the child;
            // it only calls setsid. The runner then leads its own session
            // (pgid == its pid), letting reap() signal the whole tree.
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        let mut child = command.spawn()?;

        // Start draining the child's stdout/stderr immediately. If the
        // plugin prints during GetManifest/GetRibbon, a full pipe buffer
        // would otherwise block (or kill) the runner before the host ever
        // reads it. The plugin id is filled in once we read the manifest.
        // The channel is bounded and each line is length-capped, so a plugin
        // writing endless output cannot grow host memory without bound; when
        // the queue is full lines are dropped (plugin output is auxiliary).
        let stream: Arc<Mutex<Option<Stream>>> = Arc::new(Mutex::new(None));
        let (io_tx, io_rx) = mpsc::sync_channel::<PluginIoLine>(MAX_IO_QUEUE);
        let plugin_id_for_io: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let last_stderr: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let (stderr_done_tx, stderr_done) = mpsc::channel::<()>();
        if let Some(stdout) = child.stdout.take() {
            let io_tx = io_tx.clone();
            let plugin_id = Arc::clone(&plugin_id_for_io);
            let last_stderr = Arc::clone(&last_stderr);
            std::thread::spawn(move || {
                pump_stream_lines(stdout, IoStream::Stdout, &io_tx, &plugin_id, &last_stderr);
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let plugin_id = Arc::clone(&plugin_id_for_io);
            let last_stderr = Arc::clone(&last_stderr);
            std::thread::spawn(move || {
                pump_stream_lines(stderr, IoStream::Stderr, &io_tx, &plugin_id, &last_stderr);
                let _ = stderr_done_tx.send(());
            });
        }
        let child: SharedRunnerChild = Arc::new(Mutex::new(Some(RunnerChild::new(child))));
        // A failed bootstrap (accept/handshake/manifest/ribbon) below returns
        // through this guard: the socket is closed and the runner tree reaped
        // instead of leaking a half-initialized process. Success defuses it.
        let guard = SpawnGuard {
            stream: Arc::clone(&stream),
            child: Arc::clone(&child),
        };

        // Accept the runner connection with a timeout so a hung/crashed runner
        // does not block the host indefinitely. The wait is a POLL loop that
        // also watches the runner itself: a runner that exits while connecting
        // (stale wire schema, unloadable library, crash) ends the wait within
        // one poll tick instead of burning the whole spawn budget — a
        // start-of-day worse case of 90 s per dead plugin had every launch
        // freeze for a minute and a half with a perfectly healthy host.
        let accept_deadline = Instant::now() + spawn_timeout();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(listener.accept());
        });
        // The wait also watches the runner itself: one that exits before it
        // connects (a plugin built against another wire schema, a crash while
        // loading) fails the spawn at once instead of after the whole budget.
        let accepted = loop {
            let remaining = accept_deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining.min(SPAWN_EXIT_POLL)) {
                Ok(Ok(stream)) => {
                    vlog!("[plugin] runner connected");
                    break Some(stream);
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(mpsc::RecvTimeoutError::Timeout)
                    if !remaining.is_zero() && child_status(&child).is_none() => {}
                Err(_) => {
                    if child_status(&child).is_some() {
                        // Let the drain thread read the runner's last words.
                        let _ = stderr_done.recv_timeout(SPAWN_STDERR_GRACE);
                    }
                    let status = spawn_failure_status(&child, &last_stderr);
                    reap_runner_cell(&child);
                    return Err(PluginError::RunnerCrashed {
                        request: "spawn/accept",
                        status,
                    });
                }
            }
        };
        *stream.lock().unwrap_or_else(|e| e.into_inner()) = accepted;

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
            let stream = stream
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .expect("V4 stream");
            let plugin_id_placeholder = Arc::new(std::sync::Mutex::new(String::new()));
            let handler_for_v4: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> = {
                let plugin_id = Arc::clone(&plugin_id_placeholder);
                let base_handler = Arc::clone(&notification_handler);
                Arc::new(move |command_id, notification| {
                    let id = plugin_id.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    base_handler(&id, command_id, notification);
                })
            };
            let v4_conn =
                V4Connection::new(stream, handler_for_v4, Some(Arc::clone(&child)))?;
            // Owner-scoped declarations (entity classes, quad panels, custom
            // grips) arrive as nested requests while GetManifest is in
            // flight — the plugin registers them as part of its own boot.
            // The connection carries the LOADED PACKAGE's declared id for
            // that window (the transport identity for the request shapes in
            // this protocol does not travel the wire). The manifest response
            // below must confirm the id; a mismatched manifest refuses the
            // spawn, so the attribution stays host-verified. Without a
            // declared id (test loads) the window keeps the empty identity,
            // and the declaration law refuses it as before.
            if let Some(expected) = expected_plugin_id {
                v4_conn.attach_plugin_id(expected);
            }
            manifest = match v4_conn.call(host, HostRequest::GetManifest, no_op)? {
                HostResponse::Manifest(m) => m,
                other => return Err(PluginError::UnexpectedResponse(Box::new(other))),
            };
            *plugin_id_placeholder
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = manifest.id.clone();
            v4_conn.attach_plugin_id(&manifest.id);
            ribbon = match v4_conn.call(host, HostRequest::GetRibbon, no_op)? {
                HostResponse::Ribbon(r) => r,
                other => return Err(PluginError::UnexpectedResponse(Box::new(other))),
            };
            v4 = Some(v4_conn);
        } else {
            // The runner first answers GetManifest and GetRibbon so the host can
            // build the UI without keeping the plugin object alive.
            // The manifest id is unknown until GetManifest returns, so nested
            // requests during the handshake serve with an empty identity — no
            // grip declaration can exist in that window.
            manifest = match call(&stream, &child, host, "", HostRequest::GetManifest, no_op)? {
                HostResponse::Manifest(m) => m,
                other => return Err(PluginError::UnexpectedResponse(Box::new(other))),
            };
            ribbon = match call(&stream, &child, host, "", HostRequest::GetRibbon, no_op)? {
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

        // The runner's manifest is the plugin's identity CLAIM. When the
        // loaded package declared its own id, that claim must match — a
        // repackaged binary cannot silently impersonate another plugin's
        // registered panels/classes/state.
        if let Some(expected) = expected_plugin_id {
            if !manifest.id.eq_ignore_ascii_case(expected) {
                return Err(PluginError::Runner(format!(
                    "plugin manifest claims id `{}` but the package declared `{expected}` — \
                     refusing to adopt a mismatched identity",
                    manifest.id
                )));
            }
        }

        let id = manifest.id.clone();
        *plugin_id_for_io.lock().unwrap_or_else(|e| e.into_inner()) = id.clone();

        // Bootstrap complete: the guard's teardown must not fire.
        std::mem::forget(guard);

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
    /// the host command line. The drain is capped per call — a plugin that
    /// floods output cannot turn one drain into unbounded host work; the
    /// remainder stays queued for the next tick.
    pub fn drain_io(&self) -> Vec<PluginIoLine> {
        let mut rx = self.io_lines.lock().unwrap_or_else(|e| e.into_inner());
        let Some(rx) = rx.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while out.len() < MAX_IO_LINES_PER_DRAIN {
            match rx.try_recv() {
                Ok(line) => out.push(line),
                Err(_) => break,
            }
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
                &self.id,
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

    /// Issue a custom-grip drag event WITHOUT waiting (API v12; begin, move,
    /// settle and cancel alike). The app parks move points and keeps at most
    /// one move in flight, drops superseded cursor points, and never blocks
    /// the UI loop on plugin latency — [`Self::custom_grip_poll`] collects the
    /// answers at pump ticks, and [`Self::custom_grip_abandon`] is the
    /// watchdog for plugins that never answer. Nested command frames
    /// (`run_command`/`execute_command`) are REFUSED in this serving context:
    /// their execution would need the host message loop the grip delivery
    /// cannot pump (the v11.1 UI-freeze wedge); every other nested request —
    /// including `push_undo` and direct writes — is served inline, so the
    /// settle callback keeps its full HostApi write window. V4 transport
    /// only; V3 connections refuse (no multiplexed link, no nested writes).
    #[allow(clippy::result_large_err)]
    pub fn custom_grip_event_send(
        &self,
        tab_index: usize,
        event: crate::host::CustomGripEvent,
    ) -> Result<u64, PluginError> {
        let Some(v4) = &self.v4 else {
            return Err(PluginError::Runner(
                "custom grips require the V4 protocol".into(),
            ));
        };
        v4.send_grip_event(tab_index, event)
    }

    /// Poll (bounded) for the next outstanding custom-grip answer, serving
    /// nested plugin requests inline on the way. `Ok(None)`: not answered
    /// yet. The poll never marks the connection dead.
    #[allow(clippy::result_large_err)]
    pub fn custom_grip_poll(
        &self,
        host: &mut dyn HostApi,
        wait: std::time::Duration,
    ) -> Result<Option<(u64, crate::host::CustomGripResponse)>, PluginError> {
        let Some(v4) = &self.v4 else {
            return Err(PluginError::Runner(
                "custom grips require the V4 protocol".into(),
            ));
        };
        match v4.poll_grip(host, wait)? {
            Some((id, HostResponse::CustomGrip(resp))) => Ok(Some((id, resp))),
            Some((_, other)) => Err(PluginError::UnexpectedResponse(Box::new(other))),
            None => Ok(None),
        }
    }

    /// Whether a custom-grip delivery is still unanswered.
    pub fn custom_grip_move_in_flight(&self) -> bool {
        self.v4
            .as_ref()
            .is_some_and(|v4| v4.has_grip_move_in_flight())
    }

    /// The watchdog (API v12): abandon a grip delivery that outlived its
    /// deadline by tearing the plugin's connection down. The UI loop
    /// recovers, the drag ends as a cancel, an error line names the plugin;
    /// the runner reaps the dead connection. The plugin process is the
    /// untrusted side — never waited on.
    pub fn custom_grip_abandon(&self) {
        if let Some(v4) = &self.v4 {
            v4.abandon("grip-event delivery deadline exceeded");
        }
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

    /// Legacy (non-V4) round trip for simple interactive calls: send `req`,
    /// then loop until the matching response arrives, answering any nested
    /// plugin request with an error. `kind` drives the timeout floor;
    /// `nested_what` names the operation in that error; `extract` pulls the
    /// expected payload out of the response.
    fn call_simple<T>(
        &self,
        req: HostRequest,
        kind: &'static str,
        nested_what: &'static str,
        extract: impl Fn(HostResponse) -> Result<T, Box<HostResponse>>,
    ) -> Result<T, PluginError> {
        self.send_request(req)?;
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
                PluginToHost::Response(resp) => match extract(resp) {
                    Ok(v) => return Ok(v),
                    Err(other) => return Err(PluginError::UnexpectedResponse(other)),
                },
                PluginToHost::Request(req) => {
                    let resp = crate::ipc::protocol::PluginResponse::Error(format!(
                        "unexpected nested request during {nested_what}: {req:?}"
                    ));
                    self.send_response(resp)?;
                }
            }
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
                &mut NullHost::default(),
                HostRequest::InteractiveEvent { command_id, event },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::CommandStep(s) => Ok(*s),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.call_simple(
                HostRequest::InteractiveEvent { command_id, event },
                "InteractiveEvent",
                "interactive event",
                |resp| match resp {
                    HostResponse::CommandStep(s) => Ok(*s),
                    other => Err(Box::new(other)),
                },
            )
        }
    }

    /// Release a completed or abandoned V4 interactive command in the runner.
    pub fn drop_interactive(&self, command_id: u64) -> Result<(), PluginError> {
        let Some(v4) = &self.v4 else {
            return Ok(());
        };
        match v4.call(
            &mut NullHost::default(),
            HostRequest::DropInteractive { command_id },
            &mut |_| {},
        )? {
            HostResponse::Bool(true) => Ok(()),
            other => Err(PluginError::UnexpectedResponse(Box::new(other))),
        }
    }

    /// Ask the plugin process for the current prompt of an interactive command.
    pub fn get_prompt(&self, command_id: u64) -> Result<String, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost::default(),
                HostRequest::GetPrompt { command_id },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::Text(s) => Ok(s),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.call_simple(
                HostRequest::GetPrompt { command_id },
                "GetPrompt",
                "get_prompt",
                |resp| match resp {
                    HostResponse::Text(s) => Ok(s),
                    other => Err(Box::new(other)),
                },
            )
        }
    }

    /// Ask the plugin process whether an interactive command wants object picks.
    pub fn needs_entity_pick(&self, command_id: u64) -> Result<bool, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost::default(),
                HostRequest::NeedsEntityPick { command_id },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::Bool(b) => Ok(b),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.call_simple(
                HostRequest::NeedsEntityPick { command_id },
                "NeedsEntityPick",
                "needs_entity_pick",
                |resp| match resp {
                    HostResponse::Bool(b) => Ok(b),
                    other => Err(Box::new(other)),
                },
            )
        }
    }

    /// Ask the plugin process whether an interactive command is in selection-gathering mode.
    pub fn is_selection_gathering(&self, command_id: u64) -> Result<bool, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost::default(),
                HostRequest::IsSelectionGathering { command_id },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::Bool(b) => Ok(b),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.call_simple(
                HostRequest::IsSelectionGathering { command_id },
                "IsSelectionGathering",
                "is_selection_gathering",
                |resp| match resp {
                    HostResponse::Bool(b) => Ok(b),
                    other => Err(Box::new(other)),
                },
            )
        }
    }

    /// Ask the plugin process whether an interactive command accepts drag selections.
    pub fn accepts_drag_selection(&self, command_id: u64) -> Result<bool, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost::default(),
                HostRequest::AcceptsDragSelection { command_id },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::Bool(b) => Ok(b),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.call_simple(
                HostRequest::AcceptsDragSelection { command_id },
                "AcceptsDragSelection",
                "accepts_drag_selection",
                |resp| match resp {
                    HostResponse::Bool(b) => Ok(b),
                    other => Err(Box::new(other)),
                },
            )
        }
    }

    /// Ask the plugin process whether an interactive command rejects locked layers during selection.
    pub fn selection_rejects_locked_layers(&self, command_id: u64) -> Result<bool, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost::default(),
                HostRequest::SelectionRejectsLockedLayers { command_id },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::Bool(b) => Ok(b),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.call_simple(
                HostRequest::SelectionRejectsLockedLayers { command_id },
                "SelectionRejectsLockedLayers",
                "selection_rejects_locked_layers",
                |resp| match resp {
                    HostResponse::Bool(b) => Ok(b),
                    other => Err(Box::new(other)),
                },
            )
        }
    }

    /// Ask the plugin process for real-time preview wires for the cursor position.
    pub fn on_cursor_move(
        &self,
        command_id: u64,
        pt: [f64; 3],
    ) -> Result<Vec<crate::host::PreviewWire>, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost::default(),
                HostRequest::CursorMove { command_id, pt },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::PreviewWires(w) => Ok(w),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.call_simple(
                HostRequest::CursorMove { command_id, pt },
                "CursorMove",
                "cursor_move",
                |resp| match resp {
                    HostResponse::PreviewWires(w) => Ok(w),
                    other => Err(Box::new(other)),
                },
            )
        }
    }

    /// Ask the plugin process whether an interactive command has an active modal dialog.
    pub fn get_modal_dialog(
        &self,
        command_id: u64,
    ) -> Result<Option<crate::dialog::DialogDef>, PluginError> {
        if let Some(v4) = &self.v4 {
            let resp = v4.call(
                &mut NullHost::default(),
                HostRequest::GetModalDialog { command_id },
                &mut |_| {},
            )?;
            match resp {
                HostResponse::ModalDialog(d) => Ok(d),
                other => Err(PluginError::UnexpectedResponse(Box::new(other))),
            }
        } else {
            self.call_simple(
                HostRequest::GetModalDialog { command_id },
                "GetModalDialog",
                "get_modal_dialog",
                |resp| match resp {
                    HostResponse::ModalDialog(d) => Ok(d),
                    other => Err(Box::new(other)),
                },
            )
        }
    }

    pub fn is_alive(&self) -> bool {
        if let Some(v4) = &self.v4 {
            return v4.is_alive();
        }
        let mut guard = self.child.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(runner) => match runner.child.try_wait() {
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
        if let Some(mut runner) = child {
            let _ = runner.child.kill();
            let start = Instant::now();
            while start.elapsed() < timeout {
                if let Ok(Some(status)) = runner.child.try_wait() {
                    return Some(status);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            reap(runner);
        }
        None
    }

    /// Take the stream and child handles out of the process. After this the
    /// process is considered shut down and any further IPC will fail.
    fn take_resources(&self) -> (Option<Stream>, Option<RunnerChild>) {
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

/// Kill a runner's whole process tree and reap them without blocking the
/// caller. The blocking `wait()` runs in a detached thread so the host never
/// stalls on a plugin, and the child is reaped rather than left as a zombie on
/// Unix.
fn reap(mut runner: RunnerChild) {
    // The runner runs in its own session (see spawn), so the negative-pid
    // signal reaches every process in its group — grandchildren included.
    // A hung runner is exactly the case this handles, so hang up hard.
    #[cfg(unix)]
    {
        let pgid = -(runner.child.id() as i32);
        // SAFETY: signal delivery to a live pid; ESRCH (already gone) is fine.
        unsafe { libc::kill(pgid, libc::SIGKILL) };
    }
    // The Job Object covers the whole tree; terminate it so a runner that
    // ignores a direct kill cannot leave grandchildren behind.
    #[cfg(windows)]
    {
        if !runner.job.0.is_null() {
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            // SAFETY: the job handle belongs to this RunnerChild; termination
            // is asynchronous but the direct kill/wait below still proceeds.
            unsafe { TerminateJobObject(runner.job.0, 0) };
        }
    }
    let _ = runner.child.kill();
    // Closing the job handle runs the kill-on-close catch-all for anything
    // the termination missed, and releases the kernel object.
    #[cfg(windows)]
    {
        if !runner.job.0.is_null() {
            use windows_sys::Win32::Foundation::CloseHandle;
            // SAFETY: our own job handle, closed exactly once.
            unsafe { CloseHandle(runner.job.0) };
            runner.job = SystemHandle(std::ptr::null_mut());
        }
    }
    std::thread::spawn(move || {
        let _ = runner.child.wait();
    });
}

/// Take and kill whatever runner still sits in `cell`, leaving it empty.
fn reap_runner_cell(cell: &SharedRunnerChild) {
    let runner = cell.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(runner) = runner {
        reap(runner);
    }
}

/// Pump one runner output stream into the bounded io queue, capping every
/// line at [`MAX_IO_LINE_LEN`] bytes. An over-long line is truncated with an
/// explicit marker and its tail skipped, so a stream that never emits a
/// newline cannot grow host memory without bound. A full queue drops the line
/// rather than wedging the pipe reader (plugin output is auxiliary).
fn pump_stream_lines(
    mut reader: impl Read + Send,
    source: IoStream,
    io_tx: &mpsc::SyncSender<PluginIoLine>,
    plugin_id: &Arc<Mutex<String>>,
    last_stderr: &Arc<Mutex<String>>,
) {
    fn emit(
        buffer: &[u8],
        truncated: bool,
        source: IoStream,
        io_tx: &mpsc::SyncSender<PluginIoLine>,
        plugin_id: &Arc<Mutex<String>>,
        last_stderr: &Arc<Mutex<String>>,
    ) {
        let mut text = String::from_utf8_lossy(buffer).into_owned();
        if truncated {
            text.push_str(" …[line truncated at 16 KiB]");
        }
        if matches!(source, IoStream::Stderr) {
            if let Ok(mut guard) = last_stderr.lock() {
                *guard = text.clone();
            }
        }
        let id = plugin_id.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let _ = io_tx.try_send(PluginIoLine {
            source,
            plugin_id: id,
            text,
        });
    }

    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    // Leftover bytes of a chunk already scanned past a newline, with the
    // scan position into them (so one big read can yield several lines).
    let mut carry: Option<(Vec<u8>, usize)> = None;
    'stream: loop {
        buffer.clear();
        let mut truncated = false;
        let mut line_done = false;
        let mut eof_reached = false;
        while !line_done {
            // Next bytes to scan: the carry remainder, or a fresh chunk
            // read (whole-chunk reads: one syscall per 8 KiB, not per byte).
            let (data, mut pos) = match carry.take() {
                Some((data, pos)) => (data, pos),
                None => match reader.read(&mut chunk) {
                    Ok(0) => {
                        eof_reached = true;
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        eof_reached = true;
                        break;
                    }
                    Ok(n) => (chunk[..n].to_vec(), 0),
                },
            };
            while pos < data.len() {
                let byte = data[pos];
                pos += 1;
                if byte == b'\n' {
                    line_done = true;
                    break;
                }
                if buffer.len() < MAX_IO_LINE_LEN {
                    buffer.push(byte);
                } else {
                    truncated = true;
                }
            }
            if pos < data.len() {
                // Either a newline ended this line mid-chunk, or the reader
                // is done for this iteration: keep the unscanned remainder.
                carry = Some((data, pos));
            }
        }
        if !buffer.is_empty() || truncated {
            emit(
                &buffer,
                truncated,
                source,
                io_tx,
                plugin_id,
                last_stderr,
            );
        }
        if eof_reached {
            break 'stream;
        }
    }
}

/// Owns the half-initialized runner while spawn bootstrap work proceeds. Every
/// early-return path below the accept loop drops this guard, which closes the
/// host-side socket and reaps the runner's whole tree, so a failed load can
/// never leak a half-initialized child process. Success defuses it
/// (`std::mem::forget`) once the [`PluginProcess`] owns the same handles.
struct SpawnGuard {
    stream: Arc<Mutex<Option<Stream>>>,
    child: SharedRunnerChild,
}

impl Drop for SpawnGuard {
    fn drop(&mut self) {
        mark_dead(&self.stream, &self.child);
    }
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
fn mark_dead(stream: &Mutex<Option<Stream>>, child: &SharedRunnerChild) {
    // On a timeout the live `Stream` is owned by the detached reader thread, so
    // this `take()` usually clears an already-`None` slot. The host end is not
    // closed here directly: killing the child below shuts its socket end, which
    // unblocks the reader thread's `recv` and lets it drop the `Stream`. If the
    // kill fails the reader thread can stay parked until the OS tears the socket
    // down, but the process is still treated as dead for all further IPC.
    let _ = stream.lock().unwrap_or_else(|e| e.into_inner()).take();
    reap_runner_cell(child);
}

/// Check whether a child process has already exited, returning its status if so.
fn child_status(child: &SharedRunnerChild) -> Option<std::process::ExitStatus> {
    let mut guard = child.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(runner) => runner.child.try_wait().ok().flatten(),
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
fn spawn_failure_status(child: &SharedRunnerChild, last_stderr: &Mutex<String>) -> String {
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
    child: &SharedRunnerChild,
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
/// requests inline using the supplied `HostApi`. `plugin_id` identifies the
/// process whose requests are served (attached to grip declarations; before
/// the manifest is read it is empty, where no grip traffic can exist).
fn call(
    stream: &Mutex<Option<Stream>>,
    child: &SharedRunnerChild,
    host: &mut dyn HostApi,
    plugin_id: &str,
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
                let resp =
                    handle_plugin_request(host, plugin_id, *plugin_req, on_start_interactive, true);
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

/// Constant-time equality for the presented auth token (review finding 11):
/// no branch depends on WHERE a mismatch appears, so a local process probing
/// the negotiated socket learns the token byte-at-a-time no faster than by
/// trying full candidates.
fn tokens_equal(presented: &str, expected: &str) -> bool {
    let presented = presented.as_bytes();
    let expected = expected.as_bytes();
    presented.len() == expected.len()
        && presented
            .iter()
            .zip(expected)
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
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
        RunnerHandshake::Token(ref presented) if tokens_equal(presented, expected_token) => {
            vlog!("[plugin] runner authenticated (V3)");
            Ok(false)
        }
        RunnerHandshake::TokenV4 {
            token: ref presented,
            protocol_version,
        } if tokens_equal(presented, expected_token) => {
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

/// Generate a unique local socket name. Beyond the pid+counter, a random
/// suffix from the CSPRNG makes the name unpredictable on multi-user machines
/// and un-guessable for the pre-spawn listener race (P2-1).
fn generate_socket_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0u8; 8];
    // A CSPRNG failure never becomes a predictable all-zero suffix (that
    // would re-open the guessing/squatting the suffix exists to close): the
    // pid+counter name alone stays unique within this host process, matching
    // the pre-hardening shape.
    match getrandom::getrandom(&mut bytes) {
        Ok(()) => format!(
            "ocs_plugin_{}_{n}_{:016x}",
            std::process::id(),
            u64::from_le_bytes(bytes)
        ),
        Err(_) => format!("ocs_plugin_{}_{n}", std::process::id()),
    }
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

/// In-crate test support: build a `PluginProcess` whose fields are otherwise
/// private to this module, so sibling test modules (`process::manager`) can
/// put one in a `PluginManager` without spawning a real runner.
#[cfg(all(test, feature = "host"))]
impl PluginProcess {
    /// A process backed by a real sleeping child process and a local socket
    /// pair, carrying the given manifest (no cdylib, no runner). The
    /// runner-side peer socket is dropped immediately; nothing reads it.
    pub(crate) fn fake(manifest: OwnedPluginManifest) -> Self {
        use interprocess::local_socket::traits::Stream as StreamTrait;

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "ocs_plugin_fake_process_test_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let name_ref = name
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("valid name");
        let listener = ListenerOptions::new()
            .name(name_ref)
            .create_sync()
            .expect("listener");
        let client_name = name.clone();
        let client_thread: std::thread::JoinHandle<Stream> = std::thread::spawn(move || {
            StreamTrait::connect(client_name.to_ns_name::<GenericNamespaced>().unwrap())
                .expect("connect")
        });
        let host_stream = listener.accept().expect("accept");
        drop(client_thread.join().expect("client thread"));

        Self {
            stream: Arc::new(Mutex::new(Some(host_stream))),
            v4: None,
            child: Arc::new(Mutex::new(Some(RunnerChild::new(sleepy_test_child())))),
            id: manifest.id.clone(),
            manifest,
            ribbon: vec![],
            io_lines: Mutex::new(None),
        }
    }
}

/// A child that lives ~30 s — long enough to read as "alive", short enough to
/// never hang a leaked test. Shared by `PluginProcess::fake` and this module's
/// own tests.
#[cfg(all(test, feature = "host"))]
fn sleepy_test_child() -> Child {
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

    #[test]
    fn null_host_ensure_plugin_state_does_not_panic() {
        let mut null = NullHost::default();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = null.ensure_plugin_state_any("opencad.demo", &mut || Box::new(7u32));
        }));
        assert!(
            outcome.is_ok(),
            "NullHost ensure_plugin_state must degrade, not panic: {:?}",
            outcome.err()
        );
        assert!(null
            .ensure_plugin_state_any("opencad.demo", &mut || Box::new(7u32))
            .is_none());
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
    use document::xdata::ExtendedDataRecord;
    use document::{Document, EntityType, Handle};
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
        doc: Document,
        push_info_messages: StdMutex<Vec<String>>,
    }

    impl DummyHost {
        fn new(doc: Document) -> Self {
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
        fn document(&self) -> &Document {
            &self.doc
        }
        fn document_mut(&mut self) -> &mut Document {
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
            self.push_info_messages
                .lock()
                .unwrap()
                .push(msg.to_string());
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
        ) -> Option<&mut (dyn std::any::Any + Send + Sync)> {
            None
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
            entity_classes: None,
        }
    }

    fn fake_process() -> (PluginProcess, Stream) {
        let (host_stream, runner_stream) = connected_pair();
        let process = PluginProcess {
            stream: Arc::new(Mutex::new(Some(host_stream))),
            v4: None,
            child: Arc::new(Mutex::new(Some(RunnerChild::new(sleepy_child())))),
            id: "test.plugin".to_string(),
            manifest: fake_manifest(),
            ribbon: vec![],
            io_lines: Mutex::new(None),
        };
        (process, runner_stream)
    }

    /// A runner that exits before it connects (a plugin built against another
    /// wire schema, a missing dependency, a crash in `on_load`) must fail the
    /// spawn as soon as it exits, not after the whole connect budget.
    #[test]
    fn spawn_fails_as_soon_as_the_runner_exits_before_connecting() {
        let _env_guard = ENV_LOCK.lock().expect("env lock");
        let prev_runner = std::env::var("OCS_PLUGIN_RUNNER_EXE").ok();
        let prev_timeout = std::env::var("OCS_PLUGIN_SPAWN_TIMEOUT_SECS").ok();
        // The test binary itself stands in for the runner: it rejects
        // `--ocs-plugin-runner` as an unknown option and exits at once.
        let this_test = std::env::current_exe().expect("test executable");
        std::env::set_var("OCS_PLUGIN_RUNNER_EXE", &this_test);
        std::env::set_var("OCS_PLUGIN_SPAWN_TIMEOUT_SECS", "20");

        let mut host = DummyHost::new(Document::default());
        let start = Instant::now();
        let result = PluginProcess::spawn(
            Path::new("no-such-plugin"),
            &mut host,
            Arc::new(|_, _, _| {}),
            None,
        );
        let elapsed = start.elapsed();

        match prev_runner {
            Some(v) => std::env::set_var("OCS_PLUGIN_RUNNER_EXE", v),
            None => std::env::remove_var("OCS_PLUGIN_RUNNER_EXE"),
        }
        match prev_timeout {
            Some(v) => std::env::set_var("OCS_PLUGIN_SPAWN_TIMEOUT_SECS", v),
            None => std::env::remove_var("OCS_PLUGIN_SPAWN_TIMEOUT_SECS"),
        }

        match result {
            Err(PluginError::RunnerCrashed {
                request: "spawn/accept",
                ref status,
            }) => {
                assert!(
                    status.contains("exit code"),
                    "the exit status is reported: {status}"
                );
                assert!(
                    status.contains("Unrecognized option"),
                    "the runner's last stderr line says why: {status}"
                );
            }
            Err(e) => panic!("expected RunnerCrashed during spawn/accept, got {e:?}"),
            Ok(_) => panic!("the spawn must fail"),
        }
        assert!(
            elapsed < Duration::from_secs(5),
            "the host waited {elapsed:?} for a runner that had already exited"
        );
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

        let mut host = DummyHost::new(Document::default());
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
            assert!(
                matches!(resp, HostToPlugin::Response(ref r) if matches!(**r, PluginResponse::Ok))
            );
            send(&mut peer, &PluginToHost::Response(HostResponse::Bool(true)))
                .expect("send final response");
        });

        let mut host = DummyHost::new(Document::default());
        let result = process.dispatch(&mut host, "NESTED", &mut |_| {});
        assert!(result.expect("dispatch succeeds"));
        assert!(process.is_alive(), "process should still be alive");
        let infos = host.take_push_info();
        assert_eq!(
            infos,
            vec!["hello".to_string()],
            "push_info should be delivered to host"
        );

        // Clean up the helper child so it does not outlive the test.
        if let Some(mut runner) = process
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = runner.child.kill();
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
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OCS_PLUGIN_MAX_API_VERSION");
        let result = verify_runner_handshake(
            RunnerHandshake::Token("correct-token".to_string()),
            "correct-token",
        );
        assert!(
            result.is_ok(),
            "expected authentication success, got {result:?}"
        );
        assert!(!result.unwrap(), "V3 token should select V3 path");
    }

    #[test]
    fn runner_handshake_tokenv4_selects_v4_path() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OCS_PLUGIN_MAX_API_VERSION");
        let result = verify_runner_handshake(
            RunnerHandshake::TokenV4 {
                token: "correct-token".to_string(),
                protocol_version: 4,
            },
            "correct-token",
        );
        assert!(
            result.is_ok(),
            "expected authentication success, got {result:?}"
        );
        assert!(result.unwrap(), "TokenV4 should select V4 path");
    }

    #[test]
    fn runner_handshake_tokenv4_mismatched_version_is_rejected() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

    #[test]
    fn null_host_document_does_not_panic() {
        let mut host = NullHost::default();
        let doc = host.document();
        assert!(!doc.layers.is_empty());
        let doc_mut = host.document_mut();
        assert!(!doc_mut.layers.is_empty());
    }
}
