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
pub use document::xdata::{ExtendedDataRecord, XDataValue};
pub use document::{Document, EntityType, Handle};

use crate::scene::{
    AppliedPatchReport, PanelScene, PanelScenePatch, SceneElement, SceneKindCounts, SceneRevision,
    SceneSummary, StableId,
};
use crate::widgets::{
    ItemDef, TableRowDef, TreeRowDef, WidgetEvent, WidgetRange, WidgetSurface, WidgetSurfacePatch,
    WidgetSummary, MAX_WIDGET_RANGE_LEN, MAX_WIDGET_ROWS_INITIAL,
};

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
    /// V19 (additive): an event (choice change, button, toggle, table row)
    /// inside the interactive command's open modal dialog. The runner answers
    /// with [`HostResponse::DialogAction`] carrying the command's
    /// `on_dialog_action` result, so out-of-process plugins can replace,
    /// submit or close their dialog like in-process ones.
    DialogAction {
        command_id: u64,
        event: crate::dialog::DialogEvent,
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
    /// V19 (additive): answer to [`HostRequest::DialogAction`].
    DialogAction(crate::dialog::DialogActionResponse),
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
    /// V15 (additive): register the declarative quad cursor menu panels this
    /// plugin contributes (mirrors `RegisterPanel`; the owning plugin's id
    /// comes from the transport, never the wire).
    RegisterQuadPanels {
        panels: Vec<crate::dialog::QuadPanelDef>,
    },
    /// V15 (additive): remove the named quad panels the plugin owns (ids
    /// owned by another plugin are refused in the response).
    UnregisterQuadPanels {
        panel_ids: Vec<String>,
    },
    /// Additive: audit a declared chain without staging or mutating host state.
    ValidateParametricChain {
        chain: crate::host::ParametricChain,
        tolerance: f64,
    },
    /// Additive: read the current state of an owned dock panel.
    PanelSnapshot {
        panel_id: String,
    },
    /// V22 (additive): create or update the model-space GEODATA object.
    SetGeoLocation {
        location: crate::host::GeoLocation,
    },
    /// V22 (additive): remove the model-space GEODATA object.
    RemoveGeoLocation,
    /// V23 (additive): publish a panel's scene / widget surface — the
    /// publish-or-patch contract. The FIRST publish of a panel carries the
    /// full bounded surface (a snapshot the host mounts); later state moves
    /// travel as [`PluginRequest::PanelScenePatch`] frames that name a
    /// `base_revision` (the atomic two-phase compare inside
    /// `PanelScene::apply_patch` / `WidgetSurface::apply_patch`), so this
    /// request must not recur in a steady stream. `events` is the
    /// piggybacked event leg: host-observed widget events the plugin already
    /// merged into this surface revision (the events-vs-patches split of the
    /// widgets module: full-collection retransmits are defects; a frame
    /// never re-sends a whole tree).
    ///
    /// Boundaries (recorded): this tail is TRANSPORT-ONLY — the wire layer
    /// validates (decode budgets + structural `validate()`) and answers
    /// with the typed `PatchApplied` / `PatchRejected` acks. The host
    /// side that MOUNTS the surface onto its UI state (holding the panel's
    /// `PanelScene` / `WidgetSurface`, applying patches, routing events) is
    /// a recorded follow-up (scene-UI ADR: mount/publisher attach); nothing
    /// in this package touches host-UI state.
    PanelScenePublish {
        /// Panel id as the plugin named it (the same id a `RegisterPanel`
        /// registration used; ownership is authenticated by the transport —
        /// the serving process's manifest — never by this field).
        #[serde(deserialize_with = "bounded_panel_id")]
        panel_id: String,
        /// The published surface (exactly one of scene / widget surface;
        /// a panel carries one of each alongside the same revision space).
        surface: SceneOrWidgetSurface,
        /// Piggybacked widget events (bounded batch, see
        /// [`MAX_WIDGET_EVENT_ROWS`]).
        #[serde(deserialize_with = "budgeted_publish_events")]
        events: Vec<WidgetEvent>,
    },
    /// V23 (additive): an incremental panel-surface update — the
    /// patch leg of the publish-or-patch contract. The patch's ops are
    /// applied atomically (all-or-nothing) against the surface the host
    /// holds: a stale `base_revision` (or any op failure) is answered with
    /// the structured [`PluginResponse::PatchRejected`] (bounded report, no
    /// mutation, no full dump) and the plugin re-synchronizes — it never
    /// re-publishes to muddle through.
    ///
    /// Same boundaries as [`PluginRequest::PanelScenePublish`]:
    /// transport-level validate/ack/reject only; the mount/publisher attach
    /// is the recorded follow-up.
    PanelScenePatch {
        /// Panel id (transport-authenticated ownership; wire cap).
        #[serde(deserialize_with = "bounded_panel_id")]
        panel_id: String,
        /// Which surface the patch addresses and its op list (decoded under
        /// the scene/widget budgets inside the patch types themselves).
        patch: SceneOrWidgetPatch,
    },
    /// V23 (additive): a BOUNDED store read — one window of rows, one lazy
    /// children page, or one scene region ([`PanelStoreKind`]). The range's
    /// width is capped at decode ([`MAX_WIDGET_RANGE_LEN`]); an over-cap
    /// read request is a structured refusal (the frame decodes to an error),
    /// never a larger page, and a plugin never gets an unbounded dump back:
    /// answers are [`PluginResponse::StorePage`] pages of at most the
    /// rows-per-payload budget. The future page SERVER resolves windows
    /// with checked arithmetic against the store size (an empty page for a
    /// past-the-end window); the wire never carries unwindowed asks.
    PanelStoreRead {
        /// Panel id (transport-authenticated ownership; wire cap).
        #[serde(deserialize_with = "bounded_panel_id")]
        panel_id: String,
        /// Which bounded store the window addresses.
        kind: PanelStoreKind,
        /// The anchoring id: the lazy tree parent (children pages), the
        /// list widget (item windows), the registry table (typed-table
        /// rows), or the scene subtree root (region reads). `None` addresses
        /// the surface root / the whole collection.
        parent_id: Option<StableId>,
        /// The requested window (width-capped at decode).
        #[serde(deserialize_with = "bounded_store_range")]
        range: WidgetRange,
    },
    /// V23 (additive): request the CURRENT state of a panel's surfaces as a
    /// bounded recovery read: the bounded summaries (the
    /// [`SceneSummary`]-grade digest of what the host holds) plus the LEADING
    /// window of the scene — never a full dump (full dumps remain
    /// prohibited). The answer is [`PluginResponse::StorePage`] whose page
    /// leads with a [`StoreRow::State`] digest row ([`StoreStateDigest`])
    /// followed by at most the rows-per-payload window of elements; a plugin
    /// that missed patches (revision gap after a disconnect) re-synchronizes
    /// from this and resumes patching, consuming further windows through
    /// [`PluginRequest::PanelStoreRead`] ([`PanelStoreKind::SceneRegion`]).
    PanelSceneSnapshot2 {
        /// Panel id (transport-authenticated ownership; wire cap).
        #[serde(deserialize_with = "bounded_panel_id")]
        panel_id: String,
    },
}

// ---------------------------------------------------------------------------
// V23 wire tails: payload vocabulary (envelopes, store kinds, rows, rejections)
// ---------------------------------------------------------------------------

/// Maximum `panel_id` bytes accepted on any v23 scene/store tail. No
/// earlier tail capped the id string on the wire (ownership is
/// transport-authenticated), so this is NEW at v23 for the heavy payloads'
/// frames — a hostile or fat-fingered id refuses at decode instead of
/// flowing into host state. Generous to the documented "align_tools"-grade
/// ids; the registration side stays the byte-free contract it always was
/// (only the scene/store frames gate).
pub const MAX_PANEL_ID_BYTES: usize = 256;

