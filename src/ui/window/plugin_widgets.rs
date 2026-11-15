//! Renderer-free STATE projection of plugin semantic widgets: expansion,
//! selection, focus, visible-range windowing, and lazy-request emission.
//!
//! This is the widget-side sibling of [`crate::ui::window::plugin_scene`]
//! (SCN-003's renderer-free prim projection): the plugin owns the *semantic*
//! widget data ([`WidgetSurface`], SWG-001's contracts), the host owns the
//! *state* — what is expanded, what is selected, what has keyboard focus,
//! which window of a 100k-item list is materialized, and which lazy pages are
//! in flight. Nothing here constructs an iced widget, a `Message`, or any
//! renderer type; the native-widget MOUNT is a later host task (after
//! SWG-002 + SWG-003 land) and will consume exactly this state machine.
//!
//! State-class discipline (ARCHITECTURE §3 / §14): everything [`WidgetUiState`]
//! holds is transient host state (class 3). It is never serialized into scene
//! or widget data; expansion stays host-local per §14 ("Expansion, focus,
//! hover, scrolling ... stay host-local") — the [`WidgetEvent`] values the
//! methods return are NOTIFICATIONS the mount forwards to the plugin, never
//! state-asking round trips.
//!
//! # The §16 proof instruments
//!
//! ARCHITECTURE §16 names the defects this layer must make visible:
//!
//! - "Branch-expand changes only that branch's visible state" — one
//!   [`WidgetUiState::toggle_expand`] writes exactly ONE expansion entry
//!   ([`WidgetStateMutations::row_state_changes`] += 1), and the flat-row
//!   list changes only by that branch's rows (other entries byte-identical
//!   at their positions — `expanding_one_branch_mutates_only_that_branch`);
//! - "Selection updates do not rebuild the whole tree" — selection writes
//!   are diff-based (a 2-of-500 selection touches ≤ 4 rows), and
//!   [`WidgetStateMutations::whole_state_resets`] stays 0 in every normal
//!   flow: it moves ONLY through the explicit [`WidgetUiState::reset_all`]
//!   teardown helper — closing a surface is where it fires (§16 "closing a
//!   tab/plugin cancels pending data requests and releases state");
//! - "100k backing objects do not create 100k host widgets" —
//!   [`materialize_ids`] yields only the visible window (+ margin) of the
//!   flat list, and the 100k fixture (`flat_len_of_large_list`) builds
//!   [`FlatRow`]s cheaply, without any widget;
//! - "disabled commands un-invocable" — [`WidgetUiState::command_click`]
//!   refuses through the synced enablement view
//!   ([`WidgetUiState::is_enabled`]) before any event exists;
//! - "lazy children arrive incremental with stable identities" — the lazy
//!   REQUEST is the event ([`WidgetUiState::request_children_if_lazy`]);
//!   the arrival is a plugin PATCH (rows added via `WidgetOp::Add`), and
//!   [`WidgetUiState::sync`] clears the pending marker when it observes
//!   rows under the parent — that is the whole anti-storm story: while a
//!   page is in flight the pending entry dedups re-requests, and the
//!   arriving patch (not a timeout, not a poll) ends the window.
//!
//! Event VALIDITY is not re-implemented here: every constructed event is
//! bound through the crate's own [`WidgetEvent::validate`] (the
//! construct-level companion of the bounded decoders); this module adds
//! only the STATE checks (enablement, selection capability) on top.
//!
//! # SWG-003 seam (documented, not implemented)
//!
//! Icon/menu dispatch (confirmation-policy application on
//! [`WidgetEvent::CommandInvoked`] at the mount, menu-depth bookkeeping) is
//! SWG-003's surface. The command plumbing below is written to be consumed
//! from there unchanged: the [`WidgetEvent::CommandInvoked`] construction +
//! validation + enablement gate lives here ONCE, so the native mount and
//! the MCP parity layer (SWG-005) share the same path and cannot drift
//! (§15: GUI and MCP produce the same event/validation/revision).

// Stage-1 surface: consumed by the unit tests in this module only until the
// native-widget mount wiring arrives (later host task after SWG-002+003);
// the allow mirrors `plugin_scene.rs`'s stage-1 precedent.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use ocs_plugin_api::scene::{SceneRevision, StableId};
use ocs_plugin_api::widgets::{
    ItemDef, ItemListDef, TreeRowDef, WidgetEvent, WidgetNode, WidgetRange, WidgetSurface,
    WidgetTreeDef, MAX_SELECTION_IDS, MAX_WIDGET_RANGE_LEN, MAX_WIDGET_TARGET_IDS,
};

// ---------------------------------------------------------------------------
// Flat-row model (the windowing input)
// ---------------------------------------------------------------------------

/// One row of the flattened, expansion-filtered visible tree/list.
///
/// `depth` is the nesting level (roots at 0) — the mount maps it to indent
/// guides; `height` is the row's render height in logical units (uniform
/// rows all carry the same value; the field exists so a per-row height, e.g.
/// two-line status rows, needs no model change).
///
/// The flat list is THE §16 branch-isolation instrument: expanding one
/// branch changes the flat list ONLY by that branch's rows, in place, with
/// every other entry byte-identical at its position (proved by
/// `expanding_one_branch_mutates_only_that_branch`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FlatRow {
    /// The row's presentation id (surface-wide namespace).
    pub id: StableId,
    /// Nesting depth (roots at 0); the mount's indent input.
    pub depth: usize,
    /// Row height in logical units (windowing math input).
    pub height: f32,
}

/// Uniform-height windowing parameters.
///
/// `viewport_height` / `row_height` are logical units; `margin_rows` is the
/// number of context rows materialized on each side of the visible window
/// (the roving-focus / scroll-anchor warm zone the mount keeps).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WindowSpec {
    pub viewport_height: f32,
    pub row_height: f32,
    pub margin_rows: usize,
}

impl WindowSpec {
    /// Number of whole rows the viewport shows. A degenerate `row_height`
    /// (non-finite or ≤ 0) falls back to 1 so the window never vanishes.
    fn visible_rows(&self) -> usize {
        if !self.row_height.is_finite() || self.row_height <= 0.0 {
            return 1;
        }
        (self.viewport_height / self.row_height).max(1.0).floor() as usize
    }
}

/// The visible window of a flat list: half-open row-index range
/// `[start, end)` over the flat list, already including the
/// [`WindowSpec::margin_rows`] context rows on each side. Produced by
/// [`visible_window`], consumed by [`materialize_ids`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisibleWindow {
    /// First flat-list index in the window (inclusive).
    pub start: usize,
    /// One past the last flat-list index (exclusive).
    pub end: usize,
}

impl VisibleWindow {
    /// Row count the window materializes.
    pub(crate) fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// `true` when the window is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// `true` when `index` is inside `[start, end)`.
    pub(crate) fn contains(&self, index: usize) -> bool {
        index >= self.start && index < self.end
    }
}

/// Compute the visible window of `rows_flat` under `spec` for a viewport
/// scrolled to `scroll_row` (the first row at the viewport's top edge).
///
/// Pure arithmetic — no state, no renderer: the mount calls this whenever
/// the scroll anchor or the flat list changes.
pub(crate) fn visible_window(
    rows_flat: &[FlatRow],
    scroll_row: usize,
    spec: &WindowSpec,
) -> VisibleWindow {
    let total = rows_flat.len();
    let visible = spec.visible_rows();
    // Clamp the anchor so the viewport never hangs past the end (and pins
    // to 0 when the list is shorter than the viewport).
    let start = scroll_row.min(total.saturating_sub(visible)).min(total);
    VisibleWindow {
        start: start.saturating_sub(spec.margin_rows),
        end: (start + visible + spec.margin_rows).min(total),
    }
}

/// Materialize the presentation ids of `window` over `flat`, in paint order.
///
/// The §16 windowing proof: over a 100k-row flat list a viewport-sized
/// window yields only a few dozen ids — not 100k widgets.
pub(crate) fn materialize_ids(flat: &[FlatRow], window: VisibleWindow) -> Vec<StableId> {
    debug_assert!(
        window.end <= flat.len(),
        "window must lie inside the flat list"
    );
    (window.start..window.end).map(|i| flat[i].id).collect()
}

// ---------------------------------------------------------------------------
// Row views (the synced def-data read model)
// ---------------------------------------------------------------------------

/// The state layer's read view of one row/item, gathered from the synced
/// surface at [`WidgetUiState::sync`].
///
/// Enablement checks ([`WidgetUiState::is_enabled`], focus roving, the
/// disabled-expansion refusal) consult this snapshot — the def data as of
/// the LAST sync — not a live surface walk, so state decisions are
/// deterministic against the exact revision the mount presents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RowView {
    /// The row can expand/collapse.
    pub expandable: bool,
    /// Expandable with NO static children: request a lazy page.
    pub lazy_children: bool,
    /// Enabled state (contract default `true`).
    pub enabled: bool,
}

