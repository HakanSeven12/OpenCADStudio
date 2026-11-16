//! The widget-mount bridge handler (`SCN-014`): `app::Message::WidgetMountMsg`
//! → the registered surface's widget state machines.
//!
//! ONE route arm, one handler, ONE settle policy. The native mount
//! (`plugin_widget_mount.rs`) emits its host-local vocabulary and owns the
//! routing INTO the machines (`apply_mount_msg`); this is the app half:
//!
//! 1. find the registered surface by the message's surface id
//!    ([`MountRegistry::handle_for_surface_owned`], the app-owned registry
//!    since SCN-016) — an unknown/unregistered id
//!    is a graceful no-op (no panic, nothing routed, no event exists);
//! 2. FORWARD: `apply_mount_msg` runs and every emitted [`WidgetEvent`]
//!    leaves through the COLLECTING sink ([`CollectingSink`]) — the
//!    pipeline seam whose drain this handler owns. SCN-025 (the API-24
//!    crossing's attach — the label was API-23 pre-merge, re-versioned at
//!    SCN-026) makes the drain LIVE: the drained events forward to the
//!    owning plugin's v24 notify router
//!    (`broadcast_panel_scene_events`, bounded batches — [`Self::forward_mount_events`]),
//!    the exactly-once law riding the drain itself (a re-drain cannot
//!    re-produce a forwarded event);
//! 3. SETTLE (the bridge-forwards-then-settle policy): a command message's
//!    originating address clears its exactly-once mark AFTER the forward —
//!    the in-flight guard re-arms once the handler completes, so the next
//!    genuine click emits again. A pending CONFIRMATION refuses settle
//!    here (its dialog owns the armed interaction); rows/selection/scroll/
//!    focus arm nothing and settle nothing.
//!
//! A `PromptRequested` routing records the bridge's prompt tick-count and
//! collects NO event — the real confirm dialog LANDED (SCN-016): the arm's
//! tail `arm_widget_prompt_modal` (`app::update::widget_prompt`) runs the
//! SHOW POLICY and the app's existing modal machinery displays the desc
//! (`ModalKind::WidgetPrompt`). Host-local only: `Message` carries no
//! serialization derives (no wire type), so the appended variants' only
//! consumers are these arms.

use iced::keyboard::{Key, Modifiers};
#[cfg(test)]
use iced::keyboard::key as keyboard_key; // the keyboard suites' Named-alias
use iced::{Point, Task};

// The v24 notify leg's fresh-frame cap (SCN-025): a forwarded batch never
// exceeds it. Desktop-only (the plugin manager is desktop machinery).
#[cfg(not(target_arch = "wasm32"))]
use ocs_plugin_api::ipc::protocol::MAX_WIDGET_EVENT_ROWS;

use crate::app::{Message, OpenCADStudio};
use crate::ui::window::plugin_widget_mount::{
    CollectingSink, MountRegistry, MountRouting, SurfaceMountHandle, WidgetMountMsg,
    apply_mount_msg, msg_surface_id, route_mount_table_msg_owned,
};
#[cfg(test)]
use crate::ui::window::plugin_widgets::WindowSpec;
use crate::ui::window::plugin_widgets_dispatch::DispatchKey;
#[cfg(test)]
use crate::ui::window::plugin_widgets_tables::{CellFocus, TableUiState};
use ocs_plugin_api::scene::StableId;
use ocs_plugin_api::widgets::WidgetEvent;

/// What ONE bridge route did with one mount message — the handler's total
/// outcome (the headless assertion surface; the app half only inspects the
/// drained events for now).
pub(crate) struct MountBridgeOutcome {
    /// The machines' routing verdict.
    // Consumed by the assertion tests (the app arm needs none of it yet).
    #[cfg_attr(not(test), allow(dead_code))]
    pub routing: MountRouting,
    /// The events the collecting sink drained, in emission order — the
    /// forward half of the policy. Empty for refusals and prompt requests.
    pub events: Vec<WidgetEvent>,
    /// Whether the settle half cleared the originating address's
    /// exactly-once mark: `true` only for an APPLIED command message.
    // Read by the assertion tests (the app arm infers nothing from it).
    #[cfg_attr(not(test), allow(dead_code))]
    pub settled: bool,
}

/// The dispatcher address a mount message settles — command messages only
/// (rows/selection/scroll/focus never arm the dispatcher's exactly-once
/// guard, so they settle nothing).
fn settle_address_of(msg: &WidgetMountMsg) -> Option<DispatchKey> {
    match msg {
        WidgetMountMsg::ButtonClicked { widget_id, .. } => Some(DispatchKey::Button(*widget_id)),
        WidgetMountMsg::MenuItemClicked {
            menu_id, command_id, ..
        } => Some(DispatchKey::MenuItem(*menu_id, command_id.clone())),
        WidgetMountMsg::RowActivated { .. }
        | WidgetMountMsg::RowExpandedToggled { .. }
        | WidgetMountMsg::SelectionChanged { .. }
        | WidgetMountMsg::Scrolled { .. }
        | WidgetMountMsg::FocusMoved { .. }
        // Pointer messages capture/never arm the dispatcher (§14); the
        // keyboard route's own tail variants never reach this bridge core
        // either (their arms pre-route them); the SCN-028 typed-table
        // vocabulary settles through the TABLE machine's own in-flight
        // marks (route_mount_table_msg_owned), never the dispatcher.
        | WidgetMountMsg::TableRowPressed { .. }
        | WidgetMountMsg::TableCellCommitted { .. }
        | WidgetMountMsg::TableTailReached { .. }
        | WidgetMountMsg::PointerPressed { .. }
        | WidgetMountMsg::PointerMoved { .. }
        | WidgetMountMsg::PointerReleased { .. }
        | WidgetMountMsg::KeyPressed { .. }
        | WidgetMountMsg::PointerReleasedAnywhere => None,
    }
}