/// Maximum event rows carried by ONE v23 frame's event batch
/// ([`PluginRequest::PanelScenePublish`], [`HostNotification::PanelSceneEvents`]).
/// Matches the rows-per-payload budget family (`MAX_WIDGET_ROWS_INITIAL`);
/// named for its role because events are rows of the notify leg, not
/// payload vecs of a widget def. Beyond this a producer must split (events
/// are individually bounded and semantically small — a bigger batch is a
/// defect, never a transport need).
pub const MAX_WIDGET_EVENT_ROWS: usize = 4_096;

/// Which surface of a panel a v23 publish frame carries — the minimal
/// envelope the §4 append plan names ("one `PanelScene` or one
/// `WidgetSurface`"; a panel owns one of each but a frame carries one).
///
/// Compatibility rule: appended at the END so existing read code keeps its
/// bincode discriminants (`0..=1`; pinned by the tests in this module from
/// creation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneOrWidgetSurface {
    /// The panel scene surface (the full bounded snapshot leg).
    Scene(PanelScene),
    /// The panel's semantic widget surface.
    Widgets(WidgetSurface),
}

/// Which surface of a panel a v23 patch frame updates — the minimal
/// envelope for `PanelScenePatch | WidgetSurfacePatch`. Both patch types
/// already decode under their own budgets (`budgeted_ops` /
/// `budgeted_widget_ops`) and apply atomically.
///
/// Compatibility rule: appended at the END (`0..=1`; pinned from creation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneOrWidgetPatch {
    /// The panel scene patch (scene grammar ops).
    Scene(PanelScenePatch),
    /// The widget surface patch (structural widget ops + cell facts).
    Widgets(WidgetSurfacePatch),
}

/// Which bounded store a [`PluginRequest::PanelStoreRead`] window addresses.
///
/// Compatibility rule: appended at the END (`0..=3`; pinned from creation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelStoreKind {
    /// Lazy tree-branch children under `parent_id` (the
    /// [`crate::widgets::WidgetEvent::ChildrenRequested`] analog on the
    /// plugin-pull side: a page of [`TreeRowDef`] rows).
    TreeChildren,
    /// An item-list window into `parent_id` (a page of [`ItemDef`] items).
    ListItems,
    /// A typed-table rows window (`parent_id` names the registry table;
    /// keyed windows per the typed-table contract).
    TableRows,
    /// A scene elements window (the region read / snapshot continuation).
    SceneRegion,
}

/// Bounded recovery digest the windowed snapshot answer leads with — the
/// bounded summaries of the surfaces the host currently holds for the
/// panel (`None` where it holds nothing). Carries summaries ONLY (the
/// bounded, re-derivable reads — no payload dumps).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreStateDigest {
    /// The scene summary, when the host holds a scene for the panel.
    pub scene: Option<SceneSummary>,
    /// The widget summary, when the host holds a widget surface.
    pub widgets: Option<WidgetSummary>,
}

/// One row of a [`PluginResponse::StorePage`] — the bounded row envelope of
/// the v23 store reads (each variant's payload is a bounded read the
/// scene/widget modules cap at decode: page ELEMENTS charge the scene node
/// budget, row payloads carry their own payload caps).
///
/// Compatibility rule: appended at the END (`0..=4`; pinned from creation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StoreRow {
    /// The bounded recovery digest ([`StoreStateDigest`]) — the header row
    /// the windowed snapshot answer leads with; absent from ordinary page
    /// reads.
    State(StoreStateDigest),
    /// One windowed scene element (a [`PanelStoreKind::SceneRegion`] page
    /// row / element of the snapshot window).
    Scene(SceneElement),
    /// One tree row payload (a [`PanelStoreKind::TreeChildren`] page row).
    Tree(TreeRowDef),
    /// One item payload (a [`PanelStoreKind::ListItems`] page row).
    Item(ItemDef),
    /// One typed-table row payload (a [`PanelStoreKind::TableRows`] page
    /// row).
    Table(TableRowDef),
}

/// Machine-readable code of a [`PluginResponse::PatchRejected`] — WHY the
/// patch/publish refused. Codes only (no payload): the recovery facts ride
/// [`PatchRejectReport`], bounded.
///
/// Compatibility rule: appended at the END (`0..=5`; pinned from creation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatchRejectCode {
    /// The patch's `base_revision` is no longer the panel's head.
    StaleBase,
    /// The patch's successor revision is not greater than the head.
    NonMonotonicRevision,
    /// The decoded op list exceeds [`crate::scene::MAX_PATCH_OPS`].
    OpBudgetExceeded,
    /// Structurally invalid publish/patch payload (unknown anchor, id
    /// collision, out-of-budget node/depth/text/shape…) — the
    /// structural-refusal bucket (`PanelScene::validate` /
    /// `WidgetSurface::validate` / per-op staging failures).
    InvalidOp,
    /// The v23 wire frame was accepted but no host UI publisher/mount is
    /// attached for the panel yet (the scene-UI mount/publisher attach is
    /// the recorded follow-up). Transport-honest answer, never a silent
    /// success.
    PublisherUnattached,
    /// The panel's v23 surface store is already attached to a DIFFERENT
    /// transport-authenticated plugin. The FIRST publisher owns the panel's
    /// surface store; publishing never transfers it (the owning plugin keeps
    /// the patch/publish contract; a second plugin's publish refuses
    /// structurally instead of taking the panel over). Appended with the
    /// mount/publisher attach (SCN-025); bincode discriminant 5.
    PublisherOwnedByOther,
}

/// The bounded rejection report of [`PluginResponse::PatchRejected`]:
/// explanatory facts a stale plugin needs to re-synchronize, WITHOUT any
/// payload dump (the §14 rule — structured rejections stay tiny; the
/// rejected op list is the plugin's own, and the reported carried surface
/// bytes are never echoed back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchRejectReport {
    /// The host's current head revision of the addressed surface, when
    /// known. (`None` when unknown/unattached.) A stale-base rejection and
    /// this answer is all a plugin needs to rebuild its patch.
    pub current_revision: Option<SceneRevision>,
    /// Zero-based index of the first op the host could not stage, when the
    /// refusal is op-local.
    pub failed_op: Option<usize>,
}

/// Bounded decode helper for the v23 tails' `panel_id` strings
/// ([`MAX_PANEL_ID_BYTES`]). Wire layout unchanged; a past-cap id refuses
/// at decode.
fn bounded_panel_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let panel_id = String::deserialize(d)?;
    if panel_id.len() > MAX_PANEL_ID_BYTES {
        return Err(serde::de::Error::custom(format!(
            "panel_id exceeds the {}-byte cap",
            MAX_PANEL_ID_BYTES
        )));
    }
    Ok(panel_id)
}

/// Bounded decode helper for the v23 event batches: charges the batch
/// against the per-thread widget decode budget (the same machinery the
/// widget payloads charge — `crate::widgets::budgeted_widget_events`, whose
/// production caller this wire task is) and then enforces the
/// rows-per-batch cap ([`MAX_WIDGET_EVENT_ROWS`]) the notify leg keeps.
fn budgeted_publish_events<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<WidgetEvent>, D::Error> {
    let events = crate::widgets::budgeted_widget_events(d)?;
    if events.len() > MAX_WIDGET_EVENT_ROWS {
        return Err(serde::de::Error::custom(format!(
            "event batch exceeds the decoded payload budget ({MAX_WIDGET_EVENT_ROWS})"
        )));
    }
    Ok(events)
}

