//! Host-side IPC request handler.

use crate::host::HostApi;
use crate::ipc::protocol::{PluginRequest, PluginResponse};

/// Per-variant batch budgets. Each width is far above legitimate use but
/// below pathological input, so a runaway batch fails fast with an honest
/// error instead of committing the host to full-document work (the frame size
/// envelope already bounds memory; these bound COUNT-dependent CPU passes).
const MAX_BATCH_ENTITIES: usize = 100_000;
const MAX_BATCH_HANDLES: usize = 1_000_000;
const MAX_XRECORD_BATCH: usize = 10_000;
const MAX_QUAD_PANELS: usize = 512;

fn batch_budget_error(name: &str, len: usize, cap: usize) -> PluginResponse {
    PluginResponse::Error(format!(
        "{name} batch of {len} exceeds the host budget ({cap})"
    ))
}

fn addressable_document_snapshot(host: &dyn HostApi) -> document::Document {
    let mut snapshot = host.document().clone();
    let attributes = host
        .document()
        .entities()
        .filter_map(|entity| match entity {
            document::EntityType::Insert(insert) => Some(insert.attributes.clone()),
            _ => None,
        })
        .flatten()
        .map(document::EntityType::AttributeEntity)
        .collect::<Vec<_>>();
    for attribute in attributes {
        // Attribute entities remain canonical children of INSERT in the host
        // document.  The IPC snapshot also indexes them by handle so plugin
        // document models can address a child exactly like any other entity.
        let _ = snapshot.add_entity(attribute);
    }
    snapshot
}

