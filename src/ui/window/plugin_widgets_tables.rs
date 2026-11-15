//! Renderer-free STATE projection of plugin typed tables (`SWG-004b` of the
//! scene-UI architecture effort; ARCHITECTURE §§12–16 + ADR-005 read-only in
//! project-management).
//!
//! This is the TABLE sibling of [`crate::ui::window::plugin_widgets`]
//! (SWG-002 — THE discipline template: `WidgetUiState`, mutation counters as
//! defect probes, one event per method, `Result` refusals) and
//! [`crate::ui::window::plugin_widgets_dispatch`] (SWG-003 — the
//! dispatch/settle discipline; composed via imports, its state machine is
//! DONE and frozen and is NOT touched here). The plugin owns the semantic
//! TABLE data (the [`PanelTableDef`] registry riding each
//! [`WidgetSurface::tables`] map — SWG-004's crate types), and the host owns
//! the STATE: which rows are selected, which row/cell has keyboard focus,
//! which window of a paged table is materialized, which page fetches and
//! cell facts are in flight, and the editing DRAFT of one cell. Nothing here
//! constructs an iced widget, a `Message`, or any renderer type; the
//! typed-table MOUNT is a LATER integration task — it will consume this
//! machine exactly the way `plugin_widget_mount.rs` (SCN-013) consumes the
//! SWG-002/003 state layers today (that mount integration point is
//! DOCUMENTED here, not implemented: this is the state-layer sibling of the
//! table mount; SCN-014's bridge owns `src/app/**` + the mount files).
//!
//! # State-class discipline (ARCHITECTURE §3 / §14)
//!
//! Everything [`TableUiState`] holds is transient host state (class 3):
//! selection of row keys, roving focus, the scroll index, the editing
//! draft, and the in-flight page/fact windows. It is NEVER serialized into
//! scene or widget data. Row keys are STABLE identities (ADR-005; the §13
//! WGT-ID rule: transient indices NEVER identities) — every store here is
//! keyed by [`StableId`]; the only `usize` in the state is
//! [`TableUiState::scroll_index`], the windowing input of a moment in time,
//! never an identity.
//!
//! # Selection (documented decision: tables are HOST-multi-select)
//!
//! `PanelTableDef` carries NO selection-capability field (unlike the tree's
//! `TreeCapabilities::multi_select` —
//! [`ocs_plugin_api::widgets::TreeCapabilities::multi_select`]). DECIDED
//! here: tables are
//! HOST-multi-select by default with the SAME [`MAX_SELECTION_IDS`] bound —
//! the LIST precedent (lists carry no selection-capability contract either;
//! the host-native selection is bounded user interaction). SINGLE-select
//! mode is a FUTURE def capability (a capability field appended to the
//! crate's `PanelTableDef`); this state layer already funnels every
//! selection write through one method ([`TableUiState::set_selection`]) so
//! that gate lands as one refusal there, with no model change.
//!
//! The emitted event is [`WidgetEvent::SelectionChanged`] with ROW KEYS as
//! `selected_ids` — the per-table identity namespace, NOT widget ids. The
//! crate's ONE SelectionChanged vocabulary spans tree/list/table and
//! carries no control id (a pinned enum — documented crate limitation);
//! the mount forwards per surface and the plugin's own id namespaces
//! correlate in practice. A table-specific variant is a future crate
//! append and never this layer's concern.
//!
//! # The editing draft is NOT the data of record
//!
//! The def's rows are the data-of-record for the mount's read model. A cell
//! edit draft lives HERE until commit ([`CellDraft`]: the address
//! (row key, column) + the pending [`WidgetCellValue`], seeded from the
//! CURRENT cell of that address). Commit produces ONE [`WidgetOp::TableCell`]
//! FACT (the typed patch data of the crate's documented events-vs-patches
//! split) and counts ONE row. The MOUNT hands the op to the surface's
//! apply path (the plugin patch pipeline — SCN-014's page plumbing; the
//! documented hand-off) and settles it here afterwards. The
//! [`WidgetEvent::TableCellChanged`] skeleton is deliberately NOT emitted
//! by this layer: the notify leg belongs to the plugin/read side ("one
//! event per method" — the commit's output is the FACT, exactly one).
//!
//! [`TableUiState::cancel_cell_draft`] is FREE: a cancelled draft touched
//! nothing, NO counter knows a draft ever existed, and nothing else in the
//! state moves (transient host state class 3; the counter must NOT move —
//! documented policy below).
//!
//! # The dispatcher discipline (SWG-003, mirrored)
//!
//! The committed fact is IN FLIGHT until the mount settles it — the exact
//! in-flight/settle rule of [`crate::ui::window::plugin_widgets_dispatch`]
//! (`WidgetDispatcher::dispatch` / `settle`): a second
//! [`TableUiState::begin_cell_edit`] of the same cell refuses while a draft
//! is armed OR the cell's fact is unsettled (no double facts, no
//! overwritten drafts — a double click cannot double-write); a draft on a
//! DIFFERENT address is free once the previous one is cancelled/committed.
//! [`TableUiState::settle`] is the mount's explicit post-forward signal;
//! sync NEVER closes cell facts (a sync cannot prove the fact applied —
//! the settle rule has zero ambiguity; documented decision).
//!
//! # Event validity (§16 / §15)
//!
//! Every constructed EVENT is bound through the crate's own
//! [`WidgetEvent::validate`] BEFORE it counts as emitted (the
//! construct-level companion of the bounded decoders — SWG-002's rule
//! verbatim). Value validity of the draft: the crate's own per-cell checker
//! (`check_table_cell`) is private to the crate, so this layer delegates
//! the WHOLE VALUE CHECK through the crate's PUBLIC [`PanelTableDef::validate`]
//! — a throwaway 1×1 probe def whose validate runs that checker UNMODIFIED
//! (see `probe_typed_cell`). NO cap/finiteness arithmetic is re-implemented
//! here; the ONLY local validation is the three-arm kind-vs-value-class
//! comparison that keeps typed refusals ([`TableStateError::KindMismatch`])
//! distinct from value refusals ([`TableStateError::InvalidCellValue`]).
//! The probe's failure reason is the crate's OWN reason, carried verbatim.
//!
//! # Windowing (the 100k invariant, made STRUCTURAL)
//!
//! [`TableUiState::materialize_rows`] windows THE PUBLISHED keys (the
//! def's `rows` — the plugin's currently-published window, ≤
//! `MAX_WIDGET_ROWS_INITIAL` per payload) with the SWG-002 windowing
//! arithmetic — [`crate::ui::window::plugin_widgets::visible_window`] and
//! `materialize_ids` are composed VERBATIM (never re-declared: one
//! implementation of the window math serves trees, lists, AND tables). The
//! def's `total_rows` (the backing set) NEVER enters the flat keys list:
//! a 100_000-row backing table with a 4_096-row published window
//! materializes a few dozen keys, NOT the backing set, BY STRUCTURE (there
//! is no code path yielding keys beyond the published rows — "100k backing
//! objects do not create 100k host widgets"). Rows BEYOND the published
//! window are fetched through [`WidgetEvent::TablePageRequested`]
//! (the keyed-window request; keys are learned from the published
//! registry) and arrive as plugin patches, EXTENDING the published window
//! via sync (see [`TableUiState::sync`]'s arrival logic + counter notes).
//!
//! # The §16 proof instruments (counters)
//!
//! ARCHITECTURE §16 names the defects this layer must make visible, and
//! the counters carry them from day one (renderer-free): "selection
//! updates do not rebuild the whole tree" (diff-based counting — a
//! selection write counts removed + added, an identical rewrite counts 0);
//! "one cell edit transfers only its cell event/patch" (one row per
//! commit; the byte-size probes live in the tests below); "closing a
//! tab/plugin cancels pending data requests and releases state"
//! ([`TableStateMutations::whole_state_resets`] moves ONLY through
//! [`TableUiState::reset_all`], staying 0 in every normal flow — the
//! defect probe); the page-request anti-storm (dedup + suppression
//! counting + arrival closure). Counters never reset in normal flows.

// Stage-1 surface: consumed by the unit tests in this module only until the
// typed-table mount integration arrives (SCN-014 follow-up); the allow
// mirrors plugin_scene.rs / plugin_widgets.rs / plugin_widgets_dispatch.rs
// stage precedent.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use ocs_plugin_api::scene::{SceneRevision, StableId};
use ocs_plugin_api::widgets::{
    CellKind, PanelTableDef, TableColumnDef, TableCell, TableRowDef, WidgetCellValue, WidgetError,
    WidgetEvent, WidgetOp, WidgetRange, WidgetSurface, MAX_SELECTION_IDS, MAX_WIDGET_RANGE_LEN,
};

use crate::ui::window::plugin_widgets::{
    FlatRow, WindowSpec, VisibleWindow, materialize_ids, visible_window,
};

// ---------------------------------------------------------------------------
// Draft + cell focus (the small typed values the state owns)
// ---------------------------------------------------------------------------

/// One cell-edit DRAFT: the host-local edit session of ONE cell, keyed by
/// the (row key, column id) address and holding the pending
/// [`WidgetCellValue`] skeleton. NOT the data of record — the commit turns
/// it into ONE [`WidgetOp::TableCell`] patch fact (the typed data leg; the
/// `TableCellChanged` skeleton event stays the notify/read leg and is NOT
/// emitted here — one event per method: the commit's output is the FACT).
///
/// The pending value is seeded at [`TableUiState::begin_cell_edit`] from the
/// CURRENT cell of the address, so a commit without typing re-publishes the
/// current value (a value-identical fact — harmless; the elision of
/// value-identical commits is a recessed nicety, deliberate non-goal today,
/// documented at [`TableUiState::commit_cell_draft`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CellDraft {
    /// The row key being edited (stable identity; the per-table namespace).
    pub key: StableId,
    /// The column being edited (stable id; the patch address).
    pub column: StableId,
    /// The skeleton value of the edit (scalar vocabulary only —
    /// text/number/bool; the kind gates at `begin_cell_edit` +
    /// `draft_cell_value` guarantee it never widens).
    pub pending: WidgetCellValue,
}

/// The CELL focus pair (the documented cell-focus model): row-key focus +
/// the named column. The state persists ONLY the row half
/// ([`TableUiState::focus`]) — the column cursor is the caller-held
/// cell-cursor state (a natural host rendering cursor), passed IN and
/// resolved HERE so the roving boundary ("focus moves only among
/// materialized rows") is enforced in ONE place for rows and cells alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CellFocus {
    /// The row key of the focused cell.
    pub key: StableId,
    /// The column of the focused cell.
    pub column: StableId,
}

// ---------------------------------------------------------------------------
// Mutation counters (the §16 defect probe)
// ---------------------------------------------------------------------------

