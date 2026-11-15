//! Declarative scene specification and patch types for Open CAD Studio plugin
//! panels (additive, pre-pin).
//!
//! The scene model is a *semantic* tree, not a drawing: the plugin owns the
//! semantic model and submits either whole snapshots ([`PanelScene`]) or
//! incremental updates ([`PanelScenePatch`]); the host owns the projection —
//! rendering, caches, geometry flattening, and hit-testing. Plugin and host
//! swap one serializable tree over the wire and nothing else.
//!
//! Wire rule: new variants MUST be appended at the end of the enums so
//! existing plugins keep their bincode discriminant indices. The wire grows
//! append-only the same way dialogs and panels do.
//!
//! All coordinates are plain data: [`Vec2`] pairs in scene space, no
//! anchor/affinity semantics attached at this tier. Floating-point payloads
//! that arrive over the wire are bounded (see [`MAX_SCENE_DEPTH`]); every
//! coordinate the scene carries must be finite, and [`PanelScene::validate`]
//! is the structural gate for that plus budgets, ids, and shapes.
//!
//! These types are pre-pin per ADR-001: they are not yet frozen, and the
//! discriminant indices pinned by the tests in this module freeze *order*
//! within this task only, until the host projection task lands.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Maximum nesting depth accepted when decoding scene/patch data. Real
/// panels nest groups a handful of levels deep; a hostile frame that nests
/// `Group` children tens of thousands deep previously could overflow the
/// decoding thread's stack (which aborts the host/runner process). The cap
/// mirrors `MAX_DIALOG_NESTING` and sits well above every legitimate layout
/// while staying far below any stack limit.
///
/// Budget errors report the FIRST CROSSING point, not a total: when a scene
/// trips this cap, [`SceneError::DepthBudgetExceeded`] names the depth of the
/// first element that sits past it (cap + 1 or deeper), matching the
/// first-crossing semantics of the structural gate.
pub const MAX_SCENE_DEPTH: usize = 64;

/// Maximum total element nodes accepted per top-level decoded scene frame,
/// bounding allocation and CPU magnification from shared sub-blocks. Mirrors
/// `MAX_DIALOG_NODES`: the cap is well above any authored panel and the same
/// magnitude the dialog decoder tolerates per frame.
///
/// The host's scene projection privately mirrors this value (see
/// `src/ui/window/plugin_scene.rs` `MAX_SCENE_NODES`) until a public
/// import path exists; the two must change together.
///
/// Budget errors report the FIRST CROSSING point, not a total: when a scene
/// trips this cap, [`SceneError::NodeBudgetExceeded`] names the node count at
/// the crossing (cap + 1, never the true total the payload would carry),
/// matching the first-crossing semantics of the structural gate.
pub const MAX_SCENE_NODES: usize = 10_000;

/// Maximum op count accepted in one [`PanelScenePatch`]. A patch is replayed
/// whole against the base revision; bounding its op count bounds the largest
/// single mutation a plugin can ask the host to attempt per frame.
///
/// ⚠ Load-bearing coincidence: [`MAX_PATCH_OPS`] equals [`MAX_SCENE_NODES`]
/// by today's choice, and [`budgeted_ops`] charges ops against the SAME
/// per-frame counter as scene nodes, so one decoded patch frame can never
/// carry ops that outgrow the node budget independently of elements. The
/// equality is also asserted by a test. Revisit both endpoints together if
/// [`MAX_SCENE_NODES`] ever changes; do not move one alone.
pub const MAX_PATCH_OPS: usize = 10_000;

/// Maximum serialized byte length of one [`SceneElement::Text`] `content`
/// run. Text in a panel is a label, not a document; the cap keeps a single
/// padded field from dominating frame space the way an unbounded string
/// would, while remaining far above any rendered legend. Shared with the
/// widget vocabulary ([`crate::widgets`]), which budgets every widget text
/// field against the same cap.
pub const MAX_TEXT_BYTES: usize = 4_096;

/// Maximum serialized byte length of one [`SceneElement::Asset`] `uri`.
/// Asset URIs are opaque references the host loads and caches; the cap keeps
/// a padded reference from smuggling payload bytes into the scene frame.
/// Shared with the widget vocabulary ([`crate::widgets::IconRef`]), which
/// budgets its icon references against the same cap.
pub const MAX_ASSET_URI_BYTES: usize = 512;

/// Plugin-assigned, stable identity of one scene element.
///
/// Unique within the owning panel's scene; the plugin chooses the values, the
/// host never reassigns them. [`SceneElement::Edge`] endpoints and
/// [`SceneOp`] targets reference elements through this id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StableId(pub u64);

/// Monotonically increasing revision of one panel scene.
///
/// Revisions exist so the host can reject patches built against a stale
/// snapshot (`SceneError::StaleBase`) and reject any attempt to rewind the
/// scene (`SceneError::NonMonotonicRevision`). Revisions increase
/// monotonically per panel; the plugin names each new revision it publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SceneRevision(pub u64);

/// RGBA color, one byte per channel, fully opaque at alpha `255`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba8(pub [u8; 4]);

/// A 2D point or offset in scene space (`[x, y]`).
///
/// A fixed array rather than a wrapped vector, matching the crate convention
/// (see `ParametricSegment::start_point`), so the layout is pinned at exactly
/// two finite `f64` components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec2(pub [f64; 2]);

/// An axis-aligned rectangle anchored at [`Rect2D::pos`] with extent
/// [`Rect2D::size`], used for [`SceneElement::Rect`], asset framing, and
/// group clipping.
///
/// Validation requires `size` to be finite with both components strictly
/// positive; a zero, negative, or non-finite size fails with
/// [`SceneError::InvalidRect`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect2D {
    /// Rectangle anchor (top-left in host projection terms; the scene tier
    /// states no axis convention beyond "scene space").
    pub pos: Vec2,
    /// Extent; both components must be finite and greater than zero.
    pub size: Vec2,
}

/// Theme-relative color tokens. Mirrors the crate's `TextStyle` emphasis
/// vocabulary (the same seven semantic slots panels already name); the host
/// resolves each token against its own theme. This type does not claim any
/// specific mapping — host mapping is a later concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeColor {
    /// Regular full-contrast fill.
    #[default]
    Normal,
    /// Panel/brand accent.
    Accent,
    /// De-emphasised secondary fill.
    Muted,
    /// Emphasised fill (the look of bold text in dialogs).
    Strong,
    /// Positive state ("check passed").
    Success,
    /// State that needs attention.
    Warning,
    /// Failed state.
    Danger,
}

/// Fill or stroke color: a named theme token or an exact RGBA value.
///
/// The theme form is preferred; `Rgba` is for exact-canvas needs (brand
/// marks, swatches). The default is [`SceneColor::Theme`] with
/// [`ThemeColor::Normal`] everywhere a color is defaulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SceneColor {
    /// Resolve through the host theme.
    Theme(ThemeColor),
    /// Exact RGBA bytes ([`Rgba8::0`]), fully opaque.
    Rgba(Rgba8),
}

impl Default for SceneColor {
    /// The default color is the `Normal` theme token everywhere a color is
    /// defaulted.
    fn default() -> Self {
        Self::Theme(ThemeColor::default())
    }
}

/// Outline stroke for paths, rectangles, and edges. Width is in scene units
/// and must be finite and strictly positive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Stroke color (theme token or exact RGBA).
    pub color: SceneColor,
    /// Stroke width in scene units; must be finite and greater than zero.
    pub width: f32,
}

/// Plugin-opaque context attached to an element for drag interactions.
///
/// The host hands [`DragMeta`] back verbatim when a drag settles on the
/// element it came with. The plugin defines what `opaque` means; the host
/// never interprets, indexes, or fabricates its content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DragMeta {
    /// Plugin-defined marker string (which sub-part the gesture started on,
    /// what a drop means, …), returned unchanged at settle.
    #[serde(default)]
    pub opaque: Option<String>,
}

/// Asset reference categories available to [`SceneElement::Asset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    /// An SVG document URL; the host rasterizes at the frame's extent.
    Svg,
    /// A raster image URL; the host scales it to the frame's extent.
    Image,
}

/// Endpoint decoration for [`SceneElement::Edge`] (`start_marker`/`end_marker`)
/// and the default shape of [`SceneElement::Marker`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkerKind {
    /// Plain filled disc. The default kind.
    #[default]
    Dot,
    /// Open-then-pointing triangle, oriented along the route direction.
    Arrow,
    /// Rotated square, oriented along the route direction.
    Diamond,
}

/// One cubic Bézier control quad within [`SceneElement::CubicPath`].
///
/// Flattening (to a polyline the renderer can consume) is HOST-side work:
/// the plugin states the curve, the host samples it. No flattening
/// tolerance is wired here.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CubicSegment {
    /// Segment start anchor.
    pub start: Vec2,
    /// First control point (start-side tangent).
    pub ctrl_a: Vec2,
    /// Second control point (end-side tangent).
    pub ctrl_b: Vec2,
    /// Segment end anchor (the next segment's `start` ideally continues it).
    pub end: Vec2,
}

