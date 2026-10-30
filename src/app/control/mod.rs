//! Semantic control shared by the GUI, headless client and web adapter.
use super::{Message, OpenCADStudio};
use crate::command::{InputKind, StepInput};
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
    /// Set by the first control request of the session (or by a web host's
    /// boot-time opt-in): while control is enabled, startup prompts that
    /// would block automation — the donation prompt — are suppressed (#1349).
    pub automation_seen: bool,
    serial: u64,
    events: VecDeque<Value>,
    /// Monotonic counter bumped on every real selection change, so clients
    /// can detect highlight changes without polling the full selection.
    selection_revision: u64,
    pub(super) routing: bool,
    /// Session-scoped named handle sets for selection_set_save/load.
    selection_sets: std::collections::BTreeMap<String, Vec<codec::Handle>>,
    /// Live `user_select` request — the client asked the person at the screen
    /// to pick entities; Some until that person answers with Enter/Escape.
    pub(super) user_select: Option<UserSelectSession>,
    /// Live `getpoint` request — the client asked for one picked point; Some
    /// until the person clicks (answer) or presses Escape (cancel).
    pub(super) get_point: Option<UserPointSession>,
    /// `capture` request parked until the window has drawn another frame:
    /// screenshotting in the same batch as the state change used to return the
    /// previous frame (C11). Cleared by `on_control_capture_frame`.
    pub(super) capture_frame_pending: bool,
    /// Whether the parked capture actually waited for a redraw (vs. the
    /// anti-hang timer). Reported in the capture result as `waited_for_frame`.
    pub(super) capture_after_redraw: bool,
    /// Web: when a parked capture gives up. Only a presented frame can be
    /// read back on wasm, and a hidden or idle tab may never present one, so
    /// the timer tick fails the capture past this instant instead of letting
    /// it hold the control slot forever.
    #[cfg(target_arch = "wasm32")]
    pub(super) capture_deadline: Option<iced::time::Instant>,
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
/// Edits that only touch a document's records, so they can address an
/// open document behind another tab (#2032). Interactive commands, input,
/// picks, view and UI actions, plotting and saving still need the tab active.
fn background_mutation(op: &str) -> bool {
    matches!(
        op,
        "set_properties"
            | "entities_create"
            | "entities_delete"
            | "entities_transform"
            | "text_replace"
            | "xdata_set"
            | "block_define"
            | "block_delete"
            | "group_create"
            | "undo"
            | "redo"
            | "save"
    )
}

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
/// `{"op":"font_register","file_name":"Brand-Regular.ttf","data_base64":"…"}`:
/// a host hands over a TrueType/OpenType file that text styles reference by
/// name. The web build has no font folder, so this is how such styles get
/// their real face instead of a fallback.
fn font_register_request(req: &Value) -> Result<(String, Vec<u8>), Value> {
    use base64::Engine as _;
    let name = string(req, "file_name")?;
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    if name.is_empty() {
        return Err(failure("invalid_request", "Missing file_name"));
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
fn handle(req: &Value) -> Result<codec::Handle, Value> {
    u64::from_str_radix(string(req, "handle")?.trim_start_matches("0x"), 16)
        .map(codec::Handle::new)
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
        "TXTEXP",
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
    } else if [
        "DIST",
        "ID",
        "AREA",
        "MEASURE",
        "MEASUREGEOM",
        "MEASURERADIUS",
        "MEASUREANGLE",
    ]
    .contains(&name)
    {
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
        "DCDISPLAY",
        "DIMDISASSOCIATE",
        "DIMREASSOCIATE",
        "ERASE",
        "EXPLODE",
        "TXTEXP",
        "TXT2MTXT",
        "GROUP",
        "LAYFRZ",
        "LAYLCK",
        "LAYCUR",
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

fn xref_status_name(status: &crate::io::xref::XrefStatus) -> &'static str {
    use crate::io::xref::XrefStatus as S;
    match status {
        S::Loaded => "loaded",
        S::Recovered => "recovered",
        S::Failed => "failed",
        S::NotFound => "not_found",
        S::Unloaded => "unloaded",
    }
}

impl OpenCADStudio {
    pub(super) fn control_busy(&self) -> bool {
        self.control.pending.is_some()
    }

    /// A client's `cancel`: Esc, repeated while a command is still active.
    /// One Esc may only dismiss what sits in front of the command (an open
    /// dialog, a popup, typed text) or step a command back a level, and the
    /// client has no other way to finish the job. (#1930)
    fn control_cancel(&mut self) -> Task<Message> {
        let mut tasks = vec![self.update(Message::CommandEscape)];
        // ponytail: fixed Esc budget; covers every overlay + step-back chain
        // in the escape path today, raise it if a deeper chain appears.
        for _ in 0..8 {
            if self.tabs[self.active_tab].active_cmd.is_none() {
                break;
            }
            tasks.push(self.update(Message::CommandEscape));
        }
        Task::batch(tasks)
    }

    /// Mirror a real selection change into the automation event stream. The
    /// plugin host already publishes the V4 selection event; automation clients
    /// need the same signal instead of polling `state`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn control_record_selection_event(&mut self, handles: &[codec::Handle]) {
        let (document_id, revision) = {
            let tab = &self.tabs[self.active_tab];
            (tab.id, tab.edit_revision)
        };
        let selection: Vec<String> = handles
            .iter()
            .map(|handle| format!("{:X}", handle.value()))
            .collect();
        self.control.selection_revision = self.control.selection_revision.wrapping_add(1);
        let selection_revision = self.control.selection_revision;
        self.control.serial += 1;
        self.control.events.push_back(json!({
            "sequence": self.control.serial,
            "kind": "selection",
            "document_id": document_id,
            "revision": revision,
            "selection_revision": selection_revision,
            "selection": selection,
        }));
        while self.control.events.len() > 128 {
            self.control.events.pop_front();
        }
    }

    pub(super) fn control_state(&self) -> Value {
        let tab = &self.tabs[self.active_tab];
        let command = tab.active_cmd.as_deref().map(active_command_metadata);
        let missing_fonts = crate::io::font_repo::missing_shx_fonts(&tab.scene.document);
        json!({"ok":true,"protocol":1,"session_id":session_id(),"version":env!("OCS_APP_VERSION"),
            "mode":if self.main_window.is_some(){"gui"}else{"headless"},"enabled":self.control.enabled,
            "read_only":self.read_only,"missing_fonts":missing_fonts,
            "xrefs":tab.xref_report.iter().map(|(name,path,status)|json!({"name":name,"path":path,"status":xref_status_name(status)})).collect::<Vec<_>>(),
            "document_id":tab.id,"revision":tab.edit_revision,"geometry_revision":tab.scene.geometry_epoch,"camera_revision":tab.scene.camera_generation,
            "hand_seed":format!("{:X}",tab.scene.document.header.handle_seed.max(tab.scene.document.next_handle())),
            "plugins":plugin_ids(),
            "documents":self.tabs.iter().map(|t|json!({"id":t.id,"title":t.tab_title,"path":t.current_path,"dirty":t.dirty,"revision":t.edit_revision,"start":t.is_start})).collect::<Vec<_>>(),
            "selection":tab.scene.selected_handles_in_order().iter().map(|h|format!("{:X}",h.value())).collect::<Vec<_>>(),
            "selection_revision":self.control.selection_revision,
            "command":command,"modal":self.active_modal.as_ref().map(|m|format!("{m:?}")),
            "layout":tab.scene.current_layout,
            "ucs":tab.active_ucs.as_ref().map(|u|json!({"name":u.name,"origin":[u.origin.x,u.origin.y,u.origin.z],"x_axis":[u.x_axis.x,u.x_axis.y,u.x_axis.z],"y_axis":[u.y_axis.x,u.y_axis.y,u.y_axis.z],"elevation":u.elevation})),
            "cursor":{"world":tab.last_cursor_world.to_array(),"screen":[tab.last_cursor_screen.x,tab.last_cursor_screen.y]},
            "viewport_size":({let s=tab.scene.selection.borrow();[s.view.vp_size.0,s.view.vp_size.1]}),
            "camera":({let c=tab.scene.camera.borrow();json!({"target":c.target.to_array(),"rotation":[c.rotation.x,c.rotation.y,c.rotation.z,c.rotation.w],"distance":c.distance,"fov_y":c.fov_y,"projection":format!("{:?}",c.projection),"yaw":c.yaw,"pitch":c.pitch})}),
            "mtext_editor":self.mtext_editor.as_ref().map(|e|json!({"text":e.content.text(),"height":e.height,"style":e.style})),
            "text_editor":self.text_inline.is_some(),"event_cursor":self.control.serial,
            "operation":self.control.pending.as_ref().map(|p| &p.id),
            "capabilities":["commands","command_manifest","step_input","batch","compact_results","entity_pick","structure_pick","selection","user_select","properties","records","record_schemas","record_filters","atomic_record_updates","layers","history","documents","events","capture","viewport_capture","measure","spatial_query","audit","verified_save","explicit_save_version"]
        })
    }

    pub(super) fn control_request(&mut self, req: Value) -> (Value, Task<Message>) {
        // First control contact marks the session automated (#1349): a
        // DonationPrompt that is still queued (not yet shown) is dropped so
        // automation never waits behind it, and `queue_startup_prompts`
        // stops queueing one. A prompt already on screen stays — a human
        // may be reading it; clients close it with `close_modal`.
        if !self.control.automation_seen && self.control.enabled {
            self.control.automation_seen = true;
            if self.active_modal != Some(super::ModalKind::DonationPrompt) {
                self.pending_startup_modals
                    .retain(|kind| kind != &super::ModalKind::DonationPrompt);
            }
        }
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
                | "snap"
                | "grips"
                | "query"
                | "records"
                | "record_schema"
                | "capabilities"
                | "tools"
                | "entities"
                | "layers"
                | "header"
                | "history"
                | "xdata_get"
                | "get_selection"
                | "audit"
                | "text_search"
                | "text_audit"
                | "blocks"
                | "block_instances"
                | "cad_get_blocks"
                | "cad_extract_block_instances"
                | "keytips"
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
        #[cfg(not(target_arch = "wasm32"))]
        if op == "tools" {
            return (
                json!({
                    "ok": true,
                    "status": "completed",
                    "tools": crate::mcp::tool_definitions(),
                    "instructions": crate::mcp::INSTRUCTIONS
                }),
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
                        failure(
                            "request_id_reused",
                            format!(
                                "Request payload changed for request_id '{id}'; generate a new unique request_id or resend the original payload",
                            ),
                        )
                    },
                    Task::none(),
                );
            }
            if let Some(p) = &self.control.pending {
                // `cancel` is a reference to the pending request, not a
                // replay of its payload. It must reach the interactive cancel
                // branch below even when it uses the same request_id.
                if p.id == id && op != "cancel" {
                    return (
                        if p.request == req {
                            json!({"ok":true,"status":"running","request_id":id})
                        } else {
                            failure(
                                "request_id_reused",
                                format!(
                                    "Request payload changed for request_id '{id}'; generate a new unique request_id or resend the original payload",
                                ),
                            )
                        },
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
                // Any other request settles once the work it started reports
                // back. When that never happens (a save or open whose result
                // never arrives) the bridge would stay busy until the app is
                // killed, so `cancel` ends the active command and closes the
                // request as cancelled. (#1801)
                if op == "cancel" {
                    let task = self.control_cancel();
                    if let Some(p) = self.control.pending.as_mut() {
                        p.cancelled = true;
                        p.pending = 0;
                    }
                    self.control_settle_inner(true);
                    let response = self
                        .control
                        .completed
                        .iter()
                        .find(|(i, _, _)| *i == pending_id)
                        .map(|(_, _, r)| r.clone());
                    return (
                        response
                            .unwrap_or_else(|| failure("busy", "Wait for the running operation")),
                        task,
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
                && !matches!(
                    op,
                    "activate" | "switch_document" | "close" | "entities_copy_to"
                )
            {
                if req["active_only"].as_bool() == Some(true)
                    || !query && !background_mutation(op)
                {
                    return (
                        failure(
                            "document_not_active",
                            "This operation needs the document's tab active; activate it first",
                        ),
                        Task::none(),
                    );
                }
                return self.control_request_in_background(id, req);
            }
        // Interactive requests operate on the active tab by definition, so
        // a client may omit document_id (the GUI-hosted REST channel does).
        } else if !query
            && !matches!(
                op,
                "new" | "open" | "font_register" | "stop" | "user_select" | "getpoint"
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
                // The object snap a cursor over `point` (world) would get;
                // `from` is the base for perpendicular and tangent.
                "snap" => 'snap: {
                    let p = &req["point"];
                    let (Some(x), Some(y)) = (p[0].as_f64(), p[1].as_f64()) else {
                        break 'snap failure("invalid_point", "point must be [x, y] or [x, y, z]");
                    };
                    let world = glam::DVec3::new(x, y, p[2].as_f64().unwrap_or(0.0));
                    let from = req["from"].as_array().map(|f| {
                        glam::DVec3::new(
                            f.first().and_then(|v| v.as_f64()).unwrap_or(0.0),
                            f.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                            f.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        )
                    });
                    let i = self.active_tab;
                    // A snap answers for a point the pointer can stand on.
                    // Projected off the canvas, the aperture test runs against
                    // a pixel that no cursor can occupy and can answer with a
                    // snap somewhere else, so the query is refused instead.
                    let off_screen = {
                        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
                        if vw <= 1.0 || vh <= 1.0 {
                            false
                        } else {
                            let tile = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                            let bounds = iced::Rectangle {
                                x: 0.0,
                                y: 0.0,
                                width: tile.width,
                                height: tile.height,
                            };
                            self.tabs[i]
                                .scene
                                .camera
                                .borrow()
                                .project(world, bounds)
                                .is_none_or(|p| {
                                    p.x < 0.0
                                        || p.y < 0.0
                                        || p.x > bounds.width
                                        || p.y > bounds.height
                                })
                        }
                    };
                    if off_screen {
                        json!({"ok":true,"snap":null,"reason":"point_outside_viewport"})
                    } else {
                        match self.snap_query(i, world, from) {
                            Some(hit) => {
                                json!({"ok":true,"snap":format!("{:?}", hit.snap_type),"world":[hit.world.x,hit.world.y,hit.world.z]})
                            }
                            None => json!({"ok":true,"snap":null}),
                        }
                    }
                }
                "grips" => self.control_grips(),
                "keytips" => self
                    .ribbon
                    .keytips_json(req["detail"].as_str() == Some("full")),
                "xdata_get" => self.xdata_read(&req),
                "get_selection" => self.control_get_selection(),
                "text_search" => self.control_text_search(&req).unwrap_or_else(|e| e),
                "text_audit" => self.control_text_audit(&req).unwrap_or_else(|e| e),
                "blocks"
                | "block_instances"
                | "cad_get_blocks"
                | "cad_extract_block_instances" => self.control_blocks(&req).unwrap_or_else(|e| e),
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
                || matches!(
                    op,
                    "run" | "start" | "new" | "open" | "activate" | "switch_document"
                ))
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
        let doc = if matches!(op, "new" | "open" | "activate" | "switch_document") {
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
        // Any request may move the selection (`select`, `click`, `grip_drag`, a
        // command that picks for itself), so the client's staleness counter has
        // to be maintained on this shared boundary too. The publisher's
        // fingerprint guard keeps a request that touched nothing from minting a
        // revision of its own.
        #[cfg(not(target_arch = "wasm32"))]
        self.notify_plugins_selection_changed();
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
        if matches!(op, "new" | "activate" | "switch_document")
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
            // `capture` is parked the same way until the next drawn frame.
            if !matches!(
                p.request["op"].as_str(),
                Some("user_select" | "getpoint" | "capture")
            ) {
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

    /// Serve a read or semantic edit addressed to an open document that is
    /// not the visible tab (#2032): the request runs against that document's
    /// live scene (its undo history, revision and dirty state), and the
    /// visible tab, its command, selection and camera stay as they were. The
    /// document is only made current for the duration of this call, so the
    /// GUI never draws it.
    fn control_request_in_background(&mut self, id: u64, req: Value) -> (Value, Task<Message>) {
        let visible = self.tabs[self.active_tab].id;
        let Some(target) = self.tabs.iter().position(|t| t.id == id) else {
            return (failure("document_closed", "Document no longer exists"), Task::none());
        };
        self.active_tab = target;
        let (response, task) = self.control_request(req);
        if let Some(index) = self.tabs.iter().position(|t| t.id == visible) {
            self.active_tab = index;
        }
        // Panels the edit refreshed follow the visible document again.
        self.refresh_properties();
        self.sync_ribbon_layers();
        self.sync_ribbon_styles();
        (response, task)
    }

    fn control_action(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        Ok(match req["op"].as_str().unwrap_or("") {
            "new" => {
                if let Some(template) = req["template"].as_str() {
                    match self.apply_template(template) {
                        Ok(()) => {
                            self.set_control_result(json!({
                                "template": template,
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
            "font_register" => {
                let (name, bytes) = font_register_request(req)?;
                let (family, faces) =
                    crate::scene::text::sysfont::register_font_file(&name, &bytes)
                        .ok_or_else(|| failure("font_invalid", "data_base64 holds no font face"))?;
                // Same invalidation as a fetched web font: styles that fell
                // back while the file was missing lay out again.
                crate::scene::text::ttf_glyph::clear_fallback_cache();
                for tab in self.tabs.iter_mut() {
                    tab.scene.invalidate_text_geometry_dependencies();
                }
                self.set_control_result(json!({
                    "file_name": name,
                    "family": family,
                    "faces": faces,
                }));
                Task::none()
            }
            "activate" | "switch_document" => {
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
                        // `handles` hands the prompt a set directly: a
                        // selection-gathering command does not have to be fed
                        // through `op_select` first.
                        let handles = match req["handles"].as_array() {
                            Some(list) => {
                                let mut handles = Vec::with_capacity(list.len());
                                for v in list {
                                    let h = u64::from_str_radix(
                                        v.as_str().unwrap_or("").trim_start_matches("0x"),
                                        16,
                                    )
                                    .map(codec::Handle::new)
                                    .map_err(|_| {
                                        failure("invalid_handle", "Expected hexadecimal handle")
                                    })?;
                                    if self.tabs[i].scene.document.get_entity(h).is_none() {
                                        return Err(failure(
                                            "entity_absent",
                                            format!("Entity {} does not exist", v),
                                        ));
                                    }
                                    handles.push(h);
                                }
                                handles
                            }
                            None => self.tabs[i].scene.selected_handles_in_order(),
                        };
                        self.feed_command(StepInput::SelectionComplete(handles))
                    }
                    "enter" => self.feed_command(StepInput::Enter),
                    _ => return Err(failure("invalid_input", "Unknown input kind")),
                }
            }
            "cancel" => self.control_cancel(),
            "undo" => self.update(Message::Undo),
            "redo" => self.update(Message::Redo),
            "select" => {
                if let Some(hs) = req["handles"].as_array() {
                    for v in hs {
                        let h = u64::from_str_radix(
                            v.as_str().unwrap_or("").trim_start_matches("0x"),
                            16,
                        )
                        .map(codec::Handle::new)
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
            "grip_drag" => self.control_grip_drag(req)?,
            "click" => self.control_click(req)?,
            "pointer" => self.control_pointer(req)?,
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
            "node_graph" => self.control_node_graph(req)?,
            "entities_copy_to" => self.control_entities_copy_to(req)?,
            "text_replace" => self.control_text_replace(req)?,
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
                if let Some(refusal) = self.read_only_save_refusal() {
                    return Err(refusal);
                }
                if self.tabs[i].block_test.is_some() {
                    return Err(failure(
                        "temporary_document",
                        "Test Block is temporary and cannot be saved",
                    ));
                }
                let path = req["path"]
                    .as_str()
                    .map(std::path::PathBuf::from)
                    .or_else(|| self.tabs[i].current_path.clone())
                    .ok_or_else(|| failure("path_required", "Supply a save path"))?;
                let default_is_dxf = crate::io::source_is_dxf(
                    self.tabs[i].current_path.as_deref(),
                    &self.tabs[i].scene.document,
                );
                let (version, is_dxf) = super::automation::requested_save_target(
                    req,
                    self.tabs[i].scene.document.version,
                    default_is_dxf,
                    Some(&path),
                )
                .map_err(|error| failure("invalid_target", error))?;
                let dropped =
                    crate::io::dropped_on_save_count(&self.tabs[i].scene.document, version, is_dxf);
                if dropped > 0 && req["allow_lossy"].as_bool() != Some(true) {
                    return Err(failure(
                        "lossy_conversion_not_acknowledged",
                        format!(
                            "Conversion would drop {dropped} unsupported record(s); set allow_lossy=true to acknowledge"
                        ),
                    ));
                }
                self.set_control_result(json!({
                    "saved":path,
                    "target_format":super::automation::source_format(Some(&path), is_dxf),
                    "target_version":format!("{version:?}"),"dropped_on_save":dropped,
                }));
                if self.main_window.is_none() {
                    self.save_tab_synchronously_protected_as(i, path, version, true)
                        .map_err(|e| failure("save_failed", e))?;
                    Task::none()
                } else {
                    self.prepare_native_save(i);
                    // allow_lossy already acknowledged the drawing properties a
                    // DWG/DXF version drops; a script must not be stopped by that
                    // warning. Converter losses keep their own confirmation.
                    let converter = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| ocs_save::external_format(e).is_some());
                    self.conversion_loss_allowed =
                        !converter && req["allow_lossy"].as_bool() == Some(true);
                    self.queue_native_save(
                        i,
                        path,
                        version,
                        super::SavePurpose::SaveAs,
                        super::SaveContinuation::None,
                        true,
                        true,
                    )
                }
            }
            // Web: no filesystem path; the drawing comes back in the reply.
            #[cfg(target_arch = "wasm32")]
            "save" => {
                if self.tabs[i].block_test.is_some() {
                    return Err(failure(
                        "temporary_document",
                        "Test Block is temporary and cannot be saved",
                    ));
                }
                use base64::Engine as _;
                let default_is_dxf = crate::io::source_is_dxf(None, &self.tabs[i].scene.document);
                let (version, is_dxf) = super::automation::requested_save_target(
                    req,
                    self.tabs[i].scene.document.version,
                    default_is_dxf,
                    None,
                )
                .map_err(|error| failure("invalid_target", error))?;
                let dropped =
                    crate::io::dropped_on_save_count(&self.tabs[i].scene.document, version, is_dxf);
                if dropped > 0 && req["allow_lossy"].as_bool() != Some(true) {
                    return Err(failure(
                        "lossy_conversion_not_acknowledged",
                        format!(
                            "Conversion would drop {dropped} unsupported record(s); set allow_lossy=true to acknowledge"
                        ),
                    ));
                }
                let ext = if is_dxf { "dxf" } else { "dwg" };
                self.sync_view_state_for_save(i);
                super::update::util::sync_annotation_scale_header(&mut self.tabs[i].scene);
                self.stamp_header_sysvars(i);
                self.sync_solid_models_for_save(i);
                self.tabs[i].scene.sync_native_parametric_graph();
                let bytes = crate::io::save_to_bytes(&self.tabs[i].scene.document, ext, version)
                    .map_err(|error| failure("save_failed", error))?;
                self.tabs[i].dirty = false;
                let name = req["name"].as_str().filter(|name| !name.is_empty());
                let name = name.map_or_else(|| format!("drawing.{ext}"), str::to_string);
                self.set_control_result(json!({
                    "name": name,
                    "target_format": ext,
                    "target_version": format!("{version:?}"),
                    "dropped_on_save": dropped,
                    "bytes": bytes.len(),
                    "data_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
                }));
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            "save_verified" => {
                if let Some(refusal) = self.read_only_save_refusal() {
                    return Err(refusal);
                }
                let result = self.save_verified_request(req)?;
                self.set_control_result(result);
                Task::none()
            }
            "capture" => {
                if req["view"].as_str() == Some("region")
                    && req.get("bounds").and_then(Value::as_array).is_none()
                {
                    return Err(failure(
                        "invalid_request",
                        "view region needs bounds [min_x, min_y, max_x, max_y]",
                    ));
                }
                self.main_window
                    .ok_or_else(|| failure("gui_required", "Capture requires a GUI window"))?;
                string(req, "path")?;
                let was_minimized = crate::sys::restore_window_if_minimized();
                if was_minimized {
                    std::thread::sleep(std::time::Duration::from_millis(60));
                }

                // Optional framing / selection adjustments prior to snapshot:
                if let Some(handles) = req.get("highlight_handles").and_then(Value::as_array) {
                    let hex_handles: Vec<codec::Handle> = handles
                        .iter()
                        .filter_map(|v| v.as_str())
                        .filter_map(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                        .map(codec::Handle::new)
                        .collect();
                    if !hex_handles.is_empty() {
                        let scene = &mut self.tabs[self.active_tab].scene;
                        scene.deselect_all();
                        for h in hex_handles {
                            scene.select_entity(h, false);
                        }
                    }
                }
                if let Some(focus) = req.get("focus_handles").and_then(Value::as_array) {
                    let hex_handles: Vec<codec::Handle> = focus
                        .iter()
                        .filter_map(|v| v.as_str())
                        .filter_map(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                        .map(codec::Handle::new)
                        .collect();
                    if !hex_handles.is_empty() {
                        self.tabs[self.active_tab]
                            .scene
                            .zoom_to_entities(&hex_handles);
                    }
                } else if let Some(bounds) = req.get("bounds").and_then(Value::as_array) {
                    if bounds.len() >= 4 {
                        if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                            bounds[0].as_f64(),
                            bounds[1].as_f64(),
                            bounds[2].as_f64(),
                            bounds[3].as_f64(),
                        ) {
                            let p1 = glam::Vec3::new(x0 as f32, y0 as f32, 0.0);
                            let p2 = glam::Vec3::new(x1 as f32, y1 as f32, 0.0);
                            self.tabs[self.active_tab].scene.zoom_to_window(p1, p2);
                        }
                    }
                } else if let Some(view) = req.get("view").and_then(Value::as_str) {
                    if view == "extents" {
                        self.tabs[self.active_tab].scene.fit_all();
                    } else if view == "selection" {
                        let selected = self.tabs[self.active_tab].scene.selected_handles_in_order();
                        if !selected.is_empty() {
                            self.tabs[self.active_tab].scene.zoom_to_entities(&selected);
                        }
                    }
                }

                // C11: do not capture in the same batch. Park until the next
                // rendered frame (or the timer fallback) so geometry changes are
                // visible rather than returning a stale frame.
                self.control.capture_frame_pending = true;
                #[cfg(target_arch = "wasm32")]
                {
                    self.control.capture_deadline =
                        Some(iced::time::Instant::now() + std::time::Duration::from_secs(5));
                }
                Task::none()
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
        let track_zero_unit = self.control.pending.as_ref().is_some_and(|p| {
            p.request["op"] == "run"
                && p.request["cmd"].as_str().is_some_and(|cmd| {
                    matches!(cmd.trim().to_ascii_uppercase().as_str(), "SAVE" | "QSAVE")
                })
        });
        // Most zero-unit tasks are `Task::none()` and have already completed
        // synchronously. SAVE/QSAVE are the deliberate exception: the command
        // dispatcher returns a zero-unit Task::done(Message::SaveFile), and
        // the operation must wait for that message before reporting its state.
        if task.units() == 0 && !track_zero_unit {
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
                    Some("new" | "open" | "activate" | "switch_document")
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
        self.control_settle_inner(false);
    }

    /// `force` closes the request even while an open or a save it may have
    /// started is still running.
    fn control_settle_inner(&mut self, force: bool) {
        if self.control.pending.as_ref().is_none_or(|p| p.pending != 0) {
            return;
        }
        if !force && self.opening.is_some() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !force
            && (!self.active_save_jobs.is_empty() || self.pending_native_thumbnail_save.is_some())
        {
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
        crate::ui::ribbon::keytip_note_command_finished();
    }
    pub(super) fn control_measure(&self, req: &Value) -> Value {
        let tab = &self.tabs[self.active_tab];
        let requested: Vec<codec::Handle> = req["handles"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .map(codec::Handle::new)
                    .collect()
            })
            .unwrap_or_else(|| tab.scene.selected_handles_in_order());
        let mut out = Vec::new();
        for handle in requested {
            let Some(entity) = tab.scene.document.get_entity(handle) else {
                continue;
            };
            let (min, max) =
                crate::scene::convert::tess::entity_bounds_in(&tab.scene.document, entity);
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
                        crate::entities::solid3d::clamp_header_isolines(h.isolines),
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
        let waited_for_frame = self.control.capture_after_redraw;
        let result = (|| -> Result<Value, String> {
            let s = screenshot
                .ok_or("The window is minimized or has no size; restore it and capture again")?;
            let requested_scope = self
                .control
                .pending
                .as_ref()
                .and_then(|pending| pending.request["scope"].as_str())
                .unwrap_or("viewport");
            let max_dimension = self
                .control
                .pending
                .as_ref()
                .and_then(|pending| pending.request["max_dimension"].as_u64())
                .unwrap_or(1600)
                .clamp(256, 4096) as u32;
            let annotate = self
                .control
                .pending
                .as_ref()
                .and_then(|pending| pending.request["annotate"].as_bool())
                .unwrap_or(false);

            let mut image =
                image::RgbaImage::from_raw(s.size.width, s.size.height, s.rgba.to_vec())
                    .ok_or("Renderer returned malformed image data")?;
            let mut actual_scope = "window";

            let (vp_w, vp_h) = self.tabs[self.active_tab]
                .scene
                .selection
                .borrow()
                .view
                .vp_size;
            let mut vp_logical_width = if vp_w > 0.0 {
                vp_w
            } else {
                image.width() as f32 / s.scale_factor
            };
            let mut vp_logical_height = if vp_h > 0.0 {
                vp_h
            } else {
                image.height() as f32 / s.scale_factor
            };

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
                        vp_logical_width = bounds.width;
                        vp_logical_height = bounds.height;
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

            // Spatial grounding: camera, world bounds, and visible entity centroids
            let grounding = vision::compute_grounding(
                self,
                vp_logical_width,
                vp_logical_height,
                image.width(),
                image.height(),
            );

            // Optional Set-of-Marks visual overlay badges
            if annotate {
                for ent_val in &grounding.visible_entities {
                    if let (Some(tag), Some(px_arr), Some(selected)) = (
                        ent_val["tag"].as_u64(),
                        ent_val["screen_pixel"].as_array(),
                        ent_val["selected"].as_bool(),
                    ) {
                        if px_arr.len() == 2 {
                            if let (Some(x), Some(y)) = (px_arr[0].as_i64(), px_arr[1].as_i64()) {
                                vision::draw_som_badge(
                                    &mut image,
                                    x as i32,
                                    y as i32,
                                    tag as usize,
                                    selected,
                                );
                            }
                        }
                    }
                }
            }

            #[cfg(not(target_arch = "wasm32"))]
            image
                .save_with_format(&path, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            // No filesystem behind a web "path": encode the PNG in memory and
            // hand it to the browser as a download named after the request's
            // file name — the delivery every other web output uses.
            #[cfg(target_arch = "wasm32")]
            {
                let mut png = Vec::new();
                image
                    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                    .map_err(|e| e.to_string())?;
                let download = path
                    .rsplit(['/', '\\'])
                    .next()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("capture.png")
                    .to_string();
                crate::sys::download_bytes(&download, &png);
            }

            let doc_unit = crate::app::properties::insunits_name(
                self.tabs[self.active_tab]
                    .scene
                    .document
                    .header
                    .insertion_units,
            );

            let spatial_obj = json!({
                "crs": "CAD_WCS",
                "unit": doc_unit,
                "pixel_resolution": [image.width(), image.height()],
                "world_bounds": grounding.viewport_world_bounds,
                "pixel_to_world_matrix": grounding.pixel_to_world_matrix,
                "world_to_pixel_matrix": grounding.world_to_pixel_matrix,
                "camera": grounding.camera,
                "target_plane": grounding.target_plane,
                "annotations": grounding.visible_entities.clone(),
            });

            Ok(json!({
                "path": path,
                "scope": actual_scope,
                "width": image.width(),
                "height": image.height(),
                "scale_factor": s.scale_factor,
                "waited_for_frame": waited_for_frame,
                "document_id": self.tabs[self.active_tab].id,
                "revision": self.tabs[self.active_tab].edit_revision,
                "camera_revision": self.tabs[self.active_tab].scene.camera_generation,
                "_spatial": spatial_obj,
                "pixel_to_world_matrix": grounding.pixel_to_world_matrix,
                "world_to_pixel_matrix": grounding.world_to_pixel_matrix,
                "viewport_world_bounds": grounding.viewport_world_bounds,
                "camera": grounding.camera,
                "annotated": annotate,
                "annotations": grounding.visible_entities.clone(),
                "visible_entities": grounding.visible_entities,
            }))
        })();
        match result {
            Ok(v) => {
                if let Some(p) = self.control.pending.as_mut() {
                    p.result = v;
                }
            }
            Err(e) => self.command_line.push_error(&e),
        }
        // `capture` is parked until this result (or error) arrives, rather
        // than being settled by the task counter used for ordinary actions.
        if let Some(p) = self
            .control
            .pending
            .as_mut()
            .filter(|p| p.request["op"] == "capture")
        {
            p.pending = 0;
        }
        self.control.capture_after_redraw = false;
        self.control_settle();
    }

    /// A frame finished rendering (or the anti-hang timer expired): start the
    /// screenshot only after the capture request has been parked.
    pub(super) fn on_control_capture_frame(&mut self, redrew: bool) -> Task<Message> {
        if !self.control.capture_frame_pending {
            return Task::none();
        }
        let Some(path) = self
            .control
            .pending
            .as_ref()
            .filter(|p| p.request["op"] == "capture")
            .and_then(|p| p.request["path"].as_str())
            .map(str::to_owned)
        else {
            self.control.capture_frame_pending = false;
            return Task::none();
        };
        // Web: iced's window screenshot needs a synchronous GPU map wasm
        // cannot provide; the canvas readback (the save-thumbnail technique)
        // only sees the drawing right after a presented frame, before the
        // browser clears the WebGL buffer — so wait for a real frame.
        #[cfg(target_arch = "wasm32")]
        {
            let expired = self
                .control
                .capture_deadline
                .is_some_and(|deadline| iced::time::Instant::now() >= deadline);
            if !redrew && !expired && crate::sys::page_visible() {
                return Task::none();
            }
            self.control.capture_frame_pending = false;
            self.control.capture_deadline = None;
            self.control.capture_after_redraw = redrew;
            // No frame to read (hidden tab or nothing redrawing): report the
            // capture as failed rather than hand back a blank canvas.
            let screenshot = if redrew {
                crate::sys::capture_canvas()
            } else {
                None
            };
            Task::done(Message::ControlScreenshot(path, screenshot))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.control.capture_frame_pending = false;
            self.control.capture_after_redraw = redrew;
            let Some(window) = self.main_window else {
                return Task::done(Message::ControlScreenshot(path, None));
            };
            // A minimized window has a 0x0 surface and the renderer panics
            // reading it back, so report instead of capturing.
            iced::window::size(window).then(move |size| {
                let path = path.clone();
                if size.width <= 0.0 || size.height <= 0.0 {
                    Task::done(Message::ControlScreenshot(path, None))
                } else {
                    iced::window::screenshot(window).map(move |screenshot| {
                        Message::ControlScreenshot(path.clone(), Some(screenshot))
                    })
                }
            })
        }
    }
}
mod actions;
pub(crate) mod vision;
pub(crate) use actions::{property_json, row_id};
mod entities;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod http_bridge;
mod interactive;
mod node_graph;
mod sheets;
pub(crate) use sheets::new_guid_v4;
pub(crate) mod block_ops;
pub(crate) mod spellcheck;
pub(crate) mod text_ops;

#[cfg(test)]
mod p1_tests;
pub(super) fn action_names() -> &'static [&'static str] {
    actions::NAMES
}

pub(super) fn controlled_open_pending(app: &OpenCADStudio) -> bool {
    app.control
        .pending
        .as_ref()
        .is_some_and(|pending| pending.request["op"] == "open")
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
    fn edits_reach_an_inactive_document_without_switching_tabs() {
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
        let target = app.tabs[app.active_tab].id;
        assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
        let visible = app.active_tab;
        let revision = |app: &OpenCADStudio| {
            app.tabs.iter().find(|t| t.id == target).unwrap().edit_revision
        };
        let send = |app: &mut OpenCADStudio, mut req: Value| {
            req["protocol"] = json!(1);
            req["document_id"] = json!(target);
            req["revision"] = json!(revision(app));
            req["client_id"] = json!("test");
            req["request_id"] = json!(format!("bg-{}", app.control.serial));
            app.control_request(req).0
        };
        let created = send(
            &mut app,
            json!({"op":"entities_create","entities":[{"type":"Line","start":[0,0],"end":[5,0]}]}),
        );
        assert_eq!(created["ok"], true, "{created}");
        assert_eq!(app.active_tab, visible, "the visible tab must not change");
        let count = |app: &OpenCADStudio, id: u64| {
            let tab = app.tabs.iter().find(|t| t.id == id).unwrap();
            tab.scene.document.entities().count()
        };
        assert_eq!(count(&app, target), 1);
        assert_eq!(count(&app, app.tabs[visible].id), 0);
        let records = send(&mut app, json!({"op":"entities"}));
        assert_eq!(records["ok"], true, "{records}");
        assert_eq!(send(&mut app, json!({"op":"undo"}))["ok"], true);
        assert_eq!(count(&app, target), 0);
        assert_eq!(app.active_tab, visible);
        let run = send(&mut app, json!({"op":"run","cmd":"LINE 0,0 1,1"}));
        assert_eq!(run["code"], "document_not_active", "{run}");
    }

    #[test]
    fn qsave_completion_reports_clean_document_state() {
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
        assert_eq!(
            request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}))["status"],
            "completed"
        );
        let path = std::env::temp_dir().join(format!(
            "ocs_qsave_state_{}_{}.dwg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let _ = std::fs::remove_file(&path);
        let saved = request(&mut app, json!({"op":"save","path":path,"overwrite":true}));
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(
            request(&mut app, json!({"op":"run","cmd":"LINE 0,5 10,5"}))["status"],
            "completed"
        );
        let qsave = request(&mut app, json!({"op":"run","cmd":"QSAVE"}));
        assert_eq!(qsave["status"], "completed", "{qsave}");
        let document_id = qsave["state"]["document_id"].as_u64().unwrap();
        let dirty = qsave["state"]["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|document| document["id"].as_u64() == Some(document_id))
            .and_then(|document| document["dirty"].as_bool());
        assert_eq!(dirty, Some(false), "{qsave}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn qsave_save_failure_settles_instead_of_hanging() {
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
        assert_eq!(
            request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}))["status"],
            "completed"
        );
        let dir = std::env::temp_dir().join(format!(
            "ocs_qsave_fail_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("blocked.dwg");
        let saved = request(&mut app, json!({"op":"save","path":path,"overwrite":true}));
        assert_eq!(saved["ok"], true, "{saved}");
        // A directory at the target path makes the next write fail
        // deterministically; the operation must settle as failed instead of
        // waiting forever on its task counter.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            request(&mut app, json!({"op":"run","cmd":"LINE 0,5 10,5"}))["status"],
            "completed"
        );
        let qsave = request(&mut app, json!({"op":"run","cmd":"QSAVE"}));
        assert_eq!(qsave["status"], "failed", "{qsave}");
        assert!(
            qsave["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty()),
            "{qsave}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windowed_save_with_allow_lossy_does_not_wait_for_the_loss_dialog() {
        // DWG R2000 cannot store drawing properties; a script that acknowledged
        // losses with allow_lossy must not be parked behind the confirmation.
        for allow_lossy in [false, true] {
            let mut app = OpenCADStudio::new_for_test();
            app.main_window = Some(iced::window::Id::unique());
            assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
            let i = app.active_tab;
            app.tabs[i].scene.document.summary_info.custom_properties =
                vec![("Client".into(), "ACME".into())];
            let path = std::env::temp_dir().join(format!(
                "ocs_allow_lossy_{}_{allow_lossy}.dwg",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            let _ = request(
                &mut app,
                json!({"op":"save","path":path,"overwrite":true,
                       "target_version":"2000","allow_lossy":allow_lossy}),
            );
            assert_eq!(
                app.active_modal == Some(crate::app::ModalKind::ConversionLoss),
                !allow_lossy,
                "allow_lossy={allow_lossy}"
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn windowed_save_with_allow_lossy_still_asks_before_a_converter_loses_content() {
        // allow_lossy only waives the drawing-properties warning; a converter
        // target keeps its parked loss prompt.
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
        let i = app.active_tab;
        app.tabs[i].scene.document.summary_info.custom_properties =
            vec![("Client".into(), "ACME".into())];
        // IFCX has no ray: a real converter loss.
        app.tabs[i]
            .scene
            .document
            .add_entity(codec::entities::EntityType::Ray(codec::entities::Ray::new(
                codec::types::Vector3::ZERO,
                codec::types::Vector3::UNIT_X,
            )))
            .unwrap();
        let path = std::env::temp_dir().join(format!(
            "ocs_allow_lossy_converter_{}.ifcx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let _ = request(
            &mut app,
            json!({"op":"save","path":path,"overwrite":true,"allow_lossy":true}),
        );
        let asked = app.active_modal == Some(crate::app::ModalKind::ConversionLoss);
        let _ = std::fs::remove_file(&path);
        assert!(asked, "converter target must still ask");
    }

    #[test]
    fn capture_parks_for_the_next_frame_then_finishes_with_its_result() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
        let path = std::env::temp_dir().join(format!("ocs-capture-frame-{}.png", session_id()));
        let state = app.control_state();
        let parked = app
            .control_request(json!({
                "protocol":1,
                "op":"capture",
                "request_id":"cap-frame",
                "document_id": state["document_id"],
                "revision": state["revision"],
                "path": path.to_string_lossy(),
            }))
            .0;
        assert_eq!(parked["status"], "accepted", "{parked}");
        assert!(
            app.control.capture_frame_pending,
            "capture should wait for a frame"
        );
        let running = app
            .control_request(json!({"op":"operation","request_id":"cap-frame"}))
            .0;
        assert_eq!(running["status"], "running", "{running}");

        let _frame_task = app.on_control_capture_frame(true);
        assert!(!app.control.capture_frame_pending);
        let screenshot =
            iced::window::Screenshot::new(vec![0u8; 4 * 4 * 4], iced::Size::new(4, 4), 1.0);
        app.control_screenshot(path.to_string_lossy().into_owned(), Some(screenshot));
        let done = app
            .control_request(json!({"op":"operation","request_id":"cap-frame"}))
            .0;
        assert_eq!(done["status"], "completed", "{done}");
        assert_eq!(done["result"]["waited_for_frame"], true, "{done}");
        assert_eq!(done["result"]["width"], 4, "{done}");
        assert!(path.exists(), "capture should be written");
        let _ = std::fs::remove_file(&path);
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
        assert!(started["state"]["command"]["accepts"]
            .as_array()
            .unwrap()
            .contains(&json!("point")));
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
    fn control_dynamic_text_reads_back_in_every_length_format() {
        use crate::app::block_parameter_properties::parse_value;
        use crate::entities::common::{set_unit_context, UnitContext};

        set_unit_context(UnitContext {
            lunits: 5,
            luprec: 4,
            ..Default::default()
        });
        assert_eq!(parse_value("5 3/4"), Some(5.75));
        set_unit_context(UnitContext {
            lunits: 1,
            luprec: 2,
            ..Default::default()
        });
        assert!((parse_value("1.25e+06").unwrap() - 1_250_000.).abs() < 1e-6);
        assert_eq!(parse_value("10+2"), Some(12.0));
    }

    #[test]
    fn control_property_reads_dynamic_rows_as_scalars_and_sets_them_by_id() {
        use crate::scene::block_actions as actions;
        use crate::scene::block_distance_parameter as distance;
        use codec::entities::{Insert, Line};
        use codec::types::Vector3;
        use codec::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
        let i = app.active_tab;
        let scene = &mut app.tabs[i].scene;
        scene.define_block_raw(
            "DYN_PROPS",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_X,
            ))],
        );
        let definition = scene.document.block_records.get("DYN_PROPS").unwrap().handle;
        let member = scene.document.block_records.get("DYN_PROPS").unwrap().entity_handles[0];
        let doc = &mut scene.document;
        let parameter = distance::create(
            doc,
            definition,
            distance::Kind::XY,
            Vector3::ZERO,
            Vector3::new(10., 20., 0.),
            Vector3::new(10., 20., 0.),
        )
        .unwrap();
        actions::create(
            doc,
            definition,
            actions::Kind::Move,
            parameter,
            &[member],
            glam::DVec3::ZERO,
        )
        .unwrap();
        let reference = scene.add_entity(EntityType::Insert(Insert::new(
            "DYN_PROPS",
            Vector3::ZERO,
        )));
        assert_eq!(
            request(&mut app, json!({"op":"select","type":"INSERT"}))["ok"],
            true
        );
        assert_eq!(app.tabs[i].selected_handle, Some(reference));

        let rows = |app: &mut OpenCADStudio| {
            let read = request(app, json!({"op":"properties"}));
            assert_eq!(read["ok"], true, "{read}");
            let custom = read["sections"]
                .as_array()
                .unwrap()
                .iter()
                .find(|section| section["title"] == "Custom")
                .unwrap_or_else(|| panic!("no Custom group: {read}"));
            custom["properties"].as_array().unwrap().clone()
        };
        let custom = rows(&mut app);
        let numeric: Vec<_> = custom
            .iter()
            .filter(|row| row["kind"] == "number")
            .collect();
        assert_eq!(numeric.len(), 2, "{custom:?}");
        let mut ids = std::collections::BTreeSet::new();
        for row in &numeric {
            let value = row["value"]
                .as_str()
                .unwrap_or_else(|| panic!("scalar value expected: {row}"));
            assert!(value.parse::<f64>().is_ok(), "{row}");
            assert!(
                !value.contains("BlockParameterKey"),
                "debug text reached the value: {row}"
            );
            let id = row["id"]
                .as_str()
                .unwrap_or_else(|| panic!("id expected: {row}"));
            assert!(id.starts_with("dyn_"), "dynamic id expected: {row}");
            assert!(ids.insert(id.to_string()), "duplicate id {id}: {custom:?}");
            assert!(row["component"].is_string(), "{row}");
            assert!(row["parameter"].is_string(), "{row}");
        }
        let x = numeric
            .iter()
            .copied()
            .find(|row| row["component"] == "X")
            .expect("X component row")
            .clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let changed = request(
            &mut app,
            json!({"op":"property","field":x["id"],"value":3}),
        );
        assert_eq!(changed["ok"], true, "{changed}");
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        let after = rows(&mut app);
        let value = after
            .iter()
            .find(|row| row["id"] == x["id"])
            .and_then(|row| row["value"].as_str())
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap();
        assert!((value - 3.0).abs() < 1e-6, "{value} {after:?}");
        let other = after
            .iter()
            .find(|row| row["id"] != x["id"] && row["kind"] == "number")
            .and_then(|row| row["value"].as_str())
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap();
        assert!((other - 20.0).abs() < 1e-6, "{other}");

        // The text a row prints is accepted back unchanged, even when the
        // drawing writes lengths as fractions.
        app.tabs[i].scene.document.header.linear_unit_format = 5;
        app.tabs[i].scene.document.header.linear_unit_precision = 4;
        let set = request(
            &mut app,
            json!({"op":"property","field":x["id"],"value":5.75}),
        );
        assert_eq!(set["ok"], true, "{set}");
        let printed = rows(&mut app)
            .iter()
            .find(|row| row["id"] == x["id"])
            .and_then(|row| row["value"].as_str())
            .map(str::to_owned)
            .unwrap();
        assert_eq!(printed, "5 3/4");
        let echoed = request(
            &mut app,
            json!({"op":"property","field":x["id"],"value":printed}),
        );
        assert_eq!(echoed["ok"], true, "{echoed}");
        let kept = rows(&mut app)
            .iter()
            .find(|row| row["id"] == x["id"])
            .cloned()
            .unwrap();
        assert_eq!(kept["value"], "5 3/4");
    }

    #[test]
    fn control_property_sets_dynamic_visibility_with_tooltip() {
        use crate::scene::block_visibility;
        use codec::entities::{Insert, Line};
        use codec::types::Vector3;
        use codec::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["ok"], true);
        let i = app.active_tab;
        let scene = &mut app.tabs[i].scene;
        scene.define_block_raw(
            "VISIBILITY_TEST",
            Vector3::ZERO,
            vec![
                EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 5.0, 0.0, 0.0)),
                EntityType::Line(Line::from_coords(0.0, 3.0, 0.0, 5.0, 3.0, 0.0)),
            ],
        );
        let definition = scene.document.block_records.get("VISIBILITY_TEST").unwrap();
        let owner = definition.handle;
        let first = definition.entity_handles[0];
        let second = definition.entity_handles[1];
        let parameter =
            block_visibility::create(&mut scene.document, owner, Vector3::ZERO).unwrap();
        let other =
            block_visibility::add_state(&mut scene.document, parameter, "Other", 0).unwrap();
        assert_eq!(other, 1);
        block_visibility::set_visible(&mut scene.document, parameter, 0, &[second], false);
        block_visibility::set_visible(&mut scene.document, parameter, other, &[first], false);
        block_visibility::finish_editing(&mut scene.document, owner);
        let reference = scene.add_entity(EntityType::Insert(Insert::new(
            "VISIBILITY_TEST",
            Vector3::ZERO,
        )));
        let selected = request(&mut app, json!({"op":"select","type":"INSERT"}));
        assert_eq!(selected["ok"], true, "{selected}");
        assert_eq!(app.tabs[i].selected_handle, Some(reference));
        let depth = app.tabs[i].history.undo_stack.len();

        let changed = request(
            &mut app,
            json!({"op":"property","field":"dyn_visibility","value":"Other"}),
        );
        assert_eq!(changed["ok"], true, "{changed}");
        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(insert) = doc.get_entity(reference).unwrap() else {
            panic!("expected the block reference");
        };
        assert_ne!(insert.block_name, "VISIBILITY_TEST");
        let visible: Vec<_> = doc
            .entities_in_block(&insert.block_name)
            .filter(|entity| !entity.common().invisible)
            .collect();
        assert_eq!(visible.len(), 1);
        let EntityType::Line(line) = visible[0] else {
            panic!("expected the alternative line");
        };
        assert_eq!(line.start.y, 3.0);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        let row = OpenCADStudio::visibility_property(doc, reference).unwrap();
        let row = property_json(&row);
        assert_eq!(row["kind"], "choice");
        assert_eq!(row["value"], "Other");
        assert_eq!(row["options"], json!(["VisibilityState0", "Other"]));

        let invalid = request(
            &mut app,
            json!({"op":"property","field":"dyn_visibility","value":"Missing"}),
        );
        assert_eq!(invalid["code"], "invalid_choice", "{invalid}");
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        assert_eq!(request(&mut app, json!({"op":"undo"}))["ok"], true);
        let EntityType::Insert(insert) = app.tabs[i].scene.document.get_entity(reference).unwrap()
        else {
            panic!("expected the restored block reference");
        };
        assert_eq!(insert.block_name, "VISIBILITY_TEST");
        let row =
            OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference).unwrap();
        assert_eq!(property_json(&row)["value"], "VisibilityState0");
    }
    #[test]
    fn selection_changes_are_published_to_the_event_stream() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        let handle = request(&mut app, json!({"op":"query","type":"Line"}))["entities"][0]
            ["handle"]
            .as_str()
            .unwrap()
            .to_string();
        let before = app.control_state()["selection_revision"].as_u64().unwrap();
        let selected = app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
        assert_eq!(selected["ok"], true, "{selected}");
        let state = app.control_state();
        assert_eq!(
            state["selection_revision"].as_u64().unwrap(),
            before + 1,
            "selection revision should increment once: {state}"
        );
        assert!(
            state["selection"]
                .as_array()
                .unwrap()
                .contains(&json!(handle)),
            "{state}"
        );
        let events = app.control_request(json!({"op":"events"})).0;
        let last = events["events"].as_array().unwrap().last().unwrap();
        assert_eq!(last["kind"], "selection", "{events}");
        assert_eq!(last["document_id"], state["document_id"], "{events}");
        assert!(
            last["selection"]
                .as_array()
                .unwrap()
                .contains(&json!(handle)),
            "{events}"
        );
    }

    /// The counter is the only staleness signal a client has: it has to move on
    /// every selection change made through the control surface, not only on the
    /// ones that happened to pass through `automation_op`'s wrapper.
    #[test]
    fn a_control_select_moves_the_selection_revision_when_the_set_changes() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,5 10,5"}));
        let lines: Vec<String> = request(&mut app, json!({"op":"query","type":"Line"}))["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["handle"].as_str().unwrap().to_string())
            .collect();
        let revision =
            |app: &OpenCADStudio| app.control_state()["selection_revision"].as_u64().unwrap();

        let before = revision(&app);
        request(&mut app, json!({"op":"select","handles":[&lines[0]]}));
        let first = revision(&app);
        assert!(first > before, "the first select left the counter at {first}");

        request(&mut app, json!({"op":"select","handles":[&lines[1]]}));
        let second = revision(&app);
        assert!(
            second > first,
            "the second select left the counter at {second}"
        );

        request(&mut app, json!({"op":"select","clear":true}));
        assert!(
            revision(&app) > second,
            "clearing the selection left the counter at {}",
            revision(&app)
        );
    }

    /// A selection prompt used to take its set only from the scene selection:
    /// a caller had to select first, then say "selection", and had no way to
    /// name the handles it meant. `handles` on the input request closes that.
    #[test]
    fn selection_input_can_name_the_handles_the_prompt_should_receive() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        let handle = request(&mut app, json!({"op":"query","type":"Line"}))["entities"][0]
            ["handle"]
            .as_str()
            .unwrap()
            .to_string();

        request(&mut app, json!({"op":"run","cmd":"ALIGN"}));
        let malformed = request(
            &mut app,
            json!({"op":"input","kind":"selection","handles":["NOTHEX"]}),
        );
        assert_eq!(malformed["ok"], false, "{malformed}");
        assert_eq!(malformed["code"], "invalid_handle", "{malformed}");
        let absent = request(
            &mut app,
            json!({"op":"input","kind":"selection","handles":["FFFF"]}),
        );
        assert_eq!(absent["ok"], false, "{absent}");
        assert_eq!(absent["code"], "entity_absent", "{absent}");

        let fed = request(
            &mut app,
            json!({"op":"input","kind":"selection","handles":[&handle]}),
        );
        assert_eq!(fed["ok"], true, "{fed}");
        request(&mut app, json!({"op":"input","kind":"enter"}));
        request(&mut app, json!({"op":"input","kind":"point","point":[0,0,0]}));
        request(&mut app, json!({"op":"input","kind":"point","point":[5,5,0]}));
        let applied = request(&mut app, json!({"op":"input","kind":"enter"}));
        assert_eq!(applied["ok"], true, "{applied}");

        let document = &app.tabs[app.active_tab].scene.document;
        let codec::EntityType::Line(line) = document
            .get_entity(codec::Handle::new(u64::from_str_radix(&handle, 16).unwrap()))
            .unwrap()
        else {
            panic!("expected the line");
        };
        assert_eq!(
            line.start,
            codec::types::Vector3::new(5.0, 5.0, 0.0),
            "the alignment translated the named handle, not the scene selection"
        );
    }

    /// A pick with no command running applies the two rules a pick has: a
    /// plain click adds to the set, Shift-click takes away. The audit
    /// recorded Shift-click as "not additive" and a plain click as
    /// replacing the set — the first is what Shift means, the second would
    /// be a defect, so both are pinned here.
    #[test]
    fn plain_clicks_accumulate_and_shift_click_removes() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        for y in [0.0, 4.0, 8.0] {
            request(&mut app, json!({"op":"run","cmd":format!("LINE 0,{y} 10,{y}")}));
        }
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600.0, 900.0);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-2.0, -2.0, 0.0),
            glam::DVec3::new(12.0, 10.0, 0.0),
            1600.0 / 900.0,
        );
        let click = |app: &mut OpenCADStudio, y: f64, modifiers: &str| {
            let r = request(
                app,
                json!({"op":"click","point":[2.0,y,0.0],"modifiers":modifiers}),
            );
            assert_eq!(r["ok"], true, "{r}");
            app.tabs[app.active_tab].scene.selected_handles_in_order().len()
        };
        assert_eq!(click(&mut app, 0.0, ""), 1, "the first plain click picks");
        assert_eq!(click(&mut app, 4.0, ""), 2, "a second plain click adds");
        assert_eq!(click(&mut app, 0.0, "shift"), 1, "Shift-click takes away");
        assert_eq!(
            click(&mut app, 8.0, "shift"),
            1,
            "Shift-click on an unselected line leaves the set alone"
        );
        assert_eq!(click(&mut app, 8.0, ""), 2, "the set accumulates afterwards");

        // PICKADD 0 swaps the plain pick's rule to replace, and Shift becomes
        // the toggle that puts an unselected object into the set.
        app.pick_add = false;
        assert_eq!(click(&mut app, 0.0, ""), 1, "PICKADD 0 replaces");
        assert_eq!(
            click(&mut app, 4.0, "shift"),
            2,
            "PICKADD 0 Shift-click adds an unselected line"
        );
        assert_eq!(click(&mut app, 4.0, "shift"), 1, "and the next one takes it back");
    }

    /// A region capture frames the camera from `bounds`; without them there is
    /// nothing to frame and the frame silently stays where the last one left
    /// it, which reads as "bounds were ignored". The shape is refused up front,
    /// before the GUI check, so a headless caller gets the reason too.
    #[test]
    fn a_region_capture_without_bounds_is_refused_by_name() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let r = request(&mut app, json!({"op":"capture","path":"x.png","view":"region"}));
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!(r["code"], "invalid_request", "{r}");
        let r = request(
            &mut app,
            json!({"op":"capture","path":"x.png","view":"region","bounds":[-1,-1,1,1]}),
        );
        assert_eq!(r["code"], "gui_required", "well-formed, only the window is missing: {r}");
    }

    /// CPCONSTRAINT's point pick resolves against the addressable points in
    /// the drawing, exactly: a line endpoint fed as a world point is taken,
    /// while the centre of the very circle being constrained is not (the
    /// target is excluded from its own pick, and nothing else is there). The
    /// audit read the second case as "a snap the drawing plainly has is
    /// invisible" after feeding the circle's own centre.
    #[test]
    fn a_center_point_constraint_resolves_an_exact_endpoint_and_refuses_the_targets_own_centre() {
        use crate::scene::parametric_constraints::ParametricScope;
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        request(&mut app, json!({"op":"run","cmd":"CIRCLE 5,5 2"}));
        let circle =
            request(&mut app, json!({"op":"query","type":"Circle"}))["entities"][0]["handle"]
                .as_str()
                .unwrap()
                .to_string();
        let constraints = |app: &OpenCADStudio| {
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map_or(0, |set| set.constraints.len())
        };

        request(&mut app, json!({"op":"run","cmd":"CPCONSTRAINT"}));
        request(&mut app, json!({"op":"input","kind":"selection","handles":[&circle]}));
        request(&mut app, json!({"op":"input","kind":"enter"}));
        let r = request(&mut app, json!({"op":"input","kind":"point","point":[5,5,0]}));
        assert_eq!(r["ok"], false, "{r}");
        assert!(
            r["error"].as_str().unwrap_or("").contains("didn't land on a point"),
            "{r}"
        );
        assert_eq!(constraints(&app), 0, "the target's own centre is not a point pick");

        request(&mut app, json!({"op":"run","cmd":"CPCONSTRAINT"}));
        request(&mut app, json!({"op":"input","kind":"selection","handles":[&circle]}));
        request(&mut app, json!({"op":"input","kind":"enter"}));
        let r = request(&mut app, json!({"op":"input","kind":"point","point":[0,0,0]}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            constraints(&app),
            1,
            "the line's endpoint is a point pick; history: {:?}",
            app.command_line
                .history
                .iter()
                .map(|e| e.text.clone())
                .collect::<Vec<_>>()
        );
    }

    /// A snap is a cursor measurement: it answers for a point the pointer can
    /// stand on. Asked about a point outside the viewport it used to run the
    /// aperture test against a pixel far off the canvas and answer with a
    /// snap somewhere else entirely — the live session answered `Center` at
    /// `(0,0)` for a query at `(5,0)`. It now says the point is not in view.
    #[test]
    fn a_snap_outside_the_viewport_is_reported_as_such() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        request(&mut app, json!({"op":"run","cmd":"CIRCLE 0,0 5"}));
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1200.0, 900.0);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-1.0, -1.0, 0.0),
            glam::DVec3::new(1.0, 1.0, 0.0),
            1200.0 / 900.0,
        );

        let r = request(&mut app, json!({"op":"snap","point":[5.0,0.0,0.0]}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["snap"], json!(null), "no measurement off-screen: {r}");
        assert_eq!(r["reason"], "point_outside_viewport", "{r}");

        let r = request(&mut app, json!({"op":"snap","point":[0.0,0.0,0.0]}));
        assert_ne!(r["snap"], json!(null), "in-view snaps still answer: {r}");
        assert_eq!(r.get("reason"), None, "{r}");
    }

    /// EDCONSTRAINT takes four point picks and only resolves them on the
    /// fourth, so a point that "is accepted" at picks one to three has not
    /// been judged yet. Exact endpoints go through; a point that is not an
    /// addressable point is refused when the fourth arrives.
    #[test]
    fn equal_distance_judges_its_points_on_the_fourth_pick() {
        use crate::scene::parametric_constraints::ParametricScope;
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,5 10,5"}));
        let constraints = |app: &OpenCADStudio| {
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map_or(0, |set| set.constraints.len())
        };

        request(&mut app, json!({"op":"run","cmd":"EDCONSTRAINT"}));
        for point in [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 5.0, 0.0]] {
            let r = request(&mut app, json!({"op":"input","kind":"point","point":point}));
            assert_eq!(r["ok"], true, "picks 1-3 are not judged yet: {r}");
        }
        assert_eq!(constraints(&app), 0, "nothing is judged before the fourth pick");
        let r = request(&mut app, json!({"op":"input","kind":"point","point":[10.0,5.0,0.0]}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(constraints(&app), 1, "four exact endpoints make the constraint");

        request(&mut app, json!({"op":"run","cmd":"EDCONSTRAINT"}));
        for point in [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 5.0, 0.0]] {
            request(&mut app, json!({"op":"input","kind":"point","point":point}));
        }
        let r = request(&mut app, json!({"op":"input","kind":"point","point":[5.0,2.0,0.0]}));
        assert_eq!(r["ok"], false, "a point on nothing is refused at the fourth: {r}");
        assert_eq!(constraints(&app), 1);
    }

    /// A constraint handed a live selection set applies to it. The umbrella
    /// dispatches the type while the picks are still in place, so `Equal`
    /// must take the set rather than drop it and ask for first and second
    /// objects again; with nothing selected it still opens its own prompt.
    #[test]
    fn equal_constraint_takes_a_live_selection_set() {
        use crate::scene::parametric_constraints::ParametricScope;
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,5 4,5"}));
        let handles: Vec<String> =
            request(&mut app, json!({"op":"query","type":"Line"}))["entities"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entity| entity["handle"].as_str().unwrap().to_string())
                .collect();
        assert_eq!(handles.len(), 2);
        let constraints = |app: &OpenCADStudio| {
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map_or(0, |set| set.constraints.len())
        };
        let line_length = |app: &OpenCADStudio, handle: &str| {
            let codec::EntityType::Line(line) = app.tabs[app.active_tab]
                .scene
                .document
                .get_entity(codec::Handle::new(u64::from_str_radix(handle, 16).unwrap()))
                .unwrap()
            else {
                panic!("expected a line");
            };
            (line.end - line.start).length()
        };

        let selected = request(
            &mut app,
            json!({"op":"select","handles":[&handles[0],&handles[1]]}),
        );
        assert_eq!(selected["ok"], true, "{selected}");
        let run = request(&mut app, json!({"op":"run","cmd":"GEOMCONSTRAINT E"}));
        assert_eq!(run["ok"], true, "{run}");
        assert_eq!(
            constraints(&app),
            1,
            "the live set makes the constraint; history: {:?}",
            app.command_line
                .history
                .iter()
                .map(|e| e.text.clone())
                .collect::<Vec<_>>()
        );
        assert!(
            (line_length(&app, &handles[1]) - 10.0).abs() < 1e-6,
            "the second object takes the first's length"
        );

        request(&mut app, json!({"op":"select","clear":true}));
        let empty = request(&mut app, json!({"op":"run","cmd":"ECONSTRAINT"}));
        assert_eq!(empty["status"], "waiting_input", "{empty}");
        assert!(
            empty["state"]["command"]["prompt"]
                .as_str()
                .unwrap_or("")
                .contains("Select first object"),
            "{empty}"
        );
    }

    /// `geometry_revision` is the staleness signal for drawing content: a
    /// selection, a cleared selection, an empty create or starting a command
    /// must leave it alone, or a caller cannot tell "the operation moved
    /// something" from "the operation was accepted". `REGEN` is the one
    /// command that moves the underlying epoch on purpose — regenerating the
    /// render caches is its whole contract — so it is pinned as the named
    /// exception rather than silently absorbed.
    #[test]
    fn geometry_revision_moves_only_when_the_drawing_changes() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        let handle = request(&mut app, json!({"op":"query","type":"Line"}))["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_string();
        let revision = |app: &OpenCADStudio| app.tabs[app.active_tab].scene.geometry_epoch;
        let mut moved: Vec<(&str, u64)> = Vec::new();
        let mut check = |app: &mut OpenCADStudio, label: &'static str, before: u64| {
            let after = revision(app);
            if after != before {
                moved.push((label, after));
            }
            after
        };

        let mut rev = revision(&app);
        request(&mut app, json!({"op":"select","handles":[&handle]}));
        rev = check(&mut app, "read-only select", rev);
        request(&mut app, json!({"op":"select","handles":[&handle]}));
        rev = check(&mut app, "repeated select", rev);
        request(&mut app, json!({"op":"select","clear":true}));
        rev = check(&mut app, "clearing the selection", rev);
        request(&mut app, json!({"op":"run","cmd":"REDRAW"}));
        rev = check(&mut app, "REDRAW", rev);
        request(&mut app, json!({"op":"entities_create","entities":[]}));
        rev = check(&mut app, "an empty create", rev);
        let started = request(&mut app, json!({"op":"start","cmd":"RECTANG"}));
        assert_eq!(started["status"], "waiting_input", "{started}");
        rev = check(&mut app, "starting a command", rev);
        request(&mut app, json!({"op":"input","kind":"enter"}));
        assert!(moved.is_empty(), "geometry_revision moved for: {moved:?}");

        request(&mut app, json!({"op":"run","cmd":"REGEN"}));
        assert_ne!(
            revision(&app),
            rev,
            "REGEN regenerates the render epoch by its own contract"
        );
    }

    /// `changes` names what the operation altered. A constraint whose
    /// relation already holds leaves every referenced entity byte-identical,
    /// and naming those "Modified" is what made the audit read a phantom
    /// move out of `SYCONSTRAINT` and `TCONSTRAINT`; a real solve must still
    /// be named.
    #[test]
    fn changes_names_only_entities_whose_geometry_moved() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        let flat = request(&mut app, json!({"op":"query","type":"Line"}))["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_string();
        request(&mut app, json!({"op":"select","handles":[&flat]}));
        let satisfied = request(&mut app, json!({"op":"run","cmd":"GCHORIZONTAL"}));
        assert_eq!(satisfied["ok"], true, "{satisfied}");
        assert_eq!(
            satisfied["changes"],
            json!([]),
            "the line was already horizontal, so nothing moved: {satisfied}"
        );

        request(&mut app, json!({"op":"select","clear":true}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,5 4,9"}));
        let sloped = request(&mut app, json!({"op":"query","type":"Line"}))["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entity| entity["handle"].as_str().unwrap().to_string())
            .find(|handle| handle != &flat)
            .unwrap();
        request(&mut app, json!({"op":"select","handles":[&sloped]}));
        let solved = request(&mut app, json!({"op":"run","cmd":"GCHORIZONTAL"}));
        assert_eq!(solved["ok"], true, "{solved}");
        assert!(
            solved["changes"].as_array().is_some_and(|changes| changes
                .iter()
                .any(|change| change["handle"] == sloped.as_str()
                    && change["kind"] == "Modified")),
            "a line the solver levelled is named: {solved}"
        );
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
                matches!(entity, codec::EntityType::Solid3D(_)).then(|| entity.common().handle)
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
            Some(codec::objects::SolidHistoryOperation::Brep(_))
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
    fn external_change_modal_actions_are_automatable() {
        let mut app = OpenCADStudio::new_for_test();
        let names = crate::app::automation_action_names();
        for name in [
            "external_change_reload",
            "external_change_overwrite",
            "external_change_save_as",
            "external_change_cancel",
        ] {
            assert!(names.contains(&name), "{name} should be listed");
        }
        // No conflict pending: the action is a harmless no-op, not an error.
        let reply = request(
            &mut app,
            json!({
                "op":"action",
                "request_id":"external-action-noop",
                "name":"external_change_reload"
            }),
        );
        assert_eq!(reply["status"], "completed", "{reply}");
    }

    fn recoverable_open_failure(app: &mut OpenCADStudio) {
        app.opening = Some(crate::app::OpenProgress {
            id: 7,
            name: "damaged.dwg".to_string(),
            source_path: None,
            size_bytes: 0,
            state: std::sync::Arc::new(crate::io::OpenProgressState::new(
                crate::app::OPEN_PHASE_READING,
            )),
            started: std::time::Instant::now(),
            recovery_error: None,
            recovery_read_stats: None,
            fingerprint: None,
        });
        let _ = app.update(Message::FileOpened(
            7,
            Err(crate::io::OpenLoadError {
                message: "Invalid record at handle 0x5B87, type_code=37".to_string(),
                source_sha256: None,
                read_stats: None,
                recovery_available: true,
            }),
        ));
    }

    #[test]
    fn controlled_open_fails_instead_of_waiting_on_the_recovery_prompt() {
        let mut app = OpenCADStudio::new_for_test();
        let errors = app.command_line.error_revision;
        app.control.pending = Some(Operation {
            id: "open-damaged".to_string(),
            request: json!({"op":"open","path":"damaged.dwg"}),
            pending: 0,
            error_revision: errors,
            document_id: None,
            origin_document: app.tabs[app.active_tab].id,
            geometry_revision: 0,
            result: json!({}),
            cancelled: false,
        });
        recoverable_open_failure(&mut app);
        assert!(app.opening.is_none());
        assert_ne!(
            app.active_modal,
            Some(crate::app::ModalKind::RecoveryPrompt)
        );
        app.control_settle();
        let reply = app
            .control_request(json!({"op":"operation","request_id":"open-damaged"}))
            .0;
        assert_eq!(reply["status"], "failed", "{reply}");
        assert!(app.control.pending.is_none());
    }

    #[test]
    fn recovery_prompt_is_answerable_through_actions() {
        let mut app = OpenCADStudio::new_for_test();
        assert!(crate::app::automation_action_names().contains(&"recovery_accept"));
        let reply = request(
            &mut app,
            json!({"op":"action","request_id":"accept-none","name":"recovery_accept"}),
        );
        assert_eq!(reply["status"], "completed", "{reply}");

        recoverable_open_failure(&mut app);
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::RecoveryPrompt)
        );
        let reply = request(
            &mut app,
            json!({"op":"action","request_id":"decline","name":"close_modal"}),
        );
        assert_eq!(reply["status"], "completed", "{reply}");
        assert!(app.opening.is_none());
        assert_ne!(
            app.active_modal,
            Some(crate::app::ModalKind::RecoveryPrompt)
        );
    }

    #[test]
    fn close_confirmation_prompt_is_automatable() {
        use crate::app::ModalKind;
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["status"], "completed");
        assert_eq!(
            request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}))["status"],
            "completed"
        );
        let names = crate::app::automation_action_names();
        for name in ["dialog_ok", "dialog_discard", "dialog_cancel"] {
            assert!(names.contains(&name), "{name} should be listed");
        }
        // Cancel dismisses the prompt and leaves the drawing open.
        let closed = request(&mut app, json!({"op":"action","name":"close_document"}));
        assert_eq!(closed["status"], "waiting_input", "{closed}");
        assert_eq!(app.active_modal, Some(ModalKind::Unsaved));
        let cancelled = request(&mut app, json!({"op":"action","name":"dialog_cancel"}));
        assert_eq!(cancelled["ok"], true, "{cancelled}");
        assert_ne!(app.active_modal, Some(ModalKind::Unsaved));
        assert_eq!(request(&mut app, json!({"op":"entities"}))["total"], 1);
        // Discard closes the drawing without saving it.
        let closed = request(&mut app, json!({"op":"action","name":"close_document"}));
        assert_eq!(closed["status"], "waiting_input", "{closed}");
        assert_eq!(app.active_modal, Some(ModalKind::Unsaved));
        let discarded = request(&mut app, json!({"op":"action","name":"dialog_discard"}));
        assert_eq!(discarded["ok"], true, "{discarded}");
        assert_ne!(app.active_modal, Some(ModalKind::Unsaved));
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(request(&mut app, json!({"op":"entities"}))["total"], 0);
        // Save closes the drawing through the save flow rather than refusing.
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["status"], "completed");
        let path = std::env::temp_dir().join(format!("ocs-close-{}.dwg", session_id()));
        assert_eq!(
            request(&mut app, json!({"op":"save","path":path}))["ok"],
            true
        );
        let drawn = request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        assert_eq!(drawn["ok"], true, "{drawn}");
        let closed = request(&mut app, json!({"op":"action","name":"close_document"}));
        assert_eq!(closed["status"], "waiting_input", "{closed}");
        assert_eq!(app.active_modal, Some(ModalKind::Unsaved));
        let saved = request(&mut app, json!({"op":"action","name":"dialog_ok"}));
        assert_ne!(saved["code"], "no_dialog", "{saved}");
        assert_ne!(app.active_modal, Some(ModalKind::Unsaved));
        assert!(path.is_file(), "the closed drawing was written to disk");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pending_request_id_reuse_rejects_changed_payload() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        assert_eq!(
            request(&mut app, json!({"op":"new"}))["status"],
            "completed"
        );

        let pending = request(
            &mut app,
            json!({
                "op":"user_select",
                "request_id":"pending-reuse",
                "prompt":"pick the first object"
            }),
        );
        assert_eq!(pending["status"], "running", "{pending}");

        let changed = request(
            &mut app,
            json!({
                "op":"user_select",
                "request_id":"pending-reuse",
                "prompt":"pick a different object"
            }),
        );
        assert_eq!(changed["code"], "request_id_reused", "{changed}");
        assert!(
            changed["error"].as_str().unwrap().contains("pending-reuse"),
            "{changed}"
        );

        let cancelled = request(
            &mut app,
            json!({"op":"cancel","request_id":"cancel-pending-reuse"}),
        );
        assert_eq!(cancelled["status"], "cancelled", "{cancelled}");
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
    fn cancel_ends_the_started_command() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let started = request(&mut app, json!({"op":"start","cmd":"PURGE"}));
        assert_eq!(started["status"], "waiting_input", "{started}");
        // A dialog left open by the load (missing fonts) takes the first Esc.
        app.active_modal = Some(crate::app::ModalKind::MissingFonts);
        let cancelled = request(&mut app, json!({"op":"cancel"}));
        assert_eq!(cancelled["state"]["command"], Value::Null, "{cancelled}");
        let next = request(&mut app, json!({"op":"start","cmd":"LINE"}));
        assert_eq!(next["status"], "waiting_input", "{next}");
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
    fn open_with_missing_xrefs_succeeds_and_lists_every_reference() {
        let mut doc = codec::Document::new();
        for name in ["Grid Plan", "Wall Base"] {
            let mut record = codec::tables::BlockRecord::new(name);
            record.flags.is_xref = true;
            record.xref_path = format!("missing-dir/{name}.dwg");
            doc.block_records.add(record).unwrap();
        }
        let bytes = crate::io::save_to_bytes(&doc, "dwg", doc.version).unwrap();
        let path = std::env::temp_dir().join(format!("ocs-xref-open-{}.dwg", session_id()));
        std::fs::write(&path, bytes).unwrap();
        let mut app = OpenCADStudio::new_for_test();
        let opened = request(&mut app, json!({"op":"open","path":path}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(opened["ok"], true, "{opened}");
        let xrefs = opened["state"]["xrefs"].as_array().unwrap();
        assert_eq!(xrefs.len(), 2, "{opened}");
        assert!(xrefs.iter().all(|x| x["status"] == "not_found"), "{opened}");
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
    fn font_register_validates_the_request() {
        let ok = font_register_request(
            &json!({"file_name":"C:\\Fonts\\Brand.ttf","data_base64":"AAEC"}),
        );
        assert_eq!(ok, Ok(("Brand.ttf".to_string(), vec![0, 1, 2])));
        for req in [
            json!({"data_base64":"AAEC"}),
            json!({"file_name":"fonts/","data_base64":"AAEC"}),
            json!({"file_name":"a.ttf","data_base64":""}),
            json!({"file_name":"a.ttf","data_base64":"not base64!"}),
        ] {
            assert_eq!(
                font_register_request(&req).unwrap_err()["code"],
                "invalid_request",
                "{req}"
            );
        }
    }

    #[test]
    fn font_register_makes_a_style_file_resolve() {
        use base64::Engine as _;
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/web/fonts/greek.ttf"))
            .expect("bundled web font");
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let reply = request(
            &mut app,
            json!({
                "op":"font_register",
                "file_name":"Control-Registered.ttf",
                "data_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
            }),
        );
        assert_eq!(reply["ok"], true, "{reply}");
        let family = reply["result"]["family"]
            .as_str()
            .expect("family")
            .to_string();
        assert!(reply["result"]["faces"].as_u64().unwrap() >= 1, "{reply}");
        assert_eq!(
            crate::scene::text::sysfont::family_for_reference("control-registered.ttf"),
            Some(family)
        );

        let refused = request(
            &mut app,
            json!({"op":"font_register","file_name":"x.ttf","data_base64":"AAEC"}),
        );
        assert_eq!(refused["code"], "font_invalid", "{refused}");
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
                codec::EntityType::Circle(c) => Some(c.center.y),
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
                .filter(|e| matches!(e, codec::EntityType::Ole2Frame(_)))
                .count()
        };
        assert_eq!(ole_count(&app), 1);

        // The entity must be queryable like any other and undoable as one step.
        let queried = app.automation_op(
            json!({"op":"entities","type":"OLE2FRAME"})
                .to_string()
                .as_str(),
        );
        assert_eq!(queried["total"], 1, "{queried}");
        request(&mut app, json!({"op":"undo"}));
        assert_eq!(ole_count(&app), 0);
        let _ = std::fs::remove_file(path);
        let _ = handle;
    }

    #[test]
    fn embed_image_calibrates_from_source_and_target_points() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        let img = image::RgbaImage::from_pixel(100, 100, image::Rgba([12, 12, 12, 255]));
        let path = std::env::temp_dir().join(format!("ocs-embed-calib-{}.png", session_id()));
        img.save(&path).unwrap();

        // 2 points on image: bottom-left (0, 100) and bottom-right (100, 100)
        // Target CAD points: (10.0, 20.0) and (210.0, 20.0) -> scale = 200/100 = 2.0
        let response = request(
            &mut app,
            json!({
                "op": "embed_image",
                "path": path.to_string_lossy(),
                "linked": true,
                "source_points": [[0, 100], [100, 100]],
                "target_points": [[10.0, 20.0], [210.0, 20.0]]
            }),
        );
        assert_eq!(response["status"], "completed", "{response}");
        assert_eq!(response["result"]["kind"], "RasterImage");
        assert_eq!(response["result"]["width"], 200.0);
        assert_eq!(response["result"]["height"], 200.0);
        let at = response["result"]["at"].as_array().unwrap();
        assert_eq!(at[0], 10.0);
        assert_eq!(at[1], 20.0);
        let _ = std::fs::remove_file(path);
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
            .select_entity(codec::Handle::new(value), false);
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
        assert_eq!(
            request(&mut app, json!({"op":"new"}))["status"],
            "completed"
        );
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
            tab.scene.selection.borrow_mut().view.vp_size = (1920.0, 1080.0);
            tab.scene.sync_tiles_from_panes(1920.0, 1080.0);
            tab.scene.fit_all(); // bring the line into the pane so snaps can hit
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
            sx as f32, sy as f32,
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
            tab.scene.selection.borrow_mut().view.vp_size = (1920.0, 1080.0);
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
            sx as f32, sy as f32,
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
        app.tabs[i]
            .scene
            .select_entity(codec::Handle::new(value), false);
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
        app.automation_op(&format!(
            r#"{{"op":"select","handles":["{circle_handle}"]}}"#
        ));
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
                ("THIN", codec::types::LineWeight::Value(13)),
                ("THICK", codec::types::LineWeight::Value(50)),
            ] {
                let mut layer = codec::tables::Layer::new(name);
                layer.line_weight = weight;
                let _ = scene.document.layers.add(layer);
            }
            for (name, origin) in [("THIN", 0.0), ("THICK", 3000.0)] {
                let mut line = codec::entities::Line::new();
                line.common.layer = name.to_string();
                line.start = codec::types::Vector3::new(origin, origin, 0.0);
                line.end = codec::types::Vector3::new(origin + 3000.0, origin + 2000.0, 0.0);
                scene.add_entity(codec::EntityType::Line(line));
            }
        }

        let pdf = std::env::temp_dir().join(format!("ocs-plot-lw-{}.pdf", std::process::id()));
        let response = request(
            &mut app,
            json!({"op":"plot","path":pdf.to_string_lossy(),"plot_style":"monochrome.ctb"}),
        );
        assert_eq!(response["status"], "completed", "{response}");

        // The plotted PDF carries two distinct pen widths — the per-layer
        // 0.13 mm / 0.50 mm hierarchy survives the plot style at its true
        // ratio; the screen's 1 px floor does not reach the plot (#1749).
        let text =
            crate::io::pdf_export::pdf_stream_text(&std::fs::read(&pdf).expect("pdf written"));
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
        assert_eq!(
            widths.len(),
            2,
            "two layer weights in the plotted PDF: {widths:?}"
        );
        assert!(
            (widths[1] / widths[0] - 50.0 / 13.0).abs() < 0.15,
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
            let mut frame = codec::tables::Layer::new("FRAME-CYAN");
            frame.color = codec::types::Color::Index(4);
            let _ = scene.document.layers.add(frame);
            let mut frame_line = codec::entities::Line::new();
            frame_line.common.layer = "FRAME-CYAN".to_string();
            frame_line.start = codec::types::Vector3::new(0.0, 0.0, 0.0);
            frame_line.end = codec::types::Vector3::new(3000.0, 0.0, 0.0);
            scene.add_entity(codec::EntityType::Line(frame_line));

            // An explicit red ACI 1 and a true-color green: index must map
            // through the CTB, the true color must stay RGB (aci 0).
            let mut red = codec::entities::Line::new();
            red.common.color = codec::types::Color::Index(1);
            red.start = codec::types::Vector3::new(0.0, 1000.0, 0.0);
            red.end = codec::types::Vector3::new(3000.0, 1000.0, 0.0);
            scene.add_entity(codec::EntityType::Line(red));

            let mut true_color = codec::entities::Line::new();
            true_color.common.color = codec::types::Color::from_true_color_value(0x0000FF00);
            true_color.start = codec::types::Vector3::new(0.0, 2000.0, 0.0);
            true_color.end = codec::types::Vector3::new(3000.0, 2000.0, 0.0);
            scene.add_entity(codec::EntityType::Line(true_color));
        }

        let pdf = std::env::temp_dir().join(format!("ocs-plot-aci-{}.pdf", std::process::id()));
        let response = request(
            &mut app,
            json!({"op":"plot","path":pdf.to_string_lossy(),"plot_style":"monochrome.ctb"}),
        );
        assert_eq!(response["status"], "completed", "{response}");

        let text =
            crate::io::pdf_export::pdf_stream_text(&std::fs::read(&pdf).expect("pdf written"));
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
        let plain =
            std::env::temp_dir().join(format!("ocs-plot-aci-plain-{}.pdf", std::process::id()));
        let response = request(
            &mut app,
            json!({"op":"plot","path":plain.to_string_lossy()}),
        );
        assert_eq!(response["status"], "completed", "{response}");
        let text =
            crate::io::pdf_export::pdf_stream_text(&std::fs::read(&plain).expect("pdf written"));
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
            color_ops.iter().any(|c| near(c, [0.0, 1.0, 1.0])),
            "unstyled cyan frame keeps its colour on paper (#1926): {color_ops:?}"
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

    /// An unknown `entities_transform` action must come back as a clean
    /// validation error. Pre-fix it reached
    /// `unreachable!("validated by transform()")` (entities.rs:476) and
    /// killed the whole process — over REST, MCP and `--serve` alike —
    /// because validation lived inside the `transform` closure, which the
    /// dispatch match only invokes from some of its arms.
    #[test]
    fn entities_transform_rejects_unknown_actions_without_panicking() {
        let mut app = OpenCADStudio::new_for_test();
        request(&mut app, json!({"op":"new"}));
        request(&mut app, json!({"op":"run","cmd":"LINE 0,0 10,0"}));
        let lines = app.control_request(json!({"op":"query","type":"Line"})).0;
        let handle = lines["entities"][0]["handle"].as_str().unwrap().to_string();

        // Not one of the six whitelisted actions.
        let rejected = request(
            &mut app,
            json!({
                "op":"entities_transform",
                "handles":[handle],
                "action":"explode"
            }),
        );
        assert_eq!(rejected["code"], "invalid_action", "{rejected}");

        // Whitespace-only is unknown too — not a panic.
        let blank = request(
            &mut app,
            json!({
                "op":"entities_transform",
                "handles":[handle],
                "action":"   "
            }),
        );
        assert_eq!(blank["code"], "invalid_action", "{blank}");

        // A valid action is matched case- and whitespace-insensitively.
        let moved = request(
            &mut app,
            json!({
                "op":"entities_transform",
                "handles":[handle],
                "action":" move ",
                "vector":[5,0,0]
            }),
        );
        assert_eq!(moved["ok"], true, "{moved}");
    }
}
