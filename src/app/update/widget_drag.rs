//! App-side drag rail: the headless pointer ROUTER over the per-surface
//! [`DragSession`] machines (`SCN-021`, ARCHITECTURE §7/§16).
//!
//! The drag MACHINES live in [`crate::ui::window::plugin_drag`] (`DragSession`,
//! the §7 lossless facts and refusals); THIS module owns the app-side joins:
//! the per-surface rail registry ([`DragRail`]) plus the headless
//! `route_drag_*` functions that translate raw pointer gestures into machine
//! calls, and the settle→semantic hand-off converter ([`settle_to_event`]).
//! No `OpenCADStudio` state is OWNED here yet — the mount integration task
//! later joins [`DragRail`] where the bridge joins the mount machines (the
//! seam is documented in `plugin_drag.rs` and in `plugin_widget_mount.rs`'s
//! registry docs); these functions are what that wiring will call, one
//! gesture per call, headless.
//!
//! # The composition contract with SCN-020's move coalescer (DECIDED)
//!
//! SCN-020 (running in parallel; input/focus/COALESCING additions to the
//! mount) owns the pointer-move COALESCER: at most ONE move leaves it per
//! tick, latest-wins. This rail composes strictly AFTER it:
//!
//! 1. the coalescer hands the rail ≤ 1 move per tick (its buffer does not
//!    outlive the hand-off — the point VALUE moves through the call, no
//!    shared state between the two layers);
//! 2. [`route_drag_point`] feeds that one move to the machine
//!    ([`DragSession::move_drag`]) — the rail's `owed_move` park is the
//!    SAME single-slot discipline, so nothing is double-buffered across the
//!    boundary: the coalescer's newest point IS the rail's newest point;
//! 3. the delivery half ([`DragSession::take_deliverable_move`]) emits at
//!    most one move per tick under the ≤1-in-flight gate and immediately
//!    releases the slot after the preview emission (the host-local channel
//!    is synchronous) — every delivered move yields its preview ONCE, per
//!    DELIVERED move, never per park.
//!
//! A route's outcome is terminal per gesture; the rail never invents a
//! second completion for an interaction that already completed (the machine
//! guards exactly-once; the router only maps).
//!
//! # Previews are POINT SPECS with a stroke — never pixels
//!
//! [`DragPreviewOverlay`] is WHAT THE MOUNT/SCENE PATHS RENDER: the ghost
//! prims' pure point geometry ([`PreviewSpec`], from the delivered moves)
//! plus the host-local stroke ([`PreviewStroke`] — a themed token + width).
//! The transient layer (`crate::ui::overlay::transient_layer`,
//! src/ui/overlay.rs lines 1918–1926) is the one-frame idiom the pixel half
//! hangs off; composing pixels is the mount task, keeping THIS spec pointed
//! and headless is the discipline (previews never touch machines, document,
//! or events — the machine module's INT-3 invariant).
//!
//! # The settle → semantic hand-off (DECIDED)
//!
//! [`settle_to_event`] is the PURE conversion the CALLER performs when a
//! settle fact is at hand: it binds ONE
//! `WidgetEvent::CommandInvoked(command, [the dragged element's address])`.
//! DECIDED conventions:
//! - the SETTLE is a semantic completion the plugin resolves (§13): the
//!   event carries only the surface id + the command + the target address;
//! - `DragMeta.opaque` is NEVER echoed by the host (the event vocabulary
//!   has no DragMeta field, BY WIRE PIN): the plugin's payload returns via
//!   the settled FACT ([`DragSettled`] — the caller holds it) AND via the
//!   plugin's NEXT authoritative surface sync — the meta is the plugin's
//!   own key and never needed the host to remember it;
//! - the settle command id rides the CALLER's string ("drag_settle" is the
//!   mount's spelling in v1) — the machine stays command-agnostic;
//! - a CANCEL converts to NO event at all (cancel is a lossless no-op for
//!   the plugin — the §7 cancel side).
//!
//! # What this module does NOT own (SCN-020's)
//!
//! No keyboard/focus handling, no coalescer internals, no mount/bridge/
//! panel edits, no `Message` arm — this file only CONSUMES the
//! `plugin_drag` machine's public(crate) surface and the API crate's event
//! vocabulary; the §16 teardown path routes through [`route_drag_teardown`]
//! (the close-tab contract).