/// Route ONE mount message into `handle`'s machines — the bridge core the
/// `Message::WidgetMountMsg` arm and the tests share.
///
/// Forward-then-settle (see the module docs): the sink drains after the
/// machines returned, the settle runs after the sink drained. The counters
/// move with the verdict (routed/prompts — the §16 tick-count handles).
///
/// The registry rides as the FIRST parameter (SCN-016's app-owned
/// migration): every lifecycle probe is the app-owned store's own, so this
/// core asks the caller for the store it updates — the arm passes
/// `&mut self.mount_registry`.
pub(crate) fn route_mount_msg_owned(
    registry: &mut MountRegistry,
    handle: &SurfaceMountHandle,
    msg: &WidgetMountMsg,
) -> MountBridgeOutcome {
    let mut sink = CollectingSink::default();
    // (1) FORWARD — the machines stay the semantic authority; a refusal or
    // a prompt request leaves NO event (the machines' own gates).
    let routing = {
        let mut state = handle.state.borrow_mut();
        let mut dispatcher = handle.dispatcher.borrow_mut();
        apply_mount_msg(&mut state, &mut dispatcher, msg, &mut sink)
    };
    // (2) the sink drains (the pre-wire consumption point), THEN
    // (3) settle the forwarded command's originating address.
    let settled = match (&routing, settle_address_of(msg)) {
        (MountRouting::Applied { .. }, Some(key)) => handle
            .dispatcher
            .borrow_mut()
            .settle(key),
        _ => false,
    };
    registry.record_message_routed_owned();
    if matches!(routing, MountRouting::PromptRequested { .. }) {
        registry.record_prompt_surfaced_owned();
        // SCN-020: name the armed interaction for the keyboard layer's
        // Escape cancel (the hint; the dispatcher's dismiss_prompt stays
        // the cancel edge). SCN-016: this SAME record arms the app's prompt
        // MODAL (the desc read in `app::view::modal` — the registry's
        // `prompts` map is the modal's content source).
        if let Some(key) = settle_address_of(msg) {
            registry.mount_record_prompt_armed_owned(msg_surface_id(msg), key);
        }
    }
    let events = sink.take();
    MountBridgeOutcome {
        routing,
        events,
        settled,
    }
}

/// The headless tests' facade: the SAME [`route_mount_msg_owned`] core over
/// the mount module's cfg(test) registry cell (the pre-migration static
/// signature — see the seam block in `plugin_widget_mount.rs`).
#[cfg(test)]
pub(crate) fn route_mount_msg(
    handle: &SurfaceMountHandle,
    msg: &WidgetMountMsg,
) -> MountBridgeOutcome {
    crate::ui::window::plugin_widget_mount::with_test_registry_mut(|registry| {
        route_mount_msg_owned(registry, handle, msg)
    })
}

// ---------------------------------------------------------------------------
// SCN-020 — the coalescing JOIN and the keyboard join (app-side routing)
// ---------------------------------------------------------------------------

/// What ONE coalesced mount-move entry did — the entry's total shape (the
/// latest-wins contract's test surface).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MountMoveOutcome {
    /// `false` = the surface is unregistered: a graceful no-op, nothing
    /// stored, nothing counted (the unknown-surface through-line).
    pub registered: bool,
    /// `true` = the arriving move OVERWROTE an owed move still in flight
    /// (the newest one wins; the older was counted superseded, never
    /// queued, never answered).
    pub superseded: bool,
}

/// The mount move's coalescing ENTRY (SCN-020) — the app-side arm of the
/// registry's owed-slot JOIN, with the SAME latest-wins contract the
/// viewport coalescer (`app::update::viewport::defer_pointer_move`) estab
/// lished: while a mount move is in flight, a newer move OVERWRITES the
/// owed slot; the tick/flush delivers ONLY the last (≤1 mount call per
/// surface per tick — [`MountCounters::mount_move_calls`]); NO machine
/// ever sees a raw move (§14 host-local: the flush updates the surface's
/// [`HoverState`](crate::ui::window::plugin_widget_mount::HoverState) ONLY).
///
/// This arm is deliberately CHEAP per event (one slot write): the mount
/// never runs a pass per move, so it defers unconditionally — there is no
/// pass-cost gate to measure (the viewport's measured-gate discipline reduced
/// to its invariant).
pub(crate) fn route_mount_move_owned(
    registry: &mut MountRegistry,
    surface_id: StableId,
    position: Point,
) -> MountMoveOutcome {
    match registry.mount_move_defer_owned(surface_id, position) {
        Some(superseded) => MountMoveOutcome {
            registered: true,
            superseded,
        },
        None => MountMoveOutcome {
            registered: false,
            superseded: false,
        },
    }
}

/// The press route: pointer capture ON for the surface
/// ([`MountRegistry::mount_press_owned`]); the press also makes the surface the
/// keyboard's target (the desktop input model). `false` = unknown surface
/// (graceful).
pub(crate) fn route_mount_press_owned(registry: &mut MountRegistry, surface_id: StableId) -> bool {
    registry.mount_press_owned(surface_id)
}

/// The release route: capture completes (release ANYWHERE — the pure
/// completion contract; this iced revision's `mouse_area` releases only
/// inside the area, so the window-level catcher is the app-view seam, see
/// [`OpenCADStudio::on_mount_key`]'s sibling note) and the surface's owed
/// move — if the gesture left one — is delivered NOW (exactly ≤1). Returns
/// the delivered count.
pub(crate) fn route_mount_release_owned(registry: &mut MountRegistry, surface_id: StableId) -> usize {
    registry.mount_pointer_released_owned(surface_id)
}

#[cfg(test)]
pub(crate) fn route_mount_move(surface_id: StableId, position: Point) -> MountMoveOutcome {
    crate::ui::window::plugin_widget_mount::with_test_registry_mut(|registry| {
        route_mount_move_owned(registry, surface_id, position)
    })
}

#[cfg(test)]
pub(crate) fn route_mount_press(surface_id: StableId) -> bool {
    crate::ui::window::plugin_widget_mount::with_test_registry_mut(|registry| {
        route_mount_press_owned(registry, surface_id)
    })
}

#[cfg(test)]
pub(crate) fn route_mount_release(surface_id: StableId) -> usize {
    crate::ui::window::plugin_widget_mount::with_test_registry_mut(|registry| {
        route_mount_release_owned(registry, surface_id)
    })
}

/// The keyboard routing RESOLUTION (SCN-020, the pure layer): one key event
/// against the mounted keyboard target → the mount messages to route, in
/// order, through the ordinary bridge (`Message::WidgetMountMsg` →
/// [`route_mount_msg_owned`]). The resolution:
///
/// - the TARGET surface is [`MountRegistry::mount_key_surface_owned`]'s (the
///   press-captured surface, else the sole registered mount — the
///   keyboard-only §16 path; `None` → no messages, graceful);
/// - translation is [`key_route`]'s (the mount module's pure keyboard
///   layer — arrows/Tab/Enter over the machines' ring; disabled skipped BY
///   the state layer; chords own nothing);
/// - ESCAPE is the machine-direct cancel (no message): the armed prompt
///   dismissal runs HERE ([`MountRegistry::mount_escape_cancel_owned`]); the
///   drag cancel is deliberately NOT coupled in (SCN-021 adds its own arm).
///
/// The "app-ish state" the original brief sketched stays UNPARAMETERIZED
/// in v1 on purpose: the mount's routes resolve entirely through the
/// app-owned registry (`self.mount_registry` since SCN-016) — the store
/// rides as this core's FIRST parameter, and there is still nothing of
/// `OpenCADStudio` beyond it to read.
///
/// SCN-025's D1 fold-back (the drift-killer): this app fn DELEGATES to the
/// mount-file seam ([`crate::ui::window::plugin_widget_mount::bench_handle_mount_key_owned`])
/// instead of restating its body — the seam is THE canonical route body
/// (its own doc carries the rationale; the one-direction dependency means a
/// route change edits ONE file, and the bench drives the same code the app
/// runs). Zero behavior: the delegation is byte-identical to the restated
/// body it replaces, and the keyboard suites pin that.
pub(crate) fn handle_mount_key_owned(
    registry: &mut MountRegistry,
    key: Key,
    modifiers: Modifiers,
) -> Vec<WidgetMountMsg> {
    crate::ui::window::plugin_widget_mount::bench_handle_mount_key_owned(registry, key, modifiers)
}

