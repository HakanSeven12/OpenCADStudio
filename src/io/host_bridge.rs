//! Vendor-neutral `window.postMessage` host bridge activated by `?embed=1`.
//!
//! Pure query-parsing, CAD save-format filtering, `saveId` correlation queue,
//! and display-label helpers live at the top of the module so they compile and
//! run in unit tests on native CI hosts. The browser listener and transfer
//! glue below compile only for `wasm32-unknown-unknown`.

use std::collections::VecDeque;
use std::path::Path;

#[allow(dead_code)] // bounded queue capacity exercised by wasm32 emitter and native unit tests
const MAX_PENDING_SAVES: usize = 16;

/// Metadata retained for each outbound `ocs:save` until the host acknowledges
/// completion via `ocs:saved`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub struct PendingSave {
    pub save_id: String,
    pub filename: String,
    pub tab_id: u64,
    pub geometry_epoch: u64,
    pub edit_revision: u64,
    pub camera_generation: u64,
}

/// Return `true` when query string `search` contains `key=expected`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn search_has_flag(search: &str, key: &str, expected: &str) -> bool {
    url::form_urlencoded::parse(search.trim_start_matches('?').as_bytes())
        .any(|(k, v)| k == key && v == expected)
}

/// Classify a save filename as an editable CAD format (`"dxf"` / `"dwg"`).
///
/// Non-CAD exports (`pdf`, `png`, `svg`, `csv`, `zip`, etc.) return `None` so
/// standalone browser downloads keep working when running in `?embed=1` mode.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn cad_save_format(filename: &str) -> Option<&'static str> {
    let ext = Path::new(filename).extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("dxf") {
        Some("dxf")
    } else if ext.eq_ignore_ascii_case("dwg") {
        Some("dwg")
    } else {
        None
    }
}

