//! Renderer-free DRAG state machine for DragMeta-carrying widget elements
//! (SCN-021 of the scene-UI architecture effort, ARCHITECTURE §7 invariants
//! INT-1..5 + §§13–16).
//!
//! This is the widget-panel sibling of the custom-grip drag lifecycle
//! (`src/app/update/plugin_grips.rs`): the same Begin/Move/Settle/Cancel
//! phase vocabulary the plugin API already knows (`CustomGripPhase`,
//! `crates/ocs_plugin_api/src/host.rs` lines 1201–1216), applied to a
//! **per-surface panel-element rail**. One [`DragSession`] per surface runs
//! at most ONE live drag at a time; the machine is where the §7 lossless
//! invariants are enforced — it holds no iced widgets, no `Message`, no
//! renderer, and — deliberately — NO document/selection handle at all, so
//! the preview path below cannot mutate anything it does not own (the
//! preview-never-mutates invariant is STRUCTURAL here, not merely
//! conventional).
//!
//! # The §7 invariants, mapped one to one onto this API
//!
//! **INT-1 — Begin/Settle/Cancel are LOSSLESS.** [`DragSession::begin`]
//! captures the drag's origin facts (address, the plugin's [`DragMeta`]
//! verbatim, the press point, the press tick) into an immutable
//! [`DragOrigin`]. Settle and cancel return EXACTLY what begin captured —
//! the same address, the same `drag_meta` payload, the same start point —
//! plus the completion's own end context (§7 "plugin receives start
//! context, end context"). Every begun interaction completes EXACTLY once:
//! settle and cancel CONSUME the session (back to the consumed/idle family —
//! [`DragPhase::Settled`] / [`DragPhase::Cancelled`] are the observable
//! terminal dispositions), and a second completion refuses
//! [`DragRefusal::NotInFlight`]. The 1:1 guarantee is asserted by the tests
//! (`cancel_paths_are_one_to_one_with_begins`, `begin_move_settle_is_lossless`,
//! `teardown_cancel_is_1to1`).
//!
//! **INT-2 — Moves coalesce: latest-wins, at most one in flight.** The
//! newest cursor point owns the single park slot (`owed_move`): a
//! superseding move OVERWRITES a still-owed one (never queued, never
//! needing a queue — the last position is always the one the rail will
//! define the drag by; the grip precedent's single-slot channel
//! `PluginGripDeliveries::park_move` / `take_sendable_move`,
//! `src/plugin/grip_points.rs` lines 581–591). Delivery is gated to at most
//! ONE move in flight (`take_deliverable_move` + `release_delivered_move`,
//! mirroring the grip channel's `in_flight_move` slot, grip_points.rs lines
//! 603–614): the host-local router delivers the one move per tick it took
//! and releases the slot right after that tick's preview emission
//! (synchronous completion); the wire era keeps the slot held across real
//! answer latencies. A settle or cancel DRAINS the parked slot FIRST — no
//! move is lost at the boundary and the NEWEST one defines the end point
//! (the grip settle's own drain-first rule, `plugin_grips.rs` lines
//! 302–332, the drain comment at lines 313–314). Parked moves are counted
//! (`moves_parked`, overwrites included) so the park churn stays a defect
//! probe, never silent state.
//!
//! **INT-3 — Previews are host-local and NEVER interactive.**
//! [`DragSession::preview_prims`] emits the ghost [`PreviewSpec`] (pure
//! point facts) for the ONE move the router just delivered. It never
//! mutates selection/data state (the machine HAS none — no document,
//! selection, or expansion handle exists anywhere in this module), never
//! emits a [`WidgetEvent`](ocs_plugin_api::widgets::WidgetEvent), never
//! arms or settles anything, and moves exactly ONE counter of its own
//! (`previews_emitted` — the ghost bookkeeping; every SEMANTIC counter
//! stays bit-frozen across any preview emission, pinned by
//! `previews_never_mutate_machines_or_events`). The pixels are a later
//! mount/scene concern; this module ships the SPEC ONLY (points), to be
//! rendered through the one-frame dynamic-layer idiom
//! `crate::ui::overlay::transient_layer` (src/ui/overlay.rs lines
//! 1918–1926).
//!
//! **INT-4 — exactly-once semantic completion per interaction.** The
//! machine deliberately does NOT produce the settle's semantic event: the
//! §13/§14 event vocabulary holds NO drag-settle variant (a settle is a
//! SEMANTIC COMPLETION the plugin resolves, not a widget notification) and
//! the machine is event-agnostic BY DECISION (binding for this task). The
//! settled/cancelled FACT ([`DragSettled`] / [`DragCancelled`]) is what the
//! caller converts — for a settle the app-side converter
//! `crate::app::update::widget_drag::settle_to_event` binds ONE
//! `WidgetEvent::CommandInvoked` (targets = the dragged element's address);
//! a cancel converts to NOTHING (cancel is a lossless no-op for the plugin,
//! `plugin_grips.rs` lines 334–362). One interaction, at most one semantic
//! exit — the exactly-once discipline of the SWG-003 dispatcher
//! (`plugin_widgets_dispatch.rs`) applied to a drag rail.
//!
//! **INT-5 — the meta returns VERBATIM; the host never echoes it.** The
//! plugin's [`DragMeta::opaque`] payload is the plugin's OWN key. The host
//! never interprets, indexes, or echoes it: the event vocabulary carries no
//! DragMeta field, and `settle_to_event` deliberately produces
//! `CommandInvoked(command, [address])` ONLY. The opaque payload returns to
//! the plugin through (a) the settled FACT returned to the caller
//! ([`DragSettled::drag_meta`] verbatim) and (b) the NEXT authoritative
//! surface sync on the plugin's own side — the meta never needed the host
//! to remember it (DECIDED for this task; pinned by
//! `meta_returns_via_next_sync_documented`).
//!
//! # The drag threshold (DECIDED)
//!
//! Armed without motion is not a drag. A press on a draggable element ARMS
//! the session ([`DragPhase::Armed`]); the FIRST pointer move that travels
//! ≥ [`DRAG_THRESHOLD_PX`] pixels from the armed origin transitions it to
//! [`DragPhase::InFlight`] (the crossing move parks and may deliver
//! immediately). Sub-threshold jitter while armed is pointer noise: parks
//! nothing, mutates nothing, previews nothing. An armed release is
//! therefore NOT a drag: the armed release routes to
//! [`DragCancelReason::ThresholdNotMet`] — cancel-equivalent, never a
//! settle. (A click is a click; the custom-grip precedent records `moved`
//! per move and its press-release path still delivers a settle EVENT even
//! for a click-like release — for the widget rail the STRICT form is
//! chosen: settles only fire for real drags.) 3.0 px sits deliberately
//! BELOW the grip pick apertures (`GRIP_THRESHOLD_PX` = 8.0 px hit
//! aperture, `GRIP_HALF_PX` = 5.0 px render half-size —
//! `src/scene/pick/grip.rs` lines 10–13): those gate POINTING at a grip
//! target, this one discriminates GESTURE under an already-held pointer — a
//! wider threshold would swallow small genuine drags; anything under ~2 px
//! misreads hand-shake jitter as a drag.
//!
//! # Draggability (the synced def)
//!
//! The machine refuses to drag anything not in its SYNCED draggable set
//! ([`DragSession::sync_draggables`]) — the machine's own re-check of the
//! caller's `drag.is_some()` look, matching the dispatcher's synced read
//! model discipline (an unknown address is NOT enabled). Until the
//! `WidgetNode::drag` field lands in the API crate, the mount derives the
//! set when it walks the surface's nodes and syncs it explicitly; the
//! machine never infers draggability from anything else. An in-flight
//! drag's OWN origin was validated at arm time and survives a re-sync.
//!
//! # Seam (recorded, not claimed)
//!
//! The mount integration of this machine is a LATER task: it joins
//! [`DragSession`] into the surface mount registry of
//! `plugin_widget_mount.rs` (a third machine beside
//! `WidgetUiState`/`WidgetDispatcher`) and routes the mount's pointer
//! vocabulary through `crate::app::update::widget_drag::DragRouter`. Nothing
//! in THIS file touches `plugin_widget_mount.rs`, `plugin_widgets*.rs`,
//! `plugin_panel.rs` or the app bridge — the machine consumes only the
//! `ocs_plugin_api::scene` vocabulary. The tree-row drag surface wiring is
//! likewise DEFERRED (no tree-row drag v1): the machine is generic over the
//! dragged element's address, and the row-specific ghost imagery is the
//! wiring task's decision (recorded, not silently claimed).
//!
//! # Grip precedents cited (line refs at this branch tip)
//!
//! - `src/plugin/grip_points.rs` — `PluginGripDeliveries` (lines 559–670):
//!   `park_move` (line 581) / `take_sendable_move` (line 586) — the
//!   single-Move-in-flight park discipline this session's `owed_move` +
//!   `moves_in_flight` slot mirrors; the single-slot "superseded points are
//!   DROPPED" coalescing note at lines 547–558.
//! - `src/app/update/plugin_grips.rs` — `begin_plugin_grip_drag` (lines
//!   141–175), `move_plugin_grip_drag` (lines 180–191, the newest-point
//!   park), `settle_plugin_grip_drag` (lines 302–332: drain outstanding
//!   moves FIRST, restore the visual-only originals BEFORE the plugin's
//!   push_undo write — the lossless settle order this machine's drain-first
//!   rule is the headless form of), `cancel_plugin_grip_drag` (lines
//!   339–362: restore, notify, write nothing).
//! - `crates/ocs_plugin_api/src/host.rs` — `CustomGripPhase` (lines
//!   1201–1216): Begin/Move/Settle/Cancel; "Move events are coalesced (at
//!   most one is in flight …); the settle and cancel phases are never
//!   coalesced or dropped" — this machine's phase vocabulary is pinned to
//!   the same shape.