// Stage-1 surface: the router is consumed by this module's unit tests until
// the mount integration task joins the rail into the surface registry (the
// seam documented in `plugin_drag.rs`'s module docs); the allow mirrors the
// established widget-machine stage precedent.
#![allow(dead_code)]

use std::collections::HashMap;

use iced::Point;
use ocs_plugin_api::scene::{DragMeta, StableId};
use ocs_plugin_api::widgets::WidgetEvent;

use crate::ui::window::plugin_drag::{
    DragBegan, DragCancelReason, DragCancelled, DragPhase, DragRefusal, DragSession, DragSettled,
    MoveDisposition, PreviewSpec,
};

// ---------------------------------------------------------------------------
// The rail: per-surface session registry + gestures
// ---------------------------------------------------------------------------

/// The app-side per-surface drag rail: the registry of each surface's
/// [`DragSession`]. Rails appear on a surface's first begin and leave at
/// teardown; the sessions' own machines guard the invariants (this map is
/// bookkeeping only — no invariant is implemented twice).
#[derive(Debug, Default)]
pub(crate) struct DragRail {
    sessions: HashMap<StableId, DragSession>,
}

/// What one routed pointer move did at the RAIL level (the per-tick
/// composition outcome; see the composition contract in the module docs).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DragMoveRoute {
    /// No drag is live on this surface — the pointer move is host-local
    /// noise (dropped, nothing created).
    NotDragged,
    /// Armed under the threshold: the move was jitter (no park, no
    /// delivery, no preview).
    UnderThreshold,
    /// The move parked (latest-wins) but was NOT delivered this tick — the
    /// wire-era case where the ≤1-in-flight slot is still held by a prior,
    /// unanswered delivery; the newest point is safe in the rail.
    Parked,
    /// The move left the rail THIS tick under the ≤1 gate, and its preview
    /// emitted exactly once (the mount's transient-layer input).
    Delivered {
        /// The delivered point (the newest the rail knew).
        point: Point,
        /// The ghost prims for it (HOST-LOCAL; spec points only).
        previews: Vec<PreviewSpec>,
    },
}

/// The release gesture's outcome: it discriminates a REAL drag's §7
/// completion from an armed (not-a-drag) release — a click.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DragRelease {
    /// A real drag settled (the §7 completion fact).
    Settled(DragSettled),
    /// An armed release below the [`DRAG_THRESHOLD_PX`](crate::ui::window::plugin_drag::DRAG_THRESHOLD_PX)
    /// region — a click, not a drag: routed to `cancel(ThresholdNotMet)`
    /// (the machine decision; no settle exists for this interaction).
    Click(DragCancelled),
    /// Nothing was live on this surface: a graceful no-op (no completion,
    /// nothing lost, no counter).
    NothingLive,
}

impl DragRail {
    /// The session's phase (the probes' read; `None` = no rail session).
    pub(crate) fn session_phase(&self, surface_id: StableId) -> Option<DragPhase> {
        self.sessions.get(&surface_id).map(DragSession::phase)
    }

    /// The session for a surface — the app-side READ (the machines still
    /// mutate rail-side; nothing here dereferences into a live mutation).
    pub(crate) fn session(&self, surface_id: StableId) -> Option<&DragSession> {
        self.sessions.get(&surface_id)
    }
}

/// A begin gesture into the rail (the mounting press on a
/// DragMeta-carrying element): arms — creating — the surface's session.
/// See [`DragSession::begin`] for the refusal vocabulary
/// (`DuplicateBegin` / `NotDraggable` / `ForeignSurface`).
pub(crate) fn route_drag_begin(
    rail: &mut DragRail,
    surface: StableId,
    address: StableId,
    drag_meta: DragMeta,
    start_point: Point,
    start_tick: u64,
) -> Result<DragBegan, DragRefusal> {
    rail.sessions
        .entry(surface)
        .or_insert_with(|| DragSession::new(surface))
        .begin(surface, address, drag_meta, start_point, start_tick)
}

