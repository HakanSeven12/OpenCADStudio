//! Loopback REST channel hosted by the GUI process. `OpenCADStudio.exe
//! "<file>" --http <port>` boots the normal editor AND serves this bridge, so
//! a client gets the full REST surface of `--http` — create, move, plot,
//! query, everything the headless server does — aimed at the drawing the
//! person at the screen is working on, plus the two interactive operations
//! only this process can answer (`getpoint`, `user_select`: the person
//! picks, and the HTTP connection is the thing that waits). Routes resolve
//! through the same `rest::plan` table as the headless server; requests are
//! forwarded into the GUI's automation queue (`Envelope`, exactly like the
//! native bridge in `transport`) and the JSON response is written back over
//! HTTP. The channel binds loopback only — anything that can reach the port
//! can drive the session, exactly like the headless server.

use super::{Envelope, Reply};
use crate::rest::{self, HttpRequest, Plan, RETRYABLE};
use iced::futures::{channel::mpsc, Stream};
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Port handed over by `main` when the GUI hosts the channel. Set before the
/// iced runtime boots, so the subscription worker sees it on first run.
/// Zero means "no channel" (plain GUI runs).
static GUI_HTTP_PORT: AtomicU16 = AtomicU16::new(0);
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Called from `main` before the GUI boots.
pub fn set_gui_http_port(port: u16) {
    GUI_HTTP_PORT.store(port, Ordering::SeqCst);
}