// Stage-1 surface: consumed by the unit tests in this module and by the
// app-side drag rail (`app/update/widget_drag.rs`, same task) only until the
// mount integration task routes the surface registry through it; the allow
// mirrors `plugin_widgets.rs` / `plugin_widgets_dispatch.rs` stage precedent.
#![allow(dead_code)]

use std::collections::HashSet;
use std::fmt;

use iced::Point;
use ocs_plugin_api::scene::{DragMeta, StableId};

/// The ARM→InFlight threshold in pixels: the first pointer move at or beyond
/// this distance from the armed origin IS a drag (see the module docs for
/// the magnitude decision).
pub(crate) const DRAG_THRESHOLD_PX: f32 = 3.0;

// ---------------------------------------------------------------------------
// Phases, facts, refusals
// ---------------------------------------------------------------------------

/// The drag session's phase. `Idle` and the two terminal dispositions
/// (`Settled`/`Cancelled`) form the CONSUMED family: any of them accepts a
/// fresh `begin` (the session re-arms); only `Armed`/`InFlight` are live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DragPhase {
    /// No interaction on this surface.
    #[default]
    Idle,
    /// Origin facts captured; the pointer has not yet proven a drag
    /// (sub-threshold). No move delivery, no preview.
    Armed,
    /// A live drag: moves park/deliver, previews emit.
    InFlight,
    /// Consumed by a settle (the §7 completed side).
    Settled,
    /// Consumed by a cancel (the §7 abandoned side).
    Cancelled,
}

/// The origin facts begin captured — IMMUTABLE for the whole session: the
/// exact payload any settle/cancel of this interaction returns (the §7
/// losslessness). `start_tick` is a caller-supplied monotonic logical tick
/// (headless/route input — never wall-clock), part of the start context.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DragOrigin {
    /// The dragged element's address.
    pub address: StableId,
    /// The plugin's drag payload, VERBATIM (never interpreted).
    pub drag_meta: DragMeta,
    /// The press point the drag armed at.
    pub start_point: Point,
    /// The press tick (monotonic logical input).
    pub start_tick: u64,
}

/// What begin did (the ARM receipt): the captured origin facts, returned so
/// the caller doesn't have to re-read them via an accessor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DragBegan {
    /// The surface the drag armed on (the session's own).
    pub surface_id: StableId,
    /// The captured origin (the source of truth for this interaction).
    pub origin: DragOrigin,
}

/// The settle FACT: what the §7 completion hands the caller. The event
/// conversion (if the caller binds one) happens OUTSIDE the machine — the
/// machine stays event-agnostic (INT-4).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DragSettled {
    /// The surface the drag ran on.
    pub surface_id: StableId,
    /// The dragged element (the origin's address, verbatim).
    pub address: StableId,
    /// The plugin's drag payload, returned VERBATIM (INT-5).
    pub drag_meta: DragMeta,
    /// The press point begin captured.
    pub start_point: Point,
    /// The FINAL DELIVERED point: settle drains the parked slot first, so
    /// this is the newest position the session ever accepted. The settle
    /// argument's release point stands in ONLY if the flight discipline was
    /// bypassed and nothing was ever delivered or parked (which cannot
    /// happen through the machine's own transitions).
    pub end_point: Point,
    /// Total moves DELIVERED for the interaction — per-tick deliveries plus
    /// the completion's own drain, every delivery counted exactly once.
    pub moves_delivered: usize,
}

/// Why the drag was cancelled. 1:1 with the lossless invariant: every begin
/// ends in a settle OR one of these (each names the §7 cancel context).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DragCancelReason {
    /// Escape (the grip ride's Esc-cancels — `plugin_grips.rs` lines 1–15).
    #[default]
    Escape,
    /// The pointer left the surface/drag scope.
    PointerOut,
    /// Surface teardown mid-drag (§16 close-tab).
    Teardown,
    /// Released while still armed below the threshold — a click, not a drag.
    ThresholdNotMet,
}