/// A pointer-move gesture into the rail (composing after SCN-020's move
/// coalescer: ≤ 1 move per tick). Parks the newest point, delivers it under
/// the ≤1-in-flight gate, and emits the delivered move's preview ONCE.
pub(crate) fn route_drag_point(
    rail: &mut DragRail,
    surface: StableId,
    point: Point,
) -> DragMoveRoute {
    let Some(session) = rail.sessions.get_mut(&surface) else {
        return DragMoveRoute::NotDragged;
    };
    match session.move_drag(point) {
        MoveDisposition::Inactive => DragMoveRoute::NotDragged,
        MoveDisposition::UnderThreshold { .. } => DragMoveRoute::UnderThreshold,
        // Parked (including the ARM→InFlight crossing move): the delivery
        // half runs under the ≤1 gate — one move per tick, preview once.
        MoveDisposition::Crossed | MoveDisposition::Parked { .. } => {
            match session.take_deliverable_move() {
                Some(delivered) => {
                    let previews = session.preview_prims(delivered);
                    // The host-local channel is synchronous: the slot
                    // releases this tick (the wire era holds it for the
                    // plugin's answer — the machine keeps the gate).
                    session.release_delivered_move();
                    DragMoveRoute::Delivered {
                        point: delivered,
                        previews,
                    }
                }
                None => DragMoveRoute::Parked,
            }
        }
    }
}

/// A release gesture into the rail — the §7 SETTLE boundary when the drag
/// proved real, and an explicit "it was a click" completion when it didn't.
/// The discrimination IS the machine's phase: InFlight settles, Armed
/// routes to `cancel(ThresholdNotMet)`, anything else is `NothingLive`.
pub(crate) fn route_drag_release(
    rail: &mut DragRail,
    surface: StableId,
    release_point: Point,
) -> DragRelease {
    let Some(session) = rail.sessions.get_mut(&surface) else {
        return DragRelease::NothingLive;
    };
    match session.phase() {
        DragPhase::InFlight => session
            .settle(release_point)
            .map_or(DragRelease::NothingLive, DragRelease::Settled),
        DragPhase::Armed => session
            .cancel(DragCancelReason::ThresholdNotMet)
            .map_or(DragRelease::NothingLive, DragRelease::Click),
        _ => DragRelease::NothingLive,
    }
}

/// An explicit cancel gesture ([`DragCancelReason`]) into the rail — the
/// machine's 1:1 completion, refusing gracefully when nothing is live.
pub(crate) fn route_drag_cancel(
    rail: &mut DragRail,
    surface: StableId,
    reason: DragCancelReason,
) -> Result<DragCancelled, DragRefusal> {
    match rail.sessions.get_mut(&surface) {
        Some(session) => session.cancel(reason),
        None => Err(DragRefusal::NotInFlight),
    }
}

/// The §16 teardown path: the surface is closing — its live drag ends as
/// `cancel(Teardown)` (never a settle, never a lost origin) and the rail
/// ENTRY is removed (the machine record dies with the surface). Returns the
/// cancelled fact only when something was live; an idle teardown cancels
/// nothing (idempotent — the close-tab contract runs ONCE per begun drag).
pub(crate) fn route_drag_teardown(rail: &mut DragRail, surface: StableId) -> Option<DragCancelled> {
    let mut session = rail.sessions.remove(&surface)?;
    match session.phase() {
        DragPhase::Armed | DragPhase::InFlight => {
            let cancelled = session
                .cancel(DragCancelReason::Teardown)
                .expect("the live phase was just checked");
            Some(cancelled)
        }
        // Consumed/idle: the entry is removed silently (nothing to cancel).
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The settle → semantic hand-off (the ONE event binding)
// ---------------------------------------------------------------------------

/// Convert a settle FACT into the semantic completion: ONE `CommandInvoked`
/// bound to the dragged element's address. THE pure, machine-agnostic
/// hand-off the §13 contract names; see the module docs for the meta-echo
/// decision (the plugin's [`DragMeta`] never rides the event — it returns
/// via the settled fact and the next authoritative sync).
pub(crate) fn settle_to_event(settled: DragSettled, command: String) -> WidgetEvent {
    WidgetEvent::CommandInvoked {
        surface_id: settled.surface_id,
        command_id: command,
        target_ids: vec![settled.address],
    }
}

// ---------------------------------------------------------------------------
// The drag-preview overlay (SPEC ONLY: points + stroke)
// ---------------------------------------------------------------------------

/// The themed ghost's stroke: a host-local color token (resolved by the
/// pixel half at render time) plus the stroke width in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PreviewStroke {
    /// The ghost's token (the pixel half maps it through the app palette).
    pub token: PreviewColor,
    /// Stroke width in pixels (the mount clamps to a positive width).
    pub width: f32,
}

/// The host-local ghost color tokens (the mount resolves them against the
/// app theme — spec vocabulary only; no `iced::Color` leaks into the spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PreviewColor {
    /// The app accent (the standard "this is happening" color).
    #[default]
    Accent,
    /// The muted tone (a quieter ghost, e.g. over dense chrome).
    Muted,
}

