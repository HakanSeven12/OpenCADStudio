//! SCN-022 POINTER-BURST interaction bench — `widget_interaction`.
//!
//! The §7 pointer path under burst load: a parked-move storm through the
//! move coalescer and a drag machine storm (begin/move/settle/cancel) must
//! stay ≤1-in-flight, LATEST-WINS, and LOSSLESS at the settle/cancel
//! boundary. This bench holds the wall-clock + percentile + counter-
//! evolution evidence for those contracts.
//!
//! # Bench-visibility reality (measured surface, stated per scenario)
//!
//! A bench target compiles as a SEPARATE external crate: it sees only `pub`
//! items. The machines whose behavior this task cares about are ALL
//! `pub(crate)` in the host crate and therefore bench-invisible (grep
//! evidence at this branch tip: `src/ui/window/plugin_drag.rs` —
//! `DragSession`, `DragCounters`, `DragPhase`, every method `pub(crate)`;
//! `src/ui/window/plugin_widget_mount.rs` — `MountInputState`/`HoverState`,
//! `mount_move_defer`/`mount_flush_moves`/`mount_pointer_released`,
//! `MountCounters.mount_move_calls`/`coalesced_moves_superseded` all
//! `pub(crate)`). Wider `pub` seams on host sources are NOT this task's to
//! make (no src/ edits), so per the coordinator's binding decision this
//! bench is a CONTRACT-SHAPED **REPLICA** at the `ocs_plugin_api` pub layer:
//! bench-side structures that mirror the two machines field-for-field
//! (same state shape, same transitions, same counter moves), driven over
//! the pub API vocabulary — `WidgetSurface`/`WidgetNode`/`TreeRowDef`
//! (the anchored draggable address space, validated at construction),
//! `StableId`/`SceneRevision`, `DragMeta` (the verbatim payload), and the
//! real `iced::Point` the host machines use.
//!
//! Honest-byte rule (SCN-012 convention, binding): every scenario labels
//! WHICH surface it measured — these are REPLICA numbers, not regisistry
//! numbers — and every `pub(crate)` half is recorded as a `scope:
//! "deferred"` `Counter` pointing at the in-source unit tests that hold the
//! authoritative evidence today. Both facts print every run. The replica
//! CALIBRATES the contract; the real path PINS it via tests (§7
//! anti-defect: ≤1-in-flight under load, lossless storm settle/cancel).
//!
//! Deferred authority map (the real-path evidence this bench points at):
//! - Mount coalescer: `n_moves_coalesce_to_one_mount_call`,
//!   `superseded_move_overwrites_owed_slot`,
//!   `mount_move_calls_never_exceed_one_per_tick`,
//!   `capture_completes_on_release_outside`,
//!   `unknown_surface_moves_are_graceful` (all
//!   `src/ui/window/plugin_widget_mount.rs` tests), plus the viewport
//!   coalescer precedent (`defer_pointer_move`; the 986-positions
//!   page-queue defect this slot shape exists to prevent).
//! - Drag machine: `begin_move_settle_is_lossless`,
//!   `cancel_paths_are_one_to_one_with_begins`,
//!   `rapid_moves_latest_wins_never_drops_the_last`,
//!   `duplicate_begin_refused_in_flight`,
//!   `threshold_not_met_release_is_not_a_drag`,
//!   `counters_are_the_defect_probe` (all
//!   `src/ui/window/plugin_drag.rs` tests).
//!
//! # Scenarios
//!
//! 1. `park_storm_10k` — 10 000 pointer moves deferred into ONE park slot
//!    (the `move_owed` shape): the slot never grows (constant footprint,
//!    asserted per sample), it holds EXACTLY the newest position, and the
//!    bench-emulated `Vec` page-queue anti-pattern (the same 10 000 moves
//!    queued) is measured beside it as the growth the slot must not have.
//! 2. `drain_delivers_latest_only` — the 10 000-park storm answered by ONE
//!    tick flush = exactly one delivery = the storm's LAST position
//!    (latest-wins delivery; lossless = the latest defines). Percentiles
//!    over 1 000 tick-flushes and over the release-completion path.
//! 3. `settle_cancel_storm` — 1 000 begin/settle pairs + 250 cancels per
//!    storm through the DragSession-replica rail: every interaction
//!    completes exactly once, every completion DRAINS the parked slot
//!    first (the newest move is never dropped at the boundary), the 1:1
//!    counter identity `begins == settles + cancels` holds, and re-arm
//!    (armed→in-flight→settled→armed again) costs are timed beside fresh
//!    rails. Counter evolution is printed at storm milestones.
//! 4. `keyboard_nav_burst` — DEFERRED (recorded, not run): the §7 keyboard
//!    focus ring is host state (`nav_ring`/`focus_next`/`focus_prev`,
//!    pub(crate)); no pub api-crate surface exists. n=0, `executed=false`.
//!
//! Run: `cargo bench --locked --bench widget_interaction` (bench profile =
//! release; per-scenario sample Ns documented beside the constants). The
//! run-record JSON reuses the `scn-012-run-record/v1` printer format via
//! `benches/support/scene_support.rs` INCLUDED UNEDITED (SCN-012's shared
//! harness; `benches/README.md`: harness=false, never run by `cargo test`).

#![allow(dead_code)] // each replica subset; the support module does the same

use std::collections::HashSet;
use std::hint::black_box;
use std::mem;

use iced::Point;
use ocs_plugin_api::scene::{DragMeta, SceneRevision, StableId};
use ocs_plugin_api::widgets::{
    TreeCapabilities, TreeRowDef, WidgetNode, WidgetSurface, WidgetTreeDef,
};

#[path = "support/scene_support.rs"]
mod scene_support;

use scene_support::{
    env_block, param, timed_ns, timed_samples, Counter, EnvBlock, ExtraMetric, RunRecord,
    RUN_RECORD_FORMAT, ScenarioRecord, WallPercentiles,
};

// ---------------------------------------------------------------------------
// Fixed workload contract
// ---------------------------------------------------------------------------

/// Pointer moves in one parked-move storm (scenarios 1/2).
const STORM_MOVES: usize = 10_000;
/// Rows of the anchored tree — the draggable address space the rail's
/// `draggable` set mirrors (the § "synced def" of the drag machine).
const ANCHOR_ROWS: usize = 64;
/// The anchored tree's row-id base (SCN-012's bench id namespace, so the
/// replica's addresses are recognizably the same row ids).
const TREE_ROW_BASE: u64 = 20_000;
/// The per-surface rail/surface key (the host drag tests' fixture id).
const PANEL_SURFACE_ID: u64 = 900;
/// Anchored surface revision (`SceneRevision` machinery).
const ANCHOR_REVISION: u64 = 1;

/// Sample policies (README discipline: fast ops ≥ 100, slow ops ≥ 20).
const WARMUP: usize = 3;
const SAMPLES_STORM: usize = 40; // park_storm_10k: one 10k-move storm per sample
const SAMPLES_DELIVER: usize = 60; // 10k parks + ONE tick flush per sample
const SAMPLES_STORM_INTERACT: usize = 24; // settle_cancel_storm: one 1_250-interaction storm per sample
const SAMPLES_ANTI: usize = 20; // Vec page-queue anti-pattern storm
const SAMPLES_STABILITY: usize = 20; // slot reset-stability cycle
const SAMPLES_REARM: usize = 500; // re-arm cycle timing (per direction)

/// Scenario 2's tick-flush percentile population (the assignment's "1k
/// tick-flushes") and the release-completion population.
const SAMPLES_FLUSH_TAKE: usize = 1_000;
const SAMPLES_RELEASE: usize = 200;

/// Scenario 3's storm shape: `SETTLE_PER_STORM` begin/settle pairs plus
/// `CANCELS_PER_STORM` cancels; park channels per interaction kind.
const SETTLE_PER_STORM: usize = 1_000;
const CANCELS_PER_STORM: usize = 250;
const INTERACTIONS: usize = SETTLE_PER_STORM + CANCELS_PER_STORM;
const PARKS_DRIP: usize = 5; // drip settles: park→take→release ×5 (per-tick delivery shape)
const PARKS_CHURN: usize = 5; // churn settles: 5 parks NO ticks (settle drains all)
const PARKS_CANCEL: usize = 2; // cancels: cross + 2 parks, cancel drains

/// The arm→InFlight threshold the replica mirrors (the pub(crate)
/// `DRAG_THRESHOLD_PX = 3.0` value of `plugin_drag.rs`; the magnitude is
/// that module's documented decision — 3 px sits below the grip pick
/// apertures but above hand-shake jitter).
const BENCH_DRAG_THRESHOLD_PX: f32 = 3.0;

/// Deterministic storm position `i` (dyadic floats: exactly representable,
/// so the newest-position comparisons are EXACT, not approximate).
fn storm_point(i: usize) -> Point {
    let i = i as f32;
    Point::new(1_000.0 + i * 0.25, 500.0 - i * 0.125)
}

// ---------------------------------------------------------------------------
// The anchored API-crate fixture (pub ocs_plugin_api vocabulary)
// ---------------------------------------------------------------------------