/// Bounded decode helper for [`PluginRequest::PanelStoreRead`]'s `range`:
/// mirrors the widgets module's `bounded_widget_range` (private there) —
/// the requested page width must stay within [`MAX_WIDGET_RANGE_LEN`].
/// Range sanity at the wire: `len` is the cap; an over-cap read is a
/// structured decode refusal, never a bigger page.
fn bounded_store_range<'de, D: serde::Deserializer<'de>>(d: D) -> Result<WidgetRange, D::Error> {
    let range = WidgetRange::deserialize(d)?;
    if range.len > MAX_WIDGET_RANGE_LEN {
        return Err(serde::de::Error::custom(format!(
            "store read range exceeds the decoded page-width budget ({MAX_WIDGET_RANGE_LEN})"
        )));
    }
    Ok(range)
}

/// Bounded decode helper for [`PluginResponse::StorePage`]'s `rows` vec:
/// a page carries at most the rows-per-payload budget
/// ([`MAX_WIDGET_ROWS_INITIAL`]), so an answer is bounded regardless of how
/// big the underlying store is. Refuses past-cap at decode (never a
/// materialized bigger page).
fn budgeted_store_rows<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<StoreRow>, D::Error> {
    let rows = Vec::<StoreRow>::deserialize(d)?;
    if rows.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(serde::de::Error::custom(format!(
            "store page exceeds the decoded payload budget ({MAX_WIDGET_ROWS_INITIAL})"
        )));
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Wire bridges for the in-process bounded read types (v23)
//
// `SceneSummary` / `SceneKindCounts` / `WidgetSummary` / `WidgetKindCounts` /
// `AppliedPatchReport` are in-process READ types (the bounded recovery reads
// and patch reports): before v23 nothing consumed them on the wire, and
// scene.rs / widgets.rs are SOURCE-FROZEN for this package, so their serde
// impls live HERE as same-shape mirror structs with `From` mappings. The
// wire layout is exactly the derive these types would carry (each field
// serialized in declaration order, no extra fields), and a field added to
// any mirrored type is an immediate compile failure in the mappings below
// (every mapping names each field), so a bridge cannot drift silently.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SceneKindCountsWire {
    groups: usize,
    rects: usize,
    paths: usize,
    cubic_paths: usize,
    texts: usize,
    assets: usize,
    ports: usize,
    markers: usize,
    edges: usize,
}

impl From<&SceneKindCounts> for SceneKindCountsWire {
    fn from(c: &SceneKindCounts) -> Self {
        Self {
            groups: c.groups,
            rects: c.rects,
            paths: c.paths,
            cubic_paths: c.cubic_paths,
            texts: c.texts,
            assets: c.assets,
            ports: c.ports,
            markers: c.markers,
            edges: c.edges,
        }
    }
}

impl From<SceneKindCountsWire> for SceneKindCounts {
    fn from(w: SceneKindCountsWire) -> Self {
        Self {
            groups: w.groups,
            rects: w.rects,
            paths: w.paths,
            cubic_paths: w.cubic_paths,
            texts: w.texts,
            assets: w.assets,
            ports: w.ports,
            markers: w.markers,
            edges: w.edges,
        }
    }
}

