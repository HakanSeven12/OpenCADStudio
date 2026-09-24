//! Runtime host surface (`host` feature).
//!
//! [`HostApi`] is the `acadrust`-typed adapter a plugin uses at *dispatch* time
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

use crate::manifest::PluginManifest;
use crate::ribbon::CadModule;

// Re-export the acadrust crate and the types that appear in the HostApi trait
// so out-of-tree plugins can use them without adding their own acadrust
// dependency (which would risk an ABI-mismatching version).
pub use acadrust;
pub use acadrust::objects::{
    DictionaryCloningFlags, KnownXRecordKind, ProxyObjectReference, ProxyReferenceKind, XRecord,
    XRecordEntry, XRecordSection, XRecordValue, XRecordValueType,
};
pub use acadrust::xdata::{ExtendedDataRecord, XDataValue};
pub use acadrust::{CadDocument, EntityType, Handle};

use crate::ipc::protocol::{PluginRequest, PluginResponse};

/// Thread-safe handle that can issue host requests from plugin worker threads.
/// Out-of-process V4 plugins implement this; in-process hosts may return `None`.
pub trait PluginRequestSender: Send + Sync {
    fn request(&self, req: PluginRequest) -> Result<PluginResponse, PluginRequestError>;
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
    Box { center: [f64; 3], size: [f64; 3] },
    /// Right triangular prism from its minimum corner and extents.
    Wedge { origin: [f64; 3], size: [f64; 3] },
    /// Cylinder standing on the plane through `center`.
    Cylinder { center: [f64; 3], radius: f64, height: f64 },
    Sphere { center: [f64; 3], radius: f64 },
    Torus { center: [f64; 3], major: f64, minor: f64 },
    /// Regular pyramid with `sides` base edges.
    Pyramid { center: [f64; 3], radius: f64, height: f64, sides: u32 },
}

/// How two solids combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolidBoolean {
    Union,
    /// Remove the second solid from the first.
    Subtract,
    Intersect,
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
    DimStyleCreate { name: String, copy_from: Option<String>, properties: String },
    /// Change only the DimStyle fields the JSON object `properties` names.
    DimStyleModify { name: String, properties: String },
    /// Rename a text or dimension style; references to it follow. `Standard`
    /// is never renamed and a case-only change is refused.
    StyleRename { kind: TableStyleKind, from: String, to: String },
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
    LinetypeCreate { name: String, description: String, pattern: Vec<f64> },
    /// Change a simple linetype's description or pattern in place.
    LinetypeModify { name: String, description: Option<String>, pattern: Option<Vec<f64>> },
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
    Create { primitive: SolidPrimitive, layer: Option<String> },
    /// Apply a rigid transform, given as a column-major 4x4 matrix, to an
    /// existing solid. Scaling and shear are refused.
    Transform { handle: Handle, matrix: [f64; 16] },
    /// Build a planar region from one closed planar profile entity (a circle,
    /// ellipse, closed polyline or spline, ...). The new region goes on
    /// `layer`, or on the source's layer when `None`; `delete_source` erases
    /// the profile afterwards, as the REGION command does.
    RegionFromProfile { source: Handle, layer: Option<String>, delete_source: bool },
    /// Build a plane surface from one closed planar profile entity.
    SurfaceFromProfile { source: Handle, layer: Option<String>, delete_source: bool },
    /// Extrude a planar profile along `direction`: a solid when the profile is
    /// closed, a surface when it is open.
    Extrude { source: Handle, direction: [f64; 3], layer: Option<String>, delete_source: bool },
    /// Combine two solids into a new one. Both operands are consumed unless
    /// `keep_operands`; the result goes on `layer` or the first operand's
    /// layer. The kernel refuses some cases (coincident faces, cuts it has no
    /// closed form for); a refusal changes nothing. Curved operands can take a
    /// second or two, and the host thread is busy meanwhile.
    /// Embed a picture file as an OLE frame whose bottom-left corner is at
    /// `origin`, `width` drawing units wide, keeping the picture's aspect.
    /// PNG, JPEG and BMP are stored as read; other formats are re-encoded as
    /// PNG. The frame goes on `layer` (layer `0` when `None`).
    EmbedPicture { path: String, origin: [f64; 3], width: f64, layer: Option<String> },
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
    InputLine { line: String },
    Cancel,
    DocumentChanged { version: u64 },
    SelectionChanged { handles: Vec<Handle> },
    Raw(Vec<u8>),
    /// V4 snapshot changed for a specific tab. Discriminant 5.
    DocumentChangedV4 { tab_id: u64, version: u64 },
    /// V4 tab closed notification. Discriminant 6.
    DocumentTabClosed { tab_id: u64 },
    /// V4 selection changed for a specific tab. Discriminant 7.
    SelectionChangedV4 { tab_id: u64, handles: Vec<Handle> },
    /// V7 active interactive-command state for a tab. Discriminant 8.
    CommandStateChanged { tab_id: u64, command: Option<String> },
    /// V7 drawing epoch, broadcast even without an open shared view. Discriminant 9.
    DrawingChanged { tab_id: u64, epoch: u64 },
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
                bincode::serialize_into(&mut bytes, line)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::Cancel => bytes.push(1),
            HostNotification::DocumentChanged { version } => {
                bytes.push(2);
                bincode::serialize_into(&mut bytes, version)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::SelectionChanged { handles } => {
                bytes.push(3);
                bincode::serialize_into(&mut bytes, handles)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::Raw(data) => {
                bytes.push(4);
                bincode::serialize_into(&mut bytes, data)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::DocumentChangedV4 { tab_id, version } => {
                bytes.push(5);
                bincode::serialize_into(&mut bytes, tab_id)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, version)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::DocumentTabClosed { tab_id } => {
                bytes.push(6);
                bincode::serialize_into(&mut bytes, tab_id)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::SelectionChangedV4 { tab_id, handles } => {
                bytes.push(7);
                bincode::serialize_into(&mut bytes, tab_id)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, handles)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::CommandStateChanged { tab_id, command } => {
                bytes.push(8);
                bincode::serialize_into(&mut bytes, tab_id)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, command)
                    .map_err(serde::ser::Error::custom)?;
            }
            HostNotification::DrawingChanged { tab_id, epoch } => {
                bytes.push(9);
                bincode::serialize_into(&mut bytes, tab_id)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, epoch)
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
            _ => Ok(HostNotification::Unknown(bytes)),
        }
    }
}