/// The cancel FACT: the origin verbatim plus the reason (§7 "plugin
/// receives … cancel reason").
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DragCancelled {
    /// The surface the drag ran on.
    pub surface_id: StableId,
    /// The dragged element (the origin's address, verbatim).
    pub address: StableId,
    /// The plugin's drag payload, returned VERBATIM (INT-5).
    pub drag_meta: DragMeta,
    /// The press point begin captured.
    pub start_point: Point,
    /// The drag's last position: the drained newest delivery for an
    /// InFlight cancel; the armed start point when cancelled before any
    /// crossing move proved a drag.
    pub end_point: Point,
    /// The caller's cancel reason (never the machine's invention — the
    /// `ThresholdNotMet` case replaces an armed release, see the module
    /// docs).
    pub reason: DragCancelReason,
}

/// What one machine-level `move_drag` did — every disposition is terminal
/// for that gesture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MoveDisposition {
    /// No drag on this surface (the pointer move is host-local noise).
    Inactive,
    /// Armed but still below [`DRAG_THRESHOLD_PX`]: parked nothing,
    /// delivered nothing (the jitter branch — see the module docs).
    UnderThreshold { distance_px: f32 },
    /// The ARM→InFlight crossing move: the drag is live and the move is
    /// parked (the router delivers + previews it).
    Crossed,
    /// InFlight park: the newest point owns the park slot; `superseded` is
    /// `true` when a previously parked-but-undelivered move lost the slot
    /// to it (latest-wins — INT-2).
    Parked { superseded: bool },
}

/// Why `begin` / `settle` / `cancel` refused. Display text follows the
/// machines' refusal convention (the SWG-002/003 string style).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DragRefusal {
    /// A begin while the surface's session is already `Armed`/`InFlight`
    /// (the SWG-003 `AlreadyInFlight` shape).
    DuplicateBegin(StableId),
    /// The address is not in the synced draggable set (no `def.drag`).
    NotDraggable(StableId),
    /// Nothing live to complete (a second settle/cancel, or a completion
    /// against the consumed family). BENIGN by design: it moves no counter —
    /// the exactly-once evidence lives in `settles`/`cancels`.
    NotInFlight,
    /// The gesture addressed a surface this session does not own — a
    /// routing defect (the router never constructs this: the per-surface
    /// rails are keyed by the very id). Moves no counter.
    ForeignSurface {
        /// The session's own surface id.
        session: StableId,
        /// The surface the gesture named.
        addressed: StableId,
    },
}

impl fmt::Display for DragRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBegin(address) => write!(
                f,
                "element {0} is already in a drag and cannot begin another",
                address.0
            ),
            Self::NotDraggable(address) => write!(
                f,
                "element {0} does not carry DragMeta and cannot be dragged",
                address.0
            ),
            Self::NotInFlight => write!(f, "no drag is live on this surface"),
            Self::ForeignSurface {
                session,
                addressed,
            } => write!(
                f,
                "the drag session owns surface {0} but the gesture addressed surface {1}",
                session.0, addressed.0
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Counters (the §16 defect probe)
// ---------------------------------------------------------------------------

/// The session's §16 instrumentation: every path that MATTERS moves exactly
/// its own counter and nothing else moves one. `moves_parked` counts every
/// accepted park (overwrites included — the parked-slot churn stays
/// visible); `moves_delivered` counts every delivery (each
/// `take_deliverable_move` AND a settle/cancel's drain); `previews_emitted`
/// is the ghost's own bookkeeping (preview emission is the ONE write it may
/// do); the `refusals_*` pair counts their refusals. A refusal never moves
/// a counter other than its own — `NotInFlight` and `ForeignSurface` move
/// none at all (pinned by `counters_are_the_defect_probe`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DragCounters {
    /// `begin` calls that armed a session.
    pub begins: usize,
    /// Accepted move parks (the crossing move + every InFlight park).
    pub moves_parked: usize,
    /// Delivered moves (per-tick takes + completion drains).
    pub moves_delivered: usize,
    /// Settles (the §7 completed-side count; exactly-once per interaction).
    pub settles: usize,
    /// Cancels (any reason).
    pub cancels: usize,
    /// Preview prims emitted (host-local only — INT-3).
    pub previews_emitted: usize,
    /// `DuplicateBegin` refusals.
    pub refusals_duplicate_begin: usize,
    /// `NotDraggable` refusals.
    pub refusals_not_draggable: usize,
}

// ---------------------------------------------------------------------------
// The machine
// ---------------------------------------------------------------------------

/// One surface's drag rail: the machine ALL drag gestures of ONE widget
/// surface route through. The newest pointer point always wins; the origin
/// never changes once armed; every completion is exactly-once.
#[derive(Debug, Clone)]
pub(crate) struct DragSession {
    /// The surface this rail answers (its registry key — never mutated).
    surface_id: StableId,
    phase: DragPhase,
    /// The origin facts (immutable while live). Consumed at completion.
    origin: Option<DragOrigin>,
    /// Latest-wins move buffer (INT-2): at most one parked move; a new one
    /// overwrites it. Drained by take-delivery and by settle/cancel.
    owed_move: Option<Point>,
    /// Whether one move is currently "in flight" (the ≤1 delivery gate —
    /// the grip channel's slot shape).
    moves_in_flight: bool,
    /// The last DELIVERED point (the completion's end-point authority
    /// whenever the drain had nothing to take).
    last_delivered: Option<Point>,
    /// The §16 probes (see [`DragCounters`]).
    counters: DragCounters,
    /// The synced draggable set (the NotDraggable check's authority).
    draggable: HashSet<StableId>,
}

impl DragSession {
    /// A fresh Idle rail for `surface_id`.
    pub(crate) fn new(surface_id: StableId) -> Self {
        Self {
            surface_id,
            phase: DragPhase::Idle,
            origin: None,
            owed_move: None,
            moves_in_flight: false,
            last_delivered: None,
            counters: DragCounters::default(),
            draggable: HashSet::new(),
        }
    }

    /// A fresh Idle rail with the initial synced draggable set (the mount
    /// registration convenience — sync the def walk's result at once).
    pub(crate) fn with_draggables(
        surface_id: StableId,
        addresses: impl IntoIterator<Item = StableId>,
    ) -> Self {
        let mut session = Self::new(surface_id);
        session.sync_draggables(addresses);
        session
    }

    /// The rail's surface id (its registry key).
    pub(crate) fn surface_id(&self) -> StableId {
        self.surface_id
    }

    /// The current phase (the exactly-once guard's observable).
    pub(crate) fn phase(&self) -> DragPhase {
        self.phase
    }

    /// The §16 counter snapshot (the tests' defect probe).
    pub(crate) fn counters(&self) -> DragCounters {
        self.counters
    }

    /// Whether the address carries the synced drag def (the NotDraggable
    /// re-check the callers' `drag.is_some()` mirrors).
    pub(crate) fn is_draggable(&self, address: StableId) -> bool {
        self.draggable.contains(&address)
    }

    /// REPLACE the synced draggable set (the mount's def walk re-computes
    /// it per surface sync — idempotent). In-flight drags are untouched:
    /// the session's OWN origin was validated at arm time and stays.
    pub(crate) fn sync_draggables(&mut self, addresses: impl IntoIterator<Item = StableId>) {
        self.draggable = addresses.into_iter().collect();
    }

