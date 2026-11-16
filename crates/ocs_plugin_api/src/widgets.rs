//! Versioned semantic-widget contracts for Open CAD Studio plugin panels
//! (additive, pre-pin; the `SWG-001` package of the scene-UI architecture).
//!
//! Family 1 of the binding addendum (`ARCHITECTURE.md` §§12–16, ADR-005):
//! retained *semantic* widgets — Tree, List, IconButton, Menu, Table-ref, and
//! a minimal `Group` composition container — for standard application-shell
//! UI, deliberately kept beside (never inside) the scene grammar of
//! [`crate::scene`]. The plugin owns the semantic widget data; the host owns
//! native rendering, focus/keyboard, a11y, pointer handling, visible-range
//! windowing, and transient interaction state. The layer mirrors the scene
//! vocabulary on purpose: the same identity type ([`StableId`]), the same
//! revision type ([`SceneRevision`]), the same two-phase in-place patch
//! scheme, and the same bounded-payload discipline. A panel carries ONE
//! [`WidgetSurface`] alongside its [`crate::scene::PanelScene`], sharing the
//! panel's single [`SceneRevision`] space so widgets and scenes advance at
//! the same cadence (ADR-005: the host hosts both on the same panel "at the
//! same revision cadence").
//!
//! Wire rule: new variants MUST be appended at the end of the enums so
//! existing plugins keep their bincode discriminant indices. The wire grows
//! append-only the same way dialogs, panels, and scenes do. The enums in
//! this module are pinned **from creation** (discriminant-pin tests at the
//! bottom of this file): [`WidgetNode`] `0..=5`, [`WidgetOp`] `0..=3`,
//! [`WidgetEvent`] `0..=6`, [`WidgetCellValue`] `0..=2`, [`Axis`] `0..=1`,
//! [`CellKind`] `0..=7`, [`BadgeTone`] `0..=4`, [`TableCell`] `0..=7`, and
//! [`ConfirmPolicy`] (one variant today). [`ConfirmPolicy`] in
//! particular is a single version now (`Prompt`); future policies are
//! appended, never rewritten.
//!
//! # Composition (ADR-005 discipline)
//!
//! [`WidgetNode::Group`] is the ONLY composition container: a host layout
//! capability exposing `Vertical`/`Horizontal` child stacking plus an
//! optional `split_ratio` (a `Group` with a `split_ratio` is the documented
//! *SplitView* — the host divides the box at the ratio; no general layout
//! engine exists at this tier, and the host is free to clamp or ignore a
//! nonsensical ratio rather than the data contract redefining float
//! validation for layout hints). The addendum's reference Column/Row/SplitView
//! layout composes on top of exactly this container. A scene pane mounts at
//! the HOST dock as the addendum documents — there is deliberately NO scene
//! variant on [`WidgetNode`] ("widget as a scene element" variants are
//! forbidden by ADR-005); where the reference layout shows a Scene cell, the
//! plugin states an empty `Group` placeholder and the host mounts both
//! surfaces side by side.
//!
//! # Identity (invariant WGT-ID)
//!
//! One surface-wide [`StableId`] namespace spans widget nodes, tree rows, and
//! list items ([`WidgetSurface::validate`] enforces uniqueness across the
//! union). A domain object's persistent identity is DISTINCT from its
//! presentation node id: a tree row's [`StableId`] is a presentation id and
//! the row's [`DomainRef`] (when the plugin declares one) — and nothing else
//! — names the domain object. There is no implicit conflation; menu items are
//! command-addressed (`command_id: String`) and carry no `StableId`, and
//! [`TableRef::table_id`] references the typed-table contract (`SWG-004`,
//! foreign table registry) so it is deliberately NOT part of the namespace:
//! several table refs may name one backing table.
//!
//! # Budgets
//!
//! All structural budgets mirror the scene machinery ([`MAX_SCENE_NODES`],
//! [`MAX_SCENE_DEPTH`], [`MAX_PATCH_OPS`], [`MAX_SUMMARY_ROOT_IDS`] — reused
//! verbatim, never re-declared) plus the widget-side payload caps documented
//! under their constants. Budget errors report the FIRST CROSSING point
//! (cap + 1), matching the scene and dialog decoders. Deserialization is
//! bounded exactly like the scene/dialog carriers: a hostile frame past the
//! depth/node budget refuses at decode before recursing into the stack.
//!
//! # Typed tables (`SWG-004`)
//!
//! [`WidgetNode::Table`] is a REFERENCE ([`TableRef::table_id`]) into the
//! typed-table contract this section defines; the registry itself rides the
//! surface's [`WidgetSurface::tables`] map, keyed by table id. Table ids are
//! a DIFFERENT identity space from the surface-wide widget namespace
//! (invariant WGT-ID, foreign registry): a table id colliding with a widget
//! id is meaningless by construction, several [`TableRef`]s may name ONE
//! backing table, and a table id resolves exactly once (the map's key) —
//! [`WidgetError::DuplicateWidgetId`] never fires for table ids.
//!
//! PLACEHOLDER TRANSITION (sole-owner resolution, recorded): `SWG-001`
//! shipped [`TableRef`] into a contract that did not exist yet, and its
//! creation surfaces (unmodifiable by the append-only test rule) carry
//! placeholder refs over an EMPTY registry. Resolution therefore ENGAGES
//! when the registry is non-empty and is resolve-check-free while it is
//! empty (the pre-`SWG-004` placeholder era, in both the snapshot gate and
//! the patch planner). A PUBLISHING surface (one registry entry or more)
//! is fully registry-contracted: every ref on it must resolve
//! ([`WidgetError::UnknownTableRef`] otherwise).
//!
//! ## Legacy string tables (the gate)
//!
//! `DialogControl::Table` / `DialogValue::Table` (`crate::dialog`, the
//! string-cell grids) stay BYTE-COMPATIBLE and untouched; the typed table is
//! a NEW capability layered BESIDE them — never a replacement of the legacy
//! control in place. The one-way bridge is tests-only: this module's types
//! never import `dialog`.
//!
//! ## Events vs patches (the documented split)
//!
//! [`WidgetEvent::TableCellChanged`] keeps the THREE-variant
//! [`WidgetCellValue`] skeleton — it is the read/notify shape (MCP read-back
//! summarizes through it; its pins must not break). The FULLY typed fact
//! travels as PATCH DATA: [`WidgetOp::TableCell`] names the
//! (table, row key, column) address and carries the skeleton value, and the
//! column's [`CellKind`] resolves the typed [`TableCell`] written. Only the
//! scalar kinds (`text`/`number`/`bool`) are patch-writable through the
//! skeleton — icon/link/badge/progress/date cells are plugin-PUBLISHED
//! through the registry (a snapshot carries the defs) and a patch naming
//! one refuses with [`WidgetError::InvalidCell`]. "One cell edit transfers
//! only its cell-level event/patch" is the ARCHITECTURE §16 acceptance this
//! design enforces byte-wise (see the size-A vs size-B tests).
//!
//! ## Paging and state classes
//!
//! A def states `total_rows: Some(total)` when it windows; `rows` carries
//! the published window (≤ [`MAX_WIDGET_ROWS_INITIAL`] per payload) and
//! [`WidgetEvent::TablePageRequested`] — the tree's `ChildrenRequested`
//! analog — asks for the next keyed window. The def carries NO selection/
//! scroll/expansion state: transient interaction state is HOST-LOCAL
//! (ARCHITECTURE §3, state class 3). Row-mode rows also matter for budgets:
//! the published window is a payload vec, exactly like tree rows.
//!
//! ## Wire notes (⚠ read before freezing)
//!
//! The registry is a TRAILING struct field with `#[serde(default)]`:
//! JSON payloads without the key decode with an empty registry, while a
//! BINCODE 1 struct decode reads EVERY field — an old-shape bincode frame
//! would end before this field and fail with `UnexpectedEof` (the exact
//! trailing-field hazard the host resolves via `deserialize_selectable_custom`
//! for `PluginClassInstance::selectable_custom`). PRE-PIN CLEARANCE:
//! `WidgetSurface` was created by `SWG-001` inside this same unreleased
//! series — no frozen wire ever carried the 2-field shape, so no old
//! payload exists to decode; the literal old-frame TLV decode test becomes
//! MANDATORY at the ADR-004 wire freeze (API-24; the crossing originally
//! froze as API-23 and was re-versioned 24 at the SCN-026 upstream merge
//! — upstream's own v23 = `TableOperation::LayoutSetTabOrder`), when a
//! shipped shape exists to snapshot. Until then the JSON-absence +
//! bincode/JSON roundtrip tests gate the trailing field, and this comment
//! is the WARN record.
//!
//! ## Deferred mirror (SCN-012 open question)
//!
//! The scene's `apply_patch_report` mirror for widget/table writes is
//! deliberately NOT part of this package (an unpublished half-done wire
//! surface is prohibited). SCN-012's recorded open question stays open
//! HERE on purpose: the mirror lands with the ADR-004 wire/stream work,
//! when tables cross IPC and the host mount consumes cell facts (SCN-013's
//! workers). Patch audit today: bounded bytes + validate facts.

use crate::scene::{
    MAX_ASSET_URI_BYTES, MAX_PATCH_OPS, MAX_SCENE_DEPTH, MAX_SCENE_NODES, MAX_SUMMARY_ROOT_IDS,
    MAX_TEXT_BYTES, SceneRevision, StableId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

// ---------------------------------------------------------------------------
// Widget-side budgets (documented caps)
// ---------------------------------------------------------------------------

/// Maximum tree rows / list items / menu items accepted in ONE widget
/// payload (the initial snapshot or one patch payload). Rows/items beyond
/// the cap are windowed: the plugin publishes the visible window and opens
/// the rest through lazy rows (`TreeRowDef::lazy_children` +
/// [`WidgetEvent::ChildrenRequested`]) and visible-range window reads — per
/// the addendum's "100k backing objects do not create 100k host widgets".
///
/// Budget errors carry the offending payload's row count; the per-surface
/// total of widget NODES is bounded separately by [`MAX_SCENE_NODES`].
pub const MAX_WIDGET_ROWS_INITIAL: usize = 4_096;

/// Maximum non-group widget nodes accepted per panel surface ([`WidgetNode`]
/// variants other than [`WidgetNode::Group`]: the actual interactive controls
/// a host materializes per panel). Mirrors `MAX_QUAD_PANELS` (512) by
/// magnitude — the same per-panel instance scale the quad decoder tolerates.
/// Group nodes are layout containers and count only against the shared node
/// budget [`MAX_SCENE_NODES`], so this cap does not conflict with it.
pub const MAX_WIDGETS_PER_PANEL: usize = 512;

/// Maximum `len` of one [`WidgetEvent::ChildrenRequested`] page. Window
/// reads cover visible ranges; a page this wide already exceeds any sane
/// viewport, so the decode refuses and the producer is defecting.
pub const MAX_WIDGET_RANGE_LEN: usize = 4_096;

/// Maximum `selected_ids` carried by one [`WidgetEvent::SelectionChanged`].
/// Selection sets are a UI-magnitude phenomenon; a set past this cap comes
/// from a defect, not a user gesture.
pub const MAX_SELECTION_IDS: usize = 256;

/// Maximum menu nesting depth ([`MenuDef`] <- `MenuItemDef::submenu` <- …).
/// Mirrors the [`MAX_SCENE_DEPTH`] discipline on the semantic vocabulary:
/// menus deeper than this are hostile or authoring defects.
pub const MAX_MENU_DEPTH: usize = 8;

/// Maximum `target_ids` carried by one [`WidgetEvent::CommandInvoked`].
/// Matches the crate's `MAX_PANEL_ACTION_TARGETS` magnitude: a command with
/// more targets than this should be a domain query, not one event.
pub const MAX_WIDGET_TARGET_IDS: usize = 16;

fn default_true() -> bool {
    true
}

/// Default icon size for [`IconRef`] (`16.0` logical units), used by serde
/// when the field is absent on the wire.
fn default_icon_size() -> f32 {
    16.0
}

/// serde default for [`WidgetSurface::tables`]: a payload without the
/// trailing registry field decodes as an empty (no-tables) surface — the
/// JSON-side old-shape tolerance of the SWG-004 trailing field (see the
/// [wire notes](self)). Empty is also the common non-table surface: most
/// panels publish no tables at all.
fn empty_tables() -> BTreeMap<StableId, PanelTableDef> {
    BTreeMap::new()
}

// ---------------------------------------------------------------------------
// Bounded decode (the dialog/scene thread-local budget pattern)
// ---------------------------------------------------------------------------

/// Per-thread widget decode budget. `depth` is the current recursion depth
/// (0 outside a decode); `nodes` counts total widget nodes / ops / events
/// decoded within the current frame (re-zeroed by [`widget_decode_frame_begin`]
/// at each frame boundary). Mirrors `SCENE_DECODE_BUDGET` in
/// [`crate::scene`] with the same caps ([`MAX_SCENE_DEPTH`],
/// [`MAX_SCENE_NODES`]) so widget frames are exactly as bounded as scene
/// frames; rows/items use their own per-vec caps instead of this counter, so
/// the decoded node count here matches what `WidgetSurface::validate` counts.
#[derive(Clone, Copy)]
struct WidgetDecodeBudget {
    depth: usize,
    nodes: usize,
}

thread_local! {
    static WIDGET_DECODE_BUDGET: std::cell::Cell<WidgetDecodeBudget> =
        const { std::cell::Cell::new(WidgetDecodeBudget { depth: 0, nodes: 0 }) };
}

/// Reset the per-thread widget decode budget at a frame boundary: call this
/// right before decoding one whole widget event frame so the budget applies
/// per frame rather than accumulating across frames.
///
/// This mirrors `dialog_decode_frame_begin` / `scene_decode_frame_begin` and
/// stays behind the same gate: the production callers (the future widget
/// event transport, ADR-004 wire deferral) are later tasks, so the
/// feature-less data-contract build only materializes the helper for tests.
#[cfg(any(feature = "host", test))]
#[allow(dead_code)] // wired into the widget event transport by a follow-up task
pub(crate) fn widget_decode_frame_begin() {
    WIDGET_DECODE_BUDGET.with(|budget| {
        budget.set(WidgetDecodeBudget { depth: 0, nodes: 0 });
    });
}

/// Enter one widget node during a decode. Returns `Err` when the payload
/// exceeds the nesting or node budget; the error aborts the decode before
/// the next level of recursion can consume stack.
fn widget_decode_enter() -> Result<(), String> {
    WIDGET_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.depth += 1;
        budget.nodes += 1;
        WIDGET_DECODE_BUDGET.set(budget);
        if budget.depth > MAX_SCENE_DEPTH {
            return Err(format!(
                "widget nesting exceeds the decoded depth budget ({MAX_SCENE_DEPTH})"
            ));
        }
        if budget.nodes > MAX_SCENE_NODES {
            return Err(format!(
                "widget frame exceeds the decoded node budget ({MAX_SCENE_NODES})"
            ));
        }
        Ok(())
    })
}

/// Leave one widget node (restores the depth budget even on decode failure).
fn widget_decode_exit() {
    WIDGET_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.depth = budget.depth.saturating_sub(1);
        WIDGET_DECODE_BUDGET.set(budget);
    });
}

struct WidgetDecodeGuard;

impl Drop for WidgetDecodeGuard {
    fn drop(&mut self) {
        widget_decode_exit();
    }
}

/// Charge `count` decoded widget nodes against the frame node budget.
fn widget_decode_add_nodes(count: usize) -> Result<(), String> {
    WIDGET_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.nodes = budget.nodes.saturating_add(count);
        WIDGET_DECODE_BUDGET.set(budget);
        if budget.nodes > MAX_SCENE_NODES {
            Err(format!(
                "widget frame exceeds the decoded node budget ({MAX_SCENE_NODES})"
            ))
        } else {
            Ok(())
        }
    })
}

/// Bounded deserialize helper for the recursive `Vec<WidgetNode>` carriers
/// (`WidgetSurface::roots` and `WidgetGroup::children`). Charges one nesting
/// level and the decoded node total against the frame budget: a payload past
/// the budget refuses before the next level recurses, so the decoding stack
/// stays bounded. The wire layout is unchanged.
fn budgeted_widget_nodes<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<WidgetNode>, D::Error> {
    widget_decode_enter().map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = WidgetDecodeGuard;
    let nodes = Vec::<WidgetNode>::deserialize(deserializer)?;
    widget_decode_add_nodes(nodes.len()).map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(nodes)
}

/// Bounded deserialize helper for the flat `WidgetSurfacePatch::ops` carrier.
/// Charges the decoded op total against the frame budget; nested widget
/// nodes inside ops recurse into [`budgeted_widget_nodes`] at their own
/// levels. Mirrors the scene's `budgeted_ops`, so — with the documented
/// `MAX_PATCH_OPS == MAX_SCENE_NODES` coincidence — one decoded widget patch
/// frame can never carry ops that outgrow the node budget independently of
/// widget nodes.
fn budgeted_widget_ops<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<WidgetOp>, D::Error> {
    widget_decode_enter().map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = WidgetDecodeGuard;
    let ops = Vec::<WidgetOp>::deserialize(deserializer)?;
    widget_decode_add_nodes(ops.len()).map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(ops)
}

/// Bounded deserialize helper for a whole `Vec<WidgetEvent>` event frame.
/// Mirrors [`budgeted_widget_nodes`]: one charge per frame plus the decoded
/// event total against the frame budget. The production caller is the future
/// widget event transport (ADR-004 wire deferral); the helper exists with
/// the same gating as [`widget_decode_frame_begin`] until then.
#[cfg(any(feature = "host", test))]
#[allow(dead_code)] // wired into the widget event transport by a follow-up task
pub(crate) fn budgeted_widget_events<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<WidgetEvent>, D::Error> {
    widget_decode_enter().map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = WidgetDecodeGuard;
    let events = Vec::<WidgetEvent>::deserialize(deserializer)?;
    widget_decode_add_nodes(events.len()).map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(events)
}

/// Bounded deserialize helper for the flat tree-row payload vec. The scene
/// budget does not charge payload vecs (they are not widget nodes), so the
/// cap is enforced per vec directly: a hostile frame carrying millions of
/// rows refuses at decode before the allocation is fully materialized, and
/// the count matches what `WidgetSurface::validate` enforces per payload.
fn budgeted_tree_rows<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TreeRowDef>, D::Error> {
    let rows = Vec::<TreeRowDef>::deserialize(deserializer)?;
    if rows.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "tree rows exceed the decoded payload budget ({MAX_WIDGET_ROWS_INITIAL})"
        )));
    }
    Ok(rows)
}

/// Bounded deserialize helper for the flat list-item payload vec (same
/// contract as [`budgeted_tree_rows`]).
fn budgeted_list_items<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ItemDef>, D::Error> {
    let items = Vec::<ItemDef>::deserialize(deserializer)?;
    if items.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "list items exceed the decoded payload budget ({MAX_WIDGET_ROWS_INITIAL})"
        )));
    }
    Ok(items)
}

/// Bounded deserialize helper for the menu-item vec. Unlike rows/items,
/// menu items recurse (`MenuItemDef::submenu`), so the helper ALSO charges
/// one menu nesting level against a dedicated depth-only budget capped at
/// [`MAX_MENU_DEPTH`]: a hostile frame cannot recurse the `Box<MenuDef>`
/// chain past the validation cap, matching the structural gate exactly.
fn budgeted_menu_items<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<MenuItemDef>, D::Error> {
    menu_decode_enter().map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = MenuDecodeGuard;
    let items = Vec::<MenuItemDef>::deserialize(deserializer)?;
    if items.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "menu items exceed the decoded payload budget ({MAX_WIDGET_ROWS_INITIAL})"
        )));
    }
    Ok(items)
}

/// Depth-only decode budget for the `Box<MenuDef>` recursion (cap
/// [`MAX_MENU_DEPTH`]); shape-mirrors [`WidgetDecodeBudget`].
#[derive(Clone, Copy)]
struct MenuDecodeBudget {
    depth: usize,
}

thread_local! {
    static MENU_DECODE_BUDGET: std::cell::Cell<MenuDecodeBudget> =
        const { std::cell::Cell::new(MenuDecodeBudget { depth: 0 }) };
}

/// Leave one menu level (restores the menu depth budget even on decode
/// failure).
fn menu_decode_exit() {
    MENU_DECODE_BUDGET.with(|budget| {
        budget.set(MenuDecodeBudget {
            depth: budget.get().depth.saturating_sub(1),
        });
    });
}

struct MenuDecodeGuard;

impl Drop for MenuDecodeGuard {
    fn drop(&mut self) {
        menu_decode_exit();
    }
}

/// Enter one menu level during a decode. Mirrors [`widget_decode_enter`]'s
/// error style with the [`MAX_MENU_DEPTH`] cap.
fn menu_decode_enter() -> Result<(), String> {
    MENU_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.depth += 1;
        MENU_DECODE_BUDGET.set(budget);
        if budget.depth > MAX_MENU_DEPTH {
            return Err(format!(
                "menu nesting exceeds the decoded depth budget ({MAX_MENU_DEPTH})"
            ));
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Typed-table budgets (SWG-004 payload caps)
// ---------------------------------------------------------------------------

/// Bounded deserialize helper for the trailing [`WidgetSurface::tables`]
/// registry map (serde default [`empty_tables`] covers the absent key). The
/// cap reuses the payload magnitude: a hostile frame carrying a registry
/// wider than [`MAX_WIDGET_ROWS_INITIAL`] tables per surface refuses at
/// decode before the allocation is fully materialized. The registry KEY set
/// is the foreign table identity space — no interaction with the
/// surface-wide widget namespace.
fn budgeted_table_registry<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<StableId, PanelTableDef>, D::Error> {
    let tables = BTreeMap::<StableId, PanelTableDef>::deserialize(deserializer)?;
    if tables.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "table registry exceeds the decoded payload budget ({MAX_WIDGET_ROWS_INITIAL})"
        )));
    }
    Ok(tables)
}

/// Bounded deserialize helper for one [`PanelTableDef`]'s column registry.
/// Decode-sanity cap at the shared payload magnitude: no dedicated column
/// budget exists (a real grid's column registry is far thinner than this),
/// and the exact per-table equality (cells shadow columns 1:1) is enforced
/// by [`PanelTableDef::validate`], not here.
fn budgeted_table_columns<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TableColumnDef>, D::Error> {
    let columns = Vec::<TableColumnDef>::deserialize(deserializer)?;
    if columns.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "table columns exceed the decoded payload budget ({MAX_WIDGET_ROWS_INITIAL})"
        )));
    }
    Ok(columns)
}

/// Bounded deserialize helper for one [`PanelTableDef`]'s published row
/// window — the same cap [`budgeted_tree_rows`] enforces: rows beyond
/// [`MAX_WIDGET_ROWS_INITIAL`] per payload/window refuse at decode, per the
/// "100k backing objects do not create 100k host widgets" windowing rule
/// (large backing data pages through `total_rows` +
/// [`WidgetEvent::TablePageRequested`]).
fn budgeted_table_rows<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TableRowDef>, D::Error> {
    let rows = Vec::<TableRowDef>::deserialize(deserializer)?;
    if rows.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "table rows exceed the decoded payload budget ({MAX_WIDGET_ROWS_INITIAL})"
        )));
    }
    Ok(rows)
}

/// Bounded deserialize helper for one row's cell vec. Decode-sanity cap at
/// the shared payload magnitude; the exact equality (one cell per column,
/// shadowing by position) is [`PanelTableDef::validate`]'s job.
fn budgeted_table_cells<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TableCell>, D::Error> {
    let cells = Vec::<TableCell>::deserialize(deserializer)?;
    if cells.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "table row cells exceed the decoded payload budget ({MAX_WIDGET_ROWS_INITIAL})"
        )));
    }
    Ok(cells)
}

/// Bounded deserialize helper for [`WidgetEvent::TablePageRequested`]'s
/// `len` (cap [`MAX_WIDGET_RANGE_LEN`], the same page width
/// [`WidgetEvent::ChildrenRequested`]'s range carries). Matches the
/// construct-level [`WidgetEvent::validate`] bound (both levels equal).
fn bounded_table_page_len<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<usize, D::Error> {
    let len = usize::deserialize(deserializer)?;
    if len > MAX_WIDGET_RANGE_LEN {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "table page request exceeds the decoded range budget ({MAX_WIDGET_RANGE_LEN})"
        )));
    }
    Ok(len)
}

// ---------------------------------------------------------------------------
// Shared small types
// ---------------------------------------------------------------------------

/// Host-managed asset reference for widget icons.
///
/// Mirrors [`crate::scene::SceneElement::Asset`] semantics: the plugin never
/// sends bytes — `uri` is an opaque reference the host loads and caches
/// against whatever its per-panel asset loader accepts (at most
/// [`MAX_ASSET_URI_BYTES`] bytes, the scene asset cap this crate shares).
/// `size` is the host's render hint in logical units; a nonsensical size is
/// clamped host-side like a group `split_ratio` (presentation hint, not a
/// data-contract invariant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconRef {
    /// Opaque asset reference (loaded and cached by the host).
    pub uri: String,
    /// Render hint in logical units (default [`default_icon_size`], `16.0`).
    #[serde(default = "default_icon_size")]
    pub size: f32,
}

/// The EXPLICIT domain-object mapping of a tree row or list item (invariant
/// WGT-ID).
///
/// A widget's `StableId` is presentation identity; `kind` names the
/// plugin-declared domain-object family (`"alignment"`, `"corridor"`,
/// `"surface"`, …) and `id` is the domain's own key. A row WITHOUT a
/// `domain_ref` is perfectly valid pure presentation; no code path may
/// conflate presentation ids with domain identities implicitly — the mapping
/// exists only where the plugin declares it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomainRef {
    /// Plugin-declared domain-object family name.
    pub kind: String,
    /// The domain object's own key within that family.
    pub id: u64,
}

