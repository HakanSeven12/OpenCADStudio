//! Custom-grip drag lifecycle (API v11/v12): the plugin-owned twin of the
//! built-in grip drag in `update/viewport.rs`.
//!
//! Same ride as built-ins — Esc cancels, mouse-up settles, another command
//! starting mid-drag cancels, tab switches cancel — but the host applies
//! NOTHING semantically. Delivery honors the binding invariant of the v11.1
//! hardening (a plugin must never be able to freeze the host): every
//! out-of-process phase is issued WITHOUT waiting and collected at pump
//! ticks, nested COMMAND frames are refused in the delivery context (they
//! would need the message loop the blocked UI thread cannot pump), and a
//! settle that outlives its deadline is struck: the first strike cancels
//! the drag while keeping the plugin's connection, grips and session (a
//! starved pump must not amputate a healthy plugin); the third missed
//! delivery inside the strike window tears the connection down, ends the
//! drag as a cancel, names the plugin and queues a grip re-declaration.
//!
//! v11.2 live geometry: a move/begin answer may carry `live_entities` —
//! the plugin-directed visual phase (the plugin computes exactly what the
//! settle will commit). The host swaps those entities live through the same
//! visual-only dynamic semantics its own grip drags show: elements visibly
//! move, ZERO history steps mid-drag, everything is restored byte-identically
//! before the settle lands, and cancel discards it untouched by any undo
//! step. A preview-only response behaves exactly as before.

use std::time::Duration;

use glam::DVec3;
use iced::{Point, Rectangle, Task};

use crate::app::{Message, OpenCADStudio};
use crate::plugin::grip_points::{self, ActivePluginGrip, GripCandidate};
use ocs_plugin_api::host::Ramp;

/// Poll slice for an outstanding delivery (kept tiny: the pump reschedules
/// itself, so the app loop never waits on plugin latency).
const MOVE_POLL_SLICE: Duration = Duration::from_millis(8);
/// How long the settle waits for outstanding moves before issuing (the
/// CursorMove floor): a plugin slower than this is treated as unresponsive
/// for the drag, and the settle is still delivered.
const MOVE_DRAIN_BUDGET: Duration = Duration::from_millis(500);
/// Self-rescheduling pump cadence while a drag is live.
const PUMP_CADENCE: Duration = Duration::from_millis(10);
/// Tighter cadence while a settle is outstanding: the plugin's writes
/// stream in as nested requests served by the pump, so the finalize should
/// land promptly after the callback returns.
const SETTLING_CADENCE: Duration = Duration::from_millis(2);
/// The watchdog floor for the settle phase (matches the transport's per-kind
/// floor): beyond this the delivery is struck — see
/// [`strike_plugin_grip_settle`].
pub(crate) const SETTLE_WATCHDOG: Duration = Duration::from_secs(10);
/// The strike window: three missed deliveries inside it mean the plugin is
/// genuinely wedged under a live pump, and the connection is torn down.
/// A starved UI thread or one slow settle never accumulates that far
/// because one strike only cancels the drag.
const SETTLE_STRIKE_WINDOW: Duration = Duration::from_secs(600);
/// Missed deliveries allowed before the teardown applies.
const SETTLE_STRIKES_UNTIL_TEARDOWN: usize = 3;

