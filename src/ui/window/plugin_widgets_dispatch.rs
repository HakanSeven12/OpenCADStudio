//! Renderer-free semantic dispatch state machine for icon buttons and menu
//! commands (`SWG-003` of the scene-UI architecture, ARCHITECTURE §§12–16 +
//! ADR-005 in project-management).
//!
//! This is the icon/menu sibling of [`crate::ui::window::plugin_widgets`]
//! (SWG-002); the two COMPOSE — that module owns tree/list interaction
//! state, and the machine here owns the *resolved command decision*: whether
//! an activation IS a command, whether a confirmation policy must fire
//! first, and whether exactly ONE [`WidgetEvent::CommandInvoked`] leaves the
//! machine (never two — "one activation = exactly one semantic command",
//! ARCHITECTURE §16).
//!
//! Nothing here constructs an iced widget, a `Message`, or any renderer
//! type. The native-iced MOUNT is a LATER integration task consuming this
//! machine: it maps [`ControlSemantics`] onto iced widgets, routes click /
//! context-menu activations through [`WidgetDispatcher::dispatch`], shows a
//! dialog for [`DispatchOutcome::PromptRequested`], and calls
//! [`WidgetDispatcher::settle`] after forwarding each emitted event. Message
//! types, widget construction, and keyboard bindings belong to the mount —
//! the same renderer-free/headless discipline as [`crate::ui::window::plugin_scene`]
//! and `plugin_widgets.rs`.
//!
//! # The MCP half is the SAME machine
//!
//! SWG-005 (widget MCP parity) reuses exactly this dispatcher for command
//! invocations. The enable/visible resolution below is the GUI-side half of
//! ARCHITECTURE §16 "disabled commands un-invocable from GUI AND MCP"; the
//! MCP half calls the same `sync_*` + `dispatch` path (no screen
//! coordinates anywhere — §15), so both surfaces share one refusal model,
//! one validation path, and one event shape.
//!
//! # Ownership and the read model
//!
//! Plugin semantic data flows in ONLY through
//! [`WidgetDispatcher::sync_button`] / [`WidgetDispatcher::sync_menu`]
//! capturing the `SWG-001` contracts ([`IconActionDef`], [`MenuDef`],
//! [`MenuItemDef`]). Effective enablement/visibility resolves at sync time
//! as the multiplicative fold of widget-level AND item-level flags down the
//! submenu chain: a disabled ancestor disables every descendant item, an
//! invisible one hides them. A resolved-disabled or resolved-invisible
//! address can never emit a command (refusal / no event / counter).
//!
//! # Confirmation lifecycle
//!
//! A def declaring `ConfirmPolicy::Prompt` is a two-step activation:
//! 1. `dispatch` → [`DispatchOutcome::PromptRequested`]: the policy's prompt
//!    is REQUESTED — no event yet — and the address arms (the prompt IS the
//!    armed interaction; a re-activation while the dialog is up refuses
//!    exactly like a double click).
//! 2. `confirm` → [`DispatchOutcome::ConfirmedEmitted`]: the user OK'd; the
//!    event leaves the machine NOW, exactly once.
//! The user CANCELS via [`WidgetDispatcher::dismiss_prompt`]: the pending
//! stage and the armed mark clear, no event, and a later re-click re-prompts
//! (the policy is per-def, not per-machine). The pending stage CAPTURES the
//! command and targets the arming `dispatch` validated — a concurrent
//! re-sync cannot substitute a different command into an interaction the
//! user is currently confirming.
//!
//! # Exactly-once discipline
//!
//! After an outcome (emitted, confirmed, or a requested prompt) the address
//! is marked in flight until the mount settles it — EXCEPT pending
//! prompts: [`WidgetDispatcher::settle`] refuses to clear an armed dialog's
//! mark; only `confirm`/`dismiss_prompt` take it down (otherwise a
//! double-activation could disarm the dialog's re-click guard). A SECOND activation of a
//! still-unsettled address is refused
//! ([`DispatchRefusal::AlreadyInFlight`]) with no event — a double click
//! cannot double-invoke. A genuine second interaction (after settle) emits
//! again: exactly once PER interaction, not ever.
//!
//! # Event validity (§16 / §15)
//!
//! Every command event is bound through the crate's own [`WidgetEvent::validate`]
//! before it counts as emitted — the construct-level companion of the
//! bounded decoders. This module re-implements NEITHER the bounds NOR the
//! non-empty-command-id rule; one failed validation refuses BEFORE anything
//! is armed or marked, so a defective input can never wedge an address
//! half-dispatched.
//!
//! # Addresses
//!
//! Icon buttons are addressed by their [`StableId`]; menu items are
//! COMMAND-addressed (the contract carries no per-item `StableId`), keyed by
//! the OWNING `MenuDef`'s widget id + `command_id` — for a nested submenu
//! item the owning id is the SUBMENU's own id, so nesting needs no path
//! encoding. Refusals name the owning menu's widget id for menu items,
//! matching the crate's own report convention (menu items report under
//! their menu's id).

// Stage-1 surface: consumed by the unit tests in this module only until the
// native-iced mount wiring arrives (a later host task consuming this
// machine); the allow mirrors `plugin_scene.rs` / `plugin_widgets.rs`
// stage precedent.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::fmt;

use ocs_plugin_api::scene::StableId;
use ocs_plugin_api::widgets::{
    ConfirmPolicy, IconActionDef, MenuDef, MenuItemDef, WidgetError, WidgetEvent, MAX_MENU_DEPTH,
};

// ---------------------------------------------------------------------------
// Addresses
// ---------------------------------------------------------------------------

/// A dispatchable command address: the small enum every activation names.
///
/// [`DispatchKey::Button`] is the icon button's widget id;
/// [`DispatchKey::MenuItem`] addresses one command inside its OWNING menu (
/// the submenu's own id for nested items). Derives the key set so it gates a
/// `HashMap`/`HashSet` directly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum DispatchKey {
    /// The icon button with this widget id.
    Button(StableId),
    /// The menu item `command_id` inside the menu with this widget id (the
    /// submenu's own id for nested items).
    MenuItem(StableId, String),
}

/// The StableId carried by a [`DispatchKey`] (the address's owning widget:
/// the button, or the item's owning menu — the crate's per-item-report
/// convention).
fn address_of(key: &DispatchKey) -> StableId {
    match key {
        DispatchKey::Button(id) => *id,
        DispatchKey::MenuItem(menu_id, _) => *menu_id,
    }
}

/// The item's `command_id` for a menu-item address (`None` for a button —
/// the button's command belongs to the button def itself).
fn command_of(key: &DispatchKey) -> Option<String> {
    match key {
        DispatchKey::Button(_) => None,
        DispatchKey::MenuItem(_, command) => Some(command.clone()),
    }
}

/// Confirmation-lifecycle address — the per-address pending-prompt map key.
/// The same address vocabulary as [`DispatchKey`]; the alias keeps the two
/// maps of [`WidgetDispatcher`] readable as the two halves of the lifecycle
/// (armed interactions vs. the resolved read model).
type ConfirmationKey = DispatchKey;

// ---------------------------------------------------------------------------
// Read model (captured from sync'd defs — the associated dispatch() input)
// ---------------------------------------------------------------------------

/// The dispatcher's read view of one dispatchable command, captured from the
/// synced def data at [`WidgetDispatcher::sync_button`] /
/// [`WidgetDispatcher::sync_menu`] time.
///
/// `enabled` / `visible` are the EFFECTIVE flags (the fold of widget-level
/// AND item-level flags down the submenu chain — see the module docs); an
/// address with either flag `false` can never emit a
/// command. `label` is the pre-derived a11y label (pure derivation below);
/// `confirm` carries the policy's prompt message when the def declared one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandView {
    /// The semantic command id this address invokes.
    command: String,
    /// Effective enablement (widget-level AND item-level, folded).
    enabled: bool,
    /// Effective visibility (widget-level AND item-level, folded).
    visible: bool,
    /// Pre-derived a11y label (buttons: the required `accessible_name`
    /// verbatim; menu items: label composed with the shortcut hint).
    label: String,
    /// Prompt message when the def declared `ConfirmPolicy::Prompt`.
    confirm: Option<String>,
}

/// The policy's prompt message (`None` with no prompt policy attached).
///
/// Match is EXHAUSTIVE over [`ConfirmPolicy`] (one variant today): a policy
/// variant later added by the crate fails this compile until the machine
/// states what it does with it — deliberate contract-drift guidance.
fn prompt_message(policy: Option<&ConfirmPolicy>) -> Option<String> {
    match policy {
        Some(ConfirmPolicy::Prompt { message }) => Some(message.clone()),
        None => None,
    }
}

/// Pure a11y derivation for one icon button: the REQUIRED `accessible_name`,
/// verbatim (the contract validates it non-empty; that is why the button's
/// a11y text cannot be defaulted away).
fn accessible_label_button(def: &IconActionDef) -> String {
    def.accessible_name.clone()
}

