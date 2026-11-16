//! Native iced mount over the plugin widget state layers (`SCN-013` of the
//! scene-UI architecture effort).
//!
//! This is the NATIVE half the widget layers anticipated: the plugin owns the
//! semantic widget data ([`WidgetSurface`], SWG-001), [`WidgetUiState`]
//! (SWG-002, `plugin_widgets.rs`) owns what is expanded/selected/focused/
//! windowed, and [`WidgetDispatcher`] (SWG-003, `plugin_widgets_dispatch.rs`)
//! owns the resolved command decision. The mount is deliberately thin: **the
//! state layers decide, the mount renders** — every builder is a pure
//! function over the defs + the state machines' outputs (native `button`/
//! `text` widgets, one visible window, cheap construction), and every
//! interaction routes back into the same machines through [`apply_mount_msg`].
//! Nothing duplicates enablement/selection/validation logic; the machines
//! remain the single semantic authority (§16).
//!
//! # The message bridge (SCN-014 — shipped here)
//!
//! The mount emits its own host-local vocabulary [`WidgetMountMsg`] and owns
//! its routing [`apply_mount_msg`] into the state machines; the APP-side
//! wiring landed in SCN-014:
//!
//! > The builders attach `on_press(WidgetMountMsg)`; the single panel gate
//! > maps each emitted message through [`app_message`] into
//! > `app::Message::WidgetMountMsg`, whose update arm routes into
//! > `app::update::widget_mount`: find the registered surface
//! > ([`MountRegistry::handle_for_surface_owned`]), `apply_mount_msg(.., state,
//! > dispatcher, msg, sink)`, drain the sink, and settle every forwarded
//! > command's originating address — **bridge-forwards-then-settle**, so
//! > the in-flight exactly-once guard re-arms after the handler completes.
//!
//! ADR-004 (API-23) crossing / SCN-015 boundaries, and where they stand
//! after SCN-025 (the wire attach):
//! - the events the bridge drains: the COLLECTING sink ([`CollectingSink`])
//!   was the pipeline seam the future publisher hangs off — that publisher
//!   LANDED with SCN-025 (the drained batches forward through the v23
//!   notify router from the arms via `app::update::widget_mount`'s one
//!   forwarding helper; the sink itself keeps exactly its collecting
//!   contract);
//! - teardown dropping a surface's `pending_children` (state `reset_all`)
//!   cancels the mount's VIEW of pending lazy page fetches — the genuine
//!   in-flight IPC fetch cancellations still need the request-id
//!   vocabulary the wire era brings (RECORDED, not silently claimed).
//! Additionally the prompt dialog's SEMANTIC half shipped here (SCN-015 —
//! see "The armed prompt's dialog lifecycle" below); SCN-016 landed its
//! VISUAL half (the app-modal placement) and the APP-OWNED registry
//! migration.
//!
//! # The mount becomes INPUT-CAPABLE (SCN-020 — hover, capture, coalescing)
//!
//! SCN-020 adds the mount's pointer/input layer, host-local per §7/§14:
//! - **[`HoverState`]** — the per-surface input slot (hover, capture,
//!   capture origin, last delivered move, owed-move slot) living INSIDE the
//!   [`MountRegistry`], so its lifetime is exactly its surface's (every
//!   register/teardown path sweeps it). Hover is host-local by contract:
//!   set through [`MountRegistry::mount_set_hover_owned`] (the wiring's report —
//!   the flush deliberately does NOT hit-test; the mount owns no per-row
//!   geometry), read by SCN-021's drag previews through
//!   [`MountRegistry::mount_hover_state_owned`]. NO machine ever sees a hover or
//!   a move (§14).
//! - **Pointer capture** — the panel hook wraps the mounted body with
//!   [`pointer_capture_body`] (this iced revision's `mouse_area` idiom):
//!   press → [`WidgetMountMsg::PointerPressed`] → capture ON + the press
//!   surface becomes the mount's keyboard target; move →
//!   [`WidgetMountMsg::PointerMoved`] → the coalescer entry; release →
//!   [`WidgetMountMsg::PointerReleased`] → capture OFF + the owed move
//!   delivered NOW. Release ANYWHERE is the pure completion contract
//!   ([`MountRegistry::mount_pointer_released_owned`]) — the wrapper wires
//!   the releases this iced revision delivers inside the area, and the
//!   window-level release catcher (SCN-016) is the app's gated listener
//!   completing every armed capture.
//! - **Latest-wins move coalescing** — the viewport coalescer's contract
//!   (its `defer_pointer_move`/`on_move_coalesce_tick` precedent, measured
//!   there) reduced to the mount: every move DEFERS into the surface's
//!   owed slot (a page-queue is precisely what the viewport measured as
//!   the 986-positions defect — the mount never grows one); a newer move
//!   OVERWRITES the slot and counts [`MountCounters::coalesced_moves_superseded`];
//!   the flush (the app's move-coalesce tick, joined onto the EXISTING
//!   `Message::MoveCoalesceTick` arm; plus the release completion) delivers
//!   ≤1 mount call per surface per tick ([`MountCounters::mount_move_calls`]).
//!   A mount "call" is the registry's own [`HoverState`] write — there is
//!   no plugin/event side to a move at all.
//!
//! # The mount becomes KEYBOARD-NAVIGABLE (SCN-020)
//!
//! [`key_route`] is the keyboard layer's pure translation: keys → the
//! route messages the bridge already applies. The state layers decide
//! EVERYTHING: the ring ([`nav_ring`]) only names what the builders
//! rendered (trees' current flat rows, lists, visible icon buttons), and
//! the roving (`focus_next`/`focus_prev`) SKIPS disabled by the machines'
//! own refusal — the keyboard layer never re-implements enablement. The
//! keys: Up/Down = one ring step + the selection following it
//! (FocusMoved + SelectionChanged); Left/Right = a TREE row's expansion
//! toggle; Enter/Space = the focused thing's activation; Tab/Shift+Tab =
//! the ring advance with wrap; Escape = the machine-direct cancel of the
//! surface's armed prompt ([`MountRegistry::mount_escape_cancel_owned`]; drag
//! cancel is SCN-021's). Chords (Ctrl/Alt/Logo) are the app's accelerator
//! space and route nothing. The app-side resolution ([`handle_mount_key`]
//! in `app::update::widget_mount`) resolves the TARGET surface (the
//! press-captured one, else the sole registered mount — the keyboard-only
//! §16 path), runs the translation, and routes every produced message
//! through the ordinary bridge.
//!
//! TABLES: the typed-table machine ([`mount_table_key_route`], consuming
//! the frozen SWG-004b state layer) is the table keyboard scope — Up/Down
//! row roving within the materialized window, Left/Right the caller-held
//! cell cursor, Escape the FREE draft cancel. The mount's TABLE placeholder
//! is not interactive yet (the typed-table MOUNT integration is the later
//! task), so the table keys are pure-layer complete and headless-tested.
//!
//! # The armed prompt's dialog lifecycle (SCN-015 — the semantic half)
//!
//! A `ConfirmPolicy::Prompt` activation becomes a THREE-state host flow,
//! all keyed to the ONE `PromptRequested` routing the bridge already runs:
//!
//! 1. **arm** — the routing records the surface-ready dialog desc
//!    ([`MountPromptDesc`] — prompt text from the machine's captured
//!    stage, command + targets captured at arm time like the machine's own
//!    `PromptStage`) into the registry's `prompts` map, alongside the
//!    SCN-020 escape hint;
//! 2. **show** — [`MountRegistry::prompt_dialog_owned`] is the READ the app's
//!    confirm modal displays (and [`prompt_modal_element`] the mount-side
//!    body builder: the message text + the Cancel/OK pair);
//! 3. **decide** — the decisions ride [`WidgetPromptMsg`] →
//!    `Message::WidgetPromptMsg` → `app::update::widget_prompt`:
//!    confirm → [`WidgetDispatcher::confirm`] + the collecting sink +
//!    settle (exactly one captured command); dismiss →
//!    [`WidgetDispatcher::dismiss_prompt`] (no event; a re-click
//!    re-prompts). Escape rides the SAME edge through
//!    [`MountRegistry::mount_escape_cancel_owned`].
//!
//! Refusals (a decision naming no pending interaction, an unknown surface,
//! a stale/mismatched key) are graceful: only the refusal's own probe
//! counter moves ([`MountCounters::prompts_refused`]), nothing machines,
//! nothing panics. The REGISTRY desc is display copy; the machine's stage
//! is the exchange's authority — recorded verbatim at
//! [`MountRegistry::resolve_prompt_desc_owned`].
//!
//! The VISUAL modal placement landed with SCN-016 (its own section below
//! the "Wiring status" update): the app's `ModalKind::WidgetPrompt` modal
//! consumes exactly [`MountRegistry::prompt_dialog_owned`] +
//! [`prompt_modal_element`] + [`prompt_app_message`].
//!
//! # Wiring status (SCN-016 LANDED — the recorded one-line seams)
//!
//! - The real keyboard LISTENER join is in: the app's key event flow hands
//!   a main-window key to the mount route as
//!   `Message::WidgetMountMsg(WidgetMountMsg::KeyPressed { .. })` the
//!   moment a mount surface holds the keyboard (the subscription chooser
//!   in `app/view/mod.rs` picks the mount mapper over the ordinary
//!   `main_window_event` listener exactly while
//!   [`MountRegistry::mount_key_surface_owned`] resolves; the prompt
//!   modal's own showing state swaps to the Escape-only mapper, so its
//!   Escape dismiss rides the machine-direct cancel). With no mount
//!   holding the keyboard — the shipping default, since NOTHING registers
//!   a surface yet — the ordinary listener runs and NOTHING changes.
//! - The move-coalesce tick's subscription PUMP condition also flows while
//!   the MOUNT has a move owed ([`MountRegistry::any_move_owed`]) — the
//!   mount's coalescer driver answers through the existing
//!   `Message::MoveCoalesceTick` arm.
//! - The window-level RELEASE catch (the release-ANYWHERE completion
//!   contract) is a gated listener too: subscribed exactly while a capture
//!   is armed ([`MountRegistry::any_capture_active`]), it completes every
//!   armed capture with `WidgetMountMsg::PointerReleasedAnywhere`.
//! - Hover stays host-local as designed: nothing production-side can name
//!   a per-row hover yet (the flush deliberately does not hit-test; the
//!   per-row hover wiring is a later mount slice, and
//!   [`MountRegistry::mount_set_hover_owned`] remains its seam).
//! - Headless tests set the keyboard target through
//!   [`MountRegistry::set_mount_key_target_owned`] (the same seam the
//!   app-owned registry's roving-panel focus will own in the wire era).
//!
//! # The panel hook consults the APP-OWNED store (the migration, SCN-016)
//!
//! `PluginPanelState` derives `Clone` + `PartialEq`; an
//! `Rc<RefCell<WidgetUiState>>`-shaped field cannot join those derives (the
//! panel's Clone usages are test conveniences this task may not disturb),
//! so the registry stays a SIDE store — SCN-016 moves its STORAGE into the
//! app: a `mount_registry` field on `OpenCADStudio` (the update arms hold
//! `self`; the dock's panel composition passes the store's reference into
//! `PluginPanelState::view`). Every panel without a registry entry —
//! every panel registered today, since nothing registers a widget surface
//! yet — takes the legacy path byte-identically, so the legacy-behavior
//! guarantee holds BY CONSTRUCTION.
//!
//! # Native iced realities (checked against the vendored revision)
//!
//! - iced core ships no first-class menu widget at this rev. The repo's own
//!   menu idiom is `iced_aw::menu::MenuBar` (`statusbar::status_menu`) — a
//!   native drop-down with overlay state. The mount uses exactly that for
//!   [`menu_element`] (root button + native menu + recursive submenus);
//!   a right-click/context-menu form is deferred because it needs per-widget
//!   open state + toggle messages (the SCN-015 integration follow-up).
//! - SplitView ratio: a `Group` with `split_ratio` divides via
//!   `Length::FillPortion` splits (the vendored `Length` has portion fills);
//!   the ratio is clamped host-side exactly per the contract's note
//!   ("a nonsensical or non-finite ratio is CLAMPED host-side") —
//!   [`clamp_split_ratio`]: non-finite → `0.5`, otherwise bounded into
//!   `[0.01, 0.99]` so both children keep >1% of the box (a split never
//!   fully hides a child). A `split_ratio` on a group that is not a 2-child
//!   group is ignored (plain stacking) — the ratio names a TWO-pane split.
//! - Icons: `IconRef` uris name assets in a host store (`host://…` in the
//!   fixtures); resolving them through a parsed-SVG handle cache (the
//!   `plugin_dialog::collect_svg_handles` precedent) is integration work.
//!   The v1 mount renders label + status text runs; the icon glyph slot
//!   stays reserved by the def and unrendered in v1 (documented deviation).
//! - Visible window: the mount keeps a FIXED window budget
//!   ([`DEFAULT_WINDOW_SPEC`], 48 rows + an 8-row warm margin) until the
//!   dock reports real viewport geometry — the window's size then becomes
//!   the dock's input (the SCN-015 follow-up); `Scrolled` messages already
//!   route the scroll anchor through the state machine, so that change is
//!   plumbing.
//!
//! # Windowing is the mount's local bookkeeping (§16)
//!
//! `flat_rows_of_tree` / `flat_rows_of_list` build the FLAT list of the
//! current expansion state (the SWG-002 pattern: cheap id structs, no
//! widgets), [`materialized_row_count_of_list`] and its tree sibling read
//! the visible window off it, and the builders materialize ONLY that window
//! — 100k backing objects materialize a few dozen row widgets, never 100
//! thousand (the same discipline the SWG-002 proofs pin, mirrored here over
//! the native half). The per-view flat-list walk is the established SWG-002
//! cost envelope: flat rows are cheap id structs, and there is one
//! O(def rows) scan per view — no widget per backing object.
//!
//! # Enablement pick (documented)
//!
//! Builders DO attach `on_press` only when enablement resolves clickable —
//! a disabled button renders without a press route ("the builder hides a
//! dead press"). The [`WidgetDispatcher`]'s refusal stays the semantic
//! authority: [`apply_mount_msg`] re-routes EVERY press through the
//! machines, whose refusals (disabled / hidden / in-flight / unknown /
//! no-targets / invalid-event) are the belt-and-braces second half — a
//! stale widget built during a disable streak still cannot emit a command
//! event.
//!
//! # Command targets (documented v1 semantics)
//!
//! A semantic command names what it acts ON (§13: "invocation with explicit
//! target IDs"). The mount routes the surface's CURRENT SELECTION as the
//! click's target ids; a click with an empty selection refuses at the state
//! layer (`NoTargets`) — the semantic authority, not the builder, refuses.
//! Per-button declared targets are an integration-layer concern (the
//! SCN-015 follow-up) if a host contract later wants them.
//!
//! # A11y
//!
//! iced's native widgets carry their visible text as the screen-reader text
//! on the native stack; the mount's EXACT access-name guarantee lives in
//! [`MountFacts::a11y`] (pinned by tests to the machines' pure derivations —
//! buttons: the required `accessible_name` verbatim; menu items: the label
//! composed with the shortcut hint). Tree rows additionally present the
//! expansion affordance as an ASCII `+`/`-` marker prefix (the codebase's
//! existing marker palette); the marker is decoration, the a11y fact stays
//! the label verbatim.

// The stage-1 blanket allow is LIFTED where SCN-014's wiring lands (the
// bridge: app_message, the gate mapping, registration lifecycle, sink, and
// the builders/routing reachable from them — production dead-code analysis
// now covers the whole wired surface). The remaining assert-only surface
// carries targeted `cfg_attr(not(test), allow(dead_code))`s of its own,
// each naming WHY it stays.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use iced::keyboard::{Key, Modifiers, key as keyboard_key};
use iced::widget::tooltip::Position as TooltipPosition;
use iced::widget::{Tooltip, button, column, container, mouse_area, row, text};
use iced::{Element, Fill, Length, Point, Theme};
use iced_aw::menu::{Item, Menu, MenuBar};
// The v23 panel-surface store's vocabulary (SCN-025). Wire-adjacent STRUCT
// types only (codes, reports, bounded envelopes); no frames built here — the
// PluginResponse mapping happens in `app::plugin_host`. The `ipc` module is
// host-feature-only: desktop-only machinery, which the plugin-host attach is.
#[cfg(not(target_arch = "wasm32"))]
use ocs_plugin_api::ipc::protocol::{
    PanelStoreKind, PatchRejectCode, PatchRejectReport, SceneOrWidgetPatch, SceneOrWidgetSurface,
    StoreRow, StoreStateDigest,
};
#[cfg(not(target_arch = "wasm32"))]
use ocs_plugin_api::widgets::WidgetRange;
use ocs_plugin_api::scene::{PanelScene, SceneElement, SceneKindCounts, SceneRevision, StableId};
use ocs_plugin_api::widgets::{
    Axis, IconActionDef, ItemListDef, MAX_MENU_DEPTH, MAX_WIDGET_RANGE_LEN,
    MAX_WIDGET_ROWS_INITIAL, MenuDef, TableRef, TreeRowDef, WidgetEvent, WidgetGroup, WidgetNode,
    WidgetSurface, WidgetTreeDef,
};

use crate::ui::style::form::button_style;
use crate::ui::style::readable::muted_text;
#[cfg(test)]
use crate::ui::window::plugin_panel::PluginPanelState;
use crate::ui::window::plugin_widgets::{
    FlatRow, VisibleWindow, WidgetStateError, WidgetUiState, WindowSpec, visible_window,
};
use crate::ui::window::plugin_widgets_dispatch::{
    ControlSemantics, DispatchKey, DispatchOutcome, ItemSemantics, WidgetDispatcher,
    project_button, project_menu,
};
use crate::ui::window::plugin_widgets_tables::{CellFocus, TableUiState};


/// The mount's iced_aw menu element types with the crate's theme/renderer
/// set (the vendored `Item`/`Menu` carry explicit generics).
type MountItem<'a> = Item<'a, WidgetMountMsg, iced::Theme, iced::Renderer>;
type MountMenu<'a> = Menu<'a, WidgetMountMsg, iced::Theme, iced::Renderer>;

const MOUNT_TEXT_SIZE: f32 = 12.0;
const MOUNT_HINT_TEXT_SIZE: f32 = 11.0;
const MOUNT_ROW_SPACING: f32 = 2.0;
const MOUNT_GROUP_SPACING: f32 = 8.0;
const MOUNT_MENU_WIDTH: f32 = 180.0;
/// Maximum depth indents rendered before the prefix stops growing (the
/// surface's own depth budget bounds nesting; the cap keeps labels sane).
const MAX_ROW_INDENT: usize = 16;
/// Split resolution: portions are drawn at 1/1000 granularity so
/// `left + right == 1000` exactly.
const SPLIT_RESOLUTION: u16 = 1_000;

/// The mount's default visible-window budget (see the module docs).
pub(crate) const DEFAULT_WINDOW_SPEC: WindowSpec = WindowSpec {
    viewport_height: 48.0,
    row_height: 1.0,
    margin_rows: 8,
};

// ---------------------------------------------------------------------------
// WidgetMountMsg — the mount's message vocabulary (host-local)
// ---------------------------------------------------------------------------

/// The mount's local message vocabulary: exactly what the native widgets
/// emit for one user gesture, fully addressed (surface + widget + row/item).
///
/// Host-local, NOT a wire change. The app bridge (SCN-014) routes these into
/// [`apply_mount_msg`] over the same
/// `Rc<RefCell<WidgetUiState>>` / `Rc<RefCell<WidgetDispatcher>>` pair the
/// registry handed the view.
///
/// SCN-020 adds the mount's POINTER-capture vocabulary
/// ([`WidgetMountMsg::PointerPressed`] / [`WidgetMountMsg::PointerMoved`] /
/// [`WidgetMountMsg::PointerReleased`] — `Eq` therefore yields to
/// `PartialEq`: this iced revision's `Point` is not an equality type). These
/// three ride the SAME single-wagon rule (the one `Message::WidgetMountMsg`
/// variant), but they do NOT route into the machines: pointering is the
/// mount's own host-local input store (§7 "hover/drag/previews/pan-zoom
/// stay HOST-LOCAL"; §14 "the machine layers never see raw moves") — the
/// app arm hands them to the coalescer/capture routes
/// (`app::update::widget_mount`), and a pointer message that still reached
/// [`apply_mount_msg`] would be refused by name.
// Bench seam, SCN-024; ADR-007 item 3; never wire — the mount vocabulary the bench
// reads (the keyboard route's step messages). Host-local; NOT a wire change.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetMountMsg {
    /// A row's activation gesture (a press on an already-selected list item —
    /// the native "press again to open" form, see [`list_element`]).
    RowActivated {
        surface_id: StableId,
        widget_id: StableId,
        row_id: StableId,
    },
    /// A row's expansion toggled TO `expanded` (the intended NEW state; the
    /// press computes the flip against the machine's effective value).
    RowExpandedToggled {
        surface_id: StableId,
        widget_id: StableId,
        row_id: StableId,
        expanded: bool,
    },
    /// Selection replaced; the state machine enforces capability + caps.
    SelectionChanged {
        surface_id: StableId,
        widget_id: StableId,
        selected: Vec<StableId>,
    },
    /// An icon button click (the def carries the semantics).
    ButtonClicked {
        surface_id: StableId,
        widget_id: StableId,
    },
    /// A menu item click; the item is COMMAND-addressed, so the OWNING menu
    /// id + the item's `command_id` name it (the dispatcher's address
    /// convention).
    MenuItemClicked {
        surface_id: StableId,
        menu_id: StableId,
        command_id: String,
    },
    /// A scrolled list/tree re-anchored at `top_row` (flat-list row index).
    // Wired vocabulary, but the v1 builders never construct it (the
    // scroll wheel input is the next mount slice) — its routing
    // (`apply_mount_msg`) is live all the same.
    #[allow(dead_code)]
    Scrolled {
        surface_id: StableId,
        widget_id: StableId,
        top_row: usize,
    },
    /// Keyboard focus moved to row/item `to` (`None` = away; a moved-away
    /// focus keeps its slot — blur has no machine method to call).
    // STILL never emitted by a BUILDER (focus arrives through the
    // keyboard layer, [`key_route`], from SCN-020); its routing
    // (`apply_mount_msg` → `focus_set`) is live all the same.
    #[allow(dead_code)]
    FocusMoved {
        surface_id: StableId,
        widget_id: StableId,
        to: Option<StableId>,
    },
    /// The pointer pressed over the mounted body (the surface-level capture
    /// wrapper — [`pointer_capture_body`] — emits it on every press the
    /// body's own widgets did not capture first: blank Surface presses).
    /// Capture turns ON for the surface (host-local; also the keyboard
    /// target: a press-keyed surface owns the mount's keys). The press
    /// carries NO position — this iced revision's `mouse_area` reports
    /// positions on MOVE only; the capture origin lands with the first
    /// coalesced move (§7: SCN-021's drag previews consume it).
    PointerPressed {
        surface_id: StableId,
    },
    /// The pointer moved over the mounted body. NEVER routed per event:
    /// the app arm hands it to the latest-wins coalescer
    /// ([`MountRegistry::mount_move_defer_owned`]) — the slot overwrites, the
    /// tick/flush answers ONLY the last one (≤1 mount call per surface per
    /// tick), and no machine ever sees a raw move (§14).
    PointerMoved {
        surface_id: StableId,
        position: Point,
    },
    /// The pointer released completing whatever capture held it —
    /// release ANYWHERE completes (the pure completion contract is
    /// [`MountRegistry::mount_pointer_released_owned`]; the wrapper wires the
    /// releases this iced revision delivers inside the area and the
    /// window-level release catcher rides the other tail variant).
    /// The surface is the one whose `mouse_area` saw the release.
    PointerReleased {
        surface_id: StableId,
    },
    // The vocabulary's tail variants (SCN-016's input hookups) — both ride
    // the ONE `Message::WidgetMountMsg` wagon (no new `Message` variant):
    // neither routes into the machines.
    /// One LIVE keyboard key handed to the mount's keyboard layer by the
    /// app's event flow (SCN-016's `main_window_event`/subscription join —
    /// the mapper in `app::view::mod`). Carries NO surface id: the target
    /// is the keyboard ROUTE's resolution
    /// ([`MountRegistry::mount_key_surface_owned`] — the press-captured
    /// surface, else the sole registered mount), so the arm routes this
    /// variant straight into [`OpenCADStudio::on_mount_key`]
    /// (`app::update::widget_mount`) and it never touches
    /// [`apply_mount_msg`]/the machines.
    // The app-hook completion: this variant is the join the wiring note
    // recorded — key events produced by the REAL listener land as this
    // message, whose arm runs the complete, headless-tested route.
    KeyPressed {
        key: Key,
        modifiers: Modifiers,
    },
    /// The window-level pointer release (the release-ANYWHERE catcher):
    /// complete EVERY armed capture — "release anywhere completes capture"
    /// resolved for the releases that land outside any panel's mouse area
    /// (no surface id: the captured surfaces are the store's own list).
    PointerReleasedAnywhere,
}

// ---------------------------------------------------------------------------
// Routing: one message into the layered state machines
// ---------------------------------------------------------------------------

/// The boundary a mount forwards emitted [`WidgetEvent`]s through. SCN-014
/// landed the bridge's collecting implementation ([`CollectingSink`] —
/// drained, not yet published, until the ADR-004 crossing); the headless
/// tests also plug a plain `Vec<WidgetEvent>` in and pin that the EVENTS,
/// not just counters, are exact.
pub(crate) trait WidgetEventSink {
    /// One emitted event. Implementations forward it; NOTHING else moves.
    fn emit(&mut self, event: WidgetEvent);
}

impl WidgetEventSink for Vec<WidgetEvent> {
    fn emit(&mut self, event: WidgetEvent) {
        self.push(event);
    }
}

/// What [`apply_mount_msg`] did with one mount message — the routing
/// verdict's total shape (each variant is terminal; nothing half-applies).
///
// The verdict's FIELDS are the tests' assertion surface (the bridge reads
// the routing by shape, not by field).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MountRouting {
    /// The state layers accepted the message; `forwarded` semantic events
    /// left through the sink (`0` covers an idempotent re-press that changed
    /// nothing — a stale duplicate press is benign, not a refusal).
    Applied {
        /// The number of semantic events forwarded to the sink.
        forwarded: usize,
    },
    /// The dispatch resolved to a CONFIRMATION: the mount shows the prompt
    /// and the machine arms (the dialog's confirm/dismiss path completes
    /// the interaction directly on the dispatcher). No command event yet.
    PromptRequested {
        /// The prompt text to show.
        prompt: String,
    },
    /// Refused by a state layer — no event left through the sink. The
    /// machines' own refusal counters are the §16 probes; the string is the
    /// machine's own Display text (pinned by the SWG-002/003 tests).
    Refused(String),
}

/// Route ONE [`WidgetMountMsg`] into the layered state machines — the
/// addressable half of the mount (the builders emit; this resolves).
///
/// The sink receives every VALID emitted [`WidgetEvent`]; refusals receive
/// nothing and move only the machines' own counters. Notes (pinned by
/// tests):
/// - the read model comes from the LAST `sync` (a click-rate clone of the
///   views map — the machines own both the mutable state and the read
///   model, so routing clones the model out per message);
/// - `ButtonClicked` / `MenuItemClicked` route the surface's CURRENT
///   selection as the command's explicit targets and the machine resolves
///   them (empty selection → `NoTargets`; a disabled command → `Disabled`);
/// - the message's `surface_id` must match the state's — a cross-surface
///   message is a routing defect, refused before anything moves;
/// - an emitted event is NOT settled here: settling is the bridge's job
///   after forwarding (`WidgetDispatcher::settle`), which is exactly what
///   keeps a double click refused in flight.
pub(crate) fn apply_mount_msg<S: WidgetEventSink + ?Sized>(
    state: &mut WidgetUiState,
    dispatcher: &mut WidgetDispatcher,
    msg: &WidgetMountMsg,
    sink: &mut S,
) -> MountRouting {
    if msg_surface_id(msg) != state.surface_id() {
        return MountRouting::Refused(format!(
            "message names surface {} but the mount state owns surface {}",
            msg_surface_id(msg).0,
            state.surface_id().0
        ));
    }
    let Some(synced) = state.synced().cloned() else {
        return MountRouting::Refused(
            "the mount state has no synced read model yet (registration defect)".to_string(),
        );
    };

    match msg {
        WidgetMountMsg::RowActivated { row_id, .. } => {
            walk_state_event(state.item_activate(*row_id, &synced.views), sink)
        }
        WidgetMountMsg::RowExpandedToggled {
            row_id, expanded, ..
        } => {
            // Validate the address first (an unknown/not-synced row refuses —
            // even when the intended state happens to equal the current
            // one), THEN honor the intent-explicit toggle; a stale duplicate
            // press (the row already at `expanded`) is a benign no-op.
            if !synced.views.contains_key(row_id) {
                return MountRouting::Refused(WidgetStateError::UnknownRow(*row_id).to_string());
            }
            if state.effective_expanded(*row_id, false) == *expanded {
                return MountRouting::Applied { forwarded: 0 };
            }
            match state.toggle_expand(*row_id, &synced.views) {
                Ok(Some(event)) => {
                    sink.emit(event);
                    let mut forwarded = 1;
                    // Expand-ON of a lazy row opens the parent's child page:
                    // the request is the event (SWG-002); the initial child
                    // window is the mount's page ask.
                    if *expanded {
                        if let Some(request) = state.request_children_if_lazy(
                            *row_id,
                            0..MAX_WIDGET_RANGE_LEN,
                            &synced.views,
                        ) {
                            sink.emit(request);
                            forwarded += 1;
                        }
                    }
                    MountRouting::Applied { forwarded }
                }
                Ok(None) => MountRouting::Applied { forwarded: 0 },
                Err(reason) => MountRouting::Refused(reason.to_string()),
            }
        }
        WidgetMountMsg::SelectionChanged {
            widget_id,
            selected,
            ..
        } => {
            let Some(view) = synced.widget_views.get(widget_id) else {
                return MountRouting::Refused(format!(
                    "widget {} is not in the synced surface (selection capability unknown)",
                    widget_id.0
                ));
            };
            walk_state_event(
                state.set_selection(selected.clone(), *widget_id, view),
                sink,
            )
        }
        WidgetMountMsg::ButtonClicked { widget_id, .. } => route_command(
            dispatcher,
            DispatchKey::Button(*widget_id),
            state.selection().to_vec(),
            sink,
        ),
        WidgetMountMsg::MenuItemClicked {
            menu_id,
            command_id,
            ..
        } => route_command(
            dispatcher,
            DispatchKey::MenuItem(*menu_id, command_id.clone()),
            state.selection().to_vec(),
            sink,
        ),
        WidgetMountMsg::Scrolled { top_row, .. } => {
            state.set_scroll_row(*top_row);
            // The window re-derives from the anchor on the NEXT view; no
            // event leaves (scrolling is host-local, §14).
            MountRouting::Applied { forwarded: 0 }
        }
        WidgetMountMsg::FocusMoved { to, .. } => match to {
            Some(id) => match state.focus_set(*id, &synced.views) {
                Ok(()) => MountRouting::Applied { forwarded: 0 },
                Err(reason) => MountRouting::Refused(reason.to_string()),
            },
            // Blur: focus is the roving model's single host-local slot; the
            // v1 mount keeps the current focus (no machine method clears
            // it) — a documented no-op.
            None => MountRouting::Applied { forwarded: 0 },
        },
        // Pointer messages are NOT machine vocabulary (§14: the machine
        // layers never see raw moves) — the app arm routes them to the
        // mount's coalescer/capture paths BEFORE any machine router runs;
        // one that still reached here is refused by name (defensive total —
        // nothing half-applies, nothing moves).
        WidgetMountMsg::PointerPressed { .. }
        | WidgetMountMsg::PointerMoved { .. }
        | WidgetMountMsg::PointerReleased { .. } => MountRouting::Refused(
            "pointer messages drive the mount's host-local input store, not the machines (§14)"
                .to_string(),
        ),
        // The keyboard events ride their OWN route (the app's
        // `on_mount_key` never calls this apply; the arm pre-routes
        // `KeyPressed`), and the window-level release has no surface —
        // reaching here means the message BYPASSED its route (a defect);
        // refuse by name, exactly the pointer discipline.
        WidgetMountMsg::KeyPressed { .. } | WidgetMountMsg::PointerReleasedAnywhere => {
            MountRouting::Refused(
                "this variant owns a route of its own; it is not a machine message (§14)"
                    .to_string(),
            )
        }
    }
}

/// The surface address a mount message names (uniform access for the
/// surface-mismatch check — and the bridge lookup's key: the
/// `Message::WidgetMountMsg` handler finds the registered mount by this).
pub(crate) fn msg_surface_id(msg: &WidgetMountMsg) -> StableId {
    match msg {
        WidgetMountMsg::RowActivated { surface_id, .. }
        | WidgetMountMsg::RowExpandedToggled { surface_id, .. }
        | WidgetMountMsg::SelectionChanged { surface_id, .. }
        | WidgetMountMsg::ButtonClicked { surface_id, .. }
        | WidgetMountMsg::MenuItemClicked { surface_id, .. }
        | WidgetMountMsg::Scrolled { surface_id, .. }
        | WidgetMountMsg::FocusMoved { surface_id, .. }
        | WidgetMountMsg::PointerPressed { surface_id, .. }
        | WidgetMountMsg::PointerMoved { surface_id, .. }
        | WidgetMountMsg::PointerReleased { surface_id, .. } => *surface_id,
        // The keyboard route resolves its own target (no surface in the
        // variant — see `WidgetMountMsg::KeyPressed`); the arm pre-routes
        // it, so a direct apply never reaches this arm. `StableId(0)` keeps
        // this accessor total; it names no real mount.
        WidgetMountMsg::KeyPressed { .. } | WidgetMountMsg::PointerReleasedAnywhere => StableId(0),
    }
}

/// Apply one `Result<Option<WidgetEvent>, E>` state decision: emit the event
/// (when any) and report the verdict.
fn walk_state_event<S: WidgetEventSink + ?Sized, E: std::fmt::Display>(
    decision: Result<Option<WidgetEvent>, E>,
    sink: &mut S,
) -> MountRouting {
    match decision {
        Ok(Some(event)) => {
            sink.emit(event);
            MountRouting::Applied { forwarded: 1 }
        }
        Ok(None) => MountRouting::Applied { forwarded: 0 },
        Err(reason) => MountRouting::Refused(reason.to_string()),
    }
}

