//! Plugin-side V4 IPC client and `HostApi` proxy.

use std::any::Any;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use acadrust::xdata::ExtendedDataRecord;
use acadrust::{CadDocument, EntityType, Handle};
use interprocess::local_socket::traits::Stream as StreamTrait;
use interprocess::local_socket::{GenericNamespaced, Stream, ToNsName};
use interprocess::TryClone;

use crate::host::{
    DocumentReader, ExecutionResult, HostApi, HostNotification, InteractiveCommand,
    PluginNotification, PluginRequestSender, PluginRequestError, ReaderEntity,
};
use crate::ipc::protocol::{
    HostRequest, HostResponse, PluginRequest, PluginResponse, RunnerHandshake,
};
use crate::ipc::transport::{recv, send, TransportError};
use crate::ipc::v4::protocol::{
    HostToPluginV4, NotificationEnvelope, PluginToHostV4, V4_PROTOCOL_VERSION,
};
use crate::shm::{DocumentViewInfo, SharedDocumentReader, DocumentViewData};

/// Default capacity of the bounded host→plugin notification queue.
const DEFAULT_NOTIFY_QUEUE_SIZE: usize = 1024;

/// Minimum interval between verbose drop logs to avoid spam.
const DROP_LOG_INTERVAL: Duration = Duration::from_secs(1);

/// Bounded queue of host→plugin notifications delivered to the plugin runner
/// loop and/or `HostApi::try_recv_notification`.
type NotificationQueue = mpsc::Receiver<(Option<u64>, HostNotification)>;

/// Shared state between the plugin main thread, the reader thread, and any
/// cloned `V4PluginHostApi` proxies.
struct Shared {
    writer: Mutex<Stream>,
    next_id: AtomicU64,
    in_flight: Mutex<HashMap<u64, mpsc::Sender<PluginResponse>>>,
}

/// A frame the V4 reader thread delivers to the plugin runner loop.
pub enum RunnerFrame {
    Request { id: u64, payload: HostRequest },
}

/// Plugin-side V4 connection to the host.
pub struct V4Client {
    shared: Arc<Shared>,
    notifications: Arc<Mutex<NotificationQueue>>,
    runner_queue: mpsc::Receiver<RunnerFrame>,
}

impl V4Client {
    /// Connect to the host socket, send the V4 handshake, and start the reader
    /// thread. The writer half is kept for synchronous `HostApi` calls; the
    /// reader half is moved into a background thread.
    pub fn connect_handshake(
        name: &str,
        token: &str,
    ) -> Result<Self, crate::ipc::transport::TransportError> {
        let name = name
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let mut stream = StreamTrait::connect(name)?;

        // Send the V4 handshake on the unsplit stream before spawning the
        // reader thread.
        send(
            &mut stream,
            &RunnerHandshake::TokenV4 {
                token: token.to_string(),
                protocol_version: V4_PROTOCOL_VERSION,
            },
        )?;

        // Split the stream into read/write halves so the reader can block on
        // `recv` without stalling synchronous sends from the main thread.
        let reader = stream.try_clone().map_err(|e| {
            crate::ipc::transport::TransportError::Io(std::io::Error::other(format!(
                "stream try_clone failed: {e}"
            )))
        })?;

        let (notify_tx, notify_rx) =
            mpsc::sync_channel(notify_queue_size());
        let (runner_tx, runner_rx) = mpsc::channel();

        let shared = Arc::new(Shared {
            writer: Mutex::new(stream),
            next_id: AtomicU64::new(1),
            in_flight: Mutex::new(HashMap::new()),
        });

        let shared_for_reader = Arc::clone(&shared);
        let notifications = Arc::new(Mutex::new(notify_rx));
        std::thread::spawn(move || {
            reader_thread(
                reader,
                shared_for_reader,
                notify_tx,
                runner_tx,
            )
        });

        Ok(Self {
            shared,
            notifications,
            runner_queue: runner_rx,
        })
    }

    /// Block until the next host request arrives for the runner loop.
    pub fn recv_runner_frame(&self) -> Result<RunnerFrame, mpsc::RecvError> {
        self.runner_queue.recv()
    }

    /// Wait up to `timeout` for a host request so the runner can also drain
    /// notifications while otherwise idle.
    pub fn recv_runner_frame_timeout(
        &self,
        timeout: Duration,
    ) -> Result<RunnerFrame, mpsc::RecvTimeoutError> {
        self.runner_queue.recv_timeout(timeout)
    }

