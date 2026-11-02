//! Runtime host surface (`host` feature).
//!
//! [`HostApi`] is the `opencadcodec`-typed adapter a plugin uses at *dispatch* time
//! — document access, entity creation, XDATA, undo, and the command line. It is
//! the stable counterpart to the dependency-free manifest/ribbon contract: a
//! plugin's `dispatch` receives `&mut dyn HostApi` rather than the host's
//! concrete session type, so an out-of-tree add-on compiles against this crate
//! alone.
//!
//! Per-tab plugin state is keyed by `manifest.id`. The trait exposes it in an
//! object-safe `Any` form; use the [`plugin_state`], [`plugin_state_mut`] and
//! [`ensure_plugin_state`] helpers for the ergonomic typed access.

use std::any::Any;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::manifest::{EntityClasses, PluginManifest};
use crate::ribbon::CadModule;

// Re-export the opencadcodec crate and the types that appear in the HostApi trait
// so out-of-tree plugins can use them without adding their own opencadcodec
// dependency (which would risk an ABI-mismatching version).
pub use codec;
pub use codec as acadrust;
pub use codec::objects::{
    DictionaryCloningFlags, KnownXRecordKind, ProxyObjectReference, ProxyReferenceKind, XRecord,
    XRecordEntry, XRecordSection, XRecordValue, XRecordValueType,
};
pub use codec::xdata::{ExtendedDataRecord, XDataValue};
pub use codec::{Document, EntityType, Handle};

use crate::ipc::protocol::{PluginRequest, PluginResponse};

/// Thread-safe handle that can issue host requests from plugin worker threads.
/// Out-of-process V4 plugins implement this; in-process hosts may return `None`.
pub trait PluginRequestSender: Send + Sync {
    fn request(&self, req: PluginRequest) -> Result<PluginResponse, PluginRequestError>;
}

/// One page of [`HostApi::query_entities`] (API v14).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityPage {
    pub entities: Vec<EntityType>,
    /// Matches in the whole drawing, across all pages.
    pub count: u64,
    /// The `offset` of the next page; `None` on the last one.
    pub next_offset: Option<u64>,
}

/// Error returned when a plugin worker thread cannot issue a host request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRequestError(pub String);

impl std::fmt::Display for PluginRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PluginRequestError: {}", self.0)
    }
}

impl std::error::Error for PluginRequestError {}

/// Log level carried by [`PluginNotification::Log`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// A solid primitive the host can build with its geometry kernel. Lengths are
/// in drawing units; `center`/`origin` are world coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SolidPrimitive {
    /// Axis-aligned box from its center and full extents.
    Box {
        center: [f64; 3],
        size: [f64; 3],
    },
    /// Right triangular prism from its minimum corner and extents.
    Wedge {
        origin: [f64; 3],
        size: [f64; 3],
    },
    /// Cylinder standing on the plane through `center`.
    Cylinder {
        center: [f64; 3],
        radius: f64,
        height: f64,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
    Torus {
        center: [f64; 3],
        major: f64,
        minor: f64,
    },
    /// Regular pyramid with `sides` base edges.
    Pyramid {
        center: [f64; 3],
        radius: f64,
        height: f64,
        sides: u32,
    },
}

/// How two solids combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolidBoolean {
    Union,
    /// Remove the second solid from the first.
    Subtract,
    Intersect,
}

/// One pick of a [`PluginConstraint`]: the entity (or the point, for a
/// free pick), the world point near the pick, and whether the whole curve
/// (not a specific point on it) is meant. Resolution follows the interactive
/// COINCIDENT/TANGENT pick logic (API v13, additive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginConstraintPick {
    #[serde(default)]
    pub handle: Option<Handle>,
    pub point: [f64; 3],
    #[serde(default)]
    pub whole_curve: bool,
}

/// A parametric constraint created through [`HostApi::constraint_create`]
/// (API v13, additive): the same constraint machinery the interactive
/// COINCIDENT (`GCCOINCIDENT`) and TANGENT (`TCONSTRAINT`) commands drive,
/// reached directly — no command line, no prompt echo, one undo step under
/// the entity-write contract (inside a plugin-opened `push_undo` scope the
/// constraint joins that scope).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginConstraint {
    /// A coincident constraint between the two picks.
    Coincident {
        first: PluginConstraintPick,
        second: PluginConstraintPick,
    },
    /// A tangency constraint between the two picks.
    Tangent {
        first: PluginConstraintPick,
        second: PluginConstraintPick,
    },
}

/// One step of driving an OCS command from a script (API v7, additive). The
/// host runs the real command, so every tool behaves exactly as at the command
/// line, and answers with where the command stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommandRequest {
    /// Run a whole command line (`OFFSET 5 ...`): the tokens after the name
    /// answer the prompts in order and a final Enter finishes the command.
    Run { line: String },
    /// Start a command and leave it waiting for its first input.
    Start { name: String },
    /// Answer the current prompt with a world-coordinate point.
    Point { point: [f64; 3] },
    /// Answer with typed text (a distance, an angle, a name).
    Text { text: String },
    /// Answer with a keyword option.
    Token { text: String },
    /// Pick an entity at a point.
    Entity { handle: Handle, point: [f64; 3] },
    /// Complete an object-selection prompt with the current selection.
    Selection,
    /// Press Enter.
    Enter,
    /// Cancel the running command and close anything it left open.
    Cancel,
    /// API v13 (additive): run a whole command line like [`Run`](Self::Run),
    /// but with the COMMAND-LINE ECHO SUPPRESSED for the machine replay — no
    /// prompts, no echo of internal machinery ("COINCIDENT Select first
    /// point...", "SETVAR Bezeichnung der Variablen:"...). For plugin re-bond
    /// and rebuild replays whose noise the user must not see; errors still
    /// surface. The replay's undo composition is unchanged.
    RunQuiet { line: String },
}

/// Where a command stands after a [`CommandRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CommandOutcome {
    /// `completed` when nothing is waiting, `waiting_input` when the command
    /// (or an editor or dialog it opened) needs more.
    pub status: String,
    /// What is still open: `command`, `text_editor`, `mtext_editor` or `modal:<kind>`.
    pub blocked_by: Option<String>,
    /// The waiting command's name and prompt (empty when none).
    pub command: String,
    pub prompt: String,
    /// The kinds of input the prompt accepts (`point`, `entity`, `selection`,
    /// `text`, `token`, `enter`) and its keyword options.
    pub accepts: Vec<String>,
    pub options: Vec<String>,
    /// Entities in the drawing after the step, and the change.
    pub entities: u64,
    pub added: i64,
    /// Typed tokens no prompt asked for.
    pub unconsumed: Vec<String>,
    /// The command line's error message when the step failed.
    pub error: Option<String>,
}

/// A change to a drawing table record (API v7, additive). The host validates
/// the request, records one undo step and refuses without changing anything
/// when it cannot honour it; the result is the record's handle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TableOperation {
    /// Create a layer. `config.name` must be unused and valid.
    LayerCreate { config: LayerConfig },
    /// Change only the properties `config` sets on an existing layer.
    LayerModify { config: LayerConfig },
    /// Rename a layer; entities on it follow. Layer `0` and `Defpoints` stay.
    LayerRename { from: String, to: String },
    /// Delete a layer. A layer that still holds objects is refused unless
    /// `erase_objects` is set, which erases them with the layer. Layer `0`,
    /// `Defpoints` and the current layer are never deleted.
    LayerDelete { name: String, erase_objects: bool },
    /// Make an existing layer current.
    LayerSetCurrent { name: String },
    /// Create a text style; unset properties take the AutoCAD defaults.
    TextStyleCreate { config: TextStyleConfig },
    /// Change only the properties `config` sets on an existing text style.
    TextStyleModify { config: TextStyleConfig },
    /// Create a dimension style, optionally copying an existing one first.
    /// `properties` is a JSON object of DimStyle fields (see the Python docs);
    /// handles and xref fields are managed by the host and refused.
    DimStyleCreate {
        name: String,
        copy_from: Option<String>,
        properties: String,
    },
    /// Change only the DimStyle fields the JSON object `properties` names.
    DimStyleModify { name: String, properties: String },
    /// Rename a text or dimension style; references to it follow. `Standard`
    /// is never renamed and a case-only change is refused.
    StyleRename {
        kind: TableStyleKind,
        from: String,
        to: String,
    },
    /// Delete a text or dimension style that is neither current nor in use.
    /// `Standard` is never deleted.
    StyleDelete { kind: TableStyleKind, name: String },
    /// Make a text or dimension style current.
    StyleSetCurrent { kind: TableStyleKind, name: String },
    /// Define a block from existing drawing entities. The entities are copied
    /// into the definition shifted by `-base_point` (the block's origin);
    /// `erase_originals` then removes them from the drawing. No insert is
    /// placed. Returns the block record's handle.
    BlockCreate {
        name: String,
        entities: Vec<Handle>,
        base_point: [f64; 3],
        erase_originals: bool,
        description: Option<String>,
    },
    /// Change a block's description, explodable or uniform-scale settings.
    BlockModify {
        name: String,
        description: Option<String>,
        explodable: Option<bool>,
        scale_uniformly: Option<bool>,
    },
    /// Rename a block; every insert of it follows.
    BlockRename { from: String, to: String },
    /// Delete an unreferenced block definition and its contents.
    BlockDelete { name: String },
    /// Add a new entity to a block definition, validated exactly as a new
    /// model-space entity is. The entity's owner is set by the host. Returns the
    /// new entity's handle. Edit and delete members with the ordinary entity
    /// operations.
    BlockEntityAdd { block: String, entity: EntityType },
    /// Create a simple linetype. `pattern` is signed lengths in drawing units:
    /// positive is a dash, negative a gap, zero a dot (2-12 elements with at
    /// least one dash or dot and one gap). Returns the linetype's handle.
    LinetypeCreate {
        name: String,
        description: String,
        pattern: Vec<f64>,
    },
    /// Change a simple linetype's description or pattern in place.
    LinetypeModify {
        name: String,
        description: Option<String>,
        pattern: Option<Vec<f64>>,
    },
    /// Rename a linetype; layers and entities that use it follow.
    LinetypeRename { from: String, to: String },
    /// Delete a linetype that no layer, entity, dimension style or the current
    /// setting uses. `Continuous`, `ByLayer` and `ByBlock` are never deleted.
    LinetypeDelete { name: String },
    /// Create a paper-space layout with the default page setup and sheet viewport.
    LayoutCreate { name: String },
    /// Rename a paper-space layout (never `Model`).
    LayoutRename { from: String, to: String },
    /// Delete a paper-space layout and everything on it (never `Model`).
    LayoutDelete { name: String },
    /// Switch the active layout (`Model` or a paper-space layout).
    LayoutSetCurrent { name: String },
    /// Change a paper-space layout's sheet: size in millimetres, rotation in
    /// degrees (0, 90, 180 or 270) and a custom plot scale (numerator,
    /// denominator).
    LayoutSetPage {
        name: String,
        paper_size: Option<[f64; 2]>,
        rotation: Option<u16>,
        scale: Option<[f64; 2]>,
    },
}

/// The style tables `TableOperation` can rename, delete or make current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableStyleKind {
    Text,
    Dim,
}

/// Properties for creating or changing a text style; `None` leaves a property
/// as it was (or the default for a new style). Angles are in radians.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TextStyleConfig {
    pub name: String,
    /// Fixed height; `0` lets each text choose its own.
    pub height: Option<f64>,
    pub width_factor: Option<f64>,
    pub oblique_angle: Option<f64>,
    pub font_file: Option<String>,
    pub big_font_file: Option<String>,
    pub true_type_font: Option<String>,
    pub backward: Option<bool>,
    pub upside_down: Option<bool>,
    pub vertical: Option<bool>,
    pub annotative: Option<bool>,
}