/// One element of a panel scene (API pre-pin).
///
/// Compatibility rule: new variants MUST be appended at the end so existing
/// plugins keep their bincode discriminant indices (`0..=8`; pinned by the
/// tests in this module). Common field layout: `id` first, then the optional
/// `name`/`actions`/`drag` accessories, then the variant's specifics.
///
/// Deserialization is bounded (see [`MAX_SCENE_DEPTH`]): the wire layout is
/// unchanged, but a payload whose element tree passes the decoder budget is
/// rejected instead of overflowing the stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneElement {
    /// Container for nested elements. Child coordinates stay scene-space:
    /// a group does not establish a transform.
    Group {
        /// Stable element id.
        id: StableId,
        /// Optional display name (host presentation only; also a hit-test
        /// hint).
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs the host should surface (e.g. "click",
        /// "select"); verbs mirror the crate dialog action vocabulary.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context, reported back on drag settle.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Child elements, innermost-last. Decoded under the scene budget.
        #[serde(deserialize_with = "budgeted_elements")]
        children: Vec<SceneElement>,
        /// Optional clip rectangle; children render only inside it.
        #[serde(default)]
        clip: Option<Rect2D>,
    },
    /// Filled and/or stroked rectangle.
    Rect {
        /// Stable element id.
        id: StableId,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Rectangle anchor.
        pos: Vec2,
        /// Rectangle extent (finite, both components greater than zero).
        size: Vec2,
        /// Fill color; `None` is transparent.
        #[serde(default)]
        fill: Option<SceneColor>,
        /// Outline stroke.
        #[serde(default)]
        stroke: Option<Stroke>,
    },
    /// Open or closed polyline through the given points.
    Path {
        /// Stable element id.
        id: StableId,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Polyline vertices (at least two); repeated consecutive points are
        /// a host-projection concern, not a wire error.
        points: Vec<Vec2>,
        /// `true` closes the loop back to the first point before stroke/fill.
        closed: bool,
        /// Outline stroke.
        #[serde(default)]
        stroke: Option<Stroke>,
        /// Fill color for the enclosed area; `None` is transparent.
        #[serde(default)]
        fill: Option<SceneColor>,
    },
    /// Open or closed Bézier chain; [`CubicSegment`] quads state the curve,
    /// the host flattens it.
    CubicPath {
        /// Stable element id.
        id: StableId,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Bézier quads (at least one).
        segments: Vec<CubicSegment>,
        /// `true` closes the loop back to the first `start` before
        /// stroke/fill.
        closed: bool,
        /// Outline stroke.
        #[serde(default)]
        stroke: Option<Stroke>,
        /// Fill color for the enclosed area; `None` is transparent.
        #[serde(default)]
        fill: Option<SceneColor>,
    },
    /// Single text run at a scene position (no per-line layout; the host
    /// picks line breaking if the projection needs it).
    Text {
        /// Stable element id.
        id: StableId,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Run content, at most [`MAX_TEXT_BYTES`] bytes.
        content: String,
        /// Baseline anchor of the run.
        pos: Vec2,
        /// Font size in scene units (default [`default_text_size`], `13.0`).
        #[serde(default = "default_text_size")]
        size: f32,
        /// Text color.
        color: SceneColor,
    },
    /// Host-loaded and host-cached asset drawn into a frame.
    ///
    /// The plugin never sends bytes over the scene channel: `uri` is an
    /// opaque reference the host resolves against whatever its per-panel
    /// asset loader accepts (the scene tier puts no scheme restrictions on
    /// it; at most [`MAX_ASSET_URI_BYTES`] bytes).
    Asset {
        /// Stable element id.
        id: StableId,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Asset reference kind.
        kind: AssetKind,
        /// Opaque asset reference (loaded and cached by the host).
        uri: String,
        /// Frame the asset renders into.
        frame: Rect2D,
    },
    /// Named connection point pinned at a scene position; its id is what
    /// [`SceneElement::Edge`] endpoints reference in practice.
    Port {
        /// Stable element id.
        id: StableId,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Marker position.
        at: Vec2,
        /// Optional short label drawn next to the port.
        #[serde(default)]
        label: Option<String>,
    },
    /// Standalone annotation shape pinned at a scene position.
    Marker {
        /// Stable element id.
        id: StableId,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Marker position.
        at: Vec2,
        /// Marker shape (default [`MarkerKind::Dot`]).
        #[serde(default)]
        kind: MarkerKind,
        /// Marker color.
        color: SceneColor,
    },
    /// Route between two stable ids (node ids of [`SceneElement::Port`] and
    /// [`SceneElement::Group`] elements in practice). Endpoint existence is
    /// NOT validated — edges may point to ids defined anywhere in the tree,
    /// with no forward-reference check beyond uniqueness.
    Edge {
        /// Stable element id.
        id: StableId,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
        /// Optional action verbs.
        #[serde(default)]
        actions: Vec<String>,
        /// Optional drag context.
        #[serde(default)]
        drag: Option<DragMeta>,
        /// Route start (node stable id).
        a: StableId,
        /// Route end (node stable id).
        b: StableId,
        /// Route way points (at least one).
        route: Vec<Vec2>,
        /// Route stroke.
        #[serde(default)]
        stroke: Option<Stroke>,
        /// Decoration at the `a` end.
        #[serde(default)]
        start_marker: Option<MarkerKind>,
        /// Decoration at the `b` end.
        #[serde(default)]
        end_marker: Option<MarkerKind>,
    },
}

impl SceneElement {
    /// The element's stable id. Every variant carries exactly one; edges and
    /// patches reference elements through it.
    pub const fn stable_id(&self) -> StableId {
        match self {
            Self::Group { id, .. }
            | Self::Rect { id, .. }
            | Self::Path { id, .. }
            | Self::CubicPath { id, .. }
            | Self::Text { id, .. }
            | Self::Asset { id, .. }
            | Self::Port { id, .. }
            | Self::Marker { id, .. }
            | Self::Edge { id, .. } => *id,
        }
    }
}

/// Default font size for [`SceneElement::Text`] (`13.0`), used by serde when
/// the field is absent on the wire.
fn default_text_size() -> f32 {
    13.0
}

/// Everything structural validation rejects about a scene, patch, or op.
///
/// A structural error leaves the scene completely unchanged: [`apply_patch`]
/// validates the whole op sequence through an internal planning pass and only
/// mutates the tree after the plan completes (the crate contract that an
/// error leaves the drawing and undo history unchanged).
#[derive(Debug, Clone, PartialEq)]
pub enum SceneError {
    /// The patch was built against an outdated snapshot.
    StaleBase {
        /// Revision currently applied to the scene.
        current: SceneRevision,
        /// Revision the patch declares as its base.
        given: SceneRevision,
    },
    /// The patch would rewind the scene or not advance it.
    NonMonotonicRevision {
        /// Revision currently applied to the scene.
        current: SceneRevision,
        /// Revision the patch proposes as the successor.
        given: SceneRevision,
    },
    /// An element id appears twice in the tree (or an op reuses a live id).
    DuplicateId(StableId),
    /// The tree nests past the [`MAX_SCENE_DEPTH`] cap. First-crossing
    /// semantics: `depth` is the depth of the first element found past the
    /// cap (cap + 1 or deeper), never the deepest nesting of the whole tree.
    DepthBudgetExceeded {
        /// Depth observed.
        depth: usize,
        /// Cap that was exceeded.
        max: usize,
    },
    /// The tree carries more element nodes than the [`MAX_SCENE_NODES`] cap.
    /// First-crossing semantics: `nodes` is the count at the first crossing
    /// (cap + 1), never the true total of the candidate tree.
    NodeBudgetExceeded {
        /// Node count observed.
        nodes: usize,
        /// Cap that was exceeded.
        max: usize,
    },
    /// The patch carries more ops than the [`MAX_PATCH_OPS`] cap.
    PatchOpBudgetExceeded {
        /// Op count observed.
        ops: usize,
        /// Cap that was exceeded.
        max: usize,
    },
    /// A text run's byte count exceeds the [`MAX_TEXT_BYTES`] cap.
    TextBudgetExceeded {
        /// Byte count observed.
        bytes: usize,
        /// Cap that was exceeded.
        max: usize,
    },
    /// An asset uri's byte count exceeds the [`MAX_ASSET_URI_BYTES`] cap.
    AssetUriBudgetExceeded {
        /// Byte count observed.
        bytes: usize,
        /// Cap that was exceeded.
        max: usize,
    },
    /// A coordinate, size, or stroke width float is NaN or infinite.
    NonFiniteCoordinate,
    /// A rectangle's size is zero, negative, or non-finite.
    InvalidRect,
    /// The referenced parent id does not anchor a group (Add target).
    ParentMissing(StableId),
    /// The referenced element id does not exist (Replace/Remove target).
    ElementMissing(StableId),
    /// A path/cubic/route carries fewer points than its shape requires.
    TooFewPoints,
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::StaleBase { current, given } => write!(
                f,
                "stale patch base: the scene is at revision {} but the patch was built against {}",
                current.0, given.0
            ),
            SceneError::NonMonotonicRevision { current, given } => write!(
                f,
                "non-monotonic revision: the scene is at revision {} and the patch proposes {}",
                current.0, given.0
            ),
            SceneError::DuplicateId(id) => {
                write!(f, "duplicate scene element id {}", id.0)
            }
            SceneError::DepthBudgetExceeded { depth, max } => write!(
                f,
                "scene depth budget exceeded: nesting depth {depth} passes the cap {max}"
            ),
            SceneError::NodeBudgetExceeded { nodes, max } => write!(
                f,
                "scene node budget exceeded: {nodes} nodes pass the cap {max}"
            ),
            SceneError::PatchOpBudgetExceeded { ops, max } => {
                write!(f, "patch op budget exceeded: {ops} ops pass the cap {max}")
            }
            SceneError::TextBudgetExceeded { bytes, max } => write!(
                f,
                "scene text budget exceeded: {bytes} bytes pass the cap {max}"
            ),
            SceneError::AssetUriBudgetExceeded { bytes, max } => write!(
                f,
                "scene asset uri budget exceeded: {bytes} bytes pass the cap {max}"
            ),
            SceneError::NonFiniteCoordinate => write!(
                f,
                "scene coordinate is not finite (NaN or float meta values are rejected)"
            ),
            SceneError::InvalidRect => write!(
                f,
                "scene rectangle size must be finite with both components greater than zero"
            ),
            SceneError::ParentMissing(id) => write!(
                f,
                "scene element {} does not anchor a group (Add parent id)",
                id.0
            ),
            SceneError::ElementMissing(id) => {
                write!(f, "scene element {} does not exist", id.0)
            }
            SceneError::TooFewPoints => {
                write!(f, "scene path carries fewer points than its shape requires")
            }
        }
    }
}

impl std::error::Error for SceneError {}

/// Per-thread scene decode budget. `depth` is the current recursion depth
/// (0 outside a decode); `nodes` counts total scene elements/ops decoded
/// within the current frame (re-zeroed by [`scene_decode_frame_begin`] at
/// each frame boundary).
#[derive(Clone, Copy)]
struct SceneDecodeBudget {
    depth: usize,
    nodes: usize,
}

thread_local! {
    static SCENE_DECODE_BUDGET: std::cell::Cell<SceneDecodeBudget> =
        const { std::cell::Cell::new(SceneDecodeBudget { depth: 0, nodes: 0 }) };
}

/// Reset the per-thread decode budget at a frame boundary: call this right
/// before decoding one whole wire frame so the node budget applies per frame
/// rather than accumulating across frames.
///
/// This mirrors `dialog_decode_frame_begin` and stays behind the same gate:
/// the production callers (the wired scene transports) are a later task, so
/// the feature-less data-contract build only materializes the helper for
/// tests.
#[cfg(any(feature = "host", test))]
#[allow(dead_code)] // wired into the scene transport by the follow-up task
pub(crate) fn scene_decode_frame_begin() {
    SCENE_DECODE_BUDGET.with(|budget| {
        budget.set(SceneDecodeBudget { depth: 0, nodes: 0 });
    });
}

/// Enter one scene node during a decode. Returns `Err` when the payload
/// exceeds the nesting or node budget; the error aborts the decode before the
/// next level of recursion can consume stack.
fn scene_decode_enter() -> Result<(), String> {
    SCENE_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.depth += 1;
        budget.nodes += 1;
        SCENE_DECODE_BUDGET.set(budget);
        if budget.depth > MAX_SCENE_DEPTH {
            return Err(format!(
                "scene nesting exceeds the decoded depth budget ({MAX_SCENE_DEPTH})"
            ));
        }
        if budget.nodes > MAX_SCENE_NODES {
            return Err(format!(
                "scene frame exceeds the decoded node budget ({MAX_SCENE_NODES})"
            ));
        }
        Ok(())
    })
}

/// Leave one scene node (restores the depth budget even on decode failure).
fn scene_decode_exit() {
    SCENE_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.depth = budget.depth.saturating_sub(1);
        SCENE_DECODE_BUDGET.set(budget);
    });
}

struct SceneDecodeGuard;

impl Drop for SceneDecodeGuard {
    fn drop(&mut self) {
        scene_decode_exit();
    }
}

/// Charge `count` decoded element nodes against the frame node budget.
fn scene_decode_add_nodes(count: usize) -> Result<(), String> {
    SCENE_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.nodes = budget.nodes.saturating_add(count);
        SCENE_DECODE_BUDGET.set(budget);
        if budget.nodes > MAX_SCENE_NODES {
            Err(format!(
                "scene frame exceeds the decoded node budget ({MAX_SCENE_NODES})"
            ))
        } else {
            Ok(())
        }
    })
}

/// Bounded deserialize helper for the recursive `Vec<SceneElement>` carriers
/// (`PanelScene::elements` and `Group::children`). Charges one nesting level
/// and the decoded element total against the frame budget: a payload past the
/// budget refuses before the next level recurses, so the decoding stack stays
/// bounded. The wire layout is unchanged.
fn budgeted_elements<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SceneElement>, D::Error> {
    scene_decode_enter().map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = SceneDecodeGuard;
    let elements = Vec::<SceneElement>::deserialize(deserializer)?;
    scene_decode_add_nodes(elements.len()).map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(elements)
}

