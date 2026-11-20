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
//! ADR-004 (API-24; the crossing's pre-merge label was API-23) crossing /
//! SCN-015 boundaries, and where they stand
//! after SCN-025 (the wire attach):
//! - the events the bridge drains: the COLLECTING sink ([`CollectingSink`])
//!   was the pipeline seam the future publisher hangs off — that publisher
//!   LANDED with SCN-025 (the drained batches forward through the v26
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
//! TABLES: the typed-table machine ([`mount_table_key_route_owned`],
//! consuming the frozen SWG-004b state layer) is the table keyboard scope —
//! Up/Down row roving within the materialized window, Left/Right the
//! caller-held cell cursor, Escape the FREE draft cancel. SCN-028 made the
//! scope LIVE: a def-present table renders as the REAL interactive typed
//! table ([`table_element`] — headers in def order, the machine's
//! materialized window, selection/focus/cursor/draft visible; a def-ABSENT
//! `TableRef` keeps the muted placeholder), the per-(surface, table)
//! [`TableUiState`] slots ride the APP-OWNED registry (created on surface
//! registration, `sync`ed on every surface swap, `reset_all` on teardown —
//! the §16 lifecycle), and production keys route THROUGH the pure table
//! route (`table_scope_key_owned` → [`mount_table_key_route_owned`]).
//! The SCOPE law (documented at [`key_route_owned`]): Tab at the widget
//! ring's edge DIVES IN (the table is the wrap target — a table-level ring
//! entry that dives, no widget-ring bytes move for non-table surfaces);
//! INSIDE, Tab/Shift+Tab ADVANCE/REVERSE the cell cursor (SCN-030 form
//! semantics: the row wraps at the column edge, an open draft COMMITS
//! first, and only the table's MATERIALIZED edge LEAVES to the ring;
//! SCN-035: the landing ARMS — a writable landing opens the seeded draft
//! immediately, Tab lands the user IN the editor);
//! Up/Down rove rows (a window edge
//! advances the scroll anchor; the PUBLISHED tail asks for the next page);
//! Left/Right rove the cell cursor; Enter arms/commits the edit (the
//! committed fact forwards as `WidgetEvent::TableCellChanged` through the
//! ONE drained-events path); Escape cancels an open draft FREE and falls
//! through to the armed-prompt law otherwise. Pointer presses ride the
//! bridge's typed-table route ([`route_mount_table_msg_owned`]): press
//! selects + moves the cell cursor, a press on the already-focused cursor
//! cell is the activation gesture.
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
#[cfg(not(target_arch = "wasm32"))]
use iced::mouse;
use iced::widget::tooltip::Position as TooltipPosition;
use iced::widget::{Tooltip, button, canvas, column, container, mouse_area, row, text};
use iced::{Element, Fill, Length, Point, Theme};
use iced_aw::menu::{Item, Menu, MenuBar};
// The v26 panel-surface store's vocabulary (SCN-025). Wire-adjacent STRUCT
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
use ocs_plugin_api::scene::{
    DragMeta, MarkerKind, PanelScene, Rect2D, SceneColor, SceneElement, SceneKindCounts,
    SceneRevision, StableId,
};
use ocs_plugin_api::widgets::{
    Axis, CellKind, IconActionDef, ItemListDef, MAX_MENU_DEPTH, MAX_WIDGET_RANGE_LEN,
    MAX_WIDGET_ROWS_INITIAL, MenuDef, PanelTableDef, TableRef, TableCell, TableRowDef, TreeRowDef,
    WidgetCellValue, WidgetEvent, WidgetGroup, WidgetNode, WidgetOp, WidgetSurface, WidgetTreeDef,
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
use crate::ui::window::plugin_widgets_tables::{
    is_patch_writable, CellFocus, TableUiState,
};
// (SCN-031) The retained scene-projection layer the mount's scene region
// replays — the seam consumes the SCN-011a/b cache verbatim, nothing of it
// is reinvented here.
#[cfg(not(target_arch = "wasm32"))]
use crate::ui::window::plugin_scene_cache::SceneProjectionCache;
// (SCN-032) The scene drag rail's MACHINE (SCN-021's `plugin_drag.rs` — the
// wire-free Begin/Move/Settle/Cancel state machine, untouched: this mount
// only WIRES it).
#[cfg(not(target_arch = "wasm32"))]
use crate::ui::window::plugin_drag::{
    DragCancelReason, DragPhase, DragSession, DragSettled, MoveDisposition,
};


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
    // -- SCN-028: the typed-table route vocabulary (host-local, NOT wire) --
    //
    // Three variants, each documented; ALL route through
    // [`route_mount_table_msg_owned`] (the typed-table bridge, whose body
    // lives in THIS file). Reaching [`apply_mount_msg`] instead is a bypass
    // defect and is refused by name there (the one-path discipline: the
    // drained-events forward + settle tail run exactly once per route).
    /// A pointer press on ONE materialized table cell (row key + column).
    /// The documented TWO-PHASE pointer policy: the first press SELECTS the
    /// row (the machine's `set_selection` — the host-multi-select law +
    /// `MAX_SELECTION_IDS` cap live THERE) and moves the cell cursor onto
    /// the cell; a press on the ALREADY-FOCUSED row's cursor cell is the
    /// ACTIVATION gesture (the edit begins when the column is
    /// patch-writable — the machine's gate; non-writable kinds render
    /// display-only and never emit this from the builder).
    TableRowPressed {
        /// The surface the mounted table renders on.
        surface_id: StableId,
        /// The registry table the pressed cell belongs to.
        table_id: StableId,
        /// The pressed row's stable key.
        key: StableId,
        /// The pressed column's stable id.
        column: StableId,
    },
    /// The keyboard commit's forward leg: `commit_cell_draft` produced ONE
    /// `WidgetOp::TableCell` fact; this variant carries the fact's notify
    /// shape through the ordinary bridge — the route builds
    /// `WidgetEvent::TableCellChanged`, the drained batch forwards through
    /// the ONE path (`forward_mount_events`), and the (key, column) address
    /// settles exactly-once AFTER the forward.
    TableCellCommitted {
        /// The surface the mounted table renders on.
        surface_id: StableId,
        /// The registry table the committed cell belongs to.
        table_id: StableId,
        /// The committed row key.
        key: StableId,
        /// The committed column id.
        column: StableId,
        /// The committed skeleton value (the machine's pending at commit).
        value: WidgetCellValue,
    },
    /// The published-tail landing ask: a gesture (keyboard roving at the
    /// materialized window's end, a press on the tail row) reached the LAST
    /// PUBLISHED row of a table that windows. The route calls the machine's
    /// `page_request_if_needed` exactly ONCE per gesture (the dedup rides
    /// the machine) and the `TablePageRequested` event forwards through the
    /// same drained path as every other table event.
    TableTailReached {
        /// The surface the mounted table renders on.
        surface_id: StableId,
        /// The registry table whose published tail was reached.
        table_id: StableId,
    },
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
        // `KeyPressed`), the window-level release has no surface, and the
        // SCN-028 typed-table vocabulary owns the typed-table bridge route
        // ([`route_mount_table_msg_owned`]) — reaching here means the
        // message BYPASSED its route (a defect); refuse by name, exactly
        // the pointer discipline.
        WidgetMountMsg::KeyPressed { .. }
        | WidgetMountMsg::PointerReleasedAnywhere
        | WidgetMountMsg::TableRowPressed { .. }
        | WidgetMountMsg::TableCellCommitted { .. }
        | WidgetMountMsg::TableTailReached { .. } => {
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
        | WidgetMountMsg::PointerReleased { surface_id, .. }
        | WidgetMountMsg::TableRowPressed { surface_id, .. }
        | WidgetMountMsg::TableCellCommitted { surface_id, .. }
        | WidgetMountMsg::TableTailReached { surface_id, .. } => *surface_id,
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
// SCN-028 — the typed-table bridge route (the table vocabulary's ONE path)
// ---------------------------------------------------------------------------

/// What ONE typed-table bridge route did — the table sibling of the app
/// module's `MountBridgeOutcome` (the fields the app arm consumes; the
/// settle bookkeeping stays inside [`route_mount_table_msg_owned`], so the
/// app shim needs only the events).
pub(crate) struct MountTableRouteOutcome {
    /// The typed-table route's verdict (the assertion surface; the app arm
    /// consumes only the events).
    #[cfg_attr(not(test), allow(dead_code))]
    pub routing: MountRouting,
    /// The events the collecting sink drained, in emission order — the
    /// forward half (`forward_mount_events` is the app arm's next call).
    pub events: Vec<WidgetEvent>,
}

/// Route ONE typed-table message into the registry-owned [`TableUiState`]
/// machine — the table vocabulary's bridge core. The app arm delegates the
/// same way `handle_mount_key_owned` delegates to
/// [`bench_handle_mount_key_owned`]: the canonical body lives HERE (the
/// SCN-025 D1 fold-back discipline; the app file gains no table logic).
///
/// Forward-then-settle, bridge-identical: the sink drains after the machine
/// returned, the settle runs after the drain — the committed fact's
/// (key, column) address clears exactly-once AFTER its
/// [`WidgetEvent::TableCellChanged`] left through the sink (the drained
/// batch forwards through the ONE path, [`MAX_WIDGET_EVENT_ROWS`]-style
/// batching included app-side). An unknown/unregistered table machine is a
/// graceful refusal — nothing half-applies, nothing forwards.
pub(crate) fn route_mount_table_msg_owned(
    registry: &mut MountRegistry,
    msg: &WidgetMountMsg,
) -> MountTableRouteOutcome {
    let surface_id = msg_surface_id(msg);
    let mut sink = CollectingSink::default();
    let routing = apply_mount_table_msg_owned(registry, surface_id, msg, &mut sink);
    // (1) the sink drains, THEN (2) the settle tail (the value is the
    // machine's "a fact was parked" answer — the bridge consumes nothing
    // from it; the machine's counters are the probe).
    let events = sink.take();
    if let WidgetMountMsg::TableCellCommitted {
        table_id,
        key,
        column,
        ..
    } = msg
    {
        registry.table_settle_owned(surface_id, *table_id, *key, *column);
    }
    registry.record_message_routed_owned();
    MountTableRouteOutcome { routing, events }
}

/// The typed-table route's apply half (pure routing over the registry-owned
/// machines; see [`route_mount_table_msg_owned`]).
fn apply_mount_table_msg_owned<S: WidgetEventSink + ?Sized>(
    registry: &mut MountRegistry,
    surface_id: StableId,
    msg: &WidgetMountMsg,
    sink: &mut S,
) -> MountRouting {
    let (table_id, key, column) = match msg {
        WidgetMountMsg::TableRowPressed {
            table_id,
            key,
            column,
            ..
        } => (*table_id, Some(*key), Some(*column)),
        WidgetMountMsg::TableCellCommitted {
            table_id,
            key,
            column,
            ..
        } => (*table_id, Some(*key), Some(*column)),
        WidgetMountMsg::TableTailReached { table_id, .. } => (*table_id, None, None),
        _ => {
            return MountRouting::Refused(
                "not a typed-table message; the table bridge refuses".to_string(),
            )
        }
    };
    // The cursor read lands BEFORE the machine borrow (the scope and the
    // machines are disjoint registry fields, but the two-phase policy needs
    // the pre-press cursor anyway).
    let cursor_before = registry.table_scope_cursor_of(surface_id, table_id);
    let Some(machine) = registry.table_machine_mut_owned(surface_id, table_id) else {
        return MountRouting::Refused(format!(
            "no typed-table machine for surface {} table {} (registration/sync defect)",
            surface_id.0, table_id.0
        ));
    };
    match msg {
        WidgetMountMsg::TableRowPressed { key, column, .. } => {
            // The documented TWO-PHASE pointer policy: press = select +
            // cursor; press on the ALREADY-focused row's cursor cell =
            // activation (the edit begins — the machine's gate decides;
            // non-writable kinds refuse QUIETLY, its counter the probe).
            let already_here = machine.focus() == Some(*key)
                && cursor_before == Some(CellFocus {
                    key: *key,
                    column: *column,
                });
            let mut forwarded = 0usize;
            // 1. Selection — the machine's host-multi-select law + cap.
            match machine.set_selection(vec![*key]) {
                Ok(Some(event)) => {
                    sink.emit(event);
                    forwarded += 1;
                }
                Ok(None) => {}
                Err(reason) => return MountRouting::Refused(reason.to_string()),
            }
            // 2. The click hand-off focus (the machine's window-bound gate).
            if let Err(reason) = machine.row_focus_set(*key, &DEFAULT_WINDOW_SPEC) {
                return MountRouting::Refused(reason.to_string());
            }
            // 3. The SECOND phase on an already-cursor cell: the edit arms.
            if already_here {
                let _ = machine.begin_cell_edit(*key, *column);
            }
            // 4. A press landing on the PUBLISHED TAIL of a windowing table
            //    asks for the next page (the machine dedups).
            if machine_at_published_tail(machine, *key) {
                if let Some(event) = table_tail_page_ask(machine) {
                    sink.emit(event);
                    forwarded += 1;
                }
            }
            // The cursor moves with the press (after the machine borrow ends).
            let pressed_cell = CellFocus {
                key: *key,
                column: *column,
            };
            registry.table_scope_set_cursor_owned(surface_id, table_id, Some(pressed_cell));
            // (SCN-030) A SUCCESSFUL press CLAIMS the keyboard — the same
            // desktop input model `mount_press_owned` runs for the
            // surface-level press (the clicked control owns the keys). The
            // table route never did, so the scope's keys were only reached
            // through the SOLE-mount fallback — broken the moment two mount
            // surfaces exist.
            registry.set_mount_key_target_owned(Some(surface_id));
            MountRouting::Applied { forwarded }
        }
        WidgetMountMsg::TableCellCommitted { value, .. } => {
            let event = WidgetEvent::TableCellChanged {
                surface_id,
                row_key: key.expect("the commit variant carries its key"),
                column_id: column.expect("the commit variant carries its column"),
                value: value.clone(),
            };
            // Construct-level validity (§15's ONE validity path).
            if let Err(reason) = event.validate() {
                return MountRouting::Refused(format!(
                    "the committed cell fact failed the crate's validation: {reason}"
                ));
            }
            sink.emit(event);
            MountRouting::Applied { forwarded: 1 }
        }
        WidgetMountMsg::TableTailReached { .. } => match table_tail_page_ask(machine) {
            Some(event) => {
                sink.emit(event);
                MountRouting::Applied { forwarded: 1 }
            }
            // Not the turn (not the tail, nothing left, already in flight —
            // the machine's dedup counted): a benign no-op.
            None => MountRouting::Applied { forwarded: 0 },
        },
        _ => unreachable!("the (table_id, key, column, value) match above totalizes"),
    }
}

/// `true` when `key` is the machine's LAST PUBLISHED row (the tail a page
/// request extends — the machine's keyed-window rule).
fn machine_at_published_tail(machine: &TableUiState, key: StableId) -> bool {
    machine
        .synced()
        .and_then(|synced| synced.rows.last())
        .copied()
        == Some(key)
}

/// The machine's next-page ask for the CURRENT published tail (`None` when
/// the table does not window, has nothing left, or the request is in
/// flight — the machine's own rules; the dedup counters are its probes).
fn table_tail_page_ask(machine: &mut TableUiState) -> Option<WidgetEvent> {
    let tail = machine
        .synced()
        .and_then(|synced| synced.rows.last())
        .copied()?;
    machine.page_request_if_needed(tail, MAX_WIDGET_RANGE_LEN)
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

/// Build the TABLE placeholder for one [`TableRef`] — the def-ABSENT
/// regime (SCN-028): the ref names a table id with NO registry def (or the
/// surface's machine has no synced read model yet), so there is nothing to
/// render interactively. A labeled, muted, never-interactive note; the
/// def-PRESENT regime renders the REAL typed table ([`table_element`]).
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

// ---------------------------------------------------------------------------
// SCN-028 — the REAL typed-table element (the def-present regime)
// ---------------------------------------------------------------------------

/// The builder context the typed-table element reads: the surface being
/// built (the table defs' registry), the registry-owned per-(surface,
/// table) machine slots, and the table scope's caller-held cell cursor.
/// Threading is view-pass only — the builder NEVER mutates a machine (the
/// machines decide; the routes apply).
pub(crate) struct TableMountCtx<'a> {
    /// The surface being built (its `tables` registry is the def source).
    pub surface: &'a WidgetSurface,
    /// The per-table machines of the surface being built.
    pub tables: &'a HashMap<StableId, TableUiState>,
    /// The cell cursor when the table keyboard scope is active.
    pub cell_cursor: Option<CellFocus>,
}

/// The headless VIEW MODEL of one typed table — what the builder paints
/// and the tests assert (the builder consumes exactly this; nothing is
/// computed twice).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableViewModel {
    /// Header labels in `def.columns` order (the validated registry order).
    pub headers: Vec<String>,
    /// The columns in the same order (the cell-cursor roving order).
    pub column_ids: Vec<StableId>,
    /// Per column: is it a scalar patch-writable kind (an editable cell) —
    /// the machine's OWN predicate; non-writable kinds render display-only.
    pub writable: Vec<bool>,
    /// The MATERIALIZED window's row keys in paint order (the machine's
    /// `materialize_rows` — the §16 structural windowing, never beyond the
    /// published payload).
    pub row_keys: Vec<StableId>,
    /// The machine's selected row keys.
    pub selected: Vec<StableId>,
    /// The machine's focused row.
    pub focused: Option<StableId>,
    /// The open draft's address + pending value's display text.
    pub draft: Option<(StableId, StableId, String)>,
    /// Per materialized row, per column: the cell's display text (cells
    /// shadow the columns 1:1 by position — the crate's validated contract).
    pub cells: Vec<Vec<String>>,
    /// The caller-held cell cursor when the table keyboard scope is active.
    pub cursor: Option<CellFocus>,
    /// The def's backing total when it windows (the footer's metadata —
    /// NEVER a materialization bound).
    pub total_rows: Option<u64>,
    /// The published row count (the scrollbar/footer's real bound).
    pub published_rows: usize,
}

/// Project one def-present table's view model (pure; the machine supplies
/// every state fact — selection, focus, draft, windowing; the def supplies
/// the ordered columns + typed cells).
fn table_view_model(
    def: &PanelTableDef,
    machine: &TableUiState,
    cell_cursor: Option<CellFocus>,
    spec: &WindowSpec,
) -> TableViewModel {
    let synced = machine
        .synced()
        .expect("the def-present guard checked the synced read model");
    let materialized = machine.materialize_rows(spec);
    let by_key: HashMap<StableId, &TableRowDef> =
        def.rows.iter().map(|row| (row.key, row)).collect();
    let headers: Vec<String> = def.columns.iter().map(|column| column.label.clone()).collect();
    let column_ids: Vec<StableId> = def.columns.iter().map(|column| column.id).collect();
    let writable: Vec<bool> = def
        .columns
        .iter()
        .map(|column| is_patch_writable(column.kind))
        .collect();
    let mut cells = Vec::with_capacity(materialized.keys.len());
    for key in &materialized.keys {
        let mut row_cells = Vec::with_capacity(def.columns.len());
        if let Some(row) = by_key.get(key) {
            // Cells shadow the columns 1:1 BY POSITION (the crate's own
            // validated contract; the machine already gated the shape).
            for position in 0..def.columns.len() {
                row_cells.push(
                    row.cells
                        .get(position)
                        .map(cell_display_text)
                        .unwrap_or_default(),
                );
            }
        }
        cells.push(row_cells);
    }
    let draft = machine
        .draft()
        .map(|draft| (draft.key, draft.column, pending_display(&draft.pending)));
    TableViewModel {
        headers,
        column_ids,
        writable,
        row_keys: materialized.keys,
        selected: machine.selection().to_vec(),
        focused: machine.focus(),
        draft,
        cells,
        cursor: cell_cursor,
        total_rows: synced.total_rows,
        published_rows: materialized.published_rows,
    }
}

/// The display text of one typed cell (the machine's skeleton for scalar
/// kinds, the def's presentation payload otherwise — all bounded def data).
fn cell_display_text(cell: &TableCell) -> String {
    match cell {
        TableCell::Text(text) => text.clone(),
        TableCell::Number(number) => format!("{number}"),
        TableCell::Bool(value) => value.to_string(),
        TableCell::Badge { label, .. } => label.clone(),
        TableCell::Link { label, .. } => label.clone(),
        TableCell::Icon(_) => "icon".to_string(),
        TableCell::Progress { fraction, label } => label
            .clone()
            .unwrap_or_else(|| format!("{:.0}%", fraction * 100.0)),
        TableCell::Date { y, m, d } => format!("{y:04}-{m:02}-{d:02}"),
    }
}

/// Build the REAL interactive typed table for one [`TableRef`] (SCN-028 —
/// the def-PRESENT regime; the def-ABSENT regime keeps
/// [`table_placeholder_element`], decided HERE so both regimes are
/// documented at one site):
///
/// - **def present** (`TableRef::table_id` resolves in the surface's
///   registry AND the registry-owned machine holds a synced read model):
///   semantic iced widgets only — the column headers in `def.columns`
///   order, the machine's MATERIALIZED window rows (the §16 structural
///   windowing: never beyond the published payload), row selection/focus
///   and the cell cursor visible, and an open draft visibly editing (the
///   pending value with the `✎ …▏` editor marker). Scalar
///   patch-writable cells (the machine's own `is_patch_writable` rule)
///   carry the press route ([`WidgetMountMsg::TableRowPressed`] — the
///   two-phase pointer policy); Badge and the other non-writable kinds
///   render display-only (muted text, NO press route — the machine would
///   refuse the edit anyway).
/// - **def absent**: the muted, never-interactive placeholder (a `TableRef`
///   naming an id with no registry def renders exactly the pre-SCN-028
///   bytes).
pub(crate) fn table_element(
    table: &TableRef,
    surface: &WidgetSurface,
    ctx: &TableMountCtx<'_>,
    spec: &WindowSpec,
) -> (Element<'static, WidgetMountMsg>, MountFacts) {
    let name = table.name.clone().unwrap_or_else(|| "table".to_string());
    let def_present = surface.tables.contains_key(&table.table_id)
        && ctx
            .tables
            .get(&table.table_id)
            .is_some_and(|machine| machine.synced().is_some());
    if !def_present {
        return table_placeholder_element(table);
    }
    let machine = ctx
        .tables
        .get(&table.table_id)
        .expect("the def-present guard checked the machine slot");
    let def = surface
        .tables
        .get(&table.table_id)
        .expect("the def-present guard checked the registry");
    let view = table_view_model(def, machine, ctx.cell_cursor, spec);
    let surface_id = machine.surface_id();
    let table_id = table.table_id;

    let mut col = column![].spacing(MOUNT_ROW_SPACING).width(Fill);
    // Header row: the def's labels in registry order (muted — the rows
    // carry the content).
    let mut header = row![].spacing(6);
    for label in &view.headers {
        header = header.push(
            text(label.clone())
                .size(MOUNT_HINT_TEXT_SIZE)
                .style(muted_text_style),
        );
    }
    col = col.push(header);
    // Materialized rows: one native row of cells per window key.
    for (row_index, key) in view.row_keys.iter().enumerate() {
        let selected = view.selected.contains(key);
        let focused = view.focused == Some(*key);
        let mut row_el = row![].spacing(6).align_y(iced::Alignment::Center);
        for (column_index, column_id) in view.column_ids.iter().enumerate() {
            let mut label = view
                .cells
                .get(row_index)
                .and_then(|cells| cells.get(column_index))
                .cloned()
                .unwrap_or_default();
            if focused && column_index == 0 {
                label = format!("> {label}");
            }
            if view.cursor == Some(CellFocus {
                key: *key,
                column: *column_id,
            }) {
                label = format!("[{label}]");
            }
            // The open draft IS the editor (per-kind pending, visibly
            // editing — the commit/cancel keys live in the keyboard route).
            if view
                .draft
                .as_ref()
                .is_some_and(|(draft_key, draft_column, _)| {
                    *draft_key == *key && *draft_column == *column_id
                })
            {
                let pending = view
                    .draft
                    .as_ref()
                    .map(|(_, _, text)| text.clone())
                    .unwrap_or_default();
                row_el = row_el.push(text(format!("✎ {pending}▏")).size(MOUNT_TEXT_SIZE));
                continue;
            }
            if view.writable[column_index] {
                let press = button(text(label).size(MOUNT_TEXT_SIZE))
                    .padding([2, 6])
                    .style(button_style(selected))
                    .on_press(WidgetMountMsg::TableRowPressed {
                        surface_id,
                        table_id,
                        key: *key,
                        column: *column_id,
                    });
                row_el = row_el.push(press);
            } else {
                // Display-only kinds (badge/icon/link/progress/date):
                // plugin-PUBLISHED, never patch-writable — muted text, no
                // press route.
                row_el = row_el.push(
                    text(label)
                        .size(MOUNT_TEXT_SIZE)
                        .style(muted_text_style),
                );
            }
        }
        col = col.push(row_el);
    }
    // The windowing footer: what is materialized vs published vs backing —
    // the §16 window discipline made visible (metadata only).
    let window_note = match view.total_rows {
        Some(total) => format!(
            "rows {}–{} of {} published ({} backing)",
            view.row_keys.first().map(|key| key.0).unwrap_or(0),
            view.row_keys.last().map(|key| key.0).unwrap_or(0),
            view.published_rows,
            total,
        ),
        None => format!(
            "rows {}–{} of {}",
            view.row_keys.first().map(|key| key.0).unwrap_or(0),
            view.row_keys.last().map(|key| key.0).unwrap_or(0),
            view.published_rows,
        ),
    };
    col = col.push(
        text(window_note)
            .size(MOUNT_HINT_TEXT_SIZE)
            .style(muted_text_style),
    );

    (
        col.into(),
        MountFacts {
            widget_id: table.id,
            clickable: true,
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
    table_ctx: &TableMountCtx<'_>,
) -> Option<Element<'a, WidgetMountMsg>> {
    if !node_visible(node) {
        return None;
    }
    Some(match node {
        WidgetNode::Tree(def) => tree_element(def, state, spec).0,
        WidgetNode::List(def) => list_element(def, state, spec).0,
        WidgetNode::IconButton(def) => icon_button_element(def, dispatcher).0,
        WidgetNode::Menu(def) => menu_element(def, dispatcher).0,
        WidgetNode::Table(def) => table_element(def, table_ctx.surface, table_ctx, spec).0,
        WidgetNode::Group(def) => {
            let children = visible_children(def, state, dispatcher, spec, table_ctx);
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
    table_ctx: &TableMountCtx<'_>,
) -> Vec<Element<'a, WidgetMountMsg>> {
    def.children
        .iter()
        .filter_map(|child| build_node(child, state, dispatcher, spec, table_ctx))
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
    /// model syncs at REGISTRATION, when
    /// [`MountRegistry::register_surface_owned`] walks the surface's
    /// button/menu defs into it, SCN-030). The §16 "closing a tab cancels
    /// pending requests and releases state" teardown is
    /// [`MountRegistry::teardown_surface_owned`] (SCN-014).
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
    /// (SCN-028) The typed-table context rides in: the surface itself, the
    /// registry-owned per-table machines, and the scope's cell cursor —
    /// view-pass reads only.
    ///
    /// (SCN-031) THE FIXED SPLIT (v1, recorded): when the panel's store
    /// holds a scene leg, the scene region composes ABOVE the widget stack
    /// as the column's FIRST child, at a FIXED height
    /// ([`SCENE_REGION_V1_HEIGHT`]) — inside the panel's scroll area the
    /// main axis is unbounded, so a `Fill`-height region would resolve to
    /// infinite height; fixed is the simple, correct v1 split. A scene-only
    /// panel renders the region as the whole body (the stack below is
    /// empty); a widgets-only panel receives `None` and builds EXACTLY
    /// today's element tree (byte-identical by construction — no scene
    /// element ever enters the column). A SceneRef widget variant that
    /// would let a plugin place the scene inside its own widget tree is the
    /// later refinement, deliberately NOT this package.
    fn body(
        &self,
        tables: &HashMap<StableId, TableUiState>,
        cell_cursor: Option<CellFocus>,
        scene_region: Option<Element<'static, WidgetMountMsg>>,
    ) -> Element<'static, WidgetMountMsg> {
        let state = self.state.borrow();
        let dispatcher = self.dispatcher.borrow();
        let table_ctx = TableMountCtx {
            surface: &self.surface,
            tables,
            cell_cursor,
        };
        let mut col = column![].spacing(MOUNT_GROUP_SPACING).width(Fill);
        // (SCN-031) The scene region first (the fixed split, see the doc
        // above); `None` builds exactly the pre-seam tree.
        if let Some(scene_region) = scene_region {
            col = col.push(scene_region);
        }
        for node in &self.surface.roots {
            if let Some(element) = build_node(node, &state, &dispatcher, &DEFAULT_WINDOW_SPEC, &table_ctx) {
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
    /// (SCN-028) The per-surface TYPED-TABLE store: one [`TableUiState`]
    /// slot per (surface, table) — the machine map keyed by the registry
    /// table id — plus that surface's table KEYBOARD scope (which table
    /// holds the roving position, the caller-held cell cursor, and the
    /// editing compose buffer; see [`SurfaceTableState`]).
    ///
    /// Registry-owned ON PURPOSE (the SCN-025 store precedent: the store
    /// rides inside the registry — one app-`Rc` domain, one borrow
    /// discipline, one lifecycle): the slots are created on surface
    /// registration, `sync`ed on EVERY successful surface swap (the same
    /// sync points `WidgetUiState` re-syncs — snapshots and applied
    /// patches; selection/focus/draft survive patches that do not touch
    /// their keys, the machine's documented contract), and `reset_all` +
    /// dropped on teardown ([`MountRegistry::prune_input_entries`] — the
    /// §16 "closing a tab/plugin cancels pending requests and releases
    /// state" rule, tables edition). Host-local only, never serialized.
    tables: HashMap<StableId, SurfaceTableState>,
    /// (SCN-025) The app-owned PANEL SURFACE STORE — the v26
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
    /// (SCN-031) The per-panel SCENE-PROJECTION cache slots — the retained
    /// side of the scene-mount render seam: one
    /// `Rc<RefCell<SceneProjectionCache>>` per panel id (the store's key;
    /// see [`Self::scene_cache_slot_owned`]), shared with the composed
    /// canvas program so the element tree's per-frame rebuild never
    /// rebuilds the retained projection with it. Created on first need (a
    /// scene-only panel has no registration to hang creation on), KEPT
    /// across the SCN-033 same-surface carries, DROPPED on the §16
    /// teardown and the teardown-class replace (the same lifecycle rule as
    /// the input store). Registry-adjacent ON PURPOSE: one borrow domain
    /// with the store it re-keys against, one lifecycle. Host-local only,
    /// never serialized.
    #[cfg(not(target_arch = "wasm32"))]
    scene_caches: RefCell<HashMap<String, Rc<RefCell<SceneProjectionCache>>>>,
    /// (SCN-032) The per-surface SCENE DRAG slots — SCN-021's
    /// [`DragSession`] machines (one live drag per surface at most, the
    /// machine owns every threshold/preview/settle decision) plus the
    /// settle-bound events awaiting the forward drain. One `Rc` slot per
    /// surface id, SHARED with the composed scene-region canvas program
    /// (exactly the `scene_caches` sharing law: the element tree rebuilds
    /// every view pass, the machine slot does not) — the canvas's pointer
    /// events drive the session through it, the registry-side routes
    /// (Escape cancel, §16 teardown, the release drain that forwards the
    /// settle event) reach the SAME machine. Born when a panel's scene
    /// region first composes over a REGISTERED widget surface (the
    /// scene-only shape keeps the pointer fence — no surface, no slot, no
    /// interaction; the demo's panel carries both legs), pruned with the
    /// surfaces like every other per-surface store.
    #[cfg(not(target_arch = "wasm32"))]
    scene_drags: RefCell<HashMap<StableId, Rc<RefCell<SceneDragSlot>>>>,
}

/// (SCN-032) One surface's scene-drag slot: the [`DragSession`] machine plus
/// the bookkeeping the wiring needs around it. Host-local only, never
/// serialized (§3 transient class); the machine's OWN invariants
/// (losslessness, exactly-once completion, latest-wins parks) live in
/// `plugin_drag.rs` — nothing here re-implements them.
#[cfg(not(target_arch = "wasm32"))]
struct SceneDragSlot {
    /// The SCN-021 machine. Its synced draggable set is re-synced at every
    /// region composition (the machine's REPLACE contract — idempotent);
    /// an in-flight drag's own origin was validated at arm time and survives.
    session: DragSession,
    /// The settle-bound [`WidgetEvent`]s awaiting the forward drain (one per
    /// settled drag — the machine's exactly-once discipline is what bounds
    /// this vec). Drained by [`MountRegistry::mount_pointer_released_owned`]
    /// (the release route the body wrapper already publishes) and forwarded
    /// through the same broadcast call the drained-events path uses.
    pending_events: Vec<WidgetEvent>,
    /// The monotonic logical press tick (the machine's `start_tick` is a
    /// caller-supplied headless/route input, never wall-clock). One tick per
    /// press the region canvas saw, per surface.
    next_tick: u64,
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
    // (the tests' gate probe ([`mounts_surface`]) still consumes this; the
    // wire-era registration path HAS landed (SCN-025), but this probe
    // remains a test-side read — production paths own their registration.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn mount_registered_owned(&self, panel_id: &str) -> bool {
        self.by_panel.contains_key(panel_id)
    }

    /// The mounted body for `panel_id` (see [`SurfaceMountHandle::body`]);
    /// `None` = no registered mount AND no stored scene leg → the caller
    /// renders its legacy view. (SCN-028) The typed-table context resolves
    /// HERE — the registry owns the machine slots + the scope's cursor, and
    /// the body builder is a pure function over them.
    ///
    /// (SCN-031) THE STORE→MOUNT READ PATH: the panel's scene leg is read
    /// from the store HERE, per view pass, through
    /// [`Self::scene_leg_element_owned`] — the store stays the SOLE
    /// scene-truth owner; nothing scene-shaped is stored registry-side
    /// beyond the cache's prim output ([`Self::scene_caches`]). The gate
    /// widens accordingly: a panel whose store holds ONLY a scene leg (no
    /// widget leg, no registration — the demo's `scn_panel` shape) now
    /// mounts a full-body scene region; zero entries in BOTH the registry
    /// and the store still means the byte-identical legacy path by
    /// construction (the legacy pin keeps green).
    pub(crate) fn mounted_body_owned(
        &self,
        panel_id: &str,
    ) -> Option<Element<'static, WidgetMountMsg>> {
        let scene_leg = self.scene_leg_element_owned(panel_id);
        let handle = self.by_panel.get(panel_id);
        match handle {
            Some(handle) => {
                let surface_id = handle.surface_id();
                let empty = HashMap::new();
                let entry = self.tables.get(&surface_id);
                let machines = entry.map(|entry| &entry.machines).unwrap_or(&empty);
                let cell_cursor = entry
                    .and_then(|entry| entry.scope.as_ref())
                    .and_then(|scope| scope.cursor);
                Some(handle.body(machines, cell_cursor, scene_leg))
            }
            // Scene-only panel (the store holds the leg, no widget leg ever
            // registered): the scene region IS the body — the same padded
            // container the widget body uses.
            None if scene_leg.is_some() => Some(
                container(scene_leg.expect("gated by the scene leg read"))
                    .padding([8, 10])
                    .width(Fill)
                    .into(),
            ),
            None => None,
        }
    }

    /// (SCN-031) The mounted body's SHAPE — the same gate
    /// [`Self::mounted_body_owned`] resolves, named for the composition
    /// pins (headless: which regions the body composes, without building
    /// iced elements). `None` = the legacy path (neither leg present).
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn body_shape_owned(&self, panel_id: &str) -> Option<MountBodyShape> {
        let scene = self
            .panel_surfaces
            .entry(panel_id)
            .is_some_and(|entry| entry.scene.is_some());
        let widgets = self.by_panel.contains_key(panel_id);
        match (widgets, scene) {
            (true, true) => Some(MountBodyShape::SceneAndWidgets),
            (true, false) => Some(MountBodyShape::WidgetsOnly),
            (false, true) => Some(MountBodyShape::SceneOnly),
            (false, false) => None,
        }
    }

    /// (SCN-031) The panel's scene region element, read per view pass from
    /// the store's scene leg; `None` when the panel stores no scene (the
    /// widgets-only and legacy paths stay exactly as they were).
    #[cfg(not(target_arch = "wasm32"))]
    fn scene_leg_element_owned(
        &self,
        panel_id: &str,
    ) -> Option<Element<'static, WidgetMountMsg>> {
        let scene = self.panel_surfaces.entry(panel_id)?.scene.as_ref()?;
        Some(self.scene_region_element_owned(panel_id, scene))
    }

    /// (wasm32) No panel surface store — no scene leg, no scene region; the
    /// widget-only/legacy paths are the whole story there.
    #[cfg(target_arch = "wasm32")]
    fn scene_leg_element_owned(&self, _panel_id: &str) -> Option<Element<'static, WidgetMountMsg>> {
        None
    }

    // -- SCN-031: the scene-mount render seam (the retained side) ----------

    /// The panel's scene-projection cache slot, created on first need (the
    /// generalization of the SCN-028 slot law: a SCENE-ONLY panel has no
    /// widget registration to create the slot at, so the slot is born when
    /// the first view pass composes the region). One slot per PANEL id —
    /// the store's own key and the scene leg's vocabulary; a scene-only
    /// panel has no `surface_id` to key by (the recorded deviation from the
    /// per-surface table slots, which key a widget leg that may not exist).
    ///
    /// The `&self` + `RefCell` shape: the VIEW pass takes the registry
    /// through an immutable borrow (`plugin_panel.rs`'s
    /// `mount_store.borrow()`), and the slot map is the one registry-side
    /// structure the view path must be able to grow. Interior-mutable like
    /// the cache layer's own key cell; single UI thread, no cross-thread
    /// sharing (the registry's discipline).
    ///
    /// RETAINED LAW: the slot is a plain `Rc` handle shared with the
    /// composed canvas program — the element tree is rebuilt every view
    /// pass, the cache is NOT: consecutive draws at an equal
    /// [`crate::ui::window::plugin_scene_cache::SceneLayerKey`]
    /// (revision + bounds + palette fingerprint) replay the cached frame
    /// and `project_scene` never runs (the counters prove it). No sync-on-
    /// swap is needed BEYOND the key math: the program re-reads the store's
    /// LIVE scene each frame and the cache re-keys against it — a publish's
    /// replaced leg always arrives at a greater revision or identical
    /// content (the store's monotone law), so a content change is always a
    /// revision change. The cache therefore holds no scene copy to go
    /// stale.
    #[cfg(not(target_arch = "wasm32"))]
    fn scene_cache_slot_owned(&self, panel_id: &str) -> Rc<RefCell<SceneProjectionCache>> {
        // The cache's `surface_id` is diagnostic only (its Debug/records);
        // the frozen widget identity when one exists, else the no-widget
        // placeholder.
        let surface_id = self
            .panel_surfaces
            .entry(panel_id)
            .and_then(|entry| entry.surface_id)
            .unwrap_or(StableId(0));
        Rc::clone(
            self.scene_caches
                .borrow_mut()
                .entry(panel_id.to_owned())
                .or_insert_with(|| Rc::new(RefCell::new(SceneProjectionCache::new(surface_id)))),
        )
    }

    /// The scene cache slot, read-only (no insert) — the lifecycle pins'
    /// probe (teardown drops, the carry keeps) and the tests' counter
    /// handle.
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn scene_cache_slot_read_owned(
        &self,
        panel_id: &str,
    ) -> Option<Rc<RefCell<SceneProjectionCache>>> {
        self.scene_caches.borrow().get(panel_id).cloned()
    }

    /// Drop the panel's scene cache slot (the §16 release rule, scene
    /// edition): the explicit teardown and the teardown-class REPLACE call
    /// this; the SCN-033 same-surface carry deliberately does NOT (the
    /// retained prims stay valid — the same machines, the same scene leg).
    #[cfg(not(target_arch = "wasm32"))]
    fn drop_scene_cache_slot(&mut self, panel_id: &str) {
        self.scene_caches.borrow_mut().remove(panel_id);
    }

    /// (wasm32) No scene-projection cache — nothing to drop; the §16
    /// release rule's scene edition is a no-op there (the paired stub of
    /// the desktop arm above).
    #[cfg(target_arch = "wasm32")]
    fn drop_scene_cache_slot(&mut self, _panel_id: &str) {}

    // -- SCN-032: the scene drag rail (the machine + the wiring) ------------

    /// The surface's scene-drag slot, born on first need (the cache-slot
    /// law generalized: a scene region over a REGISTERED widget surface
    /// creates the slot at composition; the scene-only shape never calls
    /// this — its region keeps the pointer fence). Shared `Rc` with the
    /// composed canvas program; registry-side routes reach the same machine
    /// through this map.
    #[cfg(not(target_arch = "wasm32"))]
    fn scene_drag_slot_owned(&self, surface_id: StableId) -> Rc<RefCell<SceneDragSlot>> {
        Rc::clone(
            self.scene_drags
                .borrow_mut()
                .entry(surface_id)
                .or_insert_with(|| {
                    Rc::new(RefCell::new(SceneDragSlot {
                        session: DragSession::new(surface_id),
                        pending_events: Vec::new(),
                        next_tick: 0,
                    }))
                }),
        )
    }

    /// The scene-drag slot read (the tests' handle; `None` = no slot — the
    /// surface never composed a scene region over a registered mount).
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(not(test), allow(dead_code))]
    fn scene_drag_slot_read_owned(
        &self,
        surface_id: StableId,
    ) -> Option<Rc<RefCell<SceneDragSlot>>> {
        self.scene_drags.borrow().get(&surface_id).cloned()
    }

    /// The machine-direct scene-drag cancel (Escape today): end the
    /// surface's live drag with NO semantic exit (the §7 cancel side — the
    /// plugin receives nothing, the landed lossless no-op). Returns `true`
    /// when a live interaction was completed. Like the armed-prompt
    /// dismissal, this is a HOST control operation routed machine-direct —
    /// no message exists for it and no machine sees a KEY.
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn scene_drag_cancel_owned(&mut self, surface_id: StableId, reason: DragCancelReason) -> bool {
        let Some(slot) = self.scene_drags.borrow().get(&surface_id).cloned() else {
            return false;
        };
        let cancelled = slot.borrow_mut().session.cancel(reason).is_ok();
        cancelled
    }

    /// Drain the surface's settle-bound events and forward them the way
    /// every drained-events batch forwards: the §16 forward probe
    /// ([`Self::record_panel_events_forwarded_owned`]) plus the v26 notify
    /// router (`broadcast_panel_scene_events`, bounded batches — the settle
    /// is exactly one row). This is the RELEASE route's tail: the body
    /// wrapper's `PointerReleased` (and the window-level release-anywhere
    /// catcher) both land in
    /// [`Self::mount_pointer_released_owned`], which drains here — so the
    /// settle event rides the mount's ONE forwarding shape without a second
    /// code path through the bridge. An unresolvable panel id is the
    /// bridge's own grace mirrored: drained and dropped, never re-delivered.
    #[cfg(not(target_arch = "wasm32"))]
    fn forward_scene_drag_events_owned(&mut self, surface_id: StableId) {
        let Some(slot) = self.scene_drags.borrow().get(&surface_id).cloned() else {
            return;
        };
        let events = std::mem::take(&mut slot.borrow_mut().pending_events);
        if events.is_empty() {
            return;
        }
        self.record_panel_events_forwarded_owned(events.len());
        if let Some(panel_id) = self.panel_for_surface_owned(surface_id) {
            crate::plugin::external::with_manager(|manager| {
                for batch in
                    events.chunks(ocs_plugin_api::ipc::protocol::MAX_WIDGET_EVENT_ROWS)
                {
                    manager.broadcast_panel_scene_events(&panel_id, surface_id, batch);
                }
            });
        }
    }

    /// The scene region element: an iced `canvas` whose program pulls the
    /// CACHED prims ([`SceneProjectionCache::draw`]) — on a layer-key miss
    /// (revision / bounds / palette fingerprint changed) it re-projects and
    /// re-emits; on a hit it replays the cached frame with zero projection
    /// work.
    ///
    /// ASSETREF v1 LAW (recorded here; the projection is the enforcer):
    /// scene `Asset` elements render as their documented placeholder — a
    /// bounded FRAME outline at the asset's frame (the projection's accent
    /// stroke, `plugin_scene.rs`'s recorded stage-1 treatment), NOT the
    /// asset's bytes. URI loading / thumbnails / a host asset store do not
    /// exist yet; when they land they slot in behind the same prim stream.
    /// The demo's asset element shows the frame plus plain swatch rects
    /// (which render fully).
    ///
    /// The scene `Rc` snapshot: cloned from the store's leg ONCE per view
    /// pass (bounded by the scene budget, view rate) so the `'static`
    /// program borrows nothing from the view. The store remains the sole
    /// truth owner — the snapshot drops with the frame, and the next pass
    /// re-reads the store (a patch the store applied mid-frame is picked up
    /// at the NEXT composition, ≤ one frame stale, and re-keys the cache by
    /// its revision bump).
    ///
    /// POINTER FENCE, SCN-032 STATUS: LIFTED FOR THIS PROGRAM ONLY. The
    /// program now consumes press/move/release over the canvas
    /// ([`SceneRegionProgram::update`]) and drives the SCN-021 drag machine
    /// through the surface's shared [`SceneDragSlot`]. The lift is scoped
    /// EXACTLY here: a region composed over a REGISTERED widget surface
    /// gets an interactive program (the demo's panel: scene leg + widget
    /// leg); a SCENE-ONLY panel composes the same canvas WITHOUT a slot —
    /// its program still consumes nothing (the recorded fence stands
    /// there), and every legacy / widgets-only body never composes a scene
    /// region at all. `DragMeta`-bearing elements are synced into the
    /// machine's draggable set at every composition (the machine's REPLACE
    /// contract — the same def-walk discipline the widget mount syncs its
    /// read models at attach/patch rate).
    #[cfg(not(target_arch = "wasm32"))]
    fn scene_region_element_owned(
        &self,
        panel_id: &str,
        scene: &PanelScene,
    ) -> Element<'static, WidgetMountMsg> {
        // The drag slot: only where a registered widget surface exists (the
        // surface id the pointer routes and the settle event name).
        let drag = self
            .panel_surface_id_owned(panel_id)
            .map(|surface_id| {
                let slot = self.scene_drag_slot_owned(surface_id);
                // The synced draggable set: every element in the CURRENT
                // scene leg that carries `DragMeta` (the machine refuses
                // everything not synced — the dispatcher's synced-read-model
                // discipline). REPLACE per composition: idempotent, and an
                // in-flight drag's own origin was validated at arm time.
                slot.borrow_mut()
                    .session
                    .sync_draggables(scene_draggable_ids(&scene.elements));
                slot
            });
        canvas(SceneRegionProgram {
            scene: Rc::new(scene.clone()),
            cache: self.scene_cache_slot_owned(panel_id),
            drag,
        })
        .width(Fill)
        .height(Length::Fixed(SCENE_REGION_V1_HEIGHT))
        .into()
    }
}