/// The headless tests' facade: the SAME [`handle_mount_key_owned`] core
/// over the mount module's cfg(test) registry cell (the pre-migration
/// static signature — see the seam block in `plugin_widget_mount.rs`).
#[cfg(test)]
pub(crate) fn handle_mount_key(key: Key, modifiers: Modifiers) -> Vec<WidgetMountMsg> {
    crate::ui::window::plugin_widget_mount::with_test_registry_mut(|registry| {
        handle_mount_key_owned(registry, key, modifiers)
    })
}

/// The typed-table keyboard route's REACH-THROUGH (SCN-020): the typed
/// table machine is state-layer-complete and the mount's table placeholder
/// is not interactive yet, so the table's keyboard layer is exposed as the
/// PURE consumption [`mount_table_key_route`] (tested at the mount module;
/// the typed-table MOUNT integration rides it). cfg(test): the mount-side
/// core ([`mount_table_key_route_owned`]) writes the app-owned registry's
/// nav-step probe, so this reach-through is the tests' cell-backed seam —
/// the TABLE route surface stays discoverable here for the MOUNT
/// integration of the later task.
#[cfg(test)]
pub(crate) fn handle_mount_table_key(
    state: &mut TableUiState,
    cell_cursor: &mut Option<CellFocus>,
    key: Key,
    modifiers: Modifiers,
    spec: &WindowSpec,
    column_order: &[StableId],
) -> Option<crate::ui::window::plugin_widget_mount::MountTableKeyStep> {
    crate::ui::window::plugin_widget_mount::mount_table_key_route(
        state,
        cell_cursor,
        key,
        modifiers,
        spec,
        column_order,
    )
}

impl OpenCADStudio {
    /// The `Message::WidgetMountMsg` arm's handler: route one
    /// builder-emitted mount gesture into its registered surface's
    /// machines (forward → settle; see [`route_mount_msg_owned`]) and drop
    /// the drained events at the collecting seam (the publish hang-off is
    /// the ADR-004 crossing — SCN-015).
    ///
    /// UNKNOWN surface: a graceful no-op — no panic, nothing routed, no
    /// event; a REGISTERED surface's stale content (a cross-surface or
    /// unknown-address message) refuses INSIDE the machines, and the app
    /// never sees a routed event either way.
    ///
    /// SCN-020: the mount's POINTER messages do NOT take the machine
    /// route — they OWN the input layer ([`route_mount_press`]/[`route_mount_move`]
    /// /[`route_mount_release`]: capture + the latest-wins coalescer, §14
    /// host-local) and are resolved before the machine path. The
    /// window-level release variant completes EVERY armed capture (the
    /// release-anywhere contract; the catcher rides
    /// `app::view::mod`'s subscription composition).
    ///
    /// SCN-016: the KEY-variant is the live keyboard join — the app's real
    /// listener hands keys to the complete [`Self::on_mount_key`] route.
    /// A `PromptRequested` machine routing ARMS the app's prompt-modal
    /// flow: the reveal state lives on the app (see
    /// `app::update::widget_prompt`), the desc rides in the registry
    /// (unchanged), and the modal machinery itself is the app's existing
    /// modal overlay.
    pub(in crate::app) fn on_widget_mount_msg(
        &mut self,
        msg: WidgetMountMsg,
    ) -> Task<Message> {
        match &msg {
            WidgetMountMsg::PointerPressed { surface_id, .. } => {
                route_mount_press_owned(&mut self.mount_registry.borrow_mut(), *surface_id);
                return Task::none();
            }
            WidgetMountMsg::PointerMoved { surface_id, position } => {
                let _ = route_mount_move_owned(&mut self.mount_registry.borrow_mut(), *surface_id, *position);
                return Task::none();
            }
            WidgetMountMsg::PointerReleased { surface_id, .. } => {
                route_mount_release_owned(&mut self.mount_registry.borrow_mut(), *surface_id);
                return Task::none();
            }
            // The window-level release pulled inside any panel's area is
            // the ordinary release route; the ANYWHERE catcher (this
            // variant) completes every capture that still stands.
            WidgetMountMsg::PointerReleasedAnywhere => {
                let captured = self.mount_registry.borrow().captured_surfaces();
                for surface_id in captured {
                    route_mount_release_owned(
                        &mut self.mount_registry.borrow_mut(),
                        surface_id,
                    );
                }
                return Task::none();
            }
            // SCN-016's keyboard join: live keys land on the complete
            // route (resolve → translate → route → apply) — see
            // [`Self::on_mount_key`].
            WidgetMountMsg::KeyPressed { key, modifiers } => {
                return self.on_mount_key(key.clone(), *modifiers);
            }
            // SCN-028's typed-table join: the table vocabulary owns the
            // typed-table bridge route (`route_mount_table_msg_owned` —
            // the canonical body lives in the mount file, the D1
            // fold-back discipline), and the drained batch forwards
            // through the SAME ONE notify path as every other bridge
            // emission ([`Self::forward_mount_events`]).
            WidgetMountMsg::TableRowPressed { .. }
            | WidgetMountMsg::TableCellCommitted { .. }
            | WidgetMountMsg::TableTailReached { .. } => {
                let outcome = route_mount_table_msg_owned(
                    &mut self.mount_registry.borrow_mut(),
                    &msg,
                );
                self.forward_mount_events(msg_surface_id(&msg), outcome.events);
                return Task::none();
            }
            _ => {}
        }
        let Some(handle) = self
            .mount_registry
            .borrow()
            .handle_for_surface_owned(msg_surface_id(&msg))
        else {
            // Unregistered surface: the bridge refuses gracefully.
            return Task::none();
        };
        let outcome =
            route_mount_msg_owned(&mut self.mount_registry.borrow_mut(), &handle, &msg);
        // SCN-025 — the sink goes LIVE: the drained events forward to the
        // v24 panel-scene-events router (bounded batches ≤
        // [`MAX_WIDGET_EVENT_ROWS`]; exactly-once — the drain empties the
        // buffer; the ONE helper both draining arms share).
        self.forward_mount_events(msg_surface_id(&msg), outcome.events);
        // SCN-016: the arm's prompt-modal seam — the machine surfaced an
        // armed confirmation, so the app's modal flow picks it up (open
        // NOW when no other modal holds the slot; queued behind it
        // otherwise — the SHOW POLICY in `app::update::widget_prompt`).
        if matches!(outcome.routing, MountRouting::PromptRequested { .. }) {
            self.arm_widget_prompt_modal(msg_surface_id(&msg));
        }
        Task::none()
    }