/// Bounded deserialize helper for the flat `PanelScenePatch::ops` carrier.
/// Charges the decoded op total against the frame budget; nested elements
/// inside ops recurse into [`budgeted_elements`] at their own levels.
fn budgeted_ops<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SceneOp>, D::Error> {
    scene_decode_enter().map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = SceneDecodeGuard;
    let ops = Vec::<SceneOp>::deserialize(deserializer)?;
    scene_decode_add_nodes(ops.len()).map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(ops)
}

/// A complete snapshot of one panel's scene.
///
/// The plugin owns the semantic model (what the scene means and holds); the
/// host owns the projection into rendered geometry and caches. A snapshot is
/// the ordered root element list — the scene carries no document reference
/// of its own.
///
/// Deserialization is bounded (see [`MAX_SCENE_DEPTH`]) exactly like the
/// dialog carriers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelScene {
    /// The scene's revision; raises monotonically per panel.
    pub revision: SceneRevision,
    /// Root elements in paint order (first painted first). Decoded under the
    /// scene budget.
    #[serde(deserialize_with = "budgeted_elements")]
    pub elements: Vec<SceneElement>,
}

impl PanelScene {
    /// Structural validation: unique ids across the whole tree, nesting
    /// within [`MAX_SCENE_DEPTH`], total element count within
    /// [`MAX_SCENE_NODES`], all floating-point payload finite, text/asset
    /// byte caps, rectangle shapes, and minimum point counts for paths,
    /// bézier chains, and routes.
    ///
    /// This is deliberately structural only. Edge endpoint existence, hit
    /// zones, and semantic sense are NOT checked: edges may point to ids
    /// defined anywhere in the tree.
    pub fn validate(&self) -> Result<(), SceneError> {
        validate_root(&self.elements)
    }

    /// Apply an incremental patch atomically (two phases, in place).
    ///
    /// The patch must name the current scene revision as its base and a
    /// strictly greater successor; a rejection leaves `self` exactly as it
    /// was (no element moves, the revision stands), mirroring the crate
    /// contract that an error leaves the drawing and undo history unchanged.
    ///
    /// Phase 0 — three cheap checks with nothing written: base freshness,
    /// monotonic successor, and the [`MAX_PATCH_OPS`] op budget.
    ///
    /// Phase 1 — planning pass, still without any mutation: the whole op
    /// sequence is replayed against [`PlannedIndex`], a lightweight id/position
    /// mirror of the current tree (built by ONE walk), and every op is
    /// expanded into a pre-located journal step ([`PlannedOp`]). Each op is
    /// fully validated against the current post-prior-ops state — payload
    /// structure, id collisions, parent existence and group anchoring, and
    /// the resulting depth/node budgets — before its journal step is
    /// recorded; any failure aborts the whole patch.
    ///
    /// Phase 2 — the journal is spliced into the real children vecs strictly
    /// in order (append / swap / `Vec::remove`), then the successor revision
    /// is adopted. Journal locations were derived from the mirror precisely
    /// as the mirror accounted for the same mutations, so after a successful
    /// phase 1 every phase-2 step is infallible by construction; the whole
    /// patch commits in one step with no partial writes possible.
    ///
    /// Cost contract (retiring the ADR-002 staged-clone debt): NO full-tree
    /// clone and no per-op full-tree walk. One O(scene) walk builds the
    /// mirror; each op then does work local to what it touches — O(depth) for
    /// location/parent-chain derivation, plus the payload/adoption subtree
    /// size, plus (for removal) the spliced container's child list, whose
    /// sibling entries shift. Total cost is scene-linear once plus the touched
    /// regions, not scene-linear per op; the patch-scaling evidence is the
    /// SCN-012 benchmark. The earlier staged pass' whole-result `validate()`
    /// no longer runs here: phase 1 proves the same properties on the planned
    /// state, which assumes the base tree is itself structurally valid (the
    /// builder/`validate` gate's job). An invalid base therefore no longer
    /// fails a patch that does not rewrite the offending region — revalidate
    /// or resnapshot in that situation.
    ///
    /// An empty op list is a legitimate no-op that advances the revision.
    pub fn apply_patch(&mut self, patch: PanelScenePatch) -> Result<(), SceneError> {
        // Phase 0: cheap whole-patch checks; nothing written yet.
        if patch.base_revision != self.revision {
            return Err(SceneError::StaleBase {
                current: self.revision,
                given: patch.base_revision,
            });
        }
        if patch.revision <= self.revision {
            return Err(SceneError::NonMonotonicRevision {
                current: self.revision,
                given: patch.revision,
            });
        }
        if patch.ops.len() > MAX_PATCH_OPS {
            return Err(SceneError::PatchOpBudgetExceeded {
                ops: patch.ops.len(),
                max: MAX_PATCH_OPS,
            });
        }

        // Phase 1: validate the whole op sequence and journal pre-located
        // steps; without writing `self`.
        let journal = plan_patch(&self.elements, patch.ops)?;

        // Phase 2: single commit point — replay the plan in place.
        apply_journal(&mut self.elements, journal);
        self.revision = patch.revision;
        Ok(())
    }

    /// Bounded recovery read ([`SceneSummary`]): revision, node total,
    /// per-kind counts, and the capped root-id list. Pure read — one walk,
    /// no validation, never fails.
    pub fn scene_summary(&self) -> SceneSummary {
        SceneSummary::of(self)
    }
}

// ---------------------------------------------------------------------------
// Bounded recovery read (SceneSummary)
// ---------------------------------------------------------------------------

/// Root-id cap of [`SceneSummary::root_ids`]: the summary keeps the first
/// [`MAX_SUMMARY_ROOT_IDS`] root ids in paint order and reports what was
/// dropped in [`SceneSummary::root_ids_truncated`].
pub const MAX_SUMMARY_ROOT_IDS: usize = 64;

/// Element-kind counts carried by [`SceneSummary`] — one fixed field per
/// [`SceneElement`] variant (9 today). The summary's counting match below is
/// exhaustive, so appending a new element variant later is a compile error
/// that guides the summary (and every other by-kind consumer) to update,
/// mirroring how the host projection treats `scene_color`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneKindCounts {
    /// [`SceneElement::Group`] nodes.
    pub groups: usize,
    /// [`SceneElement::Rect`] nodes.
    pub rects: usize,
    /// [`SceneElement::Path`] nodes.
    pub paths: usize,
    /// [`SceneElement::CubicPath`] nodes.
    pub cubic_paths: usize,
    /// [`SceneElement::Text`] nodes.
    pub texts: usize,
    /// [`SceneElement::Asset`] nodes.
    pub assets: usize,
    /// [`SceneElement::Port`] nodes.
    pub ports: usize,
    /// [`SceneElement::Marker`] nodes.
    pub markers: usize,
    /// [`SceneElement::Edge`] nodes.
    pub edges: usize,
}

impl SceneKindCounts {
    /// Sum over all kinds; always equals [`SceneSummary::nodes`].
    pub fn total(&self) -> usize {
        self.groups
            + self.rects
            + self.paths
            + self.cubic_paths
            + self.texts
            + self.assets
            + self.ports
            + self.markers
            + self.edges
    }
}

/// Bounded snapshot read of a panel scene — the stale-base recovery surface
/// of `ARCHITECTURE.md` §4 (invariant PATCH-3): a host that rejects a patch
/// for a stale base reports this summary so the plugin can rebuild against
/// the authoritative state with a bounded read instead of the full snapshot.
///
/// `root_ids` is capped to [`MAX_SUMMARY_ROOT_IDS`] entries (first root ids
/// in paint order); the exact truncation contract is
/// `root_ids.len() + root_ids_truncated == total root count` and
/// `overflow_roots` is the convenience flag for
/// `root_ids_truncated > 0`. The summary is computed by one walk of the
/// current tree ([`PanelScene::scene_summary`]); it never fails, validates
/// nothing, and its only data allocation is the capped root-id vec (plus a
/// transient walker stack).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneSummary {
    /// The scene's authoritative revision (the base a recovery patch must
    /// name).
    pub revision: SceneRevision,
    /// Total element nodes in the tree (the same node count `validate`
    /// checks against [`MAX_SCENE_NODES`]).
    pub nodes: usize,
    /// One count per element kind (sums to [`SceneSummary::nodes`]).
    pub by_kind: SceneKindCounts,
    /// First [`MAX_SUMMARY_ROOT_IDS`] root ids in paint order; fewer when
    /// the scene roots fewer elements than the cap.
    pub root_ids: Vec<StableId>,
    /// Root ids dropped beyond the [`MAX_SUMMARY_ROOT_IDS`] cap; `0` when
    /// nothing was truncated.
    pub root_ids_truncated: usize,
    /// `true` iff the root list was capped (mirror of
    /// [`SceneSummary::root_ids_truncated`] being non-zero).
    pub overflow_roots: bool,
}

impl SceneSummary {
    /// One-walk computation of the whole summary. Counting walks every node
    /// (iterative; document-order); the root-id list copies at most the cap
    /// straight from the root vector.
    fn of(scene: &PanelScene) -> SceneSummary {
        let mut summary = SceneSummary {
            revision: scene.revision,
            nodes: 0,
            by_kind: SceneKindCounts::default(),
            root_ids: Vec::with_capacity(scene.elements.len().min(MAX_SUMMARY_ROOT_IDS)),
            root_ids_truncated: 0,
            overflow_roots: false,
        };
        for element in scene.elements.iter().take(MAX_SUMMARY_ROOT_IDS) {
            summary.root_ids.push(element.stable_id());
        }
        summary.root_ids_truncated = scene.elements.len().saturating_sub(MAX_SUMMARY_ROOT_IDS);
        summary.overflow_roots = summary.root_ids_truncated > 0;

        // One walk for the node total and the per-kind counts: an
        // explicit stack popped in document order, no recursion (the
        // crate's iterative-walk discipline).
        let mut walker: Vec<&SceneElement> = scene.elements.iter().collect();
        while let Some(element) = walker.pop() {
            summary.nodes += 1;
            match element {
                SceneElement::Group { children, .. } => {
                    summary.by_kind.groups += 1;
                    walker.extend(children.iter().rev());
                }
                SceneElement::Rect { .. } => summary.by_kind.rects += 1,
                SceneElement::Path { .. } => summary.by_kind.paths += 1,
                SceneElement::CubicPath { .. } => summary.by_kind.cubic_paths += 1,
                SceneElement::Text { .. } => summary.by_kind.texts += 1,
                SceneElement::Asset { .. } => summary.by_kind.assets += 1,
                SceneElement::Port { .. } => summary.by_kind.ports += 1,
                SceneElement::Marker { .. } => summary.by_kind.markers += 1,
                SceneElement::Edge { .. } => summary.by_kind.edges += 1,
            }
        }
        // Kind counts and the node total agree by construction; a
        // debug_assert keeps the walker honest without release-path work.
        debug_assert_eq!(summary.by_kind.total(), summary.nodes);
        summary
    }
}

// ---------------------------------------------------------------------------
// Two-phase in-place patch planning (SCN-010)
// ---------------------------------------------------------------------------