impl OpenCADStudio {
    /// Custom grips of tab `i` that are visible in the current view, with
    /// their tile-relative screen positions. Hover, press and render share
    /// this one query — it goes through the per-tab uniform grid with
    /// viewport culling, never a linear scan of every declared grip.
    pub(in crate::app) fn plugin_grips_in_view(&self, i: usize) -> Vec<(GripCandidate, Point)> {
        if self.tabs.len() <= i || self.tabs[i].plugin_grips.is_empty() {
            return Vec::new();
        }
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let tile = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let bounds = Rectangle {
            width: tile.width,
            height: tile.height,
            ..Rectangle::default()
        };
        let cam = edit_frame.map_or_else(
            || self.tabs[i].scene.camera.borrow().clone(),
            |(cam, _)| cam,
        );
        // World rectangle of the view: unproject the tile corners on the
        // camera's target plane (the same plane cursor picks fall back to).
        let corners = [
            Point::new(0.0, 0.0),
            Point::new(bounds.width, 0.0),
            Point::new(0.0, bounds.height),
            Point::new(bounds.width, bounds.height),
        ];
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        let mut any = false;
        for corner in corners {
            let world = cam.pick_on_target_plane(corner, bounds);
            min[0] = min[0].min(world.x);
            min[1] = min[1].min(world.y);
            max[0] = max[0].max(world.x);
            max[1] = max[1].max(world.y);
            any = true;
        }
        if !any {
            return Vec::new();
        }
        let visible = grip_points::visibility(&self.disabled_plugins);
        let candidates = self.tabs[i].plugin_grips.query_rect(min, max, &visible);
        let mut out = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if let Some(screen) = cam.project(candidate.point, bounds) {
                out.push((candidate, iced::Point::new(screen.x, screen.y)));
            }
        }
        out
    }

    /// Nearest visible plugin grip within the grip aperture of the
    /// tile-relative cursor — the custom-grip twin of `find_hit_grip`,
    /// feeding both hover and press. Tested before any wire pick, so a
    /// grip coincident with click-through (reference-only) geometry stays
    /// fully hittable and wins the hit.
    pub(in crate::app) fn hit_plugin_grip(&self, i: usize, cursor: Point) -> Option<GripCandidate> {
        let candidates = self.plugin_grips_in_view(i);
        let mut best_dist = crate::scene::pick::grip::GRIP_THRESHOLD_PX;
        let mut best = None;
        for (candidate, screen) in candidates {
            let dx = screen.x - cursor.x;
            let dy = screen.y - cursor.y;
            let d = (dx * dx + dy * dy).sqrt();
            if d < best_dist {
                best_dist = d;
                best = Some(candidate);
            }
        }
        best
    }

    /// Press on a plugin-declared grip: engage the drag and deliver the begin
    /// event without waiting (a missing owner — process died between render
    /// and press — is a no-op). Returns immediately in every case: the UI
    /// thread never blocks on plugin answers.
    pub(in crate::app) fn begin_plugin_grip_drag(
        &mut self,
        i: usize,
        candidate: &GripCandidate,
    ) -> Task<Message> {
        if self.plugin_grip_deliveries.is_settling() {
            // A settle is still streaming; refuse new engagements until it
            // finalizes (the write window discipline).
            return Task::none();
        }
        self.plugin_grip_deliveries = Default::default();
        self.plugin_grip_live_originals = Vec::new();
        self.plugin_grip_descriptor_drag = None;
        let active = ActivePluginGrip {
            tab: i,
            plugin: candidate.plugin.clone(),
            grip_id: candidate.grip_id,
            anchor: candidate.anchor,
            origin_world: candidate.point,
            axis: candidate.axis,
            last_world: candidate.point,
            moved: false,
        };
        self.tabs[i].active_plugin_grip = Some(active);
        let event = self.event_for_active(
            i,
            ocs_plugin_api::host::CustomGripPhase::Begin,
            candidate.point,
        );
        if let Some(event) = event {
            self.issue_phase(i, ocs_plugin_api::host::CustomGripPhase::Begin, event);
        }
        self.pump_plugin_grips()
    }

    /// Park the newest constrained cursor point for the active custom drag
    /// (the viewport move branch computes the point; the channel drops
    /// superseded ones) and drive delivery. Returns immediately.
    pub(in crate::app) fn move_plugin_grip_drag(
        &mut self,
        i: usize,
        point: DVec3,
    ) -> Task<Message> {
        if let Some(active) = self.tabs[i].active_plugin_grip.as_mut() {
            active.last_world = point;
            active.moved = true;
        }
        self.plugin_grip_deliveries.park_move(point);
        self.pump_plugin_grips()
    }

    /// The pump: process landed answers in order (preview + live geometry),
    /// poll the next outstanding delivery (bounded), send the newest parked
    /// move when the channel is free, run the settle watchdog, and reschedule
    /// itself while the drag lives. NEVER blocks the app loop on plugin
    /// latency.
    pub(in crate::app) fn pump_plugin_grips(&mut self) -> Task<Message> {
        let Some(active) = self.active_plugin_grip() else {
            self.plugin_grip_deliveries = Default::default();
            return Task::none();
        };
        let tab = active.tab;
        let plugin = active.plugin.clone();

        // 1. Process landed answers in send order. Settle landings finalize
        //    the drag; late begin/move answers after the drag ended apply
        //    nothing.
        let mut finalize_pendings = Vec::new();
        while let Some((phase, response)) = self.plugin_grip_deliveries.pop_landed() {
            match phase {
                ocs_plugin_api::host::CustomGripPhase::Begin
                | ocs_plugin_api::host::CustomGripPhase::Move => {
                    if self.tabs[tab].active_plugin_grip.is_some() {
                        self.apply_plugin_grip_move_response(tab, phase, response);
                    } else if !response.live_entities.is_empty() {
                        // The drag already ended; a live-geometry answer that
                        // arrives late must not mutate anything.
                        self.restore_plugin_grip_live_geometry(tab);
                    }
                }
                ocs_plugin_api::host::CustomGripPhase::Settle => {
                    self.plugin_grip_deliveries.settle_landed();
                    if self.tabs[tab].active_plugin_grip.is_some() {
                        finalize_pendings.push(tab);
                    }
                }
                ocs_plugin_api::host::CustomGripPhase::Cancel => {
                    // The drag ends locally at cancel time; the answer is a
                    // courtesy and applies nothing.
                }
            }
        }
        for tab in finalize_pendings {
            self.finalize_plugin_grip_settle(tab);
        }

        // 2. Poll the next outstanding out-of-process answer (bounded slice;
        //    nested plugin requests — including the settle's writes — are
        //    served inline here on UI ticks; command frames are refused).
        if let Some((id, response)) =
            grip_points::poll_grip_event(self, tab, &plugin, MOVE_POLL_SLICE)
        {
            if let Some(phase) = self.plugin_grip_deliveries.resolve(id) {
                self.plugin_grip_deliveries.push_landed(phase, response);
                // Recurse once to keep order: landed answers first.
                return self.pump_plugin_grips();
            }
        }

        // 3. Coalesced move send: keep at most one in flight — then loop so
        //    the fresh answer (in-process deliveries land synchronously) is
        //    processed in THIS pump pass, never left to lag behind a
        //    later phase (e.g. re-applying stale live geometry after the
        //    settle already restored it).
        if let Some(point) = self.plugin_grip_deliveries.take_sendable_move() {
            if self.plugin_grip_descriptor_drag.is_some() {
                // API v14 (the descriptor-backed drag): the Begin answer
                // handed the host a live design, so the coalesced cursor move
                // re-derives the display set LOCALLY along the record model's
                // own laws — no Move frame crosses the IPC while the mode is
                // engaged. The v12 streaming semantics stay the fallback for
                // every unserved declaration.
                self.apply_descriptor_grip_move(tab, point);
            } else {
                let event =
                    self.event_for_active(tab, ocs_plugin_api::host::CustomGripPhase::Move, point);
                if let Some(event) = event {
                    self.issue_phase(tab, ocs_plugin_api::host::CustomGripPhase::Move, event);
                    if self.plugin_grip_deliveries.has_landed() {
                        // Process the fresh landing now (order-preserving).
                        return self.pump_plugin_grips();
                    }
                }
            }
        }

        // 4. The settle watchdog: a delivery that outlives its deadline is
        //    struck. The first strike ends the drag as a cancel and keeps the
        //    plugin's connection, grips and session alive (a starved pump
        //    must not amputate a healthy plugin); the third missed delivery
        //    inside the window tears the connection down. The UI loop never
        //    waits.
        let now = std::time::Instant::now();
        if self.plugin_grip_deliveries.settle_timed_out(now) {
            self.strike_plugin_grip_settle(tab);
            return Task::none();
        }

        // 5. Reschedule while the drag (or its finalization) is alive.
        self.schedule_plugin_grip_pump()
    }

    /// Settle (mouse release, or Enter): restore the live-geometry phase
    /// byte-identically FIRST (so the plugin's `push_undo` captures the
    /// pre-drag state, never a mid-drag visual), drain outstanding moves,
    /// then issue the settle — WITHOUT waiting. The plugin's writes stream
    /// back as nested requests served at pump ticks; its answer finalize
    /// closes any plugin-opened pending history into the drag's exactly one
    /// plugin-labeled undo step. A plugin that never answers is abandoned
    /// by the watchdog. Returns immediately in every case.
    pub(in crate::app) fn settle_plugin_grip_drag(&mut self) -> Task<Message> {
        let Some(active) = self.active_plugin_grip() else {
            return Task::none();
        };
        let tab = active.tab;
        let final_point = active.last_world;
        if self.plugin_grip_deliveries.is_settling() {
            // A second settle while settling: ignore (Enter after mouse-up).
            return Task::none();
        }
        // Drain outstanding moves first (bounded, never coalesced away):
        // the settle would otherwise race the move stream.
        self.drain_plugin_grip_moves(tab);
        // Restore the visual-only phase byte-identically BEFORE the settle:
        // the plugin's push_undo must see the pre-drag document.
        self.restore_plugin_grip_live_geometry(tab);
        let event = self.event_for_active(
            tab,
            ocs_plugin_api::host::CustomGripPhase::Settle,
            final_point,
        );
        if let Some(event) = event {
            self.issue_phase(tab, ocs_plugin_api::host::CustomGripPhase::Settle, event);
            self.plugin_grip_deliveries.begin_settle_watch(
                self.plugin_grip_settle_watchdog,
                std::time::Instant::now(),
            );
        } else {
            // The owner vanished mid-drag: end as a cancel.
            let _ = self.cancel_plugin_grip_drag();
        }
        self.pump_plugin_grips()
    }

    /// Cancel (Esc, tab switch, another command starting, process death):
    /// restore the visual-only phase, issue the cancel phase so the plugin
    /// drops preview state, and write nothing. Ends the drag immediately —
    /// the cancel answer is a courtesy and applies nothing. Zero undo
    /// steps, byte-identical drawing.
    pub(in crate::app) fn cancel_plugin_grip_drag(&mut self) -> Task<Message> {
        let Some(active) = self.active_plugin_grip() else {
            return Task::none();
        };
        let tab = active.tab;
        let plugin = active.plugin.clone();
        let last_point = active.last_world;
        self.restore_plugin_grip_live_geometry(tab);
        let event = self.event_for_active(
            tab,
            ocs_plugin_api::host::CustomGripPhase::Cancel,
            last_point,
        );
        if let Some(event) = event {
            self.issue_phase(tab, ocs_plugin_api::host::CustomGripPhase::Cancel, event);
        }
        // A well-behaved plugin wrote nothing during the drag; a badly
        // behaved one may have streamed writes with its cancel — close any
        // scope it opened so the cancel cannot leave history dangling.
        self.finish_pending_history(tab);
        self.end_plugin_grip_drag(tab);
        let _ = plugin;
        Task::none()
    }

    /// The idle state gate for the settle path: the drag is still settling
    /// (the plugin's writes are streaming through the pump).
    pub(in crate::app) fn is_plugin_grip_settling(&self) -> bool {
        self.plugin_grip_deliveries.is_settling()
    }

    /// Drive pump ticks until the outstanding settle finalizes (or the
    /// watchdog abandons it). Bounded by the settle floor plus a small
    /// margin — the automation/MCP route may wait for its outcome, the UI
    /// route never calls this. Returns `true` when the settle finalized.
    pub(in crate::app) fn drive_plugin_grip_settle_to_completion(
        &mut self,
        budget: Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + budget;
        loop {
            let _ = self.pump_plugin_grips();
            if !self.is_plugin_grip_settling() {
                return self.active_plugin_grip().is_none();
            }
            if std::time::Instant::now() >= deadline {
                // The watchdog has not fired yet but the automation budget
                // is spent: let the watchdog own the abandonment.
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The active custom drag (one at a time, app-wide).
    pub(in crate::app) fn active_plugin_grip(&self) -> Option<&ActivePluginGrip> {
        for tab in &self.tabs {
            if let Some(active) = &tab.active_plugin_grip {
                return Some(active);
            }
        }
        None
    }

    // ── internals ────────────────────────────────────────────────────────

    /// Build the event for the active drag's current point, or `None` when
    /// no drag is active.
    fn event_for_active(
        &self,
        tab: usize,
        phase: ocs_plugin_api::host::CustomGripPhase,
        point: DVec3,
    ) -> Option<ocs_plugin_api::host::CustomGripEvent> {
        self.tabs[tab]
            .active_plugin_grip
            .as_ref()
            .map(|active| active.event(phase, point))
    }

    /// Issue one delivery phase: in-process sinks dispatch synchronously and
    /// land immediately; out-of-process endpoints send WITHOUT waiting (the
    /// pump collects at ticks; nested command frames are refused in that
    /// context; the watchdog guards the settle).
    fn issue_phase(
        &mut self,
        tab: usize,
        phase: ocs_plugin_api::host::CustomGripPhase,
        event: ocs_plugin_api::host::CustomGripEvent,
    ) {
        let Some(active) = self.tabs[tab].active_plugin_grip.as_ref() else {
            return;
        };
        let plugin = active.plugin.clone();
        if grip_points::sink_installed(&plugin) {
            let response = grip_points::dispatch_grip_event(self, tab, &plugin, event.clone());
            if let Some(response) = response {
                self.plugin_grip_deliveries.push_landed(phase, response);
            } else {
                crate::plugin::push_guard_error(format!(
                    "custom grip event delivery to {plugin} produced no answer"
                ));
            }
            return;
        }
        match grip_points::send_grip_event(self, tab, &plugin, event) {
            Some(id) => self.plugin_grip_deliveries.track(id, phase),
            None => {
                // No live endpoint (death raced the drag): end the drag for
                // terminal phases; moves just stop being delivered.
                if matches!(
                    phase,
                    ocs_plugin_api::host::CustomGripPhase::Settle
                        | ocs_plugin_api::host::CustomGripPhase::Begin
                ) {
                    self.end_plugin_grip_drag(tab);
                }
            }
        }
    }

    /// Drain any outstanding move answer before settle/cancel so those
    /// phases never meet a stale frame (bounded; in-flight moves resolve
    /// their phase when they land).
    fn drain_plugin_grip_moves(&mut self, tab: usize) {
        if !self.plugin_grip_deliveries.is_move_in_flight() {
            return;
        }
        // In-process delivery is synchronous — nothing to wait for.
        let plugin = self.tabs[tab]
            .active_plugin_grip
            .as_ref()
            .map(|active| active.plugin.clone());
        let Some(plugin) = plugin else {
            return;
        };
        if grip_points::sink_installed(&plugin) {
            return;
        }
        let deadline = std::time::Instant::now() + MOVE_DRAIN_BUDGET;
        while self.plugin_grip_deliveries.is_move_in_flight() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match grip_points::poll_grip_event(self, tab, &plugin, remaining.min(MOVE_POLL_SLICE)) {
                Some((id, response)) => {
                    if let Some(phase) = self.plugin_grip_deliveries.resolve(id) {
                        self.plugin_grip_deliveries.push_landed(phase, response);
                    }
                }
                None => {
                    if !grip_points::grip_move_in_flight(&plugin) {
                        break;
                    }
                }
            }
        }
        // Process what landed (previews keep flowing; no mutation of drag
        // state).
        while let Some((phase, response)) = self.plugin_grip_deliveries.pop_landed() {
            if matches!(
                phase,
                ocs_plugin_api::host::CustomGripPhase::Begin
                    | ocs_plugin_api::host::CustomGripPhase::Move
            ) {
                self.apply_plugin_grip_move_response(tab, phase, response);
            }
        }
    }

    /// Apply a begin/move answer: the plugin's preview set REPLACES its
    /// previous one, and its live-geometry entities swap in through the
    /// visual-only phase (originals stashed once; zero history steps).
    fn apply_plugin_grip_move_response(
        &mut self,
        tab: usize,
        phase: ocs_plugin_api::host::CustomGripPhase,
        response: ocs_plugin_api::host::CustomGripResponse,
    ) {
        if matches!(phase, ocs_plugin_api::host::CustomGripPhase::Begin) {
            // API v14 (the descriptor-backed drag): a Begin answer may hand
            // the host a live design. Engagement happens only when the whole
            // declaration is serviceable (the descriptor + a finite positive
            // tolerance + a class-stamped anchor + the class schema's drive
            // row + a profile-chain END re-anchor inside the segment range);
            // anything else keeps the v12 streaming path with one headline
            // naming the constraint — older plugins and old-style responses
            // deserialize their fields to None and never reach the mode.
            let tolerance = response
                .descriptor_tolerance
                .filter(|t| t.is_finite() && *t > 0.0);
            match (response.descriptor.clone(), tolerance) {
                (
                    Some(ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(chain)),
                    Some(tolerance),
                ) => {
                    self.engage_descriptor_grip_drag(tab, chain, tolerance);
                }
                (Some(descriptor), None) => {
                    crate::plugin::push_guard_error(format!(
                        "custom grip begin: {:?} handed a descriptor design without a finite \
                         positive descriptor tolerance; the streaming path continues",
                        descriptor.kind_label(),
                    ));
                }
                (None, _) => {}
            }
        }
        let wires = crate::app::plugin_host::convert_preview_wires(response.preview.clone());
        self.tabs[tab].scene.set_preview_wires(wires);
        self.apply_plugin_grip_live_geometry(tab, &response.live_entities);
        self.tabs[tab].dirty = true;
    }

    /// Swap the plugin's live-geometry entities in (visual-only: originals
    /// stashed once per handle, no history, no notifications). Capped; a
    /// too-large payload is refused with an error line naming the constraint.
    fn apply_plugin_grip_live_geometry(
        &mut self,
        tab: usize,
        live_entities: &[(codec::Handle, codec::EntityType)],
    ) {
        if live_entities.is_empty() {
            return;
        }
        if live_entities.len() > ocs_plugin_api::host::MAX_PLUGIN_LIVE_ENTITIES {
            crate::plugin::push_guard_error(format!(
                "custom-grip live geometry refused: {} entities exceeds the per-response cap of {}",
                live_entities.len(),
                ocs_plugin_api::host::MAX_PLUGIN_LIVE_ENTITIES
            ));
            return;
        }
        for (handle, live_entity) in live_entities {
            let Some(current) = self.tabs[tab].scene.document.get_entity(*handle) else {
                crate::plugin::push_guard_error(format!(
                    "custom-grip live geometry names unknown entity {handle:?}"
                ));
                continue;
            };
            // Live geometry moves an existing editable entity; it never swaps
            // in another handle or kind, nor edits a locked layer.
            if live_entity.common().handle != *handle
                || std::mem::discriminant(live_entity) != std::mem::discriminant(current)
                || self.tabs[tab].scene.is_layer_locked(*handle)
            {
                crate::plugin::push_guard_error(format!(
                    "custom-grip live geometry for {handle:?} refused: it must keep the \
                     entity's handle and type on an unlocked layer"
                ));
                continue;
            }
            if !self
                .plugin_grip_live_originals
                .iter()
                .any(|(stashed, _)| stashed == handle)
            {
                self.plugin_grip_live_originals
                    .push((*handle, current.clone()));
            }
            if let Some(entity) = self.tabs[tab].scene.document.get_entity_mut(*handle) {
                *entity = live_entity.clone();
            }
            self.tabs[tab]
                .scene
                .bump_entities_restyled(&[(*handle, crate::scene::ChangeKind::Modified)]);
        }
    }

    /// Restore every live-geometry original byte-identically (the drag's
    /// visual state is discarded whether settling or cancelling; the
    /// restore happens BEFORE the settle so the plugin's push_undo sees the
    /// pre-drag document).
    fn restore_plugin_grip_live_geometry(&mut self, tab: usize) {
        let originals = std::mem::take(&mut self.plugin_grip_live_originals);
        for (handle, original) in originals {
            if let Some(entity) = self.tabs[tab].scene.document.get_entity_mut(handle) {
                *entity = original;
            }
            self.tabs[tab]
                .scene
                .bump_entities_restyled(&[(handle, crate::scene::ChangeKind::Modified)]);
        }
        self.tabs[tab].dirty = true;
    }

    /// The settle landed: close the plugin-opened pending history (exactly
    /// one plugin-labeled undo step when it wrote, zero when it did not),
    /// clear the drag, refresh. A landed settle is recovery: the plugin's
    /// strike count resets, so one missed deadline alone never accumulates
    /// toward the teardown.
    fn finalize_plugin_grip_settle(&mut self, tab: usize) {
        if let Some(active) = self.tabs[tab].active_plugin_grip.as_ref() {
            self.plugin_grip_settle_strikes.remove(&active.plugin);
        }
        self.finish_pending_history(tab);
        self.end_plugin_grip_drag(tab);
        self.refresh_properties();
    }

    /// The watchdog fired: the plugin never answered its settle within the
    /// settle floor. Strike semantics keep a healthy plugin healthy: the
    /// first strike ends the drag as a cancel (the live-geometry restore and
    /// the pending history closed — no dangling undo scope) while the
    /// plugin's connection, declared grips and session all stay; only the
    /// third missed delivery inside the strike window tears the connection
    /// down, because a plugin that misses three deadlines under a live pump
    /// is genuinely wedged. A struck delivery's late answer is drained
    /// gracefully by the pump's in-order poll: the nested requests it
    /// carries (the plugin's own settle transaction) are served by the host
    /// session exactly as they arrive, and the answer can never finalize a
    /// drag — its request id is no longer tracked. After a teardown the
    /// plugin is queued for a grip re-declaration the reap tick re-dispatches
    /// once it is visible again. Every strike names the plugin, the budget
    /// and the strike count in the error queue.
    fn strike_plugin_grip_settle(&mut self, tab: usize) {
        let plugin = self.tabs[tab]
            .active_plugin_grip
            .as_ref()
            .map(|active| active.plugin.clone())
            .unwrap_or_default();
        let now = std::time::Instant::now();
        let budget = self.plugin_grip_settle_watchdog;
        let strikes = {
            let ledger = self
                .plugin_grip_settle_strikes
                .entry(plugin.clone())
                .or_default();
            ledger.retain(|at| now.saturating_duration_since(*at) < SETTLE_STRIKE_WINDOW);
            ledger.push(now);
            ledger.len()
        };
        if strikes >= SETTLE_STRIKES_UNTIL_TEARDOWN {
            grip_points::abandon_grip_delivery(&plugin);
            crate::plugin::push_guard_error(format!(
                "custom-grip settle delivery to {plugin:?} exceeded its {budget:?} deadline \
                 (strike {strikes}/{}): the delivery was abandoned, the connection torn down \
                 and the drag cancelled — a live pump is starved or the plugin is wedged",
                SETTLE_STRIKES_UNTIL_TEARDOWN
            ));
            self.plugin_grip_settle_strikes.remove(&plugin);
            self.pending_plugin_grips_rearm.insert(plugin);
        } else {
            crate::plugin::push_guard_error(format!(
                "custom-grip settle delivery to {plugin:?} exceeded its {budget:?} deadline \
                 (strike {strikes}/{}): the connection is kept and the drag cancelled — a live \
                 pump is starved or the plugin is wedged",
                SETTLE_STRIKES_UNTIL_TEARDOWN
            ));
        }
        self.restore_plugin_grip_live_geometry(tab);
        self.finish_pending_history(tab);
        self.end_plugin_grip_drag(tab);
    }

    /// Shared end-of-drag cleanup: state off, preview off, channel idle,
    /// live stash gone, tab marked dirty.
    fn end_plugin_grip_drag(&mut self, tab: usize) {
        self.tabs[tab].active_plugin_grip = None;
        self.plugin_grip_deliveries = Default::default();
        self.plugin_grip_live_originals = Vec::new();
        self.plugin_grip_descriptor_drag = None;
        self.tabs[tab].scene.set_preview_wires(Vec::new());
        self.tabs[tab].dirty = true;
    }

    fn schedule_plugin_grip_pump(&mut self) -> Task<Message> {
        let drag_alive = self.active_plugin_grip().is_some();
        let settling = self.is_plugin_grip_settling();
        if drag_alive || settling || self.plugin_grip_deliveries.is_busy() {
            let cadence = if settling {
                SETTLING_CADENCE
            } else {
                PUMP_CADENCE
            };
            Task::perform(grip_pump_delay(cadence), |()| Message::PluginGripPump)
        } else {
            Task::none()
        }
    }

    /// Reap the grip sets of plugins that died or were unloaded, and cancel
    /// the drag/hover if the dying plugin owned it. Called from the periodic
    /// plugin drain tick; nothing else changes ("removes the grip and
    /// nothing else"). Liveness is the same visibility the render/hit paths
    /// apply (loaded, alive, not disabled).
    pub(in crate::app) fn reap_plugin_grips(&mut self) {
        let (dying_drag, dead_hover) = {
            let visible = grip_points::visibility(&self.disabled_plugins);
            for i in 0..self.tabs.len() {
                self.tabs[i]
                    .plugin_grips
                    .retain_alive(|plugin| visible(plugin));
                // API v14: the class registry reaps with the same liveness —
                // a dead plugin's declarations must not outlive it.
                self.tabs[i]
                    .plugin_classes
                    .retain_alive(|plugin| visible(plugin));
            }
            let dying_drag = self
                .active_plugin_grip()
                .map(|active| !visible(&active.plugin))
                .unwrap_or(false);
            let dead_hover = self
                .plugin_grip_hover
                .as_ref()
                .is_some_and(|hover| !visible(&hover.plugin));
            (dying_drag, dead_hover)
        };
        if dying_drag {
            let _ = self.cancel_plugin_grip_drag();
        }
        if dead_hover {
            self.plugin_grip_hover = None;
        }
        // The visibility predicate borrows the disabled set; a copy lets the
        // re-arm take `&mut self` after the borrow-heavy reap work above.
        let disabled = self.disabled_plugins.clone();
        let visible = grip_points::visibility(&disabled);
        self.rearm_struck_plugin_grips(&visible);
    }

    /// The strike-teardown re-arm: a plugin whose connection the watchdog
    /// tore down keeps its grip DECLARATIONS only as long as it is visible
    /// (loaded, alive, not disabled). While a queued re-arm plugin is not
    /// visible it stays queued and drops out when it is gone for good; once
    /// it is visible again, the `SelectionChanged` fingerprint dedup is
    /// cleared and the broadcast re-issues even for an unchanged selection,
    /// so the plugin re-declares its grips without human intervention
    /// (a mouse-only session never issues a dispatch of its own).
    fn rearm_struck_plugin_grips(&mut self, visible: &impl Fn(&str) -> bool) {
        if self.pending_plugin_grips_rearm.is_empty() {
            return;
        }
        let back: Vec<String> = self
            .pending_plugin_grips_rearm
            .iter()
            .filter(|plugin| visible(plugin))
            .cloned()
            .collect();
        if back.is_empty() {
            return;
        }
        for plugin in back {
            self.pending_plugin_grips_rearm.remove(&plugin);
        }
        self.last_plugin_selection = None;
        self.notify_plugins_selection_changed();
    }

    // ── API v14: the descriptor-backed grip drag ─────────────────────────
    //
    // The Begin answer handed the host a live design (the record layer's
    // own grammar): the coalesced cursor move re-derives the display set
    // LOCALLY — the drive point follows the cursor per the declared anchor
    // law, the driving ramp re-derives per its arm's own integral law, the
    // joints propagate from the engine's own sampling, and the chain's own
    // audit (the staging build) gates every move. The plugin's settle
    // callback stays the only write window: the per-move restage is the
    // visual-only phase (originals stashed once, zero history steps,
    // byte-identical restore before the settle's push_undo sees the
    // pre-drag document).

    /// Engage the descriptor mode when the whole declaration is serviceable.
    /// Everything else keeps the v12 streaming path with one headline.
    fn engage_descriptor_grip_drag(
        &mut self,
        tab: usize,
        chain: ocs_plugin_api::host::ParametricChain,
        tolerance: f64,
    ) {
        let Some(active) = self.tabs[tab].active_plugin_grip.as_ref() else {
            return;
        };
        let plugin = active.plugin.clone();
        let grip_id = active.grip_id;
        let refuse = |note: String| {
            crate::plugin::push_guard_error(format!(
                "descriptor grip {grip_id} of {plugin:?}: {note} — the streaming path continues"
            ))
        };
        let Some(anchor) = active.anchor else {
            refuse(
                "the declared grip carries no anchor entity, so the host cannot resolve the \
                 instance's identity stamp"
                    .to_owned(),
            );
            return;
        };
        let binding = match crate::app::plugin_host::resolve_plugin_class_binding(self, tab, anchor)
        {
            Ok(binding) => binding,
            Err(reason) => {
                refuse(format!(
                    "its anchor carries no registered class stamp ({reason})"
                ));
                return;
            }
        };
        let Some(declaration) = self.tabs[tab].plugin_classes.find(&plugin, &binding.class_id)
        else {
            refuse(format!("class {:?} not registered", binding.class_id));
            return;
        };
        let Some(drive) = declaration
            .grip_schema
            .iter()
            .find(|entry| entry.grip_id == grip_id)
            .and_then(|entry| entry.drives)
        else {
            refuse("the class schema's row does not declare a drive".to_owned());
            return;
        };
        if chain.plane != ocs_plugin_api::host::ParametricPlane::Profile {
            refuse(
                "the plan chain has no host-served END re-anchor; plan drags keep the \
                 plugin's own per-move semantics"
                    .to_owned(),
            );
            return;
        }
        if drive.at_start {
            refuse(
                "the declared START side has no host-served re-anchor yet; an END-point drop \
                 re-anchors fully"
                    .to_owned(),
            );
            return;
        }
        let segment = drive.segment as usize;
        if segment >= chain.segments.len() {
            refuse(format!(
                "the drive names segment {} beyond the design's {} segments",
                drive.segment,
                chain.segments.len()
            ));
            return;
        }
        let handles = Self::instance_display_set(self, tab, &binding);
        if handles.is_empty() {
            refuse(
                "no stamped display set resolves for the instance's identity stamp".to_owned(),
            );
            return;
        }
        self.plugin_grip_descriptor_drag = Some(crate::plugin::grip_points::DescriptorGripDrag {
            plugin,
            class_id: binding.class_id,
            design_record: binding.design_record,
            chain,
            tolerance,
            handles,
            segment,
            anchor: drive.anchor,
            refusal_noted: false,
        });
    }

    /// The coalesced cursor move in the descriptor mode: the local walk —
    /// re-anchor, propagate, audit, patch over the same handles. A refused
    /// move keeps the last good design on the screen (the drag continues);
    /// exactly one headline notes refusals per drag.
    fn apply_descriptor_grip_move(&mut self, tab: usize, point: DVec3) {
        // Snapshot the state (the walk reads local copies; the mode's truth
        // re-commits only when a move's own audit passes).
        let Some(drag) = self.plugin_grip_descriptor_drag.clone() else {
            return;
        };
        // The grip's declared axis hard-constrains the drive point BEFORE the
        // re-anchor (idempotent with the viewport's own constraint; the
        // direct-delivery paths pass the raw cursor). The profile sheet is
        // the world XY plane; the projection lands on it.
        let constrained = match (
            self.tabs
                .get(tab)
                .and_then(|t| t.active_plugin_grip.as_ref())
                .map(|active| active.origin_world),
            self.tabs[tab]
                .active_plugin_grip
                .as_ref()
                .and_then(|active| active.axis),
            drag.chain.plane,
        ) {
            (Some(origin), Some(axis), ocs_plugin_api::host::ParametricPlane::Profile) => {
                let d = point - origin;
                let along = d.dot(axis);
                let hit = origin + axis * along;
                DVec3::new(hit.x, hit.y, 0.0)
            }
            _ => point,
        };
        let chain = drag.chain.clone();
        let segment = drag.segment;
        let tolerance = drag.tolerance;
        let handles = drag.handles.clone();
        match reanchor_profile_chain(&chain, segment, drag.anchor, constrained) {
            Ok(reanchored) => match crate::parametric::build_display_entities(
                &reanchored,
                tolerance,
            ) {
                Ok((fresh, _carriers)) if fresh.len() == handles.len() => {
                    // The honest lane (the seat test's own arms): clone the
                    // live entities — identity, layer and the XDATA stamp ride
                    // along — and patch ONLY the geometry from the re-derived
                    // carriers. The count-staged set stays the drag's handles.
                    let document = &self.tabs[tab].scene.document;
                    let mut pairs: Vec<(codec::Handle, codec::EntityType)> =
                        Vec::with_capacity(handles.len());
                    let mut refused: Option<String> = None;
                    for (slot, fresh_entity) in handles.iter().zip(fresh.iter()) {
                        let Some(current) = document.get_entity(*slot) else {
                            refused = Some(format!("staged entity {slot:?} is no longer live"));
                            break;
                        };
                        let mut patched = current.clone();
                        if let Err(reason) =
                            patch_carrier_geometry(&mut patched, fresh_entity)
                        {
                            refused = Some(reason);
                            break;
                        }
                        pairs.push((*slot, patched));
                    }
                    match refused {
                        None => {
                            let live = crate::plugin::grip_points::DescriptorGripDrag {
                                chain: reanchored,
                                ..drag
                            };
                            self.plugin_grip_descriptor_drag = Some(live);
                            // The live-geometry machinery: same handles,
                            // originals stashed once, zero history steps.
                            self.apply_plugin_grip_live_geometry(tab, &pairs);
                        }
                        Some(reason) => Self::note_descriptor_refusal(
                            self,
                            format!("the display set cannot be patched in place: {reason}"),
                        ),
                    }
                }
                Ok((fresh, _)) => Self::note_descriptor_refusal(
                    self,
                    format!(
                        "the re-derived entity count {} differs from the staged set {}; the \
                         count-stable restage cannot apply",
                        fresh.len(),
                        handles.len()
                    ),
                ),
                Err(reason) => Self::note_descriptor_refusal(
                    self,
                    format!("the re-derivation's own gates refuse the move: {reason}"),
                ),
            },
            Err(reason) => Self::note_descriptor_refusal(self, reason),
        }
    }

    /// The stamped display set of the dragged instance: every entity whose
    /// identity stamp names the binding's class AND its design record, in
    /// document order (the staging order = the carrier order the restage
    /// patches over). An instance staged without a record linkage matches
    /// stamps that carry the schema version alone.
    fn instance_display_set(
        app: &OpenCADStudio,
        tab: usize,
        binding: &crate::app::plugin_host::PluginClassInstanceBinding,
    ) -> Vec<codec::Handle> {
        use codec::xdata::XDataValue;
        let mut out = Vec::new();
        for entity in app.tabs[tab].scene.document.entities() {
            let handle = entity.common().handle;
            let stamped = entity
                .common()
                .extended_data
                .records()
                .iter()
                .any(|record| {
                    if record.application_name != binding.class_id || record.values.is_empty() {
                        return false;
                    }
                    if !matches!(record.values.first(), Some(XDataValue::Integer32(_))) {
                        return false;
                    }
                    match record.values.get(1) {
                        Some(XDataValue::String(linkage)) => {
                            !binding.design_record.is_empty()
                                && linkage == &binding.design_record
                        }
                        None => binding.design_record.is_empty(),
                        Some(_) => false,
                    }
                });
            if stamped {
                out.push(handle);
            }
        }
        out
    }

    /// One headline per drag for move refusals, then silence (a wild drag
    /// across an illegal band stays quiet while the last good design shows).
    fn note_descriptor_refusal(this: &mut OpenCADStudio, note: String) {
        if let Some(drag) = this.plugin_grip_descriptor_drag.as_mut() {
            if !drag.refusal_noted {
                drag.refusal_noted = true;
                crate::plugin::push_guard_error(format!(
                    "descriptor grip drag for {:?}: {note}; the last good design stays until \
                     a move the model serves",
                    drag.plugin
                ));
            }
        }
    }
}

/// Patch ONLY the geometry of a live staged carrier from a re-derived
/// entity, keyed by kind (the seat test's honest arms). The identity stamp,
/// the reference-only designation and the layer ride on the clone untouched.
pub(in crate::app) fn patch_carrier_geometry(
    patched: &mut codec::EntityType,
    fresh: &codec::EntityType,
) -> Result<(), String> {
    match (&mut *patched, &*fresh) {
        (codec::EntityType::Line(old), codec::EntityType::Line(newer)) => {
            old.start = newer.start;
            old.end = newer.end;
        }
        (codec::EntityType::Spline(old), codec::EntityType::Spline(newer)) => {
            old.degree = newer.degree;
            old.control_points = newer.control_points.clone();
            old.knots = newer.knots.clone();
            old.weights = newer.weights.clone();
            old.begin_tangent = newer.begin_tangent;
            old.end_tangent = newer.end_tangent;
        }
        (codec::EntityType::Arc(old), codec::EntityType::Arc(newer)) => {
            old.center = newer.center;
            old.radius = newer.radius;
            old.start_angle = newer.start_angle;
            old.end_angle = newer.end_angle;
        }
        (codec::EntityType::Circle(old), codec::EntityType::Circle(newer)) => {
            old.center = newer.center;
            old.radius = newer.radius;
        }
        _ => {
            return Err(format!(
                "the re-derived set keeps the staged carrier shape ({} vs {})",
                entity_kind_label(patched),
                entity_kind_label(fresh),
            ));
        }
    }
    Ok(())
}

/// A stable diagnostic label for the carrier-shape refusal.
fn entity_kind_label(entity: &codec::EntityType) -> &'static str {
    match entity {
        codec::EntityType::Line(_) => "line",
        codec::EntityType::Spline(_) => "spline",
        codec::EntityType::Arc(_) => "arc",
        codec::EntityType::Circle(_) => "circle",
        _ => "other",
    }
}

