//! Host-side V4 IPC reader and dispatcher helpers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use interprocess::local_socket::Stream;

use crate::host::PluginNotification;
use crate::ipc::transport::{recv, TransportError};
use crate::ipc::v4::protocol::PluginToHostV4;

/// Default plugin→host notification rate limit (notifications per second).
const DEFAULT_NOTIFY_RATE_LIMIT: f64 = 1000.0;

/// Minimum interval between verbose drop logs to avoid spam.
const DROP_LOG_INTERVAL: Duration = Duration::from_secs(1);

macro_rules! vlog {
    ($($arg:tt)*) => {{
        if crate::process::verbose() {
            eprintln!($($arg)*);
        }
    }};
}

/// A frame delivered from the V4 reader thread to the host main thread.
pub enum HostIncoming {
    Response {
        id: u64,
        payload: crate::ipc::protocol::HostResponse,
    },
    Request {
        id: u64,
        tab_id: Option<u64>,
        payload: Box<crate::ipc::protocol::PluginRequest>,
    },
}

/// Token-bucket rate limiter.
pub struct RateLimiter {
    rate: f64,
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(rate_per_second: f64) -> Self {
        Self {
            rate: rate_per_second.max(0.0),
            tokens: rate_per_second,
            last: Instant::now(),
        }
    }

    pub fn allow(&mut self) -> bool {
        if self.rate <= 0.0 {
            return false;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.rate);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub fn default_notify_rate_limit() -> f64 {
    std::env::var("OCS_PLUGIN_NOTIFY_RATE_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(|n: f64| n.max(0.0))
        .unwrap_or(DEFAULT_NOTIFY_RATE_LIMIT)
}

/// State shared between the host main thread and the V4 reader thread.
pub struct V4HostShared {
    pub writer: Mutex<Option<Stream>>,
    pub alive: AtomicBool,
}

/// Run the host-side V4 reader thread until the socket closes or a fatal error
/// occurs. Responses and requests are forwarded to `incoming`; notifications
/// are dispatched immediately to `handler` subject to `rate_limiter`.
pub fn run_host_reader_thread(
    mut reader: Stream,
    shared: Arc<V4HostShared>,
    incoming: mpsc::Sender<HostIncoming>,
    handler: Arc<dyn Fn(Option<u64>, PluginNotification) + Send + Sync>,
    mut rate_limiter: RateLimiter,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut last_drop_log: Option<Instant> = None;
        loop {
            match recv::<PluginToHostV4>(&mut reader) {
                Ok(PluginToHostV4::Response { id, payload }) => {
                    if incoming
                        .send(HostIncoming::Response { id, payload })
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(PluginToHostV4::Notification(envelope)) => {
                    if rate_limiter.allow() {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            (handler)(envelope.command_id, envelope.payload)
                        }));
                    } else {
                        let now = Instant::now();
                        let should_log = last_drop_log
                            .map(|t| now.duration_since(t) >= DROP_LOG_INTERVAL)
                            .unwrap_or(true);
                        if should_log && crate::process::verbose() {
                            eprintln!(
                                "[plugin] plugin→host notification rate limit exceeded; dropping"
                            );
                            last_drop_log = Some(now);
                        }
                    }
                }
                Ok(PluginToHostV4::Request {
                    id,
                    tab_id,
                    payload,
                }) => {
                    if incoming
                        .send(HostIncoming::Request {
                            id,
                            tab_id,
                            payload: Box::new(payload),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(TransportError::Disconnected) => {
                    // Peer closed the connection; this is the normal shutdown
                    // path, so don't log it as an error.
                    break;
                }
                Err(e) => {
                    vlog!("[plugin] V4 host reader error: {e}");
                    break;
                }
            }
        }
    }));
    shared.alive.store(false, Ordering::SeqCst);
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_burst_then_throttles() {
        let mut limiter = RateLimiter::new(10.0);
        for _ in 0..10 {
            assert!(limiter.allow());
        }
        // Immediately after the burst we should be out of tokens.
        assert!(!limiter.allow());
    }

    #[test]
    fn rate_limiter_refills_over_time() {
        let mut limiter = RateLimiter::new(100.0);
        assert!(limiter.allow());
        std::thread::sleep(Duration::from_millis(20));
        assert!(limiter.allow());
    }
}