/// Placement of one element in the PLANNED (post-journal) tree, exactly as
/// phase 2 will splice it: parent chain, position inside the parent's
/// ordered child list, planned depth (root = 1), and whether the element
/// anchors a group. The planner keeps every field in lockstep with the
/// journal it records (sibling entries shift back on remove, appends extend
/// positions), so locations derived from the mirror equal what phase 2 finds.
#[derive(Debug, Clone, Copy)]
struct PlannedSlot {
    /// Parent group id (`None` in the root element list).
    parent: Option<StableId>,
    /// Position within the parent's ordered child list (root list order when
    /// [`PlannedSlot::parent`] is `None`).
    pos: usize,
    /// Planned depth of the element; root elements sit at depth 1.
    depth: usize,
    /// Whether the element anchors a group in the planned state.
    is_group: bool,
}

/// One planned mutation — the journal phase 1 produces and phase 2 replays.
///
/// Locations are array paths from the root element list whose last component
/// is where the node itself lives ([`PlannedOp::Insert`] names its host group
/// instead). Every path was derived from [`PlannedIndex`] after the mirror
/// accounted for every journal entry ahead of it, and phase 2 replays the
/// journal in exactly that order — so at its turn the real tree is precisely
/// the mirror state the path was derived from. That lockstep is what makes
/// phase 2 infallible once planning has succeeded.
#[derive(Debug)]
enum PlannedOp {
    /// Push an element into the child list of the group at `group_path`
    /// (empty path = the root element list).
    Insert {
        /// Path to the host group (empty = root list).
        group_path: Vec<usize>,
        /// Element to append.
        element: SceneElement,
    },
    /// Swap the element at `element_path` for `element` (same id; in place,
    /// so no sibling positions move).
    Swap {
        /// Path to the element including its own position.
        element_path: Vec<usize>,
        /// Replacement element.
        element: SceneElement,
    },
    /// Remove the subtree rooted at the element at `element_path`.
    Take {
        /// Path to the element including its own position.
        element_path: Vec<usize>,
    },
}

/// Planner-side mirror of the tree the journal will produce if committed.
/// Lean data only — ids, positions, and flags; never element clones. Kept
/// across phase 1 so every op validates and locates against the CURRENT
/// post-prior-ops state.
#[derive(Debug)]
struct PlannedIndex {
    /// Live id → planned placement.
    slots: HashMap<StableId, PlannedSlot>,
    /// Live group id → its ordered child ids (groups only; leaves have no
    /// entry).
    children: HashMap<StableId, Vec<StableId>>,
    /// Ordered root ids (mirror of the root element list).
    roots: Vec<StableId>,
    /// Planned total node count (base-counted, then advanced per payload)
    /// for the result node budget.
    nodes: usize,
}

impl PlannedIndex {
    /// Build the mirror from the CURRENT base tree with exactly one walk,
    /// O(scene) in time and lean-data memory (no element clones).
    fn from_elements(elements: &[SceneElement]) -> Self {
        let mut index = Self {
            slots: HashMap::new(),
            children: HashMap::new(),
            roots: elements.iter().map(|element| element.stable_id()).collect(),
            nodes: 0,
        };
        // Reverse-pushed work stack so pops visit document order; root
        // elements sit at depth 1 and each group level adds one.
        let mut stack: Vec<(&SceneElement, Option<StableId>, usize, usize)> = elements
            .iter()
            .enumerate()
            .rev()
            .map(|(pos, element)| (element, None, pos, 1usize))
            .collect();
        while let Some((element, parent, pos, depth)) = stack.pop() {
            let id = element.stable_id();
            let is_group = matches!(element, SceneElement::Group { .. });
            index.slots.insert(
                id,
                PlannedSlot {
                    parent,
                    pos,
                    depth,
                    is_group,
                },
            );
            index.nodes += 1;
            if let SceneElement::Group { children, .. } = element {
                index
                    .children
                    .insert(id, children.iter().map(|child| child.stable_id()).collect());
                for (pos, child) in children.iter().enumerate().rev() {
                    stack.push((child, Some(id), pos, depth + 1));
                }
            }
        }
        index
    }

    /// `true` while `id` is live in the planned state.
    fn is_live(&self, id: StableId) -> bool {
        self.slots.contains_key(&id)
    }

    /// Array path from the root element list to the live element `id`,
    /// ending at its own position. O(depth) parent-chain walk; the parent
    /// chain of a live id is live, so unreachability would be planner
    /// corruption.
    fn path_of(&self, id: StableId) -> Vec<usize> {
        debug_assert!(self.is_live(id), "path derivation for a retired id");
        let mut path = Vec::with_capacity(8);
        let mut current = Some(id);
        while let Some(node) = current {
            let slot = &self.slots[&node]; // parents of live nodes are live
            path.push(slot.pos);
            current = slot.parent;
        }
        path.reverse();
        path
    }

    /// Planned length of a container's ordered child list (the root list for
    /// a `None` parent).
    fn child_len(&self, parent: Option<StableId>) -> usize {
        match parent {
            None => self.roots.len(),
            Some(group) => self
                .children
                .get(&group)
                .expect("planner invariant: live group has a child list")
                .len(),
        }
    }

    /// Forget a live subtree in the mirror: the id, every descendant id, and
    /// their group child lists leave the planner. The container list keeps
    /// the subtree's root when `keep_top_in_parent` (Replace re-adopts into
    /// exactly that slot); otherwise the slot is dropped and the siblings to
    /// the right of it shift back by one, matching the `Vec::remove` phase 2
    /// will splice. Returns the retired node count for the node-budget math.
    fn retire_subtree(&mut self, id: StableId, keep_top_in_parent: bool) -> usize {
        let (parent, pos) = match self.slots.get(&id) {
            Some(slot) => (slot.parent, slot.pos),
            None => unreachable!("retire target is live in the planned state"),
        };
        let mut retired = 0usize;
        let mut stack: Vec<StableId> = vec![id];
        while let Some(node) = stack.pop() {
            self.slots.remove(&node);
            if let Some(kids) = self.children.remove(&node) {
                stack.extend(kids);
            }
            retired += 1;
        }
        if !keep_top_in_parent {
            // Detach from the container (root list or parent children) and
            // repair the sibling positions phase 2's `Vec::remove` shifts.
            let list = match parent {
                None => &mut self.roots,
                Some(group) => self
                    .children
                    .get_mut(&group)
                    .expect("planner invariant: live group has a child list"),
            };
            debug_assert_eq!(list[pos], id, "retire slot matches the mirror");
            list.remove(pos);
            for &sibling in list.iter().skip(pos) {
                if let Some(slot) = self.slots.get_mut(&sibling) {
                    slot.pos -= 1;
                }
            }
        }
        retired
    }

    /// Adopt a validated payload subtree into the planned state at the slot
    /// its top element will occupy: `top_pos: None` attaches at the end of
    /// the parent's child list (Add), `Some(pos)` re-adopts an already-held
    /// container slot (Replace). Checks the result budgets per adopted node
    /// (first-crossing values, payload order — the same cap+1 report shape
    /// the whole-tree gate used), records planned slots and child lists. The
    /// caller has already validated the payload structure and its id
    /// disjointness from the live set.
    fn adopt_subtree(
        &mut self,
        element: &SceneElement,
        parent: Option<StableId>,
        top_depth: usize,
        top_pos: Option<usize>,
    ) -> Result<(), SceneError> {
        let pos = match top_pos {
            Some(pos) => pos,
            None => self.child_len(parent),
        };
        if top_pos.is_none() {
            // Attach: the payload's top element extends the parent's child
            // list, keeping list position and slot in lockstep.
            match parent {
                None => self.roots.push(element.stable_id()),
                Some(group) => self
                    .children
                    .get_mut(&group)
                    .expect("planner invariant: live group has a child list")
                    .push(element.stable_id()),
            }
        }
        // Reverse-pushed stack, document order; depth carries absolute
        // planned depth (root = 1, the container's + 1 here).
        let mut stack: Vec<(&SceneElement, Option<StableId>, usize, usize)> =
            vec![(element, parent, top_depth, pos)];
        while let Some((element, parent, depth, pos)) = stack.pop() {
            let id = element.stable_id();
            let is_group = matches!(element, SceneElement::Group { .. });
            self.nodes += 1;
            if self.nodes > MAX_SCENE_NODES {
                return Err(SceneError::NodeBudgetExceeded {
                    nodes: self.nodes,
                    max: MAX_SCENE_NODES,
                });
            }
            if depth > MAX_SCENE_DEPTH {
                return Err(SceneError::DepthBudgetExceeded {
                    depth,
                    max: MAX_SCENE_DEPTH,
                });
            }
            self.slots.insert(
                id,
                PlannedSlot {
                    parent,
                    pos,
                    depth,
                    is_group,
                },
            );
            if let SceneElement::Group { children, .. } = element {
                self.children
                    .insert(id, children.iter().map(|child| child.stable_id()).collect());
                for (pos, child) in children.iter().enumerate().rev() {
                    stack.push((child, Some(id), pos, depth + 1));
                }
            }
        }
        Ok(())
    }
}

/// Phase 1 of [`PanelScene::apply_patch`]: replay `ops` against a mirror of
/// `elements`, validating each op fully (payload structure, id collisions,
/// parent/anchoring, result budgets) against the state all prior ops leave
/// behind, and journaling each op's pre-located mutation. Any error leaves
/// the mirror (and the caller's tree) exactly as the ops would have had it
/// had nothing happened, because only the mirror mutates during planning.
///
/// Per-op planning order matches the staged-clone pass so error selections
/// stay pinned: payload structural validation → id-collision check →
/// parent-anchoring check, then mirror adoption and result budgets.
fn plan_patch(elements: &[SceneElement], ops: Vec<SceneOp>) -> Result<Vec<PlannedOp>, SceneError> {
    let mut planned = PlannedIndex::from_elements(elements);
    let mut journal: Vec<PlannedOp> = Vec::new();
    for op in ops {
        match op {
            SceneOp::Add { parent, element } => {
                // Subtree-internal validation (ids, scalars, shapes, caps)
                // never touches the tree, so it runs first.
                validate_root(std::slice::from_ref(&element))?;
                // One payload walk for the fresh-id set, then the collision
                // check against the planned state.
                let fresh_ids = scan_payload_ids(&element);
                for id in &fresh_ids {
                    if planned.is_live(*id) {
                        return Err(SceneError::DuplicateId(*id));
                    }
                }
                // The parent must anchor a group in the planned state.
                let parent_depth = match parent {
                    None => 0usize,
                    Some(anchor) => {
                        let slot = planned
                            .slots
                            .get(&anchor)
                            .ok_or(SceneError::ParentMissing(anchor))?;
                        if !slot.is_group {
                            return Err(SceneError::ParentMissing(anchor));
                        }
                        slot.depth
                    }
                };
                // Adopt into the mirror (result budgets, planned slots).
                planned.adopt_subtree(&element, parent, parent_depth + 1, None)?;
                // Journal the push, located for THIS entry's phase-2 time
                // slot (the path is derived from the mirror state the phase
                // 2 replay of this entry will find).
                let group_path = match parent {
                    None => Vec::new(),
                    Some(anchor) => planned.path_of(anchor),
                };
                journal.push(PlannedOp::Insert {
                    group_path,
                    element,
                });
            }
            SceneOp::Replace { element } => {
                // Replace IS keyed by element.id: the id the element carries
                // is the id that gets swapped. A replacement that "renames"
                // its target does not express that as a second field — it is
                // expressed as a Remove + an Add.
                let target = element.stable_id();
                if !planned.is_live(target) {
                    return Err(SceneError::ElementMissing(target));
                }
                validate_root(std::slice::from_ref(&element))?;
                // Capture the target's slot, derive its pre-retire path (the
                // path is valid at THIS entry's phase-2 time slot), then
                // retire the outgoing subtree while keeping the container's
                // slot: the replacement re-adopts the very slot (same id,
                // per the keyed-by-id rule).
                let (parent, pos, top_depth) = {
                    let slot = &planned.slots[&target];
                    (slot.parent, slot.pos, slot.depth)
                };
                let element_path = planned.path_of(target);
                planned.nodes -= planned.retire_subtree(target, true);
                // The replacement may re-adopt ids retired with the outgoing
                // subtree, but must not collide with ids that survived of it.
                let fresh_ids = scan_payload_ids(&element);
                for id in &fresh_ids {
                    if planned.is_live(*id) {
                        return Err(SceneError::DuplicateId(*id));
                    }
                }
                planned.adopt_subtree(&element, parent, top_depth, Some(pos))?;
                journal.push(PlannedOp::Swap {
                    element_path,
                    element,
                });
            }
            SceneOp::Remove { id } => {
                if !planned.is_live(id) {
                    return Err(SceneError::ElementMissing(id));
                }
                // Derive the pre-retire path, then retire the subtree in the
                // mirror and detach it from its container (siblings shift
                // back, matching the `Vec::remove` phase 2 splices).
                let element_path = planned.path_of(id);
                planned.nodes -= planned.retire_subtree(id, false);
                journal.push(PlannedOp::Take { element_path });
            }
        }
    }
    Ok(journal)
}