impl RowView {
    fn from_row(row: &TreeRowDef) -> Self {
        RowView {
            expandable: row.expandable,
            lazy_children: row.lazy_children,
            enabled: row.enabled,
        }
    }

    fn from_item(item: &ItemDef) -> Self {
        RowView {
            expandable: false,
            lazy_children: false,
            enabled: item.enabled && item.visible,
        }
    }
}

/// Enablement/capability view of one widget, gathered by
/// [`WidgetUiState::sync`].
///
/// `multi_select` is the TREE capability gate
/// (`TreeCapabilities::multi_select`); lists carry no selection-capability
/// contract (multi-item selection is a host-native interaction there,
/// bounded by [`MAX_SELECTION_IDS`] alone), so a list's view states
/// `multi_select: true`. A disabled/invisible tree can neither be toggled
/// ([`WidgetUiState::toggle_expand`]) nor invoked into
/// ([`WidgetUiState::command_click`]).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct WidgetView {
    /// Tree multi-select capability (`TreeCapabilities::multi_select`);
    /// `true` for lists (no contract gate — see the type docs).
    pub multi_select: bool,
    /// Tree lazy-children capability (`TreeCapabilities::lazy_children`).
    pub lazy_children: bool,
    /// Widget-level enablement + visibility (tree/list).
    pub enabled: bool,
}

/// Row views + capability view of a [`WidgetTreeDef`] (keyed by row id).
fn tree_parts_of(def: &WidgetTreeDef) -> (WidgetView, HashMap<StableId, RowView>) {
    let view = WidgetView {
        multi_select: def.capabilities.multi_select,
        lazy_children: def.capabilities.lazy_children,
        enabled: def.enabled && def.visible,
    };
    let mut views = HashMap::new();
    for row in &def.rows {
        views.insert(row.id, RowView::from_row(row));
    }
    (view, views)
}

/// Item views + capability view of an [`ItemListDef`] (keyed by item id).
fn list_parts_of(def: &ItemListDef) -> (WidgetView, HashMap<StableId, RowView>) {
    let view = WidgetView {
        multi_select: true, // lists carry no selection-capability gate
        lazy_children: false,
        enabled: def.enabled && def.visible,
    };
    let mut views = HashMap::new();
    for item in &def.items {
        views.insert(item.id, RowView::from_item(item));
    }
    (view, views)
}

// ---------------------------------------------------------------------------
// Mutation counters (the §16 defect probe)
// ---------------------------------------------------------------------------

/// The per-surface mutation counters — the ARCHITECTURE §16 proof
/// instruments. Probes never reset these; the ONLY entry to
/// [`WidgetStateMutations::whole_state_resets`] is the explicit
/// [`WidgetUiState::reset_all`] teardown helper.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WidgetStateMutations {
    /// Per-row state writes: expansion toggles and selection diff writes,
    /// counting the ROWS the operation touched (one toggle == 1; a
    /// 2-of-500 selection write == ≤ 4). Sync prunes count ONLY for ids
    /// that were actually PRESENT in selection/expansion (see `sync`).
    pub row_state_changes: u64,
    /// Full-state replacement calls (the DEFECT counter — stays 0 in every
    /// normal flow; moves only via [`WidgetUiState::reset_all`]).
    pub whole_state_resets: u64,
    /// Lazy page requests actually emitted
    /// ([`WidgetEvent::ChildrenRequested`]).
    pub lazy_requests_emitted: u64,
    /// Requests the pending dedup window suppressed (each suppression is a
    /// re-request storm that did NOT happen).
    pub duplicate_requests_suppressed: u64,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// State-layer refusal reasons. A refusal leaves ALL state untouched (no
/// counter moves, no event) — the §16 rule that a rejected interaction
/// cannot half-apply starts here, at the state layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WidgetStateError {
    /// The row id is not in the synced surface (unknown or pruned).
    UnknownRow(StableId),
    /// The row is not expandable (contract-wise there is nothing to toggle).
    NotExpandable(StableId),
    /// The row/widget is disabled — the §16 "disabled cannot be invoked"
    /// rule as a state check.
    Disabled(StableId),
    /// A single-select tree was offered more than one id; nothing changed.
    MultiSelectNotCapable {
        /// The widget id whose capability refused.
        widget: StableId,
        /// How many ids were offered.
        offered: usize,
    },
    /// The selection set passes [`MAX_SELECTION_IDS`] (the state-side
    /// companion of the crate's budget error; the event is never
    /// constructed past the cap).
    SelectionTooLarge {
        /// How many ids were offered.
        offered: usize,
    },
    /// [`WidgetEvent::CommandInvoked`] refuses empty targets (a semantic
    /// command names what it acts ON; §13 "invocation with explicit target
    /// IDs") — also returned when the crate's own event validation rejects
    /// the built command event.
    NoTargets,
    /// Target ids pass [`MAX_WIDGET_TARGET_IDS`].
    TooManyTargets {
        /// How many targets were offered.
        offered: usize,
    },
}

impl std::fmt::Display for WidgetStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WidgetStateError::UnknownRow(id) => {
                write!(f, "row/item {} is not in the synced surface", id.0)
            }
            WidgetStateError::NotExpandable(id) => {
                write!(f, "row {} is not expandable", id.0)
            }
            WidgetStateError::Disabled(id) => {
                write!(f, "row/widget {} is disabled and cannot be invoked", id.0)
            }
            WidgetStateError::MultiSelectNotCapable { widget, offered } => write!(
                f,
                "tree {} is single-select but {} ids were offered",
                widget.0, offered
            ),
            WidgetStateError::SelectionTooLarge { offered } => write!(
                f,
                "selection of {offered} ids passes the {MAX_SELECTION_IDS}-id cap"
            ),
            WidgetStateError::NoTargets => {
                write!(f, "a semantic command needs explicit, valid target ids")
            }
            WidgetStateError::TooManyTargets { offered } => write!(
                f,
                "{offered} target ids pass the {MAX_WIDGET_TARGET_IDS}-target cap"
            ),
        }
    }
}

impl std::error::Error for WidgetStateError {}

// ---------------------------------------------------------------------------
// WidgetUiState
// ---------------------------------------------------------------------------

/// Per-surface transient host state (ARCHITECTURE §3 state class 3): what is
/// expanded, selected, focused, scrolled, and which lazy pages are in flight.
///
/// Lives host-side ONLY; never serialized into scene or widget data. One
/// instance per mounted widget surface, keyed by its
/// [`WidgetUiState::surface_id`]. Every method is headless and
/// deterministic; the mount calls them from its native interaction handlers.
#[derive(Debug, Clone)]
pub(crate) struct WidgetUiState {
    /// The surface this state belongs to (rides every emitted event).
    surface_id: StableId,
    /// Expansion state, host-local (§14): absent = the row contract's
    /// default (collapsed — `TreeRowDef` has no default-expanded field);
    /// ONLY explicit toggles are stored.
    expanded: HashMap<StableId, bool>,
    /// Selection: presentation ids (bounded by [`MAX_SELECTION_IDS`]).
    selection: Vec<StableId>,
    /// Focused row/item/widget (keyboard-nav target; roving-tabindex model).
    focus: Option<StableId>,
    /// Scroll anchor (state only; the windowing input; row index into the
    /// flat list).
    scroll_row: usize,
    /// Lazy requests in flight (dedup window): parent row id → the range
    /// requested. Cleared by `sync` when rows arrive under the parent (or
    /// the parent disappears); NOT cleared by collapse — an in-flight page
    /// fetch survives a collapsed view, and keeping the entry is exactly
    /// what makes "collapse/expand cycles while pending → 0 new requests"
    /// true (documented decision).
    pending_children: HashMap<StableId, WidgetRange>,
    /// The §16 mutation counters.
    mutations: WidgetStateMutations,
    /// The read model captured by the last `sync` (`None` before it).
    synced: Option<SyncedSurface>,
}

impl WidgetUiState {
    /// New, empty state for one surface. Everything defaults: nothing
    /// expanded, nothing selected, NO focus (focus starts `None` — the
    /// roving ring lands only after the first navigation), scrolled to the
    /// top.
    pub(crate) fn new(surface_id: StableId) -> Self {
        WidgetUiState {
            surface_id,
            expanded: HashMap::new(),
            selection: Vec::new(),
            focus: None,
            scroll_row: 0,
            pending_children: HashMap::new(),
            mutations: WidgetStateMutations::default(),
            synced: None,
        }
    }

    /// The surface this state belongs to.
    pub(crate) fn surface_id(&self) -> StableId {
        self.surface_id
    }

    /// The §16 mutation counters (read side; only `reset_all` writes the
    /// reset counter, only the state methods write the rest).
    pub(crate) fn mutations(&self) -> &WidgetStateMutations {
        &self.mutations
    }