/// (SCN-031) Which regions the mounted body composes — the composition
/// pins' vocabulary (see [`MountRegistry::body_shape_owned`]). The legacy
/// path is the `None` case, unchanged.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MountBodyShape {
    /// Only the widget stack (the pre-seam body, byte-identical).
    WidgetsOnly,
    /// Only the scene region (the demo's `scn_panel` shape): the region IS
    /// the body.
    SceneOnly,
    /// The fixed split: the scene region above, the widget stack below.
    SceneAndWidgets,
}

/// (SCN-031) The scene region's FIXED v1 height, in points — the recorded
/// fixed split (see [`SurfaceMountHandle::body`]): the panel's scroll area
/// lays its content out with an UNBOUNDED main axis, so a `Fill`-height
/// region would resolve to infinite height; fixed is the simple, correct
/// v1 answer. Tall scenes pan with the panel's own scrollbar; a
/// viewport-integrated region (or a SceneRef widget variant) is the later
/// refinement, not this package.
#[cfg(not(target_arch = "wasm32"))]
const SCENE_REGION_V1_HEIGHT: f32 = 240.0;

/// (SCN-031) The scene region's canvas program: a thin adapter over the
/// SCN-011a/b cache — every frame it hands the renderer, the store-read
/// scene snapshot, the LIVE theme palette (a theme switch therefore moves
/// the layer key's palette fingerprint and repaints) and the frame bounds
/// to [`SceneProjectionCache::draw`], which re-projects only on a key miss
/// and replays the cached geometry otherwise. The program owns nothing
/// scene-shaped beyond the view-pass `Rc` snapshot (see
/// [`MountRegistry::scene_region_element_owned`]); the retained prims live
/// registry-side in the shared cache slot.
///
/// SCN-031b FIT-AND-CENTER LAW (enforced inside the cache's build path —
/// the program only ever sees the finished `Geometry` blob): every
/// committed build fits the projected stream's bounding box into these
/// bounds — translate + uniform scale, centered, [`SCENE_FIT_MARGIN`]
/// (the cache module's constant) inset, scaling UP small scenes too (the
/// documented viewer law). That is what makes an origin-centered graph
/// (negative scene coordinates) visible in a top-left-origin region — the
/// live SCN-031 defect. The transform is a pure function of the existing
/// layer-key inputs (bbox = f(revision), bounds, const margin), so it adds
/// no key field and no rebuild: replay stays replay. SCN-032 will invert
/// it for pointer→scene mapping ([`crate::ui::window::plugin_scene_cache::SceneFitTransform::invert`]).
#[cfg(not(target_arch = "wasm32"))]
struct SceneRegionProgram {
    /// The store's scene leg, snapshotted at composition time.
    scene: Rc<PanelScene>,
    /// The registry-owned retained projection (the panel's slot).
    cache: Rc<RefCell<SceneProjectionCache>>,
    /// (SCN-032) The surface's shared scene-drag slot (`None` = the
    /// scene-only shape — the pointer fence stands: the program consumes
    /// nothing). The SAME `Rc` the registry's Escape/teardown/release
    /// routes reach: the canvas drives the machine, the registry completes
    /// it — one machine, two hands.
    drag: Option<Rc<RefCell<SceneDragSlot>>>,
}

/// (SCN-032) The settle command id the mount binds (its v1 spelling — the
/// demo plugin resolves this exact string; the host's own constant is the
/// authority here, the demo's mirrors it).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SCENE_DRAG_SETTLE_COMMAND: &str = "drag_settle";

/// (SCN-032) Hit-test apertures, mirroring the projection's painted extents
/// (`plugin_scene.rs`: the port disc radius, the marker shapes). Kept as
/// LOCAL constants — the hit-test is this module's approximation contract,
/// not a second projection.
/// Port discs (and the marker Dot) paint at radius 4.
#[cfg(not(target_arch = "wasm32"))]
const SCENE_HIT_PORT_RADIUS: f32 = 4.0;
/// The marker Arrow's tip/base/half-width (the projection's 8-unit shape).
#[cfg(not(target_arch = "wasm32"))]
const SCENE_HIT_ARROW_TIP: f32 = 8.0;
#[cfg(not(target_arch = "wasm32"))]
const SCENE_HIT_ARROW_BASE: f32 = 4.0;
#[cfg(not(target_arch = "wasm32"))]
const SCENE_HIT_ARROW_HALF_WIDTH: f32 = 5.0;
/// The marker Diamond's half diagonal.
#[cfg(not(target_arch = "wasm32"))]
const SCENE_HIT_DIAMOND_HALF_DIAGONAL: f32 = 8.0;
/// The polygon strokes' hit tolerance beyond half the stroke width (scene
/// units — a thin stroke still catches a click).
#[cfg(not(target_arch = "wasm32"))]
const SCENE_HIT_STROKE_TOLERANCE: f32 = 2.0;

/// (SCN-032) The settle→semantic binding (INT-4, the ONE event conversion):
/// a settle FACT becomes EXACTLY ONE `WidgetEvent::CommandInvoked(command,
/// [the dragged element's address])` — nothing else on the event (the
/// meta-echo law: `DragMeta.opaque` NEVER rides the wire; the plugin
/// resolves the payload from its own model). The mount-side twin of
/// `app::update::widget_drag::settle_to_event` — that module is private to
/// the app tree, and the binding is a pure fact→event mapping with no
/// machine logic in it, so the twin is deliberate and pinned by tests.
#[cfg(not(target_arch = "wasm32"))]
fn settle_to_command_event(settled: DragSettled, command: String) -> WidgetEvent {
    WidgetEvent::CommandInvoked {
        surface_id: settled.surface_id,
        command_id: command,
        target_ids: vec![settled.address],
    }
}

/// (SCN-032) The `DragMeta`-bearing element ids of one scene — the machine's
/// synced draggable set (a def walk, the registration-sync discipline).
#[cfg(not(target_arch = "wasm32"))]
fn scene_draggable_ids(elements: &[SceneElement]) -> Vec<StableId> {
    let mut ids = Vec::new();
    let mut walker: Vec<&SceneElement> = elements.iter().collect();
    while let Some(element) = walker.pop() {
        let drag = match element {
            SceneElement::Group { drag, children, .. } => {
                walker.extend(children.iter());
                drag
            }
            SceneElement::Rect { drag, .. }
            | SceneElement::Path { drag, .. }
            | SceneElement::CubicPath { drag, .. }
            | SceneElement::Text { drag, .. }
            | SceneElement::Asset { drag, .. }
            | SceneElement::Port { drag, .. }
            | SceneElement::Marker { drag, .. }
            | SceneElement::Edge { drag, .. } => drag,
        };
        if drag.is_some() {
            ids.push(element.stable_id());
        }
    }
    ids
}

/// (SCN-032) The element's `DragMeta`, if it carries one (the verbatim arm
/// payload — INT-5: the host returns it to the plugin through the settle
/// FACT, never through the event).
#[cfg(not(target_arch = "wasm32"))]
fn scene_element_drag_meta(element: &SceneElement) -> Option<&DragMeta> {
    match element {
        SceneElement::Group { drag, .. }
        | SceneElement::Rect { drag, .. }
        | SceneElement::Path { drag, .. }
        | SceneElement::CubicPath { drag, .. }
        | SceneElement::Text { drag, .. }
        | SceneElement::Asset { drag, .. }
        | SceneElement::Port { drag, .. }
        | SceneElement::Marker { drag, .. }
        | SceneElement::Edge { drag, .. } => drag.as_ref(),
    }
}

/// (SCN-032) The TOPMOST-BY-PAINT-ORDER element under the SCENE point:
/// `flat` is the document-order pre-order flatten (the SAME one walk the
/// region reads use — paint order is document order, parents before
/// children), so the REVERSE walk hits the last-painted (topmost) element
/// first. The drag-relevant test (the element's `DragMeta` presence) is the
/// caller's — this answers the pure geometric hit. Groups carry no geometry
/// and are never hit targets (v1 approximation, recorded); clip rectangles
/// do not filter the hit walk (v1 approximation, recorded — a clipped-out
/// child still answers; the demo scene carries no clips).
#[cfg(not(target_arch = "wasm32"))]
fn scene_hit_element<'a>(
    flat: &[&'a SceneElement],
    point: Point,
) -> Option<&'a SceneElement> {
    flat.iter()
        .rev()
        .copied()
        .find(|element| scene_element_contains(element, point))
}

/// (SCN-032) The containment test per element kind, in SCENE space (the
/// point arrived through the render's own transform — the custody law).
/// Approximations recorded at each arm: text is its baseline-anchored box
/// (the projection's own bbox contribution is the anchor; the box is the
/// deterministic estimate), cubic chains hit-test as their control
/// polygons, fills/closed shapes hit by area, strokes by half-width +
/// [`SCENE_HIT_STROKE_TOLERANCE`].
#[cfg(not(target_arch = "wasm32"))]
fn scene_element_contains(element: &SceneElement, point: Point) -> bool {
    match element {
        // A group paints no geometry: never a hit target.
        SceneElement::Group { .. } => false,
        SceneElement::Rect { pos, size, .. } | SceneElement::Asset { frame: Rect2D { pos, size }, .. } => {
            let (x, y) = (pos.0[0] as f32, pos.0[1] as f32);
            let (w, h) = (size.0[0] as f32, size.0[1] as f32);
            point.x >= x && point.x <= x + w && point.y >= y && point.y <= y + h
        }
        SceneElement::Path { points, closed, fill, stroke, .. } => {
            let polygon: Vec<Point> = points.iter().map(scene_element_point).collect();
            scene_polygon_contains(&polygon, *closed, *fill, stroke.as_ref(), point)
        }
        SceneElement::CubicPath { segments, closed, fill, stroke, .. } => {
            // The control polygon is the deterministic hit approximation of
            // the host-side flattening (the paint path samples the curve;
            // the hit path walks its anchors).
            let mut polygon: Vec<Point> = Vec::new();
            if let Some(first) = segments.first() {
                polygon.push(scene_element_point(&first.start));
            }
            for segment in segments {
                polygon.push(scene_element_point(&segment.ctrl_a));
                polygon.push(scene_element_point(&segment.ctrl_b));
                polygon.push(scene_element_point(&segment.end));
            }
            scene_polygon_contains(&polygon, *closed, *fill, stroke.as_ref(), point)
        }
        SceneElement::Text { content, pos, size, .. } => {
            // Baseline-anchored box: height one em ABOVE the baseline, width
            // a deterministic per-char estimate (the anchor is where iced
            // starts painting the run).
            let (x, y) = (pos.0[0] as f32, pos.0[1] as f32);
            let width = content.chars().count().max(1) as f32 * *size * 0.6;
            point.x >= x && point.x <= x + width && point.y >= y - *size && point.y <= y
        }
        SceneElement::Port { at, .. } => {
            distance_px(point, scene_element_point(at)) <= SCENE_HIT_PORT_RADIUS
        }
        SceneElement::Marker { at, kind, .. } => {
            let at = scene_element_point(at);
            match kind {
                MarkerKind::Dot => distance_px(point, at) <= SCENE_HIT_PORT_RADIUS,
                MarkerKind::Arrow => {
                    let dx = point.x - at.x;
                    let dy = point.y - at.y;
                    // The tip/base band along +x, within the triangle's
                    // taper: at the tip zero height, at the base the full
                    // half-width.
                    dx >= -SCENE_HIT_ARROW_BASE
                        && dx <= SCENE_HIT_ARROW_TIP
                        && dy.abs() <= SCENE_HIT_ARROW_HALF_WIDTH
                            * (1.0 - (dx + SCENE_HIT_ARROW_BASE)
                                / (SCENE_HIT_ARROW_TIP + SCENE_HIT_ARROW_BASE))
                }
                MarkerKind::Diamond => {
                    let dx = (point.x - at.x) / SCENE_HIT_DIAMOND_HALF_DIAGONAL;
                    let dy = (point.y - at.y) / SCENE_HIT_DIAMOND_HALF_DIAGONAL;
                    dx.abs() + dy.abs() <= 1.0
                }
            }
        }
        SceneElement::Edge { route, stroke, .. } => {
            let Some(stroke) = stroke else {
                // A stroke-less edge paints nothing (the visibility rule).
                return false;
            };
            let half = stroke.width * 0.5 + SCENE_HIT_STROKE_TOLERANCE;
            let route: Vec<Point> = route.iter().map(scene_element_point).collect();
            scene_polyline_within(&route, half, point)
        }
    }
}

/// The even-odd polygon test (closed/filled shapes) with the stroke
/// fallback for open polylines — the shared Path/CubicPath arm.
#[cfg(not(target_arch = "wasm32"))]
fn scene_polygon_contains(
    polygon: &[Point],
    closed: bool,
    fill: Option<SceneColor>,
    stroke: Option<&ocs_plugin_api::scene::Stroke>,
    point: Point,
) -> bool {
    if polygon.len() < 2 {
        return false;
    }
    if closed || fill.is_some() {
        if scene_point_in_polygon(polygon, point) {
            return true;
        }
    }
    match stroke {
        Some(stroke) => {
            let half = stroke.width * 0.5 + SCENE_HIT_STROKE_TOLERANCE;
            scene_polyline_within(polygon, half, point)
        }
        None => false,
    }
}

/// Even-odd ray cast (the classic crossing count) over the polygon ring.
#[cfg(not(target_arch = "wasm32"))]
fn scene_point_in_polygon(polygon: &[Point], point: Point) -> bool {
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &vertex in polygon {
        let crosses = (vertex.y > point.y) != (previous.y > point.y)
            && point.x
                < (previous.x - vertex.x) * (point.y - vertex.y) / (previous.y - vertex.y)
                    + vertex.x;
        if crosses {
            inside = !inside;
        }
        previous = vertex;
    }
    inside
}

/// `true` when `point` is within `half` of ANY segment of the polyline.
#[cfg(not(target_arch = "wasm32"))]
fn scene_polyline_within(points: &[Point], half: f32, point: Point) -> bool {
    points.windows(2).any(|pair| {
        let (a, b) = (pair[0], pair[1]);
        let (ax, ay) = (a.x, a.y);
        let (bx, by) = (b.x, b.y);
        let (dx, dy) = (bx - ax, by - ay);
        let length_sq = dx * dx + dy * dy;
        // The parameter clamps the projection to the segment (endpoints are
        // their own closest points).
        let t = if length_sq <= f32::EPSILON {
            0.0
        } else {
            (((point.x - ax) * dx + (point.y - ay) * dy) / length_sq).clamp(0.0, 1.0)
        };
        let (cx, cy) = (ax + t * dx, ay + t * dy);
        distance_px(point, Point::new(cx, cy)) <= half
    })
}

/// Scene `Vec2` → iced point (the projection's own mapping, mirrored).
#[cfg(not(target_arch = "wasm32"))]
fn scene_element_point(v: &ocs_plugin_api::scene::Vec2) -> Point {
    Point::new(v.0[0] as f32, v.0[1] as f32)
}

/// Euclidean pixel distance (the drag machine's own helper shape).
#[cfg(not(target_arch = "wasm32"))]
fn distance_px(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(not(target_arch = "wasm32"))]
impl SceneRegionProgram {
    /// THE PRESS: region-local pixel → SCENE point through the RENDER's own
    /// retained transform ([`SceneProjectionCache::last_fit_transform`] —
    /// the custody law: the hit-test maps through what the user actually
    /// sees), topmost-by-paint-order hit, and — only when the hit element
    /// carries `DragMeta` — the machine's Begin (origin facts captured
    /// verbatim; the machine's refusals are the evidence). A press on
    /// nothing / on a non-draggable element routes NOTHING: no begin, no
    /// counter, no capture change (the body wrapper's surface capture
    /// stays exactly the SCN-020 gesture it was).
    ///
    /// SCN-037 (live-verified defect): the pixel MUST be the REGION-LOCAL
    /// read ([`mouse::Cursor::position_in`]) — iced's `position_over`
    /// answers the WINDOW-absolute cursor, and the region's canvas sits at
    /// a nonzero window origin (the docked pane + panel chrome + the
    /// scrollable's content origin), so the absolute read inverted the fit
    /// in the WRONG frame: the mapped scene point landed off the card
    /// (usually on the underlay), the hit missed, and the press routed
    /// nothing — for EVERY input, synthetic or manual, exactly as the live
    /// host showed. The headless pins missed it because they passed bounds
    /// at the window origin, where absolute == local; the offset-bounds
    /// pins below pin the fix.
    fn scene_press(&self, slot: &RefCell<SceneDragSlot>, local: Point) {
        // The custody read: the transform the render committed (no decision,
        // no projection — a pure accessor read).
        let transform = self.cache.borrow().last_fit_transform();
        let scene_point = transform.invert(local);
        let flat = scene_region_flatten(&self.scene.elements, None);
        let Some(hit) = scene_hit_element(&flat, scene_point) else {
            return;
        };
        let Some(meta) = scene_element_drag_meta(hit).cloned() else {
            // A geometry hit WITHOUT DragMeta: the recorded boundary — the
            // press routes nothing (scene-element selection is NOT this
            // package).
            return;
        };
        let mut slot = slot.borrow_mut();
        slot.next_tick += 1;
        let surface_id = slot.session.surface_id();
        let tick = slot.next_tick;
        let _ = slot
            .session
            .begin(surface_id, hit.stable_id(), meta, scene_point, tick);
    }