/// A kernel operation on ACIS-backed solids (API v7, additive). The host owns
/// the geometry: a script never sees or rewrites the payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SolidOperation {
    /// Build a new solid on `layer` (layer `0` when `None`).
    Create {
        primitive: SolidPrimitive,
        layer: Option<String>,
    },
    /// Apply a rigid transform, given as a column-major 4x4 matrix, to an
    /// existing solid. Scaling and shear are refused.
    Transform { handle: Handle, matrix: [f64; 16] },
    /// Build a planar region from one closed planar profile entity (a circle,
    /// ellipse, closed polyline or spline, ...). The new region goes on
    /// `layer`, or on the source's layer when `None`; `delete_source` erases
    /// the profile afterwards, as the REGION command does.
    RegionFromProfile {
        source: Handle,
        layer: Option<String>,
        delete_source: bool,
    },
    /// Build a plane surface from one closed planar profile entity.
    SurfaceFromProfile {
        source: Handle,
        layer: Option<String>,
        delete_source: bool,
    },
    /// Extrude a planar profile along `direction`: a solid when the profile is
    /// closed, a surface when it is open.
    Extrude {
        source: Handle,
        direction: [f64; 3],
        layer: Option<String>,
        delete_source: bool,
    },
    /// Combine two solids into a new one. Both operands are consumed unless
    /// `keep_operands`; the result goes on `layer` or the first operand's
    /// layer. The kernel refuses some cases (coincident faces, cuts it has no
    /// closed form for); a refusal changes nothing. Curved operands can take a
    /// second or two, and the host thread is busy meanwhile.
    /// Embed a picture file as an OLE frame whose bottom-left corner is at
    /// `origin`, `width` drawing units wide, keeping the picture's aspect.
    /// PNG, JPEG and BMP are stored as read; other formats are re-encoded as
    /// PNG. The frame goes on `layer` (layer `0` when `None`).
    EmbedPicture {
        path: String,
        origin: [f64; 3],
        width: f64,
        layer: Option<String>,
    },
    Boolean {
        first: Handle,
        second: Handle,
        operation: SolidBoolean,
        layer: Option<String>,
        keep_operands: bool,
    },
}

/// Value of a host-managed drafting or document setting exposed to plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HostSettingValue {
    Text(String),
    Number(f64),
}

/// A notification the host sends to a plugin. These are best-effort,
/// full-duplex messages correlated with an optional `command_id`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum HostNotification {
    InputLine {
        line: String,
    },
    Cancel,
    DocumentChanged {
        version: u64,
    },
    SelectionChanged {
        handles: Vec<Handle>,
    },
    Raw(Vec<u8>),
    /// V4 snapshot changed for a specific tab. Discriminant 5.
    DocumentChangedV4 {
        tab_id: u64,
        version: u64,
    },
    /// V4 tab closed notification. Discriminant 6.
    DocumentTabClosed {
        tab_id: u64,
    },
    /// V4 selection changed for a specific tab. Discriminant 7.
    SelectionChangedV4 {
        tab_id: u64,
        handles: Vec<Handle>,
    },
    /// V7 active interactive-command state for a tab. Discriminant 8.
    CommandStateChanged {
        tab_id: u64,
        command: Option<String>,
    },
    /// V7 drawing epoch, broadcast even without an open shared view. Discriminant 9.
    DrawingChanged {
        tab_id: u64,
        epoch: u64,
    },
    /// V9 a document became resident in a tab: a whole document replaced the
    /// tab's previous content — opened from file, created from a template, or
    /// loaded by an automation/system open. The counterpart of
    /// [`HostNotification::DocumentTabClosed`]: with both, a plugin can keep
    /// per-document state (e.g. runtime designations) in sync from load to
    /// close. Discriminant 10.
    DocumentOpened {
        tab_id: u64,
    },
    /// V13 a document boundary settled, carrying the DIRTY REGION — the
    /// identity set of what changed since the previous boundary: the touched
    /// entity handles, and the (entity, app-name) pairs of touched XRecords.
    /// Sent alongside [`HostNotification::DocumentChanged`] at every boundary;
    /// boundary work can then be O(changed) instead of O(store). Older
    /// plugins decode it through the `Unknown` passthrough. Discriminant 11.
    DocumentChangedRegions {
        tab_id: u64,
        version: u64,
        entities: Vec<Handle>,
        records: Vec<(Handle, String)>,
    },
    /// V7 panel control event (button click, value change). Discriminant 12.
    PanelAction {
        panel_id: String,
        control_id: String,
        action: String,
        value: Option<crate::dialog::DialogValue>,
    },
    /// Fallback for notification variants added in future minor revisions.
    /// Carries the raw bincode payload so an older peer can ignore it without
    /// failing deserialization.
    Unknown(Vec<u8>),
}

impl Serialize for HostNotification {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut bytes = Vec::new();
        match self {
            HostNotification::InputLine { line } => {
                bytes.push(0);
                bincode::serialize_into(&mut bytes, line).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::Cancel => bytes.push(1),
            HostNotification::DocumentChanged { version } => {
                bytes.push(2);
                bincode::serialize_into(&mut bytes, version).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::SelectionChanged { handles } => {
                bytes.push(3);
                bincode::serialize_into(&mut bytes, handles).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::Raw(data) => {
                bytes.push(4);
                bincode::serialize_into(&mut bytes, data).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::DocumentChangedV4 { tab_id, version } => {
                bytes.push(5);
                bincode::serialize_into(&mut bytes, tab_id).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, version).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::DocumentTabClosed { tab_id } => {
                bytes.push(6);
                bincode::serialize_into(&mut bytes, tab_id).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::SelectionChangedV4 { tab_id, handles } => {
                bytes.push(7);
                bincode::serialize_into(&mut bytes, tab_id).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, handles).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::CommandStateChanged { tab_id, command } => {
                bytes.push(8);
                bincode::serialize_into(&mut bytes, tab_id).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, command).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::DrawingChanged { tab_id, epoch } => {
                bytes.push(9);
                bincode::serialize_into(&mut bytes, tab_id).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, epoch).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::DocumentOpened { tab_id } => {
                bytes.push(10);
                bincode::serialize_into(&mut bytes, tab_id).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::DocumentChangedRegions {
                tab_id,
                version,
                entities,
                records,
            } => {
                bytes.push(11);
                bincode::serialize_into(&mut bytes, tab_id).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, version).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, entities).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, records).map_err(serde::ser::Error::custom)?;
            }
            HostNotification::PanelAction {
                panel_id,
                control_id,
                action,
                value,
            } => {
                bytes.push(12);
                bincode::serialize_into(&mut bytes, panel_id)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, control_id)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, action)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, value)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::Unknown(raw) => bytes.extend_from_slice(raw),
        }
        bytes.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for HostNotification {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        if bytes.is_empty() {
            return Err(serde::de::Error::custom("empty HostNotification"));
        }
        let discriminant = bytes[0];
        let rest = &bytes[1..];
        match discriminant {
            0 => bincode::deserialize(rest)
                .map(|line| HostNotification::InputLine { line })
                .map_err(serde::de::Error::custom),
            1 => Ok(HostNotification::Cancel),
            2 => bincode::deserialize(rest)
                .map(|version| HostNotification::DocumentChanged { version })
                .map_err(serde::de::Error::custom),
            3 => bincode::deserialize(rest)
                .map(|handles| HostNotification::SelectionChanged { handles })
                .map_err(serde::de::Error::custom),
            4 => bincode::deserialize(rest)
                .map(HostNotification::Raw)
                .map_err(serde::de::Error::custom),
            5 => bincode::deserialize(rest)
                .map(|(tab_id, version)| HostNotification::DocumentChangedV4 { tab_id, version })
                .map_err(serde::de::Error::custom),
            6 => bincode::deserialize(rest)
                .map(|tab_id| HostNotification::DocumentTabClosed { tab_id })
                .map_err(serde::de::Error::custom),
            7 => bincode::deserialize(rest)
                .map(|(tab_id, handles)| HostNotification::SelectionChangedV4 { tab_id, handles })
                .map_err(serde::de::Error::custom),
            8 => bincode::deserialize(rest)
                .map(|(tab_id, command)| HostNotification::CommandStateChanged { tab_id, command })
                .map_err(serde::de::Error::custom),
            9 => bincode::deserialize(rest)
                .map(|(tab_id, epoch)| HostNotification::DrawingChanged { tab_id, epoch })
                .map_err(serde::de::Error::custom),
            10 => bincode::deserialize(rest)
                .map(|tab_id| HostNotification::DocumentOpened { tab_id })
                .map_err(serde::de::Error::custom),
            11 => {
                let (tab_id, version, entities, records): (
                    u64,
                    u64,
                    Vec<Handle>,
                    Vec<(Handle, String)>,
                ) = bincode::deserialize(rest).map_err(serde::de::Error::custom)?;
                Ok(HostNotification::DocumentChangedRegions {
                    tab_id,
                    version,
                    entities,
                    records,
                })
            }
            12 => {
                let mut cursor = std::io::Cursor::new(rest);
                let panel_id: String = bincode::deserialize_from(&mut cursor)
                    .map_err(serde::de::Error::custom)?;
                let control_id: String = bincode::deserialize_from(&mut cursor)
                    .map_err(serde::de::Error::custom)?;
                let action: String = bincode::deserialize_from(&mut cursor)
                    .map_err(serde::de::Error::custom)?;
                let value: Option<crate::dialog::DialogValue> = bincode::deserialize_from(&mut cursor)
                    .map_err(serde::de::Error::custom)?;
                Ok(HostNotification::PanelAction {
                    panel_id,
                    control_id,
                    action,
                    value,
                })
            }
            _ => Ok(HostNotification::Unknown(bytes)),
        }
    }
}

/// A notification a plugin sends to the host. These are best-effort,
/// full-duplex messages correlated with an optional `command_id`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PluginNotification {
    Output {
        text: String,
    },
    Error {
        text: String,
    },
    Prompt {
        text: String,
    },
    Progress {
        percent: u8,
    },
    Log {
        level: LogLevel,
        text: String,
    },
    Raw(Vec<u8>),
    /// V4 REPL status update. Discriminant 6.
    ReplStatus {
        status: String,
        message: String,
    },
    /// V13 the runner's notification subscription is live: its main loop
    /// entered and is draining host notifications. The host defers
    /// boot-window broadcasts per process until this ack arrives (and flushes
    /// them then), so a fresh boot's notifications can never race the
    /// runner's startup — the notification-attach determinism the dispatch
    /// table already has. Older hosts decode it through the `Unknown`
    /// passthrough. Discriminant 7.
    ChannelReady,
    /// Fallback for notification variants added in future minor revisions.
    /// Carries the raw bincode payload so an older peer can ignore it without
    /// failing deserialization.
    Unknown(Vec<u8>),
}