/// Resolve one command address through the dispatcher (button and menu-item
/// handlers share this; the click's targets are the surface's current
/// selection — see the module docs).
fn route_command<S: WidgetEventSink + ?Sized>(
    dispatcher: &mut WidgetDispatcher,
    key: DispatchKey,
    targets: Vec<StableId>,
    sink: &mut S,
) -> MountRouting {
    match dispatcher.dispatch(key, targets) {
        Ok(DispatchOutcome::Emitted(event)) => {
            sink.emit(event);
            // The bridge settles AFTER forwarding; not automatic here.
            MountRouting::Applied { forwarded: 1 }
        }
        Ok(DispatchOutcome::PromptRequested { prompt }) => MountRouting::PromptRequested { prompt },
        // Not reachable from `apply_mount_msg` (confirm is the dialog's own
        // call on the dispatcher) but the verdict stays total.
        Ok(DispatchOutcome::ConfirmedEmitted(event)) => {
            sink.emit(event);
            MountRouting::Applied { forwarded: 1 }
        }
        Err(refusal) => MountRouting::Refused(refusal.to_string()),
    }
}

// ---------------------------------------------------------------------------
// MountFacts — the headless descriptor each builder reports
// ---------------------------------------------------------------------------

/// The aux facts a builder reports ALONGSIDE its element — the headless
/// assertion surface (iced elements cannot be introspected without a
/// renderer). Facts state the WIDGET-level truth the builder baked into the
/// element; per-row routes gate themselves on the row view + the machines.
///
// Facts are consumed (read) by the assertion tests; the builders CONSTRUCT
// them on every live build.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountFacts {
    /// The widget the element answers for.
    pub widget_id: StableId,
    /// `true` when the element carries a press route — builders attach
    /// `on_press` ONLY when effective enablement resolves clickable ("the
    /// builder hides a dead press"; the dispatcher's refusal remains the
    /// semantic authority).
    pub clickable: bool,
    /// The exact accessible name the widget presents.
    pub a11y: String,
    /// The semantic command the route invokes (`None` for trees/lists and
    /// containers: rows carry their own routing; menu items are
    /// command-addressed inside [`WidgetMountMsg::MenuItemClicked`]).
    pub command: Option<String>,
}

// ---------------------------------------------------------------------------
// Builders (pure; the state layers decide, the mount renders)
// ---------------------------------------------------------------------------

/// Build the native TREE for one [`WidgetTreeDef`] over `state`.
///
/// The visible window (per `state`'s scroll anchor + `spec`) is the ONLY
/// thing materialized: each visible row is a native `button(text(..))`
/// whose label carries the depth indent + the `+`/`-` expansion marker,
/// whose style carries the selection (accent) and whose press routes per
/// the documented v1 map:
/// - expandable row → [`WidgetMountMsg::RowExpandedToggled`] (the twisty
///   affordance; a toggle ON of a lazy row also opens its child page);
/// - leaf row → [`WidgetMountMsg::SelectionChanged`] with that single row.
/// Disabled rows render muted with NO press route.
///
/// Total over `def` + `state` (a pre-sync state renders the empty control
/// rather than an `Option`; see the module docs discipline).
pub(crate) fn tree_element(
    widget: &WidgetTreeDef,
    state: &WidgetUiState,
    spec: &WindowSpec,
) -> (Element<'static, WidgetMountMsg>, MountFacts) {
    let surface_id = state.surface_id();
    let a11y = widget.name.clone().unwrap_or_else(|| "tree".to_string());
    let Some(synced) = state.synced() else {
        return empty_control(widget.id, &a11y);
    };

    let flat = state.flat_rows_of_tree(widget, &synced.views);
    let window = visible_window(&flat, state.scroll_row(), spec);

    let mut col = column![].spacing(MOUNT_ROW_SPACING).width(Fill);
    for (depth, tree_row) in windowed_tree_rows(widget, &flat, &window) {
        let enabled = synced
            .views
            .get(&tree_row.id)
            .is_some_and(|view| view.enabled);
        col = col.push(tree_row_widget(
            surface_id,
            widget.id,
            tree_row,
            depth,
            enabled,
            state.selection().contains(&tree_row.id),
            state.effective_expanded(tree_row.id, false),
        ));
    }

    (
        col.into(),
        MountFacts {
            widget_id: widget.id,
            clickable: synced
                .widget_views
                .get(&widget.id)
                .is_some_and(|view| view.enabled),
            a11y,
            command: None,
        },
    )
}

/// One visible tree row: the button run (the indented, marker-prefixed
/// label, plus the trailing muted status text when declared).
/// `enabled == false` renders muted WITHOUT a press route; the press routes
/// per [`tree_element`]'s documented map.
fn tree_row_widget<'a>(
    surface_id: StableId,
    widget_id: StableId,
    tree_row: &TreeRowDef,
    depth: usize,
    enabled: bool,
    selected: bool,
    expanded_now: bool,
) -> Element<'a, WidgetMountMsg> {
    let indent = "  ".repeat(depth.min(MAX_ROW_INDENT));
    let marker = if tree_row.expandable {
        if expanded_now { "- " } else { "+ " }
    } else {
        ""
    };
    let label_text = format!("{indent}{marker}{}", tree_row.label);
    let mut press = button(text(label_text).size(MOUNT_TEXT_SIZE))
        .padding([3, 6])
        .style(button_style(selected));
    if enabled {
        press = if tree_row.expandable {
            press.on_press(WidgetMountMsg::RowExpandedToggled {
                surface_id,
                widget_id,
                row_id: tree_row.id,
                expanded: !expanded_now,
            })
        } else {
            press.on_press(WidgetMountMsg::SelectionChanged {
                surface_id,
                widget_id,
                selected: vec![tree_row.id],
            })
        };
    }
    match &tree_row.status {
        Some(status) => {
            // OWNED String (iced's Text borrows `&str` fragments; the owned
            // form keeps the element self-contained — the 'static element).
            let status_text = text(status.clone())
                .size(MOUNT_HINT_TEXT_SIZE)
                .style(muted_text_style);
            row![press, status_text]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .into()
        }
        None => press.into(),
    }
}

/// Empty-size placeholder the builders fall back to when the state has no
/// synced read model yet (a registration defect — never an `Option`
/// return): a zero-height spacer + non-clickable facts.
fn empty_control(
    widget_id: StableId,
    a11y: &str,
) -> (Element<'static, WidgetMountMsg>, MountFacts) {
    (
        iced::widget::Space::new().width(Fill).height(0).into(),
        MountFacts {
            widget_id,
            clickable: false,
            a11y: a11y.to_string(),
            command: None,
        },
    )
}

/// The muted status/placeholder text style (the panel's muted convention).
fn muted_text_style(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(muted_text(theme)),
    }
}

/// The window's `(depth, &TreeRowDef)` pairs in paint order — the ONLY def
/// walk the mount does per view (one O(def rows) scan, O(window) copies;
/// nothing is borrowed past the builder call).
fn windowed_tree_rows<'d>(
    def: &'d WidgetTreeDef,
    flat: &[FlatRow],
    window: &VisibleWindow,
) -> Vec<(usize, &'d TreeRowDef)> {
    let mut pairs = Vec::with_capacity(window.len());
    if window.end <= window.start {
        return pairs;
    }
    let wanted: HashSet<StableId> = flat[window.start..window.end]
        .iter()
        .map(|r| r.id)
        .collect();
    let mut by_id: HashMap<StableId, &TreeRowDef> = HashMap::new();
    for tree_row in &def.rows {
        if wanted.contains(&tree_row.id) {
            by_id.insert(tree_row.id, tree_row);
        }
    }
    for slot in &flat[window.start..window.end] {
        if let Some(tree_row) = by_id.get(&slot.id) {
            pairs.push((slot.depth, *tree_row));
        }
    }
    pairs
}

/// Build the native LIST for one [`ItemListDef`] over `state` — the same
/// visible-window discipline as [`tree_element`], with the documented v1
/// press map:
/// - a press on an UNSELECTED item → [`WidgetMountMsg::SelectionChanged`]
///   with that single item (the selection click);
/// - a press on an ALREADY-SELECTED item → [`WidgetMountMsg::RowActivated`]
///   (the native double-click substitute).
///
/// The route is chosen at BUILD time from the state's view; iced rebuilds
/// views per state change, so it stays fresh — and BOTH routes consult the
/// state machine again at apply time (the state layer stays the authority).
/// Invisible items are SKIPPED inside the window (per-item visibility buys
/// one fewer visible row, not a hollow slot — lists fold `visible` into the
/// synced view only as enablement, so the mount consults the def here).
pub(crate) fn list_element(
    widget: &ItemListDef,
    state: &WidgetUiState,
    spec: &WindowSpec,
) -> (Element<'static, WidgetMountMsg>, MountFacts) {
    let surface_id = state.surface_id();
    let a11y = widget.name.clone().unwrap_or_else(|| "list".to_string());
    let Some(synced) = state.synced() else {
        return empty_control(widget.id, &a11y);
    };

    let flat = state.flat_rows_of_list(widget);
    let window = visible_window(&flat, state.scroll_row(), spec);

    let mut col = column![].spacing(MOUNT_ROW_SPACING).width(Fill);
    for (index, slot) in flat[window.start..window.end].iter().enumerate() {
        let Some(item) = widget.items.get(window.start + index) else {
            continue;
        };
        if !item.visible {
            continue;
        }
        let enabled = synced.views.get(&slot.id).is_some_and(|view| view.enabled);
        let selected = state.selection().contains(&slot.id);
        // OWNED label (the window copies what it materializes).
        let mut press = button(text(item.label.clone()).size(MOUNT_TEXT_SIZE))
            .padding([3, 6])
            .style(button_style(selected));
        if enabled {
            press = if selected {
                press.on_press(WidgetMountMsg::RowActivated {
                    surface_id,
                    widget_id: widget.id,
                    row_id: slot.id,
                })
            } else {
                press.on_press(WidgetMountMsg::SelectionChanged {
                    surface_id,
                    widget_id: widget.id,
                    selected: vec![slot.id],
                })
            };
        }
        col = col.push(press);
    }

    (
        col.into(),
        MountFacts {
            widget_id: widget.id,
            clickable: synced
                .widget_views
                .get(&widget.id)
                .is_some_and(|view| view.enabled),
            a11y,
            command: None,
        },
    )
}

/// Build the native icon button for one [`IconActionDef`]: the projection
/// ([`project_button`]) supplies label/tooltip/active/command; the
/// [`WidgetDispatcher`]'s synced read model resolves effective
/// enablement/visibility (an unknown address is NOT clickable — "unknown
/// ids are not enabled"). Tooltip via iced's native tooltip widget when
/// declared; the latch state (`active`) styles through the accent half of
/// the standard button style. Icons: see the module docs (the uri→handle
/// cache is integration work; the glyph slot stays reserved in v1).
pub(crate) fn icon_button_element(
    def: &IconActionDef,
    dispatcher: &WidgetDispatcher,
) -> (Element<'static, WidgetMountMsg>, MountFacts) {
    let semantics = project_button(def);
    let ControlSemantics::Icon {
        id,
        label,
        tooltip: tooltip_text,
        enabled,
        visible,
        active,
        command,
        ..
    } = semantics
    else {
        // `project_button` yields a `ControlSemantics::Icon` by construction.
        unreachable!("an icon button projects Icon semantics");
    };
    // The dispatcher's synced read model — the §16 semantic authority —
    // decides the route at effective level: projection enablement AND the
    // synced view agree, or nothing is clickable.
    let clickable = enabled
        && visible
        && dispatcher
            .effective_enabled(&DispatchKey::Button(id))
            .unwrap_or(false)
        && dispatcher
            .effective_visible(&DispatchKey::Button(id))
            .unwrap_or(false);

    let mut press = button(text(label.clone()).size(MOUNT_TEXT_SIZE))
        .padding([3, 6])
        .style(button_style(active));
    if clickable {
        press = press.on_press(WidgetMountMsg::ButtonClicked {
            surface_id: dispatcher.surface_id(),
            widget_id: id,
        });
    }
    let element = match tooltip_text {
        Some(tooltip_text) => {
            let hint: Element<'static, WidgetMountMsg> =
                text(tooltip_text).size(MOUNT_HINT_TEXT_SIZE).into();
            Tooltip::new(press, hint, TooltipPosition::Top).into()
        }
        None => press.into(),
    };

    (
        element,
        MountFacts {
            widget_id: id,
            clickable,
            a11y: label,
            command: Some(command),
        },
    )
}

/// Build the native MENU for one [`MenuDef`]: iced core has no first-class
/// menu at this rev — the vendored `iced_aw` menu (the statusbar's native
/// drop-down idiom) is the repo's native menu: a labeled root whose press
/// opens the overlay menu; items are command-addressed buttons
/// ([`WidgetMountMsg::MenuItemClicked`]); submenus recurse natively
/// ([`Item::with_menu`]), bounded by [`MAX_MENU_DEPTH`] upstream sync.
/// Disabled items render WITHOUT a press route; invisible items don't
/// render at all. The context-menu (right-click) form is deferred (see the
/// module docs).
pub(crate) fn menu_element(
    def: &MenuDef,
    dispatcher: &WidgetDispatcher,
) -> (Element<'static, WidgetMountMsg>, MountFacts) {
    let semantics = project_menu(def);
    let (id, items, label) = match &semantics {
        ControlSemantics::Menu { id, items, .. } => (
            *id,
            items.as_slice(),
            def.label.clone().unwrap_or_else(|| "menu".to_string()),
        ),
        ControlSemantics::Icon { .. } => {
            unreachable!("project_menu yields a ControlSemantics::Menu");
        }
    };
    let root = button(text(label.clone()).size(MOUNT_TEXT_SIZE))
        .padding([3, 6])
        .style(button_style(false));
    let menu = Menu::new(menu_items(items, dispatcher.surface_id(), id, 1))
        .width(Length::Fixed(MOUNT_MENU_WIDTH))
        .close_on_background_click(true);
    let element = MenuBar::new(vec![Item::with_menu(root, menu)])
        .close_on_background_click_global(true)
        .into();

    (
        element,
        MountFacts {
            widget_id: id,
            // The ROOT opens the native menu; each ITEM is machine-gated at
            // apply time. Root clickability is the def-level fold.
            clickable: def.enabled && def.visible,
            a11y: label,
            command: None,
        },
    )
}

/// One menu level's native items (recursive; `owning_menu_id` is the menu
/// whose items these are — a submenu's OWN id once recursed, per the
/// dispatcher's addressing convention).
fn menu_items<'a>(
    items: &[ItemSemantics],
    surface_id: StableId,
    owning_menu_id: StableId,
    depth: usize,
) -> Vec<MountItem<'a>> {
    let mut native_items = Vec::with_capacity(items.len());
    for item in items {
        if !item.visible {
            continue;
        }
        let mut press = button(text(item.label.clone()).size(MOUNT_TEXT_SIZE))
            .width(Fill)
            .padding([3, 6]);
        if item.enabled {
            press = press.on_press(WidgetMountMsg::MenuItemClicked {
                surface_id,
                menu_id: owning_menu_id,
                command_id: item.command.clone(),
            });
        }
        match item.submenu.as_deref() {
            Some(ControlSemantics::Menu { id, items }) if depth < MAX_MENU_DEPTH => {
                let sub = MountMenu::new(menu_items(items, surface_id, *id, depth + 1))
                    .width(Length::Fixed(MOUNT_MENU_WIDTH));
                native_items.push(Item::with_menu(press, sub).close_on_click(false));
            }
            _ => native_items.push(Item::new(press).close_on_click(true)),
        }
    }
    native_items
}

/// Build the TABLE placeholder for one [`TableRef`]: a labeled, disabled
/// container noting that the typed-table registry (SWG-004) — which runs in
/// PARALLEL to this task — is not mounted yet. The placeholder is total and
/// never interactive; the labeled note is the documented stand-in until
/// SWG-004's registry lands (`TableRef::table_id` names the backing table
/// in the SWG-004 contract, foreign to the surface id namespace). The mount
/// deliberately does NOT need SWG-004's registry to build anything.
pub(crate) fn table_placeholder_element(
    table: &TableRef,
) -> (Element<'static, WidgetMountMsg>, MountFacts) {
    let name = table.name.clone().unwrap_or_else(|| "table".to_string());
    let note = format!(
        "table {} — typed-table registry (SWG-004) not mounted yet",
        table.table_id.0
    );
    let element = container(
        column![
            // The name is cloned (owned text; the a11y fact keeps the name).
            text(name.clone()).size(MOUNT_TEXT_SIZE),
            text(note)
                .size(MOUNT_HINT_TEXT_SIZE)
                .style(muted_text_style),
        ]
        .spacing(MOUNT_ROW_SPACING),
    )
    .padding([3, 6])
    .width(Fill)
    .into();

    (
        element,
        MountFacts {
            widget_id: table.id,
            clickable: false,
            a11y: name,
            command: None,
        },
    )
}

/// Stack ALREADY-BUILT children per the group's [`Axis`]. A two-child group
/// with a `split_ratio` splits at the CLAMPED ratio via portion fills
/// ([`split_portions`]); any other group stacks plainly. Total: an empty
/// children list renders an empty stack (a visible `Group` placeholder is
/// exactly where reference layouts reserve the scene cell).
pub(crate) fn group_element<'a>(
    group: &WidgetGroup,
    children_built: Vec<Element<'a, WidgetMountMsg>>,
) -> Element<'a, WidgetMountMsg> {
    let mut children = children_built;
    if let Some((left, right)) = split_portions(group) {
        if children.len() == 2 {
            let second = children.pop().expect("len == 2 just checked");
            let first = children.pop().expect("len == 2 just checked");
            return match group.direction {
                Axis::Horizontal => {
                    let mut split = row![].spacing(0);
                    split = split.push(split_child(Axis::Horizontal, first, left));
                    split = split.push(split_child(Axis::Horizontal, second, right));
                    split.into()
                }
                Axis::Vertical => {
                    let mut split = column![].spacing(0);
                    split = split.push(split_child(Axis::Vertical, first, left));
                    split = split.push(split_child(Axis::Vertical, second, right));
                    split.into()
                }
            };
        }
        // A ratio on a non-2-child group: ignored — plain stacking (see the
        // module docs: the ratio names a TWO-pane split).
    }
    match group.direction {
        Axis::Horizontal => {
            let mut stack = row![].spacing(MOUNT_GROUP_SPACING);
            for child in children {
                stack = stack.push(child);
            }
            stack.into()
        }
        Axis::Vertical => {
            let mut stack = column![].spacing(MOUNT_GROUP_SPACING);
            for child in children {
                stack = stack.push(child);
            }
            stack.into()
        }
    }
}

/// Wrap one child of a split at `portion` on the split axis (the OTHER
/// axis stays as-built).
fn split_child<'a>(
    direction: Axis,
    child: Element<'a, WidgetMountMsg>,
    portion: u16,
) -> Element<'a, WidgetMountMsg> {
    match direction {
        Axis::Horizontal => container(child).width(Length::FillPortion(portion)).into(),
        Axis::Vertical => container(child).height(Length::FillPortion(portion)).into(),
    }
}

/// Clamp a split ratio into the sane window: non-finite → `0.5` (the neutral
/// split), otherwise bounded into `[0.01, 0.99]` (both panes keep >1% —
/// the host-side clamp the contract's note describes; see the module docs).
pub(crate) fn clamp_split_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(0.01, 0.99)
    } else {
        0.5
    }
}

/// The two-pane portions for a group: `None` when the group is not a
/// 2-child group with a declared ratio (plain stacking — see
/// [`group_element`]); otherwise the clamped ratio at [`SPLIT_RESOLUTION`]
/// granularity (`left + right == 1000`).
pub(crate) fn split_portions(group: &WidgetGroup) -> Option<(u16, u16)> {
    let ratio = group.split_ratio?;
    if group.children.len() != 2 {
        return None;
    }
    let ratio = clamp_split_ratio(ratio);
    let left = (ratio * SPLIT_RESOLUTION as f32).round() as u16;
    Some((left, SPLIT_RESOLUTION - left))
}

// ---------------------------------------------------------------------------
// Surface walk (the mount body over one registered surface)
// ---------------------------------------------------------------------------

/// Build ONE widget node; `None` marks a node SKIPPED from its parent
/// container (visibility=false — the documented contract; the parent stacks
/// only rendered children). Visible groups recurse through
/// [`visible_children`].
pub(crate) fn build_node<'a>(
    node: &WidgetNode,
    state: &WidgetUiState,
    dispatcher: &WidgetDispatcher,
    spec: &WindowSpec,
) -> Option<Element<'a, WidgetMountMsg>> {
    if !node_visible(node) {
        return None;
    }
    Some(match node {
        WidgetNode::Tree(def) => tree_element(def, state, spec).0,
        WidgetNode::List(def) => list_element(def, state, spec).0,
        WidgetNode::IconButton(def) => icon_button_element(def, dispatcher).0,
        WidgetNode::Menu(def) => menu_element(def, dispatcher).0,
        WidgetNode::Table(def) => table_placeholder_element(def).0,
        WidgetNode::Group(def) => {
            let children = visible_children(def, state, dispatcher, spec);
            group_element(def, children)
        }
    })
}

/// A group's RENDERED children (visible-only); the skip rule's test surface.
pub(crate) fn visible_children<'a>(
    def: &WidgetGroup,
    state: &WidgetUiState,
    dispatcher: &WidgetDispatcher,
    spec: &WindowSpec,
) -> Vec<Element<'a, WidgetMountMsg>> {
    def.children
        .iter()
        .filter_map(|child| build_node(child, state, dispatcher, spec))
        .collect()
}

/// A node's declared visibility (def-level; uniform across variants).
fn node_visible(node: &WidgetNode) -> bool {
    match node {
        WidgetNode::Tree(def) => def.visible,
        WidgetNode::List(def) => def.visible,
        WidgetNode::IconButton(def) => def.visible,
        WidgetNode::Menu(def) => def.visible,
        WidgetNode::Table(def) => def.visible,
        WidgetNode::Group(def) => def.visible,
    }
}

/// The VISIBLE-WINDOW row count the tree mount materializes — the §16
/// windowing proof helper (the list sibling of
/// `mount_materializes_visible_window_only`): reads the SAME
/// `flat_rows_of_tree` + `visible_window` math the builder uses, so the
/// proof pins the builder's own windowing, not a parallel implementation.
// The §16 windowing proofs read these off the builders' own math — test
// surface only for now.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn materialized_row_count_of_tree(
    def: &WidgetTreeDef,
    state: &WidgetUiState,
    spec: &WindowSpec,
) -> usize {
    let Some(synced) = state.synced() else {
        return 0;
    };
    let flat = state.flat_rows_of_tree(def, &synced.views);
    visible_window(&flat, state.scroll_row(), spec).len()
}

/// The VISIBLE-WINDOW row count the list mount materializes (see
/// [`materialized_row_count_of_tree`]).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn materialized_row_count_of_list(
    def: &ItemListDef,
    state: &WidgetUiState,
    spec: &WindowSpec,
) -> usize {
    let Some(_synced) = state.synced() else {
        return 0;
    };
    let flat = state.flat_rows_of_list(def);
    visible_window(&flat, state.scroll_row(), spec).len()
}

// ---------------------------------------------------------------------------
// MountRegistry — the panel-id keyed side map (the owned seam)
// ---------------------------------------------------------------------------

/// The ownership double one mounted panel carries: the surface the mount
/// renders (verbatim) plus the state machines the interactions route into.
///
/// The panel's `view()` only BORROWS through the registry read; the
/// machines are mutated (bridge-side, SCN-014) through the SAME `Rc`s.
// Bench seam, SCN-024; ADR-007 item 3; never wire — inspect/drive-only widening so the
// `widget_interaction` bench (a separate crate) constructs the handle the REAL registry
// registers. The pub(crate)-typed machine fields (`state`/`dispatcher`) stay private to
// the crate's own drive paths; the bench borrows THROUGH them (type never named
// cross-crate), hence the targeted `private_interfaces` allowance on THIS seam only.
// Not part of the host's documentary API; NOT a wire change.
#[doc(hidden)]
#[allow(private_interfaces)]
#[derive(Clone)]
pub struct SurfaceMountHandle {
    /// The surface verbatim (patch application replaces the registry entry —
    /// the documented re-register flow for SCN-014's patch plumbing).
    pub surface: WidgetSurface,
    /// The SWG-002 interaction state.
    pub state: Rc<RefCell<WidgetUiState>>,
    /// The SWG-003 command dispatch state.
    pub dispatcher: Rc<RefCell<WidgetDispatcher>>,
}

impl SurfaceMountHandle {
    /// Build a handle; the state opens for `surface_id` + syncs (the
    /// machines' registered starting point — the dispatcher's command read
    /// model syncs separately, when the owner plugin publishes its command
    /// defs). The §16 "closing a tab cancels pending requests and releases
    /// state" teardown is [`MountRegistry::teardown_surface_owned`] (SCN-014).
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the SurfaceMountHandle construction
    // path the bench's registration drives.
    #[doc(hidden)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_surface(surface_id: StableId, surface: WidgetSurface) -> Self {
        let mut state = WidgetUiState::new(surface_id);
        state.sync(&surface);
        SurfaceMountHandle {
            state: Rc::new(RefCell::new(state)),
            dispatcher: Rc::new(RefCell::new(WidgetDispatcher::new(surface_id))),
            surface,
        }
    }

    /// The surface the handle's machines command.
    pub(crate) fn surface_id(&self) -> StableId {
        self.state.borrow().surface_id()
    }

    /// The mounted panel body: native widgets over every VISIBLE root node,
    /// stacked with the panel column spacing, in the same padded container
    /// the legacy body uses. `Element<'static>` on purpose: everything the
    /// builders copy (labels, ids, flags) is owned by the element, so the
    /// panel's `view()` borrows nothing that outlives the registry read.
    fn body(&self) -> Element<'static, WidgetMountMsg> {
        let state = self.state.borrow();
        let dispatcher = self.dispatcher.borrow();
        let mut col = column![].spacing(MOUNT_GROUP_SPACING).width(Fill);
        for node in &self.surface.roots {
            if let Some(element) = build_node(node, &state, &dispatcher, &DEFAULT_WINDOW_SPEC) {
                col = col.push(element);
            }
        }
        container(col).padding([8, 10]).width(Fill).into()
    }
}

/// The mount registry: the PANEL-ID keyed side store the panel hook consults.
///
/// APP-OWNED since SCN-016 (`OpenCADStudio::mount_registry`): the update
/// arms hold `self` and the app's view composition passes the store's
/// reference down (the dock → panel `view()` call), so every PRODUCTION
/// path reads/writes it through the app field. The earlier thread-local
/// moved under `#[cfg(test)]` as the HEADLESS TESTS' seam (the seam block
/// near the file end: the tests' static facades keep their exact names and
/// delegate to the SAME instance methods there). The registration LIFECYCLE
/// ([`MountRegistry::register_surface_owned`] /
/// [`MountRegistry::teardown_surface_owned`] + the [`RegisteredGuard`]
/// test seam) is SCN-014's, semantics untouched; still NO HOST flow
/// registers a surface (the real registration calls arrive with the wire
/// era) — which is exactly why every legacy panel stays byte-identical
/// today (zero entries = the legacy `view()` path), by construction.
// Bench seam, SCN-024; ADR-007 item 3; never wire — inspect/drive-only widening so the
// `widget_interaction` bench (a separate crate) constructs and drives the REAL app-owned
// registry instance and reads its real `MountCounters`. Not part of the host's documentary
// API; NOT a wire change.
#[doc(hidden)]
#[derive(Default)]
pub struct MountRegistry {
    by_panel: HashMap<String, SurfaceMountHandle>,
    /// The mount's host-local INPUT store (SCN-020): per-surface
    /// [`HoverState`] (hover + pointer capture + the owed-move slot), the
    /// keyboard target resolution and the armed-prompt cancel hint.
    /// Registry-adjacent ON PURPOSE: one borrow domain with the registry
    /// makes the input entries' lifecycle EXACTLY the surfaces' (every
    /// register/teardown path prunes them — no orphaned input state), and
    /// §7/§14 keep this whole store host-local (never serialized, never
    /// machine-visible).
    input: MountInputState,
    /// The lifecycle instrumentation (§16 probes) — read via
    /// [`MountRegistry::counters_owned`]. SCN-020 added the interaction probes
    /// ([`MountCounters::mount_move_calls`] and siblings).
    counters: MountCounters,
    /// (SCN-015) The armed confirmation prompts' surface-ready descs — the
    /// [`MountPromptDesc`] the app's confirm modal displays, ONE per surface
    /// (the surface's latest armed interaction; a re-arm replaces). Lives in
    /// the registry so its lifetime is exactly the armed interaction's (the
    /// same lifecycle rule as the input store): recorded on the
    /// `PromptRequested` arm ([`MountRegistry::mount_record_prompt_armed_owned`]),
    /// consumed on BOTH completion edges (the modal's confirm/dismiss
    /// decision and Escape's machine-direct cancel) and swept with the
    /// surfaces. Host-local only, never serialized (§3 transient class).
    prompts: HashMap<StableId, MountPromptDesc>,
    /// (SCN-025) The app-owned PANEL SURFACE STORE — the v23
    /// publish-or-patch contract's host-side truth (keyed by panel id,
    /// recording the owning plugin's transport id), the store the wire
    /// attach publishes into, patches over, and pages. Extension of THIS
    /// registry (vs a sibling app-`Rc` field) is the recorded decision: the
    /// registry is already the app-`Rc` domain
    /// (`OpenCADStudio::mount_registry`), `HostSession` reaches it exactly
    /// the way the update arms do (`self.app.mount_registry`), and its
    /// lifecycle has nothing that outlives the app session. NOT a
    /// thread-local — the pre-SCN-016 world stays buried; every write rides
    /// the same one-borrow discipline as the mounts. `by_panel` entries are
    /// created ONLY by the attach (a legacy session stays zero-entry, so
    /// every legacy panel keeps the byte-identical legacy path — the
    /// legacy-behavior guarantee now holds for this store BY CONSTRUCTION).
    #[cfg(not(target_arch = "wasm32"))]
    panel_surfaces: PanelSurfaceStore,
}

impl MountRegistry {
    /// Register the mounted surface for `panel_id` (the RAW primitive — a
    /// re-register replaces the prior entry untouched, no machine release,
    /// no counters; the LIFECYCLE registration is [`Self::register_surface_owned`].
    /// The SCN-013 tests drive this raw form deliberately).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn register_owned(&mut self, panel_id: &str, handle: SurfaceMountHandle) {
        self.by_panel.insert(panel_id.to_string(), handle);
        self.prune_input_entries();
    }

    /// Remove a panel's mount (the RAW primitive — no machine release, the
    /// lifecycle teardown is [`Self::teardown_surface_owned`]).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn unregister_owned(&mut self, panel_id: &str) {
        self.by_panel.remove(panel_id);
        self.prune_input_entries();
    }

    /// The gate read: is a widget-surface mount registered for `panel_id`?
    // (the tests' gate probe ([`mounts_surface`]) consumes this; no
    // production caller until the wire-era registration path lands.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn mount_registered_owned(&self, panel_id: &str) -> bool {
        self.by_panel.contains_key(panel_id)
    }

    /// The mounted body for `panel_id` (see [`SurfaceMountHandle::body`]);
    /// `None` = no registered mount → the caller renders its legacy view.
    pub(crate) fn mounted_body_owned(
        &self,
        panel_id: &str,
    ) -> Option<Element<'static, WidgetMountMsg>> {
        self.by_panel.get(panel_id).map(|handle| handle.body())
    }
}

/// The panel gate READ (`plugin_panel.rs` consults the app-owned registry
/// through the direct lookup; the state-shaped signature is the
/// tests'/reviewers' handle on THE SAME decision). `false` for every legacy
/// state — the legacy-behavior guarantee, stated in one line.
// The gate hook itself runs inside the view pass; this wrapper is the
// assertion surface (the headless tests' facade over the app-owned form —
// see the cfg(test) seam block near the file end).
#[cfg(test)]
pub(crate) fn mounts_surface(state: &PluginPanelState) -> bool {
    with_test_registry(|registry| registry.mount_registered_owned(&state.def.id))
}

// ---------------------------------------------------------------------------
// SCN-014 — the bridge: Message mapping, registration lifecycle, sink
// ---------------------------------------------------------------------------

/// `WidgetMountMsg` → app `Message` — THE one mapping site. The globe's
/// single gate (`plugin_panel.rs`'s mount hook) maps every builder-emitted
/// message through this, and the app's `update` arm unwraps the variant
/// back into the bridge handler (`app::update::widget_mount`).
///
/// Host-local ONLY: `app::Message` carries no serialization derives (no
/// `Serialize`/`Deserialize` on the enum — verified by grep), so the
/// appended tail variant has no wire implication whatsoever.
pub(crate) fn app_message(msg: WidgetMountMsg) -> crate::app::Message {
    crate::app::Message::WidgetMountMsg(msg)
}

/// The sink the bridge forwards events through — the REAL
/// [`WidgetEventSink`] stand-in until the wire era: emitted
/// [`WidgetEvent`]s gather in a shared `Vec` and the bridge drains them
/// (`take`). The plugin-pipeline publishing (`v4_support::publish_*`) is
/// explicitly OUT OF SCOPE until the API-23 crossing (ADR-004): this
/// collecting buffer is the seam the future publisher hangs off (it needs
/// no type change — the drain call becomes the publish call), and the
/// tests pin the contract "the sink receives exactly the forwarded events,
/// in emission order, drained once".
///
/// `Rc<RefCell<..>>` because sharing an emission buffer across the route
/// tail (the settle half) is cheap clone-and-see; the sink never runs
/// across threads (the same UI-thread discipline as the registry).
#[derive(Debug, Clone, Default)]
pub(crate) struct CollectingSink {
    events: Rc<RefCell<Vec<WidgetEvent>>>,
}

impl CollectingSink {
    /// Snapshot of the collected events, in emission order.
    // The assertion tests' read (the bridge itself drains, never peeks).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn collected(&self) -> Vec<WidgetEvent> {
        self.events.borrow().clone()
    }

    /// Drain the collected events (the bridge's consumption point). The
    /// sink stays usable across drains.
    pub(crate) fn take(&mut self) -> Vec<WidgetEvent> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

impl WidgetEventSink for CollectingSink {
    fn emit(&mut self, event: WidgetEvent) {
        self.events.borrow_mut().push(event);
    }
}

