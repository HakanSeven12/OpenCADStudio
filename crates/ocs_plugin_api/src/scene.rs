//! The panel scene data model: the vocabulary a plugin paints a panel with.
//!
//! A [`PanelScene`] is an ordered root element list in paint order; children
//! nest inside groups, and every element carries a plugin-assigned
//! [`StableId`]. The model is pure data — serde plus `std` only, no
//! rendering, no host coupling — and its deserialization is bounded by the
//! same thread-local budget mechanism the dialog carriers use, so a hostile
//! payload cannot overflow the decoding thread's stack. [`SceneBuilder`]
//! assembles scenes fluently and is never on the wire.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Maximum nesting depth accepted in a scene: root elements sit at depth 1
/// and each open group adds one. Real panel scenes nest a handful of groups
/// deep; a hostile payload that nests groups thousands deep would overflow
/// the decoding thread's stack (which aborts the host/runner process), so
/// both decoding and validation refuse past this cap. The cap is well above
/// every legitimate layout and far below any stack limit.
pub(crate) const MAX_SCENE_DEPTH: usize = 64;

/// Maximum total scene elements accepted in one panel scene (or per decoded
/// frame), bounding allocation and CPU magnification from shared sub-blocks.
/// Budget errors report the FIRST CROSSING (cap + 1), never the true total of
/// an oversized tree.
pub(crate) const MAX_SCENE_NODES: usize = 10_000;

/// Maximum byte length of a text run's `content`, and of any element's
/// optional display `name`, bounding the cost of a single hostile string.
pub(crate) const MAX_TEXT_BYTES: usize = 4_096;

/// A plugin-assigned stable identity for one scene element. Ids only need to
/// be unique within one [`PanelScene`]; the host uses them to keep element
/// identity stable across revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StableId(pub u64);

/// A scene revision: monotonically increasing per panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SceneRevision(pub u64);

/// RGBA color, one byte per channel (red, green, blue, alpha).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba8(pub [u8; 4]);

/// A point or size in scene coordinates, as an `[x, y]` pair.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec2(pub [f64; 2]);

/// An axis-aligned rectangle: top-left corner and extent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect2D {
    pub pos: Vec2,
    pub size: Vec2,
}

/// A theme-relative color token. The host maps each token onto its theme, so
/// plugins state intent, not colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeColor {
    /// Regular foreground painting.
    #[default]
    Normal,
    /// The theme's accent color.
    Accent,
    /// De-emphasised secondary painting.
    Muted,
    /// High-emphasis painting.
    Strong,
    /// A positive state, in the theme's success color.
    Success,
    /// A state that needs attention, in the theme's warning color.
    Warning,
    /// A failed state, in the theme's danger color.
    Danger,
}

/// Fill/stroke color: a theme token or an exact RGBA value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SceneColor {
    /// Paint with the theme's color for the token.
    Theme(ThemeColor),
    /// Paint with this exact color.
    Rgba(Rgba8),
}

impl Default for SceneColor {
    fn default() -> Self {
        SceneColor::Theme(ThemeColor::Normal)
    }
}

/// An outline stroke: a color and a width in scene units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: SceneColor,
    pub width: f32,
}

/// One element of a panel scene.
///
/// Every variant leads with the element's [`StableId`] and then the optional
/// display `name`. The variant order is the wire order: new variants append
/// at the end so the bincode discriminants of the existing variants stay
/// frozen (see the pinning test).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneElement {
    /// A container whose children render inside it, in order. A group
    /// carries no transform: child coordinates stay scene-space.
    Group {
        id: StableId,
        #[serde(default)]
        name: Option<String>,
        #[serde(deserialize_with = "budgeted_elements")]
        children: Vec<SceneElement>,
    },
    /// A filled and/or stroked rectangle.
    Rect {
        id: StableId,
        #[serde(default)]
        name: Option<String>,
        pos: Vec2,
        size: Vec2,
        #[serde(default)]
        fill: Option<SceneColor>,
        #[serde(default)]
        stroke: Option<Stroke>,
    },
    /// A polyline through scene coordinates, open or closed.
    Path {
        id: StableId,
        #[serde(default)]
        name: Option<String>,
        points: Vec<Vec2>,
        closed: bool,
        #[serde(default)]
        stroke: Option<Stroke>,
    },
    /// A single text run anchored at a scene position.
    Text {
        id: StableId,
        #[serde(default)]
        name: Option<String>,
        content: String,
        pos: Vec2,
        #[serde(default = "default_text_size")]
        size: f32,
        color: SceneColor,
    },
}

/// Why a scene failed validation. The vocabulary is structural only: it says
/// nothing about what a drawing means, only that its shapes are well-formed
/// and within budget.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneError {
    /// Two elements in one scene claimed the same stable id.
    DuplicateId(StableId),
    /// The element nesting passed its budget; `depth` is the first crossing
    /// (cap + 1), never the true total of an oversized tree.
    DepthBudgetExceeded { depth: usize, max: usize },
    /// The total element count passed its budget; `nodes` is the first
    /// crossing (cap + 1), never the true total.
    NodeBudgetExceeded { nodes: usize, max: usize },
    /// A text run or display name passed the byte budget; `bytes` is the
    /// first crossing (cap + 1).
    TextBudgetExceeded { bytes: usize, max: usize },
    /// A path carries fewer points than its shape requires.
    TooFewPoints,
    /// A rectangle's size is zero, negative, or non-finite.
    InvalidRect,
    /// A coordinate, size, or stroke width is not finite, or a stroke width
    /// is not strictly positive.
    NonFiniteCoordinate,
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneError::DuplicateId(id) => write!(f, "duplicate scene element id {}", id.0),
            SceneError::DepthBudgetExceeded { depth, max } => write!(
                f,
                "scene depth budget exceeded: nesting depth {depth} passes the cap {max}"
            ),
            SceneError::NodeBudgetExceeded { nodes, max } => write!(
                f,
                "scene node budget exceeded: {nodes} nodes pass the cap {max}"
            ),
            SceneError::TextBudgetExceeded { bytes, max } => write!(
                f,
                "scene text budget exceeded: {bytes} bytes pass the cap {max}"
            ),
            SceneError::TooFewPoints => {
                write!(f, "scene path carries fewer points than its shape requires")
            }
            SceneError::InvalidRect => write!(
                f,
                "scene rectangle size must be finite with both components greater than zero"
            ),
            SceneError::NonFiniteCoordinate => write!(
                f,
                "scene coordinate is not finite (NaN or float meta values are rejected)"
            ),
        }
    }
}