/// Pure a11y derivation for one menu item: the visible label with the
/// declared shortcut hint — `"{label} ({shortcut})"` when a shortcut is
/// declared, the label verbatim otherwise. Presentation may re-style the
/// hint; this is the semantic text the machine guarantees.
fn accessible_label_menu_item(item: &MenuItemDef) -> String {
    match &item.shortcut {
        Some(shortcut) => format!("{} ({})", item.label, shortcut),
        None => item.label.clone(),
    }
}

// ---------------------------------------------------------------------------
// Confirmation lifecycle
// ---------------------------------------------------------------------------

/// One pending confirmation: the policy's prompt text plus the interaction
/// the activation ARMED (command + targets captured at dispatch; a
/// concurrent re-sync cannot swap a different command into an interaction
/// the user is currently confirming; the arming `dispatch` already
/// guaranteed the targets non-empty + within cap).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptStage {
    /// The prompt text the mount shows.
    prompt: String,
    /// The command captured when the interaction was armed.
    command: String,
    /// The targets captured when the interaction was armed.
    targets: Vec<StableId>,
}

// ---------------------------------------------------------------------------
// Outcomes + refusals
// ---------------------------------------------------------------------------

/// A resolved dispatch — exactly one of these per interaction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DispatchOutcome {
    /// [`WidgetEvent::CommandInvoked`] emitted (no confirmation policy
    /// attached): exactly one event; the address is in flight until it
    /// settles.
    Emitted(WidgetEvent),
    /// Prompt policy attached, first activation: the policy's prompt is
    /// REQUESTED — no event yet; the mount shows the dialog and later calls
    /// `confirm` (user OK) or `dismiss_prompt` (user cancel).
    PromptRequested {
        /// The prompt text to show.
        prompt: String,
    },
    /// The user CONFIRMED a previously-prompted activation: the event is
    /// emitted now (exactly once; the address stays in flight until the
    /// mount settles the forwarded event).
    ConfirmedEmitted(WidgetEvent),
}

/// Why a dispatch attempt was refused. A refusal is atomic: ALL state stays
/// as-was and only the refusal's OWN counter moves where the reason has one
/// (see [`DispatchCounters`]) — never half-applied.
///
/// NON-WIRE host-local enum (like the crate's `WidgetError`): hosts and
/// plugins agree on this vocabulary by rustc type identity, not bytes. The
/// first five variants are pinned in DECLARATION ORDER from creation
/// (tail-append only, pin test below); future reasons append freely.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DispatchRefusal {
    /// Effective enablement resolved `false` — the §16 GUI-half rule.
    /// `address` is the button id, or the item's OWNING menu id; `command`
    /// names the entry when the address was a menu item.
    Disabled {
        /// The owning widget id.
        address: StableId,
        /// The `command_id`, when the address was a menu item.
        command: Option<String>,
    },
    /// Effective visibility resolved `false` — an invisible control cannot
    /// be clicked; a conforming mount never renders it, and the machine
    /// refuses on principle (the MCP half must not dispatch it either).
    Hidden {
        /// The owning widget id.
        address: StableId,
        /// The `command_id`, when the address was a menu item.
        command: Option<String>,
    },
    /// The address is already in flight — dispatched and unsettled (a
    /// double click), or a pending prompt IS the armed interaction.
    AlreadyInFlight {
        /// The owning widget id.
        address: StableId,
    },
    /// The address is not in the synced read model (unknown or pruned).
    UnknownAddress {
        /// The owning widget id the caller named.
        address: StableId,
    },
    /// The built event failed the crate's own [`WidgetEvent::validate`] —
    /// over-cap targets, empty command id, … — carried verbatim (ONE
    /// validity path for GUI and MCP, §15). Refused BEFORE anything is
    /// armed or marked in flight.
    InvalidEvent {
        /// The owning widget id.
        address: StableId,
        /// The crate's own validation reason.
        reason: WidgetError,
    },
    // -- appended below the pinned five (non-wire; free to grow) -----------

    /// An activation with NO explicit targets (the SWG-002 naming — a
    /// semantic command names what it acts ON, ARCHITECTURE §13; mirrors
    /// that module's `WidgetStateError::NoTargets`).
    NoTargets {
        /// The owning widget id.
        address: StableId,
    },
    /// `confirm` was attempted with no stage pending — already-settled,
    /// already-absorbed, or never-armed address (raised INSTEAD of folding
    /// into UnknownAddress so callers can distinguish "unknown" from
    /// "usable but not confirming").
    ConfirmWithoutPrompt {
        /// The address that had nothing pending to confirm.
        address: StableId,
    },
}

impl fmt::Display for DispatchRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DispatchRefusal::Disabled { address, command } => match command {
                Some(command) => write!(
                    f,
                    "menu item {command:?} of widget {} is disabled and cannot be invoked",
                    address.0
                ),
                None => write!(f, "widget {} is disabled and cannot be invoked", address.0),
            },
            DispatchRefusal::Hidden { address, command } => match command {
                Some(command) => write!(
                    f,
                    "menu item {command:?} of widget {} is hidden and cannot be invoked",
                    address.0
                ),
                None => write!(f, "widget {} is hidden and cannot be invoked", address.0),
            },
            DispatchRefusal::AlreadyInFlight { address } => write!(
                f,
                "widget {} is already dispatched and awaiting settlement",
                address.0
            ),
            DispatchRefusal::UnknownAddress { address } => {
                write!(f, "widget {} is not in the synced dispatch read model", address.0)
            }
            DispatchRefusal::InvalidEvent { address, reason } => write!(
                f,
                "the dispatch on widget {} produced an invalid event: {reason}",
                address.0
            ),
            DispatchRefusal::NoTargets { address } => {
                write!(f, "the dispatch on widget {} carries no explicit targets", address.0)
            }
            DispatchRefusal::ConfirmWithoutPrompt { address } => {
                write!(f, "widget {} has no prompt pending to confirm", address.0)
            }
        }
    }
}

impl std::error::Error for DispatchRefusal {}

// ---------------------------------------------------------------------------
// Dispatch counters (the §16 defect probes)
// ---------------------------------------------------------------------------

/// The dispatch-machine counters — the ARCHITECTURE §16 proof instruments
/// (the SWG-002 widget-state counters pattern, moved to the command
/// decision).
///
/// Probe discipline: a refusal moves ONLY its own counter, where the reason
/// has one, and NOTHING else — [`DispatchRefusal::Disabled`] moves
/// [`DispatchCounters::activations_refused_disabled`], and
/// [`DispatchRefusal::AlreadyInFlight`] moves
/// [`DispatchCounters::activations_refused_in_flight`]. Refusals WITHOUT a
/// counter of their own (hidden / unknown / no-targets / invalid-event /
/// confirm-without-prompt) move nothing at all: those are plugin-authoring
/// or mount defects routed through the Err path, not user-gesture
/// statistics — the two `refused_*` counters cover precisely the refusals
/// the machine expects during NORMAL GUI flow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DispatchCounters {
    /// Activations RESOLVED to an outcome by [`WidgetDispatcher::dispatch`]
    /// (emitted OR prompt-requested — a resolution is not necessarily an
    /// event). The confirm path does not touch this (it is its own stage:
    /// [`DispatchCounters::confirmations_completed`]).
    pub activations_resolved: u64,
    /// Activations refused because the address was effectively disabled.
    pub activations_refused_disabled: u64,
    /// Extra activations refused while an interaction stayed armed (a
    /// double click, or a re-click while the confirmation dialog is up —
    /// each suppression is a duplicate command that did NOT happen).
    pub activations_refused_in_flight: u64,
    /// Confirmation prompts REQUESTED (the mount shows exactly this many
    /// dialogs); a cancelled dialog does not decrement it.
    pub confirmations_requested: u64,
    /// Confirmation prompts the user CONFIRMED (the exactly-once emission
    /// into the command stream; cancellations are absent by construction).
    pub confirmations_completed: u64,
    /// [`WidgetEvent::CommandInvoked`] events actually emitted — the
    /// "exactly one per interaction" total over BOTH the no-policy and the
    /// confirmed path.
    pub commands_emitted: u64,
    /// A11y labels derived through the machine
    /// ([`WidgetDispatcher::accessible_label`]).
    pub a11y_labels_derived: u64,
}

// ---------------------------------------------------------------------------
// Mount-facing projection
// ---------------------------------------------------------------------------

