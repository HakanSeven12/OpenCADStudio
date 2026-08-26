//! Inter-process communication layer for out-of-process plugins.
//!
//! Built only with the `host` feature because it needs `acadrust`-typed
//! messages and the plugin runner binary.
//!
//! Submodules:
//!
//! - [`protocol`](crate::ipc::protocol) — V2/V3 request/response enums and the
//!   initial runner handshake.
//! - [`transport`](crate::ipc::transport) — length-framed bincode over a local
//!   socket; enforces `MAX_MESSAGE_SIZE`.
//! - [`client`](crate::ipc::client) — plugin-side V2/V3 client and `HostApi`
//!   proxy.
//! - [`server`](crate::ipc::server) — host-side handler that applies a
//!   `PluginRequest` to a `HostApi` implementation.
//! - [`v4`](crate::ipc::v4) — multiplexed V4 frames, notifications, and
//!   correlation ids.
//! - [`proxy`](crate::ipc::proxy) — optional TCP proxy so plugin child
//!   processes can forward requests to the host.

#[cfg(feature = "host")]
pub mod client;
#[cfg(feature = "host")]
pub mod protocol;
#[cfg(feature = "host")]
pub mod server;
#[cfg(feature = "host")]
pub mod transport;
#[cfg(feature = "host")]
pub mod v4;
#[cfg(feature = "host")]
pub mod proxy;