impl std::error::Error for SceneError {}

/// Decode-time default for a text run's font size when the field is missing
/// from a payload.
fn default_text_size() -> f32 {
    13.0
}

impl SceneElement {
    /// The element's plugin-assigned stable identity.
    pub fn stable_id(&self) -> StableId {
        match self {
            SceneElement::Group { id, .. }
            | SceneElement::Rect { id, .. }
            | SceneElement::Path { id, .. }
            | SceneElement::Text { id, .. } => *id,
        }
    }

    /// The element's optional display name (a shared accessory on every
    /// variant).
    pub fn name(&self) -> Option<&str> {
        match self {
            SceneElement::Group { name, .. }
            | SceneElement::Rect { name, .. }
            | SceneElement::Path { name, .. }
            | SceneElement::Text { name, .. } => name.as_deref(),
        }
    }
}

/// Per-thread scene decode budget. `depth` is the current recursion depth
/// (0 outside a decode); `nodes` counts total scene elements decoded within
/// the current frame (re-zeroed by [`scene_decode_frame_begin`] at each
/// frame boundary).
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
/// exceeds the nesting or node budget; the error aborts the decode before
/// the next level of recursion can consume stack.
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
/// and the decoded element total against the frame budget: a payload past
/// the budget refuses before the next level recurses, so the decoding stack
/// stays bounded. The wire layout is unchanged.
fn budgeted_elements<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SceneElement>, D::Error> {
    scene_decode_enter().map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = SceneDecodeGuard;
    let elements = Vec::<SceneElement>::deserialize(deserializer)?;
    scene_decode_add_nodes(elements.len()).map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(elements)
}

/// A complete snapshot of one panel's scene.
///
/// The plugin owns the semantic model (what the scene means and holds); the
/// host owns the projection into rendered geometry. A snapshot is the
/// ordered root element list — the scene carries no document reference of
/// its own, and children nest inside groups in scene coordinates.
///
/// Deserialization is bounded (see [`MAX_SCENE_DEPTH`]) exactly like the
/// dialog carriers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelScene {
    /// The scene's revision; raises monotonically per panel.
    pub revision: SceneRevision,
    /// Root elements in paint order (first painted first). Decoded under
    /// the scene decode budget.
    #[serde(deserialize_with = "budgeted_elements")]
    pub elements: Vec<SceneElement>,
}

impl PanelScene {
    /// Structural validation: unique ids across the whole tree, nesting
    /// within [`MAX_SCENE_DEPTH`], total element count within
    /// [`MAX_SCENE_NODES`], all floating-point payload finite, text and
    /// name byte caps, rectangle shapes, stroke widths, and minimum point
    /// counts for paths.
    ///
    /// This is deliberately structural only. Hit zones, semantic sense, and
    /// "does this drawing make sense" are NOT checked: those are
    /// host-projection concerns.
    pub fn validate(&self) -> Result<(), SceneError> {
        validate_root(&self.elements)
    }
}

