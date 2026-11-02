//! Plugin-declared custom grip points (API v11).
//!
//! One rule governs everything here: plugin DECLARED, plugin OWNED. The host
//! renders, hit-tests and drags these grips; it never interprets them. No
//! host-side entity mutation, constraint solve or record write ever follows
//! from a custom grip — the drag is streamed to the owning plugin as live
//! [`CustomGripEvent`]s (preview through the existing `PreviewWire` channel),
//! and the settle is applied BY THE PLUGIN inside its own `push_undo` scope.
//!
//! Declaration is whole-batch REPLACE per plugin (idempotent at message
//! boundaries); the host applies each batch as a **diff keyed by grip id**, so
//! an unchanged re-declare never invalidates caches or rebuilds the spatial
//! index (the pull model's re-declare stays near-free). All state is display
//! session state: nothing of it is persisted in the drawing, and a set dies
//! with its plugin's process.
//!
//! Points are ABSOLUTE. The host never moves, offsets or auto-follows a
//! custom grip when its `anchor` entity moves; the anchor exists for
//! ownership scoping and hit-test legibility only, and accepts any live
//! entity handle — no type restriction, no host movement semantics.
//!
//! The spatial index is a per-tab uniform hash grid in world space, rebuilt
//! only when a declaration actually changed, and queried by view rectangle
//! (viewport/out-of-view culling) — never a linear scan of every declared
//! grip per cursor move.

use glam::DVec3;
use ocs_plugin_api::host::{
    CustomGripEvent, CustomGripPhase, CustomGripResponse, PluginGrip, PluginGripShape,
};
use std::collections::VecDeque;

use rustc_hash::FxHashMap;

use codec::Handle;

use crate::scene::model::object::GripShape;

/// Per-plugin cap on declared grips. The built-in selection grip budget
/// (GRIPOBJLIMIT / MAX_SELECTED_GRIPS) governs selection-derived grips only;
/// custom grips are plugin display state and get their own honest caps so an
/// unbounded always-visible set cannot silently defeat the budget machinery.
pub(crate) const MAX_PLUGIN_GRIPS_PER_PLUGIN: usize = 4096;
/// Total cap across all plugins on one tab.
pub(crate) const MAX_PLUGIN_GRIPS_PER_TAB: usize = 16_384;

/// One declared grip as kept by the host (display-session state only).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CustomGrip {
    /// Plugin-chosen stable id (key for the declaration diff).
    pub grip_id: u64,
    /// Any live entity handle, for ownership scoping and hit-test legibility
    /// only — never movement semantics.
    pub anchor: Option<Handle>,
    /// ABSOLUTE world point; the host never moves it.
    pub point: DVec3,
    /// Hard drag constraint direction (normalized), when declared.
    pub axis: Option<DVec3>,
    pub shape: GripShape,
}

impl CustomGrip {
    fn from_declared(declared: &PluginGrip) -> Result<Self, String> {
        let point = DVec3::new(declared.point[0], declared.point[1], declared.point[2]);
        if !point.is_finite() {
            return Err(format!("grip {} has a non-finite point", declared.grip_id));
        }
        let axis = match declared.axis {
            Some(a) => {
                let v = DVec3::new(a[0], a[1], a[2]);
                if !v.is_finite() || v.length_squared() < 1e-18 {
                    return Err(format!(
                        "grip {} declares a zero or non-finite axis",
                        declared.grip_id
                    ));
                }
                Some(v.normalize_or_zero())
            }
            None => None,
        };
        let shape = match declared.shape {
            PluginGripShape::Square => GripShape::Square,
            PluginGripShape::Circle => GripShape::Circle,
            PluginGripShape::Triangle => GripShape::Triangle,
        };
        Ok(Self {
            grip_id: declared.grip_id,
            anchor: declared.anchor,
            point,
            axis,
            shape,
        })
    }
}

