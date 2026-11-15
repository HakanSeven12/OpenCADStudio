//! The widget-mount bridge handler (`SCN-014`): `app::Message::WidgetMountMsg`
//! → the registered surface's widget state machines.
//!
//! ONE route arm, one handler, ONE settle policy. The native mount
//! (`plugin_widget_mount.rs`) emits its host-local vocabulary and owns the
//! routing INTO the machines (`apply_mount_msg`); this is the app half:
//!
//! 1. find the registered surface by the message's surface id
//!    ([`MountRegistry::handle_for_surface`]) — an unknown/unregistered id
//!    is a graceful no-op (no panic, nothing routed, no event exists);
//! 2. FORWARD: `apply_mount_msg` runs and every emitted [`WidgetEvent`]
//!    leaves through the COLLECTING sink ([`CollectingSink`]) — the
//!    pipeline seam whose drain this handler owns. The plugin publish
//!    (`v4_support::publish_*`) is EXPLICITLY out of scope until the
//!    API-23 crossing (ADR-004): the collecting sink is what the bridge
//!    drains until then;
//! 3. SETTLE (the bridge-forwards-then-settle policy): a command message's
//!    originating address clears its exactly-once mark AFTER the forward —
//!    the in-flight guard re-arms once the handler completes, so the next
//!    genuine click emits again. A pending CONFIRMATION refuses settle
//!    here (its dialog owns the armed interaction); rows/selection/scroll/
//!    focus arm nothing and settle nothing.
//!
//! A `PromptRequested` routing records the bridge's prompt tick-count and
//! collects NO event — the real confirm dialog rides the app's existing
//! modal patterns and is the SCN-015 follow-up (recorded, not claimed).
//! Host-local only: `Message` carries no serialization derives (no wire
//! type), so the appended variant's only consumer is this arm.

use iced::keyboard::{Key, Modifiers, key as keyboard_key};
use iced::{Point, Task};

use crate::app::{Message, OpenCADStudio};
use crate::ui::window::plugin_widget_mount::{
    CollectingSink, MountRegistry, MountRouting, SurfaceMountHandle, WidgetMountMsg,
    apply_mount_msg, key_route, mount_table_key_route, msg_surface_id,
};
use crate::ui::window::plugin_widgets::WindowSpec;
use crate::ui::window::plugin_widgets_dispatch::DispatchKey;
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
        // Pointer messages capture/never arm the dispatcher (§14).
        | WidgetMountMsg::PointerPressed { .. }
        | WidgetMountMsg::PointerMoved { .. }
        | WidgetMountMsg::PointerReleased { .. } => None,
    }
}