/// Phase 2 of [`PanelScene::apply_patch`]: replay the pre-validated journal
/// into the real children vecs, strictly in order, with no further decisions
/// and no allocation beyond what the mutations themselves need. Every step
/// is infallible by construction (paths came from the planning mirror exactly
/// as the mirror tracked the same mutations); the commit point is the
/// journal replay plus the revision adoption, which is why a planning failure
/// leaves everything untouched.
fn apply_journal(elements: &mut Vec<SceneElement>, journal: Vec<PlannedOp>) {
    for entry in journal {
        match entry {
            PlannedOp::Insert {
                group_path,
                element,
            } => {
                children_at_mut(elements, &group_path).push(element);
            }
            PlannedOp::Swap {
                element_path,
                element,
            } => {
                *element_at_mut(elements, &element_path) = element;
            }
            PlannedOp::Take { element_path } => {
                let (last, parent_path) = element_path
                    .split_last()
                    .expect("journal element paths are non-empty");
                children_at_mut(elements, parent_path).remove(*last);
            }
        }
    }
}

/// The child list of the group at `group_path` (empty path = the root
/// element list). Path components index children of the previous list; the
/// referenced node must exist and anchor a group — the phase-2 guarantee,
/// so a disagreement is an `unreachable`, not an error path.
fn children_at_mut<'a>(
    elements: &'a mut Vec<SceneElement>,
    group_path: &[usize],
) -> &'a mut Vec<SceneElement> {
    let mut level = elements;
    for &index in group_path {
        level = match &mut level[index] {
            SceneElement::Group { children, .. } => children,
            other => unreachable!("journal location refers to a live group, found {other:?}"),
        };
    }
    level
}

/// Mutable view of the element at `element_path` (the last component is its
/// position inside the referenced container list).
fn element_at_mut<'a>(
    elements: &'a mut Vec<SceneElement>,
    element_path: &[usize],
) -> &'a mut SceneElement {
    let (last, parent_path) = element_path
        .split_last()
        .expect("journal element paths are non-empty");
    &mut children_at_mut(elements, parent_path)[*last]
}

/// Doc-order scan of one op payload (top element first), collecting the
/// fresh-id set the collision check compares against the planned state.
/// Single walk, O(payload); the payload itself is walked again by
/// [`PlannedIndex::adopt_subtree`], so this returns no deep payload data.
fn scan_payload_ids(element: &SceneElement) -> Vec<StableId> {
    let mut ids = Vec::new();
    let mut stack = vec![element];
    while let Some(element) = stack.pop() {
        ids.push(element.stable_id());
        if let SceneElement::Group { children, .. } = element {
            stack.extend(children.iter().rev());
        }
    }
    ids
}

/// Incremental scene update requests (API pre-pin).
///
/// Compatibility rule: new variants MUST be appended at the end so existing
/// plugins keep their bincode discriminant indices (`0..=2`; pinned by the
/// tests in this module).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneOp {
    /// Append a new element. `parent: None` appends at the scene root;
    /// `parent: Some(id)` appends into that element (which must anchor a
    /// group).
    Add {
        /// Anchor group id (root append when absent).
        parent: Option<StableId>,
        /// Element to insert; every id in its subtree must not already be
        /// live.
        element: SceneElement,
    },
    /// Swap the live element whose id matches out for the given element
    /// (keyed by `element.stable_id()`); the whole subtree moves with it.
    Replace {
        /// Element to install (its id is the swap key).
        element: SceneElement,
    },
    /// Remove the subtree rooted at the element carrying `id` (the element's
    /// own id included).
    Remove {
        /// Root id of the subtree to remove.
        id: StableId,
    },
}

/// A whole scene update: declared base, declared successor, and the ops that
/// carry one to the other. Decoded under the scene budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelScenePatch {
    /// Revision the ops were computed against.
    pub base_revision: SceneRevision,
    /// Successor revision applied when the ops settle.
    pub revision: SceneRevision,
    /// Ordered op list (empty patches are valid revision bumps).
    #[serde(default, deserialize_with = "budgeted_ops")]
    pub ops: Vec<SceneOp>,
}

// ---------------------------------------------------------------------------
// Structural validation (iterative on purpose)
// ---------------------------------------------------------------------------

