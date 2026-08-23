// Shared value types used by the dispatch and grip systems.

use acadrust::types::{Color as AcadColor, LineWeight};
use glam::DVec3;

/// The kind of value held by a property row.
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    /// Read-only display text.
    ReadOnly(String),
    /// Editable numeric field.
    EditText(String),
    /// Editable text that must not be expression-evaluated.
    PlainText(String),
    /// Layer name — rendered as a combo_box.
    LayerChoice(String),
    /// Generic string choice rendered as a combo_box.
    Choice {
        selected: String,
        options: Vec<String>,
    },
    /// Editable text plus a dropdown of existing options (block reference
    /// Name row): picking an option re-points the reference, submitting a
    /// new name renames the definition.
    EditChoice {
        value: String,
        options: Vec<String>,
    },
    /// ACI/RGB/ByLayer/ByBlock color — rendered as a color picker.
    ColorChoice(AcadColor),
    /// Color varies across the current multi-selection.
    ColorVaries,
    /// Line weight — rendered as a combo_box.
    LwChoice(LineWeight),
    /// Lineweight varies across the current multi-selection.
    LwVaries,
    /// Linetype name — rendered as a combo_box.
    LinetypeChoice(String),
    /// Boolean flag — rendered as a toggle button (e.g. Invisible).
    BoolToggle { field: &'static str, value: bool },
    /// A 0-based index navigated with ◀ / ▶ buttons (e.g. a polyline's Current
    /// Vertex). `display` is the label shown between the arrows (e.g. "2 / 7").
    Stepper { field: &'static str, display: String },
    /// Hatch pattern name — rendered as a combo_box from the catalog.
    HatchPatternChoice(String),
    /// Block attribute value keyed by its (dynamic, runtime) tag — rendered as
    /// an editable text_input. Unlike the other rows the routing key is the
    /// tag carried here, not the row's `&'static str` field.
    AttrText { tag: String, value: String },
}

/// A single property row in the Properties panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub label: String,
    /// Stable field identifier used in `PropGeomInput` / `PropGeomCommit` messages.
    pub field: &'static str,
    pub value: PropValue,
}

/// A named section of properties (e.g. "General", "Geometry").
#[derive(Clone, Debug, PartialEq)]
pub struct PropSection {
    pub title: String,
    pub props: Vec<Property>,
}

// ── Grip types ─────────────────────────────────────────────────────────────

/// Visual marker shape for a grip point. The complete vocabulary that
/// matches the standard CAD grip conventions:
/// * `Square` — endpoint / vertex / centre. Drag → moves a single
///   point or translates the entity.
/// * `Rectangle` — direction-aware mid-segment stretch handle.
///   Drawn as a small box rotated along `dir` (the in-plane segment
///   direction in world XY). Used for polyline / wipeout / image /
///   dimension segment midpoints.
/// * `Triangle` — directional indicator (Phase 2: dynamic-block
///   parameters, dimension reverse-arrow flips, multi-functional
///   hover popups).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum GripShape {
    Square,
    Rectangle,
    Triangle,
    Circle,
    /// Screen-offset menu selector.
    Dropdown,
}

/// Describes one grip point for an entity.
#[derive(Clone, Debug)]
pub struct GripDef {
    /// Object-local identifier (stable index, unique per object instance).
    pub id: usize,
    /// World-space position of the grip, in f64. Entity coordinates can sit at
    /// UTM magnitudes (1e7); casting to f32 before the world-offset subtraction
    /// loses ~1 drawing unit and draws the grip visibly off the wire. Producers
    /// fill this straight from the f64 entity data; the offset is subtracted in
    /// f64 and only then cast for screen-space math.
    pub world: glam::DVec3,
    /// `true` → midpoint / centre grip (drags the whole shape).
    /// `false` → endpoint / vertex grip (moves a single point).
    pub is_midpoint: bool,
    /// Visual marker shape for the grip.
    pub shape: GripShape,
    /// World-space marker direction, projected with the grip position.
    pub dir: Option<glam::DVec3>,
    /// World-space axis that constrains this grip's drag.
    pub axis: Option<glam::DVec3>,
}

/// How to apply a grip drag result.
#[derive(Clone, Debug)]
pub enum GripApply {
    /// Move a specific vertex to this absolute world position.
    Absolute(DVec3),
    /// Translate the whole object by this delta vector.
    Translate(DVec3),
}

/// One entry in the hover-popup menu that opens when the cursor dwells
/// on a grip. The `label` is the user-visible string; `action` is the
/// operation the entity will perform when the item is committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GripMenuItem {
    pub label: &'static str,
    pub action: GripMenuAction,
}

/// All operations a grip popup menu can dispatch. Entity-specific code
/// in `apply_grip_menu` decodes these into edits. `Stretch` is the
/// default no-op-equivalent — picking it just starts the regular
/// stretch drag, identical to clicking the grip with no popup open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum GripMenuAction {
    Stretch,
    Lengthen,
    Radius,
    ArcLength,
    AddVertex,
    RemoveVertex,
    ConvertToArc,
    /// Split the polyline at this vertex (a closed one opens here; an open
    /// one splits into two). Handled by the driver — it replaces the entity.
    BreakVertex,
    ConvertToLine,
    StretchVertex,
    AddLeader,
    RemoveLeader,
    ReverseArrows,
    MoveWithDimLine,
    MoveWithLeader,
    MoveIndependent,
    ResetText,
    RotateText,
    AboveDimLine,
    Center,
    OriginPoint,
    HatchAngle,
    HatchScale,
    HatchPattern,
    TangentDirection,
    AddFitPoint,
    RemoveFitPoint,
    Refit,
    RefineVertices,
    ShowFit,
    ShowControlVertices,
    MoveWithText,
    StackText,
    UnstackText,
}
