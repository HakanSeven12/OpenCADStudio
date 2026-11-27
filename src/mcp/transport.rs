use super::session::Descriptor;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::{BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::mpsc::{Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

pub(super) const MAX_REQUEST: usize = 1_048_576;
pub(super) const MAX_RESPONSE: u64 = 16 * 1024 * 1024;

/// Reader-thread inbox for stdin lines. The reader never writes: the main
/// loop (and, during waits, the wait loop) is the only consumer, so stdout
/// stays single-writer. Non-cancel lines met during a wait are stowed in
/// `backlog` and handled in order once the wait ends.
pub(super) struct CancelPump {
    pub(super) rx: Receiver<Result<String, String>>,
    pub(super) backlog: VecDeque<String>,
}

pub(super) enum PumpEvent {
    Cancelled,
    Quiet,
}

impl CancelPump {
    pub(super) fn next(&mut self) -> Option<String> {
        if let Some(line) = self.backlog.pop_front() {
            return Some(line);
        }
        match self.rx.recv() {
            Ok(Ok(line)) => Some(line),
            Ok(Err(error)) => {
                eprintln!("MCP input error: {error}");
                None
            }
            Err(_) => None,
        }
    }

    /// Drain newly arrived lines for up to `quantum`: consume a cancel naming
    /// `mkey`, stow everything else in order. Never blocks past the quantum.
    pub(super) fn wait_line(&mut self, quantum: Duration, mkey: Option<&str>) -> PumpEvent {
        let deadline = Instant::now() + quantum;
        loop {
            match self.rx.try_recv() {
                Ok(Ok(line)) => {
                    if mkey.is_some_and(|key| is_cancel_for(&line, key)) {
                        return PumpEvent::Cancelled;
                    }
                    self.backlog.push_back(line);
                }
                // Reader gone or input broken: end the wait quietly; the
                // main loop observes the closed channel right after.
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => return PumpEvent::Quiet,
                Err(TryRecvError::Empty) => {
                    if Instant::now() >= deadline {
                        return PumpEvent::Quiet;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}

/// MCP request-id key for cancel matching. String and number ids stay
/// distinct (`7` vs `"7"`): both sides stringify the same JSON value.
pub(super) fn mcp_key(id: &Value) -> String {
    id.to_string()
}

pub(super) fn is_cancel_for(line: &str, mkey: &str) -> bool {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    if message.get("method").and_then(Value::as_str) != Some("notifications/cancelled") {
        return false;
    }
    message
        .get("params")
        .and_then(|params| params.get("requestId"))
        .is_some_and(|id| mcp_key(id) == mkey)
}

pub(super) fn wait_deadline(op: &str, wait_seconds: f64) -> Duration {
    let capped = if matches!(op, "user_select" | "getpoint") {
        wait_seconds.clamp(0.0, INTERACTIVE_MAX_WAIT.as_secs_f64())
    } else {
        wait_seconds.clamp(0.0, 60.0)
    };
    Duration::from_secs_f64(capped)
}

/// Only a wait that actually reaches the interactive ceiling dismisses the
/// prompt. Shorter waits return `running` so the agent can re-poll; without
/// this distinction the first short poll would kill every interactive op.
pub(super) fn interactive_timeout(wait_seconds: f64) -> bool {
    wait_seconds >= INTERACTIVE_MAX_WAIT.as_secs_f64()
}

pub(super) fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn iso8601_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = day_seconds / 3_600;
    let minute = day_seconds % 3_600 / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Tool-call failure split for error routing: Tool-domain failures carry
/// model-actionable guidance and stay `isError` results (SEP-1303); bridge
/// and GUI infrastructure failures (nothing the model can fix) become
/// JSON-RPC errors. `From<String>` defaults to Tool so existing sites keep
/// working unchanged; infrastructure sites opt in explicitly.
#[derive(Debug)]
pub(super) enum CallError {
    Tool(String),
    Infra(String),
    /// Client cancelled the in-flight request: send nothing back.
    Cancelled,
}

/// Interactive picks may legitimately take a human minutes; everything else
/// keeps the 60 s clamp. This bounds a single call; agents re-poll with
/// small waits by design.
pub(super) const INTERACTIVE_MAX_WAIT: Duration = Duration::from_secs(600);

impl From<String> for CallError {
    fn from(message: String) -> Self {
        CallError::Tool(message)
    }
}

impl From<&str> for CallError {
    fn from(message: &str) -> Self {
        CallError::Tool(message.to_string())
    }
}

// Collapses back to String where the caller maps everything to one code
// anyway (resources/read not-found codes); the routing decision is made
// by the caller, not the classification.
impl From<CallError> for String {
    fn from(error: CallError) -> Self {
        error.message().to_string()
    }
}

impl CallError {
    pub(super) fn message(&self) -> &str {
        match self {
            CallError::Tool(message) | CallError::Infra(message) => message,
            CallError::Cancelled => "cancelled by client",
        }
    }

    pub(super) fn infra(message: impl ToString) -> Self {
        CallError::Infra(message.to_string())
    }
}

pub(super) fn exchange(
    descriptor: &Descriptor,
    request: Value,
    timeout: Duration,
) -> Result<Value, CallError> {
    let mut object = request
        .as_object()
        .cloned()
        .ok_or_else(|| CallError::infra("GUI request must be an object"))?;
    object.insert("token".into(), Value::String(descriptor.token.clone()));
    object.insert(
        "session_id".into(),
        Value::String(descriptor.session_id.clone()),
    );
    object.insert("protocol".into(), Value::from(1));
    let mut wire = serde_json::to_vec(&Value::Object(object)).map_err(CallError::infra)?;
    wire.push(b'\n');
    if wire.len() > MAX_REQUEST {
        // Caller-caused (batch too big): model-actionable, stays Tool.
        return Err("Request exceeds 1 MiB".to_string().into());
    }

    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], descriptor.port)),
        timeout,
    )
    .map_err(CallError::infra)?;
    // One request line in, one reply line out: Nagle + the GUI's delayed ACK
    // would hold every reply's tail segment for ~200 ms per round. Matches
    // the GUI-side set_nodelay on the accepted stream (control/transport).
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(timeout))
        .map_err(CallError::infra)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(CallError::infra)?;
    stream.write_all(&wire).map_err(CallError::infra)?;

    let mut response = String::new();
    BufReader::new(stream)
        .take(MAX_RESPONSE + 1)
        .read_to_string(&mut response)
        .map_err(CallError::infra)?;
    if response.is_empty() || response.len() as u64 > MAX_RESPONSE {
        return Err(CallError::infra(
            "No valid OCS response; query request_id before retrying a mutation",
        ));
    }
    serde_json::from_str(response.trim_end()).map_err(CallError::infra)
}