/// A flat tree of [`ANCHOR_ROWS`] enabled rows — the surface whose rows'
/// ids the replica rail treats as its synced draggable set. Validates at
/// construction (a bench fixture that fails validate has no right to time
/// anything). Nothing is added to the API crate; bench-side construction
/// only, per the SCN-012 precedent.
fn anchored_surface() -> WidgetSurface {
    let rows: Vec<TreeRowDef> = (0..ANCHOR_ROWS)
        .map(|idx| TreeRowDef {
            id: StableId(TREE_ROW_BASE + idx as u64),
            parent: None,
            label: format!("row-{idx}"),
            icon: None,
            status: Some("static".to_owned()),
            expandable: false,
            enabled: true,
            lazy_children: false,
            domain_ref: None,
        })
        .collect();
    let surface = WidgetSurface {
        revision: SceneRevision(ANCHOR_REVISION),
        tables: std::collections::BTreeMap::new(),
        roots: vec![WidgetNode::Tree(WidgetTreeDef {
            id: StableId(1),
            name: Some("scn-022 pointer-burst anchor".to_owned()),
            enabled: true,
            visible: true,
            label: None,
            capabilities: TreeCapabilities {
                multi_select: false,
                filterable: false,
                lazy_children: false,
            },
            rows,
        })],
    };
    surface
        .validate()
        .expect("bench fixture must validate");
    surface
}

/// The anchored rows' ids (the replica rail's synced draggable set — the
/// mirror of `DragSession::sync_draggables` fed from the mount's def walk).
fn anchor_addresses(surface: &WidgetSurface) -> HashSet<StableId> {
    let mut ids = HashSet::new();
    for node in &surface.roots {
        if let WidgetNode::Tree(def) = node {
            for row in &def.rows {
                ids.insert(row.id);
            }
        }
    }
    assert_eq!(
        ids.len(),
        ANCHOR_ROWS,
        "the anchored draggable set must be the full row set"
    );
    ids
}

/// The plain fixture meta (the plugin's verbatim drag payload; the host
/// never interprets it — the replica returns it verbatim too).
fn meta(label: &str) -> DragMeta {
    DragMeta {
        opaque: Some(label.to_owned()),
    }
}

// ---------------------------------------------------------------------------
// REPLICA 1: the mount coalescer (MountInputState::HoverState's owed-move
// slot + mount_move_defer / mount_flush_moves / mount_pointer_released)
// ---------------------------------------------------------------------------

/// BENCH-SIDE REPLICA of the mount coalescer entry's contract, field-for-
/// field against `src/ui/window/plugin_widget_mount.rs`:
///
/// - ONE `move_owed: Option<Point>` slot per surface (LATEST-WINS, §7):
///   a new move OVERWRITES the slot (the superseded one is counted,
///   never queued — nothing in this structure can grow).
/// - The flush (`mount_flush_moves`) takes the slot: per surface at most
///   ONE delivery per flush, recorded into `last_move` and counted
///   (`mount_move_calls`) — n moves per tick = ≤1 mount call.
/// - The release completion (`mount_pointer_released`) delivers the last
///   owed move NOW (≤1 in flight per surface — the contract's other end).
#[derive(Default)]
struct MountCoalescerReplica {
    /// The owed-move slot: the newest position no tick has answered yet.
    move_owed: Option<Point>,
    /// The last DELIVERED move's position.
    last_move: Option<Point>,
    /// Pointer capture (the press path; the release completion answers it).
    captured: bool,
    /// Replica mount calls: one per surface per flush/release delivery.
    mount_move_calls: u64,
    /// Replica latest-wins overwrites (park churn stays visible).
    coalesced_moves_superseded: u64,
}

impl MountCoalescerReplica {
    /// `mount_move_defer`: a pure slot write — `replace` the slot; a
    /// superseded housekeeping count. NO queue, NO growth.
    fn mount_move_defer(&mut self, position: Point) -> bool {
        let superseded = self.move_owed.replace(position).is_some();
        if superseded {
            self.coalesced_moves_superseded += 1;
        }
        superseded
    }

    /// `mount_flush_moves`: answer the surface's LAST owed move — at most
    /// one delivery per flush (the slot is a single `Option`; structurally
    /// un-queueable). Returns the delivered count (0 or 1).
    fn mount_flush_moves(&mut self) -> usize {
        let delivered = match self.move_owed.take() {
            Some(position) => {
                self.last_move = Some(position);
                1
            }
            None => 0,
        };
        self.mount_move_calls += delivered as u64;
        delivered
    }

    /// `mount_pointer_released`: capture OFF and the pending owed move
    /// delivered NOW (0 or 1 — the ≤1-in-flight contract's completion end).
    fn mount_pointer_released(&mut self) -> usize {
        self.captured = false;
        let delivered = self.mount_flush_moves();
        delivered
    }
}

// ---------------------------------------------------------------------------
// REPLICA 2: the drag rail (DragSession + DragCounters, field-for-field)
// ---------------------------------------------------------------------------

/// BENCH-SIDE replica of `DragPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RailPhase {
    #[default]
    Idle,
    Armed,
    InFlight,
    Settled,
    Cancelled,
}

/// BENCH-SIDE replica of `DragOrigin` (the immutable start context).
#[derive(Debug, Clone, PartialEq)]
struct RailOrigin {
    address: StableId,
    drag_meta: DragMeta,
    start_point: Point,
    start_tick: u64,
}

/// BENCH-SIDE replica of `MoveDisposition`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RailMove {
    Inactive,
    UnderThreshold { distance_px: f32 },
    Crossed,
    Parked { superseded: bool },
}

/// BENCH-SIDE replica of `DragRefusal` (the checked variants; Display is
/// the hosts' string style, unneeded for timing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailRefusal {
    ForeignSurface { session: StableId, addressed: StableId },
    DuplicateBegin(StableId),
    NotDraggable(StableId),
    NotInFlight,
}

/// BENCH-SIDE replica of `DragSettled` (the settle FACT: origin verbatim +
/// the drained newest end context + the total delivered count).
#[derive(Debug, Clone, PartialEq)]
struct RailSettled {
    address: StableId,
    drag_meta: DragMeta,
    start_point: Point,
    end_point: Point,
    moves_delivered: usize,
}

/// BENCH-SIDE replica of `DragCancelReason` — the CALLER's reason rides the
/// cancel fact verbatim (never the machine's invention; the machine has
/// exactly these four and the armed release routes ThresholdNotMet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RailCancelReason {
    #[default]
    Escape,
    PointerOut,
    Teardown,
    ThresholdNotMet,
}

/// BENCH-SIDE replica of `DragCancelled` (origin verbatim + end context +
/// the caller's reason — cancel converts to NOTHING semantically, so no
/// event vocabulary here).
#[derive(Debug, Clone, PartialEq)]
struct RailCancelled {
    address: StableId,
    drag_meta: DragMeta,
    start_point: Point,
    end_point: Point,
    reason: RailCancelReason,
}

/// BENCH-SIDE replica of `DragCounters` — every path that MATTERS moves
/// exactly its own counter, refused completions move none, and the storm
/// ends with the 1:1 identity (below, `storm_counters_agree`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RailCounters {
    begins: usize,
    moves_parked: usize,
    moves_delivered: usize,
    settles: usize,
    cancels: usize,
    previews_emitted: usize,
    refusals_duplicate_begin: usize,
    refusals_not_draggable: usize,
}

/// BENCH-SIDE REPLICA of `DragSession` (one per-surface rail; at most ONE
/// live drag; latest-wins park; drain-first completion). Mirrors the
/// machine transition-for-transition so the replica's gate shape IS the
/// contract's gate shape.
struct DragRailReplica {
    surface_id: StableId,
    phase: RailPhase,
    origin: Option<RailOrigin>,
    /// Latest-wins move buffer: at most one parked move. Drained by
    /// take-delivery AND by settle/cancel.
    owed_move: Option<Point>,
    /// The ≤1-in-flight delivery gate.
    moves_in_flight: bool,
    /// The last delivered point (the end-point authority when the drain
    /// finds nothing).
    last_delivered: Option<Point>,
    counters: RailCounters,
    /// The synced draggable set (the NotDraggable re-check's authority).
    draggable: HashSet<StableId>,
}

impl DragRailReplica {
    fn new(surface_id: StableId, draggable: HashSet<StableId>) -> Self {
        Self {
            surface_id,
            phase: RailPhase::Idle,
            origin: None,
            owed_move: None,
            moves_in_flight: false,
            last_delivered: None,
            counters: RailCounters::default(),
            draggable,
        }
    }

    fn phase(&self) -> RailPhase {
        self.phase
    }

    fn counters(&self) -> RailCounters {
        self.counters
    }

