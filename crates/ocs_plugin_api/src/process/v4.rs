//! Private V4 connection owned by [`PluginProcess`](crate::process::PluginProcess).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use interprocess::local_socket::Stream;
use interprocess::TryClone;

use crate::host::{CommandSource, ExecutionResult, HostApi, HostNotification, PluginNotification};
use crate::ipc::protocol::{HostRequest, HostResponse, PluginRequest};
use crate::ipc::server::handle_plugin_request;
use crate::ipc::transport::send;
use crate::ipc::v4::protocol::{HostToPluginV4, NotificationEnvelope};
use crate::ipc::v4::server::{
    default_notify_rate_limit, run_host_reader_thread, HostIncoming, RateLimiter, V4HostShared,
};
use crate::process::{request_kind, request_timeout, PluginError};

type DeferredRequest = (u64, Option<u64>, Box<PluginRequest>);

pub(crate) struct V4Connection {
    shared: Arc<V4HostShared>,
    incoming: Mutex<mpsc::Receiver<HostIncoming>>,
    deferred: Mutex<VecDeque<DeferredRequest>>,
    call_lock: Mutex<()>,
    next_id: AtomicU64,
    reader_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Identity of the plugin process on this connection. Served requests
    /// attach it (custom-grip declarations key on it); empty until the
    /// manifest is read, where no grip traffic can exist.
    plugin_id: Mutex<String>,
    /// Ids of custom-grip move requests issued without waiting (the coalescing
    /// path, API v11): at most one per drag is ever in this set.
    grip_moves_in_flight: std::sync::Mutex<std::collections::HashSet<u64>>,
    /// Responses that arrived for requests nobody is waiting on (a grip move
    /// answered between polls, or a late frame while another call runs).
    /// Consumed first by the next [`V4Connection::call`], so no frame is lost.
    deferred_responses: Mutex<VecDeque<(u64, HostResponse)>>,
}