    /// Non-blocking poll for host-to-plugin notifications.
    pub fn try_recv_notification(&self) -> Option<(Option<u64>, HostNotification)> {
        self.notifications.lock().unwrap_or_else(|e| e.into_inner()).try_recv().ok()
    }

    /// Send a plugin response for a host request.
    pub fn send_response(
        &self,
        id: u64,
        resp: HostResponse,
    ) -> Result<(), crate::ipc::transport::TransportError> {
        let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        send(
            &mut writer,
            &PluginToHostV4::Response { id, payload: resp },
        )
    }

    /// Send a plugin-to-host notification.
    pub fn notify_plugin(
        &self,
        command_id: Option<u64>,
        notification: PluginNotification,
    ) {
        let envelope = NotificationEnvelope {
            command_id,
            payload: notification,
        };
        let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = send(&mut writer, &PluginToHostV4::Notification(envelope)) {
            eprintln!("[plugin] notify_plugin failed: {e}");
        }
    }

    /// Build a callback that sends a delayed `CodeExecutionResult` response for
    /// the given request id. Used by `start_execute_code` implementations that
    /// run REPL code on a background thread.
    pub fn execute_code_responder(
        &self,
        id: u64,
    ) -> Box<dyn FnOnce(ExecutionResult) + Send> {
        let shared = Arc::clone(&self.shared);
        Box::new(move |result| {
            let mut writer = shared.writer.lock().unwrap_or_else(|e| e.into_inner());
            let _ = send(
                &mut writer,
                &PluginToHostV4::Response {
                    id,
                    payload: HostResponse::CodeExecutionResult(result),
                },
            );
        })
    }

    /// Create a `HostApi` proxy backed by this V4 connection.
    pub fn plugin_host_api(
        &self,
        tab_index: usize,
        interactive: Rc<RefCell<HashMap<u64, Box<dyn InteractiveCommand>>>>,
    ) -> V4PluginHostApi {
        V4PluginHostApi::new(
            Arc::clone(&self.shared),
            Arc::clone(&self.notifications),
            tab_index,
            interactive,
        )
    }
}

fn notify_queue_size() -> usize {
    std::env::var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_NOTIFY_QUEUE_SIZE)
        .max(1)
}

/// Background reader thread: deserialize frames and route them to the
/// appropriate sink. Panics are caught so one bad frame does not escape the
/// thread; the error is logged and the thread exits, which closes the shared
/// channels and signals process death to waiters.
fn reader_thread(
    mut reader: Stream,
    shared: Arc<Shared>,
    notify_tx: mpsc::SyncSender<(Option<u64>, HostNotification)>,
    runner_tx: mpsc::Sender<RunnerFrame>,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut last_drop_log: Option<Instant> = None;
        loop {
            match recv::<HostToPluginV4>(&mut reader) {
                Ok(HostToPluginV4::Response { id, payload }) => {
                    let tx = shared
                        .in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&id);
                    if let Some(tx) = tx {
                        let _ = tx.send(payload);
                    } else {
                        eprintln!("[plugin] V4 reader: unexpected response id={id}");
                    }
                }
                Ok(HostToPluginV4::Notification(envelope)) => {
                    if notify_tx.try_send((envelope.command_id, envelope.payload)).is_err() {
                        let now = Instant::now();
                        let should_log = last_drop_log
                            .map(|t| now.duration_since(t) >= DROP_LOG_INTERVAL)
                            .unwrap_or(true);
                        if should_log && crate::process::verbose() {
                            eprintln!("[plugin] V4 notification queue full; dropping newest");
                            last_drop_log = Some(now);
                        }
                    }
                }
                Ok(HostToPluginV4::Request { id, payload }) => {
                    if runner_tx.send(RunnerFrame::Request { id, payload }).is_err() {
                        // Runner loop has exited; stop reading.
                        break;
                    }
                }
                Err(TransportError::Disconnected) => {
                    // Host closed the connection; normal during shutdown.
                    break;
                }
                Err(e) => {
                    eprintln!("[plugin] V4 reader error: {e}");
                    break;
                }
            }
        }
    }));
    shared
        .in_flight
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// `HostApi` implementation used inside the plugin process over the V4
/// multiplexed socket.
pub struct V4PluginHostApi {
    shared: Arc<Shared>,
    notifications: Arc<Mutex<NotificationQueue>>,
    tab_index: usize,
    document_cache: OnceCell<CadDocument>,
    interactive: Rc<RefCell<HashMap<u64, Box<dyn InteractiveCommand>>>>,
    next_command_id: Cell<u64>,
    record_cache: RefCell<HashMap<(Handle, String), &'static ExtendedDataRecord>>,
    doc_view: RefCell<Option<DocumentViewInfo>>,
    doc_view_v4: RefCell<Option<(u64, DocumentViewInfo)>>,
    tab_id_cache: Cell<Option<u64>>,
}