/// The HOST-LOCAL drag-preview overlay SPEC: the surface, the ghost POINT
/// prims collected from this tick's delivered moves, and the stroke — the
/// points the mount/scene paths draw through
/// `crate::ui::overlay::transient_layer`. NOT pixels, NOT events, NOT
/// document state (the preview-never-mutates invariant: this type is the
/// total interactive surface of a preview, and none of it is a machine).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DragPreviewOverlay {
    /// The surface whose transient layer owns the geometry.
    pub surface_id: StableId,
    /// The ghost prims (per delivered move: the origin→point outline).
    pub prims: Vec<PreviewSpec>,
    /// The ghost stroke (token + width).
    pub stroke: PreviewStroke,
}

impl DragPreviewOverlay {
    /// The default ghost: accent stroke at 1.0 px — the mount's plain
    /// "drag is live" feedback.
    pub(crate) fn collect(surface_id: StableId, prims: Vec<PreviewSpec>) -> Self {
        Self {
            surface_id,
            prims,
            stroke: PreviewStroke {
                token: PreviewColor::Accent,
                width: 1.0,
            },
        }
    }

    /// Restyle the ghost (a themed token + width; the builder form).
    pub(crate) fn with_stroke(mut self, stroke: PreviewStroke) -> Self {
        self.stroke = stroke;
        self
    }

    /// The prims' count (the §16 linear-cost probe).
    pub(crate) fn len(&self) -> usize {
        self.prims.len()
    }