/// Walk the whole tree and reject the first structural defect. The walk is
/// iterative over an explicit stack — never recursion — so nothing in this
/// module can overflow the validating thread's stack regardless of how the
/// scene was built.
fn validate_root(elements: &[SceneElement]) -> Result<(), SceneError> {
    let mut seen: HashSet<StableId> = HashSet::new();
    let mut nodes: usize = 0;
    // Root elements sit at depth 1; each nested group level adds one.
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
            SceneElement::Group { id, name, children } => {
                register_id(&mut seen, *id)?;
                require_name_budget(name)?;
                for child in children.iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
            SceneElement::Rect {
                id,
                name,
                pos,
                size,
                stroke,
                ..
            } => {
                register_id(&mut seen, *id)?;
                require_name_budget(name)?;
                require_finite(std::slice::from_ref(pos))?;
                validate_rect_size(size)?;
                validate_stroke(stroke)?;
            }
            SceneElement::Path {
                id,
                name,
                points,
                stroke,
                ..
            } => {
                register_id(&mut seen, *id)?;
                require_name_budget(name)?;
                if points.len() < 2 {
                    return Err(SceneError::TooFewPoints);
                }
                require_finite(points)?;
                validate_stroke(stroke)?;
            }
            SceneElement::Text {
                id,
                name,
                content,
                pos,
                size,
                ..
            } => {
                register_id(&mut seen, *id)?;
                require_name_budget(name)?;
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

/// Display names share the text byte budget.
fn require_name_budget(name: &Option<String>) -> Result<(), SceneError> {
    if let Some(name) = name {
        let bytes = name.len();
        if bytes > MAX_TEXT_BYTES {
            return Err(SceneError::TextBudgetExceeded {
                bytes,
                max: MAX_TEXT_BYTES,
            });
        }
    }
    Ok(())
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

// -- Scene builder (never on the wire) ---------------------------------------

/// Fluent builder for a [`PanelScene`]: typed adds append in paint order,
/// groups open and close a nesting context, and the paint setters re-point
/// at the ACTIVE element (the most recently added one), following the
/// accessory convention — a setter that the active variant does not take is
/// a documented no-op, never an error.
#[derive(Debug, Clone)]
pub struct SceneBuilder {
    revision: SceneRevision,
    /// Root elements in paint order.
    roots: Vec<SceneElement>,
    /// Index path of the innermost OPEN group, from the root list; each step
    /// indexes into a child list, and the last step names the open group
    /// within its parent. Empty = the root list is the insertion container.
    focus: Vec<usize>,
    /// Index path of the active element (most-recently-added), same shape;
    /// empty = no element has been added yet.
    active: Vec<usize>,
    /// Eager id ledger for the typed adds. `finish` re-checks everything
    /// structurally; this only turns mistakes into chain-time errors.
    taken: HashSet<StableId>,
}

impl SceneBuilder {
    /// Start a scene at the given revision (wrapped into [`SceneRevision`]).
    pub fn new(revision: u64) -> Self {
        Self {
            revision: SceneRevision(revision),
            roots: Vec::new(),
            focus: Vec::new(),
            active: Vec::new(),
            taken: HashSet::new(),
        }
    }

    /// Open a group: subsequent adds land inside it until
    /// [`SceneBuilder::end_group`]. The new group becomes the active element
    /// (so its `name` can be set fluently).
    pub fn group(&mut self, id: u64) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Group {
            id: StableId(id),
            name: None,
            children: Vec::new(),
        });
        // The just-pushed group is the new insertion container; its element
        // path (just computed into `active`) is the open-group path.
        self.focus = self.active.clone();
        Ok(self)
    }

    /// Close the innermost open group. A no-op when no group is open (the
    /// frozen [`SceneError`] vocabulary has no arm for unbalanced closes,
    /// and dropping the request silently keeps the builder infallible).
    pub fn end_group(&mut self) -> &mut Self {
        if !self.focus.is_empty() {
            // The closed group itself is the last child of the enclosing
            // container, i.e. the element named by the current focus path.
            self.active = self.focus.clone();
            self.focus.pop();
        }
        self
    }

    /// Append a rectangle (defaults `fill: None`, `stroke: None` — paint it
    /// fluently with [`SceneBuilder::fill`] / [`SceneBuilder::stroke`]).
    pub fn rect(
        &mut self,
        id: u64,
        pos: [f64; 2],
        size: [f64; 2],
    ) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Rect {
            id: StableId(id),
            name: None,
            pos: Vec2(pos),
            size: Vec2(size),
            fill: None,
            stroke: None,
        });
        Ok(self)
    }

    /// Append an open or closed polyline (defaults `stroke: None` — paint
    /// it fluently with [`SceneBuilder::stroke`]).
    pub fn path(
        &mut self,
        id: u64,
        points: Vec<Vec2>,
        closed: bool,
    ) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Path {
            id: StableId(id),
            name: None,
            points,
            closed,
            stroke: None,
        });
        Ok(self)
    }

    /// Append a single text run.
    pub fn text(
        &mut self,
        id: u64,
        content: &str,
        pos: [f64; 2],
        size: f32,
        color: SceneColor,
    ) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Text {
            id: StableId(id),
            name: None,
            content: content.to_string(),
            pos: Vec2(pos),
            size,
            color,
        });
        Ok(self)
    }

    /// Set the active element's display name (`None` clears it). No-op when
    /// no element has been added yet.
    pub fn name(&mut self, name: Option<&str>) -> &mut Self {
        if let Some(element) = self.active_element_mut() {
            match element {
                SceneElement::Group { name: slot, .. }
                | SceneElement::Rect { name: slot, .. }
                | SceneElement::Path { name: slot, .. }
                | SceneElement::Text { name: slot, .. } => *slot = name.map(str::to_owned),
            }
        }
        self
    }

    /// Set the active element's fill color (a paintable Rect). Follows the
    /// accessory convention — a documented no-op when no element has been
    /// added yet or the active element takes no fill (Group, Path, Text).
    pub fn fill(&mut self, color: SceneColor) -> &mut Self {
        if let Some(SceneElement::Rect { fill, .. }) = self.active_element_mut() {
            *fill = Some(color);
        }
        self
    }

    /// Set the active element's outline stroke (a paintable Rect or Path).
    /// Follows the accessory convention — a documented no-op when no element
    /// has been added yet or the active element takes no stroke (Group,
    /// Text).
    pub fn stroke(&mut self, stroke: Stroke) -> &mut Self {
        if let Some(
            SceneElement::Rect { stroke: line, .. } | SceneElement::Path { stroke: line, .. },
        ) = self.active_element_mut()
        {
            *line = Some(stroke);
        }
        self
    }

    /// Set the active Text's color (the variant that carries a `color` set
    /// mandatorily at add time; this is the re-theming path that spares a
    /// whole-literal rebuild). Follows the accessory convention — a
    /// documented no-op when no element has been added yet or the active
    /// element carries no color (Group, Rect, Path).
    pub fn color(&mut self, color: SceneColor) -> &mut Self {
        if let Some(SceneElement::Text { color: slot, .. }) = self.active_element_mut() {
            *slot = color;
        }
        self
    }

    /// Finish the scene: runs [`PanelScene::validate`] on the assembled
    /// snapshot and returns it. Validation errors come back as
    /// [`SceneError`]s — never swallowed, never panicking — and consume the
    /// staged elements either way: the builder is drained and reset for
    /// reuse (a `finish` on an exhausted builder produces an empty scene).
    pub fn finish(&mut self) -> Result<PanelScene, SceneError> {
        let scene = PanelScene {
            revision: self.revision,
            elements: std::mem::take(&mut self.roots),
        };
        self.focus.clear();
        self.active.clear();
        self.taken.clear();
        scene.validate()?;
        Ok(scene)
    }

    /// Record an id for the eager ledger, rejecting the first repeat.
    fn register_unique(&mut self, id: StableId) -> Result<(), SceneError> {
        if self.taken.insert(id) {
            Ok(())
        } else {
            Err(SceneError::DuplicateId(id))
        }
    }

    /// Append `element` into the current container and make it the active
    /// element. Only reachable through callers that already settled id
    /// uniqueness.
    fn push_active_child(&mut self, element: SceneElement) {
        let id = element.stable_id();
        let container = Self::container_children_mut(&mut self.roots, &self.focus)
            .expect("scene builder invariant: the open-group path names a live group");
        let index = container.len();
        container.push(element);
        self.active = self.focus.clone();
        self.active.push(index);
        // Ledger AFTER a successful insert so a rejected add never marks an
        // id as taken.
        self.taken.insert(id);
    }

    /// The active element, or `None` before the first add.
    fn active_element_mut(&mut self) -> Option<&mut SceneElement> {
        Self::element_at_mut(&mut self.roots, &self.active)
    }

    /// Descend `path` (each step indexing a child list) to the children vec
    /// of the group the path names; the empty path is `elements` itself.
    /// Returns `None` when a step is out of bounds or not a group — which
    /// cannot happen for paths produced by this builder (append-only child
    /// lists keep earlier indices stable).
    fn container_children_mut<'a>(
        elements: &'a mut Vec<SceneElement>,
        path: &[usize],
    ) -> Option<&'a mut Vec<SceneElement>> {
        let mut children = elements;
        for &index in path {
            let down = match children.get_mut(index) {
                Some(SceneElement::Group {
                    children: inner, ..
                }) => inner,
                _ => return None,
            };
            children = down;
        }
        Some(children)
    }

    /// The element named by `path` (a mutable view); `None` on a stale path.
    fn element_at_mut<'a>(
        elements: &'a mut Vec<SceneElement>,
        path: &[usize],
    ) -> Option<&'a mut SceneElement> {
        let (&last, prefix) = path.split_last()?;
        let container = Self::container_children_mut(elements, prefix)?;
        container.get_mut(last)
    }
}