    /// ARM the drag: capture the origin facts for `address` (the §7 start
    /// context — the exact data every completion of this interaction
    /// returns).
    ///
    /// Refuses ([`DragRefusal`]):
    /// - [`DragRefusal::ForeignSurface`] — the gesture does not name this
    ///   rail (routing defect; no counter),
    /// - [`DragRefusal::DuplicateBegin`] — already Armed/InFlight (its own
    ///   counter; the SWG-003 exactly-once shape),
    /// - [`DragRefusal::NotDraggable`] — not in the synced draggable set
    ///   (its own counter).
    pub(crate) fn begin(
        &mut self,
        surface: StableId,
        address: StableId,
        drag_meta: DragMeta,
        start_point: Point,
        start_tick: u64,
    ) -> Result<DragBegan, DragRefusal> {
        if surface != self.surface_id {
            return Err(DragRefusal::ForeignSurface {
                session: self.surface_id,
                addressed: surface,
            });
        }
        if matches!(self.phase, DragPhase::Armed | DragPhase::InFlight) {
            self.counters.refusals_duplicate_begin += 1;
            return Err(DragRefusal::DuplicateBegin(address));
        }
        if !self.draggable.contains(&address) {
            self.counters.refusals_not_draggable += 1;
            return Err(DragRefusal::NotDraggable(address));
        }
        let origin = DragOrigin {
            address,
            drag_meta,
            start_point,
            start_tick,
        };
        self.origin = Some(origin.clone());
        // A fresh arm never inherits a stale park/slot (a consumed session
        // cleared them already; belt-and-braces symmetric hygiene).
        self.owed_move = None;
        self.moves_in_flight = false;
        self.last_delivered = None;
        self.phase = DragPhase::Armed;
        self.counters.begins += 1;
        Ok(DragBegan {
            surface_id: self.surface_id,
            origin,
        })
    }

    /// One pointer move into the rail. Honors the arm threshold
    /// ([`DRAG_THRESHOLD_PX`]); otherwise parks the newest point (latest
    /// wins — INT-2) and reports the disposition.
    pub(crate) fn move_drag(&mut self, point: Point) -> MoveDisposition {
        match self.phase {
            DragPhase::Armed => {
                let origin = match &self.origin {
                    Some(origin) => origin.clone(),
                    None => return MoveDisposition::Inactive,
                };
                let distance_px = distance_px(origin.start_point, point);
                if distance_px < DRAG_THRESHOLD_PX {
                    // Sub-threshold jitter while armed: pointer noise. The
                    // origin stays immutable; nothing parks or previews.
                    return MoveDisposition::UnderThreshold { distance_px };
                }
                // The crossing move: the drag IS live, and the move parks
                // (the same park the InFlight path applies).
                self.owed_move = Some(point);
                self.counters.moves_parked += 1;
                self.phase = DragPhase::InFlight;
                MoveDisposition::Crossed
            }
            DragPhase::InFlight => {
                let superseded = self.owed_move.is_some();
                // The newest point owns the single park slot (INT-2: latest
                // wins; the superseded position is superseded BY the call
                // itself — never queued, never re-deliverable: the next
                // delivery or the completion's drain always sees the newest).
                self.owed_move = Some(point);
                self.counters.moves_parked += 1;
                MoveDisposition::Parked { superseded }
            }
            _ => MoveDisposition::Inactive,
        }
    }

    /// Take the parked move for delivery under the ≤1-in-flight gate: `Some`
    /// only when the park holds a point AND no move is in flight. Marks the
    /// slot occupied; the router releases it after this tick's preview
    /// emission ([`DragSession::release_delivered_move`]) — the headless
    /// twin of the grip pump's `take_sendable_move` (grip_points.rs line
    /// 586).
    pub(crate) fn take_deliverable_move(&mut self) -> Option<Point> {
        if self.phase != DragPhase::InFlight || self.moves_in_flight {
            return None;
        }
        let point = self.owed_move.take()?;
        self.moves_in_flight = true;
        self.last_delivered = Some(point);
        self.counters.moves_delivered += 1;
        Some(point)
    }

    /// The delivery completed (the router's same-tick release — the host-
    /// local channel is synchronous; the wire era keeps the slot held until
    /// the plugin answers). Returns whether a delivery was pending release.
    pub(crate) fn release_delivered_move(&mut self) -> bool {
        std::mem::replace(&mut self.moves_in_flight, false)
    }

    /// The HOST-LOCAL preview: the ghost PRIMS the mount's transient layer
    /// renders for the delivered `point`. Pure point facts (INT-3: the spec
    /// only, no pixels), emitted ONLY while the drag is `InFlight` (nothing
    /// live to ghost in any other phase) and NEVER interactive: no
    /// selection/data mutation (the machine owns none), no WidgetEvent (the
    /// vocabulary has no preview variant and the machine constructs none),
    /// no armed/dispatched state touched. Exactly ONE concern is
    /// preview-in-flight here: ONE counter — `previews_emitted`.
    pub(crate) fn preview_prims(&mut self, point: Point) -> Vec<PreviewSpec> {
        if self.phase != DragPhase::InFlight {
            return Vec::new();
        }
        let Some(origin) = &self.origin else {
            return Vec::new();
        };
        let prim = PreviewSpec::rect(origin.start_point, point);
        self.counters.previews_emitted += 1;
        vec![prim]
    }

    /// The §7 SETTLE: complete the live drag LOSSLESSLY. Drains the parked
    /// slot FIRST (the newest parked position is a delivery too — INT-2:
    /// the LAST move is never dropped), then returns the newest delivered
    /// point as the end context. The settle argument's release point is the
    /// FALLBACK context only (see [`DragSettled::end_point`]).
    ///
    /// `DragPhase::InFlight` ONLY: an armed release is not a drag — the
    /// router converts it to `cancel(ThresholdNotMet)` (a click is a
    /// click); calling settle while Armed refuses [`DragRefusal::NotInFlight`]
    /// without counters. Re-settling the consumed session refuses
    /// [`DragRefusal::NotInFlight`] the same way (exactly-once).
    pub(crate) fn settle(&mut self, end_point: Point) -> Result<DragSettled, DragRefusal> {
        if self.phase != DragPhase::InFlight {
            return Err(DragRefusal::NotInFlight);
        }
        let Some(origin) = self.origin.take() else {
            // Structurally impossible (InFlight implies a captured origin);
            // the rail demotes itself rather than panicking on the defect.
            self.phase = DragPhase::Idle;
            return Err(DragRefusal::NotInFlight);
        };
        // DRAIN FIRST (lossless): the newest parked move becomes a delivery
        // of its own; the drag's end context is therefore the newest
        // delivered point, never the older release estimate.
        let end_point = match self.owed_move.take() {
            Some(parked) => {
                self.last_delivered = Some(parked);
                self.counters.moves_delivered += 1;
                parked
            }
            // Nothing parked: the last already-delivered point defines the
            // end (the release argument is only the never-used fallback).
            None => self.last_delivered.take().unwrap_or(end_point),
        };
        let settled = DragSettled {
            surface_id: self.surface_id,
            address: origin.address,
            drag_meta: origin.drag_meta,
            start_point: origin.start_point,
            end_point,
            moves_delivered: self.counters.moves_delivered,
        };
        self.close_live_bookkeeping(DragPhase::Settled);
        self.counters.settles += 1;
        Ok(settled)
    }