/// Selection/filter/lazy capabilities of one [`WidgetNode::Tree`].
///
/// All capabilities default to `false`: a tree is single-select, unfiltered,
/// and statically populated unless the plugin states otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TreeCapabilities {
    /// Allow multi-selection (the tree's selection events then carry more
    /// than one id).
    #[serde(default)]
    pub multi_select: bool,
    /// Allow the host's native filter row on this tree.
    #[serde(default)]
    pub filterable: bool,
    /// The tree's rows declare lazy children (`TreeRowDef::lazy_children`)
    /// so the host must be ready to issue
    /// [`WidgetEvent::ChildrenRequested`].
    #[serde(default)]
    pub lazy_children: bool,
}

/// Confirmation policy attached to an [`IconActionDef`] or a
/// [`MenuItemDef`]: what the host must do before invoking the semantic
/// command.
///
/// Compatibility rule: ONE version today; future policies (password
/// re-entry, typed confirmation, …) are APPENDED so the bincode tag of
/// `Prompt` stays `0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfirmPolicy {
    /// Ask the user once, showing `message`, before invoking the command.
    Prompt {
        /// The prompt text shown by the host.
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Widget definitions (Family 1)
// ---------------------------------------------------------------------------

/// Semantic Tree widget (invariant WGT tree contract): stable persistent row
/// ids + parent/child relations; expand/collapse + selection as
/// capabilities; lazy child loading; incremental row surgery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetTreeDef {
    /// Stable widget id.
    pub id: StableId,
    /// Optional display name; for trees/lists this name IS the accessible
    /// name (menu items are command-addressed instead).
    #[serde(default)]
    pub name: Option<String>,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Visible state (default `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Optional caption label above/below the tree.
    pub label: Option<String>,
    /// Selection/filter/lazy capabilities.
    pub capabilities: TreeCapabilities,
    /// Initial row payload (windowed past [`MAX_WIDGET_ROWS_INITIAL`]).
    #[serde(deserialize_with = "budgeted_tree_rows")]
    pub rows: Vec<TreeRowDef>,
}

/// One row of a [`WidgetTreeDef`].
///
/// Ordered no-forward parent bindings (the crate's payload-validation
/// pattern): `parent`, when present, must name a row that exists EARLIER in
/// the same `rows` vec — no self-reference, no forward reference. A self
/// parent is the representable parent-cycle shape and is rejected as
/// [`WidgetError::DepthExceeded`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeRowDef {
    /// Stable presentation id of the row (surface-wide unique; never a
    /// domain identity — see [`DomainRef`]).
    pub id: StableId,
    /// Parent row id; `None` at the root level of the row payload. Must name
    /// a preceding row (no forward refs).
    pub parent: Option<StableId>,
    /// Visible row label (the row's accessible text too).
    pub label: String,
    /// Optional icon reference.
    #[serde(default)]
    pub icon: Option<IconRef>,
    /// Optional status text (e.g. "modified", counts) the host renders as a
    /// secondary run.
    #[serde(default)]
    pub status: Option<String>,
    /// The row can expand/collapse.
    #[serde(default)]
    pub expandable: bool,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Expandable with NO static children: the host must issue
    /// [`WidgetEvent::ChildrenRequested`] when the row expands.
    #[serde(default)]
    pub lazy_children: bool,
    /// The EXPLICIT domain-object mapping ([`DomainRef`]); absent on pure
    /// presentation rows.
    #[serde(default)]
    pub domain_ref: Option<DomainRef>,
}

/// Semantic List widget: stable item ids, selection + activation,
/// enabled/visible, context-command association, windowing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemListDef {
    /// Stable widget id.
    pub id: StableId,
    /// Optional display name (also the accessible name).
    #[serde(default)]
    pub name: Option<String>,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Visible state (default `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Optional caption label above/below the list.
    pub label: Option<String>,
    /// Initial item payload (windowed past [`MAX_WIDGET_ROWS_INITIAL`]).
    #[serde(deserialize_with = "budgeted_list_items")]
    pub items: Vec<ItemDef>,
}

/// One item of an [`ItemListDef`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    /// Stable presentation id of the item (surface-wide unique).
    pub id: StableId,
    /// Visible item label (the item's accessible text too).
    pub label: String,
    /// Optional icon reference.
    #[serde(default)]
    pub icon: Option<IconRef>,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Visible state (default `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Context-command association: the semantic command the item
    /// activates. `None` marks a purely presentational item.
    #[serde(default)]
    pub command: Option<String>,
    /// The EXPLICIT domain-object mapping ([`DomainRef`]).
    #[serde(default)]
    pub domain_ref: Option<DomainRef>,
}

/// Semantic IconButton widget. Accessibility is NOT optional: an
/// `accessible_name` is required and validated non-empty. The action is a
/// semantic `command` id — NEVER a drawing callback from the plugin (the
/// plugin never draws widget chrome; ADR-005).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconActionDef {
    /// Stable widget id.
    pub id: StableId,
    /// Accessible name (REQUIRED, non-empty) — the a11y contract for
    /// buttons cannot be defaulted away.
    pub accessible_name: String,
    /// Optional tooltip text.
    #[serde(default)]
    pub tooltip: Option<String>,
    /// Host-managed icon reference.
    pub icon: IconRef,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Visible state (default `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Pressed/latched state, host-rendered (default `false`).
    #[serde(default)]
    pub active: bool,
    /// Semantic command id invoked on click — never a drawing callback.
    pub command: String,
    /// Optional confirmation policy the host applies before invoking.
    #[serde(default)]
    pub confirmation: Option<ConfirmPolicy>,
}

/// Semantic Menu / context-menu widget: stable command ids; labels, icons,
/// shortcuts; enabled/visible; confirmation. Commands carry NO implicit
/// document mutation — invocation reports explicit target ids via
/// [`WidgetEvent::CommandInvoked`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuDef {
    /// Stable widget id (menus are surface widgets; submenus re-use this
    /// shape).
    pub id: StableId,
    /// Optional display name (also the accessible name).
    #[serde(default)]
    pub name: Option<String>,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Visible state (default `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Optional menu caption (menu-bar title / context-menu heading).
    pub label: Option<String>,
    /// Menu items; recurse into submenus (`MenuItemDef::submenu`) up to
    /// [`MAX_MENU_DEPTH`].
    #[serde(deserialize_with = "budgeted_menu_items")]
    pub items: Vec<MenuItemDef>,
}

/// One item of a [`MenuDef`].
///
/// Menu items are COMMAND-addressed: `command_id` is a `String`, not a
/// `StableId`, and deliberately stays OUT of the surface-wide id namespace
/// (`WidgetError::DuplicateWidgetId` never fires for command ids). There is
/// no item `StableId`, so per-item errors name the OWNING menu's widget id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuItemDef {
    /// Semantic command id (REQUIRED, non-empty).
    pub command_id: String,
    /// Visible item label.
    pub label: String,
    /// Optional icon reference.
    #[serde(default)]
    pub icon: Option<IconRef>,
    /// Display hint for a keyboard shortcut — the host never READS keys
    /// through it; bindings live host-side.
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Visible state (default `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Optional submenu (bounded nesting, [`MAX_MENU_DEPTH`]).
    #[serde(default)]
    pub submenu: Option<Box<MenuDef>>,
    /// Optional confirmation policy the host applies before invoking.
    #[serde(default)]
    pub confirmation: Option<ConfirmPolicy>,
}

/// Table REFERENCE widget: a placeholder reference into the typed-table
/// contract that `SWG-004` ships. This tier does NOT restate the legacy
/// `DialogControl::Table` grid (string cells live there and stay compatible)
/// nor a typed-cell vocabulary; `table_id` names the backing table in the
/// typed-table registry ([`WidgetSurface::tables`] — `SWG-004`'s
/// [`PanelTableDef`] map; see the [module docs](self)).
///
/// Identity rule: `table_id` is FOREIGN to the surface id namespace (WGT-ID)
/// — it is a DIFFERENT identity space, several refs may name one backing
/// table, and a PUBLISHING surface (a non-empty registry) requires every
/// ref's target to resolve ([`WidgetError::UnknownTableRef`] otherwise;
/// empty-registry placeholder surfaces are the recorded `SWG-001` era —
/// see the placeholder transition in the [module docs](self)).
/// The registry's own per-table rule: NO interaction state (row selection,
/// scroll, expansion) travels in a [`PanelTableDef`] — that is host-local
/// state class 3; the def is pure data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableRef {
    /// Stable widget id.
    pub id: StableId,
    /// Optional display name (also the accessible name).
    #[serde(default)]
    pub name: Option<String>,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Visible state (default `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Backing table id within the typed-table contract (`SWG-004`).
    pub table_id: StableId,
}

/// Stacking direction of a [`WidgetGroup`].
///
/// Compatibility rule: new variants appended; `vertical` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Children stack top-to-bottom (the Column of the reference layout).
    #[default]
    Vertical,
    /// Children stack left-to-right (the Row of the reference layout).
    Horizontal,
}

/// The DOCUMENTED minimal composition container — the host layout capability
/// behind the reference Column/Row/SplitView layout.
///
/// A `Group` with `split_ratio: Some(_)` is the documented SplitView: the
/// host divides the box at the ratio (`0.0 < ratio < 1.0` is the sane
/// window; a nonsensical or non-finite ratio is CLAMPED host-side — this
/// tier states no float invariant for layout hints). `split_ratio: None`
/// means plain stacking. No general layout engine exists at this tier
/// (ADR-005); scene panes mount at the host dock NEXT TO the composed
/// surface, so where the reference layout shows a Scene cell this group
/// stays an empty placeholder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetGroup {
    /// Stable widget id.
    pub id: StableId,
    /// Optional display name (also the accessible name for container
    /// screen-reader purposes).
    #[serde(default)]
    pub name: Option<String>,
    /// Enabled state (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Visible state (default `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Child stacking direction (default `vertical`).
    pub direction: Axis,
    /// Child widgets. Decoded under the widget budget (depth/node caps).
    #[serde(deserialize_with = "budgeted_widget_nodes")]
    pub children: Vec<WidgetNode>,
    /// SplitView ratio when this group divides itself; `None` = plain
    /// stacking.
    #[serde(default)]
    pub split_ratio: Option<f32>,
}

// ---------------------------------------------------------------------------
// WidgetNode
// ---------------------------------------------------------------------------

/// One widget node in a [`WidgetSurface`] (additive pre-pin).
///
/// Compatibility rule: new variants MUST be appended at the end so existing
/// plugins keep their bincode discriminant indices (`0..=5`; pinned by the
/// tests in this module — this order is the pin). A `Group` — always LAST —
/// is the only compositional variant; the other five are Family-1 semantic
/// controls.
///
/// Deserialization is bounded (see [module docs](self)): the wire layout is
/// unchanged, but a payload whose `Group` nesting passes the decoded budget
/// is rejected instead of overflowing the stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WidgetNode {
    /// Semantic retained tree.
    Tree(WidgetTreeDef),
    /// Semantic retained item list.
    List(ItemListDef),
    /// Semantic icon button (semantic command; no drawing callbacks).
    IconButton(IconActionDef),
    /// Semantic menu (context-capable; submenu-tree via
    /// [`MenuItemDef::submenu`]).
    Menu(MenuDef),
    /// Table REFERENCE (`SWG-004` typed-table contract placeholder).
    Table(TableRef),
    /// Minimal composition container (host layout capability; SplitView when
    /// `split_ratio` is `Some`).
    Group(WidgetGroup),
}

impl WidgetNode {
    /// The node's stable id. Every variant carries exactly one (inside its
    /// `Def` struct); patches and the surface-wide namespace reference
    /// widgets through it.
    pub const fn widget_id(&self) -> StableId {
        match self {
            Self::Tree(def) => def.id,
            Self::List(def) => def.id,
            Self::IconButton(def) => def.id,
            Self::Menu(def) => def.id,
            Self::Table(def) => def.id,
            Self::Group(def) => def.id,
        }
    }

    /// `true` while the node anchors a [`WidgetGroup`] (the only variant a
    /// patch [`WidgetOp::Add`] may append into).
    pub const fn is_group(&self) -> bool {
        matches!(self, Self::Group(_))
    }
}

// ---------------------------------------------------------------------------
// Wire containers (snapshot, patch, ops)
// ---------------------------------------------------------------------------

/// A complete snapshot of one panel's semantic widget surface.
///
/// Mirror of [`crate::scene::PanelScene`] for Family-1 widgets: the plugin
/// owns the semantic widget data, the host owns native rendering and the
/// transient interaction state. The surface shares the panel's ONE
/// [`SceneRevision`] space with its `PanelScene` — one panel, one revision
/// cadence, so widgets and scenes compose without cross-referencing
/// revisions. Decoded under the widget budget.
///
/// SWG-004 grew ONE trailing field: the typed-table registry
/// ([`WidgetSurface::tables`]) keyed by table id — a DIFFERENT identity
/// space from the widget namespace (see the [module docs](self)). The
/// trailing-field wire hazard and its pre-pin clearance are documented on
/// the field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetSurface {
    /// The surface's revision; raises monotonically per panel.
    pub revision: SceneRevision,
    /// Root widget nodes, outermost-first (document order). Decoded under
    /// the widget budget; total widget nodes per surface stay within
    /// [`MAX_SCENE_NODES`] and non-group widgets within
    /// [`MAX_WIDGETS_PER_PANEL`].
    #[serde(deserialize_with = "budgeted_widget_nodes")]
    pub roots: Vec<WidgetNode>,
    /// The typed-table registry (`SWG-004`): the defs every
    /// [`WidgetNode::Table`] reference resolves against, keyed by
    /// `PanelTableDef::id_table` — a per-surface FOREIGN identity space
    /// (not the widget namespace; several refs may share one backing
    /// table). Empty is the ordinary surface (most panels publish no
    /// tables).
    ///
    /// ⚠ TRAILING-FIELD HAZARD (documented WARN record): serde(default)
    /// makes JSON old-shape-tolerant (payloads without the key decode with
    /// an empty registry), but a BINCODE 1 struct decode reads EVERY
    /// field — a frame emitted before this field existed would end here
    /// (`UnexpectedEof`) instead of tolerating the absence. Pre-pin
    /// clearance: no frozen release wire ever carried the 2-field surface
    /// (this type is the project's own, created unreleased), so the
    /// literal old-frame decode TEST is N/A until the ADR-004 wire freeze
    /// (API-24 — the crossing, re-versioned from the API-23 label at
    /// SCN-026) ships a pinned shape to snapshot — at that freeze the
    /// test becomes mandatory, mirroring the host's
    /// `deserialize_selectable_custom` old-payload tolerance story.
    /// Registry values are decode-bounded ([`budgeted_table_registry`]).
    #[serde(default = "empty_tables", deserialize_with = "budgeted_table_registry")]
    pub tables: BTreeMap<StableId, PanelTableDef>,
}

impl WidgetSurface {
    /// Structural validation: unique ids across the whole surface-wide
    /// namespace (widget nodes + tree rows + list items), group nesting and
    /// row parent-chains within [`MAX_SCENE_DEPTH`], total widget node count
    /// within [`MAX_SCENE_NODES`], non-group widget count within
    /// [`MAX_WIDGETS_PER_PANEL`], per-payload row/item/menu-item counts
    /// within [`MAX_WIDGET_ROWS_INITIAL`], required non-empty texts, and
    /// text/asset-uri byte caps — PLUS the SWG-004 registry contract: every
    /// table def validated against its per-table rules, every registry
    /// entry keyed by its own `id_table`, and every [`WidgetNode::Table`]
    /// reference resolving into a PUBLISHING (non-empty) registry
    /// ([`WidgetError::UnknownTableRef`]; empty-registry placeholder
    /// surfaces are the documented `SWG-001` era).
    ///
    /// This is deliberately structural only: host presentation concerns
    /// (ratio/size hints, expansion state, windowing, hit zones) are NOT
    /// checked here.
    pub fn validate(&self) -> Result<(), WidgetError> {
        validate_root(&self.roots)?;
        // Registry: entries keyed by their own id (a miskeyed entry would
        // put the table under an id its patches/refs cannot agree on), then
        // the per-def rules, then every TableRef's target resolving.
        for (key, def) in &self.tables {
            if def.id_table != *key {
                return Err(WidgetError::UnknownTableRef(def.id_table));
            }
            def.validate()?;
        }
        validate_table_refs(&self.roots, &self.tables)
    }

    /// Apply an incremental patch atomically (two phases, in place) — the
    /// mirror of [`crate::scene::PanelScenePatch`] semantics on the widget
    /// vocabulary.
    ///
    /// The patch must name the current surface revision as its base and a
    /// strictly greater successor; a rejection leaves `self` exactly as it
    /// was (no node moves, the revision stands), mirroring the crate
    /// contract that an error leaves the drawing and undo history unchanged.
    ///
    /// Phase 0 — three cheap checks with nothing written: base freshness,
    /// monotonic successor, and the [`MAX_PATCH_OPS`] op budget.
    ///
    /// Phase 1 — planning pass, still without any mutation: the whole op
    /// sequence is replayed against [`PlannedIndex`], a lightweight
    /// id/position mirror of the surface (built by ONE walk, tracking each
    /// widget node's payload-internal ids for the surface-wide namespace),
    /// and every op is expanded into a pre-located journal step
    /// ([`PlannedOp`]). Each op is fully validated against the current
    /// post-prior-ops state before its journal step is recorded; any failure
    /// aborts the whole patch.
    ///
    /// Phase 2 — the journal splices into the real children vecs strictly in
    /// order, then the successor revision is adopted. Journal locations were
    /// derived from the mirror exactly as the mirror accounted for the same
    /// mutations, so after a successful phase 1 every phase-2 step is
    /// infallible by construction; the whole patch commits in one step with
    /// no partial writes possible.
    ///
    /// Like the scene machinery, phase 1 proves the same properties on the
    /// planned state and assumes the base surface is itself structurally
    /// valid (the `validate` gate's job); an invalid base no longer fails a
    /// patch that does not rewrite the offending region.
    ///
    /// An empty op list is a legitimate no-op that advances the revision.
    ///
    /// # Op families (SWG-004)
    ///
    /// [`WidgetOp::TableCell`] addresses the typed-table REGISTRY
    /// ([`WidgetSurface::tables`]); the structural ops address the widget
    /// tree. The op list is therefore split into its families and each is
    /// planned against its own store: table calls write CELLS ONLY (never
    /// table/row/column structure), so the table family needs no mutation
    /// mirror, and structural ops never touch the registry. One patch may
    /// mix families — atomicity is unchanged (any family refusal leaves
    /// `self` untouched); family order is unobservable (disjoint stores),
    /// and when BOTH families would refuse, the table family's defect is
    /// reported (its address-ful [`WidgetError::InvalidCell`] /
    /// [`WidgetError::UnknownTableRef`] names the narrower cause; a refusal
    /// is total either way).
    pub fn apply_patch(&mut self, patch: WidgetSurfacePatch) -> Result<(), WidgetError> {
        // Phase 0: cheap whole-patch checks; nothing written yet.
        if patch.base_revision != self.revision {
            return Err(WidgetError::StaleBase {
                current: self.revision,
                given: patch.base_revision,
            });
        }
        if patch.revision <= self.revision {
            return Err(WidgetError::NonMonotonicRevision {
                current: self.revision,
                given: patch.revision,
            });
        }
        if patch.ops.len() > MAX_PATCH_OPS {
            return Err(WidgetError::BudgetExceeded {
                current: patch.ops.len(),
                max: MAX_PATCH_OPS,
            });
        }

        // Phase 1: split the op families (they never share a store), then
        // validate each family's whole sequence; journals hold pre-located
        // steps without writing `self`.
        let mut table_ops = Vec::new();
        let mut structural_ops = Vec::with_capacity(patch.ops.len());
        for op in patch.ops {
            match op {
                WidgetOp::TableCell { .. } => table_ops.push(op),
                other => structural_ops.push(other),
            }
        }
        let table_journal = plan_table_writes(&self.tables, table_ops)?;
        let journal = plan_patch(&self.roots, &self.tables, structural_ops)?;

        // Phase 2: single commit point — replay the plans in place.
        apply_journal(&mut self.roots, journal);
        apply_table_writes(&mut self.tables, table_journal);
        self.revision = patch.revision;
        Ok(())
    }

    /// Bounded recovery read ([`WidgetSummary`]): revision, widget total,
    /// per-kind counts, and the capped root-id list. Pure read — one walk,
    /// no validation, never fails.
    pub fn widget_summary(&self) -> WidgetSummary {
        WidgetSummary::of(self)
    }
}

/// Incremental widget-surface update requests (additive pre-pin).
///
/// Compatibility rule: new variants MUST be appended at the end so existing
/// plugins keep their bincode discriminant indices (`0..=3`; pinned by the
/// tests in this module). The table-cell write family lives beside the
/// three structural ops but addresses a DIFFERENT store: cell writes mutate
/// the typed-table REGISTRY ([`WidgetSurface::tables`]), the structural ops
/// mutate the widget tree — see [`WidgetSurface::apply_patch`]'s family
/// split.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WidgetOp {
    /// Append a new widget. `parent: None` appends at the surface root;
    /// `parent: Some(id)` appends into that id, which must anchor a
    /// [`WidgetNode::Group`].
    Add {
        /// Anchor group id (root append when absent).
        parent: Option<StableId>,
        /// Widget to insert; every id in its payload (node ids + tree rows +
        /// list items) must not already be live.
        widget: WidgetNode,
    },
    /// Swap the live widget whose id is `id` out for `content`, whole
    /// payload and all. Keyed by `id`: the substituted `content` MUST carry
    /// the same widget id — a replacement that re-keys its target does not
    /// get a second identity field to do it in; it is expressed as Remove +
    /// Add, mirroring the scene Replace doctrine.
    Update {
        /// Target widget id (the swap key).
        id: StableId,
        /// Replacement widget (same widget id as `id`).
        content: WidgetNode,
    },
    /// Remove the widget rooted at `id` (its payload goes with it).
    Remove {
        /// Root id of the widget to remove.
        id: StableId,
    },
    /// ONE typed-table cell write — a PATCH FACT the plugin publishes (the
    /// patch = data leg of the documented events-vs-patches split; the
    /// `WidgetEvent::TableCellChanged` notify leg stays the three-variant
    /// [`WidgetCellValue`] skeleton). The address names the registry table,
    /// the stable ROW key, and the stable COLUMN id; the column's
    /// [`CellKind`] resolves the typed [`TableCell`] replacement, so only
    /// the scalar kinds (`text`/`number`/`bool`) are patch-writable through
    /// the skeleton. One op = one cell; "one cell edit transfers only its
    /// cell-level patch" is byte-proven by the size tests. Addressing is
    /// resolved against the CURRENT registry: plan/validation report an
    /// unknown table as [`WidgetError::UnknownTableRef`] and an unknown
    /// key/column (or a kind mismatch) as [`WidgetError::InvalidCell`].
    ///
    /// Compatibility: appended at the tail — bincode tag `3`.
    TableCell {
        /// The registry table id ([`TableRef::table_id`] space, NOT the
        /// widget-node namespace).
        table: StableId,
        /// Stable row key inside that table's published window.
        key: StableId,
        /// Stable column id inside that table's column registry.
        column: StableId,
        /// The new cell value (skeleton vocabulary; resolved to the typed
        /// cell by the column kind).
        value: WidgetCellValue,
    },
}

/// A whole widget-surface update: declared base, declared successor, and the
/// ops that carry one to the other. Decoded under the widget budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetSurfacePatch {
    /// Revision the ops were computed against.
    pub base_revision: SceneRevision,
    /// Successor revision applied when the ops settle.
    pub revision: SceneRevision,
    /// Ordered op list (empty patches are valid revision bumps).
    #[serde(default, deserialize_with = "budgeted_widget_ops")]
    pub ops: Vec<WidgetOp>,
}

// ---------------------------------------------------------------------------
// Widget events (small + semantic; host -> plugin direction when wired)
// ---------------------------------------------------------------------------