/// The per-(surface, table) mutation counters — the ARCHITECTURE §16 proof
/// instruments, mirroring
/// [`crate::ui::window::plugin_widgets::WidgetStateMutations`] field per
/// defect. Probes never reset these; the ONLY entry to
/// [`TableStateMutations::whole_state_resets`] is the explicit
/// [`TableUiState::reset_all`] teardown helper.
///
/// Count: the spec pins SIX fields; the task allows ONE extra counter for
/// the critical schema-change re-sync (`drafts_discarded_on_schema_change`)
/// — deliberately NOT folded into `invalid_cell_refusals` (a schema re-sync
/// is not a user-gesture refusal) and not into `row_state_changes` (a draft
/// discard is not a row write). SEVEN total; no further counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TableStateMutations {
    /// Per-row state writes: selection diff writes (the rows the operation
    /// TOUCHED — removed + added; an identical rewrite counts 0), sync
    /// prunes of selection entries that were actually present, drafts
    /// pruned because their ROW vanished (data change — mirror of SWG-002's
    /// counted-only-if-present prune policy), and the ONE-row write per
    /// committed cell fact. `whole_state_resets` never moves here.
    pub row_state_changes: u64,
    /// Cell commits applied state-side (draft → pending op FACT). One
    /// commit moves exactly ONE (plus exactly ONE `row_state_changes`);
    /// a refusal never moves any; `cancel_cell_draft` moves NONE.
    pub cell_writes: u64,
    /// Full-state replacement calls (the DEFECT counter — 0 in every
    /// normal flow) — ONLY the explicit [`TableUiState::reset_all`]
    /// teardown helper moves it (tab/plugin close).
    pub whole_state_resets: u64,
    /// Page requests actually emitted
    /// ([`WidgetEvent::TablePageRequested`], validated BEFORE counting).
    pub page_requests_emitted: u64,
    /// Page requests the in-flight dedup window suppressed (each
    /// suppression is a re-request storm that did NOT happen).
    pub duplicate_pages_suppressed: u64,
    /// Cell-edit refusals surfaced state-side, BOTH flavors of the gate:
    /// (a) the COLUMN kind gate — a non-scalar column refused at
    /// `begin_cell_edit`, a value-class mismatch refused at
    /// `draft_cell_value`; (b) the VALUE gate — the DELEGATED crate
    /// validation refusing a kind-correct but structurally invalid value
    /// (non-finite number, over-cap text). A refusal leaves ALL other
    /// state untouched. Mirror-vs-delegation duties are documented at
    /// [`TableUiState::draft_cell_value`].
    pub invalid_cell_refusals: u64,
    /// THE ONE EXTRA counter the task grants: drafts discarded during
    /// [`TableUiState::sync`] because the SCHEMA changed under them —
    /// the draft's column kind changed, the column vanished, or the whole
    /// table vanished from the registry. NOT a user-gesture refusal (so
    /// never `invalid_cell_refusals`) and NOT a row write (so never
    /// `row_state_changes`). A draft pruned because its ROW vanished
    /// counts as `row_state_changes` instead (a data prune — the SWG-002
    /// prune policy).
    pub drafts_discarded_on_schema_change: u64,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// State-layer refusal reasons (the table sibling of
/// [`crate::ui::window::plugin_widgets::WidgetStateError`]). A refusal
/// leaves ALL state untouched (no counter moves, no event, no fact) — the
/// §16 rule that a rejected interaction cannot half-apply starts here.
/// NON-wire host-local enum (like SWG-003's `DispatchRefusal`); variants
/// are pinned in DECLARATION ORDER from creation (tail-append only).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TableStateError {
    /// The row key is not in the synced table (unknown, pruned, or outside
    /// the current materialized window when a windowed focus was asked).
    UnknownRow(StableId),
    /// The column id is not in the table's column registry.
    UnknownColumn(StableId),
    /// The draft could not READ the current cell of its address (a
    /// validated def guarantees a scalar column's skeleton is stored;
    /// this refuses the impossible def shape instead of inventing a
    /// seed value). Unreachable in normal flow.
    UnreadableCell {
        /// The row key of the unreadable address.
        key: StableId,
        /// The column of the unreadable address.
        column: StableId,
    },
    /// The column's kind is NOT a scalar patch-writable kind
    /// (text/number/bool): icon/link/badge/progress/date cells are
    /// plugin-PUBLISHED through the registry — a patch naming one is
    /// refused by the crate's own apply path ([`WidgetError::InvalidCell`]),
    /// and the state layer refuses EARLY (before any draft exists) with
    /// the typed reason. Counted as `invalid_cell_refusals`.
    NotWritable {
        /// The refused column id.
        column: StableId,
        /// The column's kind (the non-scalar one).
        kind: CellKind,
    },
    /// The draft's new value does not KIND-match its column (the COLUMN
    /// kind gate mirrored — the LOCAL three-arm class comparison, the only
    /// mirrored validation duty). Counted as `invalid_cell_refusals`.
    KindMismatch {
        /// The column id (the unchanged address).
        column: StableId,
        /// The column's kind (what the value must be).
        expected: CellKind,
        /// The value class that was offered ("text" / "number" / "bool").
        offered: &'static str,
    },
    /// The draft's value is kind-CORRECT but structurally invalid per the
    /// crate's own cell validation (a non-finite number, text past the
    /// byte caps, …). The reason is the crate's OWN [`WidgetError`],
    /// carried verbatim — the DELEGATED check ran, nothing was
    /// re-implemented. Counted as `invalid_cell_refusals`.
    InvalidCellValue {
        /// The column id (the unchanged address).
        column: StableId,
        /// The crate's own refusal reason.
        reason: WidgetError,
    },
    /// `commit_cell_draft` / `draft_cell_value` with no draft open — the
    /// mount misrouting is a visible DEFECT; nothing happened.
    NoDraft,
    /// `begin_cell_edit` refused: an edit of this cell is ALREADY armed
    /// (a draft is open) or the cell's previously-committed fact is
    /// unsettled (in flight) — the SWG-003 `AlreadyInFlight` pattern; no
    /// double facts, no overwritten drafts. Uncounted (dedup-style
    /// refusal — a legitimate interaction shape, not a defect statistic).
    DraftInFlight {
        /// The cell address that could not be begun.
        key: StableId,
        /// The column of that address.
        column: StableId,
    },
    /// The selection set passes [`MAX_SELECTION_IDS`] state-side (the
    /// state-side companion of the crate's budget error; the event is
    /// never constructed past the cap).
    SelectionTooLarge {
        /// How many keys were offered.
        offered: usize,
    },
    /// The built event failed the crate's own [`WidgetEvent::validate`] —
    /// carried verbatim (ONE validity path for GUI and MCP, §15).
    /// Unreachable in normal flow (the state-side bounds mirror the
    /// event's own bounds); nothing moves when it fires.
    InvalidEvent {
        /// The crate's own validation reason.
        reason: WidgetError,
    },
}

impl std::fmt::Display for TableStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TableStateError::UnknownRow(key) => {
                write!(f, "row key {} is not a materialized row of the synced table", key.0)
            }
            TableStateError::UnknownColumn(column) => {
                write!(f, "column {} is not in the table's column registry", column.0)
            }
            TableStateError::UnreadableCell { key, column } => {
                write!(f, "cell ({} , {}) has no skeleton on record", key.0, column.0)
            }
            TableStateError::NotWritable { column, kind } => {
                write!(
                    f,
                    "column {} has kind `{kind}`, not a patch-writable scalar kind",
                    column.0
                )
            }
            TableStateError::KindMismatch { column, expected, offered } => {
                write!(
                    f,
                    "column {} expects cell kind `{expected}`, but a {offered} skeleton value was offered",
                    column.0
                )
            }
            TableStateError::InvalidCellValue { column, reason } => {
                write!(
                    f,
                    "column {} draft value refused by the crate's own cell validation: {reason}",
                    column.0
                )
            }
            TableStateError::NoDraft => write!(f, "no cell draft is open"),
            TableStateError::DraftInFlight { key, column } => {
                write!(f, "cell ({} , {}) is already armed or has an unsettled fact",
                    key.0, column.0)
            }
            TableStateError::SelectionTooLarge { offered } => {
                write!(f, "selection of {offered} row keys passes the {MAX_SELECTION_IDS}-key cap")
            }
            TableStateError::InvalidEvent { reason } => {
                write!(f, "the built event failed the crate's own validation: {reason}")
            }
        }
    }
}

impl std::error::Error for TableStateError {}

// ---------------------------------------------------------------------------
// The synced read model (captured from the def at sync)
// ---------------------------------------------------------------------------

/// The state layer's read view of ONE table def, gathered from the synced
/// [`WidgetSurface::tables`] registry entry at [`TableUiState::sync`] (the
/// SWG-002 `SyncedSurface` pattern: one snapshot against the exact revision
/// the mount presents; state decisions read THIS, never live surface
/// walks, so they are deterministic).
#[derive(Debug, Clone)]
pub(crate) struct SyncedTable {
    /// The surface revision the snapshot came from.
    pub revision: SceneRevision,
    /// Column kinds by id (the kind gate's data).
    pub columns: HashMap<StableId, CellKind>,
    /// Column ids in registry order (also the cell-focus roving order).
    pub column_order: Vec<StableId>,
    /// The published row keys in PUBLISHED order (the FLAT keys list — the
    /// windowing input; the table-tier analog of the SWG-002 flat-row
    /// model: table rows have no expansion and no depth, so the flat list
    /// IS the def's `rows` keys in published order).
    pub rows: Vec<StableId>,
    /// Every live row key (the per-table namespace — the pruning set).
    pub live: HashSet<StableId>,
    /// The scalar skeleton of the published cells by (row key, column id);
    /// NON-scalar cells are ABSENT (never patch-writable through the
    /// skeleton — the crate's own gate). This is the seed data of
    /// [`CellDraft::pending`].
    pub cells: HashMap<(StableId, StableId), WidgetCellValue>,
    /// The def's backing total when it windows (`None` = fully
    /// materialized). Metadata ONLY — never the flat keys list's length
    /// (the 100k invariant: the published window is the flat list).
    pub total_rows: Option<u64>,
    /// The published rows as cheap SWG-002 flat-row structs (the
    /// [`visible_window`] / `materialize_ids` input, built ONCE per sync —
    /// the established O(def rows) sync envelope).
    pub flat: Vec<FlatRow>,
}

impl SyncedTable {
    /// Capture one table's read model from the surface's registry (the
    /// ONLY data flowing in — the machine never walks a live surface again
    /// after this). `None` when `table_id` is not in the registry.
    fn capture(table_id: StableId, surface: &WidgetSurface) -> Option<Self> {
        let def = surface.tables.get(&table_id)?;
        let mut columns = HashMap::with_capacity(def.columns.len());
        for column in &def.columns {
            columns.insert(column.id, column.kind);
        }
        let column_order: Vec<StableId> = def.columns.iter().map(|column| column.id).collect();
        let mut rows = Vec::with_capacity(def.rows.len());
        let mut live = HashSet::with_capacity(def.rows.len());
        let mut cells = HashMap::with_capacity(def.rows.len() * def.columns.len());
        for row in &def.rows {
            rows.push(row.key);
            live.insert(row.key);
            // Cells shadow the columns 1:1 BY POSITION (the crate's own
            // validated contract); only SCALAR cells get a skeleton (the
            // presentation kinds are never patch-writable).
            for (position, cell) in row.cells.iter().enumerate() {
                if position >= def.columns.len() {
                    break; // unvalidated def shape: ignore the overshoot
                }
                if let Some(value) = skeleton_of(cell) {
                    cells.insert((row.key, def.columns[position].id), value);
                }
            }
        }
        let flat = rows
            .iter()
            .map(|key| FlatRow {
                id: *key,
                depth: 0,
                height: 1.0,
            })
            .collect();
        Some(SyncedTable {
            revision: surface.revision,
            columns,
            column_order,
            rows,
            live,
            cells,
            total_rows: def.total_rows,
            flat,
        })
    }
}

/// The skeleton projection of one typed cell: scalar kinds project to
/// their skeleton value; presentation kinds (icon/link/badge/progress/date)
/// have NO skeleton representation (they are plugin-published, never
/// patch-writable — the crate's own gate).
fn skeleton_of(cell: &TableCell) -> Option<WidgetCellValue> {
    match cell {
        TableCell::Text(text) => Some(WidgetCellValue::Text(text.clone())),
        TableCell::Number(n) => Some(WidgetCellValue::Number(*n)),
        TableCell::Bool(b) => Some(WidgetCellValue::Bool(*b)),
        _ => None,
    }
}

/// The value CLASS of a skeleton value — the LOCAL three-arm discriminator
/// (the mirrored piece of the column kind gate; NO cap arithmetic).
fn value_class(value: &WidgetCellValue) -> CellKind {
    match value {
        WidgetCellValue::Text(_) => CellKind::Text,
        WidgetCellValue::Number(_) => CellKind::Number,
        WidgetCellValue::Bool(_) => CellKind::Bool,
    }
}

/// The word for a skeleton value class (the Display's vocabulary).
fn value_class_name(value: &WidgetCellValue) -> &'static str {
    match value {
        WidgetCellValue::Text(_) => "text",
        WidgetCellValue::Number(_) => "number",
        WidgetCellValue::Bool(_) => "bool",
    }
}

