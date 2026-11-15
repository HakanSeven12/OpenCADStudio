//! The prompt-dialog lifecycle handler (`SCN-015`):
//! `app::Message::WidgetPromptMsg` → the armed confirmation's decision on
//! the SAME dispatcher the arming click used.
//!
//! The confirm lifecycle's full headless arc: a `ConfirmPolicy::Prompt`
//! activation routes `PromptRequested` through the SCN-014 bridge
//! (`app::update::widget_mount`), which records the armed desc in the mount
//! registry ([`MountRegistry::mount_record_prompt_armed`], through the
//! bridge's sink + settle policy) — the prompt is SURFACED and
//! surface-ready. The app's confirm modal (whose visual placement in the
//! app's existing modal machinery is the view composition the SCN-016
//! wiring owns — this handler is the SEMANTIC half it will plug into)
//! emits the dialog's decision:
//!
//! - **confirm** ([`WidgetPromptMsg::Confirmed`]) → the machine's
//!   [`WidgetDispatcher::confirm`]: the stage's CAPTURED command + targets
//!   emit exactly once, through the SAME collecting sink the mount bridge
//!   forwards events along ([`CollectingSink`], reused via one small
//!   wrapper, [`route_prompt_decision`]), then the SAME
//!   forward-then-settle policy (the in-flight guard re-arms);
//! - **dismiss** ([`WidgetPromptMsg::Dismissed`]) → the machine's
//!   [`WidgetDispatcher::dismiss_prompt`]: the pending stage + the armed
//!   mark clear, NO event exists, and a later genuine click re-prompts
//!   (the policy is per-def, not per-machine). The Escape cancel
//!   ([`MountRegistry::mount_escape_cancel_owned`]) lands in the SAME lifecycle
//!   edges (machine dismiss + desc consumed + the dismissal probe).
//!
//! Refusals are the unknown-surface pattern, mirrored: a decision whose
//! armed interaction is gone (unregistered surface, an address that is not
//! the armed one, or a machine that reports nothing pending anymore) is a
//! graceful no-op moving ONLY the refusal's own probe
//! ([`MountCounters::prompts_refused`]) — no panics, no event, nothing
//! half-applied.
//!
//! Host-local only: `Message` carries no serialization derives (no wire
//! type — restate by grep), so the appended tail variant's only consumer
//! is this arm. NO app state fields: the prompt's surface state lives in
//! the mount registry (whose lifetime is the surfaces'), the machines own
//! the exchange, and this handler owns only the routing + the probes (§16).

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::ui::window::plugin_widget_mount::{
    CollectingSink, MountRegistry, WidgetEventSink, WidgetPromptMsg,
};
use crate::ui::window::plugin_widgets_dispatch::{DispatchKey, DispatchOutcome};
use ocs_plugin_api::scene::StableId;
use ocs_plugin_api::widgets::WidgetEvent;

/// What ONE prompt decision did with one [`WidgetPromptMsg`] — the
/// routing verdict's total shape (each variant terminal, nothing
/// half-applies — the [`MountRouting`]
/// [`crate::ui::window::plugin_widget_mount::MountRouting`] verdict shape,
/// mirrored onto the dialog's two edges).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PromptRouting {
    /// The user CONFIRMED: the machine emitted the captured command
    /// exactly once; `forwarded` counts the events the sink drained (the
    /// `ConfirmedEmitted` outcome carries one event). The desc is consumed
    /// and the address settled (forward-then-settle).
    Confirmed {
        /// The number of semantic events forwarded to the sink.
        forwarded: usize,
    },
    /// The user DISMISSED: the armed stage + mark cleared, NO event exists
    /// (the machine's own dismissal semantics; the bridge settles nothing
    /// — the dismissal IS the release). The desc is consumed.
    Dismissed,
    /// Refused: no pending interaction matched the decision (unknown
    /// surface, a key that is not the armed one, or the machine reporting
    /// the stage gone). Only the refusal probe moved.
    Refused(String),
}

/// What ONE prompt decision produced in full — the outcome struct the
/// tests assert against (the mount bridge's
/// [`MountBridgeOutcome`](crate::app::update::widget_mount::MountBridgeOutcome)
/// shape, mirrored).
//
// `events` is the app arm's field consumer (the handler drains it); the
// verdict fields are the assertion tests' surface.
pub(crate) struct PromptBridgeOutcome {
    /// The decision's routing verdict.
    // Consumed by the assertion tests (the app arm needs none of it yet).
    #[cfg_attr(not(test), allow(dead_code))]
    pub routing: PromptRouting,
    /// The events the collecting sink drained, in emission order — a
    /// confirm forwards exactly the one `ConfirmedEmitted` event; a
    /// dismissal and a refusal drain nothing.
    pub events: Vec<WidgetEvent>,
    /// Whether the settle half cleared the address's in-flight mark
    /// (`true` only for a confirmed decision — the bridge's
    /// forward-then-settle policy).
    // Read by the assertion tests (the app arm infers nothing from it).
    #[cfg_attr(not(test), allow(dead_code))]
    pub settled: bool,
}

/// The FULL prompt-decision route (SCN-015) — the small wrapper the arm
/// delegates to: ONE decision, ONE verdict, the TWO machine edges and ONE
/// registry lifecycle:
///
/// 1. the surface must be a REGISTERED one (the unknown-surface no-op);
/// 2. the registry's armed desc must name the decision's address (the
///    not-pending / wrong-address graceful refusals — the registry stays
///    the decision's gate so the machines are never touched by a decision
///    naming nothing pending);
/// 3. confirm → `confirm` → sink → settle → desc consumed + the probe;
///    dismiss → `dismiss_prompt` → desc consumed + the probe (no settle:
///    the machine's dismissal IS the release).
///
/// A refusal verdict under one probe tick — [`route_prompt_decision_owned`]'s
/// refusal shape, hoisted out of the route body so the store borrow never
/// lives across a closure (the app-owned registry rides as the first
/// parameter).
fn prompt_refused(
    registry: &mut MountRegistry,
    reason: String,
) -> PromptBridgeOutcome {
    registry.record_prompt_refused_owned();
    PromptBridgeOutcome {
        routing: PromptRouting::Refused(reason),
        events: Vec::new(),
        settled: false,
    }
}

