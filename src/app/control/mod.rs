//! Semantic control shared by the GUI, headless client and web adapter.
use super::{Message, OpenCADStudio};
use crate::command::{InputKind, StepInput};
use iced::Task;
use serde_json::{Value, json};
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
    /// Session-scoped named handle sets for selection_set_save/load.
    selection_sets: std::collections::BTreeMap<String, Vec<acadrust::Handle>>,
    /// Live `user_select` request — the client asked the person at the screen
    /// to pick entities; Some until that person answers with Enter/Escape.
    pub(super) user_select: Option<UserSelectSession>,
    /// Live `getpoint` request — the client asked for one picked point; Some
    /// until the person clicks (answer) or presses Escape (cancel).
    pub(super) get_point: Option<UserPointSession>,
}

/// The pending half of an interactive `user_select` operation (`interactive`
/// module owns the flow): which request it answers, which document's
/// selection to read, and the filter the answer must satisfy.
pub(super) struct UserSelectSession {
    pub(super) request_id: String,
    pub(super) document_id: u64,
    pub(super) type_filter: Option<String>,
    pub(super) layer_filter: Option<String>,
    pub(super) detail: String,
    /// The pinned command-line line shown while the person picks — the
    /// prompt plus the request identity, so the screen keeps saying who is
    /// waiting for what until they answer.
    pub(super) label: String,
}

/// The pending half of a `getpoint` operation: which request it answers and
/// which document's viewport the pick must land in.
pub(super) struct UserPointSession {
    pub(super) request_id: String,
    pub(super) document_id: u64,
    /// Same pinned line as `UserSelectSession::label`.
    pub(super) label: String,
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
    /// Set when an interactive request (`getpoint`/`user_select`) was answered
    /// with a cancellation, so the final status reports "cancelled".
    cancelled: bool,
}
fn failure(code: &str, error: impl ToString) -> Value {
    json!({"ok":false,"status":"failed","code":code,"error":error.to_string()})
}
/// `{"op":"open","name":"plan.dwg","data_base64":"…"}`: the web build has no
/// filesystem path to open, so the caller sends the file itself. `name` is
/// reduced to its file name; it labels the tab and the recent-files entry.
fn open_bytes_request(req: &Value) -> Result<(String, Vec<u8>), Value> {
    use base64::Engine as _;
    let name = string(req, "name")?;
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    if name.is_empty() {
        return Err(failure("invalid_request", "Missing name"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req["data_base64"].as_str().unwrap_or_default())
        .map_err(|e| failure("invalid_request", format!("data_base64: {e}")))?;
    if bytes.is_empty() {
        return Err(failure("invalid_request", "data_base64 is empty"));
    }
    Ok((name.to_string(), bytes))
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

fn command_examples(name: &str) -> &'static [&'static str] {
    match name {
        "LINE" => &["LINE 0,0 10,10"],
        "CIRCLE" => &["CIRCLE 5,5 3"],
        "PLINE" => &["PLINE 0,0 10,0 10,10 C"],
        "ZOOM" => &["ZOOM EXTENTS"],
        "MOVE" => &["MOVE 0,0 100,0"],
        "ROTATE" => &["ROTATE 0,0 90"],
        "UCS" => &["UCS Z 90"],
        "PDMODE" => &["PDMODE 3"],
        "LTSCALE" => &["LTSCALE 2.5"],
        _ => &[],
    }
}

fn command_category(name: &str) -> &'static str {
    if [
        "LINE", "CIRCLE", "PLINE", "ARC", "ELLIPSE", "POINT", "SPLINE", "RECT", "RECTANG",
        "POLYGON", "HATCH", "RAY", "XLINE", "3DPOLY", "MLINE", "TEXT", "MTEXT", "TABLE",
    ]
    .contains(&name)
    {
        "create"
    } else if [
        "MOVE",
        "COPY",
        "ROTATE",
        "SCALE",
        "MIRROR",
        "ERASE",
        "TRIM",
        "EXTEND",
        "OFFSET",
        "FILLET",
        "CHAMFER",
        "BREAK",
        "JOIN",
        "EXPLODE",
        "STRETCH",
        "ARRAY",
        "PEDIT",
        "SPLINEDIT",
        "REVERSE",
        "ALIGN",
    ]
    .contains(&name)
    {
        "modify"
    } else if ["DIST", "ID", "AREA", "MEASURE", "MEASUREGEOM"].contains(&name) {
        "inspect"
    } else if ["ZOOM", "PAN", "UCS", "PLAN", "VPORTS", "3DORBIT"].contains(&name) {
        "view"
    } else if name.starts_with("DIM") || ["LEADER", "MLEADER", "TOLERANCE"].contains(&name) {
        "annotate"
    } else if [
        "GEOMCONSTRAINT",
        "GCCOINCIDENT",
        "GCCOLLINEAR",
        "GCPERPENDICULAR",
        "QCONSTRAINT",
        "PCONSTRAINT",
        "GCHORIZONTAL",
        "VCONSTRAINT",
        "ECONSTRAINT",
        "GCEQUAL",
        "TCONSTRAINT",
        "GCCONCENTRIC",
        "NRCONSTRAINT",
        "LCONSTRAINT",
        "DELCONSTRAINT",
    ]
    .contains(&name)
    {
        "parametric"
    } else if [
        "BOX",
        "CYLINDER",
        "CONE",
        "SPHERE",
        "TORUS",
        "WEDGE",
        "EXTRUDE",
        "REVOLVE",
        "SWEEP",
        "LOFT",
        "UNION",
        "SUBTRACT",
        "INTERSECT",
        "PRESSPULL",
    ]
    .contains(&name)
    {
        "model"
    } else {
        "other"
    }
}

fn command_selection_policy(name: &str) -> &'static str {
    if [
        "ARRAY",
        "ARRAYPATH",
        "ARRAYPOLAR",
        "ARRAYRECT",
        "BLOCK",
        "COPY",
        "COPYCLIP",
        "CUTCLIP",
        "ERASE",
        "EXPLODE",
        "GROUP",
        "LAYFRZ",
        "LAYLCK",
        "LAYMCUR",
        "LAYOFF",
        "LAYULK",
        "MIRROR",
        "MOVE",
        "ROTATE",
        "SCALE",
        "STRETCH",
    ]
    .contains(&name)
    {
        "current_selection_or_prompt"
    } else {
        "command_defined"
    }
}

