//! V4 wire frames and envelope types.
//!
//! V4 replaces the simple request/response pipe with a multiplexed frame layer.
//! Every request and response carries a monotonically increasing correlation
//! `id` so a plugin can send/receive out of order. Notifications carry an
//! optional `command_id` so a plugin can associate a host notification with the
//! long-running command that caused it.
//!
//! The two frame enums are:
//!
//! - [`HostToPluginV4`] — host → runner requests, responses to runner requests,
//!   and host notifications.
//! - [`PluginToHostV4`] — runner → host requests, responses to host requests,
//!   and plugin notifications.
//!
//! Notifications are wrapped in [`NotificationEnvelope`] and are best-effort:
//! a process that fails to accept a notification is logged and skipped, but the
//! connection stays alive. Unknown host-notification discriminants deserialize
//! to [`crate::host::HostNotification::Unknown`] so newer host notifications do
//! not break older plugins.

use serde::{Deserialize, Serialize};

use crate::host::{HostNotification, PluginNotification};
use crate::ipc::protocol::{HostRequest, HostResponse, PluginRequest, PluginResponse};

/// Protocol version carried in the V4 handshake.
pub const V4_PROTOCOL_VERSION: u32 = 4;

/// Best-effort notification envelope carrying an optional command correlation
/// ID in addition to the typed payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEnvelope<T> {
    pub command_id: Option<u64>,
    pub payload: T,
}

/// Messages sent from the host to the plugin runner on the V4 socket.
#[derive(Debug, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum HostToPluginV4 {
    Request { id: u64, payload: HostRequest },
    Response { id: u64, payload: PluginResponse },
    Notification(NotificationEnvelope<HostNotification>),
}