/// A resolved candidate for hover/hit-testing.
#[derive(Clone, Debug)]
pub(crate) struct GripCandidate {
    pub plugin: String,
    pub grip_id: u64,
    pub anchor: Option<Handle>,
    pub point: DVec3,
    pub axis: Option<DVec3>,
    pub shape: GripShape,
}

/// Uniform world-space hash grid over all declared grips, rebuilt only when
/// a declaration diff changed something. Cell size adapts to the declared
/// set's extent (clamped), so both street-scale and detail-scale sets get
/// bounded cell occupancies. Queries by world rectangle give the viewport /
/// out-of-view culling the requirement demands. Entries carry their owning
/// plugin's id so a query never has to clone or scan the sets.
#[derive(Default)]
struct Grid {
    cell: f64,
    origin: [f64; 2],
    buckets: FxHashMap<i64, Vec<usize>>,
    entries: Vec<(String, CustomGrip)>,
}

impl Grid {
    fn build(sets: &FxHashMap<String, Vec<CustomGrip>>) -> Self {
        let mut entries = Vec::new();
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for (plugin, grips) in sets {
            for grip in grips {
                min[0] = min[0].min(grip.point.x);
                min[1] = min[1].min(grip.point.y);
                max[0] = max[0].max(grip.point.x);
                max[1] = max[1].max(grip.point.y);
                entries.push((plugin.clone(), grip.clone()));
            }
        }
        // Cell size: the set's larger extent over ~256 cells, clamped so a
        // degenerate (single-point) or huge-extent set both behave.
        let cell = ((max[0] - min[0]).max(max[1] - min[1]) / 256.0).clamp(1e-6, 1e15);
        let mut buckets: FxHashMap<i64, Vec<usize>> = FxHashMap::default();
        for (i, (_, grip)) in entries.iter().enumerate() {
            let key = Grid::cell_key(cell, min, grip.point);
            buckets.entry(key).or_default().push(i);
        }
        Self {
            cell,
            origin: min,
            buckets,
            entries,
        }
    }

    fn cell_key(cell: f64, origin: [f64; 2], p: DVec3) -> i64 {
        let cx = ((p.x - origin[0]) / cell).floor() as i64;
        let cy = ((p.y - origin[1]) / cell).floor() as i64;
        (cy & 0x7fff_ffff) << 32 | (cx & 0x7fff_ffff)
    }

    /// Grip indices whose points lie inside the world rectangle (inclusive).
    fn query_rect(&self, min: [f64; 2], max: [f64; 2], out: &mut Vec<usize>) {
        let x0 = ((min[0] - self.origin[0]) / self.cell).floor() as i64;
        let x1 = ((max[0] - self.origin[0]) / self.cell).floor() as i64;
        let y0 = ((min[1] - self.origin[1]) / self.cell).floor() as i64;
        let y1 = ((max[1] - self.origin[1]) / self.cell).floor() as i64;
        if x1 < x0 || y1 < y0 {
            return;
        }
        // A degenerate (near-single-point) declared extent collapses the
        // cell size to its floor, and a zoomed-out view over it can blow the
        // covered-cell count past any sane budget. The entry set is bounded
        // (4096 per plugin, 16 384 per tab), so a linear scan of it stays
        // cheaper than a multi-million-cell probe — and correctness is the
        // same: candidates are px-filtered after projection by every
        // caller.
        let span = (x1 - x0 + 1) as u128 * (y1 - y0 + 1) as u128;
        if span > 4_000_000 {
            out.extend(0..self.entries.len());
            return;
        }
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let key = (cy & 0x7fff_ffff) << 32 | (cx & 0x7fff_ffff);
                if let Some(bucket) = self.buckets.get(&key) {
                    out.extend(bucket.iter().copied());
                }
            }
        }
    }
}

/// Session store of every plugin's declared grips for one drawing tab.
#[derive(Default)]
pub(crate) struct PluginGripStore {
    sets: FxHashMap<String, Vec<CustomGrip>>,
    grid: Option<Grid>,
    total: usize,
}

impl PluginGripStore {
    /// Whether any grip is visible at all (cheap render gate).
    pub(crate) fn is_empty(&self) -> bool {
        self.total == 0
    }

