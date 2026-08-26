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
