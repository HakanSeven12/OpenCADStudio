//! Request/response envelopes exchanged between the host and a plugin process.
//!
//! A single bidirectional socket is used. Each side sends either a request
//! (expecting a response) or a response (to a previous request). This lets the
//! host handle plugin RPCs inline while it waits for the result of a host→plugin
//! request such as `Dispatch`, avoiding the need for two sockets or threads.
//!
//! Compatibility rule: new variants are appended at the end of every public enum
//! (`HostRequest`, `HostResponse`, `PluginRequest`, `PluginResponse`,
//! `RunnerHandshake`) so older plugins keep their bincode discriminant indices.
//! The V4 additions are a separate frame layer in [`crate::ipc::v4`] and do not
//! alter these enums.
//!
//! The pre-shared runner token is delivered through [`PLUGIN_TOKEN_ENV`]
//! (`OCS_PLUGIN_TOKEN`). The runner must present the same token immediately
//! after connecting or the host closes the connection.

use serde::{Deserialize, Serialize};

use crate::host::{
    CommandSource, CommandStep, CustomGripEvent, CustomGripResponse, GripInfo, HostSettingValue,
    PluginConstraint, PluginGrip, XRecordEntry, XRecordRead, XRecordUpdate,
};
use crate::manifest::ApiVersion;
use crate::ribbon::owned::{OwnedEntityClasses, OwnedPluginManifest, OwnedRibbonGroup};

pub use crate::host::PreviewWire;
pub use codec::xdata::{ExtendedDataRecord, XDataValue};
pub use codec::{Document, EntityType, Handle};

/// Events the host forwards to an active plugin `InteractiveCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InteractiveEvent {
    /// User clicked or specified a point coordinate.
    Point([f64; 3]),
    /// User pressed Enter or Return to complete input.
    Enter,
    /// User selected an existing entity in the drawing.
    ObjectPick { handle: Handle, pt: [f64; 3] },
    /// User cancelled the prompt (e.g. pressed ESC), resetting interactive collection.
    Cancel,
    /// A set of entities selected during selection gathering mode.
    SelectionComplete(Vec<Handle>),
    /// V7 (additive): modal dialog submitted or cancelled.
    DialogResponse(crate::dialog::DialogResponse),
}

/// Initial handshake sent by the plugin runner immediately after connecting.
///
/// The runner proves it was spawned by this host by presenting a pre-shared
/// token delivered through the `OCS_PLUGIN_TOKEN` environment variable. A
/// mismatch causes the host to close the connection.
///
/// `TokenV4` is appended as the last variant so the existing `Token(String)`
/// variant keeps its bincode discriminant index (0), preserving V2/V3 wire
/// compatibility.
#[derive(Debug, Serialize, Deserialize)]
pub enum RunnerHandshake {
    Token(String),
    TokenV4 {
        token: String,
        protocol_version: u32,
    },
}

/// Environment variable through which the host passes the pre-shared
/// authentication token to the plugin runner child process.
pub const PLUGIN_TOKEN_ENV: &str = "OCS_PLUGIN_TOKEN";

/// Requests the host sends to the plugin runner.
#[derive(Debug, Serialize, Deserialize)]
pub enum HostRequest {
    GetManifest,
    GetRibbon,
    Dispatch {
        cmd: String,
    },
    InteractiveEvent {
        command_id: u64,
        event: InteractiveEvent,
    },
    GetPrompt {
        command_id: u64,
    },
    NeedsEntityPick {
        command_id: u64,
    },
    Shutdown,
    ExecuteCode {
        command_id: u64,
        source: CommandSource,
        code: String,
        tab_index: usize,
    },
    /// V7: release an interactive command after completion or cancellation.
    DropInteractive {
        command_id: u64,
    },
    /// V7: cursor move preview update during an interactive command.
    CursorMove {
        command_id: u64,
        pt: [f64; 3],
    },
    /// V11: one drag event for a plugin-declared custom grip, delivered to
    /// the owning plugin only. The runner passes the plugin a full HostApi
    /// proxy, so the settle phase can perform writes (possibly nested, over
    /// the multiplexed link) inside the plugin's own undo scope.
    CustomGripEvent {
        tab_index: usize,
        event: CustomGripEvent,
    },
    /// V7 (additive): query if an interactive command is in selection-gathering mode (as for `ssget`).
    IsSelectionGathering {
        command_id: u64,
    },
    /// V7 (additive): query if an interactive command permits drag window/crossing box selections.
    AcceptsDragSelection {
        command_id: u64,
    },
    /// V7 (additive): query if an interactive command rejects locked layers during selection.
    SelectionRejectsLockedLayers {
        command_id: u64,
    },
    /// V7: query optional modal dialog for an interactive command.
    GetModalDialog {
        command_id: u64,
    },
}