impl Serialize for PluginNotification {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut bytes = Vec::new();
        match self {
            PluginNotification::Output { text } => {
                bytes.push(0);
                bincode::serialize_into(&mut bytes, text).map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Error { text } => {
                bytes.push(1);
                bincode::serialize_into(&mut bytes, text).map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Prompt { text } => {
                bytes.push(2);
                bincode::serialize_into(&mut bytes, text).map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Progress { percent } => {
                bytes.push(3);
                bincode::serialize_into(&mut bytes, percent).map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Log { level, text } => {
                bytes.push(4);
                bincode::serialize_into(&mut bytes, level).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, text).map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Raw(data) => {
                bytes.push(5);
                bincode::serialize_into(&mut bytes, data).map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::ReplStatus { status, message } => {
                bytes.push(6);
                bincode::serialize_into(&mut bytes, status).map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, message).map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::ChannelReady => bytes.push(7),
            PluginNotification::Unknown(raw) => bytes.extend_from_slice(raw),
        }
        bytes.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PluginNotification {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        if bytes.is_empty() {
            return Err(serde::de::Error::custom("empty PluginNotification"));
        }
        let discriminant = bytes[0];
        let rest = &bytes[1..];
        match discriminant {
            0 => bincode::deserialize(rest)
                .map(|text| PluginNotification::Output { text })
                .map_err(serde::de::Error::custom),
            1 => bincode::deserialize(rest)
                .map(|text| PluginNotification::Error { text })
                .map_err(serde::de::Error::custom),
            2 => bincode::deserialize(rest)
                .map(|text| PluginNotification::Prompt { text })
                .map_err(serde::de::Error::custom),
            3 => bincode::deserialize(rest)
                .map(|percent| PluginNotification::Progress { percent })
                .map_err(serde::de::Error::custom),
            4 => {
                let (level, text): (LogLevel, String) =
                    bincode::deserialize(rest).map_err(serde::de::Error::custom)?;
                Ok(PluginNotification::Log { level, text })
            }
            5 => bincode::deserialize(rest)
                .map(PluginNotification::Raw)
                .map_err(serde::de::Error::custom),
            6 => bincode::deserialize(rest)
                .map(|(status, message)| PluginNotification::ReplStatus { status, message })
                .map_err(serde::de::Error::custom),
            7 => Ok(PluginNotification::ChannelReady),
            _ => Ok(PluginNotification::Unknown(bytes)),
        }
    }
}

/// An add-on package's entry point: its manifest, optional ribbon tab, and
/// command dispatch. Built-in (in-tree) and dynamically-loaded (cdylib) plugins
/// implement the same trait from this crate, so an out-of-tree add-on targets
/// the stable contract rather than the host binary.
pub trait BuiltinPlugin: Send + Sync {
    fn manifest(&self) -> &'static PluginManifest;
    fn ribbon(&self) -> Box<dyn CadModule>;
    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool;

    /// Long-running REPL code execution (API v4). The plugin may start the
    /// work on another thread and call `respond` when finished; the runner
    /// will forward the result to the host. The supplied `host` is the active
    /// document tab's API surface, so the REPL session is tied to a tab.
    ///
    /// Returning `false` means the plugin does not support code execution.
    fn start_execute_code(
        &mut self,
        _host: &mut dyn HostApi,
        _command_id: u64,
        _code: &str,
        _source: CommandSource,
        _respond: Box<dyn FnOnce(ExecutionResult) + Send>,
    ) -> bool {
        false
    }

    /// State-only callback for host-to-plugin notifications. Added in API v4;
    /// the V4 runner guarantees it is only called for plugins that report
    /// API major 4 or newer.
    fn on_notification(&mut self, _command_id: Option<u64>, _notification: HostNotification) {}

    /// Lifecycle callback invoked once after the runner connects. The plugin
    /// receives the active document tab's `HostApi` so it can set up worker
    /// threads or cache `plugin_request_sender()` before any user command runs.
    ///
    /// Added in API v5. The runner only calls this for plugins that report API
    /// major 5 or newer.
    fn on_load(&mut self, _host: &mut dyn HostApi) {}

    /// Entity-class display declarations (API v10): the XDATA
    /// class-identity keys the plugin stamps on entities, mapped to the
    /// display labels the host shows while this plugin is loaded (e.g. the
    /// Properties panel title becomes `"Linear Element (Line)"`). Display-only
    /// — the labels never round-trip the drawing and never govern behavior;
    /// entities carry only the stable identity keys in their XDATA.
    ///
    /// The default (`None`) declares no entity classes. The runner only calls
    /// this for plugins whose exported `ocs_plugin_api_version` symbol reports
    /// API major 10 or newer, so older cdylibs are never touched past their
    /// vtable.
    fn entity_classes(&self) -> Option<&'static EntityClasses> {
        None
    }

    /// A drag event for one of this plugin's declared custom grips (API v11).
    ///
    /// The host calls this for the begin/move/settle/cancel phases of a drag
    /// on a grip this plugin declared — and only for this plugin. `host` is a
    /// full [`HostApi`] session: from inside the settle callback the plugin
    /// can read the document/records and write — including opening its own
    /// [`HostApi::push_undo`] scope — as the settle's only write window.
    /// The settled drag totals exactly the plugin's one labeled undo step;
    /// the host records none of its own.
    ///
    /// The returned [`CustomGripResponse::preview`] replaces the plugin's
    /// live preview wire set; it is the only feedback the host shows during
    /// the drag — the host draws no ghost and moves nothing. Cancel means
    /// drop preview state with zero writes.
    ///
    /// The host guards this call: a panicking handler is contained and
    /// reported, never fatal. The runner only calls this for plugins whose
    /// exported `ocs_plugin_api_version` symbol reports API major 11 or
    /// newer, so older cdylibs are never touched past their vtable.
    fn on_custom_grip_event(
        &mut self,
        _host: &mut dyn HostApi,
        _event: CustomGripEvent,
    ) -> CustomGripResponse {
        CustomGripResponse::default()
    }
}

/// A point-driven interactive command a plugin starts via
/// [`HostApi::start_interactive`]. The host shows the prompt, collects points —
/// clicked in the viewport, or fed as coordinates over the `--serve` automation
/// API — and commits the entities the command yields, exactly like a built-in
/// tool. This is the plugin-facing slice of the host's command machinery; it
/// covers click-to-place placement without exposing the host's internal command
/// trait.
pub trait InteractiveCommand: Send {
    /// Prompt for the next point.
    fn prompt(&self) -> String;
    /// A point was supplied (clicked or typed `x,y[,z]`). Returns the next step.
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep;
    /// Enter pressed with no point — e.g. to finish a multi-point command.
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }

    /// When `true`, the next input picks an existing **entity** (the user clicks
    /// on it; over `--serve`, a handle is supplied) rather than a free point —
    /// the host then calls [`on_object_pick`](Self::on_object_pick). Use this to
    /// reference existing geometry (e.g. connect a pipe between two structures).
    fn needs_object_pick(&self) -> bool {
        false
    }
    /// An existing entity was picked: its `handle` and the pick point. Read the
    /// entity's data (XDATA / geometry) via `HostApi`, keyed by the handle.
    fn on_object_pick(&mut self, _handle: Handle, _pt: [f64; 3]) -> CommandStep {
        CommandStep::Cancel
    }

    /// Real-time preview geometry (lines, arcs, polylines) to render as the cursor moves.
    /// Each wire can specify custom vertices and an optional color (defaults to host cyan).
    fn on_cursor_move(&mut self, _pt: [f64; 3]) -> Vec<PreviewWire> {
        Vec::new()
    }

    /// When `true`, the host enters selection-gathering mode (as for `ssget`).
    /// The user can pick objects or use window/crossing drag selection.
    /// Handled selections trigger [`on_selection_complete`](Self::on_selection_complete).
    fn is_selection_gathering(&self) -> bool {
        false
    }

    /// When `true` (and `is_selection_gathering` is true), the host permits
    /// window and crossing box drag selections to be completed.
    fn accepts_drag_selection(&self) -> bool {
        false
    }

    /// Called after a selection action completes while `is_selection_gathering` is true.
    /// `handles` is the full set of currently selected entities.
    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CommandStep {
        CommandStep::NeedPoint
    }

    /// Returns `true` when selection should reject entities on locked layers (e.g. ssget ":L").
    fn selection_rejects_locked_layers(&self) -> bool {
        false
    }

    /// An optional declarative modal dialog to display while this interactive command is active.
    fn modal_dialog(&self) -> Option<crate::dialog::DialogDef> {
        None
    }

    /// User submitted or cancelled the modal dialog.
    fn on_dialog_response(&mut self, _response: crate::dialog::DialogResponse) -> CommandStep {
        CommandStep::Done
    }

    /// Pending panel updates buffered during interactive command execution (API v7, additive).
    fn take_pending_panel_updates(&mut self) -> Vec<(String, std::collections::HashMap<String, crate::dialog::DialogValue>)> {
        Vec::new()
    }
}

/// A preview wire rendered in real-time during interactive commands.
/// Supports straight polylines as well as analytical circles and arcs
/// that render via GPU shaders with infinite smoothness.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "host", derive(serde::Serialize, serde::Deserialize))]
pub enum PreviewWire {
    /// A connected sequence of straight line segments in world space.
    Polyline {
        points: Vec<[f64; 3]>,
        color: Option<[f32; 4]>,
    },
    /// An analytical circle rendered with sub-pixel GPU anti-aliasing.
    Circle {
        center: [f64; 3],
        radius: f64,
        color: Option<[f32; 4]>,
    },
    /// An analytical circular arc swept counter-clockwise from `start_angle_rad` to `end_angle_rad`.
    Arc {
        center: [f64; 3],
        radius: f64,
        start_angle_rad: f64,
        end_angle_rad: f64,
        color: Option<[f32; 4]>,
    },
}

impl PreviewWire {
    /// Create a polyline preview wire with default host cyan color.
    pub fn new(points: Vec<[f64; 3]>) -> Self {
        Self::Polyline {
            points,
            color: None,
        }
    }

    /// Optional RGBA color (0.0 to 1.0) specified for this preview wire.
    pub fn color(&self) -> Option<[f32; 4]> {
        match self {
            Self::Polyline { color, .. } => *color,
            Self::Circle { color, .. } => *color,
            Self::Arc { color, .. } => *color,
        }
    }

    /// Create a polyline preview wire with a custom RGBA color.
    pub fn with_color(points: Vec<[f64; 3]>, color: [f32; 4]) -> Self {
        Self::Polyline {
            points,
            color: Some(color),
        }
    }

    /// Create a straight line segment preview between two points.
    pub fn line(from: [f64; 3], to: [f64; 3], color: Option<[f32; 4]>) -> Self {
        Self::Polyline {
            points: vec![from, to],
            color,
        }
    }

    /// Create an analytical circle preview.
    pub fn circle(center: [f64; 3], radius: f64, color: Option<[f32; 4]>) -> Self {
        Self::Circle {
            center,
            radius,
            color,
        }
    }

    /// Create an analytical circular arc preview.
    pub fn arc(
        center: [f64; 3],
        radius: f64,
        start_angle_rad: f64,
        end_angle_rad: f64,
        color: Option<[f32; 4]>,
    ) -> Self {
        Self::Arc {
            center,
            radius,
            start_angle_rad,
            end_angle_rad,
            color,
        }
    }
}

impl From<Vec<[f64; 3]>> for PreviewWire {
    fn from(points: Vec<[f64; 3]>) -> Self {
        Self::new(points)
    }
}

// ── Plugin grip surface (API v11, additive) ────────────────────────────────
//
// Three surfaces, one rule: plugin DECLARED, plugin OWNED. The host renders,
// hit-tests and drags custom grips, but it never interprets them — every
// semantic decision lives in the declaring plugin. The host applies no entity
// mutation, constraint or record of its own when a custom grip settles; the
// plugin learns of the drag through live [`CustomGripEvent`] frames, shows a
// preview through the existing [`PreviewWire`] channel, and applies the settle
// with its own HostApi writes inside its own [`HostApi::push_undo`] scope.

/// One built-in grip of an entity as enumerated by
/// [`HostApi::entity_grips`](HostApi::entity_grips) (API v11).
///
/// The shape and field semantics mirror the automation `grips` read: `grip_id`
/// ist the entity-local grip index, `point` the grip's world position, and
/// `moves_entity` true for translate grips (the same flag the interactive
/// viewport uses to whole-entity moves).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GripInfo {
    pub grip_id: usize,
    pub point: [f64; 3],
    #[serde(default)]
    pub moves_entity: bool,
    /// Debug name of the grip shape (e.g. `"Square"`).
    pub shape: String,
    /// Drag constraint direction, when the built-in grip declares one.
    #[serde(default)]
    pub axis: Option<[f64; 3]>,
}

/// Marker shape a plugin requests for a custom grip point (API v11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PluginGripShape {
    #[default]
    Square,
    Circle,
    Triangle,
}

/// A plugin-declared custom grip point (API v11).
///
/// The whole batch passed to
/// [`set_plugin_grips`](HostApi::set_plugin_grips) REPLACES the declaring
/// plugin's previous set; an empty batch clears it. Declaration is
/// session-only display state: it is never written to the drawing file, and
/// the plugin re-declares after its own notification boundaries
/// (pull model — [`on_notification`](BuiltinPlugin::on_notification)).
///
/// `point` is an ABSOLUTE world point. The host never moves, offsets or
/// auto-follows it when the `anchor` entity moves; an anchor exists for
/// ownership scoping and hit-test legibility only. `axis`, when present,
/// hard-constrains interactive drags to that direction line through the
/// point; the drag events carry the projected point on the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginGrip {
    pub grip_id: u64,
    #[serde(default)]
    pub anchor: Option<Handle>,
    pub point: [f64; 3],
    #[serde(default)]
    pub axis: Option<[f64; 3]>,
    #[serde(default)]
    pub shape: PluginGripShape,
}

