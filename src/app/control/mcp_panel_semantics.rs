//! The semantic MCP ops' HOST half (SWG-005): `panel_semantics` /
//! `panel_page` reads and the ONE semantic mutation path,
//! `panel_widget_invoke`.
//!
//! The MCP bridge (`src/mcp.rs`) forwards these ops to the GUI through the
//! ordinary control envelope (`Message::ControlRequest` — the same pipe every
//! automation op rides); the dispatch sites in `control/mod.rs` gained the
//! thin semantic branches and the substantive machinery lives HERE, one
//! focused file, so the op surface stays reviewable as a unit.
//!
//! # The ONE path rule (§15/§16)
//!
//! `panel_widget_invoke` does NOT parallel the GUI mount arm: it routes the
//! EXACT `WidgetMountMsg` vocabulary the native mount emits
//! ([`WidgetMountMsg::SelectionChanged`] for explicit targets, then
//! [`WidgetMountMsg::ButtonClicked`] / [`WidgetMountMsg::MenuItemClicked`])
//! through [`route_mount_msg_owned`] — the very bridge core the
//! `Message::WidgetMountMsg` update arm calls — and finishes with the arm's
//! OWN two tail helpers ([`OpenCADStudio::forward_mount_events`] and
//! [`OpenCADStudio::arm_widget_prompt_modal`]). The equivalence suite below
//! drives BOTH entry points (the GUI arm AND this control arm) against the
//! same machines and pins identical selection, refusal, settle-once and
//! prompt-arming behavior — the §16 "GUI and MCP produce the same
//! event/validation/revision" acceptance.
//!
//! # Reads are bounded, summaries-first, and never auto-fetch
//!
//! `panel_semantics` reports the attached surfaces' identity (frozen surface
//! id, revisions), the BOUNDED summaries ([`StoreStateDigest`] — kind
//! counts, never payload dumps), the machine's selection/focus, the armed
//! pending prompt, and TARGETED effective enablement/visibility (the caller
//! names the address: a button, a menu item, or a row/item). `panel_page`
//! serves the SAME store page windows the GUI renders
//! ([`PanelSurfaceStore::store_page`]: past-end = empty, width-capped) with
//! explicit offset+limit. A page miss is REPORTED
//! (`requested_span_unheld`), never raised: a read must not ask the owning
//! plugin to fetch (the lazy REQUEST is a GUI gesture's event, not an MCP
//! read's side effect — "lazy-children state visible as a flag, never
//! auto-fetch").
//!
//! # Unattached / absent is a shape, not an error
//!
//! A panel with no store entry answers the store's own empty idiom
//! (`attached: false`, the all-`None` digest) with `ok: true` — the
//! `StoreReadOutcome::default` / empty-snapshot shape, structured. Scope
//! assertions that name a CLOSED document are refused by the ordinary
//! document gate before this module runs (`document_closed` — the real
//! close path's answer; a per-tab store identity does not exist in the v24
//! store, machines read-only, so tab scoping rides the caller's
//! `document_id` assertion exactly like every other automation op).
//!
//! # Non-goals honored here
//!
//! No machine edits (the machines are READ-ONLY consumers), no new wire
//! variants, no payload dumps. Table cell draft/edit is deliberately NOT
//! exposed: the typed-table machine exposes begin/draft/commit
//! (`plugin_widgets_tables.rs`), but no GUI mount route exists yet (the
//! mount's table placeholder is non-interactive), so a same-path MCP arm
//! cannot exist — recorded, skipped per the ONE-path rule.

// The v24 store vocabulary is desktop-only machinery (the plugin-host attach
// is); the whole module joins its cfg so the web build's control bridge
// answers `unknown_operation` for these op names (an honest absence — the
// store itself does not exist there).
#![cfg(not(target_arch = "wasm32"))]

use serde_json::{json, Value};

use ocs_plugin_api::ipc::protocol::{PanelStoreKind, StoreStateDigest};
use ocs_plugin_api::scene::{PanelScene, StableId};
use ocs_plugin_api::widgets::{WidgetEvent, WidgetRange, MAX_WIDGET_TARGET_IDS};

use super::{failure, OpenCADStudio};
use crate::app::update::widget_mount::route_mount_msg_owned;
use crate::ui::window::plugin_widgets_dispatch::DispatchKey;
use crate::ui::window::plugin_widget_mount::{
    MountPromptDesc, MountRouting, SurfaceMountHandle, WidgetMountMsg,
};