/// The mount-facing projection of one icon button / menu — what the future
/// native-iced MOUNT renders and routes. Pure data; message types and widget
/// construction belong to the mount. Layout containers (`WidgetNode::Group`)
/// are the mount's own layout capability (ADR-005) and never appear here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ControlSemantics {
    /// One icon button (an [`IconActionDef`]'s fully resolved semantics).
    Icon {
        /// The widget id.
        id: StableId,
        /// The a11y label (`accessible_name`, verbatim — REQUIRED).
        label: String,
        /// The tooltip, when declared.
        tooltip: Option<String>,
        /// Effective enablement (copied).
        enabled: bool,
        /// Effective visibility (copied).
        visible: bool,
        /// Pressed/latched state (copied; host-rendered).
        active: bool,
        /// The semantic command the click invokes.
        command: String,
        /// The prompt text when a `Prompt` confirmation policy is attached.
        confirm: Option<String>,
    },
    /// One menu / context menu (a [`MenuDef`]'s fully resolved semantics).
    Menu {
        /// The widget id.
        id: StableId,
        /// Item semantics in payload order (`MAX_WIDGET_ROWS_INITIAL`-bounded
        /// by the contract; each item independently rendered).
        items: Vec<ItemSemantics>,
    },
}

/// One projected menu item ([`MenuItemDef`]'s fully resolved semantics).
///
/// `enabled` / `visible` are the EFFECTIVE flags: the owning menu's
/// widget-level flags fold in on top of the item's own (a disabled/hidden
/// ancestor disables/hides every descendant). The Menu-level flags
/// themselves need no separate field in the projection — they are exactly
/// the fold an item inherits with no other factors.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ItemSemantics {
    /// Semantic command id (KEY of the item address, with the OWNING menu's
    /// widget id).
    pub command: String,
    /// The a11y label (label composed with the shortcut hint).
    pub label: String,
    /// Display shortcut hint (the host never reads keys through it).
    pub shortcut: Option<String>,
    /// Effective enablement (folded).
    pub enabled: bool,
    /// Effective visibility (folded).
    pub visible: bool,
    /// The prompt text when a `Prompt` confirmation policy is attached.
    pub confirm: Option<String>,
    /// Nested submenu (projected with its own fold), when declared.
    pub submenu: Option<Box<ControlSemantics>>,
}

/// Project one icon button's total semantics (pure; every field, no facade).
pub(crate) fn project_button(def: &IconActionDef) -> ControlSemantics {
    ControlSemantics::Icon {
        id: def.id,
        label: accessible_label_button(def),
        tooltip: def.tooltip.clone(),
        enabled: def.enabled,
        visible: def.visible,
        active: def.active,
        command: def.command.clone(),
        confirm: prompt_message(def.confirmation.as_ref()),
    }
}

/// Project one menu's total semantics (pure; recursion mirrors the def's
/// own nesting — payloads arrive validated, nesting bounded by the contract).
pub(crate) fn project_menu(def: &MenuDef) -> ControlSemantics {
    project_menu_tree(def, (true, true))
}