    /// THE MOVE: only a LIVE drag (Armed/InFlight) consumes it —
    /// host-local, NO message, NO plugin round-trip (§14). The point maps
    /// through the render's transform (the same frame the press armed in)
    /// and rides the machine's latest-wins park; the delivered move's
    /// preview emits ONCE per delivery under the machine's ≤1-in-flight
    /// gate (the synchronous host-local release — the DragRouter
    /// composition contract). The pixel half of the previews rides the
    /// later transient-layer join; the SPEC emission + counters are the
    /// machine's own evidence. The pointer OUTSIDE the region while live is
    /// the point-out cancel (`PointerOut` — the lossless no-event exit).
    fn scene_move(&self, slot: &RefCell<SceneDragSlot>, cursor: mouse::Cursor, bounds: iced::Rectangle) {
        let live = matches!(
            slot.borrow().session.phase(),
            DragPhase::Armed | DragPhase::InFlight
        );
        if !live {
            return;
        }
        // The REGION-LOCAL read (SCN-037, see scene_press): position_over
        // answers the window-absolute point and would map the move in the
        // wrong frame; the same read also gates the point-out cancel (None
        // outside the region, for both).
        let Some(local) = cursor.position_in(bounds) else {
            let _ = slot.borrow_mut().session.cancel(DragCancelReason::PointerOut);
            return;
        };
        let transform = self.cache.borrow().last_fit_transform();
        let scene_point = transform.invert(local);
        let mut slot = slot.borrow_mut();
        match slot.session.move_drag(scene_point) {
            MoveDisposition::Crossed | MoveDisposition::Parked { .. } => {
                if let Some(delivered) = slot.session.take_deliverable_move() {
                    let _previews = slot.session.preview_prims(delivered);
                    // The host-local channel is synchronous: the slot
                    // releases this tick (the DragRouter composition law).
                    slot.session.release_delivered_move();
                }
            }
            // Sub-threshold jitter and inactive phases: nothing parks,
            // nothing previews (the machine's own gates).
            MoveDisposition::UnderThreshold { .. } | MoveDisposition::Inactive => {}
        }
    }

    /// THE RELEASE: InFlight settles — the machine drains the newest parked
    /// move FIRST (lossless), the settle FACT converts through
    /// [`settle_to_event`] into EXACTLY ONE
    /// `WidgetEvent::CommandInvoked(SCENE_DRAG_SETTLE_COMMAND, [address])`,
    /// parked in the slot for the release route's forward drain (the body
    /// wrapper's `PointerReleased` lands in
    /// [`MountRegistry::mount_pointer_released_owned`], which forwards it
    /// through the v26 notify router — the drained-events shape). An ARMED
    /// release is a click, not a drag: `cancel(ThresholdNotMet)`, no event.
    /// A release observed outside the region is the point-out completion
    /// (the move that left already cancelled; this is the lossless
    /// belt-and-braces).
    fn scene_release(&self, slot: &RefCell<SceneDragSlot>, cursor: mouse::Cursor, bounds: iced::Rectangle) {
        let live = matches!(
            slot.borrow().session.phase(),
            DragPhase::Armed | DragPhase::InFlight
        );
        if !live {
            return;
        }
        // The REGION-LOCAL read (SCN-037, see scene_press): the settle point
        // maps through the same frame the press armed in; the same read
        // gates the point-out completion (None outside the region, for
        // both).
        let Some(local) = cursor.position_in(bounds) else {
            let _ = slot.borrow_mut().session.cancel(DragCancelReason::PointerOut);
            return;
        };
        let transform = self.cache.borrow().last_fit_transform();
        let scene_point = transform.invert(local);
        let mut slot = slot.borrow_mut();
        match slot.session.phase() {
            DragPhase::InFlight => {
                if let Ok(settled) = slot.session.settle(scene_point) {
                    // The ONE event binding (INT-4): the settle fact
                    // converts to exactly one CommandInvoked — the demo's
                    // authoritative-move trigger. The meta rides the FACT
                    // (dropped here), never the event (INT-5).
                    let event = settle_to_command_event(
                        settled,
                        SCENE_DRAG_SETTLE_COMMAND.to_string(),
                    );
                    slot.pending_events.push(event);
                }
            }
            // A click, not a drag: the machine decides (no settle exists).
            DragPhase::Armed => {
                let _ = slot.session.cancel(DragCancelReason::ThresholdNotMet);
            }
            _ => {}
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl canvas::Program<WidgetMountMsg> for SceneRegionProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        // The one seam call: key math + counted decision + (on a miss) the
        // projection run and emit, all inside the cache (its headless
        // `apply_build` mirror is what the tests counter-pin, the two
        // paths funnel through the same decision).
        let geometry = self
            .cache
            .borrow_mut()
            .draw(renderer, &self.scene, theme.palette(), bounds);
        vec![geometry]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<WidgetMountMsg>> {
        // THE POINTER FENCE, lifted for THIS program only (the recorded
        // SCN-031 boundary closes here): the region's canvas consumes
        // press/move/release over the fitted scene and drives the SCN-021
        // drag machine through the shared slot. Everything else is
        // unchanged — no message is published, no event is captured (the
        // body wrapper's surface capture / coalescing keep running exactly
        // as they were), and a program WITHOUT a slot (the scene-only
        // shape) consumes nothing at all.
        let Some(drag) = self.drag.as_ref() else {
            return None;
        };
        if let iced::Event::Mouse(mouse_event) = event {
            match mouse_event {
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    // THE REGION-LOCAL read (SCN-037): iced's position_over
                    // answers the WINDOW-absolute cursor; the region's
                    // canvas sits at a nonzero window origin (pane + chrome
                    // + scroll content), so the absolute read inverted the
                    // fit in the wrong frame and EVERY press missed the
                    // card — the live defect. position_in is the bounds-
                    // relative point the hit-test's custody law expects.
                    if let Some(local) = cursor.position_in(bounds) {
                        self.scene_press(drag, local);
                    }
                }
                mouse::Event::CursorMoved { .. } => {
                    self.scene_move(drag, cursor, bounds);
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    self.scene_release(drag, cursor, bounds);
                }
                _ => {}
            }
        }
        None
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
/// [`WidgetEventSink`]: emitted
/// [`WidgetEvent`]s gather in a shared `Vec` and the bridge drains them
/// (`take`). The wire era HAS landed (the API-24 scene/widget crossing,
/// live since SCN-025): the drain call IS the forwarding call now —
/// drained batches forward through the v26 notify router
/// (`broadcast_panel_scene_events`, the exact seam this buffer's shape
/// was designed for; no type change was needed) — and the
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
    /// Re-registers that REPLACED the registry entry (the surface shape
    /// moved) — the SAME-SURFACE EVOLUTION re-registrations included
    /// (SCN-033: the machines themselves are CARRIED through those;
    /// [`Self::torn_down`] is the witness of whether a release ran).
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
    /// Widget events FORWARDed through the v26 notify router
    /// (`broadcast_panel_scene_events`) from the bridge arms' sink drains —
    /// one tick per forwarded EVENT row. Exactly-once discipline: the drain
    /// (`CollectingSink::take`) empties the buffer, so a re-drain can never
    /// re-forward; this probe is the count a double-forward defect breaks.
    pub mount_event_forwards: u64,
    /// Store DEMANDS forwarded through the v26 demand router
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

/// THE machine-release path: every machine disposal flows through here
/// (teardown AND the teardown-class replacement — a same-surface-id
/// evolution REPLACES the registry entry but KEEPS the machines, SCN-033).
/// The state's `pending_children` drop (§16
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
    /// - A moved surface whose surface_id is UNCHANGED → SAME-SURFACE
    ///   EVOLUTION (SCN-033): the incoming handle takes the slot but
    ///   ADOPTS the running machines (the previous `state` + `dispatcher`
    ///   `Rc`s overwrite its fresh `from_surface` ones) and the state's
    ///   own `sync` reconciles against the new surface — pruning what the
    ///   patch removed, preserving what it didn't. Within one panel id the
    ///   owner law already blocks cross-plugin publishers, so a same-panel
    ///   replace is always that surface's next revision; the SWG-002
    ///   contract ("selection/focus/expansion survive patches that don't
    ///   touch their keys") therefore finally holds in the WIRE path (the
    ///   live-firetest finding: every applied patch rebuilt fresh
    ///   machines — the demo's expanded lazy tree branch collapsed back
    ///   and tree/list selection died with it; tables were immune only
    ///   because SCN-028 deliberately put their slots registry-side). The
    ///   armed-prompt desc stays armed (same surface, same interaction).
    /// - A moved surface whose surface_id ALSO moved → the teardown-class
    ///   REPLACE: the outgoing machines release through [`release_mount`]
    ///   (pending children, armed prompts, in-flight marks — the §16
    ///   contract), the armed desc dies with its surface, and the new
    ///   handle takes the slot with its own fresh machines (a genuinely
    ///   new logical surface starts clean).
    ///
    /// Idempotent per panel id by construction (at most one entry per id;
    /// re-registers move a counter, never duplicate state).
    ///
    /// THE test/bench entry is [`register_test_surface`]; the app-owned
    /// registration path (SCN-016) calls
    /// [`Self::register_surface_owned`] over the app's own registry — the
    /// production registration CALLS HAVE LANDED (SCN-025's wire attach:
    /// `app/plugin_host.rs` drives them when a panel publishes).
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
            // Ownership first: the previous handle leaves the map OWNED,
            // so the machine `Rc`s travel with it — every branch below
            // works on owned values and no registry borrow crosses a
            // machine borrow. (The evolution arm moves those `Rc`s INTO
            // the incoming handle; `release_mount` never runs on them —
            // that is the entire point of the carry.)
            let previous = self.by_panel.remove(panel_id);
            match previous {
                // SAME-SURFACE EVOLUTION (SCN-033 — the live-firetest
                // fixup): the incoming surface_id equals the registered
                // one, i.e. the wire era's re-register is the PATCH
                // EVOLUTION of the same logical surface. Carry the
                // machines: the previous `state` + `dispatcher` `Rc`s
                // overwrite the incoming handle's fresh (unused) ones,
                // then the machine's own `sync` reconciles against the
                // new surface — its documented contract prunes what the
                // patch removed and preserves selection/focus/expansion
                // it didn't touch. No `release_mount` (the carried
                // machines' pending children, armed marks and read model
                // live on), no `prompts.remove` (same surface, same
                // interaction — the armed desc stays armed), no
                // `torn_down`. The dispatcher/table sync block below
                // re-captures changed defs on the carried machines.
                //
                // (SCN-031) The scene-projection cache slot deliberately
                // SURVIVES the carry (no drop in this arm): the retained
                // prims stay valid — same panel, same scene leg, and the
                // cache re-keys against the live store scene at draw time.
                Some(previous) if previous.surface_id() == handle.surface_id() => {
                    let mut carried = handle;
                    carried.state = Rc::clone(&previous.state);
                    carried.dispatcher = Rc::clone(&previous.dispatcher);
                    carried.state.borrow_mut().sync(&carried.surface);
                    self.by_panel.insert(panel_id.to_string(), carried);
                    self.counters.registers_replaced += 1;
                }
                // TEARDOWN-CLASS REPLACE: a different surface_id is a
                // genuinely new logical surface — it starts clean, exactly
                // as every replacement always has (§16: the outgoing
                // machines release through the ONE dispose path; the
                // armed desc is registry-side of the same interaction and
                // dies with its surface). (SCN-031) The scene-projection
                // cache slot drops with the replace — the §16 release
                // rule, scene edition; the next region composition starts
                // a fresh cache.
                Some(previous) => {
                    release_mount(&previous);
                    self.prompts.remove(&previous.surface_id());
                    self.drop_scene_cache_slot(panel_id);
                    self.counters.registers_replaced += 1;
                    self.counters.torn_down += 1;
                    self.by_panel.insert(panel_id.to_string(), handle);
                }
                // A FRESH registration: no outgoing machines exist — no
                // replace accounting, the handle just takes the (empty)
                // slot.
                None => {
                    self.by_panel.insert(panel_id.to_string(), handle);
                }
            }
        }
        // (SCN-028) The typed-table machines ride EVERY successful surface
        // swap (the same sync points `WidgetUiState` re-syncs — snapshots
        // and applied patches): missing slots are created, existing slots
        // re-sync against the REGISTERED surface (selection/focus/draft
        // survive patches that do not touch their keys — the machine's
        // documented sync contract). (SCN-030) The command read model rides
        // the same way: the registered handle's dispatcher captures the
        // surface's buttons/menus (`dispatcher_sync_surface_owned`).
        // Attach/patch-rate cost, never keystroke-rate.
        //
        // (SCN-030 safety) PRESERVE-ON-IDENTICAL keeps the ALREADY-synced
        // dispatcher, and the upserts re-write identical views (the sync_*
        // are idempotent by address). (SCN-033) The SAME-SURFACE EVOLUTION
        // arm keeps the CARRIED dispatcher, and the upserts RE-CAPTURE the
        // changed defs against the registered surface (an unchanged def is
        // an idempotent re-write; a changed def re-upserts in place). A
        // def the patch REMOVED keeps its read-model row until a
        // teardown-class replace (the upserts feed, they never prune —
        // RECORDED: the machines' semantics are untouched by the carry).
        // The teardown-class arm REPLACES the whole handle with a FRESH
        // empty dispatcher (release_mount reset the outgoing one), so no
        // stale address survives there; and the §16 teardown still resets
        // everything via `release_mount`.
        if let Some(stored) = self.by_panel.get(panel_id) {
            let (surface_id, surface) = (stored.surface_id(), stored.surface.clone());
            self.table_sync_surface_owned(surface_id, &surface);
            self.dispatcher_sync_surface_owned(surface_id, &surface);
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
        // (SCN-031) The scene-projection cache slot dies with the §16
        // teardown (the retained prims are the surface's mirror — the same
        // lifecycle rule as the input store).
        self.drop_scene_cache_slot(panel_id);
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

    /// (wasm32) Nothing forwards on the web runtime — the probe stays put
    /// (the paired stub of the desktop arm above).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn record_panel_events_forwarded_owned(&mut self, _count: usize) {}

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
///
/// (SCN-028) One surface's typed-table store rides beside them: the
/// per-table [`TableUiState`] machines plus that surface's table keyboard
/// scope. One map entry per surface ⇒ the lifecycle is EXACTLY the
/// surface's (every register/teardown path prunes it — no orphaned
/// machine), and the whole store stays host-local (§3 transient class,
/// never serialized, never wire).
#[derive(Debug, Default)]
struct SurfaceTableState {
    /// The per-table machines, keyed by the registry table id (the
    /// per-table namespace owner; several `TableRef`s may name one table,
    /// the machine is per TABLE id).
    machines: HashMap<StableId, TableUiState>,
    /// The table KEYBOARD scope (`None` = the widget ring holds the keys):
    /// which table holds the roving position, the caller-held cell cursor
    /// (the machine's cell-focus contract keeps the cursor CALLER-held),
    /// and the editing compose buffer (the host-local text the typed
    /// characters compose into before the machine's value gate accepts
    /// them — the buffer advances ONLY on acceptance, mirroring the
    /// machine's atomicity).
    scope: Option<TableKeyScope>,
}

/// The table keyboard scope's host-local slots (SCN-028). The ROW focus
/// itself lives in the machine (`TableUiState::focus`); the cell cursor is
/// CALLER-held by the machine's contract — the caller here is the scope.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TableKeyScope {
    /// The table whose rows/cells hold the roving position.
    table: StableId,
    /// The cell cursor (row key + column) — `None` until the first
    /// Left/Right or pointer press lands it.
    cursor: Option<CellFocus>,
    /// The editing compose buffer (`None` = compose from the machine's
    /// seeded pending). Advances ONLY when the machine accepted the
    /// composed value.
    edit: Option<String>,
}

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
        // (SCN-032) The release route's scene-drag tail runs FIRST: a settled
        // drag's one `CommandInvoked("drag_settle", [address])` drains +
        // forwards here (the route the body wrapper's release AND the
        // window-level release-anywhere catcher both run). A surface without
        // a scene-drag slot — every legacy / widgets-only panel — drains
        // nothing: zero legacy bytes move.
        #[cfg(not(target_arch = "wasm32"))]
        self.forward_scene_drag_events_owned(surface_id);
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
        // (SCN-028) The typed-table machines + scopes are the surfaces'
        // mirror too: a surface leaving the registry RESETS (§16 — the
        // ONLY path that moves the machines' `whole_state_resets` in the
        // mount integration) and drops its table slots — no orphaned
        // machine outlives its surface.
        self.tables.retain(|surface_id, entry| {
            let registered = registered(*surface_id);
            if !registered {
                for machine in entry.machines.values_mut() {
                    machine.reset_all();
                }
                entry.scope = None;
            }
            registered
        });
        if self.input.key_target.is_some_and(|target| !registered(target)) {
            self.input.key_target = None;
        }
        // (SCN-032) The scene-drag slots are the surfaces' mirror too: a
        // surface leaving the registry has its LIVE drag completed as
        // `cancel(Teardown)` (never a settle, never a lost origin — the §16
        // close-tab contract; a pending settle event dies with the surface:
        // the plugin it was bound for is gone) and the slot entry drops.
        #[cfg(not(target_arch = "wasm32"))]
        self.scene_drags.borrow_mut().retain(|surface_id, slot| {
            let registered = registered(*surface_id);
            if !registered {
                let _ = slot.borrow_mut().session.cancel(DragCancelReason::Teardown);
            }
            registered
        });
    }

    // -- SCN-028: the typed-table store (machines + keyboard scope) ---------

    /// Create/sync the per-table machines for one surface swap (the
    /// registration path's sync point; see
    /// [`MountRegistry::register_surface_owned`]). The scope dies with a
    /// table that left the registry — it can hold nothing anymore.
    fn table_sync_surface_owned(&mut self, surface_id: StableId, surface: &WidgetSurface) {
        let entry = self.tables.entry(surface_id).or_default();
        for table_id in surface.tables.keys() {
            let machine = entry
                .machines
                .entry(*table_id)
                .or_insert_with(|| TableUiState::new(surface_id, *table_id));
            machine.sync(surface);
        }
        let scope_dangling = entry
            .scope
            .as_ref()
            .is_some_and(|scope| !surface.tables.contains_key(&scope.table));
        if scope_dangling {
            entry.scope = None;
        }
    }

    /// (SCN-030) Capture one surface's command read model into the
    /// REGISTERED handle's [`WidgetDispatcher`] — the registration path's
    /// sync point for the dispatcher, riding EVERY successful surface swap
    /// (the same contract as [`Self::table_sync_surface_owned`]:
    /// attach/patch-rate, never keystroke-rate). An explicit-stack walk
    /// over the VISIBLE nodes (the SAME walk discipline as
    /// [`first_table_ref_of`]/[`nav_ring`] — §16's invisible-node law skips
    /// a hidden subtree whole) upserting every icon button via
    /// [`WidgetDispatcher::sync_button`] and every menu via
    /// [`WidgetDispatcher::sync_menu`]; the machine's OWN guards (the
    /// effective-flag fold, the [`MAX_MENU_DEPTH`] submenu cap) stay the
    /// only validation — this walk feeds, it never re-implements.
    ///
    /// Registration now feeds the read model (the doc closure for
    /// `plugin_widgets_dispatch.rs`'s "ownership and the read model" note,
    /// which is outside this task's owned files): before this walk the
    /// production dispatcher stayed EMPTY and every live GUI/MCP invocation
    /// refused as an unknown address while the controls still rendered from
    /// the defs.
    fn dispatcher_sync_surface_owned(&mut self, surface_id: StableId, surface: &WidgetSurface) {
        // Clone the handle out FIRST (an `Rc` clone): no registry borrow is
        // held across the dispatcher's.
        let Some(handle) = self.handle_for_surface_owned(surface_id) else {
            return;
        };
        let mut dispatcher = handle.dispatcher.borrow_mut();
        let mut walker: Vec<&WidgetNode> = surface.roots.iter().rev().collect();
        while let Some(node) = walker.pop() {
            if !node_visible(node) {
                continue;
            }
            match node {
                WidgetNode::Group(def) => walker.extend(def.children.iter().rev()),
                WidgetNode::IconButton(def) => dispatcher.sync_button(def),
                WidgetNode::Menu(def) => dispatcher.sync_menu(def),
                // Trees/lists/tables carry no command addresses.
                _ => {}
            }
        }
    }

    /// The scoped machine read (`&mut`): the (surface, table)-addressed
    /// [`TableUiState`] slot.
    fn table_machine_mut_owned(
        &mut self,
        surface_id: StableId,
        table_id: StableId,
    ) -> Option<&mut TableUiState> {
        self.tables
            .get_mut(&surface_id)
            .and_then(|entry| entry.machines.get_mut(&table_id))
    }

    /// The test-side read of one machine slot (lifecycle assertions).
    #[cfg_attr(not(test), allow(dead_code))]
    fn table_state_owned(
        &self,
        surface_id: StableId,
        table_id: StableId,
    ) -> Option<&TableUiState> {
        self.tables
            .get(&surface_id)
            .and_then(|entry| entry.machines.get(&table_id))
    }

    /// The scope's table id read (`None` = the widget ring holds the keys).
    fn table_scope_table_of(&self, surface_id: StableId) -> Option<StableId> {
        self.tables
            .get(&surface_id)
            .and_then(|entry| entry.scope.as_ref())
            .map(|scope| scope.table)
    }

    /// Is the table keyboard scope active on this surface?
    fn table_scope_active(&self, surface_id: StableId) -> bool {
        self.table_scope_table_of(surface_id).is_some()
    }

    /// The caller-held cell cursor read (the two-phase pointer policy's
    /// pre-press comparison).
    fn table_scope_cursor_of(
        &self,
        surface_id: StableId,
        table_id: StableId,
    ) -> Option<CellFocus> {
        self.tables
            .get(&surface_id)
            .and_then(|entry| entry.scope.as_ref())
            .filter(|scope| scope.table == table_id)
            .and_then(|scope| scope.cursor)
    }

    /// The caller-held cell cursor write. A cursor landing CREATES the
    /// surface's table scope when none exists — a pointer press inside a
    /// table moves the keyboard scope there (the desktop input model: the
    /// clicked control owns the keys), exactly like the mount press's key
    /// target. The machine's cell-cursor contract keeps the cursor
    /// CALLER-held; the scope IS the caller.
    fn table_scope_set_cursor_owned(
        &mut self,
        surface_id: StableId,
        table_id: StableId,
        cursor: Option<CellFocus>,
    ) {
        let Some(entry) = self.tables.get_mut(&surface_id) else {
            return;
        };
        if !entry.machines.contains_key(&table_id) {
            return;
        }
        match entry.scope.as_mut() {
            Some(scope) if scope.table == table_id => scope.cursor = cursor,
            _ => {
                entry.scope = Some(TableKeyScope {
                    table: table_id,
                    cursor,
                    edit: None,
                });
            }
        }
    }

    /// Does this surface render a def-present (synced-machine) table? The
    /// Tab law's gate — `false` keeps every pre-existing ring byte.
    fn table_scope_ready(&self, surface_id: StableId, surface: &WidgetSurface) -> bool {
        first_table_ref_of(surface)
            .is_some_and(|table_id| self.table_state_owned(surface_id, table_id).is_some_and(|machine| machine.synced().is_some()))
    }

    /// Dive INTO the table keyboard scope: name the surface's first
    /// def-present table (paint order — the surface walk) as the scope's
    /// owner and land the machine's row focus on the materialized window's
    /// FIRST (forward) or LAST (backward) row. `false` when no def-present
    /// synced table renders or the window is empty — graceful, nothing
    /// half-applies.
    fn table_scope_enter_owned(
        &mut self,
        surface_id: StableId,
        surface: &WidgetSurface,
        forward: bool,
    ) -> bool {
        let Some(table_id) = first_table_ref_of(surface) else {
            return false;
        };
        let Some(entry) = self.tables.get_mut(&surface_id) else {
            return false;
        };
        let Some(machine) = entry.machines.get_mut(&table_id) else {
            return false;
        };
        if machine.synced().is_none() {
            return false;
        }
        let landed = if forward {
            machine.row_focus_next(&DEFAULT_WINDOW_SPEC)
        } else {
            machine.row_focus_prev(&DEFAULT_WINDOW_SPEC)
        };
        if landed.is_none() {
            return false; // an empty window has nothing to hold the scope
        }
        entry.scope = Some(TableKeyScope {
            table: table_id,
            cursor: None,
            edit: None,
        });
        true
    }

    /// LEAVE the table keyboard scope (the machine's row focus stays parked
    /// — re-entering resumes there).
    fn table_scope_leave_owned(&mut self, surface_id: StableId) -> bool {
        self.tables
            .get_mut(&surface_id)
            .and_then(|entry| entry.scope.take())
            .is_some()
    }

    /// Escape's table half: cancel the scoped machine's open draft through
    /// the PURE route's own Escape arm (ONE code path — the exact step the
    /// headless route tests pin, nav-step probe included). `false` when no
    /// scope is active or no draft is open — the caller falls through to
    /// the existing surface-escape law (the armed-prompt dismissal
    /// ordering is unchanged).
    fn table_scope_cancel_draft_owned(&mut self, surface_id: StableId) -> bool {
        let Some(entry) = self.tables.get_mut(&surface_id) else {
            return false;
        };
        let Some(scope) = entry.scope.as_mut() else {
            return false;
        };
        let table_id = scope.table;
        let Some(machine) = entry.machines.get_mut(&table_id) else {
            return false;
        };
        if machine.draft().is_none() {
            return false;
        }
        let column_order: Vec<StableId> = machine
            .synced()
            .map(|synced| synced.column_order.clone())
            .unwrap_or_default();
        let mut cursor = scope.cursor;
        // Disjoint registry fields: the machines ride `tables`, the nav-step
        // probe rides `counters`.
        let counters = &mut self.counters;
        mount_table_key_route_owned(
            counters,
            machine,
            &mut cursor,
            Key::Named(keyboard_key::Named::Escape),
            Modifiers::NONE,
            &DEFAULT_WINDOW_SPEC,
            &column_order,
        );
        scope.cursor = cursor;
        true
    }