/// The generic anchor-law serve for a profile chain (api 14, v2): the
/// driven segment's declared end re-anchors per the drive's anchor law,
/// through one generic ramp re-derivation - no per-species host code. The
/// downstream joints re-declare from the engine's own sampling: each later
/// segment's start re-bases to the sampled joint (point, sheet direction)
/// and its driving ramp re-bases by the additive shift that restates the
/// joint's slope as the ramp's own start value (the arm's shape
/// preserved). The result still owes its final legality to the chain's own
/// audit (the staging build gates the move). The plan plane and the START
/// side keep the streaming path (the engage refusals).
///
/// Served anchor laws (profile plane, end side):
/// - EndStationToXY: the run becomes the cursor's station span and the
///   ramp's driving data re-derives per its arm's own integral gain so the
///   segment's end height lands on the cursor's elevation;
/// - EndElevation: the run is kept; the same end-value re-derivation aims
///   the cursor's elevation;
/// - EndGrade: the ramp's end value takes the grade the cursor's sheet
///   point carries from the segment start (the run is kept; the height
///   follows the law);
/// - EndCurvatureScale: the whole driving ramp scales by a factor solved
///   by the deterministic bracketed bisection toward the cursor's
///   elevation (the same 16-doubling + 60-bisect discipline);
/// - Length: the run re-runs to the cursor's station span; the ramp's law
///   stays; the height follows the law.
pub(in crate::app) fn reanchor_profile_chain(
    chain: &ocs_plugin_api::host::ParametricChain,
    segment: usize,
    anchor: ocs_plugin_api::host::AnchorLaw,
    cursor: glam::DVec3,
) -> Result<ocs_plugin_api::host::ParametricChain, String> {
    use ocs_plugin_api::host::{AnchorLaw, ParametricPlane};
    const MIN_RUN: f64 = 1e-6;
    if chain.plane != ParametricPlane::Profile {
        return Err("the re-anchor serves the profile sheet only".to_owned());
    }
    let Some(_) = chain.segments.get(segment) else {
        return Err(format!("drive segment {segment} is out of the design's range"));
    };
    let mut edited = chain.clone();
    {
        let seg = &mut edited.segments[segment];
        let station0 = seg.start_point[0];
        let h0 = seg.start_point[1];
        match anchor {
            AnchorLaw::EndStationToXY => {
                let run = cursor.x - station0;
                if !(run > MIN_RUN) {
                    return Err(format!(
                        "the re-anchored run {run:.6} leaves the section's support (a backward \
                         or tiny station span)"
                    ));
                }
                let w_target = (cursor.y - h0) / run;
                seg.def.ramp = ramp_with_unit_integral(&seg.def.ramp, w_target)?;
                seg.def.length = run;
            }
            AnchorLaw::EndElevation => {
                let w_target = (cursor.y - h0) / seg.def.length;
                seg.def.ramp = ramp_with_unit_integral(&seg.def.ramp, w_target)?;
            }
            AnchorLaw::EndGrade => {
                let run = cursor.x - station0;
                if run.abs() < 1e-12 {
                    return Err(
                        "the grade grip's sheet point shares the segment's start station; the \
                         grade degenerates"
                            .to_owned(),
                    );
                }
                // The end grade = the bearing the cursor's sheet point
                // carries from the segment start; the height follows the
                // law.
                let grade = (cursor.y - h0) / run;
                seg.def.ramp = ramp_with_end_value(&seg.def.ramp, grade);
            }
            AnchorLaw::EndCurvatureScale => {
                // The scale voices the whole driving law (the end value with
                // it): the deterministic bracketed bisection toward the
                // cursor's elevation, seeded at unity.
                let current = seg.def.ramp.clone();
                let length = seg.def.length;
                let voice = |scale: f64| {
                    if !scale.is_finite() {
                        return None;
                    }
                    let scaled = scale_ramp(&current, scale);
                    Some(
                        h0
                            + length
                                * crate::parametric::ramp_integral(&scaled, 1.0),
                    )
                };
                let scale = solve_by_bisection(voice, 1.0, cursor.y)?;
                seg.def.ramp = scale_ramp(&current, scale);
            }
            AnchorLaw::Length => {
                let run = cursor.x - station0;
                if !(run > MIN_RUN) {
                    return Err(format!(
                        "the re-anchored run {run:.6} leaves the section's support (a backward \
                         or tiny station span)"
                    ));
                }
                seg.def.length = run;
            }
        }
    }
    // The downstream re-declaration: the joints propagate from the engine's
    // own sampling (the crest walk, arm-generic): each later segment's
    // start re-bases to the sampled joint (point, sheet direction) and the
    // driving ramp re-bases by the additive shift that restates the joint's
    // slope as the ramp's own start value (one delta on every value; the
    // law's shape preserved).
    let n = edited.segments.len();
    if segment + 1 < n {
        let mut run = edited
            .segments
            .iter()
            .take(segment + 1)
            .map(|s| s.def.length)
            .sum::<f64>();
        let mut joint = crate::parametric::sample_chain(&edited, run)
            .ok()
            .flatten()
            .ok_or_else(|| {
                "the re-anchored front does not sample (the design left its own domain)"
                    .to_owned()
            })?;
        for j in (segment + 1)..n {
            let slope = joint.direction[1] / joint.direction[0];
            {
                let seg = &mut edited.segments[j];
                seg.start_point = joint.point;
                seg.start_direction = slope.atan();
                seg.def.ramp = shift_ramp_to_start_value(&seg.def.ramp, slope);
            }
            run += edited.segments[j].def.length;
            joint = crate::parametric::sample_chain(&edited, run)
                .ok()
                .flatten()
                .ok_or_else(|| {
                    "the re-based chain does not sample (a re-derived joint left the domain)"
                        .to_owned()
                })?;
        }
    }
    Ok(edited)
}