    #[cfg(test)]
    pub(crate) fn total(&self) -> usize {
        self.total
    }

    /// Whole-batch REPLACE with diff semantics keyed by grip id: unchanged
    /// entries leave caches and the spatial index untouched; only a real
    /// change (add / remove / moved / re-shaped) marks the grid dirty.
    pub(crate) fn declare(&mut self, plugin: &str, grips: Vec<PluginGrip>) -> Result<(), String> {
        if grips.len() > MAX_PLUGIN_GRIPS_PER_PLUGIN {
            return Err(format!(
                "plugin {plugin} declares {} grips; the per-plugin cap is {MAX_PLUGIN_GRIPS_PER_PLUGIN}",
                grips.len()
            ));
        }
        let mut parsed = Vec::with_capacity(grips.len());
        for grip in &grips {
            let parsed_grip = CustomGrip::from_declared(grip)?;
            if let Some(other) = parsed
                .iter()
                .find(|g: &&CustomGrip| g.grip_id == parsed_grip.grip_id)
            {
                return Err(format!(
                    "grip id {} declared twice in one batch",
                    other.grip_id
                ));
            }
            parsed.push(parsed_grip);
        }
        let new_total = self.total + parsed.len() - self.sets.get(plugin).map_or(0, Vec::len);
        if new_total > MAX_PLUGIN_GRIPS_PER_TAB {
            return Err(format!(
                "tab would hold {new_total} plugin grips; the per-tab cap is {MAX_PLUGIN_GRIPS_PER_TAB}"
            ));
        }
        let changed = match self.sets.get_mut(plugin) {
            None => {
                if parsed.is_empty() {
                    // Clearing an undeclared set: a no-op.
                    return Ok(());
                }
                self.sets.insert(plugin.to_string(), parsed);
                true
            }
            Some(old) => {
                let any_change =
                    old.len() != parsed.len() || old.iter().zip(parsed.iter()).any(|(a, b)| a != b);
                if any_change {
                    *old = parsed;
                }
                any_change
            }
        };
        self.total = new_total;
        if changed {
            // Rebuild eagerly: every later query is then a pure `&self`
            // lookup, so renders and hover never need mutable access (and
            // unchanged re-declares never pay the rebuild).
            self.grid = Some(Grid::build(&self.sets));
        }
        Ok(())
    }

    /// Remove a plugin's whole set (unload / process death). Returns whether
    /// anything changed.
    pub(crate) fn clear_plugin(&mut self, plugin: &str) -> bool {
        if let Some(set) = self.sets.remove(plugin) {
            self.total -= set.len();
            self.grid = Some(Grid::build(&self.sets));
            true
        } else {
            false
        }
    }

    /// Reap the sets of every plugin the predicate reports as gone. Returns
    /// whether anything changed.
    pub(crate) fn retain_alive(&mut self, mut alive: impl FnMut(&str) -> bool) -> bool {
        let dead: Vec<String> = self
            .sets
            .keys()
            .filter(|plugin| !alive(plugin))
            .cloned()
            .collect();
        let mut changed = false;
        for plugin in dead {
            changed |= self.clear_plugin(&plugin);
        }
        changed
    }

    /// Candidates whose points lie inside the world rectangle, filtered by
    /// `visible` (loaded + alive + enabled). Pure lookup: the grid is
    /// rebuilt eagerly whenever a declaration actually changed.
    pub(crate) fn query_rect(
        &self,
        min: [f64; 2],
        max: [f64; 2],
        visible: &dyn Fn(&str) -> bool,
    ) -> Vec<GripCandidate> {
        if self.total == 0 {
            return Vec::new();
        }
        let Some(grid) = self.grid.as_ref() else {
            return Vec::new();
        };
        let mut hits = Vec::new();
        grid.query_rect(min, max, &mut hits);
        let mut out = Vec::new();
        for idx in hits {
            let Some((plugin, grip)) = grid.entries.get(idx) else {
                continue;
            };
            if !visible(plugin) {
                continue;
            }
            out.push(GripCandidate {
                plugin: plugin.clone(),
                grip_id: grip.grip_id,
                anchor: grip.anchor,
                point: grip.point,
                axis: grip.axis,
                shape: grip.shape,
            });
        }
        out
    }