pub(super) fn active_command_metadata(command: &dyn crate::command::CadCommand) -> Value {
    let options = command.options();
    let mut accepts = Vec::new();
    if command.is_selection_gathering() {
        accepts.push("selection");
    }
    if command.needs_structure_point_pick() {
        accepts.push("structure");
    }
    if command.needs_entity_pick() {
        accepts.push("entity");
    }
    if accepts.is_empty() && !command.needs_tangent_pick() {
        let kind = match command.input_kind() {
            InputKind::Point => "point",
            InputKind::SingleToken => "token",
            InputKind::FreeText => "text",
        };
        // A point step that also takes keyword letters (MOVE/COPY's base
        // point with [Displacement]) reports SingleToken so the letters reach
        // the command line, but it still takes a point.
        if command.point_step_accepts_keywords() && kind != "point" {
            accepts.push("point");
        }
        accepts.push(kind);
    }
    if options.iter().any(|option| !option.keyword.is_empty()) && !accepts.contains(&"token") {
        accepts.push("token");
    }
    accepts.push("enter");

    let input_example = if accepts.contains(&"point") {
        json!({"op":"input","kind":"point","point":[0,0,0],"space":"wcs"})
    } else if accepts.contains(&"entity") {
        json!({"op":"input","kind":"entity","handle":"HANDLE","point":[0,0,0]})
    } else if accepts.contains(&"structure") {
        json!({"op":"input","kind":"structure","handle":"HANDLE","point":[0,0,0]})
    } else if accepts.contains(&"selection") {
        json!({"op":"input","kind":"selection"})
    } else if accepts.contains(&"text") {
        json!({"op":"input","kind":"text","text":"value"})
    } else if accepts.contains(&"token") {
        let token = options
            .iter()
            .find(|option| !option.keyword.is_empty())
            .map_or("VALUE", |option| option.keyword.as_str());
        json!({"op":"input","kind":"token","text":token})
    } else {
        json!({"op":"input","kind":"enter"})
    };

    json!({
        "name":command.name(),
        "prompt":command.prompt(),
        "accepts":accepts,
        "input_example":input_example,
        "options":options.iter().map(|option| json!({
            "label":option.label,
            "keyword":option.keyword,
            "kind":if option.keyword.is_empty(){"enter"}else{"token"}
        })).collect::<Vec<_>>(),
        "entity_pick":command.needs_entity_pick(),
        "structure_pick":command.needs_structure_point_pick(),
        "selection":command.is_selection_gathering(),
        "tangent_pick":command.needs_tangent_pick(),
        "free_text":command.is_free_text_step()
    })
}

impl OpenCADStudio {
    pub(super) fn control_busy(&self) -> bool {
        self.control.pending.is_some()
    }

