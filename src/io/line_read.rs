//! Bounded line reads for the JSON-lines wire protocols (`--serve`, `--mcp`).
//!
//! `BufRead::read_line` — and the `.lines()` iterator built on it — grow
//! their `String` by doubling with no ceiling, so a peer streaming one line
//! without a newline pushes the allocation until the process dies: an
//! allocation failure is an *abort*, not a catchable panic. The REST
//! transport learned this the hard way (`src/rest.rs`); this module is the
//! shared ceiling for every other line-oriented channel.
//!
//! 64 MiB is generous enough that no legitimate message (base64 images
//! included) is refused, small enough that memory stays bounded.

use std::io::{BufRead, Read};

/// Upper bound for one protocol line, newline included.
pub const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

/// Marker for a line that exceeded its size cap. Kept as a distinct error
/// payload so callers can tell "peer blew the cap" apart from transport
/// errors (reset, timeout) if they ever need to answer differently.
#[derive(Debug)]
pub struct LineTooLong;

impl std::fmt::Display for LineTooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("line exceeds the size limit")
    }
}

impl std::error::Error for LineTooLong {}

/// True for the cap violation [`read_line_capped`] raises.
pub fn is_line_too_long(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<LineTooLong>())
}

/// `BufRead::read_line` capped at `cap` bytes — the whole line including its
/// newline. The underlying read stops at `cap + 1` bytes, so memory stays
/// bounded no matter how much the peer streams; an over-cap line fails with
/// [`LineTooLong`] instead of growing `buf` forever.
pub fn read_line_capped(
    reader: &mut impl BufRead,
    buf: &mut String,
    cap: usize,
) -> std::io::Result<usize> {
    buf.clear();
    let read = reader.by_ref().take(cap as u64 + 1).read_line(buf)?;
    if read > cap {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            LineTooLong,
        ));
    }
    Ok(read)
}

/// Iterator over [`read_line_capped`] results, trimming line endings exactly
/// like [`BufRead::lines`]. Yields `Err` — and then stops — for the first
/// over-cap line, so callers keep their existing read-error handling.
pub struct CappedLines<R> {
    reader: R,
    cap: usize,
    done: bool,
}

pub fn lines_capped<R: BufRead>(reader: R, cap: usize) -> CappedLines<R> {
    CappedLines {
        reader,
        cap,
        done: false,
    }
}

impl<R: BufRead> Iterator for CappedLines<R> {
    type Item = std::io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut buf = String::new();
        match read_line_capped(&mut self.reader, &mut buf, self.cap) {
            Ok(0) => {
                self.done = true;
                None
            }
            Ok(_) => {
                if buf.ends_with('\n') {
                    buf.pop();
                    if buf.ends_with('\r') {
                        buf.pop();
                    }
                }
                Some(Ok(buf))
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}

/// Read adapter enforcing a single wall-clock deadline across all `read()` calls
/// on a `TcpStream`. Defeats slow-loris / trickle peers that reset per-read
/// `SO_RCVTIMEO` by dripping one byte every few seconds.
#[cfg(not(target_arch = "wasm32"))]
pub struct DeadlineStream {
    stream: std::net::TcpStream,
    deadline: std::time::Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl DeadlineStream {
    pub fn new(stream: std::net::TcpStream, deadline: std::time::Instant) -> Self {
        Self { stream, deadline }
    }

    pub fn set_deadline(&mut self, deadline: std::time::Instant) {
        self.deadline = deadline;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Read for DeadlineStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let now = std::time::Instant::now();
        if now >= self.deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "read deadline exceeded",
            ));
        }
        let remaining = self
            .deadline
            .saturating_duration_since(now)
            .max(std::time::Duration::from_millis(1));
        let _ = self.stream.set_read_timeout(Some(remaining));
        self.stream.read(buf)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::io::{BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    #[test]
    fn deadline_stream_aborts_slow_trickle_peer() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let writer = std::thread::spawn(move || {
            let mut client = TcpStream::connect(addr).unwrap();
            for _ in 0..20 {
                if client.write_all(b"a").is_err() || client.flush().is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        });
        let (server, _) = listener.accept().unwrap();
        let deadline = Instant::now() + Duration::from_millis(150);
        let mut reader = BufReader::new(DeadlineStream::new(server, deadline));
        let mut line = String::new();
        let start = Instant::now();
        let err = read_line_capped(&mut reader, &mut line, 4096)
            .expect_err("slow trickle must time out before completion");
        assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ),
            "expected timeout, got {err:?}"
        );
        assert!(
            start.elapsed() < Duration::from_millis(600),
            "took too long: {:?}",
            start.elapsed()
        );
        drop(reader);
        let _ = writer.join();
    }
}