/// Drag phase of a [`CustomGripEvent`] (API v11).
///
/// Move events are coalesced (at most one is in flight to the plugin;
/// superseded cursor positions are dropped); the settle and cancel phases
/// are never coalesced or dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CustomGripPhase {
    Begin,
    Move,
    Settle,
    Cancel,
}

/// One drag event delivered to the owning plugin (API v11).
///
/// `point` is the CURRENT drag point: the projected cursor point with a
/// hard-constrained axis, the point on the constraint line. The settle event
/// carries the final point; cancel lets the plugin drop preview state with
/// zero writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomGripEvent {
    pub grip_id: u64,
    pub phase: CustomGripPhase,
    pub point: [f64; 3],
    #[serde(default)]
    pub anchor: Option<Handle>,
}

/// The plugin's answer to a [`CustomGripEvent`] (API v11, additive).
///
/// `preview` REPLACES the plugin's live preview wire set; analytical arcs
/// and circles render GPU-smooth. The plugin's preview is the only
/// host-invented live feedback — the host draws no ghost of its own and
/// moves nothing on its own during the drag.
///
/// `live_entities` (API v12, additive) is the plugin-directed LIVE-GEOMETRY
/// channel — the plugin-owned twin of what a built-in grip drag shows: the
/// CURRENT geometry of every affected entity (`(handle, entity)` pairs,
/// computed by the plugin from the drag point, exactly what the settle will
/// commit). The host applies them during the drag through the same
/// visual-only dynamic phase its own grip drags use — the elements visibly
/// move, ZERO history steps are recorded mid-drag, the whole visual state is
/// discarded byte-identically on cancel, and the settle RESTORES the
/// originals before the plugin's authoritative writes land in its own one
/// undo scope. The host still interprets nothing: the plugin computes and
/// returns every affected entity's live geometry per move. A response
/// carrying only `preview` behaves exactly as before — the drawing stays
/// static except for the preview wires. The payload is capped at
/// [`MAX_PLUGIN_LIVE_ENTITIES`] per response; a larger set answers with a
/// refused delivery (headline in the error queue), never a freeze.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomGripResponse {
    #[serde(default)]
    pub preview: Vec<PreviewWire>,
    #[serde(default)]
    pub live_entities: Vec<(Handle, EntityType)>,
    /// A descriptor-backed live design for the dragged instance (API v14,
    /// additive). Some on the Begin answer tells the host to derive the live
    /// preview ITSELF from this design for the rest of the drag â€” re-evaluated
    /// locally per coalesced cursor move, no further Move frames crossing the
    /// IPC â€” instead of the plugin streaming preview/live-entity geometry per
    /// move. The plugin's settle callback stays the only write window (its
    /// own labeled undo step; records first, then the re-stage). Older hosts
    /// deserialize the field to None and keep the v12 semantics untouched;
    /// None keeps every current response behaving exactly as before.
    #[serde(default)]
    pub descriptor: Option<PluginDisplayDescriptor>,
    /// (API v14, additive) THE per-move re-derivation tolerance of a
    /// descriptor-backed drag — the same tolerance the plugin staged its
    /// instance with: it bounds the drag's chord fits AND the per-move
    /// seam continuity reads (one declared accuracy). `Some` must accompany the Begin answer's `descriptor`
    /// (the host never invents a frame budget: a descriptor without a finite
    /// positive tolerance leaves the v12 streaming semantics untouched, with
    /// one headline in the error queue). Older hosts deserialize the field
    /// to None.
    #[serde(default)]
    pub descriptor_tolerance: Option<f64>,
}

/// Per-response cap on live-geometry entities (API v12): the drag stays
/// rubber-band smooth; wider chains should batch by kind (the plugin decides
/// what must move) or co-design a lighter frame with the host.
pub const MAX_PLUGIN_LIVE_ENTITIES: usize = 256;

/// The outcome of an [`InteractiveCommand`] step.
#[derive(Debug)]
#[cfg_attr(feature = "host", derive(serde::Serialize, serde::Deserialize))]
pub enum CommandStep {
    /// Need another point; keep the command active.
    NeedPoint,
    /// Commit an entity to the document and keep collecting points.
    Commit(EntityType),
    /// Commit an entity and end the command.
    CommitAndEnd(EntityType),
    /// End the command without committing.
    Done,
    /// Cancel the command.
    Cancel,
    // New variants go after the existing ones: the step crosses the plugin
    // IPC, which encodes a variant by its position, so inserting one earlier
    // would make a plugin built against the previous API send `Done` and
    // have the host read it as something else.
    /// Commit multiple entities to the document and keep collecting points.
    CommitMany(Vec<EntityType>),
    /// Commit multiple entities to the document and end the command.
    CommitManyAndEnd(Vec<EntityType>),
    /// End the command and dispatch a command string to the host command line.
    Dispatch(String),
}

/// Export a `BuiltinPlugin` from a `cdylib` so the host can load it at runtime.
///
/// Emits the two C symbols the loader looks for: `ocs_plugin_api_version`
/// (checked before anything else, so an ABI-incompatible build is rejected
/// without running its code) and `ocs_plugin_register` (constructs the plugin
/// and hands ownership to the host as a boxed trait object).
///
/// ```ignore
/// ocs_plugin_api::export_plugin!(MyPlugin::new());
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($ctor:expr) => {
        #[no_mangle]
        pub extern "C" fn ocs_plugin_api_version() -> u32 {
            $crate::API_VERSION
        }

        /// The whole runner, run by this plugin's own build so no Rust type
        /// crosses the library boundary (API v14 and later). `socket` is the
        /// host's socket name; returns the process exit code.
        #[no_mangle]
        pub extern "C" fn ocs_plugin_run(socket: *const ::std::os::raw::c_char) -> i32 {
            if socket.is_null() {
                return 2;
            }
            let socket = unsafe { ::std::ffi::CStr::from_ptr(socket) }
                .to_string_lossy()
                .into_owned();
            match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let plugin: ::std::boxed::Box<dyn $crate::host::BuiltinPlugin> =
                    ::std::boxed::Box::new($ctor);
                $crate::runner::run_with_plugin(&socket, $crate::API_VERSION, plugin)
                    .map_err(|e| e.to_string())
            })) {
                ::std::result::Result::Ok(::std::result::Result::Ok(())) => 0,
                ::std::result::Result::Ok(::std::result::Result::Err(e)) => {
                    eprintln!("[plugin] {e}");
                    1
                }
                ::std::result::Result::Err(_) => 2,
            }
        }

        #[no_mangle]
        pub extern "C" fn ocs_plugin_register(
        ) -> *mut ::std::boxed::Box<dyn $crate::host::BuiltinPlugin> {
            // The constructor runs across a C ABI boundary; a panic unwinding
            // past it is undefined behavior. Contain it and report failure as a
            // null pointer, which the host loader treats as "registration
            // failed" rather than crashing the runner process.
            match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let plugin: ::std::boxed::Box<dyn $crate::host::BuiltinPlugin> =
                    ::std::boxed::Box::new($ctor);
                ::std::boxed::Box::into_raw(::std::boxed::Box::new(plugin))
            })) {
                ::std::result::Result::Ok(ptr) => ptr,
                ::std::result::Result::Err(_) => ::std::ptr::null_mut(),
            }
        }
    };
}

/// The plugin-facing runtime surface for one active document tab.
pub trait HostApi {
    /// Index of the tab this session targets.
    fn tab_index(&self) -> usize;

    // ── Document ────────────────────────────────────────────────────────────
    fn document(&self) -> &Document;
    /// Mutable access to the active document.
    ///
    /// For an **out-of-process** plugin this borrows a *local snapshot*: edits
    /// to existing entities made through it are NOT sent back to the host and
    /// are silently discarded. To modify or delete entities from any plugin,
    /// use [`add_entity`](Self::add_entity), [`update_entity`](Self::update_entity)
    /// and [`remove_entity`](Self::remove_entity), which are committed to the
    /// host document over IPC.
    fn document_mut(&mut self) -> &mut Document;

    /// Add an entity to the active document, returning its handle.
    fn add_entity(&mut self, entity: EntityType) -> Handle;
    /// Replace the existing entity that carries `entity`'s handle, preserving
    /// its identity (handle and owning block). Returns `false` when no entity
    /// has that handle. This is the sanctioned way to commit in-place edits
    /// from an out-of-process plugin — mutating `document_mut()` does not work
    /// across the process boundary.
    fn update_entity(&mut self, entity: EntityType) -> bool {
        let handle = entity.common().handle;
        match self.document_mut().get_entity_mut(handle) {
            Some(slot) => {
                *slot = entity;
                true
            }
            None => false,
        }
    }
    /// Delete the entity with `handle` (and any derived render caches). Returns
    /// `true` when an entity was removed.
    fn remove_entity(&mut self, handle: Handle) -> bool {
        self.document_mut().remove_entity(handle).is_some()
    }
    /// Mark the scene geometry dirty so it is re-tessellated next frame.
    fn bump_geometry(&mut self);

    // ── XDATA ───────────────────────────────────────────────────────────────
    /// Read the XDATA record for `app_name` on entity `handle`, if any.
    fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord>;
    /// Attach `record` to entity `handle`, replacing any existing record for the
    /// same application and registering the APPID. Returns `false` if the entity
    /// does not exist.
    fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool;
    /// Remove the XDATA record for `app_name` from entity `handle`. Returns
    /// `true` if a record was removed.
    fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool;

    // ── Undo / dirty ────────────────────────────────────────────────────────
    fn push_undo(&mut self, label: &str);
    /// Open one undo step that spans several messages (API v14): every edit
    /// until [`HostApi::end_undo_group`] joins it, including writes a worker
    /// thread sends through [`HostApi::plugin_request_sender`], and
    /// `push_undo` inside it opens nothing new. The step also closes when the
    /// user runs a command of their own. The default is [`HostApi::push_undo`].
    fn begin_undo_group(&mut self, label: &str) {
        self.push_undo(label);
    }
    /// Close the step [`HostApi::begin_undo_group`] opened.
    fn end_undo_group(&mut self) {}

    /// The entities matching `query`, one page at a time (API v14). `query`
    /// is the JSON of the control API's `query` operation, e.g.
    /// `{"layer":"*-OPENBARE_RUIMTE_*","type":"Mesh","xdata_app":"OPENINFRA",
    /// "bounds":[x0,y0,x1,y1],"offset":0,"limit":500}`: `layer` takes `*` and
    /// `?`, `where` filters properties by JSON Pointer, and `limit` is at
    /// most 10000. One message carries at most 64 MiB either way, so batch
    /// writes and size pages to stay under it.
    fn query_entities(&mut self, query: &str) -> Result<EntityPage, String> {
        let _ = query;
        Err("query_entities is not supported by this host".to_owned())
    }
    fn set_dirty(&mut self);

    // ── Command line ────────────────────────────────────────────────────────
    fn push_info(&mut self, msg: &str);
    fn push_output(&mut self, msg: &str);
    fn push_error(&mut self, msg: &str);

    /// Start a plugin-defined interactive (click-to-place) command on the active
    /// tab. The host drives it through its normal point-collection flow.
    fn start_interactive(&mut self, command: Box<dyn InteractiveCommand>);

    // ── Per-tab plugin state (object-safe; use the typed helpers below) ──────
    fn plugin_state_any(&self, plugin_id: &str) -> Option<&(dyn Any + Send + Sync)>;
    fn plugin_state_any_mut(&mut self, plugin_id: &str) -> Option<&mut (dyn Any + Send + Sync)>;
    /// Get the state for `plugin_id`, inserting `init()`'s result if absent.
    /// Returns `None` when this host cannot store state (e.g. out-of-process).
    fn ensure_plugin_state_any(
        &mut self,
        plugin_id: &'static str,
        init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
    ) -> Option<&mut (dyn Any + Send + Sync)>;