/// Route ONE mount message into `handle`'s machines — the bridge core the
/// `Message::WidgetMountMsg` arm and the tests share.
///
/// Forward-then-settle (see the module docs): the sink drains after the
/// machines returned, the settle runs after the sink drained. The counters
/// move with the verdict (routed/prompts — the §16 tick-count handles).
pub(crate) fn route_mount_msg(
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
    MountRegistry::record_message_routed();
    if matches!(routing, MountRouting::PromptRequested { .. }) {
        MountRegistry::record_prompt_surfaced();
        // SCN-020: name the armed interaction for the keyboard layer's
        // Escape cancel (the hint; the dispatcher's dismiss_prompt stays
        // the cancel edge).
        if let Some(key) = settle_address_of(msg) {
            MountRegistry::mount_record_prompt_armed(msg_surface_id(msg), key);
        }
    }
    let events = sink.take();
    MountBridgeOutcome {
        routing,
        events,
        settled,
    }
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
pub(crate) fn route_mount_move(surface_id: StableId, position: Point) -> MountMoveOutcome {
    match MountRegistry::mount_move_defer(surface_id, position) {
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
/// ([`MountRegistry::mount_press`]); the press also makes the surface the
/// keyboard's target (the desktop input model). `false` = unknown surface
/// (graceful).
pub(crate) fn route_mount_press(surface_id: StableId) -> bool {
    MountRegistry::mount_press(surface_id)
}

/// The release route: capture completes (release ANYWHERE — the pure
/// completion contract; this iced revision's `mouse_area` releases only
/// inside the area, so the window-level catcher is the app-view seam, see
/// [`OpenCADStudio::on_mount_key`]'s sibling note) and the surface's owed
/// move — if the gesture left one — is delivered NOW (exactly ≤1). Returns
/// the delivered count.
pub(crate) fn route_mount_release(surface_id: StableId) -> usize {
    MountRegistry::mount_pointer_released(surface_id)
}

/// The keyboard routing RESOLUTION (SCN-020, the pure layer): one key event
/// against the mounted keyboard target → the mount messages to route, in
/// order, through the ordinary bridge (`Message::WidgetMountMsg` →
/// [`route_mount_msg`]). The resolution:
///
/// - the TARGET surface is [`MountRegistry::mount_key_surface`]'s (the
///   press-captured surface, else the sole registered mount — the
///   keyboard-only §16 path; `None` → no messages, graceful);
/// - translation is [`key_route`]'s (the mount module's pure keyboard
///   layer — arrows/Tab/Enter over the machines' ring; disabled skipped BY
///   the state layer; chords own nothing);
/// - ESCAPE is the machine-direct cancel (no message): the armed prompt
///   dismissal runs HERE ([`MountRegistry::mount_escape_cancel`]); the
///   drag cancel is deliberately NOT coupled in (SCN-021 adds its own arm).
///
/// The "app-ish state" the original brief sketched stays UNPARAMETERIZED
/// in v1 on purpose: the mount's routes resolve entirely through the
/// registry-side surfaces, so there is nothing of `OpenCADStudio` to read
/// yet — the app-owned registry migration (SCN-015) is the crossing where
/// this signature would grow the app state in.
///
/// The app-hook completion status (RECORDED, the SCN-015-style deferral):
/// the pure layer is COMPLETE here (this fn + [`route_mount_msg`]);
/// `OpenCADStudio::on_mount_key` below is the one-delegation call the real
/// keyboard listener hands to — but the LISTENER lives in the app
/// `view/mod.rs` event composition (`main_window_event`), which SCN-020
/// does not own. Until that one-line hookup lands, no live key event
/// reaches this layer; the headless tests drive the full path.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn handle_mount_key(key: Key, modifiers: Modifiers) -> Vec<WidgetMountMsg> {
    let Some(surface_id) = MountRegistry::mount_key_surface() else {
        return Vec::new();
    };
    // Escape: the mounted surface's cancel is machine-direct (the state
    // layers' own hooks — the armed prompt's dismissal); no route message
    // exists for it and no machine sees a KEY.
    if key == Key::Named(keyboard_key::Named::Escape) {
        MountRegistry::mount_escape_cancel(surface_id);
        return Vec::new();
    }
    let Some(handle) = MountRegistry::handle_for_surface(surface_id) else {
        return Vec::new();
    };
    let mut state = handle.state.borrow_mut();
    key_route(surface_id, &handle.surface, &mut state, key, modifiers)
}

/// The typed-table keyboard route's REACH-THROUGH (SCN-020): the typed
/// table machine is state-layer-complete and the mount's table placeholder
/// is not interactive yet, so the table's keyboard layer is exposed as the
/// PURE consumption [`mount_table_key_route`] (tested at the mount module;
/// the typed-table MOUNT integration rides it). Reached only by tests
/// today — surfaced here so the ONE route surface stays discoverable.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn handle_mount_table_key(
    state: &mut TableUiState,
    cell_cursor: &mut Option<CellFocus>,
    key: Key,
    modifiers: Modifiers,
    spec: &WindowSpec,
    column_order: &[StableId],
) -> Option<crate::ui::window::plugin_widget_mount::MountTableKeyStep> {
    mount_table_key_route(state, cell_cursor, key, modifiers, spec, column_order)
}

impl OpenCADStudio {
    /// The `Message::WidgetMountMsg` arm's handler: route one
    /// builder-emitted mount gesture into its registered surface's
    /// machines (forward → settle; see [`route_mount_msg`]) and drop the
    /// drained events at the collecting seam (the publish hang-off is the
    /// ADR-004 crossing — SCN-015).
    ///
    /// UNKNOWN surface: a graceful no-op — no panic, nothing routed, no
    /// event; a REGISTERED surface's stale content (a cross-surface or
    /// unknown-address message) refuses INSIDE the machines, and the app
    /// never sees a routed event either way.
    ///
    /// SCN-020: the mount's POINTER messages do NOT take the machine
    /// route — they OWN the input layer ([`route_mount_press`]/[`route_mount_move`]
    /// /[`route_mount_release`]: capture + the latest-wins coalescer, §14
    /// host-local) and are resolved before the machine path.
    pub(in crate::app) fn on_widget_mount_msg(
        &mut self,
        msg: WidgetMountMsg,
    ) -> Task<Message> {
        match &msg {
            WidgetMountMsg::PointerPressed { surface_id, .. } => {
                route_mount_press(*surface_id);
                return Task::none();
            }
            WidgetMountMsg::PointerMoved { surface_id, position } => {
                let _ = route_mount_move(*surface_id, *position);
                return Task::none();
            }
            WidgetMountMsg::PointerReleased { surface_id, .. } => {
                route_mount_release(*surface_id);
                return Task::none();
            }
            _ => {}
        }
        let Some(handle) = MountRegistry::handle_for_surface(msg_surface_id(&msg)) else {
            // Unregistered surface: the bridge refuses gracefully.
            return Task::none();
        };
        let outcome = route_mount_msg(&handle, &msg);
        // Pre-wire drain — see the module docs (the collecting sink's
        // contract; the publish hang-off is the ADR-004 crossing).
        let _ = outcome.events;
        Task::none()
    }

    /// The mount keyboard route's app join (SCN-020): resolve the key
    /// ([`handle_mount_key`]) and route EVERY produced message through the
    /// ordinary `Message::WidgetMountMsg` bridge half.
    ///
    /// WIRING STATUS (recorded — the records repeat [`handle_mount_key`]'s):
    /// the real key listener is app `view/mod.rs` territory (SCN-020 does
    /// not own it); this method is the complete, tested route the
    /// listener's one-line hookup will hand to.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::app) fn on_mount_key(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        let msgs = handle_mount_key(key, modifiers);
        if msgs.is_empty() {
            return Task::none();
        }
        Task::batch(
            msgs.into_iter()
                .map(|msg| self.on_widget_mount_msg(msg))
                .collect::<Vec<_>>(),
        )
    }

    /// The mount side of the move-coalesce tick (SCN-020): answer every
    /// surface's LAST owed move ([`MountRegistry::mount_flush_moves`] —
    /// ≤1 mount call per surface per tick, host-local state only). Joined
    /// onto the EXISTING `Message::MoveCoalesceTick` arm
    /// (`app::update::mod.rs`) so both coalescers answer on the same
    /// tick channel. NOTE (recorded): the tick subscription's PUMP
    /// condition (`view/mod.rs`) still names only the viewport's owed
    /// move — widening it is the same view-side seam as the keyboard
    /// hookup; a mount move that stops while no other move arrives waits
    /// for the next gesture (never a queue: the slot overwrites).
    pub(in crate::app) fn on_mount_move_coalesce_tick(&mut self) -> Task<Message> {
        let _ = MountRegistry::mount_flush_moves();
        Task::none()
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

}