impl V4PluginHostApi {
    fn new(
        shared: Arc<Shared>,
        notifications: Arc<Mutex<NotificationQueue>>,
        tab_index: usize,
        interactive: Rc<RefCell<HashMap<u64, Box<dyn InteractiveCommand>>>>,
    ) -> Self {
        Self {
            shared,
            notifications,
            tab_index,
            document_cache: OnceCell::new(),
            interactive,
            next_command_id: Cell::new(1),
            record_cache: RefCell::new(HashMap::new()),
            doc_view: RefCell::new(None),
            doc_view_v4: RefCell::new(None),
            tab_id_cache: Cell::new(None),
        }
    }

    fn request(
        &self,
        req: PluginRequest,
    ) -> Result<PluginResponse, crate::ipc::transport::TransportError> {
        send_plugin_request(&self.shared, self.tab_id_cache.get(), req)
    }

    fn fetch_document(&self) -> CadDocument {
        match self.request(PluginRequest::DocumentSnapshot) {
            Ok(PluginResponse::Document(doc)) => *doc,
            Ok(other) => {
                eprintln!("[plugin] unexpected DocumentSnapshot response: {other:?}");
                CadDocument::default()
            }
            Err(e) => {
                eprintln!("[plugin] failed to fetch document snapshot: {e}");
                CadDocument::default()
            }
        }
    }
}

/// Send a single plugin-to-host request using the shared V4 connection state.
/// This is the inner logic shared by [`V4PluginHostApi::request`] and the
/// thread-safe [`V4PluginRequestSender`].
fn send_plugin_request(
    shared: &Arc<Shared>,
    tab_id: Option<u64>,
    req: PluginRequest,
) -> Result<PluginResponse, crate::ipc::transport::TransportError> {
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    shared
        .in_flight
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, tx);
    let send_result = {
        let mut writer = shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        send(
            &mut writer,
            &PluginToHostV4::Request {
                id,
                tab_id,
                payload: req,
            },
        )
    };
    if let Err(error) = send_result {
        shared
            .in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        return Err(error);
    }
    match rx.recv() {
        Ok(resp) => Ok(resp),
        Err(_) => Err(crate::ipc::transport::TransportError::Io(
            std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "V4 reader thread disconnected",
            ),
        )),
    }
}

/// Thread-safe wrapper around the V4 connection so worker threads can issue
/// host requests for the originating document tab.
struct V4PluginRequestSender {
    shared: Arc<Shared>,
    tab_id: u64,
}

impl PluginRequestSender for V4PluginRequestSender {
    fn request(&self, req: PluginRequest) -> Result<PluginResponse, PluginRequestError> {
        send_plugin_request(&self.shared, Some(self.tab_id), req)
            .map_err(|e| PluginRequestError(e.to_string()))
    }
}

impl HostApi for V4PluginHostApi {
    fn tab_index(&self) -> usize {
        self.tab_index
    }

    fn document(&self) -> &CadDocument {
        self.document_cache.get_or_init(|| self.fetch_document())
    }

    fn document_mut(&mut self) -> &mut CadDocument {
        if self.document_cache.get().is_none() {
            let doc = self.fetch_document();
            let _ = self.document_cache.set(doc);
        }
        self.document_cache.get_mut().expect("document initialized")
    }

    fn add_entity(&mut self, entity: EntityType) -> Handle {
        match self.request(PluginRequest::AddEntity(entity)) {
            Ok(PluginResponse::Handle(h)) => {
                self.document_cache = OnceCell::new();
                h
            }
            Ok(other) => {
                eprintln!("[plugin] unexpected AddEntity response: {other:?}");
                Handle::default()
            }
            Err(e) => {
                eprintln!("[plugin] AddEntity failed: {e}");
                Handle::default()
            }
        }
    }