    // ── DocumentReader (added in API v3; appended at the end to keep vtable
    // indices stable for API v2 plugins) ─────────────────────────────────────

    /// Read-only, zero-copy view of the active document. For out-of-process
    /// plugins this is backed by host-owned shared memory; for in-process
    /// plugins it wraps `document()`.
    fn document_reader(&self) -> Box<dyn DocumentReader + '_>;

    /// Open (or refresh) the host-side shared document view and return the
    /// information the plugin needs to map it. In-process hosts implement this;
    /// out-of-process plugin proxies return `None`.
    fn document_view(&mut self) -> Option<crate::shm::DocumentViewInfo> {
        None
    }

    // ── Notifications (added in API v4; appended at the end to keep vtable
    // indices stable for V2/V3 plugins) ───────────────────────────────────────

    /// Send a best-effort notification from the plugin to the host.
    ///
    /// In-process plugins can override this to forward to the host event loop;
    /// out-of-process plugins send a V4 notification frame.
    fn notify_plugin(&mut self, _command_id: Option<u64>, _notification: PluginNotification) {}

    /// Poll for a host-to-plugin notification, if any.
    ///
    /// Returns the optional `command_id` used to correlate the notification
    /// with a running command, and the notification payload. Long-running
    /// plugins should call this periodically to drain the bounded queue.
    fn try_recv_notification(&mut self) -> Option<(Option<u64>, HostNotification)> {
        None
    }

    /// Returns a thread-safe handle that can issue host requests from worker
    /// threads. Out-of-process V4 plugins implement this; in-process hosts may
    /// return `None`.
    fn plugin_request_sender(&self) -> Option<Box<dyn PluginRequestSender>> {
        None
    }

    // ── V4 tab/document identity (added for REPL; appended at the end) ───────

    /// Stable tab identifier for the active document tab.
    fn tab_id(&self) -> u64 {
        self.tab_index() as u64
    }

    /// Open (or refresh) the host-side V4 shared document view for `tab_id`
    /// and return the mapping information. In-process hosts implement this;
    /// out-of-process plugin proxies return `None`.
    fn document_view_v4(&mut self, tab_id: u64) -> Option<crate::shm::DocumentViewInfo> {
        let _ = tab_id;
        None
    }

    /// Close the host-side V4 shared document view for `tab_id`.
    fn close_document_view_v4(&mut self, tab_id: u64) {
        let _ = tab_id;
    }

    // ── Batch entities (added after API v4; appended at the very end so older
    // plugins compiled without it keep stable vtable indices) ────────────────

    /// Add multiple entities to the active document, returning their handles.
    /// The default implementation calls [`add_entity`](Self::add_entity) for each
    /// entity; hosts should override it for batch efficiency.
    fn add_entities(&mut self, entities: Vec<EntityType>) -> Vec<Handle> {
        entities.into_iter().map(|e| self.add_entity(e)).collect()
    }

    /// Return the filesystem path of the document in tab `tab_id`, if any.
    /// Added in API v5. The default returns `None`; in-process hosts should
    /// override it to expose the real path.
    fn document_path(&self, tab_id: u64) -> Option<PathBuf> {
        let _ = tab_id;
        None
    }

    /// Read a named host setting without invoking the command dispatcher.
    /// Names are case-insensitive. Hosts may support only a subset.
    ///
    /// The desktop host reads `CLAYER`, `CTAB`, `CTEXTSTYLE`, `CELTYPE` and
    /// `SNAPANG`, and the drawing's numeric and point variables (`INSUNITS`,
    /// `MEASUREMENT`, `LUNITS`, `LUPREC`, `LTSCALE`, `PDMODE`, `PDSIZE`,
    /// `EXTMIN`, `EXTMAX`, `LIMMIN`, `LIMMAX`, `INSBASE`, ...). A point comes
    /// back as text, `"x,y,z"`.
    fn system_variable(&self, _name: &str) -> Option<HostSettingValue> {
        None
    }

    /// Set a named host setting without re-entering command dispatch.
    /// Returns the effective value after validation and normalization.
    fn set_system_variable(
        &mut self,
        _name: &str,
        _value: HostSettingValue,
    ) -> Result<HostSettingValue, String> {
        Err("system variable is not supported by this host".to_owned())
    }

    /// Validate and replace existing entities as one undo step (API v7).
    /// Callers clone entities from `document()`, change only supported fields,
    /// and submit all replacements together. Identity and kind must be kept.
    /// An error leaves the drawing and undo history unchanged.
    fn update_entities_transaction(
        &mut self,
        _label: &str,
        _entities: Vec<EntityType>,
    ) -> Result<(), String> {
        Err("entity transactions are not supported by this host".to_owned())
    }

    /// Current ordered selection in this session's tab (API v7).
    fn selection(&self) -> Vec<Handle> {
        Vec::new()
    }

    /// Replace this tab's selection with exactly these handles, in order.
    /// Missing or duplicate handles reject the request (API v7).
    fn set_selection(&mut self, _handles: &[Handle]) -> Result<(), String> {
        Err("selection writes are not supported by this host".to_owned())
    }

    /// Run a kernel-backed solid operation and return the created or updated
    /// entity's handle. Refused, without any change, when the kernel cannot
    /// perform it losslessly (API v7, additive).
    fn solid_operation(&mut self, _operation: SolidOperation) -> Result<Handle, String> {
        Err("solid operations are not supported by this host".to_owned())
    }

    /// Drive an OCS command from a script (API v7, additive). Refused, with
    /// nothing run, when another command is already active or the request names
    /// a command that could end the session or re-enter the plugin.
    fn run_command(&mut self, _request: CommandRequest) -> Result<CommandOutcome, String> {
        Err("running commands is not supported by this host".to_owned())
    }

    /// Create, change, rename, delete or select a drawing table record (API v7,
    /// additive). Returns the record's handle; a refusal changes nothing.
    fn table_operation(&mut self, _operation: TableOperation) -> Result<Handle, String> {
        Err("table operations are not supported by this host".to_owned())
    }
    /// Add a layer to the active document with full initial properties.
    /// If an optional property in `config` is `None`, standard CAD defaults are applied.
    /// Returns `None` if the layer already exists or `config.name` is invalid.
    ///
    /// To modify properties of an already existing layer, use [`modify_layer`](Self::modify_layer).
    fn add_layer(&mut self, config: LayerConfig) -> Option<Handle> {
        let _ = config;
        None
    }

    /// Modify specified properties of an existing layer in the active document.
    /// Properties that are `None` in `config` are left untouched as-is.
    /// Returns `false` if the layer does not exist or `config.name` is invalid.
    ///
    /// To create a new layer, use [`add_layer`](Self::add_layer).
    fn modify_layer(&mut self, config: LayerConfig) -> bool {
        let _ = config;
        false
    }

    /// Run a command on the active document tab's command line (AutoLISP / script style).
    ///
    /// The string is passed to the host command driver as if entered into the command line,
    /// supporting command names, space/newline-delimited arguments, AutoCAD-compatible
    /// aliases, system variable setters (e.g. `SETVAR PDMODE 3`), and display commands
    /// (e.g. `VSCURRENT`, `GRID`, `SNAP`).
    ///
    /// Headless async tasks spawned by commands are automatically pumped through the host
    /// application lifecycle.
    ///
    /// Returns `true` if the command was recognized and initiated successfully, `false` otherwise.
    fn execute_command(&mut self, cmd: &str) -> bool {
        let _ = cmd;
        false
    }

    // ── XRecord objects (added in API v8; appended at the end to keep vtable
    // indices stable for older plugins) ────────────────────────────────────────

    /// Create an XRecord named `name` under the dictionary addressed by
    /// `dictionary_path`, storing `entries` (API v8, additive).
    ///
    /// The path is walked from the drawing's named-object dictionary root: an
    /// empty path addresses the root itself, and missing intermediate
    /// dictionaries are created, so the first call seeds a plugin's namespace.
    /// Every refusal (empty or invalid names, a path fragment that names a
    /// non-dictionary object, a name already in use — comparison is
    /// case-insensitive like dictionary lookups) happens **before** any change,
    /// so a failure leaves the drawing untouched. The record is registered in
    /// its dictionary and owned by it; the returned handle stays stable across
    /// updates.
    ///
    /// Undo composition — the entity-write contract: a mutation performed
    /// inside a plugin-opened [`HostApi::push_undo`] scope joins that scope,
    /// so an operation composed of several writes (entities and XRecords) is
    /// a single undo step under the plugin's own label. With no scope open
    /// the change is one undo step of its own. Refusals record no undo step.
    fn create_xrecord(
        &mut self,
        _dictionary_path: &[&str],
        _name: &str,
        _entries: Vec<XRecordEntry>,
    ) -> Result<Handle, String> {
        Err("XRecord creation is not supported by this host".to_owned())
    }

    /// Read the entries of an existing XRecord by name (API v8, additive).
    ///
    /// Resolves `dictionary_path` under the named-object dictionary root and
    /// returns the record's entries verbatim. Fails when the path, the name or
    /// the record is missing, or when the name addresses a different object
    /// kind.
    fn read_xrecord(
        &self,
        _dictionary_path: &[&str],
        _name: &str,
    ) -> Result<Vec<XRecordEntry>, String> {
        Err("XRecord reads are not supported by this host".to_owned())
    }

    /// Atomically replace the entries of an existing XRecord (API v8,
    /// additive). Returns the record's handle.
    ///
    /// Neither the path nor its dictionaries are created: an update must never
    /// create anything silently. Identity (handle, owning dictionary) stays;
    /// the entries and their DWG handle-reference stream are replaced and the
    /// record's verbatim raw payloads are dropped, so subsequent saves emit the
    /// new content. A refusal leaves the drawing untouched. Undo composition —
    /// the entity-write contract: a mutation inside a plugin-opened
    /// [`HostApi::push_undo`] scope joins that scope; with no scope open it is
    /// one undo step of its own, whose undo restores the previous entries
    /// byte-exactly. Refusals record no undo step.
    fn update_xrecord(
        &mut self,
        _dictionary_path: &[&str],
        _name: &str,
        _entries: Vec<XRecordEntry>,
    ) -> Result<Handle, String> {
        Err("XRecord updates are not supported by this host".to_owned())
    }

    /// Delete the XRecord named `name` and its dictionary entry (API v8,
    /// additive). Returns the removed record, so audits keep the prior
    /// content. A refusal leaves the drawing untouched. Undo composition — the
    /// entity-write contract: a mutation inside a plugin-opened
    /// [`HostApi::push_undo`] scope joins that scope; with no scope open it is
    /// one undo step of its own. Refusals record no undo step.
    fn delete_xrecord(
        &mut self,
        _dictionary_path: &[&str],
        _name: &str,
    ) -> Result<XRecord, String> {
        Err("XRecord deletes are not supported by this host".to_owned())
    }

    /// List the XRecord names under the dictionary addressed by
    /// `dictionary_path` (API v8, additive).
    ///
    /// Returns only the dictionary keys that address XRecord objects, in the
    /// dictionary's own order, so consumers can audit and export without
    /// knowing names in advance. Fails when the path does not address a
    /// dictionary.
    fn list_xrecords(&self, _dictionary_path: &[&str]) -> Result<Vec<String>, String> {
        Err("XRecord listing is not supported by this host".to_owned())
    }

    // ── Reference-only designation (added in API v9; appended at the end to
    // keep vtable indices stable for older plugins) ──────────────────────────

