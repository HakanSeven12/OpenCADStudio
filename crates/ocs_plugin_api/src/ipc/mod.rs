//! Inter-process communication layer for out-of-process plugins.
//!
//! Built only with the `host` feature because it needs `document`-typed
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
pub mod proxy;
#[cfg(feature = "host")]
pub mod server;
#[cfg(feature = "host")]
pub mod transport;
#[cfg(feature = "host")]
pub mod v4;

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
    fn registered_manifest_roundtrips_entity_classes_over_bincode() {
        // The registration handshake is length-framed bincode, so the manifest
        // and its API-v10 entity-class declaration must survive exactly that
        // wire format.
        let manifest = crate::ribbon::owned::OwnedPluginManifest {
            id: "opencad.civil_ifc".to_string(),
            name: "Civil IFC".to_string(),
            version: "0.1.0".to_string(),
            description: "test".to_string(),
            api_version: 10,
            ribbon_order: 50,
            xdata_apps: vec!["CVIFC".to_string()],
            command_prefixes: vec![],
            entity_classes: Some(crate::ribbon::owned::OwnedEntityClasses {
                app_id: "CVIFC".to_string(),
                class_prefix: "CVIFC-CLASS:".to_string(),
                labels: [
                    ("LINE".to_string(), "Linear Element".to_string()),
                    (
                        "CIRCULARARC".to_string(),
                        "Circular Arc Element".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            }),
        };
        let resp = HostResponse::Manifest(manifest);
        let bytes = bincode::serialize(&resp).unwrap();
        let got: HostResponse = bincode::deserialize(&bytes).unwrap();
        match got {
            HostResponse::Manifest(m) => {
                let classes = m.entity_classes.expect("declaration survives");
                assert_eq!(classes.app_id, "CVIFC");
                assert_eq!(classes.class_prefix, "CVIFC-CLASS:");
                assert_eq!(
                    classes.labels.get("LINE").map(String::as_str),
                    Some("Linear Element")
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn manifest_entity_classes_default_when_absent() {
        // A manifest serialized before the field existed (an older runner)
        // decodes with the declaration defaulting to `None`.
        let older = r#"{
            "id": "old.plugin",
            "name": "Old",
            "version": "0.1.0",
            "description": "test",
            "api_version": 9,
            "ribbon_order": 0,
            "xdata_apps": [],
            "command_prefixes": []
        }"#;
        let m: crate::ribbon::owned::OwnedPluginManifest =
            serde_json::from_str(older).expect("older manifest decodes");
        assert_eq!(m.id, "old.plugin");
        assert!(m.entity_classes.is_none());
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
        // API v11 additions sit at the ENDS of their enums; the legacy
        // discriminants pinned above are unchanged. Pin the new tails too so
        // a future variant inserted mid-enum fails here.
        assert_eq!(
            discriminant(&HostRequest::CustomGripEvent {
                tab_index: 0,
                event: crate::host::CustomGripEvent {
                    grip_id: 7,
                    phase: crate::host::CustomGripPhase::Settle,
                    point: [1.0, 2.0, 3.0],
                    anchor: None,
                }
            }),
            10
        );
        assert_eq!(
            discriminant(&HostResponse::CustomGrip(crate::host::CustomGripResponse {
                preview: Vec::new(),
                live_entities: Default::default(),
                descriptor: Default::default(),
                descriptor_tolerance: Default::default(),
            })),
            8
        );
        assert_eq!(
            discriminant(&crate::ipc::protocol::PluginRequest::SetPluginGrips {
                grips: Vec::new(),
            }),
            39
        );
        assert_eq!(
            discriminant(&crate::ipc::protocol::PluginResponse::SetPluginGripsResult(
                Ok(())
            )),
            28
        );
        // API v12 additions stay at the ends (cheaper per-entity reads).
        assert_eq!(
            discriminant(&crate::ipc::protocol::PluginRequest::GetEntitiesBatch {
                handles: Vec::new(),
            }),
            40
        );
        assert_eq!(
            discriminant(
                &crate::ipc::protocol::PluginResponse::GetEntitiesBatchResult(Ok(Vec::new()))
            ),
            29
        );
    }

    #[test]
    fn v24_scene_wire_tails_are_pinned() {
        // Same discipline as `v4_additions_preserve_legacy_discriminants`
        // above: the v24 crossing appends at the ENDS of every public enum,
        // and these creation-time pins make a future insertion mid-enum
        // fail loudly. Tag numbers: the contract review pack named the new
        // request tail "61+". The crate had already appended v22's geo tail
        // AFTER the pinned PanelSnapshot=60 (SetGeoLocation=61 /
        // RemoveGeoLocation=62), so the TRUE append positions — measured,
        // pinned here — are 63..=66 for requests and 49..=51 for responses
        // (GeoLocationSet=47 / GeoLocationRemoved=48); `HostNotification`
        // appends exactly at 15..=16 as the pack names. The append-only law
        // (nothing shifts) is what carries; re-numbering would move legacy
        // tags, which this crossing must not do.
        fn discriminant<T: serde::Serialize>(value: &T) -> u32 {
            let bytes = bincode::serialize(value).unwrap();
            u32::from_le_bytes(bytes[..4].try_into().unwrap())
        }

        use crate::ipc::protocol::{PluginRequest, PluginResponse, SceneOrWidgetPatch, SceneOrWidgetSurface};

        // Requests (the append tail; after the v22 geo additions):
        let publish = PluginRequest::PanelScenePublish {
            panel_id: "p".to_owned(),
            surface: SceneOrWidgetSurface::Scene(crate::scene::PanelScene {
                revision: crate::scene::SceneRevision(1),
                elements: Vec::new(),
            }),
            events: Vec::new(),
        };
        assert_eq!(discriminant(&publish), 63);
        assert_eq!(
            discriminant(&PluginRequest::PanelScenePatch {
                panel_id: "p".to_owned(),
                patch: SceneOrWidgetPatch::Widgets(crate::widgets::WidgetSurfacePatch {
                    base_revision: crate::scene::SceneRevision(1),
                    revision: crate::scene::SceneRevision(2),
                    ops: Vec::new(),
                }),
            }),
            64
        );
        assert_eq!(
            discriminant(&PluginRequest::PanelStoreRead {
                panel_id: "p".to_owned(),
                kind: crate::ipc::protocol::PanelStoreKind::TreeChildren,
                parent_id: None,
                range: crate::widgets::WidgetRange { start: 0, len: 1 },
            }),
            65
        );
        assert_eq!(
            discriminant(&PluginRequest::PanelSceneSnapshot2 {
                panel_id: "p".to_owned(),
            }),
            66
        );
        // The v22 surface stays byte-exact: geo tail 62 (dependency-free
        // unit variant; its SetGeoLocation predecessor is the adjacent 61)
        // and the earlier pinned PanelSnapshot 60.
        assert_eq!(discriminant(&PluginRequest::RemoveGeoLocation), 62);
        assert_eq!(
            discriminant(&PluginRequest::PanelSnapshot {
                panel_id: "p".to_owned(),
            }),
            60
        );

        // Responses (the append tail; after the v22 geo additions):
        let (report_scene, report_code, report_page) = {
            use crate::ipc::protocol::{PatchRejectCode, PatchRejectReport};
            let mut scene = crate::scene::PanelScene {
                revision: crate::scene::SceneRevision(1),
                elements: vec![crate::scene::SceneElement::Rect {
                    id: crate::scene::StableId(1),
                    name: None,
                    actions: Vec::new(),
                    drag: None,
                    pos: crate::scene::Vec2([0.0, 0.0]),
                    size: crate::scene::Vec2([1.0, 1.0]),
                    fill: None,
                    stroke: None,
                }],
            };
            let report = scene
                .apply_patch_report(crate::scene::PanelScenePatch {
                    base_revision: crate::scene::SceneRevision(1),
                    revision: crate::scene::SceneRevision(2),
                    ops: Vec::new(),
                })
                .expect("empty patch advances the revision");
            (
                report,
                PatchRejectCode::StaleBase,
                PatchRejectReport {
                    current_revision: Some(crate::scene::SceneRevision(2)),
                    failed_op: None,
                },
            )
        };
        assert_eq!(discriminant(&PluginResponse::PatchApplied(report_scene)), 49);
        assert_eq!(
            discriminant(&PluginResponse::PatchRejected {
                code: report_code,
                report: report_page,
            }),
            50
        );
        assert_eq!(
            discriminant(&PluginResponse::StorePage {
                rows: vec![],
                total_hint: 0,
            }),
            51
        );
        // The v22 surface stays byte-exact: geo tail 48 (GeoLocationRemoved;
        // its GeoLocationSet predecessor is the adjacent 47) and the earlier
        // pinned PanelSnapshot 46.
        assert_eq!(
            discriminant(&PluginResponse::GeoLocationRemoved(Err("x".to_owned()))),
            48
        );
        assert_eq!(
            discriminant(&PluginResponse::PanelSnapshot(Err("unsupported".to_owned()))),
            46
        );

        // Notifications (15..=16): the manual tag byte, then a full
        // bincode roundtrip through the custom impls (both directions the
        // crate encodes).
        use crate::host::HostNotification;
        let events = HostNotification::PanelSceneEvents {
            panel_id: "p".to_owned(),
            surface_id: crate::scene::StableId(7),
            events: vec![crate::widgets::WidgetEvent::TreeExpanded {
                surface_id: crate::scene::StableId(7),
                node_id: crate::scene::StableId(2),
                expanded: true,
            }],
        };
        let bytes = bincode::serialize(&events).unwrap();
        // Serialized Vec<u8> = u64 length prefix + the tag as bytes[0].
        assert_eq!(bytes[8], 15);
        let back: HostNotification = bincode::deserialize(&bytes).unwrap();
        match back {
            HostNotification::PanelSceneEvents {
                panel_id,
                surface_id,
                events,
            } => {
                assert_eq!(panel_id, "p");
                assert_eq!(surface_id, crate::scene::StableId(7));
                assert_eq!(events.len(), 1);
            }
            other => panic!("unexpected: {other:?}"),
        }
        // The v22 digest tail stays byte-exact: tag 14.
        let digest = HostNotification::SelectionChangedDigest {
            tab_id: 1,
            revision: 2,
            count: 3,
        };
        let bytes = bincode::serialize(&digest).unwrap();
        assert_eq!(bytes[8], 14);
        let demand = HostNotification::PanelStoreDemand {
            panel_id: "p".to_owned(),
            surface_id: crate::scene::StableId(7),
            parent_id: Some(crate::scene::StableId(9)),
            range: crate::widgets::WidgetRange { start: 0, len: 4 },
        };
        let bytes = bincode::serialize(&demand).unwrap();
        assert_eq!(bytes[8], 16);
        let back: HostNotification = bincode::deserialize(&bytes).unwrap();
        match back {
            HostNotification::PanelStoreDemand {
                panel_id,
                surface_id,
                parent_id,
                range,
            } => {
                assert_eq!(panel_id, "p");
                assert_eq!(surface_id, crate::scene::StableId(7));
                assert_eq!(parent_id, Some(crate::scene::StableId(9)));
                assert_eq!(range, crate::widgets::WidgetRange { start: 0, len: 4 });
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn v11_grip_frames_bincode_roundtrip() {
        // The whole custom-grip surface over the wire: a declaration batch
        // and a settle event with its preview-wire answer.
        let grips = vec![crate::host::PluginGrip {
            grip_id: 42,
            anchor: Some(document::Handle::from(0x8A)),
            point: [12.5, -3.25, 0.0],
            xy_plane: false,
            axis: Some([
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
            ]),
            shape: crate::host::PluginGripShape::Triangle,
        }];
        let req = crate::ipc::protocol::PluginRequest::SetPluginGrips { grips };
        let bytes = bincode::serialize(&req).unwrap();
        let got: crate::ipc::protocol::PluginRequest = bincode::deserialize(&bytes).unwrap();
        match got {
            crate::ipc::protocol::PluginRequest::SetPluginGrips { grips } => {
                assert_eq!(grips.len(), 1);
                assert_eq!(grips[0].grip_id, 42);
                assert_eq!(grips[0].anchor, Some(document::Handle::from(0x8A)));
                assert_eq!(grips[0].shape, crate::host::PluginGripShape::Triangle);
            }
            other => panic!("unexpected: {other:?}"),
        }
        let resp = crate::ipc::protocol::PluginResponse::EntityGripsResult(Ok(vec![
            crate::host::GripInfo {
                grip_id: 2,
                point: [1.0, 2.0, 3.0],
                moves_entity: true,
                shape: "Square".to_string(),
                axis: None,
            },
        ]));
        let bytes = bincode::serialize(&resp).unwrap();
        let got: crate::ipc::protocol::PluginResponse = bincode::deserialize(&bytes).unwrap();
        match got {
            crate::ipc::protocol::PluginResponse::EntityGripsResult(Ok(infos)) => {
                assert_eq!(infos.len(), 1);
                assert_eq!(infos[0].shape, "Square");
                assert!(infos[0].moves_entity);
            }
            other => panic!("unexpected: {other:?}"),
        }
        let answer = HostResponse::CustomGrip(crate::host::CustomGripResponse {
            preview: vec![crate::host::PreviewWire::circle(
                [0.0; 3],
                5.0,
                Some([0.0, 1.0, 2.0, 3.5]),
            )],
            // API v12: the live-geometry channel rides the same response;
            // an old (v11) answer without it decodes default-empty above.
            live_entities: vec![(
                document::Handle::from(2),
                document::EntityType::Line(document::entities::Line::from_points(
                    document::types::Vector3::new(0.0, 0.0, 0.0),
                    document::types::Vector3::new(3.0, 0.0, 0.0),
                )),
            )],
            descriptor: None,
            descriptor_tolerance: None,
        });
        let bytes = bincode::serialize(&answer).unwrap();
        let got: HostResponse = bincode::deserialize(&bytes).unwrap();
        match got {
            HostResponse::CustomGrip(resp) => {
                assert_eq!(resp.preview.len(), 1);
                assert_eq!(resp.live_entities.len(), 1);
                assert_eq!(resp.live_entities[0].0, document::Handle::from(2));
            }
            other => panic!("unexpected: {other:?}"),
        }
        use crate::host::HostNotification;
        // API v13: the attach ack round-trips (a plugin->host notification).
        let ack = crate::host::PluginNotification::ChannelReady;
        let bytes = bincode::serialize(&ack).unwrap();
        let got: crate::host::PluginNotification = bincode::deserialize(&bytes).unwrap();
        assert_eq!(got, crate::host::PluginNotification::ChannelReady);
        // And the dirty-region payload (a host->plugin notification).
        let regions = HostNotification::DocumentChangedRegions {
            tab_id: 12,
            version: 93,
            entities: vec![document::Handle::from(0x2A)],
            records: vec![(document::Handle::from(0x8A), "CVIFC".to_string())],
        };
        let bytes = bincode::serialize(&regions).unwrap();
        let got: HostNotification = bincode::deserialize(&bytes).unwrap();
        match got {
            HostNotification::DocumentChangedRegions {
                tab_id,
                version,
                entities,
                records,
            } => {
                assert_eq!((tab_id, version), (12, 93));
                assert_eq!(entities, vec![document::Handle::from(0x2A)]);
                assert_eq!(
                    records,
                    vec![(document::Handle::from(0x8A), "CVIFC".to_string())]
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
        // And a v11-shaped answer (no live_entities, no descriptor) still
        // decodes: both later fields deserialize to their defaults.
        let old = HostResponse::CustomGrip(crate::host::CustomGripResponse {
            preview: Vec::new(),
            live_entities: Default::default(),
            descriptor: Default::default(),
            descriptor_tolerance: Default::default(),
        });
        let bytes = bincode::serialize(&old).unwrap();
        let got: HostResponse = bincode::deserialize(&bytes).unwrap();
        match got {
            HostResponse::CustomGrip(resp) => assert!(resp.live_entities.is_empty()),
            other => panic!("unexpected: {other:?}"),
        }
        // The per-entity read frames round-trip too (API v12).
        let req = crate::ipc::protocol::PluginRequest::GetEntitiesBatch {
            handles: vec![document::Handle::from(1), document::Handle::from(2)],
        };
        let bytes = bincode::serialize(&req).unwrap();
        let got: crate::ipc::protocol::PluginRequest = bincode::deserialize(&bytes).unwrap();
        match got {
            crate::ipc::protocol::PluginRequest::GetEntitiesBatch { handles } => {
                assert_eq!(handles.len(), 2);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn v11_grip_wire_shapes_default_tolerantly() {
        // Older/leaner senders omit optional fields: a bare-minimum
        // PluginGrip / GripInfo / CustomGripEvent / CustomGripResponse
        // decode with every defaulted field at its default.
        let minimal = crate::host::PluginGrip {
            grip_id: 1,
            anchor: None,
            point: [0.0; 3],
            xy_plane: false,
            axis: None,
            shape: Default::default(),
        };
        // Round-trips through JSON; and a leaner sender that omits every
        // defaulted field still decodes (the additive-tolerance discipline).
        let json = serde_json::to_value(&minimal).unwrap();
        let round: crate::host::PluginGrip = serde_json::from_value(json).unwrap();
        assert_eq!(round.grip_id, 1);
        assert_eq!(round.shape, crate::host::PluginGripShape::Square);
        let json = serde_json::json!({"grip_id": 1, "point": [0.0, 0.0, 0.0]});
        let missing: crate::host::PluginGrip = serde_json::from_value(json).unwrap();
        assert_eq!(missing.shape, crate::host::PluginGripShape::Square);
        assert!(missing.axis.is_none());
        assert!(missing.anchor.is_none());
        let missing: crate::host::GripInfo = serde_json::from_value(serde_json::json!({
            "grip_id": 0,
            "point": [0.0, 0.0, 0.0],
            "shape": "Square"
        }))
        .unwrap();
        assert!(!missing.moves_entity);
        assert!(missing.axis.is_none());
        let missing: crate::host::CustomGripResponse =
            serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(missing.preview.is_empty());
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