/// Responses the plugin runner sends back for `HostRequest`.
#[derive(Debug, Serialize, Deserialize)]
pub enum HostResponse {
    Bool(bool),
    CommandStep(Box<CommandStep>),
    Text(String),
    Ribbon(Vec<OwnedRibbonGroup>),
    Manifest(OwnedPluginManifest),
    Error(String),
    CodeExecutionResult(crate::host::ExecutionResult),
    PreviewWires(Vec<PreviewWire>),
    /// V11: the owning plugin's answer to a custom-grip drag event: the
    /// preview wire set that replaces its previous live preview.
    CustomGrip(CustomGripResponse),
    ModalDialog(Option<crate::dialog::DialogDef>),
}

/// Requests the plugin runner sends to the host.
#[derive(Debug, Serialize, Deserialize)]
pub enum PluginRequest {
    PushInfo(String),
    PushOutput(String),
    PushError(String),
    AddEntity(EntityType),
    /// Replace the existing entity carrying this entity's handle in place.
    UpdateEntity(EntityType),
    /// Delete the entity with `handle`.
    RemoveEntity {
        handle: Handle,
    },
    BumpGeometry,
    ReadRecord {
        handle: Handle,
        app_name: String,
    },
    WriteRecord {
        handle: Handle,
        record: ExtendedDataRecord,
    },
    RemoveRecord {
        handle: Handle,
        app_name: String,
    },
    PushUndo {
        label: String,
    },
    SetDirty,
    StartInteractive {
        command_id: u64,
    },
    DocumentSnapshot,
    /// Ask the host to create/refresh a shared-memory document view and return
    /// the file path + current version.
    OpenDocumentView,
    /// Add multiple entities in a single request.
    AddEntities(Vec<EntityType>),
    /// V4: ask the host to create/refresh a tab-keyed shared-memory document
    /// view and return the file path + current version.
    OpenDocumentViewV4 {
        tab_id: u64,
    },
    /// V4: ask the host to close the tab-keyed shared-memory document view.
    CloseDocumentViewV4 {
        tab_id: u64,
    },
    /// V4: ask the host for the stable tab identifier of the active tab.
    GetTabId,
    /// V5: ask the host for the filesystem path of the document in `tab_id`.
    DocumentPath {
        tab_id: u64,
    },
    /// Add a layer to the active document with full initial properties.
    AddLayer(crate::host::LayerConfig),
    /// Modify specified properties of an existing layer in the active document.
    ModifyLayer(crate::host::LayerConfig),
    /// Run a command on the active document tab's command line (AutoLISP style).
    ExecuteCommand(String),
    /// Read a host-managed setting without nested command dispatch.
    GetSystemVariable {
        name: String,
    },
    /// Change a host-managed setting without nested command dispatch.
    SetSystemVariable {
        name: String,
        value: HostSettingValue,
    },
    /// V7: validate and replace existing entities in a single undo step.
    UpdateEntitiesTransaction {
        label: String,
        entities: Vec<EntityType>,
    },
    /// V7: synchronous selection read/write for the dispatch tab.
    GetSelection,
    SetSelection {
        handles: Vec<Handle>,
    },
    /// V7 (additive): kernel-backed solid create or transform.
    SolidOperation {
        operation: crate::host::SolidOperation,
    },
    /// V7 (additive): drawing table record create/modify/rename/delete.
    TableOperation {
        operation: crate::host::TableOperation,
    },
    /// V7 (additive): drive an OCS command.
    RunCommand {
        request: crate::host::CommandRequest,
    },
    /// V8 (additive): create an XRecord under a named-object-dictionary path.
    CreateXRecord {
        dictionary_path: Vec<String>,
        name: String,
        entries: Vec<crate::host::XRecordEntry>,
    },
    /// V8 (additive): read the entries of an existing XRecord.
    ReadXRecord {
        dictionary_path: Vec<String>,
        name: String,
    },
    /// V8 (additive): replace the entries of an existing XRecord.
    UpdateXRecord {
        dictionary_path: Vec<String>,
        name: String,
        entries: Vec<crate::host::XRecordEntry>,
    },
    /// V8 (additive): delete an XRecord and its dictionary entry.
    DeleteXRecord {
        dictionary_path: Vec<String>,
        name: String,
    },
    /// V8 (additive): list the XRecord names under a dictionary path.
    ListXRecords {
        dictionary_path: Vec<String>,
    },
    /// V9 (additive): mark an entity reference-only (plugin-owned display data).
    SetReferenceOnly {
        handle: Handle,
        reference_only: bool,
    },
    /// V11 (additive): enumerate an entity's built-in grips with viewport
    /// parity.
    EntityGrips {
        handle: Handle,
    },
    /// V11 (additive): perform the one-undo-step grip edit an interactive
    /// drag performs.
    GripDrag {
        handle: Handle,
        grip_id: usize,
        to: [f64; 3],
    },
    /// V11 (additive): declare (or clear) the calling plugin's custom grip
    /// points. The declaring plugin's identity is attached by the host from
    /// the serving process's own manifest, never from the frame.
    SetPluginGrips {
        grips: Vec<PluginGrip>,
    },
    /// V12 (additive): read specific entities by handle — cheap per-entity
    /// reads for refresh passes, so a plugin that writes and re-reads never
    /// re-clones the full document snapshot (the v11.1 per-boundary cost:
    /// every write invalidated the session snapshot).
    GetEntitiesBatch {
        handles: Vec<Handle>,
    },
    /// V13 (additive): create a parametric constraint directly (the
    /// interactive COINCIDENT/TANGENT machinery, no command line, no echo;
    /// joins the caller's open plugin scope).
    CreateConstraint {
        constraint: PluginConstraint,
    },
    /// V13 (additive): read several XRecords in one round trip.
    ReadXRecords {
        reads: Vec<XRecordRead>,
    },
    /// V13 (additive): replace the entries of several XRecords in one
    /// round trip (one shared undo composition).
    UpdateXRecords {
        updates: Vec<XRecordUpdate>,
    },
    /// V13 (additive): list a dictionary's XRecords with their entries in
    /// one answer.
    ListXRecordsWithEntries {
        dictionary_path: Vec<String>,
    },
    /// V14 (additive): turn a declared parametric chain into reference-only
    /// display geometry (one publish, the entity-write undo contract).
    StageParametricChain {
        reference_name: String,
        chain: crate::host::ParametricChain,
        tolerance: f64,
    },
    /// V14 (additive): evaluate a declared chain at `s` — a pure read.
    SampleParametric {
        chain: crate::host::ParametricChain,
        s: f64,
    },
    /// V14 (additive): register a plugin-scoped entity class — session
    /// state; idempotent for an equal schema, refusing a mismatch.
    RegisterPluginClass {
        class: crate::host::PluginClassDeclaration,
    },
    /// V14 (additive): stage one instance of a registered plugin class —
    /// identity plus a host-evaluated design descriptor (the entity-write
    /// undo contract; reference-only; capped).
    StagePluginClassInstance {
        instance: crate::host::PluginClassInstance,
    },
    /// V14 (additive): read a registered class instance's declared
    /// parameter rows — the read half of the session's properties pair
    /// across the wire (the identity stamp resolves; the record row stays
    /// the values' truth).
    PluginClassInstanceProperties {
        handle: Handle,
    },
    /// V7 (additive): modal form dialog request.
    ShowDialog(crate::dialog::DialogDef),
    /// V7 (additive): register a persistent dockable side panel.
    RegisterPanel(crate::dialog::PanelDef),
    /// V7 (additive): update control values in a persistent dockable side panel.
    UpdatePanel {
        panel_id: String,
        values: std::collections::HashMap<String, crate::dialog::DialogValue>,
    },
    /// V7 (additive): toggle visibility or show/hide a persistent dockable side panel.
    TogglePanel {
        panel_id: String,
        show: Option<bool>,
    },
    /// V14: open one undo step that later messages join until
    /// [`PluginRequest::EndUndoGroup`].
    BeginUndoGroup {
        label: String,
    },
    /// V14: close the step [`PluginRequest::BeginUndoGroup`] opened.
    EndUndoGroup,
    /// V14: a page of the entities matching a control-API `query` JSON.
    QueryEntities {
        query: String,
    },
}

