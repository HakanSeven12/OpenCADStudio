//! V4 full-duplex notification protocol for out-of-process plugins.
//!
//! V4 keeps the existing V3 request/response vocabulary but multiplexes
//! requests, responses, and best-effort notifications over a single local
//! socket. The V3 files (`src/ipc/{protocol,client,server}.rs`) are left
//! untouched; all V4 logic lives in this module.
//!
//! Submodules:
//!
//! - [`protocol`](crate::ipc::v4::protocol) — V4 wire frames, notification
//!   envelopes, and correlation ids.
//! - [`client`](crate::ipc::v4::client) — plugin-side V4 client loop and
//!   notification dispatch.
//! - [`server`](crate::ipc::v4::server) — host-side V4 connection and reader
//!   thread.

pub mod client;
pub mod protocol;
pub mod server;
