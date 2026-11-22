//! Length-framed transport over `interprocess::local_socket` streams.
//!
//! Messages are serialized with `bincode`, prefixed by a little-endian `u64`
//! length, and sent over the stream. The receiver parses the length, bounds it
//! against [`MAX_MESSAGE_SIZE`], then deserializes the payload.
//!
//! [`send`] and [`recv`] are synchronous and block until the full frame is read
//! or the peer disconnects. `Disconnected` is returned on clean EOF; `TooLarge`
//! rejects messages that exceed [`MAX_MESSAGE_SIZE`]. The payload buffer grows
//! with the bytes that arrive, so a header alone allocates little.

use std::io::{Read, Write};

use interprocess::local_socket::Stream;
use serde::{Serialize, de::DeserializeOwned};

/// Maximum serialized message size accepted over the wire (1 GiB). Prevents
/// a malicious or buggy peer from exhausting host/runner memory, while a full
/// document snapshot of a large drawing (about 90 MB) still fits.
const MAX_MESSAGE_SIZE: usize = 1024 * 1024 * 1024;

/// The payload buffer grows by at most this much ahead of the bytes read.
const READ_CHUNK: usize = 16 * 1024 * 1024;

/// Errors that can occur during transport.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Encode(#[from] bincode::Error),
    #[error("empty message")]
    Empty,
    #[error("message too large: {0} bytes")]
    TooLarge(usize),
    #[error("peer disconnected")]
    Disconnected,
}

/// Send a length-framed serialized message.
pub fn send<T: Serialize>(stream: &mut Stream, msg: &T) -> Result<(), TransportError> {
    let bytes = bincode::serialize(msg)?;
    if bytes.len() > MAX_MESSAGE_SIZE {
        return Err(TransportError::TooLarge(bytes.len()));
    }
    let len = bytes.len() as u64;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

/// Receive a length-framed serialized message.
pub fn recv<T: DeserializeOwned>(stream: &mut Stream) -> Result<T, TransportError> {
    let mut len_buf = [0u8; 8];
    if let Err(e) = stream.read_exact(&mut len_buf) {
        return if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Err(TransportError::Disconnected)
        } else {
            Err(TransportError::Io(e))
        };
    }
    let len = u64::from_le_bytes(len_buf) as usize;
    if len == 0 {
        return Err(TransportError::Empty);
    }
    if len > MAX_MESSAGE_SIZE {
        return Err(TransportError::TooLarge(len));
    }
    let mut buf = Vec::with_capacity(len.min(READ_CHUNK));
    match (&mut *stream).take(len as u64).read_to_end(&mut buf) {
        Ok(n) if n < len => return Err(TransportError::Disconnected),
        Ok(_) => {}
        Err(e) => return Err(TransportError::Io(e)),
    }
    // Recursive dialog payloads are bounded during decode (see
    // `crate::dialog`): reset the per-frame budget, then let the decoders
    // refuse deep/wide payloads instead of overflowing the stack.
    crate::dialog::dialog_decode_frame_begin();
    Ok(bincode::deserialize(&buf)?)
}