    pub(super) fn control_state(&self) -> Value {
        let tab = &self.tabs[self.active_tab];
        let command = tab.active_cmd.as_deref().map(active_command_metadata);
        json!({"ok":true,"protocol":1,"session_id":session_id(),"version":env!("OCS_APP_VERSION"),
            "mode":if self.main_window.is_some(){"gui"}else{"headless"},"enabled":self.control.enabled,
            "document_id":tab.id,"revision":tab.edit_revision,"geometry_revision":tab.scene.geometry_epoch,"camera_revision":tab.scene.camera_generation,
            "hand_seed":format!("{:X}",tab.scene.document.header.handle_seed.max(tab.scene.document.next_handle())),
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
            "capabilities":["commands","command_manifest","step_input","batch","compact_results","entity_pick","structure_pick","selection","user_select","properties","records","record_schemas","record_filters","atomic_record_updates","layers","history","documents","events","capture","viewport_capture","measure","spatial_query"]
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
                | "records"
                | "record_schema"
                | "capabilities"
                | "entities"
                | "layers"
                | "header"
                | "history"
                | "xdata_get"
                | "get_selection"
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
            if let Some(requested) = req["name"].as_str() {
                let Some(name) = names
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(requested))
                else {
                    return (
                        failure(
                            "unknown_command",
                            format!(
                                "Unknown command {requested}; call commands without name to list commands"
                            ),
                        ),
                        Task::none(),
                    );
                };
                return (
                    json!({
                        "ok":true,
                        "command":{
                            "name":name,
                            "category":command_category(name),
                            "preconditions":{
                                "document":true,
                                "selection":command_selection_policy(name)
                            },
                            "batch":{
                                "op":"run",
                                "syntax":"Command name followed by prompt answers separated by spaces. Points use x,y or x,y,z; options use their token.",
                                "examples":command_examples(name)
                            },
                            "interactive":{
                                "op":"start",
                                "request":{"op":"start","cmd":name},
                                "next":"Follow state.command.accepts, options and input_example after every step."
                            },
                            "completion":"completed is final; waiting_input requires another input step; failed includes the reason."
                        }
                    }),
                    Task::none(),
                );
            }
            if let Some(search) = req["search"].as_str().filter(|search| !search.is_empty()) {
                let search = search.to_ascii_uppercase();
                names.retain(|name| name.contains(&search));
            }
            let count = names.len();
            let offset = req["offset"].as_u64().unwrap_or(0) as usize;
            let limit = req["limit"].as_u64().unwrap_or(200).min(1000) as usize;
            let commands: Vec<String> = names.into_iter().skip(offset).take(limit).collect();
            return (
                json!({
                    "ok":true,"count":count,"returned":commands.len(),
                    "next_offset":(offset + commands.len() < count).then_some(offset + commands.len()),
                    "commands":commands,"actions":actions::NAMES,
                    "detail_parameters":{"name":"LINE"},"search_parameter":{"search":"LINE"},
                    "guidance":"Request one command by name for batch examples and interactive input guidance."
                }),
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
                    );
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
                if p.id == id && p.request == req {
                    return (
                        json!({"ok":true,"status":"running","request_id":id}),
                        Task::none(),
                    );
                }
                // An interactive request (`user_select`/`getpoint`) outlives
                // normal request handling — it only ends when the person
                // answers. Let a client retract it with `cancel` instead of
                // waiting for their Escape.
                let pending_op = p.request["op"].as_str().unwrap_or("").to_owned();
                let pending_id = p.id.clone();
                if op == "cancel" && matches!(pending_op.as_str(), "user_select" | "getpoint") {
                    if pending_op == "getpoint" {
                        self.resolve_get_point(None);
                    } else {
                        self.resolve_user_select(false);
                    }
                    let response = self
                        .control
                        .completed
                        .iter()
                        .find(|(i, _, _)| *i == pending_id)
                        .map(|(_, _, r)| r.clone());
                    return (
                        response
                            .unwrap_or_else(|| failure("busy", "Wait for the running operation")),
                        Task::none(),
                    );
                }
                return (
                    failure("busy", "Wait for the running operation"),
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
            if self.tabs[self.active_tab].id != id
                && !matches!(op, "activate" | "close" | "entities_copy_to")
            {
                return (
                    failure(
                        "document_not_active",
                        "Activate the requested document first",
                    ),
                    Task::none(),
                );
            }
            // Interactive requests operate on the active tab by definition, so
            // a client may omit document_id (the GUI-hosted REST channel does).
            } else if !query
                && !matches!(
                    op,
                    "new" | "open" | "stop" | "user_select" | "getpoint"
                )
            {
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
                "xdata_get" => self.xdata_read(&req),
                "get_selection" => self.control_get_selection(),
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
            cancelled: false,
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
            // Interactive requests hold their slot open on purpose: the
            // counter stays at one so settle keeps returning `running` until
            // the person at the screen answers and the resolver zeroes it.
            if !matches!(p.request["op"].as_str(), Some("user_select" | "getpoint")) {
                p.pending -= 1;
            }
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
            "new" => {
                if let Some(template) = req["template"].as_str() {
                    match self.apply_template(template) {
                        Ok(purged) => {
                            self.set_control_result(json!({
                                "template": template,
                                "purged": purged,
                                "total": self.tabs[i].scene.document.entities().count(),
                            }));
                            Task::none()
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    self.update(Message::TabNew)
                }
            }
            "open" if req.get("data_base64").is_some() => {
                let (name, bytes) = open_bytes_request(req)?;
                #[cfg(target_arch = "wasm32")]
                {
                    if self.opening.is_some() {
                        return Err(failure("busy", "Another drawing is still opening"));
                    }
                    self.open_web_bytes(name, bytes)
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let _ = (name, bytes);
                    return Err(failure(
                        "invalid_request",
                        "data_base64 is for the web build; supply path",
                    ));
                }
            }
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
            "set_properties" => self.control_set_record_properties(req)?,
            "action" => self.control_ui_action(req)?,
            #[cfg(not(target_arch = "wasm32"))]
            "embed_image" => self.control_embed_image(req)?,
            #[cfg(not(target_arch = "wasm32"))]
            "wblock" => self.control_wblock(req)?,
            #[cfg(not(target_arch = "wasm32"))]
            "plot" => self.control_plot(req)?,
            "entities_create" => self.control_entities_create(req)?,
            "entities_delete" => self.control_entities_delete(req)?,
            "entities_transform" => self.control_entities_transform(req)?,
            "entities_copy_to" => self.control_entities_copy_to(req)?,
            "xdata_set" => self.control_xdata_set(req)?,
            "block_define" => self.control_block_define(req)?,
            "block_delete" => self.control_block_delete(req)?,
            "group_create" => self.control_group_create(req)?,
            "selection_set_save" => self.control_selection_set_save(req)?,
            "selection_set_load" => self.control_selection_set_load(req)?,
            "user_select" => self.control_user_select(req)?,
            "getpoint" => self.control_getpoint(req)?,
            "close" => self.control_close(req)?,
            "sysvar" => self.control_sysvar(req)?,
            "layout_create" => self.control_layout_create(req)?,
            "page_setup_set" => self.control_page_setup_set(req)?,
            "file_identity" => self.control_file_identity(req)?,
            #[cfg(not(target_arch = "wasm32"))]
            "view_focus" => self.control_view_focus(req)?,
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
                // A minimized window has a 0x0 surface and the renderer
                // panics reading it back, so report instead of capturing.
                iced::window::size(window).then(move |size| {
                    let path = path.clone();
                    if size.width <= 0.0 || size.height <= 0.0 {
                        Task::done(Message::ControlScreenshot(path, None))
                    } else {
                        iced::window::screenshot(window)
                            .map(move |s| Message::ControlScreenshot(path.clone(), Some(s)))
                    }
                })
            }
            "stop" => {
                self.control.enabled = false;
                Task::none()
            }
            _ => return Err(failure("unknown_operation", "Unknown operation")),
        })
    }

    pub(super) fn set_control_result(&mut self, value: Value) {
        if let Some(operation) = self.control.pending.as_mut() {
            operation.result = value;
        }
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
                    .push_error(crate::t!("Automation target document closed").as_ref());
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
        } else if p.cancelled || p.request["op"] == "cancel" {
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
            let (min, max) = crate::scene::convert::tess::entity_bounds_in(&tab.scene.document, entity);
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
            let curve = crate::entities::curve::entity_curve(entity).map(|planar| {
                let length = planar.curve.length();
                json!({
                    "bounded":length.is_finite(),
                    "closed":planar.curve.is_closed(),
                    "length":length.is_finite().then_some(length),
                    "area":planar.curve.is_closed().then(|| planar.curve.enclosed_area().abs())
                })
            });
            out.push(json!({"handle":format!("{:X}",handle.value()),"type":crate::entities::names::ui_name(entity),"bounds":{"min":min,"max":max},"curve":curve,"mesh":metrics.map(|m|json!({"vertices":m.metrics.vertices,"triangles":m.metrics.triangles,"surface_area":m.metrics.surface_area,"volume":m.metrics.volume,"centroid":m.metrics.centroid,"moment_of_inertia":m.metrics.moment_of_inertia,"principal_directions":m.metrics.principal_directions,"principal_moments":m.metrics.principal_moments,"product_of_inertia":m.metrics.product_of_inertia,"radii_of_gyration":m.metrics.radii_of_gyration}))}));
        }
        json!({"ok":true,"document_id":tab.id,"geometry_revision":tab.scene.geometry_epoch,"measurements":out})
    }

