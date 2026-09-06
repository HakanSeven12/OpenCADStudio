//! In-process transport (plan §7): drives a `DocApiBackend` directly, inline.
//! Used by in-tree/built-in plugins and host tests. Thread-safe via a mutex so
//! many handles across many threads share it (calls are serialized).

use std::sync::Mutex;

use crate::backend::DocApiBackend;
use crate::envelope::{DocApiEnvelope, EnvelopeBody, Receipt};
use crate::error::ApiResult;
use crate::executor;
use crate::transport::Transport;

/// In-process transport: holds the backend behind a mutex (reentrant `&self`).
pub struct InProcess<B: DocApiBackend> {
    backend: Mutex<B>,
}

impl<B: DocApiBackend> InProcess<B> {
    pub fn new(backend: B) -> Self {
        Self { backend: Mutex::new(backend) }
    }
    /// Borrow the backend immutably (test/introspection helper).
    pub fn backend(&self) -> std::sync::MutexGuard<'_, B> {
        self.backend.lock().expect("in-process backend poisoned")
    }
}

impl<B: DocApiBackend + Send> Transport for InProcess<B> {
    fn apply(&self, req: DocApiEnvelope) -> ApiResult<Receipt> {
        let mut backend = self.backend.lock().map_err(|_| {
            crate::error::ApiError::Transport("in-process backend poisoned".into())
        })?;
        match req.body {
            EnvelopeBody::Op(op) => executor::apply_op(&mut *backend, op),
            EnvelopeBody::Queries(queries) => executor::apply_queries(&mut *backend, queries),
        }
    }
}