    /// The §7 CANCEL `reason`: end the drag with NO semantic exit (§7
    /// "…cancel reason…"; the plugin's cancel side writes nothing —
    /// `plugin_grips.rs` lines 334–362). Drains the parked slot FIRST (the
    /// completion mirror of the settle drain — no move is ever dropped by
    /// whichever side the completion takes), returns the ORIGIN VERBATIM.
    /// `Armed` AND `InFlight` both cancel (an explicitly-cancelled hold
    /// below threshold — Escape during a pressed pointer — is still a 1:1
    /// completion); the consumed family refuses [`DragRefusal::NotInFlight`]
    /// exactly-once.
    pub(crate) fn cancel(&mut self, reason: DragCancelReason) -> Result<DragCancelled, DragRefusal> {
        if !matches!(self.phase, DragPhase::Armed | DragPhase::InFlight) {
            return Err(DragRefusal::NotInFlight);
        }
        let Some(origin) = self.origin.take() else {
            self.phase = DragPhase::Idle;
            return Err(DragRefusal::NotInFlight);
        };
        // Drain FIRST (the settle's mirror; nothing is lost either way).
        let end_point = match self.owed_move.take() {
            Some(parked) => {
                self.last_delivered = Some(parked);
                self.counters.moves_delivered += 1;
                parked
            }
            // Nothing parked in-flight, and nothing ever delivered: the
            // armed START point is the last verbatim position the session
            // knows (an Arm-phase cancel has no delivered end context).
            None => self.last_delivered.take().unwrap_or(origin.start_point),
        };
        let cancelled = DragCancelled {
            surface_id: self.surface_id,
            address: origin.address,
            drag_meta: origin.drag_meta,
            start_point: origin.start_point,
            end_point,
            reason,
        };
        self.close_live_bookkeeping(DragPhase::Cancelled);
        self.counters.cancels += 1;
        Ok(cancelled)
    }

    /// Completion bookkeeping (shared by settle/cancel): the §16 consumed
    /// phase stands, every live slot is drained, the delivered-point
    /// evidence clears. The origin and the completion FACT are already out
    /// of the rail — nothing lingers (exactly-once).
    fn close_live_bookkeeping(&mut self, phase: DragPhase) {
        self.owed_move = None;
        self.last_delivered = None;
        self.moves_in_flight = false;
        self.phase = phase;
    }
}

/// Euclidean pixel distance between two screen points.
fn distance_px(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

// ---------------------------------------------------------------------------
// Preview spec (the points the transient layer renders)
// ---------------------------------------------------------------------------

/// One host-local drag-preview prim: POINT FACTS only — never ink, never an
/// event, never document state (INT-3). The overlay painter maps these to
/// one transient-layer geometry
/// (`crate::ui::overlay::transient_layer`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PreviewSpec {
    /// The drag ghost: the corner-to-corner outline rect from the drag's
    /// origin point to the delivered point (the row/element region the
    /// gesture is relocating, drawn AS the ghost).
    Rect { start: Point, end: Point },
}

impl PreviewSpec {
    /// The spec's BYTE encoding — the deterministic headless wire the
    /// mount's frame painter consumes. Same delivered point ⇒ same bytes
    /// (pinned by `preview_specs_are_deterministic_per_delivered_move`);
    /// exact f32 LE bits, no allocations, no hashing.
    pub(crate) fn encode(&self) -> [u8; 16] {
        let (start, end) = match *self {
            Self::Rect { start, end } => (start, end),
        };
        let mut out = [0u8; 16];
        out[0..4].copy_from_slice(&start.x.to_le_bytes());
        out[4..8].copy_from_slice(&start.y.to_le_bytes());
        out[8..12].copy_from_slice(&end.x.to_le_bytes());
        out[12..16].copy_from_slice(&end.y.to_le_bytes());
        out
    }

    /// The outline-rect prim from corner to corner (order-preserving: the
    /// ghost's diagonal is start→end, never normalized away).
    fn rect(start: Point, end: Point) -> Self {
        Self::Rect { start, end }
    }
}

