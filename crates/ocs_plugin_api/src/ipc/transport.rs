//! Length-framed transport over `interprocess::local_socket` streams.

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
    stream.read_exact(&mut len_buf)?;
    let len = u64::from_le_bytes(len_buf) as usize;
    if len == 0 {
        return Err(TransportError::Empty);
    }
    if len > MAX_MESSAGE_SIZE {
        return Err(TransportError::TooLarge(len));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(bincode::deserialize(&buf)?)
}