/// Maximum nesting depth of the event itself is trivially one; the decoder
/// bounds a whole `Vec<WidgetEvent>` frame via `budgeted_widget_events` (the
/// thread-local budget pattern of [`budgeted_widget_nodes`]).
///
/// Events are VALUES the host produces and the plugin consumes (host→plugin
/// notification direction when later wired; ADR-004 defers that crossing).
/// In THIS crate they are plain data: serde + bincode pins (pinned
/// `0..=6`) plus bounded deserialization. Never send a complete tree/list/
/// table after one interaction — full-collection retransmits are DEFECTS
/// (ARCHITECTURE §14).
///
/// Compatibility rule: new variants MUST be appended at the end so existing
/// plugins keep their bincode discriminant indices (`0..=6`; pinned by the
/// tests in this module).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WidgetEvent {
    /// A tree branch expanded/collapsed (host-local state; the semantic
    /// notification rides only when the plugin declared the branch lazy).
    TreeExpanded {
        /// Panel surface id (the `WidgetSurface`'s identity within the
        /// panel's event stream).
        surface_id: StableId,
        /// The row whose expansion toggled.
        node_id: StableId,
        /// New expansion state.
        expanded: bool,
    },
    /// Lazy page fetch: the host requests rows of a lazy subtree.
    ChildrenRequested {
        /// Panel surface id.
        surface_id: StableId,
        /// The parent row to fetch children of.
        parent_id: StableId,
        /// The requested window (bounded by [`MAX_WIDGET_RANGE_LEN`]).
        #[serde(deserialize_with = "bounded_widget_range")]
        range: WidgetRange,
    },
    /// Tree/list selection moved. IDs bounded by [`MAX_SELECTION_IDS`].
    SelectionChanged {
        /// Panel surface id.
        surface_id: StableId,
        /// The selected presentation ids (bounded set).
        #[serde(deserialize_with = "bounded_selection_ids")]
        selected_ids: Vec<StableId>,
    },
    /// A list item (or tree row) double-activated/committed.
    ItemActivated {
        /// Panel surface id.
        surface_id: StableId,
        /// The activated presentation id.
        item_id: StableId,
    },
    /// A semantic command was invoked (icon button click, menu item, …)
    /// with explicit target ids. Targets bounded by
    /// [`MAX_WIDGET_TARGET_IDS`]; no implicit document mutation — the
    /// plugin resolves what `command_id` names for these targets.
    CommandInvoked {
        /// Panel surface id.
        surface_id: StableId,
        /// Semantic command id (as declared by the widget).
        command_id: String,
        /// Explicit target ids (bounded set).
        #[serde(deserialize_with = "bounded_target_ids")]
        target_ids: Vec<StableId>,
    },
    /// One typed-table cell changed (`row_key` x `column_id`). The value
    /// rides the [`WidgetCellValue`] skeleton — deliberately UNWIDENED
    /// (pins `0..=2` must not break): this event is the read/notify shape
    /// (MCP read-back summarizes through it), while the FULLY typed fact
    /// travels as [`WidgetOp::TableCell`] patch data (the documented
    /// events-vs-patches split in the [module docs](self)).
    TableCellChanged {
        /// Panel surface id.
        surface_id: StableId,
        /// Stable row key of the backing table.
        row_key: StableId,
        /// Stable column id.
        column_id: StableId,
        /// The new cell value (skeleton vocabulary).
        value: WidgetCellValue,
    },
    /// Paged-table window fetch: the host requests the next window of a
    /// table whose def windows (`total_rows: Some(_)` standing for a
    /// backing set larger than its published `rows`). The
    /// [`WidgetEvent::ChildrenRequested`] analog for the typed-table
    /// contract — the plugin answers by publishing the window (a snapshot
    /// cycle regains the registry), keeping stable row keys end to end.
    ///
    /// Compatibility: appended at the tail — bincode tag `6`.
    TablePageRequested {
        /// Panel surface id.
        surface_id: StableId,
        /// The registry table the page belongs to.
        table: StableId,
        /// The row key the requested page STARTS at: the first row the
        /// plugin must include. The host addresses keyed windows (not
        /// offsets) because row keys are the stable identities the
        /// contract guarantees; keys are learned from the published
        /// registry (the initial window publishes the table's first row).
        first_key: StableId,
        /// Rows requested (bounded by [`MAX_WIDGET_RANGE_LEN`]).
        #[serde(deserialize_with = "bounded_table_page_len")]
        len: usize,
    },
}

impl WidgetEvent {
    /// Bounds re-check for producers (construct-level companion of the
    /// bounded decode): the same bounded sets the field deserializers
    /// enforce are re-verified here so a HOST-CONSTRUCTED (not decoded)
    /// event cannot smuggle an out-of-bounds set either.
    pub fn validate(&self) -> Result<(), WidgetError> {
        match self {
            Self::TreeExpanded { .. } | Self::ItemActivated { .. } => Ok(()),
            Self::ChildrenRequested { range, .. } => range.bounds_ok(),
            Self::SelectionChanged { selected_ids, .. } => {
                if selected_ids.len() > MAX_SELECTION_IDS {
                    return Err(WidgetError::BudgetExceeded {
                        current: selected_ids.len(),
                        max: MAX_SELECTION_IDS,
                    });
                }
                Ok(())
            }
            Self::CommandInvoked {
                command_id,
                target_ids,
                ..
            } => {
                check_text(None, "event command id", true, command_id)?;
                if target_ids.len() > MAX_WIDGET_TARGET_IDS {
                    return Err(WidgetError::BudgetExceeded {
                        current: target_ids.len(),
                        max: MAX_WIDGET_TARGET_IDS,
                    });
                }
                Ok(())
            }
            Self::TableCellChanged { value, .. } => {
                if let WidgetCellValue::Text(text) = value {
                    if text.len() > MAX_TEXT_BYTES {
                        return Err(WidgetError::PayloadTooLarge {
                            kind: "cell text",
                            bytes: text.len(),
                            max: MAX_TEXT_BYTES,
                        });
                    }
                }
                Ok(())
            }
            Self::TablePageRequested { len, .. } => {
                if *len > MAX_WIDGET_RANGE_LEN {
                    return Err(WidgetError::BudgetExceeded {
                        current: *len,
                        max: MAX_WIDGET_RANGE_LEN,
                    });
                }
                Ok(())
            }
        }
    }
}

/// The requested window of one lazy page fetch ([`WidgetEvent::ChildrenRequested`]).
/// `start` is the first row offset the host wants; `len` the page width,
/// bounded by [`MAX_WIDGET_RANGE_LEN`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WidgetRange {
    /// First row offset of the window.
    pub start: usize,
    /// Rows requested (bounded by [`MAX_WIDGET_RANGE_LEN`]).
    pub len: usize,
}

impl WidgetRange {
    /// Construct-level bound: reject page widths past
    /// [`MAX_WIDGET_RANGE_LEN`].
    pub fn bounds_ok(&self) -> Result<(), WidgetError> {
        if self.len > MAX_WIDGET_RANGE_LEN {
            return Err(WidgetError::BudgetExceeded {
                current: self.len,
                max: MAX_WIDGET_RANGE_LEN,
            });
        }
        Ok(())
    }
}

/// Cell value skeleton of [`WidgetEvent::TableCellChanged`].
///
/// Deliberately NOT widened with the typed-table contract (`SWG-004`): the
/// typed cell vocabulary is [`TableCell`] and it travels as PATCH data
/// ([`WidgetOp::TableCell`]) — the event stays the compact three-variant
/// read/notify shape so its pins (`0..=2`) never break and MCP read-back
/// keeps a stable summary vocabulary. WIDENING this enum later (a table
/// tier that must notify icon/badge/… cells summarily) appends at the tail.
///
/// Compatibility rule: new variants appended; bincode discriminants pinned
/// `0..=2`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WidgetCellValue {
    /// Text cell (byte-capped like every widget text).
    Text(String),
    /// Numeric cell.
    Number(f64),
    /// Boolean cell.
    Bool(bool),
}

/// Bounded deserialize helper for [`WidgetEvent::SelectionChanged`]'s
/// `selected_ids` (cap [`MAX_SELECTION_IDS`]). Construct-level: also matches
/// the validate() bound, so the bounded sets are identical on both paths.
fn bounded_selection_ids<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<StableId>, D::Error> {
    let ids = Vec::<StableId>::deserialize(deserializer)?;
    if ids.len() > MAX_SELECTION_IDS {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "selection event exceeds the decoded selection budget ({MAX_SELECTION_IDS})"
        )));
    }
    Ok(ids)
}

/// Bounded deserialize helper for [`WidgetEvent::CommandInvoked`]'s
/// `target_ids` (cap [`MAX_WIDGET_TARGET_IDS`]).
fn bounded_target_ids<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<StableId>, D::Error> {
    let ids = Vec::<StableId>::deserialize(deserializer)?;
    if ids.len() > MAX_WIDGET_TARGET_IDS {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "command event exceeds the decoded target budget ({MAX_WIDGET_TARGET_IDS})"
        )));
    }
    Ok(ids)
}

/// Bounded deserialize helper for [`WidgetEvent::ChildrenRequested`]'s
/// `range` (cap [`MAX_WIDGET_RANGE_LEN`] on `len`).
fn bounded_widget_range<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<WidgetRange, D::Error> {
    let range = WidgetRange::deserialize(deserializer)?;
    if range.len > MAX_WIDGET_RANGE_LEN {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "child-request event exceeds the decoded range budget ({MAX_WIDGET_RANGE_LEN})"
        )));
    }
    Ok(range)
}

// ---------------------------------------------------------------------------
// Bounded recovery read (WidgetSummary)
// ---------------------------------------------------------------------------

/// Widget-kind counts carried by [`WidgetSummary`] — one fixed field per
/// [`WidgetNode`] variant (6 today). The counting match is exhaustive, so
/// appending a new widget variant later is a compile error that guides the
/// summary (and every other by-kind consumer) to update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WidgetKindCounts {
    /// [`WidgetNode::Tree`] nodes.
    pub trees: usize,
    /// [`WidgetNode::List`] nodes.
    pub lists: usize,
    /// [`WidgetNode::IconButton`] nodes.
    pub icon_buttons: usize,
    /// [`WidgetNode::Menu`] nodes.
    pub menus: usize,
    /// [`WidgetNode::Table`] nodes.
    pub tables: usize,
    /// [`WidgetNode::Group`] nodes.
    pub groups: usize,
}

impl WidgetKindCounts {
    /// Sum over all kinds; always equals [`WidgetSummary::widgets`].
    pub fn total(&self) -> usize {
        self.trees + self.lists + self.icon_buttons + self.menus + self.tables + self.groups
    }
}

/// Bounded snapshot read of a widget surface — the mirror of
/// [`crate::scene::SceneSummary`] on the widget vocabulary (same capped
/// root-id shape, same [`MAX_SUMMARY_ROOT_IDS`] cap, the stale-base
/// recovery read for a patch rejected with [`WidgetError::StaleBase`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetSummary {
    /// The surface's authoritative revision (the base a recovery patch must
    /// name).
    pub revision: SceneRevision,
    /// Total widget nodes in the surface (the same node count `validate`
    /// checks against [`MAX_SCENE_NODES`]).
    pub widgets: usize,
    /// One count per widget kind (sums to [`WidgetSummary::widgets`]).
    pub by_kind: WidgetKindCounts,
    /// First [`MAX_SUMMARY_ROOT_IDS`] root ids in document order; fewer when
    /// the surface roots fewer widgets than the cap.
    pub root_ids: Vec<StableId>,
    /// Root ids dropped beyond the [`MAX_SUMMARY_ROOT_IDS`] cap; `0` when
    /// nothing was truncated.
    pub root_ids_truncated: usize,
    /// `true` iff the root list was capped (mirror of
    /// [`WidgetSummary::root_ids_truncated`] being non-zero).
    pub overflow_roots: bool,
}

impl WidgetSummary {
    /// One-walk computation of the whole summary.
    fn of(surface: &WidgetSurface) -> WidgetSummary {
        let mut summary = WidgetSummary {
            revision: surface.revision,
            widgets: 0,
            by_kind: WidgetKindCounts::default(),
            root_ids: Vec::with_capacity(surface.roots.len().min(MAX_SUMMARY_ROOT_IDS)),
            root_ids_truncated: 0,
            overflow_roots: false,
        };
        for node in surface.roots.iter().take(MAX_SUMMARY_ROOT_IDS) {
            summary.root_ids.push(node.widget_id());
        }
        summary.root_ids_truncated = surface.roots.len().saturating_sub(MAX_SUMMARY_ROOT_IDS);
        summary.overflow_roots = summary.root_ids_truncated > 0;

        // One walk for the node total and the per-kind counts: an explicit
        // stack popped in document order, no recursion (the crate's
        // iterative-walk discipline).
        let mut walker: Vec<&WidgetNode> = surface.roots.iter().collect();
        while let Some(node) = walker.pop() {
            summary.widgets += 1;
            match node {
                WidgetNode::Group(group) => {
                    summary.by_kind.groups += 1;
                    walker.extend(group.children.iter().rev());
                }
                WidgetNode::Tree(_) => summary.by_kind.trees += 1,
                WidgetNode::List(_) => summary.by_kind.lists += 1,
                WidgetNode::IconButton(_) => summary.by_kind.icon_buttons += 1,
                WidgetNode::Menu(_) => summary.by_kind.menus += 1,
                WidgetNode::Table(_) => summary.by_kind.tables += 1,
            }
        }
        debug_assert_eq!(summary.by_kind.total(), summary.widgets);
        summary
    }
}

// ---------------------------------------------------------------------------
// Two-phase in-place patch planning (mirrored from scene.rs, SCN-010)
// ---------------------------------------------------------------------------

/// Placement of one widget in the PLANNED (post-journal) surface, exactly as
/// phase 2 splices it — mirror of the scene's `PlannedSlot`.
#[derive(Debug, Clone, Copy)]
struct PlannedSlot {
    /// Parent group id (`None` in the root surface list).
    parent: Option<StableId>,
    /// Position within the parent's ordered child list (root list order when
    /// [`PlannedSlot::parent`] is `None`).
    pos: usize,
    /// Planned depth of the widget; root widgets sit at depth 1.
    depth: usize,
    /// Whether the widget anchors a [`WidgetNode::Group`].
    is_group: bool,
}

/// One planned mutation — the journal phase 1 produces and phase 2 replays.
/// Mirror of the scene's `PlannedOp` over the widget vocabulary.
#[derive(Debug)]
enum PlannedOp {
    /// Push a widget into the child list of the group at `group_path`
    /// (empty path = the root surface list).
    Insert {
        /// Path to the host group (empty = root list).
        group_path: Vec<usize>,
        /// Widget to append.
        widget: WidgetNode,
    },
    /// Swap the widget at `element_path` for `widget` (same id; in place,
    /// so no sibling positions move).
    Swap {
        /// Path to the widget including its own position.
        element_path: Vec<usize>,
        /// Replacement widget.
        widget: WidgetNode,
    },
    /// Remove the subtree rooted at the widget at `element_path`.
    Take {
        /// Path to the widget including its own position.
        element_path: Vec<usize>,
    },
}

/// Planner-side mirror of the surface the journal will produce if committed.
/// Lean data only — ids, positions, flags; never widget clones. Extends the
/// scene mirror with payload-id bookkeeping: tree-row and list-item ids live
/// INSIDE their widget nodes, and the surface-wide namespace means a fresh
/// payload must not collide with any live NODE id nor with any live
/// PAYLOAD id, so each live node's payload ids are retired/adopted with it.
#[derive(Debug)]
struct PlannedIndex {
    /// Live widget id → planned placement.
    slots: HashMap<StableId, PlannedSlot>,
    /// Live group id → its ordered child ids (groups only; others have no
    /// entry).
    children: HashMap<StableId, Vec<StableId>>,
    /// Ordered root ids (mirror of the root surface list).
    roots: Vec<StableId>,
    /// Planned total widget-node count (base-counted, then advanced per
    /// payload) for the node budget.
    nodes: usize,
    /// Planned non-group widget count for the [`MAX_WIDGETS_PER_PANEL`]
    /// budget.
    widgets: usize,
    /// Live widget id → its payload-internal ids (tree rows / list items /
    /// submenu `MenuDef` ids).
    payload: HashMap<StableId, Vec<StableId>>,
    /// Union of every live widget's payload-internal ids (collision domain
    /// of the surface-wide namespace).
    payload_live: HashSet<StableId>,
}

impl PlannedIndex {
    /// Adopt a validated payload's payload-internal ids under the widget
    /// node `node` (payload-order vec; the retire side removes them
    /// verbatim).
    fn adopt_payload_ids(&mut self, node: StableId, ids: impl IntoIterator<Item = StableId>) {
        let ids: Vec<StableId> = ids.into_iter().collect();
        self.payload_live.extend(ids.iter().copied());
        self.payload.insert(node, ids);
    }

    /// Build the mirror from the CURRENT base surface with exactly one walk,
    /// O(surface) in time and lean-data memory (no widget clones).
    fn from_surface(roots: &[WidgetNode]) -> Self {
        let mut index = Self {
            slots: HashMap::new(),
            children: HashMap::new(),
            roots: roots.iter().map(|node| node.widget_id()).collect(),
            nodes: 0,
            widgets: 0,
            payload: HashMap::new(),
            payload_live: HashSet::new(),
        };
        // Reverse-pushed work stack so pops visit document order; root
        // widgets sit at depth 1 and each group level adds one.
        let mut stack: Vec<(&WidgetNode, Option<StableId>, usize, usize)> = roots
            .iter()
            .enumerate()
            .rev()
            .map(|(pos, node)| (node, None, pos, 1usize))
            .collect();
        while let Some((node, parent, pos, depth)) = stack.pop() {
            let id = node.widget_id();
            let is_group = node.is_group();
            index.slots.insert(
                id,
                PlannedSlot {
                    parent,
                    pos,
                    depth,
                    is_group,
                },
            );
            index.nodes += 1;
            if !is_group {
                index.widgets += 1;
            }
            match node {
                WidgetNode::Group(group) => {
                    index.children.insert(
                        id,
                        group
                            .children
                            .iter()
                            .map(|child| child.widget_id())
                            .collect(),
                    );
                    for (pos, child) in group.children.iter().enumerate().rev() {
                        stack.push((child, Some(id), pos, depth + 1));
                    }
                }
                WidgetNode::Tree(def) => {
                    index.adopt_payload_ids(id, def.rows.iter().map(|row| row.id));
                }
                WidgetNode::List(def) => {
                    index.adopt_payload_ids(id, def.items.iter().map(|item| item.id));
                }
                WidgetNode::Menu(def) => {
                    let mut payload_ids = Vec::new();
                    push_menu_payload_ids(def, &mut payload_ids);
                    index.adopt_payload_ids(id, payload_ids);
                }
                WidgetNode::IconButton(_) | WidgetNode::Table(_) => {}
            }
        }
        index
    }

    /// `true` while `id` is live in the planned state AS A WIDGET NODE. Tree
    /// rows and list items are NOT widget nodes: they cannot be patch
    /// targets ([`WidgetOp::Update`]/[`WidgetOp::Remove`] /
    /// [`WidgetOp::Add`]'s anchor), they swim inside their widget's payload.
    fn is_node_live(&self, id: StableId) -> bool {
        self.slots.contains_key(&id)
    }

    /// `true` while `id` is live in the planned state in EITHER sense —
    /// widget node OR payload-internal id (row/list item); the
    /// surface-wide namespace collision domain for fresh payload ids.
    fn is_id_live(&self, id: StableId) -> bool {
        self.slots.contains_key(&id) || self.payload_live.contains(&id)
    }

    /// Array path from the root surface list to the live widget `id`,
    /// ending at its own position. O(depth) parent-chain walk; the parent
    /// chain of a live id is live, so unreachability would be planner
    /// corruption.
    fn path_of(&self, id: StableId) -> Vec<usize> {
        debug_assert!(self.is_node_live(id), "path derivation for a retired id");
        let mut path = Vec::with_capacity(8);
        let mut current = Some(id);
        while let Some(node) = current {
            let slot = &self.slots[&node]; // parents of live nodes are live
            path.push(slot.pos);
            current = slot.parent;
        }
        path.reverse();
        path
    }

    /// Planned length of a container's ordered child list (the root list for
    /// a `None` parent).
    fn child_len(&self, parent: Option<StableId>) -> usize {
        match parent {
            None => self.roots.len(),
            Some(group) => self
                .children
                .get(&group)
                .expect("planner invariant: live group has a child list")
                .len(),
        }
    }

    /// Forget a live subtree in the mirror: the id, every descendant id,
    /// their group child lists, and their payload-id registrations leave the
    /// planner (mirror of the scene retire over the extra payload bookkeeping).
    /// The container list keeps the subtree's root when `keep_top_in_parent`
    /// (Update re-adopts into exactly that slot); otherwise the slot is
    /// dropped and the siblings to the right of it shift back by one,
    /// matching the `Vec::remove` phase 2 splices. Returns the retired
    /// `(node count, non-group widget count)` for the budget math.
    fn retire_subtree(&mut self, id: StableId, keep_top_in_parent: bool) -> (usize, usize) {
        let (parent, pos) = match self.slots.get(&id) {
            Some(slot) => (slot.parent, slot.pos),
            None => unreachable!("retire target is live in the planned state"),
        };
        let mut retired_nodes = 0usize;
        let mut retired_widgets = 0usize;
        let mut stack: Vec<StableId> = vec![id];
        while let Some(node) = stack.pop() {
            let is_group = self
                .slots
                .get(&node)
                .map(|slot| slot.is_group)
                .unwrap_or_else(|| unreachable!("retire recursion walks live slots only"));
            self.slots.remove(&node);
            if let Some(kids) = self.children.remove(&node) {
                stack.extend(kids);
            }
            if let Some(payload_ids) = self.payload.remove(&node) {
                for payload_id in payload_ids {
                    self.payload_live.remove(&payload_id);
                }
            }
            retired_nodes += 1;
            if !is_group {
                retired_widgets += 1;
            }
        }
        if !keep_top_in_parent {
            // Detach from the container (root list or parent children) and
            // repair the sibling positions phase 2's `Vec::remove` shifts.
            let list = match parent {
                None => &mut self.roots,
                Some(group) => self
                    .children
                    .get_mut(&group)
                    .expect("planner invariant: live group has a child list"),
            };
            debug_assert_eq!(list[pos], id, "retire slot matches the mirror");
            list.remove(pos);
            for &sibling in list.iter().skip(pos) {
                if let Some(slot) = self.slots.get_mut(&sibling) {
                    slot.pos -= 1;
                }
            }
        }
        (retired_nodes, retired_widgets)
    }

    /// Adopt a validated payload subtree into the planned state at the slot
    /// its top widget will occupy: `top_pos: None` attaches at the end of
    /// the parent's child list (Add), `Some(pos)` re-adopts an already-held
    /// container slot (Update). Checks the result budgets per adopted node
    /// (first-crossing values — node budget, depth budget, and the
    /// non-group widget budget — payload order, the same cap+1 report shape
    /// the whole-surface gate uses), records planned slots, child lists,
    /// and payload-id registrations. The caller already validated the
    /// payload structure and its id disjointness from the live set.
    fn adopt_subtree(
        &mut self,
        node: &WidgetNode,
        parent: Option<StableId>,
        top_depth: usize,
        top_pos: Option<usize>,
    ) -> Result<(), WidgetError> {
        let pos = match top_pos {
            Some(pos) => pos,
            None => self.child_len(parent),
        };
        if top_pos.is_none() {
            // Attach: the payload's top widget extends the parent's child
            // list, keeping list position and slot in lockstep.
            match parent {
                None => self.roots.push(node.widget_id()),
                Some(group) => self
                    .children
                    .get_mut(&group)
                    .expect("planner invariant: live group has a child list")
                    .push(node.widget_id()),
            }
        }
        // Reverse-pushed stack, document order; depth carries absolute
        // planned depth (root = 1, the container's + 1 here).
        let mut stack: Vec<(&WidgetNode, Option<StableId>, usize, usize)> =
            vec![(node, parent, top_depth, pos)];
        while let Some((node, parent, depth, pos)) = stack.pop() {
            let id = node.widget_id();
            let is_group = node.is_group();
            self.nodes += 1;
            if self.nodes > MAX_SCENE_NODES {
                return Err(WidgetError::BudgetExceeded {
                    current: self.nodes,
                    max: MAX_SCENE_NODES,
                });
            }
            if depth > MAX_SCENE_DEPTH {
                return Err(WidgetError::DepthExceeded {
                    current: depth,
                    max: MAX_SCENE_DEPTH,
                });
            }
            if !is_group {
                self.widgets += 1;
                if self.widgets > MAX_WIDGETS_PER_PANEL {
                    return Err(WidgetError::BudgetExceeded {
                        current: self.widgets,
                        max: MAX_WIDGETS_PER_PANEL,
                    });
                }
            }
            self.slots.insert(
                id,
                PlannedSlot {
                    parent,
                    pos,
                    depth,
                    is_group,
                },
            );
            match node {
                WidgetNode::Group(group) => {
                    self.children.insert(
                        id,
                        group
                            .children
                            .iter()
                            .map(|child| child.widget_id())
                            .collect(),
                    );
                    for (pos, child) in group.children.iter().enumerate().rev() {
                        stack.push((child, Some(id), pos, depth + 1));
                    }
                }
                WidgetNode::Tree(def) => {
                    self.adopt_payload_ids(id, def.rows.iter().map(|row| row.id));
                }
                WidgetNode::List(def) => {
                    self.adopt_payload_ids(id, def.items.iter().map(|item| item.id));
                }
                WidgetNode::Menu(def) => {
                    let mut payload_ids = Vec::new();
                    push_menu_payload_ids(def, &mut payload_ids);
                    self.adopt_payload_ids(id, payload_ids);
                }
                WidgetNode::IconButton(_) | WidgetNode::Table(_) => {}
            }
        }
        Ok(())
    }
}