/// Which surface kind a panel publish frame carries — the minimal common
/// surface envelope shared by scene and (later) widget surfaces: identity is
/// the transport-authenticated plugin plus the panel id, each surface kind
/// carries its own independent revision, and the wire never carries native
/// widget objects or callbacks. One frame publishes one surface.
///
/// Lives beside the scene tree (not in `ipc`) so the pure app-side store —
/// which compiles without the `host` feature — shares the exact vocabulary
/// the wire carries.
///
/// Compatibility rule: variants append at the END (the widget-surface
/// stage appends its kind); the scene leg's discriminant (0) is pinned from
/// creation and never renumbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PanelSurface {
    /// The panel's declarative scene snapshot (bounded, validated).
    Scene(PanelScene),
}

/// Why a panel surface publish refused. Codes append at the end; the
/// refusal never mutates host state (no partial writes, ever).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRejectCode {
    /// Nothing is attached for this panel: the receiving host has no
    /// surface-supporting attach, or the addressed panel is not a panel
    /// the authenticated plugin registered.
    Unattached,
    /// The panel's surface is owned by a different verified plugin.
    OwnedByOther,
    /// The surface is structurally invalid (failed its validation
    /// budgets); refused without any state change.
    InvalidSurface,
    /// The published revision is not a strict advance over the stored
    /// head and is not an identical idempotent resurface at that head.
    NonMonotonicRevision,
}