/// The bridge lifecycle counters (§16 probes) — what the SCN-014
/// registration/teardown flow did. Read via [`MountRegistry::counters_owned`];
/// the registry is a thread-local, so tests compare DELTAS around their
/// actions (a test thread always starts fresh, but the comparison is also
/// topologically stable if the test pool ever reuses threads).
// Bench seam, SCN-024; ADR-007 item 3; never wire — the real probe counters' type the
// bench reads (mount_move_calls / coalesced_moves_superseded / keyboard_nav_steps).
#[doc(hidden)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MountCounters {
    /// [`MountRegistry::register_surface_owned`] calls (fresh, preserved and
    /// replaced registrations).
    pub registers: usize,
    /// Re-registers that PRESERVED the running machines (the
    /// PRESERVE-ON-IDENTICAL rule — see [`MountRegistry::register_surface_owned`]).
    pub registers_preserved: usize,
    /// Re-registers that REPLACED the machines (the surface shape moved).
    pub registers_replaced: usize,
    /// Completed machine releases — `teardown_surface` AND the release
    /// half of a replacement (every removal flows through the one
    /// `release_mount` path, so the accounting has no second truth).
    pub torn_down: usize,
    /// Mount messages the bridge resolved against a registered surface
    /// (`Message::WidgetMountMsg` arm → routed, whatever the verdict).
    pub messages_routed: usize,
    /// Prompt requests the bridge surfaced (`MountRouting::PromptRequested`
    /// routed) — the §16 tick-count handle for the armed confirmations.
    /// SCN-015: each surfacing lands the surface-ready desc in the
    /// registry's `prompts` map (the modal content); the VISUAL dialog
    /// rides the app's modal machinery (SCN-016 landed:
    /// `ModalKind::WidgetPrompt`).
    pub prompts_surfaced: usize,
    /// (SCN-015) Prompt-dialog decisions CONFIRMED: the pending stage's
    /// captured command emitted through the bridge (forward-then-settle,
    /// the same policy as any command) and the armed desc consumed.
    pub prompts_confirmed: usize,
    /// (SCN-015) Prompt-dialog decisions DISMISSED with no event — the
    /// modal's Cancel AND Escape's machine-direct cancel (both consume the
    /// armed interaction the same way; the machine's own dismissal moves no
    /// counter of its own, so this is the §16 probe for the cancel edge).
    pub prompts_dismissed: usize,
    /// (SCN-015) Dialog decisions routed to NO pending interaction (an
    /// unknown surface, an address that is not the armed one, or a machine
    /// that reports nothing pending anymore). Probe discipline: a refusal
    /// moves ONLY its own counter — here and in
    /// [`MountCounters::prompts_surfaced`]/[`prompts_confirmed`]/[`prompts_dismissed`].
    pub prompts_refused: usize,
    //
    // -- SCN-020 interaction probes (defect instruments, §16) --------------
    /// Mount moves DELIVERED by the coalescer ([`MountRegistry::mount_flush_moves_owned`]
    /// / the release completion): ≤1 per surface per tick. A burst that
    /// moves this per-event (or whose delivered position lags the cursor)
    /// is the defect this probe names.
    pub mount_move_calls: u64,
    /// Owed moves overwritten UNANSWERED — the latest-wins discipline's
    /// count: every position a newer move (or a superseding press) made
    /// pointless to answer.
    pub coalesced_moves_superseded: u64,
    /// Keyboard navigation steps RESOLVED into ≥1 mount route message
    /// ([`key_route`]). The a11y defect probe: a keyboard path that stops
    /// moving the focus/selection is a step this counter never saw.
    pub keyboard_nav_steps: u64,
    //
    // -- SCN-025 wire-attach probes ---------------------------------------
    /// Widget events FORWARDed through the v23 notify router
    /// (`broadcast_panel_scene_events`) from the bridge arms' sink drains —
    /// one tick per forwarded EVENT row. Exactly-once discipline: the drain
    /// (`CollectingSink::take`) empties the buffer, so a re-drain can never
    /// re-forward; this probe is the count a double-forward defect breaks.
    pub mount_event_forwards: u64,
    /// Store DEMANDS forwarded through the v23 demand router
    /// (`broadcast_panel_store_demand`) from the store reads' page misses —
    /// one tick per demand raised (a read-driven fact; a re-read legitimately
    /// re-demands, but never as a replay of a queued ledger).
    pub mount_demand_forwards: u64,
}

/// The PRESERVE-ON-IDENTICAL comparison input: the surface's structural
/// fingerprint (`revision` + root count + per-kind node counts, the same
/// walk discipline as the crate's `WidgetSummary` — one explicit stack, no
/// recursion). Deriving `PartialEq` keeps the registration rule ONE
/// expression; the walk ignores KIND-LEVEL content on purpose — the
/// revision is the plugin's freshness authority (§16), so equal shape at
/// equal revision is an identical-refresh claim, not a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MountSurfaceShape {
    revision: SceneRevision,
    roots: usize,
    trees: usize,
    lists: usize,
    icon_buttons: usize,
    menus: usize,
    tables: usize,
    groups: usize,
}

/// One-walk shape of `surface` (see [`MountSurfaceShape`]).
fn surface_shape(surface: &WidgetSurface) -> MountSurfaceShape {
    let mut shape = MountSurfaceShape {
        revision: surface.revision,
        roots: surface.roots.len(),
        trees: 0,
        lists: 0,
        icon_buttons: 0,
        menus: 0,
        tables: 0,
        groups: 0,
    };
    let mut walker: Vec<&WidgetNode> = surface.roots.iter().collect();
    while let Some(node) = walker.pop() {
        match node {
            WidgetNode::Tree(_) => shape.trees += 1,
            WidgetNode::List(_) => shape.lists += 1,
            WidgetNode::IconButton(_) => shape.icon_buttons += 1,
            WidgetNode::Menu(_) => shape.menus += 1,
            WidgetNode::Table(_) => shape.tables += 1,
            WidgetNode::Group(group) => {
                shape.groups += 1;
                walker.extend(group.children.iter());
            }
        }
    }
    shape
}

/// THE machine-release path: every surface removal flows through here
/// (teardown AND replacement) — the state's `pending_children` drop (§16
/// "closing a tab cancels pending requests and releases state"), the
/// dispatcher's read model + armed prompts + in-flight marks drop. One
/// path, one counter ([`MountCounters::torn_down`]).
///
/// The wire-era gap, RECORDED not claimed: this cancels the MOUNT's view
/// of pending lazy page fetches; genuinely cancelling in-flight IPC fetches
/// needs the request-id vocabulary of the ADR-004 crossing (SCN-015).
fn release_mount(handle: &SurfaceMountHandle) {
    handle.state.borrow_mut().reset_all();
    handle.dispatcher.borrow_mut().reset_all();
}

/// The headless test/bench registration seam: build the mount from an
/// owned `surface`, register it under `panel_id` ([`MountRegistry::register_surface_owned`]
/// — the PRESERVE-ON-IDENTICAL lifecycle), and return a guard whose drop
/// tears the surface down (RAII — a test scope cannot leak a registration
/// into its thread neighbors).
///
// The seam the headless tests call (the cfg(test) seam block below backs
// it with the cfg(test)-only registry cell); production registration is
// the wire era's.
///
/// The mount state opens under the surface's FIRST root's id (every real
/// surface roots at least one node; an empty surface falls back to
/// `StableId(0)`): messages address a surface by that id, so a test routes
/// with the same id it rooted. (App-owned registration, SCN-016, names the
/// id explicitly; this helper is the tests' seam over the SAME lifecycle
/// the app path runs — "the guard's register call moves into the app
/// update, keeping this signature".)
#[cfg(test)]
pub(crate) fn register_test_surface(panel_id: &str, surface: WidgetSurface) -> RegisteredGuard {
    let surface_id = surface
        .roots
        .first()
        .map(|node| node.widget_id())
        .unwrap_or(StableId(0));
    MountRegistry::register_surface(
        panel_id,
        SurfaceMountHandle::from_surface(surface_id, surface),
    );
    RegisteredGuard {
        panel_id: panel_id.to_string(),
    }
}

impl MountRegistry {
    /// The lifecycle registration (SCN-014): mount `handle` for `panel_id`
    /// on the PRESERVE-ON-IDENTICAL rule (SCN-014 decision, REG-1 spirit):
    ///
    /// - NO prior surface for the panel → register fresh.
    /// - A prior surface whose [`surface_shape`] EQUALS the new one (equal
    ///   `revision` + root count + per-kind node counts) → KEEP the
    ///   existing handle: the running machines, their interaction state
    ///   and their counters survive (an identical-resurface claim is a
    ///   refresh, not a reset — the caller's fresh-but-identical handle is
    ///   dropped unused).
    /// - Anything else (different revision or shape) → REPLACE: the
    ///   outgoing machines release through [`release_mount`] (pending
    ///   children, armed prompts, in-flight marks — the §16 contract),
    ///   then the new handle takes the slot with its own fresh machines.
    ///
    /// Idempotent per panel id by construction (at most one entry per id;
    /// re-registers move a counter, never duplicate state).
    ///
    /// THE test/bench entry is [`register_test_surface`]; the app-owned
    /// registration path (SCN-016) calls
    /// [`Self::register_surface_owned`] over the app's own registry — the
    /// production registration CALLS still arrive with the wire era.
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the bench's registration drive.
    #[doc(hidden)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn register_surface_owned(&mut self, panel_id: &str, handle: SurfaceMountHandle) {
        let identical = self
            .by_panel
            .get(panel_id)
            .is_some_and(|existing| {
                surface_shape(&existing.surface) == surface_shape(&handle.surface)
            });
        self.counters.registers += 1;
        if identical {
            // PRESERVE-ON-IDENTICAL: the registered handle stays; the
            // caller's fresh machines are simply dropped (byte-cheap:
            // the unused surface clone dies with the handle).
            self.counters.registers_preserved += 1;
        } else {
            if let Some(previous) = self.by_panel.remove(panel_id) {
                release_mount(&previous);
                // The replaced surface's armed desc dies with the
                // surface (the §16 lifecycle: a replacement releases
                // the machines; the desc is registry-side of the same
                // interaction).
                self.prompts.remove(&previous.surface_id());
                self.counters.registers_replaced += 1;
                self.counters.torn_down += 1;
            }
            self.by_panel.insert(panel_id.to_string(), handle);
        }
        // The input store is the surfaces' mirror: sweep it after ANY
        // registration path (a replaced surface's stale hover/capture/
        // owed-move state has nothing to point at anymore).
        self.prune_input_entries();
    }

    /// The explicit teardown (§16 "closing a tab cancels pending requests
    /// and releases state"): the machines release ([`release_mount`] — the
    /// state's `pending_children` drop, the dispatcher's armed prompts/
    /// in-flight marks and read model reset) and the registry entry
    /// leaves. Idempotent: an unknown/already-torn-down panel id is a
    /// graceful no-op (no release runs, no counter moves).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn teardown_surface_owned(&mut self, panel_id: &str) {
        if let Some(handle) = self.by_panel.remove(panel_id) {
            release_mount(&handle);
            // The surface's armed desc dies with the surface (same
            // lifecycle rule as the input store's sweep — the §16
            // teardown contract).
            self.prompts.remove(&handle.surface_id());
            self.counters.torn_down += 1;
        }
        self.prune_input_entries();
    }

    /// The bridge's lookup: the registered mount whose state owns
    /// `surface_id` — messages carry the surface id, the registry keys by
    /// panel id, and this is the one place the two vocabularies meet (a
    /// linear scan over the registered panels; panel counts are small,
    /// clicks are human-rate).
    ///
    /// SURFACE-ID UNIQUENESS is the registration contract this lookup
    /// exposes (the same contract the wire events make — a
    /// `WidgetEvent::surface_id` names one surface): two registered
    /// surfaces sharing an id would make this lookup order-dependent.
    /// `None` for an unregistered id — the bridge refuses gracefully.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn handle_for_surface_owned(
        &self,
        surface_id: StableId,
    ) -> Option<SurfaceMountHandle> {
        self.by_panel
            .values()
            .find(|handle| handle.surface_id() == surface_id)
            .cloned()
    }

    /// The lifecycle counters' read side (§16 probes; tests/benches).
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the real MountCounters read
    // (evolution deltas) the bench prints.
    #[doc(hidden)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn counters_owned(&self) -> MountCounters {
        self.counters
    }

    /// The bridge's prompt tick-count (a `PromptRequested` routed): the
    /// §16 handle the prompt-modal wiring picks up.
    pub(crate) fn record_prompt_surfaced_owned(&mut self) {
        self.counters.prompts_surfaced += 1;
    }

    /// The bridge's routed-message tick — every
    /// `Message::WidgetMountMsg` the handler resolved against a registered
    /// surface (not the unknown-surface no-ops).
    pub(crate) fn record_message_routed_owned(&mut self) {
        self.counters.messages_routed += 1;
    }

    /// The keyboard layer's nav-step probe tick ([`MountCounters::keyboard_nav_steps`]):
    /// one resolved navigation step (a key whose route produced ≥1 mount
    /// message). Called by [`key_route`]/[`mount_table_key_route`] — the
    /// pure keyboard layers' single registry-side side effect.
    /// (registry-side; test-consumed probes + the keyboard hookup of SCN-016)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn record_key_nav_step_owned(&mut self) {
        self.counters.keyboard_nav_steps += 1;
    }

    // -- SCN-025: the panel surface store (the wire attach) -----------------

    /// The store read (the attach's page/snapshot reads + the tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn panel_surface_store(&self) -> &PanelSurfaceStore {
        &self.panel_surfaces
    }

    /// The store write (the attach's publish/patch; one-borrow discipline).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn panel_surface_store_mut(&mut self) -> &mut PanelSurfaceStore {
        &mut self.panel_surfaces
    }

    /// The event-forward probe tick ([`MountCounters::mount_event_forwards`])
    /// — one per forwarded EVENT row (the bridge arms' sink drains).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn record_panel_events_forwarded_owned(&mut self, count: usize) {
        self.counters.mount_event_forwards += count as u64;
    }

    /// The demand-forward probe tick ([`MountCounters::mount_demand_forwards`])
    /// — one per demand raised (the store reads' page misses).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn record_store_demand_forwarded_owned(&mut self) {
        self.counters.mount_demand_forwards += 1;
    }

    /// The reverse lookup (SCN-025): the panel a registered `surface_id`
    /// names — the event forward's `panel_id` half (the drained events
    /// carry the surface id; the manager's routers carry both). The mirror
    /// of [`Self::handle_for_surface_owned`]; `None` for an unregistered id.
    pub(crate) fn panel_for_surface_owned(&self, surface_id: StableId) -> Option<String> {
        self.by_panel
            .iter()
            .find(|(_, handle)| handle.surface_id() == surface_id)
            .map(|(panel_id, _)| panel_id.clone())
    }
}

// ---------------------------------------------------------------------------
// SCN-020 — the mount's INPUT layer: hover state, pointer capture, and the
// latest-wins move coalescer (host-local, §7/§14)
// ---------------------------------------------------------------------------

/// ONE mounted surface's pointer/input state — the mount-machine slot §14
/// demands ("the machine layers never see raw moves"). Host-local only,
/// never serialized, never machine-visible; lives in [`MountRegistry`]'s
/// input store so its lifetime is exactly the surface's (every register /
/// teardown path sweeps it).
///
/// The name is [`HoverState`] because hover is its permanent resident; the
/// pointer-capture fields and the owed-move slot live in the same per-
/// surface entry because press→capture→move→release is ONE gesture whose
/// state belongs together (SCN-021's drag previews consume exactly
/// these fields — `hover`, `last_move`, `captured`, `capture_origin` — read
/// through [`MountRegistry::mount_hover_state_owned`]).
// Bench seam, SCN-024; ADR-007 item 3; never wire — the per-surface input snapshot the
// bench reads (last_move defines the delivered newest position). Not part of the host's
// documentary API; NOT a wire change.
#[doc(hidden)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HoverState {
    /// The row/widget under the pointer, per what the wiring last
    /// reported ([`MountRegistry::mount_set_hover_owned`]; a coalesced move
    /// flush does NOT hit-test — the mount has no per-row geometry of its
    /// own, the flush's single mount call records the position only).
    pub hover: Option<StableId>,
    /// The last DELIVERED move's surface-local position (the flush answers
    /// the newest owed move at the tick/release rate, never per event).
    pub last_move: Option<Point>,
    /// Pointer capture: press over the surface → ON; release ANYWHERE →
    /// OFF (the completion contract is [`MountRegistry::mount_pointer_released_owned`]
    /// — the pure fn "release anywhere" asserts against).
    pub captured: bool,
    /// Where the capture's pointer stood when it first reported after a
    /// press (the drag-origin slot; unnamed until the first delivered
    /// move — this iced revision reports no press position).
    pub capture_origin: Option<Point>,
    /// The owed-move slot: the position the pointer last reported that the
    /// tick/flush has not answered yet. LATEST-WINS (§7): a new move
    /// OVERWRITES the slot (the superseded one is counted, never queued);
    /// the tick answers only the last — so at most ONE position is in
    /// flight per surface at any moment; moves never accumulate into a
    /// queue (the queue-could-only-grow defect the viewport coalescer
    /// measured) and nothing but the newest is ever processed.
    move_owed: Option<Point>,
}

/// The registry's input store alongside its surfaces (SCN-020). Plus the
/// two SURFACE-SCOPE slots the keyboard/cancel routes resolve:
/// `key_target` and the armed-prompt cancel hint.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Default)]
pub(crate) struct MountInputState {
    /// Per-surface pointer/input state (`None` key = no entry — input
    /// entries are created lazily by their first event and pruned with
    /// their surface).
    per_surface: HashMap<StableId, HoverState>,
    /// The surface the mount's keys route to (the keyboard's target).
    /// Set when a press captures a surface (a press-keyed panel owns the
    /// keys until another gesture moves them); cleared by the profile
    /// sweep when the surface unregisters. The keyboard-ONLY §16 path
    /// (no click at all) resolves in [`MountRegistry::mount_key_surface_owned`]:
    /// with no explicit target but EXACTLY ONE registered surface, that
    /// surface is the target (the sole mounted panel is the only thing
    /// arrows can mean; the app-owned registry's real roving-panel focus
    /// is the SCN-015 crossings concern).
    key_target: Option<StableId>,
    /// The armed command address the bridge last surfaced for a surface
    /// ([`MountRegistry::mount_record_prompt_armed_owned`]) — the hint Escape's
    /// cancel reads ([`MountRegistry::mount_escape_cancel_owned`]; the dispatcher
    /// stays the semantic authority, the hint merely NAMES the armed
    /// interaction without a key-enumeration read on the frozen
    /// dispatcher).
    escape_cancel: HashMap<StableId, DispatchKey>,
}

impl MountRegistry {
    /// (SCN-020, the coalescing JOIN's entry) Record ONE pointer move for
    /// `surface_id` into the surface's owed slot — the mount sibling of
    /// the viewport coalescer's `defer_pointer_move`:
    ///
    /// - LATEST-WINS (§7): a pending slot is OVERWRITTEN — the newer move
    ///   is, by definition, where the cursor is; the older one is dropped
    ///   unanswered and counted ([`MountCounters::coalesced_moves_superseded`]).
    ///   N moves per tick = ≤1 mount call per surface ([`MountRegistry::mount_flush_moves_owned`]).
    /// - NOTHING routes yet, and NO machine sees the move raw (§14): the
    ///   whole effect is this slot write.
    /// - Unregistered surface → `None` (graceful no-op — nothing stored,
    ///   nothing counted; the through-line of test
    ///   `unknown_surface_moves_are_graceful`), `Some(superseded)` otherwise.
    ///
    /// The app arm calls this per `PointerMoved` message; the app's
    /// move-coalesce tick (the SAME `Message::MoveCoalesceTick` channel the
    /// viewport coalescer drives) calls the flush. Contrary to the
    /// viewport's adaptive gate there is NO pass-cost threshold: the
    /// mount's move half must be CHEAP per event unconditionally (a slot
    /// write only), so it defers every move.
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the real coalescer entry
    // the bench's parked-move storm drives.
    #[doc(hidden)]
    pub fn mount_move_defer_owned(
        &mut self,
        surface_id: StableId,
        position: Point,
    ) -> Option<bool> {
        let handle_registered = self
            .by_panel
            .values()
            .any(|handle| handle.surface_id() == surface_id);
        if !handle_registered {
            return None;
        }
        let entry = self.input.per_surface.entry(surface_id).or_default();
        let superseded = entry.move_owed.replace(position).is_some();
        if superseded {
            self.counters.coalesced_moves_superseded += 1;
        }
        Some(superseded)
    }

    /// Answer every surface's LAST owed move — the coalescer's delivery
    /// point (the tick, and the release completion). This IS the mount
    /// call the counter ticks: per surface, at most one delivery per
    /// flush, updating the surface's [`HoverState`] (host-local §14) and
    /// NOTHING else — no machine route, no event, no counter wobble
    /// beyond the probe. Returns how many surfaces were delivered.
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the real coalescer flush
    // the bench's storm drives.
    #[doc(hidden)]
    pub fn mount_flush_moves_owned(&mut self) -> usize {
        let mut delivered = 0usize;
        for entry in self.input.per_surface.values_mut() {
            if let Some(position) = entry.move_owed.take() {
                entry.last_move = Some(position);
                // The capture's drag ORIGIN is the first delivered move
                // during capture (the press carries no position in this
                // iced revision); SCN-021's drag previews read it.
                if entry.captured && entry.capture_origin.is_none() {
                    entry.capture_origin = Some(position);
                }
                delivered += 1;
            }
        }
        // The probe counts AFTER the iteration (one number: the delivered
        // calls).
        self.counters.mount_move_calls += delivered as u64;
        delivered
    }

    /// Pointer capture ON for `surface_id` (the press route): capture arms
    /// and the surface becomes the keyboard target (a pressed panel owns
    /// the keys — the desktop input model). A still-owed move is
    /// SUPERSEDED by the press (the viewport precedent:
    /// `set_cursor_for_press` drops the withheld position rather than
    /// answering it backwards order — the press is the later truth), so
    /// the press does NOT deliver it and does NOT count it as an overwrite
    /// (the probe counts latest-wins overwrites only). Unregistered →
    /// `false`, registered → `true`.
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the capture arm the bench's
    // drag storm drives.
    #[doc(hidden)]
    pub fn mount_press_owned(&mut self, surface_id: StableId) -> bool {
        let handle_registered = self
            .by_panel
            .values()
            .any(|handle| handle.surface_id() == surface_id);
        if !handle_registered {
            return false;
        }
        let entry = self.input.per_surface.entry(surface_id).or_default();
        entry.captured = true;
        entry.capture_origin = None; // lands with the first delivered move
        let _already_superseded = entry.move_owed.take();
        self.input.key_target = Some(surface_id);
        true
    }

    /// Pointer capture completion — release ANYWHERE (pressed inside,
    /// released wherever): capture OFF for `surface_id`, and the pending
    /// owed move (if any) is DELIVERED NOW — the gesture's last position
    /// must not outlive the gesture unanswered (the §16 shape of the
    /// viewport's press-answers-the-owed-move rule). Returns the delivered
    /// count (0 or 1: ≤1 in flight per surface — the contract the tests
    /// pin). An unknown surface is a quiet 0.
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the completion path the bench's
    // storm drives.
    #[doc(hidden)]
    pub fn mount_pointer_released_owned(&mut self, surface_id: StableId) -> usize {
        let Some(entry) = self.input.per_surface.get_mut(&surface_id) else {
            return 0;
        };
        entry.captured = false;
        entry.capture_origin = None;
        let delivered = entry.move_owed.take().map(|position| {
            entry.last_move = Some(position);
            1
        });
        // (the entry borrow ends here; the probe counts outside it)
        let delivered = delivered.unwrap_or(0);
        self.counters.mount_move_calls += delivered as u64;
        delivered
    }

    /// Is ANY registered surface's capture armed? — the app subscription's
    /// window-level release-catcher gate (SCN-016): the catcher subscribes
    /// exactly while a capture could be waiting for its outside release.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn any_capture_active(&self) -> bool {
        self.input.per_surface.values().any(|entry| entry.captured)
    }

    /// Is there any surface with an owed (unanswered) move? — the app
    /// subscription's move-coalesce tick pump extension (SCN-016): the
    /// EXISTING pump condition widens by this predicate so the mount's
    /// coalescer driver keeps flowing after the last pointer event.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn any_move_owed(&self) -> bool {
        self.input
            .per_surface
            .values()
            .any(|entry| entry.move_owed.is_some())
    }

    /// The surfaces with capture armed, in registry order — the release
    /// catcher's completion route input (each named surface's release runs
    /// the ordinary [`Self::mount_pointer_released_owned`]).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn captured_surfaces(&self) -> Vec<StableId> {
        self.input
            .per_surface
            .iter()
            .filter(|(_, entry)| entry.captured)
            .map(|(surface_id, _)| *surface_id)
            .collect()
    }

    /// Record the wired hover: `row_id` is what the pointer is over (the
    /// per-row hover wiring's input; the move flush deliberately does NOT
    /// hit-test — see [`HoverState::hover`]). Host-local ONLY: no machine
    /// route, no event, no counter beyond nothing — the piece §14 pins
    /// ("hover does not storm the machines"). Returns `false` when the
    /// surface is unknown (a quiet no-op); `true` when recorded.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn mount_set_hover_owned(&mut self, surface_id: StableId, row: StableId) -> bool {
        let handle_registered = self
            .by_panel
            .values()
            .any(|handle| handle.surface_id() == surface_id);
        if !handle_registered {
            return false;
        }
        self.input.per_surface.entry(surface_id).or_default().hover = Some(row);
        true
    }

    /// The hover read (SCN-021's drag previews + the tests' probe): what
    /// surface `surface_id`'s pointer last hovered.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn mount_hover_owned(&self, surface_id: StableId) -> Option<StableId> {
        self.input
            .per_surface
            .get(&surface_id)
            .and_then(|entry| entry.hover)
    }

    /// The full per-surface input snapshot — SCN-021's drag previews read
    /// hover + capture + origin + last position from ONE place (the
    /// registry stays the single truth for all of them).
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the delivered-position read
    // the bench's newest-wins assertions use.
    #[doc(hidden)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn mount_hover_state_owned(&self, surface_id: StableId) -> Option<HoverState> {
        self.input.per_surface.get(&surface_id).copied()
    }

    /// The capture read (the press/release route's assertion surface).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn mount_captured_owned(&self, surface_id: StableId) -> bool {
        self.input
            .per_surface
            .get(&surface_id)
            .is_some_and(|entry| entry.captured)
    }

    /// The panel → surface-id read for the panel hook (the capture wrapper
    /// needs the address its pointer messages carry).
    pub(crate) fn panel_surface_id_owned(&self, panel_id: &str) -> Option<StableId> {
        self.by_panel.get(panel_id).map(|handle| handle.surface_id())
    }

    /// The keyboard target resolution (SCN-020): the surface a mount key
    /// routes to. Decision:
    /// 1. an explicit target (the press-captured surface — clicks move
    ///    keyboard focus like any desktop input stack);
    /// 2. else the SOLE registered surface — the keyboard-only §16 path:
    ///    with exactly one mounted panel and no pointer, arrows/Tab can
    ///    only mean that panel;
    /// 3. else `None` (no mount surface the keys could mean — graceful).
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the keyboard target
    // resolution the bench asserts before its key-step burst.
    #[doc(hidden)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn mount_key_surface_owned(&self) -> Option<StableId> {
        self.input.key_target.or_else(|| {
            let sole: Option<StableId> =
                self.by_panel.values().next().map(|handle| handle.surface_id());
            (self.by_panel.len() == 1).then_some(sole).flatten()
        })
    }

    /// The point-free keyboard-grab seam (for tests + the app-owned
    /// registry's roving-panel focus, the wire era): name `surface_id` as
    /// the keys' target without a pointer press. `None` clears.
    // Bench seam, SCN-024; ADR-007 item 3; never wire — the "set the target surface first"
    // seam the bench's keyboard burst drives.
    #[doc(hidden)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_mount_key_target_owned(&mut self, surface_id: Option<StableId>) {
        self.input.key_target = surface_id;
    }

    /// The bridge's prompt-arm record for one `PromptRequested` routing —
    /// TWO coordinated writes, one lifecycle (the surface's armed
    /// interaction):
    ///
    /// 1. (SCN-020, unchanged) the ESCAPE hint: `key` is the armed address
    ///    Escape's cancel route will dismiss. The dispatcher stays the
    ///    semantic authority; this is the keyboard layer's naming of the
    ///    armed interaction (the frozen dispatcher exposes no key
    ///    enumeration).
    /// 2. (SCN-015) the DESC arm: resolve the surface-ready dialog desc
    ///    ([`MountPromptDesc`]) NOW and place it in the registry's
    ///    `prompts` map — the modal content the app's confirm modal
    ///    displays (SCN-016 landed that wiring: `ModalKind::WidgetPrompt`).
    ///    Resolution facts mirror the
    ///    machine's own arm-time capture (see [`Self::resolve_prompt_desc_owned`]);
    ///    a resolution failure records nothing here (the interaction stays
    ///    machine-pending and its lifecycle continues through the machine
    ///    edges) — a graceful no-op, never a panic.
    ///
    /// A re-arm on the same surface REPLACES the desc (the surface shows
    /// its latest armed interaction; the machine layers stay total — their
    /// stages are keyed per address).
    pub(crate) fn mount_record_prompt_armed_owned(&mut self, surface_id: StableId, key: DispatchKey) {
        self.input.escape_cancel.insert(surface_id, key.clone());
        // The desc's resolution reads the handle's own `RefCell`s (the
        // machines), then the one insert lands it.
        if let Some(desc) = self.resolve_prompt_desc_owned(surface_id, &key) {
            self.prompts.insert(surface_id, desc);
        }
    }

    /// The arm-time desc resolution ([`Self::mount_record_prompt_armed_owned`]'s
    /// second write): machine reads ONLY, in the same synchronous scope as
    /// the arming `dispatch` — the "captured at arm time" rule. `None` when
    /// any fact cannot be named (no machine stage, unregistered surface, no
    /// surface def for a button address) — record nothing, never a panic:
    /// - `prompt`: the machine's captured stage text
    ///   ([`WidgetDispatcher::pending_prompt`] — the same text the confirm
    ///   emission rides, so a concurrent re-sync cannot retarget the
    ///   dialog's message);
    /// - `targets`: the surface's CURRENT selection — exactly what the
    ///   arming dispatch routed ([`apply_mount_msg`]'s command routes);
    /// - `command_id`: the surface def's command for a button address
    ///   ([`surface_button_command`] — the defs the mount renders FROM, the
    ///   same source the dispatched read model synced); the command id the
    ///   address itself carries for a menu item.
    ///
    /// The machine's `PromptStage` (private to the frozen SWG-003 layer)
    /// remains the exchange's semantic authority: `confirm` re-emits from
    /// ITS captured stage. This copy is the DISPLAY contract
    /// (the desc's fields), reconstructed from the same guards `dispatch`
    /// validated, at the same instant.
    fn resolve_prompt_desc_owned(&self, surface_id: StableId, key: &DispatchKey) -> Option<MountPromptDesc> {
        let handle = self.handle_for_surface_owned(surface_id)?;
        let prompt = handle
            .dispatcher
            .borrow()
            .pending_prompt(key)
            .map(|prompt| prompt.to_string())?;
        let command_id = match key {
            // A menu item's address itself carries the command id — the
            // dispatcher captured exactly that. No def walk needed.
            DispatchKey::MenuItem(_, command) => command.clone(),
            DispatchKey::Button(button_id) => {
                surface_button_command(&handle.surface, *button_id)?
            }
        };
        let targets = handle.state.borrow().selection().to_vec();
        Some(MountPromptDesc {
            surface_id,
            key: key.clone(),
            prompt,
            command_id,
            targets,
        })
    }

    /// Escape's mounted-surface cancel (the machine-direct half — Escape
    /// produces NO route message: cancelling is a host control operation,
    /// like blur, not a widget gesture). Dismisses the surface's ARMED
    /// prompt through the dispatcher's own hook
    /// (`WidgetDispatcher::dismiss_prompt` — the frozen state layer's
    /// cancel edge) and consumes the hint. SCN-015: the dismissal consumes
    /// the registry's armed desc TOO (the dialog leaves with the
    /// interaction — the same lifecycle edge the modal's own Cancel runs),
    /// and a REAL dismissal counts the dismissal probe (a silent Escape
    /// with nothing armed moves nothing). Returns `true` when an armed
    /// interaction was cancelled. NO DRAG here: drag cancel is SCN-021's
    /// (its hook slot will join this arm).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn mount_escape_cancel_owned(&mut self, surface_id: StableId) -> bool {
        let Some(key) = self.input.escape_cancel.remove(&surface_id) else {
            return false;
        };
        let Some(handle) = self.handle_for_surface_owned(surface_id) else {
            return false;
        };
        let dismissed = handle.dispatcher.borrow_mut().dismiss_prompt(key.clone());
        // The armed desc leaves in the SAME dismiss (a stale desc with no
        // machine stage is a defect the self-heal takes down quietly — the
        // machine said the interaction is over).
        self.take_prompt_dialog_owned(surface_id, &key);
        if dismissed {
            self.record_prompt_dismissed_owned();
        }
        dismissed
    }

    /// Sweep every input entry whose SURFACE left the registry (the
    /// lifecycle tail both removal paths run — no teardown can orphan a
    /// [`HoverState`], a key target, or an escape hint). One O(input
    /// entries) walk per registry mutation is trivial: a surface count is
    /// human-scale, and mutations are gesture-rate.
    #[cfg_attr(not(test), allow(dead_code))]
    fn prune_input_entries(&mut self) {
        let registered = |surface_id: StableId| {
            self.by_panel
                .values()
                .any(|handle| handle.surface_id() == surface_id)
        };
        self.input
            .per_surface
            .retain(|surface_id, _| registered(*surface_id));
        self.input.escape_cancel.retain(|surface_id, _| registered(*surface_id));
        // (SCN-015) The armed descs are the surfaces' mirror too: a raw
        // register/unregister path (no release runs there) still cannot
        // leave an orphaned desc behind.
        self.prompts.retain(|surface_id, _| registered(*surface_id));
        if self.input.key_target.is_some_and(|target| !registered(target)) {
            self.input.key_target = None;
        }
    }
}

// ---------------------------------------------------------------------------
// SCN-020 — the keyboard layer: the surface ring, key_route, and the
// table sibling (state layers decide — the mount only translates)
// ---------------------------------------------------------------------------