    fn add_entities(&mut self, entities: Vec<EntityType>) -> Vec<Handle> {
        match self.request(PluginRequest::AddEntities(entities)) {
            Ok(PluginResponse::Handles(handles)) => {
                self.document_cache = OnceCell::new();
                handles
            }
            Ok(other) => {
                eprintln!("[plugin] unexpected AddEntities response: {other:?}");
                Vec::new()
            }
            Err(e) => {
                eprintln!("[plugin] AddEntities failed: {e}");
                Vec::new()
            }
        }
    }

    fn update_entity(&mut self, entity: EntityType) -> bool {
        match self.request(PluginRequest::UpdateEntity(entity)) {
            Ok(PluginResponse::Bool(b)) => {
                if b {
                    self.document_cache = OnceCell::new();
                }
                b
            }
            Ok(other) => {
                eprintln!("[plugin] unexpected UpdateEntity response: {other:?}");
                false
            }
            Err(e) => {
                eprintln!("[plugin] UpdateEntity failed: {e}");
                false
            }
        }
    }

    fn remove_entity(&mut self, handle: Handle) -> bool {
        match self.request(PluginRequest::RemoveEntity { handle }) {
            Ok(PluginResponse::Bool(b)) => {
                if b {
                    self.document_cache = OnceCell::new();
                }
                b
            }
            Ok(other) => {
                eprintln!("[plugin] unexpected RemoveEntity response: {other:?}");
                false
            }
            Err(e) => {
                eprintln!("[plugin] RemoveEntity failed: {e}");
                false
            }
        }
    }

    fn bump_geometry(&mut self) {
        let _ = self.request(PluginRequest::BumpGeometry);
    }

    fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord> {
        let key = (handle, app_name.to_string());
        {
            let cache = self.record_cache.borrow();
            if let Some(&r) = cache.get(&key) {
                return Some(r);
            }
        }
        match self.request(PluginRequest::ReadRecord {
            handle,
            app_name: app_name.to_string(),
        }) {
            Ok(PluginResponse::Record(rec)) => rec.map(|r| {
                let leaked: &'static ExtendedDataRecord = Box::leak(Box::new(r));
                self.record_cache.borrow_mut().insert(key, leaked);
                leaked
            }),
            Ok(other) => {
                eprintln!("[plugin] unexpected ReadRecord response: {other:?}");
                None
            }
            Err(e) => {
                eprintln!("[plugin] ReadRecord failed: {e}");
                None
            }
        }
    }

    fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool {
        let app = record.application_name.clone();
        match self
            .request(PluginRequest::WriteRecord { handle, record })
        {
            Ok(PluginResponse::Bool(b)) => {
                if b {
                    self.record_cache.borrow_mut().remove(&(handle, app));
                }
                b
            }
            Ok(other) => {
                eprintln!("[plugin] unexpected WriteRecord response: {other:?}");
                false
            }
            Err(e) => {
                eprintln!("[plugin] WriteRecord failed: {e}");
                false
            }
        }
    }

    fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool {
        match self.request(PluginRequest::RemoveRecord {
            handle,
            app_name: app_name.to_string(),
        }) {
            Ok(PluginResponse::Bool(b)) => {
                if b {
                    self.record_cache
                        .borrow_mut()
                        .remove(&(handle, app_name.to_string()));
                }
                b
            }
            Ok(other) => {
                eprintln!("[plugin] unexpected RemoveRecord response: {other:?}");
                false
            }
            Err(e) => {
                eprintln!("[plugin] RemoveRecord failed: {e}");
                false
            }
        }
    }

    fn push_undo(&mut self, label: &str) {
        if let Err(e) = self.request(PluginRequest::PushUndo {
            label: label.to_string(),
        }) {
            eprintln!("[plugin] push_undo failed: {e}");
        }
    }

    fn set_dirty(&mut self) {
        if let Err(e) = self.request(PluginRequest::SetDirty) {
            eprintln!("[plugin] set_dirty failed: {e}");
        }
    }

    fn push_info(&mut self, msg: &str) {
        if let Err(e) = self.request(PluginRequest::PushInfo(msg.to_string())) {
            eprintln!("[plugin] push_info failed: {e}");
        }
    }

