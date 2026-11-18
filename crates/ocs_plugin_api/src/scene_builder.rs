//! Plugin-side scene construction helpers and deterministic fixtures layered
//! over the frozen scene vocabulary in [`crate::scene`].
//!
//! Two layers, one rule: nothing here is on the wire.
//!
//! - [`SceneBuilder`] and [`PatchBuilder`] are PLUGIN-side conveniences. The
//!   wire types ([`PanelScene`], [`PanelScenePatch`], [`SceneOp`],
//!   [`SceneElement`], …) remain the frozen vocabulary this crate ships; the
//!   builders only assemble them. A scene produced by [`SceneBuilder::finish`]
//!   is structurally identical to one written literally and has run
//!   [`PanelScene::validate`] — a scene the validator rejects is returned as
//!   an [`Err`], never swallowed and never panicking.
//!
//! - [`fixture_graph_scene`], [`try_fixture_graph_scene`], and
//!   [`fixture_scene_with_files`] are deterministic fixtures for tests,
//!   benches, and host-side projection tests. They are NOT runtime data.
//!   Stability contract: the same arguments produce a byte-identical scene —
//!   no randomness (only an `Lcg` with published constants and a documented
//!   draw order, see below), no environment reads, and no clock.
//!
//! # The "active element" pattern
//!
//! Fluent chains need somewhere to hang per-element accessories without a
//! growing parameter list. [`SceneBuilder`] therefore tracks the
//! **most-recently-added element** as the *active element*:
//! [`SceneBuilder::name`], [`SceneBuilder::actions`],
//! [`SceneBuilder::drag`], and the paint setters [`SceneBuilder::fill`] /
//! [`SceneBuilder::stroke`] apply to it and to nothing else. A subsequent
//! add re-points the active element at the new element; closing a group with
//! [`SceneBuilder::end_group`] re-points it at the just-closed group. Calling
//! an accessor before any element has been added is a documented no-op (the
//! error vocabulary of [`SceneError`] is frozen, so there is nothing to
//! report it with; the typed adds are the paths that report). The paint
//! setters follow the same convention on elements that take no such paint
//! (a `fill` on a Group, a `stroke` on a Text): documented no-ops, never
//! errors.
//!
//! # Group nesting
//!
//! [`SceneBuilder::group`] opens a group: every element added afterwards
//! lands inside it until [`SceneBuilder::end_group`] closes it. `end_group`
//! past the bottom of the stack is a no-op. Groups nest to
//! [`crate::scene::MAX_SCENE_DEPTH`], which [`SceneBuilder::finish`] enforces
//! through the validator.
//!
//! # Ids
//!
//! Raw ids are plain `u64` throughout the builders for ergonomics; they are
//! wrapped in [`StableId`] at the wire-type boundary. The typed add methods
//! reject an id that is already live in the under-construction scene
//! *eagerly* (as a chained [`SceneError::DuplicateId`]). The authoritative
//! gate is still [`SceneBuilder::finish`]; [`SceneBuilder::element`] is the
//! deliberate escape hatch that inserts pre-built elements without the eager
//! id scan so exotic literals surface their own defects at `finish` time.