/// The mounted surface's keyboard-nav ring: the (focusable id, OWNING
/// widget id) pairs in PAINT order — trees contribute their current flat
/// rows (the expansion state's visible rows), lists their items, visible
/// icon buttons their own id. THE ring is the same discipline the widgets
/// render in ([`build_node`]'s walk), and the DISABLED skip never lives
/// here: the ring names everything rendered; the machines' roving
/// ([`WidgetUiState::focus_next`]/[`WidgetUiState::focus_prev`]) skip the
/// dead rows — the state layer stays the authority.
///
/// EXCLUDED on purpose (v1, each documented at its site): menus — the
/// iced_aw menubar's root is its own native overlay and the mount's ring
/// does not reach inside it; tables — the typed-table machine
/// ([`mount_table_key_route`]) is the table rows' own keyboard scope until
/// the typed-table mount integration lands; invisible nodes (the
/// [`build_node`] skip — the ring must not name what is not rendered).
pub(crate) fn nav_ring(
    surface: &WidgetSurface,
    state: &WidgetUiState,
) -> Vec<(StableId, StableId)> {
    let Some(synced) = state.synced() else {
        return Vec::new();
    };
    let views = &synced.views;
    let mut ring = Vec::new();
    // Explicit-stack walk in PAINT order (no recursion; the `pop` discipline
    // means every push is reversed — roots and group children alike).
    let mut walker: Vec<&WidgetNode> = surface.roots.iter().rev().collect();
    while let Some(node) = walker.pop() {
        if !node_visible(node) {
            continue;
        }
        if let WidgetNode::Group(def) = node {
            walker.extend(def.children.iter().rev());
        }
        match node {
            WidgetNode::Tree(def) => {
                // The CURRENT expansion state's flat rows (collapsed
                // branches contribute nothing — the ring is what renders).
                for slot in state.flat_rows_of_tree(def, views) {
                    ring.push((slot.id, def.id));
                }
            }
            WidgetNode::List(def) => {
                for slot in state.flat_rows_of_list(def) {
                    ring.push((slot.id, def.id));
                }
            }
            WidgetNode::IconButton(def) => ring.push((def.id, def.id)),
            // Tables route through the table machine; menus stay the
            // native menubar (see the doc); groups only carried the walk.
            WidgetNode::Table(_) | WidgetNode::Menu(_) | WidgetNode::Group(_) => {}
        }
    }
    ring
}

/// Translate ONE keyboard event into the mount's route messages — the
/// keyboard layer's pure heart (SCN-020). The state layers decide
/// everything (enablement, the ring skip, the focus slot); this fn only
/// translates keys into the vocabulary the bridge already routes:
///
/// - ArrowDown / ArrowUp — one ring step through
///   [`WidgetUiState::focus_next`] / [`WidgetUiState::focus_prev`] (the
///   disabled rows are skipped BY the machine — a focus ring on a dead
///   control is a defect the machine refuses). A step that lands produces
///   THE PAIR a keyboard cursor implies: [`WidgetMountMsg::FocusMoved`]
///   plus [`WidgetMountMsg::SelectionChanged`] on the landed row (the
///   arrows are the LIST-cursor semantics: focus and selection move
///   together, replacing the selection — a single id, always cap-sane).
/// - ArrowRight / ArrowLeft — the TREE row's expansion toggle (Right
///   opens a collapsed expandable row, Left closes an open one — the
///   machine's `toggle_expand` validates unknown/not-expandable rows).
///   A non-tree focus (or a leaf) routes nothing. Tables' cell arrows are
///   the table machine's own scope ([`mount_table_key_route`]).
/// - Enter / Space — the focused thing's activation:
///   [`WidgetMountMsg::RowActivated`] for a row (the machine's
///   `item_activate`), [`WidgetMountMsg::ButtonClicked`] for a button.
///   The ENABLEMENT pre-read
///   ([`WidgetUiState::is_enabled`]) is the keyboard layer's own
///   refusal half: no message is produced for a disabled focus, so the
///   keyboard "selection/activation refuses cleanly" contract holds
///   before any route (and the machine re-gates regardless).
/// - Tab / Shift+Tab — the ring advance WITH WRAP (the roving-tabindex
///   model): at an edge the walk wraps to the first/last ENABLED row
///   (plain [`WidgetMountMsg::FocusMoved`], no selection write — Tab
///   moves focus, it does not re-litigate selection).
/// - Escape — nothing here: cancel is the host control operation the app
///   join runs machine-directly ([`MountRegistry::mount_escape_cancel_owned`];
///   drag cancel is SCN-021's).
///
/// A chord (Ctrl/Alt/Logo) routes nothing: chords are the app's
/// accelerator space, not mount navigation (Shift is honored ONLY by Tab's
/// direction — the roving-tabindex reverse). A key with no route → empty
/// vec. UNREGISTERED inputs are the CALLER'S resolution (a surface-id
/// mismatch refuses at [`apply_mount_msg`], the bridge's second gate).
///
/// The pure-layer note: deterministic over its inputs; the ONLY effect
/// beyond the returned messages is the machines' own roving focus slot
/// moving (through [`WidgetUiState::focus_next`]/[`focus_prev`] — the
/// authority — and the applied messages re-assert the SAME row through
/// [`apply_mount_msg`]'s `focus_set`, a no-op re-write), plus the
/// registry-side nav-step probe.
pub(crate) fn key_route_owned(
    registry: &mut MountRegistry,
    surface_id: StableId,
    surface: &WidgetSurface,
    state: &mut WidgetUiState,
    key: Key,
    modifiers: Modifiers,
) -> Vec<WidgetMountMsg> {
    // Chords are the app's accelerator space; only bare navigation routes.
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return Vec::new();
    }
    let views = state.synced().map(|synced| synced.views.clone()).unwrap_or_default();
    let ring = nav_ring(surface, state);
    if ring.is_empty() {
        return Vec::new();
    }
    let ring_ids: Vec<StableId> = ring.iter().map(|(id, _)| *id).collect();
    let step_msgs = |step: Option<StableId>, select: bool| {
        let Some(row_id) = step else {
            return Vec::new();
        };
        let widget_id = owner_of(&ring, Some(row_id)).unwrap_or(StableId(0));
        let mut msgs = vec![WidgetMountMsg::FocusMoved {
            surface_id,
            widget_id,
            to: Some(row_id),
        }];
        if select {
            // The arrows' list-cursor write: replace the selection with
            // the landed row (the machine caps + capability-gates).
            msgs.push(WidgetMountMsg::SelectionChanged {
                surface_id,
                widget_id,
                selected: vec![row_id],
            });
        }
        msgs
    };
    let msgs = match key {
        Key::Named(keyboard_key::Named::ArrowDown) => {
            let current = state.focus();
            let target = state.focus_next(current, &ring_ids, &views);
            step_msgs(target, true)
        }
        Key::Named(keyboard_key::Named::ArrowUp) => {
            let current = state.focus();
            let target = state.focus_prev(current, &ring_ids, &views);
            step_msgs(target, true)
        }
        Key::Named(keyboard_key::Named::ArrowRight) => tree_expand_step(surface, state, true),
        Key::Named(keyboard_key::Named::ArrowLeft) => tree_expand_step(surface, state, false),
        Key::Named(keyboard_key::Named::Enter) | Key::Named(keyboard_key::Named::Space) => {
            let Some(focused) = state.focus() else {
                return Vec::new();
            };
            // Clean refusal FIRST (the keyboard layer's own gate): a dead
            // focus never produces an activation route; the machine's
            // gates stay the authority for anything that slips through.
            if !state.is_enabled(focused, &views) {
                return Vec::new();
            }
            match entry_owner_is_button(surface, focused) {
                true => vec![WidgetMountMsg::ButtonClicked {
                    surface_id,
                    widget_id: focused,
                }],
                false => vec![WidgetMountMsg::RowActivated {
                    surface_id,
                    widget_id: owner_of(&ring, Some(focused)).unwrap_or(StableId(0)),
                    row_id: focused,
                }],
            }
        }
        Key::Named(keyboard_key::Named::Tab) => {
            let current = state.focus();
            // The roving-tabindex WRAP: at an edge the walk starts again
            // (focus_next with `None` = from the ring's top; focus_prev
            // with `None` = from its bottom) — landing on the first/last
            // ENABLED row (the machine skips the dead ones).
            let target = if modifiers.shift() {
                state.focus_prev(current, &ring_ids, &views)
                    .or_else(|| state.focus_prev(None, &ring_ids, &views))
            } else {
                state.focus_next(current, &ring_ids, &views)
                    .or_else(|| state.focus_next(None, &ring_ids, &views))
            };
            step_msgs(target, false)
        }
        // Escape (and every other key) does not route mount navigation.
        _ => Vec::new(),
    };
    if !msgs.is_empty() {
        registry.record_key_nav_step_owned();
    }
    msgs
}

/// The route-body seam (SCN-024; ADR-007 item 3) — SCN-025's D1 fold-back
/// promoted it to THE CANONICAL `on_mount_key` route body: the app's
/// `handle_mount_key_owned` (`crate::app::update::widget_mount`) now
/// DELEGATES here instead of restating the body — ONE body, one direction
/// (app → mount file; the app module chain stays unreachable from a bench
/// crate, so the bench drives the same canonical body the app runs). This
/// is the drift-kill: a route change edits THIS body only; the app fn is
/// the delegation shim (zero logic of its own).
///
/// Semantics unchanged (the state layers own the decisions): the target
/// resolution ([`Self::mount_key_surface_owned`]), Escape's machine-direct
/// cancel, the surface lookup ([`Self::handle_for_surface_owned`]), and the
/// pure translation ([`key_route_owned`]) — exactly the pre-fold pieces.
#[doc(hidden)]
pub fn bench_handle_mount_key_owned(
    registry: &mut MountRegistry,
    key: Key,
    modifiers: Modifiers,
) -> Vec<WidgetMountMsg> {
    let Some(surface_id) = registry.mount_key_surface_owned() else {
        return Vec::new();
    };
    // Escape: the mounted surface's cancel is machine-direct (the state
    // layers' own hooks — the armed prompt's dismissal); no route message
    // exists for it and no machine sees a KEY. (Same shape as the app fn.)
    if key == Key::Named(keyboard_key::Named::Escape) {
        registry.mount_escape_cancel_owned(surface_id);
        return Vec::new();
    }
    let Some(handle) = registry.handle_for_surface_owned(surface_id) else {
        return Vec::new();
    };
    let mut state = handle.state.borrow_mut();
    key_route_owned(registry, surface_id, &handle.surface, &mut state, key, modifiers)
}

/// The headless tests' facade: the SAME [`key_route_owned`] core over the
/// cfg(test) registry cell (the pre-migration static signature — see the
/// seam block near the file end).
#[cfg(test)]
pub(crate) fn key_route(
    surface_id: StableId,
    surface: &WidgetSurface,
    state: &mut WidgetUiState,
    key: Key,
    modifiers: Modifiers,
) -> Vec<WidgetMountMsg> {
    with_test_registry_mut(|registry| {
        key_route_owned(registry, surface_id, surface, state, key, modifiers)
    })
}

/// The landed row's owning widget from the ring pairs.
fn owner_of(ring: &[(StableId, StableId)], target: Option<StableId>) -> Option<StableId> {
    let id = target?;
    ring.iter()
        .find_map(|(row_id, widget_id)| (*row_id == id).then_some(*widget_id))
}

/// The Enter/Space owner's KIND read: is the focused id a visible synced
/// icon button (→ activation is a command click) or a row (→ the item
/// activation walk)?
fn entry_owner_is_button(surface: &WidgetSurface, id: StableId) -> bool {
    let mut walker: Vec<&WidgetNode> = surface.roots.iter().collect();
    while let Some(node) = walker.pop() {
        if !node_visible(node) {
            continue;
        }
        if let WidgetNode::Group(def) = node {
            walker.extend(def.children.iter());
            continue;
        }
        if let WidgetNode::IconButton(def) = node {
            if def.id == id {
                return true;
            }
        }
    }
    false
}

/// ArrowLeft/Right at a TREE row: the expand-toggle step (Right opens a
/// collapsed expandable row; Left closes an open one; the mirrored edge —
/// Right on an open row, Left on a closed one, a leaf, a disabled row, a
/// non-tree focus — routes NOTHING). Pure translation: the machine's
/// `toggle_expand` re-gates unknown/not-expandable/disabled rows inside
/// [`apply_mount_msg`].
fn tree_expand_step(
    surface: &WidgetSurface,
    state: &WidgetUiState,
    forward: bool,
) -> Vec<WidgetMountMsg> {
    let Some(current) = state.focus() else {
        return Vec::new();
    };
    let Some(synced) = state.synced() else {
        return Vec::new();
    };
    // The focused row's owning TREE (the ring's pair names it).
    let ring = nav_ring(surface, state);
    let Some(tree_id) = owner_of(&ring, Some(current)) else {
        return Vec::new();
    };
    if !state.is_enabled(current, &synced.views) {
        // A dead row takes no expansion route (its messages are the §16
        // "disabled items never emit" shape — absent, not refused).
        return Vec::new();
    }
    let Some(tree_def) = tree_def_of(surface, tree_id) else {
        return Vec::new();
    };
    if !tree_def.rows.iter().any(|row| row.id == current && row.expandable) {
        // A LEAF's Left/Right is an expand-toggle on nothing.
        return Vec::new();
    }
    let expanded_now = state.effective_expanded(current, false);
    if expanded_now == forward {
        // Right on an open row / Left on a closed one: the state is
        // already there — nothing to imply.
        return Vec::new();
    }
    let surface_id = state.surface_id();
    vec![WidgetMountMsg::RowExpandedToggled {
        surface_id,
        widget_id: tree_id,
        row_id: current,
        expanded: forward,
    }]
}

/// Find a VISIBLE node's tree def by widget id (the group walk — the same
/// discipline as [`entry_owner_is_button`]).
fn tree_def_of(surface: &WidgetSurface, widget_id: StableId) -> Option<&WidgetTreeDef> {
    let mut walker: Vec<&WidgetNode> = surface.roots.iter().collect();
    while let Some(node) = walker.pop() {
        if !node_visible(node) {
            continue;
        }
        match node {
            WidgetNode::Group(def) => walker.extend(def.children.iter()),
            WidgetNode::Tree(def) if def.id == widget_id => return Some(def),
            _ => {}
        }
    }
    None
}

/// The step a TABLE key resolved to (the pure keyboard layer's output —
/// the typed-table mount integration consumes these).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MountTableKeyStep {
    /// Row focus moved (Up/Down roving through the materialized window —
    /// [`TableUiState::row_focus_next`]/[`TableUiState::row_focus_prev`]).
    RowFocus(StableId),
    /// The cell cursor moved (Left/Right on the FOCUSED row —
    /// [`TableUiState::cell_focus_next`]/[`prev`]); the caller-held cursor
    /// slot is updated to the landed cell.
    CellFocus(CellFocus),
    /// Escape cancelled the open draft ([`TableUiState::cancel_cell_draft`]
    /// — the FREE path: no counters, no facts, nothing else moved).
    DraftCancelled,
}

/// The typed-table keyboard layer (SCN-020): translate ONE key into the
/// frozen [`TableUiState`] machine's steps — the state-layer consumption
/// the typed-table MOUNT integration (the later task) will ride, exactly
/// the way [`key_route`] consumes the SWG-002 machines:
///
/// - ArrowDown / ArrowUp — the materialized-window row roving (a
///   disabled-SKIPPING roving is the state layer's own — the published
///   rows window; the window edge stops the ring, the mount scrolls).
/// - ArrowRight / ArrowLeft — the cell cursor within `column_order` on
///   the FOCUSED row (`*cell_cursor` is the caller-held cursor slot;
///   `None` lands on the first/last column; the row never changes here —
///   the documented cell model).
/// - Escape — cancel the open draft (the free cancel; `Some(DraftCancelled)`
///   only when a draft existed).
///
/// Bare navigation keys only (chords route nothing). Returns `None` when
/// the key has no route (or the state has no synced read model —
/// graceful). NO counters, NO events: the state machine's own counters
/// are its §16 probes; `keyboard_nav_steps` ticks once per non-empty
/// route.
///
// (the typed-table MOUNT integration (the later task) is the production
// caller; the tests' facade consumes it today.)
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn mount_table_key_route_owned(
    registry: &mut MountRegistry,
    state: &mut TableUiState,
    cell_cursor: &mut Option<CellFocus>,
    key: Key,
    modifiers: Modifiers,
    spec: &WindowSpec,
    column_order: &[StableId],
) -> Option<MountTableKeyStep> {
    if modifiers.control() || modifiers.alt() || modifiers.logo() || modifiers.shift() {
        return None;
    }
    let step = match key {
        Key::Named(keyboard_key::Named::ArrowDown) => {
            let id = state.row_focus_next(spec)?;
            Some(MountTableKeyStep::RowFocus(id))
        }
        Key::Named(keyboard_key::Named::ArrowUp) => {
            let id = state.row_focus_prev(spec)?;
            Some(MountTableKeyStep::RowFocus(id))
        }
        Key::Named(keyboard_key::Named::ArrowRight) => {
            let landed = state.cell_focus_next(cell_cursor.as_ref().copied(), column_order)?;
            *cell_cursor = Some(landed); // the caller-held cursor moves with the landing
            Some(MountTableKeyStep::CellFocus(landed))
        }
        Key::Named(keyboard_key::Named::ArrowLeft) => {
            let landed = state.cell_focus_prev(cell_cursor.as_ref().copied(), column_order)?;
            *cell_cursor = Some(landed); // the caller-held cursor moves with the landing
            Some(MountTableKeyStep::CellFocus(landed))
        }
        Key::Named(keyboard_key::Named::Escape) => {
            if state.draft().is_some() {
                state.cancel_cell_draft();
                Some(MountTableKeyStep::DraftCancelled)
            } else {
                None
            }
        }
        _ => None,
    };
    if step.is_some() {
        registry.record_key_nav_step_owned();
    }
    step
}

/// The headless tests' facade: the SAME [`mount_table_key_route_owned`]
/// core over the cfg(test) registry cell.
#[cfg(test)]
pub(crate) fn mount_table_key_route(
    state: &mut TableUiState,
    cell_cursor: &mut Option<CellFocus>,
    key: Key,
    modifiers: Modifiers,
    spec: &WindowSpec,
    column_order: &[StableId],
) -> Option<MountTableKeyStep> {
    with_test_registry_mut(|registry| {
        mount_table_key_route_owned(
            registry, state, cell_cursor, key, modifiers, spec, column_order,
        )
    })
}

/// The panel hook's pointer-capture wrapper (SCN-020, the FEW-LINE form) —
/// wrap the mounted body so the surface-level gestures ride the SAME
/// [`WidgetMountMsg`] wagon: press → [`WidgetMountMsg::PointerPressed`]
/// (capture ON + the keyboard target), move → [`WidgetMountMsg::PointerMoved`]
/// (the coalescer's entry — never per-event machine work), release →
/// [`WidgetMountMsg::PointerReleased`] (the completion route). The
/// wrapper never steals from the body's own widgets: iced gives the child
/// widgets FIRST shot at every event (a row button's press captures it,
/// and this wrapper's own `on_press` is skipped — the row routes stay
/// exactly the gestures they were).
///
/// `surface_id` `None` (a body without a registry entry — cannot happen
/// through [`MountRegistry::mounted_body_owned`], but total by construction)
/// passes the element through untouched.
pub(crate) fn pointer_capture_body(
    element: Element<'static, WidgetMountMsg>,
    surface_id: Option<StableId>,
) -> Element<'static, WidgetMountMsg> {
    let Some(surface_id) = surface_id else {
        return element;
    };
    mouse_area(element)
        .on_press(WidgetMountMsg::PointerPressed { surface_id })
        .on_release(WidgetMountMsg::PointerReleased { surface_id })
        .on_move(move |position| WidgetMountMsg::PointerMoved { surface_id, position })
        .into()
}

/// RAII registration: the guarded surface tears down on drop
/// ([`MountRegistry::teardown_surface_owned`]) — the test and bench seam ("register once; the
/// scope closes; the state releases"). The app-owned registry migration
/// (SCN-016) keeps this type as the panels' keep-alive token; only its
/// drop target moved (the app-owned field, not a thread-local). In test
/// builds the drop runs the cfg(test) cell's teardown (the seam below);
/// in production builds the keep-alive's teardown is the app-owned
/// registration flow's obligation — no drop side effect runs there until
/// a production flow holds a guard (the wire era brings that flow).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RegisteredGuard {
    panel_id: String,
}

#[cfg(test)]
impl Drop for RegisteredGuard {
    fn drop(&mut self) {
        MountRegistry::teardown_surface(&self.panel_id);
    }
}

// (production builds: the natural drop — the type remains the keep-alive
// token; see the doc above for the wire-era drop contract.)
#[cfg(not(test))]
impl Drop for RegisteredGuard {
    fn drop(&mut self) {}
}

// ---------------------------------------------------------------------------
// SCN-025 — the app-owned PANEL SURFACE STORE (the v23 wire attach)
// ---------------------------------------------------------------------------
//
// The living truth the v23 wire attach publishes into, patches over, and
// pages: one entry per attached panel, keyed by the panel id the plugin's
// `RegisterPanel` registration used, recording the transport-authenticated
// OWNING plugin id and the two surface legs (a publish frame carries exactly
// one leg; a panel owns one of each — the wire contract). The attach
// (`app::plugin_host`) writes through this store and maps its semantic
// outcomes onto the wire acks; this module builds no frames.
//
// PLACEMENT DECISION (recorded, SCN-025): the store rides IN this registry
// (a `MountRegistry` field), not as a sibling app-`Rc`: the registry is
// already the app-`Rc` domain (`OpenCADStudio::mount_registry`), `HostSession`
// reaches it exactly the way the update arms do (`self.app.mount_registry`
// — inspected: that is how `HostSession` reaches ALL app state), and no
// lifecycle here diverges from the app's. A thread-local was the pre-SCN-016
// shape and stays buried.
//
// LEGACY GUARANTEE (pinned in the tests): entries appear ONLY through the
// attach. Zero publishes ⇒ zero entries ⇒ every legacy panel takes the
// byte-identical legacy view path; the store cannot add others.
//
// RECORDED BOUNDARY (SCN-025, not claimed): the SCENE MOUNT rendering seam
// (a `PanelScene` render onto host UI state) is NOT this package — the
// scene leg is STORED here (patch truth + snapshot2/pages) and stays
// invisible to the mounts until the scene-mount seam package lands.

/// One panel's stored surface truth (SCN-025).
///
/// `surface_id` is the frozen widget-surface identity
/// ([`PanelSurfaceStore::assign_surface_id`]); `None` while only the scene
/// leg is stored — so it is `Some` EXACTLY when the widget leg is, which is
/// also when a store read may raise a demand (the demand's `surface_id`
/// names the `WidgetSurface` the store addresses).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub(crate) struct PanelSurfaceEntry {
    /// The transport-authenticated publisher (the attach's owner law).
    pub(crate) owner_plugin_id: String,
    /// The frozen surface id the widget leg registers and streams under.
    pub(crate) surface_id: Option<StableId>,
    /// The stored scene leg (+ [`PanelScene::scene_summary`] — the digest
    /// row's scene half).
    pub(crate) scene: Option<PanelScene>,
    /// The stored widget-surface leg (the mount's registered copy mirrors
    /// this; the store is the truth, the handle the projection).
    pub(crate) widgets: Option<WidgetSurface>,
}

/// The app-owned per-panel surface store (see the section docs).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
pub(crate) struct PanelSurfaceStore {
    by_panel: HashMap<String, PanelSurfaceEntry>,
}

/// A successful publish/patch's semantic outcome — the attach maps it onto
/// `PluginResponse::PatchApplied`; widget legs also yield the frozen
/// `(surface id, surface)` to (re)register into the mount.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub(crate) struct PanelSurfaceOutcome {
    /// The bounded ack report: the SCENE leg's REAL planner output
    /// ([`PanelScene::apply_patch_report`]); the widget leg's capped derived
    /// facts ([`widget_patch_report`]) and a publish leg's root-level caps
    /// ([`publish_leg_report`]).
    pub(crate) report: ocs_plugin_api::scene::AppliedPatchReport,
    /// The widget surface to (re)mount, with its stored frozen identity.
    pub(crate) widget_mount: Option<(StableId, WidgetSurface)>,
}

/// A store read's answer plus the demand the read raises (the attach maps
/// onto `PluginResponse::StorePage`; non-`None` `surface_id` naming the
/// attached widget store is also the demand's permission — nothing attached
/// names nothing, so nothing demands).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
pub(crate) struct StoreReadOutcome {
    pub(crate) surface_id: Option<StableId>,
    pub(crate) rows: Vec<StoreRow>,
    /// The host's belief of the addressed store's total (the held count; a
    /// table's self-declared total when the def names one) — a hint, never a
    /// contract, `0` unattached.
    pub(crate) total_hint: u64,
    /// The lazy-children/page miss the read raises:
    /// `(parent_id, the missing span)` — answered by the owning plugin's
    /// own publish/patch (decoupled; this read's answer stays the checked
    /// page). Scene regions never demand: the host store IS the whole
    /// scene truth.
    pub(crate) demand: Option<(Option<StableId>, WidgetRange)>,
}

/// The structured refusal's semantic shape (code + bounded recovery report).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type StoreReject = (PatchRejectCode, PatchRejectReport);

#[cfg(not(target_arch = "wasm32"))]
fn reject(code: PatchRejectCode, current_revision: Option<SceneRevision>) -> StoreReject {
    (
        code,
        PatchRejectReport {
            current_revision,
            failed_op: None,
        },
    )
}

/// Capped push with an exact truncation count (the scene report's own
/// discipline; [`ocs_plugin_api::scene::MAX_REPORT_TOUCHED`] /
/// [`MAX_REPORT_RETIRED`]).
#[cfg(not(target_arch = "wasm32"))]
fn capped_push(list: &mut Vec<StableId>, truncated: &mut usize, cap: usize, id: StableId) {
    if list.len() < cap {
        list.push(id);
    } else {
        *truncated += 1;
    }
}

#[cfg(not(target_arch = "wasm32"))]
const MAX_REPORT_TOUCHED: usize = ocs_plugin_api::scene::MAX_REPORT_TOUCHED;
#[cfg(not(target_arch = "wasm32"))]
const MAX_REPORT_RETIRED: usize = ocs_plugin_api::scene::MAX_REPORT_RETIRED;

/// The ACK report's summary slot for a panel that stores NO scene leg: the
/// zero tree at the touched revision. The report type is scene-shaped by
/// wire contract (`AppliedPatchReport`); the recovery truth stays the
/// [`StoreRow::State`] digest row, never this hint.
#[cfg(not(target_arch = "wasm32"))]
fn absent_scene_summary(revision: SceneRevision) -> ocs_plugin_api::scene::SceneSummary {
    ocs_plugin_api::scene::SceneSummary {
        revision,
        nodes: 0,
        by_kind: SceneKindCounts::default(),
        root_ids: Vec::new(),
        root_ids_truncated: 0,
        overflow_roots: false,
    }
}

/// The widget leg's capped ACK report: the crate's widget `apply_patch`
/// reports no per-op facts, so the attach derives the bounded ones from the
/// applied patch's own payload addresses — touched = each op's addressed id
/// (an Add's inserted root, an Update/Remove's target, a table cell's row
/// key), retired = Remove ops' removed ids — under the scene report's caps
/// with exact truncation counts. NOT a planner-grade plan (the retire lists
/// do not chase Update-swap payload ids): the recovery truth is the
/// snapshot2 digest + windows; the report is the bounded hint.
//
// `ops` borrows the patch pre-application (the report's revision is the
// patch's declared successor — identical to the post-apply head on success;
// a failed apply discards the report).
#[cfg(not(target_arch = "wasm32"))]
fn widget_patch_report(
    revision: SceneRevision,
    ops: &[ocs_plugin_api::widgets::WidgetOp],
    scene_summary: Option<ocs_plugin_api::scene::SceneSummary>,
) -> ocs_plugin_api::scene::AppliedPatchReport {
    use ocs_plugin_api::widgets::WidgetOp;
    let mut touched = Vec::new();
    let mut touched_truncated = 0usize;
    let mut retired = Vec::new();
    let mut retired_truncated = 0usize;
    for op in ops {
        let address = match op {
            WidgetOp::Add { widget, .. } => widget.widget_id(),
            WidgetOp::Update { id, .. } | WidgetOp::Remove { id } => *id,
            WidgetOp::TableCell { key, .. } => *key,
        };
        capped_push(&mut touched, &mut touched_truncated, MAX_REPORT_TOUCHED, address);
        if let WidgetOp::Remove { id } = op {
            capped_push(&mut retired, &mut retired_truncated, MAX_REPORT_RETIRED, *id);
        }
    }
    ocs_plugin_api::scene::AppliedPatchReport {
        revision,
        touched_subtree_ids: touched,
        touched_truncated,
        retired_ids: retired,
        retired_truncated,
        summary: scene_summary.unwrap_or_else(|| absent_scene_summary(revision)),
    }
}

/// A publish leg's report (the whole bounded surface arrives; the root ids
/// are the touched set, the replaced leg's roots the retired set — capped).
#[cfg(not(target_arch = "wasm32"))]
fn publish_leg_report(
    revision: SceneRevision,
    arriving_root_ids: Vec<StableId>,
    retired_root_ids: Vec<StableId>,
    summary: Option<ocs_plugin_api::scene::SceneSummary>,
) -> ocs_plugin_api::scene::AppliedPatchReport {
    let mut touched = Vec::new();
    let mut touched_truncated = 0usize;
    for id in arriving_root_ids {
        capped_push(&mut touched, &mut touched_truncated, MAX_REPORT_TOUCHED, id);
    }
    let mut retired = Vec::new();
    let mut retired_truncated = 0usize;
    for id in retired_root_ids {
        capped_push(&mut retired, &mut retired_truncated, MAX_REPORT_RETIRED, id);
    }
    ocs_plugin_api::scene::AppliedPatchReport {
        revision,
        touched_subtree_ids: touched,
        touched_truncated,
        retired_ids: retired,
        retired_truncated,
        summary: summary.unwrap_or_else(|| absent_scene_summary(revision)),
    }
}

/// Widget tree defs in document order (one stack walk, the crate's
/// iterative-with-`.rev()` discipline).
#[cfg(not(target_arch = "wasm32"))]
fn widget_tree_defs(surface: &WidgetSurface) -> Vec<&ocs_plugin_api::widgets::WidgetTreeDef> {
    let mut defs = Vec::new();
    let mut walker: Vec<&WidgetNode> = surface.roots.iter().collect();
    while let Some(node) = walker.pop() {
        match node {
            WidgetNode::Tree(def) => defs.push(def),
            WidgetNode::Group(group) => walker.extend(group.children.iter().rev()),
            _ => {}
        }
    }
    defs
}

/// Item lists in document order.
#[cfg(not(target_arch = "wasm32"))]
fn widget_list_defs(surface: &WidgetSurface) -> Vec<&ItemListDef> {
    let mut defs = Vec::new();
    let mut walker: Vec<&WidgetNode> = surface.roots.iter().collect();
    while let Some(node) = walker.pop() {
        match node {
            WidgetNode::List(def) => defs.push(def),
            WidgetNode::Group(group) => walker.extend(group.children.iter().rev()),
            _ => {}
        }
    }
    defs
}

/// The widget ROW a `parent_id` names (surface-wide row namespace; validated
/// unique — the same contract the events and the store pages ride).
#[cfg(not(target_arch = "wasm32"))]
fn find_tree_row<'a>(
    surface: &'a WidgetSurface,
    id: StableId,
) -> Option<&'a TreeRowDef> {
    widget_tree_defs(surface)
        .into_iter()
        .flat_map(|def| def.rows.iter())
        .find(|row| row.id == id)
}

/// The tree rows of a `parent_id` window: `None` → every tree's root rows
/// (document order); `Some(pid)` → the rows declaring `pid` (rows-vec order
/// per tree, trees in document order).
#[cfg(not(target_arch = "wasm32"))]
fn tree_children_of<'a>(surface: &'a WidgetSurface, parent_id: Option<StableId>) -> Vec<&'a TreeRowDef> {
    let mut rows = Vec::new();
    for def in widget_tree_defs(surface) {
        for row in &def.rows {
            if row.parent == parent_id {
                rows.push(row);
            }
        }
    }
    rows
}

/// CHECKED page-window bounds: an empty/past-the-end window is `(0, 0)`
/// (never a panic; the store size bounds every subtraction); a served width
/// never exceeds the requested width NOR [`MAX_WIDGET_ROWS_INITIAL`] — the
/// page wire budget — so the attach answers never a bigger page.
#[cfg(not(target_arch = "wasm32"))]
fn page_window_bounds(start: usize, requested_len: usize, held_len: usize) -> (usize, usize) {
    if held_len == 0 || start >= held_len {
        return (0, 0);
    }
    let width = requested_len.min(held_len - start).min(MAX_WIDGET_ROWS_INITIAL);
    (start, width)
}

/// The page-miss DEMAND rule (the store's lazy-children / page misses):
/// only an ATTACHED widget store demands (the demand names `surface_id`);
/// an address the store cannot RESOLVE (a lazy branch's row, a row/list
/// beyond the held row window, an unpublished table) is asked verbatim; a
/// resolved address whose window extends past the held rows asks for the
/// exact holding gap. Scene regions (the scene leg) and fully-materialized
/// tables (def-declared totals) never demand: the host store is their
/// whole truth or the def says so.
#[cfg(not(target_arch = "wasm32"))]
fn widgets_demand(
    parent_id: Option<StableId>,
    requested: WidgetRange,
    held_len: usize,
    surface_id: Option<StableId>,
    parent_known: bool,
    demandable: bool,
) -> Option<(Option<StableId>, WidgetRange)> {
    if surface_id.is_none() || !demandable {
        return None;
    }
    if !parent_known {
        // The store does not NAME the address: the plugin owns rows the host
        // has not seen (lazy children, beyond-the-held rows, unpublished
        // stores) — ask for exactly the requested window.
        return Some((parent_id, requested));
    }
    let end = requested.start.saturating_add(requested.len);
    if end > held_len {
        // The span past the held rows: ask for exactly the holding gap.
        return Some((parent_id, WidgetRange { start: held_len, len: end - held_len }));
    }
    None
}