    /// Resolve one grip by (plugin, grip_id) — e.g. for MCP `grip_drag`.
    pub(crate) fn find(
        &self,
        plugin: &str,
        grip_id: u64,
        visible: &dyn Fn(&str) -> bool,
    ) -> Option<GripCandidate> {
        let set = self.sets.get(plugin)?;
        if !visible(plugin) {
            return None;
        }
        set.iter()
            .find(|g| g.grip_id == grip_id)
            .map(|g| GripCandidate {
                plugin: plugin.to_string(),
                grip_id: g.grip_id,
                anchor: g.anchor,
                point: g.point,
                axis: g.axis,
                shape: g.shape,
            })
    }

    /// Every grip of every visible plugin, in declaration order (for reads;
    /// renders should use the view-rect query instead).
    pub(crate) fn query_all(&self, visible: &dyn Fn(&str) -> bool) -> Vec<GripCandidate> {
        let mut out = Vec::new();
        for (plugin, set) in &self.sets {
            if !visible(plugin) {
                continue;
            }
            for grip in set {
                out.push(GripCandidate {
                    plugin: plugin.clone(),
                    grip_id: grip.grip_id,
                    anchor: grip.anchor,
                    point: grip.point,
                    axis: grip.axis,
                    shape: grip.shape,
                });
            }
        }
        out
    }
}

/// Visibility predicate for custom grips: the declaring plugin must be
/// loaded, alive and not disabled — the live-manager rule of the entity-class
/// labels (PR #1828) applies to grips too. (In-process test sinks count as
/// loaded+alive; the non-test build has none.)
pub(crate) fn visibility(
    disabled: &rustc_hash::FxHashSet<String>,
) -> impl Fn(&str) -> bool + use<'_> {
    move |plugin: &str| {
        !disabled.contains(plugin)
            && (sink_installed(plugin)
                || crate::plugin::external::with_manager(|m| {
                    m.grip_endpoint(plugin).is_some_and(|(_, _, alive)| alive)
                }))
    }
}

/// Whether an in-process sink is installed for this plugin (test-only; the
/// non-test build always answers `false`).
pub(crate) fn sink_installed(plugin_id: &str) -> bool {
    #[cfg(test)]
    {
        IN_PROCESS_SINKS.with(|s| s.borrow().contains_key(plugin_id))
    }
    #[cfg(not(test))]
    {
        let _ = plugin_id;
        false
    }
}

/// Endpoint handle of a live out-of-process plugin, if any.
fn live_endpoint(
    plugin_id: &str,
) -> Option<std::sync::Arc<ocs_plugin_api::process::PluginProcess>> {
    crate::plugin::external::with_manager(|manager| {
        manager
            .grip_endpoint(plugin_id)
            .filter(|(_, _, alive)| *alive)
            .map(|(process, _, _)| process)
    })
}

/// Issue one grip delivery WITHOUT waiting (out-of-process path, API v12):
/// begin/move/settle/cancel alike. `None`: no live endpoint (the pump
/// reschedules; the drag degrades gracefully, never a freeze).
pub(crate) fn send_grip_event(
    app: &mut crate::app::OpenCADStudio,
    tab: usize,
    plugin_id: &str,
    event: CustomGripEvent,
) -> Option<u64> {
    let _ = app;
    let process = live_endpoint(plugin_id)?;
    process.custom_grip_event_send(tab, event).ok()
}

/// Poll (bounded, never blocking the app loop longer than `wait`) for the
/// next outstanding grip answer, serving nested plugin requests through a
/// fresh host session on the way (command frames refused in this context).
pub(crate) fn poll_grip_event(
    app: &mut crate::app::OpenCADStudio,
    tab: usize,
    plugin_id: &str,
    wait: std::time::Duration,
) -> Option<(u64, CustomGripResponse)> {
    let process = live_endpoint(plugin_id)?;
    let mut host = crate::app::plugin_host::HostSession::new(app, tab);
    match process.custom_grip_poll(&mut host, wait) {
        Ok(landed) => landed,
        Err(_) => None,
    }
}