/// Phase 1 of `WidgetSurface`'s patch application: replay `ops` against a
/// mirror of `roots`, validating each op fully (payload structure, id
/// collisions, parent/anchoring, result budgets, and — since `SWG-004` —
/// every table reference in the payload resolving against the CURRENT
/// registry `tables`) against the state all prior ops leave behind, and
/// journaling each op's pre-located mutation. Any error leaves the mirror
/// (and the caller's surface) exactly as the ops would have had it had
/// nothing happened, because only the mirror mutates during planning.
///
/// Per-op planning order matches the scene planner so error selections stay
/// pinned: payload structural validation → id-collision check →
/// parent-anchoring check, then mirror adoption and result budgets.
///
/// Precondition: `ops` carry NO [`WidgetOp::TableCell`] (the caller's
/// family split already routed table writes through [`plan_table_writes`] —
/// cell writes address the registry, not the widget tree).
fn plan_patch(
    roots: &[WidgetNode],
    tables: &BTreeMap<StableId, PanelTableDef>,
    ops: Vec<WidgetOp>,
) -> Result<Vec<PlannedOp>, WidgetError> {
    let mut planned = PlannedIndex::from_surface(roots);
    let mut journal: Vec<PlannedOp> = Vec::new();
    for op in ops {
        match op {
            WidgetOp::Add { parent, widget } => {
                // Subtree-internal validation (ids, rows bindings, texts,
                // caps) never touches the surface, so it runs first.
                validate_root(std::slice::from_ref(&widget))?;
                // The registry never grows through patch ops (it rides
                // snapshots), so a payload's table refs resolve against the
                // CURRENT registry or the patch is a defect — a dangling
                // ref added now could never become valid later.
                validate_table_refs(std::slice::from_ref(&widget), tables)?;
                // One payload walk for the fresh-id set (widget ids + row
                // ids + list item ids), then the collision check against
                // the planned state's FULL namespace.
                let fresh_ids = scan_payload_ids(&widget);
                for id in &fresh_ids {
                    if planned.is_id_live(*id) {
                        return Err(WidgetError::DuplicateWidgetId(*id));
                    }
                }
                // The parent must anchor a group in the planned state.
                let parent_depth = match parent {
                    None => 0usize,
                    Some(anchor) => {
                        let slot = planned
                            .slots
                            .get(&anchor)
                            .ok_or(WidgetError::ParentMissing(anchor))?;
                        if !slot.is_group {
                            return Err(WidgetError::ParentMissing(anchor));
                        }
                        slot.depth
                    }
                };
                // Adopt into the mirror (result budgets, planned slots).
                planned.adopt_subtree(&widget, parent, parent_depth + 1, None)?;
                // Journal the push, located for THIS entry's phase-2 time
                // slot (the path is derived from the mirror state the phase
                // 2 replay of this entry will find).
                let group_path = match parent {
                    None => Vec::new(),
                    Some(anchor) => planned.path_of(anchor),
                };
                journal.push(PlannedOp::Insert { group_path, widget });
            }
            WidgetOp::Update { id, content } => {
                // Update keys the target by `id`; the substituted content
                // must carry the same widget id (a re-key attempt is a
                // malformed target reference — re-keying is expressed as
                // Remove + Add, mirroring the scene Replace doctrine).
                if !planned.is_node_live(id) {
                    return Err(WidgetError::ElementMissing(id));
                }
                if content.widget_id() != id {
                    return Err(WidgetError::ElementMissing(id));
                }
                validate_root(std::slice::from_ref(&content))?;
                validate_table_refs(std::slice::from_ref(&content), tables)?;
                // Capture the target's slot, derive its pre-retire path (the
                // path is valid at THIS entry's phase-2 time slot), then
                // retire the outgoing subtree while keeping the container's
                // slot: the replacement re-adopts the very slot.
                let (parent, pos, top_depth) = {
                    let slot = &planned.slots[&id];
                    (slot.parent, slot.pos, slot.depth)
                };
                let element_path = planned.path_of(id);
                let (retired_nodes, retired_widgets) = planned.retire_subtree(id, true);
                planned.nodes -= retired_nodes;
                planned.widgets -= retired_widgets;
                // The replacement may re-adopt ids retired with the outgoing
                // subtree, but must not collide with ids that survived it.
                let fresh_ids = scan_payload_ids(&content);
                for id in &fresh_ids {
                    if planned.is_id_live(*id) {
                        return Err(WidgetError::DuplicateWidgetId(*id));
                    }
                }
                planned.adopt_subtree(&content, parent, top_depth, Some(pos))?;
                journal.push(PlannedOp::Swap {
                    element_path,
                    widget: content,
                });
            }
            WidgetOp::Remove { id } => {
                if !planned.is_node_live(id) {
                    return Err(WidgetError::ElementMissing(id));
                }
                // Derive the pre-retire path, then retire the subtree in the
                // mirror and detach it from its container (siblings shift
                // back, matching the `Vec::remove` phase 2 splices).
                let element_path = planned.path_of(id);
                let (retired_nodes, retired_widgets) = planned.retire_subtree(id, false);
                planned.nodes -= retired_nodes;
                planned.widgets -= retired_widgets;
                journal.push(PlannedOp::Take { element_path });
            }
            WidgetOp::TableCell { .. } => {
                // The caller's family split routes table writes through
                // `plan_table_writes` (they address the registry, not this
                // mirror); reaching this arm would be planner corruption.
                unreachable!("table cell writes plan through plan_table_writes");
            }
        }
    }
    Ok(journal)
}

/// Phase 2 of the mirrored patch application: replay the pre-validated
/// journal into the real child lists, strictly in order, with no further
/// decisions and no allocation beyond what the mutations themselves need.
/// Every step is infallible by construction (paths came from the planning
/// mirror exactly as the mirror tracked the same mutations); the commit
/// point is the journal replay plus the revision adoption, which is why a
/// planning failure leaves everything untouched.
fn apply_journal(roots: &mut Vec<WidgetNode>, journal: Vec<PlannedOp>) {
    for entry in journal {
        match entry {
            PlannedOp::Insert { group_path, widget } => {
                children_at_mut(roots, &group_path).push(widget);
            }
            PlannedOp::Swap {
                element_path,
                widget,
            } => {
                *widget_at_mut(roots, &element_path) = widget;
            }
            PlannedOp::Take { element_path } => {
                let (last, parent_path) = element_path
                    .split_last()
                    .expect("journal widget paths are non-empty");
                children_at_mut(roots, parent_path).remove(*last);
            }
        }
    }
}

/// The child list of the group at `group_path` (empty path = the root
/// surface list). Path components index children of the previous list; the
/// referenced node must exist and anchor a group — the phase-2 guarantee, so
/// a disagreement is an `unreachable`, not an error path.
fn children_at_mut<'a>(
    roots: &'a mut Vec<WidgetNode>,
    group_path: &[usize],
) -> &'a mut Vec<WidgetNode> {
    let mut level = roots;
    for &index in group_path {
        level = match &mut level[index] {
            WidgetNode::Group(group) => &mut group.children,
            other => unreachable!("journal location refers to a live group, found {other:?}"),
        };
    }
    level
}

/// Mutable view of the widget at `widget_path` (the last component is its
/// position inside the referenced container list).
fn widget_at_mut<'a>(roots: &'a mut Vec<WidgetNode>, widget_path: &[usize]) -> &'a mut WidgetNode {
    let (last, parent_path) = widget_path
        .split_last()
        .expect("journal widget paths are non-empty");
    &mut children_at_mut(roots, parent_path)[*last]
}

/// Doc-order scan of one op payload (top widget first), collecting the
/// fresh-id set the collision check compares against the planned state:
/// the widget node's own id PLUS every payload-internal id (tree rows, list
/// items, submenu MenuDef ids) of the node and its nested groups. Single
/// walk, O(payload); the payload itself is walked again by
/// [`PlannedIndex::adopt_subtree`], so this returns no deep payload data.
fn scan_payload_ids(widget: &WidgetNode) -> Vec<StableId> {
    let mut ids = Vec::new();
    let mut stack = vec![widget];
    while let Some(node) = stack.pop() {
        ids.push(node.widget_id());
        match node {
            WidgetNode::Tree(def) => ids.extend(def.rows.iter().map(|row| row.id)),
            WidgetNode::List(def) => ids.extend(def.items.iter().map(|item| item.id)),
            WidgetNode::Menu(def) => push_menu_payload_ids(def, &mut ids),
            WidgetNode::Group(group) => stack.extend(group.children.iter().rev()),
            _ => {}
        }
    }
    ids
}

/// Payload-internal ids of one [`MenuDef`] subtree: every nested submenu's
/// own `MenuDef` id (the TOP menu's id is its [`WidgetNode`]'s id, already
/// collected by the caller; `MenuDef` submenus are payload and carry no
/// widget nodes of their own).
fn push_menu_payload_ids(menu: &MenuDef, ids: &mut Vec<StableId>) {
    for item in &menu.items {
        if let Some(submenu) = &item.submenu {
            ids.push(submenu.id);
            push_menu_payload_ids(submenu, ids);
        }
    }
}

// ---------------------------------------------------------------------------
// Structural validation (iterative walk)
// ---------------------------------------------------------------------------

/// Record one id in the surface-wide namespace, rejecting the first
/// duplicate.
fn register_id(seen: &mut HashSet<StableId>, id: StableId) -> Result<(), WidgetError> {
    if seen.insert(id) {
        Ok(())
    } else {
        Err(WidgetError::DuplicateWidgetId(id))
    }
}

/// Display-text contract: byte cap ([`MAX_TEXT_BYTES`] — mirrors the scene's
/// text-run cap) plus a required-emptiness gate (`required` = the semantic
/// visible text must be non-empty, [`WidgetError::EmptyRequiredText`]).
/// `what` names the field in [`WidgetError::PayloadTooLarge`] reports. `id`
/// is the OWNING widget's id (or `None` only for event-side texts, which
/// carry no per-item widget identity).
fn check_text(
    id: Option<StableId>,
    what: &'static str,
    required: bool,
    text: &str,
) -> Result<(), WidgetError> {
    if required && text.is_empty() {
        return Err(WidgetError::EmptyRequiredText { id, what });
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(WidgetError::PayloadTooLarge {
            kind: what,
            bytes: text.len(),
            max: MAX_TEXT_BYTES,
        });
    }
    Ok(())
}

/// Optional-text form of [`check_text`] (`None` is always fine); reports
/// under the owning widget's `id`.
fn check_optional_text(
    id: StableId,
    what: &'static str,
    text: &Option<String>,
) -> Result<(), WidgetError> {
    check_text(Some(id), what, false, text.as_deref().unwrap_or(""))
}

/// Required-text form of [`check_text`] (empty strings refused); reports
/// under the owning widget's `id`.
fn check_required_text(id: StableId, what: &'static str, text: &str) -> Result<(), WidgetError> {
    check_text(Some(id), what, true, text)
}

/// Asset-uri cap on a [`IconRef`] (<= [`MAX_ASSET_URI_BYTES`]; the scene
/// asset cap this crate shares — the plugin never sends bytes).
fn check_icon_ref(icon: &IconRef) -> Result<(), WidgetError> {
    if icon.uri.len() > MAX_ASSET_URI_BYTES {
        return Err(WidgetError::PayloadTooLarge {
            kind: "asset uri",
            bytes: icon.uri.len(),
            max: MAX_ASSET_URI_BYTES,
        });
    }
    Ok(())
}

/// Byte cap + confirmation contract for one optional [`ConfirmPolicy`].
fn validate_confirm_policy(
    id: StableId,
    policy: &Option<ConfirmPolicy>,
) -> Result<(), WidgetError> {
    match policy {
        Some(ConfirmPolicy::Prompt { message }) => {
            check_text(Some(id), "confirmation message", false, message)
        }
        None => Ok(()),
    }
}

/// Validate one [`WidgetTreeDef`]'s ordered parent bindings, laziness
/// contract, and per-row payload caps against the surface-wide `seen` set.
///
/// Pass A (ordered, single sweep): duplicates → [`WidgetError::DuplicateWidgetId`];
/// parents must name a PRECEDING row (`Some(parent_index) < index` — the
/// no-forward ordered-binding discipline of the crate's payload
/// validation); self- and forward- declarations fall through to
/// [`WidgetError::ParentMissing`] (forward) / the chain walk (self).
/// Per-row: laziness must claim expandability, labels/status/icon ride
/// their byte caps.
///
/// Pass B (parent chains): per row, walk the parent chain — depth counts
/// chained rows; past [`MAX_SCENE_DEPTH`] or upon re-entering a visited row
/// (the ONLY representable parent cycle after pass A's ordering is a
/// self-parent) → [`WidgetError::DepthExceeded`] with `current` naming the
/// chain depth where the walk recognized the defect (a self-parent
/// re-enters at depth 2).
fn validate_tree_payload(
    rows: &[TreeRowDef],
    seen: &mut HashSet<StableId>,
) -> Result<(), WidgetError> {
    // Pass A: ordered ids + no-forward parent bindings + per-row payload.
    let mut parent_of: HashMap<StableId, Option<StableId>> = HashMap::with_capacity(rows.len());
    let mut index_of: HashMap<StableId, usize> = HashMap::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        register_id(seen, row.id)?;
        if let Some(parent) = row.parent {
            if parent != row.id {
                let ok =
                    matches!(index_of.get(&parent), Some(&parent_index) if parent_index < index);
                if !ok {
                    return Err(WidgetError::ParentMissing(parent));
                }
            }
        }
        parent_of.insert(row.id, row.parent);
        index_of.insert(row.id, index);
        if row.lazy_children && !row.expandable {
            return Err(WidgetError::LazyRowNotExpandable(row.id));
        }
        check_required_text(row.id, "tree row label", &row.label)?;
        check_optional_text(row.id, "tree row status", &row.status)?;
        if let Some(icon) = &row.icon {
            check_icon_ref(icon)?;
        }
    }
    // Pass B: parent-chain acyclicity + depth cap (first crossing).
    for row in rows {
        let mut visited: HashSet<StableId> = HashSet::new();
        let mut current = Some(row.id);
        let mut depth = 0usize;
        while let Some(id) = current {
            depth += 1;
            if depth > MAX_SCENE_DEPTH || !visited.insert(id) {
                // Re-entering a visited row is the parent-cycle shape (the
                // walk would otherwise never terminate); the literal depth
                // crossing is a chain past MAX_SCENE_DEPTH. `current`
                // reports the depth where the walk recognized the defect.
                return Err(WidgetError::DepthExceeded {
                    current: depth,
                    max: MAX_SCENE_DEPTH,
                });
            }
            current = parent_of.get(&id).copied().flatten();
        }
    }
    Ok(())
}

/// Menu-item walk: per-item payload caps and per-item submenu recursion,
/// capped at [`MAX_MENU_DEPTH`], registering every submenu's `MenuDef` id
/// into the surface-wide `seen` namespace (submenus are payload — they
/// carry no [`WidgetNode`] — but their `MenuDef` ids must stay unique).
/// Menu items carry no `StableId`, so a per-item text defect names the
/// OWNING menu widget's id (documented vocabulary choice in
/// [`WidgetError`]).
fn validate_menu_items(
    menu_id: StableId,
    items: &[MenuItemDef],
    seen: &mut HashSet<StableId>,
    depth: usize,
) -> Result<(), WidgetError> {
    if depth > MAX_MENU_DEPTH {
        return Err(WidgetError::DepthExceeded {
            current: depth,
            max: MAX_MENU_DEPTH,
        });
    }
    if items.len() > MAX_WIDGET_ROWS_INITIAL {
        return Err(WidgetError::BudgetExceeded {
            current: items.len(),
            max: MAX_WIDGET_ROWS_INITIAL,
        });
    }
    for item in items {
        check_required_text(menu_id, "menu item label", &item.label)?;
        check_required_text(menu_id, "menu item command", &item.command_id)?;
        check_optional_text(menu_id, "menu item shortcut", &item.shortcut)?;
        if let Some(icon) = &item.icon {
            check_icon_ref(icon)?;
        }
        validate_confirm_policy(menu_id, &item.confirmation)?;
        if let Some(submenu) = &item.submenu {
            register_id(seen, submenu.id)?;
            validate_menu_items(menu_id, &submenu.items, seen, depth + 1)?;
        }
    }
    Ok(())
}

/// Validate a root surface list as one tree: ids, depth, node/budget
/// counts, required texts, and byte caps. Explicit work-stack traversal (the
/// crate's iterative discipline), so a validation pass can never overflow
/// the caller's stack the way the hostile payloads it refuses would.
///
/// One surface-wide `seen` set spans the whole slice: widget node ids, tree
/// row ids, and list item ids are ONE namespace. The same fn is reused for
/// single patch payloads (`Add`/`Update`), where the fresh set then is the
/// payload's own and the collisions against the LIVE set are checked
/// separately by the planner (the scene's `validate_root` pattern).
fn validate_root(nodes: &[WidgetNode]) -> Result<(), WidgetError> {
    let mut seen: HashSet<StableId> = HashSet::new();
    let mut node_count: usize = 0;
    let mut widget_count: usize = 0;
    // Root widgets sit at depth 1; each nested group level adds one.
    let mut stack: Vec<(&WidgetNode, usize)> =
        nodes.iter().rev().map(|node| (node, 1usize)).collect();
    while let Some((node, depth)) = stack.pop() {
        if depth > MAX_SCENE_DEPTH {
            return Err(WidgetError::DepthExceeded {
                current: depth,
                max: MAX_SCENE_DEPTH,
            });
        }
        node_count += 1;
        if node_count > MAX_SCENE_NODES {
            return Err(WidgetError::BudgetExceeded {
                current: node_count,
                max: MAX_SCENE_NODES,
            });
        }
        if !node.is_group() {
            // Non-group = Family-1 semantic control: the per-panel widget
            // budget. Groups are layout containers and ride the node budget
            // only.
            widget_count += 1;
            if widget_count > MAX_WIDGETS_PER_PANEL {
                return Err(WidgetError::BudgetExceeded {
                    current: widget_count,
                    max: MAX_WIDGETS_PER_PANEL,
                });
            }
        }
        match node {
            WidgetNode::Group(group) => {
                register_id(&mut seen, group.id)?;
                check_optional_text(group.id, "widget name", &group.name)?;
                for child in group.children.iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
            WidgetNode::Tree(def) => {
                register_id(&mut seen, def.id)?;
                check_optional_text(def.id, "widget name", &def.name)?;
                check_optional_text(def.id, "tree label", &def.label)?;
                if def.rows.len() > MAX_WIDGET_ROWS_INITIAL {
                    return Err(WidgetError::BudgetExceeded {
                        current: def.rows.len(),
                        max: MAX_WIDGET_ROWS_INITIAL,
                    });
                }
                validate_tree_payload(&def.rows, &mut seen)?;
            }
            WidgetNode::List(def) => {
                register_id(&mut seen, def.id)?;
                check_optional_text(def.id, "widget name", &def.name)?;
                check_optional_text(def.id, "list label", &def.label)?;
                if def.items.len() > MAX_WIDGET_ROWS_INITIAL {
                    return Err(WidgetError::BudgetExceeded {
                        current: def.items.len(),
                        max: MAX_WIDGET_ROWS_INITIAL,
                    });
                }
                for item in &def.items {
                    register_id(&mut seen, item.id)?;
                    check_required_text(item.id, "list item label", &item.label)?;
                    if let Some(command) = &item.command {
                        check_required_text(item.id, "list item command", command)?;
                    }
                    if let Some(icon) = &item.icon {
                        check_icon_ref(icon)?;
                    }
                }
            }
            WidgetNode::IconButton(def) => {
                register_id(&mut seen, def.id)?;
                check_required_text(def.id, "accessible name", &def.accessible_name)?;
                check_optional_text(def.id, "tooltip", &def.tooltip)?;
                check_icon_ref(&def.icon)?;
                check_required_text(def.id, "icon button command", &def.command)?;
                validate_confirm_policy(def.id, &def.confirmation)?;
            }
            WidgetNode::Menu(def) => {
                register_id(&mut seen, def.id)?;
                check_optional_text(def.id, "widget name", &def.name)?;
                check_optional_text(def.id, "menu label", &def.label)?;
                validate_menu_items(def.id, &def.items, &mut seen, 1)?;
            }
            WidgetNode::Table(def) => {
                register_id(&mut seen, def.id)?;
                check_optional_text(def.id, "widget name", &def.name)?;
                // `table_id` deliberately NOT registered in the namespace:
                // it references the typed-table contract (SWG-004) and may
                // legitimately repeat across table refs.
                let _ = def.table_id;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// WidgetError (aligned vocabulary; additive enum — non-wire, like SceneError)
// ---------------------------------------------------------------------------

/// Everything structural validation or patch application rejects about a
/// widget surface, patch, or op — the mirror of
/// [`crate::scene::SceneError`]'s shape (same stale-base/monotonic arms,
/// same first-crossing budget language) at the widget vocabulary.
///
/// NOT on the wire (like [`crate::scene::SceneError`]): hosts and plugins
/// agree on this vocabulary by rustc type identity, not by bytes.
///
/// A structural error leaves the surface completely unchanged: patch
/// application validates the whole op sequence through an internal planning
/// pass and only mutates the tree after the plan completes.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetError {
    /// The patch was built against an outdated surface snapshot.
    StaleBase {
        /// Revision currently applied to the surface.
        current: SceneRevision,
        /// Revision the patch declares as its base.
        given: SceneRevision,
    },
    /// The patch would rewind the surface or not advance it.
    NonMonotonicRevision {
        /// Revision currently applied to the surface.
        current: SceneRevision,
        /// Revision the patch proposes as the successor.
        given: SceneRevision,
    },
    /// A widget/row/item id appears twice in the surface-wide namespace (or
    /// a patch payload reuses a live id).
    DuplicateWidgetId(StableId),
    /// The referenced parent id does not anchor a
    /// [`WidgetNode::Group`] (Add target) — or, in a tree payload, doesn't
    /// name a preceding tree row.
    ParentMissing(StableId),
    /// The referenced widget id does not exist (Update/Remove target).
    ElementMissing(StableId),
    /// A count budget was exceeded (widget nodes, non-group widgets, patch
    /// ops, or a payload vec). First-crossing semantics: `current` is the
    /// count at the first crossing (cap + 1 for walking budgets), never the
    /// true total. With the documented `MAX_PATCH_OPS == MAX_SCENE_NODES`
    /// coincidence, the ops and node crossings are the same endpoint (the
    /// same load-bearing property as the scene's).
    BudgetExceeded {
        /// First-crossing count observed.
        current: usize,
        /// Cap that was exceeded.
        max: usize,
    },
    /// Nesting depth exceeds its cap: widget groups past
    /// [`MAX_SCENE_DEPTH`], row parent-chains past [`MAX_SCENE_DEPTH`] (a
    /// row parent-cycle — the self-parent shape — is reported here with
    /// `current` naming where the walk recognized it), or menu nesting past
    /// [`MAX_MENU_DEPTH`].
    DepthExceeded {
        /// Depth/chain count observed.
        current: usize,
        /// Cap that was exceeded.
        max: usize,
    },
    /// A byte-capped string payload is too long (kind names the field:
    /// `"accessible name"`, `"asset uri"`, `…`). Menu-item payloads carry
    /// no per-item id, so their reports reuse the owning menu widget's
    /// error shape without an id here.
    PayloadTooLarge {
        /// Which capped payload kind.
        kind: &'static str,
        /// Byte count observed.
        bytes: usize,
        /// Cap that was exceeded.
        max: usize,
    },
    /// A tree row declares `lazy_children: true` without `expandable: true`
    /// — the host could never know to issue
    /// [`WidgetEvent::ChildrenRequested`].
    LazyRowNotExpandable(StableId),
    /// A required visible/semantic text arrives empty (`what` names the
    /// field: `"accessible name"`, `"icon button command"`, `"tree row
    /// label"`, `"list item label"`, `"menu item command"`, `"event
    /// command id"`, …). Reported under the owning widget's id (menu
    /// items carry no `StableId` and report their owning MENU's id;
    /// event-side texts report `None`).
    EmptyRequiredText {
        /// The owning widget id (`None` for event-side texts).
        id: Option<StableId>,
        /// Which required text is missing.
        what: &'static str,
    },
    /// A typed-table CELL defect (SWG-004, appended at the enum tail;
    /// non-wire like the whole enum): an address-ful (table, row key,
    /// column) rejection from the registry contract. Fires for —
    /// unknown row key / unknown column id in a patch; a cell whose kind
    /// does not match its column's [`CellKind`]; a patch naming a column
    /// kind that is not patch-writable through the value skeleton; a
    /// row whose cells do not shadow the column registry 1:1 (the
    /// undershoot names the first uncovered column, the overshoot has no
    /// column to name); a duplicate row key / duplicate column id inside
    /// one table; a cell's own cap crossing (text bytes, uri bytes,
    /// non-finite or out-of-range progress/dates).
    ///
    /// ADDRESS CONVENTION (this variant is the ONLY address-ful table
    /// arm): `table` always names the registry table; for a PER-CELL
    /// defect `key` + `column` name the cell address; for a DEF-shape
    /// defect with no row axis (column-registry issues) `key` carries the
    /// offending id and `column` repeats it; for a row-shape defect with
    /// no column to name (overshoot) `column` repeats the offending row
    /// key. `reason` states the defect class in words — it is the
    /// authoritative field, the address fields only lobby.
    ///
    /// Compatibility: appended at the tail; the enum is non-wire, so
    /// nothing already-written re-orders.
    InvalidCell {
        /// The registry table the defect addresses.
        table: StableId,
        /// The row key of the cell (or the involved id per the address
        /// convention).
        key: StableId,
        /// The column id of the cell (or the repeated involved id per
        /// the address convention).
        column: StableId,
        /// The defect class in words (authoritative).
        reason: String,
    },
    /// A table identity does not resolve in the surface's registry
    /// (SWG-004 tail append after [`WidgetError::InvalidCell`]; non-wire):
    /// a [`TableRef::table_id`] naming an id the registry does not serve
    /// (validate + patch payloads), or a registry entry whose declared
    /// `id_table` is not its own map key.
    ///
    /// Compatibility: appended at the tail; the enum is non-wire.
    UnknownTableRef(StableId),
}

impl fmt::Display for WidgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WidgetError::StaleBase { current, given } => write!(
                f,
                "stale patch base: the widget surface is at revision {} but the patch was built against {}",
                current.0, given.0
            ),
            WidgetError::NonMonotonicRevision { current, given } => write!(
                f,
                "non-monotonic revision: the widget surface is at revision {} and the patch proposes {}",
                current.0, given.0
            ),
            WidgetError::DuplicateWidgetId(id) => {
                write!(f, "duplicate widget id {} in the surface-wide namespace", id.0)
            }
            WidgetError::ParentMissing(id) => write!(
                f,
                "widget id {} does not anchor a group (Add parent id) or does not name a preceding tree row",
                id.0
            ),
            WidgetError::ElementMissing(id) => {
                write!(f, "widget id {} does not exist", id.0)
            }
            WidgetError::BudgetExceeded { current, max } => write!(
                f,
                "widget budget exceeded: {current} entries pass the cap {max}"
            ),
            WidgetError::DepthExceeded { current, max } => write!(
                f,
                "widget depth exceeded: nesting/chain depth {current} passes the cap {max}"
            ),
            WidgetError::PayloadTooLarge { kind, bytes, max } => write!(
                f,
                "widget payload too large ({kind}): {bytes} bytes pass the cap {max}"
            ),
            WidgetError::LazyRowNotExpandable(id) => write!(
                f,
                "tree row {} declares lazy_children without expandable",
                id.0
            ),
            WidgetError::EmptyRequiredText { id, what } => match id {
                Some(id) => write!(f, "widget id {} carries an empty required text ({what})", id.0),
                None => write!(f, "a widget event carries an empty required text ({what})"),
            },
            WidgetError::InvalidCell {
                table,
                key,
                column,
                reason,
            } => write!(
                f,
                "invalid table cell (table {} row-key {} column {}): {reason}",
                table.0, key.0, column.0
            ),
            WidgetError::UnknownTableRef(id) => {
                write!(f, "table identity {} does not resolve in the surface registry", id.0)
            }
        }
    }
}

impl std::error::Error for WidgetError {}

// ---------------------------------------------------------------------------
// Typed tables (SWG-004): column/cell vocabulary, row/table defs, registry
// ---------------------------------------------------------------------------

/// Column DATA KIND of one [`TableColumnDef`] — the vocabulary a cell write
/// must MATCH (`WidgetOp::TableCell`'s skeleton value resolves into the
/// typed [`TableCell`] of exactly this kind).
///
/// Widens the three-variant [`WidgetCellValue`] skeleton to the full cell
/// vocabulary the addendum's "typed cells" demands; only the scalar kinds
/// (`Text`/`Number`/`Bool`) are patch-writable through the skeleton — the
/// presentation kinds (`Icon`/`Link`/`Badge`/`Progress`/`Date`) are
/// plugin-published through the registry.
///
/// Compatibility rule: new kinds appended at the end; bincode discriminants
/// pinned `0..=7`; serde tags lowercase (pinned by the tests in this
/// module).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CellKind {
    /// Text (byte-capped like every widget text).
    Text,
    /// Numeric (finite; renderable numeral).
    Number,
    /// Boolean.
    Bool,
    /// Host-managed icon reference ([`IconRef`]; the plugin never sends
    /// bytes).
    Icon,
    /// Hyperlink: visible `label` + host-loaded `uri` (byte-capped like a
    /// scene asset uri).
    Link,
    /// Status chip: `label` + [`BadgeTone`].
    Badge,
    /// Progress meter: finite `fraction` in `0..=1` + optional `label`.
    Progress,
    /// Calendar day (`y`/`m`/`d`); magnitude gates `1..=12` months and
    /// `1..=31` days (month-length/calendar accuracy is the host's
    /// rendering business).
    Date,
}