    /// `begin`: arm — capture the origin facts; a fresh arm never inherits
    /// a stale park/slot. Refusals move exactly their own counter.
    fn begin(
        &mut self,
        surface: StableId,
        address: StableId,
        drag_meta: DragMeta,
        start_point: Point,
        start_tick: u64,
    ) -> Result<RailOrigin, RailRefusal> {
        if surface != self.surface_id {
            return Err(RailRefusal::ForeignSurface {
                session: self.surface_id,
                addressed: surface,
            });
        }
        if matches!(self.phase, RailPhase::Armed | RailPhase::InFlight) {
            self.counters.refusals_duplicate_begin += 1;
            return Err(RailRefusal::DuplicateBegin(address));
        }
        if !self.draggable.contains(&address) {
            self.counters.refusals_not_draggable += 1;
            return Err(RailRefusal::NotDraggable(address));
        }
        let origin = RailOrigin {
            address,
            drag_meta,
            start_point,
            start_tick,
        };
        self.origin = Some(origin.clone());
        self.owed_move = None;
        self.moves_in_flight = false;
        self.last_delivered = None;
        self.phase = RailPhase::Armed;
        self.counters.begins += 1;
        Ok(origin)
    }

    /// `move_drag`: honor the arm threshold; else park the newest point
    /// (latest wins — a superseding move OVERWRITES the still-owed one).
    fn move_drag(&mut self, point: Point) -> RailMove {
        match self.phase {
            RailPhase::Armed => {
                let origin = match &self.origin {
                    Some(origin) => origin.clone(),
                    None => return RailMove::Inactive,
                };
                let distance_px = distance_px(origin.start_point, point);
                if distance_px < BENCH_DRAG_THRESHOLD_PX {
                    return RailMove::UnderThreshold { distance_px };
                }
                self.owed_move = Some(point);
                self.counters.moves_parked += 1;
                self.phase = RailPhase::InFlight;
                RailMove::Crossed
            }
            RailPhase::InFlight => {
                let superseded = self.owed_move.is_some();
                self.owed_move = Some(point);
                self.counters.moves_parked += 1;
                RailMove::Parked { superseded }
            }
            _ => RailMove::Inactive,
        }
    }

    /// `take_deliverable_move`: the ≤1-in-flight gate — `Some` only when a
    /// park holds a point AND no move is in flight.
    fn take_deliverable_move(&mut self) -> Option<Point> {
        if self.phase != RailPhase::InFlight || self.moves_in_flight {
            return None;
        }
        let point = self.owed_move.take()?;
        self.moves_in_flight = true;
        self.last_delivered = Some(point);
        self.counters.moves_delivered += 1;
        Some(point)
    }

    /// `release_delivered_move`: the delivery completed (release the slot).
    fn release_delivered_move(&mut self) -> bool {
        std::mem::replace(&mut self.moves_in_flight, false)
    }

    /// `settle`: LOSSLESS completion — DRAIN FIRST (the newest parked move
    /// becomes a delivery of its own; the drag's end context is the newest
    /// delivered point), close every live slot, exactly once.
    fn settle(&mut self, end_point: Point) -> Result<RailSettled, RailRefusal> {
        if self.phase != RailPhase::InFlight {
            return Err(RailRefusal::NotInFlight);
        }
        let Some(origin) = self.origin.take() else {
            self.phase = RailPhase::Idle;
            return Err(RailRefusal::NotInFlight);
        };
        let end_point = match self.owed_move.take() {
            Some(parked) => {
                self.last_delivered = Some(parked);
                self.counters.moves_delivered += 1;
                parked
            }
            None => self.last_delivered.take().unwrap_or(end_point),
        };
        let settled = RailSettled {
            address: origin.address,
            drag_meta: origin.drag_meta,
            start_point: origin.start_point,
            end_point,
            moves_delivered: self.counters.moves_delivered,
        };
        self.close_live_bookkeeping(RailPhase::Settled);
        self.counters.settles += 1;
        Ok(settled)
    }

    /// `cancel`: the lossless mirror — drain first, origin verbatim, no
    /// semantic exit, exactly once. `Armed` AND `InFlight` both cancel.
    fn cancel(&mut self, reason: RailCancelReason) -> Result<RailCancelled, RailRefusal> {
        if !matches!(self.phase, RailPhase::Armed | RailPhase::InFlight) {
            return Err(RailRefusal::NotInFlight);
        }
        let Some(origin) = self.origin.take() else {
            self.phase = RailPhase::Idle;
            return Err(RailRefusal::NotInFlight);
        };
        let end_point = match self.owed_move.take() {
            Some(parked) => {
                self.last_delivered = Some(parked);
                self.counters.moves_delivered += 1;
                parked
            }
            None => self.last_delivered.take().unwrap_or(origin.start_point),
        };
        let cancelled = RailCancelled {
            address: origin.address,
            drag_meta: origin.drag_meta,
            start_point: origin.start_point,
            end_point,
            reason,
        };
        self.close_live_bookkeeping(RailPhase::Cancelled);
        self.counters.cancels += 1;
        Ok(cancelled)
    }

    /// `close_live_bookkeeping`: every live slot drained at completion.
    fn close_live_bookkeeping(&mut self, phase: RailPhase) {
        self.owed_move = None;
        self.last_delivered = None;
        self.moves_in_flight = false;
        self.phase = phase;
    }
}