/// Whether a grip delivery is still unanswered on the plugin's connection.
pub(crate) fn grip_move_in_flight(plugin_id: &str) -> bool {
    live_endpoint(plugin_id)
        .map(|process| process.custom_grip_move_in_flight())
        .unwrap_or(false)
}

/// The watchdog (API v12): tear the plugin's connection down after a grip
/// delivery outlived its deadline. The UI loop recovers; the runner reaps.
pub(crate) fn abandon_grip_delivery(plugin_id: &str) {
    if let Some(process) = live_endpoint(plugin_id) {
        process.custom_grip_abandon();
    }
}

/// The descriptor-backed grip drag's host state (API v14, additive; the
/// data-only twin of the active custom grip). The Begin answer handed the
/// host a live design; the pump re-derives the display set LOCALLY per
/// coalesced cursor move (no Move frames cross the IPC), and the plugin's
/// settle callback stays the only write window. Native-only: the type
/// names the host module's wire types, which the wasm build configures
/// out (the wasm run configures the root field out with it).
#[derive(Clone, Debug)]
pub(crate) struct DescriptorGripDrag {
    /// The declaring plugin (the class's owner).
    pub plugin: String,
    /// The class key the dragged instance staged under (its identity stamp).
    pub class_id: String,
    /// The design-record linkage: the parameter/rule rows live in this
    /// XRecord (the record layer stays the only truth; the settle writes it).
    pub design_record: String,
    /// The LAST GOOD design: the current truth the pump re-derives from.
    pub chain: ocs_plugin_api::host::ParametricChain,
    /// The per-move re-derivation tolerance the Begin answer declared.
    pub tolerance: f64,
    /// The staged display set (the handles the per-move restage patches in
    /// place; the entity count must stay stable across the drag).
    pub handles: Vec<Handle>,
    /// Zero-based index of the re-anchored segment.
    pub segment: usize,
    /// The record-model anchor law the drive declares (which quantity the
    /// cursor re-anchors; the host's solver serves the end-side set).
    pub anchor: ocs_plugin_api::host::AnchorLaw,
    /// Whether one move refusal was already noted (one headline per drag).
    pub refusal_noted: bool,
}

/// The currently hovered plugin-declared grip (drives hover highlighting
/// through the same overlay the built-in grip hover uses).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginGripHover {
    pub plugin: String,
    pub grip_id: u64,
}

/// The active custom-grip drag (one at a time, app-scoped, tab-bound).
#[derive(Clone, Debug)]
pub(crate) struct ActivePluginGrip {
    pub tab: usize,
    pub plugin: String,
    pub grip_id: u64,
    pub anchor: Option<Handle>,
    /// The declared absolute point at drag start.
    pub origin_world: DVec3,
    /// Hard-constraint direction (normalized), when declared.
    pub axis: Option<DVec3>,
    /// Latest (constrained) drag point.
    pub last_world: DVec3,
    /// Whether any move was delivered (a bare press-release settles without
    /// moves, exactly like built-in grip clicks).
    pub moved: bool,
}

impl ActivePluginGrip {
    /// Build the event for a phase at the current point.
    pub(crate) fn event(&self, phase: CustomGripPhase, point: DVec3) -> CustomGripEvent {
        CustomGripEvent {
            grip_id: self.grip_id,
            phase,
            point: point.to_array(),
            anchor: self.anchor,
        }
    }
}

