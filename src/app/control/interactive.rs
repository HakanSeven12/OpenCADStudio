//! Interactive user-request operations — selection and point requests
//! answered by the person at the screen. A client asks the
//! person at the screen to pick entities; the operation stays pending
//! (`running` for pollers) until that person answers with Enter or Escape,
//! then resolves with the picked entities' full data.

use super::*;
use crate::app::automation::{entity_json, entity_type_matches};

impl OpenCADStudio {
    /// `user_select` — hand the screen to the person at the desk: they pick
    /// entities (optionally filtered by type / layer), Enter confirms, Escape
    /// cancels. The operation resolves only after that answer, so callers
    /// must treat `running` as "still waiting for the user", not as a hang.
    pub(super) fn control_user_select(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            return Err(failure("no_document", "Create or open a drawing first"));
        }
        // Enter and Escape are the answer keys; a live command or dialog owns
        // them, so refuse rather than race the user's keystrokes.
        if self.tabs[i].active_cmd.is_some() || self.active_modal.is_some() {
            return Err(failure(
                "user_input_busy",
                "The user is already interacting with a command or dialog",
            ));
        }
        if self.main_window.is_none() {
            return Err(failure(
                "gui_required",
                "user_select needs the desktop GUI where a person can answer",
            ));
        }
        if self.control.get_point.is_some() || self.control.user_select.is_some() {
            return Err(failure(
                "interactive_pending",
                "Another interactive request is already waiting for the user",
            ));
        }
        let detail = req["detail"].as_str().unwrap_or("full");
        if !matches!(detail, "summary" | "geometry" | "full") {
            return Err(failure(
                "invalid_detail",
                "detail must be summary, geometry or full",
            ));
        }
        let type_filter = req["type"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let layer_filter = req["layer"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        // Selection-set semantics: the request starts a fresh pick unless the
        // caller asks to keep what is already selected.
        if req["clear"].as_bool().unwrap_or(true) {
            self.tabs[i].scene.deselect_all();
        }
        let prompt = req["prompt"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                crate::t!("Select objects, then press Enter to finish (Esc cancels).")
                    .into_owned()
            });
        // Label the request on screen: the message list must say WHO is
        // waiting and for what, and the line stays pinned (non-fading) for
        // as long as the request is open — see `pending_pick_label`.
        let request_id = req["request_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let label = crate::tf!(
            "[user_select · {}] {}  ({})",
            if request_id.is_empty() { "client" } else { &request_id },
            prompt,
            crate::t!("Enter confirms, Esc cancels"),
        )
        .into_owned();
        self.command_line.push_info(&label);
        eprintln!(
            "[pick] user_select {}: start -- waiting for the person (Enter confirms, Esc cancels)",
            display_id(&request_id)
        );
        self.control.user_select = Some(UserSelectSession {
            request_id,
            document_id: self.tabs[i].id,
            type_filter,
            layer_filter,
            detail: detail.to_owned(),
            label,
        });
        Ok(Task::none())
    }