    /// Settle one committed cell fact (the typed-table bridge's
    /// post-forward signal — the machine's in-flight mark clears so the
    /// next edit of the same address can arm). Idempotent.
    fn table_settle_owned(
        &mut self,
        surface_id: StableId,
        table_id: StableId,
        key: StableId,
        column: StableId,
    ) -> bool {
        self.table_machine_mut_owned(surface_id, table_id)
            .is_some_and(|machine| machine.settle(key, column))
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
/// EXCLUDED on purpose (each documented at its site): menus — the
/// iced_aw menubar's root is its own native overlay and the mount's ring
/// does not reach inside it; tables — the table rows' roving lives in the
/// typed-table KEYBOARD SCOPE (SCN-028: the scope dives in at the ring's
/// edges — Tab is the enter/leave key; the ring itself still names no
/// table rows, whose keys are the per-table namespace, not widget ids);
/// invisible nodes (the [`build_node`] skip — the ring must not name what
/// is not rendered).
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

    // (SCN-028) THE TABLE KEYBOARD SCOPE: when a table row/cell holds the
    // roving position, the keys route through the table machine — Up/Down
    // row roving within the materialized window, Left/Right the caller-held
    // cell cursor, Enter the edit arm/commit, characters the draft's value
    // keys. (SCN-030, form semantics) Tab/Shift+Tab INSIDE advance/reverse
    // the cell cursor (wrapping rows at the column edge; an open draft
    // commits FIRST); only the table's MATERIALIZED edge leaves to the
    // widget ring. One production body ([`table_scope_key_owned`]); the
    // navigation keys route THROUGH the pure
    // [`mount_table_key_route_owned`].
    if registry.table_scope_active(surface_id) {
        return table_scope_key_owned(registry, surface_id, state, key, modifiers, &ring);
    }

    if ring.is_empty() {
        // (SCN-028) A table-ONLY surface (no widget ring at all) is still
        // reachable: Tab dives straight into the table scope (the same law
        // as the ring-edge dive below). Every other key routes nothing —
        // the pre-existing bytes.
        if matches!(key, Key::Named(keyboard_key::Named::Tab))
            && registry.table_scope_enter_owned(surface_id, surface, !modifiers.shift())
        {
            registry.record_key_nav_step_owned();
        }
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
            // (SCN-028) When the surface renders a def-present table, the
            // ring EDGES dive into the table scope — the documented ring
            // law: the table contributes a TABLE-LEVEL ring entry that
            // DIVES IN, realized as the wrap target of the edge walk (the
            // walk runs WITHOUT the wrap there; Shift+Tab's backward edge
            // dives at the window's last row; Tab INSIDE the scope returns
            // to the ring's top). Surfaces WITHOUT a def-present table
            // keep the pre-existing wrap bytes exactly.
            if registry.table_scope_ready(surface_id, surface) {
                let target = if modifiers.shift() {
                    state.focus_prev(current, &ring_ids, &views)
                } else {
                    state.focus_next(current, &ring_ids, &views)
                };
                match target {
                    Some(row_id) => step_msgs(Some(row_id), false),
                    None => {
                        if registry.table_scope_enter_owned(surface_id, surface, !modifiers.shift())
                        {
                            registry.record_key_nav_step_owned();
                        }
                        Vec::new()
                    }
                }
            } else {
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
    // SCN-028: the table scope's OPEN DRAFT cancels FIRST (the FREE cancel
    // through the pure route's own Escape arm); with no draft the key
    // falls through to the surface-escape law — the armed-prompt dismissal
    // ordering is unchanged.
    if key == Key::Named(keyboard_key::Named::Escape) {
        if registry.table_scope_cancel_draft_owned(surface_id) {
            return Vec::new();
        }
        // (SCN-032) The scene drag's cancel rides the same machine-direct
        // arm BEFORE the armed-prompt dismissal: a live drag is the newest
        // gesture on the surface, and its lossless no-event completion is
        // independent of the prompt machine's (both may run; neither sees a
        // KEY). The slot's existence gates it — a scene-less surface
        // cancels nothing, exactly as before.
        #[cfg(not(target_arch = "wasm32"))]
        registry.scene_drag_cancel_owned(surface_id, DragCancelReason::Escape);
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

/// The surface's FIRST visible [`TableRef`]'s registry table id (paint
/// order — the same explicit-stack walk discipline as [`nav_ring`]). The
/// table scope dive's target pick: a surface with several tables scopes
/// the first rendered one (documented at
/// [`MountRegistry::table_scope_enter_owned`]).
fn first_table_ref_of(surface: &WidgetSurface) -> Option<StableId> {
    let mut walker: Vec<&WidgetNode> = surface.roots.iter().rev().collect();
    while let Some(node) = walker.pop() {
        if !node_visible(node) {
            continue;
        }
        if let WidgetNode::Group(def) = node {
            walker.extend(def.children.iter().rev());
            continue;
        }
        if let WidgetNode::Table(def) = node {
            return Some(def.table_id);
        }
    }
    None
}

/// The step a TABLE key resolved to (the pure keyboard layer's output —
/// SCN-028's production scope route consumes these; the headless route
/// tests pin them).
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
// SCN-028: the first parameter is the registry's COUNTERS (not the whole
// registry) — the production scope route holds the machine borrow while
// ticking the probe (disjoint fields), and the probe was this fn's only
// registry use. Zero behavior; the cfg(test) facade + the app reach-through
// signatures are unchanged. The typed-table production route
// ([`table_scope_key_owned`]) is this fn's caller — the allowance is gone.
pub(crate) fn mount_table_key_route_owned(
    counters: &mut MountCounters,
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
        counters.keyboard_nav_steps += 1;
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
            &mut registry.counters, state, cell_cursor, key, modifiers, spec, column_order,
        )
    })
}

// ---------------------------------------------------------------------------
// SCN-028 — the production TABLE keyboard route (the scope-active half)
// ---------------------------------------------------------------------------

/// The typed-table keyboard route's PRODUCTION body — the scope-active half
/// of [`key_route_owned`] (the app's `handle_mount_key_owned` reaches it
/// through [`bench_handle_mount_key_owned`]; no app file touched).
///
/// Key map (the module docs' scope law, in code):
/// - **Tab / Shift+Tab** (SCN-030 — form semantics) advance/reverse the
///   CELL CURSOR: Right-/Left-equivalent through the SAME pure route body;
///   at the ROW's column edge the cursor wraps one row down/up (the
///   machine's own row roving) landing on the first/last column; at the
///   table's MATERIALIZED edge the scope LEAVES to the widget ring (the
///   ring walk restarts from its top — Shift: its bottom; the machine's
///   row focus stays parked, re-entering resumes). An open draft COMMITS
///   FIRST (exactly one `TableCellCommitted`, the Excel form law;
///   [`table_scope_tab_step_owned`]). (SCN-035) EVERY advance landing
///   ARMS: a writable landing opens the seeded draft immediately — Tab
///   lands the user IN the editor; a non-writable landing refuses quietly
///   (the machine's counter is the probe).
/// - **Up/Down/Left/Right + Escape** route THROUGH the pure
///   [`mount_table_key_route_owned`] (ONE translation body). At the
///   Up/Down WINDOW edges the roving stops; the v1 SCROLL law advances the
///   scroll anchor one row and re-anchors the focus inside the
///   re-materialized window (the machine's window-bound `row_focus_set`),
///   and a forward landing on the PUBLISHED TAIL emits
///   [`WidgetMountMsg::TableTailReached`] — the ROUTE (not this fn) runs
///   the machine's `page_request_if_needed` exactly once (dedup rides the
///   machine).
/// - **Enter** on a focused writable cell ARMS the edit (the machine's
///   gate: a non-writable kind refuses QUIETLY — its counter the probe;
///   the cursor's column, else the row's first column); **Enter with a
///   draft open** COMMITS: exactly ONE `WidgetOp::TableCell` fact leaves as
///   [`WidgetMountMsg::TableCellCommitted`] (the bridge forwards the
///   `TableCellChanged` notify and settles the address). **Space** on a
///   Bool draft toggles it.
/// - **Characters/Backspace with a draft open** compose the editing
///   buffer and offer each candidate to the machine's `draft_cell_value`
///   (Text: append/pop; Number: the buffer must parse — a `-` keyed FIRST
///   starts a negative, anywhere else it is ignored; Bool drafts take
///   only Space). The buffer advances ONLY on acceptance — the machine's
///   gates (kind class + delegated crate validation) are the authority and
///   their refusals leave the pending EXACTLY as-was. Without a draft the
///   typing keys route nothing (v1 edits live in the draft only).
fn table_scope_key_owned(
    registry: &mut MountRegistry,
    surface_id: StableId,
    state: &mut WidgetUiState,
    key: Key,
    modifiers: Modifiers,
    ring: &[(StableId, StableId)],
) -> Vec<WidgetMountMsg> {
    // Chords are the app's accelerator space here too (the pure route
    // guards its own keys; this covers the edit keys).
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return Vec::new();
    }
    // (SCN-030, form semantics) Tab / Shift+Tab INSIDE the scope advance /
    // reverse the cell cursor (Right-/Left-equivalent — the Excel form
    // law), wrapping rows at the column edge; at the table's MATERIALIZED
    // edge they LEAVE to the widget ring (the pre-existing leave path).
    // An open draft COMMITS FIRST — exactly one `TableCellCommitted` (see
    // the helper); Escape stays the FREE cancel. (SCN-035) the landing
    // ARMS — Tab lands the user IN the editor (see the helper's step 5).
    if key == Key::Named(keyboard_key::Named::Tab) {
        return table_scope_tab_step_owned(registry, surface_id, state, modifiers, ring);
    }

    let Some(scope_table) = registry.table_scope_table_of(surface_id) else {
        return Vec::new();
    };
    let Some(entry) = registry.tables.get_mut(&surface_id) else {
        return Vec::new();
    };
    let Some(scope) = entry.scope.as_mut() else {
        return Vec::new();
    };
    let Some(machine) = entry.machines.get_mut(&scope_table) else {
        return Vec::new();
    };
    let column_order: Vec<StableId> = machine
        .synced()
        .map(|synced| synced.column_order.clone())
        .unwrap_or_default();

    // Disjoint registry fields: the machines/scope ride `tables`, the
    // nav-step probe rides `counters`.
    let counters = &mut registry.counters;

    match &key {
        Key::Named(keyboard_key::Named::ArrowDown)
        | Key::Named(keyboard_key::Named::ArrowUp)
        | Key::Named(keyboard_key::Named::ArrowRight)
        | Key::Named(keyboard_key::Named::ArrowLeft)
        | Key::Named(keyboard_key::Named::Escape) => {
            let mut cursor = scope.cursor;
            let step = mount_table_key_route_owned(
                counters,
                machine,
                &mut cursor,
                key.clone(),
                modifiers,
                &DEFAULT_WINDOW_SPEC,
                &column_order,
            );
            scope.cursor = cursor;
            match step {
                Some(MountTableKeyStep::RowFocus(landed)) => {
                    // Down landing on the PUBLISHED TAIL of a windowing
                    // table asks for the next page — as the tail-reach
                    // MESSAGE (the bridge route runs the machine's ask).
                    if key == Key::Named(keyboard_key::Named::ArrowDown)
                        && machine_at_published_tail(machine, landed)
                    {
                        vec![WidgetMountMsg::TableTailReached {
                            surface_id,
                            table_id: scope_table,
                        }]
                    } else {
                        Vec::new()
                    }
                }
                Some(MountTableKeyStep::CellFocus(_)) | Some(MountTableKeyStep::DraftCancelled) => {
                    Vec::new()
                }
                None => match &key {
                    // The window edges: the v1 scroll law (see the fn docs).
                    Key::Named(keyboard_key::Named::ArrowDown) => table_scroll_step_owned(
                        counters,
                        machine,
                        scope,
                        surface_id,
                        scope_table,
                        true,
                    ),
                    Key::Named(keyboard_key::Named::ArrowUp) => table_scroll_step_owned(
                        counters,
                        machine,
                        scope,
                        surface_id,
                        scope_table,
                        false,
                    ),
                    // Escape with no draft: already fallen through at the
                    // route head (the armed-prompt law); Left/Right at a
                    // column edge stay put (the documented edge law).
                    _ => Vec::new(),
                },
            }
        }
        Key::Named(keyboard_key::Named::Enter) | Key::Named(keyboard_key::Named::Space) => {
            let draft_info = machine
                .draft()
                .map(|draft| (draft.key, draft.column, draft.pending.clone()));
            match draft_info {
                Some((_, draft_column, pending)) => {
                    let kind = machine
                        .synced()
                        .and_then(|synced| synced.columns.get(&draft_column))
                        .copied();
                    // Space on a Bool draft: the toggle (the Bool value key).
                    if key == Key::Named(keyboard_key::Named::Space) {
                        if kind == Some(CellKind::Bool) {
                            if let WidgetCellValue::Bool(value) = pending {
                                let _ = machine.draft_cell_value(WidgetCellValue::Bool(!value));
                            }
                        }
                        return Vec::new();
                    }
                    // Enter COMMITS: exactly ONE fact, exactly once.
                    match machine.commit_cell_draft() {
                        Ok(Some(WidgetOp::TableCell {
                            key: op_key,
                            column: op_column,
                            value,
                            ..
                        })) => {
                            scope.edit = None;
                            vec![WidgetMountMsg::TableCellCommitted {
                                surface_id,
                                table_id: scope_table,
                                key: op_key,
                                column: op_column,
                                value,
                            }]
                        }
                        // `NoDraft` cannot fire (the draft was just read);
                        // the reserved None shape is future elision.
                        _ => Vec::new(),
                    }
                }
                None => {
                    // Space activates nothing outside a draft (Enter is the
                    // table's activation key).
                    if key != Key::Named(keyboard_key::Named::Enter) {
                        return Vec::new();
                    }
                    let Some(focused) = machine.focus() else {
                        return Vec::new();
                    };
                    let column = match scope.cursor {
                        Some(cell) if cell.key == focused => Some(cell.column),
                        _ => column_order.first().copied(),
                    };
                    let Some(column) = column else {
                        return Vec::new();
                    };
                    // The machine's gate is the authority: a non-writable
                    // kind (or an in-flight address) refuses QUIETLY — its
                    // counter is the probe; the view stays as-was.
                    if machine.begin_cell_edit(focused, column).is_ok() {
                        scope.edit = None;
                    }
                    Vec::new()
                }
            }
        }
        Key::Character(typed) => {
            table_scope_edit_keys_owned(machine, scope, typed, false)
        }
        Key::Named(keyboard_key::Named::Backspace) => {
            table_scope_edit_keys_owned(machine, scope, "", true)
        }
        _ => Vec::new(),
    }
}

/// The scope's Tab step (SCN-030 — the form-semantics law the
/// [`table_scope_key_owned`] Tab arm rides). ONE machine phase (the borrow
/// ends before any leave/ring walk):
///
/// 1. an open draft COMMITS FIRST — exactly one `WidgetOp::TableCell` fact
///    leaves as [`WidgetMountMsg::TableCellCommitted`] (the Enter commit's
///    own path; the Excel form law. Escape stays the FREE cancel);
/// 2. the cell cursor advances (Tab) / reverses (Shift+Tab) through the
///    SAME pure route body — Tab rides as Right, Shift+Tab as Left, so the
///    machine's own methods do every step;
/// 3. at the ROW's column edge the cursor WRAPS: one row down/up (the
///    machine's `row_focus_next`/`prev`), landing on its first (Tab) /
///    last (Shift+Tab) column — the cursor resets and the cell step
///    re-runs from `None`;
/// 4. at the table's MATERIALIZED edge the scope LEAVES to the widget ring
///    (the pre-existing leave path: the scope clears, the ring walk
///    restarts from its top — Shift+Tab from its bottom — and the machine's
///    row focus stays parked for the resume);
/// 5. (SCN-035) every ADVANCE landing ARMS: when the landed cell is
///    writable, the edit begins immediately (the machine's
///    `begin_cell_edit` — the seeded draft; the user types directly —
///    standard form semantics: Tab lands you IN the editor). The machine's
///    gate is the authority: a non-writable landing (badge/icon/…) refuses
///    QUIETLY — its `invalid_cell_refusals` counter is the probe, the
///    cursor has still moved, no draft. Arming never happens on the LEAVE
///    (step 4) and never on the DIVE (the scope's entry lands no cursor).
fn table_scope_tab_step_owned(
    registry: &mut MountRegistry,
    surface_id: StableId,
    state: &mut WidgetUiState,
    modifiers: Modifiers,
    ring: &[(StableId, StableId)],
) -> Vec<WidgetMountMsg> {
    let Some(scope_table) = registry.table_scope_table_of(surface_id) else {
        return Vec::new();
    };
    let mut msgs: Vec<WidgetMountMsg> = Vec::new();
    let mut leave = false;
    {
        let Some(entry) = registry.tables.get_mut(&surface_id) else {
            return Vec::new();
        };
        let Some(scope) = entry.scope.as_mut() else {
            return Vec::new();
        };
        let Some(machine) = entry.machines.get_mut(&scope_table) else {
            return Vec::new();
        };
        let column_order: Vec<StableId> = machine
            .synced()
            .map(|synced| synced.column_order.clone())
            .unwrap_or_default();
        // 1. The open draft COMMITS first (exactly ONE fact, exactly once).
        if machine.draft().is_some() {
            match machine.commit_cell_draft() {
                Ok(Some(WidgetOp::TableCell {
                    key: op_key,
                    column: op_column,
                    value,
                    ..
                })) => {
                    scope.edit = None;
                    msgs.push(WidgetMountMsg::TableCellCommitted {
                        surface_id,
                        table_id: scope_table,
                        key: op_key,
                        column: op_column,
                        value,
                    });
                }
                // `NoDraft` cannot fire (the draft was just read); the
                // reserved None shape is future elision.
                _ => {}
            }
        }
        // 2/3. Advance or reverse: the column step, else the row wrap
        //      landing on the wrapped row's first/last column.
        let direction = if modifiers.shift() {
            Key::Named(keyboard_key::Named::ArrowLeft)
        } else {
            Key::Named(keyboard_key::Named::ArrowRight)
        };
        // Disjoint registry fields: the machines/scope ride `tables`, the
        // nav-step probe rides `counters`.
        let counters = &mut registry.counters;
        let column_step = mount_table_key_route_owned(
            counters,
            machine,
            &mut scope.cursor,
            direction.clone(),
            Modifiers::NONE,
            &DEFAULT_WINDOW_SPEC,
            &column_order,
        );
        if column_step.is_none() {
            let wrapped = if modifiers.shift() {
                machine.row_focus_prev(&DEFAULT_WINDOW_SPEC)
            } else {
                machine.row_focus_next(&DEFAULT_WINDOW_SPEC)
            };
            if wrapped.is_some() {
                // The wrapped row's FIRST (Tab) / LAST (Shift+Tab) column.
                scope.cursor = None;
                let _ = mount_table_key_route_owned(
                    counters,
                    machine,
                    &mut scope.cursor,
                    direction,
                    Modifiers::NONE,
                    &DEFAULT_WINDOW_SPEC,
                    &column_order,
                );
            } else {
                // 4. The MATERIALIZED edge — the leave runs below, after
                //    the machine borrow ends.
                leave = true;
            }
        }
        // 5. (SCN-035) The landing ARMS: Tab/Shift+Tab land the user IN the
        //    editor — the machine's `begin_cell_edit` on the landed cell
        //    (the seeded draft; the scope's compose buffer resets so the
        //    typing composes from the seed, the Enter arm's own discipline).
        //    The machine's gate is the authority: a non-writable landing
        //    refuses QUIETLY (its counter is the probe — the cursor moved,
        //    no draft); an in-flight address refuses the same way; a
        //    commit that somehow left its draft open (the reserved shape —
        //    unreachable post-SCN-034, the rollback law) refuses as
        //    `DraftInFlight` and the draft stays. NO arm on a leave (`leave`
        //    is the scope's exit — there is no landing) and the landing is
        //    only real when the cursor names the machine's focused row.
        if !leave {
            if let Some(cell) = scope.cursor {
                if machine.focus() == Some(cell.key)
                    && machine.begin_cell_edit(cell.key, cell.column).is_ok()
                {
                    scope.edit = None;
                }
            }
        }
    }
    if leave {
        registry.table_scope_leave_owned(surface_id);
        registry.record_key_nav_step_owned();
        let views = state
            .synced()
            .map(|synced| synced.views.clone())
            .unwrap_or_default();
        let ring_ids: Vec<StableId> = ring.iter().map(|(id, _)| *id).collect();
        let target = if modifiers.shift() {
            state.focus_prev(None, &ring_ids, &views)
        } else {
            state.focus_next(None, &ring_ids, &views)
        };
        if let Some(row_id) = target {
            let widget_id = owner_of(ring, Some(row_id)).unwrap_or(StableId(0));
            msgs.push(WidgetMountMsg::FocusMoved {
                surface_id,
                widget_id,
                to: Some(row_id),
            });
        }
    }
    msgs
}

/// The draft's VALUE keys (characters / Backspace): compose the candidate
/// from the scope buffer (or the machine's seeded pending) and offer it to
/// the machine's `draft_cell_value` — the buffer advances ONLY on
/// acceptance (the machine's gates are the authority; a refusal leaves the
/// pending and the buffer EXACTLY as-was). Without a draft, or on a Bool
/// draft (Space toggles), the keys route nothing.
///
/// The minus law (the SCN-028 follow-up): on a Number draft, a `-` keyed as
/// the FIRST keystroke of the edit — before any composed content exists —
/// PREPENDS to the seeded display, starting a negative the append-only
/// compose could never spell (the seed always carries the cell's current
/// digits, so plain appending could never put a `-` in the lead). Anywhere
/// else the `-` is ignored BEFORE composing: after the edit began (the
/// digits-then-minus shape), on a seed that already leads with `-` (a
/// double sign), and on Text drafts (there it is an ordinary character and
/// appends). The machine's parse gate stays the authority either way.
fn table_scope_edit_keys_owned(
    machine: &mut TableUiState,
    scope: &mut TableKeyScope,
    typed: &str,
    backspace: bool,
) -> Vec<WidgetMountMsg> {
    let Some((_, draft_column, _)) = machine
        .draft()
        .map(|draft| (draft.key, draft.column, draft.pending.clone()))
    else {
        return Vec::new();
    };
    let Some(kind) = machine
        .synced()
        .and_then(|synced| synced.columns.get(&draft_column))
        .copied()
    else {
        return Vec::new();
    };
    // One composed edit: `Some(ch)` appends the character; `None` is the
    // Backspace pop.
    let edit_once = |machine: &mut TableUiState, scope: &mut TableKeyScope, ch: Option<char>| {
        // `seeded` = nothing composed yet (the buffer IS the machine's
        // pending display) — the only window where the minus law opens.
        let seeded = scope.edit.is_none();
        let seed = scope
            .edit
            .clone()
            .unwrap_or_else(|| pending_display(&machine.draft().map(|draft| draft.pending.clone()).unwrap_or(WidgetCellValue::Bool(false))));
        let mut candidate = seed;
        match ch {
            // The minus law (see the fn docs): a '-' keyed as the FIRST
            // keystroke of a Number edit prepends the sign; anywhere else
            // — after composing began, or on a seed that already leads
            // with '-' (a double sign) — it is ignored BEFORE composing,
            // so the buffer and the machine's counters stay exactly as-was.
            // Text drafts take '-' as an ordinary character (the arm below).
            Some('-') if kind == CellKind::Number => {
                if !seeded || candidate.starts_with('-') {
                    return;
                }
                candidate.insert(0, '-');
            }
            Some(ch) => candidate.push(ch),
            None => {
                if candidate.pop().is_none() || candidate.is_empty() {
                    // Nothing left to delete: the buffer (and the pending)
                    // stay.
                    return;
                }
            }
        }
        let accepted = match kind {
            CellKind::Text => machine
                .draft_cell_value(WidgetCellValue::Text(candidate.clone()))
                .is_ok(),
            CellKind::Number => match candidate.parse::<f64>() {
                // The machine re-runs its own finiteness + cap gates on the
                // parsed value; a parse failure never reaches it.
                Ok(number) => machine
                    .draft_cell_value(WidgetCellValue::Number(number))
                    .is_ok(),
                Err(_) => false,
            },
            // Bool drafts take only the Space toggle.
            _ => false,
        };
        if accepted {
            scope.edit = Some(candidate);
        }
    };
    if backspace {
        edit_once(machine, scope, None);
    } else {
        for ch in typed.chars() {
            edit_once(machine, scope, Some(ch));
        }
    }
    Vec::new()
}

/// The v1 SCROLL law at the Up/Down window edges: advance the scroll
/// anchor one row and re-anchor the focus inside the re-materialized
/// window (its bottom row forward, its top row backward — the machine's
/// window-bound `row_focus_set` refuses anything else). A FORWARD landing
/// on the PUBLISHED TAIL is the page ask ([`WidgetMountMsg::TableTailReached`]
/// — the bridge route runs the machine's ask once). Already at an absolute
/// end: the tail ask when the focus rests there, else nothing.
fn table_scroll_step_owned(
    counters: &mut MountCounters,
    machine: &mut TableUiState,
    scope: &mut TableKeyScope,
    surface_id: StableId,
    table_id: StableId,
    forward: bool,
) -> Vec<WidgetMountMsg> {
    let published = machine.synced().map(|synced| synced.rows.len()).unwrap_or(0);
    if published == 0 {
        return Vec::new();
    }
    if forward {
        let anchor = machine.scroll_index();
        if anchor + 1 >= published {
            // Anchored at the published end: the tail ask (when the focus
            // rests there) — or nothing.
            if machine
                .synced()
                .and_then(|synced| synced.rows.last())
                .copied()
                .is_some_and(|tail| machine.focus() == Some(tail))
            {
                return vec![WidgetMountMsg::TableTailReached {
                    surface_id,
                    table_id,
                }];
            }
            return Vec::new();
        }
        machine.set_scroll_index(anchor + 1);
    } else {
        machine.set_scroll_index(machine.scroll_index().saturating_sub(1));
    }
    // Re-anchor INSIDE the re-materialized window.
    let materialized = machine.materialize_rows(&DEFAULT_WINDOW_SPEC);
    let target = if forward {
        materialized.keys.last().copied()
    } else {
        materialized.keys.first().copied()
    };
    let Some(target) = target else {
        return Vec::new();
    };
    if machine.row_focus_set(target, &DEFAULT_WINDOW_SPEC).is_err() {
        return Vec::new();
    }
    scope.cursor = None; // the column cursor re-anchors with the next Left/Right
    if forward && machine_at_published_tail(machine, target) {
        counters.keyboard_nav_steps += 1;
        return vec![WidgetMountMsg::TableTailReached {
            surface_id,
            table_id,
        }];
    }
    counters.keyboard_nav_steps += 1;
    Vec::new()
}

/// The display form of a skeleton value (the editing seed / the draft
/// editor's text — the same vocabulary the builder paints).
fn pending_display(value: &WidgetCellValue) -> String {
    match value {
        WidgetCellValue::Text(text) => text.clone(),
        WidgetCellValue::Number(number) => format!("{number}"),
        WidgetCellValue::Bool(value) => value.to_string(),
    }
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
// SCN-025 — the app-owned PANEL SURFACE STORE (the v26 wire attach)
// ---------------------------------------------------------------------------
//
// The living truth the v26 wire attach publishes into, patches over, and
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
// RECORDED BOUNDARY (SCN-025, updated SCN-031): the SCENE MOUNT rendering
// seam (a `PanelScene` render onto host UI state) was deferred — the scene
// leg was STORED here (patch truth + snapshot2/pages) and stayed invisible
// to the mounts. SCN-031 IS that seam package: the mount now READS the leg
// per view pass (`mounted_body_owned` → `scene_leg_element_owned`) and
// renders it through the retained cache slots (`scene_caches`) — while the
// store REMAINS the sole scene-truth owner: nothing scene-shaped is stored
// registry-side beyond the cache's prim output, and every store semantics
// below (owner law, revision law, page reads) is untouched.

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

    /// The scene cache slot read's test form — see
    /// [`Self::scene_cache_slot_read_owned`].
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    pub(crate) fn scene_cache_slot(
        panel_id: &str,
    ) -> Option<Rc<RefCell<SceneProjectionCache>>> {
        with_test_registry(|registry| registry.scene_cache_slot_read_owned(panel_id))
    }

    /// The body-shape probe's test form — see [`Self::body_shape_owned`].
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    pub(crate) fn body_shape(panel_id: &str) -> Option<MountBodyShape> {
        with_test_registry(|registry| registry.body_shape_owned(panel_id))
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
        // The typed-table context: an empty registry map + an empty surface
        // (this fixture registers no table machines).
        let empty_surface = WidgetSurface {
            revision: SceneRevision(1),
            roots: Vec::new(),
            tables: std::collections::BTreeMap::new(),
        };
        let no_machines = HashMap::new();
        let table_ctx = TableMountCtx {
            surface: &empty_surface,
            tables: &no_machines,
            cell_cursor: None,
        };

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
                    &DEFAULT_WINDOW_SPEC,
                    &table_ctx
                )
                .is_none(),
                "{node:?} must skip when visible == false"
            );
            assert!(
                build_node(
                    &visible(node.clone()),
                    &state,
                    &dispatcher,
                    &DEFAULT_WINDOW_SPEC,
                    &table_ctx
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
        let children = visible_children(
            &group_d,
            &state,
            &dispatcher,
            &DEFAULT_WINDOW_SPEC,
            &table_ctx,
        );
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
        // (SCN-028) The typed-table context: an empty machine map + no
        // cursor (this fixture carries no tables). (SCN-031) No scene leg —
        // the pre-seam body, byte-identical.
        let no_machines = HashMap::new();
        let _body = handle.body(&no_machines, None, None);
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

    /// A re-register whose surface MOVED to a DIFFERENT surface_id (the
    /// teardown-class branch, SCN-033: a genuinely new logical surface)
    /// REPLACES: the outgoing machines release through the one dispose
    /// path (pending children, armed marks), the counters say "replaced" +
    /// one torn_down, and the slot's machines are fresh (no state carried
    /// over). (The SAME-surface-id moved re-register is the EVOLUTION arm
    /// instead — the machines CARRY; see
    /// [`register_same_surface_evolution_carries_the_machines`].)
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

        // The CHANGED surface: a raised revision AND a moved root (the
        // list's widget id changes 200 → 210 — a different surface_id, the
        // teardown-class branch).
        let changed = WidgetSurface {
            revision: SceneRevision(2),
            ..surface(vec![WidgetNode::List(item_list(
                210,
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
            "the teardown-class branch released the outgoing machines through the ONE dispose path"
        );

        // The slot's machines are FRESH: a new Rc pair, no carried state.
        let new_handle = MountRegistry::handle_for_surface(sid(210)).expect("still registered");
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

    /// THE REGRESSION PIN (SCN-033 — the live-firetest defect): the demo's
    /// lazy tree branch. Row 1 is expanded THROUGH the machine (the
    /// `RowExpandedToggled` apply path — the toggle the GUI and the MCP
    /// ask both ride), the lazy child-page request parks, and then a
    /// CHANGED surface re-registers with the SAME surface_id (the patch
    /// that delivers the children) with NO state replay: the CARRIED
    /// machine still reports the row EXPANDED, the new children appear in
    /// the flat views (the branch STAYS open), and the lazy dedup window
    /// closes on the patch's arrival. Before the fix the wire era's
    /// replace built fresh `from_surface` machines on every changed
    /// surface and the branch collapsed back on every applied patch.
    #[test]
    fn register_same_surface_evolution_carries_the_machines() {
        let lazy_root = || TreeRowDef {
            lazy_children: true,
            ..expandable_row(1, None, "Root A")
        };
        let _guard = register_test_surface(
            "scn033_evolve",
            surface(vec![tree(
                100,
                vec![lazy_root(), tree_row(11, Some(1), "A.1")],
                false,
            )]),
        );
        let handle = MountRegistry::handle_for_surface(sid(100)).expect("registered");

        // Expand THROUGH the machine: the toggle emits the expanded
        // notification AND the lazy child-page request (the demo's ask).
        let routing = {
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            let mut sink = Vec::new();
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
            handle.state.borrow().explicit_expansion(sid(1)),
            Some(true),
            "the machine holds the expansion"
        );
        assert_eq!(
            handle.state.borrow().pending_children().len(),
            1,
            "the lazy page request parked"
        );

        // The patch: children arrive under row 1 (the WidgetOp::Add
        // shape) — the SAME surface_id, a raised revision, NO state
        // replay.
        let patched = WidgetSurface {
            revision: SceneRevision(2),
            ..surface(vec![tree(
                100,
                vec![
                    lazy_root(),
                    tree_row(11, Some(1), "A.1"),
                    tree_row(12, Some(1), "A.2"),
                    tree_row(13, Some(1), "A.3"),
                ],
                false,
            )])
        };
        let before = MountRegistry::counters();
        let _evolved = register_test_surface("scn033_evolve", patched);
        let after = MountRegistry::counters();
        assert_eq!(
            after.registers_replaced - before.registers_replaced,
            1,
            "the moved shape re-registered as a replacement ENTRY"
        );
        assert_eq!(
            after.torn_down - before.torn_down,
            0,
            "the EVOLUTION arm carried the machines — no release ran"
        );

        // The SAME machines hold the slot.
        let carried = MountRegistry::handle_for_surface(sid(100)).expect("still registered");
        assert!(
            Rc::ptr_eq(&carried.state, &handle.state),
            "the state machine was CARRIED, not rebuilt"
        );
        assert!(
            Rc::ptr_eq(&carried.dispatcher, &handle.dispatcher),
            "the dispatcher was CARRIED, not rebuilt"
        );

        // The expansion SURVIVED, the patch's children are REVEALED under
        // the still-expanded row, no release ran, and the lazy dedup
        // window closed on the patch's arrival (the ask's answer landed).
        let state = carried.state.borrow();
        assert_eq!(
            state.explicit_expansion(sid(1)),
            Some(true),
            "the expansion survived the same-surface patch"
        );
        assert_eq!(
            state.mutations().whole_state_resets,
            0,
            "the carry is not a release"
        );
        let views = state.synced().expect("re-synced").views.clone();
        let WidgetNode::Tree(tree_def) = &carried.surface.roots[0] else {
            panic!("the fixture's tree root");
        };
        let flat: Vec<StableId> = state
            .flat_rows_of_tree(tree_def, &views)
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(
            flat,
            vec![sid(1), sid(11), sid(12), sid(13)],
            "the patch's children revealed under the STILL-expanded row"
        );
        assert!(
            state.pending_children().is_empty(),
            "the dedup window closed: the patch arrived under the requested parent"
        );
    }

    /// Selection survives the same-surface evolution exactly as the
    /// machine's own contract states: a row the patch kept stays
    /// selected, a row the patch REMOVED is pruned (by the carried
    /// machine's sync — never a release), and `whole_state_resets` never
    /// moves.
    #[test]
    fn register_same_surface_evolution_keeps_selection_and_prunes_gone_rows() {
        let _guard = register_test_surface(
            "scn033_select",
            surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "row"), item(8, "keep"), item(9, "gone")],
            ))]),
        );
        let handle = MountRegistry::handle_for_surface(sid(200)).expect("registered");
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
                    selected: vec![sid(8), sid(9)],
                },
                &mut sink,
            );
        }
        assert_eq!(handle.state.borrow().selection(), [sid(8), sid(9)]);

        // The patch: row 9 removed, row 10 added — the SAME surface_id.
        let patched = WidgetSurface {
            revision: SceneRevision(2),
            ..surface(vec![WidgetNode::List(item_list(
                200,
                vec![item(7, "row"), item(8, "keep"), item(10, "new")],
            ))])
        };
        let before = MountRegistry::counters();
        let _evolved = register_test_surface("scn033_select", patched);
        assert_eq!(
            MountRegistry::counters().torn_down - before.torn_down,
            0,
            "the evolution carried the machines"
        );

        let carried = MountRegistry::handle_for_surface(sid(200)).expect("still registered");
        assert!(
            Rc::ptr_eq(&carried.state, &handle.state),
            "the SAME machine holds the slot"
        );
        let state = carried.state.borrow();
        assert_eq!(
            state.selection(),
            [sid(8)],
            "the untouched row stayed selected; the removed row was PRUNED (the machine's own contract)"
        );
        assert_eq!(
            state.mutations().whole_state_resets,
            0,
            "a prune is not a release"
        );
    }

    /// Armed-prompt continuity across the two swap classes (SCN-033): the
    /// same-surface EVOLUTION keeps the armed desc (same surface, same
    /// interaction — the armed stage even completes on the carried
    /// dispatcher); a teardown-class replace (a DIFFERENT surface_id)
    /// removes the desc with the released machines.
    #[test]
    fn register_armed_prompt_continuity_across_the_swap_classes() {
        let prompt_button = || {
            WidgetNode::IconButton(IconActionDef {
                confirmation: Some(ConfirmPolicy::Prompt {
                    message: "Really delete the selection?".to_string(),
                }),
                ..button_def(110, "Delete", "delete")
            })
        };
        let _guard = register_test_surface(
            "scn033_prompt",
            surface(vec![
                WidgetNode::List(item_list(200, vec![item(7, "row")])),
                prompt_button(),
            ]),
        );
        let handle = MountRegistry::handle_for_surface(sid(200)).expect("registered");

        // Select (the command's targets), then arm THROUGH the dispatcher
        // (the click's PromptRequested edge); record the desc the way the
        // app's update arm does.
        {
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            let mut sink = Vec::new();
            let routing = apply_mount_msg(
                &mut state,
                &mut dispatcher,
                &WidgetMountMsg::SelectionChanged {
                    surface_id: sid(200),
                    widget_id: sid(200),
                    selected: vec![sid(7)],
                },
                &mut sink,
            );
            assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
            let routing =
                apply_mount_msg(&mut state, &mut dispatcher, &click_msg(200, 110), &mut sink);
            assert_eq!(
                routing,
                MountRouting::PromptRequested {
                    prompt: "Really delete the selection?".to_string(),
                }
            );
        }
        MountRegistry::mount_record_prompt_armed(sid(200), DispatchKey::Button(sid(110)));
        assert!(
            MountRegistry::prompt_dialog(sid(200)).is_some(),
            "the armed desc is registered"
        );

        // (a) Same-surface evolution (children added to the list): the
        // desc STAYS armed, and the armed stage completes on the CARRIED
        // dispatcher.
        let evolved = WidgetSurface {
            revision: SceneRevision(2),
            ..surface(vec![
                WidgetNode::List(item_list(200, vec![item(7, "row"), item(8, "b")])),
                prompt_button(),
            ])
        };
        let _evolved_guard = register_test_surface("scn033_prompt", evolved);
        let carried = MountRegistry::handle_for_surface(sid(200)).expect("still registered");
        assert!(
            Rc::ptr_eq(&carried.dispatcher, &handle.dispatcher),
            "the dispatcher (armed stage included) carried"
        );
        assert!(
            MountRegistry::prompt_dialog(sid(200)).is_some(),
            "same surface, same interaction: the armed desc stays"
        );
        let outcome = carried
            .dispatcher
            .borrow_mut()
            .confirm(DispatchKey::Button(sid(110)))
            .expect("the armed stage survived the evolution");
        assert!(
            matches!(outcome, DispatchOutcome::ConfirmedEmitted(WidgetEvent::CommandInvoked { .. })),
            "the carried armed stage completes: exactly ONE CommandInvoked"
        );

        // (b) Teardown-class replace (the list re-ids 200 → 210): the
        // desc dies with the released surface.
        let replaced = WidgetSurface {
            revision: SceneRevision(3),
            ..surface(vec![
                WidgetNode::List(item_list(210, vec![item(7, "row")])),
                prompt_button(),
            ])
        };
        let _replaced_guard = register_test_surface("scn033_prompt", replaced);
        assert!(
            MountRegistry::prompt_dialog(sid(200)).is_none(),
            "a different surface_id releases: the armed desc left with its surface"
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

    // -- 17. SCN-028 — the typed-table MOUNT integration ---------------------

    use ocs_plugin_api::widgets::BadgeTone;
    use ocs_plugin_api::scene::MAX_TEXT_BYTES;

    /// The LIVE demo's shape (the task's gold standard): table 200, columns
    /// 201..=204 (text/number/bool/badge — the badge is the display-only
    /// probe), row keys `300..` with typed cells; `total` windows the
    /// backing set (`None` = fully materialized).
    fn demo_table_def(published: u64, total: Option<u64>) -> PanelTableDef {
        PanelTableDef {
            id_table: sid(200),
            columns: vec![
                table_column(201, CellKind::Text, "item"),
                table_column(202, CellKind::Number, "length"),
                table_column(203, CellKind::Bool, "locked"),
                table_column(204, CellKind::Badge, "state"),
            ],
            total_rows: total,
            rows: (0..published)
                .map(|index| TableRowDef {
                    key: sid(300 + index),
                    cells: vec![
                        TableCell::Text(format!("material {index:03}")),
                        TableCell::Number(index as f64),
                        TableCell::Bool(index % 2 == 0),
                        TableCell::Badge {
                            label: if index % 2 == 0 {
                                "locked".to_string()
                            } else {
                                "draft".to_string()
                            },
                            tone: BadgeTone::Info,
                        },
                    ],
                    domain_ref: None,
                })
                .collect(),
        }
    }

    /// The demo surface: ONE root — the `TableRef` (widget id 120) naming
    /// table 200 — so the registered surface id IS `sid(120)`.
    fn demo_surface(def: PanelTableDef) -> WidgetSurface {
        WidgetSurface {
            revision: SceneRevision(1),
            roots: vec![WidgetNode::Table(TableRef {
                id: sid(120),
                name: Some("materials".to_string()),
                enabled: true,
                visible: true,
                table_id: sid(200),
            })],
            tables: std::collections::BTreeMap::from([(sid(200), def)]),
        }
    }

    /// A LOCAL machine map for the pure builder (the builder never needs
    /// the registry — the view pass reads only).
    fn local_machines(surface: &WidgetSurface) -> HashMap<StableId, TableUiState> {
        let mut machines = HashMap::new();
        for table_id in surface.tables.keys() {
            let mut machine = TableUiState::new(sid(120), *table_id);
            machine.sync(surface);
            machines.insert(*table_id, machine);
        }
        machines
    }

    /// The demo def's roots' TableRef (the builder call's argument).
    fn demo_table_ref(surface: &WidgetSurface) -> TableRef {
        match &surface.roots[0] {
            WidgetNode::Table(def) => def.clone(),
            other => panic!("expected a table ref, got {other:?}"),
        }
    }

    /// Read one registered machine through the cfg(test) registry cell.
    fn table_read<R>(
        surface: u64,
        table: u64,
        read: impl FnOnce(&TableUiState) -> R,
    ) -> Option<R> {
        with_test_registry(|registry| {
            registry
                .table_state_owned(sid(surface), sid(table))
                .map(read)
        })
    }

    /// The registered surface's table-scope cursor read.
    fn scope_cursor_of(surface: u64, table: u64) -> Option<CellFocus> {
        with_test_registry(|registry| registry.table_scope_cursor_of(sid(surface), sid(table)))
    }

    /// The scope-active read.
    fn scope_active(surface: u64) -> bool {
        with_test_registry(|registry| registry.table_scope_active(sid(surface)))
    }

    // -- 17a. the def-present regime renders the REAL table -------------------

    /// A def-present table renders interactively (headers in DEF order, the
    /// machine's materialized window, machine-owned selection/focus/draft
    /// facts, display-only kinds marked); a def-ABSENT `TableRef` keeps the
    /// muted placeholder bytes (clickable: false).
    #[test]
    fn table_def_present_renders_real_table_and_def_absent_keeps_placeholder() {
        let def = demo_table_def(4, None);
        let surface = demo_surface(def.clone());
        let machines = local_machines(&surface);
        let table_ref = demo_table_ref(&surface);
        let ctx = TableMountCtx {
            surface: &surface,
            tables: &machines,
            cell_cursor: None,
        };

        // Def present: interactive facts + the def-ordered view model.
        let (element, facts) = table_element(&table_ref, &surface, &ctx, &DEFAULT_WINDOW_SPEC);
        assert!(facts.clickable, "the real table is interactive");
        assert_eq!(facts.widget_id, sid(120));
        assert_eq!(facts.a11y, "materials");
        let _ = element; // built headless

        let view = table_view_model(&def, &machines[&sid(200)], None, &DEFAULT_WINDOW_SPEC);
        assert_eq!(
            view.headers,
            vec!["item", "length", "locked", "state"],
            "headers in def.columns order"
        );
        assert_eq!(
            view.column_ids,
            vec![sid(201), sid(202), sid(203), sid(204)]
        );
        assert_eq!(
            view.writable,
            vec![true, true, true, false],
            "text/number/bool writable; badge display-only"
        );
        assert_eq!(
            view.row_keys,
            vec![sid(300), sid(301), sid(302), sid(303)],
            "the materialized window, not a full dump"
        );
        assert_eq!(view.published_rows, 4);
        assert_eq!(view.total_rows, None);
        assert!(view.selected.is_empty() && view.focused.is_none() && view.draft.is_none());
        assert_eq!(view.cells[0][3], "locked", "the badge cell renders its label");

        // Def absent: the placeholder (never interactive).
        let orphan = WidgetSurface {
            revision: SceneRevision(1),
            roots: vec![WidgetNode::Table(TableRef {
                id: sid(121),
                name: Some("ghost".to_string()),
                enabled: true,
                visible: true,
                table_id: sid(999),
            })],
            tables: std::collections::BTreeMap::new(),
        };
        let no_machines = HashMap::new();
        let orphan_ctx = TableMountCtx {
            surface: &orphan,
            tables: &no_machines,
            cell_cursor: None,
        };
        let WidgetNode::Table(orphan_ref) = &orphan.roots[0] else {
            unreachable!()
        };
        let (_, orphan_facts) =
            table_element(orphan_ref, &orphan, &orphan_ctx, &DEFAULT_WINDOW_SPEC);
        assert!(!orphan_facts.clickable, "def-absent keeps the placeholder");
        assert_eq!(orphan_facts.a11y, "ghost");
    }

    // -- 17b. the pointer routes (the two-phase policy + the tail ask) -------

    /// Press = select (the machine's law; the SelectionChanged event
    /// forwards through the typed-table bridge) + the cell cursor moves; a
    /// press on the ALREADY-focused cursor cell is the activation gesture
    /// (the edit arms); a display-only column refuses the edit QUIETLY (the
    /// machine's counter is the probe).
    #[test]
    fn table_pointer_press_selects_then_edits_on_second_press() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn028_pointer",
            demo_surface(demo_table_def(4, None)),
        );
        let press = |key: u64, column: u64| {
            WidgetMountMsg::TableRowPressed {
                surface_id: sid(120),
                table_id: sid(200),
                key: sid(key),
                column: sid(column),
            }
        };

        // Phase one: select + cursor.
        let first = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(300, 201))
        });
        assert_eq!(first.routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(
            first.events,
            vec![WidgetEvent::SelectionChanged {
                surface_id: sid(120),
                selected_ids: vec![sid(300)],
            }],
            "the selection event forwards through the table bridge"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.selection().to_vec()),
            Some(vec![sid(300)])
        );
        assert_eq!(
            scope_cursor_of(120, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(201)
            })
        );

        // Phase two (same cell): the edit arms. (An identical selection
        // rewrite still EMITS the event — the machine's diff counter is
        // what stays 0; the forwarded row is the notify.)
        let second = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(300, 201))
        });
        assert_eq!(
            second.routing,
            MountRouting::Applied { forwarded: 1 },
            "the selection notify re-emits; the machine's diff write is 0"
        );
        assert!(matches!(
            table_read(120, 200, |machine| machine.draft().map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Text(text))) if text == "material 000"
        ));

        // The cap enforcement lives in the machine (the host-multi-select
        // law): a past-cap offer refuses state-side and moves nothing.
        let cap_delta = table_read(120, 200, |machine| machine.mutations().row_state_changes);
        let too_many: Vec<StableId> = (0..(ocs_plugin_api::widgets::MAX_SELECTION_IDS as u64 + 1))
            .map(|i| sid(10_000 + i))
            .collect();
        with_test_registry_mut(|registry| {
            let machine = registry
                .table_machine_mut_owned(sid(120), sid(200))
                .expect("the machine");
            assert!(machine.set_selection(too_many).is_err());
        });
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().row_state_changes),
            cap_delta,
            "a refused selection writes nothing"
        );

        // Escape through the production route cancels the draft FREE.
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(120)));
            assert!(bench_handle_mount_key_owned(
                registry,
                Key::Named(keyboard_key::Named::Escape),
                Modifiers::NONE
            )
            .is_empty());
        });
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false)
        );

        // The badge column: the cursor moves onto it too, but the edit
        // refuses QUIETLY (the kind gate's counter; no draft).
        let badge_cursor = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(300, 204))
        });
        assert_eq!(badge_cursor.routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(
            scope_cursor_of(120, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(204)
            })
        );
        let refusals_before =
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals);
        let _ = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(300, 204))
        });
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals),
            refusals_before.map(|count| count + 1),
            "the kind gate counted ONE quiet refusal"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false),
            "a display-only column never arms a draft"
        );
    }

    /// THE TWO-PHASE PRESS INVESTIGATION (the coordinator's synthetic
    /// observation: press-2 ~1–2s after press-1 "did NOT arm — press 3
    /// did"). Finding: NO production defect — the press route's
    /// `already_here` probe (`machine.focus()` + the PRE-press cursor read
    /// in `apply_mount_table_msg_owned`) is state-pure: in ONE process
    /// press-2 arms (pinned by
    /// `table_pointer_press_selects_then_edits_on_second_press` and re-pinned
    /// here as the exact three-press sequence). The observation reproduces
    /// ONLY across a process boundary: two SEPARATE script invocations each
    /// start a fresh app — a fresh registry whose machines, focus and
    /// cursor are empty — so invocation two's "press-2" is a PHASE-ONE
    /// press (selection + cursor, no arm), and its "press-3" (the first
    /// repeat WITHIN that process) arms. Pinned here as the split-process
    /// simulation: the in-memory mount state is exactly what a process
    /// restart discards; the production arm route is unchanged.
    #[test]
    fn table_press_arm_in_process_and_split_invocation_analysis() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn035_press_split",
            demo_surface(demo_table_def(4, None)),
        );
        let press = |key: u64, column: u64| {
            WidgetMountMsg::TableRowPressed {
                surface_id: sid(120),
                table_id: sid(200),
                key: sid(key),
                column: sid(column),
            }
        };
        let route = |msg: WidgetMountMsg| {
            with_test_registry_mut(|registry| route_mount_table_msg_owned(registry, &msg))
        };

        // INVOCATION ONE: press-1 (phase one), press-2 (arms — in-process).
        let _ = route(press(300, 201));
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false),
            "press-1: selection + cursor, no editor yet"
        );
        let _ = route(press(300, 201));
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(true),
            "press-2 IN-PROCESS arms (the already_here probe has no defect)"
        );

        // INVOCATION TWO (a NEW app/registry — what a separate script run
        // sees; every in-memory fact above is gone): the SAME cell's press
        // is a phase-one press again — NO arm (the observation) — and the
        // NEXT press within this process arms (press-3, the observation).
        let _app2 = OpenCADStudio::new_for_test();
        let _guard2 = register_test_surface(
            "scn035_press_split",
            demo_surface(demo_table_def(4, None)),
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false),
            "the fresh process carries no draft, no focus, no cursor"
        );
        let _ = route(press(300, 201));
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false),
            "press-2 across the process boundary: phase one, no arm"
        );
        let _ = route(press(300, 201));
        assert!(
            matches!(
                table_read(120, 200, |machine| machine
                    .draft()
                    .map(|draft| draft.pending.clone())),
                Some(Some(WidgetCellValue::Text(text))) if text == "material 000"
            ),
            "press-3 — the first repeat of ITS process — arms"
        );
    }

    /// The published-tail press asks for the next page ONCE (the machine's
    /// dedup suppresses the repeat); a mid-table press asks nothing.
    #[test]
    fn table_tail_press_requests_page_once_and_duplicate_suppressed() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn028_tail",
            demo_surface(demo_table_def(8, Some(40))),
        );
        let press = |key: u64| WidgetMountMsg::TableRowPressed {
            surface_id: sid(120),
            table_id: sid(200),
            key: sid(key),
            column: sid(202),
        };

        // A mid-table press: selection only, no page ask.
        let mid = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(303))
        });
        assert_eq!(mid.routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(mid.events.len(), 1, "selection only");
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().page_requests_emitted),
            Some(0)
        );

        // The tail press: selection + ONE TablePageRequested, forwarded
        // through the table bridge.
        let tail = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(307))
        });
        assert_eq!(
            tail.events,
            vec![
                WidgetEvent::SelectionChanged {
                    surface_id: sid(120),
                    selected_ids: vec![sid(307)],
                },
                WidgetEvent::TablePageRequested {
                    surface_id: sid(120),
                    table: sid(200),
                    first_key: sid(307),
                    len: MAX_WIDGET_RANGE_LEN,
                },
            ]
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().page_requests_emitted),
            Some(1)
        );

        // The duplicate press: the selection notify re-emits (the machine's
        // diff write is 0) but the PAGE ASK is suppressed BY THE MACHINE.
        let repeat = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(307))
        });
        assert_eq!(
            repeat.events,
            vec![WidgetEvent::SelectionChanged {
                surface_id: sid(120),
                selected_ids: vec![sid(307)],
            }],
            "the page ask was suppressed; only the selection notify re-emitted"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().duplicate_pages_suppressed),
            Some(1)
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().page_requests_emitted),
            Some(1)
        );
    }

    // -- 17c. the keyboard scope (Tab dive/leave, roving, edges) --------------

    /// The ring law (SCN-028, unchanged bytes): Tab walks the WIDGET ring,
    /// the edge DIVES into the table (the machine's row focus lands on the
    /// window's first row; the dive arms nothing). The SCOPE law (SCN-030
    /// form semantics, SCN-035's arm): Tab INSIDE advances the cell cursor
    /// across the row AND ARMS the landed writable cell (the seeded draft —
    /// the user types directly); the row wraps DOWN to the next row's first
    /// column (armed there too); Shift+Tab reverses (wrapping UP to the
    /// last column, arming likewise); a display-only landing moves the
    /// cursor and refuses QUIETLY (the machine's counter is the probe);
    /// only the table's MATERIALIZED edge LEAVES to the ring (no arm on a
    /// leave; the machine's row focus stays parked — re-entering resumes
    /// forward).
    #[test]
    fn table_keyboard_tab_dives_in_and_advances_form_semantics() {
        let _app = OpenCADStudio::new_for_test();
        // A MIXED surface: list rows 7/8 + the demo table.
        let mut surface = demo_surface(demo_table_def(4, None));
        surface.roots.insert(
            0,
            WidgetNode::List(item_list(200, vec![item(7, "one"), item(8, "two")])),
        );
        let _guard = register_test_surface("scn028_tab", surface);
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(200)));
        });
        let shift_tab = || (Key::Named(keyboard_key::Named::Tab), Modifiers::SHIFT);
        let key = |(key, modifiers): (Key, Modifiers)| {
            with_test_registry_mut(|registry| {
                bench_handle_mount_key_owned(registry, key, modifiers)
            })
        };
        let bare_tab = || (Key::Named(keyboard_key::Named::Tab), Modifiers::NONE);
        let escape = || (Key::Named(keyboard_key::Named::Escape), Modifiers::NONE);

        let handle = MountRegistry::handle_for_surface(sid(200)).expect("registered");
        assert!(!scope_active(200));

        // The widget ring first (unchanged bytes).
        let msgs = key(bare_tab());
        assert_eq!(
            msgs,
            vec![WidgetMountMsg::FocusMoved {
                surface_id: sid(200),
                widget_id: sid(200),
                to: Some(sid(7)),
            }]
        );
        assert_eq!(handle.state.borrow().focus(), Some(sid(7)));
        let _ = key(bare_tab());
        assert_eq!(handle.state.borrow().focus(), Some(sid(8)));

        // The EDGE: no wrap — the DIVE. Row focus lands on the window's
        // first row; the machine's row focus is the table's own namespace.
        let msgs = key(bare_tab());
        assert!(msgs.is_empty(), "the dive routes no ring message");
        assert!(scope_active(200));
        assert_eq!(
            table_read(200, 200, |machine| machine.focus()),
            Some(Some(sid(300)))
        );
        // (SCN-035) The DIVE arms NOTHING — the law arms the ADVANCE
        // landings (the dive lands no cursor at all).
        assert_eq!(
            table_read(200, 200, |machine| machine.draft().is_some()),
            Some(false),
            "the dive lands no editor"
        );

        // Tab INSIDE (form semantics, SCN-035): the cursor ADVANCES *AND*
        // the edit ARMS — one press lands the user IN the editor (the
        // seeded draft; the scope STAYS, the machine's row focus stays).
        let _ = key(bare_tab());
        assert_eq!(
            scope_cursor_of(200, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(201)
            }),
            "the first Tab lands the first column"
        );
        assert_eq!(
            table_read(200, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Text("material 000".to_string()))),
            "the landing ARMED: the seeded text draft"
        );
        // The user types DIRECTLY into the armed draft — no second gesture.
        let _ = key((Key::Character("Z".to_string().into()), Modifiers::NONE));
        assert_eq!(
            table_read(200, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Text("material 000Z".to_string()))),
            "the typing composed the armed draft"
        );
        // Escape stays the FREE cancel; the cursor stays put.
        let _ = key(escape());
        assert_eq!(
            table_read(200, 200, |machine| machine.draft().is_some()),
            Some(false)
        );
        assert_eq!(
            scope_cursor_of(200, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(201)
            }),
            "the free cancel left the cursor as-was"
        );

        // The WRITABLE landings arm their kinds; the DISPLAY-ONLY landing
        // (badge 204) moves the cursor and refuses QUIETLY — the machine's
        // gate is the authority, its counter the probe.
        let _ = key(bare_tab()); // → (300, 202)
        assert_eq!(
            table_read(200, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Number(0.0))),
            "the Number landing armed its seeded draft"
        );
        let _ = key(escape());
        let _ = key(bare_tab()); // → (300, 203)
        assert_eq!(
            table_read(200, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Bool(true))),
            "the Bool landing armed its seeded draft"
        );
        let _ = key(escape());
        let _ = key(bare_tab()); // → (300, 204) — the BADGE
        assert_eq!(
            scope_cursor_of(200, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(204)
            }),
            "the non-writable landing still moves the cursor"
        );
        assert_eq!(
            table_read(200, 200, |machine| machine.draft().is_some()),
            Some(false),
            "the quiet refusal: no draft"
        );
        assert_eq!(
            table_read(200, 200, |machine| machine.mutations().invalid_cell_refusals),
            Some(1),
            "the machine's kind gate counted ONE refusal"
        );
        assert!(scope_active(200), "the scope survives the in-row advance");

        // The ROW's column edge: Tab wraps DOWN one row to its FIRST column.
        let _ = key(bare_tab());
        assert_eq!(
            table_read(200, 200, |machine| machine.focus()),
            Some(Some(sid(301))),
            "the wrap moved the row"
        );
        assert_eq!(
            scope_cursor_of(200, 200),
            Some(CellFocus {
                key: sid(301),
                column: sid(201)
            }),
            "the wrapped row's first column"
        );
        assert_eq!(
            table_read(200, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Text("material 001".to_string()))),
            "the row-wrap landing armed too (the new row's first cell)"
        );
        let _ = key(escape());

        // Shift+Tab REVERSES: the first column wraps UP one row to its LAST.
        let _ = key(shift_tab());
        assert_eq!(
            table_read(200, 200, |machine| machine.focus()),
            Some(Some(sid(300))),
            "the backward wrap moved the row back"
        );
        assert_eq!(
            scope_cursor_of(200, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(204)
            }),
            "the wrapped-back row's LAST column"
        );
        assert_eq!(
            table_read(200, 200, |machine| machine.draft().is_some()),
            Some(false),
            "the backward landing is the display-only column: cursor only"
        );
        assert_eq!(
            table_read(200, 200, |machine| machine.mutations().invalid_cell_refusals),
            Some(2),
            "the reverse onto the badge refused once more"
        );

        // Walk back to the FIRST row's FIRST cell (each writable landing
        // arms; the free cancel keeps the walk clean), then Shift+Tab: the
        // BACKWARD materialized edge LEAVES to the ring (the leave path's
        // FocusMoved, the ring walk restarted).
        for column in [203, 202, 201] {
            let _ = key(shift_tab());
            assert_eq!(
                scope_cursor_of(200, 200),
                Some(CellFocus {
                    key: sid(300),
                    column: sid(column)
                })
            );
            assert_eq!(
                table_read(200, 200, |machine| machine.draft().is_some()),
                Some(true),
                "the reverse landing armed (column {column})"
            );
            let _ = key(escape());
        }
        assert_eq!(
            table_read(200, 200, |machine| machine.draft().is_some()),
            Some(false),
            "the walk back ended clean"
        );
        let msgs = key(shift_tab());
        assert!(!scope_active(200), "the backward edge left the scope");
        assert_eq!(
            msgs,
            vec![WidgetMountMsg::FocusMoved {
                surface_id: sid(200),
                widget_id: sid(200),
                to: Some(sid(8)),
            }],
            "the leave routes the ring walk (Shift direction: from its bottom)"
        );
        assert_eq!(
            table_read(200, 200, |machine| machine.focus()),
            Some(Some(sid(300))),
            "the row focus parks across the leave"
        );
        assert_eq!(
            table_read(200, 200, |machine| machine.draft().is_some()),
            Some(false),
            "the LEAVE arms nothing (no landing, no editor)"
        );

        // Re-enter: the roving RESUMES from the parked row (301).
        let _ = key(bare_tab()); // 7
        let _ = key(bare_tab()); // 8
        let _ = key(bare_tab()); // dive
        assert_eq!(
            table_read(200, 200, |machine| machine.focus()),
            Some(Some(sid(301))),
            "the roving resumed, not restarted"
        );
    }

    /// THE FORM CHAIN (SCN-030's commit law + SCN-035's arm, end-to-end):
    /// Tab arms the landed writable cell (seeded from the store), the user
    /// types directly, the next Tab COMMITS exactly ONE fact, advances, and
    /// arms the next writable cell — type → Tab → type → Tab, the standard
    /// form semantics; a display-only landing (the badge) takes the quiet
    /// refusal (cursor moves, no draft, the machine's counter the probe);
    /// the machine's commit is infallible post-gates (SCN-034), so no
    /// rollback shape can fire on this path. Enter's arm/commit halves are
    /// the unchanged law (the dedicated pins hold; sanity-pinned here).
    #[test]
    fn table_tab_form_chain_arms_types_commits_advances() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn035_tab_chain",
            demo_surface(demo_table_def(4, None)),
        );
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(120)));
        });
        let key = |key: Key, modifiers: Modifiers| {
            with_test_registry_mut(|registry| {
                bench_handle_mount_key_owned(registry, key, modifiers)
            })
        };
        let tab = || Key::Named(keyboard_key::Named::Tab);
        let enter = || Key::Named(keyboard_key::Named::Enter);
        let char_key = |text: &str| Key::Character(text.to_string().into());
        let draft_of = || {
            table_read(120, 200, |machine| {
                machine
                    .draft()
                    .map(|draft| (draft.key, draft.column, draft.pending.clone()))
            })
        };

        let _ = key(tab(), Modifiers::NONE); // the dive (the table-only ring is empty)
        let _ = key(
            Key::Named(keyboard_key::Named::ArrowRight),
            Modifiers::NONE,
        ); // the cursor lands (300, 201)
        assert_eq!(
            scope_cursor_of(120, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(201)
            })
        );

        // TAB #1: the ADVANCE lands (300, 202) and ARMS it — one press, the
        // seeded Number draft waits (no draft was open: no commit fact, no
        // message; the resting cell is left untouched — Enter edits the
        // resting cell, Tab edits the LANDING).
        let msgs = key(tab(), Modifiers::NONE);
        assert!(msgs.is_empty(), "an arming advance routes no message");
        assert_eq!(
            scope_cursor_of(120, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(202)
            }),
            "the advance landed"
        );
        assert_eq!(
            draft_of(),
            Some(Some((
                sid(300),
                sid(202),
                WidgetCellValue::Number(0.0)
            ))),
            "the Tab landing armed the seeded Number draft"
        );

        // The user types DIRECTLY (the Enter arm's own compose path).
        let _ = key(char_key("4"), Modifiers::NONE);
        let _ = key(char_key("2"), Modifiers::NONE);
        assert_eq!(
            draft_of(),
            Some(Some((
                sid(300),
                sid(202),
                WidgetCellValue::Number(42.0)
            ))),
            "the pending updates under the typing"
        );

        // TAB #2: exactly ONE commit fact, THEN the advance, THEN the next
        // arm — the full form step.
        let msgs = key(tab(), Modifiers::NONE);
        assert_eq!(
            msgs,
            vec![WidgetMountMsg::TableCellCommitted {
                surface_id: sid(120),
                table_id: sid(200),
                key: sid(300),
                column: sid(202),
                value: WidgetCellValue::Number(42.0),
            }],
            "exactly ONE commit fact, then the advance"
        );
        assert_eq!(
            scope_cursor_of(120, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(203)
            }),
            "the cursor advanced AFTER the commit"
        );
        assert_eq!(
            draft_of(),
            Some(Some((
                sid(300),
                sid(203),
                WidgetCellValue::Bool(true)
            ))),
            "the NEXT writable cell armed in the same press"
        );
        assert!(scope_active(120), "the scope survives the form step");

        // The Bool draft composes through its own value key (Space), then
        // TAB #3 lands on the BADGE: the cursor moves, the edit refuses
        // QUIETLY — no draft, the machine's counter the probe. The commit
        // is infallible post-gates (SCN-034): no rollback shape fired
        // anywhere on this chain.
        let _ = key(Key::Named(keyboard_key::Named::Space), Modifiers::NONE);
        assert_eq!(
            draft_of(),
            Some(Some((
                sid(300),
                sid(203),
                WidgetCellValue::Bool(false)
            ))),
            "the Bool draft toggled"
        );
        let refusals_before =
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals);
        let msgs = key(tab(), Modifiers::NONE);
        assert_eq!(
            msgs,
            vec![WidgetMountMsg::TableCellCommitted {
                surface_id: sid(120),
                table_id: sid(200),
                key: sid(300),
                column: sid(203),
                value: WidgetCellValue::Bool(false),
            }],
            "the open Bool draft committed first; the badge landing then refused quietly"
        );
        assert_eq!(
            scope_cursor_of(120, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(204)
            }),
            "the non-writable landing moved the cursor"
        );
        assert_eq!(
            draft_of(),
            Some(None),
            "the quiet refusal: NO draft on the badge"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals),
            refusals_before.map(|count| count + 1),
            "the kind gate counted ONE refusal"
        );

        // The chain's ledger: TWO form steps, TWO facts, one per cell.
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().cell_writes),
            Some(2),
            "type → Tab → type → Tab committed exactly one fact per step"
        );

        // (Enter's law rides unchanged: Escape frees the armed draft, the
        // cursor walks back, Enter re-arms — the dedicated pins hold.)
        let _ = key(Key::Named(keyboard_key::Named::Escape), Modifiers::NONE);
        for _ in 0..3 {
            let _ = key(
                Key::Named(keyboard_key::Named::ArrowLeft),
                Modifiers::NONE,
            );
        }
        let _ = key(enter(), Modifiers::NONE);
        assert!(
            matches!(
                draft_of(),
                Some(Some((key, column, WidgetCellValue::Text(_))))
                    if key == sid(300) && column == sid(201)
            ),
            "Enter arms as before (the law's unchanged half)"
        );
    }

    /// (SCN-030) A SUCCESSFUL table press CLAIMS the keyboard — the same
    /// desktop model the surface-level press runs — so the scope's keys
    /// survive a second registered surface (no sole-mount fallback).
    #[test]
    fn table_press_claims_the_keyboard_target() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn030_press_target",
            demo_surface(demo_table_def(4, None)),
        );
        // A SECOND registered surface: the sole-mount fallback would name
        // whichever it lists first — only the explicit target is correct.
        let _other = register_test_surface(
            "scn030_press_other",
            surface(vec![WidgetNode::List(item_list(200, vec![item(7, "x")]))]),
        );
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(None);
        });
        assert_eq!(
            with_test_registry(|registry| registry.mount_key_surface_owned()),
            None,
            "no fallback is trusted with two surfaces"
        );

        // The press: row 301's cell (301, 201) selects + focuses AND claims
        // the keyboard target.
        let _ = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(
                registry,
                &WidgetMountMsg::TableRowPressed {
                    surface_id: sid(120),
                    table_id: sid(200),
                    key: sid(301),
                    column: sid(201),
                },
            )
        });
        assert_eq!(
            with_test_registry(|registry| registry.mount_key_surface_owned()),
            Some(sid(120)),
            "the pressed table owns the keys"
        );
    }

    /// Inside the scope: Up/Down rove rows through the machine, the
    /// window-edge Down advances the scroll anchor (re-anchoring focus
    /// inside the re-materialized window), and the PUBLISHED-tail edge is
    /// the page ask — emitted once, dedup'd by the machine on the repeat.
    #[test]
    fn table_keyboard_arrows_rove_and_scroll_edges_page() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn028_arrows",
            demo_surface(demo_table_def(4, Some(40))),
        );
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(120)));
        });
        let key = |key: Key| {
            with_test_registry_mut(|registry| {
                bench_handle_mount_key_owned(registry, key, Modifiers::NONE)
            })
        };
        let down = || Key::Named(keyboard_key::Named::ArrowDown);
        let tab = || Key::Named(keyboard_key::Named::Tab);

        // Dive (the table-only surface's ring is empty — Tab dives at once;
        // the dive lands the window's FIRST row).
        assert!(key(down()).is_empty(), "arrows do nothing before the scope");
        assert!(key(tab()).is_empty(), "the dive");
        assert!(scope_active(120));
        assert_eq!(
            table_read(120, 200, |machine| machine.focus()),
            Some(Some(sid(300))),
            "the dive landed the window's first row"
        );

        // Down: the row roving (the machine's own focus slot).
        for expected in [301, 302, 303] {
            let _ = key(down());
            assert_eq!(
                table_read(120, 200, |machine| machine.focus()),
                Some(Some(sid(expected)))
            );
        }

        // The window edge (the whole published window materializes): the
        // scroll law advances the anchor and re-anchors the focus at the
        // window's bottom (still 303 — the published window IS four rows).
        let mut tail_msgs = Vec::new();
        for _ in 0..3 {
            tail_msgs = key(down());
            assert_eq!(
                table_read(120, 200, |machine| machine.focus()),
                Some(Some(sid(303))),
                "the re-anchor stays inside the window"
            );
        }
        assert_eq!(
            table_read(120, 200, |machine| machine.scroll_index()),
            Some(3),
            "the anchor advanced to the published end"
        );
        // The FINAL edge (anchored at the published end, focus on the tail):
        // the page ask rides out as the tail-reach message.
        assert_eq!(
            tail_msgs,
            vec![WidgetMountMsg::TableTailReached {
                surface_id: sid(120),
                table_id: sid(200),
            }]
        );

        // The route runs the machine's ask EXACTLY once:
        let outcome = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(
                registry,
                &WidgetMountMsg::TableTailReached {
                    surface_id: sid(120),
                    table_id: sid(200),
                },
            )
        });
        assert_eq!(
            outcome.events,
            vec![WidgetEvent::TablePageRequested {
                surface_id: sid(120),
                table: sid(200),
                first_key: sid(303),
                len: MAX_WIDGET_RANGE_LEN,
            }]
        );

        // The repeat: the machine's dedup suppresses (nothing forwards).
        let _ = key(down());
        let repeat = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(
                registry,
                &WidgetMountMsg::TableTailReached {
                    surface_id: sid(120),
                    table_id: sid(200),
                },
            )
        });
        assert_eq!(repeat.events, Vec::new());
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().duplicate_pages_suppressed),
            Some(1)
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().page_requests_emitted),
            Some(1)
        );

        // Up walks backward (the same machine law).
        let _ = key(Key::Named(keyboard_key::Named::ArrowUp));
        assert_eq!(
            table_read(120, 200, |machine| machine.focus()),
            Some(Some(sid(302))),
            "Up roves backward through the window"
        );
    }

    // -- 17d. the edit lifecycle through the keyboard -------------------------

    /// Enter arms the edit at the CURSOR cell (the machine seeds the
    /// pending from the current cell); with no cursor the row's FIRST
    /// column arms; a display-only column's Enter refuses QUIETLY (the
    /// kind gate's counter, no draft).
    #[test]
    fn table_keyboard_enter_arms_and_kind_gate_refuses() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn028_enter",
            demo_surface(demo_table_def(4, None)),
        );
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(120)));
        });
        let key = |key: Key| {
            with_test_registry_mut(|registry| {
                bench_handle_mount_key_owned(registry, key, Modifiers::NONE)
            })
        };
        let enter = || Key::Named(keyboard_key::Named::Enter);
        let right = || Key::Named(keyboard_key::Named::ArrowRight);
        let tab = || Key::Named(keyboard_key::Named::Tab);

        assert!(key(tab()).is_empty(), "the dive");
        // No cursor yet: Enter arms the ROW'S FIRST column.
        assert!(key(enter()).is_empty(), "arming routes no message");
        assert!(matches!(
            table_read(120, 200, |machine| machine
                .draft()
                .map(|draft| (draft.key, draft.column, draft.pending.clone()))),
            Some(Some((key, column, WidgetCellValue::Text(_))))
                if key == sid(300) && column == sid(201)
        ));

        // Walk the cursor to the badge column (201 → 202 → 203 → 204);
        // Enter while the draft is open refuses in-flight (uncounted dedup).
        let _ = key(right());
        let _ = key(right());
        let _ = key(right());
        let _ = key(right());
        assert_eq!(
            scope_cursor_of(120, 200),
            Some(CellFocus {
                key: sid(300),
                column: sid(204)
            })
        );
        let _ = key(enter());
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals),
            Some(0),
            "an in-flight refusal is dedup, not a defect count"
        );

        // Cancel (free), then Enter on the display-only column: the KIND
        // gate refuses — ONE counted refusal, NO draft.
        assert!(key(Key::Named(keyboard_key::Named::Escape)).is_empty());
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false)
        );
        let _ = key(enter());
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals),
            Some(1),
            "the kind gate counted the refusal"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false),
            "no draft, no event, nothing half-applied"
        );
    }

    /// The draft's value keys: characters compose through the machine's
    /// OWN gates (Number: parse-then-`draft_cell_value`; the buffer
    /// advances ONLY on acceptance), a past-cap text refuses with the
    /// machine's OWN counter, and Enter commits EXACTLY ONE fact — the
    /// bridge forwards ONE `TableCellChanged` and settles the address.
    #[test]
    fn table_draft_typing_gates_and_commit_forwards_once() {
        let _app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn028_typing",
            demo_surface(demo_table_def(4, None)),
        );
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(120)));
        });
        let key = |key: Key| {
            with_test_registry_mut(|registry| {
                bench_handle_mount_key_owned(registry, key, Modifiers::NONE)
            })
        };
        let char_key = |text: &str| Key::Character(text.to_string().into());
        let enter = || Key::Named(keyboard_key::Named::Enter);
        let right = || Key::Named(keyboard_key::Named::ArrowRight);
        let tab = || Key::Named(keyboard_key::Named::Tab);

        assert!(key(tab()).is_empty());
        let _ = key(right()); // cursor → (300, 201) text
        let _ = key(enter()); // the text draft (seed "material 000")
        let _ = key(char_key("X"));
        assert!(matches!(
            table_read(120, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Text(text))) if text == "material 000X"
        ));
        // The text cap: fill to the crate's cap (accepted), one more char
        // refuses with the machine's OWN counter and the draft stays.
        let fill = MAX_TEXT_BYTES - "material 000X".len();
        let _ = key(char_key(&"a".repeat(fill)));
        let accepted_len = table_read(120, 200, |machine| {
            machine.draft().map(|draft| match &draft.pending {
                WidgetCellValue::Text(text) => text.len(),
                _ => 0,
            })
        });
        assert_eq!(accepted_len, Some(Some(MAX_TEXT_BYTES)));
        let refusals_before =
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals);
        let _ = key(char_key("a"));
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals),
            refusals_before.map(|count| count + 1),
            "the machine's delegated value gate counted ONE refusal"
        );
        assert_eq!(
            table_read(120, 200, |machine| {
                machine.draft().map(|draft| match &draft.pending {
                    WidgetCellValue::Text(text) => text.len(),
                    _ => 0,
                })
            }),
            accepted_len,
            "a refused keystroke left the draft EXACTLY as-was"
        );
        // Cancel free, then the NUMBER cell.
        let _ = key(Key::Named(keyboard_key::Named::Escape));
        let _ = key(right()); // cursor → (300, 202) number
        let _ = key(enter()); // seeded Number(0.0)
        let _ = key(char_key("4"));
        let _ = key(char_key("2"));
        assert!(matches!(
            table_read(120, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Number(value))) if value == 42.0
        ));
        // A non-numeric character composes to an unparseable candidate:
        // refused BEFORE the machine (the mount's class-aware compose), the
        // pending stays exactly 42.0.
        let _ = key(char_key("x"));
        assert!(matches!(
            table_read(120, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Number(value))) if value == 42.0
        ));
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals),
            refusals_before.map(|count| count + 1),
            "the unparseable candidate never reached the machine"
        );
        // Backspace: pop → the pending follows the buffer.
        let _ = key(Key::Named(keyboard_key::Named::Backspace));
        assert!(matches!(
            table_read(120, 200, |machine| machine
                .draft()
                .map(|draft| draft.pending.clone())),
            Some(Some(WidgetCellValue::Number(value))) if value == 4.0
        ));
        let _ = key(char_key("2")); // back to 42

        // Enter COMMITS: exactly ONE fact leaves as the commit message...
        let commits = key(enter());
        assert_eq!(
            commits,
            vec![WidgetMountMsg::TableCellCommitted {
                surface_id: sid(120),
                table_id: sid(200),
                key: sid(300),
                column: sid(202),
                value: WidgetCellValue::Number(42.0),
            }]
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().cell_writes),
            Some(1),
            "the machine counted exactly ONE cell write"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().row_state_changes),
            Some(1),
            "ONE row touched (no selection writes happened in the scope)"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.in_flight().len()),
            Some(1),
            "the fact is parked in flight"
        );

        // ...and the bridge route forwards exactly ONE TableCellChanged,
        // then settles the address (the in-flight mark clears).
        let outcome = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &commits[0])
        });
        assert_eq!(
            outcome.events,
            vec![WidgetEvent::TableCellChanged {
                surface_id: sid(120),
                row_key: sid(300),
                column_id: sid(202),
                value: WidgetCellValue::Number(42.0),
            }]
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.in_flight().len()),
            Some(0),
            "the bridge settled the committed address"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false),
            "the draft cleared on commit"
        );
    }

    /// The NUMBER draft's minus law (the SCN-028 follow-up): a `-` keyed as
    /// the FIRST keystroke prepends the sign ('-' then digits commits a
    /// NEGATIVE — impossible under plain append-only composure, whose seed
    /// always carries the cell's digits); a `-` anywhere else is ignored
    /// BEFORE composing (the digits-then-'-' shape and a '-' on an
    /// already-negative seed leave the buffer and the machine's counters
    /// exactly as-was); and a SEEDED NEGATIVE draft ("-0.5") keeps
    /// composing digits and popping Backspace exactly as a positive one
    /// does.
    #[test]
    fn table_number_draft_minus_law() {
        let _app = OpenCADStudio::new_for_test();
        let key = |key: Key| {
            with_test_registry_mut(|registry| {
                bench_handle_mount_key_owned(registry, key, Modifiers::NONE)
            })
        };
        let char_key = |text: &str| Key::Character(text.to_string().into());
        let number_pending = || {
            table_read(120, 200, |machine| {
                machine.draft().map(|draft| match draft.pending.clone() {
                    WidgetCellValue::Number(value) => value,
                    other => panic!("a Number draft's pending, got {other:?}"),
                })
            })
        };
        let refusals = || {
            table_read(120, 200, |machine| machine.mutations().invalid_cell_refusals)
        };
        // Arm the NUMBER cell (300, 202) on a fresh registration: dive,
        // walk the cursor text → number, Enter arms the seeded draft. The
        // registration guard is RETURNED so the surface stays registered
        // for the leg's keystrokes (it unregisters on drop).
        let arm = |panel: &str, def: PanelTableDef| -> RegisteredGuard {
            let guard = register_test_surface(panel, demo_surface(def));
            with_test_registry_mut(|registry| {
                registry.set_mount_key_target_owned(Some(sid(120)));
            });
            let _ = key(Key::Named(keyboard_key::Named::Tab)); // dive
            let _ = key(Key::Named(keyboard_key::Named::ArrowRight)); // → 201
            let _ = key(Key::Named(keyboard_key::Named::ArrowRight)); // → 202
            let _ = key(Key::Named(keyboard_key::Named::Enter)); // arm
            guard
        };

        // Leg 1 — the SEEDED NEGATIVE draft ("-0.5") keeps composing:
        // digits append, a '-' past the first keystroke is ignored, and
        // Backspace pops the buffer (the pending follows it exactly).
        let mut def = demo_table_def(4, None);
        def.rows[0].cells[1] = TableCell::Number(-0.5);
        {
            let _leg = arm("scn042_minus_seeded", def);
            assert_eq!(number_pending(), Some(Some(-0.5)), "the negative seed");
            let _ = key(char_key("7")); // "-0.57"
            assert_eq!(number_pending(), Some(Some(-0.57)));
            let _ = key(char_key("-")); // composing began: ignored
            assert_eq!(number_pending(), Some(Some(-0.57)), "'-' elsewhere ignored");
            let _ = key(char_key("5")); // "-0.575"
            assert_eq!(number_pending(), Some(Some(-0.575)));
            let _ = key(Key::Named(keyboard_key::Named::Backspace)); // "-0.57"
            assert_eq!(number_pending(), Some(Some(-0.57)));
            let _ = key(Key::Named(keyboard_key::Named::Escape)); // free cancel
            assert_eq!(number_pending(), Some(None), "the draft cancelled free");
        }

        // Leg 2 — '-' FIRST then digits COMMITS A NEGATIVE.
        {
            let _leg = arm("scn042_minus_first", demo_table_def(4, None));
            let _ = key(char_key("-")); // seed "0" → "-0"
            assert_eq!(number_pending(), Some(Some(-0.0)), "the sign prepended");
            let _ = key(char_key("5")); // "-05" → -5
            assert_eq!(number_pending(), Some(Some(-5.0)));
            let commits = key(Key::Named(keyboard_key::Named::Enter));
            assert_eq!(
                commits,
                vec![WidgetMountMsg::TableCellCommitted {
                    surface_id: sid(120),
                    table_id: sid(200),
                    key: sid(300),
                    column: sid(202),
                    value: WidgetCellValue::Number(-5.0),
                }],
                "'-' first then digits committed the negative"
            );
        }

        // Leg 3 — digits then '-': the '-' is ignored BEFORE composing (no
        // refusal counted, no buffer movement), and a later Backspace
        // proves no sign ever entered the buffer.
        {
            let _leg = arm("scn042_minus_late", demo_table_def(4, None));
            let _ = key(char_key("4")); // "04"
            assert_eq!(number_pending(), Some(Some(4.0)));
            let refusals_before = refusals();
            let _ = key(char_key("-")); // ignored: composing began
            assert_eq!(number_pending(), Some(Some(4.0)), "the '-' landed nowhere");
            assert_eq!(refusals(), refusals_before, "ignored BEFORE the gates");
            let _ = key(char_key("2")); // "042"
            assert_eq!(number_pending(), Some(Some(42.0)));
            let _ = key(Key::Named(keyboard_key::Named::Backspace)); // "04"
            assert_eq!(
                number_pending(),
                Some(Some(4.0)),
                "no sign ever entered the buffer"
            );
        }
    }

    // -- 17e. the one-path discipline + the live arm --------------------------

    /// The typed-table vocabulary REFUSES at `apply_mount_msg` by name (the
    /// bridge's own route is `route_mount_table_msg_owned` — reaching the
    /// machine router is a bypass defect).
    #[test]
    fn table_vocabulary_refuses_at_the_machine_apply() {
        let surface = demo_surface(demo_table_def(2, None));
        let mut state = WidgetUiState::new(sid(120));
        state.sync(&surface);
        let mut dispatcher = WidgetDispatcher::new(sid(120));
        let mut sink = Vec::new();
        let routing = apply_mount_msg(
            &mut state,
            &mut dispatcher,
            &WidgetMountMsg::TableRowPressed {
                surface_id: sid(120),
                table_id: sid(200),
                key: sid(300),
                column: sid(201),
            },
            &mut sink,
        );
        assert!(
            matches!(&routing, MountRouting::Refused(reason) if reason.contains("route of its own")),
            "the bypass is refused by name: {routing:?}"
        );
        assert!(sink.is_empty(), "no event exists on a bypass refusal");
    }

    /// The END-TO-END keyboard commit through the LIVE arm: dive → cursor →
    /// arm → type → Enter; the commit message routes through
    /// `Message::WidgetMountMsg`, the drained `TableCellChanged` forwards
    /// through the ONE notify path exactly once, and the machine's fact
    /// settles.
    #[test]
    fn table_commit_forwards_once_through_the_live_arm() {
        let mut app = OpenCADStudio::new_for_test();
        let _guard = register_test_surface(
            "scn028_live",
            demo_surface(demo_table_def(4, None)),
        );
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(120)));
        });
        let forwards_before = app
            .mount_registry
            .borrow()
            .counters_owned()
            .mount_event_forwards;
        let press = |app: &mut OpenCADStudio, key: Key| {
            let _ = app.update(crate::app::Message::WidgetMountMsg(
                WidgetMountMsg::KeyPressed {
                    key,
                    modifiers: Modifiers::NONE,
                },
            ));
        };

        let _ = press(&mut app, Key::Named(keyboard_key::Named::Tab)); // dive
        let _ = press(&mut app, Key::Named(keyboard_key::Named::ArrowRight)); // cursor (300, 201)
        let _ = press(&mut app, Key::Named(keyboard_key::Named::Enter)); // arm the text draft
        let _ = press(&mut app, Key::Character("Z".to_string().into())); // "material 000Z"
        let _ = press(&mut app, Key::Named(keyboard_key::Named::Enter)); // commit

        let forwards = app
            .mount_registry
            .borrow()
            .counters_owned()
            .mount_event_forwards
            - forwards_before;
        assert_eq!(forwards, 1, "exactly ONE forwarded event row");
        assert_eq!(
            table_read(120, 200, |machine| machine.mutations().cell_writes),
            Some(1)
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.in_flight().len()),
            Some(0),
            "the live arm settled the committed fact"
        );
        assert_eq!(
            table_read(120, 200, |machine| machine.draft().is_some()),
            Some(false)
        );
    }

    // -- 17f. Escape's law + the machine lifecycle ----------------------------

    /// Escape with a table draft cancels it FREE (no counter moves, no
    /// event) and does NOT touch the armed prompt; the NEXT Escape (no
    /// draft) falls through to the existing surface-escape law and
    /// dismisses the prompt 1:1.
    #[test]
    fn table_escape_cancels_draft_free_then_prompt_law_holds() {
        let mut app = OpenCADStudio::new_for_test();
        let mut surface = demo_surface(demo_table_def(4, None));
        surface.roots.insert(
            0,
            WidgetNode::List(item_list(200, vec![item(7, "one")])),
        );
        let _guard = register_test_surface("scn028_escape", surface);
        with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(200)));
        });
        let handle = MountRegistry::handle_for_surface(sid(200)).expect("registered");
        let prompt_key = DispatchKey::Button(sid(1));
        handle
            .dispatcher
            .borrow_mut()
            .sync_button(&IconActionDef {
                confirmation: Some(ConfirmPolicy::Prompt {
                    message: "Really delete the selection?".to_string(),
                }),
                ..button_def(1, "Delete", "delete")
            });

        // Arm a prompt through the LIVE arm (selection + click — the
        // ordinary Message::WidgetMountMsg bridge).
        let _ = app.update(crate::app::Message::WidgetMountMsg(
            WidgetMountMsg::SelectionChanged {
                surface_id: sid(200),
                widget_id: sid(200),
                selected: vec![sid(7)],
            },
        ));
        let _ = app.update(crate::app::Message::WidgetMountMsg(
            WidgetMountMsg::ButtonClicked {
                surface_id: sid(200),
                widget_id: sid(1),
            },
        ));
        assert!(
            handle
                .dispatcher
                .borrow()
                .pending_prompt(&prompt_key)
                .is_some()
        );

        // Dive + arm a table draft.
        let key_fn = |key: Key| {
            with_test_registry_mut(|registry| {
                bench_handle_mount_key_owned(registry, key, Modifiers::NONE)
            })
        };
        let _ = key_fn(Key::Named(keyboard_key::Named::Tab)); // 7 (the ring's only row)
        let _ = key_fn(Key::Named(keyboard_key::Named::Tab)); // the edge → the dive
        assert!(scope_active(200));
        let _ = key_fn(Key::Named(keyboard_key::Named::Enter)); // the first-column draft
        assert_eq!(
            table_read(200, 200, |machine| machine.draft().is_some()),
            Some(true)
        );
        let mutations_before = table_read(200, 200, |machine| *machine.mutations());

        // ESCAPE #1: the DRAFT cancels (free); the armed prompt SURVIVES.
        assert!(key_fn(Key::Named(keyboard_key::Named::Escape)).is_empty());
        assert_eq!(
            table_read(200, 200, |machine| *machine.mutations()),
            mutations_before,
            "a cancelled draft touched NOTHING (no evidence it existed)"
        );
        assert!(
            handle
                .dispatcher
                .borrow()
                .pending_prompt(&prompt_key)
                .is_some(),
            "the armed prompt was not the draft's Escape's target"
        );

        // ESCAPE #2 (no draft): the existing surface-escape law — the armed
        // prompt dismisses 1:1.
        assert!(key_fn(Key::Named(keyboard_key::Named::Escape)).is_empty());
        assert_eq!(
            handle.dispatcher.borrow().pending_prompt(&prompt_key),
            None
        );
    }

    /// The machine lifecycle at the MOUNT level: selection/focus/draft
    /// survive a patch swap that does not touch their keys (the registry's
    /// sync point), a column-kind change discards the draft on the machine's
    /// dedicated counter, and the teardown RESETS + drops every slot (§16).
    #[test]
    fn table_state_survives_swap_schema_discard_and_teardown_resets() {
        let _app = OpenCADStudio::new_for_test();
        let guard = register_test_surface(
            "scn028_lifecycle",
            demo_surface(demo_table_def(4, None)),
        );
        let press = |key: u64| WidgetMountMsg::TableRowPressed {
            surface_id: sid(120),
            table_id: sid(200),
            key: sid(key),
            column: sid(201),
        };
        let _ = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(300))
        });
        let _ = with_test_registry_mut(|registry| {
            route_mount_table_msg_owned(registry, &press(300)) // activation: the draft
        });
        assert_eq!(
            table_read(120, 200, |machine| (
                machine.selection().to_vec(),
                machine.draft().is_some(),
                machine.mutations().whole_state_resets,
            )),
            Some((vec![sid(300)], true, 0))
        );

        // The patch swap: same keys, a changed VALUE on row 301 — the
        // machine state SURVIVES (the registry's sync point), no reset.
        let mut def2 = demo_table_def(4, None);
        if let TableCell::Text(text) = &mut def2.rows[1].cells[0] {
            *text = "changed".to_string();
        }
        let mut surface2 = demo_surface(def2);
        surface2.revision = SceneRevision(2);
        with_test_registry_mut(|registry| {
            registry.register_surface_owned(
                "scn028_lifecycle",
                SurfaceMountHandle::from_surface(sid(120), surface2),
            );
        });
        assert_eq!(
            table_read(120, 200, |machine| (
                machine.selection().to_vec(),
                machine.draft().is_some(),
                machine.mutations().whole_state_resets,
            )),
            Some((vec![sid(300)], true, 0)),
            "selection/focus/draft survive a patch that does not touch their keys"
        );

        // The schema change: column 202's kind moves under a fresh draft —
        // the draft is discarded on the machine's DEDICATED counter.
        let _ = with_test_registry_mut(|registry| {
            registry.set_mount_key_target_owned(Some(sid(120)))
        });
        let _ = with_test_registry_mut(|registry| {
            bench_handle_mount_key_owned(
                registry,
                Key::Named(keyboard_key::Named::Escape),
                Modifiers::NONE,
            )
        });
        with_test_registry_mut(|registry| {
            let machine = registry
                .table_machine_mut_owned(sid(120), sid(200))
                .expect("the machine");
            machine
                .begin_cell_edit(sid(300), sid(202))
                .expect("the number column arms");
        });
        let mut def3 = demo_table_def(4, None);
        def3.columns[1].kind = CellKind::Text;
        let mut surface3 = demo_surface(def3);
        surface3.revision = SceneRevision(3);
        with_test_registry_mut(|registry| {
            registry.register_surface_owned(
                "scn028_lifecycle",
                SurfaceMountHandle::from_surface(sid(120), surface3),
            );
        });
        assert_eq!(
            table_read(120, 200, |machine| (
                machine.draft().is_some(),
                machine.mutations().drafts_discarded_on_schema_change,
            )),
            Some((false, 1)),
            "the kind-changed draft was discarded on the DEDICATED counter"
        );

        // THE teardown (§16): the guard's drop resets + drops every slot —
        // nothing outlives its surface.
        drop(guard);
        assert!(table_read(120, 200, |machine| machine.selection().is_empty()).is_none());
        assert!(!scope_active(120));
        with_test_registry(|registry| {
            assert!(
                registry.table_state_owned(sid(120), sid(200)).is_none(),
                "the machine slot left with its surface"
            );
        });
    }

    // -- SCN-030: registration feeds the dispatcher's read model --------------

    /// THE REGRESSION PIN (SCN-030 — the live-firetest defect): a surface
    /// carrying an icon button (id 110, `ConfirmPolicy::Prompt` — the demo
    /// delete's shape) and a menu with a submenu registers, and the INVOKE
    /// route (`apply_mount_msg` — the exact path the MCP
    /// `panel_widget_invoke` rides) dispatches WITHOUT any manual
    /// `sync_button`/`sync_menu`: the targeted reads resolve (they returned
    /// `null` before the fix), the button click arms the prompt, and the
    /// dialog's confirm emits exactly ONE `CommandInvoked`.
    #[test]
    fn registration_feeds_the_dispatch_read_model_invoke_route() {
        let surface = surface(vec![
            WidgetNode::List(item_list(200, vec![item(7, "row")])),
            WidgetNode::IconButton(IconActionDef {
                confirmation: Some(ConfirmPolicy::Prompt {
                    message: "Really delete the selection?".to_string(),
                }),
                ..button_def(110, "Delete", "delete")
            }),
            WidgetNode::Menu(menu_def(400, vec![MenuItemDef {
                submenu: Some(Box::new(menu_def(401, vec![menu_item_def("inner", "Inner")]))),
                ..menu_item_def("outer", "Outer")
            }])),
        ]);
        let _guard = register_test_surface("scn030_invoke", surface);
        let handle = MountRegistry::handle_for_surface(sid(200)).expect("registered");
        // NO manual sync_* anywhere: REGISTRATION walked the surface's defs.

        // The targeted read (what `panel_semantics`' per-button read
        // serves): the registered button and every menu item resolve
        // effective enablement/visibility.
        {
            let dispatcher = handle.dispatcher.borrow();
            let button = DispatchKey::Button(sid(110));
            assert_eq!(dispatcher.effective_enabled(&button), Some(true));
            assert_eq!(dispatcher.effective_visible(&button), Some(true));
            let outer = DispatchKey::MenuItem(sid(400), "outer".to_string());
            let inner = DispatchKey::MenuItem(sid(401), "inner".to_string());
            assert_eq!(dispatcher.effective_enabled(&outer), Some(true));
            assert_eq!(dispatcher.effective_visible(&inner), Some(true));
        }

        // The invoke route: select (the command's targets), then activate.
        let route = |msg: WidgetMountMsg| {
            let mut sink = Vec::new();
            let routing = {
                let mut state = handle.state.borrow_mut();
                let mut dispatcher = handle.dispatcher.borrow_mut();
                apply_mount_msg(&mut state, &mut dispatcher, &msg, &mut sink)
            };
            (routing, sink)
        };
        let (routing, _events) = route(WidgetMountMsg::SelectionChanged {
            surface_id: sid(200),
            widget_id: sid(200),
            selected: vec![sid(7)],
        });
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });

        // The prompt-policy button: the click ARMS (the existing pin),
        // the dialog's confirm emits exactly ONE CommandInvoked.
        let (routing, events) = route(click_msg(200, 110));
        assert_eq!(
            routing,
            MountRouting::PromptRequested {
                prompt: "Really delete the selection?".to_string(),
            }
        );
        assert!(events.is_empty(), "no command event exists yet");
        let outcome = handle
            .dispatcher
            .borrow_mut()
            .confirm(DispatchKey::Button(sid(110)))
            .expect("the armed stage the dialog owns");
        assert_eq!(
            outcome,
            DispatchOutcome::ConfirmedEmitted(WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: "delete".to_string(),
                target_ids: vec![sid(7)],
            }),
            "exactly ONE CommandInvoked, without any manual sync"
        );
        assert!(handle.dispatcher.borrow_mut().settle(DispatchKey::Button(sid(110))));

        // The submenu item invokes on its OWNING submenu's id (the
        // dispatcher's addressing convention) — also fed by registration.
        let (routing, events) = route(WidgetMountMsg::MenuItemClicked {
            surface_id: sid(200),
            menu_id: sid(401),
            command_id: "inner".to_string(),
        });
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(
            events,
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: "inner".to_string(),
                target_ids: vec![sid(7)],
            }]
        );
    }

    /// The teardown-class REPLACE path (a DIFFERENT surface_id, SCN-033):
    /// surface B moves button 110's command AND policy — the whole handle
    /// (dispatcher included) swaps, and the read model reflects B (the
    /// fresh machine's own upserts), no stale A view; the outgoing
    /// dispatcher released with its surface.
    #[test]
    fn registration_replace_reflects_surface_b_no_stale_a_view() {
        let _guard = register_test_surface(
            "scn030_replace",
            surface(vec![
                WidgetNode::List(item_list(200, vec![item(7, "row")])),
                WidgetNode::IconButton(IconActionDef {
                    confirmation: Some(ConfirmPolicy::Prompt {
                        message: "Really delete the selection?".to_string(),
                    }),
                    ..button_def(110, "Delete", "delete")
                }),
            ]),
        );
        let old = MountRegistry::handle_for_surface(sid(200)).expect("registered");

        // The moved surface: a raised revision AND a moved root (the list
        // re-ids 200 → 210 — a different surface_id, the teardown-class
        // branch) where 110 is a plain, prompt-free "purge".
        let changed = WidgetSurface {
            revision: SceneRevision(2),
            ..surface(vec![
                WidgetNode::List(item_list(210, vec![item(7, "row")])),
                WidgetNode::IconButton(button_def(110, "Purge", "purge")),
            ])
        };
        let before = MountRegistry::counters();
        let _replaced = register_test_surface("scn030_replace", changed);
        let after = MountRegistry::counters();
        assert_eq!(
            after.registers_replaced - before.registers_replaced,
            1,
            "the moved surface replaced"
        );
        assert_eq!(
            after.torn_down - before.torn_down,
            1,
            "the teardown-class branch released the outgoing machines"
        );

        // The fresh dispatcher took the slot; the outgoing one released.
        let handle = MountRegistry::handle_for_surface(sid(210)).expect("registered");
        assert!(
            !Rc::ptr_eq(&handle.dispatcher, &old.dispatcher),
            "a FRESH dispatcher took the slot"
        );
        assert_eq!(
            old.dispatcher
                .borrow()
                .effective_enabled(&DispatchKey::Button(sid(110))),
            None,
            "the replaced dispatcher released — no stale address survives"
        );

        // The read model (and the invoke route) reflect B only.
        let route = |msg: WidgetMountMsg| {
            let mut sink = Vec::new();
            let routing = {
                let mut state = handle.state.borrow_mut();
                let mut dispatcher = handle.dispatcher.borrow_mut();
                apply_mount_msg(&mut state, &mut dispatcher, &msg, &mut sink)
            };
            (routing, sink)
        };
        let (routing, _) = route(WidgetMountMsg::SelectionChanged {
            surface_id: sid(210),
            widget_id: sid(210),
            selected: vec![sid(7)],
        });
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
        let (routing, events) = route(click_msg(210, 110));
        assert_eq!(
            routing,
            MountRouting::Applied { forwarded: 1 },
            "B has no prompt policy — a stale A view would have armed one"
        );
        assert_eq!(
            events,
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(210),
                command_id: "purge".to_string(),
                target_ids: vec![sid(7)],
            }],
            "the read model carries B's command, never A's"
        );
    }

    /// The PRESERVE path: an IDENTICAL re-register keeps the RUNNING
    /// dispatcher (the counters say "preserved"); the walk's re-upsert of
    /// the same defs is IDEMPOTENT — 110 still resolves and still invokes,
    /// never a duplicate or a refusal.
    #[test]
    fn registration_preserve_re_upserts_idempotently() {
        let fixture = || {
            surface(vec![
                WidgetNode::List(item_list(200, vec![item(7, "row")])),
                WidgetNode::IconButton(button_def(110, "Delete", "delete")),
            ])
        };
        let _guard = register_test_surface("scn030_preserve", fixture());
        let handle = MountRegistry::handle_for_surface(sid(200)).expect("registered");

        let before = MountRegistry::counters();
        let _fresh = register_test_surface("scn030_preserve", fixture());
        let after = MountRegistry::counters();
        assert_eq!(
            after.registers_preserved - before.registers_preserved,
            1,
            "the identical resurface preserved"
        );
        assert_eq!(
            after.registers_replaced - before.registers_replaced,
            0,
            "an identical resurface never replaces"
        );

        // The SAME dispatcher stayed registered — the walk re-synced it in
        // place (no swap, no reset).
        let still = MountRegistry::handle_for_surface(sid(200)).expect("registered");
        assert!(
            Rc::ptr_eq(&still.dispatcher, &handle.dispatcher),
            "the original dispatcher survived the identical resurface"
        );
        let key = DispatchKey::Button(sid(110));
        assert_eq!(handle.dispatcher.borrow().effective_enabled(&key), Some(true));
        assert_eq!(handle.dispatcher.borrow().effective_visible(&key), Some(true));

        // Idempotence, proven on the invoke route: no duplicate state, no
        // refusal — the click emits exactly once.
        let mut sink = Vec::new();
        let routing = {
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            apply_mount_msg(
                &mut state,
                &mut dispatcher,
                &WidgetMountMsg::SelectionChanged {
                    surface_id: sid(200),
                    widget_id: sid(200),
                    selected: vec![sid(7)],
                },
                &mut sink,
            )
        };
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
        sink.clear(); // the selection's own notify drained; the click is next
        let routing = {
            let mut state = handle.state.borrow_mut();
            let mut dispatcher = handle.dispatcher.borrow_mut();
            apply_mount_msg(&mut state, &mut dispatcher, &click_msg(200, 110), &mut sink)
        };
        assert_eq!(routing, MountRouting::Applied { forwarded: 1 });
        assert_eq!(
            sink,
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: "delete".to_string(),
                target_ids: vec![sid(7)],
            }],
            "the re-upserted address still invokes, exactly once"
        );
    }

    /// The §16 invisible law + the machine's own depth guard: a button
    /// under an INVISIBLE group is never synced (the walk skips invisible
    /// nodes whole), and a submenu deeper than [`MAX_MENU_DEPTH`] never
    /// registers (the dispatcher's own cap — the walk feeds, it never
    /// re-implements).
    #[test]
    fn registration_skips_invisible_subtrees_and_overdeep_menus() {
        // A submenu chain MAX_MENU_DEPTH + 1 menus deep; the deepest
        // menu's item sits past the machine's cap.
        let mut sub: Option<MenuDef> = None;
        for level in (1..=MAX_MENU_DEPTH + 1).rev() {
            let item = MenuItemDef {
                submenu: sub.take().map(Box::new),
                ..menu_item_def(&format!("c{level}"), &format!("level {level}"))
            };
            sub = Some(menu_def(400 + level as u64, vec![item]));
        }
        let deep_menu = sub.expect("the chain built");

        let surface = surface(vec![
            invisible(WidgetNode::Group(group_def(
                300,
                Axis::Vertical,
                None,
                vec![WidgetNode::IconButton(button_def(130, "Hidden", "hidden"))],
            ))),
            WidgetNode::IconButton(button_def(110, "Live", "live")),
            WidgetNode::Menu(deep_menu),
        ]);
        let _guard = register_test_surface("scn030_law", surface);
        let handle = MountRegistry::handle_for_surface(sid(300)).expect("registered");
        let dispatcher = handle.dispatcher.borrow();

        assert_eq!(
            dispatcher.effective_visible(&DispatchKey::Button(sid(110))),
            Some(true),
            "the visible button synced"
        );
        assert_eq!(
            dispatcher.effective_enabled(&DispatchKey::Button(sid(130))),
            None,
            "a button under an invisible group is NOT synced (§16)"
        );
        let deepest_synced = DispatchKey::MenuItem(
            sid(400 + MAX_MENU_DEPTH as u64),
            format!("c{MAX_MENU_DEPTH}"),
        );
        assert_eq!(
            dispatcher.effective_enabled(&deepest_synced),
            Some(true),
            "the level-MAX item registered"
        );
        let overdeep = DispatchKey::MenuItem(
            sid(400 + MAX_MENU_DEPTH as u64 + 1),
            format!("c{}", MAX_MENU_DEPTH + 1),
        );
        assert_eq!(
            dispatcher.effective_visible(&overdeep),
            None,
            "past the machine's own depth cap: never registered"
        );
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
    /// advances the revision and RE-REGISTERS the mount (a revision move
    /// is never shape-identical) — and because the FROZEN surface id
    /// survives, the re-register resolves as the SAME-SURFACE EVOLUTION
    /// (SCN-033: the machines CARRY, no release runs; a release would only
    /// run for a different surface id), with the report's capped facts
    /// matching the ops.
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
            // mount — the frozen surface id makes it the EVOLUTION arm
            // (the machines carry; a revision move is never
            // shape-identical).
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
            assert_eq!(
                after.torn_down - before.torn_down,
                0,
                "the frozen surface id makes the re-register an EVOLUTION: the machines carried, no release ran"
            );
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
    /// re-registers (the evolution carry — same frozen id, the machines
    /// survive, SCN-033).
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

            // (3) A greater revision (a resync snapshot): re-store +
            // re-register — the stable frozen id makes it the EVOLUTION
            // carry (the machines survive, SCN-033).
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

    /// Scene regions: a scene leg STORES with no frozen surface id yet;
    /// region pages resolve the pre-order subtree; a scene read NEVER
    /// demands (the host store is the whole scene truth) and a past-end
    /// window is the empty page. (The store itself still registers nothing
    /// into the mount map — the SCN-031 seam READS the leg at view time
    /// from `mounted_body_owned`; registration stays a widget-leg affair.)
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

    // -- SCN-031: the scene-mount render seam --------------------------------

    use crate::ui::window::plugin_scene::project_scene;
    use crate::ui::window::plugin_scene_cache::{DrawDecision, ProjectionCounters, RebuildReason};
    use ocs_plugin_api::scene::{AssetKind, SceneColor, ThemeColor};
    use ocs_plugin_api::scene_builder::{PatchBuilder, SceneBuilder, fixture_graph_scene};

    /// The demo-shape fixture: the crate's deterministic graph scene (the
    /// same generator the scene_graph demo's spirit runs on), published
    /// scene-leg-only — no widget leg, no registration.
    fn graph_scene(revision: u64) -> ocs_plugin_api::scene::PanelScene {
        fixture_graph_scene(revision, 4, 0x5C39)
    }

    /// The SCN-031b defect fixture: an ORIGIN-CENTERED two-card scene —
    /// negative AND positive coordinates, the reference demo graph's shape
    /// (which the 1:1 stage-1 mapping drew entirely off-region).
    fn origin_centered_leg(revision: u64) -> ocs_plugin_api::scene::PanelScene {
        ocs_plugin_api::scene::PanelScene {
            revision: SceneRevision(revision),
            elements: vec![
                SceneElement::Rect {
                    id: sid(1),
                    name: None,
                    actions: vec![],
                    drag: None,
                    pos: Vec2([-60.0, -40.0]),
                    size: Vec2([40.0, 20.0]),
                    fill: Some(SceneColor::Theme(ThemeColor::Normal)),
                    stroke: None,
                },
                SceneElement::Rect {
                    id: sid(2),
                    name: None,
                    actions: vec![],
                    drag: None,
                    pos: Vec2([20.0, 20.0]),
                    size: Vec2([40.0, 20.0]),
                    fill: Some(SceneColor::Theme(ThemeColor::Normal)),
                    stroke: None,
                },
            ],
        }
    }

    /// One coloured root rect (the patch payload; a fill so the projection
    /// actually emits a prim for it).
    fn scene_rect(id: u64) -> SceneElement {
        SceneElement::Rect {
            id: sid(id),
            name: None,
            actions: vec![],
            drag: None,
            pos: Vec2([500.0, 500.0]),
            size: Vec2([10.0, 10.0]),
            fill: Some(SceneColor::Theme(ThemeColor::Normal)),
            stroke: None,
        }
    }

    /// THE COMPOSITION PIN (scene-only): a panel whose store holds ONLY a
    /// scene leg — the demo's `scn_panel` shape, which SCN-025 recorded as
    /// "stored but invisible" — now mounts a full-body scene region. The
    /// gate answers `Some` where it answered `None` before, the shape is
    /// `SceneOnly`, and the region's cache slot is born at composition
    /// (with counters at their defaults: composition runs NO projection —
    /// the retained law starts at draw time).
    #[test]
    fn scene_only_leg_mounts_a_full_body_scene_region() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry_mut(|registry| {
            let outcome = registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(graph_scene(1)))
                .expect("the scene leg stores");
            assert!(outcome.widget_mount.is_none(), "scene-only: no widget mount");
            assert!(!registry.mount_registered_owned("scn_panel"));
        });
        // THE SEAM: the previously-invisible leg now mounts.
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
        assert_eq!(
            MountRegistry::body_shape("scn_panel"),
            Some(MountBodyShape::SceneOnly)
        );
        // The slot exists (born at composition) and NOTHING has drawn: no
        // projection ran at view time.
        let slot = MountRegistry::scene_cache_slot("scn_panel").expect("slot at composition");
        assert_eq!(slot.borrow().counters(), &ProjectionCounters::default());
    }

    /// THE COMPOSITION PIN (both legs): scene + widgets compose the fixed
    /// split — one body, both regions; the widget reads (frozen surface id)
    /// are untouched by the scene leg.
    #[test]
    fn scene_and_widgets_compose_both_regions() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry_mut(|registry| {
            attach_publish(
                registry,
                "PLUGIN",
                "scn_panel",
                simple_list_surface(SceneRevision(1), vec![item(7, "a")]),
            )
            .expect("the widget leg attaches");
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(graph_scene(1)))
                .expect("the scene leg stores");
        });
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
        assert_eq!(
            MountRegistry::body_shape("scn_panel"),
            Some(MountBodyShape::SceneAndWidgets)
        );
        // The widget leg's frozen identity rides as before.
        assert_eq!(
            with_test_registry(|r| r.panel_surface_id_owned("scn_panel")),
            Some(sid(200))
        );
        // And the region's slot is live.
        assert!(MountRegistry::scene_cache_slot("scn_panel").is_some());
    }

    /// THE COMPOSITION PIN (widgets-only): no scene leg ⇒ NO scene region —
    /// the body is the pre-seam widget stack (byte-identical by
    /// construction: the `None` scene element never enters the column) and
    /// NO cache slot is created (nothing registry-side to go stale).
    #[test]
    fn widgets_only_body_stays_widget_only_and_cache_free() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry_mut(|registry| {
            attach_publish(
                registry,
                "PLUGIN",
                "align_tools",
                simple_list_surface(SceneRevision(1), vec![item(7, "a")]),
            )
            .expect("attach");
        });
        assert!(MountRegistry::mounted_body("align_tools").is_some());
        assert_eq!(
            MountRegistry::body_shape("align_tools"),
            Some(MountBodyShape::WidgetsOnly)
        );
        assert!(MountRegistry::scene_cache_slot("align_tools").is_none());
    }

    /// THE LEGACY PIN (re-stated at the seam): neither leg ⇒ the legacy
    /// path, unchanged — zero publishes and zero registrations mean the
    /// mount read answers `None` exactly as it always has.
    #[test]
    fn zero_entries_still_take_the_legacy_path() {
        let _app = OpenCADStudio::new_for_test();
        assert_eq!(MountRegistry::body_shape("legacy_panel"), None);
        assert!(MountRegistry::mounted_body("legacy_panel").is_none());
    }

    /// THE RETAINED LAW, through the registry's slot: same key ⇒ replay
    /// (0 re-projections); a patch's revision bump ⇒ exactly the
    /// revision-classified rebuild; a palette switch ⇒ the palette rebuild
    /// (theme switches repaint); a resize ⇒ the bounds rebuild. The slot is
    /// ALSO the same `Rc` across view passes — the element tree rebuilds
    /// every frame, the retained cache does not.
    #[test]
    fn scene_region_replays_until_inputs_change() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry_mut(|registry| {
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(graph_scene(1)))
                .expect("the scene leg stores");
        });
        // First composition: the slot is born.
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
        let slot = MountRegistry::scene_cache_slot("scn_panel").expect("slot");

        let dark = iced::Theme::Dark;
        let light = iced::Theme::Light;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 180.0,
        };
        let resized = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 180.0,
        };

        // Two draws at the same revision/bounds/palette: one build, one
        // replay — ZERO re-projections (an unexpected full-scene rebuild
        // per frame is a DEFECT, the counters are its evidence).
        with_test_registry(|registry| {
            let scene = registry
                .panel_surface_store()
                .entry("scn_panel")
                .unwrap()
                .scene
                .as_ref()
                .unwrap()
                .clone();
            let mut cache = slot.borrow_mut();
            assert_eq!(
                cache.apply_build(&scene, dark.palette(), bounds),
                DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
            );
            assert_eq!(cache.apply_build(&scene, dark.palette(), bounds), DrawDecision::LayerHit);
            let counters = cache.counters();
            assert_eq!(counters.layer_builds, 1);
            assert_eq!(counters.probe_hits, 1);
            assert_eq!(counters.rebuilds_revision, 0);
            assert_eq!(counters.rebuilds_bounds, 0);
            assert_eq!(counters.rebuilds_palette, 0);
            assert!(counters.prims_last_build > 0, "the fixture projects prims");
            assert_eq!(counters.last_error, None);
        });

        // A second view pass hands the program the SAME cache — the
        // retained side survives the element-tree rebuild.
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
        let slot2 = MountRegistry::scene_cache_slot("scn_panel").expect("slot");
        assert!(Rc::ptr_eq(&slot, &slot2), "one slot per panel, kept");

        // A patch (the demo's own move: an Add op) bumps the revision; the
        // next draw is EXACTLY the revision-classified rebuild.
        let scene_patch = PatchBuilder::new(1)
            .add(None, scene_rect(9090))
            .finish(2)
            .expect("the patch builds");
        with_test_registry_mut(|registry| {
            let outcome = registry
                .panel_surface_store_mut()
                .patch("PLUGIN", "scn_panel", SceneOrWidgetPatch::Scene(scene_patch))
                .expect("the patch applies");
            assert_eq!(outcome.report.revision, SceneRevision(2));
        });
        with_test_registry(|registry| {
            let scene = registry
                .panel_surface_store()
                .entry("scn_panel")
                .unwrap()
                .scene
                .as_ref()
                .unwrap()
                .clone();
            let mut cache = slot.borrow_mut();
            assert_eq!(
                cache.apply_build(&scene, dark.palette(), bounds),
                DrawDecision::LayerRebuild(RebuildReason::RevisionChanged)
            );
            assert_eq!(cache.counters().rebuilds_revision, 1);
        });

        // A theme switch moves the palette fingerprint: the palette rebuild.
        with_test_registry(|registry| {
            let scene = registry
                .panel_surface_store()
                .entry("scn_panel")
                .unwrap()
                .scene
                .as_ref()
                .unwrap()
                .clone();
            let mut cache = slot.borrow_mut();
            assert_eq!(
                cache.apply_build(&scene, light.palette(), bounds),
                DrawDecision::LayerRebuild(RebuildReason::PaletteChanged)
            );
            assert_eq!(cache.counters().rebuilds_palette, 1);
        });

        // A resize re-projects at the new bounds (the grammar's clip
        // discipline applies at emit; the key change is the invalidation).
        with_test_registry(|registry| {
            let scene = registry
                .panel_surface_store()
                .entry("scn_panel")
                .unwrap()
                .scene
                .as_ref()
                .unwrap()
                .clone();
            let mut cache = slot.borrow_mut();
            assert_eq!(
                cache.apply_build(&scene, dark.palette(), resized),
                DrawDecision::LayerRebuild(RebuildReason::BoundsChanged)
            );
            assert_eq!(cache.counters().rebuilds_bounds, 1);
            // Totals: exactly four builds for exactly four key misses, one
            // replay in between — never more.
            let counters = cache.counters();
            assert_eq!(counters.layer_builds, 4);
            assert_eq!(counters.probe_hits, 1);
        });
    }

    /// THE SCN-031b SEAM PIN (the live defect, dead at the composition
    /// level): the defect fixture — an ORIGIN-CENTERED scene whose cards
    /// span NEGATIVE coordinates (the demo graph's shape, which the 1:1
    /// stage-1 mapping drew entirely off the top-left-origin region) —
    /// builds ONCE through the panel's cache slot and then replays: the
    /// fit-and-center transform the cache now applies on every build is a
    /// pure function of the existing key inputs, so it forces no extra
    /// re-projection (replay stays replay), and its invertible fit law is
    /// pinned in the cache module (`SceneFitTransform`).
    #[test]
    fn negative_coord_scene_builds_once_then_replays() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry_mut(|registry| {
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(origin_centered_leg(1)))
                .expect("the scene leg stores");
        });
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
        let slot = MountRegistry::scene_cache_slot("scn_panel").expect("slot");

        let dark = iced::Theme::Dark;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 180.0,
        };
        with_test_registry(|registry| {
            let scene = registry
                .panel_surface_store()
                .entry("scn_panel")
                .unwrap()
                .scene
                .as_ref()
                .unwrap()
                .clone();
            // The defect's precondition, pinned at the seam level: the
            // stored leg projects to negative coordinates.
            let prims = project_scene(&scene, dark.palette(), bounds.size())
                .expect("the leg is valid");
            let (min, _) =
                crate::ui::window::plugin_scene_cache::prims_bbox(&prims).expect("geometry");
            assert!(min.x < 0.0 && min.y < 0.0);

            // THE LAW: exactly one build, then replay — the transform
            // forced no re-projection.
            let mut cache = slot.borrow_mut();
            assert_eq!(
                cache.apply_build(&scene, dark.palette(), bounds),
                DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
            );
            assert_eq!(
                cache.apply_build(&scene, dark.palette(), bounds),
                DrawDecision::LayerHit
            );
            let counters = cache.counters();
            assert_eq!(counters.layer_builds, 1);
            assert_eq!(counters.probe_hits, 1);
            assert_eq!(counters.rebuilds_bounds, 0);
            assert_eq!(counters.rebuilds_palette, 0);
            assert_eq!(counters.last_error, None);
        });
    }

    /// THE LIFECYCLE PIN: the scene cache slot follows the surface
    /// lifecycle — the SCN-033 same-surface carry KEEPS it (the retained
    /// prims stay valid; the cache re-keys at draw), a teardown-class
    /// replace DROPS it, and the §16 teardown stays graceful (and slotless)
    /// afterwards.
    #[test]
    fn scene_cache_slot_follows_the_surface_lifecycle() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry_mut(|registry| {
            attach_publish(
                registry,
                "PLUGIN",
                "scn_panel",
                simple_list_surface(SceneRevision(1), vec![item(7, "a")]),
            )
            .expect("the widget leg attaches");
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(graph_scene(1)))
                .expect("the scene leg stores");
        });
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
        let slot = MountRegistry::scene_cache_slot("scn_panel").expect("slot");

        // (a) The CARRY: a widget patch re-registers the same frozen
        // surface id — the machines carry, and the scene slot survives
        // VERBATIM (same Rc).
        let widget_patch = SceneOrWidgetPatch::Widgets(WidgetSurfacePatch {
            base_revision: SceneRevision(1),
            revision: SceneRevision(2),
            ops: vec![WidgetOp::Add {
                parent: None,
                widget: WidgetNode::List(ItemListDef {
                    id: sid(300),
                    name: None,
                    enabled: true,
                    visible: true,
                    label: None,
                    items: vec![item(11, "b")],
                }),
            }],
        });
        with_test_registry_mut(|registry| {
            let outcome = registry
                .panel_surface_store_mut()
                .patch("PLUGIN", "scn_panel", widget_patch)
                .expect("the widget patch applies");
            if let Some((surface_id, surface)) = outcome.widget_mount {
                registry.register_surface_owned(
                    "scn_panel",
                    SurfaceMountHandle::from_surface(surface_id, surface),
                );
            }
        });
        let carried = MountRegistry::scene_cache_slot("scn_panel").expect("the carry keeps");
        assert!(Rc::ptr_eq(&slot, &carried), "the carry keeps the slot");

        // (b) The TEARDOWN-CLASS REPLACE: a genuinely new surface id starts
        // clean — the scene slot drops with it.
        with_test_registry_mut(|registry| {
            registry.register_surface_owned(
                "scn_panel",
                SurfaceMountHandle::from_surface(
                    sid(777),
                    surface(vec![WidgetNode::List(ItemListDef {
                        id: sid(777),
                        name: None,
                        enabled: true,
                        visible: true,
                        label: None,
                        items: vec![item(12, "c")],
                    })]),
                ),
            );
        });
        assert!(
            MountRegistry::scene_cache_slot("scn_panel").is_none(),
            "the teardown-class replace drops the slot"
        );

        // (c) The §16 teardown: graceful, and still slotless.
        with_test_registry_mut(|registry| registry.teardown_surface_owned("scn_panel"));
        assert!(MountRegistry::scene_cache_slot("scn_panel").is_none());
    }

    /// THE ASSETREF v1 LAW, through the seam: an asset element renders as
    /// its documented placeholder — the bounded FRAME outline at the
    /// asset's frame (accent stroke, one scene unit), NOT the asset's
    /// bytes. Exactly one stroke prim; the cache counts it as exactly one
    /// prim / one closed rect ring, so the placeholder is stable (a second
    /// identical draw replays — no re-projection).
    #[test]
    fn asset_element_projects_as_the_documented_frame_placeholder() {
        let _app = OpenCADStudio::new_for_test();
        let mut builder = SceneBuilder::new(1);
        builder
            .asset(
                5,
                AssetKind::Svg,
                "asset://fixture",
                Rect2D {
                    pos: Vec2([0.0, 0.0]),
                    size: Vec2([10.0, 10.0]),
                },
            )
            .expect("the asset registers");
        let scene = builder.finish().expect("the asset scene validates");
        with_test_registry_mut(|registry| {
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(scene.clone()))
                .expect("the scene leg stores");
        });
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
        let slot = MountRegistry::scene_cache_slot("scn_panel").expect("slot");

        let dark = iced::Theme::Dark;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 180.0,
        };
        // The placeholder prims, pinned at the projection (the doc'd FRAME
        // outline: the frame ring, the accent slot, one scene-unit width).
        let prims = project_scene(&scene, dark.palette(), bounds.size())
            .expect("the asset scene projects");
        assert_eq!(prims.len(), 1, "the placeholder IS the frame stroke");
        match prims.as_slice() {
            [crate::ui::window::plugin_scene::ScenePrim::Stroke {
                element,
                vertices,
                color,
                width,
            }] => {
                assert_eq!(*element, sid(5));
                assert_eq!(vertices.len(), 5, "a closed rect ring");
                assert_eq!(*color, dark.palette().primary.base.color, "the accent slot");
                assert!((*width - 1.0).abs() < f32::EPSILON, "one scene unit");
            }
            other => panic!("expected exactly the frame stroke, got {other:?}"),
        }
        // And the cache counts the same story through the seam's slot.
        let mut cache = slot.borrow_mut();
        assert_eq!(
            cache.apply_build(&scene, dark.palette(), bounds),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
        );
        assert_eq!(cache.apply_build(&scene, dark.palette(), bounds), DrawDecision::LayerHit);
        let counters = cache.counters();
        assert_eq!(counters.prims_last_build, 1);
        assert_eq!(counters.path_work_last_build, 5);
        assert_eq!(counters.layer_builds, 1, "the placeholder replays");
    }

    // -- SCN-031c: the scene region's LAYOUT pin -----------------------------

    /// THE LAYOUT PIN (SCN-031c): the scene region RESERVES its documented
    /// fixed height in the REAL composition. Not a declared-length read —
    /// the pin lays the exact live element chain out with iced's own layout
    /// (mounted body → pointer-capture shell → `kit::scroll_area` →
    /// `kit::palette` → the dock's `FillPortion` clip cell) through a
    /// headless renderer and measures the RESOLVED node tree:
    ///
    /// - the region (the body column's FIRST child) resolves to EXACTLY
    ///   [`SCENE_REGION_V1_HEIGHT`], and
    /// - the widget stack begins BELOW it (region height + the column
    ///   spacing) — never inside the region's band, and
    /// - both hold in a viewport TALLER and one SHORTER than the region —
    ///   the documented v1 law's reason: the panel scroll area lays its
    ///   content out with an UNBOUNDED main axis (`scrollable` passes
    ///   `f32::INFINITY` down; iced's `Limits::height(Fixed)` clamps a
    ///   fixed length only by the incoming max), so the fixed height is
    ///   never squeezed by the window.
    ///
    /// (The live "~90 px collapse" report this pin answers closed as a
    /// misread of the CORRECT fitted render: the demo graph is wide, so the
    /// fit is width-limited and its node cards — which read as "a widget
    /// stack" on a screenshot — sit sparse inside the intact 240 band. The
    /// layout math admits no collapse: verified through the vendored
    /// iced flex/limits sources and this resolved-layout measurement.)
    #[test]
    fn scene_region_reserves_its_fixed_height_in_the_live_chain() {
        use iced::advanced::layout::Limits;
        use iced::widget::Space;
        use iced::Size;

        let _app = OpenCADStudio::new_for_test();
        let _lock = crate::headless_ui_lock();
        with_test_registry_mut(|registry| {
            attach_publish(
                registry,
                "PLUGIN",
                "scn_panel",
                simple_list_surface(SceneRevision(1), vec![item(7, "a")]),
            )
            .expect("attach");
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(graph_scene(1)))
                .expect("the scene leg stores");
        });
        assert_eq!(
            MountRegistry::body_shape("scn_panel"),
            Some(MountBodyShape::SceneAndWidgets)
        );
        let surface_id = with_test_registry(|r| r.panel_surface_id_owned("scn_panel"));

        let renderer = iced_test::futures::futures::executor::block_on(
            <iced::Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
            ),
        )
        .expect("headless renderer");

        // The exact live chain, `plugin_panel::view` + the dock edge down
        // to the FillPortion cell.
        let composed = || -> Element<'static, crate::app::Message> {
            let body = MountRegistry::mounted_body("scn_panel").expect("the body composes");
            let body = pointer_capture_body(body, surface_id).map(app_message);
            let title_bar: Element<'static, crate::app::Message> = Space::new()
                .width(Length::Fixed(360.0))
                .height(Length::Fixed(crate::ui::style::kit_metrics::TITLE_H))
                .into();
            let panel = crate::ui::kit::palette(title_bar, crate::ui::kit::scroll_area(body), 360.0);
            let cell = iced::widget::container(panel)
                .height(Length::FillPortion(1))
                .clip(true);
            iced::widget::column![cell].height(Length::Fill).into()
        };

        // The region node: the FIRST child of exactly the region height,
        // found with its parent (the body column).
        fn find_region_with_parent<'a>(
            node: &'a iced::advanced::layout::Node,
        ) -> Option<(
            &'a iced::advanced::layout::Node,
            &'a iced::advanced::layout::Node,
        )> {
            let children = node.children();
            if let Some(first) = children.first() {
                if (first.bounds().size().height - SCENE_REGION_V1_HEIGHT).abs() < f32::EPSILON {
                    return Some((node, first));
                }
            }
            children.iter().find_map(find_region_with_parent)
        }

        for viewport_height in [600.0, 120.0] {
            let mut root = composed();
            let mut tree = iced::advanced::widget::Tree::empty();
            tree.diff(root.as_widget_mut());
            let node = root.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &Limits::new(Size::ZERO, Size::new(360.0, viewport_height)),
            );
            let (column, region) = find_region_with_parent(&node)
                .unwrap_or_else(|| panic!("the region node at viewport {viewport_height}"));
            let region_height = region.bounds().size().height;
            assert!(
                (region_height - SCENE_REGION_V1_HEIGHT).abs() < f32::EPSILON,
                "the region reserves exactly its fixed height, got {region_height}"
            );
            // The widget stack begins BELOW the region, never inside it.
            let stack = &column.children()[1..];
            assert!(
                !stack.is_empty(),
                "the scene+widgets split keeps the widget stack below"
            );
            for widget in stack {
                let top = widget.bounds().position().y;
                assert!(
                    top >= SCENE_REGION_V1_HEIGHT,
                    "widgets start below the region, got y={top}"
                );
            }
            let first_widget = stack[0].bounds().position().y;
            assert!(
                (first_widget - (SCENE_REGION_V1_HEIGHT + MOUNT_GROUP_SPACING)).abs() < f32::EPSILON,
                "the stack begins at region height + column spacing, got {first_widget}"
            );
        }
    }

    // -- SCN-037: the full-chain delivery pins --------------------------------
    //
    // The decision-level pins above drive the program seam directly with
    // bounds at the WINDOW ORIGIN, where iced's absolute cursor and the
    // region-local pixel coincide. The LIVE host never satisfies that: the
    // region's canvas sits at a nonzero window origin (dock pane + panel
    // chrome + body padding — (18, 35) in this composition) and, when the
    // panel is scrolled, under a nonzero scroll translation. These two pins
    // push a synthetic gesture through iced's REAL UserInterface over the
    // EXACT live chain (`plugin_panel::view` + the dock edge down to the
    // clip cell) and require the machine to engage — the headless twin of
    // the live acceptance.

    use crate::ui::style::kit_metrics as km;

    /// The region canvas's ABSOLUTE bounds in this composition at a 360 px
    /// pane: x = the body padding (10) + the band's left grid inset
    /// ([`km::GRID_INSET_L`]); y = the title bar + the palette's top inset
    /// band + the body padding (8) — verified against the Simulator's
    /// internal layout (the throwaway `layout()` passes answer in
    /// parent-relative node coordinates and must NOT be used for pointer
    /// geometry).
    fn region_canvas_origin() -> Point {
        Point::new(
            10.0 + km::GRID_INSET_L,
            km::TITLE_H + (km::TOP_INSET - km::TOOL_GAP) + 8.0,
        )
    }

    /// THE FULL-CHAIN PIN: a press→move→release on the draggable card
    /// through the real widget tree (pointer-capture shell → scroll area →
    /// palette → cell) ARMS the machine, goes InFlight on the first
    /// threshold-crossing move, and settles with EXACTLY ONE
    /// `CommandInvoked("drag_settle", [600])` — which the release route
    /// forwards exactly once. The gesture rides WINDOW-absolute cursors
    /// over a canvas at a NONZERO window origin: the SCN-037 defect's exact
    /// shape (pre-fix, the press routed NOTHING here too).
    #[test]
    fn full_live_chain_delivers_the_scene_drag_to_the_machine() {
        use iced::widget::Space;
        use iced::Size;

        attach_drag_panel();
        let surface_id = with_test_registry(|r| r.panel_surface_id_owned("scn_panel"))
            .expect("surface id");

        let composed = || -> Element<'static, crate::app::Message> {
            let body = MountRegistry::mounted_body("scn_panel").expect("the body composes");
            let body = pointer_capture_body(body, Some(surface_id)).map(app_message);
            let title_bar: Element<'static, crate::app::Message> = Space::new()
                .width(Length::Fixed(360.0))
                .height(Length::Fixed(km::TITLE_H))
                .into();
            let panel = crate::ui::kit::palette(title_bar, crate::ui::kit::scroll_area(body), 360.0);
            let cell = iced::widget::container(panel)
                .height(Length::FillPortion(1))
                .clip(true);
            iced::widget::column![cell].height(Length::Fill).into()
        };

        // The render the live host committed before the gesture: the fit at
        // the region's size (322 px wide — 360 minus the band insets and the
        // body padding — by the fixed 240 band height).
        let transform = committed_fit(iced::Rectangle::with_size(Size::new(322.0, 240.0)));
        let origin = region_canvas_origin();
        let card_at = |scene: Point| Point::new(origin.x + transform.apply(scene).x, origin.y + transform.apply(scene).y);

        let mut sim = iced_test::simulator::Simulator::with_size(
            iced_test::core::Settings::default(),
            Size::new(360.0, 600.0),
            composed(),
        );

        // A press OUTSIDE the region (over the title bar) routes nothing.
        sim.point_at(Point::new(origin.x + 100.0, origin.y - 10.0));
        sim.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Left,
        ))]);
        assert_eq!(
            with_test_registry(|r| r.scene_drag_slot_read_owned(sid(200)))
                .map(|s| s.borrow().session.phase().clone()),
            Some(DragPhase::Idle),
            "a press outside the region routes nothing"
        );

        // THE GESTURE: press the card's window pixel, cross the threshold,
        // release — the settle binds and the forward drains once.
        let press = card_at(Point::new(100.0, 64.0));
        sim.point_at(press);
        sim.simulate([
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: press }),
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        ]);
        assert_eq!(
            with_test_registry(|r| r.scene_drag_slot_read_owned(sid(200)))
                .map(|s| s.borrow().session.phase().clone()),
            Some(DragPhase::Armed),
            "the press reached the canvas and armed the card (SCN-037)"
        );

        let move_to = Point::new(press.x + 60.0, press.y);
        sim.point_at(move_to);
        sim.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: move_to,
        })]);
        assert_eq!(
            with_test_registry(|r| r.scene_drag_slot_read_owned(sid(200)))
                .map(|s| s.borrow().session.phase().clone()),
            Some(DragPhase::InFlight),
            "the window-coordinate move crossed the threshold"
        );

        sim.point_at(move_to);
        sim.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        ))]);
        let pending = with_test_registry(|r| r.scene_drag_slot_read_owned(sid(200)))
            .map(|s| s.borrow().pending_events.clone())
            .unwrap();
        assert_eq!(
            pending,
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: SCENE_DRAG_SETTLE_COMMAND.to_string(),
                target_ids: vec![sid(600)],
            }],
            "the settle parked EXACTLY ONE CommandInvoked"
        );

        // The release route drains + forwards exactly once.
        let forwards_before = MountRegistry::counters().mount_event_forwards;
        MountRegistry::mount_pointer_released(sid(200));
        assert_eq!(
            MountRegistry::counters().mount_event_forwards,
            forwards_before + 1,
            "exactly one forwarded row"
        );
        assert!(with_test_registry(|r| r.scene_drag_slot_read_owned(sid(200)))
            .map(|s| s.borrow().pending_events.is_empty())
            .unwrap());
    }

    /// THE FULL-CHAIN PIN, BELOW THE FOLD: the widget stack taller than the
    /// pane, the scrollable at a NONZERO translation (the live demo panel's
    /// resting state — its Menu/List rows sit below the fold), the scene
    /// band's top cut under the chrome, the card still visible — and the
    /// drag at the card's VISIBLE window pixel still arms, lives and
    /// settles. This is the coordinator's live repro case: the scroll
    /// translation rides INSIDE the cursor iced hands the canvas (the
    /// scrollable's own content mapping), so the region-local read stays
    /// exact with the fold anywhere.
    #[test]
    fn full_live_chain_below_the_fold_still_delivers_the_scene_drag() {
        use iced::widget::Space;
        use iced::Size;

        attach_drag_panel();
        // Make the body TALLER than the viewport: a wide widget stack (many
        // list rows) — the live demo body is below the fold. The scene leg
        // (and its drag slot) carry over (the same-surface swap law).
        with_test_registry_mut(|registry| {
            let outcome = registry
                .panel_surface_store_mut()
                .publish(
                    "PLUGIN",
                    "scn_panel",
                    simple_list_surface(
                        SceneRevision(2),
                        (0..60u64).map(|i| item(1000 + i, "row")).collect(),
                    ),
                )
                .expect("the taller widget leg swaps in");
            if let Some((surface_id, attach_surface)) = outcome.widget_mount.as_ref() {
                registry.register_surface_owned(
                    "scn_panel",
                    SurfaceMountHandle::from_surface(*surface_id, attach_surface.clone()),
                );
            }
        });
        let surface_id = with_test_registry(|r| r.panel_surface_id_owned("scn_panel"))
            .expect("surface id");

        let composed = || -> Element<'static, crate::app::Message> {
            let body = MountRegistry::mounted_body("scn_panel").expect("the body composes");
            let body = pointer_capture_body(body, Some(surface_id)).map(app_message);
            let title_bar: Element<'static, crate::app::Message> = Space::new()
                .width(Length::Fixed(360.0))
                .height(Length::Fixed(km::TITLE_H))
                .into();
            let panel = crate::ui::kit::palette(title_bar, crate::ui::kit::scroll_area(body), 360.0);
            let cell = iced::widget::container(panel)
                .height(Length::FillPortion(1))
                .clip(true);
            iced::widget::column![cell].height(Length::Fill).into()
        };

        // The retained fit at the region's size (the render the host
        // committed before the gesture — the fold does not change it: the
        // layer key's bounds are the band's own).
        let transform = committed_fit(iced::Rectangle::with_size(Size::new(322.0, 240.0)));
        let origin = region_canvas_origin();

        let mut sim = iced_test::simulator::Simulator::with_size(
            iced_test::core::Settings::default(),
            Size::new(360.0, 300.0), // SHORT viewport: the stack is below the fold
            composed(),
        );

        // SCROLL DOWN one wheel line = 60 px (iced's Lines delta × 60): the
        // scene band's top cuts off, the card (pixel y ≈ 83 in the band)
        // stays visible, the translation is NONZERO.
        sim.point_at(Point::new(origin.x + 150.0, 250.0));
        sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        })]);
        let translation = 60.0_f32;

        // PRESS + DRAG + RELEASE at the card's VISIBLE window pixel.
        let press = Point::new(
            origin.x + transform.apply(Point::new(100.0, 64.0)).x,
            origin.y + transform.apply(Point::new(100.0, 64.0)).y - translation,
        );
        sim.point_at(press);
        sim.simulate([
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: press }),
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        ]);
        assert_eq!(
            with_test_registry(|r| r.scene_drag_slot_read_owned(sid(200)))
                .map(|s| s.borrow().session.phase().clone()),
            Some(DragPhase::Armed),
            "the card armed below the fold (SCN-037: pre-fix this press routed nothing)"
        );

        let move_to = Point::new(press.x + 60.0, press.y + 10.0);
        sim.point_at(move_to);
        sim.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: move_to,
        })]);
        assert_eq!(
            with_test_registry(|r| r.scene_drag_slot_read_owned(sid(200)))
                .map(|s| s.borrow().session.phase().clone()),
            Some(DragPhase::InFlight),
            "the drag is live under the scroll translation"
        );

        sim.point_at(move_to);
        sim.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        ))]);
        assert_eq!(
            with_test_registry(|r| r.scene_drag_slot_read_owned(sid(200)))
                .map(|s| s.borrow().pending_events.clone())
                .unwrap(),
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: SCENE_DRAG_SETTLE_COMMAND.to_string(),
                target_ids: vec![sid(600)],
            }],
            "the settle parked EXACTLY ONE CommandInvoked below the fold"
        );
    }


    /// THE LAYOUT PIN (scene-only edition): the region IS the body — the
    /// padded body container's single child resolves to exactly
    /// [`SCENE_REGION_V1_HEIGHT`] (nothing else competes for the band).
    #[test]
    fn scene_only_region_is_the_whole_body_at_the_fixed_height() {
        use iced::advanced::layout::Limits;
        use iced::widget::Space;
        use iced::Size;

        let _app = OpenCADStudio::new_for_test();
        let _lock = crate::headless_ui_lock();
        with_test_registry_mut(|registry| {
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(graph_scene(1)))
                .expect("the scene leg stores");
        });
        assert_eq!(
            MountRegistry::body_shape("scn_panel"),
            Some(MountBodyShape::SceneOnly)
        );

        let renderer = iced_test::futures::futures::executor::block_on(
            <iced::Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
            ),
        )
        .expect("headless renderer");

        let body = MountRegistry::mounted_body("scn_panel").expect("the body composes");
        let body: Element<'static, crate::app::Message> = body.map(app_message);
        let title_bar: Element<'static, crate::app::Message> = Space::new()
            .width(Length::Fixed(360.0))
            .height(Length::Fixed(crate::ui::style::kit_metrics::TITLE_H))
            .into();
        let panel = crate::ui::kit::palette(title_bar, crate::ui::kit::scroll_area(body), 360.0);
        let mut root: Element<'static, crate::app::Message> =
            iced::widget::column![iced::widget::container(panel)
                .height(Length::FillPortion(1))
                .clip(true)]
            .height(Length::Fill)
            .into();

        let mut tree = iced::advanced::widget::Tree::empty();
        tree.diff(root.as_widget_mut());
        let node = root.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &Limits::new(Size::ZERO, Size::new(360.0, 600.0)),
        );

        fn region_nodes(node: &iced::advanced::layout::Node, out: &mut Vec<f32>) {
            for child in node.children() {
                let height = child.bounds().size().height;
                if (height - SCENE_REGION_V1_HEIGHT).abs() < f32::EPSILON {
                    out.push(height);
                }
                region_nodes(child, out);
            }
        }
        let mut found = Vec::new();
        region_nodes(&node, &mut found);
        assert_eq!(
            found,
            vec![SCENE_REGION_V1_HEIGHT],
            "the scene-only body holds exactly one region, at the fixed height"
        );
    }

    // -- SCN-032: scene hit-testing + drag interactivity ----------------------

    use crate::ui::window::plugin_scene_cache::{
        SceneFitTransform, SCENE_FIT_MARGIN, fit_scene_prims,
    };
    // The canvas program trait (the update seam the synthetic events drive).
    use iced::widget::canvas::Program as _;

    /// The card's scene rect (the demo's shape: a 120x48 accent card).
    const CARD: ([f64; 2], [f64; 2]) = ([40.0, 40.0], [120.0, 48.0]);

    fn drag_meta() -> DragMeta {
        DragMeta {
            opaque: Some("scn-drag:node-1".to_string()),
        }
    }

    /// The drag fixture: a non-draggable BACKDROP, the DRAGGABLE card
    /// (id 600 — the demo's element id), and a smaller NON-draggable patch
    /// painted ON TOP of the card (document order = paint order, so the
    /// topmost law is observable: a press at the patch's center must NOT
    /// begin the card beneath it).
    fn drag_scene(revision: u64) -> ocs_plugin_api::scene::PanelScene {
        let rect = |id: u64, pos: [f64; 2], size: [f64; 2], drag: Option<DragMeta>| {
            SceneElement::Rect {
                id: sid(id),
                name: None,
                actions: vec![],
                drag,
                pos: Vec2(pos),
                size: Vec2(size),
                fill: Some(ocs_plugin_api::scene::SceneColor::Theme(
                    ocs_plugin_api::scene::ThemeColor::Normal,
                )),
                stroke: None,
            }
        };
        ocs_plugin_api::scene::PanelScene {
            revision: SceneRevision(revision),
            elements: vec![
                rect(100, [0.0, 0.0], [300.0, 200.0], None),
                rect(600, CARD.0, CARD.1, Some(drag_meta())),
                rect(101, [45.0, 45.0], [20.0, 20.0], None),
            ],
        }
    }

    /// The both-legs panel (the demo's shape): widget surface (surface id
    /// 200 by the first-root convention) + the drag scene leg. Composition
    /// is what births the slots (cache + drag) and syncs the draggables.
    fn attach_drag_panel() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry_mut(|registry| {
            attach_publish(
                registry,
                "PLUGIN",
                "scn_panel",
                simple_list_surface(SceneRevision(1), vec![item(7, "a")]),
            )
            .expect("the widget leg attaches");
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(drag_scene(1)))
                .expect("the scene leg stores");
        });
        // One composition pass: both slots are born, the card is synced.
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
    }

    /// The committed fit: run the cache's headless build over the STORE's
    /// live scene at the region bounds (the same call the renderer's draw
    /// funnels through) and read the retained transform back — the custody
    /// law's test-side twin: the test maps pixels through EXACTLY what the
    /// render committed.
    fn committed_fit(bounds: iced::Rectangle) -> SceneFitTransform {
        let cache = MountRegistry::scene_cache_slot("scn_panel").expect("cache slot");
        let scene = with_test_registry(|registry| {
            registry
                .panel_surface_store()
                .entry("scn_panel")
                .unwrap()
                .scene
                .clone()
                .expect("the scene leg")
        });
        let decision = cache
            .borrow_mut()
            .apply_build(&scene, &iced::Theme::Dark.palette(), bounds);
        assert!(
            matches!(decision, crate::ui::window::plugin_scene_cache::DrawDecision::LayerRebuild(_)),
            "the first build commits"
        );
        let transform = cache.borrow().last_fit_transform();
        // The custody is the pure fit of the projected stream (not a
        // recomputation with different values).
        let expected = fit_scene_prims(
            &crate::ui::window::plugin_scene::project_scene(
                &drag_scene(1),
                &iced::Theme::Dark.palette(),
                bounds.size(),
            )
            .expect("valid"),
            bounds.size(),
            SCENE_FIT_MARGIN,
        );
        assert_eq!(transform, expected, "the retained fit IS the committed fit");
        transform
    }

    /// The composed region program over the fixture (the child test module
    /// sees the private fields): the same construction
    /// `scene_region_element_owned` runs, driven by synthetic events.
    fn drag_program() -> (SceneRegionProgram, Rc<RefCell<SceneDragSlot>>) {
        let scene = with_test_registry(|registry| {
            registry
                .panel_surface_store()
                .entry("scn_panel")
                .unwrap()
                .scene
                .clone()
                .expect("the scene leg")
        });
        let slot = with_test_registry(|registry| {
            registry
                .scene_drag_slot_read_owned(sid(200))
                .expect("the drag slot is born at composition")
        });
        (
            SceneRegionProgram {
                scene: Rc::new(scene),
                cache: MountRegistry::scene_cache_slot("scn_panel").expect("cache slot"),
                drag: Some(Rc::clone(&slot)),
            },
            slot,
        )
    }

    /// One synthetic pointer event through the program's REAL update seam.
    fn pointer_event(
        program: &SceneRegionProgram,
        event: iced::mouse::Event,
        cursor_at: Point,
        bounds: iced::Rectangle,
    ) {
        let _ = program.update(
            &mut (),
            &iced::Event::Mouse(event),
            bounds,
            iced::mouse::Cursor::Available(cursor_at),
        );
    }

    /// The region-local pixel a SCENE point paints at (the transform's
    /// apply — the tests compute presses the way the renderer paints).
    fn region_pixel(transform: &SceneFitTransform, scene: Point) -> Point {
        transform.apply(scene)
    }

    /// The machine counter snapshot (the drag machine's own §16 probes).
    fn drag_counters(slot: &Rc<RefCell<SceneDragSlot>>) -> crate::ui::window::plugin_drag::DragCounters {
        slot.borrow().session.counters()
    }

    /// THE INTERACTION PIN: a DragMeta-carrying card fitted into region
    /// bounds — a synthetic press at the card's REGION pixel (computed via
    /// the transform's apply) begins the drag; moves cross the machine's
    /// threshold and park latest-wins with one host-local preview per
    /// delivery; the release settles with EXACTLY ONE
    /// `CommandInvoked("drag_settle", [600])`, which the release route
    /// drains and forwards exactly once.
    #[test]
    fn scene_drag_press_move_release_settles_with_exactly_one_command() {
        attach_drag_panel();
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 240.0,
        };
        let transform = committed_fit(bounds);
        let (program, slot) = drag_program();

        // PRESS at the card's center pixel (scene (100, 64) — inside the
        // card, off the overlapping non-draggable patch).
        let press = region_pixel(&transform, Point::new(100.0, 64.0));
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            press,
            bounds,
        );
        {
            let slot = slot.borrow();
            assert_eq!(slot.session.phase(), DragPhase::Armed, "the card armed");
            assert_eq!(slot.session.counters().begins, 1);
        }

        // MOVES: each crosses the machine's threshold in scene units (the
        // machine's frame — the points it is fed ARE the scene points).
        for step in 1..=3u32 {
            let local = region_pixel(
                &transform,
                Point::new(100.0 + 60.0 * step as f32, 64.0),
            );
            pointer_event(
                &program,
                iced::mouse::Event::CursorMoved { position: local },
                local,
                bounds,
            );
        }
        {
            let slot = slot.borrow();
            assert_eq!(slot.session.phase(), DragPhase::InFlight, "live");
            let counters = slot.session.counters();
            assert_eq!(counters.moves_parked, 3, "the crossing move + two more");
            assert_eq!(counters.moves_delivered, 3, "≤1 in flight, delivered per tick");
            assert_eq!(counters.previews_emitted, 3, "one preview per delivered move");
            assert_eq!(counters.settles, 0);
            assert_eq!(counters.cancels, 0);
        }

        // RELEASE at a final position: the settle converts to EXACTLY ONE
        // event, parked for the forward drain.
        let release = region_pixel(&transform, Point::new(280.0, 64.0));
        pointer_event(
            &program,
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            release,
            bounds,
        );
        let pending = slot.borrow().pending_events.clone();
        assert_eq!(
            pending,
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: SCENE_DRAG_SETTLE_COMMAND.to_string(),
                target_ids: vec![sid(600)],
            }],
            "ONE CommandInvoked, bound to the card's address, the settle command"
        );
        {
            let slot = slot.borrow();
            assert_eq!(slot.session.phase(), DragPhase::Settled);
            let counters = slot.session.counters();
            assert_eq!(counters.settles, 1);
            assert_eq!(counters.cancels, 0, "1:1: the settle was the one completion");
            // The end context is the newest delivered point (the settle
            // drains the parked slot first — the machine's own law; the
            // release argument only stands in when nothing was ever fed).
        }

        // THE FORWARD: the release route drains + forwards exactly once.
        let forwards_before = MountRegistry::counters().mount_event_forwards;
        MountRegistry::mount_pointer_released(sid(200));
        assert_eq!(
            MountRegistry::counters().mount_event_forwards,
            forwards_before + 1,
            "exactly one forwarded row (the settle command)"
        );
        assert!(
            slot.borrow().pending_events.is_empty(),
            "the drain emptied the slot (exactly-once: a re-drain forwards nothing)"
        );
        MountRegistry::mount_pointer_released(sid(200));
        assert_eq!(
            MountRegistry::counters().mount_event_forwards,
            forwards_before + 1,
            "the second release forwards nothing"
        );
    }

    /// THE SCN-037 REGRESSION PIN (the live defect, decision level): the
    /// SAME press→move→release gesture as the interaction pin above, but
    /// with the region's REAL geometry — its canvas at a NONZERO window
    /// origin (the docked pane + panel chrome + body padding put it at
    /// (18, 35) in the live host; any offset exposes it) and the pointer
    /// cursors in WINDOW coordinates. The defect: the program read the
    /// cursor through `position_over` — iced answers the WINDOW-ABSOLUTE
    /// point there — and inverted the fit in the WRONG frame, so the mapped
    /// scene point landed off the card and every press routed NOTHING
    /// (synthetic AND manual, live-verified on host 50b8bc40a). The
    /// origin-bounds pins above could never see it: at the window origin
    /// absolute == local. The fix under pin: the REGION-LOCAL read
    /// (`position_in`), which is the bounds-relative pixel the custody law
    /// (invert through the render's committed transform) requires.
    #[test]
    fn scene_pointer_events_map_through_the_region_origin() {
        attach_drag_panel();
        // The live-shape offset: the region's canvas does NOT sit at the
        // window origin.
        let bounds = iced::Rectangle {
            x: 200.0,
            y: 120.0,
            width: 240.0,
            height: 240.0,
        };
        let transform = committed_fit(iced::Rectangle::with_size(bounds.size()));
        let (program, slot) = drag_program();

        // A press OUTSIDE the region routes nothing even at the offset
        // geometry (the guard: the bounds are really where we think).
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            Point::new(bounds.x - 6.0, bounds.y + 20.0),
            bounds,
        );
        assert_eq!(slot.borrow().session.phase(), DragPhase::Idle, "outside");

        // PRESS at the card's WINDOW pixel (origin + the card's region
        // pixel): the program must map it BACK through the origin before
        // inverting the fit.
        let press = Point::new(
            bounds.x + transform.apply(Point::new(100.0, 64.0)).x,
            bounds.y + transform.apply(Point::new(100.0, 64.0)).y,
        );
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            press,
            bounds,
        );
        {
            let slot = slot.borrow();
            assert_eq!(
                slot.session.phase(),
                DragPhase::Armed,
                "the card armed through the region origin (SCN-037)"
            );
            assert_eq!(slot.session.counters().begins, 1);
        }

        // MOVES in window coordinates cross the threshold and park (the
        // same scene-space targets as the interaction pin, offset by the
        // region origin).
        for step in 1..=3u32 {
            let pixel = transform.apply(Point::new(100.0 + 60.0 * step as f32, 64.0));
            let local = Point::new(bounds.x + pixel.x, bounds.y + pixel.y);
            pointer_event(
                &program,
                iced::mouse::Event::CursorMoved { position: local },
                local,
                bounds,
            );
        }
        {
            let slot = slot.borrow();
            assert_eq!(slot.session.phase(), DragPhase::InFlight, "live");
            assert_eq!(slot.session.counters().moves_parked, 3);
        }

        // RELEASE in window coordinates: the settle binds EXACTLY ONE
        // CommandInvoked — the settle point mapped through the SAME frame.
        let release = Point::new(
            bounds.x + transform.apply(Point::new(280.0, 64.0)).x,
            bounds.y + transform.apply(Point::new(280.0, 64.0)).y,
        );
        pointer_event(
            &program,
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            release,
            bounds,
        );
        assert_eq!(
            slot.borrow().pending_events.clone(),
            vec![WidgetEvent::CommandInvoked {
                surface_id: sid(200),
                command_id: SCENE_DRAG_SETTLE_COMMAND.to_string(),
                target_ids: vec![sid(600)],
            }],
            "ONE CommandInvoked through the region origin"
        );
    }

    /// THE TRANSFORM CUSTODY, live: the hit-test and the pointer mapping go
    /// through the SAME retained transform the render committed —
    /// `invert(apply(p))` round-trips on the fixture, and a press mapped
    /// through the fit hits the element the user actually sees (the card's
    /// pixel press from the interaction pin began THE CARD).
    #[test]
    fn scene_hit_test_uses_the_render_transform_custody() {
        attach_drag_panel();
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 240.0,
        };
        let transform = committed_fit(bounds);
        for scene_point in [
            Point::new(100.0, 64.0),
            Point::new(40.0, 40.0),
            Point::new(159.0, 87.0),
        ] {
            let pixel = region_pixel(&transform, scene_point);
            let back = transform.invert(pixel);
            assert!(
                (back.x - scene_point.x).abs() < 1.0e-2 && (back.y - scene_point.y).abs() < 1.0e-2,
                "the custody round trip drifted for {scene_point:?}"
            );
        }
        // And the mapping is the render's, not the identity: the card's
        // scene origin does NOT sit at the region origin (the fit centered
        // the 300x200 scene into the 240x240 region).
        let card_pixel = region_pixel(&transform, Point::new(40.0, 40.0));
        assert!(
            card_pixel.x > 4.0 && card_pixel.y > 4.0,
            "the fit translated the scene: the card paints away from the origin ({card_pixel:?})"
        );
    }

    /// THE BOUNDARY PINS: a press on EMPTY scene area routes nothing; a
    /// press on a DRAGMETA-LESS element routes nothing (no Begin, no
    /// refusal counter — the boundary is the hit walk's, not the machine's);
    /// and the TOPMOST element wins: the non-draggable patch painted over
    /// the card blocks the drag where it covers it, while the card's own
    /// exposed area still arms.
    #[test]
    fn scene_press_on_empty_or_non_draggable_area_routes_nothing() {
        attach_drag_panel();
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 240.0,
        };
        let transform = committed_fit(bounds);
        let (program, slot) = drag_program();

        // (a) Empty area: the fitted scene leaves a margin — a pixel ABOVE
        // the fitted bbox maps to a scene point off every element (the fit
        // centered the 300x200 scene vertically, so the top band is empty).
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            Point::new(120.0, 20.0),
            bounds,
        );
        assert_eq!(slot.borrow().session.phase(), DragPhase::Idle, "nothing armed");

        // (b) TOPMOST wins: the patch (id 101) paints OVER the card; its
        // center press hits the PATCH, which carries no DragMeta → no Begin.
        let patch_center = region_pixel(&transform, Point::new(55.0, 55.0));
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            patch_center,
            bounds,
        );
        {
            let slot = slot.borrow();
            assert_eq!(slot.session.phase(), DragPhase::Idle, "the topmost element wins");
            assert_eq!(
                slot.session.counters().refusals_not_draggable, 0,
                "the boundary is the hit walk's: the machine was never asked"
            );
        }

        // (c) The card's own exposed area still arms (the same gesture, one
        // candidate over).
        let card_pixel = region_pixel(&transform, Point::new(120.0, 70.0));
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            card_pixel,
            bounds,
        );
        {
            let slot = slot.borrow();
            assert_eq!(slot.session.phase(), DragPhase::Armed, "the card armed");
            assert_eq!(slot.session.counters().begins, 1);
        }
    }

    /// THE LOSSLESS COMPLETIONS: Escape (machine-direct, the keyboard seam)
    /// and point-out (the move that leaves the region) each cancel their own
    /// begun interaction 1:1 — NO event, no forward, counters 1:1.
    #[test]
    fn scene_drag_esc_and_point_out_cancel_lossless() {
        attach_drag_panel();
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 240.0,
        };
        let transform = committed_fit(bounds);
        let forwards_before = MountRegistry::counters().mount_event_forwards;

        // (a) ESCAPE: press + crossing move, then the keyboard seam's
        // machine-direct cancel (the press also captured the surface — the
        // keyboard target the Escape route resolves).
        {
            let (program, slot) = drag_program();
            let press = region_pixel(&transform, Point::new(100.0, 64.0));
            pointer_event(
                &program,
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
                press,
                bounds,
            );
            let moved = region_pixel(&transform, Point::new(160.0, 64.0));
            pointer_event(
                &program,
                iced::mouse::Event::CursorMoved { position: moved },
                moved,
                bounds,
            );
            assert_eq!(slot.borrow().session.phase(), DragPhase::InFlight);
            assert!(MountRegistry::mount_press(sid(200)), "the capture armed the keys");
            with_test_registry_mut(|registry| {
                let msgs = bench_handle_mount_key_owned(
                    registry,
                    Key::Named(keyboard_key::Named::Escape),
                    Modifiers::NONE,
                );
                assert!(msgs.is_empty(), "the cancel is machine-direct: no messages");
            });
            {
                let slot = slot.borrow();
                assert_eq!(slot.session.phase(), DragPhase::Cancelled);
                let counters = slot.session.counters();
                assert_eq!(counters.begins, 1);
                assert_eq!(counters.cancels, 1, "1:1");
                assert_eq!(counters.settles, 0, "cancel is the no-event exit");
            }
            assert!(slot.borrow().pending_events.is_empty(), "no event parked");
            MountRegistry::mount_pointer_released(sid(200));
        }
        assert_eq!(
            MountRegistry::counters().mount_event_forwards,
            forwards_before,
            "the cancel forwarded nothing"
        );

        // (b) POINT-OUT: press + crossing move, then a move whose cursor is
        // OUTSIDE the region — the PointerOut cancel, lossless again. The
        // slot's machine ACCUMULATES its §16 counters across interactions
        // (the session is the surface's rail, not per gesture): (b)'s
        // assertions are deltas over (a)'s settled state (begins 1, cancels
        // 1 after (a)).
        {
            let begins_before = with_test_registry(|registry| {
                registry
                    .scene_drag_slot_read_owned(sid(200))
                    .map(|slot| slot.borrow().session.counters().begins)
                    .unwrap_or(0)
            });
            let cancels_before = with_test_registry(|registry| {
                registry
                    .scene_drag_slot_read_owned(sid(200))
                    .map(|slot| slot.borrow().session.counters().cancels)
                    .unwrap_or(0)
            });
            let (program, slot) = drag_program();
            let press = region_pixel(&transform, Point::new(100.0, 64.0));
            pointer_event(
                &program,
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
                press,
                bounds,
            );
            let moved = region_pixel(&transform, Point::new(160.0, 64.0));
            pointer_event(
                &program,
                iced::mouse::Event::CursorMoved { position: moved },
                moved,
                bounds,
            );
            // The pointer leaves: the cursor is far outside the bounds.
            pointer_event(
                &program,
                iced::mouse::Event::CursorMoved { position: Point::new(5_000.0, 5_000.0) },
                Point::new(5_000.0, 5_000.0),
                bounds,
            );
            {
                let slot = slot.borrow();
                assert_eq!(slot.session.phase(), DragPhase::Cancelled);
                let counters = slot.session.counters();
                assert_eq!(counters.begins, begins_before + 1, "1:1 per interaction");
                assert_eq!(counters.cancels, cancels_before + 1);
                assert_eq!(counters.settles, 0);
            }
            assert!(slot.borrow().pending_events.is_empty());
            MountRegistry::mount_pointer_released(sid(200));
        }
        assert_eq!(
            MountRegistry::counters().mount_event_forwards,
            forwards_before,
            "point-out forwarded nothing either"
        );
    }

    /// THE ARM-THRESHOLD law through the real seam: an ARMED release (no
    /// crossing move) is a CLICK — `cancel(ThresholdNotMet)`, never a
    /// settle, never an event.
    #[test]
    fn scene_armed_release_is_a_click_not_a_settle() {
        attach_drag_panel();
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 240.0,
        };
        let transform = committed_fit(bounds);
        let (program, slot) = drag_program();

        let press = region_pixel(&transform, Point::new(100.0, 64.0));
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            press,
            bounds,
        );
        // A sub-threshold jitter: one region pixel ≪ the 3-scene-unit
        // threshold at the fixture's fit scale (< 1).
        let jitter = Point::new(press.x + 1.0, press.y);
        pointer_event(
            &program,
            iced::mouse::Event::CursorMoved { position: jitter },
            jitter,
            bounds,
        );
        assert_eq!(slot.borrow().session.phase(), DragPhase::Armed, "still armed");
        pointer_event(
            &program,
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            jitter,
            bounds,
        );
        {
            let slot = slot.borrow();
            assert_eq!(slot.session.phase(), DragPhase::Cancelled);
            let counters = slot.session.counters();
            assert_eq!(counters.settles, 0, "a click is not a drag");
            assert_eq!(counters.cancels, 1);
            assert_eq!(counters.moves_parked, 0, "the jitter parked nothing");
        }
        assert!(slot.borrow().pending_events.is_empty());
        let forwards_before = MountRegistry::counters().mount_event_forwards;
        MountRegistry::mount_pointer_released(sid(200));
        assert_eq!(
            MountRegistry::counters().mount_event_forwards,
            forwards_before,
            "a click forwards nothing"
        );
    }

    /// THE COALESCING LAW through the real seam: a storm of moves parks
    /// latest-wins into the machine's single slot; every delivery is the
    /// then-newest point; the settle's end context is the LAST position —
    /// nothing between the first and last ever survives.
    #[test]
    fn scene_drag_move_storm_latest_wins_never_drops_the_last() {
        attach_drag_panel();
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 240.0,
        };
        let transform = committed_fit(bounds);
        let (program, slot) = drag_program();

        let press = region_pixel(&transform, Point::new(100.0, 64.0));
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            press,
            bounds,
        );
        let last_scene = Point::new(100.0 + 4.0 * 30.0, 64.0);
        for step in 1..=30u32 {
            let scene_point = Point::new(100.0 + 4.0 * step as f32, 64.0);
            let local = region_pixel(&transform, scene_point);
            pointer_event(
                &program,
                iced::mouse::Event::CursorMoved { position: local },
                local,
                bounds,
            );
            // At most ONE move in flight: every delivery was released
            // synchronously (the host-local law), so the machine never
            // holds a delivery across moves.
            let counters = drag_counters(&slot);
            assert!(counters.moves_delivered <= counters.moves_parked);
            assert_eq!(counters.previews_emitted, counters.moves_delivered);
        }
        let release = region_pixel(&transform, last_scene);
        pointer_event(
            &program,
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            release,
            bounds,
        );
        {
            let slot = slot.borrow();
            assert_eq!(slot.session.counters().moves_parked, 30, "every move parked");
            assert_eq!(slot.session.counters().previews_emitted, 30);
            assert_eq!(slot.session.counters().settles, 1);
        }
        // The parked event names the card (the content of the coalescing is
        // the machine's; the slot's ONE event is this test's content pin).
        let pending = slot.borrow().pending_events.clone();
        assert_eq!(pending.len(), 1);
    }

    /// THE LIFECYCLE: the slot is born at composition, pruned with the
    /// surface (a live drag completes as `cancel(Teardown)` — lossless),
    /// and a WIDGETS-ONLY panel never grows one (the release route's drain
    /// is a no-op there — zero legacy bytes).
    #[test]
    fn scene_drag_slot_follows_the_surface_teardown_and_widgets_only_grows_none() {
        attach_drag_panel();
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 240.0,
        };
        let transform = committed_fit(bounds);
        let (program, slot) = drag_program();
        let press = region_pixel(&transform, Point::new(100.0, 64.0));
        pointer_event(
            &program,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            press,
            bounds,
        );
        let moved = region_pixel(&transform, Point::new(160.0, 64.0));
        pointer_event(
            &program,
            iced::mouse::Event::CursorMoved { position: moved },
            moved,
            bounds,
        );
        assert_eq!(slot.borrow().session.phase(), DragPhase::InFlight);

        // §16 teardown: the machine completes losslessly, the slot leaves.
        MountRegistry::teardown_surface("scn_panel");
        assert!(
            with_test_registry(|registry| registry.scene_drag_slot_read_owned(sid(200)).is_none()),
            "the slot died with the surface"
        );

        // A widgets-only panel: composition grows NO drag slot and the
        // release route is a quiet no-op (the legacy shape).
        with_test_registry_mut(|registry| {
            attach_publish(
                registry,
                "PLUGIN",
                "plain_panel",
                simple_list_surface(SceneRevision(1), vec![item(9, "b")]),
            )
            .expect("attach");
        });
        assert!(MountRegistry::mounted_body("plain_panel").is_some());
        let plain_id = MountRegistry::panel_surface_id("plain_panel").expect("frozen");
        assert!(
            with_test_registry(|registry| registry.scene_drag_slot_read_owned(plain_id).is_none()),
            "no scene leg, no drag slot"
        );
        MountRegistry::mount_pointer_released(plain_id);
    }

    /// THE FENCE, scene-only edition: a scene leg WITHOUT a registered
    /// widget surface composes the region but grows NO drag slot — the
    /// pointer fence stands there (no surface id to route by; the demo's
    /// panel carries both legs, so this is the recorded boundary, not the
    /// demo's path).
    #[test]
    fn scene_only_region_keeps_the_pointer_fence() {
        let _app = OpenCADStudio::new_for_test();
        with_test_registry_mut(|registry| {
            registry
                .panel_surface_store_mut()
                .publish("PLUGIN", "scn_panel", SceneOrWidgetSurface::Scene(drag_scene(1)))
                .expect("the scene-only leg stores");
        });
        assert_eq!(
            MountRegistry::body_shape("scn_panel"),
            Some(MountBodyShape::SceneOnly)
        );
        assert!(MountRegistry::mounted_body("scn_panel").is_some());
        // No registered surface → no slot: the composed program's `drag` is
        // None and the update seam consumes nothing (fence by construction).
        assert!(
            with_test_registry(|registry| {
                registry.scene_drag_slot_read_owned(sid(600)).is_none()
                    && registry.scene_drag_slot_read_owned(sid(0)).is_none()
            }),
            "a scene-only region grows no drag slot"
        );
    }

}