impl serde::Serialize for SceneKindCounts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SceneKindCountsWire::from(self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SceneKindCounts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        SceneKindCountsWire::deserialize(deserializer).map(Self::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SceneSummaryWire {
    revision: SceneRevision,
    nodes: usize,
    by_kind: SceneKindCountsWire,
    root_ids: Vec<StableId>,
    root_ids_truncated: usize,
    overflow_roots: bool,
}

impl From<&SceneSummary> for SceneSummaryWire {
    fn from(s: &SceneSummary) -> Self {
        Self {
            revision: s.revision,
            nodes: s.nodes,
            by_kind: SceneKindCountsWire::from(&s.by_kind),
            root_ids: s.root_ids.clone(),
            root_ids_truncated: s.root_ids_truncated,
            overflow_roots: s.overflow_roots,
        }
    }
}

impl From<SceneSummaryWire> for SceneSummary {
    fn from(w: SceneSummaryWire) -> Self {
        Self {
            revision: w.revision,
            nodes: w.nodes,
            by_kind: w.by_kind.into(),
            root_ids: w.root_ids,
            root_ids_truncated: w.root_ids_truncated,
            overflow_roots: w.overflow_roots,
        }
    }
}

impl serde::Serialize for SceneSummary {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SceneSummaryWire::from(self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SceneSummary {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        SceneSummaryWire::deserialize(deserializer).map(Self::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WidgetKindCountsWire {
    trees: usize,
    lists: usize,
    icon_buttons: usize,
    menus: usize,
    tables: usize,
    groups: usize,
}

impl From<&crate::widgets::WidgetKindCounts> for WidgetKindCountsWire {
    fn from(c: &crate::widgets::WidgetKindCounts) -> Self {
        Self {
            trees: c.trees,
            lists: c.lists,
            icon_buttons: c.icon_buttons,
            menus: c.menus,
            tables: c.tables,
            groups: c.groups,
        }
    }
}

impl From<WidgetKindCountsWire> for crate::widgets::WidgetKindCounts {
    fn from(w: WidgetKindCountsWire) -> Self {
        Self {
            trees: w.trees,
            lists: w.lists,
            icon_buttons: w.icon_buttons,
            menus: w.menus,
            tables: w.tables,
            groups: w.groups,
        }
    }
}

impl serde::Serialize for crate::widgets::WidgetKindCounts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WidgetKindCountsWire::from(self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for crate::widgets::WidgetKindCounts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        WidgetKindCountsWire::deserialize(deserializer).map(Self::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WidgetSummaryWire {
    revision: SceneRevision,
    widgets: usize,
    by_kind: WidgetKindCountsWire,
    root_ids: Vec<StableId>,
    root_ids_truncated: usize,
    overflow_roots: bool,
}

impl From<&WidgetSummary> for WidgetSummaryWire {
    fn from(s: &WidgetSummary) -> Self {
        Self {
            revision: s.revision,
            widgets: s.widgets,
            by_kind: WidgetKindCountsWire::from(&s.by_kind),
            root_ids: s.root_ids.clone(),
            root_ids_truncated: s.root_ids_truncated,
            overflow_roots: s.overflow_roots,
        }
    }
}

impl From<WidgetSummaryWire> for WidgetSummary {
    fn from(w: WidgetSummaryWire) -> Self {
        Self {
            revision: w.revision,
            widgets: w.widgets,
            by_kind: w.by_kind.into(),
            root_ids: w.root_ids,
            root_ids_truncated: w.root_ids_truncated,
            overflow_roots: w.overflow_roots,
        }
    }
}

impl serde::Serialize for WidgetSummary {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WidgetSummaryWire::from(self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for WidgetSummary {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        WidgetSummaryWire::deserialize(deserializer).map(Self::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AppliedPatchReportWire {
    revision: SceneRevision,
    touched_subtree_ids: Vec<StableId>,
    touched_truncated: usize,
    retired_ids: Vec<StableId>,
    retired_truncated: usize,
    summary: SceneSummaryWire,
}

impl From<&AppliedPatchReport> for AppliedPatchReportWire {
    fn from(r: &AppliedPatchReport) -> Self {
        Self {
            revision: r.revision,
            touched_subtree_ids: r.touched_subtree_ids.clone(),
            touched_truncated: r.touched_truncated,
            retired_ids: r.retired_ids.clone(),
            retired_truncated: r.retired_truncated,
            summary: SceneSummaryWire::from(&r.summary),
        }
    }
}

impl From<AppliedPatchReportWire> for AppliedPatchReport {
    fn from(w: AppliedPatchReportWire) -> Self {
        Self {
            revision: w.revision,
            touched_subtree_ids: w.touched_subtree_ids,
            touched_truncated: w.touched_truncated,
            retired_ids: w.retired_ids,
            retired_truncated: w.retired_truncated,
            summary: w.summary.into(),
        }
    }
}

impl serde::Serialize for AppliedPatchReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AppliedPatchReportWire::from(self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for AppliedPatchReport {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        AppliedPatchReportWire::deserialize(deserializer).map(Self::from)
    }
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
    PluginClassInstancePropertiesResult(Result<Vec<crate::host::PluginClassParameterRow>, String>),
    DialogResult(crate::dialog::DialogResponse),
    PanelRegistered(bool),
    PanelUpdated(bool),
    PanelToggled(bool),
    /// V14: answer to [`PluginRequest::QueryEntities`].
    EntityPage(Result<crate::host::EntityPage, String>),
    /// V15 (additive): outcome of a quad panel registration or removal —
    /// one `ok` entry per request panel (request order), plus a short reason
    /// string for each refused entry.
    QuadPanelsRegistered {
        ok: Vec<bool>,
        errors: Vec<String>,
    },
    /// Additive: outcome of pure parametric-chain validation.
    ValidateParametricChainResult(Result<(), String>),
    /// Additive: current state of an owned dock panel.
    PanelSnapshot(Result<crate::dialog::PanelSnapshot, String>),
    /// V22 (additive): the GEODATA handle written by `SetGeoLocation`.
    GeoLocationSet(Result<crate::host::Handle, String>),
    /// V22 (additive): whether `RemoveGeoLocation` removed an object.
    GeoLocationRemoved(Result<bool, String>),
    /// V23 (additive): the bounded invalidation plan of an applied
    /// publish/patch ([`crate::scene::AppliedPatchReport`]) — the typed ack
    /// of the publish-or-patch contract. Bounded by construction (capped
    /// touched/retired id lists + the bounded post-patch summary).
    PatchApplied(AppliedPatchReport),
    /// V23 (additive): the structured refusal of a publish/patch — machine
    /// code ([`PatchRejectCode`]) plus the bounded recovery report
    /// ([`PatchRejectReport`]). Carries NO payload dumps (no full trees /
    /// op lists echoed back): the plugin re-synchronizes from its own
    /// authoritative model plus the two facts the report names.
    PatchRejected {
        /// The refusal bucket.
        code: PatchRejectCode,
        /// Bounded recovery facts (head revision / op index).
        report: PatchRejectReport,
    },
    /// V23 (additive): one bounded window of a store read
    /// ([`PluginRequest::PanelStoreRead`]) or the leading window of a
    /// windowed snapshot request ([`PluginRequest::PanelSceneSnapshot2`],
    /// whose page leads with a [`StoreRow::State`] digest row). `rows` is
    /// capped at decode by the rows-per-payload budget; `total_hint` is the
    /// host's belief of the total item count the addressed store holds
    /// (`0` when unknown/not attached) — a HINT for scrollbar sizing, never
    /// a contract (the store owner may have moved between frames).
    StorePage {
        /// The windowed rows (bounded page).
        #[serde(deserialize_with = "budgeted_store_rows")]
        rows: Vec<StoreRow>,
        /// Total-count hint of the addressed store.
        total_hint: u64,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    fn discriminant_of<T: Serialize>(value: &T) -> u32 {
        let bytes = bincode::serialize(value).expect("serialize");
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    #[test]
    fn plugin_request_v14_to_v16_discriminants_are_preserved() {
        assert_eq!(
            discriminant_of(&PluginRequest::ShowDialog(crate::dialog::DialogDef::new("T"))),
            50
        );
        assert_eq!(
            discriminant_of(&PluginRequest::RegisterPanel(
                crate::dialog::PanelDef::new("id", "T")
            )),
            51
        );
        assert_eq!(
            discriminant_of(&PluginRequest::UpdatePanel {
                panel_id: "id".into(),
                values: Default::default(),
            }),
            52
        );
        assert_eq!(
            discriminant_of(&PluginRequest::TogglePanel {
                panel_id: "id".into(),
                show: None,
            }),
            53
        );
        assert_eq!(
            discriminant_of(&PluginRequest::BeginUndoGroup {
                label: "group".into(),
            }),
            54
        );
        assert_eq!(discriminant_of(&PluginRequest::EndUndoGroup), 55);
        assert_eq!(
            discriminant_of(&PluginRequest::QueryEntities {
                query: "{}".into(),
            }),
            56
        );
        assert_eq!(
            discriminant_of(&PluginRequest::RegisterQuadPanels { panels: vec![] }),
            57
        );
        assert_eq!(
            discriminant_of(&PluginRequest::UnregisterQuadPanels { panel_ids: vec![] }),
            58
        );
        assert_eq!(
            discriminant_of(&HostRequest::GetModalDialog { command_id: 1 }),
            14
        );
        assert_eq!(
            discriminant_of(&HostRequest::DialogAction {
                command_id: 1,
                event: crate::dialog::DialogEvent {
                    control_id: String::new(),
                    action: String::new(),
                    value: None,
                    current_values: Default::default(),
                },
            }),
            15
        );
    }

    fn discriminant<T: serde::Serialize>(value: &T) -> u32 {
        let bytes = bincode::serialize(value).expect("serialize IPC variant");
        u32::from_le_bytes(bytes[..4].try_into().expect("bincode enum discriminant"))
    }

    #[test]
    fn panel_snapshot_variants_append_without_shifting_discriminants() {
        let request = PluginRequest::PanelSnapshot {
            panel_id: "alignment".to_owned(),
        };
        let response = PluginResponse::PanelSnapshot(Err("unsupported".to_owned()));

        assert_eq!(discriminant(&PluginRequest::PushInfo(String::new())), 0);
        assert_eq!(
            discriminant(&PluginRequest::ValidateParametricChain {
                chain: crate::host::ParametricChain {
                    plane: crate::host::ParametricPlane::Plan,
                    segments: Vec::new(),
                    layer: String::new(),
                },
                tolerance: 1e-3,
            }),
            59
        );
        assert_eq!(discriminant(&request), 60);
        assert_eq!(discriminant(&PluginResponse::Ok), 0);
        assert_eq!(
            discriminant(&PluginResponse::ValidateParametricChainResult(Ok(()))),
            45
        );
        assert_eq!(discriminant(&response), 46);

        let bytes = bincode::serialize(&request).expect("serialize panel snapshot request");
        match bincode::deserialize::<PluginRequest>(&bytes).expect("deserialize request") {
            PluginRequest::PanelSnapshot { panel_id } => assert_eq!(panel_id, "alignment"),
            other => panic!("unexpected: {other:?}"),
        }
        let bytes = bincode::serialize(&response).expect("serialize panel snapshot response");
        match bincode::deserialize::<PluginResponse>(&bytes).expect("deserialize response") {
            PluginResponse::PanelSnapshot(Err(error)) => assert_eq!(error, "unsupported"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    // -- v23 scene/widget wire tails (SCN-004) ------------------------------

    fn one_rect_element(id: u64) -> SceneElement {
        SceneElement::Rect {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            pos: crate::scene::Vec2([0.0, 0.0]),
            size: crate::scene::Vec2([1.0, 1.0]),
            fill: None,
            stroke: None,
        }
    }

    fn one_rect_scene(revision: u64) -> PanelScene {
        PanelScene {
            revision: SceneRevision(revision),
            elements: vec![one_rect_element(1)],
        }
    }

    fn one_widget_surface(revision: u64) -> WidgetSurface {
        WidgetSurface {
            revision: SceneRevision(revision),
            roots: Vec::new(),
            tables: std::collections::BTreeMap::new(),
        }
    }

    fn one_cell_widget_patch() -> WidgetSurfacePatch {
        WidgetSurfacePatch {
            base_revision: SceneRevision(1),
            revision: SceneRevision(2),
            ops: vec![crate::widgets::WidgetOp::TableCell {
                table: StableId(1),
                key: StableId(2),
                column: StableId(3),
                value: crate::widgets::WidgetCellValue::Number(1234.5),
            }],
        }
    }

    /// The JSON route must tolerate every new tail too (the additive
    /// tolerance discipline: bincode is transport truth; JSON is the
    /// debugging/fixture route). The wire enums are deliberately not
    /// `PartialEq` (a payload type does not carry it), so wire stability is
    /// asserted byte-wise instead.
    fn assert_json_wire_stable<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) {
        let round: T = json_roundtrip(value);
        let original = bincode::serialize(value).expect("bincode wire (original)");
        let again = bincode::serialize(&round).expect("bincode wire (after JSON roundtrip)");
        assert_eq!(
            original, again,
            "the JSON route must keep the bincode wire bytes"
        );
    }

    fn json_roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
        let json = serde_json::to_value(value).expect("serialize JSON");
        serde_json::from_value(json).expect("deserialize JSON")
    }

    #[test]
    fn v23_request_tails_roundtrip_bincode_and_json() {
        // PanelScenePublish (61): scene leg, widget leg, event batch.
        let publish = PluginRequest::PanelScenePublish {
            panel_id: "align_tools".to_owned(),
            surface: SceneOrWidgetSurface::Scene(one_rect_scene(1)),
            events: vec![crate::widgets::WidgetEvent::TreeExpanded {
                surface_id: StableId(7),
                node_id: StableId(8),
                expanded: true,
            }],
        };
        let bytes = bincode::serialize(&publish).expect("serialize publish");
        match bincode::deserialize::<PluginRequest>(&bytes).expect("decode publish") {
            PluginRequest::PanelScenePublish {
                panel_id,
                surface,
                events,
            } => {
                assert_eq!(panel_id, "align_tools");
                assert_eq!(surface, SceneOrWidgetSurface::Scene(one_rect_scene(1)));
                assert_eq!(
                    events,
                    vec![crate::widgets::WidgetEvent::TreeExpanded {
                        surface_id: StableId(7),
                        node_id: StableId(8),
                        expanded: true,
                    }]
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
        let round = json_roundtrip(&publish);
        assert_json_wire_stable(&round);
        assert!(matches!(round, PluginRequest::PanelScenePublish { .. }));

        let publish_widgets = PluginRequest::PanelScenePublish {
            panel_id: "align_tools".to_owned(),
            surface: SceneOrWidgetSurface::Widgets(one_widget_surface(4)),
            events: Vec::new(),
        };
        let bytes = bincode::serialize(&publish_widgets).expect("serialize widget publish");
        match bincode::deserialize::<PluginRequest>(&bytes).expect("decode widget publish") {
            PluginRequest::PanelScenePublish { surface, .. } => {
                assert_eq!(surface, SceneOrWidgetSurface::Widgets(one_widget_surface(4)));
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_json_wire_stable(&publish_widgets);

        // PanelScenePatch (62): both envelope legs round-trip.
        let patch = PluginRequest::PanelScenePatch {
            panel_id: "align_tools".to_owned(),
            patch: SceneOrWidgetPatch::Scene(crate::scene::PanelScenePatch {
                base_revision: SceneRevision(1),
                revision: SceneRevision(2),
                ops: vec![crate::scene::SceneOp::Add {
                    parent: None,
                    element: one_rect_element(2),
                }],
            }),
        };
        let bytes = bincode::serialize(&patch).expect("serialize scene patch request");
        match bincode::deserialize::<PluginRequest>(&bytes).expect("decode scene patch request") {
            PluginRequest::PanelScenePatch { panel_id, patch } => {
                assert_eq!(panel_id, "align_tools");
                assert_eq!(
                    patch,
                    SceneOrWidgetPatch::Scene(crate::scene::PanelScenePatch {
                        base_revision: SceneRevision(1),
                        revision: SceneRevision(2),
                        ops: vec![crate::scene::SceneOp::Add {
                            parent: None,
                            element: one_rect_element(2),
                        }],
                    })
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_json_wire_stable(&patch);

        let widget_patch = PluginRequest::PanelScenePatch {
            panel_id: "tables".to_owned(),
            patch: SceneOrWidgetPatch::Widgets(one_cell_widget_patch()),
        };
        let bytes = bincode::serialize(&widget_patch).expect("serialize widget patch request");
        match bincode::deserialize::<PluginRequest>(&bytes).expect("decode widget patch request") {
            PluginRequest::PanelScenePatch {
                patch: SceneOrWidgetPatch::Widgets(widget_surface_patch),
                ..
            } => {
                assert_eq!(widget_surface_patch, one_cell_widget_patch());
            }
            other => panic!("unexpected: {other:?}"),
        }

        // PanelStoreRead (63): every store kind, plus the range decode.
        for kind in [
            PanelStoreKind::TreeChildren,
            PanelStoreKind::ListItems,
            PanelStoreKind::TableRows,
            PanelStoreKind::SceneRegion,
        ] {
            let read = PluginRequest::PanelStoreRead {
                panel_id: "align_tools".to_owned(),
                kind,
                parent_id: Some(StableId(9)),
                range: WidgetRange { start: 12, len: 3 },
            };
            let bytes = bincode::serialize(&read).expect("serialize store read");
            match bincode::deserialize::<PluginRequest>(&bytes).expect("decode store read") {
                PluginRequest::PanelStoreRead {
                    panel_id,
                    kind: decoded_kind,
                    parent_id,
                    range,
                } => {
                    assert_eq!(panel_id, "align_tools");
                    assert_eq!(decoded_kind, kind);
                    assert_eq!(parent_id, Some(StableId(9)));
                    assert_eq!(range, WidgetRange { start: 12, len: 3 });
                }
                other => panic!("unexpected: {other:?}"),
            }
            assert_json_wire_stable(&read);
        }

        // PanelSceneSnapshot2 (64).
        let snapshot2 = PluginRequest::PanelSceneSnapshot2 {
            panel_id: "align_tools".to_owned(),
        };
        let bytes = bincode::serialize(&snapshot2).expect("serialize snapshot2");
        match bincode::deserialize::<PluginRequest>(&bytes).expect("decode snapshot2") {
            PluginRequest::PanelSceneSnapshot2 { panel_id } => assert_eq!(panel_id, "align_tools"),
            other => panic!("unexpected: {other:?}"),
        }
        assert_json_wire_stable(&snapshot2);
    }

    #[test]
    fn v23_response_tails_roundtrip_bincode_and_json() {
        // PatchApplied (47): a REAL report from the existing apply path.
        let mut scene = one_rect_scene(1);
        let report = scene
            .apply_patch_report(crate::scene::PanelScenePatch {
                base_revision: SceneRevision(1),
                revision: SceneRevision(2),
                ops: vec![crate::scene::SceneOp::Add {
                    parent: None,
                    element: one_rect_element(2),
                }],
            })
            .expect("valid patch applies with a report");
        assert_eq!(report.revision, SceneRevision(2));
        let applied = PluginResponse::PatchApplied(report.clone());
        let bytes = bincode::serialize(&applied).expect("serialize PatchApplied");
        match bincode::deserialize::<PluginResponse>(&bytes).expect("decode PatchApplied") {
            PluginResponse::PatchApplied(decoded) => {
                assert_eq!(decoded, report);
                assert_eq!(decoded.summary.revision, SceneRevision(2));
                assert_eq!(decoded.summary.nodes, 2);
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_json_wire_stable(&applied);

        // PatchRejected (48): structured, bounded, every code round-trips.
        for code in [
            PatchRejectCode::StaleBase,
            PatchRejectCode::NonMonotonicRevision,
            PatchRejectCode::OpBudgetExceeded,
            PatchRejectCode::InvalidOp,
            PatchRejectCode::PublisherUnattached,
        ] {
            let rejected = PluginResponse::PatchRejected {
                code,
                report: PatchRejectReport {
                    current_revision: Some(SceneRevision(7)),
                    failed_op: Some(3),
                },
            };
            let bytes = bincode::serialize(&rejected).expect("serialize PatchRejected");
            match bincode::deserialize::<PluginResponse>(&bytes).expect("decode PatchRejected") {
                PluginResponse::PatchRejected {
                    code: decoded_code,
                    report,
                } => {
                    assert_eq!(decoded_code, code);
                    assert_eq!(report.current_revision, Some(SceneRevision(7)));
                    assert_eq!(report.failed_op, Some(3));
                }
                other => panic!("unexpected: {other:?}"),
            }
            assert_json_wire_stable(&rejected);
        }

        // StorePage (49): a digest-led recovery page and an empty fat-finger page.
        let page = PluginResponse::StorePage {
            rows: vec![
                StoreRow::State(StoreStateDigest {
                    scene: Some(one_rect_scene(5).scene_summary()),
                    widgets: Some(one_widget_surface(5).widget_summary()),
                }),
                StoreRow::Scene(one_rect_element(2)),
            ],
            total_hint: 2,
        };
        let bytes = bincode::serialize(&page).expect("serialize StorePage");
        match bincode::deserialize::<PluginResponse>(&bytes).expect("decode StorePage") {
            PluginResponse::StorePage { rows, total_hint } => {
                assert_eq!(total_hint, 2);
                assert_eq!(rows.len(), 2);
                match &rows[0] {
                    StoreRow::State(digest) => {
                        let scene = digest.scene.as_ref().expect("scene digest");
                        assert_eq!(scene.revision, SceneRevision(5));
                        assert_eq!(scene.nodes, 1);
                        let widgets = digest.widgets.as_ref().expect("widget digest");
                        assert_eq!(widgets.revision, SceneRevision(5));
                    }
                    other => panic!("unexpected: {other:?}"),
                }
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_json_wire_stable(&page);
    }

    #[test]
    fn v23_envelope_and_reject_enums_are_pinned_from_creation() {
        fn discriminant<T: serde::Serialize>(value: &T) -> u32 {
            let bytes = bincode::serialize(value).expect("serialize orientation pin");
            u32::from_le_bytes(bytes[..4].try_into().expect("bincode tag"))
        }
        // SceneOrWidgetSurface ..=1; SceneOrWidgetPatch ..=1:
        assert_eq!(discriminant(&SceneOrWidgetSurface::Scene(one_rect_scene(1))), 0);
        assert_eq!(
            discriminant(&SceneOrWidgetSurface::Widgets(one_widget_surface(1))),
            1
        );
        assert_eq!(
            discriminant(&SceneOrWidgetPatch::Scene(crate::scene::PanelScenePatch {
                base_revision: SceneRevision(1),
                revision: SceneRevision(1),
                ops: Vec::new(),
            })),
            0
        );
        assert_eq!(discriminant(&SceneOrWidgetPatch::Widgets(one_cell_widget_patch())), 1);
        // PanelStoreKind ..=1..=3:
        assert_eq!(discriminant(&PanelStoreKind::TreeChildren), 0);
        assert_eq!(discriminant(&PanelStoreKind::ListItems), 1);
        assert_eq!(discriminant(&PanelStoreKind::TableRows), 2);
        assert_eq!(discriminant(&PanelStoreKind::SceneRegion), 3);
        // StoreRow ..=4 with State FIRST (the digest row is the "header"
        // semantic row; pinning it 0 now keeps its future insertions honest):
        assert_eq!(
            discriminant(&StoreRow::State(StoreStateDigest {
                scene: None,
                widgets: None
            })),
            0
        );
        assert_eq!(discriminant(&StoreRow::Scene(one_rect_element(1))), 1);
        assert_eq!(
            discriminant(&StoreRow::Tree(crate::widgets::TreeRowDef {
                id: StableId(1),
                parent: None,
                label: "row".to_owned(),
                icon: None,
                status: None,
                expandable: false,
                enabled: true,
                lazy_children: false,
                domain_ref: None,
            })),
            2
        );
        assert_eq!(
            discriminant(&StoreRow::Item(crate::widgets::ItemDef {
                id: StableId(1),
                label: "item".to_owned(),
                icon: None,
                enabled: true,
                visible: true,
                command: None,
                domain_ref: None,
            })),
            3
        );
        assert_eq!(
            discriminant(&StoreRow::Table(crate::widgets::TableRowDef {
                key: StableId(1),
                cells: vec![],
                domain_ref: None,
            })),
            4
        );
        // PatchRejectCode ..=5:
        assert_eq!(discriminant(&PatchRejectCode::StaleBase), 0);
        assert_eq!(discriminant(&PatchRejectCode::NonMonotonicRevision), 1);
        assert_eq!(discriminant(&PatchRejectCode::OpBudgetExceeded), 2);
        assert_eq!(discriminant(&PatchRejectCode::InvalidOp), 3);
        assert_eq!(discriminant(&PatchRejectCode::PublisherUnattached), 4);
        assert_eq!(discriminant(&PatchRejectCode::PublisherOwnedByOther), 5);
    }

    #[test]
    fn publish_apply_ack_happy_path_through_the_wire() {
        // The in-crate transport happy path: a valid publish decodes from
        // bincode wire bytes; the patch leg applies through the EXISTING
        // atomic `apply_patch*` registry machinery; the ack encodes as the
        // typed PatchApplied with the bounded report; a stale base answers
        // the structured PatchRejected and leaves the scene untouched.
        crate::scene::scene_decode_frame_begin();
        crate::widgets::widget_decode_frame_begin();

        // 1) publish: wire -> decode -> hold (the test stands in for the
        // follow-up publisher's registry; the frame never carries more).
        let publish = PluginRequest::PanelScenePublish {
            panel_id: "align_tools".to_owned(),
            surface: SceneOrWidgetSurface::Scene(one_rect_scene(1)),
            events: Vec::new(),
        };
        let bytes = bincode::serialize(&publish).expect("wire publish");
        let decoded = bincode::deserialize::<PluginRequest>(&bytes).expect("decode publish");
        let mut hold = match decoded {
            PluginRequest::PanelScenePublish { surface, .. } => match surface {
                SceneOrWidgetSurface::Scene(scene) => scene,
                other => panic!("unexpected: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        };

        // 2) patch with base_revision == the published revision: applies.
        let patch = PluginRequest::PanelScenePatch {
            panel_id: "align_tools".to_owned(),
            patch: SceneOrWidgetPatch::Scene(crate::scene::PanelScenePatch {
                base_revision: SceneRevision(1),
                revision: SceneRevision(2),
                ops: vec![crate::scene::SceneOp::Add {
                    parent: None,
                    element: one_rect_element(2),
                }],
            }),
        };
        crate::scene::scene_decode_frame_begin();
        let bytes = bincode::serialize(&patch).expect("wire patch");
        let decoded = bincode::deserialize::<PluginRequest>(&bytes).expect("decode patch");
        let report = match decoded {
            PluginRequest::PanelScenePatch {
                patch: SceneOrWidgetPatch::Scene(scene_patch),
                ..
            } => hold
                .apply_patch_report(scene_patch)
                .expect("patch with fresh base applies"),
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(report.revision, SceneRevision(2));
        assert_eq!(report.summary.nodes, 2);
        assert_eq!(
            report.summary.root_ids,
            vec![StableId(1), StableId(2)],
            "the add appended a second root; the original root stays first"
        );
        let ack = bincode::serialize(&PluginResponse::PatchApplied(report.clone()))
            .expect("wire ack");
        match bincode::deserialize::<PluginResponse>(&ack).expect("decode ack") {
            PluginResponse::PatchApplied(decoded_report) => {
                assert_eq!(decoded_report, report);
            }
            other => panic!("unexpected: {other:?}"),
        }

        // 3) patch built against the OLD revision: structured rejection,
        // and the held scene does NOT move (no mutation, no panic).
        let stale = PluginRequest::PanelScenePatch {
            panel_id: "align_tools".to_owned(),
            patch: SceneOrWidgetPatch::Scene(crate::scene::PanelScenePatch {
                base_revision: SceneRevision(1),
                revision: SceneRevision(9),
                ops: vec![crate::scene::SceneOp::Add {
                    parent: None,
                    element: one_rect_element(30),
                }],
            }),
        };
        crate::scene::scene_decode_frame_begin();
        let bytes = bincode::serialize(&stale).expect("wire stale patch");
        let decoded = bincode::deserialize::<PluginRequest>(&bytes).expect("decode stale");
        let rejection = match decoded {
            PluginRequest::PanelScenePatch {
                patch: SceneOrWidgetPatch::Scene(scene_patch),
                ..
            } => {
                let before = hold.clone();
                let outcome = hold.apply_patch_report(scene_patch);
                let error = outcome.expect_err("a stale base must be rejected");
                assert_eq!(
                    hold, before,
                    "a rejected patch leaves the held scene exactly as it was"
                );
                error
            }
            other => panic!("unexpected: {other:?}"),
        };
        // The typed rejection the transport answers with: machine code +
        // the bounded report (the head revision), no payload dump in sight.
        let rejected = PluginResponse::PatchRejected {
            code: PatchRejectCode::StaleBase,
            report: PatchRejectReport {
                current_revision: Some(hold.revision),
                failed_op: None,
            },
        };
        let _ = rejection;
        let bytes = bincode::serialize(&rejected).expect("wire rejection");
        match bincode::deserialize::<PluginResponse>(&bytes).expect("decode rejection") {
            PluginResponse::PatchRejected { code, report } => {
                assert_eq!(code, PatchRejectCode::StaleBase);
                assert_eq!(report.current_revision, Some(SceneRevision(2)));
                assert_eq!(report.failed_op, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
        // The hold is exactly what the PUBLISHED revision plus the applied
        // patch left it as:
        assert_eq!(hold.revision, SceneRevision(2));
        assert_eq!(hold.elements.len(), 2);
    }

    #[test]
    fn v23_wire_shapes_stay_minimal() {
        // The incremental wire shape is NOT proportional to the carried
        // state: a one-op patch request is constant bytes (the 40-72B
        // single-fact family here because the request envelope adds the
        // panel id and one nested tag). Both facts below are measured and
        // pinned so the family cannot silently grow.
        crate::scene::scene_decode_frame_begin();
        let one = PluginRequest::PanelScenePatch {
            panel_id: "p".to_owned(),
            patch: SceneOrWidgetPatch::Widgets(one_cell_widget_patch()),
        };
        let bytes = bincode::serialize(&one).expect("serialize one-cell request");
        let request_bytes = bytes.len();
        let bare_patch_bytes = bincode::serialize(&one_cell_widget_patch())
            .expect("serialize bare patch")
            .len();
        // The bare one-cell patch: the SWG-004 72B-family wire fact.
        assert!(
            bare_patch_bytes <= 96,
            "bare one-cell patch = {bare_patch_bytes} bytes (the SWG-004 fact family)"
        );
        // The wire REQUEST adds the request tag + the envelope tag + the
        // panel id (constants) — a small fixed family, never scene-sized.
        let envelope_overhead = request_bytes - bare_patch_bytes;
        assert!(
            request_bytes <= 160,
            "one-cell patch REQUEST = {request_bytes} bytes; envelope overhead = \
             {envelope_overhead} bytes (fixed, panel-id sized)"
        );
        println!(
            "v23 wire facts: bare one-cell patch = {bare_patch_bytes} bytes; \
             patch REQUEST = {request_bytes} bytes (envelope overhead = {envelope_overhead} bytes)"
        );

        // The recovery answer is bounded REGARDLESS of scene size: the
        // snapshot leg's page (digest + leading window) tops out at the
        // rows-per-payload cap, and its bytes do not grow with the scene.
        let digest = || StoreRow::State(StoreStateDigest {
            scene: Some(PanelScene {
                revision: SceneRevision(3),
                elements: Vec::new(),
            }
            .scene_summary()),
            widgets: None,
        });
        // The full window at the exact payload cap: the worst legal page.
        let worst_rows: Vec<StoreRow> = (0..MAX_WIDGET_ROWS_INITIAL)
            .map(|i| StoreRow::Scene(one_rect_element(i as u64 + 1)))
            .collect();
        let page_bytes = {
            let page = PluginResponse::StorePage {
                rows: {
                    let mut rows = vec![digest()];
                    rows.extend(worst_rows.iter().cloned());
                    rows
                },
                total_hint: MAX_WIDGET_ROWS_INITIAL as u64,
            };
            bincode::serialize(&page).expect("serialize worst page").len()
        };
        println!("v23 wire facts: worst snapshot page = {page_bytes} bytes");
        assert!(
            page_bytes < 1_000_000,
            "worst bounded page = {page_bytes} bytes must stay well under the frame cap"
        );
        // The snapshot REQUEST itself is tiny and constant-size:
        let snapshot_request = bincode::serialize(&PluginRequest::PanelSceneSnapshot2 {
            panel_id: "p".to_owned(),
        })
            .expect("serialize snapshot2 request");
        // tag(4) + panel_id(8 + 1) = 13 bytes: a recovery ask never costs
        // scene bytes.
        assert!(
            snapshot_request.len() <= 32,
            "snapshot2 request = {} bytes",
            snapshot_request.len()
        );
    }

    #[test]
    fn v23_decode_budgets_refuse_gracefully() {
        // Every budget is enforced at DECODE: over-budget requests answer
        // a structured Decode error, never a bigger page, never a panic.
        // (The budgets are the Q4-approved numbers.)
        crate::scene::scene_decode_frame_begin();
        crate::widgets::widget_decode_frame_begin();

        // 1) event batch: past MAX_WIDGET_EVENT_ROWS rows refuses.
        let fat = PluginRequest::PanelScenePublish {
            panel_id: "p".to_owned(),
            surface: SceneOrWidgetSurface::Widgets(one_widget_surface(1)),
            events: (0..MAX_WIDGET_EVENT_ROWS + 1)
                .map(|i| crate::widgets::WidgetEvent::TreeExpanded {
                    surface_id: StableId(7),
                    node_id: StableId(i as u64),
                    expanded: false,
                })
                .collect(),
        };
        let bytes = bincode::serialize(&fat).expect("serialize fat publish");
        assert!(
            bincode::deserialize::<PluginRequest>(&bytes).is_err(),
            "a past-cap event batch must refuse at decode"
        );
        // At exactly the cap it still decodes:
        crate::widgets::widget_decode_frame_begin();
        let at_cap = PluginRequest::PanelScenePublish {
            panel_id: "p".to_owned(),
            surface: SceneOrWidgetSurface::Widgets(one_widget_surface(1)),
            events: (0..MAX_WIDGET_EVENT_ROWS)
                .map(|i| crate::widgets::WidgetEvent::TreeExpanded {
                    surface_id: StableId(7),
                    node_id: StableId(i as u64),
                    expanded: false,
                })
                .collect(),
        };
        let bytes = bincode::serialize(&at_cap).expect("serialize at-cap publish");
        assert!(
            bincode::deserialize::<PluginRequest>(&bytes).is_ok(),
            "at-cap event batch is legal"
        );

        // 2) store page: past MAX_WIDGET_ROWS_INITIAL rows refuses.
        let fat_page = PluginResponse::StorePage {
            rows: (0..MAX_WIDGET_ROWS_INITIAL + 1)
                .map(|i| StoreRow::Scene(one_rect_element(i as u64)))
                .collect(),
            total_hint: u64::MAX,
        };
        let bytes = bincode::serialize(&fat_page).expect("serialize fat page");
        assert!(
            bincode::deserialize::<PluginResponse>(&bytes).is_err(),
            "a past-cap page must refuse at decode"
        );

        // 3) store read range: width past MAX_WIDGET_RANGE_LEN refuses.
        let fat_range = PluginRequest::PanelStoreRead {
            panel_id: "p".to_owned(),
            kind: PanelStoreKind::SceneRegion,
            parent_id: None,
            range: WidgetRange {
                start: 0,
                len: MAX_WIDGET_RANGE_LEN + 1,
            },
        };
        let bytes = bincode::serialize(&fat_range).expect("serialize fat range");
        assert!(
            bincode::deserialize::<PluginRequest>(&bytes).is_err(),
            "an over-cap read width must refuse at decode"
        );

        // 4) panel_id: past MAX_PANEL_ID_BYTES bytes refuses.
        let fat_id = PluginRequest::PanelSceneSnapshot2 {
            panel_id: "x".repeat(MAX_PANEL_ID_BYTES + 1),
        };
        let bytes = bincode::serialize(&fat_id).expect("serialize fat id");
        assert!(
            bincode::deserialize::<PluginRequest>(&bytes).is_err(),
            "a past-cap panel_id must refuse at decode"
        );

        // 5) notification direction: the same caps hold on the
        // host->plugin tails (the crate encodes BOTH sides).
        let fat_notification = crate::host::HostNotification::PanelSceneEvents {
            panel_id: "p".to_owned(),
            surface_id: StableId(7),
            events: (0..MAX_WIDGET_EVENT_ROWS + 1)
                .map(|i| crate::widgets::WidgetEvent::TreeExpanded {
                    surface_id: StableId(7),
                    node_id: StableId(i as u64),
                    expanded: false,
                })
                .collect(),
        };
        let bytes = bincode::serialize(&fat_notification).expect("serialize fat notification");
        assert!(
            bincode::deserialize::<crate::host::HostNotification>(&bytes).is_err(),
            "a past-cap notification batch must refuse at decode"
        );
        let fat_demand = crate::host::HostNotification::PanelStoreDemand {
            panel_id: "p".to_owned(),
            surface_id: StableId(7),
            parent_id: None,
            range: WidgetRange {
                start: 0,
                len: MAX_WIDGET_RANGE_LEN + 1,
            },
        };
        let bytes = bincode::serialize(&fat_demand).expect("serialize fat demand");
        assert!(
            bincode::deserialize::<crate::host::HostNotification>(&bytes).is_err(),
            "an over-cap demand width must refuse at decode"
        );
    }

    #[test]
    fn v23_malformed_and_truncated_frames_reject_without_panics() {
        // Garbage discriminants and truncated payloads decode to a
        // structured error — never a panic, never a variant.
        crate::scene::scene_decode_frame_begin();

        let patch = PluginRequest::PanelScenePatch {
            panel_id: "p".to_owned(),
            patch: SceneOrWidgetPatch::Widgets(one_cell_widget_patch()),
        };
        let bytes = bincode::serialize(&patch).expect("serialize patch");
        // Truncated frame: cut in half; structured Decode error, not a panic.
        let cut = &bytes[..bytes.len() / 2];
        let decoded = bincode::deserialize::<PluginRequest>(cut);
        assert!(decoded.is_err(), "truncated frame refuses");
        // The trusted end of the same frame (missing trailing bytes only)
        // also refuses gracefully.
        let tail_cut = &bytes[..bytes.len() - 1];
        assert!(bincode::deserialize::<PluginRequest>(tail_cut).is_err());

        // Unknown/garbage request discriminant:
        let mut garbage = Vec::new();
        garbage.extend_from_slice(&777_777_u32.to_le_bytes());
        garbage.extend_from_slice(&0_u64.to_le_bytes());
        assert!(
            bincode::deserialize::<PluginRequest>(&garbage).is_err(),
            "an unknown request discriminant refuses"
        );
        // Empty frame:
        assert!(bincode::deserialize::<PluginRequest>(&[]).is_err());

        // Responses: same for the new tails.
        let applied = PluginResponse::PatchApplied(crate::scene::AppliedPatchReport {
            revision: SceneRevision(2),
            touched_subtree_ids: vec![StableId(1)],
            touched_truncated: 0,
            retired_ids: Vec::new(),
            retired_truncated: 0,
            summary: one_rect_scene(2).scene_summary(),
        });
        let bytes = bincode::serialize(&applied).expect("serialize applied");
        let cut = &bytes[..bytes.len() / 2];
        assert!(bincode::deserialize::<PluginResponse>(cut).is_err());
        // And the scene-frame reset keeps this decode under budget even
        // though the earlier hostile work in this test busted nothing.
        crate::scene::scene_decode_frame_begin();
        let decoded =
            bincode::deserialize::<PluginResponse>(&bytes).expect("intact frame decodes");
        match decoded {
            PluginResponse::PatchApplied(report) => {
                assert_eq!(report.summary.revision, SceneRevision(2));
            }
            other => panic!("unexpected: {other:?}"),
        }

        // Notifications: garbage tag -> the Unknown passthrough (decode
        // NEVER fails on unknown tags; that is the forward-compat law), and
        // truncated 15/16 payloads fail the structured way (no panic).
        // (HostNotification customs serialize as a Vec<u8>: u64 length
        // prefix, then tag byte + payload.)
        let mut garbage = Vec::new();
        garbage.extend_from_slice(&2_u64.to_le_bytes()); // Vec len = 2
        garbage.push(200); // unknown tag
        garbage.push(0);
        let decoded = bincode::deserialize::<crate::host::HostNotification>(&garbage)
            .expect("unknown tag survives as Unknown");
        match decoded {
            crate::host::HostNotification::Unknown(raw) => assert_eq!(raw[0], 200),
            other => panic!("unexpected: {other:?}"),
        }
        // Truncated 15 payload: the claimed panel bytes never arrive.
        let mut truncated_events = Vec::new();
        truncated_events.extend_from_slice(&10_u64.to_le_bytes()); // Vec len = 10
        truncated_events.push(15); // PanelSceneEvents
        truncated_events.extend_from_slice(&6_u64.to_le_bytes()); // panel_id claims 6 bytes
        truncated_events.push(112); // "p" — then the frame ends
        assert!(bincode::deserialize::<crate::host::HostNotification>(&truncated_events).is_err());
    }
}