pub(in crate::app) fn subscribe() -> iced::Subscription<Envelope> {
    iced::Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Envelope> {
    iced::stream::channel(32, |sender| async move {
        let port = GUI_HTTP_PORT.load(Ordering::SeqCst);
        if port != 0 {
            std::thread::spawn(move || {
                if let Err(error) = listen(sender, port, Arc::new(AtomicU16::new(0))) {
                    eprintln!("--http (GUI): cannot serve 127.0.0.1:{port}: {error}");
                }
            });
        }
        iced::futures::future::pending::<()>().await;
    })
}

fn listen(
    sender: mpsc::Sender<Envelope>,
    port: u16,
    bound_port: Arc<AtomicU16>,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    bound_port.store(listener.local_addr()?.port(), Ordering::SeqCst);
    eprintln!("OpenCADStudio GUI REST listening on http://127.0.0.1:{port}/api/v1");
    // One thread per connection: a `getpoint` or `user_select` request stays
    // open until the person answers, and must not block the other reads
    // (get_selection…).
    let clients = Arc::new(AtomicUsize::new(0));
    // One document-id cache for the whole listener: a state read primes it,
    // later mutations address the live drawing without the caller repeating
    // the id — the same ergonomics as the headless server.
    let document_cache: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    for stream in listener.incoming().flatten() {
        if clients.fetch_add(1, Ordering::SeqCst) >= 8 {
            clients.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let clients = clients.clone();
        let sender = sender.clone();
        let document_cache = document_cache.clone();
        std::thread::spawn(move || {
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
            if let Ok(Some(request)) = rest::read_request(&mut stream) {
                if request.method == "OPTIONS" {
                    let _ = rest::write_response(&mut stream, 204, &Value::Null);
                } else {
                    let (status, body) = forward(&request, &sender, &document_cache, &stream);
                    let _ = rest::write_response(&mut stream, status, &body);
                }
            }
            clients.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

/// How long one request may hold its connection. The interactive picks wait
/// for the person; everything else bounds a slow op (plot, open a large
/// drawing) without wedging the connection forever.
const DEFAULT_WAIT_SECS: u64 = 300;
const PICK_WAIT_SECS: u64 = 30 * 60;

fn forward(
    request: &HttpRequest,
    sender: &mpsc::Sender<Envelope>,
    document_cache: &Mutex<Option<u64>>,
    stream: &TcpStream,
) -> (u16, Value) {
    let path = request.path.trim_end_matches('/');
    let path = if path.is_empty() { "/" } else { path };
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments.first() != Some(&"api") || segments.get(1) != Some(&"v1") {
        return (404, json!({"ok": false, "code": "unknown_route", "error": "Use /api/v1/…"}));
    }
    let plan = rest::plan(&request.method, &segments[2..], request);
    // The picks park until the person answers; an `operation` call is itself
    // a poll and must come straight back.
    let wait = match plan.op() {
        Some("getpoint" | "user_select") => PICK_WAIT_SECS,
        Some("operation") | None => 15,
        Some(_) => DEFAULT_WAIT_SECS,
    };
    match plan {
        Plan::Local(status, body) => (status, body),
        Plan::Ready => {
            let state = transact(sender, json!({"protocol":1,"op":"state"}), wait, stream);
            harvest(document_cache, &state);
            (200, json!({
                "ok": true,
                "ready": true,
                "version": state["version"],
                "session_id": state["session_id"],
                "document_id": state["document_id"],
            }))
        }
        Plan::Run { request, created } => {
            let response = transact(sender, prepare(request), wait, stream);
            harvest(document_cache, &response);
            (rest::map_status(response.clone(), created == 201), response)
        }
        Plan::Mutate { op, fields, created } => {
            let cached = *document_cache.lock().unwrap();
            let mut response = transact(sender, mutate_request(&op, &fields, cached), wait, stream);
            let code = response["code"].as_str().unwrap_or("");
            if response["ok"] == false && RETRYABLE.contains(&code) {
                // The cache is behind the live GUI; refresh from a state read
                // and retry once, exactly like the headless server.
                let state = transact(sender, json!({"protocol":1,"op":"state"}), wait, stream);
                harvest(document_cache, &state);
                let cached = *document_cache.lock().unwrap();
                response = transact(sender, mutate_request(&op, &fields, cached), wait, stream);
            }
            harvest(document_cache, &response);
            let status = rest::map_status(response.clone(), created == 201);
            (if status == 200 { created } else { status }, compact(response))
        }
    }
}

/// One envelope round trip: queue the request into the GUI and wait up to
/// `wait_secs` for its reply.
fn call(sender: &mpsc::Sender<Envelope>, request: Value, wait_secs: u64) -> Option<Value> {
    let (reply, response) = std::sync::mpsc::channel();
    sender
        .clone()
        .try_send(Envelope {
            request,
            reply: Reply::Native(reply),
        })
        .ok()?;
    response.recv_timeout(Duration::from_secs(wait_secs)).ok()
}

/// Send one request and hold the connection for the GUI's answer.
/// Interactive requests (`getpoint`, `user_select`) and async ops answer
/// `accepted` first and only complete when the person at the screen acts or
/// the task finishes; poll the operation on the caller's behalf so the HTTP
/// connection is the one thing that waits.
///
/// A parked pick is also watched from the other end: the moment the HTTP
/// client hangs up (its own timeout, a crash, a killed probe), the pending
/// pick is retracted with `cancel` so the next pick starts clean instead of
/// dying on `interactive_pending` until the person happens to press Esc.
fn transact(
    sender: &mpsc::Sender<Envelope>,
    request: Value,
    wait: u64,
    stream: &TcpStream,
) -> Value {
    let op = request["op"].as_str().unwrap_or("").to_owned();
    let request_id = request["request_id"].as_str().unwrap_or("").to_owned();
    let interactive = matches!(op.as_str(), "getpoint" | "user_select");
    if interactive {
        eprintln!(
            "[pick] {op} {request_id}: parked -- the connection stays open until the person answers"
        );
    }
    let Some(mut response) = call(sender, request, wait) else {
        if interactive {
            eprintln!("[pick] {op} {request_id}: the GUI never accepted the request");
        }
        return json!({"ok":false,"code":"response_timeout","error":"The GUI did not answer in time"});
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(wait);
    while matches!(response["status"].as_str(), Some("accepted" | "running"))
        && std::time::Instant::now() < deadline
    {
        if interactive && peer_gone(stream) {
            eprintln!("[pick] {op} {request_id}: client disconnected -- cancelling the pending pick");
            // The cancel reuses the pick's own request id: if the person
            // answered in the race window, the settled id makes the retract
            // a harmless no-op instead of an Escape into the live session.
            let _ = call(
                sender,
                json!({"protocol":1,"op":"cancel","request_id":request_id}),
                15,
            );
            return json!(
                {"ok":false,"code":"client_disconnected","error":"The HTTP client hung up; the pending pick was cancelled"}
            );
        }
        std::thread::sleep(Duration::from_millis(100));
        let poll_id = response["request_id"].clone();
        let Some(poll) = call(
            sender,
            json!({"protocol":1,"op":"operation","request_id":poll_id}),
            15,
        ) else {
            break;
        };
        response = poll;
    }
    if interactive {
        if matches!(response["status"].as_str(), Some("accepted" | "running")) {
            eprintln!(
                "[pick] {op} {request_id}: no answer in {wait}s -- cancelling the pending pick"
            );
            let _ = call(
                sender,
                json!({"protocol":1,"op":"cancel","request_id":request_id}),
                15,
            );
        } else {
            eprintln!(
                "[pick] {op} {request_id}: settled as {}",
                response["status"].as_str().unwrap_or("unknown")
            );
        }
    }
    response
}

/// True once the HTTP client has hung up: a graceful close surfaces as EOF,
/// an aborted one as a socket error. No bytes waiting (`WouldBlock`) means
/// the client is still there, holding its half of the parked connection.
fn peer_gone(stream: &TcpStream) -> bool {
    let _ = stream.set_nonblocking(true);
    let gone = match stream.peek(&mut [0u8; 1]) {
        Ok(0) => true,
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => false,
        Err(_) => true,
        Ok(_) => false,
    };
    let _ = stream.set_nonblocking(false);
    gone
}

/// The bridge has no descriptor handshake: drop any guessed `session_id`,
/// stamp protocol 1, and give requests without one a bridge request id.
fn prepare(mut request: Value) -> Value {
    if !request.is_object() {
        request = json!({});
    }
    if let Some(object) = request.as_object_mut() {
        object.remove("session_id");
    }
    request["protocol"] = json!(1);
    if request["request_id"].as_str().is_none_or(str::is_empty) {
        let serial = REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        request["request_id"] = json!(format!("gui-http-{serial}"));
    }
    request
}

/// Build the mutation envelope: a request id (the caller's wins), the cached
/// `document_id`, then the body fields on top.
fn mutate_request(op: &str, fields: &Value, document_id: Option<u64>) -> Value {
    let mut envelope = json!({"protocol":1,"op":op});
    if let Some(id) = document_id {
        envelope["document_id"] = json!(id);
    }
    if let Some(object) = fields.as_object() {
        for (key, value) in object {
            envelope[key] = value.clone();
        }
    }
    if envelope["request_id"].as_str().is_none_or(str::is_empty) {
        let serial = REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        envelope["request_id"] = json!(format!("gui-http-{serial}"));
    }
    if let Some(object) = envelope.as_object_mut() {
        object.remove("session_id");
    }
    envelope
}

/// Track the active document from responses (state reads and settle
/// snapshots both carry it) so later mutations address the live drawing.
fn harvest(document_cache: &Mutex<Option<u64>>, response: &Value) {
    let id = response
        .get("document_id")
        .and_then(Value::as_u64)
        .or_else(|| response["state"]["document_id"].as_u64());
    if let Some(id) = id {
        *document_cache.lock().unwrap() = Some(id);
    }
}

/// Drop the bulky `state` snapshot from mutation answers, like the headless
/// server; clients read it explicitly from GET /state.
fn compact(response: Value) -> Value {
    if let Some(object) = response.as_object() {
        let trimmed: serde_json::Map<String, Value> = object
            .iter()
            .filter(|(key, _)| key.as_str() != "state")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        return Value::Object(trimmed);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use iced::futures::StreamExt;
    use std::io::{BufRead, BufReader, Read, Write as _};

    /// Each test owns its port holder: two bridge tests running in parallel
    /// must never read each other's bound port.
    fn wait_for_bridge(bound_port: &AtomicU16) -> u16 {
        for _ in 0..50 {
            let port = bound_port.load(Ordering::SeqCst);
            if port != 0 {
                return port;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("GUI http bridge never bound a port");
    }

    /// Post one request without waiting; the bridge only answers once the
    /// GUI side has replied to the forwarded envelope.
    fn post(port: u16, target: &str, body: &str) -> std::net::TcpStream {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let http = format!(
            "POST {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(http.as_bytes()).unwrap();
        stream
    }

    /// Read one HTTP response (status + JSON body) off the wire.
    fn read_response(stream: &mut std::net::TcpStream) -> (u16, Value) {
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if status_line.is_empty() {
                status_line = line.clone();
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = value.trim().parse().unwrap();
            }
        }
        let mut payload = vec![0u8; content_length];
        reader.read_exact(&mut payload).unwrap();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap();
        (status, serde_json::from_slice(&payload).unwrap())
    }

    #[test]
    fn bridge_serves_get_selection_over_http_and_refuses_unknown_paths() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        let _ = app.control_request(json!({"protocol":1,"op":"new","request_id":"n1"}));
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"LINE","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let value = u64::from_str_radix(&handle, 16).unwrap();
        app.tabs[app.active_tab]
            .scene
            .select_entity(acadrust::Handle::new(value), false);

        let (sender, mut receiver) = mpsc::channel::<Envelope>(8);
        let bound = Arc::new(AtomicU16::new(0));
        let listen_bound = bound.clone();
        std::thread::spawn(move || listen(sender, 0, listen_bound).unwrap());
        let port = wait_for_bridge(&bound);

        // Read the selection back over loopback HTTP the way the client does.
        let mut stream = post(
            port,
            "/api/v1/get_selection",
            r#"{"session_id":"guessed","request_id":"bridge-1"}"#,
        );
        // The bridge forwarded the envelope; this test plays the GUI and
        // answers it, which is what unblocks the HTTP response.
        let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
        assert_eq!(envelope.request["op"], "get_selection");
        assert!(envelope.request["session_id"].is_null());
        assert_eq!(envelope.request["request_id"], "bridge-1");
        let reply = app.control_request(envelope.request).0;
        envelope.reply.send(reply);

        let (status, body) = read_response(&mut stream);
        assert_eq!(status, 200);
        assert_eq!(body["ok"], true, "{body}");
        assert_eq!(body["status"], "completed");
        assert_eq!(body["result"]["count"], 1);
        assert_eq!(body["result"]["entities"][0]["handle"], handle.as_str());
        assert!(body["result"]["entities"][0]["bounds"].is_object(), "{body}");
        assert_eq!(body["result"]["entities"][0]["type"], "Line");

        // Unrouted paths are refused locally; the GUI is never woken for
        // them. (Unknown POSTs are op attempts and DO reach the pipeline,
        // same as the headless server.)
        let mut stream = get(port, "/api/v1/nope");
        let (status, body) = read_response(&mut stream);
        assert_eq!(status, 404);
        assert_eq!(body["code"], "unknown_route", "{body}");
    }

    /// GET without a body — the read half of the REST surface.
    fn get(port: u16, target: &str) -> std::net::TcpStream {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let http = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        stream.write_all(http.as_bytes()).unwrap();
        stream
    }

    /// Answer the next `count` envelopes the way the live GUI would.
    fn drain(app: &mut OpenCADStudio, receiver: &mut mpsc::Receiver<Envelope>, count: usize) {
        for _ in 0..count {
            let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
            let reply = app.control_request(envelope.request.clone()).0;
            envelope.reply.send(reply);
        }
    }

    #[test]
    fn bridge_parks_getpoint_without_blocking_other_reads() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        let _ = app.control_request(json!({"protocol":1,"op":"new","request_id":"n1"}));
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"LINE","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let value = u64::from_str_radix(&handle, 16).unwrap();
        app.tabs[app.active_tab]
            .scene
            .select_entity(acadrust::Handle::new(value), false);

        let (sender, mut receiver) = mpsc::channel::<Envelope>(8);
        let bound = Arc::new(AtomicU16::new(0));
        let listen_bound = bound.clone();
        std::thread::spawn(move || listen(sender, 0, listen_bound).unwrap());
        let port = wait_for_bridge(&bound);

        // The client asks for a point and holds that connection open.
        let mut point_conn = post(
            port,
            "/api/v1/getpoint",
            r#"{"request_id":"gp-b","prompt":"Chọn vị trí QR"}"#,
        );
        // While the pick is pending, a second connection reads the selection
        // — thread-per-connection keeps the parked request from blocking it.
        let mut sel_conn = post(port, "/api/v1/get_selection", "{}");
        let mut polls = 0usize;
        let (point_status, point_body, sel_body) = loop {
            let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
            let reply = app.control_request(envelope.request.clone()).0;
            if envelope.request["op"] == "getpoint" {
                // The GUI accepted the pick; the bridge now polls `operation`
                // until the person answers. Simulate the click right away.
                assert_eq!(reply["status"], "accepted", "{reply}");
                envelope.reply.send(reply);
                app.tabs[app.active_tab].last_cursor_world =
                    glam::DVec3::new(1.5, 2.5, 0.0);
                let _ = app.update(crate::app::Message::ViewportLeftPress);
            } else if envelope.request["op"] == "get_selection" {
                envelope.reply.send(reply);
                let (status, body) = read_response(&mut sel_conn);
                assert_eq!(status, 200, "{body}");
                assert_eq!(body["result"]["count"], 1);
                assert_eq!(body["result"]["entities"][0]["handle"], handle.as_str());
            } else {
                // An `operation` poll from the parked getpoint connection.
                polls += 1;
                assert!(polls < 100, "getpoint never completed: {reply}");
                envelope.reply.send(reply.clone());
                if reply["status"] == "completed" {
                    let point_status = read_response(&mut point_conn);
                    break (point_status.0, point_status.1, Value::Null);
                }
            }
        };
        assert_eq!(point_status, 200, "{point_body}");
        assert_eq!(point_body["status"], "completed", "{point_body}");
        assert_eq!(point_body["result"]["point"], json!([1.5, 2.5, 0.0]));
        assert!(sel_body.is_null());
    }

    /// The client stops waiting mid-pick (its own timeout, a killed probe):
    /// the bridge must notice the hang-up, retract the pending pick with
    /// `cancel`, and leave the channel free for the next pick — instead of
    /// holding the session until the person happens to press Esc.
    #[test]
    fn bridge_cancels_a_parked_pick_when_the_client_disconnects() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        let _ = app.control_request(json!({"protocol":1,"op":"new","request_id":"n1"}));

        let (sender, mut receiver) = mpsc::channel::<Envelope>(8);
        let bound = Arc::new(AtomicU16::new(0));
        let listen_bound = bound.clone();
        std::thread::spawn(move || listen(sender, 0, listen_bound).unwrap());
        let port = wait_for_bridge(&bound);

        // The client parks a pick…
        let point_conn = post(port, "/api/v1/getpoint", r#"{"request_id":"gp-drop"}"#);
        let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
        assert_eq!(envelope.request["op"], "getpoint");
        let reply = app.control_request(envelope.request.clone()).0;
        assert_eq!(reply["status"], "accepted", "{reply}");
        envelope.reply.send(reply);

        // …then gives up: the connection closes while the person is
        // presumably still deciding.
        drop(point_conn);

        // The bridge notices and retracts the pick; `operation` polls that
        // slip in first are answered as `running` like the GUI would.
        let mut polls = 0usize;
        loop {
            let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
            if envelope.request["op"] == "cancel" {
                assert_eq!(envelope.request["request_id"], "gp-drop");
                let reply = app.control_request(envelope.request).0;
                assert_eq!(reply["status"], "cancelled", "{reply}");
                assert_eq!(reply["result"]["cancelled"], true, "{reply}");
                break;
            }
            assert_eq!(envelope.request["op"], "operation", "unexpected envelope");
            polls += 1;
            assert!(polls < 100, "the disconnect was never noticed");
            let reply = app.control_request(envelope.request.clone()).0;
            envelope.reply.send(reply);
        }

        // The channel is free: the next pick is accepted right away instead
        // of dying on `interactive_pending`. Answer it with a click so the
        // test leaves no session behind.
        let mut next_conn = post(port, "/api/v1/getpoint", r#"{"request_id":"gp-next"}"#);
        let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
        assert_eq!(envelope.request["op"], "getpoint");
        let reply = app.control_request(envelope.request).0;
        assert_eq!(reply["status"], "accepted", "{reply}");
        envelope.reply.send(reply);
        app.tabs[app.active_tab].last_cursor_world = glam::DVec3::new(2.0, 3.0, 0.0);
        let _ = app.update(crate::app::Message::ViewportLeftPress);
        let mut polls = 0usize;
        let (_, body) = loop {
            let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
            let reply = app.control_request(envelope.request.clone()).0;
            envelope.reply.send(reply.clone());
            polls += 1;
            assert!(polls < 100, "the retry pick never completed: {reply}");
            if reply["status"] == "completed" {
                break read_response(&mut next_conn);
            }
        };
        assert_eq!(body["status"], "completed", "{body}");
        assert_eq!(body["result"]["point"], json!([2.0, 3.0, 0.0]));
    }

    #[test]
    fn bridge_parks_user_select_until_the_person_answers() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        let _ = app.control_request(json!({"protocol":1,"op":"new","request_id":"n1"}));
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"LINE","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();

        let (sender, mut receiver) = mpsc::channel::<Envelope>(8);
        let bound = Arc::new(AtomicU16::new(0));
        let listen_bound = bound.clone();
        std::thread::spawn(move || listen(sender, 0, listen_bound).unwrap());
        let port = wait_for_bridge(&bound);

        // Park one `user_select` POST; the test plays the person at the
        // screen: accept, pick, then press Enter (confirm) or Escape
        // (cancel), and read the final HTTP answer the client receives.
        fn park_and_answer(
            app: &mut OpenCADStudio,
            receiver: &mut mpsc::Receiver<Envelope>,
            port: u16,
            body: &str,
            confirm: bool,
            handle: &str,
        ) -> (u16, Value) {
            let mut conn = post(port, "/api/v1/user_select", body);
            let handle_value = u64::from_str_radix(handle, 16).unwrap();
            let mut polls = 0usize;
            loop {
                let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
                let reply = app.control_request(envelope.request.clone()).0;
                if envelope.request["op"] == "user_select" {
                    // The GUI accepted the pick; the bridge now polls
                    // `operation` until the person answers.
                    assert_eq!(reply["status"], "accepted", "{reply}");
                    envelope.reply.send(reply);
                    app.tabs[app.active_tab]
                        .scene
                        .select_entity(acadrust::Handle::new(handle_value), false);
                    let _ = app.update(if confirm {
                        crate::app::Message::CommandFinalize
                    } else {
                        crate::app::Message::CommandEscape
                    });
                } else {
                    // An `operation` poll from the parked user_select
                    // connection: relay it until it settles.
                    polls += 1;
                    assert!(polls < 100, "user_select never settled: {reply}");
                    envelope.reply.send(reply.clone());
                    if reply["status"] != "running" {
                        return read_response(&mut conn);
                    }
                }
            }
        }

        // Enter after picking the line hands the picked set back with full
        // entity data, the same contract the MCP/native callers see.
        let (status, body) = park_and_answer(
            &mut app,
            &mut receiver,
            port,
            r#"{"request_id":"us-http","type":"LINE","prompt":"Chọn đối tượng mẫu","detail":"full","clear":true}"#,
            true,
            &handle,
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["status"], "completed", "{body}");
        assert_eq!(body["result"]["cancelled"], false);
        assert_eq!(body["result"]["count"], 1);
        assert_eq!(body["result"]["handles"][0], json!(handle));
        assert_eq!(body["result"]["entities"][0]["handle"], handle.as_str());
        assert_eq!(body["result"]["entities"][0]["type"], "Line");
        assert_eq!(body["result"]["entities"][0]["layer"], "0");

        // Escape answers with the cancelled contract over the same route.
        let (status, body) = park_and_answer(
            &mut app,
            &mut receiver,
            port,
            r#"{"request_id":"us-http-esc","prompt":"Chọn lại"}"#,
            false,
            &handle,
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["status"], "cancelled", "{body}");
        assert_eq!(body["result"]["cancelled"], true);
        assert_eq!(body["result"]["count"], 0);
    }

    #[test]
    fn bridge_runs_the_full_rest_surface_against_the_live_gui() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        let _ = app.control_request(json!({"protocol":1,"op":"new","request_id":"n1"}));

        let (sender, mut receiver) = mpsc::channel::<Envelope>(8);
        let bound = Arc::new(AtomicU16::new(0));
        let listen_bound = bound.clone();
        std::thread::spawn(move || listen(sender, 0, listen_bound).unwrap());
        let port = wait_for_bridge(&bound);

        // GET /state primes the document-id cache; the mutations below carry
        // no document_id of their own.
        let mut state_conn = get(port, "/api/v1/state");
        drain(&mut app, &mut receiver, 1);
        let (status, body) = read_response(&mut state_conn);
        assert_eq!(status, 200, "{body}");
        assert!(body["document_id"].is_u64(), "{body}");

        // CREATE through the headless REST URL: 201 with handles, the cached
        // document id addressed for us.
        let mut create_conn = post(
            port,
            "/api/v1/entities",
            r#"{"entities":[{"type":"Line","start":[0,0],"end":[10,0]}]}"#,
        );
        drain(&mut app, &mut receiver, 1);
        let (status, body) = read_response(&mut create_conn);
        assert_eq!(status, 201, "{body}");
        let handle = body["result"]["handles"][0].as_str().unwrap().to_owned();

        // MOVE it through the same URL the headless server serves.
        let mut move_conn = post(
            port,
            "/api/v1/entities/transform",
            &format!(r#"{{"handles":["{handle}"],"action":"move","vector":[5,0]}}"#),
        );
        drain(&mut app, &mut receiver, 1);
        let (status, body) = read_response(&mut move_conn);
        assert_eq!(status, 200, "{body}");
        let mut query_conn = get(port, "/api/v1/entities?type=Line&detail=full");
        drain(&mut app, &mut receiver, 1);
        let (_, body) = read_response(&mut query_conn);
        assert_eq!(body["entities"][0]["start"], json!([5.0, 0.0, 0.0]), "{body}");

        // A stale cache (the person switched documents) costs one state
        // refresh and a retry, not an error.
        let fresh = app.push_test_document();
        let _ = app.control_request(
            json!({"protocol":1,"op":"activate","document_id":fresh,"request_id":"a1"}),
        );
        let mut stale_conn = post(
            port,
            "/api/v1/entities",
            r#"{"entities":[{"type":"Circle","center":[0,0],"radius":1}]}"#,
        );
        // mutate refused (stale) → state read → retry succeeds.
        drain(&mut app, &mut receiver, 3);
        let (status, body) = read_response(&mut stale_conn);
        assert_eq!(status, 201, "{body}");

        // The plain op passthrough works too — MCP-style, explicit
        // document_id, any op the automation surface knows. `select`
        // settles inline; `run` would park the connection on `operation`
        // polls until the live editor's update loop finishes the command.
        let mut select_conn = post(
            port,
            "/api/v1/select",
            &format!(r#"{{"document_id":{fresh},"clear":true}}"#),
        );
        drain(&mut app, &mut receiver, 1);
        let (status, body) = read_response(&mut select_conn);
        assert_eq!(status, 200, "{body}");
    }
}