    /// Mark (or unmark) an entity as reference-only (API v9, additive).
    ///
    /// A reference-only entity is plugin-owned display geometry: it renders
    /// normally, but the built-in edit paths treat it like an entity on a
    /// locked layer — it is neither grip-editable nor click-pickable (a click
    /// passes through to the editable entity underneath), window and box
    /// selections skip it, and user-driven writes refuse it. The owning
    /// plugin keeps full write access through this API: `update_entity`,
    /// [`update_entities_transaction`](HostApi::update_entities_transaction),
    /// `remove_entity` and the XDATA record methods keep working, so derived
    /// display geometry can be refreshed in place.
    ///
    /// The designation is a scene-scoped, in-memory set keyed by entity
    /// handle: it is not written to the saved DWG/DXF (the host stores no
    /// state of its own in the file), it lives and dies with the drawing tab,
    /// and a reloaded drawing starts undesignated — the owning plugin
    /// re-marks its entities when
    /// [`on_notification`](BuiltinPlugin::on_notification) delivers
    /// [`HostNotification::DocumentOpened`], making the in-memory designation
    /// fully equivalent to a persisted one for its use case.
    ///
    /// Fails when the entity does not exist or is on a locked layer (the same
    /// policy as the XDATA record methods). A call that would not change the
    /// current designation succeeds without recording an undo step — and
    /// without publishing a document change, so idempotent re-marks cannot
    /// loop with the notification dispatch.
    ///
    /// Undo composition — the entity-write contract: a change performed
    /// inside a plugin-opened [`HostApi::push_undo`] scope joins that scope,
    /// so an operation composed of several writes is a single undo step under
    /// the plugin's own label. With no scope open the change is one undo step
    /// of its own. Refusals record no undo step.
    fn set_reference_only(&mut self, _handle: Handle, _reference_only: bool) -> Result<(), String> {
        Err("The reference-only designation is not supported by this host".to_owned())
    }

    // ── Plugin grip surface (added in API v11; appended at the end to keep
    // vtable indices stable for older plugins) ─────────────────────────────

    /// Enumerate an entity's built-in grips (API v11, additive).
    ///
    /// Generic over every entity type: the answer is whatever the entity's
    /// own grip producers define (an arc answers center / endpoints / its
    /// mid-arc radius grip, a spline its control points, and so on), with
    /// the same shape and field semantics the automation `grips` read and
    /// the interactive viewport expose. Reference-only entities and entities
    /// on locked layers answer an empty list — the same answer the viewport
    /// would give. Fails when the handle does not address an entity.
    fn entity_grips(&self, _handle: Handle) -> Result<Vec<GripInfo>, String> {
        Err("Grip reads are not supported by this host".to_owned())
    }

    /// Perform the grip edit an interactive drag would perform (API v11,
    /// additive).
    ///
    /// Drives exactly the machinery of the viewport drag: the entity-typed
    /// grip modes, snaps and ortho/polar off for the move, one undo step
    /// labeled like the interactive grip step, byte-exact restore on undo.
    /// Refusal parity: fails for unknown handles/grip ids, reference-only
    /// entities and locked layers.
    fn grip_drag(&mut self, _handle: Handle, _grip_id: usize, _to: [f64; 3]) -> Result<(), String> {
        Err("Programmatic grip drags are not supported by this host".to_owned())
    }

    /// Declare (or clear) the calling plugin's custom grip points (API v11,
    /// additive, V4 transport only).
    ///
    /// The batch REPLACES the plugin's previous set; an empty batch clears
    /// it. The host renders the points, hit-tests them with priority over
    /// click-through (reference-only) geometry, and routes drags on them
    /// back to the plugin as [`CustomGripEvent`]s — the host interprets
    /// nothing. Declaration is session display state: nothing of it is
    /// persisted in the drawing file, and the set dies with the plugin's
    /// process. Only the declaring plugin ever receives its grips' events.
    ///
    /// `plugin_id` is fixed by the host transport: over the plugin IPC the
    /// host attaches the serving process's own manifest id and refuses
    /// mismatches, so a plugin cannot declare grips in another plugin's
    /// name.
    fn set_plugin_grips(
        &mut self,
        _plugin_id: &str,
        _grips: Vec<PluginGrip>,
    ) -> Result<(), String> {
        Err("Plugin grip points are not supported by this host".to_owned())
    }

    /// Read specific entities by handle (API v12, additive).
    ///
    /// Cheap per-entity reads for refresh passes — the answer is the LIVE
    /// document's current entity per handle (`None` for an unknown handle).
    /// Use these instead of re-taking a document snapshot after writes: the
    /// session snapshot is invalidated by every write, so read-after-write
    /// loops through it re-clone the full document each time (the per-boundary
    /// quadratic). Fails only when the tab does not exist.
    fn entities_batch(&self, _handles: &[Handle]) -> Result<Vec<Option<EntityType>>, String> {
        Err("Per-entity batch reads are not supported by this host".to_owned())
    }

    /// Create a parametric constraint directly (API v13, additive).
    ///
    /// The same constraint machinery the interactive COINCIDENT/TANGENT
    /// commands drive, reached without the command line: no prompts, no echo,
    /// the solver runs exactly as interactively. Undo composition follows the
    /// entity-write contract — inside a plugin-opened `push_undo` scope the
    /// constraint joins that scope (a re-bond composed of writes + constraints
    /// is the plugin's single labeled step); with no scope open it is one
    /// step of its own. A refusal records no undo step.
    fn constraint_create(&mut self, _constraint: PluginConstraint) -> Result<(), String> {
        Err("Constraint creation is not supported by this host".to_owned())
    }

    /// Read several XRecords in ONE round trip (API v13, additive).
    ///
    /// The batched twin of [`read_xrecord`](Self::read_xrecord): one frame
    /// answers every entry (`Err` per missing path/name instead of failing
    /// the whole batch). The consumer's store walk collapses from
    /// list + one-read-per-record to one frame.
    fn read_xrecords(
        &self,
        _reads: &[XRecordRead],
    ) -> Result<Vec<Result<Vec<XRecordEntry>, String>>, String> {
        Err("Batched XRecord reads are not supported by this host".to_owned())
    }

    /// Atomically replace the entries of several XRecords in ONE round trip
    /// (API v13, additive). Each entry applies the single-update contract
    /// (identity kept, verbatim raw payloads dropped, refusals change
    /// nothing); the whole batch shares the entity-write undo contract —
    /// inside a plugin-opened `push_undo` scope the batch joins that scope as
    /// the plugin's single labeled step.
    fn update_xrecords(
        &mut self,
        _updates: &[XRecordUpdate],
    ) -> Result<Vec<Result<Handle, String>>, String> {
        Err("Batched XRecord updates are not supported by this host".to_owned())
    }

    /// List the XRecord names under a dictionary path WITH their entries in
    /// ONE answer (API v13, additive): most consumers' whole discovery walk
    /// (list, then one read per record) becomes a single frame.
    fn list_xrecords_with_entries(
        &self,
        _dictionary_path: &[&str],
    ) -> Result<Vec<(String, Vec<XRecordEntry>)>, String> {
        Err("XRecord listing with entries is not supported by this host".to_owned())
    }

    /// Turn a declared parametric alignment chain into reference-only display
    /// geometry (API v14, additive).
    ///
    /// The chain carries the record layer's truth — curve definitions
    /// (length, driving-quantity ramp, cross-slope data) plus the continuity
    /// fields (start point/direction) — and the host owns the evaluation: a
    /// generic Gaussian-Q kernel plus the closed forms a ramp class admits.
    /// Staged carriers: an exact standard entity where one exists GENERICALLY
    /// (a constant-zero plan ramp stages a line, any other constant plan
    /// ramp an arc; a profile ramp that is an at-most-quadratic polynomial
    /// in the run stages an exact clamped Hermite spline of degree up to 3,
    /// a constant profile slope a line) and bounded LwPolyline chords within
    /// `tolerance` otherwise — the same `tolerance` bounds the seam
    /// audit's continuity reads (one declared accuracy per chain) —
    /// (table ramps and every other law — measured,
    /// refined chords, never a guessed shape). Chains are audited
    /// seam-to-seam BEFORE anything is staged
    /// (refused with the audited pair named on mismatch), and a chain that
    /// would need more display entities than
    /// [`MAX_PLUGIN_LIVE_ENTITIES`](self::MAX_PLUGIN_LIVE_ENTITIES) publishes
    /// nothing.
    ///
    /// Undo composition follows the entity-write contract: inside a
    /// plugin-opened `push_undo` scope the staged set joins that scope; with
    /// no scope open it is one step of its own; a refusal records no step.
    /// Staged entities are marked reference-only (plugin-owned display
    /// data). The designation is scene state: like every designation it is
    /// not persisted in the file, so the marking plugin re-applies it on
    /// reopen.
    fn stage_parametric_chain(
        &mut self,
        _reference_name: &str,
        _chain: &ParametricChain,
        _tolerance: f64,
    ) -> Result<Vec<Handle>, String> {
        Err("Parametric chain staging is not supported by this host".to_owned())
    }

    /// Evaluate a declared parametric chain at chain position `s` (API v14,
    /// additive): the same engine the staging builds from, exposed for
    /// placement and basis-curve reads. Pure evaluation — no document state,
    /// no undo, no publish. `s` is measured along the chain's declared
    /// segment lengths from the chain start; outside the chain it answers
    /// `None`.
    fn sample_parametric(
        &self,
        _chain: &ParametricChain,
        _s: f64,
    ) -> Result<Option<ParametricSample>, String> {
        Err("Parametric chain sampling is not supported by this host".to_owned())
    }

    /// Register a plugin-scoped entity class (API v14, additive).
    ///
    /// The declaration is session state, exactly like the grip sets: the
    /// drawing file keeps only what the instances stamp (XDATA identity keys,
    /// entity-bound records) plus the plugin's own record-layer XRecords —
    /// never the class registry itself; the plugin re-declares on load and
    /// after its own notification boundaries. Re-registering `class_id` with
    /// an EQUAL schema (same `schema_version` and grip ids/parameter names)
    /// is idempotent; any mismatch refuses without touching the live class —
    /// a parameter-set change is a RE-STAGE of the instances, not a hot
    /// schema swap.
    ///
    /// The grip schema is the class-scoped surface of the v11 grip rule: it
    /// names the parameters the class's grips drive (ids must match the
    /// plugin's later [`HostApi::set_plugin_grips`] declarations for these
    /// instances). The host stores it for instance validation; grip
    /// interpretation stays the plugin's per the v11 law.
    fn register_plugin_class(
        &mut self,
        _plugin_id: &str,
        _class: &PluginClassDeclaration,
    ) -> Result<(), String> {
        Err("Plugin class registration is not supported by this host".to_owned())
    }

    /// Stage one instance of a session-registered plugin class (API v14,
    /// additive).
    ///
    /// The generalized form of [`HostApi::stage_parametric_chain`]: the
    /// instance carries only identity — class id, schema version, layer,
    /// record linkage — and a HOST-EVALUATED display descriptor
    /// ([`PluginDisplayDescriptor`]; the v14 parametric engine is the first
    /// host-known kind, so a `ParametricChain` design stages through exactly
    /// the audited-seam/cap/refusal/undo contract of chain staging). The
    /// staged set is reference-only (the designation machinery transfers
    /// verbatim), stamped with the class identity key, and the instance's
    /// record linkage (`design_record`) rides the record layer as usual: the
    /// definition lives in records, host and file never grow a new runtime.
    /// Refusals precede any change and record no undo step; otherwise one
    /// batched add, one geometry bump, one publish — one step of its own
    /// unless a plugin `push_undo` scope is open, which it joins.
    fn stage_plugin_class_instance(
        &mut self,
        _plugin_id: &str,
        _instance: &PluginClassInstance,
    ) -> Result<Vec<Handle>, String> {
        Err("Plugin class instance staging is not supported by this host".to_owned())
    }

    /// Read a class instance's declared parameter rows (API v14, additive —
    /// the read half of the session's properties pair, carried by
    /// `PluginRequest::PluginClassInstanceProperties` /
    /// `PluginResponse::PluginClassInstancePropertiesResult`). The identity
    /// stamp (the entity's XDATA class key) resolves the session-registered
    /// class, the design-record linkage resolves under the named-object
    /// dictionary, and every declared parameter reads its (name, value)
    /// entry pair. Values print exactly what the record row carries — the
    /// host never invents units or semantics for a name or kind it does not
    /// declare. Strict on purpose: a missing entity, a broken linkage or a
    /// stale schema refuses — the caller re-stages.
    fn plugin_class_instance_properties(
        &self,
        _handle: Handle,
    ) -> Result<Vec<PluginClassParameterRow>, String> {
        Err("Plugin class instance properties are not supported by this host".to_owned())
    }