/// The FULL prompt-decision route (SCN-015) — the small wrapper the arm
/// delegates to: ONE decision, ONE verdict, the TWO machine edges and ONE
/// registry lifecycle:
///
/// 1. the surface must be a REGISTERED one (the unknown-surface no-op);
/// 2. the registry's armed desc must name the decision's address (the
///    not-pending / wrong-address graceful refusals — the registry stays
///    the decision's gate so the machines are never touched by a decision
///    naming nothing pending);
/// 3. confirm → `confirm` → sink → settle → desc consumed + the probe;
///    dismiss → `dismiss_prompt` → desc consumed + the probe (no settle:
///    the machine's dismissal IS the release).
///
/// A machine-level refusal under a valid desc (unreachable in one
/// synchronous route; defense in depth) also consumes the stale desc and
/// refuses gracefully — the machines stay the semantic authority, the
/// display copy self-heals.
///
/// The registry rides as the FIRST parameter (SCN-016's app-owned
/// migration): the store is the app's own (`&mut self.mount_registry`).
pub(crate) fn route_prompt_decision_owned(
    registry: &mut MountRegistry,
    surface_id: StableId,
    key: &DispatchKey,
    confirming: bool,
) -> PromptBridgeOutcome {
    let Some(handle) = registry.handle_for_surface_owned(surface_id) else {
        return prompt_refused(registry, format!("surface {} is not registered", surface_id.0));
    };
    // The pending-prompt guard: the decision must name the address the
    // armed desc carries (the modal's own content); anything else refuses
    // BEFORE the machine is touched.
    let Some(desc) = registry.prompt_dialog_owned(surface_id) else {
        return prompt_refused(
            registry,
            format!("surface {} has no prompt pending to decide", surface_id.0),
        );
    };
    if &desc.key != key {
        return prompt_refused(
            registry,
            format!("surface {}'s pending prompt names a different address", surface_id.0),
        );
    }

    if confirming {
        // (1) the machine's confirm — the captured stage emits exactly once.
        let confirmed = handle.dispatcher.borrow_mut().confirm(key.clone());
        match confirmed {
            Ok(DispatchOutcome::ConfirmedEmitted(event)) => {
                // (2) the sink drains (the same bridge half
                // [route_mount_msg_owned] runs), THEN (3) the address settles —
                // forward-then-settle, the click path's policy.
                let mut sink = CollectingSink::default();
                sink.emit(event);
                let events = sink.take();
                let settled = handle.dispatcher.borrow_mut().settle(key.clone());
                // (4) the dialog's state completes: the desc (and the
                // matching escape hint) leave the registry; the probe ticks.
                registry.take_prompt_dialog_owned(surface_id, key);
                registry.record_prompt_confirmed_owned();
                PromptBridgeOutcome {
                    routing: PromptRouting::Confirmed {
                        forwarded: events.len(),
                    },
                    events,
                    settled,
                }
            }
            Err(machine_refusal) => {
                // The stage is gone on the machine (or its re-validation
                // failed): the interaction is dead per the machine; the
                // stale display copy consumes (self-heal).
                registry.take_prompt_dialog_owned(surface_id, key);
                prompt_refused(registry, machine_refusal.to_string())
            }
            // `confirm` resolves ONLY to `ConfirmedEmitted` (the machine's
            // own contract — pinned by SWG-003); the remaining outcomes are
            // the defensive totals a machine update could someday widen
            // with: a naked emission or a re-prompt under a pending dialog
            // means the machine OVERRULED the registry's desc — the same
            // self-heal refusal, no event taken from it.
            Ok(other) => {
                registry.take_prompt_dialog_owned(surface_id, key);
                prompt_refused(registry, format!("the machine did not confirm: {other:?}"))
            }
        }
    } else {
        // The dismissal: the machine clears the stage AND the armed mark
        // itself (no settle is needed — its own semantics); the desc
        // consumes on this edge too.
        let dismissed = handle.dispatcher.borrow_mut().dismiss_prompt(key.clone());
        registry.take_prompt_dialog_owned(surface_id, key);
        if !dismissed {
            return prompt_refused(
                registry,
                format!("surface {} had no prompt pending on the machine", surface_id.0),
            );
        }
        registry.record_prompt_dismissed_owned();
        PromptBridgeOutcome {
            routing: PromptRouting::Dismissed,
            events: Vec::new(),
            settled: false,
        }
    }
}

/// The headless tests' facade: the SAME [`route_prompt_decision_owned`]
/// core over the mount module's cfg(test) registry cell (the
/// pre-migration static signature — see the seam block in
/// `plugin_widget_mount.rs`).
#[cfg(test)]
pub(crate) fn route_prompt_decision(
    surface_id: StableId,
    key: &DispatchKey,
    confirming: bool,
) -> PromptBridgeOutcome {
    crate::ui::window::plugin_widget_mount::with_test_registry_mut(|registry| {
        route_prompt_decision_owned(registry, surface_id, key, confirming)
    })
}

impl OpenCADStudio {
    /// The `Message::WidgetPromptMsg` arm's handler: route the modal's
    /// decision into its registered surface's dispatcher
    /// ([`route_prompt_decision_owned`]) and drop the drained events at the
    /// collecting seam (the same pre-wire hang-off as the mount bridge's
    /// arm; the publish is the ADR-004 crossing).
    ///
    /// UNKNOWN surface / UNGUARDED decision: a graceful no-op — no panic,
    /// nothing routed, no event, only the refusal probe.
    ///
    /// SCN-016: THIS is the decision path the modal body's buttons
    /// (SCN-015's [`prompt_modal_element`], mapped through
    /// [`prompt_app_message`]) emit into, and the modal's ✕ too (the view
    /// composes its close as the SAME Dismissed decision for this kind).
    /// A decision for the surface whose prompt the modal shows ALSO clears
    /// the modal: the reveal state + the modal slot release, and the next
    /// queued armed prompt (if any) reveals at this boundary.
    pub(in crate::app) fn on_widget_prompt_msg(
        &mut self,
        msg: WidgetPromptMsg,
    ) -> Task<Message> {
        let (surface_id, key, confirming) = match &msg {
            WidgetPromptMsg::Confirmed { surface_id, key } => (*surface_id, key.clone(), true),
            WidgetPromptMsg::Dismissed { surface_id, key } => (*surface_id, key.clone(), false),
        };
        let outcome = route_prompt_decision_owned(
            &mut self.mount_registry.borrow_mut(),
            surface_id,
            &key,
            confirming,
        );
        // Pre-wire drain — the collecting sink's contract (see the module
        // docs); the publish hang-off is the ADR-004 crossing.
        let _ = outcome.events;
        // SCN-016: the decision completes the interaction the modal stands
        // for — the modal closes through the SAME edge (exactly-once: any
        // refused/mismatched decision names the armed surface and closes
        // the modal the desc's read supports; the machine edges were the
        // routing's to run, once).
        self.close_widget_prompt_modal_for(surface_id);
        Task::none()
    }

    /// A `PromptRequested` bridge routing ARMS the app's prompt-modal flow
    /// (SCN-016, the SHOW POLICY's entry): the surface's armed desc is
    /// already surface-ready in the registry; this records the modal
    /// request. A re-arm of an already-queued surface moves the request to
    /// the back (the LATEST armed interaction reveals first — the desc the
    /// registry keeps is the re-armed one), then the reveal check runs.
    pub(in crate::app) fn arm_widget_prompt_modal(&mut self, surface_id: StableId) {
        self.widget_prompt_modal_pending
            .retain(|queued| *queued != surface_id);
        self.widget_prompt_modal_pending.push_back(surface_id);
        self.show_next_widget_prompt_modal();
    }