/// `true` when the kind is a scalar (skeleton-)patch-writable WRITE kind —
/// the crate's rule ("only the scalar kinds are patch-writable through the
/// skeleton") as an enumeration, shared by the begin + draft gates.
fn is_patch_writable(kind: CellKind) -> bool {
    matches!(kind, CellKind::Text | CellKind::Number | CellKind::Bool)
}

/// Delegate ONE cell's value validation through the crate's OWN checker:
/// `PanelTableDef::validate` is PUBLIC and includes the private
/// `check_table_cell` unchanged, so a throwaway 1×1 def is the faithful
/// delegation surface. The probe refuses every cap/gate the crate refuses
/// (finiteness, text bytes, date bounds, …) with the crate's own reason
/// string, and refuses a KIND mismatch too — which the caller already
/// refused locally for the typed refusal BEFORE reaching here, so the
/// probe's failures at the state layer are VALUE-level failures only.
///
/// This is the documented "use the crates' own validate paths where
/// public" duty split, in code.
fn probe_typed_cell(
    table: StableId,
    key: StableId,
    column: StableId,
    kind: CellKind,
    skeleton: &WidgetCellValue,
) -> Result<(), WidgetError> {
    let cell = match skeleton {
        WidgetCellValue::Text(text) => TableCell::Text(text.clone()),
        WidgetCellValue::Number(n) => TableCell::Number(*n),
        WidgetCellValue::Bool(b) => TableCell::Bool(*b),
    };
    let def = PanelTableDef {
        id_table: table,
        columns: vec![TableColumnDef {
            id: column,
            label: "draft probe".to_string(),
            kind,
            width_hint: None,
            stretch: false,
        }],
        total_rows: None,
        rows: vec![TableRowDef {
            key,
            cells: vec![cell],
            domain_ref: None,
        }],
    };
    def.validate()
}

/// The (row key, column) address of one `WidgetOp::TableCell` fact — the
/// in-flight map key. Only TableCell facts reach here.
fn key_column_of(op: &WidgetOp) -> (StableId, StableId) {
    match op {
        WidgetOp::TableCell { key, column, .. } => (*key, *column),
        _ => unreachable!("key_column_of is only called on the cell facts this layer emits"),
    }
}

// ---------------------------------------------------------------------------
// Materialized rows (the windowing output)
// ---------------------------------------------------------------------------

/// The visible-range window over the table's PUBLISHED rows — the mount's
/// render input (the table edition of the SWG-002 `materialize_ids` output).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MaterializedRows {
    /// The half-open visible window (including margins) over the flat keys.
    pub window: VisibleWindow,
    /// The row keys of the window, in paint order — what the mount
    /// materializes as native row widgets.
    pub keys: Vec<StableId>,
    /// Total published rows (the flat keys list's length) — scrollbar
    /// input. The backing total (when the def windows) is DIFFERENT and
    /// never appears here: the materialization is STRUCTURALLY bounded to
    /// the published rows.
    pub published_rows: usize,
}

// ---------------------------------------------------------------------------
// TableUiState
// ---------------------------------------------------------------------------

/// Per-surface, per-table transient host state (ARCHITECTURE §3 state
/// class 3, the tables case): what rows are selected, what row/cell has
/// keyboard focus, where the scroll anchor is, what edit is armed, and
/// which page fetches / cell facts are in flight. One instance per mounted
/// table per surface; every method is headless and deterministic; the
/// mount calls them from its native interaction handlers (the LATER task —
/// documented, not implemented here).
#[derive(Debug, Clone)]
pub(crate) struct TableUiState {
    /// The surface this state belongs to (rides every emitted event/fact).
    surface_id: StableId,
    /// The registry table this state tracks (the per-table namespace owner;
    /// several `TableRef`s may name it, the state is per TABLE id).
    table_id: StableId,
    /// Row selection (keys; bounded MAX_SELECTION_IDS; host-multi-select by
    /// the documented decision — see the module docs).
    selection: Vec<StableId>,
    /// Focused cell / row (keyboard nav; roving within the visible
    /// window). Row focus is this slot; cell focus = (this, column) pair
    /// when editing / navigating — the documented model at [`CellFocus`].
    focus: Option<StableId>,
    /// Editing draft (host-local; NEVER the data-of-record): the def's rows
    /// are the data-of-record for the MOUNT read model; a cell EDIT draft
    /// lives here until commit; commit produces a [`WidgetOp::TableCell`]
    /// patch FACT via the plugin path (the typed fact travels through the
    /// registry's patch — the mount emits the op request).
    draft: Option<CellDraft>,
    /// Visible window scroll anchor (index into the published keys list —
    /// the windowing input; clamped in the window math; never an identity).
    scroll_index: usize,
    /// In-flight page requests (dedup; keyed by `first_key`). Closed ONLY
    /// by sync when rows under the anchor ARRIVE (the anchor stopped being
    /// the published tail) or when the anchor/table disappears — the
    /// arrival, not a timeout or a poll, ends the window (documented at
    /// [`TableUiState::page_request_if_needed`]).
    pending_pages: HashMap<StableId, WidgetRange>,
    /// Committed cell facts awaiting settle — the SWG-003 in-flight/settle
    /// rule for cell edits: the emitted fact stays keyed here until
    /// [`TableUiState::settle`]; a re-begin of the same cell refuses
    /// while it lingers (no double facts).
    in_flight: HashMap<(StableId, StableId), WidgetOp>,
    /// The synced read model (columns/rows views by key; mirrors SWG-002's
    /// SyncedSurface pattern). `None` before the first sync, or when the
    /// table vanished from the registry.
    synced: Option<SyncedTable>,
    /// The §16 counters.
    mutations: TableStateMutations,
}

impl TableUiState {
    /// New, empty state for one table on one surface. Everything defaults:
    /// nothing selected, NO focus (the roving ring lands only after the
    /// first navigation), scrolled to the top, nothing armed or in flight.
    pub(crate) fn new(surface_id: StableId, table_id: StableId) -> Self {
        TableUiState {
            surface_id,
            table_id,
            selection: Vec::new(),
            focus: None,
            draft: None,
            scroll_index: 0,
            pending_pages: HashMap::new(),
            in_flight: HashMap::new(),
            synced: None,
            mutations: TableStateMutations::default(),
        }
    }

    /// The surface this state belongs to.
    pub(crate) fn surface_id(&self) -> StableId {
        self.surface_id
    }

    /// The registry table this state tracks.
    pub(crate) fn table_id(&self) -> StableId {
        self.table_id
    }

    /// The §16 mutation counters (read side; only the state methods write
    /// them, only their documented fields).
    pub(crate) fn mutations(&self) -> &TableStateMutations {
        &self.mutations
    }

    /// The current selection (row keys, bounded by [`MAX_SELECTION_IDS`]).
    pub(crate) fn selection(&self) -> &[StableId] {
        &self.selection
    }

    /// The focused row key, when any (the mount's click re-anchor +
    /// roving-tabindex ring input).
    pub(crate) fn focus(&self) -> Option<StableId> {
        self.focus
    }

    /// The open cell draft, when any (the mount's editor widget input).
    pub(crate) fn draft(&self) -> Option<&CellDraft> {
        self.draft.as_ref()
    }

    /// The scroll anchor (index into the published keys list).
    pub(crate) fn scroll_index(&self) -> usize {
        self.scroll_index
    }

    /// Set the scroll anchor (the mount's scroll handler input; the
    /// windowing re-runs whenever it moves — scrolling re-materializes).
    pub(crate) fn set_scroll_index(&mut self, index: usize) {
        self.scroll_index = index;
    }

    /// The read model captured by the last [`TableUiState::sync`]; `None`
    /// before it (or when the table vanished from the registry).
    pub(crate) fn synced(&self) -> Option<&SyncedTable> {
        self.synced.as_ref()
    }

    /// In-flight cell facts keyed by address (read side) — the mount hands
    /// each to the surface's apply path and calls [`TableUiState::settle`]
    /// after forwarding.
    pub(crate) fn in_flight(&self) -> &HashMap<(StableId, StableId), WidgetOp> {
        &self.in_flight
    }

    /// Page requests in flight keyed by `first_key` (read side) — the
    /// dedup windows sync closes on arrival.
    pub(crate) fn pending_pages(&self) -> &HashMap<StableId, WidgetRange> {
        &self.pending_pages
    }

    // -- selection ----------------------------------------------------------

    /// Replace the selection with `keys` (row keys, order preserved).
    ///
    /// Tables are HOST-multi-select (the documented decision at the module
    /// docs) with the SAME [`MAX_SELECTION_IDS`] bound; the bound is
    /// enforced state-side BEFORE the event exists, and the constructed
    /// event is again bound through the crate's own
    /// [`WidgetEvent::validate`] (one validity path for GUI and MCP, §15).
    /// The emitted event carries the keys verbatim (row-key semantics).
    ///
    /// Mutation proof (§16 "selection updates do not rebuild the whole
    /// tree", table edition): the write is diff-based —
    /// `row_state_changes` += removed + added (the rows the operation
    /// TOUCHED), so an identical rewrite counts 0 and a same-count
    /// different-content rewrite counts exactly the exchanged keys.
    /// `whole_state_resets` stays 0. A refusal leaves ALL state untouched.
    pub(crate) fn set_selection(
        &mut self,
        keys: Vec<StableId>,
    ) -> Result<Option<WidgetEvent>, TableStateError> {
        if keys.len() > MAX_SELECTION_IDS {
            return Err(TableStateError::SelectionTooLarge {
                offered: keys.len(),
            });
        }
        // Diff against the previous selection: the rows the operation
        // touches.
        let previous: HashSet<StableId> = self.selection.iter().copied().collect();
        let next: HashSet<StableId> = keys.iter().copied().collect();
        let removed = previous.difference(&next).count();
        let added = next.difference(&previous).count();
        let event = WidgetEvent::SelectionChanged {
            surface_id: self.surface_id,
            selected_ids: keys.clone(),
        };
        // The event's validity binds BEFORE any write (atomicity rule).
        // Unreachable in normal flow: the state-side bound above mirrors
        // the event's own bound exactly — the refusal mapping is honest
        // by construction.
        if let Err(reason) = event.validate() {
            return Err(TableStateError::InvalidEvent { reason });
        }
        self.selection = keys;
        self.mutations.row_state_changes += (removed + added) as u64;
        Ok(Some(event))
    }

    // -- focus (keyboard nav; roving WITHIN the visible window) --------------

    /// The current half-open visible window of the published rows under
    /// `self.scroll_index` + `spec` (the SWG-002 windowing arithmetic —
    /// reused verbatim). `None` before the first sync (nothing can be
    /// visible).
    fn window_of(&self, spec: &WindowSpec) -> Option<VisibleWindow> {
        let table = self.synced.as_ref()?;
        Some(visible_window(&table.flat, self.scroll_index, spec))
    }

    /// The index of `key` within the published keys list, when live.
    fn published_index(&self, key: StableId) -> Option<usize> {
        self.synced
            .as_ref()?
            .rows
            .iter()
            .position(|candidate| *candidate == key)
    }

    /// Move row focus to `key` (the mount's click hand-off; the SWG-002
    /// `focus_set` analog). The roving boundary binds here: focus may
    /// only rest on a row inside the CURRENT visible window — a key
    /// outside the window (including unknown keys) refuses with the typed
    /// error instead of moving focus; the mount re-anchors visually on
    /// materialized rows by construction.
    pub(crate) fn row_focus_set(
        &mut self,
        key: StableId,
        spec: &WindowSpec,
    ) -> Result<(), TableStateError> {
        let Some(window) = self.window_of(spec) else {
            return Err(TableStateError::UnknownRow(key));
        };
        let Some(index) = self.published_index(key) else {
            return Err(TableStateError::UnknownRow(key));
        };
        if !window.contains(index) {
            return Err(TableStateError::UnknownRow(key));
        }
        self.focus = Some(key);
        Ok(())
    }