impl OpenCADStudio {
    /// `panel_semantics` — the bounded semantic state of ONE attached plugin
    /// panel (identity, summaries, selection, pending prompt, targeted
    /// effective enablement/visibility). A pure read: nothing machines,
    /// nothing forwards, nothing demands.
    pub(in crate::app) fn mcp_panel_semantics(&mut self, req: &Value) -> Value {
        let Some(panel_id) = req["panel_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
        else {
            return failure("invalid_request", "panel_id is required");
        };
        let registry = self.mount_registry.borrow();
        let Some(entry) = registry.panel_surface_store().entry(&panel_id) else {
            // The store's honest empty idiom: unattached is a shape, not an
            // error (the empty digest — `attached: false`).
            return json!({
                "ok": true,
                "status": "completed",
                "op": "panel_semantics",
                "panel_id": panel_id,
                "attached": false,
                "summary": {"scene": null, "widgets": null},
                "machine": false,
                "selection": [],
            });
        };
        let surface_id = entry.surface_id;
        let summary = StoreStateDigest {
            scene: entry.scene.as_ref().map(PanelScene::scene_summary),
            widgets: entry.widgets.as_ref().map(|w| w.widget_summary()),
        };
        let machine = surface_id
            .and_then(|id| registry.handle_for_surface_owned(id))
            .is_some();
        let mut out = json!({
            "ok": true,
            "status": "completed",
            "op": "panel_semantics",
            "panel_id": panel_id,
            "attached": true,
            "owner": entry.owner_plugin_id,
            "surface_id": surface_id.map(|id| id.0),
            "revisions": {
                "scene": entry.scene.as_ref().map(|scene| scene.revision.0),
                "widgets": entry.widgets.as_ref().map(|widgets| widgets.revision.0),
            },
            "summary": serde_json::to_value(&summary).expect("StoreStateDigest serializes"),
            // The machines' half: present only while a registered mount
            // serves the attached widget leg (a scene-only attach reports
            // machine: false — the summaries stay the truth it holds).
            "machine": machine,
            "selection": [],
        });
        // The machines' live reads (selection, focus, armed prompt).
        if let Some(handle) = surface_id.and_then(|id| registry.handle_for_surface_owned(id)) {
            let state = handle.state.borrow();
            out["selection"] =
                json!(state.selection().iter().map(|id| id.0).collect::<Vec<_>>());
            out["focus"] = json!(state.focus().map(|id| id.0));
            drop(state);
            if let Some(desc) = registry.prompt_dialog_owned(surface_id.expect("machine")) {
                out["pending_prompt"] = prompt_desc_json(&desc);
            }
        }
        drop(registry);
        // TARGETED reads: the caller names the address; a read never
        // enumerates (the flood rule).
        if let Some(targeted) = self.mcp_panel_targeted_read(req) {
            out["targeted"] = targeted;
        }
        out
    }

    /// The TARGETED read behind `panel_semantics.targeted` — one address the
    /// caller named, resolved through the SAME machines the mount renders
    /// from (the dispatcher's effective fold for buttons/menu items, the
    /// synced row views + expansion state for rows/items).
    fn mcp_panel_targeted_read(&mut self, req: &Value) -> Option<Value> {
        let button_id = req["button_id"].as_u64();
        let menu_id = req["menu_id"].as_u64();
        let command_id = req["command_id"].as_str().filter(|s| !s.is_empty());
        let row_id = req["row_id"].as_u64();
        if button_id.is_none() && command_id.is_none() && row_id.is_none() {
            return None;
        }
        if button_id.is_some() && (menu_id.is_some() || command_id.is_some()) {
            return Some(failure(
                "invalid_request",
                "button_id is mutually exclusive with a menu-item address",
            ));
        }
        let Some((_surface_id, handle)) = self.attached_widget_mount(req) else {
            return Some(json!({
                "enabled": null,
                "visible": null,
                "reason": "machine_absent",
            }));
        };
        if let Some(button_id) = button_id {
            let dispatcher = handle.dispatcher.borrow();
            let key = DispatchKey::Button(StableId(button_id));
            return Some(json!({
                "address": {"button": button_id},
                "enabled": dispatcher.effective_enabled(&key),
                "visible": dispatcher.effective_visible(&key),
            }));
        }
        if let Some(command_id) = command_id {
            let Some(menu_id) = menu_id else {
                return Some(failure(
                    "invalid_request",
                    "command_id needs menu_id (the item's owning menu)",
                ));
            };
            let dispatcher = handle.dispatcher.borrow();
            let key = DispatchKey::MenuItem(StableId(menu_id), command_id.to_owned());
            return Some(json!({
                "address": {"menu": menu_id, "command": command_id},
                "enabled": dispatcher.effective_enabled(&key),
                "visible": dispatcher.effective_visible(&key),
            }));
        }
        let row_id = StableId(row_id.expect("checked above"));
        let state = handle.state.borrow();
        let Some(view) = state.synced().and_then(|synced| synced.views.get(&row_id)) else {
            return Some(json!({
                "address": {"row": row_id.0},
                "enabled": null,
                "lazy_children": null,
                "expanded": null,
                "reason": "unknown_row",
            }));
        };
        let targeted = json!({
            "address": {"row": row_id.0},
            "enabled": view.enabled,
            "lazy_children": view.lazy_children,
            "expanded": state.effective_expanded(row_id, false),
        });
        Some(targeted)
    }

    /// `panel_page` — ONE bounded store page (tree rows / list items / table
    /// rows), the SAME window the GUI's store machine serves: past-end is
    /// empty, the width is the store's own cap. The page miss a GUI gesture
    /// would turn into a lazy fetch is REPORTED (`requested_span_unheld`),
    /// never raised — a read does not ask the plugin to fetch.
    pub(in crate::app) fn mcp_panel_page(&mut self, req: &Value) -> Value {
        let Some(panel_id) = req["panel_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
        else {
            return failure("invalid_request", "panel_id is required");
        };
        let kind = match req["kind"].as_str().unwrap_or("tree_children") {
            "tree_children" => PanelStoreKind::TreeChildren,
            "list_items" => PanelStoreKind::ListItems,
            "table_rows" => PanelStoreKind::TableRows,
            other => {
                return failure(
                    "invalid_request",
                    format!("unknown page kind {other:?} (tree_children, list_items, table_rows)"),
                )
            }
        };
        let offset = req["offset"].as_u64().unwrap_or(0) as usize;
        // The GUI's listing default (the commands op's page width); the
        // store machine caps the SERVED width by its own page budget, so a
        // bigger ask widens nothing.
        let limit = req["limit"].as_u64().unwrap_or(200) as usize;
        let range = WidgetRange {
            start: offset,
            len: limit.min(ocs_plugin_api::widgets::MAX_WIDGET_RANGE_LEN),
        };
        let parent_id = req["parent_id"].as_u64().map(StableId);
        let registry = self.mount_registry.borrow();
        let outcome = registry
            .panel_surface_store()
            .store_page(&panel_id, kind, parent_id, range);
        let rows: Vec<Value> = outcome
            .rows
            .iter()
            .map(|row| serde_json::to_value(row).expect("StoreRow serializes"))
            .collect();
        let count = rows.len();
        json!({
            "ok": true,
            "status": "completed",
            "op": "panel_page",
            "panel_id": panel_id,
            "kind": match kind {
                PanelStoreKind::TreeChildren => "tree_children",
                PanelStoreKind::ListItems => "list_items",
                PanelStoreKind::TableRows => "table_rows",
                // Not exposed to MCP (the scene leg's bounded shape is the
                // summaries; see the module docs).
                PanelStoreKind::SceneRegion => "scene_region",
            },
            "parent_id": parent_id.map(|id| id.0),
            "offset": offset,
            "limit": limit,
            "surface_id": outcome.surface_id.map(|id| id.0),
            "total_hint": outcome.total_hint,
            "count": count,
            "rows": rows,
            // The lazy/page miss REPORT (the demand a GUI gesture would
            // raise). A read never raises it — ask again later; the owning
            // plugin publishes the span when it is ready.
            "requested_span_unheld": outcome.demand.map(|(parent, span)| {
                json!({
                    "parent_id": parent.map(|id| id.0),
                    "start": span.start,
                    "len": span.len,
                })
            }),
        })
    }

    /// `panel_widget_invoke` — the ONE semantic mutation path: a stable
    /// command address through [`route_mount_msg_owned`] (the GUI mount
    /// arm's own bridge core), the arm's own forwarding + prompt-arming
    /// tail, and the machines' own refusals as the structured no.
    pub(in crate::app) fn mcp_panel_widget_invoke(
        &mut self,
        req: &Value,
    ) -> Result<Value, Value> {
        let Some(panel_id) = req["panel_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
        else {
            return Err(failure("invalid_request", "panel_id is required"));
        };
        let button_id = req["button_id"].as_u64();
        let menu_id = req["menu_id"].as_u64();
        let command_id = req["command_id"].as_str().filter(|s| !s.is_empty());
        // The bridge's validator (mcp_ops PANEL_WIDGET_INVOKE) enforces
        // exactly-one address; this re-check is the control bridge's own
        // defense (the REST/web clients reach the same handler).
        if button_id.is_some() == command_id.is_some() {
            return Err(failure(
                "invalid_request",
                "Supply exactly one address: button_id, or menu_id + command_id",
            ));
        }
        if command_id.is_some() && menu_id.is_none() {
            return Err(failure(
                "invalid_request",
                "command_id needs menu_id (the item's owning menu)",
            ));
        }
        let targets: Option<Vec<StableId>> = match req["targets"].as_array() {
            Some(list) => {
                if list.len() > MAX_WIDGET_TARGET_IDS {
                    return Err(failure(
                        "invalid_request",
                        format!("targets exceed the {MAX_WIDGET_TARGET_IDS}-id event cap"),
                    ));
                }
                let mut ids = Vec::with_capacity(list.len());
                for value in list {
                    let Some(id) = value.as_u64() else {
                        return Err(failure(
                            "invalid_request",
                            "targets are row/item ids (non-negative integers)",
                        ));
                    };
                    ids.push(StableId(id));
                }
                Some(ids)
            }
            None => None,
        };

        // The attached widget leg (store identity → registered mount).
        let Some((surface_id, handle)) = self.attached_widget_mount(req) else {
            return Err(failure(
                "panel_not_found",
                "The panel has no attached, mounted widget surface",
            ));
        };

        // Explicit targets arm the surface's selection FIRST — the same
        // machine route a GUI selection gesture runs (the mount routes the
        // surface's CURRENT selection as the click's targets). The
        // selection-capable widget resolves deterministically (lowest id)
        // so the route needs no screen coordinates (§15).
        let mut selection_armed = false;
        if let Some(targets) = targets {
            let widget_id = {
                let state = handle.state.borrow();
                state
                    .synced()
                    .map(|synced| synced.widget_views.keys().copied().min())
                    .unwrap_or(None)
            };
            let Some(widget_id) = widget_id else {
                return Err(failure(
                    "invalid_request",
                    "The surface has no selection-capable widget for explicit targets",
                ));
            };
            let msg = WidgetMountMsg::SelectionChanged {
                surface_id,
                widget_id,
                selected: targets,
            };
            let outcome =
                route_mount_msg_owned(&mut self.mount_registry.borrow_mut(), &handle, &msg);
            if let MountRouting::Refused(reason) = &outcome.routing {
                return Err(failure("refused", reason.clone()));
            }
            // The selection event forwards through the arm's own helper —
            // the sink's exactly-once drain rides the same route.
            self.forward_mount_events(surface_id, outcome.events);
            selection_armed = true;
        }

        // The click — the exact message the native mount emits for the
        // gesture, through the SAME bridge core the update arm calls.
        let msg = match (button_id, menu_id.zip(command_id)) {
            (Some(widget_id), _) => WidgetMountMsg::ButtonClicked {
                surface_id,
                widget_id: StableId(widget_id),
            },
            (None, Some((menu_id, command_id))) => WidgetMountMsg::MenuItemClicked {
                surface_id,
                menu_id: StableId(menu_id),
                command_id: command_id.to_owned(),
            },
            (None, None) => unreachable!("exactly-one address checked above"),
        };
        let outcome =
            route_mount_msg_owned(&mut self.mount_registry.borrow_mut(), &handle, &msg);
        // The arm's own tail: the drained events forward to the owning
        // plugin (bounded batches, exactly-once), an armed confirmation
        // opens the host's prompt modal (the SAME lifecycle the GUI click
        // arms — equivalence, not a bypass).
        let routed_events = outcome.events;
        self.forward_mount_events(surface_id, routed_events.clone());
        if matches!(outcome.routing, MountRouting::PromptRequested { .. }) {
            self.arm_widget_prompt_modal(surface_id);
        }
        match outcome.routing {
            MountRouting::Applied { forwarded } => {
                let events: Vec<Value> = routed_events.iter().map(event_json).collect();
                Ok(json!({
                    "ok": true,
                    "status": "completed",
                    "op": "panel_widget_invoke",
                    "panel_id": panel_id,
                    "forwarded": forwarded,
                    "selection_armed": selection_armed,
                    "settled": outcome.settled,
                    "events": events,
                }))
            }
            MountRouting::PromptRequested { prompt } => {
                let pending = self
                    .mount_registry
                    .borrow()
                    .prompt_dialog_owned(surface_id)
                    .map(|desc| prompt_desc_json(&desc))
                    .unwrap_or(Value::Null);
                Ok(json!({
                    "ok": true,
                    "status": "waiting_input",
                    "op": "panel_widget_invoke",
                    "panel_id": panel_id,
                    "code": "confirm_prompt",
                    "prompt": prompt,
                    "pending_prompt": pending,
                    "next": "The host's confirmation dialog is armed for the person at the screen; the command emits exactly once if they confirm. Re-read op panel_semantics for the armed state and the outcome.",
                }))
            }
            MountRouting::Refused(reason) => Err(failure("refused", reason)),
        }
    }

    /// The attached widget mount for a request's `panel_id`: the store
    /// entry's frozen surface id → the registered mount handle. `None`
    /// when the panel is unattached, scene-only (no widget leg), or the
    /// widget leg is not registered — the machine-absent shape.
    fn attached_widget_mount(&mut self, req: &Value) -> Option<(StableId, SurfaceMountHandle)> {
        let panel_id = req["panel_id"].as_str()?;
        let registry = self.mount_registry.borrow();
        let surface_id = registry
            .panel_surface_store()
            .entry(panel_id)
            .and_then(|entry| entry.surface_id)?;
        registry
            .handle_for_surface_owned(surface_id)
            .map(|handle| (surface_id, handle))
    }
}

// ---------------------------------------------------------------------------
// JSON shapes
// ---------------------------------------------------------------------------

/// The armed prompt's bounded shape (the desc's own fields — display copy
/// mirroring the machine's captured stage: command, targets, prompt text).
fn prompt_desc_json(desc: &MountPromptDesc) -> Value {
    json!({
        "command": desc.command_id,
        "targets": desc.targets.iter().map(|id| id.0).collect::<Vec<_>>(),
        "prompt": desc.prompt,
    })
}

/// One forwarded [`WidgetEvent`] as its serde shape (`{"CommandInvoked":{…}}`
/// — the crate's own vocabulary, byte-faithful).
fn event_json(event: &WidgetEvent) -> Value {
    serde_json::to_value(event).expect("WidgetEvent serializes")
}

// ---------------------------------------------------------------------------
// Tests (headless, deterministic) — the §16 GUI/MCP equivalence suite
// ---------------------------------------------------------------------------
//
// The suite drives the SAME machine-level assertions through BOTH entry
// points: the GUI mount arm (`route_mount_msg_owned` + the arm's own two
// tail helpers — exactly the body of `on_widget_mount_msg`) and the MCP arm
// (`app.control_request(..)` — the envelope the bridge forwards). Fixture:
// the standard list surface (rows 7/8, surface id 200) attached through the
// store's publish → the mount's register — exactly the wire attach's two
// legs, in its order.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use ocs_plugin_api::ipc::protocol::SceneOrWidgetSurface;
    use ocs_plugin_api::scene::SceneRevision;
    use ocs_plugin_api::widgets::{
        ConfirmPolicy, IconActionDef, IconRef, ItemDef, ItemListDef, TreeCapabilities, TreeRowDef,
        WidgetNode, WidgetSurface, WidgetTreeDef,
    };
    use std::collections::BTreeMap;

    fn sid(n: u64) -> StableId {
        StableId(n)
    }

    /// Unique request ids per call (the bridge mints one per invocation;
    /// a replayed body otherwise answers from the completed cache).
    static INVOKE_SEQ: AtomicUsize = AtomicUsize::new(1);

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
            tables: BTreeMap::new(),
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

    /// THE attach's two legs, in the attach's own order: the store's
    /// publish (the store entry + the frozen surface id) then the mount's
    /// register (the machines). Returns the frozen surface id.
    fn attach(app: &mut OpenCADStudio, panel: &str, surf: WidgetSurface) -> StableId {
        if let Err(reason) = surf.validate() {
            panic!("fixture surface invalid: {reason:?}");
        }
        let mut registry = app.mount_registry.borrow_mut();
        let outcome = registry
            .panel_surface_store_mut()
            .publish("test-plugin", panel, SceneOrWidgetSurface::Widgets(surf))
            .expect("the fixture surface publishes");
        let (frozen, surface) = outcome.widget_mount.expect("a widget publish mounts");
        registry.register_surface_owned(
            panel,
            crate::ui::window::plugin_widget_mount::SurfaceMountHandle::from_surface(
                frozen, surface,
            ),
        );
        frozen
    }

    /// The store+mount fixture with the dispatcher's command read model
    /// synced (the owner plugin's publish step the registered machines
    /// carry in the full app). Returns the frozen surface id.
    fn fixture(app: &mut OpenCADStudio, panel: &str, def: IconActionDef) -> StableId {
        let frozen = attach(
            app,
            panel,
            surface(vec![
                WidgetNode::List(item_list(200, vec![item(7, "row 1"), item(8, "row 2")])),
                WidgetNode::IconButton(def.clone()),
            ]),
        );
        let handle = app
            .mount_registry
            .borrow()
            .handle_for_surface_owned(frozen)
            .expect("the registered surface");
        handle.dispatcher.borrow_mut().sync_button(&def);
        frozen
    }

    fn read_app(app: &mut OpenCADStudio, request: Value) -> Value {
        let (response, _task) = app.control_request(request);
        response
    }

    fn invoke_mcp(app: &mut OpenCADStudio, body: Value) -> Result<Value, Value> {
        // The bridge inserts the ACTIVE document_id on every non-read op
        // (insert_default from its cached state) and mints a unique
        // request_id per call — mirror exactly that, so the envelope
        // matches what a real bridge sends.
        let doc = read_app(app, json!({"protocol":1,"op":"state"}))["document_id"]
            .as_u64()
            .expect("a document");
        let id = format!("t-invoke-{}", INVOKE_SEQ.fetch_add(1, Ordering::SeqCst));
        let mut request = json!({
            "protocol":1,
            "op":"panel_widget_invoke",
            "request_id":id,
            "document_id":doc,
        });
        for (key, value) in body.as_object().expect("an object body") {
            request[key.as_str()] = value.clone();
        }
        let (response, _task) = app.control_request(request);
        if response["ok"].as_bool() != Some(true) {
            return Err(response);
        }
        // The completion envelope wraps the op's own result: `result` (this
        // module's payload) — an interactive outcome also lifts the OUTER
        // status to waiting_input while the prompt modal owns the slot.
        Ok(response.get("result").cloned().unwrap_or(response))
    }

    /// The GUI mount arm, VERBATIM: the real `Message::WidgetMountMsg`
    /// update arm (the exact path a native mount gesture lands on). The
    /// equivalence assertions read the MACHINES afterwards (selection,
    /// counters, armed prompt) — machine-level facts on both sides.
    fn gui_arm(app: &mut OpenCADStudio, msg: WidgetMountMsg) {
        let _ = app
            .update(crate::app::Message::WidgetMountMsg(msg));
    }

    fn dispatcher_commands(app: &OpenCADStudio, surface_id: StableId) -> u64 {
        app.mount_registry
            .borrow()
            .handle_for_surface_owned(surface_id)
            .map(|handle| handle.dispatcher.borrow().counters().commands_emitted)
            .unwrap_or(0)
    }

    fn dispatch_counter(
        app: &OpenCADStudio,
        surface_id: StableId,
        read: impl Fn(&crate::ui::window::plugin_widgets_dispatch::DispatchCounters) -> u64,
    ) -> u64 {
        app.mount_registry
            .borrow()
            .handle_for_surface_owned(surface_id)
            .map(|handle| read(handle.dispatcher.borrow().counters()))
            .unwrap_or(0)
    }

    // -- 1. equivalence: selection + settle-once through BOTH arms ----------

    /// The §16 acceptance: the SAME machine-level assertions hold through
    /// the GUI arm and the MCP arm — same selection results, same settle
    /// behavior, same exactly-once counts.
    #[test]
    fn mcp_invoke_matches_gui_mount_arm() {
        let mut app = OpenCADStudio::new_for_test();
        let frozen = fixture(
            &mut app,
            "swg005_equivalence",
            button_def(1, "Delete", "delete"),
        );

        // (a) MCP: explicit targets arm the selection through the SAME
        // machine, then the click emits exactly once and settles.
        let response = invoke_mcp(
            &mut app,
            json!({"panel_id":"swg005_equivalence","button_id":1,"targets":[7]}),
        )
        .expect("the invoke applies");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["settled"], true);
        assert_eq!(response["selection_armed"], true);
        assert_eq!(response["forwarded"], 1);
        let events = response["events"].as_array().expect("events");
        assert_eq!(events.len(), 1, "exactly one command event");
        let event = &events[0]["CommandInvoked"];
        assert_eq!(event["command_id"], "delete");
        assert_eq!(event["target_ids"], json!([7]));

        // The MCP arm left the machine's selection at the offered targets:
        let handle = app
            .mount_registry
            .borrow()
            .handle_for_surface_owned(frozen)
            .expect("registered");
        assert_eq!(handle.state.borrow().selection(), [sid(7)]);
        assert_eq!(dispatcher_commands(&app, frozen), 1);

        // (b) GUI: the same gesture sequence through the REAL update arm
        // (the mount's message vocabulary) — produces the SAME machine
        // behavior: the selection lands, the click emits exactly once and
        // the arm's settle tail completes the address.
        gui_arm(
            &mut app,
            WidgetMountMsg::SelectionChanged {
                surface_id: frozen,
                widget_id: sid(200),
                selected: vec![sid(8)],
            },
        );
        assert_eq!(handle.state.borrow().selection(), [sid(8)]);
        assert_eq!(dispatcher_commands(&app, frozen), 1);

        gui_arm(
            &mut app,
            WidgetMountMsg::ButtonClicked {
                surface_id: frozen,
                widget_id: sid(1),
            },
        );
        assert_eq!(handle.state.borrow().selection(), [sid(8)]);

        // (c) The machine-level totals: two commands emitted (one per arm),
        // both settled — settle-once counting rides the ONE bridge core.
        assert_eq!(dispatcher_commands(&app, frozen), 2);
        assert!(
            !handle
                .dispatcher
                .borrow_mut()
                .settle(DispatchKey::Button(sid(1))),
            "nothing outstanding: both arms settled their own address"
        );
    }

    // -- 2. enablement refusals: disabled/hidden un-invocable from MCP ------

    /// The §16 "disabled commands un-invocable from GUI AND MCP" rule: the
    /// machine's OWN refusal surfaces as the host's structured no — and the
    /// GUI arm refuses the SAME way on the same address.
    #[test]
    fn mcp_refuses_disabled_like_the_gui() {
        let mut app = OpenCADStudio::new_for_test();
        let mut disabled = button_def(1, "Disabled", "delete");
        disabled.enabled = false;
        let frozen = fixture(&mut app, "swg005_refusals", disabled);

        // MCP arm: the machine's refusal, structured.
        let error = invoke_mcp(
            &mut app,
            json!({"panel_id":"swg005_refusals","button_id":1,"targets":[7]}),
        )
        .expect_err("a disabled command refuses");
        assert_eq!(error["code"], "refused");
        assert_eq!(error["ok"], false);
        assert!(
            error["error"].as_str().unwrap().contains("disabled"),
            "the machine's own refusal text: {error}"
        );

        // The GUI arm refuses the SAME address the SAME way (the machines'
        // own refusal counter has now moved once per arm — the SAME
        // refusal, from the SAME machine, in both entries):
        gui_arm(
            &mut app,
            WidgetMountMsg::ButtonClicked {
                surface_id: frozen,
                widget_id: sid(1),
            },
        );
        assert_eq!(
            dispatch_counter(&app, frozen, |c| c.activations_refused_disabled),
            2,
            "one machine refusal per arm (the MCP one + this one)"
        );

        // The machine state is untouched by the refusals (atomic — no
        // event existed to forward, nothing armed):
        assert_eq!(dispatcher_commands(&app, frozen), 0);

        // The targeted read names the SAME fact from the SAME machine:
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_semantics","panel_id":"swg005_refusals","button_id":1}),
        );
        assert_eq!(response["ok"], true);
        assert_eq!(response["targeted"]["enabled"], false);
        assert_eq!(response["targeted"]["visible"], true);
    }