/// Re-derive the ramp so its unit-span integral W(1) becomes `w_target`,
/// moving only the arm's own end-sensitive quantity: the polynomial's top
/// coefficient (the integral's gain per unit of it is 1/(degree+1)), the
/// piecewise/sampled table's last value (the trapezoid plus clamped-tail
/// gain of its own interval). Deterministic closed form, no iterations;
/// the start value and the law's shape stay.
///
/// The standing RAMP law: the declared ramp owns [0, 1] of the record
/// parameter; this re-derivation just re-declares its end-sensitive value
/// as the anchor's law requires, the gain law each arm's own integral
/// form provides.
pub(in crate::app) fn ramp_with_unit_integral(ramp: &Ramp, w_target: f64) -> Result<Ramp, String> {
    let current = crate::parametric::ramp_integral(ramp, 1.0);
    let delta = w_target - current;
    match ramp {
        Ramp::Polynomial { degree, coeffs } => {
            let d = *degree as usize;
            let mut next = *coeffs;
            next[d] += delta * (d as f64 + 1.0);
            Ok(Ramp::Polynomial { degree: *degree, coeffs: next })
        }
        Ramp::Piecewise { count, knots } => {
            let active = (*count).clamp(1, knots.len() as u32) as usize;
            let mut next = *knots;
            if active == 1 {
                // A single knot is the constant law: W = the value.
                next[0].1 += delta;
            } else {
                let m = active - 1;
                let span = knots[m].0 - knots[m - 1].0;
                // The last interval's trapezoid gain plus the constant
                // clamped tail after the last knot.
                let gain = (span * 0.5 + (1.0 - knots[m].0)).max(1e-12);
                next[m].1 += delta / gain;
            }
            Ok(Ramp::Piecewise { count: *count, knots: next })
        }
        Ramp::Sampled { n, values } => {
            let count = (*n).clamp(1, values.len() as u32) as usize;
            let mut next = values.clone();
            if count == 1 {
                next[0] += delta;
            } else {
                let step = 1.0 / (count - 1) as f64;
                next[count - 1] += delta * 2.0 / step;
            }
            Ok(Ramp::Sampled { n: *n, values: next })
        }
    }
}