/// Typed rejection of a panel surface publish (or, later, patch): the
/// machine-readable code plus the stored head when the refusal is
/// revision-related, so the publisher can resynchronize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SurfaceReject {
    pub code: SurfaceRejectCode,
    /// The stored head when the refusal is revision-related; `None`
    /// otherwise.
    pub current_revision: Option<SceneRevision>,
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

    /// A minimal, defect-free rect with the given id.
    fn plain_rect(id: u64) -> SceneElement {
        SceneElement::Rect {
            id: StableId(id),
            name: None,
            pos: v2(0.0, 0.0),
            size: v2(1.0, 1.0),
            fill: None,
            stroke: None,
        }
    }

    /// A chain of `depth` nested groups (innermost one empty); the returned
    /// root list holds exactly one element, the outermost group.
    fn nested_group_chain(depth: usize) -> Vec<SceneElement> {
        let mut children: Vec<SceneElement> = Vec::new();
        for id in (1..=depth as u64).rev() {
            children = vec![SceneElement::Group {
                id: StableId(id),
                name: None,
                children: std::mem::take(&mut children),
            }];
        }
        children
    }

    /// `count` flat root rects with distinct ids.
    fn rect_fan(count: usize) -> Vec<SceneElement> {
        (1..=count as u64).map(plain_rect).collect()
    }

    /// The deterministic graph fixture: two labeled square nodes joined by
    /// one polyline edge, all inside a named group. Hand-built with fixed
    /// coordinates, so two calls must agree bit for bit.
    fn fixture_graph_scene() -> PanelScene {
        let node_a = [80.0, 120.0];
        let node_b = [280.0, 200.0];
        let node_size = [64.0, 32.0];
        let outline = |width: f32| Stroke {
            color: SceneColor::Theme(ThemeColor::Strong),
            width,
        };
        let mut builder = SceneBuilder::new(7);
        builder
            .group(1)
            .expect("open fixture group")
            .name(Some("fixture graph"));
        builder
            .rect(2, node_a, node_size)
            .expect("node a")
            .fill(SceneColor::Theme(ThemeColor::Accent))
            .stroke(outline(1.5));
        builder
            .text(
                3,
                "node a",
                node_a,
                default_text_size(),
                SceneColor::Theme(ThemeColor::Normal),
            )
            .expect("label a");
        builder
            .rect(4, node_b, node_size)
            .expect("node b")
            .stroke(outline(1.5));
        builder
            .text(
                5,
                "node b",
                node_b,
                default_text_size(),
                SceneColor::Theme(ThemeColor::Normal),
            )
            .expect("label b");
        builder
            .path(6, vec![Vec2(node_a), Vec2(node_b)], false)
            .expect("edge")
            .stroke(outline(1.0));
        builder.end_group();
        builder.finish().expect("fixture scene is valid")
    }

    /// A group chain built through the builder and closed again, with one
    /// rect at the bottom: `groups` nested groups put the rect at depth
    /// `groups + 1`.
    fn nested_chain_via_builder(groups: usize) -> Result<PanelScene, SceneError> {
        let mut builder = SceneBuilder::new(1);
        for id in 1..=groups as u64 {
            builder.group(id)?;
        }
        builder.rect(groups as u64 + 1, [0.0, 0.0], [10.0, 10.0])?;
        for _ in 0..groups {
            builder.end_group();
        }
        builder.finish()
    }

    #[test]
    fn scene_bincode_and_json_roundtrips_representative_tree() {
        let scene = PanelScene {
            revision: SceneRevision(3),
            elements: vec![
                SceneElement::Group {
                    id: sid(1),
                    name: Some("frame".to_string()),
                    children: vec![
                        SceneElement::Rect {
                            id: sid(2),
                            name: None,
                            pos: v2(10.0, 20.0),
                            size: v2(30.0, 40.0),
                            fill: Some(SceneColor::Theme(ThemeColor::Accent)),
                            stroke: Some(Stroke {
                                color: SceneColor::Rgba(Rgba8([0, 128, 255, 255])),
                                width: 2.0,
                            }),
                        },
                        SceneElement::Path {
                            id: sid(3),
                            name: None,
                            points: vec![v2(0.0, 0.0), v2(10.0, 0.0), v2(10.0, 10.0)],
                            closed: true,
                            stroke: Some(Stroke {
                                color: SceneColor::Theme(ThemeColor::Strong),
                                width: 1.0,
                            }),
                        },
                    ],
                },
                SceneElement::Text {
                    id: sid(4),
                    name: Some("caption".to_string()),
                    content: "hello".to_string(),
                    pos: v2(5.0, 6.0),
                    size: 13.0,
                    color: SceneColor::Theme(ThemeColor::Normal),
                },
            ],
        };
        scene.validate().expect("representative scene is valid");

        let wire = bincode::serialize(&scene).expect("bincode serialize");
        let decoded: PanelScene = bincode::deserialize(&wire).expect("bincode decode");
        assert_eq!(decoded, scene);

        let json = serde_json::to_string(&scene).expect("json serialize");
        let from_json: PanelScene = serde_json::from_str(&json).expect("json decode");
        assert_eq!(from_json, scene);
        // Externally tagged: PascalCase variant keys outside.
        assert!(json.contains("\"Group\""), "unexpected json: {json}");

        // Missing `name` and Text `size` fall back to their decode defaults.
        let minimal: PanelScene = serde_json::from_str(
            "{\"revision\":1,\"elements\":[{\"Text\":{\"id\":9,\"content\":\"hi\",\
             \"pos\":[0.0,0.0],\"color\":{\"Theme\":\"normal\"}}}]}",
        )
        .expect("minimal text decodes with defaults");
        let [SceneElement::Text { size, name, .. }] = minimal.elements.as_slice() else {
            panic!("one text element");
        };
        assert_eq!(*size, default_text_size());
        assert_eq!(*name, None);
    }

    #[test]
    fn scene_element_bincode_discriminants_are_pinned() {
        // The append-only wire baseline: Group=0, Rect=1, Path=2, Text=3.
        // New variants append AFTER Text and never renumber these.
        let tag = |element: &SceneElement| -> u32 {
            let bytes = bincode::serialize(element).expect("serialize scene element");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };

        let cases = [
            (
                0u32,
                SceneElement::Group {
                    id: sid(1),
                    name: None,
                    children: Vec::new(),
                },
            ),
            (
                1,
                SceneElement::Rect {
                    id: sid(2),
                    name: None,
                    pos: v2(0.0, 0.0),
                    size: v2(1.0, 1.0),
                    fill: None,
                    stroke: None,
                },
            ),
            (
                2,
                SceneElement::Path {
                    id: sid(3),
                    name: None,
                    points: vec![v2(0.0, 0.0), v2(1.0, 1.0)],
                    closed: false,
                    stroke: None,
                },
            ),
            (
                3,
                SceneElement::Text {
                    id: sid(4),
                    name: None,
                    content: "x".to_string(),
                    pos: v2(0.0, 0.0),
                    size: default_text_size(),
                    color: SceneColor::Theme(ThemeColor::Normal),
                },
            ),
        ];
        for (expected, element) in cases {
            assert_eq!(tag(&element), expected);
        }
    }

    #[test]
    fn duplicate_ids_are_rejected_by_builder_and_validate() {
        // Builder: eager rejection at add time, before finish.
        let mut builder = SceneBuilder::new(1);
        builder.rect(7, [0.0, 0.0], [1.0, 1.0]).expect("first rect");
        let error = builder
            .rect(7, [2.0, 2.0], [1.0, 1.0])
            .expect_err("duplicate id must refuse at add time");
        assert_eq!(error, SceneError::DuplicateId(StableId(7)));

        // Across containers too: a closed sibling's id stays taken.
        let mut builder = SceneBuilder::new(1);
        builder
            .group(1)
            .expect("group")
            .rect(2, [0.0, 0.0], [1.0, 1.0])
            .expect("child rect");
        builder.end_group();
        let error = builder
            .rect(2, [3.0, 3.0], [1.0, 1.0])
            .expect_err("cross-container duplicate must refuse");
        assert_eq!(error, SceneError::DuplicateId(StableId(2)));

        // validate() re-checks whole-tree uniqueness for hand-built scenes.
        let scene = PanelScene {
            revision: SceneRevision(1),
            elements: vec![
                SceneElement::Group {
                    id: sid(1),
                    name: None,
                    children: vec![plain_rect(3)],
                },
                plain_rect(3),
            ],
        };
        assert_eq!(scene.validate(), Err(SceneError::DuplicateId(StableId(3))));
    }

    #[test]
    fn depth_and_node_budgets_reject_oversized_trees() {
        // Within budget: the deepest element sits at depth 64 exactly.
        let scene = PanelScene {
            revision: SceneRevision(1),
            elements: nested_group_chain(MAX_SCENE_DEPTH),
        };
        assert_eq!(scene.validate(), Ok(()));

        // 65-deep nesting trips the depth budget with the first crossing.
        let scene = PanelScene {
            revision: SceneRevision(1),
            elements: nested_group_chain(MAX_SCENE_DEPTH + 1),
        };
        let error = scene.validate().expect_err("65-deep nesting must refuse");
        assert!(matches!(
            &error,
            SceneError::DepthBudgetExceeded { depth, .. } if *depth == MAX_SCENE_DEPTH + 1
        ));
        assert!(error.to_string().contains("depth budget"));

        // 10_000 root elements stay inside the node budget; one more trips
        // it with the first crossing.
        let scene = PanelScene {
            revision: SceneRevision(1),
            elements: rect_fan(MAX_SCENE_NODES),
        };
        assert_eq!(scene.validate(), Ok(()));
        let scene = PanelScene {
            revision: SceneRevision(1),
            elements: rect_fan(MAX_SCENE_NODES + 1),
        };
        let error = scene.validate().expect_err("10_001 nodes must refuse");
        assert!(matches!(
            &error,
            SceneError::NodeBudgetExceeded { nodes, .. } if *nodes == MAX_SCENE_NODES + 1
        ));
        assert!(error.to_string().contains("node budget"));
    }

    #[test]
    fn text_byte_cap_rejects_first_crossing_and_caps_name() {
        let at_cap = "a".repeat(MAX_TEXT_BYTES);
        let over_cap = "a".repeat(MAX_TEXT_BYTES + 1);
        let text_scene = |content: &str, name: Option<&str>| PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Text {
                id: sid(1),
                name: name.map(str::to_owned),
                content: content.to_string(),
                pos: v2(0.0, 0.0),
                size: default_text_size(),
                color: SceneColor::Theme(ThemeColor::Normal),
            }],
        };

        // Cap-exact content validates; one byte over refuses at cap + 1.
        assert_eq!(text_scene(&at_cap, None).validate(), Ok(()));
        let error = text_scene(&over_cap, None)
            .validate()
            .expect_err("over-cap text must refuse");
        assert!(matches!(
            &error,
            SceneError::TextBudgetExceeded { bytes, .. } if *bytes == MAX_TEXT_BYTES + 1
        ));
        assert!(error.to_string().contains("text budget"));

        // Display names share the same byte budget.
        assert_eq!(text_scene("hi", Some(at_cap.as_str())).validate(), Ok(()));
        assert_eq!(
            text_scene("hi", Some(over_cap.as_str())).validate(),
            Err(SceneError::TextBudgetExceeded {
                bytes: MAX_TEXT_BYTES + 1,
                max: MAX_TEXT_BYTES,
            })
        );
    }

    #[test]
    fn non_finite_coordinates_and_stroke_widths_are_rejected() {
        let nan_text_pos = PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Text {
                id: sid(1),
                name: None,
                content: "x".to_string(),
                pos: v2(f64::NAN, 0.0),
                size: default_text_size(),
                color: SceneColor::Theme(ThemeColor::Normal),
            }],
        };
        assert_eq!(
            nan_text_pos.validate(),
            Err(SceneError::NonFiniteCoordinate)
        );

        let infinite_path_point = PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Path {
                id: sid(1),
                name: None,
                points: vec![v2(0.0, 0.0), v2(f64::INFINITY, 1.0)],
                closed: false,
                stroke: None,
            }],
        };
        assert_eq!(
            infinite_path_point.validate(),
            Err(SceneError::NonFiniteCoordinate)
        );

        let nan_text_size = PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Text {
                id: sid(1),
                name: None,
                content: "x".to_string(),
                pos: v2(0.0, 0.0),
                size: f32::NAN,
                color: SceneColor::Theme(ThemeColor::Normal),
            }],
        };
        assert_eq!(
            nan_text_size.validate(),
            Err(SceneError::NonFiniteCoordinate)
        );

        let stroked = |width: f32| SceneElement::Rect {
            id: sid(1),
            name: None,
            pos: v2(0.0, 0.0),
            size: v2(1.0, 1.0),
            fill: None,
            stroke: Some(Stroke {
                color: SceneColor::Theme(ThemeColor::Strong),
                width,
            }),
        };
        let stroked_scene = |element: SceneElement| PanelScene {
            revision: SceneRevision(1),
            elements: vec![element],
        };
        assert_eq!(
            stroked_scene(stroked(f32::NAN)).validate(),
            Err(SceneError::NonFiniteCoordinate)
        );
        // Widths must be strictly positive too, not merely finite.
        assert_eq!(
            stroked_scene(stroked(0.0)).validate(),
            Err(SceneError::NonFiniteCoordinate)
        );
    }

    #[test]
    fn paths_with_fewer_than_two_points_are_rejected() {
        let empty_scene = PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Path {
                id: sid(1),
                name: None,
                points: Vec::new(),
                closed: false,
                stroke: None,
            }],
        };
        assert_eq!(empty_scene.validate(), Err(SceneError::TooFewPoints));

        let one_point_scene = PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Path {
                id: sid(1),
                name: None,
                points: vec![v2(0.0, 0.0)],
                closed: true,
                stroke: None,
            }],
        };
        assert_eq!(one_point_scene.validate(), Err(SceneError::TooFewPoints));
    }

    #[test]
    fn invalid_rect_sizes_are_rejected() {
        let rect = |sx: f64, sy: f64| PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Rect {
                id: sid(1),
                name: None,
                pos: v2(0.0, 0.0),
                size: v2(sx, sy),
                fill: None,
                stroke: None,
            }],
        };
        assert_eq!(rect(0.0, 10.0).validate(), Err(SceneError::InvalidRect));
        assert_eq!(rect(10.0, -1.0).validate(), Err(SceneError::InvalidRect));
        assert_eq!(
            rect(10.0, f64::NAN).validate(),
            Err(SceneError::InvalidRect)
        );

        // A NaN POSITION is a finiteness defect, not a shape defect.
        let nan_pos = PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Rect {
                id: sid(1),
                name: None,
                pos: v2(f64::NAN, 0.0),
                size: v2(1.0, 1.0),
                fill: None,
                stroke: None,
            }],
        };
        assert_eq!(nan_pos.validate(), Err(SceneError::NonFiniteCoordinate));
    }

    #[test]
    fn theme_color_json_is_lowercase() {
        assert_eq!(
            serde_json::to_string(&ThemeColor::Normal).expect("serialize Normal"),
            "\"normal\""
        );
        assert_eq!(
            serde_json::to_string(&ThemeColor::Danger).expect("serialize Danger"),
            "\"danger\""
        );
        let decoded: ThemeColor = serde_json::from_str("\"accent\"").expect("decode token");
        assert_eq!(decoded, ThemeColor::Accent);

        // SceneColor stays externally tagged with PascalCase keys.
        assert_eq!(
            serde_json::to_string(&SceneColor::Rgba(Rgba8([255, 0, 0, 255])))
                .expect("serialize color"),
            "{\"Rgba\":[255,0,0,255]}"
        );
        let color: SceneColor =
            serde_json::from_str("{\"Theme\":\"warning\"}").expect("decode color");
        assert_eq!(color, SceneColor::Theme(ThemeColor::Warning));
    }

    #[test]
    fn builders_produce_valid_scenes_at_every_grammar_variant() {
        // Group-only scene.
        let mut builder = SceneBuilder::new(1);
        builder.group(1).expect("group").end_group();
        builder.finish().expect("group-only scene is valid");

        // Painted rect.
        let mut builder = SceneBuilder::new(1);
        builder
            .rect(1, [0.0, 0.0], [10.0, 10.0])
            .expect("rect")
            .fill(SceneColor::Theme(ThemeColor::Accent))
            .stroke(Stroke {
                color: SceneColor::Theme(ThemeColor::Strong),
                width: 2.0,
            });
        builder.finish().expect("painted rect scene is valid");

        // Stroked polyline.
        let mut builder = SceneBuilder::new(1);
        builder
            .path(1, vec![v2(0.0, 0.0), v2(5.0, 5.0), v2(10.0, 0.0)], true)
            .expect("path")
            .stroke(Stroke {
                color: SceneColor::Theme(ThemeColor::Muted),
                width: 1.0,
            });
        builder.finish().expect("stroked path scene is valid");

        // Text run.
        let mut builder = SceneBuilder::new(1);
        builder
            .text(
                1,
                "label",
                [1.0, 2.0],
                default_text_size(),
                SceneColor::Theme(ThemeColor::Normal),
            )
            .expect("text");
        builder.finish().expect("text scene is valid");

        // Everything nested in one group.
        let mut builder = SceneBuilder::new(1);
        builder.group(1).expect("group");
        builder.rect(2, [0.0, 0.0], [1.0, 1.0]).expect("rect");
        builder
            .text(
                3,
                "in group",
                [2.0, 2.0],
                default_text_size(),
                SceneColor::Theme(ThemeColor::Normal),
            )
            .expect("text");
        builder
            .path(4, vec![v2(0.0, 0.0), v2(1.0, 1.0)], false)
            .expect("path");
        builder.end_group();
        let scene = builder.finish().expect("nested scene is valid");
        assert_eq!(scene.elements[0].stable_id(), StableId(1));

        // A builder is drained by finish: a second finish yields an empty,
        // still-valid scene at the same revision.
        let drained = builder.finish().expect("drained builder finish is valid");
        assert!(drained.elements.is_empty());
        assert_eq!(drained.revision, SceneRevision(1));
    }

    #[test]
    fn builder_paint_setters_match_the_literal() {
        let mut builder = SceneBuilder::new(1);
        builder
            .rect(7, [1.0, 2.0], [3.0, 4.0])
            .expect("rect")
            .name(Some("panel"))
            .fill(SceneColor::Theme(ThemeColor::Accent))
            .stroke(Stroke {
                color: SceneColor::Rgba(Rgba8([0, 0, 0, 255])),
                width: 1.5,
            });
        let scene = builder.finish().expect("rect scene is valid");
        assert_eq!(
            scene.elements,
            vec![SceneElement::Rect {
                id: StableId(7),
                name: Some("panel".to_string()),
                pos: Vec2([1.0, 2.0]),
                size: Vec2([3.0, 4.0]),
                fill: Some(SceneColor::Theme(ThemeColor::Accent)),
                stroke: Some(Stroke {
                    color: SceneColor::Rgba(Rgba8([0, 0, 0, 255])),
                    width: 1.5,
                }),
            }]
        );

        // The text color setter re-themes the active Text only.
        let mut builder = SceneBuilder::new(1);
        builder
            .text(
                1,
                "hi",
                [0.0, 0.0],
                default_text_size(),
                SceneColor::Theme(ThemeColor::Normal),
            )
            .expect("text")
            .color(SceneColor::Theme(ThemeColor::Danger))
            .name(Some("caption"));
        let scene = builder.finish().expect("text scene is valid");
        assert_eq!(
            scene.elements,
            vec![SceneElement::Text {
                id: StableId(1),
                name: Some("caption".to_string()),
                content: "hi".to_string(),
                pos: Vec2([0.0, 0.0]),
                size: default_text_size(),
                color: SceneColor::Theme(ThemeColor::Danger),
            }]
        );

        // fill on a freshly opened group is a documented no-op.
        let mut builder = SceneBuilder::new(1);
        builder
            .group(1)
            .expect("group")
            .fill(SceneColor::Theme(ThemeColor::Danger));
        let scene = builder.finish().expect("group scene is valid");
        assert_eq!(
            scene.elements,
            vec![SceneElement::Group {
                id: StableId(1),
                name: None,
                children: Vec::new(),
            }]
        );
    }

    #[test]
    fn fixture_graph_scene_is_deterministic_and_valid() {
        let first = fixture_graph_scene();
        let second = fixture_graph_scene();
        assert_eq!(first, second);
        assert_eq!(
            bincode::serialize(&first).expect("serialize fixture"),
            bincode::serialize(&second).expect("serialize fixture")
        );
        first.validate().expect("fixture validates");
        // The fixture shape: one named group holding both nodes, both labels,
        // and the connecting edge.
        assert_eq!(first.revision, SceneRevision(7));
        assert_eq!(first.elements.len(), 1);
        let [SceneElement::Group { name, children, .. }] = first.elements.as_slice() else {
            panic!("fixture root is one group");
        };
        assert_eq!(*name, Some("fixture graph".to_string()));
        assert_eq!(children.len(), 5);
    }

    #[test]
    fn fixture_nested_scene_depth_boundary() {
        // 63 nested groups put the rect at depth 64 exactly: inside the cap.
        assert!(nested_chain_via_builder(MAX_SCENE_DEPTH - 1).is_ok());
        // One more group puts the rect at depth 65: the first crossing is
        // reported, not the true total.
        assert_eq!(
            nested_chain_via_builder(MAX_SCENE_DEPTH),
            Err(SceneError::DepthBudgetExceeded {
                depth: MAX_SCENE_DEPTH + 1,
                max: MAX_SCENE_DEPTH,
            })
        );
    }

    #[test]
    fn scene_decode_rejects_nesting_past_budget() {
        // Within budget: the last decoded child list is level 64 exactly.
        scene_decode_frame_begin();
        let within = PanelScene {
            revision: SceneRevision(1),
            elements: nested_group_chain(MAX_SCENE_DEPTH - 1),
        };
        let wire = bincode::serialize(&within).expect("serialize within-budget scene");
        let decoded: PanelScene = bincode::deserialize(&wire).expect("chain at the budget decodes");
        assert_eq!(decoded, within);

        // One deeper chain refuses during deserialization, before the next
        // level can recurse.
        scene_decode_frame_begin();
        let over = PanelScene {
            revision: SceneRevision(1),
            elements: nested_group_chain(MAX_SCENE_DEPTH),
        };
        let wire = bincode::serialize(&over).expect("serialize over-budget scene");
        let error = bincode::deserialize::<PanelScene>(&wire)
            .expect_err("past-budget chain refuses to decode");
        assert!(
            error.to_string().contains("depth budget"),
            "unexpected: {error}"
        );
    }

    #[test]
    fn scene_decode_rejects_node_budget_overflow() {
        // Within budget: the frame-begin charge for the root list plus
        // 9_999 rects lands on 10_000 exactly.
        scene_decode_frame_begin();
        let within = PanelScene {
            revision: SceneRevision(1),
            elements: rect_fan(MAX_SCENE_NODES - 1),
        };
        let wire = bincode::serialize(&within).expect("serialize within-budget scene");
        let decoded: PanelScene = bincode::deserialize(&wire).expect("fan at the budget decodes");
        assert_eq!(decoded, within);

        scene_decode_frame_begin();
        let over = PanelScene {
            revision: SceneRevision(1),
            elements: rect_fan(MAX_SCENE_NODES),
        };
        let wire = bincode::serialize(&over).expect("serialize over-budget scene");
        let error = bincode::deserialize::<PanelScene>(&wire)
            .expect_err("past-budget fan refuses to decode");
        assert!(
            error.to_string().contains("node budget"),
            "unexpected: {error}"
        );
    }
}