    pub(super) fn control_screenshot(
        &mut self,
        path: String,
        screenshot: Option<iced::window::Screenshot>,
    ) {
        let result = (|| -> Result<Value, String> {
            let s = screenshot
                .ok_or("The window is minimized or has no size; restore it and capture again")?;
            let requested_scope = self
                .control
                .pending
                .as_ref()
                .and_then(|pending| pending.request["scope"].as_str())
                .unwrap_or("window");
            let max_dimension = self
                .control
                .pending
                .as_ref()
                .and_then(|pending| pending.request["max_dimension"].as_u64())
                .unwrap_or(1600)
                .clamp(256, 4096) as u32;
            let mut image =
                image::RgbaImage::from_raw(s.size.width, s.size.height, s.rgba.to_vec())
                    .ok_or("Renderer returned malformed image data")?;
            let mut actual_scope = "window";
            if requested_scope == "viewport" {
                if let Some(bounds) = crate::ui::wrap_bar::dropdown_bounds(
                    crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID,
                ) {
                    let scale = s.scale_factor;
                    let left = (bounds.x * scale).floor().clamp(0.0, image.width() as f32) as u32;
                    let top = (bounds.y * scale).floor().clamp(0.0, image.height() as f32) as u32;
                    let right = ((bounds.x + bounds.width) * scale)
                        .ceil()
                        .clamp(0.0, image.width() as f32) as u32;
                    let bottom = ((bounds.y + bounds.height) * scale)
                        .ceil()
                        .clamp(0.0, image.height() as f32) as u32;
                    if right > left && bottom > top {
                        image = image::imageops::crop_imm(
                            &image,
                            left,
                            top,
                            right - left,
                            bottom - top,
                        )
                        .to_image();
                        actual_scope = "viewport";
                    }
                }
            }
            let longest = image.width().max(image.height());
            if longest > max_dimension {
                let scale = max_dimension as f64 / longest as f64;
                let width = ((image.width() as f64 * scale).round() as u32).max(1);
                let height = ((image.height() as f64 * scale).round() as u32).max(1);
                image = image::imageops::resize(
                    &image,
                    width,
                    height,
                    image::imageops::FilterType::Triangle,
                );
            }
            image
                .save_with_format(&path, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            Ok(
                json!({"path":path,"scope":actual_scope,"width":image.width(),"height":image.height(),"scale_factor":s.scale_factor,"document_id":self.tabs[self.active_tab].id,"revision":self.tabs[self.active_tab].edit_revision,"camera_revision":self.tabs[self.active_tab].scene.camera_generation}),
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
mod entities;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod http_bridge;
mod interactive;
mod sheets;

#[cfg(test)]
mod p1_tests;
pub(super) fn action_names() -> &'static [&'static str] {
    actions::NAMES
}

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
        let started = request(&mut app, json!({"op":"start","cmd":"LINE"}));
        assert_eq!(started["status"], "waiting_input");
        assert!(
            started["state"]["command"]["accepts"]
                .as_array()
                .unwrap()
                .contains(&json!("point"))
        );
        assert_eq!(
            started["state"]["command"]["input_example"]["kind"],
            "point"
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
        assert!(
            !app.control_properties()["sections"]
                .as_array()
                .unwrap()
                .is_empty()
        );
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
    fn command_discovery_explains_batch_and_interactive_use() {
        let mut app = OpenCADStudio::new_for_test();
        let detail = app
            .control_request(json!({"op":"commands","name":"pline"}))
            .0;
        assert_eq!(detail["command"]["name"], "PLINE");
        assert_eq!(
            detail["command"]["batch"]["examples"][0],
            "PLINE 0,0 10,0 10,10 C"
        );
        assert_eq!(detail["command"]["interactive"]["op"], "start");
        assert_eq!(detail["command"]["category"], "create");
        assert_eq!(detail["command"]["preconditions"]["document"], true);
        assert_eq!(
            app.control_request(json!({"op":"commands","name":"NOT_A_COMMAND"}))
                .0["code"],
            "unknown_command"
        );
    }
    #[test]
    fn control_queries_exact_curve_relationships_and_metrics() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE -5,0 5,0"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,-5 0,5"}));
        request(&mut app, json!({"op":"run","cmd":"CIRCLE 20,0 2"}));

        let lines = app.control_request(json!({"op":"query","type":"Line"})).0;
        let first = lines["entities"][0]["handle"].as_str().unwrap();
        let second = lines["entities"][1]["handle"].as_str().unwrap();
        let intersections = app
            .control_request(json!({"op":"query","intersections":[first,second]}))
            .0;
        assert_eq!(intersections["count"], 1, "{intersections}");
        assert_eq!(
            intersections["intersections"][0]["point"],
            json!([0.0, 0.0])
        );

        let nearest = app
            .control_request(json!({"op":"query","near":[20.0,0.0],"limit":1}))
            .0;
        assert_eq!(nearest["entities"][0]["type"], "Circle");
        assert_eq!(nearest["entities"][0]["distance"], 2.0);
        let circle = nearest["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();

        let contains = app
            .control_request(json!({"op":"query","contains_point":[20.0,0.0]}))
            .0;
        assert_eq!(contains["count"], 1, "{contains}");
        assert_eq!(contains["entities"][0]["handle"], circle);

        let projected = app
            .control_request(
                json!({"op":"query","handle":circle,"detail":"full","fields":["type"]}),
            )
            .0;
        assert_eq!(projected["entities"][0].as_object().unwrap().len(), 2);

        let measured = app
            .control_request(json!({"op":"measure","handles":[circle]}))
            .0;
        let curve = &measured["measurements"][0]["curve"];
        assert!((curve["length"].as_f64().unwrap() - std::f64::consts::TAU * 2.0).abs() < 1e-9);
        assert!((curve["area"].as_f64().unwrap() - std::f64::consts::PI * 4.0).abs() < 1e-9);
    }

    #[test]
    fn control_shell_roundtrips_with_kernel_mass_properties() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"start","cmd":"BOX"}));
        request(
            &mut app,
            json!({"op":"input","kind":"point","point":[0.0,0.0,0.0]}),
        );
        request(
            &mut app,
            json!({"op":"input","kind":"point","point":[6.0,5.0,0.0]}),
        );
        assert_eq!(
            request(&mut app, json!({"op":"input","kind":"token","text":"4"}))["status"],
            "completed"
        );
        let handle = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .find_map(|entity| {
                matches!(entity, acadrust::EntityType::Solid3D(_)).then(|| entity.common().handle)
            })
            .unwrap();
        let handle_text = format!("{:X}", handle.value());

        request(&mut app, json!({"op":"select","handles":[handle_text]}));
        request(&mut app, json!({"op":"start","cmd":"SHELL"}));
        request(
            &mut app,
            json!({"op":"input","kind":"entity","handle":handle_text,"point":[3.0,2.5,4.0]}),
        );
        request(&mut app, json!({"op":"input","kind":"enter"}));
        assert_eq!(
            request(&mut app, json!({"op":"input","kind":"token","text":"0.5"}))["status"],
            "completed"
        );

        let measured = app.control_measure(&json!({"handles":[handle_text]}));
        let mesh = &measured["measurements"][0]["mesh"];
        assert!((mesh["volume"].as_f64().unwrap() - 50.0).abs() < 1e-9);
        assert_eq!(mesh["principal_moments"].as_array().unwrap().len(), 3);
        assert!(matches!(
            app.tabs[app.active_tab]
                .scene
                .document
                .solid_history_operation(handle),
            Some(acadrust::objects::SolidHistoryOperation::Brep(_))
        ));

        assert_eq!(request(&mut app, json!({"op":"undo"}))["ok"], true);
        let restored = app.control_measure(&json!({"handles":[handle_text]}));
        assert!(
            (restored["measurements"][0]["mesh"]["volume"]
                .as_f64()
                .unwrap()
                - 120.0)
                .abs()
                < 1e-9
        );
        assert_eq!(request(&mut app, json!({"op":"redo"}))["ok"], true);

        let path = std::env::temp_dir().join(format!("ocs-shell-{}.dwg", session_id()));
        assert_eq!(
            request(&mut app, json!({"op":"save","path":path}))["ok"],
            true
        );
        request(&mut app, json!({"op":"new"}));
        assert_eq!(
            request(&mut app, json!({"op":"open","path":path}))["status"],
            "completed"
        );
        let reopened = app.control_measure(&json!({"handles":[handle_text]}));
        assert!(
            (reopened["measurements"][0]["mesh"]["volume"]
                .as_f64()
                .unwrap()
                - 50.0)
                .abs()
                < 1e-9
        );
        let _ = std::fs::remove_file(path);
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

    #[test]
    fn open_from_bytes_validates_the_request() {
        let ok = open_bytes_request(&json!({"name":"C:\\plans\\plan.dwg","data_base64":"AAEC"}));
        assert_eq!(ok, Ok(("plan.dwg".to_string(), vec![0, 1, 2])));
        for (req, error) in [
            (json!({"data_base64":"AAEC"}), "Missing name"),
            (json!({"name":"dir/","data_base64":"AAEC"}), "Missing name"),
            (
                json!({"name":"a.dwg","data_base64":""}),
                "data_base64 is empty",
            ),
        ] {
            assert_eq!(
                open_bytes_request(&req).unwrap_err()["error"],
                error,
                "{req}"
            );
        }
        let bad = open_bytes_request(&json!({"name":"a.dwg","data_base64":"not base64!"}));
        assert_eq!(bad.unwrap_err()["code"], "invalid_request");
    }
    #[test]
    fn open_from_bytes_is_refused_natively_without_touching_documents() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let tabs = app.tabs.len();
        let reply = request(
            &mut app,
            json!({"op":"open","name":"a.dxf","data_base64":"AAEC"}),
        );
        assert_eq!(reply["code"], "invalid_request", "{reply}");
        assert_eq!(app.tabs.len(), tabs);
        assert!(app.opening.is_none());
    }

    #[test]
    fn point_steps_that_take_keywords_accept_points() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"CIRCLE 0,0 3"}));
        request(&mut app, json!({"op":"select","type":"CIRCLE"}));
        let started = request(&mut app, json!({"op":"start","cmd":"MOVE"}));
        assert_eq!(started["status"], "waiting_input", "{started}");
        let command = &started["state"]["command"];
        let accepts = command["accepts"].as_array().unwrap();
        assert!(accepts.contains(&json!("point")), "{command}");
        assert!(accepts.contains(&json!("token")), "{command}");
        assert_eq!(command["input_example"]["kind"], "point");
        let base = request(
            &mut app,
            json!({"op":"input","kind":"point","point":[0.,0.,0.]}),
        );
        assert_eq!(base["status"], "waiting_input", "{base}");
        let moved = request(
            &mut app,
            json!({"op":"input","kind":"point","point":[0.,-10.,0.]}),
        );
        assert_eq!(moved["status"], "completed", "{moved}");
        let circle = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .find_map(|e| match e {
                acadrust::EntityType::Circle(c) => Some(c.center.y),
                _ => None,
            });
        assert_eq!(circle, Some(-10.0));
    }

    #[test]
    fn embed_image_op_places_an_ole2frame_and_undoes_it() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let img = image::RgbaImage::from_pixel(40, 30, image::Rgba([9, 9, 9, 255]));
        let path = std::env::temp_dir().join(format!("ocs-embed-{}.png", session_id()));
        img.save(&path).unwrap();

        let response = request(
            &mut app,
            json!({"op":"embed_image","path":path.to_string_lossy(),"at":[0,0,0],"width":20}),
        );
        assert_eq!(response["status"], "completed", "{response}");
        let handle = response["result"]["handle"].as_str().unwrap().to_string();
        assert_eq!(response["result"]["kind"], "Ole2Frame");
        let ole_count = |app: &OpenCADStudio| {
            app.tabs[app.active_tab]
                .scene
                .document
                .entities()
                .filter(|e| matches!(e, acadrust::EntityType::Ole2Frame(_)))
                .count()
        };
        assert_eq!(ole_count(&app), 1);

        // The entity must be queryable like any other and undoable as one step.
        let queried =
            app.automation_op(json!({"op":"entities","type":"OLE2FRAME"}).to_string().as_str());
        assert_eq!(queried["total"], 1, "{queried}");
        request(&mut app, json!({"op":"undo"}));
        assert_eq!(ole_count(&app), 0);
        let _ = std::fs::remove_file(path);
        let _ = handle;
    }

    fn user_select_pick(app: &mut OpenCADStudio, kind: &str) -> String {
        let handle = app.automation_op(
            format!(r#"{{"op":"query","type":"{kind}","detail":"summary"}}"#).as_str(),
        )["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let value = u64::from_str_radix(&handle, 16).unwrap();
        app.tabs[app.active_tab]
            .scene
            .select_entity(acadrust::Handle::new(value), false);
        handle
    }

    fn user_select_result(app: &mut OpenCADStudio, request_id: &str) -> Value {
        app.control_request(json!({"op":"operation","request_id":request_id}))
            .0
    }

    #[test]
    fn user_select_stays_running_until_the_user_confirms_then_returns_entities() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        assert_eq!(request(&mut app, json!({"op":"new"}))["status"], "completed");
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,10"}));
        let asked = request(
            &mut app,
            json!({"op":"user_select","request_id":"us-1","type":"LINE"}),
        );
        assert_eq!(asked["status"], "running", "{asked}");
        // A second request waits like any other mutation while one is open.
        let busy = request(&mut app, json!({"op":"user_select","request_id":"us-2"}));
        assert_eq!(busy["code"], "busy", "{busy}");
        // The person picks the line in the viewport and presses Enter.
        let handle = user_select_pick(&mut app, "LINE");
        let _ = app.update(Message::CommandFinalize);
        let done = user_select_result(&mut app, "us-1");
        assert_eq!(done["status"], "completed", "{done}");
        assert_eq!(done["result"]["cancelled"], false);
        assert_eq!(done["result"]["count"], 1);
        assert_eq!(done["result"]["handles"][0], json!(handle));
        assert_eq!(done["result"]["entities"][0]["type"], "Line");
        // detail defaults to full, so the whole entity object rides along.
        assert!(
            done["result"]["entities"][0]["properties"].is_object(),
            "{done}"
        );
    }

    #[test]
    fn user_select_filter_deselects_entities_outside_the_request() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,10"}));
        request(&mut app, json!({"op":"run","cmd":"CIRCLE 0,0 5"}));
        request(
            &mut app,
            json!({"op":"user_select","request_id":"us-f","type":"CIRCLE"}),
        );
        user_select_pick(&mut app, "LINE");
        user_select_pick(&mut app, "CIRCLE");
        let _ = app.update(Message::CommandFinalize);
        let done = user_select_result(&mut app, "us-f");
        assert_eq!(done["status"], "completed", "{done}");
        assert_eq!(done["result"]["count"], 1);
        assert_eq!(done["result"]["ignored"], 1);
        assert_eq!(done["result"]["entities"][0]["type"], "Circle");
        // The filtered-out pick is deselected in the drawing too.
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .selected_handles_in_order()
                .len(),
            1
        );
    }

    #[test]
    fn user_select_escape_cancels_and_an_empty_enter_confirms_zero() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,10"}));
        request(&mut app, json!({"op":"user_select","request_id":"us-x"}));
        let _ = app.update(Message::CommandEscape);
        let cancelled = user_select_result(&mut app, "us-x");
        assert_eq!(cancelled["status"], "cancelled", "{cancelled}");
        assert_eq!(cancelled["result"]["cancelled"], true);
        assert_eq!(cancelled["result"]["count"], 0);
        // An immediate Enter with nothing picked answers the
        // request with an empty set rather than hanging forever.
        request(&mut app, json!({"op":"user_select","request_id":"us-e"}));
        let _ = app.update(Message::CommandFinalize);
        let empty = user_select_result(&mut app, "us-e");
        assert_eq!(empty["result"]["cancelled"], false, "{empty}");
        assert_eq!(empty["result"]["count"], 0);
        // A client can also retract its own request programmatically.
        request(&mut app, json!({"op":"user_select","request_id":"us-c"}));
        let retracted = request(&mut app, json!({"op":"cancel","request_id":"us-c-x"}));
        assert_eq!(retracted["status"], "cancelled", "{retracted}");
        assert_eq!(retracted["result"]["cancelled"], true, "{retracted}");
    }

    #[test]
    fn user_select_request_is_printed_and_stays_visible_until_answered() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,10"}));
        request(
            &mut app,
            json!({"op":"user_select","request_id":"us-ui","type":"LINE","prompt":"Chọn block"}),
        );
        // The message list names the request — who asked and what for.
        let last = app.command_line.history.last().unwrap();
        assert!(
            last.text.contains("us-ui") && last.text.contains("Chọn block"),
            "{last:?}"
        );
        // The end-of-update driver pins that line so it cannot fade away
        // while the pick is still open.
        let label = app.pending_pick_label().expect("pending label");
        app.command_line.set_step_prompt(Some(label));
        assert!(
            app.command_line.history.last().unwrap().pinned,
            "{:?}",
            app.command_line.history.last().unwrap()
        );
        // The person answers: the line un-pins and the resolution is printed
        // with the same identity.
        user_select_pick(&mut app, "LINE");
        let _ = app.update(Message::CommandFinalize);
        assert!(app.pending_pick_label().is_none());
        let tail: Vec<String> = app
            .command_line
            .history
            .iter()
            .rev()
            .take(3)
            .map(|e| e.text.clone())
            .collect();
        assert!(
            tail.iter()
                .any(|t| t.contains("us-ui") && t.contains("1 object(s)")),
            "{tail:?}"
        );
    }

    #[test]
    fn user_select_requires_a_gui_window() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let reply = request(&mut app, json!({"op":"user_select","request_id":"us-h"}));
        assert_eq!(reply["code"], "gui_required", "{reply}");
    }

    #[test]
    fn getpoint_resolves_with_the_clicked_point_and_esc_cancels() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        request(&mut app, json!({"op":"new"}));
        let asked = request(
            &mut app,
            json!({"op":"getpoint","request_id":"gp-1","prompt":"Chọn vị trí QR"}),
        );
        assert_eq!(asked["status"], "running", "{asked}");
        // While the pick is pending, everything else waits.
        let busy = request(&mut app, json!({"op":"getpoint","request_id":"gp-2"}));
        assert_eq!(busy["code"], "busy", "{busy}");
        // The person clicks: the snapped world point under the cursor is the
        // answer, and the click leaves the selection untouched.
        app.tabs[app.active_tab].last_cursor_world = glam::DVec3::new(125.5, 64.25, 0.0);
        let _ = app.update(Message::ViewportLeftPress);
        let done = user_select_result(&mut app, "gp-1");
        assert_eq!(done["status"], "completed", "{done}");
        assert_eq!(done["result"]["point"], json!([125.5, 64.25, 0.0]));
        // Escape with no request pending is still just Escape.
        let _ = app.update(Message::CommandEscape);
        // A second pick parks again and cancels on Escape.
        request(&mut app, json!({"op":"getpoint","request_id":"gp-3"}));
        let _ = app.update(Message::CommandEscape);
        let cancelled = user_select_result(&mut app, "gp-3");
        assert_eq!(cancelled["status"], "cancelled", "{cancelled}");
        assert_eq!(cancelled["result"]["cancelled"], true);
    }

    #[test]
    fn getpoint_snap_marker_and_snapped_answer_while_pending() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 100,0"}));
        let i = app.active_tab;
        {
            let tab = &mut app.tabs[i];
            tab.scene.selection.borrow_mut().vp_size = (1920.0, 1080.0);
            tab.scene.sync_tiles_from_panes(1920.0, 1080.0);
            tab.scene.fit_all();   // bring the line into the pane so snaps can hit
        }
        // Park a client pick: with no command running, a cursor move must
        // still run the object-snap engine so the marker shows and the
        // click hands the snapped point back.
        let asked = request(
            &mut app,
            json!({"op":"getpoint","request_id":"gp-snap","prompt":"Chọn góc"}),
        );
        assert_eq!(asked["status"], "running", "{asked}");

        // Screen position of the line's endpoint through the live camera.
        let endpoint = glam::DVec3::new(100.0, 0.0, 0.0);
        let (sx, sy) = {
            let cam = app.tabs[i].scene.camera.borrow();
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            };
            let view = cam.view_proj_rte(bounds);
            let eye = cam.eye();
            let ndc = view.project_point3((endpoint - eye).as_vec3());
            (
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            )
        };
        let _ = app.update(Message::ViewportMove(iced::Point::new(
            sx as f32,
            sy as f32,
        )));

        assert!(
            app.tabs[i].snap_result.is_some(),
            "no object-snap marker while a getpoint is pending"
        );
        let snapped = app.tabs[i].last_cursor_world;
        assert!(
            (snapped - endpoint).length() < 1e-6,
            "cursor point not snapped to the endpoint: {snapped:?}"
        );

        // The click answers the pick with the snapped endpoint.
        let _ = app.update(Message::ViewportLeftPress);
        let done = user_select_result(&mut app, "gp-snap");
        assert_eq!(done["status"], "completed", "{done}");
        assert_eq!(done["result"]["point"], json!([100.0, 0.0, 0.0]));
    }

    #[test]
    fn user_select_snap_marker_shows_while_pending() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 100,0"}));
        let i = app.active_tab;
        {
            let tab = &mut app.tabs[i];
            tab.scene.selection.borrow_mut().vp_size = (1920.0, 1080.0);
            tab.scene.sync_tiles_from_panes(1920.0, 1080.0);
            tab.scene.fit_all();
        }
        let asked = request(
            &mut app,
            json!({"op":"user_select","request_id":"us-snap","prompt":"Chọn trường","clear":true}),
        );
        assert_eq!(asked["status"], "running", "{asked}");

        let endpoint = glam::DVec3::new(100.0, 0.0, 0.0);
        let (sx, sy) = {
            let cam = app.tabs[i].scene.camera.borrow();
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            };
            let view = cam.view_proj_rte(bounds);
            let eye = cam.eye();
            let ndc = view.project_point3((endpoint - eye).as_vec3());
            (
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            )
        };
        let _ = app.update(Message::ViewportMove(iced::Point::new(
            sx as f32,
            sy as f32,
        )));
        assert!(
            app.tabs[i].snap_result.is_some(),
            "no object-snap marker while a user_select is pending"
        );

        // The pick itself stays entity-based: selecting the line + Enter still
        // answers the request — the marker must not break selection.
        let handle = app.automation_op(r#"{"op":"query","type":"LINE","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let value = u64::from_str_radix(&handle, 16).unwrap();
        app.tabs[i].scene.select_entity(acadrust::Handle::new(value), false);
        let _ = app.update(Message::CommandFinalize);
        let done = user_select_result(&mut app, "us-snap");
        assert_eq!(done["status"], "completed", "{done}");
        assert_eq!(done["result"]["count"], 1);
    }

    #[test]
    fn get_selection_reports_insert_block_and_position() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,10"}));
        let line = app.automation_op(r#"{"op":"query","type":"LINE","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        // A title-block-style definition with one placed reference.
        request(
            &mut app,
            json!({"op":"block_define","name":"A3","base":[0,0],"handles":[line]}),
        );
        let insert = app.automation_op(r#"{"op":"query","type":"Insert","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        app.automation_op(&format!(r#"{{"op":"select","handles":["{insert}"]}}"#));
        let read = request(&mut app, json!({"op":"get_selection"}));
        assert_eq!(read["status"], "completed", "{read}");
        assert_eq!(read["result"]["count"], 1);
        let entity = &read["result"]["entities"][0];
        assert_eq!(entity["type"], "Block Reference");
        assert_eq!(entity["block"], "A3");
        assert!(entity["position"].is_array(), "{entity}");
        // Non-inserts answer null for the block field.
        app.tabs[app.active_tab].scene.deselect_all();
        let circle = request(
            &mut app,
            json!({"op":"entities_create","entities":[{"type":"Circle","center":[0,0],"radius":2}]}),
        );
        let circle_handle = circle["result"]["handles"][0].as_str().unwrap().to_owned();
        app.automation_op(&format!(r#"{{"op":"select","handles":["{circle_handle}"]}}"#));
        let read = request(&mut app, json!({"op":"get_selection"}));
        assert_eq!(read["result"]["count"], 1);
        assert_eq!(read["result"]["entities"][0]["block"], Value::Null);
    }

    #[test]
    fn plot_op_applies_per_layer_lineweights() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        {
            let scene = &mut app.tabs[app.active_tab].scene;
            // Two layers set to different pen weights; entities fully ByLayer.
            for (name, weight) in [
                ("THIN", acadrust::types::LineWeight::Value(13)),
                ("THICK", acadrust::types::LineWeight::Value(50)),
            ] {
                let mut layer = acadrust::tables::Layer::new(name);
                layer.line_weight = weight;
                let _ = scene.document.layers.add(layer);
            }
            for (name, origin) in [("THIN", 0.0), ("THICK", 3000.0)] {
                let mut line = acadrust::entities::Line::new();
                line.common.layer = name.to_string();
                line.start = acadrust::types::Vector3::new(origin, origin, 0.0);
                line.end = acadrust::types::Vector3::new(origin + 3000.0, origin + 2000.0, 0.0);
                scene.add_entity(acadrust::EntityType::Line(line));
            }
        }

        let pdf = std::env::temp_dir().join(format!("ocs-plot-lw-{}.pdf", std::process::id()));
        let response = request(
            &mut app,
            json!({"op":"plot","path":pdf.to_string_lossy(),"plot_style":"monochrome.ctb"}),
        );
        assert_eq!(response["status"], "completed", "{response}");

        // The plotted PDF carries two distinct pen widths — the per-layer
        // 0.13 mm / 0.50 mm hierarchy survives the plot style (thin clamps to
        // the 1 px floor, so the ratio is the clamped-pixel ratio 1.889).
        let text = crate::io::pdf_export::pdf_stream_text(&std::fs::read(&pdf).expect("pdf written"));
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let mut widths: Vec<f32> = tokens
            .windows(2)
            .filter_map(|pair| {
                if pair[1] == "w" {
                    pair[0].parse::<f32>().ok()
                } else {
                    None
                }
            })
            .collect();
        widths.sort_by(|a, b| a.partial_cmp(b).unwrap());
        widths.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        widths.retain(|width| *width > 0.2);
        assert_eq!(widths.len(), 2, "two layer weights in the plotted PDF: {widths:?}");
        assert!(
            (widths[1] / widths[0] - 1.889).abs() < 0.15,
            "0.50 mm vs 0.13 mm hierarchy: {widths:?}"
        );
        let _ = std::fs::remove_file(&pdf);
    }

    #[test]
    fn plot_op_applies_ctb_colors_through_layer_aci() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        {
            let scene = &mut app.tabs[app.active_tab].scene;
            // Frame layer carries the classic cyan ACI 4; its entity is ByLayer.
            let mut frame = acadrust::tables::Layer::new("FRAME-CYAN");
            frame.color = acadrust::types::Color::Index(4);
            let _ = scene.document.layers.add(frame);
            let mut frame_line = acadrust::entities::Line::new();
            frame_line.common.layer = "FRAME-CYAN".to_string();
            frame_line.start = acadrust::types::Vector3::new(0.0, 0.0, 0.0);
            frame_line.end = acadrust::types::Vector3::new(3000.0, 0.0, 0.0);
            scene.add_entity(acadrust::EntityType::Line(frame_line));

            // An explicit red ACI 1 and a true-color green: index must map
            // through the CTB, the true color must stay RGB (aci 0).
            let mut red = acadrust::entities::Line::new();
            red.common.color = acadrust::types::Color::Index(1);
            red.start = acadrust::types::Vector3::new(0.0, 1000.0, 0.0);
            red.end = acadrust::types::Vector3::new(3000.0, 1000.0, 0.0);
            scene.add_entity(acadrust::EntityType::Line(red));

            let mut true_color = acadrust::entities::Line::new();
            true_color.common.color = acadrust::types::Color::from_true_color_value(0x0000FF00);
            true_color.start = acadrust::types::Vector3::new(0.0, 2000.0, 0.0);
            true_color.end = acadrust::types::Vector3::new(3000.0, 2000.0, 0.0);
            scene.add_entity(acadrust::EntityType::Line(true_color));
        }

        let pdf = std::env::temp_dir().join(format!("ocs-plot-aci-{}.pdf", std::process::id()));
        let response = request(
            &mut app,
            json!({"op":"plot","path":pdf.to_string_lossy(),"plot_style":"monochrome.ctb"}),
        );
        assert_eq!(response["status"], "completed", "{response}");

        let text = crate::io::pdf_export::pdf_stream_text(&std::fs::read(&pdf).expect("pdf written"));
        // Monochrome maps every index colour (ByLayer cyan frame, explicit
        // red) to black; the true-color line keeps its RGB by definition.
        assert!(
            !text.contains("0 1 1 rg") && !text.contains("0 1 1 RG"),
            "cyan frame must be CTB-mapped to monochrome"
        );
        assert!(
            !text.contains("1 0 0 rg") && !text.contains("1 0 0 RG"),
            "explicit red must be CTB-mapped to monochrome"
        );
        assert!(
            text.contains("0 1 0 rg") || text.contains("0 1 0 RG"),
            "true-color line keeps its RGB (aci 0 is not CTB-mapped)"
        );
        assert!(
            text.contains("0 0 0 rg") || text.contains("0 0 0 RG"),
            "monochrome strokes must plot black"
        );

        // Without a plot style the same drawing keeps its colours: the cyan
        // frame plots as the dark-blue print remap, the red line stays red.
        let plain = std::env::temp_dir().join(format!("ocs-plot-aci-plain-{}.pdf", std::process::id()));
        let response = request(&mut app, json!({"op":"plot","path":plain.to_string_lossy()}));
        assert_eq!(response["status"], "completed", "{response}");
        let text = crate::io::pdf_export::pdf_stream_text(&std::fs::read(&plain).expect("pdf written"));
        let color_ops: Vec<[f32; 3]> = {
            let tokens: Vec<&str> = text.split_whitespace().collect();
            tokens
                .windows(4)
                .filter_map(|quad| {
                    if quad[3] != "rg" && quad[3] != "RG" {
                        return None;
                    }
                    Some([
                        quad[0].parse::<f32>().ok()?,
                        quad[1].parse::<f32>().ok()?,
                        quad[2].parse::<f32>().ok()?,
                    ])
                })
                .collect()
        };
        let near = |color: &[f32; 3], target: [f32; 3]| {
            color
                .iter()
                .zip(target)
                .all(|(channel, expected)| (channel - expected).abs() < 0.02)
        };
        assert!(
            color_ops.iter().any(|c| near(c, [0.0, 0.15, 0.5])),
            "unstyled cyan frame plots as the dark-blue print remap: {color_ops:?}"
        );
        assert!(
            color_ops.iter().any(|c| near(c, [1.0, 0.0, 0.0])),
            "unstyled red line keeps its colour: {color_ops:?}"
        );
        assert!(
            color_ops.iter().any(|c| near(c, [0.0, 1.0, 0.0])),
            "unstyled true-color line keeps its colour: {color_ops:?}"
        );
        let _ = std::fs::remove_file(&pdf);
        let _ = std::fs::remove_file(&plain);
    }
}