    /// Settle the pending request once the user answered. `confirmed` is the
    /// Enter branch (selection read + filter validation); Escape resolves
    /// immediately with `cancelled`. While the session lives the operation
    /// stays off the settle path (its `pending` counter is held at one), so
    /// pollers see `running` for as long as the person keeps working.
    pub(in crate::app) fn resolve_user_select(&mut self, confirmed: bool) {
        let Some(session) = self.control.user_select.take() else {
            return;
        };
        if !confirmed {
            self.command_line
                .push_info(crate::t!("*Cancel*").as_ref());
            self.command_line.push_info(
                crate::tf!(
                    "user_select {}: the request was answered with no objects.",
                    session.request_id
                )
                .as_ref(),
            );
            eprintln!(
                "[pick] user_select {}: the person pressed Esc -- cancelled",
                display_id(&session.request_id)
            );
            self.finish_interactive(
                "user_select",
                &session.request_id,
                true,
                json!({"cancelled": true, "count": 0, "handles": [], "entities": []}),
            );
            return;
        }
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == session.document_id);
        let Some(i) = index else {
            eprintln!(
                "[pick] user_select {}: document closed -- cancelled",
                display_id(&session.request_id)
            );
            self.finish_interactive(
                "user_select",
                &session.request_id,
                true,
                json!({"cancelled": true, "reason": "document_closed", "count": 0, "handles": [], "entities": []}),
            );
            return;
        };
        // The filter only gates the answer, not the user's gestures: anything
        // that does not match is deselected (and reported) at confirm time.
        let picked = self.tabs[i].scene.selected_handles_in_order();
        let mut handles: Vec<acadrust::Handle> = Vec::new();
        let mut ignored = 0usize;
        for handle in picked {
            let matches = self.tabs[i]
                .scene
                .document
                .get_entity(handle)
                .is_some_and(|entity| {
                    session
                        .type_filter
                        .as_deref()
                        .is_none_or(|filter| entity_type_matches(entity, filter))
                        && session
                            .layer_filter
                            .as_deref()
                            .is_none_or(|filter| entity.common().layer == filter)
                });
            if matches {
                handles.push(handle);
            } else {
                ignored += 1;
                self.tabs[i].scene.deselect_entity(handle);
            }
        }
        if ignored > 0 {
            self.command_line.push_info(
                crate::tf!(
                    "{} object(s) ignored: outside the requested type/layer filter.",
                    ignored
                )
                .as_ref(),
            );
        }
        let stored: Vec<String> = handles
            .iter()
            .map(|handle| format!("{:X}", handle.value()))
            .collect();
        let entities: Vec<Value> = handles
            .iter()
            .filter_map(|handle| {
                self.tabs[i]
                    .scene
                    .document
                    .get_entity(*handle)
                    .map(|entity| entity_json(entity, &session.detail))
            })
            .collect();
        let count = entities.len();
        self.command_line.push_info(
            crate::tf!(
                "user_select {}: handed {} object(s) to the client.",
                session.request_id,
                count
            )
            .as_ref(),
        );
        eprintln!(
            "[pick] user_select {}: the person confirmed -- {} object(s) handed over",
            display_id(&session.request_id),
            count
        );
        self.finish_interactive(
            "user_select",
            &session.request_id,
            false,
            json!({
                "cancelled": false,
                "count": count,
                "ignored": ignored,
                "handles": stored,
                "entities": entities,
            }),
        );
    }

    /// `getpoint` — ask the person at the screen to pick one point. The next
    /// left-click in the pinned document's viewport becomes the answer (the
    /// same snapped world point a command would receive); Escape cancels.
    /// The operation stays `running` until then, so callers may hold the
    /// HTTP request open for as long as the person takes.
    pub(super) fn control_getpoint(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            return Err(failure("no_document", "Create or open a drawing first"));
        }
        if self.tabs[i].active_cmd.is_some() || self.active_modal.is_some() {
            return Err(failure(
                "user_input_busy",
                "The user is already interacting with a command or dialog",
            ));
        }
        if self.control.get_point.is_some() || self.control.user_select.is_some() {
            return Err(failure(
                "interactive_pending",
                "Another interactive request is already waiting for the user",
            ));
        }
        if self.main_window.is_none() {
            return Err(failure(
                "gui_required",
                "getpoint needs the desktop GUI where a person can answer",
            ));
        }
        let prompt = req["prompt"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| crate::t!("Pick a point, then click (Esc cancels).").into_owned());
        let request_id = req["request_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let label = crate::tf!(
            "[getpoint · {}] {}  ({})",
            if request_id.is_empty() { "client" } else { &request_id },
            prompt,
            crate::t!("Click confirms, Esc cancels"),
        )
        .into_owned();
        self.command_line.push_info(&label);
        eprintln!(
            "[pick] getpoint {}: start -- waiting for the person (Click confirms, Esc cancels)",
            display_id(&request_id)
        );
        self.control.get_point = Some(UserPointSession {
            request_id,
            document_id: self.tabs[i].id,
            label,
        });
        Ok(Task::none())
    }

    /// Settle a `getpoint` request: `Some(point)` is the picked answer,
    /// `None` the Escape (or tab-close) cancellation.
    pub(in crate::app) fn resolve_get_point(&mut self, point: Option<[f64; 3]>) {
        let Some(session) = self.control.get_point.take() else {
            return;
        };
        // Drop the snap marker the pending pick was showing — no command is
        // running, so nothing else would clear it after the session ends.
        if let Some(i) = self.tabs.iter().position(|tab| tab.id == session.document_id) {
            self.tabs[i].snap_result = None;
        }
        let (cancelled, result) = match point {
            Some(point) => (false, json!({"point": point})),
            None => (true, json!({"cancelled": true})),
        };
        if cancelled {
            self.command_line.push_info(crate::t!("*Cancel*").as_ref());
        }
        self.command_line.push_info(
            crate::tf!(
                "getpoint {}: {}",
                session.request_id,
                match point {
                    Some(point) => format!("{point:?}"),
                    None => crate::t!("no point — the request was cancelled.").into_owned(),
                }
            )
            .as_ref(),
        );
        match point {
            Some(point) => eprintln!(
                "[pick] getpoint {}: the person picked {:?}",
                display_id(&session.request_id),
                point
            ),
            None => eprintln!(
                "[pick] getpoint {}: the person pressed Esc -- cancelled",
                display_id(&session.request_id)
            ),
        }
        self.finish_interactive("getpoint", &session.request_id, cancelled, result);
    }

    /// The pinned command-line label of a pending client pick (`user_select`
    /// or `getpoint`). The end-of-update driver feeds this to
    /// `CommandLine::set_step_prompt` so the request line stays on screen —
    /// pinned, non-fading — until the person answers, then it un-pins.
    pub(in crate::app) fn pending_pick_label(&self) -> Option<String> {
        self.control
            .user_select
            .as_ref()
            .map(|session| session.label.clone())
            .or_else(|| self.control.get_point.as_ref().map(|s| s.label.clone()))
    }

    /// Stamp an interactive answer into the pending operation and let it
    /// settle. `cancelled` flips the final status to "cancelled". The
    /// request_id + op check keeps a stale answer from writing into someone
    /// else's result slot.
    fn finish_interactive(
        &mut self,
        op: &str,
        request_id: &str,
        cancelled: bool,
        result: Value,
    ) {
        if let Some(pending) = self
            .control
            .pending
            .as_mut()
            .filter(|p| p.id == request_id && p.request["op"] == op)
        {
            pending.result = result;
            pending.cancelled = cancelled;
            pending.pending = 0;
        }
        self.control_settle();
    }

    /// `get_selection` — read-only snapshot of the active tab's selection
    /// (the read-back half of the `user_select` sampling flow). `handle`,
    /// `type`, `layer` and `bounds` come straight from the `query` output so
    /// clients share one parser; `text`/`value` expose the stripped and raw
    /// strings for TEXT/MTEXT, and `block`/`position` identify an INSERT's
    /// block definition and insertion point (null otherwise). Reads
    /// `scene.selection` and never mutates it.
    pub(in crate::app) fn control_get_selection(&self) -> Value {
        let scene = &self.tabs[self.active_tab].scene;
        let entities: Vec<Value> = scene
            .selected_handles_in_order()
            .iter()
            .filter_map(|handle| {
                scene.document.get_entity(*handle).map(|entity| {
                    let full = entity_json(entity, "full");
                    json!({
                        "handle": full["handle"],
                        "type": full["type"],
                        "layer": full["layer"],
                        "bounds": full["bounds"],
                        "text": full.get("text").cloned().unwrap_or(Value::Null),
                        "value": full.get("value").cloned().unwrap_or(Value::Null),
                        "block": full.get("block").cloned().unwrap_or(Value::Null),
                        "position": full.get("position").cloned().unwrap_or(Value::Null),
                    })
                })
            })
            .collect();
        json!({
            "ok": true,
            "status": "completed",
            "result": {"count": entities.len(), "entities": entities},
        })
    }
}

/// Stable diagnostic name for a request that arrived without an id, shared
/// by the on-screen label and the stderr `[pick]` lifecycle lines so the
/// client's own log can be cross-referenced line for line.
fn display_id(request_id: &str) -> &str {
    if request_id.is_empty() {
        "client"
    } else {
        request_id
    }
}
