//! Semantic control shared by the GUI, headless client and web adapter.
use super::{Message, OpenCADStudio};
use crate::command::StepInput;
use iced::Task;
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::OnceLock};
#[cfg(not(target_arch = "wasm32"))]
mod transport;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use transport::subscribe;

pub(super) fn session_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).expect("OS random source");
            bytes.iter().map(|v| format!("{v:02x}")).collect()
        }
        #[cfg(target_arch = "wasm32")]
        {
            format!("web-{}", js_sys::Date::now())
        }
    })
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub request: Value,
    pub reply: Reply,
}
#[derive(Debug, Clone)]
pub enum Reply {
    Native(std::sync::mpsc::Sender<Value>),
    Web(String),
}
impl Reply {
    pub(super) fn send(self, value: Value) {
        match self {
            Self::Native(sender) => {
                let _ = sender.send(value);
            }
            Self::Web(id) => web_store(id, value),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn web_store(_: String, _: Value) {}

#[cfg(target_arch = "wasm32")]
static WEB_QUEUE: std::sync::Mutex<VecDeque<(String, Value)>> =
    std::sync::Mutex::new(VecDeque::new());
#[cfg(target_arch = "wasm32")]
static WEB_RESULTS: std::sync::Mutex<VecDeque<(String, Value)>> =
    std::sync::Mutex::new(VecDeque::new());
#[cfg(target_arch = "wasm32")]
static WEB_TICKET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[cfg(target_arch = "wasm32")]
fn web_store(id: String, value: Value) {
    let mut queue = WEB_RESULTS.lock().unwrap();
    queue.push_back((id, value));
    while queue.len() > 64 {
        queue.pop_front();
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn ocs_control_submit(request: String) -> String {
    let serial = WEB_TICKET.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let id = format!("web-{}-{serial}", js_sys::Date::now());
    let value = serde_json::from_str(&request)
        .unwrap_or_else(|error| json!({"op":"invalid","parse_error":error.to_string()}));
    let mut queue = WEB_QUEUE.lock().unwrap();
    if queue.len() >= 64 {
        drop(queue);
        web_store(id.clone(), failure("busy", "Web control queue full"));
    } else {
        queue.push_back((id.clone(), value));
    }
    id
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn ocs_control_take(id: &str) -> Option<String> {
    let mut queue = WEB_RESULTS.lock().unwrap();
    let position = queue.iter().position(|(candidate, _)| candidate == id)?;
    Some(queue.remove(position).unwrap().1.to_string())
}

#[cfg(target_arch = "wasm32")]
pub(super) fn web_request() -> Option<Envelope> {
    let (id, request) = WEB_QUEUE.lock().unwrap().pop_front()?;
    Some(Envelope {
        request,
        reply: Reply::Web(id),
    })
}
#[derive(Default)]
pub(super) struct State {
    pending: Option<Operation>,
    completed: VecDeque<(String, Value, Value)>,
    owner: Option<(u64, String)>,
    pub enabled: bool,
    serial: u64,
    events: VecDeque<Value>,
    pub(super) routing: bool,
}
impl State {
    pub(super) fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}
struct Operation {
    id: String,
    request: Value,
    pending: usize,
    error_revision: u64,
    document_id: Option<u64>,
    origin_document: u64,
    geometry_revision: u64,
    result: Value,
}
fn failure(code: &str, error: impl ToString) -> Value {
    json!({"ok":false,"status":"failed","code":code,"error":error.to_string()})
}
fn string<'a>(req: &'a Value, key: &str) -> Result<&'a str, Value> {
    req[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| failure("invalid_request", format!("Missing {key}")))
}
fn handle(req: &Value) -> Result<acadrust::Handle, Value> {
    u64::from_str_radix(string(req, "handle")?.trim_start_matches("0x"), 16)
        .map(acadrust::Handle::new)
        .map_err(|_| failure("invalid_handle", "Expected hexadecimal handle"))
}
#[cfg(not(target_arch = "wasm32"))]
fn plugin_ids() -> Vec<String> {
    crate::plugin::external::with_manager(|m| m.ids())
}
#[cfg(target_arch = "wasm32")]
fn plugin_ids() -> Vec<String> {
    Vec::new()
}

fn point(req: &Value) -> Result<glam::DVec3, Value> {
    let values = req["point"]
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or_else(|| failure("invalid_point", "Expected three finite coordinates"))?;
    let mut p = [0.; 3];
    for (i, v) in values.iter().enumerate() {
        p[i] = v
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| failure("invalid_point", "Expected finite coordinates"))?;
    }
    Ok(glam::DVec3::from_array(p))
}

impl OpenCADStudio {
    pub(super) fn control_busy(&self) -> bool {
        self.control.pending.is_some()
    }

    pub(super) fn control_state(&self) -> Value {
        let tab = &self.tabs[self.active_tab];
        let command = tab.active_cmd.as_ref().map(|c| json!({
            "name":c.name(),"prompt":c.prompt(),"options":c.options().iter().map(|o|json!({"label":o.label,"keyword":o.keyword})).collect::<Vec<_>>(),
            "entity_pick":c.needs_entity_pick(),"structure_pick":c.needs_structure_point_pick(),"selection":c.is_selection_gathering(),"tangent_pick":c.needs_tangent_pick(),"free_text":c.is_free_text_step()
        }));
        json!({"ok":true,"protocol":1,"session_id":session_id(),"version":env!("OCS_APP_VERSION"),
            "mode":if self.main_window.is_some(){"gui"}else{"headless"},"enabled":self.control.enabled,
            "document_id":tab.id,"revision":tab.edit_revision,"geometry_revision":tab.scene.geometry_epoch,"camera_revision":tab.scene.camera_generation,
            "plugins":plugin_ids(),
            "documents":self.tabs.iter().map(|t|json!({"id":t.id,"title":t.tab_title,"path":t.current_path,"dirty":t.dirty,"revision":t.edit_revision,"start":t.is_start})).collect::<Vec<_>>(),
            "selection":tab.scene.selected_handles_in_order().iter().map(|h|format!("{:X}",h.value())).collect::<Vec<_>>(),
            "command":command,"modal":self.active_modal.as_ref().map(|m|format!("{m:?}")),
            "layout":tab.scene.current_layout,
            "ucs":tab.active_ucs.as_ref().map(|u|json!({"name":u.name,"origin":[u.origin.x,u.origin.y,u.origin.z],"x_axis":[u.x_axis.x,u.x_axis.y,u.x_axis.z],"y_axis":[u.y_axis.x,u.y_axis.y,u.y_axis.z],"elevation":u.elevation})),
            "cursor":{"world":tab.last_cursor_world.to_array(),"screen":[tab.last_cursor_screen.x,tab.last_cursor_screen.y]},
            "viewport_size":({let s=tab.scene.selection.borrow();[s.vp_size.0,s.vp_size.1]}),
            "camera":({let c=tab.scene.camera.borrow();json!({"target":c.target.to_array(),"rotation":[c.rotation.x,c.rotation.y,c.rotation.z,c.rotation.w],"distance":c.distance,"fov_y":c.fov_y,"projection":format!("{:?}",c.projection),"yaw":c.yaw,"pitch":c.pitch})}),
            "mtext_editor":self.mtext_editor.as_ref().map(|e|json!({"text":e.content.text(),"height":e.height,"style":e.style})),
            "text_editor":self.text_inline.is_some(),"event_cursor":self.control.serial,
            "operation":self.control.pending.as_ref().map(|p| &p.id),
            "capabilities":["commands","step_input","entity_pick","structure_pick","selection","properties","layers","history","documents","events","capture","measure"]
        })
    }

    pub(super) fn control_request(&mut self, req: Value) -> (Value, Task<Message>) {
        let op = req["op"].as_str().unwrap_or("");
        let query = matches!(
            op,
            "state"
                | "hello"
                | "operation"
                | "events"
                | "commands"
                | "properties"
                | "measure"
                | "query"
                | "entities"
                | "layers"
                | "header"
                | "history"
        );
        if !query && !self.control.enabled {
            return (
                failure("disabled", "Automation is stopped. Enable it in OCS."),
                Task::none(),
            );
        }
        if req["protocol"].as_u64().is_some_and(|v| v != 1) {
            return (
                failure("protocol_mismatch", "Protocol 1 required"),
                Task::none(),
            );
        }
        if req["session_id"]
            .as_str()
            .is_some_and(|v| v != session_id())
        {
            return (
                failure(
                    "session_changed",
                    "Reconnect; do not replay mutations in a new session",
                ),
                Task::none(),
            );
        }
        if op == "hello" || op == "state" {
            return (self.control_state(), Task::none());
        }
        if op == "operation" {
            let id = req["request_id"].as_str().unwrap_or("");
            let response = if let Some(p) = self.control.pending.as_ref().filter(|p| p.id == id) {
                json!({"ok":true,"status":"running","request_id":p.id})
            } else {
                self.control
                    .completed
                    .iter()
                    .find(|(i, _, _)| i == id)
                    .map(|(_, _, r)| r.clone())
                    .unwrap_or_else(|| {
                        failure(
                            "unknown_operation",
                            "Operation absent or expired; do not automatically replay",
                        )
                    })
            };
            return (response, Task::none());
        }
        if op == "events" {
            let cursor = req["after"].as_u64().unwrap_or(0);
            return (
                json!({"ok":true,"cursor":self.control.serial,"resync":self.control.events.front().is_some_and(|e|cursor+1<e["sequence"].as_u64().unwrap_or(0)),"events":self.control.events.iter().filter(|e|e["sequence"].as_u64().unwrap_or(0)>cursor).collect::<Vec<_>>()}),
                Task::none(),
            );
        }
        if op == "commands" {
            let mut names = crate::command::all_registered_command_names();
            let mut names: Vec<String> = names.drain(..).map(str::to_owned).collect();
            names.extend(self.command_line.dynamic_commands.iter().cloned());
            names.sort_unstable();
            names.dedup();
            return (
                json!({"ok":true,"commands":names,"actions":actions::NAMES}),
                Task::none(),
            );
        }
        let id = if query {
            String::new()
        } else {
            let id = match string(&req, "request_id") {
                Ok(v) if v.len() <= 128 => v.to_owned(),
                _ => {
                    return (
                        failure(
                            "request_id_required",
                            "Supply a unique request_id (maximum 128 bytes)",
                        ),
                        Task::none(),
                    )
                }
            };
            if let Some((_, old, result)) = self.control.completed.iter().find(|(i, _, _)| i == &id)
            {
                return (
                    if old == &req {
                        result.clone()
                    } else {
                        failure("request_id_reused", "Request payload changed")
                    },
                    Task::none(),
                );
            }
            if let Some(p) = &self.control.pending {
                return (
                    if p.id == id && p.request == req {
                        json!({"ok":true,"status":"running","request_id":id})
                    } else {
                        failure("busy", "Wait for the running operation")
                    },
                    Task::none(),
                );
            }
            id
        };
        if let Some(id) = req["document_id"].as_u64() {
            if !self.tabs.iter().any(|t| t.id == id) {
                return (
                    failure("document_closed", "Document no longer exists"),
                    Task::none(),
                );
            }
            if self.tabs[self.active_tab].id != id && op != "activate" {
                return (
                    failure(
                        "document_not_active",
                        "Activate the requested document first",
                    ),
                    Task::none(),
                );
            }
        } else if !query && !matches!(op, "new" | "open" | "stop") {
            return (
                failure("document_required", "Read state and supply document_id"),
                Task::none(),
            );
        }
        let tab = &self.tabs[self.active_tab];
        if req["revision"]
            .as_u64()
            .is_some_and(|v| v != tab.edit_revision)
            || req["geometry_revision"]
                .as_u64()
                .is_some_and(|v| v != tab.scene.geometry_epoch)
            || req["camera_revision"]
                .as_u64()
                .is_some_and(|v| v != tab.scene.camera_generation)
        {
            return (
                json!({"ok":false,"status":"failed","code":"stale_state","error":"Refresh state before editing","state":self.control_state()}),
                Task::none(),
            );
        }
        if query {
            let response = match op {
                "properties" => self.control_properties(),
                "measure" => self.control_measure(&req),
                "history" => {
                    json!({"ok":true,"entries":self.command_line.history.iter().map(|e|json!({"kind":format!("{:?}",e.kind),"text":e.text})).collect::<Vec<_>>()})
                }
                _ => self.automation_op_inner(&req.to_string()),
            };
            return (response, Task::none());
        }
        let client = req["client_id"].as_str().unwrap_or("default").to_owned();
        if tab.active_cmd.is_some()
            && (self.control.owner.as_ref() != Some(&(tab.id, client.clone()))
                || matches!(op, "run" | "start" | "new" | "open" | "activate"))
            && op != "cancel"
        {
            return (
                failure("command_busy", "An interactive command is already active"),
                Task::none(),
            );
        }
        if matches!(op, "run" | "start" | "input") && tab.is_start {
            return (
                failure("no_document", "Create or open a drawing first"),
                Task::none(),
            );
        }
        if let Some(expected) = req["selection"].as_array() {
            let actual: Vec<Value> = tab
                .scene
                .selected_handles_in_order()
                .iter()
                .map(|h| json!(format!("{:X}", h.value())))
                .collect();
            if &actual != expected {
                return (
                    failure(
                        "selection_changed",
                        "Read state and confirm the current selection",
                    ),
                    Task::none(),
                );
            }
        }
        let doc = if matches!(op, "new" | "open" | "activate") {
            None
        } else {
            Some(tab.id)
        };
        self.control.pending = Some(Operation {
            id: id.clone(),
            request: req.clone(),
            pending: 1,
            error_revision: self.command_line.error_revision,
            document_id: doc,
            origin_document: tab.id,
            geometry_revision: tab.scene.geometry_epoch,
            result: json!({}),
        });
        self.control.routing = true;
        let action = self.control_action(&req);
        self.control.routing = false;
        let task = match action {
            Ok(t) => t,
            Err(error) => {
                self.control.pending.take();
                return (error, Task::none());
            }
        };
        if self.tabs[self.active_tab].active_cmd.is_some() {
            self.control.owner = Some((self.tabs[self.active_tab].id, client));
        }
        if matches!(op, "new" | "activate")
            && self
                .control
                .pending
                .as_ref()
                .is_some_and(|pending| pending.origin_document != self.tabs[self.active_tab].id)
        {
            if let Some(pending) = self.control.pending.as_mut() {
                pending.document_id = Some(self.tabs[self.active_tab].id);
            }
        }
        self.finish_all_pending_history();
        let task = self.control_track(task);
        if let Some(p) = self.control.pending.as_mut() {
            p.pending -= 1;
        }
        self.control_settle();
        let response = self
            .control
            .completed
            .iter()
            .find(|(i, _, _)| i == &id)
            .map(|(_, _, r)| r.clone())
            .unwrap_or_else(|| json!({"ok":true,"status":"accepted","request_id":id}));
        (response, task)
    }

    fn control_action(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        Ok(match req["op"].as_str().unwrap_or("") {
            "new" => self.update(Message::TabNew),
            "open" => self.update(Message::OpenExternal(std::path::PathBuf::from(string(
                req, "path",
            )?))),
            "activate" => {
                let i = self
                    .tabs
                    .iter()
                    .position(|t| Some(t.id) == req["document_id"].as_u64())
                    .ok_or_else(|| failure("document_closed", "Document absent"))?;
                self.update(Message::TabSwitch(i))
            }
            "run" => self.run_command_line(string(req, "cmd")?),
            "start" => self.dispatch_command(string(req, "cmd")?),
            "input" => {
                if self.tabs[i].active_cmd.is_none() {
                    return Err(failure("no_command", "No command waiting for input"));
                }
                match string(req, "kind")? {
                    "text" => self
                        .feed_command(StepInput::Text(req["text"].as_str().unwrap_or("").into())),
                    "token" => self.feed_active_cmd(string(req, "text")?),
                    "point" => {
                        let mut p = point(req)?;
                        match req["space"].as_str().unwrap_or("wcs") {
                            "wcs" => {}
                            "ucs" => {
                                if let Some(u) = &self.tabs[i].active_ucs {
                                    p = super::helpers::ucs_to_wcs(p, u);
                                }
                            }
                            "relative" => {
                                let base = self.last_point.ok_or_else(|| {
                                    failure(
                                        "no_base_point",
                                        "Relative input needs a previous point",
                                    )
                                })?;
                                if let Some(u) = &self.tabs[i].active_ucs {
                                    p = super::helpers::ucs_rotate_vec(p, u);
                                }
                                p += base;
                            }
                            _ => return Err(failure("invalid_space", "Use wcs, ucs or relative")),
                        };
                        self.feed_command(StepInput::Point(p))
                    }
                    "entity" | "structure" => {
                        let h = handle(req)?;
                        if self.tabs[i].scene.document.get_entity(h).is_none() {
                            return Err(failure("entity_absent", "Unknown entity"));
                        }
                        let p = point(req)?;
                        self.feed_command(if req["kind"] == "structure" {
                            StepInput::StructurePick(h, p)
                        } else {
                            StepInput::EntityPick(h, p)
                        })
                    }
                    "selection" => {
                        let hs = self.tabs[i].scene.selected_handles_in_order();
                        self.feed_command(StepInput::SelectionComplete(hs))
                    }
                    "enter" => self.feed_command(StepInput::Enter),
                    _ => return Err(failure("invalid_input", "Unknown input kind")),
                }
            }
            "cancel" => self.update(Message::CommandEscape),
            "undo" => self.update(Message::Undo),
            "redo" => self.update(Message::Redo),
            "select" => {
                if let Some(hs) = req["handles"].as_array() {
                    for v in hs {
                        let h = u64::from_str_radix(
                            v.as_str().unwrap_or("").trim_start_matches("0x"),
                            16,
                        )
                        .map(acadrust::Handle::new)
                        .map_err(|_| failure("invalid_handle", "Expected hexadecimal handle"))?;
                        if self.tabs[i].scene.document.get_entity(h).is_none() {
                            return Err(failure(
                                "entity_absent",
                                format!("Entity {} does not exist", v),
                            ));
                        }
                    }
                }
                let result = self.automation_op_inner(&req.to_string());
                if result["ok"] != true {
                    return Err(result);
                }
                self.refresh_properties();
                Task::none()
            }
            "property" => self.control_set_property(req)?,
            "action" => self.control_ui_action(req)?,
            #[cfg(not(target_arch = "wasm32"))]
            "save" => {
                let path = req["path"]
                    .as_str()
                    .map(std::path::PathBuf::from)
                    .or_else(|| self.tabs[i].current_path.clone())
                    .ok_or_else(|| failure("path_required", "Supply a save path"))?;
                if self.main_window.is_none() {
                    self.save_tab_synchronously_protected(i, path, true)
                        .map_err(|e| failure("save_failed", e))?;
                    Task::none()
                } else {
                    self.prepare_native_save(i);
                    self.queue_native_save(
                        i,
                        path,
                        acadrust::DxfVersion::AC1032,
                        super::SavePurpose::SaveAs,
                        super::SaveContinuation::None,
                        true,
                        true,
                    )
                }
            }
            "capture" => {
                let window = self
                    .main_window
                    .ok_or_else(|| failure("gui_required", "Capture requires a GUI window"))?;
                let path = string(req, "path")?.to_owned();
                iced::window::screenshot(window)
                    .map(move |s| Message::ControlScreenshot(path.clone(), Some(s)))
            }
            "stop" => {
                self.control.enabled = false;
                Task::none()
            }
            _ => return Err(failure("unknown_operation", "Unknown operation")),
        })
    }

    pub(super) fn control_track(&mut self, task: Task<Message>) -> Task<Message> {
        if task.units() == 0 {
            return task;
        }
        let Some(p) = self.control.pending.as_mut() else {
            return task;
        };
        p.pending += 1;
        let id = p.id.clone();
        let end = id.clone();
        task.map(move |m| Message::ControlStep(id.clone(), Box::new(m)))
            .chain(Task::done(Message::ControlTaskDone(end)))
    }
    pub(super) fn control_step(&mut self, id: String, msg: Message) -> Task<Message> {
        if !self.control.pending.as_ref().is_some_and(|p| p.id == id) {
            return Task::none();
        }
        let previous = self.active_tab;
        let target = self.control.pending.as_ref().and_then(|p| p.document_id);
        if let Some(doc) = target {
            let Some(i) = self.tabs.iter().position(|t| t.id == doc) else {
                self.command_line
                    .push_error("Automation target document closed");
                return Task::none();
            };
            self.active_tab = i;
        }
        self.control.routing = true;
        let task = self.update(msg);
        self.control.routing = false;
        if let Some(pending) = self.control.pending.as_mut() {
            if pending.document_id.is_none()
                && matches!(
                    pending.request["op"].as_str(),
                    Some("new" | "open" | "activate")
                )
                && pending.origin_document != self.tabs[self.active_tab].id
            {
                pending.document_id = Some(self.tabs[self.active_tab].id);
            }
        }
        if target.is_some() && previous < self.tabs.len() {
            self.active_tab = previous;
        }
        self.control_track(task)
    }
    pub(super) fn control_task_done(&mut self, id: &str) {
        if let Some(p) = self.control.pending.as_mut().filter(|p| p.id == id) {
            p.pending = p.pending.saturating_sub(1);
        }
        self.control_settle();
    }
    pub(super) fn control_observe_user_message(&mut self, message: &Message) {
        if self.control.routing || self.control.owner.is_none() {
            return;
        }
        if matches!(
            message,
            Message::CommandSubmit
                | Message::CommandFinalize
                | Message::CommandOptionPick(_)
                | Message::CommandEscape
                | Message::ViewportLeftPress
                | Message::ViewportLeftRelease
                | Message::PanePress(_)
                | Message::PaneRelease(_)
                | Message::ShortcutPressed(_)
                | Message::MTextOk
                | Message::MTextCancel
                | Message::TextInlineOk
        ) {
            self.control.owner = None;
        }
    }
    pub(super) fn control_settle(&mut self) {
        if self.control.pending.as_ref().is_none_or(|p| p.pending != 0) || self.opening.is_some() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !self.active_save_jobs.is_empty() || self.pending_native_thumbnail_save.is_some() {
            return;
        }
        let p = self.control.pending.take().unwrap();
        let previous = self.active_tab;
        if let Some(index) = p
            .document_id
            .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
        {
            self.active_tab = index;
        }
        let failed = self.command_line.error_revision != p.error_revision;
        let waiting = self.tabs[self.active_tab].active_cmd.is_some()
            || self.active_modal.is_some()
            || self.mtext_editor.is_some()
            || self.text_inline.is_some();
        if !waiting {
            self.control.owner = None;
        }
        let status = if failed {
            "failed"
        } else if p.request["op"] == "cancel" {
            "cancelled"
        } else if waiting {
            "waiting_input"
        } else {
            "completed"
        };
        let changes = self.tabs[self.active_tab].scene.replay_since(p.geometry_revision)
            .map(|values| values.into_iter().take(1000).map(|(handle,kind)|json!({"handle":format!("{:X}",handle.value()),"kind":format!("{kind:?}")})).collect::<Vec<_>>());
        let response = json!({"ok":!failed,"status":status,"request_id":p.id,"error":if failed{self.command_line.last_error.clone()}else{None},"result":p.result,"changes":changes,"state":self.control_state()});
        self.control.serial += 1;
        self.control.events.push_back(json!({"sequence":self.control.serial,"request_id":p.id,"status":status,"document_id":self.tabs[self.active_tab].id,"revision":self.tabs[self.active_tab].edit_revision}));
        while self.control.events.len() > 128 {
            self.control.events.pop_front();
        }
        self.control
            .completed
            .push_back((p.id, p.request, response));
        while self.control.completed.len() > 128 {
            self.control.completed.pop_front();
        }
        if previous < self.tabs.len() {
            self.active_tab = previous;
        }
    }
    pub(super) fn control_measure(&self, req: &Value) -> Value {
        let tab = &self.tabs[self.active_tab];
        let requested: Vec<acadrust::Handle> = req["handles"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .map(acadrust::Handle::new)
                    .collect()
            })
            .unwrap_or_else(|| tab.scene.selected_handles_in_order());
        let mut out = Vec::new();
        for handle in requested {
            let Some(entity) = tab.scene.document.get_entity(handle) else {
                continue;
            };
            let bb = entity.as_entity().bounding_box();
            let metrics = tab
                .scene
                .meshes
                .get(&handle)
                .or_else(|| tab.scene.block_meshes.get(&handle))
                .cloned()
                .or_else(|| {
                    let h = &tab.scene.document.header;
                    crate::entities::solid3d::tessellate_volume(
                        entity,
                        [1.; 4],
                        h.facet_resolution,
                        crate::entities::solid3d::display_deflection(h, h.facet_resolution),
                        h.isolines.max(0) as usize,
                    )
                });
            out.push(json!({"handle":format!("{:X}",handle.value()),"type":crate::entities::names::ui_name(entity),"bounds":{"min":[bb.min.x,bb.min.y,bb.min.z],"max":[bb.max.x,bb.max.y,bb.max.z]},"mesh":metrics.map(|m|json!({"vertices":m.metrics.vertices,"triangles":m.metrics.triangles,"surface_area":m.metrics.surface_area,"volume":m.metrics.volume,"centroid":m.metrics.centroid}))}));
        }
        json!({"ok":true,"document_id":tab.id,"geometry_revision":tab.scene.geometry_epoch,"measurements":out})
    }

    pub(super) fn control_screenshot(
        &mut self,
        path: String,
        screenshot: Option<iced::window::Screenshot>,
    ) {
        let result = (|| -> Result<Value, String> {
            let s = screenshot.ok_or("Renderer did not return an image")?;
            image::save_buffer_with_format(
                &path,
                &s.rgba,
                s.size.width,
                s.size.height,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )
            .map_err(|e| e.to_string())?;
            Ok(
                json!({"path":path,"width":s.size.width,"height":s.size.height,"scale_factor":s.scale_factor,"document_id":self.tabs[self.active_tab].id,"revision":self.tabs[self.active_tab].edit_revision,"camera_revision":self.tabs[self.active_tab].scene.camera_generation}),
            )
        })();
        match result {
            Ok(v) => {
                if let Some(p) = self.control.pending.as_mut() {
                    p.result = v;
                }
            }
            Err(e) => self.command_line.push_error(&e),
        }
    }
}
mod actions;

#[cfg(test)]
mod tests {
    use super::*;
    fn request(app: &mut OpenCADStudio, mut req: Value) -> Value {
        let state = app.control_state();
        req["protocol"] = json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        req["client_id"] = json!("test");
        if req.get("request_id").is_none() {
            req["request_id"] = json!(format!("req-{}", app.control.serial));
        }
        let (r, t) = app.control_request(req.clone());
        app.drive_headless_task(t).unwrap();
        if matches!(r["status"].as_str(), Some("accepted" | "running")) {
            app.control_request(json!({"op":"operation","request_id":req["request_id"]}))
                .0
        } else {
            r
        }
    }
    #[test]
    fn control_stepwise_drawing_undo_and_properties() {
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(
            request(&mut app, json!({"op":"new"}))["status"],
            "completed"
        );
        assert_eq!(
            request(&mut app, json!({"op":"start","cmd":"LINE"}))["status"],
            "waiting_input"
        );
        assert_eq!(
            request(
                &mut app,
                json!({"op":"input","kind":"point","point":[0.,0.,0.]})
            )["ok"],
            true
        );
        assert_eq!(
            request(
                &mut app,
                json!({"op":"input","kind":"point","point":[10.,0.,0.]})
            )["ok"],
            true
        );
        assert_eq!(
            request(&mut app, json!({"op":"input","kind":"enter"}))["status"],
            "completed"
        );
        assert_eq!(app.automation_op(r#"{"op":"entities"}"#)["total"], 1);
        request(&mut app, json!({"op":"select","type":"LINE"}));
        assert!(!app.control_properties()["sections"]
            .as_array()
            .unwrap()
            .is_empty());
        let changed = request(&mut app, json!({"op":"property","field":"color","value":1}));
        assert_eq!(changed["ok"], true, "{changed}");
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .entities()
                .next()
                .unwrap()
                .common()
                .color
                .index(),
            Some(1)
        );
        assert_eq!(request(&mut app, json!({"op":"undo"}))["ok"], true);
        assert_eq!(app.automation_op(r#"{"op":"entities"}"#)["total"], 1);
        request(&mut app, json!({"op":"undo"}));
        assert_eq!(app.automation_op(r#"{"op":"entities"}"#)["total"], 0);
    }
    #[test]
    fn control_retry_is_idempotent_and_stale_state_rejected() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let state = app.control_state();
        let req = json!({"op":"run","cmd":"LINE 0,0 10,0","request_id":"line","document_id":state["document_id"],"revision":state["revision"]});
        let (first, t) = app.control_request(req.clone());
        app.drive_headless_task(t).unwrap();
        let repeat = app.control_request(req.clone()).0;
        assert!(repeat["ok"] == true, "{first} {repeat}");
        assert_eq!(app.automation_op(r#"{"op":"entities"}"#)["total"], 1);
        let mut stale = req;
        stale["request_id"] = json!("stale");
        assert_eq!(app.control_request(stale).0["code"], "stale_state");
    }
    #[test]
    fn control_errors_and_missing_entities_do_not_report_success() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let result = request(&mut app, json!({"op":"run","cmd":"NONEXISTENT_COMMAND"}));
        assert_eq!(result["status"], "failed", "{result}");
        assert_eq!(
            request(&mut app, json!({"op":"select","handles":["FFFFFFFF"]}))["code"],
            "entity_absent"
        );
        assert_eq!(
            app.automation_op(r#"{"op":"run","cmd":"NONEXISTENT_COMMAND"}"#)["ok"],
            false
        );
    }
    #[test]
    fn direct_user_input_takes_command_ownership() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"start","cmd":"LINE"}));
        let _ = app.update(Message::ViewportLeftPress);
        let result = request(
            &mut app,
            json!({"op":"input","kind":"point","point":[10.,0.,0.]}),
        );
        assert_eq!(result["code"], "command_busy");
    }
    #[test]
    fn control_preserves_other_documents_and_waits_for_async_open() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"CIRCLE 0,0 3"}));
        let id = app.control_state()["document_id"].as_u64().unwrap();
        let path = std::env::temp_dir().join(format!("ocs-control-{}.dxf", session_id()));
        assert_eq!(
            request(&mut app, json!({"op":"save","path":path}))["ok"],
            true
        );
        request(&mut app, json!({"op":"new"}));
        let opened = request(&mut app, json!({"op":"open","path":path}));
        assert_eq!(opened["status"], "completed", "{opened}");
        assert!(app.tabs.iter().any(|t| t.id == id));
        assert_eq!(
            app.tabs[app.active_tab].scene.document.entities().count(),
            1
        );
        let _ = std::fs::remove_file(path);
    }
}