/// Set one ramp's END value (the law's reading at xi = 1) to `end_value`:
/// the polynomial re-solves its top coefficient from the lower ones, the
/// piecewise/sampled table re-declares its last value. Deterministic; the
/// law's shape stays.
pub(in crate::app) fn ramp_with_end_value(ramp: &Ramp, end_value: f64) -> Ramp {
    match ramp {
        Ramp::Polynomial { degree, coeffs } => {
            let d = *degree as usize;
            let mut next = *coeffs;
            let lower: f64 = coeffs[..d].iter().sum();
            next[d] = end_value - lower;
            Ramp::Polynomial { degree: *degree, coeffs: next }
        }
        Ramp::Piecewise { count, knots } => {
            let active = (*count).clamp(1, knots.len() as u32) as usize;
            let mut next = *knots;
            next[active - 1].1 = end_value;
            Ramp::Piecewise { count: *count, knots: next }
        }
        Ramp::Sampled { n, values } => {
            let count = (*n).clamp(1, values.len() as u32) as usize;
            let mut next = values.clone();
            next[count - 1] = end_value;
            Ramp::Sampled { n: *n, values: next }
        }
    }
}

/// Scale the whole driving ramp by `scale` (the EndCurvatureScale anchor's
/// law: every arm's value takes the factor, the end value with them).
fn scale_ramp(ramp: &Ramp, scale: f64) -> Ramp {
    match ramp {
        Ramp::Polynomial { degree, coeffs } => Ramp::Polynomial {
            degree: *degree,
            coeffs: coeffs.map(|c| c * scale),
        },
        Ramp::Piecewise { count, knots } => Ramp::Piecewise {
            count: *count,
            knots: knots.map(|(xi, value)| (xi, value * scale)),
        },
        Ramp::Sampled { n, values } => Ramp::Sampled {
            n: *n,
            values: values.iter().map(|v| v * scale).collect(),
        },
    }
}