    /// Roving focus: move to the NEXT row (paint order) among the
    /// MATERIALIZED window rows (the documented roving boundary: focus
    /// never LANDS beyond the window; scrolling re-materializes and the
    /// roving re-anchors). From `focus: None` the roving enters the window
    /// at its FIRST row; from a focus outside the window it re-anchors at
    /// the window's first row. `None` when the move would land past the
    /// window's END — the mount's scroll handler (and re-materialize)
    /// owns the rest.
    pub(crate) fn row_focus_next(&mut self, spec: &WindowSpec) -> Option<StableId> {
        let window = self.window_of(spec)?;
        if window.is_empty() {
            return None;
        }
        let current = self.focus.and_then(|key| self.published_index(key));
        let target = match current {
            None => window.start, // enter at the window's top
            Some(index) if window.contains(index) => {
                if index + 1 >= window.end {
                    return None; // the window edge; scroll to go further
                }
                index + 1
            }
            Some(_) => window.start, // outside the window: re-anchor at its top
        };
        let key = self.synced.as_ref().map(|table| table.rows[target]);
        if let Some(key) = key {
            self.focus = Some(key);
        }
        key
    }

    /// Roving focus backward: move to the PREVIOUS row among the
    /// MATERIALIZED window rows; from `focus: None` the roving enters the
    /// window at its LAST row; from a focus outside the window it
    /// re-anchors at the window's last row; `None` at the window's START
    /// edge.
    pub(crate) fn row_focus_prev(&mut self, spec: &WindowSpec) -> Option<StableId> {
        let window = self.window_of(spec)?;
        if window.is_empty() {
            return None;
        }
        let current = self.focus.and_then(|key| self.published_index(key));
        let target = match current {
            None => window.end - 1, // enter at the window's bottom
            Some(index) if window.contains(index) => {
                if index <= window.start {
                    return None; // the window edge; scroll to go further
                }
                index - 1
            }
            Some(_) => window.end - 1, // outside the window: re-anchor at its bottom
        };
        let key = self.synced.as_ref().map(|table| table.rows[target]);
        if let Some(key) = key {
            self.focus = Some(key);
        }
        key
    }

    /// Cell roving (Left/Right arrows): move the (row, column) cell cursor
    /// within `column_order` on the FOCUSED row (the documented model —
    /// crossing rows stays [`TableUiState::row_focus_next`]/`prev`'s job;
    /// there is therefore NO window math here: the focused row is already
    /// materialized by the row-focus boundary). The cell cursor is
    /// CALLER-held (`current`, `None` on the first move); `None` lands on
    /// the FIRST (forward) or LAST (backward) column of the focused row; a
    /// `column_order` id missing from the table's columns is skipped
    /// (defensive normalization for caller-side junk, keeping the caller's
    /// order otherwise); row motion does not occur. `None` at the row's
    /// column edge (the mount stays put or wraps itself). The draft is
    /// never touched by this method (one fact per method — the edit
    /// session opens only through `begin_cell_edit`).
    pub(crate) fn cell_focus_next(
        &mut self,
        current: Option<CellFocus>,
        column_order: &[StableId],
    ) -> Option<CellFocus> {
        self.cell_focus_move(current, true, column_order)
    }

    /// Cell roving backward — the exact mirror of
    /// [`TableUiState::cell_focus_next`]: `None` (or a cursor whose row
    /// differs from the focus) lands on the LAST column; the column edge
    /// returns `None`.
    pub(crate) fn cell_focus_prev(
        &mut self,
        current: Option<CellFocus>,
        column_order: &[StableId],
    ) -> Option<CellFocus> {
        self.cell_focus_move(current, false, column_order)
    }

    /// The shared cell-motion body (the row stays fixed; the column cursor
    /// steps within `column_order`; landing always names the focused row —
    /// the state's `focus` stays what it is, the pair is authoritative).
    fn cell_focus_move(
        &mut self,
        current: Option<CellFocus>,
        forward: bool,
        column_order: &[StableId],
    ) -> Option<CellFocus> {
        let table = self.synced.as_ref()?;
        let focus = self.focus?;
        // The columns to traverse: the caller's order, filtered to the
        // table's own columns (skipped junk keeps the caller's order).
        let columns: Vec<StableId> = column_order
            .iter()
            .copied()
            .filter(|id| table.columns.contains_key(id))
            .collect();
        if columns.is_empty() {
            return None;
        }
        let position = current.and_then(|cell| {
            if cell.key == focus {
                columns.iter().position(|id| *id == cell.column)
            } else {
                None
            }
        });
        let column = match (forward, position) {
            (true, None) => columns[0],
            (true, Some(position)) => {
                if position + 1 >= columns.len() {
                    return None; // the row's column edge (forward)
                }
                columns[position + 1]
            }
            (false, None) => columns[columns.len() - 1],
            (false, Some(0)) => return None, // the row's column edge (backward)
            (false, Some(position)) => columns[position - 1],
        };
        Some(CellFocus {
            key: focus,
            column,
        })
    }

    // -- the editing draft ---------------------------------------------------

    /// Open the cell-edit draft at (row key, column) — the parent of the
    /// edit-lifecycle discipline. The draft arms ONLY on a synced, LIVE
    /// row whose column is a scalar patch-writable kind
    /// ([`CellKind::Text`]/[`CellKind::Number`]/[`CellKind::Bool`]);
    /// icon/link/badge/progress/date are plugin-PUBLISHED through the
    /// registry and REFUSE with [`TableStateError::NotWritable`] +
    /// `invalid_cell_refusals` BEFORE any draft exists (the crate's own
    /// apply path would refuse the patch too — this is the early, typed
    /// version of the same rule).
    ///
    /// Refuses while a draft is armed or the cell has an unsettled fact
    /// ([`TableStateError::DraftInFlight`] — the SWG-003 discipline; the
    /// mount cancels/commits/settles first). The fresh draft's
    /// [`CellDraft::pending`] is seeded from the CURRENT cell of the
    /// address (never a synthesized default).
    ///
    /// NO counter moves on success (arming an edit is not a row write, not
    /// a cell fact); the KIND refusal counts (`invalid_cell_refusals`);
    /// the lookups refuse plainly and move nothing.
    pub(crate) fn begin_cell_edit(
        &mut self,
        key: StableId,
        column: StableId,
    ) -> Result<(), TableStateError> {
        let Some(table) = self.synced.as_ref() else {
            return Err(TableStateError::UnknownRow(key));
        };
        if !table.live.contains(&key) {
            return Err(TableStateError::UnknownRow(key));
        }
        let Some(&kind) = table.columns.get(&column) else {
            return Err(TableStateError::UnknownColumn(column));
        };
        if self.draft.is_some() {
            return Err(TableStateError::DraftInFlight { key, column });
        }
        if self.in_flight.contains_key(&(key, column)) {
            return Err(TableStateError::DraftInFlight { key, column });
        }
        if !is_patch_writable(kind) {
            self.mutations.invalid_cell_refusals += 1;
            return Err(TableStateError::NotWritable { column, kind });
        }
        let Some(pending) = table.cells.get(&(key, column)) else {
            return Err(TableStateError::UnreadableCell { key, column });
        };
        self.draft = Some(CellDraft {
            key,
            column,
            pending: pending.clone(),
        });
        Ok(())
    }

    /// Move the draft's pending value along (per-keystroke; the draft was
    /// armed by [`TableUiState::begin_cell_edit`], whose seeded pending is
    /// the current value). TWO gates run, in this strict order:
    ///
    /// 1. THE COLUMN KIND GATE (mirrored, LOCAL — the only mirrored
    ///    validation in this module, deliberately a three-arm class
    ///    comparison and nothing more): the skeleton value's class must
    ///    equal the column's [`CellKind`] — a `Text` skeleton into a
    ///    `Number` column, … is refused as
    ///    [`TableStateError::KindMismatch`] + `invalid_cell_refusals`.
    ///    (Kept local so the typed error keeps the mismatch distinct from
    ///    the crate's stringly `InvalidCell` reasons.)
    /// 2. THE VALUE GATE (DELEGATED — the crate's own validation runs the
    ///    `probe_typed_cell` path, a throwaway 1×1 def whose public
    ///    `PanelTableDef::validate` includes the crate's private
    ///    per-cell checker UNMODIFIED): the kind-correct value is
    ///    re-checked for the crate's per-cell rules (finite number, text
    ///    byte caps) with the crate's OWN reason carried verbatim →
    ///    [`TableStateError::InvalidCellValue`] + `invalid_cell_refusals`.
    ///    Caps/finite arithmetic are NEVER re-implemented here.
    ///
    /// On a refusal the existing draft's pending stays EXACTLY as-was
    /// (atomicity: the operator retries or cancels — nothing consumed).
    /// On success only the pending value moves; NO counter move (no
    /// commit = no cell write yet).
    pub(crate) fn draft_cell_value(
        &mut self,
        value: WidgetCellValue,
    ) -> Result<(), TableStateError> {
        let Some(draft) = self.draft.as_ref() else {
            return Err(TableStateError::NoDraft);
        };
        let Some(&kind) = self
            .synced
            .as_ref()
            .and_then(|table| table.columns.get(&draft.column))
        else {
            return Err(TableStateError::UnknownColumn(draft.column));
        };
        if value_class(&value) != kind {
            self.mutations.invalid_cell_refusals += 1;
            return Err(TableStateError::KindMismatch {
                column: draft.column,
                expected: kind,
                offered: value_class_name(&value),
            });
        }
        let (key, column) = (draft.key, draft.column);
        // Delegate the VALUE check through the crate's own validate path
        // (the throwaway 1×1 def probe — the module docs record the duty
        // split; the probe's failure is the crate's OWN reason, verbatim).
        if let Err(reason) = probe_typed_cell(self.table_id, key, column, kind, &value) {
            self.mutations.invalid_cell_refusals += 1;
            return Err(TableStateError::InvalidCellValue {
                column,
                reason,
            });
        }
        let Some(draft) = self.draft.as_mut() else {
            unreachable!("the draft was dereferenced above; no re-entrancy exists");
        };
        draft.pending = value;
        Ok(())
    }

    /// Commit the open draft: produce ONE [`WidgetOp::TableCell`] FACT (the
    /// typed patch), clear the draft, count EXACTLY ONE `cell_writes` and
    /// ONE `row_state_changes` (ONE row — NO other row touched; the
    /// pointer-style proof lives in the tests), and park the fact as IN
    /// FLIGHT (the SWG-003 discipline) until [`TableUiState::settle`]. The
    /// MOUNT hands the op to the surface's apply path (the plugin patch
    /// pipeline — SCN-014's page plumbing; the documented hand-off) and
    /// settles the fact here after forwarding.
    ///
    /// The value-level validity was already bound at
    /// [`TableUiState::draft_cell_value`] — every fact leaves bounded (the
    /// crate's apply path still re-checks; a gated input cannot trip it).
    /// A commit with no draft refuses ([`TableStateError::NoDraft`] — a
    /// caller bug is LOUD: a refusal is visible, a silent no-op is not;
    /// use [`TableUiState::cancel_cell_draft`] for the free path). The
    /// Option reserves the future value-identical-commit elision
    /// (documented); today's only success shape is `Some(fact)`.
    pub(crate) fn commit_cell_draft(&mut self) -> Result<Option<WidgetOp>, TableStateError> {
        let Some(draft) = self.draft.take() else {
            return Err(TableStateError::NoDraft);
        };
        let op = WidgetOp::TableCell {
            table: self.table_id,
            key: draft.key,
            column: draft.column,
            value: draft.pending,
        };
        self.in_flight.insert(key_column_of(&op), op.clone());
        self.mutations.cell_writes += 1;
        self.mutations.row_state_changes += 1; // ONE row touched, exactly.
        Ok(Some(op))
    }

    /// Cancel the open draft (dismiss/Escape/click-away): the draft clears;
    /// NOTHING else moves — no counters (a cancelled draft touched
    /// nothing; transient host state class 3 — the counter must NOT move:
    /// the documented policy).
    pub(crate) fn cancel_cell_draft(&mut self) {
        self.draft = None;
    }

    /// Settle one committed fact (the mount's post-forward signal). `true`
    /// when a fact was parked; `false` when nothing was parked
    /// (idempotent). NO counters (settle is transport, not a user or
    /// plugin mutation).
    pub(crate) fn settle(&mut self, key: StableId, column: StableId) -> bool {
        self.in_flight.remove(&(key, column)).is_some()
    }

