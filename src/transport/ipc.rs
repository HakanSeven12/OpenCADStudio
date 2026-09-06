//! IPC transport: serializes `DocApiEnvelope` into the
//! append-only `PluginRequest::DocApiRequest { tab_id, bytes }` variant and ships
//! it through the plugin's `PluginRequestSender`. The host routes that variant to
//! the same crate executor (`ocs_doc_api::executor`) — one implementation.

use std::sync::Arc;

use ocs_plugin_api::host::{PluginRequestError, PluginRequestSender};
use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};

use crate::envelope::{DocApiEnvelope, Receipt};
use crate::error::{ApiError, ApiResult};
use crate::transport::Transport;

/// Out-of-process IPC transport over the `ocs_plugin_api` channel. `Send + Sync`;
/// the underlying `PluginRequestSender` already serializes + correlates (V4).
pub struct OcsPluginApiIpc {
    sender: Arc<dyn PluginRequestSender>,
    tab_id: u64,
}

impl OcsPluginApiIpc {
    pub fn new(sender: Arc<dyn PluginRequestSender>, tab_id: u64) -> Self {
        Self { sender, tab_id }
    }

    fn transport_err(e: PluginRequestError) -> ApiError {
        ApiError::Transport(e.0)
    }
}

impl Transport for OcsPluginApiIpc {
    fn apply(&self, req: DocApiEnvelope) -> ApiResult<Receipt> {
        let bytes = bincode::serialize(&req)
            .map_err(|e| ApiError::Transport(format!("envelope serialize: {e}")))?;
        let resp = self
            .sender
            .request(PluginRequest::DocApiRequest {
                tab_id: self.tab_id,
                bytes,
            })
            .map_err(Self::transport_err)?;
        match resp {
            PluginResponse::DocApiResponse { bytes } => {
                // The host serializes `ApiResult<Receipt>` so op/query errors
                // (validation, geometry, unknown id) surface as the SAME
                // structured `ApiError` the in-process executor produced.
                let result: ApiResult<Receipt> = bincode::deserialize(&bytes)
                    .map_err(|e| ApiError::Transport(format!("receipt deserialize: {e}")))?;
                result
            }
            PluginResponse::Error(msg) => Err(ApiError::Transport(msg)),
            other => Err(ApiError::Transport(format!(
                "unexpected DocApi response variant: {other:?}"
            ))),
        }
    }
}