    /// Display a declarative modal form dialog and wait for user submission
    /// or cancellation (API v7, additive).
    fn show_dialog(&mut self, _dialog: crate::dialog::DialogDef) -> crate::dialog::DialogResponse {
        crate::dialog::DialogResponse::Cancelled
    }

    /// Register a persistent dockable side panel / palette (API v7, additive).
    fn register_panel(&mut self, _panel: crate::dialog::PanelDef) -> bool {
        false
    }

    /// Update control values in a persistent dockable side panel (API v7, additive).
    fn update_panel(
        &mut self,
        _panel_id: &str,
        _values: std::collections::HashMap<String, crate::dialog::DialogValue>,
    ) -> bool {
        false
    }

    /// Toggle visibility or show/hide a persistent dockable side panel (API v7, additive).
    fn toggle_panel(&mut self, _panel_id: &str, _show: Option<bool>) -> bool {
        false
    }
}

/// **Parametric-chain staging types (api 14, the v2 generic shapes).**
///
/// The declared-data contract for alignment chains the host stages from
/// ramps declared as DATA. The wire carries the record layer's declared
/// truth only — curve definitions and continuity — and the host owns the
/// evaluation (curve integrals), the seam audit and the reference-only
/// display staging. There is NO host-side species vocabulary: which
/// quantity a ramp drives per plane, which laws the record model restates
/// with which ramp payloads, and what a segment's ends must be are the
/// declaring plugin's own semantics; the host refuses only what it can
/// state generically (see [`CurveSegmentDef`]). The IFC 4.3 alignment
/// libraries that reuse this contract live in plugins (e.g. the civil-ifc
/// crate), not in this host.
///
/// Staged carriers: constant plan ramps as `Line`/`Arc` (exact), profile
/// slope ramps that are at-most-quadratic polynomials in the run as exact
/// clamped Hermite splines of degree up to 3 (const slopes as lines), and
/// everything else as measured-deviation LwPolyline chords within the
/// declared tolerance. A HELIX entity is never produced: the codec's
/// Helix is a 3D constant-pitch space spiral, and a plan ramp curve is a
/// 2D law — staging one for the other would be mathematically wrong.
///
/// The plane a parametric chain lives in. A chain is single-plane: the
/// driving quantity reads as the plan curvature in the plan plane and as
/// the profile slope in the profile plane (the record model's two sheets).
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParametricPlane {
    /// Plan view: world (x, y) coordinates in the drawing's model plane,
    /// bearings in radians CCW from +x (±2π-legal, normalized by the host).
    Plan,
    /// Profile sheet: (station, elevation) coordinates — x = the distance
    /// along the horizontal alignment, y = height, exactly the
    /// IfcAlignmentVerticalSegment frame. No rotation applies.
    Profile,
}

/// One declared segment of a parametric chain (api 14, the v2 shape).
///
/// Only continuity and the law ride the wire: the curve definition plus the
/// start point and direction. There is no species enum and no per-species
/// quantity vocabulary (gradients, radii, curvature-end fields, bank/cant
/// fields): the driving ramp IS the law, declared as data, and every
/// record-model reading of those names stays in the declaring plugin. The
/// host evaluates the generic contract only (see [`CurveSegmentDef`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametricSegment {
    /// The curve definition: length, the driving ramp, the cross-slope data.
    pub def: CurveSegmentDef,
    /// Start point: plan = world (x, y); profile = (station, elevation).
    pub start_point: [f64; 2],
    /// Declared start direction, radians, ±2π-legal. Plan: bearing. Profile:
    /// the slope angle as the record declares it (the slope itself is the
    /// driving ramp's own data; this field is the continuity read the seam
    /// audit and joins consult).
    pub start_direction: f64,
}

/// The declared chain (API v14, additive): the record layer's truth, turned
/// into display geometry by the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametricChain {
    /// The plane the chain lives in.
    pub plane: ParametricPlane,
    /// The declared segments, start to end (at least one; each length must
    /// be greater than zero — a zero-length terminator is a plugin's
    /// staging detail, not a host contract, and refuses).
    pub segments: Vec<ParametricSegment>,
    /// Layer name for the staged display entities (the consumer's own
    /// display convention). Empty = the host's default layer.
    pub layer: String,
}

/// One evaluated point of a parametric chain (API v14, additive).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ParametricSample {
    /// Plane coordinates of the point (plan = world (x, y); profile =
    /// (station, elevation)).
    pub point: [f64; 2],
    /// Unit tangent in plane coordinates (plan = (cos θ, sin θ); profile =
    /// (1, gradient) / |(1, gradient)|).
    pub direction: [f64; 2],
}

/// **Generic curve-definition types (api 14; the v2 in-place reshaping).**
///
/// The host knows NO species: a curve is a driving-quantity ramp declared as
/// DATA over the unit station parameter, evaluated by the host's generic
/// integrator, audited at the declaring instance's own tolerance (the
/// seams' continuity bound — one declared accuracy for welds and chords),
/// and staged with
/// generic carriers (exact Hermite for the polynomial class, measured-
/// deviation chords otherwise). Species semantics (which quantity drives,
/// which ramps restate which law, what a segment's ends must be) are the
/// declaring plugin's own checks — the host refuses only what it can state
/// generically. These ARE the v14 envelope's shapes: the envelope was
/// reshaped IN PLACE before release (api_version stays 14) because no
/// deployed plugins pre-date the reshaping.
///
/// A curve ramp: the driving quantity's values over one unit parameter
/// (not `Copy`: the sampled arm carries a table; clone freely — staging
/// copies payloads once, the per-move tier borrows).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Ramp {
    /// An at-most-cubic polynomial ramp, coefficients ascending
    /// (`value(xi) = Σ coeffs[i]·xi^i`); the exact class: the ramp AND its
    /// integral are closed forms, and exact carriers derive generically.
    Polynomial {
        /// Coefficients in use (0..=3; higher entries ignored).
        degree: u8,
        /// Ascending coefficients, `coeffs[i]·xi^i`.
        coeffs: [f64; 4],
    },
    /// A piecewise-linear ramp: `count` active knots ascending in xi over
    /// [0, 1]; the integral accumulates per-interval quadratics. The same
    /// 6-slot layout the declared vocabulary shipped.
    Piecewise {
        /// Active knots (0 < count <= 6; the rest are padding).
        count: u32,
        /// The (xi, value) knots, ascending in xi.
        knots: [(f64, f64); 6],
    },
    /// A uniformly sampled table: `n` values at xi = i/(n-1); looked up
    /// O(log n) with a linear tail; the host precomputes the stage-time
    /// cumulative integrals so drag-time re-integration is O(log n) +
    /// a small exacting tail (the performance-critical drag tier).
    Sampled {
        /// Table length (1..=4096).
        n: u32,
        /// The values, uniform in xi.
        values: Vec<f64>,
    },
}

/// One generically-defined segment: continuity state exactly as the
/// segment struct carries it, plus the ramp payload in place of the
/// species vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveSegmentDef {
    /// The record-model length (plan = arc length; profile = horizontal run).
    pub length: f64,
    /// The driving-quantity ramp over the unit parameter.
    pub ramp: Ramp,
    /// The secondary-quantity ramp as unnamed data (the cross-slope term a
    /// plugin's cant layout carries; the host integrates it for the
    /// record's secondary rows but slides NOTHING about the plane curve).
    pub cross_slope: Option<Ramp>,
}

/// What a plugin grip re-anchors, in record-model terms only (the generic
/// replacement for per-species drive semantics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorLaw {
    /// Drag the end point in the sheet (the plan piers).
    EndStationToXY,
    /// Re-anchor the end elevation.
    EndElevation,
    /// Re-anchor the end slope (grade).
    EndGrade,
    /// Scale the ramp's end value of the driving quantity.
    EndCurvatureScale,
    /// Re-run the segment's length.
    Length,
}

/// **Plugin class-object staging types (API v14, additive).**
///
/// The generalization of the parametric staging contract: a plugin-declared
/// entity CLASS whose instances carry only identity (class id, schema
/// version, layer, record linkage) while the DEFINITION stays in the record
/// layer (XRecords — the file's only source of truth) and the display is
/// HOST-EVALUATED from a design descriptor. Alignment chains are exactly the
/// `ParametricChain` descriptor kind: a class instance whose design is a
/// chain stages through [`HostApi::stage_parametric_chain`]'s audited-seam,
/// cap, refusal, undo and reference-only discipline verbatim. Older hosts
/// simply answer the class methods with the standard degradation error.
///
/// The class declaration itself is session state like the v11 grip sets —
/// never written to the drawing file; instances stamp only XDATA identity
/// keys and record linkages, so a file without the plugin still renders its
/// staged (standard-entity) display and reads its records, it just has no
/// re-marking, re-staging or grip semantics until the plugin loads again.
///
/// A plugin-declared entity class (API v14).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginClassDeclaration {
    /// Stable identity key the host stamps on instances (XDATA class key —
    /// same key the plugin declares as its v10 `entity_classes` identity for
    /// presentation purposes; the host stamps it on HOST-staged instances so
    /// instance sets survive and re-associate across sessions).
    pub class_id: String,
    /// Monotone schema version of the class's parameter set (the record
    /// layer's truth). An instance refuses to stage against a live class
    /// whose version differs — the consumer re-declares and re-stages
    /// instead, keeping the records authoritative.
    pub schema_version: u32,
    /// The class's grip schema (ids and the parameter names they drive,
    /// matching the plugin's later `set_plugin_grips` declarations for its
    /// instances). The host uses it for instance validation; grip
    /// interpretation stays the declaring plugin's per the v11 law.
    pub grip_schema: Vec<PluginGripSchemaEntry>,
    /// The class's parameter schema (API v14, additive; serde default keeps
    /// earlier declarations readable): WHAT the parameters are — names, value
    /// kinds, and which rows host surfaces (the properties panel) may write.
    /// Declaring parameters turns the record layer's truth into named
    /// properties; the values themselves stay on the instance's design
    /// record row. Names must be unique and non-empty (registering refuses
    /// otherwise).
    #[serde(default)]
    pub parameters: Vec<PluginClassParameter>,
}

/// One grip in a class's schema (API v14): which parameter of the class
/// schema this grip drives. Drag semantics remain the declaring plugin's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginGripSchemaEntry {
    /// Grip id â€” links a schema row to the plugin's `set_plugin_grips`
    /// declaration (the drag's `CustomGripEvent::grip_id`).
    pub grip_id: u64,
    /// Parameter name in the class's record layer this grip drives
    /// (e.g. `"peak_gradient"`).
    pub param_name: String,
    /// (API v14, additive) The design point this grip re-anchors in a
    /// descriptor-backed drag: when the drag's Begin answer hands the host
    /// a live descriptor design, the host re-derives that design per
    /// coalesced cursor move through THIS declared rule instead of
    /// streaming Move frames. `None` (the serde default: earlier
    /// declarations and streaming-only grips) keeps the v11/v12 semantics
    /// untouched.
    #[serde(default)]
    pub drives: Option<PluginGripAnchor>,
}

/// The design point a descriptor-backed grip re-anchors (api 14; the v2
/// anchor-law shape — the v14 envelope was reshaped in place before
/// release, so the earlier empty drive struct never shipped). The cursor's
/// move REPLACES that point of the declared design: the segment's named
/// end follows the cursor per the declared anchor law, the driving ramp
/// re-derives per its own integral law (one generic arm-consistent
/// re-anchor — no per-species host code), the joints re-declare downstream
/// from the engine's own sampling, and the chain's own audit gates every
/// move's design. The host serves only what it can state generically: the
/// profile plane's end-side anchor laws. A drive outside that set (the
/// plan plane, the START side, a segment index out of the descriptor's
/// range) keeps the v12 streaming path, with one headline in the error
/// queue naming the constraint. Future drives append as NEW serde-default
/// fields on this struct (never new positions inside it) so older hosts
/// deserialize them harmlessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginGripAnchor {
    /// Zero-based index into the descriptor chain's segments.
    pub segment: u32,
    /// Whether the grip re-anchors that segment's START point; `false`
    /// names the segment's end. The host serves the END side only (an
    /// END-point drag fully re-derives the section through its own ramp
    /// law); a declared START side keeps the streaming path until a host
    /// release serves it.
    pub at_start: bool,
    /// The record-model quantity this grip re-anchors.
    pub anchor: AnchorLaw,
}