    /// `true` when no ghost prims collected (an empty overlay renders
    /// nothing — the mount skips the layer entirely).
    pub(crate) fn is_empty(&self) -> bool {
        self.prims.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests (headless; the ROUTER half — the machine's own invariants stay in
// `plugin_drag.rs`; these pin the joins, the composition, and the §16
// teardown path)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ----------------------------------------------------------

    fn p(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    fn meta() -> DragMeta {
        DragMeta {
            opaque: Some("row-7::opaque-payload".to_string()),
        }
    }

    /// A rail whose surface 900 has address 7 synced draggable (the
    /// draggable-set sync the mount's def walk will perform).
    fn rail_with_draggable_7() -> DragRail {
        let mut rail = DragRail::default();
        rail.sessions.insert(
            StableId(900),
            DragSession::with_draggables(StableId(900), [StableId(7)]),
        );
        rail
    }

    /// The crossing move for the common arm at (10, 10): exactly at the
    /// threshold (the ≥ boundary).
    fn crossed_point() -> Point {
        p(
            10.0 + crate::ui::window::plugin_drag::DRAG_THRESHOLD_PX,
            10.0,
        )
    }

    /// A plain armed machine (module surface 900, address 7 armed at
    /// (10, 10) / tick 42) for the settle→event conversion tests — the
    /// same facts the machine module's arm fixture uses.
    fn rail_machine_pair() -> (DragSession, crate::ui::window::plugin_drag::DragOrigin) {
        let mut session =
            DragSession::with_draggables(StableId(900), [StableId(7), StableId(8)]);
        let began = session
            .begin(
                StableId(900),
                StableId(7),
                DragMeta {
                    opaque: Some("row-7::opaque-payload".to_string()),
                },
                p(10.0, 10.0),
                42,
            )
            .expect("the arm");
        (session, began.origin)
    }

    /// The routed begin on the common rail (surface 900, address 7, the
    /// common meta, point (10, 10), tick 42).
    fn route_begin(rail: &mut DragRail) -> DragBegan {
        route_drag_begin(
            rail,
            StableId(900),
            StableId(7),
            meta(),
            p(10.0, 10.0),
            42,
        )
        .expect("the routed arm")
    }

    // -- 1. gestures without a rail are host-local noise -------------------

    /// Every gesture against an unknown surface is a GRACEFUL no-op: no
    /// panic, no creation, no completion invented (the app never routes
    /// through dead surfaces).
    #[test]
    fn gestures_without_a_rail_are_host_local_noise() {
        let mut rail = DragRail::default();
        assert_eq!(
            route_drag_point(&mut rail, StableId(404), p(5.0, 5.0)),
            DragMoveRoute::NotDragged
        );
        assert_eq!(
            route_drag_release(&mut rail, StableId(404), p(5.0, 5.0)),
            DragRelease::NothingLive
        );
        assert_eq!(
            route_drag_cancel(&mut rail, StableId(404), DragCancelReason::Escape),
            Err(DragRefusal::NotInFlight)
        );
        assert_eq!(route_drag_teardown(&mut rail, StableId(404)), None);
        assert_eq!(rail.session_phase(StableId(404)), None);
        assert!(rail.session(StableId(404)).is_none());
    }

    // -- 2. an armed release routes as a CLICK (ThresholdNotMet) ----------

    /// The router's release discrimination (the machine-module decision,
    /// proven through the ROUTER): an armed release below the threshold is
    /// `cancel(ThresholdNotMet)` — the fact returns verbatim, NO settle
    /// exists, and a second release is `NothingLive` (exactly-once).
    #[test]
    fn threshold_not_met_release_routes_as_click() {
        let mut rail = rail_with_draggable_7();
        let began = route_begin(&mut rail);
        assert_eq!(rail.session_phase(StableId(900)), Some(DragPhase::Armed));

        // Sub-threshold jitter: the rail reports it and parks nothing.
        assert_eq!(
            route_drag_point(&mut rail, StableId(900), p(10.9, 10.9)),
            DragMoveRoute::UnderThreshold
        );
        assert_eq!(rail.session_phase(StableId(900)), Some(DragPhase::Armed));
        // (Also the settle side: the release does not emit one.)
        let release = route_drag_release(&mut rail, StableId(900), p(10.5, 10.5));
        let cancelled = match release {
            DragRelease::Click(cancelled) => cancelled,
            other => panic!("an armed release is a click, got {other:?}"),
        };
        assert_eq!(cancelled.reason, DragCancelReason::ThresholdNotMet);
        assert_eq!(cancelled.address, began.origin.address);
        assert_eq!(
            cancelled.drag_meta, began.origin.drag_meta,
            "the click-like release is still lossless about the origin"
        );
        assert_eq!(rail.session_phase(StableId(900)), Some(DragPhase::Cancelled));

        // The machine's counters say the same: settles ZERO, cancels ONE.
        let session = rail.session(StableId(900)).expect("the session remains");
        assert_eq!(session.counters().settles, 0);
        assert_eq!(session.counters().cancels, 1);

        // The interaction is over: the second release is `NothingLive`.
        assert_eq!(
            route_drag_release(&mut rail, StableId(900), p(10.5, 10.5)),
            DragRelease::NothingLive
        );
    }

    // -- 3. one delivered move per tick, previewed once ---------------------

    /// The composition contract: each routed tick delivers its ONE move
    /// under the ≤1 gate and emits its preview exactly once — previews are
    /// per DELIVERED move (never per park), each tick's delivered point is
    /// that tick's newest (no double-buffering, no lost tick).
    #[test]
    fn crossing_delivers_one_previewed_move_per_tick() {
        let mut rail = rail_with_draggable_7();
        let began = route_begin(&mut rail);

        // Tick 1: the crossing move — delivered with its own ghost.
        let first = route_drag_point(&mut rail, StableId(900), crossed_point());
        let DragMoveRoute::Delivered {
            point: first_point,
            previews: first_ghosts,
        } = first
        else {
            panic!("the crossing move delivers its tick's move, got {first:?}");
        };
        assert_eq!(first_point, crossed_point());
        assert_eq!(first_ghosts.len(), 1, "ONE ghost for ONE delivered move");
        assert_eq!(
            first_ghosts[0],
            PreviewSpec::Rect {
                start: began.origin.start_point,
                end: crossed_point(),
            }
        );

        // Tick 2: the next coalesced move — delivered AGAIN (each tick
        // delivers its own move; the slot released after tick 1's ghost).
        let second = route_drag_point(&mut rail, StableId(900), p(50.0, 30.0));
        let DragMoveRoute::Delivered {
            point: second_point,
            previews: second_ghosts,
        } = second
        else {
            panic!("every routed tick delivers, got {second:?}");
        };
        assert_eq!(second_point, p(50.0, 30.0), "tick 2's newest, not tick 1's");
        assert_eq!(second_ghosts.len(), 1);

        // The machine's counters: two parks, two deliveries, two ghosts.
        let session = rail.session(StableId(900)).expect("live");
        assert_eq!(session.counters().moves_parked, 2);
        assert_eq!(session.counters().moves_delivered, 2);
        assert_eq!(session.counters().previews_emitted, 2);
        assert_eq!(session.counters().settles, 0);
    }

    // -- 4. the routed settle completes to the newest + binds ONE command --

    /// The §7 settle through the router: the completion fact binds EXACTLY
    /// ONE `CommandInvoked` — targets == [the dragged element] — and the
    /// crate's own `validate()` passes (the bounded construct path).
    #[test]
    fn route_settle_completes_to_newest_and_binds_one_command() {
        let mut rail = rail_with_draggable_7();
        let began = route_begin(&mut rail);
        assert!(matches!(
            route_drag_point(&mut rail, StableId(900), crossed_point()),
            DragMoveRoute::Delivered { .. }
        ));

        let release = route_drag_release(&mut rail, StableId(900), p(99.0, 99.0));
        let settled = match release {
            DragRelease::Settled(settled) => settled,
            other => panic!("a live drag settles, got {other:?}"),
        };
        assert_eq!(settled.address, began.origin.address);
        assert_eq!(settled.drag_meta, began.origin.drag_meta);
        assert_eq!(
            settled.end_point, crossed_point(),
            "the last DELIVERED point (the release argument is only the fallback)"
        );
        assert_eq!(rail.session_phase(StableId(900)), Some(DragPhase::Settled));

        // The semantic hand-off: ONE event, targets exact, valid.
        let event = settle_to_event(settled, "drag_settle".to_string());
        assert_eq!(
            event,
            WidgetEvent::CommandInvoked {
                surface_id: StableId(900),
                command_id: "drag_settle".to_string(),
                target_ids: vec![StableId(7)],
            }
        );
        assert!(event.validate().is_ok());
    }

    // -- 5. teardown cancels 1:1 (mid-drag), and idles gracefully ---------

    /// §16 close-tab: a surface teardown MID-drag cancels the live drag
    /// (some cancel fact, never a settle), removes the rail entry, and an
    /// idle teardown cancels NOTHING (idempotent — also twice in a row).
    #[test]
    fn route_teardown_is_1to1_and_graceful_when_idle() {
        let mut rail = rail_with_draggable_7();
        let began = route_begin(&mut rail);
        assert!(
            matches!(
                route_drag_point(&mut rail, StableId(900), crossed_point()),
                DragMoveRoute::Delivered { .. }
            ),
            "the drag is live before the teardown"
        );

        let cancelled = route_drag_teardown(&mut rail, StableId(900))
            .expect("the live drag cancelled on teardown");
        assert_eq!(cancelled.reason, DragCancelReason::Teardown);
        assert_eq!(cancelled.address, began.origin.address);
        assert_eq!(cancelled.drag_meta, began.origin.drag_meta, "verbatim");
        assert_eq!(
            rail.session_phase(StableId(900)),
            None,
            "the rail entry died with the surface"
        );

        // The teardown is idempotent: nothing live, no panic, no fact.
        assert_eq!(route_drag_teardown(&mut rail, StableId(900)), None);

        // An ARMED (pre-threshold) teardown also cancels 1:1 — the drag
        // never proved itself, but the begin still completes exactly once.
        let mut rail = rail_with_draggable_7();
        let began = route_begin(&mut rail);
        let cancelled = route_drag_teardown(&mut rail, StableId(900))
            .expect("the armed drag cancelled on teardown");
        assert_eq!(cancelled.reason, DragCancelReason::Teardown);
        assert_eq!(cancelled.address, began.origin.address);
        assert_eq!(rail.session_phase(StableId(900)), None);
    }

    // -- 6. the coalescer composition: one move per tick, latest wins ------

    /// The composition contract end-to-end: pour 100 ticks (the coalescer's
    /// ≤1-per-tick stream) through the rail — EVERY tick delivers its own
    /// move (the rail recomposes nothing across ticks), the previews ride
    /// exactly those deliveries, and the settle defines the end by the LAST
    /// tick's point (latest wins at every step; the release argument never
    /// overwrites it).
    #[test]
    fn coalescer_composition_latest_wins_per_tick() {
        let mut rail = rail_with_draggable_7();
        route_begin(&mut rail);

        for index in 1..=100usize {
            let point = p(
                10.0 + crate::ui::window::plugin_drag::DRAG_THRESHOLD_PX + index as f32,
                10.0,
            );
            let route = route_drag_point(&mut rail, StableId(900), point);
            let DragMoveRoute::Delivered {
                point: delivered,
                previews,
            } = route
            else {
                panic!("tick {index} delivered under the ≤1 gate, got {route:?}");
            };
            assert_eq!(delivered, point, "tick {index} delivered ITS point");
            assert_eq!(previews.len(), 1, "tick {index} previewed once");
        }

        // The settle: the drain has nothing parked (every tick delivered),
        // so the end is the LAST delivered point — release's (11, 11) never
        // sneaks in as the end context.
        let release = route_drag_release(&mut rail, StableId(900), p(11.0, 11.0));
        let settled = match release {
            DragRelease::Settled(settled) => settled,
            other => panic!("the live drag settles, got {other:?}"),
        };
        assert_eq!(
            settled.end_point,
            p(
                10.0 + crate::ui::window::plugin_drag::DRAG_THRESHOLD_PX + 100.0,
                10.0
            ),
            "the LAST tick's point"
        );
        let session = rail.session(StableId(900)).expect("the session remains");
        assert_eq!(
            session.counters().moves_delivered, 100,
            "one delivery per tick — the coalescer+slot discipline, end to end"
        );
        assert_eq!(
            session.counters().previews_emitted, 100,
            "one ghost per delivered move"
        );
    }

    // -- 7. the preview overlay is a POINT spec with a stroke --------------

    /// The overlay type is the SPEC the mount/scene paths render: surface +
    /// point prims + a themed stroke — and NOTHING pixel-ish. Empty
    /// collections render nowhere (`is_empty`); the default ghost is the
    /// accent stroke at 1 px.
    #[test]
    fn preview_overlay_is_a_point_spec_with_stroke() {
        let mut rail = rail_with_draggable_7();
        route_begin(&mut rail);
        let DragMoveRoute::Delivered { previews, .. } =
            route_drag_point(&mut rail, StableId(900), crossed_point())
        else {
            unreachable!("the crossing move delivered");
        };

        let overlay = DragPreviewOverlay::collect(StableId(900), previews.clone());
        assert_eq!(overlay.surface_id, StableId(900));
        assert_eq!(overlay.prims, previews, "the spec carries the prims");
        assert_eq!(
            overlay.stroke,
            PreviewStroke {
                token: PreviewColor::Accent,
                width: 1.0
            }
        );
        assert_eq!(overlay.len(), 1);
        assert!(!overlay.is_empty());

        // The builder restyle: a quieter ghost, wider stroke.
        let muted = overlay
            .clone()
            .with_stroke(PreviewStroke {
                token: PreviewColor::Muted,
                width: 2.0,
            });
        assert_eq!(muted.prims, overlay.prims, "the geometry is untouched");
        assert_eq!(muted.stroke, PreviewStroke {
            token: PreviewColor::Muted,
            width: 2.0
        });

        // An empty collection: the overlay renders nothing.
        let empty = DragPreviewOverlay::collect(StableId(900), Vec::new());
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
    }

    // -- 8. settle_to_event binds EXACTLY one command -----------------------

    /// The settle FACT converts to ONE `WidgetEvent::CommandInvoked` with
    /// targets == [the dragged element's address], and the crate's own
    /// `validate()` passes (the construct-level bounded path). One
    /// conversion per fact, never a second semantic exit: the settle
    /// consumes the session (the machine refuses re-completions).
    #[test]
    fn settle_to_event_binds_exactly_one_command() {
        let (mut session, _origin) = rail_machine_pair();
        session.move_drag(crossed_point());
        let _ = session.take_deliverable_move();
        session.release_delivered_move();
        let settled = session.settle(p(40.0, 40.0)).expect("the settle");
        let address = settled.address;

        let event = settle_to_event(settled.clone(), "drag_settle".to_string());
        assert_eq!(
            event,
            WidgetEvent::CommandInvoked {
                surface_id: StableId(900),
                command_id: "drag_settle".to_string(),
                target_ids: vec![address],
            },
            "ONE command bound to the dragged element's address"
        );
        assert!(
            event.validate().is_ok(),
            "the bound event passes the crate's own validation"
        );
        // The pure conversion of ONE fact is stable (rebinding the same
        // fact yields the same event — the fire-once discipline lives in
        // the session consumption, not in stateful conversion).
        let rebound = settle_to_event(settled, "drag_settle".to_string());
        assert_eq!(event, rebound, "the pure conversion of one fact is stable");
    }

    // -- 9. the meta returns via the next sync (documented, not echoed) ----

    /// INT-5 (the machine module's `plugin_drag.rs` docs, INT-5): the host
    /// NEVER fabricates a meta-echo — the event vocabulary carries no
    /// DragMeta field, the conversion rides only surface-id + command +
    /// target address, and the VERBATIM meta flows through the DragSettled
    /// fact the caller holds (the plugin resolves via ITS OWN next sync).
    #[test]
    fn meta_returns_via_next_sync_documented() {
        let (mut session, origin) = rail_machine_pair();
        session.move_drag(crossed_point());
        let _ = session.take_deliverable_move();
        session.release_delivered_move();
        let settled = session.settle(p(40.0, 40.0)).expect("the settle");

        // The FACT carries the meta verbatim — nothing rebuilt, nothing
        // dropped, nothing re-derived host-side.
        assert_eq!(
            settled.drag_meta, origin.drag_meta,
            "the DragMeta fact is byte-verbatim at the settle"
        );

        // The EVENT the fact produces does NOT contain (nor can it ever
        // contain) a meta echo: CommandInvoked's entire shape is surface_id
        // + command_id + target_ids — no echo field exists to fabricate.
        let event = settle_to_event(settled, "drag_settle".to_string());
        match event {
            WidgetEvent::CommandInvoked {
                surface_id,
                command_id,
                target_ids,
            } => {
                assert_eq!(surface_id, StableId(900));
                assert_eq!(command_id, "drag_settle");
                assert_eq!(target_ids, vec![StableId(7)]);
            }
            other => panic!("settle_to_event binds CommandInvoked, got {other:?}"),
        }
        // The converted event is a legal host-construed event (validate
        // passes — no smuggled meta ride).
        let legal = WidgetEvent::CommandInvoked {
            surface_id: StableId(900),
            command_id: "drag_settle".to_string(),
            target_ids: vec![StableId(7)],
        };
        assert!(legal.validate().is_ok());
    }

    // -- 10. a cancel converts to NO event (the cancel side is silent) ------

    /// The meta-echo/cancel-side discipline at the route level: a routed
    /// cancel produces NO fact convertible to an event in this file's
    /// vocabulary — `settle_to_event` accepts ONLY settled facts, a cancel
    /// rides no HAND-OFF (the plugin's cancel side writes nothing).
    #[test]
    fn cancel_converts_to_no_event() {
        let mut rail = rail_with_draggable_7();
        let began = route_begin(&mut rail);
        assert!(
            matches!(
                route_drag_point(&mut rail, StableId(900), crossed_point()),
                DragMoveRoute::Delivered { .. }
            )
        );
        let cancelled = route_drag_cancel(
            &mut rail,
            StableId(900),
            DragCancelReason::PointerOut,
        )
        .expect("the 1:1 cancel");
        assert_eq!(cancelled.reason, DragCancelReason::PointerOut);
        assert_eq!(cancelled.drag_meta, began.origin.drag_meta);
        assert_eq!(rail.session_phase(StableId(900)), Some(DragPhase::Cancelled));
        // There is no `cancel_to_event`: the fact is terminal (the module
        // docs' DECIDED convention) — exact check of the whole module API.
        // The settle path CANNOT fire anymore (exactly-once), so no event
        // can ever exist for THIS interaction:
        assert_eq!(
            route_drag_release(&mut rail, StableId(900), p(0.0, 0.0)),
            DragRelease::NothingLive
        );
    }
}