    fn push_output(&mut self, msg: &str) {
        if let Err(e) = self.request(PluginRequest::PushOutput(msg.to_string())) {
            eprintln!("[plugin] push_output failed: {e}");
        }
    }

    fn push_error(&mut self, msg: &str) {
        if let Err(e) = self.request(PluginRequest::PushError(msg.to_string())) {
            eprintln!("[plugin] push_error failed: {e}");
        }
    }

    fn start_interactive(&mut self, command: Box<dyn InteractiveCommand>) {
        let id = self.next_command_id.get();
        self.next_command_id.set(id + 1);
        self.interactive.borrow_mut().insert(id, command);
        if let Err(e) = self.request(PluginRequest::StartInteractive { command_id: id }) {
            eprintln!("[plugin] start_interactive failed: {e}");
        }
    }

    fn plugin_state_any(&self, _plugin_id: &str) -> Option<&(dyn Any + Send + Sync)> {
        None
    }

    fn plugin_state_any_mut(&mut self, _plugin_id: &str) -> Option<&mut (dyn Any + Send + Sync)> {
        None
    }

    fn ensure_plugin_state_any(
        &mut self,
        _plugin_id: &'static str,
        _init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
    ) -> &mut (dyn Any + Send + Sync) {
        panic!("ensure_plugin_state is not supported for out-of-process plugins; keep state in the plugin crate")
    }