/// A notification a plugin sends to the host. These are best-effort,
/// full-duplex messages correlated with an optional `command_id`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PluginNotification {
    Output { text: String },
    Error { text: String },
    Prompt { text: String },
    Progress { percent: u8 },
    Log { level: LogLevel, text: String },
    Raw(Vec<u8>),
    /// V4 REPL status update. Discriminant 6.
    ReplStatus { status: String, message: String },
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
                bincode::serialize_into(&mut bytes, text)
                    .map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Error { text } => {
                bytes.push(1);
                bincode::serialize_into(&mut bytes, text)
                    .map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Prompt { text } => {
                bytes.push(2);
                bincode::serialize_into(&mut bytes, text)
                    .map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Progress { percent } => {
                bytes.push(3);
                bincode::serialize_into(&mut bytes, percent)
                    .map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Log { level, text } => {
                bytes.push(4);
                bincode::serialize_into(&mut bytes, level)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, text)
                    .map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::Raw(data) => {
                bytes.push(5);
                bincode::serialize_into(&mut bytes, data)
                    .map_err(serde::ser::Error::custom)?;
            }
            PluginNotification::ReplStatus { status, message } => {
                bytes.push(6);
                bincode::serialize_into(&mut bytes, status)
                    .map_err(serde::ser::Error::custom)?;
                bincode::serialize_into(&mut bytes, message)
                    .map_err(serde::ser::Error::custom)?;
            }
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
                let (level, text): (LogLevel, String) = bincode::deserialize(rest)
                    .map_err(serde::de::Error::custom)?;
                Ok(PluginNotification::Log { level, text })
            }
            5 => bincode::deserialize(rest)
                .map(PluginNotification::Raw)
                .map_err(serde::de::Error::custom),
            6 => bincode::deserialize(rest)
                .map(|(status, message)| PluginNotification::ReplStatus { status, message })
                .map_err(serde::de::Error::custom),
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
        Self::Polyline { points, color: None }
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

/// The outcome of an [`InteractiveCommand`] step.
#[derive(Debug)]
#[cfg_attr(feature = "host", derive(serde::Serialize, serde::Deserialize))]
pub enum CommandStep {
    /// Need another point; keep the command active.
    NeedPoint,
    /// Commit an entity to the document and keep collecting points.
    Commit(EntityType),
    /// Commit multiple entities to the document and keep collecting points.
    CommitMany(Vec<EntityType>),
    /// Commit an entity and end the command.
    CommitAndEnd(EntityType),
    /// Commit multiple entities to the document and end the command.
    CommitManyAndEnd(Vec<EntityType>),
    /// End the command without committing.
    Done,
    /// Cancel the command.
    Cancel,
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
    fn document(&self) -> &CadDocument;
    /// Mutable access to the active document.
    ///
    /// For an **out-of-process** plugin this borrows a *local snapshot*: edits
    /// to existing entities made through it are NOT sent back to the host and
    /// are silently discarded. To modify or delete entities from any plugin,
    /// use [`add_entity`](Self::add_entity), [`update_entity`](Self::update_entity)
    /// and [`remove_entity`](Self::remove_entity), which are committed to the
    /// host document over IPC.
    fn document_mut(&mut self) -> &mut CadDocument;

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
    fn ensure_plugin_state_any(
        &mut self,
        plugin_id: &'static str,
        init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
    ) -> &mut (dyn Any + Send + Sync);

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
    fn try_recv_notification(
        &mut self,
    ) -> Option<(Option<u64>, HostNotification)> {
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
    fn selection(&self) -> Vec<Handle> { Vec::new() }

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
}

/// Configuration properties for creating or modifying a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LayerConfig {
    pub name: String,
    pub color: Option<acadrust::types::Color>,
    pub linetype: Option<String>,
    pub lineweight: Option<acadrust::types::LineWeight>,
    pub off: Option<bool>,
    pub frozen: Option<bool>,
    pub locked: Option<bool>,
    pub plottable: Option<bool>,
    pub transparency: Option<acadrust::types::Transparency>,
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

    /// Iterate over all entities without allocating a full `CadDocument`.
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

/// In-process `DocumentReader` implementation that wraps a borrowed `CadDocument`.
pub struct CadDocumentReader<'a>(pub &'a CadDocument);

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
pub fn ensure_plugin_state<'a, T: Any + Send + Sync>(
    host: &'a mut dyn HostApi,
    plugin_id: &'static str,
    init: impl FnOnce() -> T,
) -> &'a mut T {
    let mut init = Some(init);
    let any = host.ensure_plugin_state_any(plugin_id, &mut || {
        Box::new((init.take().expect("init called once"))())
    });
    any.downcast_mut::<T>()
        .expect("plugin state type mismatch for plugin_id")
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
