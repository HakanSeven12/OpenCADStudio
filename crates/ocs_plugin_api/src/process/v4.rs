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
use crate::process::PluginError;

/// Default maximum time to wait for a plugin call to respond.
const CALL_TIMEOUT_DEFAULT: Duration = Duration::from_secs(30);

fn call_timeout() -> Duration {
    std::env::var("OCS_PLUGIN_CALL_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(CALL_TIMEOUT_DEFAULT)
}

/// Per-request-kind timeout floors.
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

pub(crate) struct V4Connection {
    shared: Arc<V4HostShared>,
    incoming: Mutex<mpsc::Receiver<HostIncoming>>,
    deferred: Mutex<VecDeque<(u64, Option<u64>, Box<PluginRequest>)>>,
    call_lock: Mutex<()>,
    next_id: AtomicU64,
    reader_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
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
        })
    }

    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
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
        let deadline = Instant::now() + timeout;

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
                Ok(HostIncoming::Response { payload, .. }) => {
                    return Err(PluginError::UnexpectedResponse(Box::new(payload)))
                }
                Ok(HostIncoming::Request {
                    id: rid,
                    tab_id,
                    payload,
                }) => {
                    if tab_id.map_or(true, |request_tab| request_tab == host.tab_id()) {
                        self.respond_to_plugin_request(
                            host,
                            rid,
                            payload,
                            on_start_interactive,
                        )?;
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
                    return Err(PluginError::Runner(
                        "V4 reader thread disconnected".into(),
                    ));
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
                if tab_id.map_or(true, |request_tab| request_tab == current_tab_id) {
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
                    if tab_id.map_or(true, |request_tab| request_tab == current_tab_id) {
                        self.respond_to_plugin_request(
                            host,
                            id,
                            payload,
                            on_start_interactive,
                        )?;
                    } else {
                        self.deferred
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push_back((id, tab_id, payload));
                    }
                }
                Ok(HostIncoming::Response { .. }) => {
                    // Responses without a matching active call should not
                    // happen; drop them to avoid blocking the queue.
                    continue;
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.shared.alive.store(false, Ordering::SeqCst);
                    return Err(PluginError::Runner(
                        "V4 reader thread disconnected".into(),
                    ));
                }
            }
        }
    }

    fn respond_to_plugin_request(
        &self,
        host: &mut dyn HostApi,
        id: u64,
        payload: Box<PluginRequest>,
        on_start_interactive: &mut dyn FnMut(u64),
    ) -> Result<(), PluginError> {
        let response = handle_plugin_request(host, *payload, on_start_interactive);
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
        let _ = self.reader_handle.lock().unwrap_or_else(|e| e.into_inner()).take();
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
    use acadrust::{CadDocument, Handle};
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
    }
    impl DummyHost {
        fn new() -> Self {
            Self {
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
            panic!("not used")
        }
        fn document_mut(&mut self) -> &mut CadDocument {
            panic!("not used")
        }
        fn document_reader(&self) -> Box<dyn DocumentReader + '_> {
            Box::new(EmptyReader)
        }
        fn add_entity(&mut self, _entity: acadrust::EntityType) -> Handle {
            panic!("not used")
        }
        fn bump_geometry(&mut self) {}
        fn read_record(
            &self,
            _handle: Handle,
            _app_name: &str,
        ) -> Option<&acadrust::xdata::ExtendedDataRecord> {
            None
        }
        fn write_record(
            &mut self,
            _handle: Handle,
            _record: acadrust::xdata::ExtendedDataRecord,
        ) -> bool {
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
            panic!("not used")
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
        let _guard = ENV_LOCK.lock().unwrap();
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
        let _guard = ENV_LOCK.lock().unwrap();
        set_test_env();
        let (host_stream, mut runner_stream) = connect_pair();
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(|_, _| {});
        let conn = V4Connection::new(host_stream, handler).unwrap();

        let runner = thread::spawn(move || {
            // Read the host's V4 request and respond.
            let req = recv::<HostToPluginV4>(&mut runner_stream).unwrap();
            match req {
                HostToPluginV4::Request { id, payload: HostRequest::Dispatch { cmd } } => {
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
            .call(&mut host, HostRequest::Dispatch { cmd: "HELLO".to_string() }, &mut |_| {})
            .unwrap();
        assert!(matches!(resp, HostResponse::Bool(true)));
        runner.join().unwrap();
        restore_test_env();
    }

    #[test]
    fn v4_connection_handles_nested_plugin_request() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_test_env();
        let (host_stream, mut runner_stream) = connect_pair();
        let handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync> =
            Arc::new(|_, _| {});
        let conn = V4Connection::new(host_stream, handler).unwrap();

        let runner = thread::spawn(move || {
            let req = recv::<HostToPluginV4>(&mut runner_stream).unwrap();
            match req {
                HostToPluginV4::Request { id, payload: HostRequest::Dispatch { .. } } => {
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
                        HostToPluginV4::Response { id: rid, payload: PluginResponse::Ok }
                            if rid == 99 => {}
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
            .call(&mut host, HostRequest::Dispatch { cmd: "NESTED".to_string() }, &mut |_| {})
            .unwrap();
        assert!(matches!(resp, HostResponse::Bool(true)));
        let infos = host.take_push_info();
        assert_eq!(infos, vec!["nested".to_string()], "push_info should be delivered to host");
        runner.join().unwrap();
        restore_test_env();
    }

    #[test]
    fn v4_connection_notification_invokes_handler() {
        let _guard = ENV_LOCK.lock().unwrap();
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
        let _guard = ENV_LOCK.lock().unwrap();
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
        assert_eq!(got.len(), 1, "second notification should survive handler panic");
        assert_eq!(got[0].0, Some(1));
        runner.join().unwrap();
        drop(conn);
        restore_test_env();
    }

    #[test]
    fn v4_connection_notification_rate_limiter_drops_excess() {
        let _guard = ENV_LOCK.lock().unwrap();
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
            // Keep the stream open long enough for the host to verify the
            // reader thread is still alive after the flood.
            std::thread::sleep(Duration::from_millis(300));
        });

        std::thread::sleep(Duration::from_millis(200));
        let count = *received.lock().unwrap();
        assert!(
            count <= 3,
            "rate limiter should drop most notifications, got {count}"
        );
        assert!(conn.is_alive(), "connection should survive rate-limited flood");
        runner.join().unwrap();
        drop(conn);
        std::env::remove_var("OCS_PLUGIN_NOTIFY_RATE_LIMIT");
        restore_test_env();
    }

    #[test]
    fn execute_code_returns_delayed_result_with_tab_index() {
        let _guard = ENV_LOCK.lock().unwrap();
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
                    assert_eq!(tab_index, 0, "ExecuteCode should be tied to the host's tab index");
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
}