#[cfg(all(test, feature = "host"))]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    use interprocess::local_socket::{
        traits::{Listener, Stream as StreamTrait},
        GenericNamespaced, ListenerOptions, Stream, ToNsName,
    };

    use crate::ipc::protocol::{HostRequest, HostResponse, HostToPlugin, PluginToHost};
    use crate::ipc::transport::{recv, send};

    fn unique_socket_name() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("ocs_plugin_test_{}_{}", std::process::id(), n)
    }

    fn connect_pair() -> (Stream, Stream) {
        let name = unique_socket_name();
        let name_ref = name
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("valid namespaced name");
        let listener = ListenerOptions::new()
            .name(name_ref)
            .create_sync()
            .expect("create listener");
        let client = thread::spawn(move || {
            StreamTrait::connect(name.to_ns_name::<GenericNamespaced>().unwrap()).expect("connect")
        });
        let server = listener.accept().expect("accept");
        let client = client.join().expect("client thread");
        (server, client)
    }

    #[test]
    fn transport_round_trips_host_request() {
        let (mut a, mut b) = connect_pair();
        let req = HostRequest::Dispatch {
            cmd: "LINE".to_string(),
        };
        send(&mut a, &HostToPlugin::Request(req)).unwrap();
        let got = recv::<HostToPlugin>(&mut b).unwrap();
        match got {
            HostToPlugin::Request(req) => match req {
                HostRequest::Dispatch { cmd } => assert_eq!(cmd, "LINE"),
                other => panic!("unexpected: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn transport_round_trips_plugin_request() {
        let (mut a, mut b) = connect_pair();
        let req = PluginToHost::Request(Box::new(crate::ipc::protocol::PluginRequest::PushInfo(
            "hello".to_string(),
        )));
        send(&mut a, &req).unwrap();
        let got = recv::<PluginToHost>(&mut b).unwrap();
        match got {
            PluginToHost::Request(req) => match *req {
                crate::ipc::protocol::PluginRequest::PushInfo(msg) => {
                    assert_eq!(msg, "hello")
                }
                other => panic!("unexpected: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn execute_code_variant_roundtrips_and_appends_to_host_request() {
        // Serialize ExecuteCode and verify it deserializes back. The fact that
        // existing variants still round-trip in v2_protocol_frame_roundtrips_after_v4_changes
        // confirms the new variant is appended at the end and does not shift
        // earlier discriminants.
        let req = HostRequest::ExecuteCode {
            command_id: 1,
            source: crate::host::CommandSource::Editor,
            code: "1+1".to_string(),
            tab_index: 0,
        };
        let bytes = bincode::serialize(&req).unwrap();
        let got: HostRequest = bincode::deserialize(&bytes).unwrap();
        match got {
            HostRequest::ExecuteCode {
                command_id,
                source,
                code,
                tab_index,
            } => {
                assert_eq!(command_id, 1);
                assert_eq!(source, crate::host::CommandSource::Editor);
                assert_eq!(code, "1+1");
                assert_eq!(tab_index, 0);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn code_execution_result_variant_roundtrips() {
        let result = crate::host::ExecutionResult {
            success: true,
            output: Some("ok".to_string()),
            error: None,
            error_type: None,
            traceback: None,
            line_number: None,
            column_number: None,
            duration_ms: 0.0,
        };
        let resp = HostResponse::CodeExecutionResult(result);
        let bytes = bincode::serialize(&resp).unwrap();
        let got: HostResponse = bincode::deserialize(&bytes).unwrap();
        match got {
            HostResponse::CodeExecutionResult(r) => {
                assert!(r.success);
                assert_eq!(r.output, Some("ok".to_string()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn v4_additions_preserve_legacy_discriminants() {
        fn discriminant<T: serde::Serialize>(value: &T) -> u32 {
            let bytes = bincode::serialize(value).unwrap();
            u32::from_le_bytes(bytes[..4].try_into().unwrap())
        }

        assert_eq!(discriminant(&HostRequest::Shutdown), 6);
        assert_eq!(discriminant(&HostResponse::Error(String::new())), 5);
        assert_eq!(
            discriminant(&crate::ipc::protocol::PluginRequest::BumpGeometry),
            6
        );
        assert_eq!(
            discriminant(&crate::ipc::protocol::PluginResponse::Record(None)),
            3
        );
    }

    #[test]
    fn transport_rejects_oversized_message() {
        let (mut a, _b) = connect_pair();
        // A Vec<u8> larger than MAX_MESSAGE_SIZE should be rejected on send.
        let huge = vec![0u8; 65 * 1024 * 1024];
        let err = send(&mut a, &huge).unwrap_err();
        assert!(format!("{err}").contains("too large"));
    }

    #[test]
    fn protocol_host_response_serde_roundtrip() {
        let resp = HostResponse::Text("pick a point".to_string());
        let bytes = bincode::serialize(&resp).unwrap();
        let got: HostResponse = bincode::deserialize(&bytes).unwrap();
        match got {
            HostResponse::Text(s) => assert_eq!(s, "pick a point"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn protocol_plugin_to_host_serde_roundtrip() {
        let msg = PluginToHost::Response(HostResponse::Bool(true));
        let bytes = bincode::serialize(&msg).unwrap();
        let got: PluginToHost = bincode::deserialize(&bytes).unwrap();
        match got {
            PluginToHost::Response(HostResponse::Bool(b)) => assert!(b),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn v2_protocol_frame_roundtrips_after_v4_changes() {
        // The V2/V3 wire format is unchanged by the V4 additions (new enum
        // variants are appended). Verify a full request/response pair still
        // serializes and deserializes correctly.
        let req = HostToPlugin::Request(HostRequest::Dispatch {
            cmd: "V2CMD".to_string(),
        });
        let bytes = bincode::serialize(&req).unwrap();
        let got: HostToPlugin = bincode::deserialize(&bytes).unwrap();
        match got {
            HostToPlugin::Request(req) => match req {
                HostRequest::Dispatch { cmd } => assert_eq!(cmd, "V2CMD"),
                other => panic!("unexpected: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }

        let resp = PluginToHost::Response(HostResponse::Bool(true));
        let bytes = bincode::serialize(&resp).unwrap();
        let got: PluginToHost = bincode::deserialize(&bytes).unwrap();
        match got {
            PluginToHost::Response(HostResponse::Bool(b)) => assert!(b),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