    /// The mount keyboard route's app join (SCN-020): resolve the key
    /// ([`handle_mount_key_owned`]) and route EVERY produced message
    /// through the ordinary `Message::WidgetMountMsg` bridge half.
    ///
    /// WIRING STATUS (SCN-016 landed the join): the app's key event flow
    /// hands a pressed key here through `Message::WidgetMountMsg(
    /// WidgetMountMsg::KeyPressed { .. })` — the mount-key listener
    /// `view/mod.rs`'s subscription composition selects exactly while a
    /// mount surface holds the keyboard (and, for the prompt modal, while
    /// it shows, for Escape's machine-direct dismissal). While no mount
    /// owns the keyboard the ordinary `main_window_event` listener runs —
    /// the unchanged app flow.
    pub(in crate::app) fn on_mount_key(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        let msgs = handle_mount_key_owned(&mut self.mount_registry.borrow_mut(), key, modifiers);
        if msgs.is_empty() {
            // Escape's machine-direct cancel CAN consume an armed prompt
            // (the route message list is empty all the same) — the prompt
            // modal's close follows its consumed desc here.
            self.close_widget_prompt_modal_if_consumed();
            return Task::none();
        }
        Task::batch(
            msgs.into_iter()
                .map(|msg| self.on_widget_mount_msg(msg))
                .collect::<Vec<_>>(),
        )
    }

    /// The mount side of the move-coalesce tick (SCN-020): answer every
    /// surface's LAST owed move ([`MountRegistry::mount_flush_moves_owned`]
    /// — ≤1 mount call per surface per tick, host-local state only). Joined
    /// onto the EXISTING `Message::MoveCoalesceTick` arm
    /// (`app::update::mod.rs`) so both coalescers answer on the same
    /// tick channel. SCN-016: the tick subscription's PUMP condition
    /// (`view/mod.rs`) also flows while the MOUNT
    /// ([`MountRegistry::any_move_owed`]) has a move owed.
    pub(in crate::app) fn on_mount_move_coalesce_tick(&mut self) -> Task<Message> {
        let _ = self.mount_registry.borrow_mut().mount_flush_moves_owned();
        Task::none()
    }