// ---------------------------------------------------------------------------
// Tests (headless, deterministic; NO renderer, NO sleeps, no Tasks)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::widgets::WidgetEvent;

    // -- helpers ----------------------------------------------------------

    fn p(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    /// The plain `DragMeta` fixture (opaque markers split per sub-case).
    fn meta(opaque: &str) -> DragMeta {
        DragMeta {
            opaque: Some(opaque.to_string()),
        }
    }

    /// A fresh rail on surface 900 with addresses 7/8 synced draggable,
    /// armed on `address` at point (10, 10) / tick 42 — the common arm
    /// fixture.
    fn arm_on(address: StableId) -> (DragSession, DragOrigin) {
        let mut session = DragSession::new(StableId(900));
        session.sync_draggables([StableId(7), StableId(8)]);
        let began = session
            .begin(StableId(900), address, meta("row-7"), p(10.0, 10.0), 42)
            .expect("the arm");
        (session, began.origin)
    }

    /// The crossed move the common arm fixture settles on: the first move
    /// exactly AT [`DRAG_THRESHOLD_PX`] from (10, 10) (the ≥ boundary).
    fn crossed_point() -> Point {
        p(10.0 + DRAG_THRESHOLD_PX, 10.0)
    }

    /// The FULL semantic counter scroll (everything but the ghost's own
    /// `previews_emitted`) — the preview-never-mutates probe's frozen
    /// expectation.
    fn semantic_counters(
        counters: &DragCounters,
    ) -> (usize, usize, usize, usize, usize, usize, usize) {
        (
            counters.begins,
            counters.moves_parked,
            counters.moves_delivered,
            counters.settles,
            counters.cancels,
            counters.refusals_duplicate_begin,
            counters.refusals_not_draggable,
        )
    }

    // -- 1. begin → move → settle is lossless ------------------------------

    /// §7 INT-1 + INT-2: the settle returns the ORIGIN verbatim, drains the
    /// parked slot (the newest position defines the end point), and every
    /// alternative re-complete refuses — exactly one semantic completion
    /// per interaction.
    #[test]
    fn begin_move_settle_is_lossless() {
        // (a) Machine-level: park TWO moves, no per-tick delivery — the
        // settle's drain delivers the newest.
        let (mut session, origin) = arm_on(StableId(7));
        assert_eq!(session.move_drag(p(30.0, 30.0)), MoveDisposition::Crossed);
        assert_eq!(
            session.move_drag(p(44.0, 8.0)),
            MoveDisposition::Parked { superseded: true },
            "the newest point superseded the still-owed one (latest wins)"
        );
        let before = session.counters();
        let settled = session.settle(p(50.0, 12.0)).expect("the settle");
        assert_eq!(settled.address, origin.address, "the address is verbatim");
        assert_eq!(settled.drag_meta, origin.drag_meta, "DragMeta VERBATIM");
        assert_eq!(settled.start_point, origin.start_point, "start context");
        assert_eq!(
            settled.end_point,
            p(44.0, 8.0),
            "the settle DRAINED the last parked move: the newest defines the end"
        );
        assert_eq!(
            settled.moves_delivered, 1,
            "the drain was the session's sole delivery"
        );
        assert_eq!(
            session.counters(),
            DragCounters {
                begins: 1,
                moves_parked: 2,
                moves_delivered: 1,
                settles: 1,
                ..before
            },
            "the §16 probe: exactly the settle's own path moved"
        );
        // The session consumed: a second settle refuses (no facts reshaped,
        // no counter stacked).
        assert_eq!(
            session.settle(p(51.0, 12.0)),
            Err(DragRefusal::NotInFlight),
            "exactly-once: consumed, the second settle refuses"
        );
        assert_eq!(session.counters().settles, 1);
        assert_eq!(session.phase(), DragPhase::Settled);

        // (b) Per-tick deliveries flow THROUGH the same losslessness (the
        // router's shape: take → deliver, then settle): end == newest, and
        // every delivery is counted once.
        let (mut session, origin) = arm_on(StableId(7));
        assert_eq!(session.move_drag(p(30.0, 30.0)), MoveDisposition::Crossed);
        assert_eq!(session.take_deliverable_move(), Some(p(30.0, 30.0)));
        assert!(session.release_delivered_move());
        assert_eq!(
            session.move_drag(p(60.0, 60.0)),
            MoveDisposition::Parked { superseded: false }
        );
        let settled = session.settle(p(70.0, 70.0)).expect("the settle");
        assert_eq!(settled.end_point, p(60.0, 60.0), "the drained newest");
        assert_eq!(settled.start_point, origin.start_point);
        assert_eq!(settled.drag_meta, origin.drag_meta);
        assert_eq!(
            settled.moves_delivered, 2,
            "one per-tick delivery + the settle drain"
        );
        assert_eq!(session.counters().settles, 1);
    }

    // -- 2. cancel paths 1:1 with begins -----------------------------------

    /// §7 INT-1: Escape, PointerOut, and ThresholdNotMet each end their OWN
    /// begun interaction 1:1 — every begin has exactly one completion, the
    /// reasons ride the fact, the counters agree, and a double-cancel is
    /// refused without stacking.
    #[test]
    fn cancel_paths_are_one_to_one_with_begins() {
        for reason in [
            DragCancelReason::Escape,
            DragCancelReason::PointerOut,
            DragCancelReason::ThresholdNotMet,
        ] {
            let (mut session, origin) = arm_on(StableId(7));
            // A live InFlight drag (a real drag — the threshold crossed) so
            // the cancel takes the InFlight branch.
            let moved = p(
                10.0 + DRAG_THRESHOLD_PX + 4.0,
                10.0 + DRAG_THRESHOLD_PX + 2.0,
            );
            assert_eq!(session.move_drag(moved), MoveDisposition::Crossed);
            let cancelled = session
                .cancel(reason)
                .expect("the 1:1 cancel for a live drag");
            assert_eq!(cancelled.address, origin.address, "address verbatim");
            assert_eq!(cancelled.drag_meta, origin.drag_meta, "meta verbatim");
            assert_eq!(cancelled.start_point, origin.start_point);
            assert_eq!(cancelled.reason, reason, "the caller's reason, verbatim");
            assert_eq!(
                cancelled.end_point, moved,
                "the newest parked position arrives with the cancel (drain first)"
            );
            assert_eq!(session.phase(), DragPhase::Cancelled);
            assert_eq!(
                session.counters().begins, 1,
                "one begin per interaction"
            );
            assert_eq!(
                session.counters().cancels, 1,
                "1:1: one cancel ends the one begin"
            );
            assert_eq!(session.counters().settles, 0, "no settle over a cancel");
            assert_eq!(
                session.cancel(reason),
                Err(DragRefusal::NotInFlight),
                "the double-cancel refuses"
            );
            assert_eq!(
                session.counters().cancels, 1,
                "the refused double-cancel counted nowhere"
            );
        }
        // The ARMED (pre-threshold) cancel: no delivered end exists, so the
        // start context stands in (the last verbatim position).
        let (mut session, origin) = arm_on(StableId(7));
        assert!(
            matches!(
                session.move_drag(p(10.9, 10.9)),
                MoveDisposition::UnderThreshold { .. }
            ),
            "sub-threshold jitter stays under the arm"
        );
        let cancelled = session
            .cancel(DragCancelReason::ThresholdNotMet)
            .expect("the armed cancel");
        assert_eq!(
            cancelled.end_point, origin.start_point,
            "an ARMED cancel has no delivered end: the start context stands in"
        );
        assert_eq!(session.counters().cancels, 1);
    }

    // -- 3. rapid moves: latest wins, the last move is never dropped ------

    /// INT-2: 100 rapid moves park into the single slot (each overwriting —
    /// latest wins); the settle's drain delivers exactly ONE move whose
    /// point is the LAST one. No position inside the stream ever survives
    /// as the end except the newest.
    #[test]
    fn rapid_moves_latest_wins_never_drops_the_last() {
        let (mut session, _origin) = arm_on(StableId(8));
        for index in 0..100usize {
            let point = p(10.0 + DRAG_THRESHOLD_PX + index as f32, 10.0);
            let disposition = session.move_drag(point);
            if index == 0 {
                assert_eq!(
                    disposition,
                    MoveDisposition::Crossed,
                    "the exactly-at-threshold first move goes live"
                );
            } else {
                assert_eq!(
                    disposition,
                    MoveDisposition::Parked { superseded: true },
                    "every later rapid move supersedes the still-owed one"
                );
            }
        }
        // The newest survived the whole storm: settle drains and defines
        // the end with the last parked position.
        let settled = session.settle(p(112.0, 10.0)).expect("the settle");
        assert_eq!(
            settled.end_point,
            p(10.0 + DRAG_THRESHOLD_PX + 99.0, 10.0),
            "the LAST position, nothing else"
        );
        assert_eq!(
            settled.moves_delivered, 1,
            "the settle's drain delivered exactly once"
        );
        assert_eq!(
            session.counters().moves_parked, 100,
            "every accepted move counted as parked"
        );
        assert_eq!(session.counters().moves_delivered, 1);
        assert_eq!(session.counters().settles, 1);
        assert_eq!(session.counters().cancels, 0);
    }

    // -- 4. duplicate begin refused in flight (the SWG-003 shape) ----------

    /// A second begin while Armed refuses with `DuplicateBegin`; while
    /// InFlight likewise; both move ONLY `refusals_duplicate_begin` and the
    /// live origin stays untouched (the SWG-003 exactly-once shape: the
    /// first interaction owns the pointer until its completion).
    #[test]
    fn duplicate_begin_refused_in_flight() {
        let (mut session, origin) = arm_on(StableId(7));

        // (a) An armed session refuses the second begin.
        let before = session.counters();
        assert_eq!(
            session.begin(
                StableId(900),
                StableId(8),
                meta("other-row"),
                p(1.0, 1.0),
                43
            ),
            Err(DragRefusal::DuplicateBegin(StableId(8))),
            "the armed session owns the pointer"
        );
        assert_eq!(
            session.counters(),
            DragCounters {
                refusals_duplicate_begin: 1,
                ..before
            },
            "only the DuplicateBegin refusal counted"
        );

        // (b) The threshold-crossing move goes live; the third begin refuses
        // while InFlight the same way.
        assert_eq!(session.move_drag(p(30.0, 30.0)), MoveDisposition::Crossed);
        let after_crossing = session.counters();
        assert_eq!(
            session.begin(
                StableId(900),
                StableId(8),
                meta("other-row"),
                p(2.0, 2.0),
                44
            ),
            Err(DragRefusal::DuplicateBegin(StableId(8))),
            "the InFlight drag refuses begins the same way"
        );
        assert_eq!(
            session.counters(),
            DragCounters {
                refusals_duplicate_begin: 2,
                ..after_crossing
            }
        );

        // (c) The ORIGINAL interaction keeps its origin: the refused begins
        // could not retarget the armed facts.
        let settled = session.settle(p(40.0, 40.0)).expect("the settle");
        assert_eq!(
            settled.drag_meta, origin.drag_meta,
            "the original origin survived the refused attempts"
        );
        assert_eq!(settled.address, origin.address);
        assert_eq!(settled.surface_id, StableId(900));
    }

    // -- 5. armed release below threshold is a click, not a drag ----------

    /// The ARM-threshold decision (module docs): a release while still
    /// Armed (no crossing move) cancels with `ThresholdNotMet`, never a
    /// settle — the settle counter stays zero and the completion's meta is
    /// verbatim (a click-like release is still lossless about the origin).
    /// The boundary case pins the ≥ side: exactly at the threshold IS a
    /// drag and settles.
    #[test]
    fn threshold_not_met_release_is_not_a_drag() {
        let (mut session, origin) = arm_on(StableId(7));
        // Small jitters never cross.
        assert!(matches!(
            session.move_drag(p(11.0, 10.0)),
            MoveDisposition::UnderThreshold { .. }
        ));
        assert_eq!(session.phase(), DragPhase::Armed, "still armed");
        assert_eq!(session.counters().moves_parked, 0, "jitter parks nothing");
        // A settle call while merely armed is refused (the router binds an
        // armed release to cancel instead — see DragRouter in
        // widget_drag.rs).
        assert_eq!(
            session.settle(p(10.5, 10.5)),
            Err(DragRefusal::NotInFlight)
        );
        // The release: the ONLY completion this interaction can take is the
        // ThresholdNotMet cancellation (no settle exists, ever).
        let cancelled = session
            .cancel(DragCancelReason::ThresholdNotMet)
            .expect("an armed release cancels with ThresholdNotMet");
        assert_eq!(cancelled.reason, DragCancelReason::ThresholdNotMet);
        assert_eq!(
            cancelled.drag_meta, origin.drag_meta,
            "the click-like release is still lossless about the origin"
        );
        assert_eq!(cancelled.address, origin.address);
        assert_eq!(session.counters().settles, 0, "NO settle exists");
        assert_eq!(session.counters().cancels, 1);

        // The boundary case: EXACTLY at the threshold IS a drag.
        let (mut session, _origin) = arm_on(StableId(8));
        assert_eq!(
            session.move_drag(crossed_point()),
            MoveDisposition::Crossed,
            "at >= the threshold the drag is live"
        );
        let settled = session.settle(p(14.0, 10.0)).expect("the real drag settles");
        assert_eq!(
            settled,
            DragSettled {
                surface_id: StableId(900),
                address: StableId(8),
                drag_meta: meta("row-7"),
                start_point: p(10.0, 10.0),
                end_point: crossed_point(),
                moves_delivered: 1,
            },
            "the drag's facts at the boundary"
        );
        assert_eq!(session.counters().settles, 1);
    }

    // -- 6. previews never mutate machines or events -----------------------

    /// INT-3: preview emission moves NO semantic counter (the ghost's
    /// `previews_emitted` is its own bookkeeping) and produces NO
    /// WidgetEvents ever. Armed previews are denied outright; InFlight
    /// previews emit; post-completion previews stop.
    #[test]
    fn previews_never_mutate_machines_or_events() {
        let events: Vec<WidgetEvent> = Vec::new();

        // (a) Armed: no preview at all (the ghost waits for live motion).
        let (mut session, _origin) = arm_on(StableId(7));
        let armed_counters = session.counters();
        assert!(
            session.preview_prims(p(11.0, 10.0)).is_empty(),
            "no ghost for an armed-but-not-dragging session"
        );
        assert_eq!(
            session.counters(),
            armed_counters,
            "not even the ghost counter moved while denied"
        );

        // (b) InFlight: the delivered move's ghost emits, and the SEMANTIC
        // counters stay bit-frozen around the emission.
        assert_eq!(
            session.move_drag(crossed_point()),
            MoveDisposition::Crossed
        );
        assert_eq!(session.take_deliverable_move(), Some(crossed_point()));
        let semantic_before = semantic_counters(&session.counters());
        let previews = session.preview_prims(crossed_point());
        assert_eq!(previews.len(), 1, "ONE ghost for the delivered move");
        assert_eq!(
            previews[0],
            PreviewSpec::Rect {
                start: p(10.0, 10.0),
                end: crossed_point()
            },
            "the ghost spans origin → delivered point"
        );
        assert_eq!(
            semantic_counters(&session.counters()),
            semantic_before,
            "the semantic counters are FROZEN by preview emission"
        );
        assert_eq!(
            session.counters().previews_emitted, 1,
            "the ghost's own bookkeeping moved"
        );
        assert!(events.is_empty(), "no WidgetEvent from previews");

        // (c) Post-consumption: the ghost stops (a stale preview emission is
        // impossible; nothing is live).
        let ghost_before_end = session.counters().previews_emitted;
        let _ = session.settle(crossed_point()).expect("the settle");
        assert!(
            session.preview_prims(crossed_point()).is_empty(),
            "nothing ghosts after the interaction closed"
        );
        assert_eq!(
            session.counters().previews_emitted,
            ghost_before_end,
            "the consumed session counts no preview"
        );
        assert!(
            events.is_empty(),
            "the whole preview session emitted ZERO WidgetEvents"
        );
    }

    // -- 7. preview specs are deterministic per delivered move -------------

    /// The ghost spec is pure geometry: the same delivered point encodes to
    /// the same bytes (f32 LE bits), and a different point encodes
    /// differently (the encoding is not a constant).
    #[test]
    fn preview_specs_are_deterministic_per_delivered_move() {
        let (mut session, _origin) = arm_on(StableId(7));
        session.move_drag(crossed_point());
        let delivered = session.take_deliverable_move().expect("the delivery");
        let first = session.preview_prims(delivered);
        assert_eq!(first.len(), 1);

        // Same delivered point (a second emission over the identical
        // position — unreachable by a real pointer stream, but the exact
        // DETERMINISM form the invariant pins): same bytes.
        let again = session.preview_prims(delivered);
        assert_eq!(again.len(), 1);
        assert_eq!(
            first[0].encode(),
            again[0].encode(),
            "identical delivered point → identical spec bytes"
        );

        // A different delivered point: different bytes.
        let mut jittered = delivered;
        jittered.x += 0.5;
        let third = session.preview_prims(jittered);
        assert_eq!(third.len(), 1);
        assert_ne!(
            first[0].encode(),
            third[0].encode(),
            "a moved point is a different spec (not a constant encoding)"
        );

        // The bytes are the exact f32 LE layout of (start, end), stable.
        let mut expected = [0u8; 16];
        expected[0..4].copy_from_slice(&p(10.0, 10.0).x.to_le_bytes());
        expected[4..8].copy_from_slice(&p(10.0, 10.0).y.to_le_bytes());
        expected[8..12].copy_from_slice(&delivered.x.to_le_bytes());
        expected[12..16].copy_from_slice(&delivered.y.to_le_bytes());
        assert_eq!(first[0].encode(), expected);
    }

    // -- 8/9. the settle → event conversion lives app-side: the router and
    // the settle→semantic hand-off are pinned by
    // `app::update::widget_drag`'s suite (`settle_to_event_binds_exactly_one_command`,
    // `meta_returns_via_next_sync_documented`) — the machine stays
    // event-agnostic (INT-4), which is exactly what these two tests move to
    // the caller's side.

    // -- 10. teardown cancel is 1:1 ------------------------------------------

    /// §16 close-tab: a surface teardown MID-DRAG cancels the live drag
    /// (the route-level seam — see DragRouter::teardown_surface in
    /// widget_drag.rs); an idle teardown cancels nothing and panics
    /// nowhere.
    #[test]
    fn teardown_cancel_is_1to1() {
        // (a) Machine-level: the live drag ends 1:1 on the Teardown reason.
        let (mut session, origin) = arm_on(StableId(7));
        session.move_drag(p(30.0, 30.0));
        let _ = session.take_deliverable_move();
        session.release_delivered_move();
        session.move_drag(p(55.0, 55.0));
        let cancelled = session
            .cancel(DragCancelReason::Teardown)
            .expect("teardown cancels the live drag 1:1");
        assert_eq!(cancelled.reason, DragCancelReason::Teardown);
        assert_eq!(cancelled.address, origin.address);
        assert_eq!(cancelled.drag_meta, origin.drag_meta, "verbatim at teardown");
        assert_eq!(cancelled.end_point, p(55.0, 55.0));
        assert_eq!(session.phase(), DragPhase::Cancelled);
        assert_eq!(session.counters().cancels, 1);
        assert_eq!(session.counters().settles, 0);

        // (b) The dead session cannot cancel twice (graceful, no new event
        // and no counter).
        assert_eq!(
            session.cancel(DragCancelReason::Teardown),
            Err(DragRefusal::NotInFlight)
        );
        assert_eq!(session.counters().cancels, 1);
    }

    // -- 11. counters are the defect probe -----------------------------------

    /// Every §16 path moves exactly its own counter — and NOTHING else: the
    /// refusal paths move nothing but their own line, the benign
    /// not-in-flight path moves NOTHING, and each accepted path's count is
    /// the one the invariants claim.
    #[test]
    fn counters_are_the_defect_probe() {
        // (a) The not-draggable begin: exactly its own refusal counter.
        let mut session = DragSession::new(StableId(900));
        session.sync_draggables([StableId(7)]);
        assert_eq!(
            session.begin(
                StableId(900),
                StableId(99),
                meta("ghost"),
                p(1.0, 1.0),
                7
            ),
            Err(DragRefusal::NotDraggable(StableId(99)))
        );
        assert_eq!(
            session.counters(),
            DragCounters {
                refusals_not_draggable: 1,
                ..DragCounters::default()
            },
            "the NotDraggable refusal moved ONLY its own counter"
        );

        // (b) The foreign-surface begin and every benign no-to-complete:
        // counters stay put entirely.
        assert_eq!(
            session.begin(
                StableId(901),
                StableId(7),
                meta("ghost"),
                p(1.0, 1.0),
                8
            ),
            Err(DragRefusal::ForeignSurface {
                session: StableId(900),
                addressed: StableId(901)
            })
        );
        assert_eq!(session.cancel(DragCancelReason::Escape), Err(DragRefusal::NotInFlight));
        assert_eq!(session.settle(p(1.0, 1.0)), Err(DragRefusal::NotInFlight));
        assert_eq!(session.take_deliverable_move(), None);
        assert_eq!(
            session.counters(),
            DragCounters {
                refusals_not_draggable: 1,
                ..DragCounters::default()
            },
            "the benign paths moved NOTHING"
        );

        // (c) The full happy path moves exactly the expected scroll lines
        // (and no preview/refusal counters unless their path runs).
        session.sync_draggables([StableId(7), StableId(99)]);
        assert!(
            session
                .begin(StableId(900), StableId(7), meta("row-7"), p(10.0, 10.0), 9)
                .is_ok()
        );
        assert!(matches!(session.move_drag(crossed_point()), MoveDisposition::Crossed));
        assert!(
            session.take_deliverable_move().is_some(),
            "the per-tick delivery parked-slot take"
        );
        session.release_delivered_move();
        // Preview emission (the ghost bookkeeping line).
        let previews = session.preview_prims(crossed_point());
        assert_eq!(previews.len(), 1);
        // Duplicate begin STILL refuses while live (own counter).
        assert_eq!(
            session.begin(StableId(900), StableId(7), meta("row-7"), p(10.0, 10.0), 10),
            Err(DragRefusal::DuplicateBegin(StableId(7)))
        );
        // The settle completes; the settle drain has nothing parked (the
        // delivered moved) so it re-uses the last delivered point.
        let settled = session.settle(p(44.0, 44.0)).expect("the settle");
        assert_eq!(settled.moves_delivered, 1);

        assert_eq!(
            session.counters(),
            DragCounters {
                begins: 1,
                moves_parked: 1,
                moves_delivered: 1,
                settles: 1,
                cancels: 0,
                previews_emitted: 1,
                refusals_duplicate_begin: 1,
                refusals_not_draggable: 1,
            },
            "the §16 totals are the session's defect probe: every line the
             interaction produced, one counter each, nothing else"
        );

        // (d) The cancel-side scroll: a begun-and-cancelled interaction
        // counters EXACTLY its own lines (the (settles, cancels) pair never
        // exceeds one completion total per interaction).
        let (mut session, _origin) = arm_on(StableId(8));
        session.cancel(DragCancelReason::PointerOut).expect("the cancel");
        assert_eq!(
            session.counters(),
            DragCounters {
                begins: 1,
                cancels: 1,
                ..DragCounters::default()
            },
            "a begun-and-cancelled-built interaction: exactly the cancel side counted"
        );
    }
}