/// Recursive project of one menu level; `inherited` is the folded
/// widget-level `(enabled, visible)` from the ancestor chain.
fn project_menu_tree(def: &MenuDef, inherited: (bool, bool)) -> ControlSemantics {
    let level = (inherited.0 && def.enabled, inherited.1 && def.visible);
    ControlSemantics::Menu {
        id: def.id,
        items: def
            .items
            .iter()
            .map(|item| ItemSemantics {
                command: item.command_id.clone(),
                label: accessible_label_menu_item(item),
                shortcut: item.shortcut.clone(),
                enabled: level.0 && item.enabled,
                visible: level.1 && item.visible,
                confirm: prompt_message(item.confirmation.as_ref()),
                submenu: item
                    .submenu
                    .as_deref()
                    .map(|sub| Box::new(project_menu_tree(sub, level))),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// WidgetDispatcher
// ---------------------------------------------------------------------------

/// Per-surface semantic command dispatch state (ARCHITECTURE §3 state class
/// 3, host-local; never serialized): the synced command read model, the
/// pending confirmations, and the exactly-once in-flight guard.
///
/// One instance per mounted widget surface. The mount refreshes the READ
/// MODEL via the `sync_*` methods as patches arrive (a re-sync replaces
/// that def's registrations); LIFECYCLE state (armed prompts / in-flight
/// marks) settles ONLY through `confirm` / `dismiss_prompt` / `settle` —
/// data churn never conjecturally rewires an armed interaction. The whole
/// instance is cheap to replace wholesale on surface teardown
/// ([`WidgetDispatcher::reset_all`]). Every method is headless and
/// deterministic.
#[derive(Debug, Clone)]
pub(crate) struct WidgetDispatcher {
    /// The surface this dispatcher commands (rides every emitted event).
    surface_id: StableId,
    /// The read model captured from sync'd defs (the associated
    /// `dispatch()` input).
    enabled_state: HashMap<DispatchKey, CommandView>,
    /// Confirmation lifecycle: per (widget / menu-item address) -> the
    /// pending prompt stage (the armed interaction).
    confirmations: HashMap<ConfirmationKey, PromptStage>,
    /// Exactly-once guard: commands already dispatched and awaiting
    /// settlement (CommandInvoked emitted) — a SECOND activation of the
    /// same address while in flight is refused (the "exactly one semantic
    /// command per interaction" discipline; a double-click must not
    /// double-invoke). A pending prompt arms the SAME set: the prompt is
    /// the armed interaction.
    in_flight: HashSet<DispatchKey>,
    /// The §16 counters.
    counters: DispatchCounters,
}

impl WidgetDispatcher {
    /// New, empty dispatcher for one surface. Nothing synced, nothing armed.
    pub(crate) fn new(surface_id: StableId) -> Self {
        WidgetDispatcher {
            surface_id,
            enabled_state: HashMap::new(),
            confirmations: HashMap::new(),
            in_flight: HashSet::new(),
            counters: DispatchCounters::default(),
        }
    }

    /// The surface this dispatcher belongs to.
    pub(crate) fn surface_id(&self) -> StableId {
        self.surface_id
    }

    /// The §16 dispatch counters (read side).
    pub(crate) fn counters(&self) -> &DispatchCounters {
        &self.counters
    }

    /// The pending prompt's text for `key`, when one is armed (the mount's
    /// dialog input while the interaction waits on the user).
    pub(crate) fn pending_prompt(&self, key: &DispatchKey) -> Option<&str> {
        self.confirmations.get(key).map(|stage| stage.prompt.as_str())
    }

    /// Explicit teardown (tab/plugin close): the read model, every pending
    /// prompt and every in-flight mark drop — §16 "closing a tab/plugin
    /// cancels pending data requests and releases state". Deliberately NOT
    /// counter-instrumented: a teardown is the mount's explicit surface
    /// operation, not an interaction.
    pub(crate) fn reset_all(&mut self) {
        self.enabled_state = HashMap::new();
        self.confirmations = HashMap::new();
        self.in_flight = HashSet::new();
    }

    // -- read-model sync ----------------------------------------------------

    /// Capture (re-capture) one icon button's command view from `def`.
    ///
    /// A re-sync with another command/confirmation re-registers THAT
    /// address; armed lifecycle state is untouched by data churn (see the
    /// type docs).
    pub(crate) fn sync_button(&mut self, def: &IconActionDef) {
        let view = CommandView {
            command: def.command.clone(),
            enabled: def.enabled,
            visible: def.visible,
            label: accessible_label_button(def),
            confirm: prompt_message(def.confirmation.as_ref()),
        };
        self.enabled_state.insert(DispatchKey::Button(def.id), view);
    }

    /// Capture (re-capture) one menu's command views from `def`, recursively
    /// through submenus down to [`MAX_MENU_DEPTH`] menu levels (the
    /// defensive mirror of the crate's bounded decode — the surface
    /// validation already rejects deeper chains; this cap is a guard's
    /// guard, and a deeper chain's items simply never register — i.e.
    /// dispatch refuses them as unknown, never mis-executes).
    ///
    /// Widget-level flags fold multiplicatively down the chain (see the
    /// type docs on [`ItemSemantics`]).
    pub(crate) fn sync_menu(&mut self, def: &MenuDef) {
        self.sync_menu_tree(def, (true, true), 1);
    }

    /// Recursive capture for one menu level; `inherited` is the folded
    /// widget-level `(enabled, visible)` from the ancestor chain and
    /// `depth` is the 1-based menu level (the top menu is `1`).
    fn sync_menu_tree(&mut self, def: &MenuDef, inherited: (bool, bool), depth: usize) {
        let level = (inherited.0 && def.enabled, inherited.1 && def.visible);
        for item in &def.items {
            let view = CommandView {
                command: item.command_id.clone(),
                enabled: level.0 && item.enabled,
                visible: level.1 && item.visible,
                label: accessible_label_menu_item(item),
                confirm: prompt_message(item.confirmation.as_ref()),
            };
            self.enabled_state
                .insert(DispatchKey::MenuItem(def.id, item.command_id.clone()), view);
        }
        // Descend ONLY while the child menu would sit at an allowed level
        // (top menu level 1; deepest menu level [`MAX_MENU_DEPTH`] — the
        // same gate the crate's decode enforces).
        if depth < MAX_MENU_DEPTH {
            for item in &def.items {
                if let Some(sub) = &item.submenu {
                    self.sync_menu_tree(sub, level, depth + 1);
                }
            }
        }
    }

    // -- enablement/visibility reads -----------------------------------------

    /// The resolved effective enablement of a synced address (`None` when
    /// unknown) — the read the MCP parity half (SWG-005) uses to satisfy
    /// "reading command enabled/visible" (ARCHITECTURE §15) from the same
    /// model.
    pub(crate) fn effective_enabled(&self, key: &DispatchKey) -> Option<bool> {
        self.enabled_state.get(key).map(|view| view.enabled)
    }

    /// The resolved effective visibility of a synced address (`None` when
    /// unknown).
    pub(crate) fn effective_visible(&self, key: &DispatchKey) -> Option<bool> {
        self.enabled_state.get(key).map(|view| view.visible)
    }

    // -- dispatch ------------------------------------------------------------

    /// Resolve one activation attempt of `key` against the synced read
    /// model. Refusal order is fixed, and any refusal leaves ALL state and
    /// counters as-was except its own counter where it has one:
    ///
    /// 1. unknown address → [`DispatchRefusal::UnknownAddress`];
    /// 2. armed / unsettled address → [`DispatchRefusal::AlreadyInFlight`]
    ///    (which is also why re-clicks while a confirmation dialog is up
    ///    refuse as in-flight — the prompt IS the armed interaction);
    /// 3. effectively invisible | disabled → `Hidden` | `Disabled` (neither
    ///    address ever emits a command; the resolution is checked BEFORE
    ///    any event exists — §16 GUI-half);
    /// 4. no explicit targets → [`DispatchRefusal::NoTargets`];
    /// 5. build the event and bind through [`WidgetEvent::validate`] →
    ///    [`DispatchRefusal::InvalidEvent`] on failure (over-cap targets, an
    ///    empty command id, …) — with NOTHING armed, so the address stays
    ///    live for the next, valid attempt;
    /// 6. no policy on the view → [`DispatchOutcome::Emitted`]; a
    ///    `Prompt` policy → [`DispatchOutcome::PromptRequested`] (the
    ///    exactly-once guard arms AS the dialog; re-activations take the
    ///    step-2 refusal until `confirm` / `dismiss_prompt` clears the
    ///    armed mark).
    pub(crate) fn dispatch(
        &mut self,
        key: DispatchKey,
        targets: Vec<StableId>,
    ) -> Result<DispatchOutcome, DispatchRefusal> {
        let view = self
            .enabled_state
            .get(&key)
            .cloned()
            .ok_or(DispatchRefusal::UnknownAddress {
                address: address_of(&key),
            })?;

        // Armed-interaction guard FIRST: once an address is armed, it stays
        // refused until settled, with the same verdict whether the sync
        // meanwhilst moved the flags.
        if self.in_flight.contains(&key) {
            self.counters.activations_refused_in_flight += 1;
            return Err(DispatchRefusal::AlreadyInFlight {
                address: address_of(&key),
            });
        }

        if !view.visible {
            return Err(DispatchRefusal::Hidden {
                address: address_of(&key),
                command: command_of(&key),
            });
        }
        if !view.enabled {
            self.counters.activations_refused_disabled += 1;
            return Err(DispatchRefusal::Disabled {
                address: address_of(&key),
                command: command_of(&key),
            });
        }

        if targets.is_empty() {
            return Err(DispatchRefusal::NoTargets {
                address: address_of(&key),
            });
        }

        // Capture the ARMED interaction's facts before the event consumes
        // the targets; then build + validate (one validity path).
        let CommandView { command, confirm, .. } = view;
        let armed_targets = targets.clone();
        let event = WidgetEvent::CommandInvoked {
            surface_id: self.surface_id,
            command_id: command.clone(),
            target_ids: targets,
        };
        event
            .validate()
            .map_err(|reason| DispatchRefusal::InvalidEvent {
                address: address_of(&key),
                reason,
            })?;

        // Resolved: arm the exactly-once guard, THEN branch on the policy.
        self.in_flight.insert(key.clone());
        match confirm {
            None => {
                self.counters.activations_resolved += 1;
                self.counters.commands_emitted += 1;
                Ok(DispatchOutcome::Emitted(event))
            }
            Some(prompt) => {
                self.counters.activations_resolved += 1;
                self.counters.confirmations_requested += 1;
                self.confirmations.insert(
                    key,
                    PromptStage {
                        prompt: prompt.clone(),
                        command,
                        targets: armed_targets,
                    },
                );
                Ok(DispatchOutcome::PromptRequested { prompt })
            }
        }
    }

    /// The user CONFIRMED a pending dialog: pop the stage and emit the
    /// command NOW — exactly once. The armed guard remains until the mount
    /// settles the forwarded event; a confirm attempt with no pending stage
    /// is refused distinctly ([`DispatchRefusal::ConfirmWithoutPrompt`]).
    ///
    /// The event re-validates through the crate ([`WidgetEvent::validate`])
    /// against the CAPTURED command/targets — a failure there CONSUMES the
    /// armed stage and refuses (the interaction is dead; a re-click
    /// re-prompts) — but a captured valid stage by construction carries
    /// exactly what the arming dispatch validated, so this is defense in
    /// depth on the same one validity path.
    pub(crate) fn confirm(&mut self, key: DispatchKey) -> Result<DispatchOutcome, DispatchRefusal> {
        let stage = self.confirmations.remove(&key).ok_or(
            DispatchRefusal::ConfirmWithoutPrompt { address: address_of(&key) },
        )?;
        let event = WidgetEvent::CommandInvoked {
            surface_id: self.surface_id,
            command_id: stage.command,
            target_ids: stage.targets,
        };
        event
            .validate()
            .map_err(|reason| DispatchRefusal::InvalidEvent {
                address: address_of(&key),
                reason,
            })?;
        self.counters.confirmations_completed += 1;
        self.counters.commands_emitted += 1;
        Ok(DispatchOutcome::ConfirmedEmitted(event))
    }

    /// The user CANCELLED the pending dialog: clear the pending stage AND
    /// the armed mark (no event; nothing was ever emitted along this path,
    /// so no settling is needed afterwards). A later
    /// re-click re-prompts (the policy is per-def, not per-machine).
    ///
    /// Returns `false` when nothing was pending.
    pub(crate) fn dismiss_prompt(&mut self, key: DispatchKey) -> bool {
        if self.confirmations.remove(&key).is_some() {
            self.in_flight.remove(&key);
            true
        } else {
            false
        }
    }

    /// Clear the exactly-once mark for `key` — the call the mount makes
    /// after forwarding an EMITTED event, letting a genuine next
    /// interaction dispatch again (exactly once PER interaction).
    ///
    /// A pending confirmation is NOT settled by this (its dialog owns the
    /// armed interaction — only `confirm` / `dismiss_prompt` take it
    /// down); the return value (`false`) reports that no mark was cleared.
    pub(crate) fn settle(&mut self, key: DispatchKey) -> bool {
        if self.confirmations.contains_key(&key) {
            return false;
        }
        self.in_flight.remove(&key)
    }

    // -- a11y ----------------------------------------------------------------

    /// The derived a11y label for a synced address — buttons: the required
    /// `accessible_name` verbatim; menu items: label composed with the
    /// shortcut hint — COUNTED in [`DispatchCounters::a11y_labels_derived`].
    /// The derivation itself is the two pure functions above; the counter
    /// tracks the machine-path derivations the mount uses to build
    /// screen-reader text. Unknown addresses refuse
    /// ([`DispatchRefusal::UnknownAddress`]); a label derivation is a read,
    /// so no other state moves.
    pub(crate) fn accessible_label(&mut self, key: DispatchKey) -> Result<String, DispatchRefusal> {
        let label = self
            .enabled_state
            .get(&key)
            .map(|view| view.label.clone())
            .ok_or(DispatchRefusal::UnknownAddress {
                address: address_of(&key),
            })?;
        self.counters.a11y_labels_derived += 1;
        Ok(label)
    }
}

// ---------------------------------------------------------------------------
// Tests (mirror the other widget packages: headless, deterministic, fast)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::widgets::{IconRef, MAX_WIDGET_TARGET_IDS};

    // -- construction helpers ------------------------------------------------

    fn sid(n: u64) -> StableId {
        StableId(n)
    }

    fn icon(uri: &str) -> IconRef {
        IconRef {
            uri: uri.to_string(),
            size: 16.0,
        }
    }

    fn button_def(id: u64, name: &str, command: &str) -> IconActionDef {
        IconActionDef {
            id: sid(id),
            accessible_name: name.to_string(),
            tooltip: None,
            icon: icon("host://icons/test.svg"),
            enabled: true,
            visible: true,
            active: false,
            command: command.to_string(),
            confirmation: None,
        }
    }

    fn menu_item_def(command: &str, label: &str) -> MenuItemDef {
        MenuItemDef {
            command_id: command.to_string(),
            label: label.to_string(),
            icon: None,
            shortcut: None,
            enabled: true,
            visible: true,
            submenu: None,
            confirmation: None,
        }
    }

    fn menu_def(id: u64, items: Vec<MenuItemDef>) -> MenuDef {
        MenuDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            items,
        }
    }

    /// The `CommandInvoked` inside an `Emitted`/`ConfirmedEmitted` outcome.
    fn emitted_event(outcome: DispatchOutcome) -> WidgetEvent {
        match outcome {
            DispatchOutcome::Emitted(event) => event,
            DispatchOutcome::ConfirmedEmitted(event) => event,
            DispatchOutcome::PromptRequested { .. } => panic!("expected an emitted event"),
        }
    }

    fn assert_events_valid(events: &[WidgetEvent]) {
        for event in events {
            event
                .validate()
                .expect("every emitted event must pass the crate's own validate()");
        }
    }

    // -- 1. disabled half (the §16 GUI-side half) ----------------------------

    /// §16 "disabled commands un-invocable": a disabled button NEVER emits;
    /// the §16 lens-proof that the projection's `enabled == false` and the
    /// dispatcher's refusal are the two halves of ONE discipline; the
    /// refusal counter is the only one that moves.
    #[test]
    fn disabled_button_never_emits() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        let def = IconActionDef {
            enabled: false,
            ..button_def(1, "Delete", "delete")
        };
        dispatcher.sync_button(&def);

        // HALF 1 — the projection (what the mount would render).
        let semantics = project_button(&def);
        match &semantics {
            ControlSemantics::Icon { enabled, .. } => {
                assert_eq!(*enabled, false, "the projection's enablement half");
            }
            ControlSemantics::Menu { .. } => panic!("a button projects Icon"),
        }

        // HALF 2 — the dispatcher refuses: Err + exactly one counter.
        let before = *dispatcher.counters();
        assert_eq!(
            dispatcher.dispatch(DispatchKey::Button(sid(1)), vec![sid(7)]),
            Err(DispatchRefusal::Disabled {
                address: sid(1),
                command: None,
            }),
            "a disabled button can never emit a command"
        );
        assert_eq!(
            dispatcher.counters().activations_refused_disabled,
            before.activations_refused_disabled + 1,
            "the refusal counts in ITS OWN counter only"
        );
        assert_eq!(dispatcher.counters().commands_emitted, 0, "no event exists");
        assert_eq!(
            dispatcher.counters().activations_resolved,
            before.activations_resolved,
            "a refusal is not a resolution"
        );
    }

    // -- 2. hidden half ------------------------------------------------------

    /// An invisible menu item cannot be activated either: refusal (no
    /// event), and a hidden refusal moves NO counter of its own (defect-path
    /// refusal; see the counter docs) — a re-try costs the same nothing.
    #[test]
    fn hidden_item_never_emits() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        let def = menu_def(
            200,
            vec![MenuItemDef {
                visible: false,
                ..menu_item_def("unpublish", "Unpublish")
            }],
        );
        dispatcher.sync_menu(&def);
        let before = *dispatcher.counters();
        assert_eq!(
            dispatcher.dispatch(DispatchKey::MenuItem(sid(200), "unpublish".into()), vec![sid(7)]),
            Err(DispatchRefusal::Hidden {
                address: sid(200),
                command: Some("unpublish".into()),
            })
        );
        assert_eq!(*dispatcher.counters(), before, "NO counter moves on a hidden refusal");

        // And nothing got armed by the refused activation: the next
        // attempt re-refuses with the SAME reason (no wedging, no event).
        assert_eq!(
            dispatcher.dispatch(DispatchKey::MenuItem(sid(200), "unpublish".into()), vec![sid(7)]),
            Err(DispatchRefusal::Hidden {
                address: sid(200),
                command: Some("unpublish".into()),
            })
        );
        assert_eq!(dispatcher.counters().commands_emitted, 0);
    }

    // -- 3. a plain click ------------------------------------------------------

    /// A click without a confirmation policy emits EXACTLY ONE command
    /// event: command id + targets exact, valid by construction.
    #[test]
    fn click_without_policy_emits_exactly_one_command() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        dispatcher.sync_button(&button_def(1, "Zoom to fit", "zoom_fit"));

        let outcome = dispatcher
            .dispatch(DispatchKey::Button(sid(1)), vec![sid(7), sid(9)])
            .expect("enabled, visible, no in-flight");
        assert_eq!(
            outcome.clone(),
            DispatchOutcome::Emitted(WidgetEvent::CommandInvoked {
                surface_id: sid(900),
                command_id: "zoom_fit".to_string(),
                target_ids: vec![sid(7), sid(9)],
            }),
            "command id + targets exact"
        );
        assert_events_valid(&[emitted_event(outcome)]);

        assert_eq!(dispatcher.counters().activations_resolved, 1);
        assert_eq!(dispatcher.counters().commands_emitted, 1);
        assert_eq!(dispatcher.counters().activations_refused_disabled, 0);
    }

    // -- 4. exactly-once (the double click) -------------------------------------

    /// §16 "one activation = exactly one semantic command": the double click
    /// refuses in-flight (no extra event), `settle` releases the guard, and
    /// the genuine second interaction emits again — exactly once per
    /// interaction, not ever (two events after two interactions).
    #[test]
    fn double_click_is_refused_in_flight_until_settle() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        dispatcher.sync_button(&button_def(1, "Delete", "delete"));

        // First click: one event.
        let first = dispatcher
            .dispatch(DispatchKey::Button(sid(1)), vec![sid(7)])
            .expect("first click ok");
        let first_event = match first {
            DispatchOutcome::Emitted(event) => event,
            other => panic!("expected Emitted, got {other:?}"),
        };
        assert_events_valid(&[first_event.clone()]);

        // The double click: refused, no extra event.
        assert_eq!(
            dispatcher.dispatch(DispatchKey::Button(sid(1)), vec![sid(7)]),
            Err(DispatchRefusal::AlreadyInFlight { address: sid(1) })
        );
        assert_eq!(dispatcher.counters().activations_refused_in_flight, 1);
        assert_eq!(dispatcher.counters().commands_emitted, 1, "no duplicate event");

        // The mount forwards the first event, then settles.
        assert!(dispatcher.settle(DispatchKey::Button(sid(1))), "settle clears the mark");

        // The genuine second click emits again.
        let second = dispatcher
            .dispatch(DispatchKey::Button(sid(1)), vec![sid(7)])
            .expect("the second interaction is allowed once settled");
        let second_event = match second {
            DispatchOutcome::Emitted(event) => event,
            other => panic!("expected Emitted, got {other:?}"),
        };
        assert_events_valid(&[second_event.clone()]);
        assert_eq!(
            first_event, second_event,
            "same command, targets, and surface, once per interaction"
        );
        assert_eq!(dispatcher.counters().commands_emitted, 2, "two interactions, two events");

        // settle with nothing in flight reports `false` (and stays a no-op).
        assert!(!dispatcher.settle(DispatchKey::Button(sid(42))));
        assert_eq!(dispatcher.counters().commands_emitted, 2);
    }

    // -- 5. the confirmation lifecycle -------------------------------------------

    /// The two-step lifecycle: first dispatch → prompt REQUESTED (no event;
    /// the exactly-once guard arms); a re-dispatch while prompting refuses
    /// in-flight; confirm → the event, exactly once; a confirm attempt
    /// without a pending stage refuses distinctly; dismiss → a clean
    /// re-prompt later; a pending prompt denies settle.
    #[test]
    fn prompt_policy_lifecycle() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        let def = IconActionDef {
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Really delete the selection?".to_string(),
            }),
            ..button_def(1, "Delete", "delete")
        };
        dispatcher.sync_button(&def);

        let armed = DispatchKey::Button(sid(1));

        // First activation: prompt requested, NO event.
        let outcome = dispatcher
            .dispatch(armed.clone(), vec![sid(7)])
            .expect("enabled, visible, first attempt");
        assert_eq!(
            outcome.clone(),
            DispatchOutcome::PromptRequested {
                prompt: "Really delete the selection?".to_string(),
            }
        );
        assert_eq!(dispatcher.pending_prompt(&armed), Some("Really delete the selection?"));
        assert_eq!(dispatcher.counters().confirmations_requested, 1);
        assert_eq!(dispatcher.counters().commands_emitted, 0, "no event yet");

        // Re-try while prompting → refused as in-flight (the prompt IS the
        // armed interaction); no second prompt is requested.
        assert_eq!(
            dispatcher.dispatch(armed.clone(), vec![sid(7)]),
            Err(DispatchRefusal::AlreadyInFlight { address: sid(1) })
        );
        assert_eq!(dispatcher.counters().activations_refused_in_flight, 1);
        assert_eq!(dispatcher.counters().confirmations_requested, 1,
            "a refused re-try never re-prompts");

        // The user confirms: the event is emitted NOW, exactly once; the
        // armed guard SURVIVES (until the mount settles the forwarded
        // event); the pending stage is gone.
        let outcome = dispatcher.confirm(armed.clone()).expect("a pending stage exists");
        assert_eq!(
            emitted_event(outcome.clone()),
            WidgetEvent::CommandInvoked {
                surface_id: sid(900),
                command_id: "delete".to_string(),
                target_ids: vec![sid(7)],
            },
            "the captured command + targets exact"
        );
        assert!(
            matches!(outcome, DispatchOutcome::ConfirmedEmitted(_)),
            "the confirmed path returns ConfirmedEmitted"
        );
        assert_eq!(dispatcher.counters().confirmations_completed, 1);
        assert_eq!(dispatcher.counters().commands_emitted, 1, "exactly one event, now");
        assert_eq!(dispatcher.pending_prompt(&armed), None, "the stage is consumed");

        // A confirm attempt with no pending stage refuses distinctly.
        assert_eq!(
            dispatcher.confirm(armed.clone()),
            Err(DispatchRefusal::ConfirmWithoutPrompt { address: sid(1) })
        );

        // The mount settles the forwarded event; the SECOND interaction
        // re-arms (a genuine fresh prompt, not a leftover stage).
        assert!(dispatcher.settle(armed.clone()));
        let rearm = dispatcher
            .dispatch(armed.clone(), vec![sid(8)])
            .expect("a fresh interaction is allowed");
        assert!(matches!(rearm, DispatchOutcome::PromptRequested { .. }));
        assert_eq!(dispatcher.counters().confirmations_requested, 2);

        // The user cancels: clean teardown and no event; the armed mark
        // clears fully.
        assert!(dispatcher.dismiss_prompt(armed.clone()), "the armed stage cleared");
        assert_eq!(dispatcher.pending_prompt(&armed), None);
        assert_eq!(dispatcher.counters().commands_emitted, 1, "still exactly one event");

        // A re-click after the dismissal re-prompts cleanly.
        let again = dispatcher
            .dispatch(armed.clone(), vec![sid(8)])
            .expect("re-click after the cancel is allowed");
        assert!(matches!(again, DispatchOutcome::PromptRequested { .. }));
        assert_eq!(
            dispatcher.counters().confirmations_requested,
            3,
            "the re-click re-prompts (a cancelled dialog did not burn the interaction)"
        );
        assert_eq!(dispatcher.counters().confirmations_completed, 1);

        // A pending dialog DENIES settle (only confirm/dismiss own that
        // guard — otherwise the dialog's re-click protection could be
        // disarmed through settle).
        assert!(!dispatcher.settle(armed.clone()));
        assert_eq!(dispatcher.pending_prompt(&armed), Some("Really delete the selection?"));
        assert_eq!(dispatcher.counters().activations_refused_in_flight, 1,
            "still only ONE suppressed re-try-counted activation");
    }

    // -- 6. menu item addresses + submenu depth ----------------------------------

    /// Menu items are (owning menu id, command_id)-addressed; every item is
    /// independently enable-gated; per-item refusals name the menu id +
    /// command; a submenu chain nesting `MAX_MENU_DEPTH - 1` menu levels
    /// dispatches its deepest item, while a chain past the crate's
    /// [`MAX_MENU_DEPTH`] cap never registers (its items dispatch as
    /// unknown — the guard's guard).
    #[test]
    fn menu_item_addresses_and_submenu_depth() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        // Chain: root menu 200 + (MAX_MENU_DEPTH - 2) submenus = MAX_MENU_DEPTH - 1
        // MenuDefs total. Submenu ids 1001.. ascends; the deepest item's
        // command is "deep".
        let total_levels = MAX_MENU_DEPTH - 1; // 7 MenuDefs incl. the root
        let sub_count = total_levels - 1; // 6 submenus below the root
        let mut chain: MenuDef = menu_def(
            1_000 + sub_count as u64,
            vec![menu_item_def("deep", "Deep command")],
        );
        for depth in (1..sub_count).rev() {
            let mut item = menu_item_def("descend", "Descend");
            item.submenu = Some(Box::new(chain));
            chain = menu_def(1_000 + depth as u64, vec![item]);
        }
        let mut descend = menu_item_def("descend", "Descend");
        descend.submenu = Some(Box::new(chain));
        let def = menu_def(
            200,
            vec![
                menu_item_def("alpha", "Alpha action"),
                MenuItemDef {
                    enabled: false,
                    ..menu_item_def("beta", "Beta action")
                },
                descend,
            ],
        );
        dispatcher.sync_menu(&def);

        // "alpha" activates ONLY under its own address.
        let outcome = dispatcher
            .dispatch(DispatchKey::MenuItem(sid(200), "alpha".into()), vec![sid(7)])
            .expect("alpha is enabled");
        match emitted_event(outcome) {
            WidgetEvent::CommandInvoked { ref command_id, .. } => {
                assert_eq!(command_id, "alpha");
            }
            other => panic!("expected CommandInvoked, got {other:?}"),
        }
        assert!(dispatcher.settle(DispatchKey::MenuItem(sid(200), "alpha".into())));

        // "beta" refuses, with the OWNING menu id + command named.
        assert_eq!(
            dispatcher.dispatch(DispatchKey::MenuItem(sid(200), "beta".into()), vec![sid(7)]),
            Err(DispatchRefusal::Disabled {
                address: sid(200),
                command: Some("beta".into()),
            }),
            "every item is independently enable-gated; refusals name menu id + command"
        );
        assert_eq!(dispatcher.counters().activations_refused_disabled, 1);

        // The DEEPEST item of the allowed-depth chain dispatches (its
        // address = the deepest submenu's own id + its command).
        let deepest_id = 1_000 + sub_count as u64;
        let outcome = dispatcher
            .dispatch(DispatchKey::MenuItem(sid(deepest_id), "deep".into()), vec![sid(9)])
            .expect("the deepest item of an allowed-depth chain is dispatchable");
        match emitted_event(outcome) {
            WidgetEvent::CommandInvoked { ref command_id, .. } => {
                assert_eq!(command_id, "deep");
            }
            other => panic!("expected CommandInvoked, got {other:?}"),
        }

        // The widget-level fold is dispatcher-real too: a disabled
        // ANCESTOR menu disables every item (registrations read through
        // the chain; the refusal names the owning menu id + command).
        let disabled_root = MenuDef {
            enabled: false,
            ..menu_def(400, vec![menu_item_def("reprice", "Reprice all")])
        };
        dispatcher.sync_menu(&disabled_root);
        assert_eq!(
            dispatcher.dispatch(DispatchKey::MenuItem(sid(400), "reprice".into()), vec![sid(7)]),
            Err(DispatchRefusal::Disabled {
                address: sid(400),
                command: Some("reprice".into()),
            }),
            "the widget-level disable folds into every item at dispatch too"
        );
    }

    /// The guard's guard: a submenu chain nesting past the crate's
    /// [`MAX_MENU_DEPTH`] (root + MAX_MENU_DEPTH submenus) never registers
    /// its deepest level's items — dispatch refuses as unknown (the same
    /// gate the surface's bounded decode enforces upstream).
    #[test]
    fn submenu_past_max_menu_depth_never_registers() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        let sub_count = MAX_MENU_DEPTH; // root + MAX_MENU_DEPTH submenus
        let mut chain: MenuDef = menu_def(
            1_000 + sub_count as u64,
            vec![menu_item_def("toodeep", "Too deep")],
        );
        for depth in (1..sub_count).rev() {
            let mut item = menu_item_def("descend", "Descend");
            item.submenu = Some(Box::new(chain));
            chain = menu_def(1_000 + depth as u64, vec![item]);
        }
        let mut descend = menu_item_def("descend", "Descend");
        descend.submenu = Some(Box::new(chain));
        dispatcher.sync_menu(&menu_def(200, vec![descend]));

        // An intermediate level (depth ≤ MAX_MENU_DEPTH) still dispatches.
        let outcome = dispatcher
            .dispatch(DispatchKey::MenuItem(sid(1_001), "descend".into()), vec![sid(7)])
            .expect("menus up to the cap are registered");
        match emitted_event(outcome) {
            WidgetEvent::CommandInvoked { .. } => {}
            other => panic!("expected CommandInvoked, got {other:?}"),
        }

        // The deepest level (depth MAX_MENU_DEPTH + 1) is unknown.
        assert_eq!(
            dispatcher.dispatch(
                DispatchKey::MenuItem(sid(1_000 + MAX_MENU_DEPTH as u64), "toodeep".into()),
                vec![sid(7)],
            ),
            Err(DispatchRefusal::UnknownAddress {
                address: sid(1_000 + MAX_MENU_DEPTH as u64),
            }),
            "items behind the cap never register (and never mis-execute)"
        );
    }

    // -- 7. target bounds --------------------------------------------------------

    /// Targets ride the exact [`MAX_WIDGET_TARGET_IDS`] bound: at-cap events
    /// validate; over-cap refuses carrying the crate's own reason; empty
    /// targets refuse distinctly; and a failed emission NEVER arms the
    /// address.
    #[test]
    fn targets_are_bounded_and_validated() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        dispatcher.sync_button(&button_def(1, "Batch move", "batch_move"));
        let key = DispatchKey::Button(sid(1));

        // Exactly at cap: a VALID event (validated on the built event).
        let at_cap: Vec<StableId> = (0..MAX_WIDGET_TARGET_IDS as u64).map(sid).collect();
        let outcome = dispatcher.dispatch(key.clone(), at_cap.clone()).expect("at-cap targets pass");
        match &emitted_event(outcome) {
            WidgetEvent::CommandInvoked { target_ids, .. } => {
                assert_eq!(target_ids, &at_cap, "targets exact");
            }
            other => panic!("expected CommandInvoked, got {other:?}"),
        }
        assert!(dispatcher.settle(key.clone()));

        // Over-cap: refused through the crate's own validate — NO arming.
        let over: Vec<StableId> = (0..MAX_WIDGET_TARGET_IDS as u64 + 1).map(sid).collect();
        assert_eq!(
            dispatcher.dispatch(key.clone(), over),
            Err(DispatchRefusal::InvalidEvent {
                address: sid(1),
                reason: WidgetError::BudgetExceeded {
                    current: MAX_WIDGET_TARGET_IDS + 1,
                    max: MAX_WIDGET_TARGET_IDS,
                },
            }),
            "the over-cap refusal carries the crate's own validation reason"
        );

        // Empty targets: refuse distinctly (§13 — a command names what it
        // acts ON).
        assert_eq!(
            dispatcher.dispatch(key.clone(), Vec::new()),
            Err(DispatchRefusal::NoTargets { address: sid(1) })
        );

        // The failed emissions never armed the address: a fresh valid
        // attempt dispatches IMMEDIATELY (no wedging).
        let outcome = dispatcher
            .dispatch(key.clone(), vec![sid(1)])
            .expect("a refused emission never wedges the address");
        assert_events_valid(&[emitted_event(outcome)]);
        assert_eq!(dispatcher.counters().commands_emitted, 2);
    }

    // -- 8. a11y labels -----------------------------------------------------------

    /// Button labels are the required `accessible_name` VERBATIM; menu-item
    /// labels compose with the shortcut hint; every derivation through the
    /// machine counts in `a11y_labels_derived`.
    #[test]
    fn a11y_labels_derived_exactly() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        dispatcher.sync_button(&button_def(1, "Zoom to fit", "zoom_fit"));
        let item = MenuItemDef {
            shortcut: Some("Ctrl+D".to_string()),
            ..menu_item_def("delete_item", "Delete")
        };
        dispatcher.sync_menu(&menu_def(200, vec![item]));

        // Button: verbatim (the contract REQUIRES a non-empty name).
        assert_eq!(
            dispatcher
                .accessible_label(DispatchKey::Button(sid(1)))
                .expect("a synced button owns a label"),
            "Zoom to fit",
            "the button's a11y derivation is the verbatim accessible name"
        );

        // Menu item: label + shortcut composition.
        assert_eq!(
            dispatcher
                .accessible_label(DispatchKey::MenuItem(sid(200), "delete_item".into()))
                .expect("a synced item owns a label"),
            "Delete (Ctrl+D)",
            "label + shortcut hint composition"
        );

        let plain = MenuItemDef {
            shortcut: None,
            ..menu_item_def("list", "List")
        };
        dispatcher.sync_menu(&menu_def(201, vec![plain]));
        assert_eq!(
            dispatcher
                .accessible_label(DispatchKey::MenuItem(sid(201), "list".into()))
                .expect("a synced item owns a label"),
            "List",
            "no shortcut hint → the label verbatim"
        );

        // The pure derivations agree with the machine's (they ARE them,
        // reachable without a dispatcher for tests/mounts).
        let item_def = MenuItemDef {
            shortcut: Some("Ctrl+D".to_string()),
            ..menu_item_def("delete_item", "Delete")
        };
        assert_eq!(accessible_label_menu_item(&item_def), "Delete (Ctrl+D)");

        assert_eq!(dispatcher.counters().a11y_labels_derived, 3);
    }

    // -- 9. the projection is total -------------------------------------------------

    /// `project_button` / `project_menu` are TOTAL: every field lands
    /// (a11y applied), disabled/hidden/active/shortcut/confirm flags all
    /// copy verbatim, widget-level flags FOLD into item semantics — and the
    /// §16 lens shows the two halves together: a disabled item projects
    /// `enabled == false` AND its dispatcher activation refuses.
    #[test]
    fn project_button_and_menu_semantics_totality() {
        // Button: every Icon field lands; a11y label applied.
        let def = IconActionDef {
            tooltip: Some("Select all alignment rows".to_string()),
            active: true,
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Confirm wipe".to_string(),
            }),
            ..button_def(1, "Wipe selection", "wipe")
        };
        assert_eq!(
            project_button(&def),
            ControlSemantics::Icon {
                id: sid(1),
                label: "Wipe selection".to_string(),
                tooltip: Some("Select all alignment rows".to_string()),
                enabled: true,
                visible: true,
                active: true,
                command: "wipe".to_string(),
                confirm: Some("Confirm wipe".to_string()),
            },
            "every Icon field lands, with the a11y label applied"
        );

        // Menu: every Item field lands; a submenu nesting preserved.
        let submenu = menu_def(
            301,
            vec![MenuItemDef {
                shortcut: Some("Ctrl+R".to_string()),
                ..menu_item_def("reveal", "Reveal in browser")
            }],
        );
        let item = MenuItemDef {
            shortcut: Some("Ctrl+D".to_string()),
            enabled: false,
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Confirm delete".to_string(),
            }),
            submenu: Some(Box::new(submenu)),
            ..menu_item_def("delete_item", "Delete")
        };
        let def = menu_def(300, vec![item]);
        let projected = project_menu(&def);
        let ControlSemantics::Menu { id, items } = projected else {
            panic!("a menu projects Menu");
        };
        assert_eq!(id, sid(300));
        assert_eq!(items.len(), 1, "items project in payload order");
        let projected_item = &items[0];
        assert_eq!(projected_item.command, "delete_item");
        assert_eq!(projected_item.label, "Delete (Ctrl+D)", "a11y applied");
        assert_eq!(projected_item.shortcut.as_deref(), Some("Ctrl+D"));
        assert_eq!(projected_item.enabled, false, "the item-level disable copies");
        assert_eq!(projected_item.visible, true);
        assert_eq!(projected_item.confirm.as_deref(), Some("Confirm delete"));
        let submenu_semantics = projected_item.submenu.as_deref().expect("a declared submenu projects");
        let ControlSemantics::Menu { id, items: sub_items } = submenu_semantics else {
            panic!("a submenu projects Menu");
        };
        assert_eq!(*id, sid(301));
        assert_eq!(sub_items.len(), 1);
        assert_eq!(sub_items[0].command, "reveal");
        assert_eq!(sub_items[0].label, "Reveal in browser (Ctrl+R)");
        assert_eq!(sub_items[0].shortcut.as_deref(), Some("Ctrl+R"));
        assert_eq!(sub_items[0].enabled, true, "the submenu's OWN enablement reads through the fold");
        assert_eq!(sub_items[0].visible, true);

        // A disabled TOP menu disables every item (the fold: the widget-level
        // flag multiplies in), visible-with-disabled-item keeps the item's
        // own disable.
        let disabled_menu = MenuDef {
            enabled: false,
            ..menu_def(302, vec![menu_item_def("adopt", "Adopt")])
        };
        let projected = project_menu(&disabled_menu);
        if let ControlSemantics::Menu { items, .. } = &projected {
            assert_eq!(items[0].enabled, false, "the widget-level disable FOLDS in");
        } else {
            panic!("a menu projects Menu");
        }

        // §16 lens — the two halves together: the projection said
        // `enabled == false`, and the dispatcher refuses the same def.
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        dispatcher.sync_menu(&def);
        assert_eq!(
            dispatcher.dispatch(DispatchKey::MenuItem(sid(300), "delete_item".into()), vec![sid(7)]),
            Err(DispatchRefusal::Disabled {
                address: sid(300),
                command: Some("delete_item".into()),
            }),
            "both halves agree: the flagged item's dispatch refuses"
        );
    }

    // -- 10. counter probe discipline --------------------------------------------------

    /// Each refusal moves ONLY its own counter where it has one, and never a
    /// resolution/production counters; refusals without a counter move
    /// NOTHING (the SWG-002 probe discipline at the dispatch layer).
    #[test]
    fn counters_never_move_on_refusals_except_their_own_reason() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        let disabled = IconActionDef {
            enabled: false,
            ..button_def(1, "Delete", "delete")
        };
        dispatcher.sync_button(&disabled);
        // An ENABLED button for the target-bound probes.
        dispatcher.sync_button(&button_def(3, "Batch move", "batch_move"));
        let hidden_item = MenuItemDef {
            visible: false,
            ..menu_item_def("vanish", "Vanish")
        };
        dispatcher.sync_menu(&menu_def(2, vec![hidden_item]));

        let initial = *dispatcher.counters();

        // Disabled: the ONLY counter that moves is its own.
        assert!(matches!(
            dispatcher.dispatch(DispatchKey::Button(sid(1)), vec![sid(7)]),
            Err(DispatchRefusal::Disabled { .. })
        ));
        let after = *dispatcher.counters();
        assert_eq!(after.activations_refused_disabled, initial.activations_refused_disabled + 1);
        assert_eq!(after.activations_resolved, initial.activations_resolved);
        assert_eq!(after.commands_emitted, initial.commands_emitted);
        assert_eq!(after.a11y_labels_derived, initial.a11y_labels_derived);

        // From here on the timeline is post-disabled: a fresh probe
        // snapshot, then every remaining refusal moves NOTHING at all.
        let snap = *dispatcher.counters();

        // Hidden: no counter of its own; NOTHING moves.
        assert!(matches!(
            dispatcher.dispatch(DispatchKey::MenuItem(sid(2), "vanish".into()), vec![sid(7)]),
            Err(DispatchRefusal::Hidden { .. })
        ));
        assert_eq!(*dispatcher.counters(), snap);

        // Unknown: no counter moves (never-registered namespace case).
        assert!(matches!(
            dispatcher.dispatch(DispatchKey::Button(sid(77)), vec![sid(7)]),
            Err(DispatchRefusal::UnknownAddress { .. })
        ));
        assert_eq!(*dispatcher.counters(), snap);

        // No targets / invalid event: no listed counter moves (producer-
        // defect paths routed through Err).
        assert!(matches!(
            dispatcher.dispatch(DispatchKey::Button(sid(3)), Vec::new()),
            Err(DispatchRefusal::NoTargets { .. })
        ));
        let over: Vec<StableId> = (0..MAX_WIDGET_TARGET_IDS as u64 + 1).map(sid).collect();
        assert!(matches!(
            dispatcher.dispatch(DispatchKey::Button(sid(3)), over),
            Err(DispatchRefusal::InvalidEvent { .. })
        ));
        assert_eq!(*dispatcher.counters(), snap);

        // Confirm-without-prompt: no counter moves.
        assert!(matches!(
            dispatcher.confirm(DispatchKey::Button(sid(1))),
            Err(DispatchRefusal::ConfirmWithoutPrompt { .. })
        ));
        assert_eq!(*dispatcher.counters(), snap);

        // Dismiss with nothing pending: no counter moves either.
        assert!(!dispatcher.dismiss_prompt(DispatchKey::Button(sid(1))));
        assert_eq!(*dispatcher.counters(), snap);
    }

    // -- 11. unknown addresses + prompt lifecycle isolation -----------------------------

    /// Unknown addresses refuse (no phantom state, no counters); prompt
    /// lifecycles are per-address: arming A does NOT arm B, confirm pops
    /// only its own stage, and confirming a never-synced address refuses
    /// distinctly.
    #[test]
    fn unknown_addresses_are_refused() {
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        let key_a = DispatchKey::Button(sid(10));
        let key_b = DispatchKey::Button(sid(20));

        // Unknown: refused, nothing registered, no counters move.
        assert_eq!(
            dispatcher.dispatch(DispatchKey::Button(sid(70)), vec![sid(7)]),
            Err(DispatchRefusal::UnknownAddress { address: sid(70) }),
        );
        assert_eq!(
            dispatcher.dispatch(DispatchKey::MenuItem(sid(70), "ghost".into()), vec![sid(7)]),
            Err(DispatchRefusal::UnknownAddress { address: sid(70) }),
        );
        assert_eq!(dispatcher.counters().activations_resolved, 0);
        assert_eq!(dispatcher.counters().commands_emitted, 0);
        assert_eq!(dispatcher.counters().a11y_labels_derived, 0);
        assert_eq!(
            dispatcher.accessible_label(DispatchKey::Button(sid(70))),
            Err(DispatchRefusal::UnknownAddress { address: sid(70) }),
            "the a11y path refuses unknown addresses too (and counts nothing)"
        );

        // Two prompt-policy buttons — DIFFERENT def prompts, and complete
        // per-address lifecycle isolation.
        let def_a = IconActionDef {
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Confirm A".to_string(),
            }),
            ..button_def(10, "Action A", "a_command")
        };
        let def_b = IconActionDef {
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Confirm B".to_string(),
            }),
            ..button_def(20, "Action B", "b_command")
        };
        dispatcher.sync_button(&def_a);
        dispatcher.sync_button(&def_b);

        // Arm from A; B stays un-armed.
        let outcome = dispatcher.dispatch(key_a.clone(), vec![sid(1)]).expect("arm A");
        assert!(
            matches!(outcome, DispatchOutcome::PromptRequested { ref prompt } if prompt == "Confirm A"),
            "A's own prompt, from A's own def"
        );
        assert!(dispatcher.pending_prompt(&key_b).is_none(), "B is NOT armed by A's dialog");
        assert!(
            dispatcher.confirm(key_b.clone()).is_err(),
            "confirming an un-armed address refuses"
        );

        // A confirmation dialog never burns the OTHER def; arm B, then
        // assert isolation.
        let outcome = dispatcher.dispatch(key_b.clone(), vec![sid(2)]).expect("arm B");
        assert!(
            matches!(outcome, DispatchOutcome::PromptRequested { ref prompt } if prompt == "Confirm B"),
            "B's own prompt came from B's own def"
        );
        assert_eq!(dispatcher.pending_prompt(&key_a), Some("Confirm A"), "A is still armed");
        assert_eq!(dispatcher.pending_prompt(&key_b), Some("Confirm B"), "B is now armed too");

        // Confirm A pops JUST A, with A's OWN command riding the emission.
        let outcome = dispatcher.confirm(key_a.clone()).expect("A confirmed");
        match emitted_event(outcome) {
            WidgetEvent::CommandInvoked { ref command_id, .. } => {
                assert_eq!(command_id, "a_command", "the armed command delivers exactly once");
            }
            other => panic!("expected CommandInvoked, got {other:?}"),
        }
        assert!(dispatcher.pending_prompt(&key_a).is_none(), "ONLY A's stage was popped");
        assert_eq!(dispatcher.pending_prompt(&key_b), Some("Confirm B"), "B untouched by A");

        // Confirm a never-synced address: the distinct refusal.
        assert_eq!(
            dispatcher.confirm(DispatchKey::Button(sid(99))),
            Err(DispatchRefusal::ConfirmWithoutPrompt { address: sid(99) })
        );

        // B's dialog still works after A's lifecycle settled — clean
        // isolation end-to-end.
        let outcome = dispatcher.confirm(key_b.clone()).expect("B confirmed");
        match emitted_event(outcome) {
            WidgetEvent::CommandInvoked { ref command_id, .. } => {
                assert_eq!(command_id, "b_command");
            }
            other => panic!("expected CommandInvoked, got {other:?}"),
        }
    }

    // -- pin -----------------------------------------------------------------------------

    /// Pin the NON-WIRE refusal vocabulary from creation: variant count and
    /// declaration order (tail-append only) — appending future reasons
    /// keeps the order below; a reorder/rename rewrites this pin.
    #[test]
    fn refusal_reasons_pinned_from_creation() {
        assert_eq!(
            refusal_pin(&DispatchRefusal::Disabled { address: sid(0), command: None }),
            0
        );
        assert_eq!(
            refusal_pin(&DispatchRefusal::Hidden { address: sid(0), command: None }),
            1
        );
        assert_eq!(refusal_pin(&DispatchRefusal::AlreadyInFlight { address: sid(0) }), 2);
        assert_eq!(refusal_pin(&DispatchRefusal::UnknownAddress { address: sid(0) }), 3);
        assert_eq!(
            refusal_pin(&DispatchRefusal::InvalidEvent {
                address: sid(0),
                reason: WidgetError::BudgetExceeded { current: 1, max: 1 },
            }),
            4
        );
        assert_eq!(refusal_pin(&DispatchRefusal::NoTargets { address: sid(0) }), 5);
        assert_eq!(refusal_pin(&DispatchRefusal::ConfirmWithoutPrompt { address: sid(0) }), 6);
    }

    fn refusal_pin(refusal: &DispatchRefusal) -> u8 {
        use DispatchRefusal::*;
        match refusal {
            Disabled { .. } => 0,
            Hidden { .. } => 1,
            AlreadyInFlight { .. } => 2,
            UnknownAddress { .. } => 3,
            InvalidEvent { .. } => 4,
            NoTargets { .. } => 5,
            ConfirmWithoutPrompt { .. } => 6,
        }
    }
}