/// Resolve the basename label shown in `Saved: {filename}` from an `ocs:saved`
/// ack payload, falling back to the pending save's original filename.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn saved_label_from_ack(
    ack_name: Option<&str>,
    pending_filename: Option<&str>,
) -> Option<String> {
    ack_name.or(pending_filename).and_then(|raw| {
        Path::new(raw)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .or_else(|| (!raw.is_empty()).then(|| raw.to_string()))
    })
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn push_pending_save_in(queue: &mut VecDeque<PendingSave>, entry: PendingSave) {
    if queue.len() >= MAX_PENDING_SAVES {
        queue.pop_front();
    }
    queue.push_back(entry);
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn take_pending_save_in(
    queue: &mut VecDeque<PendingSave>,
    save_id: Option<&str>,
) -> Option<PendingSave> {
    let id = save_id?;
    let pos = queue.iter().position(|pending| pending.save_id == id)?;
    queue.remove(pos)
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{
        cad_save_format, push_pending_save_in, search_has_flag, take_pending_save_in, PendingSave,
    };
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
    use std::sync::{Mutex, OnceLock};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    #[derive(Debug, Clone)]
    pub enum HostCommand {
        Open {
            name: String,
            bytes: Vec<u8>,
            read_only: bool,
        },
        RequestSave {
            format: Option<String>,
        },
        SaveAck {
            save_id: Option<String>,
            ok: bool,
            name: Option<String>,
            error: Option<String>,
        },
    }

    static EMBED_MODE: OnceLock<bool> = OnceLock::new();
    static AUTOMATION_ALLOWED: OnceLock<bool> = OnceLock::new();
    static INSTALLED: OnceLock<()> = OnceLock::new();
    static COMMAND_QUEUE: Mutex<VecDeque<HostCommand>> = Mutex::new(VecDeque::new());
    static PENDING_SAVES: Mutex<VecDeque<PendingSave>> = Mutex::new(VecDeque::new());
    static NEXT_SAVE_ID: AtomicU64 = AtomicU64::new(1);
    static DIRTY_STATE: AtomicU8 = AtomicU8::new(0);

    fn has_query_flag(key: &str, expected: &str) -> bool {
        web_sys::window()
            .and_then(|window| window.location().search().ok())
            .map(|search| search_has_flag(&search, key, expected))
            .unwrap_or(false)
    }

    pub fn is_embed_mode() -> bool {
        *EMBED_MODE.get_or_init(|| has_query_flag("embed", "1"))
    }

    pub fn is_automation_allowed() -> bool {
        *AUTOMATION_ALLOWED.get_or_init(|| !is_embed_mode() || has_query_flag("automation", "1"))
    }

    pub fn holds_dirty_for_ack(filename: &str) -> bool {
        is_embed_mode() && cad_save_format(filename).is_some()
    }

    fn str_prop(obj: &JsValue, key: &str) -> Option<String> {
        js_sys::Reflect::get(obj, &key.into()).ok()?.as_string()
    }

    fn bool_prop(obj: &JsValue, key: &str) -> Option<bool> {
        js_sys::Reflect::get(obj, &key.into()).ok()?.as_bool()
    }

    pub fn install() {
        if !is_embed_mode() {
            return;
        }
        INSTALLED.get_or_init(|| {
            let Some(window) = web_sys::window() else {
                return;
            };
            let expected_origin = window.location().origin().unwrap_or_default();
            let closure = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
                move |event: web_sys::MessageEvent| {
                    if event.origin() != expected_origin {
                        return;
                    }
                    let Some(win) = web_sys::window() else {
                        return;
                    };
                    let Ok(Some(parent)) = win.parent() else {
                        return;
                    };
                    let src_win =
                        js_sys::Reflect::get(&event, &"source".into()).unwrap_or(JsValue::NULL);
                    if src_win != JsValue::from(parent) {
                        return;
                    }
                    let data = event.data();
                    if str_prop(&data, "source").as_deref() != Some("opencadstudio-host") {
                        return;
                    }
                    let Some(msg_type) = str_prop(&data, "type") else {
                        return;
                    };
                    let cmd = match msg_type.as_str() {
                        "ocs:open" => {
                            let name = str_prop(&data, "name")
                                .unwrap_or_else(|| "drawing.dxf".to_string());
                            // Fail closed: omitted `readOnly` stays read-only.
                            let read_only = bool_prop(&data, "readOnly").unwrap_or(true);
                            let buf = js_sys::Reflect::get(&data, &"buffer".into())
                                .unwrap_or(JsValue::UNDEFINED);
                            let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                            Some(HostCommand::Open {
                                name,
                                bytes,
                                read_only,
                            })
                        }
                        "ocs:request-save" => Some(HostCommand::RequestSave {
                            format: str_prop(&data, "format"),
                        }),
                        "ocs:saved" => Some(HostCommand::SaveAck {
                            save_id: str_prop(&data, "saveId"),
                            ok: bool_prop(&data, "ok").unwrap_or(false),
                            name: str_prop(&data, "name"),
                            error: str_prop(&data, "error"),
                        }),
                        _ => None,
                    };
                    if let Some(cmd) = cmd {
                        COMMAND_QUEUE
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push_back(cmd);
                    }
                },
            );
            let _ = window
                .add_event_listener_with_callback("message", closure.as_ref().unchecked_ref());
            closure.forget();

            let caps = js_sys::Array::new();
            for cap in ["dwg", "dxf", "readOnly", "save", "saveId", "dirty"] {
                caps.push(&JsValue::from_str(cap));
            }
            post_to_host(
                "ocs:ready",
                &[
                    ("version", JsValue::from_str(env!("OCS_APP_VERSION"))),
                    ("capabilities", caps.into()),
                ],
                None,
            );
        });
    }

    pub fn take_command() -> Option<HostCommand> {
        COMMAND_QUEUE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    pub fn requeue_front(cmd: HostCommand) {
        COMMAND_QUEUE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_front(cmd);
    }

    pub fn clear_pending_saves() {
        PENDING_SAVES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    pub fn take_pending_save(save_id: Option<&str>) -> Option<PendingSave> {
        let mut queue = PENDING_SAVES.lock().unwrap_or_else(|e| e.into_inner());
        take_pending_save_in(&mut queue, save_id)
    }

    pub fn emit_loaded(name: &str) {
        post_to_host("ocs:loaded", &[("name", JsValue::from_str(name))], None);
    }

    pub fn emit_dirty(dirty: bool) {
        let next = if dirty { 2 } else { 1 };
        if DIRTY_STATE.swap(next, Ordering::Relaxed) == next {
            return;
        }
        post_to_host("ocs:dirty", &[("dirty", JsValue::from_bool(dirty))], None);
    }

    pub fn emit_error(code: &str, message: &str) {
        post_to_host(
            "ocs:error",
            &[
                ("code", JsValue::from_str(code)),
                ("message", JsValue::from_str(message)),
            ],
            None,
        );
    }

    pub fn emit_save(
        name: &str,
        format: &str,
        bytes: &[u8],
        tab_id: u64,
        geometry_epoch: u64,
        edit_revision: u64,
        camera_generation: u64,
    ) -> String {
        let save_id = format!("save-{}", NEXT_SAVE_ID.fetch_add(1, Ordering::Relaxed));
        {
            let mut queue = PENDING_SAVES.lock().unwrap_or_else(|e| e.into_inner());
            push_pending_save_in(
                &mut queue,
                PendingSave {
                    save_id: save_id.clone(),
                    filename: name.to_string(),
                    tab_id,
                    geometry_epoch,
                    edit_revision,
                    camera_generation,
                },
            );
        }
        let arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
        arr.copy_from(bytes);
        let buffer = arr.buffer();
        post_to_host(
            "ocs:save",
            &[
                ("name", JsValue::from_str(name)),
                ("format", JsValue::from_str(format)),
                ("saveId", JsValue::from_str(&save_id)),
                ("buffer", buffer.clone().into()),
            ],
            Some(&buffer),
        );
        save_id
    }

    fn post_to_host(
        msg_type: &str,
        fields: &[(&str, JsValue)],
        transfer_buf: Option<&js_sys::ArrayBuffer>,
    ) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(Some(parent)) = window.parent() else {
            return;
        };
        let Ok(origin) = window.location().origin() else {
            return;
        };
        let obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&obj, &"source".into(), &"opencadstudio-embed".into());
        let _ = js_sys::Reflect::set(&obj, &"type".into(), &msg_type.into());
        for (key, value) in fields {
            let _ = js_sys::Reflect::set(&obj, &(*key).into(), value);
        }
        if let Some(buf) = transfer_buf {
            let transfer = js_sys::Array::of1(buf);
            let _ = parent.post_message_with_transfer(&obj, &origin, &transfer);
        } else {
            let _ = parent.post_message(&obj, &origin);
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(test)]
mod tests {
    use super::{
        cad_save_format, push_pending_save_in, saved_label_from_ack, search_has_flag,
        take_pending_save_in, PendingSave,
    };
    use std::collections::VecDeque;

    fn sample_save(id: &str, name: &str) -> PendingSave {
        PendingSave {
            save_id: id.to_string(),
            filename: name.to_string(),
            tab_id: 7,
            geometry_epoch: 3,
            edit_revision: 11,
            camera_generation: 2,
        }
    }

    #[test]
    fn search_has_flag_matches_exact_query_pairs() {
        assert!(search_has_flag("?embed=1", "embed", "1"));
        assert!(search_has_flag(
            "foo=bar&embed=1&automation=1",
            "embed",
            "1"
        ));
        assert!(search_has_flag("?embed=1&automation=1", "automation", "1"));
        assert!(!search_has_flag("?embed=0", "embed", "1"));
        assert!(!search_has_flag("?embedded=1", "embed", "1"));
        assert!(!search_has_flag("", "embed", "1"));
    }

    #[test]
    fn cad_save_format_routes_only_dwg_and_dxf() {
        assert_eq!(cad_save_format("plan.dxf"), Some("dxf"));
        assert_eq!(cad_save_format("PLAN.DWG"), Some("dwg"));
        assert_eq!(cad_save_format("dir/sheet.Dxf"), Some("dxf"));
        assert_eq!(cad_save_format("sheet.pdf"), None);
        assert_eq!(cad_save_format("preview.png"), None);
        assert_eq!(cad_save_format("vector.svg"), None);
        assert_eq!(cad_save_format("schedule.csv"), None);
        assert_eq!(cad_save_format("package.zip"), None);
        assert_eq!(cad_save_format("noext"), None);
    }

    #[test]
    fn pending_save_queue_matches_out_of_order_and_bounds_capacity() {
        let mut queue = VecDeque::new();
        for idx in 1..=18 {
            push_pending_save_in(&mut queue, sample_save(&format!("save-{idx}"), "a.dxf"));
        }
        assert_eq!(queue.len(), 16);
        assert!(take_pending_save_in(&mut queue, Some("save-1")).is_none());
        assert!(take_pending_save_in(&mut queue, Some("save-2")).is_none());
        assert_eq!(
            take_pending_save_in(&mut queue, Some("save-10")).map(|p| p.save_id),
            Some("save-10".to_string())
        );
        assert!(take_pending_save_in(&mut queue, Some("save-10")).is_none());
        assert!(take_pending_save_in(&mut queue, None).is_none());
    }

    #[test]
    fn saved_label_from_ack_strips_host_folders_and_falls_back() {
        assert_eq!(
            saved_label_from_ack(Some("CAD/site (copy).dxf"), Some("site.dxf")),
            Some("site (copy).dxf".to_string())
        );
        assert_eq!(
            saved_label_from_ack(None, Some("site.dxf")),
            Some("site.dxf".to_string())
        );
        assert_eq!(saved_label_from_ack(None, None), None);
    }
}
