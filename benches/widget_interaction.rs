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
//! `pub(crate)`). That r0 branch-tip grep made the wall-clock layer a
//! CONTRACT-SHAPED **REPLICA** at the `ocs_plugin_api` pub layer:
//! bench-side structures that mirror the two machines field-for-field
//! (same state shape, same transitions, same counter moves), driven over
//! the pub API vocabulary — `WidgetSurface`/`WidgetNode`/`TreeRowDef`
//! (the anchored draggable address space, validated at construction),
//! `StableId`/`SceneRevision`, `DragMeta` (the verbatim payload), and the
//! real `iced::Point` the host machines use. Those replica rows REMAIN in
//! this bench (evidence kept, none deleted); each replica `surface` label
//! now carries a `superseded-by-real` pointer to the matching REAL row.
//!
//! r1 (SCN-024, ADR-007 item 3): the deferred real-counter wall-clock rows
//! LANDED through the host's own `#[doc(hidden)] pub` bench seams —
//! `MountRegistry` (+ `SurfaceMountHandle` construction + the `*_owned`
//! drive/read paths) and the REAL `DragSession`/`DragCounters` machines.
//! The seams are inspect/drive-only (SCN-024's visibility-only widening;
//! never wire; not part of the host's documentary API). The cfg(test)
//! static facade in plugin_widget_mount.rs is still invisible to benches
//! (benches compile WITHOUT cfg(test)) — the bench-visible surface is the
//! INSTANCE world: a bench-local `MountRegistry::default()` the bench
//! registers its anchored surface into.
//!
//! Honest-byte rule (SCN-012 convention, binding): every scenario labels
//! WHICH surface it measured — REAL rows label `REAL host registry/machines`
//! and carry their counters at `scope: "measured-real"` (the run record's
//! deferred map moves those items from `deferred` to `measured-real`);
//! replica rows label the bench-side replica structures, and the remaining
//! pub(crate) halves stay `scope: "deferred"` counters pointing at the
//! in-source unit tests that hold the authoritative evidence today. Both
//! facts print every run. The replica CALIBRATES the contract; the real
//! rows MEASURE the real machines' wall-clock + counter evolution (§7
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
//! r0 shape (the SCN-022 replica rows — kept running, honestly labeled):
//! 1. `park_storm_10k` — 10 000 pointer moves deferred into ONE park slot
//!    (the `move_owed` shape): the slot never grows (constant footprint,
//!    asserted per sample), it holds EXACTLY the newest position, and the
//!    bench-emulated `Vec` page-queue anti-pattern (the same 10 000 moves
//!    queued) is measured beside it as the growth the slot must not have.
//!    *(r1: wall-clock superseded by `mount_move_coalesce_real`.)*
//! 2. `drain_delivers_latest_only` — the 10 000-park storm answered by ONE
//!    tick flush = exactly one delivery = the storm's LAST position
//!    (latest-wins delivery; lossless = the latest defines). Percentiles
//!    over 1 000 tick-flushes and over the release-completion path.
//!    *(r1: wall-clock superseded by `mount_move_coalesce_real`.)*
//! 3. `settle_cancel_storm` — 1 000 begin/settle pairs + 250 cancels per
//!    storm through the DragSession-replica rail: every interaction
//!    completes exactly once, every completion DRAINS the parked slot
//!    first (the newest move is never dropped at the boundary), the 1:1
//!    counter identity `begins == settles + cancels` holds, and re-arm
//!    (armed→in-flight→settled→armed again) costs are timed beside fresh
//!    rails. Counter evolution is printed at storm milestones.
//!    *(r1: wall-clock superseded by `drag_storm_real`.)*
//! 4. `keyboard_nav_burst` — DEFERRED in r0 (recorded, not run: the §7
//!    keyboard focus ring was pub(crate) host state). *(r1: EXECUTED as a
//!    REAL row — see below; the r0 record keeps its skipped shape in that
//!    untouched log.)*
//!
//! r1 additions (SCN-024, ADR-007 item 3 — the host-counter wall-clock
//! rows through the pub `#[doc(hidden)]` bench seams):
//! 5. `mount_move_coalesce_real` — the parked-move storm through the REAL
//!    registry (`MountRegistry::mount_move_defer_owned` /
//!    `mount_flush_moves_owned` / the release completion), real
//!    `MountCounters::mount_move_calls` / `coalesced_moves_superseded`
//!    evolution printed as before/after deltas, newest position asserted
//!    from the real per-surface input snapshot.
//! 6. `drag_storm_real` — the REAL `DragSession` begin/move/settle/cancel
//!    storm (same interleave shape as the replica row) with the real
//!    `DragCounters` scroll; printed plotted BESIDE the replica row.
//! 7. `keyboard_nav_burst` — N key steps (e.g. 10 000 Tab steps per burst,
//!    a 1 024-step Down/Up sawtooth) through the REAL ring + roving
//!    machines (`key_route_owned`, which also ticks the real
//!    `MountCounters::keyboard_nav_steps`), per-step deterministic
//!    row-landing asserts (the newest-wins roving chain), wall-clock
//!    percentiles + counter evolution. The r0 SKIPPED row is now real.
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