/// The scene subtree resolution for a region read: `None` → the whole
/// scene, pre-order, document order (the snapshot2 window and the
/// continuation [`PanelStoreKind::SceneRegion`] windows share THE one
/// flatten); `Some(root)` → that element's subtree, inclusive, pre-order
/// (an unresolvable root is an empty window — not an error).
#[cfg(not(target_arch = "wasm32"))]
fn scene_region_flatten<'a>(
    elements: &'a [SceneElement],
    root: Option<StableId>,
) -> Vec<&'a SceneElement> {
    if let Some(root_id) = root {
        // The resolved subtree (the region).
        let mut subtree: Vec<&'a SceneElement> = Vec::new();
        let mut walker: Vec<&SceneElement> = Vec::new();
        for element in elements.iter().rev() {
            walker.push(element);
        }
        while let Some(element) = walker.pop() {
            if element.stable_id() == root_id {
                scene_subtree_preorder(element, &mut subtree);
                return subtree;
            }
            if let SceneElement::Group { children, .. } = element {
                walker.extend(children.iter().rev());
            }
        }
        return subtree;
    }
    // The whole scene: pre-order, document order.
    let mut flat: Vec<&'a SceneElement> = Vec::new();
    let mut walker: Vec<&SceneElement> = Vec::new();
    for element in elements.iter().rev() {
        walker.push(element);
    }
    while let Some(element) = walker.pop() {
        flat.push(element);
        if let SceneElement::Group { children, .. } = element {
            walker.extend(children.iter().rev());
        }
    }
    flat
}

/// Inclusive pre-order of one validated element's subtree (the store only
/// holds `validate()`-passed scenes: depth ≤ [`MAX_SCENE_DEPTH`], so the
/// recursion is bounded by the crate's own cap).
#[cfg(not(target_arch = "wasm32"))]
fn scene_subtree_preorder<'a>(element: &'a SceneElement, out: &mut Vec<&'a SceneElement>) {
    out.push(element);
    if let SceneElement::Group { children, .. } = element {
        for child in children {
            scene_subtree_preorder(child, out);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl PanelSurfaceStore {
    /// The frozen surface id for `panel_id`'s widget leg: the FIRST ROOT's
    /// widget id — the registration convention the mount's test/log seam
    /// already pins (`register_test_surface`: "the mount state opens under
    /// the surface's FIRST root's id"; the plugin can name it: it built the
    /// surface) — falling back to a bounded FNV probe of the panel id when
    /// that id is ALREADY another panel's stored identity (cross-surface
    /// uniqueness is the bridge's surface-id lookups' registration
    /// contract). The ENTRY FREEZES the assignment once; re-registrations
    /// (a patch, a re-publish) KEEP it, so the panel's event stream names
    /// one stable identity per surface.
    pub(crate) fn assign_surface_id(&self, panel_id: &str, surface: &WidgetSurface) -> StableId {
        let taken = |id: StableId| self.by_panel.values().any(|entry| entry.surface_id == Some(id));
        let candidate = surface
            .roots
            .first()
            .map(|node| node.widget_id())
            .unwrap_or(StableId(0));
        if !taken(candidate) {
            return candidate;
        }
        // The bounded fallback: salted FNV-1a over the panel id (deterministic,
        // content-derived, off the first-root id space in 256 probes).
        for salt in 1u64..=256u64 {
            let mut hash: u64 = 0xcbf29ce484222325 ^ salt;
            for byte in panel_id.as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash |= 1; // off the StableId(0)-shaped empty-surface identity
            let candidate = StableId(hash);
            if !taken(candidate) {
                return candidate;
            }
        }
        // Unreachable totality: 256 collisions are not a reachable state;
        // the store never panics from a naming question.
        StableId(u64::MAX)
    }

    /// Is the panel attached (a stored entry)?
    pub(crate) fn attached(&self, panel_id: &str) -> bool {
        self.by_panel.contains_key(panel_id)
    }

    /// The entry read (the tests' and the digest recons' handle).
    pub(crate) fn entry(&self, panel_id: &str) -> Option<&PanelSurfaceEntry> {
        self.by_panel.get(panel_id)
    }

    /// The entry handle (inserting on first attach with the owner bound).
    fn entry_or_insert(&mut self, owner_plugin_id: &str, panel_id: &str) -> &mut PanelSurfaceEntry {
        self.by_panel
            .entry(panel_id.to_owned())
            .or_insert_with(|| PanelSurfaceEntry {
                owner_plugin_id: owner_plugin_id.to_owned(),
                surface_id: None,
                scene: None,
                widgets: None,
            })
    }

    /// THE attach's publish (the snapshot leg): the store's OWN structural
    /// gate (the same crate `validate()` the transport runs — an in-process
    /// path bypassing the wire gets the identical gate), the owner law, then
    /// the leg's revision law, then the store.
    ///
    /// Rejection shapes (all structured, zero mutation):
    /// - structurally invalid payload → `InvalidOp`;
    /// - another plugin's panel → `PublisherOwnedByOther` (the SCN-025
    ///   appended code; the first publisher owns the panel's store; publish
    ///   never transfers it — never a takeover);
    /// - a same-owner publish that REWINDS the leg's stored head (a full
    ///   publish must be a greater revision; an equal revision must carry
    ///   identical content — the refresh shape) → `NonMonotonicRevision`
    ///   with the stored head reported.
    ///
    /// The `events` leg (the plugin's merge acknowledgment of host-observed
    /// events) is ABSORBED, not re-forwarded: rebroadcasting would double
    /// the event stream (the exactly-once law) — the attach's doc records
    /// the shape. The published surface's per-leg re-registration of the
    /// MOUNT is the attach's next step (`register_surface_owned`), driven
    /// by this outcome's `widget_mount`.
    pub(crate) fn publish(
        &mut self,
        owner_plugin_id: &str,
        panel_id: &str,
        surface: SceneOrWidgetSurface,
    ) -> Result<PanelSurfaceOutcome, StoreReject> {
        let structural = match &surface {
            SceneOrWidgetSurface::Scene(scene) => scene.validate().is_ok(),
            SceneOrWidgetSurface::Widgets(widgets) => widgets.validate().is_ok(),
        };
        if !structural {
            return Err(reject(PatchRejectCode::InvalidOp, None));
        }
        if let Some(entry) = self.by_panel.get(panel_id) {
            if entry.owner_plugin_id != owner_plugin_id {
                return Err(reject(PatchRejectCode::PublisherOwnedByOther, None));
            }
        }
        match surface {
            SceneOrWidgetSurface::Scene(scene) => {
                if let Some(stored) =
                    self.by_panel.get(panel_id).and_then(|entry| entry.scene.as_ref())
                {
                    let head = stored.revision;
                    if scene.revision == head {
                        if scene == *stored {
                            // Identical snapshot at the head: the refresh
                            // shape (idempotent; the store keeps the stored
                            // leg — equal content either way).
                            let touched: Vec<StableId> =
                                scene.elements.iter().map(|element| element.stable_id()).collect();
                            return Ok(PanelSurfaceOutcome {
                                report: publish_leg_report(
                                    head,
                                    touched,
                                    Vec::new(),
                                    Some(scene.scene_summary()),
                                ),
                                widget_mount: None,
                            });
                        }
                        return Err(reject(PatchRejectCode::NonMonotonicRevision, Some(head)));
                    }
                    if scene.revision < head {
                        return Err(reject(PatchRejectCode::NonMonotonicRevision, Some(head)));
                    }
                }
                // The FULL root-id lists ride the report builder (it caps
                // and counts the truncation exactly — a pre-capped list
                // would lose the count).
                let touch_ids: Vec<StableId> =
                    scene.elements.iter().map(|element| element.stable_id()).collect();
                let retired_root_ids: Vec<StableId> = match self
                    .by_panel
                    .get_mut(panel_id)
                    .and_then(|entry| entry.scene.replace(scene.clone()))
                {
                    Some(replaced) => {
                        replaced.elements.iter().map(|element| element.stable_id()).collect()
                    }
                    None => Vec::new(),
                };
                let summary = scene.scene_summary();
                let report =
                    publish_leg_report(scene.revision, touch_ids, retired_root_ids, Some(summary));
                let entry = self.entry_or_insert(owner_plugin_id, panel_id);
                entry.scene = Some(scene);
                Ok(PanelSurfaceOutcome {
                    report,
                    widget_mount: None,
                })
            }
            SceneOrWidgetSurface::Widgets(widgets) => {
                // The frozen identity: assigned ONCE per entry, BEFORE the
                // map write, so the probe sees the other entries only.
                let frozen = match self.by_panel.get(panel_id) {
                    Some(entry) => {
                        entry.surface_id.unwrap_or_else(|| self.assign_surface_id(panel_id, &widgets))
                    }
                    None => self.assign_surface_id(panel_id, &widgets),
                };
                let summary = self
                    .by_panel
                    .get(panel_id)
                    .and_then(|entry| entry.scene.as_ref().map(PanelScene::scene_summary));
                if let Some(stored) =
                    self.by_panel.get(panel_id).and_then(|entry| entry.widgets.as_ref())
                {
                    let head = stored.revision;
                    if widgets.revision == head {
                        if widgets == *stored {
                            // Identical resurface at the head: PRESERVE —
                            // the mount rule's own shape (the re-register
                            // below preserves the running machines).
                            let touch_ids: Vec<StableId> =
                                widgets.roots.iter().map(|node| node.widget_id()).collect();
                            return Ok(PanelSurfaceOutcome {
                                report: publish_leg_report(head, touch_ids, Vec::new(), summary),
                                widget_mount: Some((frozen, widgets.clone())),
                            });
                        }
                        return Err(reject(PatchRejectCode::NonMonotonicRevision, Some(head)));
                    }
                    if widgets.revision < head {
                        return Err(reject(PatchRejectCode::NonMonotonicRevision, Some(head)));
                    }
                }
                let retired_root_ids: Vec<StableId> = match self
                    .by_panel
                    .get_mut(panel_id)
                    .and_then(|entry| entry.widgets.replace(widgets.clone()))
                {
                    Some(replaced) => {
                        replaced.roots.iter().map(|node| node.widget_id()).collect()
                    }
                    None => Vec::new(),
                };
                let touch_ids: Vec<StableId> =
                    widgets.roots.iter().map(|node| node.widget_id()).collect();
                let report =
                    publish_leg_report(widgets.revision, touch_ids, retired_root_ids, summary);
                let entry = self.entry_or_insert(owner_plugin_id, panel_id);
                entry.surface_id = Some(frozen);
                entry.widgets = Some(widgets.clone());
                Ok(PanelSurfaceOutcome {
                    report,
                    widget_mount: Some((frozen, widgets)),
                })
            }
        }
    }

    /// THE attach's patch (the incremental leg): the crate's ATOMIC
    /// two-phase `apply_patch(_report)` over the stored leg — a rejection
    /// leaves everything exactly as it was (the crate's own contract); a
    /// success adopts the successor revision; a WIDGET leg's outcome carries
    /// the surface for the attach's re-registration (a patch necessarily
    /// moves the revision, so the mount's PRESERVE-ON-IDENTICAL resolves it
    /// as the REPLACE flow — the documented re-register rule). SCENE-ONLY
    /// patches never re-mount (the recorded boundary above).
    pub(crate) fn patch(
        &mut self,
        owner_plugin_id: &str,
        panel_id: &str,
        patch: SceneOrWidgetPatch,
    ) -> Result<PanelSurfaceOutcome, StoreReject> {
        if !self.attached(panel_id) {
            return Err(reject(PatchRejectCode::PublisherUnattached, None));
        }
        // The owner law runs both ways (the register_quad_panels precedent):
        // a patch addresses the transport identity the publish attached.
        if self
            .by_panel
            .get(panel_id)
            .is_some_and(|entry| entry.owner_plugin_id != owner_plugin_id)
        {
            return Err(reject(PatchRejectCode::PublisherOwnedByOther, None));
        }
        let Some(entry) = self.by_panel.get_mut(panel_id) else {
            // Not reachable (checked above); stay total without panics.
            return Err(reject(PatchRejectCode::PublisherUnattached, None));
        };
        match patch {
            SceneOrWidgetPatch::Scene(scene_patch) => {
                let Some(scene) = entry.scene.as_mut() else {
                    return Err(reject(PatchRejectCode::PublisherUnattached, None));
                };
                match scene.apply_patch_report(scene_patch) {
                    Ok(report) => Ok(PanelSurfaceOutcome {
                        report,
                        widget_mount: None,
                    }),
                    Err(error) => Err(scene_reject(&error, scene.revision)),
                }
            }
            SceneOrWidgetPatch::Widgets(widget_patch) => {
                let Some(widgets) = entry.widgets.as_mut() else {
                    return Err(reject(PatchRejectCode::PublisherUnattached, None));
                };
                // The patch's facts derive from the ops themselves
                // (pre-application); the head revision is the
                // stale-refusal's recovery fact.
                let planned_report = widget_patch_report(
                    widget_patch.revision,
                    &widget_patch.ops,
                    entry.scene.as_ref().map(PanelScene::scene_summary),
                );
                let head = widgets.revision;
                match widgets.apply_patch(widget_patch) {
                    Ok(()) => {
                        let surface_id =
                            entry.surface_id.expect("frozen by the widget publish");
                        Ok(PanelSurfaceOutcome {
                            report: planned_report,
                            widget_mount: Some((surface_id, widgets.clone())),
                        })
                    }
                    Err(error) => Err(widget_reject(&error, head)),
                }
            }
        }
    }

    /// THE attach's page server: the requested window resolved with CHECKED
    /// arithmetic against the host-held rows (in-range portions serve,
    /// past-end windows answer EMPTY — never a panic; a width never exceeds
    /// the requested width NOR the page wire budget). `total_hint` = the
    /// held count (a table's self-declared total when its def names one).
    /// The DEMAND leg rides the page misses (see [`widgets_demand`]) and
    /// leaves from the ATTACH (a `broadcast_panel_store_demand`), never
    /// synchronously answered.
    pub(crate) fn store_page(
        &self,
        panel_id: &str,
        kind: PanelStoreKind,
        parent_id: Option<StableId>,
        range: WidgetRange,
    ) -> StoreReadOutcome {
        let Some(entry) = self.entry(panel_id) else {
            // Nothing attached: the honest empty page, never a demand.
            return StoreReadOutcome::default();
        };
        match kind {
            PanelStoreKind::TreeChildren => self.page_tree_children(entry, parent_id, range),
            PanelStoreKind::ListItems => self.page_list_items(entry, parent_id, range),
            PanelStoreKind::TableRows => self.page_table_rows(entry, parent_id, range),
            PanelStoreKind::SceneRegion => self.page_scene_region(entry, parent_id, range),
        }
    }

    /// THE attach's recovery read: the digest row plus the leading scene
    /// window — a page whose FIRST row is the [`StoreRow::State`] digest
    /// (`None` halves for absent legs) followed by at most the page budget
    /// of scene elements (the leading window of the same flatten the
    /// [`PanelStoreKind::SceneRegion`] continuation pages over, parent
    /// `None`). Never a full dump: the window is bounded, and the digest
    /// is the bounded summary. Unattached → the empty digest only
    /// (the transport-era default shape).
    pub(crate) fn snapshot_page(&self, panel_id: &str) -> (Vec<StoreRow>, u64) {
        let Some(entry) = self.entry(panel_id) else {
            return (
                vec![StoreRow::State(StoreStateDigest {
                    scene: None,
                    widgets: None,
                })],
                0,
            );
        };
        let digest = StoreRow::State(StoreStateDigest {
            scene: entry.scene.as_ref().map(PanelScene::scene_summary),
            widgets: entry.widgets.as_ref().map(WidgetSurface::widget_summary),
        });
        let leading: Vec<&SceneElement> =
            match entry.scene.as_ref() {
                Some(scene) => scene_region_flatten(&scene.elements, None),
                None => Vec::new(),
            };
        // The digest row shares the page's budget: the window reserves its
        // one row.
        let (start, width) =
            page_window_bounds(0, MAX_WIDGET_ROWS_INITIAL.saturating_sub(1), leading.len());
        let mut rows: Vec<StoreRow> = Vec::with_capacity(width + 1);
        rows.push(digest);
        for element in &leading[start..start + width] {
            rows.push(StoreRow::Scene((*element).clone()));
        }
        (rows, leading.len() as u64)
    }

    // -- the per-kind pages (each pins its own demand rule) -----------------

    fn page_tree_children(
        &self,
        entry: &PanelSurfaceEntry,
        parent_id: Option<StableId>,
        range: WidgetRange,
    ) -> StoreReadOutcome {
        let Some(widgets) = entry.widgets.as_ref() else {
            return StoreReadOutcome {
                surface_id: entry.surface_id,
                ..StoreReadOutcome::default()
            };
        };
        let surface_id = entry.surface_id;
        // A resolved row's children (rows declaring the parent) or the
        // surface's root rows (parent `None`).
        let parent_known = match parent_id {
            Some(pid) => find_tree_row(widgets, pid).is_some(),
            None => true,
        };
        let held = tree_children_of(widgets, parent_id);
        let (start, width) = page_window_bounds(range.start, range.len, held.len());
        let rows: Vec<StoreRow> =
            held[start..start + width].iter().map(|row| StoreRow::Tree((*row).clone())).collect();
        let demand = widgets_demand(parent_id, range, held.len(), surface_id, parent_known, true);
        StoreReadOutcome {
            surface_id,
            rows,
            total_hint: held.len() as u64,
            demand,
        }
    }

    fn page_list_items(
        &self,
        entry: &PanelSurfaceEntry,
        parent_id: Option<StableId>,
        range: WidgetRange,
    ) -> StoreReadOutcome {
        let Some(widgets) = entry.widgets.as_ref() else {
            return StoreReadOutcome {
                surface_id: entry.surface_id,
                ..StoreReadOutcome::default()
            };
        };
        let surface_id = entry.surface_id;
        let lists = widget_list_defs(widgets);
        let parent_known = match parent_id {
            Some(pid) => lists.iter().any(|def| def.id == pid),
            None => true,
        };
        let held: Vec<&ocs_plugin_api::widgets::ItemDef> = match parent_id {
            Some(pid) => lists
                .iter()
                .filter(|def| def.id == pid)
                .flat_map(|def| def.items.iter())
                .collect(),
            None => lists.iter().flat_map(|def| def.items.iter()).collect(),
        };
        let (start, width) = page_window_bounds(range.start, range.len, held.len());
        let rows: Vec<StoreRow> =
            held[start..start + width].iter().map(|item| StoreRow::Item((*item).clone())).collect();
        let demand = widgets_demand(parent_id, range, held.len(), surface_id, parent_known, true);
        StoreReadOutcome {
            surface_id,
            rows,
            total_hint: held.len() as u64,
            demand,
        }
    }

    fn page_table_rows(
        &self,
        entry: &PanelSurfaceEntry,
        parent_id: Option<StableId>,
        range: WidgetRange,
    ) -> StoreReadOutcome {
        let Some(widgets) = entry.widgets.as_ref() else {
            return StoreReadOutcome {
                surface_id: entry.surface_id,
                ..StoreReadOutcome::default()
            };
        };
        let surface_id = entry.surface_id;
        // parent_id names the registry table (None = every registry entry,
        // BTreeMap order — deterministic).
        match parent_id {
            Some(table_id) => {
                let Some(def) = widgets.tables.get(&table_id) else {
                    // The store does not name the table: the plugin may own
                    // it (unpublished) — empty page + the verbatim ask.
                    return StoreReadOutcome {
                        surface_id,
                        rows: Vec::new(),
                        total_hint: 0,
                        demand: widgets_demand(parent_id, range, 0, surface_id, false, true),
                    };
                };
                let held = def.rows.iter().collect::<Vec<_>>();
                let (start, width) = page_window_bounds(range.start, range.len, held.len());
                let rows: Vec<StoreRow> = held[start..start + width]
                    .iter()
                    .map(|row| StoreRow::Table((*row).clone()))
                    .collect();
                // The def's OWN total declares the windowed table: a read
                // past the published row window against a windowing table
                // demands; a fully-materialized table's past end is past
                // the end (no demand).
                let demandable =
                    def.total_rows.is_some_and(|total| total as usize > def.rows.len());
                let total_hint = def.total_rows.unwrap_or(def.rows.len() as u64);
                let demand = widgets_demand(
                    parent_id,
                    range,
                    held.len(),
                    surface_id,
                    true,
                    demandable,
                );
                StoreReadOutcome {
                    surface_id,
                    rows,
                    total_hint,
                    demand,
                }
            }
            None => {
                let held: Vec<&ocs_plugin_api::widgets::TableRowDef> = widgets
                    .tables
                    .values()
                    .flat_map(|def| def.rows.iter())
                    .collect();
                let (start, width) = page_window_bounds(range.start, range.len, held.len());
                let rows: Vec<StoreRow> = held[start..start + width]
                    .iter()
                    .map(|row| StoreRow::Table((*row).clone()))
                    .collect();
                let held_len = held.len();
                let demand = widgets_demand(None, range, held_len, surface_id, true, true);
                StoreReadOutcome {
                    surface_id,
                    rows,
                    total_hint: held_len as u64,
                    demand,
                }
            }
        }
    }

    fn page_scene_region(
        &self,
        entry: &PanelSurfaceEntry,
        parent_id: Option<StableId>,
        range: WidgetRange,
    ) -> StoreReadOutcome {
        // The SCENE leg resolves; a scene region read is never a demand
        // (the host store is the whole scene truth; a past-end window is
        // past-the-end — empty, never a panic).
        let Some(scene) = entry.scene.as_ref() else {
            return StoreReadOutcome {
                surface_id: entry.surface_id,
                ..StoreReadOutcome::default()
            };
        };
        let held = scene_region_flatten(&scene.elements, parent_id);
        let (start, width) = page_window_bounds(range.start, range.len, held.len());
        let rows: Vec<StoreRow> = held[start..start + width]
            .iter()
            .map(|element| StoreRow::Scene((*element).clone()))
            .collect();
        StoreReadOutcome {
            surface_id: entry.surface_id,
            rows,
            total_hint: held.len() as u64,
            demand: None,
        }
    }
}

/// The attach's SCENE-leg rejection mapping: the crate's own error
/// vocabulary to the code (bounded report; the head revision reported as
/// the recovery fact where the code left it out).
#[cfg(not(target_arch = "wasm32"))]
fn scene_reject(error: &ocs_plugin_api::scene::SceneError, head: SceneRevision) -> StoreReject {
    use ocs_plugin_api::scene::SceneError;
    match error {
        SceneError::StaleBase { current, .. } => {
            reject(PatchRejectCode::StaleBase, Some(*current))
        }
        SceneError::NonMonotonicRevision { current, .. } => {
            reject(PatchRejectCode::NonMonotonicRevision, Some(*current))
        }
        SceneError::PatchOpBudgetExceeded { .. } => {
            reject(PatchRejectCode::OpBudgetExceeded, Some(head))
        }
        // Every structural crossing (duplicate ids, parents, depths, texts,
        // point counts, floats) is the InvalidOp bucket.
        _ => reject(PatchRejectCode::InvalidOp, Some(head)),
    }
}

/// The widget-leg rejection mapping: `BudgetExceeded` fights twice — the op
/// cap IS the op budget ([`MAX_PATCH_OPS`]; the crate's documented
/// coincidence) — anything else is the structural bucket.
#[cfg(not(target_arch = "wasm32"))]
fn widget_reject(error: &ocs_plugin_api::widgets::WidgetError, head: SceneRevision) -> StoreReject {
    use ocs_plugin_api::widgets::WidgetError;
    match error {
        WidgetError::StaleBase { current, .. } => {
            reject(PatchRejectCode::StaleBase, Some(*current))
        }
        WidgetError::NonMonotonicRevision { current, .. } => {
            reject(PatchRejectCode::NonMonotonicRevision, Some(*current))
        }
        WidgetError::BudgetExceeded { max, .. } if *max == ocs_plugin_api::scene::MAX_PATCH_OPS => {
            reject(PatchRejectCode::OpBudgetExceeded, Some(head))
        }
        _ => reject(PatchRejectCode::InvalidOp, Some(head)),
    }
}

// ---------------------------------------------------------------------------
// SCN-015 — the prompt half: the armed prompt's dialog desc, the decision
// vocabulary, and the modal-body builder.
//
// The CONFIRMATION LIFECYCLE's semantic half is the frozen SWG-003
// dispatcher (its `PromptStage` captures command + targets at arm time and
// `confirm`/`dismiss_prompt`/`settle` own the machine edges). THIS section
// is the interaction half the mount owns:
//
// - the arm records a SURFACE-READY desc ([`MountPromptDesc`]) through the
//   existing bridge hook ([`MountRegistry::mount_record_prompt_armed_owned`] —
//   the one `PromptRequested` routing is where both handlers meet);
// - the desc accessor ([`MountRegistry::prompt_dialog_owned`]) is the app modal
//   integration's READ: the content + address a confirm modal displays and
//   routes back by;
// - the decisions ride their own vocabulary ([`WidgetPromptMsg`]) →
//   `Message::WidgetPromptMsg` (tail-append, host-local only) →
//   `app::update::widget_prompt` (the confirm/dismiss routing reusing the
//   SCN-014 bridge's sink + settle policy).
//
// The modal's VISUAL surface (overlay/backdrop placement inside the app's
// existing modal machinery) LANDED with SCN-016 (the `ModalKind::WidgetPrompt`
// content arm); the mount side was COMPLETE before it: this is the full
// headless lifecycle (arm → desc → decision → machine edge → counters),
// and the desc accessor + the [`prompt_modal_element`] body builder are
// exactly the surface that wiring consumes.
// ---------------------------------------------------------------------------

/// One armed confirmation's SURFACE-READY dialog desc (SCN-015): the
/// complete content a confirm modal displays and routes back by.
///
/// Fields (the desc contract, exact — pinned by
/// `prompt_desc_accessor_shape`):
/// - `surface_id` — the surface the armed interaction belongs to (the
///   registry's key; the panel id's join is
///   [`MountRegistry::handle_for_surface_owned`]'s reverse direction);
/// - `prompt` — the prompt text the modal displays (the machine's captured
///   stage text — [`WidgetDispatcher::pending_prompt`]);
/// - `command_id` — the captured command the confirm will emit;
/// - `targets` — the captured targets the confirm will emit;
/// - `key` — the armed address the dialog's decision routes back through
///   ([`WidgetDispatcher::confirm`] / [`WidgetDispatcher::dismiss_prompt`]).
///
/// THE DESC IS THE DISPLAY COPY, not the exchange's truth: the SWG-003
/// dispatcher's own `PromptStage` holds the armed stage, and the confirm
/// emission re-reads THAT (a concurrent re-sync cannot retarget it). Every
/// field above is captured AT ARM TIME from the same synchronous scope the
/// arming `dispatch` resolved (see
/// [`MountRegistry::resolve_prompt_desc_owned`]) — the panel-id/surface-id pair
/// is the registry's surface key (the desc names the interaction, so both
/// vocabularies resolve from it). Host-local only, never serialized (§3).
///
// Constructed on every live arm (the `PromptRequested` routing); since
// SCN-016 the modal-content arm is the READ consumer in production too.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountPromptDesc {
    /// The surface the armed interaction belongs to (the registry's key).
    pub surface_id: StableId,
    /// The armed address the dialog's decision routes back by.
    pub key: DispatchKey,
    /// The prompt text the modal displays (the machine's captured stage).
    pub prompt: String,
    /// The captured command the confirm will emit.
    pub command_id: String,
    /// The captured targets the confirm will emit.
    pub targets: Vec<StableId>,
}

/// The prompt dialog's decision vocabulary (SCN-015): what the armed
/// confirmation's modal emits for one user decision, FULLY addressed
/// (surface + the desc's key) so the routing can refuse a decision whose
/// armed interaction is gone GRACEFULLY — the mount bridge's
/// unknown-surface pattern, mirrored (refusals move only their own probe
/// counter; nothing machines; no panics).
///
/// Host-local, NOT a wire change: the payload rides the app's
/// `Message::WidgetPromptMsg` variant (tail-append; `Message` carries no
/// serialization derives — restate by grep — so the variant has no wire
/// implication) and routes through `app::update::widget_prompt`
/// ([`route_prompt_decision`]'s confirm/dismiss edges over the SAME
/// dispatcher the click path used).
// The vocabulary is production-reachable end-to-end (the update arm's
// handler matches these variants); since SCN-016 the modal body's buttons
// emit it for real (the body builder inside `ModalKind::WidgetPrompt`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WidgetPromptMsg {
    /// The OK decision: the user confirmed the armed interaction.
    Confirmed {
        /// The surface the armed interaction belongs to.
        surface_id: StableId,
        /// The armed address the dialog displayed (the desc's key).
        key: DispatchKey,
    },
    /// The cancel decision: the armed interaction clears, no event ever
    /// exists for it; a later re-click re-prompts (the policy is per-def).
    Dismissed {
        /// The surface the armed interaction belongs to.
        surface_id: StableId,
        /// The armed address the dialog displayed (the desc's key).
        key: DispatchKey,
    },
}

/// `WidgetPromptMsg` → app `Message` — the ONE mapping site for the prompt
/// dialog's decisions (mirrors [`app_message`]). The modal body's buttons
/// emit through this; the app's update arm unwraps the variant back into
/// the prompt handler (`app::update::widget_prompt`).
///
// Landed as the ONE mapping the prompt modal's body (SCN-016) maps its
// element through — `app::update::widget_prompt` unwraps it back.
pub(crate) fn prompt_app_message(msg: WidgetPromptMsg) -> crate::app::Message {
    crate::app::Message::WidgetPromptMsg(msg)
}

/// The query the desc asks for one BUTTON address (SCN-015's arm-time
/// resolution): the surface's published command for the button — the defs
/// the mount renders FROM (the same source the shipped read model synced).
/// Plain explicit-stack walk (the ring's discipline); visibility does NOT
/// gate the read — the dispatcher's sync set is def-complete, not
/// render-complete.
fn surface_button_command(surface: &WidgetSurface, id: StableId) -> Option<String> {
    let mut walker: Vec<&WidgetNode> = surface.roots.iter().collect();
    while let Some(node) = walker.pop() {
        if let WidgetNode::Group(def) = node {
            walker.extend(def.children.iter());
            continue;
        }
        if let WidgetNode::IconButton(def) = node {
            if def.id == id {
                return Some(def.command.clone());
            }
        }
    }
    None
}

/// The armed prompt's MODAL BODY facts (SCN-015) — the headless assertion
/// surface for what [`prompt_modal_element`] renders, the [`MountFacts`]
/// pattern: prompt text + the decision pair's labels.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountPromptFacts {
    /// The message text the body displays (the desc's prompt, verbatim).
    pub prompt: String,
    /// The OK button's label (its press routes `WidgetPromptMsg::Confirmed`).
    pub ok: String,
    /// The Cancel button's label (`WidgetPromptMsg::Dismissed`).
    pub cancel: String,
}

/// Build the armed prompt's modal BODY — the pure builder over `desc`
/// (SCN-015): the prompt text, then the decision pair
/// (Cancel → [`WidgetPromptMsg::Dismissed`], OK →
/// [`WidgetPromptMsg::Confirmed`], each addressed with the desc's
/// (surface, key)). The app's REAL modal form (overlay placement,
/// backdrop, Enter/Escape key handling) is the app's existing modal
/// machinery — SCN-016 hosts this element inside it
/// (`ModalKind::WidgetPrompt`'s content); the mount side stays the plain
/// body (its labels' semantics: Cancel DISMISSES — no event — and OK
/// CONFIRMS — exactly one captured command — the app modal policy applied
/// onto the machine's two edges).
///
// Landed: the modal-content arm maps the element through
// [prompt_app_message]; the facts stay the headless tests' surface (the
// element shape is un-introspectable without a renderer).
pub(crate) fn prompt_modal_element(
    desc: &MountPromptDesc,
) -> (Element<'static, WidgetPromptMsg>, MountPromptFacts) {
    let surface_id = desc.surface_id;
    let key = desc.key.clone();
    let facts = MountPromptFacts {
        prompt: desc.prompt.clone(),
        ok: "OK".to_string(),
        cancel: "Cancel".to_string(),
    };
    let body = column![
        text(desc.prompt.clone()).size(MOUNT_TEXT_SIZE),
        row![
            button(text(facts.cancel.clone()).size(MOUNT_TEXT_SIZE))
                .padding([3, 10])
                .style(button_style(false))
                .on_press(WidgetPromptMsg::Dismissed { surface_id, key: key.clone() }),
            button(text(facts.ok.clone()).size(MOUNT_TEXT_SIZE))
                .padding([3, 10])
                .style(button_style(true))
                .on_press(WidgetPromptMsg::Confirmed { surface_id, key }),
        ]
        .spacing(MOUNT_GROUP_SPACING),
    ]
    .spacing(MOUNT_GROUP_SPACING)
    .width(Fill);

    (
        container(body).padding([12, 16]).width(Fill).into(),
        facts,
    )
}

impl MountRegistry {
    /// The armed prompt's dialog desc for `surface_id` (SCN-015) — the
    /// app modal's READ (`ModalKind::WidgetPrompt`'s content, landed
    /// SCN-016): `Some(desc)` while the surface's armed
    /// interaction awaits its decision; the desc STAYS in the registry
    /// (read-only) until the interaction completes — the decision routes
    /// ([`route_prompt_decision`] and Escape's cancel) are the consumption
    /// points, so a view pass may re-read per frame while the dialog shows.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn prompt_dialog_owned(&self, surface_id: StableId) -> Option<MountPromptDesc> {
        self.prompts.get(&surface_id).cloned()
    }

    /// The desc's CONSUMPTION (the two completion edges call this — the
    /// decision routes and the Escape cancel): remove the surface's armed
    /// desc ONLY when it still names `key` (a newer arm on the same
    /// surface survives a stale decision's cleanup), and clear the escape
    /// hint when it names the same address (a completed interaction can no
    /// longer be cancelled). Returns whether the desc was taken.
    pub(crate) fn take_prompt_dialog_owned(&mut self, surface_id: StableId, key: &DispatchKey) -> bool {
        // The key match reads FIRST, then the removal — a newer arm on the
        // same surface survives a stale decision's cleanup.
        let names_this_address = self
            .prompts
            .get(&surface_id)
            .is_some_and(|armed| &armed.key == key);
        let taken = if names_this_address {
            self.prompts.remove(&surface_id);
            true
        } else {
            false
        };
        if self.input.escape_cancel.get(&surface_id) == Some(key) {
            self.input.escape_cancel.remove(&surface_id);
        }
        taken
    }

    /// The confirmed-decision probe tick (§16).
    pub(crate) fn record_prompt_confirmed_owned(&mut self) {
        self.counters.prompts_confirmed += 1;
    }

    /// The dismissed-decision probe tick (§16) — the modal's Cancel and
    /// the machine-direct Escape cancel are the same edge; both tick here.
    pub(crate) fn record_prompt_dismissed_owned(&mut self) {
        self.counters.prompts_dismissed += 1;
    }

    /// The refused-decision probe tick (§16) — a decision that named no
    /// pending interaction; moves ONLY its own counter.
    pub(crate) fn record_prompt_refused_owned(&mut self) {
        self.counters.prompts_refused += 1;
    }
}