/// Euclidean pixel distance between two screen points (the replica's
/// mirror of `plugin_drag.rs`'s helper).
fn distance_px(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

// ---------------------------------------------------------------------------
// Counter helpers (the honest-byte scopes of the support harness)
// ---------------------------------------------------------------------------

/// Replica counter (measured on the bench-side structure; carries the
/// replica disclaimer).
fn replica_counter(name: &str, value: u64, machine: &str) -> Counter {
    Counter {
        name: name.to_owned(),
        scope: "bench-emulated",
        value: Some(value),
        note: Some(format!(
            "replica counter of the bench-side {machine} (pub(crate) host machines are bench-invisible)"
        )),
    }
}

/// Deferred counter (the authoritative pub(crate) half; test evidence).
fn deferred_counter(name: &str, note: String) -> Counter {
    Counter {
        name: name.to_owned(),
        scope: "deferred",
        value: None,
        note: Some(note),
    }
}

/// The mount-coalescer scenario's surface honesty label.
fn mount_surface_note(machine: &str) -> String {
    format!(
        "BENCH-SIDE REPLICA of the pub(crate) {machine} contract \
         (src/ui/window/plugin_widget_mount.rs), field-for-field; anchored to \
         pub ocs_plugin_api (WidgetSurface/StableId/SceneRevision). The \
         pub(crate) registry machines are bench-invisible — the wall-clock \
         numbers measure the REPLICA, the authoritative values are the \
         in-source unit tests + registry counters (deferred counters below)"
    )
}

/// The drag-rail scenario's surface honesty label.
fn drag_surface_note(machine: &str) -> String {
    format!(
        "BENCH-SIDE REPLICA of the pub(crate) {machine} contract \
         (src/ui/window/plugin_drag.rs), field-for-field; anchored to pub \
         ocs_plugin_api (StableId/DragMeta + iced::Point). The pub(crate) \
         DragSession/DragCounters are bench-invisible — the wall-clock \
         numbers measure the REPLICA, the authoritative values are the \
         in-source unit tests (deferred counters below)"
    )
}

// ---------------------------------------------------------------------------
// Scenario 1: park_storm_10k
// ---------------------------------------------------------------------------

/// The storm outcome the suffix must assert (assert-in-print per sample).
struct ParkStormOutcome {
    bytes_before: usize,
    bytes_after: usize,
    slot_holds: Option<Point>,
    expected: Point,
    superseded: u64,
}

fn scenario_park_storm() -> ScenarioRecord {
    let scenario = "park_storm_10k";
    println!(
        "\n=== scenario: {scenario} — {STORM_MOVES} moves into ONE park slot (REPLICA of \
         the mount coalescer entry) ==="
    );
    let samples = timed_samples(
        scenario,
        SAMPLES_STORM,
        WARMUP,
        || {
            let coalescer = MountCoalescerReplica::default();
            let bytes_before = mem::size_of_val(&coalescer.move_owed);
            (coalescer, bytes_before)
        },
        |(mut coalescer, bytes_before)| {
            for i in 0..STORM_MOVES {
                black_box(coalescer.mount_move_defer(storm_point(i)));
            }
            ParkStormOutcome {
                bytes_before,
                bytes_after: mem::size_of_val(&coalescer.move_owed),
                slot_holds: coalescer.move_owed,
                expected: storm_point(STORM_MOVES - 1),
                superseded: coalescer.coalesced_moves_superseded,
            }
        },
        |outcome| {
            // The assert-in-print gate (per sample): the slot never grows,
            // and it holds EXACTLY the LAST position of the storm.
            assert_eq!(
                outcome.bytes_before, outcome.bytes_after,
                "the park slot's BYTE FOOTPRINT cannot grow under the storm"
            );
            assert_eq!(
                outcome.slot_holds,
                Some(outcome.expected),
                "latest-wins: the slot must hold EXACTLY the storm's LAST position"
            );
            assert_eq!(
                outcome.superseded,
                (STORM_MOVES - 1) as u64,
                "every park after the first must supersede (latest wins)"
            );
            format!(
                "| slot bytes {} (constant, never grew) | held == storm-last ok=true | \
                 superseded {superseded}/{moves} ok={ok}",
                outcome.bytes_before,
                superseded = outcome.superseded,
                moves = STORM_MOVES,
                ok = outcome.superseded == (STORM_MOVES - 1) as u64
            )
        },
    );
    let wall = WallPercentiles::from_samples(&samples);
    println!(
        "[park_storm_10k] storm pct: p50 {:.0} ns | p95 {:.0} ns | p99 {:.0} ns | min {:.0} | max {:.0} \
         ({} batches × {} moves, per-move mean ≈ {:.3} ns)",
        wall.p50_ns,
        wall.p95_ns,
        wall.p99_ns,
        wall.min_ns,
        wall.max_ns,
        SAMPLES_STORM,
        STORM_MOVES,
        wall.p50_ns / STORM_MOVES as f64
    );

    // Anti-pattern contrast: the same storm into a Vec page-queue — the
    // shape the viewport coalescer measured as the 986-positions defect.
    // Bytes held by the queue GROW with the storm; the slot's never move.
    let mut queue_bytes = 0usize;
    let mut queue_len = 0usize;
    let queue_wall = timed_ns(SAMPLES_ANTI, 2, || {
        let mut queue: Vec<Point> = Vec::new();
        for i in 0..STORM_MOVES {
            queue.push(storm_point(i));
        }
        black_box(&queue);
        queue_bytes = queue.capacity() * mem::size_of::<Point>();
        queue_len = queue.len();
        drop(queue);
    });
    let queue_wall = WallPercentiles::from_samples(&queue_wall);
    println!(
        "[park_storm_10k] ANTI-PATTERN Vec page-queue (bench-emulated): p50 {:.0} ns; holds \
         {queue_len} positions = {} B len / {queue_bytes} B capacity held — GROWS with the storm; \
         the slot's {} B never do",
        queue_wall.p50_ns,
        queue_len * mem::size_of::<Point>(),
        mem::size_of_val(&Option::<Point>::None)
    );

    // Reset stability: storm → flush → storm → flush — the second storm on
    // the RESET slot must behave EXACTLY like the first (delivered == the
    // newest; no haunted state, no accumulation across storms). Per-park
    // black_box (the same barrier discipline as the storm scenario) keeps
    // the park writes from folding away.
    let mut stability_note = String::new();
    let stability_wall = timed_ns(SAMPLES_STABILITY, 2, || {
        let mut coalescer = MountCoalescerReplica::default();
        for i in 0..STORM_MOVES {
            black_box(coalescer.mount_move_defer(storm_point(i)));
        }
        assert_eq!(
            coalescer.mount_flush_moves(),
            1,
            "the first flush delivers exactly one"
        );
        assert!(coalescer.last_move == Some(storm_point(STORM_MOVES - 1)));
        assert!(coalescer.move_owed.is_none(), "flush drained the slot");
        // The slot is now the RESET slot; a second full storm must be
        // byte-identical in behavior to the first.
        for i in 0..STORM_MOVES {
            black_box(coalescer.mount_move_defer(storm_point(i)));
        }
        assert_eq!(coalescer.mount_flush_moves(), 1);
        assert_eq!(
            coalescer.last_move,
            Some(storm_point(STORM_MOVES - 1)),
            "after reset the newest STILL defines the delivery (reset stability)"
        );
        black_box(&coalescer);
    });
    let stability_wall = WallPercentiles::from_samples(&stability_wall);
    stability_note
        .push_str(&format!("p50 {:.0} ns (storm+flush+storm+flush cycle)", stability_wall.p50_ns));

    // The slot's footprint: the compile-time truth the storm asserted.
    let slot_bytes = mem::size_of::<Option<Point>>() as u64;

    ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: mount_surface_note("MoveInputState/HoverState owed-move slot + mount_move_defer"),
        deferred: vec![
            "wall-clock on the pub(crate) MountRegistry path (mount_move_defer through \
             the real per-surface HashMap<StableId, HoverState> store) — registry machines \
             and MountCounters are bench-invisible from a bench target; authoritative: \
             n_moves_coalesce_to_one_mount_call, superseded_move_overwrites_owed_slot, \
             unknown_surface_moves_are_graceful (src/ui/window/plugin_widget_mount.rs tests)"
                .to_owned(),
            "registry counters mount_move_calls / coalesced_moves_superseded at burst load \
             (see the deferred counters)"
                .to_owned(),
        ],
        params: vec![
            param("moves", STORM_MOVES),
            param("park_slot_type", "Option<Point> (single slot, no queue)"),
            param("samples", SAMPLES_STORM),
            param("expected_superseded", (STORM_MOVES - 1) as u64),
            param("expected_flush_deliveries", 1_u64),
        ],
        samples_n: SAMPLES_STORM,
        wall_ns: wall,
        extra_metrics: vec![
            ExtraMetric {
                label: "vector_page_queue_antipattern".to_owned(),
                samples_n: SAMPLES_ANTI,
                wall_ns: queue_wall,
                payload_bytes: Some(queue_bytes as u64),
                note: Some(format!(
                    "bench-emulated ANTI-PATTERN (the viewport coalescer's 986-positions \
                     page-queue defect — measured as the growth the owed slot must never \
                     have): bytes the queue HELD after the {STORM_MOVES}-move storm \
                     (capacity; len {} B) — grows with the storm; the slot's {slot_bytes} B \
                     never do",
                    queue_len * mem::size_of::<Point>()
                )),
            },
            ExtraMetric {
                label: "slot_reset_stability".to_owned(),
                samples_n: SAMPLES_STABILITY,
                wall_ns: {
                    // from_samples on the ALREADY reduced wall would re-sort zeros;
                    // stability's own percentiles live in stability_note, this
                    // struct carries the cycle's distribution shape only.
                    WallPercentiles {
                        n: SAMPLES_STABILITY,
                        p50_ns: stability_wall.p50_ns,
                        p95_ns: stability_wall.p95_ns,
                        p99_ns: stability_wall.p99_ns,
                        min_ns: stability_wall.min_ns,
                        max_ns: stability_wall.max_ns,
                    }
                },
                payload_bytes: None,
                note: Some(format!(
                    "storm → flush → storm → flush on the RESET slot: both flushes deliver \
                     exactly the newest (asserted per cycle) — {stability_note}"
                )),
            },
        ],
        payload_bytes: Some(slot_bytes),
        payload_bytes_note: Some(format!(
            "the park slot's bytes ({slot_bytes} = Option<Point>): CONSTANT across the whole \
             {STORM_MOVES}-move storm (asserted per sample: bytes_before == bytes_after); \
             the slot never grows, never queues — the growth lives ONLY in the anti-pattern metric"
        )),
        counters: vec![
            replica_counter(
                "moves_parked_replica",
                STORM_MOVES as u64,
                "MountCoalescerReplica::mount_move_defer parks",
            ),
            replica_counter(
                "coalesced_moves_superseded_replica",
                (STORM_MOVES - 1) as u64,
                "MountCounters::coalesced_moves_superseded shape",
            ),
            replica_counter(
                "mount_move_calls_replica",
                0,
                "MountCounters::mount_move_calls shape (parks only: no flush in this scenario)",
            ),
            deferred_counter(
                "mount_move_calls",
                "pub(crate) MountCounters field (registry); unit-test evidence: \
                 mount_move_calls_never_exceed_one_per_tick, n_moves_coalesce_to_one_mount_call \
                 (src/ui/window/plugin_widget_mount.rs); wall-clock at storm load waits for a \
                 pub seam (SCN-012 precedent: the pub(crate) halves stayed deferred)"
                    .to_owned(),
            ),
            deferred_counter(
                "coalesced_moves_superseded",
                "pub(crate) MountCounters field (registry); unit-test evidence: \
                 superseded_move_overwrites_owed_slot (src/ui/window/plugin_widget_mount.rs); \
                 the replica's overwrite arithmetic (STORM-1 of STORM parks) calibrates the \
                 contract the registry test pins"
                    .to_owned(),
            ),
        ],
        changed_elements: None,
        total_elements: None,
        changed_less_than_total: None,
        sink: format!(
            "every sample asserted: slot bytes constant ({slot_bytes} before == after), slot \
             holds EXACTLY the storm's LAST position, superseded == {STORM_MOVES}-1 — a \
             defective storm (a queue, a dropped last move, a grown slot) panics the bench"
        ),
        notes: vec![
            format!(
                "REPLICA bench (honest-byte rule): measures the CONTRACT, not the \
                 pub(crate) registry; the contract's real-path evidence is the host's \
                 own tests (see deferred counters)"
            ),
            "storm positions are dyadic floats — the newest-position equality is EXACT"
                .to_owned(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Scenario 2: drain_delivers_latest_only
// ---------------------------------------------------------------------------

/// One (storm → single tick flush) sample's verified outcome.
struct DrainOutcome {
    delivered: usize,
    last_move: Option<Point>,
    expected_owed: Option<Point>,
    expected_last: Point,
    storm_move_calls: u64,
}

fn scenario_drain_latest_only() -> ScenarioRecord {
    let scenario = "drain_delivers_latest_only";
    println!(
        "\n=== scenario: {scenario} — {STORM_MOVES} parks, ONE tick flush, the LAST position \
         defines (REPLICA of the mount coalescer flush) ==="
    );
    let samples = timed_samples(
        scenario,
        SAMPLES_DELIVER,
        WARMUP,
        || MountCoalescerReplica::default(),
        |mut coalescer| {
            // Per-park black_box — the SAME barrier discipline as
            // park_storm_10k (each park's accept-bool is an observable the
            // compiler must keep; the storm wall is real per-park work,
            // and the 10k parks cannot fold into one final write).
            for i in 0..STORM_MOVES {
                black_box(coalescer.mount_move_defer(storm_point(i)));
            }
            let expected_last = storm_point(STORM_MOVES - 1);
            let expected_owed = coalescer.move_owed;
            let delivered = coalescer.mount_flush_moves();
            DrainOutcome {
                delivered,
                last_move: coalescer.last_move,
                expected_owed: expected_owed.filter(|p| p == &expected_last),
                expected_last,
                storm_move_calls: coalescer.mount_move_calls,
            }
        },
        |outcome| {
            assert_eq!(
                outcome.expected_owed,
                Some(outcome.expected_last),
                "the slot held exactly the newest before the flush"
            );
            assert_eq!(
                outcome.delivered, 1,
                "{STORM_MOVES} parks → exactly ONE delivery per surface per flush"
            );
            assert_eq!(
                outcome.last_move,
                Some(outcome.expected_last),
                "lossless: the flush delivered the storm's LAST position (latest-wins)"
            );
            format!(
                "| delivered {delivered}/flush | last == storm-last ok=true | move_calls = \
                 {calls} (≤1/surface/tick ok={ok})",
                delivered = outcome.delivered,
                calls = outcome.storm_move_calls,
                ok = outcome.delivered <= 1
            )
        },
    );
    let wall = WallPercentiles::from_samples(&samples);
    println!(
        "[drain_delivers_latest_only] storm+flush pct: p50 {:.0} ns | p95 {:.0} ns | p99 {:.0} ns \
         | min {:.0} | max {:.0} (per-tick delivered count == 1, asserted per sample)",
        wall.p50_ns, wall.p95_ns, wall.p99_ns, wall.min_ns, wall.max_ns
    );

    // Percentiles over 1k tick-flushes: each flush follows a fresh park
    // (the tick's normal shape); every flush delivers exactly one = the
    // newest (asserted inside the loop).
    let flush_wall = timed_ns(SAMPLES_FLUSH_TAKE, 2, || {
        let mut coalescer = MountCoalescerReplica::default();
        coalescer.mount_move_defer(storm_point(7));
        let delivered = coalescer.mount_flush_moves();
        assert_eq!(delivered, 1, "every tick-flush delivers exactly one");
        assert_eq!(coalescer.last_move, Some(storm_point(7)));
        black_box(&coalescer);
    });
    let flush_wall = WallPercentiles::from_samples(&flush_wall);
    println!(
        "[drain_delivers_latest_only] tick-flush (1 park → flush) pct over {} flushes: \
         p50 {:.0} ns | p95 {:.0} ns | p99 {:.0} ns (delivered == 1 asserted per flush)",
        SAMPLES_FLUSH_TAKE, flush_wall.p50_ns, flush_wall.p95_ns, flush_wall.p99_ns
    );

    // The release completion: capture's release answers the last owed move
    // NOW (the storm's last position must not outlive the gesture).
    let release_wall = timed_ns(SAMPLES_RELEASE, 2, || {
        let mut coalescer = MountCoalescerReplica {
            captured: true,
            ..MountCoalescerReplica::default()
        };
        coalescer.mount_move_defer(storm_point(11));
        let delivered = coalescer.mount_pointer_released();
        assert_eq!(
            delivered, 1,
            "the release completion delivers the owed move (≤1, lossless)"
        );
        assert_eq!(coalescer.last_move, Some(storm_point(11)));
        assert!(!coalescer.captured);
        black_box(&coalescer);
    });
    let release_wall = WallPercentiles::from_samples(&release_wall);
    println!(
        "[drain_delivers_latest_only] release completion pct over {}: p50 {:.0} ns | \
         p99 {:.0} ns (delivered == 1 asserted per release)",
        SAMPLES_RELEASE, release_wall.p50_ns, release_wall.p99_ns
    );

    ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: mount_surface_note("mount_flush_moves one-delivery-per-tick + release completion"),
        deferred: vec![
            "wall-clock on the pub(crate) MountRegistry::mount_flush_moves registry path — \
             bench-invisible; authoritative: n_moves_coalesce_to_one_mount_call, \
             mount_move_calls_never_exceed_one_per_tick, capture_completes_on_release_outside \
             (src/ui/window/plugin_widget_mount.rs tests)"
                .to_owned(),
            "the registry's mount_move_calls evolution under load (deferred counter)"
                .to_owned(),
        ],
        params: vec![
            param("parks_per_sample", STORM_MOVES),
            param("flushes_per_sample", 1_u64),
            param("samples", SAMPLES_DELIVER),
            param("tick_flush_percentile_population", SAMPLES_FLUSH_TAKE),
            param("release_completion_population", SAMPLES_RELEASE),
            param("expected_delivered_per_flush", 1_u64),
        ],
        samples_n: SAMPLES_DELIVER,
        wall_ns: wall,
        extra_metrics: vec![
            ExtraMetric {
                label: "tick_flush_1k".to_owned(),
                samples_n: SAMPLES_FLUSH_TAKE,
                wall_ns: flush_wall,
                payload_bytes: None,
                note: Some(
                    "percentiles over 1 000 tick-flushes (each: fresh park → flush; \
                     delivered == 1 and last == newest asserted per flush)"
                        .to_owned(),
                ),
            },
            ExtraMetric {
                label: "release_completion".to_owned(),
                samples_n: SAMPLES_RELEASE,
                wall_ns: release_wall,
                payload_bytes: None,
                note: Some(
                    "the capture release's answer-the-owed-move path (each: park under \
                     capture → release; delivered == 1 asserted per release)"
                        .to_owned(),
                ),
            },
        ],
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![
            replica_counter(
                "parks_replica",
                STORM_MOVES as u64,
                "MountCoalescerReplica::mount_move_defer parks",
            ),
            replica_counter(
                "superseded_replica",
                (STORM_MOVES - 1) as u64,
                "latest-wins overwrites inside the storm",
            ),
            replica_counter(
                "delivered_replica",
                1,
                "MountCoalescerReplica::mount_flush_moves deliveries (ONE per storm)",
            ),
            deferred_counter(
                "mount_move_calls",
                "pub(crate) MountCounters field (registry); unit-test evidence: \
                 mount_move_calls_never_exceed_one_per_tick, n_moves_coalesce_to_one_mount_call \
                 (src/ui/window/plugin_widget_mount.rs); the replica's 1-delivery-per-flush \
                 arithmetic calibrates that contract"
                    .to_owned(),
            ),
        ],
        changed_elements: None,
        total_elements: None,
        changed_less_than_total: None,
        sink: format!(
            "every sample asserted: {STORM_MOVES} parks → ONE flush delivery == the storm's \
             LAST position; every one of the {SAMPLES_FLUSH_TAKE} tick-flushes delivered \
             exactly one newest — a lost last move, a >1-in-flight flush, or a non-newest \
             delivery panics the bench"
        ),
        notes: vec![
            "REPLICA bench: the flush slot is structurally ONE Option — ≤1 delivery per \
             surface per tick cannot be violated by construction; the assertion checks the \
             newer-won position IS the newest"
                .to_owned(),
            "latest-wins = the last accepted position is the ONLY one the delivery answers; \
             the superseded ones are counted (park churn), never queued, never delivered"
                .to_owned(),
            "storm discipline: every park's accept-bool passes through black_box (the same \
             barrier as park_storm_10k) so the storm wall measures real per-park work and \
             the 10k writes cannot fold into one final store"
                .to_owned(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Scenario 3: settle_cancel_storm (the drag rail replica)
// ---------------------------------------------------------------------------

/// The storm's counter evolution snapshot (printed at milestones).
#[derive(Debug, Clone, Copy, Default)]
struct StormProgress {
    /// Machine-side settles/cancels (the interaction counters as the rail
    /// reported them).
    begins: usize,
    settles: usize,
    cancels: usize,
    /// Machine-side parks/deliveries snapshot from the rail's counters.
    moves_parked: usize,
    moves_delivered: usize,
    /// Refusal probes still live (the machine-side counts; asserted equal
    /// to the probe counters below).
    refusals_duplicate: usize,
    refusals_not_draggable: usize,
    /// LOOP-side offered counts (what the storm DROVE) — asserted equal to
    /// the machine counters at the end; a dual-side accounting defect can
    /// never hide between them.
    offered_parks: usize,
    offered_deliveries: usize,
    dup_probes: usize,
    not_draggable_probes: usize,
    /// Per-interaction checks performed (every interaction of the storm).
    newest_checked: usize,
    slot_drained_checked: usize,
}

/// The full storm: `INTERACTIONS` through ONE rail — `SETTLE_PER_STORM`
/// begin/settle pairs + `CANCELS_PER_STORM` cancels, interleaved; every
/// interaction parks at least one move; every completion DRAINS FIRST and
/// is checked for (a) the end == the interaction's newest accepted point,
/// (b) zero leftovers in the park slot, (c) exactly-once. `print_evolution`
/// prints the counter scroll at milestones (the untimed calibration pass).
fn drag_storm(print_evolution: bool) -> StormProgress {
    let surface = anchored_surface();
    let draggable = anchor_addresses(&surface);
    let rail_key = StableId(PANEL_SURFACE_ID);
    let mut rail = DragRailReplica::new(rail_key, draggable);
    let mut progress = StormProgress::default();
    let storm_start = std::time::Instant::now();

    // THE NOT-DRAGGABLE PROBE (once per storm, before the interaction
    // loop): an address the synced set does NOT contain refuses arm, counts
    // ONLY `refusals_not_draggable`, and the rail stays Idle for the storm.
    let probe_address = StableId(TREE_ROW_BASE + 999_999);
    match rail.begin(
        rail_key,
        probe_address,
        meta("probe"),
        Point::new(10.0, 10.0),
        0,
    ) {
        Err(RailRefusal::NotDraggable(addr)) => {
            assert_eq!(addr, probe_address, "the refusal names the probed address");
            progress.not_draggable_probes += 1;
        }
        Ok(_) => panic!("the probe address must refuse (NotDraggable)"),
        Err(other) => panic!("the probe address refused as {other:?}, not NotDraggable"),
    }

    for k in 0..INTERACTIONS {
        let cancelled_interaction = k % 5 == 4;
        // The interaction's address (rotating through the anchored rows —
        // the SYNCED def's addresses; unknown addresses refuse at arm).
        let address = StableId(TREE_ROW_BASE + (k % ANCHOR_ROWS) as u64);
        let label = format!("row-{}", k % ANCHOR_ROWS);
        let start = Point::new(10.0, 10.0);

        let began = rail
            .begin(rail_key, address, meta(&label), start, k as u64)
            .expect("arm (the rail was consumed by the previous interaction)");
        progress.begins += 1;

        // One duplicate-begin refusal probe every 250th interaction (the
        // exactly-once guard's live side): the SECOND begin while Armed
        // refuses, moves ONLY its own counter, and cannot retarget the
        // armed origin.
        if k % 250 == 7 {
            let before = rail.counters();
            assert_eq!(
                rail.begin(rail_key, address, meta("ignored"), start, k as u64),
                Err(RailRefusal::DuplicateBegin(address)),
                "the armed session owns the pointer"
            );
            progress.dup_probes += 1;
            assert_eq!(
                rail.counters(),
                RailCounters {
                    refusals_duplicate_begin: before.refusals_duplicate_begin + 1,
                    ..before
                },
                "the DuplicateBegin refusal moves ONLY its own counter"
            );
        }

        if cancelled_interaction {
            // CANCEL storm route: cross + PARKS_CANCEL churn parks, then
            // cancel — the drain-first rule hands the NEWEST point back.
            let crossed = Point::new(10.0 + BENCH_DRAG_THRESHOLD_PX, 10.0);
            assert_matches_crossed(rail.move_drag(crossed));
            let mut newest = crossed;
            progress.offered_parks += 1;
            for j in 1..=PARKS_CANCEL {
                let p = Point::new(13.0 + (j * 8) as f32, 10.0 + j as f32 * 3.0);
                // Both parks supersede: the cancel route takes NO per-tick
                // deliveries, so the slot is never empty in between.
                assert_eq!(
                    rail.move_drag(p),
                    RailMove::Parked { superseded: true },
                    "in-flight cancel-route park overwrites the still-owed one"
                );
                newest = p;
                progress.offered_parks += 1;
            }
            let cancelled = rail
                .cancel(RailCancelReason::Escape)
                .expect("the live drag cancels 1:1 with its begin");
            assert_eq!(
                cancelled.end_point, newest,
                "cancel DRAINED FIRST: the newest move defines the end (lossless)"
            );
            assert_eq!(cancelled.address, began.address, "address verbatim");
            assert_eq!(cancelled.drag_meta, began.drag_meta, "meta verbatim");
            assert_eq!(cancelled.start_point, began.start_point);
            assert_eq!(
                cancelled.reason, RailCancelReason::Escape,
                "the caller's reason rides the cancel fact verbatim"
            );
            assert!(rail.owed_move.is_none(), "no move survives the boundary");
            assert!(!rail.moves_in_flight, "the gate released");
            progress.offered_deliveries += 1; // the cancel's own drain
            progress.cancels += 1;
        } else {
            // SETTLE storm route: alternate per-tick Drip and pure-CHURN
            // shapes; the settle completes exactly-once with the newest
            // end context either way.
            let drip = k % 2 == 0;
            let crossed = Point::new(10.0 + BENCH_DRAG_THRESHOLD_PX, 10.0);
            assert_matches_crossed(rail.move_drag(crossed));
            let mut newest = crossed;
            progress.offered_parks += 1;
            for j in 1..=PARKS_DRIP {
                let p = Point::new(13.0 + (j * 8) as f32, 10.0 + j as f32 * 3.0);
                // Drip shape: the slot was DRAINED by the previous tick's
                // take (j = 1 alone supersedes the still-owed crossing).
                // Churn shape: every park supersedes (no ticks at all).
                assert_eq!(
                    rail.move_drag(p),
                    RailMove::Parked { superseded: !drip || j == 1 },
                    "in-flight parks overwrite the still-owed one (latest wins)"
                );
                newest = p;
                progress.offered_parks += 1;
                if drip {
                    // The per-tick shape: deliver the JUST-parked newest,
                    // release the gate (at most one in flight — the release
                    // proves the gate let it go synchronously).
                    let taken = rail.take_deliverable_move().expect("the ≤1-in-flight gate");
                    assert_eq!(taken, p, "the tick delivers the newest parked move");
                    assert!(
                        rail.take_deliverable_move().is_none(),
                        "NOTHING delivers while one is in flight (≤1 gate)"
                    );
                    assert!(rail.release_delivered_move());
                    progress.offered_deliveries += 1;
                }
            }
            // The release argument is a DELIBERATE LIE ((999, -999)) — the
            // drain-first rule must make the end context ignore it entirely
            // (the lie proves the rule).
            let settled = rail
                .settle(Point::new(999.0, -999.0))
                .expect("the live drag settles 1:1 with its begin");
            assert_eq!(
                settled.end_point, newest,
                "settle DRAINED FIRST / last-delivered: the NEWEST move defines the end, \
                 never the (lying) release argument — lossless latest-wins"
            );
            assert_eq!(settled.address, began.address, "address verbatim");
            assert_eq!(settled.drag_meta, began.drag_meta, "meta VERBATIM");
            assert_eq!(settled.start_point, began.start_point);
            assert!(rail.owed_move.is_none(), "no move survives the boundary");
            assert!(!rail.moves_in_flight);
            if !drip {
                // The churn settle's DRAIN: the newest churn park became a
                // delivery of its own at the boundary (lossless — offered).
                progress.offered_deliveries += 1;
            }
            progress.settles += 1;
        }
        progress.newest_checked += 1;
        progress.slot_drained_checked += 1;

        // Every 250th interaction: the double-completion refusal probe —
        // both completion paths refuse, and NEITHER moves a counter.
        if k % 250 == 49 {
            let before = rail.counters();
            assert_eq!(
                rail.settle(Point::new(0.0, 0.0)),
                Err(RailRefusal::NotInFlight),
                "exactly-once: the consumed rail refuses a second settle"
            );
            assert_eq!(
                rail.cancel(RailCancelReason::Escape),
                Err(RailRefusal::NotInFlight),
                "exactly-once: the consumed rail refuses a second cancel"
            );
            assert_eq!(
                rail.counters(), before,
                "refused completions move NO counter (the exactly-once evidence lives \
                 only in the settles/cancels counts)"
            );
        }

        if print_evolution && (k + 1) % 250 == 0 {
            let machine = rail.counters();
            println!(
                "[settle_cancel_storm] evolution @ interaction {:>5}/{total}: begins {begins} \
                 settles {settles} cancels {cancels} | parked {parked} delivered {delivered} | \
                 refusals: dup-start {dup} not-draggable {nd} | checked: end==newest \
                 {newest}/{INT}, slots drained {drained}/{INT}",
                k + 1,
                total = INTERACTIONS,
                begins = machine.begins,
                settles = machine.settles,
                cancels = machine.cancels,
                parked = machine.moves_parked,
                delivered = machine.moves_delivered,
                dup = progress.dup_probes,
                nd = progress.not_draggable_probes,
                newest = progress.newest_checked,
                INT = INTERACTIONS,
                drained = progress.slot_drained_checked,
            );
        }
    }

    // The dual-side SYNC: the machine's counters must equal the loop-side
    // OFFERED history EXACTLY (the machine accepted exactly the storm it
    // was offered — no extra parks, no dropped deliveries).
    let machine = rail.counters();
    progress.moves_parked = machine.moves_parked;
    progress.moves_delivered = machine.moves_delivered;
    assert_eq!(machine.begins, progress.begins, "begins: machine == offered");
    assert_eq!(
        machine.settles, progress.settles,
        "settles: machine == offered"
    );
    assert_eq!(
        machine.cancels, progress.cancels,
        "cancels: machine == offered"
    );
    assert_eq!(
        machine.moves_parked, progress.offered_parks,
        "parks: machine == offered (every park accepted, no more)"
    );
    assert_eq!(
        machine.moves_delivered, progress.offered_deliveries,
        "deliveries: machine == offered (every take and every drain counted once)"
    );
    assert_eq!(
        machine.refusals_duplicate_begin, progress.dup_probes,
        "DuplicateBegin refusals == the probes"
    );
    assert_eq!(
        machine.refusals_not_draggable, progress.not_draggable_probes,
        "NotDraggable refusals == the probes"
    );
    progress.refusals_duplicate = progress.dup_probes;
    progress.refusals_not_draggable = progress.not_draggable_probes;
    // The lossless 1:1 storm identity (deterministic expectations).
    assert_eq!(
        progress.begins,
        SETTLE_PER_STORM + CANCELS_PER_STORM,
        "1:1 begins == settles + cancels (every begun interaction completed)"
    );
    assert_eq!(
        progress.settles,
        SETTLE_PER_STORM,
        "every settle route completed its begin"
    );
    assert_eq!(
        progress.cancels,
        CANCELS_PER_STORM,
        "every cancel route completed its begin"
    );
    assert!(
        machine.previews_emitted == 0,
        "the interaction storm emits no previews (the ghost path is not this \
         bench's load; INT-3: previews would change only their own counter)"
    );
    if print_evolution {
        println!(
            "[settle_cancel_storm] storm wall (calibration pass): {} ns | counters agree \
             (1:1 begins==settles+cancels) | park slot drained on every completion",
            storm_start.elapsed().as_nanos()
        );
    }
    black_box(&rail);
    progress
}

/// Storm assertion helper: the crossing move's disposition mirror.
fn assert_matches_crossed(actual: RailMove) {
    assert_eq!(actual, RailMove::Crossed, "the crossing move must go live");
}

fn scenario_settle_cancel_storm() -> ScenarioRecord {
    let scenario = "settle_cancel_storm";
    println!(
        "\n=== scenario: {scenario} — {SETTLE_PER_STORM} begin/settle pairs + \
         {CANCELS_PER_STORM} cancels through ONE rail (REPLICA of DragSession) ==="
    );

    // The CALIBRATION pass (untimed): full storm with the counter-evolution
    // printouts + every lossless assertion live. A single defect here
    // panics the bench run.
    let calibrated = drag_storm(true);

    // The TIMED storms (assertions still live — a defect aborts the run).
    let samples = timed_ns(
        SAMPLES_STORM_INTERACT,
        1,
        || {
            let p = drag_storm(false);
            black_box(&p);
        },
    );
    let wall = WallPercentiles::from_samples(&samples);
    println!(
        "[settle_cancel_storm] storm pct ({SAMPLES_STORM_INTERACT} storms × \
         {INTERACTIONS} interactions): p50 {:.0} ns | p95 {:.0} ns | p99 {:.0} ns | min {:.0} \
         | max {:.0} (≈ {:.0} ns/interaction)",
        wall.p50_ns,
        wall.p95_ns,
        wall.p99_ns,
        wall.min_ns,
        wall.max_ns,
        wall.p50_ns / INTERACTIONS as f64
    );

    // Re-arm cost: the armed→in-flight→settled→armed cycle on ONE
    // persistent rail (true re-arm: each begin runs on the consumed rail
    // the previous cycle produced) vs a FRESH rail per cycle.
    let rail_key = StableId(PANEL_SURFACE_ID);
    let rearm_rail = std::cell::RefCell::new(DragRailReplica::new(
        rail_key,
        anchor_addresses(&anchored_surface()),
    ));
    let rearm_wall = timed_ns(SAMPLES_REARM, 2, || {
        let mut rail = rearm_rail.borrow_mut();
        let origin = rail
            .begin(
                rail_key,
                StableId(TREE_ROW_BASE),
                meta("rearm-cycle"),
                Point::new(5.0, 5.0),
                0,
            )
            .expect("re-arm on the consumed rail");
        // armed → in-flight (the crossing move) → settled:
        assert_matches_crossed(rail.move_drag(storm_point(3)));
        let taken = rail.take_deliverable_move().expect("deliver the crossing");
        assert_eq!(taken, storm_point(3));
        assert!(rail.release_delivered_move());
        let settled = rail
            .settle(Point::new(-1.0, -1.0))
            .expect("the cycle settles");
        assert_eq!(
            settled.end_point, storm_point(3),
            "the cycle's end = the delivered newest"
        );
        assert_eq!(settled.drag_meta, origin.drag_meta);
    });
    let rearm_wall = WallPercentiles::from_samples(&rearm_wall);

    let fresh_wall = timed_ns(SAMPLES_REARM, 2, || {
        let draggable = anchor_addresses(&anchored_surface());
        let mut rail = DragRailReplica::new(rail_key, draggable);
        rail.begin(
            rail_key,
            StableId(TREE_ROW_BASE + 1),
            meta("fresh-cycle"),
            Point::new(5.0, 5.0),
            0,
        )
        .expect("arm the fresh rail");
        assert_matches_crossed(rail.move_drag(storm_point(4)));
        let settled = rail.settle(Point::new(-1.0, -1.0)).expect("settle");
        assert_eq!(settled.end_point, storm_point(4));
        black_box(&rail);
    });
    let fresh_wall = WallPercentiles::from_samples(&fresh_wall);
    println!(
        "[settle_cancel_storm] re-arm cost (armed→in-flight→settled cycle): re-armed rail \
         p50 {:.0} ns | fresh rail p50 {:.0} ns ({SAMPLES_REARM} cycles each; the consumed \
         family re-arms by the machine's own design — Idle/Settled/Cancelled accept begin)",
        rearm_wall.p50_ns, fresh_wall.p50_ns
    );

    // The deterministic storm shape (asserted == the calibrated pass, so
    // the printed numbers ARE the semantics): the parks/deliveries the
    // storm OFFERS and the machine accepted, dual-side synced inside
    // `drag_storm` (the 1:1 counter identity lives there).
    let expected_parked: u64 = calibrated.offered_parks as u64;
    let expected_delivered: u64 = calibrated.offered_deliveries as u64;

    ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: drag_surface_note("DragSession begin/move_drag/take_deliverable_move/ \
                                    release_delivered_move/settle/cancel + DragCounters"),
        deferred: vec![
            "wall-clock on the pub(crate) DragSession/DragCounters machines — bench-invisible; \
             authoritative: begin_move_settle_is_lossless, \
             cancel_paths_are_one_to_one_with_begins, rapid_moves_latest_wins_never_drops_the_last, \
             duplicate_begin_refused_in_flight, threshold_not_met_release_is_not_a_drag, \
             counters_are_the_defect_probe (src/ui/window/plugin_drag.rs tests)"
                .to_owned(),
            "the preview/ghost path (INT-3) at storm load — the replica emits no previews \
             (previews_emitted stays 0); the machine's own test \
             previews_never_mutate_machines_or_events pins the frozen semantic counters"
                .to_owned(),
        ],
        params: vec![
            param("interactions_per_storm", INTERACTIONS as u64),
            param("settle_pairs", SETTLE_PER_STORM as u64),
            param("cancels", CANCELS_PER_STORM as u64),
            param("samples", SAMPLES_STORM_INTERACT),
            param("parks_per_drip_settle", (1 + PARKS_DRIP) as u64),
            param("parks_per_churn_settle", (1 + PARKS_CHURN) as u64),
            param("parks_per_cancel", (1 + PARKS_CANCEL) as u64),
            param(
                "expected_parks_per_storm",
                expected_parked,
            ),
            param(
                "expected_delivered_per_storm",
                expected_delivered,
            ),
        ],
        samples_n: SAMPLES_STORM_INTERACT,
        wall_ns: wall,
        extra_metrics: vec![
            ExtraMetric {
                label: "rearm_after_settle_cycle".to_owned(),
                samples_n: SAMPLES_REARM,
                wall_ns: rearm_wall,
                payload_bytes: None,
                note: Some(
                    "the armed→in-flight→settled RE-ARM cycle on one persistent (consumed) \
                     rail: begin + crossing + deliver + release + settle per sample; the \
                     settled rail re-arms (the machine's consumed family accepts begin)"
                        .to_owned(),
                ),
            },
            ExtraMetric {
                label: "fresh_rail_cycle".to_owned(),
                samples_n: SAMPLES_REARM,
                wall_ns: fresh_wall,
                payload_bytes: None,
                note: Some(
                    "the same cycle on a FRESH rail per sample (construction cost included) — \
                     the re-arm accent's contrast baseline"
                        .to_owned(),
                ),
            },
        ],
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![
            replica_counter(
                "begins_replica",
                calibrated.begins as u64,
                "DragCounters::begins shape (1:1 storm)",
            ),
            replica_counter(
                "settles_replica",
                calibrated.settles as u64,
                "DragCounters::settles shape",
            ),
            replica_counter(
                "cancels_replica",
                calibrated.cancels as u64,
                "DragCounters::cancels shape",
            ),
            replica_counter(
                "moves_parked_replica",
                calibrated.moves_parked as u64,
                "DragCounters::moves_parked shape (every accepted park, overwrites included)",
            ),
            replica_counter(
                "moves_delivered_replica",
                calibrated.moves_delivered as u64,
                "DragCounters::moves_delivered shape (per-tick takes + completion drains)",
            ),
            replica_counter(
                "refusals_duplicate_begin_replica",
                calibrated.refusals_duplicate as u64,
                "DragCounters::refusals_duplicate_begin shape",
            ),
            replica_counter(
                "refusals_not_draggable_replica",
                calibrated.refusals_not_draggable as u64,
                "DragCounters::refusals_not_draggable shape (the synced-def re-check)",
            ),
            replica_counter("previews_emitted_replica", 0, "DragCounters::previews_emitted shape (storm emits no ghosts)"),
            deferred_counter(
                "drag_counters",
                "the REAL DragCounters scroll (8 fields) is pub(crate) (src/ui/window/\
                 plugin_drag.rs); unit-test evidence: counters_are_the_defect_probe + the \
                 storm identities above, pinned by begin_move_settle_is_lossless / \
                 cancel_paths_are_one_to_one_with_begins / rapid_moves_latest_wins_never_drops_the_last"
                    .to_owned(),
            ),
        ],
        changed_elements: None,
        total_elements: None,
        changed_less_than_total: None,
        sink: format!(
            "lossless storm identity asserted per storm: begins == settles + cancels == 1:1 \
             ({SETTLE_PER_STORM}+{CANCELS_PER_STORM}); every completion's end == the \
             interaction's NEWEST accepted point (drain-first; the release argument is a \
             deliberate lie that must be ignored); every park slot empty at the boundary; \
             exactly-once refusals without counter movement; counter evolution printed at \
             storm milestones"
        ),
        notes: vec![
            "REPLICA bench: the rail machines transition-for-transition mirror \
             plugin_drag.rs; the REAL machines' evidence is the module's own tests \
             (deferred counters)"
                .to_owned(),
            "drip settles prove the ≤1-in-flight gate (the second take mid-flight must \
             return None); churn settles prove the drain-first settle (all churn parks \
             dropped but the newest); cancels prove the 1:1 with begins under Escape"
                .to_owned(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Scenario 4: keyboard_nav_burst — DEFERRED (recorded, not run)
// ---------------------------------------------------------------------------

/// Hand-built zero-percentile block for a NOT-RUN scenario (samples_n 0,
/// all zeros; the sink and the params name why — no invented numbers).
const ZERO_WALL: WallPercentiles = WallPercentiles {
    n: 0,
    p50_ns: 0.0,
    p95_ns: 0.0,
    p99_ns: 0.0,
    min_ns: 0.0,
    max_ns: 0.0,
};

fn scenario_keyboard_nav_deferred() -> ScenarioRecord {
    println!(
        "\n=== scenario: keyboard_nav_burst — SKIPPED (recorded, not run): the §7 keyboard \
         focus ring is HOST state (pub(crate)); no pub api-crate surface exists ==="
    );
    println!(
        "[keyboard_nav_burst] the ring walk (`nav_ring` naming what the builders rendered, \
         `focus_next`/`focus_prev` roving, `key_route` translation, \
         `MountCounters::keyboard_nav_steps`) all live in src/ui/window/\
         plugin_widget_mount.rs — bench-invisible; NOT measured, NO numbers claimed"
    );
    ScenarioRecord {
        scenario: "keyboard_nav_burst".to_owned(),
        surface: "NOT MEASURED — skipped (coordinator decision for SCN-022): the §7 keyboard \
                  focus ring is host state (pub(crate) in src/ui/window/plugin_widget_mount.rs); \
                  the API crate (WidgetSurface/PanelScene) carries NO ring/roving surface, so \
                  a replica has nothing honest to walk"
            .to_owned(),
        deferred: vec![
            "the 100k-tab-step keyboard burst (wall-clock + ring advance percentiles) — \
             planned when a bench-visible seam exists (SCN-012 precedent: kept deferred, \
             never emulated against a different semantic)"
                .to_owned(),
            "MountCounters::keyboard_nav_steps at burst load (registry counter, deferred)"
                .to_owned(),
        ],
        params: vec![
            param("executed", false),
            param("reason", "pub(crate)-only surface (ring is host state)"),
            param("planned_scope", "100_000 tab steps through the focus ring"),
        ],
        samples_n: 0,
        wall_ns: ZERO_WALL,
        extra_metrics: Vec::new(),
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![deferred_counter(
            "keyboard_nav_steps",
            "pub(crate) MountCounters::keyboard_nav_steps; unit-test evidence: \
             keyboard_ring_walks_and_skips_disabled, arrow_keys_drive_focus_and_selection_no_pointer, \
             disabled_keyboard_selection_refuses_cleanly, \
             table_keys_drive_row_and_cell_roving_and_escape_draft \
             (src/ui/window/plugin_widget_mount.rs tests)"
                .to_owned(),
        )],
        changed_elements: None,
        total_elements: None,
        changed_less_than_total: None,
        sink: "not executed — deferred with reasons; nothing here is a measurement"
            .to_owned(),
        notes: vec![
            "SKIPPED by coordinator decision — recorded so the run record shows the \
             scenario was DELIBERATELY not measured (the honest-byte rule: never silently \
             missing)"
                .to_owned(),
        ],
    }
}

// ---------------------------------------------------------------------------
// The SCN-022 run-record emitter (support pieces; task label SCN-022)
// ---------------------------------------------------------------------------

/// Print the env lines, assemble the run record with the SCN-022 task
/// label, and print the pretty JSON (the stdout JSON IS the file). Reuses
/// the support's `env_block`/`RunRecord` unchanged — only the task label
/// differs from the SCN-012 emitter (which hardcodes its own task).
fn emit_scn022_run_record(bench: &str, scenarios: Vec<ScenarioRecord>) -> RunRecord {
    let env: EnvBlock = env_block(bench);
    println!(
        "\n=== SCN-022 run: {bench} (replica of the pub(crate) pointer machines, at the \
         ocs_plugin_api pub layer) ==="
    );
    println!(
        "host={} os={} arch={} profile={} kernel={}",
        env.host, env.os, env.arch, env.profile, env.kernel
    );
    println!(
        "rustc={} git_commit={} git_dirty={} timestamp_unix={} pid={}",
        env.rustc, env.git_commit, env.git_dirty, env.timestamp_unix, env.pid
    );
    let record = RunRecord {
        run_record_format: RUN_RECORD_FORMAT,
        task: "SCN-022",
        bench: bench.to_owned(),
        env,
        scenarios,
    };
    println!("\n=== RUN RECORD ===");
    match serde_json::to_string_pretty(&record) {
        Ok(json) => println!("{json}"),
        // Unreachable for derive(Serialize) data, but never panic in a bench.
        Err(e) => println!(
            "{{\"run_record_format\":\"{RUN_RECORD_FORMAT}\",\"serialize_error\":\"{e}\"}}"
        ),
    }
    black_box(&record);
    record
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    println!("=== SCN-022 POINTER-BURST interaction bench (widget_interaction) ===");
    println!("HONEST-BYTE SPLIT (both halves stated before any number):");
    println!(
        "- bench-invisible (grep at this branch tip): src/ui/window/plugin_drag.rs prints \
         pub(crate) for DragSession/DragCounters/DragPhase/its methods; \
         src/ui/window/plugin_widget_mount.rs prints pub(crate) for MountInputState/HoverState \
         + mount_move_defer/mount_flush_moves/mount_pointer_released/mount_press"
    );
    println!(
        "- so the wall-clock layers here are the CONTRACT-SHAPED REPLICA structures \
         (bench-side, field-for-field mirrors over pub ocs_plugin_api types) — \
         every scenario's `surface` labels this"
    );
    println!(
        "- the REAL-path halves are the host's unit tests + registry counters, named per \
         scenario in the deferred counters / deferred vecs (the authoritative evidence)"
    );

    // The anchored fixture (validated; its row ids ARE the draggable set).
    let anchor = anchored_surface();
    black_box(&anchor_addresses(&anchor));
    println!(
        "anchor surface: validated (1 tree × {} enabled rows, revision {}; surface ids \
         in the 20_xxx row namespace; rail key {})",
        ANCHOR_ROWS, ANCHOR_REVISION, PANEL_SURFACE_ID
    );

    let scenarios = vec![
        scenario_park_storm(),
        scenario_drain_latest_only(),
        scenario_settle_cancel_storm(),
        scenario_keyboard_nav_deferred(),
    ];

    let record = emit_scn022_run_record("widget_interaction", scenarios);
    black_box(&record);
    // Bench convention: honest exit (0) — panics on any contract defect.
}