/// Apply one plugin request to the host's `HostApi` implementation.
///
/// `plugin_id` is the identity of the process the request came from — fixed
/// by the transport (the serving process's own manifest id), never carried on
/// the wire: the grip declaration stores the calling plugin's true id, so a
/// plugin cannot declare grips in another plugin's name.
///
/// `on_start_interactive` is called when the plugin starts an interactive
/// command; the host should install an adapter that sends
/// `HostRequest::InteractiveEvent` back to the plugin process.
///
/// `serve_commands` selects the serve context (API v12): in a GRIP-EVENT
/// delivery (the coalesced pump) nested command frames are refused — their
/// execution needs the host's message loop, which a UI-thread grip delivery
/// cannot pump (the v11.1 UI-freeze wedge). The plugin receives an honest
/// `Err` naming the constraint, so its settle callback proceeds data-plane
/// (push_undo + direct writes work inline as ever) or reports the boundary.
pub fn handle_plugin_request(
    host: &mut dyn HostApi,
    plugin_id: &str,
    req: PluginRequest,
    on_start_interactive: &mut dyn FnMut(u64),
    serve_commands: bool,
) -> PluginResponse {
    use PluginRequest::*;
    match req {
        PushInfo(msg) => {
            host.push_info(&msg);
            PluginResponse::Ok
        }
        PushOutput(msg) => {
            host.push_output(&msg);
            PluginResponse::Ok
        }
        PushError(msg) => {
            host.push_error(&msg);
            PluginResponse::Ok
        }
        AddEntity(entity) => PluginResponse::Handle(host.add_entity(entity)),
        AddEntities(entities) => {
            if entities.len() > MAX_BATCH_ENTITIES {
                batch_budget_error("add_entities", entities.len(), MAX_BATCH_ENTITIES)
            } else {
                PluginResponse::Handles(host.add_entities(entities))
            }
        }
        UpdateEntity(entity) => PluginResponse::Bool(host.update_entity(entity)),
        RemoveEntity { handle } => PluginResponse::Bool(host.remove_entity(handle)),
        BumpGeometry => {
            host.bump_geometry();
            PluginResponse::Ok
        }
        ReadRecord { handle, app_name } => {
            PluginResponse::Record(host.read_record(handle, &app_name).cloned())
        }
        WriteRecord { handle, record } => PluginResponse::Bool(host.write_record(handle, record)),
        RemoveRecord { handle, app_name } => {
            PluginResponse::Bool(host.remove_record(handle, &app_name))
        }
        PushUndo { label } => {
            host.push_undo(&label);
            PluginResponse::Ok
        }
        BeginUndoGroup { label } => {
            host.begin_undo_group(&label);
            PluginResponse::Ok
        }
        EndUndoGroup => {
            host.end_undo_group();
            PluginResponse::Ok
        }
        QueryEntities { query } => PluginResponse::EntityPage(host.query_entities(&query)),
        SetDirty => {
            host.set_dirty();
            PluginResponse::Ok
        }
        StartInteractive { command_id } => {
            on_start_interactive(command_id);
            PluginResponse::Ok
        }
        DocumentSnapshot => PluginResponse::Document(Box::new(addressable_document_snapshot(host))),
        OpenDocumentView => match host.document_view() {
            Some(info) => PluginResponse::DocumentView {
                path: info.path,
                version: info.version,
            },
            None => PluginResponse::Error("shared document view unavailable".to_string()),
        },
        OpenDocumentViewV4 { tab_id } => match host.document_view_v4(tab_id) {
            Some(info) => PluginResponse::DocumentViewV4 {
                path: info.path,
                version: info.version,
            },
            None => PluginResponse::Error("V4 shared document view unavailable".to_string()),
        },
        CloseDocumentViewV4 { tab_id } => {
            host.close_document_view_v4(tab_id);
            PluginResponse::Ok
        }
        GetTabId => PluginResponse::TabId(host.tab_id()),
        DocumentPath { tab_id } => PluginResponse::DocumentPath(
            host.document_path(tab_id).map(|path| path.into_os_string()),
        ),
        AddLayer(config) => PluginResponse::OptHandle(host.add_layer(config)),
        ModifyLayer(config) => PluginResponse::Bool(host.modify_layer(config)),
        ExecuteCommand(cmd) => {
            if !serve_commands {
                // API v12: a grip-event delivery refuses nested command
                // frames — their execution would need the host message loop
                // the blocked delivery cannot pump (UI-freeze wedge).
                PluginResponse::Error(format!(
                    "execute_command({cmd:?}) refused during a custom-grip drag: the host \
                     message loop cannot pump nested commands from a grip delivery; apply \
                     the edits directly in the settle callback (push_undo + update/add \
                     entities), or replay commands at your own boundary"
                ))
            } else {
                PluginResponse::Bool(host.execute_command(&cmd))
            }
        }
        GetSystemVariable { name } => PluginResponse::SystemVariable(host.system_variable(&name)),
        SetSystemVariable { name, value } => {
            PluginResponse::SystemVariableResult(host.set_system_variable(&name, value))
        }
        UpdateEntitiesTransaction { label, entities } => {
            if entities.len() > MAX_BATCH_ENTITIES {
                batch_budget_error("update_entities_transaction", entities.len(), MAX_BATCH_ENTITIES)
            } else {
                PluginResponse::EntityTransactionResult(
                    host.update_entities_transaction(&label, entities),
                )
            }
        }
        GetSelection => PluginResponse::Selection(host.selection()),
        SetSelection { handles } => {
            if handles.len() > MAX_BATCH_HANDLES {
                batch_budget_error("set_selection", handles.len(), MAX_BATCH_HANDLES)
            } else {
                PluginResponse::SelectionResult(host.set_selection(&handles))
            }
        }        SolidOperation { operation } => {
            PluginResponse::SolidResult(host.solid_operation(operation))
        }
        TableOperation { operation } => {
            PluginResponse::TableResult(host.table_operation(operation))
        }
        RunCommand { request } => {
            if !serve_commands {
                // API v12: same grip-event refusal as execute_command — a
                // nested run_command needs the iced loop; the plugin gets
                // the honest Err, not a hang.
                PluginResponse::CommandResult(Err(
                    "run_command refused during a custom-grip drag: the host message loop \
                     cannot pump nested commands from a grip delivery; apply the edits \
                     directly in the settle callback (push_undo + update/add entities), \
                     or replay commands at your own boundary"
                        .to_string(),
                ))
            } else {
                PluginResponse::CommandResult(host.run_command(request))
            }
        }
        CreateXRecord {
            dictionary_path,
            name,
            entries,
        } => {
            let path: Vec<&str> = dictionary_path.iter().map(String::as_str).collect();
            PluginResponse::XRecordCreateResult(host.create_xrecord(&path, &name, entries))
        }
        ReadXRecord {
            dictionary_path,
            name,
        } => {
            let path: Vec<&str> = dictionary_path.iter().map(String::as_str).collect();
            PluginResponse::XRecordEntries(host.read_xrecord(&path, &name))
        }
        UpdateXRecord {
            dictionary_path,
            name,
            entries,
        } => {
            let path: Vec<&str> = dictionary_path.iter().map(String::as_str).collect();
            PluginResponse::XRecordUpdateResult(host.update_xrecord(&path, &name, entries))
        }
        DeleteXRecord {
            dictionary_path,
            name,
        } => {
            let path: Vec<&str> = dictionary_path.iter().map(String::as_str).collect();
            PluginResponse::XRecordDeleteResult(host.delete_xrecord(&path, &name))
        }
        ListXRecords { dictionary_path } => {
            let path: Vec<&str> = dictionary_path.iter().map(String::as_str).collect();
            PluginResponse::XRecordNames(host.list_xrecords(&path))
        }
        SetReferenceOnly {
            handle,
            reference_only,
        } => {
            PluginResponse::SetReferenceOnlyResult(host.set_reference_only(handle, reference_only))
        }
        EntityGrips { handle } => PluginResponse::EntityGripsResult(host.entity_grips(handle)),
        GripDrag {
            handle,
            grip_id,
            to,
        } => PluginResponse::GripDragResult(host.grip_drag(handle, grip_id, to)),
        SetPluginGrips { grips } => {
            PluginResponse::SetPluginGripsResult(host.set_plugin_grips(plugin_id, grips))
        }
        GetEntitiesBatch { handles } => {
            if handles.len() > MAX_BATCH_HANDLES {
                PluginResponse::GetEntitiesBatchResult(Err(format!(
                    "get_entities_batch of {} handles exceeds the host budget ({MAX_BATCH_HANDLES})",
                    handles.len()
                )))
            } else {
                PluginResponse::GetEntitiesBatchResult(host.entities_batch(&handles))
            }
        }
        CreateConstraint { constraint } => {
            PluginResponse::CreateConstraintResult(host.constraint_create(constraint))
        }
        ReadXRecords { reads } => {
            if reads.len() > MAX_XRECORD_BATCH {
                PluginResponse::ReadXRecordsResult(Err(format!(
                    "read_xrecords batch of {} exceeds the host budget ({MAX_XRECORD_BATCH})",
                    reads.len()
                )))
            } else {
                PluginResponse::ReadXRecordsResult(host.read_xrecords(&reads))
            }
        }
        UpdateXRecords { updates } => {
            if updates.len() > MAX_XRECORD_BATCH {
                PluginResponse::UpdateXRecordsResult(Err(format!(
                    "update_xrecords batch of {} exceeds the host budget ({MAX_XRECORD_BATCH})",
                    updates.len()
                )))
            } else {
                PluginResponse::UpdateXRecordsResult(host.update_xrecords(&updates))
            }
        }
        ListXRecordsWithEntries { dictionary_path } => {
            let path: Vec<&str> = dictionary_path.iter().map(|s| s.as_str()).collect();
            PluginResponse::ListXRecordsWithEntriesResult(host.list_xrecords_with_entries(&path))
        }
        StageParametricChain {
            reference_name,
            chain,
            tolerance,
        } => PluginResponse::StageParametricChainResult(host.stage_parametric_chain(
            &reference_name,
            &chain,
            tolerance,
        )),
        SampleParametric { chain, s } => {
            PluginResponse::SampleParametricResult(host.sample_parametric(&chain, s))
        }
        ValidateParametricChain { chain, tolerance } => {
            PluginResponse::ValidateParametricChainResult(
                host.validate_parametric_chain(&chain, tolerance),
            )
        }
        RegisterPluginClass { class } => {
            PluginResponse::RegisterPluginClassResult(host.register_plugin_class(plugin_id, &class))
        }
        StagePluginClassInstance { instance } => PluginResponse::StagePluginClassInstanceResult(
            host.stage_plugin_class_instance(plugin_id, &instance),
        ),
        PluginClassInstanceProperties { handle } => {
            PluginResponse::PluginClassInstancePropertiesResult(
                host.plugin_class_instance_properties(handle),
            )
        }
        ShowDialog(dialog) => PluginResponse::DialogResult(if plugin_id.is_empty() {
            host.show_dialog(dialog)
        } else {
            host.show_dialog_for_plugin(plugin_id, dialog)
        }),
        // A transport with no identity keeps the unowned panel calls, as
        // ShowDialog does.
        RegisterPanel(panel) => PluginResponse::PanelRegistered(if plugin_id.is_empty() {
            host.register_panel(panel)
        } else {
            host.register_panel_for_plugin(plugin_id, panel)
        }),
        UpdatePanel { panel_id, values } => PluginResponse::PanelUpdated(if plugin_id.is_empty() {
            host.update_panel(&panel_id, values)
        } else {
            host.update_panel_for_plugin(plugin_id, &panel_id, values)
        }),
        TogglePanel { panel_id, show } => PluginResponse::PanelToggled(if plugin_id.is_empty() {
            host.toggle_panel(&panel_id, show)
        } else {
            host.toggle_panel_for_plugin(plugin_id, &panel_id, show)
        }),
        RegisterQuadPanels { panels } => {
            if panels.len() > MAX_QUAD_PANELS {
                return batch_budget_error(
                    "register_quad_panels",
                    panels.len(),
                    MAX_QUAD_PANELS,
                );
            }
            // API v15: the owning plugin's id is transport identity (see
            // handle_plugin_request's doc), so one plugin's registration can
            // never rewrite another plugin's panel. The outcome shape is one
            // entry per panel; refused entries get a short reason string (a
            // capable host reports its own specific reason on the command
            // line; the wire reason stays honest and short).
            let ok = host.register_quad_panels(plugin_id, &panels);
            let errors: Vec<String> = ok
                .iter()
                .map(|accepted| {
                    if *accepted {
                        String::new()
                    } else {
                        "quad panel refused".to_owned()
                    }
                })
                .collect();
            PluginResponse::QuadPanelsRegistered { ok, errors }
        }
        UnregisterQuadPanels { panel_ids } => {
            if panel_ids.len() > MAX_QUAD_PANELS {
                return batch_budget_error(
                    "unregister_quad_panels",
                    panel_ids.len(),
                    MAX_QUAD_PANELS,
                );
            }
            // API v15: the ownership law runs both ways — the transport's
            // plugin id scopes the removal exactly like RegisterQuadPanels
            // scopes the registration, so one plugin can never remove
            // another plugin's panel.
            let refs: Vec<&str> = panel_ids.iter().map(String::as_str).collect();
            let ok = host.unregister_quad_panels(plugin_id, &refs);
            let errors: Vec<String> = ok
                .iter()
                .map(|accepted| {
                    if *accepted {
                        String::new()
                    } else {
                        "quad panel not owned".to_owned()
                    }
                })
                .collect();
            PluginResponse::QuadPanelsRegistered { ok, errors }
        }
        PanelSnapshot { panel_id } => {
            PluginResponse::PanelSnapshot(host.panel_snapshot_for_plugin(plugin_id, &panel_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialog::{DialogValue, PanelDef, PanelSnapshot};
    use crate::host::{
        CadDocumentReader, Document, DocumentReader, EntityType, ExtendedDataRecord, Handle,
        InteractiveCommand,
    };
    use std::any::Any;
    use std::collections::HashMap;
    use std::sync::Mutex;

    const CALLER: &str = "example.actual_caller";
    const PANEL_ID: &str = "forged.plugin.id";
    const OWNER_ERROR: &str = "panel is not owned by caller";

    #[derive(Debug, PartialEq)]
    enum PanelCall {
        Register {
            plugin_id: String,
            panel_id: String,
        },
        Update {
            plugin_id: String,
            panel_id: String,
            values: HashMap<String, DialogValue>,
        },
        Toggle {
            plugin_id: String,
            panel_id: String,
            show: Option<bool>,
        },
        Snapshot {
            plugin_id: String,
            panel_id: String,
        },
        LegacyRegister {
            panel_id: String,
        },
        LegacyUpdate {
            panel_id: String,
            values: HashMap<String, DialogValue>,
        },
        LegacyToggle {
            panel_id: String,
            show: Option<bool>,
        },
    }

    #[derive(Default)]
    struct HostState {
        calls: Vec<PanelCall>,
        snapshot: Option<PanelSnapshot>,
        document_reads: usize,
        document_writes: usize,
    }

    struct RecordingHost {
        document: Document,
        state: Mutex<HostState>,
    }

    impl RecordingHost {
        fn new(snapshot: PanelSnapshot) -> Self {
            Self {
                document: Document::new(),
                state: Mutex::new(HostState {
                    snapshot: Some(snapshot),
                    ..HostState::default()
                }),
            }
        }
    }

    struct LegacyHost {
        document: Document,
        state: Mutex<HostState>,
    }

    impl Default for LegacyHost {
        fn default() -> Self {
            Self {
                document: Document::new(),
                state: Mutex::new(HostState::default()),
            }
        }
    }

    macro_rules! required_host_api_methods {
        () => {
            fn tab_index(&self) -> usize {
                0
            }

            fn document(&self) -> &Document {
                self.state.lock().unwrap().document_reads += 1;
                &self.document
            }

            fn document_mut(&mut self) -> &mut Document {
                self.state.get_mut().unwrap().document_writes += 1;
                &mut self.document
            }

            fn add_entity(&mut self, _entity: EntityType) -> Handle {
                self.state.get_mut().unwrap().document_writes += 1;
                Handle::default()
            }

            fn bump_geometry(&mut self) {}

            fn read_record(&self, _handle: Handle, _app_name: &str) -> Option<&ExtendedDataRecord> {
                self.state.lock().unwrap().document_reads += 1;
                None
            }

            fn write_record(&mut self, _handle: Handle, _record: ExtendedDataRecord) -> bool {
                self.state.get_mut().unwrap().document_writes += 1;
                false
            }

            fn remove_record(&mut self, _handle: Handle, _app_name: &str) -> bool {
                self.state.get_mut().unwrap().document_writes += 1;
                false
            }

            fn push_undo(&mut self, _label: &str) {}
            fn set_dirty(&mut self) {}
            fn push_info(&mut self, _msg: &str) {}
            fn push_output(&mut self, _msg: &str) {}
            fn push_error(&mut self, _msg: &str) {}
            fn start_interactive(&mut self, _command: Box<dyn InteractiveCommand>) {}

            fn plugin_state_any(&self, _plugin_id: &str) -> Option<&(dyn Any + Send + Sync)> {
                None
            }

            fn plugin_state_any_mut(
                &mut self,
                _plugin_id: &str,
            ) -> Option<&mut (dyn Any + Send + Sync)> {
                None
            }

            fn ensure_plugin_state_any(
                &mut self,
                _plugin_id: &'static str,
                _init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
            ) -> Option<&mut (dyn Any + Send + Sync)> {
                None
            }

            fn document_reader(&self) -> Box<dyn DocumentReader + '_> {
                self.state.lock().unwrap().document_reads += 1;
                Box::new(CadDocumentReader(&self.document))
            }
        };
    }

    impl HostApi for RecordingHost {
        required_host_api_methods!();

        fn register_panel_for_plugin(&mut self, plugin_id: &str, panel: PanelDef) -> bool {
            self.state.lock().unwrap().calls.push(PanelCall::Register {
                plugin_id: plugin_id.to_owned(),
                panel_id: panel.id,
            });
            true
        }

        fn update_panel_for_plugin(
            &mut self,
            plugin_id: &str,
            panel_id: &str,
            values: HashMap<String, DialogValue>,
        ) -> bool {
            self.state.lock().unwrap().calls.push(PanelCall::Update {
                plugin_id: plugin_id.to_owned(),
                panel_id: panel_id.to_owned(),
                values,
            });
            true
        }

        fn toggle_panel_for_plugin(
            &mut self,
            plugin_id: &str,
            panel_id: &str,
            show: Option<bool>,
        ) -> bool {
            self.state.lock().unwrap().calls.push(PanelCall::Toggle {
                plugin_id: plugin_id.to_owned(),
                panel_id: panel_id.to_owned(),
                show,
            });
            true
        }

        fn panel_snapshot_for_plugin(
            &self,
            plugin_id: &str,
            panel_id: &str,
        ) -> Result<PanelSnapshot, String> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(PanelCall::Snapshot {
                plugin_id: plugin_id.to_owned(),
                panel_id: panel_id.to_owned(),
            });
            let snapshot = state.snapshot.clone().expect("recording host snapshot");
            if plugin_id != snapshot.plugin_id.as_str() || panel_id != snapshot.panel_id.as_str() {
                Err(OWNER_ERROR.to_owned())
            } else {
                Ok(snapshot)
            }
        }
    }

    impl HostApi for LegacyHost {
        required_host_api_methods!();

        fn register_panel(&mut self, panel: PanelDef) -> bool {
            self.state
                .lock()
                .unwrap()
                .calls
                .push(PanelCall::LegacyRegister { panel_id: panel.id });
            true
        }

        fn update_panel(&mut self, panel_id: &str, values: HashMap<String, DialogValue>) -> bool {
            self.state
                .lock()
                .unwrap()
                .calls
                .push(PanelCall::LegacyUpdate {
                    panel_id: panel_id.to_owned(),
                    values,
                });
            true
        }

        fn toggle_panel(&mut self, panel_id: &str, show: Option<bool>) -> bool {
            self.state
                .lock()
                .unwrap()
                .calls
                .push(PanelCall::LegacyToggle {
                    panel_id: panel_id.to_owned(),
                    show,
                });
            true
        }
    }

    fn request(host: &mut dyn HostApi, plugin_id: &str, request: PluginRequest) -> PluginResponse {
        handle_plugin_request(
            host,
            plugin_id,
            request,
            &mut |_| panic!("unexpected interactive command"),
            true,
        )
    }

    #[test]
    fn panel_requests_use_caller_identity_and_protect_snapshot_read_back() {
        let values = HashMap::from([
            (
                "name".to_owned(),
                DialogValue::Text("  Bracket & <A>  ".to_owned()),
            ),
            ("quantity".to_owned(), DialogValue::Number(12.5)),
            ("enabled".to_owned(), DialogValue::Bool(true)),
        ]);
        let raw_text = HashMap::from([
            ("name".to_owned(), "  Bracket & <A>  ".to_owned()),
            ("quantity".to_owned(), " 12.500 ".to_owned()),
        ]);
        let mut host = RecordingHost::new(PanelSnapshot {
            panel_id: PANEL_ID.to_owned(),
            plugin_id: CALLER.to_owned(),
            tab_id: 73,
            values: values.clone(),
            raw_text: raw_text.clone(),
            errors: Vec::new(),
        });

        assert!(matches!(
            request(
                &mut host,
                CALLER,
                PluginRequest::RegisterPanel(PanelDef::new(PANEL_ID, "Forged identity"))
            ),
            PluginResponse::PanelRegistered(true)
        ));
        assert!(matches!(
            request(
                &mut host,
                CALLER,
                PluginRequest::UpdatePanel {
                    panel_id: PANEL_ID.to_owned(),
                    values: values.clone(),
                }
            ),
            PluginResponse::PanelUpdated(true)
        ));
        assert!(matches!(
            request(
                &mut host,
                CALLER,
                PluginRequest::TogglePanel {
                    panel_id: PANEL_ID.to_owned(),
                    show: Some(true),
                }
            ),
            PluginResponse::PanelToggled(true)
        ));

        let snapshot = match request(
            &mut host,
            CALLER,
            PluginRequest::PanelSnapshot {
                panel_id: PANEL_ID.to_owned(),
            },
        ) {
            PluginResponse::PanelSnapshot(Ok(snapshot)) => snapshot,
            other => panic!("unexpected correct-owner snapshot response: {other:?}"),
        };
        assert_eq!(snapshot.plugin_id, CALLER);
        assert_eq!(snapshot.tab_id, 73);
        assert_eq!(snapshot.values, values);
        assert_eq!(snapshot.raw_text, raw_text);
        assert!(snapshot.errors.is_empty());

        match request(
            &mut host,
            "example.wrong_owner",
            PluginRequest::PanelSnapshot {
                panel_id: PANEL_ID.to_owned(),
            },
        ) {
            PluginResponse::PanelSnapshot(Err(error)) => assert_eq!(error, OWNER_ERROR),
            other => panic!("wrong-owner snapshot was not rejected: {other:?}"),
        }

        let state = host.state.lock().unwrap();
        assert_eq!(
            state.calls,
            vec![
                PanelCall::Register {
                    plugin_id: CALLER.to_owned(),
                    panel_id: PANEL_ID.to_owned(),
                },
                PanelCall::Update {
                    plugin_id: CALLER.to_owned(),
                    panel_id: PANEL_ID.to_owned(),
                    values: values.clone(),
                },
                PanelCall::Toggle {
                    plugin_id: CALLER.to_owned(),
                    panel_id: PANEL_ID.to_owned(),
                    show: Some(true),
                },
                PanelCall::Snapshot {
                    plugin_id: CALLER.to_owned(),
                    panel_id: PANEL_ID.to_owned(),
                },
                PanelCall::Snapshot {
                    plugin_id: "example.wrong_owner".to_owned(),
                    panel_id: PANEL_ID.to_owned(),
                },
            ]
        );
        assert_eq!(state.document_reads, 0);
        assert_eq!(state.document_writes, 0);
    }

    #[test]
    fn legacy_host_defaults_reject_snapshot_and_keep_panel_registration_behavior() {
        let mut host = LegacyHost::default();
        let values = HashMap::from([("enabled".to_owned(), DialogValue::Bool(true))]);

        assert!(matches!(
            request(
                &mut host,
                CALLER,
                PluginRequest::RegisterPanel(PanelDef::new(PANEL_ID, "Legacy panel"))
            ),
            PluginResponse::PanelRegistered(true)
        ));
        assert!(matches!(
            request(
                &mut host,
                CALLER,
                PluginRequest::UpdatePanel {
                    panel_id: PANEL_ID.to_owned(),
                    values: values.clone(),
                }
            ),
            PluginResponse::PanelUpdated(true)
        ));
        assert!(matches!(
            request(
                &mut host,
                CALLER,
                PluginRequest::TogglePanel {
                    panel_id: PANEL_ID.to_owned(),
                    show: None,
                }
            ),
            PluginResponse::PanelToggled(true)
        ));
        match request(
            &mut host,
            CALLER,
            PluginRequest::PanelSnapshot {
                panel_id: PANEL_ID.to_owned(),
            },
        ) {
            PluginResponse::PanelSnapshot(Err(error)) => {
                assert_eq!(
                    error,
                    "Authenticated panel snapshots are not supported by this host"
                );
            }
            other => panic!("legacy host returned a snapshot: {other:?}"),
        }

        let state = host.state.lock().unwrap();
        assert_eq!(
            state.calls,
            vec![
                PanelCall::LegacyRegister {
                    panel_id: PANEL_ID.to_owned(),
                },
                PanelCall::LegacyUpdate {
                    panel_id: PANEL_ID.to_owned(),
                    values,
                },
                PanelCall::LegacyToggle {
                    panel_id: PANEL_ID.to_owned(),
                    show: None,
                },
            ]
        );
        assert_eq!(state.document_reads, 0);
        assert_eq!(state.document_writes, 0);
    }
}