/// Re-base the driving ramp by the additive shift that makes the ramp's
/// own start value read `start_value`: every arm's values rise by the same
/// delta, so the law's SHAPE is preserved while its level re-declares (the
/// downstream legs follow the moved joint exactly this way).
fn shift_ramp_to_start_value(ramp: &Ramp, start_value: f64) -> Ramp {
    let delta = start_value - crate::parametric::ramp_eval(ramp, 0.0);
    match ramp {
        Ramp::Polynomial { degree, coeffs } => Ramp::Polynomial {
            degree: *degree,
            coeffs: coeffs.map(|c| c + delta),
        },
        Ramp::Piecewise { count, knots } => Ramp::Piecewise {
            count: *count,
            knots: knots.map(|(xi, value)| (xi, value + delta)),
        },
        Ramp::Sampled { n, values } => Ramp::Sampled {
            n: *n,
            values: values.iter().map(|v| v + delta).collect(),
        },
    }
}

/// Deterministic bracketed bisection: the voice returns `None` where its
/// probe leaves the model's own domain. The bracket widens around `current`
/// (up to 16 doublings), then bisects 60 times — bit-stable per design: the
/// same move always solves the same value, seeding from the record's own
/// declared field.
pub(in crate::app) fn solve_by_bisection(
    voice: impl Fn(f64) -> Option<f64>,
    current: f64,
    target: f64,
) -> Result<f64, String> {
    let eval = |x: f64| voice(x).map(|h| h - target);
    if let Some(0.0) = eval(current) {
        return Ok(current);
    }
    let mut width = (current.abs() * 2.0).max(1e-9);
    let mut lo: Option<(f64, f64)> = None;
    let mut hi: Option<(f64, f64)> = None;
    for _ in 0..16u32 {
        if let Some(f) = eval(current - width) {
            if f < 0.0 && lo.is_none() {
                lo = Some((current - width, f));
            } else if f > 0.0 && hi.is_none() {
                hi = Some((current - width, f));
            }
        }
        if let Some(f) = eval(current + width) {
            if f < 0.0 && lo.is_none() {
                lo = Some((current + width, f));
            } else if f > 0.0 && hi.is_none() {
                hi = Some((current + width, f));
            }
        }
        if lo.is_some() && hi.is_some() {
            break;
        }
        width *= 2.0;
    }
    let (Some((mut klo, _)), Some((mut khi, _))) = (lo, hi) else {
        return Err(
            "the re-anchor's target lies outside the section's own curvature support (the \
             sloped range or the band refuses)"
                .to_owned(),
        );
    };
    if klo > khi {
        std::mem::swap(&mut klo, &mut khi);
    }
    for _ in 0..60u32 {
        let mid = 0.5 * (klo + khi);
        if !mid.is_finite() {
            break;
        }
        match eval(mid) {
            Some(f) if f < 0.0 => klo = mid,
            Some(_) => khi = mid,
            None => {
                // Unreachable inside a valid bracket (the profile domains are
                // convex in the solved variable); refuse rather than guess.
                return Err("the re-anchor's bisection left its own bracket".to_owned());
            }
        }
    }
    Ok(0.5 * (klo + khi))
}

/// A future that resolves after the pump cadence (the same thread+oneshot
/// shape the dock hover delay uses — `iced::time::sleep` is not available in
/// this iced version).
fn grip_pump_delay(cadence: Duration) -> impl std::future::Future<Output = ()> {
    #[cfg(not(target_arch = "wasm32"))]
    let rx = {
        let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
        std::thread::spawn(move || {
            std::thread::sleep(cadence);
            let _ = tx.send(());
        });
        rx
    };
    #[cfg(target_arch = "wasm32")]
    let rx = {
        // No custom grips exist on wasm (no plugin processes); a resolved
        // future keeps the type uniform.
        let (_tx, rx) = iced::futures::channel::oneshot::channel::<()>();
        rx
    };
    async move {
        let _ = rx.await;
    }
}
