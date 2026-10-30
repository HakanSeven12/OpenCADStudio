//! Host-side IPC request handler.

use crate::host::HostApi;
use crate::ipc::protocol::{PluginRequest, PluginResponse};

fn addressable_document_snapshot(host: &dyn HostApi) -> codec::Document {
    let mut snapshot = host.document().clone();
    let attributes = host
        .document()
        .entities()
        .filter_map(|entity| match entity {
            codec::EntityType::Insert(insert) => Some(insert.attributes.clone()),
            _ => None,
        })
        .flatten()
        .map(codec::EntityType::AttributeEntity)
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
        AddEntities(entities) => PluginResponse::Handles(host.add_entities(entities)),
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
        UpdateEntitiesTransaction { label, entities } => PluginResponse::EntityTransactionResult(
            host.update_entities_transaction(&label, entities),
        ),
        GetSelection => PluginResponse::Selection(host.selection()),
        SetSelection { handles } => PluginResponse::SelectionResult(host.set_selection(&handles)),
        SolidOperation { operation } => {
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
                PluginResponse::CommandResult(Err(format!(
                    "run_command refused during a custom-grip drag: the host message loop \
                     cannot pump nested commands from a grip delivery; apply the edits \
                     directly in the settle callback (push_undo + update/add entities), \
                     or replay commands at your own boundary"
                )))
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
            PluginResponse::GetEntitiesBatchResult(host.entities_batch(&handles))
        }
        CreateConstraint { constraint } => {
            PluginResponse::CreateConstraintResult(host.constraint_create(constraint))
        }
        ReadXRecords { reads } => PluginResponse::ReadXRecordsResult(host.read_xrecords(&reads)),
        UpdateXRecords { updates } => {
            PluginResponse::UpdateXRecordsResult(host.update_xrecords(&updates))
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
        RegisterPluginClass { class } => {
            PluginResponse::RegisterPluginClassResult(host.register_plugin_class(
                &plugin_id,
                &class,
            ))
        }
        StagePluginClassInstance { instance } => {
            PluginResponse::StagePluginClassInstanceResult(host.stage_plugin_class_instance(
                &plugin_id,
                &instance,
            ))
        }
        PluginClassInstanceProperties { handle } => {
            PluginResponse::PluginClassInstancePropertiesResult(
                host.plugin_class_instance_properties(handle),
            )
        }
        ShowDialog(dialog) => PluginResponse::DialogResult(host.show_dialog(dialog)),
        RegisterPanel(panel) => PluginResponse::PanelRegistered(host.register_panel(panel)),
        UpdatePanel { panel_id, values } => {
            PluginResponse::PanelUpdated(host.update_panel(&panel_id, values))
        }
        TogglePanel { panel_id, show } => {
            PluginResponse::PanelToggled(host.toggle_panel(&panel_id, show))
        }
    }
}
