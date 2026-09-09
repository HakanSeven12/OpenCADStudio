//! IPC transport: serializes `DocApiEnvelope` into the
//! append-only `PluginRequest::DocApiRequest { tab_id, bytes }` variant and ships
//! it through the plugin's `PluginRequestSender`. The host routes that variant to
//! the same crate executor (`ocs_doc_api::executor`) — one implementation.

use std::sync::Arc;

use ocs_plugin_api::host::{HostApi, PluginRequestError, PluginRequestSender};
use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};

use crate::envelope::{DocApiEnvelope, Receipt};
use crate::error::{ApiError, ApiResult};
use crate::facade::DocApi;
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

/// Build a typed [`DocApi`] from any host that can provide a
/// [`PluginRequestSender`] (out-of-process / V4 worker plugins).
///
/// Returns `None` for hosts that do not expose a request sender (e.g. in-process
/// hosts). In-process code should use [`crate::DocApi::in_process`] with a
/// concrete [`crate::backend::DocApiBackend`].
pub fn doc_api_for_host(host: &dyn HostApi) -> Option<DocApi> {
    let sender = host.plugin_request_sender()?;
    let tab_id = host.tab_id();
    Some(DocApi::connect(
        Arc::new(OcsPluginApiIpc::new(sender.into(), tab_id)),
        tab_id,
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facade::HasId;
    use crate::ObjectId;
    use ocs_plugin_api::host::{
        CadDocument, EntityType, ExtendedDataRecord, Handle, HostApi,
    };
    use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
    use std::any::Any;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockSender {
        captured: Arc<Mutex<Vec<(u64, PluginRequest)>>>,
    }

    impl PluginRequestSender for MockSender {
        fn request(&self, req: PluginRequest) -> Result<PluginResponse, PluginRequestError> {
            let tab_id = match &req {
                PluginRequest::DocApiRequest { tab_id, .. } => *tab_id,
                _ => 0,
            };
            self.captured.lock().unwrap().push((tab_id, req));
            Ok(PluginResponse::DocApiResponse {
                bytes: bincode::serialize(&crate::ApiResult::Ok(Receipt {
                    outcome: Some(crate::envelope::OpOutcome::NewId(ObjectId::from_u64(7))),
                    query_results: vec![],
                    new_revision: crate::revision::GeometryRevision(1),
                }))
                .unwrap(),
            })
        }
    }

    struct MockHost {
        sender: Option<MockSender>,
    }

    impl HostApi for MockHost {
        fn tab_index(&self) -> usize {
            0
        }
        fn document(&self) -> &CadDocument {
            unimplemented!()
        }
        fn document_mut(&mut self) -> &mut CadDocument {
            unimplemented!()
        }
        fn add_entity(&mut self, _entity: EntityType) -> Handle {
            unimplemented!()
        }
        fn bump_geometry(&mut self) {}
        fn push_undo(&mut self, _label: &str) {}
        fn set_dirty(&mut self) {}
        fn push_info(&mut self, _msg: &str) {}
        fn push_output(&mut self, _msg: &str) {}
        fn push_error(&mut self, _msg: &str) {}
        fn start_interactive(&mut self, _command: Box<dyn ocs_plugin_api::host::InteractiveCommand>) {}
        fn read_record(&self, _handle: Handle, _app_name: &str) -> Option<&ExtendedDataRecord> {
            None
        }
        fn write_record(&mut self, _handle: Handle, _record: ExtendedDataRecord) -> bool {
            false
        }
        fn remove_record(&mut self, _handle: Handle, _app_name: &str) -> bool {
            false
        }
        fn plugin_state_any(&self, _plugin_id: &str) -> Option<&(dyn Any + Send + Sync)> {
            None
        }
        fn plugin_state_any_mut(&mut self, _plugin_id: &str) -> Option<&mut (dyn Any + Send + Sync)> {
            None
        }
        fn ensure_plugin_state_any(
            &mut self,
            _plugin_id: &'static str,
            _init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
        ) -> &mut (dyn Any + Send + Sync) {
            unimplemented!()
        }
        fn document_reader(&self) -> Box<dyn ocs_plugin_api::host::DocumentReader + '_> {
            unimplemented!()
        }
        fn plugin_request_sender(&self) -> Option<Box<dyn PluginRequestSender>> {
            self.sender.as_ref().map(|s| Box::new(s.clone()) as Box<dyn PluginRequestSender>)
        }
    }

    #[test]
    fn doc_api_for_host_builds_doc_api_and_routes_doc_api_request() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let host = MockHost {
            sender: Some(MockSender {
                captured: Arc::clone(&captured),
            }),
        };
        let doc_api = doc_api_for_host(&host).expect("host exposes a sender");
        let doc = doc_api.document(host.tab_id());
        let id = doc
            .solids()
            .create_cuboid([0.0; 3], [1.0; 3])
            .expect("mock returns a NewId receipt");
        assert_eq!(id.id(), ObjectId::from_u64(7));

        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (tab_id, ref req) = &calls[0];
        assert_eq!(*tab_id, host.tab_id());
        match req {
            PluginRequest::DocApiRequest { tab_id, bytes } => {
                assert_eq!(*tab_id, host.tab_id());
                assert!(!bytes.is_empty());
            }
            other => panic!("expected DocApiRequest, got {other:?}"),
        }
    }

    #[test]
    fn doc_api_for_host_returns_none_without_sender() {
        let host = MockHost { sender: None };
        assert!(doc_api_for_host(&host).is_none());
    }
}