/// Responses the host sends back for `PluginRequest`.
#[derive(Debug, Serialize, Deserialize)]
pub enum PluginResponse {
    Ok,
    Bool(bool),
    Handle(Handle),
    Record(Option<ExtendedDataRecord>),
    Document(Box<Document>),
    Error(String),
    /// Path to the memory-mapped file and the current snapshot version.
    DocumentView {
        path: String,
        version: u64,
    },
    Handles(Vec<Handle>),
    /// V4: path to the tab-keyed memory-mapped file and current version.
    DocumentViewV4 {
        path: String,
        version: u64,
    },
    /// V4: stable tab identifier of the active tab.
    TabId(u64),
    /// V5: filesystem path of the document in the requested tab, if any.
    DocumentPath(Option<std::ffi::OsString>),
    /// Optional entity handle (e.g. from AddLayer).
    OptHandle(Option<Handle>),
    SystemVariable(Option<HostSettingValue>),
    SystemVariableResult(Result<HostSettingValue, String>),
    EntityTransactionResult(Result<(), String>),
    Selection(Vec<Handle>),
    SelectionResult(Result<(), String>),
    SolidResult(Result<Handle, String>),
    TableResult(Result<Handle, String>),
    CommandResult(Result<crate::host::CommandOutcome, String>),
    /// V8 (additive): handle of a created XRecord.
    XRecordCreateResult(Result<Handle, String>),
    /// V8 (additive): entries of a read XRecord.
    XRecordEntries(Result<Vec<crate::host::XRecordEntry>, String>),
    /// V8 (additive): handle of an updated XRecord.
    XRecordUpdateResult(Result<Handle, String>),
    /// V8 (additive): the removed XRecord, with its prior content.
    XRecordDeleteResult(Result<crate::host::XRecord, String>),
    /// V8 (additive): XRecord names under a dictionary path.
    XRecordNames(Result<Vec<String>, String>),
    /// V9 (additive): outcome of the reference-only designation.
    SetReferenceOnlyResult(Result<(), String>),
    /// V11 (additive): an entity's built-in grip list.
    EntityGripsResult(Result<Vec<GripInfo>, String>),
    /// V11 (additive): outcome of a programmatic grip drag.
    GripDragResult(Result<(), String>),
    /// V11 (additive): outcome of a custom-grip declaration.
    SetPluginGripsResult(Result<(), String>),
    /// V12 (additive): per-entity reads by handle (None per unknown handle).
    GetEntitiesBatchResult(Result<Vec<Option<EntityType>>, String>),
    /// V13 (additive): outcome of a direct constraint creation.
    CreateConstraintResult(Result<(), String>),
    /// V13 (additive): batched XRecord reads (per-entry results).
    ReadXRecordsResult(Result<Vec<Result<Vec<XRecordEntry>, String>>, String>),
    /// V13 (additive): batched XRecord updates (per-entry results).
    UpdateXRecordsResult(Result<Vec<Result<Handle, String>>, String>),
    /// V13 (additive): dictionary XRecords with their entries.
    ListXRecordsWithEntriesResult(Result<Vec<(String, Vec<XRecordEntry>)>, String>),
    /// V14 (additive): handles of the staged display entities.
    StageParametricChainResult(Result<Vec<Handle>, String>),
    /// V14 (additive): the evaluated chain point, `None` outside the chain.
    SampleParametricResult(Result<Option<crate::host::ParametricSample>, String>),
    /// V14 (additive): outcome of a plugin class registration.
    RegisterPluginClassResult(Result<(), String>),
    /// V14 (additive): handles of the staged instance display set.
    StagePluginClassInstanceResult(Result<Vec<Handle>, String>),
    /// V14 (additive): a class instance's declared parameter rows (the
    /// read half of the session's properties pair).
    PluginClassInstancePropertiesResult(
        Result<Vec<crate::host::PluginClassParameterRow>, String>,
    ),
    DialogResult(crate::dialog::DialogResponse),
    PanelRegistered(bool),
    PanelUpdated(bool),
    PanelToggled(bool),
    /// V14: answer to [`PluginRequest::QueryEntities`].
    EntityPage(Result<crate::host::EntityPage, String>),
}