    // -- sync (structural reconcile across snapshots/patches) ---------------

    /// Reconcile this state against a freshly applied [`WidgetSurface`]
    /// (the mount calls this after every successful snapshot/patch swap —
    /// the SWG-002 rule verbatim). This table's registry def is
    /// re-captured; selection/focus/draft SURVIVE patches that don't touch
    /// their keys and are pruned when they do (structural identity = the
    /// [`StableId`] key; nothing is ADDED).
    ///
    /// Prune + lifecycle policy (the SWG-002 contract plus the table-tier
    /// cases, ALL documented):
    ///
    /// - selection: keys whose row no longer exists are PRUNED, counted
    ///   ONLY-if-present (a row state write for entries actually there);
    /// - focus: a dangling focus falls back to `None` (one focus SLOT —
    ///   uncounted; the mount re-anchors through the roving methods);
    /// - draft: discarded when (a) the column's KIND changed (the critical
    ///   re-sync), (b) the column vanished, or (c) the whole table
    ///   vanished from the registry — all SCHEMA changes →
    ///   `drafts_discarded_on_schema_change`; or (d) the ROW vanished — a
    ///   data prune → `row_state_changes` (mirror of the selection prune
    ///   rule);
    /// - in-flight page dedup windows: closed when rows under the anchor
    ///   key ARRIVE (the patch facts landed — the anchor stopped being the
    ///   PUBLISHED TAIL it anchored at request time) or when the anchor
    ///   key (or table) disappears; uncounted closes (arrival is
    ///   lifecycle, not a user mutation — the mirror of SWG-002's
    ///   parents-with-rows policy);
    /// - in-flight CELL facts are deliberately NOT touched by sync
    ///   (documented at the module docs: `settle` is their only closure).
    pub(crate) fn sync(&mut self, surface: &WidgetSurface) {
        let previous = self.synced.take();
        let model = SyncedTable::capture(self.table_id, surface);
        let live: Option<&HashSet<StableId>> = model.as_ref().map(|table| &table.live);

        // Selection prune (counted-only-if-present; order kept).
        let kept: Vec<StableId> = self
            .selection
            .iter()
            .copied()
            .filter(|key| live.is_some_and(|live| live.contains(key)))
            .collect();
        let pruned = (self.selection.len() - kept.len()) as u64;
        self.selection = kept;
        debug_assert!(
            self.selection.len() <= MAX_SELECTION_IDS,
            "the pruned selection stays within the cap"
        );
        self.mutations.row_state_changes += pruned;

        // Focus prune (one slot; uncounted).
        if let Some(key) = self.focus {
            if !live.is_some_and(|live| live.contains(&key)) {
                self.focus = None;
            }
        }

        // Draft discard: the schema/data split (counter notes above).
        if let Some(draft) = self.draft.take() {
            let new_kind: Option<CellKind> = model
                .as_ref()
                .and_then(|table| table.columns.get(&draft.column))
                .copied();
            let old_kind: Option<CellKind> = previous
                .as_ref()
                .and_then(|table| table.columns.get(&draft.column))
                .copied();
            if let Some(kind) = new_kind {
                if old_kind.is_some() && old_kind == Some(kind) {
                    // The column survived with the same kind. The row?
                    if live.is_some_and(|live| live.contains(&draft.key)) {
                        self.draft = Some(draft); // untouched: keep the draft
                    } else {
                        // (d) the row vanished: a data prune, counted.
                        self.draft = None;
                        self.mutations.row_state_changes += 1;
                    }
                } else {
                    // (a) the kind changed under the draft (or the column
                    // appeared this very sync — old none, new some): the
                    // CRITICAL re-sync discard.
                    self.draft = None;
                    self.mutations.drafts_discarded_on_schema_change += 1;
                }
            } else {
                // (b)/(c) the column (or the table) vanished: schema change.
                self.draft = None;
                self.mutations.drafts_discarded_on_schema_change += 1;
            }
        }

        // In-flight page dedup windows: rows ARRIVED past the anchor (the
        // page answered) → close; the anchor or the table vanishing →
        // close (a vanished address can never be answered — SWG-002's
        // "parent vanished entirely is pruned too" analog). The anchor is
        // the published TAIL at request time (see `page_request_if_needed`),
        // so "the anchor is no longer the tail" IS the arrival marker.
        self.pending_pages.retain(|anchor, _| {
            let Some(table) = model.as_ref() else {
                return false; // the table vanished; nothing will answer
            };
            let Some(index) = table.rows.iter().position(|key| *key == *anchor) else {
                return false; // the anchor key vanished; nothing will answer
            };
            index + 1 >= table.rows.len() // keep ONLY while it stays the tail
        });

        self.synced = model;
    }

    // -- page requests (the paged backing model) -----------------------------

    /// The page request for the table's next keyed window. Emits
    /// [`WidgetEvent::TablePageRequested`] when: the table is synced and
    /// its def WINDOWS (`total_rows: Some(total)` with a published window
    /// strictly smaller than the total — there is something left to
    /// request), the anchor key is the LAST PUBLISHED row (the keyed
    /// window's boundary the host EXTENDS: rows are learned from the
    /// published registry and only the tail can be extended — a mid-table
    /// anchor has nothing to fetch THERE and returns `None`), and no
    /// request for that `first_key` is in flight
    /// ([`TableStateMutations::duplicate_pages_suppressed`] counts the
    /// suppression). `len` is clamped to
    /// [`MAX_WIDGET_RANGE_LEN`]; the event is bound through the crate's
    /// own [`WidgetEvent::validate`] BEFORE counting (an invalid request
    /// is a defect and must not count as emitted).
    ///
    /// The tail-anchor rule is what makes the lifecycle DETERMINISTIC
    /// end-to-end: the dedup window for a first_key closes in sync exactly
    /// when the anchor stops being the tail (rows landed past it), so
    /// request/arrival/suppression are one coherent keyed story. The
    /// request's `first_key` (the row the plugin's page must include —
    /// the keyed-window convention beats offsets so stable identities
    /// survive patch windows; the anchor's index into the published list
    /// rides the recorded [`WidgetRange`] for the mount's bookkeeping).
    /// `None` when the table isn't synced, doesn't window,
    /// has nothing left to request, or the anchor isn't known — anything
    /// that means "no sensible request happens now" (the SWG-002
    /// non-refusal `Option` convention for not-your-turn lookups — the
    /// mount calls this with the scroll anchor when the viewport's tail
    /// approaches the end of the published window).
    pub(crate) fn page_request_if_needed(
        &mut self,
        anchor_key: StableId,
        len: usize,
    ) -> Option<WidgetEvent> {
        let table = self.synced.as_ref()?;
        let Some(total) = table.total_rows else {
            return None;
        };
        if table.rows.len() >= total as usize {
            return None; // the whole backing set is already published
        }
        let index = table.rows.iter().position(|key| *key == anchor_key)?;
        if index + 1 != table.rows.len() {
            return None; // a mid-table anchor: only the tail extends
        }
        if self.pending_pages.contains_key(&anchor_key) {
            self.mutations.duplicate_pages_suppressed += 1;
            return None;
        }
        let len = len.min(MAX_WIDGET_RANGE_LEN);
        let event = WidgetEvent::TablePageRequested {
            surface_id: self.surface_id,
            table: self.table_id,
            first_key: anchor_key,
            len,
        };
        // Construct-level validity binds BEFORE the pending entry + the
        // emitted counter (both must count ONLY valid events; the clamp
        // above makes the failure unreachable, the check is the belt).
        let validity = event.validate();
        debug_assert!(
            validity.is_ok(),
            "TablePageRequested must be valid by construction"
        );
        if validity.is_err() {
            return None;
        }
        self.pending_pages.insert(anchor_key, WidgetRange { start: index, len });
        self.mutations.page_requests_emitted += 1;
        Some(event)
    }

    // -- materialize (the windowing output the mount paints) -----------------

    /// The visible-range window of the table's published rows under `spec`
    /// + [`TableUiState::scroll_index`] — the mount's render input (the
    /// SWG-002 4k-with-margins discipline, tables edition). THE 100k
    /// INVARIANT IS STRUCTURAL: the window math runs over the FLAT keys
    /// list, which is the def's PUBLISHED rows (≤ `MAX_WIDGET_ROWS_INITIAL`
    /// per payload — the def's own payload cap); the def's `total_rows`
    /// (the BACKING set) never enters this list and there is NO code path
    /// materializing beyond it — the API shape itself forbids a
    /// 100_000-key materialization. Rows beyond the published window
    /// arrive only through page requests + sync.
    ///
    /// Before the first sync (or a vanished table): an EMPTY
    /// materialization (the SWG-002 degenerate-list mirror — the mount
    /// renders nothing).
    pub(crate) fn materialize_rows(&self, spec: &WindowSpec) -> MaterializedRows {
        let Some(table) = self.synced.as_ref() else {
            return MaterializedRows {
                window: VisibleWindow { start: 0, end: 0 },
                keys: Vec::new(),
                published_rows: 0,
            };
        };
        let window = visible_window(&table.flat, self.scroll_index, spec);
        let keys = materialize_ids(&table.flat, window);
        MaterializedRows {
            window,
            keys,
            published_rows: table.flat.len(),
        }
    }

    // -- teardown -------------------------------------------------------------

    /// Explicit teardown (tab/plugin close, §16 "closing a tab/plugin
    /// cancels pending data requests and releases state"): every field
    /// replaced, every pending request and unsettled fact dropped, the
    /// draft released. This is the ONLY path that moves
    /// [`TableStateMutations::whole_state_resets`] — in every NORMAL flow
    /// it stays 0 (the defect probe). The synced read model is KEPT (the
    /// SWG-002 mirror precedent — `WidgetUiState::reset_all` keeps
    /// `synced` too; the machine object itself drops on surface close).
    pub(crate) fn reset_all(&mut self) {
        self.selection = Vec::new();
        self.focus = None;
        self.draft = None;
        self.scroll_index = 0;
        self.pending_pages = HashMap::new();
        self.in_flight = HashMap::new();
        self.mutations.whole_state_resets += 1;
    }
}