    /// Reveal the next queued armed prompt when the modal slot is free
    /// (SCN-016's SHOW POLICY; the `pending_startup_modals` precedent):
    /// a modal-free arm reveals NOW; an armed prompt behind another modal
    /// waits at a boundary. THE LANDED DRIVERS (all owned): the prompt
    /// modal's own close chain (its ✕-composed Dismissed decision, the
    /// body's Cancel/OK, the Escape tail), and every later arm's check —
    /// each of them calls this. The generic third-party close path
    /// (update/mod.rs's post-message hook — not this task's files) does
    /// NOT run this check; a prompt queued behind a foreign modal reveals
    /// at the next owned boundary rather than that hook (RECORDED — the
    /// one-line neighbor of `show_next_startup_modal()` remains the
    /// coordinator's option; the queue is inert-safe until then). The
    /// front's desc must still stand (a surface whose interaction was
    /// consumed/removed in the meantime is skipped quietly — the machine
    /// layers stay total).
    pub(in crate::app) fn show_next_widget_prompt_modal(&mut self) {
        while self.active_modal.is_none()
            && self.widget_prompt_modal.is_none()
            && !self.widget_prompt_modal_pending.is_empty()
        {
            let front = self
                .widget_prompt_modal_pending
                .pop_front()
                .expect("the front was non-empty");
            if self.mount_registry.borrow().prompt_dialog_owned(front).is_some() {
                self.widget_prompt_modal = Some(front);
                self.active_modal = Some(crate::app::ModalKind::WidgetPrompt);
                self.reset_modal_geometry();
                return;
            }
        }
    }

    /// The decision path's modal tail: the decision named the surface
    /// whose prompt the modal displays → clear the reveal state, close the
    /// modal, and chain the next armed prompt's reveal at this modal-close
    /// boundary. A decision naming a DIFFERENT surface (a refusal's
    /// graceful probe path) leaves the modal standing.
    fn close_widget_prompt_modal_for(&mut self, surface_id: StableId) {
        if self.widget_prompt_modal != Some(surface_id) {
            return;
        }
        self.widget_prompt_modal = None;
        if self.active_modal == Some(crate::app::ModalKind::WidgetPrompt) {
            self.close_active_modal();
        }
        self.show_next_widget_prompt_modal();
    }

    /// The mount keyboard route's empty tail ([`Self::on_mount_key`]): an
    /// Escape inside a mount-focused keyboard runs the machine-direct
    /// cancel, which produces NO route message — when the prompt modal is
    /// showing and its desc was consumed by that cancel, the modal closes
    /// here (Escape stays 1:1 with the armed interaction: the machine edge
    /// ran once, the modal follows).
    pub(in crate::app) fn close_widget_prompt_modal_if_consumed(&mut self) {
        if self.active_modal != Some(crate::app::ModalKind::WidgetPrompt) {
            return;
        }
        let Some(surface_id) = self.widget_prompt_modal else {
            return;
        };
        if self
            .mount_registry
            .borrow().prompt_dialog_owned(surface_id)
            .is_none()
        {
            self.widget_prompt_modal = None;
            self.close_active_modal();
            self.show_next_widget_prompt_modal();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (headless, deterministic; mirror `widget_mount.rs`'s style — the
// arm's full path minus the iced view, `OpenCADStudio::new_for_test()`
// underpins each test, NO renderer, NO Tasks, no timing).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::update::widget_mount::{handle_mount_key, route_mount_msg};
    use crate::ui::window::plugin_widget_mount::{
        MountCounters, MountPromptDesc, MountPromptFacts, MountRouting, SurfaceMountHandle,
        WidgetMountMsg, prompt_app_message, prompt_modal_element, register_test_surface,
    };
    use ocs_plugin_api::scene::{SceneRevision, StableId};
    use ocs_plugin_api::widgets::{
        ConfirmPolicy, IconActionDef, IconRef, ItemDef, ItemListDef, WidgetNode, WidgetSurface,
    };

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

    /// The prompt-policy button def (the `ConfirmPolicy::Prompt` form this
    /// lifecycle is about). Published IN the surface too (the arm's
    /// command resolution reads the surface's published defs — see
    /// `MountRegistry::resolve_prompt_desc`).
    fn prompt_button(id: u64, command: &str, message: &str) -> IconActionDef {
        IconActionDef {
            confirmation: Some(ConfirmPolicy::Prompt {
                message: message.to_string(),
            }),
            ..button_def(id, "Delete selection", command)
        }
    }

    fn surface(roots: Vec<WidgetNode>) -> WidgetSurface {
        WidgetSurface {
            revision: SceneRevision(1),
            roots,
            tables: std::collections::BTreeMap::new(),
        }
    }

    const PROMPT_A: &str = "Really delete the selection?";
    const PROMPT_B: &str = "Close the tool anyway?";

    /// The standard fixture's surface (button 1 → command "delete" +
    /// [`PROMPT_A`]). The mount state opens under the FIRST root's id
    /// (200 — the list — see `register_test_surface`).
    fn standard_surface() -> WidgetSurface {
        surface(vec![
            WidgetNode::List(item_list(200, vec![item(7, "row 1"), item(8, "row 2")])),
            WidgetNode::IconButton(prompt_button(1, "delete", PROMPT_A)),
        ])
    }

    /// One ISOLATED fixture's surface: its own list id (rows `a`, `b`) and
    /// its own prompt button — the first root is the list, so the surface
    /// id IS the list id.
    fn isolated_surface(
        list_id: u64,
        row_a: u64,
        row_b: u64,
        button_id: u64,
        command: &str,
        message: &str,
    ) -> WidgetSurface {
        surface(vec![
            WidgetNode::List(item_list(
                list_id,
                vec![item(row_a, "row a"), item(row_b, "row b")],
            )),
            WidgetNode::IconButton(prompt_button(button_id, command, message)),
        ])
    }

    /// Register `panel` over an isolated surface; return the handle + the
    /// teardown guard (RAII — the registry stays thread-local-clean).
    fn register_isolated(
        panel: &str,
    ) -> (
        crate::ui::window::plugin_widget_mount::RegisteredGuard,
        SurfaceMountHandle,
    ) {
        let guard = register_test_surface(panel, standard_surface());
        let handle = MountRegistry::handle_for_surface(sid(200)).expect("the registered surface");
        (guard, handle)
    }

    /// Sync the fixture button's def (the owner plugin's publish step;
    /// the surface's registry machines carry it from there).
    fn sync_button(handle: &SurfaceMountHandle, def: &IconActionDef) {
        handle.dispatcher.borrow_mut().sync_button(def);
    }

    /// Select `row` through the bridge (the command targets the confirm
    /// names). Panics the test on a refused fixture operation.
    fn select(handle: &SurfaceMountHandle, surface_id: u64, list_id: u64, row: u64) {
        let selected = route_mount_msg(
            handle,
            &WidgetMountMsg::SelectionChanged {
                surface_id: sid(surface_id),
                widget_id: sid(list_id),
                selected: vec![sid(row)],
            },
        );
        assert!(
            matches!(selected.routing, MountRouting::Applied { .. }),
            "the selection fixture applies: {:?}",
            selected.routing
        );
    }

    /// Select row 7 (the standard fixture).
    fn select_row_7(handle: &SurfaceMountHandle) {
        select(handle, 200, 200, 7);
    }

    fn click_button(surface_id: u64, button_id: u64) -> WidgetMountMsg {
        WidgetMountMsg::ButtonClicked {
            surface_id: sid(surface_id),
            widget_id: sid(button_id),
        }
    }

    /// Arm the standard fixture's confirmation through the REAL bridge
    /// route (which records the desc + the escape hint) and assert the
    /// surface-ready desc landed. Returns the arm outcome.
    fn arm_standard(handle: &SurfaceMountHandle) -> crate::app::update::widget_mount::MountBridgeOutcome {
        let click = click_button(200, 1);
        let arm = route_mount_msg(handle, &click);
        assert!(
            matches!(arm.routing, MountRouting::PromptRequested { .. }),
            "the arm surfaces the prompt: {:?}",
            arm.routing
        );
        assert!(arm.events.is_empty(), "a prompt request emits no event");
        assert!(
            MountRegistry::prompt_dialog(handle.surface_id()).is_some(),
            "the armed desc is surface-ready"
        );
        arm
    }

    // -- 1. the full headless lifecycle --------------------------------------

    /// dispatch → surface-ready arm → confirm → event + settle: the
    /// dispatcher's counters EXACT (one resolved activation, one requested
    /// prompt, one completed confirmation, one command emitted), the
    /// registry's own counters EXACT, and the desc consumed by the
    /// completion.
    #[test]
    fn prompt_lifecycle_headless_completes() {
        let _app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_lifecycle");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);

        // Everything before the decision fixture: full counter snapshots,
        // so the lifecycle's deltas are the only thing asserted.
        let before = MountRegistry::counters();
        let machine_before = *handle.dispatcher.borrow().counters();

        let arm = arm_standard(&handle);
        assert_eq!(
            arm.routing,
            MountRouting::PromptRequested {
                prompt: PROMPT_A.to_string(),
            }
        );
        let key = DispatchKey::Button(sid(1));
        {
            let machine = *handle.dispatcher.borrow().counters();
            assert_eq!(machine.confirmations_requested - machine_before.confirmations_requested, 1);
            assert_eq!(machine.confirmations_completed - machine_before.confirmations_completed, 0);
            assert_eq!(machine.commands_emitted - machine_before.commands_emitted, 0);
        }
        // The desc is surface-ready with the arm-time facts.
        let desc = MountRegistry::prompt_dialog(sid(200)).expect("the armed desc");
        assert_eq!(desc.surface_id, sid(200));
        assert_eq!(desc.key, key);
        assert_eq!(desc.prompt, PROMPT_A);
        assert_eq!(desc.command_id, "delete");
        assert_eq!(desc.targets, vec![sid(7)]);

        // The decision: OK — the captured command emits exactly once,
        // forwarded through the collecting sink and settled by the same
        // policy the click path uses.
        let confirmed = route_prompt_decision(sid(200), &key, true);
        assert_eq!(confirmed.routing, PromptRouting::Confirmed { forwarded: 1 });
        assert!(confirmed.settled, "the confirm's settle tail cleared the mark");
        assert_eq!(
            confirmed.events,
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: "delete".to_string(),
                target_ids: vec![sid(7)],
            }],
            "the dispatch-captured command + targets, exactly once"
        );

        // The registry state: the decision consumed the desc; nothing
        // lingers for a second decision.
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), None);