/// Validate a root element list as one tree: ids, depth, nodes, finiteness,
/// shapes, and byte caps. Explicit work-stack traversal, so a validation pass
/// can never overflow the caller's stack the way the hostile payloads it
/// refuses would.
fn validate_root(elements: &[SceneElement]) -> Result<(), SceneError> {
    let mut seen: HashSet<StableId> = HashSet::new();
    let mut nodes: usize = 0;
    // Root elements sit at depth 1; each nested level adds one.
    let mut stack: Vec<(&SceneElement, usize)> = elements
        .iter()
        .rev()
        .map(|element| (element, 1usize))
        .collect();
    while let Some((element, depth)) = stack.pop() {
        if depth > MAX_SCENE_DEPTH {
            return Err(SceneError::DepthBudgetExceeded {
                depth,
                max: MAX_SCENE_DEPTH,
            });
        }
        nodes += 1;
        if nodes > MAX_SCENE_NODES {
            return Err(SceneError::NodeBudgetExceeded {
                nodes,
                max: MAX_SCENE_NODES,
            });
        }
        match element {
            SceneElement::Group { id, children, .. } => {
                register_id(&mut seen, *id)?;
                for child in children.iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
            SceneElement::Rect {
                id,
                pos,
                size,
                stroke,
                ..
            } => {
                register_id(&mut seen, *id)?;
                require_finite(std::slice::from_ref(pos))?;
                validate_rect_size(size)?;
                validate_stroke(stroke)?;
            }
            SceneElement::Path {
                id, points, stroke, ..
            } => {
                register_id(&mut seen, *id)?;
                if points.len() < 2 {
                    return Err(SceneError::TooFewPoints);
                }
                require_finite(points)?;
                validate_stroke(stroke)?;
            }
            SceneElement::CubicPath {
                id,
                segments,
                stroke,
                ..
            } => {
                register_id(&mut seen, *id)?;
                if segments.is_empty() {
                    return Err(SceneError::TooFewPoints);
                }
                for segment in segments {
                    require_finite(&[segment.start, segment.ctrl_a, segment.ctrl_b, segment.end])?;
                }
                validate_stroke(stroke)?;
            }
            SceneElement::Text {
                id,
                content,
                pos,
                size,
                ..
            } => {
                register_id(&mut seen, *id)?;
                require_finite(std::slice::from_ref(pos))?;
                if !size.is_finite() {
                    return Err(SceneError::NonFiniteCoordinate);
                }
                if content.len() > MAX_TEXT_BYTES {
                    return Err(SceneError::TextBudgetExceeded {
                        bytes: content.len(),
                        max: MAX_TEXT_BYTES,
                    });
                }
            }
            SceneElement::Asset { id, uri, frame, .. } => {
                register_id(&mut seen, *id)?;
                if uri.len() > MAX_ASSET_URI_BYTES {
                    return Err(SceneError::AssetUriBudgetExceeded {
                        bytes: uri.len(),
                        max: MAX_ASSET_URI_BYTES,
                    });
                }
                require_finite(std::slice::from_ref(&frame.pos))?;
                validate_rect_size(&frame.size)?;
            }
            SceneElement::Port { id, at, .. } => {
                register_id(&mut seen, *id)?;
                require_finite(std::slice::from_ref(at))?;
            }
            SceneElement::Marker { id, at, .. } => {
                register_id(&mut seen, *id)?;
                require_finite(std::slice::from_ref(at))?;
            }
            SceneElement::Edge {
                id, route, stroke, ..
            } => {
                register_id(&mut seen, *id)?;
                if route.is_empty() {
                    return Err(SceneError::TooFewPoints);
                }
                require_finite(route)?;
                validate_stroke(stroke)?;
            }
        }
    }
    Ok(())
}

/// Record one element id, rejecting the first duplicate seen.
fn register_id(seen: &mut HashSet<StableId>, id: StableId) -> Result<(), SceneError> {
    if seen.insert(id) {
        Ok(())
    } else {
        Err(SceneError::DuplicateId(id))
    }
}

/// Both components of every point must be finite (`NaN`/infinities are
/// rejected by validation, since serde wire formats can carry them).
fn require_finite(points: &[Vec2]) -> Result<(), SceneError> {
    for point in points {
        if !point.0[0].is_finite() || !point.0[1].is_finite() {
            return Err(SceneError::NonFiniteCoordinate);
        }
    }
    Ok(())
}

/// Rect sizes must be finite with both components strictly positive.
fn validate_rect_size(size: &Vec2) -> Result<(), SceneError> {
    if !size.0[0].is_finite() || !size.0[1].is_finite() || size.0[0] <= 0.0 || size.0[1] <= 0.0 {
        return Err(SceneError::InvalidRect);
    }
    Ok(())
}

/// Stroke widths must be finite and strictly positive. The error vocabulary
/// is structural only, so a bad width reports under
/// [`SceneError::NonFiniteCoordinate`].
fn validate_stroke(stroke: &Option<Stroke>) -> Result<(), SceneError> {
    if let Some(stroke) = stroke {
        if !stroke.width.is_finite() || stroke.width <= 0.0 {
            return Err(SceneError::NonFiniteCoordinate);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- construction helpers ------------------------------------------------

    fn sid(n: u64) -> StableId {
        StableId(n)
    }

    fn v2(x: f64, y: f64) -> Vec2 {
        Vec2([x, y])
    }

    fn rect_frame(x: f64, y: f64, w: f64, h: f64) -> Rect2D {
        Rect2D {
            pos: v2(x, y),
            size: v2(w, h),
        }
    }

    fn group(id: u64, children: Vec<SceneElement>) -> SceneElement {
        SceneElement::Group {
            id: sid(id),
            name: None,
            actions: vec![],
            drag: None,
            children,
            clip: None,
        }
    }

    fn plain_rect(id: u64) -> SceneElement {
        SceneElement::Rect {
            id: sid(id),
            name: None,
            actions: vec![],
            drag: None,
            pos: v2(1.0, 2.0),
            size: v2(3.0, 4.0),
            fill: None,
            stroke: None,
        }
    }

    fn plain_port(id: u64) -> SceneElement {
        SceneElement::Port {
            id: sid(id),
            name: None,
            actions: vec![],
            drag: None,
            at: v2(0.0, 0.0),
            label: None,
        }
    }

    fn plain_text(id: u64, content: &str) -> SceneElement {
        SceneElement::Text {
            id: sid(id),
            name: None,
            actions: vec![],
            drag: None,
            content: content.to_string(),
            pos: v2(0.0, 0.0),
            size: default_text_size(),
            color: SceneColor::Theme(ThemeColor::Normal),
        }
    }

    fn plain_asset(id: u64, uri: &str) -> SceneElement {
        SceneElement::Asset {
            id: sid(id),
            name: None,
            actions: vec![],
            drag: None,
            kind: AssetKind::Svg,
            uri: uri.to_string(),
            frame: rect_frame(0.0, 0.0, 1.0, 1.0),
        }
    }

    fn simple_scene(revision: u64, elements: Vec<SceneElement>) -> PanelScene {
        PanelScene {
            revision: SceneRevision(revision),
            elements,
        }
    }

    fn patch(base: u64, next: u64, ops: Vec<SceneOp>) -> PanelScenePatch {
        PanelScenePatch {
            base_revision: SceneRevision(base),
            revision: SceneRevision(next),
            ops,
        }
    }

    /// Snapshot of a scene for deep-compare before/after assertions.
    fn shot(scene: &PanelScene) -> (SceneRevision, Vec<SceneElement>) {
        (scene.revision, scene.elements.clone())
    }

    fn next_id(next: &mut u64) -> StableId {
        let id = StableId(*next);
        *next += 1;
        id
    }

    /// A chain of `groups` nested groups closing on a Port leaf; ids come
    /// from the counter (all unique), deepest node ends at depth `groups + 1`.
    fn nested_chain(groups: usize, next: &mut u64) -> SceneElement {
        let mut element = plain_port(next_id(next).0);
        for _ in 0..groups {
            element = group(next_id(next).0, vec![element]);
        }
        element
    }

    #[test]
    fn scene_bincode_and_json_roundtrips_representative_tree() {
        let scene = PanelScene {
            revision: SceneRevision(4),
            elements: vec![
                SceneElement::Group {
                    id: StableId(1),
                    name: Some("canvas".to_string()),
                    actions: vec!["click".to_string()],
                    drag: Some(DragMeta {
                        opaque: Some("hand-edit".to_string()),
                    }),
                    children: vec![
                        SceneElement::Rect {
                            id: StableId(2),
                            name: None,
                            actions: vec![],
                            drag: None,
                            pos: Vec2([10.0, 20.0]),
                            size: Vec2([30.0, 40.0]),
                            fill: Some(SceneColor::Rgba(Rgba8([10, 20, 30, 255]))),
                            stroke: Some(Stroke {
                                color: SceneColor::Theme(ThemeColor::Accent),
                                width: 1.5,
                            }),
                        },
                        SceneElement::Path {
                            id: StableId(3),
                            name: None,
                            actions: vec![],
                            drag: None,
                            points: vec![v2(0.5, 0.5), v2(1.5, 0.25), v2(2.0, 1.0)],
                            closed: true,
                            stroke: None,
                            fill: Some(SceneColor::Theme(ThemeColor::Muted)),
                        },
                        SceneElement::CubicPath {
                            id: StableId(4),
                            name: None,
                            actions: vec![],
                            drag: None,
                            segments: vec![CubicSegment {
                                start: v2(0.0, 0.0),
                                ctrl_a: v2(1.0, 0.0),
                                ctrl_b: v2(2.0, 1.0),
                                end: v2(3.0, 1.0),
                            }],
                            closed: false,
                            stroke: Some(Stroke {
                                color: SceneColor::Theme(ThemeColor::Strong),
                                width: 0.75,
                            }),
                            fill: None,
                        },
                        SceneElement::Text {
                            id: StableId(5),
                            name: None,
                            actions: vec![],
                            drag: None,
                            content: "Spaße 150 mm".to_string(),
                            pos: v2(4.0, 5.0),
                            size: 11.5,
                            color: SceneColor::Theme(ThemeColor::Success),
                        },
                        SceneElement::Asset {
                            id: StableId(6),
                            name: None,
                            actions: vec![],
                            drag: None,
                            kind: AssetKind::Svg,
                            uri: "svg:bracket".to_string(),
                            frame: rect_frame(6.0, 7.0, 8.0, 9.0),
                        },
                        SceneElement::Port {
                            id: StableId(7),
                            name: Some("inlet".to_string()),
                            actions: vec![],
                            drag: None,
                            at: v2(11.0, 12.0),
                            label: Some("A".to_string()),
                        },
                        SceneElement::Marker {
                            id: StableId(8),
                            name: None,
                            actions: vec![],
                            drag: None,
                            at: v2(13.0, 14.0),
                            kind: MarkerKind::Diamond,
                            color: SceneColor::Theme(ThemeColor::Warning),
                        },
                        SceneElement::Edge {
                            id: StableId(9),
                            name: None,
                            actions: vec![],
                            drag: None,
                            a: StableId(7),
                            b: StableId(2),
                            route: vec![v2(11.0, 12.0), v2(10.0, 20.0)],
                            stroke: Some(Stroke {
                                color: SceneColor::Theme(ThemeColor::Danger),
                                width: 1.0,
                            }),
                            start_marker: Some(MarkerKind::Dot),
                            end_marker: Some(MarkerKind::Arrow),
                        },
                    ],
                    clip: Some(rect_frame(0.0, 0.0, 100.0, 100.0)),
                },
                group(10, vec![]),
            ],
        };
        scene.validate().expect("representative tree is valid");
        assert_eq!(scene.elements[0].stable_id(), StableId(1));
        assert_eq!(scene.elements[1].stable_id(), StableId(10));

        // Bincode preserves the tree exactly.
        let bytes = bincode::serialize(&scene).expect("serialize scene");
        let restored: PanelScene = bincode::deserialize(&bytes).expect("decode scene");
        assert_eq!(scene, restored);

        // JSON roundtrips the same tree (manifest/registry tooling shape).
        let json = serde_json::to_string(&scene).expect("scene json");
        let from_json: PanelScene = serde_json::from_str(&json).expect("decode scene json");
        assert_eq!(scene, from_json);
    }

    #[test]
    fn scene_element_and_scene_op_bincode_discriminants_are_pinned() {
        let tag = |element: &SceneElement| -> u32 {
            let bytes = bincode::serialize(element).expect("serialize scene element");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let op_tag = |op: &SceneOp| -> u32 {
            let bytes = bincode::serialize(op).expect("serialize scene op");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };

        let cases: Vec<(u32, SceneElement)> = vec![
            (0, group(31, vec![])),
            (1, plain_rect(32)),
            (
                2,
                SceneElement::Path {
                    id: sid(33),
                    name: None,
                    actions: vec![],
                    drag: None,
                    points: vec![v2(0.0, 0.0), v2(1.0, 1.0)],
                    closed: false,
                    stroke: None,
                    fill: None,
                },
            ),
            (
                3,
                SceneElement::CubicPath {
                    id: sid(34),
                    name: None,
                    actions: vec![],
                    drag: None,
                    segments: vec![CubicSegment {
                        start: v2(0.0, 0.0),
                        ctrl_a: v2(1.0, 0.0),
                        ctrl_b: v2(2.0, 1.0),
                        end: v2(3.0, 1.0),
                    }],
                    closed: false,
                    stroke: None,
                    fill: None,
                },
            ),
            (
                4,
                SceneElement::Text {
                    id: sid(35),
                    name: None,
                    actions: vec![],
                    drag: None,
                    content: "x".to_string(),
                    pos: v2(0.0, 0.0),
                    size: default_text_size(),
                    color: SceneColor::Theme(ThemeColor::Normal),
                },
            ),
            (5, plain_asset(36, "svg:x")),
            (
                6,
                SceneElement::Port {
                    id: sid(37),
                    name: None,
                    actions: vec![],
                    drag: None,
                    at: v2(1.0, 2.0),
                    label: None,
                },
            ),
            (
                7,
                SceneElement::Marker {
                    id: sid(38),
                    name: None,
                    actions: vec![],
                    drag: None,
                    at: v2(1.0, 2.0),
                    kind: MarkerKind::Dot,
                    color: SceneColor::Theme(ThemeColor::Normal),
                },
            ),
            (
                8,
                SceneElement::Edge {
                    id: sid(39),
                    name: None,
                    actions: vec![],
                    drag: None,
                    a: sid(37),
                    b: sid(38),
                    route: vec![v2(0.0, 0.0)],
                    stroke: None,
                    start_marker: None,
                    end_marker: None,
                },
            ),
        ];
        // The wire grows append-only; these indices are the whole pin.
        for (expected_discriminant, element) in &cases {
            assert_eq!(
                tag(element),
                *expected_discriminant,
                "SceneElement discriminant drift for {element:?}"
            );
        }
        assert_eq!(
            op_tag(&SceneOp::Add {
                parent: None,
                element: plain_rect(71)
            }),
            0
        );
        assert_eq!(
            op_tag(&SceneOp::Replace {
                element: plain_rect(71)
            }),
            1
        );
        assert_eq!(op_tag(&SceneOp::Remove { id: sid(71) }), 2);
    }

    #[test]
    fn apply_patch_adds_updates_removes_and_bumps_revision() {
        // One patch performing add(root), add(child of the new group),
        // replace, remove — in that order, all against revision 7.
        let mut scene = simple_scene(7, vec![group(1, vec![plain_rect(2)]), plain_rect(3)]);
        let applied = patch(
            7,
            8,
            vec![
                SceneOp::Add {
                    parent: None,
                    element: group(10, vec![]),
                },
                SceneOp::Add {
                    // The parent is a group added EARLIER IN THE SAME PATCH.
                    parent: Some(StableId(10)),
                    element: plain_rect(11),
                },
                SceneOp::Replace {
                    element: SceneElement::Rect {
                        id: StableId(3),
                        name: Some("replaced".to_string()),
                        actions: vec![],
                        drag: None,
                        pos: v2(9.0, 9.0),
                        size: v2(1.0, 1.0),
                        fill: Some(SceneColor::Theme(ThemeColor::Accent)),
                        stroke: None,
                    },
                },
                SceneOp::Remove { id: StableId(2) },
            ],
        );
        scene.apply_patch(applied).expect("whole-patch apply");
        assert_eq!(scene.revision, SceneRevision(8));
        scene.validate().expect("patched tree is valid");

        // Root order: group(1) emptied, rect(3) replaced, group(10) grown.
        assert_eq!(scene.elements.len(), 3);
        match &scene.elements[0] {
            SceneElement::Group { children, .. } => assert!(children.is_empty()),
            other => panic!("expected the emptied group, got {other:?}"),
        }
        match &scene.elements[1] {
            SceneElement::Rect { name, fill, .. } => {
                assert_eq!(name.as_deref(), Some("replaced"));
                assert_eq!(*fill, Some(SceneColor::Theme(ThemeColor::Accent)));
            }
            other => panic!("expected the replaced rect, got {other:?}"),
        }
        match &scene.elements[2] {
            SceneElement::Group { id, children, .. } => {
                assert_eq!(*id, StableId(10));
                assert_eq!(children.len(), 1);
                assert_eq!(children[0].stable_id(), StableId(11));
            }
            other => panic!("expected the new group with its child, got {other:?}"),
        }
    }

    #[test]
    fn stale_base_patch_is_rejected_without_partial_writes() {
        let mut scene = simple_scene(3, vec![group(1, vec![plain_rect(2)])]);
        let before = shot(&scene);

        // Stale base; the ops would even have applied cleanly if the base
        // had matched: Remove {id:2} targets a live element.
        let stale = patch(
            2,
            4,
            vec![
                SceneOp::Add {
                    parent: None,
                    element: plain_rect(50),
                },
                SceneOp::Remove { id: StableId(2) },
            ],
        );
        let error = scene
            .apply_patch(stale)
            .expect_err("stale base must refuse");
        assert_eq!(
            error,
            SceneError::StaleBase {
                current: SceneRevision(3),
                given: SceneRevision(2),
            }
        );
        assert_eq!(shot(&scene), before);

        // Right base and revision, but the SECOND op fails after the FIRST
        // would have applied: validation still leaves zero writes behind.
        let half_valid = patch(
            3,
            4,
            vec![
                SceneOp::Add {
                    parent: None,
                    element: plain_rect(50),
                },
                SceneOp::Remove { id: StableId(999) },
            ],
        );
        let error = scene
            .apply_patch(half_valid)
            .expect_err("missing remove target must refuse");
        assert_eq!(error, SceneError::ElementMissing(StableId(999)));
        assert_eq!(shot(&scene), before);
        assert_eq!(scene.revision, SceneRevision(3));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        // Whole-tree uniqueness via validate().
        let scene = simple_scene(1, vec![plain_rect(5), plain_rect(5)]);
        assert_eq!(scene.validate(), Err(SceneError::DuplicateId(StableId(5))));

        // An Add whose id is already live in the tree.
        let mut scene = simple_scene(1, vec![plain_rect(5)]);
        let before = shot(&scene);
        let error = scene
            .apply_patch(patch(
                1,
                2,
                vec![SceneOp::Add {
                    parent: None,
                    element: plain_rect(5),
                }],
            ))
            .expect_err("add colliding with the tree must refuse");
        assert_eq!(error, SceneError::DuplicateId(StableId(5)));
        assert_eq!(shot(&scene), before);

        // Two Adds in one patch that collide with each other.
        let mut scene = simple_scene(1, vec![]);
        let error = scene
            .apply_patch(patch(
                1,
                2,
                vec![
                    SceneOp::Add {
                        parent: None,
                        element: plain_rect(5),
                    },
                    SceneOp::Add {
                        parent: None,
                        element: plain_rect(5),
                    },
                ],
            ))
            .expect_err("op-to-op id collision must refuse");
        assert_eq!(error, SceneError::DuplicateId(StableId(5)));
    }

    #[test]
    fn depth_and_node_budgets_reject_oversized_trees() {
        // Within budget: deepest node lands at depth 64 exactly.
        let mut next = 0u64;
        let scene = simple_scene(1, vec![nested_chain(MAX_SCENE_DEPTH - 1, &mut next)]);
        assert_eq!(scene.validate(), Ok(()));

        // 65-deep nesting trips the depth budget.
        let mut next = 0u64;
        let scene = simple_scene(1, vec![nested_chain(MAX_SCENE_DEPTH, &mut next)]);
        let error = scene.validate().expect_err("65-deep nesting must refuse");
        assert!(matches!(
            &error,
            SceneError::DepthBudgetExceeded { depth, .. } if *depth == MAX_SCENE_DEPTH + 1
        ));
        assert!(error.to_string().contains("depth budget"));

        // 10_001 elements (a root group plus 10_000 children) trips the node
        // budget.
        let children = (1..=MAX_SCENE_NODES as u64).map(plain_port).collect();
        let scene = simple_scene(1, vec![group(0, children)]);
        let error = scene.validate().expect_err("10_001 nodes must refuse");
        assert!(matches!(
            &error,
            SceneError::NodeBudgetExceeded { nodes, .. } if *nodes == MAX_SCENE_NODES + 1
        ));
        assert!(error.to_string().contains("node budget"));
    }

    #[test]
    fn patch_op_budget_is_enforced() {
        let mut scene = simple_scene(1, vec![]);
        let before = shot(&scene);
        let ops = (0..=MAX_PATCH_OPS)
            .map(|i| SceneOp::Remove {
                id: StableId(i as u64),
            })
            .collect();
        let error = scene
            .apply_patch(patch(1, 2, ops))
            .expect_err("10_001 ops must refuse");
        assert!(matches!(
            &error,
            SceneError::PatchOpBudgetExceeded { ops, .. } if *ops == MAX_PATCH_OPS + 1
        ));
        assert!(error.to_string().contains("patch op budget"));
        assert_eq!(shot(&scene), before);
    }

    #[test]
    fn text_and_asset_uri_caps_are_enforced() {
        // Text payload one byte past the cap.
        let scene = simple_scene(1, vec![plain_text(1, &"x".repeat(MAX_TEXT_BYTES + 1))]);
        let error = scene.validate().expect_err("oversized run must refuse");
        assert!(matches!(
            &error,
            SceneError::TextBudgetExceeded { bytes, .. } if *bytes == MAX_TEXT_BYTES + 1
        ));
        assert!(error.to_string().contains("text budget"));
        // The bounded decode itself is budget-neutral for a single node; the
        // cap lives at validation, which is what a host calls before render.
        let bytes = bincode::serialize(&scene).expect("serialize scene");
        let decoded: PanelScene = bincode::deserialize(&bytes).expect("decode scene");
        assert_eq!(
            decoded.validate(),
            Err(SceneError::TextBudgetExceeded {
                bytes: MAX_TEXT_BYTES + 1,
                max: MAX_TEXT_BYTES,
            })
        );

        // Asset uri one byte past the cap.
        let uri_scene = simple_scene(
            1,
            vec![plain_asset(1, &"u".repeat(MAX_ASSET_URI_BYTES + 1))],
        );
        let error = uri_scene.validate().expect_err("oversized uri must refuse");
        assert!(matches!(
            &error,
            SceneError::AssetUriBudgetExceeded { bytes, .. } if *bytes == MAX_ASSET_URI_BYTES + 1
        ));
        assert!(error.to_string().contains("asset uri budget"));
        let bytes = bincode::serialize(&uri_scene).expect("serialize scene");
        let decoded: PanelScene = bincode::deserialize(&bytes).expect("decode scene");
        assert_eq!(
            decoded.validate(),
            Err(SceneError::AssetUriBudgetExceeded {
                bytes: MAX_ASSET_URI_BYTES + 1,
                max: MAX_ASSET_URI_BYTES,
            })
        );

        // Cap-exact sizes are legal.
        let ok = simple_scene(
            1,
            vec![
                plain_text(1, &"x".repeat(MAX_TEXT_BYTES)),
                plain_asset(2, &"u".repeat(MAX_ASSET_URI_BYTES)),
            ],
        );
        assert_eq!(ok.validate(), Ok(()));
    }

    #[test]
    fn non_finite_coordinates_are_rejected() {
        let nan_text = simple_scene(
            1,
            vec![SceneElement::Text {
                id: sid(1),
                name: None,
                actions: vec![],
                drag: None,
                content: "x".to_string(),
                pos: v2(f64::NAN, 0.0),
                size: default_text_size(),
                color: SceneColor::Theme(ThemeColor::Normal),
            }],
        );
        assert_eq!(nan_text.validate(), Err(SceneError::NonFiniteCoordinate));

        let inf_path = simple_scene(
            1,
            vec![SceneElement::Path {
                id: sid(1),
                name: None,
                actions: vec![],
                drag: None,
                points: vec![v2(0.0, 0.0), v2(f64::INFINITY, 0.0)],
                closed: false,
                stroke: None,
                fill: None,
            }],
        );
        assert_eq!(inf_path.validate(), Err(SceneError::NonFiniteCoordinate));

        // A wire-borne NaN (JSON cannot express it; bincode can) still fails
        // validation before anything trusts the geometry.
        let bytes = bincode::serialize(&inf_path).expect("serialize scene");
        let decoded: PanelScene = bincode::deserialize(&bytes).expect("decode scene");
        assert_eq!(decoded.validate(), Err(SceneError::NonFiniteCoordinate));

        // Stroke widths ride the same structural gate.
        let wide = simple_scene(
            1,
            vec![SceneElement::Rect {
                id: sid(1),
                name: None,
                actions: vec![],
                drag: None,
                pos: v2(0.0, 0.0),
                size: v2(1.0, 1.0),
                fill: None,
                stroke: Some(Stroke {
                    color: SceneColor::Theme(ThemeColor::Normal),
                    width: f32::NAN,
                }),
            }],
        );
        assert_eq!(wide.validate(), Err(SceneError::NonFiniteCoordinate));
    }

    #[test]
    fn non_monotonic_revision_is_rejected_even_for_identical_ops() {
        let mut scene = simple_scene(5, vec![plain_rect(1)]);
        let before = shot(&scene);
        let ops = vec![SceneOp::Add {
            parent: None,
            element: plain_rect(9),
        }];

        // Rewind (successor below the current revision): refused.
        let rewind = patch(5, 4, ops.clone());
        let error = scene.apply_patch(rewind).expect_err("rewind must refuse");
        assert!(
            matches!(
                &error,
                SceneError::NonMonotonicRevision {
                    current: SceneRevision(5),
                    given: SceneRevision(4),
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(shot(&scene), before);

        // Level (successor equal to the current revision): refused too.
        let level = patch(5, 5, ops.clone());
        let error = scene
            .apply_patch(level)
            .expect_err("level revision must refuse");
        assert!(matches!(&error, SceneError::NonMonotonicRevision { .. }));
        assert_eq!(shot(&scene), before);

        // The same ops with the right successor apply cleanly.
        scene.apply_patch(patch(5, 6, ops)).expect("bump applies");
        assert_eq!(scene.revision, SceneRevision(6));
        assert_eq!(scene.elements.len(), 2);
    }

    #[test]
    fn zero_op_patch_is_a_noop_that_advances_revision() {
        let mut scene = simple_scene(9, vec![group(1, vec![plain_rect(2)]), plain_port(3)]);
        let before_elements = scene.elements.clone();
        scene
            .apply_patch(patch(9, 10, vec![]))
            .expect("no-op applies");
        assert_eq!(scene.elements, before_elements);
        assert_eq!(scene.revision, SceneRevision(10));
    }

    #[test]
    fn parent_and_element_missing_errors() {
        let mut scene = simple_scene(1, vec![plain_rect(2)]);
        let before = shot(&scene);

        // Add into an unknown parent id.
        let error = scene
            .apply_patch(patch(
                1,
                2,
                vec![SceneOp::Add {
                    parent: Some(StableId(404)),
                    element: plain_rect(5),
                }],
            ))
            .expect_err("unknown parent must refuse");
        assert_eq!(error, SceneError::ParentMissing(StableId(404)));
        assert_eq!(shot(&scene), before);

        // Add into an id that exists but does not anchor a group: there is
        // nothing to append into, so the same structural rejection applies.
        let error = scene
            .apply_patch(patch(
                1,
                2,
                vec![SceneOp::Add {
                    parent: Some(StableId(2)),
                    element: plain_rect(5),
                }],
            ))
            .expect_err("non-group parent must refuse");
        assert_eq!(error, SceneError::ParentMissing(StableId(2)));

        // Replace of an unknown id.
        let error = scene
            .apply_patch(patch(
                1,
                2,
                vec![SceneOp::Replace {
                    element: plain_rect(404),
                }],
            ))
            .expect_err("unknown replace target must refuse");
        assert_eq!(error, SceneError::ElementMissing(StableId(404)));

        // Remove of an unknown id.
        let error = scene
            .apply_patch(patch(1, 2, vec![SceneOp::Remove { id: StableId(405) }]))
            .expect_err("unknown remove target must refuse");
        assert_eq!(error, SceneError::ElementMissing(StableId(405)));

        // None of the four refusals touched the tree.
        assert_eq!(shot(&scene), before);
    }

    #[test]
    fn theme_color_json_is_lowercase() {
        // Every token serializes to its lowercase name...
        assert_eq!(
            serde_json::to_value(ThemeColor::Normal).expect("json normal"),
            serde_json::json!("normal")
        );
        assert_eq!(
            serde_json::to_value(ThemeColor::Success).expect("json success"),
            serde_json::json!("success")
        );
        assert_eq!(
            serde_json::to_value(ThemeColor::Danger).expect("json danger"),
            serde_json::json!("danger")
        );
        // ...and parses back from the lowercase text.
        let parsed: ThemeColor =
            serde_json::from_value(serde_json::json!("success")).expect("theme token");
        assert_eq!(parsed, ThemeColor::Success);

        // SceneColor rides the default externally-tagged shape — PascalCase
        // variant keys like DialogControl's controls ("Theme"/"Rgba"); the
        // lowercase pin above is about the token INSIDE the theme arm.
        let parsed: SceneColor =
            serde_json::from_value(serde_json::json!({ "Theme": "success" })).expect("theme arm");
        assert_eq!(parsed, SceneColor::Theme(ThemeColor::Success));
        let parsed: SceneColor =
            serde_json::from_value(serde_json::json!({ "Rgba": [1, 2, 3, 255] }))
                .expect("rgba arm");
        assert_eq!(parsed, SceneColor::Rgba(Rgba8([1, 2, 3, 255])));
        assert_eq!(
            serde_json::to_value(SceneColor::Rgba(Rgba8([1, 2, 3, 255]))).expect("rgba json"),
            serde_json::json!({ "Rgba": [1, 2, 3, 255] })
        );

        // The default color is the Normal theme token everywhere it is used.
        assert_eq!(SceneColor::default(), SceneColor::Theme(ThemeColor::Normal));
    }

    // -- SCN-010 additions (two-phase in-place patching, SceneSummary) -------

    #[test]
    fn apply_patch_in_place_matches_staged_semantics() {
        // Kitchen-sink patch over a five-node base exercising every op in
        // order: add at root, add into a group added EARLIER IN THE SAME
        // PATCH, a replace that retires ids a fresh subtree re-adopts, a
        // remove, and a replace (keyed at the root level) re-adopting an id
        // the remove retired. The expected result is HAND-WRITTEN below (not
        // derived from any staged-clone run): the in-place journal must
        // produce the identical tree.
        //
        // base (rev 1): g1[ r2, g3[ p4 ] ], r5
        let mut scene = simple_scene(
            1,
            vec![
                group(1, vec![plain_rect(2), group(3, vec![plain_port(4)])]),
                plain_rect(5),
            ],
        );
        scene
            .apply_patch(patch(
                1,
                2,
                vec![
                    SceneOp::Add {
                        parent: None,
                        element: group(6, vec![]),
                    },
                    SceneOp::Add {
                        parent: Some(StableId(6)),
                        element: plain_text(7, "hi"),
                    },
                    // Retires ids {3, 4}; the replacement re-adopts both
                    // (keyed id 3 plus retired id 4 inside it).
                    SceneOp::Replace {
                        element: group(3, vec![plain_rect(4), plain_port(8)]),
                    },
                    SceneOp::Remove { id: StableId(2) },
                    // Swaps the root rect (keyed by id 5) and re-adopts
                    // id 2, retired by the previous op's remove.
                    SceneOp::Replace {
                        element: group(5, vec![plain_rect(2)]),
                    },
                ],
            ))
            .expect("kitchen-sink patch applies");
        assert_eq!(scene.revision, SceneRevision(2));

        // Hand-written expected literal:
        // g1[ g3[ r4, p8 ] ], g5[ r2 ], g6[ t7 ].
        assert_eq!(
            scene.elements,
            vec![
                group(1, vec![group(3, vec![plain_rect(4), plain_port(8)])]),
                group(5, vec![plain_rect(2)]),
                group(6, vec![plain_text(7, "hi")]),
            ]
        );
        scene.validate().expect("patched tree is valid");
    }

    #[test]
    fn apply_patch_failure_leaves_no_clone_and_no_mutation() {
        // The in-place path reasserts the atomicity contract: failures
        // BETWEEN ops — including one whose first op would have applied
        // cleanly — leave scene and revision byte-for-byte unchanged.
        let mut scene = simple_scene(7, vec![group(1, vec![plain_rect(2)]), plain_port(3)]);
        let before = shot(&scene);

        // The second remove targets an id the first op's deletion retired;
        // phase 1 must fail on it with zero writes to `self`.
        let error = scene
            .apply_patch(patch(
                7,
                8,
                vec![
                    SceneOp::Remove { id: StableId(2) },
                    SceneOp::Remove { id: StableId(2) }, // no longer live
                ],
            ))
            .expect_err("remove of an id retired mid-patch must refuse");
        assert_eq!(error, SceneError::ElementMissing(StableId(2)));
        assert_eq!(shot(&scene), before);
        assert_eq!(scene.revision, SceneRevision(7));

        // A stale base refuses in phase 0 before any planning happens.
        let error = scene
            .apply_patch(patch(6, 9, vec![SceneOp::Remove { id: StableId(2) }]))
            .expect_err("stale base must refuse");
        assert_eq!(
            error,
            SceneError::StaleBase {
                current: SceneRevision(7),
                given: SceneRevision(6),
            }
        );
        assert_eq!(shot(&scene), before);
    }

    #[test]
    fn apply_patch_journal_is_bounded() {
        // Load-bearing coincidence per MAX_PATCH_OPS' docs: the op cap and
        // the node cap coincide today, and `budgeted_ops` charges both kinds
        // of nodes against the same per-frame counter — pin the equality so
        // moving one budget alone cannot slip through review.
        assert_eq!(MAX_PATCH_OPS, MAX_SCENE_NODES);

        // One op applied to a full-budget 10_000-node scene. Portable
        // allocation behavior is not directly assertable, so the no-clone
        // contract is pinned behaviorally: the root element list keeps its
        // ORIGINAL buffer (the address of the untouched first root element
        // is identical before and after) — the staged-clone pass would have
        // rebuilt the root list with a fresh buffer on every patch — and the
        // result stays exactly at budget, valid, revision-advanced. The
        // patch-scaling evidence proper is the SCN-012 benchmark (ADR-004).
        let children = (1..MAX_SCENE_NODES as u64).map(plain_port).collect();
        let mut scene = simple_scene(1, vec![group(0, children)]);
        scene.validate().expect("base at exact node budget");
        let root_addr_before = scene.elements.first().map(|e| e as *const _ as usize);

        scene
            .apply_patch(patch(
                1,
                2,
                vec![SceneOp::Replace {
                    element: SceneElement::Port {
                        id: StableId(4_242),
                        name: Some("moved".to_string()),
                        actions: vec![],
                        drag: None,
                        at: v2(9.0, 9.0),
                        label: None,
                    },
                }],
            ))
            .expect("one-op patch applies to a full-budget scene");
        let root_addr_after = scene.elements.first().map(|e| e as *const _ as usize);

        // The patch never grows the scene past budget; the replaced op is
        // budget-neutral, so the node count stands exactly.
        let summary = scene.scene_summary();
        assert_eq!(summary.revision, SceneRevision(2));
        assert_eq!(summary.nodes, MAX_SCENE_NODES);
        assert!(
            summary.nodes <= MAX_SCENE_NODES,
            "in-place result stays within the node budget"
        );
        assert_eq!(summary.by_kind.groups, 1);
        assert_eq!(summary.by_kind.ports, MAX_SCENE_NODES - 1);
        scene
            .validate()
            .expect("in-place-patched full-budget tree is valid");
        assert_eq!(
            root_addr_before, root_addr_after,
            "an untouched root element keeps its buffer: patching must not clone the tree"
        );
    }

    #[test]
    fn scene_summary_of_fixture() {
        // Hetero hand-built scene with independently counted kinds/roots.
        // g1[ r2, p3, t4 ], a5, m6, e7 → 7 nodes, 4 roots.
        let scene = simple_scene(
            12,
            vec![
                group(1, vec![plain_rect(2), plain_port(3), plain_text(4, "t")]),
                plain_asset(5, "svg:x"),
                SceneElement::Marker {
                    id: sid(6),
                    name: None,
                    actions: vec![],
                    drag: None,
                    at: v2(1.0, 1.0),
                    kind: MarkerKind::Dot,
                    color: SceneColor::Theme(ThemeColor::Normal),
                },
                SceneElement::Edge {
                    id: sid(7),
                    name: None,
                    actions: vec![],
                    drag: None,
                    a: StableId(3),
                    b: StableId(1),
                    route: vec![v2(0.0, 0.0)],
                    stroke: None,
                    start_marker: None,
                    end_marker: None,
                },
            ],
        );
        let summary = scene.scene_summary();
        assert_eq!(summary.revision, SceneRevision(12));
        assert_eq!(summary.nodes, 7);
        assert_eq!(summary.by_kind.groups, 1);
        assert_eq!(summary.by_kind.rects, 1);
        assert_eq!(summary.by_kind.paths, 0);
        assert_eq!(summary.by_kind.cubic_paths, 0);
        assert_eq!(summary.by_kind.texts, 1);
        assert_eq!(summary.by_kind.assets, 1);
        assert_eq!(summary.by_kind.ports, 1);
        assert_eq!(summary.by_kind.markers, 1);
        assert_eq!(summary.by_kind.edges, 1);
        assert_eq!(summary.by_kind.total(), summary.nodes);
        assert_eq!(
            summary.root_ids,
            vec![StableId(1), StableId(5), StableId(6), StableId(7)]
        );
        assert_eq!(summary.root_ids_truncated, 0);
        assert!(!summary.overflow_roots);

        // Deep nesting reads identically: counts by kind, not by depth.
        let mut next = 100u64;
        let scene = simple_scene(3, vec![nested_chain(4, &mut next)]);
        let summary = scene.scene_summary();
        assert_eq!(summary.nodes, 5); // 4 groups + the port leaf
        assert_eq!(summary.by_kind.groups, 4);
        assert_eq!(summary.by_kind.ports, 1);
        assert_eq!(summary.root_ids.len(), 1);

        // Truncation contract at the cap boundary: exactly 64 roots is
        // untruncated; 65 caps and reports the dropped count.
        let mut next = 0u64;
        let mut scene = simple_scene(
            3,
            (0..MAX_SUMMARY_ROOT_IDS as u64)
                .map(|_| nested_chain(0, &mut next))
                .collect(),
        );
        let summary = scene.scene_summary();
        assert_eq!(summary.root_ids.len(), MAX_SUMMARY_ROOT_IDS);
        assert_eq!(summary.root_ids_truncated, 0);
        assert!(!summary.overflow_roots);
        assert_eq!(summary.nodes, MAX_SUMMARY_ROOT_IDS);

        scene.elements.push(nested_chain(0, &mut next));
        let summary = scene.scene_summary();
        assert_eq!(summary.root_ids.len(), MAX_SUMMARY_ROOT_IDS);
        assert_eq!(summary.root_ids_truncated, 1);
        assert!(summary.overflow_roots);
        assert_eq!(summary.nodes, MAX_SUMMARY_ROOT_IDS + 1);
        assert_eq!(summary.by_kind.ports, MAX_SUMMARY_ROOT_IDS + 1);
    }
}