// ---------------------------------------------------------------------------
// Tests (the §16 criteria, where they live in the state layer)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    // Test-only imports (the non-test surface uses none of these — they
    // would be unused-import warnings at the top of the module).
    use ocs_plugin_api::scene::MAX_TEXT_BYTES;
    use ocs_plugin_api::widgets::{
        BadgeTone, IconRef, TableRef, WidgetNode, WidgetSurfacePatch, MAX_WIDGET_ROWS_INITIAL,
    };

    // -- construction helpers ------------------------------------------------

    fn sid(n: u64) -> StableId {
        StableId(n)
    }

    fn column(id: u64, kind: CellKind, label: &str) -> TableColumnDef {
        TableColumnDef {
            id: sid(id),
            label: label.to_string(),
            kind,
            width_hint: None,
            stretch: false,
        }
    }

    fn trow(key: u64, cells: Vec<TableCell>) -> TableRowDef {
        TableRowDef {
            key: sid(key),
            cells,
            domain_ref: None,
        }
    }

    /// One Table ref wired to `table_id` at surface root id 10_000.
    fn table_node(id: u64, table_id: u64) -> WidgetNode {
        WidgetNode::Table(TableRef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            table_id: sid(table_id),
        })
    }

    fn surface_with_table(revision: u64, def: PanelTableDef) -> WidgetSurface {
        WidgetSurface {
            revision: SceneRevision(revision),
            roots: vec![table_node(10_000, def.id_table.0)],
            tables: std::collections::BTreeMap::from([(def.id_table, def)]),
        }
    }

    /// A synced state for the fixture table (surface id 900).
    fn synced(def: &PanelTableDef) -> TableUiState {
        let mut state = TableUiState::new(sid(900), def.id_table);
        state.sync(&surface_with_table(1, def.clone()));
        state
    }

    /// Rows with the three-column fixture's shape: Text("note") per id,
    /// Number(id), Bool(id even).
    fn grid_rows(count: u64, note: &str) -> Vec<TableRowDef> {
        (1..=count)
            .map(|i| {
                trow(
                    i,
                    vec![
                        TableCell::Text(format!("{note} {i}")),
                        TableCell::Number(i as f64),
                        TableCell::Bool(i % 2 == 0),
                    ],
                )
            })
            .collect()
    }

    /// The three-column fixture (text 2, number 3, bool 4).
    fn three_column_def(id: u64, rows: Vec<TableRowDef>) -> PanelTableDef {
        PanelTableDef {
            id_table: sid(id),
            columns: vec![
                column(2, CellKind::Text, "note"),
                column(3, CellKind::Number, "length"),
                column(4, CellKind::Bool, "locked"),
            ],
            total_rows: None,
            rows,
        }
    }

    fn ws(viewport: f32, row_height: f32, margin: usize) -> WindowSpec {
        WindowSpec {
            viewport_height: viewport,
            row_height,
            margin_rows: margin,
        }
    }

    fn assert_events_valid(events: &[WidgetEvent]) {
        for event in events {
            event
                .validate()
                .expect("emitted event must be valid (construct-level companion)");
        }
    }

    // -- 1. one cell edit touches one row ------------------------------------

    /// THE §16 acceptance, state-side and byte-wise: commit produces
    /// EXACTLY ONE [`WidgetOp::TableCell`] fact (its serialized shape
    /// within ~200 bytes), counts ONE `cell_writes` + ONE
    /// `row_state_changes`, and — pointed at the data of record via the
    /// crate's own patch apply — exactly ONE cell of the whole grid moves.
    #[test]
    fn one_cell_edit_touches_one_row_state() {
        let def = three_column_def(1, grid_rows(64, "material row"));
        let mut state = synced(&def);
        let before_cells = state.synced().unwrap().cells.clone();

        // arm → type → commit on ONE address.
        state.begin_cell_edit(sid(1), sid(3)).expect("scalar column");
        state
            .draft_cell_value(WidgetCellValue::Number(1234.5))
            .expect("the Number column edit");
        let op = state
            .commit_cell_draft()
            .expect("the draft was armed")
            .expect("commit produces its FACT");
        assert_eq!(
            op,
            WidgetOp::TableCell {
                table: sid(1),
                key: sid(1),
                column: sid(3),
                value: WidgetCellValue::Number(1234.5),
            },
            "one cell edit = one exact WidgetOp::TableCell"
        );
        let bytes = bincode::serialize(&op).expect("serialize the fact");
        assert!(
            bytes.len() <= 200,
            "the one-cell fact = {} bytes — within the ~200 B single-cell acceptance",
            bytes.len()
        );

        // The counters: ONE cell write, ONE row state change, nothing else.
        assert_eq!(state.mutations().cell_writes, 1);
        assert_eq!(state.mutations().row_state_changes, 1);
        assert_eq!(state.mutations().whole_state_resets, 0);
        assert_eq!(state.mutations().page_requests_emitted, 0);
        assert_eq!(state.mutations().duplicate_pages_suppressed, 0);
        assert_eq!(state.mutations().invalid_cell_refusals, 0);
        assert!(state.draft().is_none(), "the draft cleared on commit");
        assert_eq!(state.in_flight().len(), 1, "the fact parked in flight");

        // The data-of-record: apply THE patch through the crate's own
        // apply path, re-sync a fresh state, and count changed cells —
        // exactly ONE cell of the whole grid differs.
        let mut surface = surface_with_table(1, three_column_def(1, grid_rows(64, "material row")));
        surface
            .apply_patch(WidgetSurfacePatch {
                base_revision: SceneRevision(1),
                revision: SceneRevision(2),
                ops: vec![op.clone()],
            })
            .expect("the one-cell patch applies");
        let mut fresh = TableUiState::new(sid(901), sid(1));
        fresh.sync(&surface);
        let after_cells = &fresh.synced().unwrap().cells;
        let mut changed = Vec::new();
        for (address, value) in after_cells {
            let moved = match before_cells.get(address) {
                Some(before) => before != value,
                None => true,
            };
            if moved {
                changed.push(*address);
            }
        }
        assert_eq!(
            changed,
            vec![(sid(1), sid(3))],
            "exactly ONE cell moved across the whole grid"
        );
        assert_eq!(after_cells.get(&(sid(1), sid(3))).unwrap(), &WidgetCellValue::Number(1234.5));

        // The fact settles cleanly (the mount's post-forward signal).
        assert!(state.settle(sid(1), sid(3)));
        assert!(!state.settle(sid(1), sid(3)), "settle is idempotent");
        assert!(state.in_flight().is_empty());
    }

    // -- 2. the FULL-TABLE REPUBLISH is the defect; bytes are the probe ------

    /// Mirror of the crate's `single_cell_patch_bytes_versus_full_table_write`,
    /// state-side: the full-def retransmit (what a defecting publisher
    /// sends whole) is ≥ 100 KB in the same fixture, while the ONE-cell
    /// fact from the state's commit path stays ≤ 200 bytes.
    #[test]
    fn full_table_republish_is_defect_sized() {
        let rows: Vec<TableRowDef> = (1..=MAX_WIDGET_ROWS_INITIAL)
            .map(|i| {
                trow(
                    i as u64,
                    vec![
                        TableCell::Text(format!("material row {i:04} description text")),
                        TableCell::Number(i as f64),
                        TableCell::Bool(i % 2 == 0),
                    ],
                )
            })
            .collect();
        let def = three_column_def(1, rows);
        def.validate()
            .expect("the full-table republish shape validates");
        let full_bytes = bincode::serialize(&def).expect("serialize the full-table republish");

        let mut state = synced(&def);
        state.begin_cell_edit(sid(1), sid(3)).expect("scalar column");
        state
            .draft_cell_value(WidgetCellValue::Number(1234.5))
            .expect("the Number column edit");
        let op = state
            .commit_cell_draft()
            .expect("armed")
            .expect("the fact");
        let one_bytes = bincode::serialize(&op).expect("serialize the one-cell fact");
        assert!(
            one_bytes.len() <= 200,
            "one-cell fact = {} bytes — full-table republish = {} bytes \
             (ARCHITECTURE §16: one cell edit transfers only its cell)",
            one_bytes.len(),
            full_bytes.len()
        );
        assert!(
            full_bytes.len() >= 100_000,
            "one-cell fact = {} bytes — full-table republish = {} bytes \
             (ARCHITECTURE §16: one cell edit transfers only its cell)",
            one_bytes.len(),
            full_bytes.len()
        );
        println!(
            "one-cell fact: {} bytes — full-table republish: {} bytes ({:0.0}x)",
            one_bytes.len(),
            full_bytes.len(),
            full_bytes.len() as f64 / one_bytes.len() as f64
        );
        // The state machine counted exactly what the crate measures.
        assert_eq!(state.mutations().cell_writes, 1);
        assert_eq!(
            state.mutations().row_state_changes, 1,
            "ONE row touched; nothing else"
        );
        assert_eq!(state.mutations().whole_state_resets, 0);
    }

    // -- 3. selection differences count only the delta -----------------------

    /// "Selection updates do not rebuild the whole tree" — the selection
    /// write is diff-based: an identical rewrite counts 0, a same-count
    /// different-content rewrite counts exactly the exchanged keys, and the
    /// cap refusal moves nothing.
    #[test]
    fn selection_diffs_count_only_the_delta() {
        let def = three_column_def(1, grid_rows(12, "row"));
        let mut state = synced(&def);

        let event = state
            .set_selection(vec![sid(3), sid(5), sid(8), sid(11)])
            .expect("host-multi-select ok")
            .expect("selection emits");
        assert_events_valid(std::slice::from_ref(&event));
        assert_eq!(
            event,
            WidgetEvent::SelectionChanged {
                surface_id: sid(900),
                selected_ids: vec![sid(3), sid(5), sid(8), sid(11)],
            },
            "the event carries the ROW KEYS verbatim, order kept"
        );
        assert_eq!(state.mutations().row_state_changes, 4);

        // Identical rewrite: 0 moved.
        let before = state.mutations().row_state_changes;
        state
            .set_selection(vec![sid(3), sid(5), sid(8), sid(11)])
            .expect("same keys");
        assert_eq!(
            state.mutations().row_state_changes - before,
            0,
            "identical rewrite = 0 (content diff, not a rebuild)"
        );
        // Same COUNT, different content: two out, two in = 4.
        state
            .set_selection(vec![sid(3), sid(11), sid(6), sid(9)])
            .expect("replaced content");
        assert_eq!(
            state.mutations().row_state_changes - before,
            4,
            "the delta, not the size"
        );
        // Shrink by three: counts 3.
        state.set_selection(vec![sid(11)]).expect("shrunk");
        assert_eq!(state.mutations().row_state_changes - before, 7);
        assert_eq!(state.selection(), &[sid(11)]);

        // The cap binds state-side; the refusal leaves everything verbatim.
        let too_many: Vec<StableId> = (1..=(MAX_SELECTION_IDS as u64 + 1)).map(sid).collect();
        let before_selection = state.selection().to_vec();
        let before = state.mutations().row_state_changes;
        assert_eq!(
            state.set_selection(too_many),
            Err(TableStateError::SelectionTooLarge {
                offered: MAX_SELECTION_IDS + 1
            })
        );
        assert_eq!(
            state.selection(),
            &before_selection,
            "a refusal leaves the selection verbatim"
        );
        assert_eq!(
            state.mutations().row_state_changes - before,
            0,
            "a refusal moves NO counters"
        );
    }

    // -- 4. the kind gate refuses non-scalar writes BEFORE any draft ---------

    /// icon/link/badge/progress/date are plugin-PUBLISHED — the crate's own
    /// apply path refuses patches naming them; the state layer refuses
    /// EARLY with the typed reason, counts each, and the edit ledger
    /// consumed NOTHING (the scalar editor still opens + commits after).
    #[test]
    fn kind_gate_refuses_writes_to_non_scalar_columns() {
        let def = PanelTableDef {
            id_table: sid(1),
            columns: vec![
                column(2, CellKind::Number, "length"),
                column(3, CellKind::Badge, "status"),
                column(4, CellKind::Icon, "icon"),
                column(5, CellKind::Link, "link"),
                column(6, CellKind::Progress, "progress"),
                column(7, CellKind::Date, "opened"),
            ],
            total_rows: None,
            rows: vec![trow(
                10,
                vec![
                    TableCell::Number(42.5),
                    TableCell::Badge {
                        label: "wip".to_string(),
                        tone: BadgeTone::Warning,
                    },
                    TableCell::Icon(IconRef {
                        uri: "host://icons/wip.svg".to_string(),
                        size: 16.0,
                    }),
                    TableCell::Link {
                        label: "docs".to_string(),
                        uri: "https://example.test/wip".to_string(),
                    },
                    TableCell::Progress {
                        fraction: 0.25,
                        label: None,
                    },
                    TableCell::Date {
                        y: 2026,
                        m: 11,
                        d: 15,
                    },
                ],
            )],
        };
        let mut state = synced(&def);
        let mut refusals = 0u64;
        for refused_column in [sid(3), sid(4), sid(5), sid(6), sid(7)] {
            let expected_kind = def
                .column_position(refused_column)
                .map(|position| def.columns[position].kind)
                .unwrap();
            let before = state.mutations().invalid_cell_refusals;
            let refusal = state
                .begin_cell_edit(sid(10), refused_column)
                .expect_err("the NON-scalar column must refuse");
            assert_eq!(
                refusal,
                TableStateError::NotWritable {
                    column: refused_column,
                    kind: expected_kind,
                }
            );
            assert_eq!(
                state.mutations().invalid_cell_refusals - before,
                1,
                "the kind gate counts each refusal"
            );
            assert!(state.draft().is_none(), "a kind refusal consumed NOTHING");
            refusals += 1;
        }
        assert_eq!(refusals, 5, "each non-scalar kind refused once");
        assert_eq!(state.mutations().invalid_cell_refusals, 5);

        // The scalar column still opens + commits (the kind refusals did
        // not consume the edit ledger).
        state.begin_cell_edit(sid(10), sid(2)).expect("scalar");
        state
            .draft_cell_value(WidgetCellValue::Number(7.0))
            .expect("the scalar column is writable");
        let op = state
            .commit_cell_draft()
            .expect("armed")
            .expect("the fact");
        assert!(matches!(op, WidgetOp::TableCell { table, .. } if table == sid(1)));
        assert_eq!(
            state.mutations().invalid_cell_refusals, 5,
            "success consumed nothing further"
        );
        assert_eq!(state.mutations().cell_writes, 1);
        assert!(state.settle(sid(10), sid(2)));
    }

    // -- 5. the schema-change re-sync discards drafts + prunes gracefully ----

    /// The CRITICAL re-sync: a column's kind change under an open draft
    /// discards the draft on the DEDICATED counter (refusal-free); the
    /// re-arm re-seeds from the NEW kind's cell; the selection survives
    /// untouched when its keys survive; and NONE of it moves
    /// `row_state_changes`/`whole_state_resets`.
    #[test]
    fn schema_change_discards_drafts_and_prunes_gracefully() {
        let def = three_column_def(1, grid_rows(5, "row A"));
        let mut state = synced(&def);
        state
            .set_selection(vec![sid(2), sid(4)])
            .expect("selected");
        state.begin_cell_edit(sid(2), sid(3)).expect("the Number column");
        state
            .draft_cell_value(WidgetCellValue::Number(9.0))
            .expect("drafted");
        assert!(state.draft().is_some());
        let changes_before = state.mutations().row_state_changes;

        // The plugin re-publishes the def (same table id, different column
        // KIND — the schema changed under the open draft).
        let rows_text: Vec<TableRowDef> = (1..=5)
            .map(|i| {
                trow(
                    i,
                    vec![
                        TableCell::Text(format!("row A {i}")),
                        TableCell::Text(format!("{i}")),
                        TableCell::Bool(i % 2 == 0),
                    ],
                )
            })
            .collect();
        let mut def2 = three_column_def(1, rows_text);
        def2.columns[1].kind = CellKind::Text;
        state.sync(&surface_with_table(2, def2));

        assert!(state.draft().is_none(), "the kind-changed draft is discarded");
        assert_eq!(
            state.mutations().drafts_discarded_on_schema_change,
            1,
            "the discard counts the DEDICATED counter, exactly once"
        );
        assert_eq!(
            state.mutations().row_state_changes - changes_before,
            0,
            "a schema discard is NOT a row write"
        );
        assert_eq!(state.mutations().invalid_cell_refusals, 0);
        assert_eq!(
            state.mutations().whole_state_resets,
            0,
            "reconcile is NOT a reset"
        );
        assert_eq!(state.selection(), &[sid(2), sid(4)], "surviving keys kept");

        // The re-arm onto the new schema re-seeds from the NEW cell kind.
        state
            .begin_cell_edit(sid(2), sid(3))
            .expect("re-arms onto the new schema");
        let draft = state.draft().expect("draft");
        assert!(
            matches!(draft.pending, WidgetCellValue::Text(_)),
            "the new draft's seed matches the NEW column kind"
        );
        state.cancel_cell_draft(); // free — covered by the teardown test too
        assert!(state.draft().is_none());
    }

    // -- 6. page-request dedup + arrival closure -----------------------------

    /// The page-request anti-storm: the keyed request dedups while in
    /// flight (suppressed, counted); a mid-table anchor requests NOTHING
    /// (only the published TAIL extends); the ARRIVAL (the plugin's
    /// answering snapshot lands rows past the anchor — the anchor stops
    /// being the tail) closes the dedup window; afterwards re-requesting
    /// the tail emits again.
    #[test]
    fn page_requests_dedup_and_close_on_arrival() {
        let def = PanelTableDef {
            id_table: sid(1),
            columns: vec![column(2, CellKind::Number, "length")],
            total_rows: Some(20_000),
            rows: (1..=MAX_WIDGET_ROWS_INITIAL)
                .map(|i| trow(i as u64, vec![TableCell::Number(i as f64)]))
                .collect(),
        };
        let mut state = synced(&def);

        // The tail anchor: the LAST published key extends the window.
        let event = state
            .page_request_if_needed(sid(MAX_WIDGET_ROWS_INITIAL as u64), 4_096)
            .expect("the table windows; the anchor is the tail");
        match &event {
            WidgetEvent::TablePageRequested {
                table,
                first_key,
                len,
                ..
            } => {
                assert_eq!(*table, sid(1));
                assert_eq!(*first_key, sid(MAX_WIDGET_ROWS_INITIAL as u64));
                assert_eq!(*len, 4_096);
            }
            other => panic!("expected a page request, got {other:?}"),
        }
        assert_events_valid(std::slice::from_ref(&event));
        assert_eq!(state.mutations().page_requests_emitted, 1);

        // The dedup: re-requesting the SAME first_key while in flight
        // suppresses + counts.
        assert!(
            state
                .page_request_if_needed(sid(MAX_WIDGET_ROWS_INITIAL as u64), 4_096)
                .is_none()
        );
        assert_eq!(state.mutations().page_requests_emitted, 1);
        assert_eq!(state.mutations().duplicate_pages_suppressed, 1);

        // A mid-table anchor requests nothing (only the tail extends) —
        // and counts NOTHING (not a storm, just not-your-turn).
        assert!(state.page_request_if_needed(sid(4_095), 100).is_none());
        assert_eq!(state.mutations().page_requests_emitted, 1);
        assert_eq!(state.mutations().duplicate_pages_suppressed, 1);

        // The ARRIVAL: the plugin publishes its next window (the page the
        // request named — rows past the anchor), sync closes the window.
        let paged = PanelTableDef {
            id_table: sid(1),
            columns: vec![column(2, CellKind::Number, "length")],
            total_rows: Some(20_000),
            rows: (4_095..=8_190)
                .map(|i| trow(i, vec![TableCell::Number(i as f64)]))
                .collect(),
        };
        state.sync(&surface_with_table(2, paged));
        assert!(
            state.pending_pages().is_empty(),
            "the dedup windows CLOSE (rows ARRIVED past the anchor)"
        );
        assert_eq!(
            state.mutations().page_requests_emitted, 1,
            "arrival emits nothing, suppression uncounted on close"
        );

        // Requesting the NEW tail after arrival emits again (windows CLOSE
        // on arrival, forever — the dedup never wedges).
        let event = state
            .page_request_if_needed(sid(8_190), 4_096)
            .expect("re-request after arrival");
        assert_events_valid(std::slice::from_ref(&event));
        assert_eq!(state.mutations().page_requests_emitted, 2);
        // And the fresh window dedups identically.
        assert!(state.page_request_if_needed(sid(8_190), 4_096).is_none());
        assert_eq!(state.mutations().duplicate_pages_suppressed, 2);
    }

    // -- 7. 100k backing rows materialize the window ONLY ---------------------

    /// "100k backing objects do not create 100k host widgets" — the table
    /// edition, STRUCTURALLY: the flat keys list is the def's PUBLISHED
    /// window (4_096 — the payload cap), the backing total (100_000) never
    /// enters any materialization, and a paged request extends the
    /// published window only through the plugin's own snapshot.
    #[test]
    fn typed_rows_number_far_beyond_window_materialize_window_only() {
        let def = PanelTableDef {
            id_table: sid(1),
            columns: vec![column(2, CellKind::Number, "length")],
            total_rows: Some(100_000),
            rows: (1..=MAX_WIDGET_ROWS_INITIAL)
                .map(|i| trow(i as u64, vec![TableCell::Number(i as f64)]))
                .collect(),
        };
        let mut state = synced(&def);
        let model = state.synced().expect("synced");
        assert_eq!(
            model.rows.len(),
            MAX_WIDGET_ROWS_INITIAL,
            "the flat list is the PUBLISHED window, not the backing total"
        );
        assert_eq!(model.total_rows, Some(100_000), "the backing total is metadata");

        // Top of the table: a viewport of 4 rows + 8-margin context.
        let materialized = state.materialize_rows(&ws(88.0, 22.0, 8));
        assert_eq!(materialized.published_rows, MAX_WIDGET_ROWS_INITIAL);
        assert_eq!(materialized.window.start, 0);
        assert_eq!(materialized.window.end, 12, "4 visible + 8-margin tail, clamped");
        assert_eq!(materialized.keys.len(), 12);
        assert_eq!(materialized.keys[0], sid(1));
        assert_eq!(materialized.keys[11], sid(12));

        // Deep scroll: the window follows the anchor; STILL windowed.
        state.set_scroll_index(100_000); // beyond the PUBLISHED list: clamps
        let materialized = state.materialize_rows(&ws(88.0, 22.0, 8));
        assert_eq!(
            materialized.window.start,
            MAX_WIDGET_ROWS_INITIAL - 12,
            "the anchor clamps INSIDE the published window"
        );
        assert_eq!(materialized.window.end, MAX_WIDGET_ROWS_INITIAL);
        assert_eq!(materialized.keys.len(), 12);
        assert_eq!(materialized.keys[0], sid(4_085));
        assert_eq!(materialized.keys[11], sid(4_096));

        // Structural proof: 100_000 backing rows can NEVER be materialized
        // — the published-window count is the ceiling of every window.
        assert!(
            materialized.keys.len() < 100_000,
            "materialized {} keys for a 100_000-row backing set — the window held \
             (ARCHITECTURE §16: 100k backing objects do not create 100k host widgets)",
            materialized.keys.len()
        );
        println!(
            "backing: 100_000 — published: {} — materialized keys: {} ({:0.2}% of the backing set)",
            materialized.published_rows,
            materialized.keys.len(),
            materialized.keys.len() * 100 / 100_000
        );

        // Paged: the request names the NEXT window; the published window is
        // STILL the only materialization (page arrival is a LATER snapshot).
        let page = state.page_request_if_needed(sid(4_096), 4_096);
        assert!(page.is_some());
        assert_eq!(state.mutations().page_requests_emitted, 1);
        let materialized_after = state.materialize_rows(&ws(88.0, 22.0, 8));
        assert_eq!(
            materialized_after.keys.len(),
            materialized.keys.len(),
            "the in-flight page never materializes anything by itself"
        );
    }

    // -- 8. keyboard roving stays inside the window ---------------------------

    /// The roving boundary: focus moves ONLY among the MATERIALIZED rows
    /// (row edition), the cells ride the focused row's columns, and a
    /// scrolled-away focus re-anchors INSIDE the new window.
    #[test]
    fn keyboard_roving_stays_inside_the_window() {
        let def = three_column_def(1, grid_rows(40, "row"));
        let mut state = synced(&def);
        state.set_scroll_index(10);
        // 8 visible rows + 2-margin context → window [8, 20) over 40 rows.
        let spec = ws(80.0, 10.0, 2);
        let materialized = state.materialize_rows(&spec);
        assert_eq!(materialized.window.start, 8);
        assert_eq!(materialized.window.end, 20);

        // Down through the window rows (sid(9) ..= sid(20)), then the edge.
        let mut walked = Vec::new();
        loop {
            match state.row_focus_next(&spec) {
                Some(key) => walked.push(key),
                None => break,
            }
        }
        assert_eq!(
            walked,
            (9..=20).map(sid).collect::<Vec<_>>(),
            "the focus walks exactly the window rows, none beyond"
        );

        // Backward from the window's bottom down to its start, then the edge.
        let mut back = Vec::new();
        loop {
            match state.row_focus_prev(&spec) {
                Some(key) => back.push(key),
                None => break,
            }
        }
        assert_eq!(
            back,
            (9..=19).map(sid).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>(),
            "the backward walk halts at the window's start edge"
        );
        // The walk left the focus on the window's FIRST row, sid(9).
        assert_eq!(state.focus(), Some(sid(9)));

        // Cells: Left/Right rides the focused row's columns (in registry
        // order), the row cursor never crossing. Re-anchor at the window's
        // bottom first (the window covers sid(9)..=sid(20)).
        state.row_focus_set(sid(20), &spec).expect("inside the window");
        assert_eq!(state.focus(), Some(sid(20)));
        let columns = [sid(2), sid(3), sid(4), sid(999) /* junk, skipped */];
        let first = state.cell_focus_next(None, &columns).expect("the first column");
        assert_eq!(first, CellFocus { key: sid(20), column: sid(2) });
        assert_eq!(state.cell_focus_next(Some(first), &columns).map(|focus| focus.column), Some(sid(3)));
        let last = state
            .cell_focus_next(Some(CellFocus { key: sid(20), column: sid(3) }), &columns)
            .expect("the last column");
        assert_eq!(last.column, sid(4));
        assert!(
            state.cell_focus_next(Some(last), &columns).is_none(),
            "the row's column edge (forward)"
        );
        let back_one = state.cell_focus_prev(Some(last), &columns).expect("backward");
        assert_eq!(back_one.column, sid(3));
        assert!(
            state.cell_focus_prev(Some(CellFocus { key: sid(20), column: sid(2) }), &columns)
                .is_none(),
            "the row's column edge (backward)"
        );

        // The re-anchor: a scroll moves the window to [22, 34); the focus
        // (sid(20) — index 19, NOW outside) re-anchors INSIDE the new
        // window on the next roving move.
        state.set_scroll_index(24);
        let materialized = state.materialize_rows(&spec);
        assert_eq!(
            materialized.window.start, 22,
            "the window re-materialized around the new scroll anchor"
        );
        assert_eq!(materialized.window.end, 34);
        let ans = state.row_focus_next(&spec).expect("re-anchored");
        assert_eq!(ans, sid(23), "the re-anchor lands on the new window's first row");

        // The click-hand-off focus_set: WINDOW-bound refusals + positives.
        assert!(state.row_focus_set(sid(5), &spec).is_err(), "outside the window");
        assert_eq!(
            state.row_focus_set(sid(4_096_000), &spec),
            Err(TableStateError::UnknownRow(sid(4_096_000))),
            "an unknown key refuses"
        );
        state.row_focus_set(sid(25), &spec).expect("inside the window");
        assert_eq!(state.focus(), Some(sid(25)));
    }

    // -- 9. the invalid VALUES (not kind mismatches) refuse -------------------

    /// The documented duty split: the kind-class gate is the LOCAL mirror
    /// (three arms); the VALUE-level validity DELEGATES to the crate's own
    /// validate path (the probe def — the crate's registry validation run
    /// UNMODIFIED). A kind-correct but invalid value (NaN, an over-cap
    /// text) refuses with the crate's own reason; the kind-mismatch case
    /// stays a DISTINCT typed error; and the same probe refuses the
    /// skeleton-unexpressible shapes the crate owns (a 1.5 Progress
    /// fraction) — recorded so the trust surface is explicit.
    #[test]
    fn not_kind_mismatch_but_invalid_cell_values_refuse() {
        let def = three_column_def(1, grid_rows(4, "row"));
        let mut state = synced(&def);

        state.begin_cell_edit(sid(1), sid(3)).expect("the Number column");
        // The seeded pending is the CURRENT cell (Number(1.0)):
        assert!(matches!(&state.draft().unwrap().pending, WidgetCellValue::Number(seed) if *seed == 1.0));
        let before = state.mutations().invalid_cell_refusals;
        // NaN: kind-correct, structurally invalid — the crate's own gate.
        let refusal = state
            .draft_cell_value(WidgetCellValue::Number(f64::NAN))
            .expect_err("NaN must refuse");
        assert!(
            matches!(refusal, TableStateError::InvalidCellValue { .. }),
            "the crate's delegated path; not the kind gate"
        );
        assert_eq!(state.mutations().invalid_cell_refusals - before, 1);
        // The draft remains EXACTLY as it was (retriable, nothing consumed).
        assert!(matches!(&state.draft().unwrap().pending, WidgetCellValue::Number(seed) if *seed == 1.0));
        // Infinity too.
        assert!(matches!(
            state.draft_cell_value(WidgetCellValue::Number(f64::INFINITY)),
            Err(TableStateError::InvalidCellValue { .. })
        ));
        assert_eq!(state.mutations().invalid_cell_refusals, before + 2);
        // A kind-correct, valid number passes and commits the updated value.
        state
            .draft_cell_value(WidgetCellValue::Number(3.25))
            .expect("a within-gate value");
        let op = state
            .commit_cell_draft()
            .expect("armed")
            .expect("the fact");
        assert!(matches!(
            op,
            WidgetOp::TableCell { key: StableId(1), column: StableId(3), value: WidgetCellValue::Number(n), .. }
                if n == 3.25
        ));
        assert!(state.settle(sid(1), sid(3)));

        // The TEXT cap (kind-correct, past the crate's byte cap — the
        // delegated gate again).
        state.begin_cell_edit(sid(2), sid(2)).expect("the Text column");
        let before = state.mutations().invalid_cell_refusals;
        assert!(matches!(
            state.draft_cell_value(WidgetCellValue::Text("x".repeat(MAX_TEXT_BYTES + 1))),
            Err(TableStateError::InvalidCellValue { .. })
        ));
        assert_eq!(
            state.mutations().invalid_cell_refusals - before,
            1,
            "the crate's own cap, one count"
        );
        state
            .draft_cell_value(WidgetCellValue::Text("within cap".to_string()))
            .expect("a within-cap value");
        state.cancel_cell_draft(); // the text draft is not committed on purpose

        // The kind-mismatch CONTRAST: a Bool value into a Number column
        // (kind-correct VALUE-set, wrong class) — the LOCAL gate's typed error.
        state.begin_cell_edit(sid(3), sid(3)).expect("the Number column");
        let before = state.mutations().invalid_cell_refusals;
        assert!(matches!(
            state.draft_cell_value(WidgetCellValue::Bool(true)),
            Err(TableStateError::KindMismatch {
                expected: CellKind::Number,
                ..
            })
        ));
        assert_eq!(
            state.mutations().invalid_cell_refusals - before,
            1,
            "the mismatch counts the same counter"
        );
        state.cancel_cell_draft();

        // The crate's own registry validation (UNMODIFIED under the
        // delegated probe) refuses the SKELETON-unexpressible invalid
        // values directly — recorded so the trust surface is explicit:
        // fraction 1.5 on a Progress column.
        let def_bad_progress = PanelTableDef {
            id_table: sid(1),
            columns: vec![column(2, CellKind::Progress, "progress")],
            total_rows: None,
            rows: vec![trow(10, vec![TableCell::Progress { fraction: 1.5, label: None }])],
        };
        let refusal = def_bad_progress
            .validate()
            .expect_err("the crate's own gate refuses fraction 1.5");
        assert!(matches!(refusal, WidgetError::InvalidCell { .. }));
        // And the same 1×1 shape through the machine's probe:
        let probe_refused = probe_typed_cell(
            sid(1),
            sid(10),
            sid(2),
            CellKind::Number,
            &WidgetCellValue::Number(f64::NAN),
        )
        .expect_err("the probe's crate-level validation");
        assert!(matches!(probe_refused, WidgetError::InvalidCell { .. }));
        assert_eq!(
            state.mutations().invalid_cell_refusals,
            before + 1,
            "the direct crate-path probes are NOT state refusals (no draft was touched)"
        );
    }

    // -- 10. the teardown counter --------------------------------------------

    /// ONLY `reset_all` (teardown) moves `whole_state_resets`; every normal
    /// flow leaves it at 0 (the probe); the teardown releases
    /// selection/focus/draft/pages/facts (and keeps the read model — the
    /// SWG-002 mirror precedent) and fires per call.
    #[test]
    fn teardown_resets_everything_and_is_the_only_reset() {
        let def = PanelTableDef {
            id_table: sid(1),
            columns: vec![
                column(2, CellKind::Number, "length"),
                column(3, CellKind::Text, "note"),
            ],
            total_rows: Some(50_000),
            rows: (1..=1_000)
                .map(|i| {
                    trow(
                        i as u64,
                        vec![TableCell::Number(i as f64), TableCell::Text(format!("row {i}"))],
                    )
                })
                .collect(),
        };
        let mut state = synced(&def);

        // Every normal flow: selection, arm→type→commit→settle, page
        // request, materialize, re-sync — the reset probe stays 0.
        state.set_selection(vec![sid(2), sid(4)]).expect("selected");
        state.begin_cell_edit(sid(2), sid(2)).expect("armed");
        state
            .draft_cell_value(WidgetCellValue::Number(9.0))
            .expect("typed");
        state.commit_cell_draft().expect("committed");
        assert!(state.settle(sid(2), sid(2)));
        state
            .page_request_if_needed(sid(1_000), 1_000)
            .expect("the page request");
        let _ = state.materialize_rows(&ws(80.0, 16.0, 2));
        state.sync(&surface_with_table(2, def.clone()));
        state.cancel_cell_draft();
        assert_eq!(
            state.mutations().whole_state_resets,
            0,
            "no normal flow rebuilds the whole state"
        );
        assert!(state.mutations().row_state_changes > 0);
        assert_eq!(state.mutations().cell_writes, 1);
        assert_eq!(state.mutations().page_requests_emitted, 1);

        // THE teardown — the ONE path that fires the reset counter.
        state.reset_all();
        assert_eq!(state.mutations().whole_state_resets, 1);
        assert!(state.selection().is_empty());
        assert!(state.focus().is_none());
        assert!(state.draft().is_none());
        assert!(state.pending_pages().is_empty());
        assert!(state.in_flight().is_empty());
        assert_eq!(state.scroll_index(), 0);
        assert!(state.synced().is_some(), "the synced read model is KEPT");
        // The other §16 counters are NOT reset by the teardown (they are
        // per-machine defect history, not state).
        assert_eq!(state.mutations().cell_writes, 1);
    }

    // -- 11. sync prunes ONLY dangling keys -----------------------------------

    /// Selection/focus/draft keyed to rows that vanish are pruned; the
    /// prune counts ONLY-IF-PRESENT (the SWG-002 policy); the new rows
    /// adopt NOTHING (identity = key); the draft's row-wise prune counts
    /// as a row write; sync's schema-change counter stays untouched.
    #[test]
    fn sync_prunes_only_dangling_keys() {
        let def = three_column_def(1, grid_rows(12, "row"));
        let mut state = synced(&def);
        state
            .set_selection(vec![sid(3), sid(5), sid(8), sid(11)])
            .expect("selected");
        // Focus + draft on row 8: the whole state is keyed to the rows.
        let wide = ws(120.0, 1.0, 0); // the WHOLE published list materializes
        state.row_focus_set(sid(8), &wide).expect("inside");
        state.begin_cell_edit(sid(8), sid(3)).expect("armed");
        state
            .draft_cell_value(WidgetCellValue::Number(6.5))
            .expect("drafted");
        assert!(state.draft().is_some());

        // The plugin's patch: rows 5 and 8 vanish; NEW rows 100..110
        // arrive (identity = key — nothing pre-selected).
        let mut kept: Vec<TableRowDef> = grid_rows(3, "survivor");
        kept.push(trow(
            11,
            vec![
                TableCell::Text("survivor 11".to_string()),
                TableCell::Number(11.0),
                TableCell::Bool(false),
            ],
        ));
        for i in 100..110u64 {
            kept.push(trow(
                i,
                vec![
                    TableCell::Text(format!("new {i}")),
                    TableCell::Number(i as f64),
                    TableCell::Bool(true),
                ],
            ));
        }
        let patched = three_column_def(1, kept);
        let changes_before = state.mutations().row_state_changes;
        let schema_before = state.mutations().drafts_discarded_on_schema_change;
        state.sync(&surface_with_table(2, patched));

        assert_eq!(
            state.selection(),
            &[sid(3), sid(11)],
            "the 2 dangling keys pruned, the survivors kept in order"
        );
        assert_eq!(
            state.mutations().row_state_changes - changes_before,
            3, // 2 selection prunes + the discarded DRAFT's row prune
            "the prune counts ONLY the entries that were actually present; \
             the new rows adopt nothing"
        );
        assert_eq!(
            state.mutations().drafts_discarded_on_schema_change - schema_before,
            0,
            "the ROW vanished — a data prune, NOT a schema discard"
        );
        assert!(state.draft().is_none(), "the draft on the vanished row pruned");
        assert_eq!(state.focus(), None, "dangling focus prunes to None");
        assert_eq!(state.mutations().whole_state_resets, 0);
    }
}