    /// The scroll anchor (row index into the flat list).
    pub(crate) fn scroll_row(&self) -> usize {
        self.scroll_row
    }

    /// Set the scroll anchor (the mount's scroll handler input; the
    /// windowing calculator re-runs whenever this moves).
    pub(crate) fn set_scroll_row(&mut self, row: usize) {
        self.scroll_row = row;
    }

    /// The focused id, when any ([`WidgetUiState::focus_next`] /
    /// [`WidgetUiState::focus_prev`] move it; the mount reads it to paint
    /// the roving-tabindex ring).
    pub(crate) fn focus(&self) -> Option<StableId> {
        self.focus
    }

    /// Explicit expansion entry, when the user toggled this row; absent
    /// means the contract default (collapsed).
    pub(crate) fn explicit_expansion(&self, id: StableId) -> Option<bool> {
        self.expanded.get(&id).copied()
    }

    /// Effective expansion of `id`: the explicit toggle when present, else
    /// the default the CALLER states (the contract default is collapsed).
    /// The flat-list builder's input — one lookup per row.
    pub(crate) fn effective_expanded(&self, id: StableId, default: bool) -> bool {
        self.expanded.get(&id).copied().unwrap_or(default)
    }

    /// The current selection (presentation ids, bounded by
    /// [`MAX_SELECTION_IDS`]).
    pub(crate) fn selection(&self) -> &[StableId] {
        &self.selection
    }

    /// Lazy requests in flight (read side): parent row id → requested range.
    pub(crate) fn pending_children(&self) -> &HashMap<StableId, WidgetRange> {
        &self.pending_children
    }

    /// The read model captured by the last [`WidgetUiState::sync`]; `None`
    /// before the first sync. The state methods take the caller's row map
    /// (usually `state.synced().views`) so the layer stays callable
    /// headless against any snapshot the mount presents.
    pub(crate) fn synced(&self) -> Option<&SyncedSurface> {
        self.synced.as_ref()
    }

    /// Explicit teardown (tab/plugin close, §16 "closing a tab/plugin
    /// cancels pending data requests and releases state"): every field
    /// replaced, every pending request dropped. This is the ONLY path that
    /// moves [`WidgetStateMutations::whole_state_resets`] — in every normal
    /// interaction flow it stays 0, which is precisely the defect probe.
    pub(crate) fn reset_all(&mut self) {
        self.expanded = HashMap::new();
        self.selection = Vec::new();
        self.focus = None;
        self.scroll_row = 0;
        self.pending_children = HashMap::new();
        self.mutations.whole_state_resets += 1;
    }

    // -- expansion ----------------------------------------------------------

    /// Toggle one expandable row's expansion state (host-local, §14).
    ///
    /// Absent entries honor the row contract default (collapsed), so the
    /// first toggle on an untouched row stores `true`. Returns the
    /// [`WidgetEvent::TreeExpanded`] NOTIFICATION for the mount to forward.
    ///
    /// ONE event per method (documented policy): the lazy REQUEST is NOT
    /// folded in here — after an expand-on of a lazy row the mount calls
    /// [`WidgetUiState::request_children_if_lazy`], which keeps each event
    /// producer single-purpose and the pending dedup explicit. Activation
    /// likewise does NOT synthesize a [`WidgetEvent::CommandInvoked`]; the
    /// MOUNT decides command semantics later (SWG-003).
    ///
    /// Mutation proof (§16): toggling one branch writes exactly ONE
    /// expansion entry — `row_state_changes` += 1, never a wholesale
    /// rebuild. Refusals (unknown / not-expandable / disabled row) leave
    /// all state and counters untouched.
    pub(crate) fn toggle_expand(
        &mut self,
        row_id: StableId,
        rows: &HashMap<StableId, RowView>,
    ) -> Result<Option<WidgetEvent>, WidgetStateError> {
        let Some(view) = rows.get(&row_id) else {
            return Err(WidgetStateError::UnknownRow(row_id));
        };
        if !view.expandable {
            return Err(WidgetStateError::NotExpandable(row_id));
        }
        if !view.enabled {
            return Err(WidgetStateError::Disabled(row_id));
        }
        // Flip relative to the EFFECTIVE value: absent-default (collapsed)
        // → true; stored value → negated.
        let now = self.expanded.get(&row_id).copied().unwrap_or(false);
        let next = !now;
        self.expanded.insert(row_id, next);
        self.mutations.row_state_changes += 1;
        Ok(Some(WidgetEvent::TreeExpanded {
            surface_id: self.surface_id,
            node_id: row_id,
            expanded: next,
        }))
    }

    /// The [`WidgetEvent::ChildrenRequested`] for one lazy row — the lazy
    /// REQUEST is the event; the children themselves arrive later as a
    /// plugin PATCH (rows added under the parent via `WidgetOp::Add`).
    ///
    /// Emits ONLY when: the row declares `lazy_children`, the expansion is
    /// effectively ON, and no request for that parent is in flight. The
    /// pending entry IS the dedup window: a second request for the same
    /// parent is suppressed (`duplicate_requests_suppressed` += 1) until
    /// `sync` observes rows under the parent — no re-request storm. A
    /// collapse does NOT cancel the in-flight page (documented decision on
    /// `pending_children`); the window ends when the patch arrives or the
    /// parent row disappears.
    ///
    /// `range` is the parent's child window the mount wants
    /// (`range.len` clamped to [`MAX_WIDGET_RANGE_LEN`] so the emitted
    /// event is always VALID — bound through [`WidgetEvent::validate`]).
    pub(crate) fn request_children_if_lazy(
        &mut self,
        parent_id: StableId,
        range: Range<usize>,
        rows: &HashMap<StableId, RowView>,
    ) -> Option<WidgetEvent> {
        let Some(view) = rows.get(&parent_id) else {
            return None;
        };
        if !view.lazy_children {
            return None;
        }
        if !self.effective_expanded(parent_id, false) {
            return None;
        }
        if self.pending_children.contains_key(&parent_id) {
            self.mutations.duplicate_requests_suppressed += 1;
            return None;
        }
        let page = WidgetRange {
            start: range.start,
            len: range.end.saturating_sub(range.start).min(MAX_WIDGET_RANGE_LEN),
        };
        let event = WidgetEvent::ChildrenRequested {
            surface_id: self.surface_id,
            parent_id,
            range: page,
        };
        // Construct-level validity binds through the crate's own
        // validation — an invalid request is a defect and must not count
        // as emitted.
        debug_assert!(
            event.validate().is_ok(),
            "ChildrenRequested must be valid by construction"
        );
        self.pending_children.insert(parent_id, page);
        self.mutations.lazy_requests_emitted += 1;
        Some(event)
    }

    // -- selection ----------------------------------------------------------

    /// Replace the selection with `ids` (presentation ids, order kept).
    ///
    /// Enforces the tree's `multi_select` capability (single-select tree +
    /// multi-id input → refusal + NO state change) and the
    /// [`MAX_SELECTION_IDS`] bound (enforced state-side BEFORE the event
    /// exists). Lists are ungated at the capability level (see
    /// [`WidgetView`]).
    ///
    /// Mutation proof (§16): the write is diff-based —
    /// `row_state_changes` += removed + added (the rows the operation
    /// TOUCHED), never the whole list. `whole_state_resets` stays 0.
    pub(crate) fn set_selection(
        &mut self,
        ids: Vec<StableId>,
        widget_id: StableId,
        widget_view: &WidgetView,
    ) -> Result<Option<WidgetEvent>, WidgetStateError> {
        if ids.len() > MAX_SELECTION_IDS {
            return Err(WidgetStateError::SelectionTooLarge {
                offered: ids.len(),
            });
        }
        if ids.len() > 1 && !widget_view.multi_select {
            return Err(WidgetStateError::MultiSelectNotCapable {
                widget: widget_id,
                offered: ids.len(),
            });
        }
        // Diff against the previous selection: rows the operation touched.
        let previous: HashSet<StableId> = self.selection.iter().copied().collect();
        let next: HashSet<StableId> = ids.iter().copied().collect();
        let removed = previous.difference(&next).count();
        let added = next.difference(&previous).count();
        self.selection = ids;
        self.mutations.row_state_changes += (removed + added) as u64;
        let event = WidgetEvent::SelectionChanged {
            surface_id: self.surface_id,
            selected_ids: self.selection.clone(),
        };
        debug_assert!(
            event.validate().is_ok(),
            "SelectionChanged must be valid by construction"
        );
        Ok(Some(event))
    }

    // -- activation ----------------------------------------------------------