/// Delivery bookkeeping for the active custom-grip drag (requirements 8 and
/// the v11.1 hardening): out-of-process deliveries never block the UI loop —
/// every phase is issued without waiting, answers land here in send order,
/// and the pump processes them at message ticks.
///
/// Coalescing: at most one move is in flight (`pending_move` keeps only the
/// newest cursor point; superseded points are DROPPED). The begin, settle and
/// cancel phases are never coalesced or dropped. The watchdog deadline
/// guards the settle: a plugin that misses its deadline has its drag end as
/// a cancel, never a freeze — the host's strike ledger keeps the connection
/// on the first strike and tears it down on the third miss inside the
/// window.
#[derive(Default)]
pub(crate) struct PluginGripDeliveries {
    /// Newest cursor point not yet sent (coalescing park).
    pending_move: Option<DVec3>,
    /// Request id of the move currently in flight.
    in_flight_move: Option<u64>,
    /// Outstanding deliveries in send order: (request id, phase). Answers
    /// arrive in the same order (the runner is serial).
    outstanding: VecDeque<(u64, CustomGripPhase)>,
    /// Landed answers awaiting the pump, in order.
    landed: VecDeque<(CustomGripPhase, CustomGripResponse)>,
    /// Watchdog: set when the settle was issued; the drag must be finalized
    /// by this deadline or the delivery is abandoned.
    settle_deadline: Option<std::time::Instant>,
    /// The drag's settle phase has been issued (in-process deliveries: this
    /// is normally cleared within the same call; out-of-process it stays set
    /// until the pump collects the settle answer).
    settling: bool,
}

impl PluginGripDeliveries {
    /// Park the newest cursor point; drops whatever was parked before.
    pub(crate) fn park_move(&mut self, point: DVec3) {
        self.pending_move = Some(point);
    }

    /// The next parked move to send, if the move slot is free.
    pub(crate) fn take_sendable_move(&mut self) -> Option<DVec3> {
        if self.in_flight_move.is_some() {
            None
        } else {
            self.pending_move.take()
        }
    }

    /// Map a poll's answer id to its phase (removing it from the outstanding
    /// queue). `None`: not a tracked grip delivery.
    pub(crate) fn resolve(&mut self, id: u64) -> Option<CustomGripPhase> {
        if self.in_flight_move == Some(id) {
            self.in_flight_move = None;
        }
        let position = self.outstanding.iter().position(|(rid, _)| *rid == id)?;
        let (_, phase) = self.outstanding.remove(position)?;
        Some(phase)
    }

    pub(crate) fn track(&mut self, id: u64, phase: CustomGripPhase) {
        if phase == CustomGripPhase::Move {
            self.in_flight_move = Some(id);
        }
        self.outstanding.push_back((id, phase));
    }

    pub(crate) fn is_move_in_flight(&self) -> bool {
        self.in_flight_move.is_some()
    }

    #[cfg(test)]
    pub(crate) fn is_settle_ready(&self) -> bool {
        !self.settling
            && self.in_flight_move.is_none()
            && self.pending_move.is_none()
            && self
                .outstanding
                .iter()
                .all(|(_, phase)| !matches!(phase, CustomGripPhase::Move))
    }

    pub(crate) fn has_landed(&self) -> bool {
        !self.landed.is_empty()
    }

    pub(crate) fn push_landed(&mut self, phase: CustomGripPhase, response: CustomGripResponse) {
        self.landed.push_back((phase, response));
    }

    pub(crate) fn pop_landed(&mut self) -> Option<(CustomGripPhase, CustomGripResponse)> {
        self.landed.pop_front()
    }

    /// Watchdog state: the settle was issued at `now` with the settle floor.
    pub(crate) fn begin_settle_watch(
        &mut self,
        floor: std::time::Duration,
        now: std::time::Instant,
    ) {
        self.settling = true;
        self.settle_deadline = Some(now + floor);
    }

    pub(crate) fn settle_landed(&mut self) {
        self.settling = false;
        self.settle_deadline = None;
    }

    pub(crate) fn is_settling(&self) -> bool {
        self.settling
    }

    /// The watchdog fires when the settle is outstanding past its deadline.
    pub(crate) fn settle_timed_out(&self, now: std::time::Instant) -> bool {
        self.settling && self.settle_deadline.is_some_and(|deadline| now >= deadline)
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.settling
            || self.in_flight_move.is_some()
            || self.pending_move.is_some()
            || !self.outstanding.is_empty()
            || !self.landed.is_empty()
    }
}