impl V4Connection {
    /// Wrap a freshly accepted V4 stream. The stream is cloned so the reader
    /// thread owns one half and the writer half stays on the calling thread.
    #[allow(clippy::result_large_err)]
    pub fn new(
        stream: Stream,
        handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync>,
    ) -> Result<Self, PluginError> {
        let reader = stream.try_clone().map_err(|e| {
            PluginError::Io(std::io::Error::other(format!(
                "V4 stream try_clone failed: {e}"
            )))
        })?;

        let shared = Arc::new(V4HostShared {
            writer: Mutex::new(Some(stream)),
            alive: AtomicBool::new(true),
        });

        let (incoming_tx, incoming_rx) = mpsc::channel();
        let rate_limiter = RateLimiter::new(default_notify_rate_limit());

        let shared_for_thread = Arc::clone(&shared);
        let handle = std::thread::spawn(move || {
            run_host_reader_thread(
                reader,
                shared_for_thread,
                incoming_tx,
                handler,
                rate_limiter,
            )
        });

        Ok(Self {
            shared,
            incoming: Mutex::new(incoming_rx),
            deferred: Mutex::new(VecDeque::new()),
            call_lock: Mutex::new(()),
            next_id: AtomicU64::new(1),
            reader_handle: Mutex::new(Some(handle)),
            plugin_id: Mutex::new(String::new()),
            grip_moves_in_flight: std::sync::Mutex::new(std::collections::HashSet::new()),
            deferred_responses: Mutex::new(VecDeque::new()),
        })
    }

    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
    }

    /// Fix this connection's plugin identity once the manifest is read.
    pub fn attach_plugin_id(&self, id: &str) {
        *self.plugin_id.lock().unwrap_or_else(|e| e.into_inner()) = id.to_string();
    }

    /// Send a host-to-plugin notification.
    #[allow(clippy::result_large_err)]
    pub fn notify_plugin(
        &self,
        command_id: Option<u64>,
        notification: HostNotification,
    ) -> Result<(), PluginError> {
        let envelope = NotificationEnvelope {
            command_id,
            payload: notification,
        };
        let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        let stream = writer.as_mut().ok_or_else(|| {
            PluginError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "V4 connection is shut down",
            ))
        })?;
        send(stream, &HostToPluginV4::Notification(envelope))?;
        Ok(())
    }

    /// Send a V4 request and block on the matching response, handling any
    /// nested plugin requests inline using `host`.
    #[allow(clippy::result_large_err)]
    pub fn call(
        &self,
        host: &mut dyn HostApi,
        req: HostRequest,
        on_start_interactive: &mut dyn FnMut(u64),
    ) -> Result<HostResponse, PluginError> {
        let _call_guard = self.call_lock.lock().unwrap_or_else(|e| e.into_inner());
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let kind = request_kind(&req);
        let timeout = request_timeout(kind);
        let mut deadline = Instant::now() + timeout;

        // A coalesced grip move (API v11) may have been answered between
        // polls; its response waits in `deferred_responses`. Consume ours
        // there first so no frame is ever lost or misattributed.
        {
            let mut deferred_responses = self
                .deferred_responses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(position) = deferred_responses.iter().position(|(rid, _)| *rid == id) {
                let (_, payload) = deferred_responses.remove(position).unwrap();
                return Ok(payload);
            }
        }

        {
            let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
            let stream = writer.as_mut().ok_or_else(|| {
                PluginError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "V4 connection is shut down",
                ))
            })?;
            send(stream, &HostToPluginV4::Request { id, payload: req })?;
        }

        let incoming = self.incoming.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.shared.alive.store(false, Ordering::SeqCst);
                return Err(PluginError::CallTimeout {
                    request: kind,
                    duration: timeout,
                });
            }
            match incoming.recv_timeout(remaining) {
                Ok(HostIncoming::Response { id: rid, payload }) if rid == id => return Ok(payload),
                Ok(HostIncoming::Response { id: rid, payload }) => {
                    if self
                        .grip_moves_in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .contains(&rid)
                    {
                        // The answer to an outstanding coalesced move: park it
                        // for the poll instead of failing this call.
                        self.deferred_responses
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push_back((rid, payload));
                        continue;
                    }
                    return Err(PluginError::UnexpectedResponse(Box::new(payload)));
                }
                Ok(HostIncoming::Request {
                    id: rid,
                    tab_id,
                    payload,
                }) => {
                    // Refresh the call deadline whenever the plugin actively
                    // communicates (including periodic keep-alive heartbeats
                    // while paused in an interactive debugger).
                    deadline = Instant::now() + timeout;
                    if matches!(*payload, PluginRequest::GetTabId)
                        || tab_id.is_none_or(|request_tab| request_tab == host.tab_id() || host.tab_id() == 0)
                    {
                        self.respond_to_plugin_request(host, rid, payload, on_start_interactive)?;
                    } else {
                        self.deferred
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push_back((rid, tab_id, payload));
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.shared.alive.store(false, Ordering::SeqCst);
                    return Err(PluginError::CallTimeout {
                        request: kind,
                        duration: timeout,
                    });
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.shared.alive.store(false, Ordering::SeqCst);
                    return Err(PluginError::Runner("V4 reader thread disconnected".into()));
                }
            }
        }
    }

    /// Drain any plugin→host requests that have arrived since the last call and
    /// apply them using `host`. This lets long-lived plugin sessions (e.g. a
    /// Python REPL) mutate the host document while the host is not otherwise in
    /// a plugin call.
    #[allow(clippy::result_large_err)]
    pub fn drain_requests(
        &self,
        host: &mut dyn HostApi,
        on_start_interactive: &mut dyn FnMut(u64),
    ) -> Result<(), PluginError> {
        let _call_guard = match self.call_lock.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return Ok(()),
        };
        let current_tab_id = host.tab_id();
        let mut ready = VecDeque::new();
        {
            let mut deferred = self.deferred.lock().unwrap_or_else(|e| e.into_inner());
            let mut waiting = VecDeque::new();
            while let Some((id, tab_id, payload)) = deferred.pop_front() {
                if tab_id.is_none_or(|request_tab| request_tab == current_tab_id) {
                    ready.push_back((id, payload));
                } else {
                    waiting.push_back((id, tab_id, payload));
                }
            }
            *deferred = waiting;
        }
        while let Some((id, payload)) = ready.pop_front() {
            self.respond_to_plugin_request(host, id, payload, on_start_interactive)?;
        }

        let incoming = self.incoming.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match incoming.try_recv() {
                Ok(HostIncoming::Request {
                    id,
                    tab_id,
                    payload,
                }) => {
                    if tab_id.is_none_or(|request_tab| request_tab == current_tab_id) {
                        self.respond_to_plugin_request(host, id, payload, on_start_interactive)?;
                    } else {
                        self.deferred
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push_back((id, tab_id, payload));
                    }
                }
                Ok(HostIncoming::Response { id, payload }) => {
                    // The answer to an outstanding coalesced grip move waits
                    // for its poll (API v11); anything else without a matching
                    // active call should not happen — drop it to avoid
                    // blocking the queue.
                    if self
                        .grip_moves_in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .contains(&id)
                    {
                        self.deferred_responses
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push_back((id, payload));
                    }
                    continue;
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.shared.alive.store(false, Ordering::SeqCst);
                    return Err(PluginError::Runner("V4 reader thread disconnected".into()));
                }
            }
        }
    }

    /// Issue a custom-grip drag event WITHOUT waiting for the response (API
    /// v11/v12; move, begin, settle and cancel alike). The app parks move
    /// points and sends at most one move while another is unanswered, so a
    /// slow plugin can never build a laggy queue — and the delivery can
    /// never block the app loop; [`Self::poll_grip`] collects answers
    /// (serving any nested plugin requests on the way) at pump ticks, with
    /// the watchdog ([`Self::abandon`]) for plugins that never answer.
    #[allow(clippy::result_large_err)]
    pub fn send_grip_event(
        &self,
        tab_index: usize,
        event: crate::host::CustomGripEvent,
    ) -> Result<u64, PluginError> {
        let _call_guard = self.call_lock.lock().unwrap_or_else(|e| e.into_inner());
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
            let stream = writer.as_mut().ok_or_else(|| {
                PluginError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "V4 connection is shut down",
                ))
            })?;
            send(
                stream,
                &HostToPluginV4::Request {
                    id,
                    payload: HostRequest::CustomGripEvent { tab_index, event },
                },
            )?;
        }
        self.grip_moves_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id);
        Ok(id)
    }

    /// Tear an untrusted plugin's connection down (API v12 watchdog): a
    /// grip-event delivery that outlives its deadline must be abandoned —
    /// the UI loop recovers, the drag ends as a cancel, an error line names
    /// the plugin. The runner reaps the dead connection; no further frames
    /// are read or written.
    pub fn abandon(&self, reason: &str) {
        self.shared.alive.store(false, Ordering::SeqCst);
        // Dropping the writer half stops NEW frames; `alive=false` marks the
        // connection semantically dead for every is-alive gate (dispatch,
        // grip routing, reaping). The runner is reaped with the process; the
        // reason travels via the host's error queue — the plugin process is
        // not trusted to log anything.
        let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        *writer = None;
        let _ = reason;
    }

    /// Poll for the next outstanding custom-grip answer, serving any nested
    /// plugin requests with `host` on the way (request-in-request over the
    /// same multiplexed link). Nested COMMAND frames are REFUSED in this
    /// context: a grip-event delivery must never inline-serve
    /// `run_command`/`execute_command` — their execution needs the host
    /// message loop the blocked delivery cannot pump (the UI-freeze wedge,
    /// v11.1); the plugin gets an honest `Err` and proceeds data-plane or
    /// replays commands at its own boundary. `Ok(None)` means "not answered
    /// yet"; the poll never marks the connection dead and never blocks the
    /// app for longer than `wait`.
    #[allow(clippy::result_large_err)]
    pub fn poll_grip(
        &self,
        host: &mut dyn HostApi,
        wait: Duration,
    ) -> Result<Option<(u64, HostResponse)>, PluginError> {
        let _call_guard = self.call_lock.lock().unwrap_or_else(|e| e.into_inner());
        let deadline = Instant::now() + wait;
        // Deferred first: the answer may have arrived during a drain tick.
        {
            let in_flight: Vec<u64> = self
                .grip_moves_in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .copied()
                .collect();
            let mut deferred_responses = self
                .deferred_responses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(position) = deferred_responses
                .iter()
                .position(|(rid, _)| in_flight.contains(rid))
            {
                let Some((id, payload)) = deferred_responses.remove(position) else {
                    unreachable!("position came from the same queue");
                };
                return Ok(Some((id, payload)));
            }
        }
        let incoming = self.incoming.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            match incoming.recv_timeout(remaining) {
                Ok(HostIncoming::Response { id: rid, payload }) => {
                    let mine = self
                        .grip_moves_in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&rid);
                    if mine {
                        return Ok(Some((rid, payload)));
                    }
                    // Not ours (a late frame for a future call): park it.
                    self.deferred_responses
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push_back((rid, payload));
                }
                Ok(HostIncoming::Request {
                    id: rid,
                    tab_id,
                    payload,
                }) => {
                    if tab_id.is_none_or(|request_tab| request_tab == host.tab_id()) {
                        self.respond_to_plugin_request_in_context(
                            host,
                            rid,
                            payload,
                            &mut |_| {},
                            true,
                        )?;
                    } else {
                        self.deferred
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push_back((rid, tab_id, payload));
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => return Ok(None),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.shared.alive.store(false, Ordering::SeqCst);
                    return Err(PluginError::Runner("V4 reader thread disconnected".into()));
                }
            }
        }
    }

    /// Whether a custom-grip delivery is still unanswered (used by the app to
    /// keep the settle waiting for the last preview to land, and by the
    /// watchdog to know what to abandon).
    pub fn has_grip_move_in_flight(&self) -> bool {
        !self
            .grip_moves_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    fn respond_to_plugin_request(
        &self,
        host: &mut dyn HostApi,
        id: u64,
        payload: Box<PluginRequest>,
        on_start_interactive: &mut dyn FnMut(u64),
    ) -> Result<(), PluginError> {
        // Standard context: nested commands are allowed (the caller's own
        // serve loop runs on a thread whose message loop keeps pumping —
        // the dispatch path, drain ticks, and normal calls).
        self.respond_to_plugin_request_in_context(host, id, payload, on_start_interactive, false)
    }

    /// Serve one nested plugin request, optionally in a GRIP-EVENT context
    /// (API v12): command frames are refused there — their nested execution
    /// needs the iced message loop, which a UI-thread grip delivery cannot
    /// pump (the v11.1 UI-freeze wedge). The plugin gets an honest `Err`,
    /// so its settle can proceed data-plane or report the boundary.
    fn respond_to_plugin_request_in_context(
        &self,
        host: &mut dyn HostApi,
        id: u64,
        payload: Box<PluginRequest>,
        on_start_interactive: &mut dyn FnMut(u64),
        grip_event: bool,
    ) -> Result<(), PluginError> {
        let plugin_id = self
            .plugin_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let response = handle_plugin_request(
            host,
            &plugin_id,
            *payload,
            on_start_interactive,
            !grip_event,
        );
        let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        let stream = writer.as_mut().ok_or_else(|| {
            PluginError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "V4 connection is shut down",
            ))
        })?;
        send(
            stream,
            &HostToPluginV4::Response {
                id,
                payload: response,
            },
        )?;
        Ok(())
    }

    /// Send an `ExecuteCode` request and block until the plugin returns the
    /// result. The active document tab index is included so the REPL session is
    /// tied to the tab that issued it.
    #[allow(clippy::result_large_err)]
    pub fn execute_code(
        &self,
        host: &mut dyn HostApi,
        command_id: u64,
        source: CommandSource,
        code: &str,
    ) -> Result<ExecutionResult, PluginError> {
        let req = HostRequest::ExecuteCode {
            command_id,
            source,
            code: code.to_string(),
            tab_index: host.tab_index(),
        };
        match self.call(host, req, &mut |_| {})? {
            HostResponse::CodeExecutionResult(result) => Ok(result),
            other => Err(PluginError::UnexpectedResponse(Box::new(other))),
        }
    }

    /// Shut down the V4 connection. The writer half is dropped, which causes
    /// the reader thread to exit, then the thread handle is joined.
    pub fn shutdown(&self) {
        {
            let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
            // Ask the runner to shut down gracefully so the plugin has a chance
            // to clean up spawned child processes (e.g. the Python REPL). The
            // runner will send an Ack, then observe EOF on the next loop
            // iteration and exit.
            if let Some(stream) = writer.as_mut() {
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                let _ = send(
                    stream,
                    &HostToPluginV4::Request {
                        id,
                        payload: HostRequest::Shutdown,
                    },
                );
            }
            let _ = writer.take();
        }
        // Dropping the writer closes the socket. The reader thread will then
        // exit and clear `alive`. Joining can deadlock if a plugin child
        // inherited the socket handle, so we only poll briefly. The host process
        // will kill the runner afterwards, so a long wait only delays shutdown
        // without improving cleanup.
        let deadline = Instant::now() + Duration::from_millis(100);
        while self.shared.alive.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self
            .reader_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::thread;

    use crate::host::{CommandSource, DocumentReader, ExecutionResult, ReaderEntity};
    use crate::ipc::protocol::{HostResponse, PluginRequest, PluginResponse};
    use crate::ipc::transport::recv;
    use crate::ipc::v4::protocol::{HostToPluginV4, PluginToHostV4};
    use codec::{Document, Handle};
    use interprocess::local_socket::{
        traits::{Listener, Stream as StreamTrait},
        GenericNamespaced, ListenerOptions, ToNsName,
    };

    fn unique_socket_name() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("ocs_plugin_v4_proc_test_{}_{}", std::process::id(), n)
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
        push_info_messages: StdMutex<Vec<String>>,
        transactions: StdMutex<Vec<(String, Vec<codec::EntityType>)>>,
    }
    impl DummyHost {
        fn new() -> Self {
            Self {
                push_info_messages: StdMutex::new(Vec::new()),
                transactions: StdMutex::new(Vec::new()),
            }
        }
        fn take_push_info(&self) -> Vec<String> {
            std::mem::take(&mut *self.push_info_messages.lock().unwrap())
        }
        fn take_transactions(&self) -> Vec<(String, Vec<codec::EntityType>)> {
            std::mem::take(&mut *self.transactions.lock().unwrap())
        }
    }
    impl HostApi for DummyHost {
        fn tab_index(&self) -> usize {
            0
        }
        fn document(&self) -> &Document {
            panic!("not used")
        }
        fn document_mut(&mut self) -> &mut Document {
            panic!("not used")
        }
        fn document_reader(&self) -> Box<dyn DocumentReader + '_> {
            Box::new(EmptyReader)
        }
        fn add_entity(&mut self, _entity: codec::EntityType) -> Handle {
            panic!("not used")
        }
        fn bump_geometry(&mut self) {}
        fn read_record(
            &self,
            _handle: Handle,
            _app_name: &str,
        ) -> Option<&codec::xdata::ExtendedDataRecord> {
            None
        }
        fn write_record(
            &mut self,
            _handle: Handle,
            _record: codec::xdata::ExtendedDataRecord,
        ) -> bool {
            false
        }
        fn remove_record(&mut self, _handle: Handle, _app_name: &str) -> bool {
            false
        }
        fn push_undo(&mut self, _label: &str) {}
        fn update_entities_transaction(
            &mut self,
            label: &str,
            entities: Vec<codec::EntityType>,
        ) -> Result<(), String> {
            self.transactions
                .lock()
                .unwrap()
                .push((label.to_owned(), entities));
            Ok(())
        }
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

    use std::sync::Mutex as StdMutex;

    use crate::test_lock::ENV_LOCK;

    fn set_test_env() {
        std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", "2");
        std::env::set_var("OCS_PLUGIN_TEST_FLOOR_SECS", "0");
    }

    fn restore_test_env() {
        std::env::remove_var("OCS_PLUGIN_CALL_TIMEOUT_SECS");
        std::env::remove_var("OCS_PLUGIN_TEST_FLOOR_SECS");
    }

    #[test]
    fn execute_code_timeout_floor_is_at_least_60s_and_env_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_call_timeout = std::env::var("OCS_PLUGIN_CALL_TIMEOUT_SECS").ok();
        let prev_test_floor = std::env::var("OCS_PLUGIN_TEST_FLOOR_SECS").ok();
        let prev_execute_timeout = std::env::var("OCS_PLUGIN_EXECUTE_TIMEOUT_SECS").ok();
        // Remove the test floor bypass and the call-timeout override so the
        // ExecuteCode-specific floor (60 s) is actually exercised.
        std::env::remove_var("OCS_PLUGIN_CALL_TIMEOUT_SECS");
        std::env::remove_var("OCS_PLUGIN_TEST_FLOOR_SECS");
        std::env::remove_var("OCS_PLUGIN_EXECUTE_TIMEOUT_SECS");
        assert!(
            request_timeout("ExecuteCode") >= Duration::from_secs(60),
            "ExecuteCode floor should be at least 60 s"
        );

        std::env::set_var("OCS_PLUGIN_EXECUTE_TIMEOUT_SECS", "120");
        assert_eq!(
            request_timeout("ExecuteCode"),
            Duration::from_secs(120),
            "env override should be respected"
        );

        match prev_call_timeout {
            Some(v) => std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", v),
            None => std::env::remove_var("OCS_PLUGIN_CALL_TIMEOUT_SECS"),
        }
        match prev_test_floor {
            Some(v) => std::env::set_var("OCS_PLUGIN_TEST_FLOOR_SECS", v),
            None => std::env::remove_var("OCS_PLUGIN_TEST_FLOOR_SECS"),
        }
        match prev_execute_timeout {
            Some(v) => std::env::set_var("OCS_PLUGIN_EXECUTE_TIMEOUT_SECS", v),
            None => std::env::remove_var("OCS_PLUGIN_EXECUTE_TIMEOUT_SECS"),
        }
    }

    #[test]
    fn v4_connection_request_response() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_env();
        let (host_stream, mut runner_stream) = connect_pair();
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(|_, _| {});
        let conn = V4Connection::new(host_stream, handler).unwrap();

        let runner = thread::spawn(move || {
            // Read the host's V4 request and respond.
            let req = recv::<HostToPluginV4>(&mut runner_stream).unwrap();
            match req {
                HostToPluginV4::Request {
                    id,
                    payload: HostRequest::Dispatch { cmd },
                } => {
                    assert_eq!(cmd, "HELLO");
                    send(
                        &mut runner_stream,
                        &PluginToHostV4::Response {
                            id,
                            payload: HostResponse::Bool(true),
                        },
                    )
                    .unwrap();
                }
                other => panic!("unexpected: {other:?}"),
            }
        });

        let mut host = DummyHost::new();
        let resp = conn
            .call(
                &mut host,
                HostRequest::Dispatch {
                    cmd: "HELLO".to_string(),
                },
                &mut |_| {},
            )
            .unwrap();
        assert!(matches!(resp, HostResponse::Bool(true)));
        runner.join().unwrap();
        restore_test_env();
    }

    #[test]
    fn v4_connection_handles_nested_plugin_request() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_env();
        let (host_stream, mut runner_stream) = connect_pair();
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(|_, _| {});
        let conn = V4Connection::new(host_stream, handler).unwrap();

        let runner = thread::spawn(move || {
            let req = recv::<HostToPluginV4>(&mut runner_stream).unwrap();
            match req {
                HostToPluginV4::Request {
                    id,
                    payload: HostRequest::Dispatch { .. },
                } => {
                    // Send a nested plugin request.
                    send(
                        &mut runner_stream,
                        &PluginToHostV4::Request {
                            id: 99,
                            tab_id: None,
                            payload: PluginRequest::PushInfo("nested".to_string()),
                        },
                    )
                    .unwrap();
                    // Read the response to the nested request.
                    let resp = recv::<HostToPluginV4>(&mut runner_stream).unwrap();
                    match resp {
                        HostToPluginV4::Response {
                            id: rid,
                            payload: PluginResponse::Ok,
                        } if rid == 99 => {}
                        other => panic!("unexpected nested response: {other:?}"),
                    }
                    // Now respond to the original dispatch.
                    send(
                        &mut runner_stream,
                        &PluginToHostV4::Response {
                            id,
                            payload: HostResponse::Bool(true),
                        },
                    )
                    .unwrap();
                }
                other => panic!("unexpected: {other:?}"),
            }
        });

        let mut host = DummyHost::new();
        let resp = conn
            .call(
                &mut host,
                HostRequest::Dispatch {
                    cmd: "NESTED".to_string(),
                },
                &mut |_| {},
            )
            .unwrap();
        assert!(matches!(resp, HostResponse::Bool(true)));
        let infos = host.take_push_info();
        assert_eq!(
            infos,
            vec!["nested".to_string()],
            "push_info should be delivered to host"
        );
        runner.join().unwrap();
        restore_test_env();
    }

    #[test]
    fn v4_connection_routes_entity_transaction_over_nested_ipc() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_env();
        let (host_stream, mut runner_stream) = connect_pair();
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(|_, _| {});
        let conn = V4Connection::new(host_stream, handler).unwrap();
        let mut point = codec::entities::Point::new();
        point.common.handle = Handle::new(42);
        point.location.x = 7.0;
        let expected = codec::EntityType::Point(point);
        let sent = expected.clone();

        let runner = thread::spawn(move || {
            let req = recv::<HostToPluginV4>(&mut runner_stream).unwrap();
            let HostToPluginV4::Request {
                id,
                payload: HostRequest::Dispatch { .. },
            } = req
            else {
                panic!("unexpected host request: {req:?}");
            };
            send(
                &mut runner_stream,
                &PluginToHostV4::Request {
                    id: 99,
                    tab_id: Some(0),
                    payload: PluginRequest::UpdateEntitiesTransaction {
                        label: "Move point".into(),
                        entities: vec![sent],
                    },
                },
            )
            .unwrap();
            assert!(matches!(
                recv::<HostToPluginV4>(&mut runner_stream).unwrap(),
                HostToPluginV4::Response {
                    id: 99,
                    payload: PluginResponse::EntityTransactionResult(Ok(()))
                }
            ));
            send(
                &mut runner_stream,
                &PluginToHostV4::Response {
                    id,
                    payload: HostResponse::Bool(true),
                },
            )
            .unwrap();
        });

        let mut host = DummyHost::new();
        assert!(matches!(
            conn.call(
                &mut host,
                HostRequest::Dispatch { cmd: "MOVE".into() },
                &mut |_| {}
            )
            .unwrap(),
            HostResponse::Bool(true)
        ));
        assert_eq!(
            host.take_transactions(),
            vec![("Move point".into(), vec![expected])]
        );
        runner.join().unwrap();
        restore_test_env();
    }

    #[test]
    fn v4_connection_notification_invokes_handler() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_env();
        use crate::host::LogLevel;

        let (host_stream, mut runner_stream) = connect_pair();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received2 = Arc::clone(&received);
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(move |cmd_id, notif| {
                received2.lock().unwrap().push((cmd_id, notif));
            });
        let conn = V4Connection::new(host_stream, handler).unwrap();

        let runner = thread::spawn(move || {
            send(
                &mut runner_stream,
                &PluginToHostV4::Notification(NotificationEnvelope {
                    command_id: Some(42),
                    payload: PluginNotification::Log {
                        level: LogLevel::Info,
                        text: "hello".to_string(),
                    },
                }),
            )
            .unwrap();
        });

        // Wait a bit for the notification to be delivered.
        std::thread::sleep(Duration::from_millis(100));
        let got = received.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, Some(42));
        runner.join().unwrap();
        drop(conn);
        restore_test_env();
    }

    #[test]
    fn v4_connection_handler_panic_is_isolated() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_env();
        use crate::host::LogLevel;

        let (host_stream, mut runner_stream) = connect_pair();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received2 = Arc::clone(&received);
        let panic_next = AtomicBool::new(true);
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(move |cmd_id, notif| {
                if panic_next.swap(false, Ordering::SeqCst) {
                    panic!("deliberate handler panic");
                }
                received2.lock().unwrap().push((cmd_id, notif));
            });
        let conn = V4Connection::new(host_stream, handler).unwrap();

        let runner = thread::spawn(move || {
            for i in 0..2 {
                send(
                    &mut runner_stream,
                    &PluginToHostV4::Notification(NotificationEnvelope {
                        command_id: Some(i),
                        payload: PluginNotification::Log {
                            level: LogLevel::Info,
                            text: "hi".to_string(),
                        },
                    }),
                )
                .unwrap();
            }
        });

        std::thread::sleep(Duration::from_millis(200));
        let got = received.lock().unwrap().clone();
        assert_eq!(
            got.len(),
            1,
            "second notification should survive handler panic"
        );
        assert_eq!(got[0].0, Some(1));
        runner.join().unwrap();
        drop(conn);
        restore_test_env();
    }

    #[test]
    fn v4_connection_notification_rate_limiter_drops_excess() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_env();
        std::env::set_var("OCS_PLUGIN_NOTIFY_RATE_LIMIT", "2");
        use crate::host::LogLevel;

        let (host_stream, mut runner_stream) = connect_pair();
        let received = Arc::new(Mutex::new(0u32));
        let received2 = Arc::clone(&received);
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(move |_, _| {
                *received2.lock().unwrap() += 1;
            });
        let conn = V4Connection::new(host_stream, handler).unwrap();
        let (checked_tx, checked_rx) = mpsc::channel::<()>();

        let runner = thread::spawn(move || {
            for _ in 0..10 {
                send(
                    &mut runner_stream,
                    &PluginToHostV4::Notification(NotificationEnvelope {
                        command_id: None,
                        payload: PluginNotification::Log {
                            level: LogLevel::Info,
                            text: "flood".to_string(),
                        },
                    }),
                )
                .unwrap();
            }
            // Keep the stream open until the host has checked the reader
            // thread survived the flood; a fixed sleep raced on slow runners.
            let _ = checked_rx.recv_timeout(Duration::from_secs(30));
        });

        std::thread::sleep(Duration::from_millis(200));
        let count = *received.lock().unwrap();
        assert!(
            count <= 3,
            "rate limiter should drop most notifications, got {count}"
        );
        assert!(
            conn.is_alive(),
            "connection should survive rate-limited flood"
        );
        checked_tx.send(()).unwrap();
        runner.join().unwrap();
        drop(conn);
        std::env::remove_var("OCS_PLUGIN_NOTIFY_RATE_LIMIT");
        restore_test_env();
    }

    #[test]
    fn execute_code_returns_delayed_result_with_tab_index() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_env();

        let (host_stream, mut runner_stream) = connect_pair();
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(|_, _| {});
        let conn = V4Connection::new(host_stream, handler).unwrap();

        let runner = thread::spawn(move || {
            let req = recv::<HostToPluginV4>(&mut runner_stream).unwrap();
            match req {
                HostToPluginV4::Request {
                    id,
                    payload:
                        HostRequest::ExecuteCode {
                            command_id,
                            source,
                            code,
                            tab_index,
                        },
                } => {
                    assert_eq!(command_id, 1);
                    assert_eq!(source, CommandSource::Editor);
                    assert_eq!(code, "1+1");
                    assert_eq!(
                        tab_index, 0,
                        "ExecuteCode should be tied to the host's tab index"
                    );
                    thread::sleep(Duration::from_millis(100));
                    send(
                        &mut runner_stream,
                        &PluginToHostV4::Response {
                            id,
                            payload: HostResponse::CodeExecutionResult(ExecutionResult {
                                success: true,
                                output: Some("42".to_string()),
                                error: None,
                                error_type: None,
                                traceback: None,
                                line_number: None,
                                column_number: None,
                                duration_ms: 100.0,
                            }),
                        },
                    )
                    .unwrap();
                }
                other => panic!("unexpected: {other:?}"),
            }
        });

        let mut host = DummyHost::new();
        let result = conn
            .execute_code(&mut host, 1, CommandSource::Editor, "1+1")
            .expect("execute_code should succeed");
        assert!(result.success);
        assert_eq!(result.output, Some("42".to_string()));
        runner.join().unwrap();
        restore_test_env();
    }

    /// Records the calls of a fake host so assertions can check the ORDER
    /// of a settle's nested writes.
    #[derive(Default)]
    struct RecordingHost {
        calls: StdMutex<Vec<&'static str>>,
    }
    impl HostApi for RecordingHost {
        fn tab_index(&self) -> usize {
            0
        }
        fn document(&self) -> &Document {
            panic!("not used")
        }
        fn document_mut(&mut self) -> &mut Document {
            panic!("not used")
        }
        fn document_reader(&self) -> Box<dyn DocumentReader + '_> {
            Box::new(EmptyReader)
        }
        fn add_entity(&mut self, _entity: codec::EntityType) -> Handle {
            self.calls.lock().unwrap().push("add_entity");
            Handle::from(1u64)
        }
        fn bump_geometry(&mut self) {}
        fn read_record(
            &self,
            _handle: Handle,
            _app_name: &str,
        ) -> Option<&codec::xdata::ExtendedDataRecord> {
            None
        }
        fn write_record(
            &mut self,
            _handle: Handle,
            _record: codec::xdata::ExtendedDataRecord,
        ) -> bool {
            false
        }
        fn remove_record(&mut self, _handle: Handle, _app_name: &str) -> bool {
            false
        }
        fn push_undo(&mut self, _label: &str) {
            self.calls.lock().unwrap().push("push_undo");
        }
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

    #[test]
    fn custom_grip_settle_writes_through_nested_requests() {
        // The out-of-process runner path (API v11/v12): the settle event is
        // issued WITHOUT waiting (the v11.1 hardening — the UI loop never
        // blocks); the plugin's write requests are served INLINE by the
        // bounded poll on UI ticks (request-in-request over the multiplexed
        // link), and the settle's answer concludes the exchange in order.
        let (runner_end, host_end) = connect_pair();
        let runner = thread::spawn(move || {
            let mut stream = runner_end;
            let frame = recv::<HostToPluginV4>(&mut stream).expect("settle frame");
            let HostToPluginV4::Request {
                id,
                payload: HostRequest::CustomGripEvent { tab_index, event },
            } = frame
            else {
                panic!("expected a CustomGripEvent request, got {frame:?}")
            };
            assert_eq!(tab_index, 0);
            assert_eq!(event.phase, crate::host::CustomGripPhase::Settle);
            // The plugin opens its one undo scope, then writes — exactly the
            // consumer's settle shape — and each nested request is answered
            // by the waiting host before the settle resolves.
            for nested in [
                PluginRequest::PushUndo {
                    label: "CVI: PI radius".to_string(),
                },
                PluginRequest::AddEntity(codec::EntityType::Line(
                    codec::entities::Line::from_points(
                        codec::types::Vector3::new(0.0, 0.0, 0.0),
                        codec::types::Vector3::new(3.0, 0.0, 0.0),
                    ),
                )),
            ] {
                send(
                    &mut stream,
                    &PluginToHostV4::Request {
                        id: 5000 + id,
                        tab_id: Some(0),
                        payload: nested,
                    },
                )
                .expect("nested send");
                match recv::<HostToPluginV4>(&mut stream).expect("nested answer") {
                    HostToPluginV4::Response { .. } => {}
                    other => panic!("nested request answered inline, got {other:?}"),
                }
            }
            // A nested COMMAND frame is refused in this context (the v11.1
            // wedge): the plugin gets the honest Error, not a hang.
            send(
                &mut stream,
                &PluginToHostV4::Request {
                    id: 6000 + id,
                    tab_id: Some(0),
                    payload: PluginRequest::ExecuteCommand("PLUGIN_REPLAY".to_string()),
                },
            )
            .expect("nested command send");
            match recv::<HostToPluginV4>(&mut stream).expect("command answer") {
                HostToPluginV4::Response {
                    id: _,
                    payload: PluginResponse::Error(message),
                } => {
                    assert!(
                        message.contains("refused during a custom-grip drag"),
                        "the refusal must name the constraint, got: {message}"
                    );
                }
                other => panic!("nested command must refuse, got {other:?}"),
            }
            send(
                &mut stream,
                &PluginToHostV4::Response {
                    id,
                    payload: HostResponse::CustomGrip(crate::host::CustomGripResponse {
                        preview: Vec::new(),
                        live_entities: Vec::new(),
                        descriptor: None,
                        descriptor_tolerance: None,
                    }),
                },
            )
            .expect("settle answer");
        });
        let conn = V4Connection::new(host_end, Arc::new(|_, _| {})).expect("connection");
        conn.attach_plugin_id("test.grip-points");
        let mut host = RecordingHost::default();
        let settle_id = conn
            .send_grip_event(
                0,
                crate::host::CustomGripEvent {
                    grip_id: 42,
                    phase: crate::host::CustomGripPhase::Settle,
                    point: [1.0, 2.0, 3.0],
                    anchor: None,
                },
            )
            .expect("the settle is issued without waiting");
        // Bounded polls until the settle lands — the UI-shape loop.
        let landed = loop {
            match conn.poll_grip(&mut host, Duration::from_millis(200)) {
                Ok(Some((id, response))) => break (id, response),
                Ok(None) => continue,
                Err(error) => panic!("poll failed: {error}"),
            }
        };
        assert_eq!(landed.0, settle_id);
        runner.join().expect("runner thread finished");
        match landed.1 {
            HostResponse::CustomGrip(resp) => {
                assert!(resp.preview.is_empty());
                assert!(resp.live_entities.is_empty());
            }
            other => panic!("unexpected settle answer: {other:?}"),
        }
        let calls = host.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["push_undo", "add_entity"],
            "the settle's nested writes are served inline in call order; nested \
             commands are refused, not executed"
        );
    }

    #[test]
    fn custom_grip_abandon_never_freezes_the_host() {
        // The v11.1 watchdog shape: a plugin that never answers its settle
        // costs the host nothing — the polls are bounded, the delivery is
        // abandoned, the connection is torn down (is_alive false), and a
        // follow-up call fails fast instead of hanging.
        let (runner_end, host_end) = connect_pair();
        // The runner receives the settle and answers NOTHING: it just holds
        // the stream (a wedged or hostile plugin).
        let runner = thread::spawn(move || {
            let mut stream = runner_end;
            let frame = recv::<HostToPluginV4>(&mut stream).expect("settle frame");
            let HostToPluginV4::Request { id, .. } = frame else {
                panic!("unexpected frame: {frame:?}")
            };
            // Block with the id held: keep the stream open, never respond.
            std::thread::sleep(Duration::from_secs(1));
            let _ = id;
        });
        let conn = V4Connection::new(host_end, Arc::new(|_, _| {})).expect("connection");
        conn.attach_plugin_id("test.grip-points");
        let mut host = RecordingHost::default();
        let start = Instant::now();
        conn.send_grip_event(
            0,
            crate::host::CustomGripEvent {
                grip_id: 1,
                phase: crate::host::CustomGripPhase::Settle,
                point: [0.0; 3],
                anchor: None,
            },
        )
        .expect("issued without waiting");
        // A few bounded polls: each returns None quickly, never a block.
        for _ in 0..3 {
            let poll_start = Instant::now();
            let landed = conn
                .poll_grip(&mut host, Duration::from_millis(50))
                .expect("poll never errors on silence");
            assert!(landed.is_none());
            // Well under the runner's one-second hold: a slow CI machine
            // may overshoot 50 ms, a blocking poll would not return at all.
            assert!(
                poll_start.elapsed() < Duration::from_millis(900),
                "the bounded poll stays bounded"
            );
        }
        // The watchdog: abandon the delivery and tear the connection down.
        conn.abandon("settle deadline exceeded");
        assert!(!conn.is_alive(), "the connection is torn down");
        // Follow-up deliveries fail fast (no writer, no hang).
        let failed = conn.send_grip_event(
            0,
            crate::host::CustomGripEvent {
                grip_id: 1,
                phase: crate::host::CustomGripPhase::Cancel,
                point: [0.0; 3],
                anchor: None,
            },
        );
        assert!(failed.is_err());
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the whole exchange stays cheap — the wall-clock timeout never runs"
        );
        runner.join().expect("runner thread finished");
    }

    #[test]
    fn plugin_requests_refresh_call_deadline_during_debug_pause() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_env();
        std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", "1");

        let (runner_end, host_end) = connect_pair();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let runner = thread::spawn(move || {
            let mut stream = runner_end;
            let frame = recv::<HostToPluginV4>(&mut stream).expect("dispatch frame");
            let HostToPluginV4::Request { id, .. } = frame else {
                panic!("unexpected frame: {frame:?}")
            };
            // Simulate a plugin paused in a debugger for longer than the 1s
            // call timeout window by sending periodic keep-alive GetTabId
            // requests every 350ms (total ~1.75s > 1s).
            for req_idx in 0..5u64 {
                thread::sleep(Duration::from_millis(350));
                send(
                    &mut stream,
                    &PluginToHostV4::Request {
                        id: 9000 + req_idx,
                        tab_id: Some(0),
                        payload: PluginRequest::GetTabId,
                    },
                )
                .expect("keepalive send");
                match recv::<HostToPluginV4>(&mut stream).expect("keepalive reply") {
                    HostToPluginV4::Response {
                        payload: PluginResponse::TabId(_),
                        ..
                    } => {}
                    other => panic!("unexpected keepalive reply: {other:?}"),
                }
            }
            send(
                &mut stream,
                &PluginToHostV4::Response {
                    id,
                    payload: HostResponse::Bool(true),
                },
            )
            .expect("dispatch response");
            // Hold the stream open until the host thread has verified the
            // connection survived the debug pause.
            let _ = done_rx.recv_timeout(Duration::from_secs(10));
        });

        let conn = V4Connection::new(host_end, Arc::new(|_, _| {})).expect("connection");
        let mut host = RecordingHost::default();
        let resp = conn
            .call(
                &mut host,
                HostRequest::Dispatch {
                    cmd: "DEBUG_CMD".to_string(),
                },
                &mut |_| {},
            )
            .expect("periodic plugin requests should refresh deadline and prevent timeout");
        assert!(matches!(resp, HostResponse::Bool(true)));
        assert!(conn.is_alive());
        let _ = done_tx.send(());
        runner.join().expect("runner thread finished");
        std::env::remove_var("OCS_PLUGIN_CALL_TIMEOUT_SECS");
        restore_test_env();
    }
}