    /// Activate one enabled row/item (double-click / Enter). Returns the
    /// [`WidgetEvent::ItemActivated`] NOTIFICATION.
    ///
    /// ONE event per method (documented policy): activation does NOT
    /// synthesize a [`WidgetEvent::CommandInvoked`] even when the item
    /// carries a `command` — the MOUNT decides command semantics later
    /// (SWG-003); the plugin resolves the activation itself.
    pub(crate) fn item_activate(
        &mut self,
        row_id: StableId,
        rows: &HashMap<StableId, RowView>,
    ) -> Result<Option<WidgetEvent>, WidgetStateError> {
        let Some(view) = rows.get(&row_id) else {
            return Err(WidgetStateError::UnknownRow(row_id));
        };
        if !view.enabled {
            return Err(WidgetStateError::Disabled(row_id));
        }
        Ok(Some(WidgetEvent::ItemActivated {
            surface_id: self.surface_id,
            item_id: row_id,
        }))
    }

    // -- commands ------------------------------------------------------------

    /// Enablement check for one row/widget, consulting the synced view.
    ///
    /// This is the §16 "disabled commands un-invocable" rule's STATE half:
    /// [`WidgetUiState::command_click`] refuses through it before any event
    /// exists. Unknown ids are NOT enabled.
    pub(crate) fn is_enabled(&self, id: StableId, rows: &HashMap<StableId, RowView>) -> bool {
        rows.get(&id).is_some_and(|view| view.enabled)
    }

    /// Build + validate a [`WidgetEvent::CommandInvoked`] for one semantic
    /// command click (icon button, menu item — the dispatch WIRING is
    /// SWG-003's; the construction/gating lives here so the mount and MCP
    /// parity share one path).
    ///
    /// A disabled command is refused (`WidgetStateError::Disabled`) and NO
    /// event exists; empty/past-cap target sets are refused likewise. The
    /// built event is validated through [`WidgetEvent::validate`] — one
    /// validity path for GUI and MCP (§15).
    pub(crate) fn command_click(
        &self,
        command_id: String,
        targets: Vec<StableId>,
        rows: &HashMap<StableId, RowView>,
    ) -> Result<WidgetEvent, WidgetStateError> {
        let Some(&primary) = targets.first() else {
            return Err(WidgetStateError::NoTargets);
        };
        if targets.len() > MAX_WIDGET_TARGET_IDS {
            return Err(WidgetStateError::TooManyTargets {
                offered: targets.len(),
            });
        }
        if !self.is_enabled(primary, rows) {
            return Err(WidgetStateError::Disabled(primary));
        }
        let event = WidgetEvent::CommandInvoked {
            surface_id: self.surface_id,
            command_id,
            target_ids: targets,
        };
        // Construct-level validity (the empty-command-id arm of the
        // crate's validate) binds here too.
        event.validate().map_err(|_| WidgetStateError::NoTargets)?;
        Ok(event)
    }

    // -- focus (keyboard nav; roving-tabindex model) --------------------------

    /// Move focus to `id` (the mount's click/keyboard handler input).
    ///
    /// Disabled rows refuse focus (a focus ring on a dead control is a
    /// defect); unknown ids refuse too; a refusal leaves the current focus
    /// untouched.
    pub(crate) fn focus_set(
        &mut self,
        id: StableId,
        rows: &HashMap<StableId, RowView>,
    ) -> Result<(), WidgetStateError> {
        let Some(view) = rows.get(&id) else {
            return Err(WidgetStateError::UnknownRow(id));
        };
        if !view.enabled {
            return Err(WidgetStateError::Disabled(id));
        }
        self.focus = Some(id);
        Ok(())
    }

    /// Roving focus: from `current`, chain to the NEXT ENABLED row in
    /// `order` (the flat list's paint order), skipping disabled rows —
    /// Tab/Down navigation over a mixed enabled/disabled list lands on the
    /// next live control, not on a dead one. `None` when no enabled row
    /// exists after `current` (focus stays put; the mount may wrap).
    pub(crate) fn focus_next(
        &mut self,
        current: Option<StableId>,
        order: &[StableId],
        rows: &HashMap<StableId, RowView>,
    ) -> Option<StableId> {
        let start = current
            .and_then(|id| order.iter().position(|candidate| *candidate == id))
            .map_or(0, |position| position + 1);
        let candidate = order[start.min(order.len())..]
            .iter()
            .copied()
            .find(|id| self.is_enabled(*id, rows));
        if let Some(id) = candidate {
            self.focus = Some(id);
        }
        candidate
    }

    /// Roving focus backward: from `current` (or the END when `current` is
    /// `None`), chain to the PREVIOUS ENABLED row in `order`. `None` when
    /// none exists (focus stays put; the mount may wrap).
    pub(crate) fn focus_prev(
        &mut self,
        current: Option<StableId>,
        order: &[StableId],
        rows: &HashMap<StableId, RowView>,
    ) -> Option<StableId> {
        let start = current
            .and_then(|id| order.iter().position(|candidate| *candidate == id))
            .unwrap_or(order.len());
        let candidate = order[..start.min(order.len())]
            .iter()
            .rev()
            .copied()
            .find(|id| self.is_enabled(*id, rows));
        if let Some(id) = candidate {
            self.focus = Some(id);
        }
        candidate
    }

    // -- flat list (the windowing input derived from state) -------------------

    /// Flatten `def`'s rows under the CURRENT expansion state: children of
    /// collapsed rows contribute ZERO flat entries (the §16
    /// branch-isolation instrument). Deterministic depth-first order,
    /// payload order within each level. The contract default (collapsed)
    /// applies to rows the user never toggled.
    pub(crate) fn flat_rows_of_tree(
        &self,
        def: &WidgetTreeDef,
        rows: &HashMap<StableId, RowView>,
    ) -> Vec<FlatRow> {
        // Child buckets by parent id, in payload order.
        let mut children: HashMap<StableId, Vec<&TreeRowDef>> = HashMap::new();
        let mut roots: Vec<&TreeRowDef> = Vec::new();
        for row in &def.rows {
            match row.parent {
                Some(parent) => children.entry(parent).or_default().push(row),
                None => roots.push(row),
            }
        }
        let mut flat = Vec::new();
        self.flatten_row_slice(&roots, &children, 0, &mut flat, rows);
        flat
    }

    /// Flatten `def`'s items (a list has no expansion; the flat list IS the
    /// payload, one [`FlatRow`] per item).
    pub(crate) fn flat_rows_of_list(&self, def: &ItemListDef) -> Vec<FlatRow> {
        def.items
            .iter()
            .map(|item| FlatRow {
                id: item.id,
                depth: 0,
                height: 1.0,
            })
            .collect()
    }

    /// Depth-first flatten of one row slice; recursion depth is bounded by
    /// the parent chain, which the surface's own validation already caps.
    fn flatten_row_slice<'a>(
        &self,
        slice: &[&'a TreeRowDef],
        children: &HashMap<StableId, Vec<&'a TreeRowDef>>,
        depth: usize,
        out: &mut Vec<FlatRow>,
        rows: &HashMap<StableId, RowView>,
    ) {
        for row in slice {
            out.push(FlatRow {
                id: row.id,
                depth,
                height: 1.0,
            });
            if self.effective_expanded(row.id, false) {
                if let Some(kids) = children.get(&row.id) {
                    self.flatten_row_slice(kids, children, depth + 1, out, rows);
                }
            }
        }
    }

    // -- sync (structural reconcile across patches) ---------------------------

    /// Reconcile this state against a freshly-applied [`WidgetSurface`]
    /// (the mount calls this after every successful snapshot/patch swap).
    ///
    /// Structural identity is the [`StableId`] namespace (REG-1 spirit at
    /// the state layer): selection/focus/expansion SURVIVE patches that
    /// don't touch those ids; entries whose ids no longer exist are PRUNED;
    /// nothing is ADDED (identity = id, so a new row starts at its contract
    /// defaults).
    ///
    /// Lazy lifecycle: pending requests for parents that now HAVE rows
    /// under them are cleared (the patch arrived — the dedup window ends),
    /// which is what prevents a re-request storm; a pending entry whose
    /// parent vanished entirely is pruned too.
    ///
    /// Mutation-counting policy (documented): a prune counts as a
    /// `row_state_changes` write ONLY when the id was actually PRESENT in
    /// selection/expansion (that row's host-local state WAS removed); ids
    /// merely observed absent (never selected/expanded) cost nothing, and
    /// `whole_state_resets` never moves here — reconcile is not a teardown.
    /// Focus pruning is likewise uncounted: one focus SLOT, not row state.
    pub(crate) fn sync(&mut self, surface: &WidgetSurface) {
        let mut model = SyncedSurface {
            revision: surface.revision,
            live: HashSet::new(),
            views: HashMap::new(),
            widget_views: HashMap::new(),
            parents_with_rows: HashSet::new(),
        };
        collect_surface_rows(surface, &mut model);

        // Selection prune: keep ids that still exist, in order.
        let kept: Vec<StableId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| model.live.contains(id))
            .collect();
        let pruned = (self.selection.len() - kept.len()) as u64;
        self.selection = kept;
        debug_assert!(self.selection.len() <= MAX_SELECTION_IDS);
        self.mutations.row_state_changes += pruned;