/// Messages sent from the plugin runner to the host on the V4 socket.
#[derive(Debug, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum PluginToHostV4 {
    Request {
        id: u64,
        tab_id: Option<u64>,
        payload: PluginRequest,
    },
    Response { id: u64, payload: HostResponse },
    Notification(NotificationEnvelope<PluginNotification>),
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use super::*;
    use crate::host::LogLevel;

    #[test]
    fn v4_frame_roundtrip() {
        let frame = HostToPluginV4::Notification(NotificationEnvelope {
            command_id: Some(7),
            payload: HostNotification::InputLine {
                line: "hello".to_string(),
            },
        });
        let bytes = bincode::serialize(&frame).unwrap();
        let got: HostToPluginV4 = bincode::deserialize(&bytes).unwrap();
        match got {
            HostToPluginV4::Notification(env) => {
                assert_eq!(env.command_id, Some(7));
                assert_eq!(
                    env.payload,
                    HostNotification::InputLine {
                        line: "hello".to_string()
                    }
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn command_state_notification_roundtrips() {
        let notification = HostNotification::CommandStateChanged {
            tab_id: 42,
            command: Some("LINE".to_owned()),
        };
        let bytes = bincode::serialize(&notification).unwrap();
        let decoded: HostNotification = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, notification);
        let stopped = HostNotification::CommandStateChanged { tab_id: 42, command: None };
        let bytes = bincode::serialize(&stopped).unwrap();
        let decoded: HostNotification = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, stopped);
        let changed = HostNotification::DrawingChanged { tab_id: 42, epoch: 7 };
        let bytes = bincode::serialize(&changed).unwrap();
        let decoded: HostNotification = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, changed);
    }

    #[test]
    fn command_request_and_outcome_roundtrip() {
        use crate::host::{CommandOutcome, CommandRequest};
        for request in [
            CommandRequest::Run { line: "LINE 0,0 1,1".into() },
            CommandRequest::Start { name: "OFFSET".into() },
            CommandRequest::Point { point: [1.0, 2.0, 3.0] },
            CommandRequest::Text { text: "2".into() },
            CommandRequest::Token { text: "R".into() },
            CommandRequest::Entity { handle: codec::Handle::new(9), point: [0.0; 3] },
            CommandRequest::Selection,
            CommandRequest::Enter,
            CommandRequest::Cancel,
        ] {
            let bytes = bincode::serialize(&PluginRequest::RunCommand { request: request.clone() }).unwrap();
            assert!(matches!(bincode::deserialize::<PluginRequest>(&bytes).unwrap(),
                PluginRequest::RunCommand { request: decoded } if decoded == request));
        }
        let outcome = CommandOutcome {
            status: "waiting_input".into(),
            blocked_by: Some("command".into()),
            command: "OFFSET".into(),
            prompt: "Select object".into(),
            accepts: vec!["entity".into()],
            options: vec!["M".into()],
            entities: 4,
            added: -1,
            unconsumed: vec!["x".into()],
            error: Some("e".into()),
        };
        for result in [Ok(outcome), Err("refused".to_owned())] {
            let bytes = bincode::serialize(&PluginResponse::CommandResult(result.clone())).unwrap();
            assert!(matches!(bincode::deserialize::<PluginResponse>(&bytes).unwrap(),
                PluginResponse::CommandResult(decoded) if decoded == result));
        }
    }

    #[test]
    fn table_operation_request_and_response_roundtrip() {
        use crate::host::{LayerConfig, TableOperation};
        for operation in [
            TableOperation::LayerCreate {
                config: LayerConfig {
                    name: "Walls".into(),
                    color: Some(codec::types::Color::Index(5)),
                    frozen: Some(true),
                    description: Some("d".into()),
                    ..Default::default()
                },
            },
            TableOperation::LayerModify { config: LayerConfig { name: "Walls".into(), off: Some(true), ..Default::default() } },
            TableOperation::LayerRename { from: "A".into(), to: "B".into() },
            TableOperation::LayerDelete { name: "A".into(), erase_objects: true },
            TableOperation::LayerSetCurrent { name: "A".into() },
            TableOperation::TextStyleCreate {
                config: crate::host::TextStyleConfig { name: "T".into(), height: Some(2.5), oblique_angle: Some(0.2), backward: Some(true), ..Default::default() },
            },
            TableOperation::TextStyleModify { config: crate::host::TextStyleConfig { name: "T".into(), font_file: Some("romans".into()), ..Default::default() } },
            TableOperation::DimStyleCreate { name: "D".into(), copy_from: Some("Standard".into()), properties: "{\"dimscale\":2}".into() },
            TableOperation::DimStyleModify { name: "D".into(), properties: "{}".into() },
            TableOperation::StyleRename { kind: crate::host::TableStyleKind::Text, from: "A".into(), to: "B".into() },
            TableOperation::StyleDelete { kind: crate::host::TableStyleKind::Dim, name: "A".into() },
            TableOperation::StyleSetCurrent { kind: crate::host::TableStyleKind::Dim, name: "A".into() },
            TableOperation::BlockCreate {
                name: "B".into(),
                entities: vec![codec::Handle::new(4), codec::Handle::new(5)],
                base_point: [1.0, 2.0, 3.0],
                erase_originals: true,
                description: Some("d".into()),
            },
            TableOperation::BlockModify { name: "B".into(), description: None, explodable: Some(false), scale_uniformly: Some(true) },
            TableOperation::BlockRename { from: "B".into(), to: "C".into() },
            TableOperation::BlockDelete { name: "C".into() },
            TableOperation::BlockEntityAdd {
                block: "C".into(),
                entity: codec::EntityType::Line(codec::entities::Line::new()),
            },
            TableOperation::LinetypeCreate { name: "L".into(), description: "d".into(), pattern: vec![1.0, -1.0] },
            TableOperation::LinetypeModify { name: "L".into(), description: None, pattern: Some(vec![2.0, -2.0]) },
            TableOperation::LinetypeRename { from: "L".into(), to: "M".into() },
            TableOperation::LinetypeDelete { name: "M".into() },
            TableOperation::LayoutCreate { name: "S".into() },
            TableOperation::LayoutRename { from: "S".into(), to: "T".into() },
            TableOperation::LayoutDelete { name: "T".into() },
            TableOperation::LayoutSetCurrent { name: "Model".into() },
            TableOperation::LayoutSetPage { name: "S".into(), paper_size: Some([420.0, 297.0]), rotation: Some(90), scale: Some([1.0, 50.0]) },
        ] {
            let bytes = bincode::serialize(&PluginRequest::TableOperation { operation: operation.clone() }).unwrap();
            assert!(matches!(bincode::deserialize::<PluginRequest>(&bytes).unwrap(),
                PluginRequest::TableOperation { operation: decoded } if decoded == operation));
        }
        for result in [Ok(codec::Handle::new(9)), Err("refused".to_owned())] {
            let bytes = bincode::serialize(&PluginResponse::TableResult(result.clone())).unwrap();
            assert!(matches!(bincode::deserialize::<PluginResponse>(&bytes).unwrap(),
                PluginResponse::TableResult(decoded) if decoded == result));
        }
    }

    #[test]
    fn solid_operation_request_and_response_roundtrip() {
        use crate::host::{SolidOperation, SolidPrimitive};
        for operation in [
            SolidOperation::Create {
                primitive: SolidPrimitive::Pyramid { center: [1.0, 2.0, 3.0], radius: 4.0, height: 5.0, sides: 6 },
                layer: Some("SOLIDS".into()),
            },
            SolidOperation::Transform { handle: codec::Handle::new(7), matrix: [1.0; 16] },
            SolidOperation::RegionFromProfile { source: codec::Handle::new(3), layer: None, delete_source: true },
            SolidOperation::EmbedPicture { path: "p.png".into(), origin: [0.0; 3], width: 5.0, layer: None },
            SolidOperation::Boolean {
                first: codec::Handle::new(1),
                second: codec::Handle::new(2),
                operation: crate::host::SolidBoolean::Subtract,
                layer: None,
                keep_operands: false,
            },
            SolidOperation::SurfaceFromProfile { source: codec::Handle::new(4), layer: Some("S".into()), delete_source: false },
            SolidOperation::Extrude { source: codec::Handle::new(5), direction: [0.0, 0.0, 2.0], layer: None, delete_source: true },
        ] {
            let bytes = bincode::serialize(&PluginRequest::SolidOperation { operation: operation.clone() }).unwrap();
            assert!(matches!(bincode::deserialize::<PluginRequest>(&bytes).unwrap(),
                PluginRequest::SolidOperation { operation: decoded } if decoded == operation));
        }
        for result in [Ok(codec::Handle::new(9)), Err("refused".to_owned())] {
            let bytes = bincode::serialize(&PluginResponse::SolidResult(result.clone())).unwrap();
            assert!(matches!(bincode::deserialize::<PluginResponse>(&bytes).unwrap(),
                PluginResponse::SolidResult(decoded) if decoded == result));
        }
    }

    #[test]
    fn selection_request_and_response_roundtrip() {
        let request = PluginRequest::SetSelection { handles: vec![codec::Handle::new(9)] };
        let bytes = bincode::serialize(&request).unwrap();
        assert!(matches!(bincode::deserialize::<PluginRequest>(&bytes).unwrap(),
            PluginRequest::SetSelection { handles } if handles == vec![codec::Handle::new(9)]));
        let response = PluginResponse::Selection(vec![codec::Handle::new(9)]);
        let bytes = bincode::serialize(&response).unwrap();
        assert!(matches!(bincode::deserialize::<PluginResponse>(&bytes).unwrap(),
            PluginResponse::Selection(handles) if handles == vec![codec::Handle::new(9)]));
    }

    #[test]
    fn plugin_notification_roundtrip() {
        let frame = PluginToHostV4::Notification(NotificationEnvelope {
            command_id: None,
            payload: PluginNotification::Log {
                level: LogLevel::Info,
                text: "hi".to_string(),
            },
        });
        let bytes = bincode::serialize(&frame).unwrap();
        let got: PluginToHostV4 = bincode::deserialize(&bytes).unwrap();
        match got {
            PluginToHostV4::Notification(env) => {
                assert_eq!(env.command_id, None);
                assert_eq!(
                    env.payload,
                    PluginNotification::Log {
                        level: LogLevel::Info,
                        text: "hi".to_string()
                    }
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn unknown_notification_deserializes_as_unknown() {
        // Build a payload with an unknown discriminant (99) followed by some
        // bytes. The custom deserializer should return Unknown(raw_bytes).
        let mut payload = vec![99u8];
        bincode::serialize_into(&mut payload, &"future".to_string()).unwrap();
        let envelope = NotificationEnvelope {
            command_id: Some(1),
            payload: HostNotification::Unknown(payload.clone()),
        };
        let frame = HostToPluginV4::Notification(envelope);
        let bytes = bincode::serialize(&frame).unwrap();
        let got: HostToPluginV4 = bincode::deserialize(&bytes).unwrap();
        match got {
            HostToPluginV4::Notification(env) => match env.payload {
                HostNotification::Unknown(raw) => {
                    assert_eq!(raw[0], 99);
                }
                other => panic!("expected Unknown, got {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn v3_runner_handshake_token_roundtrips_after_tokenv4() {
        use crate::ipc::protocol::RunnerHandshake;
        let original = RunnerHandshake::Token("abc".to_string());
        let bytes = bincode::serialize(&original).unwrap();
        let got: RunnerHandshake = bincode::deserialize(&bytes).unwrap();
        match got {
            RunnerHandshake::Token(s) => assert_eq!(s, "abc"),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