impl fmt::Display for CellKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Bool => "bool",
            Self::Icon => "icon",
            Self::Link => "link",
            Self::Badge => "badge",
            Self::Progress => "progress",
            Self::Date => "date",
        };
        f.write_str(name)
    }
}

/// Tone of a [`TableCell::Badge`] (presentation hint; the host maps it to
/// its chip palette).
///
/// Compatibility rule: new tones appended; bincode discriminants pinned
/// `0..=4`; serde tags lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BadgeTone {
    /// Informational.
    Info,
    /// Success/healthy.
    Success,
    /// Warning.
    Warning,
    /// Danger/error.
    Danger,
    /// No tone (plain chip).
    Neutral,
}

impl fmt::Display for BadgeTone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::Neutral => "neutral",
        };
        f.write_str(name)
    }
}

/// One TYPED cell of a [`TableRowDef`] — the real cell value the addendum's
/// "typed cells" names (the [`WidgetCellValue`] skeleton stays the
/// event-side read/notify vocabulary; the split is documented in the
/// [module docs](self)).
///
/// Cell identity: cells are addressed (table, row key, column id) and
/// shadow their table's columns 1:1 BY POSITION in the row's `cells` vec —
/// the column registry owns id→position mapping ([`PanelTableDef::column_position`]);
/// a cell never names its own column on the wire.
///
/// Compatibility rule: new variants appended; bincode discriminants pinned
/// `0..=7`; serde tags lowercase (pinned from creation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TableCell {
    /// Plain text cell (byte-capped like every widget text).
    Text(#[serde(deserialize_with = "capped_text_cell")] String),
    /// Numeric cell (finite: a renderable numeral — a non-finite is a
    /// producer defect, refused at validation).
    #[serde(deserialize_with = "finite_number_cell")]
    Number(f64),
    /// Boolean cell.
    Bool(bool),
    /// Host-managed icon (see [`IconRef`]; the plugin never sends bytes).
    Icon(#[serde(deserialize_with = "capped_icon_cell")] IconRef),
    /// Hyperlink: the visible text plus the host-loaded uri (≤
    /// [`MAX_ASSET_URI_BYTES`], the scene asset cap — the plugin never
    /// sends bytes).
    Link {
        /// Visible label of the link (byte-capped).
        #[serde(deserialize_with = "capped_text_cell")]
        label: String,
        /// Host-loaded asset/uri reference (≤ [`MAX_ASSET_URI_BYTES`]).
        #[serde(deserialize_with = "capped_link_uri")]
        uri: String,
    },
    /// Status chip: label + tone.
    Badge {
        /// Visible chip text (byte-capped).
        #[serde(deserialize_with = "capped_text_cell")]
        label: String,
        /// Presentation tone.
        tone: BadgeTone,
    },
    /// Progress meter.
    Progress {
        /// Completed fraction; MUST be finite within `0.0..=1.0`
        /// (validation rejects the rest — meter arithmetic is contract,
        /// not presentation).
        #[serde(deserialize_with = "capped_progress_fraction")]
        fraction: f32,
        /// Optional status text (byte-capped).
        #[serde(deserialize_with = "capped_optional_text_cell")]
        label: Option<String>,
    },
    /// Calendar day. Magnitude gates only: `m: 1..=12`, `d: 1..=31`;
    /// month-length/calendar accuracy is the host's rendering business.
    Date {
        /// Year.
        y: u16,
        /// Month `1..=12` (validation rejects `13`+).
        #[serde(deserialize_with = "bounded_date_month")]
        m: u8,
        /// Day `1..=31` (validation rejects `0` and `32`+).
        #[serde(deserialize_with = "bounded_date_day")]
        d: u8,
    },
}

impl TableCell {
    /// The cell's data kind (the column kind a cell write must match).
    pub const fn kind(&self) -> CellKind {
        match self {
            Self::Text(_) => CellKind::Text,
            Self::Number(_) => CellKind::Number,
            Self::Bool(_) => CellKind::Bool,
            Self::Icon(_) => CellKind::Icon,
            Self::Link { .. } => CellKind::Link,
            Self::Badge { .. } => CellKind::Badge,
            Self::Progress { .. } => CellKind::Progress,
            Self::Date { .. } => CellKind::Date,
        }
    }
}

// -- bounded cell decode helpers (the bounded vec pattern, typed) ----------

/// String-cell decode cap ([`MAX_TEXT_BYTES`]) — every capped TEXT on a
/// cell uses this helper.
fn capped_text_cell<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.len() > MAX_TEXT_BYTES {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "table cell text exceeds the decoded payload budget ({MAX_TEXT_BYTES})"
        )));
    }
    Ok(text)
}

/// Optional-text-cell decode cap (same [`MAX_TEXT_BYTES`] magnitude).
fn capped_optional_text_cell<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let text = Option::<String>::deserialize(deserializer)?;
    if let Some(text) = &text {
        if text.len() > MAX_TEXT_BYTES {
            return Err(<D::Error as serde::de::Error>::custom(format!(
                "table cell text exceeds the decoded payload budget ({MAX_TEXT_BYTES})"
            )));
        }
    }
    Ok(text)
}

/// Link-uri decode cap ([`MAX_ASSET_URI_BYTES`] — the scene asset cap the
/// plugin-side uri rides).
fn capped_link_uri<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let uri = String::deserialize(deserializer)?;
    if uri.len() > MAX_ASSET_URI_BYTES {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "table cell uri exceeds the decoded payload budget ({MAX_ASSET_URI_BYTES})"
        )));
    }
    Ok(uri)
}

/// Icon-cell decode: reuses the icon uri cap check at decode (the uri cap
/// of [`IconRef`]).
fn capped_icon_cell<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<IconRef, D::Error> {
    let icon = IconRef::deserialize(deserializer)?;
    if icon.uri.len() > MAX_ASSET_URI_BYTES {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "table cell icon uri exceeds the decoded payload budget ({MAX_ASSET_URI_BYTES})"
        )));
    }
    Ok(icon)
}

/// Numeric-cell decode: REJECTS non-finite at decode (a renderable numeral
/// is contract, not presentation — and JSON could not express it anyway).
fn finite_number_cell<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let n = f64::deserialize(deserializer)?;
    if !n.is_finite() {
        return Err(<D::Error as serde::de::Error>::custom(
            "table cell number must be finite",
        ));
    }
    Ok(n)
}

/// Progress-fraction decode: finite and within `0.0..=1.0`.
fn capped_progress_fraction<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<f32, D::Error> {
    let fraction = f32::deserialize(deserializer)?;
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return Err(<D::Error as serde::de::Error>::custom(
            "table cell progress fraction must be finite within 0..=1",
        ));
    }
    Ok(fraction)
}

/// Date-month decode gate (`1..=12`).
fn bounded_date_month<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let m = u8::deserialize(deserializer)?;
    if !(1..=12).contains(&m) {
        return Err(<D::Error as serde::de::Error>::custom(
            "table cell date month must be within 1..=12",
        ));
    }
    Ok(m)
}

/// Date-day decode gate (`1..=31`, magnitude only).
fn bounded_date_day<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let d = u8::deserialize(deserializer)?;
    if !(1..=31).contains(&d) {
        return Err(<D::Error as serde::de::Error>::custom(
            "table cell date day must be within 1..=31",
        ));
    }
    Ok(d)
}

/// Cell-level structural checks, address-ful: every cap/gate of
/// [`TableCell`] reported as [`WidgetError::InvalidCell`] (the decode
/// helpers already refuse hostile frames; this validates the hand-authored
/// leg with the full (table, key, column) address in the report).
fn check_table_cell(
    table: StableId,
    key: StableId,
    column: StableId,
    cell: &TableCell,
) -> Result<(), WidgetError> {
    let invalid = |reason: String| {
        Err(WidgetError::InvalidCell {
            table,
            key,
            column,
            reason,
        })
    };
    match cell {
        TableCell::Text(text) => {
            if text.len() > MAX_TEXT_BYTES {
                return invalid(format!(
                    "cell text is {} bytes past the {MAX_TEXT_BYTES}-byte cap",
                    text.len()
                ));
            }
        }
        TableCell::Number(n) => {
            if !n.is_finite() {
                return invalid("number cell must be finite".to_string());
            }
        }
        TableCell::Bool(_) => {}
        TableCell::Icon(icon) => {
            if icon.uri.len() > MAX_ASSET_URI_BYTES {
                return invalid(format!(
                    "icon uri is {} bytes past the {MAX_ASSET_URI_BYTES}-byte cap",
                    icon.uri.len()
                ));
            }
        }
        TableCell::Link { label, uri } => {
            if label.len() > MAX_TEXT_BYTES {
                return invalid(format!(
                    "link label is {} bytes past the {MAX_TEXT_BYTES}-byte cap",
                    label.len()
                ));
            }
            if uri.len() > MAX_ASSET_URI_BYTES {
                return invalid(format!(
                    "link uri is {} bytes past the {MAX_ASSET_URI_BYTES}-byte cap",
                    uri.len()
                ));
            }
        }
        TableCell::Badge { label, .. } => {
            if label.len() > MAX_TEXT_BYTES {
                return invalid(format!(
                    "badge label is {} bytes past the {MAX_TEXT_BYTES}-byte cap",
                    label.len()
                ));
            }
        }
        TableCell::Progress { fraction, label } => {
            if !fraction.is_finite() || !(0.0..=1.0).contains(fraction) {
                return invalid("progress fraction must be finite within 0.0..=1.0".to_string());
            }
            if let Some(label) = label {
                if label.len() > MAX_TEXT_BYTES {
                    return invalid(format!(
                        "progress label is {} bytes past the {MAX_TEXT_BYTES}-byte cap",
                        label.len()
                    ));
                }
            }
        }
        TableCell::Date { m, d, .. } => {
            if !(1..=12).contains(m) {
                return invalid(format!("date month {m} is outside 1..=12"));
            }
            if !(1..=31).contains(d) {
                return invalid(format!("date day {d} is outside 1..=31"));
            }
        }
    }
    Ok(())
}

/// One COLUMN of a [`PanelTableDef`]: stable id (the patch/registry
/// address), visible label, the DATA KIND its cells must carry, and
/// presentation hints only (width/stretch are HOST decisions — a
/// nonsensical hint is clampable, never validated: the split_ratio
/// doctrine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableColumnDef {
    /// Stable column id — the address cell patches name. Unique within the
    /// table (per-table registry; a duplicate refuses as
    /// [`WidgetError::InvalidCell`]).
    pub id: StableId,
    /// Visible header label (REQUIRED, non-empty; byte-capped).
    pub label: String,
    /// The kind every cell of this column must carry.
    pub kind: CellKind,
    /// Preferred width in logical units (presentation HINT; the host may
    /// clamp or ignore it).
    #[serde(default)]
    pub width_hint: Option<f32>,
    /// The column may stretch into leftover width (presentation hint).
    #[serde(default)]
    pub stretch: bool,
}

/// One ROW of a [`PanelTableDef`]'s published window: a stable key (the
/// patch/event address — never an index; WGT-ID forbids index identity) and
/// its cells, one per column, shadowing the column registry 1:1 BY
/// POSITION. No per-row interaction state (expansion/selection is
/// host-local; the def is pure data).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableRowDef {
    /// Stable row key (unique within THIS table's registry; different
    /// tables may freely re-use keys — the per-table namespace is the
    /// registry keyed by table id). A duplicate refuses as
    /// [`WidgetError::InvalidCell`].
    pub key: StableId,
    /// The row's cells — exactly one per column ([`PanelTableDef::columns`]),
    /// shadowing by POSITION. Budget: one payload-vec cap at decode
    /// ([`budgeted_table_cells`]); the exact equality is validated by
    /// [`PanelTableDef::validate`].
    #[serde(deserialize_with = "budgeted_table_cells")]
    pub cells: Vec<TableCell>,
    /// The EXPLICIT domain-object mapping ([`DomainRef`]); absent on pure
    /// presentation rows.
    #[serde(default)]
    pub domain_ref: Option<DomainRef>,
}

/// One typed TABLE definition of the surface's registry (`SWG-004`): the
/// target every [`WidgetNode::Table`] reference resolves against.
///
/// Identity: `id_table` is the table's id — FOREIGN to the surface-wide
/// widget namespace (WGT-ID). The registry is a per-surface map keyed by
/// EXACTLY this id ([`WidgetSurface::tables`]); a def whose `id_table`
/// disagrees with its map key is a defect
/// ([`WidgetError::UnknownTableRef`]).
///
/// Paging: `total_rows: Some(total)` states the backing set's size while
/// `rows` carries the published window (≤ [`MAX_WIDGET_ROWS_INITIAL`] per
/// payload); the host asks for further windows through
/// [`WidgetEvent::TablePageRequested`]. `None` = the table is fully
/// materialized (small tables).
///
/// State classes: this def is pure DATA — row selection, scroll position,
/// expansion/draft state are HOST-LOCAL (ARCHITECTURE §3 state class 3)
/// and deliberately absent here.
///
/// NOT part of the widget-node namespace: table ids never collide with
/// widget ids by construction (separate registry map), and
/// [`WidgetError::DuplicateWidgetId`] never fires for them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelTableDef {
    /// Stable table id (the [`TableRef::table_id`] target; the registry
    /// map's key).
    pub id_table: StableId,
    /// Column vocabulary (stable column ids — the addresses cell patches
    /// name). Cells shadow these 1:1 by position; the id→position mapping
    /// is [`PanelTableDef::column_position`].
    #[serde(deserialize_with = "budgeted_table_columns")]
    pub columns: Vec<TableColumnDef>,
    /// The backing set's total when the def windows (`None` = fully
    /// materialized).
    pub total_rows: Option<u64>,
    /// The published row window (bounded
    /// [`MAX_WIDGET_ROWS_INITIAL`]/payload — paged backing data keeps keys
    /// stable across windows).
    #[serde(deserialize_with = "budgeted_table_rows")]
    pub rows: Vec<TableRowDef>,
}

impl PanelTableDef {
    /// The position a column id shadows (the mapping cell patches rely
    /// on); `None` when the id is not in the column registry.
    pub fn column_position(&self, id: StableId) -> Option<usize> {
        self.columns.iter().position(|column| column.id == id)
    }