    /// THE one v24 event-forwarding route (SCN-025 — the sink goes LIVE):
    /// the bridge arms' drained events forward to the panel-scene-events
    /// router ([`ocs_plugin_api::process::PluginManager::broadcast_panel_scene_events`]),
    /// in bounded batches (≤ [`MAX_WIDGET_EVENT_ROWS`] — the notify leg's
    /// fresh-frame cap; a producer splits, never grows a batch). BOTH
    /// draining arms funnel here — the mount bridge's arm (above) and the
    /// prompt-decision arm (`app::update::widget_prompt`) — so there is no
    /// second forwarding code path to drift.
    ///
    /// Exactly-once: the drain (`CollectingSink::take`) EMPTIES the buffer,
    /// so a re-drain can never produce an already-forwarded event; and the
    /// helper consumes the drained vec (one move onwards). The §16 probe
    /// ([`MountCounters::mount_event_forwards`]) ticks once per forwarded
    /// ROW — a double-forward defect is the kit that breaks this count.
    ///
    /// No routable endpoint = benign no-op (the manager's own test
    /// discipline: an empty manager answers nothing, sends nothing). An
    /// unregistered surface's forward never reaches here — the arm's
    /// surface lookup already refused — and a registered surface whose
    /// panel id unresolves is equally silenced (mirror the bridge's
    /// unknown-surface grace).
    pub(in crate::app) fn forward_mount_events(
        &mut self,
        surface_id: StableId,
        events: Vec<WidgetEvent>,
    ) {
        if events.is_empty() {
            return;
        }
        let Some(panel_id) = self
            .mount_registry
            .borrow()
            .panel_for_surface_owned(surface_id)
        else {
            return;
        };
        self.mount_registry
            .borrow_mut()
            .record_panel_events_forwarded_owned(events.len());
        #[cfg(not(target_arch = "wasm32"))]
        {
            for batch in events.chunks(MAX_WIDGET_EVENT_ROWS) {
                crate::plugin::external::with_manager(|manager| {
                    manager.broadcast_panel_scene_events(&panel_id, surface_id, batch);
                });
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // No plugin manager on the web runtime: the v24 notify leg has
            // no transport there; the drained events drop (the same bound).
            let _ = (panel_id, events, surface_id);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (headless, deterministic): the whole bridge minus the iced arm —
// the arm itself is one delegation line whose callee these tests drive
// directly. An `OpenCADStudio::new_for_test()` underpins each test so the
// app-owned path (`update`) is exercised too; NO renderer, NO sleeps.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::window::plugin_widget_mount::{
        WidgetEventSink, app_message, register_test_surface,
    };
    use crate::ui::window::plugin_widgets_dispatch::DispatchOutcome;
    use ocs_plugin_api::scene::{SceneRevision, StableId};
    use ocs_plugin_api::widgets::{
        CellKind, ConfirmPolicy, IconActionDef, IconRef, ItemDef, ItemListDef, PanelTableDef,
        TableCell, TableColumnDef, TableRowDef, WidgetCellValue, WidgetNode, WidgetSurface,
    };
    use crate::ui::window::plugin_widgets_tables::TableUiState;
    use crate::ui::window::plugin_widget_mount::MountTableKeyStep;

    // -- fixtures ------------------------------------------------------------

    fn sid(n: u64) -> StableId {
        StableId(n)
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

    fn item_list(id: u64, items: Vec<ItemDef>) -> ItemListDef {
        ItemListDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            items,
        }
    }

    fn surface(roots: Vec<WidgetNode>) -> WidgetSurface {
        WidgetSurface {
            revision: SceneRevision(1),
            roots,
            tables: std::collections::BTreeMap::new(),
        }
    }

    fn button_def(id: u64, name: &str, command: &str) -> IconActionDef {
        IconActionDef {
            id: sid(id),
            accessible_name: name.to_string(),
            tooltip: None,
            icon: IconRef {
                uri: "host://icons/test.svg".to_string(),
                size: 16.0,
            },
            enabled: true,
            visible: true,
            active: false,
            command: command.to_string(),
            confirmation: None,
        }
    }

    /// One registered panel over the standard fixture surface (one list of
    /// rows 7/8 — surface id 200: the mount state opens under the FIRST
    /// root's id; see `register_test_surface`). The guard tears the
    /// surface down on drop.
    fn register_list_panel(
        panel: &str,
    ) -> crate::ui::window::plugin_widget_mount::RegisteredGuard {
        register_test_surface(
            panel,
            surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "row 1"), item(8, "row 2")],
            ))]),
        )
    }

    /// Select row 7 through the bridge (the command targets the command
    /// routing then names).
    fn select_row_7(handle: &SurfaceMountHandle) -> MountBridgeOutcome {
        route_mount_msg(
            handle,
            &WidgetMountMsg::SelectionChanged {
                surface_id: sid(200),
                widget_id: sid(200),
                selected: vec![sid(7)],
            },
        )
    }

    fn command_event(command: &str) -> WidgetEvent {
        WidgetEvent::CommandInvoked {
            surface_id: sid(200),
            command_id: command.to_string(),
            target_ids: vec![sid(7)],
        }
    }

    // -- 1. the message round trip through the bridge ------------------------

    /// `WidgetMountMsg` → `app_message` (the ONE `Message` mapping) → the
    /// Message arm's handler core → the machines. The outcome's routing,
    /// drained sink events and settled state assert EXACT.
    #[test]
    fn message_roundtrip_routes_through_bridge() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("scn014_roundtrip");
        let handle =
            MountRegistry::handle_for_surface(sid(200)).expect("the registered surface");

        // The builder-emitted gesture (a list row's selection press).
        let msg = WidgetMountMsg::SelectionChanged {
            surface_id: sid(200),
            widget_id: sid(200),
            selected: vec![sid(7)],
        };
        // The single mapping: the arm's payload unwraps back to the exact
        // message (`Message` itself carries no PartialEq — the enum is
        // not an equality type; the VARIANT's payload is, and that is the
        // contract the gate rides).
        let app_msg = app_message(msg.clone());
        let Message::WidgetMountMsg(routed_msg) = app_msg else {
            unreachable!("app_message wraps exactly this variant")
        };
        assert_eq!(routed_msg, msg);

        // "The handler called with a test message": the bridge core the
        // arm delegates to.
        let outcome = route_mount_msg(&handle, &routed_msg);
        assert_eq!(outcome.routing, MountRouting::Applied { forwarded: 1 });
        assert!(!outcome.settled, "a selection arms no dispatcher address");
        assert_eq!(
            outcome.events,
            vec![WidgetEvent::SelectionChanged {
                surface_id: sid(200),
                selected_ids: vec![sid(7)],
            }],
            "the sink drained exactly the forwarded event, in order"
        );
        // The settled state: the machine's selection is the offered one.
        assert_eq!(handle.state.borrow().selection(), [sid(7)]);
    }

    // -- 2. the settle policy: bridge-forwards-then-settle -------------------

    /// A command click forwards, THEN settles, and the exactly-once guard
    /// re-arms: a duplicate landing while the mark is still armed — the
    /// precise race the policy tail exists for — is refused by the
    /// machines; the policy tail clears the mark; the next genuine click
    /// emits again.
    #[test]
    fn settle_policy_forward_then_settle() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("scn014_settle");
        let handle =
            MountRegistry::handle_for_surface(sid(200)).expect("the registered surface");
        // The command def's sync (the owner plugin's publish step in the
        // full app; the registered machines carry it from there).
        handle
            .dispatcher
            .borrow_mut()
            .sync_button(&button_def(1, "Delete", "delete"));
        select_row_7(&handle);

        let click = WidgetMountMsg::ButtonClicked {
            surface_id: sid(200),
            widget_id: sid(1),
        };

        // (a) A duplicate lands while the mark is still armed — the raw
        // apply, i.e. the handler's forward half WITHOUT the settle tail
        // (the first dispatch runs, the second refuses in flight):
        let raw_apply = |msg: &WidgetMountMsg| {
            let mut sink = CollectingSink::default();
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            apply_mount_msg(&mut state, &mut dispatcher, msg, &mut sink)
        };
        assert_eq!(
            raw_apply(&click),
            MountRouting::Applied { forwarded: 1 },
            "the first genuine click emits and arms"
        );
        assert_eq!(
            raw_apply(&click),
            MountRouting::Refused(
                "widget 1 is already dispatched and awaiting settlement".to_string()
            ),
            "the double-activation while pending is refused"
        );
        // ...and the bridge's settle tail clears the armed mark:
        assert!(
            handle.dispatcher.borrow_mut().settle(DispatchKey::Button(sid(1))),
            "the settle tail settles the forwarded command's own address"
        );

        // (b) Through the bridge, forward-then-settle in one route: the
        // emitted event leaves, and the tail settles (nothing outstanding
        // afterwards).
        let first = route_mount_msg(&handle, &click);
        assert_eq!(first.routing, MountRouting::Applied { forwarded: 1 });
        assert!(
            first.settled,
            "the policy tail settled after the forward"
        );
        assert_eq!(first.events, vec![command_event("delete")]);
        assert!(
            !handle.dispatcher.borrow_mut().settle(DispatchKey::Button(sid(1))),
            "nothing outstanding: the tail already settled"
        );

        // (c) The re-arm: the next genuine click emits again — exactly once
        // per interaction, never a replay.
        let second = route_mount_msg(&handle, &click);
        assert_eq!(
            second.routing,
            MountRouting::Applied { forwarded: 1 },
            "the guard re-armed after settle"
        );
        assert_eq!(second.events, vec![command_event("delete")], "not a replay");
        assert!(second.settled);
    }

    // -- 3. the confirmation lifecycle through the bridge --------------------

    /// The `Prompt` policy through the bridge: the first click requests
    /// the prompt (NO event collected; the tick-count records the surfaced
    /// prompt), a re-click refuses in flight, the dialog's confirm emits
    /// the captured command exactly once — forwarded through the collecting
    /// sink and settled by the same policy — and the settled state stays
    /// consistent (nothing pending, nothing in flight).
    #[test]
    fn confirmation_lifecycle_routes_prompt_then_command() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("scn014_confirm");
        let handle =
            MountRegistry::handle_for_surface(sid(200)).expect("the registered surface");
        let def = IconActionDef {
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Really delete the selection?".to_string(),
            }),
            ..button_def(1, "Delete", "delete")
        };
        handle.dispatcher.borrow_mut().sync_button(&def);
        select_row_7(&handle);

        let click = WidgetMountMsg::ButtonClicked {
            surface_id: sid(200),
            widget_id: sid(1),
        };
        let prompts_before = MountRegistry::counters().prompts_surfaced;
        let key = DispatchKey::Button(sid(1));

        // (a) The first click: PROMPT requested — no event, tick recorded.
        let prompted = route_mount_msg(&handle, &click);
        assert_eq!(
            prompted.routing,
            MountRouting::PromptRequested {
                prompt: "Really delete the selection?".to_string(),
            }
        );
        assert_eq!(
            prompted.events,
            Vec::new(),
            "a prompt request collects no command event"
        );
        assert!(
            !prompted.settled,
            "a pending confirmation refuses the bridge settle (its dialog owns it)"
        );
        assert_eq!(
            MountRegistry::counters().prompts_surfaced - prompts_before,
            1,
            "the PromptSurfaced tick-count recorded"
        );

        // (b) The re-click while armed: refused, no event, no re-prompt.
        let refused = route_mount_msg(&handle, &click);
        assert!(matches!(refused.routing, MountRouting::Refused(_)));
        assert_eq!(refused.events, Vec::new(), "still no event");
        assert_eq!(
            MountRegistry::counters().prompts_surfaced - prompts_before,
            1,
            "no re-prompt"
        );

        // (c) The dialog's confirm: the captured command emits EXACTLY
        // once, forwarded through the collecting sink, then settled by the
        // same policy the click path uses (the dialog path is the SCN-015
        // wiring; its machine edge is exactly this).
        let confirmed = handle
            .dispatcher
            .borrow_mut()
            .confirm(key.clone())
            .expect("the armed stage the future dialog owns");
        let event = match confirmed {
            DispatchOutcome::ConfirmedEmitted(event) => event,
            other => panic!("expected ConfirmedEmitted, got {other:?}"),
        };
        assert_eq!(
            event,
            command_event("delete"),
            "the dispatch-captured command + targets"
        );
        let mut sink = CollectingSink::default();
        sink.emit(event);
        assert_eq!(
            sink.take(),
            vec![command_event("delete")],
            "the confirmed command forwards once"
        );
        assert!(
            handle.dispatcher.borrow_mut().settle(key.clone()),
            "the bridge settle completes the lifecycle"
        );

        // (d) The settled state consistent: nothing pending, nothing armed.
        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None);
        assert!(!handle.dispatcher.borrow_mut().settle(key));
    }

    // -- 4. an unknown/unregistered surface is graceful -----------------------

    /// The arm reaches the handler for an UNREGISTERED surface (and a
    /// registered panel whose message names a FOREIGN surface id): no
    /// panic, nothing routed, the registered machines untouched — the app
    /// never sees a routed event.
    #[test]
    fn unknown_panel_message_is_graceful() {
        let mut app = OpenCADStudio::new_for_test();

        // Nothing registered at all: the direct handler and the full arm
        // both no-op.
        let unknown = WidgetMountMsg::ButtonClicked {
            surface_id: sid(424_242),
            widget_id: sid(1),
        };
        app.on_widget_mount_msg(unknown.clone());
        let _ = app.update(Message::WidgetMountMsg(unknown));

        // A registered panel, whose message names a foreign surface id:
        // the surface lookup still misses (registered ≠ routable).
        let _guard = register_list_panel("scn014_unknown");
        let foreign = WidgetMountMsg::ButtonClicked {
            surface_id: sid(999),
            widget_id: sid(50),
        };
        app.on_widget_mount_msg(foreign.clone());
        let _ = app.update(Message::WidgetMountMsg(foreign));

        // The registered machines are untouched by all of it.
        let handle =
            MountRegistry::handle_for_surface(sid(200)).expect("the registered surface");
        assert!(handle.state.borrow().selection().is_empty());
        assert_eq!(handle.state.borrow().mutations().row_state_changes, 0);
        assert_eq!(handle.state.borrow().pending_children().len(), 0);
    }

    // -- 4b. SCN-016: the LIVE-key hookup (the recorded join, landed) --------

    /// The app's real key-event mapper (`app::view::mount_key_event` — the
    /// listener the subscription chooser selects while a mount surface
    /// holds the keyboard) hands a LIVE main-window key to the bridge as
    /// `Message::WidgetMountMsg(WidgetMountMsg::KeyPressed { .. })`, whose
    /// arm runs the complete `on_mount_key` route: the key resolves ONCE
    /// into the ring's focus + selection write and applies ONCE (the
    /// bridge's message-count and the machines' end state are exact).
    #[test]
    fn live_key_through_mapper_routes_once() {
        use crate::app::view as app_view;

        let mut app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("scn016_keyjoin");
        // The keyboard grab — what a mount-panel press leaves (the events
        // below run with no pointer in flight; the sole-surface resolution
        // would name the same surface).
        MountRegistry::set_mount_key_target(Some(sid(200)));
        let before = MountRegistry::counters();

        // A REAL key event (the mapper's input shape). The main-window id:
        // with no OS main window registered (the headless default),
        // `is_main_window` treats any id as the main window — the mapper's
        // own rule, exercised as-is.
        let key = iced::keyboard::Key::Named(keyboard_key::Named::ArrowDown);
        let key_event = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: Modifiers::NONE,
            text: None,
            repeat: false,
        });

        // The mapper's join: a mount-routing key → the bridge message.
        let msg = app_view::mount_key_event(
            key_event,
            iced::event::Status::Ignored,
            iced::window::Id::unique(),
        )
        .expect("a mount-held keyboard hands the key through");
        let Message::WidgetMountMsg(WidgetMountMsg::KeyPressed { key, modifiers }) = msg else {
            panic!("the join rides Message::WidgetMountMsg");
        };
        assert_eq!(modifiers, Modifiers::NONE);

        // The arm: resolve → translate → route → apply, ONCE each.
        let _ = app.update(Message::WidgetMountMsg(WidgetMountMsg::KeyPressed { key, modifiers }));

        let handle = MountRegistry::handle_for_surface(sid(200)).expect("the registered surface");
        {
            let state = handle.state.borrow();
            assert_eq!(state.focus(), Some(sid(7)), "the arrow landed once");
            assert_eq!(state.selection(), [sid(7)]);
        }
        let after = MountRegistry::counters();
        assert_eq!(
            after.keyboard_nav_steps - before.keyboard_nav_steps,
            1,
            "one resolved navigation step for the one key"
        );
        assert_eq!(
            after.messages_routed - before.messages_routed,
            2,
            "the arrow's pair (focus + selection) routed exactly once each"
        );
    }

    // -- 4c. SCN-016: the move-coalesce tick pump (the recorded join) -------

    /// The mount owing a move keeps the EXISTING tick channel alive (the
    /// pump condition's `any_move_owed` half); the tick ANSWERS the owed
    /// move latest-wins: two moves owed = ONE mount call, the newest
    /// position delivered (the older counted superseded, never queued),
    /// and the capture's drag origin lands with the first delivered move.
    #[test]
    fn tick_pump_answers_mount_move_latest_wins() {
        let mut app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("scn016_tickpump");
        let _ = route_mount_press(sid(200));
        assert!(MountRegistry::mount_captured(sid(200)), "press arms capture");

        // Two moves: the LATEST wins (the first is counted superseded,
        // never answered), the slot holds ONE position, the pump's gate
        // sees the owed move.
        let _ = route_mount_move(sid(200), iced::Point::new(1.0, 1.0));
        let _ = route_mount_move(sid(200), iced::Point::new(9.0, 5.0));
        assert!(
            crate::ui::window::plugin_widget_mount::with_test_registry(|registry| {
                registry.any_move_owed()
            }),
            "the pump condition flows while the mount owes a move"
        );
        let before = MountRegistry::counters();

        // The REAL tick arm (the joined channel: `Message::MoveCoalesceTick`
        // → `on_mount_move_coalesce_tick`): answers ≤1 per surface.
        let _ = app.update(crate::app::Message::MoveCoalesceTick);

        let after = MountRegistry::counters();
        assert_eq!(
            after.mount_move_calls - before.mount_move_calls,
            1,
            "exactly one mount call for the surface's owed move"
        );
        let hover = MountRegistry::mount_hover_state(sid(200)).expect("the input entry");
        assert_eq!(
            hover.last_move,
            Some(iced::Point::new(9.0, 5.0)),
            "latest wins"
        );
        assert_eq!(
            hover.capture_origin,
            Some(iced::Point::new(9.0, 5.0)),
            "the capture origin lands with the first delivered move"
        );
        // And nothing is owed anymore (the pumped tick settled the slot).
        assert!(!crate::ui::window::plugin_widget_mount::with_test_registry(
            |registry| registry.any_move_owed()
        ));
    }

    // -- 5. SCN-020: Enter activates the focused row through the bridge -------

    /// The full keyboard path (no pointer events anywhere): the key target
    /// resolves, the route produces focus + selection messages, the bridge
    /// applies them (the app-side routing works through the ordinary
    /// `route_mount_msg`), and Enter on the focused row forwards the
    /// ItemActivated activation event — the §16 keyboard-no-pointer
    /// path, end-to-end.
    #[test]
    fn enter_activates_focused_row() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("scn020_enter");
        let handle =
            MountRegistry::handle_for_surface(sid(200)).expect("the registered surface");
        // The keyboard grab (what the view listener hookup will hand to);
        // the sole registered surface would resolve anyway.
        MountRegistry::set_mount_key_target(Some(sid(200)));
        let before = MountRegistry::counters();

        let msgs = handle_mount_key(
            iced::keyboard::Key::Named(keyboard_key::Named::ArrowDown),
            iced::keyboard::Modifiers::NONE,
        );
        assert_eq!(msgs.len(), 2, "the arrows' pair: focus + selection");
        for msg in &msgs {
            let outcome = route_mount_msg(&handle, msg);
            assert!(
                matches!(outcome.routing, MountRouting::Applied { .. }),
                "each route applies: {msg:?}"
            );
        }
        {
            let state = handle.state.borrow();
            assert_eq!(state.focus(), Some(sid(7)));
            assert_eq!(state.selection(), [sid(7)]);
        }
        assert_eq!(
            MountRegistry::counters().keyboard_nav_steps - before.keyboard_nav_steps,
            1
        );

        // Enter on the focused row: ONE activation route → ItemActivated.
        let msgs = handle_mount_key(
            iced::keyboard::Key::Named(keyboard_key::Named::Enter),
            iced::keyboard::Modifiers::NONE,
        );
        assert_eq!(
            msgs,
            vec![WidgetMountMsg::RowActivated {
                surface_id: sid(200),
                widget_id: sid(200),
                row_id: sid(7),
            }]
        );
        let outcome = route_mount_msg(&handle, &msgs[0]);
        assert_eq!(outcome.routing, MountRouting::Applied { forwarded: 1 });
        assert!(!outcome.settled, "a row activation settles no dispatcher address");
        assert_eq!(
            outcome.events,
            vec![WidgetEvent::ItemActivated {
                surface_id: sid(200),
                item_id: sid(7),
            }],
            "the activation event forwarded once"
        );
        assert_eq!(
            MountRegistry::counters().keyboard_nav_steps - before.keyboard_nav_steps,
            2,
            "two resolved keys, two nav steps"
        );
    }

    // -- 6. SCN-020: Escape cancels draft + pending (no drag coupling) ---------

    /// Escape's mounted-surface half cancels the ARMED prompt through the
    /// dispatcher's own hook (a re-click re-prompts — nothing lingering),
    /// and the typed-table layer's Escape cancels an open cell draft FREE
    /// (the state's hooks; drag cancel is SCN-021's own arm).
    #[test]
    fn escape_clears_draft_and_pending() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("scn020_escape");
        let handle =
            MountRegistry::handle_for_surface(sid(200)).expect("the registered surface");
        MountRegistry::set_mount_key_target(Some(sid(200)));
        let key = DispatchKey::Button(sid(1));
        handle
            .dispatcher
            .borrow_mut()
            .sync_button(&IconActionDef {
                confirmation: Some(ConfirmPolicy::Prompt {
                    message: "Really delete the selection?".to_string(),
                }),
                ..button_def(1, "Delete", "delete")
            });

        // Arm an interaction (the click path — the selection must exist so
        // the dispatch resolves).
        let selected = route_mount_msg(
            &handle,
            &WidgetMountMsg::SelectionChanged {
                surface_id: sid(200),
                widget_id: sid(200),
                selected: vec![sid(7)],
            },
        );
        assert_eq!(
            selected.events,
            vec![WidgetEvent::SelectionChanged {
                surface_id: sid(200),
                selected_ids: vec![sid(7)],
            }]
        );
        let click = WidgetMountMsg::ButtonClicked {
            surface_id: sid(200),
            widget_id: sid(1),
        };
        let armed = route_mount_msg(&handle, &click);
        assert_eq!(
            armed.routing,
            MountRouting::PromptRequested {
                prompt: "Really delete the selection?".to_string(),
            }
        );
        assert!(
            handle
                .dispatcher
                .borrow()
                .pending_prompt(&key)
                .is_some(),
            "the prompt is armed"
        );

        // ESCAPE: no message routes; the armed prompt CANCELS through the
        // state layer's own hook.
        let msgs = handle_mount_key(
            iced::keyboard::Key::Named(keyboard_key::Named::Escape),
            iced::keyboard::Modifiers::NONE,
        );
        assert!(msgs.is_empty(), "escape routes no navigation message");
        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None);
        let counters = handle.dispatcher.borrow().counters().clone();

        // Nothing lingers: the re-click RE-PROMPTS (the cancel re-arms the
        // in-flight mark, exactly-once intact).
        let re_armed = route_mount_msg(&handle, &click);
        assert_eq!(
            re_armed.routing,
            MountRouting::PromptRequested {
                prompt: "Really delete the selection?".to_string(),
            }
        );
        assert_eq!(re_armed.events, Vec::new());
        assert!(handle.dispatcher.borrow().pending_prompt(&key).is_some());
        {
            let now = handle.dispatcher.borrow().counters().clone();
            assert!(
                now.commands_emitted >= counters.commands_emitted,
                "emit-count only ever grows"
            );
        }
        // And the mount's own cancel hook takes the re-armed one down too.
        assert!(MountRegistry::mount_escape_cancel(sid(200)));
        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None);

        // The DRAFT half (pure table layer): the open draft cancels FREE.
        let def = mount_table_fixture();
        let mut table = state_for_table(&def);
        table.begin_cell_edit(sid(1), sid(3)).expect("the Number column");
        table
            .draft_cell_value(WidgetCellValue::Number(9.0))
            .expect("drafted");
        assert!(table.draft().is_some());
        let mutations_before = *table.mutations();
        let mut cursor = None;
        let outcome = handle_mount_table_key(
            &mut table,
            &mut cursor,
            iced::keyboard::Key::Named(keyboard_key::Named::Escape),
            iced::keyboard::Modifiers::NONE,
            &WindowSpec {
                viewport_height: 20.0,
                row_height: 1.0,
                margin_rows: 4,
            },
            &[sid(2), sid(3), sid(4)],
        );
        assert_eq!(
            outcome,
            Some(crate::ui::window::plugin_widget_mount::MountTableKeyStep::DraftCancelled)
        );
        assert!(table.draft().is_none(), "the draft is cleared");
        assert_eq!(
            *table.mutations(),
            mutations_before,
            "a cancelled draft touched nothing (no evidence it existed)"
        );
    }

    // -- SCN-020 table fixtures (the typed-table keyboard layer's pure
    // -- consumption; same shapes as the mount module's) ----------------------

    fn mount_table_column(id: u64, kind: CellKind, label: &str) -> TableColumnDef {
        TableColumnDef {
            id: sid(id),
            label: label.to_string(),
            kind,
            width_hint: None,
            stretch: false,
        }
    }

    fn mount_table_fixture() -> PanelTableDef {
        PanelTableDef {
            id_table: sid(700),
            columns: vec![
                mount_table_column(2, CellKind::Text, "note"),
                mount_table_column(3, CellKind::Number, "length"),
                mount_table_column(4, CellKind::Bool, "locked"),
            ],
            total_rows: None,
            rows: vec![
                TableRowDef {
                    key: sid(1),
                    cells: vec![
                        TableCell::Text("note 1".to_string()),
                        TableCell::Number(1.0),
                        TableCell::Bool(false),
                    ],
                    domain_ref: None,
                },
                TableRowDef {
                    key: sid(2),
                    cells: vec![
                        TableCell::Text("note 2".to_string()),
                        TableCell::Number(2.0),
                        TableCell::Bool(true),
                    ],
                    domain_ref: None,
                },
            ],
        }
    }

    fn state_for_table(def: &PanelTableDef) -> TableUiState {
        let mut state = TableUiState::new(sid(200), def.id_table);
        state.sync(&WidgetSurface {
            revision: SceneRevision(1),
            roots: Vec::new(),
            tables: std::collections::BTreeMap::from([(def.id_table, def.clone())]),
        });
        state
    }


    // -- 6. SCN-025 — the sink goes LIVE: the drain forwards ------------------

    /// The drained events forward through the ONE v24 notify route, from
    /// the arm: a routed machine emission maps 1:1 to the §16 forward
    /// probe ([`MountCounters::mount_event_forwards`]) — no extra forwards
    /// of previously-forwarded rows materialize (the take-drain empties
    /// the buffer, so a re-drain cannot replay), and an empty batch is a
    /// no-op. The manager-missing runtime is the benign no-op (the
    /// manager's own test discipline).
    #[test]
    fn drained_events_forward_once_through_the_arm() {
        let mut app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("fwd_live");
        let before = app
            .mount_registry
            .borrow()
            .counters_owned()
            .mount_event_forwards;

        // (1) One routed selection → exactly one forwarded row.
        app.on_widget_mount_msg(WidgetMountMsg::SelectionChanged {
            surface_id: sid(200),
            widget_id: sid(200),
            selected: vec![sid(7)],
        });
        let after_one = app.mount_registry.borrow().counters_owned().mount_event_forwards;
        assert_eq!(after_one - before, 1, "one emission, one forward");

        // (2) A second routed selection → exactly one MORE forward (the
        // first event is not replayed; the drain ran empty).
        app.on_widget_mount_msg(WidgetMountMsg::SelectionChanged {
            surface_id: sid(200),
            widget_id: sid(200),
            selected: vec![sid(7)],
        });
        let after_two = app.mount_registry.borrow().counters_owned().mount_event_forwards;
        assert_eq!(after_two - after_one, 1, "the next emission forwards once — no re-drain replay");

        // (3) A refusal (unknown address) forwards nothing.
        app.on_widget_mount_msg(WidgetMountMsg::ButtonClicked {
            surface_id: sid(200),
            widget_id: sid(404),
        });
        let after_refusal = app.mount_registry.borrow().counters_owned().mount_event_forwards;
        assert_eq!(after_refusal, after_two, "refusals emit no events, forward none");

        // (4) The unregistered surface's arm never forwards (the bridge's
        // surface lookup refuses first).
        app.on_widget_mount_msg(WidgetMountMsg::SelectionChanged {
            surface_id: sid(424242),
            widget_id: sid(1),
            selected: vec![sid(7)],
        });
        assert_eq!(
            app.mount_registry.borrow().counters_owned().mount_event_forwards,
            after_refusal,
            "the unregistered-surface grace"
        );
    }

    /// The bounded-batch rule: a forward SPLIT at the notify-leg cap
    /// still forwards EVERY row exactly once (the helper's chunks loop —
    /// a producer splits, never grows a batch).
    #[test]
    fn oversized_forward_splits_at_the_notify_cap() {
        let mut app = OpenCADStudio::new_for_test();
        let _guard = register_list_panel("fwd_batch");
        let events: Vec<WidgetEvent> = (0..(4096 + 7))
            .map(|i| {
                WidgetEvent::TreeExpanded {
                    surface_id: sid(200),
                    node_id: sid(i as u64 % 20_000 + 1),
                    expanded: true,
                }
            })
            .collect();
        let before = app.mount_registry.borrow().counters_owned().mount_event_forwards;
        app.forward_mount_events(sid(200), events);
        let after = app.mount_registry.borrow().counters_owned().mount_event_forwards;
        assert_eq!(after - before, 4096 + 7, "every row forwarded exactly once");
    }

    /// The exactly-once RETURN HALF pinned at the mechanism: a re-drain of
    /// a drained sink produces nothing (the forward consumes the take).
    #[test]
    fn the_collecting_sink_drains_exactly_once() {
        let mut sink = CollectingSink::default();
        sink.emit(WidgetEvent::SelectionChanged {
            surface_id: sid(200),
            selected_ids: vec![sid(7)],
        });
        assert_eq!(sink.take().len(), 1);
        assert!(sink.take().is_empty(), "a re-drain is empty — no re-forward material");
    }

}