use iced::keyboard::{Key, Modifiers, key as keyboard_key};
use iced::Point;
use ocs_plugin_api::scene::{DragMeta, SceneRevision, StableId};
use ocs_plugin_api::widgets::{
    TreeCapabilities, TreeRowDef, WidgetNode, WidgetSurface, WidgetTreeDef,
};
use OpenCADStudio::ui::window::plugin_drag::{
    DragCancelReason, DragCounters, DragRefusal, DragSession, MoveDisposition, DRAG_THRESHOLD_PX,
};
use OpenCADStudio::ui::window::plugin_widget_mount::{
    MountRegistry, SurfaceMountHandle, WidgetMountMsg, bench_handle_mount_key_owned,
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

// ---------------------------------------------------------------------------
// SCN-024 REAL-row workload constants (ADR-007 item 3; the bench seams)
// ---------------------------------------------------------------------------

/// The bench's registered panel id in its own REAL registry instance (the
/// registration key is a plain string — the app's panel ids; a bench-local
/// namespace so no real panel id can collide).
const BENCH_PANEL_ID: &str = "bench-widget-interaction";

/// Keyboard burst, main population: Tab steps per timed sample (the r0
/// skipped row's planned burst order; the ring wraps every `ANCHOR_ROWS`
/// steps — the seat model is exact for every i).
const KEY_STEPS_TAB_BURST: usize = 10_000;
/// Samples over the 10k-step Tab bursts (slow ops floor: >= 20).
const SAMPLES_KEY_TAB_BURST: usize = 20;
/// Down/Up sawtooth: cycles of (64 Downs + 64 Ups) = 1 024 keys per
/// sample; the row-aligned shape keeps every productive step on either
/// side of the ring edges and gives the edge REFUSALS exact counts (the
/// clean-refusal part of the pure-layer contract).
const KEY_SAWTOOTH_CYCLES: usize = 8;
const KEY_SAWTOOTH_STEPS: usize = KEY_SAWTOOTH_CYCLES * 2 * ANCHOR_ROWS; // 1_024
/// Samples over the 1 024-key sawtooth populations.
const SAMPLES_KEY_SAWTOOTH: usize = 60;
/// Single-step Tab population (per-step hop-cost percentiles).
const SAMPLES_KEY_STEP: usize = 1_000;

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

/// Real-counter (SCN-024): read off the REAL host machines, driven through
/// the #[doc(hidden)] pub bench seams. NOTE the scope switch: items the r0
/// run recorded as `scope: "deferred"` print at `measured-real` from here
/// on, with the value = the measured counter walk.
fn measured_real_counter(name: &str, value: u64, machine: &str) -> Counter {
    Counter {
        name: name.to_owned(),
        scope: "measured-real",
        value: Some(value),
        note: Some(format!(
            "REAL {machine} driven through the #[doc(hidden)] bench seam              (SCN-024; ADR-007 item 3; never wire) — was scope \"deferred\" in              pointer-burst-r0.log"
        )),
    }
}

/// A REAL `MountRegistry` instance with the anchored surface registered and
/// the keyboard target named — the "set the target surface first" setup
/// step of every real row. Returns the registry and the registered
/// surface's id (the mount state opens under the surface's FIRST root id —
/// the same opener `register_test_surface` uses in-crate).
fn registered_registry() -> (MountRegistry, StableId) {
    let surface = anchored_surface();
    let mount_id = surface
        .roots
        .first()
        .map(|node| node.widget_id())
        .unwrap_or(StableId(0));
    let mut registry = MountRegistry::default();
    registry.register_surface_owned(
        BENCH_PANEL_ID,
        SurfaceMountHandle::from_surface(mount_id, surface),
    );
    // The point-free keyboard-grab seam: name the surface as the keys'
    // target without a pointer press (the bench's burst pre-step).
    registry.set_mount_key_target_owned(Some(mount_id));
    // The resolution read-back: the keyboard target resolution must honor
    // the pointed target (the same read the app's route step 1 makes).
    assert_eq!(
        registry.mount_key_surface_owned(),
        Some(mount_id),
        "set-the-target-first: the REAL resolution honors the named target"
    );
    (registry, mount_id)
}

/// ONE real keyboard step — the app's `on_mount_key` route body, driven
/// here call-for-call (the app fn itself sits behind the private
/// `crate::app::update` module chain and is NOT bench-reachable; these are
/// the SAME three in-crate calls it makes, in the same order):
/// 1. target resolution ([`MountRegistry::mount_key_surface_owned`] — the
///    press-captured surface, else the sole registered mount),
/// 2. the handle lookup ([`MountRegistry::handle_for_surface_owned`]),
/// 3. the REAL pure keyboard layer (the route's `key_route_owned` core —
///    ring walk ([`nav_ring`]) + the roving machines + the mount-route
///    translation + the real `MountCounters::keyboard_nav_steps` tick
///    inside).
///
/// The route itself is the app's own `on_mount_key` body, restated
/// piece-for-piece in the host as the #[doc(hidden)] seam
/// `bench_handle_mount_key_owned` (the app fn's crate::app::update module
/// chain is private and unreachable from a bench; the seam drives the SAME
/// pieces — the bench never re-implements the route itself).
fn real_key_step(
    registry: &mut MountRegistry,
    key: Key,
    modifiers: Modifiers,
) -> Vec<WidgetMountMsg> {
    bench_handle_mount_key_owned(registry, key, modifiers)
}

/// The sawtooth key index -> key choice: the first 64 keys of a cycle are
/// ArrowDown, the last 64 ArrowUp (rows are aligned so the productive
/// steps never cross a hard frame except at the EXACT edge-refusal probes
/// the pattern wants).
fn sawtooth_key(k: usize) -> Key {
    let in_cycle = k % (2 * ANCHOR_ROWS);
    if in_cycle < ANCHOR_ROWS {
        Key::Named(keyboard_key::Named::ArrowDown)
    } else {
        Key::Named(keyboard_key::Named::ArrowUp)
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
         in-source unit tests + registry counters (deferred counters below). \
         superseded-by-real (SCN-024): the REAL registry's wall-clock + \
         counters live in the mount_move_coalesce_real row (the #[doc(hidden)] \
         bench seams, ADR-007 item 3); this replica row is kept as the \
         calibrated contract evidence, superseded for wall-clock only"
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
         in-source unit tests (deferred counters below). superseded-by-real \
         (SCN-024): the REAL DragSession's wall-clock + DragCounters live in \
         the drag_storm_real row (the #[doc(hidden)] bench seams, ADR-007 \
         item 3); this replica row is kept as the calibrated contract \
         evidence, superseded for wall-clock only"
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
// SCN-024 REAL rows: the wall-clock through the #[doc(hidden)] pub bench
// seams (ADR-007 item 3; never wire; inspect/drive-only; NOT a wire change)
// ---------------------------------------------------------------------------

/// One REAL parked-move storm's verified outcome (each field the suffix
/// asserts + the real counter evolution the caller prints).
struct RealCoalesceOutcome {
    defers_accepted: usize,
    flush_delivered: usize,
    release_delivered: usize,
    last_move_is_newest: bool,
    move_calls_delta: u64,
    superseded_delta: u64,
}

/// Drive ONE REAL parked-move storm through the REAL registry (the
/// per-surface coalescer entry inside the registry's input store):
/// STORM_MOVES defers -> ONE flush -> the newest position read back from
/// the REAL input snapshot -> press/park/release completion probe on the
/// same surface. Every contract leg asserts HERE (a defective storm panics
/// the bench).
fn real_coalesce_storm(registry: &mut MountRegistry, mount_id: StableId) -> RealCoalesceOutcome {
    let before = registry.counters_owned();
    for i in 0..STORM_MOVES {
        let Some(superseded) = registry.mount_move_defer_owned(mount_id, storm_point(i)) else {
            panic!("a registered surface defers (the unknown-surface grace is for OTHER ids)");
        };
        black_box(superseded);
    }
    let after_defers = registry.counters_owned();
    assert_eq!(
        after_defers.coalesced_moves_superseded - before.coalesced_moves_superseded,
        (STORM_MOVES - 1) as u64,
        "every storm park after the first SUPERSEDED (latest wins, REAL counters)"
    );
    assert_eq!(
        after_defers.mount_move_calls - before.mount_move_calls,
        0,
        "no flush yet: the parks answered NOTHING (a defer is a slot write)"
    );
    let flush_delivered = registry.mount_flush_moves_owned();
    assert_eq!(
        flush_delivered, 1,
        "10000 parks -> exactly ONE flush delivery per surface"
    );
    let state = registry
        .mount_hover_state_owned(mount_id)
        .expect("the driven surface's input entry");
    assert_eq!(
        state.last_move,
        Some(storm_point(STORM_MOVES - 1)),
        "lossless: the REAL flush delivered the storm's LAST position"
    );
    let after_flush = registry.counters_owned();
    assert_eq!(
        after_flush.mount_move_calls - after_defers.mount_move_calls,
        1,
        "one flush = one mount call (<=1/surface/tick)"
    );
    // The release completion: press re-captures, ONE park, release answers
    // it NOW. The press SUPERSEDES a still-owed park WITHOUT counting it
    // (the viewport press precedent: the press is the later truth).
    assert!(
        registry.mount_press_owned(mount_id),
        "press captures the registered surface"
    );
    let final_park = registry
        .mount_move_defer_owned(mount_id, storm_point(19))
        .expect("the pressed surface defers");
    assert_eq!(final_park, false, "the slot was drained by the flush");
    let release_delivered = registry.mount_pointer_released_owned(mount_id);
    assert_eq!(release_delivered, 1, "the completion answers the owed park NOW");
    let after_release = registry.counters_owned();
    assert_eq!(
        after_release.mount_move_calls - after_flush.mount_move_calls,
        1,
        "the release completion delivered exactly one"
    );
    assert_eq!(
        after_release.coalesced_moves_superseded - after_flush.coalesced_moves_superseded,
        0,
        "the press superseded the owed park WITHOUT counting it (press is the later truth)"
    );
    RealCoalesceOutcome {
        defers_accepted: STORM_MOVES,
        flush_delivered,
        release_delivered,
        last_move_is_newest: state.last_move == Some(storm_point(STORM_MOVES - 1)),
        move_calls_delta: after_release.mount_move_calls - before.mount_move_calls,
        superseded_delta: after_release.coalesced_moves_superseded
            - before.coalesced_moves_superseded,
    }
}

/// Scenario r1-1: the parked-move storm through the REAL registry: the
/// superseding-real twin of `park_storm_10k` + `drain_delivers_latest_only`
/// (printed side by side; the replica rows keep running as evidence).
fn scenario_mount_move_coalesce_real(
    replica_park: &ScenarioRecord,
    replica_drain: &ScenarioRecord,
) -> ScenarioRecord {
    let scenario = "mount_move_coalesce_real";
    println!();
    println!(
        "=== scenario: {scenario} -- {STORM_MOVES} moves into the REAL registry's owed slot (REAL host registry/machines, the #[doc(hidden)] bench seams) ==="
    );
    // Counter evolution BEFORE the timed passes: a fresh instance's
    // all-zero base, printed (the deltas print after the passes).
    let (evolution_probe, _) = registered_registry();
    let zero = evolution_probe.counters_owned();
    println!(
        "[mount_move_coalesce_real] REAL MountCounters before any storm: mount_move_calls {} coalesced_moves_superseded {} (fresh instance, all-zero base)",
        zero.mount_move_calls, zero.coalesced_moves_superseded
    );

    // TIMED storms (assertions live: real_coalesce_storm panics on any
    // contract defect; the wall measures REAL per-park registry work).
    let samples = timed_ns(SAMPLES_STORM, WARMUP, || {
        let (mut registry, mount_id) = registered_registry();
        let outcome = real_coalesce_storm(&mut registry, mount_id);
        let walk = registry.counters_owned();
        black_box(&outcome);
        black_box(&walk);
    });
    let wall = WallPercentiles::from_samples(&samples);
    println!(
        "[mount_move_coalesce_real] REAL storm pct ({SAMPLES_STORM} x {STORM_MOVES} parks + flush + release completion): p50 {:.0} ns | p95 {:.0} ns | p99 {:.0} ns | min {:.0} | max {:.0} (per-park mean ~ {:.3} ns)",
        wall.p50_ns,
        wall.p95_ns,
        wall.p99_ns,
        wall.min_ns,
        wall.max_ns,
        wall.p50_ns / STORM_MOVES as f64
    );
    println!(
        "[mount_move_coalesce_real] counter evolution per storm (before -> after, REAL deltas): mount_move_calls {} -> {} (flush +1, release +1) | coalesced_moves_superseded {} -> {} ({superseded} storm parks superseded) | flush_delivered 1 | release_delivered 1 | last_move == storm-last, read from the REAL HoverState",
        0, 2, 0, STORM_MOVES - 1, superseded = STORM_MOVES - 1
    );

    // Per-tick population: ONE park + ONE flush per sample on a PERSISTENT
    // registry (the tick's normal shape; the REAL counter accumulation is
    // itself the assertion target: +1 mount move call per tick, a thousand
    // of them, ZERO superseded: no storm here).
    let (mut tick_registry, mount_id) = registered_registry();
    let before = tick_registry.counters_owned();
    // (warmup = 2 real resolved ticks on the SAME persistent registry: their
    // counter movement is counted by the REAL counters below — the
    // percentiles only time the population's samples; the assert accounts
    // for both, honestly.)
    const TICK_WARMUP: usize = 2;
    let tick_wall = timed_ns(SAMPLES_FLUSH_TAKE, TICK_WARMUP, || {
        let defer_accepted = tick_registry
            .mount_move_defer_owned(mount_id, storm_point(7))
            .expect("registered surface defers");
        assert_eq!(
            defer_accepted, false,
            "a single park per tick supersedes nothing"
        );
        let delivered = tick_registry.mount_flush_moves_owned();
        assert_eq!(delivered, 1, "every REAL tick-flush delivers exactly one");
        let state = tick_registry
            .mount_hover_state_owned(mount_id)
            .expect("input entry");
        assert_eq!(
            state.last_move,
            Some(storm_point(7)),
            "the REAL flush delivered the newest"
        );
        black_box(&state);
    });
    let after = tick_registry.counters_owned();
    let tick_delta_calls = after.mount_move_calls - before.mount_move_calls;
    let tick_delta_superseded = after.coalesced_moves_superseded - before.coalesced_moves_superseded;
    assert_eq!(
        tick_delta_calls,
        (SAMPLES_FLUSH_TAKE + TICK_WARMUP) as u64,
        "each of the 1000 timed ticks (plus the 2 untimed warmup ticks, real resolved          ticks on the same persistent registry) is exactly one mount move call"
    );
    assert_eq!(
        tick_delta_superseded, 0,
        "no supersession across the tick population"
    );
    let tick_wall = WallPercentiles::from_samples(&tick_wall);
    println!(
        "[mount_move_coalesce_real] REAL tick population ({SAMPLES_FLUSH_TAKE} park->flush cycles, persistent registry): p50 {:.0} ns | p99 {:.0} ns | counter evolution: mount_move_calls {} -> {} (delta {tick_delta_calls}), coalesced_moves_superseded {} -> {} (delta {tick_delta_superseded})",
        tick_wall.p50_ns,
        tick_wall.p99_ns,
        before.mount_move_calls,
        after.mount_move_calls,
        before.coalesced_moves_superseded,
        after.coalesced_moves_superseded
    );

    // The side-by-side against the r0 replica rows (their evidence kept).
    println!(
        "[mount_move_coalesce_real] side-by-side (same shapes): park_storm_10k REPLICA p50 {:.0} ns per {STORM_MOVES}-park storm | REAL registry storm p50 {:.0} ns | drain_delivers_latest_only REPLICA p50 {:.0} ns per {STORM_MOVES}-park + flush | the REAL flush + completion are timed inside this row's storm samples",
        replica_park.wall_ns.p50_ns, wall.p50_ns, replica_drain.wall_ns.p50_ns
    );

    ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: "REAL host registry/machines: MountRegistry instance (mount_move_defer_owned / mount_flush_moves_owned / mount_press_owned / mount_pointer_released_owned / mount_hover_state_owned / counters_owned) over its SurfaceMountHandle::from_surface registration, driven through the #[doc(hidden)] bench seam (SCN-024; ADR-007 item 3; never wire; NOT the host's documentary API)".to_owned(),
        deferred: vec![
            "multi-surface + unknown-surface interleave loads (this row drives ONE registered surface; the unknown-surface grace is unit-tested — unknown_surface_moves_are_graceful — not timed)".to_owned(),
            "the app-arm route AROUND the registry (Message::WidgetMountMsg handling + Task plumbing): not registry-side cost, stays unmeasured".to_owned(),
        ],
        params: vec![
            param("parks_per_sample", STORM_MOVES),
            param("flushes_per_sample", 1_u64),
            param("surfaces_registered", 1_u64),
            param("samples", SAMPLES_STORM),
            param("tick_flush_population", SAMPLES_FLUSH_TAKE),
            param("expected_flush_deliveries", 1_u64),
            param("expected_superseded_per_storm", (STORM_MOVES - 1) as u64),
        ],
        samples_n: SAMPLES_STORM,
        wall_ns: wall,
        extra_metrics: vec![ExtraMetric {
            label: "real_tick_flush_1k".to_owned(),
            samples_n: SAMPLES_FLUSH_TAKE,
            wall_ns: tick_wall,
            payload_bytes: None,
            note: Some(format!(
                "percentiles over {} REAL registry tick-flushes (park -> flush, persistent registry): delivered == 1 and newest == asserted per flush; cumulative REAL counter deltas: mount_move_calls +{} (the {} timed ticks + the {} untimed warmup ticks are all real resolved ticks on the one persistent registry), coalesced_moves_superseded +{}",
                SAMPLES_FLUSH_TAKE, tick_delta_calls, SAMPLES_FLUSH_TAKE, TICK_WARMUP, tick_delta_superseded
            )),
        }],
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![
            measured_real_counter(
                "mount_move_calls",
                2,
                "MountCounters per REAL storm: +1 flush +1 release completion",
            ),
            measured_real_counter(
                "coalesced_moves_superseded",
                (STORM_MOVES - 1) as u64,
                "MountCounters per REAL storm (every park after the first supersedes)",
            ),
            measured_real_counter(
                "mount_move_calls",
                tick_delta_calls,
                "cumulative real mount_move_calls across the 1000 real tick-flushes (persistent registry)",
            ),
            measured_real_counter(
                "coalesced_moves_superseded",
                tick_delta_superseded,
                "cumulative real coalesced_moves_superseded across the tick population (zero: one park per tick never supersedes)",
            ),
        ],
        changed_elements: None,
        total_elements: None,
        changed_less_than_total: None,
        sink: format!(
            "every sample asserted on the REAL machines: all {} defers accepted, ONE flush delivery == the storm's LAST position (read from the REAL HoverState.last_move), REAL deltas mount_move_calls == 2 (flush + completion), coalesced_moves_superseded == {} - 1, the press-supersede UNCOUNTED, the 1000 tick flushes +1 call each: a lost newest, a >1 delivery or a counted press-supersede panics the bench",
            STORM_MOVES, STORM_MOVES
        ),
        notes: vec![
            "REAL row (SCN-024): the wall-clock + counters measure the REAL registry drive path through the #[doc(hidden)] bench seams (ADR-007 item 3); the r0 replica rows it supersedes for wall-clock are park_storm_10k and drain_delivers_latest_only (replica evidence kept in pointer-burst-r0.log and in this bench's rows)".to_owned(),
            "the real defer is a per-call registry scan (by_panel values, any surface-id match) + the slot write — the app arm pays exactly this per move message".to_owned(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Scenario r1-2: the REAL DragSession storm (the settle_cancel_storm
// replica row's superseding-real twin)
// ---------------------------------------------------------------------------

/// The REAL full storm: INTERACTIONS through ONE REAL DragSession — the
/// same loop the replica's drag_storm runs (the shape's mirror) on the
/// real machines: begin/move_drag/take_deliverable_move/release_delivered_move/
/// settle/cancel + the REAL DRAG_THRESHOLD_PX + the REAL DragCounters
/// scroll. Every interaction completes exactly once; every completion
/// DRAINS FIRST (the end == the interaction's newest accepted point); the
/// 1:1 identity and the no-counter-movement refusals assert.
/// print_evolution prints the REAL counter scroll at milestones (the
/// untimed calibration pass).
fn real_drag_storm(print_evolution: bool) -> StormProgress {
    let draggable = anchor_addresses(&anchored_surface());
    let rail_key = StableId(PANEL_SURFACE_ID);
    let mut session = DragSession::with_draggables(rail_key, draggable);
    let mut progress = StormProgress::default();
    let storm_start = std::time::Instant::now();

    // THE REAL NOT-DRAGGABLE PROBE (once per storm, before the interaction
    // loop): an address the REAL synced set does NOT contain refuses arm,
    // counts ONLY refusals_not_draggable, the rail stays Idle for the storm.
    let probe_address = StableId(TREE_ROW_BASE + 999_999);
    match session.begin(rail_key, probe_address, meta("probe"), Point::new(10.0, 10.0), 0) {
        Err(DragRefusal::NotDraggable(addr)) => {
            assert_eq!(addr, probe_address, "the refusal names the probed address");
            progress.not_draggable_probes += 1;
        }
        Ok(_) => panic!("the probe address must refuse (NotDraggable)"),
        Err(other) => panic!("the probe address refused as {other:?}, not NotDraggable"),
    }

    for k in 0..INTERACTIONS {
        let cancelled_interaction = k % 5 == 4;
        let address = StableId(TREE_ROW_BASE + (k % ANCHOR_ROWS) as u64);
        let label = format!("row-{}", k % ANCHOR_ROWS);
        let start = Point::new(10.0, 10.0);

        let began = session
            .begin(rail_key, address, meta(&label), start, k as u64)
            .expect("arm (the rail was consumed by the previous interaction)");
        progress.begins += 1;

        // One duplicate-begin refusal probe every 250th interaction through
        // the REAL exactly-once guard: the SECOND begin while Armed refuses,
        // moves ONLY its own REAL counter, cannot retarget the armed origin.
        if k % 250 == 7 {
            let before = session.counters();
            assert!(matches!(
                session.begin(rail_key, address, meta("ignored"), start, k as u64),
                Err(DragRefusal::DuplicateBegin(_))
            ));
            progress.dup_probes += 1;
            let after = session.counters();
            assert_eq!(
                after,
                DragCounters {
                    refusals_duplicate_begin: before.refusals_duplicate_begin + 1,
                    ..before
                },
                "the DuplicateBegin refusal moves ONLY its own REAL counter"
            );
        }

        if cancelled_interaction {
            // CANCEL storm route (the REAL machines): threshold-exact
            // crossing + PARKS_CANCEL churn parks, then cancel: the REAL
            // drain-first hands the NEWEST point back.
            let crossed = Point::new(10.0 + DRAG_THRESHOLD_PX, 10.0);
            assert_eq!(session.move_drag(crossed), MoveDisposition::Crossed);
            let mut newest = crossed;
            progress.offered_parks += 1;
            for j in 1..=PARKS_CANCEL {
                let p = Point::new(13.0 + (j * 8) as f32, 10.0 + j as f32 * 3.0);
                assert_eq!(
                    session.move_drag(p),
                    MoveDisposition::Parked { superseded: true },
                    "in-flight cancel-route park overwrites the still-owed one"
                );
                newest = p;
                progress.offered_parks += 1;
            }
            let cancelled = session
                .cancel(DragCancelReason::Escape)
                .expect("the live drag cancels 1:1 with its begin");
            assert_eq!(
                cancelled.end_point, newest,
                "cancel DRAINED FIRST: the newest move defines the end (lossless)"
            );
            assert_eq!(
                cancelled.address, began.origin.address,
                "address verbatim (origin facts captured at arm time)"
            );
            assert_eq!(
                cancelled.drag_meta, began.origin.drag_meta,
                "meta returned VERBATIM (the meta-never-interpreted rule INT-5)"
            );
            assert_eq!(cancelled.start_point, began.origin.start_point);
            assert_eq!(
                cancelled.reason,
                DragCancelReason::Escape,
                "the caller's reason rides the cancel fact verbatim"
            );
            // The REAL consumed-family observables: the park slot is empty,
            // extra moves are graceful no-ops (MoveDisposition::Inactive —
            // the REAL shape's way of saying no interaction is live).
            assert!(
                session.take_deliverable_move().is_none(),
                "no move survives the boundary"
            );
            assert_eq!(
                session.move_drag(Point::new(0.0, 0.0)),
                MoveDisposition::Inactive,
                "the consumed session treats further moves as noise"
            );
            progress.offered_deliveries += 1; // the cancel's own drain
            progress.cancels += 1;
        } else {
            // SETTLE storm route (the REAL machines): alternate per-tick
            // Drip and pure-CHURN shapes; EXACT same park pacing as the
            // replica row.
            let drip = k % 2 == 0;
            let crossed = Point::new(10.0 + DRAG_THRESHOLD_PX, 10.0);
            assert_eq!(session.move_drag(crossed), MoveDisposition::Crossed);
            let mut newest = crossed;
            progress.offered_parks += 1;
            for j in 1..=PARKS_DRIP {
                let p = Point::new(13.0 + (j * 8) as f32, 10.0 + j as f32 * 3.0);
                assert_eq!(
                    session.move_drag(p),
                    MoveDisposition::Parked { superseded: !drip || j == 1 },
                    "in-flight parks overwrite the still-owed one (latest wins)"
                );
                newest = p;
                progress.offered_parks += 1;
                if drip {
                    let taken = session
                        .take_deliverable_move()
                        .expect("the <=1-in-flight gate");
                    assert_eq!(taken, p, "the tick delivers the newest parked move");
                    assert!(
                        session.take_deliverable_move().is_none(),
                        "NOTHING delivers while one is in flight (the REAL gate)"
                    );
                    assert!(session.release_delivered_move());
                    progress.offered_deliveries += 1;
                }
            }
            // The release argument is the DELIBERATE LIE ((999, -999)) — the
            // REAL drain-first rule must make the end context ignore it.
            let settled = session
                .settle(Point::new(999.0, -999.0))
                .expect("the live drag settles 1:1 with its begin");
            assert_eq!(
                settled.end_point, newest,
                "settle DRAINED FIRST / last-delivered: the NEWEST move defines the end, never the (lying) release argument: lossless latest-wins"
            );
            assert_eq!(settled.address, began.origin.address, "address verbatim");
            assert_eq!(
                settled.drag_meta, began.origin.drag_meta,
                "meta returned VERBATIM (INT-5)"
            );
            assert_eq!(settled.start_point, began.origin.start_point);
            assert!(
                session.take_deliverable_move().is_none(),
                "no move survives the boundary"
            );
            assert_eq!(
                session.move_drag(Point::new(0.0, 0.0)),
                MoveDisposition::Inactive,
                "the consumed session treats further moves as noise"
            );
            if !drip {
                progress.offered_deliveries += 1; // the churn settle's drain
            }
            progress.settles += 1;
        }
        progress.newest_checked += 1;
        progress.slot_drained_checked += 1;

        // Every 250th interaction: the double-completion refusal probe —
        // REAL NotInFlight, NEITHER moves a REAL counter.
        if k % 250 == 49 {
            let before = session.counters();
            assert_eq!(
                session.settle(Point::new(0.0, 0.0)),
                Err(DragRefusal::NotInFlight),
                "exactly-once: the consumed rail refuses a second settle"
            );
            assert_eq!(
                session.cancel(DragCancelReason::Escape),
                Err(DragRefusal::NotInFlight),
                "exactly-once: the consumed rail refuses a second cancel"
            );
            assert_eq!(
                session.counters(),
                before,
                "refused completions move NO REAL counter"
            );
        }

        if print_evolution && (k + 1) % 250 == 0 {
            let machine = session.counters();
            println!(
                "[drag_storm_real] REAL DragCounters evolution @ interaction {:>5}/{total}: begins {begins} settles {settles} cancels {cancels} | parked {parked} delivered {delivered} | refusals: dup-start {dup} not-draggable {nd} | previews {previews} | checked: end==newest {newest_checked}/{INT}, slots cleared {drained}/{INT}",
                k + 1,
                total = INTERACTIONS,
                begins = machine.begins,
                settles = machine.settles,
                cancels = machine.cancels,
                parked = machine.moves_parked,
                delivered = machine.moves_delivered,
                dup = progress.dup_probes,
                nd = progress.not_draggable_probes,
                previews = machine.previews_emitted,
                newest_checked = progress.newest_checked,
                INT = INTERACTIONS,
                drained = progress.slot_drained_checked
            );
        }
    }

    // The dual-side SYNC (REAL counters == the storm's OFFERED history —
    // the machine accepted exactly the storm it was driven with).
    let machine = session.counters();
    progress.moves_parked = machine.moves_parked;
    progress.moves_delivered = machine.moves_delivered;
    assert_eq!(machine.begins, progress.begins, "begins: REAL machine == offered");
    assert_eq!(machine.settles, progress.settles, "settles: REAL machine == offered");
    assert_eq!(machine.cancels, progress.cancels, "cancels: REAL machine == offered");
    assert_eq!(
        machine.moves_parked, progress.offered_parks,
        "parks: REAL machine == offered (every park accepted, no more)"
    );
    assert_eq!(
        machine.moves_delivered, progress.offered_deliveries,
        "deliveries: REAL machine == offered (every take and every drain counted once)"
    );
    assert_eq!(
        machine.refusals_duplicate_begin, progress.dup_probes,
        "DuplicateBegin refusals == the probes (REAL)"
    );
    assert_eq!(
        machine.refusals_not_draggable, progress.not_draggable_probes,
        "NotDraggable refusals == the probes (REAL)"
    );
    progress.refusals_duplicate = progress.dup_probes;
    progress.refusals_not_draggable = progress.not_draggable_probes;
    // The lossless 1:1 storm identity (deterministic expectations).
    assert_eq!(
        progress.begins,
        SETTLE_PER_STORM + CANCELS_PER_STORM,
        "1:1 begins == settles + cancels (every begun interaction completed, REAL)"
    );
    assert_eq!(
        progress.settles, SETTLE_PER_STORM,
        "every settle route completed its begin"
    );
    assert_eq!(
        progress.cancels, CANCELS_PER_STORM,
        "every cancel route completed its begin"
    );
    assert_eq!(
        machine.previews_emitted, 0,
        "the storm emits no previews (the ghost path is not this storm's load; INT-3)"
    );
    if print_evolution {
        println!(
            "[drag_storm_real] REAL storm wall (calibration pass): {} ns | REAL counters agree (1:1 begins==settles+cancels) | every completion drained the park first",
            storm_start.elapsed().as_nanos()
        );
    }
    black_box(&session);
    progress
}

fn scenario_drag_storm_real(replica: &ScenarioRecord) -> ScenarioRecord {
    let scenario = "drag_storm_real";
    println!();
    println!(
        "=== scenario: {scenario} -- {SETTLE_PER_STORM} begin/settle pairs + {CANCELS_PER_STORM} cancels through ONE REAL DragSession (REAL host registry/machines) ==="
    );

    // The CALIBRATION pass (untimed): full REAL storm with the counter-
    // evolution printouts + every lossless assertion live.
    let calibrated = real_drag_storm(true);

    // The TIMED storms (assertions still live: a defect aborts the run).
    let samples = timed_ns(SAMPLES_STORM_INTERACT, 1, || {
        let p = real_drag_storm(false);
        black_box(&p);
    });
    let wall = WallPercentiles::from_samples(&samples);
    println!(
        "[drag_storm_real] REAL storm pct ({SAMPLES_STORM_INTERACT} x {INTERACTIONS} interactions): p50 {:.0} ns | p95 {:.0} ns | p99 {:.0} ns | min {:.0} | max {:.0} (~ {:.0} ns/interaction)",
        wall.p50_ns,
        wall.p95_ns,
        wall.p99_ns,
        wall.min_ns,
        wall.max_ns,
        wall.p50_ns / INTERACTIONS as f64
    );
    // The side-by-side (same storm shape; the replica evidence kept).
    println!(
        "[drag_storm_real] side-by-side (same storm shape): settle_cancel_storm REPLICA p50 {:.0} ns | REAL rail p50 {:.0} ns | ratio real/replica {:.2} ({SAMPLES_STORM_INTERACT} storms x {INTERACTIONS} interactions each)",
        replica.wall_ns.p50_ns, wall.p50_ns, wall.p50_ns / replica.wall_ns.p50_ns
    );
    // The REAL counter evolution (one storm, before -> after; the dual-side
    // sync above asserted offered == machine).
    println!(
        "[drag_storm_real] counter evolution (REAL DragCounters, before -> after one storm): begins 0 -> {} | settles 0 -> {} | cancels 0 -> {} | moves_parked 0 -> {} | moved_delivered counted as moves_delivered 0 -> {} | refusals: duplicate_begin 0 -> {}, not_draggable 0 -> {} | previews_emitted 0 -> {} (no ghosts; the storm drives no preview path, INT-3 spec-only is elsewhere)",
        calibrated.begins,
        calibrated.settles,
        calibrated.cancels,
        calibrated.moves_parked,
        calibrated.moves_delivered,
        calibrated.refusals_duplicate,
        calibrated.refusals_not_draggable,
        0
    );

    ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: "REAL host registry/machines: DragSession (with_draggables) instance, drives begin/move_drag/take_deliverable_move/release_delivered_move/settle/cancel + the REAL DragCounters scroll + the REAL DRAG_THRESHOLD_PX, driven through the #[doc(hidden)] bench seam (SCN-024; ADR-007 item 3; never wire; NOT the host's documentary API)".to_owned(),
        deferred: vec![
            "the preview/ghost path (INT-3) at storm load — this storm drives NO preview (previews_emitted stays 0); the machine's own test previews_never_mutate_machines_or_events pins the frozen semantic counters".to_owned(),
            "the app-side rail router (app/update/widget_drag: begin->preview paths, the settle->WidgetEvent conversion): app-module scope, not bench-reachable".to_owned(),
        ],
        params: vec![
            param("interactions_per_storm", INTERACTIONS as u64),
            param("settle_pairs", SETTLE_PER_STORM as u64),
            param("cancels", CANCELS_PER_STORM as u64),
            param("threshold_px", DRAG_THRESHOLD_PX),
            param("samples", SAMPLES_STORM_INTERACT),
            param("parks_per_drip_settle", (1 + PARKS_DRIP) as u64),
            param("parks_per_churn_settle", (1 + PARKS_CHURN) as u64),
            param("parks_per_cancel", (1 + PARKS_CANCEL) as u64),
            param("expected_parks_per_storm", calibrated.moves_parked as u64),
            param("expected_delivered_per_storm", calibrated.moves_delivered as u64),
        ],
        samples_n: SAMPLES_STORM_INTERACT,
        wall_ns: wall,
        extra_metrics: Vec::new(),
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![
            measured_real_counter(
                "begins",
                calibrated.begins as u64,
                "DragCounters::begins (1:1 REAL storm)",
            ),
            measured_real_counter(
                "settles",
                calibrated.settles as u64,
                "DragCounters::settles (REAL)",
            ),
            measured_real_counter(
                "cancels",
                calibrated.cancels as u64,
                "DragCounters::cancels (REAL)",
            ),
            measured_real_counter(
                "moves_parked",
                calibrated.moves_parked as u64,
                "DragCounters::moves_parked (every accepted park, overwrites included, REAL)",
            ),
            measured_real_counter(
                "moves_delivered",
                calibrated.moves_delivered as u64,
                "DragCounters::moves_delivered (per-tick takes + completion drains, REAL)",
            ),
            measured_real_counter(
                "refusals_duplicate_begin",
                calibrated.refusals_duplicate as u64,
                "DragCounters::refusals_duplicate_begin (REAL)",
            ),
            measured_real_counter(
                "refusals_not_draggable",
                calibrated.refusals_not_draggable as u64,
                "DragCounters::refusals_not_draggable (the REAL synced-def re-check)",
            ),
            measured_real_counter(
                "previews_emitted",
                0,
                "DragCounters::previews_emitted (storm emits no ghosts)",
            ),
        ],
        changed_elements: None,
        total_elements: None,
        changed_less_than_total: None,
        sink: format!(
            "lossless storm identity asserted per REAL storm: begins == settles + cancels == 1:1 ({}+{}); every completion's end == the interaction's NEWEST accepted point (REAL drain-first; the release argument is a deliberate lie that must be ignored); every completion leaves the park slot empty (take == none) and treats later moves as noise; exactly-once refusals WITHOUT REAL counter movement; the REAL DragCounters scroll printed at storm milestones",
            SETTLE_PER_STORM, CANCELS_PER_STORM
        ),
        notes: vec![
            "REAL row (SCN-024): the wall-clock + DragCounters measure the REAL DragSession machines through the #[doc(hidden)] bench seams (ADR-007 item 3); the replica row it supersedes for wall-clock is settle_cancel_storm (replica evidence kept in pointer-burst-r0.log and in this bench's rows)".to_owned(),
            "per-storm shape identical to the replica's (same interleave, same park pacing, same threshold-exact crossing) so the side-by-side columns are meaningful".to_owned(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Scenario r1-3: keyboard_nav_burst — the r0 SKIPPED row, now REAL
// ---------------------------------------------------------------------------

/// The REAL keyboard burst (SCN-024): the r0 SKIPPED row, now real. The
/// drive shape is the pure-layer contract's (the app fn `on_mount_key` /
/// `handle_mount_key_owned` sits behind the private crate::app::update
/// module chain and is NOT bench-reachable; the bench drives the SAME
/// three in-crate calls it makes, in the same order — see real_key_step):
/// - the main TAB wrap population: KEY_STEPS_TAB_BURST Tab keys per burst;
///   each key resolves exactly ONE FocusMoved and lands the ring seat
///   i % ANCHOR_ROWS (all ANCHOR_ROWS rows enabled, ring order == flat
///   order, Tab wraps by the machine's own or_else at the bottom edge —
///   the seat model is exact for every i),
/// - the Down/Up sawtooth: the pure newest-wins roving chain. Each
///   productive Arrow key lands the seat NEXT to the newest previous
///   landing; the edge keys (at the bottom/top) produce ZERO messages and
///   count NOTHING (the machines' clean refusal); the burst ENDS with ONE
///   more ArrowDown landing the newest seat's successor (the
///   newest-wins end probe, per the pure-layer contract),
/// - the single-step population: per-key hop-cost percentiles.
/// The REAL MountCounters::keyboard_nav_steps walks +1 per RESOLVED step
/// (a key whose route produced >=1 mount message): evolution printed
/// before/after and asserted for every leg.
fn scenario_keyboard_nav_burst() -> ScenarioRecord {
    let scenario = "keyboard_nav_burst";
    println!();
    println!(
        "=== scenario: {scenario} -- the r0 SKIPPED row, now REAL (the #[doc(hidden)] bench seams): Tab burst + Down/Up sawtooth through the REAL ring ==="
    );
    println!(
        "[keyboard_nav_burst] r0 cross-reference: SKIPPED in pointer-burst-r0.log (n=0, executed=false, reason pub(crate)-only surface); this r1 row EXECUTES that planned burst on the REAL machines; the skipped record in r0 stays untouched as the superseded one"
    );

    // -- the main TAB wrap population (10000-key bursts) --------------------
    // The UNTIMED calibration burst with per-step asserts: the seat model
    // must hold on a FRESH state machine; the counter walk comes along.
    let tab_outcome = {
        let (mut registry, _) = registered_registry();
        let before = registry.counters_owned();
        let mut route_msgs = 0usize;
        for i in 0..KEY_STEPS_TAB_BURST {
            let msgs = real_key_step(
                &mut registry,
                Key::Named(keyboard_key::Named::Tab),
                Modifiers::empty(),
            );
            assert_eq!(msgs.len(), 1, "a Tab step resolves exactly ONE route message");
            match &msgs[0] {
                WidgetMountMsg::FocusMoved { to: Some(row), .. } => {
                    // The seat model: all 64 rows enabled, ring order ==
                    // flat order, Tab wraps (focus_next's or_else) — step
                    // i lands seat i, ALWAYS (newest-wins at every wrap
                    // boundary).
                    assert_eq!(
                        row.0,
                        TREE_ROW_BASE + (i % ANCHOR_ROWS) as u64,
                        "Tab step {i} landed the model's seat (exact wrap)"
                    );
                }
                other => panic!("Tab step {i} landed {other:?}, not a FocusMoved"),
            }
            route_msgs += msgs.len();
        }
        let after = registry.counters_owned();
        let delta = after.keyboard_nav_steps - before.keyboard_nav_steps;
        assert_eq!(
            delta, KEY_STEPS_TAB_BURST as u64,
            "every RESOLVED step counts exactly itself (keyboard_nav_steps)"
        );
        assert_eq!(
            route_msgs, KEY_STEPS_TAB_BURST,
            "route messages == steps (one FocusMoved per Tab key)"
        );
        println!(
            "[keyboard_nav_burst] REAL counter evolution (Tab burst, before -> after): keyboard_nav_steps {} -> {} (delta {delta} == every step resolved; the REAL walk counted by the route itself)",
            before.keyboard_nav_steps, after.keyboard_nav_steps
        );
        (route_msgs, delta)
    };

    // TIMED: same-shape bursts with the full per-step assert.
    let samples = timed_ns(SAMPLES_KEY_TAB_BURST, WARMUP, || {
        let (mut registry, _) = registered_registry();
        let before = registry.counters_owned();
        let mut route_msgs = 0usize;
        for i in 0..KEY_STEPS_TAB_BURST {
            let msgs = real_key_step(
                &mut registry,
                Key::Named(keyboard_key::Named::Tab),
                Modifiers::empty(),
            );
            assert_eq!(msgs.len(), 1, "a Tab step resolves exactly ONE route message");
            assert!(matches!(
                &msgs[0],
                WidgetMountMsg::FocusMoved { to: Some(row), .. }
                    if row.0 == TREE_ROW_BASE + (i % ANCHOR_ROWS) as u64
            ));
            route_msgs += msgs.len();
        }
        let after = registry.counters_owned();
        assert_eq!(
            after.keyboard_nav_steps - before.keyboard_nav_steps,
            KEY_STEPS_TAB_BURST as u64
        );
        assert_eq!(route_msgs, KEY_STEPS_TAB_BURST);
        black_box(&after);
    });
    let wall = WallPercentiles::from_samples(&samples);

    // -- the Down/Up sawtooth (the newest-wins roving chain) ----------------
    // Per cycle (64 Downs then 64 Ups): productive Downs land seat+1
    // (fresh: start=0 => land 0..63), the 64th Down at the BOTTOM edge is
    // REFUSED (focus_next's end: no enabled row after the last -> None,
    // ZERO messages, the counter does NOT move); Ups mirror (the last one,
    // at the ring's TOP seat, is refused); the FINAL end probe: ONE more
    // ArrowDown from the settled top seat lands its successor — the
    // newest-wins end check (per the pure-layer contract).
    let sawtooth_wall = timed_ns(SAMPLES_KEY_SAWTOOTH, 2, || {
        let (mut registry, _) = registered_registry();
        let before = registry.counters_owned();
        let mut seat: Option<isize> = None;
        let mut productive = 0usize;
        for k in 0..KEY_SAWTOOTH_STEPS {
            let is_down = k % (2 * ANCHOR_ROWS) < ANCHOR_ROWS;
            let key = if is_down {
                Key::Named(keyboard_key::Named::ArrowDown)
            } else {
                Key::Named(keyboard_key::Named::ArrowUp)
            };
            let msgs = real_key_step(&mut registry, key, Modifiers::empty());
            let expected: isize = match (seat, is_down) {
                (Some(s), true) => s + 1,
                (Some(s), false) => s - 1,
                (None, true) => 0,
                (None, false) => panic!("no up before any landing"),
            };
            let refused = if is_down {
                expected > (ANCHOR_ROWS - 1) as isize
            } else {
                expected < 0
            };
            if refused {
                assert!(
                    msgs.is_empty(),
                    "an edge-refusing key produces NO route message"
                );
                assert!(seat.is_some(), "refusal keeps the seat (the pure-layer note)");
            } else {
                assert_eq!(
                    msgs.len(),
                    2,
                    "an Arrow step resolves FocusMoved + SelectionChanged"
                );
                let landed_ok = match (&msgs[0], &msgs[1]) {
                    (
                        WidgetMountMsg::FocusMoved { to: Some(row), .. },
                        WidgetMountMsg::SelectionChanged { selected, .. },
                    ) => {
                        // Selection follows the landed row, replacing it
                        // (the arrows' list-cursor semantics).
                        row.0 == TREE_ROW_BASE + expected as u64
                            && selected.len() == 1
                            && selected[0].0 == TREE_ROW_BASE + expected as u64
                    }
                    _ => false,
                };
                assert!(
                    landed_ok,
                    "Arrow step {k} landed row {expected} (the newest-wins roving chain)"
                );
                seat = Some(expected);
                productive += 1;
            }
        }
        // The newest-wins END probe (the pure-layer contract): the burst
        // settles seat 0 and ONE more ArrowDown lands EXACTLY its successor.
        assert_eq!(seat, Some(0), "the sawtooth settles the ring's TOP seat");
        let msgs = real_key_step(
            &mut registry,
            Key::Named(keyboard_key::Named::ArrowDown),
            Modifiers::empty(),
        );
        assert_eq!(msgs.len(), 2, "the end probe step is a productive Arrow key");
        let probe_landed_ok = match &msgs[0] {
            WidgetMountMsg::FocusMoved { to: Some(row), .. } => row.0 == TREE_ROW_BASE + 1,
            _ => false,
        };
        assert!(
            probe_landed_ok,
            "the end probe lands the NEWEST seat's successor (newest-wins, even at 1024 keys)"
        );
        let after = registry.counters_owned();
        assert_eq!(
            after.keyboard_nav_steps - before.keyboard_nav_steps,
            (productive + 1) as u64,
            "the REAL counter counts exactly the RESOLVED steps (refusals move nothing, the end probe counts)"
        );
        black_box(&productive);
    });
    let sawtooth_wall = WallPercentiles::from_samples(&sawtooth_wall);
    println!(
        "[keyboard_nav_burst] sawtooth ({SAMPLES_KEY_SAWTOOTH} x {KEY_SAWTOOTH_STEPS} Arrow keys, newest-wins chain asserted per key, edge refusals expected): p50 {:.0} ns | p99 {:.0} ns (the counters count ONLY the resolved keys)",
        sawtooth_wall.p50_ns, sawtooth_wall.p99_ns
    );

    // -- the single-step population (per-hop percentiles) -------------------
    let (mut step_registry, _) = registered_registry();
    let mut seat0: usize = 0;
    let step_wall = timed_ns(SAMPLES_KEY_STEP, 5, || {
        let msgs = real_key_step(
            &mut step_registry,
            Key::Named(keyboard_key::Named::Tab),
            Modifiers::empty(),
        );
        assert_eq!(msgs.len(), 1, "a Tab step resolves exactly ONE route message");
        assert!(matches!(
            &msgs[0],
            WidgetMountMsg::FocusMoved { to: Some(row), .. }
                if row.0 == TREE_ROW_BASE + (seat0 % ANCHOR_ROWS) as u64
        ));
        seat0 += 1;
    });
    let step_wall = WallPercentiles::from_samples(&step_wall);
    println!(
        "[keyboard_nav_burst] single Tab step pct ({SAMPLES_KEY_STEP} sequential steps on one persisted registry, wrap seat asserted per step): p50 {:.0} ns | p95 {:.0} ns | p99 {:.0} ns | min {:.0} | max {:.0}",
        step_wall.p50_ns, step_wall.p95_ns, step_wall.p99_ns, step_wall.min_ns, step_wall.max_ns
    );

    // MAIN wall print.
    println!(
        "[keyboard_nav_burst] Tab burst pct ({SAMPLES_KEY_TAB_BURST} x {KEY_STEPS_TAB_BURST} steps): p50 {:.0} ns | p95 {:.0} ns | p99 {:.0} ns | min {:.0} | max {:.0} (per-step mean ~ {:.0} ns)",
        wall.p50_ns,
        wall.p95_ns,
        wall.p99_ns,
        wall.min_ns,
        wall.max_ns,
        wall.p50_ns / KEY_STEPS_TAB_BURST as f64
    );
    // The r0 -> r1 deferred-map movement prints here (bench stdout + the
    // run record's counters below).
    println!(
        "[keyboard_nav_burst] deferred-map movement: keyboard_nav_steps deferred (r0) -> measured-real (r1): the counter walk is 0 -> per-burst delta, all REAL"
    );

    ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: "REAL host registry/machines: the app's own keyboard route handle_mount_key_owned over the registered anchored instance (its key_route_owned core walks the nav_ring ring + focus_next/focus_prev roving and record_key_nav_step_owned ticks MountCounters::keyboard_nav_steps), target resolution through set_mount_key_target_owned / mount_key_surface_owned, driven through the #[doc(hidden)] bench seams (SCN-024; ADR-007 item 3; never wire; NOT the host's documentary API)".to_owned(),
        deferred: vec![
            "the typed-TABLE cell/row roving burst (mount_table_key_route, the SWG-004b table scope): same pure machinery, headless-tested, not timed here".to_owned(),
            "Enter/Space ACTIVATION + Escape's machine-direct prompt cancel at burst load (activation arms commands; Escape rides the dialog cancel edge): neither is this row's pure-roving load".to_owned(),
        ],
        params: vec![
            param("tab_steps_per_burst", KEY_STEPS_TAB_BURST),
            param("samples", SAMPLES_KEY_TAB_BURST),
            param("ring_rows", ANCHOR_ROWS),
            param("all_rows_enabled", true),
            param("sawtooth_steps_per_sample", KEY_SAWTOOTH_STEPS),
            param("sawtooth_samples", SAMPLES_KEY_SAWTOOTH),
            param("single_step_population", SAMPLES_KEY_STEP),
            param(
                "target_set_first",
                "set_mount_key_target_owned(Some(mount_id))"
            ),
        ],
        samples_n: SAMPLES_KEY_TAB_BURST,
        wall_ns: wall,
        extra_metrics: vec![
            ExtraMetric {
                label: "arrow_sawtooth_1024".to_owned(),
                samples_n: SAMPLES_KEY_SAWTOOTH,
                wall_ns: sawtooth_wall,
                payload_bytes: None,
                note: Some(format!(
                    "the {}-key Down/Up sawtooth per sample (a fresh REAL state machine per sample): every productive key lands the NEWEST seat +-1, edge keys produce ZERO messages + move NO counters (machine refusal), the final probe lands the newest seat's successor: the REAL newest-wins roving chain",
                    KEY_SAWTOOTH_STEPS
                )),
            },
            ExtraMetric {
                label: "single_tab_step".to_owned(),
                samples_n: SAMPLES_KEY_STEP,
                wall_ns: step_wall,
                payload_bytes: None,
                note: Some(format!(
                    "per-key hop cost on ONE persistent registry ({} keys = 15 full ring wraps + change); the seat model asserted per step",
                    SAMPLES_KEY_STEP
                )),
            },
        ],
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![
            measured_real_counter(
                "keyboard_nav_steps",
                tab_outcome.1,
                "REAL MountCounters::keyboard_nav_steps walk across ONE 10000-step Tab burst (every resolved step counts exactly itself; refusals/edges move nothing)",
            ),
            measured_real_counter(
                "tab_route_messages",
                tab_outcome.0 as u64,
                "route messages the REAL route produced for the Tab burst (exactly one per burst step)",
            ),
        ],
        changed_elements: None,
        total_elements: None,
        changed_less_than_total: None,
        sink: format!(
            "per-step contract on the REAL machines' route output: every Tab key resolves exactly ONE FocusMoved landing seat i % {} (the wrap model exact); the sawtooth's productive Arrow keys land the newest seat +-1 (FocusMoved + SelectionChanged of the SAME row), edge keys produce ZERO messages and move NO counter; the bursts end with the REAL keyboard_nav_steps delta == the step count; the newest seat's successor probed at the end: a stale/lost seat, an edge message or an uncounted resolution panics the bench",
            ANCHOR_ROWS
        ),
        notes: vec![
            "REAL row (SCN-024): keyboard_nav_burst was SKIPPED in r0 (the honest byte rule kept it out with the deferred map); the seams landed (ADR-007 item 3) make this the FIRST wall-clock measurement of the REAL ring; the skipped row's record stays in pointer-burst-r0.log untouched".to_owned(),
            "the pure-layer contract's drive shape (mount_key_surface_owned -> handle_for_surface_owned -> key_route_owned) is identical to the app's on_mount_key arm; its per-step cost also includes the per-view views snapshot clone + the ring rebuild (production pays the same per key)".to_owned(),
        ],
    }
}

// ---------------------------------------------------------------------------
// The SCN-022 run-record emitter (support pieces; task label SCN-022)
// ---------------------------------------------------------------------------

/// Print the env lines, assemble the run record with the SCN-024 task
/// label, and print the pretty JSON (the stdout JSON IS the file). Reuses
/// the support's `env_block`/`RunRecord` unchanged; only the task label
/// differs from the r0 emitter (SCN-022's, pointer-burst-r0.log).
fn emit_scn024_run_record(bench: &str, scenarios: Vec<ScenarioRecord>) -> RunRecord {
    let env: EnvBlock = env_block(bench);
    println!(
        "\n=== SCN-024 run: {bench} (the r0 replica rows + the r1 REAL rows through the \
         #[doc(hidden)] bench seams; the run record's deferred map for the previously- \
         deferred items moves them to measured-real) ==="
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
        task: "SCN-024",
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
    println!("=== SCN-024 POINTER-BURST interaction bench (widget_interaction) — r1 of SCN-022's pointer-burst-r0 ===");
    println!("HONEST-BYTE SPLIT (all halves stated before any number):");
    println!(
        "- r0 (kept, see pointer-burst-r0.log): the CONTRACT-SHAPED REPLICA rows: the \
         pub(crate) pointer machines were bench-invisible at that branch tip \
         (DragSession/DragCounters/DragPhase, the MountInputState coalescer, MountCounters), \
         so their wall-clock layer was the bench-side field-for-field mirrors over pub \
         ocs_plugin_api types; those rows STILL RUN below, each `surface` labeled \
         superseded-by-real"
    );
    println!(
        "- r1 (SCN-024, ADR-007 item 3): the REAL rows run the host's own #[doc(hidden)] pub \
         bench seams: a bench-local MountRegistry::default() (the INSTANCE world, since the \
         cfg(test) static facade is bench-invisible) with the anchored surface registered via \
         SurfaceMountHandle::from_surface + register_surface_owned, the REAL *_owned drive/ \
         read paths (mount_move_defer/__flush_moves/__press/__pointer_released/__hover_state_ \
         ___owned/counters_owned + key_route_owned) and the REAL DragSession/DragCounters + \
         DRAG_THRESHOLD_PX; inspect/drive-only, never wire"
    );
    println!(
        "- the run record's deferred map moves the previously-deferred items (keyboard_nav_steps, \
         MountCounters mount-move probes, the DragCounters scroll) to scope measured-real; \
         the replica rows' remaining pub(crate) halves stay deferred counters"
    );

    // The anchored fixture (validated; its row ids ARE the draggable set).
    let anchor = anchored_surface();
    black_box(&anchor_addresses(&anchor));
    println!(
        "anchor surface: validated (1 tree × {} enabled rows, revision {}; surface ids \
         in the 20_xxx row namespace; rail key {})",
        ANCHOR_ROWS, ANCHOR_REVISION, PANEL_SURFACE_ID
    );

    // The replica rows run FIRST (kept evidence); each REAL row follows the
    // replica row it supersedes and prints the side-by-side columns.
    let park = scenario_park_storm();
    let drain = scenario_drain_latest_only();
    let coalesce_real = scenario_mount_move_coalesce_real(&park, &drain);
    let replica_storm = scenario_settle_cancel_storm();
    let drag_real = scenario_drag_storm_real(&replica_storm);
    let keyboard_real = scenario_keyboard_nav_burst();

    let scenarios = vec![park, drain, coalesce_real, replica_storm, drag_real, keyboard_real];

    let record = emit_scn024_run_record("widget_interaction", scenarios);
    black_box(&record);
    // Bench convention: honest exit (0) — panics on any contract defect.
}