    fn document_reader(&self) -> Box<dyn DocumentReader + '_> {
        {
            let mut view = self.doc_view.borrow_mut();
            if view.is_none() {
                match self.request(PluginRequest::OpenDocumentView) {
                    Ok(PluginResponse::DocumentView { path, version }) => {
                        *view = Some(DocumentViewInfo { path, version });
                    }
                    Ok(other) => {
                        eprintln!("[plugin] unexpected OpenDocumentView response: {other:?}");
                    }
                    Err(e) => {
                        eprintln!("[plugin] OpenDocumentView request failed: {e}");
                    }
                }
            }
        }
        match self.doc_view.borrow().as_ref() {
            Some(info) => match SharedDocumentReader::<DocumentViewData>::open(Path::new(&info.path)) {
                Ok(reader) => Box::new(reader),
                Err(e) => {
                    eprintln!(
                        "[plugin] failed to open document view at {}: {e}",
                        info.path
                    );
                    Box::new(EmptyDocumentReader)
                }
            },
            None => Box::new(EmptyDocumentReader),
        }
    }

    fn notify_plugin(&mut self, command_id: Option<u64>, notification: PluginNotification) {
        let envelope = NotificationEnvelope {
            command_id,
            payload: notification,
        };
        let mut writer = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = send(&mut writer, &PluginToHostV4::Notification(envelope)) {
            eprintln!("[plugin] notify_plugin failed: {e}");
        }
    }

    fn try_recv_notification(&mut self) -> Option<(Option<u64>, HostNotification)> {
        self.notifications.lock().unwrap_or_else(|e| e.into_inner()).try_recv().ok()
    }

    fn plugin_request_sender(&self) -> Option<Box<dyn PluginRequestSender>> {
        Some(Box::new(V4PluginRequestSender {
            shared: Arc::clone(&self.shared),
            tab_id: self.tab_id(),
        }))
    }

    fn tab_id(&self) -> u64 {
        if let Some(id) = self.tab_id_cache.get() {
            return id;
        }
        match self.request(PluginRequest::GetTabId) {
            Ok(PluginResponse::TabId(id)) => {
                self.tab_id_cache.set(Some(id));
                id
            }
            Ok(other) => {
                eprintln!("[plugin] unexpected GetTabId response: {other:?}");
                self.tab_index as u64
            }
            Err(e) => {
                eprintln!("[plugin] GetTabId failed: {e}");
                self.tab_index as u64
            }
        }
    }

    fn document_view_v4(&mut self, tab_id: u64) -> Option<DocumentViewInfo> {
        {
            let mut view = self.doc_view_v4.borrow_mut();
            if view.as_ref().map(|(cached_id, _)| *cached_id) != Some(tab_id) {
                match self.request(PluginRequest::OpenDocumentViewV4 { tab_id }) {
                    Ok(PluginResponse::DocumentViewV4 { path, version }) => {
                        *view = Some((tab_id, DocumentViewInfo { path, version }));
                    }
                    Ok(other) => {
                        eprintln!("[plugin] unexpected OpenDocumentViewV4 response: {other:?}");
                    }
                    Err(e) => {
                        eprintln!("[plugin] OpenDocumentViewV4 request failed: {e}");
                    }
                }
            }
        }
        self.doc_view_v4
            .borrow()
            .as_ref()
            .map(|(_, info)| info.clone())
    }

    fn close_document_view_v4(&mut self, tab_id: u64) {
        let _ = self.request(PluginRequest::CloseDocumentViewV4 { tab_id });
        let mut view = self.doc_view_v4.borrow_mut();
        if view.as_ref().map(|(cached_id, _)| *cached_id) == Some(tab_id) {
            view.take();
        }
    }

    fn document_path(&self, tab_id: u64) -> Option<std::path::PathBuf> {
        match self.request(PluginRequest::DocumentPath { tab_id }) {
            Ok(PluginResponse::DocumentPath(Some(path))) => Some(std::path::PathBuf::from(path)),
            Ok(PluginResponse::DocumentPath(None)) => None,
            Ok(other) => {
                eprintln!("[plugin] unexpected DocumentPath response: {other:?}");
                None
            }
            Err(e) => {
                eprintln!("[plugin] DocumentPath request failed: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
impl V4Client {
    /// Test-only constructor that splits `stream` into a reader/writer pair
    /// and starts the reader thread without performing a handshake.
    pub(crate) fn from_stream_for_test(stream: Stream) -> Self {
        let reader = stream.try_clone().expect("try_clone");
        let (notify_tx, notify_rx) = mpsc::sync_channel(notify_queue_size());
        let (runner_tx, runner_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            writer: Mutex::new(stream),
            next_id: AtomicU64::new(1),
            in_flight: Mutex::new(HashMap::new()),
        });
        let shared_for_reader = Arc::clone(&shared);
        let notifications = Arc::new(Mutex::new(notify_rx));
        std::thread::spawn(move || {
            reader_thread(reader, shared_for_reader, notify_tx, runner_tx)
        });
        Self {
            shared,
            notifications,
            runner_queue: runner_rx,
        }
    }
}

struct EmptyDocumentReader;

impl DocumentReader for EmptyDocumentReader {
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

#[cfg(all(test, feature = "host"))]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    use interprocess::local_socket::{
        traits::{Listener, Stream as StreamTrait},
        GenericNamespaced, ListenerOptions, Stream, ToNsName,
    };

    use super::*;
    use crate::ipc::transport::recv;
    use crate::ipc::v4::protocol::{HostToPluginV4, PluginToHostV4};
    use crate::test_lock::ENV_LOCK;
    use acadrust::entities::Point;

    fn unique_socket_name() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("ocs_plugin_v4_client_test_{}_{}", std::process::id(), n)
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
    fn v4_client_request_response_matching() {
        let (host_stream, mut runner_stream) = connect_pair();
        let client = V4Client::from_stream_for_test(host_stream);

        let runner = thread::spawn(move || {
            let req = recv::<PluginToHostV4>(&mut runner_stream).unwrap();
            match req {
                PluginToHostV4::Request {
                    id,
                    tab_id: _,
                    payload: PluginRequest::PushInfo(s),
                } => {
                    assert_eq!(s, "hello host");
                    send(
                        &mut runner_stream,
                        &HostToPluginV4::Response {
                            id,
                            payload: PluginResponse::Ok,
                        },
                    )
                    .unwrap();
                }
                other => panic!("unexpected: {other:?}"),
            }
        });

        let mut api = client.plugin_host_api(0, Rc::new(RefCell::new(HashMap::new())));
        api.push_info("hello host");
        runner.join().unwrap();
    }

    #[test]
    fn v4_client_add_entities_request_response_matching() {
        let (host_stream, mut runner_stream) = connect_pair();
        let client = V4Client::from_stream_for_test(host_stream);

        let runner = thread::spawn(move || {
            let req = recv::<PluginToHostV4>(&mut runner_stream).unwrap();
            match req {
                PluginToHostV4::Request {
                    id,
                    tab_id: _,
                    payload: PluginRequest::AddEntities(v),
                } => {
                    assert_eq!(v.len(), 2);
                    send(
                        &mut runner_stream,
                        &HostToPluginV4::Response {
                            id,
                            payload: PluginResponse::Handles(vec![Handle::new(1), Handle::new(2)]),
                        },
                    )
                    .unwrap();
                }
                other => panic!("unexpected: {other:?}"),
            }
        });

        let mut api = client.plugin_host_api(0, Rc::new(RefCell::new(HashMap::new())));
        let handles = api.add_entities(vec![
            EntityType::Point(Point::new()),
            EntityType::Point(Point::new()),
        ]);
        assert_eq!(handles, vec![Handle::new(1), Handle::new(2)]);
        runner.join().unwrap();
    }

    #[test]
    fn concurrent_requests_match_out_of_order_responses() {
        let (host_stream, mut runner_stream) = connect_pair();
        let client = V4Client::from_stream_for_test(host_stream);
        let sender = Arc::new(V4PluginRequestSender {
            shared: Arc::clone(&client.shared),
            tab_id: 42,
        });

        let runner = thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..2 {
                match recv::<PluginToHostV4>(&mut runner_stream).unwrap() {
                    PluginToHostV4::Request {
                        id,
                        tab_id: Some(42),
                        payload: PluginRequest::PushInfo(message),
                    } => requests.push((id, message)),
                    other => panic!("unexpected: {other:?}"),
                }
            }
            for (id, message) in requests.into_iter().rev() {
                send(
                    &mut runner_stream,
                    &HostToPluginV4::Response {
                        id,
                        payload: PluginResponse::Bool(message == "first"),
                    },
                )
                .unwrap();
            }
        });

        let first_sender = Arc::clone(&sender);
        let first = thread::spawn(move || {
            first_sender
                .request(PluginRequest::PushInfo("first".to_string()))
                .unwrap()
        });
        let second = thread::spawn(move || {
            sender
                .request(PluginRequest::PushInfo("second".to_string()))
                .unwrap()
        });

        assert!(matches!(first.join().unwrap(), PluginResponse::Bool(true)));
        assert!(matches!(second.join().unwrap(), PluginResponse::Bool(false)));
        runner.join().unwrap();
    }

    #[test]
    fn v4_client_notification_queue_size_env_is_honored() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE").ok();
        std::env::set_var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE", "7");
        assert_eq!(notify_queue_size(), 7);
        match prev {
            Some(v) => std::env::set_var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE", v),
            None => std::env::remove_var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE"),
        }
    }

    #[test]
    fn v4_client_notification_queue_drops_newest_on_overflow() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE").ok();
        std::env::set_var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE", "1");
        let (host_stream, mut runner_stream) = connect_pair();
        let client = V4Client::from_stream_for_test(host_stream);

        // Send two notifications; the bounded queue has capacity 1, so the
        // second one should be dropped.
        send(
            &mut runner_stream,
            &HostToPluginV4::Notification(NotificationEnvelope {
                command_id: Some(1),
                payload: HostNotification::Cancel,
            }),
        )
        .unwrap();
        send(
            &mut runner_stream,
            &HostToPluginV4::Notification(NotificationEnvelope {
                command_id: Some(2),
                payload: HostNotification::Cancel,
            }),
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(100));
        let first = client.try_recv_notification();
        let second = client.try_recv_notification();
        assert_eq!(first.map(|(id, _)| id), Some(Some(1)));
        assert_eq!(second, None);

        match prev {
            Some(v) => std::env::set_var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE", v),
            None => std::env::remove_var("OCS_PLUGIN_NOTIFY_QUEUE_SIZE"),
        }
    }

    #[test]
    fn execute_code_responder_emits_expected_frame() {
        let (host_stream, mut runner_stream) = connect_pair();
        let client = V4Client::from_stream_for_test(host_stream);

        let result = ExecutionResult {
            success: true,
            output: Some("42".to_string()),
            error: None,
            error_type: None,
            traceback: None,
            line_number: None,
            column_number: None,
            duration_ms: 1.0,
        };

        let responder = client.execute_code_responder(7);
        responder(result);

        let got = recv::<PluginToHostV4>(&mut runner_stream).unwrap();
        match got {
            PluginToHostV4::Response {
                id: 7,
                payload: HostResponse::CodeExecutionResult(r),
            } => {
                assert!(r.success);
                assert_eq!(r.output, Some("42".to_string()));
            }
            other => panic!("unexpected frame: {other:?}"),
        }
    }
}