use crate::scene::{
    AssetKind, CubicSegment, DragMeta, MarkerKind, MAX_PATCH_OPS, MAX_SCENE_DEPTH,
    MAX_SCENE_NODES, PanelScene, PanelScenePatch, Rect2D, SceneColor, SceneElement, SceneError,
    SceneOp, SceneRevision, StableId, Stroke, ThemeColor, Vec2,
};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// SceneBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for a [`PanelScene`] (see the [module docs](self) for the
/// active-element and nesting conventions).
///
/// Every typed add method appends into the innermost open group (the scene
/// root before the first [`SceneBuilder::group`]) and returns
/// `Ok(&mut Self)` so calls chain; the `Err` arm carries
/// [`SceneError::DuplicateId`] when the id is already live. Positional
/// parameters take `[x, y]` arrays and are wrapped into [`Vec2`] here.
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
    /// Eager id ledger for the typed adds (and raw inserts). `finish`
    /// re-checks everything structurally; this only turns mistakes into
    /// chain-time errors.
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
    /// [`SceneBuilder::end_group`]. The new group becomes the active element.
    pub fn group(&mut self, id: u64) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Group {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            children: Vec::new(),
            clip: None,
        });
        // The just-pushed group is the new insertion container; its element
        // path (just computed into `active`) is the open-group path.
        self.focus = self.active.clone();
        Ok(self)
    }

    /// Close the innermost open group. A no-op when no group is open (the
    /// frozen [`SceneError`] vocabulary has no arm for unbalanced closes, and
    /// dropping the request silently keeps the builder infallible).
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
            actions: Vec::new(),
            drag: None,
            pos: Vec2(pos),
            size: Vec2(size),
            fill: None,
            stroke: None,
        });
        Ok(self)
    }

    /// Append an open or closed polyline (defaults `fill: None`,
    /// `stroke: None` — paint it fluently with [`SceneBuilder::fill`] /
    /// [`SceneBuilder::stroke`]).
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
            actions: Vec::new(),
            drag: None,
            points,
            closed,
            stroke: None,
            fill: None,
        });
        Ok(self)
    }

    /// Append an open or closed cubic Bézier chain (defaults `fill: None`,
    /// `stroke: None` — paint it fluently with [`SceneBuilder::fill`] /
    /// [`SceneBuilder::stroke`]).
    pub fn cubic(
        &mut self,
        id: u64,
        segments: Vec<CubicSegment>,
        closed: bool,
    ) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::CubicPath {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            segments,
            closed,
            stroke: None,
            fill: None,
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
            actions: Vec::new(),
            drag: None,
            content: content.to_string(),
            pos: Vec2(pos),
            size,
            color,
        });
        Ok(self)
    }

    /// Append a host-loaded asset rendered into `frame`.
    pub fn asset(
        &mut self,
        id: u64,
        kind: AssetKind,
        uri: &str,
        frame: Rect2D,
    ) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Asset {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            kind,
            uri: uri.to_string(),
            frame,
        });
        Ok(self)
    }

    /// Append a named connection point (its `label` stays unset; label it
    /// with a [`SceneBuilder::text`] sibling or insert it raw via
    /// [`SceneBuilder::element`]).
    pub fn port(&mut self, id: u64, at: [f64; 2]) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Port {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            at: Vec2(at),
            label: None,
        });
        Ok(self)
    }

    /// Append a standalone annotation shape.
    pub fn marker(
        &mut self,
        id: u64,
        at: [f64; 2],
        kind: MarkerKind,
        color: SceneColor,
    ) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Marker {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            at: Vec2(at),
            kind,
            color,
        });
        Ok(self)
    }

    /// Append a route between two element ids. Endpoint existence is NOT
    /// checked (matching the wire vocabulary); the ids are the same `u64`
    /// values given to earlier adds. The route defaults `stroke: None`
    /// (legally invisible); stroke it fluently with
    /// [`SceneBuilder::stroke`].
    pub fn edge(
        &mut self,
        id: u64,
        a: u64,
        b: u64,
        route: Vec<Vec2>,
    ) -> Result<&mut Self, SceneError> {
        self.register_unique(StableId(id))?;
        self.push_active_child(SceneElement::Edge {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            a: StableId(a),
            b: StableId(b),
            route,
            stroke: None,
            start_marker: None,
            end_marker: None,
        });
        Ok(self)
    }

    /// Escape hatch: insert a fully constructed element (a composite group
    /// with its children already set, say) into the current container as a
    /// CLOSED subtree — the group stack is not extended into it. Unlike the
    /// typed adds this performs NO eager id scan; the inserted subtree's ids
    /// are still ledgered, so later typed adds collide with them, and
    /// defects inside the subtree surface from [`SceneBuilder::finish`].
    pub fn element(&mut self, element: SceneElement) -> &mut Self {
        self.ledger_subtree_ids(&element);
        self.push_active_child(element);
        self
    }

    /// Set the active element's display name (`None` clears it). No-op when
    /// no element has been added yet.
    pub fn name(&mut self, name: Option<&str>) -> &mut Self {
        if let Some(accessories) = self.active_element_mut().and_then(accessories_mut) {
            *accessories.0 = name.map(str::to_owned);
        }
        self
    }

    /// Replace the active element's action verbs. No-op when no element has
    /// been added yet.
    pub fn actions(&mut self, actions: &[&str]) -> &mut Self {
        if let Some(accessories) = self.active_element_mut().and_then(accessories_mut) {
            *accessories.1 = actions.iter().map(|verb| verb.to_string()).collect();
        }
        self
    }

    /// Set the active element's drag context (`None` clears it). No-op when
    /// no element has been added yet.
    pub fn drag(&mut self, drag: Option<DragMeta>) -> &mut Self {
        if let Some(accessories) = self.active_element_mut().and_then(accessories_mut) {
            *accessories.2 = drag;
        }
        self
    }

    /// Set the active element's fill color (a paintable Rect, Path, or
    /// CubicPath). Follows the accessory convention — a documented no-op
    /// when no element has been added yet or the active element takes no
    /// fill (Group, Text, Asset, Port, Marker, Edge).
    pub fn fill(&mut self, color: SceneColor) -> &mut Self {
        if let Some(fill) = self.active_element_mut().and_then(fill_mut) {
            *fill = Some(color);
        }
        self
    }

    /// Set the active element's outline stroke (a paintable Rect, Path, or
    /// CubicPath, or an Edge's route stroke). Follows the accessory
    /// convention — a documented no-op when no element has been added yet
    /// or the active element takes no stroke (Group, Text, Asset, Port,
    /// Marker).
    pub fn stroke(&mut self, stroke: Stroke) -> &mut Self {
        if let Some(line) = self.active_element_mut().and_then(stroke_mut) {
            *line = Some(stroke);
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

    /// Ledger every id in a subtree (the element's own id included) without
    /// any duplicate scan — the raw escape hatch's companion. Iterative walk
    /// over the public [`SceneElement::stable_id`] accessor only.
    fn ledger_subtree_ids(&mut self, element: &SceneElement) {
        let mut stack: Vec<&SceneElement> = vec![element];
        while let Some(next) = stack.pop() {
            self.taken.insert(next.stable_id());
            if let SceneElement::Group { children, .. } = next {
                stack.extend(children.iter());
            }
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
                Some(SceneElement::Group { children: inner, .. }) => inner,
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

/// Mutable view of the `name`/`actions`/`drag` accessory fields shared by
/// every [`SceneElement`] variant — the single place the "common layout"
/// assumption is spelled out element by element.
fn accessories_mut(
    element: &mut SceneElement,
) -> Option<(
    &mut Option<String>,
    &mut Vec<String>,
    &mut Option<DragMeta>,
)> {
    match element {
        SceneElement::Group { name, actions, drag, .. } => Some((name, actions, drag)),
        SceneElement::Rect { name, actions, drag, .. } => Some((name, actions, drag)),
        SceneElement::Path { name, actions, drag, .. } => Some((name, actions, drag)),
        SceneElement::CubicPath { name, actions, drag, .. } => Some((name, actions, drag)),
        SceneElement::Text { name, actions, drag, .. } => Some((name, actions, drag)),
        SceneElement::Asset { name, actions, drag, .. } => Some((name, actions, drag)),
        SceneElement::Port { name, actions, drag, .. } => Some((name, actions, drag)),
        SceneElement::Marker { name, actions, drag, .. } => Some((name, actions, drag)),
        SceneElement::Edge { name, actions, drag, .. } => Some((name, actions, drag)),
    }
}

/// Mutable view of the active element's `fill` field — the variants that
/// paint an enclosed area (Rect, Path, CubicPath). `None` on every other
/// variant (the [`SceneBuilder::fill`] no-op arm).
fn fill_mut(element: &mut SceneElement) -> Option<&mut Option<SceneColor>> {
    match element {
        SceneElement::Rect { fill, .. }
        | SceneElement::Path { fill, .. }
        | SceneElement::CubicPath { fill, .. } => Some(fill),
        _ => None,
    }
}

/// Mutable view of the active element's `stroke` field — the variants that
/// stroke a line or outline (Rect, Path, CubicPath, Edge). `None` on every
/// other variant (the [`SceneBuilder::stroke`] no-op arm).
fn stroke_mut(element: &mut SceneElement) -> Option<&mut Option<Stroke>> {
    match element {
        SceneElement::Rect { stroke, .. }
        | SceneElement::Path { stroke, .. }
        | SceneElement::CubicPath { stroke, .. }
        | SceneElement::Edge { stroke, .. } => Some(stroke),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// PatchBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for a [`PanelScenePatch`].
///
/// Raw ids are plain `u64` (wrapped into [`StableId`] here) and the op
/// methods chain; [`PatchBuilder::finish`] is the checkpoint: it validates
/// the op count against `MAX_PATCH_OPS` and the successor revision strictly
/// above the base BEFORE any caller can hand the patch to
/// [`PanelScene::apply_patch`]. Structural sense of the ops themselves (live
/// ids, live parents, resulting budgets) is the apply-side's job.
#[derive(Debug, Clone)]
pub struct PatchBuilder {
    base_revision: SceneRevision,
    ops: Vec<SceneOp>,
}

impl PatchBuilder {
    /// Start a patch against the given base revision.
    pub fn new(base_revision: u64) -> Self {
        Self {
            base_revision: SceneRevision(base_revision),
            ops: Vec::new(),
        }
    }

    /// Queue an [`SceneOp::Add`] (a `Some` parent appends into that group, a
    /// `None` appends at the scene root).
    pub fn add(&mut self, parent: Option<u64>, element: SceneElement) -> &mut Self {
        self.ops.push(SceneOp::Add {
            parent: parent.map(StableId),
            element,
        });
        self
    }

    /// Queue a [`SceneOp::Replace`] (keyed by `element`'s own id).
    pub fn replace(&mut self, element: SceneElement) -> &mut Self {
        self.ops.push(SceneOp::Replace { element });
        self
    }

    /// Queue a [`SceneOp::Remove`] for the subtree rooted at `id`.
    pub fn remove(&mut self, id: u64) -> &mut Self {
        self.ops.push(SceneOp::Remove { id: StableId(id) });
        self
    }

    /// Finish the patch at the given successor revision. Refuses an op list
    /// past `MAX_PATCH_OPS` and any successor that does not advance past the
    /// base ([`SceneError::NonMonotonicRevision`]). The queued ops are drained
    /// whether the patch is accepted or not: a follow-up `finish` builds a
    /// fresh (empty) patch, never a re-stamped stale one.
    pub fn finish(&mut self, revision: u64) -> Result<PanelScenePatch, SceneError> {
        // Take first, then validate: every arm — accepted or refused —
        // leaves the builder's queue empty.
        let ops = std::mem::take(&mut self.ops);
        if ops.len() > MAX_PATCH_OPS {
            return Err(SceneError::PatchOpBudgetExceeded {
                ops: ops.len(),
                max: MAX_PATCH_OPS,
            });
        }
        let successor = SceneRevision(revision);
        if successor <= self.base_revision {
            return Err(SceneError::NonMonotonicRevision {
                current: self.base_revision,
                given: successor,
            });
        }
        Ok(PanelScenePatch {
            base_revision: self.base_revision,
            revision: successor,
            ops,
        })
    }
}

// ---------------------------------------------------------------------------
// Deterministic fixtures
// ---------------------------------------------------------------------------

/// Ports the graph fixture gives every node card — one per side (N, E, S, W).
pub const FIXTURE_GRAPH_PORTS_PER_NODE: usize = 4;

/// Scene elements the graph fixture spends per node (counted for
/// `nodes ≥ 2`): one group + one card rect + 4 ports + one label + one
/// outgoing edge. `fixture_graph_scene_plan` spells out the whole math.
pub const FIXTURE_GRAPH_ELEMENTS_PER_NODE: usize = 8;

/// Id namespace stride per node: node `i` owns ids `i*10 ..= i*10+9`
/// (see [`try_fixture_graph_scene`] for the slot map); edges follow after
/// the last node's block.
pub const FIXTURE_GRAPH_ID_STRIDE_PER_NODE: u64 = 10;

/// Largest node count the graph fixture accepts before refusing
/// ([`try_fixture_graph_scene`]) or deterministically shrinking
/// ([`fixture_graph_scene`]): `MAX_SCENE_NODES / 8` = 1250 nodes fill the
/// node budget exactly (8 × 1250 = 10_000 elements).
pub const FIXTURE_GRAPH_MAX_NODES: usize = MAX_SCENE_NODES / FIXTURE_GRAPH_ELEMENTS_PER_NODE;

/// Deterministic pseudo-randomness for the graph fixture: a 64-bit linear
/// congruential generator with Knuth's MMIX constants
/// (`state ← state × 6364136223846793005 + 1442695040888963407`, wrapping).
/// Chosen because full multiplication gives every seed an independent
/// orbit from a single advance, it is four instructions per draw, and
/// wrap-around makes the sequence well-defined on every platform. Draws take
/// the raw 64-bit state modulo the requested bound; draw ORDER is part of
/// the fixture contract.
#[derive(Debug, Clone)]
struct Lcg(u64);

impl Lcg {
    const MULTIPLIER: u64 = 6_364_136_223_846_793_005;
    const INCREMENT: u64 = 14_426_950_408_889_634_07;

    fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(Self::INCREMENT);
        self.0
    }

    /// Draw in `0..bound` (a `bound` of 0 pins the draw to 0 so the
    /// generator stays total; callers never pass 0 in practice).
    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound.max(1) as u64) as usize
    }
}

/// Total scene element count the graph fixture emits for `nodes` nodes.
/// Documented multiplicity math for benches sizing counts:
///
/// - `nodes ≥ 2`: `8 × nodes` — each node spends one group + one card
///   rect + 4 ports + one label + exactly one outgoing edge
///   ([`FIXTURE_GRAPH_ELEMENTS_PER_NODE`]),
/// - `nodes == 1`: `7` — the lone node cannot host a cross-node edge,
/// - `nodes == 0`: `0` — the empty scene.
///
/// Counts saturate at `usize` rather than overflow. The fixture refuses
/// (preferred) or shrinks (convenience wrapper) anything past
/// [`FIXTURE_GRAPH_MAX_NODES`]; never emits a partial scene silently.
pub const fn fixture_graph_scene_plan(nodes: usize) -> usize {
    if nodes >= 2 {
        nodes.saturating_mul(FIXTURE_GRAPH_ELEMENTS_PER_NODE)
    } else if nodes == 1 {
        7
    } else {
        0
    }
}

/// Card/edge palette slots the fixture rotates deterministically.
const FIXTURE_GRAPH_COLUMNS: usize = 8;
const CARD_WIDTH: f64 = 120.0;
const CARD_HEIGHT: f64 = 48.0;
const COLUMN_GAP: f64 = 32.0;
const ROW_GAP: f64 = 40.0;

/// Grid anchor of node `i`'s card (deterministic from the index: `col =
/// i % COLS`, `row = i / COLS`, columns of [`FIXTURE_GRAPH_COLUMNS`]).
fn fixture_graph_node_anchor(i: usize) -> [f64; 2] {
    let col = i % FIXTURE_GRAPH_COLUMNS;
    let row = i / FIXTURE_GRAPH_COLUMNS;
    [
        col as f64 * (CARD_WIDTH + COLUMN_GAP),
        row as f64 * (CARD_HEIGHT + ROW_GAP),
    ]
}

/// Scene position of node `i`'s port on `side` (0 = N, 1 = E, 2 = S, 3 = W).
/// Pure over `(i, side)`; edge routes reuse it verbatim so routes touch the
/// ports they reference.
fn fixture_graph_port_pos(i: usize, side: usize) -> Vec2 {
    let [x, y] = fixture_graph_node_anchor(i);
    match side % FIXTURE_GRAPH_PORTS_PER_NODE {
        0 => Vec2([x + CARD_WIDTH / 2.0, y]),
        1 => Vec2([x + CARD_WIDTH, y + CARD_HEIGHT / 2.0]),
        2 => Vec2([x + CARD_WIDTH / 2.0, y + CARD_HEIGHT]),
        _ => Vec2([x, y + CARD_HEIGHT / 2.0]),
    }
}

/// Node `i`'s id slot for `role`: `0` = group, `1..=4` = ports (N, E, S, W),
/// `5` = card rect, `6` = label text, `7..=9` = reserved for future per-node
/// content (the stride buys them for free).
fn fixture_graph_node_id_slot(i: usize, role: u64) -> StableId {
    StableId(i as u64 * FIXTURE_GRAPH_ID_STRIDE_PER_NODE + role)
}

/// Node `i`'s subtree: a named group carrying the card rect, one port per
/// side, and the label text, all laid out on the fixture grid.
fn fixture_graph_node_subtree(i: usize) -> SceneElement {
    let [x, y] = fixture_graph_node_anchor(i);
    let fill = if i % 2 == 0 {
        SceneColor::Theme(ThemeColor::Accent)
    } else {
        SceneColor::Theme(ThemeColor::Muted)
    };
    let mut ports = Vec::with_capacity(FIXTURE_GRAPH_PORTS_PER_NODE);
    for side in 0..FIXTURE_GRAPH_PORTS_PER_NODE {
        ports.push(SceneElement::Port {
            id: fixture_graph_node_id_slot(i, 1 + side as u64),
            name: None,
            actions: Vec::new(),
            drag: None,
            at: fixture_graph_port_pos(i, side),
            label: None,
        });
    }
    let mut children = Vec::with_capacity(2 + FIXTURE_GRAPH_PORTS_PER_NODE);
    children.push(SceneElement::Rect {
        id: fixture_graph_node_id_slot(i, 5),
        name: None,
        actions: Vec::new(),
        drag: None,
        pos: Vec2([x, y]),
        size: Vec2([CARD_WIDTH, CARD_HEIGHT]),
        fill: Some(fill),
        stroke: Some(Stroke {
            color: SceneColor::Theme(ThemeColor::Strong),
            width: 1.0,
        }),
    });
    children.extend(ports);
    children.push(SceneElement::Text {
        id: fixture_graph_node_id_slot(i, 6),
        name: None,
        actions: Vec::new(),
        drag: None,
        content: format!("node-{i}"),
        pos: Vec2([x + 8.0, y + 30.0]),
        size: 13.0,
        color: SceneColor::Theme(ThemeColor::Strong),
    });
    SceneElement::Group {
        id: fixture_graph_node_id_slot(i, 0),
        name: Some(format!("node-{i}")),
        actions: Vec::new(),
        drag: None,
        children,
        clip: None,
    }
}

/// Deterministic graph fixture (checked form): `nodes` node-groups on a grid
/// (one card rect, one port per side, one text label each) plus one outgoing
/// edge per node running port-to-port as a 2-point route.
///
/// Edge selection is pseudo-random but fully reproducible: a single
/// [`Lcg`] stream seeded with `seed`, three draws per edge in a fixed order —
/// destination-node offset (uniform in `0..nodes-1`, skipping self), source
/// side, destination side. With `nodes ≥ 2` node `i`'s edge always targets a
/// DIFFERENT node; with `nodes < 2` no edge exists.
///
/// Id scheme (globally unique, see [`FIXTURE_GRAPH_ID_STRIDE_PER_NODE`]): node
/// `i` owns `i*10+0` (group), `i*10+1..=4` (ports), `i*10+5` (card),
/// `i*10+6` (label); edges own `nodes*10 + k` for edge `k` — so every edge
/// endpoint names a port that exists in the fixture.
///
/// Errors: `Err` when `nodes` would produce a scene past [`MAX_SCENE_NODES`]
/// ([`SceneError::NodeBudgetExceeded`], reported with the projected total —
/// the PREFER-Err decision for oversized counts). Calls
/// [`PanelScene::validate`] before returning and never returns an invalid
/// scene.
pub fn try_fixture_graph_scene(
    revision: u64,
    nodes: usize,
    seed: u64,
) -> Result<PanelScene, SceneError> {
    let planned = fixture_graph_scene_plan(nodes);
    if planned > MAX_SCENE_NODES {
        return Err(SceneError::NodeBudgetExceeded {
            nodes: planned,
            max: MAX_SCENE_NODES,
        });
    }

    let mut elements: Vec<SceneElement> = Vec::with_capacity(planned);
    for i in 0..nodes {
        elements.push(fixture_graph_node_subtree(i));
    }
    if nodes >= 2 {
        // One stream, seeded exactly once; three draws per edge in the
        // documented order (destination offset, source side, destination
        // side). Reordering the draws changes the fixture — pinned contract.
        let mut rng = Lcg::new(seed);
        for i in 0..nodes {
            let destination = (i + 1 + rng.below(nodes - 1)) % nodes;
            let source_side = rng.below(FIXTURE_GRAPH_PORTS_PER_NODE);
            let destination_side = rng.below(FIXTURE_GRAPH_PORTS_PER_NODE);
            let start = fixture_graph_port_pos(i, source_side);
            let end = fixture_graph_port_pos(destination, destination_side);
            elements.push(SceneElement::Edge {
                id: StableId(nodes as u64 * FIXTURE_GRAPH_ID_STRIDE_PER_NODE + i as u64),
                name: None,
                actions: Vec::new(),
                drag: None,
                a: fixture_graph_node_id_slot(i, 1 + source_side as u64),
                b: fixture_graph_node_id_slot(destination, 1 + destination_side as u64),
                route: vec![start, end],
                stroke: Some(Stroke {
                    color: SceneColor::Theme(ThemeColor::Muted),
                    width: 1.0,
                }),
                start_marker: Some(MarkerKind::Dot),
                end_marker: Some(MarkerKind::Arrow),
            });
        }
    }

    let scene = PanelScene {
        revision: SceneRevision(revision),
        elements,
    };
    scene.validate()?;
    Ok(scene)
}

/// Deterministic graph fixture (unchecked form, mandated signature): same
/// scene as [`try_fixture_graph_scene`], but oversized `nodes` are SHRUNK
/// deterministically to the largest count that fits the node budget
/// ([`FIXTURE_GRAPH_MAX_NODES`]) instead of erroring.
///
/// DECIDED and documented: the commanded return type (`PanelScene`) cannot
/// carry an error, and the fixture must never panic and never return an
/// invalid scene; between those, it prefers a deterministic, DOCUMENTED
/// shrink over a silent one — the shrink is a pure function of `nodes`
/// (`min(nodes, FIXTURE_GRAPH_MAX_NODES)`), so the byte-identity contract
/// holds, the exact crossing point is published through
/// [`fixture_graph_scene_plan`] / [`FIXTURE_GRAPH_MAX_NODES`], and the
/// error-preferring twin [`try_fixture_graph_scene`] exists for tests that
/// want to stride the budget boundary. (`unreachable!` fires only if the
/// shrink invariant itself is broken — a fixture bug, not an input case.)
///
/// # Panics (never, by construction)
/// The `unreachable!` arm is structurally unreachable: the plan of any
/// shrunken node count fits [`MAX_SCENE_NODES`] exactly and every other
/// structural rule the validator checks is fixed by the id scheme and the
/// grid constants above.
pub fn fixture_graph_scene(revision: u64, nodes: usize, seed: u64) -> PanelScene {
    let shrunk = nodes.min(FIXTURE_GRAPH_MAX_NODES);
    match try_fixture_graph_scene(revision, shrunk, seed) {
        Ok(scene) => scene,
        Err(_) => unreachable!("shrunken fixture plan always fits the node budget"),
    }
}

/// Nested-group fixture: `groups` sibling chains at the scene root, each
/// nesting `depth` group levels deep with a leaf [`SceneElement::Port`]
/// ("file") at the bottom. Chain `c` owns ids `c*1000 + k` for its
/// level-`k` group and `c*1000 + depth + 1` for its leaf (strides keep
/// chains from colliding for any depth that can still pass the depth cap at
/// all — deeper requests refuse before ids could matter).
///
/// Depth math (pinned by [`crate::scene`]): root elements sit at depth 1, so
/// the leaf of a `depth`-deep chain lands at depth `depth + 1`; requests
/// with a leaf past [`MAX_SCENE_DEPTH`] come back as the validator's own
/// [`SceneError::DepthBudgetExceeded`]. The same math is pre-checked before
/// any allocation, and the node budget is pre-checked against the projected
/// `groups × (depth + 1)` count, both so hostile arguments refuse fast and
/// with exact observed numbers.
///
/// Calls [`PanelScene::validate`] before returning; never returns an invalid
/// scene. `groups == 0` yields the empty scene; `depth == 0` yields a bare
/// root leaf per chain.
pub fn fixture_scene_with_files(
    revision: u64,
    groups: usize,
    depth: usize,
) -> Result<PanelScene, SceneError> {
    // Saturating adds/muls keep hostile `usize` arguments deterministic.
    // Depth math mirrors the validator exactly: the deepest node of a
    // `depth`-deep chain is the leaf, observed at depth + 1.
    let leaf_depth = (depth as u64).saturating_add(1);
    if leaf_depth > MAX_SCENE_DEPTH as u64 {
        return Err(SceneError::DepthBudgetExceeded {
            depth: usize::try_from(leaf_depth).unwrap_or(usize::MAX),
            max: MAX_SCENE_DEPTH,
        });
    }
    let projected = (groups as u64).saturating_mul(leaf_depth);
    if projected > MAX_SCENE_NODES as u64 {
        return Err(SceneError::NodeBudgetExceeded {
            nodes: usize::try_from(projected).unwrap_or(usize::MAX),
            max: MAX_SCENE_NODES,
        });
    }

    const FILE_CHAIN_ID_STRIDE: u64 = 1000;
    let mut elements: Vec<SceneElement> = Vec::with_capacity(groups);
    for chain in 0..groups {
        let base = (chain as u64).saturating_mul(FILE_CHAIN_ID_STRIDE);
        // Innermost-out construction: the leaf grows the chain upward level
        // by level, so no recursion is needed and the depth math is the
        // loop's own length.
        let mut element = SceneElement::Port {
            id: StableId(base.saturating_add((depth as u64).saturating_add(1))),
            name: None,
            actions: Vec::new(),
            drag: None,
            at: Vec2([chain as f64 * 24.0, 12.0]),
            label: Some(format!("file-{chain}")),
        };
        for level in (1..=depth).rev() {
            element = SceneElement::Group {
                id: StableId(base.saturating_add(level as u64)),
                name: Some(format!("chain-{chain} level-{level}")),
                actions: Vec::new(),
                drag: None,
                children: vec![element],
                clip: None,
            };
        }
        elements.push(element);
    }

    let scene = PanelScene {
        revision: SceneRevision(revision),
        elements,
    };
    scene.validate()?;
    Ok(scene)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{MAX_PATCH_OPS, MAX_SCENE_DEPTH, MAX_SCENE_NODES, Rgba8};

    fn v2(x: f64, y: f64) -> Vec2 {
        Vec2([x, y])
    }

    fn raw_rect(id: u64) -> SceneElement {
        SceneElement::Rect {
            id: StableId(id),
            name: None,
            actions: vec![],
            drag: None,
            pos: Vec2([1.0, 2.0]),
            size: Vec2([3.0, 4.0]),
            fill: None,
            stroke: None,
        }
    }

    fn raw_group(id: u64, children: Vec<SceneElement>) -> SceneElement {
        SceneElement::Group {
            id: StableId(id),
            name: None,
            actions: vec![],
            drag: None,
            children,
            clip: None,
        }
    }

    /// Collect every element id in the scene (iterative walk; only the
    /// public [`SceneElement::stable_id`] accessor is used).
    fn collect_all_ids(scene: &PanelScene) -> HashSet<StableId> {
        let mut seen = HashSet::new();
        let mut stack: Vec<&SceneElement> = scene.elements.iter().collect();
        while let Some(element) = stack.pop() {
            seen.insert(element.stable_id());
            if let SceneElement::Group { children, .. } = element {
                stack.extend(children.iter());
            }
        }
        seen
    }

    /// Count every element in the scene (iterative walk).
    fn count_elements(scene: &PanelScene) -> usize {
        let mut count = 0usize;
        let mut stack: Vec<&SceneElement> = scene.elements.iter().collect();
        while let Some(element) = stack.pop() {
            count += 1;
            if let SceneElement::Group { children, .. } = element {
                stack.extend(children.iter());
            }
        }
        count
    }

    #[test]
    fn builders_produce_valid_scenes_at_every_grammar_variant() {
        let mut builder = SceneBuilder::new(7);
        builder.group(1).expect("group opens");
        builder
            .name(Some("canvas"))
            .actions(&["click", "select"])
            .drag(Some(DragMeta {
                opaque: Some("hand-edit".to_string()),
            }));
        builder.rect(2, [0.0, 0.0], [10.0, 10.0]).expect("rect add");
        builder.name(Some("card"));
        builder
            .path(
                3,
                vec![v2(0.0, 0.0), v2(1.0, 1.0), v2(2.0, 0.0)],
                false,
            )
            .expect("path add");
        builder
            .cubic(
                4,
                vec![CubicSegment {
                    start: v2(0.0, 0.0),
                    ctrl_a: v2(1.0, 0.0),
                    ctrl_b: v2(2.0, 1.0),
                    end: v2(3.0, 1.0),
                }],
                true,
            )
            .expect("cubic add");
        builder
            .text(
                5,
                "hello",
                [2.0, 3.0],
                11.5,
                SceneColor::Theme(ThemeColor::Accent),
            )
            .expect("text add");
        builder
            .asset(
                6,
                AssetKind::Image,
                "img://fixture.png",
                Rect2D {
                    pos: v2(0.0, 20.0),
                    size: v2(8.0, 6.0),
                },
            )
            .expect("asset add");
        builder.port(7, [3.0, 4.0]).expect("port add");
        builder.port(8, [13.0, 4.0]).expect("port add");
        builder
            .marker(
                9,
                [5.0, 5.0],
                MarkerKind::Arrow,
                SceneColor::Rgba(Rgba8([1, 2, 3, 255])),
            )
            .expect("marker add");
        builder.end_group();
        // A raw composite at the root (the escape hatch in its intended use).
        builder.element(raw_group(10, vec![raw_rect(11)]));
        builder
            .edge(
                12,
                7,
                8,
                vec![v2(3.0, 4.0), v2(13.0, 4.0)],
            )
            .expect("edge add");

        let scene = builder.finish().expect("well-formed builder scene");
        assert_eq!(scene.revision, SceneRevision(7));
        assert_eq!(scene.validate(), Ok(()));

        // Root order: the closed group first, then the raw group, then the
        // edge — paint order is insertion order.
        assert_eq!(scene.elements.len(), 3);
        match &scene.elements[0] {
            SceneElement::Group {
                id,
                name,
                actions,
                drag,
                children,
                clip,
            } => {
                assert_eq!(*id, StableId(1));
                assert_eq!(name.as_deref(), Some("canvas"));
                assert_eq!(
                    actions,
                    &["click".to_string(), "select".to_string()][..]
                );
                assert_eq!(
                    drag.as_ref().and_then(|drag| drag.opaque.as_deref()),
                    Some("hand-edit")
                );
                assert!(clip.is_none());
                // Every child kind added inside the group, in add order.
                assert_eq!(children.len(), 8);
                assert_eq!(children[0].stable_id(), StableId(2));
                assert!(matches!(children[0], SceneElement::Rect { .. }));
                assert!(matches!(children[1], SceneElement::Path { .. }));
                assert!(matches!(children[2], SceneElement::CubicPath { .. }));
                assert!(matches!(children[3], SceneElement::Text { .. }));
                assert!(matches!(children[4], SceneElement::Asset { .. }));
                assert!(matches!(children[5], SceneElement::Port { .. }));
                assert!(matches!(children[6], SceneElement::Port { .. }));
                assert!(matches!(children[7], SceneElement::Marker { .. }));
                // The rect's own accessories (set while it was active).
                match &children[0] {
                    SceneElement::Rect {
                        id, name, pos, size, ..
                    } => {
                        assert_eq!(*id, StableId(2));
                        assert_eq!(name.as_deref(), Some("card"));
                        assert_eq!(*pos, Vec2([0.0, 0.0]));
                        assert_eq!(*size, Vec2([10.0, 10.0]));
                    }
                    other => panic!("expected the rect, got {other:?}"),
                }
                // The LAST port kept the immutable defaults.
                match &children[5] {
                    SceneElement::Port {
                        id, name, actions, drag, at, label, ..
                    } => {
                        assert_eq!(*id, StableId(7));
                        assert!(name.is_none());
                        assert!(actions.is_empty());
                        assert!(drag.is_none());
                        assert_eq!(*at, Vec2([3.0, 4.0]));
                        assert!(label.is_none());
                    }
                    other => panic!("expected the port, got {other:?}"),
                }
                // Text and marker specifics landed verbatim.
                match &children[3] {
                    SceneElement::Text { size, color, .. } => {
                        assert_eq!(*size, 11.5);
                        assert_eq!(*color, SceneColor::Theme(ThemeColor::Accent));
                    }
                    other => panic!("expected the text, got {other:?}"),
                }
                match &children[7] {
                    SceneElement::Marker { kind, color, .. } => {
                        assert_eq!(*kind, MarkerKind::Arrow);
                        assert_eq!(*color, SceneColor::Rgba(Rgba8([1, 2, 3, 255])));
                    }
                    other => panic!("expected the marker, got {other:?}"),
                }
            }
            other => panic!("expected the canvas group, got {other:?}"),
        }
        match &scene.elements[1] {
            SceneElement::Group { id, children, .. } => {
                assert_eq!(*id, StableId(10));
                assert_eq!(children.len(), 1);
                assert_eq!(children[0].stable_id(), StableId(11));
            }
            other => panic!("expected the raw composite group, got {other:?}"),
        }
        match &scene.elements[2] {
            SceneElement::Edge { id, a, b, route, .. } => {
                assert_eq!(*id, StableId(12));
                assert_eq!(*a, StableId(7));
                assert_eq!(*b, StableId(8));
                assert_eq!(*route, vec![v2(3.0, 4.0), v2(13.0, 4.0)]);
            }
            other => panic!("expected the edge, got {other:?}"),
        }
    }

    #[test]
    fn builder_reports_validation_errors_as_results_not_panics() {
        // Typed adds refuse duplicated ids AT THE CHAIN, as Results.
        let mut builder = SceneBuilder::new(1);
        builder.group(1).expect("first group");
        assert_eq!(
            builder.group(1).unwrap_err(),
            SceneError::DuplicateId(StableId(1))
        );
        assert_eq!(
            builder.rect(1, [0.0, 0.0], [1.0, 1.0]).unwrap_err(),
            SceneError::DuplicateId(StableId(1))
        );

        // The raw escape hatch skips the eager ledger, so duplicates reach
        // finish() — which reports them as the validator's own error.
        let mut builder = SceneBuilder::new(1);
        builder.element(raw_rect(30));
        builder.element(raw_rect(30));
        assert_eq!(
            builder.finish().unwrap_err(),
            SceneError::DuplicateId(StableId(30))
        );

        // A pre-built subtree with an INTERNAL duplicate surfaces at finish
        // too (the escape hatch only defers the check, never drops it).
        let mut builder = SceneBuilder::new(1);
        builder.element(raw_group(20, vec![raw_rect(21), raw_rect(21)]));
        assert_eq!(
            builder.finish().unwrap_err(),
            SceneError::DuplicateId(StableId(21))
        );

        // Non-id structural defects also surface as Results from finish.
        let mut builder = SceneBuilder::new(1);
        builder
            .rect(40, [0.0, 0.0], [0.0, 0.0])
            .expect("ids stay legal; the zero size is finish()'s business");
        assert_eq!(builder.finish().unwrap_err(), SceneError::InvalidRect);

        // Unbalanced end_group is the documented no-op; the handle survives.
        let mut builder = SceneBuilder::new(1);
        let ok = builder.end_group().end_group().finish();
        assert_eq!(ok.map(|scene| scene.elements.len()), Ok(0));

        // Accessors with no active element are the documented no-ops.
        let mut builder = SceneBuilder::new(1);
        builder.name(Some("lost")).actions(&["unheard"]).drag(None);
        assert_eq!(builder.finish().map(|scene| scene.elements.len()), Ok(0));
    }

    /// The fluent paint setters (SCN-042): a rect built
    /// `rect(..)?.fill(..)?.stroke(..)` equals the documented
    /// `element(literal)` workaround byte for byte — the gap the demo
    /// plugin worked around is closed with no wire change.
    #[test]
    fn builder_paint_setters_match_the_literal() {
        let fill = SceneColor::Theme(ThemeColor::Strong);
        let stroke = Stroke {
            color: SceneColor::Theme(ThemeColor::Normal),
            width: 1.0,
        };

        // The chained shape...
        let chained = SceneBuilder::new(1)
            .rect(1, [0.0, 0.0], [10.0, 10.0])
            .expect("rect add")
            .fill(fill)
            .stroke(stroke)
            .name(Some("card"))
            .finish()
            .expect("valid scene");
        // ...must equal the literal the escape hatch used to require.
        let literal = PanelScene {
            revision: SceneRevision(1),
            elements: vec![SceneElement::Rect {
                id: StableId(1),
                name: Some("card".to_string()),
                actions: Vec::new(),
                drag: None,
                pos: Vec2([0.0, 0.0]),
                size: Vec2([10.0, 10.0]),
                fill: Some(fill),
                stroke: Some(stroke),
            }],
        };
        assert_eq!(chained, literal, "chained paint == the literal element");
        assert_eq!(
            bincode::serialize(&chained).expect("serialize chained"),
            bincode::serialize(&literal).expect("serialize literal"),
            "the parity holds on the wire bytes too"
        );

        // Path + CubicPath: both paints land, in either accessory order.
        let painted_path = SceneBuilder::new(1)
            .path(2, vec![v2(0.0, 0.0), v2(4.0, 0.0), v2(4.0, 4.0)], true)
            .expect("path add")
            .stroke(stroke)
            .fill(fill)
            .finish()
            .expect("valid scene");
        match &painted_path.elements[0] {
            SceneElement::Path { stroke: s, fill: f, .. } => {
                assert_eq!(*s, Some(stroke));
                assert_eq!(*f, Some(fill));
            }
            other => panic!("expected the path, got {other:?}"),
        }
        let painted_cubic = SceneBuilder::new(1)
            .cubic(
                3,
                vec![CubicSegment {
                    start: v2(0.0, 0.0),
                    ctrl_a: v2(1.0, 0.0),
                    ctrl_b: v2(2.0, 1.0),
                    end: v2(3.0, 1.0),
                }],
                false,
            )
            .expect("cubic add")
            .fill(fill)
            .stroke(stroke)
            .finish()
            .expect("valid scene");
        match &painted_cubic.elements[0] {
            SceneElement::CubicPath { stroke: s, fill: f, .. } => {
                assert_eq!(*s, Some(stroke));
                assert_eq!(*f, Some(fill));
            }
            other => panic!("expected the cubic path, got {other:?}"),
        }

        // Edge: the stroke lands on the route; `fill` has no Edge arm and
        // is the documented no-op there (an Edge stays fill-less).
        let stroked_edge = SceneBuilder::new(1)
            .port(10, [0.0, 0.0])
            .expect("port add")
            .port(11, [8.0, 0.0])
            .expect("port add")
            .edge(12, 10, 11, vec![v2(0.0, 0.0), v2(8.0, 0.0)])
            .expect("edge add")
            .fill(fill) // no-op on the Edge
            .stroke(stroke)
            .finish()
            .expect("valid scene");
        match &stroked_edge.elements[2] {
            SceneElement::Edge { stroke: s, .. } => assert_eq!(*s, Some(stroke)),
            other => panic!("expected the edge, got {other:?}"),
        }

        // The setters re-point with the active element: paint applies to
        // the MOST-RECENT add only, and end_group re-points at the group.
        let re_pointed = SceneBuilder::new(1)
            .rect(20, [0.0, 0.0], [1.0, 1.0])
            .expect("rect add")
            .rect(21, [2.0, 0.0], [1.0, 1.0])
            .expect("rect add")
            .fill(fill)
            .finish()
            .expect("valid scene");
        for (index, expected_fill) in [(0usize, None), (1, Some(fill))] {
            match &re_pointed.elements[index] {
                SceneElement::Rect { fill: f, .. } => assert_eq!(*f, expected_fill),
                other => panic!("expected a rect at {index}, got {other:?}"),
            }
        }
    }

    /// Off the paintable variants the paint setters follow the accessory
    /// convention: documented no-ops, never errors — on a Group, on a Text
    /// (whose color is fixed at add time), and before any element exists.
    #[test]
    fn builder_paint_setters_are_noops_off_the_paintable_variants() {
        let fill = SceneColor::Theme(ThemeColor::Strong);
        let stroke = Stroke {
            color: SceneColor::Theme(ThemeColor::Normal),
            width: 1.0,
        };

        // A Group and a Text stay untouched by fill/stroke...
        let scene = SceneBuilder::new(1)
            .group(1)
            .expect("group add")
            .fill(fill)
            .stroke(stroke)
            .end_group()
            .text(
                2,
                "hello",
                [0.0, 0.0],
                11.0,
                SceneColor::Theme(ThemeColor::Accent),
            )
            .expect("text add")
            .fill(fill)
            .stroke(stroke)
            .finish()
            .expect("valid scene");
        match &scene.elements[0] {
            SceneElement::Group { name, actions, drag, clip, .. } => {
                assert!(name.is_none() && actions.is_empty() && drag.is_none() && clip.is_none());
            }
            other => panic!("expected the group, got {other:?}"),
        }
        match &scene.elements[1] {
            SceneElement::Text { color, .. } => {
                assert_eq!(*color, SceneColor::Theme(ThemeColor::Accent));
            }
            other => panic!("expected the text, got {other:?}"),
        }

        // ...and with no active element at all the setters are the same
        // documented no-op (an empty scene still finishes clean).
        let mut builder = SceneBuilder::new(1);
        builder.fill(fill).stroke(stroke);
        assert_eq!(builder.finish().map(|scene| scene.elements.len()), Ok(0));
    }

    #[test]
    fn fixture_graph_scene_is_deterministic_and_valid() {
        let first = fixture_graph_scene(1, 50, 42);
        let second = fixture_graph_scene(1, 50, 42);
        // Same arguments → byte-identical scenes (the whole reason the
        // fixtures exist).
        assert_eq!(
            bincode::serialize(&first).expect("serialize first"),
            bincode::serialize(&second).expect("serialize second"),
            "same (revision, nodes, seed) must encode to identical bytes"
        );
        assert_eq!(first, second);
        assert_eq!(first.revision, SceneRevision(1));
        assert_eq!(first.validate(), Ok(()));

        // Documented multiplicity: 50 nodes ≥ 2 → 8 elements each.
        assert_eq!(fixture_graph_scene_plan(50), 400);
        assert_eq!(count_elements(&first), 400);
        // Ids are unique: the walked set carries exactly one entry per built
        // element (and the node groups number exactly `nodes`).
        let ids = collect_all_ids(&first);
        assert_eq!(ids.len(), 400);
        let group_count = {
            let mut stack: Vec<&SceneElement> = first.elements.iter().collect();
            let mut count = 0usize;
            while let Some(element) = stack.pop() {
                if matches!(element, SceneElement::Group { .. }) {
                    count += 1;
                    if let SceneElement::Group { children, .. } = element {
                        stack.extend(children.iter());
                    }
                }
            }
            count
        };
        assert_eq!(group_count, 50);

        // A different seed steers a different (still deterministic) routing.
        let other_seed = fixture_graph_scene(1, 50, 43);
        assert_eq!(other_seed.validate(), Ok(()));
        assert_ne!(
            bincode::serialize(&first).expect("bytes first seed"),
            bincode::serialize(&other_seed).expect("bytes other seed"),
            "seeds steer edge selection; distinct seeds keep distinct scenes"
        );
    }

    #[test]
    fn fixture_graph_scene_edges_reference_existing_ids() {
        // Fixture contract: every Edge.a and Edge.b names a Port or a Group
        // element that exists in the same scene (the vocabulary allows edges
        // to point anywhere; the FIXTURE promises the disciplined subset).
        let scene = fixture_graph_scene(1, 12, 9);
        let mut referenceable = HashSet::new();
        let mut endpoints = Vec::new();
        let mut stack: Vec<&SceneElement> = scene.elements.iter().collect();
        while let Some(element) = stack.pop() {
            match element {
                SceneElement::Group { children, .. } => {
                    referenceable.insert(element.stable_id());
                    stack.extend(children.iter());
                }
                SceneElement::Port { .. } => {
                    referenceable.insert(element.stable_id());
                }
                SceneElement::Edge { a, b, .. } => endpoints.push((*a, *b)),
                _ => {}
            }
        }
        assert_eq!(endpoints.len(), 12, "one outgoing edge per node");
        for (a, b) in endpoints {
            assert!(
                referenceable.contains(&a),
                "edge endpoint {} must name a port/group id in the fixture",
                a.0
            );
            assert!(
                referenceable.contains(&b),
                "edge endpoint {} must name a port/group id in the fixture",
                b.0
            );
        }
    }

    #[test]
    fn fixture_graph_scene_scales_counts_and_stays_within_budgets() {
        // Documented per-node multiplicity (group + card + 4 ports + label +
        // edge = 8) masters counts for every size the plan describes.
        for (nodes, expected) in [(0usize, 0usize), (1, 7), (2, 16), (5, 40), (25, 200)] {
            let scene = fixture_graph_scene(9, nodes, 3);
            assert_eq!(fixture_graph_scene_plan(nodes), expected);
            assert_eq!(count_elements(&scene), expected, "nodes = {nodes}");
            assert_eq!(scene.validate(), Ok(()));
        }

        // A legal bench-scale count validates and spends the documented
        // budget: 400 nodes → 3200 ≤ 10_000 elements.
        let sized = fixture_graph_scene(4, 400, 7);
        assert_eq!(fixture_graph_scene_plan(400), 3200);
        assert_eq!(count_elements(&sized), 3200);
        assert!(count_elements(&sized) <= MAX_SCENE_NODES);
        assert_eq!(sized.validate(), Ok(()));

        // The max-fitting count rides the budget boundary EXACTLY: 1250
        // nodes → 10_000 elements == MAX_SCENE_NODES, still valid.
        let boundary = fixture_graph_scene(4, FIXTURE_GRAPH_MAX_NODES, 7);
        assert_eq!(count_elements(&boundary), MAX_SCENE_NODES);
        assert_eq!(boundary.validate(), Ok(()));

        // OVERSIZED counts: the fixture DECISION — deterministic shrink, no
        // panic, never an invalid scene — returns the boundary-sized scene.
        let shrunk = fixture_graph_scene(4, 5000, 7);
        assert_eq!(shrunk.validate(), Ok(()));
        assert_eq!(count_elements(&shrunk), MAX_SCENE_NODES);
        assert_eq!(
            fixture_graph_scene(4, usize::MAX, 7).validate(),
            Ok(()),
            "even a usize::MAX request pins to the same fit"
        );

        // The checked twin prefers Err over the shrink, reporting the true
        // projected count.
        assert_eq!(
            try_fixture_graph_scene(4, 5000, 7).err(),
            Some(SceneError::NodeBudgetExceeded {
                nodes: 40_000,
                max: MAX_SCENE_NODES,
            })
        );
        // The boundary itself stays accepted on the checked path too.
        let checked = try_fixture_graph_scene(4, FIXTURE_GRAPH_MAX_NODES, 7)
            .expect("boundary count is exactly inside the budget");
        assert_eq!(checked.validate(), Ok(()));
    }

    #[test]
    fn fixture_nested_scene_depth_boundary() {
        // Depth counts nested group levels; the root list sits at depth 1
        // (scene.rs pins that), so a `depth`-deep chain's leaf lands at
        // depth + 1. Every depth up to MAX - 1 therefore validates...
        for depth in [0usize, 1, 2, MAX_SCENE_DEPTH - 1] {
            let scene = fixture_scene_with_files(3, 2, depth)
                .unwrap_or_else(|error| panic!("depth {depth} must fit: {error}"));
            assert_eq!(scene.revision, SceneRevision(3));
            assert_eq!(scene.validate(), Ok(()));
            // groups × (depth + 1) elements: every chain contributes its
            // group levels plus the leaf port.
            let expected = 2 * (depth + 1);
            assert_eq!(count_elements(&scene), expected, "depth = {depth}");
        }
        // ... depth == MAX - 1 puts the leaf exactly on the cap.
        let capped = fixture_scene_with_files(3, 1, MAX_SCENE_DEPTH - 1).expect("cap fits");
        assert_eq!(count_elements(&capped), MAX_SCENE_DEPTH);

        // ... and the first depth past the cap refuses with the exact
        // validator arithmetic (leaf observed at depth + 1).
        for requested in [MAX_SCENE_DEPTH, MAX_SCENE_DEPTH + 1, MAX_SCENE_DEPTH * 4] {
            let error =
                fixture_scene_with_files(3, 1, requested).expect_err("past the cap it refuses");
            let leaf_depth = requested.saturating_add(1);
            assert!(
                matches!(
                    &error,
                    SceneError::DepthBudgetExceeded { depth, max }
                        if *depth == leaf_depth && *max == MAX_SCENE_DEPTH
                ),
                "unexpected depth error: {error:?}"
            );
        }
    }

    #[test]
    fn patch_builder_composes_with_apply_patch() {
        let mut scene = PanelScene {
            revision: SceneRevision(2),
            elements: vec![raw_rect(1), raw_group(2, vec![])],
        };

        // Add at root, add INTO a base scene group, remove an element — the
        // whole patch built fluently against revision 2, finishing at 3.
        let patch = PatchBuilder::new(2)
            .add(None, raw_rect(5))
            .add(Some(2), raw_rect(6))
            .add(None, raw_group(7, vec![]))
            .remove(1)
            .finish(3)
            .expect("legal patch");
        assert_eq!(patch.base_revision, SceneRevision(2));
        assert_eq!(patch.revision, SceneRevision(3));
        assert_eq!(patch.ops.len(), 4);

        scene.apply_patch(patch).expect("patch applies");
        assert_eq!(scene.revision, SceneRevision(3));
        assert_eq!(scene.validate(), Ok(()));
        // Removal hit rect(1); both adds landed; the nested add reached the
        // base group.
        match &scene.elements[0] {
            SceneElement::Group { id, children, .. } => {
                assert_eq!(*id, StableId(2));
                assert_eq!(children.len(), 1);
                assert_eq!(children[0].stable_id(), StableId(6));
            }
            other => panic!("expected group 2 with its new child, got {other:?}"),
        }
        let ids = collect_all_ids(&scene);
        assert!(ids.contains(&StableId(5)));
        assert!(ids.contains(&StableId(7)));
        assert!(!ids.contains(&StableId(1)));

        // Roundtrip through bincode preserves the patched scene.
        let bytes = bincode::serialize(&scene).expect("serialize patched scene");
        let decoded: PanelScene = bincode::deserialize(&bytes).expect("decode patched scene");
        assert_eq!(decoded, scene);

        // The builder refuses a non-advancing successor up-front. Snapshot
        // the patched scene first; the refusals operate on their own
        // builders and must leave it untouched.
        let snapshot = (scene.revision, scene.elements.clone());
        let error = PatchBuilder::new(2)
            .add(None, raw_rect(50))
            .finish(2)
            .expect_err("equal successor must refuse");
        assert_eq!(
            error,
            SceneError::NonMonotonicRevision {
                current: SceneRevision(2),
                given: SceneRevision(2),
            }
        );
        let error = PatchBuilder::new(3)
            .remove(50)
            .finish(2)
            .expect_err("rewind must refuse");
        assert_eq!(
            error,
            SceneError::NonMonotonicRevision {
                current: SceneRevision(3),
                given: SceneRevision(2),
            }
        );

        // One op over the budget is refused up-front, before apply_patch
        // ever sees it.
        let mut builder = PatchBuilder::new(1);
        for i in 0..=MAX_PATCH_OPS {
            builder.add(None, raw_rect(i as u64 + 100));
        }
        let error = builder.finish(2).expect_err("10_001 ops must refuse");
        assert!(matches!(
            &error,
            SceneError::PatchOpBudgetExceeded { ops, .. } if *ops == MAX_PATCH_OPS + 1
        ));

        // A refused finish drained the queue (no accidental re-stamps).
        assert_eq!(builder.finish(2).expect("fresh empty patch").ops.len(), 0);

        // ...and the snapshot still matches: zero residue on the scene.
        assert_eq!(
            (scene.revision, scene.elements.clone()),
            snapshot,
            "refusals must not touch the already-patched scene"
        );
    }
}