/// Messages sent from the host to the plugin runner.
#[derive(Debug, Serialize, Deserialize)]
pub enum HostToPlugin {
    Request(HostRequest),
    Response(Box<PluginResponse>),
}

/// Messages sent from the plugin runner to the host.
#[derive(Debug, Serialize, Deserialize)]
pub enum PluginToHost {
    Request(Box<PluginRequest>),
    Response(HostResponse),
}

/// Convenience helper for manifest serialization.
impl From<&'static crate::manifest::PluginManifest> for OwnedPluginManifest {
    fn from(m: &'static crate::manifest::PluginManifest) -> Self {
        Self {
            id: m.id.to_string(),
            name: m.name.to_string(),
            version: m.version.to_string(),
            description: m.description.to_string(),
            api_version: m.api_version.major,
            ribbon_order: m.ribbon_order,
            xdata_apps: m.xdata_apps.iter().map(|s| s.to_string()).collect(),
            command_prefixes: m.command_prefixes.iter().map(|s| s.to_string()).collect(),
            // The declaration does not live on the static manifest struct (its
            // layout is frozen ABI, shared by pointer with older cdylibs); the
            // runner folds the API-v10 trait accessor in after conversion.
            entity_classes: None,
        }
    }
}

impl From<&'static crate::manifest::EntityClasses> for OwnedEntityClasses {
    fn from(c: &'static crate::manifest::EntityClasses) -> Self {
        Self {
            app_id: c.app_id.to_string(),
            class_prefix: c.class_prefix.to_string(),
            labels: c
                .labels
                .iter()
                .map(|(key, label)| (key.to_string(), label.to_string()))
                .collect(),
        }
    }
}

impl OwnedPluginManifest {
    pub fn api_version(&self) -> ApiVersion {
        ApiVersion {
            major: self.api_version,
        }
    }
}
