//! The transport boundary — the ONLY plugin↔host boundary.
//!
//! Portability contract (SPSC / flume / shared-memory safe): a transport impl
//! must (a) accept `&self` calls from many threads and serialize sends internally,
//! (b) demultiplex responses to the right caller by a correlation id, (c)
//! block-with-timeout, (d) expose liveness. This matches the existing V4 channel
//! shape, so swapping socket → shm changes only the impl, never the facade.

#[cfg(feature = "host")]
pub mod inproc;
#[cfg(feature = "ipc")]
pub mod ipc;

#[cfg(feature = "host")]
pub use inproc::InProcess;
#[cfg(feature = "ipc")]
pub use ipc::OcsPluginApiIpc;

use crate::envelope::{DocApiEnvelope, Receipt};
use crate::error::ApiResult;

/// The only plugin↔host boundary. Object-centric: one method per unit of work —
/// a single write op, or a query batch. `&self` + `Send + Sync` so many entity
/// handles across many threads share one transport concurrently.
pub trait Transport: Send + Sync {
    /// Apply `req` (ONE write op, or a query batch) and return its `Receipt`.
    /// A write request carries exactly one `Operation`; a read request carries
    /// one-or-more `Query` items (read-only batching is safe). Blocks up to an
    /// internal timeout. Reentrant: safe to call concurrently from many threads.
    fn apply(&self, req: DocApiEnvelope) -> ApiResult<Receipt>;

    /// Cheap host-liveness probe (shm has no socket-disconnect to detect a dead peer).
    fn alive(&self) -> bool {
        true
    }
}
