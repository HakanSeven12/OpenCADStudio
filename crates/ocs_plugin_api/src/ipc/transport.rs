//! Length-framed transport over `interprocess::local_socket` streams.
//!
//! Messages are serialized with `bincode`, prefixed by a little-endian `u64`
//! length, and sent over the stream. The receiver parses the length, bounds it
//! against [`MAX_MESSAGE_SIZE`], then deserializes the payload.
//!
//! [`send`] and [`recv`] are synchronous and block until the full frame is read
//! or the peer disconnects. `Disconnected` is returned on clean EOF; `TooLarge`
//! rejects messages that exceed 64 MiB to protect host/runner memory.

use std::io::{Read, Write};

use interprocess::local_socket::Stream;
use serde::{de::DeserializeOwned, Serialize};

/// Maximum serialized message size accepted over the wire (64 MiB). Prevents
/// a malicious or buggy peer from exhausting host/runner memory.
const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

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
    let mut buf = vec![0u8; len];
    if let Err(e) = stream.read_exact(&mut buf) {
        return if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Err(TransportError::Disconnected)
        } else {
            Err(TransportError::Io(e))
        };
    }
    Ok(bincode::deserialize(&buf)?)
}