/// One declared class parameter (API v14, additive): a name, a value kind,
/// and an editability. The schema lets host surfaces render a class
/// instance's parameters without the plugin's source: the value lives on
/// the instance's design record row (the record layer stays the only
/// truth); the declaration only says what kinds exist and which rows the
/// host's property surfaces may write. The host never invents parameter
/// semantics for a name or kind it does not know.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginClassParameter {
    /// The parameter's name as the record row and host surfaces address it.
    pub name: String,
    /// The value kind the record row carries.
    pub value_kind: PluginClassParameterKind,
    /// Whether host property surfaces may write this row. A write composes
    /// as one undo step through the session's XRecord update path, and the
    /// declaring plugin re-stages from the record's dirty notification.
    /// Read-only rows render but never write.
    pub editable: bool,
}

/// The value kind of one declared class parameter (API v14, additive).
/// Kinds append at the END only (the wire encodes a variant by its
/// position).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginClassParameterKind {
    /// A floating-point quantity; the record row carries an XRecord double
    /// (an integral reading casts to one).
    Real,
    /// A count; the record row carries an XRecord integer.
    Integer,
    /// Free text; the record row carries an XRecord string.
    Text,
}

/// One read row of a class instance's declared parameters (API v14,
/// additive; the wire carrier of
/// [`HostApi::plugin_class_instance_properties`]). The value is exactly
/// what the record row carries — only the declared kind's own reading,
/// never a unit or semantic conversion; `None` = the record does not
/// carry this parameter's pair yet (the row renders the declaration).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginClassParameterRow {
    /// The parameter's declared name.
    pub name: String,
    /// The declared kind the row prints under.
    pub value_kind: PluginClassParameterKind,
    /// Whether host property surfaces may write this row.
    pub editable: bool,
    /// The record row's own printed value; `None` when absent.
    pub value: Option<String>,
}

/// One staged instance of a registered plugin class (API v14): identity plus
/// a host-evaluated design descriptor. Records carry the truth; the host
/// never re-interprets anything the plugin does not declare here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginClassInstance {
    /// Must equal a class registered in this session (else the staging
    /// refuses before any change).
    pub class_id: String,
    /// The schema version the instance was authored against (refuses on
    /// mismatch with the live declaration).
    pub schema_version: u32,
    /// The host-evaluated display design.
    pub design: PluginDisplayDescriptor,
    /// The instance's own tolerance (chain designs: BOTH readings — the
    /// carrier tolerance of the table-law ramps' chords AND the seam
    /// continuity bound of the chain's audited welds: the point seam
    /// reads it as meters, the plan's bearing seam as radians, the
    /// profile's slope seam as the gradient family). The declared
    /// accuracy is the record model's own declaration, one number.
    #[serde(default)]
    pub tolerance: f64,
    /// Layer for the staged display set; empty = the host's default layer.
    pub layer: String,
    /// NOD-side record name holding this instance's parameter truth (the
    /// handles-to-records linkage the plugin maintains).
    #[serde(default)]
    pub design_record: String,
}

/// Host-evaluated display descriptor kinds (API v14, additive).
///
/// Kinds append at the END only (the wire encodes a variant by its
/// position); the first host-known kind is the v14 parametric engine for
/// alignment chains. A kind the host does not know refuses the staging at
/// the contract — never approximated, never silently dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginDisplayDescriptor {
    /// The v14 parametric engine is the host-known evaluator (the same wire
    /// grammar [`HostApi::stage_parametric_chain`] takes).
    ParametricChain(ParametricChain),
    // New descriptor kinds go after the existing ones: inserting one earlier
    // would make a plugin built against the previous API send its kind and
    // have the host read it as something else.
}

impl PluginDisplayDescriptor {
    /// (API v14, additive) A stable diagnostic label for the error queue and
    /// the logs. Later kinds append their arm here (the law: kinds append at
    /// the END only).
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::ParametricChain(_) => "ParametricChain",
        }
    }
}


/// One XRecord addressed by dictionary path + name (API v13, additive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XRecordRead {
    pub dictionary_path: Vec<String>,
    pub name: String,
}

/// One XRecord update in a batch (API v13, additive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XRecordUpdate {
    pub dictionary_path: Vec<String>,
    pub name: String,
    pub entries: Vec<XRecordEntry>,
}

/// Configuration properties for creating or modifying a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LayerConfig {
    pub name: String,
    pub color: Option<codec::types::Color>,
    pub linetype: Option<String>,
    pub lineweight: Option<codec::types::LineWeight>,
    pub off: Option<bool>,
    pub frozen: Option<bool>,
    pub locked: Option<bool>,
    pub plottable: Option<bool>,
    pub transparency: Option<codec::types::Transparency>,
    pub description: Option<String>,
}

/// Simplified, read-only entity kind exposed by [`DocumentReader`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderEntityKind {
    Point,
    Line,
    Circle,
    Arc,
    Polyline,
    Text,
    Other,
}

/// A 3D point returned by [`DocumentReader`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReaderPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A read-only view of one entity, borrowed from a [`DocumentReader`].
pub struct ReaderEntity<'a> {
    /// Entity handle in the host document.
    pub handle: Handle,
    /// Simplified entity type.
    pub kind: ReaderEntityKind,
    /// Name of the layer the entity lives on.
    pub layer_name: &'a str,
    /// If the entity is a point, its coordinates.
    pub point: Option<ReaderPoint>,
}

/// Read-only, zero-copy view of a CAD document.
///
/// For out-of-process plugins this is backed by host-owned shared memory. The
/// plugin receives only references into that mapping, so the document model is
/// not copied into the plugin's heap.
pub trait DocumentReader {
    /// Total number of entities in the document.
    fn entity_count(&self) -> usize;

    /// Iterate over all entities without allocating a full `Document`.
    fn for_each_entity(&self, f: &mut dyn FnMut(ReaderEntity<'_>));

    /// Look up a layer name by handle.
    fn layer_name(&self, handle: Handle) -> Option<&str>;

    /// Look up an APPID name by handle.
    fn app_id_name(&self, handle: Handle) -> Option<&str>;
}

impl ReaderEntityKind {
    /// Map a concrete `EntityType` to the simplified reader kind.
    pub fn from_entity(entity: &EntityType) -> Self {
        match entity {
            EntityType::Point(_) => ReaderEntityKind::Point,
            EntityType::Line(_) => ReaderEntityKind::Line,
            EntityType::Circle(_) => ReaderEntityKind::Circle,
            EntityType::Arc(_) => ReaderEntityKind::Arc,
            EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_)
            | EntityType::LwPolyline(_) => ReaderEntityKind::Polyline,
            EntityType::Text(_) | EntityType::MText(_) => ReaderEntityKind::Text,
            _ => ReaderEntityKind::Other,
        }
    }
}

/// In-process `DocumentReader` implementation that wraps a borrowed `Document`.
pub struct CadDocumentReader<'a>(pub &'a Document);

impl<'a> DocumentReader for CadDocumentReader<'a> {
    fn entity_count(&self) -> usize {
        self.0.entities().count()
    }

    fn for_each_entity(&self, f: &mut dyn FnMut(ReaderEntity<'_>)) {
        for entity in self.0.entities() {
            let kind = ReaderEntityKind::from_entity(entity);
            let layer_name = entity.common().layer.as_str();
            let point = match entity {
                EntityType::Point(p) => Some(ReaderPoint {
                    x: p.location.x,
                    y: p.location.y,
                    z: p.location.z,
                }),
                _ => None,
            };
            f(ReaderEntity {
                handle: entity.common().handle,
                kind,
                layer_name,
                point,
            });
        }
    }

    fn layer_name(&self, handle: Handle) -> Option<&str> {
        self.0
            .layers
            .iter()
            .find(|layer| layer.handle == handle)
            .map(|layer| layer.name.as_str())
    }

    fn app_id_name(&self, handle: Handle) -> Option<&str> {
        self.0
            .app_ids
            .iter()
            .find(|app_id| app_id.handle == handle)
            .map(|app_id| app_id.name.as_str())
    }
}

/// Typed read of per-tab plugin state stored under `plugin_id`.
pub fn plugin_state<'a, T: Any + Send + Sync>(
    host: &'a dyn HostApi,
    plugin_id: &str,
) -> Option<&'a T> {
    host.plugin_state_any(plugin_id)?.downcast_ref::<T>()
}

/// Typed mutable access to per-tab plugin state stored under `plugin_id`.
pub fn plugin_state_mut<'a, T: Any + Send + Sync>(
    host: &'a mut dyn HostApi,
    plugin_id: &str,
) -> Option<&'a mut T> {
    host.plugin_state_any_mut(plugin_id)?.downcast_mut::<T>()
}

/// Typed get-or-insert of per-tab plugin state stored under `plugin_id`.
///
/// Returns `None` when this host cannot hold state (out-of-process plugins)
/// or when existing state under `plugin_id` has a different type.
pub fn ensure_plugin_state<'a, T: Any + Send + Sync>(
    host: &'a mut dyn HostApi,
    plugin_id: &'static str,
    init: impl FnOnce() -> T,
) -> Option<&'a mut T> {
    let mut init = Some(init);
    let any = host.ensure_plugin_state_any(plugin_id, &mut || match init.take() {
        Some(make) => Box::new(make()),
        None => Box::new(()),
    });
    any?.downcast_mut::<T>()
}

#[cfg(feature = "host")]
mod repl {
    use serde::{Deserialize, Serialize};

    /// Source surface that submitted a REPL code snippet.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum CommandSource {
        CommandLine,
        Script,
        Editor,
    }

    /// Outcome of a REPL `ExecuteCode` request.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[non_exhaustive]
    pub struct ExecutionResult {
        pub success: bool,
        pub output: Option<String>,
        pub error: Option<String>,
        pub error_type: Option<String>,
        pub traceback: Option<String>,
        pub line_number: Option<u32>,
        pub column_number: Option<u32>,
        pub duration_ms: f64,
    }

    impl ExecutionResult {
        /// Create a new execution result.
        pub fn new(
            success: bool,
            output: Option<String>,
            error: Option<String>,
            error_type: Option<String>,
            traceback: Option<String>,
            position: Option<(u32, u32)>,
            duration_ms: f64,
        ) -> Self {
            let (line_number, column_number) = match position {
                Some((l, c)) => (Some(l), Some(c)),
                None => (None, None),
            };
            Self {
                success,
                output,
                error,
                error_type,
                traceback,
                line_number,
                column_number,
                duration_ms,
            }
        }
    }

    /// Spawn a thread that runs `f` and calls `respond` with the returned
    /// `ExecutionResult`. If `f` panics, `respond` is called with a panic error
    /// result instead, so the host never waits indefinitely for a callback that
    /// will never arrive.
    pub fn execute_code_guard<F>(
        respond: Box<dyn FnOnce(ExecutionResult) + Send>,
        f: F,
    ) -> std::thread::JoinHandle<()>
    where
        F: FnOnce() -> ExecutionResult + Send + 'static,
    {
        std::thread::spawn(move || {
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
                Ok(result) => result,
                Err(payload) => {
                    let msg = payload
                        .downcast_ref::<&str>()
                        .copied()
                        .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                        .unwrap_or("execute_code panicked");
                    ExecutionResult {
                        success: false,
                        output: None,
                        error: Some(format!("panic: {}", msg)),
                        error_type: Some("panic".to_string()),
                        traceback: None,
                        line_number: None,
                        column_number: None,
                        duration_ms: 0.0,
                    }
                }
            };
            respond(result);
        })
    }
}

#[cfg(feature = "host")]
pub use repl::{execute_code_guard, CommandSource, ExecutionResult};