    // -- 3. the prompt lifecycle: equivalence, not a bypass ------------------

    /// `ConfirmPolicy::Prompt` arms the SAME host prompt lifecycle from
    /// BOTH entry points: the machine's stage captures command + targets,
    /// the modal arms (the user still answers), the counters agree, and a
    /// re-invoke while armed refuses exactly like the GUI's re-click.
    #[test]
    fn mcp_prompt_arms_the_same_host_lifecycle() {
        let mut app = OpenCADStudio::new_for_test();
        let def = IconActionDef {
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Really delete the selection?".to_string(),
            }),
            ..button_def(1, "Delete", "delete")
        };
        let frozen = fixture(&mut app, "swg005_prompt", def);

        // MCP arm: the first invoke ARMS (waiting_input — no event yet).
        let response = invoke_mcp(
            &mut app,
            json!({"panel_id":"swg005_prompt","button_id":1,"targets":[7]}),
        )
        .expect("the prompt arms");
        assert_eq!(response["status"], "waiting_input");
        assert_eq!(response["code"], "confirm_prompt");
        assert_eq!(response["prompt"], "Really delete the selection?");
        assert_eq!(response["pending_prompt"]["command"], "delete");
        assert_eq!(response["pending_prompt"]["targets"], json!([7]));

        // The armed stage is the machine's (the registry desc is display
        // copy; the dispatcher's pending stage is the exchange authority):
        let handle = app
            .mount_registry
            .borrow()
            .handle_for_surface_owned(frozen)
            .expect("registered");
        assert_eq!(
            handle
                .dispatcher
                .borrow()
                .pending_prompt(&DispatchKey::Button(sid(1))),
            Some("Really delete the selection?")
        );
        assert_eq!(dispatcher_commands(&app, frozen), 0, "no event yet");

        // The pending prompt is visible through the READ op — the MCP
        // caller's eventual-outcome window:
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_semantics","panel_id":"swg005_prompt"}),
        );
        assert_eq!(response["pending_prompt"]["command"], "delete");

        // A re-invoke while armed refuses (the armed interaction IS the
        // in-flight guard — the machines' own refusal):
        let error = invoke_mcp(
            &mut app,
            json!({"panel_id":"swg005_prompt","button_id":1,"targets":[7]}),
        )
        .expect_err("the armed prompt refuses a re-invoke");
        assert_eq!(error["code"], "refused");

        // The USER confirms through the host modal (the GUI decision edge
        // — the dialog owns the interaction, MCP does not bypass it):
        let _ = app.update(Message::WidgetPromptMsg(
            crate::ui::window::plugin_widget_mount::WidgetPromptMsg::Confirmed {
                surface_id: frozen,
                key: DispatchKey::Button(sid(1)),
            },
        ));
        assert_eq!(
            dispatcher_commands(&app, frozen),
            1,
            "the confirm emitted the captured command exactly once"
        );
        assert_eq!(
            handle
                .dispatcher
                .borrow()
                .pending_prompt(&DispatchKey::Button(sid(1))),
            None,
            "the interaction completed"
        );

        // And the read shows the prompt gone (the eventual outcome shape):
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_semantics","panel_id":"swg005_prompt"}),
        );
        assert!(response["pending_prompt"].is_null());
    }

    // -- 4. unattached / absent reads are shapes, not errors -----------------

    #[test]
    fn mcp_reads_answer_honest_shapes() {
        let mut app = OpenCADStudio::new_for_test();
        let _frozen = fixture(&mut app, "swg005_reads", button_def(1, "Ping", "ping"));

        // Unattached panel: the store's empty idiom, ok:true.
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_semantics","panel_id":"no_such_panel"}),
        );
        assert_eq!(response["ok"], true);
        assert_eq!(response["attached"], false);
        assert_eq!(response["summary"]["widgets"], Value::Null);

        // An unattached page: the honest empty page (never a demand).
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_page","panel_id":"no_such_panel","kind":"list_items"}),
        );
        assert_eq!(response["ok"], true);
        assert_eq!(response["count"], 0);
        assert_eq!(response["rows"], json!([]));
        assert!(response["requested_span_unheld"].is_null());

        // Attached: the identity + summaries + machine facts ride together.
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_semantics","panel_id":"swg005_reads"}),
        );
        assert_eq!(response["attached"], true);
        assert_eq!(response["machine"], true);
        assert_eq!(response["surface_id"], 200);
        assert_eq!(response["revisions"]["widgets"], 1);
        assert_eq!(
            response["summary"]["widgets"]["widgets"],
            2,
            "one list + one icon button"
        );

        // An unknown row: the null-facts shape with the reason (structured,
        // still ok:true).
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_semantics","panel_id":"swg005_reads","row_id":999}),
        );
        assert_eq!(response["targeted"]["reason"], "unknown_row");
    }

    // -- 5. the store pages: the SAME windows the GUI renders ----------------

    /// `panel_page` serves the store's page machine verbatim: past-end is
    /// empty, the served width never exceeds the held rows, and the
    /// lazy-children flag rides each tree row — never an auto-fetch.
    #[test]
    fn mcp_pages_match_the_store_windows() {
        let mut app = OpenCADStudio::new_for_test();
        let rows: Vec<TreeRowDef> = (0..12u64)
            .map(|i| TreeRowDef {
                id: sid(100 + i),
                parent: None,
                label: format!("row {i}"),
                icon: None,
                status: None,
                expandable: i == 11,
                enabled: true,
                // The lazy flag is visible per row (the read never fetches;
                // a lazy row must be expandable — the crate's payload rule).
                lazy_children: i == 11,
                domain_ref: None,
            })
            .collect();
        let tree = WidgetTreeDef {
            id: sid(300),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            capabilities: TreeCapabilities::default(),
            rows,
        };
        let _frozen = attach(
            &mut app,
            "swg005_pages",
            surface(vec![WidgetNode::Tree(tree)]),
        );

        // The first page (width 4):
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_page","panel_id":"swg005_pages","kind":"tree_children","offset":0,"limit":4}),
        );
        assert_eq!(response["count"], 4);
        assert_eq!(response["total_hint"], 12);
        assert_eq!(response["rows"][0]["Tree"]["id"], 100);
        assert_eq!(response["rows"][3]["Tree"]["id"], 103);
        assert_eq!(response["rows"][0]["Tree"]["lazy_children"], false);

        // A past-end page is EMPTY (the store's checked bounds):
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_page","panel_id":"swg005_pages","kind":"tree_children","offset":12,"limit":4}),
        );
        assert_eq!(response["count"], 0);
        assert_eq!(response["rows"], json!([]));

        // The tail page: 4 held rows, the lazy flag visible on row 111.
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_page","panel_id":"swg005_pages","kind":"tree_children","offset":8,"limit":8}),
        );
        assert_eq!(response["count"], 4);
        assert_eq!(response["rows"][0]["Tree"]["id"], 108);
        assert_eq!(response["rows"][3]["Tree"]["id"], 111);
        assert_eq!(response["rows"][3]["Tree"]["lazy_children"], true);

        // An over-wide ask: the served width never exceeds the held rows.
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_page","panel_id":"swg005_pages","kind":"tree_children","offset":0,"limit":4096}),
        );
        assert_eq!(response["count"], 12);
    }

    // -- 6. scope assertions: the closed-tab refusal rides the real gate -----

    /// A scope-asserted read/action on a CLOSED document is refused on the
    /// real close path (the tab is gone from `self.tabs`): the ordinary
    /// document gate answers `document_closed` for both envelopes — the
    /// tab-scoped liveness the ops honestly have. MCP-side page state is
    /// stateless (explicit offset+limit per call — nothing retained to
    /// cancel); machine-side in-flight state releases at the plugin's
    /// teardown (the plugin-close half, pinned with the teardown pin).
    #[test]
    fn closed_document_scope_refuses_on_the_real_gate() {
        let mut app = OpenCADStudio::new_for_test();
        let _frozen = fixture(&mut app, "swg005_scope", button_def(1, "Ping", "ping"));
        // A REAL drawing tab (the sole start tab is not closeable); make it
        // the active one so the bridge's cached document_id targets it.
        let doc = app.push_test_document();
        let (response, _task) = app.control_request(json!({
            "protocol":1,
            "op":"activate",
            "request_id":"t-scope-activate",
            "document_id":doc,
        }));
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(doc, read_app(&mut app, json!({"protocol":1,"op":"state"}))["document_id"].as_u64().unwrap());

        // Close the tab (the REAL close path — `Message::TabClose` → the
        // tab removal in `app::update::command`).
        let _ = app.update(Message::TabClose(doc));

        // A scope-asserted invoke is refused by the ordinary gate:
        let (response, _task) = app.control_request(json!({
            "protocol":1,
            "op":"panel_widget_invoke",
            "request_id":"t-scope-invoke",
            "document_id":doc,
            "panel_id":"swg005_scope",
            "button_id":1,
        }));
        assert_eq!(response["code"], "document_closed");

        // And a scope-asserted read refuses the same way (the gate is
        // op-agnostic — the closed tab invalidates the scope):
        let (response, _task) = app.control_request(json!({
            "protocol":1,
            "op":"panel_semantics",
            "document_id":doc,
            "panel_id":"swg005_scope",
        }));
        assert_eq!(response["code"], "document_closed");
    }

    // -- 7. machine-absent: teardown releases the armed interaction ----------

    /// The plugin-close half of §16 "closing a tab/plugin cancels pending
    /// data requests and releases state": after the surface teardown (the
    /// reap join's effect), reads report `machine: false`, the armed desc
    /// is gone, and the invoke refuses — the machines' released state. The
    /// store leg (the plugin's own published truth) remains until the
    /// plugin republishes — recorded, not claimed otherwise.
    #[test]
    fn teardown_releases_the_machines_and_the_armed_prompt() {
        let mut app = OpenCADStudio::new_for_test();
        let def = IconActionDef {
            confirmation: Some(ConfirmPolicy::Prompt {
                message: "Really delete the selection?".to_string(),
            }),
            ..button_def(1, "Delete", "delete")
        };
        let _frozen = fixture(&mut app, "swg005_teardown", def);
        // Arm a prompt (the MCP path).
        let response = invoke_mcp(
            &mut app,
            json!({"panel_id":"swg005_teardown","button_id":1,"targets":[7]}),
        )
        .expect("the prompt arms");
        assert_eq!(response["status"], "waiting_input");

        // The teardown (the same fn the plugin-close reap join runs):
        app.mount_registry
            .borrow_mut()
            .teardown_surface_owned("swg005_teardown");

        // The machines are released: no machine read, no armed desc, and
        // the invoke refuses with the machine-absent shape.
        let response = read_app(
            &mut app,
            json!({"protocol":1,"op":"panel_semantics","panel_id":"swg005_teardown"}),
        );
        assert_eq!(response["ok"], true, "the READ is still a shape");
        assert_eq!(response["attached"], true, "the store leg remains");
        assert_eq!(response["machine"], false);
        assert!(response["pending_prompt"].is_null());

        let error = invoke_mcp(
            &mut app,
            json!({"panel_id":"swg005_teardown","button_id":1,"targets":[7]}),
        )
        .expect_err("a torn-down surface refuses");
        assert_eq!(error["code"], "panel_not_found");
    }
}