        // The dispatcher's counters EXACT (the machine's own §16 counts).
        {
            let machine = *handle.dispatcher.borrow().counters();
            assert_eq!(machine.confirmations_requested - machine_before.confirmations_requested, 1);
            assert_eq!(
                machine.confirmations_completed - machine_before.confirmations_completed,
                1
            );
            assert_eq!(machine.commands_emitted - machine_before.commands_emitted, 1);
            assert_eq!(
                machine.activations_resolved - machine_before.activations_resolved,
                1
            );
            assert_eq!(machine.activations_refused_disabled, 0);
            assert_eq!(machine.activations_refused_in_flight, 0);
        }
        // And nothing remains outstanding: settle finds nothing.
        assert!(
            !handle.dispatcher.borrow_mut().settle(key.clone()),
            "the bridge's settle already released the address"
        );
        assert_eq!(
            handle
                .dispatcher
                .borrow()
                .pending_prompt(&key),
            None
        );

        // The bridge counters EXACT (the deltas over the whole lifecycle).
        let after = MountRegistry::counters();
        assert_eq!(after.prompts_surfaced - before.prompts_surfaced, 1);
        assert_eq!(after.prompts_confirmed - before.prompts_confirmed, 1);
        assert_eq!(after.prompts_dismissed - before.prompts_dismissed, 0);
        assert_eq!(after.prompts_refused - before.prompts_refused, 0);
    }

    // -- 2. the dismissal: no event, re-promptable ----------------------------

    /// The Cancel decision: the armed stage + mark clear, NO event exists,
    /// no settle is needed — and a later genuine click RE-PROMPTS (the
    /// policy is per-def, not per-machine).
    #[test]
    fn dismiss_clears_without_event() {
        let _app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_dismiss");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        let key = DispatchKey::Button(sid(1));
        arm_standard(&handle);

        let before = MountRegistry::counters();
        let machine_before = *handle.dispatcher.borrow().counters();

        let dismissed = route_prompt_decision(sid(200), &key, false);
        assert_eq!(dismissed.routing, PromptRouting::Dismissed);
        assert_eq!(
            dismissed.events,
            Vec::new(),
            "a dismissal never emits an event"
        );
        assert!(
            !dismissed.settled,
            "the dismissal IS the release; the bridge settles nothing"
        );
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), None);
        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None);

        // The counters: only the dismissal probe; the machines' emit/confirm
        // totals never moved (nothing was ever emitted by this path).
        let after = MountRegistry::counters();
        assert_eq!(after.prompts_dismissed - before.prompts_dismissed, 1);
        assert_eq!(after.prompts_confirmed - before.prompts_confirmed, 0);
        assert_eq!(after.prompts_refused - before.prompts_refused, 0);
        assert_eq!(after.prompts_surfaced - before.prompts_surfaced, 0);
        let machine = *handle.dispatcher.borrow().counters();
        assert_eq!(machine.commands_emitted, machine_before.commands_emitted);
        assert_eq!(
            machine.confirmations_completed,
            machine_before.confirmations_completed
        );

        // The re-prompt: the per-def policy arms the address again — the
        // counters show one more surfaced request, the desc is ready again.
        let re_arm = route_mount_msg(&handle, &click_button(200, 1));
        assert_eq!(
            re_arm.routing,
            MountRouting::PromptRequested {
                prompt: PROMPT_A.to_string(),
            }
        );
        assert!(
            handle
                .dispatcher
                .borrow()
                .pending_prompt(&key)
                .is_some()
        );
        assert!(MountRegistry::prompt_dialog(sid(200)).is_some());
        assert_eq!(
            MountRegistry::counters().prompts_surfaced - before.prompts_surfaced,
            1,
            "the second surfacing ticked"
        );
    }

    // -- 3. multi-surface isolation -------------------------------------------

    /// Two prompts from two surfaces: confirming one does not touch the
    /// other's armed state — machine counters, pending stages, and descs
    /// stay exactly as they were for the untouched surface.
    #[test]
    fn two_surfaces_isolated() {
        let _app = OpenCADStudio::new_for_test();
        // Surface 200 in panel A; surface 210 in panel B (each fixture's
        // FIRST root is its list, so the surface ids are the lists'). Each
        // panel's def is synced to MATCH its published surface def (the
        // desc's arm-time resolution reads the surface; the machine reads
        // the sync — a fixture keeps them one truth).
        let (_guard_a, handle_a) = register_isolated("scn015_iso_a");
        let button_def_a = prompt_button(1, "delete", PROMPT_A);
        sync_button(&handle_a, &button_def_a);
        select(&handle_a, 200, 200, 7);

        let surface_b = isolated_surface(210, 9, 10, 110, "delete-b", PROMPT_B);
        let _guard_b = register_test_surface("scn015_iso_b", surface_b);
        let handle_b = MountRegistry::handle_for_surface(sid(210)).expect("panel B registered");
        let button_def_b = prompt_button(110, "delete-b", PROMPT_B);
        sync_button(&handle_b, &button_def_b);
        select(&handle_b, 210, 210, 9);

        // Both armed (each through the real bridge route).
        arm_standard(&handle_a);
        let arm_b = route_mount_msg(&handle_b, &click_button(210, 110));
        assert!(matches!(arm_b.routing, MountRouting::PromptRequested { .. }));

        let key_a = DispatchKey::Button(sid(1));
        let key_b = DispatchKey::Button(sid(110));
        let desc_a = MountRegistry::prompt_dialog(sid(200)).expect("A's armed desc");
        let desc_b = MountRegistry::prompt_dialog(sid(210)).expect("B's armed desc");
        assert_eq!(desc_a.command_id, "delete");
        assert_eq!(desc_b.command_id, "delete-b");
        assert_eq!(desc_a.targets, vec![sid(7)]);
        assert_eq!(desc_b.targets, vec![sid(9)]);

        // FULL snapshots over both machines, then confirm A.
        let before = MountRegistry::counters();
        let machine_a_before = *handle_a.dispatcher.borrow().counters();
        let machine_b_before = *handle_b.dispatcher.borrow().counters();

        let confirmed = route_prompt_decision(sid(200), &key_a, true);
        assert_eq!(confirmed.routing, PromptRouting::Confirmed { forwarded: 1 });
        assert_eq!(
            confirmed.events,
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: "delete".to_string(),
                target_ids: vec![sid(7)],
            }]
        );

        // A: done and drained.
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), None);
        assert_eq!(handle_a.dispatcher.borrow().pending_prompt(&key_a), None);

        // B: UNTOUCHED by A's confirm — the desc, the pending stage and
        // every machine counter stand as they were.
        let desc_b_now = MountRegistry::prompt_dialog(sid(210)).expect("B's desc survives A");
        assert_eq!(desc_b_now, desc_b, "B's desc fields unchanged");
        assert!(handle_b.dispatcher.borrow().pending_prompt(&key_b).is_some());
        let machine_b = *handle_b.dispatcher.borrow().counters();
        assert_eq!(machine_b, machine_b_before, "B's machine counters untouched");

        // The registry counters moved only for A's decision.
        let after = MountRegistry::counters();
        assert_eq!(after.prompts_confirmed - before.prompts_confirmed, 1);
        assert_eq!(after.prompts_dismissed - before.prompts_dismissed, 0);
        assert_eq!(after.prompts_refused - before.prompts_refused, 0);
        assert_eq!(after.prompts_surfaced - before.prompts_surfaced, 0);

        // And B's own dismissal still completes after (isolation cuts both
        // ways — each decision is its own lifecycle).
        let dismissed_b = route_prompt_decision(sid(210), &key_b, false);
        assert_eq!(dismissed_b.routing, PromptRouting::Dismissed);
        assert_eq!(
            dismissed_b.events,
            Vec::new(),
            "B's dismissal emits nothing"
        );
        assert_eq!(MountRegistry::prompt_dialog(sid(210)), None);
        {
            let machine_b_now = *handle_b.dispatcher.borrow().counters();
            assert_eq!(
                machine_b_now.confirmations_completed,
                machine_b_before.confirmations_completed,
                "B's machine still never emitted anything"
            );
        }
        // A's machine exactly once (the confirm path moved only A's).
        let machine_a_now = *handle_a.dispatcher.borrow().counters();
        assert_eq!(
            machine_a_now.confirmations_completed - machine_a_before.confirmations_completed,
            1
        );
        assert_eq!(
            machine_a_now.commands_emitted - machine_a_before.commands_emitted,
            1
        );
    }

    // -- 4. graceful refusals (unknown surface / unknown address / mismatch) --

    /// A decision for an unregistered surface (direct handler AND the full
    /// update arm), a registered surface with an unknown address, and a
    /// registered surface whose armed address is of the other kind — all
    /// refused gracefully: no panic, no event, nothing half-applied, only
    /// the refusal probe, each refusal exactly once.
    #[test]
    fn handler_refuses_unknown_surface_or_key_gracefully() {
        let mut app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_refuse");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        arm_standard(&handle);
        let key = DispatchKey::Button(sid(1));

        let before = MountRegistry::counters();
        let machine_before = *handle.dispatcher.borrow().counters();

        // (a) unregistered surface, direct handler: quiet.
        app.on_widget_prompt_msg(WidgetPromptMsg::Confirmed {
            surface_id: sid(42_424),
            key: key.clone(),
        });
        // (b) unregistered surface, the FULL update arm (the tail
        // variant's own path): quiet.
        app.update(Message::WidgetPromptMsg(WidgetPromptMsg::Dismissed {
            surface_id: sid(42_424),
            key: key.clone(),
        }));
        // (c) a registered surface, an address that is not the armed one:
        // the guard refuses BEFORE the machine.
        app.update(Message::WidgetPromptMsg(WidgetPromptMsg::Confirmed {
            surface_id: sid(200),
            key: DispatchKey::Button(sid(999)),
        }));
        // (d) the armed address's OTHER kind (the dialog named a button;
        // a message arrived naming a menu item address): mismatched key.
        app.update(Message::WidgetPromptMsg(WidgetPromptMsg::Confirmed {
            surface_id: sid(200),
            key: DispatchKey::MenuItem(sid(1), "delete".to_string()),
        }));

        // Four refusals, each exactly its own probe; NOTHING else moved —
        // the armed desc still stands and the machines saw nothing.
        let after = MountRegistry::counters();
        assert_eq!(after.prompts_refused - before.prompts_refused, 4);
        assert_eq!(after.prompts_confirmed - before.prompts_confirmed, 0);
        assert_eq!(after.prompts_dismissed - before.prompts_dismissed, 0);
        assert_eq!(after.prompts_surfaced - before.prompts_surfaced, 0);
        let machine_now = *handle.dispatcher.borrow().counters();
        assert_eq!(machine_now, machine_before, "no machine edge was touched");
        assert!(
            MountRegistry::prompt_dialog(sid(200)).is_some(),
            "the refusals did not take down the armed desc"
        );

        // The armed interaction still completes after ALL that (refusals
        // left nothing half-applied).
        let confirmed = route_prompt_decision(sid(200), &key, true);
        assert_eq!(confirmed.routing, PromptRouting::Confirmed { forwarded: 1 });
        assert_eq!(
            confirmed.events,
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: "delete".to_string(),
                target_ids: vec![sid(7)],
            }]
        );
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), None);
    }

    // -- 5. confirm with nothing pending refuses ------------------------------

    /// The decision's guard refuses a confirm when NO prompt is pending
    /// (registered surface, synced def, no arm) — exactly one refusal
    /// probe, the machine edges never entered (its ConfirmWithoutPrompt
    /// state is the SECOND line of defense; the handler's registry gate
    /// stands in front of it).
    #[test]
    fn confirm_without_prompt_refuses() {
        let _app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_noarm");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        let key = DispatchKey::Button(sid(1));
        assert!(
            MountRegistry::prompt_dialog(sid(200)).is_none(),
            "nothing armed yet"
        );

        let before = MountRegistry::counters();
        let machine_before = *handle.dispatcher.borrow().counters();

        let confirmed = route_prompt_decision(sid(200), &key, true);
        assert_eq!(
            confirmed.routing,
            PromptRouting::Refused("surface 200 has no prompt pending to decide".to_string())
        );
        assert_eq!(confirmed.events, Vec::new());
        assert!(!confirmed.settled);

        // The machine never saw the decision (its own confirm-without-prompt
        // state is never reached from this path).
        let machine_now = *handle.dispatcher.borrow().counters();
        assert_eq!(machine_now, machine_before);
        // ...and the machine's own refusal edge exists beneath (the
        // SWG-003 pin, restated for the handler's depth; the address is
        // the button's widget id per the machine's refusal convention).
        let machine_direct = handle.dispatcher.borrow_mut().confirm(key);
        assert_eq!(
            machine_direct.err(),
            Some(crate::ui::window::plugin_widgets_dispatch::DispatchRefusal::ConfirmWithoutPrompt { address: sid(1) })
        );
        let machine_after_direct = *handle.dispatcher.borrow().counters();
        assert_eq!(
            machine_after_direct, machine_before,
            "the machine's refusal moves no counters either"
        );

        let after = MountRegistry::counters();
        assert_eq!(
            after.prompts_refused - before.prompts_refused,
            1,
            "one handler refusal (the direct machine call moves none)"
        );
    }

    // -- 6. the desc accessor's shape (exact fields; read keeps; consume) ----

    /// The desc's fields are EXACT (surface, key, prompt, command,
    /// targets), the READ keeps the desc in the registry (a view pass may
    /// re-read per frame), and the consumption removes it.
    #[test]
    fn prompt_desc_accessor_shape() {
        let _app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_desc");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        arm_standard(&handle);
        let key = DispatchKey::Button(sid(1));

        let desc = MountRegistry::prompt_dialog(sid(200)).expect("the armed desc");
        assert_eq!(
            desc,
            MountPromptDesc {
                surface_id: sid(200),
                key: key.clone(),
                prompt: PROMPT_A.to_string(),
                command_id: "delete".to_string(),
                targets: vec![sid(7)],
            },
            "the desc fields are exactly the arm-time facts"
        );

        // The READ keeps the desc (no consumption on access).
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), Some(desc.clone()));

        // The modal body builder off the desc: message text + the decision
        // pair, addressed with the desc's (surface, key).
        let (_element, facts) = prompt_modal_element(&desc);
        assert_eq!(
            facts,
            MountPromptFacts {
                prompt: PROMPT_A.to_string(),
                ok: "OK".to_string(),
                cancel: "Cancel".to_string(),
            },
            "the modal body: the message text + the two decision buttons"
        );

        // The consumption removes it.
        assert!(MountRegistry::take_prompt_dialog(sid(200), &key));
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), None);
        // A second take is a graceful false (nothing matched twice).
        assert!(!MountRegistry::take_prompt_dialog(sid(200), &key));
    }

    // -- 7. the decision's message round trip through the app -----------------

    /// Mount-msg arm (`PromptRequested`) → the decision's message mapped by
    /// `prompt_app_message` → `app.update` (the FULL tail variant arm) →
    /// the decision routed. Counters + registry state assert the effect.
    #[test]
    fn message_roundtrip_via_app_message() {
        let mut app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_roundtrip");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        arm_standard(&handle);
        let key = DispatchKey::Button(sid(1));

        let decision = WidgetPromptMsg::Confirmed {
            surface_id: sid(200),
            key: key.clone(),
        };
        // The single mapping: the arm's payload unwraps back to the exact
        // message (`Message` itself carries no PartialEq — the enum is not
        // an equality type; the VARIANT's payload is).
        let app_msg = prompt_app_message(decision.clone());
        let Message::WidgetPromptMsg(routed_msg) = app_msg else {
            unreachable!("prompt_app_message wraps exactly this variant")
        };
        assert_eq!(routed_msg, decision);

        // The full app arm routes the decision (the handler per the update
        // tree; the app update tree is exercised for real here). Counters
        // snapshotted around the arm so the deltas assert the effect. The
        // returned task (a `Task::none()` here — the decision route spawns
        // nothing) is dropped: no side effect to await.
        let before = MountRegistry::counters();
        let machine_before = *handle.dispatcher.borrow().counters();
        let _ = app.update(Message::WidgetPromptMsg(routed_msg));

        // The effect: the confirm ran through the decision route — the
        // counters and the registry state say exactly one completed
        // interaction.
        assert_eq!(
            handle.dispatcher.borrow().pending_prompt(&key),
            None,
            "the machine stage completed"
        );
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), None);
        let machine = *handle.dispatcher.borrow().counters();
        assert_eq!(
            machine.confirmations_completed - machine_before.confirmations_completed,
            1
        );
        assert_eq!(machine.commands_emitted - machine_before.commands_emitted, 1);
        let after = MountRegistry::counters();
        assert_eq!(after.prompts_confirmed - before.prompts_confirmed, 1);
        assert_eq!(after.prompts_refused - before.prompts_refused, 0);
        assert_eq!(after.prompts_dismissed - before.prompts_dismissed, 0);
    }

    // -- 8. the sink drains exactly the forwarded command ---------------------

    /// The bridge compose through the dialog path: the decision's confirm
    /// emits the captured command through the collecting sink, drained to
    /// EXACTLY one event (in emission order), whose command/targets are the
    /// desc's fields — the arm-time capture → emission identity — and a
    /// SECOND confirm after completion refuses (exactly once per
    /// interaction, dialogs included).
    #[test]
    fn sink_events_are_exactly_the_forwarded_command() {
        let _app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_sink");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        let key = DispatchKey::Button(sid(1));
        arm_standard(&handle);
        let desc = MountRegistry::prompt_dialog(sid(200)).expect("armed");

        let confirmed = route_prompt_decision(sid(200), &key, true);
        assert_eq!(confirmed.routing, PromptRouting::Confirmed { forwarded: 1 });
        assert_eq!(confirmed.events, vec![WidgetEvent::CommandInvoked {
            surface_id: sid(200),
            command_id: desc.command_id.clone(),
            target_ids: desc.targets.clone(),
        }], "the sink drained exactly the forwarded command, once, in order");

        // The second confirm: nothing armed anymore — refusal, and the
        // machine's emission total stays at one (never a replay).
        let replay = route_prompt_decision(sid(200), &key, true);
        assert!(matches!(replay.routing, PromptRouting::Refused(_)));
        assert_eq!(replay.events, Vec::new(), "no second event");
        let machine = *handle.dispatcher.borrow().counters();
        assert_eq!(machine.commands_emitted, 1);
        assert_eq!(machine.confirmations_completed, 1);
    }

    // -- 9. the counter probes move only their own ----------------------------

    /// The §16 probe discipline over the full registry counter set: a
    /// refusal moves ONLY `prompts_refused`; a dismissal moves ONLY
    /// `prompts_dismissed`; a confirm moves ONLY `prompts_surfaced` +
    /// `prompts_confirmed` (plus the machine's own). Whole-struct
    /// equality, no partial reads.
    #[test]
    fn counters_probe_discipline() {
        let _app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_probes");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        let key = DispatchKey::Button(sid(1));
        arm_standard(&handle);

        // (1) A refusal: only its own counter.
        let before = MountRegistry::counters();
        let refused = route_prompt_decision(sid(200), &DispatchKey::Button(sid(404)), true);
        assert!(matches!(refused.routing, PromptRouting::Refused(_)));
        let only_refused = MountCounters { prompts_refused: before.prompts_refused + 1, ..before };
        assert_eq!(MountRegistry::counters(), only_refused);

        // (2) A dismissal: only its own counter.
        let dismissed = route_prompt_decision(sid(200), &key, false);
        assert_eq!(dismissed.routing, PromptRouting::Dismissed);
        let only_dismissed = MountCounters { prompts_dismissed: before.prompts_dismissed + 1, ..only_refused };
        assert_eq!(MountRegistry::counters(), only_dismissed);

        // (3) A fresh arm + confirm: exactly surfaced + confirmed (the arm
        // is a bridge route too: messages_routed ticks with it; the
        // decision itself moves nothing but its own probe).
        let before2 = MountRegistry::counters();
        arm_standard(&handle);
        let with_new_arm = MountCounters {
            prompts_surfaced: before2.prompts_surfaced + 1,
            messages_routed: before2.messages_routed + 1,
            ..before2
        };
        assert_eq!(MountRegistry::counters(), with_new_arm);

        let confirmed = route_prompt_decision(sid(200), &key, true);
        assert_eq!(confirmed.routing, PromptRouting::Confirmed { forwarded: 1 });
        let after_confirm = MountCounters {
            prompts_surfaced: before2.prompts_surfaced + 1,
            prompts_confirmed: before2.prompts_confirmed + 1,
            messages_routed: before2.messages_routed + 1,
            ..before2
        };
        assert_eq!(MountRegistry::counters(), after_confirm);

        // The machine's counters: the whole lifecycle = 2 requested, 1
        // completed, 1 emitted (arm 1 + the re-arm; the dismissal cleared
        // the first request's stage without emitting; refusals never
        // entered).
        let machine = *handle.dispatcher.borrow().counters();
        assert_eq!(machine.confirmations_requested, 2);
        assert_eq!(machine.confirmations_completed, 1);
        assert_eq!(machine.commands_emitted, 1);
    }

    // -- 10b. SCN-016: the modal SHOW POLICY (the visual half) -----------------

    use crate::app::ModalKind;

    /// The arm with a FREE modal slot reveals the prompt modal immediately:
    /// `ModalKind::WidgetPrompt` holds the slot, keyed to the armed
    /// surface, and the modal body (the mount's builder over the
    /// registry's desc) is the armed interaction's display copy (the
    /// message text + the Cancel/OK pair).
    #[test]
    fn prompt_modal_reveals_on_arm() {
        let mut app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn016_reveal");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        assert_eq!(app.active_modal, None);

        // THE ARM — the click rides the app's full mount arm.
        let _ = app.update(Message::WidgetMountMsg(click_button(200, 1)));

        assert_eq!(app.active_modal, Some(ModalKind::WidgetPrompt), "the slot-free arm shows NOW");
        assert_eq!(app.widget_prompt_modal, Some(sid(200)), "keyed to the armed surface");
        // The modal's frame title + the body's display facts (the desc's
        // read is what the view pass would consume).
        assert_eq!(app.modal_title_for(ModalKind::WidgetPrompt), "Widget Confirmation");
        assert!(app.modal_content_for(ModalKind::WidgetPrompt, iced::Vector::ZERO).is_some(), "the modal body builds");
        let desc = MountRegistry::prompt_dialog(sid(200)).expect("the armed desc");
        let (_element, facts) = prompt_modal_element(&desc);
        assert_eq!(facts.prompt, PROMPT_A);
        assert_eq!(facts.ok, "OK");
        assert_eq!(facts.cancel, "Cancel");
    }

    /// No preemption: an arm while ANOTHER modal owns the slot queues and
    /// leaves that modal standing. The boundary reveal (the check the
    /// prompt-modal's own close chain and every later boundary runs) pops
    /// the queue at the next slot-free moment.
    #[test]
    fn arm_behind_modal_queues_then_reveals_at_boundary() {
        let mut app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn016_queue");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        // Another modal owns the slot.
        app.active_modal = Some(ModalKind::About);

        let _ = app.update(Message::WidgetMountMsg(click_button(200, 1)));

        assert_eq!(app.active_modal, Some(ModalKind::About), "no preemption");
        assert!(app.widget_prompt_modal.is_none(), "nothing showing");
        assert_eq!(app.widget_prompt_modal_pending.back(), Some(&sid(200)), "queued");

        // The other modal leaves (its own close path — no prompt mixin).
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.active_modal, None, "the blocker left");
        // The boundary reveal: the CHECK runs — the prompt modal's own close
        // chain (`close_widget_prompt_modal_for` / the Escape tail) and the
        // next arm's check call this at every boundary they own; the check
        // itself is the boundary semantics this test pins (see the wiring
        // note below for the drivers' landed truth).
        app.show_next_widget_prompt_modal();
        assert_eq!(app.active_modal, Some(ModalKind::WidgetPrompt));
        assert_eq!(app.widget_prompt_modal, Some(sid(200)));
        assert!(app.widget_prompt_modal_pending.is_empty(), "the queue drained");
    }

    /// The ✕ is the modal's CANCEL decision: `modal_close_msg` composes the
    /// shown desc's Dismissed edge (the SAME decision the body's Cancel
    /// button routes) — updating with it dismisses the machine's armed
    /// stage ONCE, consumes the desc, closes the modal, and the probe
    /// discipline is exact (one dismissed tick, nothing else).
    #[test]
    fn close_x_routes_dismissed_edge_once() {
        use crate::ui::window::plugin_widget_mount::WidgetPromptMsg as XMsg;

        let mut app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn016_closex");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        let _ = app.update(Message::WidgetMountMsg(click_button(200, 1)));
        let key = DispatchKey::Button(sid(1));
        let before = MountRegistry::counters();

        // The ✕'s composed message (what the shared overlay would emit).
        let x_msg = app.modal_close_msg();
        let Message::WidgetPromptMsg(XMsg::Dismissed { surface_id, key: msg_key }) = x_msg.clone()
        else {
            panic!("the prompt modal's ✕ is its Dismissed decision");
        };
        assert_eq!(surface_id, sid(200));
        assert_eq!(msg_key, key, "addressed with the armed desc's key");

        let _ = app.update(x_msg);

        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None, "the machine stage cleared");
        assert!(MountRegistry::prompt_dialog(sid(200)).is_none(), "the desc consumed");
        assert_eq!(app.active_modal, None, "the modal closed through the same edge");
        assert_eq!(app.widget_prompt_modal, None);
        let after = MountRegistry::counters();
        assert_eq!(after.prompts_dismissed - before.prompts_dismissed, 1, "the Dismissed edge, exactly once");
        assert_eq!(after.prompts_confirmed - before.prompts_confirmed, 0);
        assert_eq!(after.prompts_refused - before.prompts_refused, 0);
        // Unknown/gone interactions stay graceful: a second ✕ (nothing
        // armed or showing) is the ordinary CloseModal, a quiet no-op.
        assert!(
            matches!(app.modal_close_msg(), Message::CloseModal),
            "without a showing prompt the ✕ is the ordinary close"
        );
    }

    /// The OK decision: the body's confirm edge runs ONCE — the captured
    /// command emitted (the routing's event), the desc consumed, the modal
    /// closed by the same edge, and the probe discipline exact.
    #[test]
    fn ok_confirm_routes_confirmed_edge_once() {
        let mut app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn016_ok");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        let _ = app.update(Message::WidgetMountMsg(click_button(200, 1)));
        let key = DispatchKey::Button(sid(1));
        let machine_before = *handle.dispatcher.borrow().counters();
        let before = MountRegistry::counters();

        assert_eq!(app.active_modal, Some(ModalKind::WidgetPrompt));
        let _ = app.update(Message::WidgetPromptMsg(WidgetPromptMsg::Confirmed {
            surface_id: sid(200),
            key: key.clone(),
        }));

        assert_eq!(app.active_modal, None, "the decision closed the modal");
        assert_eq!(app.widget_prompt_modal, None);
        assert!(MountRegistry::prompt_dialog(sid(200)).is_none(), "the desc consumed");
        let machine = *handle.dispatcher.borrow().counters();
        assert_eq!(
            machine.commands_emitted - machine_before.commands_emitted,
            1,
            "the captured command emitted exactly once"
        );
        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None);
        let after = MountRegistry::counters();
        assert_eq!(after.prompts_confirmed - before.prompts_confirmed, 1);
        assert_eq!(after.prompts_dismissed - before.prompts_dismissed, 0);
        assert_eq!(after.prompts_refused - before.prompts_refused, 0);
    }

    /// Escape stays 1:1 with the armed interaction: the mount key route's
    /// machine-direct cancel consumes the desc, the `on_mount_key` empty
    /// tail closes the prompt modal, and the probe moved once (a second
    /// Escape with nothing armed moves nothing and cannot reopen).
    #[test]
    fn escape_dismisses_prompt_modal_once() {
        let mut app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn016_escape");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        MountRegistry::set_mount_key_target(Some(sid(200)));
        let _ = app.update(Message::WidgetMountMsg(click_button(200, 1)));
        let key = DispatchKey::Button(sid(1));
        let before = MountRegistry::counters();
        assert_eq!(app.active_modal, Some(ModalKind::WidgetPrompt));

        let _ = app.on_mount_key(
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            iced::keyboard::Modifiers::NONE,
        );

        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None);
        assert!(MountRegistry::prompt_dialog(sid(200)).is_none());
        assert_eq!(app.active_modal, None, "the escaped cancel closed the modal");
        assert_eq!(app.widget_prompt_modal, None);
        let after = MountRegistry::counters();
        assert_eq!(after.prompts_dismissed - before.prompts_dismissed, 1, "one dismissal for the one Escape");
        // 1:1 verified both ways: the NEXT Escape (nothing armed) moves
        // nothing.
        let _ = app.on_mount_key(
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            iced::keyboard::Modifiers::NONE,
        );
        assert_eq!(app.active_modal, None);
        assert_eq!(
            MountRegistry::counters().prompts_dismissed,
            after.prompts_dismissed,
            "no armed interaction, no edge"
        );
    }

    // -- 10. Escape lands in the same dismissal lifecycle ----------------------

    /// Escape (no route message, machine-direct) dismisses the armed prompt
    /// through the state layer's cancel edge — the desc consumed, the
    /// dismissal probe ticked, no events — and the next click re-prompts
    /// (the lifecycle continues), whose second Escape catches again.
    #[test]
    fn escape_route_dismisses_through_state_layer() {
        let _app = OpenCADStudio::new_for_test();
        let (_guard, handle) = register_isolated("scn015_escape");
        sync_button(&handle, &prompt_button(1, "delete", PROMPT_A));
        select_row_7(&handle);
        MountRegistry::set_mount_key_target(Some(sid(200)));
        let key = DispatchKey::Button(sid(1));
        arm_standard(&handle);

        let before = MountRegistry::counters();
        let machine_before = *handle.dispatcher.borrow().counters();

        // ESCAPE: no navigation message; the state layer's cancel edge ran.
        let msgs = handle_mount_key(
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            iced::keyboard::Modifiers::NONE,
        );
        assert!(msgs.is_empty(), "escape routes no navigation message");
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), None, "the desc left");
        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None);
        assert!(!MountRegistry::mount_escape_cancel(sid(200)), "the hint consumed");

        let after_escape = MountRegistry::counters();
        assert_eq!(after_escape.prompts_dismissed - before.prompts_dismissed, 1);
        assert_eq!(after_escape.prompts_confirmed - before.prompts_confirmed, 0);
        assert_eq!(after_escape.prompts_refused - before.prompts_refused, 0);
        // And no event anywhere on the machine (a cancel never emits).
        let machine_after = *handle.dispatcher.borrow().counters();
        assert_eq!(machine_after.commands_emitted, machine_before.commands_emitted);
        assert_eq!(
            machine_after.confirmations_completed,
            machine_before.confirmations_completed
        );

        // The lifecycle continues: the next click re-prompts (surfaced
        // again, desc ready), and the escape edge catches it once more —
        // same lifecycle, per interaction.
        arm_standard(&handle);
        assert!(MountRegistry::prompt_dialog(sid(200)).is_some());
        assert!(
            MountRegistry::mount_escape_cancel(sid(200)),
            "the re-armed hint is named for escape again"
        );
        assert_eq!(MountRegistry::prompt_dialog(sid(200)), None);
        assert_eq!(handle.dispatcher.borrow().pending_prompt(&key), None);
    }
}