// ---------------------------------------------------------------------------
// THE cfg(test) SEAM (SCN-016's app-owned registry migration): the storage
// that was a PRODUCTION thread_local moves ONTO the app
// (`OpenCADStudio::mount_registry`); every registry accessor is an INSTANCE
// method over that app-owned store (the `*_owned` forms above — the
// production call sites touch ONLY the app field). What stays here, under
// `cfg(test)` ONLY, is the test-friendly STATIC form the headless tests
// already pin: the same names/signatures the
// SCN-014/015/020 test modules (here and in `app::update`) drive, backed by
// a cfg(test)-only thread-local cell that delegates to the SAME instance
// forms — so the tests keep exercising the production logic with ZERO call
// churn. The production grep guarantee holds by construction: a `grep
// MOUNT_REGISTRY` outside cfg(test) code has nothing to find.
// ---------------------------------------------------------------------------

#[cfg(test)]
thread_local! {
    static MOUNT_REGISTRY: RefCell<std::rc::Rc<RefCell<MountRegistry>>> =
        RefCell::new(std::rc::Rc::new(RefCell::new(MountRegistry::default())));
}

/// Bind the headless tests' cell to the app's own mount store (`Rc` — the
/// app's field and the facades' delegation target become ONE storage; see
/// `OpenCADStudio::new_for_test`, the single caller).
#[cfg(test)]
pub(crate) fn test_bind_app_store(store: std::rc::Rc<RefCell<MountRegistry>>) {
    MOUNT_REGISTRY.with(|cell| {
        *cell.borrow_mut() = store;
    });
}

#[cfg(test)]
pub(crate) fn with_test_registry<R>(read: impl FnOnce(&MountRegistry) -> R) -> R {
    let store = MOUNT_REGISTRY.with(|cell| cell.borrow().clone());
    let guard = store.borrow();
    let value = read(&guard);
    value
}

#[cfg(test)]
pub(crate) fn with_test_registry_mut<R>(mutate: impl FnOnce(&mut MountRegistry) -> R) -> R {
    let store = MOUNT_REGISTRY.with(|cell| cell.borrow().clone());
    let mut guard = store.borrow_mut();
    let value = mutate(&mut guard);
    value
}

#[cfg(test)]
impl MountRegistry {
    /// The raw registration primitive's test form — see
    /// [`Self::register_owned`].
    #[allow(dead_code)]
    pub(crate) fn register(panel_id: &str, handle: SurfaceMountHandle) {
        with_test_registry_mut(|registry| registry.register_owned(panel_id, handle));
    }

    /// The raw removal's test form — see [`Self::unregister_owned`].
    #[allow(dead_code)]
    pub(crate) fn unregister(panel_id: &str) {
        with_test_registry_mut(|registry| registry.unregister_owned(panel_id));
    }

    /// The gate read's test form — see [`Self::mount_registered_owned`].
    pub(crate) fn mount_registered(panel_id: &str) -> bool {
        with_test_registry(|registry| registry.mount_registered_owned(panel_id))
    }

    /// The mounted-body read's test form — see [`Self::mounted_body_owned`].
    pub(crate) fn mounted_body(panel_id: &str) -> Option<Element<'static, WidgetMountMsg>> {
        with_test_registry(|registry| registry.mounted_body_owned(panel_id))
    }

    /// The PRESERVE-ON-IDENTICAL lifecycle's test form — see
    /// [`Self::register_surface_owned`].
    pub(crate) fn register_surface(panel_id: &str, handle: SurfaceMountHandle) {
        with_test_registry_mut(|registry| registry.register_surface_owned(panel_id, handle));
    }

    /// The teardown's test form — see [`Self::teardown_surface_owned`].
    pub(crate) fn teardown_surface(panel_id: &str) {
        with_test_registry_mut(|registry| registry.teardown_surface_owned(panel_id));
    }

    /// The surface lookup's test form — see
    /// [`Self::handle_for_surface_owned`].
    pub(crate) fn handle_for_surface(surface_id: StableId) -> Option<SurfaceMountHandle> {
        with_test_registry(|registry| registry.handle_for_surface_owned(surface_id))
    }

    /// The counters read's test form — see [`Self::counters_owned`].
    pub(crate) fn counters() -> MountCounters {
        with_test_registry(|registry| registry.counters_owned())
    }

    /// The coalescer ENTRY's test form — see
    /// [`Self::mount_move_defer_owned`].
    pub(crate) fn mount_move_defer(surface_id: StableId, position: Point) -> Option<bool> {
        with_test_registry_mut(|registry| registry.mount_move_defer_owned(surface_id, position))
    }

    /// The coalescer FLUSH's test form — see
    /// [`Self::mount_flush_moves_owned`].
    pub(crate) fn mount_flush_moves() -> usize {
        with_test_registry_mut(|registry| registry.mount_flush_moves_owned())
    }

    /// The press route's test form — see [`Self::mount_press_owned`].
    pub(crate) fn mount_press(surface_id: StableId) -> bool {
        with_test_registry_mut(|registry| registry.mount_press_owned(surface_id))
    }

    /// The release completion's test form — see
    /// [`Self::mount_pointer_released_owned`].
    pub(crate) fn mount_pointer_released(surface_id: StableId) -> usize {
        with_test_registry_mut(|registry| registry.mount_pointer_released_owned(surface_id))
    }

    /// The hover write's test form — see [`Self::mount_set_hover_owned`].
    #[allow(dead_code)]
    pub(crate) fn mount_set_hover(surface_id: StableId, row: StableId) -> bool {
        with_test_registry_mut(|registry| registry.mount_set_hover_owned(surface_id, row))
    }

    /// The hover read's test form — see [`Self::mount_hover_owned`].
    #[allow(dead_code)]
    pub(crate) fn mount_hover(surface_id: StableId) -> Option<StableId> {
        with_test_registry(|registry| registry.mount_hover_owned(surface_id))
    }

    /// The hover-snapshot read's test form — see
    /// [`Self::mount_hover_state_owned`].
    #[allow(dead_code)]
    pub(crate) fn mount_hover_state(surface_id: StableId) -> Option<HoverState> {
        with_test_registry(|registry| registry.mount_hover_state_owned(surface_id))
    }

    /// The capture read's test form — see [`Self::mount_captured_owned`].
    pub(crate) fn mount_captured(surface_id: StableId) -> bool {
        with_test_registry(|registry| registry.mount_captured_owned(surface_id))
    }

    /// The panel→surface read's test form — see
    /// [`Self::panel_surface_id_owned`].
    pub(crate) fn panel_surface_id(panel_id: &str) -> Option<StableId> {
        with_test_registry(|registry| registry.panel_surface_id_owned(panel_id))
    }

    /// The keyboard target resolution's test form — see
    /// [`Self::mount_key_surface_owned`].
    pub(crate) fn mount_key_surface() -> Option<StableId> {
        with_test_registry(|registry| registry.mount_key_surface_owned())
    }

    /// The keyboard-grab seam's test form — see
    /// [`Self::set_mount_key_target_owned`].
    #[allow(dead_code)]
    pub(crate) fn set_mount_key_target(surface_id: Option<StableId>) {
        with_test_registry_mut(|registry| registry.set_mount_key_target_owned(surface_id));
    }

    /// The prompt-arm record's test form — see
    /// [`Self::mount_record_prompt_armed_owned`].
    #[allow(dead_code)]
    pub(crate) fn mount_record_prompt_armed(surface_id: StableId, key: DispatchKey) {
        with_test_registry_mut(|registry| {
            registry.mount_record_prompt_armed_owned(surface_id, key)
        });
    }

    /// Escape's mounted-surface cancel's test form — see
    /// [`Self::mount_escape_cancel_owned`].
    #[allow(dead_code)]
    pub(crate) fn mount_escape_cancel(surface_id: StableId) -> bool {
        with_test_registry_mut(|registry| registry.mount_escape_cancel_owned(surface_id))
    }

    /// The prompt desc read's test form — see [`Self::prompt_dialog_owned`].
    pub(crate) fn prompt_dialog(surface_id: StableId) -> Option<MountPromptDesc> {
        with_test_registry(|registry| registry.prompt_dialog_owned(surface_id))
    }

    /// The desc consumption's test form — see
    /// [`Self::take_prompt_dialog_owned`].
    pub(crate) fn take_prompt_dialog(surface_id: StableId, key: &DispatchKey) -> bool {
        with_test_registry_mut(|registry| registry.take_prompt_dialog_owned(surface_id, key))
    }
}