        // Expansion prune: drop entries whose rows no longer exist.
        let mut pruned_expansions = 0u64;
        self.expanded.retain(|id, _| {
            let alive = model.live.contains(id);
            if !alive {
                pruned_expansions += 1;
            }
            alive
        });
        self.mutations.row_state_changes += pruned_expansions;

        // Focus prune: focus on a removed row falls back to None (the
        // mount re-anchors with focus_next/focus_prev; uncounted — one
        // focus slot, not row state).
        if let Some(id) = self.focus {
            if !model.live.contains(&id) {
                self.focus = None;
            }
        }

        // Lazy lifecycle: clear pending requests whose patch has arrived
        // (rows now exist under the parent) or whose parent vanished.
        self.pending_children.retain(|parent, _| {
            let parent_alive = model.live.contains(parent);
            let arrived = parent_alive && model.parents_with_rows.contains(parent);
            parent_alive && !arrived
        });

        self.synced = Some(model);
    }
}

/// The read model one [`WidgetUiState::sync`] captured from a surface.
#[derive(Debug, Clone)]
pub(crate) struct SyncedSurface {
    /// The surface revision the snapshot came from.
    pub revision: SceneRevision,
    /// Every live row/item/widget id (the surface-wide namespace).
    pub live: HashSet<StableId>,
    /// Per-row/item read views.
    pub views: HashMap<StableId, RowView>,
    /// Per-widget capability views (tree/list; keyed by widget id).
    pub widget_views: HashMap<StableId, WidgetView>,
    /// Widget/row ids that carry rows under them in the current payload —
    /// the lazy-arrival marker: a pending request for such a parent has
    /// been answered by a patch.
    pub parents_with_rows: HashSet<StableId>,
}

/// One surface walk (groups recursed): collect live ids, row views, widget
/// views, and the parents-with-rows marker set.
fn collect_surface_rows(surface: &WidgetSurface, model: &mut SyncedSurface) {
    fn walk(nodes: &[WidgetNode], model: &mut SyncedSurface) {
        for node in nodes {
            match node {
                WidgetNode::Tree(def) => {
                    model.live.insert(def.id);
                    let (view, row_views) = tree_parts_of(def);
                    model.widget_views.insert(def.id, view);
                    for row in &def.rows {
                        if let Some(parent) = row.parent {
                            model.parents_with_rows.insert(parent);
                        }
                    }
                    for (id, view) in row_views {
                        model.live.insert(id);
                        model.views.insert(id, view);
                    }
                }
                WidgetNode::List(def) => {
                    model.live.insert(def.id);
                    let (view, item_views) = list_parts_of(def);
                    model.widget_views.insert(def.id, view);
                    for (id, view) in item_views {
                        model.live.insert(id);
                        model.views.insert(id, view);
                    }
                }
                WidgetNode::Group(def) => {
                    model.live.insert(def.id);
                    walk(&def.children, model);
                }
                other => {
                    model.live.insert(other.widget_id());
                }
            }
        }
    }
    walk(&surface.roots, model);
}