/// Route one custom-grip event to its owning plugin and return its response.
///
/// Delivery order: in-process test sinks first (they drive the acceptance
/// tests inside the host binary). The panic guard applies to in-process
/// handlers. Out-of-process owners are served by the ASYNC delivery path
/// (`send_grip_event` + pump collection — the v11.1 hardening: no delivery
/// ever blocks the UI loop); this synchronous entry is the in-process test
/// surface. Returns `None` when the owner has no in-process sink: the drag
/// machinery then uses the async route (never an error loop).
pub(crate) fn dispatch_grip_event(
    app: &mut crate::app::OpenCADStudio,
    tab: usize,
    plugin_id: &str,
    event: CustomGripEvent,
) -> Option<CustomGripResponse> {
    let sink = take_in_process_sink(plugin_id)?;
    let mut host = crate::app::plugin_host::HostSession::new(app, tab);
    crate::plugin::guard("on_custom_grip_event", move || sink(&mut host, event))
}

// ── In-process sinks (test-only) ──────────────────────────────────────────
#[cfg(test)]
type GripEventSink = std::rc::Rc<
    dyn for<'a> Fn(
        &mut crate::app::plugin_host::HostSession<'a>,
        CustomGripEvent,
    ) -> CustomGripResponse,
>;

#[cfg(test)]
thread_local! {
    static IN_PROCESS_SINKS: std::cell::RefCell<rustc_hash::FxHashMap<String, GripEventSink>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

#[cfg(test)]
fn take_in_process_sink(
    plugin_id: &str,
) -> Option<
    std::rc::Rc<
        dyn for<'a> Fn(
            &mut crate::app::plugin_host::HostSession<'a>,
            CustomGripEvent,
        ) -> CustomGripResponse,
    >,
> {
    IN_PROCESS_SINKS.with(|s| s.borrow().get(plugin_id).cloned())
}

#[cfg(test)]
pub(crate) fn install_in_process_sink(
    plugin_id: &str,
    sink: impl for<'a> Fn(
            &mut crate::app::plugin_host::HostSession<'a>,
            CustomGripEvent,
        ) -> CustomGripResponse
        + 'static,
) {
    IN_PROCESS_SINKS.with(|s| {
        s.borrow_mut()
            .insert(plugin_id.to_string(), std::rc::Rc::new(sink));
    });
}

#[cfg(test)]
pub(crate) fn remove_in_process_sink(plugin_id: &str) {
    IN_PROCESS_SINKS.with(|s| {
        s.borrow_mut().remove(plugin_id);
    });
}

#[cfg(not(test))]
fn take_in_process_sink(
    _plugin_id: &str,
) -> Option<
    std::rc::Rc<
        dyn for<'a> Fn(
            &mut crate::app::plugin_host::HostSession<'a>,
            CustomGripEvent,
        ) -> CustomGripResponse,
    >,
> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(id: u64, point: [f64; 3]) -> PluginGrip {
        PluginGrip {
            grip_id: id,
            anchor: None,
            point,
            axis: None,
            shape: PluginGripShape::default(),
        }
    }

    #[test]
    fn replace_batch_diff_keeps_unchanged_declarations_free() {
        let mut store = PluginGripStore::default();
        let batch: Vec<PluginGrip> = (0..1000)
            .map(|i| declared(i, [i as f64, (i % 7) as f64, 0.0]))
            .collect();
        store.declare("p", batch).expect("declare");
        assert_eq!(store.total(), 1000);
        // Re-declare the identical batch: no diff, no rebuild.
        let batch: Vec<PluginGrip> = (0..1000)
            .map(|i| declared(i, [i as f64, (i % 7) as f64, 0.0]))
            .collect();
        store.declare("p", batch).expect("re-declare");
        assert_eq!(store.total(), 1000);
        // A real change rebuilds, and the next query reflects it.
        store
            .declare("p", vec![declared(0, [42.0, 0.0, 0.0])])
            .expect("changed");
        let hits = store.query_rect([41.0, -1.0], [43.0, 1.0], &|_| true);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].grip_id, 0);
    }

    #[test]
    fn clear_and_reap() {
        let mut store = PluginGripStore::default();
        store
            .declare("a", vec![declared(1, [0.0, 0.0, 0.0])])
            .unwrap();
        store
            .declare("b", vec![declared(2, [10.0, 0.0, 0.0])])
            .unwrap();
        assert!(store.clear_plugin("a"));
        assert_eq!(store.total(), 1);
        store
            .declare("c", vec![declared(3, [20.0, 0.0, 0.0])])
            .unwrap();
        assert!(store.retain_alive(|p| p != "b" && p != "c"));
        assert_eq!(store.total(), 0);
        assert!(store
            .query_rect([-1e9, -1e9], [1e9, 1e9], &|_| true)
            .is_empty());
    }

    #[test]
    fn visibility_filter_applies_at_query_time() {
        let mut store = PluginGripStore::default();
        store
            .declare("dead", vec![declared(1, [0.0, 0.0, 0.0])])
            .unwrap();
        let hits = store.query_rect([-1.0, -1.0], [1.0, 1.0], &|p| p != "dead");
        assert!(hits.is_empty(), "invisible plugins contribute nothing");
        let hits = store.query_rect([-1.0, -1.0], [1.0, 1.0], &|_| true);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn duplicate_ids_in_one_batch_are_refused() {
        let mut store = PluginGripStore::default();
        let err = store
            .declare("p", vec![declared(7, [0.0; 3]), declared(7, [1.0; 3])])
            .unwrap_err();
        assert!(err.contains("twice"), "got: {err}");
    }

    #[test]
    fn caps_are_enforced() {
        let mut store = PluginGripStore::default();
        let huge: Vec<PluginGrip> = (0..=MAX_PLUGIN_GRIPS_PER_PLUGIN)
            .map(|i| declared(i as u64, [0.0; 3]))
            .collect();
        assert!(store.declare("p", huge).is_err());
    }

    #[test]
    fn move_channel_coalesces_latest_wins() {
        let mut channel = PluginGripDeliveries::default();
        for i in 0..1000 {
            channel.park_move(DVec3::new(i as f64, 0.0, 0.0));
        }
        let first = channel.take_sendable_move().expect("one sendable move");
        assert_eq!(first.x, 999.0, "superseded points are dropped, latest wins");
        // Coalescing: the slot is busy while a move is in flight.
        channel.track(7, CustomGripPhase::Move);
        assert!(
            channel.take_sendable_move().is_none(),
            "only one move in flight"
        );
        channel.park_move(DVec3::new(1000.0, 0.0, 0.0));
        assert!(
            !channel.is_settle_ready(),
            "the settle must wait for the channel"
        );
        // The answer lands and maps back to its phase.
        assert_eq!(channel.resolve(7), Some(CustomGripPhase::Move));
        assert!(channel.is_move_in_flight() == false);
        let second = channel
            .take_sendable_move()
            .expect("drains the newest next");
        assert_eq!(second.x, 1000.0);
        channel.track(8, CustomGripPhase::Move);
        assert!(!channel.is_settle_ready());
        assert_eq!(channel.resolve(8), Some(CustomGripPhase::Move));
        assert!(channel.is_settle_ready());
        // Watchdog: the settle is issued, the plugin never answers.
        channel.track(9, CustomGripPhase::Settle);
        let now = std::time::Instant::now();
        channel.begin_settle_watch(std::time::Duration::from_secs(10), now);
        assert!(!channel.settle_timed_out(now));
        assert!(!channel.is_settle_ready());
        assert!(channel.settle_timed_out(now + std::time::Duration::from_secs(11)));
        channel.settle_landed();
        assert!(!channel.settle_timed_out(now + std::time::Duration::from_secs(11)));
    }
}