// ---------------------------------------------------------------------------
// Tests (headless: NO renderer, NO Tasks — structure + semantics)
//
// iced Elements cannot be introspected deeply, so every test asserts via:
// (a) the state-machine deltas the builders/mount drive (`apply_mount_msg`
//     over the SAME machines; sink = Vec<WidgetEvent>), and
// (b) the `materialized_row_count_*` helpers (the VISIBLE-WINDOW count —
//     the windowing proof: a 100k-item list materializes a few dozen rows,
//     NOT 100k), and
// (c) the builders' `MountFacts` descriptors (the headless assertion
//     surface for what the element renders and routes).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use ocs_plugin_api::dialog::PanelDef;
    use ocs_plugin_api::scene::SceneRevision;
    use ocs_plugin_api::widgets::{
        CellKind, ConfirmPolicy, IconRef, ItemDef, MenuItemDef, PanelTableDef, TableCell,
        TableColumnDef, TableRowDef, TreeCapabilities, TreeRowDef, WidgetCellValue, WidgetRange,
    };

    // -- construction helpers --------------------------------------------------

    fn sid(n: u64) -> StableId {
        StableId(n)
    }

    fn window_spec(viewport_rows: f32, margin_rows: usize) -> WindowSpec {
        WindowSpec {
            viewport_height: viewport_rows,
            row_height: 1.0,
            margin_rows,
        }
    }

    fn tree_row(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
        TreeRowDef {
            id: sid(id),
            parent: parent.map(sid),
            label: label.to_string(),
            icon: None,
            status: None,
            expandable: false,
            enabled: true,
            lazy_children: false,
            domain_ref: None,
        }
    }

    fn expandable_row(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
        TreeRowDef {
            expandable: true,
            ..tree_row(id, parent, label)
        }
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

    fn tree(id: u64, rows: Vec<TreeRowDef>, multi_select: bool) -> WidgetNode {
        WidgetNode::Tree(WidgetTreeDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            capabilities: TreeCapabilities {
                multi_select,
                filterable: false,
                lazy_children: false,
            },
            rows,
        })
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

    fn table_ref(id: u64, table_id: u64) -> TableRef {
        TableRef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            table_id: sid(table_id),
        }
    }

    fn group_def(
        id: u64,
        direction: Axis,
        ratio: Option<f32>,
        children: Vec<WidgetNode>,
    ) -> WidgetGroup {
        WidgetGroup {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            direction,
            children,
            split_ratio: ratio,
        }
    }

    fn invisible(node: WidgetNode) -> WidgetNode {
        let mut node = node;
        match &mut node {
            WidgetNode::Tree(def) => def.visible = false,
            WidgetNode::List(def) => def.visible = false,
            WidgetNode::IconButton(def) => def.visible = false,
            WidgetNode::Menu(def) => def.visible = false,
            WidgetNode::Table(def) => def.visible = false,
            WidgetNode::Group(def) => def.visible = false,
        }
        node
    }

    fn visible(node: WidgetNode) -> WidgetNode {
        let mut node = node;
        match &mut node {
            WidgetNode::Tree(def) => def.visible = true,
            WidgetNode::List(def) => def.visible = true,
            WidgetNode::IconButton(def) => def.visible = true,
            WidgetNode::Menu(def) => def.visible = true,
            WidgetNode::Table(def) => def.visible = true,
            WidgetNode::Group(def) => def.visible = true,
        }
        node
    }

    /// A state opened + synced for one fixture surface (surface id 900).
    fn synced_state(surface: &WidgetSurface) -> WidgetUiState {
        let mut state = WidgetUiState::new(sid(900));
        state.sync(surface);
        state
    }

    /// Synced state + dispatcher pair over one fixture surface.
    fn mounted_machines(surface: &WidgetSurface) -> (WidgetUiState, WidgetDispatcher) {
        (synced_state(surface), WidgetDispatcher::new(sid(900)))
    }

    /// Select `ids` through the state machine (the selection the command
    /// routing will name as targets).
    fn select(state: &mut WidgetUiState, list_id: u64, ids: Vec<u64>) {
        let view = state
            .synced()
            .expect("synced")
            .widget_views
            .get(&sid(list_id))
            .copied()
            .expect("the list widget view");
        state
            .set_selection(ids.iter().map(|id| sid(*id)).collect(), sid(list_id), &view)
            .expect("selection applies");
    }

    fn route(
        state: &mut WidgetUiState,
        dispatcher: &mut WidgetDispatcher,
        msg: &WidgetMountMsg,
    ) -> (MountRouting, Vec<WidgetEvent>) {
        let mut events = Vec::new();
        let routing = apply_mount_msg(state, dispatcher, msg, &mut events);
        (routing, events)
    }

    fn click_msg(surface_id: u64, widget_id: u64) -> WidgetMountMsg {
        WidgetMountMsg::ButtonClicked {
            surface_id: sid(surface_id),
            widget_id: sid(widget_id),
        }
    }

    /// A scoped registry entry with a UNIQUE panel id; the guard
    /// unregisters on drop (thread-local hygiene across the test pool).
    struct RegisteredPanel(String);

    fn register_panel(panel_id: &str, surface_id: u64, surface: WidgetSurface) -> RegisteredPanel {
        MountRegistry::register(
            panel_id,
            SurfaceMountHandle::from_surface(sid(surface_id), surface),
        );
        RegisteredPanel(panel_id.to_string())
    }

    impl Drop for RegisteredPanel {
        fn drop(&mut self) {
            MountRegistry::unregister(&self.0);
        }
    }

    // -- 1. windowing: a 100k list materializes the window, not 100k ----------

    /// §16 "100k backing objects do not create 100k host widgets": the
    /// mount's materialization count is the VISIBLE window (a few dozen
    /// rows), independent of the backing size and stable across scroll
    /// anchors.
    #[test]
    fn mount_materializes_visible_window_only() {
        let items: Vec<ItemDef> = (1..=100_000)
            .map(|i| item(i as u64, &format!("item {i}")))
            .collect();
        let def = item_list(100, items);
        let mut state = synced_state(&surface(vec![WidgetNode::List(def.clone())]));

        // Top anchor: 20 viewport rows + the 4-row warm margin (the top
        // margin saturates at 0 — nothing is above row 0, so 24 rows).
        let count = materialized_row_count_of_list(&def, &state, &window_spec(20.0, 4));
        assert_eq!(
            count, 24,
            "viewport (20) + top-clamped + bottom margin, not the full 100k"
        );

        // Scrolled to the end: still a window-sized materialization.
        state.set_scroll_row(99_999);
        let count = materialized_row_count_of_list(&def, &state, &window_spec(20.0, 4));
        assert_eq!(
            count, 24,
            "the end's window: 4 margin rows + the 20-row tail"
        );

        // Mid-list: the count is the spec's, never the size's.
        state.set_scroll_row(50_000);
        assert_eq!(
            materialized_row_count_of_list(&def, &state, &window_spec(20.0, 4)),
            28
        );

        // The TREE sibling reads the same windowing math.
        let tree_def = WidgetTreeDef {
            id: sid(100),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            capabilities: TreeCapabilities {
                multi_select: false,
                filterable: false,
                lazy_children: false,
            },
            rows: vec![tree_row(1, None, "A"), tree_row(2, None, "B")],
        };
        let tree_state = synced_state(&surface(vec![WidgetNode::Tree(tree_def.clone())]));
        assert_eq!(
            materialized_row_count_of_tree(&tree_def, &tree_state, &window_spec(20.0, 4)),
            2,
            "a 2-row tree shows 2 rows — the window never inflates"
        );
    }

    // -- 2. a disabled icon button hides its press AND refuses -----------------

    /// The enablement pick, both halves: the builder renders NO press route
    /// (the `MountFacts` descriptor says so headless), and the dispatcher's
    /// refusal — the semantic authority — still holds the line even when a
    /// press is routed regardless (belt and braces).
    #[test]
    fn mount_of_disabled_button_yields_no_press_route() {
        let def = IconActionDef {
            enabled: false,
            ..button_def(1, "Delete selection", "delete")
        };
        let surface = surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]);
        let (mut state, mut dispatcher) = mounted_machines(&surface);
        dispatcher.sync_button(&def);

        let (element, facts) = icon_button_element(&def, &dispatcher);
        assert_eq!(facts.widget_id, sid(1));
        assert!(!facts.clickable, "a disabled button carries no press route");
        assert_eq!(facts.a11y, "Delete selection", "the required name verbatim");
        assert_eq!(facts.command.as_deref(), Some("delete"));
        // The element exists headless (built without a renderer, not drawn).
        let _ = element;

        // The semantic authority: route the click anyway — the dispatcher
        // refuses, NO event leaves the mount, only ITS counter moves.
        select(&mut state, 200, vec![7]);
        let (routing, events) = route(&mut state, &mut dispatcher, &click_msg(900, 1));
        assert_eq!(
            routing,
            MountRouting::Refused("widget 1 is disabled and cannot be invoked".to_string())
        );
        assert_eq!(events, Vec::new(), "no event exists");
        assert_eq!(dispatcher.counters().activations_refused_disabled, 1);
        assert_eq!(dispatcher.counters().commands_emitted, 0);
    }

    // -- 3. expansion toggles through the state machine ------------------------

    /// A row press's expansion routes through `toggle_expand` (the state
    /// layers own the truth): one toggle = exactly ONE row-state write + ONE
    /// TreeExpanded notification; the flat list grows only by that branch;
    /// a stale duplicate press is a benign no-op.
    #[test]
    fn expansion_toggle_tracks_state_machine() {
        let surface = surface(vec![tree(
            100,
            vec![expandable_row(1, None, "A"), tree_row(11, Some(1), "A.1")],
            false,
        )]);
        let mut state = synced_state(&surface);
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        let def = match &surface.roots[0] {
            WidgetNode::Tree(def) => def.clone(),
            other => panic!("expected a tree, got {other:?}"),
        };
        let views = state.synced().expect("synced").views.clone();

        let (routing, events) = route(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::RowExpandedToggled {
                surface_id: sid(900),
                widget_id: sid(100),
                row_id: sid(1),
                expanded: true,
            },
        );
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(
            events,
            vec![WidgetEvent::TreeExpanded {
                surface_id: sid(900),
                node_id: sid(1),
                expanded: true,
            }]
        );
        assert_eq!(state.explicit_expansion(sid(1)), Some(true));
        assert_eq!(
            state.mutations().row_state_changes,
            1,
            "exactly one row write"
        );
        assert_eq!(state.mutations().whole_state_resets, 0);
        assert_eq!(
            state.flat_rows_of_tree(&def, &views).len(),
            2,
            "the branch's child is in the flat list"
        );

        // The stale duplicate press: already expanded → benign no-op.
        let (routing, events) = route(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::RowExpandedToggled {
                surface_id: sid(900),
                widget_id: sid(100),
                row_id: sid(1),
                expanded: true,
            },
        );
        assert_eq!(routing, MountRouting::Applied { forwarded: 0 });
        assert_eq!(events, Vec::new(), "no event re-emitted");
        assert_eq!(state.mutations().row_state_changes, 1, "no extra write");

        // Toggle back off: the notification carries the new state; the flat
        // list shrinks by the branch.
        let (routing, events) = route(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::RowExpandedToggled {
                surface_id: sid(900),
                widget_id: sid(100),
                row_id: sid(1),
                expanded: false,
            },
        );
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(
            events,
            vec![WidgetEvent::TreeExpanded {
                surface_id: sid(900),
                node_id: sid(1),
                expanded: false,
            }]
        );
        assert_eq!(state.flat_rows_of_tree(&def, &views).len(), 1);
        assert_eq!(
            state.mutations().row_state_changes,
            2,
            "two toggles, two writes"
        );
    }

    // -- 4. selection capability gates -----------------------------------------

    /// A single-select tree refuses a multi-id offer (the state machine's
    /// gate — no state change, no event); its single-id offer applies; a
    /// LIST (no capability gate) takes a 2-of-N selection as a diff write.
    #[test]
    fn selection_and_capability_gates_hold() {
        let surface = surface(vec![
            tree(
                100,
                vec![tree_row(1, None, "A"), tree_row(2, None, "B")],
                false,
            ),
            WidgetNode::List(item_list(
                200,
                vec![item(3, "one"), item(4, "two"), item(5, "three")],
            )),
        ]);
        let (mut state, mut dispatcher) = mounted_machines(&surface);

        let expected = WidgetStateError::MultiSelectNotCapable {
            widget: sid(100),
            offered: 2,
        };
        let (routing, events) = route(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::SelectionChanged {
                surface_id: sid(900),
                widget_id: sid(100),
                selected: vec![sid(1), sid(2)],
            },
        );
        assert_eq!(routing, MountRouting::Refused(expected.to_string()));
        assert_eq!(events, Vec::new(), "no event on a refusal");
        assert!(state.selection().is_empty(), "nothing half-applied");
        assert_eq!(state.mutations().row_state_changes, 0);
        assert_eq!(state.mutations().whole_state_resets, 0);

        // The tree's single selection applies.
        let (routing, events) = route(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::SelectionChanged {
                surface_id: sid(900),
                widget_id: sid(100),
                selected: vec![sid(1)],
            },
        );
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(events.len(), 1);
        assert_eq!(state.selection(), [sid(1)]);

        // A list: 2-of-3 accepted; the write touches only the diff — the
        // namespace is surface-wide, so the tree row leaves the selection
        // (1 removed) and 2 list rows are added: 3 rows touched, not 5.
        let changes_before = state.mutations().row_state_changes;
        let (routing, _) = route(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::SelectionChanged {
                surface_id: sid(900),
                widget_id: sid(200),
                selected: vec![sid(3), sid(4)],
            },
        );
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(
            state.mutations().row_state_changes - changes_before,
            3,
            "the diff write: removed 1 + added 2, not the whole namespace"
        );
        assert_eq!(state.selection(), [sid(3), sid(4)]);
    }

    // -- 5. the confirmation lifecycle through the mounting edge ---------------

    /// A `Prompt`-policy button through the MOUNT's edge: the first click
    /// requests the prompt (no event; the machine arms); the second dispatch
    /// refuses in flight; `confirm` emits the captured command exactly once;
    /// `settle` clears the mark.
    #[test]
    fn confirmation_prompt_lifecycle_drives_dispatcher_edges() {
        let surface = surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]);
        let (mut state, mut dispatcher) = mounted_machines(&surface);
        let def = IconActionDef {
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Really delete the selection?".to_string(),
            }),
            ..button_def(1, "Delete selection", "delete")
        };
        dispatcher.sync_button(&def);
        select(&mut state, 200, vec![7]);

        let clicked = click_msg(900, 1);

        // First click: the prompt is REQUESTED; no event; the guard arms.
        let (routing, events) = route(&mut state, &mut dispatcher, &clicked);
        assert_eq!(
            routing,
            MountRouting::PromptRequested {
                prompt: "Really delete the selection?".to_string(),
            }
        );
        assert_eq!(events, Vec::new(), "no command event exists yet");
        assert_eq!(dispatcher.counters().confirmations_requested, 1);
        assert_eq!(dispatcher.counters().commands_emitted, 0);

        // The click while armed: refused in flight; no re-prompt.
        let (routing, events) = route(&mut state, &mut dispatcher, &clicked);
        assert_eq!(
            routing,
            MountRouting::Refused(
                "widget 1 is already dispatched and awaiting settlement".to_string()
            )
        );
        assert_eq!(events, Vec::new());
        assert_eq!(dispatcher.counters().activations_refused_in_flight, 1);
        assert_eq!(dispatcher.counters().confirmations_requested, 1);

        // The user confirms: the captured command + targets emit exactly once.
        let outcome = dispatcher
            .confirm(DispatchKey::Button(sid(1)))
            .expect("the armed stage the dialog owns");
        let released = match outcome {
            DispatchOutcome::ConfirmedEmitted(event) => event,
            other => panic!("expected ConfirmedEmitted, got {other:?}"),
        };
        assert_eq!(
            released,
            WidgetEvent::CommandInvoked {
                surface_id: sid(900),
                command_id: "delete".to_string(),
                target_ids: vec![sid(7)],
            },
            "the dispatch-captured command + targets, exactly once"
        );
        assert_eq!(dispatcher.counters().confirmations_completed, 1);
        assert_eq!(dispatcher.counters().commands_emitted, 1);

        // The bridge settles the forwarded event; nothing lingers.
        assert!(dispatcher.settle(DispatchKey::Button(sid(1))));
        assert_eq!(
            dispatcher.pending_prompt(&DispatchKey::Button(sid(1))),
            None
        );
    }

    // -- 6. group split ratio clamping -----------------------------------------

    /// The documented host-side clamp: a wild ratio bounds into (0,1);
    /// `0.5` passes through; non-finite lands on the neutral split; the
    /// portions map at 1/1000 resolution sums exactly; non-2-child groups
    /// and `None` plainly stack.
    #[test]
    fn group_split_ratio_clamped() {
        assert_eq!(clamp_split_ratio(3.7), 0.99);
        assert!(clamp_split_ratio(3.7) > 0.0 && clamp_split_ratio(3.7) < 1.0);
        assert_eq!(clamp_split_ratio(0.5), 0.5);
        assert_eq!(clamp_split_ratio(-2.0), 0.01);
        assert_eq!(clamp_split_ratio(f32::INFINITY), 0.5);
        assert_eq!(clamp_split_ratio(f32::NAN), 0.5);

        let two_children = group_def(
            300,
            Axis::Horizontal,
            Some(0.3),
            vec![
                WidgetNode::Table(table_ref(301, 900)),
                WidgetNode::Table(table_ref(302, 900)),
            ],
        );
        let (left, right) = split_portions(&two_children).expect("a 2-child ratio group splits");
        assert_eq!((left, right), (300, 700), "exact portions");
        assert_eq!(left + right, 1_000, "portions sum exactly");

        let one_child = group_def(
            310,
            Axis::Horizontal,
            Some(0.3),
            vec![WidgetNode::Table(table_ref(311, 900))],
        );
        assert_eq!(
            split_portions(&one_child),
            None,
            "a ratio names a TWO-pane split"
        );

        let no_ratio = group_def(
            320,
            Axis::Horizontal,
            None,
            vec![
                WidgetNode::Table(table_ref(321, 900)),
                WidgetNode::Table(table_ref(322, 900)),
            ],
        );
        assert_eq!(split_portions(&no_ratio), None, "None = plain stacking");

        // The builders are total: the split path AND the plain path build
        // elements headless from any children list.
        let _element = group_element(&two_children, vec![text("").into(), text("").into()]);
        let _plain = group_element(
            &group_def(
                330,
                Axis::Vertical,
                Some(0.7),
                vec![
                    WidgetNode::Table(table_ref(331, 900)),
                    WidgetNode::Table(table_ref(332, 900)),
                ],
            ),
            vec![text("").into(), text("").into(), text("").into()],
        );
        let _empty = group_element(&group_def(340, Axis::Vertical, None, vec![]), Vec::new());
    }

    // -- 7. invisible nodes skipped from the parent container ------------------

    /// `visible == false` nodes render NOTHING: per-variant `build_node`
    /// returns None, and a group's children list carries only the rendered
    /// ones — the parent skips (rather than hollow-slots) hidden children.
    #[test]
    fn visible_false_nodes_skipped() {
        let surface = surface(vec![WidgetNode::List(item_list(200, vec![item(7, "x")]))]);
        let state = synced_state(&surface);
        let dispatcher = WidgetDispatcher::new(sid(900));

        let nodes = vec![
            tree(100, vec![tree_row(1, None, "A")], false),
            WidgetNode::List(item_list(200, vec![item(3, "x")])),
            WidgetNode::IconButton(button_def(300, "Btn", "cmd")),
            WidgetNode::Menu(menu_def(400, vec![menu_item_def("cmd", "Item")])),
            WidgetNode::Table(table_ref(500, 900)),
        ];
        for node in nodes {
            assert!(
                build_node(
                    &invisible(node.clone()),
                    &state,
                    &dispatcher,
                    &DEFAULT_WINDOW_SPEC
                )
                .is_none(),
                "{node:?} must skip when visible == false"
            );
            assert!(
                build_node(
                    &visible(node.clone()),
                    &state,
                    &dispatcher,
                    &DEFAULT_WINDOW_SPEC
                )
                .is_some(),
                "{node:?} renders when visible == true"
            );
        }

        // The group-level skip: the children list carries only the rendered.
        let group_d = group_def(
            300,
            Axis::Vertical,
            None,
            vec![
                WidgetNode::Table(table_ref(301, 900)),
                invisible(tree(100, vec![tree_row(1, None, "hidden")], false)),
            ],
        );
        let children = visible_children(&group_d, &state, &dispatcher, &DEFAULT_WINDOW_SPEC);
        assert_eq!(
            children.len(),
            1,
            "the invisible child is skipped, not hollow"
        );
    }

    // -- 8. the legacy path is untouched by foreign registry entries -----------

    /// The gate read for a legacy state: no registry entry → false (the
    /// view's legacy path runs untouched); an entry for ANOTHER panel id
    /// stays false for THIS panel; only the exact panel id mounts; the
    /// teardown restores the legacy answer.
    #[test]
    fn legacy_path_untouched() {
        let state =
            PluginPanelState::from_def(PanelDef::new("scn013_legacy_panel", "Legacy Panel"));
        assert!(
            !mounts_surface(&state),
            "no mount registered → the legacy view() path answers"
        );

        // A mount elsewhere never gates THIS panel.
        let other = register_panel(
            "scn013_other_panel",
            700,
            surface(vec![WidgetNode::List(item_list(200, vec![item(7, "x")]))]),
        );
        assert!(
            !mounts_surface(&state),
            "a foreign registration stays invisible"
        );

        // The exact registration flips the gate (and ONLY it), and the gate
        // consults the SAME registry lookup the view's hook uses.
        let own = register_panel(
            "scn013_legacy_panel",
            701,
            surface(vec![WidgetNode::List(item_list(200, vec![item(7, "x")]))]),
        );
        assert!(mounts_surface(&state), "the registered panel mounts");
        assert!(MountRegistry::mounted_body("scn013_legacy_panel").is_some());
        drop(other);
        drop(own);
        assert!(!mounts_surface(&state), "the teardown removes the mount");
    }

    // -- 9. dispatcher refusal paths route no events ---------------------------

    /// Disabled, unknown, and cross-surface dispatches refuse with no event:
    /// the disabled refusal moves ITS OWN counter; unknown moves NOTHING.
    #[test]
    fn dispatcher_refusal_paths_route_no_events() {
        let surface = surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]);
        let (mut state, mut dispatcher) = mounted_machines(&surface);
        let disabled = IconActionDef {
            enabled: false,
            ..button_def(1, "Disabled button", "cmd")
        };
        dispatcher.sync_button(&disabled);
        select(&mut state, 200, vec![7]);

        // (a) the disabled refusal: its OWN counter, zero events.
        let (routing, events) = route(&mut state, &mut dispatcher, &click_msg(900, 1));
        assert_eq!(
            routing,
            MountRouting::Refused("widget 1 is disabled and cannot be invoked".to_string())
        );
        assert_eq!(events, Vec::new());
        assert_eq!(dispatcher.counters().activations_refused_disabled, 1);

        // (b) the unknown address: NO counter moves, no event.
        let before = *dispatcher.counters();
        let (routing, events) = route(&mut state, &mut dispatcher, &click_msg(900, 99));
        assert_eq!(
            routing,
            MountRouting::Refused("widget 99 is not in the synced dispatch read model".to_string())
        );
        assert_eq!(events, Vec::new());
        assert_eq!(
            *dispatcher.counters(),
            before,
            "an unknown refusal moves nothing"
        );

        // (c) an unknown menu address: likewise eventless and counterless.
        let (routing, events) = route(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::MenuItemClicked {
                surface_id: sid(900),
                menu_id: sid(400),
                command_id: "cmd".to_string(),
            },
        );
        assert_eq!(
            routing,
            MountRouting::Refused(
                "widget 400 is not in the synced dispatch read model".to_string()
            )
        );
        assert_eq!(events, Vec::new());
        assert_eq!(*dispatcher.counters(), before);

        // (d) settle with nothing in flight: `false` (and nothing moved).
        assert!(!dispatcher.settle(DispatchKey::Button(sid(42))));
        assert_eq!(*dispatcher.counters(), before);

        // (e) a cross-surface message refuses before the machines even run.
        let (routing, events) = route(&mut state, &mut dispatcher, &click_msg(901, 1));
        assert_eq!(
            routing,
            MountRouting::Refused(
                "message names surface 901 but the mount state owns surface 900".to_string()
            )
        );
        assert_eq!(events, Vec::new());
    }

    // -- 10. a11y labels exact ---------------------------------------------------

    /// Buttons carry the required `accessible_name` verbatim; menu items
    /// carry the composed `label (shortcut hint)`; the projection and the
    /// machine derive the SAME text (one derivation, two consumers);
    /// unnamed containers fall back to their kind name.
    #[test]
    fn a11y_labels_exact() {
        let surface = surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]);
        let state = synced_state(&surface);
        let mut dispatcher = WidgetDispatcher::new(sid(900));
        let def = button_def(1, "Delete selection", "delete");
        dispatcher.sync_button(&def);

        let (_, facts) = icon_button_element(&def, &dispatcher);
        assert_eq!(
            facts.a11y, "Delete selection",
            "the button's access name exact"
        );
        // The machine-path derivation agrees with the fact (one truth).
        assert_eq!(
            dispatcher
                .accessible_label(DispatchKey::Button(sid(1)))
                .expect("a synced button has a name"),
            facts.a11y
        );

        // Menu item: label composed with the shortcut hint, exact.
        let menu = MenuDef {
            id: sid(200),
            name: Some("Publish menu".to_string()),
            enabled: true,
            visible: true,
            label: Some("Publish".to_string()),
            items: vec![MenuItemDef {
                command_id: "unpublish".to_string(),
                label: "Unpublish".to_string(),
                icon: None,
                shortcut: Some("Ctrl+U".to_string()),
                ..menu_item_def("unpublish", "Unpublish")
            }],
        };
        dispatcher.sync_menu(&menu);
        let ControlSemantics::Menu { items, .. } = project_menu(&menu) else {
            panic!("a menu projects Menu semantics");
        };
        assert_eq!(
            items[0].label, "Unpublish (Ctrl+U)",
            "item label + hint exact"
        );
        assert_eq!(
            dispatcher
                .accessible_label(DispatchKey::MenuItem(sid(200), "unpublish".to_string()))
                .expect("a synced item has a name"),
            items[0].label,
            "the projection and the machine derive the SAME text"
        );

        // Trees/lists: the declared name or the kind fallback (documented).
        let named_tree = WidgetTreeDef {
            id: sid(100),
            name: Some("Object browse".to_string()),
            enabled: true,
            visible: true,
            label: None,
            capabilities: TreeCapabilities {
                multi_select: false,
                filterable: false,
                lazy_children: false,
            },
            rows: vec![tree_row(1, None, "A")],
        };
        let (_, tree_facts) = tree_element(&named_tree, &state, &DEFAULT_WINDOW_SPEC);
        assert_eq!(tree_facts.a11y, "Object browse");
        let (_, list_facts) = list_element(
            &item_list(200, vec![item(7, "row")]),
            &state,
            &DEFAULT_WINDOW_SPEC,
        );
        assert_eq!(list_facts.a11y, "list", "the kind fallback exact");
    }

    // -- 11. the registry-built body builds headless (the mount walk) ----------

    /// The `SurfaceMountHandle` body walk over a MIXED surface (tree + list
    /// + button + menu + table placeholder + split group) constructs the
    /// native body headlessly with the machines' state applied (expanded
    /// branch, selection), and the machines see the same `Rc` clones.
    #[test]
    fn mount_body_walk_builds_mixed_surface() {
        let surface = surface(vec![
            WidgetNode::Group(group_def(
                300,
                Axis::Vertical,
                Some(0.3),
                vec![
                    tree(
                        100,
                        vec![expandable_row(1, None, "A"), tree_row(11, Some(1), "A.1")],
                        false,
                    ),
                    WidgetNode::Table(table_ref(301, 900)),
                ],
            )),
            WidgetNode::List(item_list(200, vec![item(3, "one"), item(4, "two")])),
            WidgetNode::IconButton(button_def(9, "Btn", "cmd")),
        ]);
        let handle = SurfaceMountHandle::from_surface(sid(900), surface.clone());

        // The machines register through the SAME handle refs.
        assert_eq!(handle.surface_id(), sid(900));
        {
            let state = handle.state.borrow();
            assert!(
                state.synced().is_some(),
                "the registration synced the surface"
            );
        }
        handle
            .dispatcher
            .borrow_mut()
            .sync_button(&button_def(9, "Btn", "cmd"));

        // Drive expansion through the SHARED Rc borrows (the exact shape
        // the SCN-014 bridge will use).
        {
            let views = handle
                .state
                .borrow()
                .synced()
                .expect("synced")
                .views
                .clone();
            let mut state = handle.state.borrow_mut();
            state.toggle_expand(sid(1), &views).expect("the toggle ok");
        }

        // The body walk builds the native element headlessly (iced widgets
        // construct without a renderer; only DRAWING needs one). The state
        // facts stay the assertion surface:
        assert_eq!(handle.state.borrow().explicit_expansion(sid(1)), Some(true));
        let _body = handle.body();
    }

    // -- 12. the SCN-014 registration lifecycle (headless seam) ---------------

    /// A built handle routed + registered through the LIFECYCLE seam, moved
    /// through the machines, then re-registered with an IDENTICAL surface
    /// (equal revision + shape): the running state SURVIVES (the
    /// PRESERVE-ON-IDENTICAL rule — REG-1 spirit) and the counters say
    /// "preserved", never "replaced".
    #[test]
    fn register_preserves_state_on_identical_resurface() {
        let fixture = || {
            surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "row"), item(8, "other")],
            ))])
        };
        let _guard = register_test_surface("scn014_preserve", fixture());
        let handle = MountRegistry::handle_for_surface(sid(200)).expect("registered");

        // Move the interaction state (apply through the machines).
        {
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            let mut sink = Vec::new();
            apply_mount_msg(
                &mut state,
                &mut dispatcher,
                &WidgetMountMsg::SelectionChanged {
                    surface_id: sid(200),
                    widget_id: sid(200),
                    selected: vec![sid(7)],
                },
                &mut sink,
            );
        }
        assert_eq!(handle.state.borrow().selection(), [sid(7)]);

        // The identical resurface: same revision, same shape, rebuilt.
        let before = MountRegistry::counters();
        let _fresh_guard = register_test_surface("scn014_preserve", fixture());
        let after = MountRegistry::counters();
        assert_eq!(after.registers - before.registers, 1);
        assert_eq!(
            after.registers_preserved - before.registers_preserved,
            1,
            "the PRESERVE-ON-IDENTICAL rule counted"
        );
        assert_eq!(
            after.registers_replaced - before.registers_replaced,
            0,
            "an identical resurface is not a replacement"
        );

        // The state survived: same machines (the SAME Rc pair), the
        // selection lived through the resurface.
        let still_registered = MountRegistry::handle_for_surface(sid(200))
            .expect("the panel stays registered");
        assert!(
            Rc::ptr_eq(&still_registered.state, &handle.state),
            "the ORIGINAL machines stayed registered"
        );
        assert_eq!(
            still_registered.state.borrow().selection(),
            [sid(7)],
            "the interaction state survived the identical resurface"
        );
        assert_eq!(
            still_registered.state.borrow().mutations().whole_state_resets,
            0,
            "no reset ran"
        );
    }

    /// A re-register whose surface MOVED (different revision) REPLACES: the
    /// outgoing machines release through the one dispose path (pending
    /// children, armed marks), the counter says "replaced" + one
    /// torn_down, and the slot's machines are fresh (no state carried
    /// over).
    #[test]
    fn register_replaces_and_resets_on_changed_surface() {
        let _guard = register_test_surface(
            "scn014_replace",
            surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "row"), item(8, "other")],
            ))]),
        );
        let old_handle = MountRegistry::handle_for_surface(sid(200)).expect("registered");
        {
            let mut state = old_handle.state.borrow_mut();
            let mut dispatcher = old_handle.dispatcher.borrow_mut();
            let mut sink = Vec::new();
            apply_mount_msg(
                &mut state,
                &mut dispatcher,
                &WidgetMountMsg::SelectionChanged {
                    surface_id: sid(200),
                    widget_id: sid(200),
                    selected: vec![sid(7)],
                },
                &mut sink,
            );
        }
        assert_eq!(old_handle.state.borrow().selection(), [sid(7)]);

        // The CHANGED surface: a raised revision (+ a grown list to make
        // the shape move too).
        let changed = WidgetSurface {
            revision: SceneRevision(2),
            ..surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "row"), item(8, "other"), item(9, "new")],
            ))])
        };
        let before = MountRegistry::counters();
        let _replaced = register_test_surface("scn014_replace", changed);
        let after = MountRegistry::counters();
        assert_eq!(
            after.registers_replaced - before.registers_replaced,
            1,
            "a moved surface replaces"
        );
        assert_eq!(
            after.registers_preserved - before.registers_preserved,
            0,
            "a moved surface never preserves"
        );
        assert_eq!(
            after.torn_down - before.torn_down,
            1,
            "the outgoing machine release flows through the ONE dispose path"
        );

        // The slot's machines are FRESH: a new Rc pair, no carried state.
        let new_handle = MountRegistry::handle_for_surface(sid(200)).expect("still registered");
        assert!(
            !Rc::ptr_eq(&new_handle.state, &old_handle.state),
            "a new machine pair took the slot"
        );
        assert!(new_handle.state.borrow().selection().is_empty());
        assert_eq!(
            new_handle.state.borrow().mutations().whole_state_resets,
            0,
            "the new state is fresh, not a reset"
        );
        // And the OUTGOING machines released: the interaction state is
        // gone (the §16 contract ran on the old pair).
        assert!(
            old_handle.state.borrow().selection().is_empty(),
            "the outgoing machines released"
        );
        assert_eq!(
            old_handle.state.borrow().mutations().whole_state_resets,
            1,
            "the outgoing release reset them exactly once"
        );
    }

    /// §16 "closing a tab cancels pending requests and releases state": a
    /// lazy expand toggled ON parks a pending child-page request; the
    /// teardown drops it (the state's `reset_all`) and releases the
    /// registration; a second teardown — or the guard's drop after a
    /// manual one — is a quiet idempotent no-op. (The WIRE era note: the
    /// genuinely in-flight IPC fetch cancellation needs the request-id
    /// vocabulary of the ADR-004 crossing — RECORDED, see the module docs.)
    #[test]
    fn teardown_cancels_pending_children() {
        let lazy_row = TreeRowDef {
            lazy_children: true,
            ..expandable_row(1, None, "Root A")
        };
        let _guard = register_test_surface(
            "scn014_teardown",
            surface(vec![tree(
                100,
                vec![lazy_row, tree_row(11, Some(1), "A.1")],
                false,
            )]),
        );
        let handle = MountRegistry::handle_for_surface(sid(100)).expect("registered");
        let state_rc = handle.state.clone();

        // Open the lazy branch: the toggle emits the expanded notification
        // AND the child-page request; the request parks in pending.
        let (routing, _) = {
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            let mut sink = Vec::new();
            let routing = apply_mount_msg(
                &mut state,
                &mut dispatcher,
                &WidgetMountMsg::RowExpandedToggled {
                    surface_id: sid(100),
                    widget_id: sid(100),
                    row_id: sid(1),
                    expanded: true,
                },
                &mut sink,
            );
            (routing, sink)
        };
        assert_eq!(routing, MountRouting::Applied { forwarded: 2 });
        assert_eq!(
            state_rc.borrow().pending_children().len(),
            1,
            "the lazy page request parked"
        );

        // The teardown: pending dropped, registry released, counted.
        let before = MountRegistry::counters();
        MountRegistry::teardown_surface("scn014_teardown");
        assert!(
            !MountRegistry::mount_registered("scn014_teardown"),
            "the registry released"
        );
        assert!(MountRegistry::handle_for_surface(sid(100)).is_none());
        assert_eq!(
            state_rc.borrow().pending_children().len(),
            0,
            "the pending requests DROPPED (state reset_all)"
        );
        assert_eq!(
            state_rc.borrow().mutations().whole_state_resets,
            1,
            "exactly one explicit reset"
        );
        assert_eq!(MountRegistry::counters().torn_down - before.torn_down, 1);

        // Idempotent: a second teardown — and the guard's drop after the
        // manual one — change nothing and panic nowhere.
        MountRegistry::teardown_surface("scn014_teardown");
        drop(_guard);
        assert_eq!(
            MountRegistry::counters().torn_down - before.torn_down,
            1,
            "the idempotent no-ops moved no counters"
        );
    }

    /// The guard owns the teardown when it drops; a MANUAL teardown during
    /// the guard's life makes the drop a no-op (idempotent), and every
    /// path accounts exactly.
    #[test]
    fn raii_guard_tears_down_on_drop() {
        let before = MountRegistry::counters();
        // (a) the guard owns the only teardown.
        {
            let _guard = register_test_surface(
                "scn014_raii_a",
                surface(vec![WidgetNode::List(item_list(
                    201,
                    vec![item(7, "row")],
                ))]),
            );
            assert!(MountRegistry::mount_registered("scn014_raii_a"));
        }
        assert!(
            !MountRegistry::mount_registered("scn014_raii_a"),
            "the drop tore the surface down"
        );
        assert_eq!(MountRegistry::counters().torn_down - before.torn_down, 1);

        // (b) a manual teardown during the guard's life: the guard's drop
        // is the idempotent no-op (no double count, no panic).
        {
            let _guard = register_test_surface(
                "scn014_raii_b",
                surface(vec![WidgetNode::List(item_list(
                    202,
                    vec![item(7, "row")],
                ))]),
            );
            MountRegistry::teardown_surface("scn014_raii_b");
            assert!(!MountRegistry::mount_registered("scn014_raii_b"));
        }
        assert_eq!(
            MountRegistry::counters().torn_down - before.torn_down,
            2,
            "accounted exactly once per teardown"
        );
    }

    /// The bridge sink contract (headless): the COLLECTING sink receives
    /// EXACTLY the forwarded events — in emission order, drained once, and
    /// reusable across drains (`take` is the bridge's consumption point).
    #[test]
    fn collecting_sink_receives_exactly_the_forwarded_events() {
        let lazy_row = TreeRowDef {
            lazy_children: true,
            ..expandable_row(1, None, "Root A")
        };
        let _guard = register_test_surface(
            "scn014_sink",
            surface(vec![tree(
                100,
                vec![lazy_row, tree_row(11, Some(1), "A.1")],
                false,
            )]),
        );
        let handle = MountRegistry::handle_for_surface(sid(100)).expect("registered");

        let mut sink = CollectingSink::default();
        let routing = {
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            apply_mount_msg(
                &mut state,
                &mut dispatcher,
                &WidgetMountMsg::RowExpandedToggled {
                    surface_id: sid(100),
                    widget_id: sid(100),
                    row_id: sid(1),
                    expanded: true,
                },
                &mut sink,
            )
        };
        assert_eq!(routing, MountRouting::Applied { forwarded: 2 });
        assert_eq!(
            sink.take(),
            vec![
                WidgetEvent::TreeExpanded {
                    surface_id: sid(100),
                    node_id: sid(1),
                    expanded: true,
                },
                WidgetEvent::ChildrenRequested {
                    surface_id: sid(100),
                    parent_id: sid(1),
                    range: WidgetRange {
                        start: 0,
                        len: MAX_WIDGET_RANGE_LEN
                    },
                },
            ],
            "exactly the forwarded events, in emission order"
        );
        assert_eq!(sink.take(), Vec::new(), "drained exactly once");
        assert!(
            sink.collected().is_empty(),
            "the drained sink reports nothing collected"
        );
        assert_eq!(
            sink.take(),
            Vec::new(),
            "the sink stays usable across drains"
        );
    }

    /// Two panels registered side-by-side (the fixture surface ids pick
    /// apart cleanly): a message for panel A's surface routes ONLY through
    /// A's machines — B's state, B's machines and B's read model never see
    /// it — and the surface-keyed lookup never hands a panel the other
    /// panel's handle.
    #[test]
    fn mount_registry_isolation_between_panels() {
        let surface_a = || {
            surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "A row")],
            ))])
        };
        let surface_b = || {
            surface(vec![tree(
                300,
                vec![tree_row(1, None, "B row")],
                false,
            )])
        };
        let _guard_a = register_test_surface("scn014_isolation_a", surface_a());
        let _guard_b = register_test_surface("scn014_isolation_b", surface_b());

        // The lookups separate the panels (surface 200 = A's, 300 = B's).
        let handle_a = MountRegistry::handle_for_surface(sid(200)).expect("A registered");
        let handle_b = MountRegistry::handle_for_surface(sid(300)).expect("B registered");
        assert_eq!(handle_a.surface_id(), sid(200));
        assert_eq!(handle_b.surface_id(), sid(300));
        assert!(
            !Rc::ptr_eq(&handle_a.state, &handle_b.state),
            "the panels carry distinct machines"
        );
        assert!(
            !Rc::ptr_eq(&handle_a.dispatcher, &handle_b.dispatcher),
            "the panels carry distinct dispatchers"
        );

        // A's message routes through A's machines ONLY.
        {
            let mut state = handle_a.state.borrow_mut();
            let mut dispatcher = handle_a.dispatcher.borrow_mut();
            let mut sink = Vec::new();
            apply_mount_msg(
                &mut state,
                &mut dispatcher,
                &WidgetMountMsg::SelectionChanged {
                    surface_id: sid(200),
                    widget_id: sid(200),
                    selected: vec![sid(7)],
                },
                &mut sink,
            );
        }
        assert_eq!(
            handle_a.state.borrow().selection(),
            [sid(7)],
            "A took its own message"
        );
        // B's machines never moved (no selection, no writes, no pending).
        assert!(
            handle_b.state.borrow().selection().is_empty(),
            "B never saw A's message"
        );
        assert_eq!(handle_b.state.borrow().mutations().row_state_changes, 0);
        assert_eq!(handle_b.state.borrow().pending_children().len(), 0);

        // And B's disposition is gate-identical: unmounting A leaves B's
        // registration exactly in place (per-panel accounting only).
        MountRegistry::teardown_surface("scn014_isolation_a");
        assert!(MountRegistry::mount_registered("scn014_isolation_b"));
        assert!(MountRegistry::handle_for_surface(sid(300)).is_some());
        assert!(MountRegistry::handle_for_surface(sid(200)).is_none());
    }

    // -- 13. SCN-020 — the coalescing join: N moves, ≤1 mount call ------------

    /// §7/§14: a 100-move burst defers into ONE owed slot per surface (99
    /// counted superseded — the older positions are dropped, never queued),
    /// and the flush answers exactly one call with the LATEST position.
    /// No machine, event, or counter wobble beyond the probes: moves are
    /// of the registry-side HoverState only.
    #[test]
    fn n_moves_coalesce_to_one_mount_call() {
        let _probe = register_panel(
            "scn020_coalesce",
            900,
            surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]),
        );
        let before = MountRegistry::counters();
        for i in 0..100u64 {
            let outcome = MountRegistry::mount_move_defer(sid(900), Point::new(i as f32, 0.0));
            assert!(outcome.is_some(), "a registered surface defers its moves");
        }
        let mid = MountRegistry::counters();
        assert_eq!(
            mid.coalesced_moves_superseded - before.coalesced_moves_superseded,
            99,
            "every move but the last overwrote the owed slot"
        );
        assert_eq!(
            mid.mount_move_calls - before.mount_move_calls,
            0,
            "nothing processed per event — the defer is one slot write"
        );

        // The flush (the tick's answer): exactly ONE mount call, the LAST
        // position, delivered into the host-local HoverState ONLY.
        assert_eq!(MountRegistry::mount_flush_moves(), 1);
        let after = MountRegistry::counters();
        assert_eq!(after.mount_move_calls - before.mount_move_calls, 1);
        assert_eq!(
            MountRegistry::mount_hover_state(sid(900)),
            Some(HoverState {
                hover: None,
                last_move: Some(Point::new(99.0, 0.0)),
                captured: false,
                capture_origin: None,
                move_owed: None,
            }),
            "the tick answered ONLY the latest position; the slot is empty again"
        );
    }

    /// The newest move supersedes the owed one BEFORE the tick: the
    /// overwritten slot keeps only the latest, the older position never
    /// runs, and the superseded count names the discard.
    #[test]
    fn superseded_move_overwrites_owed_slot() {
        let _probe = register_panel(
            "scn020_supersede",
            900,
            surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]),
        );
        let before = MountRegistry::counters();
        assert_eq!(
            MountRegistry::mount_move_defer(sid(900), Point::new(1.0, 2.0)),
            Some(false),
            "the first arrival finds no owed move"
        );
        // "Mid-processing": the newer move arrives while one is still owed.
        assert_eq!(
            MountRegistry::mount_move_defer(sid(900), Point::new(3.0, 4.0)),
            Some(true),
            "arriving over an in-flight owed move supersedes it"
        );
        assert_eq!(
            MountRegistry::counters().coalesced_moves_superseded - before.coalesced_moves_superseded,
            1
        );

        // The tick answers ONLY the newest (the older one never ran):
        assert_eq!(MountRegistry::mount_flush_moves(), 1);
        let after = MountRegistry::counters();
        assert_eq!(after.mount_move_calls - before.mount_move_calls, 1);
        assert_eq!(
            MountRegistry::mount_hover_state(sid(900)).and_then(|entry| entry.last_move),
            Some(Point::new(3.0, 4.0)),
            "the delivered position is the newest"
        );
        // An emptier flush delivers nothing (the slot is the ONLY thing).
        assert_eq!(MountRegistry::mount_flush_moves(), 0);
        assert_eq!(
            MountRegistry::counters().mount_move_calls,
            after.mount_move_calls
        );
    }

    // -- 14. SCN-020 — the keyboard ring: Tab / Shift+Tab skip disabled -------

    /// Tab×k walks the mounted surface's ring in PAINT order, skips the
    /// disabled row (the machines' own roving decided — the keyboard layer
    /// only translated), wraps at the edge, and Shift+Tab walks back.
    #[test]
    fn keyboard_ring_walks_and_skips_disabled() {
        let dead = ItemDef {
            enabled: false,
            ..item(8, "dead row")
        };
        let _probe = register_panel(
            "scn020_ring",
            900,
            surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "one"), dead, item(9, "three")],
            ))]),
        );
        let handle = MountRegistry::handle_for_surface(sid(900)).expect("registered");
        let surface = handle.surface.clone();
        let bare = Modifiers::NONE;
        let tab = || Key::Named(keyboard_key::Named::Tab);

        // Tab: the FIRST ENABLED row (7).
        let msgs = {
            let mut state = handle.state.borrow_mut();
            key_route(sid(900), &surface, &mut state, tab(), bare)
        };
        assert_eq!(
            msgs,
            vec![WidgetMountMsg::FocusMoved {
                surface_id: sid(900),
                widget_id: sid(200),
                to: Some(sid(7)),
            }],
            "Tab moves focus ONLY (no selection write)"
        );
        assert_eq!(handle.state.borrow().focus(), Some(sid(7)));

        // Tab again: the disabled row 8 is SKIPPED (never focused, never
        // named).
        let msgs = {
            let mut state = handle.state.borrow_mut();
            key_route(sid(900), &surface, &mut state, tab(), bare)
        };
        assert_eq!(
            msgs,
            vec![WidgetMountMsg::FocusMoved {
                surface_id: sid(900),
                widget_id: sid(200),
                to: Some(sid(9)),
            }],
            "the ring walk skipped the disabled row"
        );
        assert_eq!(handle.state.borrow().focus(), Some(sid(9)));

        // Tab at the END: the ring WRAPS to the first enabled row.
        let msgs = {
            let mut state = handle.state.borrow_mut();
            key_route(sid(900), &surface, &mut state, tab(), bare)
        };
        assert_eq!(msgs[0], WidgetMountMsg::FocusMoved {
            surface_id: sid(900),
            widget_id: sid(200),
            to: Some(sid(7)),
        });
        assert_eq!(handle.state.borrow().focus(), Some(sid(7)));

        // Shift+Tab from the top: wraps BACKWARD to the last enabled row.
        let msgs = {
            let mut state = handle.state.borrow_mut();
            key_route(
                sid(900),
                &surface,
                &mut state,
                tab(),
                Modifiers::SHIFT,
            )
        };
        assert_eq!(msgs[0], WidgetMountMsg::FocusMoved {
            surface_id: sid(900),
            widget_id: sid(200),
            to: Some(sid(9)),
        });
        assert_eq!(handle.state.borrow().focus(), Some(sid(9)));
    }

    /// §16 keyboard a11y WITHOUT pointer: arrows drive focus AND selection
    /// end-to-end over the mounted machines with ZERO pointer events (no
    /// press, no capture — the sole registered surface IS the keyboard
    /// target), and every landed step counts a nav step.
    #[test]
    fn arrow_keys_drive_focus_and_selection_no_pointer() {
        MountRegistry::set_mount_key_target(None);
        let _probe = register_panel(
            "scn020_arrows",
            900,
            surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "one"), item(8, "two"), item(9, "three")],
            ))]),
        );
        // The sole registered surface is the keyboard target (no click).
        assert_eq!(MountRegistry::mount_key_surface(), Some(sid(900)));
        let handle = MountRegistry::handle_for_surface(sid(900)).expect("registered");
        let surface = handle.surface.clone();
        let before = MountRegistry::counters();
        let down = || Key::Named(keyboard_key::Named::ArrowDown);
        let up = || Key::Named(keyboard_key::Named::ArrowUp);
        let apply = |msgs: &[WidgetMountMsg]| {
            // The bridge's application (what `route_mount_msg` runs).
            let mut events = Vec::new();
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            for msg in msgs {
                let _ = apply_mount_msg(&mut state, &mut dispatcher, msg, &mut events);
            }
            events
        };

        // ArrowDown: focus + selection land on row 7 — NO pointer call
        // happened (assert: no capture arm exists on the surface).
        let msgs = {
            let mut state = handle.state.borrow_mut();
            key_route(sid(900), &surface, &mut state, down(), Modifiers::NONE)
        };
        assert_eq!(
            msgs,
            vec![
                WidgetMountMsg::FocusMoved {
                    surface_id: sid(900),
                    widget_id: sid(200),
                    to: Some(sid(7)),
                },
                WidgetMountMsg::SelectionChanged {
                    surface_id: sid(900),
                    widget_id: sid(200),
                    selected: vec![sid(7)],
                },
            ]
        );
        assert_eq!(
            apply(&msgs),
            vec![WidgetEvent::SelectionChanged {
                surface_id: sid(900),
                selected_ids: vec![sid(7)],
            }],
            "the arrows' selection write forwards the selection event"
        );
        {
            let state = handle.state.borrow();
            assert_eq!(state.focus(), Some(sid(7)));
            assert_eq!(state.selection(), [sid(7)]);
        }
        assert!(!MountRegistry::mount_captured(sid(900)));
        let stepper = MountRegistry::counters();
        assert_eq!(
            stepper.keyboard_nav_steps - before.keyboard_nav_steps,
            1,
            "one resolved key = one nav step"
        );

        // ArrowDown twice more: the cursor walks the ring.
        for expected in [sid(8), sid(9)] {
            let msgs = {
                let mut state = handle.state.borrow_mut();
                key_route(sid(900), &surface, &mut state, down(), Modifiers::NONE)
            };
            assert_eq!(msgs.len(), 2);
            let _ = apply(&msgs);
            assert_eq!(handle.state.borrow().focus(), Some(expected));
            assert_eq!(handle.state.borrow().selection(), [expected]);
        }

        // ArrowUp steps BACK (focus + selection together).
        let msgs = {
            let mut state = handle.state.borrow_mut();
            key_route(sid(900), &surface, &mut state, up(), Modifiers::NONE)
        };
        assert_eq!(msgs.len(), 2);
        let _ = apply(&msgs);
        {
            let state = handle.state.borrow();
            assert_eq!(state.focus(), Some(sid(8)));
            assert_eq!(state.selection(), [sid(8)]);
        }
        assert_eq!(
            MountRegistry::counters().keyboard_nav_steps - before.keyboard_nav_steps,
            4
        );
    }

    // -- 15. SCN-020 — pointer capture across the coalescer -------------------

    /// Press inside → capture ON; the captured moves flow the coalescer
    /// (deferred); a release ANYWHERE completes: capture OFF and the
    /// gesture's last move processed (exactly one).
    #[test]
    fn capture_completes_on_release_outside() {
        let _probe = register_panel(
            "scn020_capture",
            900,
            surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]),
        );
        let before = MountRegistry::counters();

        assert!(MountRegistry::mount_press(sid(900)), "press inside arms capture");
        assert!(MountRegistry::mount_captured(sid(900)));
        assert_eq!(MountRegistry::mount_key_surface(), Some(sid(900)), "the pressed surface owns the keys");

        // The captured move: coalesced (deferred per event).
        assert_eq!(
            MountRegistry::mount_move_defer(sid(900), Point::new(8_000.0, 9_000.0)),
            Some(false)
        );
        assert_eq!(
            MountRegistry::counters().mount_move_calls - before.mount_move_calls,
            0,
            "moves during capture stay deferred"
        );

        // Release OUTSIDE the area: the pure completion contract — capture
        // OFF, the owed move processed (exactly one).
        assert_eq!(MountRegistry::mount_pointer_released(sid(900)), 1);
        assert!(!MountRegistry::mount_captured(sid(900)));
        let after = MountRegistry::counters();
        assert_eq!(after.mount_move_calls - before.mount_move_calls, 1);
        assert_eq!(
            MountRegistry::mount_hover_state(sid(900)).and_then(|entry| entry.last_move),
            Some(Point::new(8_000.0, 9_000.0))
        );

        // A release with nothing held delivers nothing (idempotent shape).
        assert_eq!(MountRegistry::mount_pointer_released(sid(900)), 0);
        assert_eq!(
            MountRegistry::counters().mount_move_calls,
            after.mount_move_calls
        );
    }

    /// §14 hover is host-local: setting hover / delivering coalesced moves
    /// moves NOTHING on the machines (mutation counters, dispatch
    /// counters, focus, selection all frozen) — the registry-side
    /// HoverState is the only record a move ever leaves.
    #[test]
    fn hover_is_host_local_and_never_reaches_machines() {
        let _probe = register_panel(
            "scn020_hover",
            900,
            surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]),
        );
        let handle = MountRegistry::handle_for_surface(sid(900)).expect("registered");
        let state_before = *handle.state.borrow().mutations();
        let dispatcher_before = *handle.dispatcher.borrow().counters();
        let focus_before = handle.state.borrow().focus();
        let selection_before = handle.state.borrow().selection().to_vec();

        assert!(MountRegistry::mount_set_hover(sid(900), sid(7)));
        assert!(MountRegistry::mount_move_defer(sid(900), Point::new(5.0, 5.0)).is_some());
        assert_eq!(MountRegistry::mount_flush_moves(), 1);
        assert!(!MountRegistry::mount_set_hover(sid(424_242), sid(7)), "unknown refusals quietly");

        assert_eq!(MountRegistry::mount_hover(sid(900)), Some(sid(7)));
        assert_eq!(
            MountRegistry::mount_hover_state(sid(900)).and_then(|entry| entry.last_move),
            Some(Point::new(5.0, 5.0))
        );
        // The machines see NOTHING:
        assert_eq!(*handle.state.borrow().mutations(), state_before);
        assert_eq!(*handle.dispatcher.borrow().counters(), dispatcher_before);
        assert_eq!(handle.state.borrow().focus(), focus_before);
        assert_eq!(handle.state.borrow().selection(), selection_before);
    }

    /// Unregistered surfaces are graceful everywhere: moves/presses/
    /// releases/hovers are quiet no-ops (nothing stored, nothing counted,
    /// nothing delivered) and the registry stays clean.
    #[test]
    fn unknown_surface_moves_are_graceful() {
        let before = MountRegistry::counters();
        assert_eq!(
            MountRegistry::mount_move_defer(sid(424_242), Point::new(1.0, 1.0)),
            None
        );
        assert!(!MountRegistry::mount_press(sid(424_242)));
        assert_eq!(MountRegistry::mount_pointer_released(sid(424_242)), 0);
        assert!(!MountRegistry::mount_set_hover(sid(424_242), sid(1)));
        assert_eq!(MountRegistry::mount_hover(sid(424_242)), None);
        assert!(!MountRegistry::mount_captured(sid(424_242)));
        // No keyboard target exists with nothing registered at all.
        MountRegistry::set_mount_key_target(None);
        assert_eq!(MountRegistry::mount_key_surface(), None);
        // And nothing processed: the flush finds no owed moves atomically.
        assert_eq!(MountRegistry::mount_flush_moves(), 0);
        assert_eq!(MountRegistry::counters(), before, "no probe moved");
    }

    /// The ONE-call-per-tick clamp across many bursts: after every flush
    /// the delivered count stays exactly the burst's surfaces-worth (1 per
    /// surface; one registered surface here → exactly 1 per tick, never
    /// more).
    #[test]
    fn mount_move_calls_never_exceed_one_per_tick() {
        let _probe = register_panel(
            "scn020_clamp",
            900,
            surface(vec![WidgetNode::List(item_list(200, vec![item(7, "row")]))]),
        );
        let before = MountRegistry::counters();
        for round in 0..5u64 {
            for i in 0..37u64 {
                let _ = MountRegistry::mount_move_defer(sid(900), Point::new(i as f32, round as f32));
            }
            assert_eq!(
                MountRegistry::mount_flush_moves(),
                1,
                "tick {round}: exactly one call per surface"
            );
            assert_eq!(
                MountRegistry::counters().mount_move_calls - before.mount_move_calls,
                round + 1,
                "the counter never steps past one per tick"
            );
        }
    }

    /// Disabled rows are un-navigable and un-selectable BY KEYBOARD: the
    /// ring walk skips them (no message ever names the disabled id) and a
    /// stale activation route aimed at one refuses at the machine with a
    /// counterless, eventless, stateless surface.
    #[test]
    fn disabled_keyboard_selection_refuses_cleanly() {
        let dead = ItemDef {
            enabled: false,
            ..item(8, "dead row")
        };
        let _probe = register_panel(
            "scn020_refuse",
            900,
            surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "one"), dead, item(9, "three")],
            ))]),
        );
        let handle = MountRegistry::handle_for_surface(sid(900)).expect("registered");
        let ring_surface = handle.surface.clone();
        let state_before = *handle.state.borrow().mutations();
        let dispatcher_before = *handle.dispatcher.borrow().counters();

        // The whole ring walk: Tab from nothing to the end — the disabled
        // row NEVER appears in any message (focused or selected).
        let mut named: Vec<StableId> = Vec::new();
        for _ in 0..4 {
            let msgs = {
                let mut state = handle.state.borrow_mut();
                key_route(
                    sid(900),
                    &ring_surface,
                    &mut state,
                    Key::Named(keyboard_key::Named::Tab),
                    Modifiers::NONE,
                )
            };
            for msg in &msgs {
                match msg {
                    WidgetMountMsg::FocusMoved { to: Some(row), .. } => named.push(*row),
                    WidgetMountMsg::SelectionChanged { selected, .. } => {
                        named.extend(selected.iter().copied());
                    }
                    _ => {}
                }
            }
        }
        assert!(!named.contains(&sid(8)), "the disabled row is never named: {named:?}");
        assert_eq!(*handle.state.borrow().mutations(), state_before);
        assert_eq!(*handle.dispatcher.borrow().counters(), dispatcher_before);

        // The stale-route guard (a message aimed at the disabled row): the
        // machine refuses — no event, no counter, nothing half-applied.
        let fixture = surface(vec![WidgetNode::List(item_list(
            200,
            vec![item(7, "one"), ItemDef { enabled: false, ..item(8, "dead row") }, item(9, "three")],
        ))]);
        let (mut state, mut dispatcher) = mounted_machines(&fixture);
        let (routing, events) = route(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::RowActivated {
                surface_id: sid(900),
                widget_id: sid(200),
                row_id: sid(8),
            },
        );
        assert_eq!(
            routing,
            MountRouting::Refused("row/widget 8 is disabled and cannot be invoked".to_string())
        );
        assert_eq!(events, Vec::new());
        assert_eq!(state.mutations().row_state_changes, 0, "nothing half-applied");
        assert_eq!(*dispatcher.counters(), dispatcher_before);
    }

    // -- 16. SCN-020 — the typed-table keyboard layer (pure consumption) ------

    /// Up/Down is the materialized-window ROW roving, Left/Right the cell
    /// cursor (caller-held), and Escape the FREE draft cancel: the
    /// table machine's own counters are the only traces.
    #[test]
    fn table_keys_drive_row_and_cell_roving_and_escape_draft() {
        let before = MountRegistry::counters();
        let def = table_three_column(
            700,
            3,
        );
        let mut table = table_state_of(&def);
        let mut cursor: Option<CellFocus> = None;
        let spec = window_spec(20.0, 4);
        let columns = [sid(2), sid(3), sid(4)];
        let bare = Modifiers::NONE;
        let down = || Key::Named(keyboard_key::Named::ArrowDown);
        let right = || Key::Named(keyboard_key::Named::ArrowRight);
        let left = || Key::Named(keyboard_key::Named::ArrowLeft);

        // Down: row roving enters the window at its top.
        let step =
            mount_table_key_route(&mut table, &mut cursor, down(), bare, &spec, &columns);
        assert_eq!(step, Some(MountTableKeyStep::RowFocus(sid(1))));
        assert_eq!(table.focus(), Some(sid(1)));

        // Right: the cell cursor lands on the FIRST column.
        let step =
            mount_table_key_route(&mut table, &mut cursor, right(), bare, &spec, &columns);
        assert_eq!(step, Some(MountTableKeyStep::CellFocus(CellFocus { key: sid(1), column: sid(2) })));
        assert_eq!(cursor, Some(CellFocus { key: sid(1), column: sid(2) }));

        // Right, Right: the cursor walks the columns; Left walks back.
        let step =
            mount_table_key_route(&mut table, &mut cursor, right(), bare, &spec, &columns);
        assert_eq!(step, Some(MountTableKeyStep::CellFocus(CellFocus { key: sid(1), column: sid(3) })));
        let step =
            mount_table_key_route(&mut table, &mut cursor, right(), bare, &spec, &columns);
        assert_eq!(step, Some(MountTableKeyStep::CellFocus(CellFocus { key: sid(1), column: sid(4) })));
        let step =
            mount_table_key_route(&mut table, &mut cursor, left(), bare, &spec, &columns);
        assert_eq!(step, Some(MountTableKeyStep::CellFocus(CellFocus { key: sid(1), column: sid(3) })));

        // Down again: the row moves ON (the cell cursor stays caller-held;
        // the column pair re-anchors to the row on the next roving).
        let step =
            mount_table_key_route(&mut table, &mut cursor, down(), bare, &spec, &columns);
        assert_eq!(step, Some(MountTableKeyStep::RowFocus(sid(2))));

        // The draft: armed, drafted — Escape cancels FREE (no counter).
        table.begin_cell_edit(sid(2), sid(3)).expect("the Number column");
        table.draft_cell_value(WidgetCellValue::Number(7.0)).expect("drafted");
        let mutations_before = *table.mutations();
        let step = mount_table_key_route(
            &mut table,
            &mut cursor,
            Key::Named(keyboard_key::Named::Escape),
            bare,
            &spec,
            &columns,
        );
        assert_eq!(step, Some(MountTableKeyStep::DraftCancelled));
        assert!(table.draft().is_none(), "the draft is gone");
        assert_eq!(*table.mutations(), mutations_before, "a cancelled draft touched nothing");

        // Escape with no draft: no step, no effect.
        let step = mount_table_key_route(
            &mut table,
            &mut cursor,
            Key::Named(keyboard_key::Named::Escape),
            bare,
            &spec,
            &columns,
        );
        assert_eq!(step, None);

        // Nav steps counted: 2 rows + 4 cells + 1 escape = 7.
        assert_eq!(
            MountRegistry::counters().keyboard_nav_steps - before.keyboard_nav_steps,
            7
        );
    }

    // -- SCN-020 table fixtures ------------------------------------------------

    fn table_column(id: u64, kind: CellKind, label: &str) -> TableColumnDef {
        TableColumnDef {
            id: sid(id),
            label: label.to_string(),
            kind,
            width_hint: None,
            stretch: false,
        }
    }

    /// The three-column table fixture (text 2, number 3, bool 4), `rows`
    /// rows keyed 1..=rows with typed cells matching the columns.
    fn table_three_column(id: u64, rows: u64) -> PanelTableDef {
        PanelTableDef {
            id_table: sid(id),
            columns: vec![
                table_column(2, CellKind::Text, "note"),
                table_column(3, CellKind::Number, "length"),
                table_column(4, CellKind::Bool, "locked"),
            ],
            total_rows: None,
            rows: (1..=rows)
                .map(|i| TableRowDef {
                    key: sid(i),
                    cells: vec![
                        TableCell::Text(format!("note {i}")),
                        TableCell::Number(i as f64),
                        TableCell::Bool(i % 2 == 0),
                    ],
                    domain_ref: None,
                })
                .collect(),
        }
    }

    /// A table state over the fixture def (surface id 900), synced against
    /// a tables-only surface (the typed-table state layer's own contract).
    fn table_state_of(def: &PanelTableDef) -> TableUiState {
        let mut state = TableUiState::new(sid(900), def.id_table);
        let surface = WidgetSurface {
            revision: SceneRevision(1),
            roots: Vec::new(),
            tables: std::collections::BTreeMap::from([(def.id_table, def.clone())]),
        };
        state.sync(&surface);
        state
    }



    // -- SCN-025: the app-owned PANEL SURFACE STORE (the wire attach) --------

    use ocs_plugin_api::ipc::protocol::{
        PatchRejectCode, PatchRejectReport, PanelStoreKind, SceneOrWidgetPatch,
        SceneOrWidgetSurface, StoreRow,
    };
    use ocs_plugin_api::scene::{Rect2D, Vec2};
    use ocs_plugin_api::widgets::{WidgetSurfacePatch, WidgetTreeDef};

    /// The attach's two steps, exactly as `app::plugin_host` runs them:
    /// store through [`MountRegistry::panel_surface_store_mut`], then the
    /// widget leg's registry registration. Returns the store's outcome.
    /// (The WIRE mapping is pinned in the plugin_host tests; these tests
    /// pin the store + mount machinery directly.)
    fn attach_publish(
        registry: &mut MountRegistry,
        owner: &str,
        panel_id: &str,
        surface: SceneOrWidgetSurface,
    ) -> Result<PanelSurfaceOutcome, StoreReject> {
        let outcome = registry
            .panel_surface_store_mut()
            .publish(owner, panel_id, surface)?;
        if let Some((surface_id, attach_surface)) = outcome.widget_mount.as_ref() {
            let surface_id = *surface_id;
            let attach_surface = attach_surface.clone();
            registry.register_surface_owned(
                panel_id,
                SurfaceMountHandle::from_surface(surface_id, attach_surface),
            );
        }
        Ok(outcome)
    }

    fn publish_of(revision: SceneRevision, roots: Vec<WidgetNode>) -> SceneOrWidgetSurface {
        SceneOrWidgetSurface::Widgets(WidgetSurface {
            revision,
            roots,
            tables: std::collections::BTreeMap::new(),
        })
    }

    /// One list-root surface (the minimal valid widget shape) — the list
    /// widget ids at `sid(200)`.
    fn simple_list_surface(revision: SceneRevision, items: Vec<ItemDef>) -> SceneOrWidgetSurface {
        publish_of(
            revision,
            vec![WidgetNode::List(ItemListDef {
                id: sid(200),
                name: None,
                enabled: true,
                visible: true,
                label: None,
                items,
            })],
        )
    }

    fn plain_table_row(key: u64) -> TableRowDef {
        TableRowDef {
            key: sid(key),
            cells: vec![TableCell::Text(format!("r{key}"))],
            domain_ref: None,
        }
    }

    /// THE LEGACY GUARANTEE pin: zero publishes ⇒ zero store entries ⇒ the
    /// mount stays unregistered — the BYTE-IDENTICAL legacy path holds BY
    /// CONSTRUCTION now that the store can create entries too.
    #[test]
    fn legacy_zero_publishes_leaves_the_store_and_mount_empty() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry(|r| {
            assert!(!r.panel_surface_store().attached("align_tools"));
            assert!(!r.mount_registered_owned("align_tools"));
            assert!(r.panel_surface_store().entry("align_tools").is_none());
            let counters = r.counters_owned();
            assert_eq!(counters.mount_event_forwards, 0);
            assert_eq!(counters.mount_demand_forwards, 0);
        });
        // And an unattached read answers the honest empty page (never a
        // demand — nothing attached names nothing).
        let page = with_test_registry(|r| {
            r.panel_surface_store().store_page(
                "align_tools",
                PanelStoreKind::ListItems,
                None,
                WidgetRange { start: 0, len: 8 },
            )
        });
        assert!(page.rows.is_empty());
        assert_eq!(page.total_hint, 0);
        assert!(page.demand.is_none());
        assert_eq!(page.surface_id, None);
        // The snapshot2 stub shape (the trait default's bytes).
        let (rows, total) =
            with_test_registry(|r| r.panel_surface_store().snapshot_page("align_tools"));
        assert!(matches!(
            rows.as_slice(),
            [StoreRow::State(digest)] if digest.scene.is_none() && digest.widgets.is_none()
        ));
        assert_eq!(total, 0);
    }

    /// Publish → store → MOUNT-LIVE: a widget publish stores (the owner
    /// recorded) AND the mount registers; the frozen surface id is the
    /// FIRST ROOT's id (the registration convention); the bridge's two
    /// panel reads (the gate + the identity) and the body build answer.
    #[test]
    fn widget_publish_stores_and_mounts_go_live() {
        let _app = OpenCADStudio::new_for_test();
        let publish = simple_list_surface(SceneRevision(3), vec![item(7, "first"), item(8, "second")]);
        with_test_registry_mut(|registry| {
            let outcome = attach_publish(registry, "PLUGIN", "align_tools", publish)
                .expect("the first publish attaches");
            assert!(outcome.widget_mount.is_some());
            assert_eq!(outcome.report.revision, SceneRevision(3));
            let entry = registry
                .panel_surface_store()
                .entry("align_tools")
                .expect("the first publish created the entry");
            assert_eq!(entry.owner_plugin_id, "PLUGIN");
            assert_eq!(
                entry.widgets.as_ref().expect("stored").revision,
                SceneRevision(3)
            );
            assert_eq!(entry.surface_id, Some(sid(200)), "the first root's id convention");
        });
        // THE MOUNT GATE GOES LIVE: the panel hooks' reads answer, the
        // body element builds (the first end-to-end registration pin).
        assert!(with_test_registry(|r| r.mount_registered_owned("align_tools")));
        assert_eq!(
            with_test_registry(|r| r.panel_surface_id_owned("align_tools")),
            Some(sid(200))
        );
        assert!(
            with_test_registry(|r| r.mounted_body_owned("align_tools")).is_some()
        );
    }

    /// The second-plugin capture refuses STRUCTURED
    /// (`PublisherOwnedByOther` — the appended code 5) with ZERO mutation:
    /// the first publisher's store and mount stay exactly as they were.
    #[test]
    fn second_plugin_publishing_same_panel_refuses_structured() {
        let _app = OpenCADStudio::new_for_test();
        let first = simple_list_surface(SceneRevision(2), vec![item(7, "a")]);
        let intruder = simple_list_surface(SceneRevision(9), vec![item(99, "b")]);
        with_test_registry_mut(|registry| {
            attach_publish(registry, "PLUGIN_A", "align_tools", first)
                .expect("the first publisher attaches");
            let before = registry.counters_owned();

            let (code, report) = attach_publish(registry, "PLUGIN_B", "align_tools", intruder)
                .expect_err("never a takeover");
            assert_eq!(code, PatchRejectCode::PublisherOwnedByOther);
            assert_eq!(
                report,
                PatchRejectReport {
                    current_revision: None,
                    failed_op: None,
                }
            );
            // The store: UNTOUCHED by the refusal, still PLUGIN_A's.
            let entry = registry.panel_surface_store().entry("align_tools").unwrap();
            assert_eq!(entry.owner_plugin_id, "PLUGIN_A");
            assert_eq!(entry.widgets.as_ref().unwrap().revision, SceneRevision(2));
            assert_eq!(entry.widgets.as_ref().unwrap().roots[0].widget_id(), sid(200));
            // The mount: the FIRST publisher's registration stands; the
            // refusal registered nothing.
            assert_eq!(registry.counters_owned(), before);
        });
    }

    /// The patch leg: a stale base refuses structured (the head revision
    /// reported — the recovery fact) with zero mutation; an applied patch
    /// advances the revision and RE-REGISTERS the mount (a revision move is
    /// never shape-identical: the REPLACE flow — machines released, the
    /// report's capped facts matching the ops), with the FROZEN surface
    /// id surviving the replacement.
    #[test]
    fn patch_leg_stale_refuses_applied_advances_and_reregisters() {
        let _app = OpenCADStudio::new_for_test();
        let publish = simple_list_surface(SceneRevision(5), vec![item(7, "a"), item(8, "b")]);
        let stale = SceneOrWidgetPatch::Widgets(WidgetSurfacePatch {
            base_revision: SceneRevision(3),
            revision: SceneRevision(6),
            ops: vec![],
        });
        let drift = SceneOrWidgetPatch::Widgets(WidgetSurfacePatch {
            base_revision: SceneRevision(5),
            revision: SceneRevision(6),
            ops: vec![ocs_plugin_api::widgets::WidgetOp::Add {
                parent: None,
                widget: WidgetNode::List(ItemListDef {
                    id: sid(300),
                    name: None,
                    enabled: true,
                    visible: true,
                    label: None,
                    items: vec![item(11, "c")],
                }),
            }],
        });
        with_test_registry_mut(|registry| {
            attach_publish(registry, "PLUGIN", "align_tools", publish).expect("attach");
            let before = registry.counters_owned();

            // (1) Stale: refuses with the head reported; NOTHING moves.
            let (code, report) = registry
                .panel_surface_store_mut()
                .patch("PLUGIN", "align_tools", stale)
                .expect_err("stale refuses");
            assert_eq!(code, PatchRejectCode::StaleBase);
            assert_eq!(report.current_revision, Some(SceneRevision(5)));
            let entry = registry.panel_surface_store().entry("align_tools").unwrap();
            assert_eq!(entry.widgets.as_ref().unwrap().revision, SceneRevision(5));
            assert_eq!(registry.counters_owned(), before, "no re-registration");

            // (2) Applied: the revision advances; the attach's registry
            // step (the R5 part of the store outcome) re-registers the
            // mount — REPLACE (a revision move is never shape-identical).
            let outcome = registry
                .panel_surface_store_mut()
                .patch("PLUGIN", "align_tools", drift)
                .expect("applies");
            assert_eq!(outcome.report.revision, SceneRevision(6));
            assert_eq!(outcome.report.touched_subtree_ids.as_slice(), &[sid(300)]);
            if let Some((surface_id, surface)) = outcome.widget_mount.as_ref() {
                let surface_id = *surface_id;
                let surface = surface.clone();
                registry.register_surface_owned(
                    "align_tools",
                    SurfaceMountHandle::from_surface(surface_id, surface),
                );
            }
            let entry = registry.panel_surface_store().entry("align_tools").unwrap();
            assert_eq!(entry.widgets.as_ref().unwrap().revision, SceneRevision(6));
            let after = registry.counters_owned();
            assert_eq!(after.registers_replaced - before.registers_replaced, 1);
            assert_eq!(after.torn_down - before.torn_down, 1);
            assert_eq!(
                registry.panel_surface_id_owned("align_tools"),
                Some(sid(200)),
                "the frozen surface id survives the replacement"
            );
        });
    }

    /// Re-publishes: a rewind refuses (`NonMonotonicRevision`, the head
    /// reported); an identical snapshot at the head is the refresh shape
    /// (PRESERVE — the machines stay); a greater revision re-stores and
    /// re-registers (replace).
    #[test]
    fn publish_leg_revision_law_monotone_with_identical_refresh() {
        let _app = OpenCADStudio::new_for_test();
        let attach = simple_list_surface(SceneRevision(5), vec![item(7, "a")]);
        let refresh = simple_list_surface(SceneRevision(5), vec![item(7, "a")]);
        let rewind = simple_list_surface(SceneRevision(4), vec![item(7, "a")]);
        let forward = simple_list_surface(SceneRevision(9), vec![item(7, "a"), item(8, "b")]);
        with_test_registry_mut(|registry| {
            attach_publish(registry, "PLUGIN", "align_tools", attach).expect("attach");
            let before = registry.counters_owned();

            // (1) Rewind: refused.
            let (code, report) = attach_publish(registry, "PLUGIN", "align_tools", rewind)
                .expect_err("rewind");
            assert_eq!(code, PatchRejectCode::NonMonotonicRevision);
            assert_eq!(report.current_revision, Some(SceneRevision(5)));

            // (2) Identical at the head: PRESERVE.
            attach_publish(registry, "PLUGIN", "align_tools", refresh)
                .expect("the refresh shape");
            assert_eq!(
                registry.counters_owned().registers_preserved - before.registers_preserved,
                1
            );

            // (3) A greater revision (a resync snapshot): re-store + replace.
            attach_publish(registry, "PLUGIN", "align_tools", forward)
                .expect("forward");
            assert_eq!(
                registry.counters_owned().registers_replaced - before.registers_replaced,
                1
            );
        });
    }

    /// The list pages: an in-range window serves (total = the held count);
    /// a past-end window answers EMPTY and ASKS (the exact holding gap);
    /// a usize::MAX window is checked arithmetic (never a panic); a
    /// width-clamped ask never answers a bigger page.
    #[test]
    fn list_pages_checked_arithmetic_past_end_and_cap() {
        let _app = OpenCADStudio::new_for_test();
        let publish = simple_list_surface(
            SceneRevision(1),
            (0..5)
                .map(|i| item(10 + i as u64, &format!("row {i}")))
                .collect(),
        );
        with_test_registry_mut(|registry| {
            attach_publish(registry, "PLUGIN", "list_panel", publish).expect("attach");

            // In-range: rows 1..=3.
            let page = registry.panel_surface_store().store_page(
                "list_panel",
                PanelStoreKind::ListItems,
                Some(sid(200)),
                WidgetRange { start: 1, len: 3 },
            );
            assert_eq!(page.rows.len(), 3);
            assert_eq!(page.total_hint, 5);
            assert!(page.demand.is_none(), "a fully-served window demands nothing");
            assert_eq!(page.surface_id, Some(sid(200)));
            match &page.rows[0] {
                StoreRow::Item(def) => assert_eq!(def.id, sid(11)),
                other => panic!("list pages carry items: {other:?}"),
            }

            // Past-end: empty + the exact-gap demand.
            let past = registry.panel_surface_store().store_page(
                "list_panel",
                PanelStoreKind::ListItems,
                Some(sid(200)),
                WidgetRange { start: 999, len: 4 },
            );
            assert!(past.rows.is_empty());
            assert_eq!(
                past.demand,
                Some((Some(sid(200)), WidgetRange { start: 5, len: 999 + 4 - 5 }))
            );
            // The checked extreme.
            let extreme = registry.panel_surface_store().store_page(
                "list_panel",
                PanelStoreKind::ListItems,
                Some(sid(200)),
                WidgetRange { start: usize::MAX, len: usize::MAX },
            );
            assert!(extreme.rows.is_empty(), "checked arithmetic, never a panic");
            assert!(extreme.demand.is_some());

            // The cap: a full-width ask clamps to the held rows (the
            // server never answers a bigger page than the wire budget).
            let clamped = registry.panel_surface_store().store_page(
                "list_panel",
                PanelStoreKind::ListItems,
                Some(sid(200)),
                WidgetRange { start: 0, len: ocs_plugin_api::widgets::MAX_WIDGET_RANGE_LEN },
            );
            assert_eq!(clamped.rows.len(), 5);
            assert!(
                clamped.rows.len() <= ocs_plugin_api::widgets::MAX_WIDGET_ROWS_INITIAL
            );

            // An UNKNOWN parent: empty page + the verbatim ask.
            let unknown = registry.panel_surface_store().store_page(
                "list_panel",
                PanelStoreKind::ListItems,
                Some(sid(777)),
                WidgetRange { start: 0, len: 2 },
            );
            assert!(unknown.rows.is_empty());
            assert_eq!(
                unknown.demand,
                Some((Some(sid(777)), WidgetRange { start: 0, len: 2 }))
            );
        });
    }

    /// TreeChildren pages resolve against the surface's trees: the parent
    /// ROW's declared children page in rows order; a LAZY branch (declared
    /// expandable+lazy with no held children) is the demand case (the
    /// ChildrenRequested analog); the root rows page under parent `None`.
    #[test]
    fn tree_children_pages_and_lazy_branch_demands() {
        let _app = OpenCADStudio::new_for_test();
        let publish = publish_of(
            SceneRevision(1),
            vec![WidgetNode::Tree(WidgetTreeDef {
                id: sid(1),
                name: None,
                enabled: true,
                visible: true,
                label: None,
                capabilities: ocs_plugin_api::widgets::TreeCapabilities {
                    multi_select: false,
                    filterable: false,
                    lazy_children: false,
                },
                rows: vec![
                    expandable_row(10, None, "root"),
                    tree_row(11, Some(10), "child a"),
                    tree_row(12, Some(10), "child b"),
                    expandable_row(20, Some(10), "lazy branch"),
                ],
            })],
        );
        with_test_registry_mut(|registry| {
            attach_publish(registry, "PLUGIN", "tree_panel", publish).expect("attach");

            // Root rows (parent None): only the declared root; served
            // fully (the window asks exactly the held count).
            let page = registry.panel_surface_store().store_page(
                "tree_panel",
                PanelStoreKind::TreeChildren,
                None,
                WidgetRange { start: 0, len: 1 },
            );
            assert_eq!(page.rows.len(), 1);
            assert!(page.demand.is_none(), "served fully — no page miss");
            // ...and a WIDER root-window is the page miss shape (rows may
            // sit beyond the published window plugin-side).
            let wider = registry.panel_surface_store().store_page(
                "tree_panel",
                PanelStoreKind::TreeChildren,
                None,
                WidgetRange { start: 0, len: 10 },
            );
            assert_eq!(wider.rows.len(), 1);
            assert!(wider.demand.is_some(), "a wider root window asks");

            // The children of a HELD row, in row order (the lazy branch's
            // row is 10's declared child too; its CHILDREN are the lazy
            // leg checked below) — served fully (window == held count).
            let page = registry.panel_surface_store().store_page(
                "tree_panel",
                PanelStoreKind::TreeChildren,
                Some(sid(10)),
                WidgetRange { start: 0, len: 3 },
            );
            let ids: Vec<_> = page
                .rows
                .iter()
                .map(|row| match row {
                    StoreRow::Tree(def) => def.id,
                    other => panic!("tree pages carry rows: {other:?}"),
                })
                .collect();
            assert_eq!(ids.as_slice(), &[sid(11), sid(12), sid(20)]);
            assert!(page.demand.is_none(), "served fully — no page miss");

            // A LAZY branch: nothing held → the demand (parent + window).
            let page = registry.panel_surface_store().store_page(
                "tree_panel",
                PanelStoreKind::TreeChildren,
                Some(sid(20)),
                WidgetRange { start: 0, len: 8 },
            );
            assert!(page.rows.is_empty());
            assert_eq!(
                page.demand,
                Some((Some(sid(20)), WidgetRange { start: 0, len: 8 }))
            );
        });
    }

    /// TableRows pages the typed-table registry: a keyed table pages its
    /// published rows with the def's OWN total as the hint; a WINDOWING
    /// table (declared total greater than the published window) demands
    /// past its rows; a fully-MATERIALIZED table never demands; an unknown
    /// table id asks verbatim.
    #[test]
    fn table_rows_pages_declared_totals_and_unpublished_tables() {
        let _app = OpenCADStudio::new_for_test();
        let column = table_column(501, CellKind::Text, "name");
        let windowed = PanelTableDef {
            id_table: sid(500),
            columns: vec![column.clone()],
            total_rows: Some(40),
            rows: (0..4).map(|i| plain_table_row(i as u64 + 1)).collect(),
        };
        let materialized = PanelTableDef {
            id_table: sid(600),
            columns: vec![table_column(601, CellKind::Text, "name")],
            total_rows: None,
            rows: (0..3).map(|i| plain_table_row(i as u64 + 100)).collect(),
        };
        let publish = publish_of(
            SceneRevision(1),
            vec![WidgetNode::Table(table_ref(2, 500))],
        );
        let publish = match publish {
            SceneOrWidgetSurface::Widgets(mut surface) => {
                surface.tables.insert(sid(500), windowed);
                surface.tables.insert(sid(600), materialized);
                SceneOrWidgetSurface::Widgets(surface)
            }
            _ => unreachable!("the fixture builds a widget publish"),
        };
        with_test_registry_mut(|registry| {
            attach_publish(registry, "PLUGIN", "table_panel", publish).expect("attach");

            // The held window pages; the hint is the def's declared total.
            let page = registry.panel_surface_store().store_page(
                "table_panel",
                PanelStoreKind::TableRows,
                Some(sid(500)),
                WidgetRange { start: 2, len: 2 },
            );
            assert_eq!(page.rows.len(), 2);
            assert_eq!(page.total_hint, 40);
            assert!(page.demand.is_none(), "fully in the held window");

            // Past the held window against the DECLARED total → demand the
            // gap (the typed-table page miss).
            let page = registry.panel_surface_store().store_page(
                "table_panel",
                PanelStoreKind::TableRows,
                Some(sid(500)),
                WidgetRange { start: 8, len: 4 },
            );
            assert!(page.rows.is_empty());
            assert_eq!(
                page.demand,
                Some((Some(sid(500)), WidgetRange { start: 4, len: 12 - 4 }))
            );

            // Fully materialized: past-end is past-the-end — empty, NO
            // demand (the def declared nothing beyond).
            let page = registry.panel_surface_store().store_page(
                "table_panel",
                PanelStoreKind::TableRows,
                Some(sid(600)),
                WidgetRange { start: 10, len: 4 },
            );
            assert!(page.rows.is_empty());
            assert!(page.demand.is_none());
            assert_eq!(page.total_hint, 3);

            // Unknown table: the ask out verbatim, hint 0.
            let page = registry.panel_surface_store().store_page(
                "table_panel",
                PanelStoreKind::TableRows,
                Some(sid(999)),
                WidgetRange { start: 0, len: 4 },
            );
            assert!(page.rows.is_empty());
            assert!(page.demand.is_some());
        });
    }

    /// Scene regions: a scene leg STORES (the scene MOUNT rendering seam is
    /// NOT this package — nothing registers into the mount) with no frozen
    /// surface id yet; region pages resolve the pre-order subtree; a scene
    /// read NEVER demands (the host store is the whole scene truth) and a
    /// past-end window is the empty page.
    #[test]
    fn scene_leg_stores_but_mounts_nothing_and_regions_never_demand() {
        let _app = OpenCADStudio::new_for_test();
        let scene = ocs_plugin_api::scene::PanelScene {
            revision: SceneRevision(4),
            elements: vec![ocs_plugin_api::scene::SceneElement::Group {
                id: sid(1),
                name: None,
                actions: vec![],
                drag: None,
                children: vec![
                    ocs_plugin_api::scene::SceneElement::Rect {
                        id: sid(2),
                        name: None,
                        actions: vec![],
                        drag: None,
                        pos: Vec2([0.0, 0.0]),
                        size: Vec2([10.0, 10.0]),
                        fill: None,
                        stroke: None,
                    },
                    ocs_plugin_api::scene::SceneElement::Text {
                        id: sid(3),
                        name: None,
                        actions: vec![],
                        drag: None,
                        content: "annotation".to_owned(),
                        pos: Vec2([1.0, 2.0]),
                        size: 10.0,
                        color: ocs_plugin_api::scene::SceneColor::default(),
                    },
                ],
                clip: None,
            }],
        };
        with_test_registry_mut(|registry| {
            let outcome = registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scene_panel", SceneOrWidgetSurface::Scene(scene))
                .expect("the scene leg stores");
            assert!(outcome.widget_mount.is_none(), "the recorded boundary");
            assert!(!registry.mount_registered_owned("scene_panel"));
            assert!(
                registry.panel_surface_store().entry("scene_panel").unwrap().surface_id.is_none(),
                "no widget leg — no frozen surface identity yet"
            );

            // The whole-scene read (parent None): group, rect, text.
            let page = registry.panel_surface_store().store_page(
                "scene_panel",
                PanelStoreKind::SceneRegion,
                None,
                WidgetRange { start: 0, len: 10 },
            );
            assert_eq!(page.rows.len(), 3);
            assert_eq!(page.total_hint, 3);
            assert!(page.demand.is_none());

            // A subtree read (the group's id): the same subtree.
            let page = registry.panel_surface_store().store_page(
                "scene_panel",
                PanelStoreKind::SceneRegion,
                Some(sid(1)),
                WidgetRange { start: 0, len: 10 },
            );
            assert_eq!(page.rows.len(), 3);

            // Continuation windows: the same flatten; past-end = empty.
            let tail = registry.panel_surface_store().store_page(
                "scene_panel",
                PanelStoreKind::SceneRegion,
                None,
                WidgetRange { start: 2, len: 10 },
            );
            assert_eq!(tail.rows.len(), 1);
            let past = registry.panel_surface_store().store_page(
                "scene_panel",
                PanelStoreKind::SceneRegion,
                None,
                WidgetRange { start: 8, len: 10 },
            );
            assert!(past.rows.is_empty());
            assert!(past.demand.is_none());
        });
    }

    /// snapshot2: the digest row (BOTH legs' bounded summaries) + the
    /// leading window — bounded regardless of scene size, the continuation
    /// floor at the page budget (the digest row shares it), never a full
    /// dump.
    #[test]
    fn snapshot2_leads_with_digest_and_bounded_window() {
        let _app = OpenCADStudio::new_for_test();
        // A scene whose pre-order flatten exceeds the page budget.
        let elements: Vec<ocs_plugin_api::scene::SceneElement> = (1..=5_000)
            .map(|i| ocs_plugin_api::scene::SceneElement::Rect {
                id: sid(i as u64),
                name: None,
                actions: vec![],
                drag: None,
                pos: Vec2([0.0, 0.0]),
                size: Vec2([4.0, 4.0]),
                fill: None,
                stroke: None,
            })
            .collect();
        let scene = ocs_plugin_api::scene::PanelScene {
            revision: SceneRevision(7),
            elements,
        };
        let widgets = simple_list_surface(SceneRevision(7), vec![item(8001, "x")]);
        with_test_registry_mut(|registry| {
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scene_panel", SceneOrWidgetSurface::Scene(scene))
                .expect("the scene leg stores");
            attach_publish(registry, "PLUGIN", "scene_panel", widgets)
                .expect("the widget leg stores");

            let (rows, total) = registry.panel_surface_store().snapshot_page("scene_panel");
            assert!(
                rows.len() <= ocs_plugin_api::widgets::MAX_WIDGET_ROWS_INITIAL,
                "the page stays within the wire budget"
            );
            match &rows[0] {
                StoreRow::State(digest) => {
                    let scene_summary =
                        digest.scene.as_ref().expect("the digest names the scene");
                    assert_eq!(scene_summary.revision, SceneRevision(7));
                    assert_eq!(scene_summary.nodes, 5_000);
                    assert!(digest.widgets.is_some(), "the digest names the widget leg");
                }
                other => panic!("snapshot2 leads with the digest: {other:?}"),
            }
            assert_eq!(total, 5_000, "the page continues through the region reads");
            match &rows[1] {
                StoreRow::Scene(element) => assert_eq!(element.stable_id(), sid(1)),
                other => panic!("the leading window carries elements: {other:?}"),
            }
        });
    }

    /// The publish report's caps carry EXACT truncation counts: a leg with
    /// more roots than the touched cap reports the first N plus the count
    /// of everything dropped (the scene report's own discipline).
    #[test]
    fn publish_report_truncation_counts_are_exact() {
        let _app = OpenCADStudio::new_for_test();
        let elements: Vec<ocs_plugin_api::scene::SceneElement> = (1..=70)
            .map(|i| ocs_plugin_api::scene::SceneElement::Rect {
                id: sid(i as u64),
                name: None,
                actions: vec![],
                drag: None,
                pos: Vec2([0.0, 0.0]),
                size: Vec2([4.0, 4.0]),
                fill: None,
                stroke: None,
            })
            .collect();
        let scene = ocs_plugin_api::scene::PanelScene {
            revision: SceneRevision(1),
            elements,
        };
        with_test_registry_mut(|registry| {
            let outcome = registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "wide_panel", SceneOrWidgetSurface::Scene(scene))
                .expect("the scene leg stores");
            let report = outcome.report;
            assert_eq!(
                report.touched_subtree_ids.len(),
                ocs_plugin_api::scene::MAX_REPORT_TOUCHED
            );
            assert_eq!(
                report.touched_truncated,
                70 - ocs_plugin_api::scene::MAX_REPORT_TOUCHED
            );
            // First-come order preserved.
            assert_eq!(report.touched_subtree_ids[0], sid(1));
        });
    }

    /// The surface-id convention's collision fallback: a second panel whose
    /// FIRST ROOT duplicates another panel's frozen identity is re-keyed by
    /// the hashed fallback — cross-surface uniqueness stays the contract,
    /// and each panel's identity freezes individually.
    #[test]
    fn surface_id_first_root_convention_with_hashed_collision_fallback() {
        let _app = OpenCADStudio::new_for_test();
        let a = simple_list_surface(SceneRevision(1), vec![item(201, "a")]);
        let b = simple_list_surface(SceneRevision(1), vec![item(202, "b")]);
        with_test_registry_mut(|registry| {
            attach_publish(registry, "PLUGIN_A", "panel_a", a).expect("attach");
            attach_publish(registry, "PLUGIN_B", "panel_b", b).expect("attach");
            let b_id = registry
                .panel_surface_id_owned("panel_b")
                .expect("frozen");
            assert_ne!(b_id, sid(200), "the collision fell back");
            // And the fallback is stable: re-registrations keep it.
            let forward = simple_list_surface(SceneRevision(2), vec![item(203, "b2")]);
            attach_publish(registry, "PLUGIN_B", "panel_b", forward).expect("forward");
            assert_eq!(registry.panel_surface_id_owned("panel_b"), Some(b_id));
        });
    }

}