// ---------------------------------------------------------------------------
// Tests (the §16 criteria, where they live in the state layer)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::scene::SceneRevision;
    use ocs_plugin_api::widgets::{ItemDef, ItemListDef, TreeCapabilities, WidgetTreeDef};

    // -- construction helpers ------------------------------------------------

    fn sid(n: u64) -> StableId {
        StableId(n)
    }

    fn row(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
        TreeRowDef {
            id: sid(id),
            parent: parent.map(sid),
            label: label.to_string(),
            icon: None,
            status: None,
            expandable: false,
            enabled: true,
            lazy_children: false,
            domain_ref: None,
        }
    }

    fn expandable_row(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
        TreeRowDef {
            expandable: true,
            ..row(id, parent, label)
        }
    }

    fn lazy_row(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
        TreeRowDef {
            expandable: true,
            lazy_children: true,
            ..row(id, parent, label)
        }
    }

    fn disabled_row(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
        TreeRowDef {
            enabled: false,
            ..row(id, parent, label)
        }
    }

    fn tree(id: u64, rows: Vec<TreeRowDef>, multi_select: bool) -> WidgetNode {
        WidgetNode::Tree(WidgetTreeDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            capabilities: TreeCapabilities {
                multi_select,
                filterable: false,
                lazy_children: true,
            },
            rows,
        })
    }

    fn item(id: u64, label: &str) -> ItemDef {
        ItemDef {
            id: sid(id),
            label: label.to_string(),
            icon: None,
            enabled: true,
            visible: true,
            command: None,
            domain_ref: None,
        }
    }

    fn list(id: u64, items: Vec<ItemDef>) -> WidgetNode {
        WidgetNode::List(ItemListDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            items,
        })
    }

    fn surface(roots: Vec<WidgetNode>) -> WidgetSurface {
        WidgetSurface {
            revision: SceneRevision(1),
            roots,
            tables: std::collections::BTreeMap::new(),
        }
    }

    fn tree_def_of(node: &WidgetNode) -> WidgetTreeDef {
        match node {
            WidgetNode::Tree(def) => def.clone(),
            _ => panic!("expected a tree node"),
        }
    }

    fn synced_of(state: &WidgetUiState) -> SyncedSurface {
        state.synced().expect("synced").clone()
    }

    fn assert_events_valid(events: &[WidgetEvent]) {
        for event in events {
            event
                .validate()
                .expect("every emitted event must pass the crate's own validate()");
        }
    }

    // -- 1. branch isolation ---------------------------------------------------

    /// §16: "Branch-expand changes only that branch's visible state."
    /// Mixed-depth tree; the ancestors are expanded first (separate,
    /// separately-counted toggles), then toggling ONE deep expandable row
    /// fires the TreeExpanded notification for that row, changes the flat
    /// list ONLY by that branch's rows (other entries byte-identical at
    /// their positions), and mutates exactly one row's state entry.
    #[test]
    fn expanding_one_branch_mutates_only_that_branch() {
        let mut state = WidgetUiState::new(sid(900));
        let root = surface(vec![tree(
            100,
            vec![
                expandable_row(1, None, "A"),
                row(11, Some(1), "A.1"),
                expandable_row(12, Some(1), "A.2"),
                row(121, Some(12), "A.2.a"),
                row(13, Some(1), "A.3"),
                row(2, None, "B"),
            ],
            false,
        )]);
        state.sync(&root);
        let views = synced_of(&state).views;
        let def = tree_def_of(&root.roots[0]);

        // Fully collapsed: roots only.
        assert_eq!(state.flat_rows_of_tree(&def, &views).len(), 2);

        // Expand the ancestors (their own toggles, counted separately).
        state.toggle_expand(sid(1), &views).expect("toggle ok");
        let before = state.flat_rows_of_tree(&def, &views);
        let ids_before: Vec<StableId> = before.iter().map(|row| row.id).collect();
        assert_eq!(ids_before, vec![sid(1), sid(11), sid(12), sid(13), sid(2)]);

        // Toggle the DEEP row A.2 (id 12): exactly its subtree appears.
        let changes_before = state.mutations().row_state_changes;
        let event = state
            .toggle_expand(sid(12), &views)
            .expect("toggle ok")
            .expect("TreeExpanded notification");
        assert_events_valid(&[event.clone()]);
        assert_eq!(
            event,
            WidgetEvent::TreeExpanded {
                surface_id: sid(900),
                node_id: sid(12),
                expanded: true,
            },
            "the notification names the toggled row with its new state"
        );

        let after = state.flat_rows_of_tree(&def, &views);
        // Flat-list delta == exactly that branch's rows (A.2.a), in place.
        assert_eq!(after.len(), before.len() + 1);
        assert_eq!(before[..3], after[..3], "entries before the branch untouched");
        assert_eq!(after[3].id, sid(121), "the branch's child appears right after its parent");
        assert_eq!(after[3].depth, 2, "child depth = parent depth + 1");
        assert_eq!(before[3..], after[4..], "entries after the branch untouched");

        // Mutation proof: the toggle wrote exactly ONE row-state entry.
        assert_eq!(state.mutations().row_state_changes - changes_before, 1);
        assert_eq!(state.mutations().whole_state_resets, 0, "no full-state replacement");
    }

    /// §16: "Selection updates do not rebuild the whole tree" — the write
    /// is diff-based, and no-op rewrites touch nothing.
    #[test]
    fn selection_updates_do_not_rebuild_whole_state() {
        let mut state = WidgetUiState::new(sid(900));
        let items: Vec<ItemDef> = (1..=500).map(|i| item(i, &format!("item {i}"))).collect();
        let root = surface(vec![list(100, items)]);
        state.sync(&root);
        let widget_view = synced_of(&state).widget_views[&sid(100)];

        // 2-of-500 selection: touches ONLY the diff rows (≤ 4), not 500.
        let event = state
            .set_selection(vec![sid(7), sid(42)], sid(100), &widget_view)
            .expect("a list accepts multi-id selection")
            .expect("SelectionChanged notification");
        assert_events_valid(&[event.clone()]);
        assert_eq!(
            event,
            WidgetEvent::SelectionChanged {
                surface_id: sid(900),
                selected_ids: vec![sid(7), sid(42)],
            }
        );
        assert_eq!(state.mutations().row_state_changes, 2, "added 2, removed 0 — not 500");
        assert_eq!(state.mutations().whole_state_resets, 0);

        // A further partial change mutates only the delta.
        state
            .set_selection(vec![sid(42), sid(99)], sid(100), &widget_view)
            .expect("second write ok");
        assert_eq!(state.mutations().row_state_changes, 4, "removed 7 (+1), added 99 (+1)");
        assert_eq!(state.selection(), &[sid(42), sid(99)]);

        // Identical rewrite: zero delta, zero row-state writes.
        state
            .set_selection(vec![sid(42), sid(99)], sid(100), &widget_view)
            .expect("identical rewrite ok");
        assert_eq!(state.mutations().row_state_changes, 4, "no rows touched by a no-op write");
        assert_eq!(state.mutations().whole_state_resets, 0);
    }

    /// §16: "Lazy children arrive incremental with stable identities" —
    /// exactly ONE ChildrenRequested per expand-on; the pending dedup
    /// window suppresses re-requests while in flight (collapse/expand
    /// cycles included); `sync` clears the window when the patch arrives
    /// (no re-request storm); a re-expand after arrival may request again.
    #[test]
    fn lazy_children_request_once_and_suppress_dups() {
        let mut state = WidgetUiState::new(sid(900));
        let root = surface(vec![tree(
            100,
            vec![row(1, None, "static"), lazy_row(2, None, "lazy branch")],
            false,
        )]);
        state.sync(&root);
        let views = synced_of(&state).views;

        // Expand-on of the lazy row: the mount then requests the parent's
        // child window. Exactly ONE request is emitted.
        state.toggle_expand(sid(2), &views).expect("toggle ok");
        let request = state
            .request_children_if_lazy(sid(2), 0..MAX_WIDGET_RANGE_LEN, &views)
            .expect("the first request goes out");
        assert_events_valid(&[request.clone()]);
        assert_eq!(
            request,
            WidgetEvent::ChildrenRequested {
                surface_id: sid(900),
                parent_id: sid(2),
                range: WidgetRange {
                    start: 0,
                    len: MAX_WIDGET_RANGE_LEN,
                },
            },
            "the full documented range (len ≤ MAX_WIDGET_RANGE_LEN)"
        );
        assert_eq!(state.mutations().lazy_requests_emitted, 1);

        // While pending, collapse/expand cycles CANNOT re-request (dedup):
        // each expand-on's request attempt is suppressed.
        for _ in 0..3 {
            state.toggle_expand(sid(2), &views).expect("collapse ok"); // off
            state.toggle_expand(sid(2), &views).expect("re-expand ok"); // on
            assert!(
                state
                    .request_children_if_lazy(sid(2), 0..MAX_WIDGET_RANGE_LEN, &views)
                    .is_none(),
                "no re-request while the first page is in flight"
            );
        }
        assert_eq!(state.mutations().lazy_requests_emitted, 1);
        assert_eq!(state.mutations().duplicate_requests_suppressed, 3);

        // The arrival is a PATCH (rows added under the lazy parent via
        // WidgetOp::Add); sync observes rows under the parent → the
        // pending window clears.
        let patched = surface(vec![tree(
            100,
            vec![
                row(1, None, "static"),
                lazy_row(2, None, "lazy branch"),
                row(21, Some(2), "child a"),
                row(22, Some(2), "child b"),
            ],
            false,
        )]);
        state.sync(&patched);
        assert!(
            !state.pending_children().contains_key(&sid(2)),
            "the arriving patch clears the pending dedup window"
        );

        // Collapse + re-expand AFTER the patch: a NEW request is allowed.
        let views = synced_of(&state).views;
        state.toggle_expand(sid(2), &views).expect("collapse ok"); // off
        state.toggle_expand(sid(2), &views).expect("re-expand ok"); // on
        let request = state
            .request_children_if_lazy(sid(2), 0..MAX_WIDGET_RANGE_LEN, &views)
            .expect("a re-expand after arrival may request again");
        assert_events_valid(&[request]);
        assert_eq!(state.mutations().lazy_requests_emitted, 2);
        assert_eq!(state.mutations().duplicate_requests_suppressed, 3);
    }

    // -- 2. windowing -----------------------------------------------------------

    /// §16: "100k backing objects do not create 100k host widgets" — the
    /// flat list of 100k rows materializes ONLY the viewport window
    /// (+ margin): a few dozen ids, not 100k.
    #[test]
    fn windowing_of_100k_backing_items_materializes_window_only() {
        let flat = flat_len_of_large_list(100_000);
        let spec = WindowSpec {
            viewport_height: 600.0,
            row_height: 24.0,
            margin_rows: 5,
        };
        // 600 / 24 = 25 visible rows + 5 margin each side = exactly 35 ids.
        let window = visible_window(&flat, 40_000, &spec);
        let ids = materialize_ids(&flat, window);
        assert_eq!(ids.len(), 35, "window = visible rows + margins, nothing more");
        assert_eq!(ids[0], sid(39_996), "the margin starts 5 rows above the anchor");
        assert_eq!(*ids.last().expect("nonempty"), sid(40_030));

        // Scrolled to the very end: the window clamps at the list end.
        let window = visible_window(&flat, 99_999, &spec);
        let ids = materialize_ids(&flat, window);
        assert_eq!(window.end, 100_000, "the window never hangs past the flat list");
        assert_eq!(ids.last().copied(), Some(sid(100_000)));
        assert!(ids.len() < 100, "windowing, not wholesale materialization");

        // The 100k-row flat list itself was built without widgets.
        assert_eq!(flat.len(), 100_000);
    }

    /// §16: "Branch-expand changes only that branch's visible state" (the
    /// tree case): children of COLLAPSED nodes contribute ZERO flat rows in
    /// a mixed-depth fixture; expanding the root exposes exactly its
    /// direct subtree in payload order.
    #[test]
    fn collapsed_children_contribute_zero_rows() {
        let mut state = WidgetUiState::new(sid(900));
        let root = surface(vec![tree(
            100,
            vec![
                expandable_row(1, None, "root A"),
                row(11, Some(1), "A.1"),
                expandable_row(12, Some(1), "A.2"),
                row(121, Some(12), "A.2.a"),
                row(13, Some(1), "A.3"),
                expandable_row(2, None, "root B"),
                row(21, Some(2), "B.1"),
            ],
            false,
        )]);
        state.sync(&root);
        let views = synced_of(&state).views;
        let def = tree_def_of(&root.roots[0]);

        // Fully collapsed: 7 payload rows → exactly 2 flat rows.
        let flat = state.flat_rows_of_tree(&def, &views);
        assert_eq!(flat.len(), 2, "collapsed children contribute ZERO rows");
        assert_eq!(flat[0].id, sid(1));
        assert_eq!(flat[1].id, sid(2));

        // Expand root A: exactly A's DIRECT subtree appears (A.2 stays
        // collapsed, so A.2.a contributes nothing); root B untouched.
        state.toggle_expand(sid(1), &views).expect("toggle ok");
        let flat = state.flat_rows_of_tree(&def, &views);
        let ids: Vec<StableId> = flat.iter().map(|row| row.id).collect();
        assert_eq!(ids, vec![sid(1), sid(11), sid(12), sid(13), sid(2)]);
        assert_eq!(flat[2].depth, 1, "direct children nest one under the root");
        assert!(!ids.contains(&sid(121)), "collapsed grandchild contributes zero rows");

        // Expand A.2: exactly its branch appears.
        state.toggle_expand(sid(12), &views).expect("toggle ok");
        let flat = state.flat_rows_of_tree(&def, &views);
        let ids: Vec<StableId> = flat.iter().map(|row| row.id).collect();
        assert_eq!(ids, vec![sid(1), sid(11), sid(12), sid(121), sid(13), sid(2)]);
        assert_eq!(flat[3].depth, 2);
    }

    // -- 3. focus ----------------------------------------------------------------

    /// §16: "keyboard + a11y work without pointer input" — roving focus
    /// chains through NEXT ENABLED rows (skipping disabled ones), focus
    /// starts as `None`, a disabled row refuses focus, and expansion is
    /// refused on a disabled expandable row (no event, no mutation).
    #[test]
    fn keyboard_focus_roving_and_disabled_expansion() {
        let mut state = WidgetUiState::new(sid(900));
        let root = surface(vec![tree(
            100,
            vec![
                row(1, None, "first"),
                disabled_row(2, None, "disabled"),
                row(3, None, "third"),
                disabled_row(4, None, "also disabled"),
                TreeRowDef {
                    enabled: false,
                    ..expandable_row(5, None, "disabled expandable")
                },
                row(6, None, "last"),
            ],
            false,
        )]);
        state.sync(&root);
        let views = synced_of(&state).views;
        let order: Vec<StableId> = vec![sid(1), sid(2), sid(3), sid(4), sid(5), sid(6)];

        // Focus STARTS as None.
        assert_eq!(state.focus(), None);

        // First Tab: the first ENABLED row.
        let first = state.focus_next(None, &order, &views).expect("an enabled row exists");
        assert_eq!(first, sid(1));
        // Next: skips the disabled row 2 → 3; skips 4 and 5 → 6.
        assert_eq!(state.focus_next(Some(first), &order, &views), Some(sid(3)));
        assert_eq!(state.focus_next(Some(sid(3)), &order, &views), Some(sid(6)));
        // Past the end: None (focus stays; the mount may wrap).
        assert_eq!(state.focus_next(Some(sid(6)), &order, &views), None);
        assert_eq!(state.focus(), Some(sid(6)), "a failed nav leaves focus put");

        // Backward: 6 → 3 (skips 5, 4) → 1.
        assert_eq!(state.focus_prev(Some(sid(6)), &order, &views), Some(sid(3)));
        assert_eq!(state.focus_prev(Some(sid(3)), &order, &views), Some(sid(1)));
        assert_eq!(state.focus_prev(Some(sid(1)), &order, &views), None);

        // Disabled rows refuse focus directly.
        assert_eq!(
            state.focus_set(sid(5), &views),
            Err(WidgetStateError::Disabled(sid(5)))
        );
        assert_eq!(state.focus(), Some(sid(1)), "refused focus leaves state untouched");

        // Disabled EXPAND attempt → Err + NO event + NO mutation.
        let mutations_before = *state.mutations();
        assert_eq!(
            state.toggle_expand(sid(5), &views),
            Err(WidgetStateError::Disabled(sid(5))),
            "a disabled expandable row cannot toggle"
        );
        assert_eq!(*state.mutations(), mutations_before, "a refusal moves NO counter");
        assert_eq!(state.explicit_expansion(sid(5)), None, "no expansion entry stored");

        // A disabled row cannot be activated either.
        assert_eq!(
            state.item_activate(sid(2), &views),
            Err(WidgetStateError::Disabled(sid(2)))
        );
    }

    // -- 4. sync (structural reconcile) -------------------------------------------

    /// REG-1 spirit at the state layer: a patch that REMOVES 2 selected ids
    /// and ADDS 10 others shrinks selection by exactly 2; the added ids are
    /// NOT auto-selected; dangling expansion entries are pruned likewise.
    #[test]
    fn sync_prunes_dangling_identifiers_only() {
        let mut state = WidgetUiState::new(sid(900));
        let items: Vec<ItemDef> = (1..=12).map(|i| item(i, &format!("item {i}"))).collect();
        let root = surface(vec![list(100, items)]);
        state.sync(&root);

        // Select 4; focus one of the survivors-to-be.
        let widget_view = synced_of(&state).widget_views[&sid(100)];
        state
            .set_selection(vec![sid(3), sid(5), sid(8), sid(11)], sid(100), &widget_view)
            .expect("capable list");
        state.focus_set(sid(5), &synced_of(&state).views).expect("focus ok");

        // Patch: the list now carries 3 survivors + 10 NEW ids (5 and 8
        // among the removed).
        let mut patched_items: Vec<ItemDef> = vec![item(1, "1"), item(3, "3"), item(11, "11")];
        for i in 100u64..110 {
            patched_items.push(item(i, &format!("new {i}")));
        }
        let patched = surface(vec![list(100, patched_items)]);
        let changes_before = state.mutations().row_state_changes;
        state.sync(&patched);

        // Selection shrinks by exactly 2; the 10 added ids are NOT selected.
        assert_eq!(state.selection(), &[sid(3), sid(11)]);
        assert_eq!(
            state.mutations().row_state_changes - changes_before,
            2,
            "prunes count ONLY for ids that were actually present in selection"
        );
        // Focus on the removed id 5 falls back to None.
        assert_eq!(state.focus(), None, "dangling focus prunes to None");

        // Tree-side: expansion entries of removed rows prune likewise.
        let mut state2 = WidgetUiState::new(sid(901));
        let tree_root = surface(vec![tree(
            100,
            vec![expandable_row(1, None, "A"), row(11, Some(1), "A.1")],
            false,
        )]);
        state2.sync(&tree_root);
        let views = synced_of(&state2).views;
        state2.toggle_expand(sid(1), &views).expect("expand ok");
        assert_eq!(state2.explicit_expansion(sid(1)), Some(true));

        // Patch REPLACES the tree's rows: id 1 no longer exists.
        let patched_tree = surface(vec![tree(
            100,
            vec![expandable_row(50, None, "new root")],
            false,
        )]);
        let changes_before = state2.mutations().row_state_changes;
        state2.sync(&patched_tree);
        assert_eq!(
            state2.explicit_expansion(sid(1)),
            None,
            "dangling expansion entry pruned"
        );
        assert_eq!(
            state2.mutations().row_state_changes - changes_before,
            1,
            "the pruned expansion entry counted; the new root added NOTHING (identity = id)"
        );
    }

    /// REG-1 state preservation: a patch that renames rows ELSEWHERE (same
    /// ids) leaves selection, expansion, and focus untouched — the synced
    /// views update, the state objects survive verbatim.
    #[test]
    fn state_survives_untouched_patch() {
        let mut state = WidgetUiState::new(sid(900));
        let root = surface(vec![tree(
            100,
            vec![
                expandable_row(1, None, "A"),
                row(11, Some(1), "A.1"),
                expandable_row(2, None, "B"),
                row(21, Some(2), "B.1"),
            ],
            true,
        )]);
        state.sync(&root);
        let views = synced_of(&state).views;
        state.toggle_expand(sid(1), &views).expect("expand ok");
        let widget_view = synced_of(&state).widget_views[&sid(100)];
        state
            .set_selection(vec![sid(11), sid(21)], sid(100), &widget_view)
            .expect("multi-select capable");
        state.focus_set(sid(21), &views).expect("focus ok");
        state.set_scroll_row(3);
        let (selection, expansion, focus, scroll) = (
            state.selection().to_vec(),
            state.explicit_expansion(sid(1)),
            state.focus(),
            state.scroll_row(),
        );

        // Patch: rename A and A.1 (same ids, same structure).
        let patched = surface(vec![tree(
            100,
            vec![
                expandable_row(1, None, "A (renamed)"),
                row(11, Some(1), "A.1 renamed"),
                expandable_row(2, None, "B"),
                row(21, Some(2), "B.1"),
            ],
            true,
        )]);
        state.sync(&patched);

        // State objects identical after the untouched patch.
        assert_eq!(state.selection(), &selection);
        assert_eq!(state.explicit_expansion(sid(1)), expansion);
        assert_eq!(state.focus(), focus);
        assert_eq!(state.scroll_row(), scroll);
        assert_eq!(state.mutations().whole_state_resets, 0, "reconcile is NOT a reset");
    }

    // -- 5. mutation counters ------------------------------------------------------

    /// The counters are the §16 DEFECT PROBE: across every flow in this
    /// module `whole_state_resets` stays 0; it moves ONLY through the
    /// explicit `reset_all` teardown helper (closing surfaces).
    #[test]
    fn mutation_counters_are_the_defect_probe() {
        let mut state = WidgetUiState::new(sid(900));
        let root = surface(vec![tree(
            100,
            vec![expandable_row(1, None, "A"), row(11, Some(1), "A.1"), row(2, None, "B")],
            true,
        )]);
        state.sync(&root);
        let views = synced_of(&state).views;
        let widget_view = synced_of(&state).widget_views[&sid(100)];

        // Every normal flow: expansion, selection, focus, activation,
        // commands, windowing, re-sync — zero resets.
        state.toggle_expand(sid(1), &views).expect("expand ok");
        state
            .set_selection(vec![sid(11), sid(2)], sid(100), &widget_view)
            .expect("select ok");
        state.focus_set(sid(2), &views).expect("focus ok");
        state.item_activate(sid(2), &views).expect("activate ok");
        let command = state
            .command_click("delete".into(), vec![sid(2)], &views)
            .expect("enabled target");
        assert_events_valid(&[command]);
        state.set_scroll_row(3);
        let def = tree_def_of(&root.roots[0]);
        let flat = state.flat_rows_of_tree(&def, &views);
        let window = visible_window(
            &flat,
            0,
            &WindowSpec {
                viewport_height: 100.0,
                row_height: 24.0,
                margin_rows: 2,
            },
        );
        let _ids = materialize_ids(&flat, window);
        state.sync(&root);
        assert_eq!(
            state.mutations().whole_state_resets,
            0,
            "no normal flow rebuilds whole state"
        );
        assert!(state.mutations().row_state_changes > 0, "row writes were counted instead");

        // Closing the surface: the ONLY path that fires the reset counter.
        state.reset_all();
        assert_eq!(state.mutations().whole_state_resets, 1);
        assert!(state.selection().is_empty());
        assert_eq!(state.focus(), None);
        assert!(state.expanded.is_empty());
        assert!(state.pending_children().is_empty());
        assert_eq!(state.scroll_row(), 0);
    }

    // -- 6. capability boundaries ---------------------------------------------------

    /// §16 capability boundary: single-select tree + multi-id input → Err +
    /// zero mutations; ≤ [`MAX_SELECTION_IDS`] enforced; command events are
    /// validated through the crate's own validate (bounds NOT
    /// re-implemented here — the state layer ADDS only capability checks).
    #[test]
    fn multi_select_capability_boundary() {
        let mut state = WidgetUiState::new(sid(900));
        let root = surface(vec![tree(
            100,
            vec![row(1, None, "A"), row(2, None, "B"), row(3, None, "C")],
            false, // single-select tree
        )]);
        state.sync(&root);
        let views = synced_of(&state).views;
        let widget_view = synced_of(&state).widget_views[&sid(100)];

        // Multi-id input on a single-select tree: refused, NOTHING changes.
        let mutations_before = *state.mutations();
        assert_eq!(
            state.set_selection(vec![sid(1), sid(2)], sid(100), &widget_view),
            Err(WidgetStateError::MultiSelectNotCapable {
                widget: sid(100),
                offered: 2,
            })
        );
        assert_eq!(*state.mutations(), mutations_before, "a refused write moves NO counter");
        assert!(state.selection().is_empty());

        // A single id IS acceptable on the single-select tree.
        state
            .set_selection(vec![sid(2)], sid(100), &widget_view)
            .expect("single id ok");
        assert_eq!(state.selection(), &[sid(2)]);

        // Past the MAX_SELECTION_IDS bound: refused state-side, before any
        // event exists.
        let too_many: Vec<StableId> = (0..MAX_SELECTION_IDS as u64 + 1).map(sid).collect();
        assert_eq!(
            state.set_selection(too_many, sid(100), &widget_view),
            Err(WidgetStateError::SelectionTooLarge {
                offered: MAX_SELECTION_IDS + 1,
            })
        );

        // The boundary event itself stays valid through the crate's own
        // validate (mirror of widget.rs's event bounds — called, not
        // re-implemented).
        let at_cap: Vec<StableId> = (0..MAX_SELECTION_IDS as u64).map(sid).collect();
        let event = WidgetEvent::SelectionChanged {
            surface_id: sid(900),
            selected_ids: at_cap,
        };
        event.validate().expect("the cap-sized event is valid");

        // Over-cap COMMAND targets are refused state-side too.
        let targets: Vec<StableId> = (0..MAX_WIDGET_TARGET_IDS as u64 + 1).map(sid).collect();
        assert!(matches!(
            state.command_click("cmd".into(), targets, &views),
            Err(WidgetStateError::TooManyTargets { .. })
        ));
        // Empty targets: refused (a semantic command names what it acts on).
        assert_eq!(
            state.command_click("cmd".into(), Vec::new(), &views),
            Err(WidgetStateError::NoTargets)
        );
        // A valid command event builds + validates through the crate.
        let event = state
            .command_click("delete".into(), vec![sid(1)], &views)
            .expect("enabled target, bounded command");
        assert_events_valid(&[event]);
    }

    // -- 7. misc state-layer invariants ------------------------------------------------

    /// Unknown ids refuse everything (no phantom state for ids outside the
    /// synced surface); a toggle on a NON-expandable row refuses; item
    /// activation emits exactly one ItemActivated.
    #[test]
    fn unknown_and_non_expandable_rows_refuse() {
        let mut state = WidgetUiState::new(sid(900));
        let root = surface(vec![tree(
            100,
            vec![row(1, None, "plain"), row(2, None, "also plain")],
            false,
        )]);
        state.sync(&root);
        let views = synced_of(&state).views;

        assert_eq!(
            state.toggle_expand(sid(99), &views),
            Err(WidgetStateError::UnknownRow(sid(99))),
            "unknown ids refuse"
        );
        assert_eq!(
            state.toggle_expand(sid(1), &views),
            Err(WidgetStateError::NotExpandable(sid(1))),
            "non-expandable rows refuse"
        );
        assert!(state.expanded.is_empty(), "refusals store nothing");
        assert_eq!(state.mutations().row_state_changes, 0, "refusals write nothing");

        let event = state
            .item_activate(sid(1), &views)
            .expect("activate ok")
            .expect("notification");
        assert_eq!(
            event,
            WidgetEvent::ItemActivated {
                surface_id: sid(900),
                item_id: sid(1),
            }
        );
        assert_events_valid(&[event]);
    }

    /// `effective_expanded` honors the absent-default: untouched rows read
    /// as the caller-stated contract default (collapsed here), an explicit
    /// toggle overrides, and `toggle_expand` flips relative to the
    /// EFFECTIVE value (first toggle on an absent-default-collapsed row
    /// stores true).
    #[test]
    fn expansion_defaults_and_effective_reads() {
        let mut state = WidgetUiState::new(sid(900));
        assert_eq!(state.effective_expanded(sid(1), false), false);
        assert_eq!(state.effective_expanded(sid(1), true), true, "the caller states the default");

        let root = surface(vec![tree(100, vec![expandable_row(1, None, "A")], false)]);
        state.sync(&root);
        let views = synced_of(&state).views;

        // Absent-default honored: no entry, reads collapsed.
        assert_eq!(state.explicit_expansion(sid(1)), None);
        assert_eq!(state.effective_expanded(sid(1), false), false);

        // First toggle flips the effective value → true.
        state.toggle_expand(sid(1), &views).expect("toggle ok");
        assert_eq!(state.explicit_expansion(sid(1)), Some(true));
        assert_eq!(state.effective_expanded(sid(1), false), true);

        // Second toggle flips again → false.
        state.toggle_expand(sid(1), &views).expect("toggle ok");
        assert_eq!(state.explicit_expansion(sid(1)), Some(false));
        assert_eq!(state.effective_expanded(sid(1), false), false);
    }

    /// Windowing degeneracies: an empty flat list, a viewport taller than
    /// the list, and a degenerate row height all stay bounded; the
    /// `contains`/`len` helpers agree with `materialize_ids`.
    #[test]
    fn window_basics_and_degenerate_specs() {
        let flat: Vec<FlatRow> = (1..=10)
            .map(|i| FlatRow {
                id: sid(i),
                depth: 0,
                height: 1.0,
            })
            .collect();
        let spec = WindowSpec {
            viewport_height: 50.0,
            row_height: 10.0,
            margin_rows: 2,
        };
        // 5 visible rows + 2 margin, anchored at index 4 → [2, 10) (end
        // clamped to the list length).
        let window = visible_window(&flat, 4, &spec);
        assert_eq!(window.start, 2, "margin rows before the anchor");
        assert_eq!(window.end, 10, "anchor + visible + margin, clamped to the list");
        let ids = materialize_ids(&flat, window);
        assert_eq!(ids.len(), window.len());
        assert_eq!(ids.last().copied(), Some(sid(10)));
        assert!(window.contains(4));
        assert!(!window.contains(1), "below the window start");

        // Viewport taller than the list: pinned to 0, covers everything.
        let window = visible_window(
            &flat,
            0,
            &WindowSpec {
                viewport_height: 1000.0,
                row_height: 10.0,
                margin_rows: 2,
            },
        );
        assert_eq!(window.start, 0);
        assert_eq!(window.end, 10);

        // Degenerate row height: falls back to 1 visible row, never a
        // divide-by-zero or an infinite window.
        let window = visible_window(
            &flat,
            5,
            &WindowSpec {
                viewport_height: 100.0,
                row_height: 0.0,
                margin_rows: 3,
            },
        );
        assert_eq!(window.len(), 1 + 3 + 3);
        assert_eq!(materialize_ids(&flat, window).len(), window.len());

        // Empty flat list: an empty window, empty ids.
        let empty: Vec<FlatRow> = Vec::new();
        let window = visible_window(&empty, 0, &spec);
        assert!(window.is_empty());
        assert!(materialize_ids(&empty, window).is_empty());
    }

    /// The 100k fixture builder: `n` flat rows built CHEAPLY — no widgets,
    /// no payloads, just the windowing model.
    fn flat_len_of_large_list(n: usize) -> Vec<FlatRow> {
        (1..=n as u64)
            .map(|i| FlatRow {
                id: StableId(i),
                depth: 0,
                height: 1.0,
            })
            .collect()
    }
}