    /// Per-table structural validation: unique column ids and row keys
    /// (per-table registries → [`WidgetError::InvalidCell`] per the
    /// address convention), every row's cells shadowing the column
    /// registry 1:1 BY position with MATCHING kinds, and the per-cell
    /// caps/gates of [`check_table_cell`]. Budget: rows past
    /// [`MAX_WIDGET_ROWS_INITIAL`] refuse as [`WidgetError::BudgetExceeded`]
    /// (first crossing; matching the surface/decode levels).
    pub fn validate(&self) -> Result<(), WidgetError> {
        if self.rows.len() > MAX_WIDGET_ROWS_INITIAL {
            return Err(WidgetError::BudgetExceeded {
                current: self.rows.len(),
                max: MAX_WIDGET_ROWS_INITIAL,
            });
        }
        // Column registry: unique ids + label contract.
        let mut columns: HashMap<StableId, CellKind> = HashMap::with_capacity(self.columns.len());
        for column in &self.columns {
            if columns.contains_key(&column.id) {
                return Err(WidgetError::InvalidCell {
                    table: self.id_table,
                    key: column.id,
                    column: column.id,
                    reason: "duplicate column id in the column registry".to_string(),
                });
            }
            if column.label.is_empty() {
                return Err(WidgetError::InvalidCell {
                    table: self.id_table,
                    key: column.id,
                    column: column.id,
                    reason: "empty column label".to_string(),
                });
            }
            if column.label.len() > MAX_TEXT_BYTES {
                return Err(WidgetError::InvalidCell {
                    table: self.id_table,
                    key: column.id,
                    column: column.id,
                    reason: format!(
                        "column label is {} bytes past the {MAX_TEXT_BYTES}-byte cap",
                        column.label.len()
                    ),
                });
            }
            columns.insert(column.id, column.kind);
        }
        // Row registry: unique keys + 1:1 kind-matched cell shadowing.
        let mut keys: HashSet<StableId> = HashSet::with_capacity(self.rows.len());
        for row in &self.rows {
            if !keys.insert(row.key) {
                return Err(WidgetError::InvalidCell {
                    table: self.id_table,
                    key: row.key,
                    column: row.key,
                    reason: "duplicate row key in the published window".to_string(),
                });
            }
            if row.cells.len() < self.columns.len() {
                // Undershoot: name the FIRST column the row does not cover.
                let first_uncovered = &self.columns[row.cells.len()];
                return Err(WidgetError::InvalidCell {
                    table: self.id_table,
                    key: row.key,
                    column: first_uncovered.id,
                    reason: format!(
                        "row carries {} cells for a {}-column registry (does not shadow every column)",
                        row.cells.len(),
                        self.columns.len()
                    ),
                });
            }
            if row.cells.len() > self.columns.len() {
                return Err(WidgetError::InvalidCell {
                    table: self.id_table,
                    key: row.key,
                    column: row.key,
                    reason: format!(
                        "row carries {} cells for a {}-column registry (no column shadows the extra cells)",
                        row.cells.len(),
                        self.columns.len()
                    ),
                });
            }
            for (pos, cell) in row.cells.iter().enumerate() {
                let column = &self.columns[pos];
                if cell.kind() != column.kind {
                    return Err(WidgetError::InvalidCell {
                        table: self.id_table,
                        key: row.key,
                        column: column.id,
                        reason: format!(
                            "cell kind `{}` does not match column kind `{}`",
                            cell.kind(),
                            column.kind
                        ),
                    });
                }
                check_table_cell(self.id_table, row.key, column.id, cell)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Typed-table patch planning (cell writes; the apply_patch family split)
// ---------------------------------------------------------------------------

/// One planned cell write — the journal the table planner produces and
/// [`apply_table_writes`] replays. Positions are resolved against the
/// CURRENT registry at plan time; cell writes never alter table/row/column
/// structure, so the positions stay valid through the replay and the phase-2
/// step is infallible by construction.
#[derive(Debug)]
struct PlannedTableWrite {
    /// The registry table under write.
    table: StableId,
    /// Resolved row position (the `rows` vec index of the keyed row).
    row_pos: usize,
    /// Resolved cell position (== the column position the registry maps
    /// the column id to).
    cell_pos: usize,
    /// The typed replacement (kind-resolved + cap-validated at plan time).
    cell: TableCell,
}

/// Phase 1 of the table half of `apply_patch`: validate each
/// [`WidgetOp::TableCell`] against the CURRENT registry and journal the
/// pre-resolved writes. No mutation of `tables` happens here, and none is
/// needed during planning: cell writes replace VALUES only (structure is
/// untouched), so later ops in the same patch resolve identically (the
/// last write to one cell wins at replay).
///
/// Rejections: an unknown TABLE → [`WidgetError::UnknownTableRef`]; an
/// unknown row key / column id, a kind mismatch, a non-writable column
/// kind, or a cap crossing on the replacement →
/// [`WidgetError::InvalidCell`] with the full address.
fn plan_table_writes(
    tables: &BTreeMap<StableId, PanelTableDef>,
    ops: Vec<WidgetOp>,
) -> Result<Vec<PlannedTableWrite>, WidgetError> {
    let mut journal = Vec::with_capacity(ops.len());
    for op in ops {
        let WidgetOp::TableCell {
            table,
            key,
            column,
            value,
        } = op
        else {
            // The caller's family split routes non-cell ops to the widget
            // planner; a structural op reaching here is planner corruption.
            unreachable!("table planner receives WidgetOp::TableCell ops only");
        };
        let Some(def) = tables.get(&table) else {
            return Err(WidgetError::UnknownTableRef(table));
        };
        let Some(row_pos) = def.rows.iter().position(|row| row.key == key) else {
            return Err(WidgetError::InvalidCell {
                table,
                key,
                column,
                reason: "no such row key in the published window".to_string(),
            });
        };
        let Some(cell_pos) = def.column_position(column) else {
            return Err(WidgetError::InvalidCell {
                table,
                key,
                column,
                reason: "no such column id in the column registry".to_string(),
            });
        };
        let cell = planned_cell_for(table, key, column, def.columns[cell_pos].kind, value)?;
        journal.push(PlannedTableWrite {
            table,
            row_pos,
            cell_pos,
            cell,
        });
    }
    Ok(journal)
}

/// The typed replacement a [`WidgetOp::TableCell`] writes into a column of
/// `kind`: the op carries the three-variant skeleton, so exactly the scalar
/// column kinds are patch-writable — icon/link/badge/progress/date columns
/// are plugin-PUBLISHED through the registry (see the [module docs] split)
/// and a write to one refuses with [`WidgetError::InvalidCell`]. The scalar
/// legs re-run the cell caps/gates so the REPLACEMENT is as valid as the
/// published def cells.
fn planned_cell_for(
    table: StableId,
    key: StableId,
    column: StableId,
    kind: CellKind,
    value: WidgetCellValue,
) -> Result<TableCell, WidgetError> {
    let cell = match (kind, value) {
        (CellKind::Text, WidgetCellValue::Text(text)) => TableCell::Text(text),
        (CellKind::Number, WidgetCellValue::Number(n)) => TableCell::Number(n),
        (CellKind::Bool, WidgetCellValue::Bool(b)) => TableCell::Bool(b),
        (kind, _) => {
            return Err(WidgetError::InvalidCell {
                table,
                key,
                column,
                reason: format!(
                    "column kind `{kind}` is not writable through the value skeleton (published through the registry)"
                ),
            });
        }
    };
    check_table_cell(table, key, column, &cell)?;
    Ok(cell)
}

/// Phase 2 of the table half of `apply_patch`: replay the pre-validated
/// journal into the REAL registry, strictly in order, with no further
/// decisions. Every step is infallible by construction (positions were
/// resolved at plan time and cell writes do not move structure; a
/// disagreement would be planner corruption and panics, matching the
/// widget journal's unreachable discipline).
fn apply_table_writes(
    tables: &mut BTreeMap<StableId, PanelTableDef>,
    journal: Vec<PlannedTableWrite>,
) {
    for PlannedTableWrite {
        table,
        row_pos,
        cell_pos,
        cell,
    } in journal
    {
        let def = tables
            .get_mut(&table)
            .expect("journal table resolved at plan time");
        def.rows[row_pos].cells[cell_pos] = cell;
    }
}

/// Every [`WidgetNode::Table`] reference's `table_id` resolving against
/// `tables` (one walk, iterative — the same explicit-stack discipline as
/// `validate_root`). Refs are checked WHERE THE TARGET LIVES: standalone
/// payload validation ([`validate_root`]) stays registry-blind; this helper
/// receives the registry (the surface gate and the patch planner both have
/// it) so the surface-wide rule "every table identity resolves" holds for
/// snapshot and patch alike.
///
/// PLACEHOLDER TRANSITION: while `tables` is EMPTY this check is a no-op —
/// `SWG-001` shipped refs into a contract that did not exist yet and its
/// creation surfaces (append-only, unmodifiable) encode that era; the FIRST
/// registry entry engages full resolution for the surface and every patch
/// payload applied to it (see the [module docs](self)).
fn validate_table_refs(
    nodes: &[WidgetNode],
    tables: &BTreeMap<StableId, PanelTableDef>,
) -> Result<(), WidgetError> {
    if tables.is_empty() {
        return Ok(());
    }
    let mut stack: Vec<&WidgetNode> = nodes.iter().collect();
    while let Some(node) = stack.pop() {
        if let WidgetNode::Table(def) = node {
            if !tables.contains_key(&def.table_id) {
                return Err(WidgetError::UnknownTableRef(def.table_id));
            }
        } else if let WidgetNode::Group(group) = node {
            stack.extend(group.children.iter());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- construction helpers ------------------------------------------------

    fn sid(n: u64) -> StableId {
        StableId(n)
    }

    fn icon_ref(uri: &str) -> IconRef {
        IconRef {
            uri: uri.to_string(),
            size: default_icon_size(),
        }
    }

    fn trow(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
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

    fn lazy_row(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
        TreeRowDef {
            expandable: true,
            lazy_children: true,
            ..trow(id, parent, label)
        }
    }

    fn wtree(id: u64, rows: Vec<TreeRowDef>) -> WidgetNode {
        WidgetNode::Tree(WidgetTreeDef {
            id: sid(id),
            name: Some(format!("tree-{id}")),
            enabled: true,
            visible: true,
            label: None,
            capabilities: TreeCapabilities::default(),
            rows,
        })
    }

    fn litem(id: u64, label: &str) -> ItemDef {
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

    fn wlist(id: u64, items: Vec<ItemDef>) -> WidgetNode {
        WidgetNode::List(ItemListDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            items,
        })
    }

    fn wicon(id: u64, command: &str) -> WidgetNode {
        WidgetNode::IconButton(IconActionDef {
            id: sid(id),
            accessible_name: format!("icon button {id}"),
            tooltip: None,
            icon: icon_ref("svg:g"),
            enabled: true,
            visible: true,
            active: false,
            command: command.to_string(),
            confirmation: None,
        })
    }

    fn mitem(command_id: &str, label: &str) -> MenuItemDef {
        MenuItemDef {
            command_id: command_id.to_string(),
            label: label.to_string(),
            icon: None,
            shortcut: None,
            enabled: true,
            visible: true,
            submenu: None,
            confirmation: None,
        }
    }

    fn wmenu(id: u64, items: Vec<MenuItemDef>) -> WidgetNode {
        WidgetNode::Menu(MenuDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            items,
        })
    }

    fn wtable(id: u64, table_id: u64) -> WidgetNode {
        WidgetNode::Table(TableRef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            table_id: sid(table_id),
        })
    }

    fn wgroup(
        id: u64,
        direction: Axis,
        split_ratio: Option<f32>,
        children: Vec<WidgetNode>,
    ) -> WidgetNode {
        WidgetNode::Group(WidgetGroup {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            direction,
            children,
            split_ratio,
        })
    }

    fn wsurface(revision: u64, roots: Vec<WidgetNode>) -> WidgetSurface {
        WidgetSurface {
            revision: SceneRevision(revision),
            roots,
            // SWG-004 trailing registry field; the pre-existing widget tests
            // publish no tables (the only line touch the registry decision
            // forced on this module's test plumbing — assertions untouched).
            tables: BTreeMap::new(),
        }
    }

    fn wpatch(base: u64, next: u64, ops: Vec<WidgetOp>) -> WidgetSurfacePatch {
        WidgetSurfacePatch {
            base_revision: SceneRevision(base),
            revision: SceneRevision(next),
            ops,
        }
    }

    /// Snapshot of a surface for deep-compare before/after assertions.
    fn shot(surface: &WidgetSurface) -> (SceneRevision, Vec<WidgetNode>) {
        (surface.revision, surface.roots.clone())
    }

    fn next_id(next: &mut u64) -> StableId {
        let id = StableId(*next);
        *next += 1;
        id
    }

    /// A chain of `groups` nested groups closing on an icon button; deepest
    /// node ends at depth `groups + 1`.
    fn nested_chain(groups: usize, next: &mut u64) -> WidgetNode {
        let mut node = wicon(next_id(next).0, "noop");
        for _ in 0..groups {
            node = wgroup(next_id(next).0, Axis::Vertical, None, vec![node]);
        }
        node
    }

    /// A `levels`-deep MenuDef chain (the TOP widget menu included): the last
    /// level's item submenu is `None`; every level's item carries a Prompt
    /// confirmation so the chain exercises the confirmation contract too.
    fn nested_menu(id_base: u64, levels: usize) -> WidgetNode {
        assert!(levels >= 1 && levels <= MAX_MENU_DEPTH);
        let mut submenu: Option<Box<MenuDef>> = None;
        for level in (1..=levels).rev() {
            let menu = MenuDef {
                id: sid(id_base + level as u64),
                name: None,
                enabled: true,
                visible: true,
                label: None,
                items: vec![MenuItemDef {
                    command_id: format!("cmd-{level}"),
                    label: format!("level {level}"),
                    icon: None,
                    shortcut: None,
                    enabled: true,
                    visible: true,
                    submenu: submenu.take(),
                    confirmation: Some(ConfirmPolicy::Prompt {
                        message: format!("confirm {level}?"),
                    }),
                }],
            };
            submenu = Some(Box::new(menu));
        }
        WidgetNode::Menu(*submenu.expect("at least one menu level"))
    }

    #[test]
    fn widget_surface_bincode_and_json_roundtrips_representative_surface() {
        // One surface carrying every WidgetNode kind: a tree (incl. a lazy
        // row + explicit domain refs), a list (incl. a command-tied item), an
        // icon button (incl. confirmation), a menu with a submenu chain and
        // confirmations, a table REFERENCE, and vertical/horizontal groups —
        // one of them a split (split_ratio). NO Scene variant: the scene
        // cell composes at the HOST dock (ADR-005); the reference layout's
        // scene pane stays a Group placeholder below.
        let surface = WidgetSurface {
            revision: SceneRevision(9),
            roots: vec![wgroup(
                1,
                Axis::Vertical,
                None,
                vec![
                    wgroup(
                        2,
                        Axis::Horizontal,
                        None,
                        vec![
                            WidgetNode::IconButton(IconActionDef {
                                id: sid(3),
                                accessible_name: "filter results".to_string(),
                                tooltip: Some("filter".to_string()),
                                icon: icon_ref("svg:funnel"),
                                enabled: true,
                                visible: true,
                                active: false,
                                command: "filter.toggle".to_string(),
                                confirmation: Some(ConfirmPolicy::Prompt {
                                    message: "apply the filter?".to_string(),
                                }),
                            }),
                            WidgetNode::Menu(MenuDef {
                                id: sid(4),
                                name: Some("settings".to_string()),
                                enabled: true,
                                visible: true,
                                label: Some("Settings".to_string()),
                                items: vec![
                                    mitem("settings.reset", "Reset layout"),
                                    MenuItemDef {
                                        command_id: "settings.export".to_string(),
                                        label: "Export".to_string(),
                                        icon: Some(icon_ref("svg:export")),
                                        shortcut: Some("Ctrl+E".to_string()),
                                        enabled: false,
                                        visible: true,
                                        submenu: Some(Box::new(MenuDef {
                                            id: sid(5),
                                            name: None,
                                            enabled: true,
                                            visible: true,
                                            label: Some("Export".to_string()),
                                            items: vec![MenuItemDef {
                                                command_id: "settings.export.png".to_string(),
                                                label: "As PNG".to_string(),
                                                icon: None,
                                                shortcut: None,
                                                enabled: true,
                                                visible: true,
                                                submenu: None,
                                                confirmation: Some(ConfirmPolicy::Prompt {
                                                    message: "export as PNG?".to_string(),
                                                }),
                                            }],
                                        })),
                                        confirmation: None,
                                    },
                                ],
                            }),
                        ],
                    ),
                    wtree(
                        6,
                        vec![
                            TreeRowDef {
                                id: sid(7),
                                parent: None,
                                label: "Alignments".to_string(),
                                icon: Some(icon_ref("svg:alignment")),
                                status: Some("3".to_string()),
                                expandable: true,
                                enabled: true,
                                lazy_children: false,
                                domain_ref: Some(DomainRef {
                                    kind: "alignment".to_string(),
                                    id: 77,
                                }),
                            },
                            TreeRowDef {
                                id: sid(8),
                                parent: Some(sid(7)),
                                label: "AL-1".to_string(),
                                icon: None,
                                status: None,
                                expandable: true,
                                enabled: true,
                                lazy_children: true,
                                domain_ref: Some(DomainRef {
                                    kind: "alignment".to_string(),
                                    id: 42,
                                }),
                            },
                        ],
                    ),
                    wlist(
                        9,
                        vec![
                            ItemDef {
                                id: sid(10),
                                label: "profile-1".to_string(),
                                icon: None,
                                enabled: true,
                                visible: true,
                                command: Some("profile.open".to_string()),
                                domain_ref: Some(DomainRef {
                                    kind: "surface".to_string(),
                                    id: 5,
                                }),
                            },
                            litem(11, "profile-2"),
                        ],
                    ),
                    // SplitView: horizontal group with a ratio; the scene
                    // pane composes at the HOST dock — the empty vertical
                    // group is the documented placeholder where the host
                    // mounts the panel scene surface.
                    wgroup(
                        12,
                        Axis::Horizontal,
                        Some(0.6),
                        vec![wtable(13, 900), wgroup(14, Axis::Vertical, None, vec![])],
                    ),
                ],
            )],
            // SWG-004 trailing registry field (see the wsurface helper note).
            tables: BTreeMap::new(),
        };
        surface.validate().expect("representative surface is valid");
        let summary = surface.widget_summary();
        assert_eq!(summary.widgets, 9); // g1, g2, icon3, menu4, tree6, list9, split12, table13, placeholder14
        assert_eq!(summary.by_kind.groups, 4); // ids 1, 2, 12, 14
        assert_eq!(summary.by_kind.trees, 1);
        assert_eq!(summary.by_kind.lists, 1);
        assert_eq!(summary.by_kind.icon_buttons, 1);
        assert_eq!(summary.by_kind.menus, 1); // the submenu MenuDef is PAYLOAD (not a WidgetNode); it carries no node of its own
        assert_eq!(summary.by_kind.tables, 1);
        assert_eq!(summary.by_kind.total(), summary.widgets);

        // Bincode preserves the surface exactly.
        let bytes = bincode::serialize(&surface).expect("serialize surface");
        let restored: WidgetSurface = bincode::deserialize(&bytes).expect("decode surface");
        assert_eq!(surface, restored);
        restored.validate().expect("decoded surface is valid");

        // JSON roundtrips the same surface (manifest/registry tooling shape).
        let json = serde_json::to_string(&surface).expect("surface json");
        let from_json: WidgetSurface = serde_json::from_str(&json).expect("decode surface json");
        assert_eq!(surface, from_json);
    }

    #[test]
    fn widget_node_and_widgetop_and_widgetevent_bincode_discriminants_are_pinned() {
        let node_tag = |node: &WidgetNode| -> u32 {
            let bytes = bincode::serialize(node).expect("serialize widget node");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let op_tag = |op: &WidgetOp| -> u32 {
            let bytes = bincode::serialize(op).expect("serialize widget op");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let event_tag = |event: &WidgetEvent| -> u32 {
            let bytes = bincode::serialize(event).expect("serialize widget event");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let cell_tag = |value: &WidgetCellValue| -> u32 {
            let bytes = bincode::serialize(value).expect("serialize cell value");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let axis_tag = |axis: &Axis| -> u32 {
            let bytes = bincode::serialize(axis).expect("serialize axis");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let confirm_tag = |policy: &ConfirmPolicy| -> u32 {
            let bytes = bincode::serialize(policy).expect("serialize confirm policy");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };

        // The wire grows append-only; these indices are the whole pin.
        let nodes: Vec<(u32, WidgetNode)> = vec![
            (0, wtree(31, vec![])),
            (1, wlist(32, vec![])),
            (2, wicon(33, "noop")),
            (3, wmenu(34, vec![])),
            (4, wtable(35, 36)),
            (5, wgroup(37, Axis::Vertical, None, vec![])),
        ];
        for (expected_discriminant, node) in &nodes {
            assert_eq!(
                node_tag(node),
                *expected_discriminant,
                "WidgetNode discriminant drift for {node:?}"
            );
        }
        let icon = wicon(71, "noop");
        assert_eq!(
            op_tag(&WidgetOp::Add {
                parent: None,
                widget: icon.clone()
            }),
            0
        );
        assert_eq!(
            op_tag(&WidgetOp::Update {
                id: sid(71),
                content: icon
            }),
            1
        );
        assert_eq!(op_tag(&WidgetOp::Remove { id: sid(71) }), 2);

        let events: Vec<(u32, WidgetEvent)> = vec![
            (
                0,
                WidgetEvent::TreeExpanded {
                    surface_id: sid(1),
                    node_id: sid(2),
                    expanded: true,
                },
            ),
            (
                1,
                WidgetEvent::ChildrenRequested {
                    surface_id: sid(1),
                    parent_id: sid(2),
                    range: WidgetRange { start: 0, len: 1 },
                },
            ),
            (
                2,
                WidgetEvent::SelectionChanged {
                    surface_id: sid(1),
                    selected_ids: vec![sid(2)],
                },
            ),
            (
                3,
                WidgetEvent::ItemActivated {
                    surface_id: sid(1),
                    item_id: sid(2),
                },
            ),
            (
                4,
                WidgetEvent::CommandInvoked {
                    surface_id: sid(1),
                    command_id: "c".to_string(),
                    target_ids: vec![],
                },
            ),
            (
                5,
                WidgetEvent::TableCellChanged {
                    surface_id: sid(1),
                    row_key: sid(2),
                    column_id: sid(3),
                    value: WidgetCellValue::Number(1.5),
                },
            ),
        ];
        for (expected_discriminant, event) in &events {
            assert_eq!(
                event_tag(event),
                *expected_discriminant,
                "WidgetEvent discriminant drift for {event:?}"
            );
        }
        assert_eq!(cell_tag(&WidgetCellValue::Text("x".to_string())), 0);
        assert_eq!(cell_tag(&WidgetCellValue::Number(1.0)), 1);
        assert_eq!(cell_tag(&WidgetCellValue::Bool(true)), 2);
        assert_eq!(axis_tag(&Axis::Vertical), 0);
        assert_eq!(axis_tag(&Axis::Horizontal), 1);
        assert_eq!(
            confirm_tag(&ConfirmPolicy::Prompt {
                message: "m".to_string()
            }),
            0
        );
    }

    #[test]
    fn widget_patch_apply_adds_updates_removes_two_phase_atomic() {
        // base (rev 7): g1[ icon2, tree3[rows: row4] ], menu5. One patch
        // performing add(root), add(child of the new group), update, remove,
        // remove — in that order, all against revision 7.
        let base_tree = wtree(3, vec![trow(4, None, "root")]);
        let mut surface = wsurface(
            7,
            vec![
                wgroup(1, Axis::Vertical, None, vec![wicon(2, "a"), base_tree]),
                wmenu(5, vec![mitem("m1", "one")]),
            ],
        );
        surface.validate().expect("base surface is valid");
        let before_root_count = surface.roots.len();
        assert_eq!(before_root_count, 2);

        let applied = wpatch(
            7,
            8,
            vec![
                WidgetOp::Add {
                    parent: None,
                    widget: wgroup(10, Axis::Vertical, None, vec![]),
                },
                WidgetOp::Add {
                    // The parent is a group added EARLIER IN THE SAME PATCH.
                    parent: Some(sid(10)),
                    widget: wicon(11, "b"),
                },
                WidgetOp::Update {
                    id: sid(2),
                    content: wicon(2, "a2"),
                },
                // Removing the tree takes its row payload (row id 4) with it.
                WidgetOp::Remove { id: sid(3) },
                WidgetOp::Remove { id: sid(5) },
            ],
        );
        surface.apply_patch(applied).expect("whole-patch apply");
        assert_eq!(surface.revision, SceneRevision(8));
        surface.validate().expect("patched surface is valid");

        // Root order: g1 (emptied + updated button), g10 (grown).
        assert_eq!(surface.roots.len(), 2);
        match &surface.roots[0] {
            WidgetNode::Group(group) => {
                assert_eq!(group.children.len(), 1);
                match &group.children[0] {
                    WidgetNode::IconButton(def) => {
                        assert_eq!(def.id, sid(2));
                        assert_eq!(def.command, "a2");
                    }
                    other => panic!("expected the updated icon button, got {other:?}"),
                }
            }
            other => panic!("expected the emptied group, got {other:?}"),
        }
        match &surface.roots[1] {
            WidgetNode::Group(group) => {
                assert_eq!(group.id, sid(10));
                assert_eq!(group.children.len(), 1);
                assert_eq!(group.children[0].widget_id(), sid(11));
            }
            other => panic!("expected the new group with its child, got {other:?}"),
        }

        // Zero-op patches are legitimate revision bumps.
        let elements_before = surface.roots.clone();
        surface
            .apply_patch(wpatch(8, 9, vec![]))
            .expect("no-op applies");
        assert_eq!(surface.roots, elements_before);
        assert_eq!(surface.revision, SceneRevision(9));
    }

    #[test]
    fn widget_stale_base_and_non_monotonic_rejected_without_partial_writes() {
        let mut surface = wsurface(
            7,
            vec![wgroup(1, Axis::Vertical, None, vec![wicon(2, "a2")])],
        );
        let before = shot(&surface);

        // Stale base; the op would even have applied cleanly if the base had
        // matched: Remove {id:2} targets a live widget.
        let stale = wpatch(6, 8, vec![WidgetOp::Remove { id: sid(2) }]);
        let error = surface
            .apply_patch(stale)
            .expect_err("stale base must refuse");
        assert_eq!(
            error,
            WidgetError::StaleBase {
                current: SceneRevision(7),
                given: SceneRevision(6),
            }
        );
        assert_eq!(shot(&surface), before);

        // Right base and revision, but the SECOND op fails after the FIRST
        // would have applied: validation still leaves zero writes behind.
        let half_valid = wpatch(
            7,
            8,
            vec![
                WidgetOp::Add {
                    parent: None,
                    widget: wicon(50, "c"),
                },
                WidgetOp::Remove { id: sid(999) },
            ],
        );
        let error = surface
            .apply_patch(half_valid)
            .expect_err("missing remove target must refuse");
        assert_eq!(error, WidgetError::ElementMissing(sid(999)));
        assert_eq!(shot(&surface), before);
        assert_eq!(surface.revision, SceneRevision(7));

        // Rewind and level successors are refused too; the same op set with
        // a strictly greater successor applies.
        let ops = vec![WidgetOp::Add {
            parent: None,
            widget: wicon(50, "c"),
        }];
        let error = surface
            .apply_patch(wpatch(7, 4, ops.clone()))
            .expect_err("rewind must refuse");
        assert!(
            matches!(
                &error,
                WidgetError::NonMonotonicRevision {
                    current: SceneRevision(7),
                    given: SceneRevision(4),
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(shot(&surface), before);
        surface
            .apply_patch(wpatch(7, 7, ops.clone()))
            .expect_err("level revision must refuse");
        assert_eq!(shot(&surface), before);
        surface
            .apply_patch(wpatch(7, 8, ops))
            .expect("bump applies");
        assert_eq!(surface.revision, SceneRevision(8));
        assert_eq!(surface.roots.clone().len(), 2);
    }

    #[test]
    fn widget_patch_op_budget_and_surface_node_budgets_enforced() {
        // Load-bearing coincidence mirrored from the scene tier: ops and
        // widget nodes share one budget endpoint, so a decoded patch frame
        // can never carry ops that outgrow the node budget independently
        // (MAX_PATCH_OPS docs in scene.rs pin the same equality).
        assert_eq!(MAX_PATCH_OPS, MAX_SCENE_NODES);

        // Op budget: MAX_PATCH_OPS + 1 remove ops refuse in phase 0 with the
        // first-crossing value (cap + 1) and zero writes.
        let mut surface = wsurface(1, vec![wicon(1, "c")]);
        let before = shot(&surface);
        let ops = (0..=MAX_PATCH_OPS)
            .map(|i| WidgetOp::Remove {
                id: StableId(i as u64),
            })
            .collect();
        let error = surface
            .apply_patch(wpatch(1, 2, ops))
            .expect_err("10_001 ops must refuse");
        assert!(
            matches!(
                &error,
                WidgetError::BudgetExceeded { current, max }
                    if *current == MAX_PATCH_OPS + 1 && *max == MAX_PATCH_OPS
            ),
            "unexpected error: {error:?}"
        );
        assert!(error.to_string().contains("budget"));
        assert_eq!(shot(&surface), before);

        // Surface node budget (validate): exactly MAX_SCENE_NODES root
        // groups sit AT the cap; one more crosses at cap + 1.
        let at_budget: Vec<WidgetNode> = (0..MAX_SCENE_NODES)
            .map(|i| wgroup(i as u64, Axis::Vertical, None, vec![]))
            .collect();
        let mut surface = wsurface(1, at_budget);
        assert_eq!(surface.validate(), Ok(()));
        surface
            .roots
            .push(wgroup(MAX_SCENE_NODES as u64, Axis::Vertical, None, vec![]));
        let error = surface
            .validate()
            .expect_err("10_001 widget nodes must refuse");
        assert!(
            matches!(
                &error,
                WidgetError::BudgetExceeded { current, max }
                    if *current == MAX_SCENE_NODES + 1 && *max == MAX_SCENE_NODES
            ),
            "unexpected error: {error:?}"
        );

        // Result (planner) node budget: a base at 9_999 groups plus ONE Add
        // of two groups crosses MAX_SCENE_NODES in the mirror; the walk
        // reports the first-crossing count and the surface stays untouched.
        let base: Vec<WidgetNode> = (0..9_999)
            .map(|i| wgroup(i as u64, Axis::Vertical, None, vec![]))
            .collect();
        let mut surface = wsurface(1, base);
        assert_eq!(surface.validate(), Ok(()));
        let error = surface
            .apply_patch(wpatch(
                1,
                2,
                vec![
                    WidgetOp::Add {
                        parent: None,
                        widget: wgroup(20_000, Axis::Vertical, None, vec![]),
                    },
                    WidgetOp::Add {
                        parent: None,
                        widget: wgroup(20_001, Axis::Vertical, None, vec![]),
                    },
                ],
            ))
            .expect_err("crossing the result node budget must refuse");
        assert!(matches!(
            &error,
            WidgetError::BudgetExceeded { current, max }
                if *current == MAX_SCENE_NODES + 1 && *max == MAX_SCENE_NODES
        ));
        assert_eq!(surface.roots.len(), 9_999, "zero writes on budget refusal");

        // Non-group widget budget: 512 controls sit AT the cap; 513 cross.
        let at_cap: Vec<WidgetNode> = (0..MAX_WIDGETS_PER_PANEL)
            .map(|i| wicon(i as u64, "c"))
            .collect();
        let surface = wsurface(1, vec![wgroup(999, Axis::Vertical, None, at_cap)]);
        assert_eq!(surface.validate(), Ok(()));
        let over: Vec<WidgetNode> = (0..=MAX_WIDGETS_PER_PANEL)
            .map(|i| wicon(i as u64, "c"))
            .collect();
        let surface = wsurface(1, vec![wgroup(999, Axis::Vertical, None, over)]);
        let error = surface
            .validate()
            .expect_err("513 non-group widgets must refuse the 512 cap");
        assert!(matches!(
            &error,
            WidgetError::BudgetExceeded { current, max }
                if *current == MAX_WIDGETS_PER_PANEL + 1 && *max == MAX_WIDGETS_PER_PANEL
        ));
    }

    #[test]
    fn tree_rows_parent_chain_validated() {
        // Ordered no-forward bindings: parents name PRECEDING rows; chains
        // within MAX_SCENE_DEPTH validate.
        let ordered = wtree(
            1,
            vec![
                trow(2, None, "root"),
                trow(3, Some(2), "child"),
                trow(4, Some(3), "grandchild"),
                trow(5, None, "second root"),
            ],
        );
        assert_eq!(
            wsurface(1, vec![ordered]).validate(),
            Ok(()),
            "ordered bindings validate"
        );

        // Duplicate row ids — within one tree and ACROSS two trees (the
        // surface-wide namespace) — refuse as DuplicateWidgetId.
        let surface = wsurface(
            1,
            vec![wtree(1, vec![trow(2, None, "a"), trow(2, None, "b")])],
        );
        assert_eq!(
            surface.validate(),
            Err(WidgetError::DuplicateWidgetId(sid(2)))
        );
        let surface = wsurface(
            1,
            vec![
                wtree(1, vec![trow(2, None, "a")]),
                wtree(3, vec![trow(2, None, "b")]),
            ],
        );
        assert_eq!(
            surface.validate(),
            Err(WidgetError::DuplicateWidgetId(sid(2)))
        );

        // Parent not found anywhere in the payload → ParentMissing(the
        // missing parent id).
        let surface = wsurface(1, vec![wtree(1, vec![trow(2, Some(404), "orphan")])]);
        assert_eq!(
            surface.validate(),
            Err(WidgetError::ParentMissing(sid(404)))
        );

        // Forward reference (the parent-row payload rule) → ParentMissing on
        // the forward parent id.
        let surface = wsurface(
            1,
            vec![wtree(
                1,
                vec![
                    trow(2, Some(3), "child first"),
                    trow(3, None, "parent after"),
                ],
            )],
        );
        assert_eq!(surface.validate(), Err(WidgetError::ParentMissing(sid(3))));

        // Parent cycle: the ONLY representable cycle under the ordered
        // no-forward rule is a self-parent; reports DepthExceeded with the
        // chain-depth where the walk recognized the defect.
        let surface = wsurface(1, vec![wtree(1, vec![trow(2, Some(2), "self parent")])]);
        let error = surface
            .validate()
            .expect_err("self-parent cycle must refuse");
        assert!(
            matches!(
                &error,
                WidgetError::DepthExceeded { current, max }
                    if *current == 2 && *max == MAX_SCENE_DEPTH
            ),
            "unexpected error: {error:?}"
        );

        // A chain of MAX_SCENE_DEPTH rows sits at the cap; one more row
        // crosses at cap + 1. (Tree id 900 keeps the widget id out of the
        // rows' own numbering.)
        let mut rows = vec![trow(0, None, "root")];
        for i in 1..=MAX_SCENE_DEPTH {
            rows.push(trow(i as u64, Some(i as u64 - 1), "deeper"));
        }
        assert_eq!(rows.len(), MAX_SCENE_DEPTH + 1);
        let surface = wsurface(1, vec![wtree(900, rows.clone())]);
        let error = surface.validate().expect_err("65-row chain must refuse");
        assert!(matches!(
            &error,
            WidgetError::DepthExceeded { current, max }
                if *current == MAX_SCENE_DEPTH + 1 && *max == MAX_SCENE_DEPTH
        ));
        rows.pop(); // back to a 64-row chain: exactly the cap.
        let surface = wsurface(1, vec![wtree(900, rows)]);
        assert_eq!(surface.validate(), Ok(()));
    }

    #[test]
    fn tree_lazy_rows_require_expandable() {
        // lazy_children with expandable=false is the documented refuse
        // (LazyRowNotExpandable, appended at the enum tail).
        let mut lazy = trow(2, Some(1), "lazy");
        lazy.lazy_children = true;
        assert!(!lazy.expandable);
        let surface = wsurface(1, vec![wtree(900, vec![trow(1, None, "root"), lazy])]);
        assert_eq!(
            surface.validate(),
            Err(WidgetError::LazyRowNotExpandable(sid(2)))
        );

        // expandable + lazy is the valid lazy shape; a plainly expandable
        // row without lazy children is fine too.
        let surface = wsurface(
            1,
            vec![wtree(
                900,
                vec![
                    trow(1, None, "root"),
                    lazy_row(2, Some(1), "lazy expandable"),
                    TreeRowDef {
                        expandable: true,
                        ..trow(3, None, "plain expandable")
                    },
                ],
            )],
        );
        assert_eq!(surface.validate(), Ok(()));
    }

    #[test]
    fn icon_button_requires_accessible_name_and_command() {
        // Accessibility is NOT optional: an empty accessible_name refuses as
        // EmptyRequiredText naming the field.
        let mute = WidgetNode::IconButton(IconActionDef {
            id: sid(1),
            accessible_name: String::new(),
            tooltip: None,
            icon: icon_ref("svg:x"),
            enabled: true,
            visible: true,
            active: false,
            command: "filter.toggle".to_string(),
            confirmation: None,
        });
        assert_eq!(
            wsurface(1, vec![mute]).validate(),
            Err(WidgetError::EmptyRequiredText {
                id: Some(sid(1)),
                what: "accessible name",
            })
        );

        // The semantic command is a REQUIRED non-empty id: an anonymous
        // command is exactly the "no semantic action" defect the contract
        // exists to reject.
        let silent = WidgetNode::IconButton(IconActionDef {
            id: sid(1),
            accessible_name: "visible button".to_string(),
            tooltip: None,
            icon: icon_ref("svg:x"),
            enabled: true,
            visible: true,
            active: false,
            command: String::new(),
            confirmation: None,
        });
        assert_eq!(
            wsurface(1, vec![silent]).validate(),
            Err(WidgetError::EmptyRequiredText {
                id: Some(sid(1)),
                what: "icon button command",
            })
        );

        // The complete button validates.
        assert_eq!(
            wsurface(1, vec![wicon(1, "filter.toggle")]).validate(),
            Ok(())
        );
    }

    #[test]
    fn menu_submenu_depth_and_confirmation_validate() {
        // 8 menu levels are exactly AT the cap; 9 refuse as DepthExceeded
        // (current = 9, max = MAX_MENU_DEPTH). The 8-deep chain cannot be
        // built by [`nested_menu`]'s asserted bound, so the 9-deep literal
        // wraps one 8-deep chain inside a fresh menu level: menu 0 -> items
        // [ submenu: menu chain of 8 more levels ] = 9 levels total.
        let at_cap = wsurface(1, vec![nested_menu(100, MAX_MENU_DEPTH)]);
        assert_eq!(at_cap.validate(), Ok(()));

        let WidgetNode::Menu(inner) = nested_menu(200, MAX_MENU_DEPTH) else {
            panic!("nested_menu builds a menu");
        };
        let nine = wsurface(
            1,
            vec![WidgetNode::Menu(MenuDef {
                id: sid(300),
                name: None,
                enabled: true,
                visible: true,
                label: None,
                items: vec![MenuItemDef {
                    command_id: "cmd-0".to_string(),
                    label: "level 0".to_string(),
                    icon: None,
                    shortcut: None,
                    enabled: true,
                    visible: true,
                    submenu: Some(Box::new(inner)),
                    confirmation: None,
                }],
            })],
        );
        let error = nine.validate().expect_err("9-deep menu must refuse");
        assert!(
            matches!(
                &error,
                WidgetError::DepthExceeded { current, max }
                    if *current == MAX_MENU_DEPTH + 1 && *max == MAX_MENU_DEPTH
            ),
            "unexpected error: {error:?}"
        );
        assert!(error.to_string().contains("depth"));

        // The 8-deep chain's confirmations survive a bincode roundtrip.
        let bytes = bincode::serialize(&at_cap).expect("serialize surface");
        let decoded: WidgetSurface = bincode::deserialize(&bytes).expect("decode surface");
        assert_eq!(at_cap, decoded);
    }

    #[test]
    fn widget_events_bounded_decode() {
        let encode_decode = |event: &WidgetEvent| -> Result<WidgetEvent, ()> {
            let bytes = bincode::serialize(event).expect("serialize event");
            bincode::deserialize(&bytes).map_err(|_| ())
        };

        // SelectionChanged: exactly MAX_SELECTION_IDS ids decode + validate;
        // cap + 1 refuses at DECODE (serde custom error) and at the
        // construct-level validate().
        let ok_selection = WidgetEvent::SelectionChanged {
            surface_id: sid(1),
            selected_ids: (1..=MAX_SELECTION_IDS as u64).map(sid).collect(),
        };
        assert_eq!(ok_selection.validate(), Ok(()));
        let decoded = encode_decode(&ok_selection).expect("the cap-sized selection set decodes");
        assert_eq!(decoded, ok_selection);
        let over = WidgetEvent::SelectionChanged {
            surface_id: sid(1),
            selected_ids: (1..=(MAX_SELECTION_IDS as u64 + 1)).map(sid).collect(),
        };
        assert!(over.validate().is_err());
        assert!(
            encode_decode(&over).is_err(),
            "cap+1 ids must refuse at decode"
        );

        // WidgetRange: len exactly MAX_WIDGET_RANGE_LEN is fine; cap + 1
        // refuses at decode and at validate().
        let ok_range = WidgetEvent::ChildrenRequested {
            surface_id: sid(1),
            parent_id: sid(2),
            range: WidgetRange {
                start: 0,
                len: MAX_WIDGET_RANGE_LEN,
            },
        };
        assert_eq!(ok_range.validate(), Ok(()));
        assert_eq!(encode_decode(&ok_range), Ok(ok_range.clone()));
        let over_range = WidgetEvent::ChildrenRequested {
            surface_id: sid(1),
            parent_id: sid(2),
            range: WidgetRange {
                start: 0,
                len: MAX_WIDGET_RANGE_LEN + 1,
            },
        };
        assert!(over_range.validate().is_err());
        assert!(encode_decode(&over_range).is_err());

        // CommandInvoked: exactly MAX_WIDGET_TARGET_IDS targets fine; cap + 1
        // refuses at both levels.
        let ok_targets = WidgetEvent::CommandInvoked {
            surface_id: sid(1),
            command_id: "entity.delete".to_string(),
            target_ids: (1..=MAX_WIDGET_TARGET_IDS as u64).map(sid).collect(),
        };
        assert_eq!(ok_targets.validate(), Ok(()));
        assert_eq!(encode_decode(&ok_targets), Ok(ok_targets.clone()));
        let over_targets = WidgetEvent::CommandInvoked {
            surface_id: sid(1),
            command_id: "entity.delete".to_string(),
            target_ids: (1..=(MAX_WIDGET_TARGET_IDS as u64 + 1)).map(sid).collect(),
        };
        assert!(over_targets.validate().is_err());
        assert!(encode_decode(&over_targets).is_err());

        // Whole-frame event budget: a Vec<WidgetEvent> frame decoded through
        // the thread-local budget refuses past MAX_SCENE_NODES. Like every
        // scene-tier budgeted* call, each frame's decoder charges ONE
        // nesting level on entry (the documented scene-decoder contract —
        // "charges one nesting level and the decoded element total"), so
        // the effective event count per frame is MAX_SCENE_NODES - 1; the
        // first crossing refuses the frame. The frame boundary helper
        // resets the counter between frames.
        #[derive(Serialize, Deserialize)]
        struct EventFrame {
            #[serde(deserialize_with = "budgeted_widget_events")]
            events: Vec<WidgetEvent>,
        }
        let frame_event = |index: u64| WidgetEvent::TreeExpanded {
            surface_id: sid(1),
            node_id: sid(index),
            expanded: true,
        };

        widget_decode_frame_begin();
        let under = EventFrame {
            events: (0..MAX_SCENE_NODES as u64 - 1).map(frame_event).collect(),
        };
        let bytes = bincode::serialize(&under).expect("serialize under frame");
        let decoded: EventFrame = bincode::deserialize(&bytes).expect("under frame decodes");
        assert_eq!(decoded.events.len(), MAX_SCENE_NODES - 1);

        widget_decode_frame_begin(); // the budget re-zeroes at a frame boundary
        let over = EventFrame {
            events: (0..MAX_SCENE_NODES as u64).map(frame_event).collect(),
        };
        let bytes = bincode::serialize(&over).expect("serialize over frame");
        let decoded: Result<EventFrame, _> = bincode::deserialize(&bytes);
        assert!(
            decoded.is_err(),
            "the first frame crossing must refuse at decode"
        );

        widget_decode_frame_begin();
        let again = EventFrame {
            events: vec![frame_event(1)],
        };
        let bytes = bincode::serialize(&again).expect("serialize again frame");
        let decoded: EventFrame = bincode::deserialize(&bytes).expect("budget recovered");
        assert_eq!(decoded.events.len(), 1);
    }

    #[test]
    fn domain_ref_is_explicit_mapping() {
        // The presentation id (row StableId 1001) and the domain id (113)
        // are DISTINCT values: the mapping is declared, never implicit.
        let row = TreeRowDef {
            id: sid(1001),
            parent: None,
            label: "AL-113".to_string(),
            icon: None,
            status: None,
            expandable: false,
            enabled: true,
            lazy_children: false,
            domain_ref: Some(DomainRef {
                kind: "alignment".to_string(),
                id: 113,
            }),
        };
        assert_ne!(
            row.id.0,
            row.domain_ref.as_ref().map(|domain| domain.id).unwrap()
        );
        let surface = wsurface(1, vec![wtree(1, vec![row])]);
        assert_eq!(surface.validate(), Ok(()));

        // The declared mapping survives a bincode roundtrip verbatim.
        let bytes = bincode::serialize(&surface).expect("serialize surface");
        let decoded: WidgetSurface = bincode::deserialize(&bytes).expect("decode surface");
        let WidgetNode::Tree(def) = &decoded.roots[0] else {
            panic!("expected the tree, got {:?}", decoded.roots[0]);
        };
        assert_eq!(
            def.rows[0].domain_ref,
            Some(DomainRef {
                kind: "alignment".to_string(),
                id: 113,
            })
        );

        // A row WITHOUT a domain_ref is valid pure presentation: conflation
        // is impossible implicitly because there is nothing to conflate.
        let surface = wsurface(1, vec![wtree(1, vec![trow(2, None, "presentation only")])]);
        assert_eq!(surface.validate(), Ok(()));
    }

    #[test]
    fn widget_surface_summary_bounded() {
        // Hetero hand-built surface with independently counted kinds/roots:
        // g1[ tree2, list3[items: 2] ], icon4, menu5, table6.
        let surface = wsurface(
            12,
            vec![
                wgroup(
                    1,
                    Axis::Vertical,
                    None,
                    vec![
                        wtree(2, vec![trow(7, None, "r")]),
                        wlist(3, vec![litem(8, "i1"), litem(9, "i2")]),
                    ],
                ),
                wicon(4, "c"),
                wmenu(5, vec![mitem("m", "one")]),
                wtable(6, 901),
            ],
        );
        let summary = surface.widget_summary();
        assert_eq!(summary.revision, SceneRevision(12));
        assert_eq!(summary.widgets, 6);
        assert_eq!(summary.by_kind.groups, 1);
        assert_eq!(summary.by_kind.trees, 1);
        assert_eq!(summary.by_kind.lists, 1);
        assert_eq!(summary.by_kind.icon_buttons, 1);
        assert_eq!(summary.by_kind.menus, 1);
        assert_eq!(summary.by_kind.tables, 1);
        assert_eq!(summary.by_kind.total(), summary.widgets);
        assert_eq!(summary.root_ids, vec![sid(1), sid(4), sid(5), sid(6)]);
        assert_eq!(summary.root_ids_truncated, 0);
        assert!(!summary.overflow_roots);

        // Deep nesting reads identically: counts by kind, not by depth.
        let mut next = 100u64;
        let surface = wsurface(3, vec![nested_chain(4, &mut next)]);
        let summary = surface.widget_summary();
        assert_eq!(summary.widgets, 5); // 4 groups + the icon button leaf
        assert_eq!(summary.by_kind.groups, 4);
        assert_eq!(summary.by_kind.icon_buttons, 1);
        assert_eq!(summary.root_ids.len(), 1);

        // Truncation contract at the cap boundary: exactly 64 roots is
        // untruncated; 65 caps and reports the dropped count.
        let mut next = 0u64;
        let surface = wsurface(
            3,
            (0..MAX_SUMMARY_ROOT_IDS)
                .map(|_| nested_chain(0, &mut next))
                .collect(),
        );
        let summary = surface.widget_summary();
        assert_eq!(summary.root_ids.len(), MAX_SUMMARY_ROOT_IDS);
        assert_eq!(summary.root_ids_truncated, 0);
        assert!(!summary.overflow_roots);
        assert_eq!(summary.widgets, MAX_SUMMARY_ROOT_IDS);

        let mut next = 0u64;
        let surface = wsurface(
            3,
            (0..MAX_SUMMARY_ROOT_IDS + 1)
                .map(|_| nested_chain(0, &mut next))
                .collect(),
        );
        let summary = surface.widget_summary();
        assert_eq!(summary.root_ids.len(), MAX_SUMMARY_ROOT_IDS);
        assert_eq!(summary.root_ids_truncated, 1);
        assert!(summary.overflow_roots);
        assert_eq!(summary.widgets, MAX_SUMMARY_ROOT_IDS + 1);
        assert_eq!(summary.by_kind.icon_buttons, MAX_SUMMARY_ROOT_IDS + 1);
        // The summary read NEVER mutates; the surface remains patchable
        // (recovery read -> rebuild against the authoritative revision).
        assert_eq!(surface.revision, SceneRevision(3));
    }

    #[test]
    fn legacy_dialog_wire_untouched() {
        // The widget work adds a NEW surface; the existing pinned enums must
        // show zero drift. Two in-place pins (the same shape the dialog
        // tests carry): DialogControl::Row stays bincode tag 10 and
        // DialogValue::Text stays 0.
        let row = crate::dialog::DialogControl::Row {
            controls: vec![],
            align_h: None,
            align_v: None,
        };
        let bytes = bincode::serialize(&row).expect("serialize DialogControl::Row");
        let tag = u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"));
        assert_eq!(tag, 10, "DialogControl::Row pin drifted");

        let text = crate::dialog::DialogValue::Text("hello".to_string());
        let bytes = bincode::serialize(&text).expect("serialize DialogValue::Text");
        let tag = u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"));
        assert_eq!(tag, 0, "DialogValue::Text pin drifted");
    }

    #[test]
    fn widgets_compose_in_reference_layout() {
        // ARCHITECTURE addendum §12's reference layout, composed ENTIRELY
        // from the six WidgetNode kinds (the "search field" of the sketch is
        // NOT a Family-1 widget — text entry is host chrome; the search
        // affordance rides focused via the icon button's command —
        // documented gap):
        //
        // Column
        // ├── Row [search (icon button affordance), filter icon button,
        // common settings menu]
        // ├── Tree [domain hierarchy]
        // ├── SplitView ── Table [typed-table REFERENCE] | Scene [empty
        // group placeholder — the scene surface mounts at the HOST dock
        // (ADR-005)]
        // └── Row [status and create/edit/delete commands]
        let column = wgroup(
            1,
            Axis::Vertical,
            None,
            vec![
                wgroup(
                    2,
                    Axis::Horizontal,
                    None,
                    vec![
                        WidgetNode::IconButton(IconActionDef {
                            id: sid(3),
                            accessible_name: "search".to_string(),
                            tooltip: None,
                            icon: icon_ref("svg:search"),
                            enabled: true,
                            visible: true,
                            active: false,
                            command: "search.focus".to_string(),
                            confirmation: None,
                        }),
                        wicon(4, "filter.toggle"),
                        wmenu(5, vec![mitem("settings.open", "Settings")]),
                    ],
                ),
                wtree(
                    6,
                    vec![trow(7, None, "Sections"), trow(8, Some(7), "Polylines")],
                ),
                wgroup(
                    9,
                    Axis::Horizontal,
                    Some(0.5),
                    vec![
                        wtable(10, 4000),
                        // Scene pane placeholder: the host mounts the panel
                        // scene surface in THIS group's cell (ADR-005).
                        wgroup(11, Axis::Vertical, None, vec![]),
                    ],
                ),
                wgroup(
                    12,
                    Axis::Horizontal,
                    None,
                    vec![
                        wicon(13, "entity.create"),
                        wicon(14, "entity.edit"),
                        wicon(15, "entity.delete"),
                    ],
                ),
            ],
        );
        let surface = wsurface(1, vec![column]);
        surface
            .validate()
            .expect("the reference layout composes and validates");

        let summary = surface.widget_summary();
        assert_eq!(summary.revision, SceneRevision(1));
        assert_eq!(summary.widgets, 13);
        assert_eq!(summary.by_kind.groups, 5); // 1, 2, 9, 11, 12
        assert_eq!(summary.by_kind.icon_buttons, 5); // 3, 4, 13, 14, 15
        assert_eq!(summary.by_kind.trees, 1);
        assert_eq!(summary.by_kind.menus, 1);
        assert_eq!(summary.by_kind.tables, 1);
        assert_eq!(summary.by_kind.lists, 0);
        assert_eq!(summary.by_kind.total(), summary.widgets);
        assert_eq!(summary.root_ids, vec![sid(1)]);
        assert!(!summary.overflow_roots);

        // The SplitView ratio survives composition: the split's children are
        // the table ref and the scene-cell placeholder, in that order.
        let WidgetNode::Group(column) = &surface.roots[0] else {
            panic!("expected the reference column, got {:?}", surface.roots[0]);
        };
        let WidgetNode::Group(split) = &column.children[2] else {
            panic!("expected the split, got {:?}", column.children[2]);
        };
        assert_eq!(split.split_ratio, Some(0.5));
        match &split.children[0] {
            WidgetNode::Table(def) => assert_eq!(def.table_id, sid(4000)),
            other => panic!("expected the table ref, got {other:?}"),
        }
        assert!(
            matches!(split.children[1], WidgetNode::Group(_)),
            "expected the scene-cell placeholder group"
        );
    }

    // -------------------------------------------------------------------
    // SWG-004 tests (typed tables: registry/cells/patches/paging/legacy)
    // -------------------------------------------------------------------

    // -- construction helpers (appended; no existing helper touched) ------

    fn column(id: u64, kind: CellKind, label: &str) -> TableColumnDef {
        TableColumnDef {
            id: sid(id),
            label: label.to_string(),
            kind,
            width_hint: None,
            stretch: false,
        }
    }

    fn trow_typed(key: u64, cells: Vec<TableCell>) -> TableRowDef {
        TableRowDef {
            key: sid(key),
            cells,
            domain_ref: None,
        }
    }

    fn table_def(id: u64, columns: Vec<TableColumnDef>, rows: Vec<TableRowDef>) -> PanelTableDef {
        PanelTableDef {
            id_table: sid(id),
            columns,
            total_rows: None,
            rows,
        }
    }

    fn surface_with_table(
        revision: u64,
        ref_id: u64,
        table_id: u64,
        def: PanelTableDef,
    ) -> WidgetSurface {
        WidgetSurface {
            revision: SceneRevision(revision),
            roots: vec![wtable(ref_id, table_id)],
            tables: BTreeMap::from([(sid(table_id), def)]),
        }
    }

    fn patch_cell(
        base: u64,
        next: u64,
        table: u64,
        key: u64,
        column: u64,
        value: WidgetCellValue,
    ) -> WidgetSurfacePatch {
        WidgetSurfacePatch {
            base_revision: SceneRevision(base),
            revision: SceneRevision(next),
            ops: vec![WidgetOp::TableCell {
                table: sid(table),
                key: sid(key),
                column: sid(column),
                value,
            }],
        }
    }

    /// The all-kinds reference def: 8 columns (one per [`CellKind`]) × 2 rows
    /// (pure-presentation + domain-backed), paged. Shared by the roundtrip
    /// and the cap/gate tests.
    fn all_kinds_table(id: u64) -> PanelTableDef {
        table_def(
            id,
            vec![
                column(1, CellKind::Text, "name"),
                column(2, CellKind::Number, "length"),
                column(3, CellKind::Bool, "locked"),
                column(4, CellKind::Icon, "icon"),
                column(5, CellKind::Link, "link"),
                column(6, CellKind::Badge, "status"),
                column(7, CellKind::Progress, "progress"),
                column(8, CellKind::Date, "opened"),
            ],
            vec![
                trow_typed(
                    10,
                    vec![
                        TableCell::Text("AL-1".to_string()),
                        TableCell::Number(42.5),
                        TableCell::Bool(true),
                        TableCell::Icon(icon_ref("svg:alignment")),
                        TableCell::Link {
                            label: "docs".to_string(),
                            uri: "https://example.test/a".to_string(),
                        },
                        TableCell::Badge {
                            label: "wip".to_string(),
                            tone: BadgeTone::Warning,
                        },
                        TableCell::Progress {
                            fraction: 0.25,
                            label: Some("quarter".to_string()),
                        },
                        TableCell::Date {
                            y: 2026,
                            m: 11,
                            d: 15,
                        },
                    ],
                ),
                TableRowDef {
                    domain_ref: Some(DomainRef {
                        kind: "alignment".to_string(),
                        id: 77,
                    }),
                    ..trow_typed(
                        11,
                        vec![
                            TableCell::Text("AL-2".to_string()),
                            TableCell::Number(0.0),
                            TableCell::Bool(false),
                            TableCell::Icon(icon_ref("svg:x")),
                            TableCell::Link {
                                label: "l".to_string(),
                                uri: "svg:l".to_string(),
                            },
                            TableCell::Badge {
                                label: "ok".to_string(),
                                tone: BadgeTone::Success,
                            },
                            TableCell::Progress {
                                fraction: 1.0,
                                label: None,
                            },
                            TableCell::Date {
                                y: 2025,
                                m: 1,
                                d: 31,
                            },
                        ],
                    )
                },
            ],
        )
    }

    #[test]
    fn panel_table_registry_roundtrips() {
        // The registry rides the surface (trailing map); the ref resolves;
        // all 8 cell kinds + paged total + domain refs survive BOTH wire
        // formats; the JSON-absent-registry decode (the serde(default)
        // facet) reads EMPTY.
        let def = all_kinds_table(2000);
        let surface = surface_with_table(4, 2000, 2000, def);
        surface
            .validate()
            .expect("registry + refs validate (the table id == the ref id PROVES the foreign identity spaces do not collide)");

        let summary = surface.widget_summary();
        assert_eq!(summary.by_kind.tables, 1);

        let bytes = bincode::serialize(&surface).expect("serialize surface");
        let decoded: WidgetSurface = bincode::deserialize(&bytes).expect("decode surface");
        assert_eq!(surface, decoded);
        assert_eq!(decoded.tables.len(), 1);
        let decoded_def = &decoded.tables[&sid(2000)];
        assert_eq!(decoded_def.rows.len(), 2);
        assert_eq!(decoded_def.columns.len(), 8);
        assert_eq!(
            decoded_def.column_position(sid(5)),
            Some(4),
            "the registry's id→position mapping survives the frame"
        );
        decoded.validate().expect("decoded surface valid");

        let json = serde_json::to_string(&surface).expect("surface json");
        let from_json: WidgetSurface = serde_json::from_str(&json).expect("decode surface json");
        assert_eq!(surface, from_json);

        // Old-shape JSON (no trailing registry member) decodes with an empty
        // registry — the JSON-side tolerance documented in the wire notes.
        let mut value = serde_json::to_value(&surface).expect("surface json value");
        let removed = value
            .as_object_mut()
            .expect("the surface is a JSON object")
            .remove("tables");
        assert!(removed.is_some(), "the registry member was carried");
        let old_shape: WidgetSurface =
            serde_json::from_value(value).expect("old-shape json decodes");
        assert!(old_shape.tables.is_empty());
        assert_eq!(old_shape.roots, surface.roots);
    }

    #[test]
    fn cell_kind_must_match_column() {
        // DEF leg: a Text cell under a Number column refuses (address-ful
        // InvalidCell per the documented convention).
        let def = table_def(
            1,
            vec![column(2, CellKind::Number, "length")],
            vec![trow_typed(3, vec![TableCell::Text("oops".to_string())])],
        );
        assert_eq!(
            def.validate(),
            Err(WidgetError::InvalidCell {
                table: sid(1),
                key: sid(3),
                column: sid(2),
                reason: "cell kind `text` does not match column kind `number`".to_string(),
            })
        );

        // PATCH leg: a skeleton-Text write into a Number column refuses and
        // leaves the surface untouched (phase-1 rejection, zero writes).
        let def = table_def(
            1,
            vec![
                column(2, CellKind::Text, "name"),
                column(3, CellKind::Number, "length"),
            ],
            vec![
                trow_typed(
                    4,
                    vec![TableCell::Text("a".to_string()), TableCell::Number(1.0)],
                ),
                trow_typed(
                    5,
                    vec![TableCell::Text("b".to_string()), TableCell::Number(2.0)],
                ),
            ],
        );
        let mut surface = surface_with_table(7, 100, 1, def);
        surface.validate().expect("valid base");
        let patch = patch_cell(7, 8, 1, 4, 3, WidgetCellValue::Text("nope".to_string()));
        let error = surface
            .apply_patch(patch)
            .expect_err("skeleton text into a number column must refuse");
        assert!(
            matches!(
                &error,
                WidgetError::InvalidCell { table, key, column, .. }
                    if *table == sid(1) && *key == sid(4) && *column == sid(3)
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(surface.revision, SceneRevision(7), "refusal stands");
        assert_eq!(
            surface.tables[&sid(1)].rows[0].cells[1],
            TableCell::Number(1.0),
            "the addressed cell is unchanged"
        );

        // The MATCHED write applies to exactly the addressed cell.
        surface
            .apply_patch(patch_cell(7, 8, 1, 4, 3, WidgetCellValue::Number(9.0)))
            .expect("matched kind applies");
        assert_eq!(
            surface.tables[&sid(1)].rows[0].cells[1],
            TableCell::Number(9.0)
        );
        assert_eq!(surface.revision, SceneRevision(8));

        // Presentation kinds are NOT patch-writable through the skeleton.
        let def = table_def(
            1,
            vec![column(2, CellKind::Date, "day")],
            vec![trow_typed(
                3,
                vec![TableCell::Date {
                    y: 2026,
                    m: 1,
                    d: 1,
                }],
            )],
        );
        let mut surface = surface_with_table(1, 100, 1, def);
        let error = surface
            .apply_patch(patch_cell(
                1,
                2,
                1,
                3,
                2,
                WidgetCellValue::Text("even a right-shaped text".to_string()),
            ))
            .expect_err("a date column is not patch-writable");
        assert!(
            matches!(
                &error,
                WidgetError::InvalidCell { reason, .. }
                    if reason.contains("not writable through the value skeleton")
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(surface.revision, SceneRevision(1));
    }

    #[test]
    fn row_keys_unique_and_column_registry_complete() {
        // Duplicate ROW keys (per-table registry; InvalidCell per the
        // documented def-shape address convention).
        let dup_rows = table_def(
            1,
            vec![column(2, CellKind::Text, "a")],
            vec![
                trow_typed(3, vec![TableCell::Text("x".to_string())]),
                trow_typed(3, vec![TableCell::Text("y".to_string())]),
            ],
        );
        let error = dup_rows
            .validate()
            .expect_err("duplicate row key must refuse");
        assert_eq!(
            error,
            WidgetError::InvalidCell {
                table: sid(1),
                key: sid(3),
                column: sid(3),
                reason: "duplicate row key in the published window".to_string(),
            }
        );

        // Duplicate column ids (per-table registry).
        let dup_columns = table_def(
            1,
            vec![
                column(2, CellKind::Text, "a"),
                column(2, CellKind::Text, "b"),
            ],
            vec![],
        );
        let error = dup_columns
            .validate()
            .expect_err("duplicate column id must refuse");
        assert_eq!(
            error,
            WidgetError::InvalidCell {
                table: sid(1),
                key: sid(2),
                column: sid(2),
                reason: "duplicate column id in the column registry".to_string(),
            }
        );

        // Cells UNDERshoot (2 cells, 3 columns): the first uncovered column
        // is named.
        let short = table_def(
            1,
            vec![
                column(2, CellKind::Text, "a"),
                column(3, CellKind::Number, "b"),
                column(4, CellKind::Bool, "c"),
            ],
            vec![trow_typed(
                5,
                vec![TableCell::Text("x".to_string()), TableCell::Number(1.0)],
            )],
        );
        assert_eq!(
            short.validate(),
            Err(WidgetError::InvalidCell {
                table: sid(1),
                key: sid(5),
                column: sid(4),
                reason:
                    "row carries 2 cells for a 3-column registry (does not shadow every column)"
                        .to_string(),
            })
        );

        // Cells OVERshoot (4 cells, 3 columns): no column exists for the
        // extra cells; the address convention repeats the row key.
        let long = table_def(
            1,
            vec![
                column(2, CellKind::Text, "a"),
                column(3, CellKind::Number, "b"),
                column(4, CellKind::Bool, "c"),
            ],
            vec![trow_typed(
                5,
                vec![
                    TableCell::Text("x".to_string()),
                    TableCell::Number(1.0),
                    TableCell::Bool(true),
                    TableCell::Bool(false),
                ],
            )],
        );
        assert_eq!(
            long.validate(),
            Err(WidgetError::InvalidCell {
                table: sid(1),
                key: sid(5),
                column: sid(5),
                reason:
                    "row carries 4 cells for a 3-column registry (no column shadows the extra cells)"
                        .to_string(),
            })
        );

        // Registry miskey: a def whose id_table is not its map key.
        let def = table_def(
            999,
            vec![column(2, CellKind::Text, "a")],
            vec![trow_typed(3, vec![TableCell::Text("x".to_string())])],
        );
        let surface = WidgetSurface {
            revision: SceneRevision(1),
            roots: vec![wtable(100, 999)],
            tables: BTreeMap::from([(sid(777), def)]),
        };
        assert_eq!(
            surface.validate(),
            Err(WidgetError::UnknownTableRef(sid(999)))
        );

        // PATCH routing: unknown column → InvalidCell; unknown row key →
        // InvalidCell; unknown TABLE → UnknownTableRef (the identity arm).
        let def = table_def(
            1,
            vec![column(2, CellKind::Text, "a")],
            vec![trow_typed(3, vec![TableCell::Text("x".to_string())])],
        );
        let mut surface = surface_with_table(1, 100, 1, def);
        let error = surface
            .apply_patch(patch_cell(
                1,
                2,
                1,
                3,
                404,
                WidgetCellValue::Text("y".to_string()),
            ))
            .expect_err("unknown column must refuse");
        assert!(
            matches!(
                &error,
                WidgetError::InvalidCell { column, reason, .. }
                    if *column == sid(404) && reason == "no such column id in the column registry"
            ),
            "unexpected error: {error:?}"
        );
        let error = surface
            .apply_patch(patch_cell(
                1,
                2,
                1,
                404,
                2,
                WidgetCellValue::Text("y".to_string()),
            ))
            .expect_err("unknown row key must refuse");
        assert!(
            matches!(
                &error,
                WidgetError::InvalidCell { key, reason, .. }
                    if *key == sid(404) && reason == "no such row key in the published window"
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(
            surface
                .apply_patch(patch_cell(
                    1,
                    2,
                    404,
                    3,
                    2,
                    WidgetCellValue::Text("y".to_string())
                ))
                .expect_err("unknown table must refuse"),
            WidgetError::UnknownTableRef(sid(404))
        );
        // All three refusals left the surface untouched.
        assert_eq!(surface.revision, SceneRevision(1));
        assert_eq!(
            surface.tables[&sid(1)].rows[0].cells[0],
            TableCell::Text("x".to_string())
        );
    }

    #[test]
    fn one_cell_patch_is_small() {
        // 4_096-row window (AT the payload cap), two columns; ONE cell
        // edited by ONE patch fact.
        let def = table_def(
            1,
            vec![
                column(2, CellKind::Number, "quantity"),
                column(3, CellKind::Bool, "locked"),
            ],
            (0..MAX_WIDGET_ROWS_INITIAL)
                .map(|i| {
                    trow_typed(
                        i as u64 + 1,
                        vec![TableCell::Number(i as f64), TableCell::Bool(i % 2 == 0)],
                    )
                })
                .collect(),
        );
        let mut surface = surface_with_table(1, 10_000, 1, def);
        surface
            .validate()
            .expect("the 4_096-row window validates at the cap");

        let patch = patch_cell(
            1,
            2,
            1,
            MAX_WIDGET_ROWS_INITIAL as u64,
            2,
            WidgetCellValue::Number(1234.5),
        );
        let bytes = bincode::serialize(&patch).expect("serialize 1-cell patch");
        assert!(
            bytes.len() <= 300,
            "1-cell patch payload = {} bytes (the single-cell acceptance is ~200B)",
            bytes.len()
        );
        surface.apply_patch(patch).expect("1-cell patch applies");

        // Surgical mutation assert: EXACTLY one cell moved — every other
        // cell of every other row is byte-identical to its original.
        let def = &surface.tables[&sid(1)];
        assert_eq!(def.rows.len(), MAX_WIDGET_ROWS_INITIAL);
        let mut changed_cells = 0usize;
        for (row_index, row) in def.rows.iter().enumerate() {
            let originals = [
                TableCell::Number(row_index as f64),
                TableCell::Bool(row_index % 2 == 0),
            ];
            for (cell_index, cell) in row.cells.iter().enumerate() {
                if cell != &originals[cell_index] {
                    changed_cells += 1;
                }
            }
        }
        assert_eq!(
            changed_cells, 1,
            "exactly ONE cell mutated across the 4_096x2 grid (ARCHITECTURE §16)"
        );
        assert_eq!(
            def.rows[MAX_WIDGET_ROWS_INITIAL - 1].cells[0],
            TableCell::Number(1234.5)
        );
        assert_eq!(surface.revision, SceneRevision(2));
    }

    #[test]
    fn single_cell_patch_bytes_versus_full_table_write() {
        // The §16 proof, byte-wise (no wall clock): the FULL-TABLE REPUBLISH
        // (the whole def a snapshot retransmits) versus the ONE-cell patch.
        let def = table_def(
            1,
            vec![column(2, CellKind::Text, "note")],
            (0..MAX_WIDGET_ROWS_INITIAL)
                .map(|i| {
                    trow_typed(
                        i as u64 + 1,
                        vec![TableCell::Text(format!(
                            "material row {i:04} description text"
                        ))],
                    )
                })
                .collect(),
        );
        def.validate()
            .expect("the full-table republish shape validates");
        let full_bytes = bincode::serialize(&def).expect("serialize full table");
        let one_bytes = bincode::serialize(&patch_cell(
            1,
            2,
            1,
            1,
            2,
            WidgetCellValue::Text("one cell".to_string()),
        ))
        .expect("serialize 1-cell patch");
        assert!(
            one_bytes.len() < 300,
            "one-cell patch = {} bytes; full-table republish = {} bytes (ARCHITECTURE §16: one cell edit transfers only its cell)",
            one_bytes.len(),
            full_bytes.len()
        );
        assert!(
            full_bytes.len() >= 100_000,
            "one-cell patch = {} bytes; full-table republish = {} bytes (ARCHITECTURE §16: one cell edit transfers only its cell)",
            one_bytes.len(),
            full_bytes.len()
        );
        println!(
            "one-cell patch: {} bytes — full-table republish: {} bytes ({:0.0}x)",
            one_bytes.len(),
            full_bytes.len(),
            full_bytes.len() as f64 / one_bytes.len() as f64
        );
    }

    #[test]
    fn table_page_request_bounds() {
        let seq_tag = |event: &WidgetEvent| -> u32 {
            let bytes = bincode::serialize(event).expect("serialize event");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let page = |len: usize| WidgetEvent::TablePageRequested {
            surface_id: sid(1),
            table: sid(2),
            first_key: sid(3),
            len,
        };

        // Tail pin 6 (the six prior event pins are untouched — see the
        // creation-tail test).
        assert_eq!(seq_tag(&page(4)), 6);

        // At the cap: construct-level + bounded decode both green.
        let at_cap = page(MAX_WIDGET_RANGE_LEN);
        assert_eq!(at_cap.validate(), Ok(()));
        let bytes = bincode::serialize(&at_cap).expect("serialize page request");
        assert_eq!(
            bincode::deserialize::<WidgetEvent>(&bytes).expect("the cap-sized page decodes"),
            at_cap
        );

        // Cap + 1: BudgetExceeded at construct level AND at decode.
        let over = page(MAX_WIDGET_RANGE_LEN + 1);
        assert_eq!(
            over.validate(),
            Err(WidgetError::BudgetExceeded {
                current: MAX_WIDGET_RANGE_LEN + 1,
                max: MAX_WIDGET_RANGE_LEN,
            })
        );
        let bytes = bincode::serialize(&over).expect("serialize over page request");
        assert!(
            bincode::deserialize::<WidgetEvent>(&bytes).is_err(),
            "cap+1 page width must refuse at decode"
        );
    }

    #[test]
    fn widget_op_tablecell_pin_extends_0_to_3() {
        let op_tag = |op: &WidgetOp| -> u32 {
            let bytes = bincode::serialize(op).expect("serialize widget op");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let cell_tag = |cell: &TableCell| -> u32 {
            let bytes = bincode::serialize(cell).expect("serialize table cell");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let tone_tag = |tone: &BadgeTone| -> u32 {
            let bytes = bincode::serialize(tone).expect("serialize badge tone");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let kind_tag = |kind: &CellKind| -> u32 {
            let bytes = bincode::serialize(kind).expect("serialize cell kind");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };

        // The ORIGINAL three op pins re-asserted (the table cell extends;
        // nothing shifted).
        let icon = wicon(71, "noop");
        assert_eq!(
            op_tag(&WidgetOp::Add {
                parent: None,
                widget: icon.clone()
            }),
            0
        );
        assert_eq!(
            op_tag(&WidgetOp::Update {
                id: sid(71),
                content: icon
            }),
            1
        );
        assert_eq!(op_tag(&WidgetOp::Remove { id: sid(71) }), 2);
        // The NEW tail pin.
        assert_eq!(
            op_tag(&WidgetOp::TableCell {
                table: sid(1),
                key: sid(2),
                column: sid(3),
                value: WidgetCellValue::Text("v".to_string()),
            }),
            3
        );

        // The typed-cell vocabulary pins 0..=7 + the tone/kind pins.
        assert_eq!(cell_tag(&TableCell::Text("t".to_string())), 0);
        assert_eq!(cell_tag(&TableCell::Number(1.0)), 1);
        assert_eq!(cell_tag(&TableCell::Bool(true)), 2);
        assert_eq!(cell_tag(&TableCell::Icon(icon_ref("svg:i"))), 3);
        assert_eq!(
            cell_tag(&TableCell::Link {
                label: "l".to_string(),
                uri: "svg:u".to_string(),
            }),
            4
        );
        assert_eq!(
            cell_tag(&TableCell::Badge {
                label: "b".to_string(),
                tone: BadgeTone::Neutral,
            }),
            5
        );
        assert_eq!(
            cell_tag(&TableCell::Progress {
                fraction: 0.5,
                label: None,
            }),
            6
        );
        assert_eq!(
            cell_tag(&TableCell::Date {
                y: 2026,
                m: 1,
                d: 2
            }),
            7
        );
        let tones: Vec<BadgeTone> = vec![
            BadgeTone::Info,
            BadgeTone::Success,
            BadgeTone::Warning,
            BadgeTone::Danger,
            BadgeTone::Neutral,
        ];
        for (index, tone) in tones.iter().enumerate() {
            assert_eq!(tone_tag(tone), index as u32, "BadgeTone pin drift");
        }
        assert_eq!(kind_tag(&CellKind::Text), 0);
        assert_eq!(kind_tag(&CellKind::Number), 1);
        assert_eq!(kind_tag(&CellKind::Bool), 2);
        assert_eq!(kind_tag(&CellKind::Icon), 3);
        assert_eq!(kind_tag(&CellKind::Link), 4);
        assert_eq!(kind_tag(&CellKind::Badge), 5);
        assert_eq!(kind_tag(&CellKind::Progress), 6);
        assert_eq!(kind_tag(&CellKind::Date), 7);

        // serde tags lowercase on both enums (externally tagged JSON).
        assert_eq!(
            serde_json::to_value(TableCell::Badge {
                label: "b".to_string(),
                tone: BadgeTone::Danger,
            })
            .expect("cell json"),
            serde_json::json!({"badge": {"label": "b", "tone": "danger"}})
        );
        assert_eq!(
            serde_json::to_value(CellKind::Date).expect("kind json"),
            serde_json::json!("date")
        );
        assert_eq!(
            serde_json::to_value(BadgeTone::Info).expect("tone json"),
            serde_json::json!("info")
        );
    }

    #[test]
    fn legacy_string_table_bytes_stay_stable() {
        // THE gate: the legacy string-table (`DialogControl::Table` +
        // `DialogValue::Table`) stays BYTE-COMPATIBLE. The typed tables are
        // layered BESIDE the legacy control, never replacing it in place;
        // the only bridge into `dialog` is this TEST (one-way: the module's
        // types never import it). The bytes of a `DialogValue::Table` are a
        // COMMITTED FIXTURE (copied verbatim from a run — the same discipline
        // as the creation pins).
        let legacy = crate::dialog::DialogValue::Table(vec![vec!["a".to_string()]]);
        let bytes = bincode::serialize(&legacy).expect("serialize legacy table value");
        let fixture: [u8; 29] = [
            // DialogValue::Table = tag 7 (u32 LE) + outer vec len 1 (u64)
            // + inner vec len 1 (u64) + "a" len 1 (u64) + 'a'.
            7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, b'a',
        ];
        assert_eq!(
            bytes, fixture,
            "legacy DialogValue::Table bytes drifted (THE gate)"
        );
        assert_eq!(
            bincode::deserialize::<crate::dialog::DialogValue>(&bytes).expect("legacy decode"),
            legacy
        );

        // The legacy CONTROL stays tag 8 and roundtrips (headers/rows/
        // allow_add/allow_delete untouched).
        let control = crate::dialog::DialogControl::Table {
            id: "bom".to_string(),
            label: Some("Bill of Materials".to_string()),
            headers: vec!["Item".to_string(), "Qty".to_string()],
            rows: vec![
                vec!["Bolt M6".to_string(), "8".to_string()],
                vec!["Nut M6".to_string(), "8".to_string()],
            ],
            allow_add: true,
            allow_delete: false,
        };
        let bytes = bincode::serialize(&control).expect("serialize legacy control");
        assert_eq!(
            u32::from_le_bytes(bytes[..4].try_into().expect("tag")),
            8,
            "DialogControl::Table pin drifted"
        );
        assert_eq!(
            bincode::deserialize::<crate::dialog::DialogControl>(&bytes)
                .expect("legacy control decode"),
            control
        );
    }

    #[test]
    fn cell_value_fraction_badge_link_date_roundtrip() {
        // GATES: progress fraction finite 0..=1 (1.5 / NaN / -0.5 refused);
        // date magnitude 1..=12 × 1..=31 (month 13 / day 32 / day 0
        // refused); number cells finite; text/uri caps.
        let bad_fraction = table_def(
            1,
            vec![column(2, CellKind::Progress, "p")],
            vec![trow_typed(
                3,
                vec![TableCell::Progress {
                    fraction: 1.5,
                    label: None,
                }],
            )],
        );
        assert_eq!(
            bad_fraction.validate(),
            Err(WidgetError::InvalidCell {
                table: sid(1),
                key: sid(3),
                column: sid(2),
                reason: "progress fraction must be finite within 0.0..=1.0".to_string(),
            })
        );
        let negative = table_def(
            1,
            vec![column(2, CellKind::Progress, "p")],
            vec![trow_typed(
                3,
                vec![TableCell::Progress {
                    fraction: -0.5,
                    label: None,
                }],
            )],
        );
        assert_eq!(
            negative.validate(),
            Err(WidgetError::InvalidCell {
                table: sid(1),
                key: sid(3),
                column: sid(2),
                reason: "progress fraction must be finite within 0.0..=1.0".to_string(),
            })
        );
        let nan = table_def(
            1,
            vec![column(2, CellKind::Progress, "p")],
            vec![trow_typed(
                3,
                vec![TableCell::Progress {
                    fraction: f32::NAN,
                    label: None,
                }],
            )],
        );
        assert!(
            matches!(nan.validate(), Err(WidgetError::InvalidCell { .. })),
            "NaN fraction must refuse (NaN compares false, so matches! is the assert)"
        );

        let month_13 = table_def(
            1,
            vec![column(2, CellKind::Date, "d")],
            vec![trow_typed(
                3,
                vec![TableCell::Date {
                    y: 2026,
                    m: 13,
                    d: 1,
                }],
            )],
        );
        assert!(
            matches!(
                month_13.validate(),
                Err(WidgetError::InvalidCell { reason, .. }) if reason.contains("month")
            ),
            "month 13 must refuse"
        );
        let day_32 = table_def(
            1,
            vec![column(2, CellKind::Date, "d")],
            vec![trow_typed(
                3,
                vec![TableCell::Date {
                    y: 2026,
                    m: 7,
                    d: 32,
                }],
            )],
        );
        assert!(
            matches!(
                day_32.validate(),
                Err(WidgetError::InvalidCell { reason, .. }) if reason.contains("day")
            ),
            "day 32 must refuse"
        );
        let day_0 = table_def(
            1,
            vec![column(2, CellKind::Date, "d")],
            vec![trow_typed(
                3,
                vec![TableCell::Date {
                    y: 2026,
                    m: 7,
                    d: 0,
                }],
            )],
        );
        assert!(
            matches!(
                day_0.validate(),
                Err(WidgetError::InvalidCell { reason, .. }) if reason.contains("day")
            ),
            "day 0 must refuse"
        );

        let nan_number = table_def(
            1,
            vec![column(2, CellKind::Number, "n")],
            vec![trow_typed(3, vec![TableCell::Number(f64::NAN)])],
        );
        assert!(
            matches!(
                nan_number.validate(),
                Err(WidgetError::InvalidCell { reason, .. }) if reason.contains("finite")
            ),
            "NaN number cell must refuse"
        );
        let long_text = table_def(
            1,
            vec![column(2, CellKind::Text, "t")],
            vec![trow_typed(
                3,
                vec![TableCell::Text("x".repeat(MAX_TEXT_BYTES + 1))],
            )],
        );
        assert!(
            matches!(
                long_text.validate(),
                Err(WidgetError::InvalidCell { reason, .. }) if reason.contains("cap")
            ),
            "over-cap cell text must refuse"
        );

        // The VALID all-kinds def roundtrips BOTH formats; badge/link/icon/
        // progress/date facts travel the registry verbatim.
        let def = all_kinds_table(1);
        def.validate().expect("the all-kinds def validates");
        let bytes = bincode::serialize(&def).expect("serialize def");
        let decoded: PanelTableDef = bincode::deserialize(&bytes).expect("decode def");
        assert_eq!(def, decoded);
        let json = serde_json::to_string(&def).expect("def json");
        let from_json: PanelTableDef = serde_json::from_str(&json).expect("decode def json");
        assert_eq!(def, from_json);
        // Sample the typed facts out of the decoded def.
        assert_eq!(
            decoded.rows[0].cells[5],
            TableCell::Badge {
                label: "wip".to_string(),
                tone: BadgeTone::Warning,
            }
        );
        assert_eq!(
            decoded.rows[0].cells[6],
            TableCell::Progress {
                fraction: 0.25,
                label: Some("quarter".to_string()),
            }
        );
    }

    #[test]
    fn table_ref_registry_missing_ref_rejected() {
        // A table ref naming an id absent from a PUBLISHING registry
        // refuses (UnknownTableRef — the "a table identity does not
        // resolve" arm), both at surface validate AND at patch-apply (a
        // payload can never add a dangling ref once the registry is
        // engaged). The PLACEHOLDER TRANSITION regimes are asserted first:
        // empty registry = the SWG-001 era (refs resolve-check-free).
        let def = table_def(
            2,
            vec![column(3, CellKind::Text, "a")],
            vec![trow_typed(4, vec![TableCell::Text("x".to_string())])],
        );

        // Regime 1: EMPTY registry + placeholder ref → the SWG-001 era
        // shape (the creation tests' surfaces) — valid.
        let placeholder = WidgetSurface {
            revision: SceneRevision(1),
            roots: vec![wtable(10, 404)],
            tables: BTreeMap::new(),
        };
        assert_eq!(placeholder.validate(), Ok(()));

        // Regime 2: PUBLISHING registry + RESOLVED ref → valid.
        let mut surface = surface_with_table(1, 10, 2, def.clone());
        surface.validate().expect("referencing surface validates");

        // Regime 3: PUBLISHING registry + dangling ref → refuses, address
        // in the error.
        let dangling = WidgetSurface {
            revision: SceneRevision(1),
            roots: vec![wtable(10, 404)],
            tables: BTreeMap::from([(sid(2), def)]),
        };
        let error = dangling
            .validate()
            .expect_err("dangling table ref must refuse");
        assert_eq!(error, WidgetError::UnknownTableRef(sid(404)));
        assert!(
            error.to_string().contains("does not resolve"),
            "the UnknownTableRef arm reports the unresolvable identity: {error}"
        );

        // Patch leg: Add of a dangling ref refuses; the surface unchanged
        // (the registry rides snapshots only, so the payload could never
        // become resolvable later).
        let before = surface.roots.clone();
        let patch = WidgetSurfacePatch {
            base_revision: SceneRevision(1),
            revision: SceneRevision(2),
            ops: vec![WidgetOp::Add {
                parent: None,
                widget: wtable(11, 404),
            }],
        };
        let error = surface
            .apply_patch(patch)
            .expect_err("dangling ref patch must refuse");
        assert_eq!(error, WidgetError::UnknownTableRef(sid(404)));
        assert_eq!(surface.roots, before);
        assert_eq!(surface.revision, SceneRevision(1));

        // A ref to a RESOLVED table adds fine.
        surface
            .apply_patch(WidgetSurfacePatch {
                base_revision: SceneRevision(1),
                revision: SceneRevision(2),
                ops: vec![WidgetOp::Add {
                    parent: None,
                    widget: wtable(11, 2),
                }],
            })
            .expect("ref to a registry table applies");
        surface.validate().expect("still valid");
    }
}
